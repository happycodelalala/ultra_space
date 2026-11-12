//! Loads the star system from `content/` into a resource.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use bevy::prelude::*;
use sim_core::StarSystem;

/// The star system loaded at startup (Halcyon for now), shared with the simulation's regions.
#[derive(Resource)]
pub struct Universe(pub Arc<StarSystem>);

const STAR_SYSTEM_FILE: &str = "data/star_systems/halcyon.json";

pub struct UniversePlugin;

impl Plugin for UniversePlugin {
    fn build(&self, app: &mut App) {
        let dir = content_dir().unwrap_or_else(|| {
            panic!(
                "content folder not found; set ULTRA_SPACE_CONTENT to the repo's content/ folder"
            )
        });
        let path = dir.join(STAR_SYSTEM_FILE);
        // Small file; background loading of systems comes with task 1.17.
        let system = StarSystem::from_path(&path).unwrap_or_else(|e| panic!("{e}"));
        info!(
            "loaded star system {} ({} bodies) from {}",
            system.name,
            system.bodies.len(),
            path.display()
        );
        app.insert_resource(Universe(Arc::new(system)));
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
