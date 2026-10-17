//! Cinepak (`cvid`): the Sierra logo movie's video.
//!
//! A frame is split into horizontal strips. Each strip carries (or keeps
//! from the last frame) two codebooks of 256 entries, each entry four
//! pixels' worth of colour; then, for every 4x4 block, either one "V1"
//! entry (each of its pixels drawn 2x2) or four "V4" entries (one per 2x2
//! quarter), or nothing, which keeps the last frame's block. Follows
//! FFmpeg's `cinepak.c` (LGPL-2.1+), for 24-bit video only.

/// One codebook entry: four pixels (top left, top right, bottom left,
/// bottom right) as RGB.
type Entry = [[u8; 3]; 4];

#[derive(Clone)]
struct Strip {
    v4: Box<[Entry; 256]>,
    v1: Box<[Entry; 256]>,
}

impl Default for Strip {
    fn default() -> Self {
        Strip {
            v4: Box::new([[[0; 3]; 4]; 256]),
            v1: Box::new([[[0; 3]; 4]; 256]),
        }
    }
}

/// Decodes a Cinepak movie's frames in order onto an RGB picture.
pub struct Cinepak {
    width: usize,
    height: usize,
    strips: Vec<Strip>,
    /// The picture, RGB, top row first.
    pub rgb: Vec<u8>,
}

const MAX_STRIPS: usize = 32;

fn be16(b: &[u8], at: usize) -> usize {
    b.get(at..at + 2)
        .map_or(0, |b| u16::from_be_bytes([b[0], b[1]]) as usize)
}

fn be24(b: &[u8], at: usize) -> usize {
    b.get(at..at + 3)
        .map_or(0, |b| (b[0] as usize) << 16 | (b[1] as usize) << 8 | b[2] as usize)
}

fn be32(b: &[u8], at: usize) -> Option<u32> {
    b.get(at..at + 4)
        .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

impl Cinepak {
    pub fn new(width: u32, height: u32) -> Self {
        Cinepak {
            width: width as usize,
            height: height as usize,
            strips: vec![Strip::default(); MAX_STRIPS],
            rgb: vec![0; width as usize * height as usize * 3],
        }
    }

    /// Draws one frame's data over the picture. Damaged data stops the
    /// frame where it goes wrong, leaving the rest as it was.
    pub fn decode(&mut self, data: &[u8]) {
        if data.len() < 10 {
            return;
        }
        let flags = data[0];
        let strips = be16(data, 8).min(MAX_STRIPS);
        let mut at = 10;
        let mut y0 = 0;
        for i in 0..strips {
            let Some(header) = data.get(at..at + 12) else {
                return;
            };
            // A top of zero means straight under the strip before.
            let (y1, y2) = match be16(header, 4) {
                0 => (y0, y0 + be16(header, 8)),
                top => (top, be16(header, 8)),
            };
            let (x1, x2) = (be16(header, 6), be16(header, 10));
            let Some(size) = be24(header, 1).checked_sub(12) else {
                return;
            };
            at += 12;
            let end = (at + size).min(data.len());
            // Without the flag, a strip starts from the last one's codebooks.
            if i > 0 && flags & 1 == 0 {
                self.strips[i] = self.strips[i - 1].clone();
            }
            if x2 > self.width.next_multiple_of(4)
                || y2 > self.height.next_multiple_of(4)
                || x1 >= x2
                || y1 >= y2
            {
                return;
            }
            self.strip(i, (x1, y1, x2, y2), &data[at..end]);
            at = end;
            y0 = y2;
        }
    }

    fn strip(&mut self, i: usize, area: (usize, usize, usize, usize), mut data: &[u8]) {
        while data.len() >= 4 {
            let id = data[0];
            let Some(size) = be24(data, 1).checked_sub(4) else {
                return;
            };
            let body = &data[4..(4 + size).min(data.len())];
            match id {
                0x20 | 0x21 | 0x24 | 0x25 => codebook(&mut self.strips[i].v4, id, body),
                0x22 | 0x23 | 0x26 | 0x27 => codebook(&mut self.strips[i].v1, id, body),
                0x30..=0x32 => {
                    self.vectors(i, area, id, body);
                    return;
                }
                _ => {}
            }
            data = &data[(4 + size).min(data.len())..];
        }
    }

    fn put(&mut self, x: usize, y: usize, rgb: [u8; 3]) {
        if x < self.width && y < self.height {
            let at = (y * self.width + x) * 3;
            self.rgb[at..at + 3].copy_from_slice(&rgb);
        }
    }

    /// The blocks of a strip. Chunk 0x30 flags each block as coded or
    /// kept, then as V1 or V4; 0x31 flags only which are V4 (all are
    /// coded); 0x32 codes every block as V1.
    fn vectors(&mut self, i: usize, (x1, y1, x2, y2): (usize, usize, usize, usize), id: u8, data: &[u8]) {
        let mut at = 0;
        let (mut flag, mut mask) = (0u32, 0u32);
        let next_flag = |at: &mut usize, flag: &mut u32, mask: &mut u32| -> bool {
            *mask >>= 1;
            if *mask == 0 {
                let Some(f) = be32(data, *at) else {
                    return false;
                };
                *flag = f;
                *at += 4;
                *mask = 0x8000_0000;
            }
            true
        };
        for y in (y1..y2).step_by(4) {
            for x in (x1..x2).step_by(4) {
                if id & 1 != 0 {
                    if !next_flag(&mut at, &mut flag, &mut mask) {
                        return;
                    }
                    if flag & mask == 0 {
                        continue;
                    }
                }
                let v1 = if id & 2 != 0 {
                    true
                } else {
                    if !next_flag(&mut at, &mut flag, &mut mask) {
                        return;
                    }
                    flag & mask == 0
                };
                if v1 {
                    let Some(&n) = data.get(at) else { return };
                    at += 1;
                    let e = self.strips[i].v1[n as usize];
                    for (k, rgb) in e.iter().enumerate() {
                        let (qx, qy) = (x + (k & 1) * 2, y + (k >> 1) * 2);
                        for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                            self.put(qx + dx, qy + dy, *rgb);
                        }
                    }
                } else {
                    let Some(n) = data.get(at..at + 4) else {
                        return;
                    };
                    at += 4;
                    let quarters = [n[0], n[1], n[2], n[3]].map(|n| self.strips[i].v4[n as usize]);
                    for (q, e) in quarters.iter().enumerate() {
                        let (qx, qy) = (x + (q & 1) * 2, y + (q >> 1) * 2);
                        for (k, rgb) in e.iter().enumerate() {
                            self.put(qx + (k & 1), qy + (k >> 1), *rgb);
                        }
                    }
                }
            }
        }
    }
}

/// Fills (or, with chunk bit 0, partly updates) a codebook. Entries are
/// four luma bytes, then (unless chunk bit 2 says greyscale) a signed
/// chroma pair applied to all four.
fn codebook(book: &mut [Entry; 256], id: u8, data: &[u8]) {
    let n = if id & 4 != 0 { 4 } else { 6 };
    let mut at = 0;
    let (mut flag, mut mask) = (0u32, 0u32);
    for entry in book.iter_mut() {
        if id & 1 != 0 {
            mask >>= 1;
            if mask == 0 {
                let Some(f) = be32(data, at) else { return };
                flag = f;
                at += 4;
                mask = 0x8000_0000;
            }
            if flag & mask == 0 {
                continue;
            }
        }
        let Some(b) = data.get(at..at + n) else {
            return;
        };
        at += n;
        let (u, v) = if n == 6 {
            (b[4] as i8 as i32, b[5] as i8 as i32)
        } else {
            (0, 0)
        };
        for (k, px) in entry.iter_mut().enumerate() {
            let y = b[k] as i32;
            *px = [
                (y + v * 2).clamp(0, 255) as u8,
                (y - u / 2 - v).clamp(0, 255) as u8,
                (y + u * 2).clamp(0, 255) as u8,
            ];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 4x4 frame of one strip whose single block is coded V1 from a
    /// one-entry codebook: four colours, each filling a 2x2 quarter.
    #[test]
    fn a_v1_block() {
        let mut frame = vec![0u8, 0, 0, 0, 0, 4, 0, 4, 0, 1];
        // Strip header: id 0x10, size, y1 0, x1 0, y2 4, x2 4.
        let codebook = [0x22u8, 0, 0, 10, 10, 20, 30, 40, 0, 0];
        let vectors = [0x32u8, 0, 0, 5, 0];
        let size = 12 + codebook.len() + vectors.len();
        frame.extend([0x10, 0, 0, size as u8, 0, 0, 0, 0, 0, 4, 0, 4]);
        frame.extend(codebook);
        frame.extend(vectors);
        let mut c = Cinepak::new(4, 4);
        c.decode(&frame);
        let px = |x: usize, y: usize| c.rgb[(y * 4 + x) * 3];
        assert_eq!([px(0, 0), px(1, 1), px(2, 0), px(0, 2), px(3, 3)], [10, 10, 20, 30, 40]);
    }
}
