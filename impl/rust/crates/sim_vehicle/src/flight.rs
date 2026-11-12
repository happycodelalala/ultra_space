//! Vehicle flight (design: objects-vehicles-physics.md §4; task 1.11).
//!
//! - Rigid-body dynamics at the center of mass with semi-implicit (symplectic) Euler and
//!   substeps, so orbits stay stable.
//! - Gravity from the region's body only (patched conics).
//! - Flight assist: holds a desired velocity relative to a reference frame (the body's rotating
//!   surface near it, its inertial frame higher up) and damps rotation, using the thrust
//!   allocator, so a vehicle with weak thrusters really is sluggish.
//! - Under time warp a coasting vehicle goes **on rails** ([`OnRails`]): its orbit is evaluated
//!   in closed form, like planets.
//! - Ground contact: the vehicle can't go below the body's surface (until terrain, Batch 2).

use bevy_ecs::prelude::*;
use glam::{DQuat, DVec3};
use sim_core::frames::FrameState;
use sim_core::objects::{RegionBody, RegionStage, SimClock, SystemRef};
use std::sync::Arc;

use sim_core::{KeplerOrbit, ObjectState};
use sim_planet::Terrain;

use crate::allocation::Wrench;
use crate::arcade::{ArcadeFlight, arcade_system};
use crate::cruise::{Cruise, cruise_system};
use crate::vehicle::{AXIS_DIRECTIONS, Vehicle, VehicleBody};

/// Longest physics substep (s).
pub const MAX_SUBSTEP: f64 = 1.0 / 60.0;
/// Most substeps per tick; beyond that a vehicle must be on rails (coasting under warp).
pub const MAX_SUBSTEPS: u32 = 64;
/// Thrust is ignored when a tick spans more than this much universe time (time warp above ~4×).
pub const MAX_THRUST_DT: f64 = 4.0 / 30.0 + 1e-9;
/// Assist time constants (s): how quickly velocity and spin errors are corrected.
const VELOCITY_RESPONSE: f64 = 0.8;
const ROTATION_RESPONSE: f64 = 0.25;
/// Rotation rate at full stick deflection (rad/s).
pub const MAX_TURN_RATE: f64 = 1.0;
/// Below this altitude the "standing still" reference is the rotating surface; airless bodies
/// use this fraction of their radius.
const AIRLESS_SURFACE_REFERENCE: f64 = 0.1;
/// Ground friction while in contact (fraction of sliding speed removed per second).
const GROUND_FRICTION: f64 = 2.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AssistMode {
    /// Holds the desired velocity (relative to the reference) and stops rotation.
    #[default]
    Full,
    /// Stops rotation; translation is raw thrust.
    Rotation,
    /// Raw thrust and torque.
    Off,
}

impl AssistMode {
    pub fn next(self) -> Self {
        match self {
            Self::Full => Self::Rotation,
            Self::Rotation => Self::Off,
            Self::Off => Self::Full,
        }
    }
}

/// What "standing still" means for flight assist.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Reference {
    /// The body's rotating surface (hover over the same spot).
    Surface,
    /// The body's non-rotating frame (coast along the orbit).
    #[default]
    Inertial,
}

/// Pilot input and flight-assist state.
#[derive(Component, Clone, Debug, Default, PartialEq)]
pub struct FlightControl {
    /// Translation stick in vehicle axes, each in [−1, 1] (+X right, +Y up, −Z forward).
    pub translate: DVec3,
    /// Rotation stick: pitch (about X), yaw (about Y), roll (about Z), each in [−1, 1].
    pub rotate: DVec3,
    pub assist: AssistMode,
    /// Request to bring the desired velocity to zero (relative to the reference).
    pub brake: bool,
    /// Desired velocity relative to `reference`, in that frame's axes.
    pub desired_velocity: Option<(Reference, DVec3)>,
    /// Region body that `desired_velocity` is relative to. When the vehicle moves to another
    /// body's region, [`flight_system`] converts the desired velocity so assist doesn't react.
    pub desired_body: Option<usize>,
}

/// Results of the last tick, for HUD and visuals.
#[derive(Component, Clone, Debug, Default, PartialEq)]
pub struct FlightTelemetry {
    pub throttles: Vec<f64>,
    pub reference: Reference,
    /// Velocity relative to `reference`, in the body's inertial axes.
    pub relative_velocity: DVec3,
    pub altitude: f64,
    /// Height above the ground below (m); equal to `altitude` far above the highest terrain.
    pub ground_altitude: f64,
    pub on_rails: bool,
    pub in_contact: bool,
}

/// Marks a coasting vehicle whose motion is evaluated from a Kepler orbit (relative to the
/// region's body). Added and removed by [`flight_system`].
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct OnRails(pub KeplerOrbit);

/// The body a vehicle is flying around, at one instant (all in the body's inertial frame).
#[derive(Clone, Debug)]
pub struct BodyEnv {
    pub mu: f64,
    pub radius: f64,
    /// Altitude below which the reference is the rotating surface.
    pub surface_reference_altitude: f64,
    /// Spin of the surface frame (inertial axes); zero for a non-rotating reference.
    pub spin: DVec3,
    /// Orientation of the surface frame in inertial axes.
    pub surface_rotation: DQuat,
    /// No ground (the star): skip contact.
    pub solid: bool,
    /// The body's terrain (heights in its surface frame); `None` means a smooth sphere at
    /// `radius`.
    pub terrain: Option<Arc<Terrain>>,
    /// Air density at sea level (kg/m³); 0 without an atmosphere.
    pub air_density: f64,
    /// Height over which air density falls by a factor e (m).
    pub air_scale_height: f64,
}

/// Air density at sea level for 1 atm (kg/m³).
pub const SEA_LEVEL_AIR_DENSITY: f64 = 1.225;
/// Air density scale height relative to the atmosphere's height (Earth: 8 km of an effective
/// 70 km; same rule as the renderer's scattering).
pub const AIR_SCALE: f64 = 8.0 / 70.0;
/// Detail of the ground under a vehicle (m): the finest terrain wavelength.
const GROUND_DETAIL: f64 = 1.0;
/// Coulomb friction coefficient between hulls and ground.
const GROUND_FRICTION_COEFFICIENT: f64 = 0.8;
/// A grounded vehicle slower than this (m/s) relative to the ground, on a slope gentler than
/// `MAX_STICK_SLOPE`, holds still (static friction).
const STICK_SPEED: f64 = 0.5;
const MAX_STICK_SLOPE: f64 = 0.7;
/// Drag coefficient times reference area, as a fraction of the vehicle's bounding disc.
const DRAG_AREA_FRACTION: f64 = 0.5;

/// Center-of-mass state in the body's inertial frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RigidState {
    pub position: DVec3,
    pub velocity: DVec3,
    pub rotation: DQuat,
    /// Angular velocity, inertial axes.
    pub angular_velocity: DVec3,
}

impl RigidState {
    /// From an object state (vehicle origin) and the vehicle's center of mass (vehicle axes).
    pub fn from_origin(s: &FrameState, com: DVec3) -> Self {
        let offset = s.rotation * com;
        Self {
            position: s.position + offset,
            velocity: s.velocity + s.angular_velocity.cross(offset),
            rotation: s.rotation,
            angular_velocity: s.angular_velocity,
        }
    }

    /// Back to the vehicle origin's state.
    pub fn to_origin(&self, com: DVec3) -> FrameState {
        let offset = self.rotation * com;
        FrameState {
            position: self.position - offset,
            rotation: self.rotation,
            velocity: self.velocity - self.angular_velocity.cross(offset),
            angular_velocity: self.angular_velocity,
        }
    }

    pub fn specific_energy(&self, mu: f64) -> f64 {
        self.velocity.length_squared() / 2.0 - mu / self.position.length()
    }
}

fn gravity(mu: f64, r: DVec3) -> DVec3 {
    let d = r.length();
    -r * (mu / (d * d * d))
}

impl BodyEnv {
    /// Height of the ground (m above `radius`) under `position` (inertial frame); the sea
    /// surface where there is water. 0 for a smooth sphere.
    pub fn ground_height(&self, position: DVec3) -> f64 {
        let Some(terrain) = &self.terrain else {
            return 0.0;
        };
        let dir = self.surface_rotation.inverse() * position.normalize();
        let h = terrain.height(dir, GROUND_DETAIL);
        if terrain.has_ocean() { h.max(0.0) } else { h }
    }

    /// Ground surface normal under `position` (inertial axes); straight up over water or on a
    /// smooth sphere.
    pub fn ground_normal(&self, position: DVec3) -> DVec3 {
        let up = position.normalize();
        let Some(terrain) = &self.terrain else {
            return up;
        };
        let dir = self.surface_rotation.inverse() * up;
        if terrain.has_ocean() && terrain.height(dir, GROUND_DETAIL) < 0.0 {
            return up;
        }
        self.surface_rotation * sim_planet::mesh::terrain_normal(terrain, dir, 2.0, GROUND_DETAIL)
    }

    /// Highest the ground can be (m above `radius`), to skip terrain sampling far above it.
    pub fn max_ground_height(&self) -> f64 {
        self.terrain.as_ref().map_or(0.0, |t| 1.3 * t.params.relief)
    }

    /// Air density (kg/m³) at `position`.
    pub fn air_density_at(&self, position: DVec3) -> f64 {
        if self.air_density <= 0.0 {
            return 0.0;
        }
        let altitude = (position.length() - self.radius).max(0.0);
        self.air_density * (-altitude / self.air_scale_height).exp()
    }

    /// Flight assist's reference at `position` (the body's inertial frame) and the velocity
    /// that counts as standing still there: co-rotating with the ground below the surface
    /// reference altitude, zero (in the body's inertial frame) above it.
    pub fn standing_still(&self, position: DVec3) -> (Reference, DVec3) {
        let reference = self.reference_at(position.length() - self.radius);
        (reference, self.frame_velocity(reference, position))
    }

    fn reference_at(&self, altitude: f64) -> Reference {
        if self.solid && altitude < self.surface_reference_altitude {
            Reference::Surface
        } else {
            Reference::Inertial
        }
    }

    /// Velocity of the reference frame at `r` (inertial axes).
    fn frame_velocity(&self, reference: Reference, r: DVec3) -> DVec3 {
        match reference {
            Reference::Surface => self.spin.cross(r),
            Reference::Inertial => DVec3::ZERO,
        }
    }

    /// Rotation from the reference frame's axes to inertial axes.
    fn frame_rotation(&self, reference: Reference) -> DQuat {
        match reference {
            Reference::Surface => self.surface_rotation,
            Reference::Inertial => DQuat::IDENTITY,
        }
    }
}

/// Works out the force and torque to request this tick (vehicle axes) and updates assist state.
pub fn assist_request(
    state: &RigidState,
    body: &VehicleBody,
    control: &mut FlightControl,
    env: &BodyEnv,
    dt: f64,
) -> (Wrench, Reference, DVec3) {
    let m = body.mass.mass;
    let altitude = state.position.length() - env.radius;
    let reference = env.reference_at(altitude);
    let relative = state.velocity - env.frame_velocity(reference, state.position);
    let to_vehicle = state.rotation.inverse();
    const SATURATE: f64 = 1.0e12;

    // Translation.
    let force = match control.assist {
        AssistMode::Full => {
            let frame_rot = env.frame_rotation(reference);
            // Keep the desired velocity continuous when the reference changes.
            let mut desired = match control.desired_velocity {
                Some((r, v)) if r == reference => frame_rot * v,
                _ => relative,
            };
            if control.brake {
                desired = DVec3::ZERO;
            }
            // The stick accelerates the desired velocity at the vehicle's real capability; the
            // same acceleration is fed forward so the vehicle keeps up with the stick.
            let mut feed_forward = DVec3::ZERO;
            if control.translate != DVec3::ZERO {
                let local = control
                    .translate
                    .clamp(DVec3::splat(-1.0), DVec3::splat(1.0));
                let accel_local = DVec3::new(
                    local.x * body.performance.max_acceleration[if local.x >= 0.0 { 0 } else { 1 }],
                    local.y * body.performance.max_acceleration[if local.y >= 0.0 { 2 } else { 3 }],
                    local.z * body.performance.max_acceleration[if local.z >= 0.0 { 4 } else { 5 }],
                );
                feed_forward = state.rotation * accel_local;
                desired += feed_forward * dt;
            }
            let g = gravity(env.mu, state.position);
            let frame_accel = match reference {
                // Coasting is "standing still" in the inertial frame: the desired velocity
                // falls with gravity, so no thrust is spent fighting it.
                Reference::Inertial => {
                    desired += g * dt;
                    DVec3::ZERO
                }
                // Holding still over the rotating ground: cancel gravity and supply the
                // centripetal and Coriolis accelerations of the rotating frame.
                Reference::Surface => {
                    let w = env.spin;
                    w.cross(relative) + w.cross(w.cross(state.position)) - g
                }
            };
            control.desired_velocity = Some((reference, frame_rot.inverse() * desired));
            let accel = feed_forward + (desired - relative) / VELOCITY_RESPONSE + frame_accel;
            to_vehicle * (accel * m)
        }
        AssistMode::Rotation | AssistMode::Off => {
            control.desired_velocity = None;
            control
                .translate
                .clamp(DVec3::splat(-1.0), DVec3::splat(1.0))
                * SATURATE
        }
    };

    // Rotation.
    let omega_body = to_vehicle * state.angular_velocity;
    let stick = control.rotate.clamp(DVec3::splat(-1.0), DVec3::splat(1.0));
    let torque = match control.assist {
        AssistMode::Full | AssistMode::Rotation => {
            let desired = stick * MAX_TURN_RATE;
            body.mass.inertia * ((desired - omega_body) / ROTATION_RESPONSE)
        }
        AssistMode::Off => stick * SATURATE,
    };
    (Wrench { force, torque }, reference, relative)
}

/// Advances `state` by `dt` under gravity and a constant body-axes wrench, with substeps.
pub fn integrate(
    state: &mut RigidState,
    body: &VehicleBody,
    wrench: Wrench,
    env: &BodyEnv,
    dt: f64,
) -> bool {
    let substeps = (dt / MAX_SUBSTEP)
        .ceil()
        .clamp(1.0, f64::from(MAX_SUBSTEPS)) as u32;
    let h = dt / f64::from(substeps);
    let m = body.mass.mass;
    let inertia = body.mass.inertia;
    let inv_inertia = inertia.inverse();
    let mut contact = false;
    let drag_area = DRAG_AREA_FRACTION * std::f64::consts::PI * vehicle_radius(body).powi(2);
    for _ in 0..substeps {
        // Linear: kick then drift (symplectic Euler).
        let thrust = state.rotation * wrench.force / m;
        // Drag against the air, which turns with the ground.
        let rho = env.air_density_at(state.position);
        let drag = if rho > 0.0 {
            let air = state.velocity - env.spin.cross(state.position);
            -air * (0.5 * rho * drag_area * air.length() / m)
        } else {
            DVec3::ZERO
        };
        state.velocity += (gravity(env.mu, state.position) + thrust + drag) * h;
        state.position += state.velocity * h;

        // Angular: Euler's equations in body axes.
        let to_body = state.rotation.inverse();
        let w = to_body * state.angular_velocity;
        let alpha = inv_inertia * (wrench.torque - w.cross(inertia * w));
        state.angular_velocity = state.rotation * (w + alpha * h);
        state.rotation =
            (DQuat::from_scaled_axis(state.angular_velocity * h) * state.rotation).normalize();

        contact |= ground_contact(state, env, body, h);
    }
    contact
}

/// Radius of a sphere around the center of mass that contains the vehicle (rough).
fn vehicle_radius(body: &VehicleBody) -> f64 {
    body.allocator
        .thrusters()
        .iter()
        .map(|t| t.position.length())
        .fold(2.0, f64::max)
}

/// Keeps the vehicle above the surface: removes inward motion and applies friction relative to
/// the rotating ground. Returns whether it touched.
fn ground_contact(state: &mut RigidState, env: &BodyEnv, body: &VehicleBody, h: f64) -> bool {
    if !env.solid {
        return false;
    }
    let clearance = vehicle_radius(body);
    let d = state.position.length();
    // Far above the highest possible ground: no need to sample the terrain.
    if d >= env.radius + env.max_ground_height() + clearance {
        return false;
    }
    let min = env.radius + env.ground_height(state.position) + clearance;
    if d >= min {
        return false;
    }
    let up = state.position / d;
    state.position = up * min;
    let normal = env.ground_normal(state.position);
    let ground = env.spin.cross(state.position);
    let mut relative = state.velocity - ground;
    // The ground stops motion into it; that normal impulse (it includes this substep's gravity)
    // sets how much sliding friction can remove (Coulomb friction).
    let into = relative.dot(normal);
    let normal_impulse = (-into).max(0.0);
    if into < 0.0 {
        relative -= normal * into;
    }
    let tangential = relative - normal * relative.dot(normal);
    let sliding = tangential.length();
    let slope = normal.dot(up).clamp(-1.0, 1.0).acos();
    let grip = GROUND_FRICTION_COEFFICIENT * normal_impulse;
    if slope < MAX_STICK_SLOPE && sliding <= grip.max(STICK_SPEED) {
        // Static friction: a slow vehicle on a gentle slope holds still.
        relative -= tangential;
    } else if sliding > 0.0 {
        // Sliding: Coulomb friction plus scraping drag that grows with speed.
        let slowed = (sliding - grip).max(0.0) * (1.0 - GROUND_FRICTION * h).max(0.0);
        relative -= tangential * (1.0 - slowed / sliding);
    }
    state.velocity = ground + relative;
    state.angular_velocity =
        env.spin + (state.angular_velocity - env.spin) * (1.0 - GROUND_FRICTION * h).max(0.0);
    true
}

/// One tick of flight for one vehicle. Returns the new origin state.
#[allow(clippy::too_many_arguments)]
pub fn fly(
    origin: &FrameState,
    body: &VehicleBody,
    control: &mut FlightControl,
    telemetry: &mut FlightTelemetry,
    rails: &mut Option<KeplerOrbit>,
    env: &BodyEnv,
    t_start: f64,
    dt: f64,
) -> FrameState {
    let com = body.mass.center_of_mass;
    let mut state = RigidState::from_origin(origin, com);
    let warping = dt > MAX_THRUST_DT;
    telemetry.altitude = state.position.length() - env.radius;
    telemetry.ground_altitude = ground_altitude(env, state.position);

    if warping {
        // Coast on rails when possible.
        if rails.is_none() {
            *rails = KeplerOrbit::from_state(env.mu, state.position, state.velocity, t_start)
                .ok()
                .filter(|o| !env.solid || o.periapsis() > env.radius);
        }
        if let Some(orbit) = rails {
            let (p, v) = orbit.state_at(t_start + dt);
            state.position = p;
            state.velocity = v;
            state.rotation =
                (DQuat::from_scaled_axis(state.angular_velocity * dt) * state.rotation).normalize();
            telemetry.on_rails = true;
            telemetry.throttles.iter_mut().for_each(|u| *u = 0.0);
            telemetry.in_contact = false;
            return state.to_origin(com);
        }
        // Not a safe orbit (escaping, or would hit the ground): integrate without thrust.
        telemetry.on_rails = false;
        telemetry.throttles.iter_mut().for_each(|u| *u = 0.0);
        telemetry.in_contact = integrate(&mut state, body, Wrench::default(), env, dt);
        return state.to_origin(com);
    }

    *rails = None;
    telemetry.on_rails = false;
    let (request, reference, relative) = assist_request(&state, body, control, env, dt);
    let warm = (telemetry.throttles.len() == body.allocator.thrusters().len())
        .then_some(telemetry.throttles.as_slice());
    let allocation = body.allocator.solve(request, warm);
    telemetry.reference = reference;
    telemetry.relative_velocity = relative;
    telemetry.in_contact = integrate(&mut state, body, allocation.achieved, env, dt);
    telemetry.throttles = allocation.throttles;
    state.to_origin(com)
}

/// Height above the ground under `position` (m); sea-level altitude when far above the highest
/// terrain (where sampling it would be wasted).
pub fn ground_altitude(env: &BodyEnv, position: DVec3) -> f64 {
    let altitude = position.length() - env.radius;
    if altitude > env.max_ground_height() + 20_000.0 {
        altitude
    } else {
        altitude - env.ground_height(position)
    }
}

/// Builds the body environment for region body `index` at universe time `t`.
pub fn body_env(system: &sim_core::StarSystem, index: usize, t: f64) -> BodyEnv {
    let body = &system.bodies[index];
    let surface = system
        .frames
        .state_in(body.inertial_frame, body.surface_frame, t);
    BodyEnv {
        mu: body.mu,
        radius: body.radius,
        surface_reference_altitude: body
            .atmosphere
            .as_ref()
            .map_or(body.radius * AIRLESS_SURFACE_REFERENCE, |a| a.height),
        spin: surface.angular_velocity,
        surface_rotation: surface.rotation,
        solid: body.parent.is_some(),
        terrain: None,
        air_density: body
            .atmosphere
            .as_ref()
            .map_or(0.0, |a| SEA_LEVEL_AIR_DENSITY * a.surface_pressure_atm),
        air_scale_height: body
            .atmosphere
            .as_ref()
            .map_or(1.0, |a| a.height * AIR_SCALE),
    }
}

/// A region's terrain, shared by the vehicles flying there (built once; see
/// [`install_terrain`]).
#[derive(Resource, Clone)]
pub struct RegionTerrain(pub Option<Arc<Terrain>>);

/// Gives every region of a star-system simulation its body's terrain, so vehicles land on the
/// real ground. Regions without it fly over a smooth sphere.
pub fn install_terrain(sim: &mut sim_core::Simulation) {
    let Some(system) = sim.star_system().cloned() else {
        return;
    };
    for (i, region) in sim.regions.iter_mut().enumerate() {
        let terrain = system.bodies.get(i).and_then(Terrain::of).map(Arc::new);
        region.world.insert_resource(RegionTerrain(terrain));
    }
}

/// The environment for region body `index`, with the region's terrain if installed.
pub fn region_env(
    system: &sim_core::StarSystem,
    index: usize,
    t: f64,
    terrain: Option<&RegionTerrain>,
) -> BodyEnv {
    let mut env = body_env(system, index, t);
    env.terrain = terrain.and_then(|r| r.0.clone());
    env
}

/// Flies every vehicle in a region for one tick. Add to each region's schedule in
/// [`RegionStage::Simulate`].
#[allow(clippy::type_complexity)]
pub fn flight_system(
    mut commands: Commands,
    region: Res<RegionBody>,
    system: Res<SystemRef>,
    clock: Res<SimClock>,
    terrain: Option<Res<RegionTerrain>>,
    mut vehicles: Query<
        (
            Entity,
            &mut ObjectState,
            &VehicleBody,
            &mut FlightControl,
            &mut FlightTelemetry,
            Option<&OnRails>,
            Option<&Cruise>,
        ),
        (With<Vehicle>, Without<ArcadeFlight>),
    >,
) {
    let system = &system.0;
    let t = clock.time.as_secs_f64();
    let env = region_env(system, region.0, t, terrain.as_deref());
    let inertial = system.bodies[region.0].inertial_frame;
    for (entity, mut state, body, mut control, mut telemetry, on_rails, cruise) in &mut vehicles {
        if cruise.is_some_and(Cruise::is_cruising) {
            continue; // moved by the cruise drive
        }
        if control.desired_body != Some(region.0) {
            rebase_desired_velocity(system, &mut control, region.0, t);
        }
        // Flight works in the body's inertial frame.
        let local = state.in_frame(&system.frames, inertial, t);
        let mut rails = on_rails.map(|r| r.0);
        let had_rails = rails.is_some();
        let next = fly(
            &local.state,
            body,
            &mut control,
            &mut telemetry,
            &mut rails,
            &env,
            t,
            clock.dt,
        );
        *state = ObjectState {
            frame: inertial,
            state: next,
        };
        match (had_rails, rails) {
            (false, Some(orbit)) => {
                commands.entity(entity).insert(OnRails(orbit));
            }
            (true, None) => {
                commands.entity(entity).remove::<OnRails>();
            }
            _ => {}
        }
    }
}

/// Re-expresses the held desired velocity relative to body `to` after a region transfer, so
/// flight assist keeps holding the same motion. Inertial frames share axes, so this only adds
/// the old frame's velocity relative to the new one. A surface reference never crosses a
/// sphere of influence (it ends at the atmosphere top or close to the ground), so it is dropped.
fn rebase_desired_velocity(
    system: &sim_core::StarSystem,
    control: &mut FlightControl,
    to: usize,
    t: f64,
) {
    if let (Some(from), Some((reference, v))) = (control.desired_body, control.desired_velocity) {
        control.desired_velocity = match reference {
            Reference::Inertial => {
                let shift = system
                    .frames
                    .state_in(
                        system.bodies[to].inertial_frame,
                        system.bodies[from].inertial_frame,
                        t,
                    )
                    .velocity;
                Some((Reference::Inertial, v + shift))
            }
            Reference::Surface => None,
        };
    }
    control.desired_body = Some(to);
}

/// Adds flight to a region schedule (used with `Simulation::for_star_system`).
pub fn add_flight_systems(schedule: &mut Schedule) {
    schedule.add_systems(
        (flight_system, arcade_system, cruise_system)
            .chain()
            .in_set(RegionStage::Simulate),
    );
}

/// Directions matching the performance summary, re-exported for HUDs.
pub const ACCELERATION_AXES: [DVec3; 6] = AXIS_DIRECTIONS;

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::catalog::PartCatalog;
    use crate::design::VehicleDesign;

    fn kestrel() -> VehicleBody {
        let content = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../../content/data");
        let catalog = PartCatalog::load_dir(content.join("parts")).unwrap();
        let design =
            VehicleDesign::from_path(content.join("vehicles/kestrel.json"), &catalog).unwrap();
        VehicleBody::of(&Vehicle::from_design(&design)).unwrap()
    }

    /// Meridian-like body: 600 km, 1g, 90-minute day, 70 km atmosphere.
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

    fn circular(env: &BodyEnv, altitude: f64) -> RigidState {
        let r = env.radius + altitude;
        RigidState {
            position: DVec3::new(r, 0.0, 0.0),
            velocity: DVec3::new(0.0, 0.0, -(env.mu / r).sqrt()),
            rotation: DQuat::IDENTITY,
            angular_velocity: DVec3::ZERO,
        }
    }

    #[test]
    fn coasting_orbit_conserves_energy() {
        let env = meridian();
        let body = kestrel();
        let mut s = circular(&env, 200_000.0);
        let e0 = s.specific_energy(env.mu);
        let period = KeplerOrbit::from_state(env.mu, s.position, s.velocity, 0.0)
            .unwrap()
            .period();
        let dt = 1.0 / 30.0;
        let steps = (2.0 * period / dt) as usize;
        let mut worst = 0.0f64;
        for _ in 0..steps {
            integrate(&mut s, &body, Wrench::default(), &env, dt);
            worst = worst.max(((s.specific_energy(env.mu) - e0) / e0).abs());
        }
        assert!(worst < 1e-5, "relative energy drift {worst}");
        assert!((s.position.length() - (env.radius + 200_000.0)).abs() < 50.0);
    }

    #[test]
    fn origin_and_center_of_mass_round_trip() {
        let body = kestrel();
        let origin = FrameState {
            position: DVec3::new(1.0, 2.0, 3.0),
            rotation: DQuat::from_rotation_y(0.7),
            velocity: DVec3::new(4.0, 5.0, 6.0),
            angular_velocity: DVec3::new(0.1, -0.2, 0.3),
        };
        let back = RigidState::from_origin(&origin, body.mass.center_of_mass)
            .to_origin(body.mass.center_of_mass);
        assert!((back.position - origin.position).length() < 1e-12);
        assert!((back.velocity - origin.velocity).length() < 1e-12);
    }

    #[test]
    fn full_assist_hovers_over_the_same_spot() {
        let env = meridian();
        let body = kestrel();
        // 2 km up, nose up (main engine pointing down), at rest relative to the ground.
        let r = DVec3::new(env.radius + 2_000.0, 0.0, 0.0);
        let nose_up = DQuat::from_rotation_arc(DVec3::NEG_Z, DVec3::X);
        let start = RigidState {
            position: r,
            velocity: env.spin.cross(r),
            rotation: nose_up,
            angular_velocity: env.spin,
        };
        let mut origin = start.to_origin(body.mass.center_of_mass);
        let mut control = FlightControl::default();
        let mut telemetry = FlightTelemetry::default();
        let mut rails = None;
        let dt = 1.0 / 30.0;
        for _ in 0..(30 * 20) {
            origin = fly(
                &origin,
                &body,
                &mut control,
                &mut telemetry,
                &mut rails,
                &env,
                0.0,
                dt,
            );
        }
        let s = RigidState::from_origin(&origin, body.mass.center_of_mass);
        assert_eq!(telemetry.reference, Reference::Surface);
        assert!(
            (s.position.length() - r.length()).abs() < 20.0,
            "altitude held: {}",
            s.position.length() - env.radius
        );
        assert!(
            telemetry.relative_velocity.length() < 0.5,
            "{}",
            telemetry.relative_velocity
        );
    }

    #[test]
    fn full_assist_accelerates_and_holds_speed() {
        let env = meridian();
        let body = kestrel();
        let mut origin = circular(&env, 300_000.0).to_origin(body.mass.center_of_mass);
        let mut control = FlightControl {
            translate: DVec3::new(0.0, 0.0, -1.0),
            ..Default::default()
        };
        let mut telemetry = FlightTelemetry::default();
        let mut rails = None;
        let dt = 1.0 / 30.0;
        let v0 = RigidState::from_origin(&origin, body.mass.center_of_mass).velocity;
        for _ in 0..30 {
            origin = fly(
                &origin,
                &body,
                &mut control,
                &mut telemetry,
                &mut rails,
                &env,
                0.0,
                dt,
            );
        }
        let v1 = RigidState::from_origin(&origin, body.mass.center_of_mass).velocity;
        let gained = (v1 - v0).dot(DVec3::NEG_Z);
        // One second at ~31 m/s² forward.
        assert!((25.0..32.0).contains(&gained), "gained {gained} m/s");
        // Release the stick: the new velocity is held (no drift back).
        control.translate = DVec3::ZERO;
        for _ in 0..60 {
            origin = fly(
                &origin,
                &body,
                &mut control,
                &mut telemetry,
                &mut rails,
                &env,
                0.0,
                dt,
            );
        }
        let desired = control.desired_velocity.unwrap().1;
        let v2 = RigidState::from_origin(&origin, body.mass.center_of_mass).velocity;
        assert!((v2 - desired).length() < 1.0, "{v2} vs {desired}");
    }

    #[test]
    fn rotation_assist_reaches_and_stops_turning() {
        let env = meridian();
        let body = kestrel();
        let mut origin = circular(&env, 300_000.0).to_origin(body.mass.center_of_mass);
        let mut control = FlightControl {
            rotate: DVec3::new(0.0, 1.0, 0.0),
            ..Default::default()
        };
        let mut telemetry = FlightTelemetry::default();
        let mut rails = None;
        for _ in 0..90 {
            origin = fly(
                &origin,
                &body,
                &mut control,
                &mut telemetry,
                &mut rails,
                &env,
                0.0,
                1.0 / 30.0,
            );
        }
        let yaw_rate = (origin.rotation.inverse() * origin.angular_velocity).y;
        assert!(yaw_rate > 0.3, "turning: {yaw_rate}");
        control.rotate = DVec3::ZERO;
        for _ in 0..150 {
            origin = fly(
                &origin,
                &body,
                &mut control,
                &mut telemetry,
                &mut rails,
                &env,
                0.0,
                1.0 / 30.0,
            );
        }
        assert!(
            origin.angular_velocity.length() < 0.01,
            "stopped: {}",
            origin.angular_velocity
        );
    }

    #[test]
    fn warp_goes_on_rails_and_matches_the_orbit() {
        let env = meridian();
        let body = kestrel();
        let start = circular(&env, 250_000.0);
        let origin0 = start.to_origin(body.mass.center_of_mass);
        let mut control = FlightControl {
            assist: AssistMode::Off,
            ..Default::default()
        };
        let mut telemetry = FlightTelemetry::default();
        let mut rails = None;
        // 100× warp: 3.33 s per tick, 300 ticks = 1,000 s.
        let dt = 100.0 / 30.0;
        let mut origin = origin0;
        for k in 0..300 {
            origin = fly(
                &origin,
                &body,
                &mut control,
                &mut telemetry,
                &mut rails,
                &env,
                k as f64 * dt,
                dt,
            );
        }
        assert!(telemetry.on_rails && rails.is_some());
        let expected = KeplerOrbit::from_state(env.mu, start.position, start.velocity, 0.0)
            .unwrap()
            .state_at(1_000.0)
            .0;
        let got = RigidState::from_origin(&origin, body.mass.center_of_mass).position;
        assert!(
            (got - expected).length() < 1e-3,
            "{}",
            (got - expected).length()
        );
        // Back to 1×: leaves rails and keeps flying from the same state.
        fly(
            &origin,
            &body,
            &mut control,
            &mut telemetry,
            &mut rails,
            &env,
            1_000.0,
            1.0 / 30.0,
        );
        assert!(!telemetry.on_rails && rails.is_none());
    }

    #[test]
    fn cannot_fall_through_the_ground() {
        let env = meridian();
        let body = kestrel();
        let mut s = RigidState {
            position: DVec3::new(env.radius + 500.0, 0.0, 0.0),
            velocity: DVec3::new(-200.0, 0.0, 0.0),
            rotation: DQuat::IDENTITY,
            angular_velocity: DVec3::ZERO,
        };
        let mut touched = false;
        for _ in 0..(30 * 10) {
            touched |= integrate(&mut s, &body, Wrench::default(), &env, 1.0 / 30.0);
        }
        assert!(touched);
        assert!(s.position.length() >= env.radius, "above ground");
        let ground = env.spin.cross(s.position);
        assert!(
            (s.velocity - ground).length() < 1.0,
            "resting on the rotating ground"
        );
    }

    #[test]
    fn standing_still_matches_the_ground_low_and_space_high() {
        let env = meridian();
        let low = DVec3::new(env.radius + 2_000.0, 0.0, 0.0);
        let (reference, v) = env.standing_still(low);
        assert_eq!(reference, Reference::Surface);
        assert!((v - env.spin.cross(low)).length() < 1e-9);
        let high = DVec3::new(env.radius + 200_000.0, 0.0, 0.0);
        assert_eq!(env.standing_still(high), (Reference::Inertial, DVec3::ZERO));
    }

    fn halcyon() -> sim_core::StarSystem {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../content/data/star_systems/halcyon.json");
        sim_core::StarSystem::from_path(path).unwrap()
    }

    /// Meridian's real environment at t = 0, with its terrain and air.
    fn meridian_terrain() -> BodyEnv {
        let sys = halcyon();
        let i = sys.bodies.iter().position(|b| b.id == "meridian").unwrap();
        let terrain = Terrain::of(&sys.bodies[i]).map(Arc::new);
        region_env(&sys, i, 0.0, Some(&RegionTerrain(terrain)))
    }

    /// A land spot (inertial direction) with a slope between `lo` and `hi` radians.
    fn sloped_spot(env: &BodyEnv, lo: f64, hi: f64) -> DVec3 {
        sim_planet::fibonacci_directions(20_000)
            .map(|d| env.surface_rotation * d)
            .find(|&d| {
                let p = d * env.radius;
                let slope = env.ground_normal(p).dot(d).acos();
                env.ground_height(p) > 100.0 && (lo..hi).contains(&slope)
            })
            .expect("a sloped land spot")
    }

    #[test]
    fn lands_on_a_slope_and_holds_still_with_the_ground() {
        let body = kestrel();
        let env = meridian_terrain();
        let up = sloped_spot(&env, 0.25, 0.45);
        let ground = env.radius + env.ground_height(up * env.radius);
        let p = up * (ground + 30.0);
        let mut s = RigidState {
            position: p,
            velocity: env.spin.cross(p),
            rotation: DQuat::from_rotation_arc(DVec3::Y, up),
            angular_velocity: env.spin,
        };
        let mut touched = false;
        let ticks = 600;
        for _ in 0..ticks {
            touched |= integrate(&mut s, &body, Wrench::default(), &env, 1.0 / 30.0);
        }
        assert!(touched, "reached the ground");
        // The landing spot has turned with the planet meanwhile.
        let up = DQuat::from_scaled_axis(env.spin * (f64::from(ticks) / 30.0)) * up;
        let height = s.position.length() - (env.radius + env.ground_height(s.position));
        let clearance = vehicle_radius(&body);
        assert!(
            (height - clearance).abs() < 0.5,
            "rests at its clearance: {height:.2} vs {clearance:.2}"
        );
        let rel_v = s.velocity - env.spin.cross(s.position);
        let relative = rel_v.length();
        assert!(
            relative < 0.1,
            "holds still on the slope: {relative:.3} m/s"
        );
        assert!(
            (s.position.normalize() - up).length() * env.radius < 5.0,
            "didn't slide away"
        );
    }

    #[test]
    fn air_drag_limits_falling_speed() {
        let body = kestrel();
        let env = meridian_terrain();
        let up = sloped_spot(&env, 0.0, 0.2);
        let p = up * (env.radius + 12_000.0);
        let mut s = RigidState {
            position: p,
            velocity: env.spin.cross(p),
            rotation: DQuat::IDENTITY,
            angular_velocity: DVec3::ZERO,
        };
        // Fall for 60 s without thrust.
        for _ in 0..1_800 {
            integrate(&mut s, &body, Wrench::default(), &env, 1.0 / 30.0);
        }
        let speed = (s.velocity - env.spin.cross(s.position)).length();
        assert!(
            (60.0..250.0).contains(&speed),
            "terminal speed near the ground {speed:.0} m/s"
        );
    }

    #[test]
    fn no_thrust_under_high_warp() {
        let env = meridian();
        let body = kestrel();
        let origin = circular(&env, 300_000.0).to_origin(body.mass.center_of_mass);
        let mut control = FlightControl {
            translate: DVec3::new(0.0, 0.0, -1.0),
            ..Default::default()
        };
        let mut telemetry = FlightTelemetry::default();
        let mut rails = None;
        fly(
            &origin,
            &body,
            &mut control,
            &mut telemetry,
            &mut rails,
            &env,
            0.0,
            10.0,
        );
        assert!(telemetry.on_rails);
        assert!(telemetry.throttles.iter().all(|&u| u == 0.0));
    }
}
