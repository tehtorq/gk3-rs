//! TE5's invisible bridge: nine stones across the chasm that glint, one at a
//! time in a shuffled order, while Gabriel is on them; a stone he lands on
//! glows, and he must jump on before the glow ends. As the original's
//! bridge code (`GK3.exe` 0x445688 each frame, 0x445e86 a click, 0x44594d
//! the steps of a jump, 0x4463d0 the stones' places; `docs/re-bridge.md`).
//!
//! The room's scripts take over on the way in (`PreFirstJump$`), on a fall
//! (`FallDie$` when a stone's glow ends under him, `BishopFallDie$` for a
//! jump to anywhere he cannot reach) and at the far end
//! (`GabeCrossedBridge$`).

use glam::{Mat4, Vec3};

use crate::mechanism::Room;

const STONES: usize = 9;
/// Where each stone lies (0x6ec920).
const PLACES: [(f32, f32); STONES] = [
    (-80.0, -180.0),
    (-35.0, -135.0),
    (-125.0, -90.0),
    (-125.0, 0.0),
    (-80.0, 45.0),
    (-35.0, 135.0),
    (-125.0, 180.0),
    (-125.0, 225.0),
    (-80.0, 270.0),
];
/// Before the first stone: on the near side. Beyond the last: across.
const NEAR: isize = -1;
const FAR: isize = 9;
/// The near side's spot, and how near Gabriel must stand to it, across
/// plus along, for the first stone to glint.
const START: &str = "STARTPOINT";
const START_REACH: i32 = 30;
/// How often he is looked for there, and how often the first stone glints.
const START_CHECK: f64 = 1.0;
const START_GLINT: f64 = 4.0;
/// On the stones, one glints every so often, each once in a shuffled round.
const GLINT_EVERY: f64 = 0.425;

/// What the original's bridge sends itself as its animations end.
const DONE: u32 = 1;
const GLINT_ENDS: u32 = 10;
const GLOW_ENDS: u32 = 20;
const FALL: u32 = 0x1f;

/// The jump from one place to another (stones 0 to 8, or the ends), as the
/// original's table has it: a move's code, whose tens (less 100) name the
/// turn into it and whose units the jump. None for a jump he cannot make,
/// which is a fall.
fn move_code(from: isize, to: isize) -> Option<u32> {
    Some(match (from, to) {
        (NEAR, 0) | (6, 7) | (8, FAR) => 1101,
        (2, 3) => 1104,
        (7, 6) | (0, NEAR) => 1181,
        (3, 2) => 1184,
        (4, 5) | (6, 8) | (7, FAR) => 1113,
        (0, 1) | (3, 4) | (7, 8) => 1122,
        (2, 1) | (6, 5) => 1153,
        (0, 2) => 1213,
        (1, 2) | (5, 6) => 1233,
        (1, 0) | (4, 3) | (8, 7) => 1262,
        (5, 4) | (8, 6) | (1, NEAR) => 1273,
        _ => return None,
    })
}

/// The turn a move starts with (or, `back`, the one it ends with), as
/// 0x445d46 names it from the move's tens: none for 10; clockwise for the
/// teens, anticlockwise for the twenties, and the other way back.
fn turn(code: u32, back: bool) -> Option<String> {
    let t = (code % 1000) / 10;
    if t == 10 {
        return None;
    }
    let angle = match t % 10 {
        1 => "26",
        2 => "45",
        3 => "63",
        4 => "90",
        5 => "116",
        6 => "135",
        7 => "153",
        8 => "180",
        _ => return None,
    };
    let other_way = (t >= 20) != back;
    Some(format!("GabTe1TurnC{}{angle}", if other_way { "C" } else { "" }))
}

/// The jump itself, from the move's units.
fn leap(code: u32) -> String {
    let kind = match code % 10 {
        1 => "01sq",
        2 => "45",
        3 => "26Knight",
        _ => "02sq",
    };
    format!("GabTe5Jump{kind}")
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Bridge {
    clock: f64,
    /// Where he is, or is jumping to (curBlockPos).
    on: isize,
    /// A jump, a fall or the start of one is playing (bGabeAnimating).
    animating: bool,
    /// The stones glint in turn while he is on them (bGlimmerOn), from a
    /// new shuffle when due (bGlimmerStart).
    glimmer: bool,
    shuffle_due: bool,
    order: [usize; STONES],
    round_began: f64,
    last_turn: Option<usize>,
    /// When he was last looked for at the start, and the first stone last
    /// glinted for him there.
    checked: f64,
    beckoned: f64,
    approached: bool,
    /// A stone whose glow ended while he was jumping, to go once he lands.
    still_glowing: Option<usize>,
    shown: [bool; STONES],
    /// What the bridge has sent itself, due as animations end: once their
    /// time is up and the room has finished them.
    due: Vec<(f64, String, u32)>,
    /// Waiting for the scripts after a fall.
    dying: bool,
    seed: u64,
}

impl Default for Bridge {
    fn default() -> Self {
        Self {
            clock: 0.0,
            on: NEAR,
            animating: false,
            glimmer: false,
            shuffle_due: false,
            order: std::array::from_fn(|i| i),
            round_began: 0.0,
            last_turn: None,
            checked: f64::NEG_INFINITY,
            beckoned: f64::NEG_INFINITY,
            approached: false,
            still_glowing: None,
            shown: [false; STONES],
            due: Vec::new(),
            dying: false,
            // Its own sequence, so the story's random numbers stay as they
            // were.
            seed: 0x2545_f491_4f6c_dd1d,
        }
    }
}

pub fn stone_model(i: usize) -> String {
    format!("TE5SQ{:02}", i + 1)
}

fn stone_of(model: &str) -> Option<usize> {
    let n: usize = model.to_ascii_uppercase().strip_prefix("TE5SQ")?.parse().ok()?;
    (1..=STONES).contains(&n).then(|| n - 1)
}

impl Bridge {
    /// Lays the stones out, hidden (0x4463d0).
    pub fn start(&mut self, room: &mut dyn Room) {
        for (i, (x, z)) in PLACES.iter().enumerate() {
            room.set_world(&stone_model(i), Mat4::from_translation(Vec3::new(*x, 0.0, *z)));
            self.show(i, false, room);
        }
    }

    fn show(&mut self, i: usize, shown: bool, room: &mut dyn Room) {
        self.shown[i] = shown;
        room.show(&stone_model(i), shown);
    }

    fn random(&mut self, n: usize) -> usize {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        ((self.seed >> 11) % n as u64) as usize
    }

    /// Plays an animation, then sends a message, as the original's
    /// animations send one as they end.
    fn play_then(&mut self, anim: &str, message: u32, room: &mut dyn Room) {
        let d = room.play(anim, false);
        if d > 0.0 {
            self.due.push((self.clock + d as f64, anim.to_string(), message));
        } else {
            self.message(message, room);
        }
    }

    fn glint(&mut self, i: usize, room: &mut dyn Room) {
        self.show(i, true, room);
        self.play_then(&format!("te5glint{:02}", i + 1), GLINT_ENDS + i as u32, room);
    }

    pub fn tick(&mut self, dt: f32, room: &mut dyn Room) {
        self.clock += dt as f64;
        let now = self.clock;
        while let Some(k) = (0..self.due.len())
            .filter(|&k| self.due[k].0 <= now && !room.is_playing(&self.due[k].1))
            .min_by(|&a, &b| self.due[a].0.total_cmp(&self.due[b].0))
        {
            let (_, _, message) = self.due.remove(k);
            self.message(message, room);
        }

        if !self.glimmer {
            // On the near side: the first stone beckons while he stands by.
            if self.on == NEAR
                && now - self.checked > START_CHECK
                && let Some((start, _)) = room.spot(START)
            {
                self.checked = now;
                let at = room.ego_at();
                let off = (at.x as i32 - start.x as i32).abs() + (at.z as i32 - start.z as i32).abs();
                if off < START_REACH && now - self.beckoned > START_GLINT {
                    self.beckoned = now;
                    self.glint(0, room);
                    if !self.approached {
                        self.approached = true;
                        room.script("wait CallSheep(\"TE5\", \"PreFirstJump$\");");
                    }
                }
            }
            return;
        }
        if self.shuffle_due {
            let mut left: Vec<usize> = (0..STONES).collect();
            for slot in 0..STONES {
                let k = self.random(left.len());
                self.order[slot] = left.remove(k);
            }
            self.shuffle_due = false;
            self.round_began = now;
        }
        let turn = ((now - self.round_began) / GLINT_EVERY) as usize;
        if turn < STONES {
            if self.last_turn != Some(turn) {
                let i = self.order[turn];
                if i as isize != self.on {
                    self.glint(i, room);
                }
                self.last_turn = Some(turn);
            }
        } else {
            self.shuffle_due = true;
        }
    }

    /// What the bridge does as each of its animations ends (0x44594d).
    fn message(&mut self, message: u32, room: &mut dyn Room) {
        match message {
            DONE => {
                self.animating = false;
                if let Some(i) = self.still_glowing.take() {
                    self.show(i, false, room);
                }
            }
            m if (GLINT_ENDS..GLINT_ENDS + STONES as u32).contains(&m) => {
                let i = (m - GLINT_ENDS) as usize;
                if i as isize != self.on {
                    self.show(i, false, room);
                }
            }
            m if (GLOW_ENDS..GLOW_ENDS + STONES as u32).contains(&m) => {
                let i = (m - GLOW_ENDS) as usize;
                if self.animating {
                    self.still_glowing = Some(i);
                } else {
                    self.show(i, false, room);
                    if self.on == i as isize {
                        self.on = NEAR;
                        self.fall("FallDie$", room);
                    }
                }
            }
            FALL => {
                self.on = NEAR;
                self.fall("BishopFallDie$", room);
            }
            1000..=1999 => match turn(message, false) {
                Some(t) => self.play_then(&t, message + 1000, room),
                None => self.message(message + 1000, room),
            },
            2000..=2999 => self.play_then(&leap(message), message + 1000, room),
            3000..=3999 => {
                self.play_then("GabTe5Stand", message + 1000, room);
                if self.on == FAR {
                    room.set_flag("CrossedBridge", true);
                    room.script("wait CallSheep(\"TE5\", \"GabeCrossedBridge$\");");
                } else if self.on >= 0 {
                    let i = self.on as usize;
                    self.show(i, true, room);
                    self.play_then(&format!("te5glow{:02}", i + 1), GLOW_ENDS + i as u32, room);
                }
            }
            4000..=4999 => match turn(message, true) {
                Some(t) => self.play_then(&t, DONE, room),
                None => self.message(DONE, room),
            },
            _ => {}
        }
    }

    fn fall(&mut self, script: &str, room: &mut dyn Room) {
        self.animating = true;
        self.dying = true;
        room.script(&format!("wait CallSheep(\"TE5\", \"{script}\");"));
    }

    /// The scripts have dealt with a fall and put him back: the original's
    /// `restart` (0x44635f), the stones no longer glinting.
    pub fn recovered(&mut self) {
        self.dying = false;
        self.animating = false;
        self.glimmer = false;
        self.shuffle_due = false;
    }

    /// Where a click on a model sends him, as 0x445e86 reads it: a move's
    /// code (`FALL` for one he cannot make) and where it ends; None if the
    /// click means nothing to the bridge.
    fn target(&self, model: &str) -> Option<(u32, isize)> {
        if let Some(to) = stone_of(model) {
            let to = to as isize;
            if self.on == NEAR && to != 0 || self.on == FAR {
                return None;
            }
            return Some((move_code(self.on, to).unwrap_or(FALL), to));
        }
        if !(0..STONES as isize).contains(&self.on) {
            return None;
        }
        match model.to_ascii_uppercase().as_str() {
            "TE5_HITTEST_FLOOR" => Some((FALL, self.on)),
            "TE5HITTESTEND" | "CS5_FAR_FLOOR_BAD_" => {
                Some(move_code(self.on, FAR).map_or((FALL, self.on), |c| (c, FAR)))
            }
            "TE5_FLOOR" => move_code(self.on, NEAR).map(|c| (c, NEAR)),
            _ => None,
        }
    }

    pub fn cursor(&self, model: &str) -> Option<&'static str> {
        if self.animating {
            return None;
        }
        self.target(model).filter(|(c, _)| *c != FALL).map(|_| "C_ZOOM")
    }

    /// A click on something. True if the bridge takes it: every click while
    /// he is jumping or on the stones.
    pub fn click(&mut self, model: &str, room: &mut dyn Room) -> bool {
        if self.animating {
            return true;
        }
        let Some((code, to)) = self.target(model) else {
            return (0..STONES as isize).contains(&self.on) || stone_of(model).is_some();
        };
        if self.on == NEAR {
            // Onto the first stone: the camera cuts to the stones, he is
            // stood on the spot facing them, and they start to glint.
            room.cut("TILES");
            if let Some((at, heading)) = room.spot(START) {
                room.put_ego(at, heading.to_radians());
            }
            self.glimmer = true;
            self.shuffle_due = true;
        } else if to == NEAR || to == FAR {
            self.glimmer = false;
            self.shuffle_due = false;
        }
        self.animating = true;
        self.on = to;
        self.message(code, room);
        true
    }

    /// For headless runs: the stone to jump to now on the way across,
    /// waiting for each to show: the furthest showing stone he can reach,
    /// or the far end.
    pub fn advice(&self) -> Option<String> {
        if self.animating || self.dying || self.on == FAR {
            return None;
        }
        if self.on == NEAR {
            return self.shown[0].then(|| stone_model(0));
        }
        if move_code(self.on, FAR).is_some() {
            return Some("TE5HITTESTEND".into());
        }
        (self.on as usize + 1..STONES)
            .rev()
            .find(|&i| self.shown[i] && move_code(self.on, i as isize).is_some())
            .map(stone_model)
    }

    pub fn crossed(&self) -> bool {
        self.on == FAR && !self.animating
    }

    pub fn is_dying(&self) -> bool {
        self.dying
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stones_are_where_the_original_lays_them() {
        assert_eq!(PLACES[0], (-80.0, -180.0));
        assert_eq!(stone_of("te5sq09"), Some(8));
    }

    #[test]
    fn moves_are_the_originals() {
        // From the first stone: on to the second, or a knight's move to the
        // third; back to the near side; nowhere else.
        assert_eq!(move_code(0, 1), Some(1122));
        assert_eq!(move_code(0, 2), Some(1213));
        assert_eq!(move_code(0, NEAR), Some(1181));
        assert_eq!(move_code(0, 3), None);
        assert_eq!(move_code(NEAR, 1), None);
        // Only the last two stones reach the far side.
        assert_eq!(move_code(7, FAR), Some(1113));
        assert_eq!(move_code(8, FAR), Some(1101));
        assert_eq!(move_code(6, FAR), None);

        assert_eq!(turn(1101, false), None);
        assert_eq!(turn(1122, false).as_deref(), Some("GabTe1TurnC45"));
        assert_eq!(turn(1122, true).as_deref(), Some("GabTe1TurnCC45"));
        assert_eq!(turn(1213, false).as_deref(), Some("GabTe1TurnCC26"));
        assert_eq!(turn(1213, true).as_deref(), Some("GabTe1TurnC26"));
        assert_eq!(turn(1184, false).as_deref(), Some("GabTe1TurnC180"));
        assert_eq!(leap(1122), "GabTe5Jump45");
        assert_eq!(leap(1213), "GabTe5Jump26Knight");
        assert_eq!(leap(1104), "GabTe5Jump02sq");
        assert_eq!(leap(1181), "GabTe5Jump01sq");
    }
}
