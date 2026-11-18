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
use sim_vehicle::gait::{LegsOff, Walking};
use sim_vehicle::{
    ArcadeFlight, AssistMode, Cruise, CruiseBlocked, CruisePhase, FlightControl, FlightTelemetry,
    OnRails, Vehicle, VehicleBody,
};

use crate::camera::{BODY_KEYS, CameraSet, CameraStage, SpaceCamera, look_rotation};
use crate::script::ScriptRunning;
use crate::sim::{RenderTime, Sim, SimSet};
use crate::space_render::{MainCamera, STAR_LIGHT};
use crate::universe::{GameState, InGame, StartingVehicle, Universe};

/// The body the game starts on: Meridian in the Halcyon system, otherwise the first body with
/// terrain (Tessera).
pub(crate) fn start_body(system: &StarSystem) -> Option<usize> {
    system
        .bodies
        .iter()
        .position(|b| b.id == "meridian")
        .or_else(|| system.bodies.iter().position(|b| b.terrain.is_some()))
}

/// How the player's ship flies: No Man's Sky style (arcade: flies where the nose points, no
/// gravity, lands with L) by default; Newtonian flight with orbits (`ULTRA_SPACE_FLIGHT=newtonian`,
/// or N in play: debug).
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub struct FlightModel {
    pub arcade: bool,
}

impl Default for FlightModel {
    fn default() -> Self {
        Self {
            arcade: std::env::var("ULTRA_SPACE_FLIGHT").map_or(true, |v| v != "newtonian"),
        }
    }
}

/// Landing (L) is allowed below this height above the ground (m) and speed (m/s).
const LAND_HEIGHT: f64 = 200.0;
const LAND_SPEED_LIMIT: f64 = 60.0;

/// Mouse movement (pixels per frame) giving full rotation stick.
const MOUSE_FULL_STICK: f32 = 25.0;
/// Chase camera offset behind and above the ship (vehicle axes), for a vehicle up to
/// CHASE_SIZE across; larger ones push it out in proportion.
const CHASE_OFFSET: DVec3 = DVec3::new(0.0, 5.0, 24.0);
/// The vehicle reach (m, from its origin) CHASE_OFFSET suits.
const CHASE_SIZE: f64 = 20.0;
const COCKPIT_EYE: DVec3 = DVec3::new(0.0, 0.5, -5.2);

pub struct ShipPlugin;

impl Plugin for ShipPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PilotState>()
            .init_resource::<FlightModel>()
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
                        pose_key,
                        walk_key,
                        pilot_input,
                        arcade_rules,
                    )
                        .chain()
                        .after(pilot_mode_keys)
                        .before(SimSet),
                    track_ship.after(SimSet).before(CameraSet),
                    follow_ship.in_set(CameraStage::Follow),
                    aim_starlight.after(CameraSet),
                    aim_sky.after(CameraSet),
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
    /// In build mode (see `builder`): flying and walking controls and their cameras stand down.
    pub building: bool,
}

impl PilotState {
    /// Whether the ship's controls and chase camera are in use.
    pub fn controls_ship(&self) -> bool {
        self.flying && !self.on_foot && !self.building
    }
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
            building: false,
            on_foot: false,
        }
    }
}

fn spawn_player_ship(
    mut commands: Commands,
    universe: Res<Universe>,
    start: Res<StartingVehicle>,
    mut sim: ResMut<Sim>,
    blocks: Option<Res<crate::blocks::BlockWorld>>,
    mut pilot: ResMut<PilotState>,
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
    let index = start_body(system).unwrap_or(0);
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
    // Debug: `ULTRA_SPACE_START_REGION=<region name (spaces as _) or kind>` starts over the middle of that
    // region, or of the sunniest region of that kind (so test views are in daylight), at `ULTRA_SPACE_START_ALTITUDE` above
    // sea level (default 3 km), still and level, heading east (a round body with geography; on a
    // body of plains, the first plain of that kind, heading along it).
    let to_star = system
        .frames
        .state_in(
            system.bodies[index].inertial_frame,
            system.star().inertial_frame,
            0.0,
        )
        .position
        .normalize_or(DVec3::X);
    let surface_turn = system
        .frames
        .state_in(
            system.bodies[index].inertial_frame,
            system.bodies[index].surface_frame,
            0.0,
        )
        .rotation;
    if let Ok(kind) = std::env::var("ULTRA_SPACE_START_REGION")
        && let Some(terrain) = sim_planet::Terrain::of(&system.bodies[index])
        && let Some(globe) = terrain.globe()
        && let Some(r) = (0..globe.regions.len())
            .find(|&r| {
                globe.regions[r]
                    .name
                    .eq_ignore_ascii_case(&kind.replace('_', " "))
            })
            .or_else(|| {
                (0..globe.regions.len())
                    .filter(|&r| globe.kind(r).id == kind)
                    .max_by(|&a, &b| {
                        let sun = |r: usize| (surface_turn * globe.regions[r].centre).dot(to_star);
                        sun(a).total_cmp(&sun(b))
                    })
            })
    {
        let body = &system.bodies[index];
        let dir = globe.regions[r].centre;
        // An altitude given is kept down to 25 m above the ground (low enough for the block
        // ground to show); the default stays well clear.
        let ground = terrain.surface_height(dir, 100.0).max(0.0);
        let above = match std::env::var("ULTRA_SPACE_START_ALTITUDE")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
        {
            Some(a) => a.max(ground + 25.0),
            None => 3_000.0f64.max(ground + 200.0),
        };
        let frame = system
            .frames
            .state_in(body.inertial_frame, body.surface_frame, 0.0);
        let east = DVec3::Y.cross(dir).normalize_or(DVec3::X);
        let s = &mut state.state;
        s.position = frame.rotation * (dir * (body.radius + above));
        s.velocity = frame.angular_velocity.cross(s.position);
        s.angular_velocity = frame.angular_velocity;
        s.rotation = crate::camera::look_rotation(frame.rotation * east, frame.rotation * dir);
    } else if let Ok(kind) = std::env::var("ULTRA_SPACE_START_REGION")
        && let Some(terrain) = sim_planet::Terrain::of(&system.bodies[index])
        && let Some(world) = terrain.world()
        && let Some(i) = (0..world.regions.len()).find(|&i| world.kind(i).id == kind)
    {
        let body = &system.bodies[index];
        let plain = &world.plains.plains[i];
        let above = std::env::var("ULTRA_SPACE_START_ALTITUDE")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(3_000.0);
        let frame = system
            .frames
            .state_in(body.inertial_frame, body.surface_frame, 0.0);
        let local = plain.origin() + plain.normal * above;
        let s = &mut state.state;
        s.position = frame.rotation * local;
        s.velocity = frame.angular_velocity.cross(s.position);
        s.angular_velocity = frame.angular_velocity;
        s.rotation = crate::camera::look_rotation(
            frame.rotation * plain.east,
            frame.rotation * plain.normal,
        );
    }

    let mut airborne = false;
    // The default start (Batch 5): landed in the demo village, on foot beside the ship.
    let mut on_foot = None;
    if crate::blocks::start_in_village()
        && let Some(world) = blocks.as_deref()
        && world.body == index
        && let Some((ship, walker, t)) = crate::blocks::start(world, system, &sim.0)
    {
        state = ship;
        on_foot = Some(walker);
        sim.0
            .set_universe_time(sim_core::time::UniverseTime::from_nanos((t * 1e9) as u64));
        info!("starting in the demo village {:?}", world.village.name);
    } else if let Some((near, t, up_high)) =
        crate::settlements::start_near(&mut sim.0, system, index)
    {
        state = near;
        airborne = up_high;
        sim.0
            .set_universe_time(sim_core::time::UniverseTime::from_nanos((t * 1e9) as u64));
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
    if airborne || (FlightModel::default().arcade && on_foot.is_none()) {
        // Started in the air: in arcade flight, so it holds still (no gravity) until the
        // controls move it.
        if let Some(loc) = sim.objects.location(id)
            && let Some(region) = sim.regions.get_mut(loc.region)
        {
            region
                .world
                .entity_mut(loc.entity)
                .insert(ArcadeFlight::default());
        }
    }
    commands.insert_resource(PlayerShip { id });
    if let Some(walker) = on_foot
        && let Ok(w) = sim.objects.spawn(
            &mut sim.regions,
            index,
            walker,
            (
                sim_vehicle::Walker::default(),
                sim_vehicle::WalkerControl::default(),
            ),
        )
    {
        commands.insert_resource(crate::walk::OnFoot { id: w, pitch: 0.0 });
        pilot.on_foot = true;
    }
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
    if !pilot.controls_ship() {
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
    if !pilot.controls_ship() || !keys.just_pressed(KeyCode::KeyZ) {
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

/// P: the controlled vehicle steps through its design's poses (task 3.10): each named joint
/// turns toward its angle, every other joint back to rest.
fn pose_key(
    keys: Res<ButtonInput<KeyCode>>,
    pilot: Res<PilotState>,
    ship: Option<Res<PlayerShip>>,
    mut sim: ResMut<Sim>,
    mut next: Local<usize>,
) {
    if !pilot.controls_ship() || !keys.just_pressed(KeyCode::KeyP) {
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
    let mut e = region.world.entity_mut(loc.entity);
    let (Some(vehicle), Some(body)) = (e.get::<Vehicle>(), e.get::<VehicleBody>()) else {
        return;
    };
    if vehicle.poses.is_empty() || !body.articulation.is_jointed() {
        info!("this vehicle has no poses");
        return;
    }
    let count = vehicle.poses.len();
    let pose = vehicle.poses[*next % count].name.clone();
    // Leaving the pose P went to last (its stages, in reverse), into this one (its stages).
    let from = (*next > 0).then(|| vehicle.poses[(*next - 1) % count].name.clone());
    *next += 1;
    let Some(mut route) =
        sim_vehicle::joints::pose_route(vehicle, &body.articulation, from.as_deref(), &pose)
    else {
        return;
    };
    if let Some(mut joints) = e.get_mut::<sim_vehicle::joints::JointState>() {
        joints.targets = route.remove(0);
        joints.queue = route.into();
        info!("pose: {pose}");
    }
}

/// N (debug): toggles arcade flight (No Man's Sky style: no gravity, the ship goes where the
/// nose points; W/S change the held speed). Turning it on keeps the current forward speed;
/// turning it off hands the current velocity back to Newtonian flight.
/// M: switches a legged vehicle's legs (a design `gait`, task 3.11) off and on. With legs on
/// (the default), it stands on them whenever it is on the ground and slow, and W/S walk, A/D
/// turn, Space takes off. With legs off it stops on both feet and is a plain rigid body.
fn walk_key(
    keys: Res<ButtonInput<KeyCode>>,
    pilot: Res<PilotState>,
    ship: Option<Res<PlayerShip>>,
    mut notice: ResMut<crate::hud::Notice>,
    real: Res<Time<Real>>,
    mut sim: ResMut<Sim>,
) {
    if !pilot.controls_ship() || !keys.just_pressed(KeyCode::KeyM) {
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
    let now = real.elapsed_secs_f64();
    if e.get::<Vehicle>().is_none_or(|v| v.gait.is_none()) {
        notice.show("This vehicle has no legs", now);
        return;
    }
    let text = if e.contains::<LegsOff>() {
        e.remove::<LegsOff>();
        "Legs on: stands and walks on the ground (W/S walk, A/D turn, Space takes off)"
    } else {
        e.insert(LegsOff);
        if let Some(mut walking) = e.get_mut::<Walking>() {
            // Stops on both feet, then hands over to plain physics.
            walking.stopping = true;
        }
        "Legs off: a rigid body on its feet (M: legs on)"
    };
    if let Some(mut control) = e.get_mut::<FlightControl>() {
        control.desired_velocity = None;
    }
    info!("{text}");
    notice.show(text, now);
}

fn arcade_key(
    keys: Res<ButtonInput<KeyCode>>,
    ship: Option<Res<PlayerShip>>,
    pilot: Res<PilotState>,
    mut model: ResMut<FlightModel>,
    mut sim: ResMut<Sim>,
) {
    if !pilot.controls_ship() || !keys.just_pressed(KeyCode::KeyN) {
        return;
    }
    model.arcade = !model.arcade;
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
        e.insert(ArcadeFlight::at(
            forward_speed.min(sim_vehicle::arcade::MAX_SPEED),
        ));
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
    if pilot.controls_ship() && keys.just_pressed(KeyCode::KeyJ) {
        cruise.toggle = true;
        *notice = 0.0;
    } else if cruise.blocked.is_some() {
        *notice += time.delta_secs();
        if *notice > CRUISE_NOTICE_TIME {
            cruise.blocked = None;
        }
    }
}

/// No Man's Sky flight rules for the player's ship: a landed ship lifts off into arcade flight as
/// soon as the pilot moves the controls; L sets it down (low and slow only), and once down it
/// goes back to normal flight, which lets it settle and rest (LANDED, F to step out).
fn arcade_rules(
    keys: Res<ButtonInput<KeyCode>>,
    ship: Option<Res<PlayerShip>>,
    pilot: Res<PilotState>,
    model: Res<FlightModel>,
    real: Res<Time<Real>>,
    mut notice: ResMut<crate::hud::Notice>,
    mut sim: ResMut<Sim>,
) {
    if !model.arcade {
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
    let now = real.elapsed_secs_f64();
    match e.get::<ArcadeFlight>().copied() {
        Some(arcade) if arcade.touched_down => {
            e.remove::<ArcadeFlight>();
            if let Some(mut control) = e.get_mut::<FlightControl>() {
                control.desired_velocity = None;
            }
            notice.show("Landed", now);
        }
        Some(arcade) => {
            if pilot.controls_ship() && keys.just_pressed(KeyCode::KeyL) {
                let (low, slow) = e.get::<FlightTelemetry>().map_or((false, false), |t| {
                    (
                        t.ground_altitude < LAND_HEIGHT,
                        t.relative_velocity.length() < LAND_SPEED_LIMIT,
                    )
                });
                if arcade.landing {
                    if let Some(mut a) = e.get_mut::<ArcadeFlight>() {
                        a.landing = false;
                    }
                    notice.show("Landing cancelled", now);
                } else if low && slow {
                    if let Some(mut a) = e.get_mut::<ArcadeFlight>() {
                        a.landing = true;
                    }
                    notice.show("Landing", now);
                } else {
                    notice.show("Too high or too fast to land", now);
                }
            }
        }
        None => {
            let moving = e
                .get::<FlightControl>()
                .is_some_and(|c| c.translate != DVec3::ZERO);
            if pilot.controls_ship() && moving {
                e.insert(ArcadeFlight::default());
                info!("lift-off: arcade flight");
            }
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

    if !pilot.controls_ship() {
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
        if pilot.flying && !pilot.mouse_free && !pilot.building {
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

/// Whether the player ship is walking, and its walking speed.
pub fn player_walking_speed(sim: &Simulation, ship: &PlayerShip) -> Option<f64> {
    let loc = sim.objects.location(ship.id)?;
    let world = &sim.regions.get(loc.region)?.world;
    world.get::<Walking>(loc.entity).map(|w| w.speed)
}

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
        (CruisePhase::Off, Some(CruiseBlocked::Overloaded(load))) => {
            format!("  CRUISE DRIVE OVERLOADED ({load:.1}x its rated mass)")
        }
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
    ship: Option<Res<PlayerShip>>,
    tracked: Res<ShipRenderState>,
    universe: Res<Universe>,
    sim: Res<Sim>,
    render: Res<RenderTime>,
    mut camera: ResMut<SpaceCamera>,
) {
    if !pilot.controls_ship() {
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
    let size = ship
        .and_then(|ship| {
            let loc = sim.0.objects.location(ship.id)?;
            let world = &sim.0.regions.get(loc.region)?.world;
            world.get::<Vehicle>(loc.entity).map(crate::walk::reach)
        })
        .unwrap_or(0.0);
    let chase = (size / CHASE_SIZE).max(1.0);
    let (offset, rotation) = match pilot.view {
        ShipView::Chase => {
            let orbit =
                DQuat::from_rotation_y(pilot.orbit.0) * DQuat::from_rotation_x(pilot.orbit.1);
            let look = DQuat::from_rotation_x(-0.12);
            (
                state.rotation * orbit * CHASE_OFFSET * chase,
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
    if r > body.radius + terrain.height_range().1 + 0.3 * terrain.params.relief + CAMERA_CLEARANCE {
        return point;
    }
    let surface = system
        .frames
        .state_in(body.inertial_frame, body.surface_frame, t)
        .rotation;
    let dir = surface.inverse() * point.normalize();
    let ground = terrain.surface_height(dir, 2.0);
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

/// The starlight's pose and light.
type StarlightParts = (&'static mut Transform, &'static mut DirectionalLight);
type StarlightOnly = (With<Starlight>, Without<MainCamera>);

fn spawn_starlight(mut commands: Commands) {
    let illuminance = STAR_LIGHT * std::f32::consts::PI / Exposure::default().exposure();
    commands.spawn((
        Starlight,
        DirectionalLight {
            illuminance,
            shadow_maps_enabled: true,
            ..default()
        },
        // Shadows near the camera only (vehicles, the ground under them).
        bevy::light::CascadeShadowConfigBuilder {
            num_cascades: 3,
            minimum_distance: 0.5,
            first_cascade_far_bound: 40.0,
            maximum_distance: 600.0,
            ..default()
        }
        .build(),
        Transform::default(),
    ));
}

fn aim_starlight(
    universe: Res<Universe>,
    render: Res<RenderTime>,
    camera: Res<SpaceCamera>,
    weather: Option<Res<crate::weather::Weather>>,
    mut lights: Query<StarlightParts, StarlightOnly>,
) {
    // Clouds soften the sun (rain, fog).
    let dim = weather.map_or(1.0, |w| w.sun());
    let illuminance = STAR_LIGHT * std::f32::consts::PI / Exposure::default().exposure() * dim;
    for (_, mut light) in &mut lights {
        if (light.illuminance - illuminance).abs() > 1.0 {
            light.illuminance = illuminance;
        }
    }
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
        for (mut transform, _) in &mut lights {
            *transform = Transform::default().looking_to(direction, Vec3::Y);
        }
    }
}

/// Sky light at full strength (cd/m²): about a sixth of the starlight, as a clear sky.
const SKY_LIGHT: f32 = 180.0;
/// Sky light left in space and at night (bounce from the planet, stars).
const SKY_FLOOR: f32 = 0.08;

/// Turns the sky light (the camera's environment map) to the local up over the anchor body and
/// sets its strength: full by day low down, fading at night and with height to a floor.
fn aim_sky(
    universe: Res<Universe>,
    render: Res<RenderTime>,
    camera: Res<SpaceCamera>,
    weather: Option<Res<crate::weather::Weather>>,
    mut lights: Query<&mut EnvironmentMapLight, With<MainCamera>>,
) {
    let system = &universe.0;
    let body = &system.bodies[camera.anchor];
    let up = camera.offset.normalize_or_zero();
    if up == DVec3::ZERO {
        return;
    }
    let star = system.frames.transform_point(
        body.inertial_frame,
        system.star().inertial_frame,
        render.0,
        DVec3::ZERO,
    ) - camera.offset;
    let day = up.dot(star.normalize_or_zero()).clamp(0.0, 1.0).sqrt();
    let height = (camera.offset.length() - body.radius).max(0.0);
    let air = body.atmosphere.as_ref().map_or(0.0, |a| a.height).max(1.0);
    let in_air = (1.0 - height / (air * 2.0)).clamp(0.0, 1.0);
    let strength = SKY_FLOOR + (1.0 - SKY_FLOOR) * day as f32 * in_air as f32;
    for mut light in &mut lights {
        light.rotation = Quat::from_rotation_arc(Vec3::Y, up.as_vec3());
        light.intensity = SKY_LIGHT * strength * weather.as_deref().map_or(1.0, |w| w.sky());
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
