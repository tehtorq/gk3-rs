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
