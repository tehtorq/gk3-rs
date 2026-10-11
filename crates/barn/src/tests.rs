use super::*;
use std::sync::atomic::{AtomicU32, Ordering};

/// One asset for [`build`]: name, compression code, bytes as stored, and the
/// decompressed size to write in the prefix of a compressed asset.
struct Asset<'a> {
    name: &'a str,
    code: u8,
    stored: Vec<u8>,
    unpacked: u32,
}

fn stored<'a>(name: &'a str, bytes: &[u8]) -> Asset<'a> {
    Asset {
        name,
        code: 0,
        stored: bytes.to_vec(),
        unpacked: bytes.len() as u32,
    }
}

/// Writes a Barn in the retail layout: header, TOC, directory headers,
/// directory entries, then the data section. `pointer_dirs` are directories of
/// pointer entries into other archives, as `(archive, names)`.
fn build(assets: &[Asset], pointer_dirs: &[(&str, &[&str])]) -> Vec<u8> {
    let le = |v: u32| v.to_le_bytes();
    let dir_count = 1 + pointer_dirs.len();
    let toc_offset = 24 + 64; // header plus some padding, as retail has
    let toc_len = 4 + 28 * (dir_count + 1);
    let headers_at = toc_offset + toc_len;
    let entries_at = headers_at + 84 * dir_count;

    let entry_bytes = |name: &str, size: u32, offset: u32, code: u8| {
        let mut e = Vec::new();
        e.extend(le(size));
        e.extend(le(offset));
        e.extend([0u8; 5]);
        e.push(code);
        e.push(name.len() as u8);
        e.extend(name.as_bytes());
        e.push(0);
        e
    };

    // Directory 0 holds the assets; the rest are pointer directories.
    let mut dir_entries: Vec<Vec<u8>> = Vec::new();
    let mut data = Vec::new();
    let mut own = Vec::new();
    for a in assets {
        own.extend(entry_bytes(
            a.name,
            a.stored.len() as u32,
            data.len() as u32,
            a.code,
        ));
        if a.code != 0 && a.code != 3 {
            data.extend(le(a.unpacked));
            data.extend(le(0));
        }
        data.extend(&a.stored);
    }
    dir_entries.push(own);
    for (_, names) in pointer_dirs {
        let mut e = Vec::new();
        for n in *names {
            e.extend(entry_bytes(n, 0, 0, 0));
        }
        dir_entries.push(e);
    }

    let mut entry_offsets = Vec::new();
    let mut at = entries_at;
    for e in &dir_entries {
        entry_offsets.push(at);
        at += e.len();
    }
    let data_at = at;

    let mut out = Vec::new();
    out.extend(b"GK3!Barn");
    out.extend(le(65536));
    out.extend(le(65536));
    out.extend(le(0));
    out.extend(le(toc_offset as u32));
    out.resize(toc_offset, 0);

    out.extend(le((dir_count + 1) as u32));
    for (i, offset) in entry_offsets.iter().enumerate() {
        out.extend(b"riDD");
        out.extend([0u8; 16]);
        out.extend(le((headers_at + 84 * i) as u32));
        out.extend(le(*offset as u32));
    }
    out.extend(b"ataD");
    out.extend([0u8; 16]);
    out.extend(le(data_at as u32));
    out.extend(le(0));

    for i in 0..dir_count {
        let mut name = [0u8; 32];
        let count = if i == 0 {
            assets.len()
        } else {
            let (archive, names) = pointer_dirs[i - 1];
            name[..archive.len()].copy_from_slice(archive.as_bytes());
            names.len()
        };
        out.extend(name);
        out.extend([0u8; 48]);
        out.extend(le(count as u32));
    }
    for e in dir_entries {
        out.extend(e);
    }
    assert_eq!(out.len(), data_at);
    out.extend(data);
    out
}

/// A scratch directory unique to one test, removed when dropped.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static N: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "gk3-barn-test-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn write(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn reads_stored_assets() {
    let tmp = TempDir::new();
    let path = tmp.write(
        "a.brn",
        &build(
            &[stored("ONE.TXT", b"first"), stored("TWO.TXT", b"second")],
            &[],
        ),
    );
    let barn = Barn::open(&path).unwrap();
    assert_eq!(barn.entries().len(), 2);
    assert_eq!(barn.read(&barn.entries()[0]).unwrap(), b"first");
    assert_eq!(barn.read(&barn.entries()[1]).unwrap(), b"second");
}

#[test]
fn compression_code_3_is_stored() {
    let tmp = TempDir::new();
    let mut asset = stored("X.BIN", b"plain");
    asset.code = 3;
    let barn = Barn::open(tmp.write("a.brn", &build(&[asset], &[]))).unwrap();
    assert_eq!(barn.entries()[0].compression, Compression::None);
    assert_eq!(barn.read(&barn.entries()[0]).unwrap(), b"plain");
}

#[test]
fn reads_zlib_assets() {
    let original = b"the same words, the same words, the same words".repeat(20);
    let packed = miniz_oxide::deflate::compress_to_vec_zlib(&original, 6);
    let asset = Asset {
        name: "Z.TXT",
        code: 1,
        stored: packed,
        unpacked: original.len() as u32,
    };
    let tmp = TempDir::new();
    let barn = Barn::open(tmp.write("a.brn", &build(&[asset], &[]))).unwrap();
    assert_eq!(barn.read(&barn.entries()[0]).unwrap(), original);
}

#[test]
fn reads_lzo_assets_with_slack_after_end_marker() {
    // Five literals then the end marker, with the recorded size overstating
    // the stream, as retail archives do.
    let mut stream = vec![22, b'h', b'e', b'l', b'l', b'o', 0x11, 0, 0];
    stream.extend([0u8; 7]);
    let asset = Asset {
        name: "L.TXT",
        code: 2,
        stored: stream,
        unpacked: 5,
    };
    let tmp = TempDir::new();
    let barn = Barn::open(tmp.write("a.brn", &build(&[asset], &[]))).unwrap();
    assert_eq!(barn.read(&barn.entries()[0]).unwrap(), b"hello");
}

#[test]
fn final_entry_overrunning_the_file_is_clamped() {
    let tmp = TempDir::new();
    let mut bytes = build(&[stored("END.BIN", b"0123456789")], &[]);
    bytes.truncate(bytes.len() - 4);
    let barn = Barn::open(tmp.write("a.brn", &bytes)).unwrap();
    assert_eq!(barn.read(&barn.entries()[0]).unwrap(), b"012345");
}

#[test]
fn rejects_files_that_are_not_barns() {
    let tmp = TempDir::new();
    let path = tmp.write("bad.brn", b"RIFF\0\0\0\0WAVEfmt nothing to see here");
    assert!(matches!(Barn::open(path), Err(Error::Format { .. })));
}

#[test]
fn rejects_truncated_directories() {
    let tmp = TempDir::new();
    let bytes = build(&[stored("A.TXT", b"a")], &[]);
    // Cut inside the directory entries.
    let path = tmp.write("cut.brn", &bytes[..24 + 64 + 4 + 56 + 84 + 6]);
    assert!(Barn::open(path).is_err());
}

#[test]
fn library_resolves_pointers_case_insensitively() {
    let tmp = TempDir::new();
    tmp.write(
        "core.brn",
        &build(
            &[stored("CORE.TXT", b"in core")],
            &[("day1.brn", &["Day1.Txt"])],
        ),
    );
    tmp.write(
        "DAY1.BRN",
        &build(&[stored("DAY1.TXT", b"in day one")], &[]),
    );

    let library = Library::open_dir(&tmp.0).unwrap();
    assert_eq!(library.read("core.txt").unwrap(), b"in core");
    assert_eq!(library.read("day1.TXT").unwrap(), b"in day one");
    let (barn, entry) = library.find("DAY1.TXT").unwrap();
    assert_eq!(barn.name(), "DAY1.BRN");
    assert!(!entry.is_pointer());
    assert!(library.unresolved().is_empty());
    assert_eq!(library.len(), 2);
}

#[test]
fn library_reports_pointers_to_missing_archives() {
    let tmp = TempDir::new();
    tmp.write("core.brn", &build(&[], &[("day9.brn", &["GONE.TXT"])]));
    let library = Library::open_dir(&tmp.0).unwrap();
    assert!(matches!(library.read("GONE.TXT"), Err(Error::NotFound(_))));
    assert_eq!(
        library.unresolved(),
        &[("GONE.TXT".to_string(), "day9.brn".to_string())]
    );
}

#[test]
fn later_directories_win() {
    let tmp = TempDir::new();
    tmp.write("a.brn", &build(&[stored("SAME.TXT", b"from a")], &[]));
    tmp.write("b.brn", &build(&[stored("SAME.TXT", b"from b")], &[]));
    let library = Library::open_dir(&tmp.0).unwrap();
    assert_eq!(library.read("SAME.TXT").unwrap(), b"from b");
}

#[test]
fn empty_directory_is_an_error() {
    let tmp = TempDir::new();
    assert!(Library::open_dir(&tmp.0).is_err());
}
