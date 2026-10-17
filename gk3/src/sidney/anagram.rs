//! The Anagram Parser: SIDNEY's tool for the one anagram in the game, the
//! Arcadia text ("Et in Arcadia Ego Sum") once Ophiuchus is the step of Le
//! Serpent Rouge. SIDNEY scrambles the letters while it "finds" Latin words
//! in them; the player picks words into the phrase building area, each one
//! using up its letters; when no word can be spelled from what is left, the
//! choice is checked. Arcam, Dei and Tango (in any order) leave IESU: SIDNEY
//! scrambles once more, finds the last word, and gives the translation, "I
//! touch the tomb of God, Jesus".
//!
//! As G-Engine's `SidneyAnagramParser` (GPL-3.0): its layout, timings (a
//! two-second pause, letters reshuffled every 0.15 s, words found every
//! 0.15 s after four seconds, a ten-second final scramble, two seconds
//! between closing messages), strings and outcome. Two differences, each a
//! slip in G-Engine: without "Sum", only the shorter word list is ever
//! offered (G-Engine's refresh also lets the full list's words show), and a
//! word with a space in it ("I Tego") can be picked (G-Engine counts the
//! space as a letter it lacks).
//!
//! It is a screen of its own over Analyze: the caller draws it, passes its
//! clicks back, ticks it, and carries out what it returns.

use std::collections::VecDeque;

use gk3_barn::Library;
use gk3_render::{Overlay, Renderer};

use super::Strings;
use crate::ui::{Align, Space, Ui, inside};

/// The screen the parser's strings are under.
const SCREEN: &str = "Analyze Screen";
/// The window: 600 by 319, in the middle of SIDNEY's 640 by 480.
const WINDOW: [f32; 4] = [20.0, 80.0, 600.0, 319.0];
const MAX_LETTERS: usize = 17;
const MAX_SELECTED: usize = 10;
/// The full word list, and the shorter one found without "Sum".
const FULL_WORDS: usize = 161;
const SHORT_WORDS: usize = 36;
/// The three words that solve it, by their place in the full list
/// (Word13, Word41, Word147): Arcam, Dei, Tango.
const ANSWER: [usize; 3] = [12, 40, 146];
const SCRAMBLE_INTERVAL: f32 = 0.15;
const FIRST_WORD_DELAY: f32 = 4.0;
const WORD_INTERVAL: f32 = 0.15;
const FINAL_SCRAMBLE: f32 = 10.0;
const PAUSE: f32 = 2.0;
/// The word list: where its rows start, how far apart, how many show.
const LIST_TOP: f32 = 85.0;
const ROW: f32 = 15.0;
const ROWS: usize = 9;

/// Where the parser has got to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// The pause before it starts.
    Idle,
    InitialScramble,
    SelectWords,
    FinalScramble,
    Solved,
    Unsolved,
}

/// What can be clicked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnagramHit {
    /// A word in the list, by its place in the list of words.
    Word(usize),
    Erase,
    Exit,
    ScrollUp,
    ScrollDown,
}

/// What the parser asks of the game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Out {
    /// The scrambling sound (`SIDANAGRAMSCRAMBLE`, looping) on or off.
    ScrambleSound(bool),
    /// Grace says a line.
    Say(&'static str, i32),
    Score(&'static str),
    /// Solved: the Ophiuchus step of Le Serpent Rouge is done (the caller
    /// moves the LSR state on, as G-Engine's `UpdateLSRState`).
    SetFlag(&'static str),
    /// The player left: back to the Arcadia text in Analyze.
    Exit,
}

/// Steps of the timed messages, each after its pause.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Act {
    StartScramble,
    NoMatch,
    TryAgain,
    FinalWord,
    Correct,
    Rebus,
    Translation,
}

pub struct Anagram {
    pub state: State,
    /// The text being parsed, as shown.
    text: String,
    /// Its letters, spaces out, upper case; those not yet used; and what
    /// each letter's place shows now (scrambled, or not).
    all: Vec<char>,
    remaining: Vec<char>,
    shown: Vec<char>,
    /// Places whose letter a chosen word has used.
    used: Vec<bool>,
    /// The word list: each "Latin (English)", and whether it is offered.
    words: Vec<String>,
    offered: Vec<bool>,
    /// The words chosen, in order (None: the final word SIDNEY finds).
    selected: Vec<Option<usize>>,
    messages: Vec<String>,
    translated: String,
    /// The final word, "Iesu (Jesus)".
    final_word: String,
    /// The answer's words, as they read in the list.
    answer: Vec<String>,
    /// Timers: the next reshuffle, the next word found, the final scramble.
    scramble: Option<f32>,
    next_word: Option<f32>,
    final_timer: Option<f32>,
    /// Words found so far in the initial scramble.
    found: usize,
    queue: VecDeque<(f32, Act)>,
    exit_enabled: bool,
    /// The first row of the word list shown.
    pub scroll: usize,
    rng: u64,
    /// The strings it shows, by key.
    strings: Vec<(&'static str, String)>,
}

const KEYS: [&str; 16] = [
    "AnagramTitle",
    "Parsing",
    "SelectText",
    "PhraseText",
    "EraseButton",
    "ExitButton",
    "LatinMsg",
    "Latin2Msg",
    "SelectMsg",
    "CheckingMsg",
    "MatchMsg",
    "CorrectMsg",
    "RebusMsg",
    "TransMsg",
    "NoMatchMsg",
    "TryAgain",
];

/// "Arcam (Tomb)" as the Latin and the English.
fn split(word: &str) -> (String, String) {
    match word.split_once('(') {
        Some((latin, english)) => (
            latin.trim().to_string(),
            english.trim().trim_end_matches(')').trim().to_string(),
        ),
        None => (word.trim().to_string(), String::new()),
    }
}

/// The letters of a word that count: no spaces, upper case.
fn letters(word: &str) -> Vec<char> {
    word.chars()
        .filter(|c| !c.is_whitespace())
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

/// Takes a word's letters out of a pool; false (and the pool unchanged)
/// if it lacks one.
fn take(pool: &mut Vec<char>, word: &[char]) -> bool {
    let mut left = pool.clone();
    for c in word {
        match left.iter().position(|l| l == c) {
            Some(i) => {
                left.remove(i);
            }
            None => return false,
        }
    }
    *pool = left;
    true
}

impl Anagram {
    /// Opens the parser on a text. `full` is whether "Sum" has been found
    /// (`ArcadiaComplete`): then all 161 words are there to find, else the
    /// 36 the shorter text allows.
    pub fn new(text: &str, full: bool, strings: &Strings) -> Self {
        let get = |key: &str| strings.get(SCREEN, key);
        let (prefix, count) = if full {
            ("Word", FULL_WORDS)
        } else {
            ("WordB", SHORT_WORDS)
        };
        let words: Vec<String> = (1..=count).map(|i| get(&format!("{prefix}{i}"))).collect();
        let answer = ANSWER
            .iter()
            .map(|i| get(&format!("Word{}", i + 1)))
            .collect();
        Self::with(
            text,
            words,
            answer,
            get("FinalWord"),
            KEYS.map(|k| (k, get(k))).to_vec(),
        )
    }

    fn with(
        text: &str,
        words: Vec<String>,
        answer: Vec<String>,
        final_word: String,
        strings: Vec<(&'static str, String)>,
    ) -> Self {
        let mut all = letters(text);
        all.truncate(MAX_LETTERS);
        let n = words.len();
        let mut parser = Self {
            state: State::Idle,
            text: text.to_string(),
            remaining: all.clone(),
            shown: all.clone(),
            used: vec![false; all.len()],
            all,
            words,
            offered: vec![false; n],
            selected: Vec::new(),
            messages: Vec::new(),
            translated: String::new(),
            final_word,
            answer,
            scramble: None,
            next_word: None,
            final_timer: None,
            found: 0,
            queue: VecDeque::new(),
            exit_enabled: false,
            scroll: 0,
            rng: 0x2545_F491_4F6C_DD1D,
            strings,
        };
        // A pause first: SIDNEY is thinking about it.
        parser.queue.push_back((PAUSE, Act::StartScramble));
        parser
    }

    fn s(&self, key: &str) -> String {
        self.strings
            .iter()
            .find(|(k, _)| *k == key)
            .map_or_else(|| key.to_string(), |(_, v)| v.clone())
    }

    fn say(&mut self, key: &str) {
        let text = self.s(key);
        self.messages.push(text);
    }

    /// Whether the player must wait (G-Engine's manual action): while it
    /// thinks, scrambles or reports.
    pub fn busy(&self) -> bool {
        !self.queue.is_empty() || self.scramble.is_some() || self.state == State::Idle
    }

    fn random(&mut self, n: usize) -> usize {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng % n.max(1) as u64) as usize
    }

    fn start_scramble(&mut self, out: &mut Vec<Out>) {
        out.push(Out::ScrambleSound(true));
        self.scramble = Some(0.001);
        match self.state {
            State::InitialScramble => {
                self.found = 0;
                self.next_word = Some(FIRST_WORD_DELAY);
            }
            State::FinalScramble => self.final_timer = Some(FINAL_SCRAMBLE),
            _ => {}
        }
    }

    fn stop_scramble(&mut self, out: &mut Vec<Out>) {
        out.push(Out::ScrambleSound(false));
        self.shown = self.all.clone();
        self.scramble = None;
        if self.state == State::InitialScramble {
            self.say("SelectMsg");
        }
        self.exit_enabled = true;
    }

    /// Reshuffles the letters not yet used among their places.
    fn reshuffle(&mut self) {
        let mut pool = self.remaining.clone();
        for i in 0..self.shown.len() {
            if self.used[i] {
                continue;
            }
            if pool.is_empty() {
                break;
            }
            let k = self.random(pool.len());
            self.shown[i] = pool.remove(k);
        }
    }

    /// Moves time on; returns what the game must do.
    pub fn tick(&mut self, dt: f32) -> Vec<Out> {
        let mut out = Vec::new();
        if let Some((left, _)) = self.queue.front_mut() {
            *left -= dt;
        }
        while let Some(&(left, act)) = self.queue.front() {
            if left > 0.0 {
                break;
            }
            self.queue.pop_front();
            self.act(act, &mut out);
        }
        if let Some(t) = &mut self.scramble {
            *t -= dt;
            if *t <= 0.0 {
                // The time overshot counts towards the next.
                let over = *t;
                self.reshuffle();
                self.scramble = Some(SCRAMBLE_INTERVAL + over);
            }
        }
        if let Some(t) = &mut self.next_word {
            *t -= dt;
            if *t <= 0.0 {
                if self.found < self.words.len() {
                    self.offered[self.found] = true;
                    self.found += 1;
                }
                if self.found < self.words.len() {
                    self.next_word = Some(WORD_INTERVAL + *t);
                } else {
                    self.next_word = None;
                    self.stop_scramble(&mut out);
                    self.state = State::SelectWords;
                }
            }
        }
        if let Some(t) = &mut self.final_timer {
            *t -= dt;
            if *t <= 0.0 {
                self.final_timer = None;
                self.state = State::Solved;
                self.stop_scramble(&mut out);
                self.say("MatchMsg");
                self.queue.push_back((PAUSE, Act::FinalWord));
            }
        }
        out
    }

    fn act(&mut self, act: Act, out: &mut Vec<Out>) {
        match act {
            Act::StartScramble => {
                self.say("LatinMsg");
                self.say("Latin2Msg");
                self.state = State::InitialScramble;
                self.start_scramble(out);
            }
            Act::NoMatch => {
                self.say("NoMatchMsg");
                self.queue.push_back((PAUSE, Act::TryAgain));
            }
            Act::TryAgain => {
                self.say("TryAgain");
                self.state = State::Unsolved;
            }
            Act::FinalWord => {
                self.selected.push(None);
                self.refresh();
                self.queue.push_back((PAUSE, Act::Correct));
            }
            Act::Correct => {
                let mut text = self.s("CorrectMsg");
                for slot in self.slot_words().iter().take(3) {
                    text = text.replacen("%s", slot, 1);
                }
                self.messages.push(text);
                self.queue.push_back((PAUSE, Act::Rebus));
            }
            Act::Rebus => {
                self.say("RebusMsg");
                self.queue.push_back((PAUSE, Act::Translation));
            }
            Act::Translation => {
                self.say("TransMsg");
                out.push(Out::Say("02OFT2ZRNA", 1));
                out.push(Out::Score("e_sidney_analysis_complete_anagram"));
                out.push(Out::SetFlag("Ophiuchus"));
            }
        }
    }

    fn word(&self, choice: Option<usize>) -> &str {
        match choice {
            Some(i) => &self.words[i],
            None => &self.final_word,
        }
    }

    /// The Latin of each chosen word, in its slot.
    fn slot_words(&self) -> Vec<String> {
        self.selected
            .iter()
            .map(|c| split(self.word(*c)).0)
            .collect()
    }

    /// After a choice: the letters left, the words that can still be
    /// spelled from them, the letters marked used, the translation so far.
    /// Returns whether any word can still be chosen.
    fn refresh(&mut self) -> bool {
        let mut used = Vec::new();
        let mut english = Vec::new();
        for c in &self.selected {
            let (latin, en) = split(self.word(*c));
            used.extend(letters(&latin));
            english.push(en);
        }
        self.translated = english.join(" ");
        self.remaining = self.all.clone();
        for c in &used {
            if let Some(i) = self.remaining.iter().position(|l| l == c) {
                self.remaining.remove(i);
            }
        }
        let chosen: Vec<usize> = self.selected.iter().flatten().copied().collect();
        for i in 0..self.words.len() {
            let latin = letters(&split(&self.words[i]).0);
            let mut pool = self.remaining.clone();
            self.offered[i] = !chosen.contains(&i) && take(&mut pool, &latin);
        }
        // Mark a place used for each used letter, first come first served.
        self.used = vec![false; self.shown.len()];
        for c in &used {
            if let Some(i) = (0..self.shown.len()).find(|&i| !self.used[i] && self.shown[i] == *c) {
                self.used[i] = true;
            }
        }
        self.scroll = 0;
        self.offered.iter().any(|o| *o)
    }

    /// The words offered, in list order.
    fn list(&self) -> Vec<usize> {
        (0..self.words.len()).filter(|&i| self.offered[i]).collect()
    }

    fn erase_enabled(&self) -> bool {
        !self.busy()
            && !self.selected.is_empty()
            && matches!(self.state, State::SelectWords | State::Unsolved)
    }

    /// A click on the parser. Returns what the game must do.
    pub fn click(&mut self, hit: AnagramHit) -> Vec<Out> {
        let mut out = Vec::new();
        match hit {
            AnagramHit::Word(i) => {
                if self.busy()
                    || self.state != State::SelectWords
                    || !self.offered.get(i).copied().unwrap_or(false)
                    || self.selected.len() >= MAX_SELECTED
                {
                    return out;
                }
                self.selected.push(Some(i));
                if !self.refresh() {
                    self.say("CheckingMsg");
                    let mut chosen: Vec<&str> =
                        self.selected.iter().map(|c| self.word(*c)).collect();
                    let mut answer: Vec<&str> = self.answer.iter().map(String::as_str).collect();
                    chosen.sort_unstable();
                    answer.sort_unstable();
                    if chosen == answer {
                        self.state = State::FinalScramble;
                        self.start_scramble(&mut out);
                    } else {
                        self.queue.push_back((PAUSE, Act::NoMatch));
                    }
                }
            }
            AnagramHit::Erase => {
                if !self.erase_enabled() {
                    return out;
                }
                self.selected.pop();
                // Back to choosing, with the messages as they were.
                if self.refresh() && self.state == State::Unsolved {
                    self.state = State::SelectWords;
                    self.messages.clear();
                    self.say("LatinMsg");
                    self.say("Latin2Msg");
                    self.say("SelectMsg");
                }
            }
            AnagramHit::Exit => {
                if self.exit_enabled && !self.busy() {
                    out.push(Out::ScrambleSound(false));
                    out.push(Out::Exit);
                }
            }
            AnagramHit::ScrollUp => self.scroll_by(-1),
            AnagramHit::ScrollDown => self.scroll_by(1),
        }
        out
    }

    /// Scrolls the word list by rows.
    pub fn scroll_by(&mut self, rows: isize) {
        let max = self.list().len().saturating_sub(ROWS);
        self.scroll = (self.scroll as isize + rows).clamp(0, max as isize) as usize;
    }

    /// Draws the parser over Analyze in SIDNEY's 640x480 `space`; `at` is
    /// the pointer there. Returns where its clicks go.
    pub fn draw(
        &self,
        (ui, lib, renderer): (&mut Ui, &Library, &mut Renderer),
        overlay: &mut Overlay,
        space: Space,
        at: (f32, f32),
    ) -> Vec<(AnagramHit, [f32; 4])> {
        let mut hits = Vec::new();
        let [x0, y0, w, _] = WINDOW;
        let r = |x: f32, y: f32, w: f32, h: f32| [x0 + x, y0 + y, w, h];
        let grey = [120, 120, 120, 255];
        let mut d = Draw {
            ui,
            lib,
            renderer,
            overlay,
            space,
        };
        d.ui.sid_box(
            d.lib,
            d.renderer,
            d.overlay,
            space,
            WINDOW,
            false,
            [0, 0, 0, 128],
        );
        for y in [61.0, 231.0, 291.0] {
            d.overlay.rect(space.rect(r(1.0, y, w - 2.0, 1.0)), grey);
        }
        d.overlay.rect(space.rect(r(183.0, 61.0, 1.0, 171.0)), grey);

        // Header: what is being parsed, and its letters.
        d.text(
            "SID_TEXT_14",
            &self.s("AnagramTitle"),
            r(15.0, 9.0, 0.0, 0.0),
            None,
        );
        let parsing = format!("{} {}", self.s("Parsing"), self.text);
        d.text("SID_TEXT_14", &parsing, r(161.0, 9.0, 0.0, 0.0), None);
        for (i, c) in self.shown.iter().enumerate() {
            let font = if self.used[i] {
                "SID_TEXT_14_UL"
            } else {
                "SID_TEXT_14"
            };
            d.text(
                font,
                &c.to_string(),
                r(88.0 + i as f32 * 20.0, 34.0, 0.0, 0.0),
                None,
            );
        }

        // The words found, to choose from.
        let list = self.list();
        for (row, &i) in list.iter().skip(self.scroll).take(ROWS).enumerate() {
            let y = LIST_TOP + row as f32 * ROW;
            d.text("SID_TEXT_14", &self.words[i], r(15.0, y, 0.0, 0.0), None);
            hits.push((AnagramHit::Word(i), r(1.0, y, 168.0, ROW)));
        }
        if list.len() > ROWS {
            for (hit, y, mark) in [
                (AnagramHit::ScrollUp, 64.0, "^"),
                (AnagramHit::ScrollDown, 216.0, "v"),
            ] {
                let b = r(170.0, y, 12.0, 13.0);
                d.ui.sid_box(
                    d.lib,
                    d.renderer,
                    d.overlay,
                    space,
                    b,
                    inside(b, at),
                    [0, 0, 0, 255],
                );
                d.text("SID_PDN_10_L", mark, b, Some(Align::Center));
                hits.push((hit, b));
            }
        }
        d.ui.sid_box(
            d.lib,
            d.renderer,
            d.overlay,
            space,
            r(0.0, 61.0, 60.0, 14.0),
            false,
            [0, 0, 0, 255],
        );
        d.text(
            "SID_PDN_10_UL",
            &self.s("SelectText"),
            r(8.0, 63.0, 0.0, 0.0),
            None,
        );

        // What SIDNEY has to say, its newest lines in view.
        let text = self.messages.join("\n");
        let (mx, my, mw, mh) = (x0 + 193.0, y0 + 68.5, 405.0, 160.0);
        let lines = d.ui.wrap("SID_TEXT_14", d.lib, d.renderer, &text, mw);
        let (_, lh) = d.ui.font_size(d.lib, d.renderer, "SID_TEXT_14", "X");
        let fits = (mh / lh.max(1.0)).floor() as usize;
        d.ui.write_lines(
            d.lib,
            d.renderer,
            d.overlay,
            space,
            "SID_TEXT_14",
            &text,
            (mx, my),
            mw,
            lines.saturating_sub(fits),
            fits,
        );

        // The phrase building area: a slot for each chosen word.
        d.ui.sid_box(
            d.lib,
            d.renderer,
            d.overlay,
            space,
            r(0.0, 231.0, 160.0, 14.0),
            false,
            [0, 0, 0, 255],
        );
        d.text(
            "SID_PDN_10_UL",
            &self.s("PhraseText"),
            r(8.0, 233.0, 0.0, 0.0),
            None,
        );
        let slots = self.slot_words();
        for i in 0..MAX_SELECTED {
            let x = 28.0 + 50.0 * i as f32;
            d.overlay.rect(space.rect(r(x, 262.0, 40.0, 1.0)), grey);
            if let Some(word) = slots.get(i) {
                d.text(
                    "SID_TEXT_14",
                    word,
                    r(x - 10.0, 246.0, 60.0, 15.0),
                    Some(Align::Center),
                );
            }
        }
        d.text(
            "SID_TEXT_14",
            &self.translated,
            r(28.0, 270.0, 0.0, 0.0),
            None,
        );

        // Erase and Exit.
        for (hit, x, key, on) in [
            (AnagramHit::Erase, 10.0, "EraseButton", self.erase_enabled()),
            (
                AnagramHit::Exit,
                83.0,
                "ExitButton",
                self.exit_enabled && !self.busy(),
            ),
        ] {
            let b = r(x, 299.0, 60.0, 13.0);
            d.ui.sid_button(
                d.lib,
                d.renderer,
                d.overlay,
                space,
                b,
                &self.s(key),
                ("SID_PDN_10_L", on, at),
            );
            if on {
                hits.push((hit, b));
            }
        }
        hits
    }
}

/// What drawing needs, kept together.
struct Draw<'a> {
    ui: &'a mut Ui,
    lib: &'a Library,
    renderer: &'a mut Renderer,
    overlay: &'a mut Overlay,
    space: Space,
}

impl Draw<'_> {
    /// Text at a rectangle's top left; centred in its width if asked.
    fn text(&mut self, font: &str, text: &str, [x, y, w, _]: [f32; 4], align: Option<Align>) {
        let width = align.map(|_| w);
        let align = align.unwrap_or(Align::Left);
        self.ui.write(
            self.lib,
            self.renderer,
            self.overlay,
            self.space,
            font,
            text,
            (x, y),
            (width, align),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The full list's words that matter here, and a couple of others.
    fn parser() -> Anagram {
        let mut words: Vec<String> = (0..FULL_WORDS)
            .map(|i| format!("Zzz{i} (Nothing)"))
            .collect();
        words[12] = "Arcam (Tomb)".into();
        words[40] = "Dei (God)".into();
        words[146] = "Tango (I Touch)".into();
        words[0] = "Academia (University)".into();
        words[5] = "Arca (Ark)".into();
        let answer = ANSWER.iter().map(|&i| words[i].clone()).collect();
        let strings = KEYS.map(|k| (k, k.to_string())).to_vec();
        let mut p = Anagram::with(
            "Et in Arcadia Ego Sum",
            words,
            answer,
            "Iesu (Jesus)".into(),
            strings,
        );
        if let Some(i) = p.strings.iter().position(|(k, _)| *k == "CorrectMsg") {
            p.strings[i].1 = "%s %s %s, Iesu.".into();
        }
        p
    }

    fn run(p: &mut Anagram, seconds: f32) -> Vec<Out> {
        let mut out = Vec::new();
        for _ in 0..(seconds * 100.0) as usize {
            out.extend(p.tick(0.01));
        }
        out
    }

    #[test]
    fn it_thinks_then_scrambles_then_lists_every_word() {
        let mut p = parser();
        assert_eq!(p.all.len(), 17);
        assert!(p.busy());
        let out = run(&mut p, 2.1);
        assert_eq!(p.state, State::InitialScramble);
        assert!(out.contains(&Out::ScrambleSound(true)));
        // The scramble keeps the same letters.
        let mut a = p.shown.clone();
        let mut b = p.all.clone();
        a.sort_unstable();
        b.sort_unstable();
        assert_eq!(a, b);
        // Four seconds, then a word every 0.15 s: 161 of them.
        let out = run(&mut p, 4.0 + 0.15 * 161.0 + 0.5);
        assert_eq!(p.state, State::SelectWords);
        assert!(!p.busy());
        assert!(out.contains(&Out::ScrambleSound(false)));
        assert_eq!(p.shown, p.all, "letters back in order");
        assert_eq!(p.messages.last().map(String::as_str), Some("SelectMsg"));
        assert_eq!(p.list().len(), FULL_WORDS);
    }

    #[test]
    fn choosing_uses_letters_and_narrows_the_list() {
        let mut p = parser();
        run(&mut p, 32.0);
        p.click(AnagramHit::Word(12)); // Arcam
        assert_eq!(p.used.iter().filter(|u| **u).count(), 5);
        assert!(!p.offered[12]);
        // "Academia" now needs more letters than are left.
        assert!(!p.offered[0]);
        assert!(p.offered[40] && p.offered[146]);
        assert_eq!(p.translated, "Tomb");
        p.click(AnagramHit::Erase);
        assert!(p.selected.is_empty() && p.offered[0]);
    }

    #[test]
    fn the_right_words_solve_it() {
        let mut p = parser();
        run(&mut p, 32.0);
        for i in [146, 12, 40] {
            p.click(AnagramHit::Word(i));
        }
        assert_eq!(p.state, State::FinalScramble);
        assert!(p.busy());
        let out = run(&mut p, 10.0 + 2.0 * 4.0 + 0.5);
        assert_eq!(p.state, State::Solved);
        assert_eq!(p.slot_words(), ["Tango", "Arcam", "Dei", "Iesu"]);
        assert!(p.messages.contains(&"Tango Arcam Dei, Iesu.".to_string()));
        assert!(out.contains(&Out::SetFlag("Ophiuchus")));
        assert!(out.contains(&Out::Score("e_sidney_analysis_complete_anagram")));
        assert!(out.contains(&Out::Say("02OFT2ZRNA", 1)));
        assert_eq!(p.messages.last().map(String::as_str), Some("TransMsg"));
    }

    #[test]
    fn wrong_words_say_so_and_can_be_erased() {
        let mut p = parser();
        run(&mut p, 32.0);
        // Arca, then whatever is left until nothing more can be spelled.
        p.click(AnagramHit::Word(5));
        while let Some(&i) = p.list().first() {
            p.click(AnagramHit::Word(i));
        }
        assert_eq!(p.messages.last().map(String::as_str), Some("CheckingMsg"));
        run(&mut p, 4.5);
        assert_eq!(p.state, State::Unsolved);
        assert_eq!(p.messages.last().map(String::as_str), Some("TryAgain"));
        p.click(AnagramHit::Erase);
        p.click(AnagramHit::Erase);
        assert_eq!(p.state, State::SelectWords);
        assert_eq!(p.messages, ["LatinMsg", "Latin2Msg", "SelectMsg"]);
    }

    #[test]
    fn words_with_spaces_can_be_spelled() {
        let mut pool = letters("ETINARCADIAEGO");
        assert!(take(&mut pool, &letters("I Tego")));
        assert_eq!(
            split("I Tego (I Conceal)"),
            ("I Tego".into(), "I Conceal".into())
        );
    }
}
