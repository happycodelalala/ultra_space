//! The mesh the world is generated on: a subdivided icosahedron, with its subdivision levels
//! kept so a direction finds its triangle in a few steps.

use std::collections::HashMap;

use glam::{DMat3, DVec3};

/// An icosphere: unit points, neighbours (compressed rows) and the faces of every level.
pub struct Sphere {
    pub points: Vec<DVec3>,
    start: Vec<u32>,
    adj: Vec<u32>,
    /// Faces per level; the children of face `f` at level `k` are `4f..4f+4` at level `k + 1`.
    levels: Vec<Vec<[u32; 3]>>,
    /// Mean distance between neighbours on the unit sphere.
    pub spacing: f64,
}

impl Sphere {
    /// An icosahedron subdivided `level` times: `10·4^level + 2` points.
    pub fn new(level: u32) -> Self {
        let t = (1.0 + 5f64.sqrt()) / 2.0;
        let mut points: Vec<DVec3> = [
            (-1.0, t, 0.0),
            (1.0, t, 0.0),
            (-1.0, -t, 0.0),
            (1.0, -t, 0.0),
            (0.0, -1.0, t),
            (0.0, 1.0, t),
            (0.0, -1.0, -t),
            (0.0, 1.0, -t),
            (t, 0.0, -1.0),
            (t, 0.0, 1.0),
            (-t, 0.0, -1.0),
            (-t, 0.0, 1.0),
        ]
        .iter()
        .map(|&(x, y, z)| DVec3::new(x, y, z).normalize())
        .collect();
        let mut faces: Vec<[u32; 3]> = vec![
            [0, 11, 5],
            [0, 5, 1],
            [0, 1, 7],
            [0, 7, 10],
            [0, 10, 11],
            [1, 5, 9],
            [5, 11, 4],
            [11, 10, 2],
            [10, 7, 6],
            [7, 1, 8],
            [3, 9, 4],
            [3, 4, 2],
            [3, 2, 6],
            [3, 6, 8],
            [3, 8, 9],
            [4, 9, 5],
            [2, 4, 11],
            [6, 2, 10],
            [8, 6, 7],
            [9, 8, 1],
        ];
        // Counter-clockwise seen from outside.
        for f in &mut faces {
            let (a, b, c) = (
                points[f[0] as usize],
                points[f[1] as usize],
                points[f[2] as usize],
            );
            if (b - a).cross(c - a).dot(a + b + c) < 0.0 {
                f.swap(1, 2);
            }
        }
        let mut levels = vec![faces];
        for _ in 0..level {
            let mut cache: HashMap<(u32, u32), u32> = HashMap::new();
            let mut mid = |a: u32, b: u32, points: &mut Vec<DVec3>| {
                let key = (a.min(b), a.max(b));
                *cache.entry(key).or_insert_with(|| {
                    points.push((points[a as usize] + points[b as usize]).normalize());
                    (points.len() - 1) as u32
                })
            };
            let last = levels.last().expect("a level");
            let mut next = Vec::with_capacity(last.len() * 4);
            for &[a, b, c] in last {
                let ab = mid(a, b, &mut points);
                let bc = mid(b, c, &mut points);
                let ca = mid(c, a, &mut points);
                next.extend([[a, ab, ca], [ab, b, bc], [ca, bc, c], [ab, bc, ca]]);
            }
            levels.push(next);
        }
        let n = points.len();
        let mut lists: Vec<Vec<u32>> = vec![Vec::with_capacity(6); n];
        for &[a, b, c] in levels.last().expect("a level") {
            for (x, y) in [(a, b), (b, c), (c, a)] {
                lists[x as usize].push(y);
                lists[y as usize].push(x);
            }
        }
        let mut start = Vec::with_capacity(n + 1);
        let mut adj = Vec::new();
        let mut total = 0.0;
        start.push(0);
        for (i, l) in lists.iter_mut().enumerate() {
            l.sort_unstable();
            l.dedup();
            for &j in l.iter() {
                total += points[i].distance(points[j as usize]);
            }
            adj.extend_from_slice(l);
            start.push(adj.len() as u32);
        }
        let spacing = total / adj.len() as f64;
        Self {
            points,
            start,
            adj,
            levels,
            spacing,
        }
    }

    pub fn len(&self) -> usize {
        self.points.len()
    }

    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    /// The neighbours of point `i`.
    pub fn neighbours(&self, i: usize) -> &[u32] {
        &self.adj[self.start[i] as usize..self.start[i + 1] as usize]
    }

    /// The mean of a field over each point's neighbours.
    pub fn neighbour_mean(&self, f: &[f64]) -> Vec<f64> {
        (0..self.len())
            .map(|i| {
                let nb = self.neighbours(i);
                nb.iter().map(|&j| f[j as usize]).sum::<f64>() / nb.len() as f64
            })
            .collect()
    }

    /// The triangle holding the unit direction `p` and its weights there (summing to 1).
    pub fn locate(&self, p: DVec3) -> ([u32; 3], [f64; 3]) {
        let inside = |f: &[u32; 3]| {
            let (a, b, c) = (
                self.points[f[0] as usize],
                self.points[f[1] as usize],
                self.points[f[2] as usize],
            );
            p.dot(a.cross(b))
                .min(p.dot(b.cross(c)))
                .min(p.dot(c.cross(a)))
        };
        let pick = |faces: &[[u32; 3]], range: std::ops::Range<usize>| {
            range
                .max_by(|&x, &y| inside(&faces[x]).total_cmp(&inside(&faces[y])))
                .expect("faces")
        };
        let mut f = pick(&self.levels[0], 0..self.levels[0].len());
        for level in &self.levels[1..] {
            f = pick(level, 4 * f..4 * f + 4);
        }
        let face = self.levels.last().expect("a level")[f];
        let m = DMat3::from_cols(
            self.points[face[0] as usize],
            self.points[face[1] as usize],
            self.points[face[2] as usize],
        );
        let w = (m.inverse() * p).max(DVec3::ZERO);
        let sum = (w.x + w.y + w.z).max(1e-12);
        (face, [w.x / sum, w.y / sum, w.z / sum])
    }

    /// A field interpolated at the unit direction `p`.
    pub fn sample(&self, f: &[f64], p: DVec3) -> f64 {
        let (face, w) = self.locate(p);
        (0..3).map(|k| f[face[k] as usize] * w[k]).sum()
    }

    /// The point nearest the unit direction `p`.
    pub fn nearest(&self, p: DVec3) -> usize {
        let (face, w) = self.locate(p);
        let k = (0..3).max_by(|&a, &b| w[a].total_cmp(&w[b])).unwrap_or(0);
        face[k] as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn points_find_their_triangles() {
        let s = Sphere::new(4);
        assert_eq!(s.len(), 10 * 4usize.pow(4) + 2);
        for (i, &p) in s.points.iter().enumerate().step_by(37) {
            assert_eq!(s.nearest(p), i);
        }
        let p = DVec3::new(0.3, 0.8, -0.2).normalize();
        let (face, w) = s.locate(p);
        let back: DVec3 = (0..3).map(|k| s.points[face[k] as usize] * w[k]).sum();
        assert!(back.normalize().distance(p) < 1e-9);
    }
}
