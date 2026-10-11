//! The story's state: everything that decides what the player may do next.
//!
//! Kept in ordered maps so it can be hashed the same way every run; two runs
//! of the same inputs must agree exactly, or a difference in behaviour cannot
//! be told from noise. Names are case-insensitive throughout, so keys are
//! stored upper-cased.

use std::collections::{BTreeMap, BTreeSet};

mod save;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameState {
    /// Day and hour, as the game writes it: `110A` is day 1, 10am.
    pub timeblock: String,
    pub last_timeblock: String,
    pub location: String,
    pub last_location: String,
    /// Who the player is: `GABRIEL` or `GRACE`.
    pub ego: String,
    pub flags: BTreeSet<String>,
    pub variables: BTreeMap<String, i32>,
    /// By character, noun and verb: what has been done to what, by whom.
    pub noun_verbs: BTreeMap<(String, String, String), i32>,
    pub topics: BTreeMap<(String, String), i32>,
    /// Topics already discussed, by noun, verb and the case they came under:
    /// a topic is raised once per case.
    pub played_topics: BTreeSet<(String, String, String)>,
    pub chats: BTreeMap<String, i32>,
    /// Score events achieved; each counts once.
    pub score_events: BTreeSet<String>,
    /// Items carried, by character.
    pub inventory: BTreeMap<String, BTreeSet<String>>,
    /// The item in each character's hand.
    pub active_item: BTreeMap<String, String>,
    /// The inventory screen is up: the `_INV` cases hold only then.
    pub inventory_shown: bool,
    pub actor_locations: BTreeMap<String, String>,
    /// Visits by actor, location and timeblock.
    pub location_counts: BTreeMap<(String, String, String), i32>,
    rng: u64,
    pub random_draws: u64,
}

impl Default for GameState {
    fn default() -> Self {
        Self::new("110A", "R25")
    }
}

/// Latin-1 bytes as text.
pub fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| b as char).collect()
}

pub fn key(s: &str) -> String {
    s.trim().to_ascii_uppercase()
}

/// GK3's two player characters go by several names in the scripts.
pub fn character(name: &str) -> String {
    match key(name).as_str() {
        "GABE" | "GAB" | "GABRIEL" => "GABRIEL".into(),
        "GRACE" | "GRA" => "GRACE".into(),
        other => other.into(),
    }
}

impl GameState {
    pub fn new(timeblock: &str, location: &str) -> Self {
        Self {
            timeblock: key(timeblock),
            last_timeblock: String::new(),
            location: key(location),
            last_location: String::new(),
            ego: "GABRIEL".into(),
            flags: BTreeSet::new(),
            variables: BTreeMap::new(),
            noun_verbs: BTreeMap::new(),
            topics: BTreeMap::new(),
            played_topics: BTreeSet::new(),
            chats: BTreeMap::new(),
            score_events: BTreeSet::new(),
            inventory: BTreeMap::new(),
            active_item: BTreeMap::new(),
            inventory_shown: false,
            actor_locations: BTreeMap::new(),
            location_counts: BTreeMap::new(),
            rng: 0x6B33_0001,
            random_draws: 0,
        }
    }

    pub fn noun_verb(&self, who: &str, noun: &str, verb: &str) -> i32 {
        *self
            .noun_verbs
            .get(&(character(who), key(noun), key(verb)))
            .unwrap_or(&0)
    }

    pub fn set_noun_verb(&mut self, who: &str, noun: &str, verb: &str, value: i32) {
        self.noun_verbs
            .insert((character(who), key(noun), key(verb)), value);
    }

    pub fn has_item(&self, who: &str, item: &str) -> bool {
        self.inventory
            .get(&character(who))
            .is_some_and(|items| items.contains(&key(item)))
    }

    pub fn give_item(&mut self, who: &str, item: &str) {
        self.inventory
            .entry(character(who))
            .or_default()
            .insert(key(item));
    }

    pub fn take_item(&mut self, who: &str, item: &str) {
        let who = character(who);
        if let Some(items) = self.inventory.get_mut(&who) {
            items.remove(&key(item));
        }
        if self.active_item.get(&who) == Some(&key(item)) {
            self.active_item.remove(&who);
        }
    }

    /// Moves the story to a new place, counting the visit.
    pub fn enter(&mut self, location: &str) {
        self.last_location = std::mem::replace(&mut self.location, key(location));
        let visit = (
            self.ego.clone(),
            self.location.clone(),
            self.timeblock.clone(),
        );
        *self.location_counts.entry(visit).or_default() += 1;
        self.actor_locations
            .insert(self.ego.clone(), self.location.clone());
    }

    pub fn location_count(&self, who: &str, location: &str) -> i32 {
        *self
            .location_counts
            .get(&(character(who), key(location), self.timeblock.clone()))
            .unwrap_or(&0)
    }

    pub fn ever_visited(&self, who: &str, location: &str) -> bool {
        let (who, location) = (character(who), key(location));
        self.location_counts
            .keys()
            .any(|(a, l, _)| *a == who && *l == location)
    }

    /// A number in `low..=high`, from the story's own generator. Draws are
    /// counted, because two runs that drew a different number of times will
    /// disagree about everything random afterwards.
    pub fn random(&mut self, low: i32, high: i32) -> i32 {
        let (low, high) = if low <= high {
            (low, high)
        } else {
            (high, low)
        };
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        self.random_draws += 1;
        let span = (high as i64 - low as i64 + 1) as u64;
        (low as i64 + (self.rng % span) as i64) as i32
    }

    /// A stable fingerprint of the whole state (FNV-1a over a canonical
    /// rendering, maps in key order).
    pub fn hash(&self) -> u64 {
        let text = format!(
            "{}|{}|{}|{}|{}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{}|{}",
            self.timeblock,
            self.last_timeblock,
            self.location,
            self.last_location,
            self.ego,
            self.flags,
            self.variables,
            self.noun_verbs,
            self.topics,
            self.played_topics,
            self.chats,
            self.score_events,
            self.inventory,
            self.active_item,
            self.actor_locations,
            self.location_counts,
            self.rng,
            self.random_draws
        );
        text.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
            (h ^ b as u64).wrapping_mul(0x0100_0000_01b3)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_case_insensitive_and_characters_alias() {
        let mut s = GameState::default();
        s.set_noun_verb("Gabe", "door", "open", 2);
        assert_eq!(s.noun_verb("GABRIEL", "DOOR", "Open"), 2);
        assert_eq!(s.noun_verb("Grace", "door", "open"), 0);
        s.give_item("gabriel", "Wallet");
        assert!(s.has_item("GABE", "WALLET"));
    }

    #[test]
    fn visits_count_per_timeblock() {
        let mut s = GameState::new("110a", "R25");
        s.enter("LBY");
        s.enter("R25");
        s.enter("LBY");
        assert_eq!(s.location_count("Gabriel", "lby"), 2);
        assert_eq!(s.last_location, "R25");
        s.timeblock = "112P".into();
        assert_eq!(s.location_count("Gabriel", "LBY"), 0);
        assert!(s.ever_visited("Gabriel", "LBY"));
    }

    #[test]
    fn random_numbers_are_in_range_and_repeatable() {
        let mut a = GameState::default();
        let mut b = GameState::default();
        for _ in 0..100 {
            let x = a.random(1, 3);
            assert!((1..=3).contains(&x));
            assert_eq!(x, b.random(1, 3));
        }
        assert_eq!(a.hash(), b.hash());
        a.flags.insert("X".into());
        assert_ne!(a.hash(), b.hash());
    }
}
