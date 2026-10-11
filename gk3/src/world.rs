//! The running game: a location, its people, the story's state and the
//! scripts acting on it.
//!
//! Scripts run on the Sheep machine against [`GameApi`], which performs the
//! story's state and hands everything presentational back as effects. This
//! module performs them: cutting cameras, starting animations, speaking
//! lines, walking people, and timing all of it, releasing each script that
//! waits when what it waits on is over.

use std::collections::HashMap;
use std::rc::Rc;

use gk3_audio::{Audio, Bus};
use gk3_barn::Library;
use gk3_formats::anm::Animation;
use gk3_formats::bsp::Bsp;
use gk3_formats::image;
use gk3_game::actions::{self, ActionSet, Rule};
use gk3_game::api::{Effect, GameApi};
use gk3_game::character::Character;
use gk3_game::state::GameState;
use gk3_game::walk::WalkMap;
use gk3_render::{Camera, GpuScene, Renderer};
use gk3_sheep::compile::{ENTRY, compile_action};
use gk3_sheep::{Script, ThreadId, ThreadState, Vm};
use glam::{Vec3, Vec4};

use crate::cast::{Cast, Fidget, ray_triangle, read_anm};
use crate::room::{Catalog, Room, RoomCamera};
use crate::shoot;
use crate::soundtrack::Soundtrack;

/// A line on screen.
#[derive(Debug, Clone, PartialEq)]
pub struct Caption {
    pub text: String,
    pub speaker: Option<String>,
    pub from: f64,
    pub until: f64,
}

/// Something a waiting script is waiting for.
enum Until {
    Time(f64),
    Walk(String),
}

struct Task {
    thread: ThreadId,
    until: Until,
}

/// What is under the pointer.
#[derive(Debug, Clone, PartialEq)]
pub enum Pick {
    Noun(String),
    Floor(Vec3),
    Nothing,
}

pub struct Scene {
    pub room: Room,
    pub gpu: GpuScene,
    pub cast: Cast,
    pub actions: ActionSet,
    pub walk: Option<WalkMap>,
    /// BSP object (upper case) to the noun it stands for.
    nouns: HashMap<String, String>,
    floor: Option<String>,
    cameras: HashMap<String, RoomCamera>,
    /// The player character's model name.
    pub ego: String,
    /// Where the player may take the camera.
    pub camera_bounds: crate::camera::Bounds,
    /// How actors behave in each conversation, from `[LISTENERS]`.
    listeners: Vec<Listener>,
}

/// One actor's part in a conversation: fidgets to use while it lasts, and
/// animations to step into and out of it.
#[derive(Debug, Clone, Default)]
struct Listener {
    dialogue: String,
    actor: String,
    talk: Option<String>,
    listen: Option<String>,
    enter: Option<String>,
    exit: Option<String>,
}

/// What a loaded game puts back after entering: the player's spot and
/// heading, and the camera.
#[derive(Default)]
struct Restore {
    ego_at: Option<(Vec3, f32)>,
    camera: Option<Camera>,
}

/// An action waiting for the player to arrive before it runs.
struct Approach {
    rule: Rule,
    actor: String,
}

pub struct World<'a> {
    pub lib: &'a Library,
    pub catalog: Catalog,
    scripts: HashMap<String, Rc<Script>>,
    characters: HashMap<String, Character>,
    faces: HashMap<String, crate::faces::FaceConfig>,
    pub state: GameState,
    vm: Vm,
    pub scene: Option<Scene>,
    pub camera: Camera,
    pub clock: f64,
    tasks: Vec<Task>,
    pub captions: Vec<Caption>,
    /// Threads running actions, and whether the player started them; while
    /// any runs the player cannot act.
    actions: Vec<(ThreadId, Rule, bool)>,
    /// A person whose topics should be offered once the world is free.
    pub topic_bar: Option<String>,
    /// The conversation a script has set, if any.
    pub conversation: Option<String>,
    end_conversation: bool,
    /// Who is speaking, as the lines' SPEAKER marks say, and whether this
    /// dialogue moves people between talking and listening.
    speaker: Option<String>,
    dialogue_fidgets: bool,
    /// Speaker changes due, by time.
    speaker_changes: Vec<(f64, String)>,
    /// Fidgets a conversation replaced: actor, kind, the script to restore.
    saved_fidgets: Vec<(String, Fidget, Option<String>)>,
    approach: Option<Approach>,
    /// From a loaded game: where the player stood and the camera looked,
    /// put back once the room's entry has played.
    restore: Option<Restore>,
    /// Something to turn to face when an approach walk ends.
    face_after: Option<String>,
    next_location: Option<String>,
    /// The plate of the last line spoken, for `ContinueDialogue`.
    last_plate: Option<String>,
    /// What happened, in order, for logs and tests.
    pub log: Vec<String>,
    yaks: HashMap<String, Option<Animation>>,
    pub audio: Audio,
    /// Sounds due to start, by time.
    scheduled: Vec<(f64, String, f32, Bus)>,
    sounds: HashMap<String, Option<std::sync::Arc<gk3_media::Pcm>>>,
    soundtracks: Vec<Soundtrack>,
    soundtrack_sounds: Vec<gk3_audio::SoundId>,
}

impl<'a> World<'a> {
    pub fn new(lib: &'a Library, timeblock: &str, audio: Audio) -> Self {
        let (scripts, _) = crate::sheep_cmd::load_all(lib);
        let mut state = GameState::new(timeblock, "");
        // What each of them has on them when the story starts.
        for item in [
            "DAGGER",
            "GABES_PASSPORT",
            "NOTEPAD",
            "PRINCE_JAMES_CARD",
            "R25_ROOM_KEY",
            "TALISMAN",
            "TAPE_RECORDER",
            "WALLET",
        ] {
            state.give_item("GABRIEL", item);
        }
        for item in [
            "GRACE_WALLET",
            "GRACES_PASSPORT",
            "R25_ROOM_KEY_GRACE",
            "SKETCHPAD",
        ] {
            state.give_item("GRACE", item);
        }
        Self {
            lib,
            catalog: Catalog::new(lib),
            scripts: scripts.into_iter().collect(),
            characters: shoot::characters(lib),
            faces: lib
                .read("FACES.TXT")
                .map(|b| crate::faces::parse(&b))
                .unwrap_or_default(),
            state,
            vm: Vm::new(),
            scene: None,
            camera: Camera {
                position: Vec3::ZERO,
                yaw: 0.0,
                pitch: 0.0,
                fov_y: 60f32.to_radians(),
            },
            clock: 0.0,
            tasks: Vec::new(),
            captions: Vec::new(),
            actions: Vec::new(),
            topic_bar: None,
            conversation: None,
            end_conversation: false,
            speaker: None,
            dialogue_fidgets: true,
            speaker_changes: Vec::new(),
            saved_fidgets: Vec::new(),
            approach: None,
            restore: None,
            face_after: None,
            next_location: None,
            last_plate: None,
            log: Vec::new(),
            yaks: HashMap::new(),
            audio,
            scheduled: Vec::new(),
            sounds: HashMap::new(),
            soundtracks: Vec::new(),
            soundtrack_sounds: Vec::new(),
        }
    }

    /// The game as save-file text. Only when the player is free: a running
    /// script cannot be saved.
    pub fn save(&self) -> Result<String, String> {
        if !self.idle() {
            return Err("cannot save while something is happening".into());
        }
        let num = |v: f32| format!("{v}");
        let mut extra = Vec::new();
        if let Some(scene) = &self.scene
            && let Some((p, heading)) = scene.cast.actor_position(&scene.ego)
        {
            extra.push(vec![
                "ego_at".into(),
                num(p.x),
                num(p.y),
                num(p.z),
                num(heading),
            ]);
        }
        let c = &self.camera;
        extra.push(vec![
            "camera".into(),
            num(c.position.x),
            num(c.position.y),
            num(c.position.z),
            num(c.yaw),
            num(c.pitch),
            num(c.fov_y),
        ]);
        Ok(self.state.to_save(&extra))
    }

    /// Loads a saved game: the story as it was, then the room built from it
    /// and entered, as G-Engine's first save versions did. The entry counts
    /// as a visit, so first-visit scenes do not play again. Where the player
    /// stood and looked is put back once the entry has played.
    pub fn load(&mut self, renderer: &Renderer, text: &str) -> Result<(), String> {
        let (state, extra) = GameState::from_save(text)?;
        let floats = |r: &Vec<String>| -> Vec<f32> {
            r[1..].iter().filter_map(|v| v.parse().ok()).collect()
        };
        let mut ego_at = None;
        let mut camera = None;
        for record in &extra {
            match (record[0].as_str(), floats(record).as_slice()) {
                ("ego_at", &[x, y, z, h]) => ego_at = Some((Vec3::new(x, y, z), h)),
                ("camera", &[x, y, z, yaw, pitch, fov_y]) => {
                    camera = Some(Camera {
                        position: Vec3::new(x, y, z),
                        yaw,
                        pitch,
                        fov_y,
                    })
                }
                _ => {}
            }
        }
        // Drop whatever was going on.
        self.vm = Vm::new();
        self.tasks.clear();
        self.actions.clear();
        self.captions.clear();
        self.scheduled.clear();
        self.audio.stop_all();
        self.topic_bar = None;
        self.end_conversation = false;
        self.approach = None;
        self.face_after = None;
        self.next_location = None;
        let last_location = state.last_location.clone();
        self.state = state;
        let location = self.state.location.clone();
        self.enter(renderer, &location)?;
        self.state.last_location = last_location;
        self.restore = Some(Restore { ego_at, camera });
        Ok(())
    }

    /// Whether the player is free to act.
    pub fn idle(&self) -> bool {
        self.actions.is_empty() && self.approach.is_none() && self.next_location.is_none()
    }

    /// Goes to a location: builds it as the story has it now, counts the
    /// visit, and runs its `SCENE ENTER` action.
    pub fn enter(&mut self, renderer: &Renderer, location: &str) -> Result<(), String> {
        let location = location.to_ascii_uppercase();
        self.catalog.time = Some(self.state.timeblock.clone());
        let scene_name = self
            .catalog
            .resolve(self.lib, &location)
            .ok_or_else(|| format!("no scene for {location} at {}", self.state.timeblock))?;
        let room = self.catalog.load(self.lib, &scene_name)?;
        let sif = room.sif.clone().unwrap_or_default();
        let gpu = renderer.upload(&room.mesh);
        let mut cast = Cast::load(self.lib, renderer, &sif, &self.characters, &self.faces);
        cast.update(self.lib, self.clock);
        cast.upload(renderer);

        // Which objects are things, and what the player calls them.
        let mut nouns = HashMap::new();
        for line in sif.sections("MODELS").flat_map(|s| &s.lines) {
            let (Some(model), Some(noun)) = (line.get("model"), line.get("noun")) else {
                continue;
            };
            let kind = line.get("type").unwrap_or("scene").to_ascii_lowercase();
            if matches!(kind.as_str(), "scene" | "hittest") {
                nouns
                    .entry(model.to_ascii_uppercase())
                    .or_insert_with(|| noun.to_ascii_uppercase());
            }
        }
        let general = |key: &str| {
            sif.sections("GENERAL")
                .flat_map(|s| &s.lines)
                .find_map(|l| l.get(key).map(str::to_string))
        };
        let floor = general("floor").map(|f| f.to_ascii_uppercase());
        // Every applicable section may add a bounds model.
        let bounds_models: Vec<gk3_formats::model::Model> = sif
            .sections("GENERAL")
            .flat_map(|s| &s.lines)
            .filter_map(|l| l.get("cameraBounds"))
            .filter_map(|name| {
                gk3_formats::model::Model::parse(&self.lib.read(&format!("{name}.MOD")).ok()?).ok()
            })
            .collect();
        let camera_bounds = crate::camera::Bounds::from_models(&bounds_models);
        let listeners = sif
            .sections("LISTENERS")
            .flat_map(|s| &s.lines)
            .filter_map(|l| {
                let get = |k: &str| l.get(k).map(|v| v.trim().to_string());
                Some(Listener {
                    dialogue: get("dialogue")?.to_ascii_uppercase(),
                    actor: get("actor")?.to_ascii_uppercase(),
                    talk: get("talk"),
                    listen: get("listen"),
                    enter: get("enter"),
                    exit: get("exit"),
                })
            })
            .collect();
        self.speaker = None;
        self.speaker_changes.clear();
        self.saved_fidgets.clear();
        self.conversation = None;
        let walk = sif
            .sections("GENERAL")
            .flat_map(|s| &s.lines)
            .find(|l| l.get("boundary").is_some())
            .and_then(|l| {
                let image = image::decode_indexed(
                    &self.lib.read(&format!("{}.BMP", l.get("boundary")?)).ok()?,
                )
                .ok()?;
                let size = crate::room::floats::<2>(l.get("size")?)?;
                let offset = crate::room::floats::<2>(l.get("offset")?)?;
                Some(WalkMap::new(image, size, offset))
            });
        let ego_line = sif.sections("ACTORS").flat_map(|s| &s.lines).find(|l| {
            l.pairs
                .iter()
                .any(|p| p.is_flag() && p.key.eq_ignore_ascii_case("ego"))
        });
        let ego = ego_line
            .and_then(|l| l.get("model"))
            .unwrap_or("GAB")
            .to_ascii_uppercase();
        // Whoever the room has as the player is the player, as in G-Engine.
        if let Some(noun) = ego_line.and_then(|l| l.get("noun")) {
            self.state.ego = gk3_game::state::character(noun);
        }

        // The action files in scope: the location's, most specific first,
        // then the game's global and inventory sets.
        let time = self.state.timeblock.clone();
        let mut files: Vec<String> = sif
            .sections("ACTIONS")
            .flat_map(|s| &s.lines)
            .map(|l| l.text.trim().to_ascii_uppercase())
            .filter(|n| n.ends_with(".NVC") && actions::covers(n, &time))
            .collect();
        files.sort_by_key(|n| specificity(n));
        files.dedup();
        let mut global: Vec<String> = self
            .lib
            .assets()
            .iter()
            .map(|(_, e)| e.name.to_ascii_uppercase())
            .filter(|n| {
                (n.starts_with("GLB") || n.starts_with("INV"))
                    && n.ends_with(".NVC")
                    && actions::covers(n, &time)
            })
            .collect();
        global.sort_by_key(|n| (n.starts_with("INV"), specificity(n)));
        let mut set = ActionSet::default();
        for file in files.iter().chain(&global) {
            if let Ok(bytes) = self.lib.read(file) {
                set.add_file(&bytes, file);
            }
        }

        let cameras = room
            .cameras
            .iter()
            .map(|c| (c.name.to_ascii_uppercase(), c.clone()))
            .collect();
        self.camera = shoot::start_camera(&room, None)?;
        self.log.push(format!(
            "enter {location} as {scene_name} at {time}: {} action files, {} rules, cast {}",
            files.len() + global.len(),
            set.rules.len(),
            cast.describe()
        ));
        self.scene = Some(Scene {
            room,
            gpu,
            cast,
            actions: set,
            walk,
            nouns,
            floor,
            cameras,
            ego,
            camera_bounds,
            listeners,
        });
        self.tasks.clear();
        self.captions.clear();
        for id in self.soundtrack_sounds.drain(..) {
            self.audio.stop(id);
        }
        self.soundtracks.clear();
        let ambient: Vec<String> = {
            let sif = self.scene.as_ref().and_then(|s| s.room.sif.as_ref());
            let mut names: Vec<String> = sif
                .map(|sif| {
                    sif.sections("AMBIENT")
                        .flat_map(|s| &s.lines)
                        .map(|l| l.text.trim().to_string())
                        .collect()
                })
                .unwrap_or_default();
            names.sort_by_key(|n| n.to_ascii_uppercase());
            names.dedup_by_key(|n| n.to_ascii_uppercase());
            names
        };
        for name in ambient {
            let stem = name.trim_end_matches(".STK").trim_end_matches(".stk");
            if let Ok(bytes) = self.lib.read(&format!("{stem}.STK")) {
                self.soundtracks.push(Soundtrack::parse(stem, &bytes));
            }
        }
        self.state.enter(&location);

        let enter = {
            let scene = self.scene.as_ref().unwrap();
            let scripts = &self.scripts;
            let snapshot = self.state.clone();
            let mut probe = snapshot.clone();
            let mut api = GameApi::new(&mut probe, scripts);
            scene
                .actions
                .find("SCENE", "ENTER", &mut api, &snapshot)
                .cloned()
        };
        if let Some(rule) = enter {
            self.run(renderer, rule, false);
        }
        Ok(())
    }

    /// What can be done to a noun now.
    pub fn verbs(&self, noun: &str) -> Vec<Rule> {
        let Some(scene) = &self.scene else {
            return Vec::new();
        };
        let snapshot = self.state.clone();
        let mut probe = snapshot.clone();
        let mut api = GameApi::new(&mut probe, &self.scripts);
        let mut out: Vec<Rule> = scene
            .actions
            .available(noun, &mut api, &snapshot)
            .into_iter()
            // Topics and chat belong to the topic bar, after TALK; using an
            // item is offered below, for the item in hand.
            .filter(|r| {
                !actions::is_topic(&r.verb)
                    && !matches!(r.verb.as_str(), "Z_CHAT" | "CANCEL")
                    && is_plain_verb(&r.verb)
            })
            .cloned()
            .collect();
        let ego = &self.state.ego;
        if let Some(item) = self.state.active_item.get(ego)
            && self.state.has_item(ego, item)
            && !item.eq_ignore_ascii_case(noun)
            && let Some(rule) = scene.actions.using(noun, item, &mut api, &snapshot)
        {
            // A wildcard rule still reads, and counts, as this noun and item.
            out.push(Rule {
                noun: gk3_game::state::key(noun),
                verb: gk3_game::state::key(item),
                ..rule.clone()
            });
        }
        out
    }

    /// What can be raised with somebody: their topics, then chat and a way
    /// out where the files give one.
    pub fn topics(&self, noun: &str) -> Vec<Rule> {
        let Some(scene) = &self.scene else {
            return Vec::new();
        };
        let snapshot = self.state.clone();
        let mut probe = snapshot.clone();
        let mut api = GameApi::new(&mut probe, &self.scripts);
        let available = scene.actions.available(noun, &mut api, &snapshot);
        let mut out: Vec<Rule> = available
            .iter()
            .filter(|r| actions::is_topic(&r.verb))
            .map(|r| (*r).clone())
            .collect();
        out.extend(
            available
                .iter()
                .filter(|r| r.verb == "Z_CHAT")
                .map(|r| (*r).clone()),
        );
        if !out.is_empty() {
            out.extend(
                available
                    .iter()
                    .filter(|r| r.verb == "CANCEL")
                    .map(|r| (*r).clone()),
            );
        }
        out
    }

    /// Ends the conversation the way the game does: `GLB_ALL:CodeCallEndConv$`.
    pub fn end_conversation(&mut self, renderer: &Renderer) {
        self.topic_bar = None;
        let Some(script) = self.scripts.get("GLB_ALL").cloned() else {
            return;
        };
        let (result, effects) = {
            let mut api = GameApi::new(&mut self.state, &self.scripts);
            let result = self
                .vm
                .start(&mut api, script, "GLB_ALL", "CodeCallEndConv$");
            (result, std::mem::take(&mut api.effects))
        };
        if let Ok(id) = result {
            let rule = Rule {
                noun: "CONVERSATION".into(),
                verb: "END".into(),
                case: "ALL".into(),
                approach: None,
                target: None,
                script: String::new(),
                source: "GLB_ALL".into(),
            };
            self.actions.push((id, rule, false));
        }
        self.perform(renderer, effects);
        self.conversation = None;
        self.finish_actions();
    }

    /// Every noun in scope with the verbs it offers now, for tools.
    pub fn offered(&self) -> Vec<(String, Vec<String>)> {
        let Some(scene) = &self.scene else {
            return Vec::new();
        };
        let mut nouns: Vec<String> = scene.actions.rules.iter().map(|r| r.noun.clone()).collect();
        nouns.sort();
        nouns.dedup();
        nouns
            .into_iter()
            .map(|n| {
                let mut verbs: Vec<String> = self.verbs(&n).into_iter().map(|r| r.verb).collect();
                verbs.extend(self.topics(&n).into_iter().map(|r| r.verb));
                (n, verbs)
            })
            .filter(|(_, v): &(String, Vec<String>)| !v.is_empty())
            .collect()
    }

    /// Performs a rule: walks the player into place if it says to, then runs
    /// its script.
    pub fn act(&mut self, renderer: &Renderer, rule: Rule) {
        self.log.push(format!(
            "do {}:{} [{}] from {}",
            rule.noun, rule.verb, rule.case, rule.source
        ));
        let Some(scene) = &mut self.scene else { return };
        let ego = scene.ego.clone();
        let approach = rule
            .approach
            .as_deref()
            .unwrap_or("none")
            .to_ascii_lowercase();
        let target = rule.target.clone().unwrap_or_default();
        let destination = match approach.as_str() {
            "walkto" => scene
                .cast
                .positions
                .get(&target.to_ascii_uppercase())
                .map(|&(p, h)| (p, Some(h.to_radians()))),
            "walktosee" | "nearmodel" => self.standing_spot(&target).map(|p| (p, None)),
            "turntomodel" => {
                self.face(&target);
                None
            }
            _ => None,
        };
        if let Some((goal, heading)) = destination
            && self.walk_ego(goal, heading)
        {
            if heading.is_none() && approach != "walkto" {
                // Face what was approached on arrival.
                self.face_after = Some(target);
            }
            self.approach = Some(Approach { rule, actor: ego });
            self.topic_bar = None;
            return;
        }
        self.run(renderer, rule, true);
    }

    /// Where to stand to look at something: in front of it, a character's
    /// height away, on walkable floor.
    fn standing_spot(&self, target: &str) -> Option<Vec3> {
        let scene = self.scene.as_ref()?;
        let center = scene
            .cast
            .center(target)
            .or_else(|| bsp_center(&scene.room.bsp, target))?;
        let (from, _) = scene.cast.actor_position(&scene.ego)?;
        let height = self
            .characters
            .get(&scene.ego)
            .map_or(70.0, |c| c.walker_height);
        let flat = Vec3::new(from.x - center.x, 0.0, from.z - center.z);
        let away = flat.length().min(height);
        let spot = center + flat.normalize_or_zero() * away;
        let walk = scene.walk.as_ref()?;
        let [x, z] = walk.nearest_walkable(spot.x, spot.z)?;
        Some(Vec3::new(x, from.y, z))
    }

    fn face(&mut self, target: &str) {
        let Some(scene) = &mut self.scene else { return };
        let center = scene
            .cast
            .center(target)
            .or_else(|| bsp_center(&scene.room.bsp, target));
        if let (Some(c), Some((p, _))) = (center, scene.cast.actor_position(&scene.ego)) {
            let heading = (c.x - p.x).atan2(c.z - p.z);
            let ego = scene.ego.clone();
            scene.cast.turn(&ego, heading);
        }
    }

    /// Walks the player towards a point. False if there is nowhere to go.
    pub fn walk_ego(&mut self, goal: Vec3, heading: Option<f32>) -> bool {
        let Some(scene) = &mut self.scene else {
            return false;
        };
        let ego = scene.ego.clone();
        let Some((from, _)) = scene.cast.actor_position(&ego) else {
            return false;
        };
        let path = match &scene.walk {
            Some(walk) => walk.path([from.x, from.z], [goal.x, goal.z]),
            None => Some(vec![[goal.x, goal.z]]),
        };
        match path {
            Some(path) if !path.is_empty() => {
                scene.cast.walk(self.lib, &ego, path, heading, self.clock)
            }
            _ => false,
        }
    }

    /// Runs a rule's script now.
    fn run(&mut self, renderer: &Renderer, rule: Rule, user: bool) {
        let script = match compile_action(&rule.script) {
            Ok(s) => Rc::new(s),
            Err(e) => {
                self.log
                    .push(format!("  cannot run {}:{}: {e}", rule.noun, rule.verb));
                return;
            }
        };
        let (result, effects) = {
            let mut api = GameApi::new(&mut self.state, &self.scripts);
            let result = self.vm.start(&mut api, script, "ACTION", ENTRY);
            (result, std::mem::take(&mut api.effects))
        };
        match result {
            Ok(id) => self.actions.push((id, rule, user)),
            Err(e) => self.log.push(format!("  {e}")),
        }
        self.perform(renderer, effects);
        self.finish_actions();
    }

    /// Releases a waiting script, and performs whatever it does next.
    fn release(&mut self, renderer: &Renderer, thread: ThreadId) {
        let effects = {
            let mut api = GameApi::new(&mut self.state, &self.scripts);
            self.vm.release(&mut api, thread);
            std::mem::take(&mut api.effects)
        };
        self.perform(renderer, effects);
    }

    fn perform(&mut self, renderer: &Renderer, effects: Vec<Effect>) {
        let mut queue: std::collections::VecDeque<Effect> = effects.into();
        while let Some(effect) = queue.pop_front() {
            let done_now = self.perform_one(renderer, &effect);
            if let Some(thread) = effect.thread {
                match done_now {
                    Some(until) => self.tasks.push(Task { thread, until }),
                    None => {
                        let more = {
                            let mut api = GameApi::new(&mut self.state, &self.scripts);
                            self.vm.release(&mut api, thread);
                            std::mem::take(&mut api.effects)
                        };
                        queue.extend(more);
                    }
                }
            }
        }
    }

    /// Performs one effect. Returns what a waiting script must wait for, or
    /// `None` if it is over already.
    fn perform_one(&mut self, renderer: &Renderer, e: &Effect) -> Option<Until> {
        let now = self.clock;
        let lib = self.lib;
        self.log.push(format!(
            "  {}({})",
            e.name,
            e.args
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ));
        let scene = self.scene.as_mut()?;
        let name = e.name.to_ascii_uppercase();
        match name.as_str() {
            "CAMERABOUNDARYBLOCKMODEL" => {
                let model = e.string(0);
                if let Some(triangles) = scene.cast.world_triangles(&model) {
                    scene.camera_bounds.block(&model, triangles);
                }
                None
            }
            "CAMERABOUNDARYUNBLOCKMODEL" => {
                scene.camera_bounds.unblock(&e.string(0));
                None
            }
            "ENABLECAMERABOUNDARIES" | "DISABLECAMERABOUNDARIES" => {
                scene.camera_bounds.enabled = name == "ENABLECAMERABOUNDARIES";
                None
            }
            "SHOWINVENTORY" | "HIDEINVENTORY" => {
                self.state.inventory_shown = name == "SHOWINVENTORY";
                None
            }
            "CUTTOCAMERAANGLE"
            | "FORCECUTTOCAMERAANGLE"
            | "GLIDETOCAMERAANGLE"
            | "GLIDETOCAMERAANGLEX" => {
                if let Some(c) = scene.cameras.get(&e.string(0).to_ascii_uppercase()) {
                    self.camera = shoot::from_room_camera(c);
                }
                None
            }
            "STARTANIMATION" | "STARTMOVEANIMATION" | "LOOPANIMATION" => {
                let looping = name == "LOOPANIMATION";
                let seconds = scene.cast.play(
                    lib,
                    &e.string(0),
                    looping,
                    name == "STARTMOVEANIMATION",
                    now,
                )?;
                if let Some(anm) = read_anm(lib, &e.string(0)) {
                    for f in &anm.faces {
                        scene
                            .cast
                            .face(&f.noun, now + f.frame as f64 / 15.0, f.change.clone());
                    }
                    for sound in &anm.sounds {
                        self.scheduled.push((
                            now + sound.frame as f64 / 15.0,
                            sound.name.clone(),
                            sound.volume as f32 / 100.0,
                            Bus::Effects,
                        ));
                    }
                }
                (!looping).then_some(Until::Time(now + seconds as f64))
            }
            "STOPANIMATION" => {
                scene.cast.stop(&e.string(0));
                None
            }
            "STARTVOICEOVER"
            | "STARTDIALOGUE"
            | "STARTDIALOGUENONOFIDGETS"
            | "STARTDIALOGUENOFIDGETS" => {
                self.dialogue_fidgets = name == "STARTDIALOGUE";
                let seconds = self.speak(&e.string(0), e.int(1).max(1));
                Some(Until::Time(now + seconds))
            }
            "CONTINUEDIALOGUE" | "CONTINUEDIALOGUENOFIDGETS" => {
                self.dialogue_fidgets = name == "CONTINUEDIALOGUE";
                let plate = self.last_plate.as_deref().map(next_plate)?;
                let seconds = self.speak(&plate, e.int(0).max(1));
                Some(Until::Time(now + seconds))
            }
            "STARTYAK" => {
                let seconds = self.yak(&e.string(0)).map_or(0.0, |(s, _)| s);
                Some(Until::Time(now + seconds as f64))
            }
            "SETTIMERSECONDS" => Some(Until::Time(now + e.float(0) as f64)),
            "SETTIMERMS" => Some(Until::Time(now + e.int(0) as f64 / 1000.0)),
            "WALKTO" => {
                let actor = e.string(0);
                let &(goal, heading) = scene
                    .cast
                    .positions
                    .get(&e.string(1).to_ascii_uppercase())?;
                let from = scene.cast.actor_position(&actor)?.0;
                let path = match &scene.walk {
                    Some(w) => w.path([from.x, from.z], [goal.x, goal.z])?,
                    None => vec![[goal.x, goal.z]],
                };
                scene
                    .cast
                    .walk(lib, &actor, path, Some(heading.to_radians()), now)
                    .then_some(Until::Walk(actor))
            }
            "SETACTORPOSITION" => {
                scene.cast.place(&e.string(0), &e.string(1));
                None
            }
            "INITEGOPOSITION" => {
                let ego = scene.ego.clone();
                scene.cast.place(&ego, &e.string(0));
                None
            }
            "SHOWMODEL" | "HIDEMODEL" | "SHOWSCENEMODEL" | "HIDESCENEMODEL" => {
                let show = name.starts_with("SHOW");
                let model = e.string(0);
                if !scene.cast.set_visible(&model, show) {
                    let object = model.to_ascii_uppercase();
                    let changed = if show {
                        scene.room.hidden.remove(&object)
                    } else {
                        scene.room.hidden.insert(object)
                    };
                    if changed {
                        scene.room.rebuild();
                        scene.gpu = renderer.upload(&scene.room.mesh);
                    }
                }
                None
            }
            "WALKERBOUNDARYBLOCKREGION" | "WALKERBOUNDARYUNBLOCKREGION" => {
                if let Some(walk) = &mut scene.walk {
                    let block = !name.contains("UNBLOCK");
                    walk.set_blocked(e.int(0) as u8, block);
                    walk.set_blocked(e.int(1) as u8, block);
                }
                None
            }
            "PLAYSOUND" => {
                self.scheduled.push((now, e.string(0), 1.0, Bus::Effects));
                None
            }
            "SETLOCATION" => {
                self.next_location = Some(e.string(0));
                None
            }
            "SETCONVERSATION" => {
                let seconds = self.begin_conversation(&e.string(0));
                Some(Until::Time(now + seconds))
            }
            "ENDCONVERSATION" => {
                let seconds = self.finish_conversation();
                Some(Until::Time(now + seconds))
            }
            "STARTIDLEFIDGET" | "STARTTALKFIDGET" | "STARTLISTENFIDGET" => {
                let kind = match name.as_str() {
                    "STARTTALKFIDGET" => Fidget::Talk,
                    "STARTLISTENFIDGET" => Fidget::Listen,
                    _ => Fidget::Idle,
                };
                scene.cast.start_fidget(lib, &e.string(0), kind);
                None
            }
            "SETIDLEGAS" | "SETTALKGAS" | "SETLISTENGAS" => {
                let kind = match name.as_str() {
                    "SETTALKGAS" => Fidget::Talk,
                    "SETLISTENGAS" => Fidget::Listen,
                    _ => Fidget::Idle,
                };
                let gas = e.string(1);
                let gas = (!gas.trim().is_empty()).then_some(gas);
                scene.cast.set_fidget_gas(lib, &e.string(0), kind, gas);
                None
            }
            "STOPFIDGET" => {
                scene.cast.stop_fidget(&e.string(0));
                None
            }
            // Everything else is over as soon as it starts, for now.
            _ => None,
        }
    }

    /// `SetConversation`: cuts to the conversation's opening camera (or its
    /// closing one if it has none), gives its actors their talk and listen
    /// fidgets, starts them listening and plays their way in. Returns how
    /// long the way in takes.
    fn begin_conversation(&mut self, name: &str) -> f64 {
        let name = name.to_ascii_uppercase();
        self.conversation = Some(name.clone());
        let (lib, now) = (self.lib, self.clock);
        let Some(scene) = &mut self.scene else {
            return 0.0;
        };
        let cameras = || {
            scene
                .room
                .cameras
                .iter()
                .filter(|c| c.dialogue.as_deref() == Some(name.as_str()))
        };
        if let Some(c) = cameras()
            .find(|c| c.initial)
            .or_else(|| cameras().find(|c| c.last))
        {
            self.camera = shoot::from_room_camera(c);
            self.log.push(format!("    (camera {} for {name})", c.name));
        }
        let mut longest = 0.0f64;
        for l in scene.listeners.iter().filter(|l| l.dialogue == name) {
            if l.talk.is_none() && l.listen.is_none() {
                continue;
            }
            for (kind, gas) in [(Fidget::Talk, &l.talk), (Fidget::Listen, &l.listen)] {
                if gas.is_some()
                    && let Some(old) = scene.cast.set_fidget_gas(lib, &l.actor, kind, gas.clone())
                {
                    self.saved_fidgets.push((l.actor.clone(), kind, old));
                }
            }
            let kind = if l.listen.is_some() {
                Fidget::Listen
            } else {
                Fidget::Talk
            };
            scene.cast.start_fidget(lib, &l.actor, kind);
            self.log
                .push(format!("    ({} {kind:?}s, for {name})", l.actor));
            if let Some(anim) = &l.enter
                && let Some(seconds) = scene.cast.play(lib, anim, false, false, now)
            {
                longest = longest.max(seconds as f64);
            }
        }
        longest
    }

    /// `EndConversation`: the closing camera, the actors' way out, and their
    /// own fidgets back.
    fn finish_conversation(&mut self) -> f64 {
        let Some(name) = self.conversation.take() else {
            return 0.0;
        };
        let (lib, now) = (self.lib, self.clock);
        self.speaker = None;
        let Some(scene) = &mut self.scene else {
            return 0.0;
        };
        if let Some(c) = scene
            .room
            .cameras
            .iter()
            .find(|c| c.dialogue.as_deref() == Some(name.as_str()) && c.last)
        {
            self.camera = shoot::from_room_camera(c);
        }
        let mut longest = 0.0f64;
        for l in scene.listeners.iter().filter(|l| l.dialogue == name) {
            if let Some(anim) = &l.exit
                && let Some(seconds) = scene.cast.play(lib, anim, false, false, now)
            {
                longest = longest.max(seconds as f64);
            }
        }
        for (actor, kind, gas) in self.saved_fidgets.drain(..) {
            scene.cast.set_fidget_gas(lib, &actor, kind, gas);
            scene.cast.start_fidget(lib, &actor, Fidget::Idle);
        }
        longest
    }

    /// G-Engine's `SetSpeaker`: whoever stops speaking goes from talking to
    /// listening, whoever starts goes from listening to talking. In a
    /// conversation, only those it gave such fidgets to.
    fn set_speaker(&mut self, who: &str) {
        if self.speaker.as_deref() == Some(who) {
            return;
        }
        let lib = self.lib;
        let previous = self.speaker.replace(who.to_string());
        if !self.dialogue_fidgets {
            return;
        }
        let Some(scene) = &mut self.scene else { return };
        let in_conversation = self.conversation.is_some();
        let saved = &self.saved_fidgets;
        let given = |cast: &Cast, actor: &str, kind: Fidget| {
            !in_conversation
                || saved.iter().any(|(a, k, _)| {
                    *k == kind && cast.find(a).is_some() && cast.find(a) == cast.find(actor)
                })
        };
        if let Some(prev) = previous.filter(|p| p != "UNKNOWN")
            && given(&scene.cast, &prev, Fidget::Listen)
            && scene.cast.fidget(&prev) == Some(Fidget::Talk)
        {
            scene.cast.start_fidget(lib, &prev, Fidget::Listen);
            self.log.push(format!("    ({prev} listens)"));
        }
        if who != "UNKNOWN"
            && given(&scene.cast, who, Fidget::Talk)
            && scene.cast.fidget(who) == Some(Fidget::Listen)
        {
            scene.cast.start_fidget(lib, who, Fidget::Talk);
            self.log.push(format!("    ({who} talks)"));
        }
    }

    /// Speaks `count` lines starting at `plate`: shows their captions in
    /// turn and returns how long they take.
    fn speak(&mut self, plate: &str, count: i32) -> f64 {
        let mut plate = plate.to_ascii_uppercase();
        let mut at = self.clock;
        for i in 0..count {
            if i > 0 {
                plate = next_plate(&plate);
            }
            // Spoken lines are localised: English adds an E.
            let line = self
                .yak_at(&format!("E{plate}"), at)
                .or_else(|| self.yak_at(&plate, at));
            if let Some((seconds, captions)) = line {
                for c in &captions {
                    if let Some(who) = &c.speaker {
                        self.speaker_changes
                            .push((at + c.frame as f64 / 15.0, who.clone()));
                    }
                }
                for c in captions {
                    self.captions.push(Caption {
                        text: c.text.clone(),
                        speaker: c.speaker.clone(),
                        from: at + c.frame as f64 / 15.0,
                        until: at + seconds as f64,
                    });
                    self.log.push(format!(
                        "    {}: {}",
                        c.speaker.as_deref().unwrap_or("?"),
                        c.text
                    ));
                }
                at += seconds as f64;
            } else {
                self.log.push(format!("    (no line {plate})"));
            }
            self.last_plate = Some(plate.clone());
        }
        at - self.clock
    }

    /// A line's length and captions, with its sounds scheduled from `at`.
    fn yak_at(&mut self, name: &str, at: f64) -> Option<(f32, Vec<gk3_formats::anm::Caption>)> {
        let lib = self.lib;
        let anm = self
            .yaks
            .entry(name.to_ascii_uppercase())
            .or_insert_with(|| {
                lib.read(&format!("{name}.YAK"))
                    .ok()
                    .map(|b| Animation::parse(&b))
            })
            .as_ref()?;
        let result = (anm.frames as f32 / 15.0, anm.captions.clone());
        let faces: Vec<_> = anm
            .faces
            .iter()
            .map(|f| (at + f.frame as f64 / 15.0, f.clone()))
            .collect();
        let sounds: Vec<_> = anm
            .sounds
            .iter()
            .map(|s| {
                (
                    at + s.frame as f64 / 15.0,
                    s.name.clone(),
                    s.volume as f32 / 100.0,
                    Bus::Dialogue,
                )
            })
            .collect();
        self.scheduled.extend(sounds);
        if let Some(scene) = &mut self.scene {
            for (t, f) in faces {
                scene.cast.face(&f.noun, t, f.change);
            }
        }
        Some(result)
    }

    fn yak(&mut self, name: &str) -> Option<(f32, Vec<gk3_formats::anm::Caption>)> {
        let now = self.clock;
        self.yak_at(name, now)
    }

    /// A sound by name, decoded the first time it is asked for. A name
    /// with no extension is tried again with `.WAV`.
    fn pcm(&mut self, name: &str) -> Option<std::sync::Arc<gk3_media::Pcm>> {
        let lib = self.lib;
        self.sounds
            .entry(name.to_ascii_uppercase())
            .or_insert_with(|| {
                let bytes = lib
                    .read(name)
                    .or_else(|_| lib.read(&format!("{name}.WAV")))
                    .ok()?;
                gk3_media::decode_wave(&bytes).ok().map(std::sync::Arc::new)
            })
            .clone()
    }

    /// Starts a sound by name.
    fn sound(&mut self, name: &str, gain: f32, bus: Bus) -> Option<gk3_audio::SoundId> {
        match self.pcm(name) {
            Some(pcm) => Some(self.audio.play(pcm, gain, false, bus)),
            None => {
                self.log.push(format!("    (no sound {name})"));
                None
            }
        }
    }

    /// Hurries through what is being said: the current lines end now.
    pub fn skip(&mut self) {
        if self.captions.is_empty() {
            return;
        }
        let now = self.clock;
        for task in &mut self.tasks {
            if let Until::Time(t) = &mut task.until {
                *t = t.min(now);
            }
        }
        self.captions.clear();
        self.scheduled.retain(|s| s.3 != Bus::Dialogue);
        self.audio.stop_bus(Bus::Dialogue);
    }

    /// Moves the world on to `now`.
    pub fn update(&mut self, renderer: &Renderer, now: f64) {
        self.clock = now;
        if let Some(scene) = &mut self.scene {
            scene.cast.update(self.lib, now);
            scene.cast.upload(renderer);
        }
        self.captions.retain(|c| c.until > now);
        let mut changes = std::mem::take(&mut self.speaker_changes);
        changes.sort_by(|a, b| a.0.total_cmp(&b.0));
        let later = changes.split_off(changes.partition_point(|c| c.0 <= now));
        self.speaker_changes = later;
        for (_, who) in changes {
            self.set_speaker(&who);
        }
        // A loaded game's room has played its entry: put the player back.
        if self.restore.is_some() && self.idle() {
            let Restore { ego_at, camera } = self.restore.take().unwrap_or_default();
            if let Some((position, heading)) = ego_at
                && let Some(scene) = &mut self.scene
            {
                let ego = scene.ego.clone();
                scene.cast.put(&ego, position, heading);
            }
            if let Some(camera) = camera {
                self.camera = camera;
            }
        }
        let due: Vec<_> = self
            .scheduled
            .iter()
            .filter(|s| s.0 <= now)
            .cloned()
            .collect();
        self.scheduled.retain(|s| s.0 > now);
        for (_, name, gain, bus) in due {
            self.sound(&name, gain, bus);
        }
        let mut soundtracks = std::mem::take(&mut self.soundtracks);
        for track in &mut soundtracks {
            let bus = if track.music {
                Bus::Music
            } else {
                Bus::Effects
            };
            let mut length =
                |clip: &crate::soundtrack::Clip| self.pcm(&clip.name).map(|p| p.seconds());
            let clips = track.update(now, &mut length);
            for clip in clips {
                if let Some(pcm) = self.pcm(&clip.name) {
                    let id = self.audio.play(pcm, clip.volume, clip.looping, bus);
                    self.soundtrack_sounds.push(id);
                }
            }
        }
        self.soundtracks = soundtracks;
        let audio = &self.audio;
        self.soundtrack_sounds
            .retain(|id| audio.is_playing(*id) || audio.is_silent());

        // The player arrived where an action wanted them.
        if let Some(approach) = &self.approach
            && !self
                .scene
                .as_ref()
                .is_some_and(|s| s.cast.is_walking(&approach.actor))
        {
            let approach = self.approach.take().unwrap();
            if let Some(target) = self.face_after.take() {
                self.face(&target);
            }
            self.run(renderer, approach.rule, true);
        }

        // Release scripts whose waits are over.
        let mut ready = Vec::new();
        self.tasks.retain(|task| {
            let done = match &task.until {
                Until::Time(t) => *t <= now,
                Until::Walk(actor) => !self
                    .scene
                    .as_ref()
                    .is_some_and(|s| s.cast.is_walking(actor)),
            };
            if done {
                ready.push(task.thread);
            }
            !done
        });
        for thread in ready {
            self.release(renderer, thread);
        }
        self.finish_actions();

        if self.end_conversation && self.actions.is_empty() {
            self.end_conversation = false;
            self.end_conversation(renderer);
        }

        // A change of location waits for the action that asked for it.
        if self.actions.is_empty()
            && let Some(location) = self.next_location.take()
            && let Err(e) = self.enter(renderer, &location)
        {
            self.log.push(format!("  cannot enter {location}: {e}"));
        }
    }

    /// Notes actions whose scripts have ended, and the counts that follow.
    fn finish_actions(&mut self) {
        let vm = &self.vm;
        let mut finished = Vec::new();
        self.actions.retain(|(id, rule, user)| {
            let done = !matches!(
                vm.state(*id),
                Some(ThreadState::Running | ThreadState::Waiting)
            );
            if done {
                finished.push((rule.clone(), *user));
            }
            !done
        });
        for (rule, user) in finished {
            // Topics and chats count themselves; ordinary verbs count only
            // when their script says so.
            if actions::is_topic(&rule.verb) {
                *self
                    .state
                    .topics
                    .entry((rule.noun.clone(), rule.verb.clone()))
                    .or_default() += 1;
                self.state.played_topics.insert((
                    rule.noun.clone(),
                    rule.verb.clone(),
                    rule.case.clone(),
                ));
            } else if rule.verb == "Z_CHAT" {
                *self.state.chats.entry(rule.noun.clone()).or_default() += 1;
            }
            self.log.push(format!("done {}:{}", rule.noun, rule.verb));
            // After talking, the topics come up; after a chat or a cancel
            // the conversation is over.
            if user {
                if rule.verb == "TALK" || actions::is_topic(&rule.verb) {
                    self.topic_bar = Some(rule.noun.clone());
                } else if rule.verb == "Z_CHAT" || rule.verb == "CANCEL" {
                    self.end_conversation = true;
                } else if self.conversation.is_some() {
                    self.topic_bar = Some(rule.noun.clone());
                }
            }
        }
        self.vm.collect();
    }

    /// What lies under a point on screen.
    pub fn pick(&self, x: f32, y: f32, width: u32, height: u32) -> Pick {
        let Some(scene) = &self.scene else {
            return Pick::Nothing;
        };
        let aspect = width as f32 / height.max(1) as f32;
        let inverse = self.camera.view_proj(aspect).inverse();
        let ndc = (x / width as f32 * 2.0 - 1.0, 1.0 - y / height as f32 * 2.0);
        let near = inverse * Vec4::new(ndc.0, ndc.1, 0.0, 1.0);
        let far = inverse * Vec4::new(ndc.0, ndc.1, 1.0, 1.0);
        let origin = near.truncate() / near.w;
        let direction = (far.truncate() / far.w - origin).normalize();

        let mut best: Option<(f32, Pick)> = scene
            .cast
            .pick(origin, direction)
            .map(|(d, n)| (d, Pick::Noun(n)));
        let bsp = &scene.room.bsp;
        for polygon in &bsp.polygons {
            let surface = &bsp.surfaces[polygon.surface as usize];
            let object = bsp.objects[surface.object as usize].to_ascii_uppercase();
            let noun = scene.nouns.get(&object);
            let is_floor = scene.floor.as_deref() == Some(object.as_str());
            // Hidden geometry is clickable only if it is a hit-test area.
            if scene.room.hidden.contains(&object) && noun.is_none() {
                continue;
            }
            for t in bsp.triangles(polygon) {
                let [a, b, c] = t.map(|i| Vec3::from(bsp.vertices[i as usize]));
                let Some(d) = ray_triangle(origin, direction, a, b, c) else {
                    continue;
                };
                if best.as_ref().is_some_and(|(bd, _)| *bd <= d) {
                    continue;
                }
                let what = match noun {
                    Some(n) => Pick::Noun(n.clone()),
                    None if is_floor => Pick::Floor(origin + direction * d),
                    // Walls and the like block what is behind them.
                    None => Pick::Nothing,
                };
                best = Some((d, what));
            }
        }
        best.map_or(Pick::Nothing, |(_, p)| p)
    }
}

/// The next line's plate: the last character counts 0-9 then A-Z.
pub fn next_plate(plate: &str) -> String {
    let mut chars: Vec<char> = plate.chars().collect();
    if let Some(last) = chars.last_mut() {
        *last = match *last {
            '9' => 'A',
            'Z' | 'z' => '0',
            c => char::from_u32(c as u32 + 1).unwrap_or(c),
        };
    }
    chars.into_iter().collect()
}

/// Real verbs; anything else is an inventory item used on something.
pub fn is_plain_verb(verb: &str) -> bool {
    const VERBS: &[&str] = &[
        "LOOK",
        "TALK",
        "PICKUP",
        "OPEN",
        "CLOSE",
        "PUSH",
        "PULL",
        "USE",
        "THINK",
        "READ",
        "SIT",
        "STAND",
        "KNOCK",
        "LISTEN",
        "MOVE",
        "TURN",
        "EXIT",
        "WALK",
        "INSPECT",
        "INSPECT_UNDO",
        "TOUCH",
        "SEARCH",
        "LOCK",
        "UNLOCK",
        "RING",
        "PLAY",
        "TASTE",
        "SMELL",
        "CLIMB",
        "JUMP",
        "LIE_DOWN",
        "SLEEP",
        "WAKE",
        "Z_CHAT",
        "GO_UP",
        "GO_DOWN",
        "EXIT_UP",
        "EXIT_DOWN",
        "FOLLOW",
        "TURN_LEFT",
        "TURN_RIGHT",
    ];
    verb.starts_with("T_") || VERBS.contains(&verb)
}

/// Sort key: exact timeblocks and ranges, then days, then every day.
fn specificity(file: &str) -> u8 {
    let stem = file.trim_end_matches(".NVC");
    let rest = stem.get(3..).unwrap_or("").trim_start_matches('_');
    if rest == "ALL" {
        2
    } else if rest.ends_with("ALL") {
        1
    } else {
        0
    }
}

/// The middle of a BSP object's vertices.
fn bsp_center(bsp: &Bsp, object: &str) -> Option<Vec3> {
    let index = bsp
        .objects
        .iter()
        .position(|o| o.eq_ignore_ascii_case(object))? as u32;
    let mut sum = Vec3::ZERO;
    let mut n = 0;
    for polygon in &bsp.polygons {
        if bsp.surfaces[polygon.surface as usize].object != index {
            continue;
        }
        for t in bsp.triangles(polygon) {
            for i in t {
                sum += Vec3::from(bsp.vertices[i as usize]);
                n += 1;
            }
        }
    }
    (n > 0).then(|| sum / n as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plates_count_up() {
        assert_eq!(next_plate("0NQIB44QR1"), "0NQIB44QR2");
        assert_eq!(next_plate("0NQIB44QR9"), "0NQIB44QRA");
    }

    #[test]
    fn files_sort_most_specific_first() {
        let mut files = vec!["R25_ALL.NVC", "R25_1ALL.NVC", "R25110A.NVC"];
        files.sort_by_key(|f| specificity(f));
        assert_eq!(files, ["R25110A.NVC", "R25_1ALL.NVC", "R25_ALL.NVC"]);
    }
}
