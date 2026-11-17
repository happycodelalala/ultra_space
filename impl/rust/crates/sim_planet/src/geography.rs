//! The land of a body of plains (planet-geography.md §3.1–3.3): plates and creases, continents
//! across creases, ranges, rifts, trenches, island arcs, mountain belts and volcanoes inside plains,
//! and relief. One deterministic function of the direction gives the elevation (m above the
//! plain's sea level); everything is seeded from the body's terrain seed.
//!
//! - **Plates:** every plain is a plate, continental or oceanic, drifting in its plane.
//! - **Creases** take a character from the two plates' drift across them (converging, diverging,
//!   sliding) and their kinds: ranges, coastal ranges with trenches, island arcs, rift valleys,
//!   mid-ocean ridges, faults.
//! - **Continents:** each plate's base level blended over a wide band at its creases, plus a
//!   planet-wide continent field, so coasts don't follow creases; a sea-level offset found at load
//!   puts the requested share of the surface under the sea.
//! - **Inside plains:** curving old mountain belts and volcano cones.

use glam::{DVec2, DVec3};

use crate::noise::{fbm, mix64, octaves_between, ridged, unit};
use crate::plains::Plains;

/// The arcade geography's numbers (planet-geography.md §4; Tessera's defaults).
#[derive(Clone, Copy, Debug)]
pub struct GeoParams {
    /// Share of plates that are continental.
    pub continental_share: f64,
    /// Share of the surface under the sea (the sea level is set to match it).
    pub sea_share: f64,
    pub continental_base: f64,
    pub oceanic_base: f64,
    /// Width (m of radial difference between planes) over which plate levels blend at creases.
    pub plate_blend: f64,
    /// Strength and wavelength of the planet-wide continent field (m).
    pub continent_relief: f64,
    pub continent_wavelength: f64,
    /// Ranges along converging continental creases: peak height and half width (m).
    pub range_height: f64,
    pub range_width: f64,
    pub coastal_range_height: f64,
    pub trench_depth: f64,
    pub island_height: f64,
    pub rift_depth: f64,
    pub ridge_height: f64,
    /// Mountain belts inside continental plains: height range (m) and half width.
    pub belt_height: (f64, f64),
    pub belt_width: f64,
    /// Volcano cones: height range and radius (m).
    pub volcano_height: (f64, f64),
    pub volcano_radius: f64,
    /// Hills on land: height and wavelength (m).
    pub hills: f64,
    pub hill_wavelength: f64,
}

impl Default for GeoParams {
    fn default() -> Self {
        Self {
            continental_share: 0.62,
            sea_share: 0.40,
            continental_base: 300.0,
            oceanic_base: -4_000.0,
            plate_blend: 9_000.0,
            continent_relief: 2_600.0,
            continent_wavelength: 30_000.0,
            range_height: 8_000.0,
            range_width: 8_000.0,
            coastal_range_height: 4_500.0,
            trench_depth: 5_500.0,
            island_height: 3_000.0,
            rift_depth: 1_800.0,
            ridge_height: 2_200.0,
            belt_height: (2_000.0, 5_000.0),
            belt_width: 6_500.0,
            volcano_height: (2_000.0, 4_000.0),
            volcano_radius: 7_000.0,
            hills: 260.0,
            hill_wavelength: 5_000.0,
        }
    }
}

/// How two plates meet at a crease.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CreaseKind {
    /// Converging continents: a mountain range along the crease.
    Range,
    /// Converging continent and ocean: a coastal range on land, a trench offshore.
    CoastalRange,
    /// Converging oceans: a chain of volcanic islands.
    IslandArc,
    /// Diverging continents: a rift valley.
    Rift,
    /// Diverging oceans: an undersea ridge.
    Ridge,
    /// Sliding past each other, or a gentle meeting: low hills along a fault.
    Fault,
}

#[derive(Clone, Debug)]
pub struct Crease {
    /// The two plains, and the crease's ends (surface frame, m).
    pub plains: (usize, usize),
    pub ends: (DVec3, DVec3),
    pub kind: CreaseKind,
    /// For a coastal range: the continental plain.
    pub land_side: usize,
    /// Unit vector across the crease toward the land side (in the land plain's plane).
    pub toward_land: DVec3,
}

/// A curving old mountain belt inside a plain: a quadratic curve in the plain's local plane.
#[derive(Clone, Debug)]
struct Belt {
    points: [DVec2; 3],
    height: f64,
}

#[derive(Clone, Debug)]
struct Volcano {
    at: DVec2,
    height: f64,
}

#[derive(Clone, Debug)]
pub struct Geography {
    pub params: GeoParams,
    pub continental: Vec<bool>,
    pub creases: Vec<Crease>,
    /// Per plain: the creases near enough to shape it (its own and its neighbours').
    near: Vec<Vec<usize>>,
    belts: Vec<Vec<Belt>>,
    volcanoes: Vec<Vec<Volcano>>,
    /// Subtracted from the raw elevation so the requested share is under the sea.
    sea_offset: f64,
    seeds: [u64; 8],
}

/// How much of the plates' height above the sea stays (the rest of land relief is features).
const LAND_FLATTEN: f64 = 0.3;

fn smooth(e0: f64, e1: f64, x: f64) -> f64 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Distance from `p` to the segment a–b, and where along it (0–1).
fn segment(p: DVec3, a: DVec3, b: DVec3) -> (f64, f64) {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0);
    (p.distance(a + ab * t), t)
}

impl Geography {
    pub fn new(plains: &Plains, seed: u64, params: GeoParams) -> Self {
        let n = plains.plains.len();
        let mut rng = mix64(seed ^ 0x6E06_7A1D);
        let mut next = move || {
            rng = mix64(rng);
            unit(rng)
        };
        let continental: Vec<bool> = (0..n).map(|_| next() < params.continental_share).collect();
        let drift: Vec<DVec3> = plains
            .plains
            .iter()
            .map(|p| {
                let a = next() * std::f64::consts::TAU;
                (p.east * a.cos() + p.north * a.sin()) * (0.3 + 0.7 * next())
            })
            .collect();
        let mut creases = Vec::new();
        for (i, p) in plains.plains.iter().enumerate() {
            let k = p.corners.len();
            for (e, &j) in p.neighbours.iter().enumerate() {
                if j < i {
                    continue;
                }
                // From plain i toward j, in i's plane.
                let toward = plains.plains[j].normal - p.normal;
                let toward = (toward - p.normal * toward.dot(p.normal)).normalize();
                let closing = (drift[i] - drift[j]).dot(toward);
                let (ci, cj) = (continental[i], continental[j]);
                let kind = if closing > 0.3 {
                    match (ci, cj) {
                        (true, true) => CreaseKind::Range,
                        (false, false) => CreaseKind::IslandArc,
                        _ => CreaseKind::CoastalRange,
                    }
                } else if closing < -0.3 {
                    match (ci, cj) {
                        (true, true) => CreaseKind::Rift,
                        (false, false) => CreaseKind::Ridge,
                        _ => CreaseKind::Fault,
                    }
                } else {
                    CreaseKind::Fault
                };
                let land_side = if ci { i } else { j };
                let ends = (p.corners[e], p.corners[(e + 1) % k]);
                let land = &plains.plains[land_side];
                let mut toward_land = land.normal.cross(ends.1 - ends.0).normalize();
                if (land.origin() - ends.0).dot(toward_land) < 0.0 {
                    toward_land = -toward_land;
                }
                creases.push(Crease {
                    plains: (i, j),
                    ends,
                    kind,
                    land_side,
                    toward_land,
                });
            }
        }
        // Per plain: every crease close enough to reach it (within the plain's reach plus three
        // range widths).
        let near = plains
            .plains
            .iter()
            .map(|p| {
                let centre = p.origin();
                let reach = p
                    .corners
                    .iter()
                    .map(|c| c.distance(centre))
                    .fold(0.0, f64::max)
                    + 3.0 * params.range_width.max(params.belt_width)
                    + 2_000.0;
                creases
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| segment(centre, c.ends.0, c.ends.1).0 < reach)
                    .map(|(k, _)| k)
                    .collect()
            })
            .collect();
        // Belts and volcanoes inside plains, kept at least 12 km from the edges.
        let mut belts = vec![Vec::new(); n];
        let mut volcanoes = vec![Vec::new(); n];
        for (i, p) in plains.plains.iter().enumerate() {
            let inside = |q: DVec2| p.edge_distance(p.from_local(q.extend(0.0))) > 12_000.0;
            let pick = |next: &mut dyn FnMut() -> f64| loop {
                let q = DVec2::new(next() * 2.0 - 1.0, next() * 2.0 - 1.0) * 30_000.0;
                if inside(q) {
                    return q;
                }
            };
            if continental[i] && next() < 0.55 {
                let a = pick(&mut next);
                let c = pick(&mut next);
                // All three points inside, so the curve stays inside (a convex plain).
                let b = pick(&mut next);
                let (lo, hi) = params.belt_height;
                belts[i].push(Belt {
                    points: [a, b, c],
                    height: lo + (hi - lo) * next(),
                });
            }
            let count = if continental[i] {
                (next() * 1.6) as usize
            } else {
                (next() * 2.2) as usize
            };
            for _ in 0..count {
                let (lo, hi) = params.volcano_height;
                volcanoes[i].push(Volcano {
                    at: pick(&mut next),
                    height: lo + (hi - lo) * next(),
                });
            }
        }
        let seeds = std::array::from_fn(|k| mix64(seed ^ (0x6E0 + k as u64)));
        let mut geo = Self {
            params,
            continental,
            creases,
            near,
            belts,
            volcanoes,
            sea_offset: 0.0,
            seeds,
        };
        geo.sea_offset = geo.sea_level(plains);
        geo
    }

    /// The raw elevation below which a share `sea_share` of the surface lies.
    fn sea_level(&self, plains: &Plains) -> f64 {
        let mut values: Vec<f64> = crate::terrain::fibonacci_directions(6_000)
            .map(|d| self.base(plains, d))
            .collect();
        values.sort_by(f64::total_cmp);
        values[((values.len() as f64 * self.params.sea_share) as usize).min(values.len() - 1)]
    }

    /// Elevation (m above sea level, along the plain's normal) at the unit direction `dir`, with
    /// detail down to `shortest` metres.
    pub fn elevation(&self, plains: &Plains, dir: DVec3, shortest: f64) -> f64 {
        self.raw(plains, dir, shortest)
    }

    /// The open sea's floor level without the land's features: below 0 is sea connected to the
    /// ocean (basins of land below sea level are lakes or dry, by climate).
    pub fn open_sea(&self, plains: &Plains, dir: DVec3) -> bool {
        self.base(plains, dir) - self.sea_offset < 0.0
    }

    /// Plate levels blended at creases plus the continent field.
    fn base(&self, plains: &Plains, dir: DVec3) -> f64 {
        let p = &self.params;
        let hit = plains.hit(dir, 0.0);
        let b = dir * hit.radius;
        let mut weights = Vec::with_capacity(6);
        plains.weights(dir, p.plate_blend, &mut weights);
        let base: f64 = weights
            .iter()
            .map(|&(k, w)| {
                w * if self.continental[k] {
                    p.continental_base
                } else {
                    p.oceanic_base
                }
            })
            .sum();
        base + p.continent_relief * fbm(b / p.continent_wavelength, self.seeds[0], 4, 0.5)
    }

    fn raw(&self, plains: &Plains, dir: DVec3, shortest: f64) -> f64 {
        let p = &self.params;
        let hit = plains.hit(dir, 0.0);
        let b = dir * hit.radius;
        let own = hit.plain;
        // The plates' level against the sea: land above it is flattened into lowlands and
        // plateaus (ranges, belts and volcanoes give the heights), the sea floor kept.
        let level = self.base(plains, dir) - self.sea_offset;
        let mut e = if level > 0.0 {
            LAND_FLATTEN * level
        } else {
            level
        };
        let land = smooth(-400.0, 400.0, level);
        let ridge_oct = octaves_between(4_500.0, shortest.max(60.0));
        for &k in &self.near[own] {
            let c = &self.creases[k];
            let (d, t) = segment(b, c.ends.0, c.ends.1);
            if d > 3.0 * p.range_width {
                continue;
            }
            let along = DVec3::new(t * 9.0, k as f64 * 7.31, 0.5);
            match c.kind {
                CreaseKind::Range => {
                    let height = p.range_height * (0.55 + 0.45 * fbm(along, self.seeds[1], 3, 0.5));
                    let env = (-(d / p.range_width).powi(2)).exp();
                    let crest = 0.55
                        + 0.45
                            * (t * std::f64::consts::TAU * 7.0
                                + 2.0 * fbm(b / 9_000.0, self.seeds[2], 2, 0.5))
                            .cos();
                    let rid = ridged(b / 4_500.0, self.seeds[3], ridge_oct, 0.5);
                    e += height * env * (0.25 + 0.75 * rid) * (0.6 + 0.4 * crest);
                }
                CreaseKind::CoastalRange => {
                    // Which side of the crease the point is on, by geometry (not by its plain).
                    let sd = if (b - c.ends.0).dot(c.toward_land) >= 0.0 {
                        d
                    } else {
                        -d
                    };
                    let rid = ridged(b / 4_000.0, self.seeds[3], ridge_oct, 0.5);
                    e += p.coastal_range_height
                        * (-((sd - 7_000.0) / 7_000.0).powi(2)).exp()
                        * (0.4 + 0.6 * rid);
                    e -= p.trench_depth * (-((sd + 9_000.0) / 6_000.0).powi(2)).exp();
                }
                CreaseKind::IslandArc => {
                    let isl =
                        ((fbm(b / 7_000.0, self.seeds[4], 2, 0.5) + 0.1) * 2.5).clamp(0.0, 1.0);
                    e += (p.island_height - p.oceanic_base) * (-(d / 6_000.0).powi(2)).exp() * isl;
                }
                CreaseKind::Rift => {
                    e -= p.rift_depth * (1.0 - smooth(2_500.0, 6_000.0, d));
                    e += 0.35 * p.rift_depth * (-((d - 7_000.0) / 2_500.0).powi(2)).exp();
                }
                CreaseKind::Ridge => {
                    e += p.ridge_height * (-(d / 8_000.0).powi(2)).exp();
                }
                CreaseKind::Fault => {
                    e += 300.0
                        * (-(d / 4_000.0).powi(2)).exp()
                        * (0.3 + fbm(b / 3_000.0, self.seeds[5], 2, 0.5));
                }
            }
        }
        // Inside plains: belts and volcanoes, this plain's and its neighbours' (they may reach
        // over the crease).
        for k in std::iter::once(own).chain(plains.plains[own].neighbours.iter().copied()) {
            e += self.inner(plains, k, b, ridge_oct);
        }
        // Hills on land.
        let hill_oct = octaves_between(p.hill_wavelength, shortest);
        e + land * p.hills * fbm(b / p.hill_wavelength, self.seeds[7], hill_oct, 0.5)
    }

    /// The belts and volcanoes of plain `own` at the point `b` (surface frame).
    fn inner(&self, plains: &Plains, own: usize, b: DVec3, ridge_oct: u32) -> f64 {
        let p = &self.params;
        let mut e = 0.0;
        let plain = &plains.plains[own];
        // True distances: in the plain, and off it (a point on another plain projects anywhere).
        let local3 = plain.to_local(b);
        let local = local3.truncate();
        let off = local3.z * local3.z;
        for belt in &self.belts[own] {
            let [a, m, c] = belt.points;
            let d = (0..=24)
                .map(|s| {
                    let u = f64::from(s) / 24.0;
                    let q = a * (1.0 - u) * (1.0 - u) + m * 2.0 * u * (1.0 - u) + c * u * u;
                    q.distance_squared(local)
                })
                .fold(f64::INFINITY, f64::min);
            let d = (d + off).sqrt();
            if d < 3.0 * p.belt_width {
                let rid = ridged(b / 3_500.0, self.seeds[6], ridge_oct, 0.5);
                e += belt.height * (-(d / p.belt_width).powi(2)).exp() * (0.3 + 0.7 * rid);
            }
        }
        for v in &self.volcanoes[own] {
            let d = (v.at.distance_squared(local) + off).sqrt();
            if d < p.volcano_radius {
                let cone = (1.0 - d / p.volcano_radius).powf(1.6);
                let crater = 0.25 * v.height * (1.0 - smooth(0.0, 0.08 * p.volcano_radius, d));
                let floor = if self.continental[own] {
                    0.0
                } else {
                    -p.oceanic_base
                };
                e += (v.height + floor) * cone - crater;
            }
        }
        e
    }

    /// The highest the land can reach above sea level (m), for bounds.
    pub fn highest(&self) -> f64 {
        let p = &self.params;
        p.range_height
            + p.belt_height.1
            + p.volcano_height.1
            + LAND_FLATTEN * (p.continent_relief + p.continental_base - self.sea_offset).max(0.0)
            + p.hills
    }

    /// The lowest the sea floor can reach (m, negative).
    pub fn lowest(&self) -> f64 {
        let p = &self.params;
        p.oceanic_base
            - p.trench_depth
            - p.continent_relief
            - p.rift_depth
            - p.hills
            - self.sea_offset
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tessera() -> (Plains, Geography) {
        let plains = Plains::truncated_icosahedron(40_000.0);
        let geo = Geography::new(&plains, 11, GeoParams::default());
        (plains, geo)
    }

    #[test]
    fn about_forty_percent_is_sea() {
        let (plains, geo) = tessera();
        let n = 4_000;
        let sea = crate::terrain::fibonacci_directions(n)
            .filter(|&d| geo.elevation(&plains, d, 500.0) < 0.0)
            .count();
        let share = sea as f64 / n as f64;
        assert!((share - 0.40).abs() < 0.04, "sea share {share:.2}");
    }

    #[test]
    fn creases_have_characters_and_ranges_are_high() {
        let (plains, geo) = tessera();
        assert_eq!(geo.creases.len(), 90);
        let ranges: Vec<&Crease> = geo
            .creases
            .iter()
            .filter(|c| c.kind == CreaseKind::Range)
            .collect();
        assert!(!ranges.is_empty(), "some ranges");
        // Somewhere along every range the land stands kilometres high.
        for c in ranges {
            let top = (0..40)
                .map(|s| {
                    let p = c.ends.0.lerp(c.ends.1, f64::from(s) / 40.0);
                    geo.elevation(&plains, p.normalize(), 100.0)
                })
                .fold(f64::MIN, f64::max);
            assert!(top > 3_000.0, "range top {top}");
        }
    }

    #[test]
    fn coasts_cross_creases() {
        // Some plains have both land and sea (coasts inside plains, not only on creases).
        let (plains, geo) = tessera();
        let mixed = plains
            .plains
            .iter()
            .filter(|p| {
                let samples: Vec<f64> = (0..60)
                    .map(|s| {
                        let a = f64::from(s) * 2.4;
                        let r = 25_000.0 * (f64::from(s) / 60.0).sqrt();
                        let q = p.from_local(DVec3::new(r * a.cos(), r * a.sin(), 0.0));
                        geo.elevation(&plains, q.normalize(), 1_000.0)
                    })
                    .collect();
                samples.iter().any(|&e| e > 0.0) && samples.iter().any(|&e| e < 0.0)
            })
            .count();
        assert!(mixed >= 6, "plains with a coast inside: {mixed}");
    }
}
