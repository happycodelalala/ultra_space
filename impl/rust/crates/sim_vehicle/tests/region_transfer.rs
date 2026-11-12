//! Task 1.12: a ship flying across a sphere-of-influence boundary moves to the new body's
//! region without a jump in position or velocity and without flight assist reacting to it.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use glam::DQuat;
use sim_core::{ObjectState, Simulation, StarSystem};
use sim_vehicle::flight::add_flight_systems;
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

fn index(sys: &StarSystem, id: &str) -> usize {
    sys.bodies.iter().position(|b| b.id == id).unwrap()
}

fn flight_simulation(sys: &Arc<StarSystem>) -> Simulation {
    let mut sim = Simulation::for_star_system(sys.clone(), |_, s| add_flight_systems(s));
    sim.objects.register_transferable::<Vehicle>();
    sim.objects.register_transferable::<VehicleBody>();
    sim.objects.register_transferable::<FlightControl>();
    sim.objects.register_transferable::<FlightTelemetry>();
    sim
}

/// Flies across Lantern's sphere of influence (into it from Meridian's region, or out of it when
/// `leaving`), with the given assist mode and no stick input, and checks every tick for jumps.
fn fly_across(assist: AssistMode, warp: u32, leaving: bool) {
    let sys = Arc::new(StarSystem::from_path(content().join("star_systems/halcyon.json")).unwrap());
    let (planet, moon_index) = (index(&sys, "meridian"), index(&sys, "lantern"));
    let (from, to) = if leaving {
        (moon_index, planet)
    } else {
        (planet, moon_index)
    };
    let planet_frame = sys.bodies[planet].inertial_frame;
    let star_frame = sys.star().inertial_frame;
    let mut sim = flight_simulation(&sys);
    sim.set_time_warp(warp);

    // On the planet side of the moon's sphere of influence, just outside it heading in, or
    // just inside it heading out, at 3 km/s relative to the moon.
    let moon = sys
        .frames
        .state_in(planet_frame, sys.bodies[moon_index].inertial_frame, 0.0);
    let toward = moon.position.normalize();
    let soi = sys.bodies[moon_index].soi_radius;
    let (offset, speed) = if leaving {
        (0.995, -3_000.0)
    } else {
        (1.005, 3_000.0)
    };
    let mut state = ObjectState::at_rest(
        planet_frame,
        moon.position - toward * soi * offset,
        DQuat::IDENTITY,
    );
    state.state.velocity = moon.velocity + toward * speed;
    let start = state.in_frame(&sys.frames, sys.bodies[from].inertial_frame, 0.0);
    let vehicle = kestrel();
    let body = VehicleBody::of(&vehicle).unwrap();
    let control = FlightControl {
        assist,
        ..Default::default()
    };
    let id = sim
        .objects
        .spawn(
            &mut sim.regions,
            from,
            start,
            (vehicle, body, control, FlightTelemetry::default()),
        )
        .unwrap();

    let tick = Duration::from_secs_f64(1.0 / 30.0);
    let star_state = |sim: &Simulation| {
        let t = sim.universe_time().as_secs_f64();
        sim.objects
            .state(&sim.regions, id)
            .unwrap()
            .in_frame(&sys.frames, star_frame, t)
            .state
    };
    let mut previous = star_state(&sim);
    let mut transferred_at = None;
    for i in 0..3_000 {
        let t0 = sim.universe_time().as_secs_f64();
        assert_eq!(sim.advance(tick), 1);
        let dt = sim.universe_time().as_secs_f64() - t0;
        let now = star_state(&sim);
        let loc = sim.objects.location(id).unwrap();

        // Gravity from the star and the planets is tiny over one tick; anything beyond that is a
        // jump from the transfer or a thrust reaction.
        let predicted = previous.position + previous.velocity * dt;
        let position_error = (now.position - predicted).length();
        let velocity_change = (now.velocity - previous.velocity).length();
        assert!(
            position_error < 0.5,
            "tick {i}: position jumped {position_error:.3} m"
        );
        assert!(
            velocity_change < 0.1 + dt,
            "tick {i}: velocity jumped {velocity_change:.3} m/s"
        );
        let world = &sim.regions.get(loc.region).unwrap().world;
        let throttle = world
            .get::<FlightTelemetry>(loc.entity)
            .unwrap()
            .throttles
            .iter()
            .fold(0.0f64, |a, &b| a.max(b));
        assert!(
            throttle < 0.05,
            "tick {i}: assist fired thrusters ({throttle:.2})"
        );

        if loc.region == to && transferred_at.is_none() {
            transferred_at = Some(i);
        }
        if transferred_at.is_some_and(|t| i > t + 60) {
            return;
        }
        previous = now;
    }
    panic!("never reached the destination region");
}

#[test]
fn coasting_with_full_assist_crosses_without_a_jump() {
    fly_across(AssistMode::Full, 1, false);
    fly_across(AssistMode::Full, 1, true);
}

#[test]
fn coasting_without_assist_crosses_without_a_jump() {
    fly_across(AssistMode::Off, 1, false);
    fly_across(AssistMode::Off, 1, true);
}

#[test]
fn crossing_on_rails_under_warp_is_smooth() {
    fly_across(AssistMode::Full, 10, false);
    fly_across(AssistMode::Full, 10, true);
}
