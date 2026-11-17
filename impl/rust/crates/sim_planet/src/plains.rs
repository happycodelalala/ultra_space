//! A round body as a polyhedron of flat plains (block-world.md §5.1).
//!
//! Each plain is a flat polygon in its own plane (`normal · p = distance`, surface frame, m). The
//! body's base surface is where a ray from the centre first meets one of the planes, so neighbouring
//! plains meet along straight creases. Ground at an elevation `e` above a plain lies on the plane
//! moved out by `e`; taking the nearest moved plane along each ray keeps the ground continuous over
//! creases wherever the elevation itself is continuous.
//!
//! The layout so far is the truncated icosahedron (the "football": 12 pentagons, 20 hexagons); the
//! general Goldberg layouts for larger bodies come when a body first needs them.

use glam::DVec3;

/// Pentagon or hexagon.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlainKind {
    Pentagon,
    Hexagon,
}

/// One flat plain.
#[derive(Clone, Debug)]
pub struct Plain {
    /// Outward unit normal (surface frame).
    pub normal: DVec3,
    /// Distance of the plane from the body's centre (m).
    pub distance: f64,
    /// Corners on the plane, counter-clockwise seen from outside (m, surface frame).
    pub corners: Vec<DVec3>,
    /// The plain across each edge: `neighbours[k]` shares the edge from `corners[k]` to
    /// `corners[k + 1]`.
    pub neighbours: Vec<usize>,
    pub kind: PlainKind,
    /// The plain's grid axes: `east` along its first edge, `north = normal × east`.
    pub east: DVec3,
    pub north: DVec3,
}

impl Plain {
    /// The point of the plane nearest the centre (the origin of its local coordinates).
    pub fn origin(&self) -> DVec3 {
        self.normal * self.distance
    }

    /// Local coordinates (east, north, height above the plane) of a point.
    pub fn to_local(&self, p: DVec3) -> DVec3 {
        let d = p - self.origin();
        DVec3::new(d.dot(self.east), d.dot(self.north), d.dot(self.normal))
    }

    /// The point at local coordinates (east, north, height above the plane).
    pub fn from_local(&self, l: DVec3) -> DVec3 {
        self.origin() + self.east * l.x + self.north * l.y + self.normal * l.z
    }

    /// Distance (m, in the plane) from a point on or above the plain to its nearest edge;
    /// negative outside the polygon.
    pub fn edge_distance(&self, p: DVec3) -> f64 {
        let n = self.corners.len();
        (0..n)
            .map(|k| {
                let a = self.corners[k];
                let b = self.corners[(k + 1) % n];
                // Inward direction of the edge, in the plane.
                let inward = self.normal.cross(b - a).normalize();
                (p - a).dot(inward)
            })
            .fold(f64::INFINITY, f64::min)
    }
}

/// All plains of a body.
#[derive(Clone, Debug)]
pub struct Plains {
    pub plains: Vec<Plain>,
}

/// Where a ray from the centre meets the ground.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    /// The plain whose (moved) plane the ray meets first.
    pub plain: usize,
    /// Distance from the centre (m).
    pub radius: f64,
}

impl Plains {
    /// The truncated icosahedron with every edge `edge` metres long, turned so a pentagon faces
    /// +Y (the spin axis passes through two pentagons).
    pub fn truncated_icosahedron(edge: f64) -> Self {
        let phi = (1.0 + 5f64.sqrt()) / 2.0;
        // Vertices of the solid with edge 2: cyclic permutations of these, all sign changes.
        let mut verts = Vec::new();
        for base in [
            [0.0, 1.0, 3.0 * phi],
            [1.0, 2.0 + phi, 2.0 * phi],
            [phi, 2.0, 2.0 * phi + 1.0],
        ] {
            for shift in 0..3 {
                let p = [
                    base[shift % 3],
                    base[(shift + 1) % 3],
                    base[(shift + 2) % 3],
                ];
                for signs in 0..8 {
                    let v = DVec3::new(
                        if signs & 1 == 0 { p[0] } else { -p[0] },
                        if signs & 2 == 0 { p[1] } else { -p[1] },
                        if signs & 4 == 0 { p[2] } else { -p[2] },
                    );
                    if !verts.iter().any(|w: &DVec3| w.distance(v) < 1e-9) {
                        verts.push(v);
                    }
                }
            }
        }
        debug_assert_eq!(verts.len(), 60);
        // Face normals: icosahedron vertices (pentagons) and dodecahedron vertices (hexagons).
        let mut normals = Vec::new();
        for shift in 0..3 {
            for signs in 0..4 {
                let p = [
                    0.0,
                    if signs & 1 == 0 { 1.0 } else { -1.0 },
                    if signs & 2 == 0 { phi } else { -phi },
                ];
                let q = [p[shift % 3], p[(shift + 1) % 3], p[(shift + 2) % 3]];
                normals.push((DVec3::from_array(q).normalize(), PlainKind::Pentagon));
            }
        }
        for signs in 0..8 {
            let v = DVec3::new(
                if signs & 1 == 0 { 1.0 } else { -1.0 },
                if signs & 2 == 0 { 1.0 } else { -1.0 },
                if signs & 4 == 0 { 1.0 } else { -1.0 },
            );
            normals.push((v.normalize(), PlainKind::Hexagon));
        }
        for shift in 0..3 {
            for signs in 0..4 {
                let p = [
                    0.0,
                    if signs & 1 == 0 { phi } else { -phi },
                    if signs & 2 == 0 {
                        1.0 / phi
                    } else {
                        -1.0 / phi
                    },
                ];
                let q = [p[shift % 3], p[(shift + 1) % 3], p[(shift + 2) % 3]];
                normals.push((DVec3::from_array(q).normalize(), PlainKind::Hexagon));
            }
        }
        // Turn a pentagon to +Y, scale to the edge length.
        let turn = glam::DQuat::from_rotation_arc(normals[0].0, DVec3::Y);
        let scale = edge / 2.0;
        let verts: Vec<DVec3> = verts.iter().map(|&v| turn * v * scale).collect();
        let mut plains: Vec<Plain> = normals
            .iter()
            .map(|&(n, kind)| {
                let normal = turn * n;
                let distance = verts.iter().map(|v| v.dot(normal)).fold(f64::MIN, f64::max);
                let mut corners: Vec<DVec3> = verts
                    .iter()
                    .copied()
                    .filter(|v| (v.dot(normal) - distance).abs() < 1e-6 * scale)
                    .collect();
                let centre = normal * distance;
                let east0 = (corners[0] - centre).normalize();
                let north0 = normal.cross(east0);
                corners.sort_by(|a, b| {
                    let fa = (*a - centre).dot(north0).atan2((*a - centre).dot(east0));
                    let fb = (*b - centre).dot(north0).atan2((*b - centre).dot(east0));
                    fa.total_cmp(&fb)
                });
                let east = (corners[1] - corners[0]).normalize();
                Plain {
                    normal,
                    distance,
                    corners,
                    neighbours: Vec::new(),
                    kind,
                    east,
                    north: normal.cross(east),
                }
            })
            .collect();
        // Neighbours: the plain sharing both corners of each edge.
        let edges: Vec<Vec<(DVec3, DVec3)>> = plains
            .iter()
            .map(|p| {
                let n = p.corners.len();
                (0..n)
                    .map(|k| (p.corners[k], p.corners[(k + 1) % n]))
                    .collect()
            })
            .collect();
        for (i, plain) in plains.iter_mut().enumerate() {
            plain.neighbours = edges[i]
                .iter()
                .map(|&(a, b)| {
                    (0..edges.len())
                        .find(|&j| {
                            j != i
                                && edges[j].iter().any(|&(c, d)| {
                                    c.distance(b) < 1e-6 * scale && d.distance(a) < 1e-6 * scale
                                })
                        })
                        .expect("every edge has a plain across it")
                })
                .collect();
        }
        Self { plains }
    }

    /// The radius (m) at which every plain's plane is nearest the centre, averaged: a reference
    /// radius for the body.
    pub fn mean_distance(&self) -> f64 {
        self.plains.iter().map(|p| p.distance).sum::<f64>() / self.plains.len() as f64
    }

    /// Where the ray along the unit direction `dir` meets the ground at elevation `elevation` (m
    /// above every plane): the nearest of the moved planes.
    pub fn hit(&self, dir: DVec3, elevation: f64) -> Hit {
        let mut best = Hit {
            plain: 0,
            radius: f64::INFINITY,
        };
        for (i, p) in self.plains.iter().enumerate() {
            let c = dir.dot(p.normal);
            if c <= 1e-9 {
                continue;
            }
            let r = (p.distance + elevation) / c;
            if r < best.radius {
                best = Hit {
                    plain: i,
                    radius: r,
                };
            }
        }
        best
    }

    /// The plain under the unit direction `dir` (at elevation 0).
    pub fn locate(&self, dir: DVec3) -> usize {
        self.hit(dir, 0.0).plain
    }

    /// Blending weights of the plains near `dir` (summing to 1): a plain counts while the ray meets
    /// its plane within `band` metres of the nearest one, more the nearer it is, so the weights are
    /// equal on a crease and change smoothly across it.
    pub fn weights(&self, dir: DVec3, band: f64, out: &mut Vec<(usize, f64)>) {
        out.clear();
        let first = self.hit(dir, 0.0);
        let mut total = 0.0;
        for (i, p) in self.plains.iter().enumerate() {
            let c = dir.dot(p.normal);
            if c <= 1e-9 {
                continue;
            }
            let excess = p.distance / c - first.radius;
            if excess < band {
                let t = 1.0 - excess / band;
                let w = t * t * (3.0 - 2.0 * t);
                out.push((i, w));
                total += w;
            }
        }
        for (_, w) in out.iter_mut() {
            *w /= total;
        }
    }

    /// The edge length (m), from the first edge.
    pub fn edge(&self) -> f64 {
        let p = &self.plains[0];
        p.corners[0].distance(p.corners[1])
    }

    /// The largest distance of any corner from the centre (m).
    pub fn corner_radius(&self) -> f64 {
        self.plains
            .iter()
            .flat_map(|p| p.corners.iter())
            .map(|c| c.length())
            .fold(0.0, f64::max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn football() -> Plains {
        Plains::truncated_icosahedron(40_000.0)
    }

    #[test]
    fn a_football_has_12_pentagons_and_20_hexagons() {
        let f = football();
        assert_eq!(f.plains.len(), 32);
        let pent = f
            .plains
            .iter()
            .filter(|p| p.kind == PlainKind::Pentagon)
            .count();
        assert_eq!(pent, 12);
        for p in &f.plains {
            let n = if p.kind == PlainKind::Pentagon { 5 } else { 6 };
            assert_eq!(p.corners.len(), n);
            assert_eq!(p.neighbours.len(), n);
        }
        assert!(
            f.plains[0].normal.distance(DVec3::Y) < 1e-12,
            "a pentagon faces +Y"
        );
    }

    #[test]
    fn every_edge_is_40_km_and_every_plain_is_flat() {
        let f = football();
        for p in &f.plains {
            let n = p.corners.len();
            for k in 0..n {
                let e = p.corners[k].distance(p.corners[(k + 1) % n]);
                assert!((e - 40_000.0).abs() < 1e-6, "edge {e}");
                assert!((p.corners[k].dot(p.normal) - p.distance).abs() < 1e-6);
            }
            // Counter-clockwise seen from outside.
            let turn = (p.corners[1] - p.corners[0]).cross(p.corners[2] - p.corners[1]);
            assert!(turn.dot(p.normal) > 0.0);
        }
        assert!((f.edge() - 40_000.0).abs() < 1e-6);
    }

    #[test]
    fn neighbours_are_mutual() {
        let f = football();
        for (i, p) in f.plains.iter().enumerate() {
            for &j in &p.neighbours {
                assert!(f.plains[j].neighbours.contains(&i));
            }
        }
    }

    #[test]
    fn the_ground_is_continuous_over_creases() {
        // Along a ray crossing a crease the hit radius changes smoothly (no jump).
        let f = football();
        let a = f.plains[0].origin().normalize();
        let b = f.plains[f.plains[0].neighbours[0]].origin().normalize();
        let mut last = f.hit(a, 0.0).radius;
        for k in 1..=2000 {
            let d = a.lerp(b, k as f64 / 2000.0).normalize();
            let r = f.hit(d, 250.0).radius - 250.0;
            let r0 = f.hit(d, 0.0).radius;
            assert!((r - r0).abs() < 300.0);
            assert!((r0 - last).abs() < 100.0, "jump at {k}: {last} -> {r0}");
            last = r0;
        }
    }

    #[test]
    fn locate_finds_the_plain_whose_polygon_contains_the_point() {
        let f = football();
        for d in crate::terrain::fibonacci_directions(5_000) {
            let h = f.hit(d, 0.0);
            let p = &f.plains[h.plain];
            assert!(p.edge_distance(d * h.radius) > -1e-3);
        }
    }

    #[test]
    fn weights_are_equal_on_a_crease_and_one_inside() {
        let f = football();
        let mut w = Vec::new();
        f.weights(f.plains[5].normal, 2_000.0, &mut w);
        assert_eq!(w, vec![(5, 1.0)]);
        let p = &f.plains[5];
        let mid = ((p.corners[0] + p.corners[1]) / 2.0).normalize();
        f.weights(mid, 2_000.0, &mut w);
        assert_eq!(w.len(), 2);
        assert!((w[0].1 - 0.5).abs() < 1e-6 && (w[1].1 - 0.5).abs() < 1e-6);
    }

    #[test]
    fn local_coordinates_round_trip() {
        let f = football();
        let p = &f.plains[7];
        let q = p.from_local(DVec3::new(1234.5, -987.0, 12.0));
        assert!(p.to_local(q).distance(DVec3::new(1234.5, -987.0, 12.0)) < 1e-6);
        assert!(p.east.dot(p.normal).abs() < 1e-12 && p.north.dot(p.east).abs() < 1e-12);
    }
}
