//! Translate: the Abbé's and Buchelli's tapes, the SUM note and the
//! Arcadia text, from their languages into English.
//!
//! As G-Engine's `SidneyTranslate`. The Arcadia text is short a word: once
//! translated, SIDNEY asks for it, and "sum" completes it.

use gk3_game::state::GameState;

use super::steps::{Answer, Popup, Step, ToolStep};
use super::{Event, Sidney, file};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Language {
    English,
    Latin,
    French,
    Italian,
}

pub const LANGUAGES: [Language; 4] = [
    Language::English,
    Language::Latin,
    Language::French,
    Language::Italian,
];

impl Language {
    pub fn key(self) -> &'static str {
        match self {
            Language::English => "English",
            Language::Latin => "Latin",
            Language::French => "French",
            Language::Italian => "Italian",
        }
    }
}

/// What can be translated: the file, its language, the strings it is
/// made of (and those once the Arcadia word is in), and what doing it gives.
struct Translation {
    file: u32,
    language: Language,
    prefix: &'static str,
    done_prefix: Option<&'static str>,
    flag: &'static str,
    score: &'static str,
}

const TRANSLATIONS: &[Translation] = &[
    Translation {
        file: 27,
        language: Language::French,
        prefix: "AbbeTape",
        done_prefix: None,
        flag: "TranslatedAbbeTape",
        score: "e_sidney_translate_tape_abbe",
    },
    Translation {
        file: 28,
        language: Language::Italian,
        prefix: "BuchTape",
        done_prefix: None,
        flag: "TranslatedBuchTape",
        score: "e_sidney_translate_tape_buchelli",
    },
    Translation {
        file: 26,
        language: Language::Latin,
        prefix: "SUMScript",
        done_prefix: None,
        flag: "TranslatedSUM",
        score: "e_sidney_translate_sum",
    },
    Translation {
        file: 29,
        language: Language::Latin,
        prefix: "ArcadiaText",
        done_prefix: Some("ArcSUMText"),
        flag: "ArcadiaTranslated",
        score: "e_sidney_translate_arcadia",
    },
];

fn translation(id: u32) -> Option<&'static Translation> {
    TRANSLATIONS.iter().find(|t| t.file == id)
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Translate {
    pub file: Option<u32>,
    pub from: Language,
    pub to: Language,
    performed: bool,
    pub text: String,
    pub scroll: f32,
}

impl Default for Translate {
    fn default() -> Self {
        Self {
            file: None,
            from: Language::English,
            to: Language::English,
            performed: false,
            text: String::new(),
            scroll: 0.0,
        }
    }
}

impl Sidney {
    /// Lines `PREFIX1`, `PREFIX2`... of the Translate screen's strings.
    fn body(&self, prefix: &str) -> String {
        (1..)
            .map_while(|i| {
                self.strings
                    .find("Translate Screen", &format!("{prefix}{i}"))
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn translate_prefix(&self, t: &Translation, state: &GameState) -> &'static str {
        match t.done_prefix {
            Some(done) if state.flags.contains(&gk3_game::state::key(t.flag)) => done,
            _ => t.prefix,
        }
    }

    /// Opens a file in Translate; something that cannot be translated says so.
    pub(super) fn translate_open(&mut self, id: u32, state: &GameState) {
        let s = &self.strings;
        let Some(t) = translation(id) else {
            let note = Popup::text(s.get("Translate Screen", "NotTranslatable"))
                .ok(s.get("Analyze Screen", "OKButton"));
            self.then([Step::Show(note)]);
            return;
        };
        if self.translate.file != Some(id) {
            self.translate.performed = false;
        }
        self.translate.file = Some(id);
        self.translate.scroll = 0.0;
        let name = s.file_name(id);
        let open = s.get("Translate Screen", "OpenFile").replace("%s", &name);
        let body = self.body(self.translate_prefix(t, state));
        self.translate.text = format!("{open}\n\n{body}");
        if self.translate.performed {
            self.translate.from = t.language;
            self.translate.to = Language::English;
            self.translate.text.push_str(&self.translated(t, state));
        }
    }

    fn translated(&self, t: &Translation, state: &GameState) -> String {
        let s = &self.strings;
        let into = s.get("Translate Screen", t.language.key());
        let header = s
            .get("Translate Screen", "Translating")
            .replace("%s", &into);
        let body = self.body(&format!("{}T", self.translate_prefix(t, state)));
        format!("\n\n{header}\n\n{body}")
    }

    /// TRANSLATE NOW.
    pub(super) fn translate_go(&mut self, state: &GameState) -> Vec<Event> {
        let s = &self.strings;
        let Some(id) = self.translate.file else {
            return Vec::new();
        };
        let Some(t) = translation(id) else {
            return Vec::new();
        };
        let ok = s.get("Analyze Screen", "OKButton");
        if self.translate.from != t.language {
            self.then([Step::Show(
                Popup::text(s.get("Translate Screen", "WrongFrom")).ok(ok),
            )]);
            return Vec::new();
        }
        if self.translate.to != Language::English {
            let line = if state.ego == "GABRIEL" {
                "02OC02ZFH1"
            } else {
                "02oc02zq91"
            };
            return vec![Event::Say(line.into(), 1)];
        }
        let missing_word =
            id == super::analyze::ARCADIA && !state.flags.contains("ARCADIACOMPLETE");
        if self.translate.performed {
            if missing_word {
                self.ask_missing_word();
            } else {
                self.then([Step::Show(
                    Popup::text(s.get("Translate Screen", "NoFurther")).ok(ok),
                )]);
            }
            return Vec::new();
        }
        let text = self.translated(t, state);
        self.translate.text.push_str(&text);
        self.translate.performed = true;
        let mut events = Vec::new();
        if id == 27
            && !state.flags.contains(&gk3_game::state::key(t.flag))
            && state.ego == "GABRIEL"
        {
            events.push(Event::Say("02OD95EPF2".into(), 1));
        }
        events.push(Event::SetFlag(t.flag));
        events.push(Event::Score(t.score.into()));
        if missing_word {
            self.ask_missing_word();
        }
        events
    }

    /// "There seems to be a word missing; add it?"
    fn ask_missing_word(&mut self) {
        let s = &self.strings;
        let text = format!(
            "\n  {}\n\n  {}",
            s.get("Translate Screen", "Subject"),
            s.get("Translate Screen", "Question")
        );
        let mut ask = Popup::text(text)
            .left()
            .at(88.0 - 180.0, -135.0 + 120.0)
            .sized(280.0, 120.0);
        ask.buttons = vec![
            (
                s.get("Analyze Screen", "YesButton"),
                vec![Step::Show(self.word_prompt())],
            ),
            (s.get("Analyze Screen", "NoButton"), Vec::new()),
        ];
        self.then([Step::Show(ask)]);
    }

    fn word_prompt(&self) -> Popup {
        let mut p = Popup::text(self.strings.get("Translate Screen", "Input"))
            .left()
            .sized(250.0, 60.0);
        p.input = Some((String::new(), Answer::ArcadiaWord));
        p
    }

    /// The word typed in: "sum" completes the text; anything else may be
    /// tried again.
    pub(super) fn arcadia_word(&mut self, typed: &str, _events: &mut Vec<Event>) {
        let s = &self.strings;
        if typed.trim().eq_ignore_ascii_case("sum") {
            let latin = s.get("Translate Screen", "Latin");
            let text = format!(
                "\n\n{}\n\n{}\n\n{}\n\n{}",
                s.get("Translate Screen", "ArcSUMText1"),
                s.get("Translate Screen", "Translating")
                    .replace("%s", &latin),
                s.get("Translate Screen", "ArcSUMTextT1"),
                s.get("Translate Screen", "Updating"),
            );
            self.then([
                Step::Tool(ToolStep::TranslateText(text)),
                Step::Do(Event::Say("02OFS2ZPF5".into(), 1)),
                Step::Do(Event::SetFlag("ArcadiaComplete")),
            ]);
        } else {
            let text = format!(
                "{}\n\n{}",
                s.get("Translate Screen", "BadInput1"),
                s.get("Translate Screen", "BadInput2")
            );
            let mut again = Popup::text(text).left().sized(280.0, 120.0);
            again.buttons = vec![
                (
                    s.get("Analyze Screen", "YesButton"),
                    vec![Step::Show(self.word_prompt())],
                ),
                (s.get("Analyze Screen", "NoButton"), Vec::new()),
            ];
            self.then([Step::Show(again)]);
        }
    }

    /// The file's name, for Translate's header.
    pub(super) fn translate_title(&self) -> String {
        self.translate
            .file
            .filter(|&id| file(id).is_some())
            .map(|id| self.strings.file_name(id))
            .unwrap_or_default()
    }
}
