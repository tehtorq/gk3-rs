//! Faces: expressions painted onto a character's face bitmap.
//!
//! A head wears one bitmap. Talking, blinking and frowning are done by
//! pasting smaller bitmaps over its mouth, eyelids and forehead, as
//! `FACES.TXT` lays them out, and giving the head the result.

use std::collections::HashMap;

use gk3_barn::Library;
use gk3_formats::image::{self, Image};

#[derive(Debug, Clone, PartialEq)]
pub struct FaceConfig {
    /// The character's three-letter code.
    pub code: String,
    pub face: String,
    pub mouth_offset: (u32, u32),
    pub eyelids_offset: (u32, u32),
    pub forehead_offset: (u32, u32),
    pub eyelids_alpha: Option<String>,
    /// Blink animations and their weights.
    pub blinks: Vec<(String, u32)>,
    /// Shortest and longest gap between blinks, in seconds.
    pub blink_gap: (f64, f64),
}

/// Parses `FACES.TXT`: one section per character code. Keys have spaces and
/// values run to a `//` comment. `[DEFAULT]` supplies only blink timing.
pub fn parse(bytes: &[u8]) -> HashMap<String, FaceConfig> {
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
                    .unwrap_or_else(|| format!("{name}_FACE")),
                mouth_offset: offset("mouth offset"),
                eyelids_offset: offset("eyelids offset"),
                forehead_offset: offset("forehead offset"),
                eyelids_alpha: map.get("eyelids alpha channel").cloned(),
                blinks,
                blink_gap: (gap.0 as f64 / 1000.0, gap.1 as f64 / 1000.0),
            },
        );
    }
    out
}

/// What a face currently shows; `None` is the part's resting picture.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct Expression {
    pub mouth: Option<String>,
    pub eyelids: Option<String>,
    pub forehead: Option<String>,
}

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
        let key = (config.code.clone(), expression.clone());
        if let Some(done) = self.faces.get(&key) {
            return done.clone();
        }
        let result = self.build(lib, config, expression);
        self.faces.insert(key, result.clone());
        result
    }

    fn build(&mut self, lib: &Library, config: &FaceConfig, e: &Expression) -> Option<Image> {
        let mut face = self.bitmap(lib, &config.face)?;
        let code = &config.code;
        let forehead = e
            .forehead
            .clone()
            .unwrap_or_else(|| format!("{code}_FOREHEAD"));
        if let Some(patch) = self.bitmap(lib, &forehead) {
            paste(&mut face, &patch, config.forehead_offset, None);
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
        let mouth = e.mouth.clone().unwrap_or_else(|| format!("{code}_MOUTH00"));
        if let Some(patch) = self.bitmap(lib, &mouth) {
            paste(&mut face, &patch, config.mouth_offset, None);
        }
        Some(face)
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

/// `gk3 face CODE [MOUTH] [EYELIDS] --png FILE`: writes a composed face.
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
    let configs = parse(&lib.read("FACES.TXT").map_err(|e| e.to_string())?);
    let config = configs
        .get(&code)
        .ok_or_else(|| format!("FACES.TXT has no {code}"))?;
    let expression = Expression {
        mouth: words
            .get(1)
            .map(|m| format!("{code}_{}", m.to_ascii_uppercase())),
        eyelids: words.get(2).map(|e| e.to_ascii_uppercase()),
        forehead: None,
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
