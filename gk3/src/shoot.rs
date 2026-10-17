//! `gk3 rooms` and `gk3 render`: list rooms, and draw one to a PNG without a
//! window.

use std::path::Path;
use std::process::ExitCode;

use gk3_barn::Library;
use gk3_formats::png;
use gk3_render::{Camera, Renderer, ShadeMode};

use crate::cast::Cast;
use crate::room::{Catalog, Room, RoomCamera};

/// Every character's markers and walk, from `CHARACTERS.TXT`.
pub fn characters(
    lib: &Library,
) -> std::collections::HashMap<String, gk3_game::character::Character> {
    lib.read("CHARACTERS.TXT")
        .map(|b| gk3_game::character::parse(&b))
        .unwrap_or_default()
}

/// The room's people and props, run forward `seconds` in fixed steps.
pub fn cast(lib: &Library, renderer: &Renderer, room: &Room, seconds: f64) -> Option<Cast> {
    let sif = room.sif.as_ref()?;
    let characters = characters(lib);
    let faces = lib
        .read("FACES.TXT")
        .map(|b| crate::faces::parse(&b, &characters))
        .unwrap_or_default();
    let mut cast = Cast::load(lib, renderer, sif, &characters, &faces);
    let steps = (seconds * 30.0).round() as usize;
    for step in 0..=steps {
        cast.update(lib, step as f64 / 30.0);
    }
    cast.upload(renderer);
    Some(cast)
}

pub fn rooms(lib: &Library) -> Result<ExitCode, String> {
    let catalog = Catalog::new(lib);
    for scene in &catalog.scenes {
        println!("{scene}");
    }
    Ok(ExitCode::SUCCESS)
}

/// Where a camera starts in a room: the location's default camera, else its
/// first, else outside the room's bounds looking in.
pub fn start_camera(room: &Room, name: Option<&str>) -> Result<Camera, String> {
    let named = match name {
        Some(name) => Some(
            room.cameras
                .iter()
                .find(|c| c.name.eq_ignore_ascii_case(name))
                .ok_or_else(|| {
                    let names: Vec<_> = room.cameras.iter().map(|c| c.name.as_str()).collect();
                    format!(
                        "{} has no camera {name}; it has {}",
                        room.name,
                        names.join(", ")
                    )
                })?,
        ),
        None => room
            .cameras
            .iter()
            .find(|c| c.default)
            .or(room.cameras.first()),
    };
    Ok(match named {
        Some(c) => from_room_camera(c),
        None => {
            let (min, max) = room.mesh.bounds;
            let center = glam::Vec3::from(min).lerp(glam::Vec3::from(max), 0.5);
            let depth = max[2] - min[2];
            Camera {
                position: center - glam::Vec3::Z * depth,
                yaw: 0.0,
                pitch: 0.3,
                fov_y: 60f32.to_radians(),
            }
        }
    })
}

pub fn from_room_camera(c: &RoomCamera) -> Camera {
    Camera {
        position: c.position.into(),
        yaw: c.yaw.to_radians(),
        pitch: c.pitch.to_radians(),
        fov_y: c.fov.to_radians(),
    }
}

pub fn parse_mode(text: &str) -> Result<ShadeMode, String> {
    match text {
        "lit" => Ok(ShadeMode::Lit),
        "textures" => Ok(ShadeMode::TexturesOnly),
        "lightmaps" => Ok(ShadeMode::LightmapsOnly),
        other => Err(format!(
            "unknown mode '{other}': use lit, textures or lightmaps"
        )),
    }
}

/// Renders a room offscreen: `gk3 render ROOM [--camera NAME] [--size WxH]
/// [--mode lit|textures|lightmaps] [--png FILE]`. `gk3 render all` renders
/// every room into a directory (`--png` names it).
pub fn render(lib: &Library, args: &[String]) -> Result<ExitCode, String> {
    let mut room_name = None;
    let mut camera_name = None;
    let mut size = (1280u32, 720u32);
    let mut mode = ShadeMode::Lit;
    let mut out = None;
    let mut seconds = 0.0;
    let mut view = None;
    let mut time = None;
    // Samples a pixel: smoothed edges with --msaa, as a window on a GPU.
    let mut samples = 1;
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let mut value = || it.next().cloned().ok_or(format!("{arg} needs a value"));
        match arg.as_str() {
            "--camera" => camera_name = Some(value()?),
            "--size" => {
                let v = value()?;
                let (w, h) = v.split_once('x').ok_or("--size is WIDTHxHEIGHT")?;
                size = (
                    w.parse().map_err(|_| "bad width")?,
                    h.parse().map_err(|_| "bad height")?,
                );
            }
            "--mode" => mode = parse_mode(&value()?)?,
            "--png" => out = Some(value()?),
            "--msaa" => samples = gk3_render::MSAA_SAMPLES,
            "--view" => {
                let v = value()?;
                let n: Vec<f32> = v
                    .split(',')
                    .map(|x| x.trim().parse())
                    .collect::<Result<_, _>>()
                    .map_err(|_| "--view is x,y,z,yaw,pitch")?;
                let [x, y, z, yaw, pitch] = n[..] else {
                    return Err("--view is x,y,z,yaw,pitch".into());
                };
                view = Some(Camera {
                    position: glam::Vec3::new(x, y, z),
                    yaw: yaw.to_radians(),
                    pitch: pitch.to_radians(),
                    fov_y: 60f32.to_radians(),
                });
            }
            "--time" => time = Some(value()?.to_ascii_uppercase()),
            "--seconds" => seconds = value()?.parse().map_err(|_| "--seconds takes a number")?,
            other if room_name.is_none() => room_name = Some(other.to_string()),
            other => return Err(format!("unexpected argument '{other}'")),
        }
    }
    let room_name = room_name.ok_or("render needs a room, e.g. gk3 render R25")?;

    let mut catalog = Catalog::new(lib);
    catalog.time = time;
    if room_name.eq_ignore_ascii_case("all") {
        let dir = out.as_deref().unwrap_or("rooms");
        return render_all(lib, &catalog, mode, size, samples, dir);
    }
    let scene = catalog
        .resolve(lib, &room_name)
        .ok_or_else(|| format!("no room called {room_name}; gk3 rooms lists them"))?;
    let room = catalog.load(lib, &scene)?;
    for w in &room.warnings {
        eprintln!("warning: {w}");
    }
    let camera = match view {
        Some(v) => v,
        None => start_camera(&room, camera_name.as_deref())?,
    };

    let (device, queue, gpu) = gk3_render::headless()?;
    let renderer =
        Renderer::with_samples(device, queue, gk3_render::OFFSCREEN_FORMAT, samples);
    let gpu_scene = renderer.upload(&room.mesh);
    let cast = cast(lib, &renderer, &room, seconds);
    if let Some(cast) = &cast {
        for w in &cast.warnings {
            eprintln!("warning: {w}");
        }
        println!("cast: {}", cast.describe());
    }
    let models = cast.as_ref().map(|c| c.models()).unwrap_or_default();
    let image = renderer.render_image(
        &gpu_scene,
        &models,
        &gk3_render::Overlay::default(),
        &camera,
        mode,
        size,
    )?;

    let out = out.unwrap_or_else(|| format!("{scene}.png"));
    std::fs::write(Path::new(&out), png::encode(&image)).map_err(|e| format!("{out}: {e}"))?;
    println!(
        "{scene} ({} triangles, {} textures) on {gpu} -> {out}",
        room.mesh.vertices.len() / 3,
        room.mesh.textures.len()
    );
    Ok(ExitCode::SUCCESS)
}

/// Renders every scene asset from its starting camera into `dir`.
fn render_all(
    lib: &Library,
    catalog: &Catalog,
    mode: ShadeMode,
    size: (u32, u32),
    samples: u32,
    dir: &str,
) -> Result<ExitCode, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{dir}: {e}"))?;
    let (device, queue, gpu) = gk3_render::headless()?;
    let renderer =
        Renderer::with_samples(device, queue, gk3_render::OFFSCREEN_FORMAT, samples);
    let start = std::time::Instant::now();
    let mut failures = Vec::new();
    let mut skipped = Vec::new();
    let mut warned = 0;
    let mut figures = 0;
    let mut cast_warnings: Vec<String> = Vec::new();
    for scene in &catalog.scenes {
        let result = catalog.load(lib, scene).and_then(|room| {
            if !room.warnings.is_empty() {
                warned += 1;
                println!("{scene}: {}", room.warnings.join("; "));
            }
            let camera = start_camera(&room, None)?;
            let gpu_scene = renderer.upload(&room.mesh);
            let cast = cast(lib, &renderer, &room, 0.0);
            if let Some(cast) = &cast {
                figures += cast.models().len();
                cast_warnings.extend(cast.warnings.iter().map(|w| format!("{scene}: {w}")));
            }
            let models = cast.as_ref().map(|c| c.models()).unwrap_or_default();
            let image = renderer.render_image(
                &gpu_scene,
                &models,
                &gk3_render::Overlay::default(),
                &camera,
                mode,
                size,
            )?;
            let path = Path::new(dir).join(format!("{scene}.png"));
            std::fs::write(&path, png::encode(&image)).map_err(|e| e.to_string())
        });
        match result {
            Ok(()) => {}
            Err(e) if !catalog.is_used(scene) => skipped.push(format!("{scene} ({e})")),
            Err(e) => failures.push(format!("{scene}: {e}")),
        }
    }
    println!(
        "Rendered {} of {} rooms on {gpu} into {dir} in {:.1}s; {warned} with warnings",
        catalog.scenes.len() - failures.len() - skipped.len(),
        catalog.scenes.len(),
        start.elapsed().as_secs_f64()
    );
    println!(
        "{figures} people and props placed; {} could not be",
        cast_warnings.len()
    );
    cast_warnings.sort();
    cast_warnings.dedup();
    for w in cast_warnings.iter().take(
        if crate::VERBOSE.load(std::sync::atomic::Ordering::Relaxed) {
            usize::MAX
        } else {
            12
        },
    ) {
        println!("  {w}");
    }
    if !skipped.is_empty() {
        println!("Skipped, unused by any location: {}", skipped.join(", "));
    }
    crate::report_failures(&failures)
}
