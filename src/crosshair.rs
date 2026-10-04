//! Fixed gun crosshair at the centre of the screen.
//!
//! The chase camera always looks along the aircraft's nose, so the screen centre
//! *is* the gun direction — the crosshair marks where the rounds go.

use bevy::prelude::*;

#[derive(Component)]
struct Crosshair;

pub struct CrosshairPlugin;

impl Plugin for CrosshairPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_crosshair);
    }
}

fn spawn_crosshair(mut commands: Commands) {
    let color = Color::srgba(0.95, 1.0, 0.95, 0.85);

    commands
        .spawn((
            // Full-screen, transparent, centres its child.
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            // Below the menus (10/20) so they cover it.
            GlobalZIndex(5),
            Crosshair,
        ))
        .with_children(|parent| {
            parent
                .spawn(Node {
                    width: px(28),
                    height: px(28),
                    ..default()
                })
                .with_children(|reticle| {
                    // Horizontal bar.
                    reticle.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: px(0),
                            top: px(13),
                            width: px(28),
                            height: px(2),
                            ..default()
                        },
                        BackgroundColor(color),
                    ));
                    // Vertical bar.
                    reticle.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: px(13),
                            top: px(0),
                            width: px(2),
                            height: px(28),
                            ..default()
                        },
                        BackgroundColor(color),
                    ));
                    // Centre dot.
                    reticle.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: px(12),
                            top: px(12),
                            width: px(4),
                            height: px(4),
                            ..default()
                        },
                        BackgroundColor(Color::srgba(1.0, 0.85, 0.3, 0.95)),
                    ));
                });
        });
}
