# Walking in the original GK3.exe

Notes from studying the original executable (GOG `GK3.exe`, image base
0x400000) with Ghidra, to compare its walker with ours. Behaviour only:
nothing here is copied code. Work in progress.

## Script entry points

`WalkTo` (stub 0x50aebf → 0x4a9d64) looks up the named position
(0x4af9b2), makes the actor's name a string (0x41f074) and calls the walk
method 0x407998 on the position's owner object, with the wait handle.
`WalkToAnimation` (0x50aedf → 0x4a9db1) does the same through 0x407a6b.

## Character settings (`CHARACTERS.TXT`, reader 0x454bc8)

Read into the character's record, besides those we read already:

| Key | Offset | Default |
|---|---|---|
| `HeadMeshIndex` | 0x54 | −1 (none) |
| `HeadComfortableFOV` | 0x68 | 0.75 × `HeadFOV` |
| `WalkerHeight` | 0x78 | 0 |
| `ShoeThickness` | 0x7c | 0 |
| `SoundMinDistance` | 0x80 | (shared default) |
| `SoundMaxDistance` | 0x84 | (shared default) |
| `StopAnim` | string | read like `StartAnim` and `ContAnim` |
| `StartTurnRightFrame` | 0xd8 | 0 |
| `ClothesDefault` | string | "default" |

Not used by us: `SoundMinDistance`, `SoundMaxDistance` (no character
sets them, and footsteps do not use them: see below), `ClothesDefault`.

## A walk request (save code 0x419929)

Each walk carries its own targets and tolerances; G-Engine's fixed figures
(2.0 for the end, 12.25 for corners) stand in for these.

| Field | Offset | Meaning |
|---|---|---|
| `mDestPos` | 0x00 | where to go |
| `mDestPosAccuracy` | 0x0c | how near counts as there |
| `mDestHeading` | 0x10 | facing at the end |
| `mDestHeadingAccuracy` | 0x14 | how near that facing counts |
| `mbStartIdle` | 0x18 | idle fidget again after |
| `mbSkipToView` | 0x1c | skip the route's off-screen start |
| `mbAutoScrWalk` | 0x20 | asked for by a fidget script |
| `mbUserWalk` | 0x24 | the player's click |
| `mDestPosMustBeExact` | 0x28 | end exactly at `mDestPos` |
| `mUsePathEndAsDestHeading` | 0x2c | face along the route's end |
| `mStopType` | 0x30 | how the walk ends (to trace) |
| `mKeepHandsOff` | 0x34 | |
| `mModelName`, `mModelPos`, `mModelHeading` | 0x38, 0x58, 0x64 | walking to a model |
| `mRegion` | 0x68 | walking to a region |

## The walker (save code 0x416d8f)

| Field | Offset |
|---|---|
| `mCurPos`, `mCurHeading` | 0x110, 0x11c |
| `mSavedPos`, `mSavedHeading` | 0x12c, 0x138 |
| `mCurWalkAnimType`, `mCurWalkState` | 0x13c, 0x140 |
| `mCurWalkAnimPtr` | 0x144 |
| `mStartOriginPositions`, `mContOriginPositions` | 0x150, 0x160 |
| `mDefaultTurnSpeed`, `mTurnSpeed` | 0x170, 0x174 |
| `mHeadingAtAnimStart`, `mHeadingAdjustedSinceAnimStart` | 0x178, 0x17c |
| `mCollisionRadius` | 0x180 |
| `mbUseBoundary` | 0x184 |
| `mStateStopRotate` | 0x18c |
| `mStateCollisionPausedTime` | 0x190 |
| `mLastUpdateWalkTime` | 0x1cc |
| `mHavePendingWalkTo` | 0x2e0 |
| `mFloorClamp`, `mCurFloorName` | 0x2e4, 0x2e8 |

Walkers pause when they collide with each other (`mStateCollisionPausedTime`);
ours route round other walkers' spots instead.

## Facing: the heading model (save code 0x419bcc)

An actor's facing comes from a heading model (G-Engine's "facing helper",
`DOR_<model>`), read through two reference points:

| Field | Offset |
|---|---|
| `mModel` | 0x00 |
| `mReferencePointGroupIndex`, `mReferencePointVertexIndex` | 0x04, 0x08 |
| `mHeadingAdjust` | 0x0c |
| `mHeadingModelUsed` | 0x10 |
| `mHeadingModel` | 0x14 |
| `mHeadingModelReferencePoint`, `…Point2` | 0x18, 0x1c |
| `mHeadingModelAdjust` | 0x20 |
| `mCurWalkAnimModelList` | 0x24 |
| `mStartWalkFrame0Pos`, `mStartWalkFrame0Dir` | 0x34, 0x40 |

The walker's heading data sits at +0xb8 (set up at 0x417551, read at
0x41c054 and 0x41c0e8):

- The actor's position is always its hip point (the character's
  `HipAxes…` mesh, group and vertex).
- **Without a heading model in use**, the heading is the direction from the
  hip point to a reference point on the hip mesh, across the floor
  (`atan2(dx, dz)`, 0x40aa92), plus `mHeadingAdjust`.
- **The reference point** is chosen once, as the walk animations are set up
  (0x4172cf → 0x41be02 → 0x41bf92), on a copy of the model (`WalkerCopy`)
  posed in the first frame of the character's `StartAnim`. With the model
  stood at the identity, of the hip mesh's vertices (less the four trailing
  points of a flagged part) at least a unit across the floor from the hip
  point, it is the one whose direction from it (in three dimensions, made
  unit length) lies most nearly along -Z (0x545b80). `mHeadingAdjust` is
  then π less the heading read there (0x41c003): that pose faces -Z.
- The hip point is the hip mesh's first trailing point (0x545ad0 finds the
  part flagged with them and takes its fourth point from the end), which
  is the same vertex `CHARACTERS.TXT` gives as `HipAxesPointIndex` for
  every character checked.
- **The heading model** is the model `dor_<model>.mod`, loaded if it exists
  (0x417551): an arrow of three vertices. Its tip is the vertex across from
  the shortest edge; the heading is from one end of that edge to the tip
  (0x41c14e), plus `mHeadingModelAdjust`.
- It is **in use while the animation playing animates it** (0x41c20b): the
  walk animations of characters with one (`dor_gab_gabwalk`,
  `dor_gab_gabturnright2walk` and so on). When such an animation starts,
  `mHeadingModelAdjust` is set so the two readings agree (0x41bfd9), so the
  heading model carries the facing on through the animation's turn.
- Models without one (`em2` among them) keep the hip method throughout.

Only four walk animations are handed to the walker (0x4172cf: start,
cycle, turn-left and turn-right starts); `StopAnim` is not among them. The
default turn speed is scaled from the animations (0x419619), and the
collision radius comes from the model's size (0x41985e).

## The kinds of walk (0x41c34d–0x41d34a)

Every walk starts from the same defaults (0x4198c2): position accuracy 4.0,
no heading (2^31 stands for none), idle again after, skip into view, stop
type 0. Each kind then sets its own. Accuracies are in units and radians.

| Kind | Built at | Arrives within | Heading within | Notes |
|---|---|---|---|---|
| Player's click | 0x41d34a | 4.0 | (none) | `mbUserWalk`; within 24 of where they stand, no walk at all |
| `WalkTo` | 0x41cf76 | 2.0 | 0.262 (15°) | must be exact; skips into view only for the ego |
| `WalkToAnimation` | 0x41cdd8 | 1.0 | 0.015 (0.86°) | |
| `WalkToXZ` | 0x41d1fc | (given) | (none) | must be exact |
| `WalkNear` | 0x41c34d | (given distance) | 0.785 (45°) | |
| `WalkNearModel` | 0x41c41d | (given) | 0.785 (45°) | faces along the route's end |
| `WalkToSeeModel` | 0x41c595 | (given) | 0.262 (15°) | stop type 1: stops once the model is in view |
| `TurnToModel` | 0x41cbf8 | (none) | 0.015 (0.86°) | turns on the spot |
| `ClearRegion` | 0x41c74a | 2.0 | 0.262 (15°) | must be exact |
| (region walk) | 0x41d0a8 | (given) | — | stop type 2: stops on entering a region |
| (fidget walk) | 0x41d29c | 2.0 | 0.262 (15°) | must be exact |

`mStopType` is how a walk is judged finished (0 on reaching the spot, 1 on
seeing the model, 2 on entering the region), not how it ends visually.

**Close to the end before it starts** (0x41dadd, checked by the click at
0x41d34a): within 24 units, a walk that must be exact puts the walker on the
spot and heading at once (0x41826b moves the models straight there); a
click does nothing.

**A click that near while walking** (measured from where the walker now
stands) still replaces the walk under way with the click's, which asks for
no heading (0x417d4a copies it over), and stops it: setting off or striding
(states 0 to 2), the walker goes into the stop where they are, with no
final turn (0x4187fa); already stopping or paused (3 and up), the walk is
done at once (0x4188e4). Anything the old walk was to face is dropped.

## An action's approach (0x406d5a)

A rule's `approach=` picks the walk made before its script runs, from the
table at 0x6ee4d8: 1 `WalkTo`, 2 `WalkToAnim` (`Anim`), 3 `WalkNear`,
4 `WalkNearModel` (`NearModel`), 5 `WalkToRegion` (`Region`), 6 `TurnTo`,
7 `TurnToModel`, 8 `WalkToSee`, 9 `UserWalk`. In the action files:
`WalkToSee` 2,120 rules, `WalkTo` 688, `Anim` 398, `TurnToModel` 397,
none 105, `NearModel` 17, `Region` 1.

- **`WalkToSee`** (0x41c595): the target is the middle of the model's
  bounds, its reach half their diagonal (0x41c92c). Within reach there is
  no walk; else a walk (arriving within 24, or the reach if more) that stops
  once the model is in view (stop type 1). Either way it ends facing the
  model's middle within 15° (`mUsePathEndAsDestHeading` 2: the heading is
  set, on stopping, towards the model, 0x418860).
- **`WalkNearModel`** (0x41c41d): towards the middle of the model's bounds,
  arriving within its reach plus the given distance; it ends facing along
  the route's end (`mUsePathEndAsDestHeading` 1: towards where it was
  going) within 45°.
- **`TurnToModel`** (0x41cbf8): a walk that does not move, its heading
  towards the middle of the model's bounds, within 0.86°.
- **`WalkNear`** (0x41c34d): to a named position, within the given
  distance, at its heading within 45°. No action file uses it.

The final turn of each is the walk's own (below): none if already facing
within the tolerance, else at the walker's turning speed to within a
quarter of it. The action's script runs once that is done.

## Footsteps (0x40a23d)

A `FOOTSTEP` cue (0x4a0d29) plays the walker's footstep through the sound
system as a DirectSound 3D sound at the walker's position, at full volume,
with a minimum distance of 60 and a maximum of 840 (0x6ec874, 0x6ec878:
the same for everyone). DirectSound fades it as `min / (min + rolloff ×
(distance − min))`, no further past the maximum; the sound system sets its
listener's rolloff to 3 when it starts (0x5adae8, applied through
0x5aef54) and its distance factor to 0.0254, a unit an inch (0x5aee30).
The buffers are made to fall silent beyond their maximum
(`DSBCAPS_MUTE3DATMAXDISTANCE`, 0x5b1985). So a footstep 200 units away
is at an eighth of full volume, and one beyond 840 is not heard. How
these are heard, and panned, is in `re-sound.md`. The characters' `SoundMinDistance` and
`SoundMaxDistance` (read at 0x455028, default 10⁹ each, set by no
character) are not used for footsteps.

## Setting off, and the final turn

**The walker's states** (`mCurWalkState`, +0x140; update 0x418bed): 0 about
to set off, 1 start animation playing, 2 walk cycle, 3 stopping (and
turning to the heading), 4 paused by a collision, 5 done.

**Setting off** (0x4186de): the signed angle from the walker's facing to the
route's first point (positive clockwise) picks the start animation: from
90° to 180°, the turn-right start; from −180° to −90°, the turn-left start;
otherwise the plain start. Each only if the character has it. There is no
distance condition (G-Engine's "over 30 units away" is its own).

Standing still with the first leg more than 90° round (0x417858), the walk
is kept pending and the walker goes through the stop state first.

**Walk animations** sit in five slots by type (+0xfc): 1 start, 2 turn-left
start, 3 turn-right start, 4 cycle; slot 0 would be the stop. Only types 1
to 4 are ever started (0x41dec9's callers): **`StopAnim` is never played.**

**After the turn-right start** (0x418bed → 0x4187d5), the cycle starts at
the character's `StartTurnRightFrame` (Gabriel's is 10; nobody else sets
it), not at its first frame. After the other starts, at frame 0.

**The final turn** (0x4187fa, 0x41ad54, 0x418d19): if the walker already
faces within the walk's heading tolerance, there is no turn. Otherwise the
tolerance is quartered if over 3.6° (0.0628 rad), and the walker turns
until within it, then stops (0x4188e4). The heading is not then set
exactly: `WalkTo` ends within 3.75° of its heading.

**Steering** (0x418d95, 0x418df1): each frame the walker turns towards the
route by a limited step (0x41819b) and turns its models by the same step
(0x41826b), keeping the total since the animation started
(`mHeadingAdjustedSinceAnimStart`). During a turning start it does not
steer until the heading (read from the heading model) has begun to change,
and then only by what the animation's own turn leaves over: the target less
(`mHeadingAtAnimStart` + the built-in turn + the steering so far). The
built-in turns are 135° for the turn-right start and 225° (that is, −135°)
for the turn-left (0x666188, 0x66618c), which is what Gabriel's heading
model measures over his.

**Placing an animation**: the model is turned so the animation's facing at
its first frame lines up with the walker's heading (0x41dec9). While an
animation animates the heading model, that facing is the heading model's,
which follows the body; the hip does not always (in Gabriel's turning
starts it is well off).

**Finishing** (0x4188e4): a walk that must be exact is moved onto its spot
(position only).

## Walkers meeting (0x418c91, 0x4190cf, 0x418ee3, 0x419055, 0x419270, 0x41938c)

Each frame, while a walker is setting off or walking, and only where the
scene has a walk map and the walker's `mbUseBoundary` is set:

- **Who is near** (0x418ee3): every other actor, by the floor distance
  between them less the other's radius (the gap). Standing actors are all
  obstacles; walking ones are obstacles within twice this walker's radius,
  and *colliding* within its radius.
- **Colliding** (0x419055): if the player's character is walking and among
  them, and this walker is someone else, this walker pauses; the player's
  character never gives way. Otherwise every walking walker it collides
  with pauses, and it walks on.
- **Obstacles** (0x4190cf): when the set of them changes and one stands
  within the two radii of the route still to walk, the walk is planned
  again from where the walker is (0x41d9ab).
- **Pausing** (0x419270): state 4, for a random 0.75 to 2 seconds
  (`751 + rand()/32767 × 1250` ms; under one second becomes 1.0–1.25 for a
  walker caught setting off or walking), standing.
- **Looking again** (0x41938c): still colliding, another pause; else a new
  route from where it stands, setting off again. After 30 seconds of the
  walk held up in all (`mStateCollisionPausedTime`), it no longer checks,
  and walks on; with no route at all, it counts as there (an exact walk is
  put on its spot).
- **The radius** (`mCollisionRadius`, 0x41985e): from the model's bounds
  (0x545430: each showing mesh's box about its showing parts' vertices,
  less the four trailing points some parts carry, a tenth larger), the
  length of half their width and depth together, at most 36. (Ours had
  counted the trailing points, which lie far out, so every radius was 36.)

`mbUseBoundary` is cleared for every actor at 0x43fd28 and set again at
0x440171, from an interface mode's code (0x4403ac) still to identify; not
around scripted scenes.

### Applied to gk3-rs

- A click arrives within 4 (was 2) and does nothing within 24 (was 12.25).
- `WalkToAnimation`, and an action's approach into an animation's first
  pose, arrive within 1 (was 2), and face within 0.86° (was 4°, exactly).
- `WalkTo`, an action's approach to a named spot, a fidget's walk and a
  region walk arrive within 2 and face within 15°: no final turn if already
  that near, else a turn to within 3.75°, not exactly.
- Turning starts are chosen by the first leg's angle, as above, with no
  distance condition (was: the goal more than 30 away and behind).
- Walkers meeting, as above: pauses (the player's character has the right
  of way), new routes round obstacles that move into the way, giving up
  after 30 seconds. Collision radii are half the model's breadth across the
  floor (whether the original's measure includes height is unconfirmed),
  at most 36. The pause's random length uses its own sequence, not the
  story's.
- After the turn-right start, the cycle goes on from `StartTurnRightFrame`.
- Every walk clip of a character with a heading model (setting off from
  standing, the start into the cycle, one cycle into the next) is placed so
  the heading model's direction carries straight on, as the original
  places animations by it. The body no longer swings round at a change of
  clip (it did at both ends of Gabriel's turning starts).
- During a turning start, a character with a heading model is steered as
  the original steers: once the heading model shows the turn under way, by
  what the start's own turn (±135°) leaves over. Gabriel now comes out of a
  turning start facing his route.

### Decided against

- Exact walks that start within 24 of their spot: the original puts the
  walker there at once; ours walks, deliberately (see `deviations.md`).
- The end of every walk: the original snaps onto the spot and into the
  standing pose; ours eases in over a quarter of a second, deliberately.
  The stance is turned so the heading model's direction carries on from the
  stride's, and the pose stopped in is first turned about its floor point to
  match it, so the two blend without the body folding; the model then stays
  turned so throughout. Arriving part way through a turning start, the hip
  mesh can swing 30° to 50° as the trailing leg comes in, but the body (as
  seen from above) keeps its direction and the floor point stays within 0.1
  units. Measured over clicks in 24 directions and from 27 to 80 units
  away in R25. A walker paused by another eases into standing the same way.

### Not yet applied

## Triggers

A scene file's `[TRIGGERS]` (a noun and a floor rectangle; parsed at
0x512110, the rectangle read with `strtok` and normalised) become
`Trigger` objects (`mNoun`, `mRects`, `mEnable`; saved at 0x5259d3).

**When they trip.** At the end of each frame's update (0x41838e), walking
or standing, the ego's walker looks for a trigger where it stands
(0x4194d7 → 0x525c61): the first, in order, that is enabled, holds the
point in one of its rectangles, and whose noun has a `WALK` action now
(0x525a1b). If there is one and it is not the one found last time
(+0x374), it is sent a message to trip; either way what was found (or
nothing) is kept as the last. So a trigger trips on coming into it, and
again only after leaving it, or after its `WALK` action has been
unavailable for a frame.

**What tripping does** (0x525a57): runs `noun:WALK`, if the action
manager will take it (0x4077b4): with no action, or one whose state
(`mCurrentState`) is 0 or 2, walking to where it is done. In states 1, 3
and 4 (its script playing) the trip does nothing, and the trigger, already
kept as the last, does not trip again until left. Only the ego's walker
looks (+0xb4 against the ego).

**Skipping a walk** (`SkipEgoWalk`, 0x41d4f9 → 0x418564 → 0x4e0e8f) and a
walk skipped into view jump along the route a step at a time, stopping at
the first point in a trigger not yet tripped (which then trips) or at the
end.

## Still to trace

- The interface mode that switches walkers' collisions off (0x4403ac).
- Characters without a heading model still have each clip placed by the
  hip, and do not steer during a turning start (none of them has one).
