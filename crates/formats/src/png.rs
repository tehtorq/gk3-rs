//! A minimal PNG encoder for exporting decoded images.

use crate::image::Image;

/// Encodes an image as PNG: RGBA when any pixel is transparent, RGB otherwise.
pub fn encode(image: &Image) -> Vec<u8> {
    let alpha = image.has_alpha();
    let channels = if alpha { 4 } else { 3 };
    let row_len = image.width as usize * channels;

    // Each scanline is prefixed with filter type 0 (none).
    let mut raw = Vec::with_capacity((row_len + 1) * image.height as usize);
    for row in image.rgba.chunks_exact(image.width as usize * 4) {
        raw.push(0);
        if alpha {
            raw.extend_from_slice(row);
        } else {
            for p in row.chunks_exact(4) {
                raw.extend_from_slice(&p[..3]);
            }
        }
    }

    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend(image.width.to_be_bytes());
    ihdr.extend(image.height.to_be_bytes());
    // Bit depth 8, colour type 6 (RGBA) or 2 (RGB), deflate, no filter, no interlace.
    ihdr.extend([8, if alpha { 6 } else { 2 }, 0, 0, 0]);

    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(
        &mut out,
        b"IDAT",
        &miniz_oxide::deflate::compress_to_vec_zlib(&raw, 6),
    );
    chunk(&mut out, b"IEND", &[]);
    out
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend((data.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend(kind);
    out.extend(data);
    let crc = crc32(&out[start..]);
    out.extend(crc.to_be_bytes());
}

fn crc32(bytes: &[u8]) -> u32 {
    const TABLE: [u32; 256] = {
        let mut table = [0u32; 256];
        let mut n = 0;
        while n < 256 {
            let mut c = n as u32;
            let mut k = 0;
            while k < 8 {
                c = if c & 1 != 0 {
                    0xEDB8_8320 ^ (c >> 1)
                } else {
                    c >> 1
                };
                k += 1;
            }
            table[n] = c;
            n += 1;
        }
        table
    };
    !bytes.iter().fold(!0u32, |c, &b| {
        TABLE[((c ^ b as u32) & 0xFF) as usize] ^ (c >> 8)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_matches_reference() {
        // CRC-32 of "IEND" is the well-known 0xAE426082.
        assert_eq!(crc32(b"IEND"), 0xAE42_6082);
    }

    #[test]
    fn round_trips_through_inflate() {
        let image = Image {
            width: 2,
            height: 1,
            rgba: vec![255, 0, 0, 255, 0, 0, 255, 128],
        };
        let png = encode(&image);
        assert!(png.starts_with(b"\x89PNG"));
        assert!(png.ends_with(&[0xAE, 0x42, 0x60, 0x82]));
        // IHDR says RGBA because of the half-transparent pixel.
        assert_eq!(png[25], 6);
        let idat_len = u32::from_be_bytes(png[33..37].try_into().unwrap()) as usize;
        let raw = miniz_oxide::inflate::decompress_to_vec_zlib(&png[41..41 + idat_len]).unwrap();
        assert_eq!(raw, [0, 255, 0, 0, 255, 0, 0, 255, 128]);
    }
}
