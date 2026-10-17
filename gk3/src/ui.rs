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
    pub rules: Vec<Rule>,
    /// A topic bar: closing it ends the conversation.
    pub topics: bool,
    pub x: f32,
    pub y: f32,
}

impl Menu {
    /// A verb menu ending, as G-Engine's always does, in CANCEL: the
    /// noun's own CANCEL action if it has one, or one that just closes it.
    /// Without `cancel` (the story's `StopVerbCancel`) only a CANCEL of the
    /// noun's own is offered.
    pub fn new(
        noun: String,
        mut rules: Vec<Rule>,
        (topics, cancel): (bool, bool),
        (x, y): (f32, f32),
    ) -> Self {
        let custom = rules
            .iter()
            .position(|r| r.verb == "CANCEL")
            .map(|i| rules.remove(i));
        let plain = || Rule {
            noun: noun.clone(),
            verb: "CANCEL".into(),
            case: "ALL".into(),
            approach: None,
            target: None,
            script: String::new(),
            source: "UI".into(),
        };
        if let Some(rule) = custom.or_else(|| cancel.then(plain)) {
            rules.push(rule);
        }
        Self {
            rules,
            topics,
            x,
            y,
        }
    }
}

/// A verb's pictures, from `VERBS.TXT`: its icon up, hovered and pressed,
/// and the cursor something with that verb of its own shows.
#[derive(Debug, Clone, Default)]
struct VerbIcon {
    up: String,
    hover: String,
    down: String,
    cursor: Option<String>,
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

/// The card between timeblocks: a picture for the hour, its name written in
/// a frame at a time, and a Continue button. Laid out as G-Engine's
/// `TimeblockScreen`, in the 640x480 picture's own pixels.
pub struct TitleCard {
    pub timeblock: String,
    /// When it went up, in the window's seconds.
    pub shown: f64,
    /// Lettering frames, from `D<timeblock>.SEQ`.
    frames: Vec<String>,
}

impl TitleCard {
    pub fn new(lib: &Library, timeblock: &str, shown: f64) -> Self {
        let timeblock = timeblock.to_ascii_uppercase();
        let frames = lib
            .read(&format!("D{timeblock}.SEQ"))
            .map(|b| {
                let text: String = b.iter().map(|&c| c as char).collect();
                text.lines()
                    .find_map(|l| l.trim().strip_prefix("Sprite List="))
                    .map(|list| list.split(',').map(|s| s.trim().to_string()).collect())
                    .unwrap_or_default()
            })
            .unwrap_or_default();
        Self {
            timeblock,
            shown,
            frames,
        }
    }

    /// Where the lettering's bottom left sits on the picture, measured from
    /// the picture's bottom left; G-Engine's table.
    fn lettering_at(&self) -> (f32, f32) {
        match self.timeblock.as_str() {
            "112P" | "104P" | "212P" | "310A" => (14.0, 63.0),
            "102P" => (8.0, 63.0),
            "210A" => (14.0, 65.0),
            "312P" => (13.0, 64.0),
            _ => (14.0, 64.0),
        }
    }
}

/// The title screen's buttons, left to right.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitleButton {
    Intro,
    Play,
    Restore,
    Quit,
}

impl TitleButton {
    pub const ALL: [TitleButton; 4] = [Self::Intro, Self::Play, Self::Restore, Self::Quit];

    fn bitmap(self) -> &'static str {
        match self {
            Self::Intro => "TITLE_INTRO",
            Self::Play => "TITLE_PLAY",
            Self::Restore => "TITLE_RESTORE",
            Self::Quit => "TITLE_QUIT",
        }
    }
}

/// The video texture slot full-screen movies use (SIDNEY's use small ones).
const MOVIE_SLOT: usize = usize::MAX;

/// A 640x480 screen fitted into the window: its top left and scale.
#[derive(Debug, Clone, Copy)]
pub struct Space {
    pub ox: f32,
    pub oy: f32,
    pub fit: f32,
}

impl Space {
    pub fn of(size: (u32, u32)) -> Self {
        let (ox, oy, fit) = Ui::card_frame(size);
        Self { ox, oy, fit }
    }

    /// A rectangle in the screen's pixels, on the window.
    pub fn rect(&self, r: [f32; 4]) -> [f32; 4] {
        [
            self.ox + r[0] * self.fit,
            self.oy + r[1] * self.fit,
            r[2] * self.fit,
            r[3] * self.fit,
        ]
    }
}

/// How text sits in its box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
}

/// Where the fingerprint kit's parts are on screen, as last drawn.
#[derive(Debug, Clone, Default)]
pub struct KitLayout {
    pub exit: [f32; 4],
    pub brush: [f32; 4],
    pub dust: [f32; 4],
    pub tape: [f32; 4],
    pub cloth: [f32; 4],
    pub panel: [f32; 4],
    pub prints: Vec<[f32; 4]>,
}

/// The binoculars' buttons.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinocsButton {
    Up,
    Down,
    Left,
    Right,
    Exit,
    Zoom,
}

/// The lettering's frame rate.
const CARD_FPS: f64 = 15.0;

pub struct Ui {
    fonts: HashMap<String, LoadedFont>,
    /// Item to the stem of its pictures, from INVENTORYSPRITES.TXT.
    sprites: HashMap<String, String>,
    pictures: HashMap<String, Option<(OverlayTexture, u32, u32)>>,
    /// Textures that movie frames are drawn into, by slot.
    videos: HashMap<usize, OverlayTexture>,
    /// The movie frame last sent to its texture: movie and frame number.
    movie_frame: Option<(String, usize)>,
    /// Verb to its icon names (normal, hover), from VERBS.TXT.
    icon_names: HashMap<String, VerbIcon>,
    icons: HashMap<String, Option<OverlayTexture>>,
    /// The game's interface text, from `ESTRINGS.TXT`, by lower-case key.
    strings: HashMap<String, String>,
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
                fonts.insert(name.to_string(), LoadedFont { font, texture });
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
                    VerbIcon {
                        up: up.to_string(),
                        hover: hover.to_string(),
                        down: line.get("down").unwrap_or(hover).to_string(),
                        cursor: line
                            .get("cursor")
                            .map(str::trim)
                            .filter(|c| !c.is_empty() && *c != "???")
                            .map(str::to_string),
                    },
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
            videos: HashMap::new(),
            movie_frame: None,
            icon_names,
            icons: HashMap::new(),
            strings: lib
                .read("ESTRINGS.TXT")
                .map(|b| {
                    gk3_game::state::latin1(&b)
                        .lines()
                        .filter_map(|l| {
                            let (k, v) = l.split_once('=')?;
                            Some((k.trim().to_ascii_lowercase(), v.trim().to_string()))
                        })
                        .collect()
                })
                .unwrap_or_default(),
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
        self.lines(overlay, &world.captions, world.clock, &world.state.ego, size);
    }

    /// The last two captions begun by `now` (and not over), at the foot of
    /// the screen; the player's own lines in their colour.
    pub fn lines(
        &self,
        overlay: &mut Overlay,
        captions: &[crate::world::Caption],
        now: f64,
        ego: &str,
        size: (u32, u32),
    ) {
        let scale = Self::scale(size.1);
        let (w, h) = (size.0 as f32, size.1 as f32);
        let mut y = h - 24.0 * scale;
        for caption in captions
            .iter()
            .rev()
            .filter(|c| c.from <= now && now < c.until)
            .take(2)
        {
            let ego = caption
                .speaker
                .as_deref()
                .is_some_and(|s| s.eq_ignore_ascii_case(ego));
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

    /// Where each verb's icon is drawn: side by side, the row centred on
    /// where the menu was opened and kept on screen, as G-Engine's
    /// `ActionBar`.
    pub fn menu_slots(menu: &Menu, size: (u32, u32)) -> Vec<[f32; 4]> {
        let scale = Self::scale(size.1);
        let icon = ICON * scale;
        let total = menu.rules.len() as f32 * icon;
        let x0 = (menu.x - total / 2.0).clamp(0.0, (size.0 as f32 - total).max(0.0));
        let y0 = (menu.y - icon / 2.0).clamp(0.0, (size.1 as f32 - icon).max(0.0));
        (0..menu.rules.len())
            .map(|i| [x0 + i as f32 * icon, y0, icon, icon])
            .collect()
    }

    /// The cursor something shows whose own verb is this (`EXIT` shows an
    /// arrow), if any.
    pub fn verb_cursor(&self, verb: &str) -> Option<String> {
        self.icon_names
            .get(&verb.to_ascii_uppercase())
            .and_then(|i| i.cursor.clone())
    }

    /// What a verb's tooltip says: the game's `v_` string for it.
    pub fn verb_tip(&self, verb: &str) -> String {
        let key = format!("v_{}", verb.to_ascii_lowercase());
        self.strings
            .get(&key)
            .cloned()
            .unwrap_or_else(|| display_name(verb))
    }

    /// Draws a verb menu: each verb's icon, hovered or pressed under the
    /// pointer. Returns the verb under the pointer, for its tooltip.
    pub fn menu(
        &mut self,
        lib: &Library,
        renderer: &mut Renderer,
        overlay: &mut Overlay,
        menu: &Menu,
        (pointer, pressed): ((f32, f32), bool),
        size: (u32, u32),
    ) -> Option<String> {
        let scale = Self::scale(size.1);
        let slots = Self::menu_slots(menu, size);
        let mut under = None;
        for (rule, slot) in menu.rules.iter().zip(&slots) {
            let hovered = inside(*slot, pointer);
            if hovered {
                under = Some(rule.verb.clone());
            }
            let icon = self.icon_names.get(&rule.verb).cloned().and_then(|i| {
                let name = match (hovered, pressed) {
                    (true, true) => &i.down,
                    (true, false) => &i.hover,
                    _ => &i.up,
                };
                self.icon(lib, renderer, name)
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
        }
        under
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

    /// A bitmap for the interface, colour-keyed, cached by name.
    pub(crate) fn bitmap(
        &mut self,
        lib: &Library,
        renderer: &mut Renderer,
        name: &str,
    ) -> Option<(OverlayTexture, u32, u32)> {
        *self
            .pictures
            .entry(format!("BMP:{}", name.to_ascii_uppercase()))
            .or_insert_with(|| {
                let bytes = lib.read(&format!("{name}.BMP")).ok()?;
                let image = gk3_render::color_key(&image::decode(&bytes).ok()?);
                let (w, h) = (image.width, image.height);
                Some((renderer.add_overlay_texture(&image), w, h))
            })
    }

    /// A save's thumbnail, loaded once for each version of the save.
    pub fn thumbnail(
        &mut self,
        renderer: &mut Renderer,
        key: &str,
        load: impl FnOnce() -> Option<image::Image>,
    ) -> Option<(OverlayTexture, u32, u32)> {
        *self
            .pictures
            .entry(format!("THUMB:{key}"))
            .or_insert_with(|| {
                let image = load()?;
                let (w, h) = (image.width, image.height);
                Some((renderer.add_overlay_texture(&image), w, h))
            })
    }

    /// An interface bitmap's size, loading it if need be.
    pub fn bitmap_size(
        &mut self,
        lib: &Library,
        renderer: &mut Renderer,
        name: &str,
    ) -> Option<(f32, f32)> {
        self.bitmap(lib, renderer, name)
            .map(|(_, w, h)| (w as f32, h as f32))
    }

    /// An inventory item's list picture.
    pub fn item_picture(
        &mut self,
        lib: &Library,
        renderer: &mut Renderer,
        item: &str,
    ) -> Option<(OverlayTexture, u32, u32)> {
        self.picture(lib, renderer, item, '9')
    }

    /// The title screen: `TITLE.BMP` with its buttons along the foot, from
    /// the right (as G-Engine lays them out at 640x480). Each button shows
    /// its up, hover, down or unavailable picture. Returns where they are.
    #[allow(clippy::too_many_arguments)]
    pub fn title_screen(
        &mut self,
        lib: &Library,
        renderer: &mut Renderer,
        overlay: &mut Overlay,
        pointer: (f32, f32),
        held: Option<TitleButton>,
        can_restore: bool,
        size: (u32, u32),
    ) -> Vec<(TitleButton, [f32; 4])> {
        let (w, h) = (size.0 as f32, size.1 as f32);
        overlay.rect([0.0, 0.0, w, h], [0, 0, 0, 255]);
        let space = Space::of(size);
        self.sprite(lib, renderer, overlay, space, "TITLE", (0.0, 0.0), [255; 4]);
        let mut out = Vec::new();
        let mut right = 640.0 - 33.0;
        for button in TitleButton::ALL.into_iter().rev() {
            let name = button.bitmap();
            let Some((_, bw, bh)) = self.bitmap(lib, renderer, &format!("{name}_U")) else {
                continue;
            };
            let (bw, bh) = (bw as f32, bh as f32);
            let at = (right - bw, 480.0 - 27.0 - bh);
            right -= bw + 18.0;
            let rect = space.rect([at.0, at.1, bw, bh]);
            let state = if button == TitleButton::Restore && !can_restore {
                "X"
            } else if held == Some(button) {
                "D"
            } else if inside(rect, pointer) {
                "H"
            } else {
                "U"
            };
            self.sprite(lib, renderer, overlay, space, &format!("{name}_{state}"), at, [255; 4]);
            out.push((button, rect));
        }
        out
    }

    /// A full-screen movie: its frame as large as fits the window, black
    /// round it, and its captions.
    pub fn movie(
        &mut self,
        renderer: &mut Renderer,
        overlay: &mut Overlay,
        show: &crate::world::Showing,
        (ego, captions): (&str, bool),
        size: (u32, u32),
    ) {
        let (w, h) = (size.0 as f32, size.1 as f32);
        overlay.rect([0.0, 0.0, w, h], [0, 0, 0, 255]);
        if let Some((picture, number)) = show.movie.picture() {
            let shown = Some((show.name.clone(), number));
            let changed = self.movie_frame != shown;
            self.movie_frame = shown;
            let tex = self.video_texture(renderer, MOVIE_SLOT, picture, changed);
            let (pw, ph) = (picture.width as f32, picture.height as f32);
            // No more than twice its size, as the original.
            let fit = (w / pw).min(h / ph).min(2.0);
            let (dw, dh) = (pw * fit, ph * fit);
            overlay.image(
                tex,
                [0.0, 0.0, pw, ph],
                [(w - dw) / 2.0, (h - dh) / 2.0, dw, dh],
                [255, 255, 255, 255],
            );
        }
        if captions {
            self.lines(overlay, &show.captions, show.elapsed, ego, size);
        }
    }

    /// A plain white texture, for want of anything else.
    pub fn white(&mut self) -> (OverlayTexture, u32, u32) {
        (gk3_render::WHITE, 1, 1)
    }

    /// A movie frame, into its slot's texture (redrawn when `changed`).
    pub fn video_texture(
        &mut self,
        renderer: &mut Renderer,
        slot: usize,
        frame: &image::Image,
        changed: bool,
    ) -> OverlayTexture {
        match self.videos.get(&slot) {
            Some(&tex) => {
                if changed {
                    renderer.update_overlay_texture(tex, frame);
                }
                tex
            }
            None => {
                let tex = renderer.add_overlay_texture(frame);
                self.videos.insert(slot, tex);
                tex
            }
        }
    }

    /// A picture with one colour made see-through.
    #[allow(clippy::too_many_arguments)]
    pub fn keyed_sprite(
        &mut self,
        lib: &Library,
        renderer: &mut Renderer,
        overlay: &mut Overlay,
        space: Space,
        name: &str,
        (x, y): (f32, f32),
        key: [u8; 3],
    ) {
        let entry = self
            .pictures
            .entry(format!("KEYED:{}:{key:?}", name.to_ascii_uppercase()))
            .or_insert_with(|| {
                let mut image = image::decode(&lib.read(&format!("{name}.BMP")).ok()?).ok()?;
                for px in image.rgba.chunks_mut(4) {
                    if (0..3)
                        .map(|i| (px[i] as i32 - key[i] as i32).abs())
                        .sum::<i32>()
                        < 24
                    {
                        px[3] = 0;
                    }
                }
                let (w, h) = (image.width, image.height);
                Some((renderer.add_overlay_texture(&image), w, h))
            });
        if let Some((tex, w, h)) = *entry {
            let (w, h) = (w as f32, h as f32);
            overlay.image(tex, [0.0, 0.0, w, h], space.rect([x, y, w, h]), [255; 4]);
        }
    }

    /// How many lines a text takes in a font, wrapped to a width.
    pub fn wrap(
        &mut self,
        font: &str,
        lib: &Library,
        renderer: &mut Renderer,
        text: &str,
        width: f32,
    ) -> usize {
        if !self.need_font(lib, renderer, font) {
            return 0;
        }
        let f = &self.fonts[font].font;
        text.split('\n')
            .map(|para| f.wrap(para, width as i32).len().max(1))
            .sum()
    }

    /// Writes some of a text's wrapped lines: `count` of them from `skip`.
    #[allow(clippy::too_many_arguments)]
    pub fn write_lines(
        &mut self,
        lib: &Library,
        renderer: &mut Renderer,
        overlay: &mut Overlay,
        space: Space,
        font: &str,
        text: &str,
        (x, y): (f32, f32),
        width: f32,
        skip: usize,
        count: usize,
    ) {
        if !self.need_font(lib, renderer, font) {
            return;
        }
        let f = &self.fonts[font].font;
        let lines: Vec<String> = text
            .split('\n')
            .flat_map(|para| {
                let wrapped = f.wrap(para, width as i32);
                if wrapped.is_empty() {
                    vec![String::new()]
                } else {
                    wrapped
                }
            })
            .collect();
        let lh = f.line_height() as f32;
        for (i, line) in lines.iter().skip(skip).take(count).enumerate() {
            self.text(
                overlay,
                font,
                line,
                (
                    space.ox + x * space.fit,
                    space.oy + (y + i as f32 * lh) * space.fit,
                ),
                space.fit,
                [255; 4],
            );
        }
    }

    /// The card's picture on screen: its top left and scale.
    fn card_frame(size: (u32, u32)) -> (f32, f32, f32) {
        let fit = (size.0 as f32 / 640.0).min(size.1 as f32 / 480.0);
        (
            (size.0 as f32 - 640.0 * fit) / 2.0,
            (size.1 as f32 - 480.0 * fit) / 2.0,
            fit,
        )
    }

    /// The Continue button: 81x26 at (90, 16) from the picture's bottom left.
    pub fn card_continue(size: (u32, u32)) -> [f32; 4] {
        let (ox, oy, fit) = Self::card_frame(size);
        [
            ox + 90.0 * fit,
            oy + (480.0 - 16.0 - 26.0) * fit,
            81.0 * fit,
            26.0 * fit,
        ]
    }

    /// The Save button beside it, at (180, 16).
    pub fn card_save(size: (u32, u32)) -> [f32; 4] {
        let mut r = Self::card_continue(size);
        r[0] += 90.0 * Self::card_frame(size).2;
        r
    }

    pub fn title_card(
        &mut self,
        lib: &Library,
        renderer: &mut Renderer,
        overlay: &mut Overlay,
        card: &TitleCard,
        (now, pointer, buttons): (f64, (f32, f32), bool),
        size: (u32, u32),
    ) {
        let (sw, sh) = (size.0 as f32, size.1 as f32);
        overlay.rect([0.0, 0.0, sw, sh], [0, 0, 0, 255]);
        let (ox, oy, fit) = Self::card_frame(size);
        if let Some((tex, w, h)) = self.bitmap(lib, renderer, &format!("TBT{}", card.timeblock)) {
            let (w, h) = (w as f32, h as f32);
            overlay.image(tex, [0.0, 0.0, w, h], [ox, oy, w * fit, h * fit], [255; 4]);
        }
        if !card.frames.is_empty() {
            let i = (((now - card.shown) * CARD_FPS) as usize).min(card.frames.len() - 1);
            if let Some((tex, w, h)) = self.bitmap(lib, renderer, &card.frames[i]) {
                let (x, y) = card.lettering_at();
                let (w, h) = (w as f32, h as f32);
                overlay.image(
                    tex,
                    [0.0, 0.0, w, h],
                    [ox + x * fit, oy + (480.0 - y - h) * fit, w * fit, h * fit],
                    [255; 4],
                );
            }
        }
        // A card on a timer (a new game's first) has no buttons, as
        // G-Engine's `TimeblockScreen`.
        if !buttons {
            return;
        }
        for (button, name) in [
            (Self::card_continue(size), "TB_CONT"),
            (Self::card_save(size), "TB_SAVE"),
        ] {
            let state = if inside(button, pointer) { "H" } else { "U" };
            if let Some((tex, w, h)) = self.bitmap(lib, renderer, &format!("{name}_{state}")) {
                overlay.image(tex, [0.0, 0.0, w as f32, h as f32], button, [255; 4]);
            }
        }
    }

    /// The driving map, drawn to fit the window. Returns where each button
    /// is, by id, for clicks.
    pub fn map(
        &mut self,
        lib: &Library,
        renderer: &mut Renderer,
        overlay: &mut Overlay,
        map: &crate::map::MapScreen,
        pointer: (f32, f32),
        size: (u32, u32),
    ) -> Vec<(&'static str, [f32; 4])> {
        overlay.rect([0.0, 0.0, size.0 as f32, size.1 as f32], [0, 0, 0, 255]);
        let (ox, oy, fit) = Self::card_frame(size);
        if let Some((tex, w, h)) = self.bitmap(lib, renderer, "DM_BASE") {
            let (w, h) = (w as f32, h as f32);
            overlay.image(tex, [0.0, 0.0, w, h], [ox, oy, w * fit, h * fit], [255; 4]);
        }
        let mut buttons = Vec::new();
        let mut hovered = None;
        for place in crate::map::PLACES
            .iter()
            .filter(|p| map.shown.contains(&p.id))
        {
            let Some((up, w, h)) = self.bitmap(lib, renderer, &format!("DM_{}_UL", place.id))
            else {
                continue;
            };
            let rect = [
                ox + place.at.0 * fit,
                oy + place.at.1 * fit,
                w as f32 * fit,
                h as f32 * fit,
            ];
            let hover = map.interactive() && inside(rect, pointer);
            let lit = map.flash_lit && map.flashing.contains(&place.id);
            let tex = match hover || lit {
                true => self
                    .bitmap(lib, renderer, &format!("DM_{}", place.id))
                    .map_or(up, |b| b.0),
                false => up,
            };
            overlay.image(tex, [0.0, 0.0, w as f32, h as f32], rect, [255; 4]);
            if hover {
                hovered = Some(place.id);
            }
            buttons.push((place.id, rect));
        }
        for blip in map.blips.iter().filter(|b| b.visible) {
            let s = 6.0 * fit;
            let (x, y) = (
                ox + blip.at.0 * fit - s / 2.0,
                oy + blip.at.1 * fit - s / 2.0,
            );
            let [r, g, b] = blip.color;
            // A 6x6 dot with its corners clipped.
            overlay.rect([x + s / 3.0, y, s / 3.0, s], [r, g, b, 255]);
            overlay.rect([x, y + s / 3.0, s, s / 3.0], [r, g, b, 255]);
            overlay.rect(
                [x + s / 6.0, y + s / 6.0, s * 2.0 / 3.0, s * 2.0 / 3.0],
                [r, g, b, 255],
            );
        }
        if let Some(id) = hovered {
            let key = format!("dm_{}", id.to_ascii_lowercase());
            let name = self.strings.get(&key).cloned().unwrap_or(key);
            self.tooltip(overlay, &name, pointer, size);
        }
        buttons
    }

    /// The binoculars' frame and buttons over the view. `held` is whether
    /// the mouse button is down, so an arrow shows pressed. Returns the
    /// buttons' places for clicks.
    pub fn binocs(
        &mut self,
        lib: &Library,
        renderer: &mut Renderer,
        overlay: &mut Overlay,
        (zoomed, can_zoom): (bool, bool),
        (pointer, held): ((f32, f32), bool),
        size: (u32, u32),
    ) -> Vec<(BinocsButton, [f32; 4])> {
        let (sw, sh) = (size.0 as f32, size.1 as f32);
        let (ox, oy, fit) = Self::card_frame(size);
        // Black round the 640x480 mask, then the mask with its cutout.
        overlay.rect([0.0, 0.0, sw, oy], [0, 0, 0, 255]);
        overlay.rect([0.0, oy + 480.0 * fit, sw, sh], [0, 0, 0, 255]);
        overlay.rect([0.0, 0.0, ox, sh], [0, 0, 0, 255]);
        overlay.rect([ox + 640.0 * fit, 0.0, sw, sh], [0, 0, 0, 255]);
        let at = |x: f32, y: f32, w: u32, h: u32| {
            [ox + x * fit, oy + y * fit, w as f32 * fit, h as f32 * fit]
        };
        let mut draw = |ui: &mut Self, name: &str, x: f32, y: f32| -> Option<[f32; 4]> {
            let (tex, w, h) = ui.bitmap(lib, renderer, name)?;
            let rect = at(x, y, w, h);
            overlay.image(tex, [0.0, 0.0, w as f32, h as f32], rect, [255; 4]);
            Some(rect)
        };
        draw(self, "BINOCMASK", 0.0, 0.0);
        draw(self, "BINOCBTNAREA", 264.0, 345.0);
        let mut buttons = Vec::new();
        // G-Engine's places, at 640x480.
        let arrows = [
            (BinocsButton::Up, "UP", 308.0, 349.0, (25, 45)),
            (BinocsButton::Down, "DOWN", 308.0, 413.0, (25, 45)),
            (BinocsButton::Left, "LEFT", 266.0, 390.0, (45, 25)),
            (BinocsButton::Right, "RIGHT", 329.0, 390.0, (45, 25)),
        ];
        for (button, name, x, y, (w, h)) in arrows {
            let rect = at(x, y, w, h);
            let state = if held && inside(rect, pointer) {
                "D"
            } else {
                "U"
            };
            draw(self, &format!("BINOCBTN{name}{state}"), x, y);
            buttons.push((button, rect));
        }
        let state = |enabled: bool, rect: [f32; 4]| match (enabled, inside(rect, pointer), held) {
            (false, _, _) => "DIS",
            (true, true, true) => "D",
            (true, true, false) => "HOV",
            _ => "U",
        };
        let exit = at(95.0, 389.0, 135, 30);
        draw(
            self,
            &format!("BINOCBTNEXIT{}", state(!zoomed, exit)),
            95.0,
            389.0,
        );
        buttons.push((BinocsButton::Exit, exit));
        let zoom = at(399.0, 389.0, 134, 30);
        let (name, enabled) = if zoomed {
            ("ZOOMOUT", true)
        } else {
            ("ZOOMIN", can_zoom)
        };
        draw(
            self,
            &format!("BINOCBTN{name}{}", state(enabled, zoom)),
            399.0,
            389.0,
        );
        buttons.push((BinocsButton::Zoom, zoom));
        buttons
    }

    /// A bitmap with black as see-through, for fingerprints.
    fn print_bitmap(
        &mut self,
        lib: &Library,
        renderer: &mut Renderer,
        name: &str,
    ) -> Option<(OverlayTexture, u32, u32)> {
        *self
            .pictures
            .entry(format!("PRINT:{}", name.to_ascii_uppercase()))
            .or_insert_with(|| {
                let mut image = image::decode(&lib.read(name).ok()?).ok()?;
                for px in image.rgba.chunks_mut(4) {
                    if px[0] < 8 && px[1] < 8 && px[2] < 8 {
                        px[3] = 0;
                    }
                }
                let (w, h) = (image.width, image.height);
                Some((renderer.add_overlay_texture(&image), w, h))
            })
    }

    /// A cursor (`.CUR`): its picture, with its alpha bitmap, and hotspot.
    fn cursor(
        &mut self,
        lib: &Library,
        renderer: &mut Renderer,
        name: &str,
    ) -> Option<(OverlayTexture, u32, u32, (f32, f32))> {
        let text = gk3_game::state::latin1(&lib.read(&format!("{name}.CUR")).ok()?);
        let field = |k: &str| {
            text.lines().find_map(|l| {
                let (key, v) = l.split_once('=')?;
                key.trim()
                    .eq_ignore_ascii_case(k)
                    .then(|| v.trim().to_string())
            })
        };
        let hotspot = field("Hotspot")
            .and_then(|h| {
                let (x, y) = h.split_once(',')?;
                Some((x.trim().parse().ok()?, y.trim().parse().ok()?))
            })
            .unwrap_or((0.0, 0.0));
        let sprite = field("Sprite Name").unwrap_or_else(|| name.to_string());
        let (tex, w, h) = (*self
            .pictures
            .entry(format!("CUR:{}", name.to_ascii_uppercase()))
            .or_insert_with(|| {
                let mut image = image::decode(&lib.read(&format!("{sprite}.BMP")).ok()?).ok()?;
                // The alpha bitmap, where there is one, says how solid each pixel is.
                if let Some(alpha) = lib
                    .read(&format!("{sprite}A.BMP"))
                    .ok()
                    .and_then(|b| image::decode_raw(&b).ok())
                    .filter(|a| a.width == image.width && a.height == image.height)
                {
                    for (px, a) in image.rgba.chunks_mut(4).zip(alpha.rgba.chunks(4)) {
                        px[3] = a[0];
                    }
                }
                let (w, h) = (image.width, image.height);
                Some((renderer.add_overlay_texture(&image), w, h))
            }))?;
        Some((tex, w, h, hotspot))
    }

    /// The fingerprint kit, over the whole window.
    pub fn fingerprint(
        &mut self,
        lib: &Library,
        renderer: &mut Renderer,
        overlay: &mut Overlay,
        kit: &crate::fingerprint::Kit,
        (pointer, held): ((f32, f32), bool),
        size: (u32, u32),
    ) -> KitLayout {
        use crate::fingerprint::{CLOTH, DUST, PANEL, TAPE, Tool};
        overlay.rect([0.0, 0.0, size.0 as f32, size.1 as f32], [0, 0, 0, 255]);
        let (ox, oy, fit) = Self::card_frame(size);
        let screen = |r: [f32; 4]| [ox + r[0] * fit, oy + r[1] * fit, r[2] * fit, r[3] * fit];
        let mut layout = KitLayout {
            dust: screen(DUST),
            tape: screen(TAPE),
            cloth: screen(CLOTH),
            panel: screen(PANEL),
            ..Default::default()
        };
        if let Some((tex, w, h)) = self.bitmap(lib, renderer, "FP_BASE") {
            overlay.image(
                tex,
                [0.0, 0.0, w as f32, h as f32],
                screen([0.0, 0.0, w as f32, h as f32]),
                [255; 4],
            );
        }
        let object = kit.object.texture.trim_end_matches(".BMP");
        if let Some((tex, w, h)) = self.bitmap(lib, renderer, object) {
            overlay.image(
                tex,
                [0.0, 0.0, w as f32, h as f32],
                screen(kit.object_rect((w, h))),
                [255; 4],
            );
        }
        for (i, print) in kit.object.prints.iter().enumerate() {
            let Some((tex, w, h)) = self.print_bitmap(lib, renderer, print.texture) else {
                layout.prints.push([0.0; 4]);
                continue;
            };
            let rect = screen(kit.print_rect(i, (w, h)));
            let alpha = (kit.shown[i] * 255.0) as u8;
            if alpha > 0 {
                overlay.image(
                    tex,
                    [0.0, 0.0, w as f32, h as f32],
                    rect,
                    [255, 255, 255, alpha],
                );
            }
            layout.prints.push(rect);
        }
        // The brush lies in its place unless it is in hand.
        if let Some((tex, w, h)) = self.bitmap(lib, renderer, "FP_BRUSH") {
            let (x, y) = crate::fingerprint::BRUSH_AT;
            layout.brush = screen([x, 480.0 - y - h as f32, w as f32, h as f32]);
            if !matches!(kit.tool, Tool::Brush | Tool::DustedBrush) {
                overlay.image(tex, [0.0, 0.0, w as f32, h as f32], layout.brush, [255; 4]);
            }
        }
        if let Some((_, w, h)) = self.bitmap(lib, renderer, "EXITN") {
            let (x, y) = crate::fingerprint::EXIT_AT;
            layout.exit = screen([x, 480.0 - y - h as f32, w as f32, h as f32]);
            let state = match (inside(layout.exit, pointer), held) {
                (true, true) => "EXITD",
                (true, false) => "EXITHOV",
                _ => "EXITN",
            };
            if let Some((tex, w, h)) = self.bitmap(lib, renderer, state) {
                overlay.image(tex, [0.0, 0.0, w as f32, h as f32], layout.exit, [255; 4]);
            }
        }
        if let Some(name) = kit.tool.cursor()
            && let Some((tex, w, h, (hx, hy))) = self.cursor(lib, renderer, name)
        {
            let (w, h) = (w as f32 * fit, h as f32 * fit);
            overlay.image(
                tex,
                [0.0, 0.0, w / fit, h / fit],
                [pointer.0 - hx * fit, pointer.1 - hy * fit, w, h],
                [255; 4],
            );
        }
        layout
    }

    /// Makes sure a font is loaded, by name (`SID_TEXT_14`).
    pub fn need_font(&mut self, lib: &Library, renderer: &mut Renderer, name: &str) -> bool {
        if !self.fonts.contains_key(name)
            && let Some(font) = load_font(lib, name)
        {
            let texture = renderer.add_overlay_texture(&font.sheet);
            self.fonts
                .insert(name.to_string(), LoadedFont { font, texture });
        }
        self.fonts.contains_key(name)
    }

    /// One of the game's interface strings (`ESTRINGS.TXT`) by key, in any
    /// case; the key itself if there is none.
    pub fn string(&self, key: &str) -> String {
        self.strings
            .get(&key.to_ascii_lowercase())
            .cloned()
            .unwrap_or_else(|| key.to_string())
    }

    /// A text's width and its font's line height, in the font's pixels.
    pub fn font_size(
        &mut self,
        lib: &Library,
        renderer: &mut Renderer,
        font: &str,
        text: &str,
    ) -> (f32, f32) {
        self.need_font(lib, renderer, font);
        self.measure(font, text)
    }

    /// Writes text in a 640x480 space, in a font that brings its own
    /// colours, wrapped to `width` if given and aligned in it. Returns the
    /// height used, in screen pixels.
    #[allow(clippy::too_many_arguments)]
    pub fn write(
        &mut self,
        lib: &Library,
        renderer: &mut Renderer,
        overlay: &mut Overlay,
        space: Space,
        font: &str,
        text: &str,
        (x, y): (f32, f32),
        (width, align): (Option<f32>, Align),
    ) -> f32 {
        if !self.need_font(lib, renderer, font) {
            return 0.0;
        }
        let f = &self.fonts[font].font;
        let lines: Vec<String> = match width {
            Some(w) => text
                .split('\n')
                .flat_map(|para| f.wrap(para, w as i32))
                .collect(),
            None => text.split('\n').map(str::to_string).collect(),
        };
        let line_height = f.line_height() as f32;
        let mut ty = y;
        for line in &lines {
            let w = self.fonts[font].font.width(line) as f32;
            let tx = match (align, width) {
                (Align::Center, Some(bw)) => x + (bw - w) / 2.0,
                _ => x,
            };
            self.text(
                overlay,
                font,
                line,
                (space.ox + tx * space.fit, space.oy + ty * space.fit),
                space.fit,
                [255; 4],
            );
            ty += line_height;
        }
        ty - y
    }

    /// `text` on one line no wider than `width` in `font`, cut short with
    /// an ellipsis if it must be.
    pub fn fit_line(
        &mut self,
        lib: &Library,
        renderer: &mut Renderer,
        font: &str,
        text: &str,
        width: f32,
    ) -> String {
        if !self.need_font(lib, renderer, font) {
            return text.to_string();
        }
        let f = &self.fonts[font].font;
        if f.width(text) as f32 <= width {
            return text.to_string();
        }
        let mut cut: String = text.to_string();
        while !cut.is_empty() && f.width(&format!("{cut}...")) as f32 > width {
            cut.pop();
        }
        format!("{}...", cut.trim_end())
    }

    /// An interface bitmap at a place in a 640x480 space. Returns where it
    /// went, in that space.
    #[allow(clippy::too_many_arguments)]
    pub fn sprite(
        &mut self,
        lib: &Library,
        renderer: &mut Renderer,
        overlay: &mut Overlay,
        space: Space,
        name: &str,
        (x, y): (f32, f32),
        tint: [u8; 4],
    ) -> Option<[f32; 4]> {
        let (tex, w, h) = self.bitmap(lib, renderer, name)?;
        let rect = [x, y, w as f32, h as f32];
        overlay.image(tex, [0.0, 0.0, w as f32, h as f32], space.rect(rect), tint);
        Some(rect)
    }

    /// One of SIDNEY's boxes: a fill, and a grey (or, `gold`, lit) frame of
    /// its corner and edge bitmaps.
    #[allow(clippy::too_many_arguments)]
    pub fn sid_box(
        &mut self,
        lib: &Library,
        renderer: &mut Renderer,
        overlay: &mut Overlay,
        space: Space,
        r: [f32; 4],
        gold: bool,
        fill: [u8; 4],
    ) {
        if fill[3] > 0 {
            overlay.rect(space.rect(r), fill);
        }
        let s = if gold { "_L" } else { "" };
        let [x, y, w, h] = r;
        let mut piece = |ui: &mut Self, name: String, dest: [f32; 4]| {
            if let Some((tex, tw, th)) = ui.bitmap(lib, renderer, &name) {
                overlay.image(
                    tex,
                    [0.0, 0.0, tw as f32, th as f32],
                    space.rect(dest),
                    [255; 4],
                );
            }
        };
        piece(self, format!("S_BOX_TOP{s}"), [x + 2.0, y, w - 4.0, 1.0]);
        piece(
            self,
            format!("S_BOX_TOP{s}"),
            [x + 2.0, y + h - 1.0, w - 4.0, 1.0],
        );
        piece(self, format!("S_BOX_SIDE{s}"), [x, y + 2.0, 1.0, h - 4.0]);
        piece(
            self,
            format!("S_BOX_SIDE{s}"),
            [x + w - 1.0, y + 2.0, 1.0, h - 4.0],
        );
        piece(self, format!("S_BOX_CORNER_TL{s}"), [x, y, 2.0, 2.0]);
        piece(
            self,
            format!("S_BOX_CORNER_TR{s}"),
            [x + w - 2.0, y, 2.0, 2.0],
        );
        piece(
            self,
            format!("S_BOX_CORNER_BL{s}"),
            [x, y + h - 2.0, 2.0, 2.0],
        );
        piece(
            self,
            format!("S_BOX_CORNER_BR{s}"),
            [x + w - 2.0, y + h - 2.0, 2.0, 2.0],
        );
    }

    /// A SIDNEY text button: black, framed grey, gold when the pointer is on
    /// it. `pointer` is in the 640x480 space. Returns whether it is hovered.
    #[allow(clippy::too_many_arguments)]
    pub fn sid_button(
        &mut self,
        lib: &Library,
        renderer: &mut Renderer,
        overlay: &mut Overlay,
        space: Space,
        r: [f32; 4],
        text: &str,
        (font, enabled, pointer): (&str, bool, (f32, f32)),
    ) -> bool {
        let hover = enabled && inside(r, pointer);
        self.sid_box(lib, renderer, overlay, space, r, hover, [0, 0, 0, 255]);
        let font = if enabled {
            font.to_string()
        } else {
            format!("{font}_UL").replace("_L_UL", "_UL")
        };
        let (_, lh) = self.font_size(lib, renderer, &font, text);
        self.write(
            lib,
            renderer,
            overlay,
            space,
            &font,
            text,
            (r[0], r[1] + (r[3] - lh) / 2.0),
            (Some(r[2]), Align::Center),
        );
        hover
    }

    /// Text in a pale box beside the pointer.
    /// A tooltip as `DEFAULT.TIP` describes it: the text in a white box
    /// with a black border, its foot 40 pixels below the pointer (or, near
    /// the bottom, just above it), kept on screen.
    pub fn tooltip(&self, overlay: &mut Overlay, text: &str, (x, y): (f32, f32), size: (u32, u32)) {
        let scale = Self::scale(size.1);
        let (w, h) = self.measure("F_TOOLTIP", text);
        // Text Border=3,2,2,1: left, top, right, bottom.
        let (bw, bh) = ((w + 3.0 + 2.0) * scale, (h + 2.0 + 1.0) * scale);
        let (sw, sh) = (size.0 as f32, size.1 as f32);
        let mut top = y + 40.0 * scale - bh;
        if y + 40.0 * scale > sh - 10.0 * scale {
            top = y - 10.0 * scale - bh;
        }
        let left = x.clamp(0.0, (sw - bw).max(0.0));
        let top = top.clamp(0.0, (sh - bh).max(0.0));
        overlay.rect([left, top, bw, bh], [0, 0, 0, 255]);
        overlay.rect(
            [left + scale, top + scale, bw - 2.0 * scale, bh - 2.0 * scale],
            [255, 255, 255, 255],
        );
        self.text(
            overlay,
            "F_TOOLTIP",
            text,
            (left + 3.0 * scale, top + 2.0 * scale),
            scale,
            [255; 4],
        );
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

/// A font by name; `NAME:WHITE` or `NAME:BLACK` is that font in one
/// colour, keeping the shape of its letters (for fonts with an alpha map).
fn load_font(lib: &Library, name: &str) -> Option<Font> {
    if let Some((base, colour)) = name.split_once(':') {
        let mut font = load_font(lib, base)?;
        let v = if colour == "BLACK" { 0 } else { 255 };
        // The row atop each line marks where letters part; scaled, it
        // would bleed into their edges.
        let (w, line) = (font.sheet.width as usize, font.glyph_height as usize + 1);
        for (i, px) in font.sheet.rgba.chunks_mut(4).enumerate() {
            px[..3].fill(v);
            if (i / w).is_multiple_of(line) {
                px[3] = 0;
            }
        }
        return Some(font);
    }
    let def = Definition::parse(&lib.read(&format!("{name}.FON")).ok()?);
    let sheet = def.bitmap.clone().unwrap_or_else(|| name.to_string());
    let sheet = sheet.trim_end_matches(".bmp").trim_end_matches(".BMP");
    let image = image::decode_raw(&lib.read(&format!("{sheet}.BMP")).ok()?).ok()?;
    let alpha = def
        .alpha
        .as_ref()
        .and_then(|a| {
            lib.read(&format!(
                "{}.BMP",
                a.trim_end_matches(".BMP").trim_end_matches(".bmp")
            ))
            .ok()
        })
        .and_then(|b| image::decode_raw(&b).ok());
    Some(Font::with_alpha(&def, &image, alpha.as_ref()))
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
