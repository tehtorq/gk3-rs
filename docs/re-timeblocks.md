# When a timeblock ends, in the original

Notes from `GK3.exe` on how the story moves from one timeblock to the next.
No script calls `SetTime`; the rules are in the executable. Ours,
`crates/game/src/timeblocks.rs`, follows these.

## When it is asked (0x5206ad)

Every change of location asks, of the place the player is going, whether
the timeblock is over, as long as the change keeps the timeblock. If it
is, the next timeblock is asked at once, of the same place, and so on
while each holds: a player who has already done the next timeblock's
business goes straight past it. A timeblock passed is noted (state
+0x1bf0, one past it), and no rule for an earlier one holds after.

Opening the driving map (0x47f264) is such a change, to `MAP`.

## The rules (0x492266)

A switch on the timeblock (1 to 17, `110a` to `309p` in the table at
0x6f5e98; 0 is before the first, and always passes). Each case asks where
the player is going (the location table at 0x6f5f24) and counts:

- `0x491a2e` reads a count by noun (table 0x6ef250), verb (table 0x6f61d4)
  and whether the player is Gabriel (`GABRIEL` or `GABRIEL_DISGUISED`) or
  Grace (0x491939). Topics are verbs in the same table: `SetTopicCount`
  and `SetNounVerbCount` both write it (0x491b0b). A count is a byte.
- `0x492ae4` is a flag (names at 0x6ee8f0, one on).
- `0x4929ba` is whether the player carries an item (by its place in the
  list at 0x678680: 8 is `BLOODLINE_MANUSCRIPT`).
- Game variables sit at state +0x18e0 (names at 0x6eebdc, one on).
- Visits are counted by location and timeblock, at state +0xec, 18 to a
  location; not by who visited.

A count stands for "not zero" unless said.

| Timeblock | Going to | Needs |
|---|---|---|
| 110A | RC1 | `COFFEE_POT` POUR, `SAN_GREAL_WORDS` LOOK, `REGISTER` READ, one of `PHONE`/`OTR_PHONE_2`/`OTR_PHONE_1` `PRINCE_JAMES_CARD`, `BUTHANE` T_CHECK_IN > 1, `GIRARD` T_HOLY_GRAIL, `LADY_H_ESTELLE` and `EMILIO` T_INTRODUCE |
| 112P | RC1 | `ABBE` T_TEMPLARS, `MOSELY` T_GRACE and T_SCHATTENJAGER, and `REGISTER` READ > 1 or the flag `SeenLHRoomSwitch` |
| 102P | MAP | the train station (`MARCIE` T_TRAIN_FROM_NAPLES or `ARRIVALS_IN_CU` THINK, and `TAXI_DRIVER` WALLET) or `LARRY` T_TEMPLARS; `WILKES` FOLLOW > 1, `BUTHANE` FOLLOW > 1 |
| 104P | MOP | the train station and `LARRY` T_TEMPLARS, `WILKES` T_INTRODUCE, `VIEW_OF_LHOMME_MORE` BINOCULARS |
| 106P | R25 | `HandshakeSteps` == 6, `GRACE_N_MOSE` T_ABBE |
| 207A | R25 | `BUTHANE` T_LE_SERPENT_ROUGE == 1, `MOSELY` T_TREASURE == 1 |
| 210A | LBY | `PARCHMENTS` PICKUP > 1, `GUN_CASE` OPEN, `MAGAZINE` READ, `MAP_IN_CLOSET` PICKUP > 1, `MAID` FOLLOW; the player goes to CSE instead |
| 212P | R25 | `OLD_LADY` TALK |
| 202P | R25 | `FiveMinTimer202p` == 7, `ESTELLE` FOLLOW |
| 205P | HAL | `LSRState` ≥ 6 |
| 202A | R25 | carries `BLOODLINE_MANUSCRIPT` |
| 307A | HAL | the flag `End307a` |
| 310A | R25 | `GRACE` T_WILKES, T_UNICORN, T_THRONE, T_HOLY_GRAIL, T_MANUSCRIPT and T_PRINCE_JAMES_MEN |
| 312P | DIN | `VIEW_OF_ORANGE_ROCK` BINOCULARS == 2 |
| 303P | R25 | `CEILING` INSPECT |
| 306P | R25 | a visit to R27 this timeblock |

The FOLLOW counts are the driving map's. A follow's script sets the count
to 1; following Buthane or Wilkes all the way counts it once more, as the
line about where they went is said (0x480d65); following Estelle counts it
twice as it starts (0x47f3c7, follow 6).

`LSRState` (+0x1900) is how many of Le Serpent Rouge's signs are found in
order, from Aquarius, up to the first not yet found (0x64a616, the flags
listed at 0x7243e4); six is as far as Cancer. Only R25's 106P script sets
`HandshakeSteps`.

## Ours

The table above, as Sheep conditions, asked on every change of location
and on leaving SIDNEY. The note of the timeblock passed is not kept: the
story never goes back to an earlier one.
