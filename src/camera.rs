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

use crate::aircraft::{Aircraft, PlayerControlled};
use crate::flight::{Bindings, MouseAim};

/// Radians of look per pixel of mouse movement.
const LOOK_SENSITIVITY: f32 = 0.004;
/// Maximum look pitch (radians).
const MAX_PITCH: f32 = 1.3;
/// Normal vertical field of view (radians).
const NORMAL_FOV: f32 = std::f32::consts::FRAC_PI_4;
/// Field of view while zoomed in (radians, about 18 degrees).
const ZOOM_FOV: f32 = 0.32;
/// The camera only starts orbiting toward the aim once it is this far from the
/// nose (radians) — a dead-zone around the centre of the screen.
const CAMERA_ORBIT_DEADZONE: f32 = 0.5;
/// ...and orbits fully by this offset (radians), near the edge of the screen.
const CAMERA_ORBIT_EDGE: f32 = 0.9;

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

/// Whether the player has toggled the zoom in, and how far it has eased in.
#[derive(Resource, Default)]
pub struct ZoomState {
    pub active: bool,
    /// Smoothed zoom, `0.0` (normal) .. `1.0` (fully zoomed).
    pub amount: f32,
}

impl ZoomState {
    /// How much to scale mouse sensitivity by so the cursor still moves the same
    /// distance on screen while zoomed in (the field of view is narrower).
    pub fn sensitivity_scale(&self) -> f32 {
        let ratio = ZOOM_FOV / NORMAL_FOV;
        1.0 - self.amount * (1.0 - ratio)
    }
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
            .init_resource::<ZoomState>()
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
    mouse: Res<ButtonInput<MouseButton>>,
    bindings: Res<Bindings>,
    mouse_motion: Res<AccumulatedMouseMotion>,
    mut look: ResMut<FreeLook>,
) {
    look.active = bindings.pressed(&keys, &mouse, FREE_LOOK);

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
    mouse: Res<ButtonInput<MouseButton>>,
    bindings: Res<Bindings>,
    look: Res<FreeLook>,
    mouse_aim: Res<MouseAim>,
    mut zoom_state: ResMut<ZoomState>,
    target: Query<(&Transform, &Aircraft), (With<PlayerControlled>, Without<ChaseCamera>)>,
    mut camera: Query<(&mut Transform, &mut Projection), With<ChaseCamera>>,
    mut look_dir: Local<Vec3>,
    mut prev_speed: Local<f32>,
    mut energy: Local<f32>,
) {
    let Ok((aircraft, aircraft_state)) = target.single() else {
        return;
    };
    let Ok((mut camera_transform, mut projection)) = camera.single_mut() else {
        return;
    };

    let dt = time.delta_secs();

    // --- Zoom: tap the button to toggle a narrow field of view, WT style ---
    if bindings.just_pressed(&keys, &mouse, ZOOM) {
        zoom_state.active = !zoom_state.active;
    }
    let target_zoom = if zoom_state.active { 1.0 } else { 0.0 };
    zoom_state.amount +=
        (target_zoom - zoom_state.amount) * (1.0 - (-10.0 * dt).exp()).clamp(0.0, 1.0);
    if let Projection::Perspective(perspective) = &mut *projection {
        perspective.fov = NORMAL_FOV + (ZOOM_FOV - NORMAL_FOV) * zoom_state.amount;
    }

    // --- Where the camera looks ---
    // For small aim offsets it sits behind the nose; once the cursor nears the
    // edge of the screen it orbits toward the aim (WT: "the camera accelerates
    // toward the cursor"). While free-looking it follows the orbit instead. It
    // stays level with the horizon rather than rolling with the aircraft, as in
    // WT mouse aim.
    let nose = aircraft.rotation * Vec3::NEG_Z;
    let want = if look.active {
        (aircraft.rotation * look.rotation()) * Vec3::NEG_Z
    } else if mouse_aim.engaged && mouse_aim.target != Vec3::ZERO {
        // Dead-zone around the centre, ramping to full orbit by the screen edge.
        // Zooming in locks the view behind the nose (as in WT), so the orbit
        // fades out as the zoom eases in.
        let offset = nose.angle_between(mouse_aim.target);
        let t = ((offset - CAMERA_ORBIT_DEADZONE)
            / (CAMERA_ORBIT_EDGE - CAMERA_ORBIT_DEADZONE).max(0.01))
        .clamp(0.0, 1.0);
        let t = t * t * (3.0 - 2.0 * t) * (1.0 - zoom_state.amount);
        nose.lerp(mouse_aim.target, t).normalize_or_zero()
    } else {
        nose
    };
    if look_dir.length_squared() < 0.5 {
        *look_dir = want;
    }
    let follow = (1.0 - (-8.0 * dt).exp()).clamp(0.0, 1.0);
    *look_dir = look_dir.lerp(want, follow).normalize_or_zero();

    // --- Where the camera sits ---
    // WT pulls the camera back when you accelerate or brake hard, so you can
    // gauge your energy (it is the acceleration, not the speed, that moves it).
    let speed = aircraft_state.velocity.length();
    let accel = if dt > 1e-4 {
        (speed - *prev_speed) / dt
    } else {
        0.0
    };
    *prev_speed = speed;
    let target_energy = (accel.abs() * 0.5).clamp(0.0, 5.0);
    *energy += (target_energy - *energy) * (1.0 - (-4.0 * dt).exp()).clamp(0.0, 1.0);

    // Behind the look direction, so the aircraft stays in view as you look
    // around (and stays level rather than rolling with it); pull in a little
    // while zoomed so the aircraft does not fill the view.
    let distance = 18.0 - 5.0 * zoom_state.amount + *energy;
    let offset = *look_dir * -distance + Vec3::Y * 3.0;
    camera_transform.translation = aircraft.translation + offset;

    // Look along the smoothed direction, level with the horizon.
    let up = if look_dir.y.abs() > 0.95 {
        Vec3::Z
    } else {
        Vec3::Y
    };
    *camera_transform =
        Transform::from_translation(camera_transform.translation).looking_to(*look_dir, up);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A bare aircraft for the camera tests.
    fn test_aircraft() -> Aircraft {
        Aircraft::new(crate::aircraft::AircraftSpec::from_config(
            &openthunder::plane_config::default_planes()[0],
        ))
    }

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
            .init_resource::<ButtonInput<MouseButton>>()
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

    /// Clear the "just pressed" state each frame, like the input plugin does.
    fn clear_just_pressed(
        mut keys: ResMut<ButtonInput<KeyCode>>,
        mut mouse: ResMut<ButtonInput<MouseButton>>,
    ) {
        keys.clear();
        mouse.clear();
    }

    #[test]
    fn tapping_zoom_toggles_the_field_of_view() {
        use openthunder::keybinds::Keybinds;

        let mut app = App::new();
        app.insert_resource(Time::<()>::default())
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<FreeLook>()
            .init_resource::<ZoomState>()
            .init_resource::<MouseAim>()
            .insert_resource(Bindings::from_config(&Keybinds::default()))
            .add_systems(Update, (chase_camera, clear_just_pressed).chain());

        app.world_mut().spawn((
            Transform::from_translation(Vec3::new(0.0, 1000.0, 0.0)),
            test_aircraft(),
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
        let run = |app: &mut App, frames: usize| {
            for _ in 0..frames {
                app.world_mut()
                    .resource_mut::<Time>()
                    .advance_by(std::time::Duration::from_secs_f32(0.05));
                app.update();
            }
        };

        // Tap the zoom button once: the field of view narrows.
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Right);
        run(&mut app, 80);
        assert!(
            fov_of(&app) < NORMAL_FOV * 0.7,
            "a tap should zoom in, got {}",
            fov_of(&app)
        );
        assert!(app.world().resource::<ZoomState>().active);

        // Tap again: it eases back out.
        {
            let mut mouse = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
            mouse.release(MouseButton::Right);
            mouse.clear();
            mouse.press(MouseButton::Right);
        }
        run(&mut app, 120);
        assert!(
            (fov_of(&app) - NORMAL_FOV).abs() < 0.05,
            "a second tap should zoom out, got {}",
            fov_of(&app)
        );
        assert!(!app.world().resource::<ZoomState>().active);
    }

    /// The camera follows the aim direction, so you can aim anywhere and the view
    /// pans with it.
    #[test]
    fn the_camera_follows_the_aim_direction() {
        use openthunder::keybinds::Keybinds;

        let mut app = App::new();
        app.insert_resource(Time::<()>::default())
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<FreeLook>()
            .init_resource::<ZoomState>()
            .init_resource::<MouseAim>()
            .insert_resource(Bindings::from_config(&Keybinds::default()))
            .add_systems(Update, chase_camera);

        app.world_mut().spawn((
            Transform::from_translation(Vec3::new(0.0, 1000.0, 0.0)),
            test_aircraft(),
            PlayerControlled,
        ));
        let camera = app
            .world_mut()
            .spawn((
                Transform::default(),
                Projection::Perspective(PerspectiveProjection::default()),
                ChaseCamera,
            ))
            .id();

        // Aim about 49 degrees to the right of the nose (past the dead-zone).
        {
            let mut aim = app.world_mut().resource_mut::<MouseAim>();
            aim.engaged = true;
            aim.target = Quat::from_rotation_y(-0.85) * Vec3::NEG_Z;
        }

        for _ in 0..60 {
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(std::time::Duration::from_secs_f32(0.05));
            app.update();
        }

        let rotation = app.world().get::<Transform>(camera).unwrap().rotation;
        let forward = rotation * Vec3::NEG_Z;
        assert!(
            forward.x > 0.05,
            "the camera should lean toward the aim, got {forward:?}"
        );
    }

    /// In mouse aim the camera stays level with the horizon instead of rolling
    /// with the aircraft (as in WT).
    #[test]
    fn the_camera_stays_level_with_the_horizon() {
        use openthunder::keybinds::Keybinds;

        let mut app = App::new();
        app.insert_resource(Time::<()>::default())
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<FreeLook>()
            .init_resource::<ZoomState>()
            .init_resource::<MouseAim>()
            .insert_resource(Bindings::from_config(&Keybinds::default()))
            .add_systems(Update, chase_camera);

        // A rolled aircraft, aiming straight ahead.
        app.world_mut().spawn((
            Transform::from_translation(Vec3::new(0.0, 1000.0, 0.0))
                .with_rotation(Quat::from_rotation_z(1.2)),
            test_aircraft(),
            PlayerControlled,
        ));
        let camera = app
            .world_mut()
            .spawn((
                Transform::default(),
                Projection::Perspective(PerspectiveProjection::default()),
                ChaseCamera,
            ))
            .id();
        {
            let mut aim = app.world_mut().resource_mut::<MouseAim>();
            aim.engaged = true;
            aim.target = Vec3::NEG_Z;
        }

        for _ in 0..60 {
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(std::time::Duration::from_secs_f32(0.05));
            app.update();
        }

        let rotation = app.world().get::<Transform>(camera).unwrap().rotation;
        let up = rotation * Vec3::Y;
        assert!(
            up.dot(Vec3::Y) > 0.9,
            "the camera should stay level with the horizon, got up {up:?}"
        );
    }

    /// A small aim offset stays inside the dead-zone, so the camera does not
    /// orbit for small cursor movements.
    #[test]
    fn the_camera_does_not_orbit_for_a_small_aim_offset() {
        use openthunder::keybinds::Keybinds;

        let mut app = App::new();
        app.insert_resource(Time::<()>::default())
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<FreeLook>()
            .init_resource::<ZoomState>()
            .init_resource::<MouseAim>()
            .insert_resource(Bindings::from_config(&Keybinds::default()))
            .add_systems(Update, chase_camera);

        app.world_mut().spawn((
            Transform::from_translation(Vec3::new(0.0, 1000.0, 0.0)),
            test_aircraft(),
            PlayerControlled,
        ));
        let camera = app
            .world_mut()
            .spawn((
                Transform::default(),
                Projection::Perspective(PerspectiveProjection::default()),
                ChaseCamera,
            ))
            .id();

        // A moderate aim offset, still inside the dead-zone.
        {
            let mut aim = app.world_mut().resource_mut::<MouseAim>();
            aim.engaged = true;
            aim.target = Quat::from_rotation_y(-0.3) * Vec3::NEG_Z;
        }
        for _ in 0..60 {
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(std::time::Duration::from_secs_f32(0.05));
            app.update();
        }

        let rotation = app.world().get::<Transform>(camera).unwrap().rotation;
        let forward = rotation * Vec3::NEG_Z;
        assert!(
            forward.x.abs() < 0.02,
            "the camera should not orbit for a small aim offset, got {forward:?}"
        );
    }

    /// WT pulls the camera back when you accelerate, so you can gauge your energy.
    #[test]
    fn the_camera_pulls_back_under_acceleration() {
        use openthunder::keybinds::Keybinds;

        let mut app = App::new();
        app.insert_resource(Time::<()>::default())
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<FreeLook>()
            .init_resource::<ZoomState>()
            .init_resource::<MouseAim>()
            .insert_resource(Bindings::from_config(&Keybinds::default()))
            .add_systems(Update, chase_camera);

        let mut aircraft = test_aircraft();
        aircraft.velocity = Vec3::NEG_Z * 100.0;
        let plane = app
            .world_mut()
            .spawn((
                Transform::from_translation(Vec3::new(0.0, 1000.0, 0.0)),
                aircraft,
                PlayerControlled,
            ))
            .id();
        let camera = app
            .world_mut()
            .spawn((
                Transform::default(),
                Projection::Perspective(PerspectiveProjection::default()),
                ChaseCamera,
            ))
            .id();

        for _ in 0..60 {
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(std::time::Duration::from_secs_f32(0.05));
            // Accelerate forward.
            app.world_mut().get_mut::<Aircraft>(plane).unwrap().velocity += Vec3::NEG_Z;
            app.update();
        }

        let plane_pos = app.world().get::<Transform>(plane).unwrap().translation;
        let camera_pos = app.world().get::<Transform>(camera).unwrap().translation;
        let distance = (camera_pos - plane_pos).length();
        assert!(
            distance > 19.0,
            "the camera should pull back under acceleration, got {distance:.1} m"
        );
    }

    /// Zooming in locks the camera behind the nose — it stops orbiting toward
    /// the aim (as in WT).
    #[test]
    fn zooming_in_stops_the_camera_orbiting() {
        use openthunder::keybinds::Keybinds;

        let mut app = App::new();
        app.insert_resource(Time::<()>::default())
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<FreeLook>()
            .init_resource::<ZoomState>()
            .init_resource::<MouseAim>()
            .insert_resource(Bindings::from_config(&Keybinds::default()))
            .add_systems(Update, chase_camera);

        app.world_mut().spawn((
            Transform::from_translation(Vec3::new(0.0, 1000.0, 0.0)),
            test_aircraft(),
            PlayerControlled,
        ));
        let camera = app
            .world_mut()
            .spawn((
                Transform::default(),
                Projection::Perspective(PerspectiveProjection::default()),
                ChaseCamera,
            ))
            .id();

        // A large aim offset that would normally orbit the camera...
        {
            let mut aim = app.world_mut().resource_mut::<MouseAim>();
            aim.engaged = true;
            aim.target = Quat::from_rotation_y(-0.85) * Vec3::NEG_Z;
        }
        // ...and zoom fully in.
        {
            let mut zoom = app.world_mut().resource_mut::<ZoomState>();
            zoom.active = true;
            zoom.amount = 1.0;
        }

        for _ in 0..60 {
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(std::time::Duration::from_secs_f32(0.05));
            app.update();
        }

        let rotation = app.world().get::<Transform>(camera).unwrap().rotation;
        let forward = rotation * Vec3::NEG_Z;
        assert!(
            forward.x.abs() < 0.02,
            "the camera should stay on the nose while zoomed, got {forward:?}"
        );
    }
}
