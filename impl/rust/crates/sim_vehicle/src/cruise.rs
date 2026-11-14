//! Placeholder cruise drive (design: objects-vehicles-physics.md §5; task 1.13).
//!
//! - **Kinematic:** while cruising, the vehicle ignores gravity and thrusters and moves along its
//!   nose at a speed proportional to its distance from the nearest body surface
//!   (`speed = CRUISE_RATE × distance`, capped by the drive). Heading straight at a body, the
//!   distance shrinks exponentially, so the vehicle can never reach or overshoot it.
//! - **Engage** (after a short charge) only away from every body: above its drop altitude
//!   (outside atmospheres) by a margin. **Drop out** on request, or automatically below a body's
//!   drop altitude, into a circular-orbit velocity around the nearest body, then normal flight
//!   resumes.
//! - Steering: the rotation stick turns the vehicle (and so the cruise heading) directly.
//! - Region transfers work as in normal flight: the state stays in the region body's inertial
//!   frame and the transfer check runs after this system.

use bevy_ecs::prelude::*;
use glam::{DQuat, DVec3};
use sim_core::objects::{RegionBody, SimClock, SystemRef};
use sim_core::{ObjectState, StarSystem};

use crate::flight::{FlightControl, FlightTelemetry, OnRails};
use crate::vehicle::{Vehicle, VehicleBody};

/// Cruise speed per metre of distance to the nearest surface (1/s). The inverse is the
/// time constant of the approach: the distance falls by e every 1/CRUISE_RATE seconds.
pub const CRUISE_RATE: f64 = 0.25;
/// Time to blend from the entry speed to the cruise speed at the rated mass (s).
pub const RAMP_TIME: f64 = 2.0;
/// A drive refuses to engage beyond this load (ship mass over rated mass).
pub const MAX_LOAD: f64 = 4.0;
/// Loads below this count as this (a big drive on a small ship isn't faster without end).
const MIN_LOAD: f64 = 0.25;

/// What a ship's cruise drives do for it (task 3.14): its drives' figures scaled by the load,
/// the ship's mass over the drives' combined rated mass. Charge and ramp time grow and the turn
/// rate falls with the square root of the load; the top speed falls in proportion to any load
/// above 1; above [`MAX_LOAD`] the drive won't engage.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CruiseCharacteristics {
    /// Ship mass over the drives' combined rated mass.
    pub load: f64,
    pub max_speed_ms: f64,
    pub charge_time_s: f64,
    pub ramp_time_s: f64,
    /// Turn rate at full stick (rad/s).
    pub turn_rate: f64,
}

impl CruiseCharacteristics {
    /// For a ship of `mass` (kg) with `drives`; `None` without one. Several drives add their
    /// rated masses; the biggest sets the speed, charge and turn figures.
    pub fn of(drives: &[crate::catalog::CruiseDriveDef], mass: f64) -> Option<Self> {
        let main = drives
            .iter()
            .max_by(|a, b| a.rated_mass_kg.total_cmp(&b.rated_mass_kg))?;
        let rated: f64 = drives.iter().map(|d| d.rated_mass_kg).sum();
        let load = mass / rated.max(1.0);
        let k = load.max(MIN_LOAD).sqrt();
        Some(Self {
            load,
            max_speed_ms: main.max_speed_ms / load.max(1.0),
            charge_time_s: main.charge_time_s * k,
            ramp_time_s: RAMP_TIME * k,
            turn_rate: main.turn_rate_deg_s.to_radians() / k,
        })
    }

    pub fn overloaded(&self) -> bool {
        self.load > MAX_LOAD
    }
}
/// Engaging needs this multiple of every body's drop altitude.
pub const ENGAGE_MARGIN: f64 = 1.2;
/// Longest kinematic substep (s): `CRUISE_RATE × step` stays small.
const MAX_STEP: f64 = 0.05 / CRUISE_RATE;
const MAX_STEPS: u32 = 64;
/// With the substep cap hit (high time warp), a step never covers more than this fraction of
/// the distance to the nearest surface.
const MAX_STEP_FRACTION: f64 = 0.05;

/// Altitude below which cruise drops out near a body: above the atmosphere (or a twentieth of
/// the radius for airless bodies) plus 10 km.
pub fn drop_altitude(system: &StarSystem, body: usize) -> f64 {
    let b = &system.bodies[body];
    let air = b.atmosphere.as_ref().map_or(0.0, |a| a.height);
    air.max(0.05 * b.radius) + 10_000.0
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum CruisePhase {
    #[default]
    Off,
    /// Charging; cruise starts when `remaining` reaches zero.
    Charging { remaining: f64 },
    Cruising {
        /// Time since cruise started (s).
        elapsed: f64,
        /// Speed when cruise started (m/s), blended into the cruise speed.
        entry_speed: f64,
        /// Current speed (m/s).
        speed: f64,
    },
}

/// Why the last engage request was refused.
#[derive(Clone, Debug, PartialEq)]
pub enum CruiseBlocked {
    NoDrive,
    /// The ship is too heavy for its drives (the load).
    Overloaded(f64),
    /// Too close to this body (index).
    TooClose(usize),
}

/// Cruise drive state and requests for one vehicle.
#[derive(Component, Clone, Debug, Default, PartialEq)]
pub struct Cruise {
    /// Set by input to engage (when off) or cancel / drop out (when charging or cruising);
    /// cleared by [`cruise_system`].
    pub toggle: bool,
    pub phase: CruisePhase,
    pub blocked: Option<CruiseBlocked>,
}

impl Cruise {
    /// Cruising: normal flight is suspended.
    pub fn is_cruising(&self) -> bool {
        matches!(self.phase, CruisePhase::Cruising { .. })
    }
}

/// Bodies as seen from one region at one instant: center (region body's inertial frame),
/// velocity, radius, drop altitude and `mu`.
struct Surroundings {
    centers: Vec<DVec3>,
    velocities: Vec<DVec3>,
}

impl Surroundings {
    fn at(system: &StarSystem, region: usize, t: f64) -> Self {
        let frame = system.bodies[region].inertial_frame;
        let (centers, velocities) = system
            .bodies
            .iter()
            .map(|b| {
                let s = system.frames.state_in(frame, b.inertial_frame, t);
                (s.position, s.velocity)
            })
            .unzip();
        Self {
            centers,
            velocities,
        }
    }

    /// Nearest body by distance to its surface: (index, distance to surface).
    fn nearest(&self, system: &StarSystem, p: DVec3) -> (usize, f64) {
        self.centers
            .iter()
            .enumerate()
            .map(|(i, c)| (i, (p - *c).length() - system.bodies[i].radius))
            .fold((0, f64::INFINITY), |a, b| if b.1 < a.1 { b } else { a })
    }

    /// First body whose altitude is below `factor × drop altitude`, if any.
    fn too_close(&self, system: &StarSystem, p: DVec3, factor: f64) -> Option<usize> {
        self.centers.iter().enumerate().find_map(|(i, c)| {
            let altitude = (p - *c).length() - system.bodies[i].radius;
            (altitude < factor * drop_altitude(system, i)).then_some(i)
        })
    }
}

/// Velocity of a circular orbit around a body (relative to it) at offset `r`, as close to
/// `heading` as possible: the heading projected onto the local horizontal, or prograde (about
/// +Y) when the heading is vertical.
pub fn orbit_velocity(mu: f64, r: DVec3, heading: DVec3) -> DVec3 {
    let up = r.normalize();
    let mut along = heading - up * heading.dot(up);
    if along.length_squared() < 0.01 {
        along = DVec3::Y.cross(up);
        if along.length_squared() < 0.01 {
            along = DVec3::X.cross(up);
        }
    }
    along.normalize() * (mu / r.length()).sqrt()
}

fn forward(rotation: DQuat) -> DVec3 {
    rotation * DVec3::NEG_Z
}

/// Runs the cruise drive for every vehicle in a region, **after** flight: flight skips vehicles
/// that are cruising, and those are moved here. A vehicle that finishes charging starts moving
/// on the next tick, and one that drops out is flown normally from the next tick, so no vehicle
/// is advanced twice in one tick.
#[allow(clippy::type_complexity)]
pub fn cruise_system(
    mut commands: Commands,
    region: Res<RegionBody>,
    system: Res<SystemRef>,
    clock: Res<SimClock>,
    mut vehicles: Query<
        (
            Entity,
            &mut ObjectState,
            &VehicleBody,
            &mut FlightControl,
            &mut FlightTelemetry,
            &mut Cruise,
        ),
        With<Vehicle>,
    >,
) {
    let system = &system.0;
    let t = clock.time.as_secs_f64();
    let frame = system.bodies[region.0].inertial_frame;
    let mut now: Option<Surroundings> = None;
    for (entity, mut state, body, mut control, mut telemetry, mut cruise) in &mut vehicles {
        if !cruise.toggle && cruise.phase == CruisePhase::Off {
            continue;
        }
        let toggle = std::mem::take(&mut cruise.toggle);
        let here = now.get_or_insert_with(|| Surroundings::at(system, region.0, t));
        let mut local = state.in_frame(&system.frames, frame, t);
        let p = local.state.position;

        match cruise.phase {
            CruisePhase::Off => {
                cruise.blocked = match body.performance.cruise {
                    None => Some(CruiseBlocked::NoDrive),
                    Some(c) if c.overloaded() => Some(CruiseBlocked::Overloaded(c.load)),
                    Some(_) => here
                        .too_close(system, p, ENGAGE_MARGIN)
                        .map(CruiseBlocked::TooClose),
                };
                if let (None, Some(c)) = (&cruise.blocked, body.performance.cruise) {
                    cruise.phase = CruisePhase::Charging {
                        remaining: c.charge_time_s,
                    };
                }
            }
            CruisePhase::Charging { remaining } => {
                cruise.phase = if toggle {
                    CruisePhase::Off
                } else if remaining - clock.dt > 0.0 {
                    CruisePhase::Charging {
                        remaining: remaining - clock.dt,
                    }
                } else if let Some(i) = here.too_close(system, p, ENGAGE_MARGIN) {
                    // Drifted too close while charging.
                    cruise.blocked = Some(CruiseBlocked::TooClose(i));
                    CruisePhase::Off
                } else {
                    commands.entity(entity).remove::<OnRails>();
                    let speed = local.state.velocity.length();
                    CruisePhase::Cruising {
                        elapsed: 0.0,
                        entry_speed: speed,
                        speed,
                    }
                };
            }
            CruisePhase::Cruising {
                elapsed,
                entry_speed,
                ..
            } => {
                let drive = body.performance.cruise;
                let max_speed = drive.map_or(0.0, |c| c.max_speed_ms);
                let ramp_time = drive.map_or(RAMP_TIME, |c| c.ramp_time_s);
                let turn_rate = drive.map_or(0.0, |c| c.turn_rate);
                let steps = ((clock.dt / MAX_STEP).ceil() as u32).clamp(1, MAX_STEPS);
                let h = clock.dt / f64::from(steps);
                let spin = control.rotate.clamp(DVec3::splat(-1.0), DVec3::splat(1.0)) * turn_rate;
                let mut s = local.state;
                let mut elapsed = elapsed;
                let mut speed = 0.0;
                let mut drop = toggle;
                for k in 0..steps {
                    if drop {
                        break;
                    }
                    let tk = t + h * f64::from(k);
                    let around = if k == 0 {
                        None
                    } else {
                        Some(Surroundings::at(system, region.0, tk))
                    };
                    let around = around.as_ref().unwrap_or(here);
                    let (_, distance) = around.nearest(system, s.position);
                    let target = (CRUISE_RATE * distance).min(max_speed);
                    let ramp = smoothstep(elapsed / ramp_time);
                    speed = entry_speed + (target - entry_speed) * ramp;
                    speed = speed.min(MAX_STEP_FRACTION * distance / h).max(0.0);
                    s.rotation = (s.rotation * DQuat::from_scaled_axis(spin * h)).normalize();
                    s.position += forward(s.rotation) * speed * h;
                    elapsed += h;
                    drop = around.too_close(system, s.position, 1.0).is_some();
                }
                s.velocity = forward(s.rotation) * speed;
                s.angular_velocity = s.rotation * spin;
                cruise.phase = CruisePhase::Cruising {
                    elapsed,
                    entry_speed,
                    speed,
                };

                if drop {
                    let after = Surroundings::at(system, region.0, t + clock.dt);
                    let (i, _) = after.nearest(system, s.position);
                    let r = s.position - after.centers[i];
                    s.velocity = after.velocities[i]
                        + orbit_velocity(system.bodies[i].mu, r, forward(s.rotation));
                    s.angular_velocity = DVec3::ZERO;
                    cruise.phase = CruisePhase::Off;
                    // Assist starts fresh from the new velocity.
                    control.desired_velocity = None;
                    commands.entity(entity).remove::<OnRails>();
                }
                local.state = s;
                let (i, distance) = here.nearest(system, s.position);
                telemetry.altitude = distance;
                telemetry.relative_velocity = s.velocity - here.velocities[i];
                telemetry.throttles.iter_mut().for_each(|u| *u = 0.0);
                telemetry.on_rails = false;
                telemetry.in_contact = false;
                *state = local;
            }
        }
    }
}

fn smoothstep(x: f64) -> f64 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}
