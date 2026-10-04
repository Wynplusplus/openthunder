//! The list of servers the launcher can offer.
//!
//! Stored as a simple `name = address` file (`servers.conf`), so players can add
//! their own servers by editing it and the launcher will list them. The address
//! is a `host:port` TCP endpoint for the dedicated server.

use std::fs;
use std::io;
use std::path::PathBuf;

use crate::keybinds::config_dir;

/// One selectable server.
#[derive(Clone, Debug, PartialEq)]
pub struct ServerInfo {
    pub name: String,
    pub address: String,
}

impl ServerInfo {
    fn new(name: &str, address: &str) -> Self {
        Self {
            name: name.to_string(),
            address: address.to_string(),
        }
    }
}

/// Built-in defaults, used when no `servers.conf` exists yet.
pub fn default_servers() -> Vec<ServerInfo> {
    vec![
        ServerInfo::new("Local", "127.0.0.1:7777"),
        ServerInfo::new("Local (port 7778)", "127.0.0.1:7778"),
    ]
}

/// A parsed server list.
#[derive(Clone, Debug)]
pub struct ServerList {
    pub servers: Vec<ServerInfo>,
}

impl Default for ServerList {
    fn default() -> Self {
        Self {
            servers: default_servers(),
        }
    }
}

impl ServerList {
    /// Load `servers.conf`, creating it with defaults if missing/empty.
    pub fn load_or_create() -> Self {
        match Self::load() {
            Ok(list) if !list.servers.is_empty() => list,
            _ => {
                let list = Self::default();
                let _ = list.save();
                list
            }
        }
    }

    pub fn load() -> io::Result<Self> {
        let text = fs::read_to_string(servers_path())?;
        Ok(Self::parse(&text))
    }

    pub fn parse(text: &str) -> Self {
        let mut servers = Vec::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((name, address)) = line.split_once('=') {
                let name = name.trim();
                let address = address.trim();
                if !address.is_empty() {
                    servers.push(ServerInfo {
                        name: name.to_string(),
                        address: address.to_string(),
                    });
                }
            }
        }
        if servers.is_empty() {
            Self::default()
        } else {
            Self { servers }
        }
    }

    pub fn to_config_string(&self) -> String {
        let mut out = String::from("# OpenThunder servers: name = host:port\n");
        for server in &self.servers {
            out.push_str(&format!("{} = {}\n", server.name, server.address));
        }
        out
    }

    pub fn save(&self) -> io::Result<()> {
        let path = servers_path();
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(path, self.to_config_string())
    }
}

/// Full path to the server list file.
pub fn servers_path() -> PathBuf {
    config_dir().join("servers.conf")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_name_address_pairs() {
        let list = ServerList::parse("# c\nLocal = 127.0.0.1:7777\nHome = 10.0.0.5:9000\n");
        assert_eq!(list.servers.len(), 2);
        assert_eq!(list.servers[0].name, "Local");
        assert_eq!(list.servers[1].address, "10.0.0.5:9000");
    }

    #[test]
    fn empty_falls_back_to_defaults() {
        assert!(!ServerList::parse("").servers.is_empty());
    }
}
