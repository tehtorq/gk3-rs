//! The running game: a location, its people, the story's state and the
//! scripts acting on it.
//!
//! Scripts run on the Sheep machine against [`GameApi`], which performs the
//! story's state and hands everything presentational back as effects. This
//! module performs them: cutting cameras, starting animations, speaking
//! lines, walking people, and timing all of it, releasing each script that
//! waits when what it waits on is over.

use std::collections::{HashMap, VecDeque};
use std::rc::Rc;

use gk3_audio::{Audio, Bus};
use gk3_barn::Library;
use gk3_formats::anm::{Animation, CueKind, SoundPlace};
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
use glam::{Vec2, Vec3, Vec4};

use crate::cast::{Tolerance, Cast, Fidget, ray_triangle, read_anm};
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
    /// Until the full-screen movie is over.
    Movie,
    Walk(String),
    /// Until the actor has walked out of a walk-map region (or stopped).
    LeftRegion(String, u8),
    /// Until a speech's last line is over.
    Speech(u64),
}

/// Lines being said, one after another (`speak`).
struct Speech {
    id: u64,
    /// The lines still to come, by plate.
    plates: VecDeque<String>,
    /// When the line being said ends.
    line_end: f64,
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
    /// Nouns with a verb of their own (`verb=EXIT`): a click does it at
    /// once, and the cursor shows it.
    object_verbs: HashMap<String, String>,
    /// Pieces of the room that are only there to be clicked (`type=hittest`).
    hit_tests: std::collections::HashSet<String>,
    floor: Option<String>,
    /// The scene file's `custom=`, upper case: the kind of room the
    /// original makes (0x51df4c), as `LANGOLIER` or `COORDINATEDEVICE`.
    custom: Option<String>,
    cameras: HashMap<String, RoomCamera>,
    /// The player character's model name.
    pub ego: String,
    /// Where the player may take the camera.
    pub camera_bounds: crate::camera::Bounds,
    /// How actors behave in each conversation, from `[LISTENERS]`.
    listeners: Vec<Listener>,
    /// Models that take no clicks: `type=noClick` ones, and clickable
    /// areas a script has switched off (`DisableHitTestModel`).
    untouchable: std::collections::HashSet<String>,
    /// `[TRIGGERS]`: floor areas (x and z between two corners) that, while
    /// the player stands in one and nothing is playing, act as its noun
    /// with WALK.
    triggers: Vec<Trigger>,
}

#[derive(Debug, Clone, PartialEq)]
struct Trigger {
    noun: String,
    min: (f32, f32),
    max: (f32, f32),
}

/// A trigger's `rect={x1, z1, x2, z2}`, the corners either way round. Read
/// as the original reads it: empty items are skipped (one has a doubled
/// comma), and each number is as much of its item as reads as one (one is
/// `11.03.58`).
fn trigger_rect(value: &str) -> Option<((f32, f32), (f32, f32))> {
    let numbers: Vec<f32> = value
        .split([',', '{', '}'])
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(|t| {
            let mut end = 0;
            let mut dot = false;
            for (i, c) in t.char_indices() {
                let ok = c.is_ascii_digit() || (c == '-' && i == 0) || (c == '.' && !dot);
                if !ok {
                    break;
                }
                dot |= c == '.';
                end = i + c.len_utf8();
            }
            t[..end].parse::<f32>().unwrap_or(0.0)
        })
        .collect();
    let [x1, z1, x2, z2] = numbers[..] else { return None };
    Some(((x1.min(x2), z1.min(z2)), (x1.max(x2), z1.max(z2))))
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

/// The binoculars, while they are up.
pub struct Binocs {
    /// The lookout's location code, and what can be seen from it now.
    lookout: String,
    uses: crate::binocs::Lookout,
    /// The camera before they went up.
    saved: Camera,
    /// Panned out: yaw and pitch, radians.
    pub panned: (f32, f32),
    pub zoomed: Option<Zoomed>,
    pub lookout_code: String,
    /// `BINOCS.SHP` functions waiting their turn.
    queue: Vec<String>,
}

pub struct Zoomed {
    pub place: String,
    pub gpu: GpuScene,
    /// The place's own angle, radians, which the view stays near.
    base: (f32, f32),
    exit: Option<String>,
}

/// The story moving on to another timeblock.
#[derive(Debug, Clone)]
pub struct TimeblockChange {
    pub to: String,
    pub location: String,
}

/// What follows a movie.
#[derive(Debug, Clone)]
pub enum AfterMovie {
    /// Nothing more; a script waiting on it goes on.
    Nothing,
    /// The new timeblock opens: its beginning film has been shown.
    Enter(TimeblockChange),
}

/// A full-screen movie showing. The story stands still under it.
/// Screens the scripts put up: the temple's death screen, the end of the
/// game, and the quit popup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenRequest {
    Death,
    Finished,
    Quit,
}

pub struct Showing {
    pub name: String,
    pub movie: crate::movie::Movie,
    /// When it began, by the caller's clock; none until its first update.
    started: Option<f64>,
    /// Seconds into it.
    pub elapsed: f64,
    sound: Option<gk3_audio::SoundId>,
    after: AfterMovie,
    /// Its captions (a YAK of the same name), in seconds into it.
    pub captions: Vec<Caption>,
    /// G-Engine's autoclose off: once over, its last frame stays up while
    /// the story goes on beneath, until a scene change, a new timeblock,
    /// another movie or the player takes it away.
    keep_last: bool,
    /// It has played to its end and only its last frame is left.
    pub ended: bool,
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
    /// What footsteps sound like, by shoes and floor.
    footsteps: gk3_game::footsteps::Footsteps,
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
    /// Something to turn to face when an approach walk ends, and within
    /// what of it (radians).
    face_after: Option<(String, f32)>,
    /// The trigger the player was last found in (its noun), as the
    /// original's walker keeps it (+0x374): a trigger trips on coming into
    /// it, not again until it has been left.
    last_trigger: Option<String>,
    next_location: Option<String>,
    /// A door's sound on going through it to another room (not in the
    /// original). The action under way is on a door; its change of room
    /// was asked for by it; and a door sound of its own has played.
    pub door_sounds: bool,
    door_action: bool,
    door_exit: bool,
    door_heard: bool,
    /// A timeblock that has ended: where the story goes once the player has
    /// seen its title card.
    pub timeblock_change: Option<TimeblockChange>,
    /// The full-screen movie showing, if any.
    pub showing: Option<Showing>,
    /// A screen the story asked for, for the window to put up.
    pub screen: Option<ScreenRequest>,
    /// `SetForcedCameraCuts`: the camera is the story's alone, even when
    /// nothing plays, until `ClearForcedCameraCuts`.
    pub forced_cuts: bool,
    /// What the camera has gone in close on (INSPECT), and where it was
    /// before, to go back to; and the thing last acted on, which
    /// `InspectObject` inspects.
    inspecting: Option<String>,
    inspect_from: Option<Camera>,
    active_object: Option<String>,
    /// `SetGameTimer`: actions to do once their time has run down (and
    /// nothing else plays), as G-Engine's `GameTimers`: seconds left, noun,
    /// verb. They last across rooms, and are saved.
    game_timers: Vec<(f64, String, String)>,
    /// `SetVerbModal`: the verb menu must be answered, not dismissed; and
    /// `StartVerbCancel`/`StopVerbCancel`: whether it offers CANCEL.
    pub verb_modal: bool,
    pub verb_cancel: bool,
    /// Movies are not shown, only noted (headless runs).
    pub skip_movies: bool,
    /// How long the story has stood still under movies: the clock the
    /// story sees is the caller's less this.
    paused: f64,
    /// The caller's clock at the last update.
    outside_clock: f64,
    /// Everything stands still, its sound too, as under G-Engine's menus
    /// and popups; and how long it has stood so, which movies leave out.
    held: bool,
    held_for: f64,
    /// The driving map, while it is up; who a script asked to follow on
    /// it; and a departure waiting for the map's last words to end.
    pub map: Option<crate::map::MapScreen>,
    map_follow: crate::map::Follow,
    map_leave: Option<(f64, String)>,
    pub binocs: Option<Binocs>,
    /// Grace's GPS, in a `CoordinateDevice` room.
    pub gps: Option<crate::gps::Gps>,
    /// Grace's computer, while it is up.
    pub sidney: Option<crate::sidney::Sidney>,
    /// A sound SIDNEY keeps playing (the anagram parser's scramble).
    sidney_loop: Option<gk3_audio::SoundId>,
    /// Verbs that are actions rather than items or topics, from VERBS.TXT.
    normal_verbs: std::collections::HashSet<String>,
    /// The fingerprint kit, while it is up; whether it closes once the
    /// current line is said; and, for a dirty glass, which glass is waiting
    /// on the player to say whose print it is (true for Wilkes').
    pub fingerprint: Option<crate::fingerprint::Kit>,
    fingerprint_close: bool,
    fingerprint_label: Option<bool>,
    /// The plate of the last line spoken, for `ContinueDialogue`.
    last_plate: Option<String>,
    /// What happened, in order, for logs and tests.
    pub log: Vec<String>,
    yaks: HashMap<String, Option<Animation>>,
    pub audio: Audio,
    /// Sounds due to start, by time.
    scheduled: Vec<(f64, String, f32, Bus, Option<SoundPlace>)>,
    /// Sounds playing from places in the room, heard from the camera.
    placed: Vec<Placed>,
    /// Lines being said, and the last speech's number.
    speeches: Vec<Speech>,
    next_speech: u64,
    /// For headless runs: each line is skipped this long after it starts,
    /// as a player clicking through.
    pub skip_lines_after: Option<f64>,
    /// Lines and camera cuts animations have set off, by time.
    cues: Vec<(f64, CueKind)>,
    /// Each actor's mood, by noun: the animation that ends it.
    moods: HashMap<String, String>,
    /// The church's four angels puzzle.
    angels: crate::angels::Angels,
    /// CS2's turning heads, while there.
    laser_heads: Option<crate::laser_heads::LaserHeads>,
    /// TE1's chessboard, while there; and pieces of the room due to be
    /// shown or hidden later (a tile falling once Gabriel has left it).
    chessboard: Option<crate::chessboard::Chessboard>,
    /// TE3's pendulum and TE5's bridge, while there.
    pendulum: Option<crate::pendulum::Pendulum>,
    bridge: Option<crate::bridge::Bridge>,
    /// TE6, the fight with the demon: a click on the floor is a call for
    /// the scripts' own moves, not a walk.
    demon_fight: bool,
    timed_shows: Vec<(f64, String, bool)>,
    /// Sheep to run as an action once the actions running now are over.
    deferred: Vec<String>,
    /// A script asked for an item's close-up (`Some`), or for it to go
    /// (`None`): for the screen to carry out.
    pub inspect: Option<Option<String>>,
    /// The picture's width over its height, for what the camera sees.
    pub aspect: f32,
    /// For how long walkers held up by each other wait: its own sequence,
    /// so the story's random numbers stay as they were.
    give_way_seed: u64,
    sounds: HashMap<String, Option<std::sync::Arc<gk3_media::Pcm>>>,
    soundtracks: Vec<Soundtrack>,
    /// What soundtracks are playing, by the soundtrack's name.
    soundtrack_sounds: Vec<(String, gk3_audio::SoundId)>,
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
        let characters = shoot::characters(lib);
        Self {
            lib,
            catalog: Catalog::new(lib),
            scripts: scripts.into_iter().collect(),
            faces: lib
                .read("FACES.TXT")
                .map(|b| crate::faces::parse(&b, &characters))
                .unwrap_or_default(),
            characters,
            footsteps: {
                let text = |name: &str| {
                    lib.read(name)
                        .map(|b| gk3_game::state::latin1(&b))
                        .unwrap_or_default()
                };
                gk3_game::footsteps::Footsteps::parse(
                    &text("FLOORMAP.TXT"),
                    &text("FOOTSTEPS.TXT"),
                    &text("FOOTSCUFFS.TXT"),
                )
            },
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
            last_trigger: None,
            next_location: None,
            door_sounds: true,
            door_action: false,
            door_exit: false,
            door_heard: false,
            timeblock_change: None,
            showing: None,
            skip_movies: false,
            paused: 0.0,
            outside_clock: 0.0,
            held: false,
            held_for: 0.0,
            screen: None,
            forced_cuts: false,
            inspecting: None,
            inspect_from: None,
            active_object: None,
            game_timers: Vec::new(),
            verb_modal: false,
            verb_cancel: true,
            map: None,
            map_follow: crate::map::Follow::None,
            map_leave: None,
            binocs: None,
            gps: None,
            sidney: None,
            sidney_loop: None,
            normal_verbs: lib
                .read("VERBS.TXT")
                .map(|b| normal_verbs(&gk3_game::state::latin1(&b)))
                .unwrap_or_default(),
            fingerprint: None,
            fingerprint_close: false,
            fingerprint_label: None,
            last_plate: None,
            log: Vec::new(),
            yaks: HashMap::new(),
            audio,
            scheduled: Vec::new(),
            placed: Vec::new(),
            speeches: Vec::new(),
            next_speech: 0,
            skip_lines_after: None,
            cues: Vec::new(),
            moods: HashMap::new(),
            angels: Default::default(),
            laser_heads: None,
            chessboard: None,
            pendulum: None,
            bridge: None,
            demon_fight: false,
            timed_shows: Vec::new(),
            deferred: Vec::new(),
            inspect: None,
            aspect: 16.0 / 9.0,
            give_way_seed: 0x9e37_79b9_7f4a_7c15,
            sounds: HashMap::new(),
            soundtracks: Vec::new(),
            soundtrack_sounds: Vec::new(),
        }
    }

    /// The game as save-file text. Only when the player is free: a running
    /// script cannot be saved.
    pub fn save(&self) -> Result<String, String> {
        // On a title card the story is between timeblocks: it saves as the
        // next one about to start, as the card's own Save button does.
        if let Some(change) = &self.timeblock_change
            && self.actions.is_empty()
        {
            let mut state = self.state.clone();
            if change.to != state.timeblock {
                state.set_timeblock(&change.to);
            }
            state.last_location = std::mem::replace(&mut state.location, change.location.clone());
            return Ok(state.to_save(&[]));
        }
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
        if self.forced_cuts {
            extra.push(vec!["forced_cuts".into()]);
        }
        if let Some(scene) = &self.scene
            && scene.room.name != scene.room.built_as
        {
            extra.push(vec!["scene".into(), scene.room.name.clone()]);
        }
        for (left, noun, verb) in &self.game_timers {
            extra.push(vec!["game_timer".into(), num(*left as f32), noun.clone(), verb.clone()]);
        }
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
        let forced_cuts = extra.iter().any(|r| r[0] == "forced_cuts");
        let swapped = extra
            .iter()
            .find(|r| r[0] == "scene" && r.len() == 2)
            .map(|r| r[1].clone());
        self.game_timers = extra
            .iter()
            .filter(|r| r[0] == "game_timer" && r.len() == 4)
            .filter_map(|r| Some((r[1].parse().ok()?, r[2].clone(), r[3].clone())))
            .collect();
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
        self.showing = None;
        self.audio.solo(None);
        self.vm = Vm::new();
        self.tasks.clear();
        self.speeches.clear();
        self.actions.clear();
        self.captions.clear();
        self.scheduled.clear();
        self.cues.clear();
        self.moods.clear();
        self.angels = Default::default();
        self.deferred.clear();
        self.audio.stop_all();
        self.topic_bar = None;
        self.end_conversation = false;
        self.approach = None;
        self.face_after = None;
        self.next_location = None;
        let last_location = state.last_location.clone();
        self.state = state;
        let location = self.state.location.clone();
        self.map = None;
        self.map_leave = None;
        if location == "MAP" {
            self.state.location = last_location.clone();
            self.open_map(crate::map::Follow::None);
            return Ok(());
        }
        let gps_on = self.state.flags.contains(&gk3_game::state::key("GPSOn"));
        self.enter(renderer, &location)?;
        // A game saved with the GPS on has it up and on again (0x4701d2).
        if gps_on && let Some(gps) = &mut self.gps {
            gps.restore();
            self.state.flags.insert(gk3_game::state::key("GPSOn"));
        }
        if let Some(name) = swapped {
            self.set_scene(renderer, &name);
        }
        self.state.last_location = last_location;
        self.restore = Some(Restore { ego_at, camera });
        self.forced_cuts = forced_cuts;
        Ok(())
    }

    /// What keeps the player from acting, for logs.
    pub fn waiting_on(&self) -> String {
        let mut why = Vec::new();
        if let Some(a) = &self.approach {
            why.push(format!("approaching for {}:{}", a.rule.noun, a.rule.verb));
        }
        for t in &self.tasks {
            why.push(match &t.until {
                Until::Time(at) => format!("a script waits until {at:.1}s"),
                Until::Movie => "a script waits for a movie".into(),
                Until::Walk(who) => format!("a script waits for {who} to arrive"),
                Until::LeftRegion(who, r) => format!("a script waits for {who} to leave region {r}"),
                Until::Speech(_) => "a script waits for lines to be said".into(),
            });
        }
        if !self.actions.is_empty() && self.tasks.is_empty() {
            why.push(format!("{} action(s) running", self.actions.len()));
        }
        if self.next_location.is_some() {
            why.push("going elsewhere".into());
        }
        if self.timeblock_change.is_some() {
            why.push("the timeblock is ending".into());
        }
        why.join(", ")
    }

    /// Whether the player is free to act.
    /// Where the actors are now, for the scripts' questions.
    pub fn whereabouts(&self) -> gk3_game::api::Whereabouts {
        self.scene
            .as_ref()
            .map(|s| s.cast.whereabouts())
            .unwrap_or_default()
    }

    pub fn idle(&self) -> bool {
        self.actions.is_empty()
            && self.approach.is_none()
            && self.next_location.is_none()
            && self.timeblock_change.is_none()
            && self.binocs.as_ref().is_none_or(|b| b.queue.is_empty())
    }

    /// Goes to a location: builds it as the story has it now, counts the
    /// visit, and runs its `SCENE ENTER` action.
    pub fn enter(&mut self, renderer: &Renderer, location: &str) -> Result<(), String> {
        let location = location.to_ascii_uppercase();
        // The GPS is put away on leaving its room (0x46f391).
        if self.gps.take().is_some_and(|mut g| g.leave()) {
            self.state.flags.remove(&gk3_game::state::key("GPSOn"));
        }
        // A script's SetLocation has already moved the ego; anything else
        // (the map, a new game) moves them now. Either way the room's file
        // is read as the story stands on arrival, as G-Engine reads it.
        if self.state.location != gk3_game::state::key(&location) {
            self.state.move_to(&location);
        }
        self.catalog.time = Some(self.state.timeblock.clone());
        self.catalog.state = Some(self.state.clone());
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
        let mut object_verbs = HashMap::new();
        let mut hit_tests = std::collections::HashSet::new();
        // `type=noClick` models take no clicks: they pass on to what is
        // behind (TE3's floor, over the hidden one walked on).
        let untouchable: std::collections::HashSet<String> = sif
            .sections("MODELS")
            .flat_map(|s| &s.lines)
            .filter(|l| l.get("type").is_some_and(|t| t.trim().eq_ignore_ascii_case("noclick")))
            .filter_map(|l| l.get("model").map(str::to_ascii_uppercase))
            .collect();
        let triggers: Vec<Trigger> = sif
            .sections("TRIGGERS")
            .flat_map(|s| &s.lines)
            .filter_map(|l| {
                let (min, max) = trigger_rect(l.get("rect")?)?;
                Some(Trigger {
                    noun: l.get("noun")?.trim().to_ascii_uppercase(),
                    min,
                    max,
                })
            })
            .collect();
        for line in sif.sections("MODELS").flat_map(|s| &s.lines) {
            let (Some(model), Some(noun)) = (line.get("model"), line.get("noun")) else {
                continue;
            };
            if let Some(verb) = line.get("verb").filter(|v| !v.trim().is_empty()) {
                object_verbs.insert(noun.to_ascii_uppercase(), verb.trim().to_ascii_uppercase());
            }
            let kind = line.get("type").unwrap_or("scene").to_ascii_lowercase();
            if kind == "hittest" {
                hit_tests.insert(model.to_ascii_uppercase());
            }
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
        // Each section read sets it again, so the last one given holds
        // (0x50fcbc).
        let custom = sif
            .sections("GENERAL")
            .flat_map(|s| &s.lines)
            .filter_map(|l| l.get("custom"))
            .last()
            .map(|c| c.trim().to_ascii_uppercase());
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
        for warning in &cast.warnings {
            self.log.push(format!("  cast: {warning}"));
        }
        // The floor's triangles, for standing shadows on.
        if let Some(floor) = &floor {
            let bsp = &room.bsp;
            let triangles = bsp
                .polygons
                .iter()
                .filter(|p| {
                    let surface = &bsp.surfaces[p.surface as usize];
                    bsp.objects[surface.object as usize].eq_ignore_ascii_case(floor)
                })
                .flat_map(|p| bsp.triangles(p))
                .map(|t| t.map(|i| Vec3::from(bsp.vertices[i as usize])))
                .collect();
            cast.set_floor(triangles);
        }
        self.scene = Some(Scene {
            room,
            gpu,
            cast,
            actions: set,
            walk,
            nouns,
            object_verbs,
            hit_tests,
            floor,
            custom,
            cameras,
            ego,
            camera_bounds,
            listeners,
            untouchable,
            triggers,
        });
        // The camera's forced cuts are the old room camera's, as G-Engine;
        // so is anything it was inspecting.
        self.forced_cuts = false;
        self.inspecting = None;
        self.inspect_from = None;
        self.tasks.clear();
        self.speeches.clear();
        self.captions.clear();
        for (_, id) in self.soundtrack_sounds.drain(..) {
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
        // The room's own code first, as G-Engine's `Scene::Load` runs its
        // `Init` scene function before the room is entered.
        self.laser_heads = None;
        self.chessboard = None;
        self.pendulum = None;
        self.bridge = None;
        self.demon_fight = false;
        self.last_trigger = None;
        // A `CoordinateDevice` room has the GPS, off (0x46f37a).
        if self.scene.as_ref().and_then(|s| s.custom.as_deref()) == Some("COORDINATEDEVICE") {
            self.gps = self
                .lib
                .read("GPS.TXT")
                .ok()
                .and_then(|txt| crate::gps::Gps::new(&txt, &location));
            self.state.flags.remove(&gk3_game::state::key("GPSOn"));
        }
        self.timed_shows.clear();
        self.scene_function(renderer, "init");
        self.state.count_visit();

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

    /// Does what the GPS asks of the game.
    fn gps_effects(&mut self, effects: Vec<crate::gps::Effect>) {
        for effect in effects {
            match effect {
                crate::gps::Effect::Flag(flag, on) => {
                    let flag = gk3_game::state::key(flag);
                    if on {
                        self.state.flags.insert(flag);
                    } else {
                        self.state.flags.remove(&flag);
                    }
                }
                crate::gps::Effect::Script(text) => self.deferred.push(text),
            }
        }
    }

    /// The GPS's power button pressed (0x470167): it turns off, and Grace's
    /// use of the device counts once, so the scripts take it as put away.
    pub fn gps_power(&mut self) {
        let no_turn_off = self.state.flags.contains(&gk3_game::state::key("NoGPSTurnOff"));
        let now = self.clock;
        if self.gps.as_mut().is_some_and(|g| g.press_power(now, no_turn_off)) {
            self.log.push("  (the GPS's power is pressed)".into());
            let ego = self.state.ego.clone();
            self.state
                .set_noun_verb(&ego, "GRACE", "COORDINATE_FIXING_DEVICE", 1);
        }
    }

    /// What can be done to a noun now.
    /// One of the engine's own functions for the room, by G-Engine's
    /// `SceneFunctions`: the church's angels, CS2's laser heads, and two of
    /// G-Engine's mends for faults in the original's files. None if the
    /// room has no such function; else what a script must wait for.
    fn scene_function(&mut self, renderer: &Renderer, function: &str) -> Option<Option<Until>> {
        let location = self.state.location.to_ascii_uppercase();
        let f = function.to_ascii_lowercase();
        // The GPS's room takes `On` and `Off` for the device (0x46f3c5),
        // as any name starting so.
        if let Some(gps) = &mut self.gps {
            let now = self.clock;
            let wait = if f.starts_with("on") {
                let (wait, effects) = gps.turn_on(now);
                self.gps_effects(effects);
                Some(wait)
            } else if f.starts_with("off") {
                Some(gps.turn_off(now))
            } else {
                None
            };
            if let Some(wait) = wait {
                return Some(wait.map(|w| Until::Time(now + w)));
            }
        }
        match (location.as_str(), f.as_str()) {
            ("CHU", _) => {
                let ego = self.state.ego.clone();
                for effect in self.angels.call(&f, &ego)? {
                    use crate::angels::Effect as A;
                    match effect {
                        A::Show(model, show) => self.show_model(renderer, &model, show),
                        A::Count(verb, n) => {
                            for who in ["GABRIEL", "GRACE"] {
                                self.state.set_noun_verb(who, "FOUR_ANGELS", verb, n);
                            }
                        }
                        A::FirstDot(plate) => self
                            .deferred
                            .push(format!("wait StartDialogue(\"{plate}\", 1);")),
                        A::Solved(plate) => self.deferred.push(format!(
                            "wait StartDialogue(\"{plate}\", 1); wait SetTimerSeconds(2); CallSceneFunction(\"Erase\");"
                        )),
                    }
                }
                Some(None)
            }
            // The heads start facing the middle, every time.
            ("CS2", "init") => {
                let heads = crate::laser_heads::LaserHeads::default();
                for (i, turn) in heads.turns().iter().enumerate() {
                    self.state
                        .variables
                        .insert(crate::laser_heads::variable(i), *turn as i32);
                }
                self.laser_heads = Some(heads);
                self.place_laser_heads();
                Some(None)
            }
            ("CS2", "togglelasers") => {
                let on = {
                    let heads = self.laser_heads.as_mut()?;
                    heads.lasers_on = !heads.lasers_on;
                    heads.lasers_on
                };
                for i in 0..5 {
                    self.show_model(renderer, &crate::laser_heads::laser_model(i), on);
                }
                // Turning the lasers on also ends the library's music.
                self.stop_soundtrack(Some("NocturneSlow"));
                Some(None)
            }
            ("CS2", f) if f.starts_with("turnl") || f.starts_with("turnr") => {
                let head: usize = f[5..].parse::<usize>().ok()?.checked_sub(1)?;
                let step = if f.starts_with("turnl") { 1 } else { -1 };
                let anim = self.laser_heads.as_mut()?.turn(head, step);
                let Some(anim) = anim else {
                    return Some(None);
                };
                let turn = self.laser_heads.as_ref()?.turns()[head];
                self.state
                    .variables
                    .insert(crate::laser_heads::variable(head), turn as i32);
                // The script waits while Grace turns it.
                let effect = Effect {
                    name: "StartAnimation".into(),
                    args: vec![gk3_sheep::Value::from(anim.as_str()), gk3_sheep::Value::Int(1)],
                    thread: None,
                };
                Some(self.perform_one(renderer, &effect))
            }
            // TE1's chessboard: the scripts reset it, and call back as
            // Gabriel takes off, lands, falls or needs straightening.
            // In the fight Gabriel does not walk where clicked: the click
            // is a flag the scripts answer with his backing away, as
            // G-Engine's `DemonFight`.
            ("TE6", "init") => {
                self.demon_fight = true;
                Some(None)
            }
            ("TE5", "init") => {
                let mut bridge = crate::bridge::Bridge::default();
                bridge.start(self);
                self.bridge = Some(bridge);
                Some(None)
            }
            ("TE3", "init") => {
                let mut pendulum = crate::pendulum::Pendulum::default();
                pendulum.start(self);
                self.pendulum = Some(pendulum);
                Some(None)
            }
            ("TE1", "init") => {
                self.chessboard = Some(Default::default());
                Some(None)
            }
            ("TE1", f) => {
                let board = self.chessboard.as_mut()?;
                let state = &mut self.state;
                let effects = match f {
                    "cleartiles" => board.reset(false, state),
                    "reset" => board.reset(true, state),
                    "takeoff" => board.takeoff(state),
                    "landed" => board.landed(state),
                    "hidecurrenttile" | "badland" => board.drop_current(state),
                    "centerme" => board.center(state),
                    // G-Engine does nothing here either.
                    "fell" => Vec::new(),
                    _ => return None,
                };
                self.chessboard_effects(renderer, effects);
                Some(None)
            }
            // The original leaves a flag set if the player leaves during
            // Lady Howard and Estelle's turn in the museum, and eavesdropping
            // after that waits for ever; G-Engine clears it on the way in.
            ("MS3", "init") => {
                self.state.flags.remove(&gk3_game::state::key("TE6Topics"));
                Some(None)
            }
            // A one-pixel gap in CSE's walk map lets Montreaux walk through a
            // door at 2pm on Day 2 and 3pm on Day 3: G-Engine closes it.
            ("CSE", "init") => {
                if matches!(self.state.timeblock.as_str(), "202P" | "303P")
                    && let Some(walk) = self.scene.as_mut().and_then(|s| s.walk.as_mut())
                {
                    walk.set_blocked(6, true);
                }
                Some(None)
            }
            // Every other room has nothing to do on the way in.
            (_, "init") => Some(None),
            _ => None,
        }
    }

    /// Does what the chessboard asks of the room.
    fn chessboard_effects(&mut self, renderer: &Renderer, effects: Vec<crate::chessboard::Effect>) {
        use crate::chessboard::Effect as E;
        let mut retextured = false;
        for effect in effects {
            match effect {
                E::Show(model, show) => self.show_model(renderer, &model, show),
                E::ShowLater(model, show, after) => {
                    self.timed_shows.push((self.clock + after, model, show));
                }
                E::Texture(model, texture) => {
                    if let Some(scene) = &mut self.scene {
                        retextured |= scene.room.retexture(self.lib, &model, texture);
                    }
                }
                E::Sound(name) => {
                    self.sound(name, 1.0, Bus::Effects);
                }
                E::DoorShut => {
                    if let Some(scene) = &mut self.scene {
                        scene.cast.sample_first_frame(self.lib, "Te1GoDoor");
                    }
                }
                E::Center(tile) => {
                    let Some(scene) = &mut self.scene else { continue };
                    let Some((lo, hi)) = bsp_bounds(&scene.room.bsp, &tile) else {
                        continue;
                    };
                    let ego = scene.ego.clone();
                    if let Some((at, _)) = scene.cast.actor_position(&ego) {
                        let middle = Vec3::new((lo.x + hi.x) / 2.0, at.y, (lo.z + hi.z) / 2.0);
                        scene.cast.put(&ego, middle, 0.0);
                    }
                }
            }
        }
        if retextured && let Some(scene) = &mut self.scene {
            scene.room.rebuild();
            scene.gpu = renderer.upload(&scene.room.mesh);
        }
    }

    /// The piece of the room under the pointer: on TE1's chessboard the
    /// scripts learn which tile, and whether Gabriel can jump to it, as
    /// G-Engine's `Chessboard::OnUpdate` tells them.
    pub fn hover_object(&mut self, object: Option<&str>) {
        if self.chessboard.is_none() || !self.idle() {
            return;
        }
        if let Some((row, col)) = object.and_then(crate::chessboard::tile_of) {
            crate::chessboard::Chessboard::hover(&mut self.state, row, col);
        }
    }

    /// The cursor the room's own code wants over something, if any (the
    /// pendulum's grab).
    pub fn object_cursor(&self, object: Option<&str>) -> Option<&'static str> {
        let object = object?;
        if let Some(pendulum) = &self.pendulum {
            return pendulum.cursor(object);
        }
        self.bridge.as_ref()?.cursor(object)
    }

    /// For headless runs: works TE5's bridge across, a moment at a time,
    /// as a player would click. True once over.
    pub fn bridge_cross(&mut self, renderer: &Renderer, step: f64) -> bool {
        // Across and gone on (`GabeCrossedBridge$` leaves for TE6).
        let Some(bridge) = &self.bridge else {
            return !self.state.location.eq_ignore_ascii_case("TE5");
        };
        if bridge.crossed() {
            return true;
        }
        if self.idle()
            && let Some(model) = bridge.advice()
        {
            self.click_object(renderer, Some(&model));
        }
        let t = self.outside() + step;
        self.update(renderer, t);
        false
    }

    /// Plays an animation for the room's own code, as `StartAnimation`
    /// would; its length in seconds.
    fn start_anim(&mut self, name: &str, looping: bool) -> f32 {
        let (lib, now) = (self.lib, self.clock);
        let Some(anm) = read_anm(lib, name) else {
            self.log.push(format!("    (no animation {name})"));
            return 0.0;
        };
        let Some(scene) = &mut self.scene else {
            return 0.0;
        };
        let moved = scene.cast.play_parsed(lib, name, &anm, looping, now);
        for f in &anm.faces {
            scene
                .cast
                .face(&f.noun, now + f.frame as f64 / 15.0, f.change.clone());
        }
        self.schedule(&anm, now);
        moved.unwrap_or(anm.frames as f32 / 15.0)
    }

    /// For headless runs: works TE3's pendulum towards a goal (`GRAB` the
    /// blade, `DROP` onto the altar), a moment at a time, as a player
    /// would click. True once done.
    pub fn pendulum_goal(&mut self, renderer: &Renderer, goal: &str, step: f64) -> bool {
        let Some(pendulum) = &self.pendulum else {
            return false;
        };
        let done = match goal {
            "GRAB" => pendulum.gabe() == crate::pendulum::Gabe::OnPendulum,
            "DROP" => pendulum.gabe() == crate::pendulum::Gabe::AtAltar,
            _ => return true,
        };
        if done {
            return true;
        }
        if let Some(model) = pendulum.advice(goal) {
            self.click_object(renderer, Some(&model));
        }
        let t = self.outside() + step;
        self.update(renderer, t);
        false
    }

    /// The noun a piece of the room stands for.
    pub fn noun_of_object(&self, object: &str) -> Option<String> {
        self.scene.as_ref()?.nouns.get(&object.to_ascii_uppercase()).cloned()
    }

    /// A click on a piece of the room that its own code takes, rather than
    /// a walk or a verb: on TE1's board, the floor round it is a jump back
    /// off (or too far). True if the click is taken.
    pub fn click_object(&mut self, renderer: &Renderer, object: Option<&str>) -> bool {
        if let Some(mut pendulum) = self.pendulum.take() {
            let taken = pendulum.click(object.unwrap_or(""), self);
            self.pendulum = Some(pendulum);
            return taken;
        }
        if let Some(mut bridge) = self.bridge.take() {
            let taken = bridge.click(object.unwrap_or(""), self);
            self.bridge = Some(bridge);
            return taken;
        }
        if self.demon_fight {
            let floor = self.scene.as_ref().and_then(|s| s.floor.clone());
            if object.is_some() && object.map(str::to_ascii_uppercase) == floor {
                if !self.state.flags.contains(&gk3_game::state::key("Te6GabeWalk")) {
                    self.state.flags.insert(gk3_game::state::key("Te6ClickedOnFloor"));
                }
                return true;
            }
            return false;
        }
        if self.chessboard.is_none() || !self.idle() {
            return false;
        }
        let gabe_on_board = self
            .state
            .variables
            .get(&gk3_game::state::key("Te1GabeRow"))
            .is_some_and(|r| *r >= 0);
        if !gabe_on_board {
            return false;
        }
        let object = object.map(str::to_ascii_uppercase);
        match object.as_deref() {
            Some("TE1TILEFLOOR") => {
                if crate::chessboard::Chessboard::floor_click(&mut self.state)
                    && let Some(rule) = self.rule_for("SCENE", "JUMP")
                {
                    self.run(renderer, rule, true);
                }
                true
            }
            // Elsewhere on the floor nothing happens: no walking off.
            Some("TE1_HITTESTFLOOR" | "TE1FLOOREDGES" | "TE1_FLOORSTAND") => true,
            _ => false,
        }
    }

    /// Stands CS2's heads and lasers where they face now.
    fn place_laser_heads(&mut self) {
        let (Some(heads), Some(scene)) = (&self.laser_heads, &mut self.scene) else {
            return;
        };
        for i in 0..5 {
            scene
                .cast
                .set_world(&crate::laser_heads::head_model(i), heads.head_world(i));
            scene
                .cast
                .set_world(&crate::laser_heads::laser_model(i), heads.laser_world(i));
        }
    }

    /// Whether a noun is something in the room: a piece of it, or someone
    /// or something placed in it.
    fn in_scene(&self, noun: &str) -> bool {
        let Some(scene) = &self.scene else {
            return false;
        };
        let noun = noun.to_ascii_uppercase();
        scene.nouns.values().any(|n| *n == noun) || scene.cast.find(&noun).is_some()
    }

    /// A noun's extent: its figure's, or every piece of the room it is.
    fn noun_bounds(&self, noun: &str) -> Option<(Vec3, Vec3)> {
        let scene = self.scene.as_ref()?;
        if let Some(b) = scene.cast.bounds(noun) {
            return Some(b);
        }
        let upper = noun.to_ascii_uppercase();
        scene
            .nouns
            .iter()
            .filter(|(_, n)| **n == upper)
            .filter_map(|(object, _)| bsp_bounds(&scene.room.bsp, object))
            .reduce(|(a, b), (c, d)| (a.min(c), b.max(d)))
    }

    /// Where the camera goes to look at something close up, as G-Engine's
    /// `Scene::InspectObject`: its inspect camera, by noun or by the piece
    /// or model it is; else, for someone, just before their face; else
    /// near enough that it fills the view, on the line from the camera.
    fn inspect_camera(&self, noun: &str) -> Option<Camera> {
        let scene = self.scene.as_ref()?;
        let upper = noun.to_ascii_uppercase();
        let mut names = vec![upper.clone()];
        names.extend(
            scene
                .nouns
                .iter()
                .filter(|(_, n)| **n == upper)
                .map(|(o, _)| o.clone()),
        );
        if let Some(i) = scene.cast.find(&upper) {
            names.push(scene.cast.name_of(i));
        }
        if let Some(c) = scene
            .room
            .cameras
            .iter()
            .find(|c| c.inspect && names.iter().any(|n| c.name.eq_ignore_ascii_case(n)))
        {
            return Some(shoot::from_room_camera(c));
        }
        let fov_y = self.camera.fov_y;
        if let Some((at, heading)) = scene.cast.actor_position(&upper) {
            // Their face: a little below the top of their character's
            // height above where they stand.
            let model = scene.cast.find(&upper).map(|i| scene.cast.name_of(i));
            let height = model
                .and_then(|m| self.characters.get(&m))
                .map_or(65.0, |c| c.walker_height);
            let head = at + Vec3::Y * (height - 10.0);
            let facing = Vec3::new(heading.sin(), 0.0, heading.cos());
            return Some(Camera {
                position: head + facing * 50.0,
                yaw: heading + std::f32::consts::PI,
                pitch: 0.0,
                fov_y,
            });
        }
        let (lo, hi) = self.noun_bounds(&upper)?;
        let center = (lo + hi) * 0.5;
        let size = hi - lo;
        let distance = (size.x.max(size.y).max(size.z) / fov_y.sin()).abs();
        let to_camera = (self.camera.position - center).normalize_or(Vec3::NEG_Z);
        Some(Camera {
            position: center + to_camera * distance,
            yaw: (-to_camera.x).atan2(-to_camera.z),
            pitch: to_camera.y.clamp(-1.0, 1.0).asin(),
            fov_y,
        })
    }

    /// Goes in close on something, remembering where the camera was.
    fn inspect(&mut self, noun: &str, camera: Option<Camera>) {
        let Some(camera) = camera.or_else(|| self.inspect_camera(noun)) else {
            return;
        };
        if self.inspecting.is_none() {
            self.inspect_from = Some(self.camera);
        }
        self.camera = camera;
        self.inspecting = Some(noun.to_ascii_uppercase());
        self.log.push(format!(
            "    (inspects {noun} from {:.0},{:.0},{:.0} facing {:.0}°)",
            camera.position.x,
            camera.position.y,
            camera.position.z,
            camera.yaw.to_degrees()
        ));
    }

    /// Backs away from what was inspected, to where the camera was.
    fn uninspect(&mut self) {
        if self.inspecting.take().is_some()
            && let Some(c) = self.inspect_from.take()
        {
            self.camera = c;
        }
    }

    /// The player has moved the camera: whatever was inspected no longer
    /// is, as G-Engine.
    pub fn camera_moved(&mut self) {
        self.inspecting = None;
        self.inspect_from = None;
    }

    /// Counts the game timers down; one whose time is up does its action
    /// once nothing else is playing, as G-Engine's `GameTimers::Update`.
    fn run_game_timers(&mut self, renderer: &Renderer, dt: f64) {
        for timer in &mut self.game_timers {
            timer.0 -= dt;
        }
        while self.actions.is_empty()
            && let Some(i) = self.game_timers.iter().position(|t| t.0 <= 0.0)
        {
            let (_, noun, verb) = self.game_timers.remove(i);
            match self.rule_for(&noun, &verb) {
                Some(rule) => {
                    self.log.push(format!("  (timer: {noun}:{verb})"));
                    self.run(renderer, rule, false);
                }
                None => self.log.push(format!("  (timer: no {noun}:{verb} now)")),
            }
        }
    }

    /// The action for a noun and verb as the story stands, if any.
    fn rule_for(&self, noun: &str, verb: &str) -> Option<Rule> {
        let scene = self.scene.as_ref()?;
        let snapshot = self.state.clone();
        let mut probe = snapshot.clone();
        let whereabouts = self.whereabouts();
        let mut api = GameApi::new(&mut probe, &self.scripts);
        api.whereabouts = Some(&whereabouts);
        let verb = verb.to_ascii_uppercase();
        scene
            .actions
            .available(noun, &mut api, &snapshot)
            .into_iter()
            .find(|r| r.verb == verb)
            .cloned()
    }

    /// The verb a noun does by itself when clicked, if it has one.
    pub fn object_verb(&self, noun: &str) -> Option<&str> {
        self.scene
            .as_ref()?
            .object_verbs
            .get(&noun.to_ascii_uppercase())
            .map(String::as_str)
    }

    pub fn verbs(&self, noun: &str) -> Vec<Rule> {
        let Some(scene) = &self.scene else {
            return Vec::new();
        };
        let snapshot = self.state.clone();
        let mut probe = snapshot.clone();
        let whereabouts = self.whereabouts();
        let mut api = GameApi::new(&mut probe, &self.scripts);
        api.whereabouts = Some(&whereabouts);
        let mut out: Vec<Rule> = scene
            .actions
            .available(noun, &mut api, &snapshot)
            .into_iter()
            // Topics and chat belong to the topic bar, after TALK; using an
            // item is offered below, for the item in hand.
            .filter(|r| {
                !actions::is_topic(&r.verb)
                    && !matches!(r.verb.as_str(), "Z_CHAT" | "CANCEL")
                    && (self.normal_verbs.contains(&r.verb)
                        || (self.normal_verbs.is_empty() && is_plain_verb(&r.verb)))
            })
            .cloned()
            .collect();
        // As G-Engine's action bar: INSPECT first, the rest by name.
        out.sort_by_key(|r| (!matches!(r.verb.as_str(), "INSPECT" | "INSPECT_UNDO"), r.verb.clone()));
        // Anything in the room can be looked at close up, and then backed
        // away from, as G-Engine's `Scene::Interact` adds: INSPECT, or
        // INSPECT_UNDO while it is the one inspected. Not while the camera
        // is the story's.
        if !self.forced_cuts && self.in_scene(noun) {
            let inspecting = self
                .inspecting
                .as_deref()
                .is_some_and(|n| n.eq_ignore_ascii_case(noun));
            let (drop, add, script) = if inspecting {
                ("INSPECT", "INSPECT_UNDO", "wait Uninspect();")
            } else {
                ("INSPECT_UNDO", "INSPECT", "wait InspectObject();")
            };
            out.retain(|r| r.verb != drop);
            if !out.iter().any(|r| r.verb == add) {
                out.insert(
                    0,
                    Rule {
                        noun: gk3_game::state::key(noun),
                        verb: add.into(),
                        case: "ALL".into(),
                        approach: None,
                        target: None,
                        script: script.into(),
                        source: "ENGINE".into(),
                    },
                );
            }
        }
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
        let whereabouts = self.whereabouts();
        let mut api = GameApi::new(&mut probe, &self.scripts);
        api.whereabouts = Some(&whereabouts);
        let available = scene.actions.available(noun, &mut api, &snapshot);
        let mut out: Vec<Rule> = available
            .iter()
            .filter(|r| actions::is_topic(&r.verb))
            .map(|r| (*r).clone())
            .collect();
        out.sort_by(|a, b| a.verb.cmp(&b.verb));
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
        self.call(renderer, "GLB_ALL", "CodeCallEndConv$");
        self.conversation = None;
    }

    /// Runs one function of a compiled script as an action the player waits
    /// for. Returns whether the script has that function.
    pub fn call(&mut self, renderer: &Renderer, script_name: &str, function: &str) -> bool {
        let Some(script) = self.scripts.get(&script_name.to_ascii_uppercase()).cloned() else {
            return false;
        };
        let (result, effects) = {
            let whereabouts = self.whereabouts();
            let mut api = GameApi::new(&mut self.state, &self.scripts);
            api.whereabouts = Some(&whereabouts);
            let result = self.vm.start(&mut api, script, script_name, function);
            (result, std::mem::take(&mut api.effects))
        };
        let Ok(id) = result else { return false };
        let rule = Rule {
            noun: script_name.to_ascii_uppercase(),
            verb: function.to_string(),
            case: "ALL".into(),
            approach: None,
            target: None,
            script: String::new(),
            source: script_name.to_ascii_uppercase(),
        };
        self.actions.push((id, rule, false));
        self.perform(renderer, effects);
        self.finish_actions();
        true
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
        // Saying whose print a glass held is a topic, but no conversation.
        if self.fingerprint_label.is_some() && actions::is_topic(&rule.verb) {
            self.fingerprint_labelled(&rule.verb);
            return;
        }
        self.log.push(format!(
            "do {}:{} [{}] from {}",
            rule.noun, rule.verb, rule.case, rule.source
        ));
        self.active_object = Some(rule.noun.clone());
        self.door_heard = false;
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
            // As G-Engine's `Walker::WalkToSee`: no walk if it is already
            // in view, only a turn; else towards it until it comes into
            // view.
            "walktosee" => match self.walk_to_see(&target) {
                Some(goal) => Some((goal, None)),
                // Already in sight: as the original's walk to see that
                // does not move (0x41c595), only its end, turning to face
                // the thing within 15 degrees.
                None => return self.turn_then(renderer, rule, &target, FACE_TO_SEE),
            },
            "nearmodel" => self.standing_spot(&target).map(|p| (p, None)),
            "turntomodel" => return self.turn_then(renderer, rule, &target, FACE_TURN_TO_MODEL),
            // Into the first pose of the animation the action plays.
            "anim" => scene
                .cast
                .anim_start(self.lib, &target, &ego)
                .map(|(p, h)| (p, Some(h))),
            _ => None,
        };
        let from = self.ego_position().map(|(p, _)| p);
        if let Some((goal, heading)) = destination
            && self.walk_ego(
                goal,
                heading,
                match approach.as_str() {
                    "anim" => EgoWalk::IntoAnimation,
                    "walkto" => EgoWalk::ToSpot,
                    _ => EgoWalk::Approach,
                },
            )
        {
            let from = from.map_or(String::new(), |p| format!(" from {:.0},{:.0},{:.0}", p.x, p.y, p.z));
            self.log.push(format!(
                "  (walks{from} to {:.0},{:.0},{:.0} to {approach} {target})",
                goal.x, goal.y, goal.z
            ));
            if heading.is_none() && approach != "walkto" {
                // Face what was approached on arrival, as near as the
                // original's walk of that kind turns (0x418860).
                let within = if approach == "nearmodel" { FACE_NEAR_MODEL } else { FACE_TO_SEE };
                self.face_after = Some((target, within));
            }
            self.approach = Some(Approach { rule, actor: ego });
            self.topic_bar = None;
            return;
        }
        self.run(renderer, rule, true);
    }

    /// Where the player should walk to see something, or None if they see
    /// it from where they stand. Along the way to it, stopping one waypoint
    /// past where it first comes into view, so as to be out in the open.
    fn walk_to_see(&self, target: &str) -> Option<Vec3> {
        let scene = self.scene.as_ref()?;
        let (from, _) = scene.cast.actor_position(&scene.ego)?;
        let height = self
            .characters
            .get(&scene.ego)
            .map_or(70.0, |c| c.walker_height);
        let eye = |x: f32, z: f32| Vec3::new(x, from.y + height, z);
        if self.can_see(eye(from.x, from.z), target) {
            return None;
        }
        let center = self.target_bounds(target).map(|(lo, hi)| (lo + hi) * 0.5)?;
        let path = route(scene.walk.as_ref(), &scene.cast, &scene.ego, from, center, false)?;
        // Along the route a step at a time; ours are straightened, so a leg
        // can be long.
        const STEP: f32 = 8.0;
        let mut points = vec![[from.x, from.z]];
        points.extend(&path);
        let mut along = 0.0;
        let mut seen_at = None;
        'legs: for leg in points.windows(2) {
            let (a, b) = (Vec2::from(leg[0]), Vec2::from(leg[1]));
            let length = a.distance(b);
            let mut t = 0.0;
            while t < length {
                t = (t + STEP).min(length);
                let p = a.lerp(b, t / length.max(1e-3));
                if self.can_see(eye(p.x, p.y), target) {
                    seen_at = Some(along + t);
                    break 'legs;
                }
            }
            along += length;
        }
        // On a little past where it comes into view, so as to be out in the
        // open rather than peering round a corner.
        let total: f32 = points
            .windows(2)
            .map(|l| Vec2::from(l[0]).distance(Vec2::from(l[1])))
            .sum();
        let stop = seen_at.map_or(total, |s| (s + 30.0).min(total));
        let mut left = stop;
        for leg in points.windows(2) {
            let (a, b) = (Vec2::from(leg[0]), Vec2::from(leg[1]));
            let length = a.distance(b);
            if left <= length {
                let p = a.lerp(b, left / length.max(1e-3));
                return Some(Vec3::new(p.x, from.y, p.y));
            }
            left -= length;
        }
        let end = points.last()?;
        Some(Vec3::new(end[0], from.y, end[1]))
    }

    /// The box around a person, prop or piece of the room.
    fn target_bounds(&self, target: &str) -> Option<(Vec3, Vec3)> {
        let scene = self.scene.as_ref()?;
        scene
            .cast
            .bounds(target)
            .or_else(|| bsp_bounds(&scene.room.bsp, target))
    }

    /// Whether something can be seen from a point, as G-Engine's
    /// `IsWalkToSeeTargetInView`: near enough (200 units), and a line from
    /// the eye to the middle of one of its box's faces meets it, or nothing,
    /// on the way.
    fn can_see(&self, eye: Vec3, target: &str) -> bool {
        let Some(scene) = &self.scene else {
            return false;
        };
        let Some((lo, hi)) = self.target_bounds(target) else {
            return false;
        };
        let center = (lo + hi) * 0.5;
        if eye.distance(center) > 200.0 {
            return false;
        }
        let noun = |name: &str| {
            scene
                .nouns
                .get(&name.to_ascii_uppercase())
                .cloned()
                .or_else(|| {
                    let i = scene.cast.find(name)?;
                    scene.cast.noun_of(i)
                })
        };
        let target_noun = noun(target);
        let is_target = |name: &str, hit_noun: Option<String>| {
            name.eq_ignore_ascii_case(target)
                || (target_noun.is_some() && hit_noun.or_else(|| noun(name)) == target_noun)
        };
        let bsp = &scene.room.bsp;
        let faces = [
            Vec3::new(center.x, hi.y, center.z),
            Vec3::new(hi.x, center.y, center.z),
            Vec3::new(center.x, center.y, hi.z),
            Vec3::new(center.x, lo.y, center.z),
            Vec3::new(lo.x, center.y, center.z),
            Vec3::new(center.x, center.y, lo.z),
        ];
        faces.into_iter().any(|point| {
            let to = point - eye;
            let length = to.length();
            if length < 1e-3 {
                return true;
            }
            let direction = to / length;
            let mut nearest: Option<(f32, bool)> = scene
                .cast
                .ray(eye, direction, &scene.ego)
                .map(|(d, name, noun)| (d, is_target(&name, noun)));
            for polygon in &bsp.polygons {
                let surface = &bsp.surfaces[polygon.surface as usize];
                let object = &bsp.objects[surface.object as usize];
                if scene.room.hidden.contains(&object.to_ascii_uppercase())
                    && !object.eq_ignore_ascii_case(target)
                {
                    continue;
                }
                for t in bsp.triangles(polygon) {
                    let [a, b, c] = t.map(|i| Vec3::from(bsp.vertices[i as usize]));
                    if let Some(d) = ray_triangle(eye, direction, a, b, c)
                        && nearest.is_none_or(|(nd, _)| d < nd)
                    {
                        nearest = Some((d, is_target(object, None)));
                    }
                }
            }
            // Seen if the first thing met is the target, or nothing is met
            // short of the point.
            nearest.is_none_or(|(d, hit_target)| hit_target || d >= length - 0.5)
        })
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

    /// Turns the player on the spot to face something (the middle of its
    /// bounds), as the original's walks that end facing a model: no turn if
    /// already within `within`, else at turning speed to within a quarter
    /// of it. True if a turn has begun.
    fn face_within(&mut self, target: &str, within: f32) -> bool {
        let Some(scene) = &mut self.scene else { return false };
        let center = scene
            .cast
            .center(target)
            .or_else(|| bsp_center(&scene.room.bsp, target));
        let (Some(c), Some((p, _))) = (center, scene.cast.actor_position(&scene.ego)) else {
            return false;
        };
        let heading = (c.x - p.x).atan2(c.z - p.z);
        let ego = scene.ego.clone();
        scene.cast.turn_on_spot(&ego, heading, within, self.clock)
    }

    /// An action whose approach is a turn on the spot: it runs once the
    /// player has turned, or at once if no turn is needed.
    fn turn_then(&mut self, renderer: &Renderer, rule: Rule, target: &str, within: f32) {
        if self.face_within(target, within) {
            let ego = self.scene.as_ref().map(|s| s.ego.clone()).unwrap_or_default();
            self.log.push(format!("  (turns to face {target})"));
            self.approach = Some(Approach { rule, actor: ego });
            self.topic_bar = None;
            return;
        }
        self.run(renderer, rule, true);
    }

    /// Where the player stands and faces.
    pub fn ego_position(&self) -> Option<(Vec3, f32)> {
        let scene = self.scene.as_ref()?;
        scene.cast.actor_position(&scene.ego)
    }

    /// The player's triggers, as the original's walker checks them each
    /// frame, walking or not (0x41838e → 0x4194d7): the first trigger the
    /// player stands in whose noun the story has a WALK rule for now
    /// (0x525c61), if not the one found last time, is tripped; and is kept
    /// as the last, as is being in none. So a trigger trips on coming into
    /// it, and again only once it has been left. Tripped while an action
    /// plays its script, it does nothing (0x525a57, 0x4077b4); while the
    /// player is only walking to one, it takes that action's place.
    fn trip_triggers(&mut self, renderer: &Renderer) {
        let Some((at, _)) = self.ego_position() else { return };
        let found = self.trigger_at(at);
        let noun = found.as_ref().map(|r| r.noun.to_ascii_uppercase());
        if noun == self.last_trigger {
            return;
        }
        self.last_trigger = noun;
        let Some(rule) = found else { return };
        let playing = !self.actions.is_empty()
            || self.next_location.is_some()
            || self.timeblock_change.is_some()
            || self.binocs.as_ref().is_some_and(|b| !b.queue.is_empty());
        if playing {
            self.log.push(format!("trigger {} (while busy: nothing)", rule.noun));
            return;
        }
        self.log.push(format!("trigger {}", rule.noun));
        self.approach = None;
        self.face_after = None;
        self.act(renderer, rule);
    }

    /// The trigger the player would be found in at a point (0x525c61): the
    /// first holding it whose noun has a WALK rule now, and that rule.
    fn trigger_at(&self, at: Vec3) -> Option<Rule> {
        let scene = self.scene.as_ref()?;
        let inside: Vec<&str> = scene
            .triggers
            .iter()
            .filter(|t| (t.min.0..=t.max.0).contains(&at.x) && (t.min.1..=t.max.1).contains(&at.z))
            .map(|t| t.noun.as_str())
            .collect();
        if inside.is_empty() {
            return None;
        }
        let snapshot = self.state.clone();
        let whereabouts = self.whereabouts();
        inside.into_iter().find_map(|noun| {
            // WALK is not a verb the player is offered, so not among
            // `verbs`: the rules are asked for directly.
            let mut probe = snapshot.clone();
            let mut api = GameApi::new(&mut probe, &self.scripts);
            api.whereabouts = Some(&whereabouts);
            scene
                .actions
                .available(noun, &mut api, &snapshot)
                .into_iter()
                .find(|r| r.verb == "WALK")
                .cloned()
        })
    }

    /// Whether a point is in a trigger the player has not just tripped
    /// (0x419473).
    fn new_trigger_at(&self, at: Vec3) -> bool {
        self.trigger_at(at)
            .is_some_and(|r| Some(r.noun.to_ascii_uppercase()) != self.last_trigger)
    }

    /// Whether the player is on their way somewhere.
    pub fn ego_walking(&self) -> bool {
        self.scene
            .as_ref()
            .is_some_and(|s| s.cast.is_walking(&s.ego))
    }

    /// Walks the player towards a point. False if there is nowhere to go.
    /// Walks the ego there, as the original's walk of that kind.
    pub fn walk_ego(&mut self, goal: Vec3, heading: Option<f32>, kind: EgoWalk) -> bool {
        let Some(scene) = &mut self.scene else {
            return false;
        };
        let ego = scene.ego.clone();
        let Some((from, facing)) = scene.cast.actor_position(&ego) else {
            return false;
        };
        // Already there, as G-Engine's `Walker::AtPosition`: no route, only
        // a turn to the heading, if it is much off. Routing from where one
        // stands can go out and back, when that spot is closed off (such as
        // in front of an open wardrobe). A click this near is no walk at
        // all, as the original's (0x41d34a), and stops one under way.
        // A `Langolier` room takes a click on its floor for itself: the
        // player is told so, and does not walk (0x4c22bd).
        if kind == EgoWalk::Click && scene.custom.as_deref() == Some("LANGOLIER") {
            self.deferred
                .push("wait CallSheep(\"glb_all\", \"FloorClick$\");".into());
            return false;
        }
        let near = if kind == EgoWalk::Click { CLICK_NEAR } else { AT_SPOT };
        if Vec3::new(goal.x - from.x, 0.0, goal.z - from.z).length() <= near {
            if kind == EgoWalk::Click {
                scene.cast.stop_walk(&ego);
                return false;
            }
            let off = heading.map_or(0.0, |h| {
                let d = (h - facing).rem_euclid(std::f32::consts::TAU);
                d.min(std::f32::consts::TAU - d)
            });
            if off < 0.1 {
                return false;
            }
            let (lib, now) = (self.lib, self.clock);
            return scene.cast.walk(lib, &ego, Vec::new(), heading, now);
        }
        // A walk to a spot and facing is to a named place; a click on the
        // floor is only roughly where to go.
        let path = route(scene.walk.as_ref(), &scene.cast, &ego, from, goal, heading.is_some());
        match path {
            Some(path) if !path.is_empty() => {
                let path = self.skip_into_view(&ego, from, path, heading);
                let (lib, now) = (self.lib, self.clock);
                let tolerance = kind.tolerance();
                self.scene
                    .as_mut()
                    .is_some_and(|s| s.cast.walk_within(lib, &ego, path, heading, tolerance, now))
            }
            _ => false,
        }
    }

    /// The player need not wait for the ego to walk into shot: as G-Engine's
    /// `SkipPathNodesOutsideFrustum`, a walk starting off camera begins
    /// instead just before the route comes into view (or at its end, if
    /// none of it does). Returns what is left of the route.
    fn skip_into_view(
        &mut self,
        who: &str,
        from: Vec3,
        path: Vec<[f32; 2]>,
        heading: Option<f32>,
    ) -> Vec<[f32; 2]> {
        let view_proj = self.camera.view_proj(self.aspect);
        // From the floor to the top of the head, anywhere in the picture.
        let seen = |p: Vec3| {
            (0..=8).any(|k| {
                let c = view_proj * (p + Vec3::Y * (72.0 * k as f32 / 8.0)).extend(1.0);
                c.w > 0.0 && c.x.abs() <= c.w && c.y.abs() <= c.w && c.z >= 0.0 && c.z <= c.w
            })
        };
        if seen(from) {
            return path;
        }
        // The route every few units, with the leg each point is on.
        let mut points: Vec<(Vec3, usize)> = Vec::new();
        let mut at = from;
        for (leg, p) in path.iter().enumerate() {
            let to = Vec3::new(p[0], at.y, p[1]);
            let steps = ((to - at).length() / 10.0).ceil().max(1.0) as usize;
            for k in 1..=steps {
                points.push((at.lerp(to, k as f32 / steps as f32), leg));
            }
            at = to;
        }
        let first_seen = points.iter().position(|(p, _)| seen(*p));
        // As the original's skip (0x4e0e8f), the player's stops short at
        // a trigger not yet tripped, which then trips where they are put.
        let is_ego = self.scene.as_ref().is_some_and(|s| s.ego.eq_ignore_ascii_case(who));
        let first_trigger = if is_ego {
            points
                .iter()
                .take(first_seen.unwrap_or(points.len()))
                .position(|(p, _)| self.new_trigger_at(*p))
        } else {
            None
        };
        if let Some(k) = first_trigger {
            let (put, leg) = points[k];
            let before = if k == 0 { from } else { points[k - 1].0 };
            let d = put - before;
            self.log.push(format!(
                "  ({who} skips into a trigger at {:.0},{:.0})",
                put.x, put.z
            ));
            if let Some(scene) = &mut self.scene {
                scene.cast.put(who, put, d.x.atan2(d.z));
            }
            let mut rest = path[leg..].to_vec();
            if rest.is_empty() {
                rest.push([put.x, put.z]);
            }
            return rest;
        }
        let (put, facing, rest) = match first_seen {
            // Never in view: straight to the end, facing the way asked.
            None => {
                let end = points.last().map_or(from, |p| p.0);
                let before = points.len().checked_sub(2).map_or(from, |i| points[i].0);
                let d = end - before;
                (end, heading.unwrap_or(d.x.atan2(d.z)), Vec::new())
            }
            Some(0) => return path,
            Some(k) => {
                let (start, _) = points[k - 1];
                let (next, leg) = points[k];
                let d = next - start;
                let mut rest = vec![[next.x, next.z]];
                rest.extend_from_slice(&path[leg..]);
                (start, d.x.atan2(d.z), rest)
            }
        };
        self.log.push(format!(
            "  ({who} skips into view at {:.0},{:.0})",
            put.x, put.z
        ));
        if let Some(scene) = &mut self.scene {
            scene.cast.put(who, put, facing);
        }
        if rest.is_empty() {
            // Already there: just the turn at the end.
            return vec![[put.x, put.z]];
        }
        rest
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
            let whereabouts = self.whereabouts();
            let mut api = GameApi::new(&mut self.state, &self.scripts);
            api.whereabouts = Some(&whereabouts);
            let result = self.vm.start(&mut api, script, "ACTION", ENTRY);
            (result, std::mem::take(&mut api.effects))
        };
        match result {
            Ok(id) => {
                self.door_action = user && rule.noun.to_ascii_uppercase().contains("DOOR");
                self.actions.push((id, rule, user));
            }
            Err(e) => self.log.push(format!("  {e}")),
        }
        self.perform(renderer, effects);
        self.finish_actions();
    }

    /// Releases a waiting script, and performs whatever it does next.
    fn release(&mut self, renderer: &Renderer, thread: ThreadId) {
        let effects = {
            let whereabouts = self.whereabouts();
            let mut api = GameApi::new(&mut self.state, &self.scripts);
            api.whereabouts = Some(&whereabouts);
            self.vm.release(&mut api, thread);
            std::mem::take(&mut api.effects)
        };
        self.perform(renderer, effects);
    }

    fn perform(&mut self, renderer: &Renderer, effects: Vec<Effect>) {
        let mut queue: std::collections::VecDeque<Effect> = effects.into();
        // A script whose waits all end at once can loop without end; stop it
        // rather than hang the game.
        let mut budget = 20_000;
        while let Some(effect) = queue.pop_front() {
            budget -= 1;
            if budget == 0 {
                self.log
                    .push("  (a script ran away without waiting; stopped)".into());
                return;
            }
            let done_now = self.perform_one(renderer, &effect);
            if let Some(thread) = effect.thread {
                match done_now {
                    Some(until) => self.tasks.push(Task { thread, until }),
                    None => {
                        let more = {
                            let whereabouts = self.whereabouts();
                            let mut api = GameApi::new(&mut self.state, &self.scripts);
                            api.whereabouts = Some(&whereabouts);
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
                let Some(anm) = read_anm(lib, &e.string(0)) else {
                    self.log.push(format!("    (no animation {})", e.string(0)));
                    return None;
                };
                // Plenty move nobody: they paint faces, swap the view out of
                // a window, or play sounds. They still take their time.
                let moved = scene
                    .cast
                    .play_parsed(lib, &e.string(0), &anm, looping, now);
                for f in &anm.faces {
                    scene
                        .cast
                        .face(&f.noun, now + f.frame as f64 / 15.0, f.change.clone());
                }
                self.schedule(&anm, now);
                let seconds = moved.unwrap_or(anm.frames as f32 / 15.0);
                (!looping).then_some(Until::Time(now + seconds as f64))
            }
            "PLAYSOUNDTRACK" | "PLAYSOUNDTRACKTBS" => {
                self.play_soundtrack(&e.string(0), name == "PLAYSOUNDTRACKTBS");
                None
            }
            "STOPSOUNDTRACK" => {
                self.stop_soundtrack(Some(&e.string(0)));
                None
            }
            "STOPALLSOUNDTRACKS" => {
                self.stop_soundtrack(None);
                None
            }
            // A mood is a face held until cleared: `<code><mood>ON` puts it
            // on and `<code><mood>OFF` takes it off, as G-Engine's
            // `FaceController`. Both must exist.
            "SETMOOD" | "CLEARMOOD" => {
                let who = e.string(0).to_ascii_uppercase();
                let anim = if name == "SETMOOD" {
                    // Their own moods, or else those of the face they
                    // wear: Lady Howard's other model has her own, Mosely
                    // with his paper borrows Mosely's.
                    let mood = e.string(1).to_ascii_uppercase();
                    let (on, off) = scene
                        .cast
                        .face_codes(&who)
                        .into_iter()
                        .map(|code| (format!("{code}{mood}ON"), format!("{code}{mood}OFF")))
                        .find(|(_, off)| read_anm(lib, off).is_some())?;
                    self.moods.insert(who, off);
                    on
                } else {
                    self.moods.remove(&who)?
                };
                let anm = read_anm(lib, &anim)?;
                for f in &anm.faces {
                    scene
                        .cast
                        .face(&f.noun, now + f.frame as f64 / 15.0, f.change.clone());
                }
                None
            }
            // A "moment": a little scene of its own, several actors'
            // animations with its lines, sounds and camera cuts.
            "STARTMOM" => {
                let name = e.string(0);
                let anm = [format!("E{name}.MOM"), format!("{name}.MOM")]
                    .iter()
                    .find_map(|n| lib.read(n).ok())
                    .map(|b| Animation::parse(&b))?;
                let played = scene.cast.play_parsed(lib, &name, &anm, false, now);
                for f in &anm.faces {
                    scene
                        .cast
                        .face(&f.noun, now + f.frame as f64 / 15.0, f.change.clone());
                }
                self.schedule(&anm, now);
                let seconds = played.unwrap_or(0.0).max(anm.frames as f32 / 15.0);
                Some(Until::Time(now + seconds as f64))
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
                let (speech, _) = self.speak(&e.string(0), e.int(1).max(1));
                Some(Until::Speech(speech))
            }
            "CONTINUEDIALOGUE" | "CONTINUEDIALOGUENOFIDGETS" => {
                self.dialogue_fidgets = name == "CONTINUEDIALOGUE";
                let plate = self.last_plate.as_deref().map(next_plate)?;
                let (speech, _) = self.speak(&plate, e.int(0).max(1));
                Some(Until::Speech(speech))
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
                let Some(path) = route(scene.walk.as_ref(), &scene.cast, &actor, from, goal, true)
                else {
                    self.log.push(format!(
                        "    (no way for {actor} from {:.0},{:.0} to {:.0},{:.0})",
                        from.x, from.z, goal.x, goal.z
                    ));
                    return None;
                };
                let heading = Some(heading.to_radians());
                let path = if scene.cast.find(&actor) == scene.cast.find(&scene.ego) {
                    self.skip_into_view(&actor, from, path, heading)
                } else {
                    path
                };
                self.scene
                    .as_mut()?
                    .cast
                    .walk_within(lib, &actor, path, heading, Tolerance::TO_SPOT, now)
                    .then_some(Until::Walk(actor.clone()))
                    .or_else(|| {
                        self.log.push(format!("    ({actor} cannot walk)"));
                        None
                    })
            }
            "WALKTOANIMATION" => {
                let actor = e.string(0);
                let (goal, heading) = scene.cast.anim_start(lib, &e.string(1), &actor)?;
                let from = scene.cast.actor_position(&actor)?.0;
                let path = route(scene.walk.as_ref(), &scene.cast, &actor, from, goal, true)?;
                let path = if scene.cast.find(&actor) == scene.cast.find(&scene.ego) {
                    self.skip_into_view(&actor, from, path, Some(heading))
                } else {
                    path
                };
                self.scene
                    .as_mut()?
                    .cast
                    .walk_within(lib, &actor, path, Some(heading), Tolerance::INTO_ANIMATION, now)
                    .then_some(Until::Walk(actor))
            }
            "SETACTORPOSITION" => {
                scene.cast.place(&e.string(0), &e.string(1));
                None
            }
            // As the original's (0x4a8b08): the player stands on the
            // position, and if it names a camera, the view cuts to it
            // whatever the forced cuts say (0x4ab6b1).
            "INITEGOPOSITION" => {
                let ego = scene.ego.clone();
                let position = e.string(0).to_ascii_uppercase();
                scene.cast.init_position(lib, &ego, &position);
                if let Some(c) = scene
                    .cast
                    .position_cameras
                    .get(&position)
                    .and_then(|camera| scene.cameras.get(camera))
                {
                    self.camera = shoot::from_room_camera(c);
                }
                None
            }
            "SHOWMODEL" | "HIDEMODEL" | "SHOWSCENEMODEL" | "HIDESCENEMODEL" => {
                self.show_model(renderer, &e.string(0), name.starts_with("SHOW"));
                None
            }
            "SETSCENE" | "SETSCENENOPRELOADTEXTURES" => {
                self.set_scene(renderer, &e.string(0));
                None
            }
            // Lightmaps go up with the scene they belong to.
            "UPLOADSCENELIGHTMAPS" => None,
            "INVENTORYINSPECT" => {
                self.state.inventory_shown = true;
                self.inspect = Some(Some(e.string(0).to_ascii_uppercase()));
                None
            }
            "INVENTORYUNINSPECT" => {
                self.inspect = Some(None);
                None
            }
            "SETMODELLIGHTING" => {
                let c = |i: usize| e.int(i) as f32;
                scene.cast.set_fixed_light(&e.string(0), e.float(1), [c(2), c(3), c(4)]);
                None
            }
            "ENABLEMODELSHADOW" | "DISABLEMODELSHADOW" => {
                scene
                    .cast
                    .set_shadow(&e.string(0), name == "ENABLEMODELSHADOW");
                None
            }
            // The engine's own code for a few rooms' puzzles.
            "CALLSCENEFUNCTION" => {
                let function = e.string(0);
                match self.scene_function(renderer, &function) {
                    Some(until) => until,
                    None => {
                        self.log.push(format!("    (no scene function {function})"));
                        None
                    }
                }
            }
            // Eyes: glances, following something, jitter. Heads turn only
            // where a character has head-turn animations (the Abbé), as in
            // the original; everyone else's eyes do the looking.
            "GLANCE" | "TURNHEAD" => {
                let seconds = e.int(3).max(0) as f64 / 1000.0;
                let (who, percent) = (e.string(0), (e.int(1), e.int(2)));
                if name == "GLANCE" {
                    scene.cast.glance(&who, percent, seconds, now);
                } else {
                    scene.cast.turn_head(&who, percent, seconds, now);
                }
                // The time is how long the look holds, often for good
                // (1000000 ms); the script goes on at once.
                None
            }
            "LOOKITACTOR" | "LOOKITACTORQUICK" | "LOOKITMODEL" | "LOOKITMODELQUICK"
            | "LOOKITNOUN" | "LOOKITNOUNQUICK" => {
                let target = crate::cast::LookTarget::Noun(e.string(1));
                look(
                    &mut scene.cast,
                    &e.string(0),
                    target,
                    &e.string(2),
                    e.float(3),
                    &name,
                    now,
                );
                None
            }
            "LOOKITMODELX" | "LOOKITMODELQUICKX" => {
                // Model, mesh, box modifier, then an offset from it.
                let at = scene.cast.center(&e.string(1)).unwrap_or(Vec3::ZERO)
                    + Vec3::new(e.float(4), e.float(5), e.float(6));
                let target = crate::cast::LookTarget::Point(at);
                look(
                    &mut scene.cast,
                    &e.string(0),
                    target,
                    &e.string(7),
                    e.float(8),
                    &name,
                    now,
                );
                None
            }
            "LOOKITPOINT" => {
                let at = Vec3::new(e.float(1), e.float(2), e.float(3));
                let target = crate::cast::LookTarget::Point(at);
                look(
                    &mut scene.cast,
                    &e.string(0),
                    target,
                    &e.string(4),
                    e.float(5),
                    &name,
                    now,
                );
                None
            }
            "LOOKITPLAYER" => {
                let target = crate::cast::LookTarget::Noun(scene.ego.clone());
                look(
                    &mut scene.cast,
                    &e.string(0),
                    target,
                    &e.string(1),
                    e.float(2),
                    &name,
                    now,
                );
                None
            }
            "LOOKITSCENEMODEL" | "LOOKITSCENEMODELQUICK" => {
                if let Some(at) = object_center(&scene.room.bsp, &e.string(1)) {
                    let target = crate::cast::LookTarget::Point(at);
                    look(
                        &mut scene.cast,
                        &e.string(0),
                        target,
                        &e.string(2),
                        e.float(3),
                        &name,
                        now,
                    );
                }
                None
            }
            "LOOKITCANCEL" => {
                scene.cast.look_cancel(&e.string(0), now);
                None
            }
            "ENABLEEYEJITTER" | "DISABLEEYEJITTER" | "EYEJITTER" => {
                let on = match name.as_str() {
                    "ENABLEEYEJITTER" => Some(true),
                    "DISABLEEYEJITTER" => Some(false),
                    _ => None,
                };
                scene.cast.eye_jitter(&e.string(0), on, now);
                None
            }
            // Following the mouse, and locking a look, are for the player's
            // own pointer; nothing to do here.
            "LOOKITMOUSE" | "LOOKITLOCK" | "LOOKITUNLOCK" => None,
            "ENABLEHITTESTMODEL" | "DISABLEHITTESTMODEL" => {
                let object = e.string(0).to_ascii_uppercase();
                if name == "DISABLEHITTESTMODEL" {
                    scene.untouchable.insert(object);
                } else {
                    scene.untouchable.remove(&object);
                }
                None
            }
            // Gets someone out of the way, as G-Engine's `WalkOutOfRegion`:
            // if they stand in the region, they walk towards the exit, and
            // the script goes on once they are out of it.
            "ACTIONWAITCLEARREGION" | "CLEARREGION" => {
                let actor = e.string(0);
                let region = e.int(1) as u8;
                let &(goal, heading) = scene
                    .cast
                    .positions
                    .get(&e.string(3).to_ascii_uppercase())?;
                let (from, _) = scene.cast.actor_position(&actor)?;
                let walk = scene.walk.as_ref()?;
                if walk.region_at(from.x, from.z) != region {
                    return None;
                }
                let path = route(Some(walk), &scene.cast, &actor, from, goal, true)?;
                scene
                    .cast
                    .walk_within(lib, &actor, path, Some(heading.to_radians()), Tolerance::TO_SPOT, now)
                    .then_some(Until::LeftRegion(actor, region))
            }
            // Nobody walks through the model: the floor under it is closed
            // off, as G-Engine's `SetUnwalkableRect`.
            "WALKERBOUNDARYBLOCKMODEL" | "WALKERBOUNDARYUNBLOCKMODEL" => {
                let model = e.string(0);
                let rect = if name == "WALKERBOUNDARYBLOCKMODEL" {
                    Some(scene.cast.footprint(&model)?)
                } else {
                    None
                };
                if let Some(walk) = &mut scene.walk {
                    walk.set_rect(&model, rect);
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
                self.scheduled.push((now, e.string(0), 1.0, Bus::Effects, None));
                None
            }
            // The scripts' cutscenes. The `X` form's number is G-Engine's
            // autoclose: at 0 the last frame stays until the player goes on.
            "PLAYFULLSCREENMOVIE" | "PLAYFULLSCREENMOVIEX" | "PLAYMOVIE" => {
                let showing = self.play_movie(&e.string(0), AfterMovie::Nothing);
                if name == "PLAYFULLSCREENMOVIEX"
                    && e.int(1) == 0
                    && let Some(show) = &mut self.showing
                {
                    show.keep_last = true;
                }
                showing.then_some(Until::Movie)
            }
            "SETLOCATION" => {
                self.next_location = Some(e.string(0));
                self.door_exit = self.door_action && !self.actions.is_empty();
                None
            }
            "SHOWFINGERPRINTINTERFACE" => {
                let noun = e.string(0);
                let flags = self.state.flags.clone();
                let has = move |f: &str| flags.contains(&gk3_game::state::key(f));
                self.fingerprint =
                    crate::fingerprint::Kit::open(&noun, &self.state.timeblock, &has);
                self.fingerprint_close = false;
                if self.fingerprint.is_none() {
                    self.log
                        .push(format!("  no fingerprints to take from {noun}"));
                }
                None
            }
            "SHOWSIDNEY" => {
                self.open_sidney();
                None
            }
            "INSPECTOBJECT" | "DEFAULTINSPECT" | "INSPECTMODELUSINGANGLE" => {
                let noun = if name == "INSPECTOBJECT" {
                    self.active_object.clone()?
                } else {
                    e.string(0)
                };
                let camera = (name == "INSPECTMODELUSINGANGLE")
                    .then(|| scene.cameras.get(&e.string(1).to_ascii_uppercase()))
                    .flatten()
                    .map(shoot::from_room_camera);
                self.inspect(&noun, camera);
                None
            }
            "UNINSPECT" => {
                self.uninspect();
                None
            }
            // Which camera dialogue cuts to by default: G-Engine leaves it
            // alone, and every conversation names its own cameras.
            "SETDEFAULTDIALOGUECAMERA" | "CLEARDEFAULTDIALOGUECAMERA" => None,
            // Loading ahead of need: everything here loads when used.
            "PRELOADSHEEP" => None,
            "SETWALKANIM" => {
                let (start, cycle) = (e.string(1), e.string(2));
                if !start.trim().is_empty() && !cycle.trim().is_empty() {
                    let turns = [e.string(3), e.string(4)];
                    let turns = turns.map(|t| (!t.trim().is_empty()).then_some(t));
                    scene.cast.set_walk_anims(&e.string(0), &start, &cycle, turns);
                }
                None
            }
            // A morph between two animations does nothing in G-Engine.
            "STARTMORPHANIMATION" => {
                let lib = self.lib;
                let seconds = scene.cast.morph(
                    lib,
                    &e.string(0),
                    e.int(1).max(0) as u32,
                    e.int(2).max(0) as u32,
                    now,
                );
                Some(Until::Time(now + seconds as f64))
            }
            "STOPMORPHANIMATION" => None,
            // Which kind a camera angle is: a "Room" angle is one the player
            // may choose from the option bar; any other kind ("Cinematic")
            // is not (0x4ab8bf sets the angle's kind).
            "SETCAMERAANGLETYPE" => {
                let (name, kind) = (e.string(0), e.string(1));
                if let Some(c) = scene
                    .room
                    .cameras
                    .iter_mut()
                    .find(|c| c.name.eq_ignore_ascii_case(name.trim()))
                {
                    c.room = kind.trim().eq_ignore_ascii_case("Room");
                }
                None
            }
            "SETVERBMODAL" => {
                self.verb_modal = e.int(0) != 0;
                None
            }
            "STARTVERBCANCEL" | "STOPVERBCANCEL" => {
                self.verb_cancel = name == "STARTVERBCANCEL";
                None
            }
            "SETGAMETIMER" => {
                let (noun, verb) = (e.string(0), e.string(1));
                self.game_timers
                    .push((e.int(2).max(0) as f64 / 1000.0, noun.to_ascii_uppercase(), verb.to_ascii_uppercase()));
                None
            }
            "SETCAMERAFOV" => {
                self.camera.fov_y = e.float(0).clamp(1.0, 180.0).to_radians();
                None
            }
            "SETFORCEDCAMERACUTS" | "CLEARFORCEDCAMERACUTS" => {
                self.forced_cuts = name == "SETFORCEDCAMERACUTS" && e.int(0) != 0;
                None
            }
            "SHOWDEATHLAYER" | "FINISHEDSCREEN" | "QUITAPP" => {
                let screen = match name.as_str() {
                    "SHOWDEATHLAYER" => ScreenRequest::Death,
                    "FINISHEDSCREEN" => ScreenRequest::Finished,
                    _ => ScreenRequest::Quit,
                };
                self.log.push(format!("  ({screen:?} screen)"));
                self.screen = Some(screen);
                None
            }
            "SHOWBINOCS" => {
                self.open_binocs();
                None
            }
            "SHOWDRIVINGINTERFACE" | "FOLLOWONDRIVINGMAP" => {
                self.map_follow = if name == "FOLLOWONDRIVINGMAP" {
                    crate::map::Follow::from_number(e.int(0))
                } else {
                    crate::map::Follow::None
                };
                self.next_location = Some("MAP".into());
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
            // A prop's fidget is its GAS script, by model name or noun.
            "STARTPROPFIDGET" => {
                scene.cast.start_fidget(lib, &e.string(0), Fidget::Idle);
                None
            }
            "STOPFIDGET" | "STOPPROPFIDGET" => {
                let seconds = scene.cast.stop_fidget(lib, &e.string(0), now);
                (seconds > 0.0).then_some(Until::Time(now + seconds))
            }
            // Everything else is over as soon as it starts, for now.
            _ => {
                self.log.push(format!("    (not done: {})", e.name));
                None
            }
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
            // The fidget takes over once the way in has played.
            if let Some(anim) = &l.enter
                && let Some(seconds) = scene.cast.play(lib, anim, false, now)
            {
                longest = longest.max(seconds as f64);
            }
            scene.cast.start_fidget(lib, &l.actor, kind);
            self.log
                .push(format!("    ({} {kind:?}s, for {name})", l.actor));
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
                && let Some(seconds) = scene.cast.play(lib, anim, false, now)
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
    /// Shows or hides a model: one of the cast, or else a part of the room.
    fn show_model(&mut self, renderer: &Renderer, model: &str, show: bool) {
        let Some(scene) = &mut self.scene else { return };
        if !scene.cast.set_visible(model, show) {
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
    }

    /// Swaps the room's scene asset for another, keeping all else.
    fn set_scene(&mut self, renderer: &Renderer, name: &str) {
        let Some(scene) = &mut self.scene else { return };
        if scene.room.name.eq_ignore_ascii_case(name) {
            return;
        }
        match scene.room.set_scene(self.lib, name) {
            Ok(()) => {
                scene.room.rebuild();
                scene.gpu = renderer.upload(&scene.room.mesh);
            }
            Err(err) => self.log.push(format!("    (no scene {name}: {err})")),
        }
    }

    /// Starts a soundtrack by name, unless it is already playing.
    pub fn play_soundtrack(&mut self, name: &str, once: bool) {
        let stem = name.trim_end_matches(".STK").trim_end_matches(".stk");
        if self
            .soundtracks
            .iter()
            .any(|t| t.name.eq_ignore_ascii_case(stem))
        {
            return;
        }
        match self.lib.read(&format!("{stem}.STK")) {
            Ok(bytes) => {
                let mut track = Soundtrack::parse(stem, &bytes);
                track.once = once;
                self.soundtracks.push(track);
            }
            Err(_) => self.log.push(format!("    (no soundtrack {stem})")),
        }
    }

    /// Stops a soundtrack and what it is playing; all of them without a name.
    pub fn stop_soundtrack(&mut self, name: Option<&str>) {
        let name = name.map(|n| {
            n.trim_end_matches(".STK")
                .trim_end_matches(".stk")
                .to_ascii_uppercase()
        });
        let matches = |track: &str| name.as_deref().is_none_or(|n| n == track);
        self.soundtracks.retain(|t| !matches(&t.name));
        let audio = &mut self.audio;
        self.soundtrack_sounds.retain(|(track, id)| {
            let stop = matches(track);
            if stop {
                audio.stop(*id);
            }
            !stop
        });
    }

    /// A foot comes down: the sound for the walker's shoes on the floor
    /// texture under them, quieter the farther from the camera, as
    /// G-Engine's `PlayFootSound` (heard from 60 units, gone by 800).
    fn footstep(&mut self, who: &str, scuff: bool) {
        let Some(scene) = &self.scene else { return };
        let Some((at, _)) = scene.cast.actor_position(who) else {
            return;
        };
        let shoe = scene
            .cast
            .shoe_type(who)
            .unwrap_or_else(|| "Male Leather".into());
        let texture = self.floor_texture(at).unwrap_or_else(|| "CARPET1".into());
        let sounds = self.footsteps.sounds(&shoe, &texture, scuff);
        if sounds.is_empty() {
            return;
        }
        let pick = (self.clock * 1000.0) as usize % sounds.len();
        let name = sounds[pick].clone();
        self.sound_at(&name, 1.0, Bus::Effects, at, FOOTSTEP_NEAR, FOOTSTEP_FAR);
    }

    /// The texture of the floor under a point: the room's floor object's
    /// surface nearest below (or just above) it.
    fn floor_texture(&self, at: Vec3) -> Option<String> {
        let scene = self.scene.as_ref()?;
        let floor = scene.floor.as_deref()?;
        let bsp = &scene.room.bsp;
        let origin = at + Vec3::Y * 50.0;
        let mut best: Option<(f32, String)> = None;
        for polygon in &bsp.polygons {
            let surface = &bsp.surfaces[polygon.surface as usize];
            if !bsp.objects[surface.object as usize].eq_ignore_ascii_case(floor) {
                continue;
            }
            for t in bsp.triangles(polygon) {
                let [a, b, c] = t.map(|i| Vec3::from(bsp.vertices[i as usize]));
                let hit = ray_triangle(origin, Vec3::NEG_Y, a, b, c)
                    .or_else(|| ray_triangle(origin, Vec3::NEG_Y, a, c, b));
                if let Some(d) = hit
                    && best
                        .as_ref()
                        .is_none_or(|(bd, _)| (d - 50.0).abs() < (bd - 50.0).abs())
                {
                    best = Some((d, surface.texture.clone()));
                }
            }
        }
        best.map(|(_, t)| t)
    }

    /// An animation's sounds and cues, from `start`.
    fn schedule(&mut self, anm: &Animation, start: f64) {
        for sound in &anm.sounds {
            self.scheduled.push((
                start + sound.frame as f64 / 15.0,
                sound.name.clone(),
                sound.volume as f32 / 100.0,
                Bus::Effects,
                sound.place.clone(),
            ));
        }
        for cue in &anm.cues {
            self.cues
                .push((start + cue.frame as f64 / 15.0, cue.what.clone()));
        }
    }

    /// Cues that are due: a line is a dialogue of one line, without
    /// fidgets, as G-Engine's `DialogueAnimNode`; a camera cuts.
    fn run_cues(&mut self, renderer: &Renderer, now: f64) {
        let (due, later): (Vec<_>, Vec<_>) = std::mem::take(&mut self.cues)
            .into_iter()
            .partition(|c| c.0 <= now);
        self.cues = later;
        for (_, cue) in due {
            match cue {
                CueKind::Dialogue(plate) => {
                    self.log.push(format!("  (cue) dialogue {plate}"));
                    self.dialogue_fidgets = false;
                    self.speak(&plate, 1);
                }
                CueKind::Footstep { noun, scuff } => self.footstep(&noun, scuff),
                CueKind::PartVisible {
                    model,
                    mesh,
                    group,
                    visible,
                } => {
                    if let Some(scene) = &mut self.scene {
                        scene.cast.set_part_visible(&model, mesh, group, visible);
                    }
                }
                CueKind::Visible { model, visible } => {
                    self.show_model(renderer, &model, visible);
                }
                CueKind::PlaySoundtrack { name, once } => self.play_soundtrack(&name, once),
                CueKind::StopSoundtrack(name) => self.stop_soundtrack(name.as_deref()),
                CueKind::ModelTexture {
                    model,
                    mesh,
                    group,
                    texture,
                } => {
                    if let Some(scene) = &mut self.scene {
                        scene
                            .cast
                            .retexture(self.lib, &model, mesh, group, &texture);
                    }
                }
                CueKind::SceneTexture { model, texture } => {
                    if let Some(scene) = &mut self.scene
                        && scene.room.retexture(self.lib, &model, &texture)
                    {
                        scene.room.rebuild();
                        scene.gpu = renderer.upload(&scene.room.mesh);
                    }
                }
                CueKind::Camera { name, .. } => {
                    self.log.push(format!("  (cue) camera {name}"));
                    if let Some(c) = self.scene.as_ref().and_then(|s| s.cameras.get(&name)) {
                        self.camera = shoot::from_room_camera(c);
                    }
                }
            }
        }
    }

    /// Says lines, starting with `plate` and going on through the next
    /// `count - 1`, one after another, as the original's dialogue request
    /// plays them (0x478218, 0x4782f6): each line in turn, the next when
    /// one ends (at its cue) or is skipped. Returns the speech, to wait on,
    /// and how long its lines take if none is skipped.
    fn speak(&mut self, plate: &str, count: i32) -> (u64, f64) {
        let mut plate = plate.to_ascii_uppercase();
        // A few scripts leave off the plate's sequence number (`2F9DB44PF`
        // for `2F9DB44PF1`): start from the first line.
        let exists = |p: &str| {
            self.lib.read(&format!("E{p}.YAK")).is_ok()
                || self.lib.read(&format!("{p}.YAK")).is_ok()
        };
        if !exists(&plate) && exists(&format!("{plate}1")) {
            plate.push('1');
        }
        let mut plates = VecDeque::new();
        for i in 0..count.max(1) {
            if i > 0 {
                plate = next_plate(&plate);
            }
            plates.push_back(plate.clone());
        }
        let total = plates.iter().map(|p| self.line_length(p)).sum::<f64>();
        // `ContinueDialogue` goes on from the last of them.
        self.last_plate = Some(plate);
        self.next_speech += 1;
        let id = self.next_speech;
        self.speeches.push(Speech {
            id,
            plates,
            line_end: self.clock,
        });
        self.advance_speeches();
        (id, total)
    }

    /// How long a line takes: to its cue, or its end.
    fn line_length(&mut self, plate: &str) -> f64 {
        let lib = self.lib;
        [format!("E{plate}"), plate.to_string()]
            .iter()
            .find_map(|name| {
                self.yaks
                    .entry(name.to_ascii_uppercase())
                    .or_insert_with(|| lib.read(&format!("{name}.YAK")).ok().map(|b| Animation::parse(&b)))
                    .as_ref()
                    .map(|anm| anm.dialogue_cue.filter(|&c| c > 0).unwrap_or(anm.frames) as f64 / 15.0)
            })
            .unwrap_or(0.0)
    }

    /// Starts each speech's next line once the one before has ended, and
    /// lets speeches with no lines left end.
    fn advance_speeches(&mut self) {
        let now = self.clock;
        let mut i = 0;
        while i < self.speeches.len() {
            if self.speeches[i].line_end > now {
                i += 1;
                continue;
            }
            match self.speeches[i].plates.pop_front() {
                Some(plate) => {
                    let seconds = self.say_line(&plate);
                    self.speeches[i].line_end = now + seconds;
                }
                None => {
                    self.speeches.remove(i);
                }
            }
        }
    }

    /// One line, now: its sound, lip-sync and captions, and who speaks.
    fn say_line(&mut self, plate: &str) -> f64 {
        let at = self.clock;
        // Spoken lines are localised: English adds an E.
        let line = self
            .yak_at(&format!("E{plate}"), at)
            .or_else(|| self.yak_at(plate, at));
        let Some((seconds, captions)) = line else {
            self.log.push(format!("    (no line {plate})"));
            return 0.0;
        };
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
        seconds as f64
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
        // As G-Engine: the line is over at its cue, though its sound may
        // run on a moment.
        let length = anm.dialogue_cue.filter(|&c| c > 0).unwrap_or(anm.frames);
        let result = (length as f32 / 15.0, anm.captions.clone());
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
                    s.place.clone(),
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
    pub fn sound(&mut self, name: &str, gain: f32, bus: Bus) -> Option<gk3_audio::SoundId> {
        // A door the story opens itself needs no sound of ours; a lock or a
        // knock is not the door opening.
        let upper = name.to_ascii_uppercase();
        if upper.contains("DOOR") && !upper.contains("LOCK") && !upper.contains("KNOCK") {
            self.door_heard = true;
        }
        match self.pcm(name) {
            Some(pcm) => Some(self.audio.play(pcm, gain, false, bus)),
            None => {
                self.log.push(format!("    (no sound {name})"));
                None
            }
        }
    }

    /// The middle of a model (or an object of the room) as it now stands.
    fn model_center(&self, model: &str) -> Option<Vec3> {
        let scene = self.scene.as_ref()?;
        scene.cast.center(model).or_else(|| bsp_center(&scene.room.bsp, model))
    }

    /// A sound an animation places in the room (`SoundPlace`): heard from
    /// the middle of its model as the model now stands, or from its own
    /// position, as the original places it as it starts (0x54c2b0).
    fn sound_from(&mut self, name: &str, gain: f32, bus: Bus, place: &SoundPlace) {
        let base = place.model.as_deref().and_then(|m| self.model_center(m)).unwrap_or(Vec3::ZERO);
        let at = base + Vec3::from(place.offset);
        self.sound_at(name, gain, bus, at, place.near, place.far);
    }

    /// A sound heard from a point in the room, as DirectSound plays the
    /// original's 3D sounds: fading and panned as the camera hears it, and
    /// followed as the camera moves (`place_sounds`).
    fn sound_at(&mut self, name: &str, gain: f32, bus: Bus, at: Vec3, near: f32, far: f32) {
        let upper = name.to_ascii_uppercase();
        if upper.contains("DOOR") && !upper.contains("LOCK") && !upper.contains("KNOCK") {
            self.door_heard = true;
        }
        let Some(pcm) = self.pcm(name) else {
            self.log.push(format!("    (no sound {name})"));
            return;
        };
        let (level, pan) = heard(&self.camera, at, near, far);
        let sound = self.audio.play_placed(pcm, gain * level, pan, false, bus);
        self.placed.push(Placed {
            sound,
            at,
            follow: None,
            near,
            far,
            gain,
        });
    }

    /// Each sound placed in the room, heard again from where the camera now
    /// is, as the original's listener follows its camera (0x44b55a).
    fn place_sounds(&mut self) {
        let moved: Vec<Option<Vec3>> = self
            .placed
            .iter()
            .map(|p| p.follow.as_deref().and_then(|m| self.model_center(m)))
            .collect();
        for (p, at) in self.placed.iter_mut().zip(moved) {
            if let Some(at) = at {
                p.at = at;
            }
        }
        let (audio, camera) = (&self.audio, self.camera);
        self.placed.retain(|p| {
            if !audio.is_playing(p.sound) {
                return false;
            }
            let (level, pan) = heard(&camera, p.at, p.near, p.far);
            audio.place(p.sound, p.gain * level, pan);
            true
        });
    }

    /// Hurries through what is being said: the current lines end now.
    pub fn skip(&mut self) {
        if self.captions.is_empty() {
            return;
        }
        // As the original's `SkipDialogueLine`: the line being said ends
        // now (its voice, captions, lip-sync and who is to speak next in
        // it), and its speech goes on to the next line. Nothing else is
        // hurried: animations and the story's other waits keep their time.
        let now = self.clock;
        let until = self
            .speeches
            .iter()
            .map(|s| s.line_end)
            .fold(now, f64::max);
        for speech in &mut self.speeches {
            speech.line_end = speech.line_end.min(now);
        }
        self.captions.clear();
        self.scheduled.retain(|s| s.3 != Bus::Dialogue);
        self.audio.stop_bus(Bus::Dialogue);
        self.speaker_changes.retain(|c| c.0 <= now);
        if let Some(scene) = &mut self.scene {
            scene.cast.cancel_faces(now, until);
        }
        self.advance_speeches();
    }

    /// Moves the world on to `now`.
    pub fn update(&mut self, renderer: &Renderer, now: f64) {
        renderer.tidy();
        // The story stands still under a full-screen movie: its clock is
        // the caller's less the time movies have taken.
        let outside = now;
        if self.held {
            let dt = (outside - self.outside_clock).max(0.0);
            self.paused += dt;
            self.held_for += dt;
            self.outside_clock = outside;
            return;
        }
        if self.showing.as_ref().is_some_and(|s| !s.ended) {
            self.paused += (outside - self.outside_clock).max(0.0);
            self.outside_clock = outside;
            self.run_movie(renderer, outside - self.held_for);
            return;
        }
        self.outside_clock = outside;
        let now = outside - self.paused;
        let dt = (now - self.clock).max(0.0);
        self.clock = now;
        self.run_game_timers(renderer, dt);
        if let Some(heads) = &mut self.laser_heads {
            heads.tick(dt as f32);
            self.place_laser_heads();
        }
        if let Some(mut pendulum) = self.pendulum.take() {
            pendulum.tick(dt as f32, self);
            self.pendulum = Some(pendulum);
        }
        if let Some(mut bridge) = self.bridge.take() {
            // After a fall, he may try again once the scripts are done.
            if bridge.is_dying() && self.actions.is_empty() && self.deferred.is_empty() {
                bridge.recovered();
            }
            bridge.tick(dt as f32, self);
            self.bridge = Some(bridge);
        }
        let due: Vec<(f64, String, bool)> = {
            let clock = self.clock;
            let (due, later) = std::mem::take(&mut self.timed_shows)
                .into_iter()
                .partition(|(at, _, _)| *at <= clock);
            self.timed_shows = later;
            due
        };
        for (_, model, show) in due {
            self.show_model(renderer, &model, show);
            if !show {
                self.sound("TE1TRAPDOOROPEN", 1.0, Bus::Effects);
            }
        }
        if let Some(map) = &mut self.map {
            for event in map.tick(dt) {
                match event {
                    crate::map::Event::Say(plate, lines) => {
                        let (_, seconds) = self.speak(&plate, lines);
                        if let Some((at, _)) = &mut self.map_leave {
                            *at = at.max(now + seconds);
                        }
                    }
                    crate::map::Event::ActorAt(actor, location) => {
                        self.state.actor_locations.insert(actor, location);
                    }
                    crate::map::Event::Leave(location) => {
                        let after = self.captions.iter().map(|c| c.until).fold(now, f64::max);
                        self.map_leave = Some((after, location));
                    }
                }
            }
        }
        if let Some(sidney) = &mut self.sidney {
            let busy = !self.actions.is_empty() || !self.captions.is_empty();
            let events = sidney.tick(dt, self.lib, &mut self.state, busy);
            self.sidney_events(renderer, events);
        }
        // The kit closes once its last words are said.
        if self.fingerprint_close && self.captions.is_empty() && self.fingerprint_label.is_none() {
            self.fingerprint = None;
            self.fingerprint_close = false;
        }
        // The binoculars' scripts run one after another.
        if self.actions.is_empty()
            && let Some(b) = &mut self.binocs
            && !b.queue.is_empty()
        {
            let function = b.queue.remove(0);
            if self.call(renderer, "BINOCS", &function) {
                self.log.push(format!("  BINOCS:{function}"));
            }
        }
        if let Some((at, _)) = &self.map_leave
            && now >= *at
            && let Some((_, location)) = self.map_leave.take()
        {
            self.map = None;
            self.change_location(renderer, &location);
        }
        self.keep_walkers_apart(now);
        if self.gps.is_some() {
            let on = self.state.flags.contains(&gk3_game::state::key("GPSOn"));
            let at = self.ego_position().map(|(p, _)| (p.x, p.z));
            let effects = self.gps.as_mut().map(|g| g.update(now, on, at)).unwrap_or_default();
            self.gps_effects(effects);
        }
        self.trip_triggers(renderer);
        if let Some(scene) = &mut self.scene {
            scene.cast.update(self.lib, now);
            scene.cast.upload(renderer);
            scene.cast.cull_around(self.camera.position);
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
        for (_, name, gain, bus, place) in due {
            match place {
                Some(place) => self.sound_from(&name, gain, bus, &place),
                None => {
                    self.sound(&name, gain, bus);
                }
            }
        }
        let steps = self
            .scene
            .as_mut()
            .map(|s| std::mem::take(&mut s.cast.footsteps))
            .unwrap_or_default();
        for (who, scuff) in steps {
            self.footstep(&who, scuff);
        }
        let gave_up = self
            .scene
            .as_mut()
            .map(|s| std::mem::take(&mut s.cast.gave_up))
            .unwrap_or_default();
        for (who, short) in gave_up {
            self.log.push(format!(
                "  ({who} takes too long to get there, {short}, and is put at the end)"
            ));
        }
        if let Some(scene) = &mut self.scene {
            self.cues.append(&mut scene.cast.cues);
            for (who, location) in scene.cast.moved.drain(..) {
                self.log.push(format!("    ({who} is now at {location})"));
                self.state
                    .actor_locations
                    .insert(gk3_game::state::character(&who), location);
            }
            // Walks fidget scripts asked for: find the way, as for the story.
            for (who, goal) in std::mem::take(&mut scene.cast.walk_requests) {
                let from = scene.cast.actor_position(&who).map(|(p, _)| p);
                let goal = match goal {
                    crate::cast::WalkGoal::Named(choices) => choices.iter().find_map(|c| {
                        let &(p, h) = scene.cast.positions.get(c)?;
                        // Not one they are already at, as G-Engine's `CHOOSEWALK`.
                        let here = from
                            .is_some_and(|f| Vec3::new(f.x - p.x, 0.0, f.z - p.z).length() < 12.25);
                        (!here || choices.len() == 1).then_some((p, Some(h.to_radians())))
                    }),
                    crate::cast::WalkGoal::Point(p) => Some((p, None)),
                };
                let path = match (from, goal) {
                    (Some(from), Some((to, heading))) => route(
                        scene.walk.as_ref(),
                        &scene.cast,
                        &who,
                        from,
                        to,
                        heading.is_some(),
                    ),
                    _ => None,
                };
                let heading = goal.and_then(|g| g.1);
                let path = path.unwrap_or_default();
                if let Some((to, _)) = goal {
                    self.log
                        .push(format!("    ({who} wanders to {:.0},{:.0})", to.x, to.z));
                }
                scene
                    .cast
                    .walk_for_fidget(self.lib, &who, path, heading, now);
            }
        }
        self.run_cues(renderer, now);
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
                let Some(pcm) = self.pcm(&clip.name) else { continue };
                let id = match &clip.place {
                    // Heard from its place (0x51b12a), and following its
                    // model while it plays (0x51b333).
                    Some(place) => {
                        let follow = place.follow.clone().filter(|m| self.model_center(m).is_some());
                        let at = follow
                            .as_deref()
                            .and_then(|m| self.model_center(m))
                            .unwrap_or(Vec3::from(place.at));
                        let (level, pan) = heard(&self.camera, at, place.near, place.far);
                        let id = self.audio.play_placed(pcm, clip.volume * level, pan, clip.looping, bus);
                        self.placed.push(Placed {
                            sound: id,
                            at,
                            follow,
                            near: place.near,
                            far: place.far,
                            gain: clip.volume,
                        });
                        id
                    }
                    None => self.audio.play(pcm, clip.volume, clip.looping, bus),
                };
                self.soundtrack_sounds.push((track.name.clone(), id));
            }
        }
        self.soundtracks = soundtracks;
        let audio = &self.audio;
        self.soundtrack_sounds
            .retain(|(_, id)| audio.is_playing(*id) || audio.is_silent());

        // What the engine queued to follow the actions now done.
        if self.actions.is_empty() && !self.deferred.is_empty() {
            for script in std::mem::take(&mut self.deferred) {
                let rule = Rule {
                    noun: "SCENE".into(),
                    verb: "FUNCTION".into(),
                    case: "ALL".into(),
                    approach: None,
                    target: None,
                    script,
                    source: "engine".into(),
                };
                self.run(renderer, rule, false);
            }
        }

        // The player arrived where an action wanted them.
        if let Some(approach) = &self.approach
            && !self
                .scene
                .as_ref()
                .is_some_and(|s| s.cast.is_walking(&approach.actor))
        {
            // Turning to face what was approached is the end of the walk:
            // the action waits for it.
            let turning = self
                .face_after
                .take()
                .is_some_and(|(target, within)| self.face_within(&target, within));
            if !turning {
                let approach = self.approach.take().unwrap();
                self.run(renderer, approach.rule, true);
            }
        }

        self.advance_speeches();
        if let Some(after) = self.skip_lines_after
            && self.captions.iter().any(|c| c.from + after <= now)
        {
            self.log.push("  (skips the line)".into());
            self.skip();
        }

        // Release scripts whose waits are over.
        let mut ready = Vec::new();
        self.tasks.retain(|task| {
            let done = match &task.until {
                Until::Time(t) => *t <= now,
                Until::Movie => self.showing.as_ref().is_none_or(|s| s.ended),
                Until::Speech(id) => !self.speeches.iter().any(|s| s.id == *id),
                Until::Walk(actor) => !self
                    .scene
                    .as_ref()
                    .is_some_and(|s| s.cast.is_walking(actor)),
                Until::LeftRegion(actor, region) => self.scene.as_ref().is_none_or(|s| {
                    let inside = s
                        .walk
                        .as_ref()
                        .zip(s.cast.actor_position(actor))
                        .is_some_and(|(w, (p, _))| w.region_at(p.x, p.z) == *region);
                    !inside || !s.cast.is_walking(actor)
                }),
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
        {
            let door = std::mem::take(&mut self.door_exit)
                && self.door_sounds
                && !self.door_heard
                && !location.eq_ignore_ascii_case("MAP");
            if door {
                self.log.push(format!("  door sound, to {location}"));
                self.sound("DOOROPEN", 1.0, Bus::Effects);
            }
            self.change_location(renderer, &location);
            // Shut behind them, once in.
            if door && self.scene.is_some() {
                self.scheduled
                    .push((self.clock + DOOR_SHUT_AFTER, "DOORCLOSE".into(), 1.0, Bus::Effects, None));
            }
        }
        if self.actions.is_empty() {
            self.door_action = false;
        }
        self.place_sounds();
        // What the scripts set going this frame, or a new room's entry,
        // that is due at once (a texture set on an animation's first frame,
        // as R25's view out of the window) is in place before it is drawn.
        self.run_cues(renderer, self.clock);
        // Whoever was moved since the cast was last sent (a new room's
        // entry placing the player, a script putting someone somewhere) is
        // drawn where they now are, not a frame late where they were; and
        // the floor's light about each is taken, once a frame, as the
        // original does just before it draws.
        if let Some(scene) = &mut self.scene {
            let room = &scene.room;
            let floor = scene.floor.as_deref();
            scene
                .cast
                .upload_lit(renderer, &|at| floor.and_then(|f| room.floor_light(f, at)));
        }
    }

    /// Goes somewhere, as G-Engine's `LocationManager` does: the new location
    /// is set first, then the timeblock rules are asked whether that ends
    /// the timeblock. If it does, the story waits on the title card (see
    /// [`World::continue_timeblock`]); otherwise the location is entered.
    pub fn change_location(&mut self, renderer: &Renderer, location: &str) {
        self.drop_last_frame();
        let location = gk3_game::state::key(location);
        let follow = std::mem::replace(&mut self.map_follow, crate::map::Follow::None);
        let mut probe = self.state.clone();
        probe.last_location = std::mem::replace(&mut probe.location, location.clone());
        let ending = {
            let mut copy = probe.clone();
            let mut api = GameApi::new(&mut copy, &self.scripts);
            gk3_game::timeblocks::check(&probe, &mut api)
        };
        match ending {
            Some((to, opens)) => {
                let location = opens.map_or(location, str::to_string);
                self.log.push(format!(
                    "timeblock {} is over: on to {to} at {location}",
                    self.state.timeblock
                ));
                self.end_timeblock(to, &location);
            }
            None if location == "MAP" => self.open_map(follow),
            None => {
                if let Err(e) = self.enter(renderer, &location) {
                    self.log.push(format!("  cannot enter {location}: {e}"));
                }
            }
        }
    }

    /// Puts up the driving map: the location becomes `MAP`, which is not a
    /// scene, and the map's music plays.
    pub fn open_map(&mut self, follow: crate::map::Follow) {
        if self.state.location != "MAP" {
            self.state.last_location = std::mem::replace(&mut self.state.location, "MAP".into());
        }
        let from = self.state.last_location.clone();
        self.scene = None;
        // The room's scripts go with it, as when entering another room:
        // left running with no room, their waits would end at once and a
        // script waiting on a timer in a loop would spin forever.
        self.tasks.clear();
        self.speeches.clear();
        self.scheduled.clear();
        self.cues.clear();
        self.deferred.clear();
        self.audio.stop_all();
        self.soundtracks.clear();
        self.soundtrack_sounds.clear();
        self.captions.clear();
        let roads = self
            .lib
            .read("PATHDATA.TXT")
            .map(|b| gk3_game::state::latin1(&b))
            .unwrap_or_default();
        let map = crate::map::MapScreen::new(&roads, &self.state, &from, follow);
        // G-Engine's choice of music: only these were ever used.
        let music = match (follow, self.state.timeblock.as_str()) {
            (crate::map::Follow::None, "202A") => "MAP302A",
            (crate::map::Follow::None, _) => "MAPGABEGENDAY1",
            _ => "MAPFOLLOWGEN",
        };
        for stk in [music, "MAPHARLEYAWAY"] {
            if let Ok(bytes) = self.lib.read(&format!("{stk}.STK")) {
                self.soundtracks.push(Soundtrack::parse(stk, &bytes));
            }
        }
        self.log.push(format!(
            "map from {from}, following {follow:?}: {}",
            map.shown.join(" ")
        ));
        self.map = Some(map);
        // Gabriel trying to follow the black car in the small hours of day
        // 3 talks to himself over the map; nothing in the data says so.
        let ego = self.state.ego.clone();
        if self.state.timeblock == "202A" && self.state.noun_verb(&ego, "CAR", "FOLLOW") == 1 {
            self.speak("2136L3W3K1", 2);
            *self
                .state
                .noun_verbs
                .entry((ego, "CAR".into(), "FOLLOW".into()))
                .or_default() += 1;
        }
    }

    /// Carries out what the fingerprint kit asks for.
    pub fn fingerprint_events(&mut self, events: Vec<crate::fingerprint::Event>) {
        use crate::fingerprint::{Event, GlassOutcome, dirty_glass};
        for event in events {
            match event {
                Event::Say(line) => {
                    self.speak(line, 1);
                }
                Event::Grant(i) => self.grant_print(i),
                Event::Close => self.fingerprint_close = true,
                Event::DirtyGlass(i) => {
                    let Some(kit) = &self.fingerprint else {
                        continue;
                    };
                    let wilkes = kit.object.noun.ends_with("WILKES");
                    let have = self.state.flags.contains("GOTSUITCASEBUCHELLIPRINT");
                    let now = *self.state.variables.get("DIRTYGLASSPRINT").unwrap_or(&0);
                    match dirty_glass(wilkes, have, now) {
                        Some(GlassOutcome::Known { value, line, grant }) => {
                            self.state.variables.insert("DIRTYGLASSPRINT".into(), value);
                            self.speak(line, 1);
                            if grant {
                                self.grant_print(i);
                            }
                            self.fingerprint_close = true;
                        }
                        Some(GlassOutcome::Ask { noun }) => {
                            self.speak("1EK0259291", 1);
                            self.fingerprint_label = Some(wilkes);
                            self.topic_bar = Some(noun.into());
                        }
                        Some(GlassOutcome::Done { value, item, grant }) => {
                            self.state.variables.insert("DIRTYGLASSPRINT".into(), value);
                            if let Some(item) = item {
                                let ego = self.state.ego.clone();
                                self.state.give_item(&ego, item);
                            }
                            if grant {
                                self.grant_print(i);
                            }
                            self.fingerprint_close = true;
                        }
                        None => self.fingerprint_close = true,
                    }
                }
            }
        }
    }

    /// A print is the player's: its item, flag and score.
    fn grant_print(&mut self, i: usize) {
        let Some(print) = self
            .fingerprint
            .as_ref()
            .and_then(|k| k.object.prints.get(i).cloned())
        else {
            return;
        };
        let ego = self.state.ego.clone();
        if let Some(item) = print.item {
            self.state.give_item(&ego, item);
        }
        let flag = match (ego.as_str(), print.flag_grace) {
            ("GRACE", Some(f)) => Some(f),
            _ => print.flag,
        };
        if let Some(flag) = flag {
            self.state.flags.insert(gk3_game::state::key(flag));
        }
        if let Some(score) = print.score {
            self.state.score_events.insert(gk3_game::state::key(score));
        }
        self.log
            .push(format!("  fingerprint {} kept", print.texture));
    }

    /// The player said whose print a dirty glass held.
    fn fingerprint_labelled(&mut self, topic: &str) {
        let Some(wilkes) = self.fingerprint_label.take() else {
            return;
        };
        let (value, item, grant) = crate::fingerprint::label(wilkes, topic);
        self.state.variables.insert("DIRTYGLASSPRINT".into(), value);
        if let Some(item) = item {
            let ego = self.state.ego.clone();
            self.state.give_item(&ego, item);
        }
        if grant {
            self.grant_print(0);
        }
        self.speak("1EK0259292", 1);
        self.fingerprint_close = true;
    }

    /// Brings SIDNEY up, with any new mail.
    pub fn open_sidney(&mut self) {
        crate::sidney::receive_email(&mut self.state);
        self.sidney = Some(crate::sidney::Sidney::new(self.lib));
        self.log.push("  SIDNEY up".into());
    }

    /// A press of the left button on SIDNEY (in its 640x480 pixels): the
    /// map takes it to mark points and handle shapes.
    pub fn sidney_press(&mut self, renderer: &Renderer, at: (f32, f32)) {
        let Some(sidney) = &mut self.sidney else {
            return;
        };
        let events = sidney.map_press(at, &self.state);
        self.sidney_events(renderer, events);
    }

    /// A click on SIDNEY, on whatever is under the pointer.
    pub fn sidney_click(&mut self, renderer: &Renderer, hit: crate::sidney::Hit) {
        let Some(sidney) = &mut self.sidney else {
            return;
        };
        let events = sidney.click(hit, self.lib, &mut self.state);
        self.sidney_events(renderer, events);
    }

    /// Carries out what SIDNEY asks for.
    fn sidney_events(&mut self, renderer: &Renderer, events: Vec<crate::sidney::Event>) {
        use crate::sidney::Event;
        use gk3_game::state::key;
        for event in events {
            match event {
                Event::Say(plate, lines) => {
                    self.speak(&plate, lines);
                }
                Event::Sound(name) => {
                    self.sound(name, 1.0, Bus::Effects);
                }
                Event::SetFlag(f) => {
                    self.state.flags.insert(key(f));
                }
                Event::SetFlagOwned(f) => {
                    self.state.flags.insert(key(&f));
                }
                Event::ClearFlag(f) => {
                    self.state.flags.remove(&key(f));
                }
                Event::Variable(v, n) => {
                    self.state.variables.insert(key(v), n);
                }
                Event::Score(s) => {
                    self.state.score_events.insert(key(&s));
                }
                Event::AddFile(id) => {
                    if self.state.sidney_files.insert(id)
                        && let Some(f) = crate::sidney::file(id)
                        && !f.score.is_empty()
                    {
                        self.state.score_events.insert(key(f.score));
                    }
                    self.log.push(format!("  SIDNEY file {id} added"));
                }
                Event::ShowInventory => self.state.inventory_shown = true,
                Event::GiveItem(item) => {
                    let ego = self.state.ego.clone();
                    self.state.give_item(&ego, item);
                }
                Event::Analyzed(id) => {
                    self.state.sidney_analyzed.insert(id);
                }
                Event::Pcm(pcm) => {
                    self.audio.play(pcm, 1.0, false, Bus::Effects);
                }
                Event::Exit => self.sidney_exit(renderer),
                Event::LsrState => {
                    let step = crate::sidney::lsr_step(&self.state);
                    self.state.variables.insert("LSRSTATE".into(), step);
                }
                Event::TakeItem(item) => {
                    let ego = self.state.ego.clone();
                    self.state.take_item(&ego, item);
                }
                Event::ChangeLocation(location) => {
                    self.close_sidney();
                    self.change_location(renderer, location);
                }
                Event::ExitIfOver => {
                    let ending = {
                        let mut copy = self.state.clone();
                        let mut api = GameApi::new(&mut copy, &self.scripts);
                        gk3_game::timeblocks::check(&self.state, &mut api)
                    };
                    if ending.is_some() {
                        self.sidney_exit(renderer);
                    }
                }
                Event::LoopSound(name) => {
                    if let Some(id) = self.sidney_loop.take() {
                        self.audio.stop(id);
                    }
                    if let Some(pcm) = name.and_then(|n| self.pcm(n)) {
                        self.sidney_loop = Some(self.audio.play(pcm, 1.0, true, Bus::Effects));
                    }
                }
            }
            // Leaving SIDNEY drops what it had still to ask.
            if self.sidney.is_none() {
                break;
            }
        }
    }

    /// Takes SIDNEY down, and any sound it was keeping up.
    fn close_sidney(&mut self) {
        self.sidney = None;
        if let Some(id) = self.sidney_loop.take() {
            self.audio.stop(id);
        }
    }

    /// Leaves SIDNEY, as G-Engine does: a change of location to the room
    /// already loaded, which asks whether the timeblock is over, then the
    /// room's `ExitSidney$`.
    fn sidney_exit(&mut self, renderer: &Renderer) {
        self.close_sidney();
        let here = self.state.location.clone();
        let ending = {
            let mut copy = self.state.clone();
            let mut api = GameApi::new(&mut copy, &self.scripts);
            gk3_game::timeblocks::check(&self.state, &mut api)
        };
        match ending {
            Some((to, opens)) => {
                let location = opens.map_or(here.clone(), str::to_string);
                self.log.push(format!(
                    "timeblock {} is over: on to {to} at {location}",
                    self.state.timeblock
                ));
                self.end_timeblock(to, &location);
            }
            None => {
                self.call(renderer, &format!("{here}_ALL"), "ExitSidney$");
            }
        }
    }

    /// Puts the binoculars up, if this lookout has a use now.
    fn open_binocs(&mut self) {
        let key = format!("{}{}", self.state.location, self.state.timeblock);
        let all = self
            .lib
            .read("BINOCS.TXT")
            .map(|b| crate::binocs::parse(&b))
            .unwrap_or_default();
        let Some(uses) = all.get(&key).cloned() else {
            self.log.push(format!("  no binoculars use at {key}"));
            return;
        };
        let saved = self.camera;
        // Far above the room only sky shows, whichever way one looks.
        self.camera.position.y += crate::binocs::LIFT;
        self.binocs = Some(Binocs {
            lookout_code: self.state.location.clone(),
            lookout: self.state.location.clone(),
            uses,
            saved,
            panned: (self.camera.yaw, self.camera.pitch),
            zoomed: None,
            queue: Vec::new(),
        });
        self.log.push(format!("  binoculars up at {key}"));
    }

    /// Turns the binoculars by an angle, in radians, within their limits.
    pub fn binocs_turn(&mut self, dyaw: f32, dpitch: f32) {
        let Some(b) = &mut self.binocs else { return };
        let (min, max) = crate::binocs::PITCH_RANGE;
        let (yaw, pitch) = match &b.zoomed {
            Some(z) => {
                let (ry, rp) = crate::binocs::ZOOMED_RANGE;
                (
                    (self.camera.yaw + dyaw).clamp(z.base.0 - ry, z.base.0 + ry),
                    (self.camera.pitch + dpitch).clamp(z.base.1 - rp, z.base.1 + rp),
                )
            }
            None => {
                b.panned = (b.panned.0 + dyaw, (b.panned.1 + dpitch).clamp(min, max));
                b.panned
            }
        };
        self.camera.yaw = yaw;
        self.camera.pitch = pitch;
    }

    /// The place the binoculars point at, if it can be zoomed into.
    pub fn binocs_target(&self) -> Option<String> {
        let b = self.binocs.as_ref()?;
        if b.zoomed.is_some() {
            return None;
        }
        crate::binocs::target(&b.lookout, &b.uses, b.panned.0, b.panned.1)
    }

    /// Zooms into the place pointed at: its scene, seen from its camera.
    pub fn binocs_zoom_in(&mut self, renderer: &Renderer) {
        let Some(place) = self.binocs_target() else {
            return;
        };
        let Some(b) = &self.binocs else { return };
        let Some(zoom) = b.uses.zooms.get(&place).cloned() else {
            return;
        };
        let room = match self.catalog.load(self.lib, &zoom.scene) {
            Ok(room) => room,
            Err(e) => {
                self.log
                    .push(format!("  cannot zoom to {}: {e}", zoom.scene));
                return;
            }
        };
        let gpu = renderer.upload(&room.mesh);
        let base = (zoom.angle.0.to_radians(), zoom.angle.1.to_radians());
        self.camera = Camera {
            position: zoom.position.into(),
            yaw: base.0,
            pitch: base.1,
            fov_y: crate::binocs::ZOOMED_FOV.to_radians(),
        };
        let lookout = b.lookout.clone();
        let Some(b) = &mut self.binocs else { return };
        // An "all" function for the place first, if the script has one.
        b.queue.push(format!("{lookout}ALL{place}ENT$"));
        b.queue.extend(zoom.enter.clone());
        b.zoomed = Some(Zoomed {
            place: place.clone(),
            gpu,
            base,
            exit: zoom.exit.clone(),
        });
        self.log.push(format!("  zoom to {place} ({})", zoom.scene));
    }

    /// Back out to the skyline.
    pub fn binocs_zoom_out(&mut self) {
        let Some(b) = &mut self.binocs else { return };
        let Some(z) = b.zoomed.take() else { return };
        self.log.push(format!("  zoom out of {}", z.place));
        b.queue.extend(z.exit);
        self.camera = Camera {
            position: b.saved.position + Vec3::Y * crate::binocs::LIFT,
            yaw: b.panned.0,
            pitch: b.panned.1,
            fov_y: b.saved.fov_y,
        };
    }

    /// Puts the binoculars away: the camera comes down, still facing the
    /// way they pointed, and the put-away animation plays.
    pub fn binocs_exit(&mut self, renderer: &Renderer) {
        let Some(b) = &self.binocs else { return };
        if b.zoomed.is_some() {
            return;
        }
        let b = self.binocs.take().unwrap();
        self.camera.position = b.saved.position;
        self.camera.fov_y = b.saved.fov_y;
        if let Some(anim) = b.uses.put_away {
            let rule = Rule {
                noun: "BINOCULARS".into(),
                verb: "PUT_AWAY".into(),
                case: "ALL".into(),
                approach: None,
                target: None,
                script: format!("wait StartAnimation(\"{anim}\");"),
                source: "BINOCS.TXT".into(),
            };
            self.run(renderer, rule, true);
        }
    }

    /// Takes a change of texture filtering: the room, the place the
    /// binoculars look at, and everyone and everything in the room.
    pub fn refilter(&mut self, renderer: &Renderer) {
        if let Some(scene) = &mut self.scene {
            renderer.rebind_scene(&mut scene.gpu);
            scene.cast.rebind(renderer);
        }
        if let Some(z) = self.binocs.as_mut().and_then(|b| b.zoomed.as_mut()) {
            renderer.rebind_scene(&mut z.gpu);
        }
    }

    /// Keeps walkers from walking into each other, as the original's
    /// walker does each frame (0x418c91, 0x4190cf, 0x41938c; see
    /// `docs/re-walking.md`). Only where there is a walk map.
    fn keep_walkers_apart(&mut self, now: f64) {
        let lib = self.lib;
        let Some(scene) = &mut self.scene else { return };
        let Some(walk) = scene.walk.as_ref() else { return };
        let walkers = scene.cast.walkers();
        if !walkers.iter().any(|w| w.moving || w.paused_until.is_some()) {
            return;
        }
        let ego = scene.cast.find(&scene.ego);
        let overlapping = |a: &crate::cast::Walker| -> Vec<usize> {
            walkers
                .iter()
                .filter(|b| b.index != a.index && b.moving && a.at.distance(b.at) - b.radius < a.radius)
                .map(|b| b.index)
                .collect()
        };
        let mut pauses: Vec<usize> = Vec::new();
        let mut reroutes: Vec<(usize, Vec<[f32; 2]>)> = Vec::new();
        let mut resumes: Vec<(usize, Vec<[f32; 2]>)> = Vec::new();
        let mut give_ups: Vec<usize> = Vec::new();
        for a in &walkers {
            let name = scene.cast.name_of(a.index);
            if let Some(until) = a.paused_until {
                if now < until {
                    continue;
                }
                // Looking again: still in the way, held up again; else on,
                // by a new route. Past the time limit, on regardless.
                let gave_up = a.ignore_others
                    || a.held_since.is_some_and(|t| now - t > crate::cast::GIVE_WAY_FOR);
                if gave_up {
                    give_ups.push(a.index);
                } else if !overlapping(a).is_empty() {
                    pauses.push(a.index);
                    continue;
                }
                let Some([x, z]) = a.goal else { continue };
                let from = Vec3::new(a.at.x, 0.0, a.at.y);
                let path = route(Some(walk), &scene.cast, &name, from, Vec3::new(x, 0.0, z), true)
                    .unwrap_or_default();
                resumes.push((a.index, path));
                continue;
            }
            if !a.moving || a.ignore_others {
                continue;
            }
            let hit = overlapping(a);
            if !hit.is_empty() {
                // The player's character has the right of way; otherwise
                // whoever is walked into waits (0x419055).
                let ego_hit = ego.is_some_and(|e| hit.contains(&e));
                if ego_hit && Some(a.index) != ego {
                    pauses.push(a.index);
                } else {
                    pauses.extend(hit.into_iter().filter(|&b| Some(b) != ego || !ego_hit));
                }
                continue;
            }
            // Those standing, and walkers near, as obstacles: when they
            // change and stand in the way, a new route round them.
            let others: Vec<usize> = walkers
                .iter()
                .filter(|b| b.index != a.index)
                .filter(|b| !b.moving || a.at.distance(b.at) - b.radius < 2.0 * a.radius)
                .map(|b| b.index)
                .collect();
            if others == a.others {
                continue;
            }
            scene.cast.set_walk_others(a.index, others.clone());
            let in_way = others.iter().filter_map(|&i| walkers.iter().find(|b| b.index == i)).any(|b| {
                let mut from = a.at;
                a.path.iter().any(|&p| {
                    let to = Vec2::from(p);
                    let near = segment_distance(b.at, from, to) < a.radius + b.radius;
                    from = to;
                    near
                })
            });
            if in_way && let Some([x, z]) = a.goal {
                let from = Vec3::new(a.at.x, 0.0, a.at.y);
                if let Some(path) = route(Some(walk), &scene.cast, &name, from, Vec3::new(x, 0.0, z), true) {
                    reroutes.push((a.index, path));
                }
            }
        }
        for i in give_ups {
            scene.cast.give_up_minding(i);
        }
        for (i, path) in reroutes {
            scene.cast.reroute_walk(i, path);
        }
        for (i, path) in resumes {
            scene.cast.resume_walk(lib, i, path, now);
        }
        pauses.sort_unstable();
        pauses.dedup();
        for i in pauses {
            // 0.75 to 2 seconds; a walker held up mid-stride waits at least
            // one (0x419270).
            self.give_way_seed ^= self.give_way_seed << 13;
            self.give_way_seed ^= self.give_way_seed >> 7;
            self.give_way_seed ^= self.give_way_seed << 17;
            let r = (self.give_way_seed >> 11) as f64 / (1u64 << 53) as f64;
            let mut seconds = 0.751 + r * 1.25;
            if seconds < 1.0 {
                seconds += 0.25;
            }
            if let Some(scene) = &mut self.scene {
                scene.cast.pause_walk(i, now, seconds);
            }
            let name = self.scene.as_ref().map(|s| s.cast.name_of(i)).unwrap_or_default();
            self.log.push(format!("  ({name} waits {seconds:.1}s for someone in the way)"));
        }
    }

    /// The scene being looked at: a zoomed-in place, or the room.
    pub fn view_scene(&self) -> Option<&GpuScene> {
        match self.binocs.as_ref().and_then(|b| b.zoomed.as_ref()) {
            Some(z) => Some(&z.gpu),
            None => self.scene.as_ref().map(|s| &s.gpu),
        }
    }

    /// A place on the map was clicked, by its button id.
    pub fn map_press(&mut self, id: &str) {
        let Some(map) = &self.map else { return };
        if !map.interactive() || self.map_leave.is_some() {
            return;
        }
        let Some(place) = crate::map::PLACES.iter().find(|p| p.id == id) else {
            return;
        };
        match crate::map::press(&self.state, place.location) {
            Err((plate, lines)) => {
                self.speak(&plate, lines);
            }
            Ok((location, bike)) => {
                self.sound("MAPBUTTON", 1.0, Bus::Effects);
                if let Some(bike) = bike {
                    self.state.variables.insert("BIKELOCATION".into(), bike);
                }
                self.map_leave = Some((self.clock, location));
            }
        }
    }

    /// Leaves the map for where the player came from.
    pub fn map_cancel(&mut self) {
        if self.map.is_some() && self.map_leave.is_none() {
            self.map_leave = Some((self.clock, self.state.last_location.clone()));
        }
    }

    /// Stops the room's sound and waits on the title card for `to`.
    pub fn end_timeblock(&mut self, to: &str, location: &str) {
        self.drop_last_frame();
        self.audio.stop_all();
        self.soundtracks.clear();
        self.soundtrack_sounds.clear();
        self.captions.clear();
        let to = gk3_game::state::key(to);
        // The timeblock left shows its ending film (if it has one) before
        // the card, as G-Engine's `EndCurrentTimeblock`; a new game's first
        // card has none.
        let leaving = self.state.timeblock.clone();
        if leaving != to {
            self.play_movie(&format!("{leaving}END"), AfterMovie::Nothing);
        }
        self.timeblock_change = Some(TimeblockChange {
            to,
            location: gk3_game::state::key(location),
        });
    }

    /// Shows a movie full screen, then does what is to follow. False if
    /// none is shown (it is not there, or movies are skipped), and then
    /// what was to follow is the caller's to do.
    pub fn play_movie(&mut self, name: &str, after: AfterMovie) -> bool {
        let movie = if self.skip_movies {
            None
        } else {
            crate::movie::Movie::open(self.lib, name)
        };
        let Some(movie) = movie else {
            self.log.push(format!(
                "  (movie {name} {})",
                if self.skip_movies { "skipped" } else { "not found" }
            ));
            return false;
        };
        self.log.push(format!("  (movie {name}, {:.0}s)", movie.seconds()));
        let stem = name.rsplit_once('.').map_or(name, |(s, _)| s);
        let captions = self
            .lib
            .read(&format!("{stem}.YAK"))
            .ok()
            .map(|b| Animation::parse(&b))
            .map(|yak| {
                let rate = yak.frame_rate as f64;
                yak.captions
                    .iter()
                    .map(|c| Caption {
                        text: c.text.clone(),
                        speaker: c.speaker.clone(),
                        from: c.frame as f64 / rate,
                        until: c.end.unwrap_or(c.frame + 3 * yak.frame_rate) as f64 / rate,
                    })
                    .collect()
            })
            .unwrap_or_default();
        self.showing = Some(Showing {
            name: name.to_ascii_uppercase(),
            movie,
            started: None,
            elapsed: 0.0,
            sound: None,
            after,
            captions,
            keep_last: false,
            ended: false,
        });
        true
    }

    /// The caller's clock as of the last update: the story's clock plus
    /// the time movies have taken.
    pub fn outside(&self) -> f64 {
        self.outside_clock
    }

    /// Stops everything, sound and all, while a menu or popup is up; or lets
    /// it go on.
    pub fn hold(&mut self, held: bool) {
        if self.held != held {
            self.held = held;
            self.audio.hold(held);
        }
    }

    /// The score so far.
    pub fn score(&self) -> u32 {
        gk3_game::scores::total(&self.state.score_events)
    }

    /// Ends the movie showing now, as the player skipping it does.
    /// Takes away a film's last frame left up, if one is.
    fn drop_last_frame(&mut self) {
        if self.showing.as_ref().is_some_and(|s| s.ended) {
            self.showing = None;
        }
    }

    pub fn stop_movie(&mut self, renderer: &Renderer) {
        let Some(show) = self.showing.take() else {
            return;
        };
        if let Some(sound) = show.sound {
            self.audio.stop(sound);
        }
        self.audio.solo(None);
        self.after_movie(renderer, show.after);
    }

    fn after_movie(&mut self, renderer: &Renderer, after: AfterMovie) {
        match after {
            AfterMovie::Nothing => {}
            AfterMovie::Enter(change) => {
                if let Err(e) = self.open_timeblock(renderer, change) {
                    self.log.push(format!("  cannot open the timeblock: {e}"));
                }
            }
        }
    }

    /// Plays the movie showing on to `now` (the caller's clock): its sound
    /// starts with its first frame, and it ends when its time is up.
    fn run_movie(&mut self, renderer: &Renderer, now: f64) {
        let Some(show) = &mut self.showing else {
            return;
        };
        let started = *show.started.get_or_insert_with(|| {
            if let Some(pcm) = show.movie.sound() {
                show.sound = Some(self.audio.play(std::sync::Arc::new(pcm), 1.0, false, Bus::Movie));
            }
            self.audio.solo(Some(Bus::Movie));
            now
        });
        show.elapsed = now - started;
        show.movie.advance(show.elapsed);
        if show.elapsed < show.movie.seconds() {
            return;
        }
        if !show.keep_last {
            self.stop_movie(renderer);
            return;
        }
        // Its last frame stays; the story goes on, and is heard again.
        show.ended = true;
        if let Some(sound) = show.sound.take() {
            self.audio.stop(sound);
        }
        self.audio.solo(None);
        let after = std::mem::replace(&mut show.after, AfterMovie::Nothing);
        self.after_movie(renderer, after);
    }

    /// The player has seen the title card: the story moves to the new
    /// timeblock, shows its beginning film if it has one, and opens in its
    /// location.
    pub fn continue_timeblock(&mut self, renderer: &Renderer) -> Result<(), String> {
        let Some(change) = self.timeblock_change.take() else {
            return Ok(());
        };
        if change.to != self.state.timeblock {
            self.state.set_timeblock(&change.to);
        }
        let film = format!("{}BEGIN", change.to);
        if self.play_movie(&film, AfterMovie::Enter(change.clone())) {
            return Ok(());
        }
        self.open_timeblock(renderer, change)
    }

    /// Opens a timeblock in its location, its title card and film seen.
    fn open_timeblock(&mut self, renderer: &Renderer, change: TimeblockChange) -> Result<(), String> {
        if change.location == "MAP" {
            self.open_map(crate::map::Follow::None);
            return Ok(());
        }
        self.enter(renderer, &change.location)?;
        // G-Engine: Grace's opening in these two depends on the last
        // location being this one.
        if matches!(change.to.as_str(), "202P" | "310A") {
            self.state.last_location = change.location.clone();
        }
        Ok(())
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
    /// What is under a point of the window, and the piece of the room (or
    /// figure) it is.
    pub fn pick_object(&self, x: f32, y: f32, width: u32, height: u32) -> (Pick, Option<String>) {
        let Some(scene) = &self.scene else {
            return (Pick::Nothing, None);
        };
        let aspect = width as f32 / height.max(1) as f32;
        let inverse = self.camera.view_proj(aspect).inverse();
        let ndc = (x / width as f32 * 2.0 - 1.0, 1.0 - y / height as f32 * 2.0);
        let near = inverse * Vec4::new(ndc.0, ndc.1, 0.0, 1.0);
        let far = inverse * Vec4::new(ndc.0, ndc.1, 1.0, 1.0);
        let origin = near.truncate() / near.w;
        let direction = (far.truncate() / far.w - origin).normalize();

        let mut best: Option<(f32, Pick, Option<String>)> = scene
            .cast
            .pick(origin, direction)
            .map(|(d, n)| (d, Pick::Noun(n), None));
        let bsp = &scene.room.bsp;
        for polygon in &bsp.polygons {
            let surface = &bsp.surfaces[polygon.surface as usize];
            let object = bsp.objects[surface.object as usize].to_ascii_uppercase();
            if scene.untouchable.contains(&object) {
                continue;
            }
            let noun = scene.nouns.get(&object);
            let is_floor = scene.floor.as_deref() == Some(object.as_str());
            // Hidden geometry is clickable only if it is a hit-test area,
            // which is hidden by nature; anything else hidden is gone (a
            // tile fallen away).
            if scene.room.hidden.contains(&object) && !scene.hit_tests.contains(&object) {
                continue;
            }
            for t in bsp.triangles(polygon) {
                let [a, b, c] = t.map(|i| Vec3::from(bsp.vertices[i as usize]));
                let Some(d) = ray_triangle(origin, direction, a, b, c) else {
                    continue;
                };
                if best.as_ref().is_some_and(|(bd, _, _)| *bd <= d) {
                    continue;
                }
                let what = match noun {
                    Some(n) => Pick::Noun(n.clone()),
                    None if is_floor => Pick::Floor(origin + direction * d),
                    // Walls and the like block what is behind them.
                    None => Pick::Nothing,
                };
                best = Some((d, what, Some(object.clone())));
            }
        }
        let (pick, object, d) = best.map_or((Pick::Nothing, None, f32::MAX), |(d, p, o)| (p, o, d));
        // Any figure nearer still is what is under the pointer, noun or no
        // (a pendulum platform), though clicks go on as before.
        match scene.cast.pick_any(origin, direction) {
            Some((fd, name)) if fd < d => (pick, Some(name)),
            _ => (pick, object),
        }
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

/// The verbs `VERBS.TXT` gives `type=Normal`: actions, as opposed to
/// inventory items and topics.
fn normal_verbs(text: &str) -> std::collections::HashSet<String> {
    text.lines()
        .filter_map(|l| {
            let l = l.split("//").next()?;
            let (name, rest) = l.split_once(',')?;
            rest.split(',')
                .any(|p| {
                    p.split_once('=').is_some_and(|(k, v)| {
                        k.trim().eq_ignore_ascii_case("type")
                            && v.trim().eq_ignore_ascii_case("normal")
                    })
                })
                .then(|| name.trim().to_ascii_uppercase())
        })
        .collect()
}

/// A `Lookit*` call: its component spec says what looks (`E` eyes, `H`
/// head; none, the eyes), `Quick` ones snap round, and no time is for good.
fn look(
    cast: &mut Cast,
    who: &str,
    target: crate::cast::LookTarget,
    spec: &str,
    seconds: f32,
    function: &str,
    now: f64,
) {
    let spec = spec.trim();
    let eyes = spec.is_empty() || spec.contains(['E', 'e']);
    let head = spec.contains(['H', 'h']);
    let quick = function.contains("QUICK");
    cast.look_with(who, target, Some(seconds as f64), now, eyes, head, quick);
}

/// The middle of a named BSP object.
fn object_center(bsp: &Bsp, object: &str) -> Option<Vec3> {
    let (mut lo, mut hi) = (Vec3::MAX, Vec3::MIN);
    let mut any = false;
    for polygon in &bsp.polygons {
        let surface = &bsp.surfaces[polygon.surface as usize];
        if !bsp.objects[surface.object as usize].eq_ignore_ascii_case(object) {
            continue;
        }
        for t in bsp.triangles(polygon) {
            for i in t {
                let v = Vec3::from(bsp.vertices[i as usize]);
                lo = lo.min(v);
                hi = hi.max(v);
                any = true;
            }
        }
    }
    any.then(|| (lo + hi) * 0.5)
}

/// Real verbs, when VERBS.TXT cannot be read; anything else is an
/// inventory item used on something.
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

/// How far a point is from a line segment, on the floor.
fn segment_distance(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let t = if ab.length_squared() > 1e-8 { ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0) } else { 0.0 };
    p.distance(a + ab * t)
}

/// The middle of a BSP object's vertices.
/// Someone's way from one point to another, round walls and closed-off
/// floor and everyone else who walks, as G-Engine's `WalkerBoundary`. A
/// walk to a named spot (`exact`) ends on it, as the story placed it, even
/// beside somebody; others end at the nearest free floor. With no walk map,
/// straight there.
/// Within this of a spot, a walker is there: G-Engine's `kAtNodeDist`.
const AT_SPOT: f32 = 12.25;

/// The kinds of walk the player's character makes, which the original
/// finishes differently (`docs/re-walking.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EgoWalk {
    /// A click on the floor: there within 4, and no walk at all within 24.
    Click,
    /// Into the first pose of an animation: there within 1, facing within
    /// 0.86 degrees.
    IntoAnimation,
    /// To a named spot, for an action: as `WalkTo`.
    ToSpot,
    /// Near a thing, for an action.
    Approach,
}

impl EgoWalk {
    fn tolerance(self) -> Tolerance {
        match self {
            EgoWalk::Click => Tolerance {
                arrive: 4.0,
                face: None,
            },
            EgoWalk::IntoAnimation => Tolerance::INTO_ANIMATION,
            EgoWalk::ToSpot => Tolerance::TO_SPOT,
            EgoWalk::Approach => Tolerance::EXACT,
        }
    }
}

const CLICK_NEAR: f32 = 24.0;

/// How near facing what an action approaches counts, as the original's
/// walks of each kind (`docs/re-walking.md`): walking to see something
/// (0x41c595), near a model (0x41c41d), turning to one (0x41cbf8).
const FACE_TO_SEE: f32 = 0.261_799_4;
const FACE_NEAR_MODEL: f32 = std::f32::consts::FRAC_PI_4;
const FACE_TURN_TO_MODEL: f32 = 0.015;

/// A footstep is a sound at the walker, as near as `FOOTSTEP_NEAR` heard
/// in full and fading no further past `FOOTSTEP_FAR` (0x40a23d: 60 and
/// 840, the same for everyone; the characters' `SoundMinDistance` and
/// `SoundMaxDistance` are read but not used for them).
const FOOTSTEP_NEAR: f32 = 60.0;
const FOOTSTEP_FAR: f32 = 840.0;

/// The original's rolloff for every sound placed in the room, set as its
/// sound system starts (0x5adae8) and handed to DirectSound's listener.
const SOUND_ROLLOFF: f32 = 3.0;

/// A sound placed in the room and playing.
struct Placed {
    sound: gk3_audio::SoundId,
    at: Vec3,
    /// A model it moves with, as a soundtrack's `Follow`.
    follow: Option<String>,
    near: f32,
    far: f32,
    gain: f32,
}

/// How a sound at `at` is heard from the camera: its level and each
/// channel's share (left, right). As DirectSound plays the original's 3D
/// sounds, here as Wine does (`dlls/dsound/sound3d.c`, `DSOUND_Calc3DBuffer`):
/// the original makes them muted beyond their far distance
/// (`DSBCAPS_MUTE3DATMAXDISTANCE`, 0x5b1985), near and far each between 1
/// and 10^9 (0x5b1828); within the near distance in full; beyond it, the
/// distance past it times the rolloff, the level falling by
/// `20 log10(d / near)` decibels. The camera's left is its forward crossed
/// with its up, as DirectSound's; the angle from it (negated behind the
/// camera, less a right angle) is shared between the left speaker at -90
/// degrees and the right at +90, by the square roots of how far between.
fn heard(camera: &gk3_render::Camera, at: Vec3, near: f32, far: f32) -> (f32, [f32; 2]) {
    use std::f32::consts::{FRAC_PI_2, PI, TAU};
    let near = near.clamp(1.0, 1.0e9);
    let far = far.clamp(1.0, 1.0e9);
    let to = at - camera.position;
    let distance = to.length();
    if distance > far {
        return (0.0, [0.0; 2]);
    }
    let d = near + (distance.max(near) - near) * SOUND_ROLLOFF;
    let millibels = -1000.0 * ((d / near) * (d / near)).log10();
    let level = 2f32.powf(millibels / 600.0);
    let front = camera.forward();
    let top = camera.rotation() * Vec3::Y;
    let left = front.cross(top);
    let angle = if to.length_squared() == 0.0 {
        0.0
    } else {
        let mut a = left.angle_between(to);
        if front.angle_between(to) > FRAC_PI_2 {
            a = -a;
        }
        a -= FRAC_PI_2;
        if a < -PI {
            a += TAU;
        }
        a
    };
    let pan = if (-FRAC_PI_2..=FRAC_PI_2).contains(&angle) {
        let a = (angle + FRAC_PI_2) / PI;
        [(1.0 - a).sqrt(), a.sqrt()]
    } else {
        // Round the back, from the right speaker to the left.
        let t = if angle < -FRAC_PI_2 { angle + TAU } else { angle };
        let a = ((t - FRAC_PI_2) / PI).clamp(0.0, 1.0);
        [a.sqrt(), (1.0 - a).sqrt()]
    };
    (level, pan)
}

/// How long after a door opens it shuts, in the new room.
const DOOR_SHUT_AFTER: f64 = 0.7;

fn route(
    walk: Option<&WalkMap>,
    cast: &Cast,
    who: &str,
    from: Vec3,
    to: Vec3,
    exact: bool,
) -> Option<Vec<[f32; 2]>> {
    let Some(walk) = walk else {
        return Some(vec![[to.x, to.z]]);
    };
    let (start, goal) = ([from.x, from.z], [to.x, to.z]);
    let path = walk
        .avoiding(&cast.walker_spots(who))
        .path(start, goal)
        // Hemmed in: better through them than not at all.
        .or_else(|| walk.path(start, goal))
        // Cut off: as near as can be got, as G-Engine's best effort.
        .or_else(|| walk.closest_path(start, goal));
    if !exact {
        return path;
    }
    // A script's walk must end where it says, as G-Engine's walker makes
    // sure (`mustReachDestination`): the spot itself ends the route, even
    // past where the walk map reaches, and with no route at all the walker
    // goes straight there.
    let mut path = path.unwrap_or_default();
    match path.last_mut() {
        Some(last) if walk.walkable(to.x, to.z) => *last = goal,
        Some(last) if Vec2::from(*last).distance(Vec2::from(goal)) < 1.0 => *last = goal,
        _ => path.push(goal),
    }
    Some(path)
}

/// The box around a piece of the room.
fn bsp_bounds(bsp: &Bsp, object: &str) -> Option<(Vec3, Vec3)> {
    let index = bsp
        .objects
        .iter()
        .position(|o| o.eq_ignore_ascii_case(object))? as u32;
    let mut bounds: Option<(Vec3, Vec3)> = None;
    for polygon in &bsp.polygons {
        if bsp.surfaces[polygon.surface as usize].object != index {
            continue;
        }
        for t in bsp.triangles(polygon) {
            for i in t {
                let v = Vec3::from(bsp.vertices[i as usize]);
                bounds = Some(bounds.map_or((v, v), |(lo, hi)| (lo.min(v), hi.max(v))));
            }
        }
    }
    bounds
}

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

/// What the temple's machinery asks of the room.
impl crate::mechanism::Room for World<'_> {
    fn play(&mut self, anim: &str, looping: bool) -> f32 {
        self.log.push(format!("  (machinery: {anim})"));
        self.start_anim(anim, looping)
    }

    fn stop(&mut self, anim: &str) {
        if let Some(scene) = &mut self.scene {
            scene.cast.stop(anim);
        }
    }

    fn is_playing(&self, anim: &str) -> bool {
        self.scene.as_ref().is_some_and(|s| s.cast.is_playing(anim))
    }

    fn sample(&mut self, anim: &str) {
        let lib = self.lib;
        if let Some(scene) = &mut self.scene {
            scene.cast.sample_first_frame(lib, anim);
        }
    }

    fn cut(&mut self, camera: &str) {
        if let Some(c) = self
            .scene
            .as_ref()
            .and_then(|s| s.cameras.get(&camera.to_ascii_uppercase()))
        {
            self.camera = shoot::from_room_camera(c);
        }
    }

    fn spot(&self, name: &str) -> Option<(Vec3, f32)> {
        self.scene
            .as_ref()?
            .cast
            .positions
            .get(&name.to_ascii_uppercase())
            .copied()
    }

    fn ego_at(&self) -> Vec3 {
        self.ego_position().map_or(Vec3::ZERO, |(p, _)| p)
    }

    fn put_ego(&mut self, at: Vec3, heading: f32) {
        if let Some(scene) = &mut self.scene {
            let ego = scene.ego.clone();
            scene.cast.put(&ego, at, heading);
        }
    }

    fn show(&mut self, model: &str, shown: bool) {
        if let Some(scene) = &mut self.scene {
            scene.cast.set_visible(model, shown);
        }
    }

    fn set_world(&mut self, model: &str, world: glam::Mat4) {
        if let Some(scene) = &mut self.scene {
            scene.cast.set_world(model, world);
        }
    }

    fn set_flag(&mut self, flag: &str, on: bool) {
        let flag = gk3_game::state::key(flag);
        if on {
            self.state.flags.insert(flag);
        } else {
            self.state.flags.remove(&flag);
        }
    }

    fn score(&mut self, event: &str) {
        self.state.score_events.insert(gk3_game::state::key(event));
    }

    fn script(&mut self, text: &str) {
        self.log.push(format!("  (machinery: {text})"));
        self.deferred.push(text.to_string());
    }

    fn forced_cuts(&mut self, on: bool) {
        self.forced_cuts = on;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placed_sounds_are_heard_as_directsound_hears_them() {
        // A camera at the origin looking along +Z (its right is +X).
        let camera = gk3_render::Camera {
            position: Vec3::ZERO,
            yaw: 0.0,
            pitch: 0.0,
            fov_y: 1.0,
        };
        let level = |at: Vec3| heard(&camera, at, FOOTSTEP_NEAR, FOOTSTEP_FAR).0;
        let pan = |at: Vec3| heard(&camera, at, FOOTSTEP_NEAR, FOOTSTEP_FAR).1;
        // In full within the near distance; about an eighth at 200 (a
        // rolloff of 3); muted past the far distance.
        assert_eq!(level(Vec3::new(0.0, 0.0, 50.0)), 1.0);
        assert!((level(Vec3::new(0.0, 0.0, 200.0)) - 0.125).abs() < 0.002);
        assert_eq!(level(Vec3::new(0.0, 0.0, 841.0)), 0.0);
        // Ahead: shared equally; to the right: all right; to the left: all
        // left; behind and to the left: mostly left.
        let ahead = pan(Vec3::new(0.0, 0.0, 100.0));
        assert!((ahead[0] - ahead[1]).abs() < 1e-5 && (ahead[0] - 0.5f32.sqrt()).abs() < 1e-5);
        let right = pan(Vec3::new(100.0, 0.0, 0.0));
        assert!(right[0] < 1e-3 && (right[1] - 1.0).abs() < 1e-5);
        let left = pan(Vec3::new(-100.0, 0.0, 0.0));
        assert!((left[0] - 1.0).abs() < 1e-5 && left[1] < 1e-3);
        let behind_left = pan(Vec3::new(-100.0, 0.0, -100.0));
        assert!(behind_left[0] > behind_left[1]);
    }

    #[test]
    fn trigger_rects_read_as_the_original_reads_them() {
        assert_eq!(
            trigger_rect("{309.55, 313.86, 438.45, 122.98}"),
            Some(((309.55, 122.98), (438.45, 313.86)))
        );
        // CSE212P's doubled comma and stray dot.
        assert_eq!(
            trigger_rect("{201.56,859.80,344.20,,1072.81}"),
            Some(((201.56, 859.80), (344.20, 1072.81)))
        );
        assert_eq!(
            trigger_rect("{385.26, 163.02, 935.11, 11.03.58}"),
            Some(((385.26, 11.03), (935.11, 163.02)))
        );
        assert_eq!(trigger_rect("{1, 2, 3}"), None);
    }

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
