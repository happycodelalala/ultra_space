//! Debug overlay: frame time, simulation tick time and counts. Toggle with F3.

use std::time::Duration;

use bevy::prelude::*;
use sim_core::RollingStats;

use crate::sim::Sim;

/// How often the overlay text is rebuilt (formatting every frame is wasted work).
const REFRESH_INTERVAL: Duration = Duration::from_millis(250);

/// Frame-time window (about 2 seconds at 60 fps).
const FRAME_WINDOW: usize = 120;

pub struct DebugOverlayPlugin;

impl Plugin for DebugOverlayPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(FrameStats {
            frames: RollingStats::new(FRAME_WINDOW),
            since_refresh: REFRESH_INTERVAL,
        })
        .add_systems(Startup, spawn_overlay)
        .add_systems(
            Update,
            (record_frame, toggle_overlay, update_overlay).chain(),
        );
    }
}

#[derive(Component)]
struct OverlayText;

#[derive(Resource)]
struct FrameStats {
    frames: RollingStats,
    since_refresh: Duration,
}

fn spawn_overlay(mut commands: Commands) {
    commands.spawn((
        OverlayText,
        Text::new(""),
        TextFont {
            font_size: FontSize::Px(14.0),
            ..default()
        },
        TextColor(Color::WHITE),
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
        Node {
            position_type: PositionType::Absolute,
            top: px(8),
            left: px(8),
            padding: UiRect::all(px(6)),
            ..default()
        },
    ));
}

fn record_frame(real: Res<Time<Real>>, mut stats: ResMut<FrameStats>) {
    let dt = real.delta();
    stats.frames.push(dt);
    stats.since_refresh += dt;
}

fn toggle_overlay(
    keys: Res<ButtonInput<KeyCode>>,
    mut overlay: Query<&mut Visibility, With<OverlayText>>,
) {
    if keys.just_pressed(KeyCode::F3) {
        for mut visibility in &mut overlay {
            *visibility = match *visibility {
                Visibility::Hidden => Visibility::Visible,
                _ => Visibility::Hidden,
            };
        }
    }
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1e3
}

fn update_overlay(
    mut stats: ResMut<FrameStats>,
    mut sim: ResMut<Sim>,
    mut overlay: Query<(&mut Text, &Visibility), With<OverlayText>>,
) {
    if stats.since_refresh < REFRESH_INTERVAL {
        return;
    }
    stats.since_refresh = Duration::ZERO;
    let Ok((mut text, visibility)) = overlay.single_mut() else {
        return;
    };
    if *visibility == Visibility::Hidden {
        return;
    }

    let frame_avg = stats.frames.average().unwrap_or_default();
    let frame_max = stats.frames.max().unwrap_or_default();
    let fps = if frame_avg.is_zero() {
        0.0
    } else {
        1.0 / frame_avg.as_secs_f64()
    };

    let sim = &mut sim.0;
    let tick_avg = sim.tick_stats().average().unwrap_or_default();
    let tick_max = sim.tick_stats().max().unwrap_or_default();
    let budget = sim.budget_used().unwrap_or(0.0) * 100.0;
    let hz = sim.clock().tick_hz();
    let dropped = sim.clock().dropped().as_secs_f64();
    let total = sim.ticks_total();
    let galactic = sim.universe_time().galactic_standard();
    let warp = sim.time_warp();
    let regions = sim.regions.len();
    let entities = sim.regions.entity_count();

    text.0 = format!(
        "FPS {fps:.0}  frame {:.2} ms (max {:.2})\n\
         Sim {hz:.0} Hz  tick {:.3} ms (max {:.3})  budget {budget:.1}%\n\
         regions {regions}  entities {entities}  ticks {total}  dropped {dropped:.2} s\n\
         GST {galactic}  warp x{warp}  (, / . to change)\n\
         F3: toggle overlay",
        ms(frame_avg),
        ms(frame_max),
        ms(tick_avg),
        ms(tick_max),
    );
}
