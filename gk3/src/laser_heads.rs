//! CS2's laser heads: five busts round the Chateau de Serres library, each
//! turning to one of five angles and casting a laser. Aimed right, they
//! draw a pentagram and the stairs down open. The original keeps this in
//! its code; this follows G-Engine's `LaserHead` (GPL-3.0), whose numbers
//! the original writes to its log.
//!
//! The scripts read each head's turn from `Cs2Head1`..`Cs2Head5` (0 to 4,
//! the middle 2 to start) and wait while Grace turns it.

use glam::{Mat4, Vec3};

/// Where the lasers cross.
const CENTER: Vec3 = Vec3::new(4.02, 57.0, 18.90);
/// The heads stand on a circle this far out, at this height; the lasers
/// start a little nearer, at the centre's height.
const HEAD_RADIUS: f32 = 129.5;
const HEAD_HEIGHT: f32 = 52.0;
const LASER_RADIUS: f32 = 128.0;
/// Each head's five headings, in degrees.
const ANGLES: [[f32; 5]; 5] = [
    [160.57, -163.08, -145.32, -126.91, -90.95],
    [-126.72, -90.69, -72.84, -54.56, -18.47],
    [-54.56, -18.47, -0.70, 16.62, 53.28],
    [17.77, 53.09, 71.40, 89.31, 125.44],
    [89.05, 125.27, 143.58, 161.53, -162.23],
];
/// A turn takes two seconds, starting a moment after Grace's hand does.
const TURN_SECONDS: f32 = 2.0;
const TURN_DELAY: f32 = 0.8;

#[derive(Debug, Clone)]
pub struct LaserHeads {
    turns: [usize; 5],
    /// Each head's facing now, and what a turn under way began from and
    /// how far it has got (seconds; negative before it starts).
    facing: [Vec3; 5],
    from: [Vec3; 5],
    timer: [f32; 5],
    pub lasers_on: bool,
}

impl Default for LaserHeads {
    fn default() -> Self {
        let facing: [Vec3; 5] = std::array::from_fn(|i| direction(ANGLES[i][2]));
        Self {
            turns: [2; 5],
            facing,
            from: facing,
            timer: [TURN_SECONDS; 5],
            lasers_on: false,
        }
    }
}

fn direction(degrees: f32) -> Vec3 {
    let r = degrees.to_radians();
    Vec3::new(r.sin(), 0.0, r.cos())
}

/// The game variable for a head's turn, from 0.
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

impl LaserHeads {
    /// Every head's turn, for the game variables.
    pub fn turns(&self) -> [usize; 5] {
        self.turns
    }

    /// Turns a head one step left (`1`) or right (`-1`). Returns the
    /// animation of Grace turning it, or None if it turns no further.
    pub fn turn(&mut self, head: usize, step: isize) -> Option<String> {
        let to = self.turns.get(head)?.checked_add_signed(step).filter(|t| *t <= 4)?;
        self.turns[head] = to;
        self.from[head] = self.facing[head];
        self.timer[head] = -TURN_DELAY;
        let side = if step > 0 { "L" } else { "R" };
        Some(format!("GraCs2TrnHead{side}{to}"))
    }

    /// Moves turns under way on.
    pub fn tick(&mut self, dt: f32) {
        for i in 0..5 {
            if self.timer[i] >= TURN_SECONDS {
                continue;
            }
            self.timer[i] += dt;
            let t = (self.timer[i] / TURN_SECONDS).clamp(0.0, 1.0);
            let to = direction(ANGLES[i][self.turns[i]]);
            self.facing[i] = self.from[i].lerp(to, t).normalize_or(to);
        }
    }

    fn heading(&self, head: usize) -> f32 {
        let f = self.facing[head];
        f.x.atan2(f.z)
    }

    /// Where a head stands: on its circle, opposite the way it faces.
    pub fn head_world(&self, head: usize) -> Mat4 {
        let mut at = CENTER - self.facing[head] * HEAD_RADIUS;
        at.y = HEAD_HEIGHT;
        Mat4::from_translation(at) * Mat4::from_rotation_y(self.heading(head))
    }

    /// A head's laser: from just before it, across the ring until it
    /// meets the far side (where the busts stand). The beam model is 100
    /// units long and is drawn a fifth as thick.
    pub fn laser_world(&self, head: usize) -> Mat4 {
        let d = self.facing[head];
        let mut at = CENTER - d * LASER_RADIUS;
        at.y = CENTER.y;
        // Along the beam to the far side of the head ring.
        let flat = Vec3::new(at.x - CENTER.x, 0.0, at.z - CENTER.z);
        let across = (-2.0 * flat.dot(d)).max(10.0);
        Mat4::from_translation(at)
            * Mat4::from_rotation_y(self.heading(head))
            * Mat4::from_scale(Vec3::new(0.2, 0.2, across / 100.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heads_turn_within_their_five_angles() {
        let mut heads = LaserHeads::default();
        assert_eq!(heads.turn(0, 1).as_deref(), Some("GraCs2TrnHeadL3"));
        assert_eq!(heads.turn(0, 1).as_deref(), Some("GraCs2TrnHeadL4"));
        assert_eq!(heads.turn(0, 1), None, "no further than the last angle");
        assert_eq!(heads.turn(0, -1).as_deref(), Some("GraCs2TrnHeadR3"));
        assert_eq!(heads.turns()[0], 3);
        assert_eq!(variable(0), "CS2HEAD1");
    }

    #[test]
    fn a_turn_eases_round_after_a_moment() {
        let mut heads = LaserHeads::default();
        let before = heads.facing[1];
        heads.turn(1, 1);
        heads.tick(0.5);
        assert_eq!(heads.facing[1], before, "still waiting for Grace's hand");
        heads.tick(TURN_SECONDS + TURN_DELAY);
        assert!((heads.facing[1] - direction(ANGLES[1][3])).length() < 1e-4);
    }

    #[test]
    fn heads_face_the_middle_from_their_ring() {
        let heads = LaserHeads::default();
        for i in 0..5 {
            let at = heads.head_world(i).transform_point3(Vec3::ZERO);
            let flat = Vec3::new(at.x - CENTER.x, 0.0, at.z - CENTER.z);
            assert!((flat.length() - HEAD_RADIUS).abs() < 0.1);
            // Facing (more or less) inwards.
            assert!(flat.normalize().dot(heads.facing[i]) < -0.99);
        }
    }
}
