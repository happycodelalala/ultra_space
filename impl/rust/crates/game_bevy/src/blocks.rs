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
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy::shader::ShaderRef;
use bevy::tasks::{Task, block_on, poll_once};
use glam::{DQuat, DVec2, DVec3};
use serde::Deserialize;
use sim_blocks::{BlockModel, MeshData, Palette};
use sim_planet::BlockGround;
use sim_planet::blocks::LodKey;

use crate::camera::SpaceCamera;
use crate::sim::RenderTime;
use crate::universe::{InGame, Universe};

/// How the ground near the player is made: faceted (a density field, flat-shaded; adopted
/// 2026-11-18), or `ULTRA_SPACE_GROUND=blocks` for the block ground.
pub fn ground_style() -> sim_planet::GroundStyle {
    match std::env::var("ULTRA_SPACE_GROUND").as_deref() {
        Ok("blocks") => sim_planet::GroundStyle::Blocks,
        _ => sim_planet::GroundStyle::Faceted,
    }
}

/// The coarsest level of block chunks drawn: columns 2^4 blocks (4 m) wide (block-world.md §7).
/// Beyond it the tiles, coloured and stepped like the blocks, carry on.
const TOP_LEVEL: u8 = 4;
/// The quality setting for the ground near the player: its geometry of a level is drawn while
/// its cells cover at least this many pixels (2–12). Default 8 for the faceted ground (flat
/// facets have no steps to alias), 4 for blocks (below that the tiles' shader draws their steps).
/// `ULTRA_SPACE_BLOCK_PIXELS` until the settings menu has it.
fn block_pixels() -> f64 {
    let default = match ground_style() {
        sim_planet::GroundStyle::Faceted => 8.0,
        sim_planet::GroundStyle::Blocks => 4.0,
    };
    std::env::var("ULTRA_SPACE_BLOCK_PIXELS")
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(default)
        .clamp(2.0, 12.0)
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
/// Most finished chunks shown per frame, and most triangles uploaded per frame (the first
/// chunk always goes).
const CHUNKS_PER_FRAME: usize = 16;
const TRIANGLES_PER_FRAME: usize = 400_000;
/// How long a chunk and its cover take to dissolve in or out (s).
const FADE: f64 = 0.6;
/// Chunks kept after they go, hidden, for turning back or circling (triangles).
const CACHE_TRIANGLES: usize = 1_500_000;
/// How far ahead the camera's path is predicted, for ordering the work and building ahead (s).
const LOOK_AHEAD: f64 = 1.5;
/// Near the camera a new chunk over bare ground shows at once (the far tiles under it are
/// hidden there, so a dissolve would show holes) (m).
const SHOW_AT_ONCE: f64 = 60.0;

pub struct BlocksPlugin;

impl Plugin for BlocksPlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "shaders/ground.wesl");
        bevy::asset::embedded_asset!(app, "shaders/ground_prepass.wesl");
        bevy::asset::embedded_asset!(app, "shaders/cover.wesl");
        bevy::asset::embedded_asset!(app, "shaders/cover_prepass.wesl");
        app.add_plugins(bevy::pbr::MaterialPlugin::<GroundMaterial>::default())
            .add_plugins(bevy::pbr::MaterialPlugin::<CoverMaterial>::default())
            .init_resource::<BlockCoverage>()
            .add_systems(OnEnter(crate::universe::GameState::Running), setup_blocks)
            .add_systems(
                Update,
                (
                    block_focus,
                    stream_chunks,
                    fade_chunks,
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

/// The prototype's own ground in the village file (its 25 cm cells): read only to know what
/// the placements stood on, so they can be set on the game's ground instead.
#[derive(Deserialize, Clone, Debug)]
pub struct GroundFile {
    pub size: f64,
    pub origin: [i32; 2],
    pub dims: [i32; 2],
    pub tops: Vec<i32>,
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
        let (i, j) = ((x / g.size).floor() as i32, (y / g.size).floor() as i32);
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

/// The generated cover kind (ground_cover data id) that stands in for a village model, if any.
fn cover_species(name: &str) -> Option<&'static str> {
    let kind = name.rsplit_once('_').map_or(name, |(k, _)| k);
    Some(match kind {
        "oak" => "oak",
        "birch" => "birch",
        "pine" => "pine",
        "bush" => "shrub",
        "flowers" => "flowers",
        "mushrooms" => "mushrooms",
        "pebble" => "pebbles",
        "rock" => "rock",
        "stump" => "stump",
        k if k.starts_with("tuft") => "grass",
        _ => return None,
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
    // The village's ground: its layout (the game's own, content/data/village_layouts) laid on
    // the planet's real ground at the game's block size.
    let layout_path = crate::universe::content_dir()
        .ok_or("content folder not found")?
        .join("data/village_layouts/demo_village.json");
    let own: sim_planet::VillageLayout = serde_json::from_str(&read(&layout_path)?)
        .map_err(|e| format!("{}: {e}", layout_path.display()))?;
    let mut ground =
        BlockGround::new(current.clone(), center, Some(own)).with_style(ground_style());
    // Ground cover planet-wide (design/ground-cover.md) on the faceted ground of a body with
    // geography; `ULTRA_SPACE_COVER=0` leaves it out, `ULTRA_SPACE_COVER_DENSITY` scales it.
    if ground_style() == sim_planet::GroundStyle::Faceted
        && current.globe().is_some()
        && std::env::var("ULTRA_SPACE_COVER").as_deref() != Ok("0")
    {
        let density = std::env::var("ULTRA_SPACE_COVER_DENSITY")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(1.0);
        let started = std::time::Instant::now();
        let seed = system.bodies[body].terrain.as_ref().map_or(0, |t| t.seed);
        let cover = sim_planet::cover::CoverWorld::new(seed, density);
        // The village's trees, bushes and grass become the generated kinds (they sway, shade
        // and collide like the wild cover); its own models of them aren't drawn.
        let placed = village
            .placements
            .iter()
            .filter_map(|p| {
                let species = cover.species_index(cover_species(&p.model)?)?;
                Some(sim_planet::cover::Placed {
                    species: species as u16,
                    face: 0,
                    at: DVec2::new(p.pos[0], p.pos[1]),
                    yaw: p.rot.to_radians(),
                    scale: p.scale,
                })
            })
            .collect::<Vec<_>>();
        let cover = Arc::new(cover.with_placed(placed));
        info!(
            "ground cover: {} species built in {:.0} ms",
            cover.species_count(),
            started.elapsed().as_secs_f64() * 1e3
        );
        ground = ground.with_cover(cover);
    }
    let world = BlockWorld {
        ground: Arc::new(
            BlockGround::new(current.clone(), center, None).with_style(ground_style()),
        ),
        body,
        village: Arc::new(village),
        models: Arc::new(models),
        palette: Arc::new(palette),
    };
    // Collision boxes: buildings and props whole, trees by their trunks; none where the
    // player's ship lands (it is bigger than the block ship the layout was made for).
    let ship_at = DVec3::from_array(world.village.ship.pos);
    let covered = ground.cover.is_some();
    for p in &world.village.placements {
        if covered && cover_species(&p.model).is_some() {
            continue;
        }
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
        // Temperate green ground all round (the village's ground eases into what grows there;
        // not the tropical dry forest, whose palms don't belong by a medieval house).
        use sim_planet::globe::Cover;
        let green = |d: DVec3| {
            matches!(
                globe.ground_cover(d, centre, 0.0),
                Cover::Grass | Cover::Forest
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
        // Woods round the village (oak, beech and birch), as in the style renders.
        let woods = [
            (400.0, 0.0),
            (-400.0, 0.0),
            (0.0, 400.0),
            (0.0, -400.0),
            (280.0, 280.0),
            (-280.0, -280.0),
        ]
        .iter()
        .filter(|(x, z)| {
            globe.ground_cover((dir * r + east * *x + south * *z).normalize(), centre, 0.0)
                == Cover::Forest
        })
        .count() as f64;
        let score = hi - lo + 2.0 * (m - 0.55).abs() + 0.05 * (t - 15.0).abs() + 10.0 * margin
            - 1.5 * woods;
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
/// (ms), its stages (heights, cells and colours, meshing; ms) and its ground cover (in the same
/// frame).
type MadeChunk = (Mesh, DVec3, DQuat, f64, [f32; 3], Option<Option<Mesh>>);

/// A chunk's ground cover, being made or made (its entity; none when bare).
enum CoverSlot {
    /// Not started (or the body has no cover).
    Waiting,
    Making(Task<Option<Mesh>>),
    Made(Option<Entity>),
}

/// A chunk made: its ground entity (none where there is nothing), whether that is faceted
/// ground (it dissolves), its cover, its triangles, and whether it is shown.
struct Made {
    entity: Option<Entity>,
    faceted: bool,
    cover: CoverSlot,
    triangles: usize,
    shown: bool,
}

impl Made {
    /// Whether it can take over from the chunks it replaces (its cover made too, if any).
    fn ready(&self, with_cover: bool) -> bool {
        !with_cover || self.entity.is_none() || matches!(self.cover, CoverSlot::Made(_))
    }

    /// Its ground entity when that dissolves, and its cover entity.
    fn parts(&self) -> (Option<Entity>, Option<Entity>) {
        let cover = match self.cover {
            CoverSlot::Made(e) => e,
            _ => None,
        };
        (self.entity.filter(|_| self.faceted), cover)
    }
}

/// A dissolve: out (the chunk goes to the cache when done) or in (its parts get the steady
/// materials back when done, so their shaders drop the discard again).
struct Fading {
    key: LodKey,
    start: f64,
    steady: Option<(Option<Entity>, Option<Entity>)>,
}

/// How a chunk is drawn.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Look {
    Steady,
    In,
    Out,
}

/// The dissolve materials started at one clock time (the dissolves begun in one frame share
/// them; the shaders work out the progress from the clock): in and out, the cover near and far.
struct FadeSet {
    clock: f32,
    ground: [Handle<GroundMaterial>; 2],
    cover: [[Handle<CoverMaterial>; 2]; 2],
}

/// A cover material: dissolving from `start` in `mode` (+1 in, -1 out, 0 steady), thinning out
/// towards `far` (m; 0 none).
fn cover_material(start: f32, mode: f32, far: f32) -> CoverMaterial {
    CoverMaterial {
        base: StandardMaterial {
            base_color: Color::WHITE,
            perceptual_roughness: 0.8,
            reflectance: 0.2,
            ..default()
        },
        extension: CoverFade {
            fade: Vec4::new(start, mode, far, FADE as f32),
        },
    }
}

/// Gives a chunk's parts the shared materials for `look`. Few materials in all (steady ones,
/// and one set per frame that starts dissolves): a material per chunk cost a quarter of the
/// frame rate (q0126).
#[allow(clippy::too_many_arguments)]
fn set_look(
    commands: &mut Commands,
    draw: &mut BlockDraw,
    ground_materials: &mut Assets<GroundMaterial>,
    cover_materials: &mut Assets<CoverMaterial>,
    clock: f32,
    key: LodKey,
    (ground, cover): (Option<Entity>, Option<Entity>),
    look: Look,
) {
    // Thinning out only where the cover ends (its coarsest two levels).
    let far_i = usize::from(key.0 + 1 >= TOP_LEVEL);
    let (g, c) = match look {
        Look::Steady => (
            draw.ground_material.clone(),
            draw.cover_steady[far_i].clone(),
        ),
        Look::In | Look::Out => {
            if draw.fades.as_ref().is_none_or(|f| f.clock != clock) {
                let template = ground_materials.get(&draw.ground_material).cloned();
                let mut ground_for = |mode: f32| {
                    let mut m = template.clone().unwrap_or_else(|| GroundMaterial {
                        base: StandardMaterial::default(),
                        extension: GroundDetail::default(),
                    });
                    m.extension.fade = Vec4::new(clock, mode, 0.0, FADE as f32);
                    ground_materials.add(m)
                };
                let ground = [ground_for(1.0), ground_for(-1.0)];
                let far = draw.far;
                let mut cover_for =
                    |mode: f32, f: f32| cover_materials.add(cover_material(clock, mode, f));
                let cover = [
                    [cover_for(1.0, 0.0), cover_for(1.0, far)],
                    [cover_for(-1.0, 0.0), cover_for(-1.0, far)],
                ];
                draw.fades = Some(FadeSet {
                    clock,
                    ground,
                    cover,
                });
            }
            let set = draw.fades.as_ref().expect("made above");
            let i = usize::from(look == Look::Out);
            (set.ground[i].clone(), set.cover[i][far_i].clone())
        }
    };
    if let Some(e) = ground {
        commands.entity(e).try_insert(MeshMaterial3d(g));
    }
    if let Some(e) = cover {
        commands.entity(e).try_insert(MeshMaterial3d(c));
    }
}

/// The camera's motion, smoothed (surface frame): for ordering the work by when each chunk
/// will be seen, whatever moves the camera.
#[derive(Default)]
struct Motion {
    at: Option<DVec3>,
    forward: DVec3,
    velocity: DVec3,
    /// Angular velocity of the view direction (rad/s, axis).
    turn: DVec3,
    time: f64,
}

/// A chunk's ground cover as a Bevy mesh (vertex colours final).
fn cover_mesh(c: sim_planet::cover::CoverMesh) -> Mesh {
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, c.positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, c.normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, c.colours)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, c.seeds)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_1, c.wind)
    .with_inserted_indices(Indices::U32(c.indices))
}

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
    frames: usize,
    late_frames: usize,
    late_max: usize,
    waits: usize,
    wait_sum: f64,
    wait_max: f64,
}

/// What is drawn: the root, chunks shown and being made, the materials.
#[derive(Resource)]
struct BlockDraw {
    root: Entity,
    /// Chunks wanted and made (shown, or hidden until they take over from the chunks they
    /// replace: old and new dissolve into each other).
    chunks: HashMap<LodKey, Made>,
    /// Chunks gone or built ahead, hidden, kept for turning back (least recently used go).
    cache: HashMap<LodKey, (Made, f64)>,
    cache_triangles: usize,
    /// Chunks being made: wanted ones, and ones built ahead along the camera's path.
    pending: HashMap<LodKey, Task<Option<MadeChunk>>>,
    fading: Vec<Fading>,
    /// Old chunks dissolving out (their entities go to the cache when done).
    leaving: HashMap<LodKey, Made>,
    solid: Handle<StandardMaterial>,
    chunk_material: Handle<StandardMaterial>,
    ground_material: Handle<GroundMaterial>,
    /// The cover's steady materials (near, and thinning out far), and the far distance.
    cover_steady: [Handle<CoverMaterial>; 2],
    far: f32,
    /// The dissolve materials of the latest frame that started any.
    fades: Option<FadeSet>,
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

/// The faceted ground's material: the standard one with procedural detail (shaders/ground.wesl).
pub type GroundMaterial = ExtendedMaterial<StandardMaterial, GroundDetail>;

/// Whether a ground or cover material is dissolving (or thinning): only then are its shaders
/// built with the discard (shader def `FADING`); steady ones keep the GPU's early depth test.
#[repr(C)]
#[derive(Eq, PartialEq, Hash, Copy, Clone)]
pub struct FadeKey {
    fading: bool,
}

impl From<&GroundDetail> for FadeKey {
    fn from(m: &GroundDetail) -> Self {
        Self {
            fading: m.fade.y != 0.0,
        }
    }
}

impl From<&CoverFade> for FadeKey {
    fn from(m: &CoverFade) -> Self {
        Self {
            fading: m.fade.y != 0.0 || m.fade.z > 0.0,
        }
    }
}

/// Adds `FADING` to a fading material's shaders.
fn fading_def(
    descriptor: &mut bevy::render::render_resource::RenderPipelineDescriptor,
    fading: bool,
) {
    if fading && let Some(f) = descriptor.fragment.as_mut() {
        f.shader_defs.push("FADING".into());
    }
}

/// The ground detail's settings.
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone, Default)]
#[bind_group_data(FadeKey)]
pub struct GroundDetail {
    /// x: pattern strength (0 none, 1 default); y: the radius round the camera within which the
    /// chunks are drawn and the far tiles hidden (m; 0 everywhere).
    #[uniform(100)]
    pub strength: Vec4,
    /// xyz: the body's centre (render space).
    #[uniform(100)]
    pub centre: Vec4,
    /// x dissolve progress 0..1, y mode (+1 fading in, -1 fading out, 0 steady).
    #[uniform(100)]
    pub fade: Vec4,
}

/// The ground cover's material: the standard one with vertex colours, dissolved in and out with
/// its chunk thing by thing and thinned out far away (shaders/cover.wesl).
pub type CoverMaterial = ExtendedMaterial<StandardMaterial, CoverFade>;

/// The cover's dissolve settings.
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone, Default)]
#[bind_group_data(FadeKey)]
pub struct CoverFade {
    /// x progress 0..1, y mode (+1 in, -1 out, 0 steady), z the distance things thin out
    /// towards (m; 0 none).
    #[uniform(100)]
    pub fade: Vec4,
}

impl MaterialExtension for CoverFade {
    fn vertex_shader() -> ShaderRef {
        "embedded://ultra_space/shaders/cover.wesl".into()
    }

    fn prepass_vertex_shader() -> ShaderRef {
        "embedded://ultra_space/shaders/cover_prepass.wesl".into()
    }

    fn fragment_shader() -> ShaderRef {
        "embedded://ultra_space/shaders/cover.wesl".into()
    }

    fn prepass_fragment_shader() -> ShaderRef {
        "embedded://ultra_space/shaders/cover_prepass.wesl".into()
    }

    fn specialize(
        _pipeline: &bevy::pbr::MaterialExtensionPipeline,
        descriptor: &mut bevy::render::render_resource::RenderPipelineDescriptor,
        _layout: &bevy::mesh::MeshVertexBufferLayoutRef,
        key: bevy::pbr::MaterialExtensionKey<Self>,
    ) -> Result<(), bevy::render::render_resource::SpecializedMeshPipelineError> {
        fading_def(descriptor, key.bind_group_data.fading);
        Ok(())
    }
}

impl MaterialExtension for GroundDetail {
    fn fragment_shader() -> ShaderRef {
        GROUND_SHADER.into()
    }

    fn prepass_fragment_shader() -> ShaderRef {
        GROUND_PREPASS_SHADER.into()
    }

    fn specialize(
        _pipeline: &bevy::pbr::MaterialExtensionPipeline,
        descriptor: &mut bevy::render::render_resource::RenderPipelineDescriptor,
        _layout: &bevy::mesh::MeshVertexBufferLayoutRef,
        key: bevy::pbr::MaterialExtensionKey<Self>,
    ) -> Result<(), bevy::render::render_resource::SpecializedMeshPipelineError> {
        fading_def(descriptor, key.bind_group_data.fading);
        Ok(())
    }
}

const GROUND_PREPASS_SHADER: &str = "embedded://ultra_space/shaders/ground_prepass.wesl";

const GROUND_SHADER: &str = "embedded://ultra_space/shaders/ground.wesl";

/// The detail class of a ground colour (shaders/ground.wesl): 0 grass, 1 rock, 2 sand and
/// paths, 3 earth, 4 snow.
fn ground_class(name: &str) -> f32 {
    match name {
        "rock" | "rock2" | "stone" | "stone2" | "bedrock" | "bedrock2" | "ore" => 1.0,
        "straw" | "straw2" | "sand" | "path" | "path2" => 2.0,
        "soil" | "soil2" | "tilled" => 3.0,
        "white" => 4.0,
        _ => 0.0,
    }
}

/// A faceted ground chunk's mesh: colours, and per vertex its detail class and height (UV 0)
/// and point on the face grid (UV 1).
fn ground_mesh(c: &sim_planet::blocks::ChunkMesh, colour: impl Fn(u16) -> [f32; 4]) -> Mesh {
    let uv0: Vec<[f32; 2]> = c
        .mesh
        .colours
        .iter()
        .zip(&c.ground)
        .map(|(&k, g)| {
            [
                ground_class(sim_planet::blocks::PALETTE[k as usize - 1]),
                g[2],
            ]
        })
        .collect();
    let uv1: Vec<[f32; 2]> = c.ground.iter().map(|g| [g[0], g[1]]).collect();
    bevy_mesh(&c.mesh, colour)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv0)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_1, uv1)
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
    mut ground_materials: ResMut<Assets<GroundMaterial>>,
    mut cover_materials: ResMut<Assets<CoverMaterial>>,
) {
    let Some(world) = world else { return };
    // The faceted ground: matte, with its detail patterns (`ULTRA_SPACE_GROUND_DETAIL=0` turns
    // them off).
    let detail = std::env::var("ULTRA_SPACE_GROUND_DETAIL")
        .ok()
        .and_then(|v| v.parse::<f32>().ok())
        .unwrap_or(1.0);
    let ground_material = ground_materials.add(GroundMaterial {
        base: StandardMaterial {
            base_color: Color::WHITE,
            perceptual_roughness: 0.9,
            reflectance: 0.15,
            ..default()
        },
        extension: GroundDetail {
            strength: Vec4::new(detail, 0.0, 0.0, 0.0),
            centre: Vec4::ZERO,
            fade: Vec4::ZERO,
        },
    });
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
        if world.ground.cover.is_some() && cover_species(&p.model).is_some() {
            continue;
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
        cache: HashMap::new(),
        cache_triangles: 0,
        pending: HashMap::new(),
        fading: Vec::new(),
        leaving: HashMap::new(),
        solid,
        chunk_material,
        ground_material,
        cover_steady: [
            cover_materials.add(cover_material(0.0, 0.0, 0.0)),
            cover_materials.add(cover_material(0.0, 0.0, 1.0e6)),
        ],
        far: 1.0e6,
        fades: None,
        glow,
        windows,
        lights,
    });
}

/// The streamer's own state across frames.
#[derive(Default)]
struct StreamState {
    plan: ChunkPlan,
    /// The chunks the camera will want a moment ahead (built ahead into the cache).
    ahead: ChunkPlan,
    stats: StreamStats,
    motion: Motion,
    /// When each wanted chunk not yet shown was first wanted (s).
    since: HashMap<LodKey, f64>,
    last_count: usize,
}

/// The streaming worker pool: a fifth of the logical cores (the generation pool has half, and
/// the main and render threads must never starve: all but four cost half the frame rate in
/// q0123, ten threads 10 % more than six in q0126). `ULTRA_SPACE_STREAM_THREADS` overrides. Few jobs are handed to it at once, so the most urgent chunk is always next.
fn streaming() -> &'static bevy::tasks::TaskPool {
    static POOL: std::sync::OnceLock<bevy::tasks::TaskPool> = std::sync::OnceLock::new();
    POOL.get_or_init(|| {
        let cores = std::thread::available_parallelism().map_or(4, |n| n.get());
        bevy::tasks::TaskPoolBuilder::new()
            .num_threads(
                std::env::var("ULTRA_SPACE_STREAM_THREADS")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or((cores / 5).max(2)),
            )
            .thread_name("streaming".into())
            .build()
    })
}

/// Whether two chunks of one face overlap (one contains the other).
fn overlaps(a: &LodKey, b: &LodKey) -> bool {
    if a.1 != b.1 {
        return false;
    }
    let (hi, lo) = if a.0 >= b.0 { (a, b) } else { (b, a) };
    let shift = i32::from(hi.0 - lo.0);
    lo.2 >> shift == hi.2 && lo.3 >> shift == hi.3
}

/// A chunk's cover entity: a child of its ground in the same frame, with `material`, casting
/// shadows only near.
fn spawn_cover(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    parent: Entity,
    key: LodKey,
    mesh: Mesh,
    material: Handle<CoverMaterial>,
) -> Entity {
    let mut e = commands.spawn((
        Mesh3d(meshes.add(mesh)),
        MeshMaterial3d(material),
        Transform::IDENTITY,
        Visibility::Inherited,
        ChildOf(parent),
    ));
    if key.0 >= 2 {
        e.insert(bevy::light::NotShadowCaster);
    }
    e.id()
}

/// Puts a chunk away hidden (its cover being made is dropped), dropping the least recently
/// used ones over the budget.
fn cache_put(draw: &mut BlockDraw, commands: &mut Commands, key: LodKey, mut made: Made, now: f64) {
    if let Some(e) = made.entity {
        commands.entity(e).insert(Visibility::Hidden);
    }
    if matches!(made.cover, CoverSlot::Making(_)) {
        made.cover = CoverSlot::Waiting;
    }
    made.shown = false;
    draw.fading.retain(|f| f.key != key);
    if let Some((old, _)) = draw.cache.remove(&key) {
        draw.cache_triangles -= old.triangles;
        if let Some(e) = old.entity {
            commands.entity(e).despawn();
        }
    }
    draw.cache_triangles += made.triangles;
    draw.cache.insert(key, (made, now));
    while draw.cache_triangles > CACHE_TRIANGLES {
        let Some(oldest) = draw
            .cache
            .iter()
            .min_by(|a, b| a.1.1.total_cmp(&b.1.1))
            .map(|(k, _)| *k)
        else {
            break;
        };
        if let Some((m, _)) = draw.cache.remove(&oldest) {
            draw.cache_triangles -= m.triangles;
            if let Some(e) = m.entity {
                commands.entity(e).despawn();
            }
        }
    }
}

/// Chunks within reach of the camera are made on workers and shown; far ones go. The work is
/// ordered by how soon each chunk will be seen (the camera's motion, whatever moves it), each
/// chunk's ground before its cover; chunks a moment ahead are built into a cache, and chunks
/// left behind are kept there for turning back. Replacements dissolve into what they replace.
#[allow(clippy::too_many_arguments)]
fn stream_chunks(
    mut commands: Commands,
    world: Option<Res<BlockWorld>>,
    draw: Option<ResMut<BlockDraw>>,
    universe: Res<Universe>,
    render: Res<RenderTime>,
    camera: Res<SpaceCamera>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut ground_materials: ResMut<Assets<GroundMaterial>>,
    mut cover_materials: ResMut<Assets<CoverMaterial>>,
    mut coverage: ResMut<BlockCoverage>,
    mut state: Local<StreamState>,
    real: Res<Time<Real>>,
    projections: Query<&Projection, With<crate::space_render::MainCamera>>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    views: Query<&GlobalTransform, With<crate::space_render::MainCamera>>,
    time: Res<Time>,
) {
    let _slow = crate::debug_overlay::SlowSystem::new("stream_chunks");
    let (Some(world), Some(mut draw)) = (world, draw) else {
        return;
    };
    let cam = crate::settlements::camera_on(&universe, &camera, world.body, render.0);
    let ground = &world.ground;
    // Height above the ground under the camera (not above the village's floor: on high land
    // that would leave the ground without chunks).
    let height = {
        let (face, p) = ground.locate(cam);
        cam.length() - ground.radius - ground.facet_surface(face, p)
    };
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
    let state = &mut *state;
    let plan = &mut state.plan;
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
            let centres = &mut plan.centres;
            plan.wanted = ground.lod_plan(
                cam,
                TOP_LEVEL,
                |level| level_reach(level, per_radian, pixels),
                |k| near(centres, &k),
            );
        }
    }
    // A copy (a few hundred keys) so the plan's centres stay usable below.
    let wanted_set = plan.wanted.clone();
    let wanted = &wanted_set;
    let draw = &mut *draw;
    let now = real.elapsed_secs_f64();
    // The clock the dissolve shaders read.
    let clock = time.elapsed_secs_wrapped();
    let with_cover = ground.cover.is_some();
    // The cover thins out towards the edge of what is drawn (kept up to date in its steady
    // material; dissolve materials take it when made).
    let far = (outer * 0.92) as f32;
    if (draw.far - far).abs() > 0.01 * far {
        draw.far = far;
        if let Some(mut m) = cover_materials.get_mut(&draw.cover_steady[1]) {
            m.extension.fade.z = far;
        }
    }
    // The camera's motion (surface frame), smoothed over a fraction of a second.
    let system = &universe.0;
    let anchor = system.bodies[camera.anchor].inertial_frame;
    let frame = system
        .frames
        .state_in(anchor, system.bodies[world.body].surface_frame, render.0);
    let forward = views.iter().next().map_or(DVec3::ZERO, |t| {
        frame.rotation.inverse() * t.forward().as_vec3().as_dvec3()
    });
    {
        let m = &mut state.motion;
        let dt = now - m.time;
        if let Some(at) = m.at
            && dt > 1e-4
            && dt < 1.0
        {
            let k = 1.0 - (-dt / 0.3).exp();
            let v = (cam - at) / dt;
            // A jump (a new region, a teleport) is not motion.
            let v = if v.length() > 20_000.0 {
                DVec3::ZERO
            } else {
                v
            };
            m.velocity = m.velocity.lerp(v, k);
            m.turn = m.turn.lerp(m.forward.cross(forward) / dt, k);
        }
        m.at = Some(cam);
        m.forward = forward;
        m.time = now;
    }
    let ahead = cam + state.motion.velocity * LOOK_AHEAD;
    let turned = DQuat::from_scaled_axis(state.motion.turn * 0.4) * forward;
    let aspect = windows.iter().next().map_or(1.6, |w| {
        f64::from(w.width()) / f64::from(w.height()).max(1.0)
    });
    // Half the view's width, with a margin.
    let half_view = ((fov * 0.5).tan() * aspect).atan() + 0.2;
    let in_view = |c: DVec3, radius: f64| {
        let to = c - cam;
        let d = to.length();
        if d < radius + 30.0 || forward == DVec3::ZERO {
            return true;
        }
        let dir = to / d;
        let slack = (radius / d).min(1.0).asin();
        dir.dot(forward).acos().min(dir.dot(turned).acos()) < half_view + slack
    };
    // Drawn ground over a chunk (shown or still dissolving out).
    let covered = |draw: &BlockDraw, k: &LodKey| {
        draw.chunks
            .iter()
            .any(|(o, m)| m.shown && m.entity.is_some() && overlaps(o, k))
            || draw.leaving.keys().any(|o| overlaps(o, k))
    };
    // How soon a chunk is needed (lower first): its distance from the camera now or from where
    // the camera is heading, three times as far when out of view, a quarter where nothing is
    // drawn (a hole).
    let urgency = |draw: &BlockDraw, centres: &mut HashMap<LodKey, DVec3>, k: &LodKey| {
        let c = *centres
            .entry(*k)
            .or_insert_with(|| ground.lod_centre_point(*k));
        let r = BlockGround::lod_size(k.0) * std::f64::consts::FRAC_1_SQRT_2;
        let d = (c.distance(cam) - r).min(c.distance(ahead) - r).max(0.0);
        let view = if in_view(c, r) { 1.0 } else { 3.0 };
        let hole = if covered(draw, k) { 1.0 } else { 0.25 };
        d * view * hole
    };
    // Wanted chunks kept from before are taken back at once (from the cache, or back from
    // dissolving out).
    let back: Vec<LodKey> = wanted
        .iter()
        .filter(|k| {
            !draw.chunks.contains_key(k)
                && (draw.cache.contains_key(k) || draw.leaving.contains_key(k))
        })
        .copied()
        .collect();
    for key in back {
        let made = if let Some(m) = draw.leaving.remove(&key) {
            draw.fading.retain(|f| f.key != key);
            if let Some(e) = m.entity {
                commands.entity(e).insert(Visibility::Hidden);
            }
            m
        } else {
            let Some((m, _)) = draw.cache.remove(&key) else {
                continue;
            };
            draw.cache_triangles -= m.triangles;
            m
        };
        draw.chunks.insert(
            key,
            Made {
                shown: false,
                ..made
            },
        );
    }
    // The chunks a moment ahead (when moving): built into the cache, after the wanted ones.
    // (`ULTRA_SPACE_STREAM_AHEAD=0` builds nothing ahead: for measuring.)
    let moving = state.motion.velocity.length() > 3.0
        && height < outer
        && std::env::var("ULTRA_SPACE_STREAM_AHEAD").as_deref() != Ok("0");
    if moving {
        let a = &mut state.ahead;
        if a.camera.is_none_or(|c| c.distance(ahead) > 5.0) {
            a.camera = Some(ahead);
            let centres = &mut a.centres;
            if centres.len() > 400_000 {
                centres.clear();
            }
            a.wanted = ground.lod_plan(
                ahead,
                TOP_LEVEL,
                |level| level_reach(level, per_radian, pixels),
                |k| {
                    let c = *centres
                        .entry(k)
                        .or_insert_with(|| ground.lod_centre_point(k));
                    c.distance(ahead) - BlockGround::lod_size(k.0) * std::f64::consts::FRAC_1_SQRT_2
                },
            );
        }
    } else {
        state.ahead.wanted.clear();
        state.ahead.camera = None;
    }
    let centres = &mut state.plan.centres;
    // The work to do, most urgent first: ground of wanted chunks, their cover, chunks ahead.
    enum Job {
        Ground(LodKey),
        Cover(LodKey),
        Ahead(LodKey),
    }
    let mut jobs: Vec<(f64, Job)> = Vec::new();
    let mut urgent = false;
    for k in wanted.iter() {
        if draw.chunks.contains_key(k) || draw.pending.contains_key(k) {
            continue;
        }
        let u = urgency(draw, centres, k);
        urgent |= u < 2.0 * BlockGround::lod_size(k.0);
        jobs.push((u, Job::Ground(*k)));
    }
    for (k, m) in &draw.chunks {
        if with_cover && m.entity.is_some() && matches!(m.cover, CoverSlot::Waiting) {
            jobs.push((urgency(draw, centres, k) * 1.2 + 10.0, Job::Cover(*k)));
        }
    }
    for k in state.ahead.wanted.iter() {
        if wanted.contains(k)
            || draw.cache.contains_key(k)
            || draw.pending.contains_key(k)
            || draw.chunks.contains_key(k)
        {
            continue;
        }
        jobs.push((urgency(draw, centres, k) * 1.5 + 30.0, Job::Ahead(*k)));
    }
    jobs.sort_by(|a, b| a.0.total_cmp(&b.0));
    let pool = streaming();
    let threads = pool.thread_num();
    let making_cover = draw
        .chunks
        .values()
        .filter(|m| matches!(m.cover, CoverSlot::Making(_)))
        .count();
    let cap = if urgent {
        threads + 2
    } else {
        (threads / 2).max(2)
    };
    let free = cap.saturating_sub(draw.pending.len() + making_cover);
    for (_, job) in jobs.into_iter().take(free) {
        let g = world.ground.clone();
        match job {
            Job::Cover(key) => {
                let task = pool.spawn(async move {
                    g.cover
                        .as_ref()
                        .and_then(|cw| cw.chunk_mesh(&g, key))
                        .map(cover_mesh)
                });
                if let Some(m) = draw.chunks.get_mut(&key) {
                    m.cover = CoverSlot::Making(task);
                }
            }
            Job::Ground(key) | Job::Ahead(key) => {
                let with = matches!(job, Job::Ahead(_));
                let palette = world.palette.clone();
                draw.pending.insert(
                    key,
                    pool.spawn(async move {
                        // The Bevy mesh is made here too, off the main thread.
                        let start = std::time::Instant::now();
                        g.lod_chunk(key).map(|c| {
                            let colour = |i: u16| {
                                let [r, g, b] =
                                    palette.linear(sim_planet::blocks::PALETTE[i as usize - 1]);
                                [r, g, b, 1.0]
                            };
                            let mesh = if c.ground.is_empty() {
                                bevy_mesh(&c.mesh, colour)
                            } else {
                                ground_mesh(&c, colour)
                            };
                            // Built ahead: the cover with it (both go to the cache).
                            let cover = with.then(|| {
                                g.cover
                                    .as_ref()
                                    .and_then(|cw| cw.chunk_mesh(&g, key))
                                    .map(cover_mesh)
                            });
                            (
                                mesh,
                                c.origin,
                                c.basis,
                                start.elapsed().as_secs_f64() * 1e3,
                                c.timings,
                                cover,
                            )
                        })
                    }),
                );
            }
        }
    }
    // Finished ground, a few a frame (each is a mesh upload).
    let done: Vec<LodKey> = draw
        .pending
        .iter_mut()
        .filter_map(|(k, t)| t.is_finished().then_some(*k))
        .take(CHUNKS_PER_FRAME)
        .collect();
    let mut uploaded = 0usize;
    let stats = &mut state.stats;
    for key in done {
        if uploaded >= TRIANGLES_PER_FRAME {
            break; // the rest next frame
        }
        let Some(task) = draw.pending.remove(&key) else {
            continue;
        };
        let m = block_on(poll_once(task)).flatten();
        let mut made = Made {
            entity: None,
            faceted: false,
            cover: CoverSlot::Made(None),
            triangles: 0,
            shown: false,
        };
        match m {
            Some((mesh, origin, basis, ms, stages, cover)) => {
                for (sum, t) in stats.stages.iter_mut().zip(stages) {
                    *sum += f64::from(t);
                }
                stats.built += 1;
                stats.work_ms += ms;
                stats.worst_ms = stats.worst_ms.max(ms);
                let mut triangles = mesh.indices().map_or(0, |i| i.len() / 3);
                let faceted = mesh.attribute(Mesh::ATTRIBUTE_UV_1).is_some();
                let mut e = commands.spawn((
                    ChunkPose { origin, basis },
                    Mesh3d(meshes.add(mesh)),
                    Transform::default(),
                    Visibility::Hidden,
                ));
                if faceted {
                    e.insert(MeshMaterial3d(draw.ground_material.clone()));
                    made.faceted = true;
                } else {
                    e.insert(MeshMaterial3d(draw.chunk_material.clone()));
                }
                let id = e.id();
                made.entity = Some(id);
                made.cover = match cover {
                    Some(Some(c)) => {
                        triangles += c.indices().map_or(0, |i| i.len() / 3);
                        let material =
                            draw.cover_steady[usize::from(key.0 + 1 >= TOP_LEVEL)].clone();
                        let child = spawn_cover(&mut commands, &mut meshes, id, key, c, material);
                        CoverSlot::Made(Some(child))
                    }
                    Some(None) => CoverSlot::Made(None),
                    None if with_cover => CoverSlot::Waiting,
                    None => CoverSlot::Made(None),
                };
                made.triangles = triangles;
                stats.triangles += triangles;
                uploaded += triangles;
            }
            None => stats.empty += 1,
        }
        if wanted.contains(&key) {
            draw.chunks.insert(key, made);
        } else {
            cache_put(draw, &mut commands, key, made, now);
        }
    }
    // Finished cover, within the same upload budget.
    let finished: Vec<LodKey> = draw
        .chunks
        .iter_mut()
        .filter_map(|(k, m)| match &mut m.cover {
            CoverSlot::Making(t) if t.is_finished() => Some(*k),
            _ => None,
        })
        .collect();
    for key in finished {
        if uploaded >= TRIANGLES_PER_FRAME {
            break;
        }
        let Some(m) = draw.chunks.get_mut(&key) else {
            continue;
        };
        let CoverSlot::Making(task) = std::mem::replace(&mut m.cover, CoverSlot::Made(None)) else {
            continue;
        };
        let Some(mesh) = block_on(poll_once(task)).flatten() else {
            continue;
        };
        let Some(parent) = m.entity else { continue };
        let triangles = mesh.indices().map_or(0, |i| i.len() / 3);
        m.triangles += triangles;
        stats.triangles += triangles;
        uploaded += triangles;
        let shown = m.shown;
        let steady = draw.cover_steady[usize::from(key.0 + 1 >= TOP_LEVEL)].clone();
        let child = spawn_cover(&mut commands, &mut meshes, parent, key, mesh, steady);
        if let Some(m) = draw.chunks.get_mut(&key) {
            m.cover = CoverSlot::Made(Some(child));
        }
        // Over a chunk already shown, the trees dissolve in one by one.
        if shown {
            set_look(
                &mut commands,
                draw,
                &mut ground_materials,
                &mut cover_materials,
                clock,
                key,
                (None, Some(child)),
                Look::In,
            );
            draw.fading.push(Fading {
                key,
                start: now,
                steady: Some((None, Some(child))),
            });
        }
    }
    // The ground is ready out to the nearest wanted chunk not yet made whose ground no chunk
    // drawn covers either (chunks being remade over shown ground don't count, or the disc would
    // shrink and grow all the time as the camera moves).
    let ready = wanted
        .iter()
        .filter(|k| !draw.chunks.contains_key(k) && !covered(draw, k))
        .map(|k| {
            let c = *centres
                .entry(*k)
                .or_insert_with(|| ground.lod_centre_point(*k));
            c.distance(cam) - BlockGround::lod_size(k.0) * std::f64::consts::FRAC_1_SQRT_2
        })
        .fold(outer, f64::min)
        .max(0.0);
    coverage.0 = if height < outer && std::env::var_os("ULTRA_SPACE_DEBUG_NO_HIDE").is_none() {
        ready
    } else {
        0.0
    };
    // Chunks no longer wanted go once every wanted chunk overlapping them is ready (a parent
    // stays until all its children are, children until their parent is), or at once when they
    // cover nothing wanted: shown ones dissolve out, the rest go straight to the cache.
    let gone: Vec<LodKey> = draw
        .chunks
        .keys()
        .filter(|k| !wanted.contains(k))
        .filter(|k| {
            wanted
                .iter()
                .filter(|w| overlaps(k, w))
                .all(|w| draw.chunks.get(w).is_some_and(|m| m.ready(with_cover)))
        })
        .copied()
        .collect();
    for key in &gone {
        let Some(m) = draw.chunks.remove(key) else {
            continue;
        };
        if m.shown && m.entity.is_some() {
            draw.fading.retain(|f| f.key != *key);
            set_look(
                &mut commands,
                draw,
                &mut ground_materials,
                &mut cover_materials,
                clock,
                *key,
                m.parts(),
                Look::Out,
            );
            draw.fading.push(Fading {
                key: *key,
                start: now,
                steady: None,
            });
            draw.leaving.insert(*key, m);
        } else {
            cache_put(draw, &mut commands, *key, m, now);
        }
    }
    // Hidden chunks show once no old chunk still listed overlaps them (the old ones went above,
    // in the same frame, once all their replacements were ready): they dissolve in over what
    // dissolves out, or over the far tiles; near the camera over bare ground at once.
    let old: Vec<LodKey> = draw
        .chunks
        .iter()
        .filter(|(k, m)| m.entity.is_some() && m.shown && !wanted.contains(k))
        .map(|(k, _)| *k)
        .collect();
    let show: Vec<LodKey> = draw
        .chunks
        .iter()
        .filter(|(k, m)| !m.shown && !old.iter().any(|o| overlaps(o, k)))
        .map(|(k, _)| *k)
        .collect();
    for key in show {
        let replacing = gone.iter().any(|g| overlaps(g, &key));
        let c = *centres
            .entry(key)
            .or_insert_with(|| ground.lod_centre_point(key));
        let near = c.distance(cam) - BlockGround::lod_size(key.0) * std::f64::consts::FRAC_1_SQRT_2;
        if let Some(waited) = state.since.remove(&key) {
            let w = now - waited;
            stats.waits += 1;
            stats.wait_sum += w;
            stats.wait_max = stats.wait_max.max(w);
        }
        let Some(m) = draw.chunks.get_mut(&key) else {
            continue;
        };
        m.shown = true;
        let Some(e) = m.entity else { continue };
        commands.entity(e).insert(Visibility::Visible);
        let parts = m.parts();
        draw.fading.retain(|f| f.key != key);
        let look = if replacing || near > SHOW_AT_ONCE {
            Look::In
        } else {
            Look::Steady
        };
        set_look(
            &mut commands,
            draw,
            &mut ground_materials,
            &mut cover_materials,
            clock,
            key,
            parts,
            look,
        );
        if look == Look::In {
            draw.fading.push(Fading {
                key,
                start: now,
                steady: Some(parts),
            });
        }
    }
    // Late chunks: wanted, in view, not shown yet (how long they waited is logged).
    let mut late = 0usize;
    for k in wanted.iter() {
        if draw.chunks.get(k).is_some_and(|m| m.shown) {
            continue;
        }
        state.since.entry(*k).or_insert(now);
        let c = *centres
            .entry(*k)
            .or_insert_with(|| ground.lod_centre_point(*k));
        if in_view(
            c,
            BlockGround::lod_size(k.0) * std::f64::consts::FRAC_1_SQRT_2,
        ) {
            late += 1;
        }
    }
    state.since.retain(|k, _| wanted.contains(k));
    let stats = &mut state.stats;
    stats.frames += 1;
    if late > 0 {
        stats.late_frames += 1;
    }
    stats.late_max = stats.late_max.max(late);
    if now - stats.since >= 5.0 {
        let span = now - stats.since;
        let shown = draw
            .chunks
            .values()
            .filter(|m| m.shown && m.entity.is_some())
            .count();
        info!(
            "blocks: {shown} chunks shown of {} wanted ({} pending, {} in the cache), {:.0} built/s, \
             {:.1} ms each on the workers (heights {:.1}, cells {:.1}, mesh {:.1}; worst {:.0} ms, \
             {:.1} workers busy on average), {:.0}k triangles built/s, {} px per column; late: \
             {:.0}% of frames with chunks in view not shown (at most {}), waits {:.2} s on \
             average, {:.2} s at most",
            wanted.len(),
            draw.pending.len(),
            draw.cache.len(),
            stats.built as f64 / span,
            stats.work_ms / stats.built.max(1) as f64,
            stats.stages[0] / stats.built.max(1) as f64,
            stats.stages[1] / stats.built.max(1) as f64,
            stats.stages[2] / stats.built.max(1) as f64,
            stats.worst_ms,
            stats.work_ms / 1e3 / span,
            stats.triangles as f64 / span / 1e3,
            pixels,
            100.0 * stats.late_frames as f64 / stats.frames.max(1) as f64,
            stats.late_max,
            stats.wait_sum / stats.waits.max(1) as f64,
            stats.wait_max,
        );
        *stats = StreamStats {
            since: now,
            ..default()
        };
    }
    if draw.chunks.len() != state.last_count {
        state.last_count = draw.chunks.len();
        debug!(
            "block chunks: {} made, {} being made, {} wanted, camera {:.1} m up",
            draw.chunks.len(),
            draw.pending.len(),
            wanted.len(),
            height
        );
    }
    // Chunks being made that are no longer wanted nor ahead are dropped (not yet started ones
    // never run).
    let ahead_wanted = &state.ahead.wanted;
    draw.pending
        .retain(|k, _| wanted.contains(k) || ahead_wanted.contains(k));
}

/// Ends dissolves (the shaders run them): chunks dissolved out go to the cache, ones dissolved
/// in become steady.
fn fade_chunks(
    mut commands: Commands,
    draw: Option<ResMut<BlockDraw>>,
    real: Res<Time<Real>>,
    mut ground_materials: ResMut<Assets<GroundMaterial>>,
    mut cover_materials: ResMut<Assets<CoverMaterial>>,
) {
    let Some(mut draw) = draw else { return };
    let draw = &mut *draw;
    let now = real.elapsed_secs_f64();
    let (done, keep): (Vec<Fading>, Vec<Fading>) = std::mem::take(&mut draw.fading)
        .into_iter()
        .partition(|f| now - f.start >= FADE);
    draw.fading = keep;
    for f in done {
        match f.steady {
            Some(parts) => {
                set_look(
                    &mut commands,
                    draw,
                    &mut ground_materials,
                    &mut cover_materials,
                    0.0,
                    f.key,
                    parts,
                    Look::Steady,
                );
            }
            None => {
                if let Some(m) = draw.leaving.remove(&f.key) {
                    cache_put(draw, &mut commands, f.key, m, now);
                }
            }
        }
    }
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

/// Places the chunks camera-relative, each in its own frame. (The ground chunks don't cut
/// themselves off at the circle they share with the far tiles any more: the tiles lie 0.5 m
/// below the faceted ground and hide only near the camera, so the ground material isn't
/// touched every frame; `get_mut` marks it changed and every ground chunk is re-prepared.)
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
    // `get_mut` marks the material changed (and every entity using it re-prepared): only
    // when the value really changes.
    if materials
        .get(&draw.glow)
        .is_some_and(|m| m.emissive != want)
        && let Some(mut m) = materials.get_mut(&draw.glow)
    {
        m.emissive = want;
    }
    let win = LinearRgba::rgb(1.0, 0.62, 0.3) * (night * 3.0 / exposure);
    if materials
        .get(&draw.windows)
        .is_some_and(|m| m.emissive != win)
        && let Some(mut m) = materials.get_mut(&draw.windows)
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
    // Debug: `ULTRA_SPACE_VILLAGE_WALKER=x,y,heading` starts on foot there (village metres; the
    // heading in degrees, 180 facing north).
    let (at, facing) = std::env::var("ULTRA_SPACE_VILLAGE_WALKER")
        .ok()
        .and_then(|v| {
            let n: Vec<f64> = v.split(',').filter_map(|x| x.trim().parse().ok()).collect();
            (n.len() == 3).then(|| ([n[0], n[1], 0.0], n[2]))
        })
        .unwrap_or((world.village.start.pos, world.village.start.facing));
    let walker = pose(at, facing, 1.0);
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
