//! Room tone: soundtracks (`.STK`).
//!
//! Not music but a little script, walked in order and then repeated: wait a
//! while, play a clip, wait again. `[PRS]` sections in a row are one step
//! that picks one of them. Each step may carry `Repeat` (how many more times
//! round it runs; zero for always) and `Random` (a percentage chance).

use gk3_formats::ini::{Document, Split};

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Clip {
    pub name: String,
    /// 0 to 1.
    pub volume: f32,
    pub looping: bool,
    /// Heard from a place in the room (`3D`), not everywhere alike.
    pub place: Option<ClipPlace>,
}

/// Where a soundtrack's sound is heard from, as the original reads it
/// (0x51b52b): `X`, `Y`, `Z`, or the middle of the model `Follow` names
/// while it plays (0x51b333), and how near it is heard in full (`MinDist`,
/// 60 if not given) and how far before it is gone (`MaxDist`, a unit more
/// than `MinDist` if not given; three times it if less).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ClipPlace {
    pub at: [f32; 3],
    pub follow: Option<String>,
    pub near: f32,
    pub far: f32,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
enum Kind {
    Wait { min: f64, max: f64 },
    Sound(Clip),
    OneOf(Vec<Clip>),
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
struct Step {
    kind: Kind,
    /// Remaining runs; `None` for always.
    repeat: Option<u32>,
    chance: u32,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Soundtrack {
    /// Upper-cased, without `.STK`, for stopping it by name.
    pub name: String,
    pub music: bool,
    /// Runs through its list once rather than round and round, as an
    /// animation's `PLAYSOUNDTRACKTBS` asks.
    pub once: bool,
    steps: Vec<Step>,
    at: usize,
    busy_until: f64,
    /// A looping clip has started; the rest of the list never runs.
    stopped: bool,
    /// The step whose looping clip is playing.
    looping: Option<usize>,
    rng: u64,
}

impl Soundtrack {
    pub fn parse(name: &str, bytes: &[u8]) -> Self {
        let doc = Document::parse(bytes, Split::Whole);
        let mut steps: Vec<Step> = Vec::new();
        let mut music = false;
        let mut previous_was_prs = false;
        for section in &doc.sections {
            let get = |key: &str| section.lines.iter().find_map(|l| l.get(key));
            let num = |key: &str| get(key).and_then(|v| v.trim().parse::<f64>().ok());
            let repeat = num("Repeat").map(|r| r as u32).filter(|&r| r > 0);
            let chance = num("Random").map_or(100, |r| r as u32);
            let place = || {
                num("3D").filter(|&v| v != 0.0)?;
                let near = num("MinDist").unwrap_or(60.0) as f32;
                let mut far = num("MaxDist").map_or(near + 1.0, |f| f as f32);
                if far < near {
                    far = near * 3.0;
                }
                let at = [num("X"), num("Y"), num("Z")].map(|v| v.unwrap_or(0.0) as f32);
                let follow = get("Follow").map(str::trim).filter(|f| !f.is_empty()).map(str::to_string);
                Some(ClipPlace { at, follow, near, far })
            };
            let clip = || Clip {
                name: get("Name").unwrap_or("").to_string(),
                volume: (num("Volume").unwrap_or(100.0) / 100.0) as f32,
                looping: num("Loop").is_some_and(|l| l != 0.0),
                place: place(),
            };
            let name = section.name.to_ascii_uppercase();
            let is_prs = name == "PRS";
            match name.as_str() {
                "SOUNDTRACK" => {
                    music = get("SoundType").is_some_and(|t| t.eq_ignore_ascii_case("Music"))
                }
                "WAIT" => steps.push(Step {
                    kind: Kind::Wait {
                        min: num("MinWaitMS").unwrap_or(0.0) / 1000.0,
                        max: num("MaxWaitMS").or(num("MinWaitMS")).unwrap_or(0.0) / 1000.0,
                    },
                    repeat,
                    chance,
                }),
                "SOUND" => steps.push(Step {
                    kind: Kind::Sound(clip()),
                    repeat,
                    chance,
                }),
                "PRS" => {
                    let c = Clip {
                        looping: false,
                        ..clip()
                    };
                    match steps.last_mut() {
                        Some(Step {
                            kind: Kind::OneOf(clips),
                            ..
                        }) if previous_was_prs => clips.push(c),
                        _ => steps.push(Step {
                            kind: Kind::OneOf(vec![c]),
                            repeat,
                            chance,
                        }),
                    }
                }
                _ => {}
            }
            previous_was_prs = is_prs;
        }
        let seed = name
            .bytes()
            .fold(0x2545_F491_4F6C_DD1Du64, |h, b| h.rotate_left(7) ^ b as u64);
        Self {
            name: name.to_ascii_uppercase(),
            music,
            once: false,
            steps,
            at: 0,
            busy_until: 0.0,
            stopped: false,
            looping: None,
            rng: seed | 1,
        }
    }

    /// Back from a saved game. Its sounds were not saved, as the
    /// original's are not: a step still with time to run waits it out in
    /// silence, and one playing a loop, with no time to wait, is run again
    /// (0x51c110).
    pub fn restored(&mut self) {
        if let Some(step) = self.looping.take() {
            self.at = step;
            self.stopped = false;
            self.busy_until = 0.0;
        }
    }

    fn roll(&mut self) -> f64 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng % 10_000) as f64 / 10_000.0
    }

    /// Steps the script at time `now`. `length` says how long a clip lasts,
    /// so the script waits for it before going on.
    pub fn update(&mut self, now: f64, length: &mut dyn FnMut(&Clip) -> Option<f64>) -> Vec<Clip> {
        let mut started = Vec::new();
        // Bounded, in case every step is spent or instant.
        for _ in 0..self.steps.len() * 2 + 1 {
            if self.stopped || now < self.busy_until || self.steps.is_empty() {
                break;
            }
            if self.at >= self.steps.len() {
                if self.once {
                    self.stopped = true;
                    break;
                }
                self.at = 0;
                if self.steps.iter().all(|s| s.repeat == Some(0)) {
                    self.stopped = true;
                    break;
                }
            }
            let index = self.at;
            self.at += 1;
            let step = &mut self.steps[index];
            if step.repeat == Some(0) {
                continue;
            }
            if let Some(r) = &mut step.repeat {
                *r -= 1;
            }
            let chance = step.chance;
            let kind = step.kind.clone();
            if self.roll() * 100.0 >= chance as f64 {
                continue;
            }
            match kind {
                Kind::Wait { min, max } => {
                    let t = self.roll();
                    self.busy_until = now + min + (max - min).max(0.0) * t;
                }
                Kind::Sound(clip) => {
                    self.busy_until = now + length(&clip).unwrap_or(0.0);
                    if clip.looping {
                        self.stopped = true;
                        self.looping = Some(index);
                    }
                    started.push(clip);
                }
                Kind::OneOf(clips) => {
                    let i = (self.roll() * clips.len() as f64) as usize;
                    let clip = clips[i.min(clips.len() - 1)].clone();
                    self.busy_until = now + length(&clip).unwrap_or(0.0);
                    started.push(clip);
                }
            }
        }
        started
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walks_waits_and_sounds_in_order() {
        let mut stk = Soundtrack::parse(
            "T",
            b"[WAIT]\nMinWaitMS=1000\nRepeat=1\n[SOUND]\nName=Theme\nVolume=80.0\nRepeat=1\n[WAIT]\nMinWaitMS=500\n[SOUND]\nName=Mood\n",
        );
        let mut len = |_: &Clip| Some(2.0);
        assert!(stk.update(0.0, &mut len).is_empty(), "waits first");
        let first = stk.update(1.0, &mut len);
        assert_eq!(first[0].name, "Theme");
        assert_eq!(first[0].volume, 0.8);
        assert!(
            stk.update(2.0, &mut len).is_empty(),
            "the theme is still playing"
        );
        assert!(stk.update(3.0, &mut len).is_empty(), "then a wait");
        assert_eq!(stk.update(3.5, &mut len)[0].name, "Mood");
        // Round again: the once-only wait and theme are spent.
        let later = stk.update(10.0, &mut len);
        assert!(later.is_empty() || later[0].name == "Mood");
    }

    #[test]
    fn three_d_sounds_are_placed_as_the_original_reads_them() {
        let stk = Soundtrack::parse(
            "T",
            b"[SOUND]\nName=Fountain\n3D=1\nMinDist=100\nMaxDist=900\nX=10\nY=20\nZ=30\n\
              [SOUND]\nName=Radio\n3D=1\nFollow=jea\n\
              [SOUND]\nName=Odd\n3D=1\nMinDist=200\nMaxDist=50\n\
              [SOUND]\nName=Flat\n",
        );
        let place = |i: usize| match &stk.steps[i].kind {
            Kind::Sound(c) => c.place.clone(),
            _ => None,
        };
        assert_eq!(
            place(0),
            Some(ClipPlace { at: [10.0, 20.0, 30.0], follow: None, near: 100.0, far: 900.0 })
        );
        // Unset: near 60, far a unit more.
        assert_eq!(
            place(1),
            Some(ClipPlace { at: [0.0; 3], follow: Some("jea".into()), near: 60.0, far: 61.0 })
        );
        // Far less than near: three times near.
        assert_eq!(place(2).map(|p| p.far), Some(600.0));
        assert_eq!(place(3), None);
    }

    #[test]
    fn prs_runs_are_one_choice() {
        let stk = Soundtrack::parse(
            "T",
            b"[PRS]\nName=A\n[PRS]\nName=B\n[PRS]\nName=C\n[WAIT]\nMinWaitMS=10\n",
        );
        assert_eq!(stk.steps.len(), 2);
        assert!(matches!(&stk.steps[0].kind, Kind::OneOf(c) if c.len() == 3));
    }
}
