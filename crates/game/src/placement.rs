//! Where a character's model goes so that they stand where the story put
//! them, facing the way it said.
//!
//! A character's vertices sit wherever the animator left them in model space,
//! so the model cannot simply be moved to the actor's position. As the
//! original (0x417e56, 0x41c054), the model's own floor point (under the hip
//! point, at the lower shoe) and facing (from the hip point to a reference
//! vertex of the hip mesh) are found, and the model is moved and turned so
//! those land on the actor. That is done when a clip starts and held while
//! it plays.

use glam::{Mat4, Vec3};

use crate::character::{Character, HipReference};
use crate::pose::Pose;

/// The model's floor point and, where its hip axes say, its facing
/// direction, both in model space.
pub fn floor_and_facing(pose: &Pose, character: &Character) -> Option<(Vec3, Option<Vec3>)> {
    let point = |m: &crate::character::Marker| pose.meshes.get(m.mesh)?.point(m.group, m.point);
    let hip = point(&character.hip)?;
    let left = point(&character.left_shoe)?;
    let right = point(&character.right_shoe)?;
    let floor = Vec3::new(hip.x, left.y.min(right.y) - character.shoe_thickness, hip.z);

    if let Some(r) = &character.reference {
        let to = pose.meshes[character.hip.mesh].point(r.group, r.vertex)? - hip;
        let heading = to.x.atan2(to.z) + r.adjust;
        return Some((floor, Some(Vec3::new(heading.sin(), 0.0, heading.cos()))));
    }

    // Without the original's reference (no walk start to take it from),
    // G-Engine's reading: characters are modelled facing down their hip
    // mesh's -Y axis (or +Y, for the odd model that has it the other way
    // round).
    let mut axis = -pose.meshes[character.hip.mesh].matrix().y_axis.truncate();
    if character.facing_reversed {
        axis = -axis;
    }
    let facing = Vec3::new(axis.x, 0.0, axis.z).try_normalize();
    Some((floor, facing))
}

/// The original's reading of a character's facing (0x41bf92, 0x545b80),
/// from their model posed as their walk starts: of the hip mesh's
/// vertices (less the trailing points some parts carry, `trailing` for
/// each part) at least a unit from the hip point across the floor, the one
/// lying most nearly down -Z from it; the facing is then from the hip point
/// to that vertex, turned so that this pose faces -Z (0x41c003).
pub fn hip_reference(
    pose: &Pose,
    character: &Character,
    trailing: &dyn Fn(usize) -> usize,
) -> Option<HipReference> {
    let mesh = pose.meshes.get(character.hip.mesh)?;
    let hip = mesh.point(character.hip.group, character.hip.point)?;
    let mut best: Option<(f32, usize, usize, Vec3)> = None;
    for (group, positions) in mesh.positions.iter().enumerate() {
        for vertex in 0..positions.len().saturating_sub(trailing(group)) {
            let to = mesh.point(group, vertex)? - hip;
            if to.x.hypot(to.z) < 1.0 {
                continue;
            }
            let along = to.normalize().dot(Vec3::NEG_Z);
            if best.is_none_or(|b| along > b.0) {
                best = Some((along, group, vertex, to));
            }
        }
    }
    let (_, group, vertex, to) = best?;
    let adjust = std::f32::consts::PI - to.x.atan2(to.z);
    let adjust = (adjust + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
    Some(HipReference { group, vertex, adjust })
}

/// Where the left and right shoes are, in model space.
pub fn shoes(pose: &Pose, character: &Character) -> Option<(Vec3, Vec3)> {
    let point = |m: &crate::character::Marker| pose.meshes.get(m.mesh)?.point(m.group, m.point);
    Some((point(&character.left_shoe)?, point(&character.right_shoe)?))
}

/// The model-to-world transform that stands this pose's floor point at
/// `position`, facing `heading` (radians; zero is +Z).
pub fn place(pose: &Pose, character: &Character, position: Vec3, heading: f32) -> Mat4 {
    match floor_and_facing(pose, character) {
        Some((floor, facing)) => {
            // Characters are modelled facing -Z when nothing says otherwise.
            let facing = facing.unwrap_or(Vec3::NEG_Z);
            let turn = heading - facing.x.atan2(facing.z);
            Mat4::from_translation(position)
                * Mat4::from_rotation_y(turn)
                * Mat4::from_translation(-floor)
        }
        // Without markers, the model's origin is the floor point and it is
        // modelled facing -Z.
        None => {
            Mat4::from_translation(position) * Mat4::from_rotation_y(heading + std::f32::consts::PI)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::character::Marker;
    use crate::pose::MeshPose;

    /// One mesh, identity transform, with hip at (10, 40, 5) and shoes on
    /// y = 1 and y = 2.
    fn standing() -> (Pose, Character) {
        let pose = Pose {
            meshes: vec![MeshPose {
                transform: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.], [0., 0., 0.]],
                positions: vec![vec![[10.0, 40.0, 5.0], [8.0, 1.0, 5.0], [12.0, 2.0, 5.0]]],
            }],
        };
        let character = Character {
            hip: Marker {
                mesh: 0,
                group: 0,
                point: 0,
            },
            left_shoe: Marker {
                mesh: 0,
                group: 0,
                point: 1,
            },
            right_shoe: Marker {
                mesh: 0,
                group: 0,
                point: 2,
            },
            shoe_thickness: 1.0,
            ..Default::default()
        };
        (pose, character)
    }

    #[test]
    fn floor_is_under_the_hips_at_the_lower_shoe() {
        let (pose, character) = standing();
        let (floor, facing) = floor_and_facing(&pose, &character).unwrap();
        assert_eq!(floor, Vec3::new(10.0, 0.0, 5.0));
        // -Y of an upright basis has no horizontal part.
        assert_eq!(facing, None);
    }

    #[test]
    fn placement_lands_the_floor_point_on_the_actor() {
        let (mut pose, character) = standing();
        // Tip the hip basis so -Y points along +X: the model faces +X.
        pose.meshes[0].transform = [[0., 1., 0.], [-1., 0., 0.], [0., 0., 1.], [0., 0., 0.]];
        // Re-express the markers so the hip still sits at (10, 40, 5) in model space.
        let m = pose.meshes[0].matrix().inverse();
        for v in &mut pose.meshes[0].positions[0] {
            *v = m.transform_point3(Vec3::from(*v)).into();
        }
        let world = place(&pose, &character, Vec3::new(100.0, 0.0, 200.0), 0.0);
        let floor = world.transform_point3(Vec3::new(10.0, 0.0, 5.0));
        assert!((floor - Vec3::new(100.0, 0.0, 200.0)).length() < 1e-4);
        // Facing +X turned to heading 0 (+Z).
        let ahead = world.transform_vector3(Vec3::X);
        assert!((ahead - Vec3::Z).length() < 1e-4);
    }

    #[test]
    fn the_reference_is_the_vertex_most_down_minus_z_and_that_pose_faces_it() {
        let (mut pose, mut character) = standing();
        // Hip at (10, 40, 5); candidates: one too near across the floor,
        // one down -Z and a little to the side, one off to +X, and a
        // trailing point (ignored) straight down -Z.
        pose.meshes[0].positions[0].extend([
            [10.0, 30.0, 5.5],
            [11.0, 40.0, -5.0],
            [20.0, 40.0, 5.0],
            [10.0, 40.0, -50.0],
        ]);
        let reference = hip_reference(&pose, &character, &|_| 1).unwrap();
        assert_eq!((reference.group, reference.vertex), (0, 4));
        character.reference = Some(reference);
        let (_, facing) = floor_and_facing(&pose, &character).unwrap();
        assert!((facing.unwrap() - Vec3::NEG_Z).length() < 1e-5);
    }
}
