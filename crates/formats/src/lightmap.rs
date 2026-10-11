//! Lightmap sets (`.MUL`): one bitmap per scene surface, packed back to back
//! with no offset table. `MULT` is written `TLUM`. Retail sets hold Windows
//! bitmaps; GK3's own format is accepted too.
//!
//! Sets are named after a scene plus a time of day: `RC1_A_M` is morning,
//! `_A` afternoon, `_E` evening, `_N` night. Lightmap *i* lights surface *i*.

use crate::image::{self, Image};
use crate::reader::Reader;
use crate::{Error, Result};

pub fn parse(bytes: &[u8]) -> Result<Vec<Image>> {
    let mut r = Reader::new(bytes);
    r.tag(b"TLUM")?;
    let count = r.u32()? as usize;
    // Each bitmap needs at least its 8-byte header.
    r.expect_room(count, 8)?;
    let maps = (0..count)
        .map(|_| image::read_packed(&mut r))
        .collect::<Result<Vec<_>>>()?;
    if r.remaining() != 0 {
        return Err(Error::Format(format!(
            "{} bytes left after {count} lightmaps",
            r.remaining()
        )));
    }
    Ok(maps)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_packed_bitmaps_exactly() {
        let mut bytes = b"TLUM".to_vec();
        bytes.extend(2u32.to_le_bytes());
        // 1x1, then 1x3 (odd width: one padding pixel per row).
        bytes.extend(b"61nM\x01\x00\x01\x00\xFF\xFF\x00\x00");
        bytes.extend(b"61nM\x01\x00\x03\x00");
        bytes.extend([0u8; 8]);
        let maps = parse(&bytes).unwrap();
        assert_eq!(maps.len(), 2);
        assert_eq!((maps[1].width, maps[1].height), (3, 1));
    }

    #[test]
    fn leftover_bytes_mean_a_misread() {
        let mut bytes = b"TLUM\x01\x00\x00\x00".to_vec();
        bytes.extend(b"61nM\x01\x00\x01\x00\xFF\xFF\x00\x00\x00\x00");
        assert!(parse(&bytes).is_err());
    }
}
