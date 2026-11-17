//! The ground as blocks near the demo village (Batch 5, task 5.3; design: block-world.md).
//!
//! Inside a disc round the village the ground is columns of 25 cm blocks: a column's top is the
//! natural terrain stepped to the grid, or, in the village's square, the ground its layout
//! exported (levelled pads, paths, the dug pit), easing into the natural ground at its edge.
//! The grid lies in the village's tangent plane (x east, y north) and is mapped onto the sphere
//! through the plane (each column points away from the centre, so curvature is exact); for the
//! demo this one grid stands in for the planet-wide cube-sphere grid of block-world.md §5.
//!
//! The terrain reads its surface here (landing, walking's fallback contact, placing things),
//! and its collision patches sink below it so the blocks' own collision meshes carry contact.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};

use glam::{DQuat, DVec2, DVec3, IVec3, Vec3};
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
pub const PALETTE: [&str; 14] = [
    "turf", "turf2", "turf3", "path", "path2", "soil", "soil2", "bedrock", "bedrock2", "ore",
    "sand", "rock", "rock2", "tilled",
];

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

/// The block ground of one disc.
pub struct BlockGround {
    /// Unit direction of the village centre (surface frame).
    pub center: DVec3,
    pub east: DVec3,
    pub north: DVec3,
    /// Height of the grid's k = 0 floor (m above the body's reference radius).
    pub base: f64,
    /// Radius of the disc (m).
    pub reach: f64,
    pub radius: f64,
    natural: Arc<Terrain>,
    village: Option<VillageGround>,
    /// Columns over which the village's ground eases into the natural ground.
    blend: i32,
    /// Columns at or next to a dug cell (meshed deeper).
    carved_near: HashSet<(i32, i32)>,
    /// Collision boxes of the village's buildings, props and tree trunks (surface frame).
    pub structures: Vec<crate::foundations::Structure>,
    tops: RwLock<HashMap<(i32, i32), Option<i32>>>,
    /// On a body of plains: the plain's normal and the grid's origin (surface frame, m). The
    /// grid then lies exactly in the plain, columns along its normal (block-world.md §5.1);
    /// otherwise it is the tangent plane bent onto the sphere at the vertices.
    flat: Option<(DVec3, DVec3)>,
}

impl std::fmt::Debug for BlockGround {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BlockGround")
            .field("center", &self.center)
            .field("base", &self.base)
            .field("reach", &self.reach)
            .finish_non_exhaustive()
    }
}

impl BlockGround {
    /// The block ground round `center` (unit direction) out to `reach` metres, over `natural`,
    /// with the village's ground in the middle; its floor at the natural height of the centre
    /// stepped down to the grid.
    pub fn new(
        natural: Arc<Terrain>,
        center: DVec3,
        reach: f64,
        village: Option<VillageGround>,
    ) -> Self {
        let center = center.normalize();
        let (mut east, south) = tangent_frame(center);
        let mut north = -south;
        let mut flat = None;
        let h = natural.surface_height(center, 1.0);
        let mut base = (h / CELL).floor() * CELL;
        if let Some((_, plain)) = natural.plain_at(center) {
            // Heights along the plain's normal, the grid along its edge.
            let e = natural.sample(center, 1.0).elevation;
            base = (e / CELL).floor() * CELL;
            east = plain.east;
            north = plain.north;
            let under = plain.to_local(center * (plain.distance / center.dot(plain.normal)));
            flat = Some((
                plain.normal,
                plain.from_local(DVec3::new(under.x, under.y, base)),
            ));
        }
        let carved_near = village
            .as_ref()
            .map(|v| {
                let mut s = HashSet::new();
                for c in &v.carved {
                    for a in -1..=1 {
                        for b in -1..=1 {
                            s.insert((c.x + a, c.y + b));
                        }
                    }
                }
                s
            })
            .unwrap_or_default();
        Self {
            center,
            east,
            north,
            base,
            reach,
            radius: natural.radius,
            natural,
            village,
            blend: 20,
            carved_near,
            structures: Vec::new(),
            tops: RwLock::new(HashMap::new()),
            flat,
        }
    }

    /// Adds a collision box: centre (grid point, m), turn about the vertical (radians,
    /// counter-clockwise from above) and half extents (x east, y north, z up before the turn).
    pub fn add_box(&mut self, center: DVec3, yaw: f64, half: DVec3) {
        let plane = self.basis();
        let turned = if self.flat.is_some() {
            plane
        } else {
            let up = self.dir(center.x, center.y);
            DQuat::from_rotation_arc(self.center, up) * plane
        };
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
        let up = self.flat.map_or(self.center, |(n, _)| n);
        DQuat::from_mat3(&glam::DMat3::from_cols(self.east, up, -self.north))
    }

    /// "Up" at a point of the grid (surface frame): the plain's normal, or away from the centre.
    pub fn up_at(&self, point: DVec3) -> DVec3 {
        self.flat.map_or_else(|| point.normalize(), |(n, _)| n)
    }

    /// The village frame's origin (surface frame, m): the centre at the grid's floor.
    pub fn origin(&self) -> DVec3 {
        match self.flat {
            Some((_, o)) => o,
            None => self.center * (self.radius + self.base),
        }
    }

    /// Tangent-plane point (x east, y north; m) under unit direction `dir`, if in the disc.
    pub fn plane(&self, dir: DVec3) -> Option<DVec2> {
        if let Some((n, o)) = self.flat {
            let c = dir.dot(n);
            if c <= 0.0 {
                return None;
            }
            let p = dir * (o.dot(n) / c) - o;
            let q = DVec2::new(p.dot(self.east), p.dot(self.north));
            return (q.length() <= self.reach).then_some(q);
        }
        let c = dir.dot(self.center);
        if c <= 0.0 {
            return None;
        }
        let p = dir * (self.radius / c) - self.center * self.radius;
        let q = DVec2::new(p.dot(self.east), p.dot(self.north));
        (q.length() <= self.reach).then_some(q)
    }

    /// Unit direction through tangent-plane point (x, y).
    pub fn dir(&self, x: f64, y: f64) -> DVec3 {
        match self.flat {
            Some((_, o)) => (o + self.east * x + self.north * y).normalize(),
            None => (self.center * self.radius + self.east * x + self.north * y).normalize(),
        }
    }

    /// Surface-frame position of grid point (x east, y north, z up from the floor; m).
    pub fn point(&self, p: DVec3) -> DVec3 {
        match self.flat {
            Some((n, o)) => o + self.east * p.x + self.north * p.y + n * p.z,
            None => self.dir(p.x, p.y) * (self.radius + self.base + p.z),
        }
    }

    /// The village frame's coordinates (x east, y up, z south) of grid point `p`.
    pub fn local(&self, p: DVec3) -> DVec3 {
        self.basis().inverse() * (self.point(p) - self.origin())
    }

    fn natural_top(&self, i: i32, j: i32) -> Option<i32> {
        let d = self.dir((f64::from(i) + 0.5) * CELL, (f64::from(j) + 0.5) * CELL);
        // Detail finer than a few blocks can't show in the steps (and costs octaves).
        let h = if self.flat.is_some() {
            self.natural.sample(d, 4.0).elevation
        } else {
            self.natural.height(d, 4.0)
        };
        if self.natural.has_ocean() && h < 0.3 {
            return None; // water: no blocks (the sea and its floor stay as they are)
        }
        Some(((h - self.base) / CELL).floor() as i32)
    }

    /// The top cell of column (i, j), or `None` where there are no blocks.
    pub fn top(&self, i: i32, j: i32) -> Option<i32> {
        if let Ok(t) = self.tops.read()
            && let Some(&v) = t.get(&(i, j))
        {
            return v;
        }
        let (x, y) = ((f64::from(i) + 0.5) * CELL, (f64::from(j) + 0.5) * CELL);
        let v = if x * x + y * y > self.reach * self.reach {
            None
        } else {
            let natural = self.natural_top(i, j);
            match &self.village {
                Some(v) if v.index(i, j).is_some() => {
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
            }
        };
        if let Ok(mut t) = self.tops.write() {
            t.insert((i, j), v);
        }
        v
    }

    /// The column (i, j) of tangent-plane point (x, y).
    pub fn column(x: f64, y: f64) -> (i32, i32) {
        ((x / CELL).floor() as i32, (y / CELL).floor() as i32)
    }

    /// The block surface's height (m above the reference radius) under `dir`, if in the disc.
    pub fn surface(&self, dir: DVec3) -> Option<f64> {
        let p = self.plane(dir)?;
        let (i, j) = Self::column(p.x, p.y);
        let t = self.top(i, j)?;
        let z = f64::from(t + 1) * CELL;
        match self.flat {
            // Where the ray meets the plane of the block tops.
            Some((n, o)) => Some((o.dot(n) + z) / dir.normalize().dot(n) - self.radius),
            None => Some(self.base + z),
        }
    }

    /// The ground surface height above the floor at plane point (x, y) (m; 0 outside).
    pub fn ground_z(&self, x: f64, y: f64) -> f64 {
        let (i, j) = Self::column(x, y);
        self.top(i, j).map_or(0.0, |t| f64::from(t + 1) * CELL)
    }

    fn carved(&self, p: IVec3) -> bool {
        self.village.as_ref().is_some_and(|v| v.carved.contains(&p))
    }

    /// The colour of the top cell of column (i, j).
    fn top_colour(&self, i: i32, j: i32, t: i32) -> u16 {
        if let Some(v) = &self.village
            && let Some(k) = v.index(i, j)
            && v.inset(i, j) > self.blend / 2
        {
            return colour(&v.colours[k]);
        }
        // Steep: rock; low by the sea: sand; else grass in patches.
        let steep = [(1, 0), (-1, 0), (0, 1), (0, -1)]
            .iter()
            .filter_map(|&(a, b)| self.top(i + a, j + b))
            .any(|n| (n - t).abs() >= 3);
        if steep {
            return colour(if (i + j) % 3 == 0 { "rock2" } else { "rock" });
        }
        if self.base + f64::from(t) * CELL < 1.5 && self.natural.has_ocean() {
            return colour("sand");
        }
        let n = crate::noise::fbm(
            DVec3::new(f64::from(i) * 0.075, f64::from(j) * 0.075, 9.0),
            7,
            2,
            0.5,
        ) + 0.15
            * (f64::from((i.wrapping_mul(73_856_093) ^ j.wrapping_mul(19_349_663)) & 255) / 255.0
                - 0.5);
        colour(if n < -0.12 {
            "turf3"
        } else if n < 0.1 {
            "turf"
        } else {
            "turf2"
        })
    }

    /// The cell at (i, j, k): 0 empty, else a 1-based index into [`PALETTE`].
    pub fn cell(&self, p: IVec3) -> u16 {
        let Some(t) = self.top(p.x, p.y) else {
            return 0;
        };
        if p.z > t || self.carved(p) {
            return 0;
        }
        // Only the shell is stored: the cells below the lowest neighbouring top (and deep
        // round dug spaces) are never seen, but kept solid so nothing shows through.
        let d = t - p.z;
        if d == 0 {
            return self.top_colour(p.x, p.y, t);
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

    /// The lowest cell worth meshing in column (i, j): one below the lowest neighbouring top
    /// (deeper where something is dug).
    fn floor(&self, i: i32, j: i32) -> Option<i32> {
        let t = self.top(i, j)?;
        let mut lo = t;
        for a in -1..=1 {
            for b in -1..=1 {
                if let Some(n) = self.top(i + a, j + b) {
                    lo = lo.min(n);
                }
            }
        }
        let deep = self.carved_near.contains(&(i, j));
        Some(if deep { lo.min(t - 28) } else { lo - 1 })
    }

    /// The mesh of chunk (ci, cj): positions and normals in the village frame (x east, y up,
    /// z south; m), colours 1-based into [`PALETTE`]. `None` when it has no blocks.
    pub fn chunk(&self, ci: i32, cj: i32) -> Option<MeshData> {
        let (i0, j0) = (ci * CHUNK, cj * CHUNK);
        // The shell cells of the chunk and a margin round it (shapes and hidden faces look
        // two cells out).
        let mut cells: HashMap<IVec3, u16> = HashMap::new();
        let (mut klo, mut khi) = (i32::MAX, i32::MIN);
        for j in j0 - 3..j0 + CHUNK + 3 {
            for i in i0 - 3..i0 + CHUNK + 3 {
                let (Some(t), Some(lo)) = (self.top(i, j), self.floor(i, j)) else {
                    continue;
                };
                for k in lo..=t {
                    let p = IVec3::new(i, j, k);
                    let c = self.cell(p);
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
        let m = sim_blocks::mesh(
            &shell,
            IVec3::new(i0, j0, klo),
            IVec3::new(i0 + CHUNK, j0 + CHUNK, khi + 1),
            &[Dir::PZ],
            CELL as f32,
        );
        if m.is_empty() {
            return None;
        }
        Some(self.curve(m))
    }

    /// Mesh positions from grid metres (x east, y north, z up) to the village frame, following
    /// the sphere.
    pub fn curve(&self, mut m: MeshData) -> MeshData {
        let basis = self.basis().inverse();
        let origin = self.origin();
        for p in &mut m.positions {
            let g = DVec3::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2]));
            *p = (basis * (self.point(g) - origin)).as_vec3().to_array();
        }
        for n in &mut m.normals {
            *n = Vec3::new(n[0], n[2], -n[1]).to_array();
        }
        m
    }

    /// Chunk keys within `radius` (m) of tangent-plane point `p`.
    pub fn chunks_near(&self, p: DVec2, radius: f64) -> Vec<(i32, i32)> {
        let size = f64::from(CHUNK) * CELL;
        let lo = ((p - DVec2::splat(radius)) / size).floor();
        let hi = ((p + DVec2::splat(radius)) / size).floor();
        let mut out = Vec::new();
        for cj in lo.y as i32..=hi.y as i32 {
            for ci in lo.x as i32..=hi.x as i32 {
                let centre = DVec2::new((f64::from(ci) + 0.5) * size, (f64::from(cj) + 0.5) * size);
                if centre.distance(p) <= radius + size * 0.71
                    && centre.length() <= self.reach + size
                {
                    out.push((ci, cj));
                }
            }
        }
        out
    }
}
