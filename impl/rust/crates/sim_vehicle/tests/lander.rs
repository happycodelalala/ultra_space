//! Task 3.9: a lander built from parts (cockpit, hull, a main engine pointing down, a reaction
//! wheel, mirrored legs) lifts off its pad when the pilot asks to go up.

use std::path::PathBuf;
use std::sync::Arc;

use glam::{DMat3, DQuat, DVec3};
use sim_core::ObjectId;
use sim_vehicle::builder::{
    Symmetry, attach_at_node, attach_to_surface, free_nodes, place_symmetric, stats,
};
use sim_vehicle::flight::{FlightPlan, RegionTerrain, RigidState, plan_flight, region_env};
use sim_vehicle::{
    FlightControl, FlightTelemetry, PartCatalog, PhysicsScene, Vehicle, VehicleBody, VehiclePart,
};

fn content() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../../content/data")
}

/// A balanced lander: a hull section with the cockpit on top, the main engine underneath
/// (pointing down) and a mirrored pair of legs, so the engine pushes through the centre of mass.
fn lander() -> Vehicle {
    let catalog = PartCatalog::load_dir(content().join("parts")).unwrap();
    let part = |id: &str| catalog.get(id).unwrap().clone();
    let mut v = Vehicle {
        design_id: "lander".into(),
        name: "Lander".into(),
        parts: vec![VehiclePart {
            name: "root".into(),
            def: part("hull.section.medium"),
            ..Default::default()
        }],
    };
    let node = |v: &Vehicle, part: usize, id: &str| {
        free_nodes(v)
            .into_iter()
            .find(|n| n.part == part && n.node == id)
            .unwrap()
    };
    let at = node(&v, 0, "top");
    attach_at_node(&mut v, &part("cockpit.small"), "rear", &at, 0.0).unwrap();
    let at = node(&v, 0, "bottom");
    attach_at_node(&mut v, &part("thruster.main.medium"), "top", &at, 0.0).unwrap();
    // Legs reaching below the engine's nozzle.
    let leg = attach_to_surface(
        &mut v,
        &part("landing_leg.small"),
        0,
        DVec3::new(-1.8, -3.0, 0.0),
        DQuat::IDENTITY,
    )
    .unwrap();
    place_symmetric(&mut v, leg, Symmetry::Mirror);
    v
}

/// The same parts with the cockpit ahead of the hull: the engine pushes off the centre of mass.
fn unbalanced_lander() -> Vehicle {
    let catalog = PartCatalog::load_dir(content().join("parts")).unwrap();
    let part = |id: &str| catalog.get(id).unwrap().clone();
    let mut v = Vehicle {
        design_id: "lander".into(),
        name: "Lander".into(),
        parts: vec![VehiclePart {
            name: "root".into(),
            def: part("cockpit.small"),
            ..Default::default()
        }],
    };
    let at = free_nodes(&v)
        .into_iter()
        .find(|n| n.node == "rear")
        .unwrap();
    attach_at_node(&mut v, &part("hull.section.medium"), "front", &at, 0.0).unwrap();
    let at = free_nodes(&v)
        .into_iter()
        .find(|n| n.part == 1 && n.node == "bottom")
        .unwrap();
    attach_at_node(&mut v, &part("thruster.main.medium"), "top", &at, 0.0).unwrap();
    v
}

#[test]
fn the_builder_flags_an_unbalanced_lander() {
    let problems = stats(&unbalanced_lander(), 9.81).problems;
    assert!(
        problems
            .iter()
            .any(|p| p.contains("off the centre of mass")),
        "{problems:?}"
    );
    let problems = stats(&lander(), 9.81).problems;
    assert!(problems.is_empty(), "{problems:?}");
}

#[test]
fn a_built_lander_lifts_off() {
    let body = VehicleBody::of(&lander()).unwrap();
    assert!(body.performance.max_acceleration[2] > 9.81, "can hover");
    let sys = sim_core::StarSystem::from_path(content().join("star_systems/halcyon.json")).unwrap();
    let i = sys.bodies.iter().position(|b| b.id == "meridian").unwrap();
    let terrain = sim_planet::Terrain::of(&sys.bodies[i]).map(Arc::new);
    let mut env = region_env(&sys, i, 0.0, Some(&RegionTerrain(terrain)));
    let rotation0 = env.surface_rotation;
    let up = sim_planet::fibonacci_directions(20_000)
        .map(|d| env.surface_rotation * d)
        .find(|&d| {
            let p = d * env.radius;
            env.ground_height(p) > 100.0 && env.ground_normal(p).dot(d) > 0.995
        })
        .unwrap();
    let north = up.cross(DVec3::Y.cross(up)).normalize();
    let rotation = DQuat::from_mat3(&DMat3::from_cols(north.cross(up), up, -north));
    let ground = env.radius + env.ground_height(up * env.radius);
    // Set down with the legs' feet just above the ground (3.8 m below the hull's centre).
    let p = up * (ground + 4.0) + rotation * body.mass.center_of_mass;
    let mut s = RigidState {
        position: p,
        velocity: env.spin.cross(p),
        rotation,
        angular_velocity: env.spin,
    };
    let mut scene = PhysicsScene::with_inline_patches();
    let mut control = FlightControl::default();
    let mut telemetry = FlightTelemetry::default();
    let mut rails = None;
    let dt = 1.0 / 30.0;
    let height = |s: &RigidState, env: &sim_vehicle::flight::BodyEnv| {
        s.position.length() - (env.radius + env.ground_height(s.position))
    };
    let mut start = 0.0;
    for tick in 0..(30 * 10) {
        let t = f64::from(tick) * dt;
        env.surface_rotation = (DQuat::from_scaled_axis(env.spin * t) * rotation0).normalize();
        if tick == 30 * 4 {
            start = height(&s, &env);
        }
        // Settle for 4 s, then hold "up" (Space).
        control.translate = if tick >= 30 * 4 {
            DVec3::Y
        } else {
            DVec3::ZERO
        };
        let origin = s.to_origin(body.mass.center_of_mass);
        match plan_flight(
            &origin,
            &body,
            &mut control,
            &mut telemetry,
            &mut rails,
            &env,
            t,
            dt,
        ) {
            FlightPlan::Integrate { state, wrench } => {
                scene.place_dynamic(ObjectId(1), &body, &state, wrench);
                scene.step(&env, dt);
                let (next, contact) = scene.state(ObjectId(1)).unwrap();
                s = next;
                telemetry.in_contact = contact;
            }
            FlightPlan::Moved(_) => unreachable!(),
        }
    }
    let climbed = height(&s, &env) - start;
    assert!(climbed > 20.0, "climbed {climbed:.1} m in 6 s");
}
