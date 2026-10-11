//! RIFF/WAVE headers. 97.5% of GK3's sounds are MP3 inside a WAVE wrapper
//! (format tag 0x55); the rest are 16-bit PCM. Decoding lives in `gk3-media`.

use crate::reader::Reader;
use crate::{Error, Result};

pub const FORMAT_PCM: u16 = 1;
pub const FORMAT_MP3: u16 = 0x55;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Format {
    pub tag: u16,
    pub channels: u16,
    pub sample_rate: u32,
    pub bits_per_sample: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wave<'a> {
    pub format: Format,
    /// The `data` chunk: PCM samples or a compressed stream.
    pub data: &'a [u8],
    /// Sample frames per channel, from the `fact` chunk compressed files carry.
    pub fact_frames: Option<u32>,
}

pub fn parse(bytes: &[u8]) -> Result<Wave<'_>> {
    let mut r = Reader::new(bytes);
    r.tag(b"RIFF")?;
    r.skip(4)?;
    r.tag(b"WAVE")?;
    let mut format = None;
    let mut data = None;
    let mut fact_frames = None;
    while r.remaining() >= 8 {
        let id = r.bytes(4)?;
        let size = r.u32()? as usize;
        // Clamp a data chunk that claims more than is there.
        let body = r.bytes(size.min(r.remaining()))?;
        match id {
            b"fmt " => {
                let mut f = Reader::new(body);
                format = Some(Format {
                    tag: f.u16()?,
                    channels: f.u16()?,
                    sample_rate: f.u32()?,
                    bits_per_sample: {
                        f.skip(6)?;
                        f.u16()?
                    },
                });
            }
            b"data" => data = Some(body),
            b"fact" if body.len() >= 4 => {
                fact_frames = Some(u32::from_le_bytes(body[..4].try_into().unwrap()));
            }
            _ => {}
        }
        // Chunks are word-aligned.
        if size % 2 == 1 && r.remaining() > 0 {
            r.skip(1)?;
        }
    }
    let format = format.ok_or_else(|| Error::Format("WAVE has no fmt chunk".into()))?;
    let data = data.ok_or_else(|| Error::Format("WAVE has no data chunk".into()))?;
    if format.channels == 0 || format.sample_rate == 0 {
        return Err(Error::Format(format!("WAVE format {format:?}")));
    }
    Ok(Wave {
        format,
        data,
        fact_frames,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn pcm_wave(samples: &[i16]) -> Vec<u8> {
        let mut out = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
        out.extend(16u32.to_le_bytes());
        out.extend(1u16.to_le_bytes());
        out.extend(1u16.to_le_bytes());
        out.extend(22050u32.to_le_bytes());
        out.extend(44100u32.to_le_bytes());
        out.extend(2u16.to_le_bytes());
        out.extend(16u16.to_le_bytes());
        out.extend(b"data");
        out.extend(((samples.len() * 2) as u32).to_le_bytes());
        samples.iter().for_each(|s| out.extend(s.to_le_bytes()));
        out
    }

    #[test]
    fn parses_pcm() {
        let bytes = pcm_wave(&[1, -1]);
        let wave = parse(&bytes).unwrap();
        assert_eq!(wave.format.tag, FORMAT_PCM);
        assert_eq!(wave.format.sample_rate, 22050);
        assert_eq!(wave.format.bits_per_sample, 16);
        assert_eq!(wave.data, &[1, 0, 0xFF, 0xFF]);
    }

    #[test]
    fn missing_data_is_an_error() {
        let mut bytes = pcm_wave(&[]);
        bytes.truncate(bytes.len() - 8);
        assert!(parse(&bytes).is_err());
    }
}
