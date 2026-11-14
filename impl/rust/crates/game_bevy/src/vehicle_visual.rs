//! Drawing vehicles from their compiled form (task 3.4; design: vehicle-builder.md §4,
//! ships-stations-interiors §6–7).
//!
//! A vehicle is drawn as **one merged mesh per material**, built from its parts on the worker
//! pool and cached by design hash, so a ship of hundreds of parts costs a few draws and identical
//! designs share their meshes. Until a design's meshes are ready the vehicle shows nothing new
//! (an existing visual keeps its old meshes). Thruster plumes stay separate entities (effects).

use std::collections::HashMap;

use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy::shader::ShaderRef;
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
        bevy::asset::embedded_asset!(app, "shaders/hull_detail.wesl");
        app.add_plugins(MaterialPlugin::<PartSurface>::default())
            .init_resource::<VehicleMeshes>()
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

/// A part surface: the standard material with surface detail drawn at real sizes (plate
/// seams, fasteners, wear; `shaders/hull_detail.wesl`) where the palette slot has `detail`.
pub type PartSurface = ExtendedMaterial<StandardMaterial, HullDetail>;

const HULL_DETAIL_SHADER: &str = "embedded://ultra_space/shaders/hull_detail.wesl";

/// The surface detail of a palette slot (`detail` in `content/data/part_materials.json`).
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone, Default)]
pub struct HullDetail {
    /// x plate height (m), y plate width / height, z structural seam every this many plates,
    /// w strength (0: none).
    #[uniform(100)]
    pub plate: Vec4,
    /// x seam width (m), y fastener spacing (m), z grime strength.
    #[uniform(100)]
    pub seam: Vec4,
}

impl HullDetail {
    fn from_json(v: Option<&serde_json::Value>) -> Self {
        let Some(v) = v else {
            return Self::default();
        };
        let f = |k: &str, d: f64| v.get(k).and_then(|x| x.as_f64()).unwrap_or(d) as f32;
        Self {
            plate: Vec4::new(
                f("plate_m", 3.0),
                f("aspect", 2.0),
                f("structural_every", 4.0),
                f("strength", 1.0),
            ),
            seam: Vec4::new(
                f("seam_m", 0.05),
                f("fastener_m", 0.5),
                f("grime", 1.0),
                0.0,
            ),
        }
    }
}

impl MaterialExtension for HullDetail {
    fn fragment_shader() -> ShaderRef {
        HULL_DETAIL_SHADER.into()
    }
}

/// The materials parts are drawn with: the slots of `content/data/part_materials.json`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PartMaterial {
    /// Painted armour (slot `armor`).
    Hull,
    /// Second paint: frames, inner plates.
    Armor2,
    /// Bare machined steel.
    Metal,
    /// Polished rods and pins.
    Chrome,
    Dark,
    /// Hoses, seals, treads.
    Rubber,
    /// Accent and hazard paint.
    Trim,
    Glass,
    /// Lamps (emissive).
    Light,
    /// Red warning beacons on a giant frame's extremities (emissive).
    Beacon,
}

impl PartMaterial {
    pub const ALL: [PartMaterial; 10] = [
        PartMaterial::Hull,
        PartMaterial::Armor2,
        PartMaterial::Metal,
        PartMaterial::Chrome,
        PartMaterial::Dark,
        PartMaterial::Rubber,
        PartMaterial::Trim,
        PartMaterial::Glass,
        PartMaterial::Light,
        PartMaterial::Beacon,
    ];

    /// Its name in the palette and in part models.
    pub fn slot(self) -> &'static str {
        match self {
            PartMaterial::Hull => "armor",
            PartMaterial::Armor2 => "armor2",
            PartMaterial::Metal => "metal",
            PartMaterial::Chrome => "chrome",
            PartMaterial::Dark => "dark",
            PartMaterial::Rubber => "rubber",
            PartMaterial::Trim => "trim",
            PartMaterial::Glass => "glass",
            PartMaterial::Light => "light",
            PartMaterial::Beacon => "beacon",
        }
    }

    /// The material a model primitive's material name picks (armour for unknown names).
    pub fn named(name: Option<&str>) -> PartMaterial {
        match name {
            Some("joint") => PartMaterial::Dark,
            Some(n) => Self::ALL
                .into_iter()
                .find(|m| m.slot() == n)
                .unwrap_or(PartMaterial::Hull),
            None => PartMaterial::Hull,
        }
    }
}

/// A palette entry (`content/data/part_materials.json`).
struct PaletteEntry {
    color: [f32; 3],
    metallic: f32,
    roughness: f32,
    emissive: Option<[f32; 3]>,
    emissive_strength: Option<f32>,
    detail: HullDetail,
}

impl PaletteEntry {
    fn from_json(v: &serde_json::Value) -> Option<Self> {
        let rgb = |v: &serde_json::Value| -> Option<[f32; 3]> {
            let a = v.as_array()?;
            Some([
                a.first()?.as_f64()? as f32,
                a.get(1)?.as_f64()? as f32,
                a.get(2)?.as_f64()? as f32,
            ])
        };
        Some(Self {
            color: rgb(v.get("color")?)?,
            metallic: v.get("metallic")?.as_f64()? as f32,
            roughness: v.get("roughness")?.as_f64()? as f32,
            emissive: v.get("emissive").and_then(rgb),
            emissive_strength: v
                .get("emissive_strength")
                .and_then(|s| s.as_f64())
                .map(|s| s as f32),
            detail: HullDetail::from_json(v.get("detail")),
        })
    }
}

/// The part materials from the palette file, with built-in fallbacks for missing ones.
fn palette_materials() -> HashMap<PartMaterial, PartSurface> {
    let mut entries: HashMap<String, PaletteEntry> = HashMap::new();
    if let Some(dir) = crate::universe::content_dir() {
        let path = dir.join("data/part_materials.json");
        match std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).map_err(|e| e.to_string()))
        {
            Ok(file) => {
                if let Some(map) = file.get("materials").and_then(|m| m.as_object()) {
                    for (name, v) in map {
                        match PaletteEntry::from_json(v) {
                            Some(e) => {
                                entries.insert(name.clone(), e);
                            }
                            None => warn!("part materials: bad entry {name}"),
                        }
                    }
                }
            }
            Err(e) => warn!("part materials {}: {e}", path.display()),
        }
    }
    PartMaterial::ALL
        .into_iter()
        .map(|m| {
            let e = entries.remove(m.slot()).unwrap_or(PaletteEntry {
                color: [0.5, 0.52, 0.55],
                metallic: 0.5,
                roughness: 0.5,
                emissive: None,
                emissive_strength: None,
                detail: HullDetail::default(),
            });
            let [r, g, b] = e.color;
            let emissive = e.emissive.map_or(LinearRgba::BLACK, |[r, g, b]| {
                LinearRgba::from(Color::srgb(r, g, b)) * e.emissive_strength.unwrap_or(1.0)
            });
            (
                m,
                PartSurface {
                    base: StandardMaterial {
                        base_color: Color::srgb(r, g, b),
                        metallic: e.metallic,
                        perceptual_roughness: e.roughness,
                        emissive,
                        ..default()
                    },
                    extension: e.detail,
                },
            )
        })
        .collect()
}

pub fn part_material(kind: PartKind) -> PartMaterial {
    match kind {
        PartKind::Cockpit => PartMaterial::Glass,
        PartKind::Thruster
        | PartKind::ReactionWheel
        | PartKind::CruiseDrive
        | PartKind::LandingLeg
        | PartKind::Wheel
        | PartKind::Joint => PartMaterial::Dark,
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
        // A procedural shell without a model: its collision tiles.
        Shape::Shell { .. } => {
            let mut mesh: Option<Mesh> = None;
            for t in shape.tiles() {
                let tile: Mesh = Cuboid::new(
                    t.half.x as f32 * 2.0,
                    t.half.y as f32 * 2.0,
                    t.half.z as f32 * 2.0,
                )
                .into();
                let tile = tile.transformed_by(
                    Transform::from_translation(t.centre.as_vec3())
                        .with_rotation(t.rotation.as_quat()),
                );
                match &mut mesh {
                    Some(m) => {
                        let _ = m.merge(&tile);
                    }
                    None => mesh = Some(tile),
                }
            }
            mesh.unwrap_or_else(|| Cuboid::new(1.0, 1.0, 1.0).into())
        }
    }
}

/// One mesh per material for `parts`, in the vehicle frame (ignoring joints).
pub fn merged_meshes(parts: &[VehiclePart]) -> Vec<(PartMaterial, Mesh)> {
    merged_group_meshes(parts, &vec![0; parts.len()])
        .into_iter()
        .map(|(_, m, mesh)| (m, mesh))
        .collect()
}

/// One mesh per rigid group and material, in the vehicle's rest frame (`groups`: each part's
/// group). Pure: runs on the worker pool.
pub fn merged_group_meshes(
    parts: &[VehiclePart],
    groups: &[usize],
) -> Vec<(usize, PartMaterial, Mesh)> {
    let mut merged: Vec<(usize, PartMaterial, Mesh)> = Vec::new();
    let models = crate::part_models::part_models();
    for (part, &group) in parts.iter().zip(groups) {
        let transform = Transform::from_translation(part.position.as_vec3())
            .with_rotation(part.rotation.as_quat());
        // The part's model (one mesh per material), else its placeholder shape.
        let pieces: Vec<(PartMaterial, Mesh)> =
            match part.def.form.as_ref().and_then(|f| models.get(&f.model)) {
                Some(model) => model.clone(),
                None => {
                    let mut mesh = shape_mesh(part.def.shape);
                    if part.def.wheel.is_some() {
                        // A tyre turns about its axle (the part's X); the cylinder mesh stands
                        // along Y.
                        mesh = mesh.rotated_by(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2));
                    }
                    // White vertex colours, as the models' baked occlusion: so they merge.
                    let n = mesh.count_vertices();
                    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![[1.0f32; 4]; n]);
                    vec![(part_material(part.def.kind), mesh)]
                }
            };
        for (material, mesh) in pieces {
            let mesh = mesh.transformed_by(transform);
            match merged
                .iter_mut()
                .find(|(g, m, _)| *g == group && *m == material)
            {
                Some((_, _, into)) => {
                    if let Err(e) = into.merge(&mesh) {
                        warn!("can't merge a part mesh: {e}");
                    }
                }
                None => merged.push((group, material, mesh)),
            }
        }
    }
    merged
}

/// Merged meshes of a design: one per rigid group and material.
type GroupMeshes = Vec<(usize, PartMaterial, Mesh)>;

/// Merged meshes by design hash, and designs being built.
#[derive(Resource, Default)]
pub struct VehicleMeshes {
    ready: HashMap<u64, Vec<(usize, PartMaterial, Handle<Mesh>)>>,
    pending: HashMap<u64, Task<GroupMeshes>>,
    materials: HashMap<PartMaterial, Handle<PartSurface>>,
}

impl VehicleMeshes {
    /// Starts building `vehicle`'s meshes unless they exist or are being built.
    pub fn request(&mut self, vehicle: &Vehicle) {
        let hash = vehicle.design_hash();
        if self.ready.contains_key(&hash) || self.pending.contains_key(&hash) {
            return;
        }
        let parts = vehicle.parts.clone();
        let task = workers().spawn(async move {
            let groups = sim_vehicle::joints::Articulation::of(&parts).part_group;
            merged_group_meshes(&parts, &groups)
        });
        self.pending.insert(hash, task);
    }

    fn material(
        &mut self,
        which: PartMaterial,
        materials: &mut Assets<PartSurface>,
    ) -> Handle<PartSurface> {
        if self.materials.is_empty() {
            self.materials = palette_materials()
                .into_iter()
                .map(|(m, material)| (m, materials.add(material)))
                .collect();
        }
        self.materials[&which].clone()
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

    /// Shows the meshes again once they are ready (after the group nodes were rebuilt).
    fn reset(&mut self) {
        self.shown = None;
    }
}

/// The merged-mesh children of a vehicle visual's group nodes.
#[derive(Component)]
struct MergedPart;

/// A rigid group of a vehicle visual (index into its articulation); placed at the group's joint
/// pose, it carries the group's merged meshes and thruster mounts.
#[derive(Component)]
pub struct GroupNode(pub usize);

/// Collects finished mesh builds and gives every vehicle visual its design's meshes, replacing
/// old ones only once the new ones are ready.
fn finish_vehicle_meshes(
    mut commands: Commands,
    mut cache: ResMut<VehicleMeshes>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<PartSurface>>,
    mut visuals: Query<(Entity, &mut VehicleMesh, Option<&Children>)>,
    nodes: Query<(&GroupNode, Option<&Children>)>,
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
                .map(|(group, material, mesh)| (group, material, meshes.add(mesh)))
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
        // The group nodes (spawned with the visual) carry the meshes.
        let mut group_nodes: HashMap<usize, Entity> = HashMap::new();
        for child in children.into_iter().flatten() {
            if let Ok((node, grandchildren)) = nodes.get(*child) {
                group_nodes.insert(node.0, *child);
                for g in grandchildren.into_iter().flatten() {
                    if merged_parts.contains(*g) {
                        commands.entity(*g).despawn();
                    }
                }
            }
        }
        if group_nodes.is_empty() {
            continue; // spawned next frame
        }
        let _ = entity;
        for (group, material, mesh) in handles {
            let Some(&node) = group_nodes.get(&group) else {
                continue;
            };
            let material = cache.material(material, &mut materials);
            commands.entity(node).with_children(|n| {
                n.spawn((
                    MergedPart,
                    Mesh3d(mesh),
                    MeshMaterial3d(material),
                    Transform::default(),
                ));
            });
        }
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

/// The visual's rigid-group nodes (one per joint group, one without joints), each with a mount
/// and an exhaust plume for its thrusters.
fn spawn_plumes(
    commands: &mut Commands,
    visual: Entity,
    id: ObjectId,
    vehicle: &Vehicle,
    plume_assets: &(Handle<Mesh>, Handle<StandardMaterial>),
) {
    let articulation = sim_vehicle::joints::Articulation::of(&vehicle.parts);
    let mut thruster_index = 0;
    commands.entity(visual).with_children(|v| {
        for g in 0..articulation.groups.len() {
            v.spawn((GroupNode(g), Transform::default(), Visibility::default()))
                .with_children(|node| {
                    for (i, part) in vehicle.parts.iter().enumerate() {
                        if part.def.thruster.is_none() {
                            continue;
                        }
                        // Allocator order is part order: count every thruster.
                        let index = vehicle.parts[..i]
                            .iter()
                            .filter(|p| p.def.thruster.is_some())
                            .count();
                        if articulation.part_group[i] != g {
                            continue;
                        }
                        let transform = Transform::from_translation(part.position.as_vec3())
                            .with_rotation(part.rotation.as_quat());
                        // Exhaust leaves along the part's -Y (it pushes the vehicle along +Y).
                        let (radius, length) = match part.def.shape {
                            Shape::Cylinder { radius_m, length_m } => {
                                (radius_m as f32 * 0.8, length_m as f32)
                            }
                            Shape::Box { size_m: [x, _, z] } => (x.min(z) as f32 * 0.35, 0.4),
                            Shape::Sphere { radius_m } => (radius_m as f32 * 0.6, radius_m as f32),
                            Shape::Shell { .. } => (0.5, 0.5),
                        };
                        let nozzle_y = -length / 2.0;
                        node.spawn((PlumeMount, transform, Visibility::default()))
                            .with_children(|p| {
                                p.spawn((
                                    Plume {
                                        vehicle: id,
                                        index,
                                        length: radius * 8.0,
                                        nozzle_y,
                                    },
                                    Mesh3d(plume_assets.0.clone()),
                                    MeshMaterial3d(plume_assets.1.clone()),
                                    bevy::light::NotShadowCaster,
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
    });
    let _ = thruster_index;
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
    group_nodes: Query<(), With<GroupNode>>,
    mut node_transforms: Query<(&GroupNode, &mut Transform), Without<VehicleVisual>>,
) {
    let system = &universe.0;
    let sim = &mut sim.0;
    let t = sim.universe_time().as_secs_f64();
    let plume_assets = plume_assets.get_or_insert_with(|| {
        (
            meshes.add(Cone::new(1.0, 1.0)),
            // Glowing exhaust: unlit, added to what is behind it.
            materials.add(StandardMaterial {
                base_color: Color::linear_rgb(6.0, 3.2, 1.2),
                unlit: true,
                alpha_mode: AlphaMode::Add,
                ..default()
            }),
        )
    });

    // Every vehicle, in its region body's inertial frame, with its design hash (precomputed:
    // nothing here looks at parts, except to build a new vehicle's visual).
    let shown: std::collections::HashSet<ObjectId> = visuals.iter().map(|v| v.1.id).collect();
    let mut vehicles: HashMap<ObjectId, (ObjectState, u64, Option<Vehicle>)> = HashMap::new();
    let mut poses: HashMap<ObjectId, Vec<sim_vehicle::joints::Pose>> = HashMap::new();
    for (index, region) in sim.regions.iter_mut().enumerate() {
        let frame = system.bodies[index].inertial_frame;
        let mut query = region.world.query::<(
            &Object,
            &ObjectState,
            &VehicleBody,
            &Vehicle,
            Option<&sim_vehicle::joints::JointState>,
        )>();
        for (object, state, body, vehicle, joints) in query.iter(&region.world) {
            let new = (!shown.contains(&object.id)).then(|| vehicle.clone());
            vehicles.insert(
                object.id,
                (state.in_frame(&system.frames, frame, t), body.hash, new),
            );
            if let Some(j) = joints {
                poses.insert(object.id, j.poses.clone());
            }
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
                if group_nodes.contains(*child) {
                    commands.entity(*child).despawn();
                }
            }
            mesh.reset();
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
        // Joint poses: each rigid group node at its pose in the vehicle's rest frame.
        if let Some(group_poses) = poses.get(&visual.id) {
            for child in children.into_iter().flatten() {
                if let Ok((node, mut t)) = node_transforms.get_mut(*child)
                    && let Some(p) = group_poses.get(node.0)
                {
                    t.translation = p.translation.as_vec3();
                    t.rotation = p.rotation.as_quat();
                }
            }
        }
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
        // One mesh per material: the placeholders' hull, glass and dark, and the engine
        // model's metal, chrome and dark (one mesh each, however many parts use them).
        let mut materials: Vec<PartMaterial> = merged.iter().map(|(m, _)| *m).collect();
        materials.sort_by_key(|m| m.slot());
        materials.dedup();
        assert_eq!(materials.len(), merged.len(), "one mesh per material");
        assert!(merged.len() >= 4, "{} meshes", merged.len());
        // Every part's vertices are in there (its model's, else its placeholder shape's).
        let models = crate::part_models::part_models();
        let expected: usize = design
            .parts
            .iter()
            .map(
                |p| match p.def.form.as_ref().and_then(|f| models.get(&f.model)) {
                    Some(model) => model.iter().map(|(_, m)| m.count_vertices()).sum(),
                    None => shape_mesh(p.def.shape).count_vertices(),
                },
            )
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
