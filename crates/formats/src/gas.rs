//! Behaviour scripts (`.GAS`): what a character or a piece of scenery does
//! when nothing else is directing it.
//!
//! A line is an instruction; arguments are separated by commas, spaces,
//! parentheses or a mix. A run of consecutive `ONEOF` lines is one random
//! choice. As G-Engine's `GAS` reads them.

#[derive(Debug, Clone, PartialEq)]
pub enum Instruction {
    /// Play an animation and wait for it, with a percentage chance of
    /// playing at all.
    Anim {
        name: String,
        chance: u32,
    },
    /// Play an animation repeatedly, forever.
    LoopAnim {
        name: String,
    },
    /// One animation picked at random, then played with its own chance.
    OneOf(Vec<(String, u32)>),
    /// Do nothing for between `min` and `max` seconds, with a percentage
    /// chance of waiting at all.
    Wait {
        min: f32,
        max: f32,
        chance: u32,
    },
    Label(String),
    Goto(String),
    /// Start again from the top.
    Loop,
    /// The script's one-letter counters.
    Set(char, i32),
    Inc(char),
    Dec(char),
    /// Jump to a label if a counter compares (`=`, `<` or `>`) to a value.
    If {
        var: char,
        op: char,
        value: i32,
        label: String,
    },
    /// `USE CLEANUP`: if the script is stopped while the first animation
    /// plays, let it finish and play the second to tidy up. `USETALK`
    /// cleanups are for being interrupted to talk.
    Cleanup {
        after: String,
        cleanup: String,
        talk: bool,
    },
    /// Hand over to another script, once this one is tidied away.
    NewIdle(String),
    /// Walk to a named position (`WALKTO`), or to whichever of several the
    /// actor is not already at (`CHOOSEWALK`), and wait till there.
    WalkTo(Vec<String>),
    /// Walk to a point.
    WalkToPoint([f32; 3]),
    /// The actor is now at this location, as far as the story is concerned.
    Location(String),
    /// Look at someone (`LOOKAT GABRIEL EH 5`): which parts look (eyes,
    /// head), and for how many seconds (none: until `STOPLOOKAT`).
    LookAt {
        noun: String,
        parts: String,
        seconds: Option<f32>,
    },
    StopLookAt,
    /// From now on, jump to a label whenever someone (`noun`) comes within
    /// `distance` of the actor, or of `other`; or, `away`, goes beyond it.
    WhenNear {
        noun: String,
        distance: f32,
        label: String,
        other: Option<String>,
        away: bool,
    },
    /// Anything not run yet, kept so a script's shape is visible.
    Other {
        keyword: String,
        args: Vec<String>,
    },
}

pub fn parse(bytes: &[u8]) -> Vec<Instruction> {
    let text: String = bytes
        .iter()
        .take_while(|&&b| b != 0)
        .map(|&b| b as char)
        .collect();
    let mut out: Vec<Instruction> = Vec::new();
    for raw in text.lines() {
        let line = raw.split("//").next().unwrap_or("").trim();
        let words: Vec<String> = line
            .split(|c: char| matches!(c, ',' | '(' | ')') || c.is_whitespace())
            .filter(|w| !w.is_empty())
            .map(str::to_string)
            .collect();
        let Some((keyword, args)) = words.split_first() else {
            continue;
        };
        let number = |i: usize, default: f32| {
            args.get(i)
                .and_then(|a| a.parse::<f32>().ok())
                .unwrap_or(default)
        };
        // ANIM and ONEOF are `name [moving] [chance]`; scripts that leave
        // out `moving` mean the number as the chance.
        let chance = || {
            args[1..]
                .iter()
                .find_map(|a| a.parse::<u32>().ok())
                .unwrap_or(100)
        };
        let var = |i: usize| args.get(i).and_then(|a| a.chars().next());
        let upper = keyword.to_ascii_uppercase();
        let instruction = match upper.as_str() {
            "ANIM" if !args.is_empty() => Instruction::Anim {
                name: args[0].clone(),
                chance: chance(),
            },
            "LOOPANIM" if !args.is_empty() => Instruction::LoopAnim {
                name: args[0].clone(),
            },
            "ONEOF" if !args.is_empty() => {
                let choice = (args[0].clone(), chance());
                if let Some(Instruction::OneOf(run)) = out.last_mut() {
                    run.push(choice);
                    continue;
                }
                Instruction::OneOf(vec![choice])
            }
            "WAIT" => {
                let min = number(0, 1.0);
                // A third number is a chance; it is always the last word
                // (one script has a stray FALSE before it).
                let chance = if args.len() > 2 {
                    args.last().and_then(|a| a.parse().ok()).unwrap_or(100)
                } else {
                    100
                };
                Instruction::Wait {
                    min,
                    max: number(1, min).max(min),
                    chance,
                }
            }
            "LABEL" if !args.is_empty() => Instruction::Label(args[0].to_ascii_uppercase()),
            "GOTO" if !args.is_empty() => Instruction::Goto(args[0].to_ascii_uppercase()),
            "LOOP" => Instruction::Loop,
            "SET" if args.len() >= 2 => match var(0) {
                Some(v) => Instruction::Set(v.to_ascii_uppercase(), number(1, 0.0) as i32),
                None => continue,
            },
            "INC" if !args.is_empty() => match var(0) {
                Some(v) => Instruction::Inc(v.to_ascii_uppercase()),
                None => continue,
            },
            "DEC" if !args.is_empty() => match var(0) {
                Some(v) => Instruction::Dec(v.to_ascii_uppercase()),
                None => continue,
            },
            "IF" if args.len() >= 4 => match (var(0), var(1)) {
                (Some(v), Some(op @ ('=' | '<' | '>'))) => Instruction::If {
                    var: v.to_ascii_uppercase(),
                    op,
                    value: number(2, 0.0) as i32,
                    label: args[3].to_ascii_uppercase(),
                },
                _ => continue,
            },
            "USE" | "USES" | "USETALK"
                if args.len() >= 3 && args[0].eq_ignore_ascii_case("CLEANUP") =>
            {
                Instruction::Cleanup {
                    after: args[1].to_ascii_uppercase(),
                    cleanup: args[2].clone(),
                    talk: upper == "USETALK",
                }
            }
            "NEWIDLE" if !args.is_empty() => Instruction::NewIdle(args[0].clone()),
            "LOCATION" if !args.is_empty() => Instruction::Location(args[0].to_ascii_uppercase()),
            "LOOKAT" if !args.is_empty() => Instruction::LookAt {
                noun: args[0].to_ascii_uppercase(),
                parts: args.get(1).cloned().unwrap_or_default(),
                seconds: args.get(2).and_then(|a| a.parse().ok()),
            },
            "STOPLOOKAT" => Instruction::StopLookAt,
            "WALKTO" if args.len() >= 3 => {
                Instruction::WalkToPoint([number(0, 0.0), number(1, 0.0), number(2, 0.0)])
            }
            "WALKTO" | "CHOOSEWALK" if !args.is_empty() => {
                Instruction::WalkTo(args.iter().map(|a| a.to_ascii_uppercase()).collect())
            }
            "WHENNEAR" | "WHENNOLONGERNEAR" if args.len() >= 3 => Instruction::WhenNear {
                noun: args[0].to_ascii_uppercase(),
                distance: number(1, 0.0),
                label: args[2].to_ascii_uppercase(),
                other: args.get(3).map(|a| a.to_ascii_uppercase()),
                away: upper == "WHENNOLONGERNEAR",
            },
            _ => Instruction::Other {
                keyword: keyword.to_ascii_uppercase(),
                args: args.to_vec(),
            },
        };
        out.push(instruction);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_an_idle() {
        let script = parse(
            b"// idle\r\nANIM gabbreath1\r\nONEOF gabbreath1\r\nONEOF gabfidget1, 50\r\n\
              ANIM AbeHe1FightFidget, FALSE 50\r\nWAIT 0.3\r\nLABEL Start\r\nGOTO start\r\nLOOP\r\n\
              USE CLEANUP abebinocbreath, abebinocdown\r\n",
        );
        assert_eq!(
            script[1],
            Instruction::OneOf(vec![("gabbreath1".into(), 100), ("gabfidget1".into(), 50)])
        );
        assert_eq!(
            script[2],
            Instruction::Anim {
                name: "AbeHe1FightFidget".into(),
                chance: 50
            }
        );
        assert_eq!(
            script[3],
            Instruction::Wait {
                min: 0.3,
                max: 0.3,
                chance: 100
            }
        );
        assert_eq!(script[5], Instruction::Goto("START".into()));
        assert_eq!(script[6], Instruction::Loop);
        assert_eq!(
            script[7],
            Instruction::Cleanup {
                after: "ABEBINOCBREATH".into(),
                cleanup: "abebinocdown".into(),
                talk: false
            }
        );
    }

    #[test]
    fn parses_walks_and_nearness() {
        let script = parse(
            b"CHOOSEWALK ( ABE_MOVE_1 ,ABE_MOVE_2 )\nWALKTO ABBE_IN\nWHENNEAR GABRIEL, 110, MOVE_1, CHICKEN\n",
        );
        assert_eq!(
            script[0],
            Instruction::WalkTo(vec!["ABE_MOVE_1".into(), "ABE_MOVE_2".into()])
        );
        assert_eq!(script[1], Instruction::WalkTo(vec!["ABBE_IN".into()]));
        assert_eq!(
            script[2],
            Instruction::WhenNear {
                noun: "GABRIEL".into(),
                distance: 110.0,
                label: "MOVE_1".into(),
                other: Some("CHICKEN".into()),
                away: false
            }
        );
    }

    #[test]
    fn parses_counters_and_waits() {
        let script = parse(
            b"SET X, 0
INC x
IF X < 10 START
WAIT 2, 5, FALSE, 40
",
        );
        assert_eq!(script[0], Instruction::Set('X', 0));
        assert_eq!(script[1], Instruction::Inc('X'));
        assert_eq!(
            script[2],
            Instruction::If {
                var: 'X',
                op: '<',
                value: 10,
                label: "START".into()
            }
        );
        assert_eq!(
            script[3],
            Instruction::Wait {
                min: 2.0,
                max: 5.0,
                chance: 40
            }
        );
    }
}
