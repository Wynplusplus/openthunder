//! War Thunder Air-RB-style spotting.
//!
//! Aircraft are only *drawn* within a render distance, and an enemy only gets a
//! **marker** once it is *spotted*: inside the view cone at long range (WT's
//! "Keen Vision"), or all-round when close ("Awareness"). Friendlies are always
//! marked, enemies are marked only when spotted, and everyone else is neutral.
//!
//! The ranges live in [`Spotting`], which the server can tune through its crew
//! config, so a server can run tight or generous spotting.

use std::collections::{HashMap, HashSet};

use bevy::prelude::*;
use openthunder::protocol::NO_TEAM;

use crate::aircraft::PlayerControlled;
use crate::camera::ChaseCamera;
use crate::match_client::MatchClient;
use crate::net::RemotePlayer;

/// Spotting ranges, in metres (angles in degrees).
#[derive(Resource, Clone, Copy, Debug)]
pub struct Spotting {
    /// Beyond this, an aircraft is not drawn at all.
    pub render_distance: f32,
    /// Enemies are spotted within this range when inside the view cone.
    pub detection_range: f32,
    /// Enemies are always spotted within this range (all-round awareness).
    pub awareness_range: f32,
    /// Half-angle of the view cone used for long-range spotting.
    pub view_cone_deg: f32,
}

impl Default for Spotting {
    fn default() -> Self {
        Self {
            render_distance: 9000.0,
            detection_range: 7000.0,
            awareness_range: 1500.0,
            view_cone_deg: 25.0,
        }
    }
}

impl Spotting {
    /// Apply a crew config (`key = value`); unknown keys are ignored.
    pub fn apply_text(&mut self, text: &str) {
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let Ok(value) = value.trim().parse::<f32>() else {
                continue;
            };
            match key.trim() {
                "render_distance" => self.render_distance = value,
                "detection_range" => self.detection_range = value,
                "awareness_range" => self.awareness_range = value,
                "view_cone_deg" => self.view_cone_deg = value,
                _ => {}
            }
        }
    }

    /// Cosine of the view-cone half-angle.
    pub(crate) fn view_cone_cos(&self) -> f32 {
        self.view_cone_deg.to_radians().cos()
    }
}

/// How a marker is coloured.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum MarkerKind {
    Friendly,
    Enemy,
    Neutral,
}

impl MarkerKind {
    fn color(self) -> Color {
        match self {
            MarkerKind::Friendly => Color::srgb(0.55, 0.80, 1.0),
            MarkerKind::Enemy => Color::srgb(1.0, 0.42, 0.42),
            MarkerKind::Neutral => Color::srgb(0.9, 0.9, 0.9),
        }
    }
}

/// A UI marker for one remote aircraft.
#[derive(Component)]
struct Marker;

/// Player id -> marker entity.
#[derive(Resource, Default)]
struct Markers(HashMap<u64, Entity>);

pub struct SpottingPlugin;

impl Plugin for SpottingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Spotting>()
            .init_resource::<Markers>()
            .add_systems(Update, (sync_markers, update_spotting).chain());
    }
}

/// Spawn a marker per remote player, and clean up when they leave.
fn sync_markers(
    mut commands: Commands,
    mut markers: ResMut<Markers>,
    remote: Query<&RemotePlayer>,
) {
    let ids: HashSet<u64> = remote.iter().map(|player| player.id).collect();

    for id in &ids {
        if !markers.0.contains_key(id) {
            let entity = commands
                .spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        display: Display::None,
                        ..default()
                    },
                    Text::new(""),
                    TextFont {
                        font_size: FontSize::Px(15.0),
                        ..default()
                    },
                    TextColor(Color::WHITE),
                    GlobalZIndex(6),
                    Marker,
                ))
                .id();
            markers.0.insert(*id, entity);
        }
    }

    markers.0.retain(|id, entity| {
        if ids.contains(id) {
            true
        } else {
            commands.entity(*entity).despawn();
            false
        }
    });
}

/// Decide how a remote aircraft is marked and whether it is spotted.
///
/// Friendlies are always spotted within detection range; enemies need the view
/// cone at long range or mere proximity; without teams everyone is neutral.
fn classify(
    team: u8,
    our_team: u8,
    team_mode: bool,
    distance: f32,
    in_cone: bool,
    spotting: &Spotting,
) -> (MarkerKind, bool) {
    if team_mode && team != NO_TEAM {
        if team == our_team {
            (MarkerKind::Friendly, distance < spotting.detection_range)
        } else {
            (
                MarkerKind::Enemy,
                distance < spotting.awareness_range
                    || (distance < spotting.detection_range && in_cone),
            )
        }
    } else {
        (MarkerKind::Neutral, distance < spotting.detection_range)
    }
}

/// Cull distant aircraft and place a marker on everyone we have spotted.
#[allow(clippy::type_complexity)]
fn update_spotting(
    spotting: Res<Spotting>,
    match_state: Res<MatchClient>,
    local: Query<&Transform, With<PlayerControlled>>,
    camera: Query<(&Camera, &GlobalTransform), With<ChaseCamera>>,
    mut remote: Query<(&RemotePlayer, &Transform, &mut Visibility), Without<Marker>>,
    markers: Res<Markers>,
    mut marker_nodes: Query<(&Marker, &mut Node, &mut Text, &mut TextColor), Without<RemotePlayer>>,
) {
    let Ok(local_transform) = local.single() else {
        return;
    };
    let Ok((camera, camera_transform)) = camera.single() else {
        return;
    };
    let local_position = local_transform.translation;
    let camera_forward = camera_transform.forward();
    let view_cone = spotting.view_cone_cos();

    for (player, transform, mut visibility) in &mut remote {
        let to_target = transform.translation - local_position;
        let distance = to_target.length();
        let direction = to_target / distance.max(0.001);

        // Draw the model within render distance.
        *visibility = if distance < spotting.render_distance {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };

        // Decide whether we have spotted it, and as what.
        let in_cone = camera_forward.dot(direction) > view_cone;
        let (kind, spotted) = classify(
            player.team,
            match_state.team,
            match_state.is_team_mode(),
            distance,
            in_cone,
            &spotting,
        );

        let Some(marker_entity) = markers.0.get(&player.id) else {
            continue;
        };
        let Ok((_, mut node, mut text, mut color)) = marker_nodes.get_mut(*marker_entity) else {
            continue;
        };

        // Only on-screen, in-front targets get a marker.
        if !spotted || camera_forward.dot(direction) <= 0.0 {
            node.display = Display::None;
            continue;
        }
        let Ok(screen) = camera.world_to_viewport(camera_transform, transform.translation) else {
            node.display = Display::None;
            continue;
        };

        node.left = px(screen.x + 10.0);
        node.top = px(screen.y + 6.0);
        node.display = Display::Flex;
        **text = format!("{}  {:.1} km", player.name, distance / 1000.0);
        *color = kind.color().into();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crew_config_overrides_spotting_ranges() {
        let mut spotting = Spotting::default();
        spotting.apply_text(
            "# server\nrender_distance = 12000\ndetection_range = 5000\nawareness_range = 900\nview_cone_deg = 30\n",
        );
        assert_eq!(spotting.render_distance, 12000.0);
        assert_eq!(spotting.detection_range, 5000.0);
        assert_eq!(spotting.awareness_range, 900.0);
        assert_eq!(spotting.view_cone_deg, 30.0);
    }

    #[test]
    fn an_empty_config_keeps_the_defaults() {
        let mut spotting = Spotting::default();
        spotting.apply_text("");
        assert_eq!(
            spotting.detection_range,
            Spotting::default().detection_range
        );
    }

    #[test]
    fn friendlies_are_always_spotted_enemies_need_the_cone() {
        let spotting = Spotting::default();
        // Friendly at 6 km, outside the cone: still spotted.
        assert_eq!(
            classify(0, 0, true, 6000.0, false, &spotting),
            (MarkerKind::Friendly, true)
        );
        // Enemy at 6 km outside the cone: not spotted.
        assert_eq!(
            classify(1, 0, true, 6000.0, false, &spotting),
            (MarkerKind::Enemy, false)
        );
        // Enemy at 6 km inside the cone: spotted.
        assert!(classify(1, 0, true, 6000.0, true, &spotting).1);
        // Enemy at 1 km behind us: spotted by all-round awareness.
        assert!(classify(1, 0, true, 1000.0, false, &spotting).1);
        // Beyond detection range: never spotted.
        assert!(!classify(1, 0, true, 9000.0, true, &spotting).1);
    }

    #[test]
    fn without_teams_everyone_is_neutral() {
        let spotting = Spotting::default();
        assert_eq!(
            classify(0, NO_TEAM, false, 1000.0, true, &spotting).0,
            MarkerKind::Neutral
        );
    }
}
