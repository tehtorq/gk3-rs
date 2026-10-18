//! SIDNEY's tools as sequences: a popup, a pause, a movie, then what the
//! analysis found, and so on. G-Engine writes these as chains of timers and
//! callbacks; here they are a queue of [`Step`]s played in order, each
//! waiting for the one before (a popup with buttons waits for a choice,
//! and the choice's own steps go next).

use std::collections::VecDeque;
use std::sync::Arc;

use gk3_barn::Library;
use gk3_formats::image::Image;
use gk3_game::state::GameState;
use gk3_media::avi::{Avi, Player};
use gk3_render::{Overlay, Renderer};

use super::{Event, Hit, Sidney};
use crate::ui::{Align, Space, Ui};

/// A message box: text, perhaps a picture above it, and buttons that each
/// lead somewhere; or a line to type into.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Popup {
    pub text: String,
    pub image: Option<String>,
    pub center: bool,
    /// From the screen's centre, y down; and the size, if not the usual.
    pub offset: (f32, f32),
    pub size: Option<(f32, f32)>,
    pub buttons: Vec<(String, Vec<Step>)>,
    /// A text box, and what typing in it answers.
    pub input: Option<(String, Answer)>,
}

impl Popup {
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            center: true,
            ..Default::default()
        }
    }

    /// With an OK button that just closes it.
    pub fn ok(mut self, label: impl Into<String>) -> Self {
        self.buttons = vec![(label.into(), Vec::new())];
        self
    }

    pub fn left(mut self) -> Self {
        self.center = false;
        self
    }

    pub fn at(mut self, x: f32, y: f32) -> Self {
        self.offset = (x, y);
        self
    }

    pub fn sized(mut self, w: f32, h: f32) -> Self {
        self.size = Some((w, h));
        self
    }

    pub fn image(mut self, name: &str) -> Self {
        self.image = Some(name.to_string());
        self
    }

    fn waits(&self) -> bool {
        !self.buttons.is_empty() || self.input.is_some()
    }
}

/// What a typed answer is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Answer {
    /// The word missing from "Et in Arcadia Ego".
    ArcadiaWord,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum Step {
    Show(Popup),
    Wait(f64),
    /// A movie from the Data directory, in a slot, at a place; then a
    /// still picture left in its place, keyed by colour.
    Video {
        #[serde(deserialize_with = "crate::saved::static_str")]
        name: crate::saved::Name,
        slot: usize,
        at: (f32, f32),
        #[serde(deserialize_with = "crate::saved::option_static_keyed")]
        then: Option<(crate::saved::Name, [u8; 3])>,
    },
    /// Takes a slot's movie or picture away.
    Clear(usize),
    /// Waits until what is being said (or done) in the game is over.
    Talk,
    Do(Event),
    /// A change in a tool's own state.
    Tool(ToolStep),
}

/// Changes to a tool's state that steps make once they get there.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum ToolStep {
    /// Analyze's state goes back to before analysis.
    AnalyzePre,
    /// Translate appends text.
    TranslateText(String),
    /// Asks again which language a parchment's message is in.
    Languages(#[serde(deserialize_with = "crate::saved::static_str")] crate::saved::Name, Vec<Step>),
    /// Match Analysis' scan has played: comparing begins.
    Scanned,
    /// The map: a grid to fill the screen or a shape; a grid of a size, in
    /// a shape or not; a shape put on it.
    MapGridFill(bool),
    MapGrid(u32, bool),
    MapShape(#[serde(deserialize_with = "crate::saved::static_str")] crate::saved::Name),
}

/// What the queue is waiting on.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Waiting {
    Until(f64),
    Video(usize),
    Popup,
    /// The game is busy saying something.
    Talk,
}

/// A movie playing in a slot, or the picture it left.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Video {
    pub at: (f32, f32),
    /// The movie, and when it began if it is still playing: what a saved
    /// game keeps, the decoder being opened again on restoring.
    name: String,
    since: Option<f64>,
    #[serde(skip)]
    playing: Option<(Avi, Player, f64)>,
    shown: usize,
    pub frame: Option<Image>,
    pub dirty: bool,
    pub still: Option<(String, [u8; 3])>,
}

/// The colour SIDNEY's movies leave see-through.
const MOVIE_KEY: [u8; 3] = [3, 251, 3];

/// Makes pixels near one of `keys` transparent.
pub fn key_out(image: &mut Image, keys: &[[u8; 3]]) {
    for px in image.rgba.chunks_mut(4) {
        if keys.iter().any(|k| {
            (0..3)
                .map(|i| (px[i] as i32 - k[i] as i32).abs())
                .sum::<i32>()
                < 24
        }) {
            px[3] = 0;
        }
    }
}

impl Sidney {
    /// Queues steps after any already waiting.
    pub(super) fn then(&mut self, steps: impl IntoIterator<Item = Step>) {
        self.queue.extend(steps);
    }

    /// Puts steps before any already waiting.
    pub(super) fn now(&mut self, steps: Vec<Step>) {
        for step in steps.into_iter().rev() {
            self.queue.push_front(step);
        }
    }

    /// Plays steps until one has to wait. Returns what they ask of the game.
    pub(super) fn pump(&mut self, lib: &Library, state: &mut GameState) -> Vec<Event> {
        let mut events = Vec::new();
        for _ in 0..64 {
            match self.waiting {
                Some(Waiting::Until(t)) if self.clock < t => break,
                Some(Waiting::Video(slot))
                    if self.videos[slot]
                        .as_ref()
                        .is_some_and(|v| v.playing.is_some()) =>
                {
                    break;
                }
                Some(Waiting::Popup) if self.popup.as_ref().is_some_and(Popup::waits) => break,
                // Let go of in `tick`, once the game is quiet.
                Some(Waiting::Talk) => break,
                _ => self.waiting = None,
            }
            let Some(step) = self.queue.pop_front() else {
                break;
            };
            match step {
                // As in G-Engine, a popup holds up clicks, not what comes
                // after it; its buttons' own steps follow when chosen.
                Step::Show(popup) => self.popup = Some(popup),
                Step::Wait(seconds) => self.waiting = Some(Waiting::Until(self.clock + seconds)),
                Step::Video {
                    name,
                    slot,
                    at,
                    then,
                } => {
                    let avi = lib.loose_file(name).and_then(|b| Avi::parse(b).ok());
                    let video = Video {
                        at,
                        name: name.to_string(),
                        since: None,
                        playing: None,
                        shown: 0,
                        frame: None,
                        dirty: false,
                        still: then.map(|(n, k)| (n.to_string(), k)),
                    };
                    self.videos[slot] = Some(video);
                    if let Some(avi) = avi {
                        if let Some(pcm) = &avi.audio {
                            events.push(Event::Pcm(Arc::new(pcm.clone())));
                        }
                        let player = avi.frames();
                        if let Some(v) = &mut self.videos[slot] {
                            v.playing = Some((avi, player, self.clock));
                            v.since = Some(self.clock);
                        }
                        self.waiting = Some(Waiting::Video(slot));
                    }
                }
                Step::Clear(slot) => self.videos[slot] = None,
                Step::Talk => self.waiting = Some(Waiting::Talk),
                Step::Do(event) => events.push(event),
                Step::Tool(t) => events.extend(self.tool_step(t, state)),
            }
        }
        events
    }

    fn tool_step(&mut self, step: ToolStep, state: &mut GameState) -> Vec<Event> {
        match step {
            ToolStep::Scanned => return self.suspects.scanned(self.clock, state),
            ToolStep::AnalyzePre => self.analyze.pre(),
            ToolStep::TranslateText(text) => self.translate.text.push_str(&text),
            ToolStep::Languages(key, french) => {
                let ask = self.languages(key, french);
                self.queue.push_front(Step::Show(ask));
            }
            ToolStep::MapGridFill(in_shape) => {
                let steps = self.map_grid_fill(in_shape, state);
                self.now(steps);
            }
            ToolStep::MapGrid(n, in_shape) => {
                let steps = self.map_grid(n, in_shape, state);
                self.now(steps);
            }
            ToolStep::MapShape(name) => {
                let steps = self.map_shape(name, state);
                self.now(steps);
            }
        }
        Vec::new()
    }

    /// After a restore: movies that were playing are opened again, to
    /// catch up with the clock from where they began.
    pub(super) fn reopen_videos(&mut self, lib: &gk3_barn::Library) {
        for v in self.videos.iter_mut().flatten() {
            let Some(since) = v.since else { continue };
            let Some(avi) = lib.loose_file(&v.name).and_then(|b| Avi::parse(b).ok()) else {
                v.since = None;
                continue;
            };
            let player = avi.frames();
            v.playing = Some((avi, player, since));
            v.shown = 0;
        }
    }

    /// Movies move on with the clock.
    pub(super) fn play_videos(&mut self) {
        let now = self.clock;
        for v in self.videos.iter_mut().flatten() {
            let Some((avi, player, started)) = &mut v.playing else {
                continue;
            };
            let due = ((now - *started) / avi.frame_seconds) as usize + 1;
            let mut changed = false;
            while v.shown < due {
                if !player.advance(avi) {
                    v.playing = None;
                    v.since = None;
                    break;
                }
                v.shown += 1;
                changed = true;
            }
            if changed && v.playing.is_some() {
                let mut image = player_image(v);
                key_out(&mut image, &[MOVIE_KEY]);
                v.frame = Some(image);
                v.dirty = true;
            } else if v.playing.is_none() && v.still.is_some() {
                // Finished: its last picture gives way to the still.
                v.frame = None;
            }
        }
    }

    /// A click on the popup's button `i`, or its OK with typed text.
    pub(super) fn answer(&mut self, i: usize, events: &mut Vec<Event>, state: &mut GameState) {
        let Some(popup) = self.popup.take() else {
            return;
        };
        if let Some((typed, answer)) = popup.input {
            self.typed_answer(answer, &typed, events, state);
            return;
        }
        if let Some((_, steps)) = popup.buttons.into_iter().nth(i) {
            // The choice's steps go before anything else waiting.
            for step in steps.into_iter().rev() {
                self.queue.push_front(step);
            }
        }
    }

    pub(super) fn draw_videos(
        &mut self,
        (ui, lib, renderer): (&mut Ui, &Library, &mut Renderer),
        overlay: &mut Overlay,
        space: Space,
    ) {
        for (slot, v) in self.videos.iter_mut().enumerate() {
            let Some(v) = v else { continue };
            if let Some(frame) = &v.frame {
                let tex = ui.video_texture(renderer, slot, frame, v.dirty);
                v.dirty = false;
                let (w, h) = (frame.width as f32, frame.height as f32);
                overlay.image(
                    tex,
                    [0.0, 0.0, w, h],
                    space.rect([v.at.0, v.at.1, w, h]),
                    [255; 4],
                );
            } else if v.playing.is_none()
                && let Some((name, key)) = &v.still
            {
                ui.keyed_sprite(lib, renderer, overlay, space, name, v.at, *key);
            }
        }
    }

    /// The popup, centred (then moved by its offset), its text wrapped.
    pub(super) fn draw_popup(
        &self,
        (ui, lib, renderer): (&mut Ui, &Library, &mut Renderer),
        overlay: &mut Overlay,
        space: Space,
        at: (f32, f32),
        hits: &mut Vec<(Hit, [f32; 4])>,
    ) {
        let Some(p) = &self.popup else { return };
        let (w, min_h) = p.size.unwrap_or((250.0, 0.0));
        let image = p
            .image
            .as_ref()
            .and_then(|i| ui.bitmap_size(lib, renderer, i));
        let w = image.map_or(w, |(iw, _)| w.max(iw + 20.0));
        let text_w = w - 15.0;
        // Measure the text by laying it out off screen first.
        let lines = ui.wrap("SID_TEXT_14", lib, renderer, &p.text, text_w);
        let (_, lh) = ui.font_size(lib, renderer, "SID_TEXT_14", "X");
        let text_h = lines as f32 * lh;
        let image_h = image.map_or(0.0, |(_, h)| h + 10.0);
        let input_h = if p.input.is_some() { 20.0 } else { 0.0 };
        let h = (text_h + image_h + input_h + 36.0).max(min_h);
        let x = (640.0 - w) / 2.0 + p.offset.0;
        let y = (480.0 - h) / 2.0 + p.offset.1;
        ui.sid_box(
            lib,
            renderer,
            overlay,
            space,
            [x, y, w, h],
            false,
            [0, 0, 0, 200],
        );
        let mut ty = y + 8.0;
        if let (Some(name), Some((iw, ih))) = (&p.image, image) {
            ui.sprite(
                lib,
                renderer,
                overlay,
                space,
                name,
                (x + (w - iw) / 2.0, ty),
                [255; 4],
            );
            ty += ih + 10.0;
        }
        let align = if p.center { Align::Center } else { Align::Left };
        ui.write(
            lib,
            renderer,
            overlay,
            space,
            "SID_TEXT_14",
            &p.text,
            (x + 8.0, ty),
            (Some(text_w), align),
        );
        if let Some((typed, _)) = &p.input {
            let r = [x + 8.0, ty + text_h + 2.0, w - 16.0, 16.0];
            ui.sid_box(lib, renderer, overlay, space, r, false, [0, 0, 0, 255]);
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
                &format!("{typed}{caret}"),
                (r[0] + 4.0, r[1] + 1.0),
                (None, Align::Left),
            );
        }
        let labels: Vec<String> = match (&p.input, p.buttons.is_empty()) {
            (Some(_), _) => vec![self.strings.get("Analyze Screen", "OKButton")],
            (None, false) => p.buttons.iter().map(|(l, _)| l.clone()).collect(),
            (None, true) => Vec::new(),
        };
        let n = labels.len() as f32;
        let gap = if n >= 3.0 { 70.0 } else { 96.0 };
        for (i, label) in labels.iter().enumerate() {
            let cx = x + w / 2.0 + (i as f32 - (n - 1.0) / 2.0) * gap;
            let b = [cx - 32.0, y + h - 21.0, 64.0, 13.0];
            ui.sid_button(
                lib,
                renderer,
                overlay,
                space,
                b,
                label,
                ("SID_PDN_10_L", true, at),
            );
            hits.push((Hit::PopupButton(i), b));
        }
    }
}

fn player_image(v: &Video) -> Image {
    v.playing.as_ref().map_or_else(
        || Image {
            width: 1,
            height: 1,
            rgba: vec![0; 4],
        },
        |(_, player, _)| player.image(),
    )
}

/// The popup or steps a typed answer leads to.
impl Sidney {
    fn typed_answer(
        &mut self,
        answer: Answer,
        typed: &str,
        events: &mut Vec<Event>,
        _state: &mut GameState,
    ) {
        match answer {
            Answer::ArcadiaWord => self.arcadia_word(typed, events),
        }
    }

    /// Typing into a popup's text box.
    pub fn popup_typing(&self) -> bool {
        self.popup.as_ref().is_some_and(|p| p.input.is_some())
    }

    pub(super) fn popup_type(&mut self, text: &str) {
        if let Some(Popup {
            input: Some((typed, _)),
            ..
        }) = &mut self.popup
        {
            for c in text.chars().filter(|c| !c.is_control()) {
                if typed.chars().count() < 24 {
                    typed.push(c);
                }
            }
        }
    }

    pub(super) fn popup_backspace(&mut self) {
        if let Some(Popup {
            input: Some((typed, _)),
            ..
        }) = &mut self.popup
        {
            typed.pop();
        }
    }
}

/// An empty queue.
pub fn queue() -> VecDeque<Step> {
    VecDeque::new()
}
