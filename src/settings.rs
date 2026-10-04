//! Player settings shared by the game and the TUI launcher.
//!
//! Currently this is just which aircraft to fly. Stored next to the keybinds
//! config as a trivial `name = value` file so it can be hand-edited too.

use std::fs;
use std::io;
use std::path::PathBuf;

use crate::keybinds::config_dir;
use crate::planes;

/// Persistent player settings.
#[derive(Clone, Debug)]
pub struct Settings {
    /// Aircraft id (see [`planes::PLANES`]).
    pub plane: String,
    /// Start the game fullscreen (borderless) instead of in a window.
    pub fullscreen: bool,
    /// Server address (`host:port`) to join, or empty for single-player.
    pub server: String,
    /// Name shown to other players.
    pub player_name: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            plane: planes::default_plane().to_string(),
            fullscreen: true,
            server: String::new(),
            player_name: "Pilot".to_string(),
        }
    }
}

impl Settings {
    /// True when the game should connect to a server.
    pub fn multiplayer(&self) -> bool {
        !self.server.trim().is_empty()
    }
}

impl Settings {
    /// Load the settings, creating them with defaults if the file is missing.
    pub fn load_or_create() -> Self {
        match Self::load() {
            Ok(settings) => settings,
            Err(_) => {
                let settings = Self::default();
                let _ = settings.save();
                settings
            }
        }
    }

    pub fn load() -> io::Result<Self> {
        let text = fs::read_to_string(settings_path())?;
        Ok(Self::parse(&text))
    }

    /// Parse the `name = value` format; unknown planes fall back to the default.
    pub fn parse(text: &str) -> Self {
        let mut settings = Self::default();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((name, value)) = line.split_once('=') else {
                continue;
            };
            if name.trim() == "plane" {
                let value = value.trim();
                if planes::find(value).is_some() {
                    settings.plane = value.to_string();
                }
            } else if name.trim() == "fullscreen" {
                settings.fullscreen = matches!(value.trim(), "true" | "1" | "yes" | "on");
            } else if name.trim() == "server" {
                settings.server = value.trim().to_string();
            } else if name.trim() == "player_name" {
                let value = value.trim();
                if !value.is_empty() {
                    settings.player_name = value.to_string();
                }
            }
        }
        settings
    }

    pub fn to_config_string(&self) -> String {
        format!(
            "# OpenThunder settings\n# Aircraft: one of {}\nplane = {}\nfullscreen = {}\n# server: host:port, or blank for single-player\nserver = {}\nplayer_name = {}\n",
            planes::PLANES
                .iter()
                .map(|plane| plane.id)
                .collect::<Vec<_>>()
                .join(", "),
            self.plane,
            self.fullscreen,
            self.server,
            self.player_name,
        )
    }

    pub fn save(&self) -> io::Result<()> {
        let path = settings_path();
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(path, self.to_config_string())
    }
}

/// Full path to the settings file.
pub fn settings_path() -> PathBuf {
    config_dir().join("settings.conf")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fullscreen_defaults_on() {
        assert!(Settings::default().fullscreen);
    }

    #[test]
    fn parses_fullscreen_values() {
        assert!(!Settings::parse("fullscreen = false\n").fullscreen);
        assert!(Settings::parse("fullscreen = true\n").fullscreen);
        assert!(Settings::parse("fullscreen = yes\n").fullscreen);
        // Missing line -> default (on).
        assert!(Settings::parse("plane = Bf 109 G-6\n").fullscreen);
    }

    #[test]
    fn round_trips_through_config() {
        let settings = Settings {
            plane: "Spitfire F Mk IXc".to_string(),
            fullscreen: false,
            server: "127.0.0.1:7777".to_string(),
            player_name: "Wyn".to_string(),
        };
        let parsed = Settings::parse(&settings.to_config_string());
        assert_eq!(parsed.plane, "Spitfire F Mk IXc");
        assert!(!parsed.fullscreen);
        assert_eq!(parsed.server, "127.0.0.1:7777");
        assert_eq!(parsed.player_name, "Wyn");
        assert!(parsed.multiplayer());
    }

    #[test]
    fn blank_server_means_singleplayer() {
        let settings = Settings::parse("plane = F4U-4 Corsair\nserver = \n");
        assert!(!settings.multiplayer());
    }
}
