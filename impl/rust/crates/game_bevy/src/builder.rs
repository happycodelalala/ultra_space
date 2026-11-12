//! The builder (Batch 3, task 3.7; design: vehicle-builder.md §5).
//!
//! B next to a landed vehicle (on foot, or in it) edits it in place: the vehicle is held where
//! it stands (`Frozen` in the simulation), the camera orbits it, and edits change its parts at
//! once (its merged meshes rebuild in the background). B again leaves: the vehicle is released
//! and settles on its gear.
//!
//! Keys while building: Up/Down choose a part (or a saved section), Left/Right choose a free
//! attach node (yellow), R rolls the part 90° about the joint, T picks which of its nodes joins,
//! M cycles symmetry (mirror, radial ×2–×8), Enter places it (with its symmetric copies);
//! PageUp/PageDown select a placed part (red), Delete removes it with its symmetry group and
//! everything attached below, S saves the selected part and what hangs below it as a section;
//! Ctrl+Z / Ctrl+Y undo and redo. Left click picks a node marker or selects a part; right mouse
//! orbits the camera, the wheel zooms (task 3.8).
//!
//! On foot, B with no vehicle in reach (or Shift+B) starts a new vehicle: one cockpit a few
//! metres ahead; Home swaps that lone root for the chosen part, e.g. a rover chassis (task 3.9).

use std::sync::Arc;

use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll};
use bevy::prelude::*;
use glam::{DQuat, DVec3};
use sim_core::objects::Object;
use sim_core::{ObjectId, ObjectState, Simulation};
use sim_vehicle::Attach;
use sim_vehicle::builder::{
    FreeNode, Section, Symmetry, attach_at_node, attach_to_surface, fitting_nodes, free_nodes,
    place_section, place_symmetric, placement_at_node, remove_with_symmetry, stats,
};
use sim_vehicle::{
    Cruise, FlightControl, FlightTelemetry, Frozen, PartDef, Vehicle, VehicleBody, VehiclePart,
};

use crate::camera::{CameraBlend, CameraStage, SpaceCamera};
use crate::ship::{PilotState, PlayerShip};
use crate::sim::{RenderTime, Sim, SimSet};
use crate::universe::{Catalog, InGame, Universe};
use crate::vehicle_visual::{VehicleVisual, merged_meshes, shape_mesh};
use crate::walk::{OnFoot, spot_ahead, walker_status};

/// A vehicle can be edited from this close on foot (m from its centre).
const REACH: f64 = 15.0;
/// ...if it stands still on the ground (m/s relative to it).
const STILL: f64 = 1.0;
/// A new vehicle starts as this part, this far ahead of the walker (m).
const NEW_ROOT: &str = "cockpit.small";
const NEW_DISTANCE: f64 = 8.0;
/// Orbit camera: mouse sensitivity (rad/px) and zoom per wheel notch.
const ORBIT_SENSITIVITY: f64 = 0.004;
const ZOOM_STEP: f64 = 0.9;

pub struct BuilderPlugin;

impl Plugin for BuilderPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_panel).add_systems(
            Update,
            (
                (enter_or_leave, pick, edit).chain().before(SimSet),
                builder_camera.in_set(CameraStage::Follow),
                (show_markers, update_panel).after(crate::camera::CameraSet),
            )
                .in_set(InGame),
        );
    }
}

/// Build mode: the vehicle being edited and the editor's state.
#[derive(Resource)]
pub struct Building {
    pub target: ObjectId,
    vehicle: Vehicle,
    undo: Vec<Vehicle>,
    redo: Vec<Vehicle>,
    palette: Vec<Entry>,
    symmetry: Symmetry,
    /// Surface gravity of the body the vehicle stands on (m/s²), for thrust-to-weight.
    gravity: f64,
    /// Chosen palette part, free node, joint node of the part, roll (quarter turns).
    choice: usize,
    node: usize,
    joint: usize,
    roll: u8,
    selected: Option<usize>,
    /// Orbit camera (yaw, pitch) and distance.
    orbit: (f64, f64),
    distance: f64,
    message: String,
    /// Bumped on every change (markers and the panel rebuild).
    generation: u64,
}

/// A palette entry: a catalog part, or a section saved in this session.
#[derive(Clone)]
enum Entry {
    Part(Arc<PartDef>),
    Section(Section),
}

impl Entry {
    fn root(&self) -> &Arc<PartDef> {
        match self {
            Entry::Part(def) => def,
            Entry::Section(section) => section.root(),
        }
    }

    fn name(&self) -> String {
        match self {
            Entry::Part(def) => def.name.clone(),
            Entry::Section(section) => format!("[section] {}", section.name),
        }
    }
}

impl Building {
    fn free(&self) -> Vec<FreeNode> {
        free_nodes(&self.vehicle)
    }

    fn entry(&self) -> &Entry {
        &self.palette[self.choice % self.palette.len()]
    }

    fn part(&self) -> &Arc<PartDef> {
        self.entry().root()
    }

    fn target_node(&self) -> Option<FreeNode> {
        let free = self.free();
        (!free.is_empty()).then(|| free[self.node % free.len()].clone())
    }

    fn roll_angle(&self) -> f64 {
        f64::from(self.roll) * std::f64::consts::FRAC_PI_2
    }

    /// Where the chosen part would go: at the chosen node, through one of its own nodes if it
    /// has a fitting one, else on the surface just outside the node.
    fn preview(&self) -> Option<(DVec3, DQuat, Option<String>)> {
        let at = self.target_node()?;
        let def = self.part();
        let fitting = fitting_nodes(def, &at);
        if !fitting.is_empty() {
            let own = fitting[self.joint % fitting.len()];
            let (p, r) = placement_at_node(def, own, &at, self.roll_angle())?;
            return Some((p, r, Some(own.to_string())));
        }
        if !def.can_surface_attach() {
            return None;
        }
        let rotation = DQuat::from_axis_angle(at.direction, self.roll_angle());
        let reach = half_extent(def, rotation.inverse() * at.direction);
        Some((at.position + at.direction * reach, rotation, None))
    }

    fn changed(&mut self, message: impl Into<String>) {
        self.message = message.into();
        self.generation += 1;
    }
}

/// How far a part's shape reaches from its origin along `dir` (part axes).
fn half_extent(def: &PartDef, dir: DVec3) -> f64 {
    let d = dir.normalize_or_zero().abs();
    match def.shape {
        sim_vehicle::Shape::Box { size_m } => (DVec3::from(size_m) / 2.0).dot(d),
        sim_vehicle::Shape::Cylinder { radius_m, length_m } => {
            radius_m * (1.0 - d.y * d.y).sqrt() + length_m / 2.0 * d.y
        }
        sim_vehicle::Shape::Sphere { radius_m } => radius_m,
    }
}

/// The vehicle's region, state, telemetry, parts and frozen flag.
fn vehicle_status(
    sim: &Simulation,
    id: ObjectId,
) -> Option<(usize, ObjectState, FlightTelemetry, Vehicle)> {
    let loc = sim.objects.location(id)?;
    let world = &sim.regions.get(loc.region)?.world;
    Some((
        loc.region,
        *world.get::<ObjectState>(loc.entity)?,
        world.get::<FlightTelemetry>(loc.entity)?.clone(),
        world.get::<Vehicle>(loc.entity)?.clone(),
    ))
}

/// Writes the edited parts back to the simulation (and recompiles the vehicle).
fn write_back(sim: &mut Simulation, id: ObjectId, vehicle: &Vehicle) {
    let Some(loc) = sim.objects.location(id) else {
        return;
    };
    let Some(body) = VehicleBody::of(vehicle) else {
        return;
    };
    if let Some(region) = sim.regions.get_mut(loc.region) {
        let mut e = region.world.entity_mut(loc.entity);
        e.insert((vehicle.clone(), body));
    }
}

#[allow(clippy::too_many_arguments)]
fn enter_or_leave(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    universe: Res<Universe>,
    catalog: Res<Catalog>,
    ship: Option<Res<PlayerShip>>,
    on_foot: Option<Res<OnFoot>>,
    building: Option<Res<Building>>,
    mut pilot: ResMut<PilotState>,
    mut blend: ResMut<CameraBlend>,
    camera: Res<SpaceCamera>,
    real: Res<Time<Real>>,
    mut sim: ResMut<Sim>,
    mut made: Local<u32>,
) {
    if !keys.just_pressed(KeyCode::KeyB) || !pilot.flying {
        return;
    }
    let sim = &mut sim.0;
    let now = real.elapsed_secs_f64();
    if let Some(building) = building {
        // Leave: release the vehicle; it settles on its gear.
        if let Some(loc) = sim.objects.location(building.target)
            && let Some(region) = sim.regions.get_mut(loc.region)
        {
            region.world.entity_mut(loc.entity).remove::<Frozen>();
        }
        blend.start(&camera, now);
        commands.remove_resource::<Building>();
        pilot.building = false;
        info!(
            "left the builder: {} has {} parts",
            building.vehicle.name,
            building.vehicle.parts.len()
        );
        return;
    }

    // Which vehicle: the one we're in, or the nearest one within reach on foot (Shift+B on foot,
    // or B with none in reach, starts a new one).
    let system = &universe.0;
    let t = sim.universe_time().as_secs_f64();
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    let target = if let Some(on_foot) = &on_foot {
        let Some((region, state, _)) = walker_status(sim, on_foot.id) else {
            return;
        };
        let frame = system.bodies[region].inertial_frame;
        let here = state.in_frame(&system.frames, frame, t).state.position;
        sim.regions.get_mut(region).and_then(|r| {
            let mut query = r.world.query::<(&Object, &ObjectState, &Vehicle)>();
            query
                .iter(&r.world)
                .map(|(o, s, _)| {
                    let p = s.in_frame(&system.frames, frame, t).state.position;
                    (o.id, (p - here).length())
                })
                .filter(|(_, d)| *d <= REACH)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(id, _)| id)
        })
    } else {
        ship.map(|s| s.id)
    };
    let target = match (target, &on_foot) {
        (Some(id), _) if !shift => id,
        (_, Some(on_foot)) => {
            // A new vehicle: one part, a few metres ahead, held in place.
            let Some(root) = catalog.0.get(NEW_ROOT).cloned() else {
                return;
            };
            *made += 1;
            let vehicle = Vehicle {
                design_id: "custom".into(),
                name: format!("New vehicle {}", *made),
                parts: vec![VehiclePart {
                    name: "root".into(),
                    def: root,
                    ..Default::default()
                }],
            };
            let Some(body) = VehicleBody::of(&vehicle) else {
                return;
            };
            let Some((region, state)) = spot_ahead(sim, system, on_foot.id, NEW_DISTANCE, 1.5)
            else {
                return;
            };
            let Ok(id) = sim.objects.spawn(
                &mut sim.regions,
                region,
                state,
                (
                    vehicle,
                    body,
                    FlightControl::default(),
                    FlightTelemetry {
                        in_contact: true,
                        ..Default::default()
                    },
                    Cruise::default(),
                    Frozen,
                ),
            ) else {
                return;
            };
            info!("builder: started a new vehicle {NEW_DISTANCE:.0} m ahead");
            id
        }
        _ => {
            info!("no vehicle within {REACH:.0} m to build on");
            return;
        }
    };
    let Some((region, state, telemetry, vehicle)) = vehicle_status(sim, target) else {
        return;
    };
    let speed = telemetry.relative_velocity.length();
    let frozen = sim
        .objects
        .location(target)
        .and_then(|loc| sim.regions.get(loc.region)?.world.get::<Frozen>(loc.entity))
        .is_some();
    if !frozen && (!telemetry.in_contact || speed > STILL) {
        info!("land first: the builder needs the vehicle standing still on the ground");
        return;
    }
    let _ = state;
    if let Some(loc) = sim.objects.location(target)
        && let Some(r) = sim.regions.get_mut(loc.region)
    {
        r.world.entity_mut(loc.entity).insert(Frozen);
    }
    let mut parts: Vec<Arc<PartDef>> = catalog.0.iter().cloned().collect();
    parts.sort_by(|a, b| {
        a.kind
            .cmp_key()
            .cmp(&b.kind.cmp_key())
            .then(a.id.cmp(&b.id))
    });
    let palette = parts.into_iter().map(Entry::Part).collect();
    let body = &system.bodies[region];
    let gravity = body.mu / (body.radius * body.radius);
    let size = VehicleBody::of(&vehicle).map_or(10.0, |b| {
        b.colliders
            .iter()
            .map(|c| c.position.length())
            .fold(5.0, f64::max)
    });
    info!(
        "building on the {} ({} parts)",
        vehicle.name,
        vehicle.parts.len()
    );
    commands.insert_resource(Building {
        target,
        vehicle,
        undo: Vec::new(),
        redo: Vec::new(),
        palette,
        symmetry: Symmetry::None,
        gravity,
        choice: 0,
        node: 0,
        joint: 0,
        roll: 0,
        selected: None,
        orbit: (0.7, -0.45),
        distance: size * 3.0,
        message: "B leaves the builder".into(),
        generation: 1,
    });
    blend.start(&camera, now);
    pilot.building = true;
}

fn symmetry_name(s: Symmetry) -> String {
    match s {
        Symmetry::None => "off".into(),
        Symmetry::Mirror => "mirror".into(),
        Symmetry::Radial(n) => format!("radial x{n}"),
    }
}

/// Sorting key for part kinds in the palette.
trait KindKey {
    fn cmp_key(&self) -> u8;
}

impl KindKey for sim_vehicle::PartKind {
    fn cmp_key(&self) -> u8 {
        *self as u8
    }
}

#[allow(clippy::too_many_arguments)]
fn edit(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    building: Option<ResMut<Building>>,
    mut sim: ResMut<Sim>,
) {
    let Some(mut b) = building else { return };
    let b = &mut *b;

    // Camera.
    if buttons.pressed(MouseButton::Right) {
        let d = motion.delta.as_dvec2() * ORBIT_SENSITIVITY;
        b.orbit.0 -= d.x;
        b.orbit.1 = (b.orbit.1 - d.y).clamp(-1.4, 1.4);
    }
    if scroll.delta.y != 0.0 {
        b.distance = (b.distance * ZOOM_STEP.powf(f64::from(scroll.delta.y))).clamp(3.0, 300.0);
    }

    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    let count = b.palette.len();
    let pressed = |k: KeyCode| keys.just_pressed(k);
    if pressed(KeyCode::ArrowDown) {
        b.choice = (b.choice + 1) % count;
        b.joint = 0;
        let name = b.entry().name();
        b.changed(name);
    }
    if pressed(KeyCode::ArrowUp) {
        b.choice = (b.choice + count - 1) % count;
        b.joint = 0;
        let name = b.entry().name();
        b.changed(name);
    }
    let free = b.free().len().max(1);
    if pressed(KeyCode::ArrowRight) {
        b.node = (b.node + 1) % free;
        b.changed("");
    }
    if pressed(KeyCode::ArrowLeft) {
        b.node = (b.node + free - 1) % free;
        b.changed("");
    }
    if pressed(KeyCode::KeyR) {
        b.roll = (b.roll + 1) % 4;
        b.changed(format!("rolled {}°", u32::from(b.roll) * 90));
    }
    if pressed(KeyCode::KeyT) {
        b.joint += 1;
        b.changed("next joint node");
    }
    if pressed(KeyCode::Home) {
        match b.entry().clone() {
            Entry::Part(def) if b.vehicle.parts.len() == 1 => {
                let before = b.vehicle.clone();
                b.vehicle.parts[0].def = def.clone();
                b.undo.push(before);
                b.redo.clear();
                b.node = 0;
                info!("builder: the root is now a {}", def.name);
                b.changed(format!("root: {}", def.name));
                write_back(&mut sim.0, b.target, &b.vehicle);
            }
            Entry::Part(_) => b.changed("only a lone root part can be swapped"),
            Entry::Section(_) => b.changed("a section can't be the root"),
        }
    }
    if pressed(KeyCode::KeyM) {
        b.symmetry = b.symmetry.next();
        let msg = format!("symmetry: {}", symmetry_name(b.symmetry));
        b.changed(msg);
    }
    if pressed(KeyCode::KeyS)
        && let Some(i) = b.selected
    {
        let n = b
            .palette
            .iter()
            .filter(|e| matches!(e, Entry::Section(_)))
            .count()
            + 1;
        let name = format!("{n} ({})", b.vehicle.parts[i].def.name);
        if let Some(section) = Section::of(&b.vehicle, i, name.clone()) {
            info!(
                "builder: saved section {name} ({} parts)",
                section.parts.len()
            );
            b.palette.push(Entry::Section(section));
            b.choice = b.palette.len() - 1;
            b.changed(format!("saved section {name}"));
        }
    }
    let parts = b.vehicle.parts.len();
    if pressed(KeyCode::PageDown) {
        b.selected = Some(b.selected.map_or(1, |s| s + 1) % parts);
        b.changed("");
    }
    if pressed(KeyCode::PageUp) {
        b.selected = Some(b.selected.map_or(parts - 1, |s| (s + parts - 1) % parts));
        b.changed("");
    }

    let mut edited = false;
    if pressed(KeyCode::Enter) || pressed(KeyCode::NumpadEnter) {
        let before = b.vehicle.clone();
        let result = match (b.target_node(), b.preview()) {
            (Some(at), Some((position, rotation, own))) => {
                let entry = b.entry().clone();
                let roll = b.roll_angle();
                let symmetry = b.symmetry;
                match entry {
                    Entry::Part(def) => {
                        let placed = match own {
                            Some(own) => attach_at_node(&mut b.vehicle, &def, &own, &at, roll),
                            None => {
                                attach_to_surface(&mut b.vehicle, &def, at.part, position, rotation)
                            }
                        };
                        placed.inspect(|&i| {
                            place_symmetric(&mut b.vehicle, i, symmetry);
                        })
                    }
                    Entry::Section(section) => {
                        let attach = match own {
                            Some(own) => Attach::Node {
                                node: own,
                                to: at.node.clone(),
                            },
                            None => Attach::Surface,
                        };
                        place_section(
                            &mut b.vehicle,
                            &section,
                            at.part,
                            attach,
                            position,
                            rotation,
                        )
                    }
                }
            }
            (None, _) => Err("no free attach node".into()),
            (_, None) => Err(format!("{} doesn't fit there", b.part().name)),
        };
        match result {
            Ok(i) => {
                b.undo.push(before);
                b.redo.clear();
                let msg = format!("placed {}", b.vehicle.parts[i].def.name);
                info!("builder: {msg} ({} parts)", b.vehicle.parts.len());
                b.changed(msg);
                edited = true;
            }
            Err(e) => {
                info!("builder: can't place: {e}");
                b.changed(e)
            }
        }
    }
    if (pressed(KeyCode::Delete) || pressed(KeyCode::Backspace))
        && let Some(i) = b.selected
    {
        let before = b.vehicle.clone();
        match remove_with_symmetry(&mut b.vehicle, i) {
            Ok(n) => {
                b.undo.push(before);
                b.redo.clear();
                b.selected = None;
                let msg = format!("removed {n} part{}", if n == 1 { "" } else { "s" });
                info!("builder: {msg} ({} parts)", b.vehicle.parts.len());
                b.changed(msg);
                edited = true;
            }
            Err(e) => b.changed(e),
        }
    }
    if ctrl
        && pressed(KeyCode::KeyZ)
        && let Some(previous) = b.undo.pop()
    {
        {
            let current = std::mem::replace(&mut b.vehicle, previous);
            b.redo.push(current);
            b.selected = None;
            info!("builder: undone ({} parts)", b.vehicle.parts.len());
            b.changed("undone");
            edited = true;
        }
    }
    if ctrl
        && pressed(KeyCode::KeyY)
        && let Some(next) = b.redo.pop()
    {
        {
            let current = std::mem::replace(&mut b.vehicle, next);
            b.undo.push(current);
            b.selected = None;
            info!("builder: redone ({} parts)", b.vehicle.parts.len());
            b.changed("redone");
            edited = true;
        }
    }
    if edited {
        write_back(&mut sim.0, b.target, &b.vehicle);
    }
}

/// Where a ray (vehicle axes) first meets a box at `center`, turned `rotation`, with half sizes
/// `half`: the distance along the ray.
fn ray_box(origin: DVec3, dir: DVec3, center: DVec3, rotation: DQuat, half: DVec3) -> Option<f64> {
    let inv = rotation.inverse();
    let o = inv * (origin - center);
    let d = inv * dir;
    let (mut near, mut far) = (f64::MIN, f64::MAX);
    for i in 0..3 {
        if d[i].abs() < 1e-12 {
            if o[i].abs() > half[i] {
                return None;
            }
            continue;
        }
        let a = (-half[i] - o[i]) / d[i];
        let b = (half[i] - o[i]) / d[i];
        near = near.max(a.min(b));
        far = far.min(a.max(b));
    }
    (near <= far && far >= 0.0).then_some(near.max(0.0))
}

/// Half sizes of the box around a part's shape (part axes).
fn shape_half(shape: sim_vehicle::Shape) -> DVec3 {
    match shape {
        sim_vehicle::Shape::Box { size_m } => DVec3::from(size_m) / 2.0,
        sim_vehicle::Shape::Cylinder { radius_m, length_m } => {
            DVec3::new(radius_m, length_m / 2.0, radius_m)
        }
        sim_vehicle::Shape::Sphere { radius_m } => DVec3::splat(radius_m),
    }
}

/// Left click: a node marker under the cursor becomes the chosen node; else the part under it
/// is selected (or nothing).
fn pick(
    buttons: Res<ButtonInput<MouseButton>>,
    building: Option<ResMut<Building>>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), With<crate::space_render::MainCamera>>,
    visuals: Query<(&VehicleVisual, &GlobalTransform)>,
) {
    let Some(mut b) = building else { return };
    if !buttons.just_pressed(MouseButton::Left) {
        return;
    }
    let Some(cursor) = windows.single().ok().and_then(|w| w.cursor_position()) else {
        return;
    };
    let Ok((camera, camera_transform)) = cameras.single() else {
        return;
    };
    let Ok(ray) = camera.viewport_to_world(camera_transform, cursor) else {
        return;
    };
    let Some((_, vehicle_transform)) = visuals.iter().find(|(v, _)| v.id == b.target) else {
        return;
    };
    // The ray in vehicle axes.
    let to_vehicle = vehicle_transform.affine().inverse();
    let origin = to_vehicle.transform_point3(ray.origin).as_dvec3();
    let dir = to_vehicle
        .transform_vector3(*ray.direction)
        .as_dvec3()
        .normalize();

    let free = b.free();
    let node_hit = free
        .iter()
        .enumerate()
        .filter_map(|(i, n)| {
            ray_box(origin, dir, n.position, DQuat::IDENTITY, DVec3::splat(0.4)).map(|t| (i, t))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1));
    if let Some((i, _)) = node_hit {
        b.node = i;
        let msg = format!(
            "node {} of {}",
            free[i].node, b.vehicle.parts[free[i].part].name
        );
        b.changed(msg);
        return;
    }
    let part_hit = b
        .vehicle
        .parts
        .iter()
        .enumerate()
        .filter_map(|(i, p)| {
            ray_box(origin, dir, p.position, p.rotation, shape_half(p.def.shape)).map(|t| (i, t))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1));
    match part_hit {
        Some((i, _)) => {
            b.selected = Some(i);
            let msg = format!("selected {}", b.vehicle.parts[i].name);
            b.changed(msg);
        }
        None if b.selected.is_some() => {
            b.selected = None;
            b.changed("");
        }
        None => {}
    }
}

/// Orbits the camera around the vehicle being built.
fn builder_camera(
    building: Option<Res<Building>>,
    universe: Res<Universe>,
    render: Res<RenderTime>,
    visuals: Query<&VehicleVisual>,
    mut camera: ResMut<SpaceCamera>,
) {
    let Some(b) = building else { return };
    let system = &universe.0;
    let Some(state) = visuals
        .iter()
        .find(|v| v.id == b.target)
        .and_then(|v| v.shown(system, render.0))
    else {
        return;
    };
    let Some(region) = system
        .bodies
        .iter()
        .position(|body| body.inertial_frame == state.frame)
    else {
        return;
    };
    let s = state.state;
    let center = s.position + s.rotation * vehicle_center(&b.vehicle);
    let orbit = s.rotation * DQuat::from_rotation_y(b.orbit.0) * DQuat::from_rotation_x(b.orbit.1);
    camera.anchor = region;
    camera.offset = center + orbit * DVec3::new(0.0, 0.0, b.distance);
    camera.rotation = orbit.normalize();
}

/// Middle of the vehicle's parts (vehicle axes).
fn vehicle_center(vehicle: &Vehicle) -> DVec3 {
    let n = vehicle.parts.len().max(1) as f64;
    vehicle.parts.iter().map(|p| p.position).sum::<DVec3>() / n
}

/// Markers drawn on the vehicle being built.
#[derive(Component)]
struct BuildMarker;

#[derive(Default)]
struct MarkerAssets {
    ball: Handle<Mesh>,
    free: Handle<StandardMaterial>,
    target: Handle<StandardMaterial>,
    ghost: Handle<StandardMaterial>,
    selected: Handle<StandardMaterial>,
}

/// Rebuilds the markers when the build changes: free nodes (cyan, the chosen one yellow), the
/// part about to be placed (green ghost), the selected part (red).
#[allow(clippy::too_many_arguments)]
fn show_markers(
    mut commands: Commands,
    building: Option<Res<Building>>,
    visuals: Query<(Entity, &VehicleVisual)>,
    markers: Query<Entity, With<BuildMarker>>,
    mut shown: Local<u64>,
    mut assets: Local<Option<MarkerAssets>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let generation = building.as_ref().map_or(0, |b| b.generation);
    if generation == *shown {
        return;
    }
    *shown = generation;
    for m in &markers {
        commands.entity(m).despawn();
    }
    let Some(b) = building else { return };
    let Some((visual, _)) = visuals.iter().find(|(_, v)| v.id == b.target) else {
        *shown = 0; // try again once the visual exists
        return;
    };
    let a = assets.get_or_insert_with(|| {
        let glow = |r: f32, g: f32, bl: f32, alpha: f32| StandardMaterial {
            base_color: Color::srgba(r, g, bl, alpha),
            emissive: LinearRgba::rgb(r * 2.0, g * 2.0, bl * 2.0),
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            ..default()
        };
        MarkerAssets {
            ball: meshes.add(Sphere::new(0.25).mesh().ico(2).expect("valid")),
            free: materials.add(glow(0.2, 0.9, 1.0, 0.7)),
            target: materials.add(glow(1.0, 0.85, 0.1, 0.9)),
            ghost: materials.add(glow(0.2, 1.0, 0.3, 0.35)),
            selected: materials.add(glow(1.0, 0.2, 0.2, 0.45)),
        }
    });
    let free = b.free();
    let target = (!free.is_empty()).then(|| b.node % free.len());
    commands.entity(visual).with_children(|v| {
        for (i, n) in free.iter().enumerate() {
            let material = if Some(i) == target {
                &a.target
            } else {
                &a.free
            };
            v.spawn((
                BuildMarker,
                Mesh3d(a.ball.clone()),
                MeshMaterial3d(material.clone()),
                Transform::from_translation(n.position.as_vec3()),
            ));
        }
        if let Some((position, rotation, _)) = b.preview() {
            let parts: Vec<VehiclePart> = match b.entry() {
                Entry::Part(def) => vec![VehiclePart {
                    def: def.clone(),
                    position,
                    rotation,
                    ..Default::default()
                }],
                Entry::Section(section) => section
                    .parts
                    .iter()
                    .map(|p| VehiclePart {
                        position: position + rotation * p.position,
                        rotation: rotation * p.rotation,
                        ..p.clone()
                    })
                    .collect(),
            };
            for (_, mesh) in merged_meshes(&parts) {
                v.spawn((
                    BuildMarker,
                    Mesh3d(meshes.add(mesh)),
                    MeshMaterial3d(a.ghost.clone()),
                    Transform::default(),
                ));
            }
        }
        if let Some(i) = b.selected
            && let Some(p) = b.vehicle.parts.get(i)
        {
            v.spawn((
                BuildMarker,
                Mesh3d(meshes.add(shape_mesh(p.def.shape))),
                MeshMaterial3d(a.selected.clone()),
                Transform::from_translation(p.position.as_vec3())
                    .with_rotation(p.rotation.as_quat())
                    .with_scale(Vec3::splat(1.05)),
            ));
        }
    });
}

#[derive(Component)]
struct BuildPanel;

fn spawn_panel(mut commands: Commands) {
    commands.spawn((
        BuildPanel,
        Text::new(""),
        TextFont {
            font_size: FontSize::Px(15.0),
            ..default()
        },
        TextColor(Color::srgb(0.85, 0.95, 1.0)),
        BackgroundColor(Color::srgba(0.0, 0.02, 0.05, 0.6)),
        Node {
            position_type: PositionType::Absolute,
            top: px(8),
            left: px(8),
            padding: UiRect::axes(px(12), px(8)),
            ..default()
        },
        Visibility::Hidden,
    ));
}

/// The builder's panel: the palette around the chosen part, what's selected, and the keys.
fn update_panel(
    building: Option<Res<Building>>,
    mut panel: Query<(&mut Text, &mut Visibility), With<BuildPanel>>,
    mut shown: Local<u64>,
) {
    let Ok((mut text, mut visibility)) = panel.single_mut() else {
        return;
    };
    let Some(b) = building else {
        *visibility = Visibility::Hidden;
        *shown = 0;
        return;
    };
    if b.generation == *shown {
        return;
    }
    *shown = b.generation;
    *visibility = Visibility::Inherited;
    let st = stats(&b.vehicle, b.gravity);
    let mut s = format!(
        "BUILD  {}  ({} parts, {:.1} t, {:.0} x {:.0} x {:.0} m)\n",
        b.vehicle.name,
        st.parts,
        st.mass_kg / 1000.0,
        st.size.x,
        st.size.y,
        st.size.z,
    );
    s += &format!(
        "thrust/weight {:.2} | up {:.1} m/s2 ({}) | side {:.1} m/s2 | turn {:.2} rad/s2\n",
        st.thrust_to_weight,
        st.up_acceleration,
        if st.can_hover {
            "can hover"
        } else {
            "can't hover"
        },
        st.side_acceleration,
        st.turn_acceleration,
    );
    s += &format!(
        "fuel {:.0} kg | battery {:.0} kWh",
        st.fuel_kg,
        st.battery_wh / 1000.0
    );
    if st.wheels > 0 {
        s += &format!(" | {} wheels", st.wheels);
        if let Some(t) = st.tipping {
            s += if t > 1.0 {
                " (tips easily!)"
            } else {
                " (stable)"
            };
        }
    }
    for problem in &st.problems {
        s += &format!("\n! {problem}");
    }
    s += &format!("\nsymmetry: {}\n\n", symmetry_name(b.symmetry));
    let n = b.palette.len();
    for k in 0..7 {
        let i = (b.choice + n + k - 3) % n;
        let mark = if k == 3 { "> " } else { "  " };
        s += &format!("{mark}{}\n", b.palette[i].name());
    }
    let free = b.free();
    if let Some(at) = b.target_node() {
        let owner = &b.vehicle.parts[at.part];
        s += &format!(
            "\nnode {}/{}: {} of {} ({:?})",
            b.node % free.len() + 1,
            free.len(),
            at.node,
            owner.name,
            at.size
        );
        if let Some((_, _, own)) = b.preview() {
            s += &match own {
                Some(own) => format!(", joins by its {own}"),
                None => ", on the surface".into(),
            };
        } else {
            s += ", doesn't fit";
        }
    } else {
        s += "\nno free nodes";
    }
    if let Some(i) = b.selected
        && let Some(p) = b.vehicle.parts.get(i)
    {
        s += &format!("\nselected: {} ({})", p.name, p.def.name);
    }
    if !b.message.is_empty() {
        s += &format!("\n\n{}", b.message);
    }
    s += "\n\nUp/Down part | Left/Right or click node | R roll | T joint | M symmetry | Enter place\n\
          PgUp/PgDn or click select | Del remove | S save section | Home make root | Ctrl+Z/Y undo/redo\n\
          RMB orbit | wheel zoom | B leave";
    text.0 = s;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ray_box_hits_what_it_points_at() {
        let half = DVec3::new(1.0, 0.5, 2.0);
        // Straight at a box 10 m ahead: hits its near face.
        let t = ray_box(
            DVec3::ZERO,
            DVec3::NEG_Z,
            DVec3::new(0.0, 0.0, -10.0),
            DQuat::IDENTITY,
            half,
        );
        assert!((t.unwrap() - 8.0).abs() < 1e-9);
        // Beside it: misses.
        assert!(
            ray_box(
                DVec3::new(3.0, 0.0, 0.0),
                DVec3::NEG_Z,
                DVec3::new(0.0, 0.0, -10.0),
                DQuat::IDENTITY,
                half
            )
            .is_none()
        );
        // Behind the ray: misses.
        assert!(
            ray_box(
                DVec3::ZERO,
                DVec3::Z,
                DVec3::new(0.0, 0.0, -10.0),
                DQuat::IDENTITY,
                half
            )
            .is_none()
        );
        // A turned box: 90° about Y swaps its X and Z sizes.
        let turned = DQuat::from_rotation_y(std::f64::consts::FRAC_PI_2);
        let t = ray_box(
            DVec3::ZERO,
            DVec3::NEG_Z,
            DVec3::new(0.0, 0.0, -10.0),
            turned,
            half,
        );
        assert!((t.unwrap() - 9.0).abs() < 1e-9);
    }
}
