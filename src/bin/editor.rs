//! OpenThunder mission editor.
//!
//! A mouse-driven, Godot-style 3D editor for building the maps/missions the
//! dedicated server hosts, creating planes (with glTF/GLB models), and launching
//! a local test server. Reachable from the TUI launcher.
//!
//! Run it with `cargo run --bin editor`.
//!
//! It reads and writes the **server's** `maps/` and `planes/` directories. By
//! default it looks for `../openthunder_server`; override with the
//! `OPENTHUNDER_SERVER_DIR` environment variable.
//!
//! ## Layout
//!
//! ```text
//! +---------------------------------------------------------------+
//! | toolbar: File  Map/Planes  Play      |          map name        |
//! +-----------+---------------------------------------+-----------+
//! | palette   |                                       | inspector |
//! | scene tree|            3D viewport                |           |
//! +-----------+---------------------------------------+-----------+
//! | status bar                                                     |
//! +---------------------------------------------------------------+
//! ```
//!
//! The left/right docks and the bottom bar are fixed sizes, so the viewport is a
//! known rectangle — clicks inside it place/select; clicks on the docks hit the
//! UI buttons.

use std::path::{Path, PathBuf};
use std::process::{Child, Command};

use bevy::input::keyboard::KeyboardInput;
use bevy::input::mouse::{AccumulatedMouseMotion, MouseWheel};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use openthunder::map_doc::{MapDoc, MapObject, ObjectKind, SpawnPoint};
use openthunder::plane_config::PlaneConfig;

// Dock sizes (logical pixels) — the viewport is everything inside them.
const TOP_H: f32 = 40.0;
const LEFT_W: f32 = 250.0;
const RIGHT_W: f32 = 320.0;
const BOTTOM_H: f32 = 26.0;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "OpenThunder — Mission Editor".to_string(),
                resolution: (1500, 940).into(),
                ..default()
            }),
            ..default()
        }))
        .insert_resource(ClearColor(Color::srgb(0.30, 0.45, 0.68)))
        .insert_resource(GlobalAmbientLight {
            color: Color::srgb(0.72, 0.78, 0.95),
            brightness: 650.0,
            ..default()
        })
        .insert_resource(Editor::new())
        .add_systems(Startup, (setup_scene, spawn_ui).chain())
        .add_systems(
            Update,
            (
                button_visuals,
                handle_buttons,
                rebuild_ui.after(handle_buttons),
                camera_control,
                viewport_input.after(handle_buttons),
                text_input,
                rebuild_preview,
                update_status,
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

/// A text field the keyboard is currently typing into.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Focus {
    MapName,
    PlaneName,
    DialogName,
    DialogModel,
}

#[derive(Default)]
struct TextInput {
    field: Option<Focus>,
    buffer: String,
}

/// The modal "new plane" / "set model" dialog.
#[derive(Clone, Copy, PartialEq, Eq)]
enum DialogKind {
    NewPlane,
    SetModel,
}

struct Dialog {
    kind: DialogKind,
    name: String,
    model: String,
    focus: Focus,
}

/// The property rows the inspector can nudge.
#[derive(Clone, Copy)]
enum Prop {
    PosX,
    PosY,
    PosZ,
    Yaw,
    ScaleX,
    ScaleY,
    ScaleZ,
    ColorR,
    ColorG,
    ColorB,
}

#[derive(Resource)]
struct Editor {
    doc: MapDoc,
    selected: Option<usize>,
    place: ObjectKind,
    spawn_team: u8,
    dirty: bool,
    /// Set when a UI panel must be rebuilt.
    ui_dirty: bool,
    status: String,
    server_dir: PathBuf,
    server: Option<Child>,
    mode: Mode,
    planes: Vec<PlaneConfig>,
    plane_selected: usize,
    dialog: Option<Dialog>,
    input: TextInput,
    look: Vec2,
    speed: f32,
}

impl Editor {
    fn new() -> Self {
        let server_dir = server_dir();
        let planes = load_planes(&server_dir);
        Self {
            doc: MapDoc::new("untitled"),
            selected: None,
            place: ObjectKind::Building,
            spawn_team: 0,
            dirty: true,
            ui_dirty: true,
            status: format!("Server directory: {}", server_dir.display()),
            server_dir,
            server: None,
            mode: Mode::Map,
            planes,
            plane_selected: 0,
            dialog: None,
            input: TextInput::default(),
            look: Vec2::new(0.0, -0.35),
            speed: 160.0,
        }
    }

    fn maps_dir(&self) -> PathBuf {
        self.server_dir.join("maps")
    }

    fn planes_dir(&self) -> PathBuf {
        self.server_dir.join("planes")
    }

    fn map_path(&self) -> PathBuf {
        self.maps_dir().join(format!("{}.map", self.doc.id))
    }

    fn selected_object(&self) -> Option<&MapObject> {
        self.selected.and_then(|index| self.doc.objects.get(index))
    }

    fn selected_object_mut(&mut self) -> Option<&mut MapObject> {
        let index = self.selected?;
        self.doc.objects.get_mut(index)
    }
}

// ---------------------------------------------------------------------------
// UI markers & actions
// ---------------------------------------------------------------------------

#[derive(Component)]
struct EditorCamera;

/// Container nodes whose children are rebuilt from state.
#[derive(Component)]
struct PaletteList;
#[derive(Component)]
struct SceneList;
#[derive(Component)]
struct InspectorList;
#[derive(Component)]
struct PlanesList;

/// The centre panel shown in Planes mode.
#[derive(Component)]
struct PlanesPanel;

#[derive(Component)]
struct ToolbarTitle;
#[derive(Component)]
struct StatusText;
#[derive(Component)]
struct DialogRoot;
#[derive(Component)]
struct DialogBody;

/// A clickable UI element and what it does.
#[derive(Component, Clone)]
struct ActionBtn(Action);

/// The base/active colours of a button, for hover/press feedback.
#[derive(Component)]
struct BtnColors {
    base: Color,
    active: Color,
    is_active: bool,
}

#[derive(Clone)]
enum Action {
    NewMap,
    SaveMap,
    ReloadMap,
    SetMode(Mode),
    ToggleServer,
    FocusMapName,
    PickKind(ObjectKind),
    SelectObject(usize),
    DeleteObject,
    CycleKind,
    Nudge(Prop, f32),
    AddSpawn,
    SwitchTeam,
    ClearSpawns,
    SelectPlane(usize),
    OpenDialog(DialogKind),
    RenamePlane,
    DeletePlane,
    FocusDialog(Focus),
    DialogConfirm,
    DialogCancel,
    Quit,
}

// ---------------------------------------------------------------------------
// Paths & files
// ---------------------------------------------------------------------------

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
    let Ok(entries) = std::fs::read_dir(server_dir.join("planes")) else {
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
        let plane = match std::fs::read_to_string(path.join("plane.conf")) {
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

fn load_map(editor: &mut Editor) {
    let id = editor.doc.id.clone();
    let path = editor.maps_dir().join(format!("{id}.map"));
    match std::fs::read_to_string(&path) {
        Ok(text) => {
            editor.doc = MapDoc::parse(&id, &text);
            editor.selected = None;
            editor.dirty = true;
            editor.ui_dirty = true;
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
    match std::fs::write(dir.join("plane.conf"), plane.to_config_string()) {
        Ok(()) => editor.status = format!("Saved plane '{}'.", plane.id),
        Err(err) => editor.status = format!("Save failed: {err}"),
    }
}

fn slug(value: &str) -> String {
    let slug: String = value
        .trim()
        .to_lowercase()
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
        .collect();
    slug.trim_matches('-').to_string()
}

// ---------------------------------------------------------------------------
// Scene setup
// ---------------------------------------------------------------------------

fn setup_scene(
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
        Transform::from_xyz(0.0, 260.0, 460.0).looking_at(Vec3::ZERO, Vec3::Y),
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

    let ground = materials.add(StandardMaterial {
        base_color: Color::srgb(0.24, 0.34, 0.18),
        perceptual_roughness: 1.0,
        ..default()
    });
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(40_000.0, 40_000.0))),
        MeshMaterial3d(ground),
    ));

    // 1 km grid.
    let line = meshes.add(Cuboid::new(1.0, 0.4, 20_000.0));
    let line_material = materials.add(StandardMaterial {
        base_color: Color::srgba(0.6, 0.7, 0.6, 0.45),
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
}

/// Preview geometry rebuilt from the document.
#[derive(Component)]
struct Preview;

// ---------------------------------------------------------------------------
// UI construction
// ---------------------------------------------------------------------------

const PANEL: Color = Color::srgb(0.13, 0.14, 0.16);
const BAR: Color = Color::srgb(0.16, 0.17, 0.20);
const BTN: Color = Color::srgb(0.24, 0.26, 0.30);
const BTN_ACTIVE: Color = Color::srgb(0.30, 0.52, 0.85);
const BORDER: Color = Color::srgb(0.28, 0.30, 0.34);
const TEXT: Color = Color::srgb(0.88, 0.90, 0.93);
const DIM: Color = Color::srgb(0.60, 0.63, 0.68);

fn text(content: impl Into<String>, size: f32, color: Color) -> (Text, TextFont, TextColor) {
    (
        Text::new(content),
        TextFont {
            font_size: FontSize::Px(size),
            ..default()
        },
        TextColor(color),
    )
}

/// A clickable button node.
fn button(parent: &mut ChildSpawnerCommands, label: &str, action: Action, active: bool) {
    let base = if active { BTN_ACTIVE } else { BTN };
    parent
        .spawn((
            Button,
            Node {
                padding: UiRect::axes(px(9), px(4)),
                border: UiRect::all(px(1)),
                ..default()
            },
            BackgroundColor(base),
            BorderColor::all(BORDER),
            ActionBtn(action),
            BtnColors {
                base,
                active: BTN_ACTIVE,
                is_active: active,
            },
        ))
        .with_children(|parent| {
            parent.spawn(text(label, 13.0, TEXT));
        });
}

fn spawn_ui(mut commands: Commands) {
    // Root: full-screen column of [toolbar, middle, status].
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                ..default()
            },
            GlobalZIndex(10),
        ))
        .with_children(|root| {
            // --- Toolbar ---
            root.spawn((
                Node {
                    width: percent(100),
                    height: px(TOP_H),
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: px(6),
                    padding: UiRect::horizontal(px(8)),
                    ..default()
                },
                BackgroundColor(BAR),
                BorderColor::all(BORDER),
            ))
            .with_children(|bar| {
                button(bar, "New", Action::NewMap, false);
                button(bar, "Save", Action::SaveMap, false);
                button(bar, "Reload", Action::ReloadMap, false);
                spacer(bar, 12.0);
                bar.spawn((
                    Text::new("  |  "),
                    TextFont {
                        font_size: FontSize::Px(14.0),
                        ..default()
                    },
                    TextColor(BORDER),
                ));
                button(bar, "Map", Action::SetMode(Mode::Map), false);
                button(bar, "Planes", Action::SetMode(Mode::Planes), false);
                spacer(bar, 12.0);
                button(bar, "Play / Stop test server", Action::ToggleServer, false);
                spacer(bar, 16.0);
                bar.spawn((
                    Button,
                    Node {
                        padding: UiRect::axes(px(9), px(4)),
                        ..default()
                    },
                    BackgroundColor(BAR),
                    ActionBtn(Action::FocusMapName),
                    BtnColors {
                        base: BAR,
                        active: BAR,
                        is_active: false,
                    },
                ))
                .with_children(|parent| {
                    parent.spawn((text("map: untitled", 14.0, TEXT), ToolbarTitle));
                });
                spacer(bar, 16.0);
                button(bar, "Quit", Action::Quit, false);
            });

            // --- Middle row: left dock | viewport | right dock ---
            root.spawn(Node {
                width: percent(100),
                flex_grow: 1.0,
                flex_direction: FlexDirection::Row,
                ..default()
            })
            .with_children(|row| {
                // Left dock: palette + scene tree.
                row.spawn((
                    Node {
                        width: px(LEFT_W),
                        height: percent(100),
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::all(px(6)),
                        row_gap: px(4),
                        ..default()
                    },
                    BackgroundColor(PANEL),
                    BorderColor::all(BORDER),
                ))
                .with_children(|dock| {
                    dock.spawn(text("Add object", 13.0, DIM));
                    dock.spawn((
                        Node {
                            flex_direction: FlexDirection::Row,
                            flex_wrap: FlexWrap::Wrap,
                            column_gap: px(4),
                            row_gap: px(4),
                            ..default()
                        },
                        PaletteList,
                    ));
                    spacer_h(dock, 6.0);
                    dock.spawn(text("Scene", 13.0, DIM));
                    dock.spawn((
                        Node {
                            width: percent(100),
                            flex_grow: 1.0,
                            flex_direction: FlexDirection::Column,
                            row_gap: px(2),
                            overflow: Overflow::clip(),
                            ..default()
                        },
                        SceneList,
                    ));
                });

                // Viewport / planes panel.
                row.spawn(Node {
                    flex_grow: 1.0,
                    height: percent(100),
                    ..default()
                })
                .with_children(|center| {
                    center
                        .spawn((
                            Node {
                                width: percent(100),
                                height: percent(100),
                                flex_direction: FlexDirection::Column,
                                padding: UiRect::all(px(10)),
                                row_gap: px(6),
                                display: Display::None,
                                overflow: Overflow::clip(),
                                ..default()
                            },
                            BackgroundColor(PANEL),
                            PlanesPanel,
                        ))
                        .with_children(|panel| {
                            panel.spawn((
                                Node {
                                    width: percent(100),
                                    flex_grow: 1.0,
                                    flex_direction: FlexDirection::Column,
                                    row_gap: px(4),
                                    overflow: Overflow::clip(),
                                    ..default()
                                },
                                PlanesList,
                            ));
                        });
                });

                // Right dock: inspector.
                row.spawn((
                    Node {
                        width: px(RIGHT_W),
                        height: percent(100),
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::all(px(6)),
                        row_gap: px(4),
                        ..default()
                    },
                    BackgroundColor(PANEL),
                    BorderColor::all(BORDER),
                ))
                .with_children(|dock| {
                    dock.spawn(text("Inspector", 13.0, DIM));
                    dock.spawn((
                        Node {
                            width: percent(100),
                            flex_grow: 1.0,
                            flex_direction: FlexDirection::Column,
                            row_gap: px(3),
                            overflow: Overflow::clip(),
                            ..default()
                        },
                        InspectorList,
                    ));
                });
            });

            // --- Status bar ---
            root.spawn((
                Node {
                    width: percent(100),
                    height: px(BOTTOM_H),
                    align_items: AlignItems::Center,
                    padding: UiRect::horizontal(px(8)),
                    ..default()
                },
                BackgroundColor(BAR),
                BorderColor::all(BORDER),
            ))
            .with_children(|bar| {
                bar.spawn((text("", 12.0, DIM), StatusText));
            });
        });

    // The modal dialog (hidden until opened).
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                display: Display::None,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.45)),
            GlobalZIndex(100),
            DialogRoot,
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    width: px(460),
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(px(14)),
                    row_gap: px(8),
                    border: UiRect::all(px(1)),
                    ..default()
                },
                BackgroundColor(PANEL),
                BorderColor::all(BORDER),
                DialogBody,
            ));
        });
}

fn spacer(parent: &mut ChildSpawnerCommands, width: f32) {
    parent.spawn(Node {
        width: px(width),
        height: px(1),
        ..default()
    });
}

fn spacer_h(parent: &mut ChildSpawnerCommands, height: f32) {
    parent.spawn(Node {
        width: percent(100),
        height: px(height),
        ..default()
    });
}

// ---------------------------------------------------------------------------
// Buttons: hover/press visuals and dispatch
// ---------------------------------------------------------------------------

fn button_visuals(
    mut buttons: Query<(&Interaction, &BtnColors, &mut BackgroundColor), Changed<Interaction>>,
) {
    for (interaction, colors, mut background) in &mut buttons {
        background.0 = match interaction {
            Interaction::Pressed => {
                if colors.is_active {
                    colors.active
                } else {
                    colors.base.mix(&Color::BLACK, 0.25)
                }
            }
            Interaction::Hovered => {
                if colors.is_active {
                    colors.active
                } else {
                    colors.base.mix(&Color::WHITE, 0.18)
                }
            }
            Interaction::None => {
                if colors.is_active {
                    colors.active
                } else {
                    colors.base
                }
            }
        };
    }
}

fn handle_buttons(
    interactions: Query<(&Interaction, &ActionBtn), (Changed<Interaction>, With<Button>)>,
    mut editor: ResMut<Editor>,
) {
    for (interaction, action) in &interactions {
        if *interaction != Interaction::Pressed {
            continue;
        }
        apply_action(&mut editor, action.0.clone());
    }
}

fn apply_action(editor: &mut Editor, action: Action) {
    match action {
        Action::NewMap => {
            editor.doc = MapDoc::new("untitled");
            editor.selected = None;
            editor.dirty = true;
            editor.ui_dirty = true;
            editor.status = "New map.".to_string();
        }
        Action::SaveMap => save_map(editor),
        Action::ReloadMap => load_map(editor),
        Action::SetMode(mode) => {
            editor.mode = mode;
            editor.ui_dirty = true;
        }
        Action::ToggleServer => toggle_server(editor),
        Action::FocusMapName => {
            editor.input.field = Some(Focus::MapName);
            editor.input.buffer = editor.doc.name.clone();
            editor.status = "Type the map name, Enter to apply, Esc to cancel.".to_string();
        }
        Action::PickKind(kind) => {
            editor.place = kind;
            editor.status = format!("Placing {}. Click the viewport.", kind.label());
            editor.ui_dirty = true;
        }
        Action::SelectObject(index) => {
            editor.selected = Some(index);
            editor.ui_dirty = true;
        }
        Action::DeleteObject => {
            if let Some(index) = editor.selected.take() {
                if index < editor.doc.objects.len() {
                    editor.doc.objects.remove(index);
                    editor.dirty = true;
                    editor.ui_dirty = true;
                    editor.status = "Deleted object.".to_string();
                }
            }
        }
        Action::CycleKind => {
            if let Some(object) = editor.selected_object_mut() {
                let current = ObjectKind::ALL
                    .iter()
                    .position(|kind| *kind == object.kind)
                    .unwrap_or(0);
                let next = ObjectKind::ALL[(current + 1) % ObjectKind::ALL.len()];
                object.kind = next;
                let scale = next.default_scale();
                let color = next.default_color();
                object.scale = scale;
                object.color = color;
                editor.dirty = true;
                editor.ui_dirty = true;
            }
        }
        Action::Nudge(prop, delta) => {
            if let Some(object) = editor.selected_object_mut() {
                match prop {
                    Prop::PosX => object.position[0] += delta,
                    Prop::PosY => object.position[1] += delta,
                    Prop::PosZ => object.position[2] += delta,
                    Prop::Yaw => object.yaw += delta,
                    Prop::ScaleX => object.scale[0] = (object.scale[0] + delta).max(0.5),
                    Prop::ScaleY => object.scale[1] = (object.scale[1] + delta).max(0.5),
                    Prop::ScaleZ => object.scale[2] = (object.scale[2] + delta).max(0.5),
                    Prop::ColorR => object.color[0] = (object.color[0] + delta).clamp(0.0, 1.0),
                    Prop::ColorG => object.color[1] = (object.color[1] + delta).clamp(0.0, 1.0),
                    Prop::ColorB => object.color[2] = (object.color[2] + delta).clamp(0.0, 1.0),
                }
                editor.dirty = true;
                editor.ui_dirty = true;
            }
        }
        Action::AddSpawn => {
            let team = editor.spawn_team;
            editor
                .doc
                .spawns
                .push(SpawnPoint::new(team, [0.0, 0.0, 0.0]));
            editor.dirty = true;
            editor.ui_dirty = true;
            editor.status = format!("Added spawn point for team {team}.");
        }
        Action::SwitchTeam => {
            editor.spawn_team = 1 - editor.spawn_team;
            editor.ui_dirty = true;
        }
        Action::ClearSpawns => {
            editor.doc.spawns.clear();
            editor.dirty = true;
            editor.ui_dirty = true;
        }
        Action::SelectPlane(index) => {
            editor.plane_selected = index;
            editor.ui_dirty = true;
        }
        Action::OpenDialog(kind) => {
            editor.dialog = Some(Dialog {
                kind,
                name: editor
                    .planes
                    .get(editor.plane_selected)
                    .map(|plane| plane.name.clone())
                    .unwrap_or_else(|| "New plane".to_string()),
                model: editor
                    .planes
                    .get(editor.plane_selected)
                    .and_then(|plane| plane.model.clone())
                    .unwrap_or_default(),
                focus: match kind {
                    DialogKind::NewPlane => Focus::DialogName,
                    DialogKind::SetModel => Focus::DialogModel,
                },
            });
            editor.input.field = None;
            editor.ui_dirty = true;
        }
        Action::RenamePlane => {
            editor.input.field = Some(Focus::PlaneName);
            editor.input.buffer = editor
                .planes
                .get(editor.plane_selected)
                .map(|plane| plane.name.clone())
                .unwrap_or_default();
            editor.status = "Type the new plane name, Enter to apply.".to_string();
        }
        Action::DeletePlane => {
            let Some(id) = editor
                .planes
                .get(editor.plane_selected)
                .map(|plane| plane.id.clone())
            else {
                return;
            };
            let _ = std::fs::remove_dir_all(editor.planes_dir().join(&id));
            editor.planes.remove(editor.plane_selected);
            editor.ui_dirty = true;
            editor.status = format!("Deleted plane '{id}'.");
        }
        Action::FocusDialog(focus) => {
            if let Some(dialog) = editor.dialog.as_mut() {
                dialog.focus = focus;
            }
            editor.ui_dirty = true;
        }
        Action::DialogConfirm => confirm_dialog(editor),
        Action::DialogCancel => {
            editor.dialog = None;
            editor.ui_dirty = true;
        }
        Action::Quit => std::process::exit(0),
    }
}

fn confirm_dialog(editor: &mut Editor) {
    let Some(dialog) = editor.dialog.take() else {
        return;
    };
    match dialog.kind {
        DialogKind::NewPlane => {
            let name = dialog.name.trim();
            if name.is_empty() {
                editor.status = "Plane name cannot be empty.".to_string();
            } else {
                let id = slug(name);
                let model = dialog.model.trim();
                let plane = PlaneConfig {
                    id: id.clone(),
                    name: name.to_string(),
                    model: (!model.is_empty()).then(|| model.to_string()),
                    ..Default::default()
                };
                editor.planes.push(plane);
                editor.plane_selected = editor.planes.len() - 1;
                save_plane(editor);
                editor.status = format!("Created plane '{id}'.");
            }
        }
        DialogKind::SetModel => {
            if let Some(plane) = editor.planes.get_mut(editor.plane_selected) {
                let model = dialog.model.trim();
                plane.model = (!model.is_empty()).then(|| model.to_string());
                save_plane(editor);
            }
        }
    }
    editor.ui_dirty = true;
}

// ---------------------------------------------------------------------------
// Dynamic panels
// ---------------------------------------------------------------------------

fn rebuild_ui(
    mut commands: Commands,
    mut editor: ResMut<Editor>,
    palette: Query<Entity, With<PaletteList>>,
    scene: Query<Entity, With<SceneList>>,
    inspector: Query<Entity, With<InspectorList>>,
    planes: Query<Entity, With<PlanesList>>,
    mut planes_panel: Query<&mut Node, With<PlanesPanel>>,
    dialog_root: Query<Entity, With<DialogRoot>>,
    dialog_body: Query<Entity, With<DialogBody>>,
) {
    if !editor.ui_dirty {
        return;
    }
    editor.ui_dirty = false;

    if let Ok(mut node) = planes_panel.single_mut() {
        node.display = if editor.mode == Mode::Planes {
            Display::Flex
        } else {
            Display::None
        };
    }

    if let Ok(entity) = palette.single() {
        commands.entity(entity).despawn_related::<Children>();
        commands.entity(entity).with_children(|parent| {
            for kind in ObjectKind::ALL {
                button(
                    parent,
                    kind.label(),
                    Action::PickKind(kind),
                    editor.place == kind,
                );
            }
        });
    }

    if let Ok(entity) = scene.single() {
        commands.entity(entity).despawn_related::<Children>();
        commands.entity(entity).with_children(|parent| {
            if editor.doc.objects.is_empty() {
                parent.spawn(text("(no objects yet)", 12.0, DIM));
            }
            for (index, object) in editor.doc.objects.iter().enumerate() {
                let label = format!("{index:>3}  {}", object.kind.label());
                let selected = editor.selected == Some(index);
                list_row(parent, &label, Action::SelectObject(index), selected);
            }
            if !editor.doc.spawns.is_empty() {
                parent.spawn(text("Spawn points", 12.0, DIM));
                for (index, spawn) in editor.doc.spawns.iter().enumerate() {
                    let label = format!("  spawn {} (team {})", index, spawn.team);
                    list_row(parent, &label, Action::AddSpawn, false);
                }
            }
            list_row(
                parent,
                &format!("+ Add spawn (team {})", editor.spawn_team),
                Action::AddSpawn,
                false,
            );
            list_row(parent, "Switch team", Action::SwitchTeam, false);
            list_row(parent, "Clear spawns", Action::ClearSpawns, false);
        });
    }

    if let Ok(entity) = inspector.single() {
        commands.entity(entity).despawn_related::<Children>();
        commands.entity(entity).with_children(|parent| {
            build_inspector(parent, &editor);
        });
    }

    if let Ok(entity) = planes.single() {
        commands.entity(entity).despawn_related::<Children>();
        commands.entity(entity).with_children(|parent| {
            parent.spawn(text("Planes", 15.0, TEXT));
            parent.spawn(text(
                "Click a plane to select it. \"New plane…\" opens a dialog.",
                12.0,
                DIM,
            ));
            spacer_h(parent, 6.0);
            for (index, plane) in editor.planes.iter().enumerate() {
                let label = format!(
                    "{}  ({})",
                    plane.name,
                    plane.model.as_deref().unwrap_or("procedural")
                );
                list_row(
                    parent,
                    &label,
                    Action::SelectPlane(index),
                    index == editor.plane_selected,
                );
            }
            spacer_h(parent, 8.0);
            row(parent, |row| {
                button(
                    row,
                    "New plane…",
                    Action::OpenDialog(DialogKind::NewPlane),
                    false,
                );
                button(
                    row,
                    "Set model…",
                    Action::OpenDialog(DialogKind::SetModel),
                    false,
                );
                button(row, "Rename", Action::RenamePlane, false);
                button(row, "Delete", Action::DeletePlane, false);
            });
        });
    }

    if let Ok(entity) = dialog_root.single() {
        let open = editor.dialog.is_some();
        commands.entity(entity).insert(Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            display: if open { Display::Flex } else { Display::None },
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        });
    }
    if let (Some(dialog), Ok(entity)) = (editor.dialog.as_ref(), dialog_body.single()) {
        commands.entity(entity).despawn_related::<Children>();
        commands.entity(entity).with_children(|parent| {
            let title = match dialog.kind {
                DialogKind::NewPlane => "New plane",
                DialogKind::SetModel => "Set model",
            };
            parent.spawn(text(title, 18.0, TEXT));
            if dialog.kind == DialogKind::NewPlane {
                field_row(
                    parent,
                    "Name",
                    &dialog.name,
                    dialog.focus == Focus::DialogName,
                    Action::FocusDialog(Focus::DialogName),
                );
            }
            field_row(
                parent,
                "Model (glTF/GLB)",
                &dialog.model,
                dialog.focus == Focus::DialogModel,
                Action::FocusDialog(Focus::DialogModel),
            );
            parent.spawn(text(
                "Model path is relative to the client's assets/ (optional).",
                11.0,
                DIM,
            ));
            spacer_h(parent, 6.0);
            row(parent, |row| {
                button(row, "Create", Action::DialogConfirm, true);
                button(row, "Cancel", Action::DialogCancel, false);
            });
        });
    }
}

/// A full-width clickable list row.
fn list_row(parent: &mut ChildSpawnerCommands, label: &str, action: Action, selected: bool) {
    let base = if selected { BTN_ACTIVE } else { BTN };
    parent
        .spawn((
            Button,
            Node {
                width: percent(100),
                padding: UiRect::axes(px(6), px(3)),
                ..default()
            },
            BackgroundColor(base),
            ActionBtn(action),
            BtnColors {
                base,
                active: BTN_ACTIVE,
                is_active: selected,
            },
        ))
        .with_children(|parent| {
            parent.spawn(text(label, 12.5, TEXT));
        });
}

/// A horizontal row of buttons/children.
fn row(parent: &mut ChildSpawnerCommands, build: impl FnOnce(&mut ChildSpawnerCommands)) {
    parent
        .spawn(Node {
            flex_direction: FlexDirection::Row,
            column_gap: px(6),
            ..default()
        })
        .with_children(build);
}

/// A clickable text field (used in the dialog).
fn field_row(
    parent: &mut ChildSpawnerCommands,
    label: &str,
    value: &str,
    focused: bool,
    action: Action,
) {
    parent.spawn(text(label, 12.0, DIM));
    let base = if focused { BTN_ACTIVE } else { BTN };
    parent
        .spawn((
            Button,
            Node {
                width: percent(100),
                padding: UiRect::axes(px(6), px(4)),
                border: UiRect::all(px(1)),
                ..default()
            },
            BackgroundColor(base),
            BorderColor::all(BORDER),
            ActionBtn(action),
            BtnColors {
                base,
                active: BTN_ACTIVE,
                is_active: focused,
            },
        ))
        .with_children(|parent| {
            let shown = if focused {
                format!("{value}_")
            } else if value.is_empty() {
                "(click to type)".to_string()
            } else {
                value.to_string()
            };
            parent.spawn(text(shown, 13.0, TEXT));
        });
}

fn build_inspector(parent: &mut ChildSpawnerCommands, editor: &Editor) {
    let Some(object) = editor.selected_object() else {
        parent.spawn(text(
            "Nothing selected.\n\nClick an object in the scene list, or right-click it in the viewport.",
            12.0,
            DIM,
        ));
        return;
    };

    parent.spawn(text(
        format!("Object #{}", editor.selected.unwrap()),
        14.0,
        TEXT,
    ));
    row(parent, |row| {
        button(
            row,
            &format!("Kind: {}", object.kind.label()),
            Action::CycleKind,
            false,
        );
        button(row, "Delete", Action::DeleteObject, false);
    });
    spacer_h(parent, 4.0);

    prop_row(parent, "X", object.position[0], Prop::PosX, 5.0);
    prop_row(parent, "Y", object.position[1], Prop::PosY, 5.0);
    prop_row(parent, "Z", object.position[2], Prop::PosZ, 5.0);
    prop_row(parent, "Yaw", object.yaw, Prop::Yaw, 5.0);
    prop_row(parent, "Scale X", object.scale[0], Prop::ScaleX, 2.0);
    prop_row(parent, "Scale Y", object.scale[1], Prop::ScaleY, 2.0);
    prop_row(parent, "Scale Z", object.scale[2], Prop::ScaleZ, 2.0);
    prop_row(parent, "Red", object.color[0], Prop::ColorR, 0.05);
    prop_row(parent, "Green", object.color[1], Prop::ColorG, 0.05);
    prop_row(parent, "Blue", object.color[2], Prop::ColorB, 0.05);
}

fn prop_row(parent: &mut ChildSpawnerCommands, label: &str, value: f32, prop: Prop, step: f32) {
    parent
        .spawn(Node {
            width: percent(100),
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: px(4),
            ..default()
        })
        .with_children(|row| {
            row.spawn(Node {
                width: px(70),
                ..default()
            })
            .with_children(|cell| {
                cell.spawn(text(label, 12.5, DIM));
            });
            button(row, "−", Action::Nudge(prop, -step), false);
            row.spawn(Node {
                flex_grow: 1.0,
                justify_content: JustifyContent::Center,
                ..default()
            })
            .with_children(|cell| {
                cell.spawn(text(format!("{value:.2}"), 13.0, TEXT));
            });
            button(row, "+", Action::Nudge(prop, step), false);
        });
}

// ---------------------------------------------------------------------------
// Camera (free-fly; look while the right mouse button is held)
// ---------------------------------------------------------------------------

fn camera_control(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    mut wheel: MessageReader<MouseWheel>,
    mut editor: ResMut<Editor>,
    mut camera: Query<&mut Transform, With<EditorCamera>>,
) {
    let Ok(mut transform) = camera.single_mut() else {
        return;
    };
    let dt = time.delta_secs();

    let mut scrolled = 0.0;
    for event in wheel.read() {
        scrolled += event.y;
    }

    if editor.input.field.is_none() && mouse.pressed(MouseButton::Right) {
        let sensitivity = 0.0025;
        editor.look.x -= motion.delta.x * sensitivity;
        editor.look.y = (editor.look.y - motion.delta.y * sensitivity).clamp(-1.5, 1.5);
    }
    transform.rotation =
        Quat::from_rotation_y(editor.look.x) * Quat::from_rotation_x(editor.look.y);

    let forward = transform.rotation * Vec3::NEG_Z;
    let right = transform.rotation * Vec3::X;
    let mut dir = Vec3::ZERO;
    if editor.input.field.is_none() {
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
    }
    dir += forward * scrolled * 0.6;
    let speed = if keys.pressed(KeyCode::ShiftLeft) {
        editor.speed * 6.0
    } else {
        editor.speed
    };
    transform.translation += dir.normalize_or_zero() * speed * dt;
}

// ---------------------------------------------------------------------------
// Viewport input
// ---------------------------------------------------------------------------

fn viewport_input(
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    camera: Query<(&Camera, &GlobalTransform), With<EditorCamera>>,
    mut editor: ResMut<Editor>,
) {
    if editor.input.field.is_some() || editor.dialog.is_some() {
        return;
    }
    let Ok(window) = windows.single() else {
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    let Ok((camera, camera_transform)) = camera.single() else {
        return;
    };
    let size = window.resolution.size();
    if !in_viewport(cursor, size) {
        return;
    }
    let Ok(ray) = camera.viewport_to_world(camera_transform, cursor) else {
        return;
    };

    if mouse.just_pressed(MouseButton::Left) && editor.mode == Mode::Map {
        if let Some(point) = ground_hit(ray) {
            let mut object = MapObject::new(editor.place, point);
            object.position[1] = object.scale[1] * 0.5;
            editor.doc.objects.push(object);
            editor.selected = Some(editor.doc.objects.len() - 1);
            editor.dirty = true;
            editor.ui_dirty = true;
            editor.status = format!("Placed {}.", editor.place.label());
        }
    }
    if mouse.just_pressed(MouseButton::Middle) && editor.mode == Mode::Map {
        editor.selected = pick(&editor.doc, ray);
        editor.ui_dirty = true;
        editor.status = match editor.selected {
            Some(index) => format!("Selected object #{index}."),
            None => "No object under the cursor.".to_string(),
        };
    }
}

/// Whether a point is inside the central viewport rectangle.
fn in_viewport(point: Vec2, size: Vec2) -> bool {
    point.x > LEFT_W && point.x < size.x - RIGHT_W && point.y > TOP_H && point.y < size.y - BOTTOM_H
}

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
// Text input
// ---------------------------------------------------------------------------

fn text_input(mut events: MessageReader<KeyboardInput>, mut editor: ResMut<Editor>) {
    let Some(field) = editor.input.field else {
        events.clear();
        return;
    };
    for event in events.read() {
        if !event.state.is_pressed() {
            continue;
        }
        match event.key_code {
            KeyCode::Enter => {
                let buffer = std::mem::take(&mut editor.input.buffer);
                editor.input.field = None;
                apply_text(&mut editor, field, buffer);
                editor.ui_dirty = true;
            }
            KeyCode::Escape => {
                editor.input.field = None;
                editor.input.buffer.clear();
                editor.status = "Cancelled.".to_string();
                editor.ui_dirty = true;
            }
            KeyCode::Backspace => {
                editor.input.buffer.pop();
                editor.ui_dirty = true;
            }
            _ => {
                if let Some(text) = &event.text {
                    for ch in text.chars() {
                        if ch.is_ascii_alphanumeric() || "-_/. ".contains(ch) {
                            editor.input.buffer.push(ch);
                        }
                    }
                    editor.ui_dirty = true;
                }
            }
        }
    }
}

fn apply_text(editor: &mut Editor, field: Focus, value: String) {
    let value = value.trim().to_string();
    match field {
        Focus::MapName => {
            if !value.is_empty() {
                editor.doc.name = value;
                editor.dirty = true;
                editor.status = "Renamed map.".to_string();
            }
        }
        Focus::PlaneName => {
            if let Some(plane) = editor.planes.get_mut(editor.plane_selected) {
                if !value.is_empty() {
                    plane.name = value;
                    save_plane(editor);
                }
            }
        }
        Focus::DialogName => {
            if let Some(dialog) = editor.dialog.as_mut() {
                dialog.name = value;
            }
        }
        Focus::DialogModel => {
            if let Some(dialog) = editor.dialog.as_mut() {
                dialog.model = value;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Preview, status, test server
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

fn update_status(
    editor: Res<Editor>,
    mut title: Query<&mut Text, (With<ToolbarTitle>, Without<StatusText>)>,
    mut status: Query<&mut Text, (With<StatusText>, Without<ToolbarTitle>)>,
) {
    if let Ok(mut text) = title.single_mut() {
        let mode = match editor.mode {
            Mode::Map => "Map",
            Mode::Planes => "Planes",
        };
        **text = format!("map: {}  [{mode}]", editor.doc.name);
    }
    if let Ok(mut text) = status.single_mut() {
        **text = editor.status.clone();
    }
}

fn toggle_server(editor: &mut Editor) {
    if let Some(mut child) = editor.server.take() {
        let _ = child.kill();
        editor.status = "Test server stopped.".to_string();
        return;
    }
    save_map(editor);
    let map_id = editor.doc.id.clone();

    let mut command = if let Some(binary) = server_binary(&editor.server_dir) {
        Command::new(binary)
    } else {
        let mut command = Command::new("cargo");
        command.arg("run").arg("--");
        command
    };
    if !command.get_args().any(|arg| arg == "--map") {
        command.arg("--map").arg(&map_id);
    }
    command.current_dir(&editor.server_dir);
    match command.spawn() {
        Ok(child) => {
            editor.server = Some(child);
            editor.status = format!("Test server started on '{map_id}' (0.0.0.0:7777).");
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
