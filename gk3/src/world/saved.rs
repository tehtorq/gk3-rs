//! Saved games as the original makes them (`docs/re-saves.md`): a snapshot
//! of the running world, taken only while the player is in control, and
//! put back without entering the room again. No entry scripts run and no
//! visit is counted; actors, timers, the camera and the soundtracks carry
//! on as they were; and the room's background functions start again from
//! the top, as the original starts them (0x50986b).

use serde::{Deserialize, Serialize};

use super::*;
use crate::camera::BoundsChanges;
use crate::cast::CastSnapshot;
use gk3_game::walk::WalkChanges;

/// Everything about the world a saved game keeps beyond the story itself.
#[derive(Serialize, Deserialize)]
pub(super) struct Snapshot {
    /// The story as it stood when the room was entered, which its file is
    /// read under again; and the location entered.
    entry: String,
    location: String,
    clock: f64,
    camera: Camera,
    forced_cuts: bool,
    inspecting: Option<String>,
    inspect_from: Option<Camera>,
    active_object: Option<String>,
    last_trigger: Option<String>,
    last_plate: Option<String>,
    verb_modal: bool,
    verb_cancel: bool,
    give_way_seed: u64,
    game_timers: Vec<(f64, String, String)>,
    moods: HashMap<String, String>,
    timed_shows: Vec<(f64, String, bool)>,
    room: RoomChanges,
    cast: CastSnapshot,
    soundtracks: Vec<Soundtrack>,
    angels: crate::angels::Angels,
    angels_erase_at: Option<f64>,
    laser_heads: Option<crate::laser_heads::LaserHeads>,
    chessboard: Option<crate::chessboard::Chessboard>,
    pendulum: Option<crate::pendulum::Pendulum>,
    bridge: Option<crate::bridge::Bridge>,
    gps: Option<crate::gps::Gps>,
    sidney: Option<crate::sidney::Sidney>,
    fingerprint: Option<crate::fingerprint::Kit>,
    fingerprint_close: bool,
    fingerprint_label: Option<bool>,
    /// The background functions running, by script and function: started
    /// again from the top on restoring.
    backgrounds: Vec<(String, String)>,
}

/// What the story has done to the room since it was built.
#[derive(Serialize, Deserialize)]
struct RoomChanges {
    /// Another scene for the same place (`SetScene`).
    scene: Option<String>,
    hidden: Vec<String>,
    retextured: Vec<(String, String)>,
    untouchable: Vec<String>,
    bounds: BoundsChanges,
    walk: Option<WalkChanges>,
}

/// Whether a thread's function is a background one, which the original
/// keeps by name (0x509f15).
fn background(function: &str) -> bool {
    function.to_ascii_uppercase().ends_with("_BACKGROUND$")
}

impl World<'_> {
    /// The player is in control, the original's mode 0 on top (0x4a881b):
    /// no action or its walk, no line being said, no movie, no skip, and
    /// nothing the original holds the player off for (the pendulum's fall
    /// and kill, 0x45898f, 0x458c48).
    pub fn in_control(&self) -> bool {
        let pendulum_busy = self.pendulum.as_ref().is_some_and(|p| {
            matches!(
                p.state(),
                crate::pendulum::State::Falling | crate::pendulum::State::Dying
            )
        });
        self.idle()
            && self.speeches.is_empty()
            && self.showing.as_ref().is_none_or(|s| s.ended)
            && self.skip_lock.is_none()
            && !pendulum_busy
    }

    /// Whether the game may be saved now, from the world's side: in
    /// control, and what 0x4cbe72 asks besides. No conversation is open,
    /// it is not TE6, and the screen up is the game, the fingerprint kit
    /// or SIDNEY (a timeblock's title card and the death screen are the
    /// window's to allow). The verb menu and the window's own screens
    /// are the window's to check.
    pub fn may_save(&self) -> Result<(), String> {
        if self.timeblock_change.is_some() && self.actions.is_empty() {
            return Ok(());
        }
        if !self.in_control() {
            return Err("something is happening".into());
        }
        if self.conversation.is_some() || self.topic_bar.is_some() {
            return Err("in a conversation".into());
        }
        if self.state.location == "TE6" {
            return Err("not here".into());
        }
        if self.binocs.is_some()
            || self.map.is_some()
            || self.state.inventory_shown
            || self.sidney.as_ref().is_some_and(|s| s.lsr_map_up())
        {
            return Err("not on this screen".into());
        }
        Ok(())
    }

    /// Whether a saved game may be put back now, from the world's side
    /// (0x4cbebf): the player in control, and not behind the binoculars
    /// or a movie.
    pub fn may_restore(&self) -> Result<(), String> {
        if self.scene.is_some() && !self.in_control() && self.timeblock_change.is_none() {
            return Err("something is happening".into());
        }
        if self.binocs.is_some() || self.showing.as_ref().is_some_and(|s| !s.ended) {
            return Err("not on this screen".into());
        }
        Ok(())
    }

    /// The world as it stands, for a saved game.
    pub(super) fn snapshot(&self) -> Result<Snapshot, String> {
        let scene = self.scene.as_ref().ok_or("no room is up")?;
        let entry = self.entry.as_ref().ok_or("the room was not entered")?;
        let room = RoomChanges {
            scene: (scene.room.name != scene.room.built_as).then(|| scene.room.name.clone()),
            hidden: {
                let mut h: Vec<String> = scene.room.hidden.iter().cloned().collect();
                h.sort();
                h
            },
            retextured: scene.room.retextures(),
            untouchable: {
                let mut u: Vec<String> = scene.untouchable.iter().cloned().collect();
                u.sort();
                u
            },
            bounds: scene.camera_bounds.changes(),
            walk: scene.walk.as_ref().map(|w| w.changes()),
        };
        let mut backgrounds: Vec<(String, String)> = self
            .vm
            .running()
            .into_iter()
            .filter(|(_, f)| background(f))
            .collect();
        backgrounds.dedup();
        Ok(Snapshot {
            entry: entry.to_save(&[]),
            location: self.state.location.clone(),
            clock: self.clock,
            camera: self.camera,
            forced_cuts: self.forced_cuts,
            inspecting: self.inspecting.clone(),
            inspect_from: self.inspect_from,
            active_object: self.active_object.clone(),
            last_trigger: self.last_trigger.clone(),
            last_plate: self.last_plate.clone(),
            verb_modal: self.verb_modal,
            verb_cancel: self.verb_cancel,
            give_way_seed: self.give_way_seed,
            game_timers: self.game_timers.clone(),
            moods: self.moods.clone(),
            timed_shows: self.timed_shows.clone(),
            room,
            cast: scene.cast.snapshot()?,
            soundtracks: self.soundtracks.clone(),
            angels: self.angels.clone(),
            angels_erase_at: self.angels_erase_at,
            laser_heads: self.laser_heads.clone(),
            chessboard: self.chessboard.clone(),
            pendulum: self.pendulum.clone(),
            bridge: self.bridge.clone(),
            gps: self.gps.clone(),
            sidney: self.sidney.clone(),
            fingerprint: self.fingerprint.clone(),
            fingerprint_close: self.fingerprint_close,
            fingerprint_label: self.fingerprint_label,
            backgrounds,
        })
    }

    /// Puts the world back as a snapshot has it: the room built again as
    /// it was on entry (its own code's set-up included, as the original
    /// makes its objects again from the file), then the story and
    /// everything that has changed since put back over it. Nothing is
    /// entered: no entry scripts, no visit counted.
    pub(super) fn restore_snapshot(
        &mut self,
        renderer: &Renderer,
        state: GameState,
        saved: Snapshot,
    ) -> Result<(), String> {
        let (entry, _) = GameState::from_save(&saved.entry)?;
        self.state = entry;
        self.state.location = saved.location.clone();
        self.build_room(renderer, &saved.location)?;
        if let Some(name) = &saved.room.scene {
            self.set_scene(renderer, name);
        }
        self.state = state;

        let scene = self.scene.as_mut().ok_or("the room was not built")?;
        scene.room.hidden = saved.room.hidden.into_iter().collect();
        for (object, texture) in &saved.room.retextured {
            scene.room.retexture(self.lib, object, texture);
        }
        scene.room.rebuild();
        scene.gpu = renderer.upload(&scene.room.mesh);
        scene.untouchable = saved.room.untouchable.into_iter().collect();
        scene.camera_bounds.apply(saved.room.bounds);
        if let (Some(walk), Some(changes)) = (scene.walk.as_mut(), saved.room.walk) {
            walk.apply(changes);
        }
        scene.cast.restore(self.lib, saved.cast)?;
        scene.cast.upload(renderer);

        // The story's clock goes on from where it was saved.
        self.clock = saved.clock;
        self.paused = self.outside_clock - saved.clock;
        self.camera = saved.camera;
        self.forced_cuts = saved.forced_cuts;
        self.inspecting = saved.inspecting;
        self.inspect_from = saved.inspect_from;
        self.active_object = saved.active_object;
        self.last_trigger = saved.last_trigger;
        self.last_plate = saved.last_plate;
        self.verb_modal = saved.verb_modal;
        self.verb_cancel = saved.verb_cancel;
        self.give_way_seed = saved.give_way_seed;
        self.game_timers = saved.game_timers;
        self.moods = saved.moods;
        self.timed_shows = saved.timed_shows;
        self.angels = saved.angels;
        self.angels_erase_at = saved.angels_erase_at;
        self.laser_heads = saved.laser_heads;
        self.chessboard = saved.chessboard;
        self.pendulum = saved.pendulum;
        self.bridge = saved.bridge;
        self.gps = saved.gps;
        self.sidney = saved.sidney;
        if let Some(sidney) = &mut self.sidney {
            sidney.restored(self.lib);
        }
        self.fingerprint = saved.fingerprint;
        self.fingerprint_close = saved.fingerprint_close;
        self.fingerprint_label = saved.fingerprint_label;
        // The room's sounds start afresh; its soundtracks carry on from
        // their step.
        for (_, id) in self.soundtrack_sounds.drain(..) {
            self.audio.stop(id);
        }
        self.placed.clear();
        self.soundtracks = saved.soundtracks;
        for track in &mut self.soundtracks {
            track.restored();
        }

        // The background functions, from the top. The scripts the room
        // started on entry went with the old world.
        self.vm = Vm::new();
        self.tasks.clear();
        for (script, function) in saved.backgrounds {
            self.start_background(renderer, &script, &function);
        }
        Ok(())
    }

    /// Starts a script's function on its own, as the original starts the
    /// saved background functions again (0x509a60): not as an action, so
    /// the player is not held up.
    fn start_background(&mut self, renderer: &Renderer, script_name: &str, function: &str) {
        let Some(script) = self.scripts.get(&script_name.to_ascii_uppercase()).cloned() else {
            self.log.push(format!(
                "  cannot start {script_name}'s {function}: no such script"
            ));
            return;
        };
        let (result, effects) = {
            let whereabouts = self.whereabouts();
            let mut api = GameApi::new(&mut self.state, &self.scripts);
            api.whereabouts = Some(&whereabouts);
            let result = self.vm.start(&mut api, script, script_name, function);
            (result, std::mem::take(&mut api.effects))
        };
        match result {
            Ok(_) => {
                self.log
                    .push(format!("  {script_name}'s {function} starts again"));
                self.perform(renderer, effects);
            }
            Err(e) => self.log.push(format!("  cannot start {function}: {e}")),
        }
    }
}
