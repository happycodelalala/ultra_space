//! Walking on foot (Batch 2, task 2.9): leaving and boarding the ship, first-person controls
//! and camera. The walker itself is simulated in `sim_vehicle::walker`.
//!
//! F next to a landed (or nearly still, low) ship steps out beside it; F within reach of the
//! ship climbs back in. On foot: WASD walk, Shift run, Space jump, mouse looks (turning the
//! body left and right, the head up and down).

use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use glam::{DQuat, DVec2, DVec3};
use sim_core::objects::Object;
use sim_core::{ObjectId, ObjectState, Simulation};
use sim_vehicle::walker::EYE_HEIGHT;
use sim_vehicle::{
    ArcadeFlight, Cruise, FlightControl, FlightTelemetry, Vehicle, VehicleBody, Walker,
    WalkerControl,
};

use crate::camera::{CameraBlend, CameraSet, CameraStage, SpaceCamera};
use crate::ship::{PilotState, PlayerShip, ShipRenderState, interpolate_state};
use crate::sim::{RenderTime, Sim, SimSet};
use crate::universe::{InGame, Universe};

/// The ship can be left when touching the ground, or this low and this slow relative to it.
const EXIT_MAX_ALTITUDE: f64 = 30.0;
const EXIT_MAX_SPEED: f64 = 2.0;
/// Where the walker appears: this far to the ship's right of its widest part, on the ground
/// (m).
const EXIT_OFFSET: f64 = 3.0;
/// Boarding reach from the nearest of a vehicle's parts (m; from the part's outside, roughly:
/// its centre less half its largest dimension).
const BOARD_REACH: f64 = 12.0;
/// A vehicle counts as ahead when it is within about 60° of where the walker faces.
const BOARD_FACING: f64 = 0.5;
/// Where G puts a rover: this far ahead of the walker (m).
const ROVER_DISTANCE: f64 = 8.0;
/// Mouse look sensitivity (radians per pixel).
const LOOK_SENSITIVITY: f64 = 0.0025;

pub struct WalkPlugin;

impl Plugin for WalkPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<WalkerRenderState>().add_systems(
            Update,
            (
                (exit_or_board, spawn_rover_key, walker_input)
                    .chain()
                    .before(SimSet),
                track_walker.after(SimSet).before(CameraSet),
                follow_walker.in_set(CameraStage::Follow),
            )
                .in_set(InGame),
        );
    }
}

/// The player is on foot as this walker; `pitch` is where the head looks (camera only).
#[derive(Resource)]
pub struct OnFoot {
    pub id: ObjectId,
    pub pitch: f64,
}

/// The walker's state for rendering, interpolated between ticks.
#[derive(Resource, Default)]
struct WalkerRenderState {
    previous: Option<(f64, ObjectState)>,
    current: Option<(f64, ObjectState)>,
    state: Option<ObjectState>,
}

/// The walker's region, state and walker data.
pub fn walker_status(sim: &Simulation, id: ObjectId) -> Option<(usize, ObjectState, Walker)> {
    let loc = sim.objects.location(id)?;
    let world = &sim.regions.get(loc.region)?.world;
    Some((
        loc.region,
        *world.get::<ObjectState>(loc.entity)?,
        *world.get::<Walker>(loc.entity)?,
    ))
}

/// Rotation with forward (−Z) along `forward` (projected level) and +Y along `up`.
fn upright(forward: DVec3, up: DVec3) -> DQuat {
    let f = (forward - up * forward.dot(up)).normalize_or(up.any_orthonormal_vector());
    let right = f.cross(up);
    DQuat::from_mat3(&glam::DMat3::from_cols(right, up, -f)).normalize()
}

#[allow(clippy::too_many_arguments)]
fn exit_or_board(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    universe: Res<Universe>,
    ship: Option<Res<PlayerShip>>,
    on_foot: Option<Res<OnFoot>>,
    mut pilot: ResMut<PilotState>,
    mut notice: ResMut<crate::hud::Notice>,
    mut render: ResMut<WalkerRenderState>,
    mut blend: ResMut<CameraBlend>,
    camera: Res<SpaceCamera>,
    real: Res<Time<Real>>,
    mut sim: ResMut<Sim>,
    mut tracked: ResMut<ShipRenderState>,
) {
    if !pilot.flying || pilot.building || !keys.just_pressed(KeyCode::KeyF) {
        return;
    }
    let now = real.elapsed_secs_f64();
    let Some(ship) = ship else { return };
    let system = &universe.0;
    let sim = &mut sim.0;
    let t = sim.universe_time().as_secs_f64();
    let Some(ship_loc) = sim.objects.location(ship.id) else {
        return;
    };
    let ship_frame = system.bodies[ship_loc.region].inertial_frame;
    let Some(ship_state) = sim
        .objects
        .state(&sim.regions, ship.id)
        .map(|s| s.in_frame(&system.frames, ship_frame, t))
    else {
        return;
    };

    if let Some(on_foot) = on_foot {
        // Board the nearest vehicle within reach (it becomes the one the player controls).
        let Some((region, state, _)) = walker_status(sim, on_foot.id) else {
            return;
        };
        let frame = system.bodies[region].inertial_frame;
        let here = state.in_frame(&system.frames, frame, t).state.position;
        // Of the vehicles in reach, the one most directly ahead (else the nearest).
        let look = state.in_frame(&system.frames, frame, t).state.rotation * DVec3::NEG_Z;
        let mut in_reach: Vec<(ObjectId, String, f64, f64)> = sim
            .regions
            .get_mut(region)
            .map(|r| {
                let mut query = r.world.query::<(&Object, &ObjectState, &Vehicle)>();
                query
                    .iter(&r.world)
                    .map(|(o, s, v)| {
                        let s = s.in_frame(&system.frames, frame, t).state;
                        let to = s.position - here;
                        let facing = to.normalize_or_zero().dot(look);
                        (o.id, v.name.clone(), nearest_part(&s, v, here), facing)
                    })
                    .collect()
            })
            .unwrap_or_default();
        in_reach.retain(|c| c.2 <= BOARD_REACH);
        let chosen = in_reach
            .iter()
            .filter(|c| c.3 > BOARD_FACING)
            .max_by(|a, b| a.3.total_cmp(&b.3))
            .or_else(|| in_reach.iter().min_by(|a, b| a.2.total_cmp(&b.2)))
            .cloned();
        let Some((id, name, _, _)) = chosen else {
            info!("no vehicle within {BOARD_REACH:.0} m to board");
            return;
        };
        let _ = sim.objects.despawn(&mut sim.regions, on_foot.id);
        blend.start(&camera, now);
        commands.remove_resource::<OnFoot>();
        if id != ship.id {
            commands.insert_resource(PlayerShip { id });
            *tracked = ShipRenderState::default();
        }
        pilot.on_foot = false;
        info!("boarded the {name}");
        return;
    }

    // Leave: only when landed, or low and nearly still relative to the ground.
    let Some(telemetry) = sim
        .regions
        .get(ship_loc.region)
        .and_then(|r| r.world.get::<FlightTelemetry>(ship_loc.entity))
        .cloned()
    else {
        return;
    };
    // Low (or touching the ground) and nearly still: a ship skidding along the ground counts as
    // touching it, but isn't safe to leave.
    let low = telemetry.in_contact || telemetry.ground_altitude < EXIT_MAX_ALTITUDE;
    let speed = telemetry.relative_velocity.length();
    let arcade = sim
        .regions
        .get(ship_loc.region)
        .is_some_and(|r| r.world.get::<ArcadeFlight>(ship_loc.entity).is_some());
    if !low || speed >= EXIT_MAX_SPEED {
        let text = if !low {
            format!(
                "Can't leave the ship: land first ({:.0} m above the ground)",
                telemetry.ground_altitude
            )
        } else if arcade {
            format!("Can't leave the ship: slow down first ({speed:.0} m/s; X brakes)")
        } else {
            format!("Can't leave the ship: slow down first ({speed:.0} m/s)")
        };
        info!("{text}");
        notice.show(text, now);
        return;
    }
    if arcade {
        // Hovering low in arcade flight: back to normal flight, so the ship settles onto the
        // ground once the pilot is out.
        if let Some(region) = sim.regions.get_mut(ship_loc.region)
            && let Ok(mut e) = region.world.get_entity_mut(ship_loc.entity)
        {
            e.remove::<ArcadeFlight>();
            if let Some(mut control) = e.get_mut::<FlightControl>() {
                control.desired_velocity = None;
            }
        }
        info!("arcade flight off: stepping out");
    }
    let s = ship_state.state;
    let up = s.position.normalize();
    let right = s.rotation * DVec3::X;
    let side = (right - up * right.dot(up)).normalize_or(up.any_orthonormal_vector());
    let Some(extent) = sim
        .regions
        .get(ship_loc.region)
        .and_then(|r| r.world.get::<Vehicle>(ship_loc.entity))
        .map(horizontal_extent)
    else {
        return;
    };
    let beside = (s.position + side * (extent + EXIT_OFFSET)).normalize();
    let env = sim_vehicle::flight::region_env(
        system,
        ship_loc.region,
        t,
        sim.regions
            .get(ship_loc.region)
            .and_then(|r| r.world.get_resource::<sim_vehicle::flight::RegionTerrain>()),
    );
    let position = beside * (env.radius + env.ground_height(beside * env.radius) + 1.0);
    let ground_velocity = env.spin.cross(position);
    let mut state =
        ObjectState::at_rest(ship_frame, position, upright(s.rotation * DVec3::NEG_Z, up));
    state.state.velocity = ground_velocity;
    let Ok(id) = sim.objects.spawn(
        &mut sim.regions,
        ship_loc.region,
        state,
        (Walker::default(), WalkerControl::default()),
    ) else {
        return;
    };
    *render = WalkerRenderState::default();
    blend.start(&camera, now);
    commands.insert_resource(OnFoot { id, pitch: 0.0 });
    pilot.on_foot = true;
    info!("stepped out of the ship");
}

/// A state `distance` metres ahead of the walker, `height` metres above the ground there,
/// upright and facing the walker's way, moving with the turning ground (in the walker's region
/// body's inertial frame). Returns the region too.
pub fn spot_ahead(
    sim: &Simulation,
    system: &sim_core::StarSystem,
    walker: ObjectId,
    distance: f64,
    height: f64,
) -> Option<(usize, ObjectState)> {
    let t = sim.universe_time().as_secs_f64();
    let (region, state, _) = walker_status(sim, walker)?;
    let frame = system.bodies[region].inertial_frame;
    let s = state.in_frame(&system.frames, frame, t).state;
    let forward = s.rotation * DVec3::NEG_Z;
    let env = sim_vehicle::flight::region_env(
        system,
        region,
        t,
        sim.regions
            .get(region)
            .and_then(|r| r.world.get_resource::<sim_vehicle::flight::RegionTerrain>()),
    );
    let spot = (s.position + forward * distance).normalize();
    let position = spot * (env.radius + env.ground_height(spot * env.radius) + height);
    let mut placed = ObjectState::at_rest(frame, position, upright(forward, spot));
    placed.state.velocity = env.spin.cross(position);
    placed.state.angular_velocity = env.spin;
    Some((region, placed))
}

/// Debug (G, on foot): a vehicle appears ahead, standing on the ground (task 3.5). Shift+G
/// chooses which design (the rover first, then every other design in turn).
fn spawn_rover_key(
    keys: Res<ButtonInput<KeyCode>>,
    universe: Res<Universe>,
    designs: Res<crate::universe::Designs>,
    on_foot: Option<Res<OnFoot>>,
    mut sim: ResMut<Sim>,
    mut chosen: Local<Option<String>>,
) {
    if !keys.just_pressed(KeyCode::KeyG) {
        return;
    }
    let ids: Vec<&String> = designs.0.keys().collect();
    let current = chosen
        .clone()
        .filter(|c| designs.0.contains_key(c))
        .unwrap_or_else(|| "rover".into());
    if keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight) {
        let i = ids.iter().position(|d| **d == current).unwrap_or(0);
        let next = ids[(i + 1) % ids.len()].clone();
        info!("G now spawns: {}", designs.0[&next].name);
        *chosen = Some(next);
        return;
    }
    let Some(on_foot) = on_foot else {
        info!("G spawns a vehicle when you're on foot");
        return;
    };
    let Some(design) = designs.0.get(&current) else {
        return;
    };
    let vehicle = Vehicle::from_design(design);
    let Some(body) = VehicleBody::of(&vehicle) else {
        return;
    };
    // Far enough ahead for its size, its lowest part just above the ground.
    let lowest = vehicle
        .parts
        .iter()
        .map(|p| p.position.y - shape_size(p.def.shape))
        .fold(0.0f64, f64::min);
    let distance = ROVER_DISTANCE.max(horizontal_extent(&vehicle) + 4.0);
    let system = &universe.0;
    let sim = &mut sim.0;
    let Some((region, state)) = spot_ahead(sim, system, on_foot.id, distance, 0.5 - lowest) else {
        return;
    };
    let spawned = sim.objects.spawn(
        &mut sim.regions,
        region,
        state,
        (
            vehicle,
            body,
            FlightControl::default(),
            FlightTelemetry::default(),
            Cruise::default(),
        ),
    );
    if spawned.is_ok() {
        info!(
            "a {} appeared {distance:.0} m ahead (F to board it)",
            design.name
        );
    }
}

/// Distance from `here` to the nearest of a vehicle's parts (m), so big vehicles can be reached
/// from the ground (a frame's torso is 13 m up).
pub fn nearest_part(state: &sim_core::frames::FrameState, vehicle: &Vehicle, here: DVec3) -> f64 {
    vehicle
        .parts
        .iter()
        .map(|p| {
            let centre = (state.position + state.rotation * p.position - here).length();
            (centre - shape_size(p.def.shape)).max(0.0)
        })
        .fold(f64::MAX, f64::min)
}

/// How far a vehicle's parts reach out from its origin (m; each part counted as a ball of half
/// its largest dimension).
pub fn reach(vehicle: &Vehicle) -> f64 {
    vehicle
        .parts
        .iter()
        .map(|p| p.position.length() + shape_size(p.def.shape))
        .fold(0.0, f64::max)
}

/// How far a vehicle's parts reach out from its origin across its X-Z plane (m; each part
/// counted as a ball of half its largest dimension).
fn horizontal_extent(vehicle: &Vehicle) -> f64 {
    vehicle
        .parts
        .iter()
        .map(|p| DVec3::new(p.position.x, 0.0, p.position.z).length() + shape_size(p.def.shape))
        .fold(0.0, f64::max)
}

/// Half the largest dimension of a part shape (m).
fn shape_size(shape: sim_vehicle::Shape) -> f64 {
    match shape {
        sim_vehicle::Shape::Box { size_m } => size_m.iter().cloned().fold(0.0, f64::max) / 2.0,
        sim_vehicle::Shape::Cylinder { radius_m, length_m } => radius_m.max(length_m / 2.0),
        sim_vehicle::Shape::Sphere { radius_m } => radius_m,
        sim_vehicle::Shape::Shell { .. } => {
            let (mid, half) = shape.bounds();
            (half + mid.abs()).max_element()
        }
    }
}

fn walker_input(
    keys: Res<ButtonInput<KeyCode>>,
    motion: Res<AccumulatedMouseMotion>,
    pilot: Res<PilotState>,
    on_foot: Option<ResMut<OnFoot>>,
    mut sim: ResMut<Sim>,
) {
    let Some(mut on_foot) = on_foot else { return };
    let sim = &mut sim.0;
    let Some(loc) = sim.objects.location(on_foot.id) else {
        return;
    };
    let Some(mut control) = sim
        .regions
        .get_mut(loc.region)
        .and_then(|r| r.world.get_mut::<WalkerControl>(loc.entity))
    else {
        return;
    };
    if pilot.building {
        // Standing still while the builder has the keys.
        *control = WalkerControl::default();
        return;
    }
    let axis =
        |pos: KeyCode, neg: KeyCode| f64::from(keys.pressed(pos)) - f64::from(keys.pressed(neg));
    control.movement = DVec2::new(
        axis(KeyCode::KeyD, KeyCode::KeyA),
        axis(KeyCode::KeyW, KeyCode::KeyS),
    );
    control.run = keys.pressed(KeyCode::ShiftLeft);
    control.jump |= keys.just_pressed(KeyCode::Space);
    if !pilot.mouse_free {
        let d = motion.delta.as_dvec2() * LOOK_SENSITIVITY;
        control.turn -= d.x;
        on_foot.pitch = (on_foot.pitch - d.y).clamp(-1.5, 1.5);
    }
}

fn track_walker(
    sim: Res<Sim>,
    universe: Res<Universe>,
    render: Res<RenderTime>,
    on_foot: Option<Res<OnFoot>>,
    mut tracked: ResMut<WalkerRenderState>,
) {
    let Some((region, state, _)) = on_foot.and_then(|f| walker_status(&sim.0, f.id)) else {
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

/// First-person camera at eye height, looking along the walker's heading and head pitch.
fn follow_walker(
    pilot: Res<PilotState>,
    on_foot: Option<Res<OnFoot>>,
    tracked: Res<WalkerRenderState>,
    universe: Res<Universe>,
    mut camera: ResMut<SpaceCamera>,
) {
    let (Some(on_foot), Some(object)) = (on_foot, tracked.state) else {
        return;
    };
    if !pilot.flying || pilot.building {
        return;
    }
    let Some(region) = universe
        .0
        .bodies
        .iter()
        .position(|b| b.inertial_frame == object.frame)
    else {
        return;
    };
    let s = object.state;
    let up = s.position.normalize();
    camera.anchor = region;
    camera.offset = s.position + up * EYE_HEIGHT;
    camera.rotation = (s.rotation * DQuat::from_rotation_x(on_foot.pitch)).normalize();
}
