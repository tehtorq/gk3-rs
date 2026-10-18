//! When a point in the story is over, and where it goes next.
//!
//! The original decides this in its executable, not in its data: no script
//! ever calls `SetTime`. These are its rules (0x492266), restated as Sheep
//! conditions that must all hold. They are asked on every change of
//! location, of the place the player is going (`IsCurrentLocation` here),
//! while the timeblock stays the same (0x5206ad). One that holds moves the
//! story on, and the next timeblock's rules are asked at once, and so on
//! while they hold, so a player who has already done the next timeblock's
//! business goes straight past it.

use std::collections::HashMap;
use std::rc::Rc;

use gk3_sheep::Script;
use gk3_sheep::expr::Expr;

use crate::api::GameApi;
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

/// Day one's afternoon business at the train station: Marcie asked about the
/// train from Naples, or the arrivals board thought about, and the taxi
/// driver shown the wallet.
macro_rules! train_station {
    () => {
        "(GetTopicCount(\"MARCIE\", \"T_TRAIN_FROM_NAPLES\") || GetNounVerbCount(\"ARRIVALS_IN_CU\", \"THINK\")) \
         && GetNounVerbCount(\"TAXI_DRIVER\", \"WALLET\")"
    };
}

/// By the timeblock, the original's switch cases 1 to 16 in order (309P,
/// the last, has none). A count is the ego's, and true when not zero.
pub const ENDINGS: &[Ending] = &[
    Ending {
        from: "110A",
        when: &[
            "IsCurrentLocation(\"RC1\")",
            "GetNounVerbCount(\"COFFEE_POT\", \"POUR\")",
            "GetNounVerbCount(\"SAN_GREAL_WORDS\", \"LOOK\")",
            "GetNounVerbCount(\"REGISTER\", \"READ\")",
            "GetNounVerbCount(\"PHONE\", \"PRINCE_JAMES_CARD\") || GetNounVerbCount(\"OTR_PHONE_2\", \"PRINCE_JAMES_CARD\") \
             || GetNounVerbCount(\"OTR_PHONE_1\", \"PRINCE_JAMES_CARD\")",
            "GetTopicCount(\"BUTHANE\", \"T_CHECK_IN\") > 1",
            "GetTopicCount(\"GIRARD\", \"T_HOLY_GRAIL\")",
            "GetTopicCount(\"LADY_H_ESTELLE\", \"T_INTRODUCE\")",
            "GetTopicCount(\"EMILIO\", \"T_INTRODUCE\")",
        ],
        to: "112P",
        location: None,
    },
    Ending {
        from: "112P",
        when: &[
            "IsCurrentLocation(\"RC1\")",
            "GetTopicCount(\"ABBE\", \"T_TEMPLARS\")",
            "GetTopicCount(\"MOSELY\", \"T_GRACE\")",
            "GetTopicCount(\"MOSELY\", \"T_SCHATTENJAGER\")",
            // Saw Lady Howard and Emilio swap rooms, or read the register
            // again.
            "GetNounVerbCount(\"REGISTER\", \"READ\") > 1 || GetFlag(\"SeenLHRoomSwitch\")",
        ],
        to: "102P",
        location: None,
    },
    Ending {
        from: "102P",
        when: &[
            "IsCurrentLocation(\"MAP\")",
            concat!("(", train_station!(), ") || GetTopicCount(\"LARRY\", \"T_TEMPLARS\")"),
            "GetNounVerbCount(\"WILKES\", \"FOLLOW\") > 1",
            "GetNounVerbCount(\"BUTHANE\", \"FOLLOW\") > 1",
        ],
        to: "104P",
        location: None,
    },
    Ending {
        from: "104P",
        when: &[
            "IsCurrentLocation(\"MOP\")",
            train_station!(),
            "GetTopicCount(\"LARRY\", \"T_TEMPLARS\")",
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
            // The secret handshake, all six steps (R25's 106P script).
            "GetGameVariableInt(\"HandshakeSteps\") == 6",
            "GetTopicCount(\"GRACE_N_MOSE\", \"T_ABBE\")",
        ],
        to: "207A",
        location: None,
    },
    Ending {
        from: "207A",
        when: &[
            "IsCurrentLocation(\"R25\")",
            "GetTopicCount(\"BUTHANE\", \"T_LE_SERPENT_ROUGE\") == 1",
            "GetTopicCount(\"MOSELY\", \"T_TREASURE\") == 1",
        ],
        to: "210A",
        location: None,
    },
    Ending {
        from: "210A",
        when: &[
            "IsCurrentLocation(\"LBY\")",
            "GetNounVerbCount(\"PARCHMENTS\", \"PICKUP\") > 1",
            "GetNounVerbCount(\"GUN_CASE\", \"OPEN\")",
            "GetNounVerbCount(\"MAGAZINE\", \"READ\")",
            "GetNounVerbCount(\"MAP_IN_CLOSET\", \"PICKUP\") > 1",
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
            "GetNounVerbCount(\"ESTELLE\", \"FOLLOW\")",
        ],
        to: "205P",
        location: None,
    },
    Ending {
        from: "205P",
        when: &[
            "IsCurrentLocation(\"HAL\")",
            // Le Serpent Rouge solved as far as Cancer, its sixth sign.
            "GetGameVariableInt(\"LSRState\") >= 6",
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
            "GetTopicCount(\"GRACE\", \"T_WILKES\")",
            "GetTopicCount(\"GRACE\", \"T_UNICORN\")",
            "GetTopicCount(\"GRACE\", \"T_THRONE\")",
            "GetTopicCount(\"GRACE\", \"T_HOLY_GRAIL\")",
            "GetTopicCount(\"GRACE\", \"T_MANUSCRIPT\")",
            "GetTopicCount(\"GRACE\", \"T_PRINCE_JAMES_MEN\")",
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

/// Whether an ending's rules all hold in this state.
fn holds(ending: &Ending, state: &GameState, scripts: &HashMap<String, Rc<Script>>) -> bool {
    let mut copy = state.clone();
    let mut api = GameApi::new(&mut copy, scripts);
    ending
        .when
        .iter()
        .all(|condition| match Expr::parse(condition) {
            Ok(e) => e.eval(&mut api, &|_| None).truthy(),
            // A rule that does not parse is a bug here, not in the game.
            Err(err) => panic!("timeblock rule {condition:?}: {err}"),
        })
}

/// If the timeblock the state is in is over, now that the player is going
/// to `state.location`: the timeblock the story goes on to and, if a rule
/// said, the location to open it in.
pub fn check(
    state: &GameState,
    scripts: &HashMap<String, Rc<Script>>,
) -> Option<(&'static str, Option<&'static str>)> {
    let mut probe = state.clone();
    let mut over = None;
    while let Some(ending) = ENDINGS
        .iter()
        .find(|e| e.from.eq_ignore_ascii_case(&probe.timeblock))
        .filter(|e| holds(e, &probe, scripts))
    {
        // A rule that names the place the next timeblock opens in sends
        // the player there, and later rules are asked of it.
        let opens = ending.location.or(over.and_then(|(_, at)| at));
        if let Some(at) = ending.location {
            probe.location = at.into();
        }
        probe.timeblock = ending.to.into();
        over = Some((ending.to, opens));
    }
    over
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probe(state: &GameState) -> Option<(&'static str, Option<&'static str>)> {
        check(state, &HashMap::new())
    }

    fn did(state: &mut GameState, noun: &str, verb: &str, n: i32) {
        let ego = state.ego.clone();
        state.set_noun_verb(&ego, noun, verb, n);
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
    fn day_one_evening_ends_after_the_handshake_and_the_abbe() {
        let mut state = GameState::new("106P", "R25");
        did(&mut state, "GRACE_N_MOSE", "T_ABBE", 1);
        assert_eq!(probe(&state), None, "not before the handshake");
        state.variables.insert("HANDSHAKESTEPS".into(), 6);
        assert_eq!(probe(&state), Some(("207A", None)));
        state.location = "LBY".into();
        assert_eq!(probe(&state), None, "only going back to the room");
    }

    #[test]
    fn topics_are_counted_for_whoever_is_playing() {
        let mut state = GameState::new("106P", "R25");
        state.variables.insert("HANDSHAKESTEPS".into(), 6);
        state.set_noun_verb("GRACE", "GRACE_N_MOSE", "T_ABBE", 1);
        assert_eq!(probe(&state), None, "Grace's, not Gabriel's");
    }

    #[test]
    fn the_afternoon_needs_the_station_and_larry() {
        let mut state = GameState::new("104P", "MOP");
        did(&mut state, "WILKES", "T_INTRODUCE", 1);
        did(&mut state, "VIEW_OF_LHOMME_MORE", "BINOCULARS", 1);
        did(&mut state, "LARRY", "T_TEMPLARS", 1);
        assert_eq!(probe(&state), None, "the station still to do");
        did(&mut state, "ARRIVALS_IN_CU", "THINK", 1);
        did(&mut state, "TAXI_DRIVER", "WALLET", 1);
        assert_eq!(probe(&state), Some(("106P", None)));
    }

    #[test]
    fn lunch_opens_at_the_chateau() {
        let mut state = GameState::new("210A", "LBY");
        did(&mut state, "MAID", "FOLLOW", 1);
        assert_eq!(probe(&state), None, "the searches still to do");
        did(&mut state, "PARCHMENTS", "PICKUP", 2);
        did(&mut state, "GUN_CASE", "OPEN", 1);
        did(&mut state, "MAGAZINE", "READ", 1);
        did(&mut state, "MAP_IN_CLOSET", "PICKUP", 2);
        assert_eq!(probe(&state), Some(("212P", Some("CSE"))));
    }

    #[test]
    fn goes_straight_past_a_timeblock_already_done() {
        let mut state = GameState::new("303P", "R25");
        did(&mut state, "CEILING", "INSPECT", 1);
        assert_eq!(probe(&state), Some(("306P", None)));
        let visit = ("GABRIEL".into(), "R27".into(), "306P".into());
        state.location_counts.insert(visit, 1);
        assert_eq!(probe(&state), Some(("309P", None)));
    }
}
