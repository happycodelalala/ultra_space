//! Rigid-body physics with rapier (Batch 3, task 3.1; design: vehicle-builder.md §2).
//!
//! Each region owns one [`PhysicsScene`]: a rapier world in the region body's inertial frame.
//! Every tick the region's flight code writes each vehicle's state and its control wrench into
//! the scene, the scene steps all vehicles together (gravity and drag re-applied every
//! substep), and the states are read back. The object state stays the authority between ticks,
//! so region transfers, teleports and other movers need no extra bookkeeping: a body whose object
//! wasn't placed this tick is removed.
//!
//! Until terrain colliders exist (task 3.2), ground contact is applied after each substep with
//! the Batch 2 contact model.

use std::collections::HashMap;

use bevy_ecs::prelude::*;
use glam::DVec3;
use rapier3d_f64::prelude::{
    ColliderBuilder, MassProperties as RapierMass, PhysicsWorld, Pose, RigidBodyBuilder,
    RigidBodyHandle, RigidBodyType,
};
use sim_core::ObjectId;

use crate::allocation::Wrench;
use crate::catalog::Shape;
use crate::flight::{BodyEnv, MAX_SUBSTEP, MAX_SUBSTEPS, RigidState, ground_contact};
use crate::vehicle::{PartCollider, VehicleBody};

/// Hull friction against other colliders (ground friction is the contact model's until 3.2).
const HULL_FRICTION: f64 = 0.8;
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
    /// What the body was built from, to rebuild when the vehicle's parts change.
    mass: crate::mass::MassProperties,
    colliders: Vec<PartCollider>,
    /// Radius of a sphere around the centre of mass containing the vehicle (m).
    radius: f64,
    wrench: Wrench,
    motion: Motion,
    placed: bool,
    contact: bool,
}

/// A region's rapier world and its vehicles' bodies.
pub struct PhysicsScene {
    world: PhysicsWorld,
    bodies: HashMap<ObjectId, Entry>,
}

impl Default for PhysicsScene {
    fn default() -> Self {
        let mut world = PhysicsWorld::new();
        // Gravity points to the body's centre; it's applied per body.
        world.gravity = DVec3::ZERO;
        // Rapier caps speeds at 400 m/s by default; orbits need thousands.
        world.integration_parameters.normalized_max_linear_velocity = f64::MAX;
        // One solver substep per step: gravity (applied per body) is re-evaluated every step,
        // which keeps orbits symplectic. We substep the steps ourselves.
        world.integration_parameters.num_solver_iterations = 1;
        Self {
            world,
            bodies: HashMap::new(),
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
            for part in &body.colliders {
                world.insert_collider(collider_for(part), Some(handle));
            }
            Entry {
                handle,
                mass: body.mass,
                colliders: body.colliders.clone(),
                radius: vehicle_radius(body),
                wrench,
                motion,
                placed: true,
                contact: false,
            }
        });
        entry.wrench = wrench;
        entry.motion = motion;
        entry.placed = true;
        entry.contact = false;
        let com = body.mass.center_of_mass;
        let rb = &mut world.bodies[entry.handle];
        let kind = match motion {
            Motion::Dynamic => RigidBodyType::Dynamic,
            Motion::Kinematic => RigidBodyType::KinematicPositionBased,
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
        let substeps = (dt / MAX_SUBSTEP)
            .ceil()
            .clamp(1.0, f64::from(MAX_SUBSTEPS)) as u32;
        let h = dt / f64::from(substeps);
        self.world.integration_parameters.dt = h;
        for _ in 0..substeps {
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
            for e in self.bodies.values_mut() {
                if e.motion != Motion::Dynamic {
                    continue;
                }
                let mut state = read(&self.world, e);
                if ground_contact(&mut state, env, e.radius, h) {
                    e.contact = true;
                    write(&mut self.world, e, &state);
                }
            }
        }
        for e in self.bodies.values_mut() {
            e.placed = false;
        }
    }

    /// A body's centre-of-mass state after the last step, and whether it touched the ground.
    pub fn state(&self, id: ObjectId) -> Option<(RigidState, bool)> {
        let e = self.bodies.get(&id)?;
        Some((read(&self.world, e), e.contact))
    }
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

    use glam::DQuat;

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
}
