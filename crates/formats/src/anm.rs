//! Animation scripts (`.ANM`, `.YAK`): what happens on which frame of an
//! animation. This reads the parts the engine plays so far: the length, the
//! vertex animations started (`[ACTIONS]`), sounds (`[SOUNDS]`) and the lines
//! spoken (`[GK3]`). A `.YAK` is one line of dialogue in this format.

use crate::ini::{Document, Split};

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Animation {
    /// Length in frames, at 15 a second unless `[OPTIONS]` says otherwise.
    pub frames: u32,
    pub actions: Vec<Action>,
    pub sounds: Vec<Sound>,
    pub captions: Vec<Caption>,
    pub faces: Vec<FaceEvent>,
    /// What else happens on a frame: lines spoken, cameras cut to.
    pub cues: Vec<Cue>,
    /// In a line of dialogue, the frame the next line may start on and its
    /// caption ends (`DIALOGUECUE`), a little before the sound runs out.
    pub dialogue_cue: Option<u32>,
    /// Frames a second (`[OPTIONS]` `FRAMERATE`): 15 but for a movie's
    /// captions, which count at the movie's 30.
    pub frame_rate: u32,
}

/// Something an animation sets off in the story on one of its frames.
#[derive(Debug, Clone, PartialEq)]
pub struct Cue {
    pub frame: u32,
    pub what: CueKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CueKind {
    /// Speak one line, by license plate (without the language's `E`).
    Dialogue(String),
    /// Cut (or glide) to a room camera.
    Camera { name: String, glide: bool },
    /// Show or hide a model (`[MVISIBILITY]`) or a part of the room
    /// (`[SVISIBILITY]`).
    Visible { model: String, visible: bool },
    /// Show or hide one group of one mesh of a model.
    PartVisible {
        model: String,
        mesh: usize,
        group: usize,
        visible: bool,
    },
    /// Paint a different texture on one group of one mesh of a model
    /// (`[MTEXTURES]`).
    ModelTexture {
        model: String,
        mesh: usize,
        group: usize,
        texture: String,
    },
    /// Paint a different texture on a part of the room (`[STEXTURES]`).
    SceneTexture { model: String, texture: String },
    /// Start a soundtrack (`.STK`), to run round and round or just once.
    PlaySoundtrack { name: String, once: bool },
    /// Stop one soundtrack, or every one.
    StopSoundtrack(Option<String>),
    /// Someone's foot lands (or scuffs): a sound for their shoes and floor.
    Footstep { noun: String, scuff: bool },
}

/// A change to somebody's face, by the noun of whose.
#[derive(Debug, Clone, PartialEq)]
pub struct FaceEvent {
    pub frame: u32,
    pub noun: String,
    pub change: FaceChange,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FaceChange {
    /// A mouth shape shared by everyone (`MOUTH03`); the character's own
    /// bitmap is `<code>_MOUTH03`.
    Mouth(String),
    /// A named bitmap over a part: `M` mouth, `E` eyelids, `H` forehead.
    Paint { bitmap: String, part: char },
    /// Back to the resting picture for a part.
    Clear(char),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Sound {
    pub frame: u32,
    /// An audio asset, often with a dialogue code for its extension.
    pub name: String,
    pub volume: i32,
    /// Where in the room it is heard from, for a sound placed there.
    pub place: Option<SoundPlace>,
}

/// A sound heard from a place in the room, as the original reads a sound
/// line (0x54c000): after the volume, a model's name or a position, then
/// how near it is heard in full and how far before it is gone.
#[derive(Debug, Clone, PartialEq)]
pub struct SoundPlace {
    /// Heard from the middle of this model, as it stands when the sound
    /// starts.
    pub model: Option<String>,
    /// From there (or the room's origin), this far: written x, z, y.
    pub offset: [f32; 3],
    pub near: f32,
    pub far: f32,
}

/// What the original takes for a distance a sound line leaves out.
pub const SOUND_DISTANCE_UNSET: f32 = 1.0e9;

/// The rest of a sound line after its volume, as the original reads it
/// (0x54c000): a field with a letter in it (but for an exponent's `e`) is a
/// model; otherwise three numbers are a position. Either is followed by the
/// near and far distances, each left at `SOUND_DISTANCE_UNSET` if missing
/// or unreadable. Nothing more on the line: a sound not placed.
fn sound_place(rest: &[&str]) -> Option<SoundPlace> {
    let first = rest.first().filter(|f| !f.is_empty())?;
    let distance = |i: usize| -> f32 {
        rest.get(i).and_then(|v| v.parse().ok()).unwrap_or(SOUND_DISTANCE_UNSET)
    };
    let named = first.chars().any(|c| c.is_ascii_alphabetic() && !c.eq_ignore_ascii_case(&'e'));
    if named {
        return Some(SoundPlace {
            model: Some(first.to_ascii_uppercase()),
            offset: [0.0; 3],
            near: distance(1),
            far: distance(2),
        });
    }
    let n = |i: usize| rest.get(i).and_then(|v| v.parse::<f32>().ok());
    let (x, z, y) = (n(0)?, n(1)?, n(2)?);
    Some(SoundPlace {
        model: None,
        offset: [x, y, z],
        near: distance(3),
        far: distance(4),
    })
}

/// A line of dialogue shown on screen.
#[derive(Debug, Clone, PartialEq)]
pub struct Caption {
    pub frame: u32,
    /// The frame it goes, when the line says (`SpeakerCaption`).
    pub end: Option<u32>,
    /// The noun of whoever says it.
    pub speaker: Option<String>,
    pub text: String,
}

/// One `[ACTIONS]` line: start a vertex animation on a frame.
#[derive(Debug, Clone, PartialEq)]
pub struct Action {
    pub frame: u32,
    /// The clip's name without `.ACT`, e.g. `gab_GabBreath1`.
    pub clip: String,
    /// Present when the line carries eight numbers, even all zero: the clip
    /// then plays at authored coordinates rather than where the model is.
    pub placement: Option<[f32; 8]>,
}

impl Animation {
    pub fn parse(bytes: &[u8]) -> Self {
        let doc = Document::parse(bytes, Split::Whole);
        let frames = doc
            .section("HEADER")
            .and_then(|s| s.lines.first())
            .and_then(|l| l.text.split(',').next()?.trim().parse().ok())
            .unwrap_or(0);
        let mut actions = Vec::new();
        for section in doc.sections("ACTIONS") {
            // The first line is a count; the lines present are what count.
            for line in &section.lines {
                let fields: Vec<&str> = line.text.split(',').map(str::trim).collect();
                if fields.len() < 2 {
                    continue;
                }
                let Ok(frame) = fields[0].parse() else {
                    continue;
                };
                let numbers: Vec<f32> = fields[2..].iter().filter_map(|f| f.parse().ok()).collect();
                actions.push(Action {
                    frame,
                    clip: fields[1].to_string(),
                    placement: numbers.try_into().ok(),
                });
            }
        }
        let mut sounds = Vec::new();
        for line in doc.sections("SOUNDS").flat_map(|s| &s.lines) {
            let fields: Vec<&str> = line.text.split(',').map(str::trim).collect();
            if let [frame, name, rest @ ..] = &fields[..]
                && let Ok(frame) = frame.parse()
            {
                sounds.push(Sound {
                    frame,
                    name: name.to_string(),
                    volume: rest.first().and_then(|v| v.parse().ok()).unwrap_or(100),
                    place: sound_place(rest.get(1..).unwrap_or(&[])),
                });
            }
        }

        let mut cues = Vec::new();
        let on = |v: &str| matches!(v.trim().to_ascii_uppercase().as_str(), "ON" | "TRUE" | "1");
        // `frame, model, on/off`, or for a part of a model `frame, model,
        // mesh, group, on/off`. The room's: `frame, scene, model, on/off`.
        for (section, model_at) in [("MVISIBILITY", 1), ("SVISIBILITY", 2)] {
            for line in doc.sections(section).flat_map(|s| &s.lines) {
                let fields: Vec<&str> = line.text.split(',').map(str::trim).collect();
                if let [frame, model, mesh, group, on_off] = fields[..]
                    && section == "MVISIBILITY"
                    && let (Ok(frame), Ok(mesh), Ok(group)) =
                        (frame.parse(), mesh.parse(), group.parse())
                {
                    cues.push(Cue {
                        frame,
                        what: CueKind::PartVisible {
                            model: model.to_ascii_uppercase(),
                            mesh,
                            group,
                            visible: on(on_off),
                        },
                    });
                    continue;
                }
                if fields.len() != model_at + 2 {
                    continue;
                }
                let Ok(frame) = fields[0].parse() else {
                    continue;
                };
                cues.push(Cue {
                    frame,
                    what: CueKind::Visible {
                        model: fields[model_at].to_ascii_uppercase(),
                        visible: on(fields[model_at + 1]),
                    },
                });
            }
        }

        // `frame, model, mesh, group, texture` and `frame, scene, model,
        // texture`.
        for line in doc.sections("MTEXTURES").flat_map(|s| &s.lines) {
            let f: Vec<&str> = line.text.split(',').map(str::trim).collect();
            if let [frame, model, mesh, group, texture] = f[..]
                && let (Ok(frame), Ok(mesh), Ok(group)) =
                    (frame.parse(), mesh.parse(), group.parse())
            {
                cues.push(Cue {
                    frame,
                    what: CueKind::ModelTexture {
                        model: model.to_ascii_uppercase(),
                        mesh,
                        group,
                        texture: texture.to_ascii_uppercase(),
                    },
                });
            }
        }
        for line in doc.sections("STEXTURES").flat_map(|s| &s.lines) {
            let f: Vec<&str> = line.text.split(',').map(str::trim).collect();
            if let [frame, _scene, model, texture] = f[..]
                && let Ok(frame) = frame.parse()
            {
                cues.push(Cue {
                    frame,
                    what: CueKind::SceneTexture {
                        model: model.to_ascii_uppercase(),
                        texture: texture.to_ascii_uppercase(),
                    },
                });
            }
        }

        // Captions contain commas, so everything past the fields belongs to
        // the text. SPEAKER and CAPTION come as a pair of lines.
        let mut captions = Vec::new();
        let mut faces = Vec::new();
        let mut speaker: Option<String> = None;
        let mut dialogue_cue = None;
        for line in doc.sections("GK3").flat_map(|s| &s.lines) {
            let mut parts = line.text.splitn(2, ',');
            let (Some(frame), Some(rest)) = (
                parts.next().and_then(|f| f.trim().parse::<u32>().ok()),
                parts.next(),
            ) else {
                continue;
            };
            let (kind, rest) = rest.split_once(',').map_or((rest, ""), |(k, r)| (k, r));
            match kind.trim().to_ascii_uppercase().as_str() {
                "SPEAKER" => speaker = Some(rest.trim().to_ascii_uppercase()),
                "CAPTION" => captions.push(Caption {
                    frame,
                    end: None,
                    speaker: speaker.clone(),
                    text: rest.trim().to_string(),
                }),
                "LIPSYNCH" | "FACETEX" | "UNFACETEX" => {
                    let fields: Vec<&str> = rest.split(',').map(str::trim).collect();
                    let part = |i: usize| {
                        fields
                            .get(i)
                            .and_then(|p| p.chars().next())
                            .map(|c| c.to_ascii_uppercase())
                    };
                    let change = match (kind.trim().to_ascii_uppercase().as_str(), &fields[..]) {
                        ("LIPSYNCH", [_, shape, ..]) => {
                            Some(FaceChange::Mouth(shape.to_ascii_uppercase()))
                        }
                        ("FACETEX", [_, bitmap, ..]) => part(2).map(|p| FaceChange::Paint {
                            bitmap: bitmap.to_ascii_uppercase(),
                            part: p,
                        }),
                        ("UNFACETEX", _) => part(1).map(FaceChange::Clear),
                        _ => None,
                    };
                    if let (Some(change), Some(noun)) = (change, fields.first()) {
                        faces.push(FaceEvent {
                            frame,
                            noun: noun.to_ascii_uppercase(),
                            change,
                        });
                    }
                }
                "DIALOGUECUE" => dialogue_cue = Some(frame),
                "FOOTSTEP" | "FOOTSCUFF" => {
                    let noun = rest.split(',').next().unwrap_or("").trim();
                    if !noun.is_empty() {
                        cues.push(Cue {
                            frame,
                            what: CueKind::Footstep {
                                noun: noun.to_ascii_uppercase(),
                                scuff: kind.trim().eq_ignore_ascii_case("FOOTSCUFF"),
                            },
                        });
                    }
                }
                "PLAYSOUNDTRACK" | "PLAYSOUNDTRACKTBS" | "STOPSOUNDTRACK"
                | "STOPALLSOUNDTRACKS" => {
                    let upper = kind.trim().to_ascii_uppercase();
                    let name = rest
                        .split(',')
                        .next()
                        .unwrap_or("")
                        .trim()
                        .to_ascii_uppercase();
                    let what = match upper.as_str() {
                        "STOPALLSOUNDTRACKS" => CueKind::StopSoundtrack(None),
                        _ if name.is_empty() => continue,
                        "STOPSOUNDTRACK" => CueKind::StopSoundtrack(Some(name)),
                        _ => CueKind::PlaySoundtrack {
                            name,
                            once: upper == "PLAYSOUNDTRACKTBS",
                        },
                    };
                    cues.push(Cue { frame, what });
                }
                "DIALOGUE" => {
                    let plate = rest.split(',').next().unwrap_or("").trim();
                    // Named for the English line (`E` first) almost always.
                    let plate = match plate.strip_prefix(['E', 'e']) {
                        Some(rest) if plate.len() > 10 => rest,
                        _ => plate,
                    };
                    if !plate.is_empty() {
                        cues.push(Cue {
                            frame,
                            what: CueKind::Dialogue(plate.to_ascii_uppercase()),
                        });
                    }
                }
                "CAMERA" => {
                    let mut f = rest.split(',').map(str::trim);
                    if let Some(name) = f.next().filter(|n| !n.is_empty()) {
                        cues.push(Cue {
                            frame,
                            what: CueKind::Camera {
                                name: name.to_ascii_uppercase(),
                                glide: f.next().is_some_and(|g| g.eq_ignore_ascii_case("GLIDE")),
                            },
                        });
                    }
                }
                "SPEAKERCAPTION" => {
                    // end frame, speaker, caption
                    let mut f = rest.splitn(3, ',');
                    let end = f.next().and_then(|e| e.trim().parse().ok());
                    let who = f.next().map(|w| w.trim().to_ascii_uppercase());
                    captions.push(Caption {
                        frame,
                        end,
                        speaker: who,
                        text: f.next().unwrap_or("").trim().to_string(),
                    });
                }
                _ => {}
            }
        }
        let frame_rate = doc
            .sections("OPTIONS")
            .flat_map(|s| &s.lines)
            .find_map(|line| {
                let fields: Vec<&str> = line.text.split(',').map(str::trim).collect();
                match &fields[..] {
                    [_, option, rate, ..] if option.eq_ignore_ascii_case("FRAMERATE") => {
                        rate.parse().ok().filter(|&r: &u32| r > 0)
                    }
                    _ => None,
                }
            })
            .unwrap_or(15);
        Self {
            frames,
            actions,
            sounds,
            captions,
            faces,
            cues,
            dialogue_cue,
            frame_rate,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sound_lines_place_their_sounds_as_the_original_reads_them() {
        let anm = Animation::parse(
            b"[HEADER]\r\n40\r\n[SOUNDS]\r\n5\r\n\
              10,ChrSqk04.wav,100,ABE,50,300  //Abbe shifts\r\n\
              7,DoorOpen.wav,100,EASTDOOR\r\n\
              1,CS2StrMove1.wav,100,10,0,15,100,1000\r\n\
              15,GraGrunt3.wav,100,gra,grashov2,100,650\r\n\
              3,Plain.wav,80\r\n",
        );
        let place = |i: usize| anm.sounds[i].place.clone();
        assert_eq!(
            place(0),
            Some(SoundPlace { model: Some("ABE".into()), offset: [0.0; 3], near: 50.0, far: 300.0 })
        );
        // A model alone: never faded, but still heard from it.
        assert_eq!(place(1).unwrap().near, SOUND_DISTANCE_UNSET);
        // A position, written x, z, y.
        assert_eq!(
            place(2),
            Some(SoundPlace { model: None, offset: [10.0, 15.0, 0.0], near: 100.0, far: 1000.0 })
        );
        // What cannot be read as a distance is left unset.
        assert_eq!(place(3).unwrap().near, SOUND_DISTANCE_UNSET);
        assert_eq!(place(4), None);
        assert_eq!(anm.sounds[4].volume, 80);
    }

    #[test]
    fn visibility_of_models_and_parts() {
        let anm = Animation::parse(
            b"[HEADER]\r\n30\r\n[MVISIBILITY]\r\n2\r\n0,abebinocs, ON\r\n12, gab, 3, 1, OFF\r\n",
        );
        assert_eq!(
            anm.cues[0].what,
            CueKind::Visible {
                model: "ABEBINOCS".into(),
                visible: true
            }
        );
        assert_eq!(
            anm.cues[1],
            Cue {
                frame: 12,
                what: CueKind::PartVisible {
                    model: "GAB".into(),
                    mesh: 3,
                    group: 1,
                    visible: false
                }
            }
        );
    }

    #[test]
    fn texture_swaps() {
        let anm = Animation::parse(
            b"[HEADER]\r\n30\r\n[MTEXTURES]\r\n2\r\n0, alarmclock, 0, 3, 200am\r\n\
              29, alarmclock, 0, 3, lhidigclkfc\r\n[STEXTURES]\r\n1\r\n5,bar,barsign,barsign_on\r\n",
        );
        assert_eq!(anm.cues.len(), 3);
        assert_eq!(
            anm.cues[1].what,
            CueKind::ModelTexture {
                model: "ALARMCLOCK".into(),
                mesh: 0,
                group: 3,
                texture: "LHIDIGCLKFC".into()
            }
        );
        assert_eq!(
            anm.cues[2],
            Cue {
                frame: 5,
                what: CueKind::SceneTexture {
                    model: "BARSIGN".into(),
                    texture: "BARSIGN_ON".into()
                }
            }
        );
    }

    #[test]
    fn moments_cue_lines_and_cameras() {
        let anm = Animation::parse(
            b"[HEADER]\r\n141\r\n[GK3]\r\n3\r\n15,DIALOGUE,E174AY0W5Z5\r\n\
              62,DIALOGUE,E174AY0W5Z6\r\n59,CAMERA,VIEW_OF_SPIT\r\n",
        );
        assert_eq!(
            anm.cues,
            [
                Cue {
                    frame: 15,
                    what: CueKind::Dialogue("174AY0W5Z5".into())
                },
                Cue {
                    frame: 62,
                    what: CueKind::Dialogue("174AY0W5Z6".into())
                },
                Cue {
                    frame: 59,
                    what: CueKind::Camera {
                        name: "VIEW_OF_SPIT".into(),
                        glide: false
                    }
                },
            ]
        );
    }

    #[test]
    fn reads_dialogue() {
        let yak = Animation::parse(
            b"[HEADER]\r\n52\r\n[SOUNDS]\r\n1\r\n0,A0NQIB44.QR1,100\r\n[GK3]\r\n3\r\n\
              0,SPEAKER,GABRIEL\r\n0,CAPTION,Well, that's odd.\r\n\
              5,SpeakerCaption, 149, MOSELY,Starring: me, you\r\n",
        );
        assert_eq!(yak.frames, 52);
        assert_eq!(yak.sounds[0].name, "A0NQIB44.QR1");
        assert_eq!(yak.captions[0].speaker.as_deref(), Some("GABRIEL"));
        assert_eq!(yak.captions[0].text, "Well, that's odd.");
        assert_eq!(yak.captions[1].speaker.as_deref(), Some("MOSELY"));
        assert_eq!(yak.captions[1].text, "Starring: me, you");
        assert_eq!(yak.captions[1].end, Some(149));
        assert_eq!(yak.frame_rate, 15);
        let movie = Animation::parse(b"[HEADER]\r\n30\r\n[OPTIONS]\r\n1\r\n0,FRAMERATE,30\r\n");
        assert_eq!(movie.frame_rate, 30);
    }

    #[test]
    fn reads_face_changes() {
        let anm = Animation::parse(
            b"[HEADER]\r\n4\r\n[GK3]\r\n4\r\n0,FACETEX,GABRIEL,GAB_BLINK_01,E\r\n\
              2,LIPSYNCH,GABRIEL,Mouth03\r\n3,UNFACETEX,GABRIEL,E\r\n",
        );
        assert_eq!(anm.faces.len(), 3);
        assert_eq!(
            anm.faces[0].change,
            FaceChange::Paint {
                bitmap: "GAB_BLINK_01".into(),
                part: 'E'
            }
        );
        assert_eq!(anm.faces[1].change, FaceChange::Mouth("MOUTH03".into()));
        assert_eq!(
            anm.faces[2],
            FaceEvent {
                frame: 3,
                noun: "GABRIEL".into(),
                change: FaceChange::Clear('E')
            }
        );
    }

    #[test]
    fn reads_header_and_actions() {
        let anm = Animation::parse(
            b"[HEADER]\r\n31\r\n[ACTIONS]\r\n2\r\n0,gab_GabBreath1\r\n\
              5,wilkes_moped,-12,0,88,90,1200,0,-40,270\r\n[OPTIONS]\r\n1\r\n0,SIMPLE,3\r\n",
        );
        assert_eq!(anm.frames, 31);
        assert_eq!(anm.actions.len(), 2);
        assert_eq!(anm.actions[0].clip, "gab_GabBreath1");
        assert!(anm.actions[0].placement.is_none());
        assert_eq!(anm.actions[1].placement.unwrap()[4], 1200.0);
    }
}
