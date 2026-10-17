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

Ours keys TE1, TE3, TE5, CHU and CS2's code by room, which comes to the
same, as each of those rooms' base scene file has the line. The other three
(`CoordinateDevice`, `Holy`, `Langolier`) are read from the scene file.

## `Holy` (class 0x51e4ad, vtable 0x67b4d4)

Overrides:

- **The click (slot 0x110, 0x4b62b4).**
  - `te6_floor` sets `Te6ClickedOnFloor` (flag 0x26) and is taken; the
    scripts answer it with Gabriel backing away.
  - `dem` (Asmodeus), `te6_tomblid` and `te6_sarc_walls` are left to the
    scene.
  - Anything else is taken, with `StartDialogue("1SERM44V51",1)`: "I don't
    have time for that now!"
- **The cursor (slot 0x114).** None.
- **The scene function (slot 0xe8).** Only the plain scene's; TE6's scripts
  call none.
- **The camera's set-up on entering (slot 0xf8, 0x4b623f).** It keeps only
  one of the plain scene's settings (+0x24 of the scene data). It does not
  cut to the room's first camera, does not set the camera's bounds option
  (+0x1b0), and does not clear forced cuts (0x50094d does all three).
- **Entering (slot 0x104, 0x4b625d).** Sets Asmodeus's walker's
  `mFloorClamp` (+0x2e4) to 5. In the walker's height update (around
  0x418000), mode 5 keeps ASMODEUS (noun 0x4f) off the floor while walking:
  his height comes from his walk's own heights (+0x144), or a set height
  (+0xf0), above the floor. The pendulum sets Gabriel's to 0 (none).

Ours takes the clicks as above. On entering, it keeps forced cuts but
still starts on the room's first camera, which TE6's scripts cut away from
at once. Asmodeus's height while walking is our walker's, not mode 5's
(see `deviations.md`).

## `Langolier`

Overrides one call (0x4c22bd), made for a click (0x500642 → slot
0x110): if what was clicked is the scene's `floor=` (+0x30 of the scene
data), the click is taken: `wait CallSheep("glb_all", "FloorClick$")`
(0x406676 → 0x406771), and the player does not walk. `FloorClick$` has the
player say `1LK3Y2O4G1`: "That'd be kinda rude. I'll wait til we're done
talkin'."

## `CoordinateDevice`

Grace's GPS: see `re-gps.md`.
