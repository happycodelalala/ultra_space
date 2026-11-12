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
use rapier3d_f64::prelude::{
    ColliderBuilder, ColliderHandle, MassProperties as RapierMass, PhysicsWorld, Pose,
    RigidBodyBuilder, RigidBodyHandle, RigidBodyType,
};
use sim_core::ObjectId;

use crate::allocation::Wrench;
use crate::catalog::Shape;
use crate::flight::{BodyEnv, MAX_SUBSTEP, MAX_SUBSTEPS, RigidState, ground_contact};
use crate::ground::Ground;
use crate::vehicle::{PartCollider, VehicleBody};

/// Hull friction (combined with the other collider's by averaging).
const HULL_FRICTION: f64 = 0.8;
/// Bodies closer to the ground than their radius plus this (m), plus a few seconds of travel,
/// get collision patches.
const PATCH_ALTITUDE: f64 = 50.0;
/// Solver iterations per step while bodies are near the ground.
const CONTACT_ITERATIONS: usize = 4;
/// A body still on the ground (slower than these, relative to it) with no thrust for
/// `REST_AFTER` seconds comes to rest: pinned to the turning ground until something wakes it.
const REST_SPEED: f64 = 0.05;
const REST_SPIN: f64 = 0.01;
const REST_AFTER: f64 = 1.0;
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
    /// What the body was built from, to rebuild when the vehicle's parts change.
    mass: crate::mass::MassProperties,
    colliders: Vec<PartCollider>,
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
    /// At rest: its pose in the ground's (surface) frame.
    rest: Option<Pose>,
}

/// A region's rapier world and its vehicles' bodies.
pub struct PhysicsScene {
    world: PhysicsWorld,
    bodies: HashMap<ObjectId, Entry>,
    ground: Option<Ground>,
    /// Whether terrain gets collision patches (single-vehicle helpers use the contact model).
    patches: bool,
    /// Build patches on the calling thread (tests).
    inline_patches: bool,
    /// Time stepped so far (s), for patch lifetimes.
    time: f64,
    /// The ground's angular velocity in the last step.
    ground_spin: DVec3,
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
            ground: None,
            patches: true,
            inline_patches: false,
            time: 0.0,
            ground_spin: DVec3::ZERO,
        }
    }
}

/// A region's physics scene (created by the flight system on first use).
#[derive(Resource, Default)]
pub struct RegionPhysics(pub PhysicsScene);

fn collider_for(part: &PartCollider) -> ColliderBuilder {
    let builder = match part.shape {
        Shape::Box { size_m } => {
            ColliderBuilder::cuboid(size_m[0] / 2.0, size_m[1] / 2.0, size_m[2] / 2.0)
        }
        Shape::Cylinder { radius_m, length_m } => {
            ColliderBuilder::cylinder(length_m / 2.0, radius_m)
        }
        Shape::Sphere { radius_m } => ColliderBuilder::ball(radius_m),
    };
    builder
        .position(Pose::from_parts(part.position, part.rotation))
        // Mass and inertia come from our own mass properties.
        .density(0.0)
        .friction(HULL_FRICTION)
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
        let stale = self
            .bodies
            .get(&id)
            .is_some_and(|e| e.mass != body.mass || e.colliders != body.colliders);
        if stale && let Some(old) = self.bodies.remove(&id) {
            self.world.remove_body(old.handle);
        }
        let world = &mut self.world;
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
            let collider_handles = body
                .colliders
                .iter()
                .map(|part| world.insert_collider(collider_for(part), Some(handle)))
                .collect();
            Entry {
                handle,
                collider_handles,
                mass: body.mass,
                colliders: body.colliders.clone(),
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
        if self.bodies.is_empty() || dt <= 0.0 {
            return;
        }
        self.ground_spin = env.spin;
        self.update_ground(env);
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
            if let Some(ground) = &self.ground {
                let turned = DQuat::from_scaled_axis(env.spin * (h * f64::from(k + 1)));
                let rotation = (turned * env.surface_rotation).normalize();
                ground.turn_to(&mut self.world, rotation);
                // Resting bodies turn with it.
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
            self.world.step();
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
        for e in self.bodies.values_mut() {
            e.placed = false;
        }
    }

    /// Wakes resting bodies that thrust, left the patches or were hit, and pins the others to
    /// the ground's current rotation.
    fn update_rest(&mut self, env: &BodyEnv) {
        let ground = self.ground.as_ref().map(|g| g.body);
        for e in self.bodies.values_mut() {
            let Some(local) = e.rest else { continue };
            let thrusting = e.wrench.force != DVec3::ZERO || e.wrench.torque != DVec3::ZERO;
            let hit = ground.is_some_and(|g| touched_by_other(&self.world, e, g));
            if thrusting || e.motion != Motion::Dynamic || !e.on_patches || hit {
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
            let idle = e.wrench.force == DVec3::ZERO && e.wrench.torque == DVec3::ZERO;
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
            }
        }
    }

    /// Whether a body is resting on the ground (pinned to it until woken).
    pub fn at_rest(&self, id: ObjectId) -> bool {
        self.bodies.get(&id).is_some_and(|e| e.rest.is_some())
    }

    /// Keeps the ground body and its patches in step with the region's terrain, and asks for
    /// the patches every body near the ground needs.
    fn update_ground(&mut self, env: &BodyEnv) {
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
            // A different terrain: rebuild every patch from it.
            if let Some(old) = self.ground.take() {
                old.remove(&mut self.world);
            }
            self.ground =
                terrain.map(|t| Ground::new(&mut self.world, Arc::clone(t), self.inline_patches));
        }
        let Some(ground) = &mut self.ground else {
            for e in self.bodies.values_mut() {
                e.on_patches = false;
            }
            return;
        };
        ground.place(&mut self.world, env.surface_rotation);
        let now = self.time;
        let to_surface = env.surface_rotation.inverse();
        let highest = env.radius + env.max_ground_height();
        for e in self.bodies.values_mut() {
            let rb = &self.world.bodies[e.handle];
            let p = rb.position().translation + *rb.rotation() * e.mass.center_of_mass;
            let speed = if e.rest.is_some() {
                0.0
            } else {
                (rb.linvel() - env.spin.cross(p)).length()
            };
            let reach = e.radius + PATCH_ALTITUDE + speed * 3.0;
            if p.length() > highest + reach
                || p.length() - env.radius - env.ground_height(p) > reach
            {
                e.on_patches = false;
                continue;
            }
            let dir = to_surface * p.normalize();
            e.on_patches = ground.want(
                dir,
                e.radius + PATCH_MARGIN + speed * 1.5,
                e.radius + 2.0,
                now,
            );
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

/// Ends a body's rest: dynamic again, moving with the turning ground.
fn wake(world: &mut PhysicsWorld, e: &mut Entry, spin: DVec3) {
    e.still = 0.0;
    if e.rest.take().is_none() || e.motion != Motion::Dynamic {
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
    }
}
