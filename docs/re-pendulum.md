# TE3's pendulum in the original

Notes from `GK3.exe` on the `Circle` scene (`custom=Circle`; class at
0x51e420, vtable 0x673fcc; saved fields named at 0x45799c). Ours follows
G-Engine's `Pendulum` for the motion and timing, which are not yet traced
here; what follows is what is.

## Its settings

`PENDULUM.TXT`: `circlePeriod` 50 s, `pendulumCycle` 6 platforms,
`pendulumAngle` 24.5°, `updateTime` 20 ms, `startSafe` 15° and `endSafe`
3° (the drop's window), `landDist` 3.

## Clicks (0x45848e)

The scene's click (slot 0x110) goes by its state (+0xdc) to one of four
handlers, each given the name of what was clicked; one that does not take
it leaves it to the plain scene. States past 3 (jumps, the drop) take
none.

- **0, in the doorway** (0x45906d): a platform (`te3_rNN`) starts the
  jump on; nothing else is taken.
- **1, on a platform** (0x4593d6): a piece of the blade (any name
  starting `te3_pend`), or `te3_pendulum_center_code`, at the moment it
  can be caught, is grabbed; a platform either side is jumped to (or, as
  the blade comes, is death: `KILL_LOW`, `KILL_HIGH`); the ceiling and
  walls (`te3_cieling`, `te3_innerwalls`, `te3_outer_walls`,
  `te3_wallslots`) are left to the scene; anything else, Gabriel says
  `1SERM44V51` ("I don't have time for that now!").
- **2, on the blade** (0x459736): the altar is taken, and dropped to in
  the window; the ceiling and walls are left to the scene; anything else,
  the same line.
- **3**: nothing.

Hovering (slot 0x114, 0x45989d and on) answers the same pieces with the
highlight cursor.

## Ours

The clicks above, as far as G-Engine's states match them. During a jump
ours takes clicks and does nothing with them, where the original leaves
them to the scene (whose floor here takes none).
