//! Map documents — the data the mission editor produces and the server hosts.
//!
//! A map is a `key = value` file with `[section]` blocks, so it stays trivial to
//! read and hand-edit:
//!
//! ```text
//! name = Pacific Islands
//! gamemode = team_deathmatch
//! score_limit = 50
//!
//! [terrain]
//! ground_color = 0.20 0.32 0.13
//! water = true
//!
//! [spawn]
//! team = 0
//! position = 0 0 0
//! heading = 0
//!
//! [object]
//! kind = building
//! position = 100 0 200
//! yaw = 30
//! scale = 20 40 20
//! color = 0.36 0.34 0.31
//! ```
//!
//! The header keys (`name`, `gamemode`, and any other `key = value` before the
//! first section) are the **gamemode rules** the dedicated server passes through
//! untouched. Sections describe the world: the terrain, player spawn points, and
//! placed objects. A map with no `[object]`/`[terrain]` sections is a "rules
//! only" map and the client falls back to its built-in procedural terrain.
//!
//! **This module is the single source of truth for the format on the client.**
//! The server keeps its own small parser (`openthunder_server::map`) because it
//! does not depend on this crate.

use core::fmt::Write;

use crate::terrain::Heightmap;

/// A placed object's kind — a procedural primitive, or a custom glTF model.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ObjectKind {
    #[default]
    Building,
    Tree,
    Tank,
    FuelTank,
    Hill,
    Runway,
    Objective,
    /// A custom glTF/GLB model (the object's `model` field names the file).
    Model,
}

impl ObjectKind {
    /// Every kind, in palette order.
    pub const ALL: [ObjectKind; 8] = [
        ObjectKind::Building,
        ObjectKind::Tree,
        ObjectKind::Tank,
        ObjectKind::FuelTank,
        ObjectKind::Hill,
        ObjectKind::Runway,
        ObjectKind::Objective,
        ObjectKind::Model,
    ];

    /// Stable id used in the file.
    pub fn id(self) -> &'static str {
        match self {
            ObjectKind::Building => "building",
            ObjectKind::Tree => "tree",
            ObjectKind::Tank => "tank",
            ObjectKind::FuelTank => "fuel_tank",
            ObjectKind::Hill => "hill",
            ObjectKind::Runway => "runway",
            ObjectKind::Objective => "objective",
            ObjectKind::Model => "model",
        }
    }

    /// Human-readable name.
    pub fn label(self) -> &'static str {
        match self {
            ObjectKind::Building => "Building",
            ObjectKind::Tree => "Tree",
            ObjectKind::Tank => "Tank",
            ObjectKind::FuelTank => "Fuel tank",
            ObjectKind::Hill => "Hill",
            ObjectKind::Runway => "Runway",
            ObjectKind::Objective => "Objective",
            ObjectKind::Model => "3D model",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        Self::ALL.into_iter().find(|kind| kind.id() == text)
    }

    /// A sensible size for a freshly placed object (metres).
    pub fn default_scale(self) -> [f32; 3] {
        match self {
            ObjectKind::Building => [20.0, 30.0, 20.0],
            ObjectKind::Tree => [12.0, 30.0, 12.0],
            ObjectKind::Tank => [7.0, 3.0, 4.0],
            ObjectKind::FuelTank => [6.0, 8.0, 6.0],
            ObjectKind::Hill => [800.0, 150.0, 800.0],
            ObjectKind::Runway => [60.0, 1.0, 1400.0],
            ObjectKind::Objective => [30.0, 30.0, 30.0],
            ObjectKind::Model => [1.0, 1.0, 1.0],
        }
    }

    /// A sensible colour for a freshly placed object.
    pub fn default_color(self) -> [f32; 3] {
        match self {
            ObjectKind::Building => [0.36, 0.34, 0.31],
            ObjectKind::Tree => [0.10, 0.22, 0.08],
            ObjectKind::Tank => [0.28, 0.30, 0.22],
            ObjectKind::FuelTank => [0.55, 0.50, 0.35],
            ObjectKind::Hill => [0.20, 0.32, 0.13],
            ObjectKind::Runway => [0.10, 0.10, 0.11],
            ObjectKind::Objective => [0.85, 0.25, 0.15],
            ObjectKind::Model => [0.7, 0.7, 0.7],
        }
    }
}

/// A placed object in the world.
#[derive(Clone, Debug, PartialEq)]
pub struct MapObject {
    pub kind: ObjectKind,
    /// World position (metres). `y` is the centre of the object.
    pub position: [f32; 3],
    /// Rotation about the vertical axis, in degrees.
    pub yaw: f32,
    /// Size in metres.
    pub scale: [f32; 3],
    /// Base colour.
    pub color: [f32; 3],
    /// glTF/GLB file (relative to the map or an asset path) for [`ObjectKind::Model`].
    pub model: Option<String>,
}

impl MapObject {
    pub fn new(kind: ObjectKind, position: [f32; 3]) -> Self {
        Self {
            kind,
            position,
            yaw: 0.0,
            scale: kind.default_scale(),
            color: kind.default_color(),
            model: None,
        }
    }

    /// An axis-aligned half-extent, for culling / picking bounds.
    pub fn half_extent(&self) -> [f32; 3] {
        [
            self.scale[0].abs() * 0.5,
            self.scale[1].abs() * 0.5,
            self.scale[2].abs() * 0.5,
        ]
    }
}

/// A team's starting point.
#[derive(Clone, Debug, PartialEq)]
pub struct SpawnPoint {
    /// Team index (0/1), or `255` for any team.
    pub team: u8,
    pub position: [f32; 3],
    /// Facing, in degrees (0 = along −Z).
    pub heading: f32,
}

impl SpawnPoint {
    pub fn new(team: u8, position: [f32; 3]) -> Self {
        Self {
            team,
            position,
            heading: 0.0,
        }
    }
}

/// The world's terrain.
#[derive(Clone, Debug, PartialEq)]
pub struct Terrain {
    pub ground_color: [f32; 3],
    /// Colour of steep/high ground.
    pub rock_color: [f32; 3],
    /// Whether the world has water.
    pub water: bool,
    pub water_color: [f32; 3],
    /// Height at which the water surface sits; terrain below it is underwater.
    pub water_level: f32,
    /// Full width of the terrain in metres (it spans `-size/2 .. size/2`).
    pub size: f32,
    /// Optional sculpted heightmap (`None` = flat ground).
    pub heightmap: Option<Heightmap>,
}

impl Default for Terrain {
    fn default() -> Self {
        Self {
            ground_color: [0.17, 0.27, 0.12],
            rock_color: [0.45, 0.42, 0.38],
            water: false,
            water_color: [0.09, 0.28, 0.42],
            water_level: 0.0,
            size: 30_000.0,
            heightmap: None,
        }
    }
}

impl Terrain {
    /// Terrain height at a world position (flat ground when there is no
    /// heightmap).
    pub fn height_at(&self, x: f32, z: f32) -> f32 {
        match &self.heightmap {
            Some(map) => map.sample(x, z),
            None => 0.0,
        }
    }

    /// A fresh flat heightmap sized to this terrain, ready to sculpt.
    pub fn make_heightmap(&mut self, resolution: usize, height: f32) {
        self.heightmap = Some(Heightmap::flat(resolution, self.size, height));
    }
}

/// A plane embedded in a mission (or exported as its own file): its full
/// `plane.conf` text plus, optionally, the actual model bytes so no external
/// files are needed.
#[derive(Clone, Debug, PartialEq)]
pub struct EmbeddedPlane {
    /// Directory id, e.g. `f4u-4-corsair`.
    pub id: String,
    /// The complete `plane.conf` text (flight model, armament, model params).
    pub config: String,
    /// The model's file name, e.g. `fighter.glb` (kept so the extension/format
    /// survives a round trip). `None` means "procedural mesh only".
    pub model_name: Option<String>,
    /// The raw glTF/GLB bytes, embedded so the mission is self-contained.
    pub model: Option<Vec<u8>>,
}

impl EmbeddedPlane {
    /// The plane's display name, read from its config.
    pub fn display_name(&self) -> String {
        crate::plane_config::PlaneConfig::parse(&self.id, &self.config).name
    }

    /// Parse the plane's full config.
    pub fn config(&self) -> crate::plane_config::PlaneConfig {
        crate::plane_config::PlaneConfig::parse(&self.id, &self.config)
    }

    /// Replace the config text from a parsed config.
    pub fn set_config(&mut self, config: &crate::plane_config::PlaneConfig) {
        self.config = config.to_config_string();
    }

    /// Serialize as a standalone, self-contained plane file.
    pub fn to_file_string(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "# OpenThunder plane — {}", self.display_name());
        let _ = writeln!(out, "id = {}", self.id);
        out.push_str(&self.config);
        if !self.config.ends_with('\n') {
            out.push('\n');
        }
        if let Some(name) = &self.model_name {
            let _ = writeln!(out, "model_name = {name}");
        }
        if let Some(model) = &self.model {
            let _ = writeln!(out, "model_data = {}", crate::base64::encode(model));
        }
        out
    }

    /// Parse a standalone plane file (or the body of a `[plane <id>]` block).
    pub fn parse_file(text: &str) -> Self {
        let mut plane = EmbeddedPlane {
            id: String::new(),
            config: String::new(),
            model_name: None,
            model: None,
        };
        let mut model_data: Option<String> = None;
        for raw in text.lines() {
            let trimmed = raw.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = trimmed.split_once('=') {
                match key.trim() {
                    "id" => {
                        plane.id = value.trim().to_string();
                        continue;
                    }
                    "model_name" => {
                        plane.model_name = Some(value.trim().to_string());
                        continue;
                    }
                    "model_data" => {
                        model_data = Some(value.trim().to_string());
                        continue;
                    }
                    _ => {}
                }
            }
            plane.config.push_str(raw);
            plane.config.push('\n');
        }
        plane.model = model_data.as_deref().and_then(crate::base64::decode);
        if plane.id.is_empty() {
            plane.id = slug(&plane.display_name());
        }
        plane
    }
}

/// A filesystem/asset-safe id from a display name.
pub fn slug(name: &str) -> String {
    let slug: String = name
        .trim()
        .to_lowercase()
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
        .collect();
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        "plane".to_string()
    } else {
        slug
    }
}

/// Which `[section]` the parser is currently in.
#[derive(Clone, Copy, PartialEq)]
enum Section {
    Header,
    Terrain,
    Spawn,
    Object,
    Plane,
}

/// Incremental parser state for [`MapDoc::parse`].
struct ParseState {
    doc: MapDoc,
    section: Section,
    spawn: SpawnPoint,
    object: MapObject,
    scale_set: bool,
    color_set: bool,
    plane_id: String,
    plane_config: String,
    plane_model_name: Option<String>,
    plane_model_data: Option<String>,
    terrain_resolution: usize,
    terrain_heights: Option<String>,
}

impl ParseState {
    /// Commit the current section's entry and leave the section.
    fn flush(&mut self) {
        match self.section {
            Section::Spawn => self.doc.spawns.push(self.spawn.clone()),
            Section::Object => self.doc.objects.push(self.object.clone()),
            Section::Plane => self.doc.planes.push(EmbeddedPlane {
                id: self.plane_id.clone(),
                config: std::mem::take(&mut self.plane_config),
                model_name: self.plane_model_name.take(),
                model: self
                    .plane_model_data
                    .take()
                    .and_then(|data| crate::base64::decode(&data)),
            }),
            _ => {}
        }
    }
}

/// A parsed map.
#[derive(Clone, Debug, PartialEq)]
pub struct MapDoc {
    /// File stem, e.g. `pacific_islands`.
    pub id: String,
    pub name: String,
    pub gamemode: String,
    /// Gamemode rules, in file order.
    pub rules: Vec<(String, String)>,
    pub terrain: Terrain,
    pub spawns: Vec<SpawnPoint>,
    pub objects: Vec<MapObject>,
    /// Planes embedded in the mission (configs + models), so it is self-contained.
    pub planes: Vec<EmbeddedPlane>,
}

impl MapDoc {
    /// A new, empty map with sensible defaults.
    pub fn new(id: &str) -> Self {
        Self {
            id: id.to_string(),
            name: id.to_string(),
            gamemode: "free_flight".to_string(),
            rules: vec![("max_players".to_string(), "16".to_string())],
            terrain: Terrain::default(),
            spawns: Vec::new(),
            objects: Vec::new(),
            planes: Vec::new(),
        }
    }

    /// Whether this map places any world geometry (vs a rules-only map).
    pub fn has_world(&self) -> bool {
        !self.objects.is_empty() || !self.spawns.is_empty()
    }

    pub fn rule(&self, key: &str) -> Option<&str> {
        self.rules
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// Set (or add) a rule, preserving order.
    pub fn set_rule(&mut self, key: &str, value: &str) {
        if let Some(entry) = self.rules.iter_mut().find(|(k, _)| k == key) {
            entry.1 = value.to_string();
        } else {
            self.rules.push((key.to_string(), value.to_string()));
        }
    }

    pub fn parse(id: &str, text: &str) -> Self {
        let mut state = ParseState {
            doc: MapDoc {
                id: id.to_string(),
                name: id.to_string(),
                gamemode: "free_flight".to_string(),
                rules: Vec::new(),
                terrain: Terrain::default(),
                spawns: Vec::new(),
                objects: Vec::new(),
                planes: Vec::new(),
            },
            section: Section::Header,
            spawn: SpawnPoint::new(255, [0.0; 3]),
            object: MapObject::new(ObjectKind::default(), [0.0; 3]),
            scale_set: false,
            color_set: false,
            plane_id: String::new(),
            plane_config: String::new(),
            plane_model_name: None,
            plane_model_data: None,
            terrain_resolution: 65,
            terrain_heights: None,
        };

        for raw in text.lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(inner) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                let inner = inner.trim();
                let lower = inner.to_lowercase();
                // `[gun N]` inside a plane block is part of the plane's config.
                if state.section == Section::Plane && lower.starts_with("gun") {
                    state.plane_config.push_str(line);
                    state.plane_config.push('\n');
                    continue;
                }
                state.flush();
                if let Some(rest) = inner.strip_prefix("plane") {
                    state.plane_id = rest.trim().to_string();
                    state.plane_config.clear();
                    state.plane_model_name = None;
                    state.plane_model_data = None;
                    state.section = Section::Plane;
                } else {
                    state.section = match lower.as_str() {
                        "terrain" => Section::Terrain,
                        "spawn" => {
                            state.spawn = SpawnPoint::new(255, [0.0; 3]);
                            Section::Spawn
                        }
                        "object" => {
                            state.object = MapObject::new(ObjectKind::default(), [0.0; 3]);
                            state.scale_set = false;
                            state.color_set = false;
                            Section::Object
                        }
                        _ => Section::Header,
                    };
                }
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim();
            let value = value.trim();
            match state.section {
                Section::Header => match key {
                    "name" => state.doc.name = value.to_string(),
                    "gamemode" => state.doc.gamemode = value.to_string(),
                    other => state.doc.rules.push((other.to_string(), value.to_string())),
                },
                Section::Terrain => match key {
                    "ground_color" => set_color(&mut state.doc.terrain.ground_color, value),
                    "rock_color" => set_color(&mut state.doc.terrain.rock_color, value),
                    "water_color" => set_color(&mut state.doc.terrain.water_color, value),
                    "water" => {
                        state.doc.terrain.water =
                            parse_bool(value).unwrap_or(state.doc.terrain.water)
                    }
                    "water_level" => {
                        state.doc.terrain.water_level =
                            parse_f32(value).unwrap_or(state.doc.terrain.water_level)
                    }
                    "size" => {
                        state.doc.terrain.size = parse_f32(value).unwrap_or(state.doc.terrain.size)
                    }
                    "resolution" => {
                        state.terrain_resolution = value.parse().unwrap_or(state.terrain_resolution)
                    }
                    "heights" => state.terrain_heights = Some(value.to_string()),
                    _ => {}
                },
                Section::Spawn => match key {
                    "team" => state.spawn.team = value.parse().unwrap_or(state.spawn.team),
                    "position" => set_vec3(&mut state.spawn.position, value),
                    "heading" => {
                        state.spawn.heading = parse_f32(value).unwrap_or(state.spawn.heading)
                    }
                    _ => {}
                },
                Section::Object => match key {
                    "kind" => {
                        if let Some(kind) = ObjectKind::parse(value) {
                            state.object.kind = kind;
                            if !state.scale_set {
                                state.object.scale = kind.default_scale();
                            }
                            if !state.color_set {
                                state.object.color = kind.default_color();
                            }
                        }
                    }
                    "position" => set_vec3(&mut state.object.position, value),
                    "yaw" | "rotation" => {
                        state.object.yaw = parse_f32(value).unwrap_or(state.object.yaw)
                    }
                    "scale" => {
                        set_vec3(&mut state.object.scale, value);
                        state.scale_set = true;
                    }
                    "color" => {
                        set_color(&mut state.object.color, value);
                        state.color_set = true;
                    }
                    "model" => state.object.model = Some(value.to_string()),
                    _ => {}
                },
                // Everything else in a plane block (name, mass, [gun N], …) is
                // the plane's config; `model_name`/`model_data` are metadata.
                Section::Plane => match key {
                    "model_name" => state.plane_model_name = Some(value.to_string()),
                    "model_data" => state.plane_model_data = Some(value.to_string()),
                    _ => {
                        state.plane_config.push_str(line);
                        state.plane_config.push('\n');
                    }
                },
            }
        }
        state.flush();
        // The heightmap is stored as one Base64 blob, decoded last so `size` and
        // `resolution` are known regardless of line order.
        if let Some(text) = state.terrain_heights.take() {
            state.doc.terrain.heightmap =
                Heightmap::from_base64(state.terrain_resolution, state.doc.terrain.size, &text);
        }
        state.doc
    }

    /// Serialize back to the map format.
    pub fn to_string(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(
            out,
            "# {} — created with the OpenThunder mission editor",
            self.name
        );
        let _ = writeln!(out, "name = {}", self.name);
        let _ = writeln!(out, "gamemode = {}", self.gamemode);
        for (key, value) in &self.rules {
            let _ = writeln!(out, "{key} = {value}");
        }

        for plane in &self.planes {
            let _ = writeln!(out, "\n[plane {}]", plane.id);
            out.push_str(&plane.config);
            if !plane.config.ends_with('\n') {
                out.push('\n');
            }
            if let Some(name) = &plane.model_name {
                let _ = writeln!(out, "model_name = {name}");
            }
            if let Some(model) = &plane.model {
                let _ = writeln!(out, "model_data = {}", crate::base64::encode(model));
            }
        }

        if self.terrain != Terrain::default() {
            let _ = writeln!(out, "\n[terrain]");
            let _ = writeln!(
                out,
                "ground_color = {}",
                fmt_color(self.terrain.ground_color)
            );
            let _ = writeln!(out, "rock_color = {}", fmt_color(self.terrain.rock_color));
            let _ = writeln!(out, "water = {}", self.terrain.water);
            let _ = writeln!(out, "water_color = {}", fmt_color(self.terrain.water_color));
            let _ = writeln!(out, "water_level = {}", self.terrain.water_level);
            let _ = writeln!(out, "size = {}", self.terrain.size);
            if let Some(map) = &self.terrain.heightmap {
                let _ = writeln!(out, "resolution = {}", map.resolution);
                let _ = writeln!(out, "heights = {}", map.to_base64());
            }
        }

        for spawn in &self.spawns {
            let _ = writeln!(out, "\n[spawn]");
            let _ = writeln!(out, "team = {}", spawn.team);
            let _ = writeln!(out, "position = {}", fmt_vec3(spawn.position));
            let _ = writeln!(out, "heading = {}", spawn.heading);
        }

        for object in &self.objects {
            let _ = writeln!(out, "\n[object]");
            let _ = writeln!(out, "kind = {}", object.kind.id());
            let _ = writeln!(out, "position = {}", fmt_vec3(object.position));
            let _ = writeln!(out, "yaw = {}", object.yaw);
            let _ = writeln!(out, "scale = {}", fmt_vec3(object.scale));
            let _ = writeln!(out, "color = {}", fmt_color(object.color));
            if let Some(model) = &object.model {
                let _ = writeln!(out, "model = {model}");
            }
        }
        out
    }
}

fn parse_f32(text: &str) -> Option<f32> {
    text.trim().parse().ok()
}

fn parse_bool(text: &str) -> Option<bool> {
    match text.trim().to_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Some(true),
        "false" | "0" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn parse_vec3(text: &str) -> Option<[f32; 3]> {
    let parts: Vec<f32> = text
        .split_whitespace()
        .filter_map(|p| p.parse().ok())
        .collect();
    if parts.len() == 3 {
        Some([parts[0], parts[1], parts[2]])
    } else {
        None
    }
}

fn set_vec3(target: &mut [f32; 3], text: &str) {
    if let Some(value) = parse_vec3(text) {
        *target = value;
    }
}

fn set_color(target: &mut [f32; 3], text: &str) {
    if let Some(value) = parse_vec3(text) {
        *target = value;
    }
}

fn fmt_vec3(value: [f32; 3]) -> String {
    format!("{} {} {}", value[0], value[1], value[2])
}

fn fmt_color(value: [f32; 3]) -> String {
    format!("{:.3} {:.3} {:.3}", value[0], value[1], value[2])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_a_full_map() {
        let mut doc = MapDoc::new("test");
        doc.name = "Test Island".into();
        doc.gamemode = "team_deathmatch".into();
        doc.set_rule("score_limit", "30");
        doc.terrain.water = true;
        doc.terrain.ground_color = [0.2, 0.3, 0.15];
        doc.spawns.push(SpawnPoint::new(0, [0.0, 100.0, 0.0]));
        doc.spawns.push(SpawnPoint::new(1, [500.0, 100.0, 500.0]));
        doc.objects
            .push(MapObject::new(ObjectKind::Tank, [100.0, 2.0, 200.0]));
        let mut model = MapObject::new(ObjectKind::Model, [0.0, 0.0, 0.0]);
        model.model = Some("plane.glb".into());
        doc.objects.push(model);

        let text = doc.to_string();
        let parsed = MapDoc::parse("test", &text);
        assert_eq!(parsed, doc, "map round-trip failed:\n{text}");
    }

    #[test]
    fn parses_the_built_in_style_header() {
        let text = "# comment\nname = Pacific Islands\ngamemode = team_deathmatch\nmax_players = 16\nscore_limit = 50\ntime_limit = 900\n";
        let doc = MapDoc::parse("pacific_islands", text);
        assert_eq!(doc.name, "Pacific Islands");
        assert_eq!(doc.gamemode, "team_deathmatch");
        assert_eq!(doc.rule("score_limit"), Some("50"));
        assert_eq!(doc.rule("max_players"), Some("16"));
        // A header-only map has no world data, so the client keeps its terrain.
        assert!(!doc.has_world());
    }

    #[test]
    fn section_keys_do_not_leak_into_rules() {
        let text = "name = X\n[object]\nkind = tree\nposition = 1 2 3\n";
        let doc = MapDoc::parse("x", text);
        assert_eq!(doc.objects.len(), 1);
        assert_eq!(doc.objects[0].kind, ObjectKind::Tree);
        assert_eq!(doc.objects[0].position, [1.0, 2.0, 3.0]);
        assert!(
            doc.rules
                .iter()
                .all(|(k, _)| k != "kind" && k != "position")
        );
    }

    #[test]
    fn object_defaults_come_from_the_kind() {
        let text = "[object]\nkind = runway\n";
        let doc = MapDoc::parse("x", text);
        assert_eq!(doc.objects[0].kind, ObjectKind::Runway);
        assert_eq!(doc.objects[0].scale, ObjectKind::Runway.default_scale());
        assert_eq!(doc.objects[0].color, ObjectKind::Runway.default_color());
    }

    #[test]
    fn kind_round_trips() {
        for kind in ObjectKind::ALL {
            assert_eq!(ObjectKind::parse(kind.id()), Some(kind));
        }
    }

    #[test]
    fn round_trips_embedded_planes_with_models() {
        let mut doc = MapDoc::new("m");
        doc.planes.push(EmbeddedPlane {
            id: "f4u-4-corsair".into(),
            config: "name = F4U-4 Corsair\nmass = 6000\n[gun 0]\nname = M2\nammo = 100\n".into(),
            model_name: Some("f4u.glb".into()),
            model: Some(vec![0, 1, 2, 3, 254, 255, 128]),
        });
        let text = doc.to_string();
        let parsed = MapDoc::parse("m", &text);
        assert_eq!(
            parsed.planes, doc.planes,
            "plane round-trip failed:\n{text}"
        );
        assert_eq!(parsed.planes[0].display_name(), "F4U-4 Corsair");
    }

    #[test]
    fn plane_file_round_trips() {
        let plane = EmbeddedPlane {
            id: "test".into(),
            config: "name = Test\nmass = 1000\n[gun 0]\nname = G\nammo = 5\n".into(),
            model_name: Some("t.glb".into()),
            model: Some(vec![9, 8, 7]),
        };
        let text = plane.to_file_string();
        let parsed = EmbeddedPlane::parse_file(&text);
        assert_eq!(parsed, plane, "plane file round-trip failed:\n{text}");
    }

    #[test]
    fn plane_file_without_a_model_works() {
        let plane = EmbeddedPlane::parse_file("name = Bare\nmass = 900\n");
        assert_eq!(plane.id, "bare");
        assert!(plane.model.is_none());
        assert!(plane.config.contains("mass = 900"));
    }

    #[test]
    fn round_trips_a_sculpted_heightmap() {
        let mut doc = MapDoc::new("terrain");
        doc.terrain.make_heightmap(33, 0.0);
        if let Some(map) = doc.terrain.heightmap.as_mut() {
            map.sculpt(0.0, 0.0, 500.0, 300.0);
        }
        doc.terrain.water = true;
        doc.terrain.water_level = 20.0;
        let text = doc.to_string();
        let parsed = MapDoc::parse("terrain", &text);
        assert_eq!(parsed.terrain, doc.terrain);
        assert!(
            parsed.terrain.height_at(0.0, 0.0) > 200.0,
            "the mountain should survive the round trip"
        );
    }
}
