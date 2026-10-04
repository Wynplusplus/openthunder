//! Library shared between the game binary and the TUI launcher binary.
//!
//! Only engine-independent code lives here (keybinds, the aircraft list, the
//! server list, player settings and the wire protocol), so the launcher does not
//! have to depend on Bevy.

pub mod keybinds;
pub mod planes;
pub mod protocol;
pub mod servers;
pub mod settings;
