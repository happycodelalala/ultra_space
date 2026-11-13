//! Coming down onto the terrain in normal flight (from high up, fast) through the whole
//! simulation, with collision patches streamed in the background as in the game: the ship must
//! end up standing on the ground, never sunk into it.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use glam::{DQuat, DVec3};
use sim_core::{ObjectState, Simulation, StarSystem};
use sim_vehicle::flight::{Reference, add_flight_systems, install_terrain, region_env};
use sim_vehicle::{
    AssistMode, FlightControl, FlightTelemetry, PartCatalog, Vehicle, VehicleBody, VehicleDesign,
};

fn content() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../../content/data")
}

fn kestrel() -> Vehicle {
    let catalog = PartCatalog::load_dir(content().join("parts")).unwrap();
    let design =
        VehicleDesign::from_path(content().join("vehicles/kestrel.json"), &catalog).unwrap();
    Vehicle::from_design(&design)
}

/// Drops the Kestrel from `height` m above the ground at `speed` m/s straight down, with flight
/// assist `assist` (holding still over the ground when on), for `seconds`. Returns the lowest
/// and final height of its centre of mass above the ground, and whether it ended in contact.
fn drop(height: f64, speed: f64, assist: AssistMode, seconds: f64) -> (f64, f64, bool) {
    let sys = Arc::new(StarSystem::from_path(content().join("star_systems/halcyon.json")).unwrap());
    let planet = sys.bodies.iter().position(|b| b.id == "meridian").unwrap();
    let frame = sys.bodies[planet].inertial_frame;
    let mut sim = Simulation::for_star_system(sys.clone(), |_, s| add_flight_systems(s));
    sim.objects.register_transferable::<Vehicle>();
    sim.objects.register_transferable::<VehicleBody>();
    sim.objects.register_transferable::<FlightControl>();
    sim.objects.register_transferable::<FlightTelemetry>();
    install_terrain(&mut sim);
    let terrain = sim
        .regions
        .get(planet)
        .unwrap()
        .world
        .get_resource()
        .cloned();
    let env = region_env(&sys, planet, 0.0, terrain.as_ref());

    // A land spot.
    let up = sim_planet::fibonacci_directions(20_000)
        .map(|d| env.surface_rotation * d)
        .find(|&d| env.ground_height(d * env.radius) > 200.0)
        .unwrap();
    let ground = env.radius + env.ground_height(up * env.radius);
    let p = up * (ground + height);
    let north = up.cross(DVec3::Y.cross(up)).normalize();
    let rotation = DQuat::from_mat3(&glam::DMat3::from_cols(north.cross(up), up, -north));
    let mut state = ObjectState::at_rest(frame, p, rotation);
    state.state.velocity = env.spin.cross(p) - up * speed;
    state.state.angular_velocity = env.spin;
    let vehicle = kestrel();
    let body = VehicleBody::of(&vehicle).unwrap();
    let com = body.mass.center_of_mass;
    let control = FlightControl {
        assist,
        desired_velocity: (assist != AssistMode::Off).then_some((Reference::Surface, DVec3::ZERO)),
        desired_body: Some(planet),
        ..Default::default()
    };
    let id = sim
        .objects
        .spawn(
            &mut sim.regions,
            planet,
            state,
            (vehicle, body, control, FlightTelemetry::default()),
        )
        .unwrap();

    let tick = Duration::from_secs_f64(1.0 / 30.0);
    let (mut lowest, mut last, mut contact) = (f64::MAX, 0.0, false);
    for i in 0..(seconds * 30.0) as u32 {
        sim.advance(tick);
        let t = sim.universe_time().as_secs_f64();
        let loc = sim.objects.location(id).unwrap();
        assert_eq!(loc.region, planet, "stays in Meridian's region");
        let s = sim
            .objects
            .state(&sim.regions, id)
            .unwrap()
            .in_frame(&sys.frames, frame, t)
            .state;
        let env = region_env(&sys, planet, t, terrain.as_ref());
        let c = s.position + s.rotation * com;
        let h = c.length() - env.radius - env.ground_height(c);
        lowest = lowest.min(h);
        last = h;
        let world = &sim.regions.get(planet).unwrap().world;
        contact = world.get::<FlightTelemetry>(loc.entity).unwrap().in_contact;
        if i % 150 == 0 {
            let v = (s.velocity - env.spin.cross(s.position)).length();
            println!(
                "t {:>5.1} height {h:>8.2} m speed {v:>7.2} m/s contact {contact}",
                t
            );
        }
    }
    (lowest, last, contact)
}

#[test]
#[ignore = "slow (a minute of simulation); run with --release -- --ignored"]
fn falling_onto_the_terrain_without_assist_stops_on_the_ground() {
    let (lowest, last, contact) = drop(3_000.0, 150.0, AssistMode::Off, 60.0);
    println!("lowest {lowest:.2} m, final {last:.2} m, contact {contact}");
    assert!(last > 0.0, "standing on the ground, not sunk: {last:.2} m");
}

#[test]
#[ignore = "slow (a minute of simulation); run with --release -- --ignored"]
fn braking_down_onto_the_terrain_with_assist_lands_on_it() {
    let (lowest, last, contact) = drop(3_000.0, 150.0, AssistMode::Full, 90.0);
    println!("lowest {lowest:.2} m, final {last:.2} m, contact {contact}");
    assert!(last > 0.0, "standing on the ground, not sunk: {last:.2} m");
}
