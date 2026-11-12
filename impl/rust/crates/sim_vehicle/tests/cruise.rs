//! Task 1.13: the placeholder cruise drive.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use glam::{DQuat, DVec3};
use sim_core::{ObjectId, ObjectState, Simulation, StarSystem};
use sim_vehicle::cruise::{CHARGE_TIME, drop_altitude};
use sim_vehicle::flight::add_flight_systems;
use sim_vehicle::{
    Cruise, CruiseBlocked, CruisePhase, FlightControl, FlightTelemetry, PartCatalog, PartKind,
    Vehicle, VehicleBody, VehicleDesign,
};

const TICK: f64 = 1.0 / 30.0;

fn content() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../../content/data")
}

fn halcyon() -> Arc<StarSystem> {
    Arc::new(StarSystem::from_path(content().join("star_systems/halcyon.json")).unwrap())
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

struct Trip {
    sys: Arc<StarSystem>,
    sim: Simulation,
    id: ObjectId,
}

impl Trip {
    /// A Kestrel in a circular orbit `altitude` above `body`, nose prograde.
    fn new(body: &str, altitude: f64, vehicle: Vehicle) -> Self {
        let sys = halcyon();
        let mut sim = Simulation::for_star_system(sys.clone(), |_, s| add_flight_systems(s));
        sim.objects.register_transferable::<Vehicle>();
        sim.objects.register_transferable::<VehicleBody>();
        sim.objects.register_transferable::<FlightControl>();
        sim.objects.register_transferable::<FlightTelemetry>();
        sim.objects.register_transferable::<Cruise>();
        let i = index(&sys, body);
        let b = &sys.bodies[i];
        let r = b.radius + altitude;
        let mut state = ObjectState::at_rest(
            b.inertial_frame,
            DVec3::X * r,
            DQuat::from_rotation_arc(DVec3::NEG_Z, DVec3::NEG_Z),
        );
        state.state.velocity = DVec3::NEG_Z * (b.mu / r).sqrt();
        let vbody = VehicleBody::of(&vehicle).unwrap();
        let id = sim
            .objects
            .spawn(
                &mut sim.regions,
                i,
                state,
                (
                    vehicle,
                    vbody,
                    FlightControl::default(),
                    FlightTelemetry::default(),
                    Cruise::default(),
                ),
            )
            .unwrap();
        Self { sys, sim, id }
    }

    fn time(&self) -> f64 {
        self.sim.universe_time().as_secs_f64()
    }

    fn region(&self) -> usize {
        self.sim.objects.location(self.id).unwrap().region
    }

    fn cruise(&mut self) -> &mut Cruise {
        let loc = self.sim.objects.location(self.id).unwrap();
        self.sim
            .regions
            .get_mut(loc.region)
            .unwrap()
            .world
            .get_mut::<Cruise>(loc.entity)
            .unwrap()
            .into_inner()
    }

    /// The ship's state relative to body `i` (its inertial frame).
    fn relative_to(&self, i: usize) -> ObjectState {
        self.sim
            .objects
            .state(&self.sim.regions, self.id)
            .unwrap()
            .in_frame(
                &self.sys.frames,
                self.sys.bodies[i].inertial_frame,
                self.time(),
            )
    }

    fn altitude(&self, i: usize) -> f64 {
        self.relative_to(i).state.position.length() - self.sys.bodies[i].radius
    }

    /// Points the nose at body `i`'s current position (a stand-in for the pilot steering).
    fn aim_at(&mut self, i: usize) {
        let to_target = -self.relative_to(i).state.position.normalize();
        let loc = self.sim.objects.location(self.id).unwrap();
        let world = &mut self.sim.regions.get_mut(loc.region).unwrap().world;
        let mut state = world.get_mut::<ObjectState>(loc.entity).unwrap();
        state.state.rotation = DQuat::from_rotation_arc(DVec3::NEG_Z, to_target);
    }

    fn step(&mut self) {
        assert_eq!(self.sim.advance(Duration::from_secs_f64(TICK)), 1);
    }

    /// Engages, steers at `target` until the drive drops out, and returns the trip time (s).
    fn cruise_to(&mut self, target: usize, max_seconds: f64) -> f64 {
        let start = self.time();
        self.aim_at(target);
        self.cruise().toggle = true;
        self.step();
        assert!(
            matches!(self.cruise().phase, CruisePhase::Charging { .. }),
            "engaged: {:?}",
            self.cruise()
        );
        let mut cruised = false;
        while self.time() - start < max_seconds {
            self.aim_at(target);
            self.step();
            let phase = self.cruise().phase;
            if matches!(phase, CruisePhase::Cruising { .. }) {
                cruised = true;
            } else if cruised {
                return self.time() - start;
            }
        }
        panic!("still cruising after {max_seconds} s");
    }
}

#[test]
fn neighbouring_planet_in_one_to_three_minutes_and_arrives_in_orbit() {
    let mut trip = Trip::new("meridian", 150_000.0, kestrel());
    let hoarfell = index(&trip.sys, "hoarfell");
    let seconds = trip.cruise_to(hoarfell, 600.0);
    eprintln!("Meridian → Hoarfell: {seconds:.0} s");
    assert!(
        (60.0..=180.0).contains(&seconds),
        "Meridian → Hoarfell took {seconds:.0} s"
    );
    assert_eq!(trip.region(), hoarfell, "dropped out in Hoarfell's region");
    let drop = drop_altitude(&trip.sys, hoarfell);
    let altitude = trip.altitude(hoarfell);
    assert!(
        altitude > 0.8 * drop && altitude < 1.05 * drop,
        "dropped at {altitude:.0} m (drop altitude {drop:.0} m)"
    );
    let s = trip.relative_to(hoarfell).state;
    let circular = (trip.sys.bodies[hoarfell].mu / s.position.length()).sqrt();
    assert!(
        (s.velocity.length() - circular).abs() < 1.0,
        "orbital speed"
    );
    assert!(s.velocity.dot(s.position).abs() < 1e-6 * s.velocity.length() * s.position.length());

    // Normal flight resumes: the ship keeps orbiting rather than falling.
    for _ in 0..300 {
        trip.step();
    }
    let later = trip.altitude(hoarfell);
    assert!(
        (later - altitude).abs() < 0.01 * altitude,
        "orbit holds: {later:.0} m"
    );
}

#[test]
fn across_the_system_in_under_eight_minutes() {
    let mut trip = Trip::new("cinder", 50_000.0, kestrel());
    let leviathan = index(&trip.sys, "leviathan");
    let seconds = trip.cruise_to(leviathan, 1_200.0);
    eprintln!("Cinder → Leviathan: {seconds:.0} s");
    assert!(seconds <= 480.0, "Cinder → Leviathan took {seconds:.0} s");
}

#[test]
fn cannot_engage_near_a_planet() {
    let mut trip = Trip::new("meridian", 50_000.0, kestrel());
    let meridian = index(&trip.sys, "meridian");
    trip.cruise().toggle = true;
    trip.step();
    let cruise = trip.cruise().clone();
    assert_eq!(cruise.phase, CruisePhase::Off);
    assert_eq!(cruise.blocked, Some(CruiseBlocked::TooClose(meridian)));
}

#[test]
fn needs_a_cruise_drive() {
    let mut vehicle = kestrel();
    vehicle
        .parts
        .retain(|p| p.def.kind != PartKind::CruiseDrive);
    let mut trip = Trip::new("meridian", 150_000.0, vehicle);
    trip.cruise().toggle = true;
    trip.step();
    assert_eq!(trip.cruise().blocked, Some(CruiseBlocked::NoDrive));
    assert_eq!(trip.cruise().phase, CruisePhase::Off);
}

#[test]
fn toggling_while_charging_cancels() {
    let mut trip = Trip::new("meridian", 150_000.0, kestrel());
    trip.cruise().toggle = true;
    trip.step();
    trip.step();
    trip.cruise().toggle = true;
    trip.step();
    assert_eq!(trip.cruise().phase, CruisePhase::Off);
    for _ in 0..((CHARGE_TIME / TICK) as usize + 10) {
        trip.step();
    }
    assert_eq!(trip.cruise().phase, CruisePhase::Off, "stays off");
}

#[test]
fn heading_straight_at_a_planet_never_overshoots_even_under_warp() {
    for warp in [1, 100, 10_000] {
        let mut trip = Trip::new("meridian", 150_000.0, kestrel());
        let meridian = index(&trip.sys, "meridian");
        // Leave first, then turn around and dive straight back at the planet.
        trip.cruise().toggle = true;
        let leave_until = trip.time() + CHARGE_TIME + 15.0;
        while trip.time() < leave_until {
            // Nose straight up, away from the planet.
            let up = trip.relative_to(meridian).state.position.normalize();
            let loc = trip.sim.objects.location(trip.id).unwrap();
            let world = &mut trip.sim.regions.get_mut(loc.region).unwrap().world;
            world
                .get_mut::<ObjectState>(loc.entity)
                .unwrap()
                .state
                .rotation = DQuat::from_rotation_arc(DVec3::NEG_Z, up);
            trip.step();
        }
        assert!(trip.cruise().is_cruising());
        trip.sim.set_time_warp(warp);
        let drop = drop_altitude(&trip.sys, meridian);
        for _ in 0..20_000 {
            trip.aim_at(meridian);
            trip.step();
            let altitude = trip.altitude(meridian);
            assert!(
                altitude > 0.9 * drop,
                "warp {warp}: down to {altitude:.0} m"
            );
            if !trip.cruise().is_cruising() {
                break;
            }
        }
        assert!(!trip.cruise().is_cruising(), "warp {warp}: dropped out");
    }
}
