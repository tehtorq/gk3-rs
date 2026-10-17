//! A model's current shape: every mesh's transform and every vertex.
//!
//! A pose starts as the model was authored and is changed by playing vertex
//! animations over it. Between two recorded poses on consecutive frames the
//! two are mixed; across a gap the earlier pose is held, which is what the
//! original does and what keeps a planted foot planted.

use gk3_formats::act::{Keyed, VertexAnimation};
use gk3_formats::model::Model;
use glam::{Mat4, Vec3, Vec4};

#[derive(Debug, Clone, PartialEq)]
pub struct Pose {
    pub meshes: Vec<MeshPose>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MeshPose {
    /// Mesh-to-model transform as i, j, k, position columns.
    pub transform: [[f32; 3]; 4],
    /// Vertex positions by submesh, in mesh space.
    pub positions: Vec<Vec<[f32; 3]>>,
}

impl MeshPose {
    pub fn matrix(&self) -> Mat4 {
        let [i, j, k, p] = self.transform;
        Mat4::from_cols(
            Vec4::new(i[0], i[1], i[2], 0.0),
            Vec4::new(j[0], j[1], j[2], 0.0),
            Vec4::new(k[0], k[1], k[2], 0.0),
            Vec4::new(p[0], p[1], p[2], 1.0),
        )
    }

    /// A vertex in model space.
    pub fn point(&self, submesh: usize, vertex: usize) -> Option<Vec3> {
        let p = self.positions.get(submesh)?.get(vertex)?;
        Some(self.matrix().transform_point3(Vec3::from(*p)))
    }
}

impl Pose {
    /// The model as authored.
    pub fn rest(model: &Model) -> Self {
        Self {
            meshes: model
                .meshes
                .iter()
                .map(|m| MeshPose {
                    transform: m.transform,
                    positions: m.submeshes.iter().map(|s| s.positions.clone()).collect(),
                })
                .collect(),
        }
    }

    /// Turns the whole pose about the vertical through `pivot` (in the
    /// model's space) by `angle` radians, as if the model stood turned.
    pub fn turn_about(&mut self, angle: f32, pivot: Vec3) {
        let turn = Mat4::from_translation(pivot)
            * Mat4::from_rotation_y(angle)
            * Mat4::from_translation(-pivot);
        for mesh in &mut self.meshes {
            let m = turn * mesh.matrix();
            mesh.transform = [
                m.x_axis.truncate().to_array(),
                m.y_axis.truncate().to_array(),
                m.z_axis.truncate().to_array(),
                m.w_axis.truncate().to_array(),
            ];
        }
    }

    /// Mixes this pose back towards `from` by `weight` (0 leaves it, 1 is
    /// `from`): for easing from one pose into another. Meshes or submeshes
    /// that differ in shape are left as they are.
    pub fn blend_from(&mut self, from: &Pose, weight: f32) {
        if weight <= 0.0 {
            return;
        }
        for (mesh, old) in self.meshes.iter_mut().zip(&from.meshes) {
            mesh.transform = mix_transform(&mesh.transform, &old.transform, weight);
            for (positions, earlier) in mesh.positions.iter_mut().zip(&old.positions) {
                *positions = mix_positions(positions, earlier, weight);
            }
        }
    }

    /// Applies a clip at a time measured in frames. Meshes and submeshes the
    /// clip does not record keep the shape they had.
    pub fn apply(&mut self, clip: &VertexAnimation, frame_time: f32, looping: bool) {
        let last = clip.frame_count.saturating_sub(1);
        let frame_time = frame_time.clamp(0.0, last as f32 + 0.999);
        let frame = frame_time.floor() as u32;
        let t = frame_time - frame as f32;
        let wrap = looping && frame == last;

        for (mesh, track) in self.meshes.iter_mut().zip(&clip.meshes) {
            if let Some(value) = sample(&track.transforms, frame, t, wrap, mix_transform) {
                mesh.transform = value;
            }
            for (positions, poses) in mesh.positions.iter_mut().zip(&track.submeshes) {
                if let Some(value) = sample(poses, frame, t, wrap, mix_positions)
                    && value.len() == positions.len()
                {
                    *positions = value;
                }
            }
        }
    }
}

/// The value in force at `frame`, mixed towards the one in force on the
/// next frame (or, wrapping, on frame 0). A track with a gap holds its value
/// through it and moves on only in the frame before its next key: a mesh's
/// transform may skip frames its vertices do not, and the two must move
/// together or the part leaps out of place for a moment.
fn sample<T: Clone>(
    track: &[Keyed<T>],
    frame: u32,
    t: f32,
    wrap: bool,
    mix: fn(&T, &T, f32) -> T,
) -> Option<T> {
    let i = track.partition_point(|k| k.frame <= frame).checked_sub(1)?;
    let current = &track[i];
    if t <= 0.0 {
        return Some(current.value.clone());
    }
    let next = if wrap {
        track.first().filter(|k| k.frame == 0)
    } else {
        track.get(i + 1).filter(|k| k.frame == frame + 1)
    };
    Some(match next {
        Some(next) => mix(&current.value, &next.value, t),
        None => current.value.clone(),
    })
}

fn lerp3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
}

/// Component-wise: the bases are mirrored and some are not rotations at all,
/// and at one frame's step the error is a fraction of a percent.
fn mix_transform(a: &[[f32; 3]; 4], b: &[[f32; 3]; 4], t: f32) -> [[f32; 3]; 4] {
    std::array::from_fn(|c| lerp3(a[c], b[c], t))
}

// The signature matches `sample`'s mixer for `Vec` values.
#[allow(clippy::ptr_arg)]
fn mix_positions(a: &Vec<[f32; 3]>, b: &Vec<[f32; 3]>, t: f32) -> Vec<[f32; 3]> {
    if a.len() != b.len() {
        return a.clone();
    }
    a.iter().zip(b).map(|(a, b)| lerp3(*a, *b, t)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use gk3_formats::act::MeshTrack;

    fn keyed<T>(frame: u32, value: T) -> Keyed<T> {
        Keyed { frame, value }
    }

    fn pose() -> Pose {
        Pose {
            meshes: vec![MeshPose {
                transform: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.], [0., 0., 0.]],
                positions: vec![vec![[0.0; 3]]],
            }],
        }
    }

    fn clip(frames: &[u32]) -> VertexAnimation {
        VertexAnimation {
            model: "X".into(),
            frame_count: 4,
            meshes: vec![MeshTrack {
                transforms: Vec::new(),
                submeshes: vec![
                    frames
                        .iter()
                        .map(|&f| keyed(f, vec![[f as f32 * 10.0, 0.0, 0.0]]))
                        .collect(),
                ],
            }],
        }
    }

    #[test]
    fn mixes_consecutive_frames() {
        let mut p = pose();
        p.apply(&clip(&[0, 1, 2, 3]), 1.5, false);
        assert_eq!(p.meshes[0].positions[0][0], [15.0, 0.0, 0.0]);
    }

    #[test]
    fn holds_across_a_gap_then_moves_on_into_the_next_key() {
        let mut p = pose();
        p.apply(&clip(&[0, 3]), 1.5, false);
        assert_eq!(p.meshes[0].positions[0][0], [0.0, 0.0, 0.0]);
        p.apply(&clip(&[0, 3]), 2.5, false);
        assert_eq!(p.meshes[0].positions[0][0], [15.0, 0.0, 0.0]);
    }

    #[test]
    fn wraps_into_the_first_frame_when_looping() {
        let mut p = pose();
        p.apply(&clip(&[0, 1, 2, 3]), 3.5, true);
        assert_eq!(p.meshes[0].positions[0][0], [15.0, 0.0, 0.0]);
        p.apply(&clip(&[0, 1, 2, 3]), 3.5, false);
        assert_eq!(p.meshes[0].positions[0][0], [30.0, 0.0, 0.0]);
    }

    #[test]
    fn unrecorded_meshes_keep_their_shape() {
        let mut p = pose();
        p.meshes[0].positions[0][0] = [7.0, 7.0, 7.0];
        p.apply(&clip(&[]), 1.0, false);
        assert_eq!(p.meshes[0].positions[0][0], [7.0, 7.0, 7.0]);
    }
}
