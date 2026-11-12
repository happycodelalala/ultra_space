//! Task 3.5: a rover drives over Meridian's terrain on its wheels, steers and brakes, without
//! tipping over or sinking into the ground.

use std::path::PathBuf;
use std::sync::Arc;

use glam::{DQuat, DVec3};
use sim_core::ObjectId;
use sim_vehicle::flight::{BodyEnv, RegionTerrain, RigidState, region_env};
use sim_vehicle::{Drive, PartCatalog, PhysicsScene, Vehicle, VehicleBody, VehicleDesign, Wrench};

fn content() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../../content/data")
}

fn rover() -> VehicleBody {
    let catalog = PartCatalog::load_dir(content().join("parts")).unwrap();
    let design = VehicleDesign::from_path(content().join("vehicles/rover.json"), &catalog).unwrap();
    VehicleBody::of(&Vehicle::from_design(&design)).unwrap()
}

struct Drive_ {
    scene: PhysicsScene,
    env: BodyEnv,
    rotation0: DQuat,
    body: VehicleBody,
    state: RigidState,
    tick: u32,
    touching: bool,
}

const DT: f64 = 1.0 / 30.0;

impl Drive_ {
    /// The rover set down on gentle land, facing along the ground.
    fn new() -> Self {
        let sys =
            sim_core::StarSystem::from_path(content().join("star_systems/halcyon.json")).unwrap();
        let i = sys.bodies.iter().position(|b| b.id == "meridian").unwrap();
        let terrain = sim_planet::Terrain::of(&sys.bodies[i]).map(Arc::new);
        let env = region_env(&sys, i, 0.0, Some(&RegionTerrain(terrain)));
        let up = sim_planet::fibonacci_directions(20_000)
            .map(|d| env.surface_rotation * d)
            .find(|&d| {
                let p = d * env.radius;
                env.ground_height(p) > 100.0 && env.ground_normal(p).dot(d) > 0.995
            })
            .unwrap();
        let ground = env.radius + env.ground_height(up * env.radius);
        let body = rover();
        let p = up * (ground + 1.5);
        let north = up.cross(DVec3::Y.cross(up)).normalize();
        let rotation = DQuat::from_mat3(&glam::DMat3::from_cols(north.cross(up), up, -north));
        let state = RigidState {
            position: p + rotation * body.mass.center_of_mass,
            velocity: env.spin.cross(p),
            rotation,
            angular_velocity: env.spin,
        };
        Self {
            scene: PhysicsScene::with_inline_patches(),
            rotation0: env.surface_rotation,
            env,
            body,
            state,
            tick: 0,
            touching: false,
        }
    }

    fn run(&mut self, seconds: f64, drive: Drive) {
        for _ in 0..(seconds / DT) as u32 {
            let t = f64::from(self.tick) * DT;
            self.env.surface_rotation =
                (DQuat::from_scaled_axis(self.env.spin * t) * self.rotation0).normalize();
            self.scene.place_driven(
                ObjectId(1),
                &self.body,
                &self.state,
                Wrench::default(),
                drive,
            );
            self.scene.step(&self.env, DT);
            let (s, contact) = self.scene.state(ObjectId(1)).unwrap();
            self.state = s;
            self.touching = contact;
            self.tick += 1;
            let up = self.state.position.normalize();
            assert!(
                (self.state.rotation * DVec3::Y).dot(up) > 0.7,
                "tipped over at {:.1} s",
                t
            );
            let clearance = self.state.position.length()
                - (self.env.radius + self.env.ground_height(self.state.position));
            assert!(
                clearance > 0.0,
                "sank into the ground at {t:.1} s ({clearance:.2} m)"
            );
        }
    }

    /// Ground-relative velocity.
    fn relative(&self) -> DVec3 {
        self.state.velocity - self.env.spin.cross(self.state.position)
    }

    /// Where the rover is, as a direction in the surface frame (fixed to the ground).
    fn spot(&self) -> DVec3 {
        self.env.surface_rotation.inverse() * self.state.position.normalize()
    }
}

#[test]
fn rover_drives_steers_and_brakes() {
    let mut r = Drive_::new();
    r.run(2.0, Drive::default());
    // Parked (idle controls): the parking brake holds it on the gentle slope.
    let parked_at = r.spot();
    r.run(3.0, Drive::default());
    let rolled = (r.spot() - parked_at).length() * r.env.radius;
    assert!(rolled < 0.1, "rolled {rolled:.2} m while parked");
    assert!(r.touching, "settled on its wheels");
    let start = r.spot();

    // Full throttle for 10 s: forward (−Z), at a sensible speed.
    r.run(
        10.0,
        Drive {
            throttle: 1.0,
            ..Default::default()
        },
    );
    let forward = r.state.rotation * DVec3::NEG_Z;
    let speed = r.relative().dot(forward);
    let travelled = (r.spot() - start).length() * r.env.radius;
    assert!(speed > 5.0 && speed < 40.0, "speed {speed:.1} m/s");
    assert!(travelled > 30.0, "travelled {travelled:.1} m");

    // Steering right turns the nose right.
    let heading_before = forward;
    r.run(
        1.0,
        Drive {
            throttle: 0.5,
            steer: 1.0,
            brake: false,
        },
    );
    let up = r.state.position.normalize();
    let after = r.state.rotation * DVec3::NEG_Z;
    let turn = heading_before.cross(after).dot(up);
    assert!(turn < -0.1, "turned right (signed turn {turn:.2})");

    // Brake: stops.
    r.run(
        6.0,
        Drive {
            brake: true,
            ..Default::default()
        },
    );
    let left = r.relative().length();
    assert!(left < 0.5, "stopped: {left:.2} m/s");
}
