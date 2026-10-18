//! Sound output.
//!
//! Sounds are mixed here, on a thread of their own, into 44.1 kHz stereo and
//! written to the system through PulseAudio's simple API, which PipeWire also
//! provides. The library is loaded when the game starts rather than linked,
//! so a machine without it gets a silent game instead of one that will not
//! start.

use std::ffi::{CString, c_char, c_int, c_void};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use gk3_media::Pcm;

pub const RATE: u32 = 44_100;
/// Frames mixed per write: about 23 ms.
const CHUNK: usize = 1024;

/// Which volume control a sound answers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bus {
    Dialogue,
    Effects,
    Music,
    /// A full-screen movie's sound.
    Movie,
    /// A room's ambient soundtracks: the original's `Ambient` sounds, which
    /// its effects volume sets too.
    Ambient,
}

/// How many buses there are.
const BUSES: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SoundId(u64);

struct Voice {
    id: u64,
    pcm: Arc<Pcm>,
    /// Position in source frames, fractional for resampling.
    position: f64,
    step: f64,
    gain: f32,
    /// Each output channel's share (left, right): 1 for both but for a
    /// sound placed in the room.
    pan: [f32; 2],
    looping: bool,
    bus: Bus,
    /// Held still and silent, as under a menu.
    held: bool,
}

struct Mixer {
    voices: Vec<Voice>,
    /// Each bus's level in the mix.
    gains: [f32; BUSES],
    /// The player's volume for each bus, and for everything.
    volumes: [f32; BUSES],
    master: f32,
    /// While set, only this bus is heard; the rest play on silently.
    solo: Option<Bus>,
}

impl Default for Mixer {
    fn default() -> Self {
        Self {
            voices: Vec::new(),
            gains: [1.0; BUSES],
            volumes: [1.0; BUSES],
            master: 1.0,
            solo: None,
        }
    }
}

impl Mixer {
    /// Mixes the next chunk of interleaved stereo.
    fn mix(&mut self, out: &mut [i16]) {
        let mut acc = vec![0f32; out.len()];
        let (gains, volumes, master, solo) = (self.gains, self.volumes, self.master, self.solo);
        self.voices.retain_mut(|v| {
            let frames = v.pcm.frames();
            let channels = v.pcm.channels.max(1) as usize;
            if frames == 0 {
                return false;
            }
            if v.held {
                return true;
            }
            let bus = match solo {
                Some(only) if only != v.bus => 0.0,
                _ => gains[v.bus as usize] * volumes[v.bus as usize] * master * v.gain,
            };
            for frame in acc.chunks_exact_mut(2) {
                if v.position >= frames as f64 {
                    if v.looping {
                        v.position -= frames as f64;
                    } else {
                        return false;
                    }
                }
                let i = v.position as usize;
                let t = (v.position - i as f64) as f32;
                let j = (i + 1).min(frames - 1);
                let sample = |frame: usize, ch: usize| {
                    v.pcm.samples[frame * channels + ch.min(channels - 1)] as f32
                };
                for (ch, slot) in frame.iter_mut().enumerate() {
                    let a = sample(i, ch);
                    let b = sample(j, ch);
                    *slot += (a + (b - a) * t) * bus * v.pan[ch];
                }
                v.position += v.step;
            }
            true
        });
        for (o, a) in out.iter_mut().zip(acc) {
            *o = a.clamp(i16::MIN as f32, i16::MAX as f32) as i16;
        }
    }
}

/// The sound system. Cheap to clone; every clone plays into the same mix.
#[derive(Clone)]
pub struct Audio {
    mixer: Option<Arc<Mutex<Mixer>>>,
    next: Arc<AtomicU64>,
    running: Arc<AtomicBool>,
    /// Why there is no sound, when there is none.
    pub status: Arc<String>,
}

impl Audio {
    /// No output at all, for tools and tests.
    pub fn silent(reason: &str) -> Self {
        Self {
            mixer: None,
            next: Arc::new(AtomicU64::new(1)),
            running: Arc::new(AtomicBool::new(false)),
            status: Arc::new(reason.to_string()),
        }
    }

    /// Opens the system's sound output, or returns a silent system saying why.
    pub fn open(application: &str) -> Self {
        match Pulse::open(application) {
            Ok(pulse) => {
                let mixer = Arc::new(Mutex::new(Mixer {
                    gains: [1.0, 0.8, 0.6, 1.0, 0.8],
                    ..Mixer::default()
                }));
                let running = Arc::new(AtomicBool::new(true));
                let (m, r) = (mixer.clone(), running.clone());
                let spawned = thread::Builder::new().name("audio".into()).spawn(move || {
                    let mut buffer = vec![0i16; CHUNK * 2];
                    while r.load(Ordering::Relaxed) {
                        m.lock().unwrap().mix(&mut buffer);
                        if pulse.write(&buffer).is_err() {
                            break;
                        }
                    }
                });
                match spawned {
                    Ok(_) => Self {
                        mixer: Some(mixer),
                        next: Arc::new(AtomicU64::new(1)),
                        running,
                        status: Arc::new("PulseAudio".into()),
                    },
                    Err(e) => Self::silent(&format!("no audio thread: {e}")),
                }
            }
            Err(reason) => Self::silent(&reason),
        }
    }

    pub fn is_silent(&self) -> bool {
        self.mixer.is_none()
    }

    pub fn play(&self, pcm: Arc<Pcm>, gain: f32, looping: bool, bus: Bus) -> SoundId {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        if let Some(mixer) = &self.mixer {
            let step = pcm.sample_rate as f64 / RATE as f64;
            mixer.lock().unwrap().voices.push(Voice {
                id,
                pcm,
                position: 0.0,
                step,
                gain,
                pan: [1.0; 2],
                looping,
                bus,
                held: false,
            });
        }
        SoundId(id)
    }

    /// A sound placed in the room, at a level and with each channel's
    /// share (left, right) as it is heard from where the camera is.
    pub fn play_placed(&self, pcm: Arc<Pcm>, gain: f32, pan: [f32; 2], looping: bool, bus: Bus) -> SoundId {
        let sound = self.play(pcm, gain, looping, bus);
        self.place(sound, gain, pan);
        sound
    }

    /// How a playing sound is heard now: its level, and each channel's
    /// share (left, right).
    pub fn place(&self, sound: SoundId, gain: f32, pan: [f32; 2]) {
        if let Some(mixer) = &self.mixer
            && let Some(v) = mixer.lock().unwrap().voices.iter_mut().find(|v| v.id == sound.0)
        {
            v.gain = gain;
            v.pan = pan;
        }
    }

    pub fn stop(&self, sound: SoundId) {
        if let Some(mixer) = &self.mixer {
            mixer.lock().unwrap().voices.retain(|v| v.id != sound.0);
        }
    }

    /// Stops everything playing on one bus.
    pub fn stop_bus(&self, bus: Bus) {
        if let Some(mixer) = &self.mixer {
            mixer.lock().unwrap().voices.retain(|v| v.bus != bus);
        }
    }

    pub fn stop_all(&self) {
        if let Some(mixer) = &self.mixer {
            mixer.lock().unwrap().voices.clear();
        }
    }

    pub fn is_playing(&self, sound: SoundId) -> bool {
        self.mixer
            .as_ref()
            .is_some_and(|m| m.lock().unwrap().voices.iter().any(|v| v.id == sound.0))
    }

    /// Hears only one bus (a movie's), or every bus again with `None`, as
    /// G-Engine's movie layer overrides the game's sound.
    pub fn solo(&self, bus: Option<Bus>) {
        if let Some(mixer) = &self.mixer {
            mixer.lock().unwrap().solo = bus;
        }
    }

    pub fn set_gain(&self, bus: Bus, gain: f32) {
        if let Some(mixer) = &self.mixer {
            mixer.lock().unwrap().gains[bus as usize] = gain;
        }
    }

    /// The player's volume for one bus, from 0 to 1.
    pub fn set_volume(&self, bus: Bus, volume: f32) {
        if let Some(mixer) = &self.mixer {
            mixer.lock().unwrap().volumes[bus as usize] = volume.clamp(0.0, 1.0);
        }
    }

    /// The player's volume for everything, from 0 to 1.
    pub fn set_master(&self, volume: f32) {
        if let Some(mixer) = &self.mixer {
            mixer.lock().unwrap().master = volume.clamp(0.0, 1.0);
        }
    }

    /// Holds everything playing still and silent (`true`), as G-Engine's
    /// menus and popups do, or lets it go on. Sounds started while held,
    /// such as a button's click, play as usual.
    pub fn hold(&self, held: bool) {
        if let Some(mixer) = &self.mixer {
            for v in &mut mixer.lock().unwrap().voices {
                v.held = held;
            }
        }
    }
}

impl Drop for Audio {
    fn drop(&mut self) {
        // The last handle stops the thread.
        if Arc::strong_count(&self.running) == 1 {
            self.running.store(false, Ordering::Relaxed);
        }
    }
}

#[repr(C)]
struct SampleSpec {
    format: c_int,
    rate: u32,
    channels: u8,
}

#[repr(C)]
struct BufferAttr {
    maxlength: u32,
    tlength: u32,
    prebuf: u32,
    minreq: u32,
    fragsize: u32,
}

type NewFn = unsafe extern "C" fn(
    *const c_char,
    *const c_char,
    c_int,
    *const c_char,
    *const c_char,
    *const SampleSpec,
    *const c_void,
    *const BufferAttr,
    *mut c_int,
) -> *mut c_void;
type WriteFn = unsafe extern "C" fn(*mut c_void, *const c_void, usize, *mut c_int) -> c_int;
type FreeFn = unsafe extern "C" fn(*mut c_void);

/// A playback stream through `libpulse-simple`, loaded at run time.
struct Pulse {
    _library: libloading::Library,
    stream: *mut c_void,
    write: WriteFn,
    free: FreeFn,
}

// The stream is only ever used from the audio thread it is moved to.
unsafe impl Send for Pulse {}

const PA_SAMPLE_S16LE: c_int = 3;
const PA_STREAM_PLAYBACK: c_int = 1;

impl Pulse {
    fn open(application: &str) -> Result<Self, String> {
        // SAFETY: loading a system library runs its initialisers; this one
        // is PulseAudio's own client library.
        let library = unsafe { libloading::Library::new("libpulse-simple.so.0") }
            .map_err(|e| format!("no PulseAudio or PipeWire sound library ({e})"))?;
        // SAFETY: the signatures match pulse/simple.h.
        let (new, write, free) = unsafe {
            let new: NewFn = *library
                .get::<NewFn>(b"pa_simple_new\0")
                .map_err(|e| e.to_string())?;
            let write: WriteFn = *library
                .get::<WriteFn>(b"pa_simple_write\0")
                .map_err(|e| e.to_string())?;
            let free: FreeFn = *library
                .get::<FreeFn>(b"pa_simple_free\0")
                .map_err(|e| e.to_string())?;
            (new, write, free)
        };
        let name = CString::new(application).unwrap_or_default();
        let stream_name = CString::new("game").unwrap_or_default();
        let spec = SampleSpec {
            format: PA_SAMPLE_S16LE,
            rate: RATE,
            channels: 2,
        };
        // About a tenth of a second queued: short enough that a line starts
        // with its caption.
        let attr = BufferAttr {
            maxlength: u32::MAX,
            tlength: (RATE / 10) * 4,
            prebuf: u32::MAX,
            minreq: u32::MAX,
            fragsize: u32::MAX,
        };
        let mut error = 0;
        // SAFETY: every pointer is valid for the call; null server and device
        // mean the defaults.
        let stream = unsafe {
            new(
                std::ptr::null(),
                name.as_ptr(),
                PA_STREAM_PLAYBACK,
                std::ptr::null(),
                stream_name.as_ptr(),
                &spec,
                std::ptr::null(),
                &attr,
                &mut error,
            )
        };
        if stream.is_null() {
            return Err(format!("the sound server refused a stream (error {error})"));
        }
        Ok(Self {
            _library: library,
            stream,
            write,
            free,
        })
    }

    fn write(&self, samples: &[i16]) -> Result<(), ()> {
        let mut error = 0;
        // SAFETY: the buffer is valid for its length in bytes; the call
        // blocks until the server has room, which paces the mixer.
        let r = unsafe {
            (self.write)(
                self.stream,
                samples.as_ptr().cast(),
                std::mem::size_of_val(samples),
                &mut error,
            )
        };
        if r < 0 { Err(()) } else { Ok(()) }
    }
}

impl Drop for Pulse {
    fn drop(&mut self) {
        // SAFETY: the stream came from pa_simple_new and is freed once.
        unsafe { (self.free)(self.stream) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(rate: u32, channels: u16, frames: usize) -> Arc<Pcm> {
        Arc::new(Pcm {
            channels,
            sample_rate: rate,
            samples: vec![1000; frames * channels as usize],
        })
    }

    #[test]
    fn mixes_and_finishes() {
        let mut m = Mixer::default();
        m.voices.push(Voice {
            id: 1,
            pcm: tone(RATE, 1, 3),
            position: 0.0,
            step: 1.0,
            gain: 1.0,
            pan: [1.0; 2],
            looping: false,
            bus: Bus::Effects,
            held: false,
        });
        let mut out = vec![0i16; 8];
        m.mix(&mut out);
        // Mono plays on both channels for three frames, then silence.
        assert_eq!(out, [1000, 1000, 1000, 1000, 1000, 1000, 0, 0]);
        assert!(m.voices.is_empty());
    }

    #[test]
    fn resamples_and_loops() {
        let mut m = Mixer::default();
        m.voices.push(Voice {
            id: 1,
            pcm: tone(RATE / 2, 2, 2),
            position: 0.0,
            step: 0.5,
            gain: 0.5,
            pan: [1.0; 2],
            looping: true,
            bus: Bus::Music,
            held: false,
        });
        let mut out = vec![0i16; 20];
        m.mix(&mut out);
        assert!(out.iter().all(|&s| s == 500));
        assert_eq!(m.voices.len(), 1);
    }

    #[test]
    fn silent_audio_does_nothing_and_says_why() {
        let audio = Audio::silent("testing");
        let id = audio.play(tone(RATE, 1, 10), 1.0, false, Bus::Dialogue);
        assert!(!audio.is_playing(id));
        assert_eq!(*audio.status, "testing");
    }
}
