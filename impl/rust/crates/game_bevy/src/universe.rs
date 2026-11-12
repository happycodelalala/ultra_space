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
    let system = Arc::new(StarSystem::from_path(&path).map_err(|e| e.to_string())?);
    let catalog = PartCatalog::load_dir(dir.join("data/parts")).map_err(|e| e.to_string())?;
    let design = VehicleDesign::from_path(
        dir.join(format!("data/vehicles/{START_DESIGN}.json")),
        &catalog,
    )
    .map_err(|e| e.to_string())?;
    let sim = build_sim(system.clone());
    Ok(Loaded {
        system,
        sim,
        vehicle: Vehicle::from_design(&design),
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
            commands.insert_resource(Universe(loaded.system));
            commands.insert_resource(crate::sim::Sim(loaded.sim));
            commands.insert_resource(StartingVehicle(loaded.vehicle));
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
    fn loads_halcyon_and_the_starting_ship() {
        let loaded = load(crate::sim::build_simulation).unwrap();
        assert_eq!(loaded.sim.regions.len(), loaded.system.bodies.len());
        assert!(!loaded.vehicle.parts.is_empty());
    }
}
