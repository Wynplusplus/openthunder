//! Shared keybind configuration, used by both the game and the TUI launcher.
//!
//! This module is intentionally free of any Bevy (or TUI) dependency so it can be
//! linked into the launcher without pulling in the engine. Keys are stored as
//! canonical strings (`"W"`, `"ArrowUp"`, `"Space"`, ...); the game converts them
//! to Bevy `KeyCode`s and the launcher validates them against [`SUPPORTED_KEYS`].
//!
//! The config lives at [`keybinds_path`] (an XDG config directory). The format is
//! a trivial `name = key` text file so it is easy to edit by hand.

use std::fs;
use std::io;
use std::path::PathBuf;

/// Description of one rebindable action.
pub struct ActionInfo {
    /// Name used in the config file.
    pub name: &'static str,
    /// Human-readable label shown in the launcher.
    pub label: &'static str,
    /// Default key.
    pub default_key: &'static str,
}

/// Every rebindable action, in the order the launcher lists them.
pub const ACTIONS: &[ActionInfo] = &[
    ActionInfo {
        name: "pitch_up",
        label: "Pitch up",
        default_key: "ArrowUp",
    },
    ActionInfo {
        name: "pitch_down",
        label: "Pitch down",
        default_key: "ArrowDown",
    },
    ActionInfo {
        name: "pitch_up_alt",
        label: "Pitch up (alt)",
        default_key: "ControlLeft",
    },
    ActionInfo {
        name: "pitch_down_alt",
        label: "Pitch down (alt)",
        default_key: "ShiftLeft",
    },
    ActionInfo {
        name: "roll_left",
        label: "Roll left",
        default_key: "A",
    },
    ActionInfo {
        name: "roll_right",
        label: "Roll right",
        default_key: "D",
    },
    ActionInfo {
        name: "yaw_left",
        label: "Yaw left",
        default_key: "Q",
    },
    ActionInfo {
        name: "yaw_right",
        label: "Yaw right",
        default_key: "E",
    },
    ActionInfo {
        name: "throttle_up",
        label: "Throttle up",
        default_key: "W",
    },
    ActionInfo {
        name: "throttle_down",
        label: "Throttle down",
        default_key: "S",
    },
    ActionInfo {
        name: "reset",
        label: "Respawn",
        default_key: "R",
    },
    ActionInfo {
        name: "damage_left_wing",
        label: "Test damage: wing",
        default_key: "1",
    },
    ActionInfo {
        name: "damage_engine",
        label: "Test damage: engine",
        default_key: "2",
    },
    ActionInfo {
        name: "damage_tail",
        label: "Test damage: tail",
        default_key: "3",
    },
    ActionInfo {
        name: "repair",
        label: "Repair all",
        default_key: "0",
    },
    ActionInfo {
        name: "flaps_down",
        label: "Flaps down",
        default_key: "F",
    },
    ActionInfo {
        name: "flaps_up",
        label: "Flaps up",
        default_key: "V",
    },
    ActionInfo {
        name: "wep",
        label: "War emergency power",
        // Not Shift: that is the default for "pitch down (alt)".
        default_key: "B",
    },
    ActionInfo {
        name: "free_look",
        label: "Free look",
        default_key: "C",
    },
    ActionInfo {
        name: "fire",
        label: "Fire guns",
        default_key: "Space",
    },
    ActionInfo {
        name: "gear",
        label: "Landing gear",
        default_key: "G",
    },
];

// Stable indices into [`ACTIONS`] / [`Keybinds::keys`], used by the game.
pub const PITCH_UP: usize = 0;
pub const PITCH_DOWN: usize = 1;
pub const PITCH_UP_ALT: usize = 2;
pub const PITCH_DOWN_ALT: usize = 3;
pub const ROLL_LEFT: usize = 4;
pub const ROLL_RIGHT: usize = 5;
pub const YAW_LEFT: usize = 6;
pub const YAW_RIGHT: usize = 7;
pub const THROTTLE_UP: usize = 8;
pub const THROTTLE_DOWN: usize = 9;
pub const RESET: usize = 10;
pub const DAMAGE_LEFT_WING: usize = 11;
pub const DAMAGE_ENGINE: usize = 12;
pub const DAMAGE_TAIL: usize = 13;
pub const REPAIR: usize = 14;
pub const FLAPS_DOWN: usize = 15;
pub const FLAPS_UP: usize = 16;
pub const WEP: usize = 17;
pub const FREE_LOOK: usize = 18;
pub const FIRE: usize = 19;
pub const GEAR: usize = 20;

/// Canonical names of every key that can be bound.
pub const SUPPORTED_KEYS: &[&str] = &[
    "A",
    "B",
    "C",
    "D",
    "E",
    "F",
    "G",
    "H",
    "I",
    "J",
    "K",
    "L",
    "M",
    "N",
    "O",
    "P",
    "Q",
    "R",
    "S",
    "T",
    "U",
    "V",
    "W",
    "X",
    "Y",
    "Z",
    "0",
    "1",
    "2",
    "3",
    "4",
    "5",
    "6",
    "7",
    "8",
    "9",
    "ArrowUp",
    "ArrowDown",
    "ArrowLeft",
    "ArrowRight",
    "Space",
    "Enter",
    "Tab",
    "Escape",
    "Backspace",
    "ShiftLeft",
    "ShiftRight",
    "ControlLeft",
    "ControlRight",
    "AltLeft",
    "AltRight",
];

/// Returns true if `key` is a recognised canonical key name.
pub fn is_supported(key: &str) -> bool {
    SUPPORTED_KEYS.contains(&key)
}

/// A short, friendly name for a bound key, for display in the launcher and HUD.
pub fn key_display(name: &str) -> &str {
    match name {
        "ControlLeft" | "ControlRight" => "Ctrl",
        "ShiftLeft" | "ShiftRight" => "Shift",
        "AltLeft" | "AltRight" => "Alt",
        "ArrowUp" => "Up",
        "ArrowDown" => "Down",
        "ArrowLeft" => "Left",
        "ArrowRight" => "Right",
        "Escape" => "Esc",
        other => other,
    }
}

/// The player's keybinds, one entry per [`ACTIONS`] entry.
#[derive(Clone, Debug)]
pub struct Keybinds {
    pub keys: Vec<String>,
}

impl Default for Keybinds {
    fn default() -> Self {
        Self {
            keys: ACTIONS.iter().map(|a| a.default_key.to_string()).collect(),
        }
    }
}

impl Keybinds {
    pub fn get(&self, index: usize) -> &str {
        &self.keys[index]
    }

    pub fn set(&mut self, index: usize, key: &str) {
        self.keys[index] = key.to_string();
    }

    /// Load the config, creating it with defaults if it does not exist yet.
    pub fn load_or_create() -> Self {
        match Self::load() {
            Ok(keybinds) => keybinds,
            Err(_) => {
                let keybinds = Self::default();
                let _ = keybinds.save();
                keybinds
            }
        }
    }

    pub fn load() -> io::Result<Self> {
        let text = fs::read_to_string(keybinds_path())?;
        Ok(Self::parse(&text))
    }

    /// Parse the `name = key` config format. Unknown/unsupported entries are
    /// ignored so a hand-edited file can never break the game.
    pub fn parse(text: &str) -> Self {
        let mut keybinds = Self::default();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((name, value)) = line.split_once('=') else {
                continue;
            };
            let name = name.trim();
            let value = value.trim();
            if let Some(index) = ACTIONS.iter().position(|a| a.name == name) {
                if is_supported(value) {
                    keybinds.keys[index] = value.to_string();
                }
            }
        }
        // A clash between WEP and pitch-down can happen for configs saved before
        // WEP moved off Shift. Keep pitch-down and restore WEP to its default.
        if keybinds.keys[WEP] == keybinds.keys[PITCH_DOWN_ALT] {
            keybinds.keys[WEP] = ACTIONS[WEP].default_key.to_string();
        }
        keybinds
    }

    pub fn to_config_string(&self) -> String {
        let mut out = String::from(
            "# OpenThunder keybinds\n\
             # Edit in the launcher, or by hand. Run `cargo run --bin launcher`.\n",
        );
        for (index, action) in ACTIONS.iter().enumerate() {
            out.push_str(&format!("{} = {}\n", action.name, self.keys[index]));
        }
        out
    }

    pub fn save(&self) -> io::Result<()> {
        let path = keybinds_path();
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(path, self.to_config_string())
    }
}

/// Directory the config is stored in.
pub fn config_dir() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        if !xdg.is_empty() {
            return PathBuf::from(xdg).join("openthunder");
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        if !home.is_empty() {
            return PathBuf::from(home).join(".config").join("openthunder");
        }
    }
    PathBuf::from(".").join("openthunder")
}

/// Full path to the keybinds config file.
pub fn keybinds_path() -> PathBuf {
    config_dir().join("keybinds.conf")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_include_ctrl_and_shift_pitch() {
        let keybinds = Keybinds::default();
        assert_eq!(keybinds.get(PITCH_UP), "ArrowUp");
        assert_eq!(keybinds.get(PITCH_DOWN), "ArrowDown");
        assert_eq!(keybinds.get(PITCH_UP_ALT), "ControlLeft");
        assert_eq!(keybinds.get(PITCH_DOWN_ALT), "ShiftLeft");
    }

    #[test]
    fn wep_default_does_not_clash_with_pitch_down() {
        let keybinds = Keybinds::default();
        assert_ne!(keybinds.get(WEP), keybinds.get(PITCH_DOWN_ALT));
    }

    #[test]
    fn old_config_with_wep_on_shift_is_migrated() {
        // A config saved before WEP moved off Shift would have both on Shift.
        let keybinds = Keybinds::parse("pitch_up = ArrowUp\nwep = ShiftLeft\n");
        assert_eq!(keybinds.get(PITCH_DOWN_ALT), "ShiftLeft");
        assert_eq!(keybinds.get(WEP), ACTIONS[WEP].default_key);
    }

    #[test]
    fn key_display_is_friendly() {
        assert_eq!(key_display("ControlLeft"), "Ctrl");
        assert_eq!(key_display("ShiftLeft"), "Shift");
        assert_eq!(key_display("ArrowUp"), "Up");
        assert_eq!(key_display("A"), "A");
    }
}
