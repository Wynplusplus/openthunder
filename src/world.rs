//! The map: ground/water, a runway, scattered landmarks, and the sun.
//!
//! Two variants are generated, chosen by the server's map name: the flat
//! **Training** field, and the **Pacific Islands** chain used by team
//! deathmatch. The world is rebuilt when the map changes (i.e. on connect).

use bevy::prelude::*;

use crate::net::{ConnectionStatus, NetClient};

pub struct WorldPlugin;

impl Plugin for WorldPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<WorldKind>()
            .add_systems(Startup, setup_world)
            .add_systems(Update, switch_world);
    }
}

/// Which terrain is currently built.
#[derive(Resource, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum WorldKind {
    #[default]
    Training,
    Islands,
}

impl WorldKind {
    fn for_map(map: &str) -> Self {
        let name = map.to_lowercase();
        if name.contains("island") || name.contains("pacific") {
            WorldKind::Islands
        } else {
            WorldKind::Training
        }
    }

    /// Height of the main landmass, where ground targets sit.
    pub(crate) fn ground_level(self) -> f32 {
        match self {
            WorldKind::Training => 0.0,
            WorldKind::Islands => 150.0,
        }
    }
}

/// Approximate terrain height at a world position, for the ground/landing model.
///
/// Training is a flat field; the islands map is water with a single rounded
/// home island (where the runway sits).
pub(crate) fn terrain_height(kind: WorldKind, x: f32, z: f32) -> f32 {
    match kind {
        WorldKind::Training => 0.0,
        WorldKind::Islands => {
            let radius = 2600.0;
            let r = (x * x + z * z).sqrt();
            if r < radius {
                let t = 1.0 - (r / radius).powi(2);
                150.0 * t.max(0.0).sqrt()
            } else {
                0.0
            }
        }
    }
}

/// Marks everything the world builder spawned, so it can be rebuilt.
#[derive(Component)]
struct WorldEntity;

fn setup_world(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // --- Sun (shared by every map, so never rebuilt) ---
    commands.spawn((
        DirectionalLight {
            illuminance: 15_000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(4000.0, 8000.0, 2000.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    build_world(
        &mut commands,
        &mut meshes,
        &mut materials,
        WorldKind::Training,
    );
}

/// Rebuild the terrain when the server tells us we are on a different map.
fn switch_world(
    mut commands: Commands,
    client: Res<NetClient>,
    mut current: ResMut<WorldKind>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    existing: Query<Entity, With<WorldEntity>>,
) {
    let map = {
        let status = client.status.lock().unwrap();
        match &*status {
            ConnectionStatus::Connected { map, .. } => map.clone(),
            _ => return,
        }
    };
    let wanted = WorldKind::for_map(&map);
    if *current == wanted {
        return;
    }
    for entity in &existing {
        commands.entity(entity).despawn();
    }
    *current = wanted;
    build_world(&mut commands, &mut meshes, &mut materials, wanted);
}

/// A tiny deterministic PRNG so a map looks the same every run.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f32 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((self.0 >> 33) as f32) / ((1u64 << 31) as f32)
    }

    fn range(&mut self, min: f32, max: f32) -> f32 {
        min + self.next() * (max - min)
    }
}

fn build_world(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    kind: WorldKind,
) {
    match kind {
        WorldKind::Training => build_training(commands, meshes, materials),
        WorldKind::Islands => build_islands(commands, meshes, materials),
    }
}

/// The flat training field: green ground, a runway, and scattered blocks.
fn build_training(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    let ground = materials.add(StandardMaterial {
        base_color: Color::srgb(0.17, 0.27, 0.12),
        perceptual_roughness: 1.0,
        ..default()
    });
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(60_000.0, 60_000.0))),
        MeshMaterial3d(ground),
        WorldEntity,
    ));

    // --- Runway, so the start position has a reference point ---
    let runway = materials.add(StandardMaterial {
        base_color: Color::srgb(0.09, 0.09, 0.10),
        perceptual_roughness: 0.9,
        ..default()
    });
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(60.0, 0.4, 1400.0))),
        MeshMaterial3d(runway),
        Transform::from_xyz(0.0, 0.2, 0.0),
        WorldEntity,
    ));

    let block = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    let building = materials.add(StandardMaterial {
        base_color: Color::srgb(0.36, 0.34, 0.31),
        perceptual_roughness: 0.85,
        ..default()
    });
    let tree = materials.add(StandardMaterial {
        base_color: Color::srgb(0.10, 0.22, 0.08),
        perceptual_roughness: 0.95,
        ..default()
    });

    let mut rng = Rng(0x1234_5678_9abc_def0);
    for _ in 0..500 {
        let x = rng.range(-9000.0, 9000.0);
        let z = rng.range(-9000.0, 9000.0);
        // Keep the runway clear.
        if x.abs() < 120.0 && z.abs() < 900.0 {
            continue;
        }
        let width = rng.range(20.0, 65.0);
        let depth = rng.range(20.0, 65.0);
        let height = rng.range(15.0, 105.0);
        let material = if rng.next() > 0.5 {
            tree.clone()
        } else {
            building.clone()
        };
        commands.spawn((
            Mesh3d(block.clone()),
            MeshMaterial3d(material),
            Transform::from_xyz(x, height * 0.5, z).with_scale(Vec3::new(width, height, depth)),
            WorldEntity,
        ));
    }
}

/// A Pacific island chain: water, green islands with sandy beaches, a runway on
/// the home island, and palm-ish trees.
fn build_islands(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    // --- Water ---
    let water = materials.add(StandardMaterial {
        base_color: Color::srgb(0.09, 0.28, 0.42),
        perceptual_roughness: 0.4,
        metallic: 0.1,
        ..default()
    });
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(120_000.0, 120_000.0))),
        MeshMaterial3d(water),
        WorldEntity,
    ));

    let land = materials.add(StandardMaterial {
        base_color: Color::srgb(0.20, 0.32, 0.13),
        perceptual_roughness: 0.95,
        ..default()
    });
    let sand = materials.add(StandardMaterial {
        base_color: Color::srgb(0.72, 0.66, 0.44),
        perceptual_roughness: 1.0,
        ..default()
    });
    let runway = materials.add(StandardMaterial {
        base_color: Color::srgb(0.10, 0.10, 0.11),
        perceptual_roughness: 0.9,
        ..default()
    });
    let tree = materials.add(StandardMaterial {
        base_color: Color::srgb(0.10, 0.24, 0.09),
        perceptual_roughness: 0.95,
        ..default()
    });

    // A flattened sphere makes a passable island mound.
    let mound = meshes.add(Sphere::new(1.0).mesh().ico(3).unwrap());

    let island =
        |commands: &mut Commands, x: f32, z: f32, radius: f32, height: f32, beach: bool| {
            if beach {
                commands.spawn((
                    Mesh3d(mound.clone()),
                    MeshMaterial3d(sand.clone()),
                    Transform::from_xyz(x, 0.0, z).with_scale(Vec3::new(
                        radius * 1.15,
                        height * 0.45,
                        radius * 1.15,
                    )),
                    WorldEntity,
                ));
            }
            commands.spawn((
                Mesh3d(mound.clone()),
                MeshMaterial3d(land.clone()),
                Transform::from_xyz(x, 0.0, z).with_scale(Vec3::new(radius, height, radius)),
                WorldEntity,
            ));
        };

    // --- Home island with the runway (near the spawn) ---
    island(commands, 0.0, 0.0, 2600.0, 150.0, true);
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(70.0, 4.0, 1600.0))),
        MeshMaterial3d(runway.clone()),
        Transform::from_xyz(0.0, 152.0, 0.0),
        WorldEntity,
    ));

    // --- The rest of the chain ---
    let mut rng = Rng(0x00c0_ffee_1234_5678);
    for _ in 0..14 {
        let angle = rng.range(0.0, std::f32::consts::TAU);
        let distance = rng.range(4500.0, 16000.0);
        let x = angle.cos() * distance;
        let z = angle.sin() * distance;
        let radius = rng.range(500.0, 2200.0);
        let height = rng.range(60.0, 260.0);
        island(commands, x, z, radius, height, true);

        // Trees on top.
        let block = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
        for _ in 0..18 {
            let tx = x + rng.range(-radius * 0.7, radius * 0.7);
            let tz = z + rng.range(-radius * 0.7, radius * 0.7);
            let height = rng.range(15.0, 40.0);
            commands.spawn((
                Mesh3d(block.clone()),
                MeshMaterial3d(tree.clone()),
                Transform::from_xyz(tx, height * 0.5, tz).with_scale(Vec3::new(12.0, height, 12.0)),
                WorldEntity,
            ));
        }
    }
}
