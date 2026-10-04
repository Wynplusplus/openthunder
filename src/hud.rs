//! Minimal heads-up display: flight telemetry and a controls cheat-sheet.

use bevy::prelude::*;

use openthunder::keybinds::{
    DAMAGE_ENGINE, DAMAGE_LEFT_WING, DAMAGE_TAIL, FIRE, FLAPS_DOWN, FLAPS_UP, FREE_LOOK,
    PITCH_DOWN, PITCH_DOWN_ALT, PITCH_UP, PITCH_UP_ALT, REPAIR, RESET, ROLL_LEFT, ROLL_RIGHT,
    THROTTLE_DOWN, THROTTLE_UP, WEP, YAW_LEFT, YAW_RIGHT, key_display,
};

use crate::aircraft::{Aircraft, PlayerControlled};
use crate::combat::CombatFeedback;
use crate::damage::{AircraftPart, DamageModel};
use crate::flight::Bindings;
use crate::net::NetClient;
use crate::pilot::CrewSkills;

#[derive(Component)]
struct HudText;

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_hud)
            .add_systems(Update, update_hud);
    }
}

fn spawn_hud(mut commands: Commands) {
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            top: px(10.0),
            left: px(12.0),
            ..default()
        },
        Text::new("OpenThunder"),
        TextFont {
            font_size: FontSize::Px(18.0),
            ..default()
        },
        TextColor(Color::srgb(0.85, 1.0, 0.85)),
        HudText,
    ));
}

fn update_hud(
    bindings: Res<Bindings>,
    client: Res<NetClient>,
    crew: Res<CrewSkills>,
    feedback: Res<CombatFeedback>,
    aircraft: Query<(&Aircraft, &Transform, &DamageModel), With<PlayerControlled>>,
    mut hud: Query<&mut Text, With<HudText>>,
) {
    let Ok((aircraft, transform, damage)) = aircraft.single() else {
        return;
    };
    let Ok(mut text) = hud.single_mut() else {
        return;
    };
    let connection = client.status.lock().unwrap().label();

    let ammo: u32 = aircraft.ammo.iter().sum();
    let guns = aircraft
        .spec
        .guns
        .iter()
        .map(|gun| format!("{}x {} {}mm", gun.muzzles.len(), gun.name, gun.caliber_mm))
        .collect::<Vec<_>>()
        .join(" + ");
    let hit_marker = if feedback.player_hit > 0.0 {
        "   << HIT TAKEN >>"
    } else if feedback.enemy_hit > 0.0 {
        "   < HIT >"
    } else {
        ""
    };

    let speed_kmh = aircraft.airspeed * 3.6;
    let ias_kmh = aircraft.ias * 3.6;
    let altitude = transform.translation.y;
    let alpha_deg = aircraft.alpha.to_degrees();

    // Summarise structural state; `label`/`is_destroyed`/`overall_integrity`
    // come from the damage model.
    let destroyed: Vec<&str> = AircraftPart::ALL
        .iter()
        .filter(|part| damage.is_destroyed(**part))
        .map(|part| part.label())
        .collect();
    let status = if destroyed.is_empty() {
        format!("Integrity {:.0}%", damage.overall_integrity() * 100.0)
    } else {
        format!("DAMAGED: {}", destroyed.join(", "))
    };

    let wep = if aircraft.wep {
        "WEP".to_string()
    } else if aircraft.wep_heat > 0.5 {
        format!("WEP {:.0}% heat", aircraft.wep_heat * 100.0)
    } else {
        String::new()
    };

    let pilot = if aircraft.blackout >= 1.0 {
        "BLACKED OUT".to_string()
    } else if aircraft.redout >= 1.0 {
        "REDDED OUT".to_string()
    } else if aircraft.blackout > 0.02 {
        format!("blackout {:.0}%", aircraft.blackout * 100.0)
    } else if aircraft.redout > 0.02 {
        format!("redout {:.0}%", aircraft.redout * 100.0)
    } else {
        "clear".to_string()
    };

    **text = format!(
        "{name}   {connection}\n\
         TAS {speed:5.0} km/h   IAS {ias:5.0} km/h   Alt {alt:6.0} m\n\
         Throttle {thr:3.0}% {wep}   AoA {alpha:+5.1} deg   G {g:+.1}   Stamina {stamina:3.0}%   Pilot {pilot} (tol {tol:.1}g)\n\
         Flaps {flaps}   Wing {wing:3.0}%   Engine {eng:3.0}%   Tail {tail:3.0}%   {status}\n\
         Ammo {ammo}   {guns}{hit_marker}\n\
         \n\
         Mouse: aim   {fire_key}: fire   {pitch_up}/{pitch_down}/{pitch_up_alt}/{pitch_down_alt}: pitch   {roll_left}/{roll_right}: roll   {yaw_left}/{yaw_right}: rudder\n\
         {throttle_up}/{throttle_down}: throttle   {wep_key}: WEP   {flaps_down}/{flaps_up}: flaps   {reset}: respawn\n\
         {dmg_wing}/{dmg_engine}/{dmg_tail}: damage   {repair}: repair   {free_look}: free look   Esc: menu",
        name = aircraft.spec.name,
        connection = connection,
        ammo = ammo,
        guns = guns,
        hit_marker = hit_marker,
        fire_key = key_display(bindings.name(FIRE)),
        speed = speed_kmh,
        ias = ias_kmh,
        alt = altitude,
        thr = aircraft.throttle * 100.0,
        wep = wep,
        alpha = alpha_deg,
        g = aircraft.g_load,
        stamina = aircraft.stamina * 100.0,
        pilot = pilot,
        tol = crew.g_tolerance,
        flaps = aircraft.flaps.label(),
        wing = damage.integrity(AircraftPart::LeftWing) * 100.0,
        eng = damage.integrity(AircraftPart::Engine) * 100.0,
        tail = damage.integrity(AircraftPart::Tail) * 100.0,
        status = status,
        pitch_up = key_display(bindings.name(PITCH_UP)),
        pitch_down = key_display(bindings.name(PITCH_DOWN)),
        pitch_up_alt = key_display(bindings.name(PITCH_UP_ALT)),
        pitch_down_alt = key_display(bindings.name(PITCH_DOWN_ALT)),
        roll_left = key_display(bindings.name(ROLL_LEFT)),
        roll_right = key_display(bindings.name(ROLL_RIGHT)),
        yaw_left = key_display(bindings.name(YAW_LEFT)),
        yaw_right = key_display(bindings.name(YAW_RIGHT)),
        throttle_up = key_display(bindings.name(THROTTLE_UP)),
        throttle_down = key_display(bindings.name(THROTTLE_DOWN)),
        wep_key = key_display(bindings.name(WEP)),
        flaps_down = key_display(bindings.name(FLAPS_DOWN)),
        flaps_up = key_display(bindings.name(FLAPS_UP)),
        reset = key_display(bindings.name(RESET)),
        dmg_wing = key_display(bindings.name(DAMAGE_LEFT_WING)),
        dmg_engine = key_display(bindings.name(DAMAGE_ENGINE)),
        dmg_tail = key_display(bindings.name(DAMAGE_TAIL)),
        repair = key_display(bindings.name(REPAIR)),
        free_look = key_display(bindings.name(FREE_LOOK)),
    );
}
