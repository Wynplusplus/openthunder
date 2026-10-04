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
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            plane: planes::default_plane().to_string(),
        }
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
            }
        }
        settings
    }

    pub fn to_config_string(&self) -> String {
        format!(
            "# OpenThunder settings\n# Aircraft: one of {}\nplane = {}\n",
            planes::PLANES
                .iter()
                .map(|plane| plane.id)
                .collect::<Vec<_>>()
                .join(", "),
            self.plane,
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
