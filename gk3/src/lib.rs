//! `gk3`: the Gabriel Knight 3 engine and its asset tools.

mod actions_cmd;
mod angels;
mod binocs;
mod bridge;
mod camera;
mod cast;
mod crt;
mod check;
mod chessboard;
mod cursor;
pub mod embed;
mod export;
mod faces;
mod fingerprint;
mod gps;
mod html;
mod laser_heads;
mod map;
mod mechanism;
mod movie;
mod pendulum;
mod play;
mod room;
mod saved;
mod saves;
mod scenes;
mod screens;
mod sheep_cmd;
mod shoot;
mod sidney;
mod soundtrack;
mod ui;
mod view;
mod window;
mod world;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Instant;

use gk3_barn::{Barn, Compression, Entry, Library};

const USAGE: &str = "\
gk3 - Gabriel Knight 3 engine

usage: gk3 [--data DIR] <command> [args]

commands:
  info                  Summarise the archives.
  ls [PATTERN...]       List assets, optionally only those matching a pattern.
  cat NAME              Write one asset to standard output.
  extract PATTERN...    Write matching assets to files (see --out).
  verify                Decompress every asset and check what can be checked.
  check [--no-audio]    Parse every asset with the decoder for its kind.
  export PATTERN...     Convert matching assets: bitmaps to PNG, models and
                        rooms to OBJ, sounds to PCM WAV (see --out).
  actions check         Read every action file and compile every rule's script.
  sheep check           Decode every compiled script.
  sheep disasm NAME     List a script's bytecode.
  sheep apis            The game API the scripts call, by use.
  sheep run             Run every function of every script; report and hash.
  sheep call S F        Run function F of script S and show what it did.
  play [LOC] [--time T] Play from a location (default R25 at 110A). With
                        --do NOUN:VERB (repeatable), --seconds N and --png F
                        it runs without a window and reports what happened.
                        --do WALK:X,Z clicks the floor at world X,Z;
                        --do CLICK:X,Z (or CLICK:HERE) clicks it without
                        waiting for the walk;
                        --do GPS:POWER presses the GPS's power button.
                        --do GIVE:ITEM gives the ego an item.
                        --do SKIP:ESC presses Esc (skips what is playing).
                        --do POINT:X,Y says what is under that pixel;
                        --do CALL:SCRIPT/FUNCTION runs a script function.
                        --title-screen draws the title screen; --movie NAME
                        plays a movie for --seconds and draws that frame;
                        --show-movies plays the story's movies through;
                        --where lists where every actor stands at the end;
                        --during S stops the entering scene S seconds in;
                        --size WxH draws the picture at that size.
  view [ROOM] [--time T] Fly through the rooms in a window. Starts in R25.
                        With --time (e.g. 110A, day 1 10am) rooms are set up
                        as the story has them then.
  rooms                 List the rooms (scene assets) by name.
  scenes                Every place the story can be: each timeblock and
                        location, with their names, tab-separated.
  render ROOM           Draw a room to a PNG without a window. Options:
                        --camera NAME, --size WxH, --png FILE,
                        --view x,y,z,yaw,pitch (any viewpoint),
                        --mode lit|textures|lightmaps, --seconds N,
                        --time T (set the room up as at timeblock T),
                        --msaa (smooth edges, as a window on a GPU)
                        (run the room's animations N seconds first).

options:
  --data DIR            The game's Data directory. Defaults to $GK3_DATA.
  --out DIR             Where extract and export write. Defaults to ./extracted.
  -v, --verbose         List every failure, not just the first 20.
  -h, --help            Show this.

Patterns are case-insensitive and may use * and ?, e.g. '*.BMP' or 'R25*'.";

struct Options {
    data: Option<PathBuf>,
    out: PathBuf,
    command: String,
    args: Vec<String>,
}

fn parse_args() -> Result<Options, String> {
    let mut data = None;
    let mut out = PathBuf::from("extracted");
    let mut rest = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => return Err(String::new()),
            "--data" => data = Some(args.next().ok_or("--data needs a directory")?.into()),
            "--out" => out = args.next().ok_or("--out needs a directory")?.into(),
            "-v" | "--verbose" => VERBOSE.store(true, Ordering::Relaxed),
            _ => rest.push(arg),
        }
    }
    let data = data.or_else(|| std::env::var_os("GK3_DATA").map(PathBuf::from));
    if rest.is_empty() {
        return Err(String::new());
    }
    let command = rest.remove(0);
    Ok(Options {
        data,
        out,
        command,
        args: rest,
    })
}

/// The `gk3` command: what it is asked to do, from its arguments.
pub fn main() -> ExitCode {
    let options = match parse_args() {
        Ok(o) => o,
        Err(message) => {
            if message.is_empty() {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            eprintln!("gk3: {message}");
            return ExitCode::from(2);
        }
    };

    let result = match options.command.as_str() {
        "info" => open(&options).map(|lib| info(&lib)),
        "ls" => open(&options).and_then(|lib| ls(&lib, &options.args)),
        "cat" => open(&options).and_then(|lib| cat(&lib, &options.args)),
        "extract" => open(&options).and_then(|lib| extract(&lib, &options.args, &options.out)),
        "verify" => open(&options).and_then(|lib| verify(&lib)),
        "export" => open(&options).and_then(|lib| export::run(&lib, &options.args, &options.out)),
        "actions" => open(&options).and_then(|lib| actions_cmd::run(&lib, &options.args)),
        "sheep" => open(&options).and_then(|lib| sheep_cmd::run(&lib, &options.args)),
        "face" => open(&options).and_then(|lib| faces::tool(&lib, &options.args)),
        "movie" => movie(&options),
        "play" => open(&options).and_then(|lib| play::run(lib, &options.args)),
        "view" => open(&options).and_then(|lib| view::run(lib, &options.args)),
        "rooms" => open(&options).and_then(|lib| shoot::rooms(&lib)),
        "scenes" => open(&options).and_then(|lib| scenes::run(&lib)),
        "render" => open(&options).and_then(|lib| shoot::render(&lib, &options.args)),
        "check" => open(&options)
            .and_then(|lib| check::run(&lib, options.args.iter().any(|a| a == "--no-audio"))),
        other => Err(format!("unknown command '{other}'. Try gk3 --help.")),
    };

    match result {
        Ok(code) => code,
        Err(message) => {
            eprintln!("gk3: {message}");
            ExitCode::FAILURE
        }
    }
}

/// `gk3 movie NAME.avi [--frame N] [--png FILE]`: a movie from the Data
/// directory, described, and one frame (the last by default) saved.
fn movie(options: &Options) -> Result<ExitCode, String> {
    let dir = options
        .data
        .as_deref()
        .ok_or("no data directory: pass --data or set GK3_DATA")?;
    let name = options.args.first().ok_or("movie needs a file name")?;
    let path = std::fs::read_dir(dir)
        .map_err(|e| e.to_string())?
        .flatten()
        .map(|e| e.path())
        .find(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(name))
        })
        .ok_or_else(|| format!("no {name} in {}", dir.display()))?;
    let avi = gk3_media::avi::Avi::parse(std::fs::read(&path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let value = |flag: &str| {
        options
            .args
            .iter()
            .position(|a| a == flag)
            .and_then(|i| options.args.get(i + 1))
    };
    let want = value("--frame")
        .and_then(|f| f.parse::<usize>().ok())
        .unwrap_or(usize::MAX);
    println!(
        "{name}: {}x{}, {} frames, {:.2}s{}",
        avi.width,
        avi.height,
        avi.frame_count(),
        avi.seconds(),
        avi.audio.as_ref().map_or(String::new(), |a| format!(
            ", sound {} Hz x{}",
            a.sample_rate, a.channels
        ))
    );
    if let Some(png) = value("--png") {
        let mut frames = avi.frames();
        let mut n = 0;
        while n <= want && frames.advance(&avi) {
            n += 1;
        }
        std::fs::write(png, gk3_formats::png::encode(&frames.image()))
            .map_err(|e| format!("{png}: {e}"))?;
        println!("frame {} -> {png}", n.saturating_sub(1));
    }
    Ok(ExitCode::SUCCESS)
}

fn open(options: &Options) -> Result<Library, String> {
    let dir = options.data.as_deref().ok_or(
        "no data directory. Pass --data DIR or set GK3_DATA to the game's Data directory \
         (the one holding core.brn).",
    )?;
    Library::open_dir(dir).map_err(|e| e.to_string())
}

fn info(lib: &Library) -> ExitCode {
    println!(
        "{:<14} {:>10} {:>8} {:>9}",
        "archive", "size", "assets", "pointers"
    );
    for barn in lib.barns() {
        let pointers = barn.entries().iter().filter(|e| e.is_pointer()).count();
        let size = std::fs::metadata(barn.path()).map(|m| m.len()).unwrap_or(0);
        println!(
            "{:<14} {:>10} {:>8} {:>9}",
            barn.name(),
            human(size),
            barn.entries().len() - pointers,
            pointers
        );
    }
    println!("\n{} distinct assets resolve.", lib.len());
    if !lib.unresolved().is_empty() {
        println!("{} pointer entries do not resolve:", lib.unresolved().len());
        for (asset, archive) in lib.unresolved().iter().take(10) {
            println!("  {asset} -> {archive}");
        }
    }
    ExitCode::SUCCESS
}

fn ls(lib: &Library, patterns: &[String]) -> Result<ExitCode, String> {
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    for (barn, entry) in matching(lib, patterns) {
        let line = writeln!(
            out,
            "{:<40} {:<12} {:>10} {}",
            entry.name,
            barn.name(),
            entry.stored_size,
            entry.compression
        );
        if line.is_err() {
            // Output closed, e.g. piped into `head`.
            break;
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn cat(lib: &Library, args: &[String]) -> Result<ExitCode, String> {
    let [name] = args else {
        return Err("cat takes exactly one asset name".into());
    };
    let bytes = lib.read(name).map_err(|e| e.to_string())?;
    match std::io::stdout().write_all(&bytes) {
        // The reader stopped early, as `head` does. Not an error.
        Err(e) if e.kind() != std::io::ErrorKind::BrokenPipe => Err(e.to_string()),
        _ => Ok(ExitCode::SUCCESS),
    }
}

fn extract(lib: &Library, patterns: &[String], out: &Path) -> Result<ExitCode, String> {
    if patterns.is_empty() {
        return Err("extract needs at least one pattern; use '*' for everything".into());
    }
    let assets = matching(lib, patterns);
    if assets.is_empty() {
        return Err("nothing matches".into());
    }
    std::fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;

    let failures = Mutex::new(Vec::new());
    let written = AtomicUsize::new(0);
    for_each_parallel(&assets, |(barn, entry)| {
        let path = out.join(safe_file_name(&entry.name));
        match barn
            .read(entry)
            .map_err(|e| e.to_string())
            .and_then(|bytes| {
                std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))
            }) {
            Ok(()) => {
                written.fetch_add(1, Ordering::Relaxed);
            }
            Err(e) => failures.lock().unwrap().push(e),
        }
    });

    let failures = failures.into_inner().unwrap();
    println!("Wrote {} assets to {}", written.into_inner(), out.display());
    report_failures(&failures)
}

/// Decompresses every asset each archive holds and checks the result against
/// what the bytes themselves say about their length.
fn verify(lib: &Library) -> Result<ExitCode, String> {
    let start = Instant::now();
    let owned: Vec<(&Barn, &Entry)> = lib
        .barns()
        .iter()
        .flat_map(|b| {
            b.entries()
                .iter()
                .filter(|e| !e.is_pointer())
                .map(move |e| (b, e))
        })
        .collect();
    let pointers: usize = lib
        .barns()
        .iter()
        .map(|b| b.entries().iter().filter(|e| e.is_pointer()).count())
        .sum();

    let stats = Mutex::new(Stats::default());
    let failures = Mutex::new(Vec::new());
    let done = AtomicUsize::new(0);
    let total = owned.len();

    for_each_parallel(&owned, |(barn, entry)| {
        let outcome = barn
            .read(entry)
            .map_err(|e| e.to_string())
            .and_then(|bytes| {
                let riff = check_contents(&bytes).map_err(|e| format!("{}: {e}", entry.name))?;
                Ok((bytes.len(), riff))
            });
        match outcome {
            Ok((len, riff)) => stats.lock().unwrap().add(entry.compression, len, riff),
            Err(e) => failures
                .lock()
                .unwrap()
                .push(format!("{} ({})", e, barn.name())),
        }
        let n = done.fetch_add(1, Ordering::Relaxed) + 1;
        if n.is_multiple_of(2000) || n == total {
            eprint!("\r{n}/{total}");
        }
    });
    eprintln!();

    let stats = stats.into_inner().unwrap();
    let failures = failures.into_inner().unwrap();
    println!("Archives:      {}", lib.barns().len());
    println!("Entries:       {} with data, {} pointers", total, pointers);
    println!("Unresolved:    {} pointers", lib.unresolved().len());
    println!("Distinct:      {} asset names", lib.len());
    println!(
        "Compression:   {} stored, {} zlib, {} lzo",
        stats.stored, stats.zlib, stats.lzo
    );
    println!("RIFF checked:  {}", stats.riff);
    println!("Decompressed:  {}", human(stats.bytes as u64));
    println!("Failures:      {}", failures.len());
    println!("Time:          {:.1}s", start.elapsed().as_secs_f64());
    report_failures(&failures)
}

#[derive(Default)]
struct Stats {
    stored: usize,
    zlib: usize,
    lzo: usize,
    riff: usize,
    bytes: usize,
}

impl Stats {
    fn add(&mut self, compression: Compression, len: usize, riff: bool) {
        match compression {
            Compression::Zlib => self.zlib += 1,
            Compression::Lzo => self.lzo += 1,
            _ => self.stored += 1,
        }
        self.riff += riff as usize;
        self.bytes += len;
    }
}

/// Self-consistency checks that a single wrong decompressed byte would break.
/// Returns whether there was anything to check.
fn check_contents(bytes: &[u8]) -> Result<bool, String> {
    if bytes.len() >= 12 && &bytes[..4] == b"RIFF" {
        let declared = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize + 8;
        if declared != bytes.len() {
            return Err(format!(
                "RIFF says {declared} bytes, decompressed to {}",
                bytes.len()
            ));
        }
        return Ok(true);
    }
    Ok(false)
}

/// Set by `--verbose`: list every failure rather than the first few.
static VERBOSE: AtomicBool = AtomicBool::new(false);

fn report_failures(failures: &[String]) -> Result<ExitCode, String> {
    let limit = if VERBOSE.load(Ordering::Relaxed) {
        usize::MAX
    } else {
        20
    };
    for f in failures.iter().take(limit) {
        eprintln!("  FAILED {f}");
    }
    if failures.len() > limit {
        eprintln!(
            "  ... and {} more (--verbose lists them all)",
            failures.len() - limit
        );
    }
    Ok(if failures.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

fn matching<'a>(lib: &'a Library, patterns: &[String]) -> Vec<(&'a Barn, &'a Entry)> {
    lib.assets()
        .into_iter()
        .filter(|(_, e)| patterns.is_empty() || patterns.iter().any(|p| glob(p, &e.name)))
        .collect()
}

/// Case-insensitive glob with `*` and `?`.
fn glob(pattern: &str, name: &str) -> bool {
    let p: Vec<char> = pattern.chars().map(|c| c.to_ascii_uppercase()).collect();
    let n: Vec<char> = name.chars().map(|c| c.to_ascii_uppercase()).collect();
    let (mut pi, mut ni) = (0, 0);
    let mut backtrack = None;
    while ni < n.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == n[ni]) {
            pi += 1;
            ni += 1;
        } else if pi < p.len() && p[pi] == '*' {
            backtrack = Some((pi, ni));
            pi += 1;
        } else if let Some((bp, bn)) = backtrack {
            pi = bp + 1;
            ni = bn + 1;
            backtrack = Some((bp, bn + 1));
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|&c| c == '*')
}

/// Asset names are bare file names, but never trust that when writing files.
fn safe_file_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c == '/' || c == '\\' || c == '\0' || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
    match cleaned.as_str() {
        "" | "." | ".." => format!("_{cleaned}"),
        _ => cleaned,
    }
}

/// Runs `f` over `items` on every core.
fn for_each_parallel<T: Sync>(items: &[T], f: impl Fn(&T) + Sync) {
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    let next = AtomicUsize::new(0);
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(item) = items.get(i) else { break };
                    f(item);
                }
            });
        }
    });
}

fn human(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glob_matches() {
        assert!(glob("*.bmp", "R25_WALL.BMP"));
        assert!(glob("r25*", "R25_WALL.BMP"));
        assert!(glob("R2?_*.BMP", "R25_WALL.BMP"));
        assert!(glob("*", "ANYTHING"));
        assert!(glob("*A*B*", "xxAyyBzz"));
        assert!(!glob("*.WAV", "R25_WALL.BMP"));
        assert!(!glob("R25", "R25_WALL.BMP"));
    }

    #[test]
    fn file_names_cannot_escape() {
        assert_eq!(safe_file_name("../../etc/passwd"), ".._.._etc_passwd");
        assert_eq!(safe_file_name(".."), "_..");
        assert_eq!(safe_file_name("OK.TXT"), "OK.TXT");
    }

    #[test]
    fn riff_size_check() {
        let mut wav = b"RIFF".to_vec();
        wav.extend(8u32.to_le_bytes());
        wav.extend(b"WAVEdata");
        assert_eq!(check_contents(&wav), Ok(true));
        wav.push(0);
        assert!(check_contents(&wav).is_err());
    }
}
