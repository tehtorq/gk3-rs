//! The four angels in the church (CHU): touching the angels in turn lays a
//! laser dot on each and a line from the last, and tracing the square they
//! make (its four sides, no diagonals) solves it. The original's `Angels`
//! scene (`custom=Angels`, 0x41e73e; `docs/re-angels.md`), which the scripts
//! call as `wait CallSceneFunction("Angel1")` to `("Angel4")` and `("Erase")`.

/// What the room has to do about a touch.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    Show(String, bool),
    /// `FOUR_ANGELS`'s count for a verb (`ERASE`), set; how the scripts
    /// know what was done.
    SetCount(&'static str, i32),
    /// And one added to it (`TRACE`).
    AddCount(&'static str),
    /// A line said, not waited for.
    Say(&'static str),
}

/// What the calling script waits for.
#[derive(Debug, Clone, PartialEq)]
pub enum Wait {
    Nothing,
    /// A line said, until it is over.
    Line(&'static str),
    /// The solved square, kept up this many seconds (0x41eda3), then
    /// erased (`Angels::erase`) before the script goes on (0x41e9fe).
    Solved(f64),
}

/// `angelLineFlags` with the square's four sides and nothing else.
const SQUARE: u32 = 0x158;
/// With all six lines.
const ALL_LINES: u32 = 0x1f8;
/// How long the solved square is kept up.
const SOLVED_SECONDS: f64 = 5.0;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Angels {
    /// One bit for each line drawn (`angelLineFlags`).
    lines: u32,
    /// The angel touched last, 1 to 4; 0 for none (`lastAngelNum`).
    last: u32,
    /// Never set by the original, so the mess line is said every time.
    spoke_mess: bool,
    very_first: bool,
    completed: bool,
}

impl Default for Angels {
    fn default() -> Self {
        Self {
            lines: 0,
            last: 0,
            spoke_mess: false,
            very_first: true,
            completed: false,
        }
    }
}

fn dot_model(n: u32) -> String {
    format!("CHU_LASERDOT0{n}")
}

/// The line between two angels: its bit's place and its model's number
/// (0x41eb71). The sum of their numbers, but 2 for the second and third
/// (whose sum is the first and fourth's).
fn line(a: u32, b: u32) -> (u32, u32) {
    let sum = if a + b == 5 && a.abs_diff(b) == 1 { 2 } else { a + b };
    let model = match sum {
        2 => 3,
        3 => 2,
        4 => 5,
        5 => 1,
        6 => 6,
        _ => 4,
    };
    (sum, model)
}

impl Angels {
    /// Runs one of the church's functions: `Angel1` to `Angel4` (any name
    /// so starting, the fifth letter the angel), `Erase`, or the room's
    /// `init`, which starts over as on entering (0x41e97a). `None` if it is
    /// not one of them. `traced` is whether `FOUR_ANGELS:TRACE` has a count;
    /// `grace` whether she is the one touching.
    pub fn call(&mut self, function: &str, grace: bool, traced: bool) -> Option<(Vec<Effect>, Wait)> {
        let f = function.to_ascii_lowercase();
        if f == "init" {
            *self = Self::default();
            return Some((Vec::new(), Wait::Nothing));
        }
        let angel = f.strip_prefix("angel").map(|rest| rest.bytes().next());
        if angel.is_none() && !f.starts_with("erase") {
            return None;
        }
        // Done once, nothing more is drawn (0x41ea31).
        if self.lines == SQUARE || traced {
            return Some((Vec::new(), Wait::Nothing));
        }
        if let Some(digit) = angel {
            let n = digit.map_or(0, |d| (d as u32).wrapping_sub(b'0' as u32));
            return Some(self.touch(n, grace));
        }
        Some((self.erase(), Wait::Nothing))
    }

    /// Everything off and forgotten (0x41ef93).
    pub fn erase(&mut self) -> Vec<Effect> {
        let mut out: Vec<Effect> = (1..=4).map(|n| Effect::Show(dot_model(n), false)).collect();
        out.extend((1..=6).map(|n| Effect::Show(format!("CHU_LASER0{n}"), false)));
        out.push(Effect::SetCount("ERASE", 0));
        self.lines = 0;
        self.last = 0;
        out
    }

    fn touch(&mut self, n: u32, grace: bool) -> (Vec<Effect>, Wait) {
        let mut out = Vec::new();
        if self.last == 0 {
            out.push(Effect::SetCount("ERASE", 1));
            out.push(Effect::Show(dot_model(n), true));
            self.last = n;
            if std::mem::take(&mut self.very_first) && grace {
                return (out, Wait::Line("18P1F0M021"));
            }
            return (out, Wait::Nothing);
        }
        // Only the dot touched last is lit.
        out.push(Effect::Show(dot_model(self.last), false));
        out.push(Effect::Show(dot_model(n), true));
        if self.last == n {
            return (out, Wait::Nothing);
        }
        let (sum, model) = line(self.last, n);
        out.push(Effect::Show(format!("CHU_LASER0{model}"), true));
        self.lines |= 2 << sum;
        // 0x41eda3.
        if self.lines == SQUARE {
            if !std::mem::replace(&mut self.completed, true) {
                out.push(Effect::Say(if grace { "18P9M0MCE1" } else { "18L9M0MZ81" }));
                out.push(Effect::AddCount("TRACE"));
            }
            return (out, Wait::Solved(SOLVED_SECONDS));
        }
        if self.lines == ALL_LINES {
            if !self.spoke_mess {
                return (out, Wait::Line(if grace { "18P9M0MTB1" } else { "18L9M0MZ82" }));
            }
            return (out, Wait::Nothing);
        }
        self.last = n;
        (out, Wait::Nothing)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shown(effects: &[Effect]) -> Vec<String> {
        effects
            .iter()
            .filter_map(|e| match e {
                Effect::Show(m, true) => Some(m.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn tracing_the_square_solves_it() {
        let mut a = Angels::default();
        for n in ["Angel1", "Angel2", "Angel3", "Angel4"] {
            let (_, wait) = a.call(n, false, false).unwrap();
            assert_eq!(wait, Wait::Nothing);
        }
        // Back to the first closes the square.
        let (out, wait) = a.call("Angel1", false, false).unwrap();
        assert!(shown(&out).contains(&"CHU_LASER01".to_string()));
        assert!(out.contains(&Effect::Say("18L9M0MZ81")));
        assert!(out.contains(&Effect::AddCount("TRACE")));
        assert_eq!(wait, Wait::Solved(5.0));
        // Solved, a touch does nothing.
        assert_eq!(a.call("Angel2", false, false).unwrap(), (Vec::new(), Wait::Nothing));
    }

    #[test]
    fn only_the_last_dot_is_lit() {
        let mut a = Angels::default();
        let (out, wait) = a.call("Angel3", true, false).unwrap();
        assert_eq!(shown(&out), vec!["CHU_LASERDOT03"]);
        assert_eq!(wait, Wait::Line("18P1F0M021"));
        let (out, _) = a.call("Angel2", true, false).unwrap();
        assert!(out.contains(&Effect::Show("CHU_LASERDOT03".into(), false)));
        assert_eq!(shown(&out), vec!["CHU_LASERDOT02", "CHU_LASER03"]);
    }

    #[test]
    fn every_line_is_a_mess() {
        let mut a = Angels::default();
        let mut last = Wait::Nothing;
        for n in ["Angel1", "Angel3", "Angel2", "Angel4", "Angel1", "Angel2", "Angel3", "Angel4"] {
            last = a.call(n, false, false).unwrap().1;
        }
        assert_eq!(last, Wait::Line("18L9M0MZ82"));
        let (out, _) = a.call("Erase", false, false).unwrap();
        assert!(out.contains(&Effect::SetCount("ERASE", 0)));
        assert_eq!(a.call("Look", false, false), None);
    }

    #[test]
    fn lines_by_their_ends() {
        assert_eq!(line(2, 3), (2, 3));
        assert_eq!(line(1, 2), (3, 2));
        assert_eq!(line(1, 3), (4, 5));
        assert_eq!(line(4, 1), (5, 1));
        assert_eq!(line(2, 4), (6, 6));
        assert_eq!(line(3, 4), (7, 4));
    }
}
