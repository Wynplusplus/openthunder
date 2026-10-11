//! The map: ground/water, a runway, scattered landmarks, and the sun.
//!
//! A server can ship a full **map document** (see [`crate::map_doc`]) which the
//! client builds here: terrain plus every placed object. When no map document
//! arrives (single-player, or a rules-only map) the client falls back to the
//! built-in procedural **Training** and **Pacific Islands** terrains, chosen by
//! the server's map name.

use bevy::prelude::*;

use openthunder::map_doc::{MapDoc, MapObject, ObjectKind};

use crate::net::{ConnectionStatus, NetClient};

pub struct WorldPlugin;

impl Plugin for WorldPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<WorldKind>()
            .init_resource::<MapSource>()
            .init_resource::<BuiltWorld>()
            .add_systems(Startup, setup_world)
            .add_systems(Update, switch_world);
    }
}

/// The raw map text the server sent (empty until one arrives). Filled in by
/// `net.rs` when the `MAP` message arrives.
#[derive(Resource, Default)]
pub struct MapSource {
    pub text: String,
}

/// What is currently built, so the world is only rebuilt when it changes.
#[derive(Resource, Default)]
struct BuiltWorld {
    /// The map text that was built (`""` = the legacy procedural terrain).
    source: String,
    kind: WorldKind,
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

/// Rebuild the terrain when the server tells us we are on a different map, or
/// when a full map document arrives.
fn switch_world(
    mut commands: Commands,
    client: Res<NetClient>,
    source: Res<MapSource>,
    mut built: ResMut<BuiltWorld>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    existing: Query<Entity, With<WorldEntity>>,
) {
    let name = {
        let status = client.status.lock().unwrap();
        match &*status {
            ConnectionStatus::Connected { map, .. } => map.clone(),
            _ => return,
        }
    };
    let text = source.text.clone();
    let kind = WorldKind::for_map(&name);
    if built.source == text && built.kind == kind {
        return;
    }
    for entity in &existing {
        commands.entity(entity).despawn();
    }
    built.source = text.clone();
    built.kind = kind;
    if text.trim().is_empty() {
        build_world(&mut commands, &mut meshes, &mut materials, kind);
    } else {
        let doc = MapDoc::parse(&name, &text);
        build_map(&mut commands, &mut meshes, &mut materials, &doc);
    }
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

/// Build a world from a mission-editor map document: terrain, then every object.
fn build_map(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    doc: &MapDoc,
) {
    let size = doc.terrain.size.max(1_000.0);
    let ground = materials.add(StandardMaterial {
        base_color: srgb(doc.terrain.ground_color),
        perceptual_roughness: 1.0,
        ..default()
    });
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(size * 2.0, size * 2.0))),
        MeshMaterial3d(ground),
        WorldEntity,
    ));

    if doc.terrain.water {
        let water = materials.add(StandardMaterial {
            base_color: srgb(doc.terrain.water_color),
            perceptual_roughness: 0.4,
            metallic: 0.1,
            ..default()
        });
        commands.spawn((
            Mesh3d(meshes.add(Plane3d::default().mesh().size(size * 3.0, size * 3.0))),
            MeshMaterial3d(water),
            Transform::from_xyz(0.0, -1.0, 0.0),
            WorldEntity,
        ));
    }

    for object in &doc.objects {
        spawn_object(commands, meshes, materials, object);
    }
}

/// Spawn one placed object. Primitives are unit meshes scaled by the object;
/// [`ObjectKind::Model`] loads the object's glTF/GLB file.
fn spawn_object(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    object: &MapObject,
) {
    let transform = Transform::from_xyz(object.position[0], object.position[1], object.position[2])
        .with_rotation(Quat::from_rotation_y(object.yaw.to_radians()))
        .with_scale(Vec3::from(object.scale));

    if object.kind == ObjectKind::Model {
        let entity = commands
            .spawn((transform, Visibility::default(), WorldEntity))
            .id();
        if let Some(path) = object.model.as_deref() {
            crate::models::queue_model(commands, entity, path);
        }
        return;
    }

    let material = materials.add(StandardMaterial {
        base_color: srgb(object.color),
        perceptual_roughness: 0.9,
        ..default()
    });
    let mesh = match object.kind {
        // A flattened sphere makes a passable hill.
        ObjectKind::Hill => meshes.add(Sphere::new(0.5).mesh().ico(3).unwrap()),
        _ => meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
    };
    commands.spawn((
        Mesh3d(mesh),
        MeshMaterial3d(material),
        transform,
        WorldEntity,
    ));
}

/// An sRGB colour from linear-ish editor components.
fn srgb(color: [f32; 3]) -> Color {
    Color::srgb(color[0], color[1], color[2])
}
