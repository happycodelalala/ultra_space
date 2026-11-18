//! Ultra Space: Bevy front end.
//!
//! Game logic lives in the engine-independent `sim_*` crates; this crate only renders, handles
//! input and wires plugins together.

mod blocks;
mod builder;
mod camera;
mod capture;
mod clouds;
mod debug_overlay;
mod distant;
mod hud;
mod inspector;
mod meeting;
mod part_models;
mod people;
mod planet_maps;
mod save;
mod script;
mod settlements;
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
mod weather;
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
                .set(bevy::asset::AssetPlugin {
                    // Characters load through Bevy's glTF loader from the game's assets.
                    file_path: universe::assets_root()
                        .map_or_else(|| "assets".into(), |p| p.to_string_lossy().into_owned()),
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
            settlements::SettlementsPlugin,
            people::PeoplePlugin,
            meeting::MeetingPlugin,
            distant::DistantPlugin,
            blocks::BlocksPlugin,
            weather::WeatherPlugin,
            clouds::CloudsPlugin,
        ))
        .add_systems(Startup, log_startup)
        .add_systems(Update, window_size)
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

/// Debug: `ULTRA_SPACE_WINDOW=<width>x<height>` sets the window's inner size in physical pixels
/// (whatever the display's scaling), at the screen's top left; test rounds use the
/// developer's screen (2560 × 1600 less the title bar).
fn window_size(
    mut windows: Query<&mut Window, With<bevy::window::PrimaryWindow>>,
    mut done: Local<bool>,
) {
    if *done {
        return;
    }
    *done = true;
    let Some((w, h)) = std::env::var("ULTRA_SPACE_WINDOW").ok().and_then(|v| {
        let (w, h) = v.split_once(['x', 'X'])?;
        Some((w.trim().parse::<u32>().ok()?, h.trim().parse::<u32>().ok()?))
    }) else {
        return;
    };
    for mut window in &mut windows {
        window.resolution.set_physical_resolution(w, h);
        window.position = bevy::window::WindowPosition::At(IVec2::ZERO);
        info!("window: {w} x {h} physical pixels");
    }
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
