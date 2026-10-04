//! Library shared between the game binary and the TUI launcher binary.
//!
//! Only engine-independent code lives here (currently the keybind
//! configuration), so the launcher does not have to depend on Bevy.

pub mod keybinds;
