//! TE3's pendulum: twenty-four platforms circle the room once every fifty
//! seconds while a great blade swings across it, through a slot on either
//! side. Gabriel jumps on from the doorway, from platform to platform, onto
//! the blade as it comes up to a slot, and off it onto the altar as it
//! swings past the middle. A platform in a slot when the blade arrives, or
//! a drop too far out, is death. The original's `Circle` scene
//! (`custom=Circle`; `docs/re-pendulum.md`), whose figures come from
//! `PENDULUM.TXT`.
//!
//! The room's own scripts take over at the altar (`GabeOnPillar$`) and on a
//! death (`die$`); the death screen then asks the scene to `restart`.

use std::f32::consts::{FRAC_PI_6, PI, TAU};

use glam::{Mat4, Vec3};

pub use crate::mechanism::Room;

const PLATFORMS: i32 = 24;
const PLATFORM_ANGLE: f32 = TAU / PLATFORMS as f32;
/// `PENDULUM.TXT`: one turn of the platforms (`circlePeriod`, seconds);
/// the blade's cycle in platforms' time (`pendulumCycle`); its swing
/// (`pendulumAngle`, degrees); the step (`updateTime`, ms); the drop's
/// window (`startSafe`, `endSafe`, degrees out) and how near the middle a
/// drop lands (`landDist`, degrees).
const CIRCLE_PERIOD: f32 = 50.0;
const PENDULUM_CYCLE: f32 = 6.0;
const PENDULUM_ANGLE: f32 = 24.5;
const UPDATE_MS: f64 = 20.0;
const START_SAFE: f32 = 15.0;
const END_SAFE: f32 = 3.0;
const LAND_DIST: f32 = 3.0;
/// The blade's pivot, this high above the floor (`mPendHeight`).
const PEND_HEIGHT: f32 = 2009.51;
/// Each catch of the blade takes this off its swing (0x45871d).
const GRAB_LOSS: f32 = 0.034_906_585;
/// A jump between platforms turns him this far, over this long (0x4593d6,
/// 0x4592d7).
const JUMP_TURN: f32 = 0.261_799_4;
const JUMP_MS: i32 = 800;
/// Jumping on, he lands this far out on his platform (0x459117).
const LANDING_RADIUS: f32 = 775.0;
/// Dropping short, he falls facing this way, by the swing (0x45898f).
const FALL_HEADING: f32 = 1.3;
/// Radians to platforms (24 / 2π, as the original rounds it).
const PLATFORMS_PER_RADIAN: f32 = 3.819_718_6;
/// Radians to degrees, as the original rounds it.
const DEGREES: f32 = 57.295_776;

/// The danger zones, as phases of the blade's cycle (0 the top of the
/// swing on the high side; 0x459d83): the blade going down from the high
/// slot (over 0), coming up to it, and the same on the low side.
const HIGH_DOWN: (f32, f32) = (6.195_919, 6.195_919 + FRAC_PI_6 - TAU);
const HIGH_UP: (f32, f32) = (6.195_919 - FRAC_PI_6, 6.195_919);
const LOW_UP: (f32, f32) = (3.054_326_3 - FRAC_PI_6, 3.054_326_3);
const LOW_DOWN: (f32, f32) = (3.054_326_3, 3.054_326_3 + FRAC_PI_6);

const NORMAL_BLADE: &str = "TE3_PENDULUM_CENTER_CODE";
const GABE_BLADE: &str = "TE3__PENDULUM_GABE";
/// The blade stuck in its slot, shown while he dies.
const KILL_BLADE: &str = "TE3_PENDULUM_CENTER";
pub const ALTAR: &str = "TE3_HPALTAR";
/// The room itself, whose clicks the puzzle leaves to the room.
const ROOM_SHELL: [&str; 4] = ["TE3_CIELING", "TE3_INNERWALLS", "TE3_OUTER_WALLS", "TE3_WALLSLOTS"];
const NOT_NOW: &str = "StartDialogue(\"1SERM44V51\", 1);";
const CURSOR: &str = "C_ZOOM";

/// The scene's state (+0xdc; 6 is never used).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Doorway,
    OnPlatform,
    OnBlade,
    AtAltar,
    Falling,
    Dying,
    JumpingOn,
    LandingOn,
    Jumping,
    Dropping,
}

/// The blade in a danger zone: coming up to a slot or going down from it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Zone {
    None,
    Up,
    Down,
}

/// What the scene is told when something it started ends (0x458c48).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Message {
    /// 0: the death played out.
    Killed,
    /// 1: the death screen's turn.
    Die,
    /// 2: a while on the blade, the long view of the altar.
    LongAltar,
    /// 6: the jump on is in the air.
    JumpedOn,
    /// 7: landed.
    Landed,
    /// 8, 9: a jump between platforms is in the air, left or not.
    Leapt { left: bool },
    /// 10, 11: over the gap, coming down.
    Over { left: bool },
    /// 13: let go of the blade.
    Dropped,
}

pub struct Pendulum {
    state: State,
    /// The scene's clock, ms, and its steps' (`lastUpdate`, `nextUpdate`,
    /// `startCycle`); `lastUpdate` 0 until the first step.
    now: f64,
    last_update: f64,
    next_update: f64,
    start_cycle: f64,
    pend_on: bool,
    /// The blade's swing (`mPendSwing`), angle (`mPendAngle`) and last step
    /// (`mPendSpd`), radians.
    swing: f32,
    angle: f32,
    speed: f32,
    /// The platforms' turn (`mFloorRot`) and as it will be a second on
    /// (`mPredRot`).
    floor_rot: f32,
    pred_rot: f32,
    /// What the blade's present model, and the platforms, are turned by.
    blade_world: Mat4,
    floor_world: Mat4,
    /// Which blade model is the present one.
    gabe_blade: bool,
    /// The zone the blade was in, is in, and will be in a second on, with
    /// the platform it will endanger (`mPredTarget`).
    was_zone: Zone,
    zone: Zone,
    pred_zone: Zone,
    pred_target: i32,
    /// The platform he is on and the one he is making for (-2 off them,
    /// -3 on the blade).
    gabe_block: i32,
    tgt_block: i32,
    /// A jump's time left, ms, and its turn a ms.
    time_left: i32,
    gabe_ang_v: f32,
    will_hit: bool,
    will_grab: bool,
    was_hit: bool,
    /// Which way the blade he holds faces.
    high: bool,
    /// Where he stands while carried round, and his heading.
    ego: Option<(Vec3, f32)>,
    /// Messages due, at ms.
    due: Vec<(f64, Message)>,
}

impl Default for Pendulum {
    fn default() -> Self {
        let swing = PENDULUM_ANGLE.to_radians();
        Self {
            state: State::Doorway,
            now: 0.0,
            last_update: 0.0,
            next_update: 0.0,
            start_cycle: 0.0,
            pend_on: true,
            swing,
            // As the original: the swing in radians, made radians again.
            angle: swing.to_radians(),
            speed: 0.0,
            floor_rot: 0.0,
            pred_rot: floor_rate() * 1000.0,
            blade_world: Mat4::IDENTITY,
            floor_world: Mat4::IDENTITY,
            gabe_blade: false,
            was_zone: Zone::Down,
            zone: Zone::Down,
            pred_zone: Zone::None,
            pred_target: -1,
            gabe_block: -2,
            tgt_block: -2,
            time_left: 0,
            gabe_ang_v: 0.0,
            will_hit: false,
            will_grab: false,
            was_hit: false,
            high: true,
            ego: None,
            due: Vec::new(),
        }
    }
}

/// A platform's model, from 0.
pub fn platform_model(i: i32) -> String {
    format!("TE3_R{:02}", i + 1)
}

/// A platform's number, from 0, by its model's name (`te3_rNN`, seven
/// letters).
fn platform_of(model: &str) -> Option<i32> {
    if model.len() != 7 || !model.to_ascii_uppercase().starts_with("TE3_R") {
        return None;
    }
    Some(atoi(&model[5..]) - 1)
}

/// C's `atoi`: the leading digits, 0 if none.
fn atoi(s: &str) -> i32 {
    let digits: String = s.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().unwrap_or(0)
}

/// The platforms' turn a ms (0x1bc).
fn floor_rate() -> f32 {
    TAU / (CIRCLE_PERIOD * 1000.0)
}

/// The blade's phase a ms (0x1c0): its cycle is six platforms' time.
fn phase_rate() -> f32 {
    let cycle = PENDULUM_CYCLE * CIRCLE_PERIOD * 1000.0 * 0.041_666_668;
    TAU / cycle
}

/// Folds an angle into ±2π, as 0x541c00.
fn fold(mut a: f32) -> f32 {
    while a < -TAU {
        a += TAU;
    }
    while a > TAU {
        a -= TAU;
    }
    a
}

/// The zone a phase is in, and its side, 1 high, 2 low (0x458f70).
fn classify(p: f32) -> (Zone, i32) {
    let between = |(a, b): (f32, f32)| a < p && p < b;
    if p < HIGH_DOWN.1 || HIGH_DOWN.0 < p {
        (Zone::Down, 1)
    } else if between(LOW_UP) {
        (Zone::Up, 2)
    } else if between(LOW_DOWN) {
        (Zone::Down, 2)
    } else if p <= HIGH_UP.0 || HIGH_UP.1 <= p {
        (Zone::None, 0)
    } else {
        (Zone::Up, 1)
    }
}

/// The platform at a side's slot, for the platforms' turn: 29 or 41 less
/// the platforms gone by the door (0x459abc).
fn slot(side: i32, rot: f32) -> i32 {
    let n = (rot * PLATFORMS_PER_RADIAN) as i32;
    match side {
        1 => (29 - n) % PLATFORMS,
        2 => (41 - n) % PLATFORMS,
        _ => -1,
    }
}

/// Turns something about the blade's pivot.
fn about_pivot(turn: Mat4) -> Mat4 {
    let pivot = Vec3::Y * PEND_HEIGHT;
    Mat4::from_translation(pivot) * turn * Mat4::from_translation(-pivot)
}

/// The way out to the middle of a platform, its turn `rot` round.
fn platform_way(i: i32, rot: f32) -> Vec3 {
    let a = PLATFORM_ANGLE * 0.5 + PLATFORM_ANGLE * i as f32 + rot;
    Vec3::new(a.sin(), 0.0, a.cos())
}

/// A jump's turn a ms, by how far into it (0x457eed): slow off, quicker,
/// quickest, blended over a step that spans a change.
fn ramp(before: i32, after: i32, step: i32) -> f32 {
    const SLOW: f32 = 0.32;
    const MID: f32 = 0.8;
    const FAST: f32 = 1.12;
    let (before, after, stepf) = (before as f32, after as f32, step as f32);
    if after <= 66.0 {
        SLOW
    } else if after < 199.0 {
        if before < 66.0 {
            ((after - 66.0) * MID + (66.0 - before) * SLOW) / stepf
        } else {
            MID
        }
    } else if before < 199.0 {
        if before < 66.0 {
            ((after - 198.0) * FAST + (66.0 - before) * SLOW + 132.0 * MID) / stepf
        } else {
            ((after - 198.0) * FAST + (198.0 - before) * MID) / stepf
        }
    } else {
        FAST
    }
}

impl Pendulum {
    /// The scene entered (0x457e4a): set up, and not yet at the altar.
    pub fn start(&mut self, room: &mut dyn Room) {
        self.state = State::Doorway;
        self.init(room);
        room.set_flag("Te3GabeAtAltar", false);
    }

    /// The death screen's `restart` (0x45846c): set up again, unless he is
    /// on the altar.
    pub fn restart(&mut self, room: &mut dyn Room) {
        self.init(room);
    }

    /// 0x459d83: everything back where it began, the blade to start its
    /// cycle at the top of its swing on the high side.
    fn init(&mut self, room: &mut dyn Room) {
        if self.state == State::AtAltar {
            return;
        }
        let now = self.now;
        *self = Self { now, ..Self::default() };
        room.set_world(NORMAL_BLADE, Mat4::IDENTITY);
        for i in 0..PLATFORMS {
            room.set_world(&platform_model(i), Mat4::IDENTITY);
        }
        room.show(GABE_BLADE, false);
        room.show(NORMAL_BLADE, true);
        room.show(KILL_BLADE, false);
        // The door posed open: started and stopped at once.
        room.sample("TE3_DOORCLOSE");
    }

    pub fn state(&self) -> State {
        self.state
    }

    /// The blade's angle now, radians, out to the high side positive.
    #[cfg(test)]
    fn angle(&self) -> f32 {
        self.angle
    }

    fn blade_model(&self) -> &'static str {
        if self.gabe_blade { GABE_BLADE } else { NORMAL_BLADE }
    }

    fn after(&mut self, seconds: f32, message: Message) {
        self.due.push((self.now + seconds as f64 * 1000.0, message));
    }

    /// The scene's update (0x457eed): steps of `updateTime` up to now,
    /// then whatever has ended.
    pub fn tick(&mut self, dt: f32, room: &mut dyn Room) {
        self.now += dt as f64 * 1000.0;
        let now = self.now;
        if self.last_update == 0.0 {
            // The first update: the cycle starts here.
            self.last_update = now.max(f64::MIN_POSITIVE);
            self.start_cycle = self.last_update;
            self.next_update = self.last_update + UPDATE_MS;
            self.blade_world = about_pivot(Mat4::from_rotation_z(self.angle)) * self.blade_world;
            room.set_world(self.blade_model(), self.blade_world);
        } else if self.next_update < now {
            let mut t = self.last_update + UPDATE_MS;
            while t <= now {
                let left = now - t;
                let next = if left >= UPDATE_MS || left == 0.0 { UPDATE_MS } else { left };
                self.step(t, room);
                self.last_update = t;
                self.next_update = t + UPDATE_MS;
                t += next;
            }
            room.set_world(self.blade_model(), self.blade_world);
            for i in 0..PLATFORMS {
                room.set_world(&platform_model(i), self.floor_world);
            }
            if let Some((at, heading)) = self.ego {
                room.put_ego(at, heading);
            }
        }
        while let Some(i) = self.due.iter().position(|(at, _)| *at <= self.now) {
            let (_, message) = self.due.remove(i);
            self.message(message, room);
        }
    }

    /// One step, to `t`.
    fn step(&mut self, t: f64, room: &mut dyn Room) {
        let dt = (t - self.last_update) as f32;
        let turn = dt * floor_rate();
        self.floor_rot += turn;
        self.pred_rot += turn;
        if self.floor_rot > TAU {
            self.floor_rot -= TAU;
        }
        if self.pred_rot > TAU {
            self.pred_rot -= TAU;
        }
        self.floor_world = Mat4::from_rotation_y(turn) * self.floor_world;
        // Gabriel goes round with his platform, and over to the next in a
        // jump.
        if matches!(self.state, State::OnPlatform | State::LandingOn | State::Jumping) {
            let mut his = turn;
            if self.time_left != 0 {
                let step = (dt as i32).min(self.time_left);
                let before = JUMP_MS - self.time_left;
                his += step as f32 * self.gabe_ang_v * ramp(before, before + step, step);
                self.time_left -= step;
                if self.time_left == 0 {
                    self.gabe_block = self.tgt_block;
                    if self.will_hit {
                        self.hit(room);
                    } else if self.will_grab {
                        self.grab(room);
                    }
                }
            }
            if let Some((at, heading)) = &mut self.ego {
                *at = Mat4::from_rotation_y(his).transform_point3(*at);
                *heading += his;
            }
        }
        self.swing_blade(t, room);
    }

    /// The blade's step (0x459abc), and on a platform, whether it comes for
    /// him.
    fn swing_blade(&mut self, t: f64, room: &mut dyn Room) {
        if !self.pend_on {
            return;
        }
        let phase = fold(((t - self.start_cycle) as f32) * phase_rate());
        let angle = phase.cos() * self.swing;
        self.speed = angle - self.angle;
        self.angle = angle;
        // While he dies, the blade is left where it struck.
        if self.state == State::Dying {
            return;
        }
        self.blade_world = about_pivot(Mat4::from_rotation_z(self.speed)) * self.blade_world;
        if self.state != State::OnPlatform {
            return;
        }
        self.was_zone = self.zone;
        let (zone, side) = classify(phase);
        self.zone = zone;
        let target = slot(side, self.floor_rot);
        // Coming up, the blade strikes the platform in the slot; going down,
        // the one it has passed into it.
        if zone == Zone::Down {
            if self.was_zone != Zone::Down {
                self.check_hit((target + 23) % PLATFORMS, room);
            }
        } else if zone == Zone::Up && self.was_zone != Zone::Up {
            self.check_hit(target, room);
        }
        let (zone, side) = classify(fold(phase_rate() * 1000.0 + phase));
        self.pred_zone = zone;
        self.pred_target = slot(side, self.pred_rot);
    }

    /// 0x458f26: the blade reaches his platform.
    fn check_hit(&mut self, target: i32, room: &mut dyn Room) {
        if !self.was_hit && self.gabe_block == self.tgt_block && self.gabe_block == target {
            self.hit(room);
        }
    }

    fn kill_camera(&self) -> &'static str {
        if self.angle <= 0.0 { "KILL_LOW" } else { "KILL_HIGH" }
    }

    /// 0x45858e: struck. The blade stuck in the slot stands in while he
    /// falls, forwards if it came up to him.
    fn hit(&mut self, room: &mut dyn Room) {
        room.cut(self.kill_camera());
        self.time_left = 0;
        self.will_hit = false;
        self.will_grab = false;
        self.was_hit = true;
        self.state = State::Dying;
        room.forced_cuts(true);
        room.show(self.blade_model(), false);
        room.show(KILL_BLADE, true);
        let anim = match (self.angle <= 0.0, self.zone == Zone::Up) {
            (true, true) => "GabTE3PnFtL",
            (true, false) => "GabTE3PnBkL",
            (false, true) => "GabTE3PnFtH",
            (false, false) => "GabTE3PnBkH",
        };
        room.script("StartDialogue(\"1regj67q81\", 1);");
        let d = room.play(anim, false);
        self.after(d, Message::Killed);
    }

    /// 0x45871d: on the blade. Each catch takes from its swing; the blade
    /// he holds is turned to face the way it swings from.
    fn grab(&mut self, room: &mut dyn Room) {
        room.cut(self.kill_camera());
        self.time_left = 0;
        self.will_hit = false;
        self.will_grab = false;
        self.state = State::OnBlade;
        self.gabe_block = -3;
        self.tgt_block = -3;
        self.swing -= GRAB_LOSS;
        room.show(NORMAL_BLADE, false);
        room.show(GABE_BLADE, true);
        room.show("GAB", false);
        room.show("GABMIC", false);
        room.set_flag("Te3FirstPendGrab", true);
        room.score("e_temple_grab_pendulum");
        self.gabe_blade = true;
        if (self.angle < 0.0 && self.high) || (self.angle > 0.0 && !self.high) {
            let face = Mat4::from_rotation_z(self.angle) * Mat4::from_rotation_y(PI) * Mat4::from_rotation_z(-self.angle);
            self.blade_world = about_pivot(face) * self.blade_world;
            self.high = !self.high;
        }
        room.set_world(GABE_BLADE, self.blade_world);
        self.ego = None;
        self.after(2.0, Message::LongAltar);
    }

    /// 0x45898f: off the blade, onto the altar or past it.
    fn land(&mut self, room: &mut dyn Room) {
        room.show(GABE_BLADE, false);
        room.show(NORMAL_BLADE, true);
        room.show("GAB", true);
        room.show("GABMIC", true);
        self.gabe_blade = false;
        room.set_world(NORMAL_BLADE, self.blade_world);
        let degrees = self.angle * DEGREES;
        // Under the blade's end, at the height he falls from.
        let under = self.blade_world.w_axis.truncate();
        let height = room.spot("GABE_FALL_REFERENCE").map_or(0.0, |(at, _)| at.y);
        let heading = if self.speed > 0.0 { FALL_HEADING } else { -FALL_HEADING };
        room.put_ego(Vec3::new(under.x, height, under.z), heading);
        if !(-LAND_DIST..=LAND_DIST).contains(&degrees) {
            room.cut("FALL_DOWN");
            self.state = State::Falling;
            let d = room.play("GabeFallDeath", false);
            self.after(d, Message::Die);
            return;
        }
        if let Some((at, heading)) = room.spot("ALTAR_STAND") {
            room.put_ego(at, heading.to_radians());
        }
        room.cut("ALTAR_UP");
        self.pend_on = false;
        self.state = State::AtAltar;
        self.time_left = 0;
        self.swing = 0.0;
        room.set_flag("Te3FirstPendDrop", true);
        room.score("e_temple_drop_platform");
        room.set_flag("Te3GabeAtAltar", true);
        room.script("wait CallSheep(\"te3\", \"GabeOnPillar$\");");
    }

    /// 0x458c48.
    fn message(&mut self, message: Message, room: &mut dyn Room) {
        match message {
            Message::Killed => {
                room.show(self.blade_model(), true);
                room.show(KILL_BLADE, false);
                room.forced_cuts(false);
                room.cut(if self.angle <= 0.0 { "AFTERKILL_LOW" } else { "AFTERKILL_HIGH" });
                self.after(2.0, Message::Die);
            }
            Message::Die => room.script("wait CallSheep(\"te3\", \"die$\");"),
            Message::LongAltar => {
                room.forced_cuts(true);
                room.cut("LONG_ALTAR");
            }
            Message::JumpedOn => self.jumped_on(room),
            Message::Landed => self.state = State::OnPlatform,
            Message::Leapt { left } => {
                let d = room.play(if left { "GabTE3JumpLM" } else { "GabTE3JumpRM" }, false);
                self.after(d, Message::Over { left });
                self.time_left = JUMP_MS;
                self.gabe_ang_v /= JUMP_MS as f32;
            }
            Message::Over { left } => {
                let d = room.play(if left { "GabTE3JumpLE" } else { "GabTE3JumpRE" }, false);
                self.after(d, Message::Landed);
            }
            Message::Dropped => {
                room.forced_cuts(false);
                self.land(room);
            }
        }
    }

    /// 0x459117: in the air from the doorway, he comes down on the
    /// platform before it, facing the middle.
    fn jumped_on(&mut self, room: &mut dyn Room) {
        let on = PLATFORMS - 1 - (self.floor_rot * PLATFORMS_PER_RADIAN) as i32;
        let d = room.play("GabTE3JumpFE", false);
        self.after(d, Message::Landed);
        room.play("te3_doorclose", false);
        room.forced_cuts(false);
        self.state = State::LandingOn;
        self.gabe_block = on;
        self.tgt_block = on;
        let at = platform_way(on, self.floor_rot) * LANDING_RADIUS + Vec3::Y * room.ego_at().y;
        let heading = at.x.atan2(at.z) + PI;
        self.ego = Some((at, heading));
        room.put_ego(at, heading);
        room.cut("JUMP_OUT");
    }

    /// Whether the blade can be caught from his platform: a second from
    /// now it comes up to the slot past his, or goes down from his.
    fn grabbable(&self) -> bool {
        let next = (self.gabe_block + 1) % PLATFORMS;
        (self.pred_zone == Zone::Up && next == self.pred_target)
            || (self.pred_target == self.gabe_block && self.pred_zone != Zone::Up)
    }

    /// The cursor something under the pointer shows here, if the puzzle
    /// has a say (slot 0x114).
    pub fn cursor(&self, model: &str) -> Option<&'static str> {
        let upper = model.to_ascii_uppercase();
        let next = (self.gabe_block + 1) % PLATFORMS;
        let lit = match self.state {
            State::Doorway => platform_of(model).is_some(),
            State::OnPlatform => {
                (upper.starts_with("TE3_PEND") && (self.pred_target == self.gabe_block || next == self.pred_target))
                    || platform_of(model).is_some_and(|i| i == next || i == (self.gabe_block + 23) % PLATFORMS)
            }
            // A step ahead, by the blade's last (in degrees and radians
            // both, as the original).
            State::OnBlade => {
                let ahead = self.angle * DEGREES + self.speed;
                upper == ALTAR && self.gabe_block == -3 && self.drop_window(ahead)
            }
            _ => false,
        };
        lit.then_some(CURSOR)
    }

    /// Whether the blade, `degrees` out, is coming in to the middle within
    /// the drop's window.
    fn drop_window(&self, degrees: f32) -> bool {
        (-START_SAFE < degrees && degrees < -END_SAFE && self.speed > 0.0)
            || (END_SAFE < degrees && degrees < START_SAFE && self.speed < 0.0)
    }

    /// A click on something (slot 0x110). True if the puzzle takes it.
    pub fn click(&mut self, model: &str, room: &mut dyn Room) -> bool {
        match self.state {
            State::Doorway => self.click_in_doorway(model, room),
            State::OnPlatform => self.click_on_platform(model, room),
            State::OnBlade => self.click_on_blade(model, room),
            _ => false,
        }
    }

    /// 0x45906d: any platform, and he jumps on.
    fn click_in_doorway(&mut self, model: &str, room: &mut dyn Room) -> bool {
        if platform_of(model).is_none() {
            return false;
        }
        self.state = State::JumpingOn;
        let d = room.play("GabTE3JumpFS", false);
        self.after(d, Message::JumpedOn);
        true
    }

    /// 0x4593d6.
    fn click_on_platform(&mut self, model: &str, room: &mut dyn Room) -> bool {
        let upper = model.to_ascii_uppercase();
        let gb = self.gabe_block;
        if upper.starts_with("TE3_PEND") && self.grabbable() {
            room.play("GabJmpPndulm", false);
            self.grab(room);
            return true;
        }
        // The blade about to go down from the slot he has just passed:
        // nothing.
        if (gb + 23) % PLATFORMS == self.pred_target {
            return true;
        }
        if let Some(i) = platform_of(model) {
            if self.time_left != 0 || (i != (gb + 1) % PLATFORMS && i != (gb + 23) % PLATFORMS) {
                return false;
            }
            if self.pred_zone == Zone::None || self.pred_target != self.tgt_block {
                self.will_hit = false;
                self.will_grab = false;
                // Round to the next platform is to his left.
                let left = (i == 0 && gb == 23) || (gb <= i && !(i == 23 && gb == 0));
                let (anim, turn) = if left { ("GabTE3JumpLS", JUMP_TURN) } else { ("GabTE3JumpRS", -JUMP_TURN) };
                let d = room.play(anim, false);
                self.after(d, Message::Leapt { left });
                self.gabe_ang_v = turn;
                self.state = State::Jumping;
                self.tgt_block = i;
            } else {
                // Too late: the blade is coming.
                room.cut(self.kill_camera());
                if self.pred_zone == Zone::Down && self.tgt_block == (gb + 1) % PLATFORMS {
                    self.will_grab = true;
                } else {
                    self.will_hit = true;
                }
            }
            return true;
        }
        if upper == NORMAL_BLADE {
            if self.pred_zone != Zone::Up && (self.pred_target == gb || (gb + 1) % PLATFORMS == self.pred_target) {
                room.play("GabJmpPndulm", false);
                self.grab(room);
            }
            return true;
        }
        if ROOM_SHELL.contains(&upper.as_str()) {
            return false;
        }
        room.script(NOT_NOW);
        true
    }

    /// 0x459736: the altar, let go of in the window.
    fn click_on_blade(&mut self, model: &str, room: &mut dyn Room) -> bool {
        let upper = model.to_ascii_uppercase();
        if upper == ALTAR {
            if self.drop_window(self.angle * DEGREES) {
                room.cut("LONG_ALTAR");
                self.state = State::Dropping;
                let d = room.play("GabJmpOffPen", false);
                self.after(d, Message::Dropped);
            }
            return true;
        }
        if ROOM_SHELL.contains(&upper.as_str()) {
            return false;
        }
        room.script(NOT_NOW);
        true
    }

    /// The blade's phase `ms` on from the last step.
    fn phase_in(&self, ms: f32) -> f32 {
        fold(((self.last_update - self.start_cycle) as f32 + ms) * phase_rate())
    }

    /// Whether the blade, `ms` on, is where it would strike platform `gb`.
    fn struck(&self, gb: i32, ms: f32) -> bool {
        let (zone, side) = classify(self.phase_in(ms));
        self.zone_over(gb, zone, side, ms)
    }

    /// Whether the blade enters a zone over platform `gb` in the step to
    /// `ms` on: only then does it strike (0x459abc).
    fn strikes(&self, gb: i32, ms: f32) -> bool {
        let (zone, side) = classify(self.phase_in(ms));
        zone != classify(self.phase_in(ms - UPDATE_MS as f32)).0 && self.zone_over(gb, zone, side, ms)
    }

    fn zone_over(&self, gb: i32, zone: Zone, side: i32, ms: f32) -> bool {
        let target = slot(side, (self.floor_rot + ms * floor_rate()).rem_euclid(TAU));
        match zone {
            Zone::Up => target == gb,
            Zone::Down => (target + 23) % PLATFORMS == gb,
            Zone::None => false,
        }
    }

    /// For headless runs: what to click now to work towards a goal, as a
    /// player would. `"GRAB"`: on, then catch the blade, stepping back a
    /// platform before a slot comes for him. `"DROP"`: let go so as to
    /// land at the middle, the drop's 0.8 seconds on. `"DIE"`: onto a
    /// platform the blade will strike.
    pub fn advice(&self, goal: &str) -> Option<String> {
        if !self.due.is_empty() || self.time_left != 0 {
            return None;
        }
        match (goal, self.state) {
            ("GRAB" | "DIE", State::Doorway) => Some(platform_model(0)),
            ("GRAB", State::OnPlatform) if self.grabbable() => Some(NORMAL_BLADE.into()),
            // Off a platform the blade never reaches (a third of them: the
            // platforms and the swing keep time every fifty seconds).
            ("DIE", State::OnPlatform) => {
                let gb = self.gabe_block;
                let never = (1..2500).all(|k| !self.strikes(gb, k as f32 * UPDATE_MS as f32));
                never.then(|| platform_model((gb + 1) % PLATFORMS))
            }
            ("GRAB", State::OnPlatform) => {
                let gb = self.gabe_block;
                let at = |ms: f32| {
                    let (zone, side) = classify(self.phase_in(ms));
                    (zone, slot(side, (self.floor_rot + ms * floor_rate()).rem_euclid(TAU)))
                };
                let struck = |ms: f32| self.struck(gb, ms);
                let catchable = |ms: f32| match at(ms + 1000.0) {
                    (Zone::Up, target) => target == (gb + 1) % PLATFORMS,
                    (_, target) => target == gb,
                };
                // Back a platform if struck within four seconds standing
                // here, unless the blade can be caught first.
                let first = |f: &dyn Fn(f32) -> bool| (0..80).find(|k| f(*k as f32 * 50.0));
                let back = (gb + 23) % PLATFORMS;
                let refused = (self.pred_zone != Zone::None && self.pred_target == self.tgt_block) || back == self.pred_target;
                match (first(&struck), first(&catchable)) {
                    (Some(s), c) if c.is_none_or(|c| c > s) && !refused => Some(platform_model(back)),
                    _ => None,
                }
            }
            ("DROP", State::OnBlade) => {
                let landing = self.phase_in(800.0).cos() * self.swing * DEGREES;
                (self.drop_window(self.angle * DEGREES) && landing.abs() < 1.5).then(|| ALTAR.into())
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A room that records what is asked of it; every animation lasts a
    /// second but the drop.
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
                (false, "GabJmpOffPen") => 0.8,
                _ => 1.0,
            }
        }
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

    const FRAME: f32 = 1.0 / 30.0;

    /// Runs on until something holds; how long it took.
    fn run_until(p: &mut Pendulum, room: &mut Fake, mut until: impl FnMut(&Pendulum) -> bool) -> Option<f32> {
        let mut t = 0.0;
        while t < 120.0 {
            if until(p) {
                return Some(t);
            }
            p.tick(FRAME, room);
            t += FRAME;
        }
        None
    }

    #[test]
    fn the_blade_swings_in_twelve_and_a_half_seconds() {
        let mut room = Fake::default();
        let mut p = Pendulum::default();
        p.start(&mut room);
        p.tick(0.001, &mut room);
        p.tick(0.1, &mut room);
        let max = PENDULUM_ANGLE.to_radians();
        assert!((p.angle() - max).abs() < 0.01, "starts high: {}", p.angle());
        let mut low: f32 = 0.0;
        for _ in 0..313 {
            p.tick(0.02, &mut room);
            low = low.min(p.angle());
        }
        assert!((low + max).abs() < 0.001, "half a cycle on, low: {low}");
    }

    #[test]
    fn zones_by_phase() {
        assert_eq!(classify(0.0), (Zone::Down, 1));
        assert_eq!(classify(6.0), (Zone::Up, 1));
        assert_eq!(classify(3.0), (Zone::Up, 2));
        assert_eq!(classify(3.2), (Zone::Down, 2));
        assert_eq!(classify(1.5), (Zone::None, 0));
        // At the start, the slots are the sixth platform either side of
        // the door's.
        assert_eq!((slot(1, 0.0), slot(2, 0.0)), (5, 17));
        assert_eq!(platform_of("te3_r24"), Some(23));
        assert_eq!(platform_of("te3_r245"), None);
    }

    #[test]
    fn a_jump_turns_him_one_platform() {
        // Fifteen degrees, give or take the ramp's rounding.
        let total: f32 = (0..40).map(|k| ramp(k * 20, k * 20 + 20, 20) * 20.0).sum();
        assert!((total - 800.0).abs() < 1.0, "{total}");
    }

    #[test]
    fn standing_still_the_blade_finds_him() {
        let mut room = Fake::default();
        let mut p = Pendulum::default();
        p.start(&mut room);
        assert_eq!(p.cursor("te3_r05"), Some(CURSOR));
        assert!(p.click("te3_r05", &mut room));
        run_until(&mut p, &mut room, |p| p.state == State::OnPlatform).unwrap();
        assert_eq!(p.gabe_block, 23);
        run_until(&mut p, &mut room, |p| p.state == State::Dying).expect("struck");
        assert!(room.cuts.iter().any(|c| c.starts_with("KILL_")));
        run_until(&mut p, &mut room, |_| false);
        assert!(room.scripts.iter().any(|s| s.contains("die$")));
        p.restart(&mut room);
        assert_eq!((p.state, p.gabe_block), (State::Doorway, -2));
    }

    #[test]
    fn caught_and_dropped_at_the_middle_he_reaches_the_altar() {
        let mut room = Fake::default();
        let mut p = Pendulum::default();
        p.start(&mut room);
        let mut t = 0.0;
        while p.state != State::OnBlade && t < 300.0 {
            if let Some(model) = p.advice("GRAB") {
                p.click(&model, &mut room);
            }
            p.tick(FRAME, &mut room);
            t += FRAME;
            assert_ne!(p.state, State::Dying, "he should have dodged");
        }
        assert_eq!(p.state, State::OnBlade, "not caught in {t} s");
        assert!((p.swing - (PENDULUM_ANGLE - 2.0).to_radians()).abs() < 1e-4);
        while p.state == State::OnBlade && t < 400.0 {
            if let Some(model) = p.advice("DROP") {
                p.click(&model, &mut room);
            }
            p.tick(FRAME, &mut room);
            t += FRAME;
        }
        run_until(&mut p, &mut room, |p| p.state != State::Dropping).unwrap();
        assert_eq!(p.state, State::AtAltar);
        assert!(room.flags.contains(&("Te3GabeAtAltar".into(), true)));
        assert!(room.scripts.iter().any(|s| s.contains("GabeOnPillar$")));
        // On the altar, a restart leaves him there.
        p.restart(&mut room);
        assert_eq!(p.state, State::AtAltar);
    }
}
