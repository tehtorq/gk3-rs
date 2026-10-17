//! The binoculars: from a few lookouts, the player pans across the skyline
//! and zooms into other places, where scripts show what is going on there.
//!
//! As G-Engine's `BinocsOverlay`, which this follows. `BINOCS.TXT` says,
//! for each lookout and timeblock, which places can be zoomed into and how:
//! the scene to show, where its camera stands, and the `BINOCS.SHP`
//! functions to call going in and out. Which way to point to zoom is
//! G-Engine's table; the file's own rectangles are in panorama pixels.
//! Panned out, the camera rises far above the room, where only the sky is
//! seen; zoomed in, the other place's scene is drawn at 30 degrees.

use std::collections::HashMap;

use gk3_formats::ini::{Document, Split};

/// One place to zoom into.
#[derive(Debug, Clone, PartialEq)]
pub struct Zoom {
    /// The scene asset to show, e.g. `LHM_A_A`.
    pub scene: String,
    /// Degrees: yaw, pitch.
    pub angle: (f32, f32),
    pub position: [f32; 3],
    pub enter: Option<String>,
    pub exit: Option<String>,
}

/// A lookout at a timeblock: what can be seen, and the animation that puts
/// the binoculars away.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Lookout {
    pub put_away: Option<String>,
    /// By three-letter location code.
    pub zooms: HashMap<String, Zoom>,
}

/// Reads `BINOCS.TXT`: a header section per lookout and timeblock
/// (`[CD1102P]`, with `LOC=` and `ANIM=`), followed by one section per place
/// it lists, named lookout, timeblock and scene (`[CD1102PLHM_a_a]`).
pub fn parse(bytes: &[u8]) -> HashMap<String, Lookout> {
    let doc = Document::parse(bytes, Split::Whole);
    let mut out: HashMap<String, Lookout> = HashMap::new();
    let get = |s: &gk3_formats::ini::Section, key: &str| {
        s.lines
            .iter()
            .find_map(|l| l.get(key))
            .map(|v| v.trim().to_string())
    };
    let fn_name = |name: String| {
        if name.ends_with('$') {
            name
        } else {
            format!("{name}$")
        }
    };
    for section in &doc.sections {
        let name = section.name.to_ascii_uppercase();
        if name.len() == 7 {
            let lookout = out.entry(name).or_default();
            lookout.put_away = get(section, "ANIM");
        } else if name.len() > 7 {
            let (head, scene) = name.split_at(7);
            let floats = |key: &str| -> Vec<f32> {
                get(section, key)
                    .map(|v| v.split(',').filter_map(|n| n.trim().parse().ok()).collect())
                    .unwrap_or_default()
            };
            let (angle, position) = (floats("CAMANGLE"), floats("CAMPOS"));
            let (Some(&[yaw, pitch]), Some(&[x, y, z])) = (angle.get(..2), position.get(..3))
            else {
                continue;
            };
            let zoom = Zoom {
                scene: scene.to_string(),
                angle: (yaw, pitch),
                position: [x, y, z],
                enter: get(section, "ENTERSHEEP").map(fn_name),
                exit: get(section, "EXITSHEEP").map(fn_name),
            };
            out.entry(head.to_string())
                .or_default()
                .zooms
                .insert(scene[..3.min(scene.len())].to_string(), zoom);
        }
    }
    out
}

/// Which way to point, from a lookout, to zoom to a place: yaw and pitch
/// ranges in degrees, yaw taken from 0 to 360. G-Engine's table.
pub fn aim(lookout: &str, place: &str) -> Option<((f32, f32), (f32, f32))> {
    Some(match (lookout, place) {
        ("CD1", "PL3") => ((73.0, 80.0), (0.0, 5.0)), // Orange Rock
        ("CD1", "LHM") => ((108.0, 121.0), (11.0, 14.0)), // L'Homme Mort
        ("CD1", "CSD") => ((108.0, 120.0), (2.0, 11.0)), // Coume Sourde
        ("CD1", "MA3") => ((174.0, 190.0), (1.0, 7.0)), // Tour Magdala
        ("MA3", "CD1") => ((0.0, 20.0), (-5.0, 0.0)), // Chateau de Blanchefort
        _ => return None,
    })
}

/// How high the camera goes to see only sky.
pub const LIFT: f32 = 10000.0;
/// Panned out, the pitch stays within these, in radians.
pub const PITCH_RANGE: (f32, f32) = (-0.16259, 0.239742);
/// Zoomed in, how far from the place's own angle one may look, radians.
pub const ZOOMED_RANGE: (f32, f32) = (0.5, 0.2);
pub const ZOOMED_FOV: f32 = 30.0;

/// The place the camera points at, if one can be zoomed into from here.
pub fn target(lookout: &str, uses: &Lookout, yaw: f32, pitch: f32) -> Option<String> {
    let yaw = yaw.to_degrees().rem_euclid(360.0);
    let pitch = pitch.to_degrees();
    uses.zooms.keys().find_map(|place| {
        let ((y0, y1), (p0, p1)) = aim(lookout, place)?;
        (yaw >= y0 && yaw <= y1 && pitch >= p0 && pitch <= p1).then(|| place.clone())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &[u8] = b"// comment\n[CD1102P]\nLOC=LHM_a_a,PL3\nANIM=GabPutBinoc\n\
        [CD1102PLHM_a_a]\nZOOMRECT=118,1,126,6\nCAMANGLE=258.19,0.00\nCAMPOS=562.72,61,86.287\n\
        ENTERSHEEP=CD1102PLHMEnt\nEXITSHEEP=CD1102PLHMExt\nFLOOR=lhm_floor\n\
        [CD1102pPL3]\nCAMANGLE=127.84,-3.00\nCAMPOS=-53.6,29.85,358.8\nENTERSHEEP=CD1ALLPL3Ent\n";

    #[test]
    fn reads_lookouts() {
        let all = parse(TEXT);
        let cd1 = &all["CD1102P"];
        assert_eq!(cd1.put_away.as_deref(), Some("GabPutBinoc"));
        let lhm = &cd1.zooms["LHM"];
        assert_eq!(lhm.scene, "LHM_A_A");
        assert_eq!(lhm.angle, (258.19, 0.0));
        assert_eq!(lhm.enter.as_deref(), Some("CD1102PLHMEnt$"));
        assert_eq!(cd1.zooms["PL3"].exit, None);
    }

    #[test]
    fn pointing_finds_places() {
        let all = parse(TEXT);
        let cd1 = &all["CD1102P"];
        assert_eq!(
            target("CD1", cd1, 115f32.to_radians(), 12f32.to_radians()).as_deref(),
            Some("LHM")
        );
        assert_eq!(
            target(
                "CD1",
                cd1,
                (115.0f32 - 360.0).to_radians(),
                12f32.to_radians()
            )
            .as_deref(),
            Some("LHM")
        );
        assert_eq!(target("CD1", cd1, 10f32.to_radians(), 0.0), None);
        // Coume Sourde is that way too, but not listed for this lookout.
        assert_eq!(
            target("CD1", cd1, 115f32.to_radians(), 5f32.to_radians()),
            None
        );
    }
}
