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

use openthunder::map_doc::{EmbeddedPlane, MapDoc, MapObject, ObjectKind, SpawnPoint};
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
                shortcuts,
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
    DialogPath,
}

#[derive(Default)]
struct TextInput {
    field: Option<Focus>,
    buffer: String,
}

/// The modal dialog shown over the editor.
#[derive(Clone, Copy, PartialEq, Eq)]
enum DialogKind {
    /// Create a plane in the mission's list.
    NewPlane,
    /// Embed a model file into the selected plane.
    SetModel,
    /// Load a `.plane` file into the mission's list.
    ImportPlane,
}

struct Dialog {
    kind: DialogKind,
    name: String,
    /// A filesystem path: a model file to embed, or a `.plane` file to import.
    path: String,
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

/// A terrain sculpting brush.
#[derive(Clone, Copy, PartialEq, Eq)]
enum TerrainTool {
    Raise,
    Lower,
    Smooth,
    Flatten,
    Mountain,
}

impl TerrainTool {
    const ALL: [TerrainTool; 5] = [
        TerrainTool::Raise,
        TerrainTool::Lower,
        TerrainTool::Smooth,
        TerrainTool::Flatten,
        TerrainTool::Mountain,
    ];

    fn label(self) -> &'static str {
        match self {
            TerrainTool::Raise => "Raise",
            TerrainTool::Lower => "Lower",
            TerrainTool::Smooth => "Smooth",
            TerrainTool::Flatten => "Flatten",
            TerrainTool::Mountain => "Mountain",
        }
    }
}

/// A flight-model field the plane inspector can nudge.
#[derive(Clone, Copy)]
enum PlaneProp {
    Mass,
    WingArea,
    WingSpan,
    MaxPower,
    StaticThrust,
    PropEfficiency,
    CriticalAltitude,
    ClMax,
    StallAoa,
    Cd0,
    Oswald,
    PitchRate,
    RollRate,
    YawRate,
    PitchStability,
    YawStability,
    ControlRefSpeed,
    Responsiveness,
    CruiseSpeed,
    MaxIas,
    GLimit,
    PropTorque,
    MaxHealth,
    Length,
    WingChord,
    TailSpan,
    ColorR,
    ColorG,
    ColorB,
}

impl PlaneProp {
    const ALL: [PlaneProp; 29] = [
        PlaneProp::Mass,
        PlaneProp::WingArea,
        PlaneProp::WingSpan,
        PlaneProp::MaxPower,
        PlaneProp::StaticThrust,
        PlaneProp::PropEfficiency,
        PlaneProp::CriticalAltitude,
        PlaneProp::ClMax,
        PlaneProp::StallAoa,
        PlaneProp::Cd0,
        PlaneProp::Oswald,
        PlaneProp::PitchRate,
        PlaneProp::RollRate,
        PlaneProp::YawRate,
        PlaneProp::PitchStability,
        PlaneProp::YawStability,
        PlaneProp::ControlRefSpeed,
        PlaneProp::Responsiveness,
        PlaneProp::CruiseSpeed,
        PlaneProp::MaxIas,
        PlaneProp::GLimit,
        PlaneProp::PropTorque,
        PlaneProp::MaxHealth,
        PlaneProp::Length,
        PlaneProp::WingChord,
        PlaneProp::TailSpan,
        PlaneProp::ColorR,
        PlaneProp::ColorG,
        PlaneProp::ColorB,
    ];

    fn label(self) -> &'static str {
        match self {
            PlaneProp::Mass => "Mass (kg)",
            PlaneProp::WingArea => "Wing area (m²)",
            PlaneProp::WingSpan => "Wing span (m)",
            PlaneProp::MaxPower => "Engine power (W)",
            PlaneProp::StaticThrust => "Static thrust (N)",
            PlaneProp::PropEfficiency => "Prop efficiency",
            PlaneProp::CriticalAltitude => "Critical altitude (m)",
            PlaneProp::ClMax => "CL max",
            PlaneProp::StallAoa => "Stall AoA (rad)",
            PlaneProp::Cd0 => "Parasite drag",
            PlaneProp::Oswald => "Oswald e",
            PlaneProp::PitchRate => "Pitch rate",
            PlaneProp::RollRate => "Roll rate",
            PlaneProp::YawRate => "Yaw rate",
            PlaneProp::PitchStability => "Pitch stability",
            PlaneProp::YawStability => "Yaw stability",
            PlaneProp::ControlRefSpeed => "Control ref speed",
            PlaneProp::Responsiveness => "Responsiveness",
            PlaneProp::CruiseSpeed => "Cruise speed (m/s)",
            PlaneProp::MaxIas => "Max IAS (m/s)",
            PlaneProp::GLimit => "G limit",
            PlaneProp::PropTorque => "Prop torque",
            PlaneProp::MaxHealth => "Max health",
            PlaneProp::Length => "Fuselage length (m)",
            PlaneProp::WingChord => "Wing chord (m)",
            PlaneProp::TailSpan => "Tail span (m)",
            PlaneProp::ColorR => "Body red",
            PlaneProp::ColorG => "Body green",
            PlaneProp::ColorB => "Body blue",
        }
    }

    /// A sensible nudge for this field.
    fn step(self) -> f32 {
        match self {
            PlaneProp::Mass => 100.0,
            PlaneProp::MaxPower => 20_000.0,
            PlaneProp::StaticThrust => 500.0,
            PlaneProp::CriticalAltitude => 250.0,
            PlaneProp::ColorR | PlaneProp::ColorG | PlaneProp::ColorB => 0.05,
            PlaneProp::MaxIas | PlaneProp::CruiseSpeed => 5.0,
            _ => 0.05,
        }
    }

    fn get(self, plane: &PlaneConfig) -> f32 {
        match self {
            PlaneProp::Mass => plane.mass,
            PlaneProp::WingArea => plane.wing_area,
            PlaneProp::WingSpan => plane.wing_span,
            PlaneProp::MaxPower => plane.max_power,
            PlaneProp::StaticThrust => plane.static_thrust,
            PlaneProp::PropEfficiency => plane.prop_efficiency,
            PlaneProp::CriticalAltitude => plane.critical_altitude,
            PlaneProp::ClMax => plane.cl_max,
            PlaneProp::StallAoa => plane.stall_aoa,
            PlaneProp::Cd0 => plane.cd0,
            PlaneProp::Oswald => plane.oswald,
            PlaneProp::PitchRate => plane.pitch_rate,
            PlaneProp::RollRate => plane.roll_rate,
            PlaneProp::YawRate => plane.yaw_rate,
            PlaneProp::PitchStability => plane.pitch_stability,
            PlaneProp::YawStability => plane.yaw_stability,
            PlaneProp::ControlRefSpeed => plane.control_ref_speed,
            PlaneProp::Responsiveness => plane.responsiveness,
            PlaneProp::CruiseSpeed => plane.cruise_speed,
            PlaneProp::MaxIas => plane.max_ias,
            PlaneProp::GLimit => plane.g_limit,
            PlaneProp::PropTorque => plane.prop_torque,
            PlaneProp::MaxHealth => plane.max_health,
            PlaneProp::Length => plane.length,
            PlaneProp::WingChord => plane.wing_chord,
            PlaneProp::TailSpan => plane.tail_span,
            PlaneProp::ColorR => plane.body_color[0],
            PlaneProp::ColorG => plane.body_color[1],
            PlaneProp::ColorB => plane.body_color[2],
        }
    }

    fn set(self, plane: &mut PlaneConfig, value: f32) {
        match self {
            PlaneProp::Mass => plane.mass = value.max(100.0),
            PlaneProp::WingArea => plane.wing_area = value.max(1.0),
            PlaneProp::WingSpan => plane.wing_span = value.max(1.0),
            PlaneProp::MaxPower => plane.max_power = value.max(0.0),
            PlaneProp::StaticThrust => plane.static_thrust = value.max(0.0),
            PlaneProp::PropEfficiency => plane.prop_efficiency = value.clamp(0.1, 1.0),
            PlaneProp::CriticalAltitude => plane.critical_altitude = value.max(0.0),
            PlaneProp::ClMax => plane.cl_max = value.clamp(0.2, 3.0),
            PlaneProp::StallAoa => plane.stall_aoa = value.clamp(0.05, 0.6),
            PlaneProp::Cd0 => plane.cd0 = value.clamp(0.005, 0.2),
            PlaneProp::Oswald => plane.oswald = value.clamp(0.4, 1.2),
            PlaneProp::PitchRate => plane.pitch_rate = value.max(0.1),
            PlaneProp::RollRate => plane.roll_rate = value.max(0.1),
            PlaneProp::YawRate => plane.yaw_rate = value.max(0.05),
            PlaneProp::PitchStability => plane.pitch_stability = value.max(0.0),
            PlaneProp::YawStability => plane.yaw_stability = value.max(0.0),
            PlaneProp::ControlRefSpeed => plane.control_ref_speed = value.max(10.0),
            PlaneProp::Responsiveness => plane.responsiveness = value.max(0.5),
            PlaneProp::CruiseSpeed => plane.cruise_speed = value.max(10.0),
            PlaneProp::MaxIas => plane.max_ias = value.max(20.0),
            PlaneProp::GLimit => plane.g_limit = value.clamp(2.0, 20.0),
            PlaneProp::PropTorque => plane.prop_torque = value.max(0.0),
            PlaneProp::MaxHealth => plane.max_health = value.max(1.0),
            PlaneProp::Length => plane.length = value.max(1.0),
            PlaneProp::WingChord => plane.wing_chord = value.max(0.2),
            PlaneProp::TailSpan => plane.tail_span = value.max(0.5),
            PlaneProp::ColorR => plane.body_color[0] = value.clamp(0.0, 1.0),
            PlaneProp::ColorG => plane.body_color[1] = value.clamp(0.0, 1.0),
            PlaneProp::ColorB => plane.body_color[2] = value.clamp(0.0, 1.0),
        }
    }
}

/// A solid-modelling part field the parts inspector can nudge.
#[derive(Clone, Copy)]
enum PartProp {
    PosX,
    PosY,
    PosZ,
    RotX,
    RotY,
    RotZ,
    ScaleX,
    ScaleY,
    ScaleZ,
}

impl PartProp {
    const ALL: [PartProp; 9] = [
        PartProp::PosX,
        PartProp::PosY,
        PartProp::PosZ,
        PartProp::RotX,
        PartProp::RotY,
        PartProp::RotZ,
        PartProp::ScaleX,
        PartProp::ScaleY,
        PartProp::ScaleZ,
    ];

    fn label(self) -> &'static str {
        match self {
            PartProp::PosX => "X",
            PartProp::PosY => "Y",
            PartProp::PosZ => "Z",
            PartProp::RotX => "Rot X",
            PartProp::RotY => "Rot Y",
            PartProp::RotZ => "Rot Z",
            PartProp::ScaleX => "Size X",
            PartProp::ScaleY => "Size Y",
            PartProp::ScaleZ => "Size Z",
        }
    }

    fn step(self) -> f32 {
        match self {
            PartProp::RotX | PartProp::RotY | PartProp::RotZ => 15.0,
            PartProp::ScaleX | PartProp::ScaleY | PartProp::ScaleZ => 0.5,
            _ => 0.5,
        }
    }

    fn get(self, part: &openthunder::solid::Shape) -> f32 {
        match self {
            PartProp::PosX => part.position[0],
            PartProp::PosY => part.position[1],
            PartProp::PosZ => part.position[2],
            PartProp::RotX => part.rotation[0],
            PartProp::RotY => part.rotation[1],
            PartProp::RotZ => part.rotation[2],
            PartProp::ScaleX => part.scale[0],
            PartProp::ScaleY => part.scale[1],
            PartProp::ScaleZ => part.scale[2],
        }
    }

    fn nudge(self, part: &mut openthunder::solid::Shape, delta: f32) {
        match self {
            PartProp::PosX => part.position[0] += delta,
            PartProp::PosY => part.position[1] += delta,
            PartProp::PosZ => part.position[2] += delta,
            PartProp::RotX => part.rotation[0] += delta,
            PartProp::RotY => part.rotation[1] += delta,
            PartProp::RotZ => part.rotation[2] += delta,
            PartProp::ScaleX => part.scale[0] = (part.scale[0] + delta).max(0.1),
            PartProp::ScaleY => part.scale[1] = (part.scale[1] + delta).max(0.1),
            PartProp::ScaleZ => part.scale[2] = (part.scale[2] + delta).max(0.1),
        }
    }
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
    /// Index into `doc.planes` (the mission's embedded plane list).
    plane_selected: usize,
    /// Index into the selected plane's solid parts.
    part_selected: usize,
    dialog: Option<Dialog>,
    input: TextInput,
    look: Vec2,
    speed: f32,
    /// The active terrain brush (`None` = place objects instead).
    terrain_tool: Option<TerrainTool>,
    brush_radius: f32,
    brush_strength: f32,
    /// Undo / redo snapshots of the document.
    undo: Vec<MapDoc>,
    redo: Vec<MapDoc>,
}

/// How many undo steps to keep.
const HISTORY_LIMIT: usize = 64;

impl Editor {
    fn new() -> Self {
        let server_dir = server_dir();
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
            plane_selected: 0,
            part_selected: 0,
            dialog: None,
            input: TextInput::default(),
            look: Vec2::new(0.0, -0.35),
            speed: 160.0,
            terrain_tool: None,
            brush_radius: 400.0,
            brush_strength: 40.0,
            undo: Vec::new(),
            redo: Vec::new(),
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
struct TerrainList;
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
    Undo,
    Redo,
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
    SetTerrainTool(Option<TerrainTool>),
    AddHeightmap,
    RemoveHeightmap,
    BrushRadius(f32),
    BrushStrength(f32),
    ToggleWater,
    WaterLevel(f32),
    SelectPlane(usize),
    OpenDialog(DialogKind),
    RenamePlane,
    ExportPlane,
    AddBuiltinPlanes,
    NudgePlane(PlaneProp, f32),
    ToggleWep,
    AddPart,
    DeletePart,
    SelectPart(usize),
    CyclePartShape,
    CyclePartOp,
    NudgePart(PartProp, f32),
    DeletePlane,
    FocusDialog(Focus),
    DialogConfirm,
    DialogCancel,
    Quit,
}

impl Action {
    /// Whether this action changes the document, so it should be undoable.
    /// (`Rename`/`Focus` actions only open an input; the change lands later in
    /// `apply_text`.)
    fn mutates(&self) -> bool {
        matches!(
            self,
            Action::NewMap
                | Action::ReloadMap
                | Action::DeleteObject
                | Action::CycleKind
                | Action::Nudge(..)
                | Action::AddSpawn
                | Action::ClearSpawns
                | Action::AddHeightmap
                | Action::RemoveHeightmap
                | Action::ToggleWater
                | Action::WaterLevel(..)
                | Action::AddBuiltinPlanes
                | Action::DeletePlane
                | Action::NudgePlane(..)
                | Action::ToggleWep
                | Action::AddPart
                | Action::DeletePart
                | Action::CyclePartShape
                | Action::CyclePartOp
                | Action::NudgePart(..)
                | Action::DialogConfirm
        )
    }
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

/// The built-in planes as embedded mission planes (config text, no models).
fn builtin_planes() -> Vec<EmbeddedPlane> {
    openthunder::plane_config::default_planes()
        .iter()
        .map(|config| EmbeddedPlane {
            id: config.id.clone(),
            config: config.to_config_string(),
            model_name: None,
            model: None,
        })
        .collect()
}

/// Read a model file from disk into `(file name, bytes)`.
fn read_model_file(path: &str) -> Option<(String, Vec<u8>)> {
    let path = Path::new(path);
    let name = path.file_name()?.to_str()?.to_string();
    let bytes = std::fs::read(path).ok()?;
    Some((name, bytes))
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

/// Export the selected mission plane to a standalone, self-contained `.plane`
/// file (config + model).
fn export_plane(editor: &mut Editor) {
    let Some(plane) = editor.doc.planes.get(editor.plane_selected) else {
        editor.status = "No plane selected.".to_string();
        return;
    };
    let dir = editor.planes_dir();
    if let Err(err) = std::fs::create_dir_all(&dir) {
        editor.status = format!("Could not create {}: {err}", dir.display());
        return;
    }
    let path = dir.join(format!("{}.plane", plane.id));
    match std::fs::write(&path, plane.to_file_string()) {
        Ok(()) => editor.status = format!("Exported plane to {}", path.display()),
        Err(err) => editor.status = format!("Export failed: {err}"),
    }
}

/// Load a `.plane` file into the mission's plane list.
fn import_plane(editor: &mut Editor, path: &str) {
    match std::fs::read_to_string(path) {
        Ok(text) => {
            let plane = EmbeddedPlane::parse_file(&text);
            let name = plane.display_name();
            let id = plane.id.clone();
            editor.doc.planes.push(plane);
            editor.plane_selected = editor.doc.planes.len() - 1;
            editor.dirty = true;
            editor.ui_dirty = true;
            editor.status = format!("Imported plane '{name}' ({id}).");
        }
        Err(err) => editor.status = format!("Import failed: {err}"),
    }
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
                button(bar, "Undo", Action::Undo, false);
                button(bar, "Redo", Action::Redo, false);
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
                    dock.spawn(text("Terrain", 13.0, DIM));
                    dock.spawn((
                        Node {
                            flex_direction: FlexDirection::Row,
                            flex_wrap: FlexWrap::Wrap,
                            column_gap: px(4),
                            row_gap: px(4),
                            ..default()
                        },
                        TerrainList,
                    ));
                    spacer_h(dock, 6.0);
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
                                width: percent(62),
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

/// Record the document before a change, so it can be undone.
fn snapshot(editor: &mut Editor) {
    editor.undo.push(editor.doc.clone());
    if editor.undo.len() > HISTORY_LIMIT {
        editor.undo.remove(0);
    }
    editor.redo.clear();
}

/// Undo the last change (`Ctrl+Z`).
fn undo(editor: &mut Editor) {
    match editor.undo.pop() {
        Some(previous) => {
            let current = std::mem::replace(&mut editor.doc, previous);
            editor.redo.push(current);
            editor.selected = None;
            editor.dirty = true;
            editor.ui_dirty = true;
            editor.status = format!("Undo ({} left).", editor.undo.len());
        }
        None => editor.status = "Nothing to undo.".to_string(),
    }
}

/// Redo the last undone change (`Ctrl+Shift+Z`).
fn redo(editor: &mut Editor) {
    match editor.redo.pop() {
        Some(next) => {
            let current = std::mem::replace(&mut editor.doc, next);
            editor.undo.push(current);
            editor.selected = None;
            editor.dirty = true;
            editor.ui_dirty = true;
            editor.status = format!("Redo ({} left).", editor.redo.len());
        }
        None => editor.status = "Nothing to redo.".to_string(),
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
    if action.mutates() {
        snapshot(editor);
    }
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
        Action::Undo => undo(editor),
        Action::Redo => redo(editor),
        Action::SetMode(mode) => {
            editor.mode = mode;
            editor.dirty = true;
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
        Action::SetTerrainTool(tool) => {
            editor.terrain_tool = tool;
            editor.status = match tool {
                Some(tool) => format!("{} brush: drag in the viewport.", tool.label()),
                None => "Object placement.".to_string(),
            };
            editor.ui_dirty = true;
        }
        Action::AddHeightmap => {
            editor.doc.terrain.make_heightmap(65, 0.0);
            editor.dirty = true;
            editor.ui_dirty = true;
            editor.status = "Added a flat heightmap — sculpt mountains with a brush.".to_string();
        }
        Action::RemoveHeightmap => {
            editor.doc.terrain.heightmap = None;
            editor.dirty = true;
            editor.ui_dirty = true;
            editor.status = "Removed the heightmap (flat ground).".to_string();
        }
        Action::BrushRadius(delta) => {
            editor.brush_radius = (editor.brush_radius + delta).clamp(50.0, 4000.0);
            editor.ui_dirty = true;
        }
        Action::BrushStrength(delta) => {
            editor.brush_strength = (editor.brush_strength + delta).clamp(2.0, 500.0);
            editor.ui_dirty = true;
        }
        Action::ToggleWater => {
            editor.doc.terrain.water = !editor.doc.terrain.water;
            editor.dirty = true;
            editor.ui_dirty = true;
        }
        Action::WaterLevel(delta) => {
            editor.doc.terrain.water_level += delta;
            editor.dirty = true;
            editor.ui_dirty = true;
        }
        Action::SelectPlane(index) => {
            editor.plane_selected = index;
            editor.ui_dirty = true;
        }
        Action::OpenDialog(kind) => {
            let (name, focus) = match kind {
                DialogKind::NewPlane => ("New plane".to_string(), Focus::DialogName),
                DialogKind::SetModel => (
                    editor
                        .doc
                        .planes
                        .get(editor.plane_selected)
                        .map(|plane| plane.display_name())
                        .unwrap_or_default(),
                    Focus::DialogPath,
                ),
                DialogKind::ImportPlane => ("Import plane file".to_string(), Focus::DialogPath),
            };
            editor.dialog = Some(Dialog {
                kind,
                name,
                path: String::new(),
                focus,
            });
            editor.input.field = None;
            editor.ui_dirty = true;
        }
        Action::RenamePlane => {
            editor.input.field = Some(Focus::PlaneName);
            editor.input.buffer = editor
                .doc
                .planes
                .get(editor.plane_selected)
                .map(|plane| plane.display_name())
                .unwrap_or_default();
            editor.status = "Type the new plane name, Enter to apply.".to_string();
        }
        Action::ExportPlane => export_plane(editor),
        Action::AddBuiltinPlanes => {
            let builtin = builtin_planes();
            let count = builtin.len();
            editor.doc.planes.extend(builtin);
            editor.dirty = true;
            editor.ui_dirty = true;
            editor.status = format!("Added {count} built-in planes to the mission.");
        }
        Action::DeletePlane => {
            if editor.plane_selected < editor.doc.planes.len() {
                let id = editor.doc.planes.remove(editor.plane_selected).id;
                editor.dirty = true;
                editor.ui_dirty = true;
                editor.status = format!("Removed plane '{id}' from the mission.");
            }
        }
        Action::NudgePlane(prop, delta) => {
            if let Some(plane) = editor.doc.planes.get_mut(editor.plane_selected) {
                let mut config = plane.config();
                let value = prop.get(&config) + delta;
                prop.set(&mut config, value);
                plane.set_config(&config);
                editor.dirty = true;
                editor.ui_dirty = true;
            }
        }
        Action::ToggleWep => {
            if let Some(plane) = editor.doc.planes.get_mut(editor.plane_selected) {
                let mut config = plane.config();
                config.has_wep = !config.has_wep;
                plane.set_config(&config);
                editor.dirty = true;
                editor.ui_dirty = true;
                editor.status = format!(
                    "WEP {}.",
                    if config.has_wep {
                        "enabled"
                    } else {
                        "disabled"
                    }
                );
            }
        }
        Action::AddPart => {
            if let Some(plane) = editor.doc.planes.get_mut(editor.plane_selected) {
                let mut config = plane.config();
                config.solid.parts.push(openthunder::solid::Shape::new(
                    openthunder::solid::Primitive::Box,
                    [0.0, 0.0, 0.0],
                    [1.0, 1.0, 1.0],
                ));
                editor.part_selected = config.solid.parts.len() - 1;
                plane.set_config(&config);
                editor.dirty = true;
                editor.ui_dirty = true;
                editor.status = "Added a box part.".to_string();
            }
        }
        Action::DeletePart => {
            if let Some(plane) = editor.doc.planes.get_mut(editor.plane_selected) {
                let mut config = plane.config();
                if editor.part_selected < config.solid.parts.len() {
                    config.solid.parts.remove(editor.part_selected);
                    editor.part_selected = editor.part_selected.saturating_sub(1);
                    plane.set_config(&config);
                    editor.dirty = true;
                    editor.ui_dirty = true;
                }
            }
        }
        Action::SelectPart(index) => {
            editor.part_selected = index;
            editor.ui_dirty = true;
        }
        Action::CyclePartShape => {
            if let Some(plane) = editor.doc.planes.get_mut(editor.plane_selected) {
                let mut config = plane.config();
                if let Some(part) = config.solid.parts.get_mut(editor.part_selected) {
                    let current = openthunder::solid::Primitive::ALL
                        .iter()
                        .position(|p| *p == part.primitive)
                        .unwrap_or(0);
                    part.primitive = openthunder::solid::Primitive::ALL
                        [(current + 1) % openthunder::solid::Primitive::ALL.len()];
                    plane.set_config(&config);
                    editor.dirty = true;
                    editor.ui_dirty = true;
                }
            }
        }
        Action::CyclePartOp => {
            if let Some(plane) = editor.doc.planes.get_mut(editor.plane_selected) {
                let mut config = plane.config();
                if let Some(part) = config.solid.parts.get_mut(editor.part_selected) {
                    let current = openthunder::solid::BoolOp::ALL
                        .iter()
                        .position(|op| *op == part.op)
                        .unwrap_or(0);
                    part.op = openthunder::solid::BoolOp::ALL
                        [(current + 1) % openthunder::solid::BoolOp::ALL.len()];
                    plane.set_config(&config);
                    editor.dirty = true;
                    editor.ui_dirty = true;
                }
            }
        }
        Action::NudgePart(prop, delta) => {
            if let Some(plane) = editor.doc.planes.get_mut(editor.plane_selected) {
                let mut config = plane.config();
                if let Some(part) = config.solid.parts.get_mut(editor.part_selected) {
                    prop.nudge(part, delta);
                    plane.set_config(&config);
                    editor.dirty = true;
                    editor.ui_dirty = true;
                }
            }
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
                editor.ui_dirty = true;
                return;
            }
            let id = openthunder::map_doc::slug(name);
            let config = PlaneConfig {
                id: id.clone(),
                name: name.to_string(),
                ..Default::default()
            };
            // Optionally embed a model from a file on disk.
            let path = dialog.path.trim();
            let model = (!path.is_empty()).then(|| read_model_file(path)).flatten();
            if !path.is_empty() && model.is_none() {
                editor.status =
                    format!("Could not read model '{path}'; plane created without one.");
            }
            let (model_name, model) = match model {
                Some((name, bytes)) => (Some(name), Some(bytes)),
                None => (None, None),
            };
            editor.doc.planes.push(EmbeddedPlane {
                id: id.clone(),
                config: config.to_config_string(),
                model_name,
                model,
            });
            editor.plane_selected = editor.doc.planes.len() - 1;
            editor.dirty = true;
            editor.status = format!("Created plane '{id}'.");
        }
        DialogKind::SetModel => {
            let path = dialog.path.trim();
            if path.is_empty() {
                if let Some(plane) = editor.doc.planes.get_mut(editor.plane_selected) {
                    plane.model = None;
                    plane.model_name = None;
                    editor.dirty = true;
                    editor.status = "Cleared the plane's model.".to_string();
                }
            } else if let Some((name, bytes)) = read_model_file(path) {
                if let Some(plane) = editor.doc.planes.get_mut(editor.plane_selected) {
                    plane.model_name = Some(name);
                    plane.model = Some(bytes);
                    editor.dirty = true;
                    editor.status = "Embedded the model in the mission.".to_string();
                }
            } else {
                editor.status = format!("Could not read model '{path}'.");
            }
        }
        DialogKind::ImportPlane => import_plane(editor, dialog.path.trim()),
    }
    editor.ui_dirty = true;
}

// ---------------------------------------------------------------------------
// Dynamic panels
// ---------------------------------------------------------------------------

/// A compact labelled `− value +` stepper used in the plane editor grids.
fn stepper_row(
    parent: &mut ChildSpawnerCommands,
    label: &str,
    value: f32,
    action: impl Fn(f32) -> Action,
) {
    parent
        .spawn(Node {
            width: px(330),
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: px(4),
            ..default()
        })
        .with_children(|row| {
            row.spawn(Node {
                width: px(140),
                ..default()
            })
            .with_children(|cell| {
                cell.spawn(text(label, 11.5, DIM));
            });
            button(row, "−", action(-1.0), false);
            row.spawn(Node {
                flex_grow: 1.0,
                justify_content: JustifyContent::Center,
                ..default()
            })
            .with_children(|cell| {
                cell.spawn(text(fmt_value(value), 12.0, TEXT));
            });
            button(row, "+", action(1.0), false);
        });
}

/// Format a value with a sensible number of decimals.
fn fmt_value(value: f32) -> String {
    if value.abs() >= 100.0 {
        format!("{value:.0}")
    } else if value.abs() >= 10.0 {
        format!("{value:.1}")
    } else {
        format!("{value:.2}")
    }
}

fn rebuild_ui(
    mut commands: Commands,
    mut editor: ResMut<Editor>,
    palette: Query<Entity, With<PaletteList>>,
    terrain: Query<Entity, With<TerrainList>>,
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

    if let Ok(entity) = terrain.single() {
        commands.entity(entity).despawn_related::<Children>();
        commands.entity(entity).with_children(|parent| {
            for tool in TerrainTool::ALL {
                button(
                    parent,
                    tool.label(),
                    Action::SetTerrainTool(Some(tool)),
                    editor.terrain_tool == Some(tool),
                );
            }
            button(
                parent,
                "Place objects",
                Action::SetTerrainTool(None),
                editor.terrain_tool.is_none(),
            );
            let has_map = editor.doc.terrain.heightmap.is_some();
            if has_map {
                button(parent, "Remove heightmap", Action::RemoveHeightmap, false);
            } else {
                button(parent, "Add heightmap", Action::AddHeightmap, false);
            }
            row(parent, |row| {
                row.spawn(text(
                    format!("Radius {:.0} m", editor.brush_radius),
                    11.0,
                    DIM,
                ));
                button(row, "−", Action::BrushRadius(-100.0), false);
                button(row, "+", Action::BrushRadius(100.0), false);
            });
            row(parent, |row| {
                row.spawn(text(
                    format!("Strength {:.0}", editor.brush_strength),
                    11.0,
                    DIM,
                ));
                button(row, "−", Action::BrushStrength(-10.0), false);
                button(row, "+", Action::BrushStrength(10.0), false);
            });
            row(parent, |row| {
                let water = editor.doc.terrain.water;
                button(
                    row,
                    if water { "Water: on" } else { "Water: off" },
                    Action::ToggleWater,
                    water,
                );
                button(row, "−", Action::WaterLevel(-5.0), false);
                row.spawn(text(
                    format!("{:.0} m", editor.doc.terrain.water_level),
                    11.0,
                    DIM,
                ));
                button(row, "+", Action::WaterLevel(5.0), false);
            });
        });
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
            parent.spawn(text("Mission planes", 15.0, TEXT));
            parent.spawn(text(
                "The mission carries each plane's config and model, so the server needs no setup. Click one to select it.",
                12.0,
                DIM,
            ));
            spacer_h(parent, 6.0);
            if editor.doc.planes.is_empty() {
                parent.spawn(text("(no planes in the mission yet)", 12.0, DIM));
            }
            for (index, plane) in editor.doc.planes.iter().enumerate() {
                let model = plane
                    .model
                    .as_ref()
                    .map(|bytes| format!("model {} KiB", bytes.len() / 1024))
                    .unwrap_or_else(|| "procedural".to_string());
                let label = format!("{}  ({model})", plane.display_name());
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
                    "Import .plane…",
                    Action::OpenDialog(DialogKind::ImportPlane),
                    false,
                );
            });
            row(parent, |row| {
                button(
                    row,
                    "Set model…",
                    Action::OpenDialog(DialogKind::SetModel),
                    false,
                );
                button(row, "Export .plane", Action::ExportPlane, false);
            });
            row(parent, |row| {
                button(row, "Add built-ins", Action::AddBuiltinPlanes, false);
                button(row, "Rename", Action::RenamePlane, false);
                button(row, "Delete", Action::DeletePlane, false);
            });

            // --- Flight characteristics ---
            spacer_h(parent, 10.0);
            parent.spawn(text("Flight characteristics", 14.0, TEXT));
            let selected_config = editor
                .doc
                .planes
                .get(editor.plane_selected)
                .map(|plane| plane.config());
            parent
                .spawn(Node {
                    width: percent(100),
                    flex_direction: FlexDirection::Row,
                    flex_wrap: FlexWrap::Wrap,
                    column_gap: px(14),
                    row_gap: px(2),
                    ..default()
                })
                .with_children(|grid| {
                    if let Some(config) = &selected_config {
                        for prop in PlaneProp::ALL {
                            stepper_row(grid, prop.label(), prop.get(config), move |m| {
                                Action::NudgePlane(prop, m * prop.step())
                            });
                        }
                    }
                });
            row(parent, |row| {
                let wep = selected_config
                    .as_ref()
                    .map(|config| config.has_wep)
                    .unwrap_or(false);
                button(
                    row,
                    if wep { "WEP: on" } else { "WEP: off" },
                    Action::ToggleWep,
                    wep,
                );
            });

            // --- Boolean-modelled parts ---
            spacer_h(parent, 10.0);
            parent.spawn(text("Model parts (boolean)", 14.0, TEXT));
            parent.spawn(text(
                "Primitives combined with union / subtract / intersect, meshed into the model.",
                11.0,
                DIM,
            ));
            if let Some(config) = &selected_config {
                if config.solid.parts.is_empty() {
                    parent.spawn(text(
                        "(using the procedural mesh — add a part to model one)",
                        11.0,
                        DIM,
                    ));
                }
                for (index, part) in config.solid.parts.iter().enumerate() {
                    let label =
                        format!("{index:>2}  {}  {}", part.primitive.label(), part.op.label());
                    list_row(
                        parent,
                        &label,
                        Action::SelectPart(index),
                        index == editor.part_selected,
                    );
                }
            }
            row(parent, |row| {
                button(row, "Add part", Action::AddPart, false);
                button(row, "Shape", Action::CyclePartShape, false);
                button(row, "Operation", Action::CyclePartOp, false);
                button(row, "Delete part", Action::DeletePart, false);
            });
            parent
                .spawn(Node {
                    width: percent(100),
                    flex_direction: FlexDirection::Row,
                    flex_wrap: FlexWrap::Wrap,
                    column_gap: px(14),
                    row_gap: px(2),
                    ..default()
                })
                .with_children(|grid| {
                    if let Some(part) = selected_config
                        .as_ref()
                        .and_then(|config| config.solid.parts.get(editor.part_selected))
                    {
                        for prop in PartProp::ALL {
                            stepper_row(grid, prop.label(), prop.get(part), move |m| {
                                Action::NudgePart(prop, m * prop.step())
                            });
                        }
                    }
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
                DialogKind::SetModel => "Embed a model",
                DialogKind::ImportPlane => "Import a .plane file",
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
            let (label, hint) = match dialog.kind {
                DialogKind::SetModel => (
                    "Model file (glTF/GLB)",
                    "Path to a .glb/.gltf file to embed in the mission. Leave empty to clear.",
                ),
                DialogKind::ImportPlane => (
                    "Plane file",
                    "Path to a .plane file to load into the mission's plane list.",
                ),
                DialogKind::NewPlane => (
                    "Model file (optional)",
                    "Path to a .glb/.gltf file to embed (optional).",
                ),
            };
            field_row(
                parent,
                label,
                &dialog.path,
                dialog.focus == Focus::DialogPath,
                Action::FocusDialog(Focus::DialogPath),
            );
            parent.spawn(text(hint, 11.0, DIM));
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

    if editor.mode == Mode::Map {
        if let Some(tool) = editor.terrain_tool {
            // Sculpting: apply the brush while the left button is held. One
            // history entry per drag, not per frame.
            if mouse.pressed(MouseButton::Left) {
                if let Some(point) = ground_hit(ray) {
                    if mouse.just_pressed(MouseButton::Left) {
                        snapshot(&mut editor);
                    }
                    sculpt(&mut editor, tool, point[0], point[2]);
                }
            }
        } else if mouse.just_pressed(MouseButton::Left) {
            if let Some(point) = ground_hit(ray) {
                snapshot(&mut editor);
                let mut object = MapObject::new(editor.place, point);
                object.position[1] = object.scale[1] * 0.5;
                editor.doc.objects.push(object);
                editor.selected = Some(editor.doc.objects.len() - 1);
                editor.dirty = true;
                editor.ui_dirty = true;
                editor.status = format!("Placed {}.", editor.place.label());
            }
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

/// Apply a terrain brush at a world position.
fn sculpt(editor: &mut Editor, tool: TerrainTool, x: f32, z: f32) {
    let radius = editor.brush_radius;
    let strength = editor.brush_strength;
    let Some(map) = editor.doc.terrain.heightmap.as_mut() else {
        editor.status = "Add a heightmap first (Terrain → Add heightmap).".to_string();
        return;
    };
    match tool {
        TerrainTool::Raise => map.sculpt(x, z, radius, strength),
        TerrainTool::Lower => map.sculpt(x, z, radius, -strength),
        TerrainTool::Smooth => map.smooth(x, z, radius, 0.6),
        TerrainTool::Flatten => {
            let target = map.sample(x, z);
            map.flatten(x, z, radius, target, 0.6);
        }
        TerrainTool::Mountain => map.sculpt(x, z, radius * 0.6, strength * 3.0),
    }
    editor.dirty = true;
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

/// Global keyboard shortcuts: undo (`Ctrl+Z`), redo (`Ctrl+Shift+Z` / `Ctrl+Y`)
/// and save (`Ctrl+S`).
fn shortcuts(keys: Res<ButtonInput<KeyCode>>, mut editor: ResMut<Editor>) {
    if editor.input.field.is_some() {
        return; // typing into a field
    }
    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    if !ctrl {
        return;
    }
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    if keys.just_pressed(KeyCode::KeyZ) {
        if shift {
            redo(&mut editor);
        } else {
            undo(&mut editor);
        }
    }
    if keys.just_pressed(KeyCode::KeyY) {
        redo(&mut editor);
    }
    if keys.just_pressed(KeyCode::KeyS) {
        save_map(&mut editor);
    }
}

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
            if !value.is_empty() && value != editor.doc.name {
                snapshot(editor);
                editor.doc.name = value;
                editor.dirty = true;
                editor.status = "Renamed map.".to_string();
            }
        }
        Focus::PlaneName => {
            if !value.is_empty() {
                let name = editor
                    .doc
                    .planes
                    .get(editor.plane_selected)
                    .map(|plane| plane.display_name());
                if name.as_deref() != Some(value.as_str()) {
                    snapshot(editor);
                    if let Some(plane) = editor.doc.planes.get_mut(editor.plane_selected) {
                        let mut config = PlaneConfig::parse(&plane.id, &plane.config);
                        config.name = value;
                        plane.config = config.to_config_string();
                        editor.dirty = true;
                        editor.status = "Renamed plane.".to_string();
                    }
                }
            }
        }
        Focus::DialogName => {
            if let Some(dialog) = editor.dialog.as_mut() {
                dialog.name = value;
            }
        }
        Focus::DialogPath => {
            if let Some(dialog) = editor.dialog.as_mut() {
                dialog.path = value;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Preview, status, test server
// ---------------------------------------------------------------------------

/// Build a Bevy `Mesh` from a boolean-modelled solid.
fn solid_mesh(solid: &openthunder::solid::Solid, resolution: usize) -> Mesh {
    use bevy::asset::RenderAssetUsages;
    use bevy::mesh::{Indices, PrimitiveTopology};

    let size = (solid.extent() * 2.2).max(1.0);
    let built = solid.mesh(resolution, size);
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, built.positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, built.normals)
    .with_inserted_indices(Indices::U32(built.indices))
}

/// Build a vertex-coloured mesh from a heightmap — the editor previews the same
/// mesh the game renders.
fn heightmap_mesh(
    map: &openthunder::terrain::Heightmap,
    ground_color: [f32; 3],
    rock_color: [f32; 3],
) -> Mesh {
    use bevy::asset::RenderAssetUsages;
    use bevy::color::LinearRgba;
    use bevy::mesh::{Indices, PrimitiveTopology};

    let n = map.resolution;
    let cell = map.cell();
    let half = map.size * 0.5;
    let ground = Color::srgb(ground_color[0], ground_color[1], ground_color[2]);
    let rock = Color::srgb(rock_color[0], rock_color[1], rock_color[2]);

    let mut positions = Vec::with_capacity(n * n);
    let mut normals = Vec::with_capacity(n * n);
    let mut colors = Vec::with_capacity(n * n);
    for iz in 0..n {
        for ix in 0..n {
            let x = -half + ix as f32 * cell;
            let z = -half + iz as f32 * cell;
            positions.push([x, map.height(ix, iz), z]);
            let left = map.height(ix.saturating_sub(1), iz);
            let right = map.height((ix + 1).min(n - 1), iz);
            let down = map.height(ix, iz.saturating_sub(1));
            let up = map.height(ix, (iz + 1).min(n - 1));
            let normal = Vec3::new(left - right, 2.0 * cell, down - up).normalize_or_zero();
            normals.push(normal.to_array());
            let slope = ((1.0 - normal.y.clamp(0.0, 1.0)) * 3.0).clamp(0.0, 1.0);
            let linear = LinearRgba::from(ground.mix(&rock, slope));
            colors.push([linear.red, linear.green, linear.blue, 1.0]);
        }
    }
    let mut indices = Vec::with_capacity((n - 1) * (n - 1) * 6);
    for iz in 0..n - 1 {
        for ix in 0..n - 1 {
            let a = (iz * n + ix) as u32;
            let b = a + 1;
            let c = a + n as u32;
            let d = c + 1;
            indices.extend_from_slice(&[a, c, b, b, c, d]);
        }
    }
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
    .with_inserted_indices(Indices::U32(indices))
}

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

    // In Planes mode, preview the selected plane's boolean model.
    if editor.mode == Mode::Planes {
        if let Some(plane) = editor.doc.planes.get(editor.plane_selected) {
            let solid = plane.config().solid;
            if !solid.is_empty() {
                let mesh = solid_mesh(&solid, 44);
                let material = materials.add(StandardMaterial {
                    base_color: Color::srgb(0.76, 0.77, 0.79),
                    perceptual_roughness: 0.6,
                    metallic: 0.2,
                    ..default()
                });
                commands.spawn((Mesh3d(meshes.add(mesh)), MeshMaterial3d(material), Preview));
            }
        }
        return;
    }

    // --- Terrain ---
    let terrain = &editor.doc.terrain;
    match &terrain.heightmap {
        Some(map) => {
            let material = materials.add(StandardMaterial {
                base_color: Color::WHITE,
                perceptual_roughness: 1.0,
                ..default()
            });
            commands.spawn((
                Mesh3d(meshes.add(heightmap_mesh(
                    map,
                    terrain.ground_color,
                    terrain.rock_color,
                ))),
                MeshMaterial3d(material),
                Preview,
            ));
        }
        None => {
            let material = materials.add(StandardMaterial {
                base_color: Color::srgb(
                    terrain.ground_color[0],
                    terrain.ground_color[1],
                    terrain.ground_color[2],
                ),
                perceptual_roughness: 1.0,
                ..default()
            });
            commands.spawn((
                Mesh3d(meshes.add(Plane3d::default().mesh().size(terrain.size, terrain.size))),
                MeshMaterial3d(material),
                Preview,
            ));
        }
    }
    if terrain.water {
        let material = materials.add(StandardMaterial {
            base_color: Color::srgb(
                terrain.water_color[0],
                terrain.water_color[1],
                terrain.water_color[2],
            ),
            perceptual_roughness: 0.35,
            metallic: 0.1,
            ..default()
        });
        commands.spawn((
            Mesh3d(
                meshes.add(
                    Plane3d::default()
                        .mesh()
                        .size(terrain.size * 3.0, terrain.size * 3.0),
                ),
            ),
            MeshMaterial3d(material),
            Transform::from_xyz(0.0, terrain.water_level, 0.0),
            Preview,
        ));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn undo_and_redo_restore_the_document() {
        let mut editor = Editor::new();
        let original = editor.doc.name.clone();

        snapshot(&mut editor);
        editor.doc.name = "Changed".to_string();

        undo(&mut editor);
        assert_eq!(editor.doc.name, original, "undo should restore the name");

        redo(&mut editor);
        assert_eq!(
            editor.doc.name, "Changed",
            "redo should re-apply the change"
        );
    }

    #[test]
    fn a_new_change_clears_the_redo_stack() {
        let mut editor = Editor::new();
        snapshot(&mut editor);
        editor.doc.name = "One".to_string();
        undo(&mut editor);
        assert!(!editor.redo.is_empty());

        // A fresh change invalidates the redo history.
        snapshot(&mut editor);
        editor.doc.name = "Two".to_string();
        assert!(editor.redo.is_empty());
    }

    #[test]
    fn undo_with_empty_history_is_a_no_op() {
        let mut editor = Editor::new();
        let before = editor.doc.clone();
        undo(&mut editor);
        assert_eq!(editor.doc, before);
        assert_eq!(editor.status, "Nothing to undo.");
    }
}
