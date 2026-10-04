//! OpenThunder wire protocol.
//!
//! Messages are newline-delimited and tab-separated so they need no
//! dependencies and are trivial to debug (`nc`/`telnet` friendly).
//!
//! **This file is shared verbatim** between the game (`openthunder`) and the
//! dedicated server (`openthunder_server`). Keep both copies in sync and bump
//! [`PROTOCOL_VERSION`] on any change; the server rejects mismatched clients.
//!
//! Each side uses only part of it (the server never parses `ServerMessage`),
//! hence the blanket dead-code allowance.
#![allow(dead_code)]

/// Bump this whenever the message formats change.
pub const PROTOCOL_VERSION: u32 = 2;

/// State of one aircraft, as broadcast in a snapshot.
#[derive(Clone, Debug, PartialEq)]
pub struct PlayerSnapshot {
    pub id: u64,
    pub name: String,
    pub plane: String,
    /// World position.
    pub position: [f32; 3],
    /// Orientation quaternion, `[x, y, z, w]`.
    pub rotation: [f32; 4],
    /// World velocity (m/s), for interpolation.
    pub velocity: [f32; 3],
}

/// A message sent from a client to the server.
#[derive(Clone, Debug, PartialEq)]
pub enum ClientMessage {
    /// First message after connecting.
    Join { name: String, plane: String },
    /// Periodic state update of the client's own aircraft.
    State {
        position: [f32; 3],
        rotation: [f32; 4],
        velocity: [f32; 3],
    },
    /// Graceful disconnect.
    Leave,
    /// Report that we hit another player.
    Hit {
        target: u64,
        section: u8,
        damage: f32,
    },
}

/// A message sent from the server to a client.
#[derive(Clone, Debug, PartialEq)]
pub enum ServerMessage {
    /// Accepted; carries the assigned id and the map/gamemode in play.
    Welcome {
        id: u64,
        map: String,
        gamemode: String,
    },
    /// The planes the server has, as `(id, plane.conf text)` pairs. Sent right
    /// after [`ServerMessage::Welcome`] so clients can load every plane in use.
    Planes { planes: Vec<(String, String)> },
    /// The server's pilot crew configuration, as `crew.conf` text. Sent after
    /// [`ServerMessage::Planes`] so clients use this server's pilot model
    /// (g-tolerance, blackout/redout, stamina). An empty string means "keep the
    /// client defaults".
    Crew { config: String },
    /// The full set of players, sent every tick.
    Snapshot { players: Vec<PlayerSnapshot> },
    /// A player disconnected.
    PlayerLeft { id: u64 },
    /// A player was hit (relayed to everyone but the shooter).
    Hit {
        target: u64,
        section: u8,
        damage: f32,
    },
    /// The connection was rejected (bad version, server full, ...).
    Error { reason: String },
}

/// Error parsing a wire message.
#[derive(Debug, Clone, PartialEq)]
pub struct ProtocolError(pub String);

impl core::fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "protocol error: {}", self.0)
    }
}

/// Names/plane ids may not contain the field separators.
pub fn sanitize_field(value: &str) -> String {
    value.replace(['\t', '\n', '\r'], " ")
}

/// Escape a multi-line config so it fits in a single tab-separated field.
pub fn escape_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => {}
            '\t' => out.push_str("\\t"),
            other => out.push(other),
        }
    }
    out
}

/// Reverse of [`escape_text`].
pub fn unescape_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('\\') => out.push('\\'),
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

fn fmt3(value: f32) -> String {
    format!("{value:.3}")
}

fn parse_f32(text: &str) -> Result<f32, ProtocolError> {
    text.parse::<f32>()
        .map_err(|_| ProtocolError(format!("bad number '{text}'")))
}

fn parse_u64(text: &str) -> Result<u64, ProtocolError> {
    text.parse::<u64>()
        .map_err(|_| ProtocolError(format!("bad id '{text}'")))
}

fn parse_vec3(fields: &[&str]) -> Result<[f32; 3], ProtocolError> {
    if fields.len() != 3 {
        return Err(ProtocolError("expected 3 components".into()));
    }
    Ok([
        parse_f32(fields[0])?,
        parse_f32(fields[1])?,
        parse_f32(fields[2])?,
    ])
}

fn parse_vec4(fields: &[&str]) -> Result<[f32; 4], ProtocolError> {
    if fields.len() != 4 {
        return Err(ProtocolError("expected 4 components".into()));
    }
    Ok([
        parse_f32(fields[0])?,
        parse_f32(fields[1])?,
        parse_f32(fields[2])?,
        parse_f32(fields[3])?,
    ])
}

impl ClientMessage {
    /// Serialize without the trailing newline.
    pub fn to_line(&self) -> String {
        match self {
            ClientMessage::Join { name, plane } => format!(
                "JOIN\t{}\t{}\t{}",
                PROTOCOL_VERSION,
                sanitize_field(name),
                sanitize_field(plane)
            ),
            ClientMessage::State {
                position,
                rotation,
                velocity,
            } => format!(
                "STATE\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                fmt3(position[0]),
                fmt3(position[1]),
                fmt3(position[2]),
                fmt3(rotation[0]),
                fmt3(rotation[1]),
                fmt3(rotation[2]),
                fmt3(rotation[3]),
                fmt3(velocity[0]),
                fmt3(velocity[1]),
                fmt3(velocity[2]),
            ),
            ClientMessage::Leave => "LEAVE".to_string(),
            ClientMessage::Hit {
                target,
                section,
                damage,
            } => format!("HIT\t{target}\t{section}\t{}", fmt3(*damage)),
        }
    }

    pub fn parse(line: &str) -> Result<Self, ProtocolError> {
        let fields: Vec<&str> = line.trim_end().split('\t').collect();
        match fields.first().copied() {
            Some("JOIN") => {
                if fields.len() != 4 {
                    return Err(ProtocolError("JOIN wants 4 fields".into()));
                }
                let version = parse_u64(fields[1])? as u32;
                if version != PROTOCOL_VERSION {
                    return Err(ProtocolError(format!(
                        "protocol version {version} != {PROTOCOL_VERSION}"
                    )));
                }
                Ok(ClientMessage::Join {
                    name: fields[2].to_string(),
                    plane: fields[3].to_string(),
                })
            }
            Some("STATE") => {
                if fields.len() != 11 {
                    return Err(ProtocolError("STATE wants 11 fields".into()));
                }
                Ok(ClientMessage::State {
                    position: parse_vec3(&fields[1..4])?,
                    rotation: parse_vec4(&fields[4..8])?,
                    velocity: parse_vec3(&fields[8..11])?,
                })
            }
            Some("LEAVE") => Ok(ClientMessage::Leave),
            Some("HIT") => {
                if fields.len() != 4 {
                    return Err(ProtocolError("HIT wants 4 fields".into()));
                }
                Ok(ClientMessage::Hit {
                    target: parse_u64(fields[1])?,
                    section: fields[2]
                        .parse()
                        .map_err(|_| ProtocolError("bad section".into()))?,
                    damage: parse_f32(fields[3])?,
                })
            }
            other => Err(ProtocolError(format!("unknown message {other:?}"))),
        }
    }
}

impl ServerMessage {
    /// Serialize without the trailing newline.
    pub fn to_line(&self) -> String {
        match self {
            ServerMessage::Welcome { id, map, gamemode } => format!(
                "WELCOME\t{id}\t{}\t{}",
                sanitize_field(map),
                sanitize_field(gamemode)
            ),
            ServerMessage::Planes { planes } => {
                let mut out = format!("PLANES\t{}", planes.len());
                for (id, text) in planes {
                    out.push('\t');
                    out.push_str(&sanitize_field(id));
                    out.push('\t');
                    out.push_str(&escape_text(text));
                }
                out
            }
            ServerMessage::Crew { config } => {
                format!("CREW\t{}", escape_text(config))
            }
            ServerMessage::Snapshot { players } => {
                let mut out = format!("SNAPSHOT\t{}", players.len());
                for player in players {
                    out.push_str(&format!(
                        "\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                        player.id,
                        sanitize_field(&player.name),
                        sanitize_field(&player.plane),
                        fmt3(player.position[0]),
                        fmt3(player.position[1]),
                        fmt3(player.position[2]),
                        fmt3(player.rotation[0]),
                        fmt3(player.rotation[1]),
                        fmt3(player.rotation[2]),
                        fmt3(player.rotation[3]),
                        fmt3(player.velocity[0]),
                        fmt3(player.velocity[1]),
                        fmt3(player.velocity[2]),
                    ));
                }
                out
            }
            ServerMessage::PlayerLeft { id } => format!("PLAYER_LEFT\t{id}"),
            ServerMessage::Hit {
                target,
                section,
                damage,
            } => format!("HIT\t{target}\t{section}\t{}", fmt3(*damage)),
            ServerMessage::Error { reason } => format!("ERROR\t{}", sanitize_field(reason)),
        }
    }

    pub fn parse(line: &str) -> Result<Self, ProtocolError> {
        let fields: Vec<&str> = line.trim_end().split('\t').collect();
        match fields.first().copied() {
            Some("WELCOME") => {
                if fields.len() != 4 {
                    return Err(ProtocolError("WELCOME wants 4 fields".into()));
                }
                Ok(ServerMessage::Welcome {
                    id: parse_u64(fields[1])?,
                    map: fields[2].to_string(),
                    gamemode: fields[3].to_string(),
                })
            }
            Some("PLANES") => {
                if fields.len() < 2 {
                    return Err(ProtocolError("PLANES wants a count".into()));
                }
                let count = parse_u64(fields[1])? as usize;
                let mut planes = Vec::with_capacity(count);
                let mut index = 2;
                for _ in 0..count {
                    if fields.len() < index + 2 {
                        return Err(ProtocolError("PLANES truncated".into()));
                    }
                    planes.push((fields[index].to_string(), unescape_text(fields[index + 1])));
                    index += 2;
                }
                Ok(ServerMessage::Planes { planes })
            }
            Some("CREW") => {
                if fields.len() != 2 {
                    return Err(ProtocolError("CREW wants 2 fields".into()));
                }
                Ok(ServerMessage::Crew {
                    config: unescape_text(fields[1]),
                })
            }
            Some("SNAPSHOT") => {
                if fields.len() < 2 {
                    return Err(ProtocolError("SNAPSHOT wants a count".into()));
                }
                let count = parse_u64(fields[1])? as usize;
                let mut players = Vec::with_capacity(count);
                let mut index = 2;
                for _ in 0..count {
                    if fields.len() < index + 13 {
                        return Err(ProtocolError("SNAPSHOT truncated".into()));
                    }
                    players.push(PlayerSnapshot {
                        id: parse_u64(fields[index])?,
                        name: fields[index + 1].to_string(),
                        plane: fields[index + 2].to_string(),
                        position: parse_vec3(&fields[index + 3..index + 6])?,
                        rotation: parse_vec4(&fields[index + 6..index + 10])?,
                        velocity: parse_vec3(&fields[index + 10..index + 13])?,
                    });
                    index += 13;
                }
                Ok(ServerMessage::Snapshot { players })
            }
            Some("PLAYER_LEFT") => {
                if fields.len() != 2 {
                    return Err(ProtocolError("PLAYER_LEFT wants 2 fields".into()));
                }
                Ok(ServerMessage::PlayerLeft {
                    id: parse_u64(fields[1])?,
                })
            }
            Some("HIT") => {
                if fields.len() != 4 {
                    return Err(ProtocolError("HIT wants 4 fields".into()));
                }
                Ok(ServerMessage::Hit {
                    target: parse_u64(fields[1])?,
                    section: fields[2]
                        .parse()
                        .map_err(|_| ProtocolError("bad section".into()))?,
                    damage: parse_f32(fields[3])?,
                })
            }
            Some("ERROR") => Ok(ServerMessage::Error {
                reason: fields.get(1).copied().unwrap_or("unknown").to_string(),
            }),
            other => Err(ProtocolError(format!("unknown message {other:?}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip_client(message: ClientMessage) {
        let line = message.to_line();
        assert_eq!(ClientMessage::parse(&line).unwrap(), message);
    }

    fn round_trip_server(message: ServerMessage) {
        let line = message.to_line();
        assert_eq!(ServerMessage::parse(&line).unwrap(), message);
    }

    #[test]
    fn client_messages_round_trip() {
        round_trip_client(ClientMessage::Join {
            name: "Wyn".into(),
            plane: "Bf 109 G-6".into(),
        });
        round_trip_client(ClientMessage::State {
            position: [1.0, 2.5, -3.25],
            rotation: [0.0, 0.707, 0.0, 0.707],
            velocity: [10.0, 0.0, -150.0],
        });
        round_trip_client(ClientMessage::Leave);
        round_trip_client(ClientMessage::Hit {
            target: 5,
            section: 2,
            damage: 12.0,
        });
    }

    #[test]
    fn server_messages_round_trip() {
        round_trip_server(ServerMessage::Welcome {
            id: 7,
            map: "Training".into(),
            gamemode: "free_flight".into(),
        });
        round_trip_server(ServerMessage::Snapshot {
            players: vec![
                PlayerSnapshot {
                    id: 1,
                    name: "A".into(),
                    plane: "F4U-4 Corsair".into(),
                    position: [0.0, 1000.0, 0.0],
                    rotation: [0.0, 0.0, 0.0, 1.0],
                    velocity: [0.0, 0.0, -150.0],
                },
                PlayerSnapshot {
                    id: 2,
                    name: "B".into(),
                    plane: "Spitfire F Mk IXc".into(),
                    position: [100.0, 1100.0, 50.0],
                    rotation: [0.0, 0.0, 0.0, 1.0],
                    velocity: [5.0, 0.0, -140.0],
                },
            ],
        });
        round_trip_server(ServerMessage::PlayerLeft { id: 3 });
        round_trip_server(ServerMessage::Hit {
            target: 5,
            section: 2,
            damage: 12.0,
        });
        round_trip_server(ServerMessage::Planes {
            planes: vec![
                (
                    "f4u-4-corsair".into(),
                    "name = F4U-4 Corsair\nmass = 6000\n".into(),
                ),
                (
                    "bf-109-g6".into(),
                    "name = Bf 109 G-6\n[gun 0]\nname = MG 151\n".into(),
                ),
            ],
        });
        round_trip_server(ServerMessage::Error {
            reason: "server full".into(),
        });
        round_trip_server(ServerMessage::Crew {
            config: "g_tolerance = 5.0\nnegative_g_tolerance = -2.5\n".into(),
        });
    }

    #[test]
    fn rejects_wrong_version() {
        assert!(ClientMessage::parse("JOIN\t999\tWyn\tF4U-4 Corsair").is_err());
    }
}
