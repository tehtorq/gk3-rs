//! CS2's laser heads: five busts round the Château de Serres library, each
//! turning to aim at one of the others or the middle, and casting a laser
//! when the lasers are on. All aimed two along the ring (a pentagram, one
//! way or the other), the stairs down open (`StaircaseOpen`). The
//! original's `Laser` scene (`custom=Laser`, 0x4c25d1; `docs/re-lasers.md`).
//!
//! The scripts turn a head with `wait CallSceneFunction("turnL1")` (or
//! `turnR1`, for heads 1 to 5) and read each head's setting from
//! `Cs2Head1`..`Cs2Head5` (0 to 4, 2 the middle, to start).

use std::f32::consts::TAU;

use glam::{Mat4, Vec3};

/// Where the heads stand (0x6eef6c).
const HEADS: [Vec3; 5] = [
    Vec3::new(77.026, 51.944, 124.426),
    Vec3::new(127.718, 51.944, -19.289),
    Vec3::new(5.607, 51.944, -110.376),
    Vec3::new(-118.223, 51.944, -22.235),
    Vec3::new(-72.012, 51.944, 121.967),
];
/// The height they aim from, and the lasers start at.
const EYE_HEIGHT: f32 = 57.0;
/// A turn starts this long after it is asked for (0x1c2 ms).
const TURN_DELAY: f64 = 0.45;
/// And takes this long, whichever way and however far (20 × 66.67 ms).
const TURN_SECONDS: f32 = 20.0 * 66.666_664 / 1000.0;
/// A laser starts this far out from its head, and reaches at most this far.
const LASER_START: f32 = 3.0;
pub const LASER_REACH: f32 = 1000.0;

/// A head turning (`currHead`, `currHeading`, `tgtHeading`, `turnRate`).
#[derive(Debug, Clone, Copy)]
struct Turning {
    head: usize,
    current: f32,
    target: f32,
    /// Radians a second, negative to the right.
    rate: f32,
}

/// What a frame brought about.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Event {
    /// A head began turning to a new setting, which the game variables now
    /// hold.
    Setting { head: usize, setting: usize },
    /// A turn came to its end: whether every head now aims two along the
    /// ring the same way.
    Turned { solved: bool },
}

#[derive(Debug, Clone)]
pub struct LaserHeads {
    /// Each head's five headings, radians: at the next head, the one after,
    /// the middle, and the two after that.
    headings: [[f32; 5]; 5],
    settings: [usize; 5],
    turning: Option<Turning>,
    /// A turn asked for: when it starts, which head, and whether left.
    asked: Option<(f64, usize, bool)>,
    /// When Grace's turning animation ends, the head is put on its new
    /// heading if it is not there yet (0x4c2f55).
    anim_end: Option<f64>,
    pub lasers_on: bool,
}

/// The way from one point to another, as a heading (x then z).
fn heading_to(from: Vec3, to: Vec3) -> f32 {
    (to.x - from.x).atan2(to.z - from.z)
}

fn eye(head: usize) -> Vec3 {
    Vec3::new(HEADS[head].x, EYE_HEIGHT, HEADS[head].z)
}

fn wrapped(a: f32) -> f32 {
    a.rem_euclid(TAU)
}

impl LaserHeads {
    /// The heads, at their settings (each 0 to 4); the middle for any not
    /// yet set (0x4c2912).
    pub fn new(settings: [Option<usize>; 5]) -> Self {
        let sum = HEADS.iter().fold(Vec3::ZERO, |s, h| s + Vec3::new(h.x, EYE_HEIGHT, h.z));
        let middle = sum * 0.2;
        let headings = std::array::from_fn(|i| {
            let targets = [
                eye((i + 1) % 5),
                eye((i + 2) % 5),
                middle,
                eye((i + 3) % 5),
                eye((i + 4) % 5),
            ];
            targets.map(|t| heading_to(eye(i), t))
        });
        Self {
            headings,
            settings: settings.map(|s| s.filter(|s| *s < 5).unwrap_or(2)),
            turning: None,
            asked: None,
            anim_end: None,
            lasers_on: false,
        }
    }

    pub fn settings(&self) -> [usize; 5] {
        self.settings
    }

    /// `turnL1` and the like: a turn of a head (from 0) asked for, left or
    /// right (0x4c3197). Returns the animation of Grace turning it, named
    /// for the setting it goes to.
    pub fn ask(&mut self, head: usize, left: bool, now: f64) -> Option<String> {
        let setting = *self.settings.get(head)?;
        self.asked = Some((now + TURN_DELAY, head, left));
        let to = (setting + if left { 1 } else { 4 }) % 5;
        Some(format!("GraCs2TrnHead{}{to}", if left { 'L' } else { 'R' }))
    }

    /// When Grace's animation for the turn ends.
    pub fn anim_ends(&mut self, at: f64) {
        self.anim_end = Some(at);
    }

    /// Moves turns on to `now`, `dt` seconds since the last.
    pub fn tick(&mut self, now: f64, dt: f32) -> Vec<Event> {
        let mut events = Vec::new();
        // The turn starts (0x4c32b3): from its heading the long way round
        // if need be, at a rate that takes it there in the turn's time.
        if let Some((at, head, left)) = self.asked
            && now >= at
        {
            self.asked = None;
            let current = wrapped(self.headings[head][self.settings[head]]);
            let setting = (self.settings[head] + if left { 1 } else { 4 }) % 5;
            self.settings[head] = setting;
            let target = wrapped(self.headings[head][setting]);
            // Left the heading rises, right it falls (0x4c3434).
            let delta = match (left, target <= current) {
                (true, true) => TAU - current + target,
                (false, false) => target - TAU - current,
                _ => target - current,
            };
            self.turning = Some(Turning {
                head,
                current,
                target,
                rate: delta / TURN_SECONDS,
            });
            events.push(Event::Setting { head, setting });
        }
        // 0x4c2d6a: a turn's first frame does not move it (`lastUpdate` 0).
        let started = !events.is_empty();
        if let Some(t) = &mut self.turning
            && !started
        {
            let step = t.rate * dt;
            if (t.current - t.target).abs() <= step.abs() {
                self.turning = None;
                let s = self.settings;
                let solved = (s[0] == 1 || s[0] == 3) && s.iter().all(|x| *x == s[0]);
                events.push(Event::Turned { solved });
            } else {
                t.current = wrapped(t.current + step);
            }
        }
        // Grace's hand done, the head is put straight.
        if self.anim_end.is_some_and(|at| now >= at) {
            self.anim_end = None;
            self.turning = None;
        }
        events
    }

    /// A head's heading now.
    pub fn heading(&self, head: usize) -> f32 {
        match self.turning {
            Some(t) if t.head == head => t.current,
            _ => self.headings[head][self.settings[head]],
        }
    }

    /// Where a head is drawn: on its spot, turned to its heading.
    pub fn head_world(&self, head: usize) -> Mat4 {
        Mat4::from_translation(HEADS[head]) * Mat4::from_rotation_y(self.heading(head))
    }

    /// Where a head's laser starts and the way it goes.
    pub fn laser_ray(&self, head: usize) -> (Vec3, Vec3) {
        let h = self.heading(head);
        (eye(head), Vec3::new(h.sin(), 0.0, h.cos()))
    }

    /// A head's laser, reaching `reach` from the head (0x4c349c): the beam
    /// model is 100 units long, drawn a fifth as thick.
    pub fn laser_world(&self, head: usize, reach: f32) -> Mat4 {
        let (from, d) = self.laser_ray(head);
        Mat4::from_translation(from + d * LASER_START)
            * Mat4::from_rotation_y(self.heading(head))
            * Mat4::from_scale(Vec3::new(0.2, 0.2, (reach - LASER_START) * 0.01))
    }
}

/// The game variable for a head's setting, from 0.
pub fn variable(head: usize) -> String {
    format!("CS2HEAD{}", head + 1)
}

/// The head models and their lasers, from 0.
pub fn head_model(head: usize) -> String {
    format!("CS2HEAD0{}", head + 1)
}

pub fn laser_model(head: usize) -> String {
    format!("CS2LASER_0{}", head + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_head_aims_at_the_others_and_the_middle() {
        let heads = LaserHeads::new([None; 5]);
        assert_eq!(heads.settings(), [2; 5]);
        // Setting 0 aims along to the next head.
        let (from, d) = {
            let mut h = heads.clone();
            h.settings[0] = 0;
            h.laser_ray(0)
        };
        let to = eye(1) - from;
        assert!(d.dot(to.normalize()) > 0.9999);
    }

    #[test]
    fn a_turn_starts_late_and_takes_its_time() {
        let mut heads = LaserHeads::new([None; 5]);
        assert_eq!(heads.ask(0, true, 10.0).as_deref(), Some("GraCs2TrnHeadL3"));
        assert!(heads.tick(10.2, 0.2).is_empty(), "not yet");
        let start = heads.tick(10.5, 0.3);
        assert_eq!(start[0], Event::Setting { head: 0, setting: 3 });
        let mut t = 10.5;
        let mut turned = None;
        while turned.is_none() && t < 20.0 {
            t += 1.0 / 60.0;
            turned = heads
                .tick(t, 1.0 / 60.0)
                .into_iter()
                .find(|e| matches!(e, Event::Turned { .. }));
        }
        assert!((t - 10.5 - TURN_SECONDS as f64).abs() < 0.05, "took {}", t - 10.5);
        assert_eq!(turned, Some(Event::Turned { solved: false }));
        assert_eq!(heads.ask(0, false, 30.0).as_deref(), Some("GraCs2TrnHeadR2"));
    }

    #[test]
    fn all_two_along_is_solved() {
        let mut heads = LaserHeads::new([Some(1), Some(1), Some(1), Some(1), Some(2)]);
        heads.ask(4, true, 0.0);
        let mut events = Vec::new();
        let mut t = 0.0;
        while t < 3.0 {
            t += 0.05;
            events.extend(heads.tick(t, 0.05));
        }
        assert!(events.contains(&Event::Turned { solved: false }));
        // Two along from the middle is setting 1 the other way: right.
        let mut heads = LaserHeads::new([Some(1), Some(1), Some(1), Some(1), Some(2)]);
        heads.ask(4, false, 0.0);
        let mut events = Vec::new();
        let mut t = 0.0;
        while t < 3.0 {
            t += 0.05;
            events.extend(heads.tick(t, 0.05));
        }
        assert!(events.contains(&Event::Turned { solved: true }));
    }
}
