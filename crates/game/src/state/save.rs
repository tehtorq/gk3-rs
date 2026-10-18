//! Saved games: the story's state as text.
//!
//! One record a line, its fields separated by tabs: the kind of record, then
//! its values. Names in the story are single words, so no escaping is
//! needed, but tabs keep it safe for any that are not. Lines this module does
//! not know (the engine's own, such as where the player stands) are handed
//! back to the caller, so it can add its own records without a new format.

use super::GameState;

/// The first line of every save.
const MAGIC: &str = "gk3-rs save";
const VERSION: u32 = 1;

impl GameState {
    /// The state as save-file text, followed by the caller's own records.
    pub fn to_save(&self, extra: &[Vec<String>]) -> String {
        let mut out = format!("{MAGIC}\t{VERSION}\n");
        let mut line = |fields: &[&str]| {
            out.push_str(&fields.join("\t"));
            out.push('\n');
        };
        line(&["timeblock", &self.timeblock, &self.last_timeblock]);
        line(&["location", &self.location, &self.last_location]);
        line(&["ego", &self.ego]);
        line(&[
            "random",
            &self.rng.to_string(),
            &self.random_draws.to_string(),
        ]);
        for flag in &self.flags {
            line(&["flag", flag]);
        }
        for (name, value) in &self.variables {
            line(&["variable", name, &value.to_string()]);
        }
        for ((who, noun, verb), n) in &self.noun_verbs {
            line(&["noun_verb", who, noun, verb, &n.to_string()]);
        }
        for (noun, verb, case) in &self.played_topics {
            line(&["played_topic", noun, verb, case]);
        }
        for (noun, n) in &self.chats {
            line(&["chat", noun, &n.to_string()]);
        }
        for event in &self.score_events {
            line(&["score", event]);
        }
        for (who, items) in &self.inventory {
            for item in items {
                line(&["item", who, item]);
            }
        }
        for (who, item) in &self.active_item {
            line(&["active_item", who, item]);
        }
        for (actor, location) in &self.actor_locations {
            line(&["actor_location", actor, location]);
        }
        for ((actor, location, time), n) in &self.location_counts {
            line(&["location_count", actor, location, time, &n.to_string()]);
        }
        for id in &self.sidney_files {
            line(&["sidney_file", &id.to_string()]);
        }
        for id in &self.sidney_analyzed {
            line(&["sidney_analyzed", &id.to_string()]);
        }
        for email in &self.emails_received {
            line(&["email", email]);
        }
        for email in &self.emails_read {
            line(&["email_read", email]);
        }
        for (suspect, files) in &self.suspect_links {
            for file in files {
                line(&["suspect_link", &suspect.to_string(), &file.to_string()]);
            }
        }
        for record in extra {
            let fields: Vec<&str> = record.iter().map(String::as_str).collect();
            line(&fields);
        }
        out
    }

    /// Reads a save, returning the state and the records it did not know.
    pub fn from_save(text: &str) -> Result<(Self, Vec<Vec<String>>), String> {
        let mut lines = text.lines();
        let header = lines.next().unwrap_or("");
        let version = header
            .strip_prefix(MAGIC)
            .and_then(|v| v.trim().parse::<u32>().ok())
            .ok_or("not a gk3-rs save")?;
        if version > VERSION {
            return Err(format!(
                "save version {version} is newer than this build reads ({VERSION})"
            ));
        }
        let mut state = GameState::new("110A", "");
        let mut extra = Vec::new();
        let mut topics = Vec::new();
        for (n, text) in lines.enumerate() {
            if text.trim().is_empty() {
                continue;
            }
            let f: Vec<&str> = text.split('\t').collect();
            let bad = || format!("save line {}: cannot read {text:?}", n + 2);
            let s = |i: usize| f.get(i).map(|s| s.to_string()).ok_or_else(bad);
            let int = |i: usize| f.get(i).and_then(|v| v.parse::<i32>().ok()).ok_or_else(bad);
            match f[0] {
                "timeblock" => (state.timeblock, state.last_timeblock) = (s(1)?, s(2)?),
                "location" => (state.location, state.last_location) = (s(1)?, s(2)?),
                "ego" => state.ego = s(1)?,
                "random" => {
                    state.rng = f.get(1).and_then(|v| v.parse().ok()).ok_or_else(bad)?;
                    state.random_draws = f.get(2).and_then(|v| v.parse().ok()).ok_or_else(bad)?;
                }
                "flag" => {
                    state.flags.insert(s(1)?);
                }
                "variable" => {
                    state.variables.insert(s(1)?, int(2)?);
                }
                "noun_verb" => {
                    state.noun_verbs.insert((s(1)?, s(2)?, s(3)?), int(4)?);
                }
                // Older saves kept topics apart, for whoever was playing.
                "topic" => topics.push((s(1)?, s(2)?, int(3)?)),
                "played_topic" => {
                    state.played_topics.insert((s(1)?, s(2)?, s(3)?));
                }
                "chat" => {
                    state.chats.insert(s(1)?, int(2)?);
                }
                "score" => {
                    state.score_events.insert(s(1)?);
                }
                "item" => {
                    state.inventory.entry(s(1)?).or_default().insert(s(2)?);
                }
                "active_item" => {
                    state.active_item.insert(s(1)?, s(2)?);
                }
                "actor_location" => {
                    state.actor_locations.insert(s(1)?, s(2)?);
                }
                "location_count" => {
                    state.location_counts.insert((s(1)?, s(2)?, s(3)?), int(4)?);
                }
                "sidney_file" => {
                    state.sidney_files.insert(int(1)? as u32);
                }
                "sidney_analyzed" => {
                    state.sidney_analyzed.insert(int(1)? as u32);
                }
                "email" => state.emails_received.push(s(1)?),
                "suspect_link" => {
                    let suspect = int(1)? as u32;
                    state
                        .suspect_links
                        .entry(suspect)
                        .or_default()
                        .push(int(2)? as u32);
                }
                "email_read" => {
                    state.emails_read.insert(s(1)?);
                }
                _ => extra.push(f.iter().map(|s| s.to_string()).collect()),
            }
        }
        for (noun, verb, n) in topics {
            let ego = state.ego.clone();
            state.set_noun_verb(&ego, &noun, &verb, n);
        }
        Ok((state, extra))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_everything() {
        let mut state = GameState::new("207A", "LBY");
        state.last_timeblock = "112P".into();
        state.ego = "GRACE".into();
        state.flags.insert("METBUCHELLI".into());
        state.variables.insert("SIDSCANNER".into(), -3);
        state
            .noun_verbs
            .insert(("GRACE".into(), "BELL".into(), "RING".into()), 2);
        state
            .noun_verbs
            .insert(("GRACE".into(), "JEAN".into(), "T_RENNES".into()), 1);
        state
            .played_topics
            .insert(("JEAN".into(), "T_RENNES".into(), "ALL".into()));
        state.chats.insert("JEAN".into(), 4);
        state.score_events.insert("E_GRACE_RING".into());
        state
            .inventory
            .entry("GRACE".into())
            .or_default()
            .insert("SKETCHPAD".into());
        state.active_item.insert("GRACE".into(), "SKETCHPAD".into());
        state.actor_locations.insert("JEAN".into(), "LBY".into());
        state
            .location_counts
            .insert(("GRACE".into(), "LBY".into(), "207A".into()), 1);
        state.random(1, 10);
        state.sidney_files.extend([19, 3]);
        state.sidney_analyzed.insert(19);
        state
            .emails_received
            .extend(["Email1".to_string(), "Email2".to_string()]);
        state.emails_read.insert("Email1".into());
        state.suspect_links.insert(1, vec![32, 1]);

        let extra = vec![vec!["ego_at".to_string(), "1.5".into(), "0".into()]];
        let text = state.to_save(&extra);
        let (back, rest) = GameState::from_save(&text).unwrap();
        assert_eq!(back, state);
        assert_eq!(back.hash(), state.hash());
        assert_eq!(rest, extra);
    }

    #[test]
    fn refuses_what_it_cannot_read() {
        assert!(GameState::from_save("hello").is_err());
        assert!(GameState::from_save("gk3-rs save\t99\n").is_err());
        assert!(GameState::from_save("gk3-rs save\t1\nvariable\tX\tnot-a-number\n").is_err());
    }
}
