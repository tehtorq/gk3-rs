# gk3-rs: a dependency-free Gabriel Knight 3 engine in Rust

A from-scratch engine that plays *Gabriel Knight 3: Blood of the Sacred, Blood of
the Damned* (Sierra, 1999) from a legally owned copy of the game's `Data`
directory. Licensed GPL-3.0-or-later.

## Goals

1. **One file, nothing to install.** The release is a single executable. No
   runtime (.NET, JVM), no bundled `.so` files, no `LD_LIBRARY_PATH`, no
   launcher script.
2. **Plays the whole game** from the eight retail `.brn` archives plus the
   loose `.bik`/`.avi` movies, with no offline conversion step.
3. **Runs on a modern Linux desktop out of the box**, Wayland or X11, on any
   Vulkan- or OpenGL-capable GPU. Windows and macOS follow from the same code.

Non-goals for 1.0: remastered assets, ray tracing, VR, new UI paradigms. These
can come later; the first job is faithful and robust.

## What "no dependencies" means precisely

Rust crates are compiled into the executable, so they are not runtime
dependencies. What remains is what *every* game must take from the system:

| Needs | How we get it | Linked at build time? |
|---|---|---|
| C library | glibc | Yes: `libc`, `libm` only |
| GPU driver | `libvulkan.so.1` / `libEGL.so.1`, loaded with `dlopen` at startup | No |
| Display | `libwayland-client` / `libxkbcommon` or `libX11`, `dlopen` | No |
| Audio | PipeWire / PulseAudio / ALSA, `dlopen`, falling back silently | No |

**Acceptance test:** `ldd target/release/gk3` lists nothing beyond
`linux-vdso`, `libc`, `libm`, `ld-linux` (and possibly `libgcc_s`).

Why not a fully static musl binary: a static musl executable cannot `dlopen`,
and a GPU driver cannot be anything but loaded from the system. Instead we
link glibc dynamically and build against an old baseline (glibc 2.28 via
`cargo zigbuild`) so the binary runs on any distro from roughly 2018 onwards.

Missing audio is a warning and a silent game, never a refusal to start. A
missing GPU driver is a clear message naming what to install.

## Lessons from the two existing engines

Both are GPL-3.0 and are used here as references (see *Licensing*).

| | GK3Reborn (C#/.NET 10) | G-Engine (C++) |
|---|---|---|
| Runtime | needs .NET 10 installed | none |
| Native libs shipped | GLFW, OpenAL, shaderc, SPIRV-Cross | SDL2, GLEW, FMOD, FFmpeg (5 libs), libpng, zlib |
| Movies | needs separately converted packs | FFmpeg |
| Problems seen on Fedora 44 / KDE Wayland | wrong `libs/` dir under distro .NET (RID `fedora.44-x64`); picks llvmpipe over the real GPU; Wayland protocol error *"attached a buffer before configure event"* kills the window | (not tested here) |

What we take from that:

- **Never present to a window before the compositor configures it.** Let the
  windowing layer (winit) own the surface lifecycle; create the swapchain on
  the first `Resized`/`RedrawRequested`, not before. Treat
  `SURFACE_LOST`/`OUTDATED` as recoverable and log the real error code.
- **Pick the GPU deliberately.** Prefer discrete > integrated > virtual > CPU;
  never pick a software rasteriser when hardware exists. Allow an override.
- **Find nothing by path at runtime** except the game data. Everything else is
  inside the binary.
- **Decode everything in-process.** GK3Reborn proved MP3 and video can be done
  without native libraries; we do the same in Rust, without the transcoding step.

## The content, by the numbers

From GK3Reborn's corpus survey of the retail English data (36,957 assets,
1,256 MB decompressed):

| Kind | Count | Format notes |
|---|---:|---|
| Text (SIF, NVC, STK, GAS, YAK text…) | 13,635 | Latin-1, CRLF, INI-like dialects |
| Audio | 7,852 | RIFF wrappers; **97.5% are MP3** inside (format tag 85) |
| GK3 bitmaps | 6,330 | raw 16-bit `61nM` container |
| Actor animations (`.ACT`) | 5,796 | per-frame vertex positions, no skeletons |
| Animation scripts (`.ANM`, `.YAK`) | 14,234 | keyframed events, dialogue + lip sync |
| Models (`.MOD`) | 1,878 | `LDOM` mesh groups |
| Windows BMPs | 328 | standard |
| Lightmaps (`.MUL`) | 226 | 181,089 lightmaps, vary by time of day |
| Compiled Sheep scripts (`.SHP`) | 224 | the game's logic, bytecode VM |
| Fonts (`.FON`) | 138 | bitmap fonts |
| Scenes (`.BSP`) | 110 | BSP geometry for every room |
| SIDNEY documents | 417 | HTML-ish |
| Movies | 26 BIK + 14 AVI | Bink 1 320×240 + Bink audio (RDFT); AVIs are RLE8 + PCM |

Archive facts that shape the loader: `core.brn` holds 20,991 *pointer* entries
into the other seven; compression is none / zlib / LZO1X; LZO streams end at a
`11 00 00` marker before their recorded size; names are case-insensitive; later
directories win.

The archives also contain the original team's design documents (e.g. `SHEEP
ENGINE.DOC`, `SIF.DOC`, `NVC.DOC`). These are the authoritative specs and should
be read locally from the user's own copy, **never committed or quoted wholesale**.

## Architecture

A Cargo workspace. Each crate is testable on its own, and the lower layers have
no knowledge of rendering or the game.

```
gk3-rs/
├─ crates/
│  ├─ barn/        .brn archives: TOC, pointers, zlib (miniz_oxide), LZO1X (ours)
│  ├─ formats/     parsers: bitmaps, MOD, BSP, MUL, ACT, ANM/YAK, FON,
│  │               and the text dialects (SIF, NVC, STK, GAS, SCN, faces)
│  ├─ sheep/       .SHP loader, VM with resumable waits, API registry;
│  │               later a source compiler for the dev console
│  ├─ media/       MP3 (symphonia), Bink 1 video+audio, AVI RLE8
│  ├─ render/      wgpu renderer: BSP + lightmaps, models, sprites/UI, movies
│  ├─ audio/       mixer, 3D placement (inverse-clamped rolloff), buses
│  ├─ platform/    winit window + input, audio device, paths, settings
│  └─ game/        scenes, actors, walking/pathfinding, cameras, NVC verbs,
│                  dialogue, inventory, SIDNEY, timeblocks, saves
└─ gk3/            the binary: game + tool subcommands (ls, extract, verify…)
```

### Key crate choices

| Need | Choice | Why |
|---|---|---|
| Window/input | `winit` | pure Rust; `dlopen`s Wayland/X11 libs; handles configure correctly |
| GPU | `wgpu` | Vulkan, GL, Metal, DX12 from one API; `dlopen`s the driver; WGSL shaders compiled at build |
| zlib | `miniz_oxide` | pure Rust |
| LZO1X | our own (~150 lines) | decompressor is tiny; existing crates bind C |
| MP3 | `symphonia` | pure Rust, mature |
| Bink 1 | port, see *Video* | no crates.io crate |
| Audio out | `cpal`, made runtime-loaded | default ALSA backend links `libasound` at build time; must be made to `dlopen` (feature or thin own backend) to meet the `ldd` test |
| Math | `glam` | |
| Images out (tools) | `png` | pure Rust, tooling only |
| Errors/logging | `thiserror`, `log` + a small logger | |

No C code in the build. `cargo build` needs only the Rust toolchain.

### Video

Bink 1 is the one codec with no off-the-shelf pure-Rust crate. Options, in
order of preference:

1. **NihAV's Bink decoder** (pure Rust, includes Bink 1 video and audio). It is
   AGPL-3.0, which GPL-3.0 code may combine with; its author also offers to
   relicense parts under other free licences on request, which would be cleaner.
2. **Port FFmpeg's `bink.c` / `binkaudio.c`** (LGPL-2.1+, GPL-compatible).
   Roughly 2,000 lines of C plus a DCT and an RDFT.

Either way the decoder is checked frame-by-frame against FFmpeg output on the
user's copy of the 26 movies (a local test, not CI). AVIs are RLE8 + PCM, a few
hundred lines.

### The Sheep VM

The game's logic is 224 compiled scripts calling ~130 gameplay functions (out of
305 specified; the rest are developer/console). The spec classifies each
function as `IMMEDIATE` or `WAIT`, and that drives the design: **a script is a
resumable coroutine**. A `wait` on an animation, movie or line of dialogue
suspends the VM and is resumed by the game loop when the event finishes. No
threads. The function registry is generated from a table, and unimplemented
calls are logged once and recorded, never silently ignored.

### Frame loop

Fixed-timestep simulation (60 Hz), variable-rate rendering, single game thread.
Movie decoding and audio decoding run on worker threads and hand off finished
frames/buffers. All randomness comes from one seeded RNG, so a run can be
replayed from its inputs for testing.

## Roadmap

Each phase ends with something you can run, and an exit test.

| # | Phase | Deliverable | Exit test |
|---|---|---|---|
| 0 | Skeleton | workspace, CI, release profile, glibc-2.28 build | `ldd` shows only libc/libm |
| 1 | Archives | `gk3 ls`, `gk3 extract`, `gk3 verify` | all 36,957 entries decode; 20,991 pointers resolved; WAV RIFF sizes match decompressed lengths |
| 2 | Asset decoders | bitmaps→PNG, MOD/BSP readers, MP3 decode, text-dialect parsers | every asset of each kind parses; spot checks against G-Engine |
| 3 | Scene viewer | window, BSP + textures + lightmaps, free camera, room picker | every one of the 110 rooms renders; lightmaps change by timeblock |
| 4 | Actors | models placed from SIF, ACT vertex animation, faces | Gabriel stands in R25 and plays an idle/walk clip |
| 5 | Sheep VM | headless runner over all 224 scripts | every script loads; state functions behave; trace output stable |
| 6 | First playable room | SIF cameras, walk areas + pathfinding, click → NVC verb menu, dialogue with voice + captions | **Day 1 10am in R25 is playable**: walk, look, talk, pick up |
| 7 | Audio | soundtracks (STK), positional sound, buses, volume settings | room ambience and footsteps correct in a sample of rooms |
| 8 | Movies | Bink + AVI playback, waitable from Sheep | all 26 BIK + 14 AVI play; intro sequence works |
| 9 | Interface | inventory, SIDNEY (docs, analysis, map/LSR puzzle), driving map, menus, save/load | each system usable; save/load round-trips |
| 10 | Whole game | timeblock progression, all days | full playthrough to the end credits |
| 11 | Polish | widescreen/resolution, controller, settings, Windows + macOS builds | release |

Phases 1–3 are a solid foundation and a satisfying first visible result. Phase 6
is the point where it becomes a game. Phases 9–10 are the long tail: most of the
work there is game-specific behaviour, not engine code.

## Size of the job

G-Engine is about 3.4 MB of C++ (~540 files), split roughly evenly between
engine and game logic. Expect gk3-rs to land in the 60–100k-line range. The
lower phases are well specified by existing docs; the upper phases are where
behavioural fidelity takes time.

## Risks

| Risk | Mitigation |
|---|---|
| Bink decoder effort | use or port an existing one; test against FFmpeg locally |
| Sheep semantics (waits, ordering, edge cases) | coroutine VM from day one; trace and compare with G-Engine runs |
| Retail data quirks (zeroed headers, overrunning entries, dangling references) | corpus-wide tests from phase 1 onward; quirks documented as they are found |
| Audio backend linking `libasound` at build time | runtime-loaded backend; `ldd` check in CI |
| Wayland/X11 presentation bugs | winit owns surface lifecycle; test on KDE and GNOME, Wayland and X11 |

## Testing and assets

- **No game data in the repository, ever.** Corpus tests read the archives from
  `GK3_DATA` (e.g. `~/Games/Heroic/Gabriel Knight 3/Data`) and are skipped
  when it is unset, so CI runs the unit tests only.
- Unit tests use small synthetic inputs built in the test itself.
- `gk3 verify` doubles as the corpus regression test.

## Licensing

GPL-3.0-or-later. GK3Reborn and G-Engine are both GPL-3.0; their documentation
and code are used as references, and logic ported from them is credited in
`NOTICE`. A NihAV-derived Bink decoder would be AGPL-3.0 unless relicensed.
Sierra's assets and design documents are never redistributed.

## References

- GK3Reborn: <https://github.com/Devion/GK3Reborn> (`docs/formats/` in particular)
- G-Engine: <https://github.com/kromenak/gengine>
- NihAV: <https://nihav.org/overview.html>
- Gabriel Knight 3 Secrets: <http://bonny.ploeg.ws/gk3secret.html>
