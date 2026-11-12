//! Automated screenshot capture for visual checks.
//!
//! When `ULTRA_SPACE_CAPTURE=<path.png>` is set, the app renders until the scene has settled
//! (at least `CAPTURE_AFTER_FRAMES` frames and `CAPTURE_AFTER_SECONDS` of real time, so
//! background generation can finish however fast frames are), saves a screenshot of the window
//! to that path, and exits. For longer, scripted runs see `script`.

use std::path::PathBuf;
use std::time::Duration;

use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};

/// Frames to render before capturing, so stats and assets have settled.
const CAPTURE_AFTER_FRAMES: u32 = 120;
/// Real time to run before capturing (background generation must have finished).
const CAPTURE_AFTER_SECONDS: f64 = 8.0;
/// Give up if the screenshot hasn't been written this long after it was requested.
const TIMEOUT: Duration = Duration::from_secs(60);

pub struct CapturePlugin;

impl Plugin for CapturePlugin {
    fn build(&self, app: &mut App) {
        if let Some(path) = std::env::var_os("ULTRA_SPACE_CAPTURE") {
            let path = PathBuf::from(path);
            info!("capture mode: will save a screenshot to {}", path.display());
            // A stale file would make the "written" check pass immediately.
            let _ = std::fs::remove_file(&path);
            app.insert_resource(CaptureTarget {
                path,
                frames: 0,
                requested_at: None,
            })
            .add_systems(Update, capture);
        }
    }
}

#[derive(Resource)]
struct CaptureTarget {
    path: PathBuf,
    frames: u32,
    requested_at: Option<Duration>,
}

fn capture(mut commands: Commands, mut target: ResMut<CaptureTarget>, real: Res<Time<Real>>) {
    target.frames += 1;
    let now = real.elapsed();
    match target.requested_at {
        None if target.frames >= CAPTURE_AFTER_FRAMES
            && now.as_secs_f64() >= CAPTURE_AFTER_SECONDS =>
        {
            target.requested_at = Some(now);
            commands
                .spawn(Screenshot::primary_window())
                .observe(save_to_disk(target.path.clone()));
        }
        Some(_) if target.path.exists() => {
            info!("capture saved to {}", target.path.display());
            commands.write_message(AppExit::Success);
        }
        Some(at) if now - at > TIMEOUT => {
            error!("capture timed out; no screenshot written");
            commands.write_message(AppExit::error());
        }
        _ => {}
    }
}
