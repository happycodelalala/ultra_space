//! Task 4.5: buildings stop vehicles and walkers. A building (foundation and collision box, as a
//! settlement layout makes them) stands 30 m north of a rover and a walker on Meridian; both
//! head straight for it.

use std::path::PathBuf;
use std::sync::Arc;

use glam::{DMat3, DQuat, DVec2, DVec3};
use sim_core::ObjectId;
use sim_core::frames::FrameState;
use sim_planet::{Footprint, Foundations, PlacedBox, Site, Terrain, tangent_frame};
use sim_vehicle::flight::{BodyEnv, RegionTerrain, RigidState, region_env};
use sim_vehicle::walker::{keep_out_of_buildings, walk_step};
use sim_vehicle::{
    Drive, PartCatalog, PhysicsScene, Vehicle, VehicleBody, VehicleDesign, Walker, WalkerControl,
    Wrench,
};

const DT: f64 = 1.0 / 30.0;
/// The building: centre 30 m north of the start, 16 m wide (east–west), 6 m deep, 6 m tall.
const AT: DVec2 = DVec2::new(0.0, -30.0);
const HALF: DVec3 = DVec3::new(8.0, 3.0, 3.0);

fn content() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../../content/data")
}

struct Scene {
    env: BodyEnv,
    rotation0: DQuat,
    /// Start: inertial direction at time 0, its surface-frame direction, level rotation facing
    /// north.
    up: DVec3,
    site: DVec3,
    facing: DQuat,
    /// Height of the building's floor (m above the reference radius).
    floor: f64,
}

impl Scene {
    fn new() -> Self {
        let sys =
            sim_core::StarSystem::from_path(content().join("star_systems/halcyon.json")).unwrap();
        let i = sys.bodies.iter().position(|b| b.id == "meridian").unwrap();
        let natural = Terrain::of(&sys.bodies[i]).unwrap();
        let bare = region_env(&sys, i, 0.0, None);
        // A gentle land spot (surface frame).
        let site = sim_planet::fibonacci_directions(20_000)
            .find(|&d| natural.height(d, 1.0) > 100.0 && natural.slope(d, 20.0, 1.0) < 0.05)
            .unwrap();
        let (east, south) = tangent_frame(site);
        let r = natural.radius;
        let at = (site * r + east * AT.x + south * AT.y).normalize();
        let floor = natural.height(at, 1.0);
        let foundation = Site::with_boxes(
            site,
            vec![Footprint {
                x: AT.x,
                z: AT.y,
                yaw: 0.0,
                half: [HALF.x + 1.0, HALF.z + 1.0],
                height: floor,
            }],
            vec![PlacedBox {
                center: DVec3::new(AT.x, floor + HALF.y, AT.y),
                yaw: 0.0,
                half: HALF,
            }],
            4.0,
            r,
        );
        let terrain = natural.with_foundations(Arc::new(Foundations::new(vec![foundation])));
        let env = region_env(&sys, i, 0.0, Some(&RegionTerrain(Some(Arc::new(terrain)))));
        let up = bare.surface_rotation * site;
        // Level, facing north (the surface frame's: +Y is the spin axis).
        let north = -south;
        let level = DQuat::from_mat3(&DMat3::from_cols(east, site, -north));
        Self {
            rotation0: env.surface_rotation,
            env,
            up,
            site,
            facing: (bare.surface_rotation * level).normalize(),
            floor,
        }
    }

    fn at_time(&mut self, t: f64) {
        self.env.surface_rotation =
            (DQuat::from_scaled_axis(self.env.spin * t) * self.rotation0).normalize();
    }

    /// Where inertial point `p` is relative to the building: (x east, height above its floor,
    /// z south) from the box's centre.
    fn in_building(&self, p: DVec3) -> DVec3 {
        let s = self.env.surface_rotation.inverse() * p;
        let (east, south) = tangent_frame(self.site);
        let r = self.env.radius;
        let centre = (self.site * r + east * AT.x + south * AT.y).normalize();
        let d = s.normalize() * r - centre * r;
        DVec3::new(d.dot(east), s.length() - r - self.floor, d.dot(south))
    }
}

#[test]
fn a_rover_stops_at_a_wall() {
    let mut w = Scene::new();
    let catalog = PartCatalog::load_dir(content().join("parts")).unwrap();
    let design = VehicleDesign::from_path(content().join("vehicles/rover.json"), &catalog).unwrap();
    let body = VehicleBody::of(&Vehicle::from_design(&design)).unwrap();
    let ground = w.env.radius + w.env.ground_height(w.up * w.env.radius);
    let p = w.up * (ground + 1.5);
    let mut state = RigidState {
        position: p + w.facing * body.mass.center_of_mass,
        velocity: w.env.spin.cross(p),
        rotation: w.facing,
        angular_velocity: w.env.spin,
    };
    let mut scene = PhysicsScene::with_inline_patches();
    let mut closest = f64::MAX;
    let mut buildings = 0;
    for tick in 0..(14.0 / DT) as u32 {
        w.at_time(f64::from(tick) * DT);
        let drive = Drive {
            throttle: if tick as f64 * DT > 2.0 { 1.0 } else { 0.0 },
            ..Default::default()
        };
        scene.place_driven(ObjectId(1), &body, &state, Wrench::default(), drive);
        scene.step(&w.env, DT);
        state = scene.state(ObjectId(1)).unwrap().0;
        buildings = buildings.max(scene.stats().buildings);
        let b = w.in_building(state.position);
        // Its centre never gets inside the wall (the rover is about 2 m from centre to bumper).
        assert!(
            b.z > HALF.z || b.x.abs() > HALF.x,
            "drove into the building: {b:?} at {:.1} s",
            f64::from(tick) * DT
        );
        closest = closest.min(b.z - HALF.z);
    }
    assert_eq!(buildings, 1, "the building's collider was streamed in");
    // It got there (full throttle covers 30 m in the time) and stopped at the wall.
    assert!(
        closest < 3.0,
        "never reached the wall (closest {closest:.1} m)"
    );
}

#[test]
fn a_walker_stops_at_a_wall_and_slides_along_it() {
    let mut w = Scene::new();
    let ground = w.env.radius + w.env.ground_height(w.up * w.env.radius);
    let p = w.up * ground;
    let mut state = FrameState {
        position: p,
        rotation: w.facing,
        velocity: w.env.spin.cross(p),
        angular_velocity: w.env.spin,
    };
    let mut walker = Walker {
        grounded: true,
        ..Default::default()
    };
    let mut control = WalkerControl::default();
    let mut closest = f64::MAX;
    for tick in 0..(20.0 / DT) as u32 {
        let t = f64::from(tick) * DT;
        w.at_time(t);
        // Straight at the wall, running, then along it (to the right, still pressing in).
        control.movement = if t < 12.0 {
            DVec2::new(0.0, 1.0)
        } else {
            DVec2::new(1.0, 0.3)
        };
        control.run = true;
        walk_step(&mut state, &mut walker, &mut control, &w.env, DT, None);
        keep_out_of_buildings(&mut state, &mut walker, &w.env);
        let b = w.in_building(state.position);
        assert!(
            b.z > HALF.z + 0.25 || b.x.abs() > HALF.x + 0.25,
            "walked into the building: {b:?} at {t:.1} s"
        );
        closest = closest.min(b.z - HALF.z);
    }
    let b = w.in_building(state.position);
    assert!(
        closest < 0.35,
        "never reached the wall (closest {closest:.2} m)"
    );
    // Slid east along the wall and round its corner.
    assert!(b.x > HALF.x, "didn't slide along the wall: {b:?}");
}
