//! The screens around the story: the option bar a right click brings up,
//! the save and restore screen, the quit and overwrite popups, the temple's
//! death screen, the end, and the pause sign. Laid out as G-Engine's
//! `OptionBar`, `SaveLoadScreen`, `QuitPopup`, `ConfirmPopup`,
//! `DeathScreen`, `FinishedScreen` and `PauseScreen`; the option bar's
//! places and pictures come from the game's own `RC_LAYOUT.TXT`.

use std::collections::HashMap;

use gk3_barn::Library;
use gk3_render::{Overlay, OverlayTexture, Renderer};

use crate::saves::{Settings, Summary};
use crate::ui::{Align, Space, Ui, inside};

/// Everything on these screens that takes a click.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    /// The option bar's background: a click there leaves it up.
    Panel,
    /// The option bar's.
    Inventory,
    ActiveItem,
    Radio,
    Options,
    CloseBar,
    Save,
    Restore,
    Advanced,
    QuitGame,
    SoundOptions,
    GraphicsOptions,
    GameOptions,
    /// The graphics options' Advanced button.
    GraphicsAdvanced,
    Slider(Slider),
    Toggle(Toggle),
    /// The save and restore screen's.
    Row(usize),
    SaveHere,
    LoadThis,
    ExitSaveLoad,
    ScrollUp,
    ScrollDown,
    /// The popups'.
    Yes,
    No,
    /// The help screen's.
    HelpPrevious,
    HelpNext,
    HelpExit,
    /// The option bar's way out of the map, a close-up or the kit.
    ExitScreen,
    Help,
    Cinematics,
    Hint,
    /// The camera list, and one of its angles by number.
    Cameras,
    Camera(usize),
    /// The death screen's.
    Retry,
    DeathRestore,
    DeathQuit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slider {
    Master,
    /// The sound options' own copy of the master volume.
    MasterToo,
    Effects,
    Dialogue,
    Music,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Toggle {
    MuteAll,
    MuteEffects,
    MuteDialogue,
    MuteMusic,
    Captions,
    Glide,
    DoorSounds,
    Mipmaps,
    Trilinear,
}

impl Button {
    /// The game's string for this button's tooltip (`ESTRINGS.TXT`), as
    /// G-Engine gives them; some depend on the state shown.
    pub fn tip(self, inventory_open: bool, cinematics: bool) -> Option<&'static str> {
        Some(match self {
            Button::Inventory if inventory_open => "tb_open",
            Button::Inventory => "tb_closed",
            Button::ActiveItem => "tb_currInv",
            Button::Radio => "tb_radio",
            Button::Hint => "tb_hint",
            Button::Cameras => "tb_camera",
            Button::Cinematics if cinematics => "tb_cine",
            Button::Cinematics => "tb_cineOff",
            Button::Help => "tb_help",
            Button::Options => "tb_options",
            Button::CloseBar => "tb_exit",
            Button::ExitScreen => "tb_invExit",
            Button::Save => "tb_optSave",
            Button::Restore => "tb_optRestore",
            Button::Advanced => "tb_optAdvanced",
            Button::QuitGame => "tb_optQuit",
            Button::SoundOptions => "tb_advOptSound",
            Button::GraphicsOptions => "tb_advOptGraphics",
            Button::GameOptions => "tb_advOptGame",
            Button::Toggle(Toggle::MuteAll) => "tb_soundOptGlobal",
            Button::Toggle(Toggle::MuteEffects) => "tb_soundOptSfx",
            Button::Toggle(Toggle::MuteDialogue) => "tb_soundOptDialogue",
            Button::Toggle(Toggle::MuteMusic) => "tb_soundOptMusic",
            Button::SaveHere => "savegame",
            Button::LoadThis => "loadgame",
            Button::ExitSaveLoad => "exitloadsave",
            Button::HelpPrevious => "prevgamehelp",
            Button::HelpNext => "nextgamehelp",
            Button::HelpExit => "exitgamehelp",
            _ => return None,
        })
    }
}

/// The option bar's sections below the options, one at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sub {
    Sound,
    Graphics,
    Game,
}

/// The option bar, while it is up.
#[derive(Debug, Clone)]
pub struct OptionBar {
    /// Where the pointer was when it came up: the bar is centred there.
    pub at: (f32, f32),
    pub options: bool,
    /// The room's camera angles, as pictures to choose from.
    pub cameras: bool,
    pub advanced: bool,
    pub sub: Option<Sub>,
    /// The graphics options' advanced section is open below them.
    pub graphics_advanced: bool,
    /// G-Engine's camera glide; kept for the toggle's sake.
    pub glide: bool,
}

impl OptionBar {
    pub fn new(at: (f32, f32)) -> Self {
        Self {
            at,
            options: false,
            cameras: false,
            advanced: false,
            sub: None,
            graphics_advanced: false,
            glide: true,
        }
    }

    /// A press on one of its buttons that only changes the bar.
    pub fn press(&mut self, button: Button) {
        match button {
            Button::Options => {
                self.options = !self.options;
                self.cameras = false;
                self.advanced = false;
                self.sub = None;
            }
            Button::Cameras => {
                self.cameras = !self.cameras;
                self.options = false;
            }
            Button::Advanced => {
                self.advanced = !self.advanced;
                self.sub = None;
            }
            Button::SoundOptions => self.flip(Sub::Sound),
            Button::GraphicsOptions => self.flip(Sub::Graphics),
            Button::GameOptions => self.flip(Sub::Game),
            Button::GraphicsAdvanced => self.graphics_advanced = !self.graphics_advanced,
            Button::Toggle(Toggle::Glide) => self.glide = !self.glide,
            _ => {}
        }
    }

    fn flip(&mut self, sub: Sub) {
        self.sub = if self.sub == Some(sub) { None } else { Some(sub) };
        self.graphics_advanced = false;
    }
}

/// `RC_LAYOUT.TXT`: `key = value` lines, `;` comments.
#[derive(Debug, Default)]
pub struct Layout(HashMap<String, String>);

impl Layout {
    pub fn load(lib: &Library) -> Self {
        let text = lib
            .read("RC_LAYOUT.TXT")
            .map(|b| gk3_game::state::latin1(&b))
            .unwrap_or_default();
        Self::parse(&text)
    }

    fn parse(text: &str) -> Self {
        Self(
            text.lines()
                .filter(|l| !l.trim_start().starts_with(';'))
                .filter_map(|l| {
                    let (k, v) = l.split_once('=')?;
                    Some((k.trim().to_ascii_lowercase(), v.trim().to_string()))
                })
                .collect(),
        )
    }

    fn get(&self, key: &str) -> &str {
        self.0
            .get(&key.to_ascii_lowercase())
            .map_or("", String::as_str)
    }

    fn pos(&self, key: &str) -> (f32, f32) {
        let mut it = self.get(key).split(',').map(|v| v.trim().parse().unwrap_or(0.0));
        (it.next().unwrap_or(0.0), it.next().unwrap_or(0.0))
    }
}

/// The save and restore screen, while it is up.
#[derive(Debug, Clone)]
pub struct SaveLoad {
    pub saving: bool,
    pub saves: Vec<Summary>,
    pub selected: usize,
    /// The first row shown.
    pub scroll: usize,
    /// The name being typed for a save.
    pub typing: Option<String>,
}

/// Rows that fit the list.
const ROWS: usize = 21;
const ROW_HEIGHT: f32 = 17.0;
/// The list's columns: name, day and time, score.
const COLUMNS: [(f32, f32); 3] = [(0.0, 179.0), (184.0, 147.0), (336.0, 60.0)];
/// Longest name a save takes.
pub const NAME_LENGTH: usize = 32;

/// Where the door sounds' label goes: in line with the panel's own labels.
const DOOR_LABEL_X: f32 = 53.0;
const DOOR_LABEL_DY: f32 = -3.0;
const DOOR_LABEL_FONT: &str = "F_CAPTION_D_16:WHITE";
const DOOR_LABEL_FONT_SHADOW: &str = "F_CAPTION_D_16:BLACK";

impl SaveLoad {
    /// Opens on the newest save, or (saving) the empty slot after them.
    pub fn new(saving: bool) -> Self {
        let saves = crate::saves::list();
        let rows = saves.len() + saving as usize;
        Self {
            saving,
            selected: rows.saturating_sub(1),
            scroll: rows.saturating_sub(ROWS),
            saves,
            typing: None,
        }
    }

    pub fn rows(&self) -> usize {
        self.saves.len() + self.saving as usize
    }

    /// Moves the highlight, keeping it in view.
    pub fn select(&mut self, row: usize) {
        if row >= self.rows() {
            return;
        }
        self.selected = row;
        if row < self.scroll {
            self.scroll = row;
        } else if row >= self.scroll + ROWS {
            self.scroll = row + 1 - ROWS;
        }
    }

    pub fn scroll_by(&mut self, rows: isize) {
        let max = self.rows().saturating_sub(ROWS);
        self.scroll = (self.scroll as isize + rows).clamp(0, max as isize) as usize;
    }

    /// The save highlighted, if it is not the empty slot.
    pub fn chosen(&self) -> Option<&Summary> {
        self.saves.get(self.selected)
    }
}

/// One thing on a help page, placed from its top left.
#[derive(Debug, Clone, PartialEq)]
pub enum HelpItem {
    /// The key for a control, by G-Engine's name for it.
    Key { at: (f32, f32), width: f32, action: String },
    Text { at: (f32, f32), width: f32, text: String },
    Picture { at: (f32, f32), name: String },
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct HelpPage {
    pub size: (f32, f32),
    pub title: String,
    pub next: Option<(f32, f32)>,
    pub previous: Option<(f32, f32)>,
    pub exit: (f32, f32),
    pub items: Vec<HelpItem>,
}

/// The help screen, from `HELPSCREEN.TXT`: pages of keys and what they
/// do, laid out by the file, as G-Engine's `HelpScreen`.
#[derive(Debug, Clone, Default)]
pub struct Help {
    pub pages: Vec<HelpPage>,
    /// Its settings: fonts, the box's pictures and the buttons'.
    settings: HashMap<String, String>,
}

impl Help {
    pub fn load(lib: &Library) -> Self {
        let text = lib
            .read("HELPSCREEN.TXT")
            .map(|b| gk3_game::state::latin1(&b))
            .unwrap_or_default();
        Self::parse(&text)
    }

    fn parse(text: &str) -> Self {
        // Settings first, then `[name]` sections for the pages' items.
        let mut settings = HashMap::new();
        let mut sections: HashMap<String, HashMap<String, String>> = HashMap::new();
        let mut current: Option<String> = None;
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with(';') || line.is_empty() {
                continue;
            }
            if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                current = Some(name.trim().to_ascii_lowercase());
                continue;
            }
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            let (k, v) = (k.trim().to_ascii_lowercase(), v.trim().to_string());
            match &current {
                Some(section) => {
                    sections.entry(section.clone()).or_default().insert(k, v);
                }
                None => {
                    settings.insert(k, v);
                }
            }
        }
        let pair = |v: &str| -> (f32, f32) {
            let mut it = v.split(',').map(|n| n.trim().parse().unwrap_or(0.0));
            (it.next().unwrap_or(0.0), it.next().unwrap_or(0.0))
        };
        let count: usize = settings.get("numpages").and_then(|n| n.parse().ok()).unwrap_or(0);
        let pages = (1..=count)
            .map(|n| {
                let get = |k: &str| settings.get(&format!("{k}{n}"));
                let items = get("pagesections")
                    .map(|list| {
                        list.split(',')
                            .filter_map(|name| {
                                let sec = sections.get(&name.trim().to_ascii_lowercase())?;
                                let at = pair(sec.get("pos")?);
                                let width = sec.get("width").and_then(|w| w.parse().ok()).unwrap_or(0.0);
                                if let Some(action) = sec.get("keycode") {
                                    Some(HelpItem::Key { at, width, action: action.clone() })
                                } else if let Some(text) = sec.get("usetext") {
                                    Some(HelpItem::Text { at, width, text: text.clone() })
                                } else {
                                    sec.get("sprite").map(|name| HelpItem::Picture { at, name: name.clone() })
                                }
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                HelpPage {
                    size: get("size").map_or((600.0, 245.0), |v| pair(v)),
                    title: get("title").cloned().unwrap_or_default(),
                    next: get("nextbtnpos").map(|v| pair(v)),
                    previous: get("prevbtnpos").map(|v| pair(v)),
                    exit: get("exitbtnpos").map_or((0.0, 0.0), |v| pair(v)),
                    items,
                }
            })
            .collect();
        Self { pages, settings }
    }

    fn setting(&self, key: &str) -> String {
        self.settings
            .get(&key.to_ascii_lowercase())
            .cloned()
            .unwrap_or_default()
    }
}

/// The key this engine binds to one of G-Engine's control names.
pub fn key_for(action: &str) -> &'static str {
    let action = action.to_ascii_lowercase();
    let keys: [(&str, &str); 19] = [
        ("help2", "H"),
        ("help", "F1"),
        ("inventory", "I"),
        ("pause2", "Pause"),
        ("pause", "P"),
        ("quicksave", "F5"),
        ("quickload", "F9"),
        ("back", "Escape"),
        ("forwardcamera2", "W"),
        ("forwardcamera", "Up"),
        ("backwardcamera2", "S"),
        ("backwardcamera", "Down"),
        ("leftcamera2", "A"),
        ("leftcamera", "Left"),
        ("rightcamera2", "D"),
        ("rightcamera", "Right"),
        ("levelcameramod", "Control"),
        ("levelcamera", "Space"),
        ("pitchcameramod", "Shift"),
    ];
    if action.ends_with("fastcameramod") {
        return "Alt";
    }
    keys.iter()
        .find(|(name, _)| action.ends_with(name))
        .map_or("", |(_, key)| key)
}

/// A question over everything else.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Popup {
    /// "Quit Gabriel Knight 3?"
    Quit,
    /// Saving over a save.
    Overwrite,
}

/// What the screens show of the game.
pub struct Status<'a> {
    /// A close-up, the fingerprint kit or the map is up: the bar offers a
    /// way out of it.
    pub exitable: bool,
    /// The map is up: the hint button works there.
    pub on_map: bool,
    pub cinematics: bool,
    /// The room's camera angles, pictured, and whether the player may
    /// choose one now.
    pub camera_shots: &'a [OverlayTexture],
    pub cameras_free: bool,
    pub score: u32,
    pub timeblock: &'a str,
    pub active_item: Option<&'a str>,
    pub inventory_open: bool,
    pub settings: &'a Settings,
    /// There are saves to restore.
    pub any_saves: bool,
}

/// Draws the screens and gathers where their buttons are.
pub struct Painter<'a> {
    pub ui: &'a mut Ui,
    pub lib: &'a Library,
    pub renderer: &'a mut Renderer,
    pub overlay: &'a mut Overlay,
    pub pointer: (f32, f32),
    pub held: Option<Button>,
    pub size: (u32, u32),
    /// Buttons as drawn, on the window, for clicks.
    pub hits: Vec<(Button, [f32; 4])>,
}

impl Painter<'_> {
    fn sprite(&mut self, space: Space, name: &str, at: (f32, f32)) -> Option<[f32; 4]> {
        self.ui
            .sprite(self.lib, self.renderer, self.overlay, space, name, at, [255; 4])
    }

    /// A button in one of four pictures: up, hovered, down, unavailable.
    /// An unavailable one takes no clicks.
    fn button(
        &mut self,
        space: Space,
        id: Button,
        [up, hover, down, off]: [&str; 4],
        at: (f32, f32),
        enabled: bool,
    ) {
        let Some((w, h)) = self.ui.bitmap_size(self.lib, self.renderer, up) else {
            return;
        };
        let rect = space.rect([at.0, at.1, w, h]);
        let over = inside(rect, self.pointer);
        let name = if !enabled {
            if off.is_empty() { up } else { off }
        } else if over && self.held == Some(id) {
            down
        } else if over && !hover.is_empty() {
            hover
        } else {
            up
        };
        self.sprite(space, name, at);
        if enabled {
            self.hits.push((id, rect));
        }
    }

    /// A button from the layout file's `<key>Sprite…` and `<key>Pos`.
    fn laid_button(&mut self, layout: &Layout, space: Space, id: Button, key: &str, enabled: bool) {
        let sprite = |state: &str| layout.get(&format!("{key}Sprite{state}")).to_string();
        let (up, hover, down, off) = (sprite("Up"), sprite("Hov"), sprite("Down"), sprite("Dis"));
        self.button(
            space,
            id,
            [&up, &hover, &down, &off],
            layout.pos(&format!("{key}Pos")),
            enabled,
        );
    }

    /// A check box from the layout file: down when on.
    fn laid_toggle(&mut self, layout: &Layout, space: Space, id: Toggle, key: &str, on: bool) {
        let at = layout.pos(&format!("{key}Pos"));
        let name = layout.get(&format!("{key}Sprite{}", if on { "Down" } else { "Up" })).to_string();
        if let Some(r) = self.sprite(space, &name, at) {
            self.hits.push((Button::Toggle(id), space.rect(r)));
        }
    }

    /// The door sounds' switch, a row below Captions.
    fn door_toggle(&mut self, layout: &Layout, space: Space, on: bool) {
        let (x, y) = layout.pos("gameOptCaptionsPos");
        let (_, y0) = layout.pos("gameOptGlidePos");
        let at = (x, y + (y - y0));
        let key = if on { "gameOptCaptionsSpriteDown" } else { "gameOptCaptionsSpriteUp" };
        let name = layout.get(key).to_string();
        if let Some(r) = self.sprite(space, &name, at) {
            self.hits.push((Button::Toggle(Toggle::DoorSounds), space.rect(r)));
        }
        // White with a shadow, as the panel's own labels.
        for (font, d) in [(DOOR_LABEL_FONT_SHADOW, 1.0), (DOOR_LABEL_FONT, 0.0)] {
            self.ui.write(
                self.lib,
                self.renderer,
                self.overlay,
                space,
                font,
                "Door Sounds",
                (DOOR_LABEL_X + d, at.1 + DOOR_LABEL_DY + d),
                (None, Align::Left),
            );
        }
    }

    /// A volume slider from the layout file: its handle slides between the
    /// `<key>Min` and `<key>Max` places.
    fn laid_slider(&mut self, layout: &Layout, space: Space, id: Option<Slider>, key: &str, value: f32) {
        let handle = layout.get("optSliderSprite").to_string();
        let handle = if handle.is_empty() { "RC_SO_SLIDER".to_string() } else { handle };
        let Some((hw, hh)) = self.ui.bitmap_size(self.lib, self.renderer, &handle) else {
            return;
        };
        let (min, max) = (layout.pos(&format!("{key}Min")), layout.pos(&format!("{key}Max")));
        let x = min.0 + (max.0 - min.0) * value.clamp(0.0, 1.0);
        self.sprite(space, &handle, (x.round(), min.1));
        if let Some(id) = id {
            let track = [min.0, min.1 - 2.0, max.0 - min.0 + hw, hh + 4.0];
            self.hits.push((Button::Slider(id), space.rect(track)));
        }
    }

    fn label(&mut self, space: Space, font: &str, text: &str, at: (f32, f32), width: f32) {
        self.ui.write(
            self.lib,
            self.renderer,
            self.overlay,
            space,
            font,
            text,
            at,
            (Some(width), Align::Center),
        );
    }

    /// Dims everything drawn so far, as G-Engine's popups do.
    fn dim(&mut self) {
        let (w, h) = (self.size.0 as f32, self.size.1 as f32);
        self.overlay.rect([0.0, 0.0, w, h], [0, 0, 0, 128]);
    }

    fn black(&mut self) {
        let (w, h) = (self.size.0 as f32, self.size.1 as f32);
        self.overlay.rect([0.0, 0.0, w, h], [0, 0, 0, 255]);
    }

    /// A picture centred on the window at the interface's scale.
    fn centred(&mut self, name: &str) -> Option<Space> {
        let (w, h) = self.ui.bitmap_size(self.lib, self.renderer, name)?;
        let fit = ui_scale(self.size);
        let space = Space {
            ox: ((self.size.0 as f32 - w * fit) / 2.0).round(),
            oy: ((self.size.1 as f32 - h * fit) / 2.0).round(),
            fit,
        };
        self.sprite(space, name, (0.0, 0.0));
        Some(space)
    }

    /// The option bar: the main panel, and below it whichever sections are
    /// open, kept on the window.
    pub fn option_bar(&mut self, layout: &Layout, bar: &OptionBar, status: &Status) {
        let fit = ui_scale(self.size);
        let panel = layout.get("backSprite").to_string();
        let Some((pw, ph)) = self.ui.bitmap_size(self.lib, self.renderer, &panel) else {
            return;
        };
        // The sections hang below the panel, each under the last.
        let mut sections: Vec<String> = Vec::new();
        // The camera list: three to a row, as many rows as it takes (the
        // picture is for two; more stretch it).
        let shots = status.camera_shots.len();
        let rows = shots.div_ceil(3).max(1);
        let camera_height = if bar.cameras {
            let (_, h) = self
                .ui
                .bitmap_size(self.lib, self.renderer, layout.get("camBackSprite"))
                .unwrap_or((0.0, 0.0));
            h + rows.saturating_sub(2) as f32 * layout.pos("camOffset").1
        } else {
            0.0
        };
        if bar.options {
            sections.push(layout.get("optBackSprite").into());
            if bar.advanced {
                sections.push(layout.get("advOptBackSprite").into());
                if let Some(sub) = bar.sub {
                    sections.push(
                        layout
                            .get(match sub {
                                Sub::Sound => "soundOptBackSprite",
                                Sub::Graphics => "graphicsOptBackSprite",
                                Sub::Game => "gameOptBackSprite",
                            })
                            .into(),
                    );
                    if sub == Sub::Graphics && bar.graphics_advanced {
                        sections.push(layout.get("advGraphOptBackSprite").into());
                    }
                }
            }
        }
        let heights: Vec<f32> = sections
            .iter()
            .map(|s| self.ui.bitmap_size(self.lib, self.renderer, s).map_or(0.0, |(_, h)| h))
            .collect();
        let total = ph + heights.iter().sum::<f32>() + camera_height;
        let (sw, sh) = (self.size.0 as f32, self.size.1 as f32);
        let x = (bar.at.0 - pw * fit / 2.0).round().clamp(0.0, (sw - pw * fit).max(0.0));
        let y = (bar.at.1 - ph * fit / 2.0).round().clamp(0.0, (sh - total * fit).max(0.0));
        let space = Space { ox: x, oy: y, fit };

        // Main panel.
        self.hits.push((Button::Panel, space.rect([0.0, 0.0, pw, total])));
        self.sprite(space, &panel, (0.0, 0.0));
        let font = layout.get("statusFont").to_ascii_uppercase();
        let (sx, sy) = layout.pos("scorePos");
        let score = format!("{:03}/{:03}", status.score, gk3_game::scores::MAX_SCORE);
        self.label(space, &font, &score, (sx, sy), layout.pos("scoreSize").0);
        let when = self.ui.string(&format!("Day{}", status.timeblock.to_ascii_lowercase()));
        let (day, time) = when.split_once(',').unwrap_or((&when, ""));
        let (day, time) = (day.trim().to_string(), time.trim().to_string());
        self.label(space, &font, &day, layout.pos("dayPos"), layout.pos("daySize").0);
        self.label(space, &font, &time, layout.pos("timePos"), layout.pos("timeSize").0);
        if let Some(item) = status.active_item
            && let Some((tex, w, h)) = self.ui.item_picture(self.lib, self.renderer, item)
        {
            let at = layout.pos("currInvPos");
            let (w, h) = (w as f32, h as f32);
            let rect = space.rect([at.0, at.1, w, h]);
            self.overlay.image(tex, [0.0, 0.0, w, h], rect, [255; 4]);
            self.hits.push((Button::ActiveItem, rect));
        }
        // One of three in the same place, as G-Engine shows them.
        if status.exitable {
            self.laid_button(layout, space, Button::ExitScreen, "invExit", true);
        } else {
            let inventory = if status.inventory_open { "open" } else { "closed" };
            self.laid_button(layout, space, Button::Inventory, inventory, !status.on_map);
        }
        // Day 3, 9pm has the radio where the hints would be.
        if status.timeblock.eq_ignore_ascii_case("309P") {
            self.laid_button(layout, space, Button::Radio, "radio", true);
        } else {
            self.laid_button(layout, space, Button::Hint, "hint", status.on_map);
        }
        let cameras = status.cameras_free && shots > 0;
        self.laid_button(layout, space, Button::Cameras, "camera", cameras);
        let cine = if status.cinematics { "cine" } else { "cineoff" };
        self.laid_button(layout, space, Button::Cinematics, cine, true);
        self.laid_button(layout, space, Button::Help, "help", !status.on_map);
        self.laid_button(layout, space, Button::Options, "options", true);
        self.laid_button(layout, space, Button::CloseBar, "exit", true);

        if bar.cameras {
            let s = Space {
                oy: space.oy + ph * space.fit,
                ..space
            };
            let back = layout.get("camBackSprite").to_string();
            if let Some((tex, w, h)) = self.ui.bitmap(self.lib, self.renderer, &back) {
                let dest = s.rect([0.0, 0.0, w as f32, camera_height]);
                self.overlay
                    .image(tex, [0.0, 0.0, w as f32, h as f32], dest, [255; 4]);
            }
            let (first, step, size) = (
                layout.pos("camFirstPos"),
                layout.pos("camOffset"),
                layout.pos("camSize"),
            );
            for (i, &tex) in status.camera_shots.iter().enumerate() {
                let (col, row) = ((i % 3) as f32, (i / 3) as f32);
                let r = s.rect([first.0 + col * step.0, first.1 + row * step.1, size.0, size.1]);
                self.overlay.image(tex, [0.0, 0.0, size.0, size.1], r, [255; 4]);
                if inside(r, self.pointer) {
                    // Framed in the bar's gold while the pointer is on it.
                    let t = s.fit.max(1.0);
                    for edge in [
                        [r[0], r[1], r[2], t],
                        [r[0], r[1] + r[3] - t, r[2], t],
                        [r[0], r[1], t, r[3]],
                        [r[0] + r[2] - t, r[1], t, r[3]],
                    ] {
                        self.overlay.rect(edge, [230, 190, 60, 255]);
                    }
                }
                self.hits.push((Button::Camera(i), r));
            }
            return;
        }
        let mut top = ph;
        let mut next = |space: Space, i: usize| {
            let s = Space {
                oy: space.oy + top * space.fit,
                ..space
            };
            top += heights[i];
            s
        };
        if !bar.options {
            return;
        }
        let s = next(space, 0);
        self.sprite(s, &sections[0], (0.0, 0.0));
        let settings = status.settings;
        self.laid_slider(layout, s, Some(Slider::Master), "optSlider", settings.master);
        self.laid_button(layout, s, Button::Save, "optSave", true);
        self.laid_button(layout, s, Button::Restore, "optRestore", status.any_saves);
        self.laid_button(layout, s, Button::Advanced, "optAdvanced", true);
        self.laid_button(layout, s, Button::QuitGame, "optQuit", true);
        if !bar.advanced {
            return;
        }
        let s = next(space, 1);
        self.sprite(s, &sections[1], (0.0, 0.0));
        self.laid_button(layout, s, Button::SoundOptions, "advOptSound", true);
        self.laid_button(layout, s, Button::GraphicsOptions, "advOptGraphics", true);
        self.laid_button(layout, s, Button::GameOptions, "advOptGame", true);
        let Some(sub) = bar.sub else {
            return;
        };
        let s = next(space, 2);
        self.sprite(s, &sections[2], (0.0, 0.0));
        match sub {
            Sub::Sound => {
                let sliders = [
                    (Slider::MasterToo, "optSoundGlobalSlider", settings.master),
                    (Slider::Effects, "optSoundSfxSlider", settings.effects),
                    (Slider::Dialogue, "optSoundDialogueSlider", settings.dialogue),
                    (Slider::Music, "optSoundMusicSlider", settings.music),
                ];
                for (id, key, value) in sliders {
                    self.laid_slider(layout, s, Some(id), key, value);
                }
                // How many sounds play at once: the mixer has no limit.
                self.laid_slider(layout, s, None, "optSoundNumSlider", 1.0);
                let toggles = [
                    (Toggle::MuteAll, "soundOptGlobal", settings.mute_all),
                    (Toggle::MuteEffects, "soundOptSfx", settings.mute_effects),
                    (Toggle::MuteDialogue, "soundOptDialogue", settings.mute_dialogue),
                    (Toggle::MuteMusic, "soundOptMusic", settings.mute_music),
                ];
                for (id, key, on) in toggles {
                    self.laid_toggle(layout, s, id, key, on);
                }
            }
            Sub::Graphics => self.graphics_options(layout, s),
            Sub::Game => {
                self.laid_toggle(layout, s, Toggle::Glide, "gameOptGlide", bar.glide);
                self.laid_toggle(layout, s, Toggle::Captions, "gameOptCaptions", settings.captions);
                // Ours, not the original's: on the row below, in its
                // style, with its label written in.
                self.door_toggle(layout, s, settings.door_sounds);
            }
        }
        if sub == Sub::Graphics && bar.graphics_advanced {
            let s = next(space, 3);
            self.sprite(s, &sections[3], (0.0, 0.0));
            self.advanced_graphics_options(layout, s, settings);
        }
    }

    /// The graphics options, as G-Engine has them: the GPU drawing the
    /// game (the only driver there is), the window's size, incremental
    /// rendering (which no longer means anything) and gamma (which G-Engine
    /// leaves out too) shown unavailable, and the Advanced button.
    fn graphics_options(&mut self, layout: &Layout, s: Space) {
        self.sprite(s, layout.get("graphOptGammaTextDisabled"), layout.pos("graphOptGammaTextPos"));
        // The GPU by its name, without the driver's details in brackets.
        let name = self.renderer.adapter_name.clone();
        let gpu = name.split(" (").next().unwrap_or(&name).trim().to_string();
        let size = format!("{} x {}", self.size.0, self.size.1);
        self.choice_box(layout, s, "graphOptDriver", &gpu);
        self.choice_box(layout, s, "graphOptResolution", &size);
        self.sprite(s, layout.get("graphOptIncrementalSpriteDis"), layout.pos("graphOptIncrementalPos"));
        self.sprite(s, layout.get("graphOptIncrementalDisabled"), layout.pos("graphOptIncrementalTextPos"));
        self.laid_button(layout, s, Button::GraphicsAdvanced, "graphOptAdvanced", true);
    }

    /// A drop-down box from the layout file, showing its choice; its arrow
    /// would list the others.
    fn choice_box(&mut self, layout: &Layout, s: Space, key: &str, text: &str) {
        let arrow = layout.pos(&format!("{key}Pos"));
        let (box_x, _) = layout.pos(&format!("{key}BoxPos"));
        self.sprite(s, layout.get(&format!("{key}SpriteUp")), arrow);
        let font = layout.get("statusFont").to_ascii_uppercase();
        let text = self.ui.fit_line(self.lib, self.renderer, &font, text, arrow.0 - box_x - 5.0);
        self.ui.write(
            self.lib,
            self.renderer,
            self.overlay,
            s,
            &font,
            &text,
            (box_x + 3.0, arrow.1 + 1.0),
            (None, Align::Left),
        );
    }

    /// The advanced graphics options: mipmaps and trilinear filtering work
    /// (trilinear only with mipmaps); texture quality, lighting and detail
    /// are fixed, as in G-Engine, and shown so.
    fn advanced_graphics_options(&mut self, layout: &Layout, s: Space, settings: &Settings) {
        // Textures at full size and full lighting: the original's best,
        // by its own word for it.
        for key in ["advGraphOptQual", "advGraphOptLighting"] {
            self.choice_box(layout, s, key, "High");
        }
        self.laid_toggle(layout, s, Toggle::Mipmaps, "advGraphOptMipMap", settings.mipmaps);
        self.sprite(s, layout.get("advGraphOptMipMapEnabled"), layout.pos("advGraphOptMipMapTextPos"));
        if settings.mipmaps {
            self.laid_toggle(layout, s, Toggle::Trilinear, "advGraphOptFiltering", settings.trilinear);
            self.sprite(s, layout.get("advGraphOptFilteringEnabled"), layout.pos("advGraphOptFilteringTextPos"));
        } else {
            self.sprite(s, layout.get("advGraphOptFilteringSpriteDis"), layout.pos("advGraphOptFilteringPos"));
            self.sprite(s, layout.get("advGraphOptFilteringDisabled"), layout.pos("advGraphOptFilteringTextPos"));
        }
        self.sprite(s, layout.get("advGraphOptLODDisabled"), layout.pos("advGraphOptLODTextPos"));
    }

    /// The save and restore screen: `LOADSAVE.BMP`, the saves listed with
    /// their day and score, and an empty slot to save into.
    pub fn save_load(&mut self, screen: &SaveLoad, now: f64) {
        self.black();
        let Some(space) = self.centred("LOADSAVE") else {
            return;
        };
        let font = "F_SSERIF_T8";
        let (list_x, list_y) = (34.0, 72.0);
        // The picture of the moment saved.
        let thumb_at = space.rect([466.0, 120.0, 160.0, 120.0]);
        self.overlay.rect(thumb_at, [0, 0, 0, 255]);
        if let Some(save) = screen.saves.get(screen.selected) {
            let key = format!("{}:{:?}", save.slot, save.modified);
            let slot = save.slot.clone();
            if let Some((tex, w, h)) = self
                .ui
                .thumbnail(self.renderer, &key, || crate::saves::read_thumb(&slot))
            {
                self.overlay
                    .image(tex, [0.0, 0.0, w as f32, h as f32], thumb_at, [255; 4]);
            }
        }
        let end = (screen.scroll + ROWS).min(screen.rows());
        for (line, row) in (screen.scroll..end).enumerate() {
            let y = list_y + line as f32 * ROW_HEIGHT;
            if row == screen.selected {
                for (x, w) in COLUMNS {
                    self.overlay.rect(
                        space.rect([list_x + x, y, w, ROW_HEIGHT]),
                        [128, 128, 128, 255],
                    );
                }
            }
            let rect = space.rect([list_x, y, 396.0, ROW_HEIGHT]);
            if screen.typing.is_none() {
                self.hits.push((Button::Row(row), rect));
            }
            let (name, when, score) = match screen.saves.get(row) {
                Some(save) => (
                    save.name.clone(),
                    self.ui.string(&format!("Day{}", save.timeblock.to_ascii_lowercase())),
                    format!(
                        "{:03} / {:03}",
                        gk3_game::scores::total(&save.score_events),
                        gk3_game::scores::MAX_SCORE
                    ),
                ),
                None => ("[Empty]".to_string(), String::new(), String::new()),
            };
            let name = match &screen.typing {
                // The name being typed, with a blinking caret.
                Some(text) if row == screen.selected => {
                    let caret = if now.fract() < 0.5 { "|" } else { "" };
                    format!("{text}{caret}")
                }
                _ => name,
            };
            for ((x, w), text) in COLUMNS.iter().zip([name, when, score]) {
                self.ui.write(
                    self.lib,
                    self.renderer,
                    self.overlay,
                    space,
                    font,
                    &text,
                    (list_x + x + 2.0, y + 2.0),
                    (Some(*w - 2.0), Align::Left),
                );
            }
        }
        if screen.rows() > ROWS {
            self.button(
                space,
                Button::ScrollUp,
                ["SAVELOAD_SCROLLUP_STD", "SAVELOAD_SCROLLUP_HOV", "SAVELOAD_SCROLLUP_DWN", "SAVELOAD_SCROLLUP_DIS"],
                (430.0, 67.0),
                screen.scroll > 0,
            );
            self.button(
                space,
                Button::ScrollDown,
                ["SAVELOAD_SCROLLDN_STD", "SAVELOAD_SCROLLDN_HOV", "SAVELOAD_SCROLLDN_DWN", "SAVELOAD_SCROLLDN_DIS"],
                (430.0, 452.0 - 22.0),
                end < screen.rows(),
            );
        }
        if screen.saving {
            self.button(
                space,
                Button::SaveHere,
                ["SAVEN", "SAVEHOV", "SAVED", "SAVEDIS"],
                (473.0, 31.0),
                true,
            );
        } else {
            self.button(
                space,
                Button::LoadThis,
                ["RESTOREN", "RESTOREHOV", "RESTORED", "RESTOREDIS"],
                (473.0, 31.0),
                screen.chosen().is_some(),
            );
        }
        self.button(
            space,
            Button::ExitSaveLoad,
            ["EXITN", "EXITHOV", "EXITD", "EXITDIS"],
            (5.0, 435.0),
            true,
        );
    }

    /// A popup over a dimmed screen.
    pub fn popup(&mut self, popup: Popup) {
        self.dim();
        match popup {
            Popup::Quit => {
                let Some(space) = self.centred("QUITGAME") else {
                    return;
                };
                // Bottoms 59 pixels above the picture's foot.
                self.button(space, Button::Yes, ["QG_YES_U", "QG_YES_H", "QG_YES_D", ""], (261.0, 133.0), true);
                self.button(space, Button::No, ["QG_NO_U", "QG_NO_H", "QG_NO_D", ""], (418.0, 133.0), true);
            }
            Popup::Overwrite => {
                let text = self.ui.string("OverwriteSave");
                self.message_box(&text);
            }
        }
    }

    /// G-Engine's `ConfirmPopup`, from `MSGBOX.TXT`: a black box 200 wide
    /// framed in its corner and edge pictures, the message in Tangerine,
    /// Yes and No along the foot.
    fn message_box(&mut self, text: &str) {
        let font = "F_TANGERINE";
        let fit = ui_scale(self.size);
        let lines = self.ui.wrap(font, self.lib, self.renderer, text, 170.0);
        let (_, lh) = self.ui.font_size(self.lib, self.renderer, font, "A");
        let (w, h) = (200.0, 30.0 + lines as f32 * lh + 50.0);
        let space = Space {
            ox: ((self.size.0 as f32 - w * fit) / 2.0).round(),
            oy: ((self.size.1 as f32 - h * fit) / 2.0).round(),
            fit,
        };
        self.overlay.rect(space.rect([0.0, 0.0, w, h]), [0, 0, 0, 255]);
        let edge = |p: &mut Self, name: &str, r: [f32; 4]| {
            if let Some((tex, tw, th)) = p.ui.bitmap(p.lib, p.renderer, name) {
                p.overlay
                    .image(tex, [0.0, 0.0, tw as f32, th as f32], space.rect(r), [255; 4]);
            }
        };
        edge(self, "MSG_BOX_HORIZ", [5.0, 0.0, w - 10.0, 5.0]);
        edge(self, "MSG_BOX_HORIZ", [5.0, h - 5.0, w - 10.0, 5.0]);
        edge(self, "MSG_BOX_VERT", [0.0, 5.0, 5.0, h - 10.0]);
        edge(self, "MSG_BOX_VERT", [w - 5.0, 5.0, 5.0, h - 10.0]);
        edge(self, "MSG_BOX_CORNER_UL", [0.0, 0.0, 5.0, 5.0]);
        edge(self, "MSG_BOX_CORNER_UR", [w - 5.0, 0.0, 5.0, 5.0]);
        edge(self, "MSG_BOX_CORNER_LL", [0.0, h - 5.0, 5.0, 5.0]);
        edge(self, "MSG_BOX_CORNER_LR", [w - 5.0, h - 5.0, 5.0, 5.0]);
        self.ui.write(
            self.lib,
            self.renderer,
            self.overlay,
            space,
            font,
            text,
            (15.0, 15.0),
            (Some(170.0), Align::Left),
        );
        let foot = h - 15.0 - 26.0;
        self.button(space, Button::Yes, ["MSG_YES_U", "MSG_YES_H", "MSG_YES_D", ""], (43.0, foot), true);
        self.button(space, Button::No, ["MSG_NO_U", "MSG_NO_H", "MSG_NO_D", ""], (109.0, foot), true);
    }

    /// A help page in its framed box over the dimmed game.
    pub fn help(&mut self, help: &Help, page: usize) {
        self.dim();
        let Some(p) = help.pages.get(page) else {
            return;
        };
        let fit = ui_scale(self.size);
        let (w, h) = p.size;
        let space = Space {
            ox: ((self.size.0 as f32 - w * fit) / 2.0).round(),
            oy: ((self.size.1 as f32 - h * fit) / 2.0).round(),
            fit,
        };
        self.overlay.rect(space.rect([0.0, 0.0, w, h]), [0, 0, 0, 255]);
        let size_of = |p: &mut Self, name: &str| p.ui.bitmap_size(p.lib, p.renderer, name).unwrap_or((0.0, 0.0));
        let (top, side) = (help.setting("TopSprite"), help.setting("SideSprite"));
        let (tl, tr, bl, br) = (
            help.setting("TLCornerSprite"),
            help.setting("TRCornerSprite"),
            help.setting("BLCornerSprite"),
            help.setting("BRCornerSprite"),
        );
        let (cw, ch) = size_of(self, &tl);
        let (_, th) = size_of(self, &top);
        let (sw, _) = size_of(self, &side);
        let piece = |p: &mut Self, name: &str, r: [f32; 4]| {
            if let Some((tex, tw, tht)) = p.ui.bitmap(p.lib, p.renderer, name) {
                p.overlay
                    .image(tex, [0.0, 0.0, tw as f32, tht as f32], space.rect(r), [255; 4]);
            }
        };
        piece(self, &top, [cw, 0.0, w - 2.0 * cw, th]);
        piece(self, &top, [cw, h - th, w - 2.0 * cw, th]);
        piece(self, &side, [0.0, ch, sw, h - 2.0 * ch]);
        piece(self, &side, [w - sw, ch, sw, h - 2.0 * ch]);
        piece(self, &tl, [0.0, 0.0, cw, ch]);
        piece(self, &tr, [w - cw, 0.0, cw, ch]);
        piece(self, &bl, [0.0, h - ch, cw, ch]);
        piece(self, &br, [w - cw, h - ch, cw, ch]);
        let font = |key: &str| help.setting(key).to_ascii_uppercase();
        let title = font("TitleFont");
        self.label(space, &title, &p.title, (0.0, 10.0), w);
        for item in &p.items {
            match item {
                HelpItem::Key { at, width, action } => {
                    let f = font("KeyFont");
                    self.ui.write(self.lib, self.renderer, self.overlay, space, &f, key_for(action), *at, (Some(*width), Align::Left));
                }
                HelpItem::Text { at, width, text } => {
                    let f = font("UseFont");
                    self.ui.write(self.lib, self.renderer, self.overlay, space, &f, text, *at, (Some(*width), Align::Left));
                }
                HelpItem::Picture { at, name } => {
                    self.sprite(space, name, *at);
                }
            }
        }
        let names = |stem: &str| ["Up", "Hov", "Down"].map(|s| help.setting(&format!("{stem}Btn{s}")));
        for (id, stem, at) in [
            (Button::HelpPrevious, "Prev", p.previous),
            (Button::HelpNext, "Next", p.next),
            (Button::HelpExit, "Exit", Some(p.exit)),
        ] {
            if let Some(at) = at {
                let [up, hover, down] = names(stem);
                self.button(space, id, [&up, &hover, &down, ""], at, true);
            }
        }
    }

    /// The temple's death screen: Retry, Restore and Quit along the foot.
    pub fn death(&mut self, any_saves: bool) {
        self.black();
        let Some(space) = self.fitted("DEATHSCREEN") else {
            return;
        };
        let stems = |s: &str| ["N", "H", "D", "X"].map(|k| format!("DS_{s}_{k}"));
        for (id, stem, centre, enabled) in [
            (Button::Retry, "RTRY", 320.0 - 98.0, true),
            (Button::DeathRestore, "REST", 320.0, any_saves),
            (Button::DeathQuit, "QUIT", 320.0 + 98.0, true),
        ] {
            let names = stems(stem);
            let names = [&*names[0], &*names[1], &*names[2], &*names[3]];
            self.button(space, id, names, (centre - 40.5, 480.0 - 24.0 - 26.0), enabled);
        }
    }

    /// The end: `FINISHED.BMP` until the player presses something.
    pub fn finished(&mut self) {
        self.black();
        self.fitted("FINISHED");
    }

    /// The pause sign over the dimmed game.
    pub fn paused(&mut self) {
        self.dim();
        self.centred("PAUSED");
    }

    /// A 640x480 picture as large as fits the window.
    fn fitted(&mut self, name: &str) -> Option<Space> {
        let space = Space::of(self.size);
        self.sprite(space, name, (0.0, 0.0))?;
        Some(space)
    }
}

/// The texture slots camera pictures are drawn into, reused each time.
const CAMERA_SLOTS: usize = usize::MAX - 64;

/// The room's own camera angles, by their place in its camera list.
pub fn room_cameras(world: &crate::world::World) -> Vec<usize> {
    world.scene.as_ref().map_or(Vec::new(), |s| {
        s.room
            .cameras
            .iter()
            .enumerate()
            .filter(|(_, c)| c.room)
            .map(|(i, _)| i)
            .collect()
    })
}

/// A picture of the room from each of its angles, as it stands now, for
/// the option bar's camera list: drawn at four times the size and shrunk.
pub fn camera_shots(
    world: &crate::world::World,
    renderer: &mut Renderer,
    ui: &mut Ui,
) -> Vec<OverlayTexture> {
    let (Some(scene), Some(seen)) = (&world.scene, world.view_scene()) else {
        return Vec::new();
    };
    let models = scene.cast.models();
    let mut shots = Vec::new();
    for (n, i) in room_cameras(world).into_iter().enumerate() {
        let camera = crate::shoot::from_room_camera(&scene.room.cameras[i]);
        let Ok(image) = renderer.render_image(
            seen,
            &models,
            &Overlay::default(),
            &camera,
            gk3_render::ShadeMode::Lit,
            (320, 240),
        ) else {
            break;
        };
        let small = crate::saves::shrink(&image, 4);
        shots.push(ui.video_texture(renderer, CAMERA_SLOTS + n, &small, true));
    }
    shots
}

/// The interface's scale: whole pixels at 480 lines, larger above.
pub fn ui_scale(size: (u32, u32)) -> f32 {
    (size.1 as f32 / 480.0).max(1.0)
}

/// Where a slider's handle should go for the pointer: 0 to 1 along it.
pub fn slider_value(track: [f32; 4], x: f32, fit: f32) -> f32 {
    let handle = 4.0 * fit;
    ((x - track[0] - handle / 2.0) / (track[2] - handle).max(1.0)).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_reads_places_and_names() {
        let l = Layout::parse("; comment\nbackSprite\t\t= rc_panel\nscorePos = 7,20\noptSliderMin = 175, 13\n");
        assert_eq!(l.get("BACKSPRITE"), "rc_panel");
        assert_eq!(l.pos("scorePos"), (7.0, 20.0));
        assert_eq!(l.pos("optSliderMin"), (175.0, 13.0));
        assert_eq!(l.pos("missing"), (0.0, 0.0));
    }

    #[test]
    fn the_save_list_keeps_the_highlight_in_view() {
        let mut s = SaveLoad {
            saving: true,
            saves: Vec::new(),
            selected: 0,
            scroll: 0,
            typing: None,
        };
        assert_eq!(s.rows(), 1);
        s.select(5);
        assert_eq!(s.selected, 0, "no row 5 to select");
        s.scroll_by(3);
        assert_eq!(s.scroll, 0);
    }

    #[test]
    fn help_pages_come_from_the_file() {
        let help = Help::parse(
            "NumPages = 1\nTitleFont = f_arial_t12\nSize1 = 600, 245\nTitle1 = Game Controls\n\
             ExitBtnPos1 = 270, 210\nPageSections1 = HelpKey, HelpText\n\
             [HelpKey]\nPos = 5, 30\nWidth = 90\nKeyCode = UIKeyType_Help\n\
             [HelpText]\nPos = 100, 40\nWidth = 195\nUseText = Show this screen.\n",
        );
        let page = &help.pages[0];
        assert_eq!((page.title.as_str(), page.exit, page.next), ("Game Controls", (270.0, 210.0), None));
        assert_eq!(page.items.len(), 2);
        assert_eq!(help.setting("titlefont"), "f_arial_t12");
        assert_eq!(key_for("UIKeyType_Help"), "F1");
        assert_eq!(key_for("UIKeyType_Help2"), "H");
        assert_eq!(key_for("UIKeyType_ForwardCamera2"), "W");
        assert_eq!(key_for("UIKeyType_LevelCameraMod"), "Control");
        assert_eq!(key_for("UIKeyType_FastCameraMod"), "Alt");
    }

    #[test]
    fn sliders_run_end_to_end() {
        let track = [100.0, 0.0, 72.0, 8.0];
        assert_eq!(slider_value(track, 0.0, 1.0), 0.0);
        assert_eq!(slider_value(track, 1000.0, 1.0), 1.0);
        assert!((slider_value(track, 136.0, 1.0) - 0.5).abs() < 0.01);
    }
}
