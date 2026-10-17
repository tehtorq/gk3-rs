//! `gk3 scenes`: every place the story can be, for jumping straight there.
//! Each timeblock in story order, with the locations it has a file for
//! (`R25110A.SIF`), and the names the game gives both (`ESTRINGS.TXT`).
//! Locations with no scene at that time (MA2 on some days) are left out.
//! A few locations have only a general file: the temple is listed at the
//! last timeblock, when the story reaches it, and the rest (side rooms
//! the story only passes through) at the end as other places.
//! One tab-separated line each: timeblock, its name, location, its name.

use std::collections::{BTreeSet, HashMap};
use std::process::ExitCode;

use gk3_barn::Library;

pub fn run(lib: &Library) -> Result<ExitCode, String> {
    for line in lines(lib) {
        println!("{}", line.join("\t"));
    }
    Ok(ExitCode::SUCCESS)
}

fn lines(lib: &Library) -> Vec<[String; 4]> {
    let strings = lib
        .read("ESTRINGS.TXT")
        .map(|b| gk3_game::state::latin1(&b))
        .unwrap_or_default();
    let (times, names) = parse_strings(&strings);
    let mut at: HashMap<String, BTreeSet<String>> = HashMap::new();
    let mut general = BTreeSet::new();
    for (_, entry) in lib.assets() {
        if let Some((location, time)) = location_time(&entry.name) {
            at.entry(time).or_default().insert(location);
        } else if let Some(stem) = entry.name.to_ascii_uppercase().strip_suffix(".SIF")
            && stem.len() == 3
        {
            general.insert(stem.to_string());
        }
    }
    let timed: BTreeSet<String> = at.values().flatten().cloned().collect();
    let mut other: Vec<String> = general.difference(&timed).cloned().collect();
    if let Some((last, _)) = times.last() {
        let (temple, rest) = other.into_iter().partition(|l| l.starts_with("TE"));
        at.entry(last.clone())
            .or_default()
            .extend::<Vec<String>>(temple);
        other = rest;
    }
    let mut catalog = crate::room::Catalog::new(lib);
    let mut out = Vec::new();
    for (time, label) in &times {
        catalog.time = Some(time.clone());
        for location in at.remove(time).unwrap_or_default() {
            if catalog.resolve(lib, &location).is_none() {
                continue;
            }
            let name = names
                .get(&location)
                .cloned()
                .unwrap_or_else(|| location.clone());
            out.push([time.clone(), label.clone(), location, name]);
        }
    }
    // The rest, as at the last full afternoon.
    if let Some((time, label)) = times.iter().rev().nth(2) {
        catalog.time = Some(time.clone());
        for location in other {
            if catalog.resolve(lib, &location).is_none() {
                continue;
            }
            let name = names
                .get(&location)
                .cloned()
                .unwrap_or_else(|| location.clone());
            let group = format!("Other places (as at {label})");
            out.push([time.clone(), group, location, name]);
        }
    }
    out
}

/// `R25110A.SIF` is R25 at 110A; the general `R25.SIF` is no timeblock.
fn location_time(asset: &str) -> Option<(String, String)> {
    let stem = asset.to_ascii_uppercase();
    let stem = stem.strip_suffix(".SIF")?;
    if stem.len() != 7 || !stem.is_ascii() {
        return None;
    }
    let (location, time) = stem.split_at(3);
    let b = time.as_bytes();
    let is_time = b[..3].iter().all(u8::is_ascii_digit) && matches!(b[3], b'A' | b'P');
    is_time.then(|| (location.to_string(), time.to_string()))
}

/// The timeblocks in the order the file lists them (`Day110a = Day 1,
/// 10am - 12pm`), and the locations' names (`loc_r25 = Gabriel's Room`,
/// else the driving map's `dm_` name).
fn parse_strings(text: &str) -> (Vec<(String, String)>, HashMap<String, String>) {
    let mut times = Vec::new();
    let mut names = HashMap::new();
    let mut map_names = HashMap::new();
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let (key, value) = (key.trim(), value.trim().to_string());
        let lower = key.to_ascii_lowercase();
        if let Some(time) = lower.strip_prefix("day")
            && location_time(&format!("XXX{time}.SIF")).is_some()
        {
            times.push((time.to_ascii_uppercase(), value));
        } else if let Some(location) = lower.strip_prefix("loc_") {
            names.insert(location.to_ascii_uppercase(), value);
        } else if let Some(location) = lower.strip_prefix("dm_") {
            map_names.insert(location.to_ascii_uppercase(), value);
        }
    }
    for (location, name) in map_names {
        names.entry(location).or_insert(name);
    }
    (times, names)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn location_files_name_a_place_and_a_time() {
        assert_eq!(
            location_time("R25110A.SIF"),
            Some(("R25".into(), "110A".into()))
        );
        assert_eq!(location_time("r25.sif"), None);
        assert_eq!(location_time("R25110A.NVC"), None);
        assert_eq!(location_time("GRI_ALL.SIF"), None);
    }

    #[test]
    fn strings_give_times_in_order_and_place_names() {
        let (times, names) = parse_strings(
            "Day               = <Game Not Started>\n\
             Day110a           = Day 1, 10am - 12pm\n\
             Day202a           = Day 3, 2am \n\
             loc_r25   = Gabriel's Room\n\
             dm_r25    = Not this one\n\
             dm_arm    = Armchair of the Devil\n",
        );
        assert_eq!(
            times,
            vec![
                ("110A".to_string(), "Day 1, 10am - 12pm".to_string()),
                ("202A".to_string(), "Day 3, 2am".to_string()),
            ]
        );
        assert_eq!(names["R25"], "Gabriel's Room");
        assert_eq!(names["ARM"], "Armchair of the Devil");
    }
}
