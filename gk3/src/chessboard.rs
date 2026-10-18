//! TE1's chessboard: Gabriel crosses the temple floor by a knight's moves,
//! and must land on all sixteen sword tiles, landing nowhere twice. A plain
//! tile falls away behind him; landing again where he has been, or on one
//! of the traps, drops him. The original's `Chess` scene (`custom=Chess`,
//! 0x456801; `docs/re-chess.md`).
//!
//! The scripts do the jumping. A click on a tile tells them which
//! (`Te1TileRow`, `Te1TileColumn`, `Te1TileState`), whether a knight's move
//! takes Gabriel there (`Te1MoveType`: 1 yes, 2 near but no, 0 too far) and
//! which way it lies (`Te1MoveCode`); the scripts' scene functions then call
//! back here as he takes off and lands.

use gk3_game::state::{GameState, key};

/// What the room must show or play.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// A tile (by model) shown or gone.
    Show(String, bool),
    /// A tile falls away this many seconds from now, with the trapdoor's
    /// sound.
    FallLater(String, f64),
    /// A sword tile lit or not, by its texture.
    Texture(String, &'static str),
    Sound(&'static str),
    /// The way out at the far end shut again.
    DoorShut,
    /// A line said, the click taken.
    Say(&'static str),
}

/// A tile's state (the board at +0x17c).
const PLAIN: i32 = 0;
const SWORD_LIT: i32 = 1;
const SWORD_DARK: i32 = 2;
const TRAP: i32 = 3;
const GONE: i32 = 4;
const SWORD_TILES: i32 = 16;

/// The board as it starts (0x6ee23c), by column then row: 2 a sword, 3 a
/// trap, 0 plain.
const START: [[i32; 8]; 8] = [
    [0, 0, 0, 2, 2, 0, 0, 0],
    [0, 0, 2, 3, 3, 2, 0, 0],
    [0, 2, 0, 0, 0, 0, 2, 0],
    [2, 3, 0, 3, 3, 0, 3, 2],
    [2, 3, 0, 3, 3, 0, 3, 2],
    [0, 2, 0, 0, 0, 0, 2, 0],
    [0, 0, 2, 3, 3, 2, 0, 0],
    [0, 0, 0, 2, 2, 0, 0, 0],
];

/// A tile's model: `te1floor` and its column letter and row number.
pub fn tile_model(row: usize, col: usize) -> String {
    format!("TE1FLOOR{}{}", (b'A' + col as u8) as char, row + 1)
}

/// The tile a model is, if it is one: row and column from 0 (0x456b81:
/// ten letters, `te1floor`, a column letter and a row digit).
pub fn tile_of(model: &str) -> Option<(usize, usize)> {
    let name = model.to_ascii_lowercase();
    let rest = name.strip_prefix("te1floor").filter(|_| name.len() == 10)?;
    let b = rest.as_bytes();
    let (col, row) = (b[0].wrapping_sub(b'a') as usize, b[1].wrapping_sub(b'1') as usize);
    (col < 8 && row < 8).then_some((row, col))
}

fn var(state: &GameState, name: &str) -> i32 {
    *state.variables.get(&key(name)).unwrap_or(&0)
}

fn set(state: &mut GameState, name: &str, value: i32) {
    state.variables.insert(key(name), value);
}

/// What became of a click.
#[derive(Debug, Clone, PartialEq)]
pub enum Click {
    /// Left to the room's actions (a tile, with the variables set).
    NotTaken,
    /// Taken, doing these.
    Taken(Vec<Effect>),
    /// The floor round the board while on it: `SCENE JUMP` runs.
    Jump,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Chessboard {
    /// Each tile's state, by column and row.
    board: [[i32; 8]; 8],
}

impl Chessboard {
    fn at(&mut self, row: i32, col: i32) -> Option<&mut i32> {
        self.board.get_mut(usize::try_from(col).ok()?)?.get_mut(usize::try_from(row).ok()?)
    }

    /// A sword tile's look, lit or not (0x4574b9: black where row and
    /// column sum even).
    fn sword_texture(row: usize, col: usize, lit: bool) -> Effect {
        let texture = match ((row + col).is_multiple_of(2), lit) {
            (true, true) => "TE1SWORDB_GLOW",
            (true, false) => "TE1SWORDB",
            (false, true) => "TE1SWORDW_GLOW",
            (false, false) => "TE1SWORDW",
        };
        Effect::Texture(tile_model(row, col), texture)
    }

    /// One of the room's scene functions (0x456f39). None if not its own.
    pub fn call(&mut self, function: &str, state: &mut GameState) -> Option<Vec<Effect>> {
        let (row, col) = (var(state, "Te1GabeRow"), var(state, "Te1GabeColumn"));
        let mut swords = var(state, "Te1SwordCount");
        let mut out = Vec::new();
        match function.to_ascii_lowercase().as_str() {
            // Off the board, and the door shut (0x456e54).
            "restart" => {
                set(state, "Te1GabeRow", -1);
                set(state, "Te1GabeColumn", -1);
                for noun in ["FLOOR", "FLOOR_TILES"] {
                    for who in ["GABRIEL", "GRACE"] {
                        state.set_noun_verb(who, noun, "WALK", 0);
                    }
                }
                set(state, "Te1SwordCount", 0);
                state.flags.remove(&key("AllSwords"));
                out.push(Effect::DoorShut);
                return Some(out);
            }
            // Whether he stands at the way out with every sword done.
            "centerme" => {
                let done = swords == SWORD_TILES && col == 4 && row == 7;
                set(state, "Te1MoveCode", i32::from(done));
            }
            "landed" => {
                let tile = *self.at(row, col)?;
                let (r, c) = (row as usize, col as usize);
                match tile {
                    SWORD_LIT => {
                        out.push(Self::sword_texture(r, c, false));
                        out.push(Effect::Sound("TE1SWORDOFF"));
                        swords += 1;
                        *self.at(row, col)? = SWORD_DARK;
                        if swords == SWORD_TILES {
                            state.flags.insert(key("AllSwords"));
                        }
                    }
                    SWORD_DARK | TRAP => {
                        out.push(Effect::Show(tile_model(r, c), false));
                        out.push(Effect::Sound("TE1TRAPDOOROPEN"));
                        *self.at(row, col)? = GONE;
                    }
                    PLAIN => {}
                    _ => return None,
                }
            }
            "badland" => {
                out.push(Effect::Show(tile_model(row.max(0) as usize, col.max(0) as usize), false));
                out.push(Effect::Sound("TE1TRAPDOOROPEN"));
                *self.at(row, col)? = GONE;
            }
            // A plain tile falls three quarters of a second after he leaves
            // it; swords stay.
            "takeoff" => match *self.at(row, col)? {
                PLAIN => {
                    out.push(Effect::FallLater(tile_model(row as usize, col as usize), 0.75));
                    *self.at(row, col)? = GONE;
                }
                SWORD_DARK => {}
                _ => return None,
            },
            // Every tile back; the swords lit, or not.
            f @ ("reset" | "cleartiles") => {
                let lit = f == "reset";
                for (c, column) in START.iter().enumerate() {
                    for (r, &tile) in column.iter().enumerate() {
                        self.board[c][r] = if lit && tile == SWORD_DARK { SWORD_LIT } else { tile };
                        out.push(Effect::Show(tile_model(r, c), true));
                        if tile == SWORD_DARK {
                            out.push(Self::sword_texture(r, c, lit));
                        }
                    }
                }
                swords = 0;
                state.flags.remove(&key("AllSwords"));
            }
            "fell" => {}
            "hidecurrenttile" => {
                out.push(Effect::Show(tile_model(row.max(0) as usize, col.max(0) as usize), false));
            }
            _ => return None,
        }
        set(state, "Te1SwordCount", swords);
        Some(out)
    }

    /// A click on a piece of the room (0x456b81), `on_floor` whether it is
    /// the floor round the board.
    pub fn click(&self, object: &str, state: &mut GameState) -> Click {
        let (gr, gc) = (var(state, "Te1GabeRow"), var(state, "Te1GabeColumn"));
        let on_board = gr >= 0;
        if let Some((row, col)) = tile_of(object) {
            let (row, col) = (row as i32, col as i32);
            set(state, "Te1TileRow", row);
            set(state, "Te1TileColumn", col);
            set(state, "Te1TileState", self.board[col as usize][row as usize]);
            let kind = if !on_board {
                // Onto the board only by its first row.
                if row == 0 { 1 } else { 0 }
            } else {
                let (dr, dc) = (row - gr, col - gc);
                if dr.abs() < 3 && dc.abs() < 3 && dr.abs() + dc.abs() < 4 {
                    set(state, "Te1MoveCode", dr * 5 + 12 + dc);
                    if dr.abs() + dc.abs() == 3 { 1 } else { 2 }
                } else {
                    0
                }
            };
            set(state, "Te1MoveType", kind);
            return Click::NotTaken;
        }
        if object.eq_ignore_ascii_case("te1tilefloor") {
            if !on_board {
                return Click::NotTaken;
            }
            set(state, "Te1MoveType", if gr == 0 { 1 } else { 2 });
            return Click::Jump;
        }
        if on_board {
            return Click::Taken(vec![Effect::Say("1SERM44V51")]);
        }
        Click::NotTaken
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
    fn knight_moves_near_and_far() {
        let mut state = GameState::default();
        let board = Chessboard::default();
        at(&mut state, -1, -1);
        assert_eq!(board.click("TE1FLOORD1", &mut state), Click::NotTaken);
        assert_eq!(var(&state, "Te1MoveType"), 1);
        board.click("TE1FLOORD2", &mut state);
        assert_eq!(var(&state, "Te1MoveType"), 0);
        at(&mut state, 0, 3);
        board.click("TE1FLOORE3", &mut state);
        assert_eq!((var(&state, "Te1MoveType"), var(&state, "Te1MoveCode")), (1, 23));
        board.click("TE1FLOORD3", &mut state);
        assert_eq!((var(&state, "Te1MoveType"), var(&state, "Te1MoveCode")), (2, 22));
        board.click("TE1FLOORD6", &mut state);
        assert_eq!(var(&state, "Te1MoveType"), 0, "too far");
        assert_eq!(board.click("TE1TILEFLOOR", &mut state), Click::Jump);
        assert_eq!(var(&state, "Te1MoveType"), 1);
        assert_eq!(
            board.click("TE1_SKULLS", &mut state),
            Click::Taken(vec![Effect::Say("1SERM44V51")])
        );
    }

    #[test]
    fn swords_go_dark_once_then_drop_him() {
        let mut state = GameState::default();
        let mut board = Chessboard::default();
        board.call("reset", &mut state);
        at(&mut state, 0, 3); // a white sword
        let first = board.call("landed", &mut state).unwrap();
        assert!(first.contains(&Effect::Texture("TE1FLOORD1".into(), "TE1SWORDW")));
        assert_eq!(var(&state, "Te1SwordCount"), 1);
        let again = board.call("landed", &mut state).unwrap();
        assert!(again.contains(&Effect::Show("TE1FLOORD1".into(), false)));
        assert_eq!(var(&state, "Te1SwordCount"), 1);
        // A trap drops him at once.
        at(&mut state, 3, 3);
        let trap = board.call("landed", &mut state).unwrap();
        assert!(trap.contains(&Effect::Sound("TE1TRAPDOOROPEN")));
    }

    #[test]
    fn plain_tiles_fall_behind_him() {
        let mut state = GameState::default();
        let mut board = Chessboard::default();
        board.call("reset", &mut state);
        at(&mut state, 0, 0);
        assert_eq!(
            board.call("takeoff", &mut state).unwrap(),
            [Effect::FallLater("TE1FLOORA1".into(), 0.75)]
        );
        at(&mut state, 0, 3);
        assert_eq!(board.call("takeoff", &mut state), None, "a lit sword is not the scene's");
        board.call("landed", &mut state);
        assert!(board.call("takeoff", &mut state).unwrap().is_empty(), "swords stay");
    }

    #[test]
    fn the_way_out_needs_every_sword() {
        let mut state = GameState::default();
        let mut board = Chessboard::default();
        at(&mut state, 7, 4);
        set(&mut state, "Te1SwordCount", 16);
        board.call("centerMe", &mut state);
        assert_eq!(var(&state, "Te1MoveCode"), 1);
        set(&mut state, "Te1SwordCount", 15);
        board.call("centerMe", &mut state);
        assert_eq!(var(&state, "Te1MoveCode"), 0);
    }
}
