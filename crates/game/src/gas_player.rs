//! Runs a behaviour script: decides which animation an idle character or a
//! piece of scenery plays next.

use gk3_formats::gas::Instruction;

/// What the script wants now.
#[derive(Debug, Clone, PartialEq)]
pub enum Request {
    /// Play this animation (an `.ANM` name) and come back when it ends.
    Play { anim: String, looping: bool },
    /// Nothing to do until this many seconds have passed.
    Wait(f32),
    /// The script has finished or cannot continue.
    Stop,
}

pub struct GasPlayer {
    script: Vec<Instruction>,
    pc: usize,
    rng: u64,
}

/// At most this many instructions run per request, so a script that loops
/// without waiting cannot hang the frame.
const STEP_LIMIT: usize = 16;

impl GasPlayer {
    pub fn new(script: Vec<Instruction>, seed: u64) -> Self {
        Self {
            script,
            pc: 0,
            rng: seed | 1,
        }
    }

    /// Runs until the script asks for something.
    pub fn step(&mut self) -> Request {
        for _ in 0..STEP_LIMIT {
            let Some(instruction) = self.script.get(self.pc).cloned() else {
                return Request::Stop;
            };
            self.pc += 1;
            match instruction {
                Instruction::Anim { name, chance } => {
                    if self.roll(100) < chance {
                        return Request::Play {
                            anim: name,
                            looping: false,
                        };
                    }
                }
                Instruction::LoopAnim { name } => {
                    return Request::Play {
                        anim: name,
                        looping: true,
                    };
                }
                Instruction::OneOf(choices) => {
                    let total: u32 = choices.iter().map(|(_, w)| *w).sum();
                    if total == 0 {
                        continue;
                    }
                    let mut pick = self.roll(total);
                    for (anim, weight) in choices {
                        if pick < weight {
                            return Request::Play {
                                anim,
                                looping: false,
                            };
                        }
                        pick -= weight;
                    }
                }
                Instruction::Wait { seconds } => return Request::Wait(seconds),
                Instruction::Loop => self.pc = 0,
                Instruction::Goto(label) => {
                    match self
                        .script
                        .iter()
                        .position(|i| matches!(i, Instruction::Label(l) if *l == label))
                    {
                        Some(at) => self.pc = at + 1,
                        None => return Request::Stop,
                    }
                }
                Instruction::Label(_) | Instruction::Other { .. } => {}
            }
        }
        // Ran the limit without asking for anything: pause rather than spin.
        Request::Wait(0.5)
    }

    /// A number below `limit`, from a small xorshift generator. Idle choices
    /// are cosmetic, so they never touch the story's random numbers.
    fn roll(&mut self, limit: u32) -> u32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng % limit.max(1) as u64) as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gk3_formats::gas::parse;

    #[test]
    fn plays_then_loops() {
        let mut gas = GasPlayer::new(parse(b"ANIM a\nANIM b\nLOOP\n"), 1);
        let names: Vec<_> = (0..4)
            .map(|_| match gas.step() {
                Request::Play { anim, .. } => anim,
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(names, ["a", "b", "a", "b"]);
    }

    #[test]
    fn oneof_runs_are_one_choice() {
        let mut gas = GasPlayer::new(parse(b"ONEOF a\nONEOF b\nWAIT 2\nLOOP\n"), 7);
        for _ in 0..20 {
            assert!(matches!(gas.step(), Request::Play { .. }));
            assert_eq!(gas.step(), Request::Wait(2.0));
        }
    }

    #[test]
    fn scripts_that_never_wait_do_not_hang() {
        let mut gas = GasPlayer::new(parse(b"LABEL x\nGOTO x\n"), 1);
        assert_eq!(gas.step(), Request::Wait(0.5));
    }
}
