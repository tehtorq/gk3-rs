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

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Character {
    pub hip: Marker,
    pub left_shoe: Marker,
    pub right_shoe: Marker,
    pub shoe_thickness: f32,
    pub walker_height: f32,
    pub head_mesh: Option<usize>,
    pub walk_anim: Option<String>,
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
    }
}
