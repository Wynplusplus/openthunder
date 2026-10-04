//! Aircraft metadata shared by the game and the TUI launcher.
//!
//! The game's `AircraftRegistry` builds full [`crate`] specs keyed by these
//! `id`s; the launcher only needs the names so the player can pick one.

/// Basic information about a flyable aircraft type.
pub struct PlaneInfo {
    /// Stable id, matching `AircraftSpec::name` in the game.
    pub id: &'static str,
    /// Label shown in the launcher.
    pub label: &'static str,
    /// Nation / operator.
    pub nation: &'static str,
    /// One-line description.
    pub description: &'static str,
}

/// Every aircraft the game knows about. Keep in sync with the game registry
/// (a unit test in `aircraft.rs` checks this).
pub const PLANES: &[PlaneInfo] = &[
    PlaneInfo {
        id: "F4U-4 Corsair",
        label: "F4U-4 Corsair",
        nation: "USA",
        description: "Naval fighter. Fast, heavy, superb energy retention; rolls hard.",
    },
    PlaneInfo {
        id: "Bf 109 G-6",
        label: "Bf 109 G-6",
        nation: "Germany",
        description: "Light interceptor. Great climb and zoom; controls stiffen at speed.",
    },
    PlaneInfo {
        id: "Spitfire F Mk IXc",
        label: "Spitfire F Mk IXc",
        nation: "Great Britain",
        description: "Elliptical-wing turn-fighter. Excellent climb and low-speed agility.",
    },
];

/// The aircraft selected when nothing else is configured.
pub fn default_plane() -> &'static str {
    PLANES[0].id
}

/// Look up an aircraft by its id.
pub fn find(id: &str) -> Option<&'static PlaneInfo> {
    PLANES.iter().find(|plane| plane.id == id)
}
