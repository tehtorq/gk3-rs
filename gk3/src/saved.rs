//! Helpers for saved games.
//!
//! Some of what is saved names things from the game's own tables by
//! `&'static str` (a flag, a sound). Read back, each name is made to last
//! once and shared after, so a restore leaks at most one copy of each
//! distinct name.

use std::collections::HashSet;
use std::sync::Mutex;

use serde::{Deserialize, Deserializer};

static NAMES: Mutex<Option<HashSet<&'static str>>> = Mutex::new(None);

/// A name, kept for good.
pub fn intern(name: &str) -> &'static str {
    let mut names = NAMES.lock().unwrap_or_else(|e| e.into_inner());
    let names = names.get_or_insert_with(HashSet::new);
    if let Some(&kept) = names.get(name) {
        return kept;
    }
    let kept: &'static str = Box::leak(name.to_string().into_boxed_str());
    names.insert(kept);
    kept
}

pub fn static_str<'de, D: Deserializer<'de>>(d: D) -> Result<&'static str, D::Error> {
    Ok(intern(&String::deserialize(d)?))
}

pub fn option_static_str<'de, D: Deserializer<'de>>(
    d: D,
) -> Result<Option<&'static str>, D::Error> {
    Ok(Option::<String>::deserialize(d)?.map(|s| intern(&s)))
}

pub fn static_pairs<'de, D: Deserializer<'de>>(
    d: D,
) -> Result<Vec<(&'static str, String)>, D::Error> {
    Ok(Vec::<(String, String)>::deserialize(d)?
        .into_iter()
        .map(|(k, v)| (intern(&k), v))
        .collect())
}

/// A still picture by name, and the colour it leaves see-through.
pub fn option_static_keyed<'de, D: Deserializer<'de>>(
    d: D,
) -> Result<Option<(&'static str, [u8; 3])>, D::Error> {
    Ok(Option::<(String, [u8; 3])>::deserialize(d)?.map(|(s, k)| (intern(&s), k)))
}

/// A name from the game's own tables. Written as an alias so that serde
/// reads it with the helpers above rather than borrowing from the input.
pub type Name = &'static str;
