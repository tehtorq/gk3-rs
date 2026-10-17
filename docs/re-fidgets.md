# Fidget scripts' cleanups in the original

Notes from `GK3.exe` on the animation scripts (`.GAS`) that keep actors
idling, talking and listening.

## Reading them (0x42c09f)

`USE CLEANUP anim, cleanup` and `USETALK CLEANUP anim, cleanup` each add
to a list of their own on the script, keyed by the first animation: what to
play to tidy up if the script is stopped while that animation is the last
it played. (`USETALK` lines also set a flag at +0x161,
`mbAltCleanupExists`.)

## The script's fields (save code at 0x42bb80)

| Field | Offset |
|---|---|
| `mhCurAnim` | 0xa8 |
| `mCleanupState` | 0xc0 |
| `mstCleanup`, `mstAltCleanup` | 0xc8, 0x114 (each list at +0x3c) |
| `mbPuppetCleanup`, `mbAltCleanupExists` | 0x160, 0x161 |
| `mCleanupNotify` | 0x164 |
| `mCleanUpScr`, `mbDoCleanUp` | 0x170, 0x174 |
| `mhPrevAnim`, `mhNextAnim` | 0x178, 0x17c |

## Playing them (0x42f471)

Which list is used is chosen by the caller (talking or not).

- **Stopping** (step 0): if the script last played an animation
  (`mhPrevAnim`) and it has finished, the list gives its cleanup; that
  becomes the animation to play (`mhCurAnim`) and `mCleanupState` 1.
- **Each step after** (step 1): the animation about to play is looked up in
  the same list (`mCleanupState` 1: the cleanup just chosen; 2: the one
  kept for next). If it has a cleanup in turn, that is kept as the next
  (`mhNextAnim`), `mCleanupState` becomes 2, and the one looked up plays;
  if not, the one kept (if any) plays and nothing is kept.

So cleanups chain: each cleanup's own cleanup plays after it, until one has
none. Bigout's in MOP, stopped as he wipes the tube, are `BigMopWipeB` (the
rag away), `BigMopAwayTubeA` (the tube down) and `BigMopAwayTubeB`;
18 scripts in the data chain so. G-Engine plays only the first, which left
Bigout talking with the tube standing in the air where his hand had been.

A list that leads round in a circle would play for ever; none in the data
does, and ours follows one once round.
