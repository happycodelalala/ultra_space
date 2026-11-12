//! The player's ship: spawning, controls, camera and visuals (Batch 1, task 1.11).
//!
//! Controls (design: objects-vehicles-physics.md §10): W/S forward/back, A/D strafe,
//! Space/Ctrl up/down, mouse pitch/yaw (cursor locked), Q/E roll, T cycles flight assist,
//! X brakes (assist), Z stops instantly (debug), N toggles arcade flight (debug), J engages or
//! drops out of the cruise drive, Tab switches between flying and the free debug camera, V
//! toggles the cockpit view, right mouse orbits the chase camera. Esc releases the mouse (click
//! to capture it again); the mouse is also released when the window loses focus. 1–8 teleport
//! the ship into a parking orbit around a body (debug).

use bevy::camera::Exposure;
use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow, Window};
use glam::{DQuat, DVec3};
use sim_core::{ObjectId, ObjectState, Simulation, StarSystem};
use sim_vehicle::{
    ArcadeFlight, AssistMode, Cruise, CruiseBlocked, CruisePhase, FlightControl, FlightTelemetry,
    OnRails, VehicleBody,
};

use crate::camera::{BODY_KEYS, CameraSet, CameraStage, SpaceCamera, look_rotation};
use crate::script::ScriptRunning;
use crate::sim::{RenderTime, Sim, SimSet};
use crate::space_render::{MainCamera, STAR_LIGHT};
use crate::universe::{GameState, InGame, StartingVehicle, Universe};

const START_BODY: &str = "meridian";

/// Mouse movement (pixels per frame) giving full rotation stick.
const MOUSE_FULL_STICK: f32 = 25.0;
/// Chase camera offset behind and above the ship (vehicle axes).
const CHASE_OFFSET: DVec3 = DVec3::new(0.0, 5.0, 24.0);
const COCKPIT_EYE: DVec3 = DVec3::new(0.0, 0.5, -5.2);

pub struct ShipPlugin;

impl Plugin for ShipPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PilotState>()
            .init_resource::<ShipRenderState>()
            .add_systems(
                OnEnter(GameState::Running),
                (spawn_player_ship, spawn_starlight),
            )
            .add_systems(
                Update,
                (
                    (pilot_mode_keys, release_mouse_on_focus_loss).chain(),
                    (
                        teleport_keys,
                        cruise_keys,
                        full_stop_key,
                        arcade_key,
                        pilot_input,
                    )
                        .chain()
                        .after(pilot_mode_keys)
                        .before(SimSet),
                    track_ship.after(SimSet).before(CameraSet),
                    follow_ship.in_set(CameraStage::Follow),
                    aim_starlight.after(CameraSet),
                    cursor_lock,
                )
                    .in_set(InGame),
            );
    }
}

/// The player ship's state for rendering: interpolated between its last two ticks at the
/// render time, in its region body's inertial frame.
#[derive(Resource, Default)]
pub struct ShipRenderState {
    previous: Option<(f64, ObjectState)>,
    current: Option<(f64, ObjectState)>,
    pub state: Option<ObjectState>,
}

/// Interpolates between two ticks' states at universe time `t` with a cubic Hermite curve
/// through both positions and velocities (exact enough for orbits even under time warp).
/// Snaps to `current` after a teleport (a jump much larger than the velocities explain).
pub fn interpolate_state(
    system: &StarSystem,
    previous: (f64, ObjectState),
    current: (f64, ObjectState),
    t: f64,
) -> ObjectState {
    let (t0, t1) = (previous.0, current.0);
    let dt = t1 - t0;
    if dt <= 0.0 {
        return current.1;
    }
    let a = previous
        .1
        .in_frame(&system.frames, current.1.frame, t0)
        .state;
    let b = current.1.state;
    let travel = a.velocity.length().max(b.velocity.length()) * dt;
    if (b.position - a.position).length() > 2.0 * travel + 1_000.0 {
        return current.1;
    }
    let f = ((t - t0) / dt).clamp(0.0, 1.0);
    let (f2, f3) = (f * f, f * f * f);
    let position = a.position * (2.0 * f3 - 3.0 * f2 + 1.0)
        + a.velocity * (dt * (f3 - 2.0 * f2 + f))
        + b.position * (-2.0 * f3 + 3.0 * f2)
        + b.velocity * (dt * (f3 - f2));
    let mut out = current.1;
    out.state.position = position;
    out.state.velocity = a.velocity.lerp(b.velocity, f);
    out.state.rotation = a.rotation.slerp(b.rotation, f).normalize();
    out
}

/// Records the player ship's state after each simulation advance and interpolates it at the
/// render time.
fn track_ship(
    sim: Res<Sim>,
    universe: Res<Universe>,
    render: Res<RenderTime>,
    ship: Option<Res<PlayerShip>>,
    mut tracked: ResMut<ShipRenderState>,
) {
    let Some((region, state, _, _)) = ship.and_then(|s| player_ship_state(&sim.0, &s)) else {
        tracked.state = None;
        return;
    };
    let system = &universe.0;
    let t = sim.0.universe_time().as_secs_f64();
    let state = state.in_frame(&system.frames, system.bodies[region].inertial_frame, t);
    if tracked.current.is_none_or(|(tc, _)| tc != t) {
        tracked.previous = tracked.current;
        tracked.current = Some((t, state));
    }
    tracked.state = Some(match (tracked.previous, tracked.current) {
        (Some(p), Some(c)) => interpolate_state(system, p, c, render.0),
        _ => state,
    });
}

/// The ship the player flies.
#[derive(Resource)]
pub struct PlayerShip {
    pub id: ObjectId,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ShipView {
    #[default]
    Chase,
    Cockpit,
}

/// Whether the player is flying the ship (vs the free debug camera), and the view.
#[derive(Resource, Debug)]
pub struct PilotState {
    pub flying: bool,
    pub view: ShipView,
    /// Chase-camera orbit around the ship (yaw, pitch), radians.
    pub orbit: (f64, f64),
    /// The mouse is released (Esc or focus loss): the cursor is free and doesn't steer.
    pub mouse_free: bool,
    /// Walking on foot (see `walk`): ship controls and the ship camera stand down.
    pub on_foot: bool,
}

/// Initial chase-camera orbit from `ULTRA_SPACE_CHASE_ORBIT=<yaw>,<pitch>` (degrees; negative
/// pitch looks down), for screenshots. Defaults to straight behind.
fn start_orbit() -> (f64, f64) {
    std::env::var("ULTRA_SPACE_CHASE_ORBIT")
        .ok()
        .and_then(|v| {
            let (yaw, pitch) = v.split_once(',')?;
            Some((
                yaw.trim().parse::<f64>().ok()?.to_radians(),
                pitch
                    .trim()
                    .parse::<f64>()
                    .ok()?
                    .to_radians()
                    .clamp(-1.4, 1.4),
            ))
        })
        .unwrap_or((0.0, 0.0))
}

impl Default for PilotState {
    fn default() -> Self {
        Self {
            flying: true,
            view: ShipView::Chase,
            orbit: start_orbit(),
            mouse_free: false,
            on_foot: false,
        }
    }
}

fn spawn_player_ship(
    mut commands: Commands,
    universe: Res<Universe>,
    start: Res<StartingVehicle>,
    mut sim: ResMut<Sim>,
) {
    let vehicle = start.0.clone();
    let body = VehicleBody::of(&vehicle).expect("design has parts");
    info!(
        "spawning {}: {:.1} t, forward {:.1} m/s², {} thrusters",
        vehicle.name,
        body.mass.mass / 1_000.0,
        body.performance.forward_acceleration(),
        body.allocator.thrusters().len()
    );

    let system = &universe.0;
    let index = system
        .bodies
        .iter()
        .position(|b| b.id == START_BODY)
        .unwrap_or(0);
    let mut state = parking_orbit(system, index, 0.0);
    // Debug: `ULTRA_SPACE_START_SUN_ANGLE=<deg>` moves the start along the orbit to that angle
    // from the point under the star (0 noon, 90 the terminator, 180 midnight). The parking
    // orbit starts 30° from it; rotating about the orbit normal keeps the orbit circular.
    if let Some(angle) = std::env::var("ULTRA_SPACE_START_SUN_ANGLE")
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
    {
        let turn = DQuat::from_rotation_y(-(angle - 30.0).to_radians());
        let s = &mut state.state;
        s.position = turn * s.position;
        s.velocity = turn * s.velocity;
        s.rotation = (turn * s.rotation).normalize();
    }
    // Debug: `ULTRA_SPACE_START_ALTITUDE=<m>` starts in a circular orbit at that altitude.
    if let Some(altitude) = std::env::var("ULTRA_SPACE_START_ALTITUDE")
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
    {
        let body = &system.bodies[index];
        let r = body.radius + altitude.max(0.0);
        let s = &mut state.state;
        s.position = s.position.normalize() * r;
        s.velocity = s.velocity.normalize() * (body.mu / r).sqrt();
    }

    let sim = &mut sim.0;
    let id = sim
        .objects
        .spawn(
            &mut sim.regions,
            index,
            state,
            (
                vehicle,
                body,
                FlightControl::default(),
                FlightTelemetry::default(),
                Cruise::default(),
            ),
        )
        .expect("start region exists");
    commands.insert_resource(PlayerShip { id });
}

/// Parking-orbit altitude above a body: a quarter of its radius (150 km for Meridian), at least
/// 20 km above the atmosphere, and well inside its sphere of influence.
pub fn parking_radius(system: &StarSystem, index: usize) -> f64 {
    let body = &system.bodies[index];
    let atmosphere = body.atmosphere.as_ref().map_or(0.0, |a| a.height);
    let altitude = (0.25 * body.radius).max(atmosphere + 20_000.0);
    (body.radius + altitude).min(0.5 * body.soi_radius)
}

/// A circular, prograde, equatorial parking orbit over the day side at time `t`, 30° short of
/// the subsolar point so the terminator is in view; nose along the velocity, belly toward the
/// body. The state is in the body's inertial frame.
pub fn parking_orbit(system: &StarSystem, index: usize, t: f64) -> ObjectState {
    let body = &system.bodies[index];
    let r = parking_radius(system, index);
    let speed = (body.mu / r).sqrt();
    let to_star = system.frames.transform_point(
        body.inertial_frame,
        system.star().inertial_frame,
        t,
        DVec3::ZERO,
    );
    let sunward = DVec3::new(to_star.x, 0.0, to_star.z).normalize_or(DVec3::X);
    let up = DQuat::from_rotation_y(-30f64.to_radians()) * sunward;
    // Orbits with angular momentum along +Y move along Y × r.
    let prograde = DVec3::Y.cross(up);
    let mut state = ObjectState::at_rest(body.inertial_frame, up * r, look_rotation(prograde, up));
    state.state.velocity = prograde * speed;
    state
}

// ---------------------------------------------------------------------------------------------
// Input
// ---------------------------------------------------------------------------------------------

fn pilot_mode_keys(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    mut pilot: ResMut<PilotState>,
    mut camera: ResMut<SpaceCamera>,
) {
    if keys.just_pressed(KeyCode::Tab) {
        pilot.flying = !pilot.flying;
        if !pilot.flying {
            // The free camera starts where the ship camera was.
            camera.speed = 200.0;
        }
        info!(
            "{}",
            if pilot.flying {
                "flying the ship"
            } else {
                "free camera"
            }
        );
    }
    if keys.just_pressed(KeyCode::Escape) {
        pilot.mouse_free = !pilot.mouse_free;
    } else if pilot.mouse_free && pilot.flying && buttons.just_pressed(MouseButton::Left) {
        pilot.mouse_free = false;
    }
    if keys.just_pressed(KeyCode::KeyV) {
        pilot.view = match pilot.view {
            ShipView::Chase => ShipView::Cockpit,
            ShipView::Cockpit => ShipView::Chase,
        };
    }
}

/// Releases the mouse when the window loses focus (Alt+Tab, clicking another window).
fn release_mouse_on_focus_loss(
    windows: Query<&Window, With<PrimaryWindow>>,
    script: Option<Res<ScriptRunning>>,
    mut pilot: ResMut<PilotState>,
) {
    // Scripted input keeps steering even when the window isn't focused.
    if script.is_some() {
        return;
    }
    if !pilot.mouse_free && windows.iter().any(|w| !w.focused) {
        pilot.mouse_free = true;
    }
}

/// While flying, 1–8 move the ship into a parking orbit around that body (debug teleport).
fn teleport_keys(
    keys: Res<ButtonInput<KeyCode>>,
    universe: Res<Universe>,
    ship: Option<Res<PlayerShip>>,
    pilot: Res<PilotState>,
    mut sim: ResMut<Sim>,
) {
    let Some(ship) = ship else { return };
    if !pilot.flying || pilot.on_foot {
        return; // the free camera uses 1–8 to jump the camera instead
    }
    let system = &universe.0;
    let Some(index) = BODY_KEYS
        .iter()
        .position(|k| keys.just_pressed(*k))
        .filter(|&i| i < system.bodies.len())
    else {
        return;
    };
    let sim = &mut sim.0;
    let t = sim.universe_time().as_secs_f64();
    let state = parking_orbit(system, index, t);
    if let Err(e) = sim
        .objects
        .relocate(&mut sim.regions, ship.id, index, state)
    {
        warn!("teleport failed: {e}");
        return;
    }
    if let Some(loc) = sim.objects.location(ship.id)
        && let Some(region) = sim.regions.get_mut(loc.region)
        && let Some(mut control) = region.world.get_mut::<FlightControl>(loc.entity)
    {
        // Drop any held velocity target from the old body.
        control.desired_velocity = None;
    }
    if let Some(loc) = sim.objects.location(ship.id)
        && let Some(region) = sim.regions.get_mut(loc.region)
        && let Some(mut cruise) = region.world.get_mut::<Cruise>(loc.entity)
    {
        *cruise = Cruise::default();
    }
    info!(
        "teleported to a {:.0} km parking orbit around {}",
        (parking_radius(system, index) - system.bodies[index].radius) / 1_000.0,
        system.bodies[index].name
    );
}

fn axis(keys: &ButtonInput<KeyCode>, positive: KeyCode, negative: KeyCode) -> f64 {
    f64::from(keys.pressed(positive)) - f64::from(keys.pressed(negative))
}

/// How long a refused cruise engage stays on screen (s).
const CRUISE_NOTICE_TIME: f32 = 3.0;

/// Z (debug): full stop. Instantly sets the ship's velocity to "standing still" for flight
/// assist: zero relative to the ground below the surface reference altitude (hover), zero in
/// the body's inertial frame above it (drop straight down out of orbit). Also ends cruise and
/// on-rails coasting, and makes assist hold zero speed.
fn full_stop_key(
    keys: Res<ButtonInput<KeyCode>>,
    universe: Res<Universe>,
    ship: Option<Res<PlayerShip>>,
    pilot: Res<PilotState>,
    mut sim: ResMut<Sim>,
) {
    if !pilot.flying || pilot.on_foot || !keys.just_pressed(KeyCode::KeyZ) {
        return;
    }
    let Some(ship) = ship else { return };
    let system = &universe.0;
    let sim = &mut sim.0;
    let t = sim.universe_time().as_secs_f64();
    let Some(loc) = sim.objects.location(ship.id) else {
        return;
    };
    let Some(region) = sim.regions.get_mut(loc.region) else {
        return;
    };
    let Ok(mut e) = region.world.get_entity_mut(loc.entity) else {
        return;
    };
    let frame = system.bodies[loc.region].inertial_frame;
    let Some(state) = e
        .get::<ObjectState>()
        .map(|s| s.in_frame(&system.frames, frame, t))
    else {
        return;
    };
    let env = sim_vehicle::flight::body_env(system, loc.region, t);
    let (reference, velocity) = env.standing_still(state.state.position);
    let mut stopped = state;
    stopped.state.velocity = velocity;
    stopped.state.angular_velocity = DVec3::ZERO;
    e.insert(stopped);
    e.remove::<OnRails>();
    if let Some(mut control) = e.get_mut::<FlightControl>() {
        control.desired_velocity = Some((reference, DVec3::ZERO));
        control.desired_body = Some(loc.region);
    }
    if let Some(mut cruise) = e.get_mut::<Cruise>() {
        *cruise = Cruise::default();
    }
    info!("full stop ({reference:?} reference)");
}

/// N (debug): toggles arcade flight (No Man's Sky style: no gravity, the ship goes where the
/// nose points; W/S change the held speed). Turning it on keeps the current forward speed;
/// turning it off hands the current velocity back to Newtonian flight.
fn arcade_key(
    keys: Res<ButtonInput<KeyCode>>,
    ship: Option<Res<PlayerShip>>,
    pilot: Res<PilotState>,
    mut sim: ResMut<Sim>,
) {
    if !pilot.flying || pilot.on_foot || !keys.just_pressed(KeyCode::KeyN) {
        return;
    }
    let Some(ship) = ship else { return };
    let sim = &mut sim.0;
    let Some(loc) = sim.objects.location(ship.id) else {
        return;
    };
    let Some(region) = sim.regions.get_mut(loc.region) else {
        return;
    };
    let Ok(mut e) = region.world.get_entity_mut(loc.entity) else {
        return;
    };
    if e.contains::<ArcadeFlight>() {
        e.remove::<ArcadeFlight>();
        if let Some(mut control) = e.get_mut::<FlightControl>() {
            // Assist starts from the current velocity.
            control.desired_velocity = None;
        }
        info!("arcade flight off: Newtonian flight");
    } else {
        let forward_speed = match (e.get::<ObjectState>(), e.get::<FlightTelemetry>()) {
            (Some(s), Some(tel)) => tel
                .relative_velocity
                .dot(s.state.rotation * DVec3::NEG_Z)
                .max(0.0),
            _ => 0.0,
        };
        e.insert(ArcadeFlight {
            speed: forward_speed.min(sim_vehicle::arcade::MAX_SPEED),
        });
        info!("arcade flight on: no gravity, flies where the nose points");
    }
}

/// J engages or drops out of the cruise drive; a refusal notice clears after a few seconds.
fn cruise_keys(
    time: Res<Time<Real>>,
    mut notice: Local<f32>,
    keys: Res<ButtonInput<KeyCode>>,
    ship: Option<Res<PlayerShip>>,
    pilot: Res<PilotState>,
    mut sim: ResMut<Sim>,
) {
    let Some(ship) = ship else { return };
    let sim = &mut sim.0;
    let Some(loc) = sim.objects.location(ship.id) else {
        return;
    };
    let Some(mut cruise) = sim
        .regions
        .get_mut(loc.region)
        .and_then(|r| r.world.get_mut::<Cruise>(loc.entity))
    else {
        return;
    };
    if pilot.flying && !pilot.on_foot && keys.just_pressed(KeyCode::KeyJ) {
        cruise.toggle = true;
        *notice = 0.0;
    } else if cruise.blocked.is_some() {
        *notice += time.delta_secs();
        if *notice > CRUISE_NOTICE_TIME {
            cruise.blocked = None;
        }
    }
}

fn pilot_input(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    ship: Option<Res<PlayerShip>>,
    mut pilot: ResMut<PilotState>,
    mut sim: ResMut<Sim>,
) {
    let Some(ship) = ship else { return };
    let sim = &mut sim.0;
    let Some(loc) = sim.objects.location(ship.id) else {
        return;
    };
    let Some(region) = sim.regions.get_mut(loc.region) else {
        return;
    };
    let Some(mut control) = region.world.get_mut::<FlightControl>(loc.entity) else {
        return;
    };

    if !pilot.flying || pilot.on_foot {
        // Let go of the controls; assist keeps the ship steady.
        control.translate = DVec3::ZERO;
        control.rotate = DVec3::ZERO;
        control.brake = false;
        return;
    }
    control.translate = DVec3::new(
        axis(&keys, KeyCode::KeyD, KeyCode::KeyA),
        axis(&keys, KeyCode::Space, KeyCode::ControlLeft),
        axis(&keys, KeyCode::KeyS, KeyCode::KeyW),
    );
    let orbiting = buttons.pressed(MouseButton::Right) && !pilot.mouse_free;
    let mouse = if orbiting || pilot.mouse_free {
        Vec2::ZERO
    } else {
        motion.delta / MOUSE_FULL_STICK
    };
    control.rotate = DVec3::new(
        -f64::from(mouse.y.clamp(-1.0, 1.0)),
        -f64::from(mouse.x.clamp(-1.0, 1.0)),
        axis(&keys, KeyCode::KeyQ, KeyCode::KeyE),
    );
    control.brake = keys.pressed(KeyCode::KeyX);
    if keys.just_pressed(KeyCode::KeyT) {
        control.assist = control.assist.next();
        info!("flight assist: {:?}", control.assist);
    }
    if orbiting {
        let d = motion.delta.as_dvec2() * 0.004;
        pilot.orbit.0 -= d.x;
        pilot.orbit.1 = (pilot.orbit.1 - d.y).clamp(-1.4, 1.4);
    }
}

fn cursor_lock(
    pilot: Res<PilotState>,
    script: Option<Res<ScriptRunning>>,
    mut windows: Query<&mut CursorOptions, With<PrimaryWindow>>,
) {
    // A scripted run is watched, not played: leave the desktop cursor alone.
    if !pilot.is_changed() || script.is_some() {
        return;
    }
    for mut cursor in &mut windows {
        if pilot.flying && !pilot.mouse_free {
            cursor.grab_mode = CursorGrabMode::Locked;
            cursor.visible = false;
        } else {
            cursor.grab_mode = CursorGrabMode::None;
            cursor.visible = true;
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Camera
// ---------------------------------------------------------------------------------------------

/// Current state and flight data of the player ship, if it exists.
/// Whether the player ship is in arcade flight, and its held speed.
pub fn player_arcade_speed(sim: &Simulation, ship: &PlayerShip) -> Option<f64> {
    let loc = sim.objects.location(ship.id)?;
    let world = &sim.regions.get(loc.region)?.world;
    world.get::<ArcadeFlight>(loc.entity).map(|a| a.speed)
}

/// The player ship's cruise drive status as overlay text ("" when off).
pub fn player_cruise_status(sim: &Simulation, ship: &PlayerShip, system: &StarSystem) -> String {
    let Some(cruise) = sim
        .objects
        .location(ship.id)
        .and_then(|loc| sim.regions.get(loc.region)?.world.get::<Cruise>(loc.entity))
    else {
        return String::new();
    };
    match (cruise.phase, &cruise.blocked) {
        (CruisePhase::Charging { remaining }, _) => format!("  CRUISE CHARGING {remaining:.1} s"),
        (CruisePhase::Cruising { speed, .. }, _) => {
            format!("  CRUISE {:.0} km/s (J drop)", speed / 1_000.0)
        }
        (CruisePhase::Off, Some(CruiseBlocked::NoDrive)) => "  NO CRUISE DRIVE".into(),
        (CruisePhase::Off, Some(CruiseBlocked::TooClose(i))) => {
            format!("  CRUISE BLOCKED: too close to {}", system.bodies[*i].name)
        }
        (CruisePhase::Off, None) => String::new(),
    }
}

pub fn player_ship_state(
    sim: &Simulation,
    ship: &PlayerShip,
) -> Option<(usize, ObjectState, FlightTelemetry, AssistMode)> {
    let loc = sim.objects.location(ship.id)?;
    let world = &sim.regions.get(loc.region)?.world;
    let state = *world.get::<ObjectState>(loc.entity)?;
    let telemetry = world.get::<FlightTelemetry>(loc.entity)?.clone();
    let assist = world.get::<FlightControl>(loc.entity)?.assist;
    Some((loc.region, state, telemetry, assist))
}

fn follow_ship(
    pilot: Res<PilotState>,
    tracked: Res<ShipRenderState>,
    universe: Res<Universe>,
    sim: Res<Sim>,
    render: Res<RenderTime>,
    mut camera: ResMut<SpaceCamera>,
) {
    if !pilot.flying || pilot.on_foot {
        return;
    }
    let Some(object) = tracked.state else { return };
    // The render state is in the region body's inertial frame.
    let Some(region) = universe
        .0
        .bodies
        .iter()
        .position(|b| b.inertial_frame == object.frame)
    else {
        return;
    };
    let state = object.state;
    let (offset, rotation) = match pilot.view {
        ShipView::Chase => {
            let orbit =
                DQuat::from_rotation_y(pilot.orbit.0) * DQuat::from_rotation_x(pilot.orbit.1);
            let look = DQuat::from_rotation_x(-0.12);
            (
                state.rotation * orbit * CHASE_OFFSET,
                state.rotation * orbit * look,
            )
        }
        ShipView::Cockpit => (state.rotation * COCKPIT_EYE, state.rotation),
    };
    camera.anchor = region;
    camera.offset = keep_above_ground(
        &sim.0,
        &universe.0,
        region,
        render.0,
        state.position + offset,
    );
    camera.rotation = rotation;
}

/// Lowest a camera may go above the ground (m).
const CAMERA_CLEARANCE: f64 = 2.0;

/// `point` (region body's inertial frame) raised to at least `CAMERA_CLEARANCE` above the
/// terrain, so an orbiting chase camera never ends up inside the ground.
pub fn keep_above_ground(
    sim: &Simulation,
    system: &StarSystem,
    region: usize,
    t: f64,
    point: DVec3,
) -> DVec3 {
    let Some(terrain) = sim
        .regions
        .get(region)
        .and_then(|r| r.world.get_resource::<sim_vehicle::flight::RegionTerrain>())
        .and_then(|r| r.0.clone())
    else {
        return point;
    };
    let body = &system.bodies[region];
    let r = point.length();
    if r > body.radius + 1.3 * terrain.params.relief + CAMERA_CLEARANCE {
        return point;
    }
    let surface = system
        .frames
        .state_in(body.inertial_frame, body.surface_frame, t)
        .rotation;
    let dir = surface.inverse() * point.normalize();
    let mut ground = terrain.height(dir, 2.0);
    if terrain.has_ocean() {
        ground = ground.max(0.0);
    }
    let floor = body.radius + ground + CAMERA_CLEARANCE;
    if r < floor {
        point * (floor / r)
    } else {
        point
    }
}

// ---------------------------------------------------------------------------------------------
// Visuals
// ---------------------------------------------------------------------------------------------

/// Light from the star for ordinary (PBR) meshes such as ships, matching the planets' brightness.
#[derive(Component)]
struct Starlight;

fn spawn_starlight(mut commands: Commands) {
    let illuminance = STAR_LIGHT * std::f32::consts::PI / Exposure::default().exposure();
    commands.spawn((
        Starlight,
        DirectionalLight {
            illuminance,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::default(),
    ));
}

fn aim_starlight(
    universe: Res<Universe>,
    render: Res<RenderTime>,
    camera: Res<SpaceCamera>,
    mut lights: Query<&mut Transform, (With<Starlight>, Without<MainCamera>)>,
) {
    let system = &universe.0;
    let t = render.0;
    let star = system.frames.transform_point(
        system.bodies[camera.anchor].inertial_frame,
        system.star().inertial_frame,
        t,
        DVec3::ZERO,
    ) - camera.offset;
    // The light shines from the star toward the camera's neighbourhood.
    let direction = (-star).normalize_or_zero().as_vec3();
    if direction != Vec3::ZERO {
        for mut transform in &mut lights {
            *transform = Transform::default().looking_to(direction, Vec3::Y);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use sim_core::KeplerOrbit;

    use super::*;

    fn halcyon() -> StarSystem {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../content/data/star_systems/halcyon.json");
        StarSystem::from_path(path).unwrap()
    }

    #[test]
    fn parking_orbits_are_circular_above_the_air_and_inside_the_sphere_of_influence() {
        let sys = halcyon();
        for (i, body) in sys.bodies.iter().enumerate() {
            let state = parking_orbit(&sys, i, 12_345.0);
            assert_eq!(state.frame, body.inertial_frame);
            let r = state.state.position.length();
            let air = body.atmosphere.as_ref().map_or(0.0, |a| a.height);
            assert!(r > body.radius + air, "{}: below the atmosphere", body.id);
            assert!(r < body.soi_radius, "{}: outside its sphere", body.id);
            let v = state.state.velocity;
            assert!(
                (v.length() - (body.mu / r).sqrt()).abs() < 1e-6,
                "{}",
                body.id
            );
            assert!(v.dot(state.state.position).abs() < 1e-3 * r * v.length());
        }
        let m = sys.bodies.iter().position(|b| b.id == "meridian").unwrap();
        let altitude = parking_radius(&sys, m) - sys.bodies[m].radius;
        assert!(
            (altitude - 150_000.0).abs() < 1.0,
            "Meridian start altitude"
        );
    }

    #[test]
    fn interpolation_follows_the_orbit_between_warped_ticks() {
        let sys = halcyon();
        let m = sys.bodies.iter().position(|b| b.id == "meridian").unwrap();
        let start = parking_orbit(&sys, m, 0.0).state;
        let orbit =
            KeplerOrbit::from_state(sys.bodies[m].mu, start.position, start.velocity, 0.0).unwrap();
        let at = |t: f64| {
            let (p, v) = orbit.state_at(t);
            let mut s = ObjectState::at_rest(sys.bodies[m].inertial_frame, p, DQuat::IDENTITY);
            s.state.velocity = v;
            (t, s)
        };
        // Ticks 33 s apart (x1,000 warp).
        let (a, b) = (at(100.0), at(133.3));
        for k in 0..=10 {
            let t = 100.0 + 33.3 * f64::from(k) / 10.0;
            let s = interpolate_state(&sys, a, b, t);
            let error = (s.state.position - orbit.state_at(t).0).length();
            assert!(error < 5.0, "t {t}: off by {error:.1} m");
        }
    }

    #[test]
    fn interpolation_snaps_after_a_teleport() {
        let sys = halcyon();
        let m = sys.bodies.iter().position(|b| b.id == "meridian").unwrap();
        let a = (0.0, parking_orbit(&sys, m, 0.0));
        let mut far = parking_orbit(&sys, m, 0.0);
        far.state.position *= 10.0;
        let b = (1.0 / 30.0, far);
        let s = interpolate_state(&sys, a, b, 0.5 / 30.0);
        assert_eq!(s.state.position, far.state.position);
    }
}
