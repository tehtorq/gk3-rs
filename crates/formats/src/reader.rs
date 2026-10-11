//! Bounds-checked little-endian reading over a byte slice.

use crate::{Error, Result};

/// A cursor over asset bytes. Every read checks bounds and reports the offset
/// it failed at, so a misread count shows up as an error rather than garbage.
#[derive(Clone)]
pub struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }

    pub fn position(&self) -> usize {
        self.pos
    }

    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    pub fn remaining(&self) -> usize {
        self.bytes.len() - self.pos
    }

    /// Everything from the current position on, without consuming it.
    pub fn peek_rest(&self) -> &'a [u8] {
        &self.bytes[self.pos..]
    }

    pub fn seek(&mut self, pos: usize) -> Result<()> {
        if pos > self.bytes.len() {
            return Err(Error::truncated(pos, 0));
        }
        self.pos = pos;
        Ok(())
    }

    pub fn bytes(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self
            .pos
            .checked_add(n)
            .filter(|&end| end <= self.bytes.len())
            .ok_or_else(|| Error::truncated(self.pos, n))?;
        let slice = &self.bytes[self.pos..end];
        self.pos = end;
        Ok(slice)
    }

    pub fn skip(&mut self, n: usize) -> Result<()> {
        self.bytes(n).map(|_| ())
    }

    pub fn u8(&mut self) -> Result<u8> {
        Ok(self.bytes(1)?[0])
    }

    pub fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.bytes(2)?.try_into().unwrap()))
    }

    pub fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }

    pub fn i32(&mut self) -> Result<i32> {
        Ok(i32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }

    pub fn f32(&mut self) -> Result<f32> {
        Ok(f32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }

    pub fn vec2(&mut self) -> Result<[f32; 2]> {
        Ok([self.f32()?, self.f32()?])
    }

    pub fn vec3(&mut self) -> Result<[f32; 3]> {
        Ok([self.f32()?, self.f32()?, self.f32()?])
    }

    /// Reads a four-byte tag and fails unless it is `expected` as written on disk.
    pub fn tag(&mut self, expected: &[u8; 4]) -> Result<()> {
        let at = self.pos;
        let found = self.bytes(4)?;
        if found != expected {
            return Err(Error::Format(format!(
                "expected tag {} at offset {at}, found {}",
                printable(expected),
                printable(found)
            )));
        }
        Ok(())
    }

    /// A fixed-width, NUL-padded Latin-1 string.
    pub fn fixed_str(&mut self, width: usize) -> Result<String> {
        let raw = self.bytes(width)?;
        let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
        Ok(latin1(&raw[..end]))
    }

    /// Fails unless `count` items of `size` bytes each fit in what is left.
    /// Call before allocating from a count read out of the file.
    pub fn expect_room(&self, count: usize, size: usize) -> Result<()> {
        match count.checked_mul(size) {
            Some(n) if n <= self.remaining() => Ok(()),
            _ => Err(Error::Format(format!(
                "{count} items of {size} bytes at offset {} would run past the end ({} bytes left)",
                self.pos,
                self.remaining()
            ))),
        }
    }
}

pub fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| b as char).collect()
}

fn printable(tag: &[u8]) -> String {
    tag.iter()
        .map(|&b| if b.is_ascii_graphic() { b as char } else { '.' })
        .collect()
}
