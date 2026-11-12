//! The builder (Batch 3, task 3.7; design: vehicle-builder.md §5).
//!
//! B next to a landed vehicle (on foot, or in it) edits it in place: the vehicle is held where
//! it stands (`Frozen` in the simulation), the camera orbits it, and edits change its parts at
//! once (its merged meshes rebuild in the background). B again leaves: the vehicle is released
//! and settles on its gear.
//!
//! Keys while building: Up/Down choose a part, Left/Right choose a free attach node (yellow),
//! R rolls the part 90° about the joint, T picks which of its nodes joins, Enter places it;
//! PageUp/PageDown select a placed part (red), Delete removes it with everything attached below
//! it; Ctrl+Z / Ctrl+Y undo and redo. Right mouse orbits the camera, the wheel zooms.

use std::sync::Arc;

use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll};
use bevy::prelude::*;
use glam::{DQuat, DVec3};
use sim_core::objects::Object;
use sim_core::{ObjectId, ObjectState, Simulation};
use sim_vehicle::builder::{
    FreeNode, attach_at_node, attach_to_surface, fitting_nodes, free_nodes, placement_at_node,
    remove_part,
};
use sim_vehicle::{FlightTelemetry, Frozen, PartDef, Vehicle, VehicleBody, VehiclePart};

use crate::camera::{CameraBlend, CameraStage, SpaceCamera};
use crate::ship::{PilotState, PlayerShip};
use crate::sim::{RenderTime, Sim, SimSet};
use crate::universe::{Catalog, InGame, Universe};
use crate::vehicle_visual::{VehicleVisual, merged_meshes, shape_mesh};
use crate::walk::{OnFoot, walker_status};

/// A vehicle can be edited from this close on foot (m from its centre).
const REACH: f64 = 15.0;
/// ...if it stands still on the ground (m/s relative to it).
const STILL: f64 = 1.0;
/// Orbit camera: mouse sensitivity (rad/px) and zoom per wheel notch.
const ORBIT_SENSITIVITY: f64 = 0.004;
const ZOOM_STEP: f64 = 0.9;

pub struct BuilderPlugin;

impl Plugin for BuilderPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_panel).add_systems(
            Update,
            (
                (enter_or_leave, edit).chain().before(SimSet),
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
    palette: Vec<Arc<PartDef>>,
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

impl Building {
    fn free(&self) -> Vec<FreeNode> {
        free_nodes(&self.vehicle)
    }

    fn part(&self) -> &Arc<PartDef> {
        &self.palette[self.choice % self.palette.len()]
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

    // Which vehicle: the one we're in, or the nearest one within reach on foot.
    let system = &universe.0;
    let t = sim.universe_time().as_secs_f64();
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
    let Some(target) = target else {
        info!("no vehicle within {REACH:.0} m to build on");
        return;
    };
    let Some((region, state, telemetry, vehicle)) = vehicle_status(sim, target) else {
        return;
    };
    let speed = telemetry.relative_velocity.length();
    if !telemetry.in_contact || speed > STILL {
        info!("land first: the builder needs the vehicle standing still on the ground");
        return;
    }
    let _ = (region, state);
    if let Some(loc) = sim.objects.location(target)
        && let Some(r) = sim.regions.get_mut(loc.region)
    {
        r.world.entity_mut(loc.entity).insert(Frozen);
    }
    let mut palette: Vec<Arc<PartDef>> = catalog.0.iter().cloned().collect();
    palette.sort_by(|a, b| {
        a.kind
            .cmp_key()
            .cmp(&b.kind.cmp_key())
            .then(a.id.cmp(&b.id))
    });
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
        let name = b.part().name.clone();
        b.changed(name);
    }
    if pressed(KeyCode::ArrowUp) {
        b.choice = (b.choice + count - 1) % count;
        b.joint = 0;
        let name = b.part().name.clone();
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
                let def = b.part().clone();
                let roll = b.roll_angle();
                match own {
                    Some(own) => attach_at_node(&mut b.vehicle, &def, &own, &at, roll),
                    None => attach_to_surface(&mut b.vehicle, &def, at.part, position, rotation),
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
        match remove_part(&mut b.vehicle, i) {
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
            let part = VehiclePart {
                def: b.part().clone(),
                position,
                rotation,
                ..Default::default()
            };
            for (_, mesh) in merged_meshes(&[part]) {
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
    let mut s = format!(
        "BUILD  {}  ({} parts, {:.1} t)\n\n",
        b.vehicle.name,
        b.vehicle.parts.len(),
        VehicleBody::of(&b.vehicle).map_or(0.0, |body| body.mass.mass / 1000.0)
    );
    let n = b.palette.len();
    for k in 0..7 {
        let i = (b.choice + n + k - 3) % n;
        let mark = if k == 3 { "> " } else { "  " };
        s += &format!("{mark}{}\n", b.palette[i].name);
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
    s += "\n\nUp/Down part | Left/Right node | R roll | T joint | Enter place\n\
          PgUp/PgDn select | Del remove | Ctrl+Z/Y undo/redo | RMB orbit | wheel zoom | B leave";
    text.0 = s;
}
