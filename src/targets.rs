//! Shootable test targets for free flight, War Thunder test-flight style.
//!
//! In free flight (single-player or a `free_flight` server) a column of ground
//! targets and a few circling **target planes** are placed near the runway.
//! None of them fight back. Each has a hit box, takes damage, blows up when
//! destroyed (planes tumble down first) and respawns after a short delay. All of
//! them get an Air RB-style marker once spotted, and the HUD keeps a tally.
//!
//! In team modes the targets are cleared away.

use std::collections::{HashMap, HashSet};

use bevy::prelude::*;
use openthunder::plane_config::default_planes;

use crate::aircraft::{AircraftSpec, PlayerControlled, spawn_aircraft_model};
use crate::camera::ChaseCamera;
use crate::damage::{AircraftPart, DamageModel};
use crate::match_client::MatchClient;
use crate::spotting::Spotting;
use crate::world::WorldKind;

/// Seconds before a destroyed target comes back.
const RESPAWN_SECS: f32 = 20.0;
/// How long the explosion puff lasts.
const EXPLOSION_SECS: f32 = 0.7;
/// How fast a dying target plane tumbles.
const FALL_SPIN: f32 = 2.0;

/// Half-extents of a target's hit box, in its local frame.
#[derive(Component)]
pub struct HitBox(pub Vec3);

/// What kind of target this is (for the marker label).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TargetKind {
    Tank,
    Fuel,
    Plane,
}

impl TargetKind {
    fn label(self) -> &'static str {
        match self {
            TargetKind::Tank => "Tank",
            TargetKind::Fuel => "Fuel tank",
            TargetKind::Plane => "Target plane",
        }
    }
}

/// A shootable target.
#[derive(Component)]
pub struct TestTarget {
    pub kind: TargetKind,
    pub alive: bool,
    pub respawn_in: f32,
}

/// Circular flight path of a target plane.
#[derive(Component)]
struct Orbit {
    /// Centre of the circle; `y` is the altitude.
    center: Vec3,
    radius: f32,
    speed: f32,
    angle: f32,
    /// `+1` / `-1` for clockwise / anticlockwise.
    direction: f32,
}

impl Orbit {
    /// Advance by `dt` and return the new `(position, heading)`.
    fn step(&mut self, dt: f32) -> (Vec3, Vec3) {
        self.angle += self.direction * self.speed / self.radius * dt;
        let position = Vec3::new(
            self.center.x + self.angle.cos() * self.radius,
            self.center.y,
            self.center.z + self.angle.sin() * self.radius,
        );
        let heading = Vec3::new(-self.angle.sin(), 0.0, self.angle.cos()) * self.direction;
        (position, heading)
    }
}

/// A destroyed target plane on its way down.
#[derive(Component)]
struct Falling {
    velocity: Vec3,
}

/// A brief explosion puff.
#[derive(Component)]
struct Explosion {
    age: f32,
}

/// A UI marker for one test target.
#[derive(Component)]
struct TargetMarker;

/// Score for the HUD.
#[derive(Resource, Default)]
pub struct TargetScore {
    pub destroyed: u32,
    pub total: u32,
}

/// What the targets were built for, so they are only rebuilt when it changes.
#[derive(Resource, Default)]
struct TargetsBuilt {
    for_world: Option<WorldKind>,
}

/// Test-target entity -> marker entity.
#[derive(Resource, Default)]
struct TargetMarkers(HashMap<Entity, Entity>);

pub struct TargetsPlugin;

impl Plugin for TargetsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TargetScore>()
            .init_resource::<TargetsBuilt>()
            .init_resource::<TargetMarkers>()
            .add_systems(
                Update,
                (
                    sync_targets,
                    update_target_planes,
                    update_falling,
                    update_targets,
                    update_explosions,
                    sync_target_markers,
                    update_target_markers,
                )
                    .chain(),
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
        Some(kind) => {
            let ground = kind.ground_level();
            let mut total =
                spawn_ground_targets(&mut commands, &mut meshes, &mut materials, ground);
            total += spawn_plane_targets(&mut commands, &mut meshes, &mut materials, ground);
            total
        }
        None => 0,
    };
}

/// Place the column of tanks and a few fuel tanks.
fn spawn_ground_targets(
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
        let entity = spawn_ground_target(
            commands,
            position,
            Vec3::new(3.0, 1.2, 1.8),
            14.0,
            TargetKind::Tank,
        );
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
        let entity = spawn_ground_target(
            commands,
            position,
            Vec3::new(4.5, 4.0, 4.5),
            34.0,
            TargetKind::Fuel,
        );
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

/// Spawn one ground target root.
fn spawn_ground_target(
    commands: &mut Commands,
    position: Vec3,
    half: Vec3,
    health: f32,
    kind: TargetKind,
) -> Entity {
    commands
        .spawn((
            Transform::from_translation(position),
            Visibility::default(),
            DamageModel::new(health),
            TestTarget {
                kind,
                alive: true,
                respawn_in: 0.0,
            },
            HitBox(half),
        ))
        .id()
}

/// A few target planes circling on fixed orbits. They do not fight back.
fn spawn_plane_targets(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    ground: f32,
) -> u32 {
    let mut spec = AircraftSpec::from_config(&default_planes()[1]);
    // A distinct "drone" colour so they read as targets.
    spec.body_color = Color::srgb(0.52, 0.46, 0.20);

    let orbits = [
        (Vec3::new(0.0, ground + 450.0, -900.0), 700.0, 110.0, 1.0),
        (
            Vec3::new(700.0, ground + 650.0, -1500.0),
            500.0,
            100.0,
            -1.0,
        ),
        (Vec3::new(-700.0, ground + 350.0, -300.0), 600.0, 120.0, 1.0),
    ];

    let mut count = 0;
    for (index, (center, radius, speed, direction)) in orbits.into_iter().enumerate() {
        let angle = index as f32 * 1.7;
        let position = Vec3::new(
            center.x + angle.cos() * radius,
            center.y,
            center.z + angle.sin() * radius,
        );
        let entity = commands
            .spawn((
                Transform::from_translation(position),
                Visibility::default(),
                DamageModel::new(10.0),
                TestTarget {
                    kind: TargetKind::Plane,
                    alive: true,
                    respawn_in: 0.0,
                },
                Orbit {
                    center,
                    radius,
                    speed,
                    angle,
                    direction,
                },
            ))
            .id();
        spawn_aircraft_model(commands, meshes, materials, &spec, entity);
        count += 1;
    }
    count
}

/// Fly the target planes around their orbits.
fn update_target_planes(
    time: Res<Time>,
    mut planes: Query<(&mut Transform, &mut Orbit, &TestTarget)>,
) {
    let dt = time.delta_secs();
    for (mut transform, mut orbit, target) in &mut planes {
        if !target.alive {
            continue;
        }
        let (position, heading) = orbit.step(dt);
        transform.translation = position;
        transform.look_to(heading, Vec3::Y);
    }
}

/// Tumble a destroyed target plane down to the ground.
fn update_falling(
    time: Res<Time>,
    world: Res<WorldKind>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut falling: Query<(Entity, &mut Transform, &mut Falling, &mut Visibility)>,
) {
    let dt = time.delta_secs();
    let ground = world.ground_level() + 1.0;
    for (entity, mut transform, mut fall, mut visibility) in &mut falling {
        fall.velocity += Vec3::NEG_Y * 9.81 * dt;
        transform.translation += fall.velocity * dt;
        transform.rotate_local_x(-FALL_SPIN * dt);
        if transform.translation.y <= ground {
            transform.translation.y = ground;
            spawn_explosion(
                &mut commands,
                &mut meshes,
                &mut materials,
                transform.translation,
            );
            *visibility = Visibility::Hidden;
            commands.entity(entity).remove::<Falling>();
        }
    }
}

/// Blow up destroyed targets and bring them back after a delay.
fn update_targets(
    time: Res<Time>,
    mut commands: Commands,
    mut score: ResMut<TargetScore>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut targets: Query<(
        Entity,
        &mut TestTarget,
        &mut DamageModel,
        &Transform,
        &mut Visibility,
    )>,
) {
    let dt = time.delta_secs();
    for (entity, mut target, mut damage, transform, mut visibility) in &mut targets {
        if target.alive {
            if damage.is_destroyed(AircraftPart::Fuselage) {
                target.alive = false;
                target.respawn_in = RESPAWN_SECS;
                score.destroyed += 1;
                spawn_explosion(
                    &mut commands,
                    &mut meshes,
                    &mut materials,
                    transform.translation,
                );
                if target.kind == TargetKind::Plane {
                    // Let it tumble down instead of vanishing.
                    let nose = transform.rotation * Vec3::NEG_Z;
                    commands.entity(entity).insert(Falling {
                        velocity: nose * 90.0,
                    });
                } else {
                    *visibility = Visibility::Hidden;
                }
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

/// One marker per test target.
fn sync_target_markers(
    mut commands: Commands,
    mut markers: ResMut<TargetMarkers>,
    targets: Query<Entity, With<TestTarget>>,
) {
    let ids: HashSet<Entity> = targets.iter().collect();

    for entity in &ids {
        if !markers.0.contains_key(entity) {
            let marker = commands
                .spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        display: Display::None,
                        ..default()
                    },
                    Text::new(""),
                    TextFont {
                        font_size: FontSize::Px(14.0),
                        ..default()
                    },
                    TextColor(Color::srgb(1.0, 0.55, 0.35)),
                    GlobalZIndex(6),
                    TargetMarker,
                ))
                .id();
            markers.0.insert(*entity, marker);
        }
    }

    markers.0.retain(|target, marker| {
        if ids.contains(target) {
            true
        } else {
            commands.entity(*marker).despawn();
            false
        }
    });
}

/// Put an Air RB-style marker on every test target we have spotted.
fn update_target_markers(
    spotting: Res<Spotting>,
    local: Query<&Transform, With<PlayerControlled>>,
    camera: Query<(&Camera, &GlobalTransform), With<ChaseCamera>>,
    targets: Query<(Entity, &TestTarget, &Transform)>,
    markers: Res<TargetMarkers>,
    mut nodes: Query<(&TargetMarker, &mut Node, &mut Text, &mut TextColor)>,
) {
    let Ok(local) = local.single() else {
        return;
    };
    let Ok((camera, camera_transform)) = camera.single() else {
        return;
    };
    let camera_forward = camera_transform.forward();
    let view_cone = spotting.view_cone_cos();

    for (entity, target, transform) in &targets {
        let Some(marker) = markers.0.get(&entity) else {
            continue;
        };
        let Ok((_, mut node, mut text, mut color)) = nodes.get_mut(*marker) else {
            continue;
        };

        let to_target = transform.translation - local.translation;
        let distance = to_target.length();
        let direction = to_target / distance.max(0.001);
        let in_cone = camera_forward.dot(direction) > view_cone;
        let spotted = target.alive
            && (distance < spotting.awareness_range
                || (distance < spotting.detection_range && in_cone));

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
        **text = format!("{}  {:.1} km", target.kind.label(), distance / 1000.0);
        *color = Color::srgb(1.0, 0.55, 0.35).into();
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
            .init_resource::<MatchClient>()
            .init_resource::<WorldKind>()
            .add_systems(Update, (sync_targets, update_target_planes));
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

        // Both ground targets and planes are present.
        let mut query = app.world_mut().query::<&TestTarget>();
        assert!(
            query.iter(app.world()).any(|t| t.kind == TargetKind::Plane),
            "there should be at least one target plane"
        );

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

        let mut query = app.world_mut().query::<(&TestTarget, &DamageModel)>();
        let mut seen = 0;
        for (target, damage) in query.iter(app.world()) {
            assert!(target.alive);
            assert!(damage.integrity(AircraftPart::Fuselage) > 0.0);
            seen += 1;
        }
        assert!(seen > 0);

        // Ground targets carry their own hit box; planes use the default.
        let mut query = app.world_mut().query::<(&TestTarget, Option<&HitBox>)>();
        for (target, hit_box) in query.iter(app.world()) {
            if target.kind != TargetKind::Plane {
                assert!(
                    hit_box.is_some_and(|b| b.0.min_element() > 0.0),
                    "ground targets need a non-empty hit box"
                );
            }
        }
    }

    #[test]
    fn target_plane_orbits_move_around_the_circle() {
        let mut orbit = Orbit {
            center: Vec3::new(0.0, 1000.0, 0.0),
            radius: 500.0,
            speed: 100.0,
            angle: 0.0,
            direction: 1.0,
        };
        let (start, _) = orbit.step(0.0);
        let (after, heading) = orbit.step(1.0);

        assert!(
            (after - start).length() > 50.0,
            "the plane should have moved along its orbit"
        );
        // It stays on the circle, at the orbit altitude.
        let radius = (after.x - orbit.center.x).hypot(after.z - orbit.center.z);
        assert!((radius - orbit.radius).abs() < 1.0);
        assert_eq!(after.y, orbit.center.y);
        // The heading is horizontal and non-zero.
        assert!(heading.length() > 0.5);
        assert_eq!(heading.y, 0.0);
    }
}
