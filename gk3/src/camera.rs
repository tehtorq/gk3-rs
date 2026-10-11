//! The player's camera, moved as G-Engine's `GameCamera` moves it: W/S or
//! Up/Down go forward and back over the floor, A/D or Left/Right turn.
//! Holding Shift makes those pitch and turn, Ctrl makes them rise, sink and
//! strafe, Alt doubles every speed. Dragging with the left button does the
//! same, the mouse standing in for the keys.

use std::f32::consts::{FRAC_PI_2, PI};

use gk3_render::Camera;
use glam::Vec3;

/// Units a second; G-Engine's figures, found by trial.
const SPEED: f32 = 500.0;
/// Radians a second.
const TURN: f32 = PI;
const FAST: f32 = 2.0;
/// Pixels of mouse movement a frame worth a held key.
const MOUSE_RANGE: f32 = 12.0;
/// How far the mouse must move, held, before a click becomes a drag.
pub const DRAG_START: f32 = 5.0;

#[derive(Debug, Default)]
pub struct Controls {
    pub forward: bool,
    pub back: bool,
    pub left: bool,
    pub right: bool,
    /// Shift.
    pub pitch: bool,
    /// Ctrl.
    pub pan: bool,
    /// Alt.
    pub fast: bool,
    /// Mouse movement since the last frame while dragging, in pixels.
    pub drag: (f32, f32),
}

impl Controls {
    /// Moves the camera for `dt` seconds of whatever is held.
    pub fn apply(&mut self, camera: &mut Camera, dt: f32) {
        let (mut forward, mut strafe, mut turn, mut pitch, mut rise) = (0.0, 0.0, 0.0, 0.0, 0.0);
        // The keys, then the mouse, each by the same modifiers. Screen y
        // grows downwards, so pushing the mouse away is negative.
        let keys_y = (self.forward as i32 - self.back as i32) as f32;
        let keys_x = (self.right as i32 - self.left as i32) as f32;
        let mouse = (self.drag.0 / MOUSE_RANGE, -self.drag.1 / MOUSE_RANGE);
        self.drag = (0.0, 0.0);
        for (x, y) in [(keys_x, keys_y), mouse] {
            if self.pitch {
                pitch += y * TURN;
                turn += x * TURN;
            } else if self.pan {
                rise += y * SPEED;
                strafe += x * SPEED;
            } else {
                forward += y * SPEED;
                turn += x * TURN;
            }
        }
        let boost = if self.fast { FAST } else { 1.0 };
        // Over the floor, whatever the pitch.
        let ahead = Vec3::new(camera.yaw.sin(), 0.0, camera.yaw.cos());
        let right = Vec3::new(camera.yaw.cos(), 0.0, -camera.yaw.sin());
        camera.position += (ahead * forward + right * strafe + Vec3::Y * rise) * boost * dt;
        camera.yaw += turn * boost * dt;
        // Pitch is positive looking down; stop just short of straight up
        // or down, as G-Engine does.
        camera.pitch =
            (camera.pitch - pitch * boost * dt).clamp(-FRAC_PI_2 + 0.01, FRAC_PI_2 - 0.01);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn camera() -> Camera {
        Camera {
            position: Vec3::ZERO,
            yaw: 0.0,
            pitch: 0.0,
            fov_y: 1.0,
        }
    }

    #[test]
    fn keys_move_turn_and_pitch() {
        let mut cam = camera();
        let mut c = Controls {
            forward: true,
            ..Default::default()
        };
        c.apply(&mut cam, 0.5);
        assert!((cam.position.z - 250.0).abs() < 1e-3, "{:?}", cam.position);

        let mut c = Controls {
            right: true,
            ..Default::default()
        };
        c.apply(&mut cam, 0.5);
        assert!((cam.yaw - FRAC_PI_2).abs() < 1e-5, "turning right");
        let mut c = Controls {
            forward: true,
            fast: true,
            ..Default::default()
        };
        c.apply(&mut cam, 0.1);
        assert!(
            (cam.position.x - 100.0).abs() < 1e-3,
            "now facing +x: {:?}",
            cam.position
        );

        let mut c = Controls {
            forward: true,
            pitch: true,
            ..Default::default()
        };
        c.apply(&mut cam, 10.0);
        assert!(
            cam.pitch < -1.5,
            "Shift+Up looks up, clamped: {}",
            cam.pitch
        );

        let mut c = Controls {
            back: true,
            pan: true,
            ..Default::default()
        };
        let y = cam.position.y;
        c.apply(&mut cam, 0.1);
        assert!(
            (cam.position.y - (y - 50.0)).abs() < 1e-3,
            "Ctrl+Down sinks"
        );
    }

    #[test]
    fn dragging_away_moves_forward() {
        let mut cam = camera();
        let mut c = Controls {
            drag: (0.0, -12.0),
            ..Default::default()
        };
        c.apply(&mut cam, 0.1);
        assert!(cam.position.z > 49.0);
        assert_eq!(c.drag, (0.0, 0.0), "the drag is spent");
    }
}
