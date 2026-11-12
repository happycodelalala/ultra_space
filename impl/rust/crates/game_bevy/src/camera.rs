//! Space camera with a floating origin (Batch 1, task 1.6).
//!
//! The camera's pose is `f64`, relative to an **anchor body's inertial frame**, so it moves with
//! that body along its orbit. The anchor switches automatically to the smallest sphere of
//! influence containing the camera. Rendering is camera-relative: the camera sits at the render
//! origin and everything else is placed relative to it (see `space_render`).
//!
//! When flying, the ship module drives the camera (chase or cockpit view). Tab switches to this
//! free debug camera:
//! - hold right mouse button and move the mouse to look; Q/E roll
//! - W/S forward/back, A/D left/right, R/F up/down; Shift ×10
//! - mouse wheel changes speed; number keys 1–8 jump to a body

use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll};
use bevy::prelude::*;
use glam::{DMat3, DQuat, DVec3};
use sim_core::StarSystem;

use crate::ship::PilotState;
use crate::sim::Sim;
use crate::universe::{GameState, InGame, Universe};

/// Camera pose in `f64`, relative to the anchor body's inertial frame. All inertial frames share
/// the same axes (orbits don't rotate), so `rotation` stays valid across anchor switches.
#[derive(Resource, Clone, Debug)]
pub struct SpaceCamera {
    pub anchor: usize,
    pub offset: DVec3,
    pub rotation: DQuat,
    /// Movement speed (m/s).
    pub speed: f64,
}

const MOUSE_SENSITIVITY: f64 = 0.003;
const ROLL_SPEED: f64 = 1.2;
const MIN_SPEED: f64 = 1.0;
const MAX_SPEED: f64 = 1.0e10;

/// Camera movement and anchor updates; rendering runs after this set.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct CameraSet;

/// Initial camera: looking at `ULTRA_SPACE_VIEW` (a body id; default Meridian).
fn init_camera(mut commands: Commands, universe: Res<Universe>) {
    let start = std::env::var("ULTRA_SPACE_VIEW").unwrap_or_else(|_| "meridian".into());
    let index = universe
        .0
        .bodies
        .iter()
        .position(|b| b.id == start)
        .unwrap_or(0);
    commands.insert_resource(view_of_body(&universe.0, index, 0.0));
}

/// Order within [`CameraSet`]: free-camera controls (only when not flying), following the
/// ship (when flying), then the anchor update.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CameraStage {
    Controls,
    Follow,
    Anchor,
}

pub struct SpaceCameraPlugin;

impl Plugin for SpaceCameraPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(GameState::Running), init_camera)
            .configure_sets(
                Update,
                (
                    CameraStage::Controls,
                    CameraStage::Follow,
                    CameraStage::Anchor,
                )
                    .chain()
                    .in_set(CameraSet),
            )
            .configure_sets(Update, CameraSet.in_set(InGame))
            .configure_sets(Update, CameraSet.after(crate::sim::SimSet))
            .add_systems(
                Update,
                (camera_controls, jump_to_body)
                    .chain()
                    .in_set(CameraStage::Controls)
                    .run_if(|pilot: Res<PilotState>| !pilot.flying),
            )
            .add_systems(Update, update_anchor.in_set(CameraStage::Anchor));
    }
}

/// Rotation whose -Z (Bevy's forward) points along `forward`, with +Y as close to `up` as
/// possible.
pub fn look_rotation(forward: DVec3, up: DVec3) -> DQuat {
    let z = -forward.normalize();
    let x = up
        .cross(z)
        .try_normalize()
        .unwrap_or_else(|| z.any_orthonormal_vector());
    let y = z.cross(x);
    DQuat::from_mat3(&DMat3::from_cols(x, y, z)).normalize()
}

/// A camera looking at body `index` from its lit side, at a few radii.
pub fn view_of_body(system: &StarSystem, index: usize, t: f64) -> SpaceCamera {
    let body = &system.bodies[index];
    let star = system.star();
    let to_star = system
        .frames
        .state_in(body.inertial_frame, star.inertial_frame, t)
        .position
        .try_normalize()
        .unwrap_or(DVec3::Z);
    // From the star-facing side, turned 40° so the terminator shows.
    let dir = DQuat::from_rotation_y(40f64.to_radians()) * to_star;
    let distance = body.radius * 3.0 + body.atmosphere.as_ref().map_or(0.0, |a| a.height);
    let offset = dir * distance;
    SpaceCamera {
        anchor: index,
        offset,
        rotation: look_rotation(-offset, DVec3::Y),
        speed: (distance / 10.0).clamp(MIN_SPEED, MAX_SPEED),
    }
}

/// The body whose sphere of influence contains `point` (given in `from`'s inertial frame),
/// preferring the smallest sphere. Returns the body index and the point in its frame.
pub fn anchor_for(system: &StarSystem, from: usize, point: DVec3, t: f64) -> (usize, DVec3) {
    let from_frame = system.bodies[from].inertial_frame;
    let mut best = (
        0,
        system
            .frames
            .transform_point(system.star().inertial_frame, from_frame, t, point),
    );
    let mut best_soi = f64::INFINITY;
    for (i, body) in system.bodies.iter().enumerate().skip(1) {
        let p = system
            .frames
            .transform_point(body.inertial_frame, from_frame, t, point);
        if p.length() < body.soi_radius && body.soi_radius < best_soi {
            best = (i, p);
            best_soi = body.soi_radius;
        }
    }
    best
}

fn camera_controls(
    time: Res<Time<Real>>,
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    mut camera: ResMut<SpaceCamera>,
) {
    let dt = time.delta_secs_f64();
    if buttons.pressed(MouseButton::Right) && motion.delta != Vec2::ZERO {
        let d = motion.delta.as_dvec2() * MOUSE_SENSITIVITY;
        camera.rotation =
            (camera.rotation * DQuat::from_rotation_y(-d.x) * DQuat::from_rotation_x(-d.y))
                .normalize();
    }
    let roll = f64::from(keys.pressed(KeyCode::KeyQ)) - f64::from(keys.pressed(KeyCode::KeyE));
    if roll != 0.0 {
        camera.rotation =
            (camera.rotation * DQuat::from_rotation_z(roll * ROLL_SPEED * dt)).normalize();
    }
    if scroll.delta.y != 0.0 {
        camera.speed =
            (camera.speed * 1.25f64.powf(f64::from(scroll.delta.y))).clamp(MIN_SPEED, MAX_SPEED);
    }

    let axis =
        |pos: KeyCode, neg: KeyCode| f64::from(keys.pressed(pos)) - f64::from(keys.pressed(neg));
    let local = DVec3::new(
        axis(KeyCode::KeyD, KeyCode::KeyA),
        axis(KeyCode::KeyR, KeyCode::KeyF),
        axis(KeyCode::KeyS, KeyCode::KeyW),
    );
    if local != DVec3::ZERO {
        let boost = if keys.pressed(KeyCode::ShiftLeft) {
            10.0
        } else {
            1.0
        };
        let step = camera.rotation * local.normalize() * camera.speed * boost * dt;
        camera.offset += step;
    }
}

pub const BODY_KEYS: [KeyCode; 8] = [
    KeyCode::Digit1,
    KeyCode::Digit2,
    KeyCode::Digit3,
    KeyCode::Digit4,
    KeyCode::Digit5,
    KeyCode::Digit6,
    KeyCode::Digit7,
    KeyCode::Digit8,
];

fn jump_to_body(
    keys: Res<ButtonInput<KeyCode>>,
    universe: Res<Universe>,
    sim: Res<Sim>,
    mut camera: ResMut<SpaceCamera>,
) {
    for (i, key) in BODY_KEYS.iter().enumerate() {
        if keys.just_pressed(*key) && i < universe.0.bodies.len() {
            *camera = view_of_body(&universe.0, i, sim.0.universe_time().as_secs_f64());
            info!("camera: viewing {}", universe.0.bodies[i].name);
        }
    }
}

fn update_anchor(universe: Res<Universe>, sim: Res<Sim>, mut camera: ResMut<SpaceCamera>) {
    let t = sim.0.universe_time().as_secs_f64();
    let (anchor, offset) = anchor_for(&universe.0, camera.anchor, camera.offset, t);
    if anchor != camera.anchor {
        info!("camera: now near {}", universe.0.bodies[anchor].name);
        camera.anchor = anchor;
        camera.offset = offset;
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn halcyon() -> StarSystem {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../content/data/star_systems/halcyon.json");
        StarSystem::from_path(path).unwrap()
    }

    fn index(sys: &StarSystem, id: &str) -> usize {
        sys.bodies.iter().position(|b| b.id == id).unwrap()
    }

    #[test]
    fn look_rotation_points_forward() {
        let q = look_rotation(DVec3::new(1.0, 0.0, 0.0), DVec3::Y);
        assert!((q * DVec3::NEG_Z - DVec3::X).length() < 1e-12);
        assert!((q * DVec3::Y - DVec3::Y).length() < 1e-12);
    }

    #[test]
    fn body_view_looks_at_the_body_from_its_lit_side() {
        let sys = halcyon();
        let m = index(&sys, "meridian");
        let cam = view_of_body(&sys, m, 1_000.0);
        assert_eq!(cam.anchor, m);
        let forward = cam.rotation * DVec3::NEG_Z;
        assert!(forward.dot(-cam.offset.normalize()) > 0.999_999);
        let to_star = sys
            .frames
            .state_in(
                sys.bodies[m].inertial_frame,
                sys.star().inertial_frame,
                1_000.0,
            )
            .position;
        assert!(cam.offset.dot(to_star) > 0.0, "camera on the day side");
    }

    #[test]
    fn anchor_prefers_the_smallest_sphere_of_influence() {
        let sys = halcyon();
        let (m, l) = (index(&sys, "meridian"), index(&sys, "lantern"));
        let t = 500.0;
        // A point 1,000 km from Lantern's center is inside Lantern's sphere (and Meridian's).
        let lantern_in_meridian = sys
            .frames
            .state_in(
                sys.bodies[m].inertial_frame,
                sys.bodies[l].inertial_frame,
                t,
            )
            .position;
        let point = lantern_in_meridian + DVec3::new(1_000_000.0, 0.0, 0.0);
        let (anchor, local) = anchor_for(&sys, m, point, t);
        assert_eq!(anchor, l);
        assert!((local - DVec3::new(1_000_000.0, 0.0, 0.0)).length() < 1e-3);
        // Far outside every planet: the star.
        let (anchor, _) = anchor_for(&sys, m, DVec3::new(5.0e9, 0.0, 0.0), t);
        assert_eq!(anchor, 0);
    }

    #[test]
    fn switching_anchor_keeps_the_same_position() {
        let sys = halcyon();
        let m = index(&sys, "meridian");
        let t = 12_345.0;
        let point = DVec3::new(90_000_000.0, 0.0, 0.0); // just outside Meridian's sphere
        let (anchor, local) = anchor_for(&sys, m, point, t);
        assert_eq!(anchor, 0);
        let star_frame = sys.star().inertial_frame;
        let original =
            sys.frames
                .transform_point(star_frame, sys.bodies[m].inertial_frame, t, point);
        assert!(
            (local - original).length() < 1e-3,
            "same place in the star's frame"
        );
    }
}
