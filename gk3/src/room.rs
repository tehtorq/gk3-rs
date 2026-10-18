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
    /// One of the room's own angles (`[ROOM_CAMERAS]`), which the player
    /// may choose from the option bar.
    pub room: bool,
    /// A close-up of a noun or model (`[INSPECT_CAMERAS]`), named after it.
    pub inspect: bool,
}

pub struct Room {
    /// The scene asset's name, e.g. `R25_M`.
    pub name: String,
    /// The scene the room was built from, before any script swapped it
    /// (`SetScene`).
    pub built_as: String,
    pub location: Option<String>,
    pub mesh: SceneMesh,
    pub cameras: Vec<RoomCamera>,
    /// The location file, for whatever else is placed in the room.
    pub sif: Option<Document>,
    pub bsp: Bsp,
    bsp_name: String,
    lightmaps: Vec<Image>,
    textures: HashMap<String, Image>,
    sky: Option<Sky>,
    /// BSP objects not drawn, upper-cased.
    pub hidden: HashSet<String>,
    /// BSP objects painted with another texture, by upper-cased name.
    retextured: HashMap<String, String>,
    /// Problems worth knowing about that did not stop the load.
    pub warnings: Vec<String>,
}

/// Where the original samples the floor's light around a model: under it,
/// and at six points 12 away and six 24 away, in two hexagons (0x4b08a5).
const FLOOR_LIGHT_POINTS: [(f32, f32); 13] = [
    (0.0, 0.0),
    (0.0, 12.0),
    (-10.392, -6.0),
    (10.392, -6.0),
    (0.0, -12.0),
    (-10.392, 6.0),
    (10.392, 6.0),
    (0.0, 24.0),
    (-20.784, -12.0),
    (20.784, -12.0),
    (0.0, -24.0),
    (-20.784, 12.0),
    (20.784, 12.0),
];

impl Room {
    /// The light on the floor around a point, as the original lights a
    /// model by (0x4b08a5): the lightmap colour where the walk floor
    /// (`floor`) lies straight below each of thirteen points about it,
    /// averaged, 0 to 1. None if the floor lies below none of them.
    pub fn floor_light(&self, floor: &str, at: [f32; 3]) -> Option<[f32; 3]> {
        let object = self.bsp.objects.iter().position(|o| o.eq_ignore_ascii_case(floor))?;
        let mut sum = [0.0f32; 3];
        let mut hits = 0;
        for (dx, dz) in FLOOR_LIGHT_POINTS {
            if let Some(c) = self.lightmap_below(object, [at[0] + dx, at[1], at[2] + dz]) {
                for i in 0..3 {
                    sum[i] += c[i];
                }
                hits += 1;
            }
        }
        (hits > 0).then(|| sum.map(|s| s / hits as f32))
    }

    /// The lightmap colour where a BSP object lies nearest straight below a
    /// point, blended between its four nearest texels, 0 to 1.
    fn lightmap_below(&self, object: usize, at: [f32; 3]) -> Option<[f32; 3]> {
        let bsp = &self.bsp;
        let mut best: Option<(f32, usize, [f32; 2])> = None;
        for polygon in &bsp.polygons {
            let s = polygon.surface as usize;
            if bsp.surfaces.get(s).is_none_or(|surface| surface.object as usize != object) {
                continue;
            }
            for [a, b, c] in bsp.triangles(polygon) {
                let [pa, pb, pc] = [a, b, c].map(|i| bsp.vertices[i as usize]);
                // Barycentric coordinates of the point over the triangle, seen
                // from above.
                let det = (pb[2] - pc[2]) * (pa[0] - pc[0]) + (pc[0] - pb[0]) * (pa[2] - pc[2]);
                if det.abs() < 1e-6 {
                    continue;
                }
                let wa = ((pb[2] - pc[2]) * (at[0] - pc[0]) + (pc[0] - pb[0]) * (at[2] - pc[2])) / det;
                let wb = ((pc[2] - pa[2]) * (at[0] - pc[0]) + (pa[0] - pc[0]) * (at[2] - pc[2])) / det;
                let wc = 1.0 - wa - wb;
                if wa < 0.0 || wb < 0.0 || wc < 0.0 {
                    continue;
                }
                let y = wa * pa[1] + wb * pb[1] + wc * pc[1];
                // A ray down from the point: what lies below it, nearest
                // first (with a little room for a point on the floor itself).
                if y > at[1] + 1.0 || best.is_some_and(|(by, _, _)| by >= y) {
                    continue;
                }
                let [ua, ub, uc] = [a, b, c].map(|i| bsp.uv(i));
                let uv = [
                    wa * ua[0] + wb * ub[0] + wc * uc[0],
                    wa * ua[1] + wb * ub[1] + wc * uc[1],
                ];
                best = Some((y, s, uv));
            }
        }
        let (_, s, uv) = best?;
        let surface = &bsp.surfaces[s];
        let map = self.lightmaps.get(s)?;
        if map.width == 0 || map.height == 0 {
            return None;
        }
        let u = (uv[0] + surface.lightmap_offset[0]) * surface.lightmap_scale[0] * map.width as f32 - 0.5;
        let v = (uv[1] + surface.lightmap_offset[1]) * surface.lightmap_scale[1] * map.height as f32 - 0.5;
        let (x0, y0) = (u.floor(), v.floor());
        let (fx, fy) = (u - x0, v - y0);
        let texel = |x: f32, y: f32| {
            let x = (x.max(0.0) as u32).min(map.width - 1);
            let y = (y.max(0.0) as u32).min(map.height - 1);
            map.pixel(x, y)
        };
        let corners = [texel(x0, y0), texel(x0 + 1.0, y0), texel(x0, y0 + 1.0), texel(x0 + 1.0, y0 + 1.0)];
        let mut out = [0.0f32; 3];
        for (i, o) in out.iter_mut().enumerate() {
            let top = corners[0][i] as f32 * (1.0 - fx) + corners[1][i] as f32 * fx;
            let bottom = corners[2][i] as f32 * (1.0 - fx) + corners[3][i] as f32 * fx;
            *o = (top * (1.0 - fy) + bottom * fy) / 255.0;
        }
        Some(out)
    }

    /// Paints a BSP object with another texture, as an animation's
    /// `[STEXTURES]` do; true if that changed anything (then rebuild).
    /// The pieces of the room repainted, and with what: for a saved game.
    pub fn retextures(&self) -> Vec<(String, String)> {
        let mut all: Vec<_> = self.retextured.iter().map(|(o, t)| (o.clone(), t.clone())).collect();
        all.sort();
        all
    }

    pub fn retexture(&mut self, lib: &Library, object: &str, texture: &str) -> bool {
        let (object, texture) = (object.to_ascii_uppercase(), texture.to_ascii_uppercase());
        if !self
            .bsp
            .objects
            .iter()
            .any(|o| o.eq_ignore_ascii_case(&object))
            || self.retextured.get(&object) == Some(&texture)
        {
            return false;
        }
        if !self.textures.contains_key(&texture) {
            let Some(image) = load_texture(lib, &texture) else {
                return false;
            };
            self.textures.insert(texture.clone(), image);
        }
        self.retextured.insert(object, texture);
        true
    }

    /// Swaps in another scene asset for the same location, as a script's
    /// `SetScene` does: GRI's lights going on, CEM's storm, RL2's disco.
    /// The lightmaps (and geometry, if it names other) are the new scene's;
    /// what the story has hidden or repainted stays so. Then rebuild.
    pub fn set_scene(&mut self, lib: &Library, scene: &str) -> Result<(), String> {
        let scene = scene.to_ascii_uppercase();
        let scn = lib
            .read(&format!("{scene}.SCN"))
            .map_err(|e| e.to_string())?;
        let scn = Document::parse(&scn, Split::Whole);
        let bsp_name = bsp_name(&scn, &scene);
        if bsp_name != self.bsp_name {
            let bytes = lib
                .read(&format!("{bsp_name}.BSP"))
                .map_err(|e| e.to_string())?;
            self.bsp = Bsp::parse(&bytes).map_err(|e| format!("{bsp_name}.BSP: {e}"))?;
            for surface in &self.bsp.surfaces {
                let name = surface.texture.to_ascii_uppercase();
                self.textures.entry(name).or_insert_with_key(|name| {
                    load_texture(lib, name).unwrap_or(Image {
                        width: 0,
                        height: 0,
                        rgba: Vec::new(),
                    })
                });
            }
            self.bsp_name = bsp_name;
        }
        self.lightmaps = match lib.read(&format!("{scene}.MUL")) {
            Ok(bytes) => lightmap::parse(&bytes).map_err(|e| format!("{scene}.MUL: {e}"))?,
            Err(_) => Vec::new(),
        };
        if let Some(sky) = load_sky(lib, &scn) {
            self.sky = Some(sky);
        }
        self.name = scene;
        Ok(())
    }

    /// Rebuilds the drawable mesh, after objects are shown or hidden.
    pub fn rebuild(&mut self) {
        let texture = |name: &str| {
            self.textures
                .get(&name.to_ascii_uppercase())
                .filter(|i| i.width > 0)
                .cloned()
        };
        let is_hidden = |object: &str| self.hidden.contains(&object.to_ascii_uppercase());
        let retexture = |object: &str| self.retextured.get(&object.to_ascii_uppercase()).cloned();
        self.mesh = SceneMesh::build(SceneSource {
            bsp: &self.bsp,
            lightmaps: &self.lightmaps,
            texture: &texture,
            hidden: &is_hidden,
            retexture: &retexture,
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
    /// The story as it stands, for conditions on more than the time (who is
    /// where, where the ego came from, what has been done). Without it they
    /// are judged as at the start of the game.
    pub state: Option<GameState>,
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
            state: None,
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
        let mut state = match &self.state {
            Some(s) => {
                let mut s = s.clone();
                s.timeblock = time.clone();
                if s.location != gk3_game::state::key(location) {
                    s.move_to(location);
                }
                s
            }
            None => GameState::new(time, location),
        };
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
        // A location's file says which scene it is at this time. Some
        // locations share their name with an old scene asset (DIN), so the
        // file is asked first; a bare scene name is for when there is none.
        let from_location = self.location_file(lib, &upper).and_then(|sif| {
            let mut scenes = scene_lines(&sif)
                .into_iter()
                .filter(|s| self.scenes.contains(s));
            // At a given time a later section overrides an earlier one;
            // without one, the first listed is as good as any.
            if self.time.is_some() {
                scenes.next_back()
            } else {
                scenes.next()
            }
        });
        from_location.or_else(|| self.scenes.contains(&upper).then_some(upper))
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

        let bsp_name = bsp_name(&scn, &scene);
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

        let sky = load_sky(lib, &scn);

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
            built_as: scene.clone(),
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
            bsp_name,
            lightmaps,
            textures,
            sky,
            hidden,
            retextured: HashMap::new(),
        };
        room.rebuild();
        Ok(room)
    }
}

/// The geometry a scene asset names; without a BSP line it shares the
/// scene's name.
fn bsp_name(scn: &Document, scene: &str) -> String {
    scn.sections
        .iter()
        .flat_map(|s| &s.lines)
        .find_map(|l| l.get("BSP"))
        .unwrap_or(scene)
        .to_ascii_uppercase()
}

/// A scene asset's skybox, if it has one.
fn load_sky(lib: &Library, scn: &Document) -> Option<Sky> {
    scn.section("SKYBOX").and_then(|section| {
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
    })
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
    .flat_map(|name| sif.sections(name).map(move |s| (name, s)))
    .flat_map(|(name, s)| s.lines.iter().map(move |l| (name, l)));
    for (section, line) in lines {
        let (room, inspect) = (section == "ROOM_CAMERAS", section == "INSPECT_CAMERAS");
        let Some(first) = line.pairs.first() else {
            continue;
        };
        let (Some(angle), Some(position)) = (
            line.get("angle").and_then(floats::<2>),
            line.get("pos").and_then(floats::<3>),
        ) else {
            continue;
        };
        // Inspection cameras are named by the noun (or model) they look at.
        let name = if first.key.eq_ignore_ascii_case("noun") || first.key.eq_ignore_ascii_case("model") {
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
            room,
            inspect,
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

    /// Who is in a room depends on the story, not just the time: Emilio is
    /// on his bench in the square unless the player has just come out of
    /// the hotel, where he is.
    #[test]
    fn conditions_are_judged_on_the_story_so_far() {
        let sif = || {
            Document::parse(
                b"[ACTORS={IsActorAtLocation(\"EMILIO\", \"RC1\") || (WasLastLocation(\"LBY\")==0)}]\n\
                  model=eml, noun=EMILIO\n",
                Split::Commas,
            )
        };
        let mut catalog = Catalog {
            scenes: Vec::new(),
            location_of: HashMap::new(),
            time: Some("110A".into()),
            state: None,
        };
        let mut state = GameState::new("110A", "R25");
        state.move_to("LBY");
        catalog.state = Some(state);
        assert!(catalog.at_time(sif(), "RC1").sections.is_empty());
        let mut state = GameState::new("110A", "R25");
        state.move_to("RC2");
        catalog.state = Some(state);
        assert_eq!(catalog.at_time(sif(), "RC1").sections.len(), 1);
    }

    /// A location named like an old scene asset still gets the scene its
    /// file gives for the time: the dining room at 10am is not bare `DIN`.
    #[test]
    fn locations_before_scene_names() {
        let Some(dir) = std::env::var_os("GK3_DATA") else {
            return;
        };
        let lib = Library::open_dir(dir).unwrap();
        let mut catalog = Catalog::new(&lib);
        catalog.time = Some("110A".into());
        assert_eq!(
            catalog.resolve(&lib, "DIN").as_deref(),
            Some("DIN_110A_112P(A)")
        );
        // A scene's own name still works where no location has it.
        assert_eq!(catalog.resolve(&lib, "R25_M").as_deref(), Some("R25_M"));
    }

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
