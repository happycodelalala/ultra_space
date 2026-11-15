//! Loading the universe (Batch 1, task 1.17).
//!
//! At startup a worker thread loads the star system and the starting ship from `content/` and
//! builds the simulation, while the main thread keeps rendering (a "Loading" line). When it is
//! done the resources are inserted and the game enters [`GameState::Running`]; every game
//! system runs in [`InGame`], and world setup runs on entering that state.
//!
//! The same path is meant for later loads (other star systems, procedural planets): heavy work
//! on workers, the main thread only polls, so no frame exceeds the hitch budget (overview
//! §14.6).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, poll_once};
use sim_core::{Simulation, StarSystem};
use sim_vehicle::{PartCatalog, Vehicle, VehicleDesign};

/// The star system loaded at startup (Halcyon for now), shared with the simulation's regions.
#[derive(Resource)]
pub struct Universe(pub Arc<StarSystem>);

/// The vehicle the player starts in, loaded with the universe.
#[derive(Resource)]
pub struct StartingVehicle(pub Vehicle);

/// Every vehicle design in `content/data/vehicles`, by id.
#[derive(Resource, Default)]
pub struct Designs(pub std::collections::BTreeMap<String, VehicleDesign>);

/// The part catalog (the builder's palette).
#[derive(Resource)]
pub struct Catalog(pub PartCatalog);

/// The content director (living-planet.md): generated text and choices, answered from the
/// recorded responses in `content/lore/` until the LLM gateway.
#[derive(Resource, Clone)]
pub struct Lore(pub Arc<ai_gateway::ContentDirector>);

/// How each people settles its land (content/data/settlement_patterns).
#[derive(Resource)]
pub struct Patterns(pub Arc<sim_society::SettlementPatterns>);

const STAR_SYSTEM_FILE: &str = "data/star_systems/halcyon.json";
const START_DESIGN: &str = "kestrel";

#[derive(States, Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum GameState {
    #[default]
    Loading,
    Running,
}

/// Game systems that need the universe; they only run in [`GameState::Running`].
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct InGame;

pub struct UniversePlugin;

impl Plugin for UniversePlugin {
    fn build(&self, app: &mut App) {
        app.init_state::<GameState>()
            .configure_sets(Update, InGame.run_if(in_state(GameState::Running)))
            .add_systems(Startup, start_loading)
            .add_systems(Update, finish_loading.run_if(in_state(GameState::Loading)));
    }
}

/// Everything the worker produces.
struct Loaded {
    system: Arc<StarSystem>,
    sim: Simulation,
    vehicle: Vehicle,
    designs: Designs,
    catalog: PartCatalog,
    lore: Arc<ai_gateway::ContentDirector>,
    patterns: Arc<sim_society::SettlementPatterns>,
    /// Each society generated: body, people, settlements.
    societies: Vec<(usize, u32, usize)>,
    /// Settlement layouts: buildings placed, and the time taken (ms).
    layouts: (usize, f64),
    /// The demo village (block world), and the time taken to set it up (ms).
    blocks: Option<crate::blocks::BlockWorld>,
    village_ms: f64,
    seconds: f64,
}

#[derive(Resource)]
struct LoadTask(Task<Result<Loaded, String>>, Instant);

#[derive(Component)]
struct LoadingText;

fn load(build_sim: fn(Arc<StarSystem>) -> Simulation) -> Result<Loaded, String> {
    let start = Instant::now();
    let dir = content_dir()
        .ok_or("content folder not found; set ULTRA_SPACE_CONTENT to the repo's content/ folder")?;
    let path = dir.join(STAR_SYSTEM_FILE);
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let text = match std::env::var("ULTRA_SPACE_BODY_RADIUS") {
        Ok(spec) => with_radius_override(&text, &spec)?,
        Err(_) => text,
    };
    let system = Arc::new(StarSystem::from_json_str(&text).map_err(|e| e.to_string())?);
    let catalog = PartCatalog::load_dir(dir.join("data/parts")).map_err(|e| e.to_string())?;
    let mut designs = Designs::default();
    let vehicles_dir = dir.join("data/vehicles");
    let entries =
        std::fs::read_dir(&vehicles_dir).map_err(|e| format!("{}: {e}", vehicles_dir.display()))?;
    for path in entries.filter_map(|e| e.ok().map(|e| e.path())) {
        if path.extension().is_some_and(|x| x == "json") {
            let design = VehicleDesign::from_path(&path, &catalog)
                .map_err(|e| format!("{}: {e}", path.display()))?;
            designs.0.insert(design.id.clone(), design);
        }
    }
    let design = designs
        .0
        .get(START_DESIGN)
        .ok_or(format!("no {START_DESIGN} design"))?;
    let mut sim = build_sim(system.clone());
    // The peoples of every body (generated from the data, the seeds and the director).
    let provider =
        ai_gateway::ReplayProvider::load_dir(dir.join("lore")).map_err(|e| e.to_string())?;
    let lore = Arc::new(ai_gateway::ContentDirector::new(Box::new(provider)));
    let patterns = Arc::new(sim_society::SettlementPatterns::load_dir(
        &dir.join("data/settlement_patterns"),
    )?);
    let societies = sim_society::aggregate::install(&mut sim, &lore, &patterns);
    // Their settlements on the ground: the building kits; layouts and their foundations are
    // made as the player comes near (task 4b.5).
    let laid = Instant::now();
    let kits = match buildings_dir() {
        Some(dir) => sim_society::Kits::load_dir(&dir)?,
        None => {
            warn!("building kits not found: settlements have no buildings");
            sim_society::Kits::default()
        }
    };
    sim_society::layout::install(&mut sim, Arc::new(kits));
    // The demo village's block ground and models (Batch 5).
    let village = Instant::now();
    let meridian = system
        .bodies
        .iter()
        .position(|b| b.id == crate::ship::START_BODY);
    let blocks = match meridian {
        Some(body) => crate::blocks::load(&mut sim, &system, body)?,
        None => None,
    };
    let village_ms = village.elapsed().as_secs_f64() * 1e3;
    let buildings = 0;
    let layouts = (buildings, laid.elapsed().as_secs_f64() * 1e3);
    Ok(Loaded {
        system,
        sim,
        vehicle: Vehicle::from_design(design),
        designs,
        catalog,
        lore,
        patterns,
        societies,
        layouts,
        blocks,
        village_ms,
        seconds: start.elapsed().as_secs_f64(),
    })
}

fn start_loading(mut commands: Commands) {
    let task = AsyncComputeTaskPool::get().spawn(async { load(crate::sim::build_simulation) });
    commands.insert_resource(LoadTask(task, Instant::now()));
    commands.spawn((
        LoadingText,
        Text::new("Loading Halcyon..."),
        TextFont {
            font_size: FontSize::Px(22.0),
            ..default()
        },
        Node {
            position_type: PositionType::Absolute,
            bottom: px(40),
            left: px(40),
            ..default()
        },
    ));
}

fn finish_loading(
    mut commands: Commands,
    task: Option<ResMut<LoadTask>>,
    mut text: Query<(Entity, &mut Text), With<LoadingText>>,
    mut next: ResMut<NextState<GameState>>,
) {
    let Some(mut task) = task else { return };
    let Some(result) = block_on(poll_once(&mut task.0)) else {
        return;
    };
    let waited = task.1.elapsed().as_secs_f64();
    commands.remove_resource::<LoadTask>();
    match result {
        Ok(loaded) => {
            info!(
                "loaded star system {} ({} bodies), {} regions at {:.0} Hz: {:.0} ms on a worker \
                 ({:.0} ms until ready)",
                loaded.system.name,
                loaded.system.bodies.len(),
                loaded.sim.regions.len(),
                loaded.sim.clock().tick_hz(),
                loaded.seconds * 1e3,
                waited * 1e3
            );
            commands.insert_resource(Universe(loaded.system.clone()));
            commands.insert_resource(crate::sim::Sim(loaded.sim));
            commands.insert_resource(StartingVehicle(loaded.vehicle));
            commands.insert_resource(loaded.designs);
            commands.insert_resource(Catalog(loaded.catalog));
            for (body, people, settlements) in &loaded.societies {
                info!(
                    "society of {}: {settlements} settlements, {people} people",
                    loaded.system.bodies[*body].name
                );
            }
            info!(
                "settlements ready to lay out on approach ({} buildings so far, {:.0} ms)",
                loaded.layouts.0, loaded.layouts.1
            );
            if let Some(blocks) = loaded.blocks {
                info!("demo village ready ({:.0} ms)", loaded.village_ms);
                commands.insert_resource(blocks);
            }
            commands.insert_resource(Lore(loaded.lore));
            commands.insert_resource(Patterns(loaded.patterns));
            for (entity, _) in &mut text {
                commands.entity(entity).despawn();
            }
            next.set(GameState::Running);
        }
        Err(e) => {
            error!("loading failed: {e}");
            for (_, mut t) in &mut text {
                t.0 = format!("Loading failed: {e}");
            }
        }
    }
}

/// Debug (task 2.13, comparing planet sizes on foot): `ULTRA_SPACE_BODY_RADIUS=<body id>:<km>`
/// changes one body's radius before the star system is built. Surface gravity stays as in the
/// data, so only the size changes.
pub fn with_radius_override(json: &str, spec: &str) -> Result<String, String> {
    let (id, km) = spec
        .split_once(':')
        .ok_or("ULTRA_SPACE_BODY_RADIUS must look like meridian:300")?;
    let km: f64 = km
        .trim()
        .parse()
        .map_err(|_| format!("bad radius in ULTRA_SPACE_BODY_RADIUS: {km}"))?;
    let mut value: serde_json::Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let body = value["bodies"]
        .as_array_mut()
        .and_then(|bodies| bodies.iter_mut().find(|b| b["id"] == id.trim()))
        .ok_or_else(|| format!("ULTRA_SPACE_BODY_RADIUS: no body {id:?}"))?;
    body["radius_km"] = serde_json::json!(km);
    info!("debug: {} radius set to {km} km", id.trim());
    Ok(value.to_string())
}

/// The root of the assets Bevy loads (characters): `ULTRA_SPACE_ASSETS`, else an `assets`
/// folder with `characters/` next to the executable (exported) or the repo's `assets/export`,
/// found from the executable or the working directory upward.
pub fn assets_root() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("ULTRA_SPACE_ASSETS") {
        return Some(PathBuf::from(dir));
    }
    let starts = [std::env::current_exe().ok(), std::env::current_dir().ok()];
    starts.into_iter().flatten().find_map(|start| {
        start.ancestors().find_map(|dir| {
            ["assets", "assets/export"]
                .iter()
                .map(|sub| dir.join(sub))
                .find(|d| d.join("characters").is_dir())
        })
    })
}

/// The building kits' folder: `ULTRA_SPACE_BUILDINGS`, else `assets/buildings` (exported) or
/// `assets/export/buildings` (repo) next to the executable or the working directory or any of
/// their parents.
pub fn buildings_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("ULTRA_SPACE_BUILDINGS") {
        return Some(PathBuf::from(dir));
    }
    let starts = [std::env::current_exe().ok(), std::env::current_dir().ok()];
    starts.into_iter().flatten().find_map(|start| {
        start.ancestors().find_map(|dir| {
            ["assets/buildings", "assets/export/buildings"]
                .iter()
                .map(|sub| dir.join(sub))
                .find(|d| d.is_dir())
        })
    })
}

/// Finds the `content/` folder: `ULTRA_SPACE_CONTENT`, or a `content/` folder next to the
/// executable or in any parent of it or of the working directory (repo checkout).
pub fn content_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("ULTRA_SPACE_CONTENT") {
        return Some(PathBuf::from(dir));
    }
    let starts = [std::env::current_exe().ok(), std::env::current_dir().ok()];
    starts.into_iter().flatten().find_map(|start| {
        start
            .ancestors()
            .map(|dir| dir.join("content"))
            .find(|c| is_content_dir(c))
    })
}

fn is_content_dir(dir: &Path) -> bool {
    dir.join(STAR_SYSTEM_FILE).is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn radius_override_changes_only_that_body() {
        let dir = content_dir().unwrap();
        let text = std::fs::read_to_string(dir.join(STAR_SYSTEM_FILE)).unwrap();
        let changed = with_radius_override(&text, "meridian:300").unwrap();
        let sys = StarSystem::from_json_str(&changed).unwrap();
        let original = StarSystem::from_json_str(&text).unwrap();
        let m = sys.body("meridian").unwrap();
        assert!((m.radius - 300_000.0).abs() < 1e-6);
        assert!(
            (m.surface_gravity() - original.body("meridian").unwrap().surface_gravity()).abs()
                < 1e-9
        );
        assert_eq!(
            sys.body("lantern").unwrap().radius,
            original.body("lantern").unwrap().radius
        );
        assert!(with_radius_override(&text, "nowhere:300").is_err());
        assert!(with_radius_override(&text, "meridian").is_err());
    }

    #[test]
    fn loads_halcyon_and_the_starting_ship() {
        let loaded = load(crate::sim::build_simulation).unwrap();
        assert_eq!(loaded.sim.regions.len(), loaded.system.bodies.len());
        assert!(!loaded.vehicle.parts.is_empty());
    }
}
