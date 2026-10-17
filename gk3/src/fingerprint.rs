//! The fingerprint kit: dust an object with the brush, lift what shows with
//! tape, and press the tape onto the card to keep it.
//!
//! As G-Engine's `FingerprintScreen`, which this follows, including its
//! table of objects: like the original's, it is not in the game's data.
//! Coordinates are in the 640x480 screen's pixels; print positions are
//! from the bottom left of the right-hand panel.

/// One print on an object.
#[derive(Debug, Clone)]
pub struct Print {
    pub texture: &'static str,
    pub at: (f32, f32),
    /// Said when dusting first shows it.
    pub uncover: Option<&'static str>,
    pub item: Option<&'static str>,
    pub flag: Option<&'static str>,
    /// Grace's own flag for it, where she can lift it too.
    pub flag_grace: Option<&'static str>,
    pub score: Option<&'static str>,
}

/// How an object sits in the panel: hard against its left side (G-Engine's
/// default) or centred; then nudged by an offset, y up.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Anchor {
    Left,
    Center,
}

#[derive(Debug, Clone)]
pub struct Object {
    pub noun: &'static str,
    pub texture: &'static str,
    pub anchor: Anchor,
    pub offset: (f32, f32),
    /// Said after the first, second... print is collected.
    pub collected_lines: &'static [&'static str],
    /// Said when dusting finds nothing, for objects with no prints.
    pub no_print: Option<&'static str>,
    pub prints: Vec<Print>,
}

fn print(
    texture: &'static str,
    at: (f32, f32),
    uncover: Option<&'static str>,
    item: Option<&'static str>,
) -> Print {
    Print {
        texture,
        at,
        uncover,
        item,
        flag: None,
        flag_grace: None,
        score: None,
    }
}

fn object(noun: &'static str, texture: &'static str) -> Object {
    Object {
        noun,
        texture,
        anchor: Anchor::Left,
        offset: (0.0, 0.0),
        collected_lines: &[],
        no_print: None,
        prints: Vec::new(),
    }
}

/// Every object the kit can be used on.
pub fn objects() -> Vec<Object> {
    let with = |mut p: Print, flag: &'static str, score: &'static str| {
        p.flag = (!flag.is_empty()).then_some(flag);
        p.score = (!score.is_empty()).then_some(score);
        p
    };
    vec![
        Object {
            no_print: Some("10LCQ59291"),
            ..object("HBHG_BOOK", "FP_HOLYGBOOK.BMP")
        },
        Object {
            anchor: Anchor::Center,
            prints: vec![with(
                print(
                    "FP_LHOMIR_P1.BMP",
                    (159.0, 134.0),
                    Some("2O8A805PF1"),
                    Some("HOWARDS_FINGERPRINT"),
                ),
                "GotMirrorHowardPrint",
                "e_210a_r31_fingerprint_kit_mirror",
            )],
            ..object("HAND_MIRROR", "FP_LHOMIR.BMP")
        },
        Object {
            offset: (0.0, 12.0),
            collected_lines: &["0A89N052H2"],
            prints: vec![with(
                print(
                    "FP_COLT45_P1.BMP",
                    (65.0, 279.0),
                    Some("0J8ES05291"),
                    Some("BUTHANES_FINGERPRINT"),
                ),
                "GotGunButhanePrint",
                "e_210a_r29_fingerprint_kit_on_gun",
            )],
            ..object("GUN_IN_CASE", "FP_COLT45.BMP")
        },
        Object {
            collected_lines: &["0A89N052H2"],
            prints: vec![with(
                print(
                    "FP_CIGS_P1.BMP",
                    (101.0, 201.0),
                    Some("0D83M05ML1"),
                    Some("ABBE_FINGERPRINT"),
                ),
                "GotCigPackAbbePrint",
                "e_210a_fingerprint_kit_cigarette_pack",
            )],
            ..object("CIG_PACK_IN_DRAWER", "FP_CIGS.BMP")
        },
        Object {
            offset: (0.0, -8.0),
            collected_lines: &["0A89N052H2"],
            prints: vec![with(
                print(
                    "FP_SUITCA_P1.BMP",
                    (111.0, 370.0),
                    Some("0A89N052H1"),
                    Some("BUCHELLIS_FINGERPRINT"),
                ),
                "GotSuitcaseBuchelliPrint",
                "e_210a_r21_fingerprint_kit_suitcase",
            )],
            ..object("SUITCASE", "FP_SUITCA.BMP")
        },
        Object {
            no_print: Some("0K86F05PF1"),
            ..object("JESUS_PICTURE", "FP_JESUS.BMP")
        },
        Object {
            prints: vec![with(
                print(
                    "FP_BOOKIMMORTALS_P1.BMP",
                    (113.0, 144.0),
                    Some("08BBY59411"),
                    Some("MONTREAUX_FINGERPRINT"),
                ),
                "GotImmortalsMontreauxPrint",
                "e_212p_cs2_fingerprint_kit_immortals_book",
            )],
            ..object("BOOK_IN_DRAWER", "FP_BOOKIMMORTALS.BMP")
        },
        Object {
            anchor: Anchor::Center,
            offset: (0.0, 8.0),
            prints: vec![with(
                print(
                    "FP_OCTSHOT_P1.BMP",
                    (81.0, 214.0),
                    Some("0A89N052H2"),
                    Some("WILKES_FINGERPRINT"),
                ),
                "",
                "e_202p_lby_fingerprint_kit_wilke_glass",
            )],
            ..object("DIRTY_GLASS_WILKES", "FP_OCTSHOT.BMP")
        },
        Object {
            anchor: Anchor::Center,
            prints: vec![print(
                "FP_SQRSHOT_P1.BMP",
                (150.0, 202.0),
                Some("0A89N052H2"),
                Some("BUCHELLIS_FINGERPRINT"),
            )],
            ..object("DIRTY_GLASS_BUCHELLI", "FP_SQRSHOT.BMP")
        },
        Object {
            anchor: Anchor::Center,
            prints: vec![with(
                print("FP_SODA_P1.BMP", (147.0, 150.0), None, None),
                "GotPMoselyPrint",
                "e_202p_r25_fingerprint_kit_soda_bottle",
            )],
            ..object("POP_BOTTLE", "FP_SODA.BMP")
        },
        Object {
            prints: vec![with(
                print(
                    "FP_LSRENV_P1.BMP",
                    (178.0, 305.0),
                    Some("10LEM59291"),
                    Some("ESTELLES_FINGERPRINT_LSR"),
                ),
                "GotLEstellePrint",
                "e_205p_inventory_fingerprint_kit_envelope",
            )],
            ..object("LSR_ENVELOPE_INV", "FP_LSRENV.BMP")
        },
        Object {
            collected_lines: &["1077H59291"],
            prints: vec![with(
                print(
                    "FP_BUCHGLASS_P1.BMP",
                    (82.0, 149.0),
                    None,
                    Some("BUCHELLIS_FINGERPRINT"),
                ),
                "GotWineBuchelliPrint",
                "",
            )],
            ..object("DIRTY_WINE_GLASS_BUCHELLI", "FP_BUCHGLASS.BMP")
        },
        Object {
            no_print: Some("1EPXH59291"),
            ..object("GLASS", "FP_GLASS.BMP")
        },
        Object {
            collected_lines: &["1037H59291"],
            prints: vec![with(
                print(
                    "FP_BLOMAN_P6.BMP",
                    (206.0, 301.0),
                    None,
                    Some("LARRYS_FINGERPRINT"),
                ),
                "GotMLarryPrint",
                "e_302a_inventory_fingerprint_kit_manuscript",
            )],
            ..object("BLOODLINE_MANUSCRIPT_202A", "FP_BLOMAN.BMP")
        },
        Object {
            collected_lines: &["1077H59292", "1077H59293", "1077H59294"],
            prints: vec![
                with(
                    print(
                        "FP_BLOMAN_P1.BMP",
                        (200.0, 243.0),
                        Some("1EP02593L1"),
                        Some("UNKNOWN_PRINT_1"),
                    ),
                    "GotMMoselyPrint",
                    "e_312p_bmb_fingerprint_kit_manuscript1",
                ),
                with(
                    print(
                        "FP_BLOMAN_P2.BMP",
                        (205.0, 98.0),
                        Some("1077H59291"),
                        Some("UNKNOWN_PRINT_2"),
                    ),
                    "GotMButhanePrint",
                    "e_312p_bmb_fingerprint_kit_manuscript2",
                ),
                with(
                    print(
                        "FP_BLOMAN_P3.BMP",
                        (199.0, 344.0),
                        Some("1077H59291"),
                        Some("UNKNOWN_PRINT_3"),
                    ),
                    "GotMBuchelliPrint",
                    "e_312p_bmb_fingerprint_kit_manuscript3",
                ),
            ],
            ..object("BLOODLINE_MANUSCRIPT_312P", "FP_BLOMAN.BMP")
        },
        Object {
            anchor: Anchor::Center,
            collected_lines: &["0A89N052H2"],
            prints: vec![Print {
                flag_grace: Some("GotWaterBottleEstellePrintGrace"),
                ..with(
                    print(
                        "FP_WATBTL_P1.BMP",
                        (132.0, 278.0),
                        None,
                        Some("ESTELLES_FINGERPRINT"),
                    ),
                    "GotWaterBottleEstellePrint",
                    "e_303p_wod_fingerprint_kit_water_bottle",
                )
            }],
            ..object("WATER_BOTTLE_ON_MOPED", "FP_WATBTL.BMP")
        },
    ]
}

/// What the player holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Hand,
    Brush,
    DustedBrush,
    Tape,
    /// Tape with a print on it, by index.
    TapeWithPrint(usize),
}

impl Tool {
    /// The cursor for it, if not the ordinary one.
    pub fn cursor(self) -> Option<&'static str> {
        match self {
            Tool::Hand => None,
            Tool::Brush => Some("C_FPBRUSH"),
            Tool::DustedBrush => Some("C_FPBRUSH_WDUST"),
            Tool::Tape => Some("C_FPTAPE_NOFP"),
            Tool::TapeWithPrint(_) => Some("C_FPTAPE_FP"),
        }
    }
}

/// Places on the kit, in 640x480 pixels: x, y from the top left, w, h.
pub const EXIT_AT: (f32, f32) = (5.0, 9.0); // from the bottom left
pub const BRUSH_AT: (f32, f32) = (128.0, 90.0); // from the bottom left
pub const DUST: [f32; 4] = [149.0, 480.0 - 143.0 - 108.0, 127.0, 108.0];
pub const TAPE: [f32; 4] = [278.0, 480.0 - 103.0 - 154.0, 106.0, 154.0];
pub const CLOTH: [f32; 4] = [0.0, 480.0 - 94.0 - 160.0, 140.0, 160.0];
pub const PANEL: [f32; 4] = [640.0 - 2.0 - 249.0, 2.0, 249.0, 476.0];

/// What the kit asks the game to do.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Say(&'static str),
    /// A print is the player's: item, flag and score.
    Grant(usize),
    /// The dirty glasses' own rules (see [`dirty_glass`]).
    DirtyGlass(usize),
    /// Close once anything being said is done.
    Close,
}

/// The kit while it is up.
pub struct Kit {
    pub object: Object,
    pub tool: Tool,
    /// How visible each print is, nought to one, and whether it is kept.
    pub shown: Vec<f32>,
    pub kept: Vec<bool>,
    dusted: f32,
}

impl Kit {
    /// Opens on a noun, or the noun at this timeblock (`NOUN_202A`).
    pub fn open(noun: &str, timeblock: &str, flags: &dyn Fn(&str) -> bool) -> Option<Self> {
        let noun = noun.to_ascii_uppercase();
        let timed = format!("{noun}_{}", timeblock.to_ascii_uppercase());
        let object = objects()
            .into_iter()
            .find(|o| o.noun == noun || o.noun == timed)?;
        let kept = object
            .prints
            .iter()
            .map(|p| p.flag.is_some_and(flags) || p.flag_grace.is_some_and(flags))
            .collect();
        Some(Self {
            shown: vec![0.0; object.prints.len()],
            kept,
            object,
            tool: Tool::Hand,
            dusted: 0.0,
        })
    }

    /// Where a print is on the 640x480 screen, given its picture's size.
    pub fn print_rect(&self, i: usize, (w, h): (u32, u32)) -> [f32; 4] {
        let (x, y) = self.object.prints[i].at;
        [
            PANEL[0] + x,
            PANEL[1] + PANEL[3] - y - h as f32,
            w as f32,
            h as f32,
        ]
    }

    /// Where the object's picture sits, given its size.
    pub fn object_rect(&self, (w, h): (u32, u32)) -> [f32; 4] {
        let (w, h) = (w as f32, h as f32);
        let (dx, dy) = self.object.offset;
        let x = match self.object.anchor {
            Anchor::Left => PANEL[0],
            Anchor::Center => PANEL[0] + (PANEL[2] - w) / 2.0,
        };
        [x + dx, PANEL[1] + (PANEL[3] - h) / 2.0 - dy, w, h]
    }

    /// Rubbing with the dusted brush: `moved` pixels of mouse travel over
    /// `over` (which prints, by index; or the panel, for objects with none).
    pub fn dust(&mut self, over: &[usize], in_panel: bool, moved: f32, dt: f32) -> Vec<Event> {
        let mut events = Vec::new();
        if self.tool != Tool::DustedBrush {
            return events;
        }
        if self.object.prints.is_empty() {
            const NOTHING_AFTER: f32 = 800.0;
            if in_panel && self.dusted < NOTHING_AFTER {
                self.dusted += moved;
                if self.dusted >= NOTHING_AFTER
                    && let Some(line) = self.object.no_print
                {
                    events.extend([Event::Say(line), Event::Close]);
                }
            }
            return events;
        }
        for &i in over {
            let before = self.shown[i];
            self.shown[i] = (before + moved * dt * 0.2).clamp(0.0, 1.0);
            if before < 1.0
                && self.shown[i] >= 1.0
                && let Some(line) = self.object.prints[i].uncover
            {
                events.push(Event::Say(line));
            }
        }
        events
    }

    pub fn click_brush(&mut self) {
        self.tool = match self.tool {
            Tool::Hand => Tool::Brush,
            Tool::Brush | Tool::DustedBrush => Tool::Hand,
            other => other,
        };
    }

    pub fn click_dust(&mut self) {
        if self.tool == Tool::Brush {
            self.tool = Tool::DustedBrush;
        }
    }

    pub fn click_tape(&mut self) {
        self.tool = match self.tool {
            Tool::Hand => Tool::Tape,
            Tool::Tape | Tool::Brush | Tool::DustedBrush => Tool::Hand,
            other => other,
        };
    }

    /// Clicking somewhere with no use puts a brush down.
    pub fn click_elsewhere(&mut self) {
        if matches!(self.tool, Tool::Brush | Tool::DustedBrush) {
            self.tool = Tool::Hand;
        }
    }

    pub fn click_print(&mut self, i: usize) {
        if self.shown[i] < 1.0 {
            return;
        }
        match self.tool {
            Tool::Brush => self.tool = Tool::Hand,
            Tool::Tape => self.tool = Tool::TapeWithPrint(i),
            _ => {}
        }
    }

    /// Pressing tape onto the card keeps its print.
    pub fn click_cloth(&mut self) -> Vec<Event> {
        let Tool::TapeWithPrint(i) = self.tool else {
            self.click_elsewhere();
            return Vec::new();
        };
        self.tool = Tool::Hand;
        if self.kept[i] {
            // "Already got that one."
            return vec![Event::Say("2FL8S27SG1")];
        }
        self.kept[i] = true;
        if self.object.noun.starts_with("DIRTY_GLASS_") {
            return vec![Event::DirtyGlass(i)];
        }
        let n = self.kept.iter().filter(|k| **k).count();
        let mut events = vec![Event::Grant(i)];
        if let Some(line) = self.object.collected_lines.get(n - 1) {
            events.push(Event::Say(line));
        }
        if self.kept.iter().all(|k| *k) {
            events.push(Event::Close);
        }
        events
    }
}

/// `DirtyGlassPrint`'s values: which of the two glasses' prints the player
/// has, and how they labelled them.
pub mod glass {
    pub const NONE: i32 = 0;
    pub const BUCHELLI: i32 = 1;
    pub const WILKES: i32 = 2;
    pub const BUCHELLI_LABELLED_WILKES: i32 = 3;
    pub const WILKES_LABELLED_BUCHELLI: i32 = 4;
    pub const BOTH: i32 = 5;
}

/// What lifting a dirty glass's print leads to, from G-Engine's
/// `OnCollectWilkesDirtyGlass` and `OnCollectBuchelliDirtyGlass`.
#[derive(Debug, Clone, PartialEq)]
pub enum GlassOutcome {
    /// Set the variable, say a line, grant the print (if `grant`), close.
    Known {
        value: i32,
        line: &'static str,
        grant: bool,
    },
    /// Ask whose it is, on this noun's topic bar.
    Ask { noun: &'static str },
    /// Set the variable, then give the print, or the mislabelled item.
    Done {
        value: i32,
        item: Option<&'static str>,
        grant: bool,
    },
}

pub fn dirty_glass(wilkes: bool, have_suitcase_print: bool, state: i32) -> Option<GlassOutcome> {
    use glass::*;
    if have_suitcase_print {
        let value = if state == NONE {
            if wilkes { WILKES } else { BUCHELLI }
        } else {
            BOTH
        };
        let line = if wilkes { "1EK4259NS1" } else { "1EK0259NS1" };
        return Some(GlassOutcome::Known {
            value,
            line,
            grant: wilkes,
        });
    }
    Some(match (wilkes, state) {
        (_, NONE) => GlassOutcome::Ask {
            noun: if wilkes { "CROW" } else { "DAGGER" },
        },
        (true, BUCHELLI) | (false, WILKES) => GlassOutcome::Done {
            value: BOTH,
            item: None,
            grant: true,
        },
        (true, BUCHELLI_LABELLED_WILKES) => GlassOutcome::Done {
            value: BOTH,
            item: Some("WILKES_FINGERPRINT_LABELED_BUCHELLI"),
            grant: false,
        },
        (false, WILKES_LABELLED_BUCHELLI) => GlassOutcome::Done {
            value: BOTH,
            item: Some("BUCHELLIS_FINGERPRINT_LABELED_WILKES"),
            grant: false,
        },
        _ => return None,
    })
}

/// The answer to "whose is it?": the right name gets the real print.
pub fn label(wilkes: bool, topic: &str) -> (i32, Option<&'static str>, bool) {
    use glass::*;
    match (
        wilkes,
        topic.eq_ignore_ascii_case(if wilkes { "T_WILKES" } else { "T_BUCHELLI" }),
    ) {
        (true, true) => (WILKES, None, true),
        (true, false) => (
            WILKES_LABELLED_BUCHELLI,
            Some("WILKES_FINGERPRINT_LABELED_BUCHELLI"),
            false,
        ),
        (false, true) => (BUCHELLI, None, true),
        (false, false) => (
            BUCHELLI_LABELLED_WILKES,
            Some("BUCHELLIS_FINGERPRINT_LABELED_WILKES"),
            false,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dust_lift_and_keep() {
        let mut kit = Kit::open("hand_mirror", "210A", &|_| false).unwrap();
        // Dusting needs a dusted brush.
        assert!(kit.dust(&[0], true, 1000.0, 1.0).is_empty());
        kit.click_brush();
        kit.click_dust();
        assert_eq!(kit.tool, Tool::DustedBrush);
        let events = kit.dust(&[0], true, 100.0, 0.1);
        assert_eq!(
            events,
            [Event::Say("2O8A805PF1")],
            "fully shown: Gabriel remarks on it"
        );
        // Tape can lift it only with the brush put down.
        kit.click_tape();
        kit.click_tape();
        assert_eq!(kit.tool, Tool::Tape);
        kit.click_print(0);
        assert_eq!(kit.tool, Tool::TapeWithPrint(0));
        assert_eq!(kit.click_cloth(), [Event::Grant(0), Event::Close]);
    }

    #[test]
    fn timed_objects_and_kept_prints() {
        let kit = Kit::open("BLOODLINE_MANUSCRIPT", "312P", &|f| f == "GotMButhanePrint").unwrap();
        assert_eq!(kit.object.prints.len(), 3);
        assert_eq!(kit.kept, [false, true, false]);
        assert!(Kit::open("BLOODLINE_MANUSCRIPT", "110A", &|_| false).is_none());
    }

    #[test]
    fn nothing_on_some_things() {
        let mut kit = Kit::open("GLASS", "205P", &|_| false).unwrap();
        kit.click_brush();
        kit.click_dust();
        assert!(kit.dust(&[], true, 500.0, 0.1).is_empty());
        assert_eq!(
            kit.dust(&[], true, 500.0, 0.1),
            [Event::Say("1EPXH59291"), Event::Close]
        );
    }

    #[test]
    fn dirty_glasses() {
        assert_eq!(
            dirty_glass(true, false, glass::NONE),
            Some(GlassOutcome::Ask { noun: "CROW" })
        );
        assert_eq!(
            label(true, "T_BUCHELLI").1,
            Some("WILKES_FINGERPRINT_LABELED_BUCHELLI")
        );
        assert_eq!(
            dirty_glass(false, true, glass::WILKES),
            Some(GlassOutcome::Known {
                value: glass::BOTH,
                line: "1EK0259NS1",
                grant: false
            })
        );
    }
}
