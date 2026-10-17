//! AVI movies: SIDNEY's analysis animations and the fingerprint scans.
//!
//! A RIFF file: a header list with the frame rate and, per stream, its
//! format; then the `movi` list of chunks, `00dc`/`00db` for video and
//! `01wb` for audio. The game's AVIs are 8-bit Microsoft RLE (palette in
//! the format, frames drawn over the one before) with PCM sound, but for
//! the Sierra logo, which is 24-bit Cinepak.

use std::fmt;
use std::ops::Range;

use gk3_formats::image::Image;

use crate::Pcm;
use crate::cinepak::Cinepak;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

fn err(s: impl Into<String>) -> Error {
    Error(s.into())
}

/// Video compressions this reader decodes.
const BI_RGB: u32 = 0;
const BI_RLE8: u32 = 1;
const CVID: u32 = u32::from_le_bytes(*b"cvid");

pub struct Avi {
    pub width: u32,
    pub height: u32,
    /// Seconds each frame shows.
    pub frame_seconds: f64,
    compression: u32,
    bits: u16,
    palette: Vec<[u8; 3]>,
    /// Video chunks and palette changes, in order, as ranges of `data`.
    chunks: Vec<(Chunk, Range<usize>)>,
    pub audio: Option<Pcm>,
    data: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Chunk {
    Frame,
    Palette,
}

fn u16_at(d: &[u8], at: usize) -> u16 {
    d.get(at..at + 2)
        .map_or(0, |b| u16::from_le_bytes([b[0], b[1]]))
}

fn u32_at(d: &[u8], at: usize) -> u32 {
    d.get(at..at + 4)
        .map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

impl Avi {
    pub fn parse(data: Vec<u8>) -> Result<Self, Error> {
        if data.get(0..4) != Some(b"RIFF") || data.get(8..12) != Some(b"AVI ") {
            return Err(err("not an AVI"));
        }
        let mut avi = Avi {
            width: 0,
            height: 0,
            frame_seconds: 1.0 / 15.0,
            compression: BI_RGB,
            bits: 8,
            palette: Vec::new(),
            chunks: Vec::new(),
            audio: None,
            data: Vec::new(),
        };
        // Streams in order: their type, as `strh` says.
        let mut stream_types: Vec<[u8; 4]> = Vec::new();
        let mut audio_format: Option<(u16, u16, u32, u16)> = None;
        let mut audio_bytes: Vec<u8> = Vec::new();
        let mut walk = vec![(12usize, data.len())];
        while let Some((mut at, end)) = walk.pop() {
            while at + 8 <= end {
                let id: [u8; 4] = data[at..at + 4].try_into().unwrap();
                let size = u32_at(&data, at + 4) as usize;
                let body = at + 8;
                let body_end = (body + size).min(end);
                match &id {
                    b"LIST" => {
                        // Lists hold chunks; look inside, keeping order.
                        walk.push((body_end, end));
                        walk.push((body + 4, body_end));
                        at = end;
                        continue;
                    }
                    b"avih" => {
                        let micros = u32_at(&data, body);
                        if micros > 0 {
                            avi.frame_seconds = micros as f64 / 1_000_000.0;
                        }
                    }
                    b"strh" => {
                        stream_types.push(data[body..body + 4].try_into().unwrap_or(*b"    "));
                        if &data[body..body + 4] == b"vids" {
                            let scale = u32_at(&data, body + 20);
                            let rate = u32_at(&data, body + 24);
                            if scale > 0 && rate > 0 {
                                avi.frame_seconds = scale as f64 / rate as f64;
                            }
                        }
                    }
                    b"strf" => match stream_types.last() {
                        Some(b"vids") => {
                            avi.width = u32_at(&data, body + 4);
                            avi.height = (u32_at(&data, body + 8) as i32).unsigned_abs();
                            avi.bits = u16_at(&data, body + 14);
                            avi.compression = u32_at(&data, body + 16);
                            let header = u32_at(&data, body) as usize;
                            let used = u32_at(&data, body + 32) as usize;
                            let colors = if used == 0 && avi.bits <= 8 {
                                1 << avi.bits
                            } else {
                                used
                            };
                            let pal = body + header;
                            avi.palette = (0..colors)
                                .filter_map(|i| data.get(pal + i * 4..pal + i * 4 + 3))
                                .map(|b| [b[2], b[1], b[0]])
                                .collect();
                        }
                        Some(b"auds") => {
                            audio_format = Some((
                                u16_at(&data, body),
                                u16_at(&data, body + 2),
                                u32_at(&data, body + 4),
                                u16_at(&data, body + 14),
                            ));
                        }
                        _ => {}
                    },
                    _ => {
                        // Stream chunks: two digits for the stream, then a kind.
                        let stream = std::str::from_utf8(&id[0..2])
                            .ok()
                            .and_then(|s| s.parse::<usize>().ok());
                        if let Some(stream) = stream {
                            match (stream_types.get(stream), &id[2..4]) {
                                (Some(b"vids"), b"dc" | b"db") => {
                                    avi.chunks.push((Chunk::Frame, body..body_end))
                                }
                                (Some(b"vids"), b"pc") => {
                                    avi.chunks.push((Chunk::Palette, body..body_end))
                                }
                                (Some(b"auds"), b"wb") => {
                                    audio_bytes.extend_from_slice(&data[body..body_end])
                                }
                                _ => {}
                            }
                        }
                    }
                }
                at = body + size + (size & 1);
            }
        }
        if avi.width == 0 || avi.height == 0 {
            return Err(err("no video stream"));
        }
        if !matches!(avi.compression, BI_RGB | BI_RLE8 | CVID)
            || (avi.compression == BI_RGB && avi.bits != 8)
            || (avi.compression == CVID && avi.bits != 24)
        {
            return Err(err(format!(
                "video compression {:#x} is not read",
                avi.compression
            )));
        }
        if let Some((1, channels, rate, 16)) = audio_format {
            let samples = audio_bytes
                .chunks_exact(2)
                .map(|b| i16::from_le_bytes([b[0], b[1]]))
                .collect();
            avi.audio = Some(Pcm {
                channels: channels.max(1),
                sample_rate: rate,
                samples,
            });
        } else if let Some((1, channels, rate, 8)) = audio_format {
            let samples = audio_bytes
                .iter()
                .map(|&b| ((b as i16) - 128) << 8)
                .collect();
            avi.audio = Some(Pcm {
                channels: channels.max(1),
                sample_rate: rate,
                samples,
            });
        }
        avi.data = data;
        Ok(avi)
    }

    pub fn frame_count(&self) -> usize {
        self.chunks
            .iter()
            .filter(|(k, _)| *k == Chunk::Frame)
            .count()
    }

    pub fn seconds(&self) -> f64 {
        self.frame_count() as f64 * self.frame_seconds
    }

    /// A player at the start, to decode the frames in order.
    pub fn frames(&self) -> Player {
        Player {
            next: 0,
            width: self.width,
            height: self.height,
            palette: self.palette.clone(),
            pixels: vec![0; (self.width * self.height) as usize],
            cinepak: (self.compression == CVID).then(|| Cinepak::new(self.width, self.height)),
        }
    }
}

/// Decodes a movie's frames one after another; each RLE frame is drawn
/// over the last. It holds no reference to the movie, so the two can be
/// kept side by side.
pub struct Player {
    next: usize,
    width: u32,
    height: u32,
    palette: Vec<[u8; 3]>,
    /// Palette indices, top row first.
    pixels: Vec<u8>,
    /// A full-colour movie's picture instead.
    cinepak: Option<Cinepak>,
}

impl Player {
    /// The palette index of each pixel of the frame last decoded.
    pub fn indices(&self) -> &[u8] {
        &self.pixels
    }

    /// The frame last decoded, in colour.
    pub fn image(&self) -> Image {
        if let Some(c) = &self.cinepak {
            return Image {
                width: self.width,
                height: self.height,
                rgba: c
                    .rgb
                    .chunks_exact(3)
                    .flat_map(|p| [p[0], p[1], p[2], 255])
                    .collect(),
            };
        }
        let rgba = self
            .pixels
            .iter()
            .flat_map(|&i| {
                let [r, g, b] = self.palette.get(i as usize).copied().unwrap_or([0, 0, 0]);
                [r, g, b, 255]
            })
            .collect();
        Image {
            width: self.width,
            height: self.height,
            rgba,
        }
    }

    /// Decodes the next frame of `avi` (the movie it was made from); false
    /// when there are none left.
    pub fn advance(&mut self, avi: &Avi) -> bool {
        while let Some((kind, range)) = avi.chunks.get(self.next).cloned() {
            self.next += 1;
            let bytes = &avi.data[range];
            match kind {
                Chunk::Palette => self.palette_change(bytes),
                Chunk::Frame => {
                    // An empty chunk repeats the last frame.
                    if !bytes.is_empty() {
                        if let Some(c) = &mut self.cinepak {
                            c.decode(bytes);
                            return true;
                        }
                        match avi.compression {
                            BI_RLE8 => rle8(
                                bytes,
                                avi.width as usize,
                                avi.height as usize,
                                &mut self.pixels,
                            ),
                            _ => self.raw(bytes),
                        }
                    }
                    return true;
                }
            }
        }
        false
    }

    fn palette_change(&mut self, b: &[u8]) {
        let first = b.first().copied().unwrap_or(0) as usize;
        let count = match b.get(1).copied().unwrap_or(0) {
            0 => 256,
            n => n as usize,
        };
        for i in 0..count {
            let Some(e) = b.get(4 + i * 4..4 + i * 4 + 3) else {
                break;
            };
            if let Some(p) = self.palette.get_mut(first + i) {
                *p = [e[0], e[1], e[2]];
            }
        }
    }

    /// Uncompressed 8-bit rows, bottom up, padded to four bytes.
    fn raw(&mut self, b: &[u8]) {
        let (w, h) = (self.width as usize, self.height as usize);
        let stride = w.div_ceil(4) * 4;
        for row in 0..h {
            let Some(src) = b.get(row * stride..row * stride + w) else {
                break;
            };
            let y = h - 1 - row;
            self.pixels[y * w..y * w + w].copy_from_slice(src);
        }
    }
}

/// Microsoft RLE8 onto `pixels` (top row first; the data is bottom up).
/// Runs are a count and a colour; a zero count escapes: 0 ends a line,
/// 1 the picture, 2 jumps right and up, more is that many literal bytes,
/// padded to an even count. Pixels it does not touch keep the last frame.
fn rle8(b: &[u8], w: usize, h: usize, pixels: &mut [u8]) {
    let (mut x, mut row) = (0usize, 0usize);
    let mut i = 0;
    let mut put = |x: usize, row: usize, v: u8| {
        if x < w && row < h {
            pixels[(h - 1 - row) * w + x] = v;
        }
    };
    while i + 1 < b.len() {
        let (n, v) = (b[i] as usize, b[i + 1]);
        i += 2;
        if n > 0 {
            for _ in 0..n {
                put(x, row, v);
                x += 1;
            }
            continue;
        }
        match v {
            0 => {
                x = 0;
                row += 1;
            }
            1 => return,
            2 => {
                x += b.get(i).copied().unwrap_or(0) as usize;
                row += b.get(i + 1).copied().unwrap_or(0) as usize;
                i += 2;
            }
            n => {
                let n = n as usize;
                for k in 0..n {
                    put(x, row, b.get(i + k).copied().unwrap_or(0));
                    x += 1;
                }
                i += n + (n & 1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rle8_runs_literals_and_jumps() {
        // 4x2: bottom row "1 1 2 3", then a jump that skips the top row's
        // first pixel, which keeps its old value 9.
        let data = [2, 1, 0, 3, 2, 3, 0, 0, 0, 0, 0, 2, 1, 0, 3, 7, 0, 1];
        let mut px = vec![9u8; 8];
        rle8(&data, 4, 2, &mut px);
        assert_eq!(&px[4..8], &[1, 1, 2, 3], "bottom row");
        assert_eq!(&px[0..4], &[9, 7, 7, 7], "top row, after the jump");
    }

    /// Every RLE movie the game ships decodes, with its sound.
    #[test]
    fn the_games_movies() {
        let Some(dir) = std::env::var_os("GK3_DATA") else {
            return;
        };
        let mut read = 0;
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if !path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("avi"))
            {
                continue;
            }
            match Avi::parse(std::fs::read(&path).unwrap()) {
                Ok(avi) => {
                    let mut frames = avi.frames();
                    let mut n = 0;
                    while frames.advance(&avi) {
                        n += 1;
                    }
                    assert_eq!(n, avi.frame_count());
                    assert!(n >= 2, "{}: {n} frames", path.display());
                    eprintln!("{}: {n} frames, {:.1}s", path.display(), avi.seconds());
                    assert!(avi.audio.is_some(), "{}: no sound", path.display());
                    read += 1;
                }
                Err(e) => panic!("{}: {e}", path.display()),
            }
        }
        assert!(read >= 14 || read == 0);
    }
}
