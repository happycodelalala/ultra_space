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
    Biome, Climate, Extent, Terrain, TileKey, TileMesh, View, build_tile, estimated_heights,
    select, subsolar_latitude, tile_indices,
};

use crate::camera::{CameraSet, SpaceCamera};
use crate::sim::RenderTime;
use crate::space_render::{BodyVisual, Layer, MainCamera};
use crate::universe::{GameState, InGame, Universe};

/// Terrain is streamed while the camera is closer to a body's centre than this many radii.
const ACTIVE_DISTANCE: f64 = 2.5;
/// Most tile generation tasks in flight.
const MAX_IN_FLIGHT: usize = 48;
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
            );
    }
}

enum Tile {
    Pending(Task<TileMesh>),
    Ready {
        entities: Vec<Entity>,
        heights: (f64, f64),
        last_needed: u64,
    },
}

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
    /// Tiles drawn this frame.
    shown: HashSet<TileKey>,
    covered: bool,
}

#[derive(Resource)]
struct Terrains {
    bodies: Vec<BodyTiles>,
    frame: u64,
    ground: Handle<StandardMaterial>,
    water: Handle<StandardMaterial>,
}

/// A drawn terrain or water tile.
#[derive(Component)]
struct TerrainTile {
    body: usize,
    origin: DVec3,
}

fn setup_terrains(
    mut commands: Commands,
    universe: Res<Universe>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let bodies = universe
        .0
        .bodies
        .iter()
        .enumerate()
        .filter_map(|(i, body)| {
            let terrain = Terrain::of(body)?;
            let p = &terrain.params;
            let extent = Extent {
                radius: terrain.radius,
                min_height: -(p.ocean_depth + p.relief),
                max_height: 1.25 * p.relief,
            };
            Some(BodyTiles {
                body: i,
                climate: Climate::of(body).map(Arc::new),
                terrain: Arc::new(terrain),
                extent,
                tiles: HashMap::new(),
                estimates: Mutex::new(HashMap::new()),
                finished: Vec::new(),
                in_flight: 0,
                shown: HashSet::new(),
                covered: false,
            })
        })
        .collect();
    let ground = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.92,
        reflectance: 0.2,
        ..default()
    });
    let water = materials.add(StandardMaterial {
        base_color: Color::srgb(0.02, 0.09, 0.2),
        perceptual_roughness: 0.08,
        reflectance: 0.4,
        ..default()
    });
    commands.insert_resource(Terrains {
        bodies,
        frame: 0,
        ground,
        water,
    });
}

/// sRGB colour of a biome (placeholder palette until task 2.5). Shared by tiles and the baked
/// orbit-view maps so both show the same planet.
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

/// Linear RGBA colour of a biome, for vertex colours.
fn biome_color(b: Biome) -> [f32; 4] {
    let srgb = biome_srgb(b);
    let lin = |c: f32| c.powf(2.2);
    [lin(srgb[0]), lin(srgb[1]), lin(srgb[2]), 1.0]
}

fn tile_mesh(
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    colors: Option<Vec<[f32; 4]>>,
) -> Mesh {
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_indices(Indices::U32(tile_indices().to_vec()));
    if let Some(colors) = colors {
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
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
    out: &mut Vec<TileKey>,
) -> bool {
    let ready = matches!(tiles.get(&key), Some(Tile::Ready { .. }));
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
        .all(|c| !needed.contains(c) || cover(*c, leaves, needed, tiles, out));
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
                let colors = m.biomes.iter().map(|b| biome_color(*b)).collect();
                let ground = meshes.add(tile_mesh(m.positions, m.normals, Some(colors)));
                let mut entities = vec![
                    commands
                        .spawn((
                            TerrainTile {
                                body: bt.body,
                                origin: m.origin,
                            },
                            Mesh3d(ground),
                            MeshMaterial3d(terrains.ground.clone()),
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
                                MeshMaterial3d(terrains.water.clone()),
                                Transform::IDENTITY,
                                Visibility::Hidden,
                            ))
                            .id(),
                    );
                }
                bt.tiles.insert(
                    m.key,
                    Tile::Ready {
                        entities,
                        heights: (m.min_height, m.max_height),
                        last_needed: frame,
                    },
                );
            }

            // What to draw: complete coverage of every face, or as much as is ready.
            let mut covered = true;
            for face in 0..6u8 {
                let root = TileKey::root(face);
                covered &= cover(root, &leaves, &needed, &bt.tiles, &mut shown);
            }
            bt.covered = covered;
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
fn place_tiles(
    terrains: Res<Terrains>,
    universe: Res<Universe>,
    render: Res<RenderTime>,
    camera: Res<SpaceCamera>,
    mut tiles: Query<(&TerrainTile, &Visibility, &mut Transform)>,
) {
    let system = &universe.0;
    let anchor = system.bodies[camera.anchor].inertial_frame;
    let poses: HashMap<usize, (DVec3, DQuat)> = terrains
        .bodies
        .iter()
        .filter(|bt| !bt.shown.is_empty())
        .map(|bt| {
            let s = system
                .frames
                .state_in(anchor, system.bodies[bt.body].surface_frame, render.0);
            (bt.body, (s.position - camera.offset, s.rotation))
        })
        .collect();
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

#[cfg(test)]
mod tests {
    use super::*;

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
        };
        let mut tiles = HashMap::new();
        tiles.insert(root, ready());
        tiles.insert(children[0], ready());
        let mut out = Vec::new();
        assert!(cover(root, &leaves, &needed, &tiles, &mut out));
        assert_eq!(out, vec![root], "parent until all children are ready");

        for c in &children[1..] {
            tiles.insert(*c, ready());
        }
        out.clear();
        assert!(cover(root, &leaves, &needed, &tiles, &mut out));
        assert_eq!(out.len(), 4);
        assert!(!out.contains(&root));

        // Nothing ready: not drawable.
        out.clear();
        assert!(!cover(root, &leaves, &needed, &HashMap::new(), &mut out));
        assert!(out.is_empty());
    }
}
