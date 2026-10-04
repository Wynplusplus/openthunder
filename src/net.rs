//! Multiplayer client.
//!
//! When a server is configured (via `settings.conf` or `--server host:port`),
//! this connects to a dedicated server, streams the local aircraft's state and
//! renders the other players' aircraft. In single-player it does nothing.
//!
//! Networking runs on background threads so the render loop never blocks; state
//! crosses the thread boundary through channels.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::sync::{Arc, Mutex};

use bevy::prelude::*;
use openthunder::plane_config::PlaneConfig;
use openthunder::planes;
use openthunder::protocol::{ClientMessage, PlayerSnapshot, ServerMessage};
use openthunder::settings::Settings;

use crate::aircraft::{
    Aircraft, AircraftRegistry, AircraftSpec, PlayerControlled, spawn_aircraft_model,
};
use crate::damage::{AircraftPart, DamageModel};
use crate::pilot::CrewSkills;

/// How often the local state is sent to the server.
const SEND_HZ: f32 = 15.0;

/// Connection state, shown on the HUD.
#[derive(Clone, Debug, Default)]
pub enum ConnectionStatus {
    #[default]
    Singleplayer,
    Connecting(String),
    Connected {
        id: u64,
        map: String,
        gamemode: String,
    },
    Disconnected(String),
}

impl ConnectionStatus {
    pub fn label(&self) -> String {
        match self {
            ConnectionStatus::Singleplayer => "Single-player".to_string(),
            ConnectionStatus::Connecting(addr) => format!("Connecting to {addr}..."),
            ConnectionStatus::Connected { map, gamemode, .. } => {
                format!("Online: {map} ({gamemode})")
            }
            ConnectionStatus::Disconnected(reason) => format!("Disconnected: {reason}"),
        }
    }
}

/// Channel handles + shared status for the network threads.
#[derive(Resource)]
pub struct NetClient {
    pub status: Arc<Mutex<ConnectionStatus>>,
    pub outgoing: Option<Sender<ClientMessage>>,
    /// `Receiver` is not `Sync`, so it is wrapped for use as a resource.
    pub incoming: Option<Mutex<Receiver<ServerMessage>>>,
}

impl Default for NetClient {
    fn default() -> Self {
        Self {
            status: Arc::new(Mutex::new(ConnectionStatus::Singleplayer)),
            outgoing: None,
            incoming: None,
        }
    }
}

impl NetClient {
    /// The local player's server-assigned id, if connected.
    pub fn local_id(&self) -> Option<u64> {
        match &*self.status.lock().unwrap() {
            ConnectionStatus::Connected { id, .. } => Some(*id),
            _ => None,
        }
    }
}

/// A remote player's aircraft.
#[derive(Component)]
pub struct RemotePlayer {
    pub id: u64,
    pub target_position: Vec3,
    pub target_rotation: Quat,
}

pub struct NetPlugin;

impl Plugin for NetPlugin {
    fn build(&self, app: &mut App) {
        let settings = Settings::load_or_create();
        let address =
            server_arg().or_else(|| settings.multiplayer().then(|| settings.server.clone()));
        let name = name_arg().unwrap_or(settings.player_name.clone());
        let plane = settings.plane.clone();

        let mut client = NetClient::default();
        if let Some(address) = address {
            *client.status.lock().unwrap() = ConnectionStatus::Connecting(address.clone());
            let (out_tx, out_rx) = channel::<ClientMessage>();
            let (in_tx, in_rx) = channel::<ServerMessage>();
            let status = Arc::clone(&client.status);
            std::thread::spawn(move || connect_thread(address, name, plane, out_rx, in_tx, status));
            client.outgoing = Some(out_tx);
            client.incoming = Some(Mutex::new(in_rx));
        }

        app.insert_resource(client).add_systems(
            Update,
            (send_local_state, receive_snapshots, update_remote_players),
        );
    }
}

// ---------------------------------------------------------------------------
// Background threads
// ---------------------------------------------------------------------------

fn connect_thread(
    address: String,
    name: String,
    plane: String,
    out_rx: Receiver<ClientMessage>,
    in_tx: Sender<ServerMessage>,
    status: Arc<Mutex<ConnectionStatus>>,
) {
    let stream = match TcpStream::connect(&address) {
        Ok(stream) => stream,
        Err(err) => {
            *status.lock().unwrap() = ConnectionStatus::Disconnected(format!("{address}: {err}"));
            return;
        }
    };
    let _ = stream.set_nodelay(true);
    let read_stream = match stream.try_clone() {
        Ok(stream) => stream,
        Err(err) => {
            *status.lock().unwrap() = ConnectionStatus::Disconnected(format!("{err}"));
            return;
        }
    };
    let mut writer = stream;

    if let Err(err) = send_line(&mut writer, &ClientMessage::Join { name, plane }) {
        *status.lock().unwrap() = ConnectionStatus::Disconnected(format!("{err}"));
        return;
    }

    // Reader thread.
    {
        let in_tx = in_tx.clone();
        let status = Arc::clone(&status);
        std::thread::spawn(move || {
            let reader = BufReader::new(read_stream);
            for line in reader.lines() {
                let Ok(line) = line else { break };
                match ServerMessage::parse(&line) {
                    Ok(message) => {
                        if let ServerMessage::Welcome {
                            id, map, gamemode, ..
                        } = &message
                        {
                            *status.lock().unwrap() = ConnectionStatus::Connected {
                                id: *id,
                                map: map.clone(),
                                gamemode: gamemode.clone(),
                            };
                        }
                        if let ServerMessage::Error { reason } = &message {
                            *status.lock().unwrap() =
                                ConnectionStatus::Disconnected(reason.clone());
                        }
                        if in_tx.send(message).is_err() {
                            break;
                        }
                    }
                    Err(err) => eprintln!("[net] bad message: {err}"),
                }
            }
            *status.lock().unwrap() = ConnectionStatus::Disconnected("connection closed".into());
        });
    }

    // Writer thread.
    std::thread::spawn(move || {
        while let Ok(message) = out_rx.recv() {
            if send_line(&mut writer, &message).is_err() {
                break;
            }
        }
    });
}

fn send_line(stream: &mut TcpStream, message: &ClientMessage) -> std::io::Result<()> {
    stream.write_all(message.to_line().as_bytes())?;
    stream.write_all(b"\n")?;
    stream.flush()
}

// ---------------------------------------------------------------------------
// Systems
// ---------------------------------------------------------------------------

fn send_local_state(
    time: Res<Time>,
    client: Res<NetClient>,
    mut accumulator: Local<f32>,
    query: Query<(&Transform, &Aircraft), With<PlayerControlled>>,
) {
    let Some(outgoing) = &client.outgoing else {
        return;
    };
    let Ok((transform, aircraft)) = query.single() else {
        return;
    };
    *accumulator += time.delta_secs();
    if *accumulator < 1.0 / SEND_HZ {
        return;
    }
    *accumulator = 0.0;

    let rotation = transform.rotation;
    let _ = outgoing.send(ClientMessage::State {
        position: transform.translation.to_array(),
        rotation: [rotation.x, rotation.y, rotation.z, rotation.w],
        velocity: aircraft.velocity.to_array(),
    });
}

fn receive_snapshots(
    mut commands: Commands,
    client: Res<NetClient>,
    mut crew: ResMut<CrewSkills>,
    mut match_state: ResMut<crate::match_client::MatchClient>,
    mut registry: ResMut<AircraftRegistry>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut remote: Query<(Entity, &mut RemotePlayer, &mut DamageModel), Without<PlayerControlled>>,
    mut local: Query<&mut DamageModel, (With<PlayerControlled>, Without<RemotePlayer>)>,
    mut player: Query<&mut Aircraft, (With<PlayerControlled>, Without<RemotePlayer>)>,
) {
    let Some(incoming) = &client.incoming else {
        return;
    };
    let incoming = incoming.lock().unwrap();
    let local_id = client.local_id();

    loop {
        match incoming.try_recv() {
            Ok(ServerMessage::Snapshot { players }) => {
                if let Some(id) = local_id {
                    if let Some(me) = players.iter().find(|player| player.id == id) {
                        match_state.kills = me.kills;
                        match_state.deaths = me.deaths;
                    }
                }
                apply_snapshot(
                    &mut commands,
                    &registry,
                    &mut meshes,
                    &mut materials,
                    &mut remote,
                    local_id,
                    &players,
                );
            }
            Ok(ServerMessage::PlayerLeft { id }) => {
                for (entity, player, _) in &mut remote {
                    if player.id == id {
                        commands.entity(entity).despawn();
                    }
                }
            }
            Ok(ServerMessage::Hit {
                target,
                section,
                damage,
            }) => {
                let Some(part) = AircraftPart::from_index(section as usize) else {
                    continue;
                };
                if Some(target) == local_id {
                    if let Ok(mut model) = local.single_mut() {
                        model.apply_damage(part, damage);
                    }
                } else {
                    for (_, player, mut model) in &mut remote {
                        if player.id == target {
                            model.apply_damage(part, damage);
                            break;
                        }
                    }
                }
            }
            Ok(ServerMessage::Planes { planes }) => {
                let specs: Vec<AircraftSpec> = planes
                    .iter()
                    .map(|(id, text)| AircraftSpec::from_config(&PlaneConfig::parse(id, text)))
                    .collect();
                info!(
                    "[net] loaded {} planes from server: {}",
                    specs.len(),
                    specs
                        .iter()
                        .map(|spec| spec.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
                // Adopt the server's version of our own aircraft, if it has one.
                if let Ok(mut aircraft) = player.single_mut() {
                    if let Some(spec) = specs.iter().find(|spec| spec.name == aircraft.spec.name) {
                        aircraft.ammo = spec.guns.iter().map(|gun| gun.ammo).collect();
                        aircraft.fire_timer = vec![0.0; spec.guns.len()];
                        aircraft.spec = spec.clone();
                    }
                }
                registry.specs = specs;
            }
            Ok(ServerMessage::Crew { config }) => {
                *crew = CrewSkills::from_text(&config);
                info!(
                    "[net] applied server crew config: pilot tolerates {:.1} g / {:.1} g",
                    crew.g_tolerance, crew.negative_g_tolerance
                );
            }
            Ok(ServerMessage::Welcome { team, .. }) => {
                match_state.team = team;
            }
            Ok(ServerMessage::Match {
                scores,
                score_limit,
                time_left,
            }) => {
                match_state.scores = scores;
                match_state.score_limit = score_limit;
                match_state.time_left = time_left;
            }
            Ok(ServerMessage::Kill {
                killer,
                victim,
                killer_name,
                victim_name,
            }) => {
                let text = if Some(killer) == local_id {
                    format!("You destroyed {victim_name}")
                } else if Some(victim) == local_id {
                    format!("{killer_name} destroyed you")
                } else {
                    format!("{killer_name} destroyed {victim_name}")
                };
                match_state.push_kill(text);
            }
            Ok(_) => {}
            Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => break,
        }
    }
}

fn apply_snapshot(
    commands: &mut Commands,
    registry: &AircraftRegistry,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    remote: &mut Query<(Entity, &mut RemotePlayer, &mut DamageModel), Without<PlayerControlled>>,
    local_id: Option<u64>,
    players: &[PlayerSnapshot],
) {
    let mut seen: HashMap<u64, ()> = HashMap::new();
    for player in players {
        if Some(player.id) == local_id {
            continue;
        }
        seen.insert(player.id, ());
        let position = Vec3::from_array(player.position);
        let rotation = Quat::from_xyzw(
            player.rotation[0],
            player.rotation[1],
            player.rotation[2],
            player.rotation[3],
        );

        let mut found = false;
        for (_, mut existing, _) in remote.iter_mut() {
            if existing.id == player.id {
                existing.target_position = position;
                existing.target_rotation = rotation;
                found = true;
                break;
            }
        }
        if found {
            continue;
        }

        // New remote player: spawn the shared model.
        let spec = registry
            .get(&player.plane)
            .or_else(|| registry.get(planes::default_plane()))
            .cloned();
        if let Some(spec) = spec {
            let root = commands
                .spawn((
                    Transform::from_translation(position).with_rotation(rotation),
                    Visibility::default(),
                    RemotePlayer {
                        id: player.id,
                        target_position: position,
                        target_rotation: rotation,
                    },
                    DamageModel::new(spec.max_health),
                ))
                .id();
            spawn_aircraft_model(commands, meshes, materials, &spec, root);
            info!("[net] + {} flying {}", player.name, player.plane);
        }
    }

    // Despawn players that disappeared from the snapshot.
    for (entity, player, _) in remote.iter_mut() {
        if !seen.contains_key(&player.id) {
            commands.entity(entity).despawn();
        }
    }
}

fn update_remote_players(time: Res<Time>, mut query: Query<(&mut Transform, &RemotePlayer)>) {
    let dt = time.delta_secs();
    let blend = (1.0 - (-12.0 * dt).exp()).clamp(0.0, 1.0);
    for (mut transform, player) in &mut query {
        transform.translation = transform.translation.lerp(player.target_position, blend);
        transform.rotation = transform.rotation.slerp(player.target_rotation, blend);
    }
}

// ---------------------------------------------------------------------------
// Command line
// ---------------------------------------------------------------------------

fn server_arg() -> Option<String> {
    arg_value("--server").filter(|value| !value.trim().is_empty())
}

fn name_arg() -> Option<String> {
    arg_value("--name").filter(|value| !value.trim().is_empty())
}

fn arg_value(flag: &str) -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    args.iter()
        .position(|arg| arg == flag)
        .and_then(|index| args.get(index + 1).cloned())
}
