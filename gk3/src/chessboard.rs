//! TE1's chessboard: Gabriel crosses the temple floor by a knight's moves,
//! and must land on all sixteen sword tiles without landing anywhere twice.
//! A plain tile falls away behind him; landing where he has been, or on one
//! of the trap tiles, drops him to his death. The original keeps this in its
//! code; this follows G-Engine's `Chessboard` (GPL-3.0).
//!
//! The scripts do the jumping. The engine tells them which tile is under the
//! pointer (`Te1TileRow`/`Te1TileColumn`), whether that is a knight's move
//! from where Gabriel stands (`Te1MoveType`: 1 yes, 2 no), and which way it
//! lies (`Te1MoveCode`, which picks his turn and jump). The scripts' scene
//! functions then call back here as he takes off and lands.

use gk3_game::state::{GameState, key};

/// What the room must show or play.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// A tile (by model) shown or fallen away.
    Show(String, bool),
    /// The same, a moment from now: a tile falls once he has left it.
    ShowLater(String, bool, f64),
    /// A sword tile lit or not, by its new texture.
    Texture(String, &'static str),
    Sound(&'static str),
    /// The way out at the far end shut again.
    DoorShut,
    /// Gabriel stood square on a tile, facing up the board.
    Center(String),
}

/// Move types the scripts know.
const VALID: i32 = 1;
const INVALID: i32 = 2;
const SWORD_TILES: u32 = 16;

#[derive(Debug, Clone, Default)]
pub struct Chessboard {
    /// How many times he has landed on each tile, by row then column.
    landed: [[u8; 8]; 8],
    swords: u32,
}

/// A tile's model: `te1floor` and its column letter and row number.
pub fn tile_model(row: usize, col: usize) -> String {
    format!("TE1FLOOR{}{}", (b'A' + col as u8) as char, row + 1)
}

/// The tile a model is, if it is one: row and column from 0.
pub fn tile_of(model: &str) -> Option<(usize, usize)> {
    let rest = model.to_ascii_uppercase();
    let rest = rest.strip_prefix("TE1FLOOR")?;
    let mut chars = rest.chars();
    let col = chars.next()?;
    let row = chars.next()?;
    if chars.next().is_some() || !('A'..='H').contains(&col) || !('1'..='8').contains(&row) {
        return None;
    }
    Some((row as usize - '1' as usize, col as usize - 'A' as usize))
}

fn white_sword(row: usize, col: usize) -> bool {
    matches!(
        (col, row),
        (3, 0) | (2, 1) | (1, 2) | (0, 3) | (7, 4) | (6, 5) | (5, 6) | (4, 7)
    )
}

fn black_sword(row: usize, col: usize) -> bool {
    matches!(
        (col, row),
        (4, 0) | (5, 1) | (6, 2) | (7, 3) | (0, 4) | (1, 5) | (2, 6) | (3, 7)
    )
}

fn var(state: &GameState, name: &str) -> i32 {
    *state.variables.get(&key(name)).unwrap_or(&0)
}

fn set(state: &mut GameState, name: &str, value: i32) {
    state.variables.insert(key(name), value);
}

/// Where Gabriel stands, if on the board.
fn gabe(state: &GameState) -> Option<(usize, usize)> {
    let (row, col) = (var(state, "Te1GabeRow"), var(state, "Te1GabeColumn"));
    ((0..8).contains(&row) && (0..8).contains(&col)).then_some((row as usize, col as usize))
}

impl Chessboard {
    /// A sword tile's look, lit or not; None if it is no sword tile.
    fn glow(row: usize, col: usize, lit: bool) -> Option<Effect> {
        let texture = match (white_sword(row, col), black_sword(row, col), lit) {
            (true, _, true) => "TE1SWORDW_GLOW",
            (true, _, false) => "TE1SWORDW",
            (_, true, true) => "TE1SWORDB_GLOW",
            (_, true, false) => "TE1SWORDB",
            _ => return None,
        };
        Some(Effect::Texture(tile_model(row, col), texture))
    }

    /// Everything as at the start: every tile back, the swords lit (or
    /// not), no landings but on the traps, which count as landed already.
    pub fn reset(&mut self, lit: bool, state: &mut GameState) -> Vec<Effect> {
        set(state, "Te1GabeRow", -1);
        set(state, "Te1GabeColumn", -1);
        set(state, "Te1MoveType", VALID);
        set(state, "Te1TileState", 0);
        set(state, "Te1TileRow", 0);
        set(state, "Te1TileColumn", 0);
        // G-Engine leaves `AllSwords` alone here: clearing it on a death
        // strands a script. It is cleared on the next landing instead.
        let mut out = Vec::new();
        self.landed = [[0; 8]; 8];
        for row in 0..8 {
            for col in 0..8 {
                out.push(Effect::Show(tile_model(row, col), true));
                out.extend(Self::glow(row, col, lit));
            }
        }
        for (row, col) in [
            (1, 3),
            (1, 4),
            (6, 3),
            (6, 4),
            (3, 1),
            (4, 1),
            (3, 6),
            (4, 6),
            (3, 3),
            (4, 3),
            (3, 4),
            (4, 4),
        ] {
            self.landed[row][col] = 2;
        }
        self.swords = 0;
        set(state, "Te1SwordCount", 0);
        out.push(Effect::DoorShut);
        out
    }

    /// He is jumping off a tile: a plain one falls away just after.
    pub fn takeoff(&mut self, state: &GameState) -> Vec<Effect> {
        match gabe(state) {
            Some((row, col)) if !white_sword(row, col) && !black_sword(row, col) => {
                vec![Effect::ShowLater(tile_model(row, col), false, 1.0)]
            }
            _ => Vec::new(),
        }
    }

    /// He has landed. A sword goes dark the first time; a tile landed on
    /// before gives way. The scripts read `Te1TileState` for which.
    pub fn landed(&mut self, state: &mut GameState) -> Vec<Effect> {
        state.flags.remove(&key("AllSwords"));
        let Some((row, col)) = gabe(state) else {
            return Vec::new();
        };
        self.landed[row][col] = self.landed[row][col].saturating_add(1);
        let count = self.landed[row][col];
        set(state, "Te1TileState", count as i32);
        let mut out = Vec::new();
        if count == 1 {
            if let Some(dark) = Self::glow(row, col, false) {
                out.push(dark);
                out.push(Effect::Sound("TE1SWORDOFF"));
                self.swords += 1;
                set(state, "Te1SwordCount", self.swords as i32);
                if self.swords == SWORD_TILES {
                    state.flags.insert(key("AllSwords"));
                }
            }
        } else {
            out.extend(trapdoor(row, col));
        }
        out
    }

    /// The tile he stands on gives way (a bad landing, or a fall).
    pub fn drop_current(&self, state: &GameState) -> Vec<Effect> {
        gabe(state).map_or(Vec::new(), |(row, col)| trapdoor(row, col))
    }

    /// He is put square on his tile again: the jumps drift.
    pub fn center(&self, state: &GameState) -> Vec<Effect> {
        gabe(state).map_or(Vec::new(), |(row, col)| vec![Effect::Center(tile_model(row, col))])
    }

    /// The pointer is on a tile: the scripts learn which, whether he can
    /// get there by a knight's move, and which way it lies.
    pub fn hover(state: &mut GameState, row: usize, col: usize) {
        set(state, "Te1TileRow", row as i32);
        set(state, "Te1TileColumn", col as i32);
        let Some((gr, gc)) = gabe(state) else {
            // Off the board, the first row is the only way on.
            set(state, "Te1MoveType", if row == 0 { VALID } else { INVALID });
            return;
        };
        let (dr, dc) = (row.abs_diff(gr), col.abs_diff(gc));
        let knight = dr > 0 && dc > 0 && dr + dc == 3;
        set(state, "Te1MoveType", if knight { VALID } else { INVALID });
        set(state, "Te1MoveCode", move_code((gr, gc), (row, col)));
    }

    /// A click on the floor around the board while on it: from the first
    /// row he can jump back off; from anywhere else it is too far. Returns
    /// whether the scripts' `SCENE JUMP` should run.
    pub fn floor_click(state: &mut GameState) -> bool {
        let Some((row, _)) = gabe(state) else {
            return false;
        };
        set(state, "Te1MoveType", if row == 0 { VALID } else { INVALID });
        true
    }
}

fn trapdoor(row: usize, col: usize) -> Vec<Effect> {
    vec![
        Effect::Show(tile_model(row, col), false),
        Effect::Sound("TE1TRAPDOOROPEN"),
    ]
}

/// The scripts' number for which way a tile lies from where he stands,
/// which picks his turn and jump: G-Engine's table.
fn move_code((gr, gc): (usize, usize), (row, col): (usize, usize)) -> i32 {
    let (dr, dc) = (row.abs_diff(gr), col.abs_diff(gc));
    let (back, left) = (row < gr, col < gc);
    if dr == 0 && dc == 0 {
        12
    } else if dc == 0 {
        match (back, dr == 1) {
            (true, true) => 7,
            (true, false) => 2,
            (false, true) => 17,
            (false, false) => 22,
        }
    } else if dr == 0 {
        match (left, dc == 1) {
            (true, true) => 11,
            (true, false) => 10,
            (false, true) => 13,
            (false, false) => 14,
        }
    } else if dr == dc {
        match (back, left) {
            (true, true) => 6,
            (true, false) => 8,
            (false, true) => 16,
            (false, false) => 18,
        }
    } else {
        let steep = dr > dc;
        match (back, left, steep) {
            (true, true, true) => 1,
            (true, true, false) => 5,
            (true, false, true) => 3,
            (true, false, false) => 9,
            (false, true, true) => 21,
            (false, true, false) => 15,
            (false, false, true) => 23,
            (false, false, false) => 19,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(state: &mut GameState, row: i32, col: i32) {
        set(state, "Te1GabeRow", row);
        set(state, "Te1GabeColumn", col);
    }

    #[test]
    fn tiles_are_named_by_column_and_row() {
        assert_eq!(tile_model(0, 3), "TE1FLOORD1");
        assert_eq!(tile_of("te1floord1"), Some((0, 3)));
        assert_eq!(tile_of("te1floorz9"), None);
        assert_eq!(tile_of("te1tilefloor"), None);
    }

    #[test]
    fn only_knight_moves_are_valid() {
        let mut state = GameState::default();
        // Off the board, only the first row.
        at(&mut state, -1, -1);
        Chessboard::hover(&mut state, 0, 3);
        assert_eq!(var(&state, "Te1MoveType"), VALID);
        Chessboard::hover(&mut state, 1, 3);
        assert_eq!(var(&state, "Te1MoveType"), INVALID);
        at(&mut state, 0, 3);
        Chessboard::hover(&mut state, 2, 4);
        assert_eq!((var(&state, "Te1MoveType"), var(&state, "Te1MoveCode")), (VALID, 23));
        Chessboard::hover(&mut state, 1, 5);
        assert_eq!((var(&state, "Te1MoveType"), var(&state, "Te1MoveCode")), (VALID, 19));
        Chessboard::hover(&mut state, 2, 3);
        assert_eq!((var(&state, "Te1MoveType"), var(&state, "Te1MoveCode")), (INVALID, 22));
    }

    #[test]
    fn swords_count_once_and_tiles_twice_kill() {
        let mut state = GameState::default();
        let mut board = Chessboard::default();
        board.reset(true, &mut state);
        at(&mut state, 0, 3); // a white sword
        let first = board.landed(&mut state);
        assert!(first.contains(&Effect::Texture("TE1FLOORD1".into(), "TE1SWORDW")));
        assert_eq!(var(&state, "Te1SwordCount"), 1);
        assert_eq!(var(&state, "Te1TileState"), 1);
        let again = board.landed(&mut state);
        assert_eq!(var(&state, "Te1TileState"), 2);
        assert!(again.contains(&Effect::Show("TE1FLOORD1".into(), false)));
        // A trap tile counts as landed on already.
        at(&mut state, 3, 3);
        board.landed(&mut state);
        assert_eq!(var(&state, "Te1TileState"), 3);
    }

    #[test]
    fn plain_tiles_fall_behind_him() {
        let mut state = GameState::default();
        let mut board = Chessboard::default();
        at(&mut state, 0, 0);
        assert_eq!(
            board.takeoff(&state),
            [Effect::ShowLater("TE1FLOORA1".into(), false, 1.0)]
        );
        at(&mut state, 0, 3);
        assert!(board.takeoff(&state).is_empty(), "swords stay");
    }
}
