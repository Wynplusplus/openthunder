//! Third-person chase camera, War Thunder style: it lags behind and above the
//! aircraft and partially rolls with it.
//!
//! The camera always looks exactly along the aircraft's nose (its *forward*
//! direction). That is what makes pointer aiming work: the screen centre maps to
//! the nose direction, so the aircraft can be steered by pointing the cursor.

use bevy::prelude::*;

use crate::aircraft::{Aircraft, PlayerControlled};

/// Marks the camera that follows the player's aircraft.
#[derive(Component)]
pub struct ChaseCamera;

pub struct ChaseCameraPlugin;

impl Plugin for ChaseCameraPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_camera)
            .add_systems(Update, chase_camera);
    }
}

fn spawn_camera(mut commands: Commands) {
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 1005.0, 20.0).looking_at(Vec3::new(0.0, 1000.0, -30.0), Vec3::Y),
        ChaseCamera,
        // Fade distant terrain into the horizon colour.
        DistanceFog {
            color: Color::srgb(0.45, 0.62, 0.85),
            falloff: FogFalloff::Linear {
                start: 4000.0,
                end: 35000.0,
            },
            ..default()
        },
    ));
}

fn chase_camera(
    time: Res<Time>,
    target: Query<&Transform, (With<PlayerControlled>, Without<ChaseCamera>)>,
    mut camera: Query<&mut Transform, With<ChaseCamera>>,
) {
    let Ok(aircraft) = target.single() else {
        return;
    };
    let Ok(mut camera_transform) = camera.single_mut() else {
        return;
    };

    let dt = time.delta_secs();

    // Desired position: behind (+Z is behind, since the nose is -Z) and above.
    let desired = aircraft.translation + aircraft.rotation * Vec3::new(0.0, 3.0, 18.0);
    let follow = 1.0 - (-6.0 * dt).exp();
    camera_transform.translation = camera_transform
        .translation
        .lerp(desired, follow.clamp(0.0, 1.0));

    // Look exactly along the nose. `looking_to` (rather than `looking_at`)
    // guarantees the camera's forward equals the aircraft's forward, so the
    // screen centre corresponds to where the nose points.
    let forward = Aircraft::forward(aircraft.rotation);

    // Partially inherit the aircraft's roll for a dynamic feel. Weighting world
    // up avoids a degenerate up-vector when inverted. (Roll does not change the
    // forward direction, so this does not affect aiming.)
    let up = aircraft.rotation * Vec3::Y;
    let camera_up = (up + Vec3::Y * 2.0).normalize_or_zero();
    let camera_up = if camera_up.length_squared() < 0.5 {
        Vec3::Y
    } else {
        camera_up
    };

    *camera_transform =
        Transform::from_translation(camera_transform.translation).looking_to(forward, camera_up);
}
