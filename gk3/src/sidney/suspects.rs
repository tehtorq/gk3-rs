//! Suspects: a record per suspect, and the evidence linked to it. A
//! suspect's own licence plate shows their vehicle; their own print scores;
//! and Match Analysis compares an unknown print with every suspect whose
//! print is linked, until it finds the one it belongs to.
//!
//! As G-Engine's `SidneySuspects`. Which suspects' files are linked is part
//! of the story's state; the notes the original let one type are not kept.

use gk3_game::actions::parse_timeblock;
use gk3_game::state::GameState;

use super::Event;
use super::analyze::Menu;

pub const SUSPECTS: usize = 10;

/// Each suspect's licence-plate file (with its score and flag), and
/// fingerprint file (with its score); G-Engine's tables. A plate of `None`
/// means the vehicle always shows; `Some(999)` that it never does.
const PLATES: [(Option<u32>, &str, &str); SUSPECTS] = [
    (None, "", ""),
    (
        Some(32),
        "e_sidney_suspect_link_license_buchelli",
        "IDedBuchelliVehicle",
    ),
    (
        Some(33),
        "e_sidney_suspect_link_license_emilio",
        "IDedEmilioVehicle",
    ),
    (Some(999), "", "IDedAbbeVehicle"),
    (
        Some(34),
        "e_sidney_suspect_link_license_howard",
        "IDedHowardVehicle",
    ),
    (
        Some(34),
        "e_sidney_suspect_link_license_howard",
        "IDedEstelleVehicle",
    ),
    (
        Some(36),
        "e_sidney_suspect_link_license_wilkes",
        "IDedWilkesVehicle",
    ),
    (None, "", ""),
    (None, "", ""),
    (
        Some(35),
        "e_sidney_suspect_link_license_mosely",
        "IDedMoselyVehicle",
    ),
];
const PRINTS: [(Option<u32>, &str); SUSPECTS] = [
    (Some(2), "e_sidney_suspect_link_fingerprint_buthane"),
    (Some(1), "e_sidney_suspect_link_fingerprint_buchelli"),
    (None, ""),
    (Some(0), "e_sidney_suspect_link_fingerprint_abbe"),
    (Some(4), "e_sidney_suspect_link_fingerprint_howard"),
    (Some(3), "e_sidney_suspect_link_fingerprint_estelle"),
    (Some(6), "e_sidney_suspect_link_fingerprint_wilkes"),
    (Some(8), "e_sidney_suspect_link_fingerprint_larry"),
    (Some(5), "e_sidney_suspect_link_fingerprint_montreaux"),
    (Some(7), ""),
];
/// The print pictures Match Analysis shows for each suspect.
pub const PRINT_PICTURES: [&str; SUSPECTS] = [
    "S_MAD_FPRINT",
    "S_VIT_FPRINT",
    "",
    "S_ABE_FPRINT",
    "S_LHO_FPRINT",
    "S_EST_FPRINT",
    "S_WIL_FPRINT",
    "S_LAR_FPRINT",
    "S_MON_FPRINT",
    "S_MOS_FPRINT",
];

/// The unknown prints Match Analysis can place: the file, its picture,
/// the picture it is compared with, the scan movie, whose it is, what is
/// said and scored, the flag, and the picture once matched.
struct Unknown {
    file: u32,
    print: &'static str,
    compare: &'static str,
    movie: &'static str,
    owner: usize,
    line: &'static str,
    score: &'static str,
    flag: &'static str,
    matched: &'static str,
}

const UNKNOWNS: &[Unknown] = &[
    Unknown {
        file: 18,
        print: "EST_LSR_PRINT",
        compare: "EST_LSR_COMPARE",
        movie: "EstLSRScan.avi",
        owner: 5,
        line: "02OX660SJ1",
        score: "e_sidney_suspect_link_fingerprint_lsr_unknown",
        flag: "MatchedEstelle",
        matched: "EST_RED_LSR_PRINT",
    },
    Unknown {
        file: 11,
        print: "BUCH_BLD_PRINT",
        compare: "BUCH_BLD_COMPARE",
        movie: "BuchBldScan.avi",
        owner: 1,
        line: "027X65Q3L2",
        score: "e_sidney_analysis_link_manuscript_prints_buchelli",
        flag: "MatchedBuchelli",
        matched: "BUCH_RED_BLD_PRINT",
    },
    Unknown {
        file: 12,
        print: "BUTH_BLD_PRINT",
        compare: "BUTH_BLD_COMPARE",
        movie: "ButhBldScan.avi",
        owner: 0,
        line: "027X65Q3L1",
        score: "e_sidney_analysis_link_manuscript_prints_buthane",
        flag: "MatchedButhane",
        matched: "BUTH_RED_BLD_PRINT",
    },
    Unknown {
        file: 13,
        print: "MOS_BLD_PRINT",
        compare: "MOS_BLD_COMPARE",
        movie: "MosBldScan.avi",
        owner: 9,
        line: "027X65Q3L3",
        score: "e_sidney_analysis_link_manuscript_prints_mosley",
        flag: "MatchedMosely",
        matched: "MOS_RED_BLD_PRINT",
    },
];

/// Match Analysis under way.
#[derive(Debug, Clone, PartialEq)]
pub enum Matching {
    /// The scan movie plays.
    Scanning,
    /// Comparing with a suspect until a time.
    Comparing {
        suspect: usize,
        until: f64,
    },
    Found(usize),
    None,
}

#[derive(Debug, Clone)]
pub struct Match {
    pub file: u32,
    pub phase: Matching,
    /// The print picture shown beside the scan.
    pub picture: &'static str,
}

#[derive(Debug, Default)]
pub struct Suspects {
    pub open: Option<usize>,
    pub file: Option<u32>,
    /// Which linked file is picked, by suspect.
    pub picked: [Option<usize>; SUSPECTS],
    pub matching: Option<Match>,
}

fn when(tb: &str) -> (u32, u32) {
    parse_timeblock(tb).unwrap_or((0, 0))
}

/// How many suspects are on the list now: Montreaux from day 2 at 2pm,
/// Mosely once his print is in hand later that day.
pub fn listed(state: &GameState) -> usize {
    let now = when(&state.timeblock);
    match (
        now >= (2, 14),
        now >= (2, 17) && state.flags.contains("GOTPMOSELYPRINT"),
    ) {
        (true, true) => 10,
        (true, false) => 9,
        _ => 8,
    }
}

fn links(state: &GameState, suspect: usize) -> &[u32] {
    state
        .suspect_links
        .get(&(suspect as u32))
        .map_or(&[], |v| v.as_slice())
}

pub fn print_linked(state: &GameState, suspect: usize) -> bool {
    PRINTS[suspect]
        .0
        .is_some_and(|f| links(state, suspect).contains(&f))
}

/// The vehicle a suspect's record shows.
pub fn shows_vehicle(state: &GameState, suspect: usize) -> bool {
    match PLATES[suspect].0 {
        None => true,
        Some(f) => links(state, suspect).contains(&f),
    }
}

impl Suspects {
    pub fn menus(&self, state: &GameState) -> Vec<Menu> {
        let both = self.open.is_some() && self.file.is_some();
        let matchable = self
            .file
            .is_some_and(|f| UNKNOWNS.iter().any(|u| u.file == f));
        vec![
            (
                "Menu1Name".into(),
                true,
                vec![("MenuItem1".into(), true, None)],
            ),
            (
                "Menu2Name".into(),
                true,
                (0..listed(state))
                    .map(|i| (format!("Menu2Item{}", i + 1), true, None))
                    .collect(),
            ),
            (
                "Menu3Name".into(),
                true,
                vec![
                    ("Menu3Item1".into(), both, None),
                    ("Menu3Item2".into(), both, None),
                    (String::new(), false, None),
                    ("Menu3Item4".into(), matchable, None),
                ],
            ),
        ]
    }

    /// Showing the screen: Mosely's print links itself once he is listed.
    pub fn show(&mut self, state: &mut GameState) {
        if listed(state) == 10 && !print_linked(state, 9) {
            state.suspect_links.entry(9).or_default().push(7);
            self.picked[9] = Some(0);
            self.open = Some(9);
            self.file = Some(7);
        }
    }

    /// Opening a suspect's record; the first time, a word about it.
    pub fn open_suspect(&mut self, i: usize, state: &GameState) -> Vec<Event> {
        self.open = Some(i);
        if state.flags.contains("GABESAWSUSPECTS") || state.flags.contains("GRACESAWSUSPECTS") {
            return Vec::new();
        }
        let line = if state.ego == "GABRIEL" {
            "02OC52ZPF2"
        } else {
            "02OC52ZPF1"
        };
        vec![
            Event::Say(line.into(), 1),
            Event::SetFlag("GabeSawSuspects"),
            Event::SetFlag("GraceSawSuspects"),
        ]
    }

    /// LINK TO SUSPECT. Errors are the strings key to show.
    pub fn link(&mut self, state: &mut GameState) -> Result<Vec<Event>, &'static str> {
        let (Some(s), Some(f)) = (self.open, self.file) else {
            return Err("NoSuspect");
        };
        let mine = links(state, s);
        if mine.len() >= 6 {
            return Err("MaxLinks");
        }
        let own_plate = PLATES[s].0 == Some(f);
        let own_print = PRINTS[s].0 == Some(f);
        // A suspect's own plate or print may be linked to them even if linked
        // elsewhere; anything else, only once.
        let taken = if own_plate || own_print {
            mine.contains(&f)
        } else {
            state.suspect_links.values().any(|v| v.contains(&f))
        };
        if taken {
            return Err("AlreadyLinked");
        }
        let list = state.suspect_links.entry(s as u32).or_default();
        list.push(f);
        self.picked[s] = Some(list.len() - 1);
        let mut events = Vec::new();
        if own_plate {
            let (_, score, flag) = PLATES[s];
            if !score.is_empty() {
                events.push(Event::Score(score.into()));
            }
            if !flag.is_empty() {
                events.push(Event::SetFlag(flag));
            }
        }
        if own_print && !PRINTS[s].1.is_empty() {
            events.push(Event::Score(PRINTS[s].1.into()));
        }
        self.matching = None;
        Ok(events)
    }

    /// UN-LINK FILE: a suspect's own plate and print stay.
    pub fn unlink(&mut self, state: &mut GameState) -> Result<Vec<Event>, &'static str> {
        let Some(s) = self.open else {
            return Ok(Vec::new());
        };
        let Some(i) = self.picked[s] else {
            return Err("NoLinks");
        };
        let Some(&f) = links(state, s).get(i) else {
            return Err("NoLinks");
        };
        if PLATES[s].0 == Some(f) || PRINTS[s].0 == Some(f) {
            let line = if state.ego == "GABRIEL" {
                "2FL8S27AS1"
            } else {
                "02O9H7Z411"
            };
            return Ok(vec![Event::Say(line.into(), 1)]);
        }
        let list = state.suspect_links.entry(s as u32).or_default();
        list.remove(i);
        self.picked[s] = if list.is_empty() {
            None
        } else {
            Some(i.min(list.len() - 1))
        };
        Ok(Vec::new())
    }

    /// MATCH ANALYSIS: Grace's to do. Returns what to play: the scan movie.
    pub fn start_match(&mut self, state: &GameState) -> Result<&'static str, Vec<Event>> {
        if state.ego == "GABRIEL" {
            return Err(vec![Event::Say("02O7A2ZQR1".into(), 1)]);
        }
        let Some(u) = self
            .file
            .and_then(|f| UNKNOWNS.iter().find(|u| u.file == f))
        else {
            return Err(Vec::new());
        };
        self.matching = Some(Match {
            file: u.file,
            phase: Matching::Scanning,
            picture: u.print,
        });
        Ok(u.movie)
    }

    /// The scan has played: compare with each suspect whose print is
    /// linked, two seconds each, until the owner.
    pub fn scanned(&mut self, now: f64, state: &GameState) -> Vec<Event> {
        self.next_suspect(0, now, state)
    }

    fn next_suspect(&mut self, from: usize, now: f64, state: &GameState) -> Vec<Event> {
        let Some(m) = &mut self.matching else {
            return Vec::new();
        };
        match (from..SUSPECTS).find(|&i| print_linked(state, i)) {
            Some(i) => {
                m.phase = Matching::Comparing {
                    suspect: i,
                    until: now + 2.0,
                };
                m.picture = PRINT_PICTURES[i];
                Vec::new()
            }
            None => {
                m.phase = Matching::None;
                if m.file == 18 && !state.flags.contains("PLAYEDESTELLEFPDIALOG") {
                    return vec![
                        Event::Say("02OX660SL1".into(), 1),
                        Event::SetFlag("PlayedEstelleFPDialog"),
                    ];
                }
                Vec::new()
            }
        }
    }

    /// Time passing during a match.
    pub fn tick(&mut self, now: f64, state: &GameState) -> Vec<Event> {
        let Some(m) = &self.matching else {
            return Vec::new();
        };
        let Matching::Comparing { suspect, until } = m.phase else {
            return Vec::new();
        };
        if now < until {
            return Vec::new();
        }
        let Some(u) = UNKNOWNS.iter().find(|u| u.file == m.file) else {
            return Vec::new();
        };
        if suspect != u.owner {
            return self.next_suspect(suspect + 1, now, state);
        }
        if let Some(m) = &mut self.matching {
            m.phase = Matching::Found(suspect);
            m.picture = u.matched;
        }
        self.open = Some(suspect);
        vec![
            Event::Say(u.line.into(), 1),
            Event::Score(u.score.into()),
            Event::SetFlag(u.flag),
        ]
    }

    /// The comparison picture, once the scan has played.
    pub fn compare_picture(&self) -> Option<&'static str> {
        let m = self.matching.as_ref()?;
        if m.phase == Matching::Scanning {
            return None;
        }
        UNKNOWNS
            .iter()
            .find(|u| u.file == m.file)
            .map(|u| u.compare)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linking_plates_and_prints() {
        let mut state = GameState::new("205P", "R25");
        let mut s = Suspects {
            open: Some(1),
            file: Some(32),
            ..Default::default()
        };
        assert!(!shows_vehicle(&state, 1));
        let events = s.link(&mut state).unwrap();
        assert!(events.contains(&Event::SetFlag("IDedBuchelliVehicle")));
        assert!(shows_vehicle(&state, 1));
        assert_eq!(s.link(&mut state), Err("AlreadyLinked"));
        // His own plate stays put.
        assert!(matches!(
            s.unlink(&mut state).unwrap()[..],
            [Event::Say(..)]
        ));
        // Another file, linked to one suspect only.
        s.file = Some(27);
        s.link(&mut state).unwrap();
        s.open = Some(2);
        assert_eq!(s.link(&mut state), Err("AlreadyLinked"));
    }

    #[test]
    fn match_analysis_finds_the_owner() {
        let mut state = GameState::new("312P", "R25");
        state.ego = "GRACE".into();
        state.suspect_links.insert(0, vec![2]);
        state.suspect_links.insert(1, vec![1]);
        let mut s = Suspects {
            file: Some(11),
            ..Default::default()
        };
        assert_eq!(s.start_match(&state), Ok("BuchBldScan.avi"));
        s.scanned(0.0, &state);
        assert!(matches!(
            s.matching.as_ref().unwrap().phase,
            Matching::Comparing { suspect: 0, .. }
        ));
        assert!(s.tick(2.0, &state).is_empty(), "Buthane is not the one");
        let events = s.tick(4.0, &state);
        assert!(events.contains(&Event::SetFlag("MatchedBuchelli")));
        assert_eq!(s.open, Some(1));
    }
}
