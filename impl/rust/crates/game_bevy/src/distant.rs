//! Settlements from afar (task 4b.6; living-planet.md §5): every settlement of the body the
//! camera is at, drawn from its record (no layout needed) and batched per chunk of the planet
//! (cube-sphere tiles about 60 km across), so thousands of settlements are a few dozen meshes.
//!
//! - **Forms:** a cluster of boxes per settlement, as many and as big as its size and tier
//!   (huts for the Stone Age, blocks and towers for industrial cities), in its culture's
//!   colours, on the ground; ruins as low broken forms in grey. Settlements whose own layout is
//!   drawn (near the camera, `settlements.rs`) are left out, and their chunk rebuilt.
//! - **Night glow:** a lit disc per settlement, sized by its extent and bright by its tier
//!   (camp fires dim, industrial cities bright), lit by the sun's height at the chunk and shown
//!   from a few kilometres up, so the settled lands read from orbit at night.
//!
//! Chunks are built on the worker threads when the camera comes within 250 km of them.

use std::collections::{BTreeSet, HashMap};

use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::tasks::Task;
use glam::{DQuat, DVec3};
use sim_planet::{Terrain, TileKey, locate};
use sim_society::{RegionSociety, SizeClass, TechTier};

use crate::camera::{CameraSet, SpaceCamera};
use crate::settlements::{SettlementDraw, SettlementSet, add_box, estimated_extent, vertex_mesh};
use crate::sim::{RenderTime, Sim};
use crate::universe::{InGame, Universe};

/// Chunk tiles: this level of the cube sphere (about 60 km on Meridian: a rebuild, when a
/// settlement's own layout comes or goes, is small).
const LEVEL: u8 = 4;
/// Chunks within this distance of the camera (m) are drawn.
const RANGE: f64 = 250_000.0;
/// The glow shows from this height above the ground (m).
const GLOW_FROM: f64 = 1_500.0;
/// Night glow brightness on screen.
const GLOW: f32 = 2.5;

pub struct DistantPlugin;

impl Plugin for DistantPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Distant>().add_systems(
            Update,
            (stream_chunks, place_chunks)
                .chain()
                .after(SettlementSet)
                .after(CameraSet)
                .in_set(InGame),
        );
    }
}

/// What a chunk is made from: its settlements (site indices) and ruins (indices into the
/// society's ruins), and the sites left out because their layout is drawn.
#[derive(Clone, PartialEq, Default)]
struct Contents {
    sites: Vec<usize>,
    ruins: Vec<usize>,
    left_out: BTreeSet<usize>,
}

struct Chunk {
    body: usize,
    /// Centre on the ground (surface frame, m): the meshes are relative to it.
    origin: DVec3,
    contents: Contents,
    /// What the drawn meshes were made from (`None` until the first are made).
    built: Option<Contents>,
    task: Option<Task<(Mesh, Mesh, Contents)>>,
    root: Option<Entity>,
    glow: Option<(Entity, Handle<StandardMaterial>)>,
}

#[derive(Resource, Default)]
struct Distant {
    chunks: HashMap<(usize, TileKey), Chunk>,
    /// Bodies whose chunks are listed.
    listed: Vec<usize>,
    material: Option<Handle<StandardMaterial>>,
}

/// One settlement or ruin as the worker needs it.
#[derive(Clone)]
struct Item {
    dir: DVec3,
    size: SizeClass,
    tier: Option<TechTier>,
    palette: Vec<[f64; 3]>,
    population: u32,
    seed: u64,
}

fn mix(mut x: u64) -> u64 {
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^ (x >> 31)
}

fn unit(x: u64) -> f64 {
    (x >> 11) as f64 / (1u64 << 53) as f64
}

fn linear(c: [f64; 3]) -> [f32; 3] {
    c.map(|v| (v.clamp(0.0, 1.0) as f32).powf(2.2))
}

/// The forms and glow of a chunk's items, relative to `origin` (surface frame, m).
fn build(items: &[Item], origin: DVec3, terrain: &Terrain) -> (Mesh, Mesh) {
    let r = terrain.radius;
    let (mut p, mut n, mut c, mut idx) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let (mut gp, mut gn, mut gc, mut gi) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for it in items {
        let up = it.dir.normalize();
        let east = DVec3::Y
            .cross(up)
            .try_normalize()
            .unwrap_or(up.any_orthonormal_vector());
        let south = east.cross(up);
        let extent = estimated_extent(it.size) * if it.tier.is_none() { 1.4 } else { 1.0 };
        let ruin = it.tier.is_none();
        // How many boxes, how big, by size and tier.
        let count = match it.size {
            SizeClass::Dwelling => 2,
            SizeClass::Hamlet => 6,
            SizeClass::Village => 14,
            SizeClass::Town => 40,
            SizeClass::City => 120,
            _ => 300,
        } / if ruin { 2 } else { 1 };
        let (foot, tall) = match it.tier {
            None => (14.0, 6.0),
            Some(TechTier::StoneAge) => (5.0, 3.0),
            Some(TechTier::Ancient | TechTier::Medieval) => (9.0, 7.0),
            Some(TechTier::EarlyModern) => (11.0, 10.0),
            Some(TechTier::Industrial) => match it.size {
                SizeClass::Metropolis | SizeClass::City => (22.0, 28.0),
                _ => (14.0, 12.0),
            },
            Some(_) => match it.size {
                SizeClass::Metropolis | SizeClass::City => (25.0, 45.0),
                _ => (14.0, 10.0),
            },
        };
        let roof = linear(*it.palette.first().unwrap_or(&[0.5; 3]));
        let wall = linear(*it.palette.get(2).unwrap_or(&[0.6; 3]));
        let (roof, wall) = if ruin {
            ([0.12, 0.12, 0.11], [0.2, 0.19, 0.17])
        } else {
            (roof, wall)
        };
        for k in 0..count {
            let h = mix(it.seed ^ (k as u64 + 1).wrapping_mul(0x9e37_79b9));
            // Denser towards the middle.
            let d = extent * unit(h).powf(0.75);
            let a = std::f64::consts::TAU * unit(mix(h ^ 1));
            let dir = (up * r + (east * a.cos() + south * a.sin()) * d).normalize();
            let ground = terrain.surface_height(dir, 100.0);
            let at = dir * (r + ground) - origin;
            let scale = 0.6 + 0.8 * unit(mix(h ^ 2));
            let height = tall * scale * if ruin { 0.3 + unit(mix(h ^ 3)) } else { 1.0 };
            // Box in a local frame at its spot: up along the ground's normal.
            let local_up = dir;
            let yaw = std::f64::consts::TAU * unit(mix(h ^ 4));
            let q = DQuat::from_rotation_arc(DVec3::Y, local_up) * DQuat::from_rotation_y(yaw);
            let centre = at + local_up * (height * 0.5 - 2.0);
            let half = DVec3::new(foot * scale * 0.5, height * 0.5 + 2.0, foot * scale * 0.5);
            let base = p.len();
            add_box(
                &mut p,
                &mut n,
                &mut c,
                &mut idx,
                Vec3::ZERO,
                half.as_vec3(),
                0.0,
                roof,
                wall,
            );
            // Turn and move the box's vertices into place.
            for v in base..p.len() {
                let pv = Vec3::from(p[v]).as_dvec3();
                p[v] = (q * pv + centre).as_vec3().to_array();
                n[v] = (q * Vec3::from(n[v]).as_dvec3()).as_vec3().to_array();
            }
        }
        // The built-up area, seen from a height: a muted disc of the place's colours (rubble
        // grey for ruins) just above the ground, under its boxes.
        {
            let ground = terrain.surface_height(up, 100.0);
            let tint = if ruin {
                [0.09, 0.085, 0.08]
            } else {
                [
                    0.55 * roof[0] + 0.45 * wall[0] * 0.6,
                    0.55 * roof[1] + 0.45 * wall[1] * 0.6,
                    0.55 * roof[2] + 0.45 * wall[2] * 0.6,
                ]
            };
            let radius = extent * 0.75;
            let base = p.len() as u32;
            let centre = up * (r + ground + 1.0) - origin;
            p.push(centre.as_vec3().to_array());
            n.push(up.as_vec3().to_array());
            c.push([tint[0], tint[1], tint[2], 1.0]);
            let sides = 12;
            for k in 0..sides {
                let a = std::f64::consts::TAU * k as f64 / sides as f64;
                let rim = (up * r + (east * a.cos() + south * a.sin()) * radius).normalize();
                let v = rim * (r + terrain.surface_height(rim, 100.0) + 1.0) - origin;
                p.push(v.as_vec3().to_array());
                n.push(up.as_vec3().to_array());
                c.push([tint[0], tint[1], tint[2], 1.0]);
            }
            for k in 0..sides as u32 {
                let (a, b) = (base + 1 + k, base + 1 + (k + 1) % sides as u32);
                idx.extend([base, b, a]);
            }
        }
        if !ruin {
            // A glow disc, brighter for denser, later peoples.
            let bright = match it.tier {
                Some(TechTier::StoneAge) => 0.12,
                Some(TechTier::Ancient | TechTier::Medieval) => 0.35,
                Some(TechTier::EarlyModern) => 0.55,
                _ => 1.0,
            } * (f64::from(it.population).log10() / 5.0).clamp(0.3, 1.2);
            let ground = terrain.surface_height(up, 100.0);
            let centre = up * (r + ground + 4.0) - origin;
            let radius = extent * 0.7;
            let base = gp.len() as u32;
            gp.push(centre.as_vec3().to_array());
            gn.push(up.as_vec3().to_array());
            let warm = [1.0, 0.75, 0.45];
            gc.push([
                warm[0] * bright as f32,
                warm[1] * bright as f32,
                warm[2] * bright as f32,
                1.0,
            ]);
            let sides = 10;
            for k in 0..sides {
                let a = std::f64::consts::TAU * k as f64 / sides as f64;
                let v = (up * (r + ground + 4.0) + (east * a.cos() + south * a.sin()) * radius)
                    - origin;
                gp.push(v.as_vec3().to_array());
                gn.push(up.as_vec3().to_array());
                gc.push([0.0, 0.0, 0.0, 1.0]);
            }
            for k in 0..sides as u32 {
                let (a, b) = (base + 1 + k, base + 1 + (k + 1) % sides as u32);
                gi.extend([base, b, a]);
            }
        }
    }
    (vertex_mesh(p, n, c, idx), vertex_mesh(gp, gn, gc, gi))
}

/// Lists each body's chunks once, keeps the near ones built and up to date (on workers), and
/// drops far ones.
#[allow(clippy::too_many_arguments)]
fn stream_chunks(
    mut commands: Commands,
    mut distant: ResMut<Distant>,
    draw: Option<Res<SettlementDraw>>,
    sim: Res<Sim>,
    universe: Res<Universe>,
    render: Res<RenderTime>,
    camera: Res<SpaceCamera>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Some(draw) = draw else { return };
    let distant = &mut *distant;
    let body = camera.anchor;
    let Some(rs) = sim
        .0
        .regions
        .get(body)
        .and_then(|r| r.world.get_resource::<RegionSociety>())
    else {
        return;
    };
    let Some(terrain) = draw.terrain(body).cloned() else {
        return;
    };
    let r = terrain.radius;
    let material = distant
        .material
        .get_or_insert_with(|| {
            materials.add(StandardMaterial {
                base_color: Color::WHITE,
                perceptual_roughness: 0.9,
                ..default()
            })
        })
        .clone();
    // List the body's chunks once.
    if !distant.listed.contains(&body) {
        distant.listed.push(body);
        for (k, site) in draw
            .sites
            .iter()
            .enumerate()
            .filter(|(_, s)| s.body == body)
        {
            let key = locate(site.settlement.direction, LEVEL);
            chunk_of(&mut distant.chunks, body, key, r)
                .contents
                .sites
                .push(k);
        }
        for (k, ruin) in rs.ruins.iter().enumerate() {
            let key = locate(ruin.direction, LEVEL);
            chunk_of(&mut distant.chunks, body, key, r)
                .contents
                .ruins
                .push(k);
        }
    }
    let cam = crate::settlements::camera_on(&universe, &camera, body, render.0);
    for ((b, _), chunk) in distant.chunks.iter_mut() {
        if *b != body {
            continue;
        }
        // Sites whose own layout is drawn are left out.
        chunk.contents.left_out = chunk
            .contents
            .sites
            .iter()
            .copied()
            .filter(|&k| !draw.sites[k].drawn.is_empty())
            .collect();
        let near = cam.distance(chunk.origin) < RANGE + 45_000.0;
        // A finished build: swap its meshes in.
        if let Some(task) = &mut chunk.task
            && task.is_finished()
            && let Some(task) = chunk.task.take()
        {
            let (forms, glow, made_from) = bevy::tasks::block_on(task);
            if let Some(root) = chunk.root.take() {
                commands.entity(root).despawn();
            }
            let root = commands
                .spawn((
                    Name::new("distant settlements"),
                    Transform::IDENTITY,
                    Visibility::Inherited,
                ))
                .id();
            commands.spawn((
                Mesh3d(meshes.add(forms)),
                MeshMaterial3d(material.clone()),
                NotShadowCaster,
                Transform::IDENTITY,
                ChildOf(root),
            ));
            // Unlit and added (Bevy's additive mode multiplies colour by alpha, so alpha stays
            // 1); its colour, times the discs' vertex colours (bright centres fading to the
            // rim), is set by night.
            let glow_material = materials.add(StandardMaterial {
                base_color: Color::linear_rgba(0.0, 0.0, 0.0, 1.0),
                unlit: true,
                alpha_mode: AlphaMode::Add,
                fog_enabled: false,
                ..default()
            });
            let glow_entity = commands
                .spawn((
                    Mesh3d(meshes.add(glow)),
                    MeshMaterial3d(glow_material.clone()),
                    NotShadowCaster,
                    Transform::IDENTITY,
                    Visibility::Hidden,
                    ChildOf(root),
                ))
                .id();
            debug!(
                "distant chunk built: {} settlements, {} ruins",
                made_from.sites.len() - made_from.left_out.len(),
                made_from.ruins.len()
            );
            chunk.root = Some(root);
            chunk.glow = Some((glow_entity, glow_material));
            chunk.built = Some(made_from);
        }
        if near {
            let stale = chunk.built.as_ref() != Some(&chunk.contents);
            if stale && chunk.task.is_none() {
                let contents = chunk.contents.clone();
                let items: Vec<Item> = contents
                    .sites
                    .iter()
                    .filter(|k| !contents.left_out.contains(k))
                    .map(|&k| {
                        let s = &draw.sites[k];
                        Item {
                            dir: s.settlement.direction,
                            size: s.settlement.size,
                            tier: Some(s.tier),
                            palette: s.palette.clone(),
                            population: s.settlement.population,
                            seed: s.seed,
                        }
                    })
                    .chain(contents.ruins.iter().map(|&k| {
                        let ruin = &rs.ruins[k];
                        Item {
                            dir: ruin.direction,
                            size: ruin.size,
                            tier: None,
                            palette: Vec::new(),
                            population: 0,
                            seed: ruin.layout_seed,
                        }
                    }))
                    .collect();
                let (origin, terrain) = (chunk.origin, terrain.clone());
                chunk.task = Some(crate::workers::workers().spawn(async move {
                    let (forms, glow) = build(&items, origin, &terrain);
                    (forms, glow, contents)
                }));
            }
        } else if let Some(root) = chunk.root.take() {
            commands.entity(root).despawn();
            chunk.built = None;
            chunk.glow = None;
        }
    }
}

fn chunk_of(
    chunks: &mut HashMap<(usize, TileKey), Chunk>,
    body: usize,
    key: TileKey,
    radius: f64,
) -> &mut Chunk {
    chunks.entry((body, key)).or_insert_with(|| Chunk {
        body,
        origin: key.center() * radius,
        contents: Contents::default(),
        built: None,
        task: None,
        root: None,
        glow: None,
    })
}

/// Places the drawn chunks camera-relative, and lights their glow by night from a height.
#[allow(clippy::too_many_arguments)]
fn place_chunks(
    distant: Res<Distant>,
    universe: Res<Universe>,
    render: Res<RenderTime>,
    camera: Res<SpaceCamera>,
    draw: Option<Res<SettlementDraw>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut transforms: Query<&mut Transform>,
    mut visibility: Query<&mut Visibility>,
) {
    let system = &universe.0;
    let anchor = system.bodies[camera.anchor].inertial_frame;
    let star =
        system
            .frames
            .transform_point(anchor, system.star().inertial_frame, render.0, DVec3::ZERO);
    let mut poses: HashMap<usize, (DVec3, DQuat)> = HashMap::new();
    for chunk in distant.chunks.values() {
        let Some(root) = chunk.root else { continue };
        let (position, rotation) = *poses.entry(chunk.body).or_insert_with(|| {
            let s =
                system
                    .frames
                    .state_in(anchor, system.bodies[chunk.body].surface_frame, render.0);
            (s.position, s.rotation)
        });
        if let Ok(mut t) = transforms.get_mut(root) {
            t.translation = (position + rotation * chunk.origin - camera.offset).as_vec3();
            t.rotation = rotation.as_quat().normalize();
        }
        let Some((glow, material)) = &chunk.glow else {
            continue;
        };
        // The camera's height above the ground there, and the sun's height at the chunk.
        let at = position + rotation * chunk.origin;
        let up = (rotation * chunk.origin).normalize();
        let sun = (star - at).normalize_or_zero().dot(up);
        let night = 1.0 - smoothstep(-0.12, 0.05, sun);
        let cam = rotation.inverse() * (camera.offset - position);
        let radius = system.bodies[chunk.body].radius;
        let ground = draw
            .as_ref()
            .and_then(|d| d.terrain(chunk.body))
            .map_or(0.0, |t| t.surface_height(cam.normalize(), 500.0));
        let high = cam.length() - radius - ground > GLOW_FROM;
        let show = night > 0.01 && high;
        if let Ok(mut v) = visibility.get_mut(*glow) {
            let want = if show {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
            if *v != want {
                *v = want;
            }
        }
        if show {
            let k = night as f32 * GLOW;
            let want = Color::linear_rgba(k, k, k, 1.0);
            if let Some(m) = materials.get(material)
                && m.base_color != want
                && let Some(mut m) = materials.get_mut(material)
            {
                m.base_color = want;
            }
        }
    }
}

fn smoothstep(e0: f64, e1: f64, x: f64) -> f64 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_chunk_s_forms_sit_on_the_ground_around_their_settlements() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../content/data/star_systems/halcyon.json");
        let sys = sim_core::StarSystem::from_path(path).unwrap();
        let terrain = Terrain::of(sys.body("meridian").unwrap()).unwrap();
        let dir = DVec3::new(0.3, 0.4, 0.86).normalize();
        let item = |size, tier| Item {
            dir,
            size,
            tier,
            palette: vec![[0.6, 0.2, 0.15], [0.4, 0.4, 0.4], [0.8, 0.8, 0.7]],
            population: 2000,
            seed: 7,
        };
        let origin = dir * terrain.radius;
        let (forms, glow) = build(
            &[
                item(SizeClass::Town, Some(TechTier::Medieval)),
                item(SizeClass::City, None),
            ],
            origin,
            &terrain,
        );
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(points)) =
            forms.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("no positions");
        };
        // 40 boxes for the town, 60 for the ruined city (20 vertices each, no bottom face),
        // and each one's built-up disc (13 vertices).
        assert_eq!(points.len(), (40 + 60) * 20 + 2 * 13);
        let ground = terrain.surface_height(dir, 100.0);
        for p in points {
            let world = origin + Vec3::from(*p).as_dvec3();
            let off = world.normalize().angle_between(dir) * terrain.radius;
            assert!(off < 3_000.0, "{off} m from the centre");
            let h = world.length() - terrain.radius - ground;
            assert!(h > -60.0 && h < 80.0, "{h} m from the ground");
        }
        // One glow disc: the living town's, not the ruin's.
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(disc)) =
            glow.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("no glow");
        };
        assert_eq!(disc.len(), 11);
    }
}
