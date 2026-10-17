# TE1's chessboard in the original

Notes from `GK3.exe` on the `Chess` scene (`custom=Chess`, TE1; class at
0x456801, vtable 0x673d74; saved fields named at 0x4569e7: `notify`,
`pTempTimer`, `animCount`, the board as `currMap0` to `currMap7`).

## The board

Each tile has a state (+0x17c, by column then row): 0 plain, 1 a lit
sword, 2 a sword gone dark, 3 a trap, 4 gone. It starts from a table
(0x6ee23c): sixteen swords in a diamond, twelve traps, the rest plain. A
sword is black where its row and column sum even, else white
(`te1swordb`, `te1swordw`, and `_glow` lit; 0x4574b9).

The engine's game variables (state +0x1940 on): `Te1SwordCount`,
`Te1TileRow`, `Te1TileColumn`, `Te1GabeRow`, `Te1GabeColumn`,
`Te1MoveType`, `Te1MoveCode`, `Te1TileState`.

## A click (0x456b81)

- **A tile** (`te1floor`, its column letter, its row digit): `Te1TileRow`,
  `Te1TileColumn` and `Te1TileState` (its state) are set. Off the board
  (`Te1GabeRow` below 0), `Te1MoveType` is 1 for the first row, else 0.
  On it, with the row and column apart (dr, dc): within two of each and
  three together, `Te1MoveCode` is dr × 5 + 12 + dc and `Te1MoveType` 1 if
  together three (a knight's move), else 2; further, `Te1MoveType` 0. The
  click is left to the room's actions, whose rules choose by `Te1MoveType`
  (1 a jump, 2 "too far", 0 none).
- **The floor round the board** (`te1tilefloor`): off the board, a walk;
  on it, `Te1MoveType` 1 from the first row, else 2, and `SCENE JUMP` is
  done.
- **Anything else**, on the board: `1SERM44V51` ("I don't have time for
  that now!"); off it, left to the room. Hovering does nothing.

## Its functions (0x456f39)

- `takeoff`: from a plain tile, it falls three quarters of a second later
  (a timer, 0x2ee; its event 1000 hides it with `te1trapdooropen`), and is
  gone at once; from a dark sword nothing; else not its own.
- `landed`: on a lit sword, its unlit animation (`Te1UnLight`), the sound
  `te1swordoff`, the count up one and the sword dark; the sixteenth sets
  `AllSwords`. On a dark sword or a trap, the tile hidden, `te1trapdooropen`,
  gone. On a plain tile nothing.
- `badland`: the tile hidden, `te1trapdooropen`, gone. `hidecurrenttile`:
  hidden only. `fell`: nothing.
- `centerMe`: `Te1MoveCode` 1 if every sword is done and Gabriel stands at
  row 8, column E (the way out), else 0. It does not move him.
- `reset` and `clearTiles`: the board as it starts, every tile shown, the
  swords lit (`reset`) or not, the count 0 and `AllSwords` cleared.
- `restart` (sent by the death screen, 0x4d5b85, after the location's
  `Restart$`): Gabriel off the board (-1, -1), `FLOOR` and `FLOOR_TILES`
  walk counts 0, the count 0, `AllSwords` cleared, `Te1godoor` shut.

## Ours

As above. Where `Te1GabeRow` is first set below 0, before any death, is
not traced; ours sets it -1 on entering if the story has no value.
