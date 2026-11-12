//! Ultra Space: Bevy front end.
//!
//! Game logic lives in the engine-independent `sim_*` crates; this crate only renders, handles
//! input and wires plugins together.

mod camera;
mod capture;
mod debug_overlay;
mod hud;
mod inspector;
mod ship;
mod sim;
mod sky;
mod space_render;
mod universe;

use bevy::log::LogPlugin;
use bevy::prelude::*;

/// Default log filter. Override at runtime with `RUST_LOG`, e.g. `RUST_LOG=debug`.
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
                    ..default()
                }),
        )
        // Loads the universe on a worker and owns the Loading -> Running game state.
        .add_plugins(universe::UniversePlugin)
        .add_plugins((
            sim::SimPlugin,
            camera::SpaceCameraPlugin,
            space_render::SpaceRenderPlugin,
            sky::SkyPlugin,
            ship::ShipPlugin,
            debug_overlay::DebugOverlayPlugin,
            hud::HudPlugin,
            inspector::InspectorPlugin,
            capture::CapturePlugin,
        ))
        .add_systems(Startup, log_startup)
        .run();
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
