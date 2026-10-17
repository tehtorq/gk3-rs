//! A full-screen movie: the Sierra logo, the intro, the timeblock films and
//! the story's cutscenes. They lie loose beside the archives, as Bink
//! (`.bik`) or AVI files; a name is looked for with either.

use gk3_barn::Library;
use gk3_bink::Bink;
use gk3_formats::image::Image;
use gk3_media::Pcm;
use gk3_media::avi::{Avi, Player};

enum Source {
    Avi(Box<(Avi, Player)>),
    /// A Bink movie, and its sound decoded whole when it was opened.
    Bink(Box<Bink>, Option<Pcm>),
}

pub struct Movie {
    source: Source,
    frame_seconds: f64,
    frames: usize,
    /// How many frames have been decoded; the picture is the last.
    decoded: usize,
    picture: Option<Image>,
}

impl Movie {
    /// Opens a movie by name, with or without its extension.
    pub fn open(lib: &Library, name: &str) -> Option<Movie> {
        let stem = name
            .rsplit_once('.')
            .filter(|(_, ext)| ext.eq_ignore_ascii_case("bik") || ext.eq_ignore_ascii_case("avi"))
            .map_or(name, |(stem, _)| stem);
        if let Some(bink) = lib
            .loose_file(&format!("{stem}.bik"))
            .and_then(|b| Bink::open(b).ok())
        {
            let (num, den) = bink.fps();
            let sound = bink.audio_format().and_then(|(rate, channels)| {
                Some(Pcm {
                    channels,
                    sample_rate: rate,
                    samples: bink.decode_audio().ok()?,
                })
            });
            return Some(Movie {
                frame_seconds: den.max(1) as f64 / num.max(1) as f64,
                frames: bink.frame_count() as usize,
                source: Source::Bink(Box::new(bink), sound),
                decoded: 0,
                picture: None,
            });
        }
        let avi = Avi::parse(lib.loose_file(&format!("{stem}.avi"))?).ok()?;
        let player = avi.frames();
        Some(Movie {
            frame_seconds: avi.frame_seconds,
            frames: avi.frame_count(),
            source: Source::Avi(Box::new((avi, player))),
            decoded: 0,
            picture: None,
        })
    }

    pub fn seconds(&self) -> f64 {
        self.frames as f64 * self.frame_seconds
    }

    /// The movie's sound, whole.
    pub fn sound(&self) -> Option<Pcm> {
        match &self.source {
            Source::Avi(avi) => avi.0.audio.clone(),
            Source::Bink(_, sound) => sound.clone(),
        }
    }

    /// Decodes up to the frame showing `seconds` in. True if the picture
    /// changed. Frames are decoded in order; a late call catches up.
    pub fn advance(&mut self, seconds: f64) -> bool {
        let want = ((seconds / self.frame_seconds).floor() as usize + 1).min(self.frames);
        if want <= self.decoded {
            return false;
        }
        match &mut self.source {
            Source::Avi(avi) => {
                let (avi, player) = &mut **avi;
                while self.decoded < want && player.advance(avi) {
                    self.decoded += 1;
                }
                self.picture = Some(player.image());
            }
            Source::Bink(bink, _) => {
                let mut last = None;
                while self.decoded < want {
                    // A damaged frame keeps the last good picture.
                    match bink.next_frame() {
                        Some(Ok(rgba)) => last = Some(rgba),
                        Some(Err(_)) => {}
                        None => break,
                    }
                    self.decoded += 1;
                }
                if let Some(rgba) = last {
                    self.picture = Some(Image {
                        width: bink.width(),
                        height: bink.height(),
                        rgba,
                    });
                }
            }
        }
        true
    }

    /// The frame last decoded, and its number from 1.
    pub fn picture(&self) -> Option<(&Image, usize)> {
        self.picture.as_ref().map(|p| (p, self.decoded))
    }
}
