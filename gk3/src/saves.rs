//! Where saved games live: `$XDG_DATA_HOME/gk3-rs/saves`, or
//! `~/.local/share/gk3-rs/saves` without it.

use std::path::PathBuf;

/// The quick save slot's name.
pub const QUICK: &str = "quick";

pub fn dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_DATA_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))?;
    Some(base.join("gk3-rs").join("saves"))
}

/// A save by name (`quick` is `quick.sav` in the saves directory), or a
/// path to a file if it looks like one.
pub fn path(name: &str) -> Option<PathBuf> {
    if name.contains('/') || name.ends_with(".sav") {
        return Some(PathBuf::from(name));
    }
    Some(dir()?.join(format!("{name}.sav")))
}

pub fn write(name: &str, text: &str) -> Result<PathBuf, String> {
    let path = path(name).ok_or("no home directory to save in")?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    // Write beside it and rename, so a crash never leaves half a save.
    let partial = path.with_extension("sav.partial");
    std::fs::write(&partial, text).map_err(|e| format!("{}: {e}", partial.display()))?;
    std::fs::rename(&partial, &path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path)
}

pub fn read(name: &str) -> Result<String, String> {
    let path = path(name).ok_or("no home directory to load from")?;
    std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))
}
