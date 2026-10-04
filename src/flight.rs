//! Flight model, control input, and rigid-body integration.
//!
//! # Model overview
//!
//! This is a simplified but genuinely physical 6-DoF model, in the spirit of a
//! War Thunder "Air RB" flight model:
//!
//! * Forces: gravity, engine thrust, aerodynamic **lift**, **drag**, and a small
//!   sideslip side-force.
//! * Lift uses a real lift-curve slope and **stalls** past the critical angle of
//!   attack (lift collapses), which produces wing drops and nose-over behaviour.
//! * Drag is parasitic + induced (`CD = CD0 + CL^2 / (pi * e * AR)`).
//! * Air density falls exponentially with altitude.
//! * Rotation is driven by control inputs (pitch/roll/yaw) scaled by dynamic
//!   pressure, so controls go mushy at low speed, plus aerodynamic
//!   **weathervane stability** that aligns the nose with the airflow.
//! * Damage degrades all of the above (see `damage.rs`).

use std::f32::consts::PI;

use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use openthunder::keybinds::{
    FLAPS_DOWN, FLAPS_UP, Keybinds, PITCH_DOWN, PITCH_DOWN_ALT, PITCH_UP, PITCH_UP_ALT, RESET,
    ROLL_LEFT, ROLL_RIGHT, THROTTLE_DOWN, THROTTLE_UP, WEP, YAW_LEFT, YAW_RIGHT,
};

use crate::aircraft::{
    Aircraft, AircraftSpec, Controls, FlapSetting, PlayerControlled, START_POSITION,
};
use crate::camera::{ChaseCamera, FreeLook};
use crate::damage::{AircraftPart, DamageModel};
use crate::menu::GameMenu;

/// Canonical key names -> Bevy key codes. The same names are validated by the
/// launcher against `openthunder::keybinds::SUPPORTED_KEYS`.
const KEY_TABLE: &[(&str, KeyCode)] = &[
    ("A", KeyCode::KeyA),
    ("B", KeyCode::KeyB),
    ("C", KeyCode::KeyC),
    ("D", KeyCode::KeyD),
    ("E", KeyCode::KeyE),
    ("F", KeyCode::KeyF),
    ("G", KeyCode::KeyG),
    ("H", KeyCode::KeyH),
    ("I", KeyCode::KeyI),
    ("J", KeyCode::KeyJ),
    ("K", KeyCode::KeyK),
    ("L", KeyCode::KeyL),
    ("M", KeyCode::KeyM),
    ("N", KeyCode::KeyN),
    ("O", KeyCode::KeyO),
    ("P", KeyCode::KeyP),
    ("Q", KeyCode::KeyQ),
    ("R", KeyCode::KeyR),
    ("S", KeyCode::KeyS),
    ("T", KeyCode::KeyT),
    ("U", KeyCode::KeyU),
    ("V", KeyCode::KeyV),
    ("W", KeyCode::KeyW),
    ("X", KeyCode::KeyX),
    ("Y", KeyCode::KeyY),
    ("Z", KeyCode::KeyZ),
    ("0", KeyCode::Digit0),
    ("1", KeyCode::Digit1),
    ("2", KeyCode::Digit2),
    ("3", KeyCode::Digit3),
    ("4", KeyCode::Digit4),
    ("5", KeyCode::Digit5),
    ("6", KeyCode::Digit6),
    ("7", KeyCode::Digit7),
    ("8", KeyCode::Digit8),
    ("9", KeyCode::Digit9),
    ("ArrowUp", KeyCode::ArrowUp),
    ("ArrowDown", KeyCode::ArrowDown),
    ("ArrowLeft", KeyCode::ArrowLeft),
    ("ArrowRight", KeyCode::ArrowRight),
    ("Space", KeyCode::Space),
    ("Enter", KeyCode::Enter),
    ("Tab", KeyCode::Tab),
    ("Escape", KeyCode::Escape),
    ("Backspace", KeyCode::Backspace),
    ("ShiftLeft", KeyCode::ShiftLeft),
    ("ShiftRight", KeyCode::ShiftRight),
    ("ControlLeft", KeyCode::ControlLeft),
    ("ControlRight", KeyCode::ControlRight),
    ("AltLeft", KeyCode::AltLeft),
    ("AltRight", KeyCode::AltRight),
];

/// Converts a canonical key name into a Bevy [`KeyCode`].
pub fn keycode_from_name(name: &str) -> Option<KeyCode> {
    KEY_TABLE
        .iter()
        .find(|(key, _)| *key == name)
        .map(|(_, code)| *code)
}

/// Keybinds resolved to [`KeyCode`]s, ready for the input systems.
#[derive(Resource)]
pub struct Bindings {
    codes: Vec<Option<KeyCode>>,
    names: Vec<String>,
}

impl Bindings {
    pub fn from_config(config: &Keybinds) -> Self {
        Self {
            codes: config
                .keys
                .iter()
                .map(|name| keycode_from_name(name))
                .collect(),
            names: config.keys.clone(),
        }
    }

    pub fn get(&self, index: usize) -> Option<KeyCode> {
        self.codes.get(index).copied().flatten()
    }

    /// True while the key bound to `index` is held.
    pub fn pressed(&self, keys: &ButtonInput<KeyCode>, index: usize) -> bool {
        self.get(index).is_some_and(|code| keys.pressed(code))
    }

    /// True on the frame the key bound to `index` was pressed.
    pub fn just_pressed(&self, keys: &ButtonInput<KeyCode>, index: usize) -> bool {
        self.get(index).is_some_and(|code| keys.just_pressed(code))
    }

    /// The human-readable name of a bound key (used by the HUD).
    pub fn name(&self, index: usize) -> &str {
        self.names.get(index).map(String::as_str).unwrap_or("?")
    }
}

/// Tracks whether the player has moved the mouse yet. Mouse-aim stays neutral
/// until then, so the aircraft doesn't react to wherever the OS cursor happens
/// to be sitting when the window opens.
#[derive(Resource, Default)]
pub struct MouseAim {
    pub engaged: bool,
}

/// System set for the flight simulation. Other systems (such as the camera) can
/// order themselves after it so they see the aircraft's final transform.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct FlightSet;

pub struct FlightPlugin;

impl Plugin for FlightPlugin {
    fn build(&self, app: &mut App) {
        let config = Keybinds::load_or_create();
        app.insert_resource(Bindings::from_config(&config))
            .init_resource::<MouseAim>()
            .add_systems(
                Update,
                (
                    read_player_input,
                    reset_aircraft,
                    flight_dynamics.after(read_player_input),
                )
                    .in_set(FlightSet),
            );
    }
}

/// Angle-of-attack -> lift coefficient, with a post-stall drop-off.
fn lift_coefficient(alpha: f32, spec: &AircraftSpec) -> f32 {
    let stall = spec.stall_aoa;
    if alpha.abs() <= stall {
        (spec.cl_slope * alpha).clamp(-spec.cl_max, spec.cl_max)
    } else {
        // Past the stall the wing keeps some lift but loses most of it.
        let stall_cl = (spec.cl_slope * stall).min(spec.cl_max);
        let overshoot = ((alpha.abs() - stall) / stall).clamp(0.0, 1.0);
        alpha.signum() * stall_cl * (1.0 - 0.75 * overshoot)
    }
}

/// Engine power fraction at altitude: full below the critical altitude, then
/// falling linearly to zero over the falloff height (supercharger behaviour).
fn engine_power_factor(altitude: f32, spec: &AircraftSpec) -> f32 {
    if altitude <= spec.critical_altitude {
        1.0
    } else {
        (1.0 - (altitude - spec.critical_altitude) / spec.altitude_power_falloff).clamp(0.0, 1.0)
    }
}

/// Control authority factor. It grows with airspeed (mushy controls when slow)
/// and then stiffens at high indicated airspeed / Mach, like the real
/// compressibility that makes the controls lock up in a dive. Near the limit the
/// surfaces are almost useless — the classic "nothing works, ride it down".
fn control_authority(ias: f32, mach: f32, spec: &AircraftSpec) -> f32 {
    let base = (ias / spec.control_ref_speed).clamp(0.0, 1.0);
    let ias_stiffen = ((ias - spec.stiffening_onset_ias)
        / (spec.max_ias - spec.stiffening_onset_ias).max(1.0))
    .clamp(0.0, 1.0);
    let mach_stiffen = ((mach - 0.45) / (spec.stiffening_mach - 0.45).max(0.01)).clamp(0.0, 1.0);
    // Up to an 85% loss: the controls do not merely get heavy, they lock.
    base * (1.0 - 0.85 * ias_stiffen.max(mach_stiffen))
}

/// Reads keyboard + mouse and writes the pilot's control inputs.
///
/// War Thunder style mouse aim: the aircraft points wherever the cursor is on
/// screen. On top of that the instructor performs the same jobs as War Thunder's:
/// it banks into the turn and pulls so the nose follows the pointer, levels the
/// wings when the pointer is centred, keeps the wing off the critical angle of
/// attack, eases off near the structural g limit, trims out the propeller torque
/// and auto-trims the pitch to hold the current trajectory. Keyboard
/// pitch/roll/yaw, throttle, flaps and WEP work as manual overrides.
fn read_player_input(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    bindings: Res<Bindings>,
    mouse_motion: Res<AccumulatedMouseMotion>,
    mut mouse_aim: ResMut<MouseAim>,
    menu: Res<GameMenu>,
    free_look: Res<FreeLook>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), With<ChaseCamera>>,
    mut query: Query<(&Transform, &mut Aircraft), With<PlayerControlled>>,
) {
    let Ok((transform, mut aircraft)) = query.single_mut() else {
        return;
    };

    // While the in-game menu is open the world keeps simulating, but the player
    // is not steering: hand the controls back to the instructor (neutral).
    if menu.open {
        aircraft.controls = Controls::default();
        return;
    }

    // A blacked-out or redded-out pilot cannot move the controls. The aircraft
    // keeps flying on its trim (and unloads), so the pilot comes back round.
    if aircraft.blackout >= 1.0 || aircraft.redout >= 1.0 {
        aircraft.controls = Controls::default();
        return;
    }

    let dt = time.delta_secs();
    let spec = aircraft.spec.clone();

    // Don't act on the mouse until the player actually moves it, so the aircraft
    // doesn't lurch toward wherever the OS cursor happens to be at launch.
    // While free-looking the mouse drives the camera, not the aircraft.
    if !free_look.active && !mouse_aim.engaged && mouse_motion.delta != Vec2::ZERO {
        mouse_aim.engaged = true;
    }

    // --- Throttle ---
    let mut throttle = aircraft.throttle;
    if bindings.pressed(&keys, THROTTLE_UP) {
        throttle += 0.7 * dt;
    }
    if bindings.pressed(&keys, THROTTLE_DOWN) {
        throttle -= 0.7 * dt;
    }
    aircraft.throttle = throttle.clamp(0.0, 1.0);

    // --- Flaps ---
    if bindings.just_pressed(&keys, FLAPS_DOWN) {
        aircraft.flaps = aircraft.flaps.more();
    }
    if bindings.just_pressed(&keys, FLAPS_UP) {
        aircraft.flaps = aircraft.flaps.less();
    }

    // --- War emergency power (cuts out if it overheats; must cool to re-engage) ---
    let want_wep = bindings.pressed(&keys, WEP);
    aircraft.wep = if aircraft.wep {
        want_wep && aircraft.wep_heat < 0.999
    } else {
        want_wep && aircraft.wep_heat < 0.5
    };

    // --- Keyboard pitch / roll / yaw (manual override) ---
    let mut keyboard_pitch = 0.0;
    let mut keyboard_roll: f32 = 0.0;
    let mut keyboard_yaw = 0.0;
    if bindings.pressed(&keys, PITCH_UP) || bindings.pressed(&keys, PITCH_UP_ALT) {
        keyboard_pitch += 1.0;
    }
    if bindings.pressed(&keys, PITCH_DOWN) || bindings.pressed(&keys, PITCH_DOWN_ALT) {
        keyboard_pitch -= 1.0;
    }
    if bindings.pressed(&keys, ROLL_LEFT) {
        keyboard_roll -= 1.0;
    }
    if bindings.pressed(&keys, ROLL_RIGHT) {
        keyboard_roll += 1.0;
    }
    if bindings.pressed(&keys, YAW_LEFT) {
        keyboard_yaw -= 1.0;
    }
    if bindings.pressed(&keys, YAW_RIGHT) {
        keyboard_yaw += 1.0;
    }

    // --- Pointer aim: point the nose at the cursor (War Thunder style) ---
    let mut aim_pitch = 0.0;
    let mut aim_roll = 0.0;
    let mut aim_yaw = 0.0;
    if mouse_aim.engaged && !free_look.active {
        if let (Ok(window), Ok((camera, camera_transform))) = (windows.single(), cameras.single()) {
            if let Some(cursor) = window.cursor_position() {
                if let Ok(ray) = camera.viewport_to_world(camera_transform, cursor) {
                    let bank = current_bank(transform.rotation);
                    (aim_pitch, aim_roll, aim_yaw) =
                        aim_controls(transform.rotation, *ray.direction, bank);
                }
            }
        }
    }

    // --- Instructor limits: never pull into a stall or past the g limit ---
    let mut pitch = keyboard_pitch + aim_pitch;
    if pitch > 0.0 {
        let stall_margin =
            ((spec.stall_aoa - aircraft.alpha) / (spec.stall_aoa * 0.6)).clamp(0.0, 1.0);
        let g_margin = ((spec.g_limit - aircraft.g_load) / (spec.g_limit * 0.3)).clamp(0.0, 1.0);
        pitch *= stall_margin.min(g_margin);
    } else if pitch < 0.0 {
        let stall_margin =
            ((spec.stall_aoa + aircraft.alpha) / (spec.stall_aoa * 0.6)).clamp(0.0, 1.0);
        pitch *= stall_margin;
    }

    // A/D always give full manual roll authority; otherwise the instructor rolls.
    let mut roll = if keyboard_roll.abs() > 0.01 {
        keyboard_roll
    } else {
        aim_roll
    };
    // The instructor trims out the propeller torque with a small counter-roll.
    let torque = prop_torque(
        spec.prop_torque,
        aircraft.throttle,
        aircraft.wep,
        spec.wep_multiplier,
        aircraft.ias,
    );
    roll += torque / spec.roll_rate;

    aircraft.controls = Controls {
        pitch: pitch.clamp(-1.0, 1.0),
        roll: roll.clamp(-1.0, 1.0),
        yaw: (keyboard_yaw + aim_yaw).clamp(-1.0, 1.0),
    };
}

/// Propeller torque-roll rate (rad/s): strongest at high power and low speed.
fn prop_torque(coefficient: f32, throttle: f32, wep: bool, wep_multiplier: f32, ias: f32) -> f32 {
    let wep_factor = if wep { wep_multiplier } else { 1.0 };
    coefficient * throttle * wep_factor * (1.0 - (ias / 150.0).clamp(0.0, 1.0))
}

/// Current bank angle in radians; positive means rolled right (right wing down).
fn current_bank(rotation: Quat) -> f32 {
    let right = rotation * Vec3::X;
    let up = rotation * Vec3::Y;
    (-right.dot(Vec3::Y)).atan2(up.dot(Vec3::Y))
}

/// Instructor that points the nose at `desired_dir` (world space): it banks
/// toward the target and pulls, then levels the wings once the nose is on
/// target. Returns `(pitch, roll, yaw)` control inputs.
fn aim_controls(rotation: Quat, desired_dir: Vec3, bank: f32) -> (f32, f32, f32) {
    // Where the target is, expressed in the aircraft's body frame.
    let local = rotation.inverse() * desired_dir.normalize_or_zero();
    // Angle of the target above/below the nose and left/right of it.
    let elevation = local.y.atan2(-local.z);
    let azimuth = local.x.atan2(-local.z);

    // Bank angle that would turn the nose toward the target. Because it goes to
    // zero as the target comes onto the nose, the wings level out on target with
    // no steady-state aim error.
    let desired_bank = (azimuth * 2.5).clamp(-1.4, 1.4);
    let roll = ((desired_bank - bank) * 3.0).clamp(-1.0, 1.0);

    let pitch = (elevation * 2.5).clamp(-1.0, 1.0);
    let yaw = (azimuth * 0.3).clamp(-1.0, 1.0);
    (pitch, roll, yaw)
}

/// Put the aircraft back in the air at the start position.
fn reset_aircraft(
    keys: Res<ButtonInput<KeyCode>>,
    bindings: Res<Bindings>,
    menu: Res<GameMenu>,
    mut query: Query<(&mut Transform, &mut Aircraft), With<PlayerControlled>>,
) {
    if menu.open || !bindings.just_pressed(&keys, RESET) {
        return;
    }
    let Ok((mut transform, mut aircraft)) = query.single_mut() else {
        return;
    };
    *transform = Transform::from_translation(START_POSITION);
    aircraft.velocity = Vec3::NEG_Z * aircraft.spec.cruise_speed;
    aircraft.angular_velocity = Vec3::ZERO;
    aircraft.throttle = 0.8;
    aircraft.controls = Controls::default();
    aircraft.wep = false;
    aircraft.wep_heat = 0.0;
    aircraft.flaps = FlapSetting::Up;
    aircraft.flap_position = 0.0;
    aircraft.trim_alpha = aircraft.spec.trim_alpha;
    aircraft.blackout = 0.0;
    aircraft.redout = 0.0;
    aircraft.stamina = 1.0;
    aircraft.ammo = aircraft.spec.guns.iter().map(|gun| gun.ammo).collect();
    aircraft.fire_timer = vec![0.0; aircraft.spec.guns.len()];
}

/// Integrates the equations of motion for every player-controlled aircraft.
fn flight_dynamics(
    time: Res<Time>,
    mut query: Query<(&mut Transform, &mut Aircraft, &mut DamageModel), With<PlayerControlled>>,
) {
    // Clamp dt so a hitch can't blow up the simulation.
    let dt = time.delta_secs().min(0.05);
    if dt <= 0.0 {
        return;
    }
    let Ok((mut transform, mut aircraft, mut damage)) = query.single_mut() else {
        return;
    };

    let spec = aircraft.spec.clone();
    let rotation = transform.rotation;
    let forward = Aircraft::forward(rotation);
    let up = rotation * Vec3::Y;
    let right = rotation * Vec3::X;

    let velocity = aircraft.velocity;
    let speed = velocity.length();

    // --- Damage effects ---
    let engine_health = damage.integrity(AircraftPart::Engine);
    // A destroyed fuselage means the airframe is finished: no thrust, no
    // control, and a lot of drag, so it goes down like in War Thunder.
    let destroyed = damage.is_destroyed(AircraftPart::Fuselage);
    if destroyed {
        aircraft.controls = Controls::default();
    }
    let wing_health = 0.5
        * (damage.integrity(AircraftPart::LeftWing) + damage.integrity(AircraftPart::RightWing));
    let tail_health = damage.integrity(AircraftPart::Tail);

    // --- Atmosphere ---
    let altitude = transform.translation.y.max(0.0);
    let air_density = 1.225 * (-altitude / 8500.0).exp();
    let temperature = (288.15 - 0.0065 * altitude).max(216.65);
    let speed_of_sound = (1.4 * 287.05 * temperature).sqrt();
    let mach = speed / speed_of_sound;
    // Indicated airspeed: what the pilot's ASI shows and what the limits use.
    let ias = speed * (air_density / 1.225).sqrt();
    aircraft.ias = ias;

    // --- Airflow angles (in the body frame) ---
    let body_velocity = rotation.inverse() * velocity;
    let (alpha, beta) = if speed > 1.0 {
        (
            (-body_velocity.y).atan2(-body_velocity.z),
            body_velocity.x.atan2(-body_velocity.z),
        )
    } else {
        (0.0, 0.0)
    };
    aircraft.alpha = alpha;
    aircraft.beta = beta;
    aircraft.airspeed = speed;

    // --- Flaps: move toward the selected setting, auto-retracting if overspeed ---
    let over_flap_limit = match aircraft.flaps {
        FlapSetting::Up => false,
        FlapSetting::Combat => ias > spec.flap_speed_limits[0],
        FlapSetting::Takeoff => ias > spec.flap_speed_limits[1],
        FlapSetting::Landing => ias > spec.flap_speed_limits[2],
    };
    let target_flap = if over_flap_limit {
        0.0
    } else {
        aircraft.flaps.factor()
    };
    let flap_step = 2.5 * dt;
    aircraft.flap_position += (target_flap - aircraft.flap_position).clamp(-flap_step, flap_step);
    let flap = aircraft.flap_position;

    // --- Aerodynamic coefficients ---
    let aspect_ratio = spec.wing_span * spec.wing_span / spec.wing_area;
    let cl = lift_coefficient(alpha, &spec) + spec.cl_flap * flap;
    let induced_drag = cl * cl / (PI * spec.oswald * aspect_ratio);
    // Transonic drag rise near the Mach limit. Kept gentle enough that a dive
    // can reach the high-Mach regime (where the controls stiffen) before the
    // wall stops it.
    let mach_drag = if mach > 0.55 {
        (mach - 0.55).powi(2) * 6.0
    } else {
        0.0
    };
    let cd = spec.cd0
        + induced_drag
        + spec.cd_flap * flap
        + mach_drag
        + if destroyed { 0.6 } else { 0.0 };
    let dynamic_pressure = 0.5 * air_density * speed * speed;

    // --- Engine: power falls off above the critical altitude; WEP adds thrust ---
    let power_factor = engine_power_factor(altitude, &spec);
    let wep_factor = if aircraft.wep {
        spec.wep_multiplier
    } else {
        1.0
    };
    let available_power = spec.max_power
        * power_factor
        * wep_factor
        * engine_health
        * if destroyed { 0.0 } else { 1.0 };
    // Propeller thrust: power / speed, capped at the static-thrust figure.
    let thrust_power = spec.prop_efficiency * available_power / speed.max(25.0);
    let thrust = forward * (aircraft.throttle * thrust_power.min(spec.static_thrust));

    // --- Forces ---
    let lift_direction = (up - forward * up.dot(forward)).normalize_or_zero();
    let drag_direction = -velocity.normalize_or_zero();

    let lift =
        lift_direction * (dynamic_pressure * spec.wing_area * cl * (0.3 + 0.7 * wing_health));
    let drag = drag_direction * (dynamic_pressure * spec.wing_area * cd);
    let gravity = Vec3::NEG_Y * (spec.mass * 9.81);
    let side_force = right * (-beta * dynamic_pressure * spec.wing_area * 0.4 * tail_health);

    let acceleration = (lift + drag + thrust + gravity + side_force) / spec.mass;
    aircraft.velocity += acceleration * dt;
    transform.translation += aircraft.velocity * dt;

    // --- Rotation ---
    // Authority grows with airspeed, then stiffens (compresses) at high IAS/Mach:
    // the classic "the controls lock up in a dive" of Air Realistic.
    let control_authority = control_authority(ias, mach, &spec);

    let controls = aircraft.controls;
    let mut target_rates = Vec3::new(
        controls.pitch * spec.pitch_rate,
        -controls.yaw * spec.yaw_rate,
        -controls.roll * spec.roll_rate,
    ) * control_authority;

    // Weathervane stability: pitch toward the (instructor) trim angle of attack
    // and yaw the nose into the airflow. The `*_damping` terms are rate feedback
    // that *subtract from the target rate*, so a nonzero value divides the
    // steady-state rate (roll_damping = 2 would cut the roll rate to a third).
    // They are all zero: the `responsiveness` lag below already damps the rates.
    let damping_authority = (ias / spec.control_ref_speed).clamp(0.0, 1.0);
    target_rates.x += -spec.pitch_stability * (alpha - aircraft.trim_alpha) * control_authority;
    target_rates.x -= spec.pitch_damping * aircraft.angular_velocity.x * damping_authority;
    target_rates.y += -spec.yaw_stability * beta * control_authority * (0.4 + 0.6 * tail_health);
    target_rates.y -= spec.yaw_damping * aircraft.angular_velocity.y * damping_authority;
    target_rates.z -= spec.roll_damping * aircraft.angular_velocity.z * damping_authority;
    // Propeller torque-roll (the instructor trims most of it out upstream).
    target_rates.z += prop_torque(
        spec.prop_torque,
        aircraft.throttle,
        aircraft.wep,
        spec.wep_multiplier,
        ias,
    );

    let responsiveness = spec.responsiveness * (0.35 + 0.65 * tail_health);
    let blend = (responsiveness * dt).clamp(0.0, 1.0);
    aircraft.angular_velocity = aircraft.angular_velocity.lerp(target_rates, blend);

    let delta_rotation = Quat::from_scaled_axis(aircraft.angular_velocity * dt);
    transform.rotation = (transform.rotation * delta_rotation).normalize();

    // --- Telemetry ---
    // Signed load factor along the body-up axis: positive when pulling, negative
    // when pushing, so the pilot model can tell blackout from redout.
    let body_up = rotation * Vec3::Y;
    aircraft.g_load = lift.dot(body_up) / (spec.mass * 9.81);

    // --- WEP heat: builds while used, cools otherwise ---
    if aircraft.wep {
        aircraft.wep_heat = (aircraft.wep_heat + dt / 25.0).min(1.0);
    } else {
        aircraft.wep_heat = (aircraft.wep_heat - dt / 20.0).max(0.0);
    }

    // --- Structural limits: over-g or over-speed damages the airframe ---
    let over_g = (aircraft.g_load.abs() - spec.g_limit).max(0.0);
    let over_ias = (ias - spec.max_ias).max(0.0);
    if over_g > 0.0 || over_ias > 0.0 {
        let amount = (over_g * 0.3 + over_ias * 0.01) * dt * 10.0;
        damage.apply_damage(AircraftPart::LeftWing, amount);
        damage.apply_damage(AircraftPart::RightWing, amount);
    }

    // --- Crude ground interaction: don't fall through the map ---
    let ground_level = 0.8;
    if transform.translation.y < ground_level {
        transform.translation.y = ground_level;
        if aircraft.velocity.y < 0.0 {
            aircraft.velocity.y = 0.0;
        }
        aircraft.velocity.x *= 0.99;
        aircraft.velocity.z *= 0.99;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use openthunder::keybinds::Keybinds;

    /// The Corsair, built from the built-in default config.
    fn corsair() -> AircraftSpec {
        AircraftSpec::from_config(&openthunder::plane_config::default_planes()[0])
    }

    /// Headless app containing just the input system and one aircraft, banked by
    /// `bank_radians` to the right.
    fn input_app(bank_radians: f32) -> (App, Entity) {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<AccumulatedMouseMotion>()
            .init_resource::<MouseAim>()
            .init_resource::<crate::menu::GameMenu>()
            .init_resource::<crate::camera::FreeLook>()
            .insert_resource(Bindings::from_config(&Keybinds::default()))
            .add_systems(Update, read_player_input);

        let entity = app
            .world_mut()
            .spawn((
                Transform::from_rotation(Quat::from_rotation_z(-bank_radians)),
                Aircraft::new(corsair()),
                PlayerControlled,
            ))
            .id();
        (app, entity)
    }

    fn roll_of(app: &App, entity: Entity) -> f32 {
        app.world().get::<Aircraft>(entity).unwrap().controls.roll
    }

    fn press_and_update(app: &mut App, key: KeyCode) {
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(key);
        app.update();
    }

    /// Regression test: the old code added the auto-level instructor on top of
    /// the keyboard roll, so at a 0.5 rad bank holding `D` produced
    /// `1 - 0.5*2.5 = -0.25` and the aircraft could never roll past ~23 deg.
    #[test]
    fn holding_roll_has_full_authority_while_banked() {
        let (mut app, entity) = input_app(0.5);
        press_and_update(&mut app, KeyCode::KeyD);
        assert!(
            roll_of(&app, entity) > 0.99,
            "D should command full right roll even while banked, got {}",
            roll_of(&app, entity)
        );
    }

    #[test]
    fn roll_keys_map_to_the_correct_direction() {
        let (mut app, entity) = input_app(0.0);
        press_and_update(&mut app, KeyCode::KeyD);
        assert!(roll_of(&app, entity) > 0.0, "D should roll right");

        let (mut app, entity) = input_app(0.0);
        press_and_update(&mut app, KeyCode::KeyA);
        assert!(roll_of(&app, entity) < 0.0, "A should roll left");
    }

    #[test]
    fn aim_controls_command_nothing_when_on_target() {
        let (pitch, roll, yaw) = aim_controls(Quat::IDENTITY, Vec3::NEG_Z, 0.0);
        assert!(pitch.abs() < 1e-3, "pitch {pitch}");
        assert!(roll.abs() < 1e-3, "roll {roll}");
        assert!(yaw.abs() < 1e-3, "yaw {yaw}");
    }

    #[test]
    fn aim_controls_bank_toward_a_target_to_the_right() {
        // A direction 30 degrees to the right of the nose.
        let target = Quat::from_rotation_y(-0.5) * Vec3::NEG_Z;
        let (pitch, roll, yaw) = aim_controls(Quat::IDENTITY, target, 0.0);
        assert!(
            roll > 0.0,
            "target to the right should roll right, got {roll}"
        );
        assert!(yaw > 0.0, "target to the right should yaw right, got {yaw}");
        assert!(
            pitch.abs() < 0.2,
            "level target should not pitch, got {pitch}"
        );
    }

    #[test]
    fn aim_controls_level_the_wings_when_banked_and_on_target() {
        let rotation = Quat::from_rotation_z(-0.6); // banked right
        let forward = rotation * Vec3::NEG_Z; // rolling does not change forward
        let (_, roll, _) = aim_controls(rotation, forward, current_bank(rotation));
        assert!(
            roll < 0.0,
            "on-target but banked right should roll left to level, got {roll}"
        );
    }

    #[test]
    fn current_bank_is_positive_for_a_right_bank() {
        assert!(current_bank(Quat::from_rotation_z(-0.6)) > 0.0);
        assert!(current_bank(Quat::from_rotation_z(0.6)) < 0.0);
    }

    #[test]
    fn engine_makes_full_power_below_critical_altitude() {
        let spec = corsair();
        assert!((engine_power_factor(0.0, &spec) - 1.0).abs() < 1e-6);
        assert!((engine_power_factor(6_000.0, &spec) - 1.0).abs() < 1e-6);
        let high = engine_power_factor(9_000.0, &spec);
        assert!(
            high > 0.0 && high < 1.0,
            "power should fall off, got {high}"
        );
    }

    #[test]
    fn controls_stiffen_at_high_speed() {
        let spec = corsair();
        let cruise = control_authority(120.0, 0.35, &spec);
        let redline = control_authority(spec.max_ias, 0.80, &spec);
        assert!(cruise > 0.9, "cruise authority {cruise}");
        assert!(redline < 0.4, "redline authority {redline}");
        assert!(redline < cruise);
    }

    #[test]
    fn controls_are_mushy_when_slow() {
        let spec = corsair();
        assert!(control_authority(30.0, 0.1, &spec) < 0.4);
    }

    #[test]
    fn propeller_torque_fades_with_speed() {
        let spec = corsair();
        let slow = prop_torque(spec.prop_torque, 1.0, false, spec.wep_multiplier, 20.0);
        let fast = prop_torque(spec.prop_torque, 1.0, false, spec.wep_multiplier, 200.0);
        assert!(slow > 0.0);
        assert!(
            fast.abs() < 1e-6,
            "torque should vanish at speed, got {fast}"
        );
        let wep = prop_torque(spec.prop_torque, 1.0, true, spec.wep_multiplier, 20.0);
        assert!(wep > slow, "WEP should increase torque");
    }

    /// Approximate War Thunder Air RB roll rates: the Corsair rolls best, the
    /// Bf 109 worst, and everyone stiffens up badly near the redline.
    #[test]
    fn roll_rates_are_realistic() {
        let spec = |name: &str| {
            AircraftSpec::from_config(
                &openthunder::plane_config::default_planes()
                    .into_iter()
                    .find(|config| config.name == name)
                    .unwrap(),
            )
        };
        let roll = |spec: &AircraftSpec, ias: f32| {
            (spec.roll_rate * control_authority(ias, ias / 320.0, spec)).to_degrees()
        };

        let corsair = spec("F4U-4 Corsair");
        let spitfire = spec("Spitfire F Mk IXc");
        let bf109 = spec("Bf 109 G-6");

        for plane in [&corsair, &spitfire, &bf109] {
            let rate = roll(plane, 150.0);
            assert!(
                (50.0..=140.0).contains(&rate),
                "{} cruise roll {rate:.0}°/s out of range",
                plane.name
            );
        }
        assert!(roll(&corsair, 150.0) > roll(&spitfire, 150.0));
        assert!(roll(&spitfire, 150.0) > roll(&bf109, 150.0));

        for plane in [&corsair, &spitfire, &bf109] {
            let fast = roll(plane, 235.0);
            assert!(
                fast < roll(plane, 150.0) * 0.6,
                "{} should stiffen near the redline, got {fast:.0}°/s",
                plane.name
            );
        }
    }

    #[test]
    fn flap_limits_are_ordered_combat_takeoff_landing() {
        let spec = corsair();
        assert!(spec.cl_flap > 0.0 && spec.cd_flap > 0.0);
        assert!(spec.flap_speed_limits[0] > spec.flap_speed_limits[1]);
        assert!(spec.flap_speed_limits[1] > spec.flap_speed_limits[2]);
    }

    #[test]
    fn alt_pitch_keys_control_pitch() {
        let pitch_of =
            |app: &App, entity: Entity| app.world().get::<Aircraft>(entity).unwrap().controls.pitch;

        let (mut app, entity) = input_app(0.0);
        press_and_update(&mut app, KeyCode::ControlLeft);
        assert!(pitch_of(&app, entity) > 0.0, "Ctrl should pitch up");

        let (mut app, entity) = input_app(0.0);
        press_and_update(&mut app, KeyCode::ShiftLeft);
        assert!(pitch_of(&app, entity) < 0.0, "Shift should pitch down");
    }

    #[test]
    fn open_menu_neutralizes_player_controls() {
        let (mut app, entity) = input_app(0.0);
        app.world_mut().resource_mut::<crate::menu::GameMenu>().open = true;
        press_and_update(&mut app, KeyCode::KeyD);
        assert_eq!(
            roll_of(&app, entity),
            0.0,
            "the menu should take over the controls"
        );
    }

    /// Drives `flight_dynamics` for a fixed number of 20 ms steps with a fixed
    /// pitch input and returns the final signed load factor.
    fn run_pull(pitch: f32, steps: usize) -> f32 {
        let mut app = App::new();
        app.insert_resource(Time::<()>::default());
        app.add_systems(Update, flight_dynamics);

        let mut aircraft = Aircraft::new(corsair());
        aircraft.velocity = Vec3::NEG_Z * aircraft.spec.cruise_speed;
        aircraft.controls.pitch = pitch;
        let entity = app
            .world_mut()
            .spawn((
                Transform::from_translation(Vec3::new(0.0, 1000.0, 0.0)),
                aircraft,
                DamageModel::new(100.0),
                PlayerControlled,
            ))
            .id();

        for _ in 0..steps {
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(std::time::Duration::from_secs_f32(0.02));
            app.update();
        }
        app.world().get::<Aircraft>(entity).unwrap().g_load
    }

    /// The pilot model needs a signed load factor: pulling is positive, pushing
    /// negative. `lift.length()` used to hide the sign.
    #[test]
    fn load_factor_is_signed_for_pull_and_push() {
        assert!(
            run_pull(0.6, 60) > 1.5,
            "pulling should give positive g, got {}",
            run_pull(0.6, 60)
        );
        assert!(
            run_pull(-1.0, 60) < -0.5,
            "pushing should give negative g, got {}",
            run_pull(-1.0, 60)
        );
    }

    /// Peak g reached by a full pull at `speed` (m/s) over `steps` 20 ms steps.
    fn peak_g_at(speed: f32, steps: usize) -> f32 {
        let mut app = App::new();
        app.insert_resource(Time::<()>::default());
        app.add_systems(Update, flight_dynamics);
        let mut aircraft = Aircraft::new(corsair());
        aircraft.velocity = Vec3::NEG_Z * speed;
        aircraft.controls.pitch = 1.0;
        let entity = app
            .world_mut()
            .spawn((
                Transform::from_translation(Vec3::new(0.0, 1000.0, 0.0)),
                aircraft,
                DamageModel::new(100.0),
                PlayerControlled,
            ))
            .id();
        let mut peak = 0.0f32;
        for _ in 0..steps {
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(std::time::Duration::from_secs_f32(0.02));
            app.update();
            peak = peak.max(app.world().get::<Aircraft>(entity).unwrap().g_load);
        }
        peak
    }

    /// A hard pull at combat speed should load the wing past the pilot's ~6.5 g
    /// tolerance, so blackout is actually reachable in a fight.
    #[test]
    fn a_hard_pull_exceeds_the_pilot_g_tolerance() {
        let peak = peak_g_at(160.0, 200);
        assert!(
            peak > crate::pilot::CrewSkills::default().g_tolerance,
            "hard pull only reached {peak:.1} g"
        );
    }

    /// A dive should build enough speed for the controls to stiffen — the
    /// classic "ride it down" of Air RB.
    #[test]
    fn a_dive_stiffens_the_controls() {
        let mut app = App::new();
        app.insert_resource(Time::<()>::default());
        app.add_systems(Update, flight_dynamics);
        let mut aircraft = Aircraft::new(corsair());
        aircraft.velocity = Vec3::NEG_Z * 150.0;
        aircraft.throttle = 1.0;
        aircraft.controls.pitch = -0.5;
        let entity = app
            .world_mut()
            .spawn((
                Transform::from_translation(Vec3::new(0.0, 8000.0, 0.0)),
                aircraft,
                DamageModel::new(100.0),
                PlayerControlled,
            ))
            .id();
        let mut min_authority = 1.0f32;
        for _ in 0..600 {
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(std::time::Duration::from_secs_f32(0.05));
            app.update();
            let ac = app.world().get::<Aircraft>(entity).unwrap();
            let alt = app
                .world()
                .get::<Transform>(entity)
                .unwrap()
                .translation
                .y
                .max(0.0);
            let temp = (288.15 - 0.0065 * alt).max(216.65);
            let mach = ac.airspeed / (1.4 * 287.05 * temp).sqrt();
            min_authority = min_authority.min(control_authority(ac.ias, mach, &ac.spec));
        }
        assert!(
            min_authority < 0.6,
            "a dive should stiffen the controls; minimum authority was {min_authority:.2}"
        );
    }

    /// Steady-state roll rate (deg/s) for a full roll input at a given speed,
    /// holding the speed with a simple throttle autopilot. Returns `(rate, ias)`.
    fn steady_roll_rate(spec: AircraftSpec, speed: f32) -> (f32, f32) {
        let mut app = App::new();
        app.insert_resource(Time::<()>::default());
        app.add_systems(Update, flight_dynamics);
        let mut aircraft = Aircraft::new(spec);
        aircraft.velocity = Vec3::NEG_Z * speed;
        aircraft.throttle = 0.8;
        aircraft.controls.roll = 1.0;
        let entity = app
            .world_mut()
            .spawn((
                Transform::from_translation(Vec3::new(0.0, 1000.0, 0.0)),
                aircraft,
                DamageModel::new(100.0),
                PlayerControlled,
            ))
            .id();
        let mut rate = 0.0f32;
        for _ in 0..100 {
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(std::time::Duration::from_secs_f32(0.02));
            app.update();
            let ac = app.world().get::<Aircraft>(entity).unwrap();
            rate = ac.angular_velocity.z;
            // Crude speed hold.
            let throttle = (ac.throttle + (speed - ac.airspeed) * 0.02).clamp(0.0, 1.0);
            app.world_mut()
                .get_mut::<Aircraft>(entity)
                .unwrap()
                .throttle = throttle;
        }
        let ac = app.world().get::<Aircraft>(entity).unwrap();
        (rate.abs().to_degrees(), ac.ias)
    }

    fn spec_named(name: &str) -> AircraftSpec {
        AircraftSpec::from_config(
            &openthunder::plane_config::default_planes()
                .into_iter()
                .find(|config| config.name == name)
                .unwrap(),
        )
    }

    /// Roll must behave sensibly across the speed range: mush at low speed,
    /// peak around cruise, and stiffen toward the redline. This caught the old
    /// `roll_damping` bug, which divided the achieved rate by three and made the
    /// curve nearly flat.
    #[test]
    fn roll_rate_peaks_at_cruise_and_stiffens_with_speed() {
        for name in ["F4U-4 Corsair", "Bf 109 G-6", "Spitfire F Mk IXc"] {
            let spec = spec_named(name);
            let slow = steady_roll_rate(spec.clone(), 70.0).0;
            let cruise = steady_roll_rate(spec.clone(), 140.0).0;
            let fast = steady_roll_rate(spec.clone(), 230.0).0;

            // The cruise rate should be close to the plane's rated roll rate.
            let rated = spec.roll_rate.to_degrees();
            assert!(
                cruise > rated * 0.7 && cruise < rated * 1.1,
                "{name}: cruise roll {cruise:.0} deg/s is not near rated {rated:.0}"
            );
            assert!(
                slow < cruise * 0.8,
                "{name}: low-speed roll {slow:.0} should be well below cruise {cruise:.0}"
            );
            assert!(
                fast < cruise * 0.7,
                "{name}: high-speed roll {fast:.0} should stiffen below cruise {cruise:.0}"
            );
        }
    }
}
