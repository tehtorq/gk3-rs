//! Faces: expressions painted onto a character's face bitmap.
//!
//! A head wears one bitmap. Talking, blinking and frowning are done by
//! pasting smaller bitmaps over its mouth, eyelids and forehead, as
//! `FACES.TXT` lays them out, and giving the head the result. The eyes are
//! pictures of their own at four times the face's resolution, shrunk onto it
//! a quarter-pixel at a time so they can glance and jitter.

use std::collections::HashMap;

use gk3_barn::Library;
use gk3_formats::image::{self, Image};
use gk3_game::character::Character;

#[derive(Debug, Clone, PartialEq)]
pub struct FaceConfig {
    /// The character's three-letter code, which moods are named after.
    pub code: String,
    /// What the forehead and eyelid pictures are named after: the
    /// character's `BaseName` (`VR3` wears `VA3_EYELIDS`).
    pub parts: String,
    /// What the mouths are named after: the wearer's `MouthName`.
    pub mouths: String,
    pub face: String,
    pub mouth_offset: (u32, u32),
    pub eyelids_offset: (u32, u32),
    pub forehead_offset: (u32, u32),
    pub eyelids_alpha: Option<String>,
    /// Blink animations and their weights.
    pub blinks: Vec<(String, u32)>,
    /// Shortest and longest gap between blinks, in seconds.
    pub blink_gap: (f64, f64),
    pub eyes: Option<Eyes>,
}

/// A character's painted eyes, from `FACES.TXT`.
#[derive(Debug, Clone, PartialEq)]
pub struct Eyes {
    /// The eye bitmaps, at four times the face's resolution.
    pub left: String,
    pub right: String,
    /// Where each goes on the face, in pixels.
    pub left_offset: (f32, f32),
    pub right_offset: (f32, f32),
    /// A nudge to each eye's resting place, in pixels.
    pub left_bias: (f32, f32),
    pub right_bias: (f32, f32),
    /// How far the eyes can look each way, in pixels.
    pub max_look: (f32, f32),
    /// Shortest and longest gap between jitters, in seconds.
    pub jitter_gap: (f64, f64),
    /// How far a jitter may move them, in pixels.
    pub max_jitter: (f32, f32),
    /// What they can see without turning, in degrees across and up-down.
    pub field_of_view: (f32, f32),
}

/// Parses `FACES.TXT`: one section per character code. Keys have spaces and
/// values run to a `//` comment. `[DEFAULT]` supplies only blink timing.
/// Pictures not named outright are named after the character's `BaseName`
/// from `CHARACTERS.TXT`, as the original.
pub fn parse(bytes: &[u8], characters: &HashMap<String, Character>) -> HashMap<String, FaceConfig> {
    let text: String = bytes.iter().map(|&b| b as char).collect();
    let mut sections: Vec<(String, HashMap<String, String>)> = Vec::new();
    for raw in text.lines() {
        let line = raw.split("//").next().unwrap_or("").trim();
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            sections.push((name.trim().to_ascii_uppercase(), HashMap::new()));
        } else if let (Some((key, value)), Some((_, map))) =
            (line.split_once('='), sections.last_mut())
        {
            map.insert(key.trim().to_ascii_lowercase(), value.trim().to_string());
        }
    }
    let floats = |v: &str| -> Option<(f32, f32)> {
        let mut parts = v
            .split([',', 'x', 'X'])
            .map(|p| p.trim().parse::<f32>().ok());
        Some((parts.next()??, parts.next()??))
    };
    let default = sections
        .iter()
        .find(|(n, _)| n == "DEFAULT")
        .map(|(_, m)| m.clone())
        .unwrap_or_default();
    let pair = |v: &str| -> Option<(u32, u32)> {
        let mut parts = v
            .split([',', 'x', 'X'])
            .map(|p| p.trim().parse::<f32>().ok());
        Some((parts.next()?? as u32, parts.next()?? as u32))
    };
    let default_gap = sections
        .iter()
        .find(|(n, _)| n == "DEFAULT")
        .and_then(|(_, m)| m.get("blink frequency"))
        .and_then(|v| pair(v))
        .unwrap_or((5000, 12000));
    let mut out = HashMap::new();
    for (name, map) in &sections {
        // Three-letter codes only; suffixed sections are unfinished art.
        if name == "DEFAULT" || name.len() != 3 {
            continue;
        }
        let offset = |key: &str| map.get(key).and_then(|v| pair(v)).unwrap_or((0, 0));
        let character = characters.get(name);
        let parts = character.map_or_else(|| name.clone(), |c| c.base_name.clone());
        let mouths = character.map_or_else(|| parts.clone(), |c| c.mouth_name.clone());
        let blinks = map
            .get("blink anims")
            .map(|v| {
                let parts: Vec<&str> = v.split(',').map(str::trim).collect();
                parts
                    .chunks(2)
                    .filter_map(|c| {
                        Some((
                            c.first()?.to_string(),
                            c.get(1).and_then(|w| w.parse().ok()).unwrap_or(1),
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let gap = map
            .get("blink frequency")
            .and_then(|v| pair(v))
            .unwrap_or(default_gap);
        out.insert(
            name.clone(),
            FaceConfig {
                code: name.clone(),
                face: map
                    .get("face name")
                    .cloned()
                    .unwrap_or_else(|| format!("{parts}_FACE")),
                parts,
                mouths,
                mouth_offset: offset("mouth offset"),
                eyelids_offset: offset("eyelids offset"),
                forehead_offset: offset("forehead offset"),
                eyelids_alpha: map.get("eyelids alpha channel").cloned(),
                blinks,
                blink_gap: (gap.0 as f64 / 1000.0, gap.1 as f64 / 1000.0),
                eyes: {
                    let get = |key: &str| {
                        map.get(key)
                            .or_else(|| default.get(key))
                            .and_then(|v| floats(v))
                    };
                    match (map.get("left eye name"), map.get("right eye name")) {
                        (Some(l), Some(r)) => {
                            let ms = get("jitter frequency").unwrap_or((1000.0, 4000.0));
                            Some(Eyes {
                                left: l.clone(),
                                right: r.clone(),
                                left_offset: get("left eye offset").unwrap_or_default(),
                                right_offset: get("right eye offset").unwrap_or_default(),
                                left_bias: get("left eye bias").unwrap_or_default(),
                                right_bias: get("right eye bias").unwrap_or_default(),
                                max_look: get("max look distance").unwrap_or((2.0, 2.0)),
                                jitter_gap: (ms.0 as f64 / 1000.0, ms.1 as f64 / 1000.0),
                                // Written in subpixels, a quarter-pixel each.
                                max_jitter: get("max jitter distance")
                                    .map_or((0.5, 0.5), |(x, y)| (x / 4.0, y / 4.0)),
                                field_of_view: get("eye field of view").unwrap_or((120.0, 90.0)),
                            })
                        }
                        _ => None,
                    }
                },
            },
        );
    }
    out
}

/// The face a character wears: their own section of `FACES.TXT`, or else
/// their `BaseName`'s (Mosely with his newspaper wears Mosely's), with
/// their own mouths.
pub fn for_character(
    faces: &HashMap<String, FaceConfig>,
    characters: &HashMap<String, Character>,
    code: &str,
) -> Option<FaceConfig> {
    let code = code.to_ascii_uppercase();
    let character = characters.get(&code);
    let mut config = faces
        .get(&code)
        .or_else(|| faces.get(&character?.base_name))?
        .clone();
    if let Some(c) = character {
        config.mouths = c.mouth_name.clone();
    }
    Some(config)
}

/// What a face currently shows; `None` is the part's resting picture.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct Expression {
    pub mouth: Option<String>,
    pub eyelids: Option<String>,
    pub forehead: Option<String>,
    /// How far the eyes are moved from rest, in quarter-pixels right and
    /// down: where they look, and their jitter.
    pub eyes: (i32, i32),
}

/// How many composed faces to keep before starting afresh.
const FACE_CACHE: usize = 64;

/// Builds face pictures, caching the bitmaps it reads and the results.
#[derive(Default)]
pub struct Compositor {
    bitmaps: HashMap<String, Option<Image>>,
    raw: HashMap<String, Option<Image>>,
    faces: HashMap<(String, Expression), Option<Image>>,
}

impl Compositor {
    fn bitmap(&mut self, lib: &Library, name: &str) -> Option<Image> {
        self.bitmaps
            .entry(name.to_ascii_uppercase())
            .or_insert_with(|| image::decode(&lib.read(&format!("{name}.BMP")).ok()?).ok())
            .clone()
    }

    fn raw(&mut self, lib: &Library, name: &str) -> Option<Image> {
        self.raw
            .entry(name.to_ascii_uppercase())
            .or_insert_with(|| image::decode_raw(&lib.read(&format!("{name}.BMP")).ok()?).ok())
            .clone()
    }

    pub fn compose(
        &mut self,
        lib: &Library,
        config: &FaceConfig,
        expression: &Expression,
    ) -> Option<Image> {
        let key = (format!("{}/{}", config.code, config.mouths), expression.clone());
        if let Some(done) = self.faces.get(&key) {
            return done.clone();
        }
        let result = self.build(lib, config, expression);
        // Mouths times eye positions are many: keep the cache small rather
        // than every face ever made.
        if self.faces.len() >= FACE_CACHE {
            self.faces.clear();
        }
        self.faces.insert(key, result.clone());
        result
    }

    fn build(&mut self, lib: &Library, config: &FaceConfig, e: &Expression) -> Option<Image> {
        let mut face = self.bitmap(lib, &config.face)?;
        let code = &config.parts;
        let forehead = e
            .forehead
            .clone()
            .unwrap_or_else(|| format!("{code}_FOREHEAD"));
        if let Some(patch) = self.bitmap(lib, &forehead) {
            paste(&mut face, &patch, config.forehead_offset, None);
        }
        // The eyes go under the eyelids: only within the lids' ring (the
        // eyelids bitmap, less its transparent surround), whose open middle
        // the lids' alpha then leaves showing.
        if let Some(eyes) = &config.eyes
            && let Some(lids) = self.bitmap(lib, &format!("{code}_EYELIDS"))
        {
            let (lx, ly) = config.eyelids_offset;
            let within = |x: i32, y: i32| {
                let (ex, ey) = (x - lx as i32, y - ly as i32);
                ex >= 0
                    && ey >= 0
                    && (ex as u32) < lids.width
                    && (ey as u32) < lids.height
                    && lids.pixel(ex as u32, ey as u32)[3] > 0
            };
            for (name, offset, bias) in [
                (&eyes.left, eyes.left_offset, eyes.left_bias),
                (&eyes.right, eyes.right_offset, eyes.right_bias),
            ] {
                if let Some(eye) = self.bitmap(lib, name) {
                    paint_eye(&mut face, &eye, offset, bias, e.eyes, &within);
                }
            }
        }
        match &e.eyelids {
            Some(eyelids) => {
                if let Some(patch) = self.bitmap(lib, eyelids) {
                    paste(&mut face, &patch, config.eyelids_offset, None);
                }
            }
            // Resting eyelids are a soft edge against the skin: blended by
            // their alpha bitmap.
            None => {
                let alpha = config.eyelids_alpha.as_ref().and_then(|a| self.raw(lib, a));
                if let Some(patch) = self.bitmap(lib, &format!("{code}_EYELIDS")) {
                    paste(&mut face, &patch, config.eyelids_offset, alpha.as_ref());
                }
            }
        }
        let mouth = e
            .mouth
            .clone()
            .unwrap_or_else(|| format!("{}_MOUTH00", config.mouths));
        if let Some(patch) = self.bitmap(lib, &mouth) {
            paste(&mut face, &patch, config.mouth_offset, None);
        }
        Some(face)
    }
}

/// Shrinks an eye picture (four times the face's resolution) onto the face
/// at its offset, moved by its bias and by `shift` quarter-pixels, as
/// G-Engine's `UpdateEyeOnFaceTexture`: a resting eye sits half a pixel
/// left and a pixel up of its offset plus bias. Only where `within` allows.
fn paint_eye(
    face: &mut Image,
    eye: &Image,
    offset: (f32, f32),
    bias: (f32, f32),
    shift: (i32, i32),
    within: &dyn Fn(i32, i32) -> bool,
) {
    // Where the eye's top-left lands, in quarter-pixels.
    let qx = ((offset.0 + bias.0 - 0.5) * 4.0).round() as i32 + shift.0;
    let qy = ((offset.1 + bias.1 - 1.0) * 4.0).round() as i32 + shift.1;
    let (px, py) = (qx.div_euclid(4), qy.div_euclid(4));
    let (sx, sy) = (qx.rem_euclid(4), qy.rem_euclid(4));
    let (w, h) = (eye.width / 4, eye.height / 4);
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            let (tx, ty) = (px + x, py + y);
            if tx < 0
                || ty < 0
                || tx as u32 >= face.width
                || ty as u32 >= face.height
                || !within(tx, ty)
            {
                continue;
            }
            // The 4x4 block under this face pixel, moved by the
            // quarter-pixel remainder; edges clamp.
            let mut sum = [0u32; 3];
            for by in 0..4 {
                for bx in 0..4 {
                    let ex = (x * 4 + bx - sx).clamp(0, eye.width as i32 - 1) as u32;
                    let ey = (y * 4 + by - sy).clamp(0, eye.height as i32 - 1) as u32;
                    let p = eye.pixel(ex, ey);
                    for (total, &value) in sum.iter_mut().zip(&p[..3]) {
                        *total += value as u32;
                    }
                }
            }
            let i = ((ty as u32 * face.width + tx as u32) * 4) as usize;
            for (out, total) in face.rgba[i..i + 3].iter_mut().zip(sum) {
                *out = (total / 16) as u8;
            }
        }
    }
}

/// Pastes `patch` onto `onto` at an offset, honouring the patch's colour key
/// and, if given, a greyscale alpha bitmap.
fn paste(onto: &mut Image, patch: &Image, (ox, oy): (u32, u32), alpha: Option<&Image>) {
    for y in 0..patch.height {
        for x in 0..patch.width {
            let (tx, ty) = (ox + x, oy + y);
            if tx >= onto.width || ty >= onto.height {
                continue;
            }
            let p = patch.pixel(x, y);
            let mut a = p[3] as u32;
            if let Some(alpha) = alpha
                && x < alpha.width
                && y < alpha.height
            {
                a = a * alpha.pixel(x, y)[0] as u32 / 255;
            }
            if a == 0 {
                continue;
            }
            let i = ((ty * onto.width + tx) * 4) as usize;
            for (under, &over) in onto.rgba[i..i + 3].iter_mut().zip(&p[..3]) {
                *under = ((over as u32 * a + *under as u32 * (255 - a)) / 255) as u8;
            }
        }
    }
}

/// `gk3 face CODE [MOUTH] [EYELIDS|-] [EYES x,y] --png FILE`: writes a
/// composed face, its eyes moved by quarter-pixels if asked.
pub fn tool(lib: &Library, args: &[String]) -> Result<std::process::ExitCode, String> {
    let mut words: Vec<&String> = args.iter().collect();
    let out = match words.iter().position(|a| *a == "--png") {
        Some(i) => {
            let path = words.get(i + 1).ok_or("--png needs a file")?.to_string();
            words.drain(i..=i + 1);
            path
        }
        None => "face.png".into(),
    };
    let code = words
        .first()
        .ok_or("face needs a character code, e.g. GAB")?
        .to_ascii_uppercase();
    let characters = crate::shoot::characters(lib);
    let configs = parse(
        &lib.read("FACES.TXT").map_err(|e| e.to_string())?,
        &characters,
    );
    let config = &for_character(&configs, &characters, &code)
        .ok_or_else(|| format!("FACES.TXT has no {code}"))?;
    let expression = Expression {
        mouth: words
            .get(1)
            .map(|m| format!("{}_{}", config.mouths, m.to_ascii_uppercase())),
        eyelids: words
            .get(2)
            .filter(|e| e.as_str() != "-")
            .map(|e| e.to_ascii_uppercase()),
        forehead: None,
        // An eye shift in quarter-pixels, right and down: `8,0`.
        eyes: words
            .get(3)
            .and_then(|v| {
                let (x, y) = v.split_once(',')?;
                Some((x.trim().parse().ok()?, y.trim().parse().ok()?))
            })
            .unwrap_or((0, 0)),
    };
    let image = Compositor::default()
        .compose(lib, config, &expression)
        .ok_or_else(|| format!("no face bitmap {}", config.face))?;
    std::fs::write(&out, gk3_formats::png::encode(&image)).map_err(|e| format!("{out}: {e}"))?;
    println!("{code} {expression:?} -> {out}");
    Ok(std::process::ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_character() {
        let faces = parse(
            b"[DEFAULT]\r\nBlink Frequency = 4000,8000\r\n[GAB]\r\nMouth Offset = 90,132 // pixels\r\n\
              Mouth Size = 78x82\r\nEyelids Offset = 105,106\r\nEyelids Alpha Channel = gab_eyelids_alpha\r\n\
              Blink Anims = gabblink,90,gabblink2,10\r\n[CON-XXX]\r\nMouth Offset = 1,1\r\n",
            &HashMap::new(),
        );
        let gab = &faces["GAB"];
        assert_eq!(gab.mouth_offset, (90, 132));
        assert_eq!(gab.face, "GAB_FACE");
        assert_eq!(
            gab.blinks,
            [("gabblink".to_string(), 90), ("gabblink2".to_string(), 10)]
        );
        assert_eq!(gab.blink_gap, (4.0, 8.0));
        assert!(!faces.contains_key("CON-XXX"));
    }

    #[test]
    fn pasting_respects_keys_and_alpha() {
        let mut face = Image {
            width: 2,
            height: 1,
            rgba: vec![0, 0, 0, 255, 0, 0, 0, 255],
        };
        let patch = Image {
            width: 2,
            height: 1,
            rgba: vec![200, 200, 200, 255, 9, 9, 9, 0],
        };
        paste(&mut face, &patch, (0, 0), None);
        assert_eq!(face.pixel(0, 0), [200, 200, 200, 255]);
        assert_eq!(
            face.pixel(1, 0),
            [0, 0, 0, 255],
            "keyed pixels leave the face alone"
        );
        let half = Image {
            width: 1,
            height: 1,
            rgba: vec![128, 128, 128, 255],
        };
        let mut face = Image {
            width: 1,
            height: 1,
            rgba: vec![0, 0, 0, 255],
        };
        paste(
            &mut face,
            &Image {
                width: 1,
                height: 1,
                rgba: vec![255, 255, 255, 255],
            },
            (0, 0),
            Some(&half),
        );
        assert_eq!(face.pixel(0, 0)[0], 128);
    }
}
