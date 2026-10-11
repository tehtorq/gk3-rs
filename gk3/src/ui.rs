//! The interface drawn over the scene: captions, the name of what is under
//! the pointer, and the verb menu, drawn with GK3's own fonts and icons.

use std::collections::HashMap;

use gk3_barn::Library;
use gk3_formats::font::{Definition, Font};
use gk3_formats::image;
use gk3_game::actions::Rule;
use gk3_render::{Overlay, OverlayTexture, Renderer};

use crate::world::World;

/// Icons are drawn at this size on a 480-line screen, and scaled with it.
const ICON: f32 = 32.0;

struct LoadedFont {
    font: Font,
    texture: OverlayTexture,
}

/// A verb menu open on a noun.
pub struct Menu {
    pub noun: String,
    pub rules: Vec<Rule>,
    pub x: f32,
    pub y: f32,
}

pub struct Ui {
    fonts: HashMap<&'static str, LoadedFont>,
    /// Verb to its icon names (normal, hover), from VERBS.TXT.
    icon_names: HashMap<String, (String, String)>,
    icons: HashMap<String, Option<OverlayTexture>>,
}

impl Ui {
    pub fn new(lib: &Library, renderer: &mut Renderer) -> Self {
        let mut fonts = HashMap::new();
        for name in [
            "F_CAPTION_DEFAULT",
            "F_CAPTION_EGO",
            "F_CAPTION_VOICEOVER",
            "F_TOOLTIP",
        ] {
            if let Some(font) = load_font(lib, name) {
                let texture = renderer.add_overlay_texture(&font.sheet);
                fonts.insert(name, LoadedFont { font, texture });
            }
        }
        let mut icon_names = HashMap::new();
        if let Ok(bytes) = lib.read("VERBS.TXT") {
            let doc = gk3_formats::ini::Document::parse(&bytes, gk3_formats::ini::Split::Commas);
            for line in doc.sections("VERBS").flat_map(|s| &s.lines) {
                let (Some(first), Some(up), Some(hover)) =
                    (line.pairs.first(), line.get("up"), line.get("hover"))
                else {
                    continue;
                };
                icon_names.insert(
                    first.key.to_ascii_uppercase(),
                    (up.to_string(), hover.to_string()),
                );
            }
        }
        Self {
            fonts,
            icon_names,
            icons: HashMap::new(),
        }
    }

    fn icon(
        &mut self,
        lib: &Library,
        renderer: &mut Renderer,
        name: &str,
    ) -> Option<OverlayTexture> {
        *self
            .icons
            .entry(name.to_ascii_uppercase())
            .or_insert_with(|| {
                let bytes = lib.read(&format!("{name}.BMP")).ok()?;
                let image = gk3_render::color_key(&image::decode(&bytes).ok()?);
                Some(renderer.add_overlay_texture(&image))
            })
    }

    fn scale(height: u32) -> f32 {
        (height as f32 / 480.0).max(1.0)
    }

    /// Draws text with its top left at (x, y), scaled; returns its width.
    fn text(
        &self,
        overlay: &mut Overlay,
        font: &str,
        text: &str,
        (x, y): (f32, f32),
        scale: f32,
        color: [u8; 4],
    ) -> f32 {
        let Some(f) = self.fonts.get(font).or_else(|| self.fonts.values().next()) else {
            return 0.0;
        };
        let mut pen = x;
        for c in text.chars() {
            let Some(g) = f.font.glyph(c) else { continue };
            let (w, h) = (g.width as f32, f.font.glyph_height as f32);
            overlay.image(
                f.texture,
                [g.x as f32, g.y as f32, w, h],
                [pen, y, w * scale, h * scale],
                color,
            );
            pen += (w + f.font.char_extra as f32) * scale;
        }
        pen - x
    }

    fn measure(&self, font: &str, text: &str) -> (f32, f32) {
        match self.fonts.get(font).or_else(|| self.fonts.values().next()) {
            Some(f) => (f.font.width(text) as f32, f.font.line_height() as f32),
            None => (0.0, 0.0),
        }
    }

    /// The lines being spoken, centred at the bottom of the screen.
    pub fn captions(&self, overlay: &mut Overlay, world: &World, size: (u32, u32)) {
        let scale = Self::scale(size.1);
        let (w, h) = (size.0 as f32, size.1 as f32);
        let mut y = h - 24.0 * scale;
        for caption in world
            .captions
            .iter()
            .rev()
            .filter(|c| c.from <= world.clock)
            .take(2)
        {
            let ego = caption
                .speaker
                .as_deref()
                .is_some_and(|s| s.eq_ignore_ascii_case(&world.state.ego));
            let font = match caption.speaker.as_deref() {
                None | Some("UNKNOWN") | Some("") => "F_CAPTION_VOICEOVER",
                _ if ego => "F_CAPTION_EGO",
                _ => "F_CAPTION_DEFAULT",
            };
            let max = (w * 0.8 / scale) as i32;
            let lines = match self.fonts.get(font) {
                Some(f) => f.font.wrap(&caption.text, max),
                None => vec![caption.text.clone()],
            };
            let (_, line_h) = self.measure(font, "A");
            for line in lines.iter().rev() {
                y -= line_h * scale;
                let (lw, _) = self.measure(font, line);
                let x = (w - lw * scale) / 2.0;
                overlay.rect(
                    [x - 6.0, y - 2.0, lw * scale + 12.0, line_h * scale + 4.0],
                    [0, 0, 0, 110],
                );
                self.text(overlay, font, line, (x, y), scale, [255, 255, 255, 255]);
            }
            y -= 6.0 * scale;
        }
    }

    /// The name of what is under the pointer.
    pub fn label(&self, overlay: &mut Overlay, noun: &str, x: f32, y: f32, size: (u32, u32)) {
        let scale = Self::scale(size.1);
        let text = display_name(noun);
        let (w, h) = self.measure("F_TOOLTIP", &text);
        let (bx, by) = (x + 14.0 * scale, y + 18.0 * scale);
        overlay.rect(
            [bx - 4.0, by - 2.0, w * scale + 8.0, h * scale + 4.0],
            [250, 240, 210, 230],
        );
        self.text(
            overlay,
            "F_TOOLTIP",
            &text,
            (bx, by),
            scale,
            [255, 255, 255, 255],
        );
    }

    /// Where each verb's icon is drawn.
    pub fn menu_slots(menu: &Menu, size: (u32, u32)) -> Vec<[f32; 4]> {
        let scale = Self::scale(size.1);
        let icon = ICON * scale;
        let count = menu.rules.len() as f32;
        let total = count * (icon + 4.0);
        let x0 = (menu.x - total / 2.0).clamp(4.0, (size.0 as f32 - total - 4.0).max(4.0));
        let y0 = (menu.y - icon - 12.0).clamp(4.0, size.1 as f32 - icon - 4.0);
        (0..menu.rules.len())
            .map(|i| [x0 + i as f32 * (icon + 4.0), y0, icon, icon])
            .collect()
    }

    /// Draws a verb menu, highlighting the verb under the pointer.
    pub fn menu(
        &mut self,
        lib: &Library,
        renderer: &mut Renderer,
        overlay: &mut Overlay,
        menu: &Menu,
        pointer: (f32, f32),
        size: (u32, u32),
    ) {
        let scale = Self::scale(size.1);
        let slots = Self::menu_slots(menu, size);
        if let (Some(first), Some(last)) = (slots.first(), slots.last()) {
            overlay.rect(
                [
                    first[0] - 4.0,
                    first[1] - 4.0,
                    last[0] + last[2] - first[0] + 8.0,
                    last[3] + 8.0,
                ],
                [20, 15, 10, 170],
            );
        }
        // What the menu is for, above it.
        if let Some(first) = slots.first() {
            let name = display_name(&menu.noun);
            let (_, h) = self.measure("F_TOOLTIP", &name);
            self.text(
                overlay,
                "F_TOOLTIP",
                &name,
                (first[0], first[1] - (h + 8.0) * scale),
                scale,
                [255; 4],
            );
        }
        for (rule, slot) in menu.rules.iter().zip(&slots) {
            let hovered = inside(*slot, pointer);
            let icon = self
                .icon_names
                .get(&rule.verb)
                .cloned()
                .and_then(|(up, hover)| {
                    self.icon(lib, renderer, if hovered { &hover } else { &up })
                });
            match icon {
                Some(texture) => overlay.image(texture, [0.0, 0.0, ICON, ICON], *slot, [255; 4]),
                None => {
                    // A verb with no icon: its name in a box.
                    overlay.rect(
                        *slot,
                        if hovered {
                            [120, 90, 50, 230]
                        } else {
                            [70, 55, 35, 230]
                        },
                    );
                    let text: String = display_name(&rule.verb).chars().take(4).collect();
                    self.text(
                        overlay,
                        "F_TOOLTIP",
                        &text,
                        (slot[0] + 2.0, slot[1] + slot[3] / 3.0),
                        scale,
                        [255; 4],
                    );
                }
            }
            if hovered {
                let name = display_name(&rule.verb);
                let (w, _) = self.measure("F_TOOLTIP", &name);
                let x = slot[0] + slot[2] / 2.0 - w * scale / 2.0;
                self.text(
                    overlay,
                    "F_TOOLTIP",
                    &name,
                    (x, slot[1] + slot[3] + 4.0 * scale),
                    scale,
                    [255; 4],
                );
            }
        }
    }
}

pub fn inside(r: [f32; 4], (x, y): (f32, f32)) -> bool {
    x >= r[0] && y >= r[1] && x < r[0] + r[2] && y < r[1] + r[3]
}

/// `CANOE_PAINTING` as "Canoe painting"; `T_ABBE` as "Ask about Abbe".
pub fn display_name(id: &str) -> String {
    let (prefix, rest) = match id.strip_prefix("T_") {
        Some(rest) => ("Ask about ", rest),
        None if id == "Z_CHAT" => return "Chat".into(),
        None => ("", id),
    };
    let words = rest.replace('_', " ").to_ascii_lowercase();
    let mut chars = words.chars();
    let cased = match chars.next() {
        Some(c) if prefix.is_empty() => c.to_ascii_uppercase().to_string() + chars.as_str(),
        Some(c) => c.to_ascii_uppercase().to_string() + chars.as_str(),
        None => String::new(),
    };
    format!("{prefix}{cased}")
}

fn load_font(lib: &Library, name: &str) -> Option<Font> {
    let def = Definition::parse(&lib.read(&format!("{name}.FON")).ok()?);
    let sheet = def.bitmap.clone().unwrap_or_else(|| name.to_string());
    let sheet = sheet.trim_end_matches(".bmp").trim_end_matches(".BMP");
    let image = image::decode_raw(&lib.read(&format!("{sheet}.BMP")).ok()?).ok()?;
    Some(Font::new(&def, &image))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_read_naturally() {
        assert_eq!(display_name("CANOE_PAINTING"), "Canoe painting");
        assert_eq!(display_name("T_ABBE"), "Ask about Abbe");
        assert_eq!(display_name("Z_CHAT"), "Chat");
    }
}
