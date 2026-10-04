//! OpenThunder — a War Thunder "Air Realistic Battle"-style flight prototype.
//!
//! A small flight prototype: several aircraft, a simple map, a physical flight
//! model with stalls, and a per-section damage model. There is no combat yet.
//!
//! Run with `cargo run --release` (or `cargo run`).

mod aircraft;
mod camera;
mod combat;
mod crosshair;
mod damage;
mod flight;
mod hud;
mod match_client;
mod menu;
mod net;
mod pilot;
mod spawn_menu;
mod world;

use bevy::prelude::*;
use bevy::window::WindowMode;

use openthunder::settings::Settings;

fn main() {
    // Window mode comes from the launcher's setting (fullscreen by default).
    let settings = Settings::load_or_create();
    let mode = if settings.fullscreen {
        WindowMode::BorderlessFullscreen(MonitorSelection::Primary)
    } else {
        WindowMode::Windowed
    };

    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "OpenThunder — Air RB Prototype".to_string(),
                resolution: (1280, 720).into(),
                mode,
                ..default()
            }),
            ..default()
        }))
        // Sky colour, also used as the fog colour in `camera.rs`.
        .insert_resource(ClearColor(Color::srgb(0.45, 0.62, 0.85)))
        .insert_resource(GlobalAmbientLight {
            color: Color::srgb(0.65, 0.75, 0.95),
            brightness: 500.0,
            ..default()
        })
        .add_plugins((
            world::WorldPlugin,
            aircraft::AircraftPlugin,
            flight::FlightPlugin,
            camera::ChaseCameraPlugin,
            damage::DamagePlugin,
            hud::HudPlugin,
            menu::GameMenuPlugin,
            net::NetPlugin,
            combat::CombatPlugin,
            spawn_menu::SpawnMenuPlugin,
            crosshair::CrosshairPlugin,
            pilot::PilotPlugin,
            match_client::MatchClientPlugin,
        ))
        .run();
}
