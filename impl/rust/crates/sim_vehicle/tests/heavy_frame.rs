//! Validating the builder with a big, symmetric design: an original heavy humanoid frame
//! (~18 m, ~90 t) assembled only with the builder's operations, as a player would: limbs by
//! mirror symmetry, four binders on the back by radial ×4 symmetry, engines and drone pods
//! copied onto every binder. `content/data/vehicles/heavy_frame.json` is this design; refresh it
//! with `cargo test -p sim_vehicle --test heavy_frame -- --ignored`.

use std::path::PathBuf;
use std::sync::Arc;

use glam::{DQuat, DVec3};
use sim_core::ObjectId;
use sim_vehicle::builder::{
    Symmetry, attach_at_node, attach_to_surface, free_nodes, place_symmetric, stats,
};
use sim_vehicle::design::{NamedPose, check_join, design_json};
use sim_vehicle::flight::{BodyEnv, FlightPlan, RigidState, plan_flight};
use sim_vehicle::{
    FlightControl, FlightTelemetry, PartCatalog, PartDef, PhysicsScene, Vehicle, VehicleBody,
    VehicleDesign, VehiclePart,
};

fn content() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../../content/data")
}

const NAME: &str = "Heavy Frame";
const DESCRIPTION: &str = "An original heavy humanoid frame, about 18 m and 90 tonnes, built in \
     the builder to validate it: a torso cockpit, mirrored arms and legs, four binders on the \
     back (radial symmetry) with an engine and drone pods each, and a big gyro. Forward is -Z.";

/// How far below the thigh's centre the thigh engines sit (m), chosen so the forward thrust
/// passes through the centre of mass.
const THIGH_ENGINE_DROP: f64 = -2.0;

/// Assembles the frame with the builder's operations.
fn assemble() -> Vehicle {
    let catalog = PartCatalog::load_dir(content().join("parts")).unwrap();
    let part = |id: &str| -> Arc<PartDef> { catalog.get(id).unwrap().clone() };
    let mut v = Vehicle {
        design_id: "heavy_frame".into(),
        name: NAME.into(),
        parts: vec![VehiclePart {
            name: "torso".into(),
            def: part("frame.torso"),
            ..Default::default()
        }],
        ..Default::default()
    };
    // Attach `id` by its node `own` to node `to` of the named part; returns the new index.
    let join = |v: &mut Vehicle, parent: &str, to: &str, id: &str, own: &str| -> usize {
        let p = v.parts.iter().position(|p| p.name == parent).unwrap();
        let at = free_nodes(v)
            .into_iter()
            .find(|n| n.part == p && n.node == to)
            .unwrap_or_else(|| panic!("{parent}.{to} is free"));
        attach_at_node(v, &part(id), own, &at, 0.0).unwrap()
    };
    let rename = |v: &mut Vehicle, i: usize, name: &str| v.parts[i].name = name.into();
    let rename_last = |v: &mut Vehicle, name: &str| {
        let last = v.parts.len() - 1;
        v.parts[last].name = name.into();
    };

    let i = join(&mut v, "torso", "neck", "frame.head", "neck");
    rename(&mut v, i, "head");
    let i = join(&mut v, "torso", "waist", "frame.waist", "top");
    rename(&mut v, i, "waist");

    // Arms: a shoulder block, upper arm and forearm on the left, mirrored to the right.
    let i = join(&mut v, "torso", "shoulder_left", "frame.shoulder", "right");
    rename(&mut v, i, "shoulder_left");
    place_symmetric(&mut v, i, Symmetry::Mirror);
    rename_last(&mut v, "shoulder_right");
    let i = join(&mut v, "shoulder_left", "down", "frame.hinge", "base");
    rename(&mut v, i, "shoulder_joint_left");
    place_symmetric(&mut v, i, Symmetry::Mirror);
    rename_last(&mut v, "shoulder_joint_right");
    let i = join(&mut v, "shoulder_joint_left", "end", "frame.limb", "top");
    rename(&mut v, i, "upper_arm_left");
    place_symmetric(&mut v, i, Symmetry::Mirror);
    rename_last(&mut v, "upper_arm_right");
    let i = join(&mut v, "upper_arm_left", "bottom", "frame.hinge", "base");
    rename(&mut v, i, "elbow_joint_left");
    place_symmetric(&mut v, i, Symmetry::Mirror);
    rename_last(&mut v, "elbow_joint_right");
    let i = join(&mut v, "elbow_joint_left", "end", "frame.limb", "top");
    rename(&mut v, i, "forearm_left");
    place_symmetric(&mut v, i, Symmetry::Mirror);
    rename_last(&mut v, "forearm_right");

    // Legs: thigh, shin and foot on the left hip, mirrored.
    let i = join(&mut v, "waist", "hip_left", "frame.hinge", "base");
    rename(&mut v, i, "hip_joint_left");
    place_symmetric(&mut v, i, Symmetry::Mirror);
    rename_last(&mut v, "hip_joint_right");
    let i = join(&mut v, "hip_joint_left", "end", "frame.leg", "top");
    rename(&mut v, i, "thigh_left");
    place_symmetric(&mut v, i, Symmetry::Mirror);
    rename_last(&mut v, "thigh_right");
    let i = join(&mut v, "thigh_left", "bottom", "frame.hinge", "base");
    rename(&mut v, i, "knee_joint_left");
    place_symmetric(&mut v, i, Symmetry::Mirror);
    rename_last(&mut v, "knee_joint_right");
    let i = join(&mut v, "knee_joint_left", "end", "frame.leg", "top");
    rename(&mut v, i, "shin_left");
    place_symmetric(&mut v, i, Symmetry::Mirror);
    rename_last(&mut v, "shin_right");
    let i = join(&mut v, "shin_left", "bottom", "frame.foot", "top");
    rename(&mut v, i, "foot_left");
    place_symmetric(&mut v, i, Symmetry::Mirror);
    rename_last(&mut v, "foot_right");

    // Backpack on the long axis; one binder, copied to all four mounts.
    let i = join(&mut v, "torso", "back", "frame.backpack", "front");
    rename(&mut v, i, "backpack");
    let i = join(
        &mut v,
        "backpack",
        "mount_upper_right",
        "frame.hinge",
        "base",
    );
    rename(&mut v, i, "binder_joint_1");
    let first_copy = v.parts.len();
    place_symmetric(&mut v, i, Symmetry::Radial(4));
    for (k, j) in (first_copy..v.parts.len()).enumerate() {
        rename(&mut v, j, &format!("binder_joint_{}", k + 2));
    }
    let i = join(&mut v, "binder_joint_1", "end", "frame.binder", "root");
    rename(&mut v, i, "binder_1");
    let first_copy = v.parts.len();
    place_symmetric(&mut v, i, Symmetry::Radial(4));
    for (k, j) in (first_copy..v.parts.len()).enumerate() {
        rename(&mut v, j, &format!("binder_{}", k + 2));
    }
    // An engine on the first binder's rear, copied to every binder.
    let i = join(&mut v, "binder_1", "rear", "thruster.main.medium", "top");
    rename(&mut v, i, "engine_1");
    let first_copy = v.parts.len();
    place_symmetric(&mut v, i, Symmetry::Radial(4));
    for (k, j) in (first_copy..v.parts.len()).enumerate() {
        rename(&mut v, j, &format!("engine_{}", k + 2));
    }
    // Two drone pods on the first binder's outer face, copied to every binder.
    let b = v.parts.iter().position(|p| p.name == "binder_1").unwrap();
    let (bp, br) = (v.parts[b].position, v.parts[b].rotation);
    for (n, along) in [1.5, 2.8].iter().enumerate() {
        let at = bp + br * DVec3::new(1.0, *along, -0.6);
        let i = attach_to_surface(&mut v, &part("frame.drone_pod"), b, at, br).unwrap();
        rename(&mut v, i, &format!("pod_{}_1", n + 1));
        let first_copy = v.parts.len();
        place_symmetric(&mut v, i, Symmetry::Radial(4));
        for (k, j) in (first_copy..v.parts.len()).enumerate() {
            rename(&mut v, j, &format!("pod_{}_{}", n + 1, k + 2));
        }
    }
    // The binder engines sit around the torso but the legs pull the centre of mass down: a pair
    // of engines on the backs of the thighs brings the thrust line down to it (the builder's
    // balance check flags the frame without them).
    let thigh = v.parts.iter().position(|p| p.name == "thigh_left").unwrap();
    let at = v.parts[thigh].position + DVec3::new(0.0, THIGH_ENGINE_DROP, 2.0);
    let nozzle_back = DQuat::from_rotation_x(-std::f64::consts::FRAC_PI_2);
    let i = attach_to_surface(
        &mut v,
        &part("thruster.main.medium"),
        thigh,
        at,
        nozzle_back,
    )
    .unwrap();
    rename(&mut v, i, "thigh_engine_left");
    place_symmetric(&mut v, i, Symmetry::Mirror);
    rename_last(&mut v, "thigh_engine_right");
    // The gyro inside the torso.
    let i =
        attach_to_surface(&mut v, &part("frame.gyro"), 0, DVec3::ZERO, DQuat::IDENTITY).unwrap();
    rename(&mut v, i, "gyro");
    // Poses (degrees; a positive turn about a hinge's X swings what hangs below it forward).
    let both = |side: &str, a: f64| vec![(format!("{side}_left"), a), (format!("{side}_right"), a)];
    let pose = |name: &str, angles: Vec<(String, f64)>| NamedPose {
        name: name.into(),
        angles,
    };
    let mut arms_up = both("shoulder_joint", 150.0);
    arms_up.extend(both("elbow_joint", 20.0));
    let binders = (1..=4)
        .map(|k| (format!("binder_joint_{k}"), 35.0))
        .collect();
    let mut crouch = both("hip_joint", 45.0);
    crouch.extend(both("knee_joint", -90.0));
    v.poses = vec![
        pose("stand", Vec::new()),
        pose("arms_up", arms_up),
        pose("binders_open", binders),
        pose("crouch", crouch),
    ];
    v
}

#[test]
fn the_heavy_frame_assembles_into_a_valid_balanced_design() {
    let v = assemble();
    // Torso, head, waist; 2 × (shoulder, upper arm, forearm, thigh, shin, foot); backpack;
    // 4 binders, 4 engines, 8 pods; 2 thigh engines; gyro; 12 hinges (shoulders, elbows,
    // hips, knees, binders).
    assert_eq!(v.parts.len(), 3 + 12 + 1 + 4 + 4 + 8 + 2 + 1 + 12);
    for (i, p) in v.parts.iter().enumerate().skip(1) {
        let parent = p.parent.unwrap();
        assert!(parent < i);
        assert!(check_join(p, &v.parts[parent]).is_none(), "{}", p.name);
    }
    // Four binders in an X around the long axis, each with its engine.
    let binders: Vec<&VehiclePart> = v
        .parts
        .iter()
        .filter(|p| p.def.id == "frame.binder")
        .collect();
    assert_eq!(binders.len(), 4);
    for b in &binders {
        assert!(
            (b.position.x.abs() - b.position.y.abs()).abs() < 1e-6,
            "{:?}",
            b.position
        );
    }
    let body = VehicleBody::of(&v).unwrap();
    let com = body.mass.center_of_mass;
    assert!(com.x.abs() < 1e-9, "balanced left and right: {com:?}");
    let s = stats(&v, 9.81);
    let forward: DVec3 = body
        .allocator
        .thrusters()
        .iter()
        .filter(|t| t.direction.z < -0.9)
        .map(|t| t.position.cross(t.direction * t.max_thrust))
        .sum();
    println!(
        "forward thrust torque {:.0} kN m, com {com:?}",
        forward.length() / 1000.0
    );
    println!(
        "{} parts, {:.1} t, {:.1} x {:.1} x {:.1} m, thrust/weight {:.2}, turn {:.2} rad/s2, problems {:?}",
        s.parts,
        s.mass_kg / 1000.0,
        s.size.x,
        s.size.y,
        s.size.z,
        s.thrust_to_weight,
        s.turn_acceleration,
        s.problems
    );
    assert!(s.problems.is_empty(), "{:?}", s.problems);
    assert!(s.thrust_to_weight > 1.5);
}

#[test]
fn the_heavy_frame_flies_straight_in_space() {
    let body = VehicleBody::of(&assemble()).unwrap();
    let env = BodyEnv {
        mu: 0.0,
        radius: 1.0,
        surface_reference_altitude: 0.0,
        spin: DVec3::ZERO,
        surface_rotation: DQuat::IDENTITY,
        solid: false,
        terrain: None,
        air_density: 0.0,
        air_scale_height: 1.0,
    };
    let mut s = RigidState {
        position: DVec3::new(1.0e7, 0.0, 0.0),
        velocity: DVec3::ZERO,
        rotation: DQuat::IDENTITY,
        angular_velocity: DVec3::ZERO,
    };
    let mut scene = PhysicsScene::without_patches();
    let mut control = FlightControl {
        translate: DVec3::NEG_Z, // full forward (W)
        ..Default::default()
    };
    let mut telemetry = FlightTelemetry::default();
    let mut rails = None;
    let dt = 1.0 / 30.0;
    for tick in 0..(30 * 10) {
        let origin = s.to_origin(body.mass.center_of_mass);
        let t = f64::from(tick) * dt;
        if let FlightPlan::Integrate { state, wrench } = plan_flight(
            &origin,
            &body,
            &mut control,
            &mut telemetry,
            &mut rails,
            &env,
            t,
            dt,
        ) {
            scene.place_dynamic(ObjectId(1), &body, &state, wrench);
            scene.step(&env, dt);
            s = scene.state(ObjectId(1)).unwrap().0;
        }
    }
    let forward = s.rotation * DVec3::NEG_Z;
    let speed = s.velocity.dot(forward);
    assert!(speed > 100.0, "10 s at full forward thrust: {speed:.0} m/s");
    assert!(
        forward.dot(DVec3::NEG_Z) > 0.99,
        "flew straight (heading {forward:?})"
    );
}

#[test]
fn the_design_file_is_this_design() {
    let catalog = PartCatalog::load_dir(content().join("parts")).unwrap();
    let file = VehicleDesign::from_path(content().join("vehicles/heavy_frame.json"), &catalog)
        .expect("content/data/vehicles/heavy_frame.json (refresh with --ignored)");
    let saved = Vehicle::from_design(&file);
    let built = assemble();
    assert_eq!(
        saved.parts.len(),
        built.parts.len(),
        "refresh with --ignored"
    );
    for (a, b) in saved.parts.iter().zip(&built.parts) {
        assert_eq!(
            (&a.name, &a.def.id, a.parent),
            (&b.name, &b.def.id, b.parent)
        );
        assert_eq!(a.attach, b.attach, "{}", a.name);
        // The file keeps micrometres and whole or near-whole degrees.
        assert!(
            a.position.distance(b.position) < 1e-5,
            "{} (refresh with --ignored)",
            a.name
        );
        assert!(a.rotation.dot(b.rotation).abs() > 1.0 - 1e-9, "{}", a.name);
    }
}

#[test]
#[ignore = "writes content/data/vehicles/heavy_frame.json"]
fn write_the_design_file() {
    let json = design_json(&assemble(), "heavy_frame", NAME, DESCRIPTION);
    std::fs::write(content().join("vehicles/heavy_frame.json"), json).unwrap();
}

#[test]
fn the_heavy_frame_moves_its_joints() {
    let v = assemble();
    let body = VehicleBody::of(&v).unwrap();
    let a = &body.articulation;
    // The root plus one rigid group per hinge.
    assert_eq!(a.groups.len(), 13);
    let mut state = sim_vehicle::joints::JointState::rest(a);
    let set = |state: &mut sim_vehicle::joints::JointState, pose: &str| {
        let pose = v.poses.iter().find(|p| p.name == pose).unwrap();
        for (joint, deg) in &pose.angles {
            let part = v.parts.iter().position(|p| &p.name == joint).unwrap();
            state.targets[a.group_of_joint(part).unwrap()] = deg.to_radians();
        }
    };
    set(&mut state, "arms_up");
    for _ in 0..(30 * 4) {
        state.advance(a, 1.0 / 30.0);
    }
    // The left forearm, which hung below the shoulder, is now above it.
    let forearm = v
        .parts
        .iter()
        .position(|p| p.name == "forearm_left")
        .unwrap();
    let shoulder = v
        .parts
        .iter()
        .position(|p| p.name == "shoulder_left")
        .unwrap();
    let raised = state.poses[a.part_group[forearm]].apply(v.parts[forearm].position);
    assert!(
        raised.y > v.parts[shoulder].position.y,
        "forearm at {raised:?}, shoulder at {:?}",
        v.parts[shoulder].position
    );
    // Opening the binders turns their engines: the allocator follows.
    let rest = body.allocator.thrusters()[0].direction;
    set(&mut state, "binders_open");
    for _ in 0..(30 * 2) {
        state.advance(a, 1.0 / 30.0);
    }
    let posed = body.posed_allocator(&state.poses);
    let engine = posed.thrusters()[0].direction;
    assert!(
        rest.dot(engine) < 0.95,
        "engine turned with its binder: {rest:?} -> {engine:?}"
    );
}
