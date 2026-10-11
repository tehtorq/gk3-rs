//! `gk3 view`: fly through the rooms in a window.

use std::collections::HashSet;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Instant;

use gk3_barn::Library;
use gk3_render::{Camera, GpuScene, Renderer, ShadeMode};
use glam::Vec3;
use winit::application::ApplicationHandler;
use winit::event::{DeviceEvent, ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

use crate::room::{Catalog, Room};
use crate::shoot;

const HELP: &str = "\
WASD/arrows move, Q/E down/up, Shift faster; drag with a mouse button to look.
Tab/Shift+Tab next/previous room, C next camera, M shading, Esc quit.";

/// Units per second. Rooms are a few hundred units across; the town is
/// thousands.
const SPEED: f32 = 150.0;
const LOOK_SPEED: f32 = 0.004;

pub fn run(lib: Library, args: &[String]) -> Result<ExitCode, String> {
    let catalog = Catalog::new(&lib);
    if catalog.scenes.is_empty() {
        return Err("the archives hold no rooms".into());
    }
    let start = match args.first() {
        Some(name) => catalog
            .resolve(&lib, name)
            .ok_or_else(|| format!("no room called {name}; gk3 rooms lists them"))?,
        None => catalog
            .resolve(&lib, "R25")
            .unwrap_or_else(|| catalog.scenes[0].clone()),
    };
    let index = catalog.scenes.iter().position(|s| *s == start).unwrap_or(0);
    println!("{HELP}");

    let event_loop = EventLoop::new().map_err(|e| format!("cannot open a window: {e}"))?;
    let mut app = Viewer {
        lib,
        catalog,
        index,
        gfx: None,
        error: None,
        held: HashSet::new(),
        looking: false,
        shift: false,
        last_frame: Instant::now(),
    };
    event_loop
        .run_app(&mut app)
        .map_err(|e| format!("window error: {e}"))?;
    match app.error {
        Some(e) => Err(e),
        None => Ok(ExitCode::SUCCESS),
    }
}

struct Viewer {
    lib: Library,
    catalog: Catalog,
    index: usize,
    gfx: Option<Gfx>,
    error: Option<String>,
    held: HashSet<KeyCode>,
    looking: bool,
    shift: bool,
    last_frame: Instant,
}

struct Gfx {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    renderer: Renderer,
    depth: wgpu::TextureView,
    gpu_name: String,
    room: Room,
    scene: GpuScene,
    camera: Camera,
    camera_index: usize,
    mode: ShadeMode,
}

impl Viewer {
    fn fail(&mut self, event_loop: &ActiveEventLoop, message: String) {
        self.error = Some(message);
        event_loop.exit();
    }

    fn open(&mut self, event_loop: &ActiveEventLoop) -> Result<Gfx, String> {
        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title("gk3")
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
        let gpu_name = adapter.get_info().name;
        let (device, queue) = gk3_render::request_device(&adapter)?;

        // GK3 multiplied lightmaps in gamma space, so draw to a non-sRGB
        // format when the surface has one.
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

        let renderer = Renderer::new(device, queue, format);
        let depth = renderer.depth_view(config.width, config.height);
        let (room, scene, camera) = self.load(&renderer, self.index)?;
        let gfx = Gfx {
            window,
            surface,
            config,
            renderer,
            depth,
            gpu_name,
            room,
            scene,
            camera,
            camera_index: 0,
            mode: ShadeMode::Lit,
        };
        gfx.update_title();
        Ok(gfx)
    }

    fn load(&self, renderer: &Renderer, index: usize) -> Result<(Room, GpuScene, Camera), String> {
        let room = self.catalog.load(&self.lib, &self.catalog.scenes[index])?;
        for w in &room.warnings {
            eprintln!("{}: {w}", room.name);
        }
        let scene = renderer.upload(&room.mesh);
        let camera = shoot::start_camera(&room, None)?;
        Ok((room, scene, camera))
    }

    fn switch_room(&mut self, step: isize) {
        let count = self.catalog.scenes.len() as isize;
        let next = (self.index as isize + step).rem_euclid(count) as usize;
        let Some(gfx) = &self.gfx else { return };
        match self.load(&gfx.renderer, next) {
            Ok((room, scene, camera)) => {
                self.index = next;
                let gfx = self.gfx.as_mut().unwrap();
                gfx.room = room;
                gfx.scene = scene;
                gfx.camera = camera;
                gfx.camera_index = 0;
                gfx.update_title();
            }
            Err(e) => {
                eprintln!("{}: {e}", self.catalog.scenes[next]);
                // Skip rooms that will not load rather than getting stuck.
                self.index = next;
            }
        }
    }

    fn fly(&mut self, dt: f32) {
        let Some(gfx) = &mut self.gfx else { return };
        let held = |k: &[KeyCode]| k.iter().any(|k| self.held.contains(k));
        let mut motion = Vec3::ZERO;
        if held(&[KeyCode::KeyW, KeyCode::ArrowUp]) {
            motion += gfx.camera.forward();
        }
        if held(&[KeyCode::KeyS, KeyCode::ArrowDown]) {
            motion -= gfx.camera.forward();
        }
        if held(&[KeyCode::KeyD, KeyCode::ArrowRight]) {
            motion += gfx.camera.right();
        }
        if held(&[KeyCode::KeyA, KeyCode::ArrowLeft]) {
            motion -= gfx.camera.right();
        }
        if held(&[KeyCode::KeyE]) {
            motion += Vec3::Y;
        }
        if held(&[KeyCode::KeyQ]) {
            motion -= Vec3::Y;
        }
        let speed = if self.shift { SPEED * 5.0 } else { SPEED };
        gfx.camera.position += motion.normalize_or_zero() * speed * dt;
    }
}

impl Gfx {
    fn update_title(&self) {
        let camera = self
            .room
            .cameras
            .get(self.camera_index)
            .map_or("free".to_string(), |c| c.name.clone());
        let mode = match self.mode {
            ShadeMode::Lit => "lit",
            ShadeMode::TexturesOnly => "textures",
            ShadeMode::LightmapsOnly => "lightmaps",
        };
        self.window.set_title(&format!(
            "gk3 - {} ({}) - camera {camera} - {mode} - {}",
            self.room.name,
            self.room.location.as_deref().unwrap_or("no location"),
            self.gpu_name
        ));
    }

    fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(self.renderer.device(), &self.config);
        self.depth = self.renderer.depth_view(width, height);
    }

    fn redraw(&mut self) {
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            // Not a failure: try again next frame, after reconfiguring if the
            // surface says it changed.
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(self.renderer.device(), &self.config);
                return;
            }
            other => {
                eprintln!("skipped a frame: {other:?}");
                return;
            }
        };
        let view = frame.texture.create_view(&Default::default());
        self.renderer.draw(
            &self.scene,
            &self.camera,
            self.mode,
            &view,
            &self.depth,
            (self.config.width, self.config.height),
        );
        self.window.pre_present_notify();
        self.renderer.queue().present(frame);
    }
}

impl ApplicationHandler for Viewer {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gfx.is_some() {
            return;
        }
        match self.open(event_loop) {
            Ok(gfx) => self.gfx = Some(gfx),
            Err(e) => self.fail(event_loop, e),
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(gfx) = &mut self.gfx {
                    gfx.resize(size.width, size.height);
                }
            }
            WindowEvent::ModifiersChanged(m) => self.shift = m.state().shift_key(),
            WindowEvent::MouseInput { state, button, .. } => {
                if matches!(button, MouseButton::Left | MouseButton::Right) {
                    self.looking = state == ElementState::Pressed;
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let PhysicalKey::Code(key) = event.physical_key else {
                    return;
                };
                if event.state == ElementState::Released {
                    self.held.remove(&key);
                    return;
                }
                if event.repeat {
                    return;
                }
                self.held.insert(key);
                match key {
                    KeyCode::Escape => event_loop.exit(),
                    KeyCode::Tab => self.switch_room(if self.shift { -1 } else { 1 }),
                    KeyCode::KeyC => {
                        if let Some(gfx) = &mut self.gfx
                            && !gfx.room.cameras.is_empty()
                        {
                            gfx.camera_index = (gfx.camera_index + 1) % gfx.room.cameras.len();
                            gfx.camera =
                                shoot::from_room_camera(&gfx.room.cameras[gfx.camera_index]);
                            gfx.update_title();
                        }
                    }
                    KeyCode::KeyM => {
                        if let Some(gfx) = &mut self.gfx {
                            gfx.mode = match gfx.mode {
                                ShadeMode::Lit => ShadeMode::TexturesOnly,
                                ShadeMode::TexturesOnly => ShadeMode::LightmapsOnly,
                                ShadeMode::LightmapsOnly => ShadeMode::Lit,
                            };
                            gfx.update_title();
                        }
                    }
                    _ => {}
                }
            }
            WindowEvent::RedrawRequested => {
                let now = Instant::now();
                let dt = (now - self.last_frame).as_secs_f32().min(0.1);
                self.last_frame = now;
                self.fly(dt);
                if let Some(gfx) = &mut self.gfx {
                    gfx.redraw();
                }
            }
            _ => {}
        }
    }

    fn device_event(&mut self, _: &ActiveEventLoop, _: winit::event::DeviceId, event: DeviceEvent) {
        if let (DeviceEvent::MouseMotion { delta }, true) = (event, self.looking)
            && let Some(gfx) = &mut self.gfx
        {
            gfx.camera.yaw += delta.0 as f32 * LOOK_SPEED;
            gfx.camera.pitch = (gfx.camera.pitch + delta.1 as f32 * LOOK_SPEED).clamp(-1.5, 1.5);
        }
    }

    fn about_to_wait(&mut self, _: &ActiveEventLoop) {
        if let Some(gfx) = &self.gfx {
            gfx.window.request_redraw();
        }
    }
}
