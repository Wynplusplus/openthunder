//! Library shared between the game binary and the TUI launcher binary.
//!
//! Only engine-independent code lives here (keybinds, the aircraft list and
//! player settings), so the launcher does not have to depend on Bevy.

pub mod keybinds;
pub mod planes;
pub mod settings;
