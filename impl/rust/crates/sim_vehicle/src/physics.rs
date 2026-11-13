//! Rigid-body physics with rapier (Batch 3, task 3.1; design: vehicle-builder.md §2).
//!
//! Each region owns one [`PhysicsScene`]: a rapier world in the region body's inertial frame.
//! Every tick the region's flight code writes each vehicle's state and its control wrench into
//! the scene, the scene steps all vehicles together (gravity and drag re-applied every
//! substep), and the states are read back. The object state stays the authority between ticks,
//! so region transfers, teleports and other movers need no extra bookkeeping: a body whose object
//! wasn't placed this tick is removed.
//!
//! The ground is made of terrain collision patches on a body turning with the planet
//! ([`crate::ground`], task 3.2). Where a body's patches aren't built yet, the Batch 2 contact
//! model against the terrain height keeps it above the ground.

use std::collections::HashMap;
use std::sync::Arc;

use bevy_ecs::prelude::*;
use glam::{DQuat, DVec3};
use rapier3d_f64::control::{
    CharacterLength, DynamicRayCastVehicleController, KinematicCharacterController,
};
use rapier3d_f64::parry::query::Ray;
use rapier3d_f64::parry::shape::Capsule;
use rapier3d_f64::prelude::{
    ColliderBuilder, ColliderHandle, Group, InteractionGroups, InteractionTestMode,
    MassProperties as RapierMass, PhysicsWorld, Pose, QueryFilter, RigidBodyBuilder,
    RigidBodyHandle, RigidBodyType, SharedShape,
};
use sim_core::ObjectId;

use crate::allocation::Wrench;
use crate::catalog::Shape;
use crate::flight::{BodyEnv, MAX_SUBSTEP, MAX_SUBSTEPS, RigidState, ground_contact};
use crate::ground::{GROUND_GROUP, Ground};
use crate::vehicle::{PartCollider, VehicleBody};
use crate::wheels::{Drive, WheelMount};

/// Hull friction (combined with the other collider's by averaging).
const HULL_FRICTION: f64 = 0.8;
/// Bodies closer to the ground than their radius plus this (m), plus a few seconds of travel,
/// get collision patches.
const PATCH_ALTITUDE: f64 = 50.0;
/// Collision group of vehicles.
const VEHICLE_GROUP: Group = Group::GROUP_2;
/// Solver iterations per step while bodies are near the ground.
const CONTACT_ITERATIONS: usize = 4;
/// A body still on the ground (slower than these, relative to it) with no thrust for
/// `REST_AFTER` seconds comes to rest: pinned to the turning ground until something wakes it.
const REST_SPEED: f64 = 0.1;
const REST_SPIN: f64 = 0.05;
const REST_AFTER: f64 = 1.0;
/// Walkers' capsule (m).
const CHARACTER_HEIGHT: f64 = 1.8;
const CHARACTER_RADIUS: f64 = 0.3;
const CHARACTER_GAP: f64 = 0.02;
/// Most look-ahead along a body's motion when choosing patches (m).
const PATCH_LOOKAHEAD: f64 = 60.0;
/// Patches are built this far around a body (m), plus a second and a half of travel.
const PATCH_MARGIN: f64 = 20.0;
/// Drag coefficient times reference area, as a fraction of the vehicle's bounding disc.
const DRAG_AREA_FRACTION: f64 = 0.5;

/// How a body moves this tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Motion {
    /// Integrated by rapier under gravity, drag and the control wrench.
    Dynamic,
    /// Moved by other code (cruise, arcade flight); placed where its object is.
    Kinematic,
}

struct Entry {
    handle: RigidBodyHandle,
    collider_handles: Vec<ColliderHandle>,
    /// The rigid group of each collider, and the joint poses they were last placed at.
    collider_groups: Vec<usize>,
    pose_generation: u64,
    /// The joint poses changed since the last step (a resting body wakes to settle again).
    reposed: bool,
    /// The design hash it was built from; a different hash means the parts changed.
    hash: u64,
    mass: crate::mass::MassProperties,
    /// Radius of a sphere around the centre of mass containing the vehicle (m).
    radius: f64,
    wrench: Wrench,
    motion: Motion,
    placed: bool,
    contact: bool,
    /// The ground under it has collision patches (otherwise the Batch 2 contact model applies).
    on_patches: bool,
    /// How long it has been still on the ground (s).
    still: f64,
    /// Wheels and the controller driving them.
    wheels: Vec<WheelMount>,
    controller: Option<DynamicRayCastVehicleController>,
    drive: Drive,
    /// At rest: its pose in the ground's (surface) frame.
    rest: Option<Pose>,
}

/// A region's rapier world and its vehicles' bodies.
pub struct PhysicsScene {
    world: PhysicsWorld,
    bodies: HashMap<ObjectId, Entry>,
    /// Compound collision shapes by design hash and rigid group, shared by every vehicle of a
    /// design.
    shapes: HashMap<(u64, usize), SharedShape>,
    ground: Option<Ground>,
    /// Whether terrain gets collision patches (single-vehicle helpers use the contact model).
    patches: bool,
    /// Build patches on the calling thread (tests).
    inline_patches: bool,
    /// Time stepped so far (s), for patch lifetimes.
    time: f64,
    /// The ground's angular velocity in the last step.
    ground_spin: DVec3,
    stats: PhysicsStats,
}

/// What the last step cost and held (diagnostics: logged when a tick runs over budget).
#[derive(Clone, Copy, Debug, Default)]
pub struct PhysicsStats {
    pub bodies: usize,
    pub patches: usize,
    pub patches_pending: usize,
    /// Times the ground was rebuilt from a different terrain.
    pub ground_rebuilds: u64,
    pub substeps: u32,
    pub step_ms: f64,
    pub ground_ms: f64,
    pub world_ms: f64,
}

impl Default for PhysicsScene {
    fn default() -> Self {
        let mut world = PhysicsWorld::new();
        // Gravity points to the body's centre; it's applied per body.
        world.gravity = DVec3::ZERO;
        // Rapier caps speeds at 400 m/s by default; orbits need thousands.
        world.integration_parameters.normalized_max_linear_velocity = f64::MAX;
        // Solver iterations are set per step (see `step`).
        world.integration_parameters.num_solver_iterations = 1;
        Self {
            world,
            bodies: HashMap::new(),
            shapes: HashMap::new(),
            ground: None,
            patches: true,
            inline_patches: false,
            time: 0.0,
            ground_spin: DVec3::ZERO,
            stats: PhysicsStats::default(),
        }
    }
}

/// A region's physics scene (created by the flight system on first use).
#[derive(Resource, Default)]
pub struct RegionPhysics(pub PhysicsScene);

fn part_shape(shape: Shape) -> SharedShape {
    match shape {
        Shape::Box { size_m } => {
            SharedShape::cuboid(size_m[0] / 2.0, size_m[1] / 2.0, size_m[2] / 2.0)
        }
        Shape::Cylinder { radius_m, length_m } => SharedShape::cylinder(length_m / 2.0, radius_m),
        Shape::Sphere { radius_m } => SharedShape::ball(radius_m),
    }
}

/// Grid cells per axis (x, y, z) for merging parts into boxes.
const HULL_GRID: [usize; 3] = [4, 2, 4];

/// Points on a part shape's surface (its corners, or rings for round shapes), placed.
fn shape_points(c: &PartCollider) -> Vec<DVec3> {
    let corners = |h: DVec3| -> Vec<DVec3> {
        (0..8)
            .map(|i| {
                let s = |bit: usize| if i & bit == 0 { -1.0 } else { 1.0 };
                DVec3::new(s(1) * h.x, s(2) * h.y, s(4) * h.z)
            })
            .collect()
    };
    let local: Vec<DVec3> = match c.shape {
        Shape::Box { size_m } => corners(DVec3::from(size_m) / 2.0),
        Shape::Cylinder { radius_m, length_m } => (0..16)
            .map(|i| {
                let a = std::f64::consts::TAU * f64::from(i % 8) / 8.0;
                let y = if i < 8 {
                    -length_m / 2.0
                } else {
                    length_m / 2.0
                };
                DVec3::new(a.cos() * radius_m, y, a.sin() * radius_m)
            })
            .collect(),
        Shape::Sphere { radius_m } => [
            DVec3::X,
            DVec3::NEG_X,
            DVec3::Y,
            DVec3::NEG_Y,
            DVec3::Z,
            DVec3::NEG_Z,
        ]
        .into_iter()
        .chain(corners(DVec3::ONE).into_iter().map(DVec3::normalize))
        .map(|d| d * radius_m)
        .collect(),
    };
    local
        .into_iter()
        .map(|p| c.position + c.rotation * p)
        .collect()
}

/// A vehicle's compiled collision shape (vehicle-builder.md §4): one compound collider whose
/// pieces are the exact shapes of feet and wheels plus a few boxes, each wrapping the other
/// parts within one cell of a coarse grid over the vehicle (boxes step cheaper than convex
/// hulls). Its cost per step then stays about the same however many parts the vehicle has.
pub fn compound_shape(colliders: &[PartCollider]) -> SharedShape {
    let mut pieces = Vec::new();
    let (lo, hi) = colliders.iter().fold(
        (DVec3::splat(f64::MAX), DVec3::splat(f64::MIN)),
        |(lo, hi), c| (lo.min(c.position), hi.max(c.position)),
    );
    let size = (hi - lo).max(DVec3::splat(1e-6));
    let mut cells: HashMap<[usize; 3], Vec<DVec3>> = HashMap::new();
    for c in colliders {
        if c.exact {
            pieces.push((
                Pose::from_parts(c.position, c.rotation),
                part_shape(c.shape),
            ));
            continue;
        }
        let f = (c.position - lo) / size;
        let cell = [0, 1, 2].map(|i| ((f[i] * HULL_GRID[i] as f64) as usize).min(HULL_GRID[i] - 1));
        cells.entry(cell).or_default().extend(shape_points(c));
    }
    let mut cells: Vec<_> = cells.into_iter().collect();
    cells.sort_by_key(|(cell, _)| *cell);
    for (_, points) in cells {
        let (lo, hi) = points.iter().fold(
            (DVec3::splat(f64::MAX), DVec3::splat(f64::MIN)),
            |(lo, hi), p| (lo.min(*p), hi.max(*p)),
        );
        let half = ((hi - lo) / 2.0).max(DVec3::splat(0.05));
        pieces.push((
            Pose::from_translation((lo + hi) / 2.0),
            SharedShape::cuboid(half.x, half.y, half.z),
        ));
    }
    SharedShape::compound(pieces)
}

/// Radius of a sphere around the centre of mass that contains the vehicle (rough; the
/// Batch 2 ground-contact clearance, kept until terrain colliders replace it in task 3.2).
pub fn vehicle_radius(body: &VehicleBody) -> f64 {
    body.allocator
        .thrusters()
        .iter()
        .map(|t| t.position.length())
        .fold(2.0, f64::max)
}

fn gravity(mu: f64, r: DVec3) -> DVec3 {
    let d = r.length();
    if d <= 0.0 {
        return DVec3::ZERO;
    }
    -r * (mu / (d * d * d))
}

impl PhysicsScene {
    /// A scene whose ground is only the Batch 2 contact model (no collision patches): for
    /// single-vehicle helpers that make a new scene every step.
    pub fn without_patches() -> Self {
        Self {
            patches: false,
            ..Self::default()
        }
    }

    /// A scene that builds collision patches immediately instead of in the background (tests).
    pub fn with_inline_patches() -> Self {
        Self {
            inline_patches: true,
            ..Self::default()
        }
    }

    /// What the last step cost and held.
    pub fn stats(&self) -> PhysicsStats {
        self.stats
    }

    /// Number of built collision patches.
    pub fn patch_count(&self) -> usize {
        self.ground.as_ref().map_or(0, Ground::len)
    }

    /// Number of bodies (for stats and tests).
    pub fn len(&self) -> usize {
        self.bodies.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bodies.is_empty()
    }

    /// Places a vehicle integrated by physics this tick, at its centre-of-mass `state`, with a
    /// control wrench in vehicle axes.
    pub fn place_dynamic(
        &mut self,
        id: ObjectId,
        body: &VehicleBody,
        state: &RigidState,
        wrench: Wrench,
    ) {
        self.place(id, body, state, wrench, Motion::Dynamic);
    }

    /// Like [`Self::place_dynamic`], for a vehicle that may have wheels: `drive` is what the
    /// driver asks of them.
    pub fn place_driven(
        &mut self,
        id: ObjectId,
        body: &VehicleBody,
        state: &RigidState,
        wrench: Wrench,
        drive: Drive,
    ) {
        self.place(id, body, state, wrench, Motion::Dynamic);
        if let Some(e) = self.bodies.get_mut(&id) {
            e.drive = drive;
        }
    }

    /// Moves a jointed vehicle's group colliders to `poses` (from its `JointState`), unless they
    /// are already there (same `generation`).
    pub fn set_poses(&mut self, id: ObjectId, poses: &[crate::joints::Pose], generation: u64) {
        let Some(e) = self.bodies.get_mut(&id) else {
            return;
        };
        if e.pose_generation == generation {
            return;
        }
        e.pose_generation = generation;
        e.reposed = true;
        for (&h, &g) in e.collider_handles.iter().zip(&e.collider_groups) {
            let pose = poses
                .get(g)
                .copied()
                .unwrap_or(crate::joints::Pose::IDENTITY);
            if let Some(c) = self.world.colliders.get_mut(h) {
                c.set_position_wrt_parent(Pose::from_parts(pose.translation, pose.rotation));
            }
        }
    }

    /// Places a vehicle moved by other code this tick (it still collides with others).
    pub fn place_kinematic(&mut self, id: ObjectId, body: &VehicleBody, state: &RigidState) {
        self.place(id, body, state, Wrench::default(), Motion::Kinematic);
    }

    fn place(
        &mut self,
        id: ObjectId,
        body: &VehicleBody,
        state: &RigidState,
        wrench: Wrench,
        motion: Motion,
    ) {
        let stale = self.bodies.get(&id).is_some_and(|e| e.hash != body.hash);
        if stale && let Some(old) = self.bodies.remove(&id) {
            self.world.remove_body(old.handle);
        }
        let world = &mut self.world;
        let shapes = &mut self.shapes;
        let entry = self.bodies.entry(id).or_insert_with(|| {
            let m = &body.mass;
            let rigid = RigidBodyBuilder::dynamic()
                .can_sleep(false)
                .additional_mass_properties(RapierMass::with_inertia_matrix(
                    m.center_of_mass,
                    m.mass,
                    m.inertia,
                ))
                .build();
            let handle = world.insert_body(rigid);
            // One compound collider per rigid group (one without joints); jointed groups move
            // with their poses (see `set_poses`).
            let collider_handles = (0..body.articulation.groups.len().max(1))
                .filter_map(|g| {
                    let pieces: Vec<PartCollider> = body
                        .colliders
                        .iter()
                        .filter(|c| c.group == g)
                        .copied()
                        .collect();
                    if pieces.is_empty() {
                        return None;
                    }
                    let shape = shapes
                        .entry((body.hash, g))
                        .or_insert_with(|| compound_shape(&pieces))
                        .clone();
                    let collider = ColliderBuilder::new(shape)
                        // Mass and inertia come from our own mass properties.
                        .density(0.0)
                        .friction(HULL_FRICTION)
                        .collision_groups(vehicle_groups(true));
                    Some((g, world.insert_collider(collider, Some(handle))))
                })
                .collect::<Vec<_>>();
            let collider_groups = collider_handles.iter().map(|(g, _)| *g).collect();
            let collider_handles = collider_handles.into_iter().map(|(_, h)| h).collect();
            let wheels = body.wheels.clone();
            let controller =
                (!wheels.is_empty()).then(|| crate::wheels::controller(handle, &wheels, m.mass));
            Entry {
                handle,
                collider_handles,
                collider_groups,
                pose_generation: 0,
                reposed: false,
                wheels,
                controller,
                drive: Drive::default(),
                hash: body.hash,
                mass: body.mass,
                radius: vehicle_radius(body),
                wrench,
                motion,
                placed: true,
                contact: false,
                on_patches: false,
                still: 0.0,
                rest: None,
            }
        });
        entry.wrench = wrench;
        entry.drive = Drive::default();
        entry.motion = motion;
        entry.placed = true;
        entry.contact = false;
        let com = body.mass.center_of_mass;
        let rb = &mut world.bodies[entry.handle];
        let kind = match motion {
            Motion::Dynamic if entry.rest.is_none() => RigidBodyType::Dynamic,
            _ => RigidBodyType::KinematicPositionBased,
        };
        if rb.body_type() != kind {
            rb.set_body_type(kind, true);
        }
        let origin = state.position - state.rotation * com;
        rb.set_position(Pose::from_parts(origin, state.rotation), true);
        rb.set_linvel(state.velocity, true);
        rb.set_angvel(state.angular_velocity, true);
    }

    /// Advances every dynamic body by `dt` under its body's gravity, air drag and its control
    /// wrench, in substeps of at most [`MAX_SUBSTEP`]. Bodies not placed since the last step are
    /// removed first.
    pub fn step(&mut self, env: &BodyEnv, dt: f64) {
        let gone: Vec<ObjectId> = self
            .bodies
            .iter()
            .filter(|(_, e)| !e.placed)
            .map(|(id, _)| *id)
            .collect();
        for id in gone {
            if let Some(e) = self.bodies.remove(&id) {
                self.world.remove_body(e.handle);
            }
        }
        if (self.bodies.is_empty() && self.ground.is_none()) || dt <= 0.0 {
            return;
        }
        let started = std::time::Instant::now();
        self.ground_spin = env.spin;
        self.update_ground(env);
        let ground_ms = started.elapsed().as_secs_f64() * 1e3;
        let mut world_ms = 0.0;
        self.update_rest(env);
        // Rapier splits each step into solver iterations that share one force: in free flight
        // one iteration per step keeps gravity re-evaluated every step (symplectic orbits);
        // near the ground, contacts and friction need rapier's usual four.
        let near_ground = self.bodies.values().any(|e| e.on_patches);
        self.world.integration_parameters.num_solver_iterations =
            if near_ground { CONTACT_ITERATIONS } else { 1 };
        let substeps = (dt / MAX_SUBSTEP)
            .ceil()
            .clamp(1.0, f64::from(MAX_SUBSTEPS)) as u32;
        let h = dt / f64::from(substeps);
        self.world.integration_parameters.dt = h;
        for k in 0..substeps {
            if self.ground.is_some() {
                let turned = DQuat::from_scaled_axis(env.spin * (h * f64::from(k + 1)));
                let rotation = (turned * env.surface_rotation).normalize();
                // Resting bodies turn with the ground.
                for e in self.bodies.values() {
                    if let Some(local) = e.rest {
                        let world = Pose::from_rotation(rotation) * local;
                        self.world.bodies[e.handle].set_next_kinematic_position(world);
                    }
                }
            }
            for e in self.bodies.values() {
                if e.motion != Motion::Dynamic {
                    continue;
                }
                let rb = &mut self.world.bodies[e.handle];
                let m = e.mass.mass;
                let rotation = *rb.rotation();
                let p = rb.position().translation + rotation * e.mass.center_of_mass;
                let rho = env.air_density_at(p);
                let drag = if rho > 0.0 {
                    let area = DRAG_AREA_FRACTION * std::f64::consts::PI * e.radius * e.radius;
                    let air = rb.linvel() - env.spin.cross(p);
                    -air * (0.5 * rho * area * air.length())
                } else {
                    DVec3::ZERO
                };
                rb.reset_forces(false);
                rb.reset_torques(false);
                rb.add_force(
                    gravity(env.mu, p) * m + rotation * e.wrench.force + drag,
                    false,
                );
                rb.add_torque(rotation * e.wrench.torque, false);
            }
            // Wheels: suspension, drive and tyre forces for this step. Rapier's controller damps
            // the suspension with the chassis's absolute velocity, so on a turning planet it runs
            // in the ground's frame: the ground's motion is taken out of the chassis (and the
            // ground body stopped) for the call, then put back. Impulses add, so this is exact
            // but for the tiny rotating-frame terms.
            let world = &mut self.world;
            let ground_body = self.ground.as_ref().map(|g| g.body);
            let ground_vels = ground_body.map(|g| {
                let b = &mut world.bodies[g];
                let v = (b.linvel(), b.angvel());
                b.set_linvel(DVec3::ZERO, false);
                b.set_angvel(DVec3::ZERO, false);
                v
            });
            for e in self.bodies.values_mut() {
                let Some(c) = &mut e.controller else { continue };
                if e.motion != Motion::Dynamic || e.rest.is_some() {
                    continue;
                }
                crate::wheels::apply(c, &e.wheels, e.drive, h);
                let chassis = &mut world.bodies[e.handle];
                let frame_velocity = env.spin.cross(chassis.center_of_mass());
                chassis.set_linvel(chassis.linvel() - frame_velocity, false);
                chassis.set_angvel(chassis.angvel() - env.spin, false);
                let filter = QueryFilter::default().exclude_rigid_body(e.handle);
                let queries = world.broad_phase.as_query_pipeline_mut(
                    world.narrow_phase.query_dispatcher(),
                    &mut world.bodies,
                    &mut world.colliders,
                    filter,
                );
                c.update_vehicle(h, queries);
                let chassis = &mut world.bodies[e.handle];
                chassis.set_linvel(chassis.linvel() + frame_velocity, false);
                chassis.set_angvel(chassis.angvel() + env.spin, false);
                if c.wheels().iter().any(|w| w.raycast_info().is_in_contact) {
                    e.contact = true;
                }
            }
            if let (Some(g), Some((v, w))) = (ground_body, ground_vels) {
                let b = &mut world.bodies[g];
                b.set_linvel(v, false);
                b.set_angvel(w, false);
            }
            let world_started = std::time::Instant::now();
            self.world.step();
            world_ms += world_started.elapsed().as_secs_f64() * 1e3;
            let ground = self.ground.as_ref().map(|g| g.body);
            for e in self.bodies.values_mut() {
                if e.motion != Motion::Dynamic {
                    continue;
                }
                if e.rest.is_some() {
                    e.contact = true;
                    continue;
                }
                if e.on_patches {
                    if let Some(ground) = ground {
                        e.contact |= touching(&self.world, e, ground);
                    }
                    continue;
                }
                let mut state = read(&self.world, e);
                if ground_contact(&mut state, env, e.radius, h) {
                    e.contact = true;
                    write(&mut self.world, e, &state);
                }
            }
        }
        self.time += dt;
        self.settle(env, dt);
        self.stats = PhysicsStats {
            bodies: self.bodies.len(),
            patches: self.patch_count(),
            patches_pending: self.ground.as_ref().map_or(0, Ground::pending),
            ground_rebuilds: self.stats.ground_rebuilds,
            substeps,
            step_ms: started.elapsed().as_secs_f64() * 1e3,
            ground_ms,
            world_ms,
        };
        for e in self.bodies.values_mut() {
            e.placed = false;
        }
    }

    /// Wakes resting bodies that thrust, left the patches or were hit, and pins the others to
    /// the ground's current rotation.
    fn update_rest(&mut self, env: &BodyEnv) {
        let ground = self.ground.as_ref().map(|g| g.body);
        for e in self.bodies.values_mut() {
            let reposed = std::mem::take(&mut e.reposed);
            if reposed {
                e.still = 0.0;
            }
            let Some(local) = e.rest else { continue };
            let thrusting = e.wrench.force != DVec3::ZERO
                || e.wrench.torque != DVec3::ZERO
                || !e.drive.is_idle();
            let hit = ground.is_some_and(|g| touched_by_other(&self.world, e, g));
            if thrusting || reposed || e.motion != Motion::Dynamic || !e.on_patches || hit {
                wake(&mut self.world, e, env.spin);
                continue;
            }
            let rb = &mut self.world.bodies[e.handle];
            if rb.body_type() != RigidBodyType::KinematicPositionBased {
                rb.set_body_type(RigidBodyType::KinematicPositionBased, true);
            }
            rb.set_position(Pose::from_rotation(env.surface_rotation) * local, false);
        }
    }

    /// Puts bodies that have been still on the ground long enough to rest.
    fn settle(&mut self, env: &BodyEnv, dt: f64) {
        if self.ground.is_none() {
            return;
        }
        // The ground's rotation at the end of this step.
        let rotation = (DQuat::from_scaled_axis(env.spin * dt) * env.surface_rotation).normalize();
        for e in self.bodies.values_mut() {
            if e.motion != Motion::Dynamic || e.rest.is_some() {
                continue;
            }
            let idle = e.wrench.force == DVec3::ZERO
                && e.wrench.torque == DVec3::ZERO
                && e.drive.is_idle();
            let s = read(&self.world, e);
            let speed = (s.velocity - env.spin.cross(s.position)).length();
            let spin = (s.angular_velocity - env.spin).length();
            if idle && e.on_patches && e.contact && speed < REST_SPEED && spin < REST_SPIN {
                e.still += dt;
            } else {
                e.still = 0.0;
            }
            if e.still >= REST_AFTER {
                let pose = *self.world.bodies[e.handle].position();
                e.rest = Some(Pose::from_rotation(rotation.inverse()) * pose);
                self.world.bodies[e.handle]
                    .set_body_type(RigidBodyType::KinematicPositionBased, true);
                // Pinned to the ground: no need to collide with it (other bodies still hit it).
                set_ground_collisions(&mut self.world, e, false);
            }
        }
    }

    /// Moves a walker's capsule (feet at `feet`, standing along `up`) by `desired`, against the
    /// vehicles as the last step left them (not the ground: walkers stand on the terrain height,
    /// which the patches match). Returns the push out of any overlap, the movement actually made
    /// after it, and whether it stands on a vehicle.
    pub fn sweep_character(
        &self,
        feet: DVec3,
        up: DVec3,
        desired: DVec3,
        dt: f64,
    ) -> (DVec3, DVec3, bool) {
        let controller = KinematicCharacterController {
            up,
            offset: CharacterLength::Absolute(CHARACTER_GAP),
            ..Default::default()
        };
        let half = CHARACTER_HEIGHT / 2.0 - CHARACTER_RADIUS;
        let shape = Capsule::new_y(half, CHARACTER_RADIUS);
        let pose = Pose::from_parts(
            // The controller keeps its gap from surfaces: the capsule floats that far above the
            // feet (touching the ground counts as stuck in it).
            feet + up * (CHARACTER_HEIGHT / 2.0 + CHARACTER_GAP),
            DQuat::from_rotation_arc(DVec3::Y, up),
        );
        let world = &self.world;
        let vehicles_only = QueryFilter::default().groups(InteractionGroups::new(
            Group::ALL,
            Group::ALL.difference(GROUND_GROUP),
            InteractionTestMode::And,
        ));
        let queries = world.broad_phase.as_query_pipeline(
            world.narrow_phase.query_dispatcher(),
            &world.bodies,
            &world.colliders,
            vehicles_only,
        );
        // First out of any vehicle the capsule overlaps (rapier only depenetrates on a zero
        // move), then the move itself from there.
        let out = controller.move_shape(dt, &queries, &shape, &pose, DVec3::ZERO, |_| {});
        let pose = Pose::from_parts(pose.translation + out.translation, pose.rotation);
        let moved = controller.move_shape(dt, &queries, &shape, &pose, desired, |_| {});
        (out.translation, moved.translation, moved.grounded)
    }

    /// Distance along a ray to the first collider it hits (ground patches and vehicles, as the
    /// last step left them), within `max` metres.
    pub fn cast_ray(&self, origin: DVec3, direction: DVec3, max: f64) -> Option<f64> {
        let world = &self.world;
        let queries = world.broad_phase.as_query_pipeline(
            world.narrow_phase.query_dispatcher(),
            &world.bodies,
            &world.colliders,
            QueryFilter::default(),
        );
        queries
            .cast_ray(&Ray::new(origin, direction), max, true)
            .map(|(_, toi)| toi)
    }

    /// How many of a vehicle's wheels touch something.
    pub fn wheels_on_ground(&self, id: ObjectId) -> usize {
        self.bodies
            .get(&id)
            .and_then(|e| e.controller.as_ref())
            .map_or(0, |c| {
                c.wheels()
                    .iter()
                    .filter(|w| w.raycast_info().is_in_contact)
                    .count()
            })
    }

    /// Whether a body is resting on the ground (pinned to it until woken).
    pub fn at_rest(&self, id: ObjectId) -> bool {
        self.bodies.get(&id).is_some_and(|e| e.rest.is_some())
    }

    /// Keeps the ground body and its patches in step with the region's terrain, and asks for
    /// the patches every body near the ground needs.
    /// Keeps the ground body and its patches in step with the region's terrain.
    fn sync_ground(&mut self, env: &BodyEnv) {
        let terrain = env.terrain.as_ref().filter(|_| self.patches && env.solid);
        let current = self.ground.as_ref().map(|g| Arc::clone(g.terrain()));
        let same = match (&current, terrain) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        let extended = !same
            && match (&mut self.ground, terrain) {
                (Some(g), Some(t)) => g.extend_terrain(Arc::clone(t)),
                _ => false,
            };
        if !same {
            // The ground changed: everything resting on it settles again.
            for e in self.bodies.values_mut() {
                wake(&mut self.world, e, env.spin);
            }
        }
        if !same && !extended {
            self.stats.ground_rebuilds += 1;
            // A different terrain: rebuild every patch from it.
            if let Some(old) = self.ground.take() {
                old.remove(&mut self.world);
            }
            self.ground =
                terrain.map(|t| Ground::new(&mut self.world, Arc::clone(t), self.inline_patches));
        }
    }

    fn update_ground(&mut self, env: &BodyEnv) {
        self.sync_ground(env);
        let Some(ground) = &mut self.ground else {
            for e in self.bodies.values_mut() {
                e.on_patches = false;
            }
            return;
        };
        ground.place(&mut self.world, env.surface_rotation, env.spin);
        let now = self.time;
        let to_surface = env.surface_rotation.inverse();
        let highest = env.radius + env.max_ground_height();
        for e in self.bodies.values_mut() {
            // Kinematic movers (arcade flight, cruise) follow the ground their own way.
            if e.motion != Motion::Dynamic {
                e.on_patches = false;
                continue;
            }
            let rb = &self.world.bodies[e.handle];
            let p = rb.position().translation + *rb.rotation() * e.mass.center_of_mass;
            let speed = if e.rest.is_some() {
                0.0
            } else {
                (rb.linvel() - env.spin.cross(p)).length()
            };
            // Look ahead with the speed, within limits (a fast pass over the ground uses the
            // fallback contact rather than streaming hundreds of patches).
            let ahead = (speed * 1.5).min(PATCH_LOOKAHEAD);
            let reach = e.radius + PATCH_ALTITUDE + (speed * 3.0).min(PATCH_LOOKAHEAD * 2.0);
            if p.length() > highest + reach
                || p.length() - env.radius - env.ground_height(p) > reach
            {
                e.on_patches = false;
                continue;
            }
            let dir = to_surface * p.normalize();
            e.on_patches = ground.want(dir, e.radius + PATCH_MARGIN + ahead, e.radius + 2.0, now);
        }
        ground.update(&mut self.world, now);
    }

    /// Whether a body stood on collision patches in the last step (rather than the fallback
    /// contact model).
    pub fn on_patches(&self, id: ObjectId) -> bool {
        self.bodies.get(&id).is_some_and(|e| e.on_patches)
    }

    /// A body's centre-of-mass state after the last step, and whether it touched the ground.
    /// `spin` is the ground's angular velocity (resting bodies move with it).
    pub fn state(&self, id: ObjectId) -> Option<(RigidState, bool)> {
        let e = self.bodies.get(&id)?;
        let mut s = read(&self.world, e);
        if e.rest.is_some() {
            s.velocity = self.ground_spin.cross(s.position);
            s.angular_velocity = self.ground_spin;
        }
        Some((s, e.contact || e.rest.is_some()))
    }
}

/// Whether any of a body's colliders touches the ground body's patches.
fn touching(world: &PhysicsWorld, e: &Entry, ground: RigidBodyHandle) -> bool {
    e.collider_handles.iter().any(|&c| {
        world.narrow_phase.contact_pairs_with(c).any(|pair| {
            let other = if pair.collider1 == c {
                pair.collider2
            } else {
                pair.collider1
            };
            pair.has_any_active_contact()
                && world.colliders.get(other).and_then(|o| o.parent()) == Some(ground)
        })
    })
}

/// Collision groups of a vehicle collider: everything, or everything but the ground.
fn vehicle_groups(with_ground: bool) -> InteractionGroups {
    let filter = if with_ground {
        Group::ALL
    } else {
        Group::ALL.difference(GROUND_GROUP)
    };
    InteractionGroups::new(VEHICLE_GROUP, filter, InteractionTestMode::And)
}

fn set_ground_collisions(world: &mut PhysicsWorld, e: &Entry, on: bool) {
    for &c in &e.collider_handles {
        if let Some(collider) = world.colliders.get_mut(c) {
            collider.set_collision_groups(vehicle_groups(on));
        }
    }
}

/// Ends a body's rest: dynamic again, moving with the turning ground.
fn wake(world: &mut PhysicsWorld, e: &mut Entry, spin: DVec3) {
    e.still = 0.0;
    if e.rest.take().is_none() {
        return;
    }
    // Ground collisions back on whatever moves it next (a kinematic mover that later turns
    // dynamic again, e.g. after walking, would otherwise fall through the ground).
    set_ground_collisions(world, e, true);
    if e.motion != Motion::Dynamic {
        return;
    }
    let rb = &mut world.bodies[e.handle];
    let com = rb.position().translation + *rb.rotation() * e.mass.center_of_mass;
    rb.set_body_type(RigidBodyType::Dynamic, true);
    rb.set_linvel(spin.cross(com), true);
    rb.set_angvel(spin, true);
}

/// Whether a body is in contact with anything other than the ground (another vehicle).
fn touched_by_other(world: &PhysicsWorld, e: &Entry, ground: RigidBodyHandle) -> bool {
    e.collider_handles.iter().any(|&c| {
        world.narrow_phase.contact_pairs_with(c).any(|pair| {
            let other = if pair.collider1 == c {
                pair.collider2
            } else {
                pair.collider1
            };
            let parent = world.colliders.get(other).and_then(|o| o.parent());
            pair.has_any_active_contact() && parent.is_some_and(|p| p != ground && p != e.handle)
        })
    })
}

fn read(world: &PhysicsWorld, e: &Entry) -> RigidState {
    let rb = &world.bodies[e.handle];
    let rotation = *rb.rotation();
    RigidState {
        position: rb.position().translation + rotation * e.mass.center_of_mass,
        velocity: rb.linvel(),
        rotation: rotation.normalize(),
        angular_velocity: rb.angvel(),
    }
}

fn write(world: &mut PhysicsWorld, e: &Entry, state: &RigidState) {
    let rb = &mut world.bodies[e.handle];
    let com = e.mass.center_of_mass;
    let origin = state.position - state.rotation * com;
    rb.set_position(Pose::from_parts(origin, state.rotation), false);
    rb.set_linvel(state.velocity, false);
    rb.set_angvel(state.angular_velocity, false);
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::catalog::PartCatalog;
    use crate::design::VehicleDesign;
    use crate::vehicle::Vehicle;

    fn kestrel() -> VehicleBody {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../../content/data");
        let catalog = PartCatalog::load_dir(root.join("parts")).unwrap();
        let design =
            VehicleDesign::from_path(root.join("vehicles/kestrel.json"), &catalog).unwrap();
        VehicleBody::of(&Vehicle::from_design(&design)).unwrap()
    }

    /// Deep space: no gravity worth mentioning, no air, no ground.
    fn space() -> BodyEnv {
        BodyEnv {
            mu: 0.0,
            radius: 1.0,
            surface_reference_altitude: 0.0,
            spin: DVec3::ZERO,
            surface_rotation: DQuat::IDENTITY,
            solid: false,
            terrain: None,
            air_density: 0.0,
            air_scale_height: 1.0,
        }
    }

    fn at(position: DVec3, velocity: DVec3) -> RigidState {
        RigidState {
            position,
            velocity,
            rotation: DQuat::IDENTITY,
            angular_velocity: DVec3::ZERO,
        }
    }

    #[test]
    fn orbital_speeds_are_not_capped() {
        let body = kestrel();
        let mut scene = PhysicsScene::default();
        let v = DVec3::new(0.0, 0.0, 3000.0);
        let start = DVec3::new(1.0e6, 0.0, 0.0);
        scene.place_dynamic(ObjectId(1), &body, &at(start, v), Wrench::default());
        scene.step(&space(), 1.0);
        let (s, _) = scene.state(ObjectId(1)).unwrap();
        assert!((s.velocity - v).length() < 1e-9, "{:?}", s.velocity);
        assert!((s.position.z - 3000.0).abs() < 1e-6, "{:?}", s.position);
    }

    #[test]
    fn thrust_acts_from_the_first_step() {
        let body = kestrel();
        let mut scene = PhysicsScene::default();
        let push = Wrench {
            force: DVec3::new(0.0, 0.0, -body.mass.mass),
            torque: DVec3::ZERO,
        };
        let start = DVec3::new(1.0e6, 0.0, 0.0);
        scene.place_dynamic(ObjectId(1), &body, &at(start, DVec3::ZERO), push);
        scene.step(&space(), 1.0);
        let (s, _) = scene.state(ObjectId(1)).unwrap();
        // 1 m/s² forward (−Z) for 1 s.
        assert!((s.velocity - DVec3::new(0.0, 0.0, -1.0)).length() < 1e-9);
    }

    #[test]
    fn ships_collide_instead_of_passing_through() {
        let body = kestrel();
        let mut scene = PhysicsScene::default();
        let (a, b) = (ObjectId(1), ObjectId(2));
        let mut sa = at(DVec3::new(-30.0, 0.0, 0.0), DVec3::new(5.0, 0.0, 0.0));
        let mut sb = at(DVec3::new(30.0, 0.0, 0.0), DVec3::new(-5.0, 0.0, 0.0));
        let mut closest = f64::MAX;
        for _ in 0..(60 * 10) {
            scene.place_dynamic(a, &body, &sa, Wrench::default());
            scene.place_dynamic(b, &body, &sb, Wrench::default());
            scene.step(&space(), 1.0 / 60.0);
            sa = scene.state(a).unwrap().0;
            sb = scene.state(b).unwrap().0;
            closest = closest.min(sb.position.x - sa.position.x);
        }
        // Without collision they would have swapped sides 60 m apart.
        assert!(closest > 0.0, "passed through each other ({closest:.1} m)");
        assert!(sb.position.x > sa.position.x);
        assert!(
            sa.velocity.x < 1.0 && sb.velocity.x > -1.0,
            "bounced or stopped"
        );
    }

    #[test]
    fn bodies_not_placed_are_removed() {
        let body = kestrel();
        let mut scene = PhysicsScene::default();
        scene.place_dynamic(
            ObjectId(1),
            &body,
            &at(DVec3::ZERO, DVec3::ZERO),
            Wrench::default(),
        );
        scene.step(&space(), 0.1);
        assert_eq!(scene.len(), 1);
        scene.step(&space(), 0.1);
        assert!(scene.is_empty());
    }

    fn meridian(t: f64) -> BodyEnv {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../../content/data");
        let sys = sim_core::StarSystem::from_path(root.join("star_systems/halcyon.json")).unwrap();
        let i = sys.bodies.iter().position(|b| b.id == "meridian").unwrap();
        let terrain = sim_planet::Terrain::of(&sys.bodies[i]).map(Arc::new);
        crate::flight::region_env(&sys, i, t, Some(&crate::flight::RegionTerrain(terrain)))
    }

    #[test]
    fn lands_on_terrain_patches_and_turns_with_the_planet() {
        let body = kestrel();
        let mut env = meridian(0.0);
        let rotation0 = env.surface_rotation;
        // A gently sloped land spot.
        let up = sim_planet::fibonacci_directions(20_000)
            .map(|d| env.surface_rotation * d)
            .find(|&d| {
                let p = d * env.radius;
                let slope = env.ground_normal(p).dot(d).acos();
                env.ground_height(p) > 100.0 && (0.15..0.3).contains(&slope)
            })
            .unwrap();
        let ground = env.radius + env.ground_height(up * env.radius);
        // Touching down from 3 m, nearly at rest relative to the ground.
        let p = up * (ground + 5.0);
        let mut s = RigidState {
            position: p,
            velocity: env.spin.cross(p),
            rotation: DQuat::from_rotation_arc(DVec3::Y, up),
            angular_velocity: env.spin,
        };
        let mut scene = PhysicsScene::with_inline_patches();
        let id = ObjectId(1);
        let dt = 1.0 / 30.0;
        let ticks = 30 * 70;
        let settle = 30 * 10;
        let (mut settled_at, mut settled_t) = (DVec3::ZERO, 0.0);
        let mut touched = false;
        for tick in 0..ticks {
            let t = f64::from(tick) * dt;
            env.surface_rotation = (DQuat::from_scaled_axis(env.spin * t) * rotation0).normalize();
            scene.place_dynamic(id, &body, &s, Wrench::default());
            scene.step(&env, dt);
            let (next, contact) = scene.state(id).unwrap();
            s = next;
            touched |= contact;
            if tick == settle {
                settled_at = s.position.normalize();
                settled_t = t + dt;
            }
        }
        assert!(touched, "touched the ground");
        assert!(
            scene.on_patches(id),
            "stands on collision patches, not the fallback"
        );
        assert!(scene.patch_count() > 0);
        // The spot has turned with the planet.
        let turned = DQuat::from_scaled_axis(env.spin * (f64::from(ticks) * dt)) * up;
        let relative = (s.velocity - env.spin.cross(s.position)).length();
        assert!(
            relative < 0.05,
            "holds still on the slope: {relative:.3} m/s"
        );
        let drift = (s.position.normalize() - turned).length() * env.radius;
        assert!(drift < 2.0, "landed where it touched down: {drift:.2} m");
        // A minute standing on the slope after settling: no creep.
        let turned =
            DQuat::from_scaled_axis(env.spin * (f64::from(ticks) * dt - settled_t)) * settled_at;
        let creep = (s.position.normalize() - turned).length() * env.radius;
        assert!(creep < 0.05, "crept {creep:.3} m in a minute");
        assert!(scene.at_rest(id), "came to rest");
        // Resting on its hull: the centre of mass is a little above the ground, not sunk.
        let height = s.position.length() - (env.radius + env.ground_height(s.position));
        assert!(
            (0.3..5.0).contains(&height),
            "centre of mass {height:.2} m above ground"
        );

        // Flatten a pad under it, 2 m below the ground there: it keeps standing on patches
        // while the touched ones rebuild, then settles onto the pad.
        let terrain = env.terrain.clone().unwrap();
        let under = env.surface_rotation.inverse() * s.position.normalize();
        let pad_height = terrain.surface_height(under, 1.0) - 2.0;
        env.terrain = Some(Arc::new(terrain.with_pad(sim_planet::Pad {
            center: under,
            radius: 30.0,
            blend: 20.0,
            height: pad_height,
        })));
        for tick in ticks..ticks + 30 * 5 {
            let t = f64::from(tick) * dt;
            env.surface_rotation = (DQuat::from_scaled_axis(env.spin * t) * rotation0).normalize();
            scene.place_dynamic(id, &body, &s, Wrench::default());
            scene.step(&env, dt);
            s = scene.state(id).unwrap().0;
            assert!(scene.on_patches(id), "on patches through the change");
        }
        let after = s.position.length() - (env.radius + pad_height);
        assert!(
            (after - height).abs() < 0.3,
            "rests on the pad as before: {after:.2} vs {height:.2} m"
        );

        // Resting again, then its joint poses change: it wakes to settle on them.
        let mut tick = ticks + 30 * 5;
        while !scene.at_rest(id) && tick < ticks + 30 * 20 {
            let t = f64::from(tick) * dt;
            env.surface_rotation = (DQuat::from_scaled_axis(env.spin * t) * rotation0).normalize();
            scene.place_dynamic(id, &body, &s, Wrench::default());
            scene.step(&env, dt);
            s = scene.state(id).unwrap().0;
            tick += 1;
        }
        assert!(scene.at_rest(id), "rests on the pad");
        scene.set_poses(id, &[crate::joints::Pose::IDENTITY], 1);
        scene.place_dynamic(id, &body, &s, Wrench::default());
        scene.step(&env, dt);
        assert!(!scene.at_rest(id), "woken by the pose change");

        // Rest again, then moved kinematically (walking) and handed back: it stands on the
        // ground rather than falling through it.
        let mut tick = tick + 1;
        let start = tick;
        while !scene.at_rest(id) && tick < start + 30 * 20 {
            let t = f64::from(tick) * dt;
            env.surface_rotation = (DQuat::from_scaled_axis(env.spin * t) * rotation0).normalize();
            scene.place_dynamic(id, &body, &s, Wrench::default());
            scene.step(&env, dt);
            s = scene.state(id).unwrap().0;
            tick += 1;
        }
        assert!(scene.at_rest(id), "rests again");
        for _ in 0..30 {
            let t = f64::from(tick) * dt;
            env.surface_rotation = (DQuat::from_scaled_axis(env.spin * t) * rotation0).normalize();
            scene.place_kinematic(id, &body, &s);
            scene.step(&env, dt);
            s.position = DQuat::from_scaled_axis(env.spin * dt) * s.position;
            s.velocity = env.spin.cross(s.position);
            tick += 1;
        }
        let before = s.position.length() - (env.radius + env.ground_height(s.position));
        for _ in 0..(30 * 3) {
            let t = f64::from(tick) * dt;
            env.surface_rotation = (DQuat::from_scaled_axis(env.spin * t) * rotation0).normalize();
            scene.place_dynamic(id, &body, &s, Wrench::default());
            scene.step(&env, dt);
            s = scene.state(id).unwrap().0;
            tick += 1;
        }
        let after = s.position.length() - (env.radius + env.ground_height(s.position));
        assert!(
            (after - before).abs() < 0.5,
            "stands on the ground after moving kinematically: {before:.2} -> {after:.2} m"
        );
    }
}
