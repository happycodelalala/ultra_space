//! Saving and loading (task 3.13): a save restores the same objects (identities, states,
//! parts, joint poses, flight settings), terrain deltas, universe time and ID allocator; a part
//! that no longer exists is dropped with what hangs on it; a save from a newer game is refused.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use glam::{DQuat, DVec3};
use sim_core::{ObjectId, ObjectState, Simulation, StarSystem};
use sim_planet::Pad;
use sim_vehicle::flight::{RegionTerrain, add_flight_systems, install_terrain, region_env};
use sim_vehicle::gait::Walking;
use sim_vehicle::joints::JointState;
use sim_vehicle::save::{ObjectKind, SAVE_FORMAT, SaveError, SaveFile, capture, restore};
use sim_vehicle::{
    ArcadeFlight, AssistMode, Cruise, FlightControl, FlightTelemetry, PartCatalog, Vehicle,
    VehicleBody, VehicleDesign, Walker, WalkerControl,
};

fn content() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../../content/data")
}

struct World {
    sim: Simulation,
    sys: Arc<StarSystem>,
    catalog: PartCatalog,
    planet: usize,
}

fn world() -> World {
    let catalog = PartCatalog::load_dir(content().join("parts")).unwrap();
    let sys = Arc::new(StarSystem::from_path(content().join("star_systems/halcyon.json")).unwrap());
    let planet = sys.bodies.iter().position(|b| b.id == "meridian").unwrap();
    let mut sim = Simulation::for_star_system(sys.clone(), |_, s| add_flight_systems(s));
    sim.objects.register_transferable::<Vehicle>();
    sim.objects.register_transferable::<VehicleBody>();
    sim.objects.register_transferable::<FlightControl>();
    sim.objects.register_transferable::<FlightTelemetry>();
    sim.objects.register_transferable::<Cruise>();
    sim.objects.register_transferable::<ArcadeFlight>();
    sim.objects.register_transferable::<Walker>();
    sim.objects.register_transferable::<WalkerControl>();
    sim.objects.register_transferable::<Walking>();
    sim.objects.register_transferable::<JointState>();
    install_terrain(&mut sim);
    World {
        sim,
        sys,
        catalog,
        planet,
    }
}

/// A state `height` m above the ground at the point under `dir` (surface direction), upright,
/// moving with the turning ground.
fn on_ground(w: &World, dir: DVec3, height: f64) -> ObjectState {
    let terrain = w
        .sim
        .regions
        .get(w.planet)
        .unwrap()
        .world
        .get_resource::<RegionTerrain>()
        .cloned();
    let env = region_env(&w.sys, w.planet, 0.0, terrain.as_ref());
    let up = dir.normalize();
    let p = up * (env.radius + env.ground_height(up * env.radius) + height);
    let north = up.cross(DVec3::Y.cross(up)).normalize();
    let rotation = DQuat::from_mat3(&glam::DMat3::from_cols(north.cross(up), up, -north));
    let mut state = ObjectState::at_rest(w.sys.bodies[w.planet].inertial_frame, p, rotation);
    state.state.velocity = env.spin.cross(p);
    state.state.angular_velocity = env.spin;
    state
}

fn spawn_design(w: &mut World, id: &str, state: ObjectState, control: FlightControl) -> ObjectId {
    let design =
        VehicleDesign::from_path(content().join(format!("vehicles/{id}.json")), &w.catalog)
            .unwrap();
    let vehicle = Vehicle::from_design(&design);
    let body = VehicleBody::of(&vehicle).unwrap();
    w.sim
        .objects
        .spawn(
            &mut w.sim.regions,
            w.planet,
            state,
            (
                vehicle,
                body,
                control,
                FlightTelemetry::default(),
                Cruise::default(),
            ),
        )
        .unwrap()
}

fn run(sim: &mut Simulation, seconds: f64) {
    for _ in 0..(seconds * 30.0) as u32 {
        sim.advance(Duration::from_secs_f64(1.0 / 30.0));
    }
}

/// A busy world: a rover and the heavy frame on the ground (the frame mid-way through a pose
/// change), a ship in arcade flight with rotation assist, a walker, a landing pad.
fn busy_world() -> World {
    let mut w = world();
    let rover = on_ground(&w, DVec3::new(0.3, 0.2, 1.0), 1.0);
    spawn_design(&mut w, "rover", rover, FlightControl::default());
    let frame = on_ground(&w, DVec3::new(0.31, 0.2, 1.0), 12.0);
    let frame_id = spawn_design(&mut w, "heavy_frame", frame, FlightControl::default());
    let ship = on_ground(&w, DVec3::new(0.32, 0.2, 1.0), 300.0);
    let ship_id = spawn_design(
        &mut w,
        "kestrel",
        ship,
        FlightControl {
            assist: AssistMode::Rotation,
            ..Default::default()
        },
    );
    let walker = on_ground(&w, DVec3::new(0.305, 0.2, 1.0), 0.1);
    w.sim
        .objects
        .spawn(
            &mut w.sim.regions,
            w.planet,
            walker,
            (Walker::default(), WalkerControl::default()),
        )
        .unwrap();
    // A landing pad.
    let region = w.sim.regions.get_mut(w.planet).unwrap();
    let terrain = region
        .world
        .get_resource::<RegionTerrain>()
        .unwrap()
        .0
        .clone()
        .unwrap();
    let center = DVec3::new(0.3, 0.2, 1.0).normalize();
    let pad = Pad {
        center,
        radius: 30.0,
        blend: 20.0,
        height: terrain.height(center, 1.0),
    };
    region
        .world
        .insert_resource(RegionTerrain(Some(Arc::new(terrain.with_pad(pad)))));
    run(&mut w.sim, 1.0);
    // Arcade flight for the ship, a pose change under way for the frame.
    let loc = w.sim.objects.location(ship_id).unwrap();
    let region = w.sim.regions.get_mut(loc.region).unwrap();
    region
        .world
        .entity_mut(loc.entity)
        .insert(ArcadeFlight { speed: 42.0 });
    let loc = w.sim.objects.location(frame_id).unwrap();
    let region = w.sim.regions.get_mut(loc.region).unwrap();
    let mut joints = region.world.get_mut::<JointState>(loc.entity).unwrap();
    for t in joints.targets.iter_mut() {
        *t += 0.3;
    }
    run(&mut w.sim, 0.5);
    w
}

#[test]
fn a_save_restores_everything_as_it_was() {
    let w = busy_world();
    let saved = capture(&w.sim, &w.sys);
    assert_eq!(saved.format, SAVE_FORMAT);
    assert_eq!(saved.objects.len(), 4);
    assert_eq!(saved.terrain.len(), 1);
    let vehicles: Vec<_> = saved
        .objects
        .iter()
        .filter_map(|o| match &o.kind {
            ObjectKind::Vehicle(v) => Some(v),
            _ => None,
        })
        .collect();
    assert!(
        vehicles.iter().any(|v| v.joints.is_some()),
        "joint state saved"
    );
    assert!(vehicles.iter().any(|v| v.arcade_speed == Some(42.0)));
    assert!(vehicles.iter().any(|v| v.assist == "rotation"));

    // Through a file, into a fresh world (with something else in it already).
    let text = saved.to_json();
    let mut fresh = world();
    let other = on_ground(&fresh, DVec3::new(-1.0, 0.0, 0.2), 1.0);
    spawn_design(&mut fresh, "rover", other, FlightControl::default());
    let report = restore(
        &mut fresh.sim,
        &fresh.sys,
        &fresh.catalog,
        &SaveFile::from_json(&text).unwrap(),
    );
    assert!(report.problems.is_empty(), "{:?}", report.problems);
    assert_eq!(report.terrain_changed, vec![fresh.planet]);
    let again = capture(&fresh.sim, &fresh.sys);
    assert_eq!(again, saved, "the restored world saves the same");

    // IDs keep counting where they left off, and the restored world runs.
    let next = saved.next_object_id;
    let new = on_ground(&fresh, DVec3::new(0.0, 1.0, 0.1), 1.0);
    let id = spawn_design(&mut fresh, "rover", new, FlightControl::default());
    assert!(id.0 >= next, "{id} reused an id below {next}");
    run(&mut fresh.sim, 1.0);
}

#[test]
fn a_missing_part_is_dropped_with_what_hangs_on_it() {
    let w = busy_world();
    let mut saved = capture(&w.sim, &w.sys);
    // The heavy frame's left thigh is no longer in the game.
    let (count, name) = saved
        .objects
        .iter_mut()
        .find_map(|o| match &mut o.kind {
            ObjectKind::Vehicle(v) if v.joints.is_some() => {
                let parts = v.design["parts"].as_array_mut().unwrap();
                let thigh = parts
                    .iter_mut()
                    .find(|p| p["id"].as_str().unwrap().starts_with("thigh"))
                    .unwrap();
                let name = thigh["id"].as_str().unwrap().to_string();
                thigh["part"] = "frame.retired_thigh".into();
                Some((parts.len(), name))
            }
            _ => None,
        })
        .unwrap();
    let mut fresh = world();
    let report = restore(&mut fresh.sim, &fresh.sys, &fresh.catalog, &saved);
    assert!(
        report
            .problems
            .iter()
            .any(|p| p.contains("frame.retired_thigh")),
        "{:?}",
        report.problems
    );
    assert!(report.problems.len() > 1, "the parts below it go too");
    let again = capture(&fresh.sim, &fresh.sys);
    let restored = again
        .objects
        .iter()
        .find_map(|o| match &o.kind {
            ObjectKind::Vehicle(v) if v.design["parts"].as_array().unwrap().len() < count => {
                Some(v)
            }
            _ => None,
        })
        .expect("the frame is still there, smaller");
    assert!(
        !restored.design["parts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["id"] == name.as_str())
    );
    assert_eq!(again.objects.len(), 4, "{:?}", report.problems);
}

#[test]
fn a_save_from_a_newer_game_is_refused() {
    let w = world();
    let mut saved = serde_json::to_value(capture(&w.sim, &w.sys)).unwrap();
    saved["format"] = (SAVE_FORMAT + 1).into();
    assert!(matches!(
        SaveFile::from_json(&saved.to_string()),
        Err(SaveError::TooNew(_))
    ));
}
