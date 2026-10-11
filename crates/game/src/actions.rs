//! What the player can do: action files (`.NVC`).
//!
//! Each rule is `NOUN, VERB, CASE, [approach=…,] [target=…,] script={…}`, and
//! a `[LOGIC]` section defines cases as Sheep expressions. Which files apply
//! is decided by their names, which encode the timeblocks they cover. Rules
//! are kept in priority order, most specific file first; the first rule for
//! a verb whose case holds is the one that runs.

use std::collections::HashMap;

use gk3_formats::ini::{Document, Split};
use gk3_sheep::Host;
use gk3_sheep::Value;
use gk3_sheep::expr::Expr;

use crate::state::{GameState, key};

#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    pub noun: String,
    pub verb: String,
    pub case: String,
    /// How the player gets into position: `WalkTo`, `WalkToSee`, `ANIM`,
    /// `TurnToModel`, `NearModel`, `none`.
    pub approach: Option<String>,
    pub target: Option<String>,
    /// Sheep statements, without their braces.
    pub script: String,
    pub source: String,
}

#[derive(Debug, Clone, Default)]
pub struct ActionSet {
    pub rules: Vec<Rule>,
    logic: HashMap<String, String>,
}

/// A timeblock as day and hour on a 24-hour clock: `110A` is (1, 10).
pub fn parse_timeblock(text: &str) -> Option<(u32, u32)> {
    let t = key(text);
    let (digits, half) = t.split_at(t.len().checked_sub(1)?);
    let day: u32 = digits.get(..1)?.parse().ok()?;
    let hour: u32 = digits.get(1..)?.parse().ok()?;
    let hour = match (half, hour) {
        ("A", 12) => 0,
        ("A", h) => h,
        ("P", 12) => 12,
        ("P", h) => h + 12,
        _ => return None,
    };
    Some((day, hour))
}

/// Whether an action file, by its name, applies at a timeblock. After three
/// letters of location and an optional underscore: `ALL`, days then `ALL`
/// (`23ALL`), a timeblock (`202P`), or a range whose end borrows the
/// start's day (`110A04P`). Names that cannot be read apply nowhere.
pub fn covers(file: &str, time: &str) -> bool {
    let stem = key(file);
    let stem = stem.trim_end_matches(".NVC");
    let Some(rest) = stem.get(3..) else {
        return false;
    };
    let rest = rest.trim_start_matches('_');
    let Some((day, hour)) = parse_timeblock(time) else {
        return false;
    };
    if rest == "ALL" {
        return true;
    }
    if let Some(days) = rest.strip_suffix("ALL") {
        return !days.is_empty()
            && days.chars().all(|c| c.is_ascii_digit())
            && days.contains(char::from_digit(day, 10).unwrap_or('?'));
    }
    if rest.len() == 4 {
        return parse_timeblock(rest) == Some((day, hour));
    }
    if rest.len() == 7 {
        let (Some(start), Some(end)) = (
            parse_timeblock(&rest[..4]),
            parse_timeblock(&format!("{}{}", &rest[..1], &rest[4..])),
        ) else {
            return false;
        };
        return (day, hour) >= start && (day, hour) <= end;
    }
    false
}

impl ActionSet {
    /// Adds a file's rules after those already held, so add the most
    /// specific file first.
    pub fn add_file(&mut self, bytes: &[u8], source: &str) {
        let doc = Document::parse(&join_scripts(bytes), Split::Commas);
        for section in &doc.sections {
            if section.name.eq_ignore_ascii_case("LOGIC") {
                for line in &section.lines {
                    for pair in &line.pairs {
                        self.logic
                            .entry(key(&pair.key))
                            .or_insert_with(|| pair.value.clone());
                    }
                }
                continue;
            }
            for line in &section.lines {
                let flags: Vec<&str> = line
                    .pairs
                    .iter()
                    .take_while(|p| p.is_flag())
                    .map(|p| p.key.as_str())
                    .collect();
                let (Some(script), [noun, verb, case, ..]) = (line.get("script"), &flags[..])
                else {
                    continue;
                };
                let script = script.trim();
                let script = script
                    .strip_prefix('{')
                    .and_then(|s| s.strip_suffix('}'))
                    .unwrap_or(script);
                self.rules.push(Rule {
                    noun: key(noun),
                    verb: key(verb),
                    case: key(case),
                    approach: line.get("approach").map(str::to_string),
                    target: line.get("target").map(str::to_string),
                    script: script.trim().to_string(),
                    source: source.to_string(),
                });
            }
        }
    }

    /// Whether a rule's case holds now. Never changes the story.
    pub fn holds(&self, rule: &Rule, host: &mut dyn Host, state: &GameState) -> bool {
        let count = state.noun_verb(&state.ego, &rule.noun, &rule.verb);
        match rule.case.as_str() {
            "ALL" | "DEFAULT" | "TIME_BLOCK" | "TIME_BLOCK_OVERRIDE" => true,
            "GABE_ALL" => state.ego == "GABRIEL",
            "GRACE_ALL" => state.ego == "GRACE",
            "NOT_GABE_ALL" => state.ego != "GABRIEL",
            "NOT_GRACE_ALL" => state.ego != "GRACE",
            "1ST_TIME" => count == 0,
            "2CD_TIME" | "2ND_TIME" => count == 1,
            "3RD_TIME" => count == 2,
            "OTR_TIME" => count > 0,
            "DIALOGUE_TOPICS_LEFT" => self.topics_left(&rule.noun, host, state),
            "NOT_DIALOGUE_TOPICS_LEFT" => !self.topics_left(&rule.noun, host, state),
            "EGG" => false,
            case => match self.logic.get(case) {
                Some(text) => {
                    let text = text
                        .trim()
                        .trim_start_matches('{')
                        .trim_end_matches('}')
                        .trim()
                        .trim_end_matches(';');
                    let (noun, verb) = (rule.noun.clone(), rule.verb.clone());
                    let resolve = move |name: &str| match name {
                        n if n.eq_ignore_ascii_case("n$") => Some(Value::from(noun.as_str())),
                        v if v.eq_ignore_ascii_case("v$") => Some(Value::from(verb.as_str())),
                        _ => None,
                    };
                    Expr::parse(text)
                        .map(|e| e.eval(host, &resolve).truthy())
                        .unwrap_or(false)
                }
                None => false,
            },
        }
    }

    fn topics_left(&self, noun: &str, host: &mut dyn Host, state: &GameState) -> bool {
        self.rules
            .iter()
            .filter(|r| {
                r.noun == noun && r.verb.starts_with("T_") && !r.case.contains("TOPICS_LEFT")
            })
            .any(|r| self.holds(r, host, state))
    }

    /// What can be done to a noun now: the first rule per verb whose case
    /// holds, in priority order.
    pub fn available(&self, noun: &str, host: &mut dyn Host, state: &GameState) -> Vec<&Rule> {
        let noun = key(noun);
        let mut out: Vec<&Rule> = Vec::new();
        for rule in self.rules.iter().filter(|r| r.noun == noun) {
            if out.iter().any(|r| r.verb == rule.verb) {
                continue;
            }
            if self.holds(rule, host, state) {
                out.push(rule);
            }
        }
        out
    }

    pub fn find(
        &self,
        noun: &str,
        verb: &str,
        host: &mut dyn Host,
        state: &GameState,
    ) -> Option<&Rule> {
        let verb = key(verb);
        self.available(noun, host, state)
            .into_iter()
            .find(|r| r.verb == verb)
    }
}

/// A script may run over several lines; joins them so each rule is one line.
/// Some retail rules never close their script's brace, which the original's
/// line-by-line reader never noticed: an open script ends at the next line
/// that starts a rule (`NAME,`) or a section, rather than swallowing them.
fn join_scripts(bytes: &[u8]) -> Vec<u8> {
    let text = crate::state::latin1(bytes);
    let starts_rule = |line: &str| {
        let t = line.trim_start();
        t.starts_with('[')
            || t.split_once(',').is_some_and(|(head, _)| {
                let head = head.trim();
                !head.is_empty() && head.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            })
    };
    let depth_of = |line: &str| {
        let (mut depth, mut quoted) = (0i32, false);
        for c in line.chars() {
            match c {
                '"' => quoted = !quoted,
                '{' if !quoted => depth += 1,
                '}' if !quoted => depth -= 1,
                _ => {}
            }
        }
        depth
    };
    let mut out = String::with_capacity(text.len());
    let mut depth = 0;
    for line in text.split('\n') {
        let line = line.trim_end_matches('\r');
        if depth > 0 {
            if starts_rule(line) {
                out.push_str(&"}".repeat(depth as usize));
                out.push('\n');
            } else {
                out.push(' ');
                out.push_str(line);
                depth = (depth + depth_of(line)).max(0);
                if depth == 0 {
                    out.push('\n');
                }
                continue;
            }
        }
        out.push_str(line);
        depth = depth_of(line).max(0);
        if depth == 0 {
            out.push('\n');
        }
    }
    out.bytes().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::GameApi;

    #[test]
    fn scripts_may_span_lines() {
        let mut set = ActionSet::default();
        set.add_file(b"A, LOOK, ALL, script={\r\n  wait Foo(1);\r\n  Bar(\"x\");\r\n}\r\nB, LOOK, ALL, script={Baz();}\r\n", "X.NVC");
        assert_eq!(set.rules.len(), 2);
        assert!(set.rules[0].script.contains("Bar"));
        // An unclosed script ends at the next rule.
        let mut set = ActionSet::default();
        set.add_file(
            b"A, LOOK, ALL, script={wait Foo(1);\r\nB, LOOK, ALL, script={Baz();}\r\n",
            "X.NVC",
        );
        assert_eq!(set.rules.len(), 2);
        assert_eq!(set.rules[0].script, "wait Foo(1);");
    }

    #[test]
    fn timeblocks_and_file_names() {
        assert_eq!(parse_timeblock("110a"), Some((1, 10)));
        assert_eq!(parse_timeblock("112P"), Some((1, 12)));
        assert_eq!(parse_timeblock("306P"), Some((3, 18)));
        assert!(covers("r25_all.nvc", "202P"));
        assert!(covers("R25_1ALL", "110A"));
        assert!(!covers("R25_1ALL", "202P"));
        assert!(covers("GLB_23ALL", "307A"));
        assert!(covers("r25110a.nvc", "110A"));
        assert!(!covers("r25110a.nvc", "112P"));
        assert!(covers("HAL110A04P", "102P"));
        assert!(!covers("HAL110A04P", "106P"));
        assert!(!covers("ch312p06p", "110A"));
    }

    #[test]
    fn rules_cases_and_priority() {
        let mut set = ActionSet::default();
        set.add_file(
            br#"SCENE, ENTER, TIME_BLOCK_START, script={wait CallSheep("r25_all", "DoYawnScene$");}
WINDOW, LOOK, 1ST_TIME, script={wait StartVoiceOver("A", 1);}
WINDOW, LOOK, OTR_TIME, script={wait StartVoiceOver("B", 1);}
WINDOW, OPEN, ALL, approach=WalkTo, target=AT_WIN, script={wait CallSheep("R25_ALL","WINDOW_OPEN");}
[LOGIC]
TIME_BLOCK_START    ={(GetEgoCurrentLocationCount() == 1)}
"#,
            "R25110A.NVC",
        );
        set.add_file(
            b"WINDOW, LOOK, ALL, script={wait StartVoiceOver(\"C\", 1);}\n",
            "GLB_ALL.NVC",
        );
        let scripts = HashMap::new();
        let mut state = GameState::new("110A", "R25");
        let snapshot = state.clone();
        let mut api_state = state.clone();
        let mut api = GameApi::new(&mut api_state, &scripts);

        let look = set.find("window", "look", &mut api, &snapshot).unwrap();
        assert_eq!(look.script, r#"wait StartVoiceOver("A", 1);"#);
        let open = set.find("WINDOW", "OPEN", &mut api, &snapshot).unwrap();
        assert_eq!(
            (open.approach.as_deref(), open.target.as_deref()),
            (Some("WalkTo"), Some("AT_WIN"))
        );
        // Entry runs only on the first visit.
        assert!(set.find("SCENE", "ENTER", &mut api, &snapshot).is_none());
        state.enter("R25");
        let mut api_state = state.clone();
        let mut api = GameApi::new(&mut api_state, &scripts);
        assert!(set.find("SCENE", "ENTER", &mut api, &state).is_some());
        // Second look gets the other line.
        state.set_noun_verb("GABRIEL", "WINDOW", "LOOK", 1);
        let look = set.find("window", "look", &mut api, &state).unwrap();
        assert!(look.script.contains("\"B\""));
    }
}
