//! The INI dialect shared by GK3's text assets: scene files (`.SIF`, `.SCN`),
//! soundtracks (`.STK`), animations (`.ANM`, `.YAK`) and others.
//!
//! On top of plain INI:
//!
//! - A line may hold several comma-separated pairs, and commas inside braces
//!   belong to the value: `START, pos={1, 2, 3}, heading=-16` is three pairs.
//!   Whether to split this way depends on the file type ([`Split`]), because
//!   scene assets write bare vectors (`Position=1,2,3`).
//! - A bare token is a flag whose value repeats its key.
//! - A section header may carry a Sheep condition:
//!   `[MODELS={IsCurrentTime("202p")}]`. It is kept verbatim for the caller.
//! - `//` and `/* */` comments.
//!
//! Text is Latin-1 with CRLF (sometimes CR CR LF) line endings and may be
//! NUL-terminated.

/// How a line's text becomes key/value pairs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Split {
    /// Comma-separated pairs; commas inside braces do not split.
    Commas,
    /// One `key=value` per line; commas are part of the value.
    Whole,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Document {
    pub sections: Vec<Section>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Section {
    /// Empty for lines before the first header.
    pub name: String,
    /// The Sheep expression between the header's braces, if any.
    pub condition: Option<String>,
    pub lines: Vec<Line>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    /// The line with comments removed and whitespace trimmed. Formats whose
    /// lines are not key/value (animation events) read this directly.
    pub text: String,
    pub pairs: Vec<Pair>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pair {
    pub key: String,
    /// For a bare flag, the same as the key.
    pub value: String,
}

impl Pair {
    pub fn is_flag(&self) -> bool {
        self.key == self.value
    }
}

impl Line {
    /// The value of the first pair with this key, ignoring case.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.pairs
            .iter()
            .find(|p| p.key.eq_ignore_ascii_case(key))
            .map(|p| p.value.as_str())
    }
}

impl Document {
    pub fn parse(bytes: &[u8], split: Split) -> Self {
        let text = decode(bytes);
        let text = strip_block_comments(&text);
        let mut doc = Document::default();
        let mut current = Section::default();

        for raw in text.split('\n') {
            let line = strip_line_comment(raw).trim();
            if line.is_empty() {
                continue;
            }
            // LHM.SIF has a conditional header missing its closing bracket,
            // which the original read as a header all the same.
            let unclosed = line.contains("={") && line.ends_with('}');
            if let Some(header) = line
                .strip_prefix('[')
                .filter(|_| line.contains(']') || unclosed)
            {
                if !current.name.is_empty() || !current.lines.is_empty() {
                    doc.sections.push(std::mem::take(&mut current));
                }
                current = parse_header(header);
                continue;
            }
            current.lines.push(Line {
                text: line.to_string(),
                pairs: parse_pairs(line, split),
            });
        }
        if !current.name.is_empty() || !current.lines.is_empty() {
            doc.sections.push(current);
        }
        doc
    }

    /// Every section with this name, ignoring case, in file order. A name can
    /// repeat with different conditions.
    pub fn sections<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Section> + 'a {
        self.sections
            .iter()
            .filter(move |s| s.name.eq_ignore_ascii_case(name))
    }

    /// The first section with this name, ignoring case.
    pub fn section(&self, name: &str) -> Option<&Section> {
        self.sections
            .iter()
            .find(|s| s.name.eq_ignore_ascii_case(name))
    }
}

/// Latin-1 to UTF-8, dropping trailing NULs and normalising line endings.
fn decode(bytes: &[u8]) -> String {
    let end = bytes.iter().rposition(|&b| b != 0).map_or(0, |i| i + 1);
    bytes[..end]
        .iter()
        .filter(|&&b| b != b'\r')
        .map(|&b| b as char)
        .collect()
}

fn strip_block_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("/*") {
        out.push_str(&rest[..start]);
        match rest[start + 2..].find("*/") {
            Some(end) => {
                // Keep line structure so later lines keep their numbers.
                let comment = &rest[start..start + 2 + end + 2];
                out.extend(comment.chars().filter(|&c| c == '\n'));
                rest = &rest[start + 2 + end + 2..];
            }
            None => {
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

fn strip_line_comment(line: &str) -> &str {
    // `//` starts a comment, except in a URL (`http://`), which SIDNEY-related
    // text does contain.
    let mut search = 0;
    while let Some(i) = line[search..].find("//") {
        let at = search + i;
        if line[..at].ends_with(':') {
            search = at + 2;
            continue;
        }
        return &line[..at];
    }
    line
}

fn parse_header(header: &str) -> Section {
    // `NAME]` or `NAME={condition}]`. The condition is everything between the
    // first `{` and the last `}`, so a stray `]` after it is not carried in.
    let (name, condition) = match (header.find('{'), header.rfind('}')) {
        (Some(open), Some(close)) if close > open => {
            let name = header[..open].trim_end().trim_end_matches('=').trim();
            (name, Some(header[open + 1..close].trim().to_string()))
        }
        _ => (
            header[..header.find(']').unwrap_or(header.len())].trim(),
            None,
        ),
    };
    Section {
        name: name.to_string(),
        condition,
        lines: Vec::new(),
    }
}

fn parse_pairs(line: &str, split: Split) -> Vec<Pair> {
    let parts: Vec<&str> = match split {
        Split::Whole => vec![line],
        Split::Commas => split_outside_braces(line),
    };
    parts
        .into_iter()
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(|part| match part.split_once('=') {
            Some((k, v)) => Pair {
                key: k.trim().to_string(),
                value: v.trim().to_string(),
            },
            None => Pair {
                key: part.to_string(),
                value: part.to_string(),
            },
        })
        .collect()
}

fn split_outside_braces(line: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    for (i, c) in line.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => depth -= 1,
            ',' if depth <= 0 => {
                parts.push(&line[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&line[start..]);
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commas_inside_braces_stay_in_the_value() {
        let doc = Document::parse(
            b"[POSITIONS]\r\nSTART, pos={235.26, 2.72, 57.99}, heading=-16.04, camera=START\r\n",
            Split::Commas,
        );
        let line = &doc.section("positions").unwrap().lines[0];
        assert_eq!(line.pairs.len(), 4);
        assert!(line.pairs[0].is_flag());
        assert_eq!(line.get("POS"), Some("{235.26, 2.72, 57.99}"));
        assert_eq!(line.get("camera"), Some("START"));
    }

    #[test]
    fn whole_mode_keeps_bare_vectors() {
        let doc = Document::parse(b"[Light]\nPosition=1,2,3\n", Split::Whole);
        assert_eq!(doc.sections[0].lines[0].get("Position"), Some("1,2,3"));
    }

    #[test]
    fn conditional_headers() {
        let doc = Document::parse(
            b"[MODELS={IsCurrentTime(\"202p\") && GetEgoCurrentLocationCount() < 1}]\nbed\n\
                            [MODELS]\nlamp\n[ODD={A()}]]\nx\n[GENERAL={B() || C()}\nscene=y\n",
            Split::Commas,
        );
        let models: Vec<_> = doc.sections("MODELS").collect();
        assert_eq!(models.len(), 2);
        assert_eq!(
            models[0].condition.as_deref(),
            Some("IsCurrentTime(\"202p\") && GetEgoCurrentLocationCount() < 1")
        );
        assert_eq!(models[1].condition, None);
        assert_eq!(
            doc.section("ODD").unwrap().condition.as_deref(),
            Some("A()")
        );
        let general = doc.section("GENERAL").unwrap();
        assert_eq!(general.condition.as_deref(), Some("B() || C()"));
        assert_eq!(general.lines[0].get("scene"), Some("y"));
    }

    #[test]
    fn comments_line_endings_and_nuls() {
        let doc = Document::parse(
            b"[A]\r\r\nkey=value // note\r\n/* gone\r\n still gone */after=1\r\nurl=http://x\0\0",
            Split::Commas,
        );
        let lines = &doc.sections[0].lines;
        assert_eq!(lines[0].get("key"), Some("value"));
        assert_eq!(lines[1].get("after"), Some("1"));
        assert_eq!(lines[2].get("url"), Some("http://x"));
        assert_eq!(lines.len(), 3);
    }

    #[test]
    fn latin1_survives() {
        let doc = Document::parse(b"[T]\ncaption=caf\xe9\n", Split::Whole);
        assert_eq!(doc.sections[0].lines[0].get("caption"), Some("caf\u{e9}"));
    }

    #[test]
    fn animation_lines_keep_their_text() {
        let doc = Document::parse(
            b"[HEADER]\n150\n[GK3]\n1\n0,SpeakerCaption, 149, GABRIEL,Starring: me, you\n",
            Split::Whole,
        );
        let gk3 = doc.section("GK3").unwrap();
        assert_eq!(
            gk3.lines[1].text,
            "0,SpeakerCaption, 149, GABRIEL,Starring: me, you"
        );
    }
}
