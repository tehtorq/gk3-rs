//! The game's mouse cursors (`.CUR`): a little `key=value` file naming the
//! hotspot, how many frames there are and how fast they go, beside a bitmap
//! of the same name with the frames side by side. Shown as the system's own
//! cursor, as G-Engine's `Cursor` does.

use gk3_barn::Library;
use gk3_formats::image::{self, Image};

/// A cursor's frames, scaled, and how to show them.
#[derive(Debug, Clone)]
pub struct CursorArt {
    pub frames: Vec<Image>,
    /// The point of the cursor, in the scaled frame's pixels.
    pub hotspot: (u32, u32),
    pub frames_per_second: f32,
}

impl CursorArt {
    /// Loads a cursor by name (`C_POINT`), each pixel drawn `scale` times
    /// wide and high: the interface's whole-number scale.
    pub fn load(lib: &Library, name: &str, scale: u32) -> Option<Self> {
        let text = gk3_game::state::latin1(&lib.read(&format!("{name}.CUR")).ok()?);
        let settings = Settings::parse(&text);
        let sheet = image::decode(&lib.read(&format!("{name}.BMP")).ok()?).ok()?;
        // Pink is see-through, unless an alpha picture says how much.
        let sheet = match settings
            .alpha
            .as_ref()
            .and_then(|a| lib.read(&format!("{a}.BMP")).ok())
            .and_then(|b| image::decode(&b).ok())
        {
            Some(alpha) if (alpha.width, alpha.height) == (sheet.width, sheet.height) => {
                let mut out = sheet.clone();
                for (px, a) in out.rgba.chunks_exact_mut(4).zip(alpha.rgba.chunks_exact(4)) {
                    px[3] = a[0];
                }
                out
            }
            _ => gk3_render::color_key(&sheet),
        };
        Some(Self::from_sheet(&sheet, &settings, scale))
    }

    fn from_sheet(sheet: &Image, settings: &Settings, scale: u32) -> Self {
        let count = settings.frames.max(1);
        let (w, h) = (sheet.width / count, sheet.height);
        let scale = scale.max(1);
        let frames = (0..count)
            .map(|i| {
                let mut rgba = Vec::with_capacity((w * h * scale * scale * 4) as usize);
                for y in 0..h * scale {
                    for x in 0..w * scale {
                        let at = ((y / scale) * sheet.width + i * w + x / scale) as usize * 4;
                        rgba.extend_from_slice(&sheet.rgba[at..at + 4]);
                    }
                }
                Image {
                    width: w * scale,
                    height: h * scale,
                    rgba,
                }
            })
            .collect();
        let (hx, hy) = match settings.hotspot {
            Hotspot::Center => (w / 2, h / 2),
            Hotspot::At(x, y) => (x.min(w.saturating_sub(1)), y.min(h.saturating_sub(1))),
        };
        Self {
            frames,
            hotspot: (hx * scale, hy * scale),
            frames_per_second: settings.rate,
        }
    }

    /// The frame to show `seconds` after the cursor came up.
    pub fn frame_at(&self, seconds: f64, animate: bool) -> usize {
        if !animate || self.frames.len() < 2 || self.frames_per_second <= 0.0 {
            return 0;
        }
        (seconds * self.frames_per_second as f64) as usize % self.frames.len()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Hotspot {
    Center,
    At(u32, u32),
}

#[derive(Debug, Clone, PartialEq)]
struct Settings {
    hotspot: Hotspot,
    frames: u32,
    rate: f32,
    /// A picture giving each pixel's opacity.
    alpha: Option<String>,
}

impl Settings {
    fn parse(text: &str) -> Self {
        let mut s = Settings {
            hotspot: Hotspot::At(0, 0),
            frames: 1,
            rate: 0.0,
            alpha: None,
        };
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let value = value.trim();
            match key.trim().to_ascii_lowercase().as_str() {
                "hotspot" if value.eq_ignore_ascii_case("center") => s.hotspot = Hotspot::Center,
                "hotspot" => {
                    let mut it = value.split(',').map(|n| n.trim().parse().unwrap_or(0));
                    s.hotspot = Hotspot::At(it.next().unwrap_or(0), it.next().unwrap_or(0));
                }
                "frame count" => s.frames = value.parse().unwrap_or(1),
                "frame rate" => s.rate = value.parse().unwrap_or(0.0),
                "alpha channel" if !value.is_empty() => s.alpha = Some(value.to_string()),
                _ => {}
            }
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_files_read() {
        let s = Settings::parse("Hotspot=center\nFrame Count=15\nFrame Rate=10\nAlpha Channel=c_wait_alpha\n");
        assert_eq!(
            s,
            Settings {
                hotspot: Hotspot::Center,
                frames: 15,
                rate: 10.0,
                alpha: Some("c_wait_alpha".into())
            }
        );
        assert_eq!(Settings::parse("Hotspot=3,3\r\n").hotspot, Hotspot::At(3, 3));
    }

    /// The cursors the game shows all load, their points on them.
    #[test]
    fn the_games_cursors_load() {
        let Some(dir) = std::env::var_os("GK3_DATA") else {
            return;
        };
        let lib = Library::open_dir(dir).unwrap();
        for name in ["C_POINT", "C_ZOOM", "C_ZOOM_2", "C_WAIT", "C_EXIT_FORWARD", "C_EXIT_LEFT", "C_GRAB"] {
            let art = CursorArt::load(&lib, name, 2).unwrap_or_else(|| panic!("{name}"));
            let f = &art.frames[0];
            assert!(art.hotspot.0 < f.width && art.hotspot.1 < f.height, "{name}");
            assert!(f.rgba.chunks(4).any(|p| p[3] == 0), "{name} has no see-through pixels");
            assert!(f.rgba.chunks(4).any(|p| p[3] > 0), "{name} is empty");
        }
        assert_eq!(CursorArt::load(&lib, "C_WAIT", 1).unwrap().frames.len(), 15);
    }

    #[test]
    fn frames_are_cut_and_scaled() {
        // Two 1x1 frames, red then green, at twice the size.
        let sheet = Image {
            width: 2,
            height: 1,
            rgba: vec![255, 0, 0, 255, 0, 255, 0, 255],
        };
        let s = Settings::parse("Hotspot=center\nFrame Count=2\nFrame Rate=4");
        let art = CursorArt::from_sheet(&sheet, &s, 2);
        assert_eq!(art.frames.len(), 2);
        assert_eq!((art.frames[1].width, art.frames[1].height), (2, 2));
        assert_eq!(&art.frames[1].rgba[..4], &[0, 255, 0, 255]);
        assert_eq!(art.frame_at(0.3, true), 1);
        assert_eq!(art.frame_at(0.3, false), 0);
    }
}
