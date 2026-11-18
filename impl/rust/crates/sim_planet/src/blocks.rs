//! The ground as blocks, planet-wide (block-world.md §5.0, §7).
//!
//! The ground is columns of 50 cm blocks on a bent cube-sphere grid: six faces, each mapped onto
//! the sphere by equal angles (columns point away from the centre), oriented so face 0 is centred
//! on the demo village with its axes east and north (there the grid is the village's tangent
//! grid to well under a millimetre). A column's top is the natural terrain stepped to the grid,
//! with, round the demo village, its layout laid on that ground (levelled pads, paths, the dug
//! pit; content/data/village_layouts). Heights are radial, cell k = 0 at one floor
//! height everywhere. Chunks of 32 × 32 columns are made on demand near the player; nothing is
//! stored for untouched ground.
//!
//! Near the focus (the player, set by the game) the terrain reads its surface here (landing,
//! walking's fallback contact, placing things) and its collision patches sink below it so the
//! blocks' own collision meshes carry contact; farther away the natural ground stands.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};

use glam::{DMat3, DQuat, DVec2, DVec3, IVec3, Vec3};
use sim_blocks::{Dir, MeshData};

use crate::Terrain;
use crate::foundations::tangent_frame;

/// Block edge (m).
pub const CELL: f64 = 0.5;
/// Columns per chunk side.
pub const CHUNK: i32 = 32;
/// How far below the block surface the coarse collision patches sit (m).
pub const PATCH_SINK: f64 = 0.75;

/// The ground colours, by index (1-based in cells; 0 is empty).
pub const PALETTE: [&str; 20] = [
    "turf", "turf2", "turf3", "path", "path2", "soil", "soil2", "bedrock", "bedrock2", "ore",
    "sand", "rock", "rock2", "tilled", "straw", "straw2", "moss", "stone", "stone2", "white",
];
/// Columns per side of a drawn chunk (any level of detail): bigger than the physics chunks, so
/// fewer meshes are drawn.
pub const LOD_COLUMNS: i32 = 64;
/// How far round the focus the block surface replaces the natural one (m).
pub const FOCUS_REACH: f64 = 2_000.0;
/// Chunks of column tops remembered (4 KB each); the least recently used half goes past this.
const CACHE_CHUNKS: usize = 3_000;
/// A column not yet worked out, and a column with no blocks (in the cache).
const UNKNOWN: i32 = i32::MIN;
const NO_BLOCKS: i32 = i32::MIN + 1;

/// The tops of one chunk's columns, as worked out so far, and when it was last used.
struct ChunkTops {
    tops: Box<[i32]>,
    used: std::sync::atomic::AtomicU64,
}

/// How the ground near the player is drawn and walked on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GroundStyle {
    /// Columns of blocks (block-world.md).
    #[default]
    Blocks,
    /// Flat-shaded low-poly triangles over the same ground (the faceted alternative, tried
    /// 2026-11-18): facets two columns wide, coloured per facet.
    Faceted,
}

/// A chunk: its grid face and chunk column indices.
pub type ChunkKey = (u8, i32, i32);
/// A chunk of any level of detail: level (columns 2^level blocks wide; 0 the real blocks),
/// face and chunk indices at that level. Its four children are the next level down.
pub type LodKey = (u8, u8, i32, i32);

/// A chunk's mesh in its own frame: positions relative to `origin` (surface frame, m; the
/// chunk's centre at the grid floor) turned by `basis` (columns east-ish, up, south-ish).
pub struct ChunkMesh {
    pub mesh: MeshData,
    pub origin: DVec3,
    pub basis: DQuat,
    /// Time spent (ms): column heights, cells and colours, meshing.
    pub timings: [f32; 3],
}

fn colour(name: &str) -> u16 {
    PALETTE
        .iter()
        .position(|&p| p == name)
        .map_or(1, |i| i as u16 + 1)
}

/// A village's ground (content/data/village_layouts): levelled pads, paths and dug pits in
/// metres from the village centre (x east, y north), laid on the real ground by the game.
#[derive(Clone, Debug, serde::Deserialize)]
pub struct VillageLayout {
    pub id: String,
    #[serde(default)]
    pub description: String,
    pub half_size_m: f64,
    pub pads: Vec<LayoutPad>,
    pub paths: Vec<LayoutPath>,
    pub pits: Vec<LayoutPit>,
}

/// A levelled area: flat at the real ground's height at its middle, easing into the ground.
#[derive(Clone, Debug, serde::Deserialize)]
pub struct LayoutPad {
    #[serde(default)]
    pub name: String,
    pub rect: Option<[f64; 4]>,
    pub circle: Option<[f64; 3]>,
    pub blend_m: f64,
}

impl LayoutPad {
    fn middle(&self) -> DVec2 {
        match (self.rect, self.circle) {
            (Some(r), _) => DVec2::new((r[0] + r[2]) * 0.5, (r[1] + r[3]) * 0.5),
            (_, Some(c)) => DVec2::new(c[0], c[1]),
            _ => DVec2::ZERO,
        }
    }

    /// Distance from the pad (0 inside).
    fn distance(&self, p: DVec2) -> f64 {
        match (self.rect, self.circle) {
            (Some(r), _) => (r[0] - p.x)
                .max(p.x - r[2])
                .max(r[1] - p.y)
                .max(p.y - r[3])
                .max(0.0),
            (_, Some(c)) => ((p - DVec2::new(c[0], c[1])).length() - c[2]).max(0.0),
            _ => f64::INFINITY,
        }
    }
}

#[derive(Clone, Debug, serde::Deserialize)]
pub struct LayoutPath {
    pub points: Vec<[f64; 2]>,
    pub half_width_m: f64,
}

#[derive(Clone, Debug, serde::Deserialize)]
pub struct LayoutPit {
    pub centre: [f64; 2],
    pub radius_m: f64,
    pub tunnel_m: [f64; 3],
}

/// A village laid on the ground: its layout, each pad's level and each pit's ground (m above
/// the reference radius).
struct Village {
    layout: VillageLayout,
    levels: Vec<f64>,
    pit_ground: Vec<f64>,
}

fn smoothstep01(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

impl Village {
    /// The ground height (m above the reference radius) at face 0's grid point `p`, from the
    /// natural height `h` there: pulled to each pad's level within its blend.
    fn height(&self, p: DVec2, mut h: f64) -> f64 {
        for (pad, &level) in self.layout.pads.iter().zip(&self.levels) {
            let w = smoothstep01(pad.distance(p) / pad.blend_m);
            h = level + (h - level) * w;
        }
        h
    }

    /// Whether `p` (grid metres) is on a path (ragged edges).
    fn on_path(&self, p: DVec2) -> bool {
        let rag = 0.15 * crate::noise::fbm(DVec3::new(p.x * 0.9, p.y * 0.9, 5.0), 3, 2, 0.5) * 2.0;
        self.layout.paths.iter().any(|path| {
            path.points.windows(2).any(|w| {
                let (a, b) = (DVec2::from_array(w[0]), DVec2::from_array(w[1]));
                let ab = b - a;
                let t = ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0);
                (p - (a + ab * t)).length() - path.half_width_m < rag
            })
        })
    }

    /// Negative inside a dug space; `q` is a grid point with z the height (m above the
    /// reference radius).
    fn dug(&self, q: DVec3) -> f64 {
        let mut best = f64::INFINITY;
        for (pit, &z0) in self.layout.pits.iter().zip(&self.pit_ground) {
            let c = DVec2::from_array(pit.centre);
            let d = DVec3::new(q.x - c.x, q.y - c.y, (q.z - z0) * 1.3);
            best = best.min(d.length() - pit.radius_m);
            let a = DVec3::new(c.x + 1.0, c.y, z0 - 1.6);
            let b = a + DVec3::from_array(pit.tunnel_m);
            let t = ((q - a).dot(b - a) / (b - a).length_squared()).clamp(0.0, 1.0);
            best = best.min((q - (a + (b - a) * t)).length() - 1.0);
        }
        best
    }

    /// Whether `p` is near a pit or its tunnel (where the ground is meshed deep).
    fn near_pit(&self, p: DVec2) -> bool {
        self.layout.pits.iter().any(|pit| {
            let c = DVec2::from_array(pit.centre);
            let end = c + DVec2::new(pit.tunnel_m[0], pit.tunnel_m[1]);
            (p - c).length() < pit.radius_m + 3.5 || (p - end).length() < 3.0
        })
    }
}

/// The block ground of a planet.
pub struct BlockGround {
    /// Unit direction of the village centre (surface frame): face 0's centre.
    pub center: DVec3,
    pub east: DVec3,
    pub north: DVec3,
    /// Height of the grid's k = 0 floor (m above the body's reference radius).
    pub base: f64,
    pub radius: f64,
    natural: Arc<Terrain>,
    village: Option<Village>,
    /// Columns at or next to a dug cell (meshed deeper).
    carved_near: HashSet<(i32, i32)>,
    /// Collision boxes of the village's buildings, props and tree trunks (surface frame).
    pub structures: Vec<crate::foundations::Structure>,
    /// The cube's faces: centre, x axis, y axis (surface frame, unit).
    faces: [[DVec3; 3]; 6],
    tops: RwLock<HashMap<ChunkKey, ChunkTops>>,
    clock: std::sync::atomic::AtomicU64,
    /// Where the player is (unit direction); `None` means everywhere.
    focus: RwLock<Option<DVec3>>,
    pub style: GroundStyle,
}

impl std::fmt::Debug for BlockGround {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BlockGround")
            .field("center", &self.center)
            .field("base", &self.base)
            .finish_non_exhaustive()
    }
}

impl BlockGround {
    /// The block ground of `natural`'s body, face 0 centred on `center` (unit direction) with
    /// the village's ground there; its floor at the natural height of the centre stepped down
    /// to the grid.
    pub fn new(natural: Arc<Terrain>, center: DVec3, layout: Option<VillageLayout>) -> Self {
        let c = center.normalize();
        let (e, south) = tangent_frame(c);
        let n = -south;
        let h = natural.surface_height(c, 1.0);
        let base = (h / CELL).floor() * CELL;
        let mut g = Self {
            center: c,
            east: e,
            north: n,
            base,
            radius: natural.radius,
            natural,
            village: None,
            carved_near: HashSet::new(),
            structures: Vec::new(),
            faces: [
                [c, e, n],
                [-c, -e, n],
                [e, -c, n],
                [-e, c, n],
                [n, e, -c],
                [-n, e, c],
            ],
            tops: RwLock::new(HashMap::new()),
            clock: std::sync::atomic::AtomicU64::new(0),
            focus: RwLock::new(None),
            style: GroundStyle::Blocks,
        };
        if let Some(layout) = layout {
            g.lay_village(layout);
        }
        g
    }

    /// Lays a village on the ground round face 0's centre: each pad at the real ground's height
    /// at its middle, each pit at the ground of its centre (after the pads).
    fn lay_village(&mut self, layout: VillageLayout) {
        let ground = |p: DVec2| self.natural.height(self.dir(p.x, p.y), 4.0);
        let levels: Vec<f64> = layout.pads.iter().map(|pad| ground(pad.middle())).collect();
        let mut village = Village {
            layout,
            levels,
            pit_ground: Vec::new(),
        };
        village.pit_ground = village
            .layout
            .pits
            .iter()
            .map(|pit| {
                let c = DVec2::from_array(pit.centre);
                village.height(c, ground(c))
            })
            .collect();
        let n = (village.layout.half_size_m / CELL).ceil() as i32;
        for j in -n..n {
            for i in -n..n {
                let p = DVec2::new((f64::from(i) + 0.5) * CELL, (f64::from(j) + 0.5) * CELL);
                if village.near_pit(p) {
                    self.carved_near.insert((i, j));
                }
            }
        }
        self.village = Some(village);
    }

    /// The same ground drawn and walked on in `style`.
    pub fn with_style(mut self, style: GroundStyle) -> Self {
        self.style = style;
        self
    }

    /// Sets where the player is (unit direction, surface frame); `None` for everywhere.
    pub fn set_focus(&self, dir: Option<DVec3>) {
        if let Ok(mut f) = self.focus.write() {
            *f = dir.map(DVec3::normalize);
        }
    }

    /// Where the player is (unit direction), if set.
    pub fn focus(&self) -> Option<DVec3> {
        self.focus.read().ok().and_then(|f| *f)
    }

    /// Whether `dir` is near enough the focus for the block surface to stand.
    pub fn in_focus(&self, dir: DVec3) -> bool {
        match self.focus.read().ok().and_then(|f| *f) {
            Some(f) => f.angle_between(dir.normalize()) * self.radius <= FOCUS_REACH,
            None => true,
        }
    }

    /// Adds a collision box: centre (grid point, m), turn about the vertical (radians,
    /// counter-clockwise from above) and half extents (x east, y north, z up before the turn).
    pub fn add_box(&mut self, center: DVec3, yaw: f64, half: DVec3) {
        let up = self.dir(center.x, center.y);
        let turned = DQuat::from_rotation_arc(self.center, up) * self.basis();
        self.structures.push(crate::foundations::Structure {
            center: self.point(center),
            rotation: (turned * DQuat::from_rotation_y(yaw)).normalize(),
            half: DVec3::new(half.x, half.z, half.y),
        });
    }

    /// Structures within `radius` (m) of unit direction `dir`: (index, structure).
    pub fn structures_near(
        &self,
        dir: DVec3,
        radius: f64,
    ) -> impl Iterator<Item = (usize, &crate::foundations::Structure)> {
        let at = dir * self.radius;
        self.structures.iter().enumerate().filter(move |(_, s)| {
            (s.center.normalize() * self.radius).distance(at) <= radius + s.half.length()
        })
    }

    /// The village frame's rotation (columns: east, up, south; as a settlement's).
    pub fn basis(&self) -> DQuat {
        DQuat::from_mat3(&DMat3::from_cols(self.east, self.center, -self.north))
    }

    /// Height of `point` (surface frame, m) above the grid's floor.
    pub fn height_above(&self, point: DVec3) -> f64 {
        point.length() - self.radius - self.base
    }

    /// "Up" at a point (surface frame): away from the centre.
    pub fn up_at(&self, point: DVec3) -> DVec3 {
        point.normalize()
    }

    /// The village frame's origin (surface frame, m): the centre at the grid's floor.
    pub fn origin(&self) -> DVec3 {
        self.center * (self.radius + self.base)
    }

    /// Unit direction through grid point (x, y) (m) of `face`.
    pub fn dir_on(&self, face: u8, x: f64, y: f64) -> DVec3 {
        let [c, e, n] = self.faces[face as usize];
        (c + e * (x / self.radius).tan() + n * (y / self.radius).tan()).normalize()
    }

    /// The face and grid point (x, y; m) under unit direction `dir`.
    pub fn locate(&self, dir: DVec3) -> (u8, DVec2) {
        let d = dir.normalize();
        let face = (0..6)
            .max_by(|&a, &b| d.dot(self.faces[a][0]).total_cmp(&d.dot(self.faces[b][0])))
            .unwrap_or(0);
        let [c, e, n] = self.faces[face];
        let z = d.dot(c);
        (
            face as u8,
            DVec2::new(
                (d.dot(e) / z).atan() * self.radius,
                (d.dot(n) / z).atan() * self.radius,
            ),
        )
    }

    /// Grid point (x, y; m) of face 0 (the village's) under `dir`, if `dir` is on face 0.
    pub fn plane(&self, dir: DVec3) -> Option<DVec2> {
        let (face, p) = self.locate(dir);
        (face == 0).then_some(p)
    }

    /// Unit direction through face 0's grid point (x, y).
    pub fn dir(&self, x: f64, y: f64) -> DVec3 {
        self.dir_on(0, x, y)
    }

    /// Surface-frame position of grid point (x, y, z up from the floor; m) of `face`.
    pub fn point_on(&self, face: u8, p: DVec3) -> DVec3 {
        self.dir_on(face, p.x, p.y) * (self.radius + self.base + p.z)
    }

    /// Surface-frame position of face 0's grid point (x east, y north, z up from the floor; m).
    pub fn point(&self, p: DVec3) -> DVec3 {
        self.point_on(0, p)
    }

    /// The village frame's coordinates (x east, y up, z south) of face 0's grid point `p`.
    pub fn local(&self, p: DVec3) -> DVec3 {
        self.basis().inverse() * (self.point(p) - self.origin())
    }

    fn column_dir(&self, face: u8, i: i32, j: i32) -> DVec3 {
        self.dir_on(
            face,
            (f64::from(i) + 0.5) * CELL,
            (f64::from(j) + 0.5) * CELL,
        )
    }

    fn natural_top(&self, face: u8, i: i32, j: i32) -> Option<i32> {
        // Detail finer than a few blocks can't show in the steps (and costs octaves).
        let h = self.natural.height(self.column_dir(face, i, j), 4.0);
        if self.natural.has_ocean() && h < 0.3 {
            return None; // water: no blocks (the sea and its floor stay as they are)
        }
        Some(((h - self.base) / CELL).floor() as i32)
    }

    /// The top cell of column (i, j) of `face`, or `None` where there are no blocks.
    pub fn top_on(&self, face: u8, i: i32, j: i32) -> Option<i32> {
        use std::sync::atomic::Ordering::Relaxed;
        let key = (face, i.div_euclid(CHUNK), j.div_euclid(CHUNK));
        let slot = (i.rem_euclid(CHUNK) + CHUNK * j.rem_euclid(CHUNK)) as usize;
        let now = self.clock.fetch_add(1, Relaxed);
        if let Ok(t) = self.tops.read()
            && let Some(c) = t.get(&key)
            && c.tops[slot] != UNKNOWN
        {
            c.used.store(now, Relaxed);
            let v = c.tops[slot];
            return (v != NO_BLOCKS).then_some(v);
        }
        let v = match &self.village {
            Some(v) if face == 0 && self.in_village(i, j) => {
                let p = DVec2::new((f64::from(i) + 0.5) * CELL, (f64::from(j) + 0.5) * CELL);
                let h = v.height(p, self.natural.height(self.column_dir(face, i, j), 4.0));
                Some(((h - self.base) / CELL).floor() as i32)
            }
            _ => self.natural_top(face, i, j),
        };
        if let Ok(mut t) = self.tops.write() {
            if t.len() >= CACHE_CHUNKS && !t.contains_key(&key) {
                // Forget the least recently used half.
                let mut ages: Vec<(u64, ChunkKey)> =
                    t.iter().map(|(k, c)| (c.used.load(Relaxed), *k)).collect();
                ages.sort_unstable();
                for (_, k) in ages.iter().take(ages.len() / 2) {
                    t.remove(k);
                }
            }
            let c = t.entry(key).or_insert_with(|| ChunkTops {
                tops: vec![UNKNOWN; (CHUNK * CHUNK) as usize].into_boxed_slice(),
                used: std::sync::atomic::AtomicU64::new(now),
            });
            c.tops[slot] = v.unwrap_or(NO_BLOCKS);
            c.used.store(now, Relaxed);
        }
        v
    }

    /// The top cell of face 0's column (i, j).
    pub fn top(&self, i: i32, j: i32) -> Option<i32> {
        self.top_on(0, i, j)
    }

    /// The column (i, j) of grid point (x, y).
    pub fn column(x: f64, y: f64) -> (i32, i32) {
        ((x / CELL).floor() as i32, (y / CELL).floor() as i32)
    }

    /// The block surface's height (m above the reference radius) under `dir`, near the focus.
    pub fn surface(&self, dir: DVec3) -> Option<f64> {
        if !self.in_focus(dir) {
            return None;
        }
        let (face, p) = self.locate(dir);
        if self.style == GroundStyle::Faceted {
            return Some(self.facet_surface(face, p));
        }
        let (i, j) = Self::column(p.x, p.y);
        let t = self.top_on(face, i, j)?;
        Some(self.base + f64::from(t + 1) * CELL)
    }

    /// The ground surface height above the floor at face 0's grid point (x, y) (m; 0 where
    /// there are no blocks).
    pub fn ground_z(&self, x: f64, y: f64) -> f64 {
        if self.style == GroundStyle::Faceted {
            return self.facet_surface(0, DVec2::new(x, y)) - self.base;
        }
        let (i, j) = Self::column(x, y);
        self.top(i, j).map_or(0.0, |t| f64::from(t + 1) * CELL)
    }

    /// Whether face 0's column (i, j) lies in the village's square.
    fn in_village(&self, i: i32, j: i32) -> bool {
        self.village.as_ref().is_some_and(|v| {
            let half = v.layout.half_size_m;
            let (x, y) = ((f64::from(i) + 0.5) * CELL, (f64::from(j) + 0.5) * CELL);
            x.abs() < half && y.abs() < half
        })
    }

    fn carved(&self, face: u8, p: IVec3) -> bool {
        face == 0
            && self.carved_near.contains(&(p.x, p.y))
            && self.village.as_ref().is_some_and(|v| {
                let q = DVec3::new(
                    (f64::from(p.x) + 0.5) * CELL,
                    (f64::from(p.y) + 0.5) * CELL,
                    self.base + (f64::from(p.z) + 0.5) * CELL,
                );
                v.dug(q) <= 0.0
            })
    }

    /// The colour of the top cell of column (i, j) of `face`: the village's own, rock where
    /// steep, beaches by the sea, else from what grows there (the world's cover).
    fn top_colour(&self, face: u8, i: i32, j: i32, t: i32) -> u16 {
        self.top_colour_with(face, i, j, t, None)
    }

    /// `top_colour`, with the steepness given (`None`: judged from the fine neighbours).
    fn top_colour_with(&self, face: u8, i: i32, j: i32, t: i32, steep: Option<bool>) -> u16 {
        if face == 0
            && self.in_village(i, j)
            && let Some(v) = &self.village
            && v.on_path(DVec2::new(
                (f64::from(i) + 0.5) * CELL,
                (f64::from(j) + 0.5) * CELL,
            ))
        {
            return colour(if (i * 7 + j * 3).rem_euclid(5) == 0 {
                "path2"
            } else {
                "path"
            });
        }
        let speckle = f64::from((i.wrapping_mul(73_856_093) ^ j.wrapping_mul(19_349_663)) & 255)
            / 255.0
            - 0.5;
        let n = crate::noise::fbm(
            // 0.3 per metre, as the tiles' shader (blocks and far ground jitter alike).
            DVec3::new(
                f64::from(i) * CELL * 0.3,
                f64::from(j) * CELL * 0.3,
                9.0 + f64::from(face),
            ),
            7,
            2,
            0.5,
        ) + 0.15 * speckle;
        let steep = steep.unwrap_or_else(|| {
            [(1, 0), (-1, 0), (0, 1), (0, -1)]
                .iter()
                .filter_map(|&(a, b)| self.top_on(face, i + a, j + b))
                .any(|m| (m - t).abs() >= 3)
        });
        let height = self.base + f64::from(t) * CELL;
        let cover = self
            .natural
            .globe()
            .map(|g| g.ground_cover(self.column_dir(face, i, j), height, n));
        use crate::globe::Cover;
        if steep && !matches!(cover, Some(Cover::Ice | Cover::Snow)) {
            return colour(if (i + j) % 3 == 0 { "rock2" } else { "rock" });
        }
        if height < 1.5 && self.natural.has_ocean() {
            return colour(if n < 0.0 { "straw" } else { "straw2" });
        }
        cover_colour(cover, n)
    }

    // ---- the faceted ground ---------------------------------------------------------------------

    /// The ground's height (m above the reference radius) at grid point `p` of `face`, detail
    /// down to `detail` (m): the natural ground, the village's pads, and its pits as hollows.
    fn ground_height(&self, face: u8, p: DVec2, detail: f64) -> f64 {
        let mut h = self.natural.height(self.dir_on(face, p.x, p.y), detail);
        if face == 0
            && let Some(v) = &self.village
            && p.x.abs() < v.layout.half_size_m
            && p.y.abs() < v.layout.half_size_m
        {
            h = v.height(p, h);
            for (pit, &z0) in v.layout.pits.iter().zip(&v.pit_ground) {
                let d = (p - DVec2::from_array(pit.centre)).length();
                if d < pit.radius_m {
                    let depth = (pit.radius_m * pit.radius_m - d * d).sqrt() / 1.3;
                    h = h.min(z0 - depth);
                }
            }
        }
        h
    }

    /// Whether the facet quad (a, b) of `face` at `level` is split along its other diagonal
    /// (alternating by a hash, so the facets don't line up in rows).
    fn facet_flip(face: u8, level: u8, a: i64, b: i64) -> bool {
        let h = (a.wrapping_mul(73_856_093) ^ b.wrapping_mul(19_349_663))
            .wrapping_add(i64::from(face) * 83_492_791 + i64::from(level) * 2_654_435);
        (h >> 3) & 1 == 1
    }

    /// The faceted surface's height (m above the reference radius) at grid point `p` of `face`:
    /// the plane of the finest facet over it.
    pub fn facet_surface(&self, face: u8, p: DVec2) -> f64 {
        let fs = 2.0 * CELL;
        let (a, b) = ((p.x / fs).floor(), (p.y / fs).floor());
        let (u, v) = (p.x / fs - a, p.y / fs - b);
        let at = |da: f64, db: f64| {
            self.ground_height(face, DVec2::new((a + da) * fs, (b + db) * fs), fs)
        };
        let (h00, h10, h01, h11) = (at(0.0, 0.0), at(1.0, 0.0), at(0.0, 1.0), at(1.0, 1.0));
        if Self::facet_flip(face, 0, a as i64, b as i64) {
            // Split along (0,1)-(1,0).
            if u + v <= 1.0 {
                h00 + (h10 - h00) * u + (h01 - h00) * v
            } else {
                h11 + (h01 - h11) * (1.0 - u) + (h10 - h11) * (1.0 - v)
            }
        } else if u >= v {
            // Split along (0,0)-(1,1).
            h00 + (h10 - h00) * u + (h11 - h10) * v
        } else {
            h00 + (h01 - h00) * v + (h11 - h01) * u
        }
    }

    /// A faceted chunk of `n` × `n` columns, `size` metres a side: facets two columns wide,
    /// flat-shaded, each one colour; a 2 m skirt round its edge.
    fn facet_chunk(&self, key: LodKey, n: i32, size: f64) -> Option<ChunkMesh> {
        let started = std::time::Instant::now();
        let (level, face, ci, cj) = key;
        let facets = (n / 2).max(1);
        let fs = size / f64::from(facets);
        let (x0, y0) = (f64::from(ci) * size, f64::from(cj) * size);
        // Vertex heights.
        let side = (facets + 1) as usize;
        let mut hs = Vec::with_capacity(side * side);
        for b in 0..=facets {
            for a in 0..=facets {
                let p = DVec2::new(x0 + f64::from(a) * fs, y0 + f64::from(b) * fs);
                hs.push(self.ground_height(face, p, fs));
            }
        }
        let t_heights = started.elapsed().as_secs_f32() * 1e3;
        let up = self.dir_on(face, x0 + size * 0.5, y0 + size * 0.5);
        let origin = up * (self.radius + self.base);
        let e = self.faces[face as usize][1];
        let east = (e - up * e.dot(up)).normalize();
        let south = east.cross(up);
        let basis = DQuat::from_mat3(&DMat3::from_cols(east, up, south));
        let inverse = basis.inverse();
        let point = |a: i32, b: i32, drop: f64| {
            let p = DVec2::new(x0 + f64::from(a) * fs, y0 + f64::from(b) * fs);
            let h = hs[a as usize + b as usize * side] - drop;
            self.dir_on(face, p.x, p.y) * (self.radius + h)
        };
        let mut m = MeshData::default();
        // A triangle facing `out` (default: up, away from the centre).
        let tri = |m: &mut MeshData, w: [DVec3; 3], colour: u16, out: Option<DVec3>| {
            let n = (w[1] - w[0]).cross(w[2] - w[0]);
            let outward = out.unwrap_or_else(|| (w[0] + w[1] + w[2]).normalize());
            let w = if n.dot(outward) < 0.0 {
                [w[0], w[2], w[1]]
            } else {
                w
            };
            let normal = (inverse * (w[1] - w[0]).cross(w[2] - w[0]).normalize()).as_vec3();
            let first = m.positions.len() as u32;
            for q in w {
                m.positions
                    .push((inverse * (q - origin)).as_vec3().to_array());
                m.normals.push(normal.to_array());
                m.colours.push(colour);
            }
            m.indices.extend([first, first + 1, first + 2]);
        };
        let globe = self.natural.globe();
        for b in 0..facets {
            for a in 0..facets {
                let (ga, gb) = (
                    i64::from(ci) * i64::from(facets) + i64::from(a),
                    i64::from(cj) * i64::from(facets) + i64::from(b),
                );
                let c = [
                    point(a, b, 0.0),
                    point(a + 1, b, 0.0),
                    point(a, b + 1, 0.0),
                    point(a + 1, b + 1, 0.0),
                ];
                let tris = if Self::facet_flip(face, level, ga, gb) {
                    [[c[0], c[1], c[2]], [c[3], c[2], c[1]]]
                } else {
                    [[c[0], c[1], c[3]], [c[0], c[3], c[2]]]
                };
                for w in tris {
                    // The facet's colour from its middle.
                    let mid = (w[0] + w[1] + w[2]) / 3.0;
                    let height = mid.length() - self.radius;
                    let dir = mid.normalize();
                    let normal = (w[1] - w[0]).cross(w[2] - w[0]).normalize();
                    let steep = normal.dot(dir).abs() < 0.72;
                    let (_, gp) = self.locate(dir);
                    let jitter = crate::noise::fbm(
                        DVec3::new(gp.x * 0.3, gp.y * 0.3, 9.0 + f64::from(face)),
                        7,
                        2,
                        0.5,
                    ) + 0.1
                        * (f64::from(((ga * 31 + gb * 17) & 255) as u8) / 255.0 - 0.5);
                    let on_path = face == 0
                        && self.village.as_ref().is_some_and(|v| {
                            gp.x.abs() < v.layout.half_size_m
                                && gp.y.abs() < v.layout.half_size_m
                                && v.on_path(gp)
                        });
                    let cover = globe.map(|g| g.ground_cover(dir, height, jitter));
                    use crate::globe::Cover;
                    let col = if on_path {
                        colour(if jitter < 0.0 { "path" } else { "path2" })
                    } else if height < 0.0 && self.natural.has_ocean() {
                        colour("path2")
                    } else if steep && !matches!(cover, Some(Cover::Ice | Cover::Snow)) {
                        colour(if jitter < 0.0 { "rock" } else { "rock2" })
                    } else if height < 1.5 && self.natural.has_ocean() {
                        colour(if jitter < 0.0 { "straw" } else { "straw2" })
                    } else {
                        cover_colour(cover, jitter)
                    };
                    tri(&mut m, w, col, None);
                }
            }
        }
        // Skirts: 2 m walls down the outside of the chunk's edge.
        let soil = colour("soil");
        for k in 0..facets {
            for (a, b, a2, b2) in [
                (k, 0, k + 1, 0),
                (k + 1, facets, k, facets),
                (0, k + 1, 0, k),
                (facets, k, facets, k + 1),
            ] {
                let (p, q) = (point(a, b, 0.0), point(a2, b2, 0.0));
                let (pd, qd) = (point(a, b, 2.0), point(a2, b2, 2.0));
                // Away from the chunk's middle.
                let mid = point(facets / 2, facets / 2, 0.0);
                let out = Some((p + q) * 0.5 - mid);
                tri(&mut m, [p, q, qd], soil, out);
                tri(&mut m, [p, qd, pd], soil, out);
            }
        }
        let t_mesh = started.elapsed().as_secs_f32() * 1e3 - t_heights;
        Some(ChunkMesh {
            mesh: m,
            origin,
            basis,
            timings: [t_heights, 0.0, t_mesh],
        })
    }

    /// The cell at (i, j, k) of `face`: 0 empty, else a 1-based index into [`PALETTE`].
    pub fn cell_on(&self, face: u8, p: IVec3) -> u16 {
        let Some(t) = self.top_on(face, p.x, p.y) else {
            return 0;
        };
        if p.z > t || self.carved(face, p) {
            return 0;
        }
        // Only the shell is stored: the cells below the lowest neighbouring top (and deep
        // round dug spaces) are never seen, but kept solid so nothing shows through.
        let d = t - p.z;
        if d == 0 {
            return self.top_colour(face, p.x, p.y, t);
        }
        if d < 7 {
            return colour(if (p.x + p.y + p.z).rem_euclid(4) == 0 {
                "soil2"
            } else {
                "soil"
            });
        }
        if (p.x * 5 + p.y * 3 + p.z * 7).rem_euclid(41) == 0 {
            return colour("ore");
        }
        colour(if (p.x + 2 * p.y + p.z).rem_euclid(3) == 0 {
            "bedrock2"
        } else {
            "bedrock"
        })
    }

    /// The cell at (i, j, k) of face 0.
    pub fn cell(&self, p: IVec3) -> u16 {
        self.cell_on(0, p)
    }

    /// Side of a chunk (m).
    pub fn chunk_size() -> f64 {
        f64::from(CHUNK) * CELL
    }

    /// The unit direction through a chunk's centre.
    pub fn chunk_centre(&self, key: ChunkKey) -> DVec3 {
        let size = Self::chunk_size();
        self.dir_on(
            key.0,
            (f64::from(key.1) + 0.5) * size,
            (f64::from(key.2) + 0.5) * size,
        )
    }

    /// The mesh of a chunk in its own frame (colours 1-based into [`PALETTE`]); `None` when it
    /// has no blocks.
    pub fn chunk(&self, key: ChunkKey) -> Option<ChunkMesh> {
        self.chunk_of((0, key.0, key.1, key.2), CHUNK, Self::chunk_size())
    }

    /// Side of a chunk of `level` (m).
    pub fn lod_size(level: u8) -> f64 {
        f64::from(LOD_COLUMNS) * CELL * f64::from(1u32 << level)
    }

    /// The unit direction through the centre of a chunk of any level.
    pub fn lod_centre(&self, key: LodKey) -> DVec3 {
        let size = Self::lod_size(key.0);
        self.dir_on(
            key.1,
            (f64::from(key.2) + 0.5) * size,
            (f64::from(key.3) + 0.5) * size,
        )
    }

    /// The centre of a chunk of any level at its ground (surface frame, m).
    pub fn lod_centre_point(&self, key: LodKey) -> DVec3 {
        let d = self.lod_centre(key);
        let h = self.natural.height(d, Self::lod_size(key.0)).max(0.0);
        d * (self.radius + h)
    }

    /// The four children of a chunk (the next level down).
    pub fn lod_children(key: LodKey) -> [LodKey; 4] {
        let (l, f, i, j) = key;
        let l = l.saturating_sub(1);
        [
            (l, f, 2 * i, 2 * j),
            (l, f, 2 * i + 1, 2 * j),
            (l, f, 2 * i, 2 * j + 1),
            (l, f, 2 * i + 1, 2 * j + 1),
        ]
    }

    /// The mesh of a chunk of any level in its own frame. Level `l` has columns 2^l blocks wide
    /// (their tops the ground at each column's middle), with the blocks' own 50 cm height steps,
    /// so its terraces are the real blocks' seen coarser.
    pub fn lod_chunk(&self, key: LodKey) -> Option<ChunkMesh> {
        self.chunk_of(key, LOD_COLUMNS, Self::lod_size(key.0))
    }

    /// The mesh of chunk `key` of `n` × `n` columns, `size` metres a side.
    fn chunk_of(&self, key: LodKey, n: i32, size: f64) -> Option<ChunkMesh> {
        if self.style == GroundStyle::Faceted {
            return self.facet_chunk(key, n, size);
        }
        let (level, face, ci, cj) = key;
        let s = 1i32 << level;
        let fine = |a: i32| a * s + s / 2;
        let width = f64::from(s) * CELL;
        // Coarse columns read the ground at their own scale (and skip the fine cache), except
        // in the village's square.
        let top = |a: i32, b: i32| {
            let (i, j) = (fine(a), fine(b));
            let village = face == 0 && self.in_village(i, j);
            if level == 0 || village {
                return self.top_on(face, i, j);
            }
            let h = self.natural.height(self.column_dir(face, i, j), width);
            if self.natural.has_ocean() && h < 0.3 {
                return None;
            }
            Some(((h - self.base) / CELL).floor() as i32)
        };
        let started = std::time::Instant::now();
        let (i0, j0) = (ci * n, cj * n);
        // Column tops of the chunk and a margin round it (shapes and hidden faces look two
        // cells out), in a plain array.
        const MARGIN: i32 = 4;
        let side = n + 2 * MARGIN;
        let mut tops: Vec<Option<i32>> = Vec::with_capacity((side * side) as usize);
        for j in j0 - MARGIN..j0 + n + MARGIN {
            for i in i0 - MARGIN..i0 + n + MARGIN {
                tops.push(top(i, j));
            }
        }
        let top_at = |i: i32, j: i32| {
            let (a, b) = (i - i0 + MARGIN, j - j0 + MARGIN);
            if a < 0 || b < 0 || a >= side || b >= side {
                return None;
            }
            tops[(a + b * side) as usize]
        };
        let t_heights = started.elapsed().as_secs_f32() * 1e3;
        let mesh = if level == 0 {
            // Real blocks: the full mesher (slopes on terrace edges), over a dense grid.
            let mut lows: Vec<Option<(i32, i32)>> =
                Vec::with_capacity(((n + 6) * (n + 6)) as usize);
            let (mut kmin, mut kmax) = (i32::MAX, i32::MIN);
            let (mut klo, mut khi) = (i32::MAX, i32::MIN);
            for j in j0 - 3..j0 + n + 3 {
                for i in i0 - 3..i0 + n + 3 {
                    let Some(t) = top_at(i, j) else {
                        lows.push(None);
                        continue;
                    };
                    let mut lo = t;
                    for a in -1..=1 {
                        for b in -1..=1 {
                            if let Some(m) = top_at(i + a, j + b) {
                                lo = lo.min(m);
                            }
                        }
                    }
                    let deep = face == 0 && self.carved_near.contains(&(i, j));
                    let lo = if deep { lo.min(t - 14) } else { lo - 1 };
                    lows.push(Some((lo, t)));
                    kmin = kmin.min(lo - 2);
                    kmax = kmax.max(t);
                    if (i0..i0 + n).contains(&i) && (j0..j0 + n).contains(&j) {
                        klo = klo.min(lo);
                        khi = khi.max(t);
                    }
                }
            }
            if klo > khi {
                return None;
            }
            let origin = IVec3::new(i0 - 3, j0 - 3, kmin);
            let mut grid = sim_blocks::Grid::new(origin, IVec3::new(n + 6, n + 6, kmax - kmin + 1));
            let mut slot = 0;
            for j in j0 - 3..j0 + n + 3 {
                for i in i0 - 3..i0 + n + 3 {
                    if let Some((lo, t)) = lows[slot] {
                        for k in lo..=t {
                            let c = self.cell_on(face, IVec3::new(i, j, k));
                            if c != 0 {
                                grid.set(IVec3::new(i, j, k), c);
                            }
                        }
                        // Below the shell counts as solid, so its faces stay hidden.
                        for k in lo - 2..lo {
                            grid.set(IVec3::new(i, j, k), colour("bedrock"));
                        }
                    }
                    slot += 1;
                }
            }
            let t_cells = started.elapsed().as_secs_f32() * 1e3 - t_heights;
            let mesh = sim_blocks::mesh(
                &grid,
                IVec3::new(i0, j0, klo),
                IVec3::new(i0 + n, j0 + n, khi + 1),
                &[Dir::PZ],
                1.0,
            );
            (mesh, t_cells)
        } else {
            // Coarse columns: tops merged along each row, walls only where a neighbour is lower.
            let t0 = started.elapsed().as_secs_f32() * 1e3;
            let mut m = MeshData::default();
            let soil = colour("soil");
            let mut colours: Vec<u16> = Vec::with_capacity((n * n) as usize);
            for j in j0..j0 + n {
                for i in i0..i0 + n {
                    let c = match top_at(i, j) {
                        Some(t) => {
                            let steep = [(1, 0), (-1, 0), (0, 1), (0, -1)]
                                .iter()
                                .filter_map(|&(a, b)| top_at(i + a, j + b))
                                .any(|o| (o - t).abs() >= 3 * s);
                            self.top_colour_with(face, fine(i), fine(j), t, Some(steep))
                        }
                        None => 0,
                    };
                    colours.push(c);
                }
            }
            let t_cells = started.elapsed().as_secs_f32() * 1e3 - t0;
            let colour_at = |i: i32, j: i32| colours[((i - i0) + (j - j0) * n) as usize];
            for j in j0..j0 + n {
                let mut i = i0;
                while i < i0 + n {
                    let Some(t) = top_at(i, j) else {
                        i += 1;
                        continue;
                    };
                    let c = colour_at(i, j);
                    let mut e = i + 1;
                    while e < i0 + n && top_at(e, j) == Some(t) && colour_at(e, j) == c {
                        e += 1;
                    }
                    let z = (t + 1) as f32;
                    push_quad(
                        &mut m,
                        [
                            Vec3::new(i as f32, j as f32, z),
                            Vec3::new(e as f32, j as f32, z),
                            Vec3::new(e as f32, (j + 1) as f32, z),
                            Vec3::new(i as f32, (j + 1) as f32, z),
                        ],
                        Vec3::Z,
                        c,
                    );
                    i = e;
                }
            }
            for j in j0..j0 + n {
                for i in i0..i0 + n {
                    let Some(t) = top_at(i, j) else { continue };
                    let (x, y) = (i as f32, j as f32);
                    for (di, dj, out, a, b) in [
                        (
                            1,
                            0,
                            Vec3::X,
                            Vec3::new(x + 1.0, y, 0.0),
                            Vec3::new(x + 1.0, y + 1.0, 0.0),
                        ),
                        (
                            -1,
                            0,
                            Vec3::NEG_X,
                            Vec3::new(x, y, 0.0),
                            Vec3::new(x, y + 1.0, 0.0),
                        ),
                        (
                            0,
                            1,
                            Vec3::Y,
                            Vec3::new(x, y + 1.0, 0.0),
                            Vec3::new(x + 1.0, y + 1.0, 0.0),
                        ),
                        (
                            0,
                            -1,
                            Vec3::NEG_Y,
                            Vec3::new(x, y, 0.0),
                            Vec3::new(x + 1.0, y, 0.0),
                        ),
                    ] {
                        let low = top_at(i + di, j + dj).unwrap_or(t - 8);
                        if low >= t {
                            continue;
                        }
                        let (top_z, mid_z, bottom_z) = ((t + 1) as f32, t as f32, (low + 1) as f32);
                        let at = |p: Vec3, z: f32| Vec3::new(p.x, p.y, z);
                        // The top cell's side in its own colour, the rest soil.
                        push_quad(
                            &mut m,
                            [at(a, top_z), at(b, top_z), at(b, mid_z), at(a, mid_z)],
                            out,
                            colour_at(i, j),
                        );
                        if bottom_z < mid_z {
                            push_quad(
                                &mut m,
                                [at(a, mid_z), at(b, mid_z), at(b, bottom_z), at(a, bottom_z)],
                                out,
                                soil,
                            );
                        }
                    }
                }
            }
            if m.is_empty() {
                return None;
            }
            (m, t_cells)
        };
        let t_cells = mesh.1;
        let t_mesh = started.elapsed().as_secs_f32() * 1e3 - t_heights - t_cells;
        let mut mesh = mesh.0;
        if mesh.is_empty() {
            return None;
        }
        // Skirts: a wall 2 m down the outside of every border column, so where the neighbour is
        // of another level (its edge a step lower) no slit opens; elsewhere it stays hidden
        // under the neighbour's tops.
        let soil = colour("soil");
        let wall = |m: &mut MeshData, a: Vec3, b: Vec3, out: Vec3, depth: f32| {
            let down = Vec3::new(0.0, 0.0, -depth);
            let mut quad = [a, b, b + down, a + down];
            if (quad[1] - quad[0]).cross(quad[2] - quad[0]).dot(out) < 0.0 {
                quad.reverse();
            }
            let first = m.positions.len() as u32;
            for p in quad {
                m.positions.push(p.to_array());
                m.normals.push(out.to_array());
                m.colours.push(soil);
            }
            m.indices
                .extend([first, first + 1, first + 2, first, first + 2, first + 3]);
        };
        let depth = 8.0;
        for m in 0..n {
            for (i, j, out) in [
                (i0 + m, j0, Vec3::NEG_Y),
                (i0 + m, j0 + n - 1, Vec3::Y),
                (i0, j0 + m, Vec3::NEG_X),
                (i0 + n - 1, j0 + m, Vec3::X),
            ] {
                let Some(t) = top_at(i, j) else { continue };
                if level == 0 && face == 0 && self.carved_near.contains(&(i, j)) {
                    continue; // dug ground: the hole's own walls
                }
                let z = (t + 1) as f32;
                let (x0, y0) = (i as f32, j as f32);
                let (a, b) = match out {
                    o if o == Vec3::NEG_Y => (Vec3::new(x0, y0, z), Vec3::new(x0 + 1.0, y0, z)),
                    o if o == Vec3::Y => {
                        (Vec3::new(x0, y0 + 1.0, z), Vec3::new(x0 + 1.0, y0 + 1.0, z))
                    }
                    o if o == Vec3::NEG_X => (Vec3::new(x0, y0, z), Vec3::new(x0, y0 + 1.0, z)),
                    _ => (Vec3::new(x0 + 1.0, y0, z), Vec3::new(x0 + 1.0, y0 + 1.0, z)),
                };
                wall(&mut mesh, a, b, out, depth);
            }
        }
        // Into the chunk's own frame: origin at its centre on the floor, up there.
        let (face_, ci_, cj_) = (key.1, key.2, key.3);
        let up = self.dir_on(
            face_,
            (f64::from(ci_) + 0.5) * size,
            (f64::from(cj_) + 0.5) * size,
        );
        let origin = up * (self.radius + self.base);
        let e = self.faces[face as usize][1];
        let east = (e - up * e.dot(up)).normalize();
        let south = east.cross(up);
        let basis = DQuat::from_mat3(&DMat3::from_cols(east, up, south));
        let inverse = basis.inverse();
        for p in &mut mesh.positions {
            let g = DVec3::new(
                f64::from(p[0]) * width,
                f64::from(p[1]) * width,
                f64::from(p[2]) * CELL,
            );
            *p = (inverse * (self.point_on(face, g) - origin))
                .as_vec3()
                .to_array();
        }
        if level == 0 {
            for n in &mut mesh.normals {
                *n = Vec3::new(n[0], n[2], -n[1]).to_array();
            }
        } else {
            // Columns wider than tall: slopes tilt differently, so normals from the triangles
            // (every face has its own vertices).
            for t in mesh.indices.chunks(3) {
                let [a, b, c] =
                    [t[0], t[1], t[2]].map(|v| Vec3::from_array(mesh.positions[v as usize]));
                let n = (b - a).cross(c - a).normalize_or_zero().to_array();
                for &v in t {
                    mesh.normals[v as usize] = n;
                }
            }
        }
        Some(ChunkMesh {
            mesh,
            origin,
            basis,
            timings: [t_heights, t_cells, t_mesh],
        })
    }

    /// The chunks to draw round the camera at `camera` (surface frame, m): a quadtree from
    /// `top_level`, each chunk splitting into its children while their columns would still be
    /// at least `reach(level - 1)` away... that is, `reach(level)` is how far columns of `level`
    /// stay wide enough on screen. Only the coarsest chunks are left out by distance, so the
    /// chunks drawn cover the ground without holes or overlaps. `near` gives how near a chunk
    /// comes to the camera (m; callers may remember its centre).
    pub fn lod_plan(
        &self,
        camera: DVec3,
        top_level: u8,
        reach: impl Fn(u8) -> f64,
        mut near: impl FnMut(LodKey) -> f64,
    ) -> HashSet<LodKey> {
        let mut wanted = HashSet::new();
        let mut stack: Vec<LodKey> =
            self.lod_around(camera.normalize(), reach(top_level), top_level);
        while let Some(k) = stack.pop() {
            let d = near(k);
            if k.0 == top_level && d >= reach(k.0) {
                continue;
            }
            if k.0 > 0 && d < reach(k.0 - 1) {
                stack.extend(Self::lod_children(k));
            } else {
                wanted.insert(k);
            }
        }
        wanted
    }

    /// The chunks of `level` within `radius` (m) of unit direction `dir`, on whichever faces
    /// that disc touches.
    pub fn lod_around(&self, dir: DVec3, radius: f64, level: u8) -> Vec<LodKey> {
        self.keys_around(dir, radius, Self::lod_size(level), level)
    }

    /// Keys of chunks `size` metres a side (tagged `level`) within `radius` of `dir`.
    fn keys_around(&self, dir: DVec3, radius: f64, size: f64, level: u8) -> Vec<LodKey> {
        let d = dir.normalize();
        let (east, south) = tangent_frame(d);
        let step = size * 0.5;
        let n = (radius / step).ceil() as i32 + 1;
        let mut keys = HashSet::new();
        for a in -n..=n {
            for b in -n..=n {
                let (x, y) = (f64::from(a) * step, f64::from(b) * step);
                if x * x + y * y > (radius + size) * (radius + size) {
                    continue;
                }
                let at = (d * self.radius + east * x + south * y).normalize();
                let (face, p) = self.locate(at);
                keys.insert((
                    level,
                    face,
                    (p.x / size).floor() as i32,
                    (p.y / size).floor() as i32,
                ));
            }
        }
        let centre = |k: LodKey| {
            self.dir_on(
                k.1,
                (f64::from(k.2) + 0.5) * size,
                (f64::from(k.3) + 0.5) * size,
            )
        };
        let mut out: Vec<LodKey> = keys
            .into_iter()
            .filter(|&k| centre(k).angle_between(d) * self.radius <= radius + size * 0.71)
            .collect();
        out.sort_unstable();
        out
    }

    /// The chunks within `radius` (m) of unit direction `dir`, on whichever faces that disc
    /// touches.
    pub fn chunks_around(&self, dir: DVec3, radius: f64) -> Vec<ChunkKey> {
        self.keys_around(dir, radius, Self::chunk_size(), 0)
            .into_iter()
            .map(|k| (k.1, k.2, k.3))
            .collect()
    }
}

/// Adds a flat quad (corners in order round it) facing `out`, in colour `colour`.
fn push_quad(m: &mut MeshData, corners: [Vec3; 4], out: Vec3, colour: u16) {
    let mut q = corners;
    if (q[1] - q[0]).cross(q[2] - q[0]).dot(out) < 0.0 {
        q.reverse();
    }
    let first = m.positions.len() as u32;
    for p in q {
        m.positions.push(p.to_array());
        m.normals.push(out.to_array());
        m.colours.push(colour);
    }
    m.indices
        .extend([first, first + 1, first + 2, first, first + 2, first + 3]);
}

/// The ground's colour for what covers it, jittered by `n` (about ±0.5) between three shades.
fn cover_colour(cover: Option<crate::globe::Cover>, n: f64) -> u16 {
    use crate::globe::Cover;
    let pick = |a: &str, b: &str, c: &str| {
        colour(if n < -0.12 {
            a
        } else if n < 0.1 {
            b
        } else {
            c
        })
    };
    match cover {
        Some(Cover::Ice | Cover::Snow | Cover::Salt) => colour("white"),
        Some(Cover::Tundra) => pick("moss", "stone", "moss"),
        Some(Cover::ColdDesert) => pick("stone", "stone2", "path2"),
        Some(Cover::Desert) => pick("straw", "straw2", "straw"),
        Some(Cover::Steppe | Cover::Savanna) => pick("straw2", "turf2", "straw"),
        Some(Cover::Taiga | Cover::Rainforest | Cover::Jungle) => pick("moss", "turf3", "moss"),
        Some(Cover::Forest | Cover::Seasonal) => pick("turf3", "turf", "turf3"),
        _ => pick("turf3", "turf", "turf2"),
    }
}
