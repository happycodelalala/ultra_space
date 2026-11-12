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

/// Advances a walker by `dt` (state in the body's inertial frame; `position` is the feet).
pub fn walk_step(
    state: &mut FrameState,
    walker: &mut Walker,
    control: &mut WalkerControl,
    env: &BodyEnv,
    dt: f64,
) {
    // Ride with the turning ground.
    let spin = DQuat::from_scaled_axis(env.spin * dt);
    state.position = spin * state.position;
    state.rotation = (spin * state.rotation).normalize();

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

/// Moves every walker in a region for one tick.
pub fn walker_system(
    region: Res<RegionBody>,
    system: Res<SystemRef>,
    clock: Res<SimClock>,
    terrain: Option<Res<RegionTerrain>>,
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
        for _ in 0..steps {
            walk_step(&mut local.state, &mut walker, &mut control, &env, h);
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
            walk_step(&mut s, &mut w, &mut c, &env, 1.0 / 60.0);
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
            walk_step(&mut s, &mut w, &mut c, &env, 1.0 / 60.0);
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
                walk_step(&mut s, &mut w, &mut c, &env, 1.0 / 60.0);
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
