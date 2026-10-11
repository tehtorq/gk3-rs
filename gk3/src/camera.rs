//! The player's camera, moved as G-Engine's `GameCamera` moves it: W/S or
//! Up/Down go forward and back over the floor, A/D or Left/Right turn.
//! Holding Shift makes those pitch and turn, Ctrl makes them rise, sink and
//! strafe, Alt doubles every speed. Dragging with the left button does the
//! same, the mouse standing in for the keys.
//!
//! A room may name camera bounds models: one-sided walls, facing into the
//! space the camera may use, that it slides along rather than passes. Like
//! G-Engine, the camera is a sphere, and only stopped moving into a wall's
//! front, so a camera placed outside can still come in.

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
/// The camera's size against the bounds, in units.
const RADIUS: f32 = 16.0;

/// A wall: its corners and front-facing unit normal.
type Wall = ([Vec3; 3], Vec3);

/// The walls the camera keeps within: world-space triangles, each with its
/// front-facing unit normal. Scripts may add a prop's model as a wall, take
/// it away again, or switch the walls off until the next room.
#[derive(Debug, Clone)]
pub struct Bounds {
    triangles: Vec<Wall>,
    /// Props blocking the camera, by model name.
    blocks: Vec<(String, Vec<Wall>)>,
    pub enabled: bool,
}

impl Default for Bounds {
    fn default() -> Self {
        Self {
            triangles: Vec::new(),
            blocks: Vec::new(),
            enabled: true,
        }
    }
}

/// Triangles with their front normals; degenerate ones are dropped.
fn with_normals(triangles: impl IntoIterator<Item = [Vec3; 3]>) -> Vec<Wall> {
    triangles
        .into_iter()
        .filter_map(|[a, b, c]| {
            // Front is the side (b - a) x (c - a) points to: rays from inside
            // R25 meet every wall's front.
            let normal = (b - a).cross(c - a);
            (normal.length_squared() > 1e-8).then(|| ([a, b, c], normal.normalize()))
        })
        .collect()
}

impl Bounds {
    /// `CameraBoundaryBlockModel`: a prop becomes a wall.
    pub fn block(&mut self, model: &str, triangles: Vec<[Vec3; 3]>) {
        self.unblock(model);
        self.blocks
            .push((model.to_ascii_uppercase(), with_normals(triangles)));
    }

    pub fn unblock(&mut self, model: &str) {
        self.blocks
            .retain(|(name, _)| !name.eq_ignore_ascii_case(model));
    }

    fn walls(&self) -> impl Iterator<Item = &Wall> {
        self.triangles
            .iter()
            .chain(self.blocks.iter().flat_map(|(_, t)| t))
    }

    /// Bounds models sit at the world origin, so only each mesh's own
    /// transform applies.
    pub fn from_models<'a>(
        models: impl IntoIterator<Item = &'a gk3_formats::model::Model>,
    ) -> Self {
        let mut corners = Vec::new();
        for model in models {
            for mesh in &model.meshes {
                let [i, j, k, t] = mesh.transform.map(Vec3::from);
                let place = |p: [f32; 3]| i * p[0] + j * p[1] + k * p[2] + t;
                for sub in &mesh.submeshes {
                    corners.extend(
                        sub.triangles
                            .iter()
                            .map(|tri| tri.map(|v| place(sub.positions[v as usize]))),
                    );
                }
            }
        }
        Self {
            triangles: with_normals(corners),
            ..Self::default()
        }
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.triangles.len()
    }

    /// Where a camera at `from` ends up trying to move by `offset`. Moves in
    /// steps short enough not to pass through a wall, pushing out of any wall
    /// it is moving into the front of; what remains of the push slides it
    /// along the wall.
    pub fn slide(&self, from: Vec3, offset: Vec3) -> Vec3 {
        if !self.enabled || self.walls().next().is_none() {
            return from + offset;
        }
        let steps = (offset.length() / (RADIUS * 0.5)).ceil().max(1.0) as usize;
        let step = offset / steps as f32;
        let mut p = from;
        for _ in 0..steps {
            let before = p;
            p += step;
            for _ in 0..4 {
                let mut pushed = false;
                for (tri, n) in self.walls() {
                    // One-sided: only walls being approached from the front.
                    if n.dot(p - before) >= 0.0 || n.dot(before - tri[0]) < -RADIUS * 0.5 {
                        continue;
                    }
                    let q = closest_on_triangle(p, tri);
                    let apart = p - q;
                    let d = apart.length();
                    if d >= RADIUS {
                        continue;
                    }
                    // Off the face (an edge or corner) push straight away
                    // from it; on it, or gone through, push out along the
                    // normal.
                    let out = if d > 1e-4 && n.dot(apart) > 0.0 {
                        apart / d
                    } else {
                        *n
                    };
                    let depth = if out == *n {
                        RADIUS - n.dot(p - tri[0])
                    } else {
                        RADIUS - d
                    };
                    p += out * depth.max(0.0);
                    pushed = true;
                }
                if !pushed {
                    break;
                }
            }
        }
        p
    }
}

/// The point of a triangle nearest `p` (Ericson, Real-Time Collision
/// Detection, 5.1.5).
fn closest_on_triangle(p: Vec3, [a, b, c]: &[Vec3; 3]) -> Vec3 {
    let (a, b, c) = (*a, *b, *c);
    let (ab, ac, ap) = (b - a, c - a, p - a);
    let (d1, d2) = (ab.dot(ap), ac.dot(ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = p - b;
    let (d3, d4) = (ab.dot(bp), ac.dot(bp));
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return a + ab * (d1 / (d1 - d3));
    }
    let cp = p - c;
    let (d5, d6) = (ab.dot(cp), ac.dot(cp));
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return a + ac * (d2 / (d2 - d6));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        return b + (c - b) * ((d4 - d3) / ((d4 - d3) + (d5 - d6)));
    }
    let denom = 1.0 / (va + vb + vc);
    a + ab * (vb * denom) + ac * (vc * denom)
}

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
    pub fn apply(&mut self, camera: &mut Camera, bounds: &Bounds, dt: f32) {
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
        let offset = (ahead * forward + right * strafe + Vec3::Y * rise) * boost * dt;
        camera.position = bounds.slide(camera.position, offset);
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
        c.apply(&mut cam, &Bounds::default(), 0.5);
        assert!((cam.position.z - 250.0).abs() < 1e-3, "{:?}", cam.position);

        let mut c = Controls {
            right: true,
            ..Default::default()
        };
        c.apply(&mut cam, &Bounds::default(), 0.5);
        assert!((cam.yaw - FRAC_PI_2).abs() < 1e-5, "turning right");
        let mut c = Controls {
            forward: true,
            fast: true,
            ..Default::default()
        };
        c.apply(&mut cam, &Bounds::default(), 0.1);
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
        c.apply(&mut cam, &Bounds::default(), 10.0);
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
        c.apply(&mut cam, &Bounds::default(), 0.1);
        assert!(
            (cam.position.y - (y - 50.0)).abs() < 1e-3,
            "Ctrl+Down sinks"
        );
    }

    #[test]
    fn walls_stop_from_the_front_and_slide() {
        // A wall at z = 100 facing back towards -z, and wide.
        let (a, b, c) = (
            Vec3::new(-1000.0, -1000.0, 100.0),
            Vec3::new(1000.0, -1000.0, 100.0),
            Vec3::new(0.0, 1000.0, 100.0),
        );
        let bounds = Bounds {
            triangles: vec![([a, b, c], Vec3::NEG_Z)],
            ..Bounds::default()
        };
        let end = bounds.slide(Vec3::ZERO, Vec3::new(0.0, 0.0, 500.0));
        assert!(
            (end.z - (100.0 - RADIUS)).abs() < 0.5,
            "stopped a radius short: {end}"
        );
        let end = bounds.slide(Vec3::new(0.0, 0.0, 80.0), Vec3::new(50.0, 0.0, 50.0));
        assert!(
            (end.x - 50.0).abs() < 0.5 && end.z <= 100.0 - RADIUS + 0.5,
            "slid along: {end}"
        );
        // From behind, the wall lets the camera through.
        let end = bounds.slide(Vec3::new(0.0, 0.0, 200.0), Vec3::new(0.0, 0.0, -300.0));
        assert!((end.z + 100.0).abs() < 1e-3);
        // Switched off, nothing stops it.
        let mut off = bounds.clone();
        off.enabled = false;
        assert_eq!(off.slide(Vec3::ZERO, Vec3::Z * 500.0).z, 500.0);
        // A blocking prop is a wall too, until unblocked; (b - a) x (c - a)
        // of this one faces -z.
        let mut props = Bounds::default();
        props.block("BOX", vec![[a, c, b]]);
        assert!(props.slide(Vec3::ZERO, Vec3::Z * 500.0).z < 100.0);
        props.unblock("box");
        assert_eq!(props.slide(Vec3::ZERO, Vec3::Z * 500.0).z, 500.0);
    }

    /// The real bounds keep the camera in Gabriel's room, whichever way it
    /// flies from the room's own camera.
    #[test]
    fn r25_bounds_keep_the_camera_in() {
        let Some(dir) = std::env::var_os("GK3_DATA") else {
            return;
        };
        let lib = gk3_barn::Library::open_dir(dir).unwrap();
        let model =
            gk3_formats::model::Model::parse(&lib.read("R25CAMERABOUNDS.MOD").unwrap()).unwrap();
        let bounds = Bounds::from_models([&model]);
        assert!(bounds.len() > 10);
        let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
        for (tri, _) in &bounds.triangles {
            for v in tri {
                lo = lo.min(*v);
                hi = hi.max(*v);
            }
        }
        // The room's own START camera is inside, so the walls face it: that
        // settles which winding is the front.
        let start = Vec3::new(277.58, 61.70, 50.57);
        // The first wall each ray from it meets is met from the front.
        let (mut front, mut rays) = (0, 0);
        for i in 0..64 {
            let (yaw, pitch) = (i as f32 * 0.7, (i % 7) as f32 * 0.2 - 0.6);
            let dir = Vec3::new(
                yaw.sin() * pitch.cos(),
                pitch.sin(),
                yaw.cos() * pitch.cos(),
            );
            let hit = bounds
                .triangles
                .iter()
                .filter_map(|(tri, n)| {
                    crate::cast::ray_triangle(start, dir, tri[0], tri[1], tri[2]).map(|t| (t, n))
                })
                .min_by(|a, b| a.0.total_cmp(&b.0));
            if let Some((_, n)) = hit {
                rays += 1;
                front += (n.dot(dir) < 0.0) as usize;
            }
        }
        assert!(
            rays > 32 && front * 10 >= rays * 9,
            "{front} of {rays} rays meet a wall's front"
        );
        for i in 0..16 {
            let angle = i as f32 / 16.0 * std::f32::consts::TAU;
            let dir = Vec3::new(angle.sin(), (i as f32 - 8.0) / 16.0, angle.cos());
            let mut p = start;
            for _ in 0..200 {
                p = bounds.slide(p, dir * 10.0);
            }
            assert!(
                p.cmpge(lo - 1.0).all() && p.cmple(hi + 1.0).all(),
                "flew out heading {i}: {p} not in {lo}..{hi}"
            );
        }
    }

    #[test]
    fn dragging_away_moves_forward() {
        let mut cam = camera();
        let mut c = Controls {
            drag: (0.0, -12.0),
            ..Default::default()
        };
        c.apply(&mut cam, &Bounds::default(), 0.1);
        assert!(cam.position.z > 49.0);
        assert_eq!(c.drag, (0.0, 0.0), "the drag is spent");
    }
}
