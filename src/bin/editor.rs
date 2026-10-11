//! OpenThunder mission editor.
//!
//! A small Bevy 3D application for building the maps/missions the dedicated
//! server hosts, creating planes (including importing glTF/GLB models), and
//! launching a local test server. Reachable from the TUI launcher.
//!
//! Run it with `cargo run --bin editor`.
//!
//! It reads and writes the **server's** `maps/` and `planes/` directories. By
//! default it looks for `../openthunder_server`; override with the
//! `OPENTHUNDER_SERVER_DIR` environment variable.

use std::path::{Path, PathBuf};
use std::process::{Child, Command};

use bevy::input::keyboard::KeyboardInput;
use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};

use openthunder::map_doc::{MapDoc, MapObject, ObjectKind, SpawnPoint};
use openthunder::plane_config::PlaneConfig;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "OpenThunder — Mission Editor".to_string(),
                resolution: (1440, 900).into(),
                ..default()
            }),
            ..default()
        }))
        .insert_resource(ClearColor(Color::srgb(0.42, 0.58, 0.8)))
        .insert_resource(GlobalAmbientLight {
            color: Color::srgb(0.7, 0.78, 0.95),
            brightness: 600.0,
            ..default()
        })
        .insert_resource(Editor::new())
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (
                camera_control,
                text_input,
                editor_input,
                rebuild_preview,
                update_hud,
                manage_cursor,
                poll_server,
            ),
        )
        .run();
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Map,
    Planes,
}

/// A text field being typed into.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Field {
    NewMap,
    Rename,
    NewPlane,
    PlaneName,
    Model,
}

#[derive(Default)]
struct TextInput {
    field: Option<Field>,
    buffer: String,
}

#[derive(Resource)]
struct Editor {
    doc: MapDoc,
    selected: Option<usize>,
    place: ObjectKind,
    spawn_team: u8,
    /// Set when the document changed and the 3D preview must be rebuilt.
    dirty: bool,
    status: String,
    server_dir: PathBuf,
    server: Option<Child>,
    mode: Mode,
    planes: Vec<PlaneConfig>,
    plane_selected: usize,
    input: TextInput,
    look: Vec2,
}

impl Editor {
    fn new() -> Self {
        let server_dir = server_dir();
        let doc = MapDoc::new("untitled");
        let planes = load_planes(&server_dir);
        Self {
            doc,
            selected: None,
            place: ObjectKind::Building,
            spawn_team: 0,
            dirty: true,
            status: format!("Server directory: {}", server_dir.display()),
            server_dir,
            server: None,
            mode: Mode::Map,
            planes,
            plane_selected: 0,
            input: TextInput::default(),
            look: Vec2::new(0.0, -0.3),
        }
    }

    fn maps_dir(&self) -> PathBuf {
        self.server_dir.join("maps")
    }

    fn planes_dir(&self) -> PathBuf {
        self.server_dir.join("planes")
    }

    /// The path this document saves to.
    fn map_path(&self) -> PathBuf {
        self.maps_dir().join(format!("{}.map", self.doc.id))
    }
}

#[derive(Component)]
struct EditorCamera;

#[derive(Component)]
struct EditorHud;

/// Preview geometry rebuilt from the document.
#[derive(Component)]
struct Preview;

// ---------------------------------------------------------------------------
// Paths & files
// ---------------------------------------------------------------------------

/// Locate the dedicated-server repository (where `maps/` and `planes/` live).
fn server_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("OPENTHUNDER_SERVER_DIR") {
        return PathBuf::from(dir);
    }
    for candidate in ["../openthunder_server", "openthunder_server", "."] {
        let path = PathBuf::from(candidate);
        if path.join("maps").is_dir() || path.join("planes").is_dir() {
            return path;
        }
    }
    PathBuf::from("..")
}

fn load_planes(server_dir: &Path) -> Vec<PlaneConfig> {
    let mut planes = Vec::new();
    let dir = server_dir.join("planes");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return planes;
    };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.is_dir())
        .collect();
    paths.sort();
    for path in paths {
        let id = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("plane")
            .to_string();
        let conf = path.join("plane.conf");
        let plane = match std::fs::read_to_string(&conf) {
            Ok(text) => PlaneConfig::parse(&id, &text),
            Err(_) => PlaneConfig {
                id: id.clone(),
                name: id.clone(),
                ..Default::default()
            },
        };
        planes.push(plane);
    }
    planes
}

fn save_map(editor: &mut Editor) {
    let path = editor.map_path();
    if let Some(parent) = path.parent() {
        if let Err(err) = std::fs::create_dir_all(parent) {
            editor.status = format!("Could not create {}: {err}", parent.display());
            return;
        }
    }
    match std::fs::write(&path, editor.doc.to_string()) {
        Ok(()) => {
            editor.dirty = false;
            editor.status = format!("Saved {}", path.display());
        }
        Err(err) => editor.status = format!("Save failed: {err}"),
    }
}

fn load_map(editor: &mut Editor, id: &str) {
    let path = editor.maps_dir().join(format!("{id}.map"));
    match std::fs::read_to_string(&path) {
        Ok(text) => {
            editor.doc = MapDoc::parse(id, &text);
            editor.selected = None;
            editor.dirty = true;
            editor.status = format!("Loaded {}", path.display());
        }
        Err(err) => editor.status = format!("Load failed: {err}"),
    }
}

fn save_plane(editor: &mut Editor) {
    let Some(plane) = editor.planes.get(editor.plane_selected) else {
        return;
    };
    let dir = editor.planes_dir().join(&plane.id);
    if let Err(err) = std::fs::create_dir_all(&dir) {
        editor.status = format!("Could not create {}: {err}", dir.display());
        return;
    }
    let path = dir.join("plane.conf");
    match std::fs::write(&path, plane.to_config_string()) {
        Ok(()) => editor.status = format!("Saved {}", path.display()),
        Err(err) => editor.status = format!("Save failed: {err}"),
    }
}

// ---------------------------------------------------------------------------
// Setup
// ---------------------------------------------------------------------------

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn((
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection {
            fov: 0.9,
            ..default()
        }),
        Transform::from_xyz(0.0, 220.0, 420.0).looking_at(Vec3::new(0.0, 0.0, 0.0), Vec3::Y),
        EditorCamera,
    ));

    commands.spawn((
        DirectionalLight {
            illuminance: 14_000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(2000.0, 4000.0, 1000.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    // Ground plane.
    let ground = materials.add(StandardMaterial {
        base_color: Color::srgb(0.24, 0.34, 0.18),
        perceptual_roughness: 1.0,
        ..default()
    });
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(40_000.0, 40_000.0))),
        MeshMaterial3d(ground),
    ));

    // A 1 km grid so distances are readable.
    let line = meshes.add(Cuboid::new(1.0, 0.4, 20_000.0));
    let line_material = materials.add(StandardMaterial {
        base_color: Color::srgba(0.6, 0.7, 0.6, 0.5),
        alpha_mode: AlphaMode::Blend,
        ..default()
    });
    let mut step = -10_000.0;
    while step <= 10_000.0 {
        if step != 0.0 {
            commands.spawn((
                Mesh3d(line.clone()),
                MeshMaterial3d(line_material.clone()),
                Transform::from_xyz(step, 0.2, 0.0),
            ));
            commands.spawn((
                Mesh3d(line.clone()),
                MeshMaterial3d(line_material.clone()),
                Transform::from_xyz(0.0, 0.2, step)
                    .with_rotation(Quat::from_rotation_y(std::f32::consts::FRAC_PI_2)),
            ));
        }
        step += 1000.0;
    }

    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            top: px(10.0),
            left: px(12.0),
            ..default()
        },
        Text::new("OpenThunder mission editor"),
        TextFont {
            font_size: FontSize::Px(16.0),
            ..default()
        },
        TextColor(Color::srgb(0.92, 1.0, 0.92)),
        EditorHud,
    ));
}

// ---------------------------------------------------------------------------
// Camera (free-fly)
// ---------------------------------------------------------------------------

fn camera_control(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    motion: Res<AccumulatedMouseMotion>,
    mut editor: ResMut<Editor>,
    mut camera: Query<&mut Transform, With<EditorCamera>>,
) {
    if editor.input.field.is_some() {
        return; // typing
    }
    let Ok(mut transform) = camera.single_mut() else {
        return;
    };
    let dt = time.delta_secs();

    // Mouse look (yaw/pitch kept in the resource so pitch can be clamped).
    let sensitivity = 0.0025;
    editor.look.x -= motion.delta.x * sensitivity;
    editor.look.y = (editor.look.y - motion.delta.y * sensitivity).clamp(-1.5, 1.5);
    transform.rotation =
        Quat::from_rotation_y(editor.look.x) * Quat::from_rotation_x(editor.look.y);

    let forward = transform.rotation * Vec3::NEG_Z;
    let right = transform.rotation * Vec3::X;
    let mut dir = Vec3::ZERO;
    if keys.pressed(KeyCode::KeyW) {
        dir += forward;
    }
    if keys.pressed(KeyCode::KeyS) {
        dir -= forward;
    }
    if keys.pressed(KeyCode::KeyD) {
        dir += right;
    }
    if keys.pressed(KeyCode::KeyA) {
        dir -= right;
    }
    if keys.pressed(KeyCode::KeyE) {
        dir += Vec3::Y;
    }
    if keys.pressed(KeyCode::KeyQ) {
        dir -= Vec3::Y;
    }
    let speed = if keys.pressed(KeyCode::ShiftLeft) {
        1200.0
    } else {
        160.0
    };
    transform.translation += dir.normalize_or_zero() * speed * dt;
}

// ---------------------------------------------------------------------------
// Text input
// ---------------------------------------------------------------------------

fn text_input(mut events: MessageReader<KeyboardInput>, mut editor: ResMut<Editor>) {
    if editor.input.field.is_none() {
        events.clear();
        return;
    }
    for event in events.read() {
        if !event.state.is_pressed() {
            continue;
        }
        match event.key_code {
            KeyCode::Enter => {
                let field = editor.input.field.take().unwrap();
                let value = std::mem::take(&mut editor.input.buffer);
                apply_text(&mut editor, field, value);
            }
            KeyCode::Escape => {
                editor.input.field = None;
                editor.input.buffer.clear();
                editor.status = "Cancelled.".to_string();
            }
            KeyCode::Backspace => {
                editor.input.buffer.pop();
            }
            _ => {
                if let Some(text) = &event.text {
                    for ch in text.chars() {
                        if ch.is_ascii_alphanumeric() || "-_/. ".contains(ch) {
                            editor.input.buffer.push(ch);
                        }
                    }
                }
            }
        }
    }
}

fn apply_text(editor: &mut Editor, field: Field, value: String) {
    let value = value.trim().to_string();
    match field {
        Field::NewMap => {
            if value.is_empty() {
                editor.status = "New map cancelled (no name).".to_string();
                return;
            }
            let id = slug(&value);
            editor.doc = MapDoc::new(&id);
            editor.doc.name = value;
            editor.selected = None;
            editor.dirty = true;
            editor.status = format!("New map '{id}'.");
        }
        Field::Rename => {
            if !value.is_empty() {
                editor.doc.name = value;
                editor.dirty = true;
            }
        }
        Field::NewPlane => {
            if value.is_empty() {
                editor.status = "New plane cancelled (no name).".to_string();
                return;
            }
            let id = slug(&value);
            let plane = PlaneConfig {
                id: id.clone(),
                name: value,
                ..Default::default()
            };
            editor.planes.push(plane);
            editor.plane_selected = editor.planes.len() - 1;
            save_plane(editor);
            editor.status = format!("Created plane '{id}'.");
        }
        Field::PlaneName => {
            if let Some(plane) = editor.planes.get_mut(editor.plane_selected) {
                if !value.is_empty() {
                    plane.name = value;
                    save_plane(editor);
                }
            }
        }
        Field::Model => {
            if let Some(plane) = editor.planes.get_mut(editor.plane_selected) {
                plane.model = if value.is_empty() { None } else { Some(value) };
                save_plane(editor);
            }
        }
    }
}

fn slug(value: &str) -> String {
    value
        .trim()
        .to_lowercase()
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

// ---------------------------------------------------------------------------
// Editing input
// ---------------------------------------------------------------------------

fn editor_input(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    camera: Query<(&Camera, &GlobalTransform), With<EditorCamera>>,
    mut editor: ResMut<Editor>,
) {
    if editor.input.field.is_some() {
        return;
    }
    let Ok((camera, camera_transform)) = camera.single() else {
        return;
    };
    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);

    // --- Global keys ---
    if keys.just_pressed(KeyCode::Escape) {
        editor.mode = if editor.mode == Mode::Map {
            Mode::Planes
        } else {
            Mode::Map
        };
    }
    if keys.just_pressed(KeyCode::F2) {
        editor.mode = Mode::Planes;
    }
    if keys.just_pressed(KeyCode::F1) {
        editor.mode = Mode::Map;
    }
    if ctrl && keys.just_pressed(KeyCode::KeyS) {
        if editor.mode == Mode::Planes {
            save_plane(&mut editor);
        } else {
            save_map(&mut editor);
        }
    }
    if keys.just_pressed(KeyCode::F5) {
        toggle_server(&mut editor);
    }
    if ctrl && keys.just_pressed(KeyCode::KeyN) {
        editor.input.field = Some(Field::NewMap);
        editor.input.buffer.clear();
        editor.status = "Type a name for the new map, then Enter.".to_string();
    }
    if ctrl && keys.just_pressed(KeyCode::KeyQ) {
        std::process::exit(0);
    }

    if editor.mode == Mode::Planes {
        planes_input(&keys, &mut editor);
        return;
    }

    if keys.just_pressed(KeyCode::KeyR) {
        editor.input.field = Some(Field::Rename);
        editor.input.buffer = editor.doc.name.clone();
        editor.status = "Type a new map name, then Enter.".to_string();
    }
    if ctrl && keys.just_pressed(KeyCode::KeyO) {
        let id = editor.doc.id.clone();
        load_map(&mut editor, &id);
    }

    // --- Palette: 1-8 pick the kind to place ---
    for (index, kind) in ObjectKind::ALL.iter().enumerate() {
        let code = digit_key(index);
        if keys.just_pressed(code) {
            editor.place = *kind;
            editor.status = format!("Placing {}.", kind.label());
        }
    }

    // --- Place / select with the mouse ---
    let ray = screen_ray(camera, camera_transform);
    if mouse.just_pressed(MouseButton::Left) {
        if let Some(ray) = ray {
            if let Some(point) = ground_hit(ray) {
                let mut object = MapObject::new(editor.place, point);
                object.position[1] = object.scale[1] * 0.5;
                editor.doc.objects.push(object);
                editor.selected = Some(editor.doc.objects.len() - 1);
                editor.dirty = true;
                editor.status = format!("Placed {}.", editor.place.label());
            }
        }
    }
    if mouse.just_pressed(MouseButton::Right) {
        if let Some(ray) = ray {
            editor.selected = pick(&editor.doc, ray);
            editor.status = match editor.selected {
                Some(index) => format!("Selected object #{index}."),
                None => "No object under the cursor.".to_string(),
            };
        }
    }
    if keys.just_pressed(KeyCode::Tab) {
        let count = editor.doc.objects.len();
        if count > 0 {
            editor.selected = Some(match editor.selected {
                Some(index) => (index + 1) % count,
                None => 0,
            });
        }
    }

    // --- Spawn points ---
    if keys.just_pressed(KeyCode::KeyT) {
        editor.spawn_team = 1 - editor.spawn_team;
        editor.status = format!("Spawn team {}.", editor.spawn_team);
    }
    if keys.just_pressed(KeyCode::KeyB) {
        if let Some(ray) = ray {
            if let Some(point) = ground_hit(ray) {
                let mut spawn = SpawnPoint::new(editor.spawn_team, point);
                spawn.position[1] = 0.0;
                editor.doc.spawns.push(spawn);
                editor.dirty = true;
                editor.status = format!("Added spawn for team {}.", editor.spawn_team);
            }
        }
    }

    // --- Edit the selected object ---
    let Some(index) = editor.selected else {
        return;
    };
    let Some(object) = editor.doc.objects.get_mut(index) else {
        editor.selected = None;
        return;
    };
    let step = if keys.pressed(KeyCode::ShiftLeft) {
        50.0
    } else {
        5.0
    };
    let mut changed = false;
    if keys.pressed(KeyCode::ArrowLeft) {
        object.position[0] -= step;
        changed = true;
    }
    if keys.pressed(KeyCode::ArrowRight) {
        object.position[0] += step;
        changed = true;
    }
    if keys.pressed(KeyCode::ArrowUp) {
        object.position[2] -= step;
        changed = true;
    }
    if keys.pressed(KeyCode::ArrowDown) {
        object.position[2] += step;
        changed = true;
    }
    if keys.pressed(KeyCode::PageUp) {
        object.position[1] += step;
        changed = true;
    }
    if keys.pressed(KeyCode::PageDown) {
        object.position[1] -= step;
        changed = true;
    }
    if keys.pressed(KeyCode::BracketLeft) {
        object.yaw -= 5.0;
        changed = true;
    }
    if keys.pressed(KeyCode::BracketRight) {
        object.yaw += 5.0;
        changed = true;
    }
    if keys.pressed(KeyCode::Minus) {
        for axis in &mut object.scale {
            *axis = (*axis - 2.0).max(1.0);
        }
        changed = true;
    }
    if keys.pressed(KeyCode::Equal) {
        for axis in &mut object.scale {
            *axis += 2.0;
        }
        changed = true;
    }
    if keys.just_pressed(KeyCode::Delete) {
        editor.doc.objects.remove(index);
        editor.selected = None;
        editor.dirty = true;
        editor.status = "Deleted object.".to_string();
        return;
    }
    if changed {
        editor.dirty = true;
    }
}

fn planes_input(keys: &ButtonInput<KeyCode>, editor: &mut Editor) {
    if editor.planes.is_empty() {
        editor.plane_selected = 0;
    } else {
        editor.plane_selected = editor.plane_selected.min(editor.planes.len() - 1);
    }
    if keys.just_pressed(KeyCode::ArrowUp) {
        editor.plane_selected = editor.plane_selected.saturating_sub(1);
    }
    if keys.just_pressed(KeyCode::ArrowDown) {
        editor.plane_selected =
            (editor.plane_selected + 1).min(editor.planes.len().saturating_sub(1));
    }
    if keys.just_pressed(KeyCode::KeyN) {
        editor.input.field = Some(Field::NewPlane);
        editor.input.buffer.clear();
        editor.status = "Type a name for the new plane, then Enter.".to_string();
    }
    if keys.just_pressed(KeyCode::KeyR) {
        editor.input.field = Some(Field::PlaneName);
        editor.input.buffer = editor
            .planes
            .get(editor.plane_selected)
            .map(|plane| plane.name.clone())
            .unwrap_or_default();
        editor.status = "Type a new name for the plane, then Enter.".to_string();
    }
    if keys.just_pressed(KeyCode::KeyM) {
        editor.input.field = Some(Field::Model);
        editor.input.buffer = editor
            .planes
            .get(editor.plane_selected)
            .and_then(|plane| plane.model.clone())
            .unwrap_or_default();
        editor.status =
            "Type a glTF/GLB path (relative to the client's assets/), then Enter.".to_string();
    }
    if keys.just_pressed(KeyCode::Delete) {
        let Some(id) = editor
            .planes
            .get(editor.plane_selected)
            .map(|plane| plane.id.clone())
        else {
            return;
        };
        let dir = editor.planes_dir().join(&id);
        let _ = std::fs::remove_dir_all(&dir);
        editor.planes.remove(editor.plane_selected);
        editor.status = format!("Deleted plane '{id}'.");
    }
}

/// The `KeyCode` for the 1-based palette digit `index`.
fn digit_key(index: usize) -> KeyCode {
    match index {
        0 => KeyCode::Digit1,
        1 => KeyCode::Digit2,
        2 => KeyCode::Digit3,
        3 => KeyCode::Digit4,
        4 => KeyCode::Digit5,
        5 => KeyCode::Digit6,
        6 => KeyCode::Digit7,
        _ => KeyCode::Digit8,
    }
}

/// A ray from the camera through the centre of the screen.
fn screen_ray(camera: &Camera, camera_transform: &GlobalTransform) -> Option<Ray3d> {
    let center = camera.logical_viewport_size()? / 2.0;
    camera.viewport_to_world(camera_transform, center).ok()
}

/// Where a ray meets the ground plane (`y = 0`).
fn ground_hit(ray: Ray3d) -> Option<[f32; 3]> {
    let dir: Vec3 = *ray.direction;
    if dir.y.abs() < 1e-4 {
        return None;
    }
    let t = -ray.origin.y / dir.y;
    if t < 0.0 {
        return None;
    }
    let point = ray.origin + dir * t;
    Some([point.x, 0.0, point.z])
}

/// The object nearest to the ray, within a generous radius.
fn pick(doc: &MapDoc, ray: Ray3d) -> Option<usize> {
    let dir: Vec3 = *ray.direction;
    let mut best: Option<(usize, f32)> = None;
    for (index, object) in doc.objects.iter().enumerate() {
        let center = Vec3::from(object.position);
        let to_center = center - ray.origin;
        let along = to_center.dot(dir);
        if along < 0.0 {
            continue;
        }
        let closest = ray.origin + dir * along;
        let distance = (closest - center).length();
        let radius = object
            .scale
            .iter()
            .fold(0.0f32, |acc, axis| acc.max(axis.abs() * 0.6));
        if distance <= radius.max(6.0) && best.map(|(_, d)| distance < d).unwrap_or(true) {
            best = Some((index, distance));
        }
    }
    best.map(|(index, _)| index)
}

// ---------------------------------------------------------------------------
// Preview
// ---------------------------------------------------------------------------

fn rebuild_preview(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut editor: ResMut<Editor>,
    existing: Query<Entity, With<Preview>>,
) {
    if !editor.dirty {
        return;
    }
    editor.dirty = false;
    for entity in &existing {
        commands.entity(entity).despawn();
    }

    for object in &editor.doc.objects {
        let transform =
            Transform::from_xyz(object.position[0], object.position[1], object.position[2])
                .with_rotation(Quat::from_rotation_y(object.yaw.to_radians()))
                .with_scale(Vec3::from(object.scale));
        let material = materials.add(StandardMaterial {
            base_color: Color::srgb(object.color[0], object.color[1], object.color[2]),
            perceptual_roughness: 0.9,
            ..default()
        });
        let mesh = match object.kind {
            ObjectKind::Hill => meshes.add(Sphere::new(0.5).mesh().ico(3).unwrap()),
            ObjectKind::Model => meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
            _ => meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
        };
        commands.spawn((Mesh3d(mesh), MeshMaterial3d(material), transform, Preview));
    }

    for spawn in &editor.doc.spawns {
        let color = if spawn.team == 0 {
            Color::srgb(0.2, 0.6, 1.0)
        } else {
            Color::srgb(1.0, 0.35, 0.25)
        };
        let material = materials.add(StandardMaterial {
            base_color: color,
            ..default()
        });
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::new(12.0, 120.0, 12.0))),
            MeshMaterial3d(material),
            Transform::from_xyz(spawn.position[0], 60.0, spawn.position[2]),
            Preview,
        ));
    }
}

// ---------------------------------------------------------------------------
// HUD
// ---------------------------------------------------------------------------

fn update_hud(editor: Res<Editor>, mut hud: Query<&mut Text, With<EditorHud>>) {
    let Ok(mut text) = hud.single_mut() else {
        return;
    };
    let selected = match editor
        .selected
        .and_then(|index| editor.doc.objects.get(index))
    {
        Some(object) => format!(
            "#{} {}  pos [{:.0}, {:.0}, {:.0}]  yaw {:.0}  scale [{:.0}, {:.0}, {:.0}]",
            editor.selected.unwrap(),
            object.kind.label(),
            object.position[0],
            object.position[1],
            object.position[2],
            object.yaw,
            object.scale[0],
            object.scale[1],
            object.scale[2],
        ),
        None => "none".to_string(),
    };

    let body = if let Some(field) = editor.input.field {
        format!(
            "TYPE ({}): {}_\nEnter confirm, Esc cancel",
            field_label(field),
            editor.input.buffer
        )
    } else if editor.mode == Mode::Planes {
        let mut lines = vec!["PLANES  (F1 back to the map)".to_string()];
        for (index, plane) in editor.planes.iter().enumerate() {
            let marker = if index == editor.plane_selected {
                ">"
            } else {
                " "
            };
            lines.push(format!(
                "{marker} {:<22} model: {}",
                plane.name,
                plane.model.as_deref().unwrap_or("(procedural)")
            ));
        }
        lines.push(String::new());
        lines.push("N new   R rename   M set model   Del delete   Ctrl+S save".to_string());
        lines.join("\n")
    } else {
        format!(
            "MAP '{name}'  ({id})   gamemode {gm}   objects {objects}  spawns {spawns}\n\
             Placing: {place}   Spawn team: {team}   Selected: {selected}\n\
             \n\
             Look: mouse   Move: WASD  Q/E  Shift-fast\n\
             LMB place   RMB select   Tab cycle   1-8 pick kind   B add spawn   T team\n\
             Arrows move   PgUp/PgDn height   [ ] yaw   - = scale   Del delete\n\
             Ctrl+N new   Ctrl+O reload   Ctrl+S save   F5 test server   F2 planes   Ctrl+Q quit",
            name = editor.doc.name,
            id = editor.doc.id,
            gm = editor.doc.gamemode,
            objects = editor.doc.objects.len(),
            spawns = editor.doc.spawns.len(),
            place = editor.place.label(),
            team = editor.spawn_team,
            selected = selected,
        )
    };

    **text = format!("{body}\n\n{status}", status = editor.status);
}

fn field_label(field: Field) -> &'static str {
    match field {
        Field::NewMap => "new map name",
        Field::Rename => "map name",
        Field::NewPlane => "new plane name",
        Field::PlaneName => "plane name",
        Field::Model => "glTF/GLB path",
    }
}

// ---------------------------------------------------------------------------
// Cursor & test server
// ---------------------------------------------------------------------------

fn manage_cursor(mut cursors: Query<&mut CursorOptions, With<PrimaryWindow>>) {
    let Ok(mut cursor) = cursors.single_mut() else {
        return;
    };
    cursor.visible = false;
    cursor.grab_mode = CursorGrabMode::Locked;
}

/// Start or stop the dedicated test server for the current map.
fn toggle_server(editor: &mut Editor) {
    if let Some(mut child) = editor.server.take() {
        let _ = child.kill();
        editor.status = "Test server stopped.".to_string();
        return;
    }
    save_map(editor);
    let map_id = editor.doc.id.clone();

    // Prefer a built server binary; otherwise fall back to `cargo run`.
    let mut command = if let Some(binary) = server_binary(&editor.server_dir) {
        Command::new(binary)
    } else {
        let mut command = Command::new("cargo");
        command.arg("run").arg("--").arg("--map").arg(&map_id);
        command.current_dir(&editor.server_dir);
        command
    };
    if !command.get_args().any(|arg| arg == "--map") {
        command.arg("--map").arg(&map_id);
    }
    command.current_dir(&editor.server_dir);
    match command.spawn() {
        Ok(child) => {
            editor.server = Some(child);
            editor.status = format!("Test server started on map '{map_id}' (bind 0.0.0.0:7777).");
        }
        Err(err) => editor.status = format!("Could not start the server: {err}"),
    }
}

fn server_binary(server_dir: &Path) -> Option<PathBuf> {
    let name = format!("openthunder_server{}", std::env::consts::EXE_SUFFIX);
    for profile in ["release", "debug"] {
        let candidate = server_dir.join("target").join(profile).join(&name);
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

fn poll_server(mut editor: ResMut<Editor>) {
    let Some(child) = editor.server.as_mut() else {
        return;
    };
    match child.try_wait() {
        Ok(Some(status)) => {
            editor.server = None;
            editor.status = format!("Test server exited ({status}).");
        }
        Ok(None) => {}
        Err(err) => {
            editor.server = None;
            editor.status = format!("Test server error: {err}");
        }
    }
}
