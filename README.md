# gk3-rs

A Gabriel Knight 3 engine in Rust that ships as a single executable: no
runtime to install, no bundled shared libraries. It plays the game from your
own copy's `Data` directory. See [PLAN.md](PLAN.md) for the design and roadmap.

**Status:** phase 3 of 11 done. Every asset decodes, and every room can be
flown through in a window, textured, lightmapped for its time of day and with
its skybox. There are no characters or gameplay yet.

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
```

In the viewer: WASD or the arrow keys move, Q and E go down and up, Shift is
faster, and dragging with a mouse button looks around. Tab and Shift+Tab step
through all 229 rooms, C cycles a room's own cameras, M switches between lit,
textures only and lightmaps only, and Esc quits.

Against the retail English release:

- `verify`: 36,957 assets and 20,991 pointer entries across 8 archives,
  no failures.
- `check`: all 6,658 bitmaps, 1,878 models (203,385 triangles), 110 rooms
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
