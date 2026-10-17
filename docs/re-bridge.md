# TE5's bridge in the original

Notes from `GK3.exe`. The room's scene file says `custom=Bridge`; the
puzzle is a scene object of its own whose fields the save file names
(0x4454d8): `bGabeAnimating`, `bBlockAnimating`, `bGlimmerOn`,
`bGlimmerStart`, `bApproachedPuzzle`, `startTime`, `beginTime`, `checkTime`,
`timeBetweenTiles`, `oldCurTile`, `stillGlowing`, `curBlockPos`.

## The stones

Setting up (0x4463d0), each of `TE5SQ01` to `TE5SQ09` is placed from a
table of nine positions (0x6ec920) and hidden:

| Stone | x | z |
|---|---|---|
| 1 | -80 | -180 |
| 2 | -35 | -135 |
| 3 | -125 | -90 |
| 4 | -125 | 0 |
| 5 | -80 | 45 |
| 6 | -35 | 135 |
| 7 | -125 | 180 |
| 8 | -125 | 225 |
| 9 | -80 | 270 |

On a grid of 45-unit squares, three across, that is (across, along):
(1,0) (2,1) (0,2) (0,4) (1,5) (2,7) (0,8) (0,9) (1,10); the near side is
(1,-1) and the far side (1,11).

## Each frame (0x445688)

**Before the first stone** (`curBlockPos` -1, glimmer off): at most once a
second (`checkTime`), Gabriel is looked for at the position `STARTPOINT`
(-80, -225 in TE5.SIF): if his position's whole units are less than 30 from
it, across plus along, then at most every four seconds (`beginTime`) stone
1 is shown and plays `te5glint01`, and the first time, `PreFirstJump` is
called in `te5` (`bApproachedPuzzle`).

**On the stones** (glimmer on): when a new round is due (`bGlimmerStart`),
the nine stones are shuffled with `rand()` into a new order and the round's
start time kept (`startTime`). Each 425 ms (0x1a9) of the round, the next
stone in the order, unless it is `curBlockPos`, is shown and plays
`te5glint%02d`. After the ninth (3.825 s), a new round is due.

## A click (0x445e86)

Ignored, but taken, while `bGabeAnimating`. Otherwise by the model clicked:

- **`TE5SQnn`**: a move from `curBlockPos` to that stone, by the table
  below. From the near side only stone 1 counts (anything else is taken
  and ignored); from there the camera cuts to `TILES`, Gabriel is put on
  `STARTPOINT` at its heading (0x41d603), and the glimmer starts. A stone
  the table does not join to where he is, his own included, is a fall.
- Anything else, while he is not on a stone, is not the bridge's.
- **`te5_hittest_floor`** (the chasm): a fall.
- **`te5hittestend`** or **`CS5_FAR_FLOOR_BAD_`** (the far side): from
  stones 8 or 9, across (`curBlockPos` 9, glimmer off); from any other, a
  fall.
- **`te5_floor`** (the near side): from stones 1 or 2, back (`curBlockPos`
  -1, glimmer off); from any other, nothing.

`curBlockPos` becomes where the move ends as the move starts, and
`bGabeAnimating` is set.

### The moves

A move is a number sent through the scene's message queue. Its tens, less
100, name the turn into the jump; its units the jump:

| Code | Turn | Jump | From → to (stones 1-9; N near, F far) |
|---|---|---|---|
| 1101 | none | `GabTe5Jump01sq` | N→1, 7→8, 9→F |
| 1104 | none | `GabTe5Jump02sq` | 3→4 |
| 1113 | `GabTe1TurnC26` | `GabTe5Jump26Knight` | 5→6, 7→9, 8→F |
| 1122 | `GabTe1TurnC45` | `GabTe5Jump45` | 1→2, 4→5, 8→9 |
| 1153 | `GabTe1TurnC116` | `GabTe5Jump26Knight` | 3→2, 7→6 |
| 1181 | `GabTe1TurnC180` | `GabTe5Jump01sq` | 8→7, 1→N |
| 1184 | `GabTe1TurnC180` | `GabTe5Jump02sq` | 4→3 |
| 1213 | `GabTe1TurnCC26` | `GabTe5Jump26Knight` | 1→3 |
| 1233 | `GabTe1TurnCC63` | `GabTe5Jump26Knight` | 2→3, 6→7 |
| 1262 | `GabTe1TurnCC135` | `GabTe5Jump45` | 2→1, 5→4, 9→8 |
| 1273 | `GabTe1TurnCC153` | `GabTe5Jump26Knight` | 6→5, 9→7, 2→N |

The turn's name (0x445d46): `GabTe1TurnC`, then `C` for tens in the
twenties (the turn back the other way: `C` for the teens), then the angle
for the tens' units 1 to 8: 26, 45, 63, 90, 116, 135, 153, 180. Tens of 10
are no turn.

## The steps (0x44594d)

Each animation is started with the next message, sent when it ends:

| Message | Does |
|---|---|
| 1000s | the turn into the jump (none: straight on), then +1000 |
| 2000s | the jump, then +1000 |
| 3000s | `GabTe5Stand`, then +1000. Across (`curBlockPos` 9): sets flag 0xb9, `CrossedBridge` (the flag names' table at 0x6ee8f0 is one ahead of the numbers), and calls `GabeCrossedBridge$`. On a stone: shows it and plays `te5glow%02d`, sending 20 + stone at its end |
| 4000s | the turn back, then 1 |
| 1 | the move is over: `bGabeAnimating` off, and a stone whose glow ended meanwhile (`stillGlowing`) hidden |
| 10 + stone | a glint ended: the stone hidden unless it is `curBlockPos` |
| 20 + stone | a glow ended: the stone hidden (while a move plays, once it is over); and if not moving and still on it, he falls: `curBlockPos` -1, `FallDie` called, `bGabeAnimating` set |
| 0x1f | a fall: `curBlockPos` -1, `BishopFallDie` called |

Gabriel is never placed on the stones: the jumps' relative animations carry
him there. Nor is the camera cut on reaching the far side: the scripts do.

## Restart

The scene object takes two commands from outside its room's scripts:
`restart` (0x44635f: an animation ending in message 1, so `bGabeAnimating`
off, and the glimmer off) and `BridgeStart`. `restart` is sent by the death
screen as it comes up (0x4d5b85, at 0x4d5c49), to whatever scene is current;
TE5's scripts after a fall show the death screen, then `Restart$` puts
everyone back. Ours sends it too, and also counts the bridge recovered once
the scripts after a fall are done.
