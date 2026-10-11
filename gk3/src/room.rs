//! Assembling a room from the archives.
//!
//! A location's initialisation file (`R25.SIF`) names scene assets
//! (`scene=r25_m`), one per time of day. A scene asset (`R25_M.SCN`) names the
//! geometry (`bsp=r25`), the skybox and which objects start hidden, and shares
//! its name with the lightmap set (`R25_M.MUL`). Cameras come from the
//! location file.

use std::collections::{HashMap, HashSet};

use gk3_barn::Library;
use gk3_formats::bsp::Bsp;
use gk3_formats::image::{self, Image};
use gk3_formats::ini::{Document, Split};
use gk3_formats::lightmap;
use gk3_render::{SceneMesh, SceneSource, Sky};

/// A named viewpoint from a location's `[ROOM_CAMERAS]`.
#[derive(Debug, Clone, PartialEq)]
pub struct RoomCamera {
    pub name: String,
    pub position: [f32; 3],
    /// Degrees.
    pub yaw: f32,
    /// Degrees; positive looks down.
    pub pitch: f32,
    pub default: bool,
}

pub struct Room {
    /// The scene asset's name, e.g. `R25_M`.
    pub name: String,
    pub location: Option<String>,
    pub mesh: SceneMesh,
    pub cameras: Vec<RoomCamera>,
    /// Problems worth knowing about that did not stop the load.
    pub warnings: Vec<String>,
}

/// Which scene assets exist and which location each belongs to.
pub struct Catalog {
    /// Scene asset names, sorted.
    pub scenes: Vec<String>,
    location_of: HashMap<String, String>,
}

impl Catalog {
    pub fn new(lib: &Library) -> Self {
        let mut scenes = Vec::new();
        let mut location_of: HashMap<String, String> = HashMap::new();
        for (barn, entry) in lib.assets() {
            let upper = entry.name.to_ascii_uppercase();
            if let Some(stem) = upper.strip_suffix(".SCN") {
                scenes.push(stem.to_string());
            } else if let Some(location) = upper.strip_suffix(".SIF") {
                let Ok(bytes) = barn.read(entry) else {
                    continue;
                };
                // Several locations can show the same scene (RC1_A_M is also
                // what Room 31's balcony looks out on); the one whose name the
                // scene starts with owns it.
                for scene in scene_lines(&Document::parse(&bytes, Split::Commas)) {
                    let owns = scene.starts_with(location);
                    match location_of.get(&scene) {
                        Some(current) if !owns || scene.starts_with(current.as_str()) => {}
                        _ => {
                            location_of.insert(scene, location.to_string());
                        }
                    }
                }
            }
        }
        scenes.sort();
        Self {
            scenes,
            location_of,
        }
    }

    /// Resolves what a user typed: a scene asset (`R25_M`), a location
    /// (`R25`, meaning its first scene), or a bare BSP.
    pub fn resolve(&self, lib: &Library, name: &str) -> Option<String> {
        let upper = name.to_ascii_uppercase();
        if self.scenes.contains(&upper) {
            return Some(upper);
        }
        let sif = lib.read(&format!("{upper}.SIF")).ok()?;
        scene_lines(&Document::parse(&sif, Split::Commas))
            .into_iter()
            .find(|s| self.scenes.contains(s))
    }

    /// Whether any location uses this scene. A few unused scene assets point
    /// at geometry the retail data does not contain (TE2A and TE2B).
    pub fn is_used(&self, scene: &str) -> bool {
        self.location_of.contains_key(&scene.to_ascii_uppercase())
    }

    pub fn load(&self, lib: &Library, scene: &str) -> Result<Room, String> {
        let scene = scene.to_ascii_uppercase();
        let mut warnings = Vec::new();
        let scn = lib
            .read(&format!("{scene}.SCN"))
            .map_err(|e| e.to_string())?;
        let scn = Document::parse(&scn, Split::Whole);

        let bsp_name = scn
            .sections
            .iter()
            .flat_map(|s| &s.lines)
            .find_map(|l| l.get("BSP"))
            // Without a BSP line the geometry shares the scene's name.
            .unwrap_or(&scene)
            .to_ascii_uppercase();
        let bsp_bytes = lib
            .read(&format!("{bsp_name}.BSP"))
            .map_err(|e| e.to_string())?;
        let bsp = Bsp::parse(&bsp_bytes).map_err(|e| format!("{bsp_name}.BSP: {e}"))?;

        let lightmaps = match lib.read(&format!("{scene}.MUL")) {
            Ok(bytes) => lightmap::parse(&bytes).map_err(|e| format!("{scene}.MUL: {e}"))?,
            Err(_) => {
                warnings.push(format!("no lightmaps ({scene}.MUL)"));
                Vec::new()
            }
        };
        if !lightmaps.is_empty() && lightmaps.len() < bsp.surfaces.len() {
            warnings.push(format!(
                "{} lightmaps for {} surfaces",
                lightmaps.len(),
                bsp.surfaces.len()
            ));
        }

        // The scene asset's [MODELS] values are not visibility: LBY_M marks
        // its own walls and floor 0. Hidden objects come from the location.
        let mut hidden: HashSet<String> = HashSet::new();

        let location = self.location_of.get(&scene).cloned();
        let mut cameras = Vec::new();
        if let Some(location) = &location
            && let Ok(sif) = lib.read(&format!("{location}.SIF"))
        {
            let sif = Document::parse(&sif, Split::Commas);
            cameras = room_cameras(&sif);
            hidden.extend(hidden_models(&sif));
        }

        let sky = scn.section("SKYBOX").and_then(|section| {
            let get = |key: &str| {
                section
                    .lines
                    .iter()
                    .find_map(|l| l.get(key))
                    .and_then(|name| load_texture(lib, name))
            };
            let azimuth = section
                .lines
                .iter()
                .find_map(|l| l.get("Azimuth"))
                .and_then(|a| a.parse().ok())
                .unwrap_or(0.0);
            Sky::from_named(
                get("Left"),
                get("Right"),
                get("Front"),
                get("Back"),
                get("Up"),
                get("Down"),
                azimuth,
            )
        });

        let missing = std::cell::RefCell::new(Vec::new());
        let texture = |name: &str| {
            let found = load_texture(lib, name);
            if found.is_none() {
                missing.borrow_mut().push(name.to_string());
            }
            found
        };
        let is_hidden = |object: &str| hidden.contains(&object.to_ascii_uppercase());
        let mesh = SceneMesh::build(SceneSource {
            bsp: &bsp,
            lightmaps: &lightmaps,
            texture: &texture,
            hidden: &is_hidden,
            sky,
        });
        let missing = missing.into_inner();
        if !missing.is_empty() {
            warnings.push(format!("missing textures: {}", missing.join(", ")));
        }

        Ok(Room {
            name: scene,
            location,
            mesh,
            cameras,
            warnings,
        })
    }
}

fn load_texture(lib: &Library, name: &str) -> Option<Image> {
    let bytes = lib
        .read(&format!("{name}.BMP"))
        .or_else(|_| lib.read(name))
        .ok()?;
    image::decode(&bytes).ok()
}

/// Every `scene=` value in a location file, upper-cased, in file order.
fn scene_lines(sif: &Document) -> Vec<String> {
    sif.sections("GENERAL")
        .flat_map(|s| &s.lines)
        .filter_map(|l| l.get("scene"))
        .map(|s| s.to_ascii_uppercase())
        .collect()
}

/// Objects hidden regardless of story state: those in unconditional
/// `[MODELS]` sections marked `hidden`, and invisible click regions.
fn hidden_models(sif: &Document) -> Vec<String> {
    let mut hidden = Vec::new();
    for section in sif.sections("MODELS") {
        for line in &section.lines {
            let Some(model) = line.get("model") else {
                continue;
            };
            let hit_test = line
                .get("type")
                .is_some_and(|t| t.eq_ignore_ascii_case("HitTest"));
            let flagged = line
                .pairs
                .iter()
                .any(|p| p.is_flag() && p.key.eq_ignore_ascii_case("hidden"));
            if hit_test || (flagged && section.condition.is_none()) {
                hidden.push(model.to_ascii_uppercase());
            }
        }
    }
    hidden
}

fn room_cameras(sif: &Document) -> Vec<RoomCamera> {
    let mut cameras = Vec::new();
    for line in sif.sections("ROOM_CAMERAS").flat_map(|s| &s.lines) {
        let Some(first) = line.pairs.first() else {
            continue;
        };
        let (Some(angle), Some(position)) = (
            line.get("angle").and_then(floats::<2>),
            line.get("pos").and_then(floats::<3>),
        ) else {
            continue;
        };
        cameras.push(RoomCamera {
            name: first.key.clone(),
            position,
            yaw: angle[0],
            pitch: angle[1],
            default: line
                .pairs
                .iter()
                .any(|p| p.is_flag() && p.key.eq_ignore_ascii_case("default")),
        });
    }
    cameras
}

/// Parses `{1, 2, 3}` into numbers.
fn floats<const N: usize>(text: &str) -> Option<[f32; N]> {
    let inner = text.trim().trim_start_matches('{').trim_end_matches('}');
    let values: Vec<f32> = inner
        .split(',')
        .map(|v| v.trim().parse().ok())
        .collect::<Option<_>>()?;
    values.try_into().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_vectors() {
        assert_eq!(
            floats::<3>("{277.58, 61.70, 50.57}"),
            Some([277.58, 61.70, 50.57])
        );
        assert_eq!(floats::<2>("{1,2,3}"), None);
    }

    #[test]
    fn reads_cameras_and_hidden_models() {
        let sif = Document::parse(
            b"[ROOM_CAMERAS]\n\
              START,        angle={-46.97, 0.90}, pos={277.58, 61.70, 50.57}, Default\n\
              DESK,  angle={-108.38, 0.00},pos={164.14, 58.08, 106.80}\n\
              [MODELS]\nmodel=RC1_HOTEL_01, type=scene, hidden\n\
              model=gabeswindow_hittest, noun=WINDOW, type=HitTest, verb=EXIT\n\
              model=r25couch, noun=COUCH, type=scene\n\
              [MODELS={IsCurrentTime(\"207a\")}]\nmodel=r25_blanket, type=scene, hidden\n",
            Split::Commas,
        );
        let cameras = room_cameras(&sif);
        assert_eq!(cameras.len(), 2);
        assert!(cameras[0].default && !cameras[1].default);
        assert_eq!(cameras[1].yaw, -108.38);
        assert_eq!(hidden_models(&sif), ["RC1_HOTEL_01", "GABESWINDOW_HITTEST"]);
    }
}
