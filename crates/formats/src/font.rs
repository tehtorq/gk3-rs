//! Bitmap fonts (`.FON`): a definition naming every character, and a sheet
//! bitmap holding them.
//!
//! The sheet says nothing about where its characters are. Each row's top line
//! is a marker strip: a marker pixel at the left edge of every glyph, the
//! rest background. A glyph runs from its marker to the next and is one pixel
//! shorter than its row. On sheets of several rows, a run that reaches the
//! edge without finding another marker was the row's end mark, so the
//! character is looked for again on the next row. This follows G-Engine's
//! `Font::Load`.

use std::collections::HashMap;

use crate::image::Image;
use crate::reader::latin1;

#[derive(Debug, Clone, PartialEq)]
pub struct Definition {
    /// Every character on the sheet, in sheet order.
    pub characters: Vec<char>,
    pub bitmap: Option<String>,
    pub line_count: u32,
    pub char_extra: i32,
    pub line_extra: i32,
    pub default_char: Option<char>,
    /// `Color Replacement` fonts draw their glyphs in this colour.
    pub color: Option<[u8; 3]>,
}

impl Definition {
    /// Key/value lines whose keys have spaces. The `Font=` value is taken
    /// exactly, punctuation and all.
    pub fn parse(bytes: &[u8]) -> Self {
        let text = latin1(bytes);
        let mut def = Definition {
            characters: Vec::new(),
            bitmap: None,
            line_count: 1,
            char_extra: 0,
            line_extra: 0,
            default_char: None,
            color: None,
        };
        let mut replacement = false;
        for line in text.split(['\r', '\n']) {
            let line = line.trim_end_matches('\0');
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            match key.trim().to_ascii_lowercase().as_str() {
                "font" => def.characters = value.chars().collect(),
                "bitmap name" => def.bitmap = Some(value.trim().to_string()),
                "line count" => def.line_count = value.trim().parse().unwrap_or(1).max(1),
                "char extra" => def.char_extra = value.trim().parse().unwrap_or(0),
                "line extra" => def.line_extra = value.trim().parse().unwrap_or(0),
                "default char" => def.default_char = value.chars().next(),
                "type" => replacement = value.trim().eq_ignore_ascii_case("Color Replacement"),
                "color" => {
                    let parts: Vec<u8> = value
                        .split('/')
                        .filter_map(|p| p.trim().parse().ok())
                        .collect();
                    if let [r, g, b] = parts[..] {
                        def.color = Some([r, g, b]);
                    }
                }
                _ => {}
            }
        }
        if !replacement {
            def.color = None;
        }
        def
    }
}

/// Where a glyph is on the sheet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Glyph {
    pub x: u32,
    pub y: u32,
    pub width: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Font {
    pub glyphs: HashMap<char, Glyph>,
    pub glyph_height: u32,
    pub char_extra: i32,
    pub line_extra: i32,
    pub default_char: Option<char>,
    /// The sheet recoloured for drawing: white or the font's colour, with
    /// coverage in alpha. Tint it by multiplying.
    pub sheet: Image,
}

fn same(a: [u8; 4], b: [u8; 4], slack: i32) -> bool {
    (0..3)
        .map(|i| (a[i] as i32 - b[i] as i32).abs())
        .sum::<i32>()
        < slack
}

impl Font {
    /// Measures a sheet against its definition. `sheet` is decoded as the
    /// raw bitmap, before any colour keying.
    pub fn new(def: &Definition, sheet: &Image) -> Font {
        let (w, h) = (sheet.width, sheet.height);
        let pixel = |x: u32, y: u32| {
            let mut p = sheet.pixel(x.min(w - 1), y.min(h - 1));
            p[3] = 255;
            p
        };
        let background = pixel(0, 0);
        let marker = (0..w)
            .map(|x| pixel(x, 0))
            .find(|&p| !same(p, background, 1))
            .unwrap_or([255, 0, 0, 255]);
        let line_height = h / def.line_count.max(1);
        let glyph_height = line_height.saturating_sub(1);

        let mut glyphs = HashMap::new();
        let mut x = (0..w)
            .find(|&x| !same(pixel(x, 0), background, 1))
            .unwrap_or(0);
        let mut y = 0;
        let mut line = 1;
        let mut i = 0;
        while i < def.characters.len() {
            let start = x;
            let mut found = false;
            x += 1;
            while x < w {
                if same(pixel(x, y), marker, 10) {
                    found = true;
                    break;
                }
                x += 1;
            }
            let last = i + 1 == def.characters.len();
            if def.line_count > 1 && !found {
                if last {
                    // The final glyph: it ends at the first empty column.
                    x = start + 1;
                    while x < w
                        && (y + 1..(y + line_height).min(h))
                            .any(|yy| !same(pixel(x, yy), background, 1))
                    {
                        x += 1;
                    }
                } else {
                    // That run was the row's end mark: next row, same character.
                    line += 1;
                    x = 1;
                    y += line_height;
                    if y >= h {
                        break;
                    }
                    continue;
                }
            }
            glyphs.insert(
                def.characters[i],
                Glyph {
                    x: start,
                    y: y + 1,
                    width: (x - start).max(1),
                },
            );
            i += 1;
            if x >= w.saturating_sub(1) && line < def.line_count {
                line += 1;
                x = 1;
                y += line_height;
            }
        }

        Font {
            glyphs,
            glyph_height,
            char_extra: def.char_extra,
            line_extra: def.line_extra,
            default_char: def.default_char,
            sheet: recolor(sheet, def.color),
        }
    }

    pub fn glyph(&self, c: char) -> Option<&Glyph> {
        self.glyphs
            .get(&c)
            .or_else(|| self.default_char.and_then(|d| self.glyphs.get(&d)))
    }

    /// Width of a line of text in pixels.
    pub fn width(&self, text: &str) -> i32 {
        text.chars()
            .filter_map(|c| self.glyph(c))
            .map(|g| g.width as i32 + self.char_extra)
            .sum()
    }

    pub fn line_height(&self) -> i32 {
        self.glyph_height as i32 + self.line_extra
    }

    /// Splits text into lines no wider than `max_width`, at spaces.
    pub fn wrap(&self, text: &str, max_width: i32) -> Vec<String> {
        let mut lines = Vec::new();
        let mut current = String::new();
        for word in text.split_whitespace() {
            let candidate = if current.is_empty() {
                word.to_string()
            } else {
                format!("{current} {word}")
            };
            if self.width(&candidate) > max_width && !current.is_empty() {
                lines.push(std::mem::replace(&mut current, word.to_string()));
            } else {
                current = candidate;
            }
        }
        if !current.is_empty() {
            lines.push(current);
        }
        lines
    }
}

/// Coverage into alpha. Sheets come two ways: magenta-backed with hard
/// glyphs (the key gives coverage, and only the replacement colour is
/// recoloured, so drop shadows survive), or black-backed with antialiased
/// white glyphs (brightness gives coverage).
fn recolor(sheet: &Image, color: Option<[u8; 3]>) -> Image {
    let background = sheet.pixel(0, 0);
    let keyed = background[0] >= 230 && background[1] <= 25 && background[2] >= 230;
    let replace = sheet.pixel(0, 1.min(sheet.height - 1));
    let mut out = sheet.clone();
    for p in out.rgba.chunks_exact_mut(4) {
        let texel = [p[0], p[1], p[2], 255];
        if keyed {
            if same(texel, background, 40) {
                p.copy_from_slice(&[0, 0, 0, 0]);
            } else if let Some(c) = color.filter(|_| same(texel, replace, 40)) {
                p.copy_from_slice(&[c[0], c[1], c[2], 255]);
            } else {
                p[3] = 255;
            }
        } else {
            let coverage = p[0].max(p[1]).max(p[2]);
            let [r, g, b] = color.unwrap_or([255, 255, 255]);
            p.copy_from_slice(&[r, g, b, coverage]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A one-row sheet: row 0 is the marker strip (background at x = 0, red
    /// markers at x = 1, 4, 7), rows 1-2 the glyphs.
    fn sheet() -> Image {
        let (w, h) = (10u32, 3u32);
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        for x in [1, 4, 7] {
            rgba[(x * 4) as usize..(x * 4 + 4) as usize].copy_from_slice(&[255, 0, 0, 255]);
        }
        for p in rgba.chunks_exact_mut(4) {
            p[3] = 255;
        }
        Image {
            width: w,
            height: h,
            rgba,
        }
    }

    #[test]
    fn parses_definitions_exactly() {
        let def = Definition::parse(b"Font=AB=,;\r\nBitmap Name=x.bmp\r\nLine Count=2\r\nType=Color Replacement\r\nColor=1/2/3\r\n");
        assert_eq!(def.characters, ['A', 'B', '=', ',', ';']);
        assert_eq!(def.line_count, 2);
        assert_eq!(def.color, Some([1, 2, 3]));
        assert_eq!(Definition::parse(b"Font=A\nColor=1/2/3\n").color, None);
    }

    #[test]
    fn measures_glyphs_between_markers() {
        let def = Definition::parse(b"Font=ABC\n");
        let font = Font::new(&def, &sheet());
        assert_eq!(
            font.glyphs[&'A'],
            Glyph {
                x: 1,
                y: 1,
                width: 3
            }
        );
        assert_eq!(
            font.glyphs[&'B'],
            Glyph {
                x: 4,
                y: 1,
                width: 3
            }
        );
        // The last runs to the edge.
        assert_eq!(font.glyphs[&'C'].width, 3);
        assert_eq!(font.glyph_height, 2);
        assert_eq!(font.width("AB"), 6);
    }

    #[test]
    fn wraps_at_spaces() {
        let def = Definition::parse(b"Font=ABC \n");
        // The space runs off the end of the sheet and gets the minimum width.
        let font = Font::new(&def, &sheet());
        assert_eq!(font.wrap("AB AB AB", 15), ["AB AB", "AB"]);
    }
}
