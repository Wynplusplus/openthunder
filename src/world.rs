//! A simple, large training map: ground, a runway, scattered landmarks, and the
//! sun. Just enough to give a sense of speed, altitude and scale while flying.

use bevy::prelude::*;

pub struct WorldPlugin;

impl Plugin for WorldPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_world);
    }
}

fn setup_world(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // --- Sun ---
    commands.spawn((
        DirectionalLight {
            illuminance: 15_000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(4000.0, 8000.0, 2000.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    // --- Ground ---
    let ground = materials.add(StandardMaterial {
        base_color: Color::srgb(0.17, 0.27, 0.12),
        perceptual_roughness: 1.0,
        ..default()
    });
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(60_000.0, 60_000.0))),
        MeshMaterial3d(ground),
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
    ));

    // --- Scattered landmarks to judge motion and altitude ---
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

    // Tiny deterministic PRNG so the map is the same every run.
    let mut seed: u64 = 0x1234_5678_9abc_def0;
    let mut rand = move || {
        seed = seed
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((seed >> 33) as f32) / ((1u64 << 31) as f32)
    };

    for _ in 0..500 {
        let x = (rand() * 2.0 - 1.0) * 9000.0;
        let z = (rand() * 2.0 - 1.0) * 9000.0;

        // Keep the runway clear.
        if x.abs() < 120.0 && z.abs() < 900.0 {
            continue;
        }

        let width = 20.0 + rand() * 45.0;
        let depth = 20.0 + rand() * 45.0;
        let height = 15.0 + rand() * 90.0;
        let material = if rand() > 0.5 {
            tree.clone()
        } else {
            building.clone()
        };

        commands.spawn((
            Mesh3d(block.clone()),
            MeshMaterial3d(material),
            Transform::from_xyz(x, height * 0.5, z).with_scale(Vec3::new(width, height, depth)),
        ));
    }
}
