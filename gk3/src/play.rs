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
    /// Stop the story this far into entering, as if caught mid-scene.
    pub during: Option<f64>,
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
    /// Locations to go to after the actions, as a door would take you.
    pub goto: Vec<String>,
    /// Picture the current timeblock's title card instead of the room.
    pub card: bool,
    /// Draw the title screen.
    pub title_screen: bool,
    /// Draw one of the screens around the story: `bar`, `bar-cameras`, `bar-options`,
    /// `bar-sound`, `bar-graphics`, `bar-game`, `save`, `load`, `quit`,
    /// `overwrite`, `death`, `finished`, `paused` or `help` (`help2`, `help3`
    /// for its other pages), or `menu:NOUN` for a noun's verb menu.
    pub screen: Option<String>,
    /// At the end, say where every actor stands.
    pub where_actors: bool,
    /// The picture's size, as a window of that size would draw it.
    pub size: (u32, u32),
    /// Play movies through (at story speed) rather than skip them.
    pub show_movies: bool,
    /// Play this movie (for `--seconds`) and draw where it has got to.
    pub movie: Option<String>,
    /// Buttons to press on the driving map, by id (`PLO`, `RLC`...).
    pub map: Vec<String>,
    /// With the binoculars up, a place to point at and zoom into.
    pub zoom: Option<String>,
    /// With the fingerprint kit up, dust and keep every print.
    pub lift: bool,
    /// Things to do on SIDNEY: `addata=ITEM`, `email=N`, `files`, `exit`.
    pub sid: Vec<String>,
    /// Neither a place nor a time was asked for: a new game.
    pub new_game: bool,
}

pub fn parse(args: &[String]) -> Result<(Options, bool), String> {
    let mut o = Options {
        location: "R25".into(),
        time: "110A".into(),
        actions: Vec::new(),
        seconds: 0.0,
        during: None,
        png: None,
        quiet: false,
        list: false,
        view: None,
        inventory: false,
        closeup: None,
        load: None,
        save: None,
        goto: Vec::new(),
        card: false,
        title_screen: false,
        screen: None,
        where_actors: false,
        size: (1280, 720),
        show_movies: false,
        movie: None,
        map: Vec::new(),
        zoom: None,
        lift: false,
        sid: Vec::new(),
        new_game: true,
    };
    let mut headless = false;
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let mut value = || it.next().cloned().ok_or(format!("{arg} needs a value"));
        match arg.as_str() {
            "--time" => {
                o.time = value()?.to_ascii_uppercase();
                o.new_game = false;
            }
            "--goto" => {
                o.goto.push(value()?.to_ascii_uppercase());
                headless = true;
            }
            "--sid" => {
                o.sid.push(value()?);
                headless = true;
            }
            "--lift" => {
                o.lift = true;
                headless = true;
            }
            "--zoom" => {
                o.zoom = Some(value()?.to_ascii_uppercase());
                headless = true;
            }
            "--map" => {
                o.map.push(value()?.to_ascii_uppercase());
                headless = true;
            }
            "--show-movies" => {
                o.show_movies = true;
                headless = true;
            }
            "--screen" => {
                o.screen = Some(value()?.to_ascii_lowercase());
                headless = true;
            }
            "--title-screen" => {
                o.title_screen = true;
                headless = true;
            }
            "--movie" => {
                o.movie = Some(value()?);
                headless = true;
            }
            "--card" => {
                o.card = true;
                headless = true;
            }
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
            "--during" => {
                o.during = Some(value()?.parse().map_err(|_| "--during takes a number")?);
                headless = true;
            }
            "--png" => {
                o.png = Some(value()?);
                headless = true;
            }
            "--quiet" => o.quiet = true,
            "--where" => o.where_actors = true,
            "--size" => {
                let v = value()?;
                let parsed = v
                    .split_once('x')
                    .and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?)));
                o.size = parsed
                    .filter(|&(w, h): &(u32, u32)| w > 0 && h > 0)
                    .ok_or("--size is WIDTHxHEIGHT")?;
            }
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
            other if !other.starts_with('-') => {
                o.location = other.to_ascii_uppercase();
                o.new_game = false;
            }
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
        None => {
            // Starting somewhere the driving map goes is arriving by it, as
            // the story has it: the rooms put the moped there and the ego
            // gets off it (LHE plays the getting off for any other start).
            if crate::map::PLACES
                .iter()
                .any(|p| p.location.eq_ignore_ascii_case(&o.location))
            {
                world.state.location = "MAP".into();
            }
            world.enter(renderer, &o.location)
        }
    }
}

/// Runs until a condition holds, or patience runs out.
fn run_until(world: &mut World, renderer: &Renderer, done: impl Fn(&World) -> bool) {
    let start = world.outside();
    while !done(world) && world.outside() - start < PATIENCE {
        let t = world.outside() + STEP;
        world.update(renderer, t);
    }
}

/// Runs until nothing is being said.
fn settle_speech(world: &mut World, renderer: &Renderer) {
    let start = world.outside();
    while !world.captions.is_empty() && world.outside() - start < PATIENCE {
        let t = world.outside() + STEP;
        world.update(renderer, t);
    }
    let t = world.outside() + STEP;
    world.update(renderer, t);
}

/// Runs the world until the player is free (or `limit` passes). Title
/// cards are clicked through at once.
fn settle(world: &mut World, renderer: &Renderer, limit: f64) {
    let start = world.outside();
    while !world.idle() && world.outside() - start < limit {
        if world.timeblock_change.is_some() {
            if let Err(e) = world.continue_timeblock(renderer) {
                world.log.push(format!("  cannot start the timeblock: {e}"));
                return;
            }
            continue;
        }
        let t = world.outside() + STEP;
        world.update(renderer, t);
    }
}

fn headless_run(lib: &Library, o: &Options) -> Result<ExitCode, String> {
    let (device, queue, gpu) = gk3_render::headless()?;
    let mut renderer = Renderer::new(device, queue, gk3_render::OFFSCREEN_FORMAT);
    renderer.adapter_name = gpu;
    let mut world = World::new(lib, &o.time, gk3_audio::Audio::silent("headless"));
    world.aspect = o.size.0 as f32 / o.size.1 as f32;
    // Movies are noted in the log, not played, unless asked for.
    world.skip_movies = !o.show_movies;
    start(&mut world, &renderer, o)?;
    settle(&mut world, &renderer, o.during.unwrap_or(PATIENCE));
    // Once settled, a loaded game puts the player back.
    let t = world.outside();
    world.update(&renderer, t);

    // The player's own camera, as if they had moved it there first.
    if let Some([x, y, z, yaw, pitch]) = o.view {
        world.camera = gk3_render::Camera {
            position: glam::Vec3::new(x, y, z),
            yaw: yaw.to_radians(),
            pitch: pitch.to_radians(),
            fov_y: 60f32.to_radians(),
        };
    }
    for (i, (noun, verb)) in o.actions.iter().enumerate() {
        // `CALL:SCRIPT/FUNCTION` runs a script's function, as if an action
        // had: for reaching what the story would only do later.
        if noun == "CALL" {
            let Some((script, function)) = verb.split_once('/') else {
                world.log.push(format!("cannot CALL:{verb}; it is CALL:SCRIPT/FUNCTION"));
                continue;
            };
            if world.call(&renderer, script, function) {
                world.log.push(format!("call {script}/{function}"));
                settle(&mut world, &renderer, PATIENCE);
            } else {
                world.log.push(format!("cannot call {script}/{function}"));
            }
            continue;
        }
        // `PENDULUM:GRAB` and `PENDULUM:DROP` work TE3's pendulum as a
        // player would; `WAIT:SECONDS` lets the story run on.
        if noun == "PENDULUM" {
            let start = world.outside();
            while !world.pendulum_goal(&renderer, verb, STEP) {
                if world.outside() - start > PATIENCE {
                    world.log.push(format!("cannot PENDULUM:{verb}"));
                    break;
                }
            }
            settle(&mut world, &renderer, PATIENCE);
            continue;
        }
        if noun == "BRIDGE" {
            let start = world.outside();
            while !world.bridge_cross(&renderer, STEP) {
                if world.outside() - start > PATIENCE {
                    world.log.push("cannot BRIDGE:CROSS".into());
                    break;
                }
            }
            settle(&mut world, &renderer, PATIENCE);
            continue;
        }
        // `SKIPLINES:0.5` clicks through every line half a second in, from
        // here on.
        // `GIVE:ITEM` puts an item in the ego's inventory.
        if noun == "GIVE" {
            let ego = world.state.ego.clone();
            world.state.give_item(&ego, verb);
            continue;
        }
        // `SKIP:ESC` is the player's Esc: skips what is playing, or walks.
        if noun == "SKIP" {
            world.skip_action(&renderer);
            continue;
        }
        // `GPS:POWER` presses the GPS's power button.
        if noun == "GPS" {
            world.gps_power();
            continue;
        }
        if noun == "SKIPLINES" {
            world.skip_lines_after = verb.parse::<f64>().ok();
            continue;
        }
        if noun == "WAIT" {
            let end = world.outside() + verb.parse::<f64>().unwrap_or(1.0);
            while world.outside() < end {
                let t = world.outside() + STEP;
                world.update(&renderer, t);
            }
            continue;
        }
        // `TILE:D1` points at a tile of TE1's chessboard and jumps to it;
        // `FLOOR:OBJECT` is a click on a piece of the room.
        if noun == "TILE" || noun == "FLOOR" {
            let object = if noun == "TILE" { format!("TE1FLOOR{verb}") } else { verb.clone() };
            world.hover_object(Some(&object));
            if noun == "FLOOR" {
                if !world.click_object(&renderer, Some(&object)) {
                    world.log.push(format!("the room takes no click on {object}"));
                }
                settle(&mut world, &renderer, PATIENCE);
                continue;
            }
            let rule = world
                .noun_of_object(&object)
                .and_then(|n| world.verbs(&n).into_iter().find(|r| r.verb == "JUMP"));
            match rule {
                Some(rule) => {
                    world.act(&renderer, rule);
                    settle(&mut world, &renderer, PATIENCE);
                }
                None => world.log.push(format!("cannot jump to {object}")),
            }
            continue;
        }
        // `POINT:X,Y` reports what is under that pixel of the picture.
        if noun == "POINT" {
            let at = verb
                .split_once(',')
                .and_then(|(x, y)| Some((x.trim().parse::<f32>().ok()?, y.trim().parse::<f32>().ok()?)));
            match at {
                Some((x, y)) => {
                    let (pick, object) = world.pick_object(x, y, o.size.0, o.size.1);
                    world.log.push(format!("point {x:.0},{y:.0}: {pick:?} on {object:?}"));
                }
                None => world.log.push(format!("cannot POINT:{verb}; it is POINT:X,Y")),
            }
            continue;
        }
        // `CLICK:X,Z` is a click on the floor there, not waited out (so a
        // walk under way can be clicked into); `CLICK:HERE` at the ego's
        // own feet.
        if noun == "CLICK" {
            let here = world.ego_position().map(|(at, _)| (at.x, at.z));
            let goal = if verb == "HERE" {
                here
            } else {
                verb.split_once(',')
                    .and_then(|(x, z)| Some((x.trim().parse().ok()?, z.trim().parse().ok()?)))
            };
            let Some((x, z)) = goal else {
                world.log.push(format!("cannot CLICK:{verb}; it is CLICK:X,Z or CLICK:HERE"));
                continue;
            };
            let walks = world.walk_ego(glam::Vec3::new(x, 0.0, z), None, crate::world::EgoWalk::Click);
            world.log.push(format!(
                "click at {x:.0},{z:.0}{}",
                if walks { "" } else { " (no walk)" }
            ));
            continue;
        }
        // `WALK:X,Z` is a click on the floor there.
        if noun == "WALK" {
            let goal = verb
                .split_once(',')
                .and_then(|(x, z)| Some((x.trim().parse().ok()?, z.trim().parse().ok()?)));
            let Some((x, z)) = goal else {
                world.log.push(format!("cannot WALK:{verb}; it is WALK:X,Z"));
                continue;
            };
            if world.walk_ego(glam::Vec3::new(x, 0.0, z), None, crate::world::EgoWalk::Click) {
                world.log.push(format!("walk to {x:.0},{z:.0}"));
                // With --seconds, the last walk is caught that far in.
                if i + 1 == o.actions.len() && o.seconds > 0.0 {
                    continue;
                }
                let start = world.outside();
                while world.ego_walking() && world.outside() - start < PATIENCE {
                    let t = world.outside() + STEP;
                    world.update(&renderer, t);
                }
                settle(&mut world, &renderer, PATIENCE);
                if let Some((at, _)) = world.ego_position() {
                    world.log.push(format!("  (stops at {:.0},{:.0})", at.x, at.z));
                }
            } else {
                world.log.push(format!("cannot walk to {x:.0},{z:.0}"));
            }
            continue;
        }
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
    if let Some(name) = &o.movie {
        // Shown for real, not skipped: the story stands still meanwhile.
        world.skip_movies = false;
        world.play_movie(name, crate::world::AfterMovie::Nothing);
        let base = world.outside();
        let steps = (o.seconds.max(0.0) / STEP).round() as usize;
        for k in 0..=steps {
            world.update(&renderer, base + k as f64 * STEP);
        }
    }
    for location in &o.goto {
        world.change_location(&renderer, location);
        settle(&mut world, &renderer, PATIENCE);
    }
    for id in &o.map {
        // A follow plays out first; then the button, and the ride.
        let start = world.outside();
        while world.map.as_ref().is_some_and(|m| !m.interactive()) && world.outside() - start < PATIENCE
        {
            let t = world.outside() + STEP;
            world.update(&renderer, t);
        }
        if world.map.is_none() {
            world
                .log
                .push(format!("cannot press {id}: the map is not up"));
            continue;
        }
        world.map_press(id);
        let start = world.outside();
        while world.map.is_some() && world.outside() - start < PATIENCE {
            let t = world.outside() + STEP;
            world.update(&renderer, t);
        }
        settle(&mut world, &renderer, PATIENCE);
    }
    if let Some(place) = &o.zoom {
        match &world.binocs {
            Some(b) => {
                // Point at the middle of where the place is, then zoom.
                if let Some(((y0, y1), (p0, p1))) = crate::binocs::aim(&b.lookout_code, place) {
                    let (yaw, pitch) = (
                        ((y0 + y1) / 2.0).to_radians(),
                        ((p0 + p1) / 2.0).to_radians(),
                    );
                    let (dy, dp) = (yaw - b.panned.0, pitch - b.panned.1);
                    world.binocs_turn(dy, dp);
                }
                world.binocs_zoom_in(&renderer);
                settle(&mut world, &renderer, PATIENCE);
            }
            None => world
                .log
                .push(format!("cannot zoom to {place}: the binoculars are not up")),
        }
    }
    if o.lift {
        match world.fingerprint.as_mut() {
            Some(kit) => {
                kit.click_brush();
                kit.click_dust();
                let all: Vec<usize> = (0..kit.object.prints.len()).collect();
                let events = kit.dust(&all, true, 2000.0, 1.0);
                world.fingerprint_events(events);
                settle_speech(&mut world, &renderer);
                // Put the brush back before taking the tape.
                if let Some(kit) = world.fingerprint.as_mut() {
                    kit.click_brush();
                }
                for i in 0..all.len() {
                    let Some(kit) = world.fingerprint.as_mut() else {
                        break;
                    };
                    kit.click_tape();
                    kit.click_print(i);
                    let events = kit.click_cloth();
                    world.fingerprint_events(events);
                    settle_speech(&mut world, &renderer);
                }
            }
            None => world
                .log
                .push("cannot lift prints: the kit is not up".into()),
        }
    }
    for step in &o.sid {
        if step == "open" {
            world.open_sidney();
            continue;
        }
        if world.sidney.is_none() {
            world.log.push(format!("cannot {step}: SIDNEY is not up"));
            continue;
        }
        use crate::sidney::{Hit, Tool};
        let (what, arg) = step.split_once('=').unwrap_or((step, ""));
        match what {
            "addata" => {
                world.sidney_click(&renderer, Hit::Tool(Tool::AddData));
                run_until(&mut world, &renderer, |w| w.state.inventory_shown);
                // The scanner, on the item, from the inventory's menu.
                let item = arg.to_ascii_uppercase();
                match world.verbs(&item).into_iter().find(|r| r.verb == "SCANNER") {
                    Some(rule) => world.act(&renderer, rule),
                    None => world.log.push(format!("cannot scan {item}")),
                }
                run_until(&mut world, &renderer, |w| !w.state.inventory_shown);
                // Scanning, then the "input complete" box.
                for _ in 0..(4.0 / STEP) as usize {
                    let t = world.outside() + STEP;
                    world.update(&renderer, t);
                }
                world.sidney_click(&renderer, Hit::PopupOk);
            }
            "email" => {
                world.sidney_click(&renderer, Hit::Tool(Tool::Email));
                world.sidney_click(&renderer, Hit::EmailItem(arg.parse().unwrap_or(0)));
            }
            "files" => world.sidney_click(&renderer, Hit::Tool(Tool::Files)),
            "search" => {
                world.sidney_click(&renderer, Hit::Tool(Tool::Search));
                if let Some(s) = world.sidney.as_mut() {
                    s.type_text(arg);
                }
                world.sidney_click(&renderer, Hit::SearchGo);
            }
            "link" => world.sidney_click(&renderer, Hit::Link(crate::html::page_name(arg))),
            // A file opened in a tool (by number).
            "analyze" | "translate" => {
                let tool = if what == "analyze" {
                    Tool::Analyze
                } else {
                    Tool::Translate
                };
                world.sidney_click(&renderer, Hit::Tool(tool));
                world.sidney_click(&renderer, Hit::Tool(Tool::Files));
                if let Some(s) = world.sidney.as_mut() {
                    s.choose_for(tool);
                }
                world.sidney_click(&renderer, Hit::File(arg.parse().unwrap_or(0)));
            }
            "go" => {
                let analyzing = world
                    .sidney
                    .as_ref()
                    .is_some_and(|s| s.screen == crate::sidney::Screen::Analyze);
                world.sidney_click(
                    &renderer,
                    if analyzing {
                        Hit::AnalyzeGo
                    } else {
                        Hit::TranslateGo
                    },
                );
            }
            "menu" => {
                let (m, i) = arg.split_once(',').unwrap_or((arg, "0"));
                let (m, i) = (m.parse().unwrap_or(0), i.parse().unwrap_or(0));
                world.sidney_click(&renderer, Hit::MenuHeader(m));
                world.sidney_click(&renderer, Hit::MenuItem(m, i));
            }
            "from" => world.sidney_click(&renderer, Hit::Language(true, arg.parse().unwrap_or(0))),
            // Make ID: a job (menu, entry), then print.
            "makeid" => {
                let (m, i) = arg.split_once(',').unwrap_or((arg, "0"));
                let (m, i) = (m.parse().unwrap_or(0), i.parse().unwrap_or(0));
                world.sidney_click(&renderer, Hit::Tool(Tool::MakeId));
                world.sidney_click(&renderer, Hit::MenuHeader(m));
                world.sidney_click(&renderer, Hit::MenuItem(m, i));
                world.sidney_click(&renderer, Hit::Print);
            }
            "pick" => world.sidney_click(&renderer, Hit::PopupButton(arg.parse().unwrap_or(0))),
            // Suspects: open one, choose a file, link it, match a print.
            "suspect" => {
                world.sidney_click(&renderer, Hit::Tool(Tool::Suspects));
                world.sidney_click(&renderer, Hit::MenuHeader(1));
                world.sidney_click(&renderer, Hit::MenuItem(1, arg.parse().unwrap_or(0)));
            }
            "sfile" => {
                if let Some(s) = world.sidney.as_mut() {
                    s.choose_for(Tool::Suspects);
                }
                world.sidney_click(&renderer, Hit::File(arg.parse().unwrap_or(0)));
            }
            "slink" => world.sidney_click(&renderer, Hit::MenuItem(2, 0)),
            "match" => world.sidney_click(&renderer, Hit::MenuItem(2, 3)),
            "type" => {
                if let Some(s) = world.sidney.as_mut() {
                    s.type_text(arg);
                }
                world.sidney_click(&renderer, Hit::PopupButton(0));
            }
            "exit" => world.sidney_click(&renderer, Hit::Exit),
            // The map: a click, or a drag, at SIDNEY's pixels.
            "mapclick" | "mapdrag" => {
                let n: Vec<f32> = arg.split(',').filter_map(|v| v.trim().parse().ok()).collect();
                if n.len() < 2 {
                    world.log.push(format!("cannot {what}={arg}"));
                    continue;
                }
                world.sidney_press(&renderer, (n[0], n[1]));
                if n.len() >= 4 {
                    for k in 1..=20 {
                        let t = k as f32 / 20.0;
                        let at = (n[0] + (n[2] - n[0]) * t, n[1] + (n[3] - n[1]) * t);
                        if let Some(s) = world.sidney.as_mut() {
                            s.map_held(at);
                        }
                        let t = world.outside() + STEP;
                        world.update(&renderer, t);
                    }
                }
                if let Some(s) = world.sidney.as_mut() {
                    s.map_release();
                }
            }
            "choose" => world.sidney_click(&renderer, Hit::Choice(arg.parse().unwrap_or(0))),
            // For trying a step of the story without playing up to it.
            "flag" => {
                world.state.flags.insert(gk3_game::state::key(arg));
            }
            "file" => {
                world.state.sidney_files.insert(arg.parse().unwrap_or(0));
            }
            "give" => {
                let ego = world.state.ego.clone();
                world.state.give_item(&ego, arg);
            }
            other => world.log.push(format!("no SIDNEY step {other}")),
        }
        // Let SIDNEY's own steps (movies, pauses) play out.
        let start = world.outside();
        while world.sidney.as_ref().is_some_and(|s| s.busy()) && world.outside() - start < 30.0 {
            let t = world.outside() + STEP;
            world.update(&renderer, t);
        }
        settle(&mut world, &renderer, PATIENCE);
    }
    // A movie's time is its own (the story stands still under it).
    let end = world.outside() + if o.movie.is_some() { 0.0 } else { o.seconds };
    while world.outside() < end {
        let t = world.outside() + STEP;
        world.update(&renderer, t);
    }

    if let Some(name) = &o.save {
        let path = world
            .save()
            .and_then(|text| crate::saves::write(name, &text))?;
        println!("saved to {}", path.display());
        if let (Some(scene), Some(seen)) = (&world.scene, world.view_scene()) {
            let image = renderer.render_image(
                seen,
                &scene.cast.models(),
                &gk3_render::Overlay::default(),
                &world.camera,
                ShadeMode::Lit,
                (640, 480),
            )?;
            crate::saves::write_thumb(name, &crate::saves::shrink(&image, 4))?;
        }
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
    if o.where_actors {
        if let Some(scene) = &world.scene {
            let names: Vec<&str> = scene
                .room
                .cameras
                .iter()
                .filter(|c| c.room)
                .map(|c| c.name.as_str())
                .collect();
            println!("  (camera angles to choose from: {})", names.join(", "));
        }
        let mut actors: Vec<_> = world.whereabouts().actors.into_iter().collect();
        actors.sort_by(|a, b| a.0.cmp(&b.0));
        for (name, (at, goal)) in actors {
            let walking = if at == goal {
                String::new()
            } else {
                format!(", walking to {:.0},{:.0}", goal[0], goal[2])
            };
            // And the middle of the model as drawn, which an animation
            // can carry away from where the actor stands.
            let model = world
                .scene
                .as_ref()
                .and_then(|s| s.cast.center(&name))
                .map_or(String::new(), |c| {
                    format!("; model centre {:.1},{:.1},{:.1}", c.x, c.y, c.z)
                });
            let facing = world
                .scene
                .as_ref()
                .and_then(|s| s.cast.actor_position(&name))
                .map_or(String::new(), |(_, h)| format!(" facing {:.0}°", h.to_degrees()));
            let hidden = world
                .scene
                .as_ref()
                .and_then(|s| s.cast.is_visible(&name))
                .map_or("", |v| if v { "" } else { ", hidden" });
            let lit = world
                .scene
                .as_ref()
                .and_then(|s| s.cast.floor_light_of(&name))
                .map_or(String::new(), |c| format!("; lit {:.2},{:.2},{:.2}", c[0], c[1], c[2]));
            println!(
                "  ({name} at {:.0},{:.0},{:.0}{facing}{walking}{hidden}{model}{lit})",
                at[0], at[1], at[2]
            );
        }
    }
    if o.where_actors
        && let Some(scene) = &world.scene
    {
        // And the props shown, by the middle of each as drawn.
        for name in scene.cast.shown_props() {
            if let Some(c) = scene.cast.center(&name) {
                println!("  (prop {name}; model centre {:.1},{:.1},{:.1})", c.x, c.y, c.z);
            }
        }
    }
    if !world.idle()
        && let Some((at, heading)) = world.ego_position()
    {
        // Where things stand, for finding what a stuck run waits on.
        println!(
            "  (player at {:.0},{:.0},{:.0} facing {:.0}°{}; {})",
            at.x,
            at.y,
            at.z,
            heading.to_degrees(),
            if world.ego_walking() { ", walking" } else { "" },
            world.waiting_on()
        );
    }
    if let Some(path) = &o.png {
        let size = o.size;
        if let Some([x, y, z, yaw, pitch]) = o.view {
            world.camera = gk3_render::Camera {
                position: glam::Vec3::new(x, y, z),
                yaw: yaw.to_radians(),
                pitch: pitch.to_radians(),
                fov_y: 60f32.to_radians(),
            };
        }
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
                size,
            );
        }
        if let Some(map) = &world.map {
            ui.map(
                lib,
                &mut ui_renderer,
                &mut overlay,
                map,
                (-1.0, -1.0),
                size,
            );
        }
        if let Some(gps) = &world.gps {
            ui.gps(lib, &mut ui_renderer, &mut overlay, gps, ((-1.0, -1.0), false), size);
        }
        ui.captions(&mut overlay, &world, size);
        if o.title_screen {
            ui.title_screen(
                lib,
                &mut ui_renderer,
                &mut overlay,
                (-1.0, -1.0),
                None,
                crate::saves::has_quick(),
                size,
            );
        }
        if let Some(show) = &world.showing {
            ui.movie(&mut ui_renderer, &mut overlay, show, (&world.state.ego, true), size);
        }
        if o.card {
            // Two seconds in: the lettering has finished writing itself.
            let card = crate::ui::TitleCard::new(lib, &world.state.timeblock, 0.0);
            ui.title_card(
                lib,
                &mut ui_renderer,
                &mut overlay,
                &card,
                (2.0, (-1.0, -1.0), true),
                size,
            );
        }
        if let Some(sidney) = world.sidney.as_mut() {
            sidney.draw(
                (&mut ui, lib, &mut ui_renderer),
                &mut overlay,
                &world.state,
                (-1.0, -1.0),
                size,
            );
        }
        if let Some(kit) = &world.fingerprint {
            ui.fingerprint(
                lib,
                &mut ui_renderer,
                &mut overlay,
                kit,
                ((640.0, 300.0), false),
                size,
            );
        }
        if world.binocs.is_some() {
            let zoomed = world.binocs.as_ref().is_some_and(|b| b.zoomed.is_some());
            let can_zoom = world.binocs_target().is_some();
            ui.binocs(
                lib,
                &mut ui_renderer,
                &mut overlay,
                (zoomed, can_zoom),
                ((-1.0, -1.0), false),
                size,
            );
        }
        let mut full_screen = o.title_screen || world.showing.is_some();
        if let Some(name) = &o.screen {
            full_screen |= draw_screen(name, &world, (&mut ui, lib, &mut ui_renderer), &mut overlay, size)?;
        }
        // The camera may have been moved since the last update (`--view`).
        let eye = world.camera.position;
        if let Some(scene) = &mut world.scene {
            scene.cast.cull_around(eye);
        }
        let image = match (&world.scene, world.view_scene()) {
            (Some(scene), Some(seen))
                if world.fingerprint.is_none() && world.sidney.is_none() && !full_screen =>
            {
                ui_renderer.render_image(
                    seen,
                    &scene.cast.models(),
                    &overlay,
                    &world.camera,
                    ShadeMode::Lit,
                    size,
                )?
            }
            _ => ui_renderer.render_overlay_image(&overlay, size)?,
        };
        std::fs::write(path, png::encode(&image)).map_err(|e| format!("{path}: {e}"))?;
    }
    Ok(ExitCode::SUCCESS)
}

/// Draws one of the screens around the story over the picture, as the
/// window would with the pointer away from it. True if it fills the screen.
fn draw_screen(
    name: &str,
    world: &World,
    (ui, lib, renderer): (&mut crate::ui::Ui, &Library, &mut Renderer),
    overlay: &mut gk3_render::Overlay,
    size: (u32, u32),
) -> Result<bool, String> {
    use crate::screens::{Button, OptionBar, Painter, Popup, SaveLoad, Status};
    // A noun's verb menu, the pointer on its second verb with its tooltip.
    if let Some(noun) = name.strip_prefix("menu:") {
        let noun = noun.to_ascii_uppercase();
        let menu = crate::ui::Menu::new(noun.clone(), world.verbs(&noun), (false, true), (size.0 as f32 / 2.0, size.1 as f32 / 2.0));
        let slots = crate::ui::Ui::menu_slots(&menu, size);
        let at = slots
            .get(1)
            .or(slots.first())
            .map_or((0.0, 0.0), |r| (r[0] + r[2] / 2.0, r[1] + r[3] / 2.0));
        if let Some(verb) = ui.menu(lib, renderer, overlay, &menu, (at, false), size) {
            let tip = ui.verb_tip(&verb);
            ui.tooltip(overlay, &tip, at, size);
        }
        return Ok(false);
    }
    let settings = crate::saves::Settings::default();
    let mut painter = Painter {
        ui,
        lib,
        renderer,
        overlay,
        pointer: (-1.0, -1.0),
        held: None,
        size,
        hits: Vec::new(),
    };
    if let Some(section) = name.strip_prefix("bar") {
        let mut bar = OptionBar::new((640.0, 200.0));
        let presses: &[Button] = match section {
            "" => &[],
            "-options" => &[Button::Options],
            "-sound" => &[Button::Options, Button::Advanced, Button::SoundOptions],
            "-graphics" => &[Button::Options, Button::Advanced, Button::GraphicsOptions],
            "-graphics-advanced" => &[
                Button::Options,
                Button::Advanced,
                Button::GraphicsOptions,
                Button::GraphicsAdvanced,
            ],
            "-game" => &[Button::Options, Button::Advanced, Button::GameOptions],
            "-cameras" => &[],
            _ => return Err(format!("no option bar section {section}")),
        };
        for &b in presses {
            bar.press(b);
        }
        let ego = &world.state.ego;
        if section == "-cameras" {
            bar.cameras = true;
        }
        let shots = if bar.cameras {
            crate::screens::camera_shots(world, painter.renderer, painter.ui)
        } else {
            Vec::new()
        };
        let status = Status {
            camera_shots: &shots,
            cameras_free: true,
            exitable: false,
            on_map: world.map.is_some(),
            cinematics: true,
            score: world.score(),
            timeblock: &world.state.timeblock,
            active_item: world.state.active_item.get(ego).map(String::as_str),
            inventory_open: world.state.inventory_shown,
            settings: &settings,
            any_saves: true,
        };
        painter.option_bar(&crate::screens::Layout::load(lib), &bar, &status);
        return Ok(false);
    }
    match name {
        "save" | "load" => painter.save_load(&SaveLoad::new(name == "save"), 0.0),
        "quit" => painter.popup(Popup::Quit),
        "overwrite" => painter.popup(Popup::Overwrite),
        "death" => painter.death(true),
        "finished" => painter.finished(),
        "paused" => painter.paused(),
        "help" | "help2" | "help3" => {
            let page = name.strip_prefix("help").and_then(|n| n.parse::<usize>().ok()).unwrap_or(1);
            painter.help(&crate::screens::Help::load(lib), page - 1);
        }
        _ => return Err(format!("no screen {name}")),
    }
    Ok(matches!(name, "save" | "load" | "death" | "finished"))
}
