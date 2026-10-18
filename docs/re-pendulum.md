# TE3's pendulum in the original

Notes from `GK3.exe` on the `Circle` scene (`custom=Circle`; class at
0x51e420, vtable 0x673fcc; saved fields named at 0x45799c). Ours,
`gk3/src/pendulum.rs`, follows these.

## Its settings (0x459d83)

`PENDULUM.TXT`: `circlePeriod` 50 s, `pendulumCycle` 6, `pendulumAngle`
24.5°, `updateTime` 20 ms, `startSafe` 15°, `endSafe` 3°, `landDist` 3°.
From them:

- the platforms turn 2π / 50 000 a ms (`mFloorDelta`);
- the blade's cycle is 6 × 50 000 / 24 = 12 500 ms (`mPendMult` 2π over
  it): six platforms' time, so the platforms go round once in exactly four
  swings;
- the swing (`mPendSwing`) is 24.5° in radians. `mPendAngle` starts as that
  in radians again (0.0075), and the blade's model takes it on the first
  step;
- the pivot is 2009.51 up (`mPendHeight`);
- the danger zones are phases of the cycle: the blade coming up to the high
  slot (325°–355°), going down from it (355°–25°), coming up to the low slot
  (145°–175°) and going down from it (175°–205°). They are 30° wide
  (`highUSstart` … `lowDSend`).

Setting up puts all 24 platforms (`te3_r01`…`te3_r24`) and the blade back
to identity, shows `te3_pendulum_center_code` and hides
`te3__pendulum_gabe` and `te3_pendulum_center`. It sets state 0,
`gabeBlock`/`tgtBlock` to -2, `mHigh` and `mCanFall` on, and the
predicted turn to a second's worth. It poses `te3_doorclose` at its start
(started and stopped at once). The scene's enter (0x457e4a) does this and
clears `Te3GabeAtAltar`. Its function `restart` (0x45846c) does it again,
unless Gabriel is on the altar (state 3).

## The update (0x457eed)

The update runs fixed steps of `updateTime`, the last one short so as to
end at the present. The first update starts the cycle (`startCycle`). Each
step:

1. turns the platforms (and `mFloorRot`, `mPredRot`, wrapped at 2π);
2. in states 1, 8 and 9 (on a platform, landing on, jumping), turns
   Gabriel round the middle with them. A jump's 800 ms add `gabeAngV` a ms
   (±15° over the jump), eased ×0.32 for the first 66 ms, ×0.8 to 199 and
   ×1.12 after, blended over a step that spans a change. When the jump's
   time is up, `gabeBlock` becomes `tgtBlock`;
3. swings the blade (0x459abc): the phase is (t − `startCycle`) ×
   `mPendMult`, folded into ±2π, and the angle is cos(phase) ×
   `mPendSwing`. The model is turned by the change about the pivot, except
   in state 5, when it is left where it struck. On a platform (state 1):
   - the zone the phase is in and its side give the platform at the slot,
     29 − n (high) or 41 − n (low), mod 24, where n = ⌊`mFloorRot` × 24 /
     2π⌋;
   - going down, the platform before it is the one at risk;
   - on entering a zone, if Gabriel is on that platform (and not mid-jump),
     he is struck (0x458f26).

   The same is worked out a second ahead (phase + 1 s, `mPredRot`) for
   `mPredTarget`, which the clicks go by.

Standing still is therefore safe on a third of the platforms. Platform 23
is the door's at the start, and the struck ones come up as entries at
fixed times in the 50 s round. The others are never in a slot at a zone's
entry.

## Clicks (0x45848e) and the cursor (0x45850e)

By state. Clicks are handled in states 0–2; a click a handler does not
take is left to the plain scene.

- **0, in the doorway** (0x45906d): any platform (`te3_rNN`, seven letters):
  state 7 and `GabTE3JumpFS`, then message 6.
- **1, on a platform** (0x4593d6):
  - **The blade.** A piece of it (`te3_pend…`) is grabbed if a second on it
    comes up to the slot past him, or goes down from his. This plays
    `GabJmpPndulm` and scores `e_temple_grab_pendulum`.
  - **The platform behind him.** If the blade will next go down from the
    slot there, every click does nothing.
  - **A neighbouring platform,** with no jump under way:
    - if the blade is not coming for his platform within the second, he
      jumps left (`GabTE3JumpLS`, +15°, message 9) or right (`RS`, −15°,
      message 8), in state 9;
    - else it is too late: `KILL_LOW` / `KILL_HIGH` (by the blade's side)
      and `mGabeWillHit` (or `mGabeWillGrab`), which the end of a later
      jump would act on.
  - **`te3_pendulum_center_code`** is also grabbed going down or out of the
    zones, without the score.
  - **The room's shell** (`te3_cieling`, `te3_innerwalls`,
    `te3_outer_walls`, `te3_wallslots`) is left to the scene.
  - **Anything else:** `1SERM44V51` ("I don't have time for that now!").
- **2, on the blade** (0x459736):
  - **`te3_hpaltar`** is taken. If the blade is 3°–15° out and swinging in
    (`startSafe`, `endSafe`, by `mPendSpd`'s sign), he cuts to
    `LONG_ALTAR`, enters state 10 and plays `GabJmpOffPen`, then message 13.
  - **The shell** is left to the scene.
  - **Anything else:** the same line.

The cursor is `c_zoom` over: any platform in the doorway; on a platform,
the blade when it is grabbable (without the zone check) and either
neighbour; on the blade, the altar in the window. The window is judged on
the angle in degrees plus `mPendSpd` in radians, as the original adds them.

## Messages (0x458c48)

| Code | Sent by | Does |
|---|---|---|
| 6 | `GabTE3JumpFS` ending | He lands on platform 23 − n, 775 out, facing the middle. `GabTE3JumpFE` (then 7) and `te3_doorclose` play, forced cuts are cleared, state 8, `JUMP_OUT` |
| 7 | a landing ending | state 1 |
| 8, 9 | `GabTE3JumpRS`, `LS` ending | `RM` or `LM` plays (then 10 or 11), with 800 ms of turn |
| 10, 11 | `RM`, `LM` ending | `RE` or `LE` plays (then 7) |
| 13 | `GabJmpOffPen` ending | forced cuts cleared, then the landing (below) |
| 0 | a death's script ending | the blade shown again, `te3_pendulum_center` hidden, forced cuts cleared, `AFTERKILL_LOW`/`HIGH`; 2 s on, 1 |
| 1 | that timer, or `GabeFallDeath` ending | `CallSheep("te3", "die$")` |
| 2 | 2 s on the blade | forced cuts, `LONG_ALTAR` |

**Struck** (0x45858e):
- cuts to `KILL_LOW` or `KILL_HIGH` and sets state 5 with forced cuts;
- the blade's model is hidden and `te3_pendulum_center` shown;
- the script runs `StartDialogue("1regj67q81",1)` and `wait
  StartAnimation(...)`, then message 0. The animation is `GabTE3PnFt…` if
  the blade came up to him, `…Bk…` if it was going down, and `…L` or `…H`
  by side.

**Grabbed** (0x45871d):
- the kill camera by side, state 2, `gabeBlock` -3;
- the swing loses 2°;
- the plain blade, `gab` and `gabmic` are hidden and `te3__pendulum_gabe`
  is shown, given the blade's matrix. It is turned half round about the
  pivot's upright if it faces the wrong side (`mHigh`);
- `Te3FirstPendGrab` and its score;
- message 2 in 2 s.

**Landing** (0x45898f):
- the plain blade takes back the matrix, and Gabriel is shown under its end
  at `gabe_fall_reference`'s height, facing ±1.3 by the swing;
- more than `landDist` out: `FALL_DOWN`, state 4, `GabeFallDeath`, then
  message 1;
- otherwise, at the altar:
  - he stands at `ALTAR_STAND` and the camera cuts to `ALTAR_UP`;
  - the blade stops (`pendOn` off, swing 0) and the state is 3;
  - `Te3FirstPendDrop` is set and scores `e_temple_drop_platform`;
  - `Te3GabeAtAltar` is set and `CallSheep("te3", "GabeOnPillar$")` runs.

## After a death

`die$` stops the music, shows the death screen and calls `Restart$`. The
death screen, coming up (0x4d5b85), plays `TempleDeathTag.STK` and calls
the current scene's `restart`. It also calls the location's `Restart$`,
but only when no script is running (0x4077b4), which after `die$` there
is. Retrying calls `PostDeath$` (0x4d5cd5).

## Ours

The above, with our animation lengths standing in for the original's
messages. The platform Gabriel lands on is placed from the platforms'
layout (each 15° round, the first 7.5° from +Z) rather than its model's
centre; the two differ by the model's height, which the original's
normalising lets in. In the steps that carry Gabriel round (states 1, 6,
8, 9) the original reads the centre's, `gabmic`'s and `groundblood`'s
transforms, turns the first two, and writes `gabmic`'s back to both
`gabmic` and `groundblood` (0x457eed): the blood goes where Gabriel's
model goes. Ours gives `groundblood` `gabmic`'s transform after each such
update.
