//! Decodes a Bink movie and writes chosen frames as PPM images and the
//! first audio track as `audio.wav`, for checking the decoder by eye and
//! ear.
//!
//! `cargo run -p gk3-bink --example bink_frames -- FILE OUTDIR FRAME...`

use std::fs;
use std::path::Path;
use std::time::Instant;

use gk3_bink::Bink;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: bink_frames FILE OUTDIR FRAME...");
        std::process::exit(2);
    }
    let out = Path::new(&args[2]);
    fs::create_dir_all(out).expect("create output directory");
    let wanted: Vec<u32> = args[3..]
        .iter()
        .map(|a| a.parse().expect("frame number"))
        .collect();

    let data = fs::read(&args[1]).expect("read movie");
    let mut bink = match Bink::open(data) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("{}: {e}", args[1]);
            std::process::exit(1);
        }
    };
    let (w, h) = (bink.width(), bink.height());
    let (num, den) = bink.fps();
    println!(
        "{w}x{h}, {} frames at {num}/{den} fps, audio {:?}",
        bink.frame_count(),
        bink.audio_format()
    );

    let last = wanted
        .iter()
        .copied()
        .max()
        .unwrap_or(0)
        .min(bink.frame_count() - 1);
    let start = Instant::now();
    let mut errors = 0;
    for i in 0..=last {
        let frame = match bink.next_frame() {
            Some(Ok(f)) => f,
            Some(Err(e)) => {
                eprintln!("frame {i}: {e}");
                errors += 1;
                continue;
            }
            None => break,
        };
        if wanted.contains(&i) {
            let mut ppm = format!("P6\n{w} {h}\n255\n").into_bytes();
            for px in frame.chunks_exact(4) {
                ppm.extend_from_slice(&px[..3]);
            }
            fs::write(out.join(format!("frame{i:05}.ppm")), ppm).expect("write frame");
        }
    }
    let secs = start.elapsed().as_secs_f64();
    println!(
        "decoded {} frames in {secs:.3} s ({:.0} fps), {errors} errors",
        last + 1,
        (last + 1) as f64 / secs
    );

    let start = Instant::now();
    match bink.decode_audio() {
        Ok(samples) => {
            println!(
                "decoded {} audio samples in {:.3} s",
                samples.len(),
                start.elapsed().as_secs_f64()
            );
            if let Some((rate, channels)) = bink.audio_format() {
                write_wav(&out.join("audio.wav"), rate, channels, &samples);
            }
        }
        Err(e) => eprintln!("audio: {e}"),
    }
}

/// Writes 16-bit PCM WAV.
fn write_wav(path: &Path, rate: u32, channels: u16, samples: &[i16]) {
    let bytes = (samples.len() * 2) as u32;
    let mut wav = Vec::with_capacity(44 + bytes as usize);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + bytes).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&channels.to_le_bytes());
    wav.extend_from_slice(&rate.to_le_bytes());
    wav.extend_from_slice(&(rate * channels as u32 * 2).to_le_bytes());
    wav.extend_from_slice(&(channels * 2).to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&bytes.to_le_bytes());
    for s in samples {
        wav.extend_from_slice(&s.to_le_bytes());
    }
    fs::write(path, wav).expect("write wav");
}
