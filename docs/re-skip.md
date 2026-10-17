# Skipping in the original

Notes from `GK3.exe` on Esc. The manual: Esc "skips current action. (If
Gabriel or Grace is walking, it will skip to the end of their path. If it
is in dialogue, it will skip to the end of the current topic. If you are
in a movie, it will skip to the end of the movie.)" The keys are bound by
name (`Skip Action`, `Skip Walk`, `Skip Dialogue`, `Skip Movie`, `Skip
Glide`, all `Escape`, in the `Bindings` registry key); Ctrl+Q quits.

## The game's modes (state +0x3c)

A stack of modes; a change from one to another calls a handler from the
table at 0x6eedd0:

| Mode | Is | Pushed by |
|---|---|---|
| 0 | the player in control | — |
| 1 | an action playing, skippable | an action starting with a walk to it (0x406bae), or its script running (0x406fc9) |
| 2 | skipping | the skip (0x4075f5, held as the action's `pSkipLock`) |
| 3 | a movie | 0x4d7df0 |
| 4 | hands off, not skippable | 0x406350 |

0 → 1 puts up the `ActionSkipper` layer, which takes the keys; 0 → 4 the
`HandsOffControl` layer, which does not skip. An action's end (0x407317),
or its walk to it ending (0x406c63), lets go of both its locks, popping
the modes.

## Esc with nothing playing (0x49d6c2)

The `Skip Walk` key: every actor in the room has their walk skipped
(0x41d4f9).

## Esc while an action plays (0x49dc78 → 0x49dd22)

Taken unless already skipping (mode 2 on top). Then:

1. the player's walk, if any, is skipped (0x41d4f9);
2. mode 2 is pushed (0x4075f5), and sound types 1 and 8 are switched off
   (0x4b18a5), as is a game setting (+0x449, 0x4995f1);
3. while mode 2 is on top, the game is run on ten seconds at a time
   (0x49dcf1: the engine's clock, then the game's timed things, 10000 ms);
4. the settings are put back.

While mode 2 is on the stack, every walker's update skips its walk
(0x41838e → 0x41a55e → 0x41d4f9). A movie, pushing mode 3, stops the
running on (3 is on top); it plays, and the rest of the action at its own
speed, walks still skipped, until the action ends.

## A walk skipped (0x41d4f9 → 0x418564 → 0x4e0e8f)

Only a walk setting off or striding (state below 3). The route is stepped
along by the walker's radius (`mCollisionRadius`) from where they stand:
the player's walk stops at the first point in a trigger not yet tripped
(0x419473), where they are put, facing along the route, and walk on, and
the trigger trips; otherwise the walker is put at the route's end, facing
the walk's heading (or along it), stopped.

## Ours

As above. The running on stops after an hour of story in any case. What
was being said when the skip came is cut off at its end. Speech and sound
effects do not start while running on; music and the room's ambient sound
go on. The `Skip Dialogue` key's line skip (a click, Space or `.`) is
`re-dialogue.md`'s.
