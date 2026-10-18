# Scene files in the original

Notes from `GK3.exe` on what it reads of a location's files, beyond what
the other notes cover.

## `.SCN`

Only `[Skybox]` (its six sides and `Azimuth`, 0x5622ee) is read at run
time. `[Models]`, `[Lights]` and the many `Light_*` sections are not
named anywhere in the executable: they were for making the lightmaps.

## `[GROUPS]` (0x50f769, 0x51112b)

`label=name,model,model,...` gathers models under a name, kept by the
scene (+0x14c, 0x503a9c). Only the scripts' `ShowModelGroup` and
`HideModelGroup` use them (0x4ae141 → 0x5004ca: each member shown or
hidden, prop or piece of the room). No script or action calls either, so
the two rooms with groups (HAL, R25) never use them; we read none.

## `[REGIONS]` (0x50f5fd, 0x51267d)

`NAME,rect={x1,z1,x2,z2}` names a floor area, kept by the scene
(`mRegions`, +0x1c4). An action's `approach=Region` (0x407a05 →
0x41d0a8) walks the player into the named one before it runs, if there
is one (0x4f7921); else it runs where they stand. The game's only such
action (RC1's `NO_VACANCIES_SIGN`, `target=SIGN`) names a region no RC1
file has, and the regions there are (`NEAR_EMILIO`, a test `MYTEST`) are
named by nothing, so ours, which runs such an action where the player
stands, does as the original.
