//! Damage model scaffolding.
//!
//! War Thunder Air RB tracks damage per structural section, and that damage has
//! real aerodynamic/mechanical consequences. This module implements the same
//! *idea* in the simplest useful form:
//!
//! * Each aircraft has a [`DamageModel`] with a fixed array of [`AircraftPart`]s.
//! * Every part has its own hit points.
//! * The flight model reads the integrity of each part and degrades the relevant
//!   behaviour (wing damage -> less lift, engine damage -> less thrust, tail
//!   damage -> sluggish controls / weak stability).
//!
//! There is no combat yet, so a few debug keys let you simulate hits and watch
//! the aircraft degrade. Adding real weapons later only means calling
//! [`DamageModel::apply_damage`] with the part the round/shell hit.

use bevy::prelude::*;

use openthunder::keybinds::{DAMAGE_ENGINE, DAMAGE_LEFT_WING, DAMAGE_TAIL, REPAIR};

use crate::aircraft::PlayerControlled;
use crate::flight::Bindings;

/// The damageable sections of an aircraft. Extend this enum (and [`AircraftPart::ALL`])
/// as more systems (fuel tanks, radiators, control cables, ...) are added.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AircraftPart {
    Fuselage,
    LeftWing,
    RightWing,
    Engine,
    Tail,
}

impl AircraftPart {
    /// Keep this in sync with the enum variants above.
    pub const ALL: [AircraftPart; 5] = [
        AircraftPart::Fuselage,
        AircraftPart::LeftWing,
        AircraftPart::RightWing,
        AircraftPart::Engine,
        AircraftPart::Tail,
    ];

    /// Index into [`DamageModel::parts`].
    fn index(self) -> usize {
        self as usize
    }

    pub fn label(self) -> &'static str {
        match self {
            AircraftPart::Fuselage => "Fuselage",
            AircraftPart::LeftWing => "Left wing",
            AircraftPart::RightWing => "Right wing",
            AircraftPart::Engine => "Engine",
            AircraftPart::Tail => "Tail",
        }
    }
}

/// Hit points for a single section.
#[derive(Clone, Copy, Debug)]
pub struct PartState {
    pub health: f32,
    pub max_health: f32,
}

impl PartState {
    fn new(max_health: f32) -> Self {
        Self {
            health: max_health,
            max_health,
        }
    }

    /// 0.0 = destroyed, 1.0 = pristine.
    pub fn integrity(&self) -> f32 {
        if self.max_health <= 0.0 {
            0.0
        } else {
            (self.health / self.max_health).clamp(0.0, 1.0)
        }
    }

    pub fn is_destroyed(&self) -> bool {
        self.health <= 0.0
    }
}

/// Per-section damage state for one aircraft.
#[derive(Component, Clone, Debug)]
pub struct DamageModel {
    pub parts: [PartState; AircraftPart::ALL.len()],
}

impl DamageModel {
    pub fn new(max_health: f32) -> Self {
        Self {
            parts: AircraftPart::ALL.map(|_| PartState::new(max_health)),
        }
    }

    pub fn part(&self, part: AircraftPart) -> &PartState {
        &self.parts[part.index()]
    }

    /// 0.0 = destroyed, 1.0 = pristine.
    pub fn integrity(&self, part: AircraftPart) -> f32 {
        self.part(part).integrity()
    }

    pub fn is_destroyed(&self, part: AircraftPart) -> bool {
        self.part(part).is_destroyed()
    }

    /// Apply `amount` damage (in hit points) to a section, clamped at 0.
    pub fn apply_damage(&mut self, part: AircraftPart, amount: f32) {
        let state = &mut self.parts[part.index()];
        state.health = (state.health - amount.max(0.0)).max(0.0);
    }

    pub fn repair_all(&mut self) {
        for state in &mut self.parts {
            state.health = state.max_health;
        }
    }

    /// Average integrity across all sections, for a quick overall health readout.
    pub fn overall_integrity(&self) -> f32 {
        let sum: f32 = self.parts.iter().map(PartState::integrity).sum();
        sum / self.parts.len() as f32
    }
}

pub struct DamagePlugin;

impl Plugin for DamagePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, debug_damage_input);
    }
}

/// Temporary "weapons": press 1/2/3 to chew up a section, 0 to repair.
/// Replace/augment this with real projectile collision later.
fn debug_damage_input(
    keys: Res<ButtonInput<KeyCode>>,
    bindings: Res<Bindings>,
    menu: Res<crate::menu::GameMenu>,
    mut query: Query<&mut DamageModel, With<PlayerControlled>>,
) {
    if menu.open {
        return;
    }
    let Ok(mut damage) = query.single_mut() else {
        return;
    };

    if bindings.just_pressed(&keys, DAMAGE_LEFT_WING) {
        damage.apply_damage(AircraftPart::LeftWing, 35.0);
    }
    if bindings.just_pressed(&keys, DAMAGE_ENGINE) {
        damage.apply_damage(AircraftPart::Engine, 35.0);
    }
    if bindings.just_pressed(&keys, DAMAGE_TAIL) {
        damage.apply_damage(AircraftPart::Tail, 35.0);
    }
    if bindings.just_pressed(&keys, REPAIR) {
        damage.repair_all();
    }
}
