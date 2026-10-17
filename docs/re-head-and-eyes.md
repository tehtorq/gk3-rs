# Head turns and glances in the original GK3.exe

Notes from studying the original executable (GOG `GK3.exe`, image base
0x400000) to learn how `TurnHead`, `Glance` and the `Lookit*` family behave.
Behaviour only: nothing here is copied code.

## Script entry points

The Sheep API table in `.data` holds, per function, its name, signature and
code address. The head and eye functions are thin stubs that forward to one
manager object:

| Function | Stub | Implementation |
|---|---|---|
| `TurnHead(actor, percentX, percentY, durationMs)` | 0x50adc8 | 0x4a9b94 → actor head component 0x4b4c97 |
| `Glance(actor, percentX, percentY, durationMs)` | 0x50a7ce | 0x4a8a2b → face/eye controller 0x40ca12 → 0x40ca3d |
| `LookitActor(actor, other, componentsSpec, durationSec)` | 0x50a8d0 | 0x4a8ce9 (quick = 0) |
| `LookitActorQuick` | 0x50a8fa | same, quick = 1 |
| `LookitCancel(actor)` | 0x50a924 | 0x4a8dcd |

Both `TurnHead` and `Glance` refuse percentages outside -100..100 and
durations of zero or less.

## Head turns

- The character's `HeadFOV` (left, top, right, bottom, in degrees) is read from
  `CHARACTERS.TXT` and stored in radians (0x454f07, conversion 0x554ba0).
  `HeadComfortableFOV` defaults to 0.75 × `HeadFOV` when absent.
- The head component (actor + 0x68) copies the full `HeadFOV` (0x4b4787) and
  loads four animations named `<code>TurnHeadL`, `TurnHeadR`, `TurnHeadUp`
  and `TurnHeadDown` (0x4b4554), measuring their extreme angles. Head poses
  are blends towards those authored poses, not free rotations of the head
  mesh.
- Sixteen characters have these animations in the shipped data: ABE, EM2,
  EML, EST, GAB, GRA, HE1, HE2, JEA, LAR, LH2, MAD, MAR, MOS, VIT, WI2. Each
  is a steady turn from rest to its furthest (60° sideways, 30° up or down
  for the Abbé) over 31 frames. Characters without them keep their heads
  still; their eyes still look (below).
- `TurnHead` turns the percentages into angles: x > 0 uses the right limit,
  x ≤ 0 the left (×−0.01); y > 0 the top, y ≤ 0 the bottom (×−0.01). It
  queues a timed node (type 5) holding both angles and the duration.

## Eyes (glances)

- `FACES.TXT` gives each character painted eyes on the face texture:
  `Left/Right Eye Name` (a bitmap from `[Eyes]`), `Left/Right Eye Offset`
  (pixels), `Left/Right Eye Bias`, `Max Look Distance` (pixels each way),
  `Jitter Frequency` and `Max Jitter Distance` (small random eye movements),
  `Eye Field Of View` and `Eye Short Field Of View` (the latter "used when
  figuring out when to move head").
- `Glance` scales the percentages by `Max Look Distance` (×0.01, negated) to
  get a pixel offset for the eyes, and holds it for the duration.
- `componentsSpec` in `Lookit*` (e.g. `EH`) says what looks: eyes, head.

## Which face a character wears

- `CHARACTERS.TXT` names chain by default (0x454d2f): `ModelName` defaults
  to the section's code, `BaseName` to `ModelName`, `MouthName` to
  `BaseName`.
- Each `FACES.TXT` section is read for the character of the same code
  (0x4560a6); its unnamed pictures (`_FACE`, `_EYELIDS`, `_FOREHEAD`) are
  named after that character's `BaseName` (0x453fdf), so `VR3` wears
  `VA3_EYELIDS`.
- An actor's face is the section under its own code, or else the one under
  its `BaseName` (0x454a98): EM2, GAG, JA2, LH2, MA2, MOV, MO2, MO3, SIM_,
  VI2 and WI2 borrow their main character's face this way.
- Lip-sync mouths are the actor's `MouthName` + `_` + `Mouth00`..`Mouth07`
  (0x40e958, 0x4239bc): VM1 talks with `VA1_MOUTH*`.
- A face picture may be used by several parts of a model: Jean's head has
  two, Madeline's three. All of them show the face.
