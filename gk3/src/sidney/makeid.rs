//! Make ID: forge an identity card for Gabriel or Grace, as a doctor, a
//! reporter, a repairman... Only on day 2 at 2pm, and only some of them
//! are any use; the printed ones become inventory items.
//!
//! As G-Engine's `SidneyMakeId`.

use gk3_game::state::GameState;

use super::Event;
use super::analyze::Menu;

/// Jobs by menu: the job's code in the card's bitmap name.
pub const JOBS: &[&[&str]] = &[
    &["DOC", "CORONER", "BLOOD"],
    &["NYTIMES", "FREELANCE", "EMONTHLY", "SPORTSI"],
    &["ELEC", "PLUMB"],
    &["ENCY", "SHOES", "AUTO", "DIAPER"],
    &["NOPD", "SECURITY"],
];

pub struct MakeId {
    /// `GAB` or `GRA`.
    pub person: &'static str,
    /// Menu and entry of the job.
    pub job: (usize, usize),
}

impl Default for MakeId {
    fn default() -> Self {
        Self {
            person: "GAB",
            job: (0, 0),
        }
    }
}

impl MakeId {
    pub fn code(&self) -> &'static str {
        JOBS[self.job.0][self.job.1]
    }

    /// The card's picture: person and job.
    pub fn card(&self) -> String {
        format!("{}_{}", self.person, self.code())
    }

    /// The menus, keys into the Make ID strings: `MenuNName`, `MenuNItemM`.
    pub fn menus(&self) -> Vec<Menu> {
        JOBS.iter()
            .enumerate()
            .map(|(m, jobs)| {
                let items = (0..jobs.len())
                    .map(|i| (format!("Menu{}Item{}", m + 1, i + 1), true, None))
                    .collect();
                (format!("Menu{}Name", m + 1), true, items)
            })
            .collect()
    }

    /// PRINT: on day 2 at 2pm, Gabriel's reporter and odd-job cards are
    /// worth having; anything else gets a line saying why not.
    pub fn print(&self, state: &GameState) -> Vec<Event> {
        let say = |plate: &str| Event::Say(plate.into(), 1);
        if state.timeblock != "202P" {
            return vec![say(if state.ego == "GABRIEL" {
                "02O8G5FVU1"
            } else {
                "02O8G5FZ51"
            })];
        }
        if self.person == "GRA" {
            return vec![say("02O8G5F961")];
        }
        let should_work = "02O8G5F772";
        let interesting = "02O8G5FQ21";
        let (item, line) = match self.code() {
            "EMONTHLY" | "SPORTSI" => return vec![say("02O8G5FNV1")],
            "NYTIMES" => ("FAKE_ID_NYT_REP", should_work),
            "FREELANCE" => ("FAKE_ID_REPORTER", should_work),
            "BLOOD" => ("FAKE_ID_BLOODBANK", interesting),
            "AUTO" => ("FAKE_ID_CAR", interesting),
            "DIAPER" => ("FAKE_ID_DIAPERS", interesting),
            _ => return vec![say("02O8G5F1K1")],
        };
        let mut events = if state.has_item(&state.ego, item) {
            vec![say("0XF724XBL1")]
        } else {
            vec![say(line), Event::Sound("SIDPRINTID"), Event::GiveItem(item)]
        };
        if line == should_work {
            events.push(Event::Score("e_sidney_makeid_print".into()));
        }
        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cards_print_on_the_day() {
        let mut id = MakeId::default();
        let mut state = GameState::new("202P", "R25");
        id.job = (1, 1);
        assert_eq!(id.card(), "GAB_FREELANCE");
        let events = id.print(&state);
        assert!(events.contains(&Event::GiveItem("FAKE_ID_REPORTER")));
        assert!(events.contains(&Event::Score("e_sidney_makeid_print".into())));
        state.give_item("GABRIEL", "FAKE_ID_REPORTER");
        assert_eq!(
            id.print(&state),
            [
                Event::Say("0XF724XBL1".into(), 1),
                Event::Score("e_sidney_makeid_print".into())
            ]
        );
        state.timeblock = "205P".into();
        assert_eq!(id.print(&state), [Event::Say("02O8G5FVU1".into(), 1)]);
    }
}
