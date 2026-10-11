//! LZO1X decompression.
//!
//! A safe port of the reference `lzo1x_decompress_safe` state machine. Every read
//! and write is bounds-checked, so corrupt input produces an error rather than
//! a panic or out-of-bounds access.
//!
//! GK3's streams end at the `0x11 0x00 0x00` end-of-stream marker, which often
//! comes *before* the end of the input the archive recorded, so trailing input
//! is not an error here.

use std::fmt;

/// Why a stream could not be decompressed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LzoError {
    /// The input ended before the end-of-stream marker.
    InputOverrun { at: usize },
    /// The stream would write more than the expected decompressed size.
    OutputOverrun { at: usize },
    /// A match refers to data before the start of the output.
    LookBehindOverrun { at: usize },
    /// The stream ended before filling the expected decompressed size.
    ShortOutput { expected: usize, got: usize },
}

impl fmt::Display for LzoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InputOverrun { at } => write!(f, "LZO input ended early at byte {at}"),
            Self::OutputOverrun { at } => write!(f, "LZO output overflow at input byte {at}"),
            Self::LookBehindOverrun { at } => {
                write!(f, "LZO match before start of output at input byte {at}")
            }
            Self::ShortOutput { expected, got } => {
                write!(f, "LZO stream produced {got} bytes, expected {expected}")
            }
        }
    }
}

impl std::error::Error for LzoError {}

/// Longest run of zero bytes allowed in a length extension, as in the reference
/// decoder. Prevents a malicious stream from overflowing the length.
const MAX_255_COUNT: usize = usize::MAX / 255 - 2;

struct Decoder<'a> {
    input: &'a [u8],
    ip: usize,
    out: Vec<u8>,
    capacity: usize,
}

impl Decoder<'_> {
    fn byte(&mut self) -> Result<usize, LzoError> {
        let b = *self
            .input
            .get(self.ip)
            .ok_or(LzoError::InputOverrun { at: self.ip })?;
        self.ip += 1;
        Ok(b as usize)
    }

    fn le16(&mut self) -> Result<usize, LzoError> {
        Ok(self.byte()? | (self.byte()? << 8))
    }

    /// Reads the zero-run length extension that follows a zero length field,
    /// returning `255 * zeros + final byte`.
    fn extended_length(&mut self) -> Result<usize, LzoError> {
        let start = self.ip;
        while *self
            .input
            .get(self.ip)
            .ok_or(LzoError::InputOverrun { at: self.ip })?
            == 0
        {
            self.ip += 1;
        }
        let zeros = self.ip - start;
        if zeros > MAX_255_COUNT {
            return Err(LzoError::InputOverrun { at: self.ip });
        }
        Ok(zeros * 255 + self.byte()?)
    }

    fn literals(&mut self, count: usize) -> Result<(), LzoError> {
        let end = self.ip + count;
        if end > self.input.len() {
            return Err(LzoError::InputOverrun {
                at: self.input.len(),
            });
        }
        if self.out.len() + count > self.capacity {
            return Err(LzoError::OutputOverrun { at: self.ip });
        }
        self.out.extend_from_slice(&self.input[self.ip..end]);
        self.ip = end;
        Ok(())
    }

    /// Copies `count` bytes from `distance` bytes back in the output. The
    /// regions may overlap, which repeats a short pattern, so this goes byte by
    /// byte when they do.
    fn back_copy(&mut self, distance: usize, count: usize) -> Result<(), LzoError> {
        if distance == 0 || distance > self.out.len() {
            return Err(LzoError::LookBehindOverrun { at: self.ip });
        }
        if self.out.len() + count > self.capacity {
            return Err(LzoError::OutputOverrun { at: self.ip });
        }
        let start = self.out.len() - distance;
        if distance >= count {
            self.out.extend_from_within(start..start + count);
        } else {
            for i in 0..count {
                let b = self.out[start + i];
                self.out.push(b);
            }
        }
        Ok(())
    }
}

/// Decompresses an LZO1X stream that is expected to produce exactly
/// `decompressed_size` bytes.
pub fn decompress(input: &[u8], decompressed_size: usize) -> Result<Vec<u8>, LzoError> {
    let mut d = Decoder {
        input,
        ip: 0,
        out: Vec::with_capacity(decompressed_size),
        capacity: decompressed_size,
    };

    // `state` is the number of literals copied after the previous instruction:
    // 0 after a long literal run is still pending, 1-3 after a match's trailing
    // literals, 4 after a literal run. It decides what a 0-15 opcode means.
    let mut state: usize;

    // A first byte above 17 is a literal run with no preceding instruction.
    if input.first().is_some_and(|&b| b > 17) {
        let t = d.byte()? - 17;
        d.literals(t)?;
        state = if t < 4 { t } else { 4 };
    } else {
        state = 0;
    }

    loop {
        let t = d.byte()?;
        let (distance, length, next);

        if t < 16 {
            if state == 0 {
                // A run of literals.
                let mut run = t;
                if run == 0 {
                    run = 15 + d.extended_length()?;
                }
                d.literals(run + 3)?;
                state = 4;
                continue;
            } else if state != 4 {
                // A two-byte match within 1 KiB, after 1-3 literals.
                next = t & 3;
                distance = 1 + (t >> 2) + (d.byte()? << 2);
                length = 2;
            } else {
                // A three-byte match 2-3 KiB back, after a literal run.
                next = t & 3;
                distance = 1 + 0x0800 + (t >> 2) + (d.byte()? << 2);
                length = 3;
            }
        } else if t >= 64 {
            // M2: a short match within 2 KiB.
            next = t & 3;
            distance = 1 + ((t >> 2) & 7) + (d.byte()? << 3);
            length = (t >> 5) + 1;
        } else if t >= 32 {
            // M3: a match within 16 KiB.
            let mut len = t & 31;
            if len == 0 {
                len = 31 + d.extended_length()?;
            }
            length = len + 2;
            let word = d.le16()?;
            distance = 1 + (word >> 2);
            next = word & 3;
        } else {
            // M4: a match 16-48 KiB back, or the end-of-stream marker.
            let high = (t & 8) << 11;
            let mut len = t & 7;
            if len == 0 {
                len = 7 + d.extended_length()?;
            }
            length = len + 2;
            let word = d.le16()?;
            let low = word >> 2;
            next = word & 3;
            if high == 0 && low == 0 {
                break;
            }
            distance = high + low + 0x4000;
        }

        d.back_copy(distance, length)?;

        // Every match carries 0-3 trailing literals in its low two bits.
        d.literals(next)?;
        state = next;
    }

    if d.out.len() != decompressed_size {
        return Err(LzoError::ShortOutput {
            expected: decompressed_size,
            got: d.out.len(),
        });
    }
    Ok(d.out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const END: [u8; 3] = [0x11, 0x00, 0x00];

    fn stream(body: &[u8]) -> Vec<u8> {
        [body, &END].concat()
    }

    #[test]
    fn leading_literal_run() {
        // 17 + 5: five literals, then the end marker.
        let input = stream(&[22, b'h', b'e', b'l', b'l', b'o']);
        assert_eq!(decompress(&input, 5).unwrap(), b"hello");
    }

    #[test]
    fn literal_run_opcode() {
        // Opcode 1 with state 0: 1 + 3 = 4 literals.
        let input = stream(&[1, b'a', b'b', b'c', b'd']);
        assert_eq!(decompress(&input, 4).unwrap(), b"abcd");
    }

    #[test]
    fn long_literal_run_uses_zero_extension() {
        // Opcode 0, then one zero byte and 2: 255 + 2 + 15 + 3 = 275 literals.
        let literals: Vec<u8> = (0..275).map(|i| i as u8).collect();
        let mut body = vec![0, 0, 2];
        body.extend_from_slice(&literals);
        assert_eq!(decompress(&stream(&body), 275).unwrap(), literals);
    }

    #[test]
    fn m2_overlapping_match_repeats_pattern() {
        // "ab", then an M2 match: length (t>>5)+1 = 3 → t = 0b010_xxx_xx (64..95),
        // distance 2 → ((t>>2)&7) = 1, next byte 0, trailing literals 0.
        // t = 0b0100_0100 = 68.
        let input = stream(&[19, b'a', b'b', 68, 0]);
        assert_eq!(decompress(&input, 5).unwrap(), b"ababa");
    }

    #[test]
    fn m3_match_with_trailing_literal() {
        // Four literals "wxyz", then M3 (t = 32 + 2 → length 4) at distance 4,
        // trailing literal count 1, then literal '!'.
        // word = ((4 - 1) << 2) | 1 = 13.
        let input = stream(&[21, b'w', b'x', b'y', b'z', 34, 13, 0, b'!']);
        assert_eq!(decompress(&input, 9).unwrap(), b"wxyzwxyz!");
    }

    #[test]
    fn trailing_input_after_marker_is_ignored() {
        let mut input = stream(&[22, b'h', b'e', b'l', b'l', b'o']);
        input.extend_from_slice(&[0xAA; 16]);
        assert_eq!(decompress(&input, 5).unwrap(), b"hello");
    }

    #[test]
    fn match_before_start_is_rejected() {
        let input = stream(&[19, b'a', b'b', 68, 1]);
        assert!(matches!(
            decompress(&input, 64),
            Err(LzoError::LookBehindOverrun { .. })
        ));
    }

    #[test]
    fn truncated_input_is_rejected() {
        assert!(matches!(
            decompress(&[22, b'h', b'e'], 5),
            Err(LzoError::InputOverrun { .. })
        ));
    }

    #[test]
    fn output_overflow_is_rejected() {
        let input = stream(&[22, b'h', b'e', b'l', b'l', b'o']);
        assert!(matches!(
            decompress(&input, 3),
            Err(LzoError::OutputOverrun { .. })
        ));
    }

    #[test]
    fn short_output_is_rejected() {
        let input = stream(&[22, b'h', b'e', b'l', b'l', b'o']);
        assert!(matches!(
            decompress(&input, 6),
            Err(LzoError::ShortOutput { .. })
        ));
    }
}
