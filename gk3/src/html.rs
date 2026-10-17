//! The web pages SIDNEY's search finds: a small HTML reader, and the layout
//! of a page in the game's Times fonts.
//!
//! As G-Engine's `SidneySearch`, which this follows: only the tags the
//! pages use are tags; anything else in angle brackets is text. `P`, `BR`,
//! `LI`, `HR` and `IMG` never close. The pages are Latin-1.

/// A tag with its attributes and contents, or a run of text.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Element {
    /// Upper case for a tag; `None` for text.
    pub tag: Option<String>,
    pub text: String,
    pub attributes: Vec<(String, String)>,
    pub contents: Vec<Element>,
}

const TAGS: &[&str] = &[
    "HTML",
    "HEAD",
    "TITLE",
    "BODY",
    "P",
    "FONT",
    "HR",
    "A",
    "I",
    "B",
    "U",
    "BR",
    "LI",
    "IMG",
    "UL",
    "OL",
    "TT",
    "DIV",
    "CENTER",
    "TABLE",
    "TR",
    "TD",
    "ST1:PLACE",
    "ST1:PLACETYPE",
    "ST1:PLACENAME",
    "ST1:STREET",
    "ST1:CITY",
    "ST1:STATE",
    "ST1:POSTALCODE",
    "ST1:ADDRESS",
    "ST1:COUNTRY-REGION",
];
const UNCLOSED: &[&str] = &["P", "BR", "LI", "HR", "IMG"];
const LONGEST_TAG: usize = 18;

fn is_tag(name: &str) -> bool {
    TAGS.iter().any(|t| t.eq_ignore_ascii_case(name.trim()))
}

fn entities(text: &str) -> String {
    let mut s = text.to_string();
    for (from, to) in [
        ("&nbsp;", " "),
        ("&nbsp", " "),
        ("&sup1;", "'"),
        ("&sup2;", "\""),
        ("&sup3;", "\""),
        ("&quot;", "\""),
        ("&quot", "\""),
        ("&aelig;", "\u{e6}"),
        ("&eacute;", "\u{e9}"),
        ("&amp;", "&"),
        ("&gt;", ">"),
        ("\t", " "),
    ] {
        s = s.replace(from, to);
    }
    s
}

/// Reads a page into its `HTML` element.
pub fn parse(bytes: &[u8]) -> Element {
    let chars: Vec<char> = bytes.iter().map(|&b| b as char).collect();
    let mut root = Element {
        tag: Some("HTML".into()),
        ..Default::default()
    };
    // Where each open element is, as a path of indices from the root.
    let mut path: Vec<usize> = Vec::new();
    let mut content = String::new();
    let mut i = 0;
    fn at<'a>(root: &'a mut Element, path: &[usize]) -> &'a mut Element {
        path.iter().fold(root, |e, &i| &mut e.contents[i])
    }
    while i < chars.len() {
        let c = chars[i];
        i += 1;
        if c == '\n' || c == '\r' {
            if !content.is_empty() && !content.ends_with(' ') {
                content.push(' ');
            }
            continue;
        }
        if c != '<' {
            content.push(c);
            continue;
        }
        // A tag, if it names one we know.
        let mut raw = String::from("<");
        let mut tag = String::new();
        let mut close = false;
        let mut valid = false;
        let mut first = true;
        while i < chars.len() {
            let t = chars[i];
            i += 1;
            raw.push(t);
            if first && t == '/' {
                close = true;
                continue;
            }
            first = false;
            if t == '>' {
                if !valid {
                    content.push_str(&raw);
                }
                break;
            }
            if t == '<' {
                raw.pop();
                content.push_str(&raw);
                i -= 1;
                break;
            }
            tag.push(t);
            if !valid {
                valid = is_tag(&tag);
                if !valid && tag.trim().len() >= LONGEST_TAG {
                    content.push_str(&raw);
                    break;
                }
            }
        }
        if !valid {
            continue;
        }
        let tokens: Vec<&str> = tag.split_whitespace().collect();
        let Some(name) = tokens.first().map(|t| t.to_ascii_uppercase()) else {
            continue;
        };
        if !is_tag(&name) {
            continue;
        }
        if !content.is_empty() {
            let text = entities(&content);
            at(&mut root, &path).contents.push(Element {
                text,
                ..Default::default()
            });
            content.clear();
        }
        if close {
            if !UNCLOSED.contains(&name.as_str()) && !path.is_empty() {
                let open = at(&mut root, &path).tag.clone().unwrap_or_default();
                if open == name {
                    path.pop();
                }
            }
            continue;
        }
        if name == "HTML" {
            continue;
        }
        let attributes = tokens[1..]
            .iter()
            .map(|t| {
                let (k, v) = t.split_once('=').unwrap_or((t, ""));
                (
                    k.trim_matches('"').to_ascii_uppercase(),
                    v.trim_matches('"').to_string(),
                )
            })
            .collect();
        let parent = at(&mut root, &path);
        parent.contents.push(Element {
            tag: Some(name.clone()),
            attributes,
            ..Default::default()
        });
        if !UNCLOSED.contains(&name.as_str()) {
            path.push(parent.contents.len() - 1);
        }
    }
    if !content.is_empty() {
        let text = entities(&content);
        at(&mut root, &path).contents.push(Element {
            text,
            ..Default::default()
        });
    }
    root
}

/// A link or page name as the archives have it: upper case, one `.HTML`,
/// no folders; `#anchors` go to the sources page; and the pages' own
/// misspellings put right.
pub fn page_name(link: &str) -> String {
    if link.starts_with('#') {
        return "SOURCES.HTML".into();
    }
    let link = link.rsplit('/').next().unwrap_or(link).to_ascii_uppercase();
    let ext = match link.rfind('.') {
        Some(i) => link[i..].to_string(),
        None => ".HTML".into(),
    };
    let stem = link.split('.').next().unwrap_or("");
    let name = format!("{stem}{ext}");
    const MISSPELT: &[(&str, &str)] = &[
        (
            "ALTERED_STATES_OF_CONCIOUSNESS.HTML",
            "ALTERED_STATES_OF_CONSCIOUSNESS.HTML",
        ),
        ("APPORPTS.HTML", "APPORTS.HTML"),
        ("BLACKWOOD_ALGERON.HTML", "BLACKWOOD_ALGERNON.HTML"),
        ("CEPHALOMNACY.HTML", "CEPHALOMANCY.HTML"),
        (
            "CLEDONISM_OR_CLEDOMANTIA.HTML",
            "CLEDONISM_OR_CLEDONOMANTIA.HTML",
        ),
        (
            "HOUSE_OF_THE_SEVEN_GABLES_THE.HTML",
            "HOUSE_OF_SEVEN_GABLES_THE.HTML",
        ),
        ("INCOMMUNICABLE AXIOM.HTML", "INCOMMUNICABLE_AXIOM.HTML"),
        ("KHYLYSTY.HTML", "KHLYSTY.HTML"),
        ("LAMPDOMANCY.HTML", "LAMPODOMANCY.HTML"),
        ("LAW_OF_TRUE-FALSEHOODS.HTML", "LAW_OF_TRUE-FALSHOODS.HTML"),
        ("LEVIATION.HTML", "LEVITATION.HTML"),
        ("MAGIC SQUARES.HTML", "MAGIC_SQUARES.HTML"),
        ("METEMPHYSCHOSIS.HTML", "METEMPSYCHOSIS.HTML"),
        ("NUMA_POMPLILIUS.HTML", "NUMA_POMPILIUS.HTML"),
        (
            "ONIMNANCY_OR_ONYCOMONACY.HTML",
            "ONIMANCY_OR_ONYCOMANCY.HTML",
        ),
        ("PSYCHOKNESIS_PK.HTML", "PSYCHOKINESIS_PK.HTML"),
        ("RASPUTIAN.HTML", "RASPUTIN.HTML"),
        ("TETRAGRAMMMATION.HTML", "TETRAGRAMMATIOM.HTML"),
    ];
    MISSPELT
        .iter()
        .find(|(from, _)| *from == name)
        .map_or(name, |(_, to)| to.to_string())
}

/// The style text is drawn in, from the tags it is inside.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Style {
    pub bold: bool,
    pub italic: bool,
    pub big: bool,
    pub link: Option<String>,
}

impl Style {
    /// The Times font for it, as G-Engine chooses.
    pub fn font(&self) -> &'static str {
        match (self.link.is_some(), self.bold, self.italic, self.big) {
            (true, ..) => "F_TIMES_L",
            (_, true, _, true) => "F_TIMES_B_2",
            (_, true, _, false) => "F_TIMES_B",
            (_, false, true, _) => "F_TIMES_I",
            (_, _, _, true) => "F_TIMES_2",
            _ => "F_TIMES",
        }
    }
}

/// One thing on a laid-out page, in its pixels from the page's top left.
#[derive(Debug, Clone, PartialEq)]
pub enum Piece {
    Text {
        font: &'static str,
        text: String,
        x: f32,
        y: f32,
        link: Option<String>,
    },
    Image {
        name: String,
        x: f32,
        y: f32,
    },
    Rule {
        y: f32,
    },
    Bullet {
        x: f32,
        y: f32,
    },
}

/// What layout needs to know of fonts and pictures.
pub trait Measure {
    /// A text's width and its font's line height.
    fn text(&mut self, font: &str, text: &str) -> (f32, f32);
    fn image(&mut self, name: &str) -> Option<(f32, f32)>;
}

/// Flows a page into a column `width` wide. Returns its pieces and height.
pub fn layout(root: &Element, width: f32, m: &mut dyn Measure) -> (Vec<Piece>, f32) {
    struct Flow<'a> {
        m: &'a mut dyn Measure,
        width: f32,
        x: f32,
        y: f32,
        line: f32,
        out: Vec<Piece>,
    }
    const BREAK: f32 = 15.0;
    impl Flow<'_> {
        fn newline(&mut self) {
            self.y += self.line.max(0.0);
            self.x = 0.0;
            self.line = 0.0;
        }
        fn text(&mut self, style: &Style, text: &str) {
            let font = style.font();
            let mut run = String::new();
            let mut run_x = self.x;
            // Words, keeping the spaces with the word before.
            for word in text.split_inclusive(' ') {
                let (w, h) = self.m.text(font, word);
                let (ww, _) = self.m.text(font, word.trim_end());
                if self.x > 0.0 && self.x + ww > self.width {
                    self.flush(font, &mut run, run_x, style);
                    self.newline();
                    run_x = 0.0;
                    if word.trim().is_empty() {
                        continue;
                    }
                }
                self.line = self.line.max(h);
                run.push_str(word);
                self.x += w;
            }
            self.flush(font, &mut run, run_x, style);
        }
        fn flush(&mut self, font: &'static str, run: &mut String, x: f32, style: &Style) {
            if !run.is_empty() {
                self.out.push(Piece::Text {
                    font,
                    text: std::mem::take(run),
                    x,
                    y: self.y,
                    link: style.link.clone(),
                });
            }
        }
        fn walk(&mut self, e: &Element, style: &Style) {
            let Some(tag) = &e.tag else {
                self.text(style, &e.text);
                return;
            };
            match tag.as_str() {
                "HEAD" => return,
                "P" | "BR" => {
                    self.newline();
                    self.y += BREAK;
                }
                "LI" => {
                    if self.x > 0.0 {
                        self.newline();
                    }
                    let (bw, bh) = self.m.image("SIDNEYBULLET").unwrap_or((6.0, 6.0));
                    self.out.push(Piece::Bullet {
                        x: self.x,
                        y: self.y + 4.0,
                    });
                    self.x += bw + 2.0;
                    self.line = self.line.max(bh);
                }
                "HR" => {
                    self.newline();
                    self.y += BREAK / 2.0;
                    self.out.push(Piece::Rule { y: self.y });
                    self.y += BREAK / 2.0;
                }
                "IMG" => {
                    let src = e
                        .attributes
                        .iter()
                        .find(|(k, _)| k == "SRC")
                        .or(e.attributes.first());
                    if let Some((_, src)) = src {
                        let name = src.rsplit('/').next().unwrap_or(src);
                        let name = name.split('.').next().unwrap_or(name).to_ascii_uppercase();
                        if let Some((_, h)) = self.m.image(&name) {
                            self.out.push(Piece::Image {
                                name,
                                x: self.x,
                                y: self.y,
                            });
                            self.newline();
                            self.y += h;
                        }
                    }
                }
                _ => {}
            }
            let mut inner = style.clone();
            match tag.as_str() {
                "B" => inner.bold = true,
                "I" => inner.italic = true,
                "FONT" => {
                    let size = e
                        .attributes
                        .first()
                        .and_then(|(_, v)| v.trim_start_matches('+').parse::<i32>().ok());
                    inner.big = size.unwrap_or(0) >= 2;
                }
                "A" => {
                    inner.link = e
                        .attributes
                        .iter()
                        .find(|(k, _)| k == "HREF")
                        .map(|(_, v)| page_name(v))
                }
                _ => {}
            }
            for child in &e.contents {
                self.walk(child, &inner);
            }
        }
    }
    let mut flow = Flow {
        m,
        width,
        x: 0.0,
        y: 0.0,
        line: 0.0,
        out: Vec::new(),
    };
    flow.walk(root, &Style::default());
    flow.newline();
    (flow.out, flow.y)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixed;
    impl Measure for Fixed {
        fn text(&mut self, _: &str, text: &str) -> (f32, f32) {
            (text.chars().count() as f32 * 10.0, 12.0)
        }
        fn image(&mut self, _: &str) -> Option<(f32, f32)> {
            Some((20.0, 30.0))
        }
    }

    #[test]
    fn reads_tags_and_leaves_the_rest_as_text() {
        let page = parse(b"<HTML><HEAD><TITLE>x</TITLE></HEAD><BODY><B>Bold</B> a<z <P>Next\r\nline &amp; more</BODY></HTML>");
        let body = &page.contents[1];
        assert_eq!(body.tag.as_deref(), Some("BODY"));
        assert_eq!(body.contents[0].tag.as_deref(), Some("B"));
        assert_eq!(body.contents[1].text, " a<z ");
        assert_eq!(body.contents[2].tag.as_deref(), Some("P"));
        assert_eq!(body.contents[3].text, "Next line & more");
    }

    #[test]
    fn links_are_cleaned_up() {
        assert_eq!(page_name("!HTML_rus/divination.html"), "DIVINATION.HTML");
        assert_eq!(page_name("Rasputian.htm.html"), "RASPUTIN.HTML");
        assert_eq!(page_name("#top"), "SOURCES.HTML");
        assert_eq!(page_name("Chessboard"), "CHESSBOARD.HTML");
    }

    #[test]
    fn flows_and_wraps() {
        let page =
            parse(b"<HTML><BODY>aaa bbb ccc<P><A HREF=\"x.html\">link</A><LI>item</BODY></HTML>");
        let (pieces, height) = layout(&page, 80.0, &mut Fixed);
        // "aaa bbb " fits in 80, "ccc" wraps.
        assert!(
            matches!(&pieces[0], Piece::Text { text, y, .. } if text == "aaa bbb " && *y == 0.0)
        );
        assert!(matches!(&pieces[1], Piece::Text { text, y, .. } if text == "ccc" && *y == 12.0));
        assert!(
            matches!(&pieces[2], Piece::Text { link: Some(l), font: "F_TIMES_L", .. } if l == "X.HTML")
        );
        assert!(pieces.iter().any(|p| matches!(p, Piece::Bullet { .. })));
        assert!(height > 40.0);
    }
}
