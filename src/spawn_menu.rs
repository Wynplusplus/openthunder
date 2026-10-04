//! Pre-spawn plane selection menu.
//!
//! The game starts (and "Change plane" returns) here instead of spawning an
//! aircraft immediately. The player picks a plane from the registry — which
//! includes any planes the server sent — and confirms to spawn.
//!
//! The world keeps simulating while the menu is up; there is simply no player
//! aircraft until you spawn.

use bevy::prelude::*;
use openthunder::protocol::ClientMessage;
use openthunder::settings::Settings;

use crate::aircraft::{Aircraft, AircraftRegistry, PlayerControlled, spawn_aircraft};
use crate::damage::DamageModel;
use crate::net::NetClient;

/// True while the player is choosing a plane (no aircraft spawned yet).
#[derive(Resource, Default)]
pub struct SpawnState {
    pub choosing: bool,
    pub selected: usize,
}

#[derive(Component)]
struct SpawnRoot;

#[derive(Component)]
struct SpawnItem(usize);

pub struct SpawnMenuPlugin;

impl Plugin for SpawnMenuPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SpawnState>()
            .add_systems(Startup, (init_spawn, spawn_menu_ui).chain())
            .add_systems(Update, (spawn_menu_input, update_spawn_ui));
    }
}

/// Pick the initial selection from `--plane` / the saved settings.
fn init_spawn(mut state: ResMut<SpawnState>, registry: Res<AircraftRegistry>) {
    let requested = plane_arg().unwrap_or_else(|| Settings::load_or_create().plane);
    state.selected = registry
        .specs
        .iter()
        .position(|spec| spec.name == requested)
        .unwrap_or(0);
    state.choosing = true;
}

fn spawn_menu_ui(mut commands: Commands, registry: Res<AircraftRegistry>) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                row_gap: px(8),
                ..default()
            },
            BackgroundColor(Color::srgba(0.05, 0.06, 0.09, 0.92)),
            GlobalZIndex(20),
            SpawnRoot,
        ))
        .with_children(|parent| {
            parent.spawn((
                Text::new("Choose your aircraft"),
                TextFont {
                    font_size: FontSize::Px(32.0),
                    ..default()
                },
                TextColor(Color::srgb(0.9, 0.95, 1.0)),
            ));
            parent.spawn((
                Text::new("Up/Down to select, Enter to spawn"),
                TextFont {
                    font_size: FontSize::Px(16.0),
                    ..default()
                },
                TextColor(Color::srgb(0.6, 0.65, 0.72)),
            ));
            for (index, spec) in registry.specs.iter().enumerate() {
                parent.spawn((
                    Text::new(spec.name.clone()),
                    TextFont {
                        font_size: FontSize::Px(24.0),
                        ..default()
                    },
                    TextColor(Color::WHITE),
                    SpawnItem(index),
                ));
            }
        });
}

fn spawn_menu_input(
    keys: Res<ButtonInput<KeyCode>>,
    registry: Res<AircraftRegistry>,
    client: Res<NetClient>,
    world: Res<crate::world::WorldKind>,
    mut state: ResMut<SpawnState>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    existing: Query<Entity, With<PlayerControlled>>,
) {
    if !state.choosing {
        return;
    }
    let count = registry.specs.len();
    if count == 0 {
        return;
    }

    if keys.just_pressed(KeyCode::ArrowUp) || keys.just_pressed(KeyCode::KeyW) {
        state.selected = state.selected.saturating_sub(1);
    }
    if keys.just_pressed(KeyCode::ArrowDown) || keys.just_pressed(KeyCode::KeyS) {
        state.selected = (state.selected + 1).min(count - 1);
    }
    if !(keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::Space)) {
        return;
    }

    // Replace any existing player aircraft.
    for entity in &existing {
        commands.entity(entity).despawn();
    }

    let spec = registry.specs[state.selected].clone();
    let mut aircraft = Aircraft::new(spec.clone());
    // Start on the runway, ready to take off.
    let mut transform = Transform::IDENTITY;
    let mut damage = DamageModel::new(spec.max_health);
    let ground = crate::world::terrain_height(*world, 0.0, 0.0);
    crate::flight::respawn_on_runway(&mut transform, &mut aircraft, &mut damage, ground);
    let entity = spawn_aircraft(
        &mut commands,
        &mut meshes,
        &mut materials,
        aircraft,
        transform,
    );
    commands.entity(entity).insert(PlayerControlled);
    info!("Spawning {}", spec.name);

    // Tell the server which plane we are flying now.
    if let Some(outgoing) = &client.outgoing {
        let name = Settings::load_or_create().player_name;
        let _ = outgoing.send(ClientMessage::Join {
            name,
            plane: spec.name.clone(),
        });
    }

    state.choosing = false;
}

fn update_spawn_ui(
    state: Res<SpawnState>,
    mut root: Query<&mut Node, With<SpawnRoot>>,
    mut items: Query<(&SpawnItem, &mut TextColor)>,
) {
    if let Ok(mut node) = root.single_mut() {
        node.display = if state.choosing {
            Display::Flex
        } else {
            Display::None
        };
    }
    for (item, mut color) in &mut items {
        *color = if state.choosing && item.0 == state.selected {
            TextColor(Color::srgb(1.0, 0.84, 0.25))
        } else {
            TextColor(Color::WHITE)
        };
    }
}

fn plane_arg() -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    args.iter()
        .position(|arg| arg == "--plane")
        .and_then(|index| args.get(index + 1).cloned())
}
