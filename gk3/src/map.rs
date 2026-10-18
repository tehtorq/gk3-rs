//! The driving map: the valley seen from above, with a button for each place
//! the player can ride to, and blips for whoever is on the roads.
//!
//! Like G-Engine's `DrivingScreen`, which this follows: which places show
//! depends on the story; a blip moves along `PATHDATA.TXT`'s roads one point
//! every fifth of a second; and when the player follows someone, their blip
//! trails the other until it stops, which opens the place it stopped at.

use gk3_game::actions::parse_timeblock;
use gk3_game::state::GameState;

/// A button: the location it goes to, its art (`DM_<id>.BMP` and
/// `DM_<id>_UL.BMP`), and its top left on the 640x480 map.
pub struct Place {
    pub location: &'static str,
    pub id: &'static str,
    pub at: (f32, f32),
}

pub const PLACES: &[Place] = &[
    Place {
        location: "VGR",
        id: "ARM",
        at: (458.0, 225.0),
    }, // Devil's Armchair
    Place {
        location: "BEC",
        id: "BEC",
        at: (94.0, 400.0),
    }, // Southwest arm of the hexagram
    Place {
        location: "PL3",
        id: "BMB",
        at: (499.0, 137.0),
    }, // Orange Rock
    Place {
        location: "PL2",
        id: "CSD",
        at: (396.0, 187.0),
    }, // Coume Sourde
    Place {
        location: "PL6",
        id: "CSE",
        at: (520.0, 4.0),
    }, // Chateau de Serras
    Place {
        location: "PL4",
        id: "LER",
        at: (387.0, 258.0),
    }, // L'Ermitage
    Place {
        location: "LHE",
        id: "LHE",
        at: (454.0, 65.0),
    }, // Larry's house
    Place {
        location: "PL1",
        id: "LHM",
        at: (442.0, 155.0),
    }, // L'Homme Mort
    Place {
        location: "MCB",
        id: "MCB",
        at: (555.0, 72.0),
    }, // Northeast arm of the hexagram
    Place {
        location: "PLO",
        id: "PLO",
        at: (447.0, 91.0),
    }, // Chateau de Blanchefort
    Place {
        location: "POU",
        id: "POU",
        at: (578.0, 22.0),
    }, // Poussin's tomb
    Place {
        location: "RL1",
        id: "RL1",
        at: (487.0, 174.0),
    }, // Rennes-les-Bains
    Place {
        location: "MOP",
        id: "RLC",
        at: (193.0, 119.0),
    }, // Rennes-le-Chateau
    Place {
        location: "TR1",
        id: "TR1",
        at: (57.0, 131.0),
    }, // Train station
    Place {
        location: "TRE",
        id: "TRE",
        at: (506.0, 89.0),
    }, // "The Site"
    Place {
        location: "PL5",
        id: "WOD",
        at: (44.0, 218.0),
    }, // Lady Howard and Estelle's site
];

/// Whom the player is following, by `FollowOnDrivingMap`'s number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Follow {
    None,
    Buthane,
    Wilkes,
    LadyHoward,
    PrinceJamesMen,
    Estelle,
}

impl Follow {
    pub fn from_number(n: i32) -> Self {
        match n {
            1 => Follow::Buthane,
            2 => Follow::Wilkes,
            // 3 was to follow Mosely, cut from the game.
            4 => Follow::LadyHoward,
            5 => Follow::PrinceJamesMen,
            6 => Follow::Estelle,
            _ => Follow::None,
        }
    }
}

/// What the map asks the game to do.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// Speak lines, by license plate and count.
    Say(String, i32),
    ActorAt(String, String),
    /// Count a noun and verb once more, for the player.
    Count(&'static str, &'static str),
    /// Leave the map for a location, once anything being said is done.
    Leave(String),
}

/// Blip colours.
pub const EGO: [u8; 3] = [0, 255, 0];
const BUTHANE: [u8; 3] = [198, 182, 255];
const WILKES: [u8; 3] = [247, 150, 57];
const ESTELLE: [u8; 3] = [33, 56, 140];
const JAMES_MEN: [u8; 3] = [24, 146, 49];

/// Seconds a blip waits at each point of a road.
const STEP: f64 = 0.2;
const BLINK: f64 = 0.5;
/// Each half of a hint's flash.
const FLASH: f64 = 0.2;

struct Node {
    name: String,
    point: (f32, f32),
    /// To node, road, and whether the road runs from here.
    links: Vec<(usize, Option<usize>, bool)>,
}

/// The roads, from `PATHDATA.TXT`.
pub struct Roads {
    nodes: Vec<Node>,
    roads: Vec<(String, Vec<(f32, f32)>)>,
}

impl Roads {
    pub fn parse(text: &str) -> Self {
        let pair = |s: &str| {
            let (x, y) = s.split_once(',')?;
            Some((x.trim().parse().ok()?, y.trim().parse().ok()?))
        };
        let mut nodes: Vec<Node> = Vec::new();
        let mut roads: Vec<(String, Vec<(f32, f32)>)> = Vec::new();
        for line in text.lines() {
            let mut words = line.split_whitespace();
            match (words.next(), words.next()) {
                (Some(w), Some(name)) if w.eq_ignore_ascii_case("NodeBegin") => nodes.push(Node {
                    name: name.to_ascii_uppercase(),
                    point: (0.0, 0.0),
                    links: Vec::new(),
                }),
                (Some(w), Some(p)) if w.eq_ignore_ascii_case("Location") => {
                    if let (Some(n), Some(p)) = (nodes.last_mut(), pair(p)) {
                        n.point = p;
                    }
                }
                (Some(w), Some(name)) if w.eq_ignore_ascii_case("SegmentBegin") => {
                    roads.push((name.to_ascii_uppercase(), Vec::new()))
                }
                (Some(w), Some(p)) if w.eq_ignore_ascii_case("Point") => {
                    if let (Some(r), Some(p)) = (roads.last_mut(), pair(p)) {
                        r.1.push(p);
                    }
                }
                _ => {}
            }
        }
        // Links name nodes and roads that may come later, so a second pass.
        let mut node = 0;
        let mut in_links = false;
        for line in text.lines() {
            let words: Vec<&str> = line.split_whitespace().collect();
            match words.as_slice() {
                [w, ..] if w.eq_ignore_ascii_case("LinksBegin") => in_links = true,
                [w, ..] if w.eq_ignore_ascii_case("LinksEnd") => {
                    in_links = false;
                    node += 1;
                }
                [to, road, forward, ..] if in_links => {
                    let to = nodes.iter().position(|n| n.name.eq_ignore_ascii_case(to));
                    let road = roads.iter().position(|r| r.0.eq_ignore_ascii_case(road));
                    if let (Some(to), Some(n)) = (to, nodes.get_mut(node)) {
                        n.links
                            .push((to, road, forward.eq_ignore_ascii_case("TRUE")));
                    }
                }
                _ => {}
            }
        }
        Self { nodes, roads }
    }

    fn node(&self, name: &str) -> Option<usize> {
        self.nodes
            .iter()
            .position(|n| n.name.eq_ignore_ascii_case(name))
    }

    pub fn point(&self, name: &str) -> Option<(f32, f32)> {
        self.node(name).map(|i| self.nodes[i].point)
    }

    /// The points along the road from one node to the next, in order, or
    /// none where the data has no road between them (IN3 to PL2).
    fn road(&self, from: usize, to: usize) -> Vec<(f32, f32)> {
        let Some(&(_, Some(road), forward)) = self.nodes[from].links.iter().find(|l| l.0 == to)
        else {
            return Vec::new();
        };
        let mut points = self.roads[road].1.clone();
        if !forward {
            points.reverse();
        }
        points
    }
}

/// A dot on the map.
pub struct Blip {
    pub color: [u8; 3],
    pub at: (f32, f32),
    pub visible: bool,
    blinks: bool,
    /// The whole drive, as map points, and how far along it the blip is.
    route: Vec<(f32, f32)>,
    step: usize,
    looping: bool,
    /// The point just left: where a follower is drawn.
    behind: (f32, f32),
}

impl Blip {
    fn new(color: [u8; 3], at: (f32, f32)) -> Self {
        Self {
            color,
            at,
            visible: true,
            blinks: false,
            route: Vec::new(),
            step: 0,
            looping: false,
            behind: at,
        }
    }

    /// Drives through named nodes in turn, along the roads between them.
    fn drive(&mut self, roads: &Roads, through: &[&str], looping: bool) {
        let nodes: Vec<usize> = through.iter().filter_map(|n| roads.node(n)).collect();
        let mut route = Vec::new();
        for pair in nodes.windows(2) {
            route.push(roads.nodes[pair[0]].point);
            route.extend(roads.road(pair[0], pair[1]));
        }
        if let Some(&last) = nodes.last() {
            route.push(roads.nodes[last].point);
        }
        if looping
            && let Some(&first) = nodes.first()
            && let Some(&last) = nodes.last()
        {
            route.extend(roads.road(last, first));
        }
        if let Some(&start) = route.first() {
            self.at = start;
            self.behind = start;
        }
        self.route = route;
        self.step = 0;
        self.looping = looping;
    }

    /// Jumps along the route to a node it passes, if it does.
    fn skip_to(&mut self, roads: &Roads, node: &str) {
        let Some(point) = roads.point(node) else {
            return;
        };
        if let Some(i) = self.route.iter().position(|&p| p == point) {
            self.step = i;
            self.at = point;
            self.behind = point;
        }
    }

    fn arrived(&self) -> bool {
        !self.looping && self.step + 1 >= self.route.len()
    }

    /// One step along the route.
    fn advance(&mut self) {
        if self.route.is_empty() || self.arrived() {
            self.behind = self.at;
            return;
        }
        self.step = if self.looping {
            (self.step + 1) % self.route.len()
        } else {
            self.step + 1
        };
        self.behind = self.at;
        self.at = self.route[self.step];
    }
}

/// The map while it is up.
pub struct MapScreen {
    pub from: String,
    pub follow: Follow,
    pub blips: Vec<Blip>,
    /// Buttons shown, by button id.
    pub shown: Vec<&'static str>,
    roads: Roads,
    clock: f64,
    next_step: f64,
    /// The followed blip has stopped: buttons work again.
    pub follow_done: bool,
    /// Said once the followed blip stops, and what happens then.
    on_arrival: Vec<Event>,
    /// Places the hint button picked out: they flash ten times, a fifth
    /// of a second each way, as G-Engine's `FlashLocation`.
    pub flashing: Vec<&'static str>,
    pub flash_lit: bool,
    flash_left: u32,
    flash_timer: f64,
}

/// Day and hour, comparable; `202A` is the small hours after day 2.
fn when(timeblock: &str) -> (u32, u32) {
    parse_timeblock(timeblock).unwrap_or((0, 0))
}

impl MapScreen {
    pub fn new(roads_text: &str, state: &GameState, from: &str, follow: Follow) -> Self {
        let roads = Roads::parse(roads_text);
        let tb = state.timeblock.as_str();
        let now = when(tb);
        let late_night = tb == "202A";
        let nvc = |noun: &str, verb: &str| state.noun_verb(&state.ego, noun, verb) > 0;
        let mut shown: Vec<&'static str> = vec!["LHE", "PLO", "RL1", "RLC", "TR1"];
        let mut show = |id: &'static str, yes: bool| {
            if yes {
                shown.push(id);
            }
        };
        show("ARM", now >= (2, 14) || late_night);
        show("CSE", now >= (2, 12) || late_night);
        show("POU", now >= (2, 7) || late_night);
        show("BEC", tb == "312P");
        show("MCB", tb == "312P");
        show("TRE", state.flags.contains("MARKEDTHESITE") && tb == "312P");
        show(
            "BMB",
            nvc("VIEW_OF_ORANGE_ROCK", "BINOCULARS") || now >= (3, 15),
        );
        let csd = (nvc("BUTHANE", "FOLLOW") && follow != Follow::Buthane) || now >= (1, 16);
        show("CSD", csd);
        show("LHM", csd);
        show(
            "LER",
            (nvc("WILKES", "FOLLOW") && follow != Follow::Wilkes) || now >= (1, 16),
        );
        show(
            "WOD",
            (nvc("ESTELLE", "FOLLOW") && follow != Follow::Estelle) || now >= (2, 17),
        );

        // Places in town are all Rennes-le-Chateau on the map.
        let start = roads
            .point(from)
            .or(roads.point("MOP"))
            .unwrap_or((0.0, 0.0));
        let mut ego = Blip::new(EGO, start);
        ego.blinks = follow == Follow::None;
        let mut map = Self {
            from: from.to_ascii_uppercase(),
            follow,
            blips: vec![ego],
            shown,
            roads,
            clock: 0.0,
            next_step: STEP,
            flashing: Vec::new(),
            flash_lit: false,
            flash_left: 0,
            flash_timer: 0.0,
            follow_done: follow == Follow::None,
            on_arrival: Vec::new(),
        };
        map.place_others(state);
        map
    }

    fn other(&mut self, color: [u8; 3], through: &[&str], looping: bool) -> &mut Blip {
        let mut blip = Blip::new(color, (0.0, 0.0));
        blip.drive(&self.roads, through, looping);
        self.blips.push(blip);
        self.blips.last_mut().unwrap()
    }

    /// Who else is driving about, as G-Engine's `PlaceBlips`.
    fn place_others(&mut self, state: &GameState) {
        const CIRCUIT: &[&str] = &[
            "PL3", "RL1", "IN4", "VGR", "PL4", "BEC", "PL5", "TR1", "IN1", "LHE", "PLO",
        ];
        let tb = state.timeblock.clone();
        let say = |plate: &str| Event::Say(plate.into(), 1);
        match self.follow {
            Follow::None => match tb.as_str() {
                "102P" => {
                    if !self.shown.contains(&"LHM") {
                        self.other(BUTHANE, CIRCUIT, true);
                    }
                    if !self.shown.contains(&"LER") {
                        let route = [
                            "LHE", "PLO", "PL3", "RL1", "IN4", "VGR", "PL4", "BEC", "PL5", "TR1",
                            "IN1",
                        ];
                        self.other(WILKES, &route, true);
                    }
                }
                "104P" => {
                    let route = [
                        "PLO", "PL3", "RL1", "IN4", "VGR", "PL4", "BEC", "PL5", "TR1", "IN1", "LHE",
                    ];
                    self.other(ESTELLE, &route, true);
                }
                "106P" if state.noun_verb(&state.ego, "TWO_MEN", "FOLLOW") == 0 => {
                    let route = [
                        "LHE", "IN1", "TR1", "PL5", "BEC", "PL4", "VGR", "IN4", "RL1", "PL3", "PLO",
                    ];
                    self.other(JAMES_MEN, &route, true);
                }
                "202P"
                    if state
                        .actor_locations
                        .get("ESTELLE")
                        .is_some_and(|l| l == "MAP") =>
                {
                    self.other(ESTELLE, CIRCUIT, true);
                }
                _ => {}
            },
            Follow::Buthane => {
                let route = [
                    "VGR", "PL4", "BEC", "PL5", "TR1", "IN1", "LHE", "PLO", "PL3", "RL1", "IN4",
                    "IN3", "PL2",
                ];
                self.other(BUTHANE, &route, false);
                self.on_arrival = vec![
                    say("2196L3WL71"),
                    // Followed all the way, as the original counts it
                    // (0x480d65); day one's afternoon ends on it.
                    Event::Count("BUTHANE", "FOLLOW"),
                    Event::ActorAt("BUTHANE".into(), "CSD".into()),
                ];
            }
            Follow::Wilkes => {
                self.other(WILKES, &["PLO", "PL3", "RL1", "IN4", "VGR", "PL4"], false);
                self.on_arrival = vec![
                    say("2196L3WLM1"),
                    Event::Count("WILKES", "FOLLOW"),
                    Event::ActorAt("WILKES".into(), "LER".into()),
                ];
            }
            Follow::LadyHoward => {
                let route: &[&str] = if self.from == "PLO" {
                    &[
                        "PLO", "PL3", "RL1", "IN4", "VGR", "PL4", "BEC", "PL5", "TR1", "IN1",
                        "LHE", "PLO",
                    ]
                } else {
                    &[
                        "PL4", "BEC", "PL5", "TR1", "IN1", "LHE", "PLO", "PL3", "RL1", "IN4",
                        "VGR", "PL4",
                    ]
                };
                self.other(ESTELLE, route, false);
                // "They're just driving around the valley": back where we were.
                self.on_arrival = vec![say("21A6L3WLX1"), Event::Leave(self.from.clone())];
            }
            Follow::PrinceJamesMen => {
                let route: &[&str] = if self.from == "PLO" || self.from == "LER" {
                    &[
                        "LHE", "IN1", "TR1", "PL5", "BEC", "PL4", "VGR", "IN4", "RL1", "PL3",
                        "PLO", "LHE",
                    ]
                } else {
                    &[
                        "MOP", "IN2", "IN1", "TR1", "PL5", "BEC", "PL4", "VGR", "IN4", "RL1",
                        "PL3", "PLO", "LHE",
                    ]
                };
                self.other(JAMES_MEN, route, false);
                self.on_arrival = vec![say("21F6L3WBH1")];
            }
            Follow::Estelle => {
                let route = [
                    "TR1", "IN1", "LHE", "PLO", "PL3", "RL1", "IN4", "VGR", "PL4", "BEC", "PL5",
                ];
                self.other(ESTELLE, &route, false);
                self.on_arrival = vec![
                    say("21K6L3WJI1"),
                    Event::ActorAt("ESTELLE".into(), "WOD".into()),
                ];
            }
        }
        // The followed blip picks up where the player set off from.
        if self.follow != Follow::None && self.blips.len() > 1 {
            let from = self.from.clone();
            let (roads, blips) = (&self.roads, &mut self.blips);
            blips[1].skip_to(roads, &from);
            blips[0].at = blips[1].at;
        }
    }

    /// Advances the blips; returns what the map asks for.
    pub fn tick(&mut self, dt: f64) -> Vec<Event> {
        self.clock += dt;
        if self.flash_left > 0 {
            self.flash_timer -= dt;
            if self.flash_timer <= 0.0 {
                self.flash_left -= 1;
                self.flash_lit = !self.flash_lit && self.flash_left > 0;
                self.flash_timer = FLASH;
            }
        }
        for blip in &mut self.blips {
            blip.visible = !blip.blinks || ((self.clock / BLINK) as u64).is_multiple_of(2);
        }
        let mut events = Vec::new();
        while self.clock >= self.next_step {
            self.next_step += STEP;
            for blip in self.blips.iter_mut().skip(1) {
                blip.advance();
            }
            if self.follow != Follow::None && !self.follow_done && self.blips.len() > 1 {
                // The player trails one point behind.
                self.blips[0].at = self.blips[1].behind;
                if self.blips[1].arrived() {
                    self.blips[0].at = self.blips[1].at;
                    self.follow_done = true;
                    self.blips[0].blinks = true;
                    // Arriving shows the place they stopped at.
                    match self.follow {
                        Follow::Buthane => self.shown.extend(["CSD", "LHM"]),
                        Follow::Wilkes => self.shown.push("LER"),
                        Follow::Estelle => self.shown.push("WOD"),
                        _ => {}
                    }
                    events.append(&mut self.on_arrival);
                }
            }
        }
        events
    }

    /// Flashes the places the story wants the player to go next. False if
    /// the hints have nothing to say.
    pub fn hint(&mut self, state: &GameState) -> bool {
        let places: Vec<&'static str> = hints(state)
            .into_iter()
            .filter(|p| self.shown.contains(p))
            .collect();
        if places.is_empty() {
            return false;
        }
        self.flashing = places;
        self.flash_lit = true;
        self.flash_left = 10;
        self.flash_timer = FLASH;
        true
    }

    /// Whether the buttons can be pressed.
    pub fn interactive(&self) -> bool {
        self.follow_done
    }
}

/// What pressing a place's button does, as G-Engine's
/// `OnLocationButtonPressed`: refusals, then the bike's new spot and the
/// location to go to.
pub fn press(state: &GameState, location: &str) -> Result<(String, Option<i32>), (String, i32)> {
    let tb = state.timeblock.as_str();
    if location == "TR1" && matches!(tb, "307A" | "312P") {
        // Grace sees nothing for her at the station.
        return Err(("21LP362PF1".into(), 1));
    }
    if location == "LHE" && matches!(tb, "106P" | "202A") {
        return Err(("21F4F625S1".into(), 1));
    }
    let mut real = if location == "TRE" {
        "PLO".to_string()
    } else {
        location.to_string()
    };
    let bike = match real.as_str() {
        "PL5" => Some(0),
        "PL4" => Some(1),
        "VGR" => Some(2),
        "PL2" => Some(3),
        "PL1" => Some(4),
        "PL3" => Some(5),
        "BEC" => Some(6),
        "MCB" => Some(7),
        "POU" => Some(8),
        // The chateau's gate stands open on these days: the bike goes in.
        "PL6" if matches!(tb, "202P" | "212P") => {
            real = "CSE".into();
            Some(15)
        }
        "PL6" => Some(9),
        "MOP" => Some(10),
        "LHE" => Some(11),
        "PLO" => Some(12),
        "RL1" => Some(13),
        "TR1" => Some(14),
        _ => None,
    };
    Ok((real, bike))
}

/// Where the hint button points: the places the player still has business
/// in this timeblock. The original keeps this in its code; these rules are
/// G-Engine's (`HintManager::ShowHint`, GPL-3.0), which follow it.
pub fn hints(state: &GameState) -> Vec<&'static str> {
    let ego = state.ego.as_str();
    let nvc = |noun: &str, verb: &str| state.noun_verb(ego, noun, verb);
    let topic = nvc;
    let var = |name: &str| *state.variables.get(&gk3_game::state::key(name)).unwrap_or(&0);
    let flag = |name: &str| state.flags.contains(&gk3_game::state::key(name));
    let has = |item: &str| state.has_item(ego, item);
    let mut out = Vec::new();
    let mut flash = |p: &'static str| {
        if !out.contains(&p) {
            out.push(p);
        }
    };
    match state.timeblock.as_str() {
        tb @ ("102P" | "104P") => {
            if tb == "102P" && (nvc("BUTHANE", "FOLLOW") == 0 || nvc("WILKES", "FOLLOW") == 0) {
                flash("PLO");
            }
            let mut more = false;
            if tb == "104P"
                && (topic("WILKES", "T_INTRODUCE") == 0
                    || nvc("VIEW_OF_LHOMME_MORE", "BINOCULARS") == 0)
            {
                flash("PLO");
                more = true;
            }
            if topic("LARRY", "T_TEMPLARS") < 2 {
                flash("LHE");
                more = true;
            }
            let train = nvc("ARRIVALS_IN_CU", "THINK") > 0 || topic("MARCIE", "T_TRAIN_FROM_NAPLES") > 0;
            if !train || nvc("TAXI_DRIVER", "WALLET") < 2 {
                flash("TR1");
                more = true;
            }
            if tb == "104P" && !more {
                flash("RLC");
            }
        }
        "106P" => match var("TwoMenState") {
            ..4 => flash("RLC"),
            4 => flash("PLO"),
            5 => flash("LHE"),
            _ => flash("RLC"),
        },
        "202P" => {
            let mut first_part = false;
            if var("FiveMinTimer202p") != 6 {
                flash("LHE");
                first_part = true;
            }
            if nvc("KNEE_INDENTS", "THINK") < 2 {
                flash("ARM");
                first_part = true;
            }
            if !first_part {
                if state.actor_locations.get("ESTELLE").is_some_and(|l| l == "MAP") {
                    flash("PLO");
                }
                if topic("GRACE_N_MOSE", "T_BOOK") < 2 {
                    flash("RLC");
                } else {
                    let good_id = has("FAKE_ID_REPORTER") && !has("FAKE_ID_NYT_REP");
                    if topic("JEAN", "T_WAKEUPCALL") == 0 || !good_id {
                        flash("RLC");
                    }
                    if topic("MONTREAUX", "T_VITICULTURE") < 2 {
                        flash("CSE");
                    } else {
                        flash("RLC");
                    }
                }
            }
        }
        "202A" => flash(if has("BLOODLINE_MANUSCRIPT") { "RLC" } else { "PLO" }),
        "307A" => {
            flash("RLC");
            if !flag("UseCoordLER") || nvc("CLUE_NOTE_1", "PICKUP") == 0 {
                flash("LER");
            }
        }
        "310A" => {
            let mut done = true;
            if topic("LARRY", "T_MANUSCRIPT") == 0 {
                flash("LHE");
                done = false;
            }
            if state.location_count("GABRIEL", "WDB") == 0 {
                flash("LHM");
                done = false;
            }
            if done {
                flash("RLC");
            }
        }
        "312P" => {
            flash("RLC");
            if nvc("CLUE_NOTE_2", "PICKUP") == 0 {
                flash("MCB");
            }
            if nvc("CLUE_NOTE_4", "PICKUP") == 0 {
                flash("BEC");
            }
            let site_note = nvc("CLUE_NOTE_3", "PICKUP") != 0;
            if flag("MarkedTheSite") && !site_note {
                flash("TRE");
            }
            if site_note {
                if !has("BINOCULARS") {
                    flash("WOD");
                }
                if nvc("VIEW_OF_ORANGE_ROCK", "BINOCULARS") == 0 {
                    flash("PLO");
                }
            }
        }
        "303P" => {
            let manuscript = has("BLOODLINE_MANUSCRIPT");
            let met_james = state.location_count("GABRIEL", "BET") > 0;
            if !met_james {
                flash(if manuscript { "RLC" } else { "BMB" });
            } else {
                flash("CSE");
            }
        }
        _ => {}
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROADS: &str = "NodeBegin A\n Location 10,10\n LinksBegin\n  B AB TRUE\n LinksEnd\nNodeEnd\n\
        NodeBegin B\n Location 30,10\n LinksBegin\n  A AB FALSE\n LinksEnd\nNodeEnd\n\
        SegmentBegin AB\n Point 15,10\n Point 20,10\n Point 25,10\nSegmentEnd\n";

    #[test]
    fn hints_point_where_the_story_waits() {
        let mut state = GameState::new("202A", "MAP");
        assert_eq!(hints(&state), ["PLO"]);
        state.give_item(&state.ego.clone(), "BLOODLINE_MANUSCRIPT");
        assert_eq!(hints(&state), ["RLC"]);
        let mut state = GameState::new("106P", "MAP");
        state.variables.insert("TWOMENSTATE".into(), 5);
        assert_eq!(hints(&state), ["LHE"]);
        assert!(hints(&GameState::new("110A", "MAP")).is_empty());
    }

    #[test]
    fn roads_run_both_ways() {
        let roads = Roads::parse(ROADS);
        assert_eq!(roads.point("a"), Some((10.0, 10.0)));
        assert_eq!(roads.road(0, 1), [(15.0, 10.0), (20.0, 10.0), (25.0, 10.0)]);
        assert_eq!(roads.road(1, 0), [(25.0, 10.0), (20.0, 10.0), (15.0, 10.0)]);
        let mut blip = Blip::new(EGO, (0.0, 0.0));
        blip.drive(&roads, &["A", "B"], false);
        assert_eq!(blip.at, (10.0, 10.0));
        for _ in 0..10 {
            blip.advance();
        }
        assert!(blip.arrived());
        assert_eq!(blip.at, (30.0, 10.0));
    }

    #[test]
    fn places_open_with_the_story() {
        let roads = "";
        let mut state = GameState::new("102P", "MAP");
        let map = MapScreen::new(roads, &state, "PLO", Follow::None);
        assert!(
            map.shown.contains(&"PLO")
                && !map.shown.contains(&"LER")
                && !map.shown.contains(&"CSE")
        );
        state
            .noun_verbs
            .insert(("GABRIEL".into(), "WILKES".into(), "FOLLOW".into()), 1);
        let map = MapScreen::new(roads, &state, "PLO", Follow::None);
        assert!(map.shown.contains(&"LER"));
        // While following him, it opens only when he gets there.
        let map = MapScreen::new(roads, &state, "PLO", Follow::Wilkes);
        assert!(!map.shown.contains(&"LER"));
        let late = GameState::new("202A", "MAP");
        assert!(
            MapScreen::new(roads, &late, "R25", Follow::None)
                .shown
                .contains(&"CSE")
        );
    }

    #[test]
    fn pressing_places() {
        let state = GameState::new("212P", "MAP");
        assert_eq!(press(&state, "PL6"), Ok(("CSE".into(), Some(15))));
        assert_eq!(press(&state, "TRE"), Ok(("PLO".into(), Some(12))));
        let grace = GameState::new("312P", "MAP");
        assert!(press(&grace, "TR1").is_err());
    }
}
