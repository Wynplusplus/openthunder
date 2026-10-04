//! Client-side match state for scored online modes (team deathmatch).
//!
//! The server owns the scores. This tracks them for the HUD, reports our own
//! destruction so the server can credit the killer, and respawns us after a
//! short delay — the "you died, you're back in the fight" loop of Air RB.

use bevy::prelude::*;
use openthunder::protocol::{ClientMessage, NO_TEAM};

use crate::aircraft::{Aircraft, PlayerControlled};
use crate::damage::{AircraftPart, DamageModel};
use crate::flight;
use crate::net::NetClient;

/// Team names, indexed by team id.
pub const TEAM_NAMES: [&str; 2] = ["Blue", "Red"];
/// Seconds between being destroyed and respawning.
const RESPAWN_DELAY: f32 = 4.0;
/// How long a kill-feed entry stays on screen.
const FEED_LIFETIME: f32 = 8.0;

/// One line of the kill feed.
#[derive(Clone, Debug)]
pub struct KillFeedEntry {
    pub text: String,
    pub age: f32,
}

/// Match state mirrored from the server, for the HUD.
#[derive(Resource)]
pub struct MatchClient {
    /// Our team, or [`NO_TEAM`].
    pub team: u8,
    pub scores: Vec<u32>,
    pub score_limit: u32,
    pub time_left: f32,
    pub kills: u32,
    pub deaths: u32,
    pub kill_feed: Vec<KillFeedEntry>,
    /// Set once we have told the server we died this life.
    death_reported: bool,
    /// Seconds until we respawn (0 = alive).
    respawn_in: f32,
}

impl Default for MatchClient {
    fn default() -> Self {
        Self {
            team: NO_TEAM,
            scores: Vec::new(),
            score_limit: 0,
            time_left: 0.0,
            kills: 0,
            deaths: 0,
            kill_feed: Vec::new(),
            death_reported: false,
            respawn_in: 0.0,
        }
    }
}

impl MatchClient {
    /// Whether we are in a scored team mode.
    pub fn is_team_mode(&self) -> bool {
        self.team != NO_TEAM
    }

    pub fn team_name(team: u8) -> &'static str {
        TEAM_NAMES.get(team as usize).copied().unwrap_or("None")
    }

    /// Seconds until respawn (0 while alive).
    pub fn respawn_in(&self) -> f32 {
        self.respawn_in
    }

    pub fn push_kill(&mut self, text: String) {
        self.kill_feed.push(KillFeedEntry { text, age: 0.0 });
        if self.kill_feed.len() > 6 {
            self.kill_feed.remove(0);
        }
    }
}

pub struct MatchClientPlugin;

impl Plugin for MatchClientPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MatchClient>()
            .add_systems(Update, (report_death_and_respawn, age_kill_feed));
    }
}

/// When our fuselage is destroyed, tell the server and respawn after a delay.
fn report_death_and_respawn(
    time: Res<Time>,
    client: Res<NetClient>,
    mut match_state: ResMut<MatchClient>,
    mut query: Query<(&mut Transform, &mut Aircraft, &mut DamageModel), With<PlayerControlled>>,
) {
    let Ok((mut transform, mut aircraft, mut damage)) = query.single_mut() else {
        return;
    };
    // Only team modes report deaths and auto-respawn; elsewhere the player
    // respawns manually with the reset key.
    if !match_state.is_team_mode() {
        return;
    }
    let dt = time.delta_secs();

    if match_state.respawn_in > 0.0 {
        match_state.respawn_in = (match_state.respawn_in - dt).max(0.0);
        if match_state.respawn_in == 0.0 {
            flight::respawn(&mut transform, &mut aircraft, &mut damage);
            match_state.death_reported = false;
        }
        return;
    }

    if damage.is_destroyed(AircraftPart::Fuselage) && !match_state.death_reported {
        match_state.death_reported = true;
        match_state.respawn_in = RESPAWN_DELAY;
        if let Some(outgoing) = &client.outgoing {
            let _ = outgoing.send(ClientMessage::Death);
        }
    }
}

fn age_kill_feed(time: Res<Time>, mut match_state: ResMut<MatchClient>) {
    let dt = time.delta_secs();
    for entry in &mut match_state.kill_feed {
        entry.age += dt;
    }
    match_state
        .kill_feed
        .retain(|entry| entry.age < FEED_LIFETIME);
}
