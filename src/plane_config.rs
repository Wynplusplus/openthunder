//! Plane definitions.
//!
//! A plane is a directory on the server (`planes/<id>/plane.conf`) holding its
//! flight-model configuration and its (procedural) model parameters. The same
//! `key = value` format with `[gun N]` sections is used by the client's built-in
//! defaults, so a server can add planes just by dropping in new directories and
//! clients pick them up on connect.
//!
//! Units are SI: metres, kilograms, newtons, watts, seconds, radians.

use core::fmt::Write;

/// One gun group.
#[derive(Clone, Debug, PartialEq)]
pub struct GunConfig {
    pub name: String,
    pub caliber_mm: f32,
    pub rounds_per_second: f32,
    pub muzzle_velocity: f32,
    pub damage: f32,
    pub spread: f32,
    pub muzzles: Vec<[f32; 3]>,
    pub ammo: u32,
}

impl Default for GunConfig {
    fn default() -> Self {
        Self {
            name: "Gun".to_string(),
            caliber_mm: 12.7,
            rounds_per_second: 10.0,
            muzzle_velocity: 800.0,
            damage: 4.0,
            spread: 0.003,
            muzzles: Vec::new(),
            ammo: 500,
        }
    }
}

/// A complete plane definition.
#[derive(Clone, Debug, PartialEq)]
pub struct PlaneConfig {
    /// Directory id.
    pub id: String,
    /// Display name (also the id used on the wire).
    pub name: String,

    // --- Flight model ---
    pub mass: f32,
    pub wing_area: f32,
    pub wing_span: f32,
    pub max_power: f32,
    pub static_thrust: f32,
    pub prop_efficiency: f32,
    pub wep_multiplier: f32,
    pub critical_altitude: f32,
    pub altitude_power_falloff: f32,
    pub cl_slope: f32,
    pub cl_max: f32,
    pub stall_aoa: f32,
    pub cd0: f32,
    pub oswald: f32,
    pub cl_flap: f32,
    pub cd_flap: f32,
    pub trim_alpha: f32,
    pub pitch_rate: f32,
    pub yaw_rate: f32,
    pub roll_rate: f32,
    pub pitch_stability: f32,
    pub yaw_stability: f32,
    pub control_ref_speed: f32,
    pub responsiveness: f32,
    pub pitch_damping: f32,
    pub yaw_damping: f32,
    pub roll_damping: f32,
    pub cruise_speed: f32,
    pub stiffening_onset_ias: f32,
    pub stiffening_mach: f32,
    pub max_ias: f32,
    pub g_limit: f32,
    pub prop_torque: f32,
    pub flap_speed_limits: [f32; 3],
    pub max_health: f32,

    // --- Model (procedural; a mesh path could be added later) ---
    pub body_color: [f32; 3],
    pub length: f32,
    pub wing_chord: f32,
    pub tail_span: f32,

    // --- Armament ---
    pub guns: Vec<GunConfig>,
}

impl Default for PlaneConfig {
    fn default() -> Self {
        Self {
            id: "plane".to_string(),
            name: "Plane".to_string(),
            mass: 3_000.0,
            wing_area: 20.0,
            wing_span: 10.0,
            max_power: 1_000_000.0,
            static_thrust: 12_000.0,
            prop_efficiency: 0.8,
            wep_multiplier: 1.1,
            critical_altitude: 6_000.0,
            altitude_power_falloff: 5_000.0,
            cl_slope: 4.5,
            cl_max: 1.4,
            stall_aoa: 0.28,
            cd0: 0.026,
            oswald: 0.8,
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
            max_ias: 220.0,
            g_limit: 11.0,
            prop_torque: 0.35,
            flap_speed_limits: [100.0, 90.0, 75.0],
            max_health: 100.0,
            body_color: [0.4, 0.45, 0.4],
            length: 9.5,
            wing_chord: 2.0,
            tail_span: 3.4,
            guns: vec![GunConfig::default()],
        }
    }
}

fn parse_f32(text: &str) -> Option<f32> {
    text.trim().parse().ok()
}

fn parse_vec3(text: &str) -> Option<[f32; 3]> {
    let parts: Vec<f32> = text
        .split_whitespace()
        .filter_map(|p| p.parse().ok())
        .collect();
    if parts.len() == 3 {
        Some([parts[0], parts[1], parts[2]])
    } else {
        None
    }
}

impl PlaneConfig {
    /// Parse a `plane.conf` file. Unknown keys are ignored; missing keys keep
    /// their defaults.
    pub fn parse(id: &str, text: &str) -> Self {
        let mut plane = PlaneConfig {
            id: id.to_string(),
            ..Default::default()
        };
        let mut current_gun: Option<usize> = None;

        for raw in text.lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            // Section header: `[gun N]`.
            if let Some(inner) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                if let Some(rest) = inner.trim().strip_prefix("gun") {
                    let index: usize = rest.trim().parse().unwrap_or(plane.guns.len());
                    while plane.guns.len() <= index {
                        plane.guns.push(GunConfig::default());
                    }
                    current_gun = Some(index);
                }
                continue;
            }

            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim();
            let value = value.trim();

            if let Some(index) = current_gun {
                let gun = &mut plane.guns[index];
                match key {
                    "name" => gun.name = value.to_string(),
                    "caliber_mm" => gun.caliber_mm = parse_f32(value).unwrap_or(gun.caliber_mm),
                    "rounds_per_second" => {
                        gun.rounds_per_second = parse_f32(value).unwrap_or(gun.rounds_per_second)
                    }
                    "muzzle_velocity" => {
                        gun.muzzle_velocity = parse_f32(value).unwrap_or(gun.muzzle_velocity)
                    }
                    "damage" => gun.damage = parse_f32(value).unwrap_or(gun.damage),
                    "spread" => gun.spread = parse_f32(value).unwrap_or(gun.spread),
                    "ammo" => gun.ammo = value.parse().unwrap_or(gun.ammo),
                    "muzzle" => {
                        if let Some(muzzle) = parse_vec3(value) {
                            gun.muzzles.push(muzzle);
                        }
                    }
                    _ => {}
                }
                continue;
            }

            match key {
                "name" => plane.name = value.to_string(),
                "mass" => plane.mass = parse_f32(value).unwrap_or(plane.mass),
                "wing_area" => plane.wing_area = parse_f32(value).unwrap_or(plane.wing_area),
                "wing_span" => plane.wing_span = parse_f32(value).unwrap_or(plane.wing_span),
                "max_power" => plane.max_power = parse_f32(value).unwrap_or(plane.max_power),
                "static_thrust" => {
                    plane.static_thrust = parse_f32(value).unwrap_or(plane.static_thrust)
                }
                "prop_efficiency" => {
                    plane.prop_efficiency = parse_f32(value).unwrap_or(plane.prop_efficiency)
                }
                "wep_multiplier" => {
                    plane.wep_multiplier = parse_f32(value).unwrap_or(plane.wep_multiplier)
                }
                "critical_altitude" => {
                    plane.critical_altitude = parse_f32(value).unwrap_or(plane.critical_altitude)
                }
                "altitude_power_falloff" => {
                    plane.altitude_power_falloff =
                        parse_f32(value).unwrap_or(plane.altitude_power_falloff)
                }
                "cl_slope" => plane.cl_slope = parse_f32(value).unwrap_or(plane.cl_slope),
                "cl_max" => plane.cl_max = parse_f32(value).unwrap_or(plane.cl_max),
                "stall_aoa" => plane.stall_aoa = parse_f32(value).unwrap_or(plane.stall_aoa),
                "cd0" => plane.cd0 = parse_f32(value).unwrap_or(plane.cd0),
                "oswald" => plane.oswald = parse_f32(value).unwrap_or(plane.oswald),
                "cl_flap" => plane.cl_flap = parse_f32(value).unwrap_or(plane.cl_flap),
                "cd_flap" => plane.cd_flap = parse_f32(value).unwrap_or(plane.cd_flap),
                "trim_alpha" => plane.trim_alpha = parse_f32(value).unwrap_or(plane.trim_alpha),
                "pitch_rate" => plane.pitch_rate = parse_f32(value).unwrap_or(plane.pitch_rate),
                "yaw_rate" => plane.yaw_rate = parse_f32(value).unwrap_or(plane.yaw_rate),
                "roll_rate" => plane.roll_rate = parse_f32(value).unwrap_or(plane.roll_rate),
                "pitch_stability" => {
                    plane.pitch_stability = parse_f32(value).unwrap_or(plane.pitch_stability)
                }
                "yaw_stability" => {
                    plane.yaw_stability = parse_f32(value).unwrap_or(plane.yaw_stability)
                }
                "control_ref_speed" => {
                    plane.control_ref_speed = parse_f32(value).unwrap_or(plane.control_ref_speed)
                }
                "responsiveness" => {
                    plane.responsiveness = parse_f32(value).unwrap_or(plane.responsiveness)
                }
                "pitch_damping" => {
                    plane.pitch_damping = parse_f32(value).unwrap_or(plane.pitch_damping)
                }
                "yaw_damping" => plane.yaw_damping = parse_f32(value).unwrap_or(plane.yaw_damping),
                "roll_damping" => {
                    plane.roll_damping = parse_f32(value).unwrap_or(plane.roll_damping)
                }
                "cruise_speed" => {
                    plane.cruise_speed = parse_f32(value).unwrap_or(plane.cruise_speed)
                }
                "stiffening_onset_ias" => {
                    plane.stiffening_onset_ias =
                        parse_f32(value).unwrap_or(plane.stiffening_onset_ias)
                }
                "stiffening_mach" => {
                    plane.stiffening_mach = parse_f32(value).unwrap_or(plane.stiffening_mach)
                }
                "max_ias" => plane.max_ias = parse_f32(value).unwrap_or(plane.max_ias),
                "g_limit" => plane.g_limit = parse_f32(value).unwrap_or(plane.g_limit),
                "prop_torque" => plane.prop_torque = parse_f32(value).unwrap_or(plane.prop_torque),
                "flap_speed_limits" => {
                    if let Some(limits) = parse_vec3(value) {
                        plane.flap_speed_limits = limits;
                    }
                }
                "max_health" => plane.max_health = parse_f32(value).unwrap_or(plane.max_health),
                "body_color" => {
                    if let Some(color) = parse_vec3(value) {
                        plane.body_color = color;
                    }
                }
                "length" => plane.length = parse_f32(value).unwrap_or(plane.length),
                "wing_chord" => plane.wing_chord = parse_f32(value).unwrap_or(plane.wing_chord),
                "tail_span" => plane.tail_span = parse_f32(value).unwrap_or(plane.tail_span),
                _ => {}
            }
        }
        plane
    }

    /// Serialize back to the `plane.conf` format (used to ship the built-in
    /// defaults to the server repo).
    pub fn to_config_string(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "# {} ({})", self.name, self.id);
        let _ = writeln!(out, "name = {}", self.name);
        let _ = writeln!(out, "mass = {}", self.mass);
        let _ = writeln!(out, "wing_area = {}", self.wing_area);
        let _ = writeln!(out, "wing_span = {}", self.wing_span);
        let _ = writeln!(out, "max_power = {}", self.max_power);
        let _ = writeln!(out, "static_thrust = {}", self.static_thrust);
        let _ = writeln!(out, "prop_efficiency = {}", self.prop_efficiency);
        let _ = writeln!(out, "wep_multiplier = {}", self.wep_multiplier);
        let _ = writeln!(out, "critical_altitude = {}", self.critical_altitude);
        let _ = writeln!(
            out,
            "altitude_power_falloff = {}",
            self.altitude_power_falloff
        );
        let _ = writeln!(out, "cl_slope = {}", self.cl_slope);
        let _ = writeln!(out, "cl_max = {}", self.cl_max);
        let _ = writeln!(out, "stall_aoa = {}", self.stall_aoa);
        let _ = writeln!(out, "cd0 = {}", self.cd0);
        let _ = writeln!(out, "oswald = {}", self.oswald);
        let _ = writeln!(out, "cl_flap = {}", self.cl_flap);
        let _ = writeln!(out, "cd_flap = {}", self.cd_flap);
        let _ = writeln!(out, "trim_alpha = {}", self.trim_alpha);
        let _ = writeln!(out, "pitch_rate = {}", self.pitch_rate);
        let _ = writeln!(out, "yaw_rate = {}", self.yaw_rate);
        let _ = writeln!(out, "roll_rate = {}", self.roll_rate);
        let _ = writeln!(out, "pitch_stability = {}", self.pitch_stability);
        let _ = writeln!(out, "yaw_stability = {}", self.yaw_stability);
        let _ = writeln!(out, "control_ref_speed = {}", self.control_ref_speed);
        let _ = writeln!(out, "responsiveness = {}", self.responsiveness);
        let _ = writeln!(out, "pitch_damping = {}", self.pitch_damping);
        let _ = writeln!(out, "yaw_damping = {}", self.yaw_damping);
        let _ = writeln!(out, "roll_damping = {}", self.roll_damping);
        let _ = writeln!(out, "cruise_speed = {}", self.cruise_speed);
        let _ = writeln!(out, "stiffening_onset_ias = {}", self.stiffening_onset_ias);
        let _ = writeln!(out, "stiffening_mach = {}", self.stiffening_mach);
        let _ = writeln!(out, "max_ias = {}", self.max_ias);
        let _ = writeln!(out, "g_limit = {}", self.g_limit);
        let _ = writeln!(out, "prop_torque = {}", self.prop_torque);
        let _ = writeln!(
            out,
            "flap_speed_limits = {} {} {}",
            self.flap_speed_limits[0], self.flap_speed_limits[1], self.flap_speed_limits[2]
        );
        let _ = writeln!(out, "max_health = {}", self.max_health);
        let _ = writeln!(out, "length = {}", self.length);
        let _ = writeln!(out, "wing_chord = {}", self.wing_chord);
        let _ = writeln!(out, "tail_span = {}", self.tail_span);
        let _ = writeln!(
            out,
            "body_color = {} {} {}",
            self.body_color[0], self.body_color[1], self.body_color[2]
        );
        for (index, gun) in self.guns.iter().enumerate() {
            let _ = writeln!(out, "\n[gun {index}]");
            let _ = writeln!(out, "name = {}", gun.name);
            let _ = writeln!(out, "caliber_mm = {}", gun.caliber_mm);
            let _ = writeln!(out, "rounds_per_second = {}", gun.rounds_per_second);
            let _ = writeln!(out, "muzzle_velocity = {}", gun.muzzle_velocity);
            let _ = writeln!(out, "damage = {}", gun.damage);
            let _ = writeln!(out, "spread = {}", gun.spread);
            let _ = writeln!(out, "ammo = {}", gun.ammo);
            for muzzle in &gun.muzzles {
                let _ = writeln!(out, "muzzle = {} {} {}", muzzle[0], muzzle[1], muzzle[2]);
            }
        }
        out
    }
}

/// The planes that ship with the game (used in single-player and as the
/// launcher's list). The server keeps its own copies under `planes/`.
pub fn default_planes() -> Vec<PlaneConfig> {
    vec![
        PlaneConfig {
            id: "f4u-4-corsair".to_string(),
            name: "F4U-4 Corsair".to_string(),
            mass: 6_000.0,
            wing_area: 29.2,
            wing_span: 12.5,
            max_power: 1_640_000.0,
            static_thrust: 18_000.0,
            prop_efficiency: 0.82,
            wep_multiplier: 1.15,
            critical_altitude: 7_000.0,
            altitude_power_falloff: 6_000.0,
            cl_slope: 4.5,
            cl_max: 1.4,
            stall_aoa: 0.28,
            cd0: 0.025,
            oswald: 0.80,
            cl_flap: 0.5,
            cd_flap: 0.08,
            trim_alpha: 0.042,
            pitch_rate: 1.1,
            yaw_rate: 0.5,
            roll_rate: 3.0,
            pitch_stability: 2.2,
            yaw_stability: 3.0,
            control_ref_speed: 120.0,
            responsiveness: 6.0,
            pitch_damping: 0.0,
            yaw_damping: 0.0,
            roll_damping: 2.0,
            cruise_speed: 150.0,
            stiffening_onset_ias: 160.0,
            stiffening_mach: 0.78,
            max_ias: 246.0,
            g_limit: 11.0,
            prop_torque: 0.35,
            flap_speed_limits: [388.0 / 3.6, 299.0 / 3.6, 253.0 / 3.6],
            max_health: 100.0,
            body_color: [0.13, 0.19, 0.42],
            length: 10.3,
            wing_chord: 2.0,
            tail_span: 3.4,
            guns: vec![GunConfig {
                name: "M2 Browning".to_string(),
                caliber_mm: 12.7,
                rounds_per_second: 12.5,
                muzzle_velocity: 870.0,
                damage: 3.0,
                spread: 0.0035,
                muzzles: vec![
                    [-1.9, -0.05, -0.9],
                    [-2.7, -0.05, -0.9],
                    [-3.5, -0.05, -0.9],
                    [1.9, -0.05, -0.9],
                    [2.7, -0.05, -0.9],
                    [3.5, -0.05, -0.9],
                ],
                ammo: 2350,
            }],
        },
        PlaneConfig {
            id: "bf-109-g6".to_string(),
            name: "Bf 109 G-6".to_string(),
            mass: 3_150.0,
            wing_area: 16.1,
            wing_span: 9.9,
            max_power: 1_100_000.0,
            static_thrust: 13_000.0,
            prop_efficiency: 0.80,
            wep_multiplier: 1.12,
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
            body_color: [0.44, 0.46, 0.43],
            length: 9.2,
            wing_chord: 2.0,
            tail_span: 3.4,
            guns: vec![
                GunConfig {
                    name: "MG 151/20".to_string(),
                    caliber_mm: 20.0,
                    rounds_per_second: 11.7,
                    muzzle_velocity: 800.0,
                    damage: 12.0,
                    spread: 0.003,
                    muzzles: vec![[0.0, -0.05, -4.15]],
                    ammo: 200,
                },
                GunConfig {
                    name: "MG 131".to_string(),
                    caliber_mm: 13.0,
                    rounds_per_second: 15.0,
                    muzzle_velocity: 800.0,
                    damage: 5.0,
                    spread: 0.003,
                    muzzles: vec![[-0.4, 0.35, -3.4], [0.4, 0.35, -3.4]],
                    ammo: 600,
                },
            ],
        },
        PlaneConfig {
            id: "spitfire-f-mk-ixc".to_string(),
            name: "Spitfire F Mk IXc".to_string(),
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
            cl_max: 1.5,
            stall_aoa: 0.28,
            cd0: 0.025,
            oswald: 0.85,
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
            flap_speed_limits: [260.0 / 3.6, 260.0 / 3.6, 260.0 / 3.6],
            max_health: 100.0,
            body_color: [0.24, 0.30, 0.20],
            length: 9.1,
            wing_chord: 2.0,
            tail_span: 3.4,
            guns: vec![GunConfig {
                name: "Hispano Mk II".to_string(),
                caliber_mm: 20.0,
                rounds_per_second: 10.0,
                muzzle_velocity: 880.0,
                damage: 12.0,
                spread: 0.003,
                muzzles: vec![
                    [-2.9, -0.05, -1.0],
                    [-2.1, -0.05, -1.0],
                    [2.9, -0.05, -1.0],
                    [2.1, -0.05, -1.0],
                ],
                ammo: 480,
            }],
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_planes_round_trip() {
        for plane in default_planes() {
            let text = plane.to_config_string();
            let parsed = PlaneConfig::parse(&plane.id, &text);
            assert_eq!(parsed, plane, "round-trip failed for {}", plane.name);
        }
    }

    #[test]
    fn parses_a_custom_plane_with_guns() {
        let text = "\
name = Test Fighter
mass = 2500
wing_span = 8.5
body_color = 0.1 0.2 0.3

[gun 0]
name = Test Cannon
caliber_mm = 20
rounds_per_second = 9
muzzle_velocity = 900
damage = 15
ammo = 100
muzzle = 1 2 3
muzzle = -1 2 3
";
        let plane = PlaneConfig::parse("test-fighter", text);
        assert_eq!(plane.name, "Test Fighter");
        assert_eq!(plane.mass, 2500.0);
        assert_eq!(plane.wing_span, 8.5);
        assert_eq!(plane.body_color, [0.1, 0.2, 0.3]);
        assert_eq!(plane.guns.len(), 1);
        assert_eq!(plane.guns[0].name, "Test Cannon");
        assert_eq!(plane.guns[0].caliber_mm, 20.0);
        assert_eq!(plane.guns[0].muzzles.len(), 2);
        assert_eq!(plane.guns[0].ammo, 100);
    }

    #[test]
    fn missing_keys_keep_defaults() {
        let plane = PlaneConfig::parse("blank", "name = Blank\n");
        assert_eq!(plane.name, "Blank");
        assert_eq!(plane.mass, PlaneConfig::default().mass);
    }
}
