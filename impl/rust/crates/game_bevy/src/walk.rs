//! Walking on foot (Batch 2, task 2.9): leaving and boarding the ship, first-person controls
//! and camera. The walker itself is simulated in `sim_vehicle::walker`.
//!
//! F next to a landed (or nearly still, low) ship steps out beside it; F within reach of the
//! ship climbs back in. On foot: WASD walk, Shift run, Space jump, mouse looks (turning the
//! body left and right, the head up and down).

use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use glam::{DQuat, DVec2, DVec3};
use sim_core::{ObjectId, ObjectState, Simulation};
use sim_vehicle::walker::EYE_HEIGHT;
use sim_vehicle::{FlightTelemetry, Walker, WalkerControl};

use crate::camera::{CameraBlend, CameraSet, CameraStage, SpaceCamera};
use crate::ship::{PilotState, PlayerShip, interpolate_state};
use crate::sim::{RenderTime, Sim, SimSet};
use crate::universe::{InGame, Universe};

/// The ship can be left when touching the ground, or this low and this slow relative to it.
const EXIT_MAX_ALTITUDE: f64 = 30.0;
const EXIT_MAX_SPEED: f64 = 2.0;
/// Where the walker appears: this far to the ship's right (m).
const EXIT_OFFSET: f64 = 6.0;
/// Boarding reach from the ship's centre (m).
const BOARD_REACH: f64 = 12.0;
/// Mouse look sensitivity (radians per pixel).
const LOOK_SENSITIVITY: f64 = 0.0025;

pub struct WalkPlugin;

impl Plugin for WalkPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<WalkerRenderState>().add_systems(
            Update,
            (
                (exit_or_board, walker_input).chain().before(SimSet),
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
    mut render: ResMut<WalkerRenderState>,
    mut blend: ResMut<CameraBlend>,
    camera: Res<SpaceCamera>,
    real: Res<Time<Real>>,
    mut sim: ResMut<Sim>,
) {
    if !pilot.flying || !keys.just_pressed(KeyCode::KeyF) {
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
        // Board: close enough to the ship?
        let Some((region, state, _)) = walker_status(sim, on_foot.id) else {
            return;
        };
        let here = state
            .in_frame(&system.frames, system.bodies[region].inertial_frame, t)
            .in_frame(&system.frames, ship_frame, t);
        let distance = (here.state.position - ship_state.state.position).length();
        if distance > BOARD_REACH {
            info!("too far from the ship to board ({distance:.0} m)");
            return;
        }
        let _ = sim.objects.despawn(&mut sim.regions, on_foot.id);
        blend.start(&camera, now);
        commands.remove_resource::<OnFoot>();
        pilot.on_foot = false;
        info!("boarded the ship");
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
    let settled = telemetry.in_contact
        || (telemetry.ground_altitude < EXIT_MAX_ALTITUDE
            && telemetry.relative_velocity.length() < EXIT_MAX_SPEED);
    if !settled {
        info!(
            "can't leave the ship: land first ({:.0} m above ground, {:.1} m/s)",
            telemetry.ground_altitude,
            telemetry.relative_velocity.length()
        );
        return;
    }
    let s = ship_state.state;
    let up = s.position.normalize();
    let right = s.rotation * DVec3::X;
    let side = (right - up * right.dot(up)).normalize_or(up.any_orthonormal_vector());
    let position = s.position + side * EXIT_OFFSET + up * 1.0;
    let env = sim_vehicle::flight::body_env(system, ship_loc.region, t);
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
    if !pilot.flying {
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
