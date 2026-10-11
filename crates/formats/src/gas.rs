//! Behaviour scripts (`.GAS`): what a character or a piece of scenery does
//! when nothing else is directing it.
//!
//! A line is an instruction; arguments are separated by commas, spaces or
//! both. A run of consecutive `ONEOF` lines is one weighted random choice.

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
    /// One weighted random choice among animations.
    OneOf(Vec<(String, u32)>),
    /// Do nothing for a number of seconds.
    Wait {
        seconds: f32,
    },
    Label(String),
    Goto(String),
    /// Start again from the top.
    Loop,
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
            .split(|c: char| c == ',' || c.is_whitespace())
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
        let instruction = match keyword.to_ascii_uppercase().as_str() {
            "ANIM" if !args.is_empty() => {
                // ANIM name [TRUE|FALSE] [chance]
                let chance = args[1..]
                    .iter()
                    .find_map(|a| a.parse::<u32>().ok())
                    .unwrap_or(100);
                Instruction::Anim {
                    name: args[0].clone(),
                    chance,
                }
            }
            "LOOPANIM" if !args.is_empty() => Instruction::LoopAnim {
                name: args[0].clone(),
            },
            "ONEOF" if !args.is_empty() => {
                let choice = (args[0].clone(), number(1, 100.0) as u32);
                if let Some(Instruction::OneOf(run)) = out.last_mut() {
                    run.push(choice);
                    continue;
                }
                Instruction::OneOf(vec![choice])
            }
            "WAIT" => Instruction::Wait {
                seconds: number(0, 1.0),
            },
            "LABEL" if !args.is_empty() => Instruction::Label(args[0].to_ascii_uppercase()),
            "GOTO" if !args.is_empty() => Instruction::Goto(args[0].to_ascii_uppercase()),
            "LOOP" => Instruction::Loop,
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
        assert_eq!(script[3], Instruction::Wait { seconds: 0.3 });
        assert_eq!(script[5], Instruction::Goto("START".into()));
        assert_eq!(script[6], Instruction::Loop);
        assert!(matches!(&script[7], Instruction::Other { keyword, .. } if keyword == "USE"));
    }
}
