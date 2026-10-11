//! `gk3 check`: parse every asset with the decoder for its kind.

use std::collections::{BTreeMap, HashMap};
use std::process::ExitCode;
use std::sync::Mutex;
use std::time::Instant;

use gk3_barn::Library;
use gk3_formats::bsp::Bsp;
use gk3_formats::ini::{Document, Split};
use gk3_formats::model::Model;
use gk3_formats::{Kind, image, lightmap};

/// Inconsistencies in the retail data itself, which the engine has to
/// tolerate. Reported, but not counted as failures.
const KNOWN_QUIRKS: &[(&str, &str)] = &[(
    "PL6_A_A",
    "PL6_A_A.MUL has 830 lightmaps for 821 surfaces; the last 9 are unused",
)];

#[derive(Default)]
struct Tally {
    count: usize,
    parsed: usize,
    /// Kinds with no decoder yet are counted, not parsed.
    unparsed: usize,
    bytes: usize,
    /// A one-line summary of what was found, e.g. triangle counts.
    detail: Detail,
}

#[derive(Default)]
struct Detail {
    triangles: usize,
    vertices: usize,
    images: usize,
    pixels: usize,
    seconds: f64,
    mp3: usize,
    /// MP3s whose decoded length matches their `fact` chunk exactly, within
    /// one MP3 frame (1,152 samples), and further off.
    fact_exact: usize,
    fact_close: usize,
    fact_off: usize,
    sections: usize,
    keyframes: usize,
    models: std::collections::BTreeSet<String>,
}

/// What one asset produced, for the cross-checks after the parallel pass.
enum Parsed {
    Scene {
        surfaces: usize,
    },
    Lightmap {
        maps: usize,
    },
    /// A scene asset (`.SCN`) and the BSP it names.
    SceneAsset {
        bsp: Option<String>,
    },
    Other,
}

pub fn run(lib: &Library, skip_audio: bool) -> Result<ExitCode, String> {
    let start = Instant::now();
    let assets = lib.assets();
    let tallies: Mutex<BTreeMap<Kind, Tally>> = Mutex::default();
    let failures = Mutex::new(Vec::new());
    let scenes: Mutex<HashMap<String, usize>> = Mutex::default();
    let lightmaps: Mutex<Vec<(String, usize)>> = Mutex::default();
    let scene_assets: Mutex<HashMap<String, String>> = Mutex::default();

    crate::for_each_parallel(&assets, |(barn, entry)| {
        let bytes = match barn.read(entry) {
            Ok(b) => b,
            Err(e) => {
                failures.lock().unwrap().push(e.to_string());
                return;
            }
        };
        let kind = Kind::of(&bytes);
        let mut detail = Detail::default();
        let outcome = parse(kind, &entry.name, &bytes, skip_audio, &mut detail);

        let mut tallies = tallies.lock().unwrap();
        let tally = tallies.entry(kind).or_default();
        tally.count += 1;
        tally.bytes += bytes.len();
        match outcome {
            Ok(None) => tally.unparsed += 1,
            Ok(Some(parsed)) => {
                tally.parsed += 1;
                let d = &mut tally.detail;
                d.triangles += detail.triangles;
                d.vertices += detail.vertices;
                d.images += detail.images;
                d.pixels += detail.pixels;
                d.seconds += detail.seconds;
                d.mp3 += detail.mp3;
                d.fact_exact += detail.fact_exact;
                d.fact_close += detail.fact_close;
                d.fact_off += detail.fact_off;
                d.sections += detail.sections;
                d.keyframes += detail.keyframes;
                d.models.extend(std::mem::take(&mut detail.models));
                let stem = stem(&entry.name);
                match parsed {
                    Parsed::Scene { surfaces } => {
                        scenes.lock().unwrap().insert(stem, surfaces);
                    }
                    Parsed::Lightmap { maps } => lightmaps.lock().unwrap().push((stem, maps)),
                    Parsed::SceneAsset { bsp: Some(bsp) } => {
                        scene_assets
                            .lock()
                            .unwrap()
                            .insert(stem, bsp.to_ascii_uppercase());
                    }
                    Parsed::SceneAsset { bsp: None } | Parsed::Other => {}
                }
            }
            Err(e) => failures
                .lock()
                .unwrap()
                .push(format!("{} ({kind:?}): {e}", entry.name)),
        }
    });

    // Lightmap i lights surface i, so a set must have one map per surface of
    // its geometry. A set shares its name with a scene asset, whose BSP= line
    // names the geometry: CDB_E.MUL lights CDB_E.SCN, which is CDB_E_N.BSP.
    // Sets with no scene asset fall back to naming: B21 lights B21.BSP.
    let scenes = scenes.into_inner().unwrap();
    let scene_assets = scene_assets.into_inner().unwrap();
    let mut failures = failures.into_inner().unwrap();
    let mut matched = 0;
    let mut orphans = Vec::new();
    let mut quirks = Vec::new();
    for (set, maps) in lightmaps.into_inner().unwrap() {
        let scene = match scene_assets.get(&set) {
            Some(bsp) => scenes.get_key_value(bsp),
            None => scenes.get_key_value(&set),
        };
        match scene {
            Some((_, &surfaces)) if surfaces == maps => matched += 1,
            Some(_) if KNOWN_QUIRKS.iter().any(|(name, _)| *name == set) => quirks.push(set),
            Some((name, &surfaces)) => failures.push(format!(
                "{set}.MUL has {maps} lightmaps but {name}.BSP has {surfaces} surfaces"
            )),
            None => orphans.push(set),
        }
    }

    // Fonts: measure each sheet against its definition.
    let (mut fonts, mut settled) = (0, 0);
    let mut unsettled = Vec::new();
    for (barn, entry) in &assets {
        if !entry.name.to_ascii_uppercase().ends_with(".FON") {
            continue;
        }
        let Ok(bytes) = barn.read(entry) else {
            continue;
        };
        let def = gk3_formats::font::Definition::parse(&bytes);
        let sheet_name = def.bitmap.clone().unwrap_or_else(|| stem(&entry.name));
        let sheet_name = sheet_name.trim_end_matches(".bmp").trim_end_matches(".BMP");
        let Some(sheet) = lib
            .read(&format!("{sheet_name}.BMP"))
            .ok()
            .and_then(|b| image::decode_raw(&b).ok())
        else {
            failures.push(format!("{}: no sheet {sheet_name}.BMP", entry.name));
            continue;
        };
        fonts += 1;
        let font = gk3_formats::font::Font::new(&def, &sheet);
        let distinct: std::collections::HashSet<char> = def.characters.iter().copied().collect();
        if font.glyphs.len() == distinct.len() {
            settled += 1;
        } else {
            unsettled.push(format!(
                "{} ({} of {})",
                stem(&entry.name),
                font.glyphs.len(),
                distinct.len()
            ));
        }
    }
    println!("Fonts: {settled} of {fonts} measure exactly as many glyphs as they declare");
    if !unsettled.is_empty() {
        println!("  not yet: {}", unsettled.join(", "));
    }

    let tallies = tallies.into_inner().unwrap();
    println!(
        "{:<16} {:>7} {:>7} {:>9} {:>10}  details",
        "kind", "assets", "parsed", "not yet", "size"
    );
    let mut total = 0;
    for (kind, t) in &tallies {
        total += t.count;
        println!(
            "{:<16} {:>7} {:>7} {:>9} {:>10}  {}",
            format!("{kind:?}"),
            t.count,
            t.parsed,
            t.unparsed,
            crate::human(t.bytes as u64),
            describe(*kind, &t.detail, skip_audio)
        );
    }
    println!("{:<16} {:>7}", "total", total);
    println!(
        "\nLightmap sets matched to their scene's surface count: {matched}{}",
        if orphans.is_empty() {
            String::new()
        } else {
            orphans.sort();
            format!(" ({} with no scene: {})", orphans.len(), orphans.join(", "))
        }
    );
    for set in &quirks {
        if let Some((_, note)) = KNOWN_QUIRKS.iter().find(|(name, _)| name == set) {
            println!("Known retail quirk: {note}");
        }
    }
    println!("Failures: {}", failures.len());
    println!("Time: {:.1}s", start.elapsed().as_secs_f64());
    failures.sort();
    crate::report_failures(&failures)
}

fn parse(
    kind: Kind,
    name: &str,
    bytes: &[u8],
    skip_audio: bool,
    d: &mut Detail,
) -> Result<Option<Parsed>, String> {
    let e = |e: gk3_formats::Error| e.to_string();
    Ok(Some(match kind {
        Kind::Gk3Bitmap | Kind::WindowsBitmap => {
            let image = image::decode(bytes).map_err(e)?;
            d.images = 1;
            d.pixels = (image.width * image.height) as usize;
            Parsed::Other
        }
        Kind::Model => {
            let model = Model::parse(bytes).map_err(e)?;
            d.triangles = model.triangle_count();
            d.vertices = model.vertex_count();
            Parsed::Other
        }
        Kind::Scene => {
            let bsp = Bsp::parse(bytes).map_err(e)?;
            d.triangles = bsp.triangle_count();
            d.vertices = bsp.vertices.len();
            Parsed::Scene {
                surfaces: bsp.surfaces.len(),
            }
        }
        Kind::ActorAnimation => {
            let act = gk3_formats::act::VertexAnimation::parse(bytes).map_err(e)?;
            d.keyframes = act.frame_count as usize;
            d.models.insert(act.model.to_ascii_uppercase());
            Parsed::Other
        }
        Kind::Font => {
            // Measured in a second pass, which can look up the sheet.
            return Ok(None);
        }
        Kind::Lightmap => {
            let maps = lightmap::parse(bytes).map_err(e)?;
            d.images = maps.len();
            d.pixels = maps.iter().map(|m| (m.width * m.height) as usize).sum();
            Parsed::Lightmap { maps: maps.len() }
        }
        Kind::Wave => {
            let wave = gk3_formats::wav::parse(bytes).map_err(e)?;
            if wave.format.tag == gk3_formats::wav::FORMAT_MP3 {
                d.mp3 = 1;
            }
            if !skip_audio {
                let pcm = gk3_media::decode_wave(bytes).map_err(|e| e.to_string())?;
                if pcm.samples.is_empty() {
                    return Err("decoded to no samples".into());
                }
                d.seconds = pcm.seconds();
                if let Some(expected) = wave.fact_frames {
                    match pcm.frames().abs_diff(expected as usize) {
                        0 => d.fact_exact = 1,
                        1..=1152 => d.fact_close = 1,
                        _ => d.fact_off = 1,
                    }
                }
            }
            Parsed::Other
        }
        Kind::Text => {
            let scene_asset = name.to_ascii_uppercase().ends_with(".SCN");
            // Scene assets write bare vectors, which comma splitting would break.
            let split = if scene_asset {
                Split::Whole
            } else {
                Split::Commas
            };
            let doc = Document::parse(bytes, split);
            d.sections = doc.sections.len();
            if scene_asset {
                let bsp = doc
                    .sections
                    .iter()
                    .flat_map(|s| &s.lines)
                    .find_map(|l| l.get("BSP"))
                    .map(str::to_string);
                Parsed::SceneAsset { bsp }
            } else {
                Parsed::Other
            }
        }
        _ => return Ok(None),
    }))
}

fn describe(kind: Kind, d: &Detail, skip_audio: bool) -> String {
    match kind {
        Kind::Gk3Bitmap | Kind::WindowsBitmap => format!("{:.1} Mpixels", d.pixels as f64 / 1e6),
        Kind::Model | Kind::Scene => format!("{} triangles, {} vertices", d.triangles, d.vertices),
        Kind::Lightmap => format!(
            "{} lightmaps, {:.1} Mpixels",
            d.images,
            d.pixels as f64 / 1e6
        ),
        Kind::Wave if skip_audio => format!("{} MP3 (not decoded)", d.mp3),
        Kind::Wave => format!(
            "{} MP3, {:.1} hours decoded; length vs fact chunk: {} exact, {} within a frame, {} off",
            d.mp3,
            d.seconds / 3600.0,
            d.fact_exact,
            d.fact_close,
            d.fact_off
        ),
        Kind::Text => format!("{} sections", d.sections),
        Kind::ActorAnimation => {
            format!("{} keyframes across {} models", d.keyframes, d.models.len())
        }
        _ => String::new(),
    }
}

/// Upper-cased name without its extension.
fn stem(name: &str) -> String {
    name.rsplit_once('.')
        .map_or(name, |(stem, _)| stem)
        .to_ascii_uppercase()
}
