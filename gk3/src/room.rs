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
use gk3_game::api::GameApi;
use gk3_game::state::GameState;
use gk3_render::{SceneMesh, SceneSource, Sky};
use gk3_sheep::expr::Expr;

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
    /// Vertical field of view in degrees; close-ups narrow it.
    pub fov: f32,
    /// For dialogue cameras: the conversation they belong to, and whether
    /// they open it or close it.
    pub dialogue: Option<String>,
    pub initial: bool,
    pub last: bool,
}

pub struct Room {
    /// The scene asset's name, e.g. `R25_M`.
    pub name: String,
    pub location: Option<String>,
    pub mesh: SceneMesh,
    pub cameras: Vec<RoomCamera>,
    /// The location file, for whatever else is placed in the room.
    pub sif: Option<Document>,
    pub bsp: Bsp,
    lightmaps: Vec<Image>,
    textures: HashMap<String, Image>,
    sky: Option<Sky>,
    /// BSP objects not drawn, upper-cased.
    pub hidden: HashSet<String>,
    /// Problems worth knowing about that did not stop the load.
    pub warnings: Vec<String>,
}

impl Room {
    /// Rebuilds the drawable mesh, after objects are shown or hidden.
    pub fn rebuild(&mut self) {
        let texture = |name: &str| {
            self.textures
                .get(&name.to_ascii_uppercase())
                .filter(|i| i.width > 0)
                .cloned()
        };
        let is_hidden = |object: &str| self.hidden.contains(&object.to_ascii_uppercase());
        self.mesh = SceneMesh::build(SceneSource {
            bsp: &self.bsp,
            lightmaps: &self.lightmaps,
            texture: &texture,
            hidden: &is_hidden,
            sky: self.sky.clone(),
        });
    }
}

/// Which scene assets exist and which location each belongs to.
pub struct Catalog {
    /// Scene asset names, sorted.
    pub scenes: Vec<String>,
    location_of: HashMap<String, String>,
    /// The story time conditions are judged at, such as `110A`. Without one,
    /// every section of a location file applies.
    pub time: Option<String>,
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
            time: None,
        }
    }

    /// A location's initialisation file as it stands at the catalog's time:
    /// the general file (`R25.SIF`), then the timeblock's own (`R25110A.SIF`),
    /// whose sections add to and override it.
    pub fn location_file(&self, lib: &Library, location: &str) -> Option<Document> {
        let location = location.to_ascii_uppercase();
        let general = Document::parse(&lib.read(&format!("{location}.SIF")).ok()?, Split::Commas);
        let mut doc = self.at_time(general, &location);
        let Some(time) = &self.time else {
            return Some(doc);
        };
        let specific = lib
            .read(&format!("{location}{time}.SIF"))
            .ok()
            .map(|bytes| self.at_time(Document::parse(&bytes, Split::Commas), &location));
        // Who the player is, as G-Engine decides: the timeblock's file says
        // if it names one, else the location's. Anyone else marked as the
        // player is not in the scene at all.
        let ego = specific
            .as_ref()
            .and_then(ego_model)
            .or_else(|| ego_model(&doc));
        if let Some(specific) = specific {
            doc.sections.extend(specific.sections);
        }
        if let Some(ego) = ego {
            for section in doc
                .sections
                .iter_mut()
                .filter(|s| s.name.eq_ignore_ascii_case("ACTORS"))
            {
                section.lines.retain(|l| {
                    !is_ego(l) || l.get("model").is_some_and(|m| m.eq_ignore_ascii_case(&ego))
                });
            }
        }
        Some(doc)
    }

    /// The location file as it stands at the catalog's time: sections whose
    /// condition is false are dropped, and the rest become unconditional.
    fn at_time(&self, sif: Document, location: &str) -> Document {
        let Some(time) = &self.time else { return sif };
        let scripts = HashMap::new();
        let mut state = GameState::new(time, location);
        let mut api = GameApi::new(&mut state, &scripts);
        let sections = sif
            .sections
            .into_iter()
            .filter_map(|mut section| {
                if let Some(condition) = section.condition.take() {
                    let holds = Expr::parse(&condition)
                        .map(|e| e.eval(&mut api, &|_| None).truthy())
                        .unwrap_or(false);
                    if !holds {
                        return None;
                    }
                }
                Some(section)
            })
            .collect();
        Document { sections }
    }

    /// Resolves what a user typed: a scene asset (`R25_M`), a location
    /// (`R25`, meaning its first scene), or a bare BSP.
    pub fn resolve(&self, lib: &Library, name: &str) -> Option<String> {
        let upper = name.to_ascii_uppercase();
        if self.scenes.contains(&upper) {
            return Some(upper);
        }
        let sif = self.location_file(lib, &upper)?;
        let mut scenes = scene_lines(&sif)
            .into_iter()
            .filter(|s| self.scenes.contains(s));
        // At a given time a later section overrides an earlier one; without
        // one, the first listed is as good as any.
        if self.time.is_some() {
            scenes.next_back()
        } else {
            scenes.next()
        }
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
        let mut sif_doc = None;
        if let Some(location) = &location
            && let Some(sif) = self.location_file(lib, location)
        {
            cameras = room_cameras(&sif);
            hidden.extend(hidden_models(&sif));
            sif_doc = Some(sif);
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

        let mut textures = HashMap::new();
        let mut missing = Vec::new();
        for surface in &bsp.surfaces {
            let name = surface.texture.to_ascii_uppercase();
            if textures.contains_key(&name) {
                continue;
            }
            match load_texture(lib, &name) {
                Some(image) => {
                    textures.insert(name, image);
                }
                None => {
                    missing.push(name.clone());
                    textures.insert(
                        name,
                        Image {
                            width: 0,
                            height: 0,
                            rgba: Vec::new(),
                        },
                    );
                }
            }
        }
        if !missing.is_empty() {
            warnings.push(format!("missing textures: {}", missing.join(", ")));
        }
        let mut room = Room {
            name: scene,
            location,
            mesh: SceneMesh {
                vertices: Vec::new(),
                batches: Vec::new(),
                textures: Vec::new(),
                lightmap_atlas: Image {
                    width: 1,
                    height: 1,
                    rgba: vec![255; 4],
                },
                sky: None,
                bounds: ([0.0; 3], [0.0; 3]),
            },
            cameras,
            sif: sif_doc,
            warnings,
            bsp,
            lightmaps,
            textures,
            sky,
            hidden,
        };
        room.rebuild();
        Ok(room)
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

fn is_ego(line: &gk3_formats::ini::Line) -> bool {
    line.pairs
        .iter()
        .any(|p| p.is_flag() && p.key.eq_ignore_ascii_case("ego"))
}

/// The model of the first actor a location file marks as the player.
fn ego_model(sif: &Document) -> Option<String> {
    sif.sections("ACTORS")
        .flat_map(|s| &s.lines)
        .find(|l| is_ego(l))
        .and_then(|l| l.get("model"))
        .map(|m| m.trim().to_ascii_uppercase())
}

/// Every named viewpoint: room cameras first (the ones the player cycles
/// through), then the cinematic, inspection and dialogue cameras scripts cut
/// to by name.
pub fn room_cameras(sif: &Document) -> Vec<RoomCamera> {
    let mut cameras = Vec::new();
    let lines = [
        "ROOM_CAMERAS",
        "CINEMATIC_CAMERAS",
        "INSPECT_CAMERAS",
        "DIALOGUE_CAMERAS",
    ]
    .into_iter()
    .flat_map(|name| sif.sections(name))
    .flat_map(|s| &s.lines);
    for line in lines {
        let Some(first) = line.pairs.first() else {
            continue;
        };
        let (Some(angle), Some(position)) = (
            line.get("angle").and_then(floats::<2>),
            line.get("pos").and_then(floats::<3>),
        ) else {
            continue;
        };
        // Inspection cameras are named by the noun they look at.
        let name = if first.key.eq_ignore_ascii_case("noun") {
            first.value.clone()
        } else {
            first.key.clone()
        };
        let flag = |name: &str| {
            line.pairs
                .iter()
                .any(|p| p.is_flag() && p.key.eq_ignore_ascii_case(name))
        };
        cameras.push(RoomCamera {
            name,
            position,
            yaw: angle[0],
            pitch: angle[1],
            default: flag("default"),
            fov: line
                .get("fov")
                .and_then(|f| f.trim().parse().ok())
                .unwrap_or(60.0),
            dialogue: line.get("dialogue").map(|d| d.trim().to_ascii_uppercase()),
            initial: flag("initial"),
            last: flag("final"),
        });
    }
    cameras
}

/// Parses `{1, 2, 3}` into numbers.
pub(crate) fn floats<const N: usize>(text: &str) -> Option<[f32; N]> {
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
              [DIALOGUE_CAMERAS]\n\
              CLOSE, angle={1, 2}, pos={1, 2, 3}, FOV=20 , dialogue=GabeJean1\n\
              WIDE, angle={1, 2}, pos={1, 2, 3}, dialogue=GABEJEAN1, initial, final\n\
              [MODELS]\nmodel=RC1_HOTEL_01, type=scene, hidden\n\
              model=gabeswindow_hittest, noun=WINDOW, type=HitTest, verb=EXIT\n\
              model=r25couch, noun=COUCH, type=scene\n\
              [MODELS={IsCurrentTime(\"207a\")}]\nmodel=r25_blanket, type=scene, hidden\n",
            Split::Commas,
        );
        let cameras = room_cameras(&sif);
        assert_eq!(cameras.len(), 4);
        assert!(cameras[0].default && !cameras[1].default);
        assert_eq!(cameras[1].yaw, -108.38);
        assert_eq!((cameras[1].fov, cameras[2].fov), (60.0, 20.0));
        assert_eq!(cameras[2].dialogue.as_deref(), Some("GABEJEAN1"));
        assert!(!cameras[2].initial && cameras[3].initial && cameras[3].last);
        assert_eq!(hidden_models(&sif), ["RC1_HOTEL_01", "GABESWINDOW_HITTEST"]);
    }
}
