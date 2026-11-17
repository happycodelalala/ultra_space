//! Arcade flight (debug, No Man's Sky style): no gravity and no drifting.
//!
//! A vehicle with [`ArcadeFlight`] moves kinematically: its velocity is the "standing still"
//! velocity of flight assist's reference (the ground below the surface reference altitude, the
//! body's inertial frame above it) plus the nose direction times a held speed, plus strafing.
//! The forward stick changes the held speed; the rotation stick turns the vehicle directly.
//! Normal Newtonian flight skips vehicles in arcade mode; the cruise drive still takes over
//! while cruising.

use bevy_ecs::prelude::*;
use glam::{DQuat, DVec3};
use sim_core::ObjectState;
use sim_core::objects::{RegionBody, SimClock, SystemRef};

use crate::cruise::Cruise;
use crate::flight::{
    BodyEnv, FlightControl, FlightTelemetry, OnRails, RegionTerrain, ground_altitude,
    horizon_weight, level_roll_rate, region_env,
};
use crate::vehicle::{Vehicle, VehicleBody};

/// How fast the held speed changes at full forward or back stick (m/s²).
pub const SPEED_RATE: f64 = 150.0;
/// Fastest held speed (m/s): enough to climb out to orbit.
pub const MAX_SPEED: f64 = 4_000.0;
/// Strafe and vertical speed at full stick, as a fraction of the held speed (with a floor).
const STRAFE_FRACTION: f64 = 0.3;
const MIN_STRAFE_SPEED: f64 = 40.0;
/// Turn rate at full stick (rad/s).
pub const TURN_RATE: f64 = 1.2;
/// Lowest height of the origin above the ground (m) for a vehicle without a known hull.
const MIN_ALTITUDE: f64 = 8.0;
/// Gap kept between the hull's lowest point and the ground (m).
const HULL_CLEARANCE: f64 = 1.0;

/// How far the hull reaches below the origin along `up` (m), for a vehicle turned by
/// `rotation`: the lowest point of its collision shapes (rest pose).
pub fn hull_depth(hull: &[crate::vehicle::PartCollider], rotation: DQuat, up: DVec3) -> f64 {
    use crate::catalog::Shape;
    hull.iter()
        .map(|c| {
            let centre = -(rotation * c.position).dot(up);
            // The shape's own axes: `up` seen from the part.
            let u = (rotation * c.rotation).inverse() * up;
            let extent = match c.shape {
                Shape::Box { size_m } => {
                    u.x.abs() * size_m[0] / 2.0
                        + u.y.abs() * size_m[1] / 2.0
                        + u.z.abs() * size_m[2] / 2.0
                }
                Shape::Cylinder {
                    radius_m, length_m, ..
                } => u.y.abs() * length_m / 2.0 + radius_m * (1.0 - u.y * u.y).max(0.0).sqrt(),
                Shape::Sphere { radius_m } => radius_m,
                // Shells reach here as their tiles (vehicle colliders); otherwise their bounds:
                // the box's lowest point along `u` (part axes).
                Shape::Shell { .. } => {
                    let (mid, half) = c.shape.bounds();
                    u.x.abs() * half.x + u.y.abs() * half.y + u.z.abs() * half.z - u.dot(mid)
                }
            };
            centre + extent
        })
        .fold(f64::MIN, f64::max)
}

/// Arcade flight state: the speed held along the nose (m/s, relative to the reference).
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct ArcadeFlight {
    pub speed: f64,
}

fn forward(rotation: DQuat) -> DVec3 {
    rotation * DVec3::NEG_Z
}

/// Advances one vehicle in arcade mode by `dt` (state in the body's inertial frame).
pub fn arcade_step(
    state: &mut sim_core::frames::FrameState,
    arcade: &mut ArcadeFlight,
    control: &FlightControl,
    env: &BodyEnv,
    hull: &[crate::vehicle::PartCollider],
    dt: f64,
) -> (crate::flight::Reference, DVec3) {
    let stick = control
        .translate
        .clamp(DVec3::splat(-1.0), DVec3::splat(1.0));
    arcade.speed = (arcade.speed - stick.z * SPEED_RATE * dt).clamp(0.0, MAX_SPEED);
    if control.brake {
        arcade.speed = 0.0;
    }
    let mut spin = control.rotate.clamp(DVec3::splat(-1.0), DVec3::splat(1.0)) * TURN_RATE;
    // Level the wings with the horizon near the ground while the pilot isn't rolling.
    if control.rotate.z.abs() < 0.05 {
        let altitude = state.position.length() - env.radius;
        spin.z +=
            horizon_weight(env, altitude) * level_roll_rate(state.rotation, env.up(state.position));
    }
    state.rotation = (state.rotation * DQuat::from_scaled_axis(spin * dt)).normalize();
    state.angular_velocity = state.rotation * spin;

    let strafe = (arcade.speed * STRAFE_FRACTION).max(MIN_STRAFE_SPEED);
    let relative = forward(state.rotation) * arcade.speed
        + state.rotation * DVec3::new(stick.x, stick.y, 0.0) * strafe;
    let (reference, _) = env.standing_still(state.position);
    if reference == crate::flight::Reference::Surface {
        // Ride with the ground exactly: turn position and heading with the planet's spin (a
        // straight step along the spin velocity would drift outward), then add own motion.
        let turn = DQuat::from_scaled_axis(env.spin * dt);
        state.position = turn * state.position;
        state.rotation = (turn * state.rotation).normalize();
    }
    let radius_before = state.position.length();
    state.position += relative * dt;
    let mut relative = relative;

    // Stay above the ground, the hull's lowest point clear of it (a nose-down hull reaches far
    // below its origin); the velocity then follows the ground, not the commanded dive.
    let clearance = if hull.is_empty() {
        MIN_ALTITUDE
    } else {
        hull_depth(hull, state.rotation, env.up(state.position)) + HULL_CLEARANCE
    };
    if env.solid && state.position.length() < env.radius + env.max_ground_height() + clearance {
        let floor = env.radius + env.ground_height(state.position) + clearance;
        if state.position.length() < floor {
            state.position = state.position.normalize() * floor;
            let up = env.up(state.position);
            let climb = (floor - radius_before) / dt;
            relative = relative - up * relative.dot(up) + up * climb;
        }
    }
    state.velocity = env.standing_still(state.position).1 + relative;
    (reference, relative)
}

/// Moves every vehicle in arcade mode for one tick (runs after Newtonian flight, which skips
/// them, and before the cruise drive).
#[allow(clippy::type_complexity)]
pub fn arcade_system(
    mut commands: Commands,
    region: Res<RegionBody>,
    system: Res<SystemRef>,
    clock: Res<SimClock>,
    terrain: Option<Res<RegionTerrain>>,
    mut vehicles: Query<
        (
            Entity,
            &mut ObjectState,
            &FlightControl,
            &mut FlightTelemetry,
            &mut ArcadeFlight,
            Option<&Cruise>,
            Option<&OnRails>,
            &VehicleBody,
        ),
        With<Vehicle>,
    >,
) {
    let system = &system.0;
    let t = clock.time.as_secs_f64();
    let env = region_env(system, region.0, t, terrain.as_deref());
    let frame = system.bodies[region.0].inertial_frame;
    for (entity, mut state, control, mut telemetry, mut arcade, cruise, rails, body) in
        &mut vehicles
    {
        if cruise.is_some_and(Cruise::is_cruising) {
            continue;
        }
        if rails.is_some() {
            commands.entity(entity).remove::<OnRails>();
        }
        let mut local = state.in_frame(&system.frames, frame, t);
        let (reference, relative) = arcade_step(
            &mut local.state,
            &mut arcade,
            control,
            &env,
            &body.colliders,
            clock.dt,
        );
        *state = local;
        telemetry.reference = reference;
        telemetry.relative_velocity = relative;
        telemetry.altitude = local.state.position.length() - env.radius;
        telemetry.ground_altitude = ground_altitude(&env, local.state.position);
        telemetry.on_rails = false;
        telemetry.in_contact = false;
        telemetry.throttles.iter_mut().for_each(|u| *u = 0.0);
    }
}

#[cfg(test)]
mod tests {
    use sim_core::frames::FrameState;

    use super::*;
    use crate::flight::{AssistMode, Reference};

    fn meridian() -> BodyEnv {
        let radius = 600_000.0;
        BodyEnv {
            mu: 9.81 * radius * radius,
            radius,
            surface_reference_altitude: 70_000.0,
            spin: DVec3::Y * (std::f64::consts::TAU / 5_400.0),
            surface_rotation: DQuat::IDENTITY,
            solid: true,
            terrain: None,
            air_density: 0.0,
            air_scale_height: 8_000.0,
        }
    }

    fn level_at(altitude: f64) -> FrameState {
        // At +X, nose along −Z (horizontal), up along +X.
        FrameState {
            position: DVec3::X * (600_000.0 + altitude),
            rotation: DQuat::from_rotation_arc(DVec3::Y, DVec3::X),
            velocity: DVec3::ZERO,
            angular_velocity: DVec3::ZERO,
        }
    }

    #[test]
    fn hovers_without_gravity_and_rides_with_the_ground() {
        let env = meridian();
        let mut s = level_at(1_000.0);
        let mut arcade = ArcadeFlight::default();
        let control = FlightControl::default();
        for _ in 0..300 {
            let (reference, relative) =
                arcade_step(&mut s, &mut arcade, &control, &env, &[], 1.0 / 30.0);
            assert_eq!(reference, Reference::Surface);
            assert_eq!(relative, DVec3::ZERO);
        }
        let altitude = s.position.length() - env.radius;
        assert!((altitude - 1_000.0).abs() < 0.5, "no falling: {altitude}");
        assert!((s.velocity - env.spin.cross(s.position)).length() < 1e-6);
    }

    #[test]
    fn forward_stick_builds_speed_along_the_nose() {
        let env = meridian();
        let mut s = level_at(1_000.0);
        let mut arcade = ArcadeFlight::default();
        let control = FlightControl {
            translate: DVec3::new(0.0, 0.0, -1.0),
            assist: AssistMode::Full,
            ..Default::default()
        };
        for _ in 0..30 {
            arcade_step(&mut s, &mut arcade, &control, &env, &[], 1.0 / 30.0);
        }
        assert!(
            (arcade.speed - SPEED_RATE).abs() < 1e-6,
            "1 s of full stick"
        );
        let relative = s.velocity - env.spin.cross(s.position);
        assert!(relative.normalize().dot(s.rotation * DVec3::NEG_Z) > 0.999);
        // Brake stops it.
        let brake = FlightControl {
            brake: true,
            ..Default::default()
        };
        arcade_step(&mut s, &mut arcade, &brake, &env, &[], 1.0 / 30.0);
        assert_eq!(arcade.speed, 0.0);
    }

    #[test]
    fn cannot_go_below_the_ground() {
        let env = meridian();
        let mut s = level_at(1.0);
        // Nose straight down.
        s.rotation = DQuat::from_rotation_arc(DVec3::NEG_Z, DVec3::NEG_X);
        let mut arcade = ArcadeFlight { speed: 500.0 };
        arcade_step(
            &mut s,
            &mut arcade,
            &FlightControl::default(),
            &env,
            &[],
            0.1,
        );
        assert!(s.position.length() >= env.radius + MIN_ALTITUDE - 1e-6);
    }

    #[test]
    fn a_long_hull_diving_nose_down_stays_above_the_ground() {
        use crate::catalog::Shape;
        use crate::vehicle::PartCollider;
        let env = meridian();
        // A 20 m hull along Z, its nose 10 m ahead of the origin.
        let hull = [PartCollider {
            shape: Shape::Box {
                size_m: [4.0, 3.0, 20.0],
            },
            position: DVec3::ZERO,
            rotation: DQuat::IDENTITY,
            exact: false,
            group: 0,
        }];
        let mut s = level_at(30.0);
        // Nose straight down (toward the planet's centre).
        s.rotation = DQuat::from_rotation_arc(DVec3::NEG_Z, -s.position.normalize());
        let mut arcade = ArcadeFlight { speed: 300.0 };
        for _ in 0..30 {
            arcade_step(
                &mut s,
                &mut arcade,
                &FlightControl::default(),
                &env,
                &hull,
                0.1,
            );
        }
        let up = s.position.normalize();
        let lowest = s.position.length() - hull_depth(&hull, s.rotation, up) - env.radius;
        assert!(
            (0.9..1.5).contains(&lowest),
            "the nose stays just above the ground: {lowest:.2} m"
        );
        assert!((hull_depth(&hull, s.rotation, up) - 10.0).abs() < 1e-6);
    }
}
