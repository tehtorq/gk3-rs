# The church's angels in the original

Notes from `GK3.exe` on the `Angels` scene (`custom=Angels`, CHU; class at
0x41e73e, vtable 0x6663f4; saved fields named at 0x41e8b5:
`mExternalNotify`, `pTempTimer`, `angelLineFlags`, `lastAngelNum`,
`spokeMessLine`, `mVeryFirstTime`, `mHasBeenCompleted`).

The scripts touch an angel with `wait CallSceneFunction("Angel1")` to
`("Angel4")` (then `CallSheep("chu205p", "Done")`), and wipe the square with
`wait CallSceneFunction("Erase")`; the function's wait ends when the scene
says so.

## Entering (0x41e97a)

Nothing touched, no lines, `mVeryFirstTime` set, not completed.

## A touch (0x41ea31)

Any name starting `Angel`, its sixth letter the angel (1 to 4); any
starting `Erase`. Nothing is done once the square is up (`angelLineFlags`
0x158) or `FOUR_ANGELS:TRACE` has a count: the script goes straight on.

- **The first touch** (`lastAngelNum` 0): `FOUR_ANGELS:ERASE` is set to 1
  and that angel's dot (`chu_laserdot0N`) shown. The very first time, if
  Grace touches, she says `18P1F0M021` and the script waits for it.
- **After**: the last angel's dot is hidden and this one's shown (only one
  dot is ever lit). The same angel again draws nothing. Else the line
  between them is shown and its bit set (0x41eb71): bit (a + b + 1), but
  for angels 2 and 3, bit 3 (their sum is 1 and 4's), the models being

  | Angels | 1–4 | 1–2 | 2–3 | 3–4 | 1–3 | 2–4 |
  |---|---|---|---|---|---|---|
  | `chu_laser0…` | 1 | 2 | 3 | 4 | 5 | 6 |

  Then (0x41eda3):
  - **The square** (the four sides, no diagonals, 0x158): a timer of five
    seconds starts; the first time, the line `18L9M0MZ81` (Gabriel) or
    `18P9M0MCE1` (Grace) is said, not waited for, and
    `FOUR_ANGELS:TRACE` counted once more. When the timer runs out
    (0x41e9fe) the square is wiped, and the script goes on.
  - **All six lines** (0x1f8): `18L9M0MZ82` or `18P9M0MTB1`, waited for,
    unless `spokeMessLine`, which nothing sets.
  - **Otherwise** this angel becomes the last.
- **Erase** (0x41ef93): every dot and line hidden, `FOUR_ANGELS:ERASE` set
  to 0, no lines, no last angel.
