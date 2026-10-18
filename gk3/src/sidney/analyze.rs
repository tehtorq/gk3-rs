//! Analyze: open a file and let SIDNEY find what it hides. The parchments
//! give up a message and shapes, the paintings their geometry and the words
//! in them; tapes and notes are sent to Translate.
//!
//! As G-Engine's `SidneyAnalyze` and `_Image`/`_Text`, step for step. The
//! map (Le Serpent Rouge's puzzle) is in `lsr_map`, the anagram parser in
//! `anagram`.

use gk3_game::state::GameState;

use super::steps::{Popup, Step, ToolStep};
use super::{Event, Kind, Sidney, file};

/// Files by number.
pub const MAP: u32 = 19;
pub const PARCHMENT_1: u32 = 20;
pub const PARCHMENT_2: u32 = 21;
pub const POUSSIN: u32 = 22;
pub const TENIERS_1: u32 = 23;
pub const TENIERS_2: u32 = 24;
pub const HERMETIC: u32 = 25;
pub const SUM_NOTE: u32 = 26;
pub const ARCADIA: u32 = 29;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum State {
    #[default]
    Empty,
    PreAnalyze,
    Image,
    Text,
    Audio,
    Map,
}

/// A menu: its name's key, whether it opens, and its entries: each one's
/// key, whether it can be chosen, and what it is (`None` for a divider).
pub type Menu = (String, bool, Vec<(String, bool, Option<Item>)>);

/// The menus' entries, by menu then position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Item {
    OpenFile,
    ExtractAnomalies,
    Translate,
    AnagramParser,
    AnalyzeText,
    ViewGeometry,
    RotateShape,
    ZoomClarify,
    UseShape,
    EraseShape,
    EnterPoints,
    ClearPoints,
    DrawGrid,
    EraseGrid,
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Analyze {
    pub file: Option<u32>,
    pub state: State,
    pub map: Option<super::lsr_map::Map>,
    /// Shapes have been saved, for the map's Use and Erase Shape.
    pub has_shapes: bool,
}

impl Analyze {
    pub fn pre(&mut self) {
        self.state = State::PreAnalyze;
    }

    /// The picture being analysed, and where it is.
    pub fn picture(&self) -> Option<(&'static str, (f32, f32))> {
        if self.state != State::Image {
            return None;
        }
        Some(match self.file? {
            PARCHMENT_1 => ("PARCHMENT1_BASE", (10.0, 50.0)),
            PARCHMENT_2 => ("PARCHMENT2_BASE", (10.0, 50.0)),
            POUSSIN => ("POUSSIN", (10.0, 107.0)),
            TENIERS_2 => ("TENIERS", (10.0, 50.0)),
            _ => return None,
        })
    }

    /// The menus: name key, then each entry's key, whether it can be
    /// chosen, and what it is (`None` for a divider).
    pub fn menus(&self) -> Vec<Menu> {
        use Item::*;
        let (s, f) = (self.state, self.file);
        let image = s == State::Image;
        let text_on = matches!(s, State::Image | State::Text | State::Audio)
            || (s == State::PreAnalyze && f == Some(SUM_NOTE));
        let map = s == State::Map;
        let graphic_on = (image && f != Some(SUM_NOTE)) || map;
        let shapes = map && self.has_shapes;
        let text = |item: Item| -> bool {
            matches!(
                (s, f, item),
                (State::Image, Some(PARCHMENT_1), ExtractAnomalies)
                    | (State::Image, Some(PARCHMENT_2), AnalyzeText)
                    | (State::Image | State::PreAnalyze, Some(SUM_NOTE), Translate)
                    | (State::Audio, _, Translate)
                    | (State::Text, Some(ARCADIA), Translate | AnagramParser)
            )
        };
        let graphic = |item: Item| -> bool {
            matches!(
                (f, item),
                (Some(PARCHMENT_1), ViewGeometry)
                    | (Some(PARCHMENT_2), ViewGeometry | RotateShape)
                    | (Some(POUSSIN | TENIERS_2), ViewGeometry | ZoomClarify)
            )
        };
        vec![
            (
                "Menu1Name".into(),
                true,
                vec![("MenuItem1".into(), true, Some(OpenFile))],
            ),
            (
                "Menu2Name".into(),
                text_on,
                vec![
                    (
                        "Menu2Item1".into(),
                        text(ExtractAnomalies),
                        Some(ExtractAnomalies),
                    ),
                    ("Menu2Item2".into(), text(Translate), Some(Translate)),
                    (
                        "Menu2Item3".into(),
                        text(AnagramParser),
                        Some(AnagramParser),
                    ),
                    ("Menu2Item4".into(), text(AnalyzeText), Some(AnalyzeText)),
                ],
            ),
            (
                "Menu3Name".into(),
                graphic_on,
                vec![
                    (
                        "Menu3Item1".into(),
                        graphic(ViewGeometry),
                        Some(ViewGeometry),
                    ),
                    ("Menu3Item2".into(), graphic(RotateShape), Some(RotateShape)),
                    ("Menu3Item3".into(), graphic(ZoomClarify), Some(ZoomClarify)),
                    ("".into(), false, None),
                    ("Menu3Item5".into(), shapes, Some(UseShape)),
                    ("Menu3Item7".into(), shapes, Some(EraseShape)),
                ],
            ),
            (
                "Menu4Name".into(),
                s == State::Map,
                vec![
                    ("Menu4Item1".into(), map, Some(EnterPoints)),
                    ("Menu4Item2".into(), map, Some(ClearPoints)),
                    ("".into(), false, None),
                    ("Menu4Item4".into(), map, Some(DrawGrid)),
                    ("Menu4Item5".into(), map, Some(EraseGrid)),
                ],
            ),
        ]
    }
}

impl Sidney {
    fn msg(&self, key: &str) -> Popup {
        Popup::text(self.strings.get("Analyze Screen", key))
            .left()
            .ok(self.strings.get("Analyze Screen", "OKButton"))
    }

    fn centered(&self, key: &str) -> Popup {
        let mut p = self.msg(key);
        p.center = true;
        p
    }

    /// A message with no button, shown while something else goes on.
    fn passing(&self, key: &str) -> Popup {
        let mut p = Popup::text(self.strings.get("Analyze Screen", key));
        p.center = true;
        p
    }

    fn yes_no(&self, popup: Popup, yes: Vec<Step>) -> Popup {
        let s = &self.strings;
        Popup {
            buttons: vec![
                (s.get("Analyze Screen", "YesButton"), yes),
                (s.get("Analyze Screen", "NoButton"), Vec::new()),
            ],
            ..popup
        }
    }

    /// "Suggested language?": French finds the message; the others say why
    /// not, and ask again.
    pub(super) fn languages(&self, key: &'static str, french: Vec<Step>) -> Popup {
        let s = &self.strings;
        let again = Step::Tool(ToolStep::Languages(key, french.clone()));
        let mut ask = Popup::text(s.get("Analyze Screen", key)).left();
        ask.buttons = vec![
            (s.get("Analyze Screen", "French"), french),
            (
                s.get("Analyze Screen", "English"),
                vec![Step::Show(self.centered("ParchEnglish")), again.clone()],
            ),
            (
                s.get("Analyze Screen", "Latin"),
                vec![Step::Show(self.centered("ParchLatin")), again],
            ),
        ];
        ask
    }

    /// Opens a file in Analyze.
    pub(super) fn analyze_open(&mut self, id: u32, state: &GameState) {
        self.analyze.file = Some(id);
        self.videos = Default::default();
        self.analyze.state = if file(id).is_none() {
            State::Empty
        } else if !state.sidney_analyzed.contains(&id) {
            State::PreAnalyze
        } else {
            state_of(id)
        };
        self.analyze.has_shapes = super::lsr_map::SHAPE_FILES
            .iter()
            .any(|(f, _)| state.sidney_files.contains(f));
        if self.analyze.state == State::Map {
            self.map_open(state);
        }
    }

    /// START ANALYSIS.
    pub(super) fn analyze_go(&mut self, state: &GameState) -> Vec<Event> {
        let Some(id) = self.analyze.file else {
            return Vec::new();
        };
        let mut events = Vec::new();
        self.analyze.state = state_of(id);
        if self.analyze.state == State::Map {
            self.map_open(state);
        }
        let analysed = state.sidney_analyzed.contains(&id);
        let mut pre = false;
        let steps: Vec<Step> = match (self.analyze.state, id) {
            (State::Image, PARCHMENT_1) => vec![Step::Show(self.msg("AnalyzeParch1"))],
            (State::Image, PARCHMENT_2) => vec![Step::Show(self.msg("AnalyzeParch2"))],
            (State::Image, POUSSIN) => vec![Step::Show(self.msg("AnalyzePous"))],
            (State::Image, TENIERS_1) => {
                pre = true;
                vec![
                    Step::Show(self.centered("AnalyzeTemp")),
                    Step::Tool(ToolStep::AnalyzePre),
                ]
            }
            (State::Image, TENIERS_2) => {
                let mut v = vec![Step::Show(self.msg("AnalyzePous"))];
                if !analysed {
                    v.push(Step::Do(Event::Say("02o3h2zq32".into(), 1)));
                }
                v.push(Step::Do(Event::SetFlag("AnalyzedBasePainting3")));
                v
            }
            (State::Image, HERMETIC) => {
                pre = true;
                vec![
                    Step::Show(self.centered("AnalyzeHermNote")),
                    Step::Do(Event::Score("e_sidney_analysis_symbols_from_serres".into())),
                    Step::Tool(ToolStep::AnalyzePre),
                ]
            }
            (State::Image, SUM_NOTE) => vec![Step::Show(self.centered("AnalyzeSUM"))],
            (State::Text, ARCADIA) => vec![Step::Show(self.msg("ArcadiaAnalysis"))],
            (State::Audio, _) => vec![Step::Show(self.centered("AnalyzeTape"))],
            (State::Map, _) => self.map_analyze(state),
            (State::PreAnalyze, _) => {
                pre = true;
                // Prints and plates go to Suspects; SIDNEY says as much.
                let key = match (file(id).map(|f| f.kind), id) {
                    (Some(Kind::Fingerprint), 0..=10) => "AnalyzeKPrint",
                    (Some(Kind::Fingerprint), _) => "AnalyzeUPrint",
                    (Some(Kind::License), _) => "AnalyzeLicense",
                    _ => "AnalyzeSidFile",
                };
                vec![Step::Show(self.centered(key))]
            }
            _ => {
                pre = true;
                vec![Step::Tool(ToolStep::AnalyzePre)]
            }
        };
        self.then(steps);
        if !pre {
            events.push(Event::Analyzed(id));
        }
        events
    }

    /// A choice from Analyze's menus.
    pub(super) fn analyze_item(&mut self, item: Item, state: &GameState) -> Vec<Event> {
        use Item::*;
        let Some(id) = self.analyze.file else {
            if item == OpenFile {
                self.files_for = Some(super::Tool::Analyze);
                self.files_open = true;
            }
            return Vec::new();
        };
        if self.analyze.state == State::Map
            && matches!(
                item,
                UseShape | EraseShape | EnterPoints | ClearPoints | DrawGrid | EraseGrid
            )
        {
            let steps = self.map_item(item, state);
            self.then(steps);
            return Vec::new();
        }
        let at = self.analyze.picture().map_or((10.0, 50.0), |(_, at)| at);
        let s = &self.strings;
        let say = |plate: &str, n: i32| Step::Do(Event::Say(plate.into(), n));
        let score = |name: &str| Step::Do(Event::Score(name.into()));
        let flag = |name: &'static str| Step::Do(Event::SetFlag(name));
        let add = |file: u32| Step::Do(Event::AddFile(file));
        let magenta = [255, 0, 255];
        let green = [0, 255, 0];
        let steps: Vec<Step> = match (item, id) {
            (OpenFile, _) => {
                self.files_for = Some(super::Tool::Analyze);
                self.files_open = true;
                Vec::new()
            }
            (Translate, _) => {
                self.screen = super::Screen::Translate;
                self.translate_open(id, state);
                Vec::new()
            }
            (ExtractAnomalies, PARCHMENT_1) => {
                let mut french = vec![
                    Step::Show(self.msg("Parch1French")),
                    score("e_sidney_analysis_anomalies_parch1"),
                ];
                if !state.flags.contains("PLAYEDPARCH1FRENCHMSG") {
                    french.extend([say("02OCR2ZMX3", 1), flag("PlayedParch1FrenchMsg")]);
                }
                vec![Step::Show(self.languages("ExtractParch1", french))]
            }
            (AnalyzeText, PARCHMENT_2) => {
                let mut french = vec![
                    Step::Show(self.msg("Parch2French")),
                    score("e_sidney_analysis_anomalies_parch2"),
                ];
                if !state.flags.contains("GOTTHEMAPPLES") {
                    french.extend([
                        say("02OCR2Z4L3", 2),
                        Step::Do(Event::GiveItem("BLUE_APPLES_RIDDLE")),
                        flag("GotThemApples"),
                    ]);
                }
                vec![
                    Step::Do(Event::Sound("SIDMODEM")),
                    Step::Show(self.passing("Text1Parch2")),
                    Step::Wait(3.0),
                    Step::Show(self.passing("Text2Parch2")),
                    Step::Wait(3.0),
                    Step::Show(self.passing("Text3Parch2")),
                    Step::Wait(3.0),
                    Step::Show(self.languages("Text4Parch2", french)),
                ]
            }
            (ViewGeometry, PARCHMENT_1) => vec![
                Step::Video {
                    name: "Parch1Geo.avi",
                    slot: 0,
                    at,
                    then: Some(("GEOMPARCH1FINAL", magenta)),
                },
                Step::Show(self.centered("GeometryParch1").at(180.0, 4.0)),
                add(37),
                score("e_sidney_analysis_gemoetry_parch1"),
                flag("AnalyzedGeomParchment1"),
            ],
            (ViewGeometry, PARCHMENT_2) => vec![
                Step::Video {
                    name: "Parch2Geo.avi",
                    slot: 0,
                    at,
                    then: Some(("GEOMPARCH2FINAL", magenta)),
                },
                Step::Show(self.centered("GeometryParch2").at(180.0, 4.0)),
                add(38),
                add(39),
                score("e_sidney_analysis_gemoetry_parch2"),
                flag("AnalyzedGeomParchment2"),
            ],
            (ViewGeometry, POUSSIN) => vec![
                Step::Video {
                    name: "Poussingeo.avi",
                    slot: 0,
                    at: (at.0 - 8.0, at.1 - 68.0),
                    then: Some(("GEOMPOUSSINFINAL", magenta)),
                },
                Step::Show(self.centered("GeometryPous").at(180.0, 0.0)),
                add(40),
                score("e_sidney_analysis_gemoetry_poussin"),
                flag("AnalyzedGeomPainting1"),
                flag("SavedHexagram"),
            ],
            (ViewGeometry, TENIERS_2) => {
                let v = (at.0, at.1 - 20.0);
                let note = |k: &str| Step::Show(self.passing(k).at(190.0, 160.0));
                let mut steps = vec![
                    note("GeometryTenier1"),
                    Step::Video {
                        name: "TenierGeoA.avi",
                        slot: 0,
                        at: v,
                        then: Some(("TENIERGEOA", green)),
                    },
                    note("GeometryTenier2"),
                    Step::Wait(4.0),
                    Step::Video {
                        name: "TenierGeob.avi",
                        slot: 0,
                        at: v,
                        then: Some(("TENIERGEOB", green)),
                    },
                    note("GeometryTenier3"),
                    Step::Wait(4.0),
                    // A still with no movie before it.
                    Step::Video {
                        name: "",
                        slot: 0,
                        at: v,
                        then: Some(("TENIERGEOC", green)),
                    },
                    Step::Do(Event::Sound("SIDBUTTON4")),
                    note("GeometryTenier4"),
                    Step::Wait(4.0),
                    Step::Video {
                        name: "TenierGeoD.avi",
                        slot: 1,
                        at: v,
                        then: Some(("GEOMTENNIERSFINAL", magenta)),
                    },
                    Step::Show(self.centered("GeometryTenier5").at(190.0, 164.0)),
                ];
                if !state.flags.contains("ANALYZEDGEOMPAINTING3") {
                    steps.extend([say("02O2F2ZQ37", 2), flag("AnalyzedGeomPainting3")]);
                }
                steps
            }
            (RotateShape, PARCHMENT_2) => vec![
                Step::Video {
                    name: "parch2zoom.avi",
                    slot: 2,
                    at,
                    then: None,
                },
                Step::Clear(2),
                score("e_sidney_analysis_view_rotation_parch2"),
                say("02OEV2Z4L1", 1),
                Step::Show(
                    self.centered("RotateParch2")
                        .image("ZION_ROT")
                        .at(-112.0, 0.0)
                        .sized(135.0, 243.0),
                ),
            ],
            (ZoomClarify, POUSSIN) => {
                let saved = Popup::text(s.get("Analyze Screen", "SavingArcadia"))
                    .ok(s.get("Analyze Screen", "OKButton"));
                let ask = Popup::text(s.get("Analyze Screen", "SaveArcadia"))
                    .image("POUSSIN_ZOOM")
                    .at(-106.0, 0.0);
                vec![
                    Step::Video {
                        name: "poussinzoom.avi",
                        slot: 2,
                        at: (at.0 - 8.0, at.1 - 68.0),
                        then: None,
                    },
                    Step::Clear(2),
                    score("e_sidney_analysis_view_words_poussin"),
                    Step::Show(self.yes_no(
                        ask,
                        vec![add(ARCADIA), flag("SavedArcadiaText"), Step::Show(saved)],
                    )),
                ]
            }
            (ZoomClarify, TENIERS_2) => {
                let ask = Popup::text(s.get("Analyze Screen", "GetVerse"))
                    .image("TENIERS_ZOOM")
                    .at(-78.0, -37.0);
                let yes = vec![
                    Step::Do(Event::Sound("SIDMODEM")),
                    Step::Show(self.passing("RetrieveVerse")),
                    Step::Wait(6.0),
                    Step::Show(self.msg("Verse")),
                    say("02OCB2ZQ35", 1),
                ];
                vec![
                    Step::Video {
                        name: "Tenierzoom.avi",
                        slot: 2,
                        at: (at.0, at.1 - 20.0),
                        then: None,
                    },
                    Step::Clear(2),
                    score("e_sidney_analysis_view_words_tenier"),
                    Step::Show(self.yes_no(ask, yes)),
                ]
            }
            (AnagramParser, ARCADIA) if super::lsr_step(state) >= 10 => {
                let full = state.flags.contains("ARCADIACOMPLETE");
                let key = if full { "ArcadiaText" } else { "ArcadiaText2" };
                self.anagram = Some(super::anagram::Anagram::new(
                    &self.strings.get("Analyze Screen", key),
                    full,
                    &self.strings,
                ));
                vec![flag("StartArcadiaAnagram")]
            }
            (AnagramParser, _) => vec![say("02O3H2Z951", 1)],
            _ => vec![Step::Show(self.centered("NotImplemented"))],
        };
        self.then(steps);
        Vec::new()
    }
}

/// The state a file is analysed in.
fn state_of(id: u32) -> State {
    match file(id).map(|f| f.kind) {
        _ if id == MAP => State::Map,
        Some(Kind::Image) => State::Image,
        Some(Kind::Text) => State::Text,
        Some(Kind::Audio) => State::Audio,
        _ => State::PreAnalyze,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menus_follow_the_file() {
        let mut a = Analyze {
            file: Some(PARCHMENT_2),
            state: State::Image,
            ..Default::default()
        };
        let menus = a.menus();
        let on = |m: usize, i: usize| menus[m].2[i].1;
        assert!(
            menus[1].1 && on(1, 3) && !on(1, 0),
            "parchment 2: analyse text only"
        );
        assert!(on(2, 0) && on(2, 1) && !on(2, 2), "geometry and rotation");
        a.file = Some(ARCADIA);
        a.state = State::Text;
        let menus = a.menus();
        assert!(!menus[2].1, "no graphics for text");
        assert!(menus[1].2[1].1 && menus[1].2[2].1);
    }
}
