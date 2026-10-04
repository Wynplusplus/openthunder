//! Shootable test targets for free flight, War Thunder test-flight style.
//!
//! In free flight (single-player or a `free_flight` server) a column of ground
//! targets is placed near the runway. Strafe them with your guns: each has a
//! hit box, takes damage, and blows up when destroyed, then respawns after a
//! short delay so you can keep practising. The HUD shows how many you have
//! destroyed. In team modes the targets are cleared away.

use bevy::prelude::*;

use crate::damage::{AircraftPart, DamageModel};
use crate::match_client::MatchClient;
use crate::world::WorldKind;

/// Seconds before a destroyed target comes back.
const RESPAWN_SECS: f32 = 20.0;
/// How long the explosion puff lasts.
const EXPLOSION_SECS: f32 = 0.7;

/// Half-extents of a target's hit box, in its local frame.
#[derive(Component)]
pub struct HitBox(pub Vec3);

/// A shootable ground target.
#[derive(Component)]
pub struct TestTarget {
    pub alive: bool,
    pub respawn_in: f32,
}

/// Where a target lives (for respawn).
#[derive(Component)]
struct TargetHome;

/// Score for the HUD.
#[derive(Resource, Default)]
pub struct TargetScore {
    pub destroyed: u32,
    pub total: u32,
}

/// A brief explosion puff.
#[derive(Component)]
struct Explosion {
    age: f32,
}

/// What the targets were built for, so they are only rebuilt when it changes.
#[derive(Resource, Default)]
struct TargetsBuilt {
    for_world: Option<WorldKind>,
}

pub struct TargetsPlugin;

impl Plugin for TargetsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TargetScore>()
            .init_resource::<TargetsBuilt>()
            .add_systems(
                Update,
                (sync_targets, update_targets, update_explosions).chain(),
            );
    }
}

/// Build the targets for free flight, or clear them for team modes.
fn sync_targets(
    mut commands: Commands,
    world: Res<WorldKind>,
    match_state: Res<MatchClient>,
    mut built: ResMut<TargetsBuilt>,
    mut score: ResMut<TargetScore>,
    existing: Query<Entity, With<TestTarget>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Targets only belong to free flight.
    let wanted = if match_state.is_team_mode() {
        None
    } else {
        Some(*world)
    };
    if built.for_world == wanted {
        return;
    }

    for entity in &existing {
        commands.entity(entity).despawn();
    }
    score.destroyed = 0;
    built.for_world = wanted;
    score.total = match wanted {
        Some(kind) => spawn_targets(
            &mut commands,
            &mut meshes,
            &mut materials,
            kind.ground_level(),
        ),
        None => 0,
    };
}

/// Place the column of tanks and a few fuel tanks.
fn spawn_targets(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    ground: f32,
) -> u32 {
    let tank_body = meshes.add(Cuboid::new(6.0, 2.4, 3.6));
    let tank_turret = meshes.add(Cuboid::new(3.0, 1.2, 3.0));
    let tank_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.30, 0.34, 0.22),
        perceptual_roughness: 0.9,
        ..default()
    });
    let fuel_mesh = meshes.add(Cuboid::new(9.0, 8.0, 9.0));
    let fuel_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.48, 0.45, 0.32),
        perceptual_roughness: 0.8,
        ..default()
    });

    let mut count = 0;

    // A column of tanks marching away from the runway.
    for i in 0..6 {
        let position = Vec3::new(
            -110.0 + i as f32 * 44.0,
            ground + 1.2,
            -700.0 - i as f32 * 55.0,
        );
        let entity = spawn_target(commands, position, Vec3::new(3.0, 1.2, 1.8), 14.0);
        commands.entity(entity).with_children(|parent| {
            parent.spawn((
                Mesh3d(tank_body.clone()),
                MeshMaterial3d(tank_material.clone()),
            ));
            parent.spawn((
                Mesh3d(tank_turret.clone()),
                MeshMaterial3d(tank_material.clone()),
                Transform::from_xyz(0.0, 1.8, 0.0),
            ));
        });
        count += 1;
    }

    // A couple of fat fuel tanks (tougher, bigger bang).
    for i in 0..3 {
        let position = Vec3::new(
            170.0 + i as f32 * 42.0,
            ground + 4.0,
            -520.0 - i as f32 * 70.0,
        );
        let entity = spawn_target(commands, position, Vec3::new(4.5, 4.0, 4.5), 34.0);
        commands.entity(entity).with_children(|parent| {
            parent.spawn((
                Mesh3d(fuel_mesh.clone()),
                MeshMaterial3d(fuel_material.clone()),
            ));
        });
        count += 1;
    }

    count
}

/// Spawn one target root (hit box + health + respawn state).
fn spawn_target(commands: &mut Commands, position: Vec3, half: Vec3, health: f32) -> Entity {
    commands
        .spawn((
            Transform::from_translation(position),
            Visibility::default(),
            DamageModel::new(health),
            TestTarget {
                alive: true,
                respawn_in: 0.0,
            },
            TargetHome,
            HitBox(half),
        ))
        .id()
}

/// Blow up destroyed targets and bring them back after a delay.
fn update_targets(
    time: Res<Time>,
    mut commands: Commands,
    mut score: ResMut<TargetScore>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut targets: Query<
        (
            &mut TestTarget,
            &mut DamageModel,
            &Transform,
            &mut Visibility,
        ),
        With<TargetHome>,
    >,
) {
    let dt = time.delta_secs();
    for (mut target, mut damage, transform, mut visibility) in &mut targets {
        if target.alive {
            if damage.is_destroyed(AircraftPart::Fuselage) {
                target.alive = false;
                target.respawn_in = RESPAWN_SECS;
                *visibility = Visibility::Hidden;
                score.destroyed += 1;
                spawn_explosion(
                    &mut commands,
                    &mut meshes,
                    &mut materials,
                    transform.translation,
                );
            }
        } else {
            target.respawn_in -= dt;
            if target.respawn_in <= 0.0 {
                damage.repair_all();
                *visibility = Visibility::Inherited;
                target.alive = true;
            }
        }
    }
}

/// A quick expanding, fading fireball.
fn spawn_explosion(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    position: Vec3,
) {
    commands.spawn((
        Mesh3d(meshes.add(Sphere::new(1.0).mesh().ico(2).unwrap())),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgba(1.0, 0.55, 0.15, 0.9),
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            ..default()
        })),
        Transform::from_translation(position).with_scale(Vec3::splat(3.0)),
        Explosion { age: 0.0 },
    ));
}

fn update_explosions(
    time: Res<Time>,
    mut commands: Commands,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut explosions: Query<(
        Entity,
        &mut Explosion,
        &mut Transform,
        &MeshMaterial3d<StandardMaterial>,
    )>,
) {
    let dt = time.delta_secs();
    for (entity, mut explosion, mut transform, material) in &mut explosions {
        explosion.age += dt;
        let t = (explosion.age / EXPLOSION_SECS).clamp(0.0, 1.0);
        if t >= 1.0 {
            commands.entity(entity).despawn();
            continue;
        }
        transform.scale = Vec3::splat(3.0 + t * 14.0);
        if let Some(mut mat) = materials.get_mut(&material.0) {
            mat.base_color = Color::srgba(1.0, 0.55 + 0.3 * t, 0.15, 0.9 * (1.0 - t));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::asset::AssetPlugin;

    fn targets_app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(AssetPlugin::default());
        app.init_asset::<Mesh>();
        app.init_asset::<StandardMaterial>();
        app.init_resource::<TargetScore>()
            .init_resource::<TargetsBuilt>()
            .init_resource::<WorldKind>()
            .init_resource::<MatchClient>()
            .add_systems(Update, sync_targets);
        app
    }

    #[test]
    fn free_flight_builds_targets_and_team_modes_clear_them() {
        let mut app = targets_app();

        app.update();
        let total = app.world().resource::<TargetScore>().total;
        assert!(total > 0, "free flight should place test targets");

        let mut query = app.world_mut().query::<&TestTarget>();
        let spawned = query.iter(app.world()).count() as u32;
        assert_eq!(spawned, total, "every target should be a TestTarget entity");

        // Entering a team mode clears them.
        app.world_mut().resource_mut::<MatchClient>().team = 0;
        app.update();
        assert_eq!(app.world().resource::<TargetScore>().total, 0);
        let mut query = app.world_mut().query::<&TestTarget>();
        assert_eq!(query.iter(app.world()).count(), 0);
    }

    #[test]
    fn targets_have_a_hit_box_and_health() {
        let mut app = targets_app();
        app.update();

        let mut query = app
            .world_mut()
            .query::<(&TestTarget, &HitBox, &DamageModel)>();
        let mut seen = 0;
        for (target, hit_box, damage) in query.iter(app.world()) {
            assert!(target.alive);
            assert!(hit_box.0.min_element() > 0.0, "hit box should be non-empty");
            assert!(damage.integrity(AircraftPart::Fuselage) > 0.0);
            seen += 1;
        }
        assert!(seen > 0);
    }
}
