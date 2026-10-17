//! Grace's GPS (the `COORDINATE_FIXING_DEVICE`) on Day 3, as the original's
//! `CoordinateDevice` rooms have it (`custom=CoordinateDevice`: LER at 307A,
//! MCF and BEC at 312P; 0x46ee8f and on, `docs/re-gps.md`).
//!
//! The story turns it on and off with `CallSceneFunction("on")` and
//! `("off")`. On, a panel with a map of the room comes up in the top left
//! corner, its power off; a second later it comes on (`GPSOn` is set):
//! lines cross on the map where the player stands, and the latitude and
//! longitude read out, both every quarter of a second. Off, or its power
//! button pressed, the display goes out and a second later the panel goes.

use gk3_formats::ini::{Document, Split};

/// How long the display takes to come on, or the panel to go, in seconds.
const TURN: f64 = 1.0;
/// How often the reading is taken, in seconds.
const UPDATE: f64 = 0.25;
/// Where the panel's top left is, in the art's pixels.
pub const PANEL_AT: (f32, f32) = (2.0, 2.0);

/// How a room's map lies over the world, and the spot it reads out from:
/// a section of `GPS.TXT`, named for the room.
#[derive(Debug, Clone, PartialEq)]
pub struct Calibration {
    /// The panel's picture, before the size's ending (`gpsLER`).
    pub name: String,
    /// Degrees clockwise from the world's x axis to north.
    pub ang_x_to_n: f32,
    /// World units across the map.
    pub world_width: f32,
    /// Where the world's origin is on the map, as fractions of its width.
    pub origin: (f32, f32),
    /// A point in the world, and its latitude and longitude.
    pub sig: (f32, f32),
    pub lat: (i32, i32, f32),
    pub lng: (i32, i32, f32),
}

/// One of the panel's three sizes: a section of `GPS.TXT`, in pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    /// The ending of each picture of this size (`_L`).
    pub ext: String,
    pub v_line: String,
    pub h_line: String,
    pub target: String,
    pub font: String,
    /// The map's top left, on screen.
    pub corner: (i32, i32),
    pub map_width: i32,
    pub target_width: i32,
    /// Where the readings start, and the height of each.
    pub text_x: i32,
    pub lat_y: i32,
    pub lng_y: i32,
    pub power_at: (i32, i32),
    pub power_up: String,
    pub power_down: String,
    pub power_hover: String,
    pub power_disabled: String,
    /// The screen height the size was drawn for.
    pub screen_height: u32,
}

/// The device's settings for a room, as `GPS.TXT` has them.
#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub calibration: Calibration,
    /// Small, medium and large.
    pub layouts: [Layout; 3],
}

impl Config {
    /// The settings for a room (`ler`, `mcf`, `bec`), if it has a section.
    pub fn parse(bytes: &[u8], room: &str) -> Option<Self> {
        let doc = Document::parse(bytes, Split::Whole);
        let section = |name: &str| {
            doc.sections
                .iter()
                .find(|s| s.name.trim().eq_ignore_ascii_case(name))
        };
        let get = |s: &gk3_formats::ini::Section, key: &str| {
            s.lines
                .iter()
                .filter_map(|l| l.get(key))
                .next()
                .map(|v| v.trim().to_string())
        };
        let float = |s, key| {
            get(s, key)
                .and_then(|v| v.parse::<f32>().ok())
                .unwrap_or(0.0)
        };
        let int = |s, key| get(s, key).and_then(|v| v.parse::<i32>().ok()).unwrap_or(0);
        let room = section(room)?;
        let calibration = Calibration {
            name: get(room, "name")?,
            ang_x_to_n: float(room, "angXtoN"),
            world_width: float(room, "worldWidth"),
            origin: (float(room, "mapOriginXPct"), float(room, "mapOriginYPct")),
            sig: (float(room, "sigx"), float(room, "sigz")),
            lat: (
                int(room, "sigLatDeg"),
                int(room, "sigLatMin"),
                float(room, "sigLatSec"),
            ),
            lng: (
                int(room, "sigLngDeg"),
                int(room, "sigLngMin"),
                float(room, "sigLngSec"),
            ),
        };
        let layout = |name: &str, screen_height: u32| -> Option<Layout> {
            let s = section(name)?;
            let text = |key| get(s, key).unwrap_or_default();
            let pair = |key| {
                let v = get(s, key)?;
                let (a, b) = v.split_once(',')?;
                Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
            };
            Some(Layout {
                ext: text("nameExt"),
                v_line: text("vLineFile"),
                h_line: text("hLineFile"),
                target: text("targetFile"),
                font: text("fontFile"),
                corner: (int(s, "cornerWidth"), int(s, "cornerHeight")),
                map_width: int(s, "mapWidth"),
                target_width: int(s, "targetWidth"),
                text_x: int(s, "textStartWidth"),
                lat_y: int(s, "latHeight"),
                lng_y: int(s, "lngHeight"),
                power_at: pair("powerBtnPos").unwrap_or((0, 0)),
                power_up: text("powerBtnUpSprite"),
                power_down: text("powerBtnDownSprite"),
                power_hover: text("powerBtnHoverSprite"),
                power_disabled: text("powerBtnDisabledSprite"),
                screen_height,
            })
        };
        Some(Self {
            calibration,
            layouts: [
                layout("small", 480)?,
                layout("medium", 600)?,
                layout("large", 768)?,
            ],
        })
    }

    /// The size drawn on a screen this tall. The original picks by width:
    /// 1024 and over large, 800 and over medium, else small (0x46f7ca),
    /// drawing it unscaled; ours picks by the 4:3 screen of that height and
    /// scales it to the window as the rest of the interface.
    pub fn layout(&self, height: u32) -> &Layout {
        let i = match height {
            768.. => 2,
            600.. => 1,
            _ => 0,
        };
        &self.layouts[i]
    }
}

/// A point turned about the world's origin so north is up, as the original
/// turns it (0x5410b0): by the angle from the x axis to north, backwards.
fn north_up(cal: &Calibration, x: f32, z: f32) -> (f32, f32) {
    let a = cal.ang_x_to_n * -0.017_453_292;
    let (s, c) = a.sin_cos();
    (x * c + z * s, z * c - x * s)
}

/// Where the lines cross on the map for a player at (x, z), in pixels from
/// its top left (0x4703c1). The origin's height is taken up from the map's
/// foot, as the original's code has it, though `GPS.TXT` says down.
pub fn cross(cal: &Calibration, map_width: i32, (x, z): (f32, f32)) -> (i32, i32) {
    let (rx, rz) = north_up(cal, x, z);
    let w = map_width as f32;
    let scale = w / cal.world_width;
    let across = (w * cal.origin.0) as i32 + (rx * scale) as i32;
    let down = map_width - (w * cal.origin.1) as i32 - (rz * scale) as i32;
    (across, down)
}

/// The latitude and longitude read out for a player at (x, z), as the
/// original writes them (0x4704da): a thousandth of a second of arc for
/// each unit north or east of the room's point, the minutes carried once.
pub fn reading(cal: &Calibration, (x, z): (f32, f32)) -> (String, String) {
    let (east, north) = north_up(cal, x - cal.sig.0, z - cal.sig.1);
    let carry = |(deg, min, sec): (i32, i32, f32), by: f32| {
        let (mut min, mut sec) = (min, sec + by * 0.001);
        if sec < 0.0 {
            sec += 60.0;
            min -= 1;
        } else if sec > 60.0 {
            sec -= 60.0;
            min += 1;
        }
        format!("{deg:02}\u{b0}{min:02}'{sec:05.2}\"")
    };
    (carry(cal.lat, north), carry(cal.lng, east))
}

/// What the device has the game do.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// Sets or clears a flag (`GPSOn`, `NoGPSTurnOff`).
    Flag(&'static str, bool),
    /// Runs a line of script.
    Script(String),
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Turn {
    On,
    Away,
}

/// The device in a room.
#[derive(Debug, Clone)]
pub struct Gps {
    pub config: Config,
    /// The room's script with `HideCoord`, called when the power is turned
    /// off by its button.
    script: &'static str,
    /// The panel is up.
    pub shown: bool,
    /// Its display is on.
    pub on: bool,
    /// What happens when, as the original's `mTurnOnOffTimer` and `State`.
    pending: Option<(f64, Turn)>,
    /// The power button turned it off: `HideCoord` is called once it goes.
    play_sheep: bool,
    last_update: f64,
    /// Where the player was when last read.
    pub fix: Option<(f32, f32)>,
}

impl Gps {
    /// The device for a room, if `GPS.TXT` has it.
    pub fn new(gps_txt: &[u8], location: &str) -> Option<Self> {
        let room = location.to_ascii_lowercase();
        let config = Config::parse(gps_txt, &room)?;
        // As the original picks by the room (0x4702a3).
        let script = match room.as_str() {
            "ler" => "ler307a",
            "mcf" => "mcf_all",
            "bec" => "bec312p",
            _ => "",
        };
        Some(Self {
            config,
            script,
            shown: false,
            on: false,
            pending: None,
            play_sheep: false,
            last_update: f64::NEG_INFINITY,
            fix: None,
        })
    }

    /// `CallSceneFunction("on")`: the panel comes up, off, and comes on a
    /// second later (0x46f3c5). Nothing if it is up already. Returns how
    /// long until it is on.
    pub fn turn_on(&mut self, now: f64) -> (Option<f64>, Vec<Effect>) {
        if self.shown {
            return (None, Vec::new());
        }
        self.shown = true;
        self.on = false;
        self.pending = Some((now + TURN, Turn::On));
        (Some(TURN), vec![Effect::Flag("NoGPSTurnOff", false)])
    }

    /// `CallSceneFunction("off")`: the display goes out, and the panel a
    /// second later. Nothing if the display is not on.
    pub fn turn_off(&mut self, now: f64) -> Option<f64> {
        if !self.on {
            return None;
        }
        self.on = false;
        self.pending = Some((now + TURN, Turn::Away));
        Some(TURN)
    }

    /// The power button pressed (0x470167): as `off`, then the room's
    /// `HideCoord` once the panel has gone. Not while the display is off,
    /// or the story has set `NoGPSTurnOff`. True if it turned off.
    pub fn press_power(&mut self, now: f64, no_turn_off: bool) -> bool {
        if no_turn_off || self.turn_off(now).is_none() {
            return false;
        }
        self.play_sheep = true;
        true
    }

    /// Up and on at once, as a saved game in a room with `GPSOn` set is
    /// put back (0x4701d2).
    pub fn restore(&mut self) {
        self.shown = true;
        self.on = true;
        self.pending = None;
    }

    /// Leaving the room (0x46f391): the device is put away. True if it was
    /// up, and so `GPSOn` is to be cleared.
    pub fn leave(&mut self) -> bool {
        let was = self.shown;
        self.shown = false;
        self.on = false;
        self.pending = None;
        was
    }

    /// Each frame (0x47021d): the display comes on or the panel goes when
    /// due; while `GPSOn`, the player's place is read every quarter second.
    pub fn update(&mut self, now: f64, gps_on: bool, player: Option<(f32, f32)>) -> Vec<Effect> {
        let mut effects = Vec::new();
        if let Some((due, turn)) = self.pending
            && now > due
        {
            self.pending = None;
            match turn {
                Turn::On => {
                    self.on = true;
                    effects.push(Effect::Flag("GPSOn", true));
                }
                Turn::Away => {
                    self.shown = false;
                    effects.push(Effect::Flag("GPSOn", false));
                    if std::mem::take(&mut self.play_sheep) && !self.script.is_empty() {
                        effects.push(Effect::Script(format!(
                            "wait CallSheep(\"{}\", \"HideCoord\");",
                            self.script
                        )));
                    }
                }
            }
        }
        let on = gps_on || effects.contains(&Effect::Flag("GPSOn", true));
        if on && now - UPDATE > self.last_update {
            self.last_update = now;
            if player.is_some() {
                self.fix = player;
            }
        }
        effects
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TXT: &[u8] = b"[ler]\r\nname = gpsLER\r\nangXtoN = 278 // degrees\r\nworldWidth = 2904\r\n\
        mapOriginXPct = .1902\r\nmapOriginYPct = .6780\r\nsigx = 204\r\nsigz = 517\r\n\
        sigLatDeg = 42\r\nsigLatMin = 54\r\nsigLatSec = 56\r\nsigLngDeg = 2\r\nsigLngMin = 18\r\n\
        sigLngSec = 47\r\n\
        [large]\r\nnameExt = _L\r\nvLineFile = gpsvline_L\r\ncornerWidth = 11\r\ncornerHeight = 10\r\n\
        mapWidth = 205\r\ntargetWidth = 20\r\npowerBtnPos = 181, 290\r\n\
        [medium]\r\nnameExt = _M\r\n[small]\r\nnameExt = _S\r\n";

    #[test]
    fn reads_a_room_and_its_sizes() {
        let c = Config::parse(TXT, "ler").unwrap();
        assert_eq!(c.calibration.name, "gpsLER");
        assert_eq!(c.calibration.ang_x_to_n, 278.0);
        assert_eq!(c.calibration.lng, (2, 18, 47.0));
        assert_eq!(c.layout(1080).power_at, (181, 290));
        assert_eq!(c.layout(1080).corner, (11, 10));
        assert_eq!(c.layout(600).ext, "_M");
        assert_eq!(c.layout(480).ext, "_S");
        assert!(Config::parse(TXT, "mcb").is_none());
    }

    /// At the room's point the reading is the point's own.
    #[test]
    fn reads_the_point_itself() {
        let c = Config::parse(TXT, "ler").unwrap().calibration;
        let (lat, lng) = reading(&c, (204.0, 517.0));
        assert_eq!(lat, "42\u{b0}54'56.00\"");
        assert_eq!(lng, "02\u{b0}18'47.00\"");
    }

    /// A long way north the seconds run over into the next minute, once.
    #[test]
    fn carries_the_minutes() {
        let mut c = Config::parse(TXT, "ler").unwrap().calibration;
        c.ang_x_to_n = 0.0;
        let (lat, lng) = reading(&c, (204.0, 517.0 + 5000.0));
        assert_eq!(lat, "42\u{b0}55'01.00\"");
        assert_eq!(lng, "02\u{b0}18'47.00\"");
        let (_, lng) = reading(&c, (204.0 - 50_000.0, 517.0));
        assert_eq!(lng, "02\u{b0}17'57.00\"");
    }

    /// The world's origin lies at the map's origin, measured as the code
    /// measures it.
    #[test]
    fn crosses_at_the_origin() {
        let c = Config::parse(TXT, "ler").unwrap().calibration;
        assert_eq!(cross(&c, 205, (0.0, 0.0)), (38, 205 - 138));
    }

    #[test]
    fn turns_on_and_off_a_second_apart() {
        let mut g = Gps::new(TXT, "LER").unwrap();
        let (wait, effects) = g.turn_on(10.0);
        assert_eq!(wait, Some(1.0));
        assert_eq!(effects, vec![Effect::Flag("NoGPSTurnOff", false)]);
        assert!(g.shown && !g.on);
        assert!(g.update(10.5, false, Some((1.0, 2.0))).is_empty());
        assert_eq!(
            g.update(11.1, false, Some((1.0, 2.0))),
            vec![Effect::Flag("GPSOn", true)]
        );
        assert!(g.on);
        assert_eq!(g.fix, Some((1.0, 2.0)));
        assert!(!g.press_power(12.0, true));
        assert!(g.press_power(12.0, false));
        assert!(g.shown && !g.on);
        assert_eq!(
            g.update(13.1, true, None),
            vec![
                Effect::Flag("GPSOn", false),
                Effect::Script("wait CallSheep(\"ler307a\", \"HideCoord\");".into()),
            ]
        );
        assert!(!g.shown);
    }
}
