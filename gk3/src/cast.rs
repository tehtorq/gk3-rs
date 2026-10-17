//! The people and props in a room: loading them from the location file,
//! animating them for scripts and idle behaviour, walking them, and handing
//! their vertices to the renderer.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use gk3_barn::Library;
use gk3_formats::act::{FRAMES_PER_SECOND, VertexAnimation};
use gk3_formats::anm::{Animation, CueKind};
use gk3_formats::image::{self, Image};
use gk3_formats::ini::{Document, Line};
use gk3_formats::model::Model;
use gk3_game::character::Character;
use gk3_game::gas_player::{GasPlayer, Request};
use gk3_game::placement;
use gk3_game::pose::Pose;
use gk3_render::{GpuModel, ModelLight, ModelTexture, ModelVertex, Renderer, SubmeshDesc};
use glam::{Mat3, Mat4, Vec2, Vec3};

use crate::faces::{Compositor, Expression, FaceConfig};
use gk3_formats::anm::FaceChange;

/// The vertex animations an `.ANM` starts, by the frame each begins on.
type Clips = Vec<(u32, Arc<VertexAnimation>)>;

/// What an `.ANM` does to each figure, by index: its clips and, for a set
/// piece, where it is placed.
type ByFigure = HashMap<usize, (Clips, Option<[f32; 8]>)>;

/// An animation in progress: the clips it starts and when.
struct Playing {
    anim: String,
    clips: Vec<(u32, Arc<VertexAnimation>)>,
    frames: u32,
    looping: bool,
    started: f64,
    /// Started by the actor's own fidget script rather than by the story.
    fidgeting: bool,
}

struct Walk {
    path: Vec<[f32; 2]>,
    clip: Option<Arc<VertexAnimation>>,
    started: f64,
    last: f64,
    speed: f32,
    heading: Option<f32>,
    /// The start animation; its first frame is the stance a walk ends in
    /// before turning.
    stance: Option<Arc<VertexAnimation>>,
    /// The walk's opening stride: the start animation, or a turning start
    /// for a goal behind.
    opener: Option<Arc<VertexAnimation>>,
    /// The opener turns the walker round, as G-Engine's
    /// `FollowPathStartTurnLeft` and `Right`.
    turning_start: bool,
    /// Arrived, and turning to the heading asked for.
    turning: bool,
    /// Asked for by the actor's own fidget script, which carries on after.
    fidget: bool,
    /// The clips carry the actor forward themselves, as G-Engine plays
    /// walks: feet stay planted and the walker only steers. Clips that walk
    /// on the spot are slid along the route instead.
    carried: bool,
    /// Where the clip now playing put the model, and when it began.
    cycle: Mat4,
    cycle_started: f64,
    /// The floor's height where the clip now playing was placed: the model
    /// rises and falls with the floor from there, frame by frame.
    cycle_ground: f32,
    /// Still in the opening stride.
    opening: bool,
    /// Where the walk ends, and when to give up getting there.
    goal: Option<[f32; 2]>,
    deadline: f64,
    /// The frames the opening stride and the cycle put a foot down on (and
    /// whether it scuffs), from their animations' `FOOTSTEP` lines.
    opening_steps: Vec<(u32, bool)>,
    cycle_steps: Vec<(u32, bool)>,
    /// How far into the clip now playing the last frame got, in frames.
    clip_time: f32,
    /// Arrived: easing from where and how the last stride left the actor
    /// onto the goal, standing.
    settle: Option<Settle>,
    /// How near the end, and the heading, count as there.
    tolerance: Tolerance,
    /// Arrived and turning: facing within this of the heading is done.
    face_within: Option<f32>,
    /// The opening stride is the turn-right start: the cycle goes on from
    /// the character's `start_turn_right_frame`.
    turned_right: bool,
    /// The heading model's direction (see `heading_model_angles`) at each
    /// frame of the opening stride and of the cycle, for characters with
    /// one: each clip is placed so it carries the body straight on from
    /// the last, as the original places animations by it. Empty without.
    arrow_opener: Vec<f32>,
    arrow_cycle: Vec<f32>,
    /// The heading model's direction in the standing pose.
    arrow_standing: Option<f32>,
    /// A turning start's own turn (±135°), the heading set off with, and
    /// how much the walker has been steered on top while it plays.
    builtin_turn: f32,
    set_off_heading: f32,
    steered: f32,
    /// Held up by another walker, and since when (the first hold of this
    /// walk), and whether the walk has given up minding others (after
    /// `GIVE_WAY_FOR`), as the original's state 4 (0x419270, 0x41938c).
    pause: Option<Pause>,
    held_since: Option<f64>,
    ignore_others: bool,
    /// The others near enough to matter when last looked at, by index: the
    /// route is checked again only when they change (0x4190cf).
    others: Vec<usize>,
}

/// A walk paused for another walker: the stride it stopped in, eased out
/// of into standing, and when to look again.
struct Pause {
    until: f64,
    from: Pose,
    /// How the model is turned standing (see `stand_from`).
    yaw_to: f32,
    started: f64,
}

/// The rest of a walk (`Cast::walk_left`).
#[derive(Debug, Clone)]
pub struct WalkLeft {
    pub path: Vec<[f32; 2]>,
    pub heading: Option<f32>,
    pub tolerance: Tolerance,
    /// The walker's size, which the original steps along the route by.
    pub radius: f32,
}

/// What the world needs of each walker to keep them from walking into
/// each other.
#[derive(Debug, Clone)]
pub struct Walker {
    pub index: usize,
    pub at: Vec2,
    pub radius: f32,
    /// Walking (not standing, turning at the end, or paused).
    pub moving: bool,
    /// Paused, until when.
    pub paused_until: Option<f64>,
    pub held_since: Option<f64>,
    pub ignore_others: bool,
    pub goal: Option<[f32; 2]>,
    pub path: Vec<[f32; 2]>,
    pub others: Vec<usize>,
}

/// How long a walk minds other walkers before going on regardless: the
/// original's 30 seconds.
pub const GIVE_WAY_FOR: f64 = 30.0;

/// The ease at the end of a walk: from the spot and pose the last stride
/// reached, to the goal and the standing pose, over `SETTLE_SECONDS`.
/// G-Engine (and the original) jump straight there, which shows as a hop:
/// a deliberate difference (`docs/deviations.md`).
struct Settle {
    from: Vec2,
    pose: Pose,
    /// How the model is turned standing. The pose eased from has been
    /// turned to match (see `stand_from`), so the model stays turned so
    /// throughout, about the floor point.
    yaw_to: f32,
    started: f64,
}

/// How close a walker must come to a walk's end, and to a corner on the
/// way, for it to count as reached: G-Engine's figures.
pub const AT_END: f32 = 2.0;

/// How a walk counts as finished, which the original sets by the kind of
/// walk (`docs/re-walking.md`): within `arrive` of its end, and facing
/// within `face` of its heading. Without a `face`, the walker turns to the
/// heading exactly.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tolerance {
    pub arrive: f32,
    pub face: Option<f32>,
}

impl Tolerance {
    pub const EXACT: Tolerance = Tolerance {
        arrive: AT_END,
        face: None,
    };
    /// `WalkTo`, a fidget's walk, `ClearRegion`: within 2, facing within
    /// 15 degrees.
    pub const TO_SPOT: Tolerance = Tolerance {
        arrive: AT_END,
        face: Some(0.261_799_4),
    };
    /// Into an animation's first pose: within 1, facing within 0.86
    /// degrees.
    pub const INTO_ANIMATION: Tolerance = Tolerance {
        arrive: 1.0,
        face: Some(0.015),
    };
}
const AT_CORNER: f32 = 12.25;

/// How long the end of a walk eases into standing.
const SETTLE_SECONDS: f64 = 0.25;

/// Further than this from the goal (a walk given up on), the actor is put
/// there at once rather than slid.
const SETTLE_REACH: f32 = 12.0;

/// Where a fidget script wants its actor to walk to.
pub enum WalkGoal {
    /// The first of these positions they are not already at.
    Named(Vec<String>),
    Point(Vec3),
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
    /// A stopped fidget's tidying-up animations, each due when the last
    /// ends.
    cleanup: VecDeque<String>,
    /// `StartMorphAnimation` under way: easing from a pose into another.
    morph: Option<Morph>,
    /// Submeshes to repaint with another texture, by flat index and the
    /// texture's name.
    repaint: Vec<(usize, String)>,
    /// Parts not drawn: mesh and group.
    hidden_parts: std::collections::HashSet<(usize, usize)>,
    /// An actor's blob shadow on the floor, and whether the story wants it.
    shadow: Option<GpuModel>,
    shadow_on: bool,
    /// Whether the floor was found below it, where its shadow lies.
    shadow_found: bool,
    /// Its bounds in the world as the original reckons them (see
    /// `model_bounds`), which its light and shadow are taken from.
    bounds: Option<(Vec3, Vec3)>,
    /// Lit evenly in this colour (0 to 1) rather than by the room's light:
    /// G-Engine's fixed model lighting.
    fixed_light: Option<[f32; 3]>,
    /// How far the light swings the shading either way (the scene's
    /// `ambientRange`; the original's default is 0.3, and 0 lights the
    /// model flat).
    ambient_range: Option<f32>,
    /// The light on the floor about it, which lights it.
    floor_light: FloorLight,
    /// A head that turns, for characters with head-turn animations.
    head: Option<HeadRig>,
    playing: Option<Playing>,
    walk: Option<Walk>,
    /// How near another walker may come: half the model's breadth, at most
    /// 36, as the original's `mCollisionRadius` (0x41985e).
    radius: f32,
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
    /// Every part of the model wearing the face: Jean's head is two.
    submeshes: Vec<usize>,
    expression: Expression,
    events: Vec<(f64, FaceChange)>,
    next_blink: f64,
    dirty: bool,
    image: Option<Image>,
    /// Where a `Glance` holds the eyes (pixels right and down), and until.
    glance: Option<((f32, f32), f64)>,
    /// What the eyes follow (`Lookit*`), and until when, if not for good.
    look_at: Option<(LookTarget, Option<f64>)>,
    /// The eyes' idle jitter: its offset in pixels, whether it runs, and
    /// when it next moves.
    jitter: (f32, f32),
    jitter_on: bool,
    next_jitter: f64,
}

/// What someone's eyes are turned to.
#[derive(Debug, Clone, PartialEq)]
pub enum LookTarget {
    /// Someone or something in the room, by noun or model.
    Noun(String),
    Point(Vec3),
}

/// A head that turns, as the original turns one: by taking the head's pose
/// from the character's `<code>TurnHead{L,R,Up,Down}` animations, each a
/// steady turn from rest to its furthest, at the frame matching the angle
/// wanted. Only characters with those animations (the Abbé) have one.
struct HeadRig {
    mesh: usize,
    /// How far it may turn: left, up, right, down, in radians.
    limits: [f32; 4],
    /// Left, right, up, down, each with the angle its last frame reaches.
    clips: [(Arc<VertexAnimation>, f32); 4],
    /// Now, and where `TurnHead` asked for, held until when: yaw (right
    /// +), pitch (up +).
    angle: (f32, f32),
    to: (f32, f32),
    until: f64,
    /// Something it follows, until when (if not for good), and whether it
    /// snaps there.
    follow: Option<(LookTarget, Option<f64>, bool)>,
    /// The head's turn in its own frame, applied over its posed transform.
    local: Mat4,
}

/// How fast a head turns to follow something, radians a second.
const HEAD_FOLLOW_SPEED: f32 = 2.0;

impl FaceState {
    fn apply(&mut self, change: &FaceChange) {
        match change {
            FaceChange::Mouth(shape) => {
                self.expression.mouth = Some(format!("{}_{shape}", self.config.mouths))
            }
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
    /// Whether the actor stands anywhere yet. One the scene gave no spot
    /// (or a spot it never defines, as DIN110A's Mosely) is placed by the
    /// first animation that puts the model somewhere.
    placed: bool,
}

pub struct Cast {
    figures: Vec<Figure>,
    clips: HashMap<String, Option<Arc<VertexAnimation>>>,
    light: Option<Vec3>,
    pub positions: HashMap<String, (Vec3, f32)>,
    /// The camera a position names (`camera=`), which `InitEgoPosition`
    /// cuts to.
    pub position_cameras: HashMap<String, String>,
    pub warnings: Vec<String>,
    /// What fidgets' animations show, hide and repaint, by time, for the
    /// world to carry out (some are parts of the room).
    pub cues: Vec<(f64, CueKind)>,
    /// The room's floor, for finding how high it is under someone.
    floor: Vec<[Vec3; 3]>,
    /// When `update` last ran, for things that move at a speed.
    last_update: f64,
    /// Textures already read for repainting, by name, and those already on
    /// the GPU: animations cycle the same few over and over.
    paints: HashMap<String, Image>,
    gpu_paints: HashMap<String, ModelTexture>,
    /// Walks fidget scripts have asked for, by model name, for the world to
    /// find a way for and start with `walk_for_fidget`.
    pub walk_requests: Vec<(String, WalkGoal)>,
    /// Actors whose fidget scripts moved them to another location, by noun.
    pub moved: Vec<(String, String)>,
    /// Feet put down while walking, by noun, and whether each scuffed.
    pub footsteps: Vec<(String, bool)>,
    /// Walkers who ran out of time getting there and were put at the end,
    /// since the last look: for the log.
    pub gave_up: Vec<(String, String)>,
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
            position_cameras: position_cameras(sif),
            warnings: Vec::new(),
            cues: Vec::new(),
            paints: HashMap::new(),
            gpu_paints: HashMap::new(),
            floor: Vec::new(),
            last_update: 0.0,
            walk_requests: Vec::new(),
            moved: Vec::new(),
            footsteps: Vec::new(),
            gave_up: Vec::new(),
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
                    // Written `pos` or `position`, which the original reads
                    // alike (0x5100e0); its `ambient` it reads and drops.
                    .and(l.get("pos").or_else(|| l.get("position")))
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
                // Somebody with no spot, or one the scene never defines, is
                // left where their walker was made (0x502c9d).
                if spot.is_none()
                    && let Some(p) = line.get("pos")
                {
                    cast.warnings.push(format!("{name}: no position {p} in the scene"));
                }
                // Unplaced, a walker stands where it was made: the origin,
                // facing 180 degrees (0x416873).
                let (position, heading) = spot.unwrap_or((Vec3::ZERO, 180.0));
                let placement = ActorPlacement {
                    character,
                    position,
                    heading: heading.to_radians(),
                    placed: spot.is_some(),
                };
                let gas = line.get("idle").and_then(|g| load_gas(lib, g));
                if let Some(mut figure) = cast.figure(lib, renderer, name, Some(placement), gas) {
                    if let Some(reference) = cast.hip_reference(lib, &figure)
                        && let Some(a) = &mut figure.actor
                    {
                        a.character.reference = Some(reference);
                        figure.world = placement::place(&figure.pose, &a.character, a.position, a.heading);
                    } else if cast.facing_backwards(lib, &figure) {
                        if let Some(a) = &mut figure.actor {
                            a.character.facing_reversed = true;
                            figure.world =
                                placement::place(&figure.pose, &a.character, a.position, a.heading);
                        }
                        cast.warnings.push(format!("{name}: faces down its hip's +Y; turned round"));
                    }
                    figure.shadow = Some(shadow_model(lib, renderer));
                    figure.head = head_rig(lib, &mut cast, &figure);
                    figure.noun = line.get("noun").map(|n| n.to_ascii_uppercase());
                    figure.ambient_range = ambient_range(line);
                    figure.fidgets =
                        ["idle", "talk", "listen"].map(|k| line.get(k).map(str::to_string));
                    if let Some(config) =
                        crate::faces::for_character(faces, characters, &figure.name)
                    {
                        let submeshes: Vec<usize> = figure
                            .model
                            .submeshes()
                            .enumerate()
                            .filter(|(_, s)| s.texture.eq_ignore_ascii_case(&config.face))
                            .map(|(i, _)| i)
                            .collect();
                        figure.face = (!submeshes.is_empty()).then(|| FaceState {
                            config,
                            submeshes,
                            expression: Expression::default(),
                            events: Vec::new(),
                            next_blink: 2.0,
                            dirty: true,
                            image: None,
                            glance: None,
                            look_at: None,
                            jitter: (0.0, 0.0),
                            jitter_on: true,
                            next_jitter: 1.0,
                        });
                    }
                    // As G-Engine: an actor is there unless the SIF hides
                    // them, wherever they stand; their idle often places them.
                    figure.visible = !hidden(line);
                    cast.figures.push(figure);
                    // As the scene starts (0x500a77 → 0x501920): posed in
                    // the first frame of their `initAnim`, where it puts
                    // them (0x50186f); else standing where they are, in
                    // their walk's stance.
                    let i = cast.figures.len() - 1;
                    match line.get("initanim") {
                        Some(anim) => cast.sample(lib, i, anim),
                        None => cast.stand_in_stance(lib, i),
                    }
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
                // Its GAS is its idle fidget, for StopPropFidget and
                // StartPropFidget.
                if figure.gas.is_some() {
                    figure.fidgets[Fidget::Idle as usize] = line.get("gas").map(str::to_string);
                    figure.fidget = Some(Fidget::Idle);
                }
                figure.fixed_light = fixed_lighting(line);
                figure.ambient_range = ambient_range(line);
                // A prop casts a blob shadow if its line says `shadow` (the
                // later of `shadow` and `noShadow` holds), as the original
                // (0x510b4c, 0x4d6810); characters always may.
                let mut shadow = false;
                for p in &line.pairs {
                    if p.key.eq_ignore_ascii_case("shadow") || p.key.eq_ignore_ascii_case("shadowtex") {
                        shadow = true;
                    } else if p.key.eq_ignore_ascii_case("noShadow") {
                        shadow = false;
                    }
                }
                if shadow {
                    figure.shadow = Some(shadow_model(lib, renderer));
                }
                // Hidden props are loaded anyway: scripts show them.
                figure.visible = !hidden(line);
                cast.figures.push(figure);
                // A prop's starting animation sets its first pose.
                if let Some(anim) = line.get("initanim") {
                    let i = cast.figures.len() - 1;
                    cast.sample(lib, i, anim);
                }
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
        let mut figure = Figure {
            name: name.to_ascii_uppercase(),
            fidgets: Default::default(),
            fidget,
            noun: None,
            model,
            pose,
            actor,
            world,
            gas,
            cleanup: VecDeque::new(),
            morph: None,
            repaint: Vec::new(),
            hidden_parts: Default::default(),
            shadow: None,
            shadow_on: true,
            shadow_found: false,
            bounds: None,
            fixed_light: None,
            ambient_range: None,
            floor_light: FloorLight::default(),
            head: None,
            playing: None,
            walk: None,
            radius: 0.0,
            waiting_until: 0.0,
            visible: true,
            gpu,
            dirty: true,
            world_vertices: Vec::new(),
            triangles,
            face: None,
        };
        figure.radius = collision_radius(&figure);
        Some(figure)
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
        // As G-Engine's `GKActor::StartAnimation`: a fidget never cuts into
        // what the story asked for, and the story's animations stop the
        // fidget (and any walk) until something starts it again.
        if !how.scripted && (figure.playing.as_ref().is_some_and(|p| !p.fidgeting) || figure.morph.is_some()) {
            return;
        }
        figure.morph = None;
        if how.scripted {
            figure.gas = None;
            figure.fidget = None;
            figure.cleanup.clear();
            figure.walk = None;
        }
        for (start, clip) in &clips {
            if *start == 0 {
                figure.pose.apply(clip, 0.0, false);
            }
        }
        match (how.absolute, &figure.actor) {
            // A set piece plays where it was authored. An actor not yet
            // anywhere stands where it puts them, as G-Engine's actors are
            // synced to their model once their first fidget has posed it.
            (Some(world), a) => {
                let unplaced = a.as_ref().is_some_and(|a| !a.placed);
                figure.world = world;
                if unplaced {
                    sync_actor(figure);
                }
            }
            // An actor never placed has nowhere to play from: the model
            // stays as the animation has it (some are authored where they
            // play in the room, as Buthane's at dinner at 306P), and the
            // actor stands where that is.
            (None, Some(a)) if !a.placed => {
                figure.world = Mat4::IDENTITY;
                sync_actor(figure);
            }
            // Otherwise it plays from wherever the actor stands: place the
            // model so its first pose stands on them.
            (None, Some(a)) => {
                figure.world = placement::place(&figure.pose, &a.character, a.position, a.heading)
            }
            // A prop goes where the animation's actor is put.
            (None, None) => {
                if let Some(world) = how.reference {
                    figure.world = world;
                }
            }
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
            fidgeting: !how.scripted,
        });
        figure.dirty = true;
    }

    /// Plays an `.ANM` on every model it animates. Returns how long it lasts
    /// in seconds, or `None` if nothing in the room is animated by it.
    ///
    /// Whether it is a move animation makes no difference here: G-Engine
    /// puts a non-move actor back where they started, then straight away
    /// stands them wherever the model was left, as everyone always is.
    pub fn play(&mut self, lib: &Library, anim: &str, looping: bool, now: f64) -> Option<f32> {
        self.launch(lib, anim, looping, true, now)
    }

    /// Starts an `.ANM` for the story or for a fidget script.
    fn launch(
        &mut self,
        lib: &Library,
        anim: &str,
        looping: bool,
        scripted: bool,
        now: f64,
    ) -> Option<f32> {
        let anm = read_anm(lib, anim)?;
        self.launch_parsed(lib, anim, &anm, looping, scripted, now)
    }

    /// Plays an animation already read, such as a moment (`.MOM`), for the
    /// story. Returns its length in seconds, as `play`; nothing if it moves nobody here.
    pub fn play_parsed(
        &mut self,
        lib: &Library,
        name: &str,
        anm: &Animation,
        looping: bool,
        now: f64,
    ) -> Option<f32> {
        self.launch_parsed(lib, name, anm, looping, true, now)
    }

    fn launch_parsed(
        &mut self,
        lib: &Library,
        anim: &str,
        anm: &Animation,
        looping: bool,
        scripted: bool,
        now: f64,
    ) -> Option<f32> {
        if !scripted {
            for cue in &anm.cues {
                if matches!(
                    cue.what,
                    CueKind::Visible { .. }
                        | CueKind::PartVisible { .. }
                        | CueKind::Footstep { .. }
                        | CueKind::ModelTexture { .. }
                        | CueKind::SceneTexture { .. }
                ) {
                    let at = now + cue.frame as f64 / FRAMES_PER_SECOND as f64;
                    self.cues.push((at, cue.what.clone()));
                }
            }
        }
        // An animation that moves nobody (it swaps textures, or plays a
        // sound) still takes its time.
        let Some((by_figure, frames)) = self.gather(lib, anm) else {
            let seconds = anm.frames.max(1) as f32 / FRAMES_PER_SECOND;
            return (!anm.cues.is_empty() || !anm.sounds.is_empty() || !anm.faces.is_empty())
                .then_some(seconds);
        };
        // The actors first: then a prop the animation moves relatively is
        // put where its last relative actor is, as the original (0x420346),
        // the clips all being written in one space (Buthane's cigarette in
        // her hand); with no such actor, at the world's origin.
        let mut entries: Vec<_> = by_figure.into_iter().collect();
        entries.sort_by_key(|(i, _)| (self.figures[*i].actor.is_none(), *i));
        let mut reference = None;
        for (index, (clips, placement)) in entries {
            if self.figures[index].actor.is_none() && reference.is_none() {
                reference = Some(
                    self.last_relative_actor(lib, anm)
                        .map_or(Mat4::IDENTITY, |i| self.figures[i].world),
                );
            }
            let absolute = placement.map(absolute_transform);
            self.start(
                index,
                anim,
                clips,
                frames,
                Start {
                    looping,
                    scripted,
                    absolute,
                    reference,
                    now,
                },
            );
        }
        Some(frames as f32 / FRAMES_PER_SECOND)
    }

    /// `StartMorphAnimation(anim, start, frames)`, as the original's
    /// (0x4aa4e4, 0x55acf0, 0x553700): what the animation moves is placed
    /// where the animation puts it and eased, in a straight line, from
    /// where it stood into the animation's frame `start`, over `frames`
    /// less one of the animation's frames; it then holds there. Nothing if
    /// `start` is past the animation's end or `frames` is not above 0.
    /// Returns how long the easing takes, which the story waits for.
    pub fn morph(&mut self, lib: &Library, anim: &str, start: u32, frames: u32, now: f64) -> f32 {
        let Some(anm) = read_anm(lib, anim) else {
            return 0.0;
        };
        let Some((by_figure, total)) = self.gather(lib, &anm) else {
            return 0.0;
        };
        if start >= total || frames == 0 {
            return 0.0;
        }
        let seconds = frames.saturating_sub(1) as f32 / FRAMES_PER_SECOND;
        for (index, (clips, placement)) in by_figure {
            let (from_pose, from_world) = (self.figures[index].pose.clone(), self.figures[index].world);
            let absolute = placement.map(absolute_transform);
            self.start(
                index,
                anim,
                clips.clone(),
                total,
                Start {
                    looping: false,
                    scripted: true,
                    absolute,
                    reference: None,
                    now,
                },
            );
            let f = &mut self.figures[index];
            f.playing = None;
            for (offset, clip) in &clips {
                if start >= *offset {
                    f.pose.apply(clip, (start - offset) as f32, false);
                }
            }
            // Where it stood, as seen from where the animation puts it.
            let mut from = from_pose;
            from.transform_by(f.world.inverse() * from_world);
            f.morph = Some(Morph {
                from,
                to: f.pose.clone(),
                started: now,
                seconds,
            });
            f.dirty = true;
        }
        seconds
    }

    /// Poses a figure on an animation's first frame, where it puts them, as
    /// a SIF's `initAnim` does.
    /// Poses everything an animation moves in its first frame, as a
    /// room's code does to put a door back (`Animator::Sample`).
    pub fn sample_first_frame(&mut self, lib: &Library, anim: &str) {
        let Some(anm) = read_anm(lib, anim) else {
            return;
        };
        let figures: Vec<usize> = self
            .gather(lib, &anm)
            .map(|(by_figure, _)| by_figure.into_keys().collect())
            .unwrap_or_default();
        for i in figures {
            self.figures[i].playing = None;
            self.sample(lib, i, anim);
        }
    }

    fn sample(&mut self, lib: &Library, index: usize, anim: &str) {
        let Some(anm) = read_anm(lib, anim) else {
            return;
        };
        let Some((mut by_figure, _)) = self.gather(lib, &anm) else {
            return;
        };
        let Some((clips, placement)) = by_figure.remove(&index) else {
            return;
        };
        let figure = &mut self.figures[index];
        for (start, clip) in &clips {
            if *start == 0 {
                figure.pose.apply(clip, 0.0, false);
            }
        }
        match (placement, &figure.actor) {
            (Some(p), _) => figure.world = absolute_transform(p),
            // Never placed: the model stays as the animation has it.
            (None, Some(a)) if !a.placed => figure.world = Mat4::IDENTITY,
            (None, Some(a)) => {
                figure.world = placement::place(&figure.pose, &a.character, a.position, a.heading)
            }
            // A pose applied straight to a prop leaves it where it is: only
            // an animation started on actors takes its props along
            // (0x420346 is not on this way, 0x50186f).
            (None, None) => {}
        }
        sync_actor(figure);
        figure.dirty = true;
    }

    /// The clips an `.ANM` starts on each figure it animates, with the
    /// placement of a set piece, and how many frames it lasts.
    fn gather(&mut self, lib: &Library, anm: &Animation) -> Option<(ByFigure, u32)> {
        let mut by_figure: ByFigure = HashMap::new();
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
        Some((by_figure, frames))
    }

    /// The last actor, in the animation's order, it moves relatively: whose
    /// placement its relative props take (0x420346).
    fn last_relative_actor(&mut self, lib: &Library, anm: &Animation) -> Option<usize> {
        anm.actions.iter().rev().find_map(|action| {
            if action.placement.is_some() {
                return None;
            }
            let clip = self.clip(lib, &action.clip)?;
            let i = self.find(&clip.model)?;
            self.figures[i].actor.is_some().then_some(i)
        })
    }

    /// Where an animation starts an actor: the floor point and heading of
    /// its first frame, as G-Engine's `WalkToAnimationStart` finds them.
    /// Only a set piece (an absolute animation) has a place of its own;
    /// anything else plays wherever the actor stands, so needs no walk.
    pub fn anim_start(&mut self, lib: &Library, anim: &str, who: &str) -> Option<(Vec3, f32)> {
        let index = self.find(who)?;
        let model = self.figures[index].name.clone();
        let anm = read_anm(lib, anim)?;
        let (clip, placement) = anm.actions.iter().find_map(|action| {
            let clip = self.clip(lib, &action.clip)?;
            clip.model
                .eq_ignore_ascii_case(&model)
                .then_some((clip, action.placement))
        })?;
        let world = absolute_transform(placement?);
        let figure = &self.figures[index];
        let character = &figure.actor.as_ref()?.character;
        let mut pose = figure.pose.clone();
        pose.apply(&clip, 0.0, false);
        let (floor, facing) = placement::floor_and_facing(&pose, character)?;
        let facing = world.transform_vector3(facing.unwrap_or(Vec3::NEG_Z));
        Some((world.transform_point3(floor), facing.x.atan2(facing.z)))
    }

    pub fn stop(&mut self, anim: &str) {
        let anim = anim.to_ascii_uppercase();
        for f in &mut self.figures {
            if f.playing.as_ref().is_some_and(|p| p.anim == anim) {
                f.playing = None;
                sync_actor(f);
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
            a.placed = true;
            f.world = placement::place(&f.pose, &a.character, a.position, a.heading);
            f.visible = true;
            f.dirty = true;
        }
        self.snap_to_floor(i);
        true
    }

    /// `InitEgoPosition`: places someone as `place` does, then stops what
    /// they were playing and stands them in the first frame of their walk's
    /// opening stride, as G-Engine's `GKActor::InitPosition`. Otherwise a
    /// player arriving mid-stride from the last room would shift as their
    /// idle starts.
    pub fn init_position(&mut self, lib: &Library, name: &str, position: &str) -> bool {
        if !self.place(name, position) {
            return false;
        }
        let Some(i) = self.find(name) else {
            return false;
        };
        self.figures[i].playing = None;
        self.stand_in_stance(lib, i);
        true
    }

    /// Stands an actor where they are in the first frame of their walk's
    /// start, as the original poses one by starting that animation and
    /// stopping it at once (0x41d50a).
    fn stand_in_stance(&mut self, lib: &Library, i: usize) {
        let model = self.figures[i].name.clone();
        let stance = self.figures[i]
            .actor
            .as_ref()
            .and_then(|a| a.character.start_anim.clone())
            .and_then(|anim| self.clips_for(lib, &anim, &model).into_iter().next())
            .map(|(_, c)| c);
        let f = &mut self.figures[i];
        if let (Some(clip), Some(a)) = (stance, &f.actor) {
            f.pose.apply(&clip, 0.0, false);
            f.world = placement::place(&f.pose, &a.character, a.position, a.heading);
            f.dirty = true;
        }
    }

    /// Paints one group of one mesh of a model with another texture, as an
    /// animation's `[MTEXTURES]` do. False if there is no such model.
    pub fn retexture(
        &mut self,
        lib: &Library,
        model: &str,
        mesh: usize,
        group: usize,
        texture: &str,
    ) -> bool {
        let Some(i) = self.find(model) else {
            return false;
        };
        let figure = &self.figures[i];
        let Some(index) = figure.model.meshes.get(mesh).and_then(|m| {
            (group < m.submeshes.len()).then(|| {
                figure.model.meshes[..mesh]
                    .iter()
                    .map(|m| m.submeshes.len())
                    .sum::<usize>()
                    + group
            })
        }) else {
            return true;
        };
        let name = texture.to_ascii_uppercase();
        self.paints
            .entry(name.clone())
            .or_insert_with(|| load_texture(lib, texture));
        self.figures[i].repaint.push((index, name));
        true
    }

    /// Shows or hides one part of a model. False if there is no such model.
    pub fn set_part_visible(
        &mut self,
        model: &str,
        mesh: usize,
        group: usize,
        visible: bool,
    ) -> bool {
        let Some(i) = self.find(model) else {
            return false;
        };
        let f = &mut self.figures[i];
        let changed = if visible {
            f.hidden_parts.remove(&(mesh, group))
        } else {
            f.hidden_parts.insert((mesh, group))
        };
        f.dirty |= changed;
        true
    }

    /// What shoes someone wears, for their footsteps.
    pub fn shoe_type(&self, who: &str) -> Option<String> {
        let figure = &self.figures[self.find(who)?];
        figure.actor.as_ref().map(|a| a.character.shoe_type.clone())
    }

    /// The codes somebody's moods may be named by: their own (`MO2`), then
    /// the face they wear's (`MOS`). Empty if they have no face.
    pub fn face_codes(&self, who: &str) -> Vec<String> {
        let Some(figure) = self.find(who).map(|i| &self.figures[i]) else {
            return Vec::new();
        };
        let Some(face) = &figure.face else {
            return Vec::new();
        };
        let mut codes = vec![figure.name.clone()];
        if face.config.code != figure.name {
            codes.push(face.config.code.clone());
        }
        codes
    }

    /// Schedules a change to somebody's face, by noun or model name.
    pub fn face(&mut self, who: &str, at: f64, change: FaceChange) {
        if let Some(i) = self.find(who)
            && let Some(face) = &mut self.figures[i].face
        {
            face.events.push((at, change));
        }
    }

    /// Drops the lip-sync and other face changes due after `from` and up
    /// to `until` (a line cut short), and closes the mouths they would have
    /// moved.
    pub fn cancel_faces(&mut self, from: f64, until: f64) {
        for f in &mut self.figures {
            let Some(face) = &mut f.face else { continue };
            let before = face.events.len();
            face.events.retain(|(at, _)| *at <= from || *at > until);
            if face.events.len() != before {
                face.events.push((from, FaceChange::Clear('M')));
            }
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
            // The eyes: a glance, else whatever they follow, plus jitter.
            let look = self.eye_look(i, now);
            let jitter_due = {
                let face = self.figures[i].face.as_ref().unwrap();
                face.jitter_on && now >= face.next_jitter
            };
            if jitter_due
                && let Some(eyes) = self.figures[i].face.as_ref().unwrap().config.eyes.clone()
            {
                let (rx, ry, rg) = (self.roll(), self.roll(), self.roll());
                let face = self.figures[i].face.as_mut().unwrap();
                face.jitter = (
                    (rx as f32 * 2.0 - 1.0) * eyes.max_jitter.0,
                    (ry as f32 * 2.0 - 1.0) * eyes.max_jitter.1,
                );
                face.next_jitter =
                    now + eyes.jitter_gap.0 + (eyes.jitter_gap.1 - eyes.jitter_gap.0).max(0.0) * rg;
            }
            let figure = &mut self.figures[i];
            let face = figure.face.as_mut().unwrap();
            let quarter = |v: f32| (v * 4.0).round() as i32;
            let eyes = (
                quarter(look.0 + face.jitter.0),
                quarter(look.1 + face.jitter.1),
            );
            if face.expression.eyes != eyes {
                face.expression.eyes = eyes;
                face.dirty = true;
            }
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

    /// Where someone's eyes are turned now, in pixels right and down of
    /// rest: a glance while it lasts, else towards what they follow, as far
    /// as their eyes can see (`Eye Field Of View`) and move (`Max Look
    /// Distance`).
    fn eye_look(&mut self, i: usize, now: f64) -> (f32, f32) {
        let figure = &self.figures[i];
        let Some(face) = &figure.face else {
            return (0.0, 0.0);
        };
        let Some(eyes) = &face.config.eyes else {
            return (0.0, 0.0);
        };
        if let Some((offset, until)) = face.glance
            && now < until
        {
            return offset;
        }
        let Some((target, until)) = &face.look_at else {
            return (0.0, 0.0);
        };
        if until.is_some_and(|u| now >= u) {
            let face = self.figures[i].face.as_mut().unwrap();
            face.look_at = None;
            return (0.0, 0.0);
        }
        let target = target.clone();
        let fov = eyes.field_of_view;
        let max_look = eyes.max_look;
        let Some((mut yaw, mut pitch)) = self.look_angles(i, &target) else {
            return (0.0, 0.0);
        };
        // A turned head has done part of the looking.
        if let Some(head) = &self.figures[i].head {
            yaw -= head.angle.0;
            pitch -= head.angle.1;
        }
        // Their right is the picture's left: the face is painted as seen.
        let half = |deg: f32| (deg / 2.0).to_radians().max(1e-3);
        let fx = (yaw / half(fov.0)).clamp(-1.0, 1.0);
        let fy = (pitch / half(fov.1)).clamp(-1.0, 1.0);
        (-fx * max_look.0, -fy * max_look.1)
    }

    /// Which way something lies from someone's head, as yaw (right +) and
    /// pitch (up +) from the way their body faces, in radians.
    fn look_angles(&self, i: usize, target: &LookTarget) -> Option<(f32, f32)> {
        let a = self.figures[i].actor.as_ref()?;
        let head = a.position + Vec3::Y * a.character.walker_height * 0.92;
        let at = match target {
            LookTarget::Point(p) => *p,
            LookTarget::Noun(n) => {
                let f = &self.figures[self.find(n)?];
                match &f.actor {
                    Some(b) => b.position + Vec3::Y * b.character.walker_height * 0.92,
                    None => self.center(n)?,
                }
            }
        };
        let to = at - head;
        let flat = Vec3::new(to.x, 0.0, to.z).length();
        if flat < 1e-3 && to.y.abs() < 1e-3 {
            return None;
        }
        Some((angle_between(a.heading, to.x.atan2(to.z)), to.y.atan2(flat)))
    }

    /// `TurnHead`: the head to a share (-100 to 100) of how far it turns
    /// each way, held for a while and then back, as the original's timed
    /// head node. Only heads that turn.
    pub fn turn_head(&mut self, who: &str, percent: (i32, i32), seconds: f64, now: f64) -> bool {
        let Some(i) = self.find(who) else {
            return false;
        };
        let Some(head) = &mut self.figures[i].head else {
            return false;
        };
        let [left, up, right, down] = head.limits;
        let (x, y) = (
            percent.0.clamp(-100, 100) as f32 / 100.0,
            percent.1.clamp(-100, 100) as f32 / 100.0,
        );
        head.follow = None;
        head.to = (
            x * if x > 0.0 { right } else { left },
            y * if y > 0.0 { up } else { down },
        );
        head.until = now + seconds;
        true
    }

    /// Moves a turning head on: towards what it follows, or along a turn.
    fn update_head(&mut self, i: usize, now: f64, dt: f32) {
        let follow = self.figures[i].head.as_ref().and_then(|h| h.follow.clone());
        let wanted = follow.as_ref().and_then(|(target, until, _)| {
            if until.is_some_and(|u| now >= u) {
                return Some((0.0, 0.0));
            }
            self.look_angles(i, target)
        });
        let figure = &mut self.figures[i];
        let Some(head) = &mut figure.head else { return };
        let [left, up, right, down] = head.limits;
        let (goal, quick) = match (&follow, wanted) {
            (Some((_, until, quick)), Some((yaw, pitch))) => {
                if until.is_some_and(|u| now >= u) {
                    head.follow = None;
                }
                ((yaw.clamp(-left, right), pitch.clamp(-down, up)), *quick)
            }
            // A `TurnHead` while it holds; then back to rest.
            _ if now < head.until => (head.to, false),
            _ => ((0.0, 0.0), false),
        };
        let angle = if quick {
            goal
        } else {
            let step = HEAD_FOLLOW_SPEED * dt;
            (
                head.angle.0 + (goal.0 - head.angle.0).clamp(-step, step),
                head.angle.1 + (goal.1 - head.angle.1).clamp(-step, step),
            )
        };
        if (angle.0 - head.angle.0).abs() > 1e-4 || (angle.1 - head.angle.1).abs() > 1e-4 {
            head.angle = angle;
            head.local = head_turn(&figure.pose, head);
            figure.dirty = true;
        }
    }

    /// `Glance`: the eyes to a share (-100 to 100) of how far they move, for
    /// a while. Positive is right and down in the face picture, as the
    /// original reads it.
    pub fn glance(&mut self, who: &str, percent: (i32, i32), seconds: f64, now: f64) -> bool {
        let Some(i) = self.find(who) else {
            return false;
        };
        let Some(face) = &mut self.figures[i].face else {
            return false;
        };
        let Some(eyes) = &face.config.eyes else {
            return false;
        };
        let offset = (
            percent.0.clamp(-100, 100) as f32 / 100.0 * eyes.max_look.0,
            percent.1.clamp(-100, 100) as f32 / 100.0 * eyes.max_look.1,
        );
        face.glance = Some((offset, now + seconds));
        true
    }

    /// `Lookit*`: something to look at, for a while or (no time) until
    /// `LookitCancel`, saying which parts look: the eyes, the head (if it
    /// turns), and whether the head snaps round (`Quick`).
    #[allow(clippy::too_many_arguments)]
    pub fn look_with(
        &mut self,
        who: &str,
        target: LookTarget,
        seconds: Option<f64>,
        now: f64,
        eyes: bool,
        head: bool,
        quick: bool,
    ) -> bool {
        let Some(i) = self.find(who) else {
            return false;
        };
        let until = seconds.filter(|s| *s > 0.0).map(|s| now + s);
        let figure = &mut self.figures[i];
        if head && let Some(rig) = &mut figure.head {
            rig.follow = Some((target.clone(), until, quick));
        }
        if eyes && let Some(face) = &mut figure.face {
            face.look_at = Some((target, until));
        }
        true
    }

    pub fn look_cancel(&mut self, who: &str, now: f64) {
        let Some(i) = self.find(who) else { return };
        let figure = &mut self.figures[i];
        if let Some(face) = &mut figure.face {
            face.look_at = None;
            face.glance = None;
        }
        // The head comes back round.
        if let Some(head) = &mut figure.head {
            head.follow = None;
            head.until = now;
        }
    }

    /// `EnableEyeJitter`, `DisableEyeJitter` and `EyeJitter` (one now).
    pub fn eye_jitter(&mut self, who: &str, on: Option<bool>, now: f64) {
        if let Some(i) = self.find(who)
            && let Some(face) = &mut self.figures[i].face
        {
            match on {
                Some(on) => {
                    face.jitter_on = on;
                    if !on {
                        face.jitter = (0.0, 0.0);
                    }
                }
                None => face.next_jitter = now,
            }
        }
    }

    /// `SetWalkAnim`: someone walks with other animations from now on (a
    /// start and a cycle), as when Larry walks with a tray. False if there
    /// is no such actor.
    /// `SetWalkAnim`: the start and cycle, and the turning starts (left,
    /// right) when given.
    pub fn set_walk_anims(
        &mut self,
        who: &str,
        start: &str,
        cycle: &str,
        [left, right]: [Option<String>; 2],
    ) -> bool {
        let Some(i) = self.find(who) else {
            return false;
        };
        let Some(a) = &mut self.figures[i].actor else {
            return false;
        };
        a.character.start_anim = Some(start.to_string());
        a.character.walk_anim = Some(cycle.to_string());
        if left.is_some() && right.is_some() {
            a.character.start_turn_left = left;
            a.character.start_turn_right = right;
        }
        true
    }

    /// Places a prop by its whole transform, as a room's own code does
    /// (CS2's turning heads). False if there is no such figure.
    pub fn set_world(&mut self, name: &str, world: Mat4) -> bool {
        let Some(i) = self.find(name) else {
            return false;
        };
        let f = &mut self.figures[i];
        if f.world != world {
            f.world = world;
            f.dirty = true;
        }
        true
    }

    /// Stands an actor at a spot, facing a heading in radians.
    pub fn put(&mut self, name: &str, position: Vec3, heading: f32) {
        let Some(i) = self.find(name) else { return };
        let f = &mut self.figures[i];
        if let Some(a) = &mut f.actor {
            a.position = position;
            a.heading = heading;
            a.placed = true;
            f.world = placement::place(&f.pose, &a.character, a.position, a.heading);
            f.walk = None;
            f.dirty = true;
        }
        self.snap_to_floor(i);
    }

    /// Where everyone else who walks stands: the seen actors but `who`.
    pub fn walker_spots(&self, who: &str) -> Vec<[f32; 2]> {
        let me = self.find(who);
        self.figures
            .iter()
            .enumerate()
            .filter(|(i, f)| f.visible && Some(*i) != me)
            .filter_map(|(_, f)| f.actor.as_ref().map(|a| [a.position.x, a.position.z]))
            .collect()
    }

    pub fn actor_position(&self, name: &str) -> Option<(Vec3, f32)> {
        let f = &self.figures[self.find(name)?];
        f.actor.as_ref().map(|a| (a.position, a.heading))
    }

    /// Where the actors stand and their walks end, and the named
    /// positions, for the scripts to ask about.
    pub fn whereabouts(&self) -> gk3_game::api::Whereabouts {
        let mut actors = HashMap::new();
        for f in &self.figures {
            let Some(a) = &f.actor else { continue };
            // As G-Engine has an actor's position, and the scenes' positions
            // were taken: its floor point raised by its shoes.
            let at = a.position + Vec3::Y * a.character.shoe_thickness;
            let goal = f
                .walk
                .as_ref()
                .and_then(|w| w.goal)
                .map_or(at, |g| Vec3::new(g[0], at.y, g[1]));
            let spot = (at.to_array(), goal.to_array());
            actors.insert(f.name.clone(), spot);
            if let Some(noun) = &f.noun {
                actors.entry(noun.clone()).or_insert(spot);
            }
        }
        let positions = self
            .positions
            .iter()
            .map(|(name, (p, _))| (name.clone(), p.to_array()))
            .collect();
        gk3_game::api::Whereabouts { actors, positions }
    }

    /// Where someone is, by noun or model: an actor's floor point, or the
    /// middle of anything else.
    fn spot(&self, who: &str) -> Option<Vec3> {
        self.actor_position(who)
            .map(|(p, _)| p)
            .or_else(|| self.center(who))
    }

    /// The floor a figure covers, as world min x, min z, max x, max z.
    pub fn footprint(&self, name: &str) -> Option<[f32; 4]> {
        let f = &self.figures[self.find(name)?];
        // Not drawn yet (as when the room is just set up): pose it now.
        let fresh: Vec<Vec3>;
        let points = if f.world_vertices.is_empty() || f.dirty {
            fresh = vertices(f).iter().map(|v| Vec3::from(v.position)).collect();
            &fresh
        } else {
            &f.world_vertices
        };
        if points.is_empty() {
            return None;
        }
        let (min, max) = points.iter().fold((Vec3::MAX, Vec3::MIN), |(lo, hi), v| {
            (lo.min(*v), hi.max(*v))
        });
        Some([min.x, min.z, max.x, max.z])
    }

    /// The middle of a figure's posed vertices, for walking up to it.
    /// The light on the floor about someone or something, which lights
    /// them, 0 to 1; None if lit evenly.
    pub fn floor_light_of(&self, name: &str) -> Option<[f32; 3]> {
        let f = &self.figures[self.find(name)?];
        f.fixed_light.is_none().then_some(f.floor_light.colour)
    }

    /// Whether someone or something is drawn.
    pub fn is_visible(&self, name: &str) -> Option<bool> {
        self.find(name).map(|i| self.figures[i].visible)
    }

    pub fn center(&self, name: &str) -> Option<Vec3> {
        self.bounds(name).map(|(min, max)| (min + max) * 0.5)
    }

    /// Remakes every model's texture bindings, after a change of
    /// filtering.
    pub fn rebind(&mut self, renderer: &Renderer) {
        for f in &mut self.figures {
            renderer.rebind_model(&mut f.gpu);
            if let Some(shadow) = &mut f.shadow {
                renderer.rebind_model(shadow);
            }
        }
    }

    /// A figure's model name.
    pub fn name_of(&self, i: usize) -> String {
        self.figures.get(i).map_or(String::new(), |f| f.name.clone())
    }

    /// The noun a figure answers to, if any.
    pub fn noun_of(&self, i: usize) -> Option<String> {
        self.figures.get(i)?.noun.clone()
    }

    /// The box around a figure's posed vertices.
    /// Draws a character the camera is in (or so close that it cuts into
    /// them) one-sided, so the camera sees out through them rather than
    /// their insides; everyone else two-sided, as the original, since some
    /// of their triangles face inwards and would show as holes.
    pub fn cull_around(&mut self, eye: Vec3) {
        for f in &mut self.figures {
            let around = f.actor.is_some()
                && f.world_vertices.first().is_some_and(|first| {
                    let (lo, hi) = f
                        .world_vertices
                        .iter()
                        .fold((*first, *first), |(lo, hi), v| (lo.min(*v), hi.max(*v)));
                    let margin = Vec3::splat(CAMERA_CUTS_WITHIN);
                    eye.cmpge(lo - margin).all() && eye.cmple(hi + margin).all()
                });
            f.gpu.one_sided = around;
        }
    }

    pub fn bounds(&self, name: &str) -> Option<(Vec3, Vec3)> {
        let f = &self.figures[self.find(name)?];
        if f.world_vertices.is_empty() {
            return None;
        }
        Some(
            f.world_vertices
                .iter()
                .fold((Vec3::MAX, Vec3::MIN), |(lo, hi), v| {
                    (lo.min(*v), hi.max(*v))
                }),
        )
    }

    /// The nearest visible figure a ray meets, but `skip`: how far, and its
    /// model name and noun.
    pub fn ray(
        &self,
        origin: Vec3,
        direction: Vec3,
        skip: &str,
    ) -> Option<(f32, String, Option<String>)> {
        let mut best: Option<(f32, usize)> = None;
        for (i, f) in self.figures.iter().enumerate() {
            if !f.visible || f.name.eq_ignore_ascii_case(skip) {
                continue;
            }
            for t in &f.triangles {
                let [a, b, c] = t.map(|i| {
                    f.world_vertices
                        .get(i as usize)
                        .copied()
                        .unwrap_or(Vec3::ZERO)
                });
                if let Some(d) = ray_triangle(origin, direction, a, b, c)
                    && best.is_none_or(|(bd, _)| d < bd)
                {
                    best = Some((d, i));
                }
            }
        }
        best.map(|(d, i)| (d, self.figures[i].name.clone(), self.figures[i].noun.clone()))
    }

    /// Sends an actor along a route, playing their walk. The story's walks
    /// stop their fidget, which starts again when they arrive.
    pub fn walk(
        &mut self,
        lib: &Library,
        name: &str,
        path: Vec<[f32; 2]>,
        heading: Option<f32>,
        now: f64,
    ) -> bool {
        self.walk_within(lib, name, path, heading, Tolerance::EXACT, now)
    }

    /// A walk that finishes as `tolerance` says.
    pub fn walk_within(
        &mut self,
        lib: &Library,
        name: &str,
        path: Vec<[f32; 2]>,
        heading: Option<f32>,
        tolerance: Tolerance,
        now: f64,
    ) -> bool {
        let started = self.start_walk(lib, name, path, heading, false, now);
        if started
            && let Some(i) = self.find(name)
            && let Some(walk) = &mut self.figures[i].walk
        {
            walk.tolerance = tolerance;
        }
        started
    }

    /// A click too near to walk to, made while walking, as the original's
    /// (0x41d34a → 0x41dadd): the walk becomes the click's, which asks for
    /// no heading, and stops. Setting off or striding, the walker goes into
    /// the stop where they are (0x4187fa); already stopping, the turn to
    /// the old heading is dropped (0x4188e4). False if not walking.
    pub fn stop_walk(&mut self, name: &str) -> bool {
        let Some(walk) = self.find(name).and_then(|i| self.figures[i].walk.as_mut()) else {
            return false;
        };
        walk.path.clear();
        walk.goal = None;
        walk.heading = None;
        walk.face_within = None;
        walk.tolerance.face = None;
        true
    }

    /// Turns someone on the spot to face a heading, as the original's walks
    /// that do not move (`TurnToModel`, a walk to see something already in
    /// reach): a walk already at its end, turning to the heading at the
    /// walker's turning speed, with the walk's own tolerance (0x4187fa):
    /// none if already facing within `face` of it, else to within a
    /// quarter of it. False if no turn is needed.
    pub fn turn_on_spot(&mut self, name: &str, heading: f32, face: f32, now: f64) -> bool {
        let Some(i) = self.find(name) else {
            return false;
        };
        let f = &mut self.figures[i];
        let Some(a) = &f.actor else {
            return false;
        };
        if angle_between(a.heading, heading).abs() <= face {
            return false;
        }
        let here = [a.position.x, a.position.z];
        let ground = a.position.y;
        f.playing = None;
        f.gas = None;
        f.fidget = None;
        f.walk = Some(Walk {
            path: Vec::new(),
            clip: None,
            started: now,
            last: now,
            speed: DEFAULT_WALK_SPEED,
            heading: Some(heading),
            stance: None,
            opener: None,
            turning_start: false,
            turning: true,
            fidget: false,
            carried: false,
            cycle: f.world,
            cycle_started: now,
            cycle_ground: ground,
            opening: false,
            goal: Some(here),
            deadline: now + 10.0,
            opening_steps: Vec::new(),
            cycle_steps: Vec::new(),
            clip_time: -1.0,
            settle: None,
            tolerance: Tolerance {
                arrive: AT_END,
                face: Some(face),
            },
            face_within: Some(if face > FINAL_TURN_SHARPEN_ABOVE { face * 0.25 } else { face }),
            turned_right: false,
            arrow_opener: Vec::new(),
            arrow_cycle: Vec::new(),
            arrow_standing: None,
            builtin_turn: 0.0,
            set_off_heading: heading,
            steered: 0.0,
            pause: None,
            held_since: None,
            ignore_others: false,
            others: Vec::new(),
        });
        true
    }

    /// A walk a fidget script asked for: the script waits and goes on.
    pub fn walk_for_fidget(
        &mut self,
        lib: &Library,
        name: &str,
        path: Vec<[f32; 2]>,
        heading: Option<f32>,
        now: f64,
    ) -> bool {
        let started = self.start_walk(lib, name, path, heading, true, now);
        // The original's fidget walk: as `WalkTo` (0x41d29c).
        if started
            && let Some(i) = self.find(name)
            && let Some(walk) = &mut self.figures[i].walk
        {
            walk.tolerance = Tolerance::TO_SPOT;
        }
        if !started && let Some(i) = self.find(name) {
            // No way there: the script goes on regardless.
            self.figures[i].waiting_until = now;
        }
        started
    }

    fn start_walk(
        &mut self,
        lib: &Library,
        name: &str,
        path: Vec<[f32; 2]>,
        heading: Option<f32>,
        fidget: bool,
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
        let stance = self.figures[i]
            .actor
            .as_ref()
            .and_then(|a| a.character.start_anim.clone())
            .and_then(|anim| self.clips_for(lib, &anim, &model).into_iter().next())
            .map(|(_, c)| c);
        let stride = clip
            .as_ref()
            .and_then(|c| stride_speed(&self.figures[i], c));
        let speed = stride.unwrap_or(DEFAULT_WALK_SPEED);
        let from = self.figures[i]
            .actor
            .as_ref()
            .map_or(Vec3::ZERO, |a| a.position);
        let length: f32 = path
            .iter()
            .scan(from, |at, p| {
                let next = Vec3::new(p[0], at.y, p[1]);
                let d = (next - *at).length();
                *at = next;
                Some(d)
            })
            .sum();
        let goal = path.last().copied();
        let steps_of = |anim: Option<&String>| -> Vec<(u32, bool)> {
            anim.and_then(|a| read_anm(lib, a))
                .map_or(Vec::new(), |anm| {
                    anm.cues
                        .iter()
                        .filter_map(|c| match &c.what {
                            CueKind::Footstep { scuff, .. } => Some((c.frame, *scuff)),
                            _ => None,
                        })
                        .collect()
                })
        };
        let character = self.figures[i].actor.as_ref().map(|a| a.character.clone());
        // As the original (0x4186de): the first leg of the route 90
        // degrees or more to the right is set out on with the turn-right
        // start, as far to the left with the turn-left start, if the
        // character has them; however near.
        let heading_now = self.figures[i].actor.as_ref().map_or(0.0, |a| a.heading);
        let off = path.first().and_then(|&[x, z]| {
            let to = Vec2::new(x - from.x, z - from.z);
            (to.length() > 1e-3).then(|| angle_between(heading_now, to.x.atan2(to.y)))
        });
        let right_turn = off.is_some_and(|d| d >= std::f32::consts::FRAC_PI_2);
        let turn_anim = off.and_then(|d| {
            let c = character.as_ref()?;
            if d >= std::f32::consts::FRAC_PI_2 {
                c.start_turn_right.clone()
            } else if d <= -std::f32::consts::FRAC_PI_2 {
                c.start_turn_left.clone()
            } else {
                None
            }
        });
        let turner = turn_anim
            .as_ref()
            .and_then(|anim| self.clips_for(lib, anim, &model).into_iter().next())
            .map(|(_, c)| c);
        let turning_start = turner.is_some();
        let turned_right = turning_start && right_turn;
        // The heading model through the opening stride and the cycle, and
        // where it stands in the standing pose (the start's first frame).
        let start_name = character.as_ref().and_then(|c| c.start_anim.clone());
        let opener_name = if turning_start { turn_anim.clone() } else { start_name.clone() };
        let arrow_opener = opener_name
            .as_deref()
            .and_then(|a| self.heading_model_angles(lib, a, &model))
            .unwrap_or_default();
        let arrow_cycle = self.heading_model_angles(lib, &walk_anim, &model).unwrap_or_default();
        let arrow_standing = start_name
            .as_deref()
            .and_then(|a| self.heading_model_angles(lib, a, &model))
            .and_then(|v| v.first().copied());
        // Setting off: the opening stride is placed to carry the standing
        // body straight on.
        let set_off_turn = arrow_standing
            .zip(arrow_opener.first().copied())
            .map(|(standing, first)| angle_between(first, standing));
        let builtin_turn = match (turning_start, turned_right) {
            (false, _) => 0.0,
            (true, true) => BUILTIN_TURN,
            (true, false) => -BUILTIN_TURN,
        };
        let opener = turner.or_else(|| stance.clone());
        let cycle_steps = steps_of(Some(&walk_anim));
        let opening_anim = if turning_start {
            turn_anim.as_ref()
        } else {
            character.as_ref().and_then(|c| c.start_anim.as_ref())
        };
        let opening_steps = steps_of(opening_anim);
        let f = &mut self.figures[i];
        f.playing = None;
        if !fidget {
            f.gas = None;
            f.fidget = None;
        }
        f.walk = Some(Walk {
            path,
            clip,
            started: now,
            last: now,
            speed,
            heading,
            stance,
            opener,
            turning_start,
            turning: false,
            fidget,
            carried: stride.is_some(),
            cycle: f.world,
            cycle_started: now,
            cycle_ground: f.actor.as_ref().map_or(0.0, |a| a.position.y),
            opening: true,
            goal,
            // Steering can circle a corner now and then: never for long.
            deadline: now + 3.0 + 2.0 * (length / speed) as f64,
            opening_steps,
            cycle_steps,
            clip_time: -1.0,
            settle: None,
            tolerance: Tolerance::EXACT,
            face_within: None,
            turned_right,
            arrow_opener,
            arrow_cycle,
            arrow_standing,
            builtin_turn,
            set_off_heading: heading_now,
            steered: 0.0,
            pause: None,
            held_since: None,
            ignore_others: false,
            others: Vec::new(),
        });
        if let Some(walk) = &mut f.walk
            && walk.carried
        {
            let clip = walk.opener.clone().or_else(|| walk.clip.clone());
            if let (Some(clip), Some(a)) = (clip, &f.actor) {
                // How the standing pose (the start's first frame) is turned
                // at this heading, placed by the hip as standing walkers are.
                let standing = walk.stance.as_ref().map(|stance| {
                    let mut pose = f.pose.clone();
                    pose.apply(stance, 0.0, false);
                    let place = placement::place(&pose, &a.character, a.position, a.heading);
                    let z = place.transform_vector3(Vec3::Z);
                    z.x.atan2(z.z)
                });
                f.pose.apply(&clip, 0.0, false);
                walk.cycle = placement::place(&f.pose, &a.character, a.position, a.heading);
                if let Some(turn) = set_off_turn
                    && let Some(standing) = standing
                    && walk.opener.is_some()
                    && let Some((floor_point, _)) = placement::floor_and_facing(&f.pose, &a.character)
                {
                    walk.cycle = Mat4::from_translation(a.position)
                        * Mat4::from_rotation_y(standing + turn)
                        * Mat4::from_translation(-floor_point);
                }
            }
        }
        true
    }

    /// Every visible actor, for keeping walkers apart.
    pub fn walkers(&self) -> Vec<Walker> {
        self.figures
            .iter()
            .enumerate()
            .filter(|(_, f)| f.visible)
            .filter_map(|(index, f)| {
                let a = f.actor.as_ref()?;
                let w = f.walk.as_ref();
                Some(Walker {
                    index,
                    at: Vec2::new(a.position.x, a.position.z),
                    radius: f.radius,
                    moving: w.is_some_and(|w| !w.turning && w.pause.is_none()),
                    paused_until: w.and_then(|w| w.pause.as_ref().map(|p| p.until)),
                    held_since: w.and_then(|w| w.held_since),
                    ignore_others: w.is_some_and(|w| w.ignore_others),
                    goal: w.and_then(|w| w.goal),
                    path: w.map(|w| w.path.clone()).unwrap_or_default(),
                    others: w.map(|w| w.others.clone()).unwrap_or_default(),
                })
            })
            .collect()
    }

    /// Holds a walker up for `seconds`, standing (0x419270).
    pub fn pause_walk(&mut self, index: usize, now: f64, seconds: f64) {
        let Some(f) = self.figures.get_mut(index) else { return };
        let (pose, world) = (f.pose.clone(), f.world);
        let (Some(w), Some(a)) = (&mut f.walk, &mut f.actor) else { return };
        if w.turning {
            return;
        }
        let (held, yaw_to, started) = match &w.pause {
            Some(p) => (p.from.clone(), p.yaw_to, p.started),
            None => {
                let (yaw_to, from) = stand_from(w, a, &pose, world);
                (from, yaw_to, now)
            }
        };
        w.pause = Some(Pause {
            until: now + seconds,
            from: held,
            yaw_to,
            started,
        });
        w.held_since.get_or_insert(now);
    }

    /// Notes who is near a walker now.
    pub fn set_walk_others(&mut self, index: usize, others: Vec<usize>) {
        if let Some(w) = self.figures.get_mut(index).and_then(|f| f.walk.as_mut()) {
            w.others = others;
        }
    }

    /// Gives a walk under way a new route (0x4190cf's case 2).
    pub fn reroute_walk(&mut self, index: usize, path: Vec<[f32; 2]>) {
        if let Some(w) = self.figures.get_mut(index).and_then(|f| f.walk.as_mut())
            && !path.is_empty()
        {
            w.goal = path.last().copied();
            w.path = path;
        }
    }

    /// Sets a paused walk off again on a new route, as it was asked for
    /// (0x41938c); with no route, a walk that has given up minding others
    /// counts as there, and is put on its end.
    pub fn resume_walk(&mut self, lib: &Library, index: usize, path: Vec<[f32; 2]>, now: f64) {
        let Some(f) = self.figures.get_mut(index) else { return };
        let Some(old) = f.walk.take() else { return };
        let name = f.name.clone();
        if path.is_empty() {
            if old.ignore_others
                && let (Some([x, z]), Some(a)) = (old.goal, &mut f.actor)
            {
                a.position.x = x;
                a.position.z = z;
            } else {
                f.walk = Some(old);
                return;
            }
            f.waiting_until = now;
            return;
        }
        if self.start_walk(lib, &name, path, old.heading, old.fidget, now)
            && let Some(w) = &mut self.figures[index].walk
        {
            w.tolerance = old.tolerance;
            w.held_since = old.held_since;
            w.ignore_others = old.ignore_others;
        }
    }

    /// Stops minding other walkers for the rest of this walk.
    pub fn give_up_minding(&mut self, index: usize) {
        if let Some(w) = self.figures.get_mut(index).and_then(|f| f.walk.as_mut()) {
            w.ignore_others = true;
        }
    }

    /// What is left of someone's walk, for skipping it: the original skips
    /// a walk only while it sets off or strides (0x41d4f9: state below 3),
    /// not once it is stopping. None if not walking so.
    pub fn walk_left(&self, name: &str) -> Option<WalkLeft> {
        let f = &self.figures[self.find(name)?];
        let w = f.walk.as_ref()?;
        if w.turning || w.path.is_empty() {
            return None;
        }
        Some(WalkLeft {
            path: w.path.clone(),
            heading: w.heading,
            tolerance: w.tolerance,
            radius: f.radius,
        })
    }

    /// Ends a walk at once, standing on a spot, as the original's skipped
    /// walk arrives (0x418564): put there, in the stance, and free to idle
    /// again as on arriving.
    pub fn end_walk_at(&mut self, lib: &Library, name: &str, at: Vec3, heading: f32, now: f64) {
        let Some(i) = self.find(name) else { return };
        self.put(name, at, heading);
        self.figures[i].playing = None;
        self.stand_in_stance(lib, i);
        self.figures[i].waiting_until = now;
    }

    /// The props shown now: models with no actor.
    pub fn shown_props(&self) -> Vec<String> {
        self.figures
            .iter()
            .filter(|f| f.actor.is_none() && f.visible)
            .map(|f| f.name.clone())
            .collect()
    }

    pub fn is_walking(&self, name: &str) -> bool {
        self.find(name)
            .is_some_and(|i| self.figures[i].walk.is_some())
    }

    /// Advances every walk, animation, face and idle script to `now`.
    pub fn update(&mut self, lib: &Library, now: f64) {
        self.update_faces(lib, now);
        let dt = (now - self.last_update).max(0.0) as f32;
        self.last_update = now;
        for i in 0..self.figures.len() {
            if self.figures[i].head.is_some() {
                self.update_head(i, now, dt);
            }
        }
        for i in 0..self.figures.len() {
            if let Some(walk) = &self.figures[i].walk {
                let fidget = walk.fidget;
                // (A walk held up for someone is not overdue: it is set off
                // again with time of its own.)
                let overdue = walk.carried
                    && !walk.turning
                    && walk.pause.is_none()
                    && !walk.path.is_empty()
                    && now > walk.deadline;
                // Where they had got to, for the log.
                let short = overdue
                    .then(|| {
                        let at = self.figures[i].actor.as_ref()?.position;
                        let end = walk.goal?;
                        let left = Vec2::new(end[0] - at.x, end[1] - at.z).length();
                        Some(format!(
                            "{left:.0} short, at {:.0},{:.0}, {} corners to go",
                            at.x,
                            at.z,
                            walk.path.len()
                        ))
                    })
                    .flatten();
                step_walk(&mut self.figures[i], &self.floor, now, &mut self.footsteps);
                if let Some(short) = short {
                    self.gave_up.push((self.figures[i].name.clone(), short));
                }
                // As G-Engine's walker: idle again on arriving, unless the
                // fidget sent them.
                if self.figures[i].walk.is_none() && !fidget {
                    self.start_fidget_at(lib, i, Fidget::Idle);
                }
                continue;
            }
            let figure = &mut self.figures[i];
            if let Some(m) = &figure.morph {
                let t = if m.seconds > 0.0 {
                    ((now - m.started) / m.seconds as f64).clamp(0.0, 1.0) as f32
                } else {
                    1.0
                };
                figure.pose = m.to.clone();
                figure.pose.blend_from(&m.from, 1.0 - t);
                figure.dirty = true;
                if t >= 1.0 {
                    figure.morph = None;
                }
                continue;
            }
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
                    // The model stays as the clip left it, and the actor
                    // with it, so whatever plays next carries on from there.
                    sync_actor(figure);
                    figure.playing = None;
                } else {
                    continue;
                }
            }
            if !figure.visible {
                continue;
            }
            if let Some(cleanup) = figure.cleanup.pop_front() {
                self.launch(lib, &cleanup, false, false, now);
                continue;
            }
            let Some(mut gas) = figure.gas.take() else {
                continue;
            };
            if gas.conditions().next().is_some() {
                let holds: Vec<bool> = gas
                    .conditions()
                    .map(|near| {
                        let me = self.figures[i].name.clone();
                        let other = near.other.as_deref().unwrap_or(&me);
                        match (self.spot(&near.noun), self.spot(other)) {
                            (Some(a), Some(b)) => {
                                let d = a.distance(b);
                                if near.away {
                                    d >= near.distance
                                } else {
                                    d <= near.distance
                                }
                            }
                            _ => false,
                        }
                    })
                    .collect();
                gas.judge(&holds);
            }
            if now < self.figures[i].waiting_until {
                self.figures[i].gas = Some(gas);
                continue;
            }
            let mut keep = true;
            // A few tries, so a missing animation does not stall the script.
            for _ in 0..4 {
                match gas.step() {
                    Request::Play { anim, looping } => {
                        // The whole animation plays, set piece or not, on
                        // whoever it animates.
                        if let Some(seconds) = self.launch(lib, &anim, looping, false, now) {
                            // Nothing of theirs moves (a texture swapped, a
                            // sound): the script waits the animation out.
                            if self.figures[i].playing.is_none() {
                                self.figures[i].waiting_until = now + seconds as f64;
                            }
                            break;
                        }
                    }
                    Request::Wait(seconds) => {
                        self.figures[i].waiting_until = now + seconds as f64;
                        break;
                    }
                    Request::Walk(choices) => {
                        let name = self.figures[i].name.clone();
                        self.walk_requests.push((name, WalkGoal::Named(choices)));
                        self.figures[i].waiting_until = f64::INFINITY;
                        break;
                    }
                    Request::WalkToPoint(p) => {
                        let name = self.figures[i].name.clone();
                        self.walk_requests
                            .push((name, WalkGoal::Point(Vec3::from(p))));
                        self.figures[i].waiting_until = f64::INFINITY;
                        break;
                    }
                    Request::NewIdle(name) => {
                        if let Some(next) = load_gas(lib, &name) {
                            gas = next;
                        }
                    }
                    Request::Stop => {
                        keep = false;
                        break;
                    }
                }
            }
            for (noun, parts, seconds) in std::mem::take(&mut gas.looks) {
                let me = self.figures[i].name.clone();
                match noun {
                    Some(noun) => {
                        let eyes = parts.is_empty() || parts.contains(['E', 'e']);
                        let head = parts.contains(['H', 'h']);
                        let seconds = seconds.map(|s| s as f64);
                        self.look_with(
                            &me,
                            LookTarget::Noun(noun),
                            seconds,
                            now,
                            eyes,
                            head,
                            false,
                        );
                    }
                    None => self.look_cancel(&me, now),
                }
            }
            if !gas.moved_to.is_empty() {
                let who = self.figures[i]
                    .noun
                    .clone()
                    .unwrap_or_else(|| self.figures[i].name.clone());
                for location in gas.moved_to.drain(..) {
                    self.moved.push((who.clone(), location));
                }
            }
            if keep {
                self.figures[i].gas = Some(gas);
            }
        }
    }

    /// Writes changed figures' vertices to the GPU.
    pub fn upload(&mut self, renderer: &Renderer) {
        self.upload_shapes(renderer);
        self.light(renderer);
    }

    /// Sends the cast as it now stands, taking the floor's light about each
    /// model in between, as the original does each frame (0x4b05e9): the
    /// floor below the model's bounds as now posed, then the light it is
    /// drawn with, so the first frame drawn has its first sample.
    pub fn upload_lit(
        &mut self,
        renderer: &Renderer,
        sample: &dyn Fn([f32; 3]) -> Option<[f32; 3]>,
    ) {
        self.upload_shapes(renderer);
        self.sample_floor_light(sample);
        self.light(renderer);
    }

    fn upload_shapes(&mut self, renderer: &Renderer) {
        for figure in &mut self.figures {
            figure.gpu.visible = figure.visible;
            for (submesh, name) in figure.repaint.drain(..) {
                let Some(image) = self.paints.get(&name) else {
                    continue;
                };
                let texture = self
                    .gpu_paints
                    .entry(name)
                    .or_insert_with(|| renderer.model_texture(image));
                renderer.set_model_texture(&mut figure.gpu, submesh, texture);
            }
            if let Some(face) = &mut figure.face
                && let Some(image) = face.image.take()
            {
                let texture = renderer.model_texture(&image);
                for &submesh in &face.submeshes {
                    renderer.set_model_texture(&mut figure.gpu, submesh, &texture);
                }
            }
            if figure.dirty {
                figure.dirty = false;
                let verts = vertices(figure);
                figure.world_vertices = verts.iter().map(|v| Vec3::from(v.position)).collect();
                renderer.update_model(&figure.gpu, &verts);
                figure.bounds = model_bounds(figure);
                if let Some(shadow) = &figure.shadow {
                    let quad = figure.bounds.and_then(|b| shadow_quad(b, &self.floor));
                    figure.shadow_found = quad.is_some();
                    if let Some(quad) = quad {
                        renderer.update_model(shadow, &quad);
                    }
                }
            }
            if let Some(shadow) = &mut figure.shadow {
                shadow.visible = figure.visible && figure.shadow_on && figure.shadow_found;
            }
        }
    }

    fn light(&self, renderer: &Renderer) {
        for figure in &self.figures {
            // Lit every frame, as the original sets a model up to draw
            // (0x558c80): from the light to the middle of the model; its own
            // colour if lit evenly, else the floor's (which eases in over a
            // few frames). With its default "2x lighting" on, and a card that
            // can modulate twice over, the colour is capped at 1 and the
            // range halved.
            let mut light = ModelLight::default();
            if let (Some(sun), Some((low, high))) = (self.light, figure.bounds) {
                light.direction = (sun - (low + high) * 0.5).into();
            }
            let colour = figure.fixed_light.unwrap_or(figure.floor_light.colour);
            light.colour = colour.map(|c| c.min(1.0));
            light.range = figure.ambient_range.unwrap_or(DEFAULT_AMBIENT_RANGE) * 0.5;
            renderer.set_model_light(&figure.gpu, light);
        }
    }

    /// Takes the light on the floor about each model that is not lit evenly,
    /// as the original does each frame (0x4b05e9): once it has moved more
    /// than a unit across the floor since it was last taken, it is taken
    /// again where it stands, and eased in over four frames (the light is
    /// the mean of the last four taken).
    fn sample_floor_light(&mut self, sample: &dyn Fn([f32; 3]) -> Option<[f32; 3]>) {
        for f in &mut self.figures {
            let Some((low, high)) = f.bounds.filter(|_| f.fixed_light.is_none()) else {
                continue;
            };
            // The middle of the model as drawn (0x4b07e9), whence the
            // original looks straight down at the floor.
            let at = (low + high) * 0.5;
            let s = &mut f.floor_light;
            if s.pending != 4
                && s.taken_at.is_none_or(|(x, z)| (at.x - x).powi(2) + (at.z - z).powi(2) > 1.0)
            {
                s.pending = 4;
            }
            if s.pending == 0 {
                continue;
            }
            if s.pending == 4 {
                if let Some(c) = sample(at.into()) {
                    // The first is taken as the light outright, as the
                    // original does when it restores a game (0x4b050b),
                    // rather than eased in from half: a deliberate change,
                    // so a room is lit right from its first frame.
                    if s.taken_at.is_none() {
                        s.ring = [c; 4];
                    }
                    s.push(c);
                }
            } else {
                let latest = s.ring[(s.next + 3) % 4];
                s.push(latest);
            }
            s.pending -= 1;
            s.taken_at = Some((at.x, at.z));
        }
    }

    pub fn models(&self) -> Vec<&GpuModel> {
        self.figures
            .iter()
            .flat_map(|f| std::iter::once(&f.gpu).chain(&f.shadow))
            .collect()
    }

    pub fn set_floor(&mut self, triangles: Vec<[Vec3; 3]>) {
        self.floor = triangles;
        for i in 0..self.figures.len() {
            if self.figures[i].walk.is_none() {
                self.snap_to_floor(i);
            }
            self.figures[i].dirty = true;
        }
    }

    /// Stands an actor on the floor under them, model and all, as
    /// G-Engine's `GKActor::SnapToFloor` whenever an actor is placed: the
    /// positions the rooms give are often a few units above or below it.
    fn snap_to_floor(&mut self, i: usize) {
        let f = &mut self.figures[i];
        let Some(a) = &mut f.actor else { return };
        let Some(y) = floor_under(&self.floor, a.position) else {
            return;
        };
        let rise = y - a.position.y;
        a.position.y = y;
        f.world = Mat4::from_translation(Vec3::Y * rise) * f.world;
        f.dirty = true;
    }

    /// `SetModelLighting`: lit evenly in one colour from now on.
    /// `SetModelLighting(model, range, r, g, b)`: lit evenly in this colour
    /// (over 512) and with this range, as the original (0x4adf4d).
    pub fn set_fixed_light(&mut self, name: &str, range: f32, rgb: [f32; 3]) -> bool {
        let Some(i) = self.find(name) else {
            return false;
        };
        let f = &mut self.figures[i];
        f.fixed_light = Some(rgb.map(|c| c / 512.0));
        f.ambient_range = Some(range);
        f.dirty = true;
        true
    }

    /// `EnableModelShadow` and `DisableModelShadow`.
    pub fn set_shadow(&mut self, name: &str, on: bool) -> bool {
        let Some(i) = self.find(name) else {
            return false;
        };
        self.figures[i].shadow_on = on;
        true
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
            self.start_fidget_at(lib, i, kind);
            // No script for it now: it stops.
            if self.figures[i].fidget.is_none() {
                self.figures[i].gas = None;
            }
        }
        Some(old)
    }

    /// Starts an actor's idle, talk or listen fidget, as G-Engine does: not
    /// if it is already the one running, and only if the actor has a script
    /// for it.
    pub fn start_fidget(&mut self, lib: &Library, who: &str, kind: Fidget) {
        if let Some(i) = self.find(who) {
            self.start_fidget_at(lib, i, kind);
        }
    }

    fn start_fidget_at(&mut self, lib: &Library, i: usize, kind: Fidget) {
        let figure = &mut self.figures[i];
        if figure.fidget == Some(kind) {
            return;
        }
        if let Some(gas) = figure.fidgets[kind as usize]
            .as_deref()
            .and_then(|g| load_gas(lib, g))
        {
            // As G-Engine's `Interrupt`: what the old script is playing
            // finishes, and its cleanup (for talking, if that is what is
            // starting) tidies up before the new one begins.
            let talking = kind != Fidget::Idle;
            if let Some(old) = figure.gas.take()
                && figure.playing.as_ref().is_some_and(|p| p.fidgeting)
            {
                figure.cleanup = old.cleanups(talking).into();
            }
            figure.gas = Some(gas);
            figure.fidget = Some(kind);
            figure.waiting_until = 0.0;
        }
    }

    /// Stops an actor's fidget, as G-Engine's `StopFidget`: if the script
    /// has a cleanup for what it is playing, that finishes and the cleanups
    /// play, one after another. Returns how long all that takes, which the
    /// story waits for.
    pub fn stop_fidget(&mut self, lib: &Library, who: &str, now: f64) -> f64 {
        let Some(i) = self.find(who) else { return 0.0 };
        let figure = &mut self.figures[i];
        figure.fidget = None;
        let Some(gas) = figure.gas.take() else {
            return 0.0;
        };
        let cleanups = gas.cleanups(false);
        if cleanups.is_empty() {
            return 0.0;
        }
        let remaining = match &figure.playing {
            Some(p) if p.fidgeting && !p.looping => {
                (p.frames as f64 / FRAMES_PER_SECOND as f64 - (now - p.started)).max(0.0)
            }
            Some(p) if p.fidgeting => {
                figure.playing = None;
                0.0
            }
            _ => 0.0,
        };
        let length: f64 = cleanups
            .iter()
            .map(|c| read_anm(lib, c).map_or(0.0, |a| a.frames as f64 / FRAMES_PER_SECOND as f64))
            .sum();
        figure.cleanup = cleanups.into();
        remaining + length
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

    /// The nearest visible figure on a ray, noun or no: its distance and
    /// model name.
    pub fn pick_any(&self, origin: Vec3, direction: Vec3) -> Option<(f32, String)> {
        let mut best: Option<(f32, String)> = None;
        for f in self.figures.iter().filter(|f| f.visible) {
            for t in &f.triangles {
                let [a, b, c] = t.map(|i| f.world_vertices.get(i as usize).copied().unwrap_or(Vec3::ZERO));
                if let Some(d) = ray_triangle(origin, direction, a, b, c)
                    && best.as_ref().is_none_or(|(bd, _)| d < *bd)
                {
                    best = Some((d, f.name.clone()));
                }
            }
        }
        best
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

    /// How far along a ray one figure, if shown, is first met.
    pub fn ray_figure(&self, name: &str, origin: Vec3, direction: Vec3) -> Option<f32> {
        let f = self.figures.get(self.find(name)?).filter(|f| f.visible)?;
        f.triangles
            .iter()
            .filter_map(|t| {
                let [a, b, c] = t.map(|i| f.world_vertices.get(i as usize).copied().unwrap_or(Vec3::ZERO));
                ray_triangle(origin, direction, a, b, c)
            })
            .min_by(f32::total_cmp)
    }

    /// Whether an animation is still moving anyone.
    pub fn is_playing(&self, anim: &str) -> bool {
        self.figures
            .iter()
            .any(|f| f.playing.as_ref().is_some_and(|p| p.anim.eq_ignore_ascii_case(anim)))
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

struct Morph {
    from: Pose,
    to: Pose,
    started: f64,
    seconds: f32,
}

struct Start {
    looping: bool,
    scripted: bool,
    absolute: Option<Mat4>,
    /// Where an actor in a relative animation was put, for its props.
    reference: Option<Mat4>,
    now: f64,
}

/// Stands an actor where their model is, facing its way, as G-Engine's
/// `SyncActorToModelPositionAndRotation`.
fn sync_actor(figure: &mut Figure) {
    if let Some(a) = &mut figure.actor
        && let Some((floor, facing)) = placement::floor_and_facing(&figure.pose, &a.character)
    {
        a.position = figure.world.transform_point3(floor);
        a.placed = true;
        let facing = figure
            .world
            .transform_vector3(facing.unwrap_or(Vec3::NEG_Z));
        a.heading = facing.x.atan2(facing.z);
    }
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

impl Cast {
    /// The direction, in the model's space, a character faces at each
    /// frame of an animation, read as the original reads facing while an
    /// animation animates it (0x41c054): from the heading model
    /// `DOR_<model>`, an arrow of three vertices whose tip is the one
    /// across from the shortest edge. None without one, or if the
    /// animation does not animate it.
    fn heading_model_angles(&mut self, lib: &Library, anim: &str, model: &str) -> Option<Vec<f32>> {
        let dor = format!("DOR_{model}");
        let arrow = Model::parse(&lib.read(&format!("{dor}.MOD")).ok()?).ok()?;
        let (_, clip) = self.clips_for(lib, anim, &dor).into_iter().next()?;
        let mut pose = Pose::rest(&arrow);
        (0..clip.frame_count.max(1))
            .map(|frame| {
                pose.apply(&clip, frame as f32, false);
                arrow_heading(&pose)
            })
            .collect()
    }

    /// Whether a character's walk cycle carries them backwards by the
    /// facing read from their hip mesh: then that reading is the wrong way
    /// round for this model. G-Engine reads facing from a helper model
    /// instead (`DOR_GAB` and so on), which not every model has.
    /// The original's reference for a character's facing, from their
    /// model posed in the first frame of their walk's start, as the
    /// original takes it when it sets up their walking (0x4172cf).
    fn hip_reference(&mut self, lib: &Library, figure: &Figure) -> Option<gk3_game::character::HipReference> {
        let a = figure.actor.as_ref()?;
        let start = a.character.start_anim.clone()?;
        let mut pose = Pose::rest(&figure.model);
        for (_, clip) in self.clips_for(lib, &start, &figure.name) {
            pose.apply(&clip, 0.0, false);
        }
        let mesh = figure.model.meshes.get(a.character.hip.mesh)?;
        let trailing = |group: usize| mesh.submeshes.get(group).map_or(0, |s| s.trailing_points);
        placement::hip_reference(&pose, &a.character, &trailing)
    }

    fn facing_backwards(&mut self, lib: &Library, figure: &Figure) -> bool {
        let Some(a) = &figure.actor else { return false };
        let Some(walk) = a.character.walk_anim.clone() else {
            return false;
        };
        let Some((_, clip)) = self.clips_for(lib, &walk, &figure.name).into_iter().next() else {
            return false;
        };
        let mut pose = figure.pose.clone();
        pose.apply(&clip, 0.0, false);
        let Some((start, Some(facing))) = placement::floor_and_facing(&pose, &a.character) else {
            return false;
        };
        pose.apply(&clip, clip.frame_count.saturating_sub(1) as f32, false);
        let Some((end, _)) = placement::floor_and_facing(&pose, &a.character) else {
            return false;
        };
        let travel = Vec3::new(end.x - start.x, 0.0, end.z - start.z);
        travel.length() > 1.0 && travel.dot(facing) < 0.0
    }
}

/// Standing still from part way through a walk: the heading to stand at
/// and how the standing model is turned, so the body carries straight on,
/// and the pose stopped in, turned about its floor point to stand turned
/// so too, ready to ease from without the body folding. With a heading
/// model, the standing model is turned so its direction goes on from the
/// stride's; part way through a turning start without one, the walker faces
/// where the hip now points. Sets the actor's heading.
fn stand_from(walk: &Walk, actor: &mut ActorPlacement, pose: &Pose, world: Mat4) -> (f32, Pose) {
    let yaw_of = |m: &Mat4| {
        let x = m.transform_vector3(Vec3::X);
        (-x.z).atan2(x.x)
    };
    let yaw_from = yaw_of(&world);
    // Which way the standing pose's hip points in the model: standing at a
    // heading turns the model by the heading less that (`placement::place`).
    let stance_hip = walk.stance.as_ref().and_then(|stance| {
        let mut standing = pose.clone();
        standing.apply(stance, 0.0, false);
        let (_, f) = placement::floor_and_facing(&standing, &actor.character)?;
        f.map(|f| f.x.atan2(f.z))
    });
    let arrows = if walk.opening { &walk.arrow_opener } else { &walk.arrow_cycle };
    let frame = (walk.clip_time.max(0.0) as usize).min(arrows.len().saturating_sub(1));
    if let (Some(standing), Some(now_arrow), Some(hip)) = (walk.arrow_standing, arrows.get(frame), stance_hip) {
        actor.heading = angle_between(0.0, yaw_from + angle_between(standing, *now_arrow) + hip);
    } else if walk.opening
        && walk.turning_start
        && let Some((_, Some(facing))) = placement::floor_and_facing(pose, &actor.character)
    {
        let facing = world.transform_vector3(facing);
        actor.heading = facing.x.atan2(facing.z);
    }
    let yaw_to = stance_hip.map_or(yaw_from, |hip| actor.heading - hip);
    let mut from = pose.clone();
    if let Some((floor_point, _)) = placement::floor_and_facing(pose, &actor.character) {
        from.turn_about(angle_between(yaw_to, yaw_from), floor_point);
    }
    (yaw_to, from)
}

/// How near another walker may come to an actor: half the model's
/// breadth across the floor, at most 36, as the original's (0x41985e).
fn collision_radius(figure: &Figure) -> f32 {
    // As the original (0x41985e): from its bounds (`model_bounds`), the
    // length of half their width and depth together, at most 36.
    let Some((low, high)) = model_bounds(figure) else { return 0.0 };
    let half = (high - low) * 0.5;
    Vec2::new(half.x, half.z).length().min(36.0)
}

/// The heading a heading model's arrow points, in its model's space: from
/// one end of its shortest edge to the vertex across from it (0x41c14e).
fn arrow_heading(pose: &Pose) -> Option<f32> {
    let mesh = pose.meshes.first()?;
    let p: Vec<Vec3> = (0..3).map(|v| mesh.point(0, v)).collect::<Option<_>>()?;
    let (d01, d02, d12) = (p[0].distance(p[1]), p[0].distance(p[2]), p[1].distance(p[2]));
    let (tip, base) = if d01 <= d02.min(d12) {
        (2, 1)
    } else if d02 <= d12 {
        (1, 0)
    } else {
        (0, 1)
    };
    let d = p[tip] - p[base];
    Some(d.x.atan2(d.z))
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

/// How fast a standing actor turns, radians a second, and how near a
/// heading counts as there: G-Engine's walker's.
const TURN_SPEED: f32 = std::f32::consts::TAU;
const AT_HEADING: f32 = 4.0 * std::f32::consts::PI / 180.0;
/// Turning while walking: slower the farther the next corner and the more
/// nearly they face it, as G-Engine's `GetWalkTurnSpeed`.
const WALK_TURN_FASTEST: f32 = std::f32::consts::TAU * 1.5;
const WALK_TURN_SLOWEST: f32 = std::f32::consts::FRAC_PI_2;

fn walk_turn_speed(heading: f32, to: Vec3) -> f32 {
    let distance = Vec3::new(to.x, 0.0, to.z).length();
    let far = ((distance - 30.0) / 70.0).clamp(0.0, 1.0);
    let off = angle_between(heading, to.x.atan2(to.z)).abs() / std::f32::consts::PI;
    let t = far.min(1.0 - off);
    let eased = 1.0 - (1.0 - t).powi(3);
    WALK_TURN_FASTEST + (WALK_TURN_SLOWEST - WALK_TURN_FASTEST) * eased
}

/// The signed turn from one heading to another, the short way round.
fn angle_between(from: f32, to: f32) -> f32 {
    let d = (to - from).rem_euclid(std::f32::consts::TAU);
    if d > std::f32::consts::PI {
        d - std::f32::consts::TAU
    } else {
        d
    }
}

/// How far the turning starts turn the walker themselves, right or (less)
/// left: the original's 135° and 225° (0x666188, 0x66618c).
const BUILTIN_TURN: f32 = 2.356_194_5;

/// Above this, a walk's heading tolerance is quartered for its final
/// turn, as the original's (3.6 degrees).
const FINAL_TURN_SHARPEN_ABOVE: f32 = 0.062_831_86;

/// Turns a heading towards another by at most `step`, never past it; true
/// once within `within` of it, where it stops.
fn turn_within(heading: &mut f32, target: f32, step: f32, within: f32) -> bool {
    let d = angle_between(*heading, target);
    if d.abs() <= within {
        return true;
    }
    *heading += d.signum() * step.min(d.abs());
    angle_between(*heading, target).abs() <= within
}

/// Turns a heading towards another by at most `step`; true once there.
fn turn_towards(heading: &mut f32, target: f32, step: f32) -> bool {
    let d = angle_between(*heading, target);
    if d.abs() - step <= AT_HEADING {
        *heading = target;
        true
    } else {
        *heading += step.copysign(d);
        false
    }
}

/// Moves a walking actor along their route and poses their stride. The clip
/// is played for its pose; placing the model on the actor every frame takes
/// its own forward travel back out, so the route decides where they go.
/// They turn into each leg rather than snapping round, and on arriving take
/// their opening stance and turn to face the way asked, as G-Engine's walker.
fn step_walk(figure: &mut Figure, floor: &[[Vec3; 3]], now: f64, steps: &mut Vec<(String, bool)>) {
    let who = figure.noun.clone().unwrap_or_else(|| figure.name.clone());
    let Some(walk) = &mut figure.walk else { return };
    let Some(actor) = &mut figure.actor else {
        figure.walk = None;
        return;
    };
    let dt = (now - walk.last).max(0.0) as f32;
    walk.last = now;
    // Paused for another walker: standing, eased into from the stride.
    if let Some(p) = &walk.pause {
        let progress = ((now - p.started) / SETTLE_SECONDS).clamp(0.0, 1.0) as f32;
        let eased = progress * progress * (3.0 - 2.0 * progress);
        if let Some(stance) = &walk.stance {
            figure.pose.apply(stance, 0.0, false);
        }
        figure.pose.blend_from(&p.from, 1.0 - eased);
        if let Some((floor_point, _)) = placement::floor_and_facing(&figure.pose, &actor.character) {
            figure.world = Mat4::from_translation(actor.position)
                * Mat4::from_rotation_y(p.yaw_to)
                * Mat4::from_translation(-floor_point);
        }
        figure.dirty = true;
        return;
    }
    // A turning start turns the walker itself; steering waits until it is
    // over, but for a walker with a heading model, which is steered as the
    // original steers (0x418df1): once the heading model shows the turn
    // under way, by what the start's own turn leaves over.
    let steering = !(walk.opening && walk.turning_start);
    if !walk.turning && walk.carried && !steering && !walk.arrow_opener.is_empty() {
        let frame = ((now - walk.cycle_started) as f32 * FRAMES_PER_SECOND).max(0.0) as usize;
        let at = walk.arrow_opener[frame.min(walk.arrow_opener.len() - 1)];
        let turning = angle_between(walk.arrow_opener[0], at).abs() > 1e-3;
        if turning && let Some(&next) = walk.path.first() {
            let to = Vec3::new(next[0] - actor.position.x, 0.0, next[1] - actor.position.z);
            if to.length() > 0.01 {
                let facing = walk.set_off_heading + walk.builtin_turn + walk.steered;
                let want = angle_between(facing, to.x.atan2(to.z));
                let step = walk_turn_speed(facing, to) * dt;
                let turn = want.signum() * step.min(want.abs());
                walk.steered += turn;
                let p = actor.position;
                walk.cycle = Mat4::from_translation(p)
                    * Mat4::from_rotation_y(turn)
                    * Mat4::from_translation(-p)
                    * walk.cycle;
            }
        }
    }
    if !walk.turning && walk.carried {
        // Steer towards the next corner, turning the model about the actor.
        if let Some(&next) = walk.path.first().filter(|_| steering) {
            let to = Vec3::new(next[0] - actor.position.x, 0.0, next[1] - actor.position.z);
            if to.length() > 0.01 {
                let before = actor.heading;
                let speed = walk_turn_speed(actor.heading, to);
                turn_towards(&mut actor.heading, to.x.atan2(to.z), speed * dt);
                let turn = angle_between(before, actor.heading);
                let p = actor.position;
                walk.cycle = Mat4::from_translation(p)
                    * Mat4::from_rotation_y(turn)
                    * Mat4::from_translation(-p)
                    * walk.cycle;
            }
        }
        // The opening stride, then the cycle over and over; each starts
        // where the last left the actor, on the floor there.
        loop {
            let clip = if walk.opening {
                walk.opener.clone()
            } else {
                walk.clip.clone()
            };
            let Some(clip) = clip.or_else(|| walk.clip.clone()) else {
                break;
            };
            let last = clip.frame_count.saturating_sub(1).max(1) as f32;
            let t = (now - walk.cycle_started) as f32 * FRAMES_PER_SECOND;
            // Feet that have come down since the last frame.
            let marks = if walk.opening && walk.opener.is_some() {
                &walk.opening_steps
            } else {
                &walk.cycle_steps
            };
            for &(frame, scuff) in marks {
                let f = frame as f32;
                if f > walk.clip_time && f <= t.min(last) {
                    steps.push((who.clone(), scuff));
                }
            }
            if t < last {
                walk.clip_time = t;
                figure.pose.apply(&clip, t, false);
                figure.world = walk.cycle;
                break;
            }
            walk.clip_time = -1.0;
            figure.pose.apply(&clip, last, false);
            figure.world = walk.cycle;
            follow_model(
                figure_floor(&figure.pose, &actor.character, figure.world),
                actor,
            );
            walk.cycle_started += (last / FRAMES_PER_SECOND) as f64;
            // After a turning start, the walk goes on the way it left the
            // walker facing, from where the hip points; but see below for
            // characters with a heading model.
            let arrow_end = if walk.opening { walk.arrow_opener.last() } else { walk.arrow_cycle.last() }
                .copied();
            if walk.opening
                && walk.turning_start
                && let Some((_, Some(facing))) =
                    placement::floor_and_facing(&figure.pose, &actor.character)
            {
                let facing = figure.world.transform_vector3(facing);
                actor.heading = facing.x.atan2(facing.z);
            }
            let start_rotation = {
                let z = walk.cycle.transform_vector3(Vec3::Z);
                z.x.atan2(z.z)
            };
            // After the turn-right start, the cycle goes on from the
            // character's frame for it (0x4187d5), not its first.
            let first = if walk.opening && walk.turned_right {
                actor.character.start_turn_right_frame as f32
            } else {
                0.0
            };
            walk.opening = false;
            let Some(next) = walk.clip.clone() else { break };
            let first = first.min(next.frame_count.saturating_sub(2) as f32);
            if first > 0.0 {
                walk.cycle_started -= (first / FRAMES_PER_SECOND) as f64;
                walk.clip_time = first;
            }
            figure.pose.apply(&next, first, false);
            if let Some(ground) = floor_under(floor, actor.position) {
                actor.position.y = ground;
            }
            walk.cycle = placement::place(
                &figure.pose,
                &actor.character,
                actor.position,
                actor.heading,
            );
            // With a heading model (whose arrow follows the body, where the
            // hip does not quite), each clip is turned to carry the body
            // straight on from the last, as the original places each
            // animation by it; the heading is then read from the hip again.
            let arrow_next = walk.arrow_cycle.get(first as usize).copied();
            if let Some((end, next_first)) = arrow_end.zip(arrow_next)
                && let turn = angle_between(next_first, end)
                && let Some((floor_point, facing)) =
                    placement::floor_and_facing(&figure.pose, &actor.character)
            {
                let rotation = start_rotation + turn;
                walk.cycle = Mat4::from_translation(actor.position)
                    * Mat4::from_rotation_y(rotation)
                    * Mat4::from_translation(-floor_point);
                // The heading as the cycles to come will be placed by: from
                // the hip at the cycle's first frame (the hips swing through
                // a stride, so not the frame it picks up at).
                let mut at_first = figure.pose.clone();
                at_first.apply(&next, 0.0, false);
                let facing = placement::floor_and_facing(&at_first, &actor.character)
                    .and_then(|(_, f)| f)
                    .or(facing);
                if let Some(f) = facing {
                    let f = walk.cycle.transform_vector3(f);
                    actor.heading = f.x.atan2(f.z);
                }
            }
            walk.cycle_ground = actor.position.y;
        }
        follow_model(
            figure_floor(&figure.pose, &actor.character, figure.world),
            actor,
        );
        // Every frame the lowest shoe is stood on the floor, as G-Engine's
        // walker snaps to the floor: down slopes and up steps, and through
        // strides whose planted foot drifts up or down.
        // A kerb or step is eased over a few frames rather than popped.
        let target = floor_under(floor, actor.position).unwrap_or(walk.cycle_ground);
        let step = STEP_SPEED * dt;
        let ground = actor.position.y + (target - actor.position.y).clamp(-step, step);
        actor.position.y = ground;
        if let Some(model_floor) = figure_floor(&figure.pose, &actor.character, walk.cycle) {
            figure.world = Mat4::from_translation(Vec3::Y * (ground - model_floor.y)) * walk.cycle;
        }
        // As G-Engine's `AdvancePath`: the end counts as reached within
        // `AT_END`, any corner within `AT_CORNER`, and reaching one (the
        // furthest along, first) leaves out those before it. Nothing counts
        // as reached for being behind: that cut short walks to points just
        // behind or beside the walker, who then jumped there.
        let at = Vec2::new(actor.position.x, actor.position.z);
        let last = walk.path.len().saturating_sub(1);
        let reached = (0..walk.path.len()).rev().find(|&j| {
            let near = if j == last { walk.tolerance.arrive } else { AT_CORNER };
            at.distance(Vec2::from(walk.path[j])) <= near
        });
        if let Some(j) = reached {
            walk.path.drain(..=j);
        }
        if walk.path.is_empty() || now > walk.deadline {
            walk.path.clear();
            arrive(walk, actor, &figure.pose, figure.world, floor, now);
        }
    } else if !walk.turning {
        // A walk on the spot: slide along the route.
        let mut budget = walk.speed * dt;
        while budget > 0.0 {
            let Some(&next) = walk.path.first() else {
                break;
            };
            let to = Vec3::new(next[0], actor.position.y, next[1]) - actor.position;
            let distance = Vec3::new(to.x, 0.0, to.z).length();
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
        if let Some(&next) = walk.path.first() {
            let to = Vec3::new(next[0], actor.position.y, next[1]) - actor.position;
            if Vec3::new(to.x, 0.0, to.z).length() > 0.01 {
                let speed = walk_turn_speed(actor.heading, to);
                turn_towards(&mut actor.heading, to.x.atan2(to.z), speed * dt);
            }
        }
        if let Some(clip) = &walk.clip {
            let length = clip.frame_count.saturating_sub(1).max(1) as f32;
            let t = (now - walk.started) as f32 * FRAMES_PER_SECOND;
            figure.pose.apply(clip, t % length, true);
        }
        if walk.path.is_empty() {
            arrive(walk, actor, &figure.pose, figure.world, floor, now);
        }
    }
    let easing_to = walk.settle.as_ref().map(|s| s.yaw_to);
    if walk.turning {
        let settled = settle(walk, actor, &mut figure.pose, floor, now);
        let turned = match (walk.heading, walk.face_within) {
            (Some(h), Some(within)) => turn_within(&mut actor.heading, h, TURN_SPEED * dt, within),
            (Some(h), None) => turn_towards(&mut actor.heading, h, TURN_SPEED * dt),
            (None, _) => true,
        };
        if settled && turned {
            figure.walk = None;
            figure.waiting_until = now;
        }
    }
    if figure.walk.as_ref().is_none_or(|w| w.turning || !w.carried) {
        figure.world = placement::place(
            &figure.pose,
            &actor.character,
            actor.position,
            actor.heading,
        );
        // Easing into standing: turned as it will stand throughout, about
        // the floor point (the pose eased from was turned to match).
        if let Some(yaw_to) = easing_to
            && let Some((floor_point, _)) = placement::floor_and_facing(&figure.pose, &actor.character)
        {
            figure.world = Mat4::from_translation(actor.position)
                * Mat4::from_rotation_y(yaw_to)
                * Mat4::from_translation(-floor_point);
        }
    }
    figure.dirty = true;
}

/// Where a posed, placed model's floor point is.
fn figure_floor(pose: &Pose, character: &Character, world: Mat4) -> Option<Vec3> {
    placement::floor_and_facing(pose, character).map(|(f, _)| world.transform_point3(f))
}

/// The actor follows the model across the floor (their height is the
/// floor's, held while the model bobs).
fn follow_model(floor: Option<Vec3>, actor: &mut ActorPlacement) {
    if let Some(f) = floor {
        actor.position.x = f.x;
        actor.position.z = f.z;
    }
}

/// The end of the route: exactly where it was meant to end, standing as at
/// the start of a stride, then turning to face the way asked, as G-Engine's
/// walker does on `FollowPathEnd`.
fn arrive(
    walk: &mut Walk,
    actor: &mut ActorPlacement,
    pose: &Pose,
    world: Mat4,
    floor: &[[Vec3; 3]],
    now: f64,
) {
    walk.turning = true;
    let (yaw_to, from_pose) = stand_from(walk, actor, pose, world);
    // As the original's stop (0x4187fa): already facing within the walk's
    // tolerance, no turn; else a turn to within a quarter of it (if more
    // than 3.6 degrees), stopping there rather than on the heading.
    if let (Some(h), Some(face)) = (walk.heading, walk.tolerance.face) {
        if angle_between(actor.heading, h).abs() <= face {
            walk.heading = None;
        } else {
            walk.face_within = Some(if face > FINAL_TURN_SHARPEN_ABOVE { face * 0.25 } else { face });
        }
    }
    let from = Vec2::new(actor.position.x, actor.position.z);
    let near = walk
        .goal
        .is_none_or(|[x, z]| from.distance(Vec2::new(x, z)) <= SETTLE_REACH);
    if near {
        walk.settle = Some(Settle {
            from,
            pose: from_pose,
            yaw_to,
            started: now,
        });
        return;
    }
    if let Some([x, z]) = walk.goal {
        actor.position.x = x;
        actor.position.z = z;
    }
    if let Some(y) = floor_under(floor, actor.position) {
        actor.position.y = y;
    }
}

/// Eases an arrived walker onto the goal and into the stance (see
/// `Settle`); without a settle under way, just stands them in it. True once
/// they stand there.
fn settle(walk: &mut Walk, actor: &mut ActorPlacement, pose: &mut Pose, floor: &[[Vec3; 3]], now: f64) -> bool {
    let progress = walk
        .settle
        .as_ref()
        .map_or(1.0, |s| ((now - s.started) / SETTLE_SECONDS).clamp(0.0, 1.0) as f32);
    // Smoothstep: it slows into the stop rather than halting.
    let eased = progress * progress * (3.0 - 2.0 * progress);
    if let Some(stance) = &walk.stance {
        pose.apply(stance, 0.0, false);
    }
    if let Some(s) = &walk.settle {
        pose.blend_from(&s.pose, 1.0 - eased);
        if let Some([x, z]) = walk.goal {
            let at = s.from.lerp(Vec2::new(x, z), eased);
            actor.position.x = at.x;
            actor.position.z = at.y;
        }
    }
    if let Some(y) = floor_under(floor, actor.position) {
        actor.position.y = y;
    }
    if progress >= 1.0 {
        walk.settle = None;
    }
    progress >= 1.0
}

/// The floor's height under a point: the nearest of the floor's surfaces
/// above or below it, as G-Engine's `GetFloorInfo`.
/// The height of the floor nearest straight below a point.
fn floor_below(floor: &[[Vec3; 3]], at: Vec3) -> Option<f32> {
    floor
        .iter()
        .filter_map(|[a, b, c]| {
            ray_triangle(at, Vec3::NEG_Y, *a, *b, *c).or_else(|| ray_triangle(at, Vec3::NEG_Y, *a, *c, *b))
        })
        .filter(|d| *d >= 0.0)
        .min_by(|a, b| a.total_cmp(b))
        .map(|d| at.y - d)
}

fn floor_under(floor: &[[Vec3; 3]], at: Vec3) -> Option<f32> {
    let origin = Vec3::new(at.x, at.y + 50.0, at.z);
    floor
        .iter()
        .filter_map(|[a, b, c]| {
            ray_triangle(origin, Vec3::NEG_Y, *a, *b, *c)
                .or_else(|| ray_triangle(origin, Vec3::NEG_Y, *a, *c, *b))
        })
        .map(|d| origin.y - d)
        .min_by(|a, b| (a - at.y).abs().total_cmp(&(b - at.y).abs()))
}

/// How fast a walker rises or drops to a new floor height, units a second.
const STEP_SPEED: f32 = 120.0;

/// A character's turning head, if they have head-turn animations: one for
/// each way, each turning steadily to its furthest by its last frame.
fn head_rig(lib: &Library, cast: &mut Cast, figure: &Figure) -> Option<HeadRig> {
    let a = figure.actor.as_ref()?;
    let mesh = a.character.head_mesh?;
    let code = &figure.name;
    let mut clips = Vec::new();
    for way in ["L", "R", "Up", "Down"] {
        let (_, clip) = cast
            .clips_for(lib, &format!("{code}TurnHead{way}"), code)
            .into_iter()
            .next()?;
        let last = clip.frame_count.saturating_sub(1) as f32;
        let turn = clip_turn(&figure.pose, mesh, &clip, last);
        // How far it has turned: the angle of that rotation.
        let m = Mat3::from_mat4(turn);
        let reach = ((m.x_axis.x + m.y_axis.y + m.z_axis.z - 1.0) / 2.0)
            .clamp(-1.0, 1.0)
            .acos()
            .max(1e-3);
        clips.push((clip, reach));
    }
    let limits = a.character.head_fov.map(f32::to_radians);
    Some(HeadRig {
        mesh,
        limits,
        clips: clips.try_into().ok()?,
        angle: (0.0, 0.0),
        to: (0.0, 0.0),
        until: 0.0,
        follow: None,
        local: Mat4::IDENTITY,
    })
}

/// How a clip has turned the head mesh by a frame, in the head's own frame.
fn clip_turn(pose: &Pose, mesh: usize, clip: &VertexAnimation, frame: f32) -> Mat4 {
    let mut p = pose.clone();
    p.apply(clip, 0.0, false);
    let start = p.meshes[mesh].matrix();
    p.apply(clip, frame, false);
    start.inverse() * p.meshes[mesh].matrix()
}

/// The head's turn for its angle: from the left or right clip at the frame
/// matching the yaw, then the up or down one for the pitch.
fn head_turn(pose: &Pose, head: &HeadRig) -> Mat4 {
    let part = |clip: &(Arc<VertexAnimation>, f32), angle: f32| {
        let last = clip.0.frame_count.saturating_sub(1) as f32;
        let frame = (angle.abs() / clip.1).min(1.0) * last;
        clip_turn(pose, head.mesh, &clip.0, frame)
    };
    let (yaw, pitch) = head.angle;
    let turn = if yaw < 0.0 {
        part(&head.clips[0], yaw)
    } else {
        part(&head.clips[1], yaw)
    };
    let nod = if pitch > 0.0 {
        part(&head.clips[2], pitch)
    } else {
        part(&head.clips[3], pitch)
    };
    turn * nod
}

/// A blob shadow: a square on the floor, painted with `SHADOW.BMP`.
fn shadow_model(lib: &Library, renderer: &Renderer) -> GpuModel {
    let mut gpu = renderer.upload_model(&[SubmeshDesc {
        texture: load_texture(lib, "SHADOW"),
        tint: [255, 255, 255],
        triangles: &[[0, 1, 2], [0, 2, 3]],
        vertex_count: 4,
    }]);
    gpu.shadow = true;
    gpu
}

/// Where a blob shadow lies, as the original's (0x4b05e9, 0x599f20): on
/// the walk floor straight below the middle of the model's bounds, half a
/// unit above it, as wide and deep as the model's bounds; none if no floor
/// lies below.
fn shadow_quad((low, high): (Vec3, Vec3), floor: &[[Vec3; 3]]) -> Option<[ModelVertex; 4]> {
    let centre = (low + high) * 0.5;
    let y = floor_below(floor, centre)? + 0.5;
    let (half_x, half_z) = ((high.x - low.x) / 2.0, (high.z - low.z) / 2.0);
    let (cx, cz) = (centre.x, centre.z);
    let corner = |dx: f32, dz: f32, uv: [f32; 2]| ModelVertex {
        position: [cx + dx, y, cz + dz],
        normal: [0.0, 1.0, 0.0],
        uv,
        tint: [255, 255, 255, 255],
    };
    Some([
        corner(-half_x, -half_z, [0.0, 0.0]),
        corner(half_x, -half_z, [1.0, 0.0]),
        corner(half_x, half_z, [1.0, 1.0]),
        corner(-half_x, half_z, [0.0, 1.0]),
    ])
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
    for (m, (mesh, posed)) in figure
        .model
        .meshes
        .iter()
        .zip(&figure.pose.meshes)
        .enumerate()
    {
        let mut to_world = figure.world * posed.matrix();
        if let Some(head) = figure.head.as_ref().filter(|h| h.mesh == m) {
            to_world *= head.local;
        }
        // G-Engine's finding: characters' normals are stored in model space
        // as the mesh rests, props' in mesh space. A character's are taken
        // back into mesh space by its rest transform, so that both then turn
        // with the mesh as posed and the light on a character follows its
        // animation.
        let posed_rotation = Mat3::from_mat4(posed.matrix());
        let normal_matrix = if figure.actor.is_some() {
            let [i, j, k, _] = mesh.transform;
            let rest = Mat3::from_cols(Vec3::from(i), Vec3::from(j), Vec3::from(k));
            let back = if rest.determinant().abs() > 1e-6 { rest.inverse() } else { Mat3::IDENTITY };
            world_rotation * posed_rotation * back
        } else {
            world_rotation * posed_rotation
        };
        for (group, (sub, positions)) in mesh.submeshes.iter().zip(&posed.positions).enumerate() {
            // A hidden part keeps its vertices, all at one point.
            if figure.hidden_parts.contains(&(m, group)) {
                let at = to_world
                    .transform_point3(Vec3::from(positions.first().copied().unwrap_or_default()));
                out.extend(positions.iter().map(|_| ModelVertex {
                    position: at.into(),
                    normal: [0.0, 1.0, 0.0],
                    uv: [0.0, 0.0],
                    tint: [0, 0, 0, 0],
                }));
                continue;
            }
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

fn position_cameras(sif: &Document) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for line in sif.sections("POSITIONS").flat_map(|s| &s.lines) {
        let (Some(first), Some(camera)) = (line.pairs.first(), line.get("camera")) else {
            continue;
        };
        out.entry(first.key.to_ascii_uppercase())
            .or_insert_with(|| camera.trim().to_ascii_uppercase());
    }
    out
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

/// A model's own lighting in its location file: `fullColor={r,g,b}`, or
/// `fullLighting` for white.
/// How near a character the camera may come before it cuts into them: its
/// near plane is 4 units away, and its corners about 6.
const CAMERA_CUTS_WITHIN: f32 = 8.0;

/// The original renderer's default `ambientRange` for a model (its
/// software renderer's is 0.07).
const DEFAULT_AMBIENT_RANGE: f32 = 0.3;

/// A model's `ambientRange` in its location file; unset, or negative (the
/// original's own "unset"), leaves the default.
fn ambient_range(line: &Line) -> Option<f32> {
    line.get("ambientRange")
        .and_then(|v| v.trim().parse::<f32>().ok())
        .filter(|r| *r >= 0.0)
}

/// The floor's light about a model (0x4b0546): the mean of the last four
/// colours taken, each 0 to 1; half until any is.
#[derive(Debug, Clone, PartialEq)]
struct FloorLight {
    ring: [[f32; 3]; 4],
    next: usize,
    colour: [f32; 3],
    /// Frames still to ease in over; 4 when one is due to be taken.
    pending: u8,
    taken_at: Option<(f32, f32)>,
}

impl Default for FloorLight {
    fn default() -> Self {
        Self {
            ring: [[0.5; 3]; 4],
            next: 0,
            colour: [0.5; 3],
            pending: 0,
            taken_at: None,
        }
    }
}

impl FloorLight {
    fn push(&mut self, c: [f32; 3]) {
        self.ring[self.next] = c;
        self.next = (self.next + 1) % 4;
        self.colour = std::array::from_fn(|i| self.ring.iter().map(|r| r[i]).sum::<f32>() / 4.0);
    }
}

/// A model lit evenly in its location file, and in what colour, as the
/// original reads it (0x510b4c): `fullLighting` is half white, `fullColor`
/// its numbers over 512; the later of the two in the line holds.
fn fixed_lighting(line: &Line) -> Option<[f32; 3]> {
    let mut colour = None;
    for p in &line.pairs {
        if p.key.eq_ignore_ascii_case("fullLighting") {
            colour = Some([0.5; 3]);
        } else if p.key.eq_ignore_ascii_case("fullColor") {
            let n: Vec<f32> = p
                .value
                .split(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-'))
                .filter_map(|x| x.parse().ok())
                .collect();
            if let [r, g, b] = n[..] {
                colour = Some([r / 512.0, g / 512.0, b / 512.0]);
            }
        }
    }
    colour
}

/// A model's bounds in the world as the original reckons them (0x545430,
/// 0x57cfd0, 0x57ce20): each mesh with a part showing has a box about the
/// showing parts' vertices, in the mesh's own space, made a tenth larger
/// about its middle; the model's is about all their corners in the world.
fn model_bounds(figure: &Figure) -> Option<(Vec3, Vec3)> {
    let mut bounds: Option<(Vec3, Vec3)> = None;
    for (m, posed) in figure.pose.meshes.iter().enumerate() {
        let mut mesh: Option<(Vec3, Vec3)> = None;
        for (group, positions) in posed.positions.iter().enumerate() {
            if figure.hidden_parts.contains(&(m, group)) {
                continue;
            }
            let points = figure
                .model
                .meshes
                .get(m)
                .and_then(|mesh| mesh.submeshes.get(group))
                .map_or(0, |s| s.trailing_points);
            for p in &positions[..positions.len().saturating_sub(points)] {
                let p = Vec3::from(*p);
                mesh = Some(mesh.map_or((p, p), |(l, h)| (l.min(p), h.max(p))));
            }
        }
        let Some((low, high)) = mesh else { continue };
        let middle = (low + high) * 0.5;
        let (low, high) = (middle + (low - middle) * 1.1, middle + (high - middle) * 1.1);
        let mut to_world = figure.world * posed.matrix();
        if let Some(head) = figure.head.as_ref().filter(|h| h.mesh == m) {
            to_world *= head.local;
        }
        for i in 0..8 {
            let corner = Vec3::new(
                if i & 1 == 0 { low.x } else { high.x },
                if i & 2 == 0 { low.y } else { high.y },
                if i & 4 == 0 { low.z } else { high.z },
            );
            let c = to_world.transform_point3(corner);
            bounds = Some(bounds.map_or((c, c), |(l, h)| (l.min(c), h.max(c))));
        }
    }
    bounds
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
    fn ambient_range_is_read_and_floor_light_eases_in() {
        let doc = Document::parse(
            b"[MODELS]\nmodel=a, type=prop, ambientrange=0.0\nmodel=b, type=prop, ambientRange=-1\nmodel=c, type=prop\n",
            gk3_formats::ini::Split::Commas,
        );
        let lines: Vec<_> = doc.sections("MODELS").flat_map(|s| &s.lines).collect();
        assert_eq!(ambient_range(lines[0]), Some(0.0));
        assert_eq!(ambient_range(lines[1]), None);
        assert_eq!(ambient_range(lines[2]), None);

        // Half until taken; then a quarter of the way each frame.
        let mut light = FloorLight::default();
        assert_eq!(light.colour, [0.5; 3]);
        light.push([0.9; 3]);
        assert!((light.colour[0] - 0.6).abs() < 1e-6);
        for _ in 0..3 {
            let latest = light.ring[(light.next + 3) % 4];
            light.push(latest);
        }
        assert!((light.colour[0] - 0.9).abs() < 1e-6);
    }

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
