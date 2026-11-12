//! Drawing vehicles from their compiled form (task 3.4; design: vehicle-builder.md §4,
//! ships-stations-interiors §6–7).
//!
//! A vehicle is drawn as **one merged mesh per material**, built from its parts on the worker
//! pool and cached by design hash, so a ship of hundreds of parts costs a few draws and identical
//! designs share their meshes. Until a design's meshes are ready the vehicle shows nothing new
//! (an existing visual keeps its old meshes). Thruster plumes stay separate entities (effects).

use std::collections::HashMap;

use bevy::prelude::*;
use bevy::tasks::{Task, block_on, poll_once};
use sim_core::objects::Object;
use sim_core::{ObjectId, ObjectState};
use sim_vehicle::{FlightTelemetry, PartKind, Shape, Vehicle, VehicleBody, VehiclePart};

use crate::camera::{CameraSet, SpaceCamera};
use crate::ship::interpolate_state;
use crate::sim::{RenderTime, Sim};
use crate::universe::{InGame, Universe};
use crate::workers::workers;

pub struct VehicleVisualPlugin;

impl Plugin for VehicleVisualPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<VehicleMeshes>()
            .add_systems(Update, finish_vehicle_meshes)
            .add_systems(
                Update,
                (sync_vehicle_visuals, update_plumes)
                    .chain()
                    .after(CameraSet)
                    .in_set(InGame),
            );
    }
}

/// The materials parts are drawn with.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PartMaterial {
    Hull,
    Glass,
    Dark,
}

pub fn part_material(kind: PartKind) -> PartMaterial {
    match kind {
        PartKind::Cockpit => PartMaterial::Glass,
        PartKind::Thruster
        | PartKind::ReactionWheel
        | PartKind::CruiseDrive
        | PartKind::LandingLeg
        | PartKind::Wheel => PartMaterial::Dark,
        PartKind::Hull | PartKind::Structure | PartKind::Tank | PartKind::Battery => {
            PartMaterial::Hull
        }
    }
}

/// Placeholder mesh for a part shape (real models come with the form layer's art).
pub fn shape_mesh(shape: Shape) -> Mesh {
    match shape {
        Shape::Box { size_m: [x, y, z] } => Cuboid::new(x as f32, y as f32, z as f32).into(),
        Shape::Cylinder { radius_m, length_m } => {
            Cylinder::new(radius_m as f32, length_m as f32).into()
        }
        Shape::Sphere { radius_m } => Sphere::new(radius_m as f32).mesh().ico(3).expect("valid"),
    }
}

/// One mesh per material for `parts`, in the vehicle frame. Pure: runs on the worker pool.
pub fn merged_meshes(parts: &[VehiclePart]) -> Vec<(PartMaterial, Mesh)> {
    let mut merged: Vec<(PartMaterial, Mesh)> = Vec::new();
    for part in parts {
        let material = part_material(part.def.kind);
        let transform = Transform::from_translation(part.position.as_vec3())
            .with_rotation(part.rotation.as_quat());
        let mut mesh = shape_mesh(part.def.shape);
        if part.def.wheel.is_some() {
            // A tyre turns about its axle (the part's X); the cylinder mesh stands along Y.
            mesh = mesh.rotated_by(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2));
        }
        let mesh = mesh.transformed_by(transform);
        match merged.iter_mut().find(|(m, _)| *m == material) {
            Some((_, into)) => {
                if let Err(e) = into.merge(&mesh) {
                    warn!("can't merge a part mesh: {e}");
                }
            }
            None => merged.push((material, mesh)),
        }
    }
    merged
}

/// Merged meshes by design hash, and designs being built.
#[derive(Resource, Default)]
pub struct VehicleMeshes {
    ready: HashMap<u64, Vec<(PartMaterial, Handle<Mesh>)>>,
    pending: HashMap<u64, Task<Vec<(PartMaterial, Mesh)>>>,
    materials: Option<[Handle<StandardMaterial>; 3]>,
}

impl VehicleMeshes {
    /// Starts building `vehicle`'s meshes unless they exist or are being built.
    pub fn request(&mut self, vehicle: &Vehicle) {
        let hash = vehicle.design_hash();
        if self.ready.contains_key(&hash) || self.pending.contains_key(&hash) {
            return;
        }
        let parts = vehicle.parts.clone();
        let task = workers().spawn(async move { merged_meshes(&parts) });
        self.pending.insert(hash, task);
    }

    fn material(
        &mut self,
        which: PartMaterial,
        materials: &mut Assets<StandardMaterial>,
    ) -> Handle<StandardMaterial> {
        let [hull, glass, dark] = self.materials.get_or_insert_with(|| {
            [
                materials.add(StandardMaterial {
                    base_color: Color::srgb(0.62, 0.64, 0.68),
                    metallic: 0.6,
                    perceptual_roughness: 0.45,
                    ..default()
                }),
                materials.add(StandardMaterial {
                    base_color: Color::srgb(0.12, 0.2, 0.3),
                    metallic: 0.2,
                    perceptual_roughness: 0.15,
                    ..default()
                }),
                materials.add(StandardMaterial {
                    base_color: Color::srgb(0.18, 0.18, 0.2),
                    metallic: 0.8,
                    perceptual_roughness: 0.5,
                    ..default()
                }),
            ]
        });
        match which {
            PartMaterial::Hull => hull.clone(),
            PartMaterial::Glass => glass.clone(),
            PartMaterial::Dark => dark.clone(),
        }
    }
}

/// A vehicle visual waiting for (or showing) the merged meshes of this design hash.
#[derive(Component)]
pub struct VehicleMesh {
    pub hash: u64,
    shown: Option<u64>,
}

impl VehicleMesh {
    pub fn new(hash: u64) -> Self {
        Self { hash, shown: None }
    }
}

/// The merged-mesh children of a vehicle visual.
#[derive(Component)]
struct MergedPart;

/// Collects finished mesh builds and gives every vehicle visual its design's meshes, replacing
/// old ones only once the new ones are ready.
fn finish_vehicle_meshes(
    mut commands: Commands,
    mut cache: ResMut<VehicleMeshes>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut visuals: Query<(Entity, &mut VehicleMesh, Option<&Children>)>,
    merged_parts: Query<(), With<MergedPart>>,
) {
    let cache = &mut *cache;
    let finished: Vec<u64> = cache
        .pending
        .iter_mut()
        .filter_map(|(hash, task)| block_on(poll_once(task)).map(|built| (*hash, built)))
        .map(|(hash, built)| {
            let handles = built
                .into_iter()
                .map(|(material, mesh)| (material, meshes.add(mesh)))
                .collect();
            cache.ready.insert(hash, handles);
            hash
        })
        .collect();
    for hash in finished {
        cache.pending.remove(&hash);
    }

    for (entity, mut visual, children) in &mut visuals {
        if visual.shown == Some(visual.hash) {
            continue;
        }
        let Some(handles) = cache.ready.get(&visual.hash).cloned() else {
            continue;
        };
        for child in children.into_iter().flatten() {
            if merged_parts.contains(*child) {
                commands.entity(*child).despawn();
            }
        }
        commands.entity(entity).with_children(|v| {
            for (material, mesh) in handles {
                v.spawn((
                    MergedPart,
                    Mesh3d(mesh),
                    MeshMaterial3d(cache.material(material, &mut materials)),
                    Transform::default(),
                ));
            }
        });
        visual.shown = Some(visual.hash);
    }
}

/// A vehicle drawn in the world: one per simulated vehicle, placed at its interpolated state.
#[derive(Component)]
pub struct VehicleVisual {
    pub id: ObjectId,
    previous: Option<(f64, ObjectState)>,
    current: Option<(f64, ObjectState)>,
}

impl VehicleVisual {
    /// The state it is drawn at: interpolated to universe time `t` (render time).
    pub fn shown(&self, system: &sim_core::StarSystem, t: f64) -> Option<ObjectState> {
        match (self.previous, self.current) {
            (Some(p), Some(c)) if p.1.frame == c.1.frame => {
                Some(interpolate_state(system, p, c, t))
            }
            (_, Some(c)) => Some(c.1),
            _ => None,
        }
    }
}

/// A thruster's mount on a vehicle visual (holds its plume).
#[derive(Component)]
struct PlumeMount;

/// Exhaust plume for thruster `index` (allocator order) of vehicle `vehicle`.
#[derive(Component)]
struct Plume {
    vehicle: ObjectId,
    index: usize,
    /// Full-throttle length (m).
    length: f32,
    /// Where the nozzle is along the part's Y axis.
    nozzle_y: f32,
}

/// Spawns a vehicle's visual: its merged meshes (built in the background) and a mount with an
/// exhaust plume per thruster.
fn spawn_vehicle_visual(
    commands: &mut Commands,
    id: ObjectId,
    vehicle: &Vehicle,
    cache: &mut VehicleMeshes,
    plume_assets: &(Handle<Mesh>, Handle<StandardMaterial>),
) {
    cache.request(vehicle);
    let entity = commands
        .spawn((
            Name::new(vehicle.name.clone()),
            VehicleVisual {
                id,
                previous: None,
                current: None,
            },
            VehicleMesh::new(vehicle.design_hash()),
            Transform::default(),
            Visibility::Hidden,
        ))
        .id();
    spawn_plumes(commands, entity, id, vehicle, plume_assets);
}

/// A mount with an exhaust plume for each of the vehicle's thrusters, under its visual.
fn spawn_plumes(
    commands: &mut Commands,
    visual: Entity,
    id: ObjectId,
    vehicle: &Vehicle,
    plume_assets: &(Handle<Mesh>, Handle<StandardMaterial>),
) {
    let mut thruster_index = 0;
    commands.entity(visual).with_children(|v| {
        for part in vehicle.parts.iter().filter(|p| p.def.thruster.is_some()) {
            let transform = Transform::from_translation(part.position.as_vec3())
                .with_rotation(part.rotation.as_quat());
            // Exhaust leaves along the part's -Y (it pushes the vehicle along +Y).
            let (radius, length) = match part.def.shape {
                Shape::Cylinder { radius_m, length_m } => (radius_m as f32 * 0.8, length_m as f32),
                Shape::Box { size_m: [x, _, z] } => (x.min(z) as f32 * 0.35, 0.4),
                Shape::Sphere { radius_m } => (radius_m as f32 * 0.6, radius_m as f32),
            };
            let nozzle_y = -length / 2.0;
            v.spawn((PlumeMount, transform, Visibility::default()))
                .with_children(|p| {
                    p.spawn((
                        Plume {
                            vehicle: id,
                            index: thruster_index,
                            length: radius * 8.0,
                            nozzle_y,
                        },
                        Mesh3d(plume_assets.0.clone()),
                        MeshMaterial3d(plume_assets.1.clone()),
                        Transform::from_xyz(0.0, nozzle_y, 0.0)
                            .with_rotation(Quat::from_rotation_x(std::f32::consts::PI))
                            .with_scale(Vec3::new(radius, 0.0, radius)),
                        Visibility::Hidden,
                    ));
                });
            thruster_index += 1;
        }
    });
}

/// Keeps one visual per simulated vehicle and places each at its state interpolated to the
/// render time, relative to the camera.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn sync_vehicle_visuals(
    mut commands: Commands,
    mut sim: ResMut<Sim>,
    universe: Res<Universe>,
    render: Res<RenderTime>,
    camera: Res<SpaceCamera>,
    mut cache: ResMut<VehicleMeshes>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut plume_assets: Local<Option<(Handle<Mesh>, Handle<StandardMaterial>)>>,
    mut visuals: Query<(
        Entity,
        &mut VehicleVisual,
        &mut VehicleMesh,
        &mut Transform,
        &mut Visibility,
        Option<&Children>,
    )>,
    mounts: Query<(), With<PlumeMount>>,
) {
    let system = &universe.0;
    let sim = &mut sim.0;
    let t = sim.universe_time().as_secs_f64();
    let plume_assets = plume_assets.get_or_insert_with(|| {
        (
            meshes.add(Cone::new(1.0, 1.0)),
            materials.add(StandardMaterial {
                base_color: Color::BLACK,
                emissive: LinearRgba::rgb(9.0, 5.0, 2.0),
                unlit: true,
                ..default()
            }),
        )
    });

    // Every vehicle, in its region body's inertial frame, with its design hash (precomputed:
    // nothing here looks at parts, except to build a new vehicle's visual).
    let shown: std::collections::HashSet<ObjectId> = visuals.iter().map(|v| v.1.id).collect();
    let mut vehicles: HashMap<ObjectId, (ObjectState, u64, Option<Vehicle>)> = HashMap::new();
    for (index, region) in sim.regions.iter_mut().enumerate() {
        let frame = system.bodies[index].inertial_frame;
        let mut query = region
            .world
            .query::<(&Object, &ObjectState, &VehicleBody, &Vehicle)>();
        for (object, state, body, vehicle) in query.iter(&region.world) {
            let new = (!shown.contains(&object.id)).then(|| vehicle.clone());
            vehicles.insert(
                object.id,
                (state.in_frame(&system.frames, frame, t), body.hash, new),
            );
        }
    }

    let anchor = system.bodies[camera.anchor].inertial_frame;
    for (entity, mut visual, mut mesh, mut transform, mut visibility, children) in &mut visuals {
        let Some((state, hash, vehicle)) = vehicles.get_mut(&visual.id) else {
            commands.entity(entity).despawn();
            continue;
        };
        vehicle.take(); // already has a visual
        if mesh.hash != *hash
            && let Some(v) = vehicles_vehicle(&*sim, visual.id)
        {
            // Rebuilt (the builder): new meshes (shown once ready) and new plumes.
            cache.request(&v);
            mesh.hash = *hash;
            for child in children.into_iter().flatten() {
                if mounts.contains(*child) {
                    commands.entity(*child).despawn();
                }
            }
            spawn_plumes(&mut commands, entity, visual.id, &v, plume_assets);
        }
        if visual.current.is_none_or(|(tc, _)| tc != t) {
            visual.previous = visual.current;
            visual.current = Some((t, *state));
        }
        let shown = visual.shown(system, render.0).unwrap_or(*state);
        let s = shown.in_frame(&system.frames, anchor, render.0).state;
        transform.translation = (s.position - camera.offset).as_vec3();
        transform.rotation = s.rotation.as_quat().normalize();
        *visibility = Visibility::Inherited;
    }
    for (id, (_, _, vehicle)) in vehicles {
        if let Some(vehicle) = vehicle {
            spawn_vehicle_visual(&mut commands, id, &vehicle, &mut cache, plume_assets);
        }
    }
}

/// A vehicle's current parts (when its design changed).
fn vehicles_vehicle(sim: &sim_core::Simulation, id: ObjectId) -> Option<Vehicle> {
    let loc = sim.objects.location(id)?;
    sim.regions
        .get(loc.region)?
        .world
        .get::<Vehicle>(loc.entity)
        .cloned()
}

/// Scales each plume by its thruster's throttle.
fn update_plumes(sim: Res<Sim>, mut plumes: Query<(&Plume, &mut Transform, &mut Visibility)>) {
    let sim = &sim.0;
    for (plume, mut transform, mut visibility) in &mut plumes {
        let throttle = sim
            .objects
            .location(plume.vehicle)
            .and_then(|loc| {
                sim.regions
                    .get(loc.region)?
                    .world
                    .get::<FlightTelemetry>(loc.entity)
                    .and_then(|t| t.throttles.get(plume.index).copied())
            })
            .unwrap_or(0.0) as f32;
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use bevy::mesh::VertexAttributeValues;
    use sim_vehicle::{PartCatalog, VehicleDesign};

    use super::*;

    #[test]
    fn kestrel_merges_into_one_mesh_per_material() {
        let content = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../../content/data");
        let catalog = PartCatalog::load_dir(content.join("parts")).unwrap();
        let design =
            VehicleDesign::from_path(content.join("vehicles/kestrel.json"), &catalog).unwrap();
        let merged = merged_meshes(&design.parts);
        assert_eq!(merged.len(), 3);
        // Every part's vertices are in there.
        let expected: usize = design
            .parts
            .iter()
            .map(|p| shape_mesh(p.def.shape).count_vertices())
            .sum();
        let total: usize = merged.iter().map(|(_, m)| m.count_vertices()).sum();
        assert_eq!(total, expected);
        // And placed in the vehicle frame: the engine reaches the stern (z ≈ 6.6 m).
        let far = merged
            .iter()
            .filter_map(|(_, m)| match m.attribute(Mesh::ATTRIBUTE_POSITION) {
                Some(VertexAttributeValues::Float32x3(p)) => {
                    p.iter().map(|v| v[2]).reduce(f32::max)
                }
                _ => None,
            })
            .fold(f32::MIN, f32::max);
        assert!((far - 6.6).abs() < 0.05, "{far}");
    }
}
