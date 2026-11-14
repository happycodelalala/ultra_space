//! Animated walking (Batch 3, task 3.11; design: vehicle-builder.md §7.2).
//!
//! A vehicle whose design declares legs can walk: in walk mode ([`Walking`]) a locomotion
//! controller moves its body kinematically along the terrain at standing height (like arcade
//! flight), turns it, and runs a gait. Each foot stays planted on the ground (fixed to the
//! turning surface) until it falls too far behind where the body wants it; then it steps along
//! an arc to a spot ahead, on the real terrain. A two-joint leg solver turns each foot's target
//! into hip and knee angles (and an ankle angle that keeps the sole level), set directly on the
//! vehicle's joints; colliders and meshes follow through the joint system.

use bevy_ecs::prelude::*;
use glam::{DQuat, DVec3};
use sim_core::ObjectState;
use sim_core::objects::{RegionBody, SimClock, SystemRef};

use crate::flight::{BodyEnv, FlightControl, FlightTelemetry, RegionTerrain, region_env};
use crate::joints::JointState;
use crate::vehicle::{Vehicle, VehicleBody};

/// One leg, by part names: the hip and knee joints, an optional ankle joint and the foot.
#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct LegDef {
    pub hip: String,
    pub knee: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ankle: Option<String>,
    pub foot: String,
}

/// How a vehicle walks (the design file's `gait`).
#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct GaitDef {
    pub legs: Vec<LegDef>,
    /// Longest step (m), step lift (m), top walking speed (m/s), turn rate (degrees per second).
    pub step_length_m: f64,
    pub step_height_m: f64,
    pub speed_m_s: f64,
    pub turn_rate_deg_s: f64,
    /// How bent the legs are when standing: hip-to-ankle distance as a fraction of the leg's
    /// length (1 = straight).
    pub stance: f64,
    /// How long a step takes (s): longer for longer legs (a step's time goes with the square
    /// root of the leg's length).
    #[serde(default = "default_step_time")]
    pub step_time_s: f64,
}

fn default_step_time() -> f64 {
    0.55
}

/// A leg resolved against the vehicle's parts (rest frame, vehicle axes).
#[derive(Clone, Debug, PartialEq)]
pub struct Leg {
    /// Rigid groups turned by the hip, knee and ankle joints.
    pub hip_group: usize,
    pub knee_group: usize,
    pub ankle_group: Option<usize>,
    /// Joint pivots at rest.
    pub hip: DVec3,
    pub knee: DVec3,
    pub ankle: DVec3,
    /// The sole's centre at rest (bottom of the foot).
    pub sole: DVec3,
}

impl Leg {
    pub fn thigh(&self) -> f64 {
        self.knee.distance(self.hip)
    }

    pub fn shin(&self) -> f64 {
        self.ankle.distance(self.knee)
    }
}

/// Resolves a gait's legs against a vehicle (`None` if a name is missing or not a joint).
pub fn resolve_legs(vehicle: &Vehicle, body: &VehicleBody, gait: &GaitDef) -> Option<Vec<Leg>> {
    let index = |name: &str| vehicle.parts.iter().position(|p| p.name == name);
    let joint_group = |name: &str| body.articulation.group_of_joint(index(name)?);
    gait.legs
        .iter()
        .map(|l| {
            let hip_group = joint_group(&l.hip)?;
            let knee_group = joint_group(&l.knee)?;
            let ankle_group = match &l.ankle {
                Some(a) => Some(joint_group(a)?),
                None => None,
            };
            let foot = &vehicle.parts[index(&l.foot)?];
            let half_height = foot.def.shape.bounds().1.y;
            let groups = &body.articulation.groups;
            let ankle = match ankle_group {
                Some(g) => groups[g].pivot,
                None => foot.position + DVec3::Y * half_height,
            };
            Some(Leg {
                hip_group,
                knee_group,
                ankle_group,
                hip: groups[hip_group].pivot,
                knee: groups[knee_group].pivot,
                ankle,
                sole: foot.position - DVec3::Y * half_height,
            })
        })
        .collect()
}

/// Hip and knee angles (radians, about the joints' X axes) that put the ankle at `target`
/// (vehicle rest frame; only Y and Z matter: legs swing forward and back). A positive turn
/// swings what hangs below a joint forward (−Z); knees bend backward.
pub fn solve_leg(leg: &Leg, target: DVec3) -> (f64, f64) {
    let (l1, l2) = (leg.thigh(), leg.shin());
    let rel = target - leg.hip;
    let (y, z) = (rel.y, rel.z);
    let d = (y * y + z * z).sqrt().clamp(1e-6, (l1 + l2) * 0.9999);
    // Angle from straight down to the target, positive forward (−Z).
    let phi = (-z).atan2(-y);
    let at_hip = ((l1 * l1 + d * d - l2 * l2) / (2.0 * l1 * d))
        .clamp(-1.0, 1.0)
        .acos();
    let at_knee = ((l1 * l1 + l2 * l2 - d * d) / (2.0 * l1 * l2))
        .clamp(-1.0, 1.0)
        .acos();
    (phi + at_hip, -(std::f64::consts::PI - at_knee))
}

/// A foot: planted on the ground (a point fixed to the turning surface) or stepping.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Foot {
    /// Where the sole is, in the body's surface frame (m from the planet's centre).
    pub planted: DVec3,
    /// While stepping: where it lifted from, where it lands (surface frame), and progress 0..1.
    pub step: Option<(DVec3, DVec3, f64)>,
}

/// Walk mode: the vehicle's legs carry it. Insert to start walking; set `stopping` to stop: it
/// slows down, brings its feet under it, and then leaves walk mode (the component is removed).
#[derive(Component, Clone, Debug, Default, PartialEq)]
pub struct Walking {
    /// Speed along the ground (m/s, forward positive) and the feet (empty until the first tick).
    pub speed: f64,
    pub feet: Vec<Foot>,
    pub stopping: bool,
    /// The body's vertical speed on its legs (m/s, the suspension's state).
    pub lift_rate: f64,
}

/// Keeps a legged vehicle off its legs: it doesn't stand or walk by itself on the ground (plain
/// physics), until removed.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct LegsOff;

/// What a tick of walking ended with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WalkStep {
    Walking,
    /// Stopped and standing on both feet: back to flight.
    Stopped,
    /// No ground under the feet: back to flight.
    NoGround,
    /// The pilot thrusts up: back to flight.
    TakeOff,
}

/// Walking acceleration (m/s²).
const ACCELERATION: f64 = 3.0;
/// Steepest slope the soles follow (radians).
const MAX_SOLE_PITCH: f64 = 0.35;
/// Walking ends when the ground is this much further below the feet than standing (m).
const LOST_GROUND: f64 = 3.0;
/// The legs' suspension: natural frequency (rad/s), critically damped.
const SUSPENSION: f64 = 5.0;
/// How fast the body straightens up after a tilted landing (rad/s).
const UPRIGHT_RATE: f64 = 1.5;
/// Upward stick beyond this takes off.
const TAKE_OFF_STICK: f64 = 0.5;

fn upright(forward: DVec3, up: DVec3) -> DQuat {
    let f = (forward - up * forward.dot(up)).normalize_or(up.any_orthonormal_vector());
    let right = f.cross(up);
    DQuat::from_mat3(&glam::DMat3::from_cols(right, up, -f)).normalize()
}

/// The ground point (body inertial frame) under `p`.
fn ground_at(env: &BodyEnv, p: DVec3) -> DVec3 {
    let dir = p.normalize();
    dir * (env.radius + env.ground_height(dir * env.radius))
}

/// One tick of walking for one vehicle.
#[allow(clippy::too_many_arguments)]
pub fn walk_tick(
    state: &mut sim_core::frames::FrameState,
    walking: &mut Walking,
    joints: &mut JointState,
    body: &VehicleBody,
    legs: &[Leg],
    gait: &GaitDef,
    control: &FlightControl,
    env: &BodyEnv,
    dt: f64,
) -> WalkStep {
    let to_surface = env.surface_rotation.inverse();
    let to_inertial = env.surface_rotation;
    // Standing height: the origin this far above the ground (legs bent to the stance).
    let leg_length = legs
        .iter()
        .map(|l| l.thigh() + l.shin())
        .fold(0.0, f64::max);
    let sole_depth = legs.iter().map(|l| -l.sole.y).fold(0.0, f64::max);
    let stand = sole_depth - leg_length * (1.0 - gait.stance);

    if !walking.stopping && control.translate.y > TAKE_OFF_STICK {
        return WalkStep::TakeOff;
    }
    // Ride the turning ground, then turn and move.
    let spin = DQuat::from_scaled_axis(env.spin * dt);
    state.position = spin * state.position;
    state.rotation = (spin * state.rotation).normalize();
    let up = state.position.normalize();
    let (steer, drive) = if walking.stopping {
        (0.0, 0.0)
    } else {
        (control.translate.x, control.translate.z)
    };
    let turn = -steer.clamp(-1.0, 1.0) * gait.turn_rate_deg_s.to_radians() * dt;
    let forward = DQuat::from_axis_angle(up, turn) * (state.rotation * DVec3::NEG_Z);
    let wanted = -drive.clamp(-1.0, 1.0) * gait.speed_m_s;
    walking.speed += (wanted - walking.speed).clamp(-ACCELERATION * dt, ACCELERATION * dt);
    let forward_level = (forward - up * forward.dot(up)).normalize_or(forward);
    state.position += forward_level * walking.speed * dt;
    let ground = ground_at(env, state.position);
    let height = state.position.length() - ground.length();
    if height > stand + LOST_GROUND {
        return WalkStep::NoGround;
    }
    // Suspension: the body rides at standing height over the mean ground under it and its
    // planted feet, sprung (critically damped) rather than snapped, and never higher than the
    // legs reach from the lowest foot.
    let mut target = ground.length() + stand;
    let planted: Vec<f64> = walking
        .feet
        .iter()
        .filter(|f| f.step.is_none())
        .map(|f| (to_inertial * f.planted).length())
        .collect();
    if !planted.is_empty() {
        let mean = (planted.iter().sum::<f64>() + ground.length()) / (planted.len() + 1) as f64;
        let lowest = planted.iter().copied().fold(f64::MAX, f64::min);
        target = (mean + stand).min(lowest + sole_depth - 0.05);
    }
    let radius = state.position.length();
    let accel = SUSPENSION * SUSPENSION * (target - radius) - 2.0 * SUSPENSION * walking.lift_rate;
    walking.lift_rate += accel * dt;
    let radius = radius + walking.lift_rate * dt;
    state.position = ground.normalize() * radius;
    let up = state.position.normalize();
    // Straighten up (after a tilted landing) at a limited rate.
    let level = upright(forward_level, up);
    let off = state.rotation.angle_between(level);
    let step = UPRIGHT_RATE * dt;
    state.rotation = if off <= step {
        level
    } else {
        state.rotation.slerp(level, step / off).normalize()
    };
    state.velocity =
        env.spin.cross(state.position) + forward_level * walking.speed + up * walking.lift_rate;
    state.angular_velocity = env.spin;

    // The body's pose: vehicle rest frame -> inertial.
    let to_world = |x: DVec3| state.position + state.rotation * x;
    let from_world = |p: DVec3| state.rotation.inverse() * (p - state.position);
    let surface = |p: DVec3| to_surface * p;
    let inertial = |p: DVec3| to_inertial * p;

    // Feet: plant them where they stand on the first tick.
    if walking.feet.len() != legs.len() {
        walking.feet = legs
            .iter()
            .map(|l| Foot {
                planted: surface(ground_at(env, to_world(l.sole))),
                step: None,
            })
            .collect();
    }
    let stepping = walking.feet.iter().any(|f| f.step.is_some());
    // Where each foot would like to be: under the vehicle, a little ahead when moving.
    let lead = forward_level * walking.speed * gait.step_time_s * 0.5;
    let mut started = stepping;
    // Stopping: feet come closer under the hips before standing.
    let near = if walking.stopping { 0.15 } else { 0.4 };
    let mut settled = !stepping && walking.speed == 0.0;
    for (foot, leg) in walking.feet.iter_mut().zip(legs) {
        // Fore and aft under the centre of mass (balance), sideways under the hip.
        let under = DVec3::new(leg.sole.x, leg.sole.y, body.mass.center_of_mass.z);
        let home = ground_at(env, to_world(under) + lead);
        match foot.step {
            Some((from, to, t)) => {
                let t = (t + dt / gait.step_time_s).min(1.0);
                if t >= 1.0 {
                    foot.planted = to;
                    foot.step = None;
                } else {
                    foot.step = Some((from, to, t));
                }
            }
            None => {
                let off = inertial(foot.planted).distance(home);
                let limit = if walking.speed.abs() > 0.1 {
                    gait.step_length_m * 0.5
                } else {
                    near
                };
                if off > near {
                    settled = false;
                }
                if off > limit && !started {
                    foot.step = Some((foot.planted, surface(home), 0.0));
                    started = true;
                }
            }
        }
    }

    // Legs: solve each ankle onto its foot.
    let n = body.articulation.groups.len();
    if joints.angles.len() != n {
        *joints = JointState::rest(&body.articulation);
    }
    for (foot, leg) in walking.feet.iter().zip(legs) {
        let sole = match foot.step {
            Some((from, to, t)) => {
                let (a, b) = (inertial(from), inertial(to));
                let p = a.lerp(b, t);
                let lift = gait.step_height_m * (std::f64::consts::PI * t).sin();
                ground_at(env, p) + p.normalize() * lift
            }
            None => inertial(foot.planted),
        };
        // With an ankle, the sole follows the ground's fore-and-aft slope (a flat sole on a slope
        // would dig in at one end); the ankle sits above the sole as at rest, tilted with it.
        let pitch = if leg.ankle_group.is_some() {
            let n = state.rotation.inverse() * env.ground_normal(sole);
            n.z.atan2(n.y).clamp(-MAX_SOLE_PITCH, MAX_SOLE_PITCH)
        } else {
            0.0
        };
        let target = from_world(sole) + DQuat::from_rotation_x(pitch) * (leg.ankle - leg.sole);
        let (hip, knee) = solve_leg(leg, target);
        joints.angles[leg.hip_group] = hip;
        joints.targets[leg.hip_group] = hip;
        joints.angles[leg.knee_group] = knee;
        joints.targets[leg.knee_group] = knee;
        if let Some(a) = leg.ankle_group {
            // Undo the thigh's and shin's tilt, then match the ground.
            joints.angles[a] = pitch - (hip + knee);
            joints.targets[a] = pitch - (hip + knee);
        }
    }
    joints.poses = body.articulation.poses(&joints.angles);
    joints.generation += 1;
    if walking.stopping && settled {
        WalkStep::Stopped
    } else {
        WalkStep::Walking
    }
}

/// A legged vehicle on the ground slower than this (m/s) stands on its legs.
const STAND_SPEED: f64 = 3.0;

/// Puts legged vehicles that touch the ground slowly on their legs (spawned, landed, or stopped
/// after walking): they stand and walk instead of resting on their feet as a rigid body, which
/// topples on slopes. Not while held in the builder, in arcade flight or cruise, with the legs
/// switched off, or with the pilot thrusting up. Runs before [`walking_system`].
#[allow(clippy::type_complexity)]
pub fn stand_system(
    mut commands: Commands,
    vehicles: Query<
        (
            Entity,
            &Vehicle,
            &FlightControl,
            &FlightTelemetry,
            Option<&crate::cruise::Cruise>,
        ),
        (
            Without<Walking>,
            Without<LegsOff>,
            Without<crate::flight::Frozen>,
            Without<crate::arcade::ArcadeFlight>,
        ),
    >,
) {
    for (entity, vehicle, control, telemetry, cruise) in &vehicles {
        if vehicle.gait.is_some()
            && telemetry.in_contact
            && telemetry.relative_velocity.length() < STAND_SPEED
            && control.translate.y <= TAKE_OFF_STICK
            && !cruise.is_some_and(crate::cruise::Cruise::is_cruising)
        {
            commands.entity(entity).insert(Walking::default());
        }
    }
}

/// Walks every vehicle in walk mode for one tick (vehicles without legs, or without ground under
/// them, leave walk mode). Runs before the joint system.
#[allow(clippy::type_complexity)]
pub fn walking_system(
    mut commands: Commands,
    region: Res<RegionBody>,
    system: Res<SystemRef>,
    clock: Res<SimClock>,
    terrain: Option<Res<RegionTerrain>>,
    mut vehicles: Query<(
        Entity,
        &mut ObjectState,
        &mut Walking,
        &Vehicle,
        &VehicleBody,
        &FlightControl,
        Option<&mut JointState>,
        Option<&mut FlightTelemetry>,
    )>,
) {
    let system = &system.0;
    let t = clock.time.as_secs_f64();
    let env = region_env(system, region.0, t, terrain.as_deref());
    let frame = system.bodies[region.0].inertial_frame;
    for (entity, mut state, mut walking, vehicle, body, control, joints, telemetry) in &mut vehicles
    {
        let legs = vehicle
            .gait
            .as_ref()
            .and_then(|g| Some((g, resolve_legs(vehicle, body, g)?)));
        let (Some((gait, legs)), Some(mut joints)) = (legs, joints) else {
            commands.entity(entity).remove::<Walking>();
            continue;
        };
        let mut local = state.in_frame(&system.frames, frame, t);
        let step = walk_tick(
            &mut local.state,
            &mut walking,
            &mut joints,
            body,
            &legs,
            gait,
            control,
            &env,
            clock.dt,
        );
        *state = local;
        if let Some(mut tel) = telemetry {
            tel.in_contact = step != WalkStep::NoGround;
            tel.relative_velocity = local.state.velocity - env.spin.cross(local.state.position);
            // Heights of the centre of mass, as in flight.
            let com = local.state.position + local.state.rotation * body.mass.center_of_mass;
            tel.altitude = com.length() - env.radius;
            tel.ground_altitude = crate::flight::ground_altitude(&env, com);
        }
        if step != WalkStep::Walking {
            commands.entity(entity).remove::<Walking>();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leg() -> Leg {
        // Thigh 5 m, shin 5 m, straight down from the hip at the origin.
        Leg {
            hip_group: 1,
            knee_group: 2,
            ankle_group: None,
            hip: DVec3::ZERO,
            knee: DVec3::new(0.0, -5.0, 0.0),
            ankle: DVec3::new(0.0, -10.0, 0.0),
            sole: DVec3::new(0.0, -11.0, 0.0),
        }
    }

    /// Where the ankle ends up for hip and knee angles (forward kinematics in the leg plane).
    fn ankle_at(l: &Leg, hip: f64, knee: f64) -> DVec3 {
        let r_hip = DQuat::from_rotation_x(hip);
        let knee_at = l.hip + r_hip * (l.knee - l.hip);
        knee_at + r_hip * DQuat::from_rotation_x(knee) * (l.ankle - l.knee)
    }

    #[test]
    fn the_leg_solver_reaches_its_targets() {
        let l = leg();
        for target in [
            DVec3::new(0.0, -9.0, 0.0),
            DVec3::new(0.0, -8.0, -3.0), // ahead
            DVec3::new(0.0, -8.5, 2.5),  // behind
        ] {
            let (hip, knee) = solve_leg(&l, target);
            let reached = ankle_at(&l, hip, knee);
            assert!(reached.distance(target) < 1e-9, "{target:?} -> {reached:?}");
            assert!(knee <= 1e-12, "knees bend backward: {knee}");
        }
        // Straight down at full length: (nearly) straight leg; the solver keeps a slight bend.
        let (hip, knee) = solve_leg(&l, DVec3::new(0.0, -10.0, 0.0));
        assert!(hip.abs() < 0.05 && knee.abs() < 0.05, "{hip} {knee}");
    }
}
