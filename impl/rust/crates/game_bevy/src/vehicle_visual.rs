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
use sim_vehicle::{PartKind, Shape, Vehicle, VehiclePart};

use crate::workers::workers;

pub struct VehicleVisualPlugin;

impl Plugin for VehicleVisualPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<VehicleMeshes>()
            .add_systems(Update, finish_vehicle_meshes);
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
        let mesh = shape_mesh(part.def.shape).transformed_by(transform);
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
