//! The ground as blocks, planet-wide (block-world.md §5.0, §7).
//!
//! The ground is columns of 25 cm blocks on a bent cube-sphere grid: six faces, each mapped onto
//! the sphere by equal angles (columns point away from the centre), oriented so face 0 is centred
//! on the demo village with its axes east and north (there the grid is the village's tangent
//! grid to well under a millimetre). A column's top is the natural terrain stepped to the grid,
//! or, in the village's square, the ground its layout exported (levelled pads, paths, the dug
//! pit), easing into the natural ground at its edge. Heights are radial, cell k = 0 at one floor
//! height everywhere. Chunks of 32 × 32 columns are made on demand near the player; nothing is
//! stored for untouched ground.
//!
//! Near the focus (the player, set by the game) the terrain reads its surface here (landing,
//! walking's fallback contact, placing things) and its collision patches sink below it so the
//! blocks' own collision meshes carry contact; farther away the natural ground stands.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};

use glam::{DMat3, DQuat, DVec2, DVec3, IVec3, Vec3};
use sim_blocks::{Cells, Dir, MeshData};

use crate::Terrain;
use crate::foundations::tangent_frame;

/// Block edge (m).
pub const CELL: f64 = 0.25;
/// Columns per chunk side.
pub const CHUNK: i32 = 32;
/// How far below the block surface the coarse collision patches sit (m).
pub const PATCH_SINK: f64 = 0.75;

/// The ground colours, by index (1-based in cells; 0 is empty).
pub const PALETTE: [&str; 20] = [
    "turf", "turf2", "turf3", "path", "path2", "soil", "soil2", "bedrock", "bedrock2", "ore",
    "sand", "rock", "rock2", "tilled", "straw", "straw2", "moss", "stone", "stone2", "white",
];
/// How far round the focus the block surface replaces the natural one (m).
pub const FOCUS_REACH: f64 = 2_000.0;
/// Columns remembered before the cache starts afresh.
const CACHE_MAX: usize = 4_000_000;

/// A chunk: its grid face and chunk column indices.
pub type ChunkKey = (u8, i32, i32);

/// A chunk's mesh in its own frame: positions relative to `origin` (surface frame, m; the
/// chunk's centre at the grid floor) turned by `basis` (columns east-ish, up, south-ish).
pub struct ChunkMesh {
    pub mesh: MeshData,
    pub origin: DVec3,
    pub basis: DQuat,
}

fn colour(name: &str) -> u16 {
    PALETTE
        .iter()
        .position(|&p| p == name)
        .map_or(1, |i| i as u16 + 1)
}

/// The village's own ground, as its layout exported it (cells of `CELL`, k = 0 the first
/// layer above the village's base height).
#[derive(Clone, Debug, Default)]
pub struct VillageGround {
    /// Column index of the first column (x east, y north).
    pub origin: [i32; 2],
    pub dims: [i32; 2],
    /// Top cell index per column, x fastest.
    pub tops: Vec<i32>,
    /// Top colour name per column.
    pub colours: Vec<String>,
    /// Cells dug out.
    pub carved: HashSet<IVec3>,
}

impl VillageGround {
    fn index(&self, i: i32, j: i32) -> Option<usize> {
        let (x, y) = (i - self.origin[0], j - self.origin[1]);
        (x >= 0 && y >= 0 && x < self.dims[0] && y < self.dims[1])
            .then(|| (x + y * self.dims[0]) as usize)
    }

    /// Distance (in columns) from (i, j) inward to the square's nearest edge (negative outside).
    fn inset(&self, i: i32, j: i32) -> i32 {
        let x = (i - self.origin[0]).min(self.origin[0] + self.dims[0] - 1 - i);
        let y = (j - self.origin[1]).min(self.origin[1] + self.dims[1] - 1 - j);
        x.min(y)
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
    village: Option<VillageGround>,
    /// Columns over which the village's ground eases into the natural ground.
    blend: i32,
    /// Columns at or next to a dug cell (meshed deeper).
    carved_near: HashSet<(i32, i32)>,
    /// Collision boxes of the village's buildings, props and tree trunks (surface frame).
    pub structures: Vec<crate::foundations::Structure>,
    /// The cube's faces: centre, x axis, y axis (surface frame, unit).
    faces: [[DVec3; 3]; 6],
    tops: RwLock<HashMap<(u8, i32, i32), Option<i32>>>,
    /// Where the player is (unit direction); `None` means everywhere.
    focus: RwLock<Option<DVec3>>,
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
    pub fn new(natural: Arc<Terrain>, center: DVec3, village: Option<VillageGround>) -> Self {
        let c = center.normalize();
        let (e, south) = tangent_frame(c);
        let n = -south;
        let h = natural.surface_height(c, 1.0);
        let base = (h / CELL).floor() * CELL;
        let carved_near = village
            .as_ref()
            .map(|v| {
                let mut s = HashSet::new();
                for cell in &v.carved {
                    for a in -1..=1 {
                        for b in -1..=1 {
                            s.insert((cell.x + a, cell.y + b));
                        }
                    }
                }
                s
            })
            .unwrap_or_default();
        Self {
            center: c,
            east: e,
            north: n,
            base,
            radius: natural.radius,
            natural,
            village,
            blend: 20,
            carved_near,
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
            focus: RwLock::new(None),
        }
    }

    /// Sets where the player is (unit direction, surface frame); `None` for everywhere.
    pub fn set_focus(&self, dir: Option<DVec3>) {
        if let Ok(mut f) = self.focus.write() {
            *f = dir.map(DVec3::normalize);
        }
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
        if let Ok(t) = self.tops.read()
            && let Some(&v) = t.get(&(face, i, j))
        {
            return v;
        }
        let natural = self.natural_top(face, i, j);
        let v = match &self.village {
            Some(v) if face == 0 && v.index(i, j).is_some() => {
                let own = v.tops[v.index(i, j).unwrap_or(0)];
                let w = (f64::from(v.inset(i, j)) / f64::from(self.blend)).clamp(0.0, 1.0);
                match natural {
                    Some(n) => {
                        Some((f64::from(n) + (f64::from(own) - f64::from(n)) * w).round() as i32)
                    }
                    None => Some(own),
                }
            }
            _ => natural,
        };
        if let Ok(mut t) = self.tops.write() {
            if t.len() > CACHE_MAX {
                t.clear();
            }
            t.insert((face, i, j), v);
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
        let (i, j) = Self::column(p.x, p.y);
        let t = self.top_on(face, i, j)?;
        Some(self.base + f64::from(t + 1) * CELL)
    }

    /// The ground surface height above the floor at face 0's grid point (x, y) (m; 0 where
    /// there are no blocks).
    pub fn ground_z(&self, x: f64, y: f64) -> f64 {
        let (i, j) = Self::column(x, y);
        self.top(i, j).map_or(0.0, |t| f64::from(t + 1) * CELL)
    }

    fn carved(&self, face: u8, p: IVec3) -> bool {
        face == 0 && self.village.as_ref().is_some_and(|v| v.carved.contains(&p))
    }

    /// The colour of the top cell of column (i, j) of `face`: the village's own, rock where
    /// steep, beaches by the sea, else from what grows there (the world's cover).
    fn top_colour(&self, face: u8, i: i32, j: i32, t: i32) -> u16 {
        if face == 0
            && let Some(v) = &self.village
            && let Some(k) = v.index(i, j)
            && v.inset(i, j) > self.blend / 2
        {
            return colour(&v.colours[k]);
        }
        let speckle = f64::from((i.wrapping_mul(73_856_093) ^ j.wrapping_mul(19_349_663)) & 255)
            / 255.0
            - 0.5;
        let n = crate::noise::fbm(
            DVec3::new(
                f64::from(i) * 0.075,
                f64::from(j) * 0.075,
                9.0 + f64::from(face),
            ),
            7,
            2,
            0.5,
        ) + 0.15 * speckle;
        let steep = [(1, 0), (-1, 0), (0, 1), (0, -1)]
            .iter()
            .filter_map(|&(a, b)| self.top_on(face, i + a, j + b))
            .any(|m| (m - t).abs() >= 3);
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

    /// The lowest cell worth meshing in column (i, j): one below the lowest neighbouring top
    /// (deeper where something is dug).
    fn floor(&self, face: u8, i: i32, j: i32) -> Option<i32> {
        let t = self.top_on(face, i, j)?;
        let mut lo = t;
        for a in -1..=1 {
            for b in -1..=1 {
                if let Some(n) = self.top_on(face, i + a, j + b) {
                    lo = lo.min(n);
                }
            }
        }
        let deep = face == 0 && self.carved_near.contains(&(i, j));
        Some(if deep { lo.min(t - 28) } else { lo - 1 })
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
        let (face, ci, cj) = key;
        let (i0, j0) = (ci * CHUNK, cj * CHUNK);
        // The shell cells of the chunk and a margin round it (shapes and hidden faces look
        // two cells out).
        let mut cells: HashMap<IVec3, u16> = HashMap::new();
        let (mut klo, mut khi) = (i32::MAX, i32::MIN);
        for j in j0 - 3..j0 + CHUNK + 3 {
            for i in i0 - 3..i0 + CHUNK + 3 {
                let (Some(t), Some(lo)) = (self.top_on(face, i, j), self.floor(face, i, j)) else {
                    continue;
                };
                for k in lo..=t {
                    let p = IVec3::new(i, j, k);
                    let c = self.cell_on(face, p);
                    if c != 0 {
                        cells.insert(p, c);
                    }
                }
                // Below the shell counts as solid, so its faces stay hidden.
                for k in lo - 2..lo {
                    cells.insert(IVec3::new(i, j, k), colour("bedrock"));
                }
                if (i0..i0 + CHUNK).contains(&i) && (j0..j0 + CHUNK).contains(&j) {
                    klo = klo.min(lo);
                    khi = khi.max(t);
                }
            }
        }
        if klo > khi {
            return None;
        }
        let shell = |p: IVec3| -> u16 { Cells::get(&cells, p) };
        let mut mesh = sim_blocks::mesh(
            &shell,
            IVec3::new(i0, j0, klo),
            IVec3::new(i0 + CHUNK, j0 + CHUNK, khi + 1),
            &[Dir::PZ],
            CELL as f32,
        );
        if mesh.is_empty() {
            return None;
        }
        // Into the chunk's own frame: origin at its centre on the floor, up there.
        let up = self.chunk_centre(key);
        let origin = up * (self.radius + self.base);
        let e = self.faces[face as usize][1];
        let east = (e - up * e.dot(up)).normalize();
        let south = east.cross(up);
        let basis = DQuat::from_mat3(&DMat3::from_cols(east, up, south));
        let inverse = basis.inverse();
        for p in &mut mesh.positions {
            let g = DVec3::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2]));
            *p = (inverse * (self.point_on(face, g) - origin))
                .as_vec3()
                .to_array();
        }
        for n in &mut mesh.normals {
            *n = Vec3::new(n[0], n[2], -n[1]).to_array();
        }
        Some(ChunkMesh {
            mesh,
            origin,
            basis,
        })
    }

    /// The chunks within `radius` (m) of unit direction `dir`, on whichever faces that disc
    /// touches.
    pub fn chunks_around(&self, dir: DVec3, radius: f64) -> Vec<ChunkKey> {
        let d = dir.normalize();
        let size = Self::chunk_size();
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
                    face,
                    (p.x / size).floor() as i32,
                    (p.y / size).floor() as i32,
                ));
            }
        }
        let mut out: Vec<ChunkKey> = keys
            .into_iter()
            .filter(|&k| {
                self.chunk_centre(k).angle_between(d) * self.radius <= radius + size * 0.71
            })
            .collect();
        out.sort_unstable();
        out
    }
}
