//! Terrain tile streaming and rendering (Batch 2, task 2.3; design: planet-surface.md §3).
//!
//! For every body with terrain near the camera:
//! - select tiles for the camera (`sim_planet::select`), using exact height ranges of tiles
//!   already generated and estimates for the rest;
//! - generate missing tiles on worker threads, coarse levels first, a bounded number in flight;
//! - upload at most a few finished tiles per frame (hitch budget);
//! - draw a tile's children only when all of them are ready, keeping the parent until then, so
//!   no holes appear;
//! - drop tiles that haven't been needed for a while.
//!
//! While a body's tiles cover it completely, its distant shader sphere is hidden.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use bevy::asset::RenderAssetUsages;
use bevy::mesh::Indices;
use bevy::prelude::*;
use bevy::render::render_resource::PrimitiveTopology;
use bevy::tasks::{Task, block_on, poll_once};
use bevy::window::PrimaryWindow;
use glam::{DQuat, DVec3};
use sim_planet::quadtree::DEFAULT_SPACING_PIXELS;
use sim_planet::{
    Biome, Climate, Extent, Pad, Terrain, TileKey, TileMesh, View, build_tile, estimated_heights,
    select, subsolar_latitude, tile_indices,
};

use crate::camera::{CameraSet, SpaceCamera};
use crate::planet_maps::BakedMapHandles;
use crate::sim::RenderTime;
use crate::space_render::{BodyVisual, Layer, MainCamera, STAR_LIGHT};
use crate::terrain_material::{TerrainMaterial, TerrainUniform, WaterMaterial, WaterUniform};
use crate::universe::{GameState, InGame, Universe};

/// Terrain is streamed while the camera is closer to a body's centre than this many radii.
const ACTIVE_DISTANCE: f64 = 2.5;
/// Most tile generation tasks in flight.
const MAX_IN_FLIGHT: usize = 48;
/// Frames between creating a tile's mesh and drawing it (time for the GPU upload).
const DRAW_DELAY_FRAMES: u64 = 3;
/// Most finished tiles turned into meshes per frame.
const UPLOADS_PER_FRAME: usize = 16;
/// A tile not needed for this many frames is dropped.
const EVICT_AFTER_FRAMES: u64 = 300;

pub struct TerrainPlugin;

impl Plugin for TerrainPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(GameState::Running), setup_terrains)
            .add_systems(
                Update,
                (update_terrain, place_tiles)
                    .chain()
                    .after(CameraSet)
                    .in_set(InGame),
            )
            .add_systems(Update, pad_key.before(crate::sim::SimSet).in_set(InGame));
    }
}

enum Tile {
    Pending(Task<TileMesh>),
    Ready {
        entities: Vec<Entity>,
        heights: (f64, f64),
        last_needed: u64,
        /// First frame the tile may be drawn: its mesh needs a few frames to reach the GPU,
        /// and swapping it in earlier leaves a brief hole where neither it nor the tile it
        /// replaces is drawn.
        drawable_from: u64,
    },
}

/// A rebuilt tile waiting to replace the drawn one: entities, height range, and the first frame
/// it may be drawn.
type Incoming = (Vec<Entity>, (f64, f64), u64);

struct BodyTiles {
    body: usize,
    terrain: Arc<Terrain>,
    climate: Option<Arc<Climate>>,
    extent: Extent,
    tiles: HashMap<TileKey, Tile>,
    /// Height estimates for tiles not generated yet (a cache; uncontended).
    estimates: Mutex<HashMap<TileKey, (f64, f64)>>,
    finished: Vec<TileMesh>,
    in_flight: usize,
    /// Drawn tiles out of date after a terrain edit, waiting to be rebuilt.
    stale: HashSet<TileKey>,
    /// Rebuilds in progress (the old tile stays drawn meanwhile).
    rebuilding: HashMap<TileKey, Task<TileMesh>>,
    /// Rebuilt tiles uploaded but not drawable yet.
    incoming: HashMap<TileKey, Incoming>,
    /// Tiles drawn this frame.
    shown: HashSet<TileKey>,
    /// Tiles have covered the whole body since it became active. Sticky while active: the
    /// distant sphere must never reappear under drawn tiles (both sit at sea level and would
    /// z-fight), and coarse parents always remain to fall back on.
    covered: bool,
    /// The camera is close enough for tiles (set every frame).
    active: bool,
    ground: Handle<TerrainMaterial>,
    water: Handle<WaterMaterial>,
}

/// Whether every body near the camera has its terrain on screen (for the start-up warm-up).
pub fn terrain_ready(terrains: Option<&Terrains>) -> bool {
    terrains.is_some_and(|t| t.bodies.iter().all(|bt| !bt.active || bt.covered))
}

#[derive(Resource)]
pub struct Terrains {
    bodies: Vec<BodyTiles>,
    frame: u64,
}

/// A drawn terrain or water tile.
#[derive(Component)]
struct TerrainTile {
    body: usize,
    origin: DVec3,
}

/// Vertex fields blend into the baked map's fields between these distances (m): where tile
/// vertex spacing (≈ distance × 12 px / 935 px per radian) passes a map texel (≈ 3.7 km on
/// Meridian), so the finer source is always used.
const MAP_BLEND: (f32, f32) = (150_000.0, 400_000.0);

fn linear(c: [f32; 3]) -> Vec4 {
    Vec4::new(c[0].powf(2.2), c[1].powf(2.2), c[2].powf(2.2), 1.0)
}

fn setup_terrains(
    mut commands: Commands,
    universe: Res<Universe>,
    sim: Res<crate::sim::Sim>,
    mut ground_materials: ResMut<Assets<TerrainMaterial>>,
    mut water_materials: ResMut<Assets<WaterMaterial>>,
) {
    let bodies = universe
        .0
        .bodies
        .iter()
        .enumerate()
        .filter_map(|(i, body)| {
            // The region's terrain (with its settlements' foundations), else the body's own.
            let terrain = sim
                .0
                .regions
                .get(i)
                .and_then(|r| r.world.get_resource::<sim_vehicle::flight::RegionTerrain>())
                .and_then(|t| t.0.as_deref().cloned())
                .or_else(|| Terrain::of(body))?;
            let p = &terrain.params;
            let extent = Extent {
                radius: terrain.radius,
                min_height: -(p.ocean_depth + p.relief),
                max_height: 1.25 * p.relief,
            };
            let ground = ground_materials.add(TerrainMaterial {
                data: TerrainUniform {
                    params: Vec4::new(0.0, MAP_BLEND.0, MAP_BLEND.1, 0.0),
                    surface: surface_flags(body),
                    palette: biome_palette(),
                    ..default()
                },
                field_map: None,
            });
            let sky = body.atmosphere.as_ref().map_or(Vec4::ZERO, |a| {
                Vec4::new(a.color[0] as f32, a.color[1] as f32, a.color[2] as f32, 1.0) * 0.35
            });
            let water = water_materials.add(WaterMaterial {
                data: WaterUniform {
                    color: linear(biome_srgb(Biome::Ocean)).truncate().extend(0.0),
                    sky,
                    ..default()
                },
            });
            Some(BodyTiles {
                ground,
                water,
                body: i,
                climate: Climate::of(body).map(Arc::new),
                terrain: Arc::new(terrain),
                extent,
                tiles: HashMap::new(),
                estimates: Mutex::new(HashMap::new()),
                finished: Vec::new(),
                in_flight: 0,
                stale: HashSet::new(),
                rebuilding: HashMap::new(),
                incoming: HashMap::new(),
                shown: HashSet::new(),
                covered: false,
                active: false,
            })
        })
        .collect();
    commands.insert_resource(Terrains { bodies, frame: 0 });
}

/// sRGB colour of a biome. Shaders get these (as linear colours) and decide biomes per pixel, so
/// tiles and the orbit view show the same planet.
pub fn biome_srgb(b: Biome) -> [f32; 3] {
    match b {
        Biome::Ocean => [0.05, 0.2, 0.45],
        Biome::SeaIce => [0.82, 0.88, 0.93],
        Biome::Beach => [0.82, 0.76, 0.58],
        Biome::Desert => [0.8, 0.66, 0.44],
        Biome::Grassland => [0.42, 0.55, 0.26],
        Biome::Forest => [0.2, 0.36, 0.16],
        Biome::Jungle => [0.12, 0.3, 0.1],
        Biome::Taiga => [0.24, 0.34, 0.27],
        Biome::Tundra => [0.52, 0.5, 0.42],
        Biome::Ice => [0.93, 0.95, 0.98],
        Biome::Rock => [0.42, 0.4, 0.38],
        Biome::Regolith => [0.55, 0.53, 0.5],
    }
}

/// Biomes in shader index order (matches `sim_planet::Biome` and the shaders' biome rules).
pub const BIOMES: [Biome; 12] = [
    Biome::Ocean,
    Biome::SeaIce,
    Biome::Beach,
    Biome::Desert,
    Biome::Grassland,
    Biome::Forest,
    Biome::Jungle,
    Biome::Taiga,
    Biome::Tundra,
    Biome::Ice,
    Biome::Rock,
    Biome::Regolith,
];

/// Linear biome colours in shader index order.
pub fn biome_palette() -> [Vec4; 12] {
    BIOMES.map(|b| linear(biome_srgb(b)))
}

/// Biome-rule flags for shaders: x = has ocean, y = has atmosphere.
pub fn surface_flags(body: &sim_core::Body) -> Vec4 {
    let ocean = body
        .terrain
        .as_ref()
        .is_some_and(|t| t.ocean_coverage > 0.0);
    Vec4::new(
        f32::from(u8::from(ocean)),
        f32::from(u8::from(body.atmosphere.is_some())),
        0.0,
        0.0,
    )
}

/// A tile mesh; `fields` (height, temperature, moisture, continent signal) go into the two UV
/// channels for per-pixel biomes.
fn tile_mesh(
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    fields: Option<&[[f32; 4]]>,
) -> Mesh {
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_indices(Indices::U32(tile_indices().to_vec()));
    if let Some(fields) = fields {
        let a: Vec<[f32; 2]> = fields.iter().map(|f| [f[0], f[1]]).collect();
        let b: Vec<[f32; 2]> = fields.iter().map(|f| [f[2], f[3]]).collect();
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, a);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, b);
    }
    mesh
}

/// Whether `key` (or, failing that, a full set of its descendants) can be drawn; collects the
/// tiles to draw. `needed` holds the selected leaves and all their ancestors.
fn cover(
    key: TileKey,
    leaves: &HashSet<TileKey>,
    needed: &HashSet<TileKey>,
    tiles: &HashMap<TileKey, Tile>,
    frame: u64,
    out: &mut Vec<TileKey>,
) -> bool {
    let ready = matches!(
        tiles.get(&key),
        Some(Tile::Ready { drawable_from, .. }) if frame >= *drawable_from
    );
    if leaves.contains(&key) {
        if ready {
            out.push(key);
        }
        return ready;
    }
    // An inner node: draw the children if every needed one can be drawn (children culled
    // behind the horizon need nothing).
    let mark = out.len();
    let children_ok = key
        .children()
        .iter()
        .all(|c| !needed.contains(c) || cover(*c, leaves, needed, tiles, frame, out));
    if children_ok {
        return true;
    }
    out.truncate(mark);
    if ready {
        out.push(key);
    }
    ready
}

#[allow(clippy::too_many_arguments)]
fn update_terrain(
    mut commands: Commands,
    mut terrains: ResMut<Terrains>,
    mut meshes: ResMut<Assets<Mesh>>,
    universe: Res<Universe>,
    render: Res<RenderTime>,
    camera: Res<SpaceCamera>,
    projections: Query<&Projection, With<MainCamera>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut visibility: Query<&mut Visibility, With<TerrainTile>>,
    mut spheres: Query<(&BodyVisual, &mut Visibility), Without<TerrainTile>>,
) {
    let system = &universe.0;
    let t = render.0;
    let fov = projections
        .iter()
        .find_map(|p| match p {
            Projection::Perspective(p) => Some(f64::from(p.fov)),
            _ => None,
        })
        .unwrap_or(1.0);
    let viewport = windows
        .iter()
        .next()
        .map_or(1080.0, |w| f64::from(w.height()));
    let terrains = &mut *terrains;
    terrains.frame += 1;
    let frame = terrains.frame;
    let anchor = system.bodies[camera.anchor].inertial_frame;

    for bt in &mut terrains.bodies {
        let body = &system.bodies[bt.body];
        let cam = system
            .frames
            .transform_point(body.surface_frame, anchor, t, camera.offset);
        let active = cam.length() < ACTIVE_DISTANCE * bt.extent.radius;
        bt.active = active;

        // Collect finished generation tasks.
        let mut done = Vec::new();
        for (key, tile) in bt.tiles.iter_mut() {
            if let Tile::Pending(task) = tile
                && let Some(mesh) = block_on(poll_once(task))
            {
                done.push((*key, mesh));
            }
        }
        for (key, mesh) in done {
            bt.tiles.remove(&key);
            bt.in_flight -= 1;
            bt.finished.push(mesh);
        }
        let rebuilt: Vec<TileKey> = bt
            .rebuilding
            .iter_mut()
            .filter_map(|(key, task)| block_on(poll_once(task)).map(|m| (*key, m)))
            .map(|(key, mesh)| {
                bt.finished.push(mesh);
                key
            })
            .collect();
        for key in rebuilt {
            bt.rebuilding.remove(&key);
            bt.in_flight -= 1;
        }
        // Swap in rebuilt tiles once they are drawable; the old ones go only then.
        let ready: Vec<TileKey> = bt
            .incoming
            .iter()
            .filter(|(_, (_, _, from))| frame >= *from)
            .map(|(k, _)| *k)
            .collect();
        for key in ready {
            let (entities, heights, from) = bt.incoming.remove(&key).expect("listed");
            if let Some(Tile::Ready {
                entities: old,
                heights: h,
                drawable_from,
                ..
            }) = bt.tiles.get_mut(&key)
            {
                for e in std::mem::replace(old, entities) {
                    commands.entity(e).despawn();
                }
                *h = heights;
                *drawable_from = from;
            } else {
                for e in entities {
                    commands.entity(e).despawn();
                }
            }
        }

        let mut shown = Vec::new();
        let mut needed = HashSet::new();
        if active {
            let view = View {
                camera: cam,
                fov_y: fov,
                viewport_height: viewport,
                max_spacing_pixels: DEFAULT_SPACING_PIXELS,
            };
            let leaves: HashSet<TileKey> = {
                let estimate = estimated_heights(&bt.terrain, bt.extent);
                let heights = |k: TileKey| match bt.tiles.get(&k) {
                    Some(Tile::Ready { heights, .. }) => *heights,
                    _ => *bt
                        .estimates
                        .lock()
                        .expect("estimates")
                        .entry(k)
                        .or_insert_with(|| estimate(k)),
                };
                select(&view, &bt.extent, &heights).into_iter().collect()
            };
            for leaf in &leaves {
                let mut k = Some(*leaf);
                while let Some(key) = k {
                    if !needed.insert(key) {
                        break;
                    }
                    k = key.parent();
                }
            }

            // Request missing tiles, coarse first, nearest first within a level.
            let mut missing: Vec<TileKey> = needed
                .iter()
                .filter(|k| !bt.tiles.contains_key(k) && !bt.finished.iter().any(|m| m.key == **k))
                .copied()
                .collect();
            let cam_dir = cam.normalize_or(DVec3::Y);
            missing.sort_by(|a, b| {
                a.level.cmp(&b.level).then(
                    a.center()
                        .angle_between(cam_dir)
                        .total_cmp(&b.center().angle_between(cam_dir)),
                )
            });
            let season = subsolar_latitude(system, bt.body, t);
            // Rebuild tiles changed by terrain edits first (they are on screen).
            let stale: Vec<TileKey> = bt
                .stale
                .iter()
                .filter(|k| matches!(bt.tiles.get(k), Some(Tile::Ready { .. })))
                .copied()
                .collect();
            for key in stale {
                if bt.in_flight >= MAX_IN_FLIGHT {
                    break;
                }
                bt.stale.remove(&key);
                let terrain = bt.terrain.clone();
                let climate = bt.climate.clone();
                let task = crate::workers::workers()
                    .spawn(async move { build_tile(&terrain, climate.as_deref(), key, season) });
                bt.rebuilding.insert(key, task);
                bt.in_flight += 1;
            }
            for key in missing {
                if bt.in_flight >= MAX_IN_FLIGHT {
                    break;
                }
                let terrain = bt.terrain.clone();
                let climate = bt.climate.clone();
                let task = crate::workers::workers()
                    .spawn(async move { build_tile(&terrain, climate.as_deref(), key, season) });
                bt.tiles.insert(key, Tile::Pending(task));
                bt.in_flight += 1;
            }

            // Upload a few finished tiles.
            let count = bt.finished.len().min(UPLOADS_PER_FRAME);
            for m in bt.finished.drain(..count) {
                let ground = meshes.add(tile_mesh(m.positions, m.normals, Some(&m.fields)));
                let mut entities = vec![
                    commands
                        .spawn((
                            TerrainTile {
                                body: bt.body,
                                origin: m.origin,
                            },
                            Mesh3d(ground),
                            MeshMaterial3d(bt.ground.clone()),
                            Transform::IDENTITY,
                            Visibility::Hidden,
                        ))
                        .id(),
                ];
                if let Some(w) = m.water {
                    let water = meshes.add(tile_mesh(w.positions, w.normals, None));
                    entities.push(
                        commands
                            .spawn((
                                TerrainTile {
                                    body: bt.body,
                                    origin: m.origin,
                                },
                                Mesh3d(water),
                                MeshMaterial3d(bt.water.clone()),
                                Transform::IDENTITY,
                                Visibility::Hidden,
                            ))
                            .id(),
                    );
                }
                let heights = (m.min_height, m.max_height);
                if matches!(bt.tiles.get(&m.key), Some(Tile::Ready { .. })) {
                    // A rebuild: keep the old tile drawn until this one is ready.
                    if let Some((old, _, _)) = bt
                        .incoming
                        .insert(m.key, (entities, heights, frame + DRAW_DELAY_FRAMES))
                    {
                        for e in old {
                            commands.entity(e).despawn();
                        }
                    }
                } else {
                    bt.tiles.insert(
                        m.key,
                        Tile::Ready {
                            entities,
                            heights,
                            last_needed: frame,
                            drawable_from: frame + DRAW_DELAY_FRAMES,
                        },
                    );
                }
            }

            // What to draw: complete coverage of every face, or as much as is ready.
            let mut covered = true;
            for face in 0..6u8 {
                let root = TileKey::root(face);
                covered &= cover(root, &leaves, &needed, &bt.tiles, frame, &mut shown);
            }
            bt.covered |= covered;
        } else {
            bt.covered = false;
            bt.finished.clear();
        }

        // Visibility, bookkeeping and eviction.
        let shown_set: HashSet<TileKey> = shown.iter().copied().collect();
        let mut evict = Vec::new();
        for (key, tile) in bt.tiles.iter_mut() {
            if let Tile::Ready {
                entities,
                last_needed,
                ..
            } = tile
            {
                let draw = shown_set.contains(key);
                for e in entities.iter() {
                    if let Ok(mut v) = visibility.get_mut(*e) {
                        let want = if draw {
                            Visibility::Visible
                        } else {
                            Visibility::Hidden
                        };
                        if *v != want {
                            *v = want;
                        }
                    }
                }
                if needed.contains(key) {
                    *last_needed = frame;
                } else if frame - *last_needed > EVICT_AFTER_FRAMES {
                    evict.push(*key);
                }
            }
        }
        for key in evict {
            if let Some(Tile::Ready { entities, .. }) = bt.tiles.remove(&key) {
                for e in entities {
                    commands.entity(e).despawn();
                }
            }
            if let Some((entities, _, _)) = bt.incoming.remove(&key) {
                for e in entities {
                    commands.entity(e).despawn();
                }
            }
            bt.stale.remove(&key);
            bt.estimates.lock().expect("estimates").remove(&key);
        }
        bt.shown = shown_set;

        // Hand over from the distant shader sphere when the tiles cover the body.
        for (visual, mut v) in &mut spheres {
            if visual.body == bt.body && visual.layer == Layer::Surface {
                let want = if bt.covered {
                    Visibility::Hidden
                } else {
                    Visibility::Inherited
                };
                if *v != want {
                    *v = want;
                }
            }
        }
    }
}

/// Places drawn tiles camera-relative: tile origin in the body's surface frame, moved into the
/// camera anchor's frame at the render time.
#[allow(clippy::too_many_arguments)]
fn place_tiles(
    terrains: Res<Terrains>,
    universe: Res<Universe>,
    render: Res<RenderTime>,
    real: Res<Time<Real>>,
    camera: Res<SpaceCamera>,
    maps: Res<BakedMapHandles>,
    mut ground_materials: ResMut<Assets<TerrainMaterial>>,
    mut water_materials: ResMut<Assets<WaterMaterial>>,
    mut tiles: Query<(&TerrainTile, &Visibility, &mut Transform)>,
) {
    let system = &universe.0;
    let anchor = system.bodies[camera.anchor].inertial_frame;
    let star = (system.frames.transform_point(
        anchor,
        system.star().inertial_frame,
        render.0,
        DVec3::ZERO,
    ) - camera.offset)
        .as_vec3()
        .extend(STAR_LIGHT);
    let mut poses: HashMap<usize, (DVec3, DQuat)> = HashMap::new();
    for bt in terrains.bodies.iter().filter(|bt| !bt.shown.is_empty()) {
        let s = system
            .frames
            .state_in(anchor, system.bodies[bt.body].surface_frame, render.0);
        let (position, rotation) = (s.position - camera.offset, s.rotation);
        poses.insert(bt.body, (position, rotation));
        // Per-body lighting uniforms, shared formula with the distant planet shader.
        let center = position.as_vec3().extend(bt.extent.radius as f32);
        let q = rotation.as_quat().normalize();
        let q = Vec4::new(q.x, q.y, q.z, q.w);
        if let Some(mut m) = ground_materials.get_mut(&bt.ground) {
            m.data.star = star;
            m.data.center = center;
            m.data.rotation = q;
            if m.field_map.is_none()
                && let Some(map) = maps.fields.get(&bt.body)
            {
                m.field_map = Some(map.clone());
                m.data.params.x = 1.0;
            }
        }
        if let Some(mut m) = water_materials.get_mut(&bt.water) {
            m.data.star = star;
            m.data.center = center;
            m.data.rotation = q;
            m.data.color.w = real.elapsed_secs_wrapped();
        }
    }
    for (tile, visibility, mut transform) in &mut tiles {
        if *visibility == Visibility::Hidden {
            continue;
        }
        let Some((position, rotation)) = poses.get(&tile.body) else {
            continue;
        };
        transform.translation = (*position + *rotation * tile.origin).as_vec3();
        transform.rotation = rotation.as_quat().normalize();
    }
}

/// Tiles built from an older terrain: drawn ones are rebuilt in place (the old one stays until
/// its replacement is ready), the rest dropped.
fn invalidate(bt: &mut BodyTiles, commands: &mut Commands, keys: Vec<TileKey>) {
    for key in keys {
        bt.estimates.lock().expect("estimates").remove(&key);
        if bt.shown.contains(&key) {
            bt.stale.insert(key);
        } else if let Some(Tile::Ready { entities, .. }) = bt.tiles.remove(&key) {
            for e in entities {
                commands.entity(e).despawn();
            }
        } else {
            // Still being generated from the old terrain: rebuild once it arrives.
            bt.stale.insert(key);
        }
    }
}

/// Replaces a body's terrain (a loaded save's terrain deltas): every tile is rebuilt from it.
pub fn replace_terrain(
    terrains: &mut Terrains,
    commands: &mut Commands,
    body: usize,
    terrain: Arc<Terrain>,
) {
    let Some(bt) = terrains.bodies.iter_mut().find(|bt| bt.body == body) else {
        return;
    };
    bt.terrain = terrain;
    let keys: Vec<TileKey> = bt.tiles.keys().copied().collect();
    invalidate(bt, commands, keys);
}

/// Replaces a body's terrain with one whose settlement foundations changed (task 4b.5): only
/// the tiles within reach of `changed` (unit directions with their angular reach) are rebuilt,
/// each old tile drawn until its replacement is ready.
pub fn update_terrain_near(
    terrains: &mut Terrains,
    commands: &mut Commands,
    body: usize,
    terrain: Arc<Terrain>,
    changed: &[(DVec3, f64)],
) {
    let Some(bt) = terrains.bodies.iter_mut().find(|bt| bt.body == body) else {
        return;
    };
    bt.terrain = terrain;
    if changed.is_empty() {
        return;
    }
    let touched: Vec<TileKey> = bt
        .tiles
        .keys()
        .filter(|key| {
            let c = key.center();
            let spread = [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)]
                .iter()
                .map(|&(s, u)| c.angle_between(key.direction(s, u)))
                .fold(0.0, f64::max);
            changed
                .iter()
                .any(|&(centre, reach)| c.angle_between(centre) <= spread + reach)
        })
        .copied()
        .collect();
    invalidate(bt, commands, touched);
}

/// Landing pad size (m): flat radius and the blend back to natural terrain.
const PAD_RADIUS: f64 = 30.0;
const PAD_BLEND: f64 = 20.0;
/// The ship must be this low (m above ground) to flatten a pad under it.
const PAD_MAX_SHIP_HEIGHT: f64 = 50.0;

/// K (debug, first terrain delta, task 2.11): flattens a landing pad under the player, on foot or
/// in a ship near the ground, at the ground's current height there.
#[allow(clippy::too_many_arguments)]
fn pad_key(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    universe: Res<Universe>,
    ship: Option<Res<crate::ship::PlayerShip>>,
    on_foot: Option<Res<crate::walk::OnFoot>>,
    pilot: Res<crate::ship::PilotState>,
    mut terrains: ResMut<Terrains>,
    mut sim: ResMut<crate::sim::Sim>,
) {
    if !pilot.flying || !keys.just_pressed(KeyCode::KeyK) {
        return;
    }
    let system = &universe.0;
    let sim = &mut sim.0;
    let t = sim.universe_time().as_secs_f64();
    // Where the player is: the walker, or the ship if it's close to the ground.
    let (region, state) = if let Some(f) = on_foot {
        match crate::walk::walker_status(sim, f.id) {
            Some((region, state, _)) => (region, state),
            None => return,
        }
    } else {
        let Some((region, state, telemetry, _)) =
            ship.and_then(|s| crate::ship::player_ship_state(sim, &s))
        else {
            return;
        };
        if telemetry.ground_altitude > PAD_MAX_SHIP_HEIGHT {
            info!(
                "too high to flatten a pad ({:.0} m above ground)",
                telemetry.ground_altitude
            );
            return;
        }
        (region, state)
    };
    let Some(bt) = terrains.bodies.iter_mut().find(|bt| bt.body == region) else {
        return;
    };
    let body = &system.bodies[region];
    let local = state.in_frame(&system.frames, body.inertial_frame, t).state;
    let center = system
        .frames
        .transform_point(body.surface_frame, body.inertial_frame, t, local.position)
        .normalize();
    let pad = Pad {
        center,
        radius: PAD_RADIUS,
        blend: PAD_BLEND,
        height: bt.terrain.height(center, 1.0),
    };
    let terrain = Arc::new(bt.terrain.with_pad(pad));
    bt.terrain = terrain.clone();

    // Tiles touching the pad: rebuild the drawn ones in place, drop the rest.
    let touched: Vec<TileKey> = bt
        .tiles
        .keys()
        .filter(|key| {
            let c = key.center();
            let spread = [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)]
                .iter()
                .map(|&(s, u)| c.angle_between(key.direction(s, u)))
                .fold(0.0, f64::max);
            terrain.pad_touches(&pad, c, spread)
        })
        .copied()
        .collect();
    invalidate(bt, &mut commands, touched);

    // Physics uses the same terrain at once.
    if let Some(r) = sim.regions.get_mut(region) {
        r.world
            .insert_resource(sim_vehicle::flight::RegionTerrain(Some(terrain)));
    }
    info!(
        "flattened a {:.0} m landing pad on {} at height {:.0} m",
        2.0 * PAD_RADIUS,
        body.name,
        pad.height
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn biome_block(shader: &str) -> &str {
        let start = shader.find("// BIOMES BEGIN").expect("biome block start");
        let end = shader.find("// BIOMES END").expect("biome block end");
        &shader[start..end]
    }

    #[test]
    fn shader_biome_rules_are_identical_in_both_shaders() {
        let planet = include_str!("shaders/planet.wesl");
        let terrain = include_str!("shaders/terrain.wesl");
        assert_eq!(biome_block(planet), biome_block(terrain));
    }

    #[test]
    fn palette_order_matches_the_biome_enum() {
        // The shaders index the palette by `sim_planet::Biome` order.
        for (i, b) in BIOMES.iter().enumerate() {
            assert_eq!(*b as usize, i, "{b:?}");
        }
    }

    #[test]
    fn cover_falls_back_to_the_parent_until_all_children_are_ready() {
        let root = TileKey::root(0);
        let children = root.children();
        let leaves: HashSet<TileKey> = children.iter().copied().collect();
        let needed: HashSet<TileKey> = leaves.iter().copied().chain([root]).collect();
        let ready = || Tile::Ready {
            entities: Vec::new(),
            heights: (0.0, 0.0),
            last_needed: 0,
            drawable_from: 0,
        };
        let mut tiles = HashMap::new();
        tiles.insert(root, ready());
        tiles.insert(children[0], ready());
        let mut out = Vec::new();
        assert!(cover(root, &leaves, &needed, &tiles, 10, &mut out));
        assert_eq!(out, vec![root], "parent until all children are ready");

        for c in &children[1..] {
            tiles.insert(*c, ready());
        }
        out.clear();
        assert!(cover(root, &leaves, &needed, &tiles, 10, &mut out));
        assert_eq!(out.len(), 4);
        assert!(!out.contains(&root));

        // A child that only just arrived isn't drawn yet: the parent stays.
        if let Some(Tile::Ready { drawable_from, .. }) = tiles.get_mut(&children[3]) {
            *drawable_from = 12;
        }
        out.clear();
        assert!(cover(root, &leaves, &needed, &tiles, 10, &mut out));
        assert_eq!(out, vec![root], "new tile waits for its GPU upload");

        // Nothing ready: not drawable.
        out.clear();
        assert!(!cover(
            root,
            &leaves,
            &needed,
            &HashMap::new(),
            10,
            &mut out
        ));
        assert!(out.is_empty());
    }
}
