//! Little-endian bit reader: Bink packs its fields starting from the lowest
//! bit of each byte, so the first bit read is bit 0 of byte 0.

/// Reads bit fields from a byte slice. Reading past the end yields zero
/// bits, like FFmpeg's padded buffers; `left` tells how many real bits
/// remain.
pub(crate) struct BitReader<'a> {
    data: &'a [u8],
    /// Position in bits from the start of `data`.
    pos: usize,
}

impl<'a> BitReader<'a> {
    pub(crate) fn new(data: &'a [u8]) -> Self {
        BitReader { data, pos: 0 }
    }

    /// Bits read so far.
    pub(crate) fn position(&self) -> usize {
        self.pos
    }

    /// Bits not yet read (negative once reads ran past the end).
    pub(crate) fn left(&self) -> isize {
        (self.data.len() * 8) as isize - self.pos as isize
    }

    /// The next 32 bits without consuming them, first bit lowest.
    #[inline]
    pub(crate) fn peek32(&self) -> u32 {
        let byte = self.pos >> 3;
        let window = if byte + 8 <= self.data.len() {
            u64::from_le_bytes(self.data[byte..byte + 8].try_into().unwrap())
        } else {
            let mut buf = [0u8; 8];
            if byte < self.data.len() {
                let rest = &self.data[byte..];
                buf[..rest.len()].copy_from_slice(rest);
            }
            u64::from_le_bytes(buf)
        };
        (window >> (self.pos & 7)) as u32
    }

    /// Reads `n` bits (0 to 32) as an unsigned number.
    #[inline]
    pub(crate) fn read(&mut self, n: u32) -> u32 {
        if n == 0 {
            return 0;
        }
        let v = self.peek32();
        self.pos += n as usize;
        if n == 32 { v } else { v & ((1 << n) - 1) }
    }

    #[inline]
    pub(crate) fn bit(&mut self) -> bool {
        self.read(1) != 0
    }

    pub(crate) fn skip(&mut self, n: usize) {
        self.pos += n;
    }

    /// Skips to the next multiple of 32 bits, where Bink starts each plane
    /// and each audio block.
    pub(crate) fn align32(&mut self) {
        self.pos = (self.pos + 31) & !31;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_lowest_bits_first() {
        let data = [0b1011_0101, 0xFF, 0x00, 0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC];
        let mut r = BitReader::new(&data);
        assert_eq!(r.read(1), 1);
        assert_eq!(r.read(2), 0b10);
        assert_eq!(r.read(5), 0b10110);
        assert_eq!(r.read(4), 0xF);
        assert_eq!(r.read(8), 0x0F);
        assert_eq!(r.position(), 20);
        r.align32();
        assert_eq!(r.position(), 32);
        assert_eq!(r.read(32), 0x9A78_5634);
        assert_eq!(r.left(), 8);
        assert_eq!(r.read(8), 0xBC);
        // Past the end everything reads as zero.
        assert_eq!(r.read(16), 0);
        assert!(r.left() < 0);
    }

    #[test]
    fn reads_across_byte_boundaries() {
        let data: Vec<u8> = (0..16u8).map(|i| i.wrapping_mul(37) ^ 0x5A).collect();
        // Compare with a bit-by-bit reference for every width at many offsets.
        let bit_at = |i: usize| (data[i / 8] >> (i % 8)) & 1;
        for start in 0..40 {
            for n in 1..=32u32 {
                let mut r = BitReader::new(&data);
                r.skip(start);
                let mut expect = 0u64;
                for b in 0..n as usize {
                    expect |= (bit_at(start + b) as u64) << b;
                }
                assert_eq!(r.read(n) as u64, expect, "start {start} n {n}");
            }
        }
    }
}
