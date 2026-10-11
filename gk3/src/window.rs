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
use crate::ui::{Inventory, Menu, Ui, inside};
use crate::world::{Pick, World};
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

struct Gfx {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    renderer: Renderer,
    depth: wgpu::TextureView,
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
    started: Instant,
    overlay: Overlay,
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

        let mut renderer = Renderer::new(device, queue, format);
        let depth = renderer.depth_view(config.width, config.height);
        let ui = Ui::new(self.lib, &mut renderer);
        let audio = gk3_audio::Audio::open("Gabriel Knight 3");
        if audio.is_silent() {
            eprintln!("no sound: {}", audio.status);
        }
        let mut world = World::new(self.lib, &self.options.time, audio);
        world.enter(&renderer, &self.options.location)?;
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
            started: Instant::now(),
            overlay: Overlay::default(),
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
        self.world.idle() && self.menu.is_none() && !self.world.state.inventory_shown
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
        self.inventory.open = self.world.state.inventory_shown;
        if self.inventory.closeup.take().is_some() {
            return;
        }
        if let Some(menu) = self.menu.take() {
            let slots = Ui::menu_slots(&menu, size);
            match slots.iter().position(|s| inside(*s, self.pointer)) {
                Some(i) => {
                    let rule = menu.rules[i].clone();
                    if rule.source == "UI" && rule.verb == "INSPECT" {
                        self.inventory.closeup = Some(rule.noun);
                    } else if rule.source == "UI" && rule.verb == "PICKUP" {
                        let ego = self.world.state.ego.clone();
                        self.world.state.active_item.insert(ego, rule.noun);
                    } else {
                        self.world.act(&self.renderer, rule);
                    }
                }
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
                    rules.extend(verbs);
                    rules.insert(after_look + 1, screen("PICKUP"));
                    self.menu = Some(Menu {
                        noun: item,
                        rules,
                        topics: false,
                        x: self.pointer.0,
                        y: self.pointer.1,
                    });
                }
                Some(_) => self.world.skip(),
                None => self.world.state.inventory_shown = false,
            }
            return;
        }
        if inside(Ui::inventory_button(size), self.pointer) && self.world.idle() {
            self.world.state.inventory_shown = true;
            return;
        }
        if !self.world.idle() {
            // A click while someone is talking moves the conversation on.
            self.world.skip();
            return;
        }
        match self
            .world
            .pick(self.pointer.0, self.pointer.1, size.0, size.1)
        {
            Pick::Noun(noun) => {
                let rules = self.world.verbs(&noun);
                if rules.len() == 1 {
                    let rule = rules[0].clone();
                    self.world.act(&self.renderer, rule);
                } else if !rules.is_empty() {
                    self.menu = Some(Menu {
                        noun,
                        rules,
                        topics: false,
                        x: self.pointer.0,
                        y: self.pointer.1,
                    });
                }
            }
            Pick::Floor(point) => {
                self.world.walk_ego(point, None);
            }
            Pick::Nothing => {}
        }
    }

    fn redraw(&mut self) {
        let now = self.started.elapsed().as_secs_f64();
        self.world.update(&self.renderer, now);
        let size = self.size();
        let dt = (now - self.last_frame).clamp(0.0, 0.1) as f32;
        self.last_frame = now;
        if self.camera_free() {
            self.controls.apply(&mut self.world.camera, dt);
        } else {
            self.controls.drag = (0.0, 0.0);
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
                self.menu = Some(Menu {
                    noun,
                    rules,
                    topics: true,
                    x: size.0 as f32 / 2.0,
                    y: size.1 as f32 * 0.75,
                });
            }
        }

        self.hover = None;
        if self.menu.is_none()
            && !self.inventory.open
            && !self.dragging
            && self.world.idle()
            && let Pick::Noun(noun) =
                self.world
                    .pick(self.pointer.0, self.pointer.1, size.0, size.1)
        {
            self.hover = Some(noun);
        }

        let mut overlay = std::mem::take(&mut self.overlay);
        overlay.clear();
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
        } else {
            self.ui
                .draw_inventory_button(&mut overlay, self.pointer, size);
        }
        self.ui.captions(&mut overlay, &self.world, size);
        if let Some(noun) = &self.hover {
            self.ui
                .label(&mut overlay, noun, self.pointer.0, self.pointer.1, size);
        }
        if let Some(menu) = &self.menu {
            self.ui.menu(
                self.world.lib,
                &mut self.renderer,
                &mut overlay,
                menu,
                self.pointer,
                size,
            );
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
        if let Some(scene) = &self.world.scene {
            let models = scene.cast.models();
            self.renderer.draw(
                &scene.gpu,
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
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) if size.width > 0 && size.height > 0 => {
                gfx.config.width = size.width;
                gfx.config.height = size.height;
                gfx.surface.configure(gfx.renderer.device(), &gfx.config);
                gfx.depth = gfx.renderer.depth_view(size.width, size.height);
            }
            WindowEvent::CursorMoved { position, .. } => {
                let to = (position.x as f32, position.y as f32);
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
            }
            // A click acts on release, unless the press became a drag.
            WindowEvent::MouseInput { state, button, .. } => match (button, state) {
                (MouseButton::Left, ElementState::Pressed) => {
                    gfx.press = Some(gfx.pointer);
                    gfx.dragging = false;
                }
                (MouseButton::Left, ElementState::Released) => {
                    if gfx.press.take().is_some() && !gfx.dragging {
                        gfx.click();
                    }
                    gfx.dragging = false;
                }
                (MouseButton::Right, ElementState::Pressed) => gfx.close_menu(),
                _ => {}
            },
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
                match event.physical_key {
                    PhysicalKey::Code(KeyCode::Escape) => {
                        if gfx.menu.is_some() {
                            gfx.close_menu();
                        } else if gfx.world.state.inventory_shown {
                            gfx.world.state.inventory_shown = false;
                        } else {
                            event_loop.exit();
                        }
                    }
                    // Space moves things on; with nothing playing it levels
                    // the camera, as in G-Engine.
                    PhysicalKey::Code(KeyCode::Space) if gfx.camera_free() => {
                        gfx.world.camera.pitch = 0.0;
                    }
                    PhysicalKey::Code(KeyCode::Space | KeyCode::Period) => gfx.world.skip(),
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

    fn about_to_wait(&mut self, _: &ActiveEventLoop) {
        if let Some(gfx) = &self.gfx {
            gfx.window.request_redraw();
        }
    }
}
