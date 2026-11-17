//! Cotton clouds on a body of plains (planet-geography.md §3.7).
//!
//! The clouds come from `sim_planet::clouds` (placed by the weather and climate); here they are
//! built into one mesh per plain on the worker threads (each puff a low sphere, flat at the
//! cloud's base, with its height in the cloud in the UVs for shading), drawn with a soft cotton
//! shader and placed each frame relative to the camera, drifting slowly about the planet's axis.

use std::sync::Arc;

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, Mesh, PrimitiveTopology};
use bevy::pbr::{Material, MaterialPlugin};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;
use bevy::tasks::{Task, block_on, poll_once};
use glam::{DQuat, DVec3};

use crate::camera::{CameraSet, SpaceCamera};
use crate::sim::RenderTime;
use crate::space_render::STAR_LIGHT;
use crate::universe::{InGame, Universe};

const CLOUD_SHADER: &str = "embedded://ultra_space/shaders/cloud.wesl";
/// Share of the surface under cloud (planet-geography.md decisions).
const COVER: f64 = 1.0 / 3.0;
/// Drift of the clouds about the planet's axis (rad/s of game time).
const DRIFT: f64 = 2.0e-5;

#[derive(Clone, Copy, Debug, Default, ShaderType)]
pub struct CloudUniform {
    /// xyz = star position relative to the camera, w = starlight brightness.
    pub star: Vec4,
    /// xyz = body centre relative to the camera.
    pub center: Vec4,
    /// rgb = sky light (linear).
    pub sky: Vec4,
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct CloudMaterial {
    #[uniform(0)]
    pub data: CloudUniform,
}

impl Material for CloudMaterial {
    fn enable_shadows() -> bool {
        false
    }

    fn fragment_shader() -> ShaderRef {
        CLOUD_SHADER.into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Premultiplied
    }
}

pub struct CloudsPlugin;

impl Plugin for CloudsPlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "shaders/cloud.wesl");
        app.add_plugins(MaterialPlugin::<CloudMaterial>::default())
            .add_systems(
                Update,
                (start_clouds, finish_clouds, place_clouds.after(CameraSet)).in_set(InGame),
            );
    }
}

/// One plain's cloud mesh, in the plain's frame: the body, the mesh's origin (surface frame).
#[derive(Component)]
struct CloudChunk {
    body: usize,
    origin: DVec3,
}

/// One body's cloud meshes: (origin, mesh) per plain.
type CloudMeshes = Vec<(DVec3, Mesh)>;

/// Cloud meshes being built: (body, task) per body of plains.
#[derive(Resource, Default)]
struct CloudJobs {
    started: bool,
    tasks: Vec<(usize, Task<CloudMeshes>)>,
    material: Option<Handle<CloudMaterial>>,
}

fn start_clouds(mut commands: Commands, universe: Res<Universe>, jobs: Option<ResMut<CloudJobs>>) {
    if jobs.is_some_and(|j| j.started) {
        return;
    }
    let mut tasks = Vec::new();
    for (i, body) in universe.0.bodies.iter().enumerate() {
        if body.terrain.as_ref().and_then(|t| t.plains).is_none() {
            continue;
        }
        let Some(terrain) = sim_planet::Terrain::of(body) else {
            continue;
        };
        let seed = body.terrain.as_ref().map_or(0, |t| t.seed);
        let terrain = Arc::new(terrain);
        tasks.push((
            i,
            crate::workers::workers().spawn(async move { build(&terrain, seed) }),
        ));
    }
    commands.insert_resource(CloudJobs {
        started: true,
        tasks,
        material: None,
    });
}

/// Every cloud of a body, as one mesh per plain (origin at the plain's centre).
fn build(terrain: &sim_planet::Terrain, seed: u64) -> CloudMeshes {
    let Some(world) = terrain.world() else {
        return Vec::new();
    };
    let clouds = sim_planet::clouds::clouds(world, seed, COVER);
    let sphere = Sphere::new(1.0).mesh().ico(1).expect("icosphere");
    let unit: Vec<[f32; 3]> = match sphere.attribute(Mesh::ATTRIBUTE_POSITION) {
        Some(bevy::mesh::VertexAttributeValues::Float32x3(v)) => v.clone(),
        _ => Vec::new(),
    };
    let tris: Vec<u32> = match sphere.indices() {
        Some(Indices::U32(i)) => i.clone(),
        Some(Indices::U16(i)) => i.iter().map(|&x| u32::from(x)).collect(),
        None => Vec::new(),
    };
    let mut out = Vec::new();
    for (k, plain) in world.plains.plains.iter().enumerate() {
        let origin = plain.origin();
        let (e, n, u) = (plain.east, plain.north, plain.normal);
        let mut positions: Vec<[f32; 3]> = Vec::new();
        let mut normals: Vec<[f32; 3]> = Vec::new();
        let mut uvs: Vec<[f32; 2]> = Vec::new();
        let mut indices: Vec<u32> = Vec::new();
        for c in clouds.iter().filter(|c| c.plain == k) {
            let top = c
                .puffs
                .iter()
                .map(|p| p.offset.z + p.radius.z)
                .fold(1.0, f64::max);
            for puff in &c.puffs {
                let base = positions.len() as u32;
                for v in &unit {
                    let v = DVec3::new(f64::from(v[0]), f64::from(v[1]), f64::from(v[2]));
                    // Flat at the cloud's base: nothing below it.
                    let z = (puff.offset.z + v.z * puff.radius.z).max(0.0);
                    let local = DVec3::new(
                        puff.offset.x + v.x * puff.radius.x,
                        puff.offset.y + v.y * puff.radius.y,
                        z,
                    );
                    let world_p = c.base + e * local.x + n * local.y + u * local.z;
                    positions.push((world_p - origin).as_vec3().to_array());
                    let normal = (e * v.x / puff.radius.x
                        + n * v.y / puff.radius.y
                        + u * v.z / puff.radius.z)
                        .normalize();
                    normals.push(normal.as_vec3().to_array());
                    uvs.push([(z / top) as f32, 0.0]);
                }
                indices.extend(tris.iter().map(|t| t + base));
            }
        }
        if indices.is_empty() {
            continue;
        }
        let mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::RENDER_WORLD,
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_indices(Indices::U32(indices));
        out.push((origin, mesh));
    }
    out
}

fn finish_clouds(
    mut commands: Commands,
    jobs: Option<ResMut<CloudJobs>>,
    universe: Res<Universe>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<CloudMaterial>>,
) {
    let Some(mut jobs) = jobs else { return };
    let mut i = 0;
    while i < jobs.tasks.len() {
        let Some(chunks) = block_on(poll_once(&mut jobs.tasks[i].1)) else {
            i += 1;
            continue;
        };
        let (body, _) = jobs.tasks.swap_remove(i);
        let sky = universe.0.bodies[body]
            .atmosphere
            .as_ref()
            .map_or(Vec4::ZERO, |a| {
                Vec4::new(a.color[0] as f32, a.color[1] as f32, a.color[2] as f32, 1.0) * 0.5
            });
        let material = jobs
            .material
            .get_or_insert_with(|| {
                materials.add(CloudMaterial {
                    data: CloudUniform { sky, ..default() },
                })
            })
            .clone();
        info!("clouds: {} plain meshes for body {body}", chunks.len());
        for (origin, mesh) in chunks {
            commands.spawn((
                CloudChunk { body, origin },
                Mesh3d(meshes.add(mesh)),
                MeshMaterial3d(material.clone()),
                bevy::light::NotShadowCaster,
                Transform::default(),
                Name::new("clouds"),
            ));
        }
    }
}

fn place_clouds(
    universe: Res<Universe>,
    render: Res<RenderTime>,
    camera: Res<SpaceCamera>,
    jobs: Option<Res<CloudJobs>>,
    mut materials: ResMut<Assets<CloudMaterial>>,
    mut chunks: Query<(&CloudChunk, &mut Transform)>,
) {
    let system = &universe.0;
    let anchor = system.bodies[camera.anchor].inertial_frame;
    let star = (system.frames.transform_point(
        anchor,
        system.star().inertial_frame,
        render.0,
        DVec3::ZERO,
    ) - camera.offset)
        .as_vec3();
    let mut centre = None;
    for (chunk, mut transform) in &mut chunks {
        let s = system
            .frames
            .state_in(anchor, system.bodies[chunk.body].surface_frame, render.0);
        // Drifting slowly about the planet's axis.
        let drift = DQuat::from_rotation_y(render.0 * DRIFT);
        let rotation = s.rotation * drift;
        let position = s.position - camera.offset;
        centre.get_or_insert(position);
        transform.translation = (position + rotation * chunk.origin).as_vec3();
        transform.rotation = rotation.as_quat().normalize();
    }
    if let (Some(jobs), Some(c)) = (jobs, centre)
        && let Some(handle) = &jobs.material
        && let Some(mut m) = materials.get_mut(handle)
    {
        m.data.star = star.extend(STAR_LIGHT);
        m.data.center = c.as_vec3().extend(0.0);
    }
}
