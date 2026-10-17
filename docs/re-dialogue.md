# Dialogue in the original

Notes from `GK3.exe` on how lines are said and skipped.

## A request for lines

`StartDialogue(plate, numLines)` and its kin make a dialogue request
(class 0x677020; saved as `mLine`, `mLinesLeft`, `mbDoFidgets`,
`mExternalNotify`, 0x478154). It is played a line at a time:

- the first line is started as an object of its own (0x478218 →
  0x477b28) and handed to the dialogue manager (game state +0x34);
- when a line ends (at its `DIALOGUECUE`, 0x4a0688 → 0x478df0), the
  request counts it off (`mLinesLeft`, 0x4782f6) and, if any are left,
  steps its plate on (0x4777d5) and starts the next as a new line;
- after the last, it sends the message the script waits on.

So a script that says four lines waits through four lines, each played
in turn, not one block of time.

## Skipping

The game's keys include `UIKeyType_SkipDialogueLine` (key type 15, beside
`SkipAction`, `SkipGlide`, `SkipEgoWalk` and `SkipMovie`, at 0x6ee59c): a
skip of the line being said. As each line is its own object ending at its
own cue, skipping one ends that line, and the request goes on to its next;
nothing else the story is doing is hurried. (Where the keys are mapped is
not found yet: 0x491f7d, once taken for it, answers whether a noun's verb
may be offered. The line's own stop on a skip is not traced in detail.)

## Ours

As above: a speech plays its lines one after another, the script waiting
until the last is over. A click, Space or `.` ends the line being said
(its voice, captions, the rest of its lip-sync, closing the mouth, and the
speaker changes still to come in it), and the next begins. Other waits,
for animations or timers, keep their time.
