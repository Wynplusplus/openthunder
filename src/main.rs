//! OpenThunder — a War Thunder "Air Realistic Battle"-style flight prototype.
//!
//! This is the smallest thing that is genuinely fun to fly: one aircraft (the
//! F4U-4 Corsair), a simple map, a physical flight model with stalls, and a
//! damage model with per-section hit points. There is no combat yet.
//!
//! Run with `cargo run --release` (or `cargo run`).

mod aircraft;
mod camera;
mod damage;
mod flight;
mod hud;
mod world;

use bevy::prelude::*;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "OpenThunder — Air RB Prototype".to_string(),
                resolution: (1280, 720).into(),
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
        ))
        .run();
}
