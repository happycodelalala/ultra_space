//! Automated screenshot capture for visual checks.
//!
//! When `ULTRA_SPACE_CAPTURE=<path.png>` is set, the app renders a few seconds of frames, saves
//! a screenshot of the primary window to that path, and exits. Used to verify visual work
//! without a person watching the screen (e.g. from WSL, which renders in software).

use std::path::PathBuf;

use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};

/// Frames to render before capturing, so stats and assets have settled.
const CAPTURE_AFTER_FRAMES: u32 = 120;

/// Give up if the screenshot hasn't been written after this many extra frames.
const TIMEOUT_FRAMES: u32 = 600;

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
                requested: false,
            })
            .add_systems(Update, capture);
        }
    }
}

#[derive(Resource)]
struct CaptureTarget {
    path: PathBuf,
    frames: u32,
    requested: bool,
}

fn capture(mut commands: Commands, mut target: ResMut<CaptureTarget>) {
    target.frames += 1;
    if !target.requested && target.frames >= CAPTURE_AFTER_FRAMES {
        target.requested = true;
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(target.path.clone()));
    } else if target.requested && target.path.exists() {
        info!("capture saved to {}", target.path.display());
        commands.write_message(AppExit::Success);
    } else if target.frames >= CAPTURE_AFTER_FRAMES + TIMEOUT_FRAMES {
        error!("capture timed out; no screenshot written");
        commands.write_message(AppExit::error());
    }
}
