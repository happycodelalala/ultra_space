//! Task 3.6: the walker moves against the physics world: it walks on the ground patches and
//! is stopped by vehicles instead of walking through them.

use std::path::PathBuf;
use std::sync::Arc;

use glam::{DMat3, DQuat, DVec2, DVec3};
use sim_core::ObjectId;
use sim_core::frames::FrameState;
use sim_vehicle::flight::{BodyEnv, RegionTerrain, RigidState, region_env};
use sim_vehicle::walker::{ride, walk_step};
use sim_vehicle::{
    PartCatalog, PhysicsScene, Vehicle, VehicleBody, VehicleDesign, Walker, WalkerControl, Wrench,
};

/// The simulation tick.
const DT: f64 = 1.0 / 30.0;

fn content() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../../content/data")
}

fn meridian() -> BodyEnv {
    let sys = sim_core::StarSystem::from_path(content().join("star_systems/halcyon.json")).unwrap();
    let i = sys.bodies.iter().position(|b| b.id == "meridian").unwrap();
    let terrain = sim_planet::Terrain::of(&sys.bodies[i]).map(Arc::new);
    region_env(&sys, i, 0.0, Some(&RegionTerrain(terrain)))
}

/// A gently sloped land spot (inertial direction) and a level frame there facing "north".
fn spot(env: &BodyEnv) -> (DVec3, DQuat) {
    let up = sim_planet::fibonacci_directions(20_000)
        .map(|d| env.surface_rotation * d)
        .find(|&d| {
            let p = d * env.radius;
            env.ground_height(p) > 100.0 && env.ground_normal(p).dot(d) > 0.99
        })
        .unwrap();
    let north = up.cross(DVec3::Y.cross(up)).normalize();
    (
        up,
        DQuat::from_mat3(&DMat3::from_cols(north.cross(up), up, -north)),
    )
}

struct World {
    env: BodyEnv,
    rotation0: DQuat,
    scene: PhysicsScene,
    tick: u32,
}

impl World {
    fn new() -> Self {
        let env = meridian();
        Self {
            rotation0: env.surface_rotation,
            env,
            scene: PhysicsScene::with_inline_patches(),
            tick: 0,
        }
    }

    /// One tick: the walker first (against the world as the last step left it), then the
    /// vehicles (if any) and the ground step.
    fn tick(
        &mut self,
        walker: (&mut FrameState, &mut Walker, &mut WalkerControl),
        ship: Option<(&VehicleBody, &mut RigidState)>,
    ) {
        let t = f64::from(self.tick) * DT;
        self.env.surface_rotation =
            (DQuat::from_scaled_axis(self.env.spin * t) * self.rotation0).normalize();
        let (s, w, c) = walker;
        // Like the game: a 1/30 s tick in two walker steps against the vehicles as the last
        // physics step left them, then one turn with the planet.
        let scene = &self.scene;
        for _ in 0..2 {
            let mut sweep = |feet, up, desired| scene.sweep_character(feet, up, desired, DT / 2.0);
            walk_step(s, w, c, &self.env, DT / 2.0, Some(&mut sweep));
        }
        ride(s, w, &self.env, DT);
        if let Some((body, state)) = ship {
            self.scene
                .place_dynamic(ObjectId(1), body, state, Wrench::default());
        }
        self.scene.step(&self.env, DT);
        self.tick += 1;
    }
}

fn ship_state(scene: &PhysicsScene) -> Option<(RigidState, bool)> {
    scene.state(ObjectId(1))
}

fn standing(env: &BodyEnv, up: DVec3, frame: DQuat, offset: DVec3) -> FrameState {
    let p = (up * env.radius + frame * offset).normalize();
    let feet = p * (env.radius + env.ground_height(p * env.radius) + 0.05);
    FrameState {
        position: feet,
        rotation: frame,
        velocity: env.spin.cross(feet),
        angular_velocity: env.spin,
    }
}

#[test]
fn walks_over_the_ground_with_a_scene() {
    let mut world = World::new();
    let (up, frame) = spot(&world.env);
    let start = standing(&world.env, up, frame, DVec3::ZERO).position;
    let mut s = standing(&world.env, up, frame, DVec3::ZERO);
    let mut w = Walker {
        grounded: true,
        ..Default::default()
    };
    let mut c = WalkerControl::default();
    let seconds = 20.0;
    for _ in 0..(30.0 * seconds) as u32 {
        c.movement = DVec2::new(0.0, 1.0);
        c.run = true;
        world.tick((&mut s, &mut w, &mut c), None);
        let feet = s.position.length() - (world.env.radius + world.env.ground_height(s.position));
        // (Sampled with the tick's starting rotation, about a metre off along the planet's
        // turn: a few centimetres of slack.)
        assert!(
            feet > -0.05 && feet < 0.7,
            "feet {feet:.2} m from the ground"
        );
    }
    // Running at 12 m/s, compared along the ground (the planet has turned meanwhile).
    let turned = DQuat::from_scaled_axis(world.env.spin * seconds) * start;
    let travelled = (s.position.normalize() - turned.normalize()).length() * world.env.radius;
    assert!((200.0..260.0).contains(&travelled), "ran {travelled:.0} m");
    assert!(w.grounded);
}

#[test]
fn a_parked_ship_blocks_the_way() {
    let catalog = PartCatalog::load_dir(content().join("parts")).unwrap();
    let design =
        VehicleDesign::from_path(content().join("vehicles/kestrel.json"), &catalog).unwrap();
    let body = VehicleBody::of(&Vehicle::from_design(&design)).unwrap();
    let mut world = World::new();
    let (up, frame) = spot(&world.env);
    // The ship 12 m ahead (north), side-on, settled on its legs.
    let ahead = frame * DVec3::new(0.0, 0.0, -12.0);
    let ship_spot = (up * world.env.radius + ahead).normalize();
    let ship_ground = world.env.radius + world.env.ground_height(ship_spot * world.env.radius);
    let ship_rotation = (DQuat::from_rotation_arc(DVec3::Y, ship_spot)
        * DQuat::from_rotation_y(std::f64::consts::FRAC_PI_2))
    .normalize();
    let p = ship_spot * (ship_ground + 3.0);
    let mut ship = RigidState {
        position: p,
        velocity: world.env.spin.cross(p),
        rotation: ship_rotation,
        angular_velocity: world.env.spin,
    };
    let mut s = standing(&world.env, up, frame, DVec3::ZERO);
    let mut w = Walker {
        grounded: true,
        ..Default::default()
    };
    let mut c = WalkerControl::default();
    for _ in 0..(30 * 4) {
        world.tick((&mut s, &mut w, &mut c), Some((&body, &mut ship)));
        ship = ship_state(&world.scene).unwrap().0;
    }
    // Walk at it for 6 s (it would be 60 m on at a run).
    let mut closest = f64::MAX;
    let mut max_speed: f64 = 0.0;
    for _ in 0..(30 * 6) {
        c.movement = DVec2::new(0.0, 1.0);
        c.run = true;
        world.tick((&mut s, &mut w, &mut c), Some((&body, &mut ship)));
        ship = ship_state(&world.scene).unwrap().0;
        let to_ship = ship.position - s.position;
        let level = to_ship - s.position.normalize() * to_ship.dot(s.position.normalize());
        closest = closest.min(level.length());
        let speed = (s.velocity - world.env.spin.cross(s.position)).length();
        max_speed = max_speed.max(speed);
    }
    assert!(max_speed < 13.0, "flung at {max_speed:.0} m/s");
    // Reached the ship (12 m away) and stopped at its hull (half-width 1.5 m plus the
    // capsule), not walked through.
    assert!(closest < 3.0, "never reached the ship ({closest:.2} m)");
    assert!(
        closest > 1.4,
        "got within {closest:.2} m of the ship's centre line"
    );
    let forward = s.rotation * DVec3::NEG_Z;
    let past = (s.position - ship.position).dot(forward);
    assert!(past < 0.0, "ended up past the ship ({past:.1} m)");
}

/// Strafing into the ship's side anywhere along its length (legs, thrusters, hull) never throws
/// the walker.
#[test]
fn bumping_the_ship_anywhere_never_throws_the_walker() {
    let catalog = PartCatalog::load_dir(content().join("parts")).unwrap();
    let design =
        VehicleDesign::from_path(content().join("vehicles/kestrel.json"), &catalog).unwrap();
    let body = VehicleBody::of(&Vehicle::from_design(&design)).unwrap();
    for along in [-5.0, -3.5, -2.4, -1.0, 0.0, 1.5, 2.4, 3.5, 5.0] {
        let mut world = World::new();
        let (up, frame) = spot(&world.env);
        let ship_spot = up;
        let ship_ground = world.env.radius + world.env.ground_height(ship_spot * world.env.radius);
        let p = ship_spot * (ship_ground + 3.0);
        let mut ship = RigidState {
            position: p,
            velocity: world.env.spin.cross(p),
            rotation: frame,
            angular_velocity: world.env.spin,
        };
        // The walker 6 m to the ship's right, `along` metres along it, facing the same way.
        let mut s = standing(&world.env, up, frame, DVec3::new(6.0, 0.0, along));
        let mut w = Walker {
            grounded: true,
            ..Default::default()
        };
        let mut c = WalkerControl::default();
        for _ in 0..(30 * 4) {
            world.tick((&mut s, &mut w, &mut c), Some((&body, &mut ship)));
            ship = ship_state(&world.scene).unwrap().0;
        }
        let mut fastest: f64 = 0.0;
        for _ in 0..(30 * 3) {
            c.movement = DVec2::new(-1.0, 0.0); // strafe left, into the ship
            world.tick((&mut s, &mut w, &mut c), Some((&body, &mut ship)));
            ship = ship_state(&world.scene).unwrap().0;
            let rel = s.velocity - world.env.spin.cross(s.position);
            fastest = fastest.max(rel.length());
        }
        assert!(
            fastest < 6.0,
            "thrown at {fastest:.0} m/s when bumping the ship {along} m along"
        );
    }
}
