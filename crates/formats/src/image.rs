//! Bitmaps: GK3's own 16-bit format and Windows BMP.

use crate::reader::Reader;
use crate::{Error, Result};

/// An 8-bit RGBA image, top row first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Image {
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * self.width + x) * 4) as usize;
        self.rgba[i..i + 4].try_into().unwrap()
    }

    /// Whether any pixel is less than fully opaque.
    pub fn has_alpha(&self) -> bool {
        self.rgba.chunks_exact(4).any(|p| p[3] != 255)
    }

    /// GK3's colour key: when the top-left pixel is magenta, every magenta
    /// pixel is transparent. Images without the marker keep magenta, which some
    /// artwork uses as an ordinary colour.
    fn apply_color_key(&mut self) {
        if self.rgba.len() < 4 || self.rgba[..3] != [255, 0, 255] {
            return;
        }
        for p in self.rgba.chunks_exact_mut(4) {
            if p[..3] == [255, 0, 255] {
                p[3] = 0;
            }
        }
    }
}

/// A palettised image whose indices carry meaning, such as a walk boundary
/// map where the index is the region.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedImage {
    pub width: u32,
    pub height: u32,
    /// One index per pixel, top row first.
    pub indices: Vec<u8>,
    /// RGB colours the indices refer to.
    pub palette: Vec<[u8; 3]>,
}

/// Decodes a GK3 or Windows bitmap, applying the magenta colour key.
pub fn decode(bytes: &[u8]) -> Result<Image> {
    let mut reader = Reader::new(bytes);
    let mut image = if bytes.starts_with(b"61nM") {
        read_gk3(&mut reader)?
    } else if bytes.starts_with(b"BM") {
        let bmp = read_bmp(bytes)?;
        match bmp {
            Bmp::Rgb(image) => image,
            Bmp::Indexed(indexed) => indexed.to_rgba(),
        }
    } else {
        return Err(Error::Format("not a GK3 or Windows bitmap".into()));
    };
    image.apply_color_key();
    Ok(image)
}

/// Decodes an 8-bit Windows bitmap as indices rather than colours.
pub fn decode_indexed(bytes: &[u8]) -> Result<IndexedImage> {
    match read_bmp(bytes)? {
        Bmp::Indexed(indexed) => Ok(indexed),
        Bmp::Rgb(_) => Err(Error::Format("bitmap is not palettised".into())),
    }
}

/// Reads one GK3 bitmap at the reader's position and leaves the reader just
/// past it. Lightmap sets pack these back to back with no offset table, so the
/// size has to come out exactly right.
pub fn read_gk3(reader: &mut Reader) -> Result<Image> {
    reader.tag(b"61nM")?;
    // Height comes first. Square textures hide a mix-up; 200AM.BMP does not.
    let height = reader.u16()? as u32;
    let width = reader.u16()? as u32;
    // Rows of odd width are padded by one pixel.
    let stride = width + (width & 1);
    reader.expect_room(stride as usize * height as usize, 2)?;

    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for _ in 0..height {
        for _ in 0..width {
            rgba.extend_from_slice(&rgb565(reader.u16()?));
        }
        if width & 1 == 1 {
            reader.skip(2)?;
        }
    }
    Ok(Image {
        width,
        height,
        rgba,
    })
}

/// Reads one bitmap of either kind at the reader's position, leaving the
/// reader just past it. Used where bitmaps are packed back to back.
pub fn read_packed(reader: &mut Reader) -> Result<Image> {
    let rest = &reader.peek_rest();
    if rest.starts_with(b"61nM") {
        return read_gk3(reader);
    }
    if !rest.starts_with(b"BM") {
        return Err(Error::Format(format!(
            "no bitmap at offset {}",
            reader.position()
        )));
    }
    let size = bmp_size(rest)?;
    let image = decode(reader.bytes(size)?)?;
    Ok(image)
}

/// A Windows bitmap's length measured from its header: where the pixels
/// start plus the padded rows.
fn bmp_size(bytes: &[u8]) -> Result<usize> {
    let mut r = Reader::new(bytes);
    r.seek(10)?;
    let pixel_offset = r.u32()? as usize;
    r.skip(4)?;
    let width = r.i32()?.unsigned_abs() as usize;
    let height = r.i32()?.unsigned_abs() as usize;
    r.skip(2)?;
    let bits = r.u16()? as usize;
    let row_bytes = (bits * width).div_ceil(32) * 4;
    Ok(pixel_offset + row_bytes * height)
}

/// Expands a 5-6-5 pixel. Scaling rather than shifting, so full intensity
/// becomes 255 and white stays white.
fn rgb565(pixel: u16) -> [u8; 4] {
    let r = (pixel >> 11) as u32 & 0x1F;
    let g = (pixel >> 5) as u32 & 0x3F;
    let b = pixel as u32 & 0x1F;
    [
        (r * 255 / 31) as u8,
        (g * 255 / 63) as u8,
        (b * 255 / 31) as u8,
        255,
    ]
}

enum Bmp {
    Rgb(Image),
    Indexed(IndexedImage),
}

impl IndexedImage {
    fn to_rgba(&self) -> Image {
        let mut rgba = Vec::with_capacity(self.indices.len() * 4);
        for &i in &self.indices {
            let [r, g, b] = self.palette.get(i as usize).copied().unwrap_or([0, 0, 0]);
            rgba.extend_from_slice(&[r, g, b, 255]);
        }
        Image {
            width: self.width,
            height: self.height,
            rgba,
        }
    }
}

fn read_bmp(bytes: &[u8]) -> Result<Bmp> {
    if !bytes.starts_with(b"BM") {
        return Err(Error::Format("not a Windows bitmap".into()));
    }
    let mut r = Reader::new(bytes);
    r.seek(10)?;
    let pixel_offset = r.u32()? as usize;
    let header_size = r.u32()?;
    if header_size < 40 {
        return Err(Error::Unsupported(format!(
            "BMP header of {header_size} bytes"
        )));
    }
    let width = r.i32()?;
    let raw_height = r.i32()?;
    let planes = r.u16()?;
    let bits = r.u16()?;
    let compression = r.u32()?;
    r.skip(12)?;
    let colors_used = r.u32()?;

    if width <= 0 || raw_height == 0 || planes != 1 {
        return Err(Error::Format(format!(
            "BMP of {width}x{raw_height} with {planes} planes"
        )));
    }
    if compression != 0 {
        return Err(Error::Unsupported(format!(
            "BMP compression method {compression}"
        )));
    }
    let width = width as u32;
    // A negative height means rows are stored top-down.
    let top_down = raw_height < 0;
    let height = raw_height.unsigned_abs();

    let row_bytes = (bits as usize * width as usize).div_ceil(32) * 4;
    let row_at = |y: u32| -> usize {
        let stored = if top_down { y } else { height - 1 - y };
        pixel_offset + stored as usize * row_bytes
    };
    let needed = pixel_offset + row_bytes * height as usize;
    if bytes.len() < needed {
        return Err(Error::truncated(bytes.len(), needed - bytes.len()));
    }

    match bits {
        8 => {
            r.seek(14 + header_size as usize)?;
            let count = if colors_used == 0 {
                256
            } else {
                colors_used.min(256) as usize
            };
            let mut palette = Vec::with_capacity(count);
            for _ in 0..count {
                let bgra = r.bytes(4)?;
                palette.push([bgra[2], bgra[1], bgra[0]]);
            }
            let mut indices = Vec::with_capacity((width * height) as usize);
            for y in 0..height {
                let row = row_at(y);
                indices.extend_from_slice(&bytes[row..row + width as usize]);
            }
            Ok(Bmp::Indexed(IndexedImage {
                width,
                height,
                indices,
                palette,
            }))
        }
        24 | 32 => {
            let step = bits as usize / 8;
            let mut rgba = Vec::with_capacity((width * height * 4) as usize);
            for y in 0..height {
                let row = row_at(y);
                for x in 0..width as usize {
                    let p = &bytes[row + x * step..row + x * step + 3];
                    // BI_RGB stores no alpha even at 32 bits.
                    rgba.extend_from_slice(&[p[2], p[1], p[0], 255]);
                }
            }
            Ok(Bmp::Rgb(Image {
                width,
                height,
                rgba,
            }))
        }
        other => Err(Error::Unsupported(format!("{other}-bit BMP"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gk3(height: u16, width: u16, pixels: &[u16], pad: bool) -> Vec<u8> {
        let mut out = b"61nM".to_vec();
        out.extend(height.to_le_bytes());
        out.extend(width.to_le_bytes());
        for row in pixels.chunks(width as usize) {
            for p in row {
                out.extend(p.to_le_bytes());
            }
            if pad {
                out.extend([0xEE, 0xEE]);
            }
        }
        out
    }

    #[test]
    fn gk3_height_comes_first() {
        // 1 high, 2 wide: red then blue.
        let image = decode(&gk3(1, 2, &[0xF800, 0x001F], false)).unwrap();
        assert_eq!((image.width, image.height), (2, 1));
        assert_eq!(image.pixel(0, 0), [255, 0, 0, 255]);
        assert_eq!(image.pixel(1, 0), [0, 0, 255, 255]);
    }

    #[test]
    fn gk3_white_is_white() {
        let image = decode(&gk3(1, 2, &[0xFFFF, 0xFFFF], false)).unwrap();
        assert_eq!(image.pixel(0, 0), [255, 255, 255, 255]);
    }

    #[test]
    fn gk3_odd_rows_are_padded() {
        // 3 wide, 2 high: the padding must not shear the second row.
        let bytes = gk3(
            2,
            3,
            &[0xF800, 0xF800, 0xF800, 0x07E0, 0x07E0, 0x07E0],
            true,
        );
        let mut reader = Reader::new(&bytes);
        let image = read_gk3(&mut reader).unwrap();
        assert_eq!(reader.remaining(), 0, "consumed exactly");
        assert_eq!(image.pixel(0, 1), [0, 255, 0, 255]);
        assert_eq!(image.pixel(2, 1), [0, 255, 0, 255]);
    }

    #[test]
    fn color_key_only_with_marker() {
        let keyed = decode(&gk3(1, 2, &[0xF81F, 0xF81F], false)).unwrap();
        assert_eq!(keyed.pixel(1, 0)[3], 0);
        let unkeyed = decode(&gk3(1, 2, &[0x0000, 0xF81F], false)).unwrap();
        assert_eq!(unkeyed.pixel(1, 0)[3], 255);
    }

    fn bmp(width: i32, height: i32, bits: u16, palette: &[[u8; 4]], rows: &[&[u8]]) -> Vec<u8> {
        let header = 14 + 40 + palette.len() * 4;
        let mut out = b"BM".to_vec();
        out.extend(0u32.to_le_bytes());
        out.extend(0u32.to_le_bytes());
        out.extend((header as u32).to_le_bytes());
        out.extend(40u32.to_le_bytes());
        out.extend(width.to_le_bytes());
        out.extend(height.to_le_bytes());
        out.extend(1u16.to_le_bytes());
        out.extend(bits.to_le_bytes());
        out.extend([0u8; 16]);
        out.extend((palette.len() as u32).to_le_bytes());
        out.extend(0u32.to_le_bytes());
        for p in palette {
            out.extend(p);
        }
        for row in rows {
            out.extend(*row);
        }
        out
    }

    #[test]
    fn bmp_24_bit_is_bottom_up_and_bgr() {
        // 1x2; the stored first row is the bottom one. Rows pad to 4 bytes.
        let bytes = bmp(1, 2, 24, &[], &[&[255, 0, 0, 0], &[0, 0, 255, 0]]);
        let image = decode(&bytes).unwrap();
        assert_eq!(image.pixel(0, 0), [255, 0, 0, 255], "top is red");
        assert_eq!(image.pixel(0, 1), [0, 0, 255, 255], "bottom is blue");
    }

    #[test]
    fn bmp_8_bit_indexed() {
        let palette = [[0, 0, 0, 0], [0, 255, 0, 0]];
        let bytes = bmp(2, 1, 8, &palette, &[&[1, 0, 0, 0]]);
        let indexed = decode_indexed(&bytes).unwrap();
        assert_eq!(indexed.indices, vec![1, 0]);
        assert_eq!(decode(&bytes).unwrap().pixel(0, 0), [0, 255, 0, 255]);
    }

    #[test]
    fn truncated_images_are_errors() {
        let mut bytes = gk3(2, 2, &[1, 2, 3, 4], false);
        bytes.truncate(bytes.len() - 1);
        assert!(decode(&bytes).is_err());
        let bytes = bmp(4, 4, 24, &[], &[&[0; 12]]);
        assert!(decode(&bytes).is_err());
    }
}
