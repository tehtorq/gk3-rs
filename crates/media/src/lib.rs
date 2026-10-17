//! Audio and video decoding.
//!
//! Sounds come out of the archives as RIFF/WAVE files. Most hold an MP3
//! stream (format tag 0x55), which is decoded here with symphonia's pure-Rust
//! MP3 decoder; the rest are plain PCM.

pub mod avi;
pub mod cinepak;

use std::fmt;
use std::io::Cursor;

use gk3_formats::wav::{self, FORMAT_MP3, FORMAT_PCM};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{Decoder, DecoderOptions};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::{FormatOptions, FormatReader};
use symphonia::core::io::{MediaSourceStream, MediaSourceStreamOptions};
use symphonia::default::codecs::MpaDecoder;
use symphonia::default::formats::MpaReader;

/// Decoded audio: interleaved signed 16-bit samples.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pcm {
    pub channels: u16,
    pub sample_rate: u32,
    pub samples: Vec<i16>,
}

impl Pcm {
    pub fn frames(&self) -> usize {
        self.samples.len() / self.channels.max(1) as usize
    }

    pub fn seconds(&self) -> f64 {
        self.frames() as f64 / self.sample_rate as f64
    }

    /// The audio as a 16-bit PCM WAVE file.
    pub fn to_wave(&self) -> Vec<u8> {
        let data_len = self.samples.len() as u32 * 2;
        let block = self.channels * 2;
        let mut out = Vec::with_capacity(44 + data_len as usize);
        out.extend(b"RIFF");
        out.extend((36 + data_len).to_le_bytes());
        out.extend(b"WAVEfmt ");
        out.extend(16u32.to_le_bytes());
        out.extend(FORMAT_PCM.to_le_bytes());
        out.extend(self.channels.to_le_bytes());
        out.extend(self.sample_rate.to_le_bytes());
        out.extend((self.sample_rate * block as u32).to_le_bytes());
        out.extend(block.to_le_bytes());
        out.extend(16u16.to_le_bytes());
        out.extend(b"data");
        out.extend(data_len.to_le_bytes());
        for s in &self.samples {
            out.extend(s.to_le_bytes());
        }
        out
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    Format(gk3_formats::Error),
    Unsupported(String),
    Decode(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Format(e) => e.fmt(f),
            Self::Unsupported(m) => write!(f, "unsupported: {m}"),
            Self::Decode(m) => write!(f, "decode: {m}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<gk3_formats::Error> for Error {
    fn from(e: gk3_formats::Error) -> Self {
        Self::Format(e)
    }
}

/// Decodes a WAVE file holding PCM or MP3.
pub fn decode_wave(bytes: &[u8]) -> Result<Pcm, Error> {
    let wave = wav::parse(bytes)?;
    let f = wave.format;
    match (f.tag, f.bits_per_sample) {
        (FORMAT_PCM, 16) => Ok(Pcm {
            channels: f.channels,
            sample_rate: f.sample_rate,
            samples: wave
                .data
                .chunks_exact(2)
                .map(|b| i16::from_le_bytes([b[0], b[1]]))
                .collect(),
        }),
        (FORMAT_PCM, 8) => Ok(Pcm {
            channels: f.channels,
            sample_rate: f.sample_rate,
            // 8-bit WAVE is unsigned.
            samples: wave.data.iter().map(|&b| ((b as i16) - 128) << 8).collect(),
        }),
        (FORMAT_MP3, _) => decode_mp3(wave.data),
        (tag, bits) => Err(Error::Unsupported(format!(
            "WAVE format {tag:#x} at {bits} bits"
        ))),
    }
}

/// Decodes a bare MPEG audio stream.
pub fn decode_mp3(stream: &[u8]) -> Result<Pcm, Error> {
    let err = |e: SymphoniaError| Error::Decode(e.to_string());
    let source = MediaSourceStream::new(
        Box::new(Cursor::new(stream.to_vec())),
        MediaSourceStreamOptions::default(),
    );
    let mut reader = MpaReader::try_new(source, &FormatOptions::default()).map_err(err)?;
    let track = reader
        .default_track()
        .ok_or_else(|| Error::Decode("no audio track".into()))?;
    let track_id = track.id;
    let mut decoder =
        MpaDecoder::try_new(&track.codec_params, &DecoderOptions::default()).map_err(err)?;

    let mut samples = Vec::new();
    let mut channels = 0u16;
    let mut sample_rate = 0u32;
    let mut buffer: Option<SampleBuffer<i16>> = None;

    loop {
        let packet = match reader.next_packet() {
            Ok(p) => p,
            Err(SymphoniaError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                break;
            }
            Err(e) => return Err(err(e)),
        };
        if packet.track_id() != track_id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(decoded) => {
                let spec = *decoded.spec();
                channels = spec.channels.count() as u16;
                sample_rate = spec.rate;
                let needed = decoded.capacity() as u64;
                let buf = match &mut buffer {
                    Some(b) if b.capacity() as u64 >= needed * spec.channels.count() as u64 => b,
                    _ => buffer.insert(SampleBuffer::new(needed, spec)),
                };
                buf.copy_interleaved_ref(decoded);
                samples.extend_from_slice(buf.samples());
            }
            // A damaged frame is skipped, as a player would.
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(e) => return Err(err(e)),
        }
    }

    if channels == 0 {
        return Err(Error::Decode("no audio frames".into()));
    }
    Ok(Pcm {
        channels,
        sample_rate,
        samples,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pcm16(samples: &[i16]) -> Vec<u8> {
        Pcm {
            channels: 1,
            sample_rate: 22050,
            samples: samples.to_vec(),
        }
        .to_wave()
    }

    #[test]
    fn pcm_round_trips() {
        let pcm = decode_wave(&pcm16(&[0, 1000, -1000, i16::MAX])).unwrap();
        assert_eq!(pcm.samples, [0, 1000, -1000, i16::MAX]);
        assert_eq!(pcm.sample_rate, 22050);
        assert_eq!(pcm.frames(), 4);
    }

    #[test]
    fn garbage_mp3_is_an_error_not_a_panic() {
        assert!(decode_mp3(&[0x12; 64]).is_err());
    }
}
