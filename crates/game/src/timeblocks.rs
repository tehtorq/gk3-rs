//! When a point in the story is over, and where it goes next.
//!
//! The original game decides this in its executable, not in its data: no
//! script ever calls `SetTime`. These rules are G-Engine's reconstruction
//! (`Assets/Timeblocks.shp`, GPL-3.0, by Clark Kromenaker; see NOTICE),
//! restated as Sheep conditions that must all hold. Like G-Engine, they are
//! checked whenever the player changes location, after the new location is
//! set, so a rule can ask where the player is going and came from.

use gk3_sheep::Host;
use gk3_sheep::expr::Expr;

use crate::state::GameState;

/// One timeblock's ending.
pub struct Ending {
    pub from: &'static str,
    /// Sheep conditions; the timeblock is over when all hold.
    pub when: &'static [&'static str],
    pub to: &'static str,
    /// Where the next timeblock opens, if not where the player was going.
    pub location: Option<&'static str>,
}

/// G-Engine's `CountTrainStationAndLarryActions$`: how many of day one's two
/// afternoon threads are done, as a Sheep expression.
macro_rules! afternoon_threads {
    () => {
        "(((GetNounVerbCount(\"ARRIVALS_IN_CU\", \"THINK\") > 0 || GetTopicCount(\"MARCIE\", \"T_TRAIN_FROM_NAPLES\") > 0) \
          && GetNounVerbCount(\"TAXI_DRIVER\", \"WALLET\") == 2) + (GetTopicCount(\"LARRY\", \"T_TEMPLARS\") >= 2))"
    };
}

pub const ENDINGS: &[Ending] = &[
    Ending {
        from: "110A",
        when: &[
            "IsCurrentLocation(\"RC1\")",
            // Met Mosely: the dining room scene pours the coffee.
            "GetNounVerbCount(\"COFFEE_POT\", \"POUR\")",
            "GetNounVerbCount(\"PHONE\", \"PRINCE_JAMES_CARD\") || GetNounVerbCount(\"OTR_PHONE_1\", \"PRINCE_JAMES_CARD\") \
             || GetNounVerbCount(\"OTR_PHONE_2\", \"PRINCE_JAMES_CARD\")",
            "GetNounVerbCount(\"REGISTER\", \"READ\")",
            "GetTopicCount(\"EMILIO\", \"T_INTRODUCE\")",
            "GetTopicCount(\"BUTHANE\", \"T_CHECK_IN\") >= 2",
            "GetNounVerbCount(\"SAN_GREAL_WORDS\", \"LOOK\")",
            "GetTopicCount(\"GIRARD\", \"T_HOLY_GRAIL\")",
            "GetTopicCount(\"LADY_H_ESTELLE\", \"T_INTRODUCE\")",
        ],
        to: "112P",
        location: None,
    },
    Ending {
        from: "112P",
        when: &[
            "IsCurrentLocation(\"RC1\")",
            "GetTopicCount(\"BUCHELLI\", \"T_CHECK_IN\") >= 1",
            "GetTopicCount(\"ABBE\", \"T_TEMPLARS\") >= 1",
            // Saw Lady Howard and Emilio swap rooms, or read the register again.
            "GetFlag(\"SeenLHRoomSwitch\") || GetNounVerbCount(\"REGISTER\", \"READ\") >= 2",
            "GetTopicCount(\"MOSELY\", \"T_CASE\") && GetTopicCount(\"MOSELY\", \"T_GRACE\") \
             && GetTopicCount(\"MOSELY\", \"T_SCHATTENJAGER\")",
        ],
        to: "102P",
        location: None,
    },
    Ending {
        from: "102P",
        when: &[
            "IsCurrentLocation(\"MAP\")",
            "DoesEgoHaveInvItem(\"MOPED_KEYS\")",
            "IsActorAtLocation(\"BUTHANE\", \"CSD\")",
            "IsActorAtLocation(\"WILKES\", \"LER\")",
            concat!(afternoon_threads!(), " >= 1"),
        ],
        to: "104P",
        location: None,
    },
    Ending {
        from: "104P",
        when: &[
            "IsCurrentLocation(\"MOP\")",
            concat!(afternoon_threads!(), " >= 2"),
            "GetTopicCount(\"WILKES\", \"T_INTRODUCE\")",
            "GetNounVerbCount(\"VIEW_OF_LHOMME_MORE\", \"BINOCULARS\")",
        ],
        to: "106P",
        location: None,
    },
    Ending {
        from: "106P",
        when: &[
            "IsCurrentLocation(\"R25\")",
            "GetTopicCount(\"GRACE_N_MOSE\", \"T_ABBE\")",
        ],
        to: "207A",
        location: None,
    },
    Ending {
        from: "207A",
        when: &[
            "IsCurrentLocation(\"R25\")",
            "WasLastLocation(\"CD1\")",
            "GetTopicCount(\"EMILIO\", \"T_LE_SERPENT_ROUGE\") && GetTopicCount(\"MOSELY\", \"T_LE_SERPENT_ROUGE\") \
             && GetTopicCount(\"BUTHANE\", \"T_LE_SERPENT_ROUGE\")",
            "GetTopicCount(\"MOSELY\", \"T_GABRIEL\") && GetTopicCount(\"MOSELY\", \"T_MOSELY\") \
             && GetTopicCount(\"MOSELY\", \"T_TREASURE\")",
        ],
        to: "210A",
        location: None,
    },
    Ending {
        from: "210A",
        when: &[
            "IsCurrentLocation(\"LBY\")",
            // Set by the lunch scene that closes the morning.
            "GetNounVerbCount(\"MAID\", \"FOLLOW\")",
        ],
        to: "212P",
        location: Some("CSE"),
    },
    Ending {
        from: "212P",
        when: &[
            "IsCurrentLocation(\"R25\")",
            "GetNounVerbCount(\"OLD_LADY\", \"TALK\")",
        ],
        to: "202P",
        location: None,
    },
    Ending {
        from: "202P",
        when: &[
            "IsCurrentLocation(\"R25\")",
            "GetGameVariableInt(\"FiveMinTimer202p\") == 7",
        ],
        to: "205P",
        location: None,
    },
    Ending {
        from: "205P",
        when: &[
            "IsCurrentLocation(\"HAL\")",
            "GetFlag(\"Gemini\") && GetFlag(\"Cancer\")",
        ],
        // Day 3, 2am, though the game calls it 202A.
        to: "202A",
        location: None,
    },
    Ending {
        from: "202A",
        when: &[
            "IsCurrentLocation(\"R25\")",
            "DoesEgoHaveInvItem(\"BLOODLINE_MANUSCRIPT\")",
        ],
        to: "307A",
        location: None,
    },
    Ending {
        from: "307A",
        when: &["IsCurrentLocation(\"HAL\")", "GetFlag(\"End307a\")"],
        to: "310A",
        location: None,
    },
    Ending {
        from: "310A",
        when: &[
            "IsCurrentLocation(\"R25\")",
            "GetTopicCount(\"GRACE\", \"T_FREEMASONS\") && GetTopicCount(\"GRACE\", \"T_WILKES\") \
             && GetTopicCount(\"GRACE\", \"T_UNICORN\") && GetTopicCount(\"GRACE\", \"T_THRONE\") \
             && GetTopicCount(\"GRACE\", \"T_PRINCE_JAMES_MEN\") && GetTopicCount(\"GRACE\", \"T_MANUSCRIPT\")",
        ],
        to: "312P",
        location: None,
    },
    Ending {
        from: "312P",
        when: &[
            "IsCurrentLocation(\"DIN\")",
            "GetNounVerbCount(\"VIEW_OF_ORANGE_ROCK\", \"BINOCULARS\") == 2",
        ],
        to: "303P",
        location: None,
    },
    Ending {
        from: "303P",
        when: &[
            "IsCurrentLocation(\"R25\")",
            "GetNounVerbCount(\"CEILING\", \"INSPECT\")",
        ],
        to: "306P",
        location: None,
    },
    Ending {
        from: "306P",
        when: &["IsCurrentLocation(\"R25\")", "GetEgoLocationCount(\"R27\")"],
        to: "309P",
        location: None,
    },
];

/// The story's timeblocks in order, from the endings.
pub fn order() -> Vec<&'static str> {
    let mut out: Vec<&str> = ENDINGS.iter().map(|e| e.from).collect();
    if let Some(last) = ENDINGS.last() {
        out.push(last.to);
    }
    out
}

/// If the timeblock the state is in is over, where the story goes: the next
/// timeblock and, if it says, the location to open it in.
pub fn check(
    state: &GameState,
    host: &mut dyn Host,
) -> Option<(&'static str, Option<&'static str>)> {
    let ending = ENDINGS
        .iter()
        .find(|e| e.from.eq_ignore_ascii_case(&state.timeblock))?;
    ending
        .when
        .iter()
        .all(|condition| match Expr::parse(condition) {
            Ok(e) => e.eval(host, &|_| None).truthy(),
            // A rule that does not parse is a bug here, not in the game.
            Err(err) => panic!("timeblock rule {condition:?}: {err}"),
        })
        .then_some((ending.to, ending.location))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::GameApi;
    use std::collections::HashMap;

    fn probe(state: &GameState) -> Option<(&'static str, Option<&'static str>)> {
        let scripts = HashMap::new();
        let mut copy = state.clone();
        let mut api = GameApi::new(&mut copy, &scripts);
        check(state, &mut api)
    }

    #[test]
    fn every_rule_parses_and_chains() {
        for e in ENDINGS {
            for c in e.when {
                Expr::parse(c).unwrap_or_else(|err| panic!("{c}: {err}"));
            }
        }
        let order = order();
        assert_eq!(order.first(), Some(&"110A"));
        assert_eq!(order.last(), Some(&"309P"));
        assert_eq!(order.len(), 17);
    }

    #[test]
    fn day_one_evening_ends_after_asking_about_the_abbe() {
        let mut state = GameState::new("106P", "R25");
        assert_eq!(probe(&state), None);
        state
            .topics
            .insert(("GRACE_N_MOSE".into(), "T_ABBE".into()), 1);
        assert_eq!(probe(&state), Some(("207A", None)));
        state.location = "LBY".into();
        assert_eq!(probe(&state), None, "only once back in the room");
    }

    #[test]
    fn counts_the_afternoon_threads() {
        let mut state = GameState::new("104P", "MOP");
        state
            .topics
            .insert(("WILKES".into(), "T_INTRODUCE".into()), 1);
        state.noun_verbs.insert(
            (
                "GABRIEL".into(),
                "VIEW_OF_LHOMME_MORE".into(),
                "BINOCULARS".into(),
            ),
            1,
        );
        state
            .topics
            .insert(("LARRY".into(), "T_TEMPLARS".into()), 2);
        assert_eq!(probe(&state), None, "one thread of two");
        state.noun_verbs.insert(
            ("GABRIEL".into(), "ARRIVALS_IN_CU".into(), "THINK".into()),
            1,
        );
        state
            .noun_verbs
            .insert(("GABRIEL".into(), "TAXI_DRIVER".into(), "WALLET".into()), 2);
        assert_eq!(probe(&state), Some(("106P", None)));
    }

    #[test]
    fn lunch_opens_at_the_chateau() {
        let mut state = GameState::new("210A", "LBY");
        state
            .noun_verbs
            .insert(("GABRIEL".into(), "MAID".into(), "FOLLOW".into()), 1);
        assert_eq!(probe(&state), Some(("212P", Some("CSE"))));
    }
}
