# The score and the status line in the original GK3.exe

Notes from studying the original executable (GOG `GK3.exe`, image base
0x400000). Behaviour only: nothing here is copied code.

## Scoring

`ChangeScore(event)` (stub 0x50bbfd) goes to the score manager (0x507347):

- the event is looked up in the executable's table of 385 names
  (0x6f0c14, lookup 0x507090); an event not in it does nothing;
- an event already scored does nothing (a flag per event, manager +0xc);
- six pairs rule each other out (0x50721b): once either is scored, the
  other does nothing, whichever came first. Exiting R29 by the balcony (4)
  or entering from it (2); the same in R31; fingerprinting the suitcase in
  R21 or Buchelli's glass in the lobby (2 each); climbing the vines at the
  château (4) or opening the cellar doors (2); seeing Jean and Roxanne in
  R23 (2) or talking to Jean about Wilkes (1); the manuscript without
  prints (2) or with them (10);
- otherwise the event's points (0x685f40) are added to the score (manager
  +4), and message -0x72 is posted to the `RCBarPopper`.

`IncreaseScore` (0x50bdd8) does nothing, and no script calls it.

The most a game can score is not in the executable: the manager reads it
from `GAME.CFG`, `[Logic] Max Score = 965` (0x5071f7), into manager +0. The
table totals 978, 967 with the lesser of each pair left out.

No sound plays: nothing in the score manager or the status line calls the
sound system, and the data has no scoring sound.

## The status line

The `RCBarPopper` (`mpCurrentRCBarPopper`, 0x7061e8, made by 0x4f0a56) is
the input handler that opens the option bar on a right-click (0x4f1187)
and shows a line of status at the top left. Its text is built at 0x4f0dd0,
unless the top screen is the driving map (4) or a movie (12):

    <loc_ + location>, <day + timeblock>, <ScoreText>

each part looked up in `ESTRINGS.TXT` (`loc_lby = Hotel Lobby`, `Day110a =
Day 1, 10am - 12pm`, `ScoreText = Score: %03d of %03d`, filled in with the
score and the maximum), joined by `", "` (0x674dac), in the font
`f_status_fade`, at (5, 5) (0x4f0979). Building it also refreshes the option
bar's own `%03d/%03d` if the bar is up (0x4ec23f).

It is in one of four states (popper +0x1c):

| State | Pointer at the top (y ≤ 16, 0x4f1196) | Pointer away | Score up (-0x72, 0x4f1299) | Timer (-0x71) | Fade done (-0x70) |
|---|---|---|---|---|---|
| 0 hidden | build, 1 | — | build, then 2 | — | — |
| 1 up, held | — | start the 3.5 s timer, 2 | build, then 2 | — | — |
| 2 up, timed | stop the timer, 1 | — | build, then 2 | fade over 1 s, 3 | — |
| 3 fading | stop the fade, build, 1 | — | build, then 2 | — | remove, 0 |

The timer is 0xdac ms (0x4f0a56); the fade 1000 ms (0x4f127a), linear,
the text's opacity going from where it is to nothing (0x4678c4).

The popper also posts -0x72 itself when it is switched on (0x4f1134,
reached through event 0x400 of its base class, 0x489bc9 and 0x489c77). A
screen switches on the event handlers it holds (`mEventHandlers`, 0x4dd455)
when it is made current (0x4bf201). The screens that hold the popper
(0x4dc725) are the game (`GameLayer`, 0x49d410), fingerprinting
(0x48df40), the driving map (0x47c070), the LSR map (0x4ce3d0), and three
not yet named (0x425fe0, 0x45a070, 0x4c0080). When the game's screen is
switched off and on again (arriving in a room, coming back from another
screen) is not yet traced.

The font, `F_STATUS_FADE.FON`, is `f_caption_goudy14ds` in colour
192/192/192, with `Function=Add`, `Source Opacity=0.7` and `Destination
Opacity=0.3` (read at 0x4618f0); how the 2D blitter applies them is not yet
traced.
