//! The block world in the game (Batch 5; design: block-world.md): the demo village's ground as
//! blocks round the player, its block models, and the start there.
//!
//! At load the village (exported by `tools/blocks`) is given a site on Meridian and becomes the
//! terrain's block ground ([`sim_planet::BlockGround`]), so physics and everything that reads
//! the ground stand on its blocks. Here the ground's chunks are meshed on workers round the
//! camera and the models drawn; everything hangs on one root placed like a settlement.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::tasks::{Task, block_on, poll_once};
use glam::{DQuat, DVec3, IVec3};
use serde::Deserialize;
use sim_blocks::{BlockModel, MeshData, Palette};
use sim_planet::blocks::LodKey;
use sim_planet::{BlockGround, VillageGround};

use crate::camera::SpaceCamera;
use crate::sim::RenderTime;
use crate::universe::{InGame, Universe};

/// The coarsest level of block chunks drawn: columns 2^4 blocks (4 m) wide (block-world.md §7).
/// Beyond it the tiles, coloured and stepped like the blocks, carry on.
const TOP_LEVEL: u8 = 4;
/// The quality setting for the block ground: block geometry of a level is drawn while its
/// columns cover at least this many pixels (2–8; default 4); below that the tiles' shader draws
/// the same 25 cm steps averaged over each pixel (no zig-zag), down to a pixel and less.
/// `ULTRA_SPACE_BLOCK_PIXELS` until the settings menu has it.
fn block_pixels() -> f64 {
    std::env::var("ULTRA_SPACE_BLOCK_PIXELS")
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(4.0)
        .clamp(2.0, 8.0)
}

/// How far (m) columns of `level` stay at least `pixels` wide, at `per_radian` pixels per
/// radian of view.
fn level_reach(level: u8, per_radian: f64, pixels: f64) -> f64 {
    sim_planet::blocks::CELL * f64::from(1u32 << level) * per_radian / pixels
}

/// The chunk plan: the wanted chunks, worked out again only when the camera has moved, and
/// chunk centres remembered.
#[derive(Default)]
struct ChunkPlan {
    camera: Option<DVec3>,
    per_radian: f64,
    wanted: HashSet<LodKey>,
    centres: HashMap<LodKey, DVec3>,
}
/// No collision boxes this close to the ship's landing spot (m).
const SHIP_CLEAR: f64 = 8.0;
/// Chunk meshes being made at once.
const CHUNKS_IN_FLIGHT: usize = 64;
/// Most finished chunks shown per frame, and most triangles uploaded per frame (the first
/// chunk always goes).
const CHUNKS_PER_FRAME: usize = 16;
const TRIANGLES_PER_FRAME: usize = 400_000;

pub struct BlocksPlugin;

impl Plugin for BlocksPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BlockCoverage>()
            .add_systems(OnEnter(crate::universe::GameState::Running), setup_blocks)
            .add_systems(
                Update,
                (
                    block_focus,
                    stream_chunks,
                    place_root,
                    place_chunks,
                    night_glow,
                )
                    .chain()
                    .after(crate::camera::CameraSet)
                    .in_set(InGame),
            )
            .add_systems(
                Update,
                hold_view
                    .after(crate::camera::CameraStage::Follow)
                    .before(crate::camera::CameraStage::Anchor)
                    .in_set(InGame),
            );
    }
}

// ---- the village file ------------------------------------------------------------------------------

#[derive(Deserialize, Clone, Debug)]
pub struct GroundFile {
    pub size: f64,
    pub origin: [i32; 2],
    pub dims: [i32; 2],
    pub tops: Vec<i32>,
    pub palette: Vec<String>,
    pub colours: Vec<usize>,
    pub carved: Vec<[i32; 3]>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Placement {
    pub model: String,
    pub pos: [f64; 3],
    pub rot: f64,
    pub scale: f64,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Light {
    pub kind: String,
    pub pos: [f64; 3],
}

#[derive(Deserialize, Clone, Debug)]
pub struct Pose {
    pub pos: [f64; 3],
    #[serde(default)]
    pub rot: f64,
    #[serde(default)]
    pub facing: f64,
}

#[derive(Deserialize, Clone, Debug)]
pub struct VillageFile {
    pub name: String,
    pub ground: GroundFile,
    pub placements: Vec<Placement>,
    pub lights: Vec<Light>,
    pub ship: Pose,
    pub start: Pose,
}

/// The block world: its ground, the village and its models.
#[derive(Resource, Clone)]
pub struct BlockWorld {
    pub ground: Arc<BlockGround>,
    pub body: usize,
    pub village: Arc<VillageFile>,
    pub models: Arc<HashMap<String, BlockModel>>,
    pub palette: Arc<Palette>,
}

impl BlockWorld {
    /// Ground surface (m above the grid's floor) in the village file's own ground at (x, y).
    fn file_ground_z(&self, x: f64, y: f64) -> Option<f64> {
        let g = &self.village.ground;
        let (i, j) = BlockGround::column(x, y);
        let (a, b) = (i - g.origin[0], j - g.origin[1]);
        if a < 0 || b < 0 || a >= g.dims[0] || b >= g.dims[1] {
            return None;
        }
        Some(f64::from(g.tops[(a + b * g.dims[0]) as usize] + 1) * g.size)
    }

    /// A placement's grid point, lifted or lowered by however the game's ground differs from
    /// the file's there (the edge, where the village eases into the natural ground).
    pub fn grid_point(&self, p: [f64; 3]) -> DVec3 {
        let dz = self
            .file_ground_z(p[0], p[1])
            .map_or(0.0, |f| self.ground.ground_z(p[0], p[1]) - f);
        DVec3::new(p[0], p[1], p[2] + dz)
    }

    /// Unit direction (surface frame) and the transform in the village frame of a pose.
    pub fn local_transform(&self, p: [f64; 3], rot_deg: f64, scale: f64) -> Transform {
        let local = self.ground.local(self.grid_point(p));
        Transform {
            translation: local.as_vec3(),
            rotation: Quat::from_rotation_y(rot_deg.to_radians() as f32),
            scale: Vec3::splat(scale as f32),
        }
    }
}

/// The folder of block models: `ULTRA_SPACE_BLOCKS`, else `assets/blocks` (exported) or
/// `assets/export/blocks` (repo) next to the executable or the working directory.
pub fn blocks_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("ULTRA_SPACE_BLOCKS") {
        return Some(PathBuf::from(dir));
    }
    let starts = [std::env::current_exe().ok(), std::env::current_dir().ok()];
    starts.into_iter().flatten().find_map(|start| {
        start.ancestors().find_map(|dir| {
            ["assets/blocks", "assets/export/blocks"]
                .iter()
                .map(|sub| dir.join(sub))
                .find(|d| d.join("village.json").is_file())
        })
    })
}

/// Model names that collide, and how: a box round the whole model, or a trunk.
fn collision_of(name: &str) -> Option<bool> {
    let kind = name.rsplit_once('_').map_or(name, |(k, _)| k);
    match kind {
        "house" | "woodshed" | "well" | "fence" | "barrel" | "crates" | "wagon"
        | "lantern_post" | "bed" | "stump" | "mined" | "rock" => Some(false),
        "oak" | "birch" | "pine" => Some(true),
        _ => None,
    }
}

/// Loads the village and its models, finds its site on `body` and installs the block ground in
/// the body's terrain. Runs on the loading worker.
pub fn load(
    sim: &mut sim_core::Simulation,
    system: &sim_core::StarSystem,
    body: usize,
) -> Result<Option<BlockWorld>, String> {
    let Some(dir) = blocks_dir() else {
        warn!("block models not found (assets/export/blocks): no demo village");
        return Ok(None);
    };
    let read = |p: &Path| std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()));
    let village: VillageFile = serde_json::from_str(&read(&dir.join("village.json"))?)
        .map_err(|e| format!("village.json: {e}"))?;
    let palette = Palette::parse(&read(&dir.join("palette.json"))?).map_err(|e| e.to_string())?;
    let mut models = HashMap::new();
    for key in village
        .placements
        .iter()
        .map(|p| p.model.clone())
        .collect::<HashSet<_>>()
    {
        let model =
            BlockModel::load(&dir.join(format!("{key}.json"))).map_err(|e| e.to_string())?;
        models.insert(key, model);
    }
    let region = sim
        .regions
        .get_mut(body)
        .ok_or("no region for the village's body")?;
    let current = region
        .world
        .get_resource::<sim_vehicle::flight::RegionTerrain>()
        .and_then(|t| t.0.clone())
        .or_else(|| sim_planet::Terrain::of(&system.bodies[body]).map(Arc::new))
        .ok_or("the village's body has no terrain")?;
    let center = choose_site(&region.world, &current, &system.bodies[body])
        .ok_or("no site for the demo village")?;
    let g = &village.ground;
    let own = VillageGround {
        origin: g.origin,
        dims: g.dims,
        tops: g.tops.clone(),
        colours: g.colours.iter().map(|&c| g.palette[c].clone()).collect(),
        carved: g
            .carved
            .iter()
            .map(|c| IVec3::new(c[0], c[1], c[2]))
            .collect(),
    };
    let mut ground = BlockGround::new(current.clone(), center, Some(own));
    let world = BlockWorld {
        ground: Arc::new(BlockGround::new(current.clone(), center, None)),
        body,
        village: Arc::new(village),
        models: Arc::new(models),
        palette: Arc::new(palette),
    };
    // Collision boxes: buildings and props whole, trees by their trunks; none where the
    // player's ship lands (it is bigger than the block ship the layout was made for).
    let ship_at = DVec3::from_array(world.village.ship.pos);
    for p in &world.village.placements {
        let Some(trunk) = collision_of(&p.model) else {
            continue;
        };
        let Some(model) = world.models.get(&p.model) else {
            continue;
        };
        let at = DVec3::from_array(p.pos);
        if (at - ship_at).truncate().length() < SHIP_CLEAR {
            continue;
        }
        if trunk {
            ground.add_box(
                at + DVec3::new(0.0, 0.0, 1.5),
                0.0,
                DVec3::new(0.3, 0.3, 1.5),
            );
            continue;
        }
        let (mut lo, mut hi) = (DVec3::splat(f64::MAX), DVec3::splat(f64::MIN));
        for part in &model.parts {
            let s = part.size;
            lo = lo.min(part.grid.origin.as_dvec3() * s);
            hi = hi.max(part.grid.end().as_dvec3() * s);
        }
        let mid = (lo + hi) * 0.5 * p.scale;
        let yaw = p.rot.to_radians();
        let turned = DQuat::from_rotation_z(yaw) * mid;
        ground.add_box(at + turned, yaw, (hi - lo) * 0.5 * p.scale);
    }
    let ground = Arc::new(ground);
    let terrain = Arc::new(current.with_blocks(ground.clone()));
    region
        .world
        .insert_resource(sim_vehicle::flight::RegionTerrain(Some(terrain)));
    let world = BlockWorld { ground, ..world };
    info!(
        "demo village {:?}: {} models, {} placed, at {:.9},{:.9},{:.9} (ground {:.1} m)",
        world.village.name,
        world.models.len(),
        world.village.placements.len(),
        center.x,
        center.y,
        center.z,
        world.ground.base
    );
    Ok(Some(world))
}

/// The village's site: temperate, gently sloped land away from the cube's face seams and from
/// other settlements, not far from a medieval people's settlements (the village is theirs).
fn choose_site(
    world: &bevy::ecs::world::World,
    terrain: &sim_planet::Terrain,
    body: &sim_core::Body,
) -> Option<DVec3> {
    if let Ok(spec) = std::env::var("ULTRA_SPACE_VILLAGE_AT") {
        let v: Vec<f64> = spec
            .split(',')
            .filter_map(|s| s.trim().parse().ok())
            .collect();
        if v.len() == 3 {
            return Some(DVec3::new(v[0], v[1], v[2]).normalize());
        }
    }
    if let Some(globe) = terrain.globe() {
        return choose_globe_site(terrain, globe);
    }
    if let Some(plains) = terrain.world() {
        return choose_plain_site(terrain, plains);
    }
    let climate = sim_planet::Climate::of(body);
    let society = world.get_resource::<sim_society::RegionSociety>();
    let medieval: Vec<DVec3> = society
        .map(|s| {
            s.settlements
                .iter()
                .filter(|x| {
                    s.society.cultures[x.culture].profile.tech_tier
                        == sim_society::TechTier::Medieval
                })
                .map(|x| x.direction)
                .collect()
        })
        .unwrap_or_default();
    let all: Vec<DVec3> = society
        .map(|s| s.settlements.iter().map(|x| x.direction).collect())
        .unwrap_or_default();
    let r = terrain.radius;
    let mut best: Option<(f64, DVec3)> = None;
    for (n, dir) in sim_planet::fibonacci_directions(400_000).enumerate() {
        let h = terrain.height(dir, 1.0);
        if h < 15.0 {
            continue;
        }
        if let Some(c) = &climate {
            let t = c.temperature(dir, h, 0.0);
            if !(8.0..=20.0).contains(&t) {
                continue;
            }
        }
        // Away from the cube's face seams.
        let key = sim_planet::locate(dir, 12);
        let n_cells = f64::from(1u32 << 12);
        let (u, v) = (f64::from(key.x) / n_cells, f64::from(key.y) / n_cells);
        if !(0.08..0.92).contains(&u) || !(0.08..0.92).contains(&v) {
            continue;
        }
        let near_medieval = medieval.iter().any(|m| m.angle_between(dir) * r < 6_000.0);
        if !medieval.is_empty() && !near_medieval {
            continue;
        }
        if all.iter().any(|m| m.angle_between(dir) * r < 1_500.0) {
            continue;
        }
        let (east, south) = sim_planet::tangent_frame(dir);
        let mut lo = f64::MAX;
        let mut hi = f64::MIN;
        for (x, z) in [
            (-60.0, -60.0),
            (60.0, -60.0),
            (-60.0, 60.0),
            (60.0, 60.0),
            (0.0, 0.0),
            (-30.0, 0.0),
            (30.0, 0.0),
            (0.0, 30.0),
            (0.0, -30.0),
        ] {
            let d = (dir * r + east * x + south * z).normalize();
            let g = terrain.height(d, 1.0);
            lo = lo.min(g);
            hi = hi.max(g);
        }
        let rough = hi - lo;
        if rough < 2.5 && best.is_none_or(|b| rough < b.0) {
            best = Some((rough, dir));
            if rough < 0.8 || n > 300_000 {
                break;
            }
        }
    }
    best.map(|b| b.1)
}

/// The village's site on a round body with geography: the flattest temperate, moderately wet
/// lowland, away from the cube's face seams (where the ground grid will skew).
fn choose_globe_site(
    terrain: &sim_planet::Terrain,
    globe: &sim_planet::globe::GlobeWorld,
) -> Option<DVec3> {
    let r = terrain.radius;
    let mut best: Option<(f64, DVec3)> = None;
    for dir in sim_planet::fibonacci_directions(40_000) {
        let centre = terrain.sample(dir, 1.0).elevation;
        if !(20.0..800.0).contains(&centre) {
            continue;
        }
        let t = globe.temperature(dir, centre);
        let m = globe.wetness(dir);
        if !(8.0..=22.0).contains(&t) || !(0.35..=0.75).contains(&m) {
            continue;
        }
        // Green ground all round (the village's ground eases into what grows there).
        use sim_planet::globe::Cover;
        let green = |d: DVec3| {
            matches!(
                globe.ground_cover(d, centre, 0.0),
                Cover::Grass | Cover::Forest | Cover::Seasonal
            )
        };
        if !green(dir) {
            continue;
        }
        let key = sim_planet::locate(dir, 12);
        let n_cells = f64::from(1u32 << 12);
        let (u, v) = (f64::from(key.x) / n_cells, f64::from(key.y) / n_cells);
        if !(0.1..0.9).contains(&u) || !(0.1..0.9).contains(&v) {
            continue;
        }
        let (east, south) = sim_planet::tangent_frame(dir);
        let mut lo = f64::MAX;
        let mut hi = f64::MIN;
        for (x, z) in [
            (-60.0, -60.0),
            (60.0, -60.0),
            (-60.0, 60.0),
            (60.0, 60.0),
            (0.0, 0.0),
            (-30.0, 0.0),
            (30.0, 0.0),
            (0.0, 30.0),
            (0.0, -30.0),
        ] {
            let d = (dir * r + east * x + south * z).normalize();
            let g = terrain.height(d, 1.0);
            lo = lo.min(g);
            hi = hi.max(g);
        }
        // Away from the edge of the green land (dry or cold patches within ~200 m).
        let margin = [(200.0, 0.0), (-200.0, 0.0), (0.0, 200.0), (0.0, -200.0)]
            .iter()
            .filter(|(x, z)| !green((dir * r + east * *x + south * *z).normalize()))
            .count() as f64;
        let score = hi - lo + 2.0 * (m - 0.55).abs() + 0.05 * (t - 15.0).abs() + 10.0 * margin;
        if best.is_none_or(|b| score < b.0) {
            best = Some((score, dir));
        }
    }
    best.map(|b| b.1)
}

/// The village's site on a body of plains: the flattest temperate, moderately wet lowland well
/// inside its plain (at least 6 km from any crease), away from the sea's edge.
fn choose_plain_site(
    terrain: &sim_planet::Terrain,
    world: &sim_planet::PlainWorld,
) -> Option<DVec3> {
    let mut best: Option<(f64, DVec3)> = None;
    for plain in &world.plains.plains {
        // A lattice of candidate spots 2 km apart across the plain.
        for gx in -12..=12 {
            for gy in -12..=12 {
                let local = DVec3::new(f64::from(gx) * 2_000.0, f64::from(gy) * 2_000.0, 0.0);
                let on_plane = plain.from_local(local);
                if plain.edge_distance(on_plane) < 6_000.0 {
                    continue;
                }
                let dir = on_plane.normalize();
                let centre = terrain.sample(dir, 1.0).elevation;
                if !(20.0..800.0).contains(&centre) {
                    continue; // the shore, water or highland
                }
                let t = world.temperature(dir, centre);
                let m = world.wetness(dir);
                if !(8.0..=22.0).contains(&t) || !(0.35..=0.75).contains(&m) {
                    continue;
                }
                let elevation = |x: f64, y: f64| {
                    let p = plain.from_local(local + DVec3::new(x, y, 0.0));
                    terrain.sample(p.normalize(), 1.0).elevation
                };
                let mut lo = f64::MAX;
                let mut hi = f64::MIN;
                for (x, y) in [
                    (-60.0, -60.0),
                    (60.0, -60.0),
                    (-60.0, 60.0),
                    (60.0, 60.0),
                    (0.0, 0.0),
                    (-30.0, 0.0),
                    (30.0, 0.0),
                    (0.0, 30.0),
                    (0.0, -30.0),
                ] {
                    let e = elevation(x, y);
                    lo = lo.min(e);
                    hi = hi.max(e);
                }
                // Flattest first; a little preference for mild, middling wetness.
                let score = hi - lo + 2.0 * (m - 0.55).abs() + 0.05 * (t - 15.0).abs();
                if best.is_none_or(|b| score < b.0) {
                    best = Some((score, dir));
                }
            }
        }
    }
    best.map(|b| b.1)
}

// ---- drawing ---------------------------------------------------------------------------------------

#[derive(Component)]
struct BlockRoot;

/// A chunk of the block ground, in its own frame (surface frame of `body`).
#[derive(Component)]
struct ChunkPose {
    origin: DVec3,
    basis: DQuat,
}

/// How far round the camera the block ground is ready (m): tiles are hidden only that far.
#[derive(Resource, Default, Clone, Copy)]
pub struct BlockCoverage(pub f64);

/// A chunk made on a worker: its mesh, pose (origin, basis; surface frame), the time it took
/// (ms) and its stages (heights, cells and colours, meshing; ms).
type MadeChunk = (Mesh, DVec3, DQuat, f64, [f32; 3]);

/// Block streaming counts since the last report (logged every few seconds, for performance
/// rounds: how fast the workers keep up).
#[derive(Default)]
struct StreamStats {
    since: f64,
    built: usize,
    empty: usize,
    work_ms: f64,
    worst_ms: f64,
    triangles: usize,
    stages: [f64; 3],
}

/// What is drawn: the root, chunks shown and being made, the materials.
#[derive(Resource)]
struct BlockDraw {
    root: Entity,
    chunks: HashMap<LodKey, Option<Entity>>,
    pending: HashMap<LodKey, Task<Option<MadeChunk>>>,
    solid: Handle<StandardMaterial>,
    chunk_material: Handle<StandardMaterial>,
    glow: Handle<StandardMaterial>,
    windows: Handle<StandardMaterial>,
    lights: Vec<Entity>,
}

/// A Bevy mesh from block mesh data, colours by `colour`.
fn bevy_mesh(m: &MeshData, colour: impl Fn(u16) -> [f32; 4]) -> Mesh {
    let colours: Vec<[f32; 4]> = m.colours.iter().map(|&c| colour(c)).collect();
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, m.positions.clone())
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, m.normals.clone())
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colours)
    .with_inserted_indices(Indices::U32(m.indices.clone()))
}

/// Splits `m` into the triangles whose colour `keep` accepts and the rest.
fn split(m: &MeshData, keep: impl Fn(u16) -> bool) -> (MeshData, MeshData) {
    let (mut a, mut b) = (MeshData::default(), MeshData::default());
    for t in m.indices.chunks(3) {
        let target = if keep(m.colours[t[0] as usize]) {
            &mut a
        } else {
            &mut b
        };
        for &i in t {
            let i = i as usize;
            target.indices.push(target.positions.len() as u32);
            target.positions.push(m.positions[i]);
            target.normals.push(m.normals[i]);
            target.colours.push(m.colours[i]);
        }
    }
    (a, b)
}

/// A model part's mesh in the village frame's axes (x east, y up, z south).
fn part_axes(m: &mut MeshData) {
    for p in &mut m.positions {
        *p = [p[0], p[2], -p[1]];
    }
    for n in &mut m.normals {
        *n = [n[0], n[2], -n[1]];
    }
}

fn setup_blocks(
    mut commands: Commands,
    world: Option<Res<BlockWorld>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Some(world) = world else { return };
    let solid = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.8,
        reflectance: 0.2,
        ..default()
    });
    // Debug: chunks drawn flat magenta (where the block ground is).
    let chunk_material = if std::env::var_os("ULTRA_SPACE_DEBUG_BLOCKS").is_some() {
        materials.add(StandardMaterial {
            base_color: Color::srgb(1.0, 0.0, 1.0),
            unlit: true,
            ..default()
        })
    } else {
        solid.clone()
    };
    let glow = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.6,
        emissive: LinearRgba::rgb(1.0, 0.6, 0.25) * 4.0,
        ..default()
    });
    let windows = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.15,
        reflectance: 0.5,
        ..default()
    });
    let root = commands
        .spawn((
            BlockRoot,
            Transform::default(),
            Visibility::Visible,
            Name::new("blocks"),
        ))
        .id();
    // Models: one mesh per model part (shared by its placements), split into what glows by
    // night (windows), what always glows (lanterns) and the rest.
    let palette = world.palette.clone();
    let mut made: HashMap<String, Vec<(Handle<Mesh>, u8)>> = HashMap::new();
    for (key, model) in world.models.iter() {
        let mut list = Vec::new();
        for part in &model.parts {
            let mut m = part.mesh();
            part_axes(&mut m);
            let colour = |c: u16| {
                let [r, g, b] = palette.linear(part.colour_name(c));
                [r, g, b, 1.0]
            };
            let glows = |c: u16| part.glow.iter().any(|g| g == part.colour_name(c));
            let window = |c: u16| matches!(part.colour_name(c), "glass" | "glasshi");
            let (lit, rest) = split(&m, |c| glows(c) && !window(c));
            let (win, rest) = split(&rest, window);
            for (mesh, kind) in [(rest, 0u8), (lit, 1), (win, 2)] {
                if !mesh.is_empty() {
                    list.push((meshes.add(bevy_mesh(&mesh, colour)), kind));
                }
            }
        }
        made.insert(key.clone(), list);
    }
    for p in &world.village.placements {
        if std::env::var_os("ULTRA_SPACE_DEBUG_NO_MODELS").is_some() {
            break;
        }
        let Some(list) = made.get(&p.model) else {
            continue;
        };
        let transform = world.local_transform(p.pos, p.rot, p.scale);
        let e = commands
            .spawn((transform, Visibility::Inherited, ChildOf(root)))
            .id();
        for (mesh, kind) in list {
            let material = match kind {
                1 => glow.clone(),
                2 => windows.clone(),
                _ => solid.clone(),
            };
            commands.spawn((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material),
                Transform::default(),
                ChildOf(e),
            ));
        }
    }
    // Lanterns and lit windows: point lights, on at dusk.
    let mut lights = Vec::new();
    for l in &world.village.lights {
        let t = world.local_transform(l.pos, 0.0, 1.0);
        let (intensity, range) = if l.kind == "window" {
            (0.0, 8.0)
        } else {
            (0.0, 14.0)
        };
        lights.push(
            commands
                .spawn((
                    PointLight {
                        intensity,
                        range,
                        color: Color::srgb(1.0, 0.68, 0.36),
                        shadow_maps_enabled: false,
                        ..default()
                    },
                    t,
                    ChildOf(root),
                ))
                .id(),
        );
    }
    commands.insert_resource(BlockDraw {
        root,
        chunks: HashMap::new(),
        pending: HashMap::new(),
        solid,
        chunk_material,
        glow,
        windows,
        lights,
    });
}

/// Chunks within reach of the camera are made on workers and shown; far ones go.
#[allow(clippy::too_many_arguments)]
fn stream_chunks(
    mut commands: Commands,
    world: Option<Res<BlockWorld>>,
    draw: Option<ResMut<BlockDraw>>,
    universe: Res<Universe>,
    render: Res<RenderTime>,
    camera: Res<SpaceCamera>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut last_count: Local<usize>,
    mut coverage: ResMut<BlockCoverage>,
    mut plan: Local<ChunkPlan>,
    mut stats: Local<StreamStats>,
    real: Res<Time<Real>>,
    projections: Query<&Projection, With<crate::space_render::MainCamera>>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
) {
    let _slow = crate::debug_overlay::SlowSystem::new("stream_chunks");
    let (Some(world), Some(mut draw)) = (world, draw) else {
        return;
    };
    let cam = crate::settlements::camera_on(&universe, &camera, world.body, render.0);
    let ground = &world.ground;
    let here = cam.normalize();
    let height = ground.height_above(cam);
    // Pixels per radian of view: the window's height over the vertical field of view.
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
    let per_radian = viewport / (2.0 * (fov * 0.5).tan());
    let pixels = block_pixels();
    let outer = level_reach(TOP_LEVEL, per_radian, pixels);
    let plan = &mut *plan;
    if plan.centres.len() > 400_000 {
        plan.centres.clear();
    }
    // How near a chunk comes to the camera (m).
    let near = |centres: &mut HashMap<LodKey, DVec3>, k: &LodKey| {
        let c = *centres
            .entry(*k)
            .or_insert_with(|| ground.lod_centre_point(*k));
        c.distance(cam) - BlockGround::lod_size(k.0) * std::f64::consts::FRAC_1_SQRT_2
    };
    // The chunks wanted: a quadtree from the coarsest level, a chunk splitting into its four
    // children while their columns would still cover the pixels set; nothing where even the
    // coarsest columns are smaller (the tiles show the land). Worked out again when the camera
    // has moved a metre or the view changed.
    let moved = plan
        .camera
        .is_none_or(|c| c.distance(cam) > 1.0 || (plan.per_radian - per_radian).abs() > 1.0);
    if moved {
        plan.camera = Some(cam);
        plan.per_radian = per_radian;
        plan.wanted.clear();
        if height < outer && std::env::var_os("ULTRA_SPACE_DEBUG_NO_CHUNKS").is_none() {
            let mut stack: Vec<LodKey> = ground.lod_around(here, outer, TOP_LEVEL);
            while let Some(k) = stack.pop() {
                let d = near(&mut plan.centres, &k);
                if d >= level_reach(k.0, per_radian, pixels) {
                    continue;
                }
                if k.0 > 0 && d < level_reach(k.0 - 1, per_radian, pixels) {
                    stack.extend(BlockGround::lod_children(k));
                } else {
                    plan.wanted.insert(k);
                }
            }
        }
    }
    let wanted = &plan.wanted;
    let centres = &mut plan.centres;
    let mut near = |k: &LodKey| near(centres, k);
    // Start the nearest missing ones.
    let mut missing: Vec<(f64, LodKey)> = wanted
        .iter()
        .filter(|k| !draw.chunks.contains_key(k) && !draw.pending.contains_key(k))
        .map(|k| (near(k), *k))
        .collect();
    missing.sort_by(|a, b| a.0.total_cmp(&b.0));
    let pool = crate::workers::workers();
    for (_, key) in missing
        .into_iter()
        .take(CHUNKS_IN_FLIGHT.saturating_sub(draw.pending.len()))
    {
        let g = world.ground.clone();
        let palette = world.palette.clone();
        draw.pending.insert(
            key,
            pool.spawn(async move {
                // The Bevy mesh is made here too, off the main thread.
                let start = std::time::Instant::now();
                g.lod_chunk(key).map(|c| {
                    let mesh = bevy_mesh(&c.mesh, |i| {
                        let [r, g, b] = palette.linear(sim_planet::blocks::PALETTE[i as usize - 1]);
                        [r, g, b, 1.0]
                    });
                    (
                        mesh,
                        c.origin,
                        c.basis,
                        start.elapsed().as_secs_f64() * 1e3,
                        c.timings,
                    )
                })
            }),
        );
    }
    let done: Vec<LodKey> = draw
        .pending
        .iter_mut()
        .filter_map(|(k, t)| t.is_finished().then_some(*k))
        // A few a frame: each is a mesh upload.
        .take(CHUNKS_PER_FRAME)
        .collect();
    let mut uploaded = 0usize;
    for key in done {
        if uploaded >= TRIANGLES_PER_FRAME {
            break; // the rest next frame
        }
        let Some(task) = draw.pending.remove(&key) else {
            continue;
        };
        let m = block_on(poll_once(task)).flatten();
        match &m {
            Some((mesh, _, _, ms, stages)) => {
                for (sum, t) in stats.stages.iter_mut().zip(stages) {
                    *sum += f64::from(*t);
                }
                stats.built += 1;
                stats.work_ms += ms;
                stats.worst_ms = stats.worst_ms.max(*ms);
                let triangles = mesh.indices().map_or(0, |i| i.len() / 3);
                stats.triangles += triangles;
                uploaded += triangles;
            }
            None => stats.empty += 1,
        }
        let entity = m.map(|(mesh, origin, basis, _, _)| {
            commands
                .spawn((
                    ChunkPose { origin, basis },
                    Mesh3d(meshes.add(mesh)),
                    MeshMaterial3d(draw.chunk_material.clone()),
                    Transform::default(),
                    Visibility::Visible,
                ))
                .id()
        });
        draw.chunks.insert(key, entity);
    }
    // The ground is ready out to the nearest wanted chunk not yet shown.
    let ready = wanted
        .iter()
        .filter(|k| !draw.chunks.contains_key(k))
        .map(near)
        .fold(outer, f64::min)
        .max(0.0);
    coverage.0 = if height < outer && std::env::var_os("ULTRA_SPACE_DEBUG_NO_HIDE").is_none() {
        ready
    } else {
        0.0
    };
    let now = real.elapsed_secs_f64();
    if now - stats.since >= 5.0 {
        let span = now - stats.since;
        let shown = draw.chunks.values().filter(|c| c.is_some()).count();
        info!(
            "blocks: {shown} chunks shown of {} wanted ({} pending), {:.0} built/s, {:.1} ms each on \
             the workers (heights {:.1}, cells {:.1}, mesh {:.1}; worst {:.0} ms, {:.1} workers busy \
             on average), {:.0}k triangles built/s, {} px per column",
            wanted.len(),
            draw.pending.len(),
            stats.built as f64 / span,
            stats.work_ms / stats.built.max(1) as f64,
            stats.stages[0] / stats.built.max(1) as f64,
            stats.stages[1] / stats.built.max(1) as f64,
            stats.stages[2] / stats.built.max(1) as f64,
            stats.worst_ms,
            stats.work_ms / 1e3 / span,
            stats.triangles as f64 / span / 1e3,
            pixels
        );
        *stats = StreamStats {
            since: now,
            ..default()
        };
    }
    if draw.chunks.len() != *last_count {
        *last_count = draw.chunks.len();
        debug!(
            "block chunks: {} shown, {} empty, {} being made, {} wanted, camera {:.1} m up",
            draw.chunks.values().filter(|c| c.is_some()).count(),
            draw.chunks.values().filter(|c| c.is_none()).count(),
            draw.pending.len(),
            wanted.len(),
            height
        );
    }
    // Chunks no longer wanted go once every wanted chunk overlapping them is shown (a parent
    // stays until all its children are ready, children until their parent is), or at once
    // when they cover nothing wanted.
    let overlaps = |a: &LodKey, b: &LodKey| {
        if a.1 != b.1 {
            return false;
        }
        let (hi, lo) = if a.0 >= b.0 { (a, b) } else { (b, a) };
        let shift = i32::from(hi.0 - lo.0);
        lo.2 >> shift == hi.2 && lo.3 >> shift == hi.3
    };
    let gone: Vec<LodKey> = draw
        .chunks
        .keys()
        .filter(|k| !wanted.contains(k))
        .filter(|k| {
            let mut covering = wanted.iter().filter(|w| overlaps(k, w)).peekable();
            if covering.peek().is_none() {
                return true;
            }
            covering.all(|w| draw.chunks.contains_key(w))
        })
        .copied()
        .collect();
    for key in gone {
        if let Some(Some(e)) = draw.chunks.remove(&key) {
            commands.entity(e).despawn();
        }
    }
    // Chunks being made that are no longer wanted are forgotten.
    draw.pending.retain(|k, _| wanted.contains(k));
}

/// The block surface stands round the player (on foot or in the ship; the camera when there
/// is neither), wherever the camera goes.
fn block_focus(
    world: Option<Res<BlockWorld>>,
    sim: Res<crate::sim::Sim>,
    on_foot: Option<Res<crate::walk::OnFoot>>,
    ship: Option<Res<crate::ship::PlayerShip>>,
    universe: Res<Universe>,
    render: Res<RenderTime>,
    camera: Res<SpaceCamera>,
) {
    let Some(world) = world else { return };
    let system = &universe.0;
    let surface = system.bodies[world.body].surface_frame;
    let id = on_foot.map(|f| f.id).or_else(|| ship.map(|s| s.id));
    let player = id.and_then(|id| {
        let loc = sim.0.objects.location(id)?;
        let region = sim.0.regions.get(loc.region)?;
        let state = region.world.get::<sim_core::ObjectState>(loc.entity)?;
        let t = sim.0.universe_time().as_secs_f64();
        Some(
            system
                .frames
                .transform_point(surface, state.frame, t, state.state.position),
        )
    });
    let at = player
        .unwrap_or_else(|| crate::settlements::camera_on(&universe, &camera, world.body, render.0));
    world.ground.set_focus(Some(at.normalize_or(DVec3::Y)));
}

/// Places the chunks camera-relative, each in its own frame.
fn place_chunks(
    world: Option<Res<BlockWorld>>,
    universe: Res<Universe>,
    render: Res<RenderTime>,
    camera: Res<SpaceCamera>,
    mut chunks: Query<(&ChunkPose, &mut Transform)>,
) {
    let Some(world) = world else { return };
    let system = &universe.0;
    let anchor = system.bodies[camera.anchor].inertial_frame;
    let s = system
        .frames
        .state_in(anchor, system.bodies[world.body].surface_frame, render.0);
    for (pose, mut t) in &mut chunks {
        t.translation = (s.position + s.rotation * pose.origin - camera.offset).as_vec3();
        t.rotation = (s.rotation * pose.basis).as_quat().normalize();
    }
}

/// Places the root camera-relative, like a settlement.
fn place_root(
    world: Option<Res<BlockWorld>>,
    draw: Option<Res<BlockDraw>>,
    universe: Res<Universe>,
    render: Res<RenderTime>,
    camera: Res<SpaceCamera>,
    mut roots: Query<&mut Transform, With<BlockRoot>>,
) {
    let (Some(world), Some(draw)) = (world, draw) else {
        return;
    };
    let system = &universe.0;
    let anchor = system.bodies[camera.anchor].inertial_frame;
    let s = system
        .frames
        .state_in(anchor, system.bodies[world.body].surface_frame, render.0);
    let at = s.position + s.rotation * world.ground.origin();
    if let Ok(mut t) = roots.get_mut(draw.root) {
        t.translation = (at - camera.offset).as_vec3();
        t.rotation = (s.rotation * world.ground.basis()).as_quat().normalize();
    }
}

/// Lanterns and windows light up from dusk.
fn night_glow(
    world: Option<Res<BlockWorld>>,
    draw: Option<Res<BlockDraw>>,
    weather: Option<Res<crate::weather::Weather>>,
    universe: Res<Universe>,
    render: Res<RenderTime>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut lights: Query<(&mut PointLight, &Transform)>,
) {
    let (Some(world), Some(draw)) = (world, draw) else {
        return;
    };
    let system = &universe.0;
    let body = &system.bodies[world.body];
    let star = system.frames.transform_point(
        body.surface_frame,
        system.star().inertial_frame,
        render.0,
        DVec3::ZERO,
    );
    let up = world.ground.up_at(world.ground.origin());
    let sun = (star - world.ground.origin()).normalize_or_zero().dot(up);
    let night = (1.0 - smooth(-0.12, 0.08, sun)) as f32;
    // Wet in the rain.
    let rough = if weather.as_deref() == Some(&crate::weather::Weather::Rain) {
        0.35
    } else {
        0.8
    };
    if let Some(m) = materials.get(&draw.solid)
        && (m.perceptual_roughness - rough).abs() > 1e-3
        && let Some(mut m) = materials.get_mut(&draw.solid)
    {
        m.perceptual_roughness = rough;
    }
    // Overcast days light the lanterns early.
    let night = if weather
        .as_deref()
        .is_some_and(|w| *w != crate::weather::Weather::Clear)
    {
        night.max(0.5)
    } else {
        night
    };
    let exposure = bevy::camera::Exposure::default().exposure();
    let want = LinearRgba::rgb(1.0, 0.6, 0.25) * (night * 6.0 / exposure);
    if let Some(mut m) = materials.get_mut(&draw.glow)
        && m.emissive != want
    {
        m.emissive = want;
    }
    let win = LinearRgba::rgb(1.0, 0.62, 0.3) * (night * 3.0 / exposure);
    if let Some(mut m) = materials.get_mut(&draw.windows)
        && m.emissive != win
    {
        m.emissive = win;
    }
    for &e in &draw.lights {
        if let Ok((mut l, _)) = lights.get_mut(e) {
            let target = night * if l.range > 10.0 { 60_000.0 } else { 120_000.0 };
            if (l.intensity - target).abs() > 1.0 {
                l.intensity = target;
            }
        }
    }
}

fn smooth(e0: f64, e1: f64, x: f64) -> f64 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// A camera held at a viewpoint in the village (review shots; script action `view`, or
/// `ULTRA_SPACE_VILLAGE_VIEW=x,y,z,tx,ty,tz[,fov]`): grid metres, turning with the planet.
#[derive(Resource, Clone, Copy, Debug)]
pub struct VillageView {
    pub eye: DVec3,
    pub target: DVec3,
    pub fov: f64,
}

#[allow(clippy::too_many_arguments)]
fn hold_view(
    mut view: Option<ResMut<VillageView>>,
    world: Option<Res<BlockWorld>>,
    universe: Res<Universe>,
    render: Res<RenderTime>,
    mut camera: ResMut<SpaceCamera>,
    mut pilot: ResMut<crate::ship::PilotState>,
    mut projection: Query<&mut Projection, With<crate::space_render::MainCamera>>,
    mut checked: Local<bool>,
    mut commands: Commands,
) {
    let Some(world) = world else { return };
    if !*checked {
        *checked = true;
        if let Ok(spec) = std::env::var("ULTRA_SPACE_VILLAGE_VIEW") {
            let v: Vec<f64> = spec
                .split(',')
                .filter_map(|s| s.trim().parse().ok())
                .collect();
            if v.len() >= 6 {
                commands.insert_resource(VillageView {
                    eye: DVec3::new(v[0], v[1], v[2]),
                    target: DVec3::new(v[3], v[4], v[5]),
                    fov: v.get(6).copied().unwrap_or(50.0),
                });
                pilot.flying = false;
            }
        }
    }
    let Some(view) = view.as_deref_mut() else {
        return;
    };
    let system = &universe.0;
    let body = &system.bodies[world.body];
    let s = system
        .frames
        .state_in(body.inertial_frame, body.surface_frame, render.0);
    let g = &world.ground;
    let eye = s.rotation * g.point(view.eye);
    let target = s.rotation * g.point(view.target);
    let up = s.rotation * g.up_at(g.point(view.eye));
    camera.anchor = world.body;
    camera.offset = eye;
    camera.rotation = crate::camera::look_rotation(target - eye, up);
    for mut p in &mut projection {
        if let Projection::Perspective(pp) = p.as_mut() {
            let fov = view.fov.to_radians() as f32;
            if (pp.fov - fov).abs() > 1e-4 {
                pp.fov = fov;
            }
        }
    }
}

/// The start: the ship landed on its pad and the player on foot beside it, in the morning.
/// (`ship` is the state to give the player's ship; `walker` the walker's.)
pub fn start(
    world: &BlockWorld,
    system: &sim_core::StarSystem,
    sim: &sim_core::Simulation,
) -> Option<(sim_core::ObjectState, sim_core::ObjectState, f64)> {
    let _ = sim;
    let body = world.body;
    let g = &world.ground;
    let t = match start_time() {
        Ok(hour) => crate::settlements::at_local_hour(system, body, g.center, hour),
        Err(when) => crate::settlements::time_of_day(system, body, g.center, &when),
    };
    let frame = system.frames.state_in(
        system.bodies[body].inertial_frame,
        system.bodies[body].surface_frame,
        t,
    );
    let pose = |p: [f64; 3], heading_deg: f64, lift: f64| {
        let gp = world.grid_point(p) + DVec3::new(0.0, 0.0, lift);
        let at = g.point(gp);
        let up = g.up_at(at);
        // Heading: the scene's turn (counter-clockwise from above) from north... the models face
        // south (-y) unturned, so a heading of h faces south turned by h.
        let a = heading_deg.to_radians();
        let facing_grid = DVec3::new(a.sin(), -a.cos(), 0.0);
        let forward = (g.east * facing_grid.x + g.north * facing_grid.y).normalize();
        let forward = (forward - up * forward.dot(up)).normalize();
        let position = frame.rotation * at;
        let rotation = crate::camera::look_rotation(frame.rotation * forward, frame.rotation * up);
        let mut s =
            sim_core::ObjectState::at_rest(system.bodies[body].inertial_frame, position, rotation);
        s.state.velocity = frame.angular_velocity.cross(position);
        s.state.angular_velocity = frame.angular_velocity;
        s
    };
    let ship = pose(world.village.ship.pos, world.village.ship.rot, 3.0);
    let walker = pose(world.village.start.pos, world.village.start.facing, 1.0);
    Some((ship, walker, t))
}

/// The start's time: `ULTRA_SPACE_START_TIME` as a local hour (`Ok`) or "day", "dusk" or
/// "night" (`Err`); else mid-morning.
fn start_time() -> Result<f64, String> {
    match std::env::var("ULTRA_SPACE_START_TIME") {
        Ok(v) => v.parse::<f64>().map_err(|_| v.to_lowercase()),
        Err(_) => Ok(9.5),
    }
}

/// Whether the game starts in the village (the default) or in orbit (`ULTRA_SPACE_START=orbit`,
/// or any of the debug starts: near a settlement, at an altitude, at a sun angle).
pub fn start_in_village() -> bool {
    let set = |k: &str| std::env::var_os(k).is_some();
    std::env::var("ULTRA_SPACE_START").map_or(true, |v| v != "orbit")
        && !set("ULTRA_SPACE_START_NEAR")
        && !set("ULTRA_SPACE_START_ALTITUDE")
        && !set("ULTRA_SPACE_START_SUN_ANGLE")
        && !set("ULTRA_SPACE_START_REGION")
}
