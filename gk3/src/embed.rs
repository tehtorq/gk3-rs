//! The engine for an embedder that owns the surface, the clock and the
//! input (the browser build): the game as in a window, fed plain input.

pub use crate::cursor::CursorArt;
pub use crate::window::{Frontend, Input};
pub use winit::event::MouseButton;
pub use winit::keyboard::KeyCode;
