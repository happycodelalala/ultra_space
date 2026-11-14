//! Walking on foot (Batch 2, task 2.9; design: planet-surface.md §5).
//!
//! A walker is a kinematic character: an object whose position is its feet. It rides with the
//! turning ground (like arcade flight), falls under the body's real gravity, keeps its up
//! direction along local gravity (so "down" changes smoothly around the planet), and walks on
//! the terrain: grounded feet follow the ground up and down hills, and a jump leaves it.
//! Swimming and slope limits come later; for now the sea surface is a floor.

use bevy_ecs::prelude::*;
use glam::{DQuat, DVec2, DVec3};
use sim_core::ObjectState;
use sim_core::frames::FrameState;
use sim_core::objects::{RegionBody, SimClock, SystemRef};

use crate::flight::{BodyEnv, RegionTerrain, ground_altitude, region_env};
use crate::physics::{CHARACTER_HEIGHT, CHARACTER_RADIUS, RegionPhysics};

/// Walking and running speeds (m/s).
pub const WALK_SPEED: f64 = 5.0;
pub const RUN_SPEED: f64 = 12.0;
/// Upward speed of a jump (m/s): ~0.8 m high on Meridian, much higher on small moons.
pub const JUMP_SPEED: f64 = 4.0;
/// How fast walking speed changes on the ground (m/s²).
const GROUND_ACCELERATION: f64 = 40.0;
/// Steering in the air, as a fraction of ground acceleration.
const AIR_CONTROL: f64 = 0.15;
/// Grounded feet follow the ground down steps up to this high (m) instead of falling off.
const STEP_DOWN: f64 = 0.6;
/// Most a walker is pushed out of an overlap with a vehicle per step (m).
const MAX_PUSH: f64 = 0.2;
/// Eye height above the feet (m).
pub const EYE_HEIGHT: f64 = 1.7;

/// Walker state. Velocity is relative to the turning ground, in inertial axes.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct Walker {
    pub velocity: DVec3,
    pub grounded: bool,
    /// Height of the feet above the ground (m).
    pub ground_altitude: f64,
}

/// Input for a walker.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct WalkerControl {
    /// Movement stick: x right, y forward, each in [−1, 1].
    pub movement: DVec2,
    pub run: bool,
    /// Jump request (consumed when grounded).
    pub jump: bool,
    /// Turn about the local up axis this tick (radians, positive = left).
    pub turn: f64,
}

/// Rotation with forward (−Z) along `forward` and +Y along `up`.
fn upright(forward: DVec3, up: DVec3) -> DQuat {
    let f = (forward - up * forward.dot(up)).normalize_or(up.any_orthonormal_vector());
    let right = f.cross(up);
    DQuat::from_mat3(&glam::DMat3::from_cols(right, up, -f)).normalize()
}

/// Moves a walker's feet from a position along `up` by a desired displacement against the
/// world's colliders: returns the push out of any overlap, the displacement made, and whether
/// it stands on something ([`crate::PhysicsScene::sweep_character`]).
pub type Mover<'a> = &'a mut dyn FnMut(DVec3, DVec3, DVec3) -> (DVec3, DVec3, bool);

/// Advances a walker by `dt` (state in the body's inertial frame; `position` is the feet).
///
/// It walks on the terrain height. With a `mover` it also moves against the region's vehicles
/// (as they were at the start of the tick: blocked by hulls, standing on decks) and does *not*
/// turn with the planet here: all of a tick's steps happen in the frame of the tick's start, and
/// the caller turns it once for the whole tick ([`ride`]), so it never moves relative to the
/// vehicles it is swept against.
/// Turns a walker with the planet for `dt` (after its steps against the vehicles).
pub fn ride(state: &mut FrameState, walker: &mut Walker, env: &BodyEnv, dt: f64) {
    let spin = DQuat::from_scaled_axis(env.spin * dt);
    state.position = spin * state.position;
    state.rotation = (spin * state.rotation).normalize();
    walker.velocity = spin * walker.velocity;
    state.velocity = env.spin.cross(state.position) + walker.velocity;
    state.angular_velocity = env.spin;
}

pub fn walk_step(
    state: &mut FrameState,
    walker: &mut Walker,
    control: &mut WalkerControl,
    env: &BodyEnv,
    dt: f64,
    mover: Option<Mover<'_>>,
) {
    let spin = DQuat::from_scaled_axis(env.spin * dt);
    if mover.is_none() {
        // Ride with the turning ground.
        state.position = spin * state.position;
        state.rotation = (spin * state.rotation).normalize();
    }

    // Stand upright along local gravity, then turn.
    let up = state.position.normalize();
    let forward = state.rotation * DVec3::NEG_Z;
    let turn = DQuat::from_axis_angle(up, control.turn);
    state.rotation = upright(turn * forward, up);
    control.turn = 0.0;

    // Wanted velocity along the ground.
    let forward = state.rotation * DVec3::NEG_Z;
    let right = forward.cross(up);
    let stick = control.movement.clamp_length_max(1.0);
    let speed = if control.run { RUN_SPEED } else { WALK_SPEED };
    let wanted = (forward * stick.y + right * stick.x) * speed;

    let tangential = walker.velocity - up * walker.velocity.dot(up);
    let rate = if walker.grounded {
        GROUND_ACCELERATION
    } else {
        GROUND_ACCELERATION * AIR_CONTROL
    };
    let change = (wanted - tangential).clamp_length_max(rate * dt);
    walker.velocity += change;

    if walker.grounded && control.jump {
        walker.velocity += up * JUMP_SPEED;
        walker.grounded = false;
    }
    control.jump = false;

    // Gravity, then move.
    let r = state.position.length();
    walker.velocity -= up * (env.mu / (r * r)) * dt;
    if let Some(mover) = mover {
        // Vehicles: blocked by hulls, standing on decks.
        let desired = walker.velocity * dt;
        let (out, moved, on_vehicle) = mover(state.position, up, desired);
        // Rapier's controller can return more movement than asked (snapping and sliding over
        // the edges of a hull): never more than asked plus a little, and a push out of an
        // overlap of at most MAX_PUSH per step, or a bump can throw the walker.
        let moved = moved.clamp_length_max(desired.length() + 0.02);
        let out = out.clamp_length_max(MAX_PUSH);
        // The push out of an overlap moves the walker but isn't its own speed.
        state.position += out + moved;
        let vertical = walker.velocity.dot(up);
        // Sliding along a hull redirects the walker's speed but never adds to it.
        let asked = (desired - up * desired.dot(up)).length() / dt;
        let tangential = ((moved - up * moved.dot(up)) / dt).clamp_length_max(asked);
        walker.velocity = tangential + up * vertical;
        // The ground: the terrain height, as without vehicles.
        let ground = env.radius + env.ground_height(state.position);
        let feet = state.position.length() - ground;
        let rising = walker.velocity.dot(up) > 0.0;
        let on_ground = feet <= 0.0 || (walker.grounded && !rising && feet < STEP_DOWN);
        if on_ground && !on_vehicle {
            state.position = state.position.normalize() * ground;
        }
        if on_ground || on_vehicle {
            let vertical = walker.velocity.dot(up);
            if vertical < 0.0 || walker.grounded {
                walker.velocity -= up * vertical;
            }
        }
        walker.grounded = on_ground || on_vehicle;
        walker.ground_altitude = (state.position.length() - ground).max(0.0);
        return;
    }
    state.position += walker.velocity * dt;

    // Ground: land, stay on it, or follow it down small steps.
    let ground = env.radius + env.ground_height(state.position);
    let feet = state.position.length() - ground;
    let rising = walker.velocity.dot(up) > 0.0;
    if feet <= 0.0 || (walker.grounded && !rising && feet < STEP_DOWN) {
        state.position = state.position.normalize() * ground;
        let up = state.position.normalize();
        let vertical = walker.velocity.dot(up);
        if vertical < 0.0 || walker.grounded {
            walker.velocity -= up * vertical;
        }
        walker.grounded = true;
    } else {
        walker.grounded = false;
    }
    walker.ground_altitude = (state.position.length() - ground).max(0.0);
    state.velocity = env.spin.cross(state.position) + walker.velocity;
    state.angular_velocity = env.spin;
}

/// Walls (task 4.5): pushes the walker's body (a cylinder of the character's radius and
/// height, feet at its position) out of any building it entered, along the nearer side, and
/// takes away its speed into the wall, so it slides along. Buildings come with the terrain's
/// foundations.
pub fn keep_out_of_buildings(state: &mut FrameState, walker: &mut Walker, env: &BodyEnv) {
    let Some(f) = env.terrain.as_ref().and_then(|t| t.foundations()) else {
        return;
    };
    let p = env.surface_rotation.inverse() * state.position;
    let dir = p.normalize();
    let mut push = DVec3::ZERO;
    for (_, b) in f.structures_near(dir, 2.0) {
        let local = b.rotation.inverse() * (p + push - b.center);
        // Feet above the roof, or the head below the floor: clear.
        if local.y > b.half.y || local.y + CHARACTER_HEIGHT < -b.half.y {
            continue;
        }
        let dx = b.half.x + CHARACTER_RADIUS - local.x.abs();
        let dz = b.half.z + CHARACTER_RADIUS - local.z.abs();
        if dx <= 0.0 || dz <= 0.0 {
            continue;
        }
        let out = if dx < dz {
            DVec3::X * dx * if local.x < 0.0 { -1.0 } else { 1.0 }
        } else {
            DVec3::Z * dz * if local.z < 0.0 { -1.0 } else { 1.0 }
        };
        push += b.rotation * out;
    }
    if push == DVec3::ZERO {
        return;
    }
    let push = env.surface_rotation * push;
    state.position += push;
    let n = push.normalize();
    let into = walker.velocity.dot(n);
    if into < 0.0 {
        walker.velocity -= n * into;
    }
}

/// Moves every walker in a region for one tick.
pub fn walker_system(
    region: Res<RegionBody>,
    system: Res<SystemRef>,
    clock: Res<SimClock>,
    terrain: Option<Res<RegionTerrain>>,
    physics: Option<Res<RegionPhysics>>,
    mut walkers: Query<(&mut ObjectState, &mut Walker, &mut WalkerControl)>,
) {
    let system = &system.0;
    let t = clock.time.as_secs_f64();
    let env = region_env(system, region.0, t, terrain.as_deref());
    let frame = system.bodies[region.0].inertial_frame;
    for (mut state, mut walker, mut control) in &mut walkers {
        let mut local = state.in_frame(&system.frames, frame, t);
        // Steps of at most 1/60 s so jumps and slopes stay smooth under light time warp.
        let steps = (clock.dt / (1.0 / 60.0)).ceil().clamp(1.0, 64.0) as u32;
        let h = clock.dt / f64::from(steps);
        // Against the region's vehicles when there is a physics scene.
        let scene = physics.as_deref().map(|p| &p.0);
        for _ in 0..steps {
            match &scene {
                Some(scene) => {
                    let mut sweep = |feet, up, desired| scene.sweep_character(feet, up, desired, h);
                    walk_step(
                        &mut local.state,
                        &mut walker,
                        &mut control,
                        &env,
                        h,
                        Some(&mut sweep),
                    );
                }
                None => walk_step(&mut local.state, &mut walker, &mut control, &env, h, None),
            }
            keep_out_of_buildings(&mut local.state, &mut walker, &env);
        }
        if scene.is_some() {
            // Turned with the planet once for the whole tick.
            ride(&mut local.state, &mut walker, &env, clock.dt);
        }
        walker.ground_altitude = ground_altitude(&env, local.state.position).max(0.0);
        *state = local;
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use sim_planet::Terrain;

    use super::*;

    fn env_of(id: &str) -> BodyEnv {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../content/data/star_systems/halcyon.json");
        let sys = sim_core::StarSystem::from_path(path).unwrap();
        let i = sys.bodies.iter().position(|b| b.id == id).unwrap();
        let terrain = Terrain::of(&sys.bodies[i]).map(Arc::new);
        region_env(&sys, i, 0.0, Some(&RegionTerrain(terrain)))
    }

    /// A walker standing on dry land at some spot.
    fn standing(env: &BodyEnv) -> (FrameState, Walker) {
        let up = sim_planet::fibonacci_directions(5_000)
            .map(|d| env.surface_rotation * d)
            .find(|&d| env.ground_height(d * env.radius) > 50.0)
            .unwrap();
        let p = up * (env.radius + env.ground_height(up * env.radius));
        let state = FrameState {
            position: p,
            rotation: upright(up.any_orthonormal_vector(), up),
            velocity: env.spin.cross(p),
            angular_velocity: env.spin,
        };
        (
            state,
            Walker {
                grounded: true,
                ..Default::default()
            },
        )
    }

    fn feet_above_ground(env: &BodyEnv, s: &FrameState) -> f64 {
        s.position.length() - (env.radius + env.ground_height(s.position))
    }

    #[test]
    fn stands_still_on_the_turning_ground() {
        let env = env_of("meridian");
        let (mut s, mut w) = standing(&env);
        let mut c = WalkerControl::default();
        for _ in 0..600 {
            walk_step(&mut s, &mut w, &mut c, &env, 1.0 / 60.0, None);
        }
        assert!(w.grounded);
        assert!(feet_above_ground(&env, &s).abs() < 0.01);
        assert!(w.velocity.length() < 1e-6);
    }

    #[test]
    fn walks_over_hills_keeping_its_feet_on_the_ground() {
        let env = env_of("meridian");
        let (mut s, mut w) = standing(&env);
        let start = s.position;
        let mut c = WalkerControl::default();
        for _ in 0..(60 * 20) {
            c.movement = DVec2::new(0.0, 1.0);
            c.run = true;
            walk_step(&mut s, &mut w, &mut c, &env, 1.0 / 60.0, None);
            assert!(
                feet_above_ground(&env, &s) > -0.01,
                "never below the ground"
            );
        }
        // 20 s of running at 12 m/s, minus the planet's turning (compare along the ground).
        let turned = DQuat::from_scaled_axis(env.spin * 20.0) * start;
        let travelled = (s.position.normalize() - turned.normalize()).length() * env.radius;
        assert!((200.0..260.0).contains(&travelled), "ran {travelled:.0} m");
        // Up stays along local gravity.
        let up = s.rotation * DVec3::Y;
        assert!(up.dot(s.position.normalize()) > 0.9999);
    }

    #[test]
    fn jumps_higher_in_low_gravity() {
        let jump_height = |id: &str| {
            let env = env_of(id);
            let (mut s, mut w) = standing(&env);
            let mut c = WalkerControl {
                jump: true,
                ..Default::default()
            };
            let mut highest: f64 = 0.0;
            for _ in 0..(60 * 30) {
                walk_step(&mut s, &mut w, &mut c, &env, 1.0 / 60.0, None);
                highest = highest.max(feet_above_ground(&env, &s));
                if w.grounded && highest > 0.1 {
                    break;
                }
            }
            assert!(w.grounded, "{id}: landed again");
            highest
        };
        let meridian = jump_height("meridian");
        let lantern = jump_height("lantern");
        assert!(
            (0.6..1.0).contains(&meridian),
            "Meridian jump {meridian:.2} m"
        );
        assert!(lantern > 3.0 * meridian, "Lantern jump {lantern:.2} m");
    }
}
