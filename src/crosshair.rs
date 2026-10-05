//! The gun crosshair (fixed at screen centre) and the mouse-aim cursor.
//!
//! The chase camera always looks along the aircraft's nose, so the screen centre
//! *is* the gun direction — the crosshair marks where the rounds go. The mouse
//! instead moves a **world-space aim direction**, and its on-screen cursor drifts
//! back to the centre as the aircraft turns toward it (like War Thunder). The OS
//! cursor is hidden and locked while flying so the mouse is relative.

use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};

use crate::camera::ChaseCamera;
use crate::flight::MouseAim;
use crate::menu::GameMenu;

#[derive(Component)]
struct Crosshair;

/// The moving mouse-aim cursor.
#[derive(Component)]
struct AimCursor;

pub struct CrosshairPlugin;

impl Plugin for CrosshairPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_crosshair)
            .add_systems(Update, (update_aim_cursor, manage_cursor));
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

    // The mouse-aim cursor: a small square outline that drifts back to centre.
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            display: Display::None,
            width: px(16),
            height: px(16),
            border: UiRect::all(px(2)),
            ..default()
        },
        BorderColor::all(Color::srgba(1.0, 0.9, 0.4, 0.95)),
        GlobalZIndex(6),
        AimCursor,
    ));
}

/// Put the aim cursor wherever the aim direction projects on screen.
fn update_aim_cursor(
    mouse_aim: Res<MouseAim>,
    camera: Query<(&Camera, &GlobalTransform), With<ChaseCamera>>,
    mut cursor: Query<&mut Node, With<AimCursor>>,
) {
    let Ok(mut node) = cursor.single_mut() else {
        return;
    };
    let Ok((camera, camera_transform)) = camera.single() else {
        return;
    };
    if !mouse_aim.engaged || mouse_aim.target == Vec3::ZERO {
        node.display = Display::None;
        return;
    }
    let point = camera_transform.translation() + mouse_aim.target * 1000.0;
    match camera.world_to_viewport(camera_transform, point) {
        Ok(screen) => {
            node.left = px(screen.x - 8.0);
            node.top = px(screen.y - 8.0);
            node.display = Display::Flex;
        }
        Err(_) => node.display = Display::None,
    }
}

/// Hide and lock the OS cursor while flying so the mouse is relative, and
/// release it while the menu is open.
fn manage_cursor(menu: Res<GameMenu>, mut cursors: Query<&mut CursorOptions, With<PrimaryWindow>>) {
    let Ok(mut cursor) = cursors.single_mut() else {
        return;
    };
    let captured = !menu.open;
    cursor.visible = !captured;
    cursor.grab_mode = if captured {
        CursorGrabMode::Locked
    } else {
        CursorGrabMode::None
    };
}
