# Grace's GPS in the original

Notes from `GK3.exe` on the `CoordinateDevice` scene (class at 0x46ee8f,
vtable 0x675e0c; saved fields named at 0x46f16e).

## Its settings (0x46f4b7)

`GPS.TXT`, the section named for the room (`[ler]`, `[mcf]`, `[bec]`):
the panel's picture (`name`), the map's lie (`angXtoN`, `worldWidth`,
`mapOriginXPct`, `mapOriginYPct`) and a point and its latitude and
longitude (`sigx`, `sigz`, `sigLat…`, `sigLng…`). Then the size's section,
by the screen's width (0x46f7ca): 1024 and over `[large]`, 800 and over
`[medium]`, else `[small]`, drawn unscaled: the pictures' ending, the
lines, target and font, and where things go, in screen pixels.

The panel is `name` plus the ending (`gpsLER_L`), at (2, 2). Its
`_ALPHA` bitmaps are not loaded: the panel is drawn colour-keyed.

## On and off

`CallSceneFunction` (0x46f3c5) takes any name starting `On` or `Off`:

- **On**, if the panel is not up (0x46feb0): the panel comes up, with a
  trap for clicks over it and the power button, off; `NoGPSTurnOff` is
  cleared; a second later (`mTurnOnOffTimer`, `State` 1) it comes on
  (0x46ff2e): `GPSOn` is set, and the lines, target and readings are
  shown, the button lit.
- **Off**, if the display is on (0x470031): the lines, target and
  readings go, the button is unlit; a second later (`State` 2) the panel
  goes (0x4700ce) and `GPSOn` is cleared.
- **The power button** (0x470167), unless `NoGPSTurnOff` is set: as
  **Off**, and `GRACE:COORDINATE_FIXING_DEVICE`'s count is set to 1; once
  the panel has gone, the room's `HideCoord` is called (`ler307a`,
  `mcf_all` or `bec312p`, by room, 0x4702a3), which has Grace put it away.

Entering the room clears `GPSOn` (0x4ffe19 → slot 0x104); leaving it puts
the device away (0x4fff39 → slot 0x10c). A room put back from a saved game
with `GPSOn` set has it up and on (0x4701d2).

## The readings (0x47035e)

Every quarter second (`lastUpdate`) while `GPSOn` is set, from the
player's walker's position (x, z):

- Turned north up: (x′, z′) = (x cos a + z sin a, z cos a − x sin a),
  a = −`angXtoN` degrees (0x5410b0).
- With s = `mapWidth` / `worldWidth`, across = int(`mapWidth` ×
  `mapOriginXPct`) + int(x′ s), down = `mapWidth` − int(`mapWidth` ×
  `mapOriginYPct`) − int(z′ s). The origin's height is taken up from the
  map's foot, though `GPS.TXT` and the art's notes say down from its top.
- The vertical line at (`cornerWidth` + across, `cornerHeight`), the
  horizontal at (`cornerWidth`, `cornerHeight` + down), the target at
  (`cornerWidth` − `targetWidth`/2 + across + 1, `cornerHeight` −
  `targetWidth`/2 + down).
- (x − `sigx`, z − `sigz`) turned the same way gives east and north; the
  seconds are the point's plus a thousandth for each unit, and the minutes
  carried once if they go under 0 or over 60. Written `%02d°%02d'%05.2f"`
  (0x676054) at (`textStartWidth`, `latHeight`) and (…, `lngHeight`).

## Ours

As above, but the size is picked by the window's height as a 4:3 screen
(768 and over large, 600 and over medium, else small) and scaled to the
window with the rest of the interface. At the original's own
resolutions this is the same picture.
