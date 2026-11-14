//! Ultra Space: Bevy front end.
//!
//! Game logic lives in the engine-independent `sim_*` crates; this crate only renders, handles
//! input and wires plugins together.

mod builder;
mod camera;
mod capture;
mod debug_overlay;
mod hud;
mod inspector;
mod part_models;
mod planet_maps;
mod save;
mod script;
mod ship;
mod sim;
mod sky;
mod space_render;
mod terrain;
mod terrain_material;
mod universe;
mod vehicle_visual;
mod walk;
mod warmup;
mod workers;

use bevy::log::{BoxedLayer, LogPlugin};
use bevy::prelude::*;

/// Default log filter. Override at runtime with `RUST_LOG`, e.g. `RUST_LOG=debug`.
/// Log file written next to the executable on every run.
const LOG_FILE: &str = "ultra_space.log";

const LOG_FILTER: &str = "info,wgpu=error,naga=warn,ultra_space=debug,sim_core=debug";

fn main() {
    App::new()
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "Ultra Space".into(),
                        ..default()
                    }),
                    ..default()
                })
                .set(LogPlugin {
                    filter: LOG_FILTER.into(),
                    custom_layer: log_file_layer,
                    ..default()
                }),
        )
        // Loads the universe on a worker and owns the Loading -> Running game state.
        .add_plugins(universe::UniversePlugin)
        .add_plugins((
            sim::SimPlugin,
            camera::SpaceCameraPlugin,
            space_render::SpaceRenderPlugin,
            terrain::TerrainPlugin,
            terrain_material::TerrainMaterialPlugin,
            planet_maps::PlanetMapsPlugin,
            sky::SkyPlugin,
            ship::ShipPlugin,
            debug_overlay::DebugOverlayPlugin,
            hud::HudPlugin,
            inspector::InspectorPlugin,
            capture::CapturePlugin,
            script::ScriptPlugin,
            walk::WalkPlugin,
            warmup::WarmupPlugin,
        ))
        .add_plugins((
            vehicle_visual::VehicleVisualPlugin,
            builder::BuilderPlugin,
            save::SavePlugin,
        ))
        .add_systems(Startup, log_startup)
        .run();
}

/// Also writes the log to `ultra_space.log` next to the executable (or in the working
/// directory), replaced on every run, so testers can send one file.
fn log_file_layer(_app: &mut App) -> Option<BoxedLayer> {
    use bevy::log::tracing_subscriber::{Layer, fmt};
    let dir = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    // `ULTRA_SPACE_LOG` names another log file (parallel test runs share one folder).
    let path = std::env::var_os("ULTRA_SPACE_LOG").map_or_else(|| dir.join(LOG_FILE), Into::into);
    let file = std::fs::File::create(&path).ok()?;
    Some(
        fmt::layer()
            .with_ansi(false)
            .with_writer(std::sync::Mutex::new(file))
            .boxed(),
    )
}

fn log_startup() {
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    info!(
        "Ultra Space {} ({profile} build), {threads} logical CPUs",
        env!("CARGO_PKG_VERSION")
    );
}
