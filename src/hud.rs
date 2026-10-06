//! Minimal heads-up display: flight telemetry and a controls cheat-sheet.

use bevy::prelude::*;

use openthunder::keybinds::{
    DAMAGE_ENGINE, DAMAGE_LEFT_WING, DAMAGE_TAIL, FIRE, FLAPS_DOWN, FLAPS_UP, FREE_LOOK, GEAR,
    PITCH_DOWN, PITCH_DOWN_ALT, PITCH_UP, PITCH_UP_ALT, REPAIR, RESET, ROLL_LEFT, ROLL_RIGHT,
    THROTTLE_DOWN, THROTTLE_UP, WEP, YAW_LEFT, YAW_RIGHT, ZOOM, key_display,
};

use crate::aircraft::{Aircraft, PlayerControlled};
use crate::combat::CombatFeedback;
use crate::damage::{AircraftPart, DamageModel};
use crate::flight::Bindings;
use crate::net::NetClient;
use crate::pilot::CrewSkills;

#[derive(Component)]
struct HudText;

#[derive(Component)]
struct MatchText;

#[derive(Component)]
struct KillFeedText;

/// The bottom-right pitch / roll / yaw input readout.
#[derive(Component)]
struct ControlsText;

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_hud)
            .add_systems(Update, (update_hud, update_controls));
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

    // Team scoreboard, centred along the bottom.
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            bottom: px(16.0),
            width: percent(100.0),
            justify_content: JustifyContent::Center,
            ..default()
        },
        Text::new(""),
        TextFont {
            font_size: FontSize::Px(22.0),
            ..default()
        },
        TextLayout::justify(Justify::Center),
        TextColor(Color::srgb(1.0, 1.0, 1.0)),
        MatchText,
    ));

    // Kill feed, top right.
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            top: px(10.0),
            right: px(12.0),
            ..default()
        },
        Text::new(""),
        TextFont {
            font_size: FontSize::Px(16.0),
            ..default()
        },
        TextLayout::justify(Justify::Right),
        TextColor(Color::srgb(1.0, 0.85, 0.55)),
        KillFeedText,
    ));

    // Control inputs (pitch / roll / yaw), bottom right.
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            bottom: px(16.0),
            right: px(12.0),
            ..default()
        },
        Text::new(""),
        TextFont {
            font_size: FontSize::Px(16.0),
            ..default()
        },
        TextLayout::justify(Justify::Right),
        TextColor(Color::srgb(0.85, 0.92, 1.0)),
        ControlsText,
    ));
}

fn update_hud(
    bindings: Res<Bindings>,
    client: Res<NetClient>,
    crew: Res<CrewSkills>,
    feedback: Res<CombatFeedback>,
    match_state: Res<crate::match_client::MatchClient>,
    target_score: Res<crate::targets::TargetScore>,
    aircraft: Query<(&Aircraft, &Transform, &DamageModel), With<PlayerControlled>>,
    mut hud: Query<
        &mut Text,
        (
            With<HudText>,
            Without<MatchText>,
            Without<KillFeedText>,
            Without<ControlsText>,
        ),
    >,
    mut match_text: Query<
        &mut Text,
        (
            With<MatchText>,
            Without<KillFeedText>,
            Without<ControlsText>,
        ),
    >,
    mut feed_text: Query<&mut Text, (With<KillFeedText>, Without<ControlsText>)>,
) {
    // The scoreboard and kill feed update even before we have spawned.
    if let Ok(mut text) = match_text.single_mut() {
        if match_state.is_team_mode() {
            let scores = match_state
                .scores
                .iter()
                .enumerate()
                .map(|(index, score)| {
                    format!(
                        "{} {score}",
                        crate::match_client::MatchClient::team_name(index as u8)
                    )
                })
                .collect::<Vec<_>>()
                .join("   -   ");
            let minutes = (match_state.time_left / 60.0).floor() as u32;
            let seconds = (match_state.time_left % 60.0).floor() as u32;
            let down = if match_state.respawn_in() > 0.0 {
                format!("   DOWN — respawn in {:.1}s", match_state.respawn_in())
            } else {
                String::new()
            };
            **text = format!(
                "TEAM DEATHMATCH   {scores}   (first to {})   {minutes:02}:{seconds:02}{down}",
                match_state.score_limit
            );
        } else {
            **text = String::new();
        }
    }
    if let Ok(mut text) = feed_text.single_mut() {
        **text = match_state
            .kill_feed
            .iter()
            .rev()
            .map(|entry| entry.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
    }

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

    let wep = if !aircraft.spec.has_wep {
        // This engine has no war emergency power.
        String::new()
    } else if aircraft.wep {
        "WEP".to_string()
    } else if aircraft.wep_heat > 0.5 {
        format!("WEP cooling {:.0}%", aircraft.wep_heat * 100.0)
    } else if aircraft.throttle >= 0.99 {
        "WEP ready".to_string()
    } else {
        "WEP needs 100% throttle".to_string()
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

    let gear = if aircraft.gear_position > 0.95 {
        "down"
    } else if aircraft.gear_position < 0.05 {
        "up"
    } else {
        "moving"
    };

    let targets = if target_score.total > 0 {
        format!(
            "Targets {}/{} destroyed",
            target_score.destroyed, target_score.total
        )
    } else {
        String::new()
    };

    **text = format!(
        "{name}   {connection}\n\
         TAS {speed:5.0} km/h   IAS {ias:5.0} km/h   Alt {alt:6.0} m\n\
         Throttle {thr:3.0}% {wep}   AoA {alpha:+5.1} deg   G {g:+.1}   Stamina {stamina:3.0}%   Pilot {pilot} (tol {tol:.1}g)\n\
         Flaps {flaps}   Gear {gear}   Wing {wing:3.0}%   Engine {eng:3.0}%   Tail {tail:3.0}%   {status}\n\
         Ammo {ammo}   {guns}{hit_marker}\n\
         {targets}\n\
         \n\
         Mouse: aim   {fire_key}: fire   {pitch_up}/{pitch_down}/{pitch_up_alt}/{pitch_down_alt}: pitch   {roll_left}/{roll_right}: roll   {yaw_left}/{yaw_right}: rudder\n\
         {throttle_up}/{throttle_down}: throttle   {wep_key}: WEP   {flaps_down}/{flaps_up}: flaps   {gear_key}: gear   {reset}: respawn\n\
         {dmg_wing}/{dmg_engine}/{dmg_tail}: damage   {repair}: repair   {free_look}: free look   {zoom_key}: zoom   Esc: menu",
        name = aircraft.spec.name,
        connection = connection,
        ammo = ammo,
        guns = guns,
        hit_marker = hit_marker,
        targets = targets,
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
        gear = gear,
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
        gear_key = key_display(bindings.name(GEAR)),
        reset = key_display(bindings.name(RESET)),
        dmg_wing = key_display(bindings.name(DAMAGE_LEFT_WING)),
        dmg_engine = key_display(bindings.name(DAMAGE_ENGINE)),
        dmg_tail = key_display(bindings.name(DAMAGE_TAIL)),
        repair = key_display(bindings.name(REPAIR)),
        free_look = key_display(bindings.name(FREE_LOOK)),
        zoom_key = key_display(bindings.name(ZOOM)),
    );
}

/// Show how much pitch, roll and rudder the controls are applying, bottom right.
fn update_controls(
    aircraft: Query<&Aircraft, With<PlayerControlled>>,
    mut text: Query<&mut Text, With<ControlsText>>,
) {
    let Ok(mut text) = text.single_mut() else {
        return;
    };
    let Ok(aircraft) = aircraft.single() else {
        **text = String::new();
        return;
    };
    let controls = aircraft.controls;
    **text = format!(
        "CONTROLS\n\
         Pitch {pitch:+4.0}%  {pitch_bar}\n\
         Roll  {roll:+4.0}%  {roll_bar}\n\
         Yaw   {yaw:+4.0}%  {yaw_bar}",
        pitch = controls.pitch * 100.0,
        roll = controls.roll * 100.0,
        yaw = controls.yaw * 100.0,
        pitch_bar = control_bar(controls.pitch),
        roll_bar = control_bar(controls.roll),
        yaw_bar = control_bar(controls.yaw),
    );
}

/// A centred bar for a control input: `-1.0` fills to the left, `+1.0` to the
/// right, `0.0` is empty.
fn control_bar(value: f32) -> String {
    const HALF: usize = 4;
    let value = value.clamp(-1.0, 1.0);
    let filled = (value.abs() * HALF as f32).round() as usize;
    let mut bar = String::with_capacity(2 * HALF + 2);
    bar.push('[');
    for cell in 0..(2 * HALF) {
        let on = if value >= 0.0 {
            cell >= HALF && cell < HALF + filled
        } else {
            cell >= HALF - filled && cell < HALF
        };
        bar.push(if on { '#' } else { '-' });
    }
    bar.push(']');
    bar
}

#[cfg(test)]
mod tests {
    use super::control_bar;

    #[test]
    fn control_bar_is_centred_and_signed() {
        assert_eq!(control_bar(0.0), "[--------]");
        assert_eq!(control_bar(1.0), "[----####]");
        assert_eq!(control_bar(-1.0), "[####----]");
        assert_eq!(control_bar(0.5), "[----##--]");
        assert_eq!(control_bar(-0.5), "[--##----]");
        // Out-of-range values clamp rather than panic.
        assert_eq!(control_bar(5.0), "[----####]");
    }
}
