# gk3-rs

A Gabriel Knight 3 engine in Rust that ships as a single executable: no
runtime to install, no bundled shared libraries. It plays the game from your
own copy's `Data` directory. See [PLAN.md](PLAN.md) for the design and roadmap.

**Status:** phase 6 of 11 done: the first room is playable. Every asset decodes, and every room can be
flown through in a window, textured, lightmapped for its time of day and with
its skybox. Characters and props stand where the scene puts them and run their
idle animations (Gabriel breathes and fidgets; the lobby fans turn). The
game's 224 compiled Sheep scripts run on a virtual machine against a model of
the story's state, and rooms can be set up as the story has them at any time
of day.

Gabriel's hotel room on day one is playable: click the floor to walk, click
anything to get the original verb icons, and every action plays as the game
wrote it, with the right camera cuts, animations, captions and recorded
voice, and the room's ambient soundtrack. Leaving the room loads the next one.
Not yet: lip sync, the inventory screen, SIDNEY, conversations with a topic
menu, movies, saving.

## Playing

```sh
gk3 play                     # day 1, 10am, Gabriel's room
gk3 play LBY --time 207A     # anywhere, any time
```

Left-click the floor to walk; left-click a thing to see what can be done to it
and click a verb icon; right-click or Esc closes the menu; while someone is
speaking, a click or Space moves on to the next line. Esc with no menu open
quits. Sound comes from PulseAudio or PipeWire if they are there; without
them the game is silent rather than refusing to start.

For testing, the same command runs without a window:

```sh
gk3 play R25 --list                        # everything that can be done now
gk3 play R25 --do WINDOW:OPEN --png out.png # do it, report, take a picture
```

## Building

```sh
cargo build --release
scripts/check-deps.sh   # confirms only the C runtime is linked
```

## Using

Point it at the game's `Data` directory (the one holding `core.brn`) with
`--data`, or set `GK3_DATA`:

```sh
export GK3_DATA="$HOME/Games/Heroic/Gabriel Knight 3/Data"
gk3 info                  # what is in each archive
gk3 ls '*.SIF'            # list assets matching a pattern
gk3 cat R25.SIF           # print one asset
gk3 extract 'R25*'        # write matching assets to ./extracted, as stored
gk3 export 'R25*'         # convert them: PNG, OBJ+MTL, PCM WAV
gk3 verify                # decompress and check every asset
gk3 check                 # parse every asset with the decoder for its kind
gk3 view R25              # fly through Gabriel's hotel room (see below)
gk3 render RC1 --camera FR_LBY --png square.png   # one frame, no window
gk3 render all --png rooms                        # every room, one PNG each
gk3 render R25_M --view 210,45,150,164,8 --seconds 4   # any viewpoint, 4s in
gk3 view R25 --time 207A  # the room as it is on day 2 at 7am
gk3 sheep run             # run every function of every script
gk3 sheep call MS2307A Grace_Buy_Tomb --time 307A      # one, with its trace
```

In the viewer: WASD or the arrow keys move, Q and E go down and up, Shift is
faster, and dragging with a mouse button looks around. Tab and Shift+Tab step
through all 229 rooms, C cycles a room's own cameras, M switches between lit,
textures only and lightmaps only, and Esc quits.

Against the retail English release:

- `verify`: 36,957 assets and 20,991 pointer entries across 8 archives,
  no failures.
- `actions check`: all 390 action files, 6,041 rules over 964 nouns and 221
  verbs, every rule's script compiling to Sheep bytecode.
- `play R25 --time 110A`: every one of the 52 things that can be done in
  Gabriel's room on day one runs to completion with its line, sound and
  animation found.
- `sheep check`: 224 scripts, 1,481 functions and 147,223 instructions decode
  completely, calling 139 distinct API functions; `sheep run` finishes 1,468
  functions, leaves 13 background loops waiting on the live game, faults
  none, and hashes the resulting state identically every run.
- `check`: all 5,796 vertex animations (280,617 keyframes across 709 models),
  all 6,658 bitmaps, 1,878 models (203,385 triangles), 110 rooms
  (1,009,659 triangles), 226 lightmap sets (181,089 lightmaps) and 13,635 text
  files parse. All 7,656 MP3 sounds decode to exactly the sample count recorded
  in their `fact` chunks (10.4 hours). 225 of 226 lightmap sets have exactly
  one lightmap per surface of their room; `PL6_A_A` has 9 spare, a quirk of
  the retail data.

## Game data

You need a legally owned copy of Gabriel Knight 3 (GOG and Steam sell it).
Nothing from the game belongs in this repository, including files extracted
from it.

## Licence

GPL-3.0-or-later. See [LICENSE](LICENSE) and [NOTICE](NOTICE).
