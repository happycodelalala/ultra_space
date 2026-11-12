//! Debug overlay: frame time, simulation tick time and counts. Hidden at start; F3 toggles.
//!
//! Also watches for **hitches** (frames over the 50 ms budget, overview §14.6) in every state:
//! each one is logged (rate-limited), and a startup summary is logged after the first seconds
//! of play.

use std::time::Duration;

use bevy::prelude::*;
use sim_core::RollingStats;

use crate::camera::SpaceCamera;
use crate::ship::{PilotState, PlayerShip, player_cruise_status, player_ship_state};
use crate::sim::Sim;
use crate::universe::{GameState, InGame, Universe};

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
        .init_resource::<Hitches>()
        .add_systems(Startup, spawn_overlay)
        .add_systems(Update, watch_hitches)
        .add_systems(
            Update,
            (record_frame, toggle_overlay, update_overlay)
                .chain()
                .in_set(InGame),
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
        // Hidden by default now that the HUD shows flight data; F3 shows it.
        Visibility::Hidden,
        Node {
            position_type: PositionType::Absolute,
            top: px(8),
            left: px(8),
            padding: UiRect::all(px(6)),
            ..default()
        },
    ));
}

/// Hitch budget: no frame should take longer (overview §14.6).
const HITCH_BUDGET: Duration = Duration::from_millis(50);
/// The startup summary covers this much play after loading.
const STARTUP_WINDOW: Duration = Duration::from_secs(10);
const HITCH_WARNING_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Resource, Default)]
struct Hitches {
    loading_worst: Duration,
    loading_frames: u32,
    running_since: Option<Duration>,
    startup_worst: Duration,
    startup_over: u32,
    reported: bool,
    last_warning: Option<Duration>,
}

fn watch_hitches(real: Res<Time<Real>>, state: Res<State<GameState>>, mut h: ResMut<Hitches>) {
    let dt = real.delta();
    let now = real.elapsed();
    match state.get() {
        GameState::Loading => {
            h.loading_worst = h.loading_worst.max(dt);
            h.loading_frames += 1;
        }
        GameState::Running => {
            let since = *h.running_since.get_or_insert(now);
            if !h.reported {
                h.startup_worst = h.startup_worst.max(dt);
                h.startup_over += u32::from(dt > HITCH_BUDGET);
                if now - since >= STARTUP_WINDOW {
                    h.reported = true;
                    info!(
                        "startup frames: worst {:.1} ms over {} loading frames, worst {:.1} ms \
                         in the first {} s of play, {} frames over the {} ms budget",
                        ms(h.loading_worst),
                        h.loading_frames,
                        ms(h.startup_worst),
                        STARTUP_WINDOW.as_secs(),
                        h.startup_over,
                        HITCH_BUDGET.as_millis()
                    );
                }
            }
        }
    }
    if dt > HITCH_BUDGET
        && h.last_warning
            .is_none_or(|t| now - t >= HITCH_WARNING_INTERVAL)
    {
        h.last_warning = Some(now);
        warn!("hitch: frame took {:.1} ms ({:?})", ms(dt), state.get());
    }
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

/// Distance in m, km or Mm/Gm, whichever reads best.
fn distance(m: f64) -> String {
    let a = m.abs();
    if a < 10_000.0 {
        format!("{m:.0} m")
    } else if a < 1.0e7 {
        format!("{:.0} km", m / 1_000.0)
    } else if a < 1.0e10 {
        format!("{:.1} Mm", m / 1.0e6)
    } else {
        format!("{:.2} Gm", m / 1.0e9)
    }
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1e3
}

fn update_overlay(
    mut stats: ResMut<FrameStats>,
    mut sim: ResMut<Sim>,
    camera: Res<SpaceCamera>,
    universe: Res<Universe>,
    pilot: Res<PilotState>,
    ship: Option<Res<PlayerShip>>,
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
    let flight_line = match ship
        .as_deref()
        .and_then(|ship| player_ship_state(sim, ship))
    {
        Some((region, _, telemetry, assist)) if pilot.flying => format!(
            "Flying near {}  assist {:?}  ref {:?}  speed {}/s  alt {}{}{}{}  (T assist, X brake, J cruise, V view, Tab free cam, 1-8 teleport, Esc {})",
            universe.0.bodies[region].name,
            assist,
            telemetry.reference,
            distance(telemetry.relative_velocity.length()),
            distance(telemetry.altitude),
            if telemetry.on_rails { "  ON RAILS" } else { "" },
            if telemetry.in_contact { "  LANDED" } else { "" },
            ship.as_deref()
                .map(|s| player_cruise_status(sim, s, &universe.0))
                .unwrap_or_default(),
            if pilot.mouse_free {
                "MOUSE FREE: click to capture"
            } else {
                "frees mouse"
            },
        ),
        _ => "Free camera  (Tab: fly the ship)".to_string(),
    };
    let near = &universe.0.bodies[camera.anchor];
    let altitude = camera.offset.length() - near.radius;
    let camera_line = format!(
        "Camera near {}  altitude {}  speed {}/s{}",
        near.name,
        distance(altitude),
        distance(camera.speed),
        if pilot.flying {
            ""
        } else {
            "  (1-8: jump to body)"
        }
    );
    let galactic = sim.universe_time().galactic_standard();
    let warp = sim.time_warp();
    let regions = sim.regions.len();
    let entities = sim.regions.entity_count();

    text.0 = format!(
        "FPS {fps:.0}  frame {:.2} ms (max {:.2})\n\
         Sim {hz:.0} Hz  tick {:.3} ms (max {:.3})  budget {budget:.1}%\n\
         regions {regions}  entities {entities}  ticks {total}  dropped {dropped:.2} s\n\
         GST {galactic}  warp x{warp}  (, / . to change)\n\
         {flight_line}\n\
         {camera_line}\n\
         F3: toggle overlay",
        ms(frame_avg),
        ms(frame_max),
        ms(tick_avg),
        ms(tick_max),
    );
}
