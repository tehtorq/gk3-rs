//! The people and props in a room: loading them from the location file,
//! animating them for scripts and idle behaviour, walking them, and handing
//! their vertices to the renderer.

use std::collections::HashMap;
use std::sync::Arc;

use gk3_barn::Library;
use gk3_formats::act::{FRAMES_PER_SECOND, VertexAnimation};
use gk3_formats::anm::Animation;
use gk3_formats::image::{self, Image};
use gk3_formats::ini::{Document, Line};
use gk3_formats::model::Model;
use gk3_game::character::Character;
use gk3_game::gas_player::{GasPlayer, Request};
use gk3_game::placement;
use gk3_game::pose::Pose;
use gk3_render::{GpuModel, ModelLight, ModelVertex, Renderer, SubmeshDesc};
use glam::{Mat3, Mat4, Vec3};

use crate::faces::{Compositor, Expression, FaceConfig};
use gk3_formats::anm::FaceChange;

/// The vertex animations an `.ANM` starts, by the frame each begins on.
type Clips = Vec<(u32, Arc<VertexAnimation>)>;

/// An animation in progress: the clips it starts and when.
struct Playing {
    anim: String,
    clips: Vec<(u32, Arc<VertexAnimation>)>,
    frames: u32,
    looping: bool,
    started: f64,
    /// For a move animation, the actor keeps where the clip leaves them.
    moves: bool,
}

struct Walk {
    path: Vec<[f32; 2]>,
    clip: Option<Arc<VertexAnimation>>,
    started: f64,
    last: f64,
    speed: f32,
    heading: Option<f32>,
}

/// What an actor does with themselves when nothing else is asked of them:
/// stand about, talk, or listen. Each is a GAS script.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fidget {
    Idle = 0,
    Talk = 1,
    Listen = 2,
}

struct Figure {
    name: String,
    /// GAS names for idling, talking and listening, by `Fidget`.
    fidgets: [Option<String>; 3],
    fidget: Option<Fidget>,
    noun: Option<String>,
    model: Model,
    pose: Pose,
    /// Actors are placed by their floor point and facing; props stand at the
    /// room's own coordinates.
    actor: Option<ActorPlacement>,
    world: Mat4,
    gas: Option<GasPlayer>,
    playing: Option<Playing>,
    walk: Option<Walk>,
    waiting_until: f64,
    visible: bool,
    gpu: GpuModel,
    dirty: bool,
    /// The last vertices sent to the GPU, for picking.
    world_vertices: Vec<Vec3>,
    triangles: Vec<[u32; 3]>,
    face: Option<FaceState>,
}

/// A character's face: what it shows and what is due to change.
struct FaceState {
    config: FaceConfig,
    submesh: usize,
    expression: Expression,
    events: Vec<(f64, FaceChange)>,
    next_blink: f64,
    dirty: bool,
    image: Option<Image>,
}

impl FaceState {
    fn apply(&mut self, change: &FaceChange) {
        let code = &self.config.code;
        match change {
            FaceChange::Mouth(shape) => self.expression.mouth = Some(format!("{code}_{shape}")),
            FaceChange::Paint { bitmap, part } => match part {
                'M' => self.expression.mouth = Some(bitmap.clone()),
                'E' => self.expression.eyelids = Some(bitmap.clone()),
                'H' => self.expression.forehead = Some(bitmap.clone()),
                // The eyes are part of the face bitmap and are left alone.
                _ => return,
            },
            FaceChange::Clear(part) => match part {
                'M' => self.expression.mouth = None,
                'E' => self.expression.eyelids = None,
                'H' => self.expression.forehead = None,
                _ => return,
            },
        }
        self.dirty = true;
    }
}

struct ActorPlacement {
    character: Character,
    position: Vec3,
    heading: f32,
}

pub struct Cast {
    figures: Vec<Figure>,
    clips: HashMap<String, Option<Arc<VertexAnimation>>>,
    light: Option<Vec3>,
    pub positions: HashMap<String, (Vec3, f32)>,
    pub warnings: Vec<String>,
    compositor: Compositor,
    rng: u64,
}

/// Units a second for a walk with no clip to measure.
const DEFAULT_WALK_SPEED: f32 = 36.0;

impl Cast {
    pub fn load(
        lib: &Library,
        renderer: &Renderer,
        sif: &Document,
        characters: &HashMap<String, Character>,
        faces: &HashMap<String, FaceConfig>,
    ) -> Self {
        let mut cast = Self {
            compositor: Compositor::default(),
            rng: 0x5DEE_CE66_D1CE_4E5B,
            figures: Vec::new(),
            clips: HashMap::new(),
            light: None,
            positions: positions(sif),
            warnings: Vec::new(),
        };
        // Where the player stands when nothing else says: START, else the
        // first position the file lists.
        let first_position = sif
            .sections("POSITIONS")
            .flat_map(|s| &s.lines)
            .find_map(|l| {
                cast.positions
                    .get(&l.pairs.first()?.key.to_ascii_uppercase())
            })
            .copied();
        cast.light = sif
            .sections("GENERAL")
            .flat_map(|s| &s.lines)
            .find_map(|l| {
                l.pairs
                    .iter()
                    .find(|p| p.key.eq_ignore_ascii_case("globalLight"))
                    .and(l.get("pos"))
            })
            .and_then(crate::room::floats::<3>)
            .map(Vec3::from);

        let mut seen = std::collections::HashSet::new();
        for section in sif.sections("ACTORS") {
            for line in &section.lines {
                let Some(name) = line.get("model") else {
                    continue;
                };
                if !seen.insert(name.to_ascii_uppercase()) {
                    continue;
                }
                let ego = line
                    .pairs
                    .iter()
                    .any(|p| p.is_flag() && p.key.eq_ignore_ascii_case("ego"));
                let spot = line
                    .get("pos")
                    .and_then(|p| cast.positions.get(&p.to_ascii_uppercase()))
                    .copied()
                    .or_else(|| {
                        ego.then(|| cast.positions.get("START").copied().or(first_position))
                            .flatten()
                    });
                let Some(character) = characters.get(&name.to_ascii_uppercase()).cloned() else {
                    cast.warnings
                        .push(format!("{name}: no entry in CHARACTERS.TXT"));
                    continue;
                };
                // Somebody with no spot is placed by the story when it wants
                // them; until then they are not seen.
                let (position, heading) = spot.unwrap_or((Vec3::ZERO, 0.0));
                let placement = ActorPlacement {
                    character,
                    position,
                    heading: heading.to_radians(),
                };
                let gas = line.get("idle").and_then(|g| load_gas(lib, g));
                if let Some(mut figure) = cast.figure(lib, renderer, name, Some(placement), gas) {
                    figure.noun = line.get("noun").map(|n| n.to_ascii_uppercase());
                    figure.fidgets =
                        ["idle", "talk", "listen"].map(|k| line.get(k).map(str::to_string));
                    if let Some(config) = faces.get(&figure.name) {
                        let submesh = figure
                            .model
                            .submeshes()
                            .position(|s| s.texture.eq_ignore_ascii_case(&config.face));
                        figure.face = submesh.map(|submesh| FaceState {
                            config: config.clone(),
                            submesh,
                            expression: Expression::default(),
                            events: Vec::new(),
                            next_blink: 2.0,
                            dirty: true,
                            image: None,
                        });
                    }
                    figure.visible = spot.is_some() && !hidden(line);
                    cast.figures.push(figure);
                }
            }
        }

        for section in sif.sections("MODELS") {
            for line in &section.lines {
                let Some(name) = line.get("model") else {
                    continue;
                };
                let kind = line.get("type").unwrap_or("").to_ascii_lowercase();
                if !matches!(kind.as_str(), "prop" | "gasprop")
                    || !seen.insert(name.to_ascii_uppercase())
                {
                    continue;
                }
                let gas = line.get("gas").and_then(|g| load_gas(lib, g));
                let Some(mut figure) = cast.figure(lib, renderer, name, None, gas) else {
                    continue;
                };
                figure.noun = line.get("noun").map(|n| n.to_ascii_uppercase());
                // Hidden props are loaded anyway: scripts show them.
                figure.visible = !hidden(line);
                // A prop's starting animation sets its first pose.
                if let Some(anim) = line.get("initanim") {
                    for (_, clip) in cast.clips_for(lib, anim, &figure.name) {
                        figure.pose.apply(&clip, 0.0, false);
                    }
                }
                cast.figures.push(figure);
            }
        }
        cast
    }

    fn figure(
        &mut self,
        lib: &Library,
        renderer: &Renderer,
        name: &str,
        actor: Option<ActorPlacement>,
        gas: Option<GasPlayer>,
    ) -> Option<Figure> {
        let model = match lib
            .read(&format!("{name}.MOD"))
            .map_err(|e| e.to_string())
            .and_then(|b| Model::parse(&b).map_err(|e| e.to_string()))
        {
            Ok(m) => m,
            Err(e) => {
                self.warnings.push(format!("{name}: {e}"));
                return None;
            }
        };
        let submeshes: Vec<SubmeshDesc> = model
            .submeshes()
            .map(|s| SubmeshDesc {
                texture: load_texture(lib, &s.texture),
                tint: s.color,
                triangles: &s.triangles,
                vertex_count: s.positions.len(),
            })
            .collect();
        let gpu = renderer.upload_model(&submeshes);
        let mut triangles = Vec::new();
        let mut base = 0u32;
        for sub in model.submeshes() {
            triangles.extend(sub.triangles.iter().map(|t| t.map(|i| base + i as u32)));
            base += sub.positions.len() as u32;
        }
        let pose = Pose::rest(&model);
        let world = match &actor {
            Some(a) => placement::place(&pose, &a.character, a.position, a.heading),
            None => Mat4::IDENTITY,
        };
        let fidget = gas.is_some().then_some(Fidget::Idle);
        Some(Figure {
            name: name.to_ascii_uppercase(),
            fidgets: Default::default(),
            fidget,
            noun: None,
            model,
            pose,
            actor,
            world,
            gas,
            playing: None,
            walk: None,
            waiting_until: 0.0,
            visible: true,
            gpu,
            dirty: true,
            world_vertices: Vec::new(),
            triangles,
            face: None,
        })
    }

    fn clip(&mut self, lib: &Library, name: &str) -> Option<Arc<VertexAnimation>> {
        self.clips
            .entry(name.to_ascii_uppercase())
            .or_insert_with(|| {
                let bytes = lib.read(&format!("{name}.ACT")).ok()?;
                VertexAnimation::parse(&bytes).ok().map(Arc::new)
            })
            .clone()
    }

    /// The vertex animations an `.ANM` starts on one model.
    fn clips_for(
        &mut self,
        lib: &Library,
        anim: &str,
        model: &str,
    ) -> Vec<(u32, Arc<VertexAnimation>)> {
        let Some(anm) = read_anm(lib, anim) else {
            return Vec::new();
        };
        anm.actions
            .iter()
            .filter_map(|a| {
                let clip = self.clip(lib, &a.clip)?;
                clip.model
                    .eq_ignore_ascii_case(model)
                    .then_some((a.frame, clip))
            })
            .collect()
    }

    /// Which figure a script means: by model name or noun.
    pub fn find(&self, name: &str) -> Option<usize> {
        let upper = name.to_ascii_uppercase();
        self.figures
            .iter()
            .position(|f| f.name == upper)
            .or_else(|| {
                self.figures
                    .iter()
                    .position(|f| f.noun.as_deref() == Some(upper.as_str()))
            })
    }

    fn start(
        &mut self,
        index: usize,
        anim: &str,
        clips: Vec<(u32, Arc<VertexAnimation>)>,
        frames: u32,
        how: Start,
    ) {
        let figure = &mut self.figures[index];
        for (start, clip) in &clips {
            if *start == 0 {
                figure.pose.apply(clip, 0.0, false);
            }
        }
        match (how.absolute, &figure.actor) {
            // A set piece plays where it was authored.
            (Some(world), _) => figure.world = world,
            // Otherwise it plays from wherever the actor stands: place the
            // model so its first pose stands on them.
            (None, Some(a)) => {
                figure.world = placement::place(&figure.pose, &a.character, a.position, a.heading)
            }
            (None, None) => {}
        }
        if how.scripted {
            figure.visible = true;
        }
        figure.playing = Some(Playing {
            anim: anim.to_ascii_uppercase(),
            clips,
            frames,
            looping: how.looping,
            started: how.now,
            moves: how.moves || how.absolute.is_some(),
        });
        figure.dirty = true;
    }

    /// Plays an `.ANM` on every model it animates. Returns how long it lasts
    /// in seconds, or `None` if nothing in the room is animated by it.
    pub fn play(
        &mut self,
        lib: &Library,
        anim: &str,
        looping: bool,
        moves: bool,
        now: f64,
    ) -> Option<f32> {
        let anm = read_anm(lib, anim)?;
        let mut by_figure: HashMap<usize, (Clips, Option<[f32; 8]>)> = HashMap::new();
        for action in &anm.actions {
            let Some(clip) = self.clip(lib, &action.clip) else {
                continue;
            };
            let Some(index) = self.find(&clip.model) else {
                continue;
            };
            let entry = by_figure.entry(index).or_default();
            entry.0.push((action.frame, clip));
            entry.1 = entry.1.or(action.placement);
        }
        if by_figure.is_empty() {
            return None;
        }
        let longest = by_figure
            .values()
            .flat_map(|(clips, _)| clips.iter().map(|(f, c)| f + c.frame_count))
            .max()
            .unwrap_or(0);
        let frames = anm.frames.max(longest).max(1);
        for (index, (clips, placement)) in by_figure {
            let absolute = placement.map(absolute_transform);
            self.start(
                index,
                anim,
                clips,
                frames,
                Start {
                    looping,
                    scripted: true,
                    moves,
                    absolute,
                    now,
                },
            );
        }
        Some(frames as f32 / FRAMES_PER_SECOND)
    }

    pub fn stop(&mut self, anim: &str) {
        let anim = anim.to_ascii_uppercase();
        for f in &mut self.figures {
            if f.playing.as_ref().is_some_and(|p| p.anim == anim) {
                f.playing = None;
            }
        }
    }

    pub fn set_visible(&mut self, name: &str, visible: bool) -> bool {
        match self.find(name) {
            Some(i) => {
                self.figures[i].visible = visible;
                true
            }
            None => false,
        }
    }

    /// Stands an actor at a named position.
    pub fn place(&mut self, name: &str, position: &str) -> bool {
        let (Some(i), Some(&(pos, heading))) = (
            self.find(name),
            self.positions.get(&position.to_ascii_uppercase()),
        ) else {
            return false;
        };
        let f = &mut self.figures[i];
        f.walk = None;
        if let Some(a) = &mut f.actor {
            a.position = pos;
            a.heading = heading.to_radians();
            f.world = placement::place(&f.pose, &a.character, a.position, a.heading);
            f.visible = true;
            f.dirty = true;
        }
        true
    }

    /// Schedules a change to somebody's face, by noun or model name.
    pub fn face(&mut self, who: &str, at: f64, change: FaceChange) {
        if let Some(i) = self.find(who)
            && let Some(face) = &mut self.figures[i].face
        {
            face.events.push((at, change));
        }
    }

    fn roll(&mut self) -> f64 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng % 10_000) as f64 / 10_000.0
    }

    /// Plays due face changes, starts blinks, and composes changed faces.
    fn update_faces(&mut self, lib: &Library, now: f64) {
        for i in 0..self.figures.len() {
            let Some(face) = &self.figures[i].face else {
                continue;
            };
            let blink_due = now >= face.next_blink && self.figures[i].visible;
            if blink_due {
                let face = self.figures[i].face.as_ref().unwrap();
                let total: u32 = face.config.blinks.iter().map(|(_, w)| w).sum();
                let gap = face.config.blink_gap;
                let blinks = face.config.blinks.clone();
                let mut pick = (self.roll() * total.max(1) as f64) as u32;
                let chosen = blinks.iter().find(|(_, w)| {
                    let hit = pick < *w;
                    pick = pick.saturating_sub(*w);
                    hit
                });
                let next = now + gap.0 + (gap.1 - gap.0).max(0.0) * self.roll();
                let events: Vec<(f64, FaceChange)> = chosen
                    .and_then(|(name, _)| read_anm(lib, name))
                    .map(|anm| {
                        anm.faces
                            .iter()
                            .map(|f| (now + f.frame as f64 / 15.0, f.change.clone()))
                            .collect()
                    })
                    .unwrap_or_default();
                let face = self.figures[i].face.as_mut().unwrap();
                face.events.extend(events);
                face.next_blink = next;
            }
            let figure = &mut self.figures[i];
            let face = figure.face.as_mut().unwrap();
            face.events.sort_by(|a, b| a.0.total_cmp(&b.0));
            while face.events.first().is_some_and(|(t, _)| *t <= now) {
                let (_, change) = face.events.remove(0);
                face.apply(&change);
            }
            if face.dirty {
                face.dirty = false;
                face.image = self.compositor.compose(lib, &face.config, &face.expression);
            }
        }
    }

    /// Turns an actor on the spot.
    pub fn turn(&mut self, name: &str, heading: f32) {
        let Some(i) = self.find(name) else { return };
        let f = &mut self.figures[i];
        if let Some(a) = &mut f.actor {
            a.heading = heading;
            f.world = placement::place(&f.pose, &a.character, a.position, a.heading);
            f.dirty = true;
        }
    }

    /// Stands an actor at a spot, facing a heading in radians.
    pub fn put(&mut self, name: &str, position: Vec3, heading: f32) {
        let Some(i) = self.find(name) else { return };
        let f = &mut self.figures[i];
        if let Some(a) = &mut f.actor {
            a.position = position;
            a.heading = heading;
            f.world = placement::place(&f.pose, &a.character, a.position, a.heading);
            f.walk = None;
            f.dirty = true;
        }
    }

    pub fn actor_position(&self, name: &str) -> Option<(Vec3, f32)> {
        let f = &self.figures[self.find(name)?];
        f.actor.as_ref().map(|a| (a.position, a.heading))
    }

    /// The middle of a figure's posed vertices, for walking up to it.
    pub fn center(&self, name: &str) -> Option<Vec3> {
        let f = &self.figures[self.find(name)?];
        if f.world_vertices.is_empty() {
            return None;
        }
        let (min, max) = f
            .world_vertices
            .iter()
            .fold((Vec3::MAX, Vec3::MIN), |(lo, hi), v| {
                (lo.min(*v), hi.max(*v))
            });
        Some((min + max) * 0.5)
    }

    /// Sends an actor along a route, playing their walk.
    pub fn walk(
        &mut self,
        lib: &Library,
        name: &str,
        path: Vec<[f32; 2]>,
        heading: Option<f32>,
        now: f64,
    ) -> bool {
        let Some(i) = self.find(name) else {
            return false;
        };
        let Some(walk_anim) = self.figures[i]
            .actor
            .as_ref()
            .and_then(|a| a.character.walk_anim.clone())
        else {
            return false;
        };
        let model = self.figures[i].name.clone();
        let clip = self
            .clips_for(lib, &walk_anim, &model)
            .into_iter()
            .next()
            .map(|(_, c)| c);
        let speed = clip
            .as_ref()
            .and_then(|c| stride_speed(&self.figures[i], c))
            .unwrap_or(DEFAULT_WALK_SPEED);
        let f = &mut self.figures[i];
        f.playing = None;
        f.walk = Some(Walk {
            path,
            clip,
            started: now,
            last: now,
            speed,
            heading,
        });
        true
    }

    pub fn is_walking(&self, name: &str) -> bool {
        self.find(name)
            .is_some_and(|i| self.figures[i].walk.is_some())
    }

    /// Advances every walk, animation, face and idle script to `now`.
    pub fn update(&mut self, lib: &Library, now: f64) {
        self.update_faces(lib, now);
        for i in 0..self.figures.len() {
            if self.figures[i].walk.is_some() {
                step_walk(&mut self.figures[i], now);
                continue;
            }
            let figure = &mut self.figures[i];
            if let Some(playing) = &figure.playing {
                let mut t = ((now - playing.started) as f32) * FRAMES_PER_SECOND;
                let finished = !playing.looping && t >= playing.frames as f32;
                if playing.looping {
                    t %= playing.frames.max(1) as f32;
                }
                // A clip that ends poses its last frame rather than stopping
                // short of it.
                let t = t.min(playing.frames as f32);
                for (start, clip) in &playing.clips {
                    let local = t - *start as f32;
                    if local >= 0.0 {
                        figure.pose.apply(clip, local, playing.looping);
                    }
                }
                figure.dirty = true;
                if finished {
                    // A move leaves the actor where the clip put them.
                    if playing.moves
                        && let Some(a) = &mut figure.actor
                        && let Some((floor, facing)) =
                            placement::floor_and_facing(&figure.pose, &a.character)
                    {
                        a.position = figure.world.transform_point3(floor);
                        let facing = figure
                            .world
                            .transform_vector3(facing.unwrap_or(Vec3::NEG_Z));
                        a.heading = facing.x.atan2(facing.z);
                    }
                    figure.playing = None;
                } else {
                    continue;
                }
            }
            if now < figure.waiting_until || !figure.visible {
                continue;
            }
            let Some(mut gas) = figure.gas.take() else {
                continue;
            };
            let mut keep = true;
            // A few tries, so a missing animation does not stall the script.
            for _ in 0..4 {
                match gas.step() {
                    Request::Play { anim, looping } => {
                        let model = self.figures[i].name.clone();
                        let clips = self.clips_for(lib, &anim, &model);
                        if !clips.is_empty() {
                            let frames = read_anm(lib, &anim).map_or(0, |a| a.frames).max(
                                clips
                                    .iter()
                                    .map(|(f, c)| f + c.frame_count)
                                    .max()
                                    .unwrap_or(1),
                            );
                            self.start(
                                i,
                                &anim,
                                clips,
                                frames,
                                Start {
                                    looping,
                                    scripted: false,
                                    moves: false,
                                    absolute: None,
                                    now,
                                },
                            );
                            break;
                        }
                    }
                    Request::Wait(seconds) => {
                        self.figures[i].waiting_until = now + seconds as f64;
                        break;
                    }
                    Request::Stop => {
                        keep = false;
                        break;
                    }
                }
            }
            if keep {
                self.figures[i].gas = Some(gas);
            }
        }
    }

    /// Writes changed figures' vertices to the GPU.
    pub fn upload(&mut self, renderer: &Renderer) {
        for figure in &mut self.figures {
            figure.gpu.visible = figure.visible;
            if let Some(face) = &mut figure.face
                && let Some(image) = face.image.take()
            {
                renderer.replace_model_texture(&mut figure.gpu, face.submesh, &image);
            }
            if !figure.dirty {
                continue;
            }
            figure.dirty = false;
            let verts = vertices(figure);
            figure.world_vertices = verts.iter().map(|v| Vec3::from(v.position)).collect();
            renderer.update_model(&figure.gpu, &verts);
            let mut light = ModelLight::default();
            if let (Some(a), Some(sun)) = (&figure.actor, self.light) {
                light.direction = (sun - a.position).into();
            }
            renderer.set_model_light(&figure.gpu, light);
        }
    }

    pub fn models(&self) -> Vec<&GpuModel> {
        self.figures.iter().map(|f| &f.gpu).collect()
    }

    /// The nearest visible figure with a noun under a ray: distance and noun.
    /// Which fidget an actor is doing.
    pub fn fidget(&self, who: &str) -> Option<Fidget> {
        self.figures[self.find(who)?].fidget
    }

    /// Gives an actor a different script for one kind of fidget, returning
    /// the one it replaces. If that kind is running it restarts with the new
    /// script, or stops if there is none, as G-Engine's
    /// `CheckUpdateActiveFidget` does.
    pub fn set_fidget_gas(
        &mut self,
        lib: &Library,
        who: &str,
        kind: Fidget,
        gas: Option<String>,
    ) -> Option<Option<String>> {
        let i = self.find(who)?;
        let figure = &mut self.figures[i];
        let old = std::mem::replace(&mut figure.fidgets[kind as usize], gas);
        if figure.fidget == Some(kind) {
            figure.fidget = None;
            figure.gas = None;
            self.start_fidget(lib, who, kind);
        }
        Some(old)
    }

    /// Starts an actor's idle, talk or listen fidget, as G-Engine does: not
    /// if it is already the one running, and only if the actor has a script
    /// for it.
    pub fn start_fidget(&mut self, lib: &Library, who: &str, kind: Fidget) {
        let Some(i) = self.find(who) else { return };
        let figure = &mut self.figures[i];
        if figure.fidget == Some(kind) {
            return;
        }
        if let Some(gas) = figure.fidgets[kind as usize]
            .as_deref()
            .and_then(|g| load_gas(lib, g))
        {
            figure.gas = Some(gas);
            figure.fidget = Some(kind);
            figure.waiting_until = 0.0;
        }
    }

    pub fn stop_fidget(&mut self, who: &str) {
        if let Some(i) = self.find(who) {
            self.figures[i].gas = None;
            self.figures[i].fidget = None;
        }
    }

    /// A figure's triangles where they stand now, found by model name.
    pub fn world_triangles(&self, model: &str) -> Option<Vec<[Vec3; 3]>> {
        let upper = model.to_ascii_uppercase();
        let f = self.figures.iter().find(|f| f.name == upper)?;
        Some(
            f.triangles
                .iter()
                .filter_map(|t| {
                    let [a, b, c] = t.map(|i| f.world_vertices.get(i as usize).copied());
                    Some([a?, b?, c?])
                })
                .collect(),
        )
    }

    pub fn pick(&self, origin: Vec3, direction: Vec3) -> Option<(f32, String)> {
        let mut best: Option<(f32, String)> = None;
        for f in self.figures.iter().filter(|f| f.visible) {
            let Some(noun) = &f.noun else { continue };
            for t in &f.triangles {
                let [a, b, c] = t.map(|i| {
                    f.world_vertices
                        .get(i as usize)
                        .copied()
                        .unwrap_or(Vec3::ZERO)
                });
                if let Some(d) = ray_triangle(origin, direction, a, b, c)
                    && best.as_ref().is_none_or(|(bd, _)| d < *bd)
                {
                    best = Some((d, noun.clone()));
                }
            }
        }
        best
    }

    pub fn describe(&self) -> String {
        let names: Vec<_> = self
            .figures
            .iter()
            .filter(|f| f.visible)
            .map(|f| f.name.as_str())
            .collect();
        names.join(", ")
    }
}

struct Start {
    looping: bool,
    scripted: bool,
    moves: bool,
    absolute: Option<Mat4>,
    now: f64,
}

/// Where an absolute animation puts its model, after G-Engine's
/// `CalcAbsolutePosition`. The first offset is written actor-to-model, so it
/// is negated; both are written with y and z swapped, from Maya.
fn absolute_transform(p: [f32; 8]) -> Mat4 {
    let model_to_actor = Vec3::new(-p[0], -p[2], -p[1]);
    let world_to_model = Vec3::new(p[4], p[6], p[5]);
    let turn = Mat4::from_rotation_y(p[7].to_radians());
    let position = world_to_model + turn.transform_vector3(model_to_actor);
    Mat4::from_translation(position) * Mat4::from_rotation_y((p[7] - p[3]).to_radians())
}

/// How fast a walk clip's floor point travels, in units a second.
fn stride_speed(figure: &Figure, clip: &VertexAnimation) -> Option<f32> {
    let a = figure.actor.as_ref()?;
    let mut pose = figure.pose.clone();
    pose.apply(clip, 0.0, false);
    let (start, _) = placement::floor_and_facing(&pose, &a.character)?;
    pose.apply(clip, clip.frame_count.saturating_sub(1) as f32, false);
    let (end, _) = placement::floor_and_facing(&pose, &a.character)?;
    let distance = Vec3::new(end.x - start.x, 0.0, end.z - start.z).length();
    let seconds = clip.frame_count.saturating_sub(1) as f32 / FRAMES_PER_SECOND;
    (distance > 1.0 && seconds > 0.0).then(|| distance / seconds)
}

/// Moves a walking actor along their route and poses their stride. The clip
/// is played for its pose; placing the model on the actor every frame takes
/// its own forward travel back out, so the route decides where they go.
fn step_walk(figure: &mut Figure, now: f64) {
    let Some(walk) = &mut figure.walk else { return };
    let Some(actor) = &mut figure.actor else {
        figure.walk = None;
        return;
    };
    let dt = (now - walk.last).max(0.0) as f32;
    walk.last = now;
    let mut budget = walk.speed * dt;
    while budget > 0.0 {
        let Some(&next) = walk.path.first() else {
            break;
        };
        let to = Vec3::new(next[0], actor.position.y, next[1]) - actor.position;
        let distance = Vec3::new(to.x, 0.0, to.z).length();
        if distance > 0.01 {
            actor.heading = to.x.atan2(to.z);
        }
        if distance <= budget {
            actor.position.x = next[0];
            actor.position.z = next[1];
            budget -= distance;
            walk.path.remove(0);
        } else {
            actor.position += Vec3::new(to.x, 0.0, to.z) / distance * budget;
            budget = 0.0;
        }
    }
    let arrived = walk.path.is_empty();
    if let Some(clip) = &walk.clip {
        let length = clip.frame_count.saturating_sub(1).max(1) as f32;
        let t = ((now - walk.started) as f32 * FRAMES_PER_SECOND) % length;
        figure.pose.apply(clip, t, true);
    }
    if arrived {
        if let Some(h) = walk.heading {
            actor.heading = h;
        }
        figure.walk = None;
        figure.waiting_until = now;
    }
    figure.world = placement::place(
        &figure.pose,
        &actor.character,
        actor.position,
        actor.heading,
    );
    figure.dirty = true;
}

/// Möller–Trumbore: distance along the ray to the triangle, if it hits.
pub fn ray_triangle(origin: Vec3, dir: Vec3, a: Vec3, b: Vec3, c: Vec3) -> Option<f32> {
    let (e1, e2) = (b - a, c - a);
    let p = dir.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-7 {
        return None;
    }
    let inv = 1.0 / det;
    let s = origin - a;
    let u = s.dot(p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = s.cross(e1);
    let v = dir.dot(q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = e2.dot(q) * inv;
    (t > 0.0).then_some(t)
}

/// World-space vertices for a figure's current pose.
fn vertices(figure: &Figure) -> Vec<ModelVertex> {
    let world_rotation = Mat3::from_mat4(figure.world);
    let mut out = Vec::with_capacity(figure.gpu.vertex_count);
    for (mesh, posed) in figure.model.meshes.iter().zip(&figure.pose.meshes) {
        let to_world = figure.world * posed.matrix();
        // G-Engine's finding: characters' normals are already in model space,
        // props' are in mesh space.
        let normal_matrix = if figure.actor.is_some() {
            world_rotation
        } else {
            world_rotation * Mat3::from_mat4(posed.matrix())
        };
        for (sub, positions) in mesh.submeshes.iter().zip(&posed.positions) {
            let [r, g, b] = sub.color;
            for (k, p) in positions.iter().enumerate() {
                let normal = sub.normals.get(k).copied().unwrap_or([0.0, 1.0, 0.0]);
                out.push(ModelVertex {
                    position: to_world.transform_point3(Vec3::from(*p)).into(),
                    normal: (normal_matrix * Vec3::from(normal))
                        .normalize_or_zero()
                        .into(),
                    uv: sub.uvs.get(k).copied().unwrap_or([0.0, 0.0]),
                    tint: [r, g, b, 255],
                });
            }
        }
    }
    out
}

pub fn read_anm(lib: &Library, name: &str) -> Option<Animation> {
    let name = name.trim_end_matches(".anm").trim_end_matches(".ANM");
    lib.read(&format!("{name}.ANM"))
        .or_else(|_| lib.read(&format!("{name}.YAK")))
        .ok()
        .map(|b| Animation::parse(&b))
}

fn positions(sif: &Document) -> HashMap<String, (Vec3, f32)> {
    let mut out = HashMap::new();
    for line in sif.sections("POSITIONS").flat_map(|s| &s.lines) {
        let (Some(first), Some(pos)) = (
            line.pairs.first(),
            line.get("pos").and_then(crate::room::floats::<3>),
        ) else {
            continue;
        };
        let heading = line
            .get("heading")
            .and_then(|h| h.parse().ok())
            .unwrap_or(0.0);
        out.entry(first.key.to_ascii_uppercase())
            .or_insert((Vec3::from(pos), heading));
    }
    out
}

fn hidden(line: &Line) -> bool {
    line.pairs
        .iter()
        .any(|p| p.is_flag() && p.key.eq_ignore_ascii_case("hidden"))
}

fn load_gas(lib: &Library, name: &str) -> Option<GasPlayer> {
    let name = name.trim();
    let bytes = lib
        .read(name)
        .or_else(|_| lib.read(&format!("{name}.GAS")))
        .ok()?;
    let seed = name
        .bytes()
        .fold(0x9E37_79B9_7F4A_7C15u64, |h, b| h.rotate_left(5) ^ b as u64);
    Some(GasPlayer::new(gk3_formats::gas::parse(&bytes), seed))
}

fn load_texture(lib: &Library, name: &str) -> Image {
    lib.read(&format!("{name}.BMP"))
        .ok()
        .and_then(|b| image::decode(&b).ok())
        .unwrap_or(Image {
            width: 1,
            height: 1,
            rgba: vec![200, 200, 200, 255],
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rays_hit_triangles() {
        let (a, b, c) = (
            Vec3::new(-1.0, -1.0, 5.0),
            Vec3::new(1.0, -1.0, 5.0),
            Vec3::new(0.0, 1.0, 5.0),
        );
        assert_eq!(ray_triangle(Vec3::ZERO, Vec3::Z, a, b, c), Some(5.0));
        assert_eq!(ray_triangle(Vec3::ZERO, -Vec3::Z, a, b, c), None);
        assert_eq!(
            ray_triangle(Vec3::new(5.0, 0.0, 0.0), Vec3::Z, a, b, c),
            None
        );
    }

    #[test]
    fn absolute_placement_matches_the_reference_formula() {
        // No offsets: the identity.
        let m = absolute_transform([0.0; 8]);
        assert!((m.transform_point3(Vec3::ONE) - Vec3::ONE).length() < 1e-5);
        // A world offset only, written x, z, y.
        let m = absolute_transform([0.0, 0.0, 0.0, 0.0, 10.0, 20.0, 30.0, 0.0]);
        assert!((m.transform_point3(Vec3::ZERO) - Vec3::new(10.0, 30.0, 20.0)).length() < 1e-5);
    }
}
