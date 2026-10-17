//! Footsteps: which sound a step makes, by the walker's shoes and the floor
//! under them.
//!
//! `FLOORMAP.TXT` sorts floor textures into types (`Carpet=27RUG,21rug,...`);
//! `FOOTSTEPS.TXT` and `FOOTSCUFFS.TXT` give, for each shoe type (the
//! `ShoeType` of `CHARACTERS.TXT`), the sounds for each floor type. As
//! G-Engine's `FootstepManager`.

use std::collections::HashMap;

#[derive(Debug, Default)]
pub struct Footsteps {
    floor_type: HashMap<String, String>,
    steps: HashMap<(String, String), Vec<String>>,
    scuffs: HashMap<(String, String), Vec<String>>,
}

/// `Key=a,b,c` lines by section, comments and blank lines skipped.
fn lines(text: &str) -> Vec<(String, String, Vec<String>)> {
    let mut section = String::new();
    let mut out = Vec::new();
    for raw in text.lines() {
        let line = raw.split("//").next().unwrap_or("").trim();
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            section = name.trim().to_ascii_uppercase();
        } else if let Some((key, values)) = line.split_once('=') {
            let values = values
                .split(',')
                .map(|v| v.trim().to_ascii_uppercase())
                .filter(|v| !v.is_empty())
                .collect();
            out.push((section.clone(), key.trim().to_ascii_uppercase(), values));
        }
    }
    out
}

impl Footsteps {
    pub fn parse(floor_map: &str, footsteps: &str, footscuffs: &str) -> Self {
        let mut f = Self::default();
        for (_, kind, textures) in lines(floor_map) {
            for texture in textures {
                f.floor_type.insert(texture, kind.clone());
            }
        }
        for (shoe, floor, sounds) in lines(footsteps) {
            f.steps.insert((shoe, floor), sounds);
        }
        for (shoe, floor, sounds) in lines(footscuffs) {
            f.scuffs.insert((shoe, floor), sounds);
        }
        f
    }

    /// The sounds a step (or a scuff) could make, for shoes on a floor
    /// texture; one is picked at random. None for floors not in the map.
    pub fn sounds(&self, shoe: &str, floor_texture: &str, scuff: bool) -> &[String] {
        let Some(kind) = self.floor_type.get(&floor_texture.to_ascii_uppercase()) else {
            return &[];
        };
        let table = if scuff { &self.scuffs } else { &self.steps };
        table
            .get(&(shoe.to_ascii_uppercase(), kind.clone()))
            .map_or(&[], |v| v.as_slice())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_by_shoe_and_floor() {
        let f = Footsteps::parse(
            "// FloorMap\n[FloorMap]\n\nCarpet=27RUG,carpet1\nTile=LBYFLOOR\n",
            "[Male Leather]\nCarpet=MCarpLeth1,MCarpLeth2\nTile=MTileLeth1\n",
            "[Male Leather]\nCarpet=MSCarpLeth1\n",
        );
        assert_eq!(
            f.sounds("Male Leather", "carpet1", false),
            ["MCARPLETH1", "MCARPLETH2"]
        );
        assert_eq!(f.sounds("male leather", "LbyFloor", false), ["MTILELETH1"]);
        assert_eq!(f.sounds("Male Leather", "27rug", true), ["MSCARPLETH1"]);
        assert!(f.sounds("Male Leather", "marble", false).is_empty());
        assert!(f.sounds("Male Boot", "carpet1", false).is_empty());
    }
}
