//! Pilot physiology: g-tolerance, blackout/redout and stamina.
//!
//! War Thunder models the pilot separately from the airframe. A trained crew can
//! hold roughly 6–7 g before the vision starts to close in; push harder (or for
//! longer, as stamina runs out) and the screen tunnels down to a full blackout,
//! at which point you lose control until the aircraft unloads and you come back.
//! Negative g produces a red-out instead. This module reproduces that loop and
//! draws the tunnel-vision overlay.
//!
//! The structural g limit is *higher* than the pilot's tolerance (see
//! `aircraft.rs`), so in a hard turn you black out well before the wings are at
//! risk — just like Air RB.

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use crate::aircraft::{Aircraft, PlayerControlled};

/// Crew-skill limits. The defaults approximate a well-trained (expert) crew;
/// War Thunder's maxed "G-tolerance" is about 6.9 g.
#[derive(Resource, Clone, Copy, Debug)]
pub struct CrewSkills {
    /// Positive g the pilot can hold indefinitely before greying out.
    pub g_tolerance: f32,
    /// Negative g at which the pilot starts to red out.
    pub negative_g_tolerance: f32,
    /// Blackout gained per g above the tolerance, per second.
    pub blackout_rate: f32,
    /// Vision recovered per second once the g comes off.
    pub recovery_rate: f32,
    /// Stamina lost per g above 3, per second.
    pub stamina_drain: f32,
    /// Stamina recovered per second below 3 g.
    pub stamina_recovery: f32,
}

impl Default for CrewSkills {
    fn default() -> Self {
        Self {
            g_tolerance: 6.5,
            negative_g_tolerance: -3.0,
            blackout_rate: 0.18,
            recovery_rate: 0.4,
            stamina_drain: 0.012,
            stamina_recovery: 0.1,
        }
    }
}

/// Advances pilot physiology by `dt` at load factor `g`, returning the new
/// `(blackout, redout, stamina)`.
///
/// This is pure so it can be unit-tested without an app.
pub fn step_pilot(
    blackout: f32,
    redout: f32,
    stamina: f32,
    g: f32,
    crew: &CrewSkills,
    dt: f32,
) -> (f32, f32, f32) {
    // Stamina drains during sustained manoeuvring and recovers in level flight.
    let mut stamina = if g > 3.0 {
        stamina - crew.stamina_drain * (g - 3.0) * dt
    } else {
        stamina + crew.stamina_recovery * dt
    };
    stamina = stamina.clamp(0.0, 1.0);

    // Fatigue lowers the tolerance: a fresh pilot is at full, an exhausted one
    // at 70%.
    let fatigue = 0.7 + 0.3 * stamina;
    let positive = crew.g_tolerance * fatigue;
    let negative = crew.negative_g_tolerance * fatigue;

    let mut blackout = if g > positive {
        blackout + (g - positive) * crew.blackout_rate * dt
    } else {
        blackout - crew.recovery_rate * dt
    };
    blackout = blackout.clamp(0.0, 1.0);

    // Negative g is harder on the pilot, so the red-out comes on quicker.
    let mut redout = if g < negative {
        redout + (negative - g) * crew.blackout_rate * 2.0 * dt
    } else {
        redout - crew.recovery_rate * dt
    };
    redout = redout.clamp(0.0, 1.0);

    (blackout, redout, stamina)
}

pub struct PilotPlugin;

impl Plugin for PilotPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CrewSkills>()
            .add_systems(Startup, spawn_overlay)
            .add_systems(
                Update,
                (update_pilot_physiology, update_overlay)
                    .chain()
                    .after(crate::flight::FlightSet),
            );
    }
}

/// Full-screen tunnel-vision overlay (radial gradient) plus a flat veil that
/// darkens everything as the blackout approaches full.
#[derive(Component)]
struct Vignette;

#[derive(Component)]
struct Veil;

fn spawn_overlay(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let vignette = images.add(vignette_texture());

    let fullscreen = Node {
        position_type: PositionType::Absolute,
        width: percent(100),
        height: percent(100),
        ..default()
    };

    commands.spawn((
        fullscreen.clone(),
        ImageNode {
            image: vignette,
            image_mode: NodeImageMode::Stretch,
            color: Color::NONE,
            ..default()
        },
        GlobalZIndex(4),
        Vignette,
    ));
    // A flat veil that only ramps in near a full blackout, so the screen goes
    // completely dark rather than just tunnelled.
    commands.spawn((
        fullscreen,
        ImageNode {
            image: Handle::default(),
            image_mode: NodeImageMode::Stretch,
            color: Color::NONE,
            ..default()
        },
        GlobalZIndex(3),
        Veil,
    ));
}

fn update_pilot_physiology(
    time: Res<Time>,
    crew: Res<CrewSkills>,
    mut query: Query<&mut Aircraft, With<PlayerControlled>>,
) {
    let dt = time.delta_secs();
    let Ok(mut aircraft) = query.single_mut() else {
        return;
    };
    let (blackout, redout, stamina) = step_pilot(
        aircraft.blackout,
        aircraft.redout,
        aircraft.stamina,
        aircraft.g_load,
        &crew,
        dt,
    );
    aircraft.blackout = blackout;
    aircraft.redout = redout;
    aircraft.stamina = stamina;
}

fn update_overlay(
    aircraft: Query<&Aircraft, With<PlayerControlled>>,
    mut vignette: Query<&mut ImageNode, (With<Vignette>, Without<Veil>)>,
    mut veil: Query<&mut ImageNode, (With<Veil>, Without<Vignette>)>,
) {
    let Ok(aircraft) = aircraft.single() else {
        return;
    };
    let (color, strength) = if aircraft.redout > aircraft.blackout {
        (Color::srgba(0.55, 0.0, 0.0, 1.0), aircraft.redout)
    } else {
        (Color::srgba(0.0, 0.0, 0.0, 1.0), aircraft.blackout)
    };

    if let Ok(mut node) = vignette.single_mut() {
        node.color = color.with_alpha(strength);
    }
    // Cubic so the flat veil stays clear until the blackout is nearly total.
    if let Ok(mut node) = veil.single_mut() {
        node.color = color.with_alpha(strength.powi(3));
    }
}

/// A 16:9 radial alpha gradient: clear in the middle, opaque at the edges.
fn vignette_texture() -> Image {
    const W: u32 = 256;
    const H: u32 = 144;
    let mut data = Vec::with_capacity((W * H * 4) as usize);
    let cx = W as f32 / 2.0;
    let cy = H as f32 / 2.0;
    let norm = 0.5 * W.min(H) as f32;

    for y in 0..H {
        for x in 0..W {
            let dx = x as f32 + 0.5 - cx;
            let dy = y as f32 + 0.5 - cy;
            let r = (dx * dx + dy * dy).sqrt() / norm;
            let a = ((r - 0.35) / 0.65).clamp(0.0, 1.0);
            let a = a * a * (3.0 - 2.0 * a); // smoothstep
            data.extend_from_slice(&[255, 255, 255, (a * 255.0).round() as u8]);
        }
    }

    Image::new(
        Extent3d {
            width: W,
            height: H,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn crew() -> CrewSkills {
        CrewSkills::default()
    }

    #[test]
    fn level_flight_does_not_black_out() {
        let (blackout, redout, stamina) = step_pilot(0.0, 0.0, 1.0, 1.0, &crew(), 1.0);
        assert_eq!(blackout, 0.0);
        assert_eq!(redout, 0.0);
        assert!(stamina >= 1.0);
    }

    #[test]
    fn sustained_high_g_blacks_out_in_a_few_seconds() {
        let crew = crew();
        let mut blackout = 0.0;
        // 8 g for 5 s should be well past the tolerance and knock the pilot out.
        for _ in 0..50 {
            (blackout, _, _) = step_pilot(blackout, 0.0, 1.0, 8.0, &crew, 0.1);
        }
        assert!(blackout >= 1.0, "expected blackout, got {blackout}");
    }

    #[test]
    fn brief_spike_does_not_black_out() {
        // Half a second at 9 g, then back to level: vision should recover.
        let crew = crew();
        let mut blackout = 0.0;
        for _ in 0..5 {
            (blackout, _, _) = step_pilot(blackout, 0.0, 1.0, 9.0, &crew, 0.1);
        }
        assert!(blackout < 0.6, "a brief spike should not fully black out");
        for _ in 0..30 {
            (blackout, _, _) = step_pilot(blackout, 0.0, 1.0, 1.0, &crew, 0.1);
        }
        assert_eq!(blackout, 0.0);
    }

    #[test]
    fn pushing_over_reds_out() {
        let crew = crew();
        let mut redout = 0.0;
        for _ in 0..50 {
            (_, redout, _) = step_pilot(0.0, redout, 1.0, -5.0, &crew, 0.1);
        }
        assert!(redout >= 1.0, "expected redout, got {redout}");
    }

    #[test]
    fn stamina_falls_under_load_and_lowers_tolerance() {
        let crew = crew();
        let mut stamina = 1.0;
        for _ in 0..100 {
            (_, _, stamina) = step_pilot(0.0, 0.0, stamina, 8.0, &crew, 0.1);
        }
        assert!(stamina < 1.0, "stamina should drain under sustained g");
        // Recover in level flight.
        for _ in 0..200 {
            (_, _, stamina) = step_pilot(0.0, 0.0, stamina, 1.0, &crew, 0.1);
        }
        assert_eq!(stamina, 1.0);
    }
}
