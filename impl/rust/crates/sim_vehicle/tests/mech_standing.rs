//! Legged vehicles stand by themselves (task 3.11): set down on a slope, a mech goes onto its
//! legs and stays upright, its feet on the ground, instead of toppling like a rigid statue.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use glam::{DQuat, DVec3};
use sim_core::{ObjectState, Simulation, StarSystem};
use sim_vehicle::flight::{add_flight_systems, install_terrain, region_env};
use sim_vehicle::gait::Walking;
use sim_vehicle::joints::JointState;
use sim_vehicle::{
    FlightControl, FlightTelemetry, PartCatalog, Vehicle, VehicleBody, VehicleDesign,
};

fn content() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../../content/data")
}

/// Sets design `id` down on a slope steeper than `min_slope_deg`, runs `seconds`, and returns
/// its final tilt from the vertical (degrees), whether it is on its legs, and its centre of
/// mass height above the ground.
fn stand_on_slope(id: &str, min_slope_deg: f64, seconds: f64) -> (f64, bool, f64) {
    let catalog = PartCatalog::load_dir(content().join("parts")).unwrap();
    let design =
        VehicleDesign::from_path(content().join(format!("vehicles/{id}.json")), &catalog).unwrap();
    let vehicle = Vehicle::from_design(&design);
    let body = VehicleBody::of(&vehicle).unwrap();
    let com = body.mass.center_of_mass;
    let sys = Arc::new(StarSystem::from_path(content().join("star_systems/halcyon.json")).unwrap());
    let planet = sys.bodies.iter().position(|b| b.id == "meridian").unwrap();
    let frame = sys.bodies[planet].inertial_frame;
    let mut sim = Simulation::for_star_system(sys.clone(), |_, s| add_flight_systems(s));
    for register in [
        |s: &mut Simulation| s.objects.register_transferable::<Vehicle>(),
        |s: &mut Simulation| s.objects.register_transferable::<VehicleBody>(),
        |s: &mut Simulation| s.objects.register_transferable::<FlightControl>(),
        |s: &mut Simulation| s.objects.register_transferable::<FlightTelemetry>(),
        |s: &mut Simulation| s.objects.register_transferable::<Walking>(),
        |s: &mut Simulation| s.objects.register_transferable::<JointState>(),
    ] {
        register(&mut sim);
    }
    install_terrain(&mut sim);
    let terrain = sim
        .regions
        .get(planet)
        .unwrap()
        .world
        .get_resource()
        .cloned();
    let env = region_env(&sys, planet, 0.0, terrain.as_ref());
    let cos = min_slope_deg.to_radians().cos();
    let up = sim_planet::fibonacci_directions(200_000)
        .map(|d| env.surface_rotation * d)
        .find(|&d| {
            let p = d * env.radius;
            env.ground_height(p) > 100.0
                && env.ground_normal(p).dot(d) < cos
                && env.ground_normal(p).dot(d) > cos - 0.03
        })
        .expect("a slope");
    let slope = env
        .ground_normal(up * env.radius)
        .dot(up)
        .acos()
        .to_degrees();
    // Standing on straight legs, the soles just above the ground under the body.
    let ground = env.radius + env.ground_height(up * env.radius);
    let sole_depth = vehicle
        .parts
        .iter()
        .filter(|p| p.def.landing_leg.is_some())
        .map(|p| -(p.position.y - 0.5))
        .fold(0.0, f64::max);
    let p = up * (ground + sole_depth + 0.5);
    let north = up.cross(DVec3::Y.cross(up)).normalize();
    let rotation = DQuat::from_mat3(&glam::DMat3::from_cols(north.cross(up), up, -north));
    let mut state = ObjectState::at_rest(frame, p, rotation);
    state.state.velocity = env.spin.cross(p);
    state.state.angular_velocity = env.spin;
    let object = sim
        .objects
        .spawn(
            &mut sim.regions,
            planet,
            state,
            (
                vehicle,
                body,
                FlightControl::default(),
                FlightTelemetry::default(),
            ),
        )
        .unwrap();
    let tick = Duration::from_secs_f64(1.0 / 30.0);
    let mut worst = 0.0f64;
    for _ in 0..(seconds * 30.0) as u32 {
        sim.advance(tick);
        let t = sim.universe_time().as_secs_f64();
        let s = sim
            .objects
            .state(&sim.regions, object)
            .unwrap()
            .in_frame(&sys.frames, frame, t)
            .state;
        worst = worst.max((s.rotation * DVec3::Y).angle_between(s.position.normalize()));
    }
    let t = sim.universe_time().as_secs_f64();
    let s = sim
        .objects
        .state(&sim.regions, object)
        .unwrap()
        .in_frame(&sys.frames, frame, t)
        .state;
    let env = region_env(&sys, planet, t, terrain.as_ref());
    let tilt = (s.rotation * DVec3::Y)
        .angle_between(s.position.normalize())
        .to_degrees();
    let c = s.position + s.rotation * com;
    let height = c.length() - env.radius - env.ground_height(c);
    let loc = sim.objects.location(object).unwrap();
    let standing = sim
        .regions
        .get(loc.region)
        .unwrap()
        .world
        .get::<Walking>(loc.entity)
        .is_some();
    println!(
        "{id} on a {slope:.0} deg slope: tilt {tilt:.1} deg (worst {:.1}), on its legs {standing}, centre of mass {height:.1} m up",
        worst.to_degrees()
    );
    (tilt, standing, height)
}

#[test]
fn the_heavy_frame_stands_on_a_slope() {
    let (tilt, standing, height) = stand_on_slope("heavy_frame", 14.0, 12.0);
    assert!(standing, "on its legs");
    assert!(tilt < 3.0, "upright: {tilt:.1} deg");
    assert!(height > 8.0, "standing, not lying: {height:.1} m");
}

#[test]
fn the_bastion_stands_on_a_slope() {
    let (tilt, standing, height) = stand_on_slope("bastion", 14.0, 12.0);
    assert!(standing, "on its legs");
    assert!(tilt < 3.0, "upright: {tilt:.1} deg");
    assert!(height > 8.0, "standing, not lying: {height:.1} m");
}
