//! Where saved games live: `$XDG_DATA_HOME/gk3-rs/saves`, or
//! `~/.local/share/gk3-rs/saves` without it.

use std::path::PathBuf;

/// The quick save slot's name.
pub const QUICK: &str = "quick";

pub fn dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_DATA_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))?;
    Some(base.join("gk3-rs").join("saves"))
}

/// A save by name (`quick` is `quick.sav` in the saves directory), or a
/// path to a file if it looks like one.
pub fn path(name: &str) -> Option<PathBuf> {
    if name.contains('/') || name.ends_with(".sav") {
        return Some(PathBuf::from(name));
    }
    Some(dir()?.join(format!("{name}.sav")))
}

pub fn write(name: &str, text: &str) -> Result<PathBuf, String> {
    let path = path(name).ok_or("no home directory to save in")?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    // Write beside it and rename, so a crash never leaves half a save.
    let partial = path.with_extension("sav.partial");
    std::fs::write(&partial, text).map_err(|e| format!("{}: {e}", partial.display()))?;
    std::fs::rename(&partial, &path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path)
}

pub fn read(name: &str) -> Result<String, String> {
    let path = path(name).ok_or("no home directory to load from")?;
    std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))
}

/// Whether the intro has been shown before: the first run shows it before
/// the title screen, later ones go straight there, as G-Engine.
pub fn intro_seen() -> bool {
    intro_marker().is_some_and(|p| p.exists())
}

pub fn mark_intro_seen() {
    if let Some(path) = intro_marker() {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(path, "");
    }
}

fn intro_marker() -> Option<PathBuf> {
    Some(dir()?.parent()?.join("intro-seen"))
}

/// Whether there is a quick save to restore.
pub fn has_quick() -> bool {
    path(QUICK).is_some_and(|p| p.exists())
}

/// A save as the save and restore screens list it.
#[derive(Debug, Clone)]
pub struct Summary {
    /// Its slot: the file's name less `.sav`.
    pub slot: String,
    /// The name the player gave it.
    pub name: String,
    pub timeblock: String,
    /// Score events achieved, for its score.
    pub score_events: Vec<String>,
    pub modified: std::time::SystemTime,
}

/// The record a save's name is kept in, after the story's own.
pub const NAME_RECORD: &str = "name";

/// Every save, oldest first, as G-Engine lists them. The quick save is
/// among them, named so unless it was given a name.
pub fn list() -> Vec<Summary> {
    let Some(dir) = dir() else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out: Vec<Summary> = entries
        .filter_map(|e| {
            let path = e.ok()?.path();
            let slot = path.file_name()?.to_str()?.strip_suffix(".sav")?.to_string();
            let modified = path.metadata().ok()?.modified().ok()?;
            let text = std::fs::read_to_string(&path).ok()?;
            Some(summarize(slot, &text, modified))
        })
        .collect();
    out.sort_by_key(|s| s.modified);
    out
}

fn summarize(slot: String, text: &str, modified: std::time::SystemTime) -> Summary {
    let mut summary = Summary {
        name: if slot == QUICK { "Quick save".into() } else { slot.clone() },
        slot,
        timeblock: String::new(),
        score_events: Vec::new(),
        modified,
    };
    for line in text.lines() {
        let mut fields = line.split('\t');
        match (fields.next(), fields.next()) {
            (Some("timeblock"), Some(t)) => summary.timeblock = t.to_string(),
            (Some("score"), Some(e)) => summary.score_events.push(e.to_string()),
            (Some(NAME_RECORD), Some(n)) => summary.name = n.to_string(),
            _ => {}
        }
    }
    summary
}

/// A save's picture of the moment it was made: `<slot>.thumb` beside it,
/// "GK3T", width and height (16 bits each, little-endian), then RGBA.
fn thumb_path(slot: &str) -> Option<PathBuf> {
    Some(path(slot)?.with_extension("thumb"))
}

pub fn write_thumb(slot: &str, image: &gk3_formats::image::Image) -> Result<(), String> {
    let path = thumb_path(slot).ok_or("no home directory to save in")?;
    let mut bytes = b"GK3T".to_vec();
    bytes.extend((image.width as u16).to_le_bytes());
    bytes.extend((image.height as u16).to_le_bytes());
    bytes.extend(&image.rgba);
    std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn read_thumb(slot: &str) -> Option<gk3_formats::image::Image> {
    let bytes = std::fs::read(thumb_path(slot)?).ok()?;
    let rest = bytes.strip_prefix(b"GK3T")?;
    let width = u16::from_le_bytes([*rest.first()?, *rest.get(1)?]) as u32;
    let height = u16::from_le_bytes([*rest.get(2)?, *rest.get(3)?]) as u32;
    let rgba = rest.get(4..)?.to_vec();
    (rgba.len() == (width * height * 4) as usize).then_some(gk3_formats::image::Image {
        width,
        height,
        rgba,
    })
}

/// A picture shrunk by a whole factor, each pixel the mean of its block.
pub fn shrink(image: &gk3_formats::image::Image, factor: u32) -> gk3_formats::image::Image {
    let f = factor.max(1);
    let (w, h) = (image.width / f, image.height / f);
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let mut sum = [0u32; 4];
            for dy in 0..f {
                for dx in 0..f {
                    let at = (((y * f + dy) * image.width + x * f + dx) * 4) as usize;
                    for (c, s) in sum.iter_mut().enumerate() {
                        *s += image.rgba[at + c] as u32;
                    }
                }
            }
            rgba.extend(sum.map(|s| (s / (f * f)) as u8));
        }
    }
    gk3_formats::image::Image {
        width: w,
        height: h,
        rgba,
    }
}

/// A slot no save uses yet: `save001`, `save002` and so on.
pub fn free_slot() -> String {
    let taken: std::collections::HashSet<String> = list().into_iter().map(|s| s.slot).collect();
    (1..)
        .map(|n| format!("save{n:03}"))
        .find(|s| !taken.contains(s))
        .unwrap_or_else(|| "save".into())
}

/// A save's text with the player's name for it. Tabs and line breaks in
/// the name would break the record, so they become spaces.
pub fn named(text: &str, name: &str) -> String {
    let name: String = name
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    format!("{text}{NAME_RECORD}\t{}\n", name.trim())
}

/// The player's choices that outlast a game: volumes and captions.
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    pub master: f32,
    pub effects: f32,
    pub dialogue: f32,
    pub music: f32,
    pub mute_all: bool,
    pub mute_effects: bool,
    pub mute_dialogue: bool,
    pub mute_music: bool,
    pub captions: bool,
    /// G-Engine's cinematics: while on, the camera is the story's during
    /// its actions; off, the player may move it then too.
    pub cinematics: bool,
    /// Not in the original: a door's sound when the player goes through
    /// it to another room.
    pub door_sounds: bool,
    /// The original's advanced graphics options: mipmaps, and trilinear
    /// filtering between them.
    pub mipmaps: bool,
    pub trilinear: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            master: 1.0,
            effects: 1.0,
            dialogue: 1.0,
            music: 1.0,
            mute_all: false,
            mute_effects: false,
            mute_dialogue: false,
            mute_music: false,
            captions: true,
            cinematics: true,
            door_sounds: true,
            mipmaps: true,
            trilinear: true,
        }
    }
}

impl Settings {
    fn path() -> Option<PathBuf> {
        Some(dir()?.parent()?.join("settings.txt"))
    }

    /// The settings last kept, or the defaults.
    pub fn load() -> Self {
        let text = Self::path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .unwrap_or_default();
        Self::parse(&text)
    }

    fn parse(text: &str) -> Self {
        let mut s = Self::default();
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let (key, value) = (key.trim(), value.trim());
            let level = value.parse::<f32>().ok().map(|v| v.clamp(0.0, 1.0));
            let flag = value == "true";
            match key {
                "master" => s.master = level.unwrap_or(s.master),
                "effects" => s.effects = level.unwrap_or(s.effects),
                "dialogue" => s.dialogue = level.unwrap_or(s.dialogue),
                "music" => s.music = level.unwrap_or(s.music),
                "mute_all" => s.mute_all = flag,
                "mute_effects" => s.mute_effects = flag,
                "mute_dialogue" => s.mute_dialogue = flag,
                "mute_music" => s.mute_music = flag,
                "captions" => s.captions = flag,
                "cinematics" => s.cinematics = flag,
                "door_sounds" => s.door_sounds = flag,
                "mipmaps" => s.mipmaps = flag,
                "trilinear" => s.trilinear = flag,
                _ => {}
            }
        }
        s
    }

    fn text(&self) -> String {
        format!(
            "master = {}\neffects = {}\ndialogue = {}\nmusic = {}\nmute_all = {}\nmute_effects = {}\nmute_dialogue = {}\nmute_music = {}\ncaptions = {}\ncinematics = {}\ndoor_sounds = {}\nmipmaps = {}\ntrilinear = {}\n",
            self.master,
            self.effects,
            self.dialogue,
            self.music,
            self.mute_all,
            self.mute_effects,
            self.mute_dialogue,
            self.mute_music,
            self.captions,
            self.cinematics,
            self.door_sounds,
            self.mipmaps,
            self.trilinear
        )
    }

    /// Keeps the settings for next time; failing that, they last the run.
    pub fn store(&self) {
        if let Some(path) = Self::path() {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(path, self.text());
        }
    }

    /// Sets the sound's volumes to match.
    /// How room and model textures are to be filtered.
    pub fn filtering(&self) -> gk3_render::Filtering {
        gk3_render::Filtering {
            mipmaps: self.mipmaps,
            trilinear: self.trilinear,
        }
    }

    pub fn apply(&self, audio: &gk3_audio::Audio) {
        use gk3_audio::Bus;
        let level = |v: f32, muted: bool| if muted { 0.0 } else { v };
        audio.set_master(level(self.master, self.mute_all));
        audio.set_volume(Bus::Effects, level(self.effects, self.mute_effects));
        // The effects volume is the ambient sound's too (0x4e463d).
        audio.set_volume(Bus::Ambient, level(self.effects, self.mute_effects));
        audio.set_volume(Bus::Dialogue, level(self.dialogue, self.mute_dialogue));
        audio.set_volume(Bus::Music, level(self.music, self.mute_music));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_read_back() {
        let s = Settings {
            music: 0.25,
            mute_dialogue: true,
            captions: false,
            door_sounds: false,
            ..Settings::default()
        };
        assert_eq!(Settings::parse(&s.text()), s);
        assert_eq!(Settings::parse("master = 7\nnonsense"), Settings::default());
        // Kept before door sounds were a setting: they come on.
        assert!(Settings::parse("captions = true\n").door_sounds);
    }

    #[test]
    fn pictures_shrink_by_averaging() {
        let image = gk3_formats::image::Image {
            width: 2,
            height: 2,
            rgba: vec![0, 0, 0, 255, 100, 100, 100, 255, 200, 200, 200, 255, 100, 100, 100, 255],
        };
        let small = shrink(&image, 2);
        assert_eq!((small.width, small.height, small.rgba), (1, 1, vec![100, 100, 100, 255]));
    }

    #[test]
    fn a_save_names_itself() {
        let text = named("gk3-rs save\t1\ntimeblock\t110A\t110A\nscore\tE_X\n", "At\tthe inn");
        let s = summarize("save001".into(), &text, std::time::SystemTime::UNIX_EPOCH);
        assert_eq!((s.name.as_str(), s.timeblock.as_str()), ("At the inn", "110A"));
        assert_eq!(s.score_events, ["E_X"]);
    }
}
