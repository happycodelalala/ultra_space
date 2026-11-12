//! The player's ship: spawning, controls, camera and visuals (Batch 1, task 1.11).
//!
//! Controls (design: objects-vehicles-physics.md §10): W/S forward/back, A/D strafe,
//! Space/Ctrl up/down, mouse pitch/yaw (cursor locked), Q/E roll, T cycles flight assist,
//! X brakes (assist), Tab switches between flying and the free debug camera, V toggles the
//! cockpit view, right mouse orbits the chase camera.

use bevy::camera::Exposure;
use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
use glam::{DQuat, DVec3};
use sim_core::{ObjectId, ObjectState, Simulation};
use sim_vehicle::{
    AssistMode, FlightControl, FlightTelemetry, PartCatalog, PartKind, Shape, Vehicle, VehicleBody,
    VehicleDesign,
};

use crate::camera::{CameraSet, CameraStage, SpaceCamera, look_rotation};
use crate::sim::{Sim, SimSet};
use crate::space_render::{MainCamera, STAR_LIGHT};
use crate::universe::{Universe, content_dir};

const START_BODY: &str = "meridian";
const START_ALTITUDE: f64 = 150_000.0;
const START_DESIGN: &str = "kestrel";

/// Mouse movement (pixels per frame) giving full rotation stick.
const MOUSE_FULL_STICK: f32 = 25.0;
/// Chase camera offset behind and above the ship (vehicle axes).
const CHASE_OFFSET: DVec3 = DVec3::new(0.0, 5.0, 24.0);
const COCKPIT_EYE: DVec3 = DVec3::new(0.0, 0.5, -5.2);

pub struct ShipPlugin;

impl Plugin for ShipPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PilotState>()
            .add_systems(Startup, (spawn_player_ship, spawn_starlight))
            .add_systems(
                Update,
                (
                    pilot_mode_keys,
                    pilot_input.before(SimSet),
                    follow_ship.in_set(CameraStage::Follow),
                    (place_ship_visual, update_plumes, aim_starlight).after(CameraSet),
                    cursor_lock,
                ),
            );
    }
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
}

impl Default for PilotState {
    fn default() -> Self {
        Self {
            flying: true,
            view: ShipView::Chase,
            orbit: (0.0, 0.0),
        }
    }
}

fn spawn_player_ship(
    mut commands: Commands,
    universe: Res<Universe>,
    mut sim: ResMut<Sim>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let content = content_dir().expect("content folder");
    let catalog =
        PartCatalog::load_dir(content.join("data/parts")).unwrap_or_else(|e| panic!("{e}"));
    let design = VehicleDesign::from_path(
        content.join(format!("data/vehicles/{START_DESIGN}.json")),
        &catalog,
    )
    .unwrap_or_else(|e| panic!("{e}"));
    let vehicle = Vehicle::from_design(&design);
    let body = VehicleBody::of(&vehicle).expect("design has parts");
    info!(
        "spawning {}: {:.1} t, forward {:.1} m/s², {} thrusters",
        design.name,
        body.mass.mass / 1_000.0,
        body.performance.forward_acceleration(),
        body.allocator.thrusters().len()
    );

    // Circular, prograde, equatorial orbit over the day side, 30° short of the subsolar point
    // so the terminator is in view; nose along the velocity, belly toward the planet.
    let system = &universe.0;
    let index = system
        .bodies
        .iter()
        .position(|b| b.id == START_BODY)
        .unwrap_or(0);
    let planet = &system.bodies[index];
    let r = planet.radius + START_ALTITUDE;
    let speed = (planet.mu / r).sqrt();
    let to_star = system.frames.transform_point(
        planet.inertial_frame,
        system.star().inertial_frame,
        0.0,
        DVec3::ZERO,
    );
    let sunward = DVec3::new(to_star.x, 0.0, to_star.z).normalize_or(DVec3::X);
    let up = DQuat::from_rotation_y(-30f64.to_radians()) * sunward;
    // Orbits with angular momentum along +Y move along Y × r.
    let prograde = DVec3::Y.cross(up);
    let mut state =
        ObjectState::at_rest(planet.inertial_frame, up * r, look_rotation(prograde, up));
    state.state.velocity = prograde * speed;

    spawn_ship_visual(&mut commands, &vehicle, &mut meshes, &mut materials);
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
            ),
        )
        .expect("start region exists");
    commands.insert_resource(PlayerShip { id });
}

// ---------------------------------------------------------------------------------------------
// Input
// ---------------------------------------------------------------------------------------------

fn pilot_mode_keys(
    keys: Res<ButtonInput<KeyCode>>,
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
    if keys.just_pressed(KeyCode::KeyV) {
        pilot.view = match pilot.view {
            ShipView::Chase => ShipView::Cockpit,
            ShipView::Cockpit => ShipView::Chase,
        };
    }
}

fn axis(keys: &ButtonInput<KeyCode>, positive: KeyCode, negative: KeyCode) -> f64 {
    f64::from(keys.pressed(positive)) - f64::from(keys.pressed(negative))
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

    if !pilot.flying {
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
    let orbiting = buttons.pressed(MouseButton::Right);
    let mouse = if orbiting {
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
    mut windows: Query<&mut CursorOptions, With<PrimaryWindow>>,
) {
    if !pilot.is_changed() {
        return;
    }
    for mut cursor in &mut windows {
        if pilot.flying {
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
    sim: Res<Sim>,
    universe: Res<Universe>,
    mut camera: ResMut<SpaceCamera>,
) {
    if !pilot.flying {
        return;
    }
    let Some(ship) = ship else { return };
    let Some((region, state, _, _)) = player_ship_state(&sim.0, &ship) else {
        return;
    };
    // Flight states live in the region body's inertial frame.
    let t = sim.0.universe_time().as_secs_f64();
    let state = state
        .in_frame(
            &universe.0.frames,
            universe.0.bodies[region].inertial_frame,
            t,
        )
        .state;
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
    camera.offset = state.position + offset;
    camera.rotation = rotation;
}

// ---------------------------------------------------------------------------------------------
// Visuals
// ---------------------------------------------------------------------------------------------

#[derive(Component)]
struct ShipVisual;

/// Exhaust plume for thruster `index` (in allocator order), scaled by its throttle.
#[derive(Component)]
struct Plume {
    index: usize,
    /// Full-throttle length (m).
    length: f32,
    /// Where the nozzle is along the part's Y axis.
    nozzle_y: f32,
}

fn shape_mesh(shape: Shape) -> Mesh {
    match shape {
        Shape::Box { size_m: [x, y, z] } => Cuboid::new(x as f32, y as f32, z as f32).into(),
        Shape::Cylinder { radius_m, length_m } => {
            Cylinder::new(radius_m as f32, length_m as f32).into()
        }
        Shape::Sphere { radius_m } => Sphere::new(radius_m as f32).mesh().ico(3).expect("valid"),
    }
}

fn spawn_ship_visual(
    commands: &mut Commands,
    vehicle: &Vehicle,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    let hull = materials.add(StandardMaterial {
        base_color: Color::srgb(0.62, 0.64, 0.68),
        metallic: 0.6,
        perceptual_roughness: 0.45,
        ..default()
    });
    let glass = materials.add(StandardMaterial {
        base_color: Color::srgb(0.12, 0.2, 0.3),
        metallic: 0.2,
        perceptual_roughness: 0.15,
        ..default()
    });
    let dark = materials.add(StandardMaterial {
        base_color: Color::srgb(0.18, 0.18, 0.2),
        metallic: 0.8,
        perceptual_roughness: 0.5,
        ..default()
    });
    let flame = materials.add(StandardMaterial {
        base_color: Color::BLACK,
        emissive: LinearRgba::rgb(9.0, 5.0, 2.0),
        unlit: true,
        ..default()
    });
    let plume_mesh = meshes.add(Cone::new(1.0, 1.0));

    let mut thruster_index = 0;
    commands
        .spawn((
            Name::new(vehicle.name.clone()),
            ShipVisual,
            Transform::default(),
            Visibility::default(),
        ))
        .with_children(|ship| {
            for part in &vehicle.parts {
                let material = match part.def.kind {
                    PartKind::Cockpit => glass.clone(),
                    PartKind::Thruster | PartKind::ReactionWheel | PartKind::CruiseDrive => {
                        dark.clone()
                    }
                    PartKind::Hull | PartKind::Tank => hull.clone(),
                };
                let transform = Transform::from_translation(part.position.as_vec3())
                    .with_rotation(part.rotation.as_quat());
                let mut part_entity = ship.spawn((
                    Mesh3d(meshes.add(shape_mesh(part.def.shape))),
                    MeshMaterial3d(material),
                    transform,
                ));
                if part.def.thruster.is_some() {
                    // Exhaust leaves along the part's -Y (it pushes the vehicle along +Y).
                    let (radius, length) = match part.def.shape {
                        Shape::Cylinder { radius_m, length_m } => {
                            (radius_m as f32 * 0.8, length_m as f32)
                        }
                        Shape::Box { size_m: [x, _, z] } => (x.min(z) as f32 * 0.35, 0.4),
                        Shape::Sphere { radius_m } => (radius_m as f32 * 0.6, radius_m as f32),
                    };
                    let plume_length = radius * 8.0;
                    let nozzle_y = -length / 2.0;
                    part_entity.with_children(|p| {
                        p.spawn((
                            Plume {
                                index: thruster_index,
                                length: plume_length,
                                nozzle_y,
                            },
                            Mesh3d(plume_mesh.clone()),
                            MeshMaterial3d(flame.clone()),
                            Transform::from_xyz(0.0, nozzle_y, 0.0)
                                .with_rotation(Quat::from_rotation_x(std::f32::consts::PI))
                                .with_scale(Vec3::new(radius, 0.0, radius)),
                            Visibility::Hidden,
                        ));
                    });
                    thruster_index += 1;
                }
            }
        });
}

fn place_ship_visual(
    ship: Option<Res<PlayerShip>>,
    sim: Res<Sim>,
    universe: Res<Universe>,
    camera: Res<SpaceCamera>,
    mut visuals: Query<&mut Transform, With<ShipVisual>>,
) {
    let Some(ship) = ship else { return };
    let Some((_, state, _, _)) = player_ship_state(&sim.0, &ship) else {
        return;
    };
    let system = &universe.0;
    let t = sim.0.universe_time().as_secs_f64();
    let anchor = system.bodies[camera.anchor].inertial_frame;
    let s = state.in_frame(&system.frames, anchor, t).state;
    for mut transform in &mut visuals {
        transform.translation = (s.position - camera.offset).as_vec3();
        transform.rotation = s.rotation.as_quat().normalize();
    }
}

fn update_plumes(
    ship: Option<Res<PlayerShip>>,
    sim: Res<Sim>,
    mut plumes: Query<(&Plume, &mut Transform, &mut Visibility)>,
) {
    let Some(ship) = ship else { return };
    let Some((_, _, telemetry, _)) = player_ship_state(&sim.0, &ship) else {
        return;
    };
    for (plume, mut transform, mut visibility) in &mut plumes {
        let throttle = telemetry.throttles.get(plume.index).copied().unwrap_or(0.0) as f32;
        if throttle < 0.02 {
            *visibility = Visibility::Hidden;
            continue;
        }
        *visibility = Visibility::Inherited;
        let length = plume.length * throttle.sqrt();
        // The cone is centered on its origin with its apex flipped to point away from the
        // nozzle: put its base at the nozzle and let it extend outward along -Y.
        transform.scale.y = length;
        transform.translation.y = plume.nozzle_y - length / 2.0;
    }
}

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
    sim: Res<Sim>,
    camera: Res<SpaceCamera>,
    mut lights: Query<&mut Transform, (With<Starlight>, Without<MainCamera>)>,
) {
    let system = &universe.0;
    let t = sim.0.universe_time().as_secs_f64();
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
