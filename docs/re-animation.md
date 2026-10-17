# Animations in the original

Notes from `GK3.exe` on how scripts start animations.

## `StartMorphAnimation(animationName, animStartFrame, morphFrames)`

The script call (0x50b13c → 0x4aa4e4) does nothing unless the animation
exists, `animStartFrame` is before its last frame and `morphFrames` is
above 0. Otherwise the animation manager (0x420be4) makes an animation of
its own kind (`mbIsMorph`, `mAnimStartFrame`, `mMorphFrames`; 0x41fae4),
which the renderer plays (0x55ac70 → 0x55acf0 → 0x552b70):

- what the animation moves is posed at `animStartFrame`, where the
  animation puts it;
- each frame of the animation's rate (0x552ed0), every vertex is placed in
  a straight line between where it stood and where that frame has it, at
  the time so far over `morphFrames − 1` frames (0x553700), and at the end
  exactly there;
- then it holds, and the message the script waits on is sent.

The game's only uses are four in CS8 at 202P, each
`StartMorphAnimation(…, 1, 5)`: a quarter of a second's easing into the
first frame of `GabCseLookDown` (which then holds) or of `GabCs8AhNo`
(which the script then plays). `StopMorphAnimation` and
`StopAllMorphAnimations` are used by no script.

## Actors and props at the start of a scene

Creating an actor from a scene file's `[ACTORS]` line (0x502c9d), its
walker is placed at its `pos=` if the scene defines that position;
otherwise it stays where the walker was made: the origin, facing 180
degrees (0x416873). The actor's model is built from the same model
definition a prop has (0x50db23 → 0x4d6810), which keeps the `initAnim`
(+0x20 on the model).

As the scene starts (0x500a77), each model, then each actor (0x501920),
is posed (0x50186f): with an `initAnim`, that animation is loaded, its
first frame applied to the model (0x54d880 → 0x549630: each mesh as the
animation has it), and let go again (0x54d930). It does not play, and the
walker is not moved. An actor without one (+0xa4 still 3, from setup at
0x409467) has its walk's start animation started and stopped at once
(0x41d50a → 0x417774): it stands at its walker in that animation's first
frame. Then its idle fidget is started (0x40a3db, 0x40a1b5).

A walker that is not walking (state 5) follows its model every frame
while the model is visible (0x41838e: 0x4d6d81, then 0x417e56 takes the
walker's position and heading from the posed model, 0x41c0e8). So an
actor posed by its `initAnim` stands where the pose puts them from the
first frame, unless hidden; a hidden one's walker stays where it was
made until it is shown.

## Props in an actor's animation (0x420346)

An animation's clips are written in one space for all the models it
moves; a relative one (no placement on its `[ACTIONS]` line) does not
place its models itself (a track sets its model's transform only when the
line gives one, 0x54c4b0, 0x54c6b0). Starting an animation (0x4209ea →
0x41fddc → 0x4201b9), once it plays, 0x420346 takes the transform of each
actor it moves relatively, and gives every prop (a scene model of type 2)
it moves relatively the last of them: Buthane's cigarette and smoke go
with her (`MadSmokingStart` and the rest). With no such actor, the props
are given the world's origin. A prop posed by an `initAnim` (0x50186f) does
not go this way, and stays where it is.
