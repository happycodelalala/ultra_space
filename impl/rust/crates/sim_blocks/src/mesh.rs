//! Meshing: the faces between solid and empty cells (block-world.md section 7).
//!
//! Full cells' faces in one plane and colour merge into the largest rectangles (greedy meshing);
//! the slopes of wedges in one plane and colour merge the same way; corners and the ends of
//! slopes are drawn per cell. A face is skipped when the neighbouring cell's face against it
//! covers the whole square, so nothing hidden is drawn.

use std::collections::HashMap;

use glam::{IVec3, Vec3};

use crate::grid::Cells;
use crate::shape::{Dir, RING, Shape, shapes};

/// A mesh in the model's frame (metres): flat-shaded faces, one palette index per vertex.
#[derive(Clone, Debug, Default)]
pub struct MeshData {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub colours: Vec<u16>,
    pub indices: Vec<u32>,
}

impl MeshData {
    pub fn triangles(&self) -> usize {
        self.indices.len() / 3
    }

    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }

    /// Adds a convex polygon (counter-clockwise seen from outside).
    fn poly(&mut self, pts: &[Vec3], colour: u16) {
        let n = (pts[1] - pts[0]).cross(pts[2] - pts[0]).normalize_or_zero();
        let base = self.positions.len() as u32;
        for p in pts {
            self.positions.push(p.to_array());
            self.normals.push(n.to_array());
            self.colours.push(colour);
        }
        for k in 1..pts.len() as u32 - 1 {
            self.indices
                .extend_from_slice(&[base, base + k, base + k + 1]);
        }
    }

    /// Visible area per palette index (m²), for comparing meshers.
    pub fn area_by_colour(&self) -> HashMap<u16, f64> {
        let mut out = HashMap::new();
        for t in self.indices.chunks(3) {
            let p = |i: u32| Vec3::from_array(self.positions[i as usize]).as_dvec3();
            let a = (p(t[1]) - p(t[0])).cross(p(t[2]) - p(t[0])).length() * 0.5;
            *out.entry(self.colours[t[0] as usize]).or_insert(0.0) += a;
        }
        out
    }

    /// Appends another mesh.
    pub fn append(&mut self, other: &MeshData) {
        let base = self.positions.len() as u32;
        self.positions.extend_from_slice(&other.positions);
        self.normals.extend_from_slice(&other.normals);
        self.colours.extend_from_slice(&other.colours);
        self.indices.extend(other.indices.iter().map(|i| i + base));
    }
}

/// Faces of full cells by plane (axis, sign, slice): (u, v) -> colour.
type Planes = HashMap<(u8, i8, i32), HashMap<(i32, i32), u16>>;
/// Wedge slopes by plane (up, low direction, colour, plane constant): (run, slope index).
type Slopes = HashMap<(Dir, Dir, u16, i32), Vec<(i32, i32)>>;

/// Adds `pts` wound so its normal points along `out`.
fn oriented(m: &mut MeshData, mut pts: Vec<Vec3>, out: Vec3, colour: u16) {
    if pts.len() < 3 {
        return;
    }
    let n = (pts[1] - pts[0]).cross(pts[2] - pts[0]);
    if n.length_squared() < 1e-12 {
        return;
    }
    if n.dot(out) < 0.0 {
        pts.reverse();
    }
    m.poly(&pts, colour);
}

fn unit(d: Dir) -> Vec3 {
    d.vec().as_vec3()
}

/// Meshes the cells in [`lo`, `hi`) of `cells` (cells outside are read only to hide faces and
/// shape the cells inside), shaped by `ups`, with blocks of `size` metres.
pub fn mesh(cells: &impl Cells, lo: IVec3, hi: IVec3, ups: &[Dir], size: f32) -> MeshData {
    let shp = if ups.is_empty() {
        HashMap::new()
    } else {
        shapes(cells, lo - IVec3::ONE, hi + IVec3::ONE, ups)
    };
    // Whether the cell at `n` covers its whole face toward `d`.
    let covers = |n: IVec3, d: Dir| cells.solid(n) && shp.get(&n).is_none_or(|s| s.face_full(d));
    let mut m = MeshData::default();

    // Full cells: faces grouped by plane, merged greedily.
    let mut planes: Planes = HashMap::new();
    // Wedge slopes grouped by plane: (up, low dir, colour, plane constant) -> (run, slope index).
    let mut slopes: Slopes = HashMap::new();
    for z in lo.z..hi.z {
        for y in lo.y..hi.y {
            for x in lo.x..hi.x {
                let c = IVec3::new(x, y, z);
                let col = cells.get(c);
                if col == 0 {
                    continue;
                }
                match shp.get(&c) {
                    None => {
                        for axis in 0..3u8 {
                            for sign in [1i8, -1] {
                                let d = Dir::new(axis, sign);
                                if covers(c + d.vec(), d.flip()) {
                                    continue;
                                }
                                let (u, w) = d.across();
                                planes
                                    .entry((axis, sign, c[axis as usize]))
                                    .or_default()
                                    .insert((c[u], c[w]), col);
                            }
                        }
                    }
                    Some(s) => shaped(&mut m, c, s, col, &covers, size, &mut slopes),
                }
            }
        }
    }
    for ((axis, sign, slice), faces) in planes {
        let d = Dir::new(axis, sign);
        let (u, w) = d.across();
        let plane = (slice + i32::from(sign > 0)) as f32;
        for (u0, v0, wd, ht, col) in greedy(&faces) {
            let corner = |cu: i32, cv: i32| {
                let mut p = Vec3::ZERO;
                p[axis as usize] = plane;
                p[u] = cu as f32;
                p[w] = cv as f32;
                p * size
            };
            let pts = vec![
                corner(u0, v0),
                corner(u0 + wd, v0),
                corner(u0 + wd, v0 + ht),
                corner(u0, v0 + ht),
            ];
            oriented(&mut m, pts, unit(d), col);
        }
    }
    for ((up, lowd, col, k), cellsxy) in slopes {
        let (pa, qa) = up.across();
        let l = lowd.axis as usize;
        let r = if l == pa { qa } else { pa };
        let a = up.axis as usize;
        let (sg, dl) = (up.sign as i32, lowd.sign as i32);
        let faces: HashMap<(i32, i32), u16> = cellsxy.into_iter().map(|rs| (rs, col)).collect();
        for (r0, s0, wd, ht, _) in greedy(&faces) {
            // The cell at slope index s, and a point on its top face.
            let point = |run: i32, s: i32, high: bool| {
                let cl = s * dl;
                let ca = sg * (k - dl * cl);
                let mut p = Vec3::ZERO;
                p[r] = run as f32;
                p[l] = cl as f32 + if high == (dl > 0) { 0.0 } else { 1.0 };
                p[a] = ca as f32 + if high == (sg > 0) { 1.0 } else { 0.0 };
                p * size
            };
            let s1 = s0 + ht - 1;
            let pts = vec![
                point(r0, s0, true),
                point(r0 + wd, s0, true),
                point(r0 + wd, s1, false),
                point(r0, s1, false),
            ];
            oriented(&mut m, pts, unit(up), col);
        }
    }
    m
}

/// Greedy rectangles over a set of (u, v) cells with colours: (u0, v0, width, height, colour).
fn greedy(faces: &HashMap<(i32, i32), u16>) -> Vec<(i32, i32, i32, i32, u16)> {
    let mut keys: Vec<(i32, i32)> = faces.keys().copied().collect();
    keys.sort_unstable_by_key(|&(u, v)| (v, u));
    let mut done = std::collections::HashSet::new();
    let mut out = Vec::new();
    for (u0, v0) in keys {
        if done.contains(&(u0, v0)) {
            continue;
        }
        let c = faces[&(u0, v0)];
        let same = |p: (i32, i32), done: &std::collections::HashSet<(i32, i32)>| {
            faces.get(&p) == Some(&c) && !done.contains(&p)
        };
        let mut wd = 1;
        while same((u0 + wd, v0), &done) {
            wd += 1;
        }
        let mut ht = 1;
        while (0..wd).all(|du| same((u0 + du, v0 + ht), &done)) {
            ht += 1;
        }
        for du in 0..wd {
            for dv in 0..ht {
                done.insert((u0 + du, v0 + dv));
            }
        }
        out.push((u0, v0, wd, ht, c));
    }
    out
}

/// The faces of one shaped cell: its bottom and sides (unless covered), its top (wedge slopes
/// collected for merging; corners drawn as two triangles).
#[allow(clippy::too_many_arguments)]
fn shaped(
    m: &mut MeshData,
    c: IVec3,
    s: &Shape,
    col: u16,
    covers: &impl Fn(IVec3, Dir) -> bool,
    size: f32,
    slopes: &mut Slopes,
) {
    let up = s.up;
    let (pa, qa) = up.across();
    let a = up.axis as usize;
    let pt = |cp: i32, cq: i32, h: bool| {
        let mut p = c.as_vec3();
        p[pa] += cp as f32;
        p[qa] += cq as f32;
        p[a] += if h == (up.sign > 0) { 1.0 } else { 0.0 };
        p * size
    };
    let down = up.flip();
    if !covers(c + down.vec(), up) {
        oriented(
            m,
            RING.iter().map(|&(p, q)| pt(p, q, false)).collect(),
            unit(down),
            col,
        );
    }
    for k in 0..4 {
        let (mi, ni) = (k, (k + 1) % 4);
        let ((mp, mq), (np, nq)) = (RING[mi], RING[ni]);
        let d = if mp == np {
            Dir::new(pa as u8, if mp == 1 { 1 } else { -1 })
        } else {
            Dir::new(qa as u8, if mq == 1 { 1 } else { -1 })
        };
        if covers(c + d.vec(), d.flip()) {
            continue;
        }
        let mut pts = vec![pt(mp, mq, false), pt(np, nq, false)];
        if s.high[ni] {
            pts.push(pt(np, nq, true));
        }
        if s.high[mi] {
            pts.push(pt(mp, mq, true));
        }
        oriented(m, pts, unit(d), col);
    }
    let highs = s.highs();
    if highs == 2 {
        // A wedge: its slope joins others in the same plane.
        let lows: Vec<usize> = (0..4).filter(|&k| !s.high[k]).collect();
        let (c0, c1) = (RING[lows[0]], RING[lows[1]]);
        let lowd = if c0.0 == c1.0 {
            Dir::new(pa as u8, if c0.0 == 1 { 1 } else { -1 })
        } else {
            Dir::new(qa as u8, if c0.1 == 1 { 1 } else { -1 })
        };
        let l = lowd.axis as usize;
        let r = if l == pa { qa } else { pa };
        let k = up.sign as i32 * c[a] + lowd.sign as i32 * c[l];
        slopes
            .entry((up, lowd, col, k))
            .or_default()
            .push((c[r], lowd.sign as i32 * c[l]));
        return;
    }
    // Corners: split along the diagonal through the odd corner.
    let odd = (0..4).find(|&k| s.high[k] == (highs == 1)).unwrap_or(0);
    let t: Vec<Vec3> = (0..4)
        .map(|k| pt(RING[k].0, RING[k].1, s.high[k]))
        .collect();
    for tri in [[odd, odd + 1, odd + 2], [odd, odd + 2, odd + 3]] {
        oriented(m, tri.iter().map(|&k| t[k % 4]).collect(), unit(up), col);
    }
}
