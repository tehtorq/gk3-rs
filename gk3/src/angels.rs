//! The four angels in the church (CHU): touching the angels in turn lays
//! laser dots on them and lines between, and tracing the tilted square (all
//! four sides, no diagonals) solves it. G-Engine's `CHU_*` scene functions,
//! which the scripts call by name (`CallSceneFunction("Angel1")`).

/// What the room has to do about a touch.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    Show(String, bool),
    /// `Four_Angels`, verb, count: how the scripts know what was done.
    Count(&'static str, i32),
    /// Lines to say once the touching action is over, then wait two seconds
    /// and wipe the square, as the original keeps it up a moment.
    Solved(&'static str),
    /// The first dot, when Grace lays it.
    FirstDot(&'static str),
}

#[derive(Debug, Default)]
pub struct Angels {
    dots: [bool; 4],
    /// Left-top, top-right, right-bottom, bottom-left, top-bottom,
    /// left-right.
    edges: [bool; 6],
    last: Option<usize>,
    said_first: bool,
}

/// The edge between two dots (top 0, right 1, bottom 2, left 3).
fn edge(a: usize, b: usize) -> Option<usize> {
    let (a, b) = (a.min(b), a.max(b));
    Some(match (a, b) {
        (0, 3) => 0,
        (0, 1) => 1,
        (1, 2) => 2,
        (2, 3) => 3,
        (0, 2) => 4,
        (1, 3) => 5,
        _ => return None,
    })
}

fn dot_model(i: usize) -> String {
    format!("CHU_LASERDOT0{}", i + 1)
}

fn edge_model(i: usize) -> String {
    format!("CHU_LASER0{}", i + 1)
}

impl Angels {
    /// Runs one of the church's functions: `Erase`, or `Angel1` to `Angel4`
    /// (top, right, bottom, left). `None` if it is not one of them.
    pub fn call(&mut self, function: &str, ego: &str) -> Option<Vec<Effect>> {
        let f = function.to_ascii_lowercase();
        if f == "erase" {
            return Some(self.erase());
        }
        let n: usize = f.strip_prefix("angel")?.parse().ok()?;
        (1..=4).contains(&n).then(|| self.touch(n - 1, ego))
    }

    pub fn erase(&mut self) -> Vec<Effect> {
        self.dots = [false; 4];
        self.edges = [false; 6];
        self.last = None;
        let mut out: Vec<Effect> = (0..4).map(|i| Effect::Show(dot_model(i), false)).collect();
        out.extend((0..6).map(|i| Effect::Show(edge_model(i), false)));
        out.push(Effect::Count("ERASE", 0));
        out
    }

    fn touch(&mut self, dot: usize, ego: &str) -> Vec<Effect> {
        let mut out = Vec::new();
        self.dots[dot] = true;
        out.push(Effect::Show(dot_model(dot), true));
        if let Some(e) = self.last.and_then(|last| edge(last, dot)) {
            self.edges[e] = true;
            out.push(Effect::Show(edge_model(e), true));
        }
        self.last = Some(dot);
        out.push(Effect::Count("ERASE", 1));
        let grace = ego.eq_ignore_ascii_case("GRACE");
        if !self.said_first {
            self.said_first = true;
            if grace {
                out.push(Effect::FirstDot("18P1F0M021"));
            }
        }
        if self.edges == [true, true, true, true, false, false] {
            out.push(Effect::Count("TRACE", 1));
            out.push(Effect::Solved(if grace {
                "18P9M0MCE1"
            } else {
                "18L9M0MZ81"
            }));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracing_the_square_solves_it() {
        let mut a = Angels::default();
        for n in ["Angel1", "Angel2", "Angel3", "Angel4"] {
            assert!(
                !a.call(n, "GABRIEL")
                    .unwrap()
                    .iter()
                    .any(|e| matches!(e, Effect::Solved(_)))
            );
        }
        // Back to the top closes the square.
        let last = a.call("Angel1", "GABRIEL").unwrap();
        assert!(last.contains(&Effect::Show("CHU_LASER01".into(), true)));
        assert!(last.contains(&Effect::Solved("18L9M0MZ81")));
    }

    #[test]
    fn diagonals_spoil_it() {
        let mut a = Angels::default();
        for n in ["Angel1", "Angel3", "Angel2", "Angel1", "Angel4", "Angel3"] {
            let out = a.call(n, "GRACE").unwrap();
            assert!(!out.iter().any(|e| matches!(e, Effect::Solved(_))));
        }
        assert!(
            a.call("Erase", "GRACE")
                .unwrap()
                .contains(&Effect::Count("ERASE", 0))
        );
        assert_eq!(a.call("Init", "GRACE"), None);
    }
}
