//! Player HUD (Batch 1, task 1.15): flight data, clocks and orbit lines.
//!
//! - Bottom center: reference body and frame, speed, vertical speed, altitude, apoapsis and
//!   periapsis, flight assist, cruise drive status, key hints.
//! - Top right: Galactic Standard Time, local solar time under the ship, time warp.
//! - Orbit lines: the ship's predicted path around its region body (bright) and the orbits of
//!   other bodies (faint), except the camera's body and its parents.
//!
//! H toggles the HUD; O cycles orbit lines: ship path, ship path and body orbits, none. The F3 debug overlay is separate.

use std::time::Duration;

use bevy::prelude::*;
use glam::DVec3;
use sim_core::time::{local_clock, local_solar_fraction};
use sim_core::{KeplerOrbit, StarSystem};
use sim_vehicle::Reference;

use crate::camera::{CameraSet, SpaceCamera};
use crate::ship::{
    PilotState, PlayerShip, ShipRenderState, player_arcade_speed, player_cruise_status,
    player_ship_state,
};
use crate::sim::{RenderTime, Sim};
use crate::space_render::body_poses;
use crate::universe::{InGame, Universe};
use crate::walk::{OnFoot, walker_status};

/// How often the HUD text is rebuilt.
const REFRESH_INTERVAL: Duration = Duration::from_millis(100);
/// Points per body orbit line.
const BODY_ORBIT_POINTS: usize = 256;
/// Most points on the ship's predicted path.
const PATH_MAX_POINTS: usize = 1_500;
/// Path step as a fraction of the local orbital time scale (about 1.1° of arc per step).
const PATH_STEP: f64 = 0.02;

const SHIP_PATH_COLOR: Color = Color::srgba(0.3, 0.9, 1.0, 0.9);
const BODY_ORBIT_COLOR: Color = Color::srgba(0.55, 0.65, 0.85, 0.25);

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(HudSettings {
            visible: true,
            orbits: OrbitLines::Ship,
        })
        .insert_resource(HudRefresh(REFRESH_INTERVAL))
        .add_systems(Startup, spawn_hud)
        .add_systems(
            Update,
            ((hud_keys, update_hud).chain(), draw_orbits.after(CameraSet)).in_set(InGame),
        );
    }
}

#[derive(Resource, Debug)]
pub struct HudSettings {
    pub visible: bool,
    pub orbits: OrbitLines,
}

/// Which orbit lines are drawn. Body orbits are off by default: seen from inside the plane of
/// the planets' orbits they are edge-on lines across the view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrbitLines {
    Ship,
    All,
    None,
}

#[derive(Resource)]
struct HudRefresh(Duration);

#[derive(Component)]
struct HudRoot;

#[derive(Component)]
struct FlightPanel;

#[derive(Component)]
struct TimePanel;

fn panel_text() -> impl Bundle {
    (
        Text::new(""),
        TextFont {
            font_size: FontSize::Px(16.0),
            ..default()
        },
        TextColor(Color::srgb(0.85, 0.95, 1.0)),
        BackgroundColor(Color::srgba(0.0, 0.02, 0.05, 0.55)),
    )
}

fn spawn_hud(mut commands: Commands) {
    commands
        .spawn((
            HudRoot,
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                ..default()
            },
        ))
        .with_children(|root| {
            root.spawn(Node {
                position_type: PositionType::Absolute,
                bottom: px(16),
                width: percent(100),
                justify_content: JustifyContent::Center,
                ..default()
            })
            .with_children(|row| {
                row.spawn((
                    FlightPanel,
                    panel_text(),
                    TextLayout::justify(Justify::Center),
                    Node {
                        padding: UiRect::axes(px(14), px(8)),
                        ..default()
                    },
                ));
            });
            root.spawn((
                TimePanel,
                panel_text(),
                TextLayout::justify(Justify::Right),
                Node {
                    position_type: PositionType::Absolute,
                    top: px(8),
                    right: px(8),
                    padding: UiRect::axes(px(10), px(6)),
                    ..default()
                },
            ));
        });
}

fn hud_keys(
    keys: Res<ButtonInput<KeyCode>>,
    mut settings: ResMut<HudSettings>,
    mut root: Query<&mut Visibility, With<HudRoot>>,
) {
    if keys.just_pressed(KeyCode::KeyH) {
        settings.visible = !settings.visible;
        for mut v in &mut root {
            *v = if settings.visible {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
        }
    }
    if keys.just_pressed(KeyCode::KeyO) {
        settings.orbits = match settings.orbits {
            OrbitLines::Ship => OrbitLines::All,
            OrbitLines::All => OrbitLines::None,
            OrbitLines::None => OrbitLines::Ship,
        };
    }
}

// ---------------------------------------------------------------------------------------------
// Formatting
// ---------------------------------------------------------------------------------------------

/// Speed in m/s below 10 km/s, otherwise km/s.
pub fn speed(v: f64) -> String {
    // No "-0 m/s" for tiny negative values.
    let v = if v.abs() < 0.5 { 0.0 } else { v };
    if v.abs() < 10_000.0 {
        format!("{v:.0} m/s")
    } else {
        format!("{:.0} km/s", v / 1_000.0)
    }
}

/// Distance in m, km or million km.
pub fn distance(m: f64) -> String {
    let a = m.abs();
    if a < 10_000.0 {
        format!("{m:.0} m")
    } else if a < 1.0e9 {
        format!("{:.1} km", m / 1_000.0)
    } else {
        format!("{:.2} Mkm", m / 1.0e9)
    }
}

/// Duration as "1h 32m", "4m 05s" or "12s".
pub fn duration(seconds: f64) -> String {
    let s = seconds.max(0.0).round() as u64;
    let (h, m, s) = (s / 3_600, (s / 60) % 60, s % 60);
    if h > 0 {
        format!("{h}h {m:02}m")
    } else if m > 0 {
        format!("{m}m {s:02}s")
    } else {
        format!("{s}s")
    }
}

/// Apoapsis/periapsis line for a state relative to a body, or a note when not orbiting.
pub fn orbit_summary(mu: f64, radius: f64, soi: f64, position: DVec3, velocity: DVec3) -> String {
    match KeplerOrbit::from_state(mu, position, velocity, 0.0) {
        Ok(orbit) if orbit.apoapsis() < soi => {
            let pe = orbit.periapsis() - radius;
            let warning = if pe < 0.0 { "  IMPACT" } else { "" };
            format!(
                "Ap {}   Pe {}   T {}{warning}",
                distance(orbit.apoapsis() - radius),
                distance(pe),
                duration(orbit.period())
            )
        }
        _ => {
            let r = position.length();
            if r > 0.0 && position.dot(velocity) > 0.0 {
                "ESCAPE TRAJECTORY".to_string()
            } else {
                "SUBORBITAL / LEAVING".to_string()
            }
        }
    }
}

/// Local solar time ("14:05") at the point under `position` (body inertial frame), if the
/// body has a star to measure it against.
fn local_time(system: &StarSystem, body: usize, position: DVec3, t: f64) -> Option<String> {
    let b = &system.bodies[body];
    b.parent?;
    let surface = system
        .frames
        .transform_point(b.surface_frame, b.inertial_frame, t, position);
    let longitude = (-surface.z).atan2(surface.x);
    let fraction = local_solar_fraction(
        &system.frames,
        b.surface_frame,
        system.star().inertial_frame,
        longitude,
        sim_core::UniverseTime::from_nanos((t * 1e9) as u64),
    );
    let (h, m) = local_clock(fraction);
    Some(format!("{h:02}:{m:02}"))
}

// ---------------------------------------------------------------------------------------------
// Panels
// ---------------------------------------------------------------------------------------------

type FlightPanelQuery = (&'static mut Text, &'static mut Visibility);

#[allow(clippy::too_many_arguments)]
fn update_hud(
    on_foot: Option<Res<OnFoot>>,
    real: Res<Time<Real>>,
    mut refresh: ResMut<HudRefresh>,
    settings: Res<HudSettings>,
    sim: Res<Sim>,
    universe: Res<Universe>,
    pilot: Res<PilotState>,
    ship: Option<Res<PlayerShip>>,
    mut flight: Query<FlightPanelQuery, (With<FlightPanel>, Without<TimePanel>)>,
    mut time_panel: Query<&mut Text, (With<TimePanel>, Without<FlightPanel>)>,
) {
    refresh.0 += real.delta();
    if refresh.0 < REFRESH_INTERVAL || !settings.visible {
        return;
    }
    refresh.0 = Duration::ZERO;

    let system = &universe.0;
    let sim = &sim.0;
    let t = sim.universe_time().as_secs_f64();
    let status = ship
        .as_deref()
        .and_then(|s| player_ship_state(sim, s).map(|st| (s, st)));

    // Clocks.
    let mut clock = format!("GST {}", sim.universe_time().galactic_standard());
    if let Some((_, (region, state, _, _))) = &status {
        let local = state.in_frame(&system.frames, system.bodies[*region].inertial_frame, t);
        if let Some(time) = local_time(system, *region, local.state.position, t) {
            clock += &format!("\n{} local {time}", system.bodies[*region].name);
        }
    }
    clock += &format!("\nWarp x{}", sim.time_warp());
    for mut text in &mut time_panel {
        text.0 = clock.clone();
    }

    // Flight.
    for (mut text, mut visibility) in &mut flight {
        let Some((ship, (region, state, telemetry, assist))) = &status else {
            *visibility = Visibility::Hidden;
            continue;
        };
        if !pilot.flying {
            *visibility = Visibility::Hidden;
            continue;
        }
        *visibility = Visibility::Inherited;
        if let Some((region, state, walker)) =
            on_foot.as_deref().and_then(|f| walker_status(sim, f.id))
        {
            let body = &system.bodies[region];
            let local = state.in_frame(&system.frames, body.inertial_frame, t).state;
            text.0 = format!(
                "{} | ON FOOT\n\
                 SPEED {}   ALT {}{}\n\
                 WASD walk | Shift run | Space jump | F board (vehicle ahead, 12 m) | G spawn (Shift+G: which) | H HUD",
                body.name.to_uppercase(),
                speed(walker.velocity.length()),
                distance(local.position.length() - body.radius),
                if walker.grounded {
                    String::new()
                } else {
                    format!("   AIRBORNE {}", distance(walker.ground_altitude))
                },
            );
            continue;
        }
        let body = &system.bodies[*region];
        let local = state.in_frame(&system.frames, body.inertial_frame, t).state;
        let up = local.position.normalize_or_zero();
        let relative = telemetry.relative_velocity;
        let frame = match telemetry.reference {
            Reference::Surface => "SURFACE",
            Reference::Inertial => "ORBIT",
        };
        let cruise = player_cruise_status(sim, ship, system);
        let arcade = player_arcade_speed(sim, ship)
            .map_or(String::new(), |s| format!("  ARCADE {}", speed(s)));
        let state_flags = format!(
            "{}{}{}{}",
            if telemetry.on_rails { "  ON RAILS" } else { "" },
            if telemetry.in_contact { "  LANDED" } else { "" },
            arcade,
            cruise
        );
        text.0 = format!(
            "{} | {frame}\n\
             SPEED {}   V/S {}   ALT {}{}\n\
             {}\n\
             ASSIST {:?}{state_flags}\n\
             T assist | X brake | Z stop | N arcade | J cruise{} | V view | O orbits | H HUD | F3 debug",
            body.name.to_uppercase(),
            speed(relative.length()),
            speed(relative.dot(up)),
            distance(local.position.length() - body.radius),
            if telemetry.ground_altitude < 20_000.0 {
                format!("   AGL {}", distance(telemetry.ground_altitude))
            } else {
                String::new()
            },
            if telemetry.in_contact {
                "(on the ground)".to_string()
            } else {
                orbit_summary(
                    body.mu,
                    body.radius,
                    body.soi_radius,
                    local.position,
                    local.velocity,
                )
            },
            assist,
            if telemetry.ground_altitude < 30.0 && relative.length() < 2.0 {
                " | F exit"
            } else {
                ""
            },
        );
    }
}

// ---------------------------------------------------------------------------------------------
// Orbit lines
// ---------------------------------------------------------------------------------------------

/// Predicted path (relative to the body) from a state under the body's gravity alone: one full
/// revolution, or until it hits the surface or leaves the sphere of influence.
pub fn predict_path(
    mu: f64,
    radius: f64,
    soi: f64,
    position: DVec3,
    velocity: DVec3,
) -> Vec<DVec3> {
    let accel = |p: DVec3| -p * (mu / p.length().powi(3));
    let mut points = vec![position];
    let (mut p, mut v) = (position, velocity);
    let mut swept = 0.0;
    while points.len() < PATH_MAX_POINTS {
        let r = p.length();
        let timescale = (r / v.length().max(1e-9)).min((r.powi(3) / mu).sqrt());
        let h = PATH_STEP * timescale;
        // Classic RK4.
        let (k1p, k1v) = (v, accel(p));
        let (k2p, k2v) = (v + k1v * (h / 2.0), accel(p + k1p * (h / 2.0)));
        let (k3p, k3v) = (v + k2v * (h / 2.0), accel(p + k2p * (h / 2.0)));
        let (k4p, k4v) = (v + k3v * h, accel(p + k3p * h));
        let next = p + (k1p + 2.0 * k2p + 2.0 * k3p + k4p) * (h / 6.0);
        v += (k1v + 2.0 * k2v + 2.0 * k3v + k4v) * (h / 6.0);
        swept += p.angle_between(next);
        p = next;
        if p.length() < radius {
            points.push(p.normalize() * radius);
            break;
        }
        if swept >= std::f64::consts::TAU {
            points.push(position);
            break;
        }
        points.push(p);
        if p.length() > soi {
            break;
        }
    }
    points
}

#[allow(clippy::too_many_arguments)]
fn draw_orbits(
    sim: Res<Sim>,
    ship: Option<Res<PlayerShip>>,
    on_foot: Option<Res<OnFoot>>,
    settings: Res<HudSettings>,
    render: Res<RenderTime>,
    universe: Res<Universe>,
    camera: Res<SpaceCamera>,
    tracked: Res<ShipRenderState>,
    mut gizmos: Gizmos,
) {
    if settings.orbits == OrbitLines::None || !settings.visible {
        return;
    }
    let system = &universe.0;
    let t = render.0;
    let poses = body_poses(system, &camera, t);

    // The orbits of the body the camera is near and of its parents pass right through the
    // view and only add clutter there; skip them.
    let mut near = vec![false; system.bodies.len()];
    let mut b = Some(camera.anchor);
    while let Some(i) = b {
        near[i] = true;
        b = system.bodies[i].parent;
    }
    for (i, body) in system.bodies.iter().enumerate() {
        let (Some(parent), Some(orbit)) = (body.parent, body.orbit.as_ref()) else {
            continue;
        };
        if near[i] || settings.orbits != OrbitLines::All {
            continue;
        }
        let center = poses[parent].0;
        let period = orbit.period();
        // Start at the body's current position so the line passes exactly through it.
        let points = (0..=BODY_ORBIT_POINTS).map(|k| {
            let tk = t + period * k as f64 / BODY_ORBIT_POINTS as f64;
            (center + orbit.state_at(tk).0).as_vec3()
        });
        gizmos.linestrip(points, BODY_ORBIT_COLOR);
    }

    // No orbit line while landed or walking: it would just lie along the ground.
    let grounded = on_foot.is_some()
        || ship
            .as_deref()
            .and_then(|s| player_ship_state(&sim.0, s))
            .is_some_and(|(_, _, telemetry, _)| telemetry.in_contact);
    if !grounded
        && let Some(object) = tracked.state
        && let Some(region) = system
            .bodies
            .iter()
            .position(|b| b.inertial_frame == object.frame)
    {
        let body = &system.bodies[region];
        let local = object.state;
        let center = poses[region].0;
        let soi = if body.soi_radius.is_finite() {
            body.soi_radius
        } else {
            f64::MAX
        };
        let path = predict_path(body.mu, body.radius, soi, local.position, local.velocity);
        gizmos.linestrip(
            path.iter().map(|p| (center + *p).as_vec3()),
            SHIP_PATH_COLOR,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MU: f64 = 9.81 * 600_000.0 * 600_000.0;
    const R: f64 = 600_000.0;

    #[test]
    fn circular_path_closes_on_itself_at_constant_radius() {
        let r = R + 150_000.0;
        let p = DVec3::X * r;
        let v = DVec3::NEG_Z * (MU / r).sqrt();
        let path = predict_path(MU, R, 1e9, p, v);
        assert!(path.len() > 200 && path.len() < PATH_MAX_POINTS);
        for q in &path {
            assert!(
                (q.length() - r).abs() < 1.0,
                "radius drift {}",
                q.length() - r
            );
        }
        assert_eq!(*path.last().unwrap(), p, "closed");
    }

    #[test]
    fn falling_path_ends_on_the_surface() {
        let p = DVec3::X * (R + 50_000.0);
        let path = predict_path(MU, R, 1e9, p, DVec3::ZERO);
        let end = path.last().unwrap();
        assert!((end.length() - R).abs() < 1.0);
    }

    #[test]
    fn escaping_path_stops_at_the_sphere_of_influence() {
        let r = R + 150_000.0;
        let p = DVec3::X * r;
        let v = DVec3::NEG_Z * (2.0 * MU / r).sqrt() * 1.2;
        let soi = 20.0 * R;
        let path = predict_path(MU, R, soi, p, v);
        assert!(path.last().unwrap().length() > soi);
        assert!(path.len() < PATH_MAX_POINTS);
    }

    #[test]
    fn orbit_summary_reports_apsides_and_escape() {
        let r = R + 150_000.0;
        let p = DVec3::X * r;
        let circular = orbit_summary(MU, R, 1e9, p, DVec3::NEG_Z * (MU / r).sqrt());
        assert!(
            circular.starts_with("Ap 150.0 km   Pe 150.0 km"),
            "{circular}"
        );
        let escape = orbit_summary(MU, R, 1e9, p, DVec3::new(1.0, 0.0, -1.0) * 1e5);
        assert_eq!(escape, "ESCAPE TRAJECTORY");
        let falling = orbit_summary(MU, R, 1e9, p, DVec3::NEG_Z * 1_000.0);
        assert!(falling.ends_with("IMPACT"), "{falling}");
    }

    #[test]
    fn formats_read_well() {
        assert_eq!(speed(2_170.4), "2170 m/s");
        assert_eq!(speed(-0.2), "0 m/s");
        assert_eq!(speed(150_000_000.0), "150000 km/s");
        assert_eq!(distance(150_000.0), "150.0 km");
        assert_eq!(distance(8.0e9), "8.00 Mkm");
        assert_eq!(duration(5_520.0), "1h 32m");
        assert_eq!(duration(245.0), "4m 05s");
    }
}
