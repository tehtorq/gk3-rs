//! Bink audio, RDFT variant: each block is a frame of quantised frequency
//! coefficients turned back into samples with an inverse real FFT. Stereo
//! is coded as one interleaved signal at twice the rate. Consecutive blocks
//! overlap by a sixteenth of a block and are crossfaded there.

use crate::bits::BitReader;
use crate::{Error, err};

/// Audio track flags in the header.
pub(crate) const AUD_STEREO: u16 = 0x2000;
pub(crate) const AUD_USEDCT: u16 = 0x1000;

/// Upper edges (Hz) of the critical bands, from FFmpeg's `wma_freqs.c`;
/// each band shares one quantiser.
const CRITICAL_FREQS: [u32; 25] = [
    100, 200, 300, 400, 510, 630, 770, 920, 1080, 1270, 1480, 1720, 2000, 2320, 2700, 3150, 3700,
    4400, 5300, 6400, 7700, 9500, 12000, 15500, 24500,
];

/// Lengths, in groups of 8 coefficients, of the run codes.
const RLE_LENGTHS: [usize; 16] = [2, 3, 4, 5, 6, 8, 9, 10, 11, 12, 13, 14, 15, 16, 32, 64];

pub(crate) struct AudioDecoder {
    /// Coefficients (and samples) per block.
    frame_len: usize,
    /// Samples shared by the end of one block and the start of the next.
    overlap_len: usize,
    num_bands: usize,
    /// First coefficient of each band.
    bands: [usize; 26],
    root: f32,
    quant_table: [f32; 96],
    /// Tail of the previous block, crossfaded into the next one.
    previous: Vec<f32>,
    first: bool,
    rdft: Rdft,
    coeffs: Vec<f32>,
}

impl AudioDecoder {
    pub(crate) fn new(sample_rate: u32, channels: u16, flags: u16) -> Result<AudioDecoder, Error> {
        if flags & AUD_USEDCT != 0 {
            return Err(err("bink: DCT audio is not supported"));
        }
        if !(1..=2).contains(&channels) || sample_rate == 0 {
            return Err(err("bink: bad audio format"));
        }
        let mut frame_len_bits = if sample_rate < 22050 {
            9
        } else if sample_rate < 44100 {
            10
        } else {
            11
        };
        // The RDFT variant codes channels interleaved as one signal.
        let rate = sample_rate * channels as u32;
        frame_len_bits += (channels as u32).ilog2();
        let frame_len = 1usize << frame_len_bits;
        let overlap_len = frame_len / 16;
        let rate_half = (rate as u64).div_ceil(2);
        let root = (2.0 / ((frame_len as f64).sqrt() * 32768.0)) as f32;
        let quant_table = std::array::from_fn(|i| (i as f32 * 0.152_891_64_f32).exp() * root);

        let mut num_bands = 1;
        while num_bands < 25 {
            if rate_half <= CRITICAL_FREQS[num_bands - 1] as u64 {
                break;
            }
            num_bands += 1;
        }
        let mut bands = [0usize; 26];
        bands[0] = 2;
        for i in 1..num_bands {
            bands[i] =
                ((CRITICAL_FREQS[i - 1] as u64 * frame_len as u64 / rate_half) & !1) as usize;
        }
        bands[num_bands] = frame_len;

        Ok(AudioDecoder {
            frame_len,
            overlap_len,
            num_bands,
            bands,
            root,
            quant_table,
            previous: vec![0.0; overlap_len],
            first: true,
            rdft: Rdft::new(frame_len),
            coeffs: vec![0.0; frame_len],
        })
    }

    /// Decodes one audio packet (as stored in a frame, starting with its
    /// decoded size) and appends interleaved samples in -1..1 to `out`.
    pub(crate) fn decode_packet(&mut self, packet: &[u8], out: &mut Vec<f32>) -> Result<(), Error> {
        if packet.len() < 4 {
            return Err(err("bink: audio packet too small"));
        }
        let mut r = BitReader::new(packet);
        r.skip(32);
        while r.left() > 0 {
            self.decode_block(&mut r)?;
            out.extend_from_slice(&self.coeffs[..self.frame_len - self.overlap_len]);
            r.align32();
        }
        Ok(())
    }

    /// Reads float values: a 5-bit exponent, 23-bit mantissa and a sign.
    fn get_float(r: &mut BitReader) -> f32 {
        let power = r.read(5) as i32;
        let f = r.read(23) as f32 * 2f32.powi(power - 23);
        if r.bit() { -f } else { f }
    }

    /// Decodes one block into `self.coeffs` as samples.
    fn decode_block(&mut self, r: &mut BitReader) -> Result<(), Error> {
        let n = self.frame_len;
        let coeffs = &mut self.coeffs;
        if r.left() < 58 {
            return Err(err("bink: truncated audio block"));
        }
        coeffs[0] = Self::get_float(r) * self.root;
        coeffs[1] = Self::get_float(r) * self.root;

        if r.left() < self.num_bands as isize * 8 {
            return Err(err("bink: truncated audio block"));
        }
        let mut quant = [0f32; 25];
        for q in &mut quant[..self.num_bands] {
            *q = self.quant_table[(r.read(8) as usize).min(95)];
        }

        let mut k = 0;
        let mut q = quant[0];
        let mut i = 2;
        while i < n {
            // A run of coefficients sharing one bit width.
            let j = if r.bit() {
                i + RLE_LENGTHS[r.read(4) as usize] * 8
            } else {
                i + 8
            };
            let j = j.min(n);
            let width = r.read(4);
            if width == 0 {
                coeffs[i..j].fill(0.0);
                i = j;
                while self.bands[k] < i {
                    q = quant[k];
                    k += 1;
                }
            } else {
                while i < j {
                    if self.bands[k] == i {
                        q = quant[k];
                        k += 1;
                    }
                    let coeff = r.read(width);
                    coeffs[i] = if coeff == 0 {
                        0.0
                    } else if r.bit() {
                        -q * coeff as f32
                    } else {
                        q * coeff as f32
                    };
                    i += 1;
                }
            }
        }

        self.rdft.inverse(coeffs);

        let count = self.overlap_len as f32;
        if !self.first {
            for (i, (s, &p)) in coeffs.iter_mut().zip(&self.previous).enumerate() {
                *s = (p * (count - i as f32) + *s * i as f32) / count;
            }
        }
        self.previous
            .copy_from_slice(&coeffs[n - self.overlap_len..]);
        self.first = false;
        Ok(())
    }
}

/// FFmpeg's real DFT in its `DFT_C2R` setup: takes a packed spectrum
/// (`data[0]` the DC term, `data[1]` the Nyquist term, then real and
/// imaginary parts of bins 1 to n/2-1) and produces n real samples,
/// x[j] = (X0 + Xn/2 (-1)^j) / 2 + sum Re(X_k e^{-2 pi i jk/n}).
/// (Bink's imaginary parts have the opposite sign to the usual inverse
/// transform; FFmpeg 6 negates them before its standard inverse RDFT.)
pub(crate) struct Rdft {
    n: usize,
    /// cos and sin of 2 pi i / n for i < n/4.
    tcos: Vec<f32>,
    tsin: Vec<f32>,
    fft: Fft,
}

impl Rdft {
    pub(crate) fn new(n: usize) -> Rdft {
        let step = 2.0 * std::f64::consts::PI / n as f64;
        Rdft {
            n,
            tcos: (0..n / 4).map(|i| (i as f64 * step).cos() as f32).collect(),
            tsin: (0..n / 4).map(|i| (i as f64 * step).sin() as f32).collect(),
            fft: Fft::new(n / 2),
        }
    }

    /// Transforms `data` (n values) in place.
    pub(crate) fn inverse(&self, data: &mut [f32]) {
        let n = self.n;
        let k1 = 0.5f32;
        let k2 = -0.5f32;
        let ev = data[0];
        data[0] = ev + data[1];
        data[1] = ev - data[1];
        for i in 1..n / 4 {
            let i1 = 2 * i;
            let i2 = n - i1;
            // Separate the even and odd half-size FFTs.
            let ev_re = k1 * (data[i1] + data[i2]);
            let od_im = k2 * (data[i2] - data[i1]);
            let ev_im = k1 * (data[i1 + 1] - data[i2 + 1]);
            let od_re = k2 * (data[i1 + 1] + data[i2 + 1]);
            // Twiddle the odd one and recombine.
            let odsum_re = od_re * self.tcos[i] + od_im * self.tsin[i];
            let odsum_im = od_im * self.tcos[i] - od_re * self.tsin[i];
            data[i1] = ev_re + odsum_re;
            data[i1 + 1] = ev_im + odsum_im;
            data[i2] = ev_re - odsum_re;
            data[i2 + 1] = odsum_im - ev_im;
        }
        data[0] *= k1;
        data[1] *= k1;
        self.fft.forward(data);
    }
}

/// Radix-2 complex FFT, forward direction (e^{-2 pi i jk/n}), on
/// interleaved real and imaginary parts.
struct Fft {
    n: usize,
    rev: Vec<u32>,
    /// e^{-2 pi i k/n} for k < n/2.
    twiddle: Vec<(f32, f32)>,
}

impl Fft {
    fn new(n: usize) -> Fft {
        let bits = n.trailing_zeros();
        let rev = (0..n as u32)
            .map(|i| {
                if bits == 0 {
                    0
                } else {
                    i.reverse_bits() >> (32 - bits)
                }
            })
            .collect();
        let twiddle = (0..n / 2)
            .map(|k| {
                let a = -2.0 * std::f64::consts::PI * k as f64 / n as f64;
                (a.cos() as f32, a.sin() as f32)
            })
            .collect();
        Fft { n, rev, twiddle }
    }

    fn forward(&self, data: &mut [f32]) {
        let n = self.n;
        for i in 0..n {
            let j = self.rev[i] as usize;
            if j > i {
                data.swap(2 * i, 2 * j);
                data.swap(2 * i + 1, 2 * j + 1);
            }
        }
        let mut len = 2;
        while len <= n {
            let half = len / 2;
            let step = n / len;
            for start in (0..n).step_by(len) {
                for k in 0..half {
                    let (wr, wi) = self.twiddle[k * step];
                    let a = 2 * (start + k);
                    let b = 2 * (start + k + half);
                    let (br, bi) = (data[b], data[b + 1]);
                    let tr = br * wr - bi * wi;
                    let ti = br * wi + bi * wr;
                    let (ar, ai) = (data[a], data[a + 1]);
                    data[a] = ar + tr;
                    data[a + 1] = ai + ti;
                    data[b] = ar - tr;
                    data[b + 1] = ai - ti;
                }
            }
            len *= 2;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn random(len: usize, mut seed: u32) -> Vec<f32> {
        (0..len)
            .map(|_| {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (seed >> 8) as f32 / (1 << 24) as f32 * 2.0 - 1.0
            })
            .collect()
    }

    #[test]
    fn fft_matches_naive_dft() {
        for n in [1usize, 2, 8, 64] {
            let input = random(2 * n, n as u32);
            let mut data = input.clone();
            Fft::new(n).forward(&mut data);
            for k in 0..n {
                let (mut re, mut im) = (0f64, 0f64);
                for j in 0..n {
                    let a = -2.0 * std::f64::consts::PI * (j * k) as f64 / n as f64;
                    let (xr, xi) = (input[2 * j] as f64, input[2 * j + 1] as f64);
                    re += xr * a.cos() - xi * a.sin();
                    im += xr * a.sin() + xi * a.cos();
                }
                assert!((data[2 * k] as f64 - re).abs() < 1e-3, "n {n} k {k}");
                assert!((data[2 * k + 1] as f64 - im).abs() < 1e-3, "n {n} k {k}");
            }
        }
    }

    #[test]
    fn rdft_inverse_matches_naive_dft() {
        for n in [16usize, 64, 256, 2048] {
            let spectrum = random(n, 7 + n as u32);
            let mut data = spectrum.clone();
            Rdft::new(n).inverse(&mut data);
            let scale = spectrum.iter().map(|v| v.abs() as f64).sum::<f64>();
            for (j, &got) in data.iter().enumerate() {
                let mut x = 0.5
                    * (spectrum[0] as f64
                        + spectrum[1] as f64 * if j % 2 == 0 { 1.0 } else { -1.0 });
                for k in 1..n / 2 {
                    let a = 2.0 * std::f64::consts::PI * (j * k % n) as f64 / n as f64;
                    x += spectrum[2 * k] as f64 * a.cos() + spectrum[2 * k + 1] as f64 * a.sin();
                }
                assert!(
                    (got as f64 - x).abs() < 1e-5 * scale,
                    "n {n} j {j}: {got} vs {x}"
                );
            }
        }
    }

    #[test]
    fn bands_for_gk3_audio() {
        let d = AudioDecoder::new(22050, 2, 0xE000).unwrap();
        assert_eq!(d.frame_len, 2048);
        assert_eq!(d.overlap_len, 128);
        assert_eq!(d.num_bands, 25);
        assert_eq!(d.bands[1], (100 * 2048 / 22050) & !1);
        assert_eq!(d.bands[25], 2048);
    }
}
