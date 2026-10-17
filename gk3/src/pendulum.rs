//! TE3's pendulum: twenty-four platforms circle the room once every fifty
//! seconds while a great blade swings across it, through a slot on either
//! side. Gabriel jumps on from the doorway, from platform to platform, onto
//! the blade as it nears a slot, and off it onto the altar as it swings past
//! the middle. A platform in a slot when the blade arrives, or a drop too
//! far out, is death. The original keeps this in its code; this follows
//! G-Engine's `Pendulum` (GPL-3.0), whose figures come from the original's
//! `PENDULUM.TXT` and log.
//!
//! The room's own scripts take over at the altar (`GabeOnPillar$`) and on a
//! death (`Die$`).

use std::collections::VecDeque;
use std::f32::consts::TAU;

use glam::{Mat4, Vec3};

pub use crate::mechanism::Room;

const PLATFORMS: usize = 24;
const PLATFORM_ANGLE: f32 = TAU / PLATFORMS as f32;
/// One turn of the platforms, and the blade's cycle: six platforms' time.
const REVOLUTION_SECONDS: f32 = 50.0;
const PLATFORMS_PER_CYCLE: f32 = 6.0;
const MAX_ANGLE_DEGREES: f32 = 24.5;
/// The blade's arm: it hangs this far below its pivot.
const ARM: f32 = 2000.0;
/// The platforms' middles: this far out, this high.
const PLATFORM_RADIUS: f32 = 800.0;
const PLATFORM_TOP: f32 = 25.0;
/// The blade can kill, or be caught, this near the end of its swing.
const DANGER_DEGREES: f32 = 8.0;
/// Dropping from the blade: allowed this near the middle; safe nearer still.
const DROP_DEGREES: f32 = 15.0;
const SAFE_DROP_DEGREES: f32 = 3.0;

const IDLE: &str = "gabBreathMic";
const NORMAL_BLADE: &str = "TE3_PENDULUM_CENTER_CODE";
const GABE_BLADE: &str = "TE3__PENDULUM_GABE";
pub const ALTAR: &str = "TE3_HPALTAR";

/// Where Gabriel is in the puzzle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gabe {
    InEntryway,
    Jumping,
    OnPlatform,
    OnPendulum,
    AtAltar,
    Dying,
    DeadOnPlatform,
}

/// The blade's place in its swing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Blade {
    Swinging,
    DangerLeft,
    DangerRight,
}

/// Steps of a sequence, each due once the last has played.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Step {
    ForwardLand,
    Landed,
    JumpMiddle { to: usize, left: bool },
    JumpEnd { to: usize, left: bool },
    AfterKill { left_side: bool },
    Reset,
    AfterGrab,
    DropLand,
}

pub struct Pendulum {
    cycle_timer: f32,
    platform_rotation: f32,
    pub gabe: Gabe,
    platform: Option<usize>,
    blade: Blade,
    /// Sequence steps, each with the seconds left before it.
    steps: VecDeque<(f32, Step)>,
    /// A jump between platforms in flight: from, to, seconds left of one.
    jump: Option<(Vec3, usize, f32)>,
    /// The blade Gabriel holds is turned about when he caught it from the
    /// right.
    grabbed_right: bool,
}

impl Default for Pendulum {
    fn default() -> Self {
        Self {
            cycle_timer: 0.0,
            platform_rotation: 0.0,
            gabe: Gabe::InEntryway,
            platform: None,
            blade: Blade::Swinging,
            steps: VecDeque::new(),
            jump: None,
            grabbed_right: false,
        }
    }
}

/// A platform's model, from 0.
pub fn platform_model(i: usize) -> String {
    format!("TE3_R{:02}", i + 1)
}

pub fn platform_of(model: &str) -> Option<usize> {
    let n: usize = model.to_ascii_uppercase().strip_prefix("TE3_R")?.parse().ok()?;
    (1..=PLATFORMS).contains(&n).then(|| n - 1)
}

fn cycle_seconds() -> f32 {
    let speed = TAU / REVOLUTION_SECONDS;
    PLATFORM_ANGLE / speed * PLATFORMS_PER_CYCLE
}

fn cubic_in_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
    }
}

impl Pendulum {
    /// Sets the room up: Gabriel idling in the doorway, the blade at the
    /// top of its swing.
    pub fn start(&mut self, room: &mut dyn Room) {
        room.play(IDLE, true);
        self.place(room);
    }

    /// The blade's angle now, in radians: out and back once a cycle,
    /// easing at the ends.
    pub fn angle(&self) -> f32 {
        let max = MAX_ANGLE_DEGREES.to_radians();
        let half = cycle_seconds() / 2.0;
        if self.cycle_timer <= half {
            max + (-2.0 * max) * cubic_in_out(self.cycle_timer / half)
        } else {
            -max + (2.0 * max) * cubic_in_out((self.cycle_timer - half) / half)
        }
    }

    fn entry_platform(&self) -> usize {
        let rotation = self.platform_rotation.rem_euclid(TAU);
        let past = (rotation / PLATFORM_ANGLE) as usize;
        (PLATFORMS - 1) - past.min(PLATFORMS - 1)
    }

    fn left_slot(&self) -> usize {
        (self.entry_platform() + 6) % PLATFORMS
    }

    fn right_slot(&self) -> usize {
        (self.entry_platform() + PLATFORMS - 6) % PLATFORMS
    }

    /// Where the middle of a platform's top is now.
    pub fn platform_position(&self, i: usize) -> Vec3 {
        let a = PLATFORM_ANGLE * 0.5 + PLATFORM_ANGLE * i as f32 + self.platform_rotation;
        Vec3::new(a.sin() * PLATFORM_RADIUS, PLATFORM_TOP, a.cos() * PLATFORM_RADIUS)
    }

    /// Turns the platforms and swings the blade on to the next moment,
    /// moves Gabriel with them, and plays out any sequence.
    pub fn tick(&mut self, dt: f32, room: &mut dyn Room) {
        // Gabriel first, as G-Engine.
        match self.gabe {
            Gabe::Jumping => {
                if let Some((from, to, left)) = self.jump {
                    let left = (left - dt).max(0.0);
                    self.jump = Some((from, to, left));
                    let at = from.lerp(self.platform_position(to), 1.0 - left);
                    room.put_ego(at, (-at.x).atan2(-at.z));
                }
            }
            Gabe::OnPlatform => {
                if let Some(i) = self.platform {
                    let at = self.platform_position(i);
                    room.put_ego(at, (-at.x).atan2(-at.z));
                }
            }
            _ => {}
        }

        let cycle = cycle_seconds();
        self.cycle_timer = (self.cycle_timer + dt) % cycle;
        self.platform_rotation += TAU / REVOLUTION_SECONDS * dt;
        self.place(room);

        let angle = self.angle();
        let max = MAX_ANGLE_DEGREES.to_radians();
        if (max - angle.abs()).abs() < DANGER_DEGREES.to_radians() {
            let left = angle > 0.0;
            self.blade = if left { Blade::DangerLeft } else { Blade::DangerRight };
            let slot = if left { self.left_slot() } else { self.right_slot() };
            if self.gabe == Gabe::OnPlatform && self.platform == Some(slot) {
                let moving_left = self.cycle_timer > cycle / 2.0;
                self.platform_death(left, moving_left, room);
            }
        } else {
            self.blade = Blade::Swinging;
        }

        // The sequence under way.
        if let Some((left, _)) = self.steps.front_mut() {
            *left -= dt;
        }
        while let Some(&(left, step)) = self.steps.front() {
            if left > 0.0 {
                break;
            }
            self.steps.pop_front();
            self.step(step, room);
        }
    }

    /// Stands the platforms and the blade (Gabriel's or the empty one)
    /// where they are now.
    fn place(&self, room: &mut dyn Room) {
        let turn = Mat4::from_rotation_y(self.platform_rotation);
        for i in 0..PLATFORMS {
            room.set_world(&platform_model(i), turn);
        }
        let pivot = Vec3::Y * ARM;
        let swing = Mat4::from_translation(pivot)
            * Mat4::from_rotation_z(self.angle())
            * Mat4::from_translation(-pivot);
        let holding = self.gabe == Gabe::OnPendulum;
        room.show(NORMAL_BLADE, !holding && self.gabe != Gabe::AtAltar);
        room.show(GABE_BLADE, holding);
        room.set_world(NORMAL_BLADE, swing);
        let flip = if self.grabbed_right {
            Mat4::from_scale(Vec3::new(1.0, 1.0, -1.0))
        } else {
            Mat4::IDENTITY
        };
        room.set_world(GABE_BLADE, swing * flip);
    }

    fn after(&mut self, seconds: f32, step: Step) {
        self.steps.push_back((seconds, step));
    }

    fn step(&mut self, step: Step, room: &mut dyn Room) {
        match step {
            Step::ForwardLand => {
                room.cut("JUMP_OUT");
                let i = self.entry_platform();
                let at = self.platform_position(i);
                room.put_ego(at, (-at.x).atan2(-at.z));
                self.platform = Some(i);
                room.play("TE3_DOORCLOSE", false);
                let d = room.play("GABTE3JUMPFE", false);
                self.after(d, Step::Landed);
            }
            Step::Landed => {
                self.gabe = Gabe::OnPlatform;
                room.play(IDLE, true);
            }
            Step::JumpMiddle { to, left } => {
                self.jump = Some((room.ego_at(), to, 1.0));
                let d = room.play(if left { "GABTE3JUMPLM" } else { "GABTE3JUMPRM" }, false);
                self.after(d, Step::JumpEnd { to, left });
            }
            Step::JumpEnd { to, left } => {
                self.jump = None;
                let at = self.platform_position(to);
                room.put_ego(at, (-at.x).atan2(-at.z));
                self.platform = Some(to);
                let d = room.play(if left { "GABTE3JUMPLE" } else { "GABTE3JUMPRE" }, false);
                self.after(d, Step::Landed);
            }
            Step::AfterKill { left_side } => {
                self.gabe = Gabe::DeadOnPlatform;
                room.cut(if left_side { "AFTERKILL_HIGH" } else { "AFTERKILL_LOW" });
                self.after(2.0, Step::Reset);
            }
            Step::Reset => self.reset(room),
            Step::AfterGrab => room.cut("LONG_ALTAR"),
            Step::DropLand => {
                if self.angle().abs() < SAFE_DROP_DEGREES.to_radians() {
                    self.fall_to_altar(room);
                } else {
                    self.fall_to_death(room);
                }
            }
        }
    }

    /// Back to the doorway after a death, with the death screen up.
    fn reset(&mut self, room: &mut dyn Room) {
        room.script("wait CallSheep(\"TE3\", \"Die$\");");
        self.gabe = Gabe::InEntryway;
        room.set_flag("Te3GabeAtAltar", false);
        self.platform = None;
        self.jump = None;
        self.steps.clear();
        room.show("GABRIEL", true);
        room.show("GABMIC", true);
        if let Some((at, heading)) = room.spot("LOOKOUT") {
            room.put_ego(at, heading.to_radians());
        }
        room.play(IDLE, true);
        room.forced_cuts(false);
        room.sample("TE3_DOORCLOSE");
    }

    fn platform_death(&mut self, left_side: bool, moving_left: bool, room: &mut dyn Room) {
        self.gabe = Gabe::Dying;
        room.stop(IDLE);
        room.cut(if left_side { "KILL_HIGH" } else { "KILL_LOW" });
        let anim = match (left_side, moving_left) {
            (true, true) => "GABTE3PNFTH",
            (true, false) => "GABTE3PNBKH",
            (false, true) => "GABTE3PNBKL",
            (false, false) => "GABTE3PNFTL",
        };
        room.script("StartDialogue(\"1REGJ67Q81\", 1);");
        let d = room.play(anim, false);
        self.after(d, Step::AfterKill { left_side });
    }

    fn fall_to_death(&mut self, room: &mut dyn Room) {
        self.gabe = Gabe::Dying;
        room.show("GABRIEL", true);
        room.show("GABMIC", true);
        let below = Mat4::from_rotation_z(self.angle()).transform_vector3(Vec3::NEG_Y) * ARM
            + Vec3::Y * ARM
            - Vec3::Y * 250.0;
        room.put_ego(below, 0.0);
        room.cut("FALL_DOWN");
        let d = room.play("GABEFALLDEATH", false);
        self.after(d + 2.0, Step::Reset);
    }

    fn fall_to_altar(&mut self, room: &mut dyn Room) {
        room.show("GABRIEL", true);
        room.show("GABMIC", true);
        if let Some((at, heading)) = room.spot("ALTAR_LAND") {
            room.put_ego(at, heading.to_radians());
        }
        room.cut("ALTAR_UP");
        room.forced_cuts(false);
        self.gabe = Gabe::AtAltar;
        room.set_flag("Te3GabeAtAltar", true);
        room.script("wait CallSheep(\"TE3\", \"GabeOnPillar$\");");
    }

    /// The platform Gabriel may jump to by clicking this one, if any:
    /// any from the doorway; else the next one round either way.
    fn jumpable(&self, i: usize) -> bool {
        match (self.gabe, self.platform) {
            (Gabe::InEntryway, _) => true,
            (Gabe::OnPlatform, Some(on)) => i == (on + 1) % PLATFORMS || i == (on + PLATFORMS - 1) % PLATFORMS,
            _ => false,
        }
    }

    /// Whether the blade can be caught from where he stands: it is near a
    /// slot, and he is one or two platforms short of it.
    fn can_grab(&self) -> bool {
        if self.gabe != Gabe::OnPlatform || self.blade == Blade::Swinging || !self.steps.is_empty() {
            return false;
        }
        let slot = if self.blade == Blade::DangerLeft { self.left_slot() } else { self.right_slot() };
        let on = self.platform;
        on == Some((slot + PLATFORMS - 1) % PLATFORMS) || on == Some((slot + PLATFORMS - 2) % PLATFORMS)
    }

    fn can_drop(&self) -> bool {
        self.gabe == Gabe::OnPendulum && self.steps.is_empty() && self.angle().abs() < DROP_DEGREES.to_radians()
    }

    /// The cursor something under the pointer shows here, if the puzzle
    /// has a say: the red highlight on a platform he can jump to, the grab
    /// on a blade or altar he can make for.
    pub fn cursor(&self, model: &str) -> Option<&'static str> {
        let model = model.to_ascii_uppercase();
        if let Some(i) = platform_of(&model)
            && self.steps.is_empty()
            && self.jumpable(i)
        {
            return Some("C_ZOOM");
        }
        if model == NORMAL_BLADE && self.can_grab() {
            return Some("C_GRAB");
        }
        if model == ALTAR && self.can_drop() {
            return Some("C_GRAB");
        }
        None
    }

    /// A click on something. True if the puzzle takes it.
    pub fn click(&mut self, model: &str, room: &mut dyn Room) -> bool {
        let model = model.to_ascii_uppercase();
        if !self.steps.is_empty() {
            return matches!(self.gabe, Gabe::Jumping | Gabe::OnPendulum);
        }
        if let Some(i) = platform_of(&model)
            && self.jumpable(i)
        {
            room.stop(IDLE);
            if self.gabe == Gabe::InEntryway {
                self.gabe = Gabe::Jumping;
                let d = room.play("GABTE3JUMPFS", false);
                self.after(d, Step::ForwardLand);
            } else if let Some(on) = self.platform {
                // The next platform round is to his left.
                let left = i == (on + 1) % PLATFORMS;
                self.gabe = Gabe::Jumping;
                let at = self.platform_position(on);
                room.put_ego(at, (-at.x).atan2(-at.z));
                let d = room.play(if left { "GABTE3JUMPLS" } else { "GABTE3JUMPRS" }, false);
                self.after(d, Step::JumpMiddle { to: i, left });
            }
            return true;
        }
        if model == NORMAL_BLADE && self.can_grab() {
            let right = self.blade == Blade::DangerRight;
            self.gabe = Gabe::OnPendulum;
            self.grabbed_right = right;
            room.cut(if right { "KILL_LOW" } else { "KILL_HIGH" });
            room.forced_cuts(true);
            room.stop(IDLE);
            room.show("GABRIEL", false);
            room.show("GABMIC", false);
            self.place(room);
            let d = room.play("GABJMPPNDULM", false);
            self.after(d, Step::AfterGrab);
            room.score("e_temple_grab_pendulum");
            return true;
        }
        if self.gabe == Gabe::OnPendulum {
            if model == ALTAR && self.can_drop() {
                let d = room.play("GABJMPOFFPEN", false);
                self.after(d, Step::DropLand);
            }
            // Nothing else answers while he hangs on.
            return true;
        }
        false
    }

    /// For headless runs: what to click now to work towards a goal, as a
    /// player would. `"GRAB"`: catch the blade, stepping back a platform
    /// whenever a slot is about to come for him. `"DROP"`: let go just
    /// before the middle of the swing, the drop's 0.8 seconds ahead.
    pub fn advice(&self, goal: &str) -> Option<String> {
        if !self.steps.is_empty() {
            return None;
        }
        match goal {
            "GRAB" if self.can_grab() => Some(NORMAL_BLADE.into()),
            "GRAB" if self.gabe == Gabe::InEntryway => Some(platform_model(self.entry_platform())),
            "GRAB" => {
                let on = self.platform?;
                let danger = [self.left_slot(), self.right_slot(), (self.left_slot() + 1) % PLATFORMS];
                danger
                    .contains(&on)
                    .then(|| platform_model((on + PLATFORMS - 1) % PLATFORMS))
            }
            "DROP" => {
                let ahead = Pendulum {
                    cycle_timer: (self.cycle_timer + 0.8) % cycle_seconds(),
                    ..Default::default()
                };
                (self.can_drop() && ahead.angle().abs() < 0.5_f32.to_radians()).then(|| ALTAR.into())
            }
            _ => None,
        }
    }

    pub fn gabe(&self) -> Gabe {
        self.gabe
    }

    /// Seconds until he could catch the blade standing where he is, for
    /// tests: None if he never could.
    #[cfg(test)]
    fn simulate_until(&mut self, mut until: impl FnMut(&Self) -> bool, room: &mut dyn Room) -> Option<f32> {
        let mut t = 0.0;
        while t < 120.0 {
            if until(self) {
                return Some(t);
            }
            self.tick(1.0 / 30.0, room);
            t += 1.0 / 30.0;
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A room that records what is asked of it; every animation lasts a
    /// second.
    #[derive(Default)]
    struct Fake {
        ego: Vec3,
        cuts: Vec<String>,
        flags: Vec<(String, bool)>,
        scripts: Vec<String>,
    }

    impl Room for Fake {
        fn play(&mut self, anim: &str, looping: bool) -> f32 {
            match (looping, anim) {
                (true, _) => 0.0,
                // The drop: twelve frames.
                (false, "GABJMPOFFPEN") => 0.8,
                _ => 1.0,
            }
        }
        fn stop(&mut self, _: &str) {}
        fn is_playing(&self, _: &str) -> bool {
            false
        }
        fn sample(&mut self, _: &str) {}
        fn cut(&mut self, camera: &str) {
            self.cuts.push(camera.into());
        }
        fn spot(&self, _: &str) -> Option<(Vec3, f32)> {
            Some((Vec3::ZERO, 180.0))
        }
        fn ego_at(&self) -> Vec3 {
            self.ego
        }
        fn put_ego(&mut self, at: Vec3, _: f32) {
            self.ego = at;
        }
        fn show(&mut self, _: &str, _: bool) {}
        fn set_world(&mut self, _: &str, _: Mat4) {}
        fn set_flag(&mut self, flag: &str, on: bool) {
            self.flags.push((flag.into(), on));
        }
        fn score(&mut self, _: &str) {}
        fn script(&mut self, text: &str) {
            self.scripts.push(text.into());
        }
        fn forced_cuts(&mut self, _: bool) {}
    }

    #[test]
    fn the_blade_swings_out_and_back() {
        let mut p = Pendulum::default();
        let max = MAX_ANGLE_DEGREES.to_radians();
        assert!((p.angle() - max).abs() < 1e-5);
        p.cycle_timer = cycle_seconds() / 2.0;
        assert!((p.angle() + max).abs() < 1e-5);
        p.cycle_timer = cycle_seconds() / 4.0;
        assert!(p.angle().abs() < 1e-4);
        // Six platforms' time: about 12.5 seconds.
        assert!((cycle_seconds() - 12.5).abs() < 0.01);
    }

    #[test]
    fn platforms_are_where_their_models_are() {
        let p = Pendulum::default();
        // The first platform's middle, 7.5 degrees round from +Z.
        let at = p.platform_position(0);
        assert!((at.x.atan2(at.z).to_degrees() - 7.5).abs() < 0.01);
        assert_eq!(p.entry_platform(), 23);
        assert_eq!((p.left_slot(), p.right_slot()), (5, 17));
        assert_eq!(platform_of("te3_r24"), Some(23));
    }

    #[test]
    fn he_jumps_on_rides_and_dies_in_a_slot() {
        let mut room = Fake::default();
        let mut p = Pendulum::default();
        p.start(&mut room);
        assert!(p.click("te3_r05", &mut room));
        // Jump start, then landing: a second each.
        p.simulate_until(|p| p.gabe == Gabe::OnPlatform, &mut room).unwrap();
        assert_eq!(p.platform, Some(p.entry_platform()));
        // Standing still, his platform comes round to a slot as the blade
        // arrives, sooner or later.
        p.simulate_until(|p| p.gabe == Gabe::DeadOnPlatform, &mut room)
            .expect("the blade finds him");
        p.simulate_until(|p| p.gabe == Gabe::InEntryway, &mut room).unwrap();
        assert!(room.scripts.iter().any(|s| s.contains("Die$")));
    }

    #[test]
    fn caught_and_dropped_at_the_middle_he_reaches_the_altar() {
        let mut room = Fake::default();
        let mut p = Pendulum::default();
        p.start(&mut room);
        p.click("te3_r05", &mut room);
        p.simulate_until(|p| p.gabe == Gabe::OnPlatform, &mut room).unwrap();
        // Ride until the blade can be caught, stepping back a platform
        // whenever a slot is about to come for him.
        let mut caught = false;
        for _ in 0..4000 {
            if p.can_grab() {
                assert!(p.click(NORMAL_BLADE, &mut room));
                caught = true;
                break;
            }
            if p.gabe == Gabe::OnPlatform
                && let Some(on) = p.platform
                && (on == p.left_slot() || on == p.right_slot() || on == (p.left_slot() + 1) % PLATFORMS)
            {
                p.click(&platform_model((on + PLATFORMS - 1) % PLATFORMS), &mut room);
            }
            p.tick(1.0 / 30.0, &mut room);
            assert_ne!(p.gabe, Gabe::DeadOnPlatform, "he should have dodged");
        }
        assert!(caught);
        assert_eq!(p.gabe, Gabe::OnPendulum);
        p.simulate_until(|p| p.can_drop() && p.angle().abs() < 0.01, &mut room).unwrap();
        // Let go just before the middle, so as to land on it.
        let mut dropped = false;
        for _ in 0..2000 {
            let ahead = {
                let mut q = p.clone_state();
                q.cycle_timer = (q.cycle_timer + 0.8) % cycle_seconds();
                q.angle().abs()
            };
            if p.can_drop() && ahead < 0.5_f32.to_radians() {
                p.click(ALTAR, &mut room);
                dropped = true;
                break;
            }
            p.tick(1.0 / 30.0, &mut room);
        }
        assert!(dropped);
        p.simulate_until(|p| p.gabe == Gabe::AtAltar, &mut room).expect("lands on the altar");
        assert!(room.flags.contains(&("Te3GabeAtAltar".into(), true)));
        assert!(room.scripts.iter().any(|s| s.contains("GabeOnPillar$")));
    }

    impl Pendulum {
        fn clone_state(&self) -> Box<Pendulum> {
            Box::new(Pendulum {
                cycle_timer: self.cycle_timer,
                platform_rotation: self.platform_rotation,
                gabe: self.gabe,
                platform: self.platform,
                blade: self.blade,
                steps: VecDeque::new(),
                jump: None,
                grabbed_right: self.grabbed_right,
            })
        }
    }
}
