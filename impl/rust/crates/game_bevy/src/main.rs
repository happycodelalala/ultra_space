//! Ultra Space: Bevy front end.
//!
//! Game logic lives in the engine-independent `sim_*` crates; this crate only renders, handles
//! input and wires plugins together.

mod capture;
mod debug_overlay;
mod sim;

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
        .add_plugins((
            sim::SimPlugin,
            debug_overlay::DebugOverlayPlugin,
            capture::CapturePlugin,
        ))
        .add_systems(Startup, (log_startup, scene.spawn()))
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

/// A placeholder "planet" and light, replaced in Batch 1.
fn scene() -> impl SceneList {
    bsn_list! {
        #Planet
        Mesh3d(asset_value(Sphere::new(1.0)))
        MeshMaterial3d::<StandardMaterial>(asset_value(Color::srgb_u8(90, 140, 200)))
        Transform::from_xyz(0.0, 0.0, 0.0)
        --
        DirectionalLight {
            shadow_maps_enabled: true,
        }
        Transform::from_xyz(4.0, 6.0, 4.0).looking_at(Vec3::ZERO, Vec3::Y)
        --
        Camera3d
        Transform::from_xyz(0.0, 1.0, 4.0).looking_at(Vec3::ZERO, Vec3::Y)
    }
}
