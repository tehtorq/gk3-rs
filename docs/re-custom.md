# Rooms of their own kind in the original

Notes from `GK3.exe` on a scene file's `custom=`.

## Reading it (0x50fcbc)

`custom=` in a `[GENERAL]` section sets the scene's type (+0x134 of the
scene data); each section read sets it again, so the last one given holds.
An unknown name sets 0.

| `custom=` | Type | Rooms |
|---|---|---|
| `Bridge` | 1 | TE5 |
| `Chess` | 2 | TE1 |
| `Circle` | 3 | TE3 |
| `CoordinateDevice` | 4 | LER at 307A, MCF and BEC at 312P |
| `Laser` | 7 | CS2 |
| `Angels` | 8 | CHU |
| `Holy` | 9 | TE6 |
| `Langolier` | 10 | BET, CS8 at 202P |

The names table (0x6ee490) also has `Demon` (5) and `Hex` (6), which no
scene file names, and the reader does not take.

## What it makes (0x51df4c)

Entering a room makes its scene object by type: each of the types above
has a class of its own, overriding some of the plain scene's virtual
calls; anything else (0, 5, 6) is a plain scene. So the puzzle code is
there only where, and when, the scene files say: CoordinateDevice's GPS
only at LER at 307A and at MCF and BEC at 312P.

Ours keys TE1, TE3, TE5, TE6, CHU and CS2's code by room, which comes to
the same, as each of those rooms' base scene file has the line. The other
two are read from the scene file.

## `Langolier`

Overrides one call (0x4c22bd), made for a click (0x500642 → slot
0x110): if what was clicked is the scene's `floor=` (+0x30 of the scene
data), the click is taken: `wait CallSheep("glb_all", "FloorClick$")`
(0x406676 → 0x406771), and the player does not walk. `FloorClick$` has the
player say `1LK3Y2O4G1`: "That'd be kinda rude. I'll wait til we're done
talkin'."

## `CoordinateDevice`

Grace's GPS: see `re-gps.md`.
