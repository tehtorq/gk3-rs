//! `gk3 play`: the game.
//!
//! With `--do`, `--seconds` or `--png` it runs without a window: enters the
//! location, lets the story run until the player is free, performs each
//! `--do NOUN:VERB` in turn, and reports what happened. That is how the game
//! is tested. Without them it opens a window.

use std::process::ExitCode;

use gk3_barn::Library;
use gk3_formats::png;
use gk3_render::{Renderer, ShadeMode};

use crate::world::World;

/// The longest the story is let run on its own before giving up waiting.
const PATIENCE: f64 = 300.0;
const STEP: f64 = 1.0 / 30.0;

pub struct Options {
    pub location: String,
    pub time: String,
    pub actions: Vec<(String, String)>,
    pub seconds: f64,
    pub png: Option<String>,
    pub quiet: bool,
    pub list: bool,
    pub view: Option<[f32; 5]>,
    /// Show the inventory (and with `--closeup`, one item) in the picture.
    pub inventory: bool,
    pub closeup: Option<String>,
    /// A saved game to start from, and one to write at the end (headless).
    pub load: Option<String>,
    pub save: Option<String>,
}

pub fn parse(args: &[String]) -> Result<(Options, bool), String> {
    let mut o = Options {
        location: "R25".into(),
        time: "110A".into(),
        actions: Vec::new(),
        seconds: 0.0,
        png: None,
        quiet: false,
        list: false,
        view: None,
        inventory: false,
        closeup: None,
        load: None,
        save: None,
    };
    let mut headless = false;
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let mut value = || it.next().cloned().ok_or(format!("{arg} needs a value"));
        match arg.as_str() {
            "--time" => o.time = value()?.to_ascii_uppercase(),
            "--do" => {
                let v = value()?;
                let (noun, verb) = v.split_once(':').ok_or("--do is NOUN:VERB")?;
                o.actions
                    .push((noun.to_ascii_uppercase(), verb.to_ascii_uppercase()));
                headless = true;
            }
            "--seconds" => {
                o.seconds = value()?.parse().map_err(|_| "--seconds takes a number")?;
                headless = true;
            }
            "--png" => {
                o.png = Some(value()?);
                headless = true;
            }
            "--quiet" => o.quiet = true,
            "--view" => {
                let v = value()?;
                let n: Vec<f32> = v.split(',').filter_map(|x| x.trim().parse().ok()).collect();
                o.view = Some(n.try_into().map_err(|_| "--view is x,y,z,yaw,pitch")?);
            }
            "--inventory" => {
                o.inventory = true;
                headless = true;
            }
            "--closeup" => {
                o.closeup = Some(value()?.to_ascii_uppercase());
                o.inventory = true;
                headless = true;
            }
            "--load" => o.load = Some(value()?),
            "--save" => {
                o.save = Some(value()?);
                headless = true;
            }
            "--list" => {
                o.list = true;
                headless = true;
            }
            other if !other.starts_with('-') => o.location = other.to_ascii_uppercase(),
            other => return Err(format!("unknown option {other}")),
        }
    }
    Ok((o, headless))
}

pub fn run(lib: Library, args: &[String]) -> Result<ExitCode, String> {
    let (options, headless) = parse(args)?;
    if headless {
        headless_run(&lib, &options)
    } else {
        crate::window::run(lib, options)
    }
}

/// Enters the location asked for, or the one a saved game was in.
pub fn start(world: &mut World, renderer: &Renderer, o: &Options) -> Result<(), String> {
    match &o.load {
        Some(name) => world.load(renderer, &crate::saves::read(name)?),
        None => world.enter(renderer, &o.location),
    }
}

/// Runs the world until the player is free (or `limit` passes).
fn settle(world: &mut World, renderer: &Renderer, limit: f64) {
    let start = world.clock;
    while !world.idle() && world.clock - start < limit {
        let t = world.clock + STEP;
        world.update(renderer, t);
    }
}

fn headless_run(lib: &Library, o: &Options) -> Result<ExitCode, String> {
    let (device, queue, _) = gk3_render::headless()?;
    let renderer = Renderer::new(device, queue, gk3_render::OFFSCREEN_FORMAT);
    let mut world = World::new(lib, &o.time, gk3_audio::Audio::silent("headless"));
    start(&mut world, &renderer, o)?;
    settle(&mut world, &renderer, PATIENCE);
    // Once settled, a loaded game puts the player back.
    let t = world.clock;
    world.update(&renderer, t);

    for (i, (noun, verb)) in o.actions.iter().enumerate() {
        // Using an item means picking it up from the inventory first.
        let ego = world.state.ego.clone();
        if world.state.has_item(&ego, verb) {
            world.state.active_item.insert(ego, verb.clone());
        }
        let mut verbs = world.verbs(noun);
        verbs.extend(world.topics(noun));
        // With --seconds, the last action is caught in progress.
        let last = i + 1 == o.actions.len() && o.seconds > 0.0;
        match verbs.iter().find(|r| r.verb == *verb) {
            Some(rule) => {
                world.act(&renderer, rule.clone());
                if !last {
                    settle(&mut world, &renderer, PATIENCE);
                }
            }
            None => {
                let offered: Vec<_> = verbs.iter().map(|r| r.verb.as_str()).collect();
                world.log.push(format!(
                    "cannot {noun}:{verb}; offered {}",
                    offered.join(", ")
                ));
            }
        }
    }
    let end = world.clock + o.seconds;
    while world.clock < end {
        let t = world.clock + STEP;
        world.update(&renderer, t);
    }

    if let Some(name) = &o.save {
        let path = world
            .save()
            .and_then(|text| crate::saves::write(name, &text))?;
        println!("saved to {}", path.display());
    }
    if o.inventory {
        world.state.inventory_shown = true;
        let ego = world.state.ego.clone();
        let items: Vec<String> = world
            .state
            .inventory
            .get(&ego)
            .map(|i| i.iter().cloned().collect())
            .unwrap_or_default();
        for item in &items {
            let verbs: Vec<String> = world.verbs(item).into_iter().map(|r| r.verb).collect();
            println!("{ego} has {item}: INSPECT, {}", verbs.join(", "));
        }
    }
    if o.list {
        for (noun, verbs) in world.offered() {
            println!("{noun}: {}", verbs.join(", "));
        }
    }
    if !o.quiet {
        for line in &world.log {
            println!("{line}");
        }
    }
    println!(
        "{:.1}s of story; {} flags, {} counts; player {}",
        world.clock,
        world.state.flags.len(),
        world.state.noun_verbs.len(),
        if world.idle() { "free" } else { "still busy" }
    );
    if let (Some(path), Some(scene)) = (&o.png, &world.scene) {
        if let Some([x, y, z, yaw, pitch]) = o.view {
            world.camera = gk3_render::Camera {
                position: glam::Vec3::new(x, y, z),
                yaw: yaw.to_radians(),
                pitch: pitch.to_radians(),
                fov_y: 60f32.to_radians(),
            };
        }
        let models = scene.cast.models();
        // Captions as they stand at the end, drawn as the window would.
        let mut overlay = gk3_render::Overlay::default();
        let mut ui_renderer = renderer;
        let ui = crate::ui::Ui::new(lib, &mut ui_renderer);
        let mut ui = ui;
        if o.inventory {
            let ego = world.state.ego.clone();
            let items: Vec<String> = world
                .state
                .inventory
                .get(&ego)
                .map(|i| i.iter().cloned().collect())
                .unwrap_or_default();
            let state = crate::ui::Inventory {
                open: true,
                items,
                closeup: o.closeup.clone(),
            };
            ui.inventory(
                lib,
                &mut ui_renderer,
                &mut overlay,
                &state,
                (-1.0, -1.0),
                (1280, 720),
            );
        }
        ui.captions(&mut overlay, &world, (1280, 720));
        let image = ui_renderer.render_image(
            &scene.gpu,
            &models,
            &overlay,
            &world.camera,
            ShadeMode::Lit,
            (1280, 720),
        )?;
        std::fs::write(path, png::encode(&image)).map_err(|e| format!("{path}: {e}"))?;
    }
    Ok(ExitCode::SUCCESS)
}
