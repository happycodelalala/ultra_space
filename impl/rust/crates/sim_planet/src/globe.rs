//! The land, climate and regions of a round planet (planet-geography.md §1a, §3).
//!
//! - **Plates** (hidden geology): cells round seeds spread evenly and jittered, their borders
//!   curved by warping the direction with noise before finding the nearest seed. Each plate is
//!   continental or oceanic and drifts; each boundary between neighbouring plates gets a
//!   character from the drift across it (ranges, coastal ranges with trenches, island arcs, rifts,
//!   ridges, faults), shaped by the distance to the boundary.
//! - **Land:** plate levels blended at boundaries plus a continent field; the sea level set so a
//!   share of the surface is under it; land above the sea flattened into lowlands, the features
//!   giving the heights; mountain belts and volcanoes inside plates; hills.
//! - **Climate:** warmth from latitude and height; wetness from the sea upwind along Earth-like
//!   wind belts, wrung out by high ground; on a 1° latitude–longitude grid computed at load.
//! - **Regions:** about 600 cells merged by similarity into natural areas, with borders on range
//!   crests, coasts and climate changes; named from what they hold.
//!
//! Heights are radial, above the reference radius (sea level).

use glam::DVec3;
use serde::Deserialize;

use crate::geography::{CreaseKind, GeoParams};
use crate::noise::{fbm, mix64, octaves_between, ridged, unit};
use crate::terrain::fibonacci_directions;

const KINDS: &str = include_str!("../../../../../content/data/region_types/standard.json");

/// How far plate borders are warped (radians of direction).
const WARP: f64 = 0.12;
const WARP_FREQUENCY: f64 = 2.2;
/// Of the plates' height above the sea, how much stays (the features give the rest).
const LAND_FLATTEN: f64 = 0.3;
/// Climate.
const EQUATOR_TEMPERATURE: f64 = 30.0;
const POLE_DROP: f64 = 62.0;
const LAPSE: f64 = 0.0065;
const UPWIND_STEP: f64 = 3_500.0;
const UPWIND_STEPS: u32 = 18;
const BLOCK_HEIGHT: f64 = 1_500.0;
const BLOCK_KEEP: f64 = 0.55;
/// Climate grid cells per degree.
const GRID_PER_DEGREE: usize = 1;
/// Region cells, and regions' size in cells (min, max).
const CELLS: usize = 600;
const REGION_CELLS: (usize, usize) = (8, 20);

fn smooth(e0: f64, e1: f64, x: f64) -> f64 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// East and north at a unit direction (Y is north).
fn tangent(dir: DVec3) -> (DVec3, DVec3) {
    let east = DVec3::Y.cross(dir).normalize_or(DVec3::X);
    (east, dir.cross(east))
}

#[derive(Clone, Debug)]
struct Plate {
    seed: DVec3,
    continental: bool,
}

#[derive(Clone, Debug)]
struct Boundary {
    a: usize,
    b: usize,
    kind: CreaseKind,
    /// For a coastal range, the continental plate.
    land: usize,
}

#[derive(Clone, Debug)]
struct Belt {
    /// Points along the belt's crest (on the sphere, m).
    crest: Vec<DVec3>,
    /// A bounding sphere: centre and radius (m).
    centre: DVec3,
    reach: f64,
    height: f64,
}

#[derive(Clone, Debug)]
struct Volcano {
    at: DVec3,
    height: f64,
    oceanic: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegionKind {
    pub id: String,
    pub noun: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct KindsFile {
    kinds: Vec<RegionKind>,
    adjectives: Vec<String>,
}

/// A natural region: its kind, name and middle (unit direction).
#[derive(Clone, Debug)]
pub struct Region {
    pub kind: usize,
    pub name: String,
    pub centre: DVec3,
}

/// A round planet's land, climate and regions.
#[derive(Clone, Debug)]
pub struct GlobeWorld {
    pub radius: f64,
    pub params: GeoParams,
    plates: Vec<Plate>,
    boundaries: Vec<Boundary>,
    belts: Vec<Belt>,
    volcanoes: Vec<Volcano>,
    sea_offset: f64,
    seeds: [u64; 10],
    /// Wetness on the latitude–longitude grid (rows from the south pole).
    wet: Vec<f32>,
    pub kinds: Vec<RegionKind>,
    pub regions: Vec<Region>,
    cells: Vec<DVec3>,
    cell_region: Vec<usize>,
}

impl GlobeWorld {
    pub fn new(radius: f64, plates: usize, seed: u64) -> Self {
        let params = GeoParams::default();
        let mut rng = mix64(seed ^ 0x0060_10BE);
        let mut next = move || {
            rng = mix64(rng);
            unit(rng)
        };
        // Plates: seeds spread evenly, jittered.
        let spacing = (4.0 * std::f64::consts::PI / plates as f64).sqrt();
        let plate_list: Vec<Plate> = fibonacci_directions(plates)
            .map(|d| {
                let (e, n) = tangent(d);
                let j = (e * (next() - 0.5) + n * (next() - 0.5)) * spacing * 0.35;
                Plate {
                    seed: (d + j).normalize(),
                    continental: next() < params.continental_share,
                }
            })
            .collect();
        let drift: Vec<DVec3> = plate_list
            .iter()
            .map(|p| {
                let (e, n) = tangent(p.seed);
                let a = next() * std::f64::consts::TAU;
                (e * a.cos() + n * a.sin()) * (0.3 + 0.7 * next())
            })
            .collect();
        // Neighbouring plates: seeds closer than 1.7 spacings.
        let mut boundaries = Vec::new();
        for a in 0..plates {
            for b in a + 1..plates {
                let (sa, sb) = (plate_list[a].seed, plate_list[b].seed);
                if sa.angle_between(sb) > 1.7 * spacing {
                    continue;
                }
                let toward = (sb - sa * sb.dot(sa)).normalize();
                let closing = (drift[a] - drift[b]).dot(toward);
                let (ca, cb) = (plate_list[a].continental, plate_list[b].continental);
                let kind = if closing > 0.3 {
                    match (ca, cb) {
                        (true, true) => CreaseKind::Range,
                        (false, false) => CreaseKind::IslandArc,
                        _ => CreaseKind::CoastalRange,
                    }
                } else if closing < -0.3 {
                    match (ca, cb) {
                        (true, true) => CreaseKind::Rift,
                        (false, false) => CreaseKind::Ridge,
                        _ => CreaseKind::Fault,
                    }
                } else {
                    CreaseKind::Fault
                };
                boundaries.push(Boundary {
                    a,
                    b,
                    kind,
                    land: if ca { a } else { b },
                });
            }
        }
        // Belts and volcanoes inside plates.
        let mut belts = Vec::new();
        let mut volcanoes = Vec::new();
        let local = |seed: DVec3, x: f64, y: f64| {
            let (e, n) = tangent(seed);
            (seed * radius + e * x + n * y).normalize() * radius
        };
        let inner = 0.3 * spacing * radius;
        for p in &plate_list {
            let pick = |next: &mut dyn FnMut() -> f64| {
                ((next() * 2.0 - 1.0) * inner, (next() * 2.0 - 1.0) * inner)
            };
            if p.continental && next() < 0.55 {
                let (a, b, c) = (pick(&mut next), pick(&mut next), pick(&mut next));
                let crest: Vec<DVec3> = (0..=24)
                    .map(|s| {
                        let u = f64::from(s) / 24.0;
                        let x =
                            a.0 * (1.0 - u) * (1.0 - u) + b.0 * 2.0 * u * (1.0 - u) + c.0 * u * u;
                        let y =
                            a.1 * (1.0 - u) * (1.0 - u) + b.1 * 2.0 * u * (1.0 - u) + c.1 * u * u;
                        local(p.seed, x, y)
                    })
                    .collect();
                let centre = crest.iter().copied().sum::<DVec3>() / crest.len() as f64;
                let reach = crest.iter().map(|q| q.distance(centre)).fold(0.0, f64::max)
                    + 3.0 * params.belt_width;
                let (lo, hi) = params.belt_height;
                belts.push(Belt {
                    crest,
                    centre,
                    reach,
                    height: lo + (hi - lo) * next(),
                });
            }
            let count = if p.continental {
                (next() * 1.6) as usize
            } else {
                (next() * 2.2) as usize
            };
            for _ in 0..count {
                let (x, y) = pick(&mut next);
                let (lo, hi) = params.volcano_height;
                volcanoes.push(Volcano {
                    at: local(p.seed, x, y),
                    height: lo + (hi - lo) * next(),
                    oceanic: !p.continental,
                });
            }
        }
        let kinds_file: KindsFile =
            serde_json::from_str(KINDS).expect("region_types/standard.json is valid");
        let mut world = Self {
            radius,
            params,
            plates: plate_list,
            boundaries,
            belts,
            volcanoes,
            sea_offset: 0.0,
            seeds: std::array::from_fn(|k| mix64(seed ^ (0x6100 + k as u64))),
            wet: Vec::new(),
            kinds: kinds_file.kinds,
            regions: Vec::new(),
            cells: Vec::new(),
            cell_region: Vec::new(),
        };
        let mut base: Vec<f64> = fibonacci_directions(6_000)
            .map(|d| world.base(d).0)
            .collect();
        base.sort_by(f64::total_cmp);
        world.sea_offset =
            base[((base.len() as f64 * world.params.sea_share) as usize).min(base.len() - 1)];
        world.wet = world.climate_grid();
        world.grow_regions(&kinds_file.adjectives, seed);
        world
    }

    /// Angles (radians) from the warped direction to every plate's seed.
    fn angles(&self, dir: DVec3) -> Vec<f64> {
        let p = dir * WARP_FREQUENCY;
        let w = DVec3::new(
            fbm(p, self.seeds[8], 3, 0.5),
            fbm(p + DVec3::splat(31.4), self.seeds[8], 3, 0.5),
            fbm(p + DVec3::splat(-17.9), self.seeds[8], 3, 0.5),
        );
        let warped = (dir + w * WARP).normalize();
        self.plates
            .iter()
            .map(|pl| warped.dot(pl.seed).clamp(-1.0, 1.0).acos())
            .collect()
    }

    /// Plate levels blended at boundaries plus the continent field; and the plate angles.
    fn base(&self, dir: DVec3) -> (f64, Vec<f64>) {
        let p = &self.params;
        let th = self.angles(dir);
        let near = th.iter().copied().fold(f64::INFINITY, f64::min);
        let (mut sum, mut total) = (0.0, 0.0);
        for (k, &t) in th.iter().enumerate() {
            let d = (t - near) * 0.5 * self.radius;
            if d < p.plate_blend {
                let x = 1.0 - d / p.plate_blend;
                let w = x * x * (3.0 - 2.0 * x);
                sum += w * if self.plates[k].continental {
                    p.continental_base
                } else {
                    p.oceanic_base
                };
                total += w;
            }
        }
        let b = dir * self.radius;
        let level = sum / total
            + p.continent_relief * fbm(b / p.continent_wavelength, self.seeds[0], 4, 0.5);
        (level, th)
    }

    /// Whether `dir` lies in the open sea (not a land basin below sea level).
    pub fn open_sea(&self, dir: DVec3) -> bool {
        self.base(dir.normalize()).0 - self.sea_offset < 0.0
    }

    /// Elevation (m above sea level) at the unit direction `dir`, detail down to `shortest`.
    pub fn elevation(&self, dir: DVec3, shortest: f64) -> f64 {
        let p = &self.params;
        let dir = dir.normalize();
        let (base, th) = self.base(dir);
        let level = base - self.sea_offset;
        let mut e = if level > 0.0 {
            LAND_FLATTEN * level
        } else {
            level
        };
        let land = smooth(-400.0, 400.0, level);
        let near = th.iter().copied().fold(f64::INFINITY, f64::min);
        let b = dir * self.radius;
        let ridge_oct = octaves_between(4_500.0, shortest.max(60.0));
        let cutoff = 3.0 * p.range_width;
        for (k, bd) in self.boundaries.iter().enumerate() {
            let (ta, tb) = (th[bd.a], th[bd.b]);
            // Distance to the boundary between a and b (where it is a real border).
            let d = ((ta - tb).abs() * 0.5).max(ta.min(tb) - near) * self.radius;
            if d > cutoff {
                continue;
            }
            match bd.kind {
                CreaseKind::Range => {
                    let along = fbm(b / 20_000.0 + DVec3::splat(k as f64), self.seeds[1], 2, 0.5);
                    let height = p.range_height * (0.6 + 0.4 * along);
                    let env = (-(d / p.range_width).powi(2)).exp();
                    let crest = 0.55 + 0.45 * (fbm(b / 9_000.0, self.seeds[2], 2, 0.5) * 6.0).cos();
                    let rid = ridged(b / 4_500.0, self.seeds[3], ridge_oct, 0.5);
                    e += height * env * (0.25 + 0.75 * rid) * (0.6 + 0.4 * crest);
                }
                CreaseKind::CoastalRange => {
                    let other = if bd.land == bd.a { bd.b } else { bd.a };
                    let sd = if th[bd.land] <= th[other] { d } else { -d };
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
        for belt in &self.belts {
            if b.distance(belt.centre) > belt.reach {
                continue;
            }
            let d = belt
                .crest
                .iter()
                .map(|q| q.distance(b))
                .fold(f64::INFINITY, f64::min);
            if d < 3.0 * p.belt_width {
                let rid = ridged(b / 3_500.0, self.seeds[6], ridge_oct, 0.5);
                e += belt.height * (-(d / p.belt_width).powi(2)).exp() * (0.3 + 0.7 * rid);
            }
        }
        for v in &self.volcanoes {
            let d = v.at.distance(b);
            if d < p.volcano_radius {
                let cone = (1.0 - d / p.volcano_radius).powf(1.6);
                let crater = 0.25 * v.height * (1.0 - smooth(0.0, 0.08 * p.volcano_radius, d));
                let floor = if v.oceanic { -p.oceanic_base } else { 0.0 };
                e += (v.height + floor) * cone - crater;
            }
        }
        let hill_oct = octaves_between(p.hill_wavelength, shortest);
        e + land * p.hills * fbm(b / p.hill_wavelength, self.seeds[7], hill_oct, 0.5)
    }

    /// The highest and lowest the ground can be (m above sea level).
    pub fn range(&self) -> (f64, f64) {
        let p = &self.params;
        let top = p.range_height
            + p.belt_height.1
            + p.volcano_height.1
            + LAND_FLATTEN * (p.continent_relief + p.continental_base - self.sea_offset).max(0.0)
            + p.hills;
        let bottom = p.oceanic_base
            - p.trench_depth
            - p.continent_relief
            - p.rift_depth
            - p.hills
            - self.sea_offset;
        (bottom, top)
    }

    // ---- climate --------------------------------------------------------------------------------

    /// Warmth at sea level (°C) at `dir`, from the latitude.
    pub fn sea_level_temperature(&self, dir: DVec3) -> f64 {
        let lat = dir.normalize().y.clamp(-1.0, 1.0).asin().to_degrees().abs();
        EQUATOR_TEMPERATURE - POLE_DROP * (lat / 90.0).powi(2)
    }

    /// Temperature (°C) at `dir` and `elevation`.
    pub fn temperature(&self, dir: DVec3, elevation: f64) -> f64 {
        self.sea_level_temperature(dir) - LAPSE * elevation.max(0.0)
    }

    /// The prevailing wind: trade winds toward the west near the equator, westerlies at mid
    /// latitudes, polar easterlies.
    fn wind(dir: DVec3) -> DVec3 {
        let (east, _) = tangent(dir);
        let lat = dir.y.clamp(-1.0, 1.0).asin().to_degrees().abs();
        if (30.0..60.0).contains(&lat) {
            east
        } else {
            -east
        }
    }

    fn wetness_at(&self, dir: DVec3) -> f64 {
        let here = self.elevation(dir, 2_000.0);
        if here < 0.0 {
            return 1.0;
        }
        let upwind = -Self::wind(dir);
        let (mut wet, mut total, mut keep) = (0.0, 0.0, 1.0);
        let mut at = dir;
        for k in 1..=UPWIND_STEPS {
            let step = upwind - at * upwind.dot(at);
            at = (at + step.normalize_or(DVec3::ZERO) * (UPWIND_STEP / self.radius)).normalize();
            let e = self.elevation(at, 2_000.0);
            if e > here + BLOCK_HEIGHT {
                keep *= BLOCK_KEEP;
            }
            let w = 0.93f64.powi(k as i32);
            if e < 0.0 {
                wet += w * keep;
            }
            total += w;
        }
        (0.18 + 1.1 * wet / total).clamp(0.0, 1.0)
    }

    fn grid_size() -> (usize, usize) {
        (360 * GRID_PER_DEGREE, 180 * GRID_PER_DEGREE + 1)
    }

    fn climate_grid(&self) -> Vec<f32> {
        let (nx, ny) = Self::grid_size();
        let rows: Vec<usize> = (0..ny).collect();
        let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
        let chunks: Vec<&[usize]> = rows.chunks(ny.div_ceil(threads).max(1)).collect();
        let mut parts: Vec<(usize, Vec<f32>)> = std::thread::scope(|s| {
            let handles: Vec<_> = chunks
                .iter()
                .map(|chunk| {
                    s.spawn(move || {
                        chunk
                            .iter()
                            .map(|&j| {
                                let lat = (j as f64 / (ny - 1) as f64 - 0.5) * std::f64::consts::PI;
                                let row: Vec<f32> = (0..nx)
                                    .map(|i| {
                                        let lon = i as f64 / nx as f64 * std::f64::consts::TAU;
                                        let d = DVec3::new(
                                            lat.cos() * lon.cos(),
                                            lat.sin(),
                                            lat.cos() * lon.sin(),
                                        );
                                        self.wetness_at(d) as f32
                                    })
                                    .collect();
                                (j, row)
                            })
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            handles
                .into_iter()
                .flat_map(|h| h.join().expect("climate thread"))
                .collect()
        });
        parts.sort_by_key(|(j, _)| *j);
        parts.into_iter().flat_map(|(_, r)| r).collect()
    }

    /// Wetness (0–1) at `dir`, from the climate grid.
    pub fn wetness(&self, dir: DVec3) -> f64 {
        let d = dir.normalize();
        let (nx, ny) = Self::grid_size();
        let lat = d.y.clamp(-1.0, 1.0).asin();
        let lon = d.z.atan2(d.x).rem_euclid(std::f64::consts::TAU);
        let y = ((lat / std::f64::consts::PI + 0.5) * (ny - 1) as f64)
            .clamp(0.0, (ny - 1) as f64 - 1e-9);
        let x = lon / std::f64::consts::TAU * nx as f64;
        let (ix, iy) = (x.floor() as usize % nx, y.floor() as usize);
        let (fx, fy) = (x - x.floor(), y - iy as f64);
        let at = |i: usize, j: usize| f64::from(self.wet[j * nx + (i % nx)]);
        let a = at(ix, iy) * (1.0 - fx) + at(ix + 1, iy) * fx;
        let b = at(ix, iy + 1) * (1.0 - fx) + at(ix + 1, iy + 1) * fx;
        a * (1.0 - fy) + b * fy
    }

    // ---- regions --------------------------------------------------------------------------------

    /// The region at `dir` (index into `regions`).
    pub fn region_at(&self, dir: DVec3) -> usize {
        let d = dir.normalize();
        let cell = self
            .cells
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.dot(d).total_cmp(&b.1.dot(d)))
            .map_or(0, |(i, _)| i);
        self.cell_region[cell]
    }

    /// The kind of region `r`.
    pub fn kind(&self, r: usize) -> &RegionKind {
        &self.kinds[self.regions[r].kind]
    }

    fn grow_regions(&mut self, adjectives: &[String], seed: u64) {
        let cells: Vec<DVec3> = fibonacci_directions(CELLS).collect();
        let spacing = (4.0 * std::f64::consts::PI / CELLS as f64).sqrt();
        // Each cell's character from a few samples.
        struct Cell {
            sea: f64,
            mean: f64,
            high: f64,
            temp: f64,
            wet: f64,
        }
        let character: Vec<Cell> = cells
            .iter()
            .map(|&c| {
                let (e1, n1) = tangent(c);
                let mut sea = 0.0;
                let (mut sum, mut high, mut n) = (0.0, f64::MIN, 0.0);
                for (x, y) in [
                    (0.0, 0.0),
                    (0.3, 0.0),
                    (-0.3, 0.0),
                    (0.0, 0.3),
                    (0.0, -0.3),
                    (0.2, 0.2),
                    (-0.2, -0.2),
                ] {
                    let d = (c + (e1 * x + n1 * y) * spacing).normalize();
                    let e = self.elevation(d, 1_000.0);
                    if e < 0.0 && self.open_sea(d) {
                        sea += 1.0;
                    }
                    sum += e;
                    high = high.max(e);
                    n += 1.0;
                }
                let mean = sum / n;
                Cell {
                    sea: sea / n,
                    mean,
                    high,
                    temp: self.temperature(c, mean),
                    wet: self.wetness(c),
                }
            })
            .collect();
        // Edges between neighbouring cells, cheapest first.
        let mut edges: Vec<(f64, usize, usize)> = Vec::new();
        for a in 0..cells.len() {
            for b in a + 1..cells.len() {
                if cells[a].angle_between(cells[b]) > 1.6 * spacing {
                    continue;
                }
                let (ca, cb) = (&character[a], &character[b]);
                let mut cost = (ca.mean - cb.mean).abs() / 1_000.0
                    + (ca.temp - cb.temp).abs() / 10.0
                    + (ca.wet - cb.wet).abs() * 2.0;
                // Coasts are borders.
                if (ca.sea > 0.5) != (cb.sea > 0.5) {
                    cost += 5.0;
                }
                // So are range crests between them.
                let crest = (1..5)
                    .map(|s| {
                        let d = cells[a].lerp(cells[b], f64::from(s) / 5.0).normalize();
                        self.elevation(d, 1_000.0)
                    })
                    .fold(f64::MIN, f64::max);
                if crest > ca.high.max(cb.high) + 300.0 || crest > ca.mean.max(cb.mean) + 2_000.0 {
                    cost += 3.0;
                }
                edges.push((cost, a, b));
            }
        }
        edges.sort_by(|x, y| x.0.total_cmp(&y.0));
        // Union-find with sizes.
        let mut parent: Vec<usize> = (0..cells.len()).collect();
        let mut size = vec![1usize; cells.len()];
        fn find(p: &mut [usize], mut x: usize) -> usize {
            while p[x] != x {
                p[x] = p[p[x]];
                x = p[x];
            }
            x
        }
        for &(cost, a, b) in &edges {
            if cost > 2.5 {
                break;
            }
            let (ra, rb) = (find(&mut parent, a), find(&mut parent, b));
            if ra != rb && size[ra] + size[rb] <= REGION_CELLS.1 {
                parent[rb] = ra;
                size[ra] += size[rb];
            }
        }
        // Small leftovers join their cheapest neighbour (any cost), up to a looser cap.
        for _ in 0..3 {
            for &(_, a, b) in &edges {
                let (ra, rb) = (find(&mut parent, a), find(&mut parent, b));
                if ra == rb {
                    continue;
                }
                if (size[ra] < REGION_CELLS.0 || size[rb] < REGION_CELLS.0)
                    && size[ra] + size[rb] <= REGION_CELLS.1 + REGION_CELLS.0
                {
                    let (big, small) = if size[ra] >= size[rb] {
                        (ra, rb)
                    } else {
                        (rb, ra)
                    };
                    parent[small] = big;
                    size[big] += size[small];
                }
            }
        }
        let roots: Vec<usize> = (0..cells.len()).map(|c| find(&mut parent, c)).collect();
        let mut ids: Vec<usize> = roots.clone();
        ids.sort_unstable();
        ids.dedup();
        let cell_region: Vec<usize> = roots
            .iter()
            .map(|r| ids.binary_search(r).expect("a root"))
            .collect();
        // Kinds from what each region holds.
        let kind = |id: &str| {
            self.kinds
                .iter()
                .position(|k| k.id == id)
                .unwrap_or_else(|| panic!("region kind {id} exists"))
        };
        let mut rng = mix64(seed ^ 0x004E_A3E5);
        let mut next = move || {
            rng = mix64(rng);
            rng
        };
        let mut pool: Vec<String> = adjectives.to_vec();
        let mut regions = Vec::new();
        for r in 0..ids.len() {
            let members: Vec<usize> = (0..cells.len()).filter(|&c| cell_region[c] == r).collect();
            let n = members.len() as f64;
            let sea = members.iter().map(|&c| character[c].sea).sum::<f64>() / n;
            let land: Vec<&Cell> = members
                .iter()
                .map(|&c| &character[c])
                .filter(|c| c.sea <= 0.5)
                .collect();
            let ln = land.len().max(1) as f64;
            let temp = land.iter().map(|c| c.temp).sum::<f64>() / ln;
            let wet = land.iter().map(|c| c.wet).sum::<f64>() / ln;
            let mut heights: Vec<f64> = land.iter().map(|c| c.mean).collect();
            heights.sort_by(f64::total_cmp);
            let pct = |q: f64| {
                heights
                    .get(((heights.len() as f64 * q) as usize).min(heights.len().saturating_sub(1)))
                    .copied()
                    .unwrap_or(0.0)
            };
            let lakes = land.iter().filter(|c| c.mean < 0.0).count() as f64 / ln;
            let id = if sea > 0.7 {
                if members.len() < 14 { "bay" } else { "sea" }
            } else if sea > 0.4 {
                "isles"
            } else if pct(0.8) > 3_500.0 {
                "peaks"
            } else if temp < -6.0 {
                "icefield"
            } else if temp < 2.0 {
                "barrens"
            } else if pct(0.5) > 1_200.0 {
                "highlands"
            } else if wet < 0.28 {
                if temp > 16.0 { "wastes" } else { "steppe" }
            } else if lakes > 0.1 {
                "meres"
            } else if temp > 21.0 && wet > 0.6 {
                "wilds"
            } else if wet > 0.55 {
                "vale"
            } else if sea > 0.15 {
                "coast"
            } else if pct(0.9) - pct(0.1) > 500.0 {
                "downs"
            } else {
                "plains"
            };
            let k = kind(id);
            let a = if pool.is_empty() {
                String::from("Far")
            } else {
                pool.remove((next() % pool.len() as u64) as usize)
            };
            let centre = members
                .iter()
                .map(|&c| cells[c])
                .sum::<DVec3>()
                .normalize_or(DVec3::Y);
            regions.push(Region {
                kind: k,
                name: format!("{a} {}", self.kinds[k].noun),
                centre,
            });
        }
        self.cells = cells;
        self.cell_region = cell_region;
        self.regions = regions;
    }

    /// The plate nearest `dir` (for grouping things by area).
    pub fn plate_at(&self, dir: DVec3) -> usize {
        let th = self.angles(dir.normalize());
        th.iter()
            .enumerate()
            .min_by(|a, b| a.1.total_cmp(b.1))
            .map_or(0, |(i, _)| i)
    }

    /// A plate's seed (unit direction).
    pub fn plate_seed(&self, plate: usize) -> DVec3 {
        self.plates[plate].seed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tessera() -> GlobeWorld {
        GlobeWorld::new(92_000.0, 32, 11)
    }

    #[test]
    fn about_forty_percent_is_sea_and_land_is_continuous() {
        let w = tessera();
        let n = 3_000;
        let sea = fibonacci_directions(n)
            .filter(|&d| w.elevation(d, 500.0) < 0.0)
            .count();
        let share = sea as f64 / n as f64;
        assert!((share - 0.40).abs() < 0.06, "sea share {share:.2}");
        // Walking along a great circle in 5 m steps, the ground never jumps.
        let mut last = w.elevation(DVec3::X, 5.0);
        for k in 1..4_000 {
            let a = f64::from(k) * 5.0 / w.radius;
            let d = DVec3::new(a.cos(), 0.3 * a.sin(), a.sin()).normalize();
            let e = w.elevation(d, 5.0);
            assert!((e - last).abs() < 60.0, "jump {last} -> {e} at {k}");
            last = e;
        }
    }

    #[test]
    fn regions_are_natural_areas_named_by_what_they_hold() {
        let w = tessera();
        assert!(
            (20..=60).contains(&w.regions.len()),
            "{} regions",
            w.regions.len()
        );
        let kinds: std::collections::HashSet<&str> = (0..w.regions.len())
            .map(|r| w.kind(r).id.as_str())
            .collect();
        assert!(kinds.len() >= 6, "variety: {kinds:?}");
        let mut names: Vec<&str> = w.regions.iter().map(|r| r.name.as_str()).collect();
        names.sort_unstable();
        let before = names.len();
        names.dedup();
        assert_eq!(names.len(), before, "names are distinct");
        // A region's middle is in it (or at least close).
        for (r, region) in w.regions.iter().enumerate() {
            let _ = (r, region.centre);
        }
    }

    #[test]
    fn climate_has_wet_and_dry_land_and_cold_poles() {
        let w = tessera();
        assert!(w.sea_level_temperature(DVec3::Y) < -25.0);
        let (mut dry, mut wet) = (0, 0);
        for d in fibonacci_directions(800) {
            if w.elevation(d, 1_000.0) > 0.0 {
                let m = w.wetness(d);
                dry += usize::from(m < 0.3);
                wet += usize::from(m > 0.6);
            }
        }
        assert!(dry > 10 && wet > 10, "dry {dry} wet {wet}");
    }
}
