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
    /// A topic bar: closing it ends the conversation.
    pub topics: bool,
    pub x: f32,
    pub y: f32,
}

/// The inventory screen's state.
#[derive(Default)]
pub struct Inventory {
    pub open: bool,
    /// What the player carries, in the order shown.
    pub items: Vec<String>,
    /// An item shown full size.
    pub closeup: Option<String>,
}

pub struct Ui {
    fonts: HashMap<&'static str, LoadedFont>,
    /// Item to the stem of its pictures, from INVENTORYSPRITES.TXT.
    sprites: HashMap<String, String>,
    pictures: HashMap<String, Option<(OverlayTexture, u32, u32)>>,
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
        // NAME = stem, with ; comments; the one file of its kind.
        let mut sprites = HashMap::new();
        if let Ok(bytes) = lib.read("INVENTORYSPRITES.TXT") {
            let text: String = bytes.iter().map(|&b| b as char).collect();
            for line in text.lines() {
                let line = line.split(';').next().unwrap_or("").trim();
                if let Some((name, stem)) = line.split_once('=') {
                    sprites.insert(name.trim().to_ascii_uppercase(), stem.trim().to_string());
                }
            }
        }
        Self {
            fonts,
            sprites,
            pictures: HashMap::new(),
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
    /// A short message at the top of the screen, such as "Game saved".
    pub fn notice(&self, overlay: &mut Overlay, text: &str, size: (u32, u32)) {
        let scale = Self::scale(size.1);
        let (w, h) = self.measure("F_TOOLTIP", text);
        let (bx, by) = ((size.0 as f32 - w * scale) / 2.0, 12.0 * scale);
        overlay.rect(
            [bx - 8.0, by - 4.0, w * scale + 16.0, h * scale + 8.0],
            [250, 240, 210, 230],
        );
        self.text(overlay, "F_TOOLTIP", text, (bx, by), scale, [0, 0, 0, 255]);
    }

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

impl Ui {
    /// An item's picture: `9` for the inventory list, `6` for a close-up,
    /// with or without an underscore before the number.
    fn picture(
        &mut self,
        lib: &Library,
        renderer: &mut Renderer,
        item: &str,
        size: char,
    ) -> Option<(OverlayTexture, u32, u32)> {
        let stem = self.sprites.get(&item.to_ascii_uppercase())?.clone();
        *self
            .pictures
            .entry(format!("{stem}{size}"))
            .or_insert_with(|| {
                // Close-ups are mostly `…6_ALPHA` (one is `…_6_ALPHA`).
                let bytes = [
                    format!("{stem}{size}.BMP"),
                    format!("{stem}_{size}.BMP"),
                    format!("{stem}{size}_ALPHA.BMP"),
                    format!("{stem}_{size}_ALPHA.BMP"),
                ]
                .iter()
                .find_map(|name| lib.read(name).ok())?;
                let image = gk3_render::color_key(&image::decode(&bytes).ok()?);
                let (w, h) = (image.width, image.height);
                Some((renderer.add_overlay_texture(&image), w, h))
            })
    }

    /// Where each carried item is drawn on the inventory screen.
    pub fn inventory_slots(items: &[String], size: (u32, u32)) -> Vec<[f32; 4]> {
        let scale = Self::scale(size.1);
        let cell = 80.0 * scale;
        let columns = ((size.0 as f32 * 0.8) / cell).floor().max(1.0) as usize;
        let x0 = (size.0 as f32 - columns.min(items.len().max(1)) as f32 * cell) / 2.0;
        let y0 = size.1 as f32 * 0.2;
        (0..items.len())
            .map(|i| {
                [
                    x0 + (i % columns) as f32 * cell,
                    y0 + (i / columns) as f32 * cell,
                    cell - 8.0,
                    cell - 8.0,
                ]
            })
            .collect()
    }

    pub fn inventory(
        &mut self,
        lib: &Library,
        renderer: &mut Renderer,
        overlay: &mut Overlay,
        state: &Inventory,
        pointer: (f32, f32),
        size: (u32, u32),
    ) {
        let items = &state.items;
        let scale = Self::scale(size.1);
        overlay.rect([0.0, 0.0, size.0 as f32, size.1 as f32], [10, 8, 6, 200]);
        if let Some(item) = &state.closeup {
            if let Some((texture, w, h)) = self.picture(lib, renderer, item, '6') {
                let fit = ((size.0 as f32 * 0.8) / w as f32).min((size.1 as f32 * 0.8) / h as f32);
                let (dw, dh) = (w as f32 * fit, h as f32 * fit);
                overlay.image(
                    texture,
                    [0.0, 0.0, w as f32, h as f32],
                    [
                        (size.0 as f32 - dw) / 2.0,
                        (size.1 as f32 - dh) / 2.0,
                        dw,
                        dh,
                    ],
                    [255; 4],
                );
            }
            return;
        }
        let title = "Inventory";
        let (tw, _) = self.measure("F_CAPTION_EGO", title);
        self.text(
            overlay,
            "F_CAPTION_EGO",
            title,
            ((size.0 as f32 - tw * scale) / 2.0, size.1 as f32 * 0.1),
            scale,
            [255; 4],
        );
        for (item, slot) in items.iter().zip(Self::inventory_slots(items, size)) {
            let hovered = inside(slot, pointer);
            if hovered {
                overlay.rect(
                    [slot[0] - 3.0, slot[1] - 3.0, slot[2] + 6.0, slot[3] + 6.0],
                    [200, 170, 110, 120],
                );
            }
            match self.picture(lib, renderer, item, '9') {
                Some((texture, w, h)) => {
                    let fit = (slot[2] / w as f32).min(slot[3] / h as f32);
                    let (dw, dh) = (w as f32 * fit, h as f32 * fit);
                    overlay.image(
                        texture,
                        [0.0, 0.0, w as f32, h as f32],
                        [
                            slot[0] + (slot[2] - dw) / 2.0,
                            slot[1] + (slot[3] - dh) / 2.0,
                            dw,
                            dh,
                        ],
                        [255; 4],
                    );
                }
                None => overlay.rect(slot, [80, 60, 40, 200]),
            }
            if hovered {
                let name = display_name(item);
                let (w, _) = self.measure("F_TOOLTIP", &name);
                let at = (
                    slot[0] + slot[2] / 2.0 - w * scale / 2.0,
                    slot[1] + slot[3] + 2.0,
                );
                self.text(overlay, "F_TOOLTIP", &name, at, scale, [255; 4]);
            }
        }
    }

    /// A small button in the corner that opens the inventory.
    pub fn inventory_button(size: (u32, u32)) -> [f32; 4] {
        let scale = Self::scale(size.1);
        [
            8.0,
            size.1 as f32 - 36.0 * scale,
            90.0 * scale,
            28.0 * scale,
        ]
    }

    pub fn draw_inventory_button(
        &self,
        overlay: &mut Overlay,
        pointer: (f32, f32),
        size: (u32, u32),
    ) {
        let scale = Self::scale(size.1);
        let r = Self::inventory_button(size);
        let colour = if inside(r, pointer) {
            [120, 90, 50, 220]
        } else {
            [60, 45, 30, 200]
        };
        overlay.rect(r, colour);
        self.text(
            overlay,
            "F_CAPTION_EGO",
            "Inventory",
            (r[0] + 8.0, r[1] + 4.0 * scale),
            scale,
            [255; 4],
        );
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
