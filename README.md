# gk3-rs

A Gabriel Knight 3 engine in Rust that ships as a single executable: no
runtime to install, no bundled shared libraries. It plays the game from your
own copy's `Data` directory. See [PLAN.md](PLAN.md) for the design and roadmap.

**Status:** the whole story is in, from the "Day 1, 10am" title card to the
temple, the fight with the demon and the closing movies. Every timeblock of
all three days has been swept without a window, running every action in
every room, and the final fight plays through to the end; nobody has yet
played the whole game through in one sitting. Still to do: the build for
older Linux systems (phase 0) and the polish of phase 11 (resolutions, a
controller, Windows and macOS builds).

Every asset decodes, and every room can be flown through in a window,
textured, lightmapped for its time of day and with its skybox. The game's
224 compiled Sheep scripts run on a virtual machine against a model of the
story's state, and rooms can be set up as the story has them at any time of
day.

Click the floor to walk, click anything to get the original verb icons, and
every action plays as the game wrote it, with the right camera cuts,
animations, captions and recorded voice, lip-synced faces, and the room's
ambient soundtrack. Conversations work: TALK opens a topic bar, each topic
is raised once, and the "last word" topics come up only when the rest have
been said; they cut to their own camera angles, and the people in them
switch between talking and listening as each line's speaker changes. The
inventory shows Gabriel's or Grace's items with the original pictures and
close-ups, and an item can be picked up and used on things. Leaving a room
loads the next one, with Gabriel or Grace as the player as the story has
it. The story moves through its timeblocks: when one is done, its title
card comes up and the next opens. The Bink and AVI movies play, and the
game can be saved and loaded.

The driving map works: ride to any place the story has opened, watch for
suspects' blips on the roads, and follow them to where they stop, which puts
that place on the map. The binoculars pan across the skyline from the
lookouts and zoom into far places, where their scenes play out; the
fingerprint kit dusts objects, lifts prints with tape, and keeps them on
cards. SIDNEY, Grace's computer, is in: Add Data scans items into files;
Search finds and shows the game's 400-odd web pages; E-mail, Translate,
Make ID and Suspects (with fingerprint match analysis) work; Analyze reads
the parchments and paintings, with their geometry movies, and holds the
map puzzle of Le Serpent Rouge and the anagram. The temple's puzzles work:
the chessboard of swords, the pendulum, the bridge of stones, and the
fight with the demon, which ends the game.

## Playing

```sh
gk3 play                     # day 1, 10am, Gabriel's room
gk3 play LBY --time 207A     # anywhere, any time
scripts/gk3-jump             # pick a time and a place from a list
```

`gk3-jump` is a small launcher window (kdialog or zenity): it lists every
timeblock and the places the story has then, by the game's own names
(`gk3 scenes` prints the same list), and opens the game at the one picked.
Close the game to pick another.

Left-click the floor to walk; left-click a thing to see what can be done to it
and click a verb icon; right-click or Esc closes the menu; while someone is
speaking, a click or Space moves on to the next line. I, Tab or the button at
the bottom left opens the inventory; click an item for INSPECT (close-up),
its verbs, and PICKUP, which puts it in hand so it is offered as a verb on
whatever you click next. Esc with nothing open quits.

The camera moves as in the original: W/S or Up/Down go forward and back, A/D
or Left/Right turn, Shift makes them pitch, Ctrl makes them rise, sink and
strafe, Alt doubles the speed, and dragging with the left button does the
same with the mouse. Space levels the camera; 1 to 9 jump to the room's set
camera angles. The camera only moves while nothing is playing, and stays
inside the room's camera bounds, sliding along walls as the original does.
On a graphics card the picture is drawn with 4x multisampling, which smooths
the edges of walls and people; on a software renderer it is not, to keep it
fast.

F5 saves to the quick slot and F9 loads it. Saves are small text files in
`~/.local/share/gk3-rs/saves` (or `$XDG_DATA_HOME/gk3-rs/saves`); `gk3 play
--load quick` starts from one. A save holds the whole story state and where
you stood; loading rebuilds the room and enters it again, as the original's
first save format did, so the visit counts once more but first-visit scenes
do not replay. Saving waits until nothing is playing. Sound comes from PulseAudio or PipeWire if they are there; without
them the game is silent rather than refusing to start.

For testing, the same command runs without a window:

```sh
gk3 play R25 --list                        # everything that can be done now
gk3 play R25 --do WINDOW:OPEN --png out.png # do it, report, take a picture
gk3 play R25 --do HALL_DOOR:R25_ROOM_KEY    # use an item on something
gk3 play R25 --inventory --png inv.png      # the items and their verbs
gk3 play R25 --closeup TALISMAN --png t.png # one item's close-up
gk3 play R25 --do CHICKEN:PICKUP --save x.sav  # play, then save
gk3 play --load x.sav --list                # carry on from a save
gk3 play --load x.sav --goto R25            # walk somewhere; may end the timeblock
gk3 play --time 207A --card --png card.png  # a timeblock's title card
gk3 play --load x.sav --do WILKES:FOLLOW --map LER   # follow, then ride
gk3 play --load x.sav --do VIEW_OF_LHOMME_MORE:BINOCULARS --zoom LHM --png z.png
gk3 play --load x.sav --do HAND_MIRROR:FINGERPRINT_KIT --lift   # dust and keep
gk3 play --load x.sav --sid open --sid "search=holy grail" --png s.png  # SIDNEY
gk3 play TE6 --time 309P --do FLOOR:TE6_FLOOR --do WAIT:3.5 \
  --do FLOOR:TE6_FLOOR --do WAIT:3.5 --do SARCOPHAGUS:CLIMB --do WAIT:4 \
  --do ASMODEUS:TALISMAN --do WAIT:1 --do ASMODEUS:DAGGER --do WAIT:240  # the end
gk3 play TE6 --time 309P --do WAIT:2 --where   # where everyone stands
gk3 movie Parch1Geo.avi --png last.png       # an AVI from the Data folder
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

## Differences from the original

Where gk3-rs plays differently from the original on purpose (smoother walk
endings, multisampling, door sounds and so on), and where it still differs
for want of knowing better, is listed in
[docs/deviations.md](docs/deviations.md). Notes from studying the original
executable are in `docs/re-*.md`.

## Game data

You need a legally owned copy of Gabriel Knight 3 (GOG and Steam sell it).
Nothing from the game belongs in this repository, including files extracted
from it.

## Licence

GPL-3.0-or-later. See [LICENSE](LICENSE) and [NOTICE](NOTICE).
