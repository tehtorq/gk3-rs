# CS2's laser heads in the original

Notes from `GK3.exe` on the `Laser` scene (`custom=Laser`, CS2; class at
0x4c25d1, vtable 0x67c218; saved fields named at 0x4c27b5:
`mExternalNotify`, `mpLocalTimer`, `turnLeft`, `animFinished`, `lasersOn`,
`turnHead`, `currHead`, `lastUpdate`, `turnRate`, `currHeading`,
`tgtHeading`).

## Entering (0x4c2912)

The heads stand where a table has them (0x6eef6c): (77.026, 51.944,
124.426), (127.718, 51.944, -19.289), (5.607, 51.944, -110.376),
(-118.223, 51.944, -22.235), (-72.012, 51.944, 121.967). Each aims from
its place at height 57; the middle is the five such points' mean. Each
head's five headings (0 to 4) aim at the next head round, the one after,
the middle, then the two after that (x then z into `atan2`).

The settings are the game variables `Cs2Head1` to `Cs2Head5`: all 2 the
first time the room is entered (a flag at state +0x1974 marks that),
else as they were. Each head is put on its place, turned to its heading;
the lasers off.

## Turning (0x4c3023 → 0x4c3197 → 0x4c32b3)

`CallSceneFunction` with any name starting `turn`, its fifth letter `L`
or `R` (either case) and sixth the head (1 to 5). The flag 0x12
(`StaircaseOpen`) is cleared. Grace plays `GraCs2TrnHead` with `L` or `R`
and the setting the head goes to (left one up, right one down, round from
4 to 0 and back); 450 ms on (a timer, 0x1c2), the turn starts: the setting
changes (and its variable), and the head turns at a steady rate from its
heading to the new one, left upwards and right downwards, the long way
round if need be, taking 20 × 66.67 ms (0x700274).

Each frame (0x4c2d6a), a turn under way moves on; at its end, if every
head is on setting 1, or every head on 3 (a pentagram one way or the
other), `StaircaseOpen` is set. The script's `wait` ends when Grace's
animation does (0x4c2f55: a turn not yet over is put on its end then,
untested).

## The lasers (0x4c349c)

`toggleLasers` turns them on or off, shows or hides `cs2laser_01` to
`_05`, and plays `cs2lasersOn` or `cs2lasersOff` (which light the heads
and stop the library's music, or the lasers' drone). While on, each frame
each laser goes from 3 units before its head's aiming point along its
heading, to the first thing it meets within 1000: the room, another head,
or Grace (`gra`), scaled 0.2 across and a hundredth of its length along.
