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

    /// Where a camera at `from` ends up trying to move by `offset`, as
    /// G-Engine's `ResolveCollisions`: the sphere goes as far as the first
    /// wall it would touch, and what is left of the move slides along that
    /// wall, a few times over. It never moves into a wall, so it is never
    /// pushed back out of one either, which jitters in corners and gaps.
    pub fn slide(&self, from: Vec3, offset: Vec3) -> Vec3 {
        if !self.enabled || self.walls().next().is_none() {
            return from + offset;
        }
        let mut p = from;
        let mut rest = offset;
        let mut first: Option<Vec3> = None;
        for pass in 0..3 {
            if rest.length_squared() < 1e-8 {
                break;
            }
            let hit = self
                .walls()
                .filter_map(|(tri, n)| sweep(p, rest, tri, *n))
                .min_by(|a, b| a.0.total_cmp(&b.0));
            let Some((t, normal)) = hit else {
                p += rest;
                break;
            };
            // Up to the wall, less a hair so the next move starts clear.
            let length = rest.length();
            let travel = (t * length - SKIN).max(0.0);
            p += rest / length * travel;
            if pass == 2 {
                break;
            }
            // What is left, along the wall.
            let left = rest * (1.0 - t);
            rest = left - normal * left.dot(normal).min(0.0);
            // In a corner, sliding along the second wall would go back
            // into the first: only along the crease where they meet, or
            // the camera swings from one to the other frame after frame.
            if let Some(n1) = first
                && rest.dot(n1) < 0.0
            {
                let crease = n1.cross(normal).normalize_or_zero();
                rest = crease * left.dot(crease);
            }
            first.get_or_insert(normal);
        }
        // Never back the way it was pushed.
        if (p - from).dot(offset) < 0.0 {
            return from;
        }
        p
    }
}

/// How far short of a wall the camera stops.
const SKIN: f32 = 0.01;

/// When, as a fraction of `offset`, a sphere of `RADIUS` at `c` moving by
/// `offset` first touches a wall's front, and the direction it is pushed
/// back in there. Only a move towards the front counts (one-sided, as
/// G-Engine's `Collide::SphereTriangle`); a sphere already touching it
/// stops at once.
fn sweep(c: Vec3, offset: Vec3, tri: &[Vec3; 3], n: Vec3) -> Option<(f32, Vec3)> {
    let towards = -n.dot(offset);
    if towards <= 0.0 {
        return None;
    }
    let distance = n.dot(c - tri[0]);
    if distance < -RADIUS {
        return None;
    }
    let mut best: Option<(f32, Vec3)> = None;
    // A touch counts only if it pushes back against the move.
    let mut consider = |t: f32, normal: Vec3| {
        if (0.0..=1.0).contains(&t)
            && normal.dot(offset) < 0.0
            && best.is_none_or(|(b, _)| t < b)
        {
            best = Some((t, normal));
        }
    };
    // The face: the sphere's leading point meets the plane inside it.
    if distance >= 0.0 {
        let t = ((distance - RADIUS) / towards).max(0.0);
        if t > 1.0 {
            return None;
        }
        let touch = c + offset * t - n * distance.min(RADIUS);
        if contains(tri, touch) {
            consider(t, n);
        }
    }
    // The corners and edges: where the moving centre comes within a radius.
    for k in 0..3 {
        let (a, b) = (tri[k], tri[(k + 1) % 3]);
        if let Some(t) = reach_point(c, offset, a) {
            consider(t, (c + offset * t - a).normalize_or(n));
        }
        if let Some((t, q)) = reach_edge(c, offset, a, b) {
            consider(t, (c + offset * t - q).normalize_or(n));
        }
    }
    best
}

/// Whether `p`, on a triangle's plane, is inside it (either winding).
fn contains([a, b, c]: &[Vec3; 3], p: Vec3) -> bool {
    let n = (*b - *a).cross(*c - *a);
    let side = |u: Vec3, v: Vec3| (v - u).cross(p - u).dot(n) >= -1e-4;
    side(*a, *b) && side(*b, *c) && side(*c, *a)
}

/// When a sphere at `c` moving by `offset` first comes within `RADIUS` of
/// point `p` (at once if it already is).
fn reach_point(c: Vec3, offset: Vec3, p: Vec3) -> Option<f32> {
    let m = c - p;
    let cc = m.length_squared() - RADIUS * RADIUS;
    if cc <= 0.0 {
        return Some(0.0);
    }
    first_root(offset.length_squared(), 2.0 * m.dot(offset), cc)
}

/// The same for the segment `a`-`b`, with the point on it touched.
fn reach_edge(c: Vec3, offset: Vec3, a: Vec3, b: Vec3) -> Option<(f32, Vec3)> {
    let e = b - a;
    let ee = e.length_squared();
    if ee < 1e-8 {
        return None;
    }
    // Work across the edge: drop each vector's part along it.
    let across = |v: Vec3| v - e * (v.dot(e) / ee);
    let (m, d) = (across(c - a), across(offset));
    let cc = m.length_squared() - RADIUS * RADIUS;
    let t = if cc <= 0.0 {
        0.0
    } else {
        first_root(d.length_squared(), 2.0 * m.dot(d), cc)?
    };
    let s = (c + offset * t - a).dot(e) / ee;
    (0.0..=1.0).contains(&s).then(|| (t, a + e * s))
}

/// The smaller root of a t^2 + b t + c = 0 in [0, 1], if moving closer.
fn first_root(a: f32, b: f32, c: f32) -> Option<f32> {
    if a < 1e-8 || b >= 0.0 {
        return None;
    }
    let disc = b * b - 4.0 * a * c;
    if disc < 0.0 {
        return None;
    }
    let t = (-b - disc.sqrt()) / (2.0 * a);
    (0.0..=1.0).contains(&t).then_some(t)
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

    /// Pushed into the corners and creases of Gabriel's room (both of its
    /// bounds models) from all over it, the camera never swings back and
    /// forth between two walls from one frame to the next.
    #[test]
    fn r25_corners_hold_the_camera_steady() {
        let Some(dir) = std::env::var_os("GK3_DATA") else {
            return;
        };
        let lib = gk3_barn::Library::open_dir(dir).unwrap();
        let models: Vec<_> = ["R25CAMERABOUNDS.MOD", "R25_SIDCM.MOD"]
            .iter()
            .map(|n| gk3_formats::model::Model::parse(&lib.read(n).unwrap()).unwrap())
            .collect();
        let bounds = Bounds::from_models(models.iter());
        for gx in 0..6 {
            for gz in 0..7 {
                let start = Vec3::new(-20.0 + gx as f32 * 60.0, 62.0, 30.0 + gz as f32 * 60.0);
                for i in 0..36 {
                    let a = i as f32 / 12.0 * std::f32::consts::TAU;
                    let d = Vec3::new(a.sin(), (i / 12) as f32 * 0.3 - 0.3, a.cos());
                    let (mut p, mut prev, mut swings) = (start, Vec3::ZERO, 0);
                    for _ in 0..180 {
                        let q = bounds.slide(p, d * SPEED / 60.0);
                        let m = q - p;
                        if m.length() > 0.05 && prev.length() > 0.05 && m.dot(prev) < 0.0 {
                            swings += 1;
                        }
                        (prev, p) = (m, q);
                    }
                    assert!(swings <= 3, "from {start} heading {i}: {swings} swings, ending {p}");
                }
            }
        }
    }

    /// Held against walls, in corners and across the bathroom's narrow
    /// spaces, the camera comes to rest or slides on: it never shakes back
    /// and forth from frame to frame.
    #[test]
    fn r25_walls_hold_the_camera_steady() {
        let Some(dir) = std::env::var_os("GK3_DATA") else {
            return;
        };
        let lib = gk3_barn::Library::open_dir(dir).unwrap();
        let model =
            gk3_formats::model::Model::parse(&lib.read("R25CAMERABOUNDS.MOD").unwrap()).unwrap();
        let bounds = Bounds::from_models([&model]);
        // The START camera, and one inside the bathroom door.
        for start in [Vec3::new(277.58, 61.70, 50.57), Vec3::new(300.0, 65.0, 130.0)] {
            for i in 0..32 {
                let angle = i as f32 / 32.0 * std::f32::consts::TAU;
                let dir = Vec3::new(angle.sin(), (i % 5) as f32 * 0.1 - 0.2, angle.cos());
                // Five seconds at 60 frames a second, full speed.
                let mut p = start;
                let mut trail = vec![p];
                for _ in 0..300 {
                    p = bounds.slide(p, dir * SPEED / 60.0);
                    trail.push(p);
                }
                // The last second: the way it went is as long as how far
                // it got, give or take.
                let last = &trail[trail.len() - 61..];
                let path: f32 = last.windows(2).map(|w| w[0].distance(w[1])).sum();
                let net = last[0].distance(last[60]);
                assert!(
                    path <= net + 1.0,
                    "from {start} heading {i}: moved {path:.1} to get {net:.1}"
                );
            }
        }
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
