//! In-game menu, opened with `Escape`.
//!
//! The simulation keeps running while the menu is open — the world is **not**
//! paused. Only the player's control inputs are handed back to the instructor
//! (controls go neutral) so menu navigation does not steer the aircraft.

use bevy::prelude::*;

/// Open/closed state and current selection for the in-game menu.
#[derive(Resource, Default)]
pub struct GameMenu {
    pub open: bool,
    pub selected: usize,
}

/// The menu actions, in order.
const ITEMS: [&str; 3] = ["Resume", "Change plane", "Quit to Desktop"];

#[derive(Component)]
struct MenuRoot;

#[derive(Component)]
struct MenuItem(usize);

pub struct GameMenuPlugin;

impl Plugin for GameMenuPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<GameMenu>()
            .add_systems(Startup, spawn_menu)
            .add_systems(Update, (toggle_menu, menu_input, update_menu_ui));
    }
}

fn spawn_menu(mut commands: Commands) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                display: Display::None,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
            GlobalZIndex(10),
            MenuRoot,
        ))
        .with_children(|parent| {
            parent
                .spawn((
                    Node {
                        flex_direction: FlexDirection::Column,
                        align_items: AlignItems::Center,
                        padding: UiRect::all(px(28)),
                        row_gap: px(10),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.05, 0.06, 0.09, 0.95)),
                ))
                .with_children(|panel| {
                    panel.spawn((
                        Text::new("Menu"),
                        TextFont {
                            font_size: FontSize::Px(30.0),
                            ..default()
                        },
                        TextColor(Color::srgb(0.9, 0.95, 1.0)),
                    ));
                    panel.spawn((
                        Text::new("The world keeps flying while this menu is open."),
                        TextFont {
                            font_size: FontSize::Px(14.0),
                            ..default()
                        },
                        TextColor(Color::srgb(0.6, 0.65, 0.72)),
                    ));
                    for (index, label) in ITEMS.iter().enumerate() {
                        panel.spawn((
                            Text::new(*label),
                            TextFont {
                                font_size: FontSize::Px(22.0),
                                ..default()
                            },
                            TextColor(Color::WHITE),
                            MenuItem(index),
                        ));
                    }
                    panel.spawn((
                        Text::new("Up/Down: select    Enter: confirm    Esc: resume"),
                        TextFont {
                            font_size: FontSize::Px(14.0),
                            ..default()
                        },
                        TextColor(Color::srgb(0.6, 0.65, 0.72)),
                    ));
                });
        });
}

/// `Escape` opens or closes the menu.
fn toggle_menu(keys: Res<ButtonInput<KeyCode>>, mut menu: ResMut<GameMenu>) {
    if keys.just_pressed(KeyCode::Escape) {
        menu.open = !menu.open;
        menu.selected = 0;
    }
}

/// Navigate and activate the menu items.
fn menu_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut menu: ResMut<GameMenu>,
    mut spawn: ResMut<crate::spawn_menu::SpawnState>,
    mut exit: MessageWriter<AppExit>,
) {
    if !menu.open {
        return;
    }
    if keys.just_pressed(KeyCode::ArrowUp) {
        menu.selected = menu.selected.saturating_sub(1);
    }
    if keys.just_pressed(KeyCode::ArrowDown) {
        menu.selected = (menu.selected + 1).min(ITEMS.len() - 1);
    }
    if keys.just_pressed(KeyCode::Enter) {
        match menu.selected {
            0 => menu.open = false, // Resume
            1 => {
                // Change plane: reopen the spawn menu.
                spawn.choosing = true;
                menu.open = false;
            }
            2 => {
                exit.write(AppExit::Success); // Quit to desktop
            }
            _ => {}
        }
    }
}

/// Show/hide the menu and highlight the current selection.
fn update_menu_ui(
    menu: Res<GameMenu>,
    mut root: Query<&mut Node, With<MenuRoot>>,
    mut items: Query<(&MenuItem, &mut TextColor)>,
) {
    if let Ok(mut node) = root.single_mut() {
        node.display = if menu.open {
            Display::Flex
        } else {
            Display::None
        };
    }
    for (item, mut color) in &mut items {
        *color = if menu.open && item.0 == menu.selected {
            TextColor(Color::srgb(1.0, 0.84, 0.25))
        } else {
            TextColor(Color::WHITE)
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::message::Messages;

    fn menu_app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<GameMenu>()
            .init_resource::<crate::spawn_menu::SpawnState>()
            .add_message::<AppExit>()
            .add_systems(Update, (toggle_menu, menu_input));
        app
    }

    /// Simulate one frame with `keys` freshly pressed.
    fn step(app: &mut App, keys: &[KeyCode]) {
        {
            let mut input = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            input.reset_all();
            for key in keys {
                input.press(*key);
            }
        }
        app.update();
    }

    #[test]
    fn escape_opens_and_closes_the_menu() {
        let mut app = menu_app();
        step(&mut app, &[KeyCode::Escape]);
        assert!(app.world().resource::<GameMenu>().open);
        step(&mut app, &[]); // release frame
        step(&mut app, &[KeyCode::Escape]);
        assert!(!app.world().resource::<GameMenu>().open);
    }

    #[test]
    fn navigation_moves_and_clamps_the_selection() {
        let mut app = menu_app();
        app.world_mut().resource_mut::<GameMenu>().open = true;
        step(&mut app, &[KeyCode::ArrowDown]);
        assert_eq!(app.world().resource::<GameMenu>().selected, 1);
        step(&mut app, &[KeyCode::ArrowDown]);
        assert_eq!(app.world().resource::<GameMenu>().selected, 2);
        step(&mut app, &[KeyCode::ArrowDown]);
        assert_eq!(app.world().resource::<GameMenu>().selected, 2); // clamped
        step(&mut app, &[KeyCode::ArrowUp]);
        assert_eq!(app.world().resource::<GameMenu>().selected, 1);
    }

    #[test]
    fn selecting_quit_requests_exit() {
        let mut app = menu_app();
        {
            let mut menu = app.world_mut().resource_mut::<GameMenu>();
            menu.open = true;
            menu.selected = 2; // "Quit to Desktop"
        }
        step(&mut app, &[KeyCode::Enter]);
        assert!(
            !app.world().resource::<Messages<AppExit>>().is_empty(),
            "quit should write an AppExit"
        );
    }

    #[test]
    fn selecting_change_plane_opens_the_spawn_menu() {
        let mut app = menu_app();
        {
            let mut menu = app.world_mut().resource_mut::<GameMenu>();
            menu.open = true;
            menu.selected = 1; // "Change plane"
        }
        app.world_mut()
            .resource_mut::<crate::spawn_menu::SpawnState>()
            .choosing = false;
        step(&mut app, &[KeyCode::Enter]);
        assert!(
            app.world()
                .resource::<crate::spawn_menu::SpawnState>()
                .choosing
        );
        assert!(!app.world().resource::<GameMenu>().open);
    }

    #[test]
    fn selecting_resume_closes_the_menu() {
        let mut app = menu_app();
        app.world_mut().resource_mut::<GameMenu>().open = true; // selected = 0
        step(&mut app, &[KeyCode::Enter]);
        assert!(!app.world().resource::<GameMenu>().open);
    }
}
