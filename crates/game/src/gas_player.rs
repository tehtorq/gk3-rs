//! Runs a behaviour script: decides which animation an idle character or a
//! piece of scenery plays next.

use std::collections::HashMap;

use gk3_formats::gas::Instruction;

/// What the script wants now.
#[derive(Debug, Clone, PartialEq)]
pub enum Request {
    /// Play this animation (an `.ANM` name) and come back when it ends.
    Play {
        anim: String,
        looping: bool,
    },
    /// Nothing to do until this many seconds have passed.
    Wait(f32),
    /// Hand over to another script (a `.GAS` name).
    NewIdle(String),
    /// Walk to the first of these positions the actor is not already at,
    /// and come back on arriving.
    Walk(Vec<String>),
    WalkToPoint([f32; 3]),
    /// The script has finished or cannot continue.
    Stop,
}

pub struct GasPlayer {
    script: Vec<Instruction>,
    pc: usize,
    rng: u64,
    vars: HashMap<char, i32>,
    /// Animation to tidy up after, by the animation (upper-cased) it
    /// follows, for being stopped and for being interrupted to talk.
    cleanups: HashMap<String, String>,
    talk_cleanups: HashMap<String, String>,
    /// The last animation asked for.
    current: Option<String>,
    /// `WHENNEAR` conditions, and whether each held last time.
    near: Vec<(Near, bool)>,
    /// Locations the script has moved its actor to, for the story to take
    /// note of (`LOCATION`).
    pub moved_to: Vec<String>,
    /// Looks the script has asked for: whom (none to stop), which parts,
    /// and how long.
    pub looks: Vec<(Option<String>, String, Option<f32>)>,
}

/// Someone coming near (or going away), and where the script goes then.
#[derive(Debug, Clone, PartialEq)]
pub struct Near {
    pub noun: String,
    pub distance: f32,
    pub label: String,
    /// Near this instead of the actor running the script.
    pub other: Option<String>,
    pub away: bool,
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
            vars: HashMap::new(),
            cleanups: HashMap::new(),
            talk_cleanups: HashMap::new(),
            current: None,
            near: Vec::new(),
            moved_to: Vec::new(),
            looks: Vec::new(),
        }
    }

    /// The `WHENNEAR` conditions in force.
    pub fn conditions(&self) -> impl Iterator<Item = &Near> {
        self.near.iter().map(|(n, _)| n)
    }

    /// Re-judges the `WHENNEAR` conditions, given for each whether it
    /// holds now: one that has just come true sends the script to its
    /// label, which it goes on from once what it is doing is done.
    pub fn judge(&mut self, holds: &[bool]) {
        let mut jump = None;
        for ((near, held), &now) in self.near.iter_mut().zip(holds) {
            if now && !*held {
                jump = Some(near.label.clone());
            }
            *held = now;
        }
        if let Some(label) = jump {
            self.goto(&label);
        }
    }

    /// What to play to tidy up if the script is stopped now: the cleanup
    /// for the animation it last asked for, then that cleanup's own, and so
    /// on until one has none, as the original plays them (0x42f471: each
    /// cleanup's cleanup is looked up in the same list before it plays).
    /// A list that leads round in a circle is followed once round.
    pub fn cleanups(&self, for_talk: bool) -> Vec<String> {
        let talk = for_talk
            .then_some(&self.talk_cleanups)
            .filter(|c| !c.is_empty());
        let list = talk.unwrap_or(&self.cleanups);
        let mut chain: Vec<String> = Vec::new();
        let mut at = self.current.as_deref().map(str::to_ascii_uppercase);
        while let Some(next) = at.and_then(|a| list.get(&a)) {
            if chain.iter().any(|c| c.eq_ignore_ascii_case(next)) {
                break;
            }
            chain.push(next.clone());
            at = Some(next.to_ascii_uppercase());
        }
        chain
    }

    fn play(&mut self, anim: String, looping: bool) -> Request {
        self.current = Some(anim.clone());
        Request::Play { anim, looping }
    }

    /// Whether a percentage chance comes off, as G-Engine rolls 1 to 100.
    fn chance(&mut self, percent: u32) -> bool {
        self.roll(100) < percent
    }

    fn goto(&mut self, label: &str) -> bool {
        match self
            .script
            .iter()
            .position(|i| matches!(i, Instruction::Label(l) if *l == label))
        {
            Some(at) => {
                self.pc = at + 1;
                true
            }
            None => false,
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
                    if self.chance(chance) {
                        return self.play(name, false);
                    }
                }
                Instruction::LoopAnim { name } => return self.play(name, true),
                // As G-Engine: any one of them, which then plays with its
                // own chance.
                Instruction::OneOf(choices) => {
                    let pick = self.roll(choices.len() as u32) as usize;
                    let (anim, chance) = choices[pick].clone();
                    if self.chance(chance) {
                        return self.play(anim, false);
                    }
                }
                Instruction::Wait { min, max, chance } => {
                    if self.chance(chance) {
                        let t = self.roll(1001) as f32 / 1000.0;
                        return Request::Wait(min + (max - min) * t);
                    }
                }
                Instruction::Loop => self.pc = 0,
                Instruction::Goto(label) => {
                    if !self.goto(&label) {
                        return Request::Stop;
                    }
                }
                Instruction::Set(var, value) => {
                    self.vars.insert(var, value);
                }
                Instruction::Inc(var) => *self.vars.entry(var).or_default() += 1,
                Instruction::Dec(var) => *self.vars.entry(var).or_default() -= 1,
                Instruction::If {
                    var,
                    op,
                    value,
                    label,
                } => {
                    let v = self.vars.get(&var).copied().unwrap_or(0);
                    let holds = match op {
                        '=' => v == value,
                        '<' => v < value,
                        _ => v > value,
                    };
                    if holds && !self.goto(&label) {
                        return Request::Stop;
                    }
                }
                Instruction::Cleanup {
                    after,
                    cleanup,
                    talk,
                } => {
                    let set = if talk {
                        &mut self.talk_cleanups
                    } else {
                        &mut self.cleanups
                    };
                    set.insert(after, cleanup);
                }
                Instruction::NewIdle(gas) => return Request::NewIdle(gas),
                // As G-Engine's `CHOOSEWALK`: start from a random one.
                Instruction::WalkTo(mut choices) => {
                    let start = self.roll(choices.len() as u32) as usize;
                    choices.rotate_left(start);
                    return Request::Walk(choices);
                }
                Instruction::WalkToPoint(point) => return Request::WalkToPoint(point),
                Instruction::Location(location) => self.moved_to.push(location),
                Instruction::LookAt {
                    noun,
                    parts,
                    seconds,
                } => self.looks.push((Some(noun), parts, seconds)),
                Instruction::StopLookAt => self.looks.push((None, String::new(), None)),
                Instruction::WhenNear {
                    noun,
                    distance,
                    label,
                    other,
                    away,
                } => {
                    let near = Near {
                        noun,
                        distance,
                        label,
                        other,
                        away,
                    };
                    if !self.near.iter().any(|(n, _)| *n == near) {
                        self.near.push((near, false));
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
    fn counters_branch() {
        let mut gas = GasPlayer::new(
            parse(b"SET X, 0\nLABEL START\nANIM a\nINC X\nIF X < 3 START\nANIM b\nLOOP\n"),
            1,
        );
        let names: Vec<_> = (0..8)
            .map(|_| match gas.step() {
                Request::Play { anim, .. } => anim,
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(names, ["a", "a", "a", "b", "a", "a", "a", "b"]);
    }

    #[test]
    fn cleanups_follow_the_last_animation() {
        let mut gas = GasPlayer::new(
            parse(b"USE CLEANUP up, down\nUSETALK CLEANUP up, nod\nANIM up\nANIM flat\nLOOP\n"),
            1,
        );
        gas.step();
        assert_eq!(gas.cleanups(false), ["down"]);
        assert_eq!(gas.cleanups(true), ["nod"]);
        gas.step();
        assert!(gas.cleanups(false).is_empty());
    }

    #[test]
    fn cleanups_chain_until_one_has_none() {
        // As Bigout's in MOP: stopped mid-wipe, he puts the rag away, then
        // the tube, then stands.
        let mut gas = GasPlayer::new(
            parse(
                b"USE CLEANUP wipeA, wipeB\nUSE CLEANUP wipeB, awayA\nUSE CLEANUP awayA, awayB\n\
                  USE CLEANUP loopA, loopB\nUSE CLEANUP loopB, loopA\nANIM wipeA\nANIM loopA\nLOOP\n",
            ),
            1,
        );
        gas.step();
        assert_eq!(gas.cleanups(false), ["wipeB", "awayA", "awayB"]);
        // A circle is followed once round.
        gas.step();
        assert_eq!(gas.cleanups(false), ["loopB", "loopA"]);
    }

    #[test]
    fn scripts_that_never_wait_do_not_hang() {
        let mut gas = GasPlayer::new(parse(b"LABEL x\nGOTO x\n"), 1);
        assert_eq!(gas.step(), Request::Wait(0.5));
    }
}
