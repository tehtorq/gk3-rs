# Saving and restoring in the original

Notes from `GK3.exe` on saved games. In short: a save is a snapshot of
every live object the engine knows how to persist, and a restore throws the
running game away and rebuilds it from that snapshot. Nothing is entered,
no scene script runs and no visit is counted. The one exception is the
background scripts: they are not saved mid-run, but noted by name and
started again from the top.

## When a save is allowed

Every save goes through 0x4ab1d2 (from the quick-save key, 0x4bdf3e; the
save screen's button, 0x4cb034; and the scripts' `SaveGame`, 0x50b31d). It
saves only if the action-mode stack (state +0x3c, see `re-skip.md`) has
mode 0 on top (0x4a881b): the player in control, with no action, script,
movie or hands-off running. The save screen is closed first (layer method
+0xe8).

The quick-save key, and the in-game Save, also ask 0x4cbe72:

- not busy (0x4ec511): no conversation open (the stage's `mConversations`,
  StageMgr +0xc then +8) and no line being spoken (dialogue manager +0x110);
- no verb menu up (`mpCurrentVerbChooser`, 0x706f70; 0x528b9e);
- not in TE6 (location 0x45);
- the top layer (0x4c487a, the slots made by 0x4c4977) is not binoculars
  (0), close-up (1), the controls (2), quit (3), the driving map (4), help
  (6), inventory (8), the load screen (9), the LSR puzzle (10), a message
  (11), a movie (12), pause (13), the save screen (14) or the finished
  screen (20). The game itself (7 or 15), fingerprinting (5), SIDNEY (16),
  a timeblock's title card (17) and the death screen (19) allow it.

Restoring (0x4cbebf) also needs the player in control, and not the
binoculars, a movie, the save screen or the finished screen on top.

## What a save holds (0x4ccd9f)

The file is `<name>.gk3` (the quick save is `fastsave.gk3`, 0x6ee5e0). It
is written by the engine's persistence system ("GEngine Persistence",
0x57e410 and 0x57faa0), in binary unless asked for text:

1. a header (0x57d6d0, 0x4cc0d1): the tags `GK3!` (0x21334b47) and `Save`,
   the game's version (0x10000) and build, the screen's size, the computer's
   and user's names, the location and timeblock (+0xf4, +0xf8, from the
   stage), the score (score manager), a play time, and a 160×120 picture of
   the screen (`PersistHeader`, taken from a 640×480 `CompositionBuffer`,
   0x4cc1f2);
2. every live instance of every class registered with the persistence
   system (0x52f790 registers one), found by walking the class list
   (0x707048) and each class's instances (0x57deb0). Each object writes its
   own fields by name (0x4046f1 and the archive's method +0x40).

That covers the whole game object (0x42648f): `mActionMgr` (the action
playing and its state), `mAnimator`, `mCallbackMgr`, `mCamera`,
`mCameraAngleMgr`, `mCursorMgr`, `mDialogueMgr` (and its `mCaptionMgr`),
`mFlagMgr` (the game state at +0x38: flags, counts, variables, items,
visits, the timeblock passed), `mGameState` (the action-mode stack),
`mGenerics`, `mInterfaceMgr`, `mLayerMgr`, `mReportMgr`, `mScoreMgr`,
`mSheepEngine`, `mSoundTrackMgr`, `mStageMgr` and `mStaticConfig`. Through
them it covers:

- the stage (0x520498, 0x51e5a1): location and timeblock, the last ones,
  `mInitialized`, the verb-menu settings, and the stage itself with its
  `mNouns`, `mActions`, `mConversations`, `mSifData`, `mNVCIndex`, `mScene`
  and `mInset`;
- every actor's walker (0x416d8f, 0x419929): where it stands and faces,
  its walk state, destination, turn speed and route;
- fidgets (0x414693: `mFidgetState`), GAS scripts (0x42bb3f: `mScript`,
  `mRunning`, `mStop`, and their nodes' timers), animations
  (`mSequence`, `mCurrentFrame`, `mNextUpdate`) and morphs;
- timers (0x52384c: `mRunning`, `mPeriod`, `mNextUpdate`, `mAutoRestart`);
- the camera, the conversation (`mCurrConv`, 0x46b7bd), the captions,
  the layers, the interface (`mInventoryBtn` and the rest), and the scenes'
  own kinds: the driving map (`mTimeblockChangeCheckInMap`, 0x47d573), the
  pendulum (0x45799c), the angels (0x41e8b5), and so on.

Each soundtrack saves where it is (0x51c235): `State`, `Stopping`, the
current node and sound, the sound's handle, and how many milliseconds
(`MS`) until its next step.

The Sheep engine (0x5096a2) saves its settings and reporters, but not its
threads. While saving, it lists every thread now in a function whose name
ends `_Background$` (its script, function and owner), and saves that list
(0x509f15).

## A restore (0x4cd300)

A restore is asked for by name (0x4ab163: the quick-load key, the load
screen, the scripts' `LoadGame`). It is posted to the window as message
0x402 (0x499132) and done from the message loop (0x49915b), between
frames. Then:

1. the file is read and its header checked (`Save` at +4 picks the binary
   reader; the version, 0x4cdaff);
2. the running game is taken down (the app's method +0xa4);
3. every object is made again from the file (0x57f1c0) and their pointers
   joined up (0x57faa0). As objects load, some do more:
   - the game object fires its "loaded" events 1, 2 and (once ready) 3
     (0x4261dd);
   - the stage manager reloads its text tables (0x520614: 0x4549f7,
     0x496919, 0x494b54) and puts the screen's gamma back (0x4ac366);
     it does not change location, so 0x5210b2 (the visit count) and
     the scene's entry are not run;
   - each soundtrack carries on from its saved node and step (0x51c110);
4. if the screen's size differs from the saved one, the display is redone
   (0x4cd9b8);
5. the scene is told the time (0x4cda36 → 0x500074, the scene's method
   +0xa8), and the app refreshes (method +0x58);
6. the listed background functions are started again by name, from their
   first line (0x50986b → 0x509a60 → 0x5a0e92), and the list is cleared;
7. message 0x468 ("restored") is sent (0x4ac3ee).

So after a restore the room, its actors (posed, walking, fidgeting), the
timers, the soundtracks, the camera and the interface are as they were.
No `SCENE ENTER` rules, no `SceneEnter` functions and no scene set-up run;
the location's visit count is unchanged; the room's ambient sound and
soundtracks continue from the saved state rather than starting again.

## What is not kept

- Sheep threads. Only background functions (`…_Background$`) come back,
  and they start over from the top. Anything else could not have been
  running, since saving needs the player in control.
- The sounds playing apart from soundtracks: a soundtrack keeps its
  current sound's handle, but nothing restores the sound mid-play.
- A line being spoken, a conversation, the verb menu, a movie: none can be
  up when saving is allowed.

## Not determined

- What the game object's "loaded" events 1–3 (0x4261dd) set off: they are
  callbacks registered elsewhere.
- Exactly what the scene object (`mScene`, class 0x706338) reloads from
  the room's files on restore (its geometry and models are not in the
  file's fields seen here, so are presumably reloaded by name).
- The `[Last Save Game Journal]` string is in the executable, but no code
  reference to it was found.
- Top-layer slots 18 and 21 (made by 0x523cc6 and not made by 0x4c4977)
  are not named here.

## Ours

`gk3/src/world/saved.rs` and `gk3/src/cast/saved.rs` follow this.

- **When.** A save needs the player in control (`World::in_control`: no
  action or its walk, no line being said, no movie, no skip, and not the
  pendulum's fall or kill, which the original holds the player off for,
  0x45898f, 0x458c48). The quick save and the save screen also ask what
  0x4cbe72 asks (`World::may_save`, `Window::may_save`): no conversation,
  no verb menu, not TE6, and not under the binoculars, the driving map,
  the inventory or a close-up, the LSR map, help, pause, a message or the
  finished screen. The fingerprint kit, SIDNEY (a movie playing in it
  too: SIDNEY pushes no mode), a title card and the death screen allow it.
  A restore needs the player in control, or the title screen, a card or
  the death screen, and not the binoculars, a movie or the finished
  screen.
- **What.** The story as it stands, as before, and a snapshot of the
  world: the story as it stood on entering the room, the clock, camera,
  timers, moods, the room's changes (hidden and repainted pieces,
  switched-off clicks, the camera's and the walk map's blocks), every
  figure as it stands (pose, place, walk, animation, GAS script, fidget,
  face, head, paints), the soundtracks, the puzzles (angels, laser heads,
  chessboard, pendulum, bridge), the GPS, SIDNEY and the fingerprint kit,
  and the background functions running, by name. Animations a figure is
  in the middle of are kept by name and loaded again.
- **Restoring.** The room is built again from its files under the story
  as it stood on entry (the same scene, cast and actions it was entered
  with), its own code set up as on entry (`init`), and then the story and
  the snapshot are put over it. No `SCENE ENTER` runs and no visit is
  counted. Each soundtrack carries on from its step: a step still with
  time to run waits it out in silence; one playing a loop runs again. A
  SIDNEY movie that was playing opens again and catches up with the clock,
  without its sound. The background functions start again from the top.

A game saved on a title card is the next timeblock about to start, and is
entered as one. Saves made before snapshots, which have none, are entered
as they always were.
