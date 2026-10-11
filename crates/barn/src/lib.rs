//! Reader for Gabriel Knight 3's `.brn` asset archives ("Barns").
//!
//! A [`Barn`] is one archive file. A [`Library`] opens every archive in the
//! game's `Data` directory and resolves an asset name to its bytes, following
//! the pointer entries that `core.brn` uses to index the other archives.
//!
//! Layout (all integers little-endian; tags read reversed on disk):
//!
//! ```text
//! header   "GK3!" "Barn" u32 u32 u32 toc_offset ...
//! toc      count, then count × { tag "DDir"|"Data", 16 unknown, header_off, data_off }
//! ddir     header: name[32] (archive holding the assets; empty = this one),
//!                  4 unknown, description[40], 4 unknown, asset_count
//!          data:   asset_count × { size, offset, 5 unknown, compression u8,
//!                                  name_len u8, name, NUL }
//! asset    at data_base + offset; compressed assets start with
//!          { decompressed_size u32, unknown u32 } before `size` bytes
//! ```

mod lzo;

use std::collections::HashMap;
use std::fmt;
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};

pub use lzo::LzoError;

const GAME_TAG: u32 = u32::from_le_bytes(*b"GK3!");
const BARN_TAG: u32 = u32::from_le_bytes(*b"Barn");
// These two are written with their bytes reversed: "riDD" and "ataD" on disk.
const DDIR_TAG: u32 = u32::from_be_bytes(*b"DDir");
const DATA_TAG: u32 = u32::from_be_bytes(*b"Data");

/// Upper bound on a single asset's decompressed size. The largest retail asset
/// is a few tens of megabytes; this rejects corrupt size fields before they
/// turn into a huge allocation.
const MAX_ASSET_SIZE: usize = 512 * 1024 * 1024;

/// Something that went wrong reading an archive or an asset.
#[derive(Debug)]
pub enum Error {
    Io {
        path: PathBuf,
        source: io::Error,
    },
    /// The file is not a Barn, or its structure is inconsistent.
    Format {
        path: PathBuf,
        message: String,
    },
    /// No archive in the library holds an asset with this name.
    NotFound(String),
    /// A pointer entry names an archive that is not in the library.
    MissingArchive {
        asset: String,
        archive: String,
    },
    /// An asset's compressed bytes could not be decompressed.
    Decompress {
        asset: String,
        message: String,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => write!(f, "{}: {source}", path.display()),
            Self::Format { path, message } => write!(f, "{}: {message}", path.display()),
            Self::NotFound(name) => write!(f, "no asset named {name}"),
            Self::MissingArchive { asset, archive } => {
                write!(
                    f,
                    "{asset} is in {archive}, which is not in the data directory"
                )
            }
            Self::Decompress { asset, message } => write!(f, "{asset}: {message}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// How an asset's bytes are stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Compression {
    None,
    Zlib,
    Lzo,
    /// A code no known archive uses. Reading such an asset fails.
    Unknown(u8),
}

impl Compression {
    fn from_code(code: u8) -> Self {
        match code {
            // 3 appears in retail data and behaves exactly like 0.
            0 | 3 => Self::None,
            1 => Self::Zlib,
            2 => Self::Lzo,
            other => Self::Unknown(other),
        }
    }
}

impl fmt::Display for Compression {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => f.write_str("stored"),
            Self::Zlib => f.write_str("zlib"),
            Self::Lzo => f.write_str("lzo"),
            Self::Unknown(code) => write!(f, "unknown({code})"),
        }
    }
}

/// One entry in an archive's directories.
#[derive(Debug, Clone)]
pub struct Entry {
    pub name: String,
    /// Bytes stored in the archive, excluding a compressed asset's 8-byte prefix.
    pub stored_size: u32,
    /// Offset from the archive's data section.
    pub offset: u32,
    pub compression: Compression,
    /// For a pointer entry, the name of the archive that really holds the asset.
    pub pointer: Option<String>,
}

impl Entry {
    pub fn is_pointer(&self) -> bool {
        self.pointer.is_some()
    }
}

/// One `.brn` file.
pub struct Barn {
    path: PathBuf,
    name: String,
    file: File,
    file_len: u64,
    data_base: u64,
    entries: Vec<Entry>,
}

/// Bounds-checked little-endian reads from an in-memory slice.
struct Cursor<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn at(bytes: &'a [u8], pos: usize) -> Self {
        Self { bytes, pos }
    }

    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let slice = self.bytes.get(self.pos..self.pos.checked_add(n)?)?;
        self.pos += n;
        Some(slice)
    }

    fn u8(&mut self) -> Option<u8> {
        self.take(1).map(|b| b[0])
    }

    fn u32(&mut self) -> Option<u32> {
        self.take(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
    }

    /// A fixed-width, NUL-padded Latin-1 string.
    fn padded_str(&mut self, width: usize) -> Option<String> {
        let raw = self.take(width)?;
        let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
        Some(latin1(&raw[..end]))
    }
}

fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| b as char).collect()
}

#[cfg(unix)]
fn read_at(file: &File, buf: &mut [u8], offset: u64) -> io::Result<usize> {
    std::os::unix::fs::FileExt::read_at(file, buf, offset)
}

#[cfg(windows)]
fn read_at(file: &File, buf: &mut [u8], offset: u64) -> io::Result<usize> {
    std::os::windows::fs::FileExt::seek_read(file, buf, offset)
}

/// Fills as much of `buf` as the file has from `offset`, returning the count.
/// Short only at end of file.
fn read_fully_at(file: &File, buf: &mut [u8], offset: u64) -> io::Result<usize> {
    let mut done = 0;
    while done < buf.len() {
        match read_at(file, &mut buf[done..], offset + done as u64) {
            Ok(0) => break,
            Ok(n) => done += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(done)
}

impl Barn {
    /// Opens an archive and reads its directories. Asset bytes are read on demand.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let io_err = |source| Error::Io {
            path: path.clone(),
            source,
        };
        let format_err = |message: String| Error::Format {
            path: path.clone(),
            message,
        };

        let file = File::open(&path).map_err(io_err)?;
        let file_len = file.metadata().map_err(io_err)?.len();

        // The directories sit between the header and the data section in every
        // retail archive, but nothing guarantees it, so read the header first
        // and then exactly the regions the table of contents points at.
        let mut header = [0u8; 24];
        if read_fully_at(&file, &mut header, 0).map_err(io_err)? < header.len() {
            return Err(format_err("too short to be a Barn".into()));
        }
        let mut h = Cursor::at(&header, 0);
        let (game, barn) = (h.u32().unwrap(), h.u32().unwrap());
        if game != GAME_TAG || barn != BARN_TAG {
            return Err(format_err("not a Barn archive (missing GK3!Barn)".into()));
        }
        h.take(12);
        let toc_offset = h.u32().unwrap() as u64;

        let read_region = |offset: u64, len: usize| -> Result<Vec<u8>> {
            if offset.saturating_add(len as u64) > file_len {
                return Err(format_err(format!(
                    "region at {offset} of {len} bytes is past the end of the file"
                )));
            }
            let mut buf = vec![0u8; len];
            read_fully_at(&file, &mut buf, offset).map_err(io_err)?;
            Ok(buf)
        };

        let toc_count = u32::from_le_bytes(read_region(toc_offset, 4)?.try_into().unwrap());
        if toc_count > 4096 {
            return Err(format_err(format!(
                "implausible TOC entry count {toc_count}"
            )));
        }
        let toc = read_region(toc_offset + 4, toc_count as usize * 28)?;

        let mut directories = Vec::new();
        let mut data_base = None;
        let mut t = Cursor::at(&toc, 0);
        for _ in 0..toc_count {
            let tag = t.u32().unwrap();
            t.take(16);
            let header_offset = t.u32().unwrap() as u64;
            let data_offset = t.u32().unwrap() as u64;
            match tag {
                DDIR_TAG => directories.push((header_offset, data_offset)),
                // The Data entry's *header* offset is the base for asset offsets.
                DATA_TAG => data_base = Some(header_offset),
                _ => {
                    return Err(format_err(format!(
                        "unknown TOC entry tag {:?}",
                        tag.to_be_bytes().map(|b| b as char)
                    )));
                }
            }
        }
        let data_base = data_base.unwrap_or(0);

        let mut entries = Vec::new();
        for (header_offset, data_offset) in directories {
            let dir_header = read_region(header_offset, 84)?;
            let mut d = Cursor::at(&dir_header, 0);
            let target = d.padded_str(32).unwrap();
            d.take(48);
            let count = d.u32().unwrap() as usize;
            let pointer = (!target.is_empty()).then_some(target);

            // Entries are variable-length; the longest possible is 13 + 255 + 1.
            let max_len = (count * 269).min((file_len - data_offset.min(file_len)) as usize);
            let block = read_region(data_offset, max_len)?;
            let mut c = Cursor::at(&block, 0);
            for i in 0..count {
                let truncated =
                    || format_err(format!("directory entry {i} of {count} is truncated"));
                let stored_size = c.u32().ok_or_else(truncated)?;
                let offset = c.u32().ok_or_else(truncated)?;
                c.take(5).ok_or_else(truncated)?;
                let compression = Compression::from_code(c.u8().ok_or_else(truncated)?);
                let name_len = c.u8().ok_or_else(truncated)? as usize;
                let name = latin1(c.take(name_len).ok_or_else(truncated)?);
                c.take(1).ok_or_else(truncated)?;
                entries.push(Entry {
                    name,
                    stored_size,
                    offset,
                    compression,
                    pointer: pointer.clone(),
                });
            }
        }

        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        Ok(Self {
            path,
            name,
            file,
            file_len,
            data_base,
            entries,
        })
    }

    /// The archive's file name, e.g. `core.brn`.
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Every entry, in directory order, pointers included.
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Reads and decompresses an entry that this archive holds itself.
    ///
    /// # Panics
    /// If `entry` is a pointer; resolve pointers through a [`Library`].
    pub fn read(&self, entry: &Entry) -> Result<Vec<u8>> {
        assert!(!entry.is_pointer(), "{} is a pointer entry", entry.name);
        let format_err = |message: String| Error::Format {
            path: self.path.clone(),
            message,
        };
        let decompress_err = |message: String| Error::Decompress {
            asset: entry.name.clone(),
            message,
        };

        let start = self.data_base + entry.offset as u64;
        if start > self.file_len {
            return Err(format_err(format!(
                "{} starts past the end of the file",
                entry.name
            )));
        }

        let read = |offset: u64, len: usize| -> Result<Vec<u8>> {
            // The last entry of some archives claims more bytes than the file
            // has left. The data is intact, so take what is there.
            let available = (self.file_len - offset.min(self.file_len)) as usize;
            let mut buf = vec![0u8; len.min(available)];
            read_fully_at(&self.file, &mut buf, offset).map_err(|source| Error::Io {
                path: self.path.clone(),
                source,
            })?;
            Ok(buf)
        };

        if entry.compression == Compression::None {
            return read(start, entry.stored_size as usize);
        }

        let prefix = read(start, 8)?;
        if prefix.len() < 8 {
            return Err(format_err(format!("{} is truncated", entry.name)));
        }
        let size = u32::from_le_bytes(prefix[..4].try_into().unwrap()) as usize;
        if size > MAX_ASSET_SIZE {
            return Err(decompress_err(format!(
                "implausible decompressed size {size}"
            )));
        }
        let packed = read(start + 8, entry.stored_size as usize)?;

        match entry.compression {
            Compression::Zlib => {
                let out = miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(&packed, size)
                    .map_err(|e| decompress_err(format!("zlib: {e}")))?;
                if out.len() != size {
                    return Err(decompress_err(format!(
                        "zlib produced {} bytes, expected {size}",
                        out.len()
                    )));
                }
                Ok(out)
            }
            Compression::Lzo => {
                lzo::decompress(&packed, size).map_err(|e| decompress_err(e.to_string()))
            }
            Compression::Unknown(code) => {
                Err(decompress_err(format!("unknown compression type {code}")))
            }
            Compression::None => unreachable!(),
        }
    }
}

/// Where a resolved asset lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Location {
    pub barn: usize,
    pub entry: usize,
}

/// Every archive in a data directory, with a case-insensitive name index.
pub struct Library {
    barns: Vec<Barn>,
    /// Upper-cased asset name → where it resolves after following pointers.
    index: HashMap<String, Location>,
    /// Pointer entries whose target could not be found, by asset name.
    unresolved: Vec<(String, String)>,
}

impl Library {
    /// Opens every `.brn` file in `dir`.
    pub fn open_dir(dir: impl AsRef<Path>) -> Result<Self> {
        let dir = dir.as_ref();
        let read_dir = std::fs::read_dir(dir).map_err(|source| Error::Io {
            path: dir.to_path_buf(),
            source,
        })?;
        let mut paths: Vec<PathBuf> = read_dir
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("brn")))
            .collect();
        if paths.is_empty() {
            return Err(Error::Format {
                path: dir.to_path_buf(),
                message: "no .brn archives in this directory".into(),
            });
        }
        paths.sort();
        let barns = paths.iter().map(Barn::open).collect::<Result<Vec<_>>>()?;
        Ok(Self::from_barns(barns))
    }

    /// Builds a library from already-open archives.
    pub fn from_barns(barns: Vec<Barn>) -> Self {
        let by_name: HashMap<String, usize> = barns
            .iter()
            .enumerate()
            .map(|(i, b)| (b.name().to_ascii_uppercase(), i))
            .collect();

        // Each archive's own assets first, so that pointers can find them.
        let mut owned: HashMap<(usize, String), usize> = HashMap::new();
        for (b, barn) in barns.iter().enumerate() {
            for (e, entry) in barn.entries.iter().enumerate() {
                if !entry.is_pointer() {
                    owned.insert((b, entry.name.to_ascii_uppercase()), e);
                }
            }
        }

        // Then the name index in archive and directory order: when two
        // directories declare the same name, the later one wins.
        let mut index = HashMap::new();
        let mut unresolved = Vec::new();
        for (b, barn) in barns.iter().enumerate() {
            for (e, entry) in barn.entries.iter().enumerate() {
                let key = entry.name.to_ascii_uppercase();
                let location = match &entry.pointer {
                    None => Some(Location { barn: b, entry: e }),
                    Some(target) => by_name.get(&target.to_ascii_uppercase()).and_then(|&tb| {
                        owned.get(&(tb, key.clone())).map(|&te| Location {
                            barn: tb,
                            entry: te,
                        })
                    }),
                };
                match location {
                    Some(location) => {
                        index.insert(key, location);
                    }
                    None => unresolved.push((
                        entry.name.clone(),
                        entry.pointer.clone().unwrap_or_default(),
                    )),
                }
            }
        }

        Self {
            barns,
            index,
            unresolved,
        }
    }

    pub fn barns(&self) -> &[Barn] {
        &self.barns
    }

    /// Number of distinct asset names that resolve.
    pub fn len(&self) -> usize {
        self.index.len()
    }

    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }

    /// Pointer entries naming an archive or asset that is not present, as
    /// `(asset, archive)` pairs.
    pub fn unresolved(&self) -> &[(String, String)] {
        &self.unresolved
    }

    /// Finds an asset by name, ignoring case.
    pub fn find(&self, name: &str) -> Option<(&Barn, &Entry)> {
        let loc = self.index.get(&name.to_ascii_uppercase())?;
        let barn = &self.barns[loc.barn];
        Some((barn, &barn.entries[loc.entry]))
    }

    /// Reads an asset by name, ignoring case.
    pub fn read(&self, name: &str) -> Result<Vec<u8>> {
        let (barn, entry) = self
            .find(name)
            .ok_or_else(|| Error::NotFound(name.into()))?;
        barn.read(entry)
    }

    /// Every resolvable asset, sorted by name.
    pub fn assets(&self) -> Vec<(&Barn, &Entry)> {
        let mut all: Vec<_> = self
            .index
            .values()
            .map(|loc| {
                let barn = &self.barns[loc.barn];
                (barn, &barn.entries[loc.entry])
            })
            .collect();
        all.sort_by(|a, b| {
            a.1.name
                .to_ascii_uppercase()
                .cmp(&b.1.name.to_ascii_uppercase())
        });
        all
    }
}

#[cfg(test)]
mod tests;
