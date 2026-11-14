//! A vehicle's per-tick cost must not grow with its part count (vehicle-builder.md §4; task
//! 3.4): a 300-part ship should cost about as much as the 27-part Kestrel. The timing check is a
//! benchmark run on demand (`cargo test --release -p sim_vehicle --test part_count_cost --
//! --ignored --nocapture`); the structural checks run with every test.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use glam::{DQuat, DVec3};
use sim_core::ObjectId;
use sim_vehicle::catalog::PartCatalog;
use sim_vehicle::design::{Attach, VehicleDesign, VehiclePart};
use sim_vehicle::flight::{FlightPlan, RegionTerrain, RigidState, plan_flight, region_env};
use sim_vehicle::{FlightControl, FlightTelemetry, PhysicsScene, Vehicle, VehicleBody};

fn content() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../../content/data")
}

fn kestrel() -> Vehicle {
    let catalog = PartCatalog::load_dir(content().join("parts")).unwrap();
    let design =
        VehicleDesign::from_path(content().join("vehicles/kestrel.json"), &catalog).unwrap();
    Vehicle::from_design(&design)
}

/// The Kestrel with structural blocks bolted on until it has `parts` parts.
fn big_ship(parts: usize) -> Vehicle {
    let mut v = kestrel();
    let block = v.parts[1].def.clone(); // a hull section
    let mut i = 0;
    while v.parts.len() < parts {
        let (x, z) = ((i % 6) as f64 - 2.5, (i / 6 % 8) as f64 - 4.0);
        let y = 3.0 + (i / 48) as f64 * 2.5;
        v.parts.push(VehiclePart {
            name: format!("block_{i}"),
            def: Arc::clone(&block),
            position: DVec3::new(x * 3.0, y, z * 3.0),
            rotation: DQuat::IDENTITY,
            parent: Some(1),
            attach: Attach::Surface,
            symmetry: None,
        });
        i += 1;
    }
    v
}

#[test]
fn colliders_compile_into_a_few_pieces() {
    let body = VehicleBody::of(&big_ship(300)).unwrap();
    assert_eq!(body.colliders.len(), 300);
    let shape = sim_vehicle::physics::compound_shape(&body.colliders);
    let pieces = shape.as_compound().unwrap().shapes().len();
    // Four exact landing legs plus at most 32 boxes.
    assert!(pieces <= 4 + 32, "{pieces} pieces");
    let kestrel = VehicleBody::of(&kestrel()).unwrap();
    let pieces = sim_vehicle::physics::compound_shape(&kestrel.colliders)
        .as_compound()
        .unwrap()
        .shapes()
        .len();
    assert!(pieces <= 16, "Kestrel: {pieces} pieces");
}

fn tick_cost(vehicle: &Vehicle) -> f64 {
    tick_cost_at(vehicle, 30.0)
}

/// Microseconds per tick for one vehicle over Meridian's ground (patches built): hovering if it
/// can, else landed.
fn tick_cost_at(vehicle: &Vehicle, height: f64) -> f64 {
    let sys = sim_core::StarSystem::from_path(content().join("star_systems/halcyon.json")).unwrap();
    let i = sys.bodies.iter().position(|b| b.id == "meridian").unwrap();
    let terrain = sim_planet::Terrain::of(&sys.bodies[i]).map(Arc::new);
    let mut env = region_env(&sys, i, 0.0, Some(&RegionTerrain(terrain)));
    let rotation0 = env.surface_rotation;
    let body = VehicleBody::of(vehicle).unwrap();
    let up = DVec3::Y;
    let p = up * (env.radius + env.ground_height(up * env.radius) + height);
    let mut s = RigidState {
        position: p,
        velocity: env.spin.cross(p),
        rotation: DQuat::from_rotation_arc(DVec3::Y, up),
        angular_velocity: env.spin,
    };
    let mut scene = PhysicsScene::with_inline_patches();
    let mut control = FlightControl::default();
    let mut telemetry = FlightTelemetry::default();
    let mut rails = None;
    let id = ObjectId(1);
    let dt = 1.0 / 30.0;
    let mut n = 0u32;
    let mut tick = || {
        // The planet turns.
        env.surface_rotation =
            (DQuat::from_scaled_axis(env.spin * (f64::from(n) * dt)) * rotation0).normalize();
        n += 1;
        let origin = s.to_origin(body.mass.center_of_mass);
        let t0 = Instant::now();
        let plan = plan_flight(
            &origin,
            &body,
            &mut control,
            &mut telemetry,
            &mut rails,
            &env,
            0.0,
            dt,
        );
        let planned = t0.elapsed().as_secs_f64();
        if let FlightPlan::Integrate { state, wrench } = plan {
            scene.place_dynamic(id, &body, &state, wrench);
            scene.step(&env, dt);
            let (next, contact) = scene.state(id).unwrap();
            s = next;
            telemetry.in_contact = contact;
        }
        (planned, t0.elapsed().as_secs_f64() - planned)
    };
    // Warm up: patches built, and a ship too heavy to hover has landed and come to rest.
    for _ in 0..300 {
        tick();
    }
    let ticks = 600;
    let (mut plan, mut step) = (0.0, 0.0);
    for _ in 0..ticks {
        let (p, st) = tick();
        plan += p;
        step += st;
    }
    let per = |x: f64| x * 1e6 / ticks as f64;
    println!(
        "  plan {:.1} µs, step {:.1} µs, at rest {}, contact {}",
        per(plan),
        per(step),
        scene.at_rest(id),
        telemetry.in_contact
    );
    per(plan + step)
}

#[test]
#[ignore = "benchmark: run with --release --ignored"]
fn per_tick_cost_does_not_grow_with_parts() {
    let high_small = tick_cost_at(&kestrel(), 100_000.0);
    let high_big = tick_cost_at(&big_ship(300), 100_000.0);
    println!("flying: Kestrel {high_small:.1} µs, 300 parts {high_big:.1} µs");
    let small = tick_cost(&kestrel());
    let big = tick_cost(&big_ship(300));
    println!("landed: Kestrel (27 parts) {small:.1} µs, 300 parts {big:.1} µs");
    assert!(
        high_big < high_small * 2.0,
        "flying, 300 parts cost {:.1}× the Kestrel",
        high_big / high_small
    );
    // Landed (at rest) a big ship still costs a few times more, but stays small.
    assert!(big < 500.0, "landed, 300 parts cost {big:.0} µs per tick");
}
