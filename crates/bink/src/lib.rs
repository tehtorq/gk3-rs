//! Bink 1 movies: the game's cutscenes, its intro and the timeblock films.
//!
//! A Bink file is a header (signature `BIK` plus a revision letter, frame
//! count, size, frame rate, audio tracks), an index of frame offsets, then
//! the frames. Each frame holds one packet per audio track (prefixed with
//! its size) followed by the video packet. GK3's movies are revision 'f',
//! 320x240, with one stereo 22050 Hz track in the RDFT audio variant.
//!
//! The decoder follows FFmpeg's `bink.c`, `binkaudio.c`, `binkdsp.c` and
//! the Bink demuxer (LGPL-2.1-or-later), ported to Rust; the old revision
//! 'b' and Bink 2 are not supported.

mod audio;
mod bits;
mod tables;
mod video;

use std::fmt;
use std::ops::Range;

use audio::{AUD_STEREO, AudioDecoder};
use video::VideoDecoder;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

pub(crate) fn err(s: impl Into<String>) -> Error {
    Error(s.into())
}

/// Limits FFmpeg applies to headers.
const MAX_WIDTH: u32 = 7680;
const MAX_HEIGHT: u32 = 4800;
const MAX_FRAMES: u32 = 1_000_000;
const MAX_AUDIO_TRACKS: u32 = 256;

/// An audio track as the header describes it.
#[derive(Debug, Clone, Copy)]
struct AudioTrack {
    sample_rate: u32,
    flags: u16,
}

impl AudioTrack {
    fn channels(&self) -> u16 {
        if self.flags & AUD_STEREO != 0 { 2 } else { 1 }
    }
}

/// Byte ranges, within the file, of one frame's packets.
struct FramePackets {
    /// One packet per audio track (empty if the track has none here).
    audio: Vec<Range<usize>>,
    video: Range<usize>,
}

/// An open Bink movie, decoding its frames in order.
pub struct Bink {
    data: Vec<u8>,
    width: u32,
    height: u32,
    fps: (u32, u32),
    audio: Vec<AudioTrack>,
    /// Byte range of each frame in `data`.
    frames: Vec<(usize, usize)>,
    /// Revision letter and header flags of the video.
    version: u8,
    video_flags: u32,
    video: VideoDecoder,
    /// Index of the frame `next_frame` decodes.
    next: usize,
}

fn u16_at(d: &[u8], at: usize) -> Result<u16, Error> {
    d.get(at..at + 2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .ok_or_else(|| err("bink: truncated header"))
}

fn u32_at(d: &[u8], at: usize) -> Result<u32, Error> {
    d.get(at..at + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or_else(|| err("bink: truncated header"))
}

impl Bink {
    /// Parses the header and frame index. Errors on a non-Bink file.
    pub fn open(data: Vec<u8>) -> Result<Bink, Error> {
        if data.len() < 4 || &data[0..3] != b"BIK" {
            return Err(err("not a Bink file"));
        }
        let version = data[3];
        if !matches!(version, b'f' | b'g' | b'h' | b'i' | b'k') {
            return Err(err(format!(
                "unsupported Bink revision '{}'",
                version as char
            )));
        }
        let file_size = u32_at(&data, 4)? as usize + 8;
        let frame_count = u32_at(&data, 8)?;
        let width = u32_at(&data, 20)?;
        let height = u32_at(&data, 24)?;
        let fps = (u32_at(&data, 28)?, u32_at(&data, 32)?);
        let video_flags = u32_at(&data, 36)?;
        let tracks = u32_at(&data, 40)?;
        if frame_count == 0 || frame_count > MAX_FRAMES {
            return Err(err("bink: bad frame count"));
        }
        if width == 0 || width > MAX_WIDTH || height == 0 || height > MAX_HEIGHT {
            return Err(err("bink: bad picture size"));
        }
        if fps.0 == 0 || fps.1 == 0 {
            return Err(err("bink: bad frame rate"));
        }
        if tracks > MAX_AUDIO_TRACKS {
            return Err(err("bink: too many audio tracks"));
        }
        let tracks = tracks as usize;
        let mut at = 44;
        if version == b'k' {
            at += 4;
        }
        // Skip each track's largest decoded size.
        at += 4 * tracks;
        let mut audio = Vec::with_capacity(tracks);
        for _ in 0..tracks {
            audio.push(AudioTrack {
                sample_rate: u16_at(&data, at)? as u32,
                flags: u16_at(&data, at + 2)?,
            });
            at += 4;
        }
        // Skip track ids.
        at += 4 * tracks;

        // Frame offsets; the low bit marks key frames. The last frame ends
        // at the end of the file.
        let file_end = file_size.min(data.len());
        let mut frames = Vec::with_capacity(frame_count as usize);
        let mut next_pos = u32_at(&data, at)? as usize & !1;
        for i in 0..frame_count as usize {
            let pos = next_pos;
            next_pos = if i + 1 == frame_count as usize {
                file_end
            } else {
                u32_at(&data, at + 4 * (i + 1))? as usize & !1
            };
            if next_pos <= pos || next_pos > data.len() {
                return Err(err("bink: bad frame index"));
            }
            frames.push((pos, next_pos));
        }

        Ok(Bink {
            video: VideoDecoder::new(width, height, version, video_flags),
            data,
            width,
            height,
            fps,
            audio,
            frames,
            version,
            video_flags,
            next: 0,
        })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn frame_count(&self) -> u32 {
        self.frames.len() as u32
    }

    /// Frames per second as numerator/denominator.
    pub fn fps(&self) -> (u32, u32) {
        self.fps
    }

    /// Sample rate and channel count of the first audio track, if any.
    pub fn audio_format(&self) -> Option<(u32, u16)> {
        self.audio.first().map(|t| (t.sample_rate, t.channels()))
    }

    /// Splits frame `i` into its packets.
    fn frame_packets(&self, i: usize) -> Result<FramePackets, Error> {
        let (start, end) = self.frames[i];
        let mut at = start;
        let mut audio = Vec::with_capacity(self.audio.len());
        for _ in 0..self.audio.len() {
            let size =
                u32_at(&self.data[..end], at).map_err(|_| err("bink: truncated frame"))? as usize;
            at += 4;
            if size > end - at {
                return Err(err(format!(
                    "bink: frame {i}: audio packet larger than the frame"
                )));
            }
            audio.push(at..at + size);
            at += size;
        }
        Ok(FramePackets {
            audio,
            video: at..end,
        })
    }

    /// Decodes the next frame's picture as RGBA (width*height*4 bytes,
    /// alpha 255). None at the end.
    pub fn next_frame(&mut self) -> Option<Result<Vec<u8>, Error>> {
        if self.next >= self.frames.len() {
            return None;
        }
        let i = self.next;
        self.next += 1;
        let result = self
            .frame_packets(i)
            .and_then(|p| self.video.decode(&self.data[p.video]))
            .map_err(|e| err(format!("{e} (frame {i})")));
        Some(result.map(|()| {
            let mut rgba = Vec::new();
            video::yuv_to_rgba(
                self.width as usize,
                self.height as usize,
                &self.video.planes(),
                self.video.full_range(),
                &mut rgba,
            );
            rgba
        }))
    }

    /// Restarts from frame 0.
    pub fn rewind(&mut self) {
        self.next = 0;
        self.video = VideoDecoder::new(self.width, self.height, self.version, self.video_flags);
    }

    /// Decodes the whole first audio track (without decoding video) to
    /// interleaved i16.
    pub fn decode_audio(&self) -> Result<Vec<i16>, Error> {
        let Some(track) = self.audio.first() else {
            return Ok(Vec::new());
        };
        let mut decoder = AudioDecoder::new(track.sample_rate, track.channels(), track.flags)?;
        let mut samples = Vec::new();
        // Bytes of 16-bit output the packets say they hold.
        let mut reported = 0;
        for i in 0..self.frames.len() {
            let range = self.frame_packets(i)?.audio.swap_remove(0);
            // The demuxer drops packets too small to hold their size field.
            if range.len() >= 4 {
                let packet = &self.data[range];
                reported += u32_at(packet, 0)? as usize;
                decoder
                    .decode_packet(packet, &mut samples)
                    .map_err(|e| err(format!("{e} (frame {i})")))?;
            }
        }
        // The last block is decoded whole; drop what runs past the end.
        samples.truncate(reported / 2);
        Ok(samples
            .iter()
            .map(|&s| (s * 32768.0).round().clamp(-32768.0, 32767.0) as i16)
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_bink_data() {
        assert!(Bink::open(vec![0x12; 4096]).is_err());
        assert!(Bink::open(b"BIKf".to_vec()).is_err());
        assert!(Bink::open(Vec::new()).is_err());
    }
}
