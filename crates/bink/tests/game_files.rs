//! Checks against the installed game's movies; skipped when they are not
//! there (game data is never part of the repository).

use std::path::PathBuf;

use gk3_bink::Bink;

fn data_dir() -> Option<PathBuf> {
    let dir = PathBuf::from(std::env::var_os("HOME")?).join("Games/Heroic/Gabriel Knight 3/Data");
    dir.join("intro.bik").exists().then_some(dir)
}

#[test]
fn intro_header_and_first_frames() {
    let Some(dir) = data_dir() else { return };
    let mut bink = Bink::open(std::fs::read(dir.join("intro.bik")).unwrap()).unwrap();
    assert_eq!((bink.width(), bink.height()), (320, 240));
    assert_eq!(bink.frame_count(), 6390);
    assert_eq!(bink.fps(), (30, 1));
    assert_eq!(bink.audio_format(), Some((22050, 2)));
    for _ in 0..10 {
        let frame = bink.next_frame().unwrap().unwrap();
        assert_eq!(frame.len(), 320 * 240 * 4);
        assert!(frame.chunks(4).all(|p| p[3] == 255));
    }
    bink.rewind();
    assert!(bink.next_frame().unwrap().is_ok());
}

#[test]
fn filler_files_are_rejected() {
    let Some(dir) = data_dir() else { return };
    for name in [
        "day1-1.bik",
        "day1-2.bik",
        "day2-3.bik",
        "day2-4.bik",
        "day3-c.bik",
        "day3-d.bik",
    ] {
        let Ok(data) = std::fs::read(dir.join(name)) else {
            continue;
        };
        assert!(Bink::open(data).is_err(), "{name}");
    }
}
