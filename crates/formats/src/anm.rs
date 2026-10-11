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
}

#[derive(Debug, Clone, PartialEq)]
pub struct Sound {
    pub frame: u32,
    /// An audio asset, often with a dialogue code for its extension.
    pub name: String,
    pub volume: i32,
}

/// A line of dialogue shown on screen.
#[derive(Debug, Clone, PartialEq)]
pub struct Caption {
    pub frame: u32,
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
                });
            }
        }

        // Captions contain commas, so everything past the fields belongs to
        // the text. SPEAKER and CAPTION come as a pair of lines.
        let mut captions = Vec::new();
        let mut speaker: Option<String> = None;
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
                    speaker: speaker.clone(),
                    text: rest.trim().to_string(),
                }),
                "SPEAKERCAPTION" => {
                    // end frame, speaker, caption
                    let mut f = rest.splitn(3, ',');
                    let _end = f.next();
                    let who = f.next().map(|w| w.trim().to_ascii_uppercase());
                    captions.push(Caption {
                        frame,
                        speaker: who,
                        text: f.next().unwrap_or("").trim().to_string(),
                    });
                }
                _ => {}
            }
        }
        Self {
            frames,
            actions,
            sounds,
            captions,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
