//! The gun crosshair and the mouse-aim cursor.
//!
//! The crosshair always marks **where the guns are pointing**: the aircraft's
//! nose, projected on screen, so it tracks the real firing direction as the
//! camera orbits (as in War Thunder). The mouse instead moves a **world-space
//! aim direction**, and its on-screen cursor drifts back toward the crosshair as
//! the aircraft turns onto it. The OS cursor is hidden and locked while flying
//! so the mouse is relative.

use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};

use crate::aircraft::PlayerControlled;
use crate::camera::ChaseCamera;
use crate::flight::MouseAim;
use crate::menu::GameMenu;

/// How far along the guns to project the crosshair (metres).
const GUN_RANGE: f32 = 1000.0;

#[derive(Component)]
struct Crosshair;

/// The moving mouse-aim cursor.
#[derive(Component)]
struct AimCursor;

pub struct CrosshairPlugin;

impl Plugin for CrosshairPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_crosshair).add_systems(
            Update,
            (
                // Run after the flight step so the aircraft's transform is final
                // and the aim clamp sees the freshly-moved aim.
                update_crosshair.after(crate::flight::FlightSet),
                update_aim_cursor.after(crate::flight::FlightSet),
                manage_cursor,
            ),
        );
    }
}

fn spawn_crosshair(mut commands: Commands) {
    let color = Color::srgba(0.95, 1.0, 0.95, 0.85);

    // The gun crosshair — repositioned each frame at the screen point the guns
    // are actually aimed at (the aircraft's nose), so it moves as the camera
    // orbits and always shows where the rounds go.
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                width: px(28),
                height: px(28),
                ..default()
            },
            // Below the menus (10/20) so they cover it.
            GlobalZIndex(5),
            Crosshair,
        ))
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

/// The world point the guns are aimed at, [`GUN_RANGE`] metres along the nose.
fn gun_point(translation: Vec3, rotation: Quat) -> Vec3 {
    translation + (rotation * Vec3::NEG_Z) * GUN_RANGE
}

/// Put the gun crosshair wherever the guns are actually pointing — the
/// aircraft's nose — so it stays on the firing direction as the camera orbits.
fn update_crosshair(
    aircraft: Query<&Transform, (With<PlayerControlled>, Without<ChaseCamera>)>,
    camera: Query<(&Camera, &GlobalTransform), With<ChaseCamera>>,
    mut crosshair: Query<&mut Node, With<Crosshair>>,
) {
    let Ok(mut node) = crosshair.single_mut() else {
        return;
    };
    let Ok(aircraft) = aircraft.single() else {
        return;
    };
    let Ok((camera, camera_transform)) = camera.single() else {
        return;
    };
    let point = gun_point(aircraft.translation, aircraft.rotation);
    if let Ok(screen) = camera.world_to_viewport(camera_transform, point) {
        node.left = px(screen.x - 14.0);
        node.top = px(screen.y - 14.0);
    }
}

/// Half the aim cursor's size in pixels (it is a 16x16 box).
const AIM_CURSOR_HALF: f32 = 8.0;
/// Keep the cursor at least this far inside the screen edge.
const AIM_CURSOR_MARGIN: f32 = 6.0;

/// Put the aim cursor wherever the aim direction projects on screen — and keep
/// it on screen. Zooming in narrows the field of view, so the same aim can
/// otherwise slide off the edge; when that happens the cursor is clamped to the
/// edge and the aim is pulled in with it, so the guns follow the cursor.
fn update_aim_cursor(
    mut mouse_aim: ResMut<MouseAim>,
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
    let Ok(screen) = camera.world_to_viewport(camera_transform, point) else {
        node.display = Display::None;
        return;
    };
    let Some(size) = camera.logical_viewport_size() else {
        return;
    };

    // Clamp the cursor inside the viewport (with a small margin so its box stays
    // fully visible), and move the aim to match when it had to be clamped.
    let lo = Vec2::splat(AIM_CURSOR_HALF + AIM_CURSOR_MARGIN);
    let hi = (size - lo).max(lo);
    let clamped = screen.clamp(lo, hi);
    if clamped != screen {
        if let Ok(ray) = camera.viewport_to_world(camera_transform, clamped) {
            mouse_aim.target = *ray.direction;
        }
    }

    node.left = px(clamped.x - AIM_CURSOR_HALF);
    node.top = px(clamped.y - AIM_CURSOR_HALF);
    node.display = Display::Flex;
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

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::camera::{CameraProjection, PerspectiveProjection, RenderTargetInfo};

    const SCREEN: UVec2 = UVec2::new(1280, 720);

    /// A projection with the given vertical field of view and the 1280x720 aspect.
    fn projection(fov: f32) -> PerspectiveProjection {
        PerspectiveProjection {
            fov,
            aspect_ratio: SCREEN.x as f32 / SCREEN.y as f32,
            ..default()
        }
    }

    /// Spawn a chase camera with a pre-computed projection and a fixed viewport,
    /// so `world_to_viewport` works without a renderer.
    fn spawn_camera(
        app: &mut App,
        translation: Vec3,
        rotation: Quat,
        projection: PerspectiveProjection,
    ) {
        let mut camera = Camera::default();
        camera.computed.clip_from_view = projection.get_clip_from_view();
        camera.computed.target_info = Some(RenderTargetInfo {
            physical_size: SCREEN,
            scale_factor: 1.0,
        });
        app.world_mut().spawn((
            camera,
            GlobalTransform::from(Transform::from_translation(translation).with_rotation(rotation)),
            ChaseCamera,
        ));
    }

    fn spawn_aircraft(app: &mut App, rotation: Quat) -> Entity {
        app.world_mut()
            .spawn((
                Transform::from_translation(Vec3::ZERO).with_rotation(rotation),
                PlayerControlled,
            ))
            .id()
    }

    fn spawn_crosshair_node(app: &mut App) -> Entity {
        app.world_mut().spawn((Node::default(), Crosshair)).id()
    }

    fn node_px(app: &App, entity: Entity) -> (f32, f32) {
        let node = app.world().get::<Node>(entity).unwrap();
        let (Val::Px(left), Val::Px(top)) = (node.left, node.top) else {
            panic!("the node position should be set in pixels");
        };
        (left, top)
    }

    /// With the guns aimed straight ahead of a centred camera, the crosshair sits
    /// at the centre of the screen.
    #[test]
    fn the_crosshair_sits_on_the_gun_direction() {
        let mut app = App::new();
        app.add_systems(Update, update_crosshair);

        spawn_aircraft(&mut app, Quat::IDENTITY);
        spawn_camera(
            &mut app,
            Vec3::new(0.0, 0.0, 18.0),
            Quat::IDENTITY,
            projection(std::f32::consts::FRAC_PI_4),
        );
        let crosshair = spawn_crosshair_node(&mut app);

        app.update();

        // Centre of 1280x720 is (640, 360); the node is 28px, so its corner is
        // offset by 14px.
        let (left, top) = node_px(&app, crosshair);
        assert!((left - 626.0).abs() < 1.0, "left = {left}");
        assert!((top - 346.0).abs() < 1.0, "top = {top}");
    }

    /// The crosshair tracks the nose, not the screen centre: turning the guns
    /// right moves it right.
    #[test]
    fn the_crosshair_follows_the_nose() {
        let mut app = App::new();
        app.add_systems(Update, update_crosshair);

        // Nose yawed to the right of the (fixed) camera.
        spawn_aircraft(&mut app, Quat::from_rotation_y(-0.3));
        spawn_camera(
            &mut app,
            Vec3::new(0.0, 0.0, 18.0),
            Quat::IDENTITY,
            projection(std::f32::consts::FRAC_PI_4),
        );
        let crosshair = spawn_crosshair_node(&mut app);

        app.update();

        let (left, _) = node_px(&app, crosshair);
        assert!(
            left > 626.0 + 10.0,
            "the crosshair should follow the nose right, left = {left}"
        );
    }

    /// Even when the camera orbits away from the nose (as it does when the aim
    /// nears the screen edge), the crosshair stays on the gun direction.
    #[test]
    fn the_crosshair_stays_on_the_guns_when_the_camera_orbits() {
        let mut app = App::new();
        app.add_systems(Update, update_crosshair);

        // Guns straight ahead, but the camera yawed to the left.
        spawn_aircraft(&mut app, Quat::IDENTITY);
        spawn_camera(
            &mut app,
            Vec3::new(0.0, 0.0, 18.0),
            Quat::from_rotation_y(0.4),
            projection(std::f32::consts::FRAC_PI_4),
        );
        let crosshair = spawn_crosshair_node(&mut app);

        app.update();

        // The nose now projects to the right of the (off-centre) view.
        let (left, _) = node_px(&app, crosshair);
        assert!(
            left > 626.0 + 10.0,
            "the crosshair should leave the screen centre, left = {left}"
        );
    }

    /// Zooming in narrows the view, so a far-off aim would slide off the edge; the
    /// cursor is clamped on screen and the aim is pulled in with it.
    #[test]
    fn the_aim_cursor_stays_on_screen_when_zoomed() {
        let mut app = App::new();
        app.init_resource::<MouseAim>()
            .add_systems(Update, update_aim_cursor);

        spawn_camera(
            &mut app,
            Vec3::new(0.0, 0.0, 18.0),
            Quat::IDENTITY,
            projection(0.32), // zoomed in
        );
        let cursor = app.world_mut().spawn((Node::default(), AimCursor)).id();

        // Aim far to the right — well outside the narrow zoomed view.
        {
            let mut aim = app.world_mut().resource_mut::<MouseAim>();
            aim.engaged = true;
            aim.target = Quat::from_rotation_y(-0.9) * Vec3::NEG_Z;
        }
        let before = app.world().resource::<MouseAim>().target;

        app.update();

        // The cursor box is fully on screen...
        let (left, top) = node_px(&app, cursor);
        assert!(
            left >= 0.0 && left + 2.0 * AIM_CURSOR_HALF <= SCREEN.x as f32,
            "cursor left = {left}"
        );
        assert!(
            top >= 0.0 && top + 2.0 * AIM_CURSOR_HALF <= SCREEN.y as f32,
            "cursor top = {top}"
        );
        // ...and the aim was pulled in to match.
        let after = app.world().resource::<MouseAim>().target;
        assert!(
            after.angle_between(before) > 0.05,
            "the aim should be clamped, was {before:?}, now {after:?}"
        );
    }

    /// An aim that is already on screen is left alone.
    #[test]
    fn an_on_screen_aim_is_not_clamped() {
        let mut app = App::new();
        app.init_resource::<MouseAim>()
            .add_systems(Update, update_aim_cursor);

        spawn_camera(
            &mut app,
            Vec3::new(0.0, 0.0, 18.0),
            Quat::IDENTITY,
            projection(0.32),
        );
        app.world_mut().spawn((Node::default(), AimCursor));

        let target = Quat::from_rotation_y(-0.05) * Vec3::NEG_Z;
        {
            let mut aim = app.world_mut().resource_mut::<MouseAim>();
            aim.engaged = true;
            aim.target = target;
        }

        app.update();

        let after = app.world().resource::<MouseAim>().target;
        assert!(
            (after - target).length() < 1e-4,
            "a centred aim should not move, was {target:?}, now {after:?}"
        );
    }
}
