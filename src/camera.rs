//! Third-person chase camera, War Thunder style: it lags behind and above the
//! aircraft and partially rolls with it.
//!
//! The camera normally looks exactly along the aircraft's nose (its *forward*
//! direction). That is what makes pointer aiming work: the screen centre maps to
//! the nose direction, so the aircraft can be steered by pointing the cursor.
//!
//! **Free look** (hold the `free_look` key, default `C`) orbits the camera
//! around the aircraft with the mouse while the aircraft keeps flying. Release
//! and the camera eases back behind the nose.

use std::f32::consts::PI;

use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use openthunder::keybinds::{FREE_LOOK, ZOOM};

use crate::aircraft::PlayerControlled;
use crate::flight::Bindings;

/// Radians of look per pixel of mouse movement.
const LOOK_SENSITIVITY: f32 = 0.004;
/// Maximum look pitch (radians).
const MAX_PITCH: f32 = 1.3;
/// Normal vertical field of view (radians).
const NORMAL_FOV: f32 = std::f32::consts::FRAC_PI_4;
/// Field of view while zoomed in (radians, about 18 degrees).
const ZOOM_FOV: f32 = 0.32;

/// Marks the camera that follows the player's aircraft.
#[derive(Component)]
pub struct ChaseCamera;

/// Free-look state: `active` while the key is held, plus the current orbit.
#[derive(Resource, Default)]
pub struct FreeLook {
    pub active: bool,
    pub yaw: f32,
    pub pitch: f32,
}

impl FreeLook {
    /// Orbit rotation in the aircraft's frame.
    pub fn rotation(&self) -> Quat {
        Quat::from_rotation_y(self.yaw) * Quat::from_rotation_x(self.pitch)
    }
}

pub struct ChaseCameraPlugin;

impl Plugin for ChaseCameraPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FreeLook>()
            .add_systems(Startup, spawn_camera)
            .add_systems(
                Update,
                (
                    free_look_input,
                    chase_camera.after(crate::flight::FlightSet),
                ),
            );
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

/// Hold the free-look key and move the mouse to orbit the camera.
fn free_look_input(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    bindings: Res<Bindings>,
    mouse_motion: Res<AccumulatedMouseMotion>,
    mut look: ResMut<FreeLook>,
) {
    look.active = bindings.pressed(&keys, FREE_LOOK);

    if look.active {
        let delta = mouse_motion.delta;
        // Mouse right -> look right, mouse up -> look up.
        look.yaw = (look.yaw - delta.x * LOOK_SENSITIVITY).clamp(-PI, PI);
        look.pitch = (look.pitch - delta.y * LOOK_SENSITIVITY).clamp(-MAX_PITCH, MAX_PITCH);
    } else {
        // Ease back behind the aircraft.
        let blend = (1.0 - (-12.0 * time.delta_secs()).exp()).clamp(0.0, 1.0);
        look.yaw -= look.yaw * blend;
        look.pitch -= look.pitch * blend;
        if look.yaw.abs() < 1e-3 {
            look.yaw = 0.0;
        }
        if look.pitch.abs() < 1e-3 {
            look.pitch = 0.0;
        }
    }
}

fn chase_camera(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    bindings: Res<Bindings>,
    look: Res<FreeLook>,
    target: Query<&Transform, (With<PlayerControlled>, Without<ChaseCamera>)>,
    mut camera: Query<(&mut Transform, &mut Projection), With<ChaseCamera>>,
    mut rig: Local<Quat>,
    mut zoom: Local<f32>,
) {
    let Ok(aircraft) = target.single() else {
        return;
    };
    let Ok((mut camera_transform, mut projection)) = camera.single_mut() else {
        return;
    };

    let dt = time.delta_secs();

    // --- Zoom: hold the key to narrow the field of view, WT style ---
    let want_zoom = bindings.pressed(&keys, ZOOM);
    let target_zoom = if want_zoom { 1.0 } else { 0.0 };
    *zoom += (target_zoom - *zoom) * (1.0 - (-10.0 * dt).exp()).clamp(0.0, 1.0);
    if let Projection::Perspective(perspective) = &mut *projection {
        perspective.fov = NORMAL_FOV + (ZOOM_FOV - NORMAL_FOV) * *zoom;
    }

    // The camera rig is the aircraft's frame rotated by the free-look orbit, so
    // the aircraft stays put on screen while the camera circles it.
    //
    // We smooth the *rig* (a rotation) rather than the world position: that way
    // the camera always sits exactly on the circle of radius |offset| around the
    // aircraft instead of cutting the corner and drifting in and out.
    let target_rig = aircraft.rotation * look.rotation();
    let follow = (1.0 - (-10.0 * dt).exp()).clamp(0.0, 1.0);
    *rig = rig.slerp(target_rig, follow);

    // Constant-radius offset: behind (+Z is behind, since the nose is -Z) and up.
    // Pull in a little while zoomed so the aircraft does not fill the view.
    let distance = 18.0 - 5.0 * *zoom;
    let offset = *rig * Vec3::new(0.0, 3.0, distance);
    camera_transform.translation = aircraft.translation + offset;

    // Look along the (free-look rotated) nose. `looking_to` guarantees the
    // camera's forward is exactly this direction.
    let forward = *rig * Vec3::NEG_Z;

    // Partially inherit the aircraft's roll for a dynamic feel. Weighting world
    // up avoids a degenerate up-vector when inverted.
    let up = *rig * Vec3::Y;
    let camera_up = (up + Vec3::Y * 2.0).normalize_or_zero();
    let camera_up = if camera_up.length_squared() < 0.5 {
        Vec3::Y
    } else {
        camera_up
    };

    *camera_transform =
        Transform::from_translation(camera_transform.translation).looking_to(forward, camera_up);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn look_rotation_is_identity_at_rest() {
        let look = FreeLook::default();
        let rotated = look.rotation() * Vec3::NEG_Z;
        assert!((rotated - Vec3::NEG_Z).length() < 1e-6);
    }

    #[test]
    fn positive_yaw_looks_right() {
        // Mouse right accumulates negative yaw (see `free_look_input`), so a
        // negative yaw should swing the view toward the aircraft's right (+X).
        let look = FreeLook {
            yaw: -0.5,
            ..default()
        };
        let forward = look.rotation() * Vec3::NEG_Z;
        assert!(forward.x > 0.0, "expected to look right, got {forward:?}");
    }

    #[test]
    fn positive_pitch_looks_up() {
        let look = FreeLook {
            pitch: 0.5,
            ..default()
        };
        let forward = look.rotation() * Vec3::NEG_Z;
        assert!(forward.y > 0.0, "expected to look up, got {forward:?}");
    }

    #[test]
    fn free_look_key_and_mouse_orbit_then_return() {
        use openthunder::keybinds::Keybinds;

        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<AccumulatedMouseMotion>()
            .init_resource::<FreeLook>()
            .insert_resource(Bindings::from_config(&Keybinds::default()))
            .add_systems(Update, free_look_input);

        // Hold C and move the mouse right: look right (negative yaw).
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyC);
        app.world_mut()
            .resource_mut::<AccumulatedMouseMotion>()
            .delta = Vec2::new(100.0, 0.0);
        app.update();
        {
            let look = app.world().resource::<FreeLook>();
            assert!(look.active, "free look should be active while C is held");
            assert!(
                look.yaw < 0.0,
                "mouse right should look right, got {}",
                look.yaw
            );
        }

        // Release C: the orbit eases back toward centre.
        let before = app.world().resource::<FreeLook>().yaw.abs();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(KeyCode::KeyC);
        app.world_mut()
            .resource_mut::<AccumulatedMouseMotion>()
            .delta = Vec2::ZERO;
        for _ in 0..60 {
            app.update();
        }
        let look = app.world().resource::<FreeLook>();
        assert!(!look.active);
        assert!(
            look.yaw.abs() < before,
            "orbit should ease back, was {before}, now {}",
            look.yaw.abs()
        );
    }

    #[test]
    fn holding_zoom_narrows_the_field_of_view() {
        use openthunder::keybinds::Keybinds;

        let mut app = App::new();
        app.insert_resource(Time::<()>::default())
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<FreeLook>()
            .insert_resource(Bindings::from_config(&Keybinds::default()))
            .add_systems(Update, chase_camera);

        app.world_mut().spawn((
            Transform::from_translation(Vec3::new(0.0, 1000.0, 0.0)),
            PlayerControlled,
        ));
        let camera = app
            .world_mut()
            .spawn((
                Transform::default(),
                Projection::Perspective(PerspectiveProjection {
                    fov: NORMAL_FOV,
                    ..default()
                }),
                ChaseCamera,
            ))
            .id();

        let fov_of = |app: &App| match app.world().get::<Projection>(camera).unwrap() {
            Projection::Perspective(perspective) => perspective.fov,
            _ => unreachable!("the chase camera is a perspective camera"),
        };

        // Hold the zoom key: the field of view narrows.
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyZ);
        for _ in 0..80 {
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(std::time::Duration::from_secs_f32(0.05));
            app.update();
        }
        let zoomed = fov_of(&app);
        assert!(
            zoomed < NORMAL_FOV * 0.7,
            "zoom should narrow the fov, got {zoomed}"
        );

        // Release: it eases back out.
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(KeyCode::KeyZ);
        for _ in 0..120 {
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(std::time::Duration::from_secs_f32(0.05));
            app.update();
        }
        assert!(
            (fov_of(&app) - NORMAL_FOV).abs() < 0.05,
            "zoom should return to normal, got {}",
            fov_of(&app)
        );
    }
}
