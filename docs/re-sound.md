# Sound placed in the room, in the original

Notes from `GK3.exe` on its 3D sound, and from Wine on how DirectSound
then hears it.

## The listener

Each time the camera moves (0x44b55a), the sound system's listener is
set to the camera's position, heading and pitch (0x56da10 → 0x5aefa3,
`listenerPos`, `listenerHeading`, `listenerHeadingVertAngle`): front is
the camera's forward, top its up. The sound system starts (0x5adae8) with
a rolloff of 3, handed to DirectSound's listener (0x5aef54), and a distance
factor of 0.0254 (0x5aee30): a unit is an inch.

## The sounds

A 3D sound is a DirectSound buffer made with `DSBCAPS_CTRL3D`,
`CTRLVOLUME`, `CTRLFREQUENCY`, `GETCURRENTPOSITION2` and
`MUTE3DATMAXDISTANCE` (flags 0x300b0, 0x5b1985; made again without the
last if that fails), its minimum and maximum distances each held between
1 and 10^9 (0x5b1828). Three kinds are placed:

- **Footsteps** (0x40a23d): at the walker, minimum 60 and maximum 840 for
  everyone (0x6ec874, 0x6ec878).
- **An animation's sounds** (`[SOUNDS]`, read at 0x54c000): after the
  frame, name and volume, a model's name (a field with a letter but for an
  exponent's `e`) or a position (written x, z, y), then the minimum and
  maximum, each 10^9 if missing or unreadable. Played (0x54c2b0) from the
  middle of the model's bounds as it then stands, plus the position; not
  moved after. Most of the game's animation sounds are placed so.
- **A soundtrack's** (`.STK`, read at 0x51b52b): with `3D=1`, at `X`, `Y`,
  `Z` (the origin if not given), or at the middle of the model `Follow`
  names, found by name as written (0x50326d), and kept there as it moves
  (0x51b333, every tick from 0x51c6c1); `MinDist` 60 if not given,
  `MaxDist` a unit more than `MinDist` if not given, and three times it if
  given less.

## How DirectSound hears them

The game asks DirectSound for 3D sound and leaves the rest to it. Wine's
DirectSound (`dlls/dsound/sound3d.c`, `DSOUND_Calc3DBuffer`; speakers in
`dsound.c`, `DSOUND_ParseSpeakerConfig`), which the original runs on here:

- beyond the maximum distance, a buffer made `MUTE3DATMAXDISTANCE` is
  silent;
- else the distance, at least the minimum, is taken as `min + (d − min) ×
  rolloff`, and the volume falls by `1000 log10((d / min)²)` millibels;
  the gain is `2^(millibels / 600)`;
- the listener's left is front × top; the angle from it to the sound is
  negated if the sound is behind (more than a right angle from front), and
  a right angle taken off; stereo speakers stand at −90° (left) and +90°
  (right), and the sound is shared between the two it lies between by
  `√(1 − a)` and `√a`, `a` how far between (round the back, from right to
  left).

So a sound ahead is heard at 0.71 in each ear, one to the side in that ear
alone.

## Ours

The same: footsteps, animation sounds and soundtrack sounds are placed as
above, heard from the camera by Wine's formulas, and heard again each
frame as the camera moves (and as a followed model moves). Sounds not
placed (voices, music, the interface) play as before.
