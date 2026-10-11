//! Parsers for Gabriel Knight 3's asset formats.
//!
//! Each module turns the bytes of one kind of asset into plain data. Nothing
//! here knows about archives, rendering or the game; see `gk3-barn` for
//! getting the bytes.

pub mod bsp;
pub mod image;
pub mod ini;
pub mod lightmap;
pub mod model;
pub mod png;
pub mod reader;
pub mod wav;

use std::fmt;

/// Why an asset could not be parsed.
#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    /// The data ended before a field that should be there.
    Truncated { offset: usize, wanted: usize },
    /// The data is inconsistent with the format.
    Format(String),
    /// A valid variant of the format that is not supported.
    Unsupported(String),
}

impl Error {
    pub(crate) fn truncated(offset: usize, wanted: usize) -> Self {
        Self::Truncated { offset, wanted }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated { offset, wanted } => {
                write!(
                    f,
                    "data ends at offset {offset}, wanted {wanted} more bytes"
                )
            }
            Self::Format(message) => f.write_str(message),
            Self::Unsupported(message) => write!(f, "unsupported: {message}"),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

/// What an asset is, decided by its leading bytes rather than its name.
///
/// Names are only hints: most dialogue audio carries a three-character code
/// where `.WAV` would be, and a few retail files are mislabelled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Kind {
    /// RIFF/WAVE audio, usually MP3 inside.
    Wave,
    /// RIFF/AVI video.
    Avi,
    /// GK3's own 16-bit bitmap (`61nM`).
    Gk3Bitmap,
    /// Windows bitmap.
    WindowsBitmap,
    Png,
    /// Model geometry (`MODL`).
    Model,
    /// Actor vertex animation (`ACTH`).
    ActorAnimation,
    /// Lightmap set (`MULT`).
    Lightmap,
    /// BSP room geometry (`SCEN`).
    Scene,
    /// Compiled Sheep script (`GK3Sheep`).
    Sheep,
    /// Bitmap font definition.
    Font,
    /// SIDNEY document (HTML).
    Html,
    /// OLE compound document: the original team's design documents.
    OleDocument,
    /// Windows executable or DLL left in the archives.
    Executable,
    Zip,
    /// Latin-1 text: scene, action, animation and other script-like files.
    Text,
    Unknown,
}

impl Kind {
    pub fn of(bytes: &[u8]) -> Self {
        let starts = |prefix: &[u8]| bytes.starts_with(prefix);
        if starts(b"RIFF") && bytes.len() >= 12 {
            return match &bytes[8..12] {
                b"WAVE" => Self::Wave,
                b"AVI " => Self::Avi,
                _ => Self::Unknown,
            };
        }
        match bytes.get(..4) {
            Some(b"61nM") => return Self::Gk3Bitmap,
            Some(b"LDOM") => return Self::Model,
            Some(b"HTCA") => return Self::ActorAnimation,
            Some(b"TLUM") => return Self::Lightmap,
            Some(b"NECS") => return Self::Scene,
            Some(b"GK3S") => return Self::Sheep,
            Some(b"Font") | Some(b"Bitm") => return Self::Font,
            Some(b"PK\x03\x04") => return Self::Zip,
            Some([0xD0, 0xCF, 0x11, 0xE0]) => return Self::OleDocument,
            Some([0x89, b'P', b'N', b'G']) => return Self::Png,
            _ => {}
        }
        if starts(b"BM") {
            return Self::WindowsBitmap;
        }
        if starts(b"MZ") {
            return Self::Executable;
        }
        let head = &bytes[..bytes.len().min(64)];
        if head
            .windows(5)
            .any(|w| w.eq_ignore_ascii_case(b"<html") || w.eq_ignore_ascii_case(b"<!doc"))
            || starts(b"<htm")
        {
            return Self::Html;
        }
        if is_text(bytes) {
            return Self::Text;
        }
        Self::Unknown
    }
}

/// Text assets have no signature. They are Latin-1 and may end in NULs, but
/// a NUL in the middle, or many control bytes, means binary.
fn is_text(bytes: &[u8]) -> bool {
    let content = match bytes.iter().rposition(|&b| b != 0) {
        Some(last) => &bytes[..=last],
        None => return false,
    };
    if content.contains(&0) {
        return false;
    }
    let control = content
        .iter()
        .filter(|&&b| b < 0x20 && !matches!(b, b'\t' | b'\n' | b'\r' | 0x0C | 0x1A))
        .count();
    control * 100 <= content.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_by_content_not_name() {
        assert_eq!(Kind::of(b"RIFF\0\0\0\0WAVEfmt "), Kind::Wave);
        assert_eq!(Kind::of(b"RIFF\0\0\0\0AVI LIST"), Kind::Avi);
        assert_eq!(Kind::of(b"61nM\x01\0\x01\0\0\0"), Kind::Gk3Bitmap);
        assert_eq!(Kind::of(b"BM\0\0"), Kind::WindowsBitmap);
        assert_eq!(Kind::of(b"LDOM\x01\x09"), Kind::Model);
        assert_eq!(Kind::of(b"NECS\x02\x02"), Kind::Scene);
        assert_eq!(Kind::of(b"[GENERAL]\r\nfloor=r25\r\n\0\0"), Kind::Text);
        assert_eq!(Kind::of(b"abc\0def"), Kind::Unknown);
        assert_eq!(Kind::of(&[0u8; 48]), Kind::Unknown);
    }
}
