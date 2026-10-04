//! Combat: projectile guns and hit detection.
//!
//! Guns fire ballistic tracers (gravity + drag) that are swept against aircraft
//! every frame. A hit is resolved to a section (wing / engine / tail / fuselage)
//! and damages that section's [`DamageModel`], which the flight model already
//! reads to degrade lift, thrust and control.
//!
//! Fire with the left mouse button or the `fire` key (default `Space`).

use bevy::prelude::*;
use openthunder::keybinds::FIRE;
use openthunder::protocol::ClientMessage;

use crate::aircraft::{Aircraft, PlayerControlled};
use crate::damage::{AircraftPart, DamageModel};
use crate::flight::Bindings;
use crate::menu::GameMenu;
use crate::net::{NetClient, RemotePlayer};

/// Gravity applied to projectiles, m/s^2.
const GRAVITY: f32 = 9.81;
/// Projectile drag (per metre); slows rounds a little over distance.
const DRAG: f32 = 0.00018;
/// Projectile lifetime, seconds.
const MAX_LIFE: f32 = 6.0;
/// Aircraft hit box half-extents (m) in the aircraft's local frame.
const HIT_HALF_EXTENTS: Vec3 = Vec3::new(6.4, 0.9, 4.2);

/// A flying round.
#[derive(Component)]
pub struct Projectile {
    pub velocity: Vec3,
    pub damage: f32,
    /// Shooter, so you cannot shoot yourself.
    pub owner: Entity,
    pub life: f32,
}

/// Shared tracer mesh/material (created once, not per shot).
#[derive(Resource)]
struct CombatAssets {
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
}

/// Tiny deterministic RNG for weapon spread.
#[derive(Resource)]
struct CombatRng(u64);

impl CombatRng {
    /// A value in `[-0.5, 0.5)`.
    fn next(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        ((x >> 40) as f32) / ((1u64 << 24) as f32) - 0.5
    }
}

/// Transient "you were hit" / "you hit an enemy" flashes, shown on the HUD.
#[derive(Resource, Default)]
pub struct CombatFeedback {
    pub player_hit: f32,
    pub enemy_hit: f32,
}

pub struct CombatPlugin;

impl Plugin for CombatPlugin {
    fn build(&self, app: &mut App) {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9E37_79B9_7F4A_7C15)
            | 1;
        app.insert_resource(CombatRng(seed))
            .init_resource::<CombatFeedback>()
            .add_systems(Startup, setup_combat_assets)
            .add_systems(
                Update,
                (fire_guns, update_projectiles, decay_feedback).chain(),
            );
    }
}

fn setup_combat_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.insert_resource(CombatAssets {
        // A long, bright tracer so you can clearly see where the rounds fly.
        mesh: meshes.add(Cuboid::new(0.4, 0.4, 6.0)),
        material: materials.add(StandardMaterial {
            base_color: Color::srgb(1.0, 0.9, 0.45),
            unlit: true,
            ..default()
        }),
    });
}

fn fire_guns(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    bindings: Res<Bindings>,
    menu: Res<GameMenu>,
    assets: Res<CombatAssets>,
    mut rng: ResMut<CombatRng>,
    mut commands: Commands,
    mut query: Query<(Entity, &Transform, &mut Aircraft), With<PlayerControlled>>,
) {
    let Ok((entity, transform, mut aircraft)) = query.single_mut() else {
        return;
    };
    let dt = time.delta_secs();

    // Cool down every gun.
    for timer in &mut aircraft.fire_timer {
        *timer = (*timer - dt).max(0.0);
    }

    let firing =
        !menu.open && (mouse.pressed(MouseButton::Left) || bindings.pressed(&keys, &mouse, FIRE));
    if !firing {
        return;
    }

    // Clone the gun list so we can mutate the aircraft's ammo/timers below.
    let guns = aircraft.spec.guns.clone();
    let rotation = transform.rotation;
    let forward = rotation * Vec3::NEG_Z;
    let right = rotation * Vec3::X;
    let up = rotation * Vec3::Y;
    let base_velocity = aircraft.velocity;

    for (index, gun) in guns.iter().enumerate() {
        if aircraft.fire_timer[index] > 0.0 || aircraft.ammo[index] == 0 {
            continue;
        }
        for muzzle in &gun.muzzles {
            if aircraft.ammo[index] == 0 {
                break;
            }
            aircraft.ammo[index] -= 1;

            let origin = transform.translation + rotation * *muzzle;
            let jitter = right * rng.next() * gun.spread + up * rng.next() * gun.spread;
            let direction = (forward + jitter).normalize_or_zero();
            let velocity = base_velocity + direction * gun.muzzle_velocity;

            commands.spawn((
                Mesh3d(assets.mesh.clone()),
                MeshMaterial3d(assets.material.clone()),
                Transform::from_translation(origin).looking_to(direction, up),
                Projectile {
                    velocity,
                    damage: gun.damage,
                    owner: entity,
                    life: MAX_LIFE,
                },
            ));
        }
        aircraft.fire_timer[index] = 1.0 / gun.rounds_per_second.max(0.01);
    }
}

fn update_projectiles(
    time: Res<Time>,
    mut commands: Commands,
    client: Res<NetClient>,
    mut feedback: ResMut<CombatFeedback>,
    mut projectiles: Query<(Entity, &mut Transform, &mut Projectile), Without<DamageModel>>,
    mut targets: Query<
        (
            Entity,
            &Transform,
            &mut DamageModel,
            Option<&PlayerControlled>,
            Option<&RemotePlayer>,
            Option<&crate::targets::TestTarget>,
            Option<&crate::targets::HitBox>,
        ),
        Without<Projectile>,
    >,
) {
    let dt = time.delta_secs();

    for (entity, mut transform, mut projectile) in &mut projectiles {
        let p0 = transform.translation;

        // Ballistics: gravity + quadratic drag.
        let mut velocity = projectile.velocity;
        velocity += Vec3::NEG_Y * GRAVITY * dt;
        velocity *= 1.0 - DRAG * velocity.length() * dt;
        projectile.velocity = velocity;
        let p1 = p0 + velocity * dt;
        transform.translation = p1;
        if velocity.length_squared() > 1.0 {
            transform.rotation = Transform::from_translation(p1)
                .looking_to(velocity, Vec3::Y)
                .rotation;
        }

        projectile.life -= dt;
        let life = projectile.life;
        let damage_amount = projectile.damage;
        let owner = projectile.owner;
        if life <= 0.0 || p1.y < 0.0 {
            commands.entity(entity).despawn();
            continue;
        }

        // Swept hit test against every aircraft and target (skip the shooter).
        for (target, target_transform, mut damage, player, remote, test_target, hit_box) in
            &mut targets
        {
            if target == owner {
                continue;
            }
            let half = hit_box.map_or(HIT_HALF_EXTENTS, |hit_box| hit_box.0);
            let Some(t) = segment_hits_box(target_transform, p0, p1, half) else {
                continue;
            };
            let point = p0.lerp(p1, t);
            if test_target.is_some() {
                // Ground targets have a single structure pool.
                damage.apply_damage(AircraftPart::Fuselage, damage_amount);
            } else {
                let section = classify_hit(target_transform, point);
                // Tell the server about hits on other players so everyone agrees
                // (the server relays it to the others; we apply it locally now).
                if let Some(remote) = remote {
                    if let Some(outgoing) = &client.outgoing {
                        let _ = outgoing.send(ClientMessage::Hit {
                            target: remote.id,
                            section: section.index() as u8,
                            damage: damage_amount,
                        });
                    }
                }
                damage.apply_damage(section, damage_amount);
            }
            if player.is_some() {
                feedback.player_hit = 0.6;
            } else {
                feedback.enemy_hit = 0.4;
            }
            commands.entity(entity).despawn();
            break;
        }
    }
}

fn decay_feedback(time: Res<Time>, mut feedback: ResMut<CombatFeedback>) {
    let dt = time.delta_secs();
    feedback.player_hit = (feedback.player_hit - dt).max(0.0);
    feedback.enemy_hit = (feedback.enemy_hit - dt).max(0.0);
}

/// Sweeps the segment `p0 -> p1` against the target's oriented box. Returns the
/// entry parameter `t` in `[0, 1]`, if it hits.
fn segment_hits_box(target: &Transform, p0: Vec3, p1: Vec3, half: Vec3) -> Option<f32> {
    let inverse = target.rotation.inverse();
    let a = inverse * (p0 - target.translation);
    let b = inverse * (p1 - target.translation);
    let d = b - a;

    let mut t_min = 0.0f32;
    let mut t_max = 1.0f32;
    for axis in 0..3 {
        let origin = a[axis];
        let delta = d[axis];
        if delta.abs() < 1e-6 {
            if origin < -half[axis] || origin > half[axis] {
                return None;
            }
        } else {
            let inverse_delta = 1.0 / delta;
            let mut t1 = (-half[axis] - origin) * inverse_delta;
            let mut t2 = (half[axis] - origin) * inverse_delta;
            if t1 > t2 {
                std::mem::swap(&mut t1, &mut t2);
            }
            t_min = t_min.max(t1);
            t_max = t_max.min(t2);
            if t_min > t_max {
                return None;
            }
        }
    }
    Some(t_min)
}

/// Which section a hit at `point` (world space) struck.
fn classify_hit(target: &Transform, point: Vec3) -> AircraftPart {
    let local = target.rotation.inverse() * (point - target.translation);
    if local.z > 2.8 {
        AircraftPart::Tail
    } else if local.z < -3.0 {
        AircraftPart::Engine
    } else if local.x < -1.6 {
        AircraftPart::LeftWing
    } else if local.x > 1.6 {
        AircraftPart::RightWing
    } else {
        AircraftPart::Fuselage
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segment_hits_a_box_ahead() {
        let target = Transform::from_translation(Vec3::new(0.0, 0.0, -100.0));
        let hit = segment_hits_box(
            &target,
            Vec3::ZERO,
            Vec3::new(0.0, 0.0, -200.0),
            HIT_HALF_EXTENTS,
        );
        assert!(hit.is_some(), "should hit the box");
    }

    #[test]
    fn segment_misses_a_box_to_the_side() {
        let target = Transform::from_translation(Vec3::new(100.0, 0.0, -100.0));
        let hit = segment_hits_box(
            &target,
            Vec3::ZERO,
            Vec3::new(0.0, 0.0, -200.0),
            HIT_HALF_EXTENTS,
        );
        assert!(hit.is_none(), "should miss the box");
    }

    #[test]
    fn hit_sections_are_classified() {
        let target = Transform::IDENTITY;
        assert_eq!(
            classify_hit(&target, Vec3::new(0.0, 0.0, 3.5)),
            AircraftPart::Tail
        );
        assert_eq!(
            classify_hit(&target, Vec3::new(0.0, 0.0, -3.6)),
            AircraftPart::Engine
        );
        assert_eq!(
            classify_hit(&target, Vec3::new(-3.0, 0.0, 0.0)),
            AircraftPart::LeftWing
        );
        assert_eq!(
            classify_hit(&target, Vec3::new(3.0, 0.0, 0.0)),
            AircraftPart::RightWing
        );
        assert_eq!(
            classify_hit(&target, Vec3::new(0.0, 0.0, 0.0)),
            AircraftPart::Fuselage
        );
    }

    #[test]
    fn rng_stays_in_range_and_varies() {
        let mut rng = CombatRng(12345);
        let mut saw_negative = false;
        let mut saw_positive = false;
        for _ in 0..1000 {
            let value = rng.next();
            assert!((-0.5..0.5).contains(&value), "out of range: {value}");
            saw_negative |= value < 0.0;
            saw_positive |= value > 0.0;
        }
        assert!(saw_negative && saw_positive);
    }

    /// A projectile aimed at a test target should damage it through the target's
    /// own (small) hit box, not the aircraft-sized default.
    #[test]
    fn projectiles_damage_test_targets() {
        use crate::net::NetClient;
        use crate::targets::{HitBox, TestTarget};

        let mut app = App::new();
        app.insert_resource(Time::<()>::default())
            .init_resource::<CombatFeedback>()
            .insert_resource(NetClient::default())
            .add_systems(Update, update_projectiles);

        let target = app
            .world_mut()
            .spawn((
                Transform::from_translation(Vec3::new(0.0, 50.0, -100.0)),
                DamageModel::new(10.0),
                TestTarget {
                    kind: crate::targets::TargetKind::Tank,
                    alive: true,
                    respawn_in: 0.0,
                },
                HitBox(Vec3::new(1.0, 1.0, 1.0)),
            ))
            .id();
        let shooter = app.world_mut().spawn(()).id();
        app.world_mut().spawn((
            Transform::from_translation(Vec3::new(0.0, 50.0, -95.0)),
            Projectile {
                velocity: Vec3::new(0.0, 0.0, -800.0),
                damage: 5.0,
                owner: shooter,
                life: 1.0,
            },
        ));

        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs_f32(0.05));
        app.update();

        let damage = app.world().get::<DamageModel>(target).unwrap();
        assert!(
            damage.integrity(AircraftPart::Fuselage) < 1.0,
            "the target should take damage"
        );
    }
}
