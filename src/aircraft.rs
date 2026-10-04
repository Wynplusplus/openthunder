//! Aircraft types, runtime state, and spawning.
//!
//! # Adding a new plane
//!
//! 1. Write a function returning an [`AircraftSpec`] (copy [`f4u_4_corsair`] as a
//!    template and change the numbers).
//! 2. Add it to the `specs` list in [`AircraftPlugin`].
//! 3. Spawn it with [`spawn_aircraft`].
//!
//! The visual model is generated from the spec (wing span, colour, ...), so a new
//! plane is *just data*. Nothing in the flight or damage code is plane-specific.

use bevy::prelude::*;

use openthunder::planes;
use openthunder::settings::Settings;

use crate::damage::DamageModel;

/// Physical + aerodynamic description of an aircraft type.
///
/// Units are SI unless noted: metres, kilograms, newtons, watts, seconds, radians.
#[derive(Clone, Debug)]
pub struct AircraftSpec {
    pub name: &'static str,

    // --- Mass & geometry ---
    /// Loaded mass in kg.
    pub mass: f32,
    /// Wing planform area in m^2.
    pub wing_area: f32,
    /// Tip-to-tip wing span in m (also used for the visual model and aspect ratio).
    pub wing_span: f32,

    // --- Propulsion ---
    /// Shaft power at full throttle, sea level, in watts.
    pub max_power: f32,
    /// Maximum static thrust (the low-speed cap on propeller thrust), newtons.
    pub static_thrust: f32,
    /// Propeller efficiency (0..1).
    pub prop_efficiency: f32,
    /// Thrust multiplier when using war emergency power.
    pub wep_multiplier: f32,
    /// Altitude (m) below which the engine makes full power.
    pub critical_altitude: f32,
    /// Height (m) over which power falls to zero above the critical altitude.
    pub altitude_power_falloff: f32,

    // --- Lift / drag ---
    /// Lift-curve slope, per radian.
    pub cl_slope: f32,
    /// Maximum lift coefficient (clamp before stall modelling).
    pub cl_max: f32,
    /// Angle of attack (radians) at which the wing stalls.
    pub stall_aoa: f32,
    /// Zero-lift (parasitic) drag coefficient.
    pub cd0: f32,
    /// Oswald span efficiency factor.
    pub oswald: f32,
    /// Extra lift coefficient with full flaps.
    pub cl_flap: f32,
    /// Extra drag coefficient with full flaps.
    pub cd_flap: f32,

    // --- Handling ---
    /// Angle of attack (radians) the aircraft naturally trims to hands-off.
    pub trim_alpha: f32,
    /// Maximum pitch rate at full deflection, rad/s.
    pub pitch_rate: f32,
    /// Maximum yaw (rudder) rate, rad/s.
    pub yaw_rate: f32,
    /// Maximum roll rate, rad/s.
    pub roll_rate: f32,
    /// Pitch (weathervane) stability gain.
    pub pitch_stability: f32,
    /// Yaw (weathervane) stability gain.
    pub yaw_stability: f32,
    /// Airspeed (m/s) at which control surfaces reach full authority.
    pub control_ref_speed: f32,
    /// How quickly the aircraft reaches a commanded rotation rate.
    pub responsiveness: f32,
    /// Pitch-rate damping (short-period damping), per second.
    pub pitch_damping: f32,
    /// Yaw-rate damping, per second.
    pub yaw_damping: f32,
    /// Roll-rate damping, per second.
    pub roll_damping: f32,
    /// Suggested cruise speed for spawning, m/s.
    pub cruise_speed: f32,

    // --- Limits (Air RB "realistic" behaviour) ---
    /// Indicated airspeed (m/s) at which control stiffening begins.
    pub stiffening_onset_ias: f32,
    /// Mach number at which stiffening is at its worst.
    pub stiffening_mach: f32,
    /// Never-exceed indicated airspeed, m/s (structural).
    pub max_ias: f32,
    /// Positive structural g limit.
    pub g_limit: f32,
    /// Propeller torque-roll coefficient.
    pub prop_torque: f32,
    /// IAS (m/s) flap limits for the combat / takeoff / landing settings.
    pub flap_speed_limits: [f32; 3],

    // --- Damage ---
    /// Hit points given to every section.
    pub max_health: f32,

    // --- Looks ---
    pub body_color: Color,
}

/// Registry of every aircraft type the game knows about.
///
/// Inserted as a resource at startup. Look types up by name.
#[derive(Resource, Default)]
pub struct AircraftRegistry {
    pub specs: Vec<AircraftSpec>,
}

impl AircraftRegistry {
    pub fn get(&self, name: &str) -> Option<&AircraftSpec> {
        self.specs.iter().find(|spec| spec.name == name)
    }

    pub fn names(&self) -> Vec<&'static str> {
        self.specs.iter().map(|spec| spec.name).collect()
    }
}

/// Control surface inputs, each in `[-1.0, 1.0]`.
#[derive(Clone, Copy, Debug, Default)]
pub struct Controls {
    /// +1 = nose up.
    pub pitch: f32,
    /// +1 = roll right.
    pub roll: f32,
    /// +1 = nose right.
    pub yaw: f32,
}

/// Flap position, as in War Thunder's combat / takeoff / landing settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlapSetting {
    Up,
    Combat,
    Takeoff,
    Landing,
}

impl FlapSetting {
    /// 0.0 (up) .. 1.0 (full landing flaps).
    pub fn factor(self) -> f32 {
        match self {
            FlapSetting::Up => 0.0,
            FlapSetting::Combat => 0.33,
            FlapSetting::Takeoff => 0.66,
            FlapSetting::Landing => 1.0,
        }
    }

    /// One step more flaps.
    pub fn more(self) -> Self {
        match self {
            FlapSetting::Up => FlapSetting::Combat,
            FlapSetting::Combat => FlapSetting::Takeoff,
            FlapSetting::Takeoff | FlapSetting::Landing => FlapSetting::Landing,
        }
    }

    /// One step fewer flaps.
    pub fn less(self) -> Self {
        match self {
            FlapSetting::Landing => FlapSetting::Takeoff,
            FlapSetting::Takeoff => FlapSetting::Combat,
            FlapSetting::Combat | FlapSetting::Up => FlapSetting::Up,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            FlapSetting::Up => "up",
            FlapSetting::Combat => "combat",
            FlapSetting::Takeoff => "takeoff",
            FlapSetting::Landing => "landing",
        }
    }
}

/// Runtime state of a flying aircraft.
#[derive(Component, Clone, Debug)]
pub struct Aircraft {
    pub spec: AircraftSpec,

    /// Velocity in world space, m/s.
    pub velocity: Vec3,
    /// Body angular velocity in rad/s, ordered `(pitch about +X, yaw about +Y, roll about +Z)`.
    pub angular_velocity: Vec3,
    /// Throttle, `0.0..=1.0`.
    pub throttle: f32,
    /// Current control inputs.
    pub controls: Controls,

    /// War emergency power engaged.
    pub wep: bool,
    /// WEP heat, `0.0..=1.0`; at 1.0 WEP cuts out until it cools.
    pub wep_heat: f32,
    /// Selected flap setting.
    pub flaps: FlapSetting,
    /// Actual flap position, `0.0..=1.0` (moves toward the selected setting).
    pub flap_position: f32,
    /// Instructor pitch trim: the angle of attack held when hands-off.
    pub trim_alpha: f32,

    // --- Derived, refreshed every physics step (used by the HUD) ---
    pub airspeed: f32,
    /// Indicated airspeed, m/s.
    pub ias: f32,
    /// Angle of attack, radians.
    pub alpha: f32,
    /// Sideslip angle, radians.
    pub beta: f32,
    /// Approximate load factor in g.
    pub g_load: f32,
}

impl Aircraft {
    pub fn new(spec: AircraftSpec) -> Self {
        let trim_alpha = spec.trim_alpha;
        Self {
            spec,
            velocity: Vec3::ZERO,
            angular_velocity: Vec3::ZERO,
            throttle: 0.8,
            controls: Controls::default(),
            wep: false,
            wep_heat: 0.0,
            flaps: FlapSetting::Up,
            flap_position: 0.0,
            trim_alpha,
            airspeed: 0.0,
            ias: 0.0,
            alpha: 0.0,
            beta: 0.0,
            g_load: 1.0,
        }
    }

    /// World-space forward direction (the nose), for a given orientation.
    pub fn forward(rotation: Quat) -> Vec3 {
        rotation * Vec3::NEG_Z
    }
}

/// Marks the aircraft the player is currently controlling.
#[derive(Component)]
pub struct PlayerControlled;

/// Marks a spinning propeller visual.
#[derive(Component)]
pub struct Propeller {
    pub angle: f32,
}

/// Where a freshly spawned aircraft appears.
pub const START_POSITION: Vec3 = Vec3::new(0.0, 1000.0, 0.0);

/// The aircraft type that ships with the prototype.
///
/// Figures are taken from the F4U-4's real data sheet (R-2800-18W, ~2450 hp with
/// WEP, ~711 km/h at 9,000 m, ~18 m/s climb, +11/-4 g, 885 km/h IAS redline).
pub fn f4u_4_corsair() -> AircraftSpec {
    AircraftSpec {
        name: "F4U-4 Corsair",

        mass: 6_000.0,
        wing_area: 29.2,
        wing_span: 12.5,

        // R-2800-18W: ~2200 hp military, ~2450 hp WEP.
        max_power: 1_640_000.0,
        static_thrust: 18_000.0,
        prop_efficiency: 0.82,
        wep_multiplier: 1.15,
        critical_altitude: 7_000.0,
        altitude_power_falloff: 6_000.0,

        cl_slope: 4.5,
        cl_max: 1.4,
        stall_aoa: 0.28, // ~16 degrees
        cd0: 0.025,
        oswald: 0.80,
        cl_flap: 0.5,
        cd_flap: 0.08,

        // Chosen so that lift balances weight near the cruise speed.
        trim_alpha: 0.042, // ~2.4 degrees
        pitch_rate: 1.1,
        yaw_rate: 0.5,
        roll_rate: 3.0, // the Corsair is a fast roller
        pitch_stability: 2.2,
        yaw_stability: 3.0,
        control_ref_speed: 120.0,
        responsiveness: 6.0,
        pitch_damping: 0.0,
        yaw_damping: 0.0,
        roll_damping: 2.0,
        cruise_speed: 150.0,

        // Control stiffening above ~576 km/h IAS, worst near the 885 km/h redline.
        stiffening_onset_ias: 160.0,
        stiffening_mach: 0.78,
        max_ias: 246.0,
        g_limit: 11.0,
        prop_torque: 0.35,
        flap_speed_limits: [388.0 / 3.6, 299.0 / 3.6, 253.0 / 3.6],

        max_health: 100.0,

        body_color: Color::srgb(0.13, 0.19, 0.42), // US Navy dark blue
    }
}

/// Messerschmitt Bf 109 G-6. Data sheet: DB-605AM, ~669 km/h at 5,500 m,
/// ~19.6 m/s climb, 20 s turn, +13/-6 g, 790 km/h IAS redline.
pub fn bf109_g6() -> AircraftSpec {
    AircraftSpec {
        name: "Bf 109 G-6",

        mass: 3_150.0,
        wing_area: 16.1,
        wing_span: 9.9,

        max_power: 1_100_000.0,
        static_thrust: 13_000.0,
        prop_efficiency: 0.80,
        wep_multiplier: 1.12, // MW-50
        critical_altitude: 5_800.0,
        altitude_power_falloff: 5_000.0,

        cl_slope: 4.5,
        cl_max: 1.4,
        stall_aoa: 0.28,
        cd0: 0.027,
        oswald: 0.80,
        cl_flap: 0.5,
        cd_flap: 0.08,

        trim_alpha: 0.035,
        pitch_rate: 1.2,
        yaw_rate: 0.5,
        roll_rate: 2.5,
        pitch_stability: 2.2,
        yaw_stability: 3.0,
        control_ref_speed: 120.0,
        responsiveness: 6.0,
        pitch_damping: 0.0,
        yaw_damping: 0.0,
        roll_damping: 2.0,
        cruise_speed: 150.0,

        stiffening_onset_ias: 150.0,
        stiffening_mach: 0.78,
        max_ias: 790.0 / 3.6,
        g_limit: 13.0,
        prop_torque: 0.4,
        flap_speed_limits: [438.0 / 3.6, 409.0 / 3.6, 260.0 / 3.6],

        max_health: 100.0,

        body_color: Color::srgb(0.44, 0.46, 0.43), // Luftwaffe grey
    }
}

/// Supermarine Spitfire F Mk IXc. Data sheet: Merlin-61, ~642 km/h at 8,537 m,
/// ~18.9 m/s climb, 17.2 s turn, +10/-5 g, 774 km/h IAS redline.
pub fn spitfire_mk9() -> AircraftSpec {
    AircraftSpec {
        name: "Spitfire F Mk IXc",

        mass: 3_400.0,
        wing_area: 22.48,
        wing_span: 11.23,

        max_power: 1_167_000.0,
        static_thrust: 13_000.0,
        prop_efficiency: 0.82,
        wep_multiplier: 1.15,
        critical_altitude: 6_500.0,
        altitude_power_falloff: 6_000.0,

        cl_slope: 4.5,
        cl_max: 1.5, // elliptical wing
        stall_aoa: 0.28,
        cd0: 0.025,
        oswald: 0.85, // elliptical wing -> high span efficiency
        cl_flap: 0.55,
        cd_flap: 0.09,

        trim_alpha: 0.029,
        pitch_rate: 1.3,
        yaw_rate: 0.55,
        roll_rate: 2.6,
        pitch_stability: 2.2,
        yaw_stability: 3.0,
        control_ref_speed: 115.0,
        responsiveness: 6.5,
        pitch_damping: 0.0,
        yaw_damping: 0.0,
        roll_damping: 2.0,
        cruise_speed: 145.0,

        stiffening_onset_ias: 145.0,
        stiffening_mach: 0.80,
        max_ias: 774.0 / 3.6,
        g_limit: 10.0,
        prop_torque: 0.3,
        // The Mk IX had a single flap position; use the same limit for all.
        flap_speed_limits: [260.0 / 3.6, 260.0 / 3.6, 260.0 / 3.6],

        max_health: 100.0,

        body_color: Color::srgb(0.24, 0.30, 0.20), // RAF dark green
    }
}

pub struct AircraftPlugin;

impl Plugin for AircraftPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(AircraftRegistry {
            // Add new aircraft types here (and to `openthunder::planes::PLANES`).
            specs: vec![f4u_4_corsair(), bf109_g6(), spitfire_mk9()],
        })
        .add_systems(Startup, (log_aircraft_types, spawn_player_aircraft))
        .add_systems(Update, spin_propellers);
    }
}

/// Logs the available aircraft types so it is obvious where to add new ones.
fn log_aircraft_types(registry: Res<AircraftRegistry>) {
    info!("Registered aircraft: {}", registry.names().join(", "));
}

fn spawn_player_aircraft(
    mut commands: Commands,
    registry: Res<AircraftRegistry>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Aircraft is chosen by `--plane <id>` on the command line (used by the
    // launcher), otherwise by the saved settings, otherwise the default.
    let requested = plane_arg().unwrap_or_else(|| Settings::load_or_create().plane);

    let spec = registry
        .get(requested.as_str())
        .or_else(|| registry.get(planes::default_plane()))
        .expect("the default aircraft spec must be registered")
        .clone();
    info!("Flying: {}", spec.name);

    let mut aircraft = Aircraft::new(spec);
    // Start already flying so the player is immediately airborne.
    aircraft.velocity = Vec3::NEG_Z * aircraft.spec.cruise_speed;

    let entity = spawn_aircraft(
        &mut commands,
        &mut meshes,
        &mut materials,
        aircraft,
        Transform::from_translation(START_POSITION),
    );
    commands.entity(entity).insert(PlayerControlled);
}

/// Reads `--plane <id>` from the command line, if present.
fn plane_arg() -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    args.iter()
        .position(|arg| arg == "--plane")
        .and_then(|index| args.get(index + 1).cloned())
}

/// Spawns an aircraft (root entity + visual children) and returns the root.
///
/// The visual model is built from primitives and scaled from the spec, so no
/// per-plane model code is needed to add a new type.
pub fn spawn_aircraft(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    aircraft: Aircraft,
    transform: Transform,
) -> Entity {
    let spec = aircraft.spec.clone();

    let root = commands
        .spawn((
            transform,
            Visibility::default(),
            aircraft,
            DamageModel::new(spec.max_health),
        ))
        .id();

    spawn_aircraft_model(commands, meshes, materials, &spec, root);
    root
}

/// Builds the visual model for `spec` as children of `root`.
///
/// Used for both the player's aircraft and remote (multiplayer) ones.
pub fn spawn_aircraft_model(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    spec: &AircraftSpec,
    root: Entity,
) {
    // Shared materials.
    let body = materials.add(StandardMaterial {
        base_color: spec.body_color,
        perceptual_roughness: 0.65,
        metallic: 0.15,
        ..default()
    });
    let dark = materials.add(StandardMaterial {
        base_color: Color::srgb(0.12, 0.13, 0.15),
        perceptual_roughness: 0.5,
        ..default()
    });
    let glass = materials.add(StandardMaterial {
        base_color: Color::srgba(0.35, 0.55, 0.65, 0.55),
        perceptual_roughness: 0.1,
        alpha_mode: AlphaMode::Blend,
        ..default()
    });

    // Shared meshes, sized from the spec.
    let fuselage = meshes.add(Cuboid::new(1.1, 1.1, 8.0));
    let wing = meshes.add(Cuboid::new(spec.wing_span, 0.22, 2.0));
    let tailplane = meshes.add(Cuboid::new(3.4, 0.18, 1.0));
    let fin = meshes.add(Cuboid::new(0.18, 1.4, 1.2));
    let canopy = meshes.add(Sphere::new(0.55));
    let propeller = meshes.add(Cuboid::new(0.14, 3.0, 0.12));

    commands.entity(root).with_children(|parent| {
        parent.spawn((
            Mesh3d(fuselage),
            MeshMaterial3d(body.clone()),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        parent.spawn((
            Mesh3d(wing),
            MeshMaterial3d(body.clone()),
            Transform::from_xyz(0.0, 0.15, 0.4),
        ));
        parent.spawn((
            Mesh3d(tailplane),
            MeshMaterial3d(body.clone()),
            Transform::from_xyz(0.0, 0.3, 3.5),
        ));
        parent.spawn((
            Mesh3d(fin),
            MeshMaterial3d(body.clone()),
            Transform::from_xyz(0.0, 0.95, 3.7),
        ));
        parent.spawn((
            Mesh3d(canopy),
            MeshMaterial3d(glass),
            Transform::from_xyz(0.0, 0.7, -1.4),
        ));
        parent.spawn((
            Mesh3d(propeller),
            MeshMaterial3d(dark),
            Transform::from_xyz(0.0, 0.0, -4.2),
            Propeller { angle: 0.0 },
        ));
    });
}

/// Spin the propeller visual at a rate proportional to engine throttle.
fn spin_propellers(
    time: Res<Time>,
    player: Query<&Aircraft, With<PlayerControlled>>,
    mut propellers: Query<(&mut Transform, &mut Propeller)>,
) {
    let throttle = player.single().map(|ac| ac.throttle).unwrap_or(0.0);
    let spin_rate = 30.0 + throttle * 80.0; // rad/s (purely cosmetic)
    for (mut transform, mut propeller) in &mut propellers {
        propeller.angle += spin_rate * time.delta_secs();
        transform.rotation = Quat::from_rotation_z(propeller.angle);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openthunder::planes::PLANES;

    /// Every plane offered by the launcher must have a spec in the game registry,
    /// and vice versa.
    #[test]
    fn registry_matches_plane_list() {
        let specs = vec![f4u_4_corsair(), bf109_g6(), spitfire_mk9()];
        let registry = AircraftRegistry {
            specs: specs.clone(),
        };
        for plane in PLANES {
            assert!(
                registry.get(plane.id).is_some(),
                "PLANES lists '{}' but the registry has no spec for it",
                plane.id
            );
        }
        assert_eq!(specs.len(), PLANES.len(), "registry/PLANES length mismatch");
    }

    /// Sanity-check that each aircraft's numbers are physically reasonable.
    #[test]
    fn aircraft_specs_are_sane() {
        for spec in [f4u_4_corsair(), bf109_g6(), spitfire_mk9()] {
            assert!(spec.mass > 2_000.0 && spec.mass < 8_000.0, "{}", spec.name);
            assert!(spec.max_power > 500_000.0, "{}", spec.name);
            assert!(spec.g_limit >= 8.0, "{}", spec.name);
            assert!(spec.max_ias > 150.0, "{}", spec.name);
            assert!(
                spec.flap_speed_limits[0] >= spec.flap_speed_limits[1]
                    && spec.flap_speed_limits[1] >= spec.flap_speed_limits[2],
                "{}",
                spec.name
            );
        }
    }
}
