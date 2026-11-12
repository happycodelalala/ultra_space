//! Simulation cost per tick near the ground, through a whole region (task 3.4 follow-up: the
//! Windows rounds showed multi-second frames in low arcade flight and landings). Run on demand:
//! `cargo test --release -p sim_vehicle --test ground_tick_cost -- --ignored --nocapture`.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use glam::{DQuat, DVec3};
use sim_core::{ObjectState, Simulation, StarSystem};
use sim_vehicle::flight::{add_flight_systems, install_terrain, region_env};
use sim_vehicle::{
    ArcadeFlight, FlightControl, FlightTelemetry, PartCatalog, Vehicle, VehicleBody, VehicleDesign,
};

fn content() -> PathBuf {
    // `ULTRA_SPACE_CONTENT` points elsewhere, e.g. when this runs natively on Windows.
    std::env::var_os("ULTRA_SPACE_CONTENT").map_or_else(
        || PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../../content/data"),
        PathBuf::from,
    )
}

fn kestrel() -> Vehicle {
    let catalog = PartCatalog::load_dir(content().join("parts")).unwrap();
    let design =
        VehicleDesign::from_path(content().join("vehicles/kestrel.json"), &catalog).unwrap();
    Vehicle::from_design(&design)
}

/// Mean and worst tick time (ms) for the Kestrel `height` m over Meridian's ground, moving at
/// `speed` m/s along the ground, in arcade flight or not.
fn run(height: f64, speed: f64, arcade: bool) -> (f64, f64) {
    let sys = Arc::new(StarSystem::from_path(content().join("star_systems/halcyon.json")).unwrap());
    let i = sys.bodies.iter().position(|b| b.id == "meridian").unwrap();
    let mut sim = Simulation::for_star_system(sys.clone(), |_, s| add_flight_systems(s));
    install_terrain(&mut sim);
    let env = region_env(&sys, i, 0.0, None);
    let terrain = sim_planet::Terrain::of(&sys.bodies[i]).unwrap();
    let up = env.surface_rotation * DVec3::Y;
    let ground = env.radius + terrain.surface_height(DVec3::Y, 1.0);
    let p = up * (ground + height);
    let east = DVec3::Y.cross(up).normalize();
    let mut state = ObjectState::at_rest(
        sys.bodies[i].inertial_frame,
        p,
        DQuat::from_rotation_arc(DVec3::NEG_Z, east),
    );
    state.state.velocity = env.spin.cross(p) + east * speed;
    let vehicle = kestrel();
    let body = VehicleBody::of(&vehicle).unwrap();
    let id = sim
        .objects
        .spawn(
            &mut sim.regions,
            i,
            state,
            (
                vehicle,
                body,
                FlightControl::default(),
                FlightTelemetry::default(),
            ),
        )
        .unwrap();
    if arcade {
        let loc = sim.objects.location(id).unwrap();
        sim.regions
            .get_mut(loc.region)
            .unwrap()
            .world
            .entity_mut(loc.entity)
            .insert(ArcadeFlight { speed });
    }
    let tick = Duration::from_secs_f64(1.0 / 30.0);
    let (mut total, mut worst) = (0.0f64, 0.0f64);
    let ticks = 900;
    for _ in 0..ticks {
        let t0 = Instant::now();
        sim.advance(tick);
        let ms = t0.elapsed().as_secs_f64() * 1e3;
        total += ms;
        worst = worst.max(ms);
    }
    let loc = sim.objects.location(id).unwrap();
    let patches = sim
        .regions
        .get(loc.region)
        .unwrap()
        .world
        .get_resource::<sim_vehicle::RegionPhysics>()
        .map_or(0, |p| p.0.patch_count());
    println!("  patches alive: {patches}");
    (total / ticks as f64, worst)
}

#[test]
#[ignore = "benchmark: run with --release --ignored"]
fn ticks_near_the_ground_stay_cheap() {
    for (name, height, speed, arcade) in [
        ("arcade 10 m up, 300 m/s", 10.0, 300.0, true),
        ("flying 30 m up, 50 m/s", 30.0, 50.0, false),
        ("falling from 20 m", 20.0, 0.0, false),
        ("orbit-high", 100_000.0, 0.0, false),
        ("orbital speed 380 m up", 380.0, 2_300.0, false),
        ("orbital speed 60 m up", 60.0, 2_300.0, false),
    ] {
        let (mean, worst) = run(height, speed, arcade);
        println!("{name}: mean {mean:.2} ms, worst {worst:.2} ms");
    }
}

/// The descent test's flight: arcade from a 150 km orbit, nose 50° down at 3 km/s.
#[test]
#[ignore = "benchmark: run with --release --ignored"]
fn arcade_descent_stays_cheap() {
    let sys = Arc::new(StarSystem::from_path(content().join("star_systems/halcyon.json")).unwrap());
    let i = sys.bodies.iter().position(|b| b.id == "meridian").unwrap();
    let mut sim = Simulation::for_star_system(sys.clone(), |_, s| add_flight_systems(s));
    install_terrain(&mut sim);
    let env = region_env(&sys, i, 0.0, None);
    let up = env.surface_rotation * DVec3::Y;
    let east = DVec3::Y.cross(up).normalize();
    let p = up * (env.radius + 150_000.0);
    let nose = (east * 0.64 - up * 0.77).normalize();
    let state = ObjectState::at_rest(
        sys.bodies[i].inertial_frame,
        p,
        DQuat::from_rotation_arc(DVec3::NEG_Z, nose),
    );
    let vehicle = kestrel();
    let body = VehicleBody::of(&vehicle).unwrap();
    let id = sim
        .objects
        .spawn(
            &mut sim.regions,
            i,
            state,
            (
                vehicle,
                body,
                FlightControl::default(),
                FlightTelemetry::default(),
                ArcadeFlight { speed: 3_000.0 },
            ),
        )
        .unwrap();
    let tick = Duration::from_secs_f64(1.0 / 30.0);
    for second in 0..90 {
        let t0 = Instant::now();
        for _ in 0..30 {
            sim.advance(tick);
        }
        let ms = t0.elapsed().as_secs_f64() * 1e3 / 30.0;
        if second % 10 == 9 {
            let loc = sim.objects.location(id).unwrap();
            let world = &sim.regions.get(loc.region).unwrap().world;
            let alt = sim
                .objects
                .state(&sim.regions, id)
                .unwrap()
                .state
                .position
                .length()
                - env.radius;
            let patches = world
                .get_resource::<sim_vehicle::RegionPhysics>()
                .map_or(0, |p| p.0.patch_count());
            println!(
                "{}s: {ms:.2} ms per tick, altitude {alt:.0} m, patches {patches}",
                second + 1
            );
        }
    }
}
