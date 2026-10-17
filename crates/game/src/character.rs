//! Per-character data from `CHARACTERS.TXT`: which vertices mark a
//! character's hips and shoes, and how they walk.

use std::collections::HashMap;

use gk3_formats::ini::{Document, Split};

/// A vertex in a model: mesh, submesh ("group"), vertex.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Marker {
    pub mesh: usize,
    pub group: usize,
    pub point: usize,
}

/// Which way a character faces, as the original reads it (0x41c054): from
/// the hip point to this vertex of the hip mesh, across the floor, turned
/// by `adjust` (radians). Chosen once from the model as its walk starts
/// (`placement::hip_reference`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct HipReference {
    pub group: usize,
    pub vertex: usize,
    pub adjust: f32,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Character {
    pub hip: Marker,
    pub left_shoe: Marker,
    pub right_shoe: Marker,
    pub shoe_thickness: f32,
    pub walker_height: f32,
    pub head_mesh: Option<usize>,
    pub walk_anim: Option<String>,
    /// Starting to walk; its first frame is the stance a walk ends in.
    pub start_anim: Option<String>,
    /// Starting to walk by turning left or right first, for a goal behind.
    pub start_turn_left: Option<String>,
    pub start_turn_right: Option<String>,
    /// The walk cycle's frame to go on from after the turn-right start
    /// (Gabriel's is 10): the original's `StartTurnRightFrame`.
    pub start_turn_right_frame: u32,
    /// For which footstep sounds they make (`FOOTSTEPS.TXT`).
    pub shoe_type: String,
    /// The hip mesh of this model points backwards: its facing is the
    /// opposite of the usual reading. Found from the way the walk cycle
    /// carries the feet (Emilio with his knapsack, EM2, is one).
    pub facing_reversed: bool,
    /// The original's way of reading the facing, once it is known.
    pub reference: Option<HipReference>,
    /// How far the head can turn: left, up, right, down, in degrees.
    pub head_fov: [f32; 4],
    /// Whose face they wear when `FACES.TXT` has none under their own
    /// code, and what its pictures are named after (`EM2` is Emilio with a
    /// knapsack: `EML`). Upper case.
    pub base_name: String,
    /// What their lip-sync mouths are named after (`VA1_MOUTH03`). Upper
    /// case.
    pub mouth_name: String,
}

/// Parses every section, keyed by the upper-cased three-letter model code.
pub fn parse(bytes: &[u8]) -> HashMap<String, Character> {
    let doc = Document::parse(bytes, Split::Whole);
    let mut out = HashMap::new();
    for section in &doc.sections {
        if section.name.is_empty() {
            continue;
        }
        let get = |key: &str| section.lines.iter().find_map(|l| l.get(key));
        let num = |key: &str| get(key).and_then(|v| v.trim().parse::<f32>().ok());
        let index = |key: &str| num(key).map_or(0, |v| v as usize);
        let marker = |prefix: &str| Marker {
            mesh: index(&format!("{prefix}MeshIndex")),
            group: index(&format!("{prefix}GroupIndex")),
            point: index(&format!("{prefix}PointIndex")),
        };
        // Each defaults to the one before, as the original reads them.
        let name = |key: &str, or: String| {
            get(key)
                .map(|v| v.trim().to_ascii_uppercase())
                .filter(|v| !v.is_empty())
                .unwrap_or(or)
        };
        let model_name = name("ModelName", section.name.to_ascii_uppercase());
        let base_name = name("BaseName", model_name);
        let mouth_name = name("MouthName", base_name.clone());
        out.insert(
            section.name.to_ascii_uppercase(),
            Character {
                hip: marker("HipAxes"),
                left_shoe: marker("LShoeAxes"),
                right_shoe: marker("RShoeAxes"),
                shoe_thickness: num("ShoeThickness").unwrap_or(0.0),
                walker_height: num("WalkerHeight").unwrap_or(70.0),
                head_mesh: num("HeadMeshIndex").map(|v| v as usize),
                walk_anim: get("ContAnim").map(str::to_string),
                start_anim: get("StartAnim").map(str::to_string),
                start_turn_left: get("StartTurnLeftAnim").map(str::to_string),
                start_turn_right: get("StartTurnRightAnim").map(str::to_string),
                start_turn_right_frame: num("StartTurnRightFrame").map_or(0, |v| v.max(0.0) as u32),
                shoe_type: get("ShoeType").unwrap_or("Male Leather").trim().to_string(),
                facing_reversed: false,
                reference: None,
                head_fov: {
                    let n: Vec<f32> = get("HeadFOV")
                        .unwrap_or("")
                        .split(',')
                        .filter_map(|v| v.trim().parse().ok())
                        .collect();
                    n.try_into().unwrap_or([60.0, 40.0, 60.0, 34.0])
                },
                base_name,
                mouth_name,
            },
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_gabriel() {
        let chars = parse(
            b"[GAB] // Gabriel\r\nHeadMeshIndex=10\r\nHipAxesMeshIndex=5\r\nHipAxesGroupIndex=0\r\n\
              HipAxesPointIndex=125\r\nLShoeAxesMeshIndex=1\r\nLShoeAxesPointIndex=39\r\n\
              WalkerHeight=76.0\r\nShoeThickness=0.75\r\nContAnim=Gabwalk\r\n",
        );
        let gab = &chars["GAB"];
        assert_eq!(
            gab.hip,
            Marker {
                mesh: 5,
                group: 0,
                point: 125
            }
        );
        assert_eq!(gab.left_shoe.point, 39);
        assert_eq!(gab.shoe_thickness, 0.75);
        assert_eq!(gab.head_mesh, Some(10));
        assert_eq!(gab.walk_anim.as_deref(), Some("Gabwalk"));
        assert_eq!((gab.base_name.as_str(), gab.mouth_name.as_str()), ("GAB", "GAB"));
    }

    #[test]
    fn names_default_down_the_chain() {
        let chars = parse(
            b"[EM2]\r\nModelName=em2\r\nBasename=eml\r\n\
              [VM1]\r\nModelName=vm1\r\nBaseName=vm1\r\nMouthName=va1\r\n\
              [MOV]\r\nModelName=mov\r\n",
        );
        let names = |c: &str| (chars[c].base_name.clone(), chars[c].mouth_name.clone());
        assert_eq!(names("EM2"), ("EML".into(), "EML".into()));
        assert_eq!(names("VM1"), ("VM1".into(), "VA1".into()));
        assert_eq!(names("MOV"), ("MOV".into(), "MOV".into()));
    }
}
