//! The game in a window.

use std::process::ExitCode;
use std::sync::Arc;
use std::time::Instant;

use gk3_barn::Library;
use gk3_render::{Overlay, Renderer, ShadeMode, Target};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

use crate::camera::{self, Controls};
use crate::play::Options;
use crate::saves::Settings;
use crate::screens::{Button, Help, Layout, OptionBar, Painter, Popup, SaveLoad, Slider, Status, Toggle};
use crate::ui::{
    BinocsButton, Inventory, KitLayout, Menu, Space, TitleButton, TitleCard, Ui, inside,
};
use crate::world::{AfterMovie, Pick, ScreenRequest, World};
use gk3_game::actions::Rule;

pub fn run(lib: Library, options: Options) -> Result<ExitCode, String> {
    // The world borrows the library for the whole run.
    let lib: &'static Library = Box::leak(Box::new(lib));
    let event_loop = EventLoop::new().map_err(|e| format!("cannot open a window: {e}"))?;
    let mut app = Game {
        lib,
        options,
        gfx: None,
        error: None,
    };
    event_loop
        .run_app(&mut app)
        .map_err(|e| format!("window error: {e}"))?;
    match app.error {
        Some(e) => Err(e),
        None => Ok(ExitCode::SUCCESS),
    }
}

struct Game {
    lib: &'static Library,
    options: Options,
    gfx: Option<Gfx>,
    error: Option<String>,
}

/// Before the game: the Sierra logo, the intro and the title screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Front {
    Logo,
    Intro,
    Title,
}

/// A cursor's art and the system cursors made from its frames.
struct Cursors {
    art: crate::cursor::CursorArt,
    frames: Vec<winit::window::CustomCursor>,
}

struct Gfx {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    renderer: Renderer,
    depth: gk3_render::FrameBuffers,
    world: World<'static>,
    ui: Ui,
    menu: Option<Menu>,
    inventory: Inventory,
    hover: Option<String>,
    pointer: (f32, f32),
    /// Where the left button went down, and whether it has since dragged
    /// the camera rather than clicked.
    press: Option<(f32, f32)>,
    dragging: bool,
    controls: Controls,
    last_frame: f64,
    /// A message shown until a time, such as "Game saved".
    notice: Option<(String, f64)>,
    /// The card between timeblocks, while it is up; and, for a new game's
    /// first card, when it goes by itself.
    card: Option<TitleCard>,
    card_until: Option<f64>,
    /// The driving map's buttons as last drawn, for clicks.
    map_buttons: Vec<(&'static str, [f32; 4])>,
    /// The binoculars' buttons as last drawn.
    binocs_buttons: Vec<(BinocsButton, [f32; 4])>,
    /// The GPS's panel and power button, while it is up.
    gps_rects: Option<([f32; 4], [f32; 4])>,
    /// The fingerprint kit's parts as last drawn, and the pointer then.
    kit_layout: KitLayout,
    kit_pointer: (f32, f32),
    started: Instant,
    overlay: Overlay,
    /// Where the presentation before the game has got to, if it is on.
    front: Option<Front>,
    /// The title screen's buttons as last drawn, and the one held down.
    title_buttons: Vec<(TitleButton, [f32; 4])>,
    title_held: Option<TitleButton>,
    /// The player asked to quit.
    quit: bool,
    /// Where a new game starts: timeblock and location.
    start: (String, String),
    /// The option bar's layout, from the game's data.
    layout: Layout,
    /// The option bar, while it is up.
    bar: Option<OptionBar>,
    /// The save and restore screen, a popup, the death and end screens and
    /// the pause sign: each stops the story while it is up.
    save_load: Option<SaveLoad>,
    popup: Option<Popup>,
    /// The cursor wanted (by name, and whether it animates), since when,
    /// and the cursors made so far, by name and scale.
    cursor_want: (String, bool),
    cursor_since: f64,
    cursor_shown: Option<(String, u32, usize)>,
    cursors: std::collections::HashMap<(String, u32), Option<Cursors>>,
    /// The thing last under the pointer, and whether its highlight is the
    /// blue one: moving straight from one thing to another flips it, so
    /// neighbours can be told apart, as G-Engine.
    last_hover: Option<String>,
    blue: bool,
    /// A cursor the room's own code asks for (the pendulum's grab).
    room_cursor: Option<&'static str>,
    /// The tooltip wanted: its text, when it was asked for and how long it
    /// waits; and when the last one went.
    tip: Option<(String, f64, f64)>,
    tip_gone: f64,
    /// Pictures of the room's camera angles, made as the list opens.
    camera_shots: Vec<gk3_render::OverlayTexture>,
    /// The help screen's page, while it is up; and its pages.
    help: Option<usize>,
    help_pages: Help,
    death: bool,
    finished: bool,
    paused: bool,
    /// There are saves to restore; looked at when the saves change.
    any_saves: bool,
    settings: Settings,
    /// The screens' buttons as last drawn, top last; the one pressed; and a
    /// volume slider being dragged.
    buttons: Vec<(Button, [f32; 4])>,
    held_button: Option<Button>,
    sliding: Option<(Slider, [f32; 4])>,
}

impl Game {
    fn open(&mut self, event_loop: &ActiveEventLoop) -> Result<Gfx, String> {
        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title("Gabriel Knight 3")
                        .with_inner_size(winit::dpi::LogicalSize::new(1280.0, 720.0)),
                )
                .map_err(|e| format!("cannot open a window: {e}"))?,
        );
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_with_display_handle_from_env(
                Box::new(event_loop.owned_display_handle()),
            ));
        let surface = instance
            .create_surface(window.clone())
            .map_err(|e| format!("cannot draw to the window: {e}"))?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            ..Default::default()
        }))
        .map_err(|e| format!("no GPU can draw to this window: {e}"))?;
        let (device, queue) = gk3_render::request_device(&adapter)?;
        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| !f.is_srgb())
            .or(caps.formats.first().copied())
            .ok_or("the window offers no formats")?;
        let size = window.inner_size();
        let mut config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .ok_or("the window cannot be drawn to by this GPU")?;
        config.format = format;
        config.present_mode = wgpu::PresentMode::AutoVsync;
        surface.configure(&device, &config);
        let samples = gk3_render::window_samples(&adapter, format);

        let mut renderer = Renderer::with_samples(device, queue, format, samples);
        let depth = renderer.frame_buffers(config.width, config.height);
        let ui = Ui::new(self.lib, &mut renderer);
        let audio = gk3_audio::Audio::open("Gabriel Knight 3");
        if audio.is_silent() {
            eprintln!("no sound: {}", audio.status);
        }
        let settings = Settings::load();
        settings.apply(&audio);
        let info = adapter.get_info();
        if info.device_type == wgpu::DeviceType::Cpu {
            renderer.max_anisotropy = 1;
        }
        renderer.set_filtering(settings.filtering());
        renderer.adapter_name = info.name;
        let mut world = World::new(self.lib, &self.options.time, audio);
        world.door_sounds = settings.door_sounds;
        // A new game opens as the original does: the Sierra logo, the
        // intro on the first run, and the title screen.
        let new_game = self.options.new_game && self.options.load.is_none();
        let front = if new_game {
            world.play_movie("SIERRA", AfterMovie::Nothing);
            Some(Front::Logo)
        } else {
            crate::play::start(&mut world, &renderer, &self.options)?;
            None
        };
        Ok(Gfx {
            window,
            surface,
            config,
            renderer,
            depth,
            world,
            ui,
            menu: None,
            inventory: Inventory::default(),
            hover: None,
            pointer: (0.0, 0.0),
            press: None,
            dragging: false,
            controls: Controls::default(),
            last_frame: 0.0,
            notice: None,
            card: None,
            card_until: None,
            map_buttons: Vec::new(),
            binocs_buttons: Vec::new(),
            gps_rects: None,
            kit_layout: KitLayout::default(),
            kit_pointer: (0.0, 0.0),
            started: Instant::now(),
            overlay: Overlay::default(),
            front,
            title_buttons: Vec::new(),
            title_held: None,
            quit: false,
            start: (self.options.time.clone(), self.options.location.clone()),
            layout: Layout::load(self.lib),
            bar: None,
            save_load: None,
            popup: None,
            camera_shots: Vec::new(),
            cursor_want: ("C_POINT".into(), true),
            cursor_since: 0.0,
            cursor_shown: None,
            cursors: Default::default(),
            last_hover: None,
            blue: false,
            room_cursor: None,
            tip: None,
            tip_gone: 0.0,
            help: None,
            help_pages: Help::load(self.lib),
            death: false,
            finished: false,
            paused: false,
            any_saves: !crate::saves::list().is_empty(),
            settings,
            buttons: Vec::new(),
            held_button: None,
            sliding: None,
        })
    }
}

impl Gfx {
    fn size(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
    }

    /// The player may move the camera only in the room itself, with nothing
    /// playing and no menu or inventory up.
    fn camera_free(&self) -> bool {
        self.camera_yours()
            && !self.modal()
            && self.bar.is_none()
            && self.menu.is_none()
            && !self.world.state.inventory_shown
            && self.world.binocs.is_none()
            && self.world.fingerprint.is_none()
            && self.world.sidney.is_none()
    }

    /// Moves the presentation on once its movie is over: the logo gives
    /// way to the intro (the first time) or the title screen.
    fn front_step(&mut self) {
        if self.world.showing.is_some() {
            return;
        }
        match self.front {
            Some(Front::Logo) if !crate::saves::intro_seen() => {
                crate::saves::mark_intro_seen();
                self.world.play_movie("INTRO", AfterMovie::Nothing);
                self.front = Some(Front::Intro);
            }
            Some(Front::Logo | Front::Intro) => {
                self.front = Some(Front::Title);
                self.world.play_soundtrack("TITLETHEME", false);
            }
            _ => {}
        }
    }

    /// A title screen button, clicked or by its key.
    fn title_press(&mut self, button: TitleButton) {
        match button {
            TitleButton::Intro => {
                self.world.stop_soundtrack(None);
                if self.world.play_movie("INTRO", AfterMovie::Nothing) {
                    self.front = Some(Front::Intro);
                }
            }
            // A new game opens on its first title card, which goes by
            // itself after five seconds, as in G-Engine.
            TitleButton::Play => {
                self.front = None;
                let (time, location) = self.start.clone();
                self.world.end_timeblock(&time, &location);
                self.card_until = Some(self.started.elapsed().as_secs_f64() + 5.0);
            }
            TitleButton::Restore if self.any_saves => self.open_save_load(false),
            TitleButton::Restore => {}
            TitleButton::Quit => self.quit = true,
        }
    }

    /// A screen that stops the story while it is up.
    fn modal(&self) -> bool {
        self.save_load.is_some()
            || self.popup.is_some()
            || self.help.is_some()
            || self.death
            || self.finished
            || self.paused
    }

    /// The camera is the player's: always with cinematics off, otherwise
    /// only while the story is idle.
    fn camera_yours(&self) -> bool {
        (self.world.idle() || !self.settings.cinematics) && !self.world.forced_cuts
    }

    /// A close-up, the fingerprint kit or the map is up, for the option
    /// bar's way out.
    fn exitable(&self) -> bool {
        self.inventory.closeup.is_some() || self.world.fingerprint.is_some() || self.world.map.is_some()
    }

    /// Asks for a tooltip (or none). It shows after a wait: shorter if one
    /// was up a moment ago, as `DEFAULT.TIP` gives them.
    fn set_tip(&mut self, text: Option<String>, now: f64) {
        match text {
            Some(text) if self.tip.as_ref().is_some_and(|t| t.0 == text) => {}
            Some(text) => {
                let delay = if now - self.tip_gone < 0.5 { 0.2 } else { 0.6 };
                self.tip = Some((text, now, delay));
            }
            None => {
                if self.tip.take().is_some() {
                    self.tip_gone = now;
                }
            }
        }
    }

    /// Shows the cursor wanted, making the system cursors for it the first
    /// time, at the interface's whole-number scale.
    fn show_cursor(&mut self, event_loop: &ActiveEventLoop) {
        let scale = (self.config.height / 480).max(1);
        let (name, animate) = self.cursor_want.clone();
        let key = (name.clone(), scale);
        let lib = self.world.lib;
        let entry = self.cursors.entry(key).or_insert_with(|| {
            let art = crate::cursor::CursorArt::load(lib, &name, scale)?;
            let frames = art
                .frames
                .iter()
                .filter_map(|f| {
                    let source = winit::window::CustomCursor::from_rgba(
                        f.rgba.clone(),
                        f.width as u16,
                        f.height as u16,
                        art.hotspot.0 as u16,
                        art.hotspot.1 as u16,
                    )
                    .ok()?;
                    Some(event_loop.create_custom_cursor(source))
                })
                .collect::<Vec<_>>();
            (!frames.is_empty()).then_some(Cursors { art, frames })
        });
        let Some(cursors) = entry else {
            return;
        };
        let now = self.started.elapsed().as_secs_f64();
        let frame = cursors
            .art
            .frame_at(now - self.cursor_since, animate)
            .min(cursors.frames.len() - 1);
        let shown = Some((name, scale, frame));
        if self.cursor_shown != shown {
            self.window.set_cursor(cursors.frames[frame].clone());
            self.cursor_shown = shown;
        }
    }

    /// The pointer in SIDNEY's 640x480 pixels.
    fn sidney_at(&self) -> (f32, f32) {
        let space = Space::of(self.size());
        (
            (self.pointer.0 - space.ox) / space.fit,
            (self.pointer.1 - space.oy) / space.fit,
        )
    }

    /// The topmost screen button under the pointer.
    fn button_at(&self, at: (f32, f32)) -> Option<Button> {
        self.buttons
            .iter()
            .rev()
            .find(|(_, r)| inside(*r, at))
            .map(|(b, _)| *b)
    }

    fn click_sound(&mut self) {
        self.world.sound("SIDBUTN-1", 1.0, gk3_audio::Bus::Effects);
    }

    /// Whether the game may be saved from where the player is, as the
    /// original asks before its save screen or quick save (0x4cbe72): on
    /// a title card or the death screen; else with no verb menu up and
    /// none of the screens that forbid it (inventory, close-up, help,
    /// pause, a message, the finished screen), and the world allowing it.
    fn may_save(&self) -> Result<(), String> {
        if self.card.is_some() || self.death {
            return Ok(());
        }
        if self.menu.is_some() {
            return Err("the verbs are up".into());
        }
        if self.inventory.open
            || self.inventory.closeup.is_some()
            || self.help.is_some()
            || self.paused
            || self.popup.is_some()
            || self.finished
            || self.quit
        {
            return Err("not on this screen".into());
        }
        self.world.may_save()
    }

    /// Whether a saved game may be put back from where the player is
    /// (0x4cbebf): the title screen, a title card and the death screen
    /// allow it; else not the finished screen, and the world must allow it.
    fn may_restore(&self) -> Result<(), String> {
        if self.front.is_some() || self.card.is_some() || self.death {
            return Ok(());
        }
        if self.finished {
            return Err("not on this screen".into());
        }
        self.world.may_restore()
    }

    /// Opens the save screen (if the story can be saved now) or the restore
    /// screen (if a game can be put back now).
    fn open_save_load(&mut self, saving: bool) {
        self.bar = None;
        let allowed = if saving {
            self.may_save()
        } else {
            self.may_restore()
        };
        if let Err(e) = allowed {
            self.tell(format!("Not now: {e}"));
            return;
        }
        self.save_load = Some(SaveLoad::new(saving));
    }

    /// Saves into the highlighted slot under the name typed.
    fn commit_save(&mut self) {
        let Some(screen) = self.save_load.take() else {
            return;
        };
        let name = screen.typing.clone().unwrap_or_default();
        let slot = screen
            .chosen()
            .map(|s| s.slot.clone())
            .unwrap_or_else(crate::saves::free_slot);
        let result = self
            .world
            .save()
            .and_then(|text| crate::saves::write(&slot, &crate::saves::named(&text, &name)));
        match result {
            Ok(_) => {
                self.save_thumbnail(&slot);
                self.tell("Game saved".into());
            }
            Err(e) => self.tell(format!("Not saved: {e}")),
        }
        self.any_saves = !crate::saves::list().is_empty();
    }

    /// A small picture of the room as it stands, beside the save. Drawn at
    /// 640x480 and shrunk, so it is smooth.
    fn save_thumbnail(&mut self, slot: &str) {
        let (Some(scene), Some(seen)) = (&self.world.scene, self.world.view_scene()) else {
            return;
        };
        if self.card.is_some() {
            return;
        }
        let image = self.renderer.render_image(
            seen,
            &scene.cast.models(),
            &gk3_render::Overlay::default(),
            &self.world.camera,
            ShadeMode::Lit,
            (640, 480),
        );
        if let Ok(image) = image {
            let _ = crate::saves::write_thumb(slot, &crate::saves::shrink(&image, 4));
        }
    }

    /// Restores the highlighted save.
    fn load_chosen(&mut self) {
        let Some(slot) = self
            .save_load
            .as_ref()
            .and_then(|s| s.chosen())
            .map(|s| s.slot.clone())
        else {
            return;
        };
        self.save_load = None;
        self.load_slot(&slot);
    }

    /// Restores a save, from wherever the player is: the title screen, the
    /// death screen, a title card or the story.
    fn load_slot(&mut self, slot: &str) {
        let result = crate::saves::read(slot).and_then(|text| {
            // The world stands still under the screens; it starts afresh.
            self.world.hold(false);
            self.world.load(&self.renderer, &text)
        });
        match result {
            Ok(()) => {
                if self.front.take().is_some() {
                    self.world.stop_soundtrack(None);
                }
                self.death = false;
                self.card = None;
                self.card_until = None;
                self.bar = None;
                self.menu = None;
                self.inventory.closeup = None;
                self.tell("Game loaded".into());
            }
            Err(e) => self.tell(format!("Not loaded: {e}")),
        }
    }

    fn set_slider(&mut self, slider: Slider, value: f32) {
        let s = &mut self.settings;
        match slider {
            Slider::Master | Slider::MasterToo => s.master = value,
            Slider::Effects => s.effects = value,
            Slider::Dialogue => s.dialogue = value,
            Slider::Music => s.music = value,
        }
        self.settings.apply(&self.world.audio);
    }

    /// A press on one of the screens' buttons.
    fn screen_button(&mut self, button: Button) {
        match button {
            Button::Panel | Button::Slider(_) => {}
            Button::ExitScreen => {
                self.bar = None;
                if self.inventory.closeup.take().is_some() {
                } else if self.world.fingerprint.is_some() {
                    self.world
                        .fingerprint_events(vec![crate::fingerprint::Event::Close]);
                } else if self.world.map.is_some() {
                    self.world.map_cancel();
                }
            }
            Button::Hint => {
                self.bar = None;
                let state = self.world.state.clone();
                if let Some(map) = &mut self.world.map
                    && !map.hint(&state)
                {
                    self.tell("No hints here".into());
                }
            }
            Button::Cinematics => {
                self.settings.cinematics = !self.settings.cinematics;
                self.settings.store();
            }
            Button::Help => {
                self.bar = None;
                self.help = Some(0);
            }
            Button::HelpPrevious => {
                if let Some(page) = &mut self.help {
                    *page = page.saturating_sub(1);
                }
            }
            Button::HelpNext => {
                let last = self.help_pages.pages.len().saturating_sub(1);
                if let Some(page) = &mut self.help {
                    *page = (*page + 1).min(last);
                }
            }
            Button::HelpExit => self.help = None,
            Button::Inventory => {
                self.bar = None;
                if self.world.idle() {
                    self.world.state.inventory_shown = !self.world.state.inventory_shown;
                    self.inventory.closeup = None;
                }
            }
            Button::ActiveItem => {
                self.bar = None;
                let ego = &self.world.state.ego;
                if let Some(item) = self.world.state.active_item.get(ego).cloned() {
                    self.world.state.inventory_shown = true;
                    self.inventory.closeup = Some(item);
                }
            }
            Button::Radio => {
                self.bar = None;
                let location = self.world.state.location.clone();
                self.world.call(&self.renderer, &location, "RadioButton$");
            }
            Button::Cameras => {
                let opening = self.bar.as_ref().is_some_and(|b| !b.cameras);
                if opening {
                    self.camera_shots =
                        crate::screens::camera_shots(&self.world, &mut self.renderer, &mut self.ui);
                }
                if let Some(bar) = &mut self.bar {
                    bar.press(button);
                }
            }
            Button::Camera(n) => {
                self.bar = None;
                if let Some(&i) = crate::screens::room_cameras(&self.world).get(n) {
                    self.room_camera(i);
                }
            }
            Button::Options
            | Button::Advanced
            | Button::SoundOptions
            | Button::GraphicsOptions
            | Button::GameOptions
            | Button::GraphicsAdvanced
            | Button::Toggle(Toggle::Glide) => {
                if let Some(bar) = &mut self.bar {
                    bar.press(button);
                }
            }
            Button::Toggle(toggle) => {
                let s = &mut self.settings;
                let flag = match toggle {
                    Toggle::MuteAll => &mut s.mute_all,
                    Toggle::MuteEffects => &mut s.mute_effects,
                    Toggle::MuteDialogue => &mut s.mute_dialogue,
                    Toggle::MuteMusic => &mut s.mute_music,
                    Toggle::Captions | Toggle::Glide => &mut s.captions,
                    Toggle::DoorSounds => &mut s.door_sounds,
                    Toggle::Mipmaps => &mut s.mipmaps,
                    Toggle::Trilinear => &mut s.trilinear,
                };
                *flag = !*flag;
                if matches!(toggle, Toggle::Mipmaps | Toggle::Trilinear) {
                    self.renderer.set_filtering(self.settings.filtering());
                    self.world.refilter(&self.renderer);
                }
                self.world.door_sounds = self.settings.door_sounds;
                self.settings.apply(&self.world.audio);
                self.settings.store();
            }
            Button::CloseBar => self.bar = None,
            Button::Save => self.open_save_load(true),
            Button::Restore | Button::DeathRestore => self.open_save_load(false),
            Button::QuitGame | Button::DeathQuit => {
                self.bar = None;
                self.popup = Some(Popup::Quit);
            }
            Button::Row(row) => {
                let Some(screen) = &mut self.save_load else {
                    return;
                };
                // Saving, a second click on the highlighted row saves there.
                let again = row == screen.selected && screen.saving;
                screen.select(row);
                if again {
                    self.screen_button(Button::SaveHere);
                }
            }
            Button::SaveHere => {
                self.click_sound();
                let Some(screen) = &mut self.save_load else {
                    return;
                };
                match (&screen.typing, screen.chosen()) {
                    (Some(_), _) => self.commit_save(),
                    // Over a save: asked first, then its name to change.
                    (None, Some(_)) => self.popup = Some(Popup::Overwrite),
                    (None, None) => screen.typing = Some(String::new()),
                }
            }
            Button::LoadThis => {
                self.click_sound();
                self.load_chosen();
            }
            Button::ExitSaveLoad => {
                self.click_sound();
                self.save_load = None;
            }
            Button::ScrollUp | Button::ScrollDown => {
                if let Some(screen) = &mut self.save_load {
                    screen.scroll_by(if button == Button::ScrollUp { -1 } else { 1 });
                }
            }
            Button::Yes => match self.popup.take() {
                Some(Popup::Quit) => self.quit = true,
                Some(Popup::Overwrite) => {
                    if let Some(screen) = &mut self.save_load {
                        screen.typing = screen.chosen().map(|s| s.name.clone());
                    }
                }
                None => {}
            },
            Button::No => {
                self.popup = None;
                self.click_sound();
            }
            // As G-Engine: the location's script may say what a retry does.
            Button::Retry => {
                self.death = false;
                self.world.hold(false);
                let location = self.world.state.location.clone();
                self.world.call(&self.renderer, &location, "PostDeath$");
            }
        }
    }

    /// Puts up a screen the story asked for.
    fn story_screen(&mut self, request: ScreenRequest) {
        self.bar = None;
        self.menu = None;
        match request {
            ScreenRequest::Death => {
                self.death = true;
                // The world stops first, so the sting is heard over it.
                self.world.hold(true);
                self.world.sound("TEMPLEDEATHTAG", 1.0, gk3_audio::Bus::Music);
            }
            ScreenRequest::Finished => self.finished = true,
            ScreenRequest::Quit => self.popup = Some(Popup::Quit),
        }
    }

    /// Typing a save's name.
    fn type_name(&mut self, code: KeyCode, text: Option<&str>) {
        let Some(screen) = &mut self.save_load else {
            return;
        };
        let Some(name) = &mut screen.typing else {
            return;
        };
        match code {
            KeyCode::Backspace => {
                name.pop();
            }
            KeyCode::Enter | KeyCode::NumpadEnter => self.commit_save(),
            KeyCode::Escape => self.save_load = None,
            _ => {
                for c in text.unwrap_or("").chars().filter(|c| !c.is_control()) {
                    if name.chars().count() < crate::screens::NAME_LENGTH {
                        name.push(c);
                    }
                }
            }
        }
    }

    /// A key while one of the screens is up. True if the screen took it.
    fn screen_key(&mut self, code: KeyCode) -> bool {
        if self.finished {
            self.quit = true;
            return true;
        }
        if self.popup.is_some() {
            match code {
                KeyCode::Enter | KeyCode::NumpadEnter => self.screen_button(Button::Yes),
                KeyCode::Escape => self.screen_button(Button::No),
                _ => {}
            }
            return true;
        }
        if let Some(screen) = &mut self.save_load {
            match code {
                KeyCode::ArrowUp => screen.select(screen.selected.saturating_sub(1)),
                KeyCode::ArrowDown => screen.select(screen.selected + 1),
                KeyCode::PageUp => screen.scroll_by(-10),
                KeyCode::PageDown => screen.scroll_by(10),
                KeyCode::Escape => self.screen_button(Button::ExitSaveLoad),
                KeyCode::Enter | KeyCode::NumpadEnter => {
                    let button = if screen.saving { Button::SaveHere } else { Button::LoadThis };
                    self.screen_button(button);
                }
                _ => {}
            }
            return true;
        }
        if self.paused {
            if matches!(code, KeyCode::KeyP | KeyCode::Pause) {
                self.paused = false;
            }
            return true;
        }
        if self.help.is_some() {
            match code {
                KeyCode::Escape | KeyCode::F1 | KeyCode::KeyH => self.help = None,
                KeyCode::ArrowLeft | KeyCode::PageUp => self.screen_button(Button::HelpPrevious),
                KeyCode::ArrowRight | KeyCode::PageDown => self.screen_button(Button::HelpNext),
                _ => {}
            }
            return true;
        }
        self.death
    }

    /// Leaves the title card for the timeblock it announced.
    fn continue_timeblock(&mut self) {
        self.card = None;
        self.card_until = None;
        if let Err(e) = self.world.continue_timeblock(&self.renderer) {
            self.tell(format!("Cannot go on: {e}"));
        }
    }

    /// A click on the fingerprint kit.
    fn kit_click(&mut self) {
        let layout = &self.kit_layout;
        let at = self.pointer;
        let Some(kit) = &mut self.world.fingerprint else {
            return;
        };
        if inside(layout.exit, at) {
            self.world
                .fingerprint_events(vec![crate::fingerprint::Event::Close]);
        } else if inside(layout.cloth, at) {
            let events = kit.click_cloth();
            self.world.fingerprint_events(events);
        } else if let Some(i) = layout.prints.iter().position(|r| inside(*r, at)) {
            kit.click_print(i);
        } else if inside(layout.brush, at) {
            kit.click_brush();
        } else if inside(layout.dust, at) {
            kit.click_dust();
        } else if inside(layout.tape, at) {
            kit.click_tape();
        } else {
            kit.click_elsewhere();
        }
    }

    fn tell(&mut self, text: String) {
        let until = self.started.elapsed().as_secs_f64() + 3.0;
        self.notice = Some((text, until));
    }

    fn quick_save(&mut self) {
        let result = self
            .may_save()
            .and_then(|()| self.world.save())
            .and_then(|text| crate::saves::write(crate::saves::QUICK, &text));
        match result {
            Ok(_) => {
                self.save_thumbnail(crate::saves::QUICK);
                self.any_saves = true;
                self.tell("Game saved".into());
            }
            Err(e) => self.tell(format!("Not saved: {e}")),
        }
    }

    fn quick_load(&mut self) {
        let result = self
            .may_restore()
            .and_then(|()| crate::saves::read(crate::saves::QUICK))
            .and_then(|text| self.world.load(&self.renderer, &text));
        match result {
            Ok(()) => {
                self.menu = None;
                self.inventory.closeup = None;
                self.tell("Game loaded".into());
            }
            Err(e) => self.tell(format!("Not loaded: {e}")),
        }
    }

    /// Jumps to one of the room's camera angles, by number from 1.
    fn room_camera(&mut self, index: usize) {
        let Some(scene) = &self.world.scene else {
            return;
        };
        if let Some(c) = scene.room.cameras.get(index) {
            self.world.camera = crate::shoot::from_room_camera(c);
        }
    }

    /// Closes the menu; closing a topic bar ends the conversation.
    fn close_menu(&mut self) {
        // A modal menu (`SetVerbModal`) must be answered.
        if self.world.verb_modal {
            return;
        }
        if let Some(menu) = self.menu.take()
            && menu.topics
        {
            self.world.end_conversation(&self.renderer);
        }
    }

    /// The player's items, in a steady order.
    fn items(&self) -> Vec<String> {
        let state = &self.world.state;
        state
            .inventory
            .get(&state.ego)
            .map(|i| i.iter().cloned().collect())
            .unwrap_or_default()
    }

    fn click(&mut self) {
        let size = self.size();
        let held = self.held_button.take();
        // The screens over the story take every click; the option bar
        // goes away at one off it.
        if self.modal() || self.bar.is_some() {
            let hit = self.button_at(self.pointer);
            match hit {
                Some(b) if Some(b) == held => self.screen_button(b),
                None if !self.modal() => self.bar = None,
                _ => {}
            }
            return;
        }
        // A click skips a movie.
        if self.world.showing.is_some() {
            self.world.stop_movie(&self.renderer);
            return;
        }
        if self.front == Some(Front::Title) {
            let hit = self
                .title_buttons
                .iter()
                .find(|(_, r)| inside(*r, self.pointer))
                .map(|(b, _)| *b);
            if let Some(button) = hit.filter(|b| Some(*b) == self.title_held) {
                self.title_press(button);
            }
            self.title_held = None;
            return;
        }
        if self.card.is_some() {
            // A card on a timer has no buttons.
            if self.card_until.is_some() {
            } else if inside(Ui::card_continue(size), self.pointer) {
                self.continue_timeblock();
            } else if inside(Ui::card_save(size), self.pointer) {
                self.open_save_load(true);
            }
            return;
        }
        if self.world.fingerprint.is_some() {
            if self.menu.is_some() {
                // Saying whose print it is: the topic bar handles that below.
            } else {
                self.kit_click();
                return;
            }
        }
        if self.world.binocs.is_some() && self.world.idle() {
            let hit = self
                .binocs_buttons
                .iter()
                .find(|(_, r)| inside(*r, self.pointer))
                .map(|(b, _)| *b);
            match hit {
                Some(BinocsButton::Exit) => self.world.binocs_exit(&self.renderer),
                Some(BinocsButton::Zoom) => {
                    if self
                        .world
                        .binocs
                        .as_ref()
                        .is_some_and(|b| b.zoomed.is_some())
                    {
                        self.world.binocs_zoom_out();
                    } else {
                        self.world.binocs_zoom_in(&self.renderer);
                    }
                }
                _ => {}
            }
            return;
        }
        // SIDNEY takes clicks, unless Add Data has the inventory up.
        if self.world.sidney.is_some() && !self.world.state.inventory_shown {
            let hit = self
                .world
                .sidney
                .as_ref()
                .and_then(|s| s.hit(Space::of(size), self.pointer));
            match hit {
                Some(hit) => self.world.sidney_click(&self.renderer, hit),
                None if !self.world.captions.is_empty() => self.world.skip(),
                None => {}
            }
            return;
        }
        if self.world.map.is_some() {
            if let Some((id, _)) = self
                .map_buttons
                .iter()
                .find(|(_, r)| inside(*r, self.pointer))
            {
                let id = *id;
                self.world.map_press(id);
            } else if !self.world.captions.is_empty() {
                self.world.skip();
            }
            return;
        }
        self.inventory.open = self.world.state.inventory_shown;
        if self.inventory.closeup.take().is_some() {
            return;
        }
        if let Some(menu) = self.menu.take() {
            let slots = Ui::menu_slots(&menu, size);
            match slots.iter().position(|s| inside(*s, self.pointer)) {
                Some(i) => {
                    let rule = menu.rules[i].clone();
                    if rule.source == "UI" && rule.verb == "CANCEL" {
                        if menu.topics {
                            self.world.end_conversation(&self.renderer);
                        }
                    } else if rule.source == "UI" && rule.verb == "INSPECT" {
                        self.inventory.closeup = Some(rule.noun);
                    } else if rule.source == "UI" && rule.verb == "PICKUP" {
                        let ego = self.world.state.ego.clone();
                        self.world.state.active_item.insert(ego, rule.noun);
                    } else {
                        self.world.act(&self.renderer, rule);
                    }
                }
                None if self.world.verb_modal => self.menu = Some(menu),
                None if menu.topics => self.world.end_conversation(&self.renderer),
                None => {}
            }
            return;
        }
        if self.inventory.open {
            let items = self.items();
            let slots = Ui::inventory_slots(&items, size);
            match slots.iter().position(|s| inside(*s, self.pointer)) {
                Some(i) if self.world.idle() => {
                    // As G-Engine's inventory screen: INSPECT first, then
                    // the item's verbs with PICKUP (take it in hand) after LOOK.
                    let item = items[i].clone();
                    let screen = |verb: &str| Rule {
                        noun: item.clone(),
                        verb: verb.into(),
                        case: "ALL".into(),
                        approach: None,
                        target: None,
                        script: String::new(),
                        source: "UI".into(),
                    };
                    let mut rules = vec![screen("INSPECT")];
                    let verbs = self.world.verbs(&item);
                    let after_look = verbs
                        .iter()
                        .position(|r| r.verb == "LOOK")
                        .map_or(0, |i| i + 1);
                    // The story's own PICKUP has no place in the inventory:
                    // there PICKUP takes the item in hand, as G-Engine.
                    rules.extend(verbs.into_iter().filter(|r| r.verb != "PICKUP"));
                    rules.insert(after_look + 1, screen("PICKUP"));
                    self.menu = Some(Menu::new(item, rules, (false, self.world.verb_cancel), (self.pointer.0, self.pointer.1)));
                }
                Some(_) => self.world.skip(),
                None => self.world.state.inventory_shown = false,
            }
            return;
        }
        if !self.world.idle() {
            // A click while someone is talking moves the conversation on.
            self.world.skip();
            return;
        }
        // The GPS takes clicks on its panel; its power button turns it off.
        if let Some((panel, power)) = self.gps_rects
            && inside(panel, self.pointer)
        {
            if inside(power, self.pointer) {
                self.world.gps_power();
            }
            return;
        }
        let (pick, object) = self
            .world
            .pick_object(self.pointer.0, self.pointer.1, size.0, size.1);
        // Some rooms take clicks on their own pieces (TE1's floor).
        if self.world.click_object(&self.renderer, object.as_deref()) {
            return;
        }
        match pick {
            Pick::Noun(noun) => {
                let rules = self.world.verbs(&noun);
                // A thing with a verb of its own (a way out) does it at once.
                let own = self
                    .world
                    .object_verb(&noun)
                    .and_then(|v| rules.iter().find(|r| r.verb == v))
                    .cloned();
                if let Some(rule) = own {
                    self.world.act(&self.renderer, rule);
                } else if rules.len() == 1 {
                    let rule = rules[0].clone();
                    self.world.act(&self.renderer, rule);
                } else if !rules.is_empty() {
                    self.menu = Some(Menu::new(noun, rules, (false, self.world.verb_cancel), (self.pointer.0, self.pointer.1)));
                }
            }
            Pick::Floor(point) => {
                self.world.walk_ego(point, None, crate::world::EgoWalk::Click);
            }
            Pick::Nothing => {}
        }
    }

    fn redraw(&mut self) {
        let now = self.started.elapsed().as_secs_f64();
        let (w, h) = self.size();
        self.world.aspect = w as f32 / h.max(1) as f32;
        let modal = self.modal();
        self.world.hold(modal);
        self.world.update(&self.renderer, now);
        if let Some(request) = self.world.screen.take() {
            self.story_screen(request);
        }
        self.front_step();
        // A script opening or closing an item's close-up.
        if let Some(inspect) = self.world.inspect.take() {
            self.inventory.open = self.world.state.inventory_shown;
            self.inventory.closeup = inspect;
        }
        let size = self.size();
        let dt = (now - self.last_frame).clamp(0.0, 0.1) as f32;
        self.last_frame = now;
        if self.camera_free() {
            let none = crate::camera::Bounds::default();
            let bounds = self
                .world
                .scene
                .as_ref()
                .map_or(&none, |s| &s.camera_bounds);
            let before = self.world.camera.position;
            self.controls.apply(&mut self.world.camera, bounds, dt);
            // Moving away from a close-up ends it, as G-Engine.
            if self.world.camera.position.distance(before) > 1.0 {
                self.world.camera_moved();
            }
        } else {
            self.controls.drag = (0.0, 0.0);
        }
        // Rubbing with the dusted brush brings prints up.
        if self.press.is_some() {
            let moved =
                (self.pointer.0 - self.kit_pointer.0).hypot(self.pointer.1 - self.kit_pointer.1);
            let over: Vec<usize> = (0..self.kit_layout.prints.len())
                .filter(|&i| inside(self.kit_layout.prints[i], self.pointer))
                .collect();
            let in_panel = inside(self.kit_layout.panel, self.pointer);
            if let Some(kit) = &mut self.world.fingerprint {
                let events = kit.dust(&over, in_panel, moved, dt);
                self.world.fingerprint_events(events);
            }
        }
        self.kit_pointer = self.pointer;
        // The fingerprint kit draws its tools as the pointer, and movies
        // hide it, as G-Engine locks the mouse under them.
        self.window.set_cursor_visible(
            self.world.showing.is_none()
                && self
                    .world
                    .fingerprint
                    .as_ref()
                    .is_none_or(|k| k.tool.cursor().is_none()),
        );
        // The binoculars turn while an arrow is held, a radian a second.
        if self.world.binocs.is_some() {
            let c = &self.controls;
            let (mut dx, mut dy) = (
                (c.right as i32 - c.left as i32) as f32,
                (c.back as i32 - c.forward as i32) as f32,
            );
            if self.press.is_some() {
                for (button, rect) in &self.binocs_buttons {
                    if inside(*rect, self.pointer) {
                        match button {
                            BinocsButton::Up => dy -= 1.0,
                            BinocsButton::Down => dy += 1.0,
                            BinocsButton::Left => dx -= 1.0,
                            BinocsButton::Right => dx += 1.0,
                            _ => {}
                        }
                    }
                }
            }
            self.world.binocs_turn(dx * dt, dy * dt);
        }
        // A timeblock has ended: its successor's card goes up, once the
        // ending film (if any) is over.
        if let Some(change) = &self.world.timeblock_change
            && self.card.is_none()
            && self.world.showing.is_none()
        {
            self.menu = None;
            self.world.state.inventory_shown = false;
            self.card = Some(TitleCard::new(self.world.lib, &change.to, now));
            self.world
                .sound("CLOCKTIMEBLOCK", 1.0, gk3_audio::Bus::Effects);
        }
        if self.card_until.is_some_and(|t| now >= t) {
            self.continue_timeblock();
        }
        // Scripts show and hide the inventory too.
        self.inventory.open = self.world.state.inventory_shown;
        if !self.inventory.open {
            self.inventory.closeup = None;
        }

        // The topics come up by themselves once the world is free.
        if self.menu.is_none()
            && self.world.idle()
            && let Some(noun) = self.world.topic_bar.take()
        {
            let rules = self.world.topics(&noun);
            if rules.is_empty() {
                self.world.end_conversation(&self.renderer);
            } else {
                self.menu = Some(Menu::new(noun, rules, (true, self.world.verb_cancel), (size.0 as f32 / 2.0, size.1 as f32 * 0.75)));
            }
        }

        self.hover = None;
        self.room_cursor = None;
        if self.menu.is_none()
            && self.bar.is_none()
            && !self.modal()
            && !self.inventory.open
            && !self.dragging
            && self.world.idle()
        {
            let (pick, object) =
                self.world
                    .pick_object(self.pointer.0, self.pointer.1, size.0, size.1);
            self.room_cursor = self.world.object_cursor(object.as_deref());
            if let Pick::Noun(noun) = pick {
                self.hover = Some(noun);
            }
        }

        let mut overlay = std::mem::take(&mut self.overlay);
        // The verb under the pointer on a verb menu, for its tooltip.
        let mut menu_verb: Option<String> = None;
        overlay.clear();
        let full_screen = self.world.showing.is_some() || self.front.is_some();
        if let Some(show) = &self.world.showing {
            self.ui.movie(
                &mut self.renderer,
                &mut overlay,
                show,
                (&self.world.state.ego, self.settings.captions),
                size,
            );
        } else if self.front == Some(Front::Title) {
            self.title_buttons = self.ui.title_screen(
                self.world.lib,
                &mut self.renderer,
                &mut overlay,
                self.pointer,
                self.title_held,
                self.any_saves,
                size,
            );
        } else if let Some(card) = &self.card {
            self.ui.title_card(
                self.world.lib,
                &mut self.renderer,
                &mut overlay,
                card,
                (now, self.pointer, self.card_until.is_none()),
                size,
            );
        } else if self.world.sidney.is_some() {
            let at = self.sidney_at();
            if let Some(sidney) = self.world.sidney.as_mut() {
                sidney.draw(
                    (&mut self.ui, self.world.lib, &mut self.renderer),
                    &mut overlay,
                    &self.world.state,
                    self.pointer,
                    size,
                );
                self.room_cursor = sidney.map_cursor(at, &self.world.state);
            }
            // Add Data brings the inventory up over it.
            if self.inventory.open {
                self.inventory.items = self.items();
                self.ui.inventory(
                    self.world.lib,
                    &mut self.renderer,
                    &mut overlay,
                    &self.inventory,
                    self.pointer,
                    size,
                );
                if let Some(menu) = &self.menu {
                    menu_verb = self.ui.menu(
                        self.world.lib,
                        &mut self.renderer,
                        &mut overlay,
                        menu,
                        (self.pointer, self.press.is_some()),
                        size,
                    );
                }
            }
            if self.settings.captions {
                self.ui.captions(&mut overlay, &self.world, size);
            }
        } else if let Some(kit) = &self.world.fingerprint {
            self.kit_layout = self.ui.fingerprint(
                self.world.lib,
                &mut self.renderer,
                &mut overlay,
                kit,
                (self.pointer, self.press.is_some()),
                size,
            );
            if self.settings.captions {
                self.ui.captions(&mut overlay, &self.world, size);
            }
            if let Some(menu) = &self.menu {
                menu_verb = self.ui.menu(
                    self.world.lib,
                    &mut self.renderer,
                    &mut overlay,
                    menu,
                    (self.pointer, self.press.is_some()),
                    size,
                );
            }
        } else if self.world.binocs.is_some() {
            let zoomed = self
                .world
                .binocs
                .as_ref()
                .is_some_and(|b| b.zoomed.is_some());
            let can_zoom = self.world.binocs_target().is_some();
            self.binocs_buttons = self.ui.binocs(
                self.world.lib,
                &mut self.renderer,
                &mut overlay,
                (zoomed, can_zoom),
                (self.pointer, self.press.is_some()),
                size,
            );
            if self.settings.captions {
                self.ui.captions(&mut overlay, &self.world, size);
            }
        } else if let Some(map) = &self.world.map {
            self.map_buttons = self.ui.map(
                self.world.lib,
                &mut self.renderer,
                &mut overlay,
                map,
                self.pointer,
                size,
            );
            if self.settings.captions {
                self.ui.captions(&mut overlay, &self.world, size);
            }
        } else {
            self.gps_rects = self.world.gps.as_ref().and_then(|gps| {
                self.ui.gps(
                    self.world.lib,
                    &mut self.renderer,
                    &mut overlay,
                    gps,
                    (self.pointer, self.press.is_some()),
                    size,
                )
            });
            if self.inventory.open {
                self.inventory.items = self.items();
                self.ui.inventory(
                    self.world.lib,
                    &mut self.renderer,
                    &mut overlay,
                    &self.inventory,
                    self.pointer,
                    size,
                );
            }
            if self.settings.captions {
                self.ui.captions(&mut overlay, &self.world, size);
            }
            if let Some((text, until)) = &self.notice {
                if now < *until {
                    self.ui.notice(&mut overlay, text, size);
                } else {
                    self.notice = None;
                }
            }
            if let Some(menu) = &self.menu {
                menu_verb = self.ui.menu(
                    self.world.lib,
                    &mut self.renderer,
                    &mut overlay,
                    menu,
                    (self.pointer, self.press.is_some()),
                    size,
                );
            }
        }

        // The screens over everything, the topmost taking the clicks.
        let exitable = self.exitable();
        // The camera list is for looking round the room, when the player
        // may move the camera.
        let cameras_free = self.world.scene.is_some()
            && self.camera_yours()
            && self.world.map.is_none()
            && self.world.sidney.is_none()
            && self.world.fingerprint.is_none()
            && self.world.binocs.is_none();
        let mut painter = Painter {
            ui: &mut self.ui,
            lib: self.world.lib,
            renderer: &mut self.renderer,
            overlay: &mut overlay,
            pointer: self.pointer,
            held: self.held_button,
            size,
            hits: Vec::new(),
        };
        if let Some(bar) = &self.bar {
            let ego = &self.world.state.ego;
            let status = Status {
                exitable,
                on_map: self.world.map.is_some(),
                cinematics: self.settings.cinematics,
                camera_shots: &self.camera_shots,
                cameras_free,
                score: self.world.score(),
                timeblock: &self.world.state.timeblock,
                active_item: self.world.state.active_item.get(ego).map(String::as_str),
                inventory_open: self.world.state.inventory_shown,
                settings: &self.settings,
                any_saves: self.any_saves,
            };
            painter.option_bar(&self.layout, bar, &status);
        }
        if self.death {
            painter.hits.clear();
            painter.death(self.any_saves);
        }
        if self.finished {
            painter.hits.clear();
            painter.finished();
        }
        if self.paused {
            painter.hits.clear();
            painter.paused();
        }
        if let Some(page) = self.help {
            painter.hits.clear();
            painter.help(&self.help_pages, page);
        }
        if let Some(screen) = &self.save_load {
            painter.hits.clear();
            painter.save_load(screen, now);
        }
        if let Some(popup) = self.popup {
            painter.hits.clear();
            painter.popup(popup);
        }
        self.buttons = painter.hits;

        // The tooltip: for a verb, or a button of the screens.
        let under = self.button_at(self.pointer);
        let tip_text = match (&menu_verb, under) {
            (Some(verb), _) => Some(self.ui.verb_tip(verb)),
            (None, Some(b)) => b
                .tip(self.world.state.inventory_shown, self.settings.cinematics)
                .map(|k| self.ui.string(k)),
            _ if self.front == Some(Front::Title) => self
                .title_buttons
                .iter()
                .find(|(_, r)| inside(*r, self.pointer))
                .map(|(b, _)| {
                    self.ui.string(match b {
                        TitleButton::Intro => "titleintro",
                        TitleButton::Play => "titleplay",
                        TitleButton::Restore => "titlerestore",
                        TitleButton::Quit => "titlequit",
                    })
                }),
            _ => None,
        };
        self.set_tip(tip_text, now);
        if let Some((text, since, delay)) = &self.tip
            && now - since >= *delay
            && self.press.is_none()
        {
            self.ui.tooltip(&mut overlay, text, self.pointer, size);
        }

        // The cursor: waiting while the story plays, lit over what can be
        // used (the arrow of a way out), or the plain pointer.
        let at = self.pointer;
        let over_button = menu_verb.is_some()
            || under.is_some_and(|b| b != Button::Panel)
            || (self.world.map.is_some() && self.map_buttons.iter().any(|b| inside(b.1, at)))
            || (self.world.binocs.is_some() && self.binocs_buttons.iter().any(|b| inside(b.1, at)))
            || self.gps_rects.is_some_and(|(_, power)| inside(power, at))
            || (self.front == Some(Front::Title)
                && self.title_buttons.iter().any(|(_, r)| inside(*r, self.pointer)));
        let story_busy = !self.world.idle()
            && self.menu.is_none()
            && self.bar.is_none()
            && !self.modal()
            && self.front.is_none()
            && self.card.is_none();
        let hover = self.hover.clone();
        if hover != self.last_hover {
            self.blue = hover.is_some() && self.last_hover.is_some() && !self.blue;
            self.last_hover = hover.clone();
        }
        let want = if story_busy {
            ("C_WAIT".to_string(), false)
        } else if let Some(cursor) = self.room_cursor {
            (cursor.to_string(), true)
        } else if over_button {
            ("C_ZOOM".to_string(), true)
        } else if let Some(noun) = &hover {
            let own = self
                .world
                .object_verb(noun)
                .and_then(|v| self.ui.verb_cursor(v));
            let lit = if self.blue { "C_ZOOM_2" } else { "C_ZOOM" };
            (own.unwrap_or_else(|| lit.to_string()), true)
        } else {
            ("C_POINT".to_string(), true)
        };
        if want != self.cursor_want {
            self.cursor_want = want;
            self.cursor_since = now;
        }
        if let Some((text, until)) = &self.notice
            && (self.modal() || self.front.is_some())
            && now < *until
        {
            self.ui.notice(&mut overlay, text, size);
        }

        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f)
            | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(self.renderer.device(), &self.config);
                self.overlay = overlay;
                return;
            }
            _ => {
                self.overlay = overlay;
                return;
            }
        };
        let view = frame.texture.create_view(&Default::default());
        if full_screen
            || self.death
            || self.finished
            || self.save_load.is_some()
            || self.card.is_some()
            || self.world.scene.is_none()
            || self.world.fingerprint.is_some()
            || self.world.sidney.is_some()
        {
            self.renderer.draw_overlay(
                &overlay,
                Target {
                    color: &view,
                    depth: &self.depth,
                    size,
                },
            );
        } else if let (Some(scene), Some(seen)) = (&self.world.scene, self.world.view_scene()) {
            let models = scene.cast.models();
            self.renderer.draw(
                seen,
                &models,
                &overlay,
                &self.world.camera,
                ShadeMode::Lit,
                Target {
                    color: &view,
                    depth: &self.depth,
                    size,
                },
            );
        }
        self.window.pre_present_notify();
        self.renderer.queue().present(frame);
        self.overlay = overlay;
    }
}

impl ApplicationHandler for Game {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gfx.is_some() {
            return;
        }
        match self.open(event_loop) {
            Ok(gfx) => self.gfx = Some(gfx),
            Err(e) => {
                self.error = Some(e);
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        let Some(gfx) = &mut self.gfx else { return };
        match event {
            // As G-Engine: closing the window asks first; asked twice, it goes.
            WindowEvent::CloseRequested => {
                if gfx.popup == Some(Popup::Quit) {
                    event_loop.exit();
                } else {
                    gfx.bar = None;
                    gfx.popup = Some(Popup::Quit);
                }
            }
            WindowEvent::Resized(size) if size.width > 0 && size.height > 0 => {
                gfx.config.width = size.width;
                gfx.config.height = size.height;
                gfx.surface.configure(gfx.renderer.device(), &gfx.config);
                gfx.depth = gfx.renderer.frame_buffers(size.width, size.height);
            }
            WindowEvent::CursorMoved { position, .. } => {
                let to = (position.x as f32, position.y as f32);
                if let Some((slider, track)) = gfx.sliding {
                    let fit = crate::screens::ui_scale(gfx.size());
                    gfx.set_slider(slider, crate::screens::slider_value(track, to.0, fit));
                }
                // Holding the left button and moving drags the camera.
                if let Some(start) = gfx.press
                    && gfx.camera_free()
                {
                    let moved = (to.0 - start.0).hypot(to.1 - start.1);
                    if gfx.dragging {
                        gfx.controls.drag.0 += to.0 - gfx.pointer.0;
                        gfx.controls.drag.1 += to.1 - gfx.pointer.1;
                    } else if moved > camera::DRAG_START {
                        gfx.dragging = true;
                    }
                }
                gfx.pointer = to;
                if gfx.press.is_some() {
                    let at = gfx.sidney_at();
                    if let Some(s) = gfx.world.sidney.as_mut() {
                        s.map_held(at);
                    }
                }
            }
            // A click acts on release, unless the press became a drag.
            WindowEvent::MouseInput { state, button, .. } => match (button, state) {
                // The end goes at any press.
                (_, ElementState::Pressed) if gfx.finished => gfx.quit = true,
                (MouseButton::Left, ElementState::Pressed) => {
                    gfx.press = Some(gfx.pointer);
                    if gfx.world.sidney.is_some()
                        && !gfx.world.state.inventory_shown
                        && !gfx.modal()
                        && gfx.menu.is_none()
                    {
                        let at = gfx.sidney_at();
                        gfx.world.sidney_press(&gfx.renderer, at);
                    }
                    gfx.dragging = false;
                    gfx.held_button = gfx.button_at(gfx.pointer);
                    if let Some(Button::Slider(slider)) = gfx.held_button
                        && let Some((_, track)) =
                            gfx.buttons.iter().rev().find(|(b, _)| *b == Button::Slider(slider))
                    {
                        let track = *track;
                        let fit = crate::screens::ui_scale(gfx.size());
                        gfx.sliding = Some((slider, track));
                        gfx.set_slider(slider, crate::screens::slider_value(track, gfx.pointer.0, fit));
                    }
                    if gfx.front == Some(Front::Title) {
                        gfx.title_held = gfx
                            .title_buttons
                            .iter()
                            .find(|(_, r)| inside(*r, gfx.pointer))
                            .map(|(b, _)| *b);
                    }
                }
                (MouseButton::Left, ElementState::Released) => {
                    if let Some(s) = gfx.world.sidney.as_mut() {
                        s.map_release();
                    }
                    if gfx.sliding.take().is_some() {
                        gfx.settings.store();
                        gfx.held_button = None;
                    } else if gfx.press.take().is_some() && !gfx.dragging {
                        gfx.click();
                    }
                    gfx.press = None;
                    gfx.dragging = false;
                }
                (MouseButton::Right, ElementState::Pressed)
                    if gfx.modal()
                        || gfx.world.showing.is_some()
                        || gfx.front.is_some()
                        || gfx.card.is_some() => {}
                (MouseButton::Right, ElementState::Pressed) if gfx.menu.is_some() => gfx.close_menu(),
                // On SIDNEY's map, it lets go of the shape picked.
                (MouseButton::Right, ElementState::Pressed)
                    if gfx.bar.is_none()
                        && gfx.world.sidney.as_ref().is_some_and(|s| s.map_showing()) =>
                {
                    if let Some(s) = gfx.world.sidney.as_mut() {
                        s.map_deselect();
                    }
                }
                // As G-Engine, a right click brings the option bar up where
                // the pointer is, or puts it away.
                (MouseButton::Right, ElementState::Pressed) => {
                    gfx.bar = match gfx.bar {
                        Some(_) => None,
                        None => Some(OptionBar::new(gfx.pointer)),
                    };
                }
                _ => {}
            },
            // Typing a save's name.
            WindowEvent::KeyboardInput { event, .. }
                if event.state == ElementState::Pressed
                    && gfx.popup.is_none()
                    && gfx.save_load.as_ref().is_some_and(|s| s.typing.is_some()) =>
            {
                if let PhysicalKey::Code(code) = event.physical_key {
                    gfx.type_name(code, event.text.as_deref());
                }
            }
            // Typing into SIDNEY's search box.
            WindowEvent::KeyboardInput { event, .. }
                if event.state == ElementState::Pressed
                    && !gfx.world.state.inventory_shown
                    && gfx.world.sidney.as_ref().is_some_and(|s| s.wants_text()) =>
            {
                match event.physical_key {
                    PhysicalKey::Code(KeyCode::Backspace) => {
                        if let Some(s) = gfx.world.sidney.as_mut() {
                            s.backspace();
                        }
                    }
                    PhysicalKey::Code(KeyCode::Enter | KeyCode::NumpadEnter) => gfx
                        .world
                        .sidney_click(&gfx.renderer, crate::sidney::Hit::SearchGo),
                    PhysicalKey::Code(KeyCode::Escape) => gfx
                        .world
                        .sidney_click(&gfx.renderer, crate::sidney::Hit::MainMenu),
                    PhysicalKey::Code(KeyCode::PageDown | KeyCode::ArrowDown) => {
                        if let Some(s) = gfx.world.sidney.as_mut() {
                            s.scroll(60.0);
                        }
                    }
                    PhysicalKey::Code(KeyCode::PageUp | KeyCode::ArrowUp) => {
                        if let Some(s) = gfx.world.sidney.as_mut() {
                            s.scroll(-60.0);
                        }
                    }
                    _ => {
                        if let (Some(s), Some(text)) =
                            (gfx.world.sidney.as_mut(), event.text.as_ref())
                        {
                            s.type_text(text);
                        }
                    }
                }
            }
            WindowEvent::MouseWheel { delta, .. } if gfx.save_load.is_some() => {
                let dy = match delta {
                    winit::event::MouseScrollDelta::LineDelta(_, y) => y,
                    winit::event::MouseScrollDelta::PixelDelta(p) => p.y as f32 / 20.0,
                };
                if let Some(screen) = &mut gfx.save_load {
                    screen.scroll_by(if dy > 0.0 { -1 } else { 1 });
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                if let Some(s) = gfx.world.sidney.as_mut() {
                    let dy = match delta {
                        winit::event::MouseScrollDelta::LineDelta(_, y) => y * 40.0,
                        winit::event::MouseScrollDelta::PixelDelta(p) => p.y as f32,
                    };
                    s.scroll(-dy);
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let down = event.state == ElementState::Pressed;
                let PhysicalKey::Code(code) = event.physical_key else {
                    return;
                };
                let c = &mut gfx.controls;
                match code {
                    KeyCode::KeyW | KeyCode::ArrowUp => c.forward = down,
                    KeyCode::KeyS | KeyCode::ArrowDown => c.back = down,
                    KeyCode::KeyA | KeyCode::ArrowLeft => c.left = down,
                    KeyCode::KeyD | KeyCode::ArrowRight => c.right = down,
                    KeyCode::ShiftLeft | KeyCode::ShiftRight => c.pitch = down,
                    KeyCode::ControlLeft | KeyCode::ControlRight => c.pan = down,
                    KeyCode::AltLeft | KeyCode::AltRight => c.fast = down,
                    _ => {}
                }
                if !down || event.repeat {
                    return;
                }
                // Fullscreen and back, anywhere: F11, or Alt and Enter.
                if code == KeyCode::F11 || (code == KeyCode::Enter && gfx.controls.fast) {
                    let full = gfx.window.fullscreen().is_some();
                    gfx.window.set_fullscreen(
                        (!full).then_some(winit::window::Fullscreen::Borderless(None)),
                    );
                    return;
                }
                if gfx.modal() {
                    gfx.screen_key(code);
                    return;
                }
                // Any key puts the option bar away, and does its own work
                // too; Escape only that.
                if gfx.bar.take().is_some() && code == KeyCode::Escape {
                    return;
                }
                // A movie is skipped, as G-Engine's Escape (and Space and
                // Enter here); nothing else listens under it.
                if gfx.world.showing.is_some() {
                    if matches!(code, KeyCode::Escape | KeyCode::Space | KeyCode::Enter) {
                        gfx.world.stop_movie(&gfx.renderer);
                    }
                    return;
                }
                // The title screen's buttons have keys.
                if gfx.front == Some(Front::Title) {
                    let button = match code {
                        KeyCode::KeyI => Some(TitleButton::Intro),
                        KeyCode::KeyP => Some(TitleButton::Play),
                        KeyCode::KeyR => Some(TitleButton::Restore),
                        KeyCode::KeyQ => Some(TitleButton::Quit),
                        _ => None,
                    };
                    if let Some(button) = button {
                        gfx.title_press(button);
                    }
                    return;
                }
                // The title card's keys, while it has buttons: C or Escape
                // (and Space or Enter) go on, S saves, as G-Engine.
                if gfx.card.is_some() {
                    if gfx.card_until.is_none() {
                        match code {
                            KeyCode::KeyC | KeyCode::Escape | KeyCode::Space | KeyCode::Enter => {
                                gfx.continue_timeblock()
                            }
                            KeyCode::KeyS => gfx.open_save_load(true),
                            _ => {}
                        }
                    }
                    return;
                }
                match event.physical_key {
                    PhysicalKey::Code(KeyCode::Escape) => {
                        if gfx.menu.is_some() {
                            gfx.close_menu();
                        } else if gfx.world.sidney.is_some() && !gfx.world.state.inventory_shown {
                            let hit = match gfx.world.sidney.as_ref().map(|s| &s.screen) {
                                Some(crate::sidney::Screen::Desktop) => crate::sidney::Hit::Exit,
                                _ => crate::sidney::Hit::MainMenu,
                            };
                            gfx.world.sidney_click(&gfx.renderer, hit);
                        } else if gfx.world.fingerprint.is_some() {
                            gfx.world
                                .fingerprint_events(vec![crate::fingerprint::Event::Close]);
                        } else if gfx.world.map.is_some() {
                            gfx.world.map_cancel();
                        } else if gfx.world.state.inventory_shown {
                            gfx.world.state.inventory_shown = false;
                        } else {
                            // As the original's Esc: skips what is playing,
                            // or with nothing playing, the walks under way.
                            gfx.world.skip_action(&gfx.renderer);
                        }
                    }
                    // Ctrl+Q asks to quit, as the original.
                    PhysicalKey::Code(KeyCode::KeyQ) if gfx.controls.pan => {
                        gfx.popup = Some(Popup::Quit);
                    }
                    // P stops the story until P again, as G-Engine.
                    PhysicalKey::Code(KeyCode::KeyP | KeyCode::Pause) => {
                        gfx.paused = true
                    }
                    PhysicalKey::Code(KeyCode::F1 | KeyCode::KeyH) => {
                        gfx.help = Some(0)
                    }
                    // Space moves things on; with nothing playing it levels
                    // the camera, as in G-Engine.
                    PhysicalKey::Code(KeyCode::Space) if gfx.camera_free() => {
                        gfx.world.camera.pitch = 0.0;
                    }
                    PhysicalKey::Code(KeyCode::Space | KeyCode::Period) => gfx.world.skip(),
                    PhysicalKey::Code(KeyCode::F5) => gfx.quick_save(),
                    PhysicalKey::Code(KeyCode::F9) => gfx.quick_load(),
                    PhysicalKey::Code(
                        k @ (KeyCode::Digit1
                        | KeyCode::Digit2
                        | KeyCode::Digit3
                        | KeyCode::Digit4
                        | KeyCode::Digit5
                        | KeyCode::Digit6
                        | KeyCode::Digit7
                        | KeyCode::Digit8
                        | KeyCode::Digit9),
                    ) if gfx.camera_free() => {
                        gfx.room_camera(k as usize - KeyCode::Digit1 as usize);
                    }
                    PhysicalKey::Code(KeyCode::KeyI | KeyCode::Tab) if gfx.menu.is_none() => {
                        gfx.world.state.inventory_shown = !gfx.world.state.inventory_shown;
                        gfx.inventory.closeup = None;
                    }
                    _ => {}
                }
            }
            WindowEvent::RedrawRequested => gfx.redraw(),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(gfx) = &mut self.gfx {
            if gfx.quit {
                event_loop.exit();
                return;
            }
            gfx.show_cursor(event_loop);
            gfx.window.request_redraw();
        }
    }
}
