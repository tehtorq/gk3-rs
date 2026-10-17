//! Bink 1 video: each frame is three 8-bit planes (Y at full size, U and V
//! at half size), each coded as a grid of 8x8 blocks.
//!
//! Every block has a type: copied from the previous frame (skip, motion),
//! filled with one colour, two colours in a bit pattern, runs of colours in
//! a scan order, raw pixels, an intra DCT, or a motion copy plus a DCT or a
//! residue correction. A "scaled" block covers 16x16 pixels and is drawn at
//! 8x8 and doubled. The values these blocks need (types, colours, motion
//! vectors, DC values, run lengths, patterns) are not stored inline: they
//! come from nine "bundles", each refilled at the start of a block row from
//! the bitstream and read in order as blocks consume them.

use crate::bits::BitReader;
use crate::tables::{INTER_QUANT, INTRA_QUANT, PATTERNS, SCAN, TREE_CODES, TREE_LENS};
use crate::{Error, err};

/// Header flag: the video has an alpha plane.
pub(crate) const FLAG_ALPHA: u32 = 0x0010_0000;

/// Block types.
const SKIP_BLOCK: i32 = 0;
const SCALED_BLOCK: i32 = 1;
const MOTION_BLOCK: i32 = 2;
const RUN_BLOCK: i32 = 3;
const RESIDUE_BLOCK: i32 = 4;
const INTRA_BLOCK: i32 = 5;
const FILL_BLOCK: i32 = 6;
const INTER_BLOCK: i32 = 7;
const PATTERN_BLOCK: i32 = 8;
const RAW_BLOCK: i32 = 9;

/// Bundles, in the order they are refilled.
const SRC_BLOCK_TYPES: usize = 0;
const SRC_SUB_BLOCK_TYPES: usize = 1;
const SRC_COLORS: usize = 2;
const SRC_PATTERN: usize = 3;
const SRC_X_OFF: usize = 4;
const SRC_Y_OFF: usize = 5;
const SRC_INTRA_DC: usize = 6;
const SRC_INTER_DC: usize = 7;
const SRC_RUN: usize = 8;
const NB_SRC: usize = 9;

/// Bits of the first DC value in a DC bundle refill.
const DC_START_BITS: u32 = 11;

/// Run lengths that block-type symbols 12 to 15 stand for.
const RLE_LENS: [usize; 4] = [4, 8, 12, 32];

/// Lookup table for one of the sixteen fixed Huffman trees: indexed by the
/// next `bits` bits of the stream, it gives the leaf and its code length.
struct Vlc {
    bits: u32,
    table: Vec<(u8, u8)>,
}

impl Vlc {
    fn new(tree: usize) -> Vlc {
        let bits = TREE_LENS[tree][15] as u32;
        let mut table = vec![(0, 0); 1 << bits];
        for leaf in 0..16 {
            let code = TREE_CODES[tree][leaf] as usize;
            let len = TREE_LENS[tree][leaf] as u32;
            // Codes are read lowest bit first, so the bits after the code
            // are the high bits of the index.
            for ext in 0..1usize << (bits - len) {
                table[code | (ext << len)] = (leaf as u8, len as u8);
            }
        }
        Vlc { bits, table }
    }
}

/// A Huffman tree as a bundle uses it: one of the fixed tree shapes and a
/// mapping from its leaves to 4-bit symbols, both read from the stream.
#[derive(Clone, Copy, Default)]
struct Tree {
    vlc: usize,
    syms: [u8; 16],
}

/// Reads a 4-bit symbol with `tree`.
#[inline]
fn get_huff(r: &mut BitReader, vlcs: &[Vlc], tree: &Tree) -> i32 {
    let vlc = &vlcs[tree.vlc];
    let (leaf, len) = vlc.table[(r.peek32() & ((1 << vlc.bits) - 1)) as usize];
    r.skip(len as usize);
    tree.syms[leaf as usize] as i32
}

/// Merges two runs of `size` symbols, choosing from either by one bit
/// each, for the "shuffled" tree symbol mapping.
fn merge(r: &mut BitReader, dst: &mut [u8], src: &[u8], size: usize) {
    let (mut a, mut b) = (0, size);
    let (mut left_a, mut left_b) = (size, size);
    let mut out = 0;
    while left_a > 0 && left_b > 0 {
        if !r.bit() {
            dst[out] = src[a];
            a += 1;
            left_a -= 1;
        } else {
            dst[out] = src[b];
            b += 1;
            left_b -= 1;
        }
        out += 1;
    }
    for _ in 0..left_a {
        dst[out] = src[a];
        a += 1;
        out += 1;
    }
    for _ in 0..left_b {
        dst[out] = src[b];
        b += 1;
        out += 1;
    }
}

/// Reads which tree shape a bundle uses and how its leaves map to symbols.
fn read_tree(r: &mut BitReader) -> Result<Tree, Error> {
    if r.left() < 4 {
        return Err(err("bink: truncated tree"));
    }
    let mut tree = Tree {
        vlc: r.read(4) as usize,
        syms: [0; 16],
    };
    if tree.vlc == 0 {
        for (i, s) in tree.syms.iter_mut().enumerate() {
            *s = i as u8;
        }
        return Ok(tree);
    }
    if r.bit() {
        // Some symbols listed explicitly, the rest in ascending order.
        let mut len = r.read(3) as usize;
        let mut used = [false; 16];
        for i in 0..=len {
            tree.syms[i] = r.read(4) as u8;
            used[tree.syms[i] as usize] = true;
        }
        for (i, &u) in used.iter().enumerate() {
            if len >= 15 {
                break;
            }
            if !u {
                len += 1;
                tree.syms[len] = i as u8;
            }
        }
    } else {
        // The identity order shuffled by up to four rounds of merges.
        let len = r.read(2) as usize;
        let mut a: [u8; 16] = std::array::from_fn(|i| i as u8);
        let mut b = [0u8; 16];
        for i in 0..=len {
            let size = 1 << i;
            for t in (0..16).step_by(size << 1) {
                merge(r, &mut b[t..], &a[t..], size);
            }
            std::mem::swap(&mut a, &mut b);
        }
        tree.syms = a;
    }
    Ok(tree)
}

/// One kind of value blocks draw from. Values are decoded a block row at a
/// time into `data` and consumed from `ptr`.
#[derive(Default)]
struct Bundle {
    /// Bits of the count at the start of each refill.
    len_bits: u32,
    tree: Tree,
    data: Vec<i16>,
    /// How many values `data` may hold.
    cap: usize,
    /// End of the decoded values; `None` once the bundle signalled that it
    /// has no more values in this plane.
    dec: Option<usize>,
    /// Next value to hand out.
    ptr: usize,
}

impl Bundle {
    /// Whether a refill should be read now: only once every decoded value
    /// has been used. Returns the number of values to decode, or `None`.
    fn refill_count(&mut self, r: &mut BitReader) -> Option<usize> {
        let dec = self.dec?;
        if dec > self.ptr {
            return None;
        }
        let t = r.read(self.len_bits) as usize;
        if t == 0 {
            self.dec = None;
            return None;
        }
        Some(t)
    }

    /// Next value; 0 once the bundle is exhausted (a corrupt stream).
    #[inline]
    fn next(&mut self) -> i32 {
        let v = self.data.get(self.ptr).copied().unwrap_or(0);
        self.ptr += 1;
        v as i32
    }
}

/// floor(log2(x)) for x > 0.
fn log2(x: usize) -> u32 {
    usize::BITS - 1 - x.leading_zeros()
}

/// A plane's pixels with room for whole (and scaled 16x16) blocks.
#[derive(Clone)]
struct Plane {
    data: Vec<u8>,
    stride: usize,
    /// Size in pixels.
    width: usize,
    height: usize,
    /// Size in 8x8 blocks.
    bw: usize,
    bh: usize,
}

impl Plane {
    fn new(width: usize, height: usize, bw: usize, bh: usize, fill: u8) -> Plane {
        // Rounded up to an even number of blocks so a scaled block at the
        // edge stays inside.
        let stride = bw.next_multiple_of(2) * 8;
        let rows = bh.next_multiple_of(2) * 8;
        Plane {
            data: vec![fill; stride * rows],
            stride,
            width,
            height,
            bw,
            bh,
        }
    }
}

/// The Bink video decoder's state between frames.
pub(crate) struct VideoDecoder {
    /// Revision letter of the file (`b'f'` for GK3).
    version: u8,
    has_alpha: bool,
    swap_planes: bool,
    vlcs: Vec<Vlc>,
    bundles: [Bundle; NB_SRC],
    /// Trees for the high nibble of colours, chosen by the previous one.
    col_high: [Tree; 16],
    col_lastval: usize,
    /// Planes Y, U, V and, if present, alpha: being decoded, and the
    /// previous frame.
    cur: Vec<Plane>,
    prev: Vec<Plane>,
}

impl VideoDecoder {
    pub(crate) fn new(width: u32, height: u32, version: u8, flags: u32) -> VideoDecoder {
        let (w, h) = (width as usize, height as usize);
        let has_alpha = flags & FLAG_ALPHA != 0;
        let (bw, bh) = ((w + 7) >> 3, (h + 7) >> 3);
        let (cbw, cbh) = ((w + 15) >> 4, (h + 15) >> 4);
        let mut planes = vec![
            Plane::new(w, h, bw, bh, 16),
            Plane::new(w >> 1, h >> 1, cbw, cbh, 128),
            Plane::new(w >> 1, h >> 1, cbw, cbh, 128),
        ];
        if has_alpha {
            planes.push(Plane::new(w, h, bw, bh, 255));
        }
        let blocks = bw * bh;
        let bundles = std::array::from_fn(|i| Bundle {
            data: vec![0; blocks * 64],
            // DC values are 16-bit, so their share of FFmpeg's byte buffer
            // holds half as many.
            cap: if i == SRC_INTRA_DC || i == SRC_INTER_DC {
                blocks * 32
            } else {
                blocks * 64
            },
            ..Bundle::default()
        });
        VideoDecoder {
            version,
            has_alpha,
            swap_planes: version >= b'h',
            vlcs: (0..16).map(Vlc::new).collect(),
            bundles,
            col_high: [Tree::default(); 16],
            col_lastval: 0,
            prev: planes.clone(),
            cur: planes,
        }
    }

    /// The last decoded frame's planes: Y, U, V and maybe alpha, each as
    /// (pixels, stride).
    pub(crate) fn planes(&self) -> Vec<(&[u8], usize)> {
        self.prev.iter().map(|p| (&p.data[..], p.stride)).collect()
    }

    /// Full-range (JPEG) levels instead of studio range.
    pub(crate) fn full_range(&self) -> bool {
        self.version == b'k'
    }

    /// Decodes one video packet over the previous frame.
    pub(crate) fn decode(&mut self, packet: &[u8]) -> Result<(), Error> {
        let mut r = BitReader::new(packet);
        let bits = packet.len() * 8;
        if self.has_alpha {
            if self.version >= b'i' {
                r.skip(32);
            }
            self.decode_plane(&mut r, 3)?;
        }
        if self.version >= b'i' {
            r.skip(32);
        }
        let mut done = 0;
        for plane in 0..3 {
            let idx = if plane == 0 || !self.swap_planes {
                plane
            } else {
                plane ^ 3
            };
            self.decode_plane(&mut r, idx)?;
            done += 1;
            if r.position() >= bits {
                break;
            }
        }
        // Planes the packet ended before keep the previous picture.
        for plane in done..3 {
            let idx = if plane == 0 || !self.swap_planes {
                plane
            } else {
                plane ^ 3
            };
            let (cur, prev) = (&mut self.cur[idx], &self.prev[idx]);
            cur.data.copy_from_slice(&prev.data);
        }
        std::mem::swap(&mut self.cur, &mut self.prev);
        Ok(())
    }

    fn init_lengths(&mut self, width: usize, bw: usize) {
        let width = width.next_multiple_of(8);
        let b = &mut self.bundles;
        b[SRC_BLOCK_TYPES].len_bits = log2((width >> 3) + 511) + 1;
        b[SRC_SUB_BLOCK_TYPES].len_bits = log2((width >> 4) + 511) + 1;
        b[SRC_COLORS].len_bits = log2(bw * 64 + 511) + 1;
        let l = log2((width >> 3) + 511) + 1;
        for i in [SRC_INTRA_DC, SRC_INTER_DC, SRC_X_OFF, SRC_Y_OFF] {
            b[i].len_bits = l;
        }
        b[SRC_PATTERN].len_bits = log2((bw << 3) + 511) + 1;
        b[SRC_RUN].len_bits = log2(bw * 48 + 511) + 1;
    }

    fn read_bundle(&mut self, r: &mut BitReader, n: usize) -> Result<(), Error> {
        if n == SRC_COLORS {
            for i in 0..16 {
                self.col_high[i] = read_tree(r)?;
            }
            self.col_lastval = 0;
        }
        if n != SRC_INTRA_DC && n != SRC_INTER_DC {
            self.bundles[n].tree = read_tree(r)?;
        }
        self.bundles[n].dec = Some(0);
        self.bundles[n].ptr = 0;
        Ok(())
    }

    /// Refills of the simple bundles: runs and motion values (`signed`).
    fn read_values(&mut self, r: &mut BitReader, n: usize, signed: bool) -> Result<(), Error> {
        let b = &mut self.bundles[n];
        let Some(t) = b.refill_count(r) else {
            return Ok(());
        };
        let start = b.dec.unwrap_or(0);
        let end = start + t;
        if end > b.cap {
            return Err(err("bink: too many bundle values"));
        }
        if r.left() < 1 {
            return Err(err("bink: truncated bundle"));
        }
        let read_sign =
            |r: &mut BitReader, v: i32| if v != 0 && signed && r.bit() { -v } else { v };
        if r.bit() {
            let v = r.read(4) as i32;
            let v = read_sign(r, v);
            b.data[start..end].fill(v as i16);
        } else {
            for i in start..end {
                let v = get_huff(r, &self.vlcs, &b.tree);
                b.data[i] = read_sign(r, v) as i16;
            }
        }
        b.dec = Some(end);
        Ok(())
    }

    fn read_block_types(&mut self, r: &mut BitReader, n: usize) -> Result<(), Error> {
        let b = &mut self.bundles[n];
        let Some(mut t) = b.refill_count(r) else {
            return Ok(());
        };
        if self.version == b'k' {
            t ^= 0xBB;
            if t == 0 {
                b.dec = None;
                return Ok(());
            }
        }
        let start = b.dec.unwrap_or(0);
        let end = start + t;
        if end > b.cap {
            return Err(err("bink: too many block type values"));
        }
        if r.left() < 1 {
            return Err(err("bink: truncated block types"));
        }
        if r.bit() {
            let v = r.read(4) as i16;
            b.data[start..end].fill(v);
        } else {
            let mut last = 0;
            let mut i = start;
            while i < end {
                let v = get_huff(r, &self.vlcs, &b.tree);
                if v < 12 {
                    last = v as i16;
                    b.data[i] = last;
                    i += 1;
                } else {
                    // Repeat the previous type.
                    let run = RLE_LENS[(v - 12) as usize];
                    if end - i < run {
                        return Err(err("bink: block type run too long"));
                    }
                    b.data[i..i + run].fill(last);
                    i += run;
                }
            }
        }
        b.dec = Some(end);
        Ok(())
    }

    fn read_patterns(&mut self, r: &mut BitReader) -> Result<(), Error> {
        let b = &mut self.bundles[SRC_PATTERN];
        let Some(t) = b.refill_count(r) else {
            return Ok(());
        };
        let start = b.dec.unwrap_or(0);
        let end = start + t;
        if end > b.cap {
            return Err(err("bink: too many pattern values"));
        }
        for i in start..end {
            if r.left() < 2 {
                return Err(err("bink: truncated patterns"));
            }
            let lo = get_huff(r, &self.vlcs, &b.tree);
            let hi = get_huff(r, &self.vlcs, &b.tree);
            b.data[i] = (lo | (hi << 4)) as i16;
        }
        b.dec = Some(end);
        Ok(())
    }

    /// One colour: the high nibble with a tree chosen by the previous high
    /// nibble, then the low nibble. Before revision 'i' colours are stored
    /// as sign and magnitude around 128.
    #[inline]
    fn read_color(&mut self, r: &mut BitReader) -> i16 {
        self.col_lastval = get_huff(r, &self.vlcs, &self.col_high[self.col_lastval]) as usize;
        let lo = get_huff(r, &self.vlcs, &self.bundles[SRC_COLORS].tree);
        let mut v = ((self.col_lastval as i32) << 4) | lo;
        if self.version < b'i' {
            let sign = ((v as u8 as i8) >> 7) as i32;
            v = ((v & 0x7F) ^ sign) - sign;
            v += 0x80;
        }
        v as u8 as i16
    }

    fn read_colors(&mut self, r: &mut BitReader) -> Result<(), Error> {
        let Some(t) = self.bundles[SRC_COLORS].refill_count(r) else {
            return Ok(());
        };
        let start = self.bundles[SRC_COLORS].dec.unwrap_or(0);
        let end = start + t;
        if end > self.bundles[SRC_COLORS].cap {
            return Err(err("bink: too many colour values"));
        }
        if r.left() < 1 {
            return Err(err("bink: truncated colours"));
        }
        if r.bit() {
            let v = self.read_color(r);
            self.bundles[SRC_COLORS].data[start..end].fill(v);
        } else {
            for i in start..end {
                if r.left() < 2 {
                    return Err(err("bink: truncated colours"));
                }
                let v = self.read_color(r);
                self.bundles[SRC_COLORS].data[i] = v;
            }
        }
        self.bundles[SRC_COLORS].dec = Some(end);
        Ok(())
    }

    /// DC values: a first value, then groups of up to eight deltas each
    /// with its own bit width.
    fn read_dcs(&mut self, r: &mut BitReader, n: usize, has_sign: bool) -> Result<(), Error> {
        let b = &mut self.bundles[n];
        let Some(mut len) = b.refill_count(r) else {
            return Ok(());
        };
        let start_bits = DC_START_BITS - has_sign as u32;
        if r.left() < start_bits as isize {
            return Err(err("bink: truncated DC values"));
        }
        let mut v = r.read(start_bits) as i32;
        if v != 0 && has_sign && r.bit() {
            v = -v;
        }
        let mut dst = b.dec.unwrap_or(0);
        if dst >= b.cap {
            return Err(err("bink: too many DC values"));
        }
        b.data[dst] = v as i16;
        dst += 1;
        len -= 1;
        for i in (0..len).step_by(8) {
            let len2 = (len - i).min(8);
            if b.cap - dst < len2 {
                return Err(err("bink: too many DC values"));
            }
            let bsize = r.read(4);
            if bsize != 0 {
                for _ in 0..len2 {
                    let mut v2 = r.read(bsize) as i32;
                    if v2 != 0 && r.bit() {
                        v2 = -v2;
                    }
                    v += v2;
                    b.data[dst] = v as i16;
                    dst += 1;
                    if !(-32768..=32767).contains(&v) {
                        return Err(err("bink: DC value out of range"));
                    }
                }
            } else {
                b.data[dst..dst + len2].fill(v as i16);
                dst += len2;
            }
        }
        b.dec = Some(dst);
        Ok(())
    }

    /// Decodes plane `idx` into `self.cur`, reading motion sources from
    /// `self.prev`.
    fn decode_plane(&mut self, r: &mut BitReader, idx: usize) -> Result<(), Error> {
        let geo = self.prev[idx].geometry();
        let mut cur = std::mem::take(&mut self.cur[idx].data);
        let prev = std::mem::take(&mut self.prev[idx].data);
        let result = self.decode_plane_into(r, &mut cur, geo, &prev);
        self.cur[idx].data = cur;
        self.prev[idx].data = prev;
        result?;
        r.align32();
        Ok(())
    }

    fn decode_plane_into(
        &mut self,
        r: &mut BitReader,
        dst: &mut [u8],
        geo: Geometry,
        prev: &[u8],
    ) -> Result<(), Error> {
        let Geometry {
            stride,
            width,
            height,
            bw,
            bh,
        } = geo;

        if self.version == b'k' && r.bit() {
            let fill = r.read(8) as u8;
            for row in dst.chunks_mut(stride).take(height) {
                row[..width].fill(fill);
            }
            return Ok(());
        }

        self.init_lengths(width.max(8), bw);
        for n in 0..NB_SRC {
            self.read_bundle(r, n)?;
        }

        // Furthest position a motion source block may start at.
        let ref_end = ((bw - 1 + stride * (bh - 1)) * 8) as isize;
        let coordmap: [usize; 64] = std::array::from_fn(|i| (i & 7) + (i >> 3) * stride);
        let mut ublock = [0u8; 64];
        let mut dct = [0i32; 64];
        let mut residue = [0i16; 64];

        for by in 0..bh {
            self.read_block_types(r, SRC_BLOCK_TYPES)?;
            self.read_block_types(r, SRC_SUB_BLOCK_TYPES)?;
            self.read_colors(r)?;
            self.read_patterns(r)?;
            self.read_values(r, SRC_X_OFF, true)?;
            self.read_values(r, SRC_Y_OFF, true)?;
            self.read_dcs(r, SRC_INTRA_DC, false)?;
            self.read_dcs(r, SRC_INTER_DC, true)?;
            self.read_values(r, SRC_RUN, false)?;

            let mut bx = 0;
            while bx < bw {
                let off = by * 8 * stride + bx * 8;
                let blk = self.bundles[SRC_BLOCK_TYPES].next();
                // A scaled block's other three quarters repeat its type;
                // they were drawn with it.
                if ((by & 1) != 0 || (bx & 1) != 0) && blk == SCALED_BLOCK {
                    bx += 2;
                    continue;
                }
                match blk {
                    SKIP_BLOCK => copy_block(dst, off, prev, off, stride),
                    SCALED_BLOCK => {
                        let sub = self.bundles[SRC_SUB_BLOCK_TYPES].next();
                        match sub {
                            RUN_BLOCK => {
                                if r.left() < 4 {
                                    return Err(err("bink: truncated run block"));
                                }
                                self.run_block(r, &mut ublock, 0, &std::array::from_fn(|i| i))?;
                            }
                            INTRA_BLOCK => {
                                dct.fill(0);
                                dct[0] = self.bundles[SRC_INTRA_DC].next();
                                read_dct_coeffs(r, &mut dct, &INTRA_QUANT)?;
                                idct_put(&mut ublock, 0, 8, &dct);
                            }
                            FILL_BLOCK => {
                                let v = self.bundles[SRC_COLORS].next() as u8;
                                for row in 0..16 {
                                    dst[off + row * stride..][..16].fill(v);
                                }
                            }
                            PATTERN_BLOCK => self.pattern_block(&mut ublock, 0, 8),
                            RAW_BLOCK => {
                                for p in ublock.iter_mut() {
                                    *p = self.bundles[SRC_COLORS].next() as u8;
                                }
                            }
                            _ => return Err(err(format!("bink: bad scaled block type {sub}"))),
                        }
                        if sub != FILL_BLOCK {
                            scale_block(&ublock, dst, off, stride);
                        }
                        bx += 1;
                    }
                    MOTION_BLOCK => self.motion(dst, off, prev, stride, ref_end)?,
                    RUN_BLOCK => self.run_block(r, dst, off, &coordmap)?,
                    RESIDUE_BLOCK => {
                        self.motion(dst, off, prev, stride, ref_end)?;
                        residue.fill(0);
                        let masks = r.read(7) as i32;
                        read_residue(r, &mut residue, masks);
                        for (row, line) in residue.chunks(8).enumerate() {
                            let d = &mut dst[off + row * stride..][..8];
                            for (p, &c) in d.iter_mut().zip(line) {
                                *p = p.wrapping_add(c as u8);
                            }
                        }
                    }
                    INTRA_BLOCK => {
                        dct.fill(0);
                        dct[0] = self.bundles[SRC_INTRA_DC].next();
                        read_dct_coeffs(r, &mut dct, &INTRA_QUANT)?;
                        idct_put(dst, off, stride, &dct);
                    }
                    FILL_BLOCK => {
                        let v = self.bundles[SRC_COLORS].next() as u8;
                        for row in 0..8 {
                            dst[off + row * stride..][..8].fill(v);
                        }
                    }
                    INTER_BLOCK => {
                        self.motion(dst, off, prev, stride, ref_end)?;
                        dct.fill(0);
                        dct[0] = self.bundles[SRC_INTER_DC].next();
                        read_dct_coeffs(r, &mut dct, &INTER_QUANT)?;
                        idct_add(dst, off, stride, &mut dct);
                    }
                    PATTERN_BLOCK => self.pattern_block(dst, off, stride),
                    RAW_BLOCK => {
                        for row in 0..8 {
                            for p in &mut dst[off + row * stride..][..8] {
                                *p = self.bundles[SRC_COLORS].next() as u8;
                            }
                        }
                    }
                    _ => return Err(err(format!("bink: bad block type {blk}"))),
                }
                bx += 1;
            }
        }
        Ok(())
    }

    /// Copies the block at `off` from the previous frame, displaced by the
    /// next motion vector.
    fn motion(
        &mut self,
        dst: &mut [u8],
        off: usize,
        prev: &[u8],
        stride: usize,
        ref_end: isize,
    ) -> Result<(), Error> {
        let xoff = self.bundles[SRC_X_OFF].next() as isize;
        let yoff = self.bundles[SRC_Y_OFF].next() as isize;
        let src = off as isize + xoff + yoff * stride as isize;
        if src < 0 || src > ref_end {
            return Err(err(format!(
                "bink: motion vector out of bounds ({xoff}, {yoff})"
            )));
        }
        copy_block(dst, off, prev, src as usize, stride);
        Ok(())
    }

    /// Runs of colours (single repeated or listed) along one of sixteen
    /// scan orders. `coords` maps a block position to an offset.
    fn run_block(
        &mut self,
        r: &mut BitReader,
        dst: &mut [u8],
        off: usize,
        coords: &[usize; 64],
    ) -> Result<(), Error> {
        let scan = &PATTERNS[r.read(4) as usize];
        let mut s = scan.iter();
        let mut i = 0;
        loop {
            let run = self.bundles[SRC_RUN].next() as usize + 1;
            i += run;
            if i > 64 {
                return Err(err("bink: run out of bounds"));
            }
            if r.bit() {
                let v = self.bundles[SRC_COLORS].next() as u8;
                for &p in s.by_ref().take(run) {
                    dst[off + coords[p as usize]] = v;
                }
            } else {
                for &p in s.by_ref().take(run) {
                    dst[off + coords[p as usize]] = self.bundles[SRC_COLORS].next() as u8;
                }
            }
            if i >= 63 {
                break;
            }
        }
        if i == 63 {
            let p = *s.next().unwrap();
            dst[off + coords[p as usize]] = self.bundles[SRC_COLORS].next() as u8;
        }
        Ok(())
    }

    /// Two colours chosen per pixel by eight row bitmasks.
    fn pattern_block(&mut self, dst: &mut [u8], off: usize, stride: usize) {
        let col = [
            self.bundles[SRC_COLORS].next() as u8,
            self.bundles[SRC_COLORS].next() as u8,
        ];
        for row in 0..8 {
            let mut v = self.bundles[SRC_PATTERN].next();
            for p in &mut dst[off + row * stride..][..8] {
                *p = col[(v & 1) as usize];
                v >>= 1;
            }
        }
    }
}

/// Plane dimensions, copied out so the plane can be borrowed separately.
#[derive(Clone, Copy)]
struct Geometry {
    stride: usize,
    width: usize,
    height: usize,
    bw: usize,
    bh: usize,
}

impl Plane {
    fn geometry(&self) -> Geometry {
        Geometry {
            stride: self.stride,
            width: self.width,
            height: self.height,
            bw: self.bw,
            bh: self.bh,
        }
    }
}

fn copy_block(dst: &mut [u8], off: usize, src: &[u8], src_off: usize, stride: usize) {
    for row in 0..8 {
        dst[off + row * stride..][..8].copy_from_slice(&src[src_off + row * stride..][..8]);
    }
}

/// Draws an 8x8 block at twice the size.
fn scale_block(src: &[u8; 64], dst: &mut [u8], off: usize, stride: usize) {
    for (j, line) in src.chunks(8).enumerate() {
        for half in 0..2 {
            let d = &mut dst[off + (2 * j + half) * stride..][..16];
            for (i, &p) in line.iter().enumerate() {
                d[2 * i] = p;
                d[2 * i + 1] = p;
            }
        }
    }
}

/// Reads one coefficient value of magnitude 2^`bits` to 2^(`bits`+1)-1.
#[inline]
fn read_coef(r: &mut BitReader, bits: i32) -> i32 {
    if bits == 0 {
        1 - ((r.read(1) as i32) << 1)
    } else {
        let t = (r.read(bits as u32) | (1 << bits)) as i32;
        if r.bit() { -t } else { t }
    }
}

/// Reads the AC coefficients of a DCT block (the DC is already in
/// `block[0]`) and dequantises the block in place.
///
/// Coefficients are sent bit plane by bit plane, from the most significant
/// down; a list of pending groups of coefficients (in scan order) is
/// refined each plane: mode 0 is a group of 4x4 coefficients not yet split,
/// 1 the remaining three quarters of it, 2 a group of four, 3 a single one.
fn read_dct_coeffs(
    r: &mut BitReader,
    block: &mut [i32; 64],
    quant: &[[i32; 64]; 16],
) -> Result<(), Error> {
    if r.left() < 4 {
        return Err(err("bink: truncated DCT block"));
    }
    let mut coef_list = [0usize; 128];
    let mut mode_list = [0u8; 128];
    let mut coef_idx = [0usize; 64];
    let mut coef_count = 0;
    let (mut list_start, mut list_end) = (64, 64);
    for (c, m) in [(4, 0), (24, 0), (44, 0), (1, 3), (2, 3), (3, 3)] {
        coef_list[list_end] = c;
        mode_list[list_end] = m;
        list_end += 1;
    }

    let mut bits = r.read(4) as i32 - 1;
    while bits >= 0 {
        let mut pos = list_start;
        while pos < list_end {
            if (mode_list[pos] == 0 && coef_list[pos] == 0) || !r.bit() {
                pos += 1;
                continue;
            }
            let mut ccoef = coef_list[pos];
            let mode = mode_list[pos];
            match mode {
                0 | 2 => {
                    if mode == 0 {
                        coef_list[pos] = ccoef + 4;
                        mode_list[pos] = 1;
                    } else {
                        coef_list[pos] = 0;
                        mode_list[pos] = 0;
                        pos += 1;
                    }
                    for _ in 0..4 {
                        if r.bit() {
                            list_start -= 1;
                            coef_list[list_start] = ccoef;
                            mode_list[list_start] = 3;
                        } else {
                            block[SCAN[ccoef] as usize] = read_coef(r, bits);
                            coef_idx[coef_count] = ccoef;
                            coef_count += 1;
                        }
                        ccoef += 1;
                    }
                }
                1 => {
                    mode_list[pos] = 2;
                    for _ in 0..3 {
                        ccoef += 4;
                        coef_list[list_end] = ccoef;
                        mode_list[list_end] = 2;
                        list_end += 1;
                    }
                }
                _ => {
                    block[SCAN[ccoef] as usize] = read_coef(r, bits);
                    coef_idx[coef_count] = ccoef;
                    coef_count += 1;
                    coef_list[pos] = 0;
                    mode_list[pos] = 0;
                    pos += 1;
                }
            }
        }
        bits -= 1;
    }

    let q = r.read(4) as usize;
    let quant = &quant[q];
    block[0] = block[0].wrapping_mul(quant[0]) >> 11;
    for &idx in &coef_idx[..coef_count] {
        let p = SCAN[idx] as usize;
        block[p] = block[p].wrapping_mul(quant[idx]) >> 11;
    }
    Ok(())
}

/// Reads a residue block: corrections to a motion-copied block, sent bit
/// plane by bit plane like DCT coefficients, at most `masks_count` + 1
/// of them.
fn read_residue(r: &mut BitReader, block: &mut [i16; 64], mut masks_count: i32) {
    let mut coef_list = [0usize; 128];
    let mut mode_list = [0u8; 128];
    let mut nz_coeff = [0usize; 64];
    let mut nz_count = 0;
    let (mut list_start, mut list_end) = (64, 64);
    for (c, m) in [(4, 0), (24, 0), (44, 0), (0, 2)] {
        coef_list[list_end] = c;
        mode_list[list_end] = m;
        list_end += 1;
    }

    let mut mask: i16 = 1 << r.read(3);
    while mask != 0 {
        // Refine the coefficients already found.
        for &p in &nz_coeff[..nz_count] {
            if !r.bit() {
                continue;
            }
            if block[p] < 0 {
                block[p] -= mask;
            } else {
                block[p] += mask;
            }
            masks_count -= 1;
            if masks_count < 0 {
                return;
            }
        }
        let mut pos = list_start;
        while pos < list_end {
            if (coef_list[pos] == 0 && mode_list[pos] == 0) || !r.bit() {
                pos += 1;
                continue;
            }
            let mut ccoef = coef_list[pos];
            let mode = mode_list[pos];
            match mode {
                0 | 2 => {
                    if mode == 0 {
                        coef_list[pos] = ccoef + 4;
                        mode_list[pos] = 1;
                    } else {
                        coef_list[pos] = 0;
                        mode_list[pos] = 0;
                        pos += 1;
                    }
                    for _ in 0..4 {
                        if r.bit() {
                            list_start -= 1;
                            coef_list[list_start] = ccoef;
                            mode_list[list_start] = 3;
                        } else {
                            let p = SCAN[ccoef] as usize;
                            nz_coeff[nz_count] = p;
                            nz_count += 1;
                            block[p] = if r.bit() { -mask } else { mask };
                            masks_count -= 1;
                            if masks_count < 0 {
                                return;
                            }
                        }
                        ccoef += 1;
                    }
                }
                1 => {
                    mode_list[pos] = 2;
                    for _ in 0..3 {
                        ccoef += 4;
                        coef_list[list_end] = ccoef;
                        mode_list[list_end] = 2;
                        list_end += 1;
                    }
                }
                _ => {
                    let p = SCAN[ccoef] as usize;
                    nz_coeff[nz_count] = p;
                    nz_count += 1;
                    block[p] = if r.bit() { -mask } else { mask };
                    coef_list[pos] = 0;
                    mode_list[pos] = 0;
                    pos += 1;
                    masks_count -= 1;
                    if masks_count < 0 {
                        return;
                    }
                }
            }
        }
        mask >>= 1;
    }
}

// Bink's integer IDCT constants (scaled by 2^11 or 2^12).
const A1: i64 = 2896;
const A2: i64 = 2217;
const A3: i64 = 3784;
const A4: i64 = -5352;

/// Multiplies by a constant and drops 11 fraction bits, wrapping like
/// FFmpeg's 32-bit `MUL`.
#[inline(always)]
fn mul(x: i64, y: i64) -> i64 {
    ((x as i32).wrapping_mul(y as i32) >> 11) as i64
}

/// One 8-point pass of Bink's IDCT.
#[inline(always)]
fn idct_1d(s: [i64; 8]) -> [i64; 8] {
    let a0 = s[0] + s[4];
    let a1 = s[0] - s[4];
    let a2 = s[2] + s[6];
    let a3 = mul(A1, s[2] - s[6]);
    let a4 = s[5] + s[3];
    let a5 = s[5] - s[3];
    let a6 = s[1] + s[7];
    let a7 = s[1] - s[7];
    let b0 = a4 + a6;
    let b1 = mul(A3, a5 + a7);
    let b2 = mul(A4, a5) - b0 + b1;
    let b3 = mul(A1, a6 - a4) - b2;
    let b4 = mul(A2, a7) + b3 - b1;
    [
        a0 + a2 + b0,
        a1 + a3 - a2 + b2,
        a1 - a3 + a2 + b3,
        a0 - a2 - b4,
        a0 - a2 + b4,
        a1 - a3 + a2 - b3,
        a1 + a3 - a2 - b2,
        a0 + a2 - b0,
    ]
}

/// The IDCT as FFmpeg's `bink_idct`: columns, then rows with rounding and
/// eight fraction bits dropped. Results are the 32-bit values C would keep.
fn idct(block: &[i32; 64]) -> [i32; 64] {
    let mut temp = [0i32; 64];
    for c in 0..8 {
        let col: [i64; 8] = std::array::from_fn(|i| block[c + 8 * i] as i64);
        let out = if col[1..].iter().all(|&v| v == 0) {
            [col[0]; 8]
        } else {
            idct_1d(col)
        };
        for i in 0..8 {
            temp[c + 8 * i] = out[i] as i32;
        }
    }
    let mut res = [0i32; 64];
    for row in 0..8 {
        let line: [i64; 8] = std::array::from_fn(|i| temp[8 * row + i] as i64);
        let out = idct_1d(line);
        for i in 0..8 {
            res[8 * row + i] = ((out[i] as i32).wrapping_add(0x7F)) >> 8;
        }
    }
    res
}

/// Writes the IDCT of `block` (stored to 8 bits, wrapping as in FFmpeg).
fn idct_put(dst: &mut [u8], off: usize, stride: usize, block: &[i32; 64]) {
    let res = idct(block);
    for (row, line) in res.chunks(8).enumerate() {
        for (p, &v) in dst[off + row * stride..][..8].iter_mut().zip(line) {
            *p = v as u8;
        }
    }
}

/// Adds the IDCT of `block` to the pixels (wrapping as in FFmpeg).
fn idct_add(dst: &mut [u8], off: usize, stride: usize, block: &mut [i32; 64]) {
    let res = idct(block);
    for (row, line) in res.chunks(8).enumerate() {
        for (p, &v) in dst[off + row * stride..][..8].iter_mut().zip(line) {
            *p = p.wrapping_add(v as u8);
        }
    }
}

/// Converts decoded planes to RGBA: BT.601, studio range unless `full`,
/// chroma interpolated bilinearly (samples centred between luma pairs).
pub(crate) fn yuv_to_rgba(
    width: usize,
    height: usize,
    planes: &[(&[u8], usize)],
    full: bool,
    out: &mut Vec<u8>,
) {
    out.clear();
    out.resize(width * height * 4, 255);
    let (y_plane, y_stride) = planes[0];
    let (u_plane, c_stride) = planes[1];
    let (v_plane, _) = planes[2];
    let alpha = planes.get(3).copied();
    let cw = (width / 2).max(1);
    let ch = (height / 2).max(1);

    // Coefficients in 16.16 fixed point.
    let (ys, yo, rv, gu, gv, bu) = if full {
        (65536i32, 0, 91881, 22554, 46802, 116130)
    } else {
        (76309, 16, 104597, 25675, 53279, 132201)
    };
    // Chroma sample index pairs and weights (out of 4) for each output x.
    let taps = |n: usize, len: usize| -> Vec<(usize, usize, i32)> {
        (0..n)
            .map(|x| {
                let c = (x / 2).min(len - 1);
                let other = if x % 2 == 0 {
                    c.saturating_sub(1)
                } else {
                    (c + 1).min(len - 1)
                };
                (c, other, 3)
            })
            .collect()
    };
    let xt = taps(width, cw);
    let yt = taps(height, ch);
    let mut u_row = vec![0i32; cw];
    let mut v_row = vec![0i32; cw];

    for (y, out_row) in out.chunks_mut(width * 4).enumerate() {
        // Vertically interpolated chroma rows, scaled by 4.
        let (c0, c1, w) = yt[y];
        for x in 0..cw {
            let (a, b) = (c0 * c_stride + x, c1 * c_stride + x);
            u_row[x] = u_plane[a] as i32 * w + u_plane[b] as i32 * (4 - w);
            v_row[x] = v_plane[a] as i32 * w + v_plane[b] as i32 * (4 - w);
        }
        let yl = &y_plane[y * y_stride..][..width];
        for (x, px) in out_row.chunks_exact_mut(4).enumerate() {
            let (a, b, w) = xt[x];
            // Scaled by 16.
            let u = u_row[a] * w + u_row[b] * (4 - w) - 128 * 16;
            let v = v_row[a] * w + v_row[b] * (4 - w) - 128 * 16;
            let l = (yl[x] as i32 - yo) * ys + (1 << 15);
            px[0] = clamp8((l + ((rv * v) >> 4)) >> 16);
            px[1] = clamp8((l - ((gu * u) >> 4) - ((gv * v) >> 4)) >> 16);
            px[2] = clamp8((l + ((bu * u) >> 4)) >> 16);
            if let Some((a_plane, a_stride)) = alpha {
                px[3] = a_plane[y * a_stride + x];
            }
        }
    }
}

#[inline(always)]
fn clamp8(v: i32) -> u8 {
    v.clamp(0, 255) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every tree is a complete prefix code: each table slot is filled by
    /// exactly one leaf.
    #[test]
    fn trees_are_complete_prefix_codes() {
        for t in 0..16 {
            let bits = TREE_LENS[t][15] as u32;
            let mut hits = vec![0; 1 << bits];
            for leaf in 0..16 {
                let code = TREE_CODES[t][leaf] as usize;
                let len = TREE_LENS[t][leaf] as u32;
                assert!(code < 1 << len, "tree {t} leaf {leaf}");
                for ext in 0..1usize << (bits - len) {
                    hits[code | (ext << len)] += 1;
                }
            }
            assert!(hits.iter().all(|&h| h == 1), "tree {t}");
        }
    }

    /// Writes bits lowest first, for building test streams.
    struct BitWriter {
        bytes: Vec<u8>,
        n: usize,
    }

    impl BitWriter {
        fn put(&mut self, v: u32, len: u32) {
            for i in 0..len {
                if self.n.is_multiple_of(8) {
                    self.bytes.push(0);
                }
                self.bytes[self.n / 8] |= (((v >> i) & 1) as u8) << (self.n % 8);
                self.n += 1;
            }
        }
    }

    #[test]
    fn huffman_decodes_every_leaf() {
        let vlcs: Vec<Vlc> = (0..16).map(Vlc::new).collect();
        for t in 0..16 {
            let mut w = BitWriter {
                bytes: vec![],
                n: 0,
            };
            for leaf in (0..16).rev() {
                w.put(TREE_CODES[t][leaf] as u32, TREE_LENS[t][leaf] as u32);
            }
            let tree = Tree {
                vlc: t,
                syms: std::array::from_fn(|i| i as u8),
            };
            let mut r = BitReader::new(&w.bytes);
            for leaf in (0..16).rev() {
                assert_eq!(get_huff(&mut r, &vlcs, &tree), leaf, "tree {t}");
            }
            assert_eq!(r.position(), w.n);
        }
    }

    #[test]
    fn tree_symbol_maps() {
        // Explicit list: tree 3, three symbols 7, 2, 9, then the rest.
        let mut w = BitWriter {
            bytes: vec![],
            n: 0,
        };
        w.put(3, 4);
        w.put(1, 1);
        w.put(2, 3);
        for s in [7, 2, 9] {
            w.put(s, 4);
        }
        let t = read_tree(&mut BitReader::new(&w.bytes)).unwrap();
        assert_eq!(t.vlc, 3);
        assert_eq!(
            t.syms,
            [7, 2, 9, 0, 1, 3, 4, 5, 6, 8, 10, 11, 12, 13, 14, 15]
        );

        // Shuffle with one merge round taking always the second element
        // first: pairs swap.
        let mut w = BitWriter {
            bytes: vec![],
            n: 0,
        };
        w.put(5, 4);
        w.put(0, 1);
        w.put(0, 2);
        for _ in 0..8 {
            w.put(1, 1);
        }
        let t = read_tree(&mut BitReader::new(&w.bytes)).unwrap();
        assert_eq!(
            t.syms,
            [1, 0, 3, 2, 5, 4, 7, 6, 9, 8, 11, 10, 13, 12, 15, 14]
        );
    }

    /// Straightforward floating-point 8x8 inverse DCT with Bink's scaling.
    /// Bink's IDCT is the "scaled" kind whose dequantisation tables carry
    /// the per-frequency factors: a flat block of value v has DC 256 v, and
    /// frequency k > 0 is divided by cos(k pi / 16) on each axis.
    fn float_idct(block: &[i32; 64]) -> [f64; 64] {
        use std::f64::consts::PI;
        let mut out = [0.0; 64];
        for y in 0..8 {
            for x in 0..8 {
                let mut s = 0.0;
                for v in 0..8 {
                    for u in 0..8 {
                        let cu = if u == 0 {
                            1.0
                        } else {
                            1.0 / (u as f64 * PI / 16.0).cos()
                        };
                        let cv = if v == 0 {
                            1.0
                        } else {
                            1.0 / (v as f64 * PI / 16.0).cos()
                        };
                        s += cu
                            * cv
                            * block[v * 8 + u] as f64
                            * ((2 * x + 1) as f64 * u as f64 * PI / 16.0).cos()
                            * ((2 * y + 1) as f64 * v as f64 * PI / 16.0).cos();
                    }
                }
                out[y * 8 + x] = s / 256.0;
            }
        }
        out
    }

    #[test]
    fn idct_matches_float_reference() {
        // DC only: a flat block.
        let mut b = [0i32; 64];
        b[0] = 256 * 100;
        assert!(idct(&b).iter().all(|&v| v == 100));

        // Random sparse blocks: close to a real DCT (Bink's integer
        // approximation is accurate to a pixel level or so).
        let mut seed = 12345u32;
        let mut rand = || {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
            (seed >> 16) as i32
        };
        for _ in 0..200 {
            let mut b = [0i32; 64];
            b[0] = rand() % 30000;
            for _ in 0..6 {
                let i = (rand() % 64) as usize;
                b[i] = rand() % 4000 - 2000;
            }
            let got = idct(&b);
            let want = float_idct(&b);
            for i in 0..64 {
                assert!(
                    (got[i] as f64 - want[i]).abs() < 2.5,
                    "{} vs {} at {i}",
                    got[i],
                    want[i]
                );
            }
        }
    }

    #[test]
    fn scaled_block_doubles_pixels() {
        let src: [u8; 64] = std::array::from_fn(|i| i as u8);
        let mut dst = vec![0u8; 16 * 16];
        scale_block(&src, &mut dst, 0, 16);
        for y in 0..16 {
            for x in 0..16 {
                assert_eq!(dst[y * 16 + x], src[(y / 2) * 8 + x / 2]);
            }
        }
    }
}
