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

use openthunder::plane_config;
use openthunder::plane_config::PlaneConfig;

use crate::damage::DamageModel;

/// A gun group: one or more muzzles firing the same round.
#[derive(Clone, Debug)]
pub struct GunSpec {
    pub name: String,
    pub caliber_mm: f32,
    /// Rounds per second, per muzzle.
    pub rounds_per_second: f32,
    /// Muzzle velocity, m/s.
    pub muzzle_velocity: f32,
    /// Damage applied to a hit section.
    pub damage: f32,
    /// Cone of fire, radians.
    pub spread: f32,
    /// Muzzle positions in the aircraft's local frame.
    pub muzzles: Vec<Vec3>,
    /// Total rounds carried (shared across this gun's muzzles).
    pub ammo: u32,
}

/// Physical + aerodynamic description of an aircraft type.
///
/// Units are SI unless noted: metres, kilograms, newtons, watts, seconds, radians.
#[derive(Clone, Debug)]
pub struct AircraftSpec {
    pub name: String,

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
    /// Whether this engine has war emergency power (water injection / boost).
    pub has_wep: bool,
    /// Power multiplier while war emergency power is engaged.
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

    // --- Armament ---
    /// Offensive guns.
    pub guns: Vec<GunSpec>,

    // --- Looks / model ---
    pub body_color: Color,
    /// Fuselage length (m).
    pub length: f32,
    /// Wing chord (m).
    pub wing_chord: f32,
    /// Tailplane span (m).
    pub tail_span: f32,
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

    pub fn names(&self) -> Vec<&str> {
        self.specs.iter().map(|spec| spec.name.as_str()).collect()
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

    // --- Landing gear ---
    /// Whether the gear is selected down.
    pub gear_down: bool,
    /// Gear position, `0.0` (up) .. `1.0` (down).
    pub gear_position: f32,
    /// Whether the wheels are on the ground.
    pub on_ground: bool,
    /// Instructor pitch trim: the angle of attack held when hands-off.
    pub trim_alpha: f32,

    /// Remaining rounds per gun (see `spec.guns`).
    pub ammo: Vec<u32>,
    /// Time until each gun can fire again, seconds.
    pub fire_timer: Vec<f32>,

    // --- Derived, refreshed every physics step (used by the HUD) ---
    pub airspeed: f32,
    /// Indicated airspeed, m/s.
    pub ias: f32,
    /// Angle of attack, radians.
    pub alpha: f32,
    /// Sideslip angle, radians.
    pub beta: f32,
    /// Signed load factor in g (positive = pulling, negative = pushing).
    pub g_load: f32,

    // --- Pilot physiology (see `pilot.rs`) ---
    /// Positive-g blackout veil, `0.0` (clear) .. `1.0` (unconscious).
    pub blackout: f32,
    /// Negative-g redout veil, `0.0` (clear) .. `1.0` (unconscious).
    pub redout: f32,
    /// Pilot stamina, `1.0` (fresh) .. `0.0` (exhausted). Low stamina lowers
    /// the pilot's g tolerance.
    pub stamina: f32,
}

impl Aircraft {
    pub fn new(spec: AircraftSpec) -> Self {
        let trim_alpha = spec.trim_alpha;
        let ammo = spec.guns.iter().map(|gun| gun.ammo).collect();
        let fire_timer = vec![0.0; spec.guns.len()];
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
            gear_down: false,
            gear_position: 0.0,
            on_ground: false,
            trim_alpha,
            ammo,
            fire_timer,
            airspeed: 0.0,
            ias: 0.0,
            alpha: 0.0,
            beta: 0.0,
            g_load: 1.0,
            blackout: 0.0,
            redout: 0.0,
            stamina: 1.0,
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

/// Builds a runtime [`AircraftSpec`] from a plane config — either one of the
/// built-in defaults or one received from a server.
impl AircraftSpec {
    pub fn from_config(config: &PlaneConfig) -> Self {
        Self {
            name: config.name.clone(),
            mass: config.mass,
            wing_area: config.wing_area,
            wing_span: config.wing_span,
            max_power: config.max_power,
            static_thrust: config.static_thrust,
            prop_efficiency: config.prop_efficiency,
            has_wep: config.has_wep,
            wep_multiplier: config.wep_multiplier,
            critical_altitude: config.critical_altitude,
            altitude_power_falloff: config.altitude_power_falloff,
            cl_slope: config.cl_slope,
            cl_max: config.cl_max,
            stall_aoa: config.stall_aoa,
            cd0: config.cd0,
            oswald: config.oswald,
            cl_flap: config.cl_flap,
            cd_flap: config.cd_flap,
            trim_alpha: config.trim_alpha,
            pitch_rate: config.pitch_rate,
            yaw_rate: config.yaw_rate,
            roll_rate: config.roll_rate,
            pitch_stability: config.pitch_stability,
            yaw_stability: config.yaw_stability,
            control_ref_speed: config.control_ref_speed,
            responsiveness: config.responsiveness,
            pitch_damping: config.pitch_damping,
            yaw_damping: config.yaw_damping,
            roll_damping: config.roll_damping,
            cruise_speed: config.cruise_speed,
            stiffening_onset_ias: config.stiffening_onset_ias,
            stiffening_mach: config.stiffening_mach,
            max_ias: config.max_ias,
            g_limit: config.g_limit,
            prop_torque: config.prop_torque,
            flap_speed_limits: config.flap_speed_limits,
            max_health: config.max_health,
            guns: config
                .guns
                .iter()
                .map(|gun| GunSpec {
                    name: gun.name.clone(),
                    caliber_mm: gun.caliber_mm,
                    rounds_per_second: gun.rounds_per_second,
                    muzzle_velocity: gun.muzzle_velocity,
                    damage: gun.damage,
                    spread: gun.spread,
                    muzzles: gun
                        .muzzles
                        .iter()
                        .map(|muzzle| Vec3::new(muzzle[0], muzzle[1], muzzle[2]))
                        .collect(),
                    ammo: gun.ammo,
                })
                .collect(),
            body_color: Color::srgb(
                config.body_color[0],
                config.body_color[1],
                config.body_color[2],
            ),
            length: config.length,
            wing_chord: config.wing_chord,
            tail_span: config.tail_span,
        }
    }
}

pub struct AircraftPlugin;

impl Plugin for AircraftPlugin {
    fn build(&self, app: &mut App) {
        // Built-in planes; replaced by the server's planes when connected.
        let specs = plane_config::default_planes()
            .iter()
            .map(AircraftSpec::from_config)
            .collect();
        app.insert_resource(AircraftRegistry { specs })
            .add_systems(Startup, log_aircraft_types)
            .add_systems(Update, (spin_propellers, update_gear_visual));
    }
}

/// Logs the available aircraft types so it is obvious where to add new ones.
fn log_aircraft_types(registry: Res<AircraftRegistry>) {
    info!("Registered aircraft: {}", registry.names().join(", "));
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

    // Shared meshes, sized from the plane's model config.
    let half_length = (spec.length * 0.5).max(1.0);
    let fuselage = meshes.add(Cuboid::new(1.1, 1.1, spec.length.max(2.0)));
    let wing = meshes.add(Cuboid::new(spec.wing_span, 0.22, spec.wing_chord.max(0.5)));
    let tailplane = meshes.add(Cuboid::new(spec.tail_span.max(1.0), 0.18, 1.0));
    let fin = meshes.add(Cuboid::new(0.18, 1.4, 1.2));
    let canopy = meshes.add(Sphere::new(0.55));
    let blade = meshes.add(Cuboid::new(0.10, 1.5, 0.10));
    let spinner = meshes.add(Sphere::new(0.22));
    let strut = meshes.add(Cuboid::new(0.14, 0.9, 0.14));
    let wheel = meshes.add(Cylinder::new(0.28, 0.18));

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
            Transform::from_xyz(0.0, 0.3, half_length - 0.8),
        ));
        parent.spawn((
            Mesh3d(fin),
            MeshMaterial3d(body.clone()),
            Transform::from_xyz(0.0, 0.95, half_length - 0.6),
        ));
        parent.spawn((
            Mesh3d(canopy),
            MeshMaterial3d(glass),
            Transform::from_xyz(0.0, 0.7, -half_length * 0.35),
        ));
        parent
            .spawn((
                Transform::from_xyz(0.0, 0.0, -(half_length + 0.2)),
                Visibility::default(),
                Propeller { angle: 0.0 },
            ))
            .with_children(|hub| {
                // Spinner cone in the middle of the propeller.
                hub.spawn((
                    Mesh3d(spinner.clone()),
                    MeshMaterial3d(dark.clone()),
                    Transform::default(),
                ));
                // Three blades radiating from the hub.
                for index in 0..3 {
                    let rotation =
                        Quat::from_rotation_z(index as f32 * std::f32::consts::TAU / 3.0);
                    hub.spawn((
                        Mesh3d(blade.clone()),
                        MeshMaterial3d(dark.clone()),
                        Transform::from_rotation(rotation)
                            .with_translation(rotation * Vec3::new(0.0, 0.75, 0.0)),
                    ));
                }
            });

        // --- Landing gear: two main legs with wheels (extend / retract) ---
        parent
            .spawn((Transform::default(), Visibility::default(), GearVisual))
            .with_children(|gear| {
                for side in [-1.0f32, 1.0] {
                    gear.spawn((
                        Mesh3d(strut.clone()),
                        MeshMaterial3d(dark.clone()),
                        Transform::from_xyz(side * 0.9, -0.7, 0.0),
                    ));
                    gear.spawn((
                        Mesh3d(wheel.clone()),
                        MeshMaterial3d(dark.clone()),
                        Transform::from_xyz(side * 0.9, -1.35, 0.0)
                            .with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2)),
                    ));
                }
            });
    });
}

/// Marks the retractable landing-gear group of an aircraft model.
#[derive(Component)]
pub struct GearVisual;

/// Extend or retract the gear group as the aircraft's gear position changes.
fn update_gear_visual(
    aircraft: Query<&Aircraft>,
    mut gear: Query<(&ChildOf, &mut Transform, &mut Visibility), With<GearVisual>>,
) {
    for (child_of, mut transform, mut visibility) in &mut gear {
        // Remote players do not network their gear, so default to down.
        let position = aircraft
            .get(child_of.parent())
            .map(|aircraft| aircraft.gear_position)
            .unwrap_or(1.0);
        *visibility = if position < 0.1 {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
        // Slide the legs up into the fuselage as they retract.
        transform.translation.y = 1.2 * (1.0 - position);
    }
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
        // Wrap the angle so it never loses precision during long flights.
        propeller.angle =
            (propeller.angle + spin_rate * time.delta_secs()).rem_euclid(std::f32::consts::TAU);
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
        let specs: Vec<AircraftSpec> = plane_config::default_planes()
            .iter()
            .map(AircraftSpec::from_config)
            .collect();
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
        for config in plane_config::default_planes() {
            let spec = AircraftSpec::from_config(&config);
            assert!(spec.mass > 2_000.0 && spec.mass < 8_000.0, "{}", spec.name);
            assert!(spec.max_power > 500_000.0, "{}", spec.name);
            assert!(spec.g_limit >= 8.0, "{}", spec.name);
            assert!(spec.max_ias > 150.0, "{}", spec.name);
            assert!(!spec.guns.is_empty(), "{}", spec.name);
            assert!(
                spec.flap_speed_limits[0] >= spec.flap_speed_limits[1]
                    && spec.flap_speed_limits[1] >= spec.flap_speed_limits[2],
                "{}",
                spec.name
            );
        }
    }
}
