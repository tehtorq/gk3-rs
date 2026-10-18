//! Gabriel Knight 3 in a web browser. The engine runs in a Web Worker: the
//! page hands it the canvas (as an `OffscreenCanvas`) and the player's own
//! copy of the game's `Data` files, picked on the page or served beside it
//! by `scripts/gk3-web`, which are read a piece at a time as the engine asks
//! for them, as on disk.

#![cfg(target_arch = "wasm32")]

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use std::rc::Rc;

use gk3::embed::{CursorArt, Frontend, Input, KeyCode, MouseButton};
use gk3_barn::{Barn, Library, Storage};
use gk3_render::Renderer;
use js_sys::{Array, Float32Array, Uint8Array};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use web_sys::{
    DedicatedWorkerGlobalScope, File, FileReaderSync, MessageEvent, MessagePort, OffscreenCanvas, XmlHttpRequest,
    XmlHttpRequestResponseType,
};

/// One of the player's files, read a slice at a time. A worker reads
/// files synchronously, as the engine does.
struct BlobFile {
    file: File,
    len: u64,
}

// A wasm module without threads runs on one thread only: the file is never
// touched from another.
unsafe impl Send for BlobFile {}
unsafe impl Sync for BlobFile {}

fn js_err(e: JsValue) -> io::Error {
    io::Error::other(format!("{e:?}"))
}

/// The bytes of part of a file.
fn read_slice(file: &File, start: u64, end: u64) -> io::Result<Uint8Array> {
    let blob = file
        .slice_with_f64_and_f64(start as f64, end as f64)
        .map_err(js_err)?;
    let reader = FileReaderSync::new().map_err(js_err)?;
    let buffer = reader.read_as_array_buffer(&blob).map_err(js_err)?;
    Ok(Uint8Array::new(&buffer))
}

impl Storage for BlobFile {
    fn size(&self) -> io::Result<u64> {
        Ok(self.len)
    }

    fn read_at(&self, buf: &mut [u8], offset: u64) -> io::Result<usize> {
        if offset >= self.len {
            return Ok(0);
        }
        let end = (offset + buf.len() as u64).min(self.len);
        let bytes = read_slice(&self.file, offset, end)?;
        let n = bytes.length() as usize;
        bytes.copy_to(&mut buf[..n]);
        Ok(n)
    }
}

/// One of the files the page's own server serves from the Data folder
/// (`scripts/gk3-web`), read a byte range at a time. A small read brings in
/// the block around it, which the reads that follow it mostly fall in.
struct ServedFile {
    url: String,
    len: u64,
    block: RefCell<Option<(u64, Vec<u8>)>>,
}

unsafe impl Send for ServedFile {}
unsafe impl Sync for ServedFile {}

const BLOCK: u64 = 256 * 1024;

/// Bytes `start..end` of what the server has at `url` (all of it with no
/// range), waiting for them, as a worker may.
fn fetch(url: &str, range: Option<(u64, u64)>) -> io::Result<Vec<u8>> {
    let request = XmlHttpRequest::new().map_err(js_err)?;
    request.open_with_async("GET", url, false).map_err(js_err)?;
    request.set_response_type(XmlHttpRequestResponseType::Arraybuffer);
    if let Some((start, end)) = range {
        request
            .set_request_header("Range", &format!("bytes={start}-{}", end - 1))
            .map_err(js_err)?;
    }
    request.send().map_err(js_err)?;
    let status = request.status().map_err(js_err)?;
    if !(status == 200 && range.is_none() || status == 206) {
        return Err(io::Error::other(format!("{url}: HTTP {status}")));
    }
    let bytes = Uint8Array::new(&request.response().map_err(js_err)?);
    Ok(bytes.to_vec())
}

impl Storage for ServedFile {
    fn size(&self) -> io::Result<u64> {
        Ok(self.len)
    }

    fn read_at(&self, buf: &mut [u8], offset: u64) -> io::Result<usize> {
        if offset >= self.len {
            return Ok(0);
        }
        let end = (offset + buf.len() as u64).min(self.len);
        let first = offset / BLOCK * BLOCK;
        if end > first + BLOCK {
            // Bigger than a block: just what was asked for.
            let bytes = fetch(&self.url, Some((offset, end)))?;
            buf[..bytes.len()].copy_from_slice(&bytes);
            return Ok(bytes.len());
        }
        let mut block = self.block.borrow_mut();
        if block.as_ref().is_none_or(|(at, _)| *at != first) {
            let bytes = fetch(&self.url, Some((first, (first + BLOCK).min(self.len))))?;
            *block = Some((first, bytes));
        }
        let (_, bytes) = block.as_ref().unwrap();
        let from = (offset - first) as usize;
        let n = ((end - offset) as usize).min(bytes.len().saturating_sub(from));
        buf[..n].copy_from_slice(&bytes[from..from + n]);
        Ok(n)
    }
}

/// The files lying loose beside the archives (the movies): the player's
/// own, or the server's.
enum Loose {
    Picked(Vec<File>),
    Served(String, Vec<String>),
}

unsafe impl Send for Loose {}
unsafe impl Sync for Loose {}

impl Loose {
    fn read(&self, name: &str) -> Option<Vec<u8>> {
        match self {
            Loose::Picked(files) => {
                let file = files.iter().find(|f| f.name().eq_ignore_ascii_case(name))?;
                let bytes = read_slice(file, 0, file.size() as u64).ok()?;
                Some(bytes.to_vec())
            }
            Loose::Served(base, names) => {
                let found = names.iter().find(|n| n.eq_ignore_ascii_case(name))?;
                fetch(&format!("{base}{}", js_sys::encode_uri_component(found)), None).ok()
            }
        }
    }
}

/// The game's library from archives, by name, and the loose files.
fn library_of(archives: Vec<(String, Box<dyn Storage>)>, loose: Loose) -> Result<Library, String> {
    if archives.is_empty() {
        return Err("no .brn archives among the files: choose the game's Data folder".into());
    }
    let mut barns = archives
        .into_iter()
        .map(|(name, storage)| Barn::from_storage(PathBuf::from(name), storage).map_err(|e| e.to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    // As on disk: the archives in name order.
    barns.sort_by_key(|b| b.name().to_ascii_lowercase());
    Ok(Library::from_barns(barns).with_loose(Box::new(move |name| loose.read(name))))
}

fn is_archive(name: &str) -> bool {
    name.to_ascii_lowercase().ends_with(".brn")
}

/// The game's library from the player's files: every `.brn` archive, and
/// the rest as loose files.
fn library(files: &Array) -> Result<Library, String> {
    let mut archives: Vec<(String, Box<dyn Storage>)> = Vec::new();
    let mut loose = Vec::new();
    for value in files.iter() {
        let file: File = value.dyn_into().map_err(|_| "not a file".to_string())?;
        if is_archive(&file.name()) {
            let len = file.size() as u64;
            archives.push((file.name(), Box::new(BlobFile { file, len })));
        } else {
            loose.push(file);
        }
    }
    library_of(archives, Loose::Picked(loose))
}

/// The game's library from the server's Data folder at `base` (ending in
/// `/`), as its `index.json` lists it.
fn served_library(base: &str) -> Result<Library, String> {
    let index = fetch(&format!("{base}index.json"), None).map_err(|e| e.to_string())?;
    let index: JsValue = js_sys::JSON::parse(&String::from_utf8_lossy(&index)).map_err(|e| format!("{e:?}"))?;
    let mut archives: Vec<(String, Box<dyn Storage>)> = Vec::new();
    let mut loose = Vec::new();
    for entry in Array::from(&index).iter() {
        let name = js_sys::Reflect::get(&entry, &"name".into())
            .ok()
            .and_then(|v| v.as_string())
            .ok_or("index.json: an entry has no name")?;
        let len = js_sys::Reflect::get(&entry, &"size".into())
            .ok()
            .and_then(|v| v.as_f64())
            .ok_or("index.json: an entry has no size")? as u64;
        if is_archive(&name) {
            let url = format!("{base}{}", js_sys::encode_uri_component(&name));
            archives.push((
                name,
                Box::new(ServedFile {
                    url,
                    len,
                    block: RefCell::new(None),
                }),
            ));
        } else {
            loose.push(name);
        }
    }
    library_of(archives, Loose::Served(base.to_string(), loose))
}

fn log(text: &str) {
    web_sys::console::log_1(&text.into());
}

/// What the browser calls for each frame.
type Tick = Closure<dyn FnMut(f64)>;

/// The game and the canvas it draws to, once started.
struct Running {
    game: Frontend,
    canvas: OffscreenCanvas,
    sound: Sound,
    /// The CSS cursor up, and those made so far, by name, scale and frame.
    cursor: Option<String>,
    cursors: HashMap<(String, u32, usize), String>,
}

/// How far ahead of what is playing the mix is kept, in seconds: enough to
/// ride out a frame or two that takes long.
const LEAD: f64 = 0.15;

/// The mix's way to the page's Web Audio output: an `AudioWorklet` there
/// plays what is sent, and says now and then how much it has still queued.
struct Sound {
    port: Option<MessagePort>,
    /// The output's last word (frames queued, and when), shared with the
    /// port's handler; and frames sent since it (and when it was).
    queued: Rc<Cell<Option<(f64, f64)>>>,
    sent: f64,
    sent_since: f64,
    /// When the mix was last taken.
    mixed: f64,
}

impl Sound {
    fn new(port: Option<MessagePort>) -> Self {
        let queued = Rc::new(Cell::new(None));
        if let Some(port) = &port {
            let heard = queued.clone();
            let on = Closure::<dyn FnMut(MessageEvent)>::new(move |e: MessageEvent| {
                if let Some(frames) = e.data().as_f64() {
                    heard.set(Some((frames, now())));
                }
            });
            port.set_onmessage(Some(on.as_ref().unchecked_ref()));
            on.forget();
        }
        Self {
            port,
            queued,
            sent: 0.0,
            sent_since: 0.0,
            mixed: now(),
        }
    }

    /// Mixes what is due: enough to keep the output `LEAD` ahead while it
    /// is playing, or, with no output playing (none, or the page's not yet
    /// let to start), as much as has gone by, unheard, so that sounds
    /// still end when they should.
    fn feed(&mut self, audio: &gk3_audio::Audio) {
        let rate = gk3_audio::RATE as f64;
        let now = now();
        let since = (now - self.mixed).min(0.5);
        self.mixed = now;
        let live = self.port.as_ref().zip(self.queued.get().filter(|(_, at)| now - at < 0.25));
        let frames = match live {
            Some((_, (queued, at))) => {
                if at != self.sent_since {
                    self.sent = 0.0;
                    self.sent_since = at;
                }
                let queued = (queued + self.sent - (now - at) * rate).max(0.0);
                (LEAD * rate - queued).max(0.0)
            }
            None => since * rate,
        } as usize;
        if frames == 0 {
            return;
        }
        let mut mix = vec![0i16; frames * 2];
        audio.mix(&mut mix);
        if let Some((port, _)) = live {
            let samples: Vec<f32> = mix.iter().map(|&s| s as f32 / 32768.0).collect();
            let chunk = Float32Array::from(samples.as_slice());
            let _ = port.post_message_with_transferable(&chunk, &Array::of1(&chunk.buffer()));
            self.sent += frames as f64;
        }
    }
}

/// Bytes as base64, for a `data:` URL.
fn base64(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, &b)| n | (b as u32) << (16 - 8 * i));
        for i in 0..4 {
            out.push(if i <= chunk.len() {
                DIGITS[(n >> (18 - 6 * i) & 63) as usize] as char
            } else {
                '='
            });
        }
    }
    out
}

/// One frame of the game's cursor as a CSS cursor.
fn css_cursor(art: &CursorArt, frame: usize) -> String {
    let image = &art.frames[frame.min(art.frames.len() - 1)];
    let png = gk3_formats::png::encode(image);
    format!(
        "url(\"data:image/png;base64,{}\") {} {}, default",
        base64(&png),
        art.hotspot.0,
        art.hotspot.1
    )
}

thread_local! {
    static RUNNING: RefCell<Option<Running>> = const { RefCell::new(None) };
}

fn scope() -> DedicatedWorkerGlobalScope {
    js_sys::global().unchecked_into()
}

/// The time now, in seconds, by the worker's clock (the one its animation
/// frames are timed by).
fn now() -> f64 {
    scope().performance().map_or(0.0, |p| p.now()) / 1000.0
}

/// Tells the page something: `{key: value}`.
fn tell(key: &str, value: JsValue) {
    let message = js_sys::Object::new();
    let _ = js_sys::Reflect::set(&message, &key.into(), &value);
    let _ = scope().post_message(&message);
}

/// Gives the game what the player did, and passes on its log.
fn feed(input: Input) {
    RUNNING.with_borrow_mut(|running| {
        if let Some(r) = running {
            r.game.input(now(), input);
            for line in r.game.take_log() {
                log(&line);
            }
        }
    });
}

/// The key the page names, as `KeyboardEvent.code` does: the same names as
/// the engine's.
fn key_code(name: &str) -> Option<KeyCode> {
    use KeyCode::*;
    const KEYS: &[KeyCode] = &[
        KeyA, KeyB, KeyC, KeyD, KeyE, KeyF, KeyG, KeyH, KeyI, KeyJ, KeyK, KeyL, KeyM, KeyN, KeyO, KeyP, KeyQ, KeyR,
        KeyS, KeyT, KeyU, KeyV, KeyW, KeyX, KeyY, KeyZ, Digit0, Digit1, Digit2, Digit3, Digit4, Digit5, Digit6,
        Digit7, Digit8, Digit9, F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12, Escape, Enter, NumpadEnter, Tab,
        Space, Backspace, Delete, Period, Comma, Minus, Equal, Slash, Semicolon, Quote, ArrowUp, ArrowDown,
        ArrowLeft, ArrowRight, PageUp, PageDown, Home, End, Pause, ShiftLeft, ShiftRight, ControlLeft,
        ControlRight, AltLeft, AltRight,
    ];
    KEYS.iter().copied().find(|k| format!("{k:?}") == name)
}

/// The pointer moved to `x`, `y`, in the canvas's pixels.
#[wasm_bindgen]
pub fn moved(x: f32, y: f32) {
    feed(Input::Moved(x, y));
}

/// A mouse button (as `MouseEvent.button`: 0 left, 1 middle, 2 right) went
/// down or up.
#[wasm_bindgen]
pub fn mouse(button: u8, down: bool) {
    let button = match button {
        0 => MouseButton::Left,
        1 => MouseButton::Middle,
        2 => MouseButton::Right,
        n => MouseButton::Other(n.into()),
    };
    feed(Input::Mouse { button, down });
}

/// The wheel turned, `dy` pixels, up positive.
#[wasm_bindgen]
pub fn wheel(dy: f32) {
    feed(Input::Wheel(dy));
}

/// A key (`KeyboardEvent.code`) went down or up, typing `text`, if any.
#[wasm_bindgen]
pub fn key(code: String, text: Option<String>, down: bool, repeat: bool) {
    if let Some(code) = key_code(&code) {
        feed(Input::Key {
            code,
            text,
            down,
            repeat,
        });
    }
}

/// The canvas is now `width` by `height` pixels.
#[wasm_bindgen]
pub fn resized(width: u32, height: u32) {
    RUNNING.with_borrow_mut(|running| {
        if let Some(r) = running {
            r.canvas.set_width(width.max(1));
            r.canvas.set_height(height.max(1));
        }
    });
    feed(Input::Resized(width, height));
}

/// Starts the game in the worker, as `gk3 play` would with `args` (none: a
/// new game), drawn to `canvas` from then on, a frame each time the browser
/// asks.
#[wasm_bindgen]
/// `files` are the player's own, picked on the page, or with none, those the
/// page's server has at `data/`.
/// `audio` is the way to the page's sound output, if it has one.
pub async fn start(
    canvas: OffscreenCanvas,
    files: Option<Array>,
    audio: Option<MessagePort>,
    args: Vec<String>,
) -> Result<(), JsValue> {
    std::panic::set_hook(Box::new(|info| web_sys::console::error_1(&info.to_string().into())));
    let lib = match files {
        Some(files) => library(&files)?,
        None => served_library("data/")?,
    };
    let lib: &'static Library = Box::leak(Box::new(lib));
    log(&format!("{} archives, {} assets", lib.barns().len(), lib.assets().len()));

    let instance = wgpu::util::new_instance_with_webgpu_detection(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::BROWSER_WEBGPU | wgpu::Backends::GL,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    })
    .await;
    let size = (canvas.width().max(1), canvas.height().max(1));
    let surface = instance
        .create_surface(wgpu::SurfaceTarget::OffscreenCanvas(canvas.clone()))
        .map_err(|e| format!("cannot draw to the canvas: {e}"))?;
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: Some(&surface),
            ..Default::default()
        })
        .await
        .map_err(|e| format!("no GPU for the canvas: {e}"))?;
    let info = adapter.get_info();
    log(&format!("GPU: {info:?}"));
    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: Some("gk3"),
            required_limits: wgpu::Limits::downlevel_webgl2_defaults().using_resolution(adapter.limits()),
            ..Default::default()
        })
        .await
        .map_err(|e| format!("could not open the GPU: {e}"))?;
    let caps = surface.get_capabilities(&adapter);
    let format = caps
        .formats
        .iter()
        .copied()
        .find(|f| !f.is_srgb())
        .or(caps.formats.first().copied())
        .ok_or("the canvas offers no formats")?;
    let mut config = surface
        .get_default_config(&adapter, size.0, size.1)
        .ok_or("the canvas cannot be drawn to by this GPU")?;
    config.format = format;
    surface.configure(&device, &config);

    let mut renderer = Renderer::new(device, queue, format);
    renderer.adapter_name = info.name;
    let game = Frontend::new(lib, &args, surface, config, renderer)?;
    RUNNING.with_borrow_mut(|running| {
        *running = Some(Running {
            game,
            canvas,
            sound: Sound::new(audio),
            cursor: None,
            cursors: HashMap::new(),
        })
    });

    // A frame each time the browser asks for one.
    let tick: Rc<RefCell<Option<Tick>>> = Rc::new(RefCell::new(None));
    let again = tick.clone();
    *tick.borrow_mut() = Some(Closure::new(move |_: f64| {
        let quit = RUNNING.with_borrow_mut(|running| {
            let Some(r) = running else {
                return true;
            };
            r.game.frame(now());
            for line in r.game.take_log() {
                log(&line);
            }
            r.sound.feed(r.game.audio());
            let cursors = &mut r.cursors;
            let cursor = match r.game.cursor() {
                Some(((name, scale), art, frame)) => cursors
                    .entry((name.clone(), *scale, frame))
                    .or_insert_with(|| css_cursor(art, frame))
                    .clone(),
                None => "none".into(),
            };
            if r.cursor.as_ref() != Some(&cursor) {
                tell("cursor", cursor.as_str().into());
                r.cursor = Some(cursor);
            }
            r.game.quit()
        });
        if quit {
            tell("quit", true.into());
            return;
        }
        if let Some(f) = again.borrow().as_ref() {
            let _ = scope().request_animation_frame(f.as_ref().unchecked_ref());
        }
    }));
    if let Some(f) = tick.borrow().as_ref() {
        scope().request_animation_frame(f.as_ref().unchecked_ref())?;
    }
    Ok(())
}
