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

use crate::play::Options;
use crate::ui::{Menu, Ui, inside};
use crate::world::{Pick, World};

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
    hover: Option<String>,
    pointer: (f32, f32),
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
            hover: None,
            pointer: (0.0, 0.0),
            started: Instant::now(),
            overlay: Overlay::default(),
        })
    }
}

impl Gfx {
    fn size(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
    }

    fn click(&mut self) {
        let size = self.size();
        if let Some(menu) = self.menu.take() {
            let slots = Ui::menu_slots(&menu, size);
            if let Some(i) = slots.iter().position(|s| inside(*s, self.pointer)) {
                let rule = menu.rules[i].clone();
                self.world.act(&self.renderer, rule);
            }
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

        self.hover = None;
        if self.menu.is_none()
            && self.world.idle()
            && let Pick::Noun(noun) =
                self.world
                    .pick(self.pointer.0, self.pointer.1, size.0, size.1)
        {
            self.hover = Some(noun);
        }

        let mut overlay = std::mem::take(&mut self.overlay);
        overlay.clear();
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
                gfx.pointer = (position.x as f32, position.y as f32)
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button,
                ..
            } => match button {
                MouseButton::Left => gfx.click(),
                MouseButton::Right => gfx.menu = None,
                _ => {}
            },
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                match event.physical_key {
                    PhysicalKey::Code(KeyCode::Escape) => {
                        if gfx.menu.take().is_none() {
                            event_loop.exit();
                        }
                    }
                    PhysicalKey::Code(KeyCode::Space | KeyCode::Period) => gfx.world.skip(),
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
