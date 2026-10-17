//! SIDNEY, Grace's computer: a desktop of tools for the evidence the
//! player gathers. Scanned into it with Add Data, items become files, which
//! the other tools search, analyse, translate and match.
//!
//! As G-Engine's `Sidney` and its parts, which this follows. What it keeps
//! (files, e-mails) lives in the story state, so saves carry it; the rest is
//! the screen's own. It is drawn in the original's 640x480 pixels and
//! returns [`Event`]s for the game to carry out rather than changing the
//! world itself.

mod anagram;
mod analyze;
mod lsr_map;
mod makeid;
mod steps;
mod suspects;
mod translate;

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::Arc;

use gk3_barn::Library;
use gk3_formats::ini::{Document, Split};
use gk3_game::actions::parse_timeblock;
use gk3_game::state::GameState;
use gk3_render::{Overlay, Renderer};

use crate::ui::{Align, Space, Ui, inside};

/// What kind of evidence a file is, which decides the tool that opens it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Image,
    Audio,
    Text,
    Fingerprint,
    License,
    Shape,
}

/// A file SIDNEY can hold: its number, kind, the item scanned to make it,
/// and the score for adding it.
pub struct File {
    pub id: u32,
    pub kind: Kind,
    /// The inventory item scanned to make it; its picture is the file's
    /// icon in the tools still to come.
    #[allow(dead_code)]
    pub item: &'static str,
    pub score: &'static str,
}

/// Every file, in G-Engine's numbering, which `SidScanner` (less one) uses.
pub const FILES: &[File] = &{
    use Kind::*;
    const fn f(id: u32, kind: Kind, item: &'static str, score: &'static str) -> File {
        File {
            id,
            kind,
            item,
            score,
        }
    }
    [
        f(
            0,
            Fingerprint,
            "ABBE_FINGERPRINT",
            "e_sidney_add_fingerprint_abbe",
        ),
        f(
            1,
            Fingerprint,
            "BUCHELLIS_FINGERPRINT",
            "e_sidney_add_fingerprint_buchelli",
        ),
        f(
            2,
            Fingerprint,
            "BUTHANES_FINGERPRINT",
            "e_sidney_add_fingerprint_buthane",
        ),
        f(
            3,
            Fingerprint,
            "ESTELLES_FINGERPRINT",
            "e_sidney_add_fingerprint_estelle",
        ),
        f(
            4,
            Fingerprint,
            "HOWARDS_FINGERPRINT",
            "e_sidney_add_fingerprint_howard",
        ),
        f(
            5,
            Fingerprint,
            "MONTREAUX_FINGERPRINT",
            "e_sidney_add_fingerprint_montreaux",
        ),
        f(
            6,
            Fingerprint,
            "WILKES_FINGERPRINT",
            "e_sidney_add_fingerprint_wilkes",
        ),
        // No score: the player never adds this one directly.
        f(7, Fingerprint, "MOSELYS_FINGERPRINT", ""),
        f(
            8,
            Fingerprint,
            "LARRYS_FINGERPRINT",
            "e_sidney_add_fingerprint_larry",
        ),
        f(9, Fingerprint, "WILKES_FINGERPRINT_LABELED_BUCHELLI", ""),
        f(10, Fingerprint, "BUCHELLIS_FINGERPRINT_LABELED_WILKES", ""),
        f(
            11,
            Fingerprint,
            "UNKNOWN_PRINT_1",
            "e_sidney_add_manuscript_prints_buthane",
        ),
        f(
            12,
            Fingerprint,
            "UNKNOWN_PRINT_2",
            "e_sidney_add_manuscript_prints_mosley",
        ),
        f(
            13,
            Fingerprint,
            "UNKNOWN_PRINT_3",
            "e_sidney_add_manuscript_prints_buchelli",
        ),
        f(14, Fingerprint, "UNKNOWN_PRINT_4", ""),
        f(15, Fingerprint, "UNKNOWN_PRINT_5", ""),
        f(16, Fingerprint, "UNKNOWN_PRINT_6", ""),
        f(17, Fingerprint, "UNKNOWN_PRINT_7", ""),
        f(
            18,
            Fingerprint,
            "ESTELLES_FINGERPRINT_LSR",
            "e_sidney_add_fingerprint_lsr_unknown",
        ),
        f(19, Image, "MAP", "e_sidney_add_map"),
        f(20, Image, "PARCHMENT_1", "e_sidney_add_parch1"),
        f(21, Image, "PARCHMENT_2", "e_sidney_add_parch2"),
        f(22, Image, "POUSSIN_POSTCARD", "e_sidney_add_postcard_1"),
        f(
            23,
            Image,
            "TENIERS_POSTCARD_TEMP",
            "e_sidney_add_postcard_2",
        ),
        f(
            24,
            Image,
            "TENIERS_POSTCARD_NO_TEMP",
            "e_sidney_add_postcard_3",
        ),
        f(
            25,
            Image,
            "HERM_SYMBOLS",
            "e_sidney_add_hermetical_symbols_from_serres",
        ),
        f(26, Image, "I_AM_WORDS", "e_sidney_add_sum_note"),
        f(27, Audio, "ABBE_TAPE", "e_sidney_add_tape_abbe"),
        f(28, Audio, "BUCHELLI_TAPE", "e_sidney_add_tape_buchelli"),
        f(29, Text, "", ""),
        f(30, Text, "", ""),
        f(31, Image, "", ""),
        f(
            32,
            License,
            "BUCHELLIS_LICENSE",
            "e_sidney_add_license_buchelli",
        ),
        f(
            33,
            License,
            "EMILIOS_LICENSE",
            "e_sidney_add_license_emilio",
        ),
        f(
            34,
            License,
            "HOWARDS_LICENSE",
            "e_sidney_add_license_howard",
        ),
        f(
            35,
            License,
            "MOSELYS_LICENSE",
            "e_sidney_add_license_mosely",
        ),
        f(36, License, "WILKES_LICENSE", "e_sidney_add_license_wilkes"),
        f(37, Shape, "", ""),
        f(38, Shape, "", ""),
        f(39, Shape, "", ""),
        f(40, Shape, "", ""),
    ]
};

/// The folders, in order, with their names' keys in SIDNEY's strings.
const FOLDERS: &[(Kind, &str)] = &[
    (Kind::Image, "ImageDir"),
    (Kind::Audio, "AudioDir"),
    (Kind::Text, "TextDir"),
    (Kind::Fingerprint, "FingerDir"),
    (Kind::License, "LicenseDir"),
    (Kind::Shape, "ShapeDir"),
];

pub fn file(id: u32) -> Option<&'static File> {
    FILES.get(id as usize)
}

/// SIDNEY's interface text (`ESIDNEY.TXT`), by screen and key.
#[derive(Default)]
pub struct Strings(HashMap<String, HashMap<String, String>>);

impl Strings {
    pub fn load(lib: &Library) -> Self {
        let Ok(bytes) = lib.read("ESIDNEY.TXT").or_else(|_| lib.read("SIDNEY.TXT")) else {
            return Self::default();
        };
        let doc = Document::parse(&bytes, Split::Whole);
        let mut out: HashMap<String, HashMap<String, String>> = HashMap::new();
        for section in &doc.sections {
            let entries = out.entry(section.name.to_ascii_lowercase()).or_default();
            for line in &section.lines {
                if let Some((k, v)) = line.text.split_once('=') {
                    entries.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
                }
            }
        }
        Self(out)
    }

    pub fn get(&self, screen: &str, key: &str) -> String {
        self.find(screen, key).unwrap_or_else(|| key.to_string())
    }

    /// A string if there is one, with its `\n` and `\t` escapes made real.
    pub fn find(&self, screen: &str, key: &str) -> Option<String> {
        self.0
            .get(&screen.to_ascii_lowercase())
            .and_then(|s| s.get(&key.to_ascii_lowercase()))
            .map(|v| v.replace("\\n", "\n").replace("\\t", "    "))
    }

    /// A file's name, as Add Data lists it.
    pub fn file_name(&self, id: u32) -> String {
        self.get("AddData Screen", &format!("ScanItem{}", id + 1))
    }
}

#[derive(Debug, Clone, Default)]
pub struct Email {
    pub from: String,
    pub to: String,
    pub cc: String,
    pub date: String,
    pub subject: String,
    pub body: Vec<String>,
}

/// The e-mails, by id (`EMail1`), from `ESIDNEYEMAIL.TXT`.
pub fn load_emails(lib: &Library) -> BTreeMap<String, Email> {
    let Ok(bytes) = lib
        .read("ESIDNEYEMAIL.TXT")
        .or_else(|_| lib.read("SIDNEYEMAIL.TXT"))
    else {
        return BTreeMap::new();
    };
    let doc = Document::parse(&bytes, Split::Whole);
    let mut out = BTreeMap::new();
    for section in &doc.sections {
        if section.name.is_empty() || section.name.eq_ignore_ascii_case("Email Files") {
            continue;
        }
        let mut email = Email::default();
        for line in &section.lines {
            let Some((k, v)) = line.text.split_once('=') else {
                continue;
            };
            let (k, v) = (k.trim(), v.trim().to_string());
            match k.to_ascii_lowercase().as_str() {
                "from" => email.from = v,
                "to" => email.to = v,
                "cc" => email.cc = v,
                "date" => email.date = v,
                "subject" => email.subject = v,
                key if key.starts_with("body") => {
                    let i: usize = key[4..].parse().unwrap_or(1);
                    if email.body.len() < i {
                        email.body.resize(i, String::new());
                    }
                    email.body[i - 1] = if v.eq_ignore_ascii_case("<space>") {
                        String::new()
                    } else {
                        v
                    };
                }
                _ => {}
            }
        }
        out.insert(section.name.to_ascii_uppercase(), email);
    }
    out
}

/// What SIDNEY asks the game to do.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Say(String, i32),
    Sound(&'static str),
    SetFlag(&'static str),
    ClearFlag(&'static str),
    Variable(&'static str, i32),
    Score(String),
    AddFile(u32),
    /// A flag named at run time (from the page triggers' file).
    SetFlagOwned(String),
    ShowInventory,
    GiveItem(&'static str),
    /// A file has been analysed.
    Analyzed(u32),
    /// A movie's sound.
    Pcm(Arc<gk3_media::Pcm>),
    /// Close SIDNEY and go back to the room.
    Exit,
    /// Le Serpent Rouge has moved on: `LSRState` is to be brought up to
    /// date with the flags.
    LsrState,
    TakeItem(&'static str),
    /// SIDNEY closes and the story goes elsewhere.
    ChangeLocation(&'static str),
    /// SIDNEY closes if that ends the timeblock.
    ExitIfOver,
    /// A sound played over and over, or (`None`) stopped.
    LoopSound(Option<&'static str>),
}

/// A list to choose from: the grid's sizes, the shapes.
#[derive(Debug, Clone)]
pub(crate) struct Choice {
    title: String,
    rows: Vec<(Row, Vec<steps::Step>)>,
}

#[derive(Debug, Clone)]
enum Row {
    Text(String),
    Picture(String),
}

/// The tools on the desktop, left to right.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Search,
    Email,
    Files,
    Analyze,
    Translate,
    AddData,
    MakeId,
    Suspects,
}

const TOOLS: &[(Tool, &str)] = &[
    (Tool::Search, "SEARCH"),
    (Tool::Email, "EMAIL"),
    (Tool::Files, "FILES"),
    (Tool::Analyze, "ANALYZE"),
    (Tool::Translate, "TRANSL"),
    (Tool::AddData, "ADDATA"),
    (Tool::MakeId, "MAKEID"),
    (Tool::Suspects, "SUSPT"),
];

#[derive(Debug, Clone, PartialEq)]
pub enum Screen {
    Desktop,
    EmailList,
    Email(usize),
    Search,
    Analyze,
    Translate,
    MakeId,
    Suspects,
    /// A tool not built yet: its name, and a note.
    Unbuilt(Tool),
}

/// Add Data's progress.
#[derive(Debug, Clone, PartialEq)]
enum Adding {
    Idle,
    /// "Waiting for input", then the inventory comes up.
    Waiting {
        until: f64,
    },
    /// The inventory is up with the scanner; waiting for the player.
    Scanning,
    /// A message in the box until a time; `blink` flashes it while
    /// scanning, then the file is added.
    Box {
        text: String,
        until: f64,
        blink: bool,
        then: Option<u32>,
    },
}

/// A clickable thing, as last drawn.
#[derive(Debug, Clone, PartialEq)]
pub enum Hit {
    Exit,
    Tool(Tool),
    MainMenu,
    CloseFiles,
    File(u32),
    EmailItem(usize),
    EmailNext,
    EmailPrev,
    EmailReply,
    EmailCompose,
    EmailPrint,
    EmailClose,
    PopupOk,
    PopupButton(usize),
    MenuHeader(usize),
    MenuItem(usize, usize),
    AnalyzeGo,
    TranslateGo,
    /// A suspect's linked file, picked; and Match Analysis' buttons.
    Evidence(usize),
    MatchLink,
    MatchClose,
    /// Make ID's person (`GAB` or `GRA`), and PRINT.
    Person(&'static str),
    Print,
    TranslateOpen,
    /// A language button: from (true) or to, and which.
    Language(bool, usize),
    SearchGo,
    SearchReset,
    SearchBack,
    SearchForward,
    Link(String),
    /// A row of the list to choose from.
    Choice(usize),
    Anagram(anagram::AnagramHit),
}

/// A page visit's one-time line, from `SIDNEYDIALOG.TXT`: said if the
/// flag it sets is not yet set, the Le Serpent Rouge puzzle is within its
/// steps, and its required flags (`!` for not) hold.
#[derive(Debug, Clone, Default)]
struct Trigger {
    line: String,
    flag: String,
    requires: Vec<String>,
    lsr_min: Option<i32>,
    lsr_max: Option<i32>,
}

/// Search's state: what is typed, the page shown and how far down, and
/// the pages visited.
#[derive(Default)]
struct Search {
    input: String,
    page: Option<String>,
    scroll: f32,
    history: Vec<String>,
    at: usize,
    /// The page laid out: its name, pieces and height.
    laid_out: Option<(String, Vec<crate::html::Piece>, f32)>,
}

/// Pages that score when visited, and flags they set; G-Engine's tables.
const PAGE_SCORES: &[(&str, &str)] = &[
    ("VAMPIRE.HTML", "e_sidney_search_vampires"),
    ("HOLYGRAIL.HTML", "e_sidney_search_grail"),
    ("QUATERNITY.HTML", "e_sidney_search_quaternity"),
    ("STMICHAEL.HTML", "e_sidney_search_st_michael"),
    ("PYTHAGORAS.HTML", "e_sidney_search_pythagoras"),
    ("ASMODEUS.HTML", "e_sidney_search_asmodeus"),
    ("CHESSBOARD.HTML", "e_sidney_search_chessboard"),
    ("DUALITY.HTML", "e_sidney_search_duality"),
    ("SOLOMON.HTML", "e_sidney_search_solomon"),
    ("STVINCENT.HTML", "e_sidney_search_st_vincent"),
    ("TEMPLEENTRY.HTML", "e_sidney_search_temple_solomon"),
    (
        "TEMPLEFLOORPLAN.HTML",
        "e_sidney_analysis_send_email_for_floor_plan",
    ),
    ("SOUL.HTML", "e_sidney_search_soul"),
    ("HEXAGRAM.HTML", "e_sidney_search_hexagram"),
    ("SEAL.HTML", "e_sidney_search_seal"),
];
const PAGE_FLAGS: &[(&str, &str)] = &[
    ("VAMPIRE.HTML", "Vampire"),
    ("RA.HTML", "RA"),
    ("ASMODEUS.HTML", "Asmodeus"),
    ("QUATERNITY.HTML", "Quaternity"),
    ("STMICHAEL.HTML", "StMichael"),
    ("PYTHAGORAS.HTML", "Pythagoras"),
    ("CHESSBOARD.HTML", "Chessboard"),
    ("DUALITY.HTML", "Duality"),
    ("SOLOMON.HTML", "Solomon"),
    ("STVINCENT.HTML", "StVincent"),
    ("TEMPLEENTRY.HTML", "SolomonTemple"),
    ("SOUL.HTML", "Soul"),
    ("HEXAGRAM.HTML", "Hexagram"),
    ("SEAL.HTML", "Seal"),
    ("ALCHEMYTILTEDSQUARE.HTML", "AlchemyTiltedSquare"),
    ("TEMPLEFLOORPLAN.HTML", "TempleFloorplan"),
];

/// How far the Le Serpent Rouge puzzle has come: its signs solved.
pub fn lsr_step(state: &GameState) -> i32 {
    const SIGNS: &[&str] = &[
        "AQUARIUS",
        "PISCES",
        "ARIES",
        "TAURUS",
        "GEMINI",
        "CANCER",
        "LEO",
        "VIRGO",
        "LIBRA",
        "SCORPIO",
        "OPHIUCHUS",
        "SAGITTARIUS",
        "CAPRICORN",
    ];
    SIGNS.iter().filter(|s| state.flags.contains(**s)).count() as i32
}

/// Search terms, lower case, to the page each finds (`SIDSEARCH.TXT`).
fn load_terms(lib: &Library) -> HashMap<String, String> {
    let Ok(bytes) = lib.read("SIDSEARCH.TXT") else {
        return HashMap::new();
    };
    let doc = Document::parse(&bytes, Split::Whole);
    let mut out = HashMap::new();
    for section in &doc.sections {
        let page = crate::html::page_name(&section.name);
        for line in &section.lines {
            if let Some((k, v)) = line.text.split_once('=')
                && k.trim().eq_ignore_ascii_case("text")
            {
                for term in v.split(',') {
                    out.insert(term.trim().to_lowercase(), page.clone());
                }
            }
        }
    }
    out
}

fn load_triggers(lib: &Library) -> HashMap<String, Trigger> {
    let Ok(bytes) = lib.read("SIDNEYDIALOG.TXT") else {
        return HashMap::new();
    };
    let doc = Document::parse(&bytes, Split::Whole);
    let mut out = HashMap::new();
    for section in &doc.sections {
        let mut t = Trigger::default();
        for line in &section.lines {
            let Some((k, v)) = line.text.split_once('=') else {
                continue;
            };
            let v = v.split("//").next().unwrap_or("").trim();
            match k.trim().to_ascii_lowercase().as_str() {
                "me" => t.line = v.to_string(),
                "flag" => t.flag = v.to_string(),
                "flags" => t.requires = v.split(',').map(|f| f.trim().to_string()).collect(),
                "lsr" => t.lsr_min = v.parse().ok(),
                "lsrmax" => t.lsr_max = v.parse().ok(),
                _ => {}
            }
        }
        out.insert(crate::html::page_name(&section.name), t);
    }
    out
}

/// SIDNEY while it is up.
pub struct Sidney {
    pub screen: Screen,
    pub files_open: bool,
    strings: Strings,
    emails: BTreeMap<String, Email>,
    adding: Adding,
    /// "Input complete" and what was added, until clicked away.
    notice: Option<(String, String, String)>,
    search: Search,
    terms: HashMap<String, String>,
    triggers: HashMap<String, Trigger>,
    clock: f64,
    new_mail_sound: bool,
    hits: Vec<(Hit, [f32; 4])>,
    /// Steps under way, what they wait on, and what they show.
    popup: Option<steps::Popup>,
    queue: VecDeque<steps::Step>,
    waiting: Option<steps::Waiting>,
    videos: [Option<steps::Video>; 3],
    analyze: analyze::Analyze,
    translate: translate::Translate,
    makeid: makeid::MakeId,
    suspects: suspects::Suspects,
    /// The menu dropped down, and the tool the file list is choosing for.
    menu_open: Option<usize>,
    files_for: Option<Tool>,
    choice: Option<Choice>,
    anagram: Option<anagram::Anagram>,
    /// The pointer as last drawn, in SIDNEY's pixels.
    pointer: (f32, f32),
}

/// Where Match Analysis shows a print, in SIDNEY's pixels.
const MATCH_PRINT: (f32, f32) = (
    (640.0 - 267.0) / 2.0 + 22.0,
    (480.0 - 156.0) / 2.0 + 15.0 + 36.0,
);

/// Where a search's page is shown, in SIDNEY's pixels; G-Engine's places.
const PAGE: [f32; 4] = [60.0, 136.0, 520.0, 254.0];

/// Day and hour, comparable.
fn when(timeblock: &str) -> (u32, u32) {
    parse_timeblock(timeblock).unwrap_or((0, 0))
}

/// E-mails that have arrived by now, as G-Engine's `CheckNewEmail`.
pub fn receive_email(state: &mut GameState) -> bool {
    let now = when(&state.timeblock);
    let mut arrived = vec!["EMAIL1", "EMAIL2", "EMAIL3"];
    if now >= (3, 12) {
        arrived.push("EMAIL4");
    }
    if now >= (3, 18)
        && state
            .score_events
            .contains("E_SIDNEY_ANALYSIS_SYMBOLS_FROM_SERRES")
    {
        arrived.push("EMAIL5");
    }
    if state.flags.contains("EGG") {
        arrived.push("EMAIL6");
    }
    for id in arrived {
        if !state.emails_received.iter().any(|e| e == id) {
            state.emails_received.push(id.to_string());
        }
    }
    state.emails_received.len() != state.emails_read.len()
}

impl Sidney {
    pub fn new(lib: &Library) -> Self {
        Self {
            screen: Screen::Desktop,
            files_open: false,
            strings: Strings::load(lib),
            emails: load_emails(lib),
            adding: Adding::Idle,
            notice: None,
            search: Search::default(),
            terms: load_terms(lib),
            triggers: load_triggers(lib),
            clock: 0.0,
            new_mail_sound: true,
            hits: Vec::new(),
            popup: None,
            queue: steps::queue(),
            waiting: None,
            videos: Default::default(),
            analyze: analyze::Analyze::default(),
            translate: translate::Translate::default(),
            makeid: makeid::MakeId::default(),
            suspects: suspects::Suspects::default(),
            menu_open: None,
            files_for: None,
            choice: None,
            anagram: None,
            pointer: (0.0, 0.0),
        }
    }

    /// What the pointer is on, in window pixels.
    pub fn hit(&self, space: Space, pointer: (f32, f32)) -> Option<Hit> {
        // Later things are drawn on top, so look from the end.
        self.hits
            .iter()
            .rev()
            .find(|(_, r)| inside(space.rect(*r), pointer))
            .map(|(h, _)| h.clone())
    }

    /// Time passing. `scan_result` is called once the inventory closes
    /// during Add Data, and gives `SidScanner` (the file number plus one,
    /// zero for none); `busy` is whether anything is playing.
    pub fn tick(
        &mut self,
        dt: f64,
        lib: &Library,
        state: &mut GameState,
        busy: bool,
    ) -> Vec<Event> {
        self.clock += dt;
        self.play_videos();
        if self.waiting == Some(steps::Waiting::Talk) && !busy {
            self.waiting = None;
        }
        self.map_tick(state);
        let mut events = Vec::new();
        if let Some(a) = &mut self.anagram {
            let out = a.tick(dt as f32);
            events.extend(self.anagram_out(out));
        }
        events.extend(self.pump(lib, state));
        events.extend(self.suspects.tick(self.clock, state));
        if self.new_mail_sound && state.emails_received.len() != state.emails_read.len() {
            events.push(Event::Sound("NEWEMAIL"));
            self.new_mail_sound = false;
        }
        match self.adding.clone() {
            Adding::Waiting { until } if self.clock >= until => {
                self.adding = Adding::Scanning;
                events.extend([
                    Event::ShowInventory,
                    Event::SetFlag("UsingScanner"),
                    Event::Variable("SidScanner", 0),
                ]);
            }
            Adding::Scanning if !state.inventory_shown && !busy => {
                events.push(Event::ClearFlag("UsingScanner"));
                let chosen = *state.variables.get("SIDSCANNER").unwrap_or(&0) - 1;
                match u32::try_from(chosen).ok().filter(|&id| file(id).is_some()) {
                    None => {
                        self.adding = Adding::Box {
                            text: self.strings.get("AddData Screen", "AbortInput"),
                            until: self.clock + 2.0,
                            blink: false,
                            then: None,
                        };
                    }
                    Some(id) if state.sidney_files.contains(&id) => {
                        self.adding = Adding::Idle;
                        let line = if state.ego == "GABRIEL" {
                            "02O4G2K6R1"
                        } else {
                            "02O4G716R1"
                        };
                        events.push(Event::Say(line.into(), 1));
                    }
                    Some(id) => {
                        events.push(Event::Sound("SIDSCAN"));
                        self.adding = Adding::Box {
                            text: self.strings.get("AddData Screen", "ScanningItem"),
                            until: self.clock + 2.0,
                            blink: true,
                            then: Some(id),
                        };
                    }
                }
            }
            Adding::Box { until, then, .. } if self.clock >= until => {
                self.adding = Adding::Idle;
                if let Some(id) = then {
                    events.push(Event::AddFile(id));
                    self.notice = Some((
                        self.strings.get("AddData Screen", "InputComplete"),
                        self.strings.get("AddData Screen", "InputPrompt"),
                        self.strings.file_name(id),
                    ));
                }
            }
            _ => {}
        }
        events
    }

    /// A click, on what [`Sidney::hit`] found.
    pub fn click(&mut self, hit: Hit, lib: &Library, state: &mut GameState) -> Vec<Event> {
        let mut events = self.click_inner(hit, state);
        events.extend(self.pump(lib, state));
        events
    }

    fn click_inner(&mut self, hit: Hit, state: &mut GameState) -> Vec<Event> {
        let gabriel = state.ego == "GABRIEL";
        let mut events = Vec::new();
        // A popup waiting on an answer takes the clicks; one without
        // buttons lets them past.
        if let Some(p) = &self.popup
            && (!p.buttons.is_empty() || p.input.is_some())
        {
            if let Hit::PopupButton(i) = hit {
                self.answer(i, &mut events, state);
            }
            return events;
        }
        if !self.queue.is_empty() || self.waiting.is_some() {
            return events;
        }
        if let Some(a) = &mut self.anagram {
            match hit {
                Hit::Anagram(h) => {
                    let out = a.click(h);
                    events.extend(self.anagram_out(out));
                }
                // Home and the menus wait while it works.
                Hit::MainMenu | Hit::Exit if !a.busy() => {
                    let out = a.click(anagram::AnagramHit::Exit);
                    events.extend(self.anagram_out(out));
                    self.anagram = None;
                    events.push(Event::LoopSound(None));
                    events.extend(self.click_inner(hit, state));
                }
                _ => {}
            }
            return events;
        }
        if let Some(choice) = &self.choice {
            match hit {
                Hit::Choice(i) => {
                    let steps = choice.rows.get(i).map(|r| r.1.clone()).unwrap_or_default();
                    self.choice = None;
                    self.then(steps);
                }
                Hit::CloseFiles => self.choice = None,
                _ => {}
            }
            return events;
        }
        if self.notice.is_some() {
            if hit == Hit::PopupOk {
                self.notice = None;
            }
            return events;
        }
        if self.adding != Adding::Idle {
            return events;
        }
        match hit {
            Hit::Exit => events.push(Event::Exit),
            Hit::MainMenu => {
                self.choice = None;
                self.screen = Screen::Desktop;
                self.files_open = false;
                self.files_for = None;
                self.menu_open = None;
                self.popup = None;
                events.push(Event::Sound("SIDEXIT"));
            }
            Hit::PopupButton(_) => self.popup = None,
            Hit::MenuHeader(i) => {
                self.menu_open = if self.menu_open == Some(i) {
                    None
                } else {
                    Some(i)
                };
            }
            Hit::MenuItem(m, i) if self.screen == Screen::MakeId => {
                self.menu_open = None;
                self.makeid.job = (m, i);
            }
            Hit::MenuItem(m, i) if self.screen == Screen::Suspects => {
                self.menu_open = None;
                match (m, i) {
                    (0, _) => self.choose_for(Tool::Suspects),
                    (1, i) => events.extend(self.suspects.open_suspect(i, state)),
                    (2, 0) => match self.suspects.link(state) {
                        Ok(e) => events.extend(e),
                        Err(key) => self.suspects_note(key),
                    },
                    (2, 1) => match self.suspects.unlink(state) {
                        Ok(e) => events.extend(e),
                        Err(key) => self.suspects_note(key),
                    },
                    (2, 3) => match self.suspects.start_match(state) {
                        Ok(movie) => self.then([
                            steps::Step::Do(Event::Sound("WORKING3")),
                            steps::Step::Wait(1.0),
                            steps::Step::Video {
                                name: movie,
                                slot: 0,
                                at: MATCH_PRINT,
                                then: None,
                            },
                            steps::Step::Clear(0),
                            steps::Step::Tool(steps::ToolStep::Scanned),
                        ]),
                        Err(e) => events.extend(e),
                    },
                    _ => {}
                }
            }
            Hit::Evidence(i) => {
                if let Some(s) = self.suspects.open {
                    self.suspects.picked[s] = Some(i);
                }
            }
            Hit::MatchLink => {
                if let Some(suspects::Matching::Found(who)) =
                    self.suspects.matching.as_ref().map(|m| m.phase.clone())
                {
                    self.suspects.open = Some(who);
                    match self.suspects.link(state) {
                        Ok(e) => events.extend(e),
                        Err(key) => self.suspects_note(key),
                    }
                }
                self.suspects.matching = None;
            }
            Hit::MatchClose => self.suspects.matching = None,
            Hit::Person(who) => self.makeid.person = who,
            Hit::Print => events.extend(self.makeid.print(state)),
            Hit::MenuItem(m, i) => {
                self.menu_open = None;
                let item = self
                    .analyze
                    .menus()
                    .get(m)
                    .and_then(|(_, _, items)| items.get(i))
                    .and_then(|e| e.2);
                if let Some(item) = item {
                    events.extend(self.analyze_item(item, state));
                }
            }
            Hit::AnalyzeGo => events.extend(self.analyze_go(state)),
            Hit::Choice(_) | Hit::Anagram(_) => {}
            Hit::TranslateGo => events.extend(self.translate_go(state)),
            Hit::TranslateOpen => {
                self.files_for = Some(Tool::Translate);
                self.files_open = true;
            }
            Hit::Language(from, i) => {
                let language = translate::LANGUAGES[i];
                if from {
                    self.translate.from = language;
                } else {
                    self.translate.to = language;
                }
            }
            Hit::CloseFiles => {
                self.files_open = false;
                self.files_for = None;
            }
            Hit::File(id) => {
                self.files_open = false;
                // Chosen for a tool, or from the desktop: then each kind
                // opens in its own.
                let tool = match self.files_for.take() {
                    Some(tool) => tool,
                    None => match file(id).map(|f| f.kind) {
                        Some(Kind::Image) => Tool::Analyze,
                        Some(Kind::Text | Kind::Audio) => Tool::Translate,
                        Some(Kind::Fingerprint | Kind::License) => Tool::Suspects,
                        _ => return events,
                    },
                };
                match tool {
                    Tool::Analyze => {
                        self.screen = Screen::Analyze;
                        self.analyze_open(id, state);
                    }
                    Tool::Translate => {
                        self.screen = Screen::Translate;
                        self.translate_open(id, state);
                    }
                    Tool::Suspects => {
                        self.screen = Screen::Suspects;
                        self.suspects.show(state);
                        self.suspects.file = Some(id);
                    }
                    other => self.screen = Screen::Unbuilt(other),
                }
                events.push(Event::Sound("SIDENTER"));
            }
            Hit::Tool(tool) => {
                events.push(Event::Sound("SIDENTER"));
                // Gabriel cannot use some of Grace's tools; he says so.
                let refusal = match tool {
                    Tool::Search => "02O6I2ZQR1",
                    Tool::Email => "02O1E2ZQR1",
                    Tool::Analyze => "02O7A2ZQR1",
                    _ => "",
                };
                if gabriel && !refusal.is_empty() {
                    events.push(Event::Say(refusal.into(), 1));
                    return events;
                }
                self.files_open = false;
                match tool {
                    Tool::Files => {
                        self.files_open = true;
                        if !gabriel && !state.flags.contains("PLAYEDEMPTYFILELISTDIALOG") {
                            if state.sidney_files.is_empty() {
                                events.push(Event::Say("026Y22Z291".into(), 1));
                            }
                            events.push(Event::SetFlag("PlayedEmptyFilelistDialog"));
                        }
                    }
                    Tool::Email => self.screen = Screen::EmailList,
                    Tool::Search => self.screen = Screen::Search,
                    Tool::Analyze => self.screen = Screen::Analyze,
                    Tool::Translate => self.screen = Screen::Translate,
                    Tool::MakeId => self.screen = Screen::MakeId,
                    Tool::Suspects => {
                        self.screen = Screen::Suspects;
                        self.suspects.show(state);
                    }
                    Tool::AddData => {
                        if state.timeblock == "207A" {
                            events.push(Event::Say("0264G2ZPF1".into(), 2));
                        } else {
                            self.adding = Adding::Waiting {
                                until: self.clock + 2.0,
                            };
                        }
                    }
                }
            }
            Hit::EmailItem(i) => self.view_email(i, state, &mut events),
            Hit::EmailNext => {
                if let Screen::Email(i) = self.screen {
                    self.view_email(i + 1, state, &mut events);
                }
            }
            Hit::EmailPrev => {
                if let Screen::Email(i) = self.screen
                    && i > 0
                {
                    self.view_email(i - 1, state, &mut events);
                }
            }
            Hit::EmailReply => {
                let id = match self.screen {
                    Screen::Email(i) => state.emails_received.get(i).cloned().unwrap_or_default(),
                    _ => String::new(),
                };
                let line = match id.as_str() {
                    "EMAIL1" => "02O1V0OPF1",
                    "EMAIL2" => "02O240OPF1",
                    _ => "02O570OPF1",
                };
                events.push(Event::Say(line.into(), 1));
            }
            Hit::EmailCompose => events.push(Event::Say("02OXI2ZZ51".into(), 1)),
            Hit::EmailPrint => events.push(Event::Say("02OXJ2ZZ51".into(), 1)),
            Hit::EmailClose => self.screen = Screen::EmailList,
            Hit::PopupOk => {}
            Hit::SearchGo => {
                let term = self.search.input.trim().to_lowercase();
                match self.terms.get(&term).cloned() {
                    Some(page) => self.visit(page, true, state, &mut events),
                    None => {
                        let text = self.strings.get("Search Screen", "NotFound");
                        self.notice = Some((String::new(), text, String::new()));
                    }
                }
            }
            Hit::SearchReset => {
                self.search.input.clear();
                self.search.page = None;
            }
            Hit::SearchBack => {
                if self.search.at > 0 {
                    self.search.at -= 1;
                    let page = self.search.history[self.search.at].clone();
                    self.visit(page, false, state, &mut events);
                }
            }
            Hit::SearchForward => {
                if self.search.at + 1 < self.search.history.len() {
                    self.search.at += 1;
                    let page = self.search.history[self.search.at].clone();
                    self.visit(page, false, state, &mut events);
                }
            }
            Hit::Link(page) => self.visit(page, true, state, &mut events),
        }
        events
    }

    /// Shows a page, and does what visiting it does.
    fn visit(&mut self, page: String, remember: bool, state: &GameState, events: &mut Vec<Event>) {
        if remember {
            self.search
                .history
                .truncate(self.search.at + usize::from(!self.search.history.is_empty()));
            self.search.history.push(page.clone());
            self.search.at = self.search.history.len() - 1;
        }
        self.search.page = Some(page.clone());
        self.search.scroll = 0.0;
        if let Some(t) = self.triggers.get(&page) {
            let key = |f: &str| gk3_game::state::key(f);
            let step = lsr_step(state);
            let ok = !t.line.is_empty()
                && !state.flags.contains(&key(&t.flag))
                && t.lsr_min.is_none_or(|m| step >= m)
                && t.lsr_max.is_none_or(|m| step <= m)
                && t.requires.iter().all(|f| match f.strip_prefix('!') {
                    Some(not) => !state.flags.contains(&key(not)),
                    None => state.flags.contains(&key(f)),
                });
            if ok {
                events.push(Event::Say(t.line.clone(), 1));
                events.push(Event::SetFlagOwned(t.flag.clone()));
            }
        }
        if let Some((_, score)) = PAGE_SCORES.iter().find(|(p, _)| *p == page) {
            events.push(Event::Score(score.to_string()));
        }
        if let Some((_, flag)) = PAGE_FLAGS.iter().find(|(p, _)| *p == page) {
            events.push(Event::SetFlag(flag));
        }
    }

    /// Carries out what the anagram parser asks.
    fn anagram_out(&mut self, out: Vec<anagram::Out>) -> Vec<Event> {
        let mut events = Vec::new();
        for o in out {
            match o {
                anagram::Out::ScrambleSound(on) => {
                    events.push(Event::LoopSound(on.then_some("SIDANAGRAMSCRAMBLE")))
                }
                anagram::Out::Say(plate, n) => events.push(Event::Say(plate.into(), n)),
                anagram::Out::Score(score) => events.push(Event::Score(score.into())),
                anagram::Out::SetFlag(flag) => events.extend([Event::SetFlag(flag), Event::LsrState]),
                anagram::Out::Exit => {
                    self.anagram = None;
                    events.push(Event::LoopSound(None));
                }
            }
        }
        events
    }

    /// Whether steps are still playing (not waiting on the player).
    pub fn busy(&self) -> bool {
        matches!(
            self.waiting,
            Some(steps::Waiting::Until(_) | steps::Waiting::Video(_))
        ) || (!self.queue.is_empty() && self.waiting != Some(steps::Waiting::Popup))
            || self.anagram.as_ref().is_some_and(|a| a.busy())
    }

    /// The file list will choose for this tool.
    pub fn choose_for(&mut self, tool: Tool) {
        self.files_for = Some(tool);
        self.files_open = true;
    }

    /// Whether typing goes to SIDNEY (the search box).
    pub fn wants_text(&self) -> bool {
        (self.screen == Screen::Search && self.notice.is_none() && self.popup.is_none())
            || self.popup_typing()
    }

    pub fn type_text(&mut self, text: &str) {
        if self.popup_typing() {
            self.popup_type(text);
            return;
        }
        for c in text.chars().filter(|c| !c.is_control()) {
            if self.search.input.chars().count() < 40 {
                self.search.input.push(c);
            }
        }
    }

    pub fn backspace(&mut self) {
        if self.popup_typing() {
            self.popup_backspace();
            return;
        }
        self.search.input.pop();
    }

    /// Scrolls the page by a number of pixels.
    pub fn scroll(&mut self, dy: f32) {
        let height = self.search.laid_out.as_ref().map_or(0.0, |l| l.2);
        self.search.scroll =
            (self.search.scroll + dy).clamp(0.0, (height - PAGE[3] + 20.0).max(0.0));
    }

    /// Lays the current page out, if it is not already.
    fn lay_out(&mut self, ui: &mut Ui, lib: &Library, renderer: &mut Renderer) {
        let Some(page) = &self.search.page else {
            return;
        };
        if self.search.laid_out.as_ref().is_some_and(|l| &l.0 == page) {
            return;
        }
        let bytes = lib
            .read(page)
            .or_else(|_| lib.read(&page.replace(".HTML", ".HTM")))
            .unwrap_or_default();
        let root = crate::html::parse(&bytes);
        struct M<'a> {
            ui: &'a mut Ui,
            lib: &'a Library,
            renderer: &'a mut Renderer,
        }
        impl crate::html::Measure for M<'_> {
            fn text(&mut self, font: &str, text: &str) -> (f32, f32) {
                self.ui.font_size(self.lib, self.renderer, font, text)
            }
            fn image(&mut self, name: &str) -> Option<(f32, f32)> {
                self.ui.bitmap_size(self.lib, self.renderer, name)
            }
        }
        let (pieces, height) =
            crate::html::layout(&root, PAGE[2] - 16.0, &mut M { ui, lib, renderer });
        self.search.laid_out = Some((page.clone(), pieces, height));
    }

    fn view_email(&mut self, i: usize, state: &mut GameState, events: &mut Vec<Event>) {
        let Some(id) = state.emails_received.get(i).cloned() else {
            return;
        };
        self.screen = Screen::Email(i);
        state.emails_read.insert(id.clone());
        let (score, flag) = match id.as_str() {
            "EMAIL4" => ("e_sidney_email_open_temple_diagram", "OpenedTempleDiagram"),
            "EMAIL5" => (
                "e_sidney_email_open_symbols_from_serres",
                "OpenedHermeticDiagram",
            ),
            _ => return,
        };
        events.push(Event::Score(score.into()));
        if id == "EMAIL5" && !state.flags.contains("OPENEDHERMETICDIAGRAM") {
            events.push(Event::Say("02DG4583L2".into(), 2));
        }
        events.push(Event::SetFlag(flag));
    }

    /// Draws SIDNEY over the whole window, noting what can be clicked.
    pub fn draw(
        &mut self,
        (ui, lib, renderer): (&mut Ui, &Library, &mut Renderer),
        overlay: &mut Overlay,
        state: &GameState,
        pointer: (f32, f32),
        size: (u32, u32),
    ) {
        let space = Space::of(size);
        if self.screen == Screen::Search {
            self.lay_out(ui, lib, renderer);
        }
        // The pointer in SIDNEY's own pixels.
        let at = (
            (pointer.0 - space.ox) / space.fit,
            (pointer.1 - space.oy) / space.fit,
        );
        self.pointer = at;
        let mut hits = Vec::new();
        overlay.rect([0.0, 0.0, size.0 as f32, size.1 as f32], [0, 0, 0, 255]);
        let s = &self.strings;
        match &self.screen {
            Screen::Desktop => {
                ui.sprite(
                    lib,
                    renderer,
                    overlay,
                    space,
                    "S_MAIN_SCN",
                    (0.0, 0.0),
                    [255; 4],
                );
                for (i, (tool, id)) in TOOLS.iter().enumerate() {
                    let x = 5.0 + 79.0 * i as f32;
                    let r = [x, 24.0, 76.0, 13.0];
                    let state = if inside(r, at) { "H" } else { "U" };
                    ui.sprite(
                        lib,
                        renderer,
                        overlay,
                        space,
                        &format!("B_{id}_{state}"),
                        (x, 24.0),
                        [255; 4],
                    );
                    hits.push((Hit::Tool(*tool), r));
                }
                let exit = [640.0 - 10.0 - 80.0, 480.0 - 10.0 - 20.0, 80.0, 20.0];
                ui.sid_button(
                    lib,
                    renderer,
                    overlay,
                    space,
                    exit,
                    &s.get("Main Screen", "MenuItem9"),
                    ("SID_TEXT_18", true, at),
                );
                hits.push((Hit::Exit, exit));
                // New mail blinks at the bottom left.
                if state.emails_received.len() != state.emails_read.len()
                    && ((self.clock * 2.0) as u64).is_multiple_of(2)
                {
                    let text = s.get("EMail Screen", "NewEMail");
                    ui.write(
                        lib,
                        renderer,
                        overlay,
                        space,
                        "SID_TEXT_18",
                        &text,
                        (12.0, 452.0),
                        (None, Align::Left),
                    );
                }
            }
            screen => {
                ui.sprite(
                    lib,
                    renderer,
                    overlay,
                    space,
                    "S_BKGND",
                    (0.0, 0.0),
                    [255; 4],
                );
                let (title_screen, title_key) = match screen {
                    Screen::EmailList | Screen::Email(_) => ("EMail Screen", "ScreenName"),
                    Screen::Unbuilt(Tool::Analyze) => ("Analyze Screen", "ScreenName"),
                    Screen::Unbuilt(Tool::Translate) => ("Translate Screen", "ScreenName"),
                    Screen::Search => ("Search Screen", "ScreenName"),
                    Screen::Analyze => ("Analyze Screen", "ScreenName"),
                    Screen::Translate => ("Translate Screen", "ScreenName"),
                    Screen::MakeId => ("MakeID Screen", "ScreenName"),
                    Screen::Suspects => ("Suspects Screen", "ScreenName"),
                    Screen::Unbuilt(Tool::MakeId) => ("MakeID Screen", "ScreenName"),
                    Screen::Unbuilt(Tool::Suspects) => ("Suspects Screen", "ScreenName"),
                    _ => ("Main Screen", "MenuName"),
                };
                let title = s.get(title_screen, title_key);
                ui.write(
                    lib,
                    renderer,
                    overlay,
                    space,
                    "SID_TEXT_18",
                    &title,
                    (10.0, 6.0),
                    (None, Align::Left),
                );
                let home = [640.0 - 10.0 - 96.0, 480.0 - 10.0 - 20.0, 96.0, 20.0];
                ui.sid_button(
                    lib,
                    renderer,
                    overlay,
                    space,
                    home,
                    &s.get("Main Screen", "HomeButton"),
                    ("SID_TEXT_18", true, at),
                );
                hits.push((Hit::MainMenu, home));
                match screen {
                    Screen::EmailList => {
                        self.draw_email_list((ui, lib, renderer), overlay, space, state, &mut hits)
                    }
                    Screen::Search => {
                        self.draw_search((ui, lib, renderer), overlay, space, at, &mut hits)
                    }
                    Screen::Email(i) => self.draw_email(
                        (ui, lib, renderer),
                        overlay,
                        space,
                        state,
                        *i,
                        at,
                        &mut hits,
                    ),
                    Screen::Analyze => {
                        self.draw_analyze((ui, lib, renderer), overlay, space, state, at, &mut hits)
                    }
                    Screen::Translate => {
                        self.draw_translate((ui, lib, renderer), overlay, space, at, &mut hits)
                    }
                    Screen::MakeId => {
                        self.draw_makeid((ui, lib, renderer), overlay, space, at, &mut hits)
                    }
                    Screen::Suspects => self.draw_suspects(
                        (ui, lib, renderer),
                        overlay,
                        space,
                        state,
                        at,
                        &mut hits,
                    ),
                    _ => {
                        let note = "This part of SIDNEY is not built yet.";
                        ui.sid_box(
                            lib,
                            renderer,
                            overlay,
                            space,
                            [170.0, 200.0, 300.0, 40.0],
                            false,
                            [0, 0, 0, 180],
                        );
                        ui.write(
                            lib,
                            renderer,
                            overlay,
                            space,
                            "SID_TEXT_14",
                            note,
                            (170.0, 212.0),
                            (Some(300.0), Align::Center),
                        );
                    }
                }
            }
        }
        if matches!(self.screen, Screen::Analyze | Screen::Suspects) {
            self.draw_videos((ui, lib, renderer), overlay, space);
        }
        if matches!(
            self.screen,
            Screen::Analyze | Screen::MakeId | Screen::Suspects
        ) {
            self.draw_menu_bar((ui, lib, renderer), overlay, space, state, at, &mut hits);
        }
        if self.screen == Screen::Analyze
            && let Some(a) = &self.anagram
        {
            let drawn = a.draw((ui, lib, renderer), overlay, space, at);
            hits.extend(drawn.into_iter().map(|(h, r)| (Hit::Anagram(h), r)));
        }
        if self.files_open {
            self.draw_files((ui, lib, renderer), overlay, space, state, at, &mut hits);
        }
        lsr_map::draw_choice(self, (ui, lib, renderer), overlay, space, at, &mut hits);
        self.draw_popup((ui, lib, renderer), overlay, space, at, &mut hits);
        self.draw_adding((ui, lib, renderer), overlay, space);
        if let Some((title, prompt, text)) = &self.notice {
            let r = [170.0, 180.0, 300.0, 110.0];
            ui.sid_box(lib, renderer, overlay, space, r, false, [0, 0, 0, 220]);
            ui.write(
                lib,
                renderer,
                overlay,
                space,
                "SID_TEXT_18",
                title,
                (r[0], r[1] + 8.0),
                (Some(r[2]), Align::Center),
            );
            ui.write(
                lib,
                renderer,
                overlay,
                space,
                "SID_TEXT_14",
                prompt,
                (r[0] + 12.0, r[1] + 36.0),
                (Some(r[2] - 24.0), Align::Left),
            );
            ui.write(
                lib,
                renderer,
                overlay,
                space,
                "SID_TEXT_14_GRN",
                text,
                (r[0] + 12.0, r[1] + 54.0),
                (Some(r[2] - 24.0), Align::Left),
            );
            let ok = [r[0] + (r[2] - 60.0) / 2.0, r[1] + r[3] - 26.0, 60.0, 18.0];
            ui.sid_button(
                lib,
                renderer,
                overlay,
                space,
                ok,
                "OK",
                ("SID_TEXT_14", true, at),
            );
            hits.push((Hit::PopupOk, ok));
        }
        self.hits = hits;
    }

    fn draw_files(
        &self,
        (ui, lib, renderer): (&mut Ui, &Library, &mut Renderer),
        overlay: &mut Overlay,
        space: Space,
        state: &GameState,
        at: (f32, f32),
        hits: &mut Vec<(Hit, [f32; 4])>,
    ) {
        let s = &self.strings;
        let r = [40.0, 66.0, 154.0, 350.0];
        ui.sid_box(lib, renderer, overlay, space, r, false, [0, 0, 0, 200]);
        ui.write(
            lib,
            renderer,
            overlay,
            space,
            "SID_TEXT_18",
            &s.get("Analyze Screen", "FileList"),
            (r[0], r[1] + 2.0),
            (Some(r[2]), Align::Center),
        );
        overlay.rect(
            space.rect([r[0] + 1.0, r[1] + 20.0, r[2] - 2.0, 1.0]),
            [120, 120, 120, 255],
        );
        let close = [r[0] + 2.0, r[1] + 2.0, 16.0, 16.0];
        let up = if inside(close, at) {
            "CLOSEWIN_HOVER"
        } else {
            "CLOSEWIN_UP"
        };
        ui.sprite(
            lib,
            renderer,
            overlay,
            space,
            up,
            (close[0], close[1]),
            [255; 4],
        );
        hits.push((Hit::CloseFiles, close));
        let mut y = r[1] + 26.0;
        for (kind, key) in FOLDERS {
            ui.write(
                lib,
                renderer,
                overlay,
                space,
                "SID_TEXT_14_UL",
                &format!("-{}", s.get("Main Screen", key)),
                (r[0] + 6.0, y),
                (None, Align::Left),
            );
            y += 16.0;
            for f in FILES
                .iter()
                .filter(|f| f.kind == *kind && state.sidney_files.contains(&f.id))
            {
                let row = [r[0] + 6.0, y, r[2] - 12.0, 16.0];
                let font = if inside(row, at) {
                    "SID_TEXT_14_GRN"
                } else {
                    "SID_TEXT_14"
                };
                ui.write(
                    lib,
                    renderer,
                    overlay,
                    space,
                    font,
                    &format!("  {}", s.file_name(f.id)),
                    (row[0], y),
                    (None, Align::Left),
                );
                hits.push((Hit::File(f.id), row));
                y += 16.0;
            }
        }
    }

    fn draw_search(
        &self,
        (ui, lib, renderer): (&mut Ui, &Library, &mut Renderer),
        overlay: &mut Overlay,
        space: Space,
        at: (f32, f32),
        hits: &mut Vec<(Hit, [f32; 4])>,
    ) {
        let s = &self.strings;
        ui.sid_box(
            lib,
            renderer,
            overlay,
            space,
            [60.0, 88.0, 520.0, 48.0],
            false,
            [0, 0, 0, 128],
        );
        ui.sid_box(
            lib,
            renderer,
            overlay,
            space,
            [60.0, 390.0, 520.0, 28.0],
            false,
            [0, 0, 0, 128],
        );
        let input = [160.0, 105.0, 320.0, 15.0];
        ui.sid_box(lib, renderer, overlay, space, input, false, [0, 0, 0, 255]);
        let caret = if ((self.clock * 2.0) as u64).is_multiple_of(2) {
            "_"
        } else {
            ""
        };
        ui.write(
            lib,
            renderer,
            overlay,
            space,
            "SID_TEXT_14",
            &format!("{}{caret}", self.search.input),
            (input[0] + 4.0, input[1]),
            (None, Align::Left),
        );
        let buttons = [
            (Hit::SearchReset, "Reset", true, [74.0, 105.0, 80.0, 15.0]),
            (Hit::SearchGo, "Search", true, [486.0, 105.0, 80.0, 15.0]),
            (
                Hit::SearchBack,
                "Back",
                self.search.at > 0 && !self.search.history.is_empty(),
                [123.0, 398.0, 80.0, 13.0],
            ),
            (
                Hit::SearchForward,
                "Forward",
                self.search.at + 1 < self.search.history.len(),
                [215.0, 398.0, 80.0, 13.0],
            ),
        ];
        for (hit, key, enabled, b) in buttons {
            ui.sid_button(
                lib,
                renderer,
                overlay,
                space,
                b,
                &s.get("Search Screen", key),
                ("SID_PDN_10_L", enabled, at),
            );
            if enabled {
                hits.push((hit, b));
            }
        }
        let Some((_, pieces, _)) = &self.search.laid_out else {
            return;
        };
        if self.search.page.is_none() {
            return;
        }
        // The page: dark glass, its contents scrolled within it.
        ui.sid_box(lib, renderer, overlay, space, PAGE, false, [0, 0, 0, 128]);
        let (px, py) = (PAGE[0] + 8.0, PAGE[1] + 6.0);
        let visible = |y: f32, h: f32| y >= 0.0 && y + h <= PAGE[3] - 10.0;
        for piece in pieces {
            match piece {
                crate::html::Piece::Text {
                    font,
                    text,
                    x,
                    y,
                    link,
                } => {
                    let y = y - self.search.scroll;
                    let (w, h) = ui.font_size(lib, renderer, font, text);
                    if !visible(y, h) {
                        continue;
                    }
                    ui.write(
                        lib,
                        renderer,
                        overlay,
                        space,
                        font,
                        text,
                        (px + x, py + y),
                        (None, Align::Left),
                    );
                    if let Some(link) = link {
                        hits.push((Hit::Link(link.clone()), [px + x, py + y, w, h]));
                    }
                }
                crate::html::Piece::Image { name, x, y } => {
                    let y = y - self.search.scroll;
                    if let Some((_, h)) = ui.bitmap_size(lib, renderer, name)
                        && visible(y, h)
                    {
                        ui.sprite(
                            lib,
                            renderer,
                            overlay,
                            space,
                            name,
                            (px + x, py + y),
                            [255; 4],
                        );
                    }
                }
                crate::html::Piece::Bullet { x, y } => {
                    let y = y - self.search.scroll;
                    if visible(y, 6.0) {
                        ui.sprite(
                            lib,
                            renderer,
                            overlay,
                            space,
                            "SIDNEYBULLET",
                            (px + x, py + y),
                            [255; 4],
                        );
                    }
                }
                crate::html::Piece::Rule { y } => {
                    let y = y - self.search.scroll;
                    if visible(y, 2.0) {
                        overlay.rect(
                            space.rect([px, py + y, PAGE[2] - 16.0, 2.0]),
                            [128, 128, 128, 255],
                        );
                    }
                }
            }
        }
    }

    /// Analyze: the picture or text being analysed, or the file waiting
    /// for analysis, and START ANALYSIS.
    #[allow(clippy::too_many_arguments)]
    fn draw_analyze(
        &self,
        (ui, lib, renderer): (&mut Ui, &Library, &mut Renderer),
        overlay: &mut Overlay,
        space: Space,
        state: &GameState,
        at: (f32, f32),
        hits: &mut Vec<(Hit, [f32; 4])>,
    ) {
        let s = &self.strings;
        let a = &self.analyze;
        if let Some((name, (x, y))) = a.picture() {
            ui.sprite(lib, renderer, overlay, space, name, (x, y), [255; 4]);
        }
        let id = a.file;
        match a.state {
            analyze::State::PreAnalyze | analyze::State::Audio => {
                if let Some(id) = id {
                    let r = [40.0, 108.0, 153.0, 167.0];
                    ui.sid_box(lib, renderer, overlay, space, r, false, [0, 0, 0, 128]);
                    overlay.rect(
                        space.rect([r[0] + 1.0, r[1] + 17.0, r[2] - 2.0, 1.0]),
                        [120, 120, 120, 255],
                    );
                    ui.write(
                        lib,
                        renderer,
                        overlay,
                        space,
                        "SID_TEXT_14",
                        &s.file_name(id),
                        (r[0], r[1] + 2.0),
                        (Some(r[2] - 8.0), Align::Center),
                    );
                    let icon = file(id).map(|f| f.item).filter(|i| !i.is_empty());
                    let (tex, w, h) = icon
                        .and_then(|item| ui.item_picture(lib, renderer, item))
                        .or_else(|| ui.bitmap(lib, renderer, "SIDFILE_9"))
                        .unwrap_or_else(|| ui.white());
                    let (w, h) = (w as f32, h as f32);
                    let fit = (94.0 / w).min(94.0 / h);
                    let dest = [
                        r[0] + (r[2] - w * fit) / 2.0,
                        r[1] + 30.0 + (110.0 - h * fit) / 2.0,
                        w * fit,
                        h * fit,
                    ];
                    overlay.image(tex, [0.0, 0.0, w, h], space.rect(dest), [255; 4]);
                }
            }
            // The anagram parser covers it.
            analyze::State::Text if self.anagram.is_none() => {
                let r = [170.0, 188.0, 300.0, 104.0];
                ui.sid_box(lib, renderer, overlay, space, r, false, [0, 0, 0, 128]);
                if let Some(id) = id {
                    ui.write(
                        lib,
                        renderer,
                        overlay,
                        space,
                        "SID_TEXT_14",
                        &s.file_name(id),
                        (r[0], r[1] + 2.0),
                        (Some(r[2] - 8.0), Align::Center),
                    );
                }
                let key = if state.flags.contains("ARCADIACOMPLETE") {
                    "ArcadiaText"
                } else {
                    "ArcadiaText2"
                };
                ui.write(
                    lib,
                    renderer,
                    overlay,
                    space,
                    "SID_TEXT_14",
                    &s.get("Analyze Screen", key),
                    (r[0], r[1] + 50.0),
                    (Some(r[2]), Align::Center),
                );
            }
            analyze::State::Map => self.draw_map((ui, lib, renderer), overlay, space, state),
            _ => {}
        }
        let go = [
            640.0 - 10.0 - 96.0 - 10.0 - 140.0,
            480.0 - 10.0 - 20.0,
            140.0,
            20.0,
        ];
        let enabled = a.state != analyze::State::Empty;
        ui.sid_button(
            lib,
            renderer,
            overlay,
            space,
            go,
            &s.get("Analyze Screen", "MenuItem2"),
            ("SID_TEXT_18", enabled, at),
        );
        if enabled {
            hits.push((Hit::AnalyzeGo, go));
        }
    }

    /// Analyze's menu bar, with whichever menu is dropped down.
    #[allow(clippy::too_many_arguments)]
    fn draw_menu_bar(
        &self,
        (ui, lib, renderer): (&mut Ui, &Library, &mut Renderer),
        overlay: &mut Overlay,
        space: Space,
        state: &GameState,
        at: (f32, f32),
        hits: &mut Vec<(Hit, [f32; 4])>,
    ) {
        let s = &self.strings;
        let mut x = 24.0;
        let y = 27.0;
        let (section, menus) = match self.screen {
            Screen::MakeId => ("MakeID Screen", self.makeid.menus()),
            Screen::Suspects => ("Suspects Screen", self.suspects.menus(state)),
            _ => ("Analyze Screen", self.analyze.menus()),
        };
        for (m, (name, enabled, items)) in menus.into_iter().enumerate() {
            let label = s.get(section, &name);
            let font = if enabled {
                "SID_EMB_10"
            } else {
                "SID_NO_EMB_10"
            };
            let (w, _) = ui.font_size(lib, renderer, font, &label);
            let header = [x, y, w + 14.0, 13.0];
            ui.write(
                lib,
                renderer,
                overlay,
                space,
                font,
                &label,
                (x, y),
                (None, Align::Left),
            );
            let arrow = if enabled {
                "S_DWNARW"
            } else {
                "S_DWNARW_NOEMB"
            };
            ui.sprite(
                lib,
                renderer,
                overlay,
                space,
                arrow,
                (x + w + 4.0, y + 4.0),
                [255; 4],
            );
            if enabled {
                hits.push((Hit::MenuHeader(m), header));
            }
            if self.menu_open == Some(m) && enabled {
                let width = items
                    .iter()
                    .map(|(k, _, _)| {
                        ui.font_size(lib, renderer, "SID_PDN_10_L", &s.get(section, k))
                            .0
                    })
                    .fold(w + 14.0, f32::max)
                    + 12.0;
                let mut iy = y + 14.0;
                overlay.rect(
                    space.rect([x, iy, width, items.len() as f32 * 13.0 + 1.0]),
                    [66, 65, 66, 255],
                );
                for (i, (key, on, item)) in items.iter().enumerate() {
                    let row = [x, iy, width, 13.0];
                    match item {
                        None => overlay.rect(
                            space.rect([x + 2.0, iy + 6.0, width - 4.0, 1.0]),
                            [120, 120, 120, 255],
                        ),
                        Some(_) => {
                            let hover = *on && crate::ui::inside(row, at);
                            if hover {
                                overlay.rect(space.rect(row), [100, 90, 60, 255]);
                            }
                            let f = if *on { "SID_PDN_10_L" } else { "SID_PDN_10_UL" };
                            ui.write(
                                lib,
                                renderer,
                                overlay,
                                space,
                                f,
                                &s.get(section, key),
                                (x + 6.0, iy + 1.0),
                                (None, Align::Left),
                            );
                            if *on {
                                hits.push((Hit::MenuItem(m, i), row));
                            }
                        }
                    }
                    iy += 13.0;
                }
            }
            x += w + 14.0 + 26.0;
        }
    }

    /// A note from Suspects, by its strings key.
    fn suspects_note(&mut self, key: &str) {
        let note = steps::Popup::text(self.strings.get("Suspects Screen", key))
            .ok(self.strings.get("Analyze Screen", "OKButton"));
        self.then([steps::Step::Show(note)]);
    }

    /// Suspects: the open suspect's record and linked files, the file to
    /// link, and Match Analysis.
    #[allow(clippy::too_many_arguments)]
    fn draw_suspects(
        &self,
        (ui, lib, renderer): (&mut Ui, &Library, &mut Renderer),
        overlay: &mut Overlay,
        space: Space,
        state: &GameState,
        at: (f32, f32),
        hits: &mut Vec<(Hit, [f32; 4])>,
    ) {
        let s = &self.strings;
        let sus = &self.suspects;
        let section = "Suspects Screen";
        // The file to link, at the left.
        if let Some(id) = sus.file.filter(|&id| file(id).is_some()) {
            let r = [13.0, 92.0, 153.0, 167.0];
            ui.sid_box(lib, renderer, overlay, space, r, false, [0, 0, 0, 128]);
            overlay.rect(
                space.rect([r[0] + 1.0, r[1] + 17.0, r[2] - 2.0, 1.0]),
                [120, 120, 120, 255],
            );
            ui.write(
                lib,
                renderer,
                overlay,
                space,
                "SID_TEXT_14",
                &s.file_name(id),
                (r[0], r[1] + 2.0),
                (Some(r[2] - 8.0), Align::Center),
            );
            self.draw_icon(
                (ui, lib, renderer),
                overlay,
                space,
                id,
                [r[0] + 30.0, r[1] + 36.0, 94.0, 94.0],
            );
        }
        if let Some(i) = sus.open {
            let (x, y) = (181.0, 63.0);
            ui.sid_box(
                lib,
                renderer,
                overlay,
                space,
                [x, y, 432.0, 325.0],
                false,
                [0, 0, 0, 128],
            );
            ui.sid_box(
                lib,
                renderer,
                overlay,
                space,
                [x, y, 70.0, 77.0],
                false,
                [0, 0, 0, 0],
            );
            if suspects::print_linked(state, i) {
                ui.sprite(
                    lib,
                    renderer,
                    overlay,
                    space,
                    suspects::PRINT_PICTURES[i],
                    (x + 14.0, y + 13.0),
                    [255; 4],
                );
            }
            let n = i + 1;
            let vehicle = if suspects::shows_vehicle(state, i) {
                s.get(section, &format!("VehicleID{n}"))
            } else {
                String::new()
            };
            for (row, (label, value)) in [
                ("Name", s.get(section, &format!("Name{n}"))),
                ("Nationality", s.get(section, &format!("Nationality{n}"))),
                ("Vehicle", vehicle),
            ]
            .into_iter()
            .enumerate()
            {
                let ry = y + 10.0 + row as f32 * 20.0;
                ui.write(
                    lib,
                    renderer,
                    overlay,
                    space,
                    "SID_TEXT_14",
                    &s.get(section, label),
                    (x + 78.0, ry + 2.0),
                    (None, Align::Left),
                );
                ui.sid_box(
                    lib,
                    renderer,
                    overlay,
                    space,
                    [x + 178.0, ry, 245.0, 18.0],
                    false,
                    [0, 0, 0, 160],
                );
                ui.write(
                    lib,
                    renderer,
                    overlay,
                    space,
                    "SID_TEXT_14",
                    &value,
                    (x + 182.0, ry + 2.0),
                    (None, Align::Left),
                );
            }
            overlay.rect(
                space.rect([x + 1.0, y + 76.0, 430.0, 1.0]),
                [120, 120, 120, 255],
            );
            ui.sid_box(
                lib,
                renderer,
                overlay,
                space,
                [x, y + 76.0, 57.0, 18.0],
                false,
                [0, 0, 0, 0],
            );
            ui.write(
                lib,
                renderer,
                overlay,
                space,
                "SID_TEXT_14",
                &s.get(section, "Notes"),
                (x + 8.0, y + 79.0),
                (None, Align::Left),
            );
            overlay.rect(
                space.rect([x + 1.0, y + 251.0, 430.0, 1.0]),
                [120, 120, 120, 255],
            );
            for (k, &f) in state
                .suspect_links
                .get(&(i as u32))
                .map_or(&[][..], |v| v.as_slice())
                .iter()
                .enumerate()
                .take(6)
            {
                let r = [x + 1.0 + k as f32 * 70.0, y + 250.0, 70.0, 75.0];
                ui.sid_box(
                    lib,
                    renderer,
                    overlay,
                    space,
                    r,
                    sus.picked[i] == Some(k),
                    [0, 0, 0, 128],
                );
                ui.write(
                    lib,
                    renderer,
                    overlay,
                    space,
                    "SID_TEXT_14",
                    &s.file_name(f),
                    (r[0] + 4.0, r[1] + 3.0),
                    (None, Align::Left),
                );
                self.draw_icon(
                    (ui, lib, renderer),
                    overlay,
                    space,
                    f,
                    [r[0] + 19.0, r[1] + 29.0, 32.0, 30.0],
                );
                hits.push((Hit::Evidence(k), r));
            }
        }
        if let Some(m) = &sus.matching {
            let (x, y) = (MATCH_PRINT.0 - 22.0, MATCH_PRINT.1 - 36.0);
            ui.sid_box(
                lib,
                renderer,
                overlay,
                space,
                [x, y, 267.0, 156.0],
                false,
                [0, 0, 0, 220],
            );
            ui.write(
                lib,
                renderer,
                overlay,
                space,
                "SID_TEXT_14",
                &s.get(section, "MatchTitle"),
                (x, y + 2.0),
                (Some(267.0), Align::Center),
            );
            overlay.rect(
                space.rect([x + 1.0, y + 18.0, 265.0, 1.0]),
                [120, 120, 120, 255],
            );
            ui.sid_box(
                lib,
                renderer,
                overlay,
                space,
                [x + 18.0, y + 32.0, 49.0, 59.0],
                false,
                [0, 0, 0, 0],
            );
            // The scan movie plays in the print's place; then the unknown
            // print shows while it is compared, red once matched.
            let picture = match m.phase {
                suspects::Matching::Scanning => None,
                suspects::Matching::Found(_) => Some(m.picture),
                _ => sus.compare_picture(),
            };
            if let Some(picture) = picture {
                ui.sprite(
                    lib,
                    renderer,
                    overlay,
                    space,
                    picture,
                    MATCH_PRINT,
                    [255; 4],
                );
            }
            let (action, who) = match &m.phase {
                suspects::Matching::Scanning => ("MatchAnalyze", String::new()),
                suspects::Matching::Comparing { suspect, .. } => (
                    "MatchCompare",
                    s.get(section, &format!("Name{}", suspect + 1)),
                ),
                suspects::Matching::Found(i) => {
                    ("MatchFound", s.get(section, &format!("Name{}", i + 1)))
                }
                suspects::Matching::None => ("MatchNone", String::new()),
            };
            ui.sid_box(
                lib,
                renderer,
                overlay,
                space,
                [x + 85.0, y + 32.0, 170.0, 18.0],
                false,
                [0, 0, 0, 255],
            );
            ui.write(
                lib,
                renderer,
                overlay,
                space,
                "SID_TEXT_14_GRN",
                &s.get(section, action),
                (x + 85.0, y + 33.0),
                (Some(170.0), Align::Center),
            );
            ui.write(
                lib,
                renderer,
                overlay,
                space,
                "SID_TEXT_14",
                &who,
                (x + 85.0, y + 58.0),
                (Some(170.0), Align::Center),
            );
            let done = matches!(
                m.phase,
                suspects::Matching::Found(_) | suspects::Matching::None
            );
            let found = matches!(m.phase, suspects::Matching::Found(_));
            let link = [x + 12.0, y + 133.0, 112.0, 13.0];
            let close = [x + 144.0, y + 133.0, 112.0, 13.0];
            ui.sid_button(
                lib,
                renderer,
                overlay,
                space,
                link,
                &s.get(section, "MatchLink"),
                ("SID_PDN_10_L", found, at),
            );
            ui.sid_button(
                lib,
                renderer,
                overlay,
                space,
                close,
                &s.get(section, "MatchClose"),
                ("SID_PDN_10_L", done, at),
            );
            if found {
                hits.push((Hit::MatchLink, link));
            }
            if done {
                hits.push((Hit::MatchClose, close));
            }
        }
    }

    /// A file's icon (its item's inventory picture), fitted in a box.
    fn draw_icon(
        &self,
        (ui, lib, renderer): (&mut Ui, &Library, &mut Renderer),
        overlay: &mut Overlay,
        space: Space,
        id: u32,
        r: [f32; 4],
    ) {
        let icon = file(id).map(|f| f.item).filter(|i| !i.is_empty());
        let (tex, w, h) = icon
            .and_then(|item| ui.item_picture(lib, renderer, item))
            .or_else(|| ui.bitmap(lib, renderer, "SIDFILE_9"))
            .unwrap_or_else(|| ui.white());
        let (w, h) = (w as f32, h as f32);
        let fit = (r[2] / w).min(r[3] / h);
        let dest = [
            r[0] + (r[2] - w * fit) / 2.0,
            r[1] + (r[3] - h * fit) / 2.0,
            w * fit,
            h * fit,
        ];
        overlay.image(tex, [0.0, 0.0, w, h], space.rect(dest), [255; 4]);
    }

    /// Make ID: who, the card as it will print, and PRINT.
    fn draw_makeid(
        &self,
        (ui, lib, renderer): (&mut Ui, &Library, &mut Renderer),
        overlay: &mut Overlay,
        space: Space,
        at: (f32, f32),
        hits: &mut Vec<(Hit, [f32; 4])>,
    ) {
        let s = &self.strings;
        let id = &self.makeid;
        // G-Engine's places, in a 426x282 window centred on the screen.
        let (wx, wy) = ((640.0 - 426.0) / 2.0, (480.0 - 282.0) / 2.0);
        let window = [wx, wy, 426.0, 282.0];
        ui.sid_box(lib, renderer, overlay, space, window, false, [0, 0, 0, 128]);
        overlay.rect(
            space.rect([wx + 1.0, wy + 17.0, 424.0, 1.0]),
            [120, 120, 120, 255],
        );
        let header = format!(
            "{} ; {}",
            s.get("MakeID Screen", &format!("Menu{}Name", id.job.0 + 1)),
            s.get(
                "MakeID Screen",
                &format!("Menu{}Item{}", id.job.0 + 1, id.job.1 + 1)
            )
        );
        ui.write(
            lib,
            renderer,
            overlay,
            space,
            "SID_TEXT_14",
            &header,
            (wx, wy + 2.0),
            (Some(418.0), Align::Center),
        );
        let chars = [wx + 22.0, wy + 282.0 - 61.0 - 192.0, 76.0, 192.0];
        ui.sid_box(lib, renderer, overlay, space, chars, false, [0, 0, 0, 128]);
        ui.write(
            lib,
            renderer,
            overlay,
            space,
            "SID_TEXT_14",
            &s.get("MakeID Screen", "Select"),
            (chars[0], chars[1] + 2.0),
            (Some(76.0), Align::Center),
        );
        for (who, from_bottom) in [("GAB", 91.0), ("GRA", 6.0)] {
            let name = format!("{who}_MUGSHOT");
            if let Some((w, h)) = ui.bitmap_size(lib, renderer, &name) {
                let r = [chars[0] + 4.0, chars[1] + 192.0 - from_bottom - h, w, h];
                ui.sprite(lib, renderer, overlay, space, &name, (r[0], r[1]), [255; 4]);
                if id.person == who {
                    ui.sid_box(lib, renderer, overlay, space, r, true, [0, 0, 0, 0]);
                }
                hits.push((Hit::Person(if who == "GAB" { "GAB" } else { "GRA" }), r));
            }
        }
        let preview = [wx + 140.0, wy + 282.0 - 62.0 - 168.0, 259.0, 168.0];
        ui.sid_box(
            lib,
            renderer,
            overlay,
            space,
            preview,
            false,
            [0, 0, 0, 128],
        );
        if let Some((w, h)) = ui.bitmap_size(lib, renderer, &id.card()) {
            let (x, y) = (
                preview[0] + (preview[2] - w) / 2.0,
                preview[1] + (preview[3] - h) / 2.0,
            );
            ui.sprite(lib, renderer, overlay, space, &id.card(), (x, y), [255; 4]);
        }
        if let Some((_, h)) = ui.bitmap_size(lib, renderer, "S_ID_BRACKET") {
            ui.sprite(
                lib,
                renderer,
                overlay,
                space,
                "S_ID_BRACKET",
                (wx + 96.0, wy + 282.0 - 143.0 - h),
                [255; 4],
            );
        }
        let print = [
            wx + 426.0 - 28.0 - 135.0,
            wy + 282.0 - 23.0 - 18.0,
            135.0,
            18.0,
        ];
        ui.sid_button(
            lib,
            renderer,
            overlay,
            space,
            print,
            &s.get("MakeID Screen", "Print"),
            ("SID_TEXT_14", true, at),
        );
        hits.push((Hit::Print, print));
    }

    /// Translate: the file's text, the languages, and TRANSLATE NOW.
    fn draw_translate(
        &self,
        (ui, lib, renderer): (&mut Ui, &Library, &mut Renderer),
        overlay: &mut Overlay,
        space: Space,
        at: (f32, f32),
        hits: &mut Vec<(Hit, [f32; 4])>,
    ) {
        let s = &self.strings;
        let t = &self.translate;
        let open = [24.0, 27.0, 100.0, 13.0];
        ui.write(
            lib,
            renderer,
            overlay,
            space,
            "SID_EMB_10",
            &s.get("Translate Screen", "MenuItem1"),
            (open[0], open[1]),
            (None, Align::Left),
        );
        hits.push((Hit::TranslateOpen, open));
        if t.file.is_none() {
            return;
        }
        let r = [60.0, 70.0, 520.0, 330.0];
        ui.sid_box(lib, renderer, overlay, space, r, false, [0, 0, 0, 128]);
        ui.write(
            lib,
            renderer,
            overlay,
            space,
            "SID_TEXT_14",
            &self.translate_title(),
            (r[0], r[1] + 2.0),
            (Some(r[2] - 8.0), Align::Center),
        );
        overlay.rect(
            space.rect([r[0] + 1.0, r[1] + 17.0, r[2] - 2.0, 1.0]),
            [120, 120, 120, 255],
        );
        // The text, its newest end in view.
        let text_r = [r[0] + 8.0, r[1] + 24.0, 330.0, r[3] - 32.0];
        let lines = ui.wrap("SID_TEXT_14", lib, renderer, &t.text, text_r[2]);
        let (_, lh) = ui.font_size(lib, renderer, "SID_TEXT_14", "X");
        let fits = (text_r[3] / lh).floor() as usize;
        let skip = lines.saturating_sub(fits);
        ui.write_lines(
            lib,
            renderer,
            overlay,
            space,
            "SID_TEXT_14",
            &t.text,
            (text_r[0], text_r[1]),
            text_r[2],
            skip,
            fits,
        );
        // From and to.
        let (fx, tx) = (r[0] + 360.0, r[0] + 440.0);
        ui.write(
            lib,
            renderer,
            overlay,
            space,
            "SID_TEXT_14",
            &s.get("Translate Screen", "From"),
            (fx, r[1] + 26.0),
            (None, Align::Left),
        );
        ui.write(
            lib,
            renderer,
            overlay,
            space,
            "SID_TEXT_14",
            &s.get("Translate Screen", "To"),
            (tx, r[1] + 26.0),
            (None, Align::Left),
        );
        for (i, language) in translate::LANGUAGES.iter().enumerate() {
            let y = r[1] + 44.0 + i as f32 * 16.0;
            let label = s.get("Translate Screen", language.key()).to_uppercase();
            for (from, x, chosen) in [(true, fx, t.from), (false, tx, t.to)] {
                let b = [x, y, 70.0, 13.0];
                ui.sid_box(
                    lib,
                    renderer,
                    overlay,
                    space,
                    b,
                    chosen == *language || crate::ui::inside(b, at),
                    [0, 0, 0, 255],
                );
                ui.write(
                    lib,
                    renderer,
                    overlay,
                    space,
                    "SID_PDN_10_L",
                    &label,
                    (b[0], b[1] + 1.0),
                    (Some(b[2]), Align::Center),
                );
                hits.push((Hit::Language(from, i), b));
            }
        }
        let go = [fx, r[1] + 120.0, 150.0, 13.0];
        ui.sid_button(
            lib,
            renderer,
            overlay,
            space,
            go,
            &s.get("Translate Screen", "TranslateNow"),
            ("SID_PDN_10_L", true, at),
        );
        hits.push((Hit::TranslateGo, go));
    }

    fn draw_adding(
        &self,
        (ui, lib, renderer): (&mut Ui, &Library, &mut Renderer),
        overlay: &mut Overlay,
        space: Space,
    ) {
        let (text, green) = match &self.adding {
            Adding::Waiting { .. } => (self.strings.get("AddData Screen", "WaitForInput"), true),
            Adding::Box { text, blink, .. } => (
                text.clone(),
                !*blink || ((self.clock * 4.0) as u64).is_multiple_of(2),
            ),
            _ => return,
        };
        let outer = [(640.0 - 248.0) / 2.0, (480.0 - 44.0) / 2.0, 248.0, 44.0];
        ui.sid_box(lib, renderer, overlay, space, outer, false, [0, 0, 0, 160]);
        let inner = [outer[0] + 14.0, outer[1] + 13.0, 220.0, 17.0];
        ui.sid_box(lib, renderer, overlay, space, inner, false, [0, 0, 0, 180]);
        let font = if green {
            "SID_TEXT_14_GRN"
        } else {
            "SID_TEXT_14"
        };
        ui.write(
            lib,
            renderer,
            overlay,
            space,
            font,
            &text,
            (inner[0], inner[1] + 1.0),
            (Some(inner[2]), Align::Center),
        );
    }

    fn draw_email_list(
        &self,
        (ui, lib, renderer): (&mut Ui, &Library, &mut Renderer),
        overlay: &mut Overlay,
        space: Space,
        state: &GameState,
        hits: &mut Vec<(Hit, [f32; 4])>,
    ) {
        let r = [20.0, 40.0, 600.0, 380.0];
        ui.sid_box(lib, renderer, overlay, space, r, false, [0, 0, 0, 160]);
        ui.write(
            lib,
            renderer,
            overlay,
            space,
            "SID_TEXT_18",
            &self.strings.get("EMail Screen", "EMailList"),
            (r[0] + 8.0, r[1] + 4.0),
            (None, Align::Left),
        );
        let mut y = r[1] + 28.0;
        for (i, id) in state.emails_received.iter().enumerate() {
            let subject = self
                .emails
                .get(id)
                .map_or(id.as_str(), |e| e.subject.as_str());
            let font = if state.emails_read.contains(id) {
                "SID_TEXT_14"
            } else {
                "SID_TEXT_14_GRN"
            };
            ui.write(
                lib,
                renderer,
                overlay,
                space,
                font,
                subject,
                (r[0] + 8.0, y),
                (None, Align::Left),
            );
            hits.push((Hit::EmailItem(i), [r[0] + 8.0, y, 400.0, 16.0]));
            y += 16.0;
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_email(
        &self,
        (ui, lib, renderer): (&mut Ui, &Library, &mut Renderer),
        overlay: &mut Overlay,
        space: Space,
        state: &GameState,
        index: usize,
        at: (f32, f32),
        hits: &mut Vec<(Hit, [f32; 4])>,
    ) {
        let Some(email) = state
            .emails_received
            .get(index)
            .and_then(|id| self.emails.get(id))
        else {
            return;
        };
        let s = &self.strings;
        let r = [20.0, 40.0, 600.0, 400.0];
        ui.sid_box(lib, renderer, overlay, space, r, false, [0, 0, 0, 160]);
        let mut y = r[1] + 8.0;
        for (key, value) in [
            ("From", &email.from),
            ("To", &email.to),
            ("CC", &email.cc),
            ("Date", &email.date),
            ("Subject", &email.subject),
        ] {
            ui.write(
                lib,
                renderer,
                overlay,
                space,
                "SID_TEXT_14_UL",
                &s.get("EMail Screen", key),
                (r[0] + 8.0, y),
                (None, Align::Left),
            );
            ui.write(
                lib,
                renderer,
                overlay,
                space,
                "SID_TEXT_14",
                value,
                (r[0] + 80.0, y),
                (None, Align::Left),
            );
            y += 16.0;
        }
        overlay.rect(
            space.rect([r[0] + 1.0, y + 2.0, r[2] - 2.0, 1.0]),
            [120, 120, 120, 255],
        );
        y += 8.0;
        let body: Vec<String> = match state.emails_received[index].as_str() {
            // Two answers are diagrams, written out from SIDNEY's strings.
            "EMAIL4" => (1..=10)
                .map(|i| s.get("EMail Screen", &format!("SolomonFile{i}")))
                .collect(),
            "EMAIL5" => (1..=9)
                .map(|i| s.get("EMail Screen", &format!("HermFile{i}")))
                .collect(),
            _ => email.body.clone(),
        };
        for para in &body {
            y += ui
                .write(
                    lib,
                    renderer,
                    overlay,
                    space,
                    "SID_TEXT_14",
                    para,
                    (r[0] + 8.0, y),
                    (Some(r[2] - 16.0), Align::Left),
                )
                .max(8.0);
            if y > r[1] + r[3] - 40.0 {
                break;
            }
        }
        let buttons = [
            (Hit::EmailPrev, "Previous", index > 0),
            (
                Hit::EmailNext,
                "Next",
                index + 1 < state.emails_received.len(),
            ),
            (Hit::EmailReply, "Reply", true),
            (Hit::EmailCompose, "Compose", true),
            (Hit::EmailPrint, "Print", true),
            (Hit::EmailClose, "Exit", true),
        ];
        for (i, (hit, key, enabled)) in buttons.into_iter().enumerate() {
            let b = [r[0] + 8.0 + i as f32 * 90.0, r[1] + r[3] - 24.0, 84.0, 16.0];
            ui.sid_button(
                lib,
                renderer,
                overlay,
                space,
                b,
                &s.get("EMail Screen", key),
                ("SID_PDN_10_L", enabled, at),
            );
            if enabled {
                hits.push((hit, b));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_are_numbered_as_the_scanner_counts() {
        for (i, f) in FILES.iter().enumerate() {
            assert_eq!(f.id as usize, i);
        }
        assert_eq!(
            file(27).unwrap().item,
            "ABBE_TAPE",
            "SidScanner 28, less one"
        );
    }

    /// Every term finds a page the archives have; every page and every
    /// link from one reads and lays out.
    #[test]
    fn every_search_page_is_there() {
        let Some(dir) = std::env::var_os("GK3_DATA") else {
            return;
        };
        let lib = gk3_barn::Library::open_dir(dir).unwrap();
        let terms = load_terms(&lib);
        assert!(terms.len() > 300, "{} terms", terms.len());
        let exists = |p: &str| lib.read(p).is_ok() || lib.read(&p.replace(".HTML", ".HTM")).is_ok();
        struct Fixed;
        impl crate::html::Measure for Fixed {
            fn text(&mut self, _: &str, t: &str) -> (f32, f32) {
                (t.len() as f32 * 6.0, 12.0)
            }
            fn image(&mut self, _: &str) -> Option<(f32, f32)> {
                Some((10.0, 10.0))
            }
        }
        let mut todo: Vec<String> = terms.values().cloned().collect();
        let mut seen = std::collections::HashSet::new();
        let mut missing = Vec::new();
        while let Some(page) = todo.pop() {
            if !seen.insert(page.clone()) {
                continue;
            }
            let Ok(bytes) = lib
                .read(&page)
                .or_else(|_| lib.read(&page.replace(".HTML", ".HTM")))
            else {
                missing.push(page);
                continue;
            };
            let root = crate::html::parse(&bytes);
            let (pieces, _) = crate::html::layout(&root, 500.0, &mut Fixed);
            for piece in pieces {
                if let crate::html::Piece::Text { link: Some(l), .. } = piece
                    && !seen.contains(&l)
                {
                    todo.push(l);
                }
            }
        }
        missing.sort();
        // Links the shipped pages have to pages that were never made.
        assert!(missing.len() < 10, "{} missing: {missing:?}", missing.len());
        assert!(terms.values().all(|p| exists(p) || missing.contains(p)));
        eprintln!("{} pages reachable; missing {missing:?}", seen.len());
    }

    #[test]
    fn mail_arrives_with_the_story() {
        let mut state = GameState::new("205P", "R25");
        assert!(receive_email(&mut state));
        assert_eq!(state.emails_received, ["EMAIL1", "EMAIL2", "EMAIL3"]);
        state.timeblock = "312P".into();
        receive_email(&mut state);
        assert_eq!(state.emails_received.len(), 4);
        state
            .emails_read
            .extend(state.emails_received.iter().cloned());
        assert!(!receive_email(&mut state));
    }
}
