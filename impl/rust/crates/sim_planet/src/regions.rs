//! A body of plains with its land and climate (planet-geography.md §3.4–3.6).
//!
//! - **Land:** [`Geography`] (plates and creases, continents, ranges, relief).
//! - **Climate:** warmth from latitude and height; wetness carried from the sea downwind along
//!   Earth-like wind belts and wrung out by high ground (rain shadows). Computed once at load on a
//!   grid over each plain (3 km apart) and read back by bilinear interpolation, blended near
//!   creases.
//! - **Regions:** each plain's kind (sea, peaks, wastes, vale, …) is read from the land and
//!   climate on it, and named with an adjective and the kind's noun
//!   (`content/data/region_types/standard.json`).

use glam::{DVec2, DVec3};
use serde::Deserialize;

use crate::geography::{GeoParams, Geography};
use crate::noise::mix64;
use crate::plains::Plains;

/// The region kinds file, embedded so the world is the same everywhere it is computed.
const STANDARD: &str = include_str!("../../../../../content/data/region_types/standard.json");

/// Width (m of radial difference between neighbouring planes) over which the climate grids of
/// neighbouring plains blend at a crease.
pub const BLEND_BAND: f64 = 2_000.0;
/// Climate grid spacing (m).
const GRID: f64 = 3_000.0;
/// Warmth at sea level on the equator and its fall toward the poles (°C).
const EQUATOR_TEMPERATURE: f64 = 30.0;
const POLE_DROP: f64 = 62.0;
/// Temperature drop with height (°C per m).
const LAPSE: f64 = 0.0065;
/// Wetness: steps upwind (m) and how many.
const UPWIND_STEP: f64 = 3_500.0;
const UPWIND_STEPS: u32 = 18;
/// Land this much higher than the point blocks moist air (m), each time keeping this share.
const BLOCK_HEIGHT: f64 = 1_500.0;
const BLOCK_KEEP: f64 = 0.55;

/// One kind of region: what it is called.
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

/// One region: the plain's kind and its name.
#[derive(Clone, Debug)]
pub struct Region {
    /// Index into `PlainWorld::kinds`.
    pub kind: usize,
    pub name: String,
}

/// A plain's climate grid: wetness at points `GRID` apart in its plane, centred on its origin.
#[derive(Clone, Debug)]
struct Grid {
    half: usize,
    wet: Vec<f32>,
}

impl Grid {
    fn sample(&self, local: DVec2) -> f64 {
        let n = 2 * self.half + 1;
        let x = (local.x / GRID + self.half as f64).clamp(0.0, (n - 1) as f64 - 1e-9);
        let y = (local.y / GRID + self.half as f64).clamp(0.0, (n - 1) as f64 - 1e-9);
        let (ix, iy) = (x.floor() as usize, y.floor() as usize);
        let (fx, fy) = (x - ix as f64, y - iy as f64);
        let at = |i: usize, j: usize| f64::from(self.wet[j * n + i]);
        let a = at(ix, iy) * (1.0 - fx) + at(ix + 1, iy) * fx;
        let b = at(ix, iy + 1) * (1.0 - fx) + at(ix + 1, iy + 1) * fx;
        a * (1.0 - fy) + b * fy
    }
}

/// A body's plains with their land, climate and regions.
#[derive(Clone, Debug)]
pub struct PlainWorld {
    pub plains: Plains,
    pub geo: Geography,
    pub kinds: Vec<RegionKind>,
    /// One per plain, in the plains' order.
    pub regions: Vec<Region>,
    grids: Vec<Grid>,
}

/// Latitude (radians) of a surface-frame direction; +Y is north.
fn latitude(dir: DVec3) -> f64 {
    dir.y.clamp(-1.0, 1.0).asin()
}

/// The prevailing wind at `dir` (unit, tangent): three belts per hemisphere like Earth's —
/// trade winds toward the west near the equator, westerlies at mid latitudes, polar easterlies.
fn wind(dir: DVec3) -> DVec3 {
    let east = DVec3::Y.cross(dir).normalize_or(DVec3::X);
    let lat = latitude(dir).to_degrees().abs();
    if (30.0..60.0).contains(&lat) {
        east
    } else {
        -east
    }
}

impl PlainWorld {
    /// The land and climate of `plains` from `seed`, and the regions they make.
    pub fn new(plains: Plains, seed: u64) -> Self {
        let file: KindsFile =
            serde_json::from_str(STANDARD).expect("region_types/standard.json is valid");
        let geo = Geography::new(&plains, seed, GeoParams::default());
        let grids = Self::climate_grids(&plains, &geo);
        let mut world = Self {
            plains,
            geo,
            kinds: file.kinds,
            regions: Vec::new(),
            grids,
        };
        world.regions = world.read_regions(&file.adjectives, seed);
        world
    }

    /// Warmth at sea level (°C) at `dir`: from the latitude.
    pub fn sea_level_temperature(&self, dir: DVec3) -> f64 {
        let lat = latitude(dir.normalize()).to_degrees().abs();
        EQUATOR_TEMPERATURE - POLE_DROP * (lat / 90.0).powi(2)
    }

    /// Temperature (°C) at `dir` and `elevation` (m above sea level).
    pub fn temperature(&self, dir: DVec3, elevation: f64) -> f64 {
        self.sea_level_temperature(dir) - LAPSE * elevation.max(0.0)
    }

    /// Wetness (0–1) at `dir`, from the climate grids, blended near creases.
    pub fn wetness(&self, dir: DVec3) -> f64 {
        let dir = dir.normalize();
        let mut weights = Vec::with_capacity(4);
        self.plains.weights(dir, BLEND_BAND, &mut weights);
        let point = dir * self.plains.hit(dir, 0.0).radius;
        weights
            .iter()
            .map(|&(i, w)| {
                let p = &self.plains.plains[i];
                w * self.grids[i].sample(p.to_local(point).truncate())
            })
            .sum()
    }

    /// The kind of plain `i`'s region.
    pub fn kind(&self, i: usize) -> &RegionKind {
        &self.kinds[self.regions[i].kind]
    }

    /// Wetness at `dir` computed from scratch (the grids store it): sea is wet; over land, sea
    /// upwind within ~50 km brings moisture, less the farther it is, and each crossing of high
    /// ground on the way wrings some out.
    fn wetness_at(plains: &Plains, geo: &Geography, dir: DVec3) -> f64 {
        let here = geo.elevation(plains, dir, 2_000.0);
        if here < 0.0 {
            return 1.0;
        }
        let radius = plains.hit(dir, 0.0).radius;
        let upwind = -wind(dir);
        let (mut wet, mut total, mut keep) = (0.0, 0.0, 1.0);
        let mut at = dir;
        for k in 1..=UPWIND_STEPS {
            let step = upwind - at * upwind.dot(at);
            at = (at + step.normalize_or(DVec3::ZERO) * (UPWIND_STEP / radius)).normalize();
            let e = geo.elevation(plains, at, 2_000.0);
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

    fn climate_grids(plains: &Plains, geo: &Geography) -> Vec<Grid> {
        let reach = plains
            .plains
            .iter()
            .flat_map(|p| p.corners.iter().map(move |c| c.distance(p.origin())))
            .fold(0.0, f64::max)
            + 2.0 * GRID;
        let half = (reach / GRID).ceil() as usize;
        let n = 2 * half + 1;
        // One plain per task, on all cores.
        let jobs: Vec<usize> = (0..plains.plains.len()).collect();
        let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
        let chunks: Vec<&[usize]> = jobs.chunks(jobs.len().div_ceil(threads).max(1)).collect();
        let mut grids: Vec<(usize, Grid)> = std::thread::scope(|s| {
            let handles: Vec<_> = chunks
                .iter()
                .map(|chunk| {
                    s.spawn(move || {
                        chunk
                            .iter()
                            .map(|&i| {
                                let p = &plains.plains[i];
                                let mut wet = Vec::with_capacity(n * n);
                                for j in 0..n {
                                    for k in 0..n {
                                        let local = DVec3::new(
                                            (k as f64 - half as f64) * GRID,
                                            (j as f64 - half as f64) * GRID,
                                            0.0,
                                        );
                                        let dir = p.from_local(local).normalize();
                                        wet.push(Self::wetness_at(plains, geo, dir) as f32);
                                    }
                                }
                                (i, Grid { half, wet })
                            })
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            handles
                .into_iter()
                .flat_map(|h| h.join().expect("climate grid thread"))
                .collect()
        });
        grids.sort_by_key(|(i, _)| *i);
        grids.into_iter().map(|(_, g)| g).collect()
    }

    /// Each plain's kind from what is on it, then names.
    fn read_regions(&self, adjectives: &[String], seed: u64) -> Vec<Region> {
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
        self.plains
            .plains
            .iter()
            .map(|p| {
                // Samples over the plain.
                let (mut sea, mut lakes, mut n) = (0.0f64, 0.0f64, 0.0f64);
                let (mut temp, mut wet, mut high, mut sum, mut sum2) =
                    (0.0, 0.0, f64::MIN, 0.0, 0.0);
                for s in 0..150 {
                    let a = f64::from(s) * 2.399_963;
                    let r = 30_000.0 * (f64::from(s) / 150.0).sqrt();
                    let dir = p
                        .from_local(DVec3::new(r * a.cos(), r * a.sin(), 0.0))
                        .normalize();
                    let e = self.geo.elevation(&self.plains, dir, 1_000.0);
                    n += 1.0;
                    if e < 0.0 {
                        if self.geo.open_sea(&self.plains, dir) {
                            sea += 1.0;
                        } else {
                            lakes += 1.0;
                        }
                        continue;
                    }
                    temp += self.temperature(dir, e);
                    wet += self.wetness(dir);
                    high = f64::max(high, e);
                    sum += e;
                    sum2 += e * e;
                }
                let land = (n - sea - lakes).max(1.0);
                let (temp, wet, mean) = (temp / land, wet / land, sum / land);
                let rough = (sum2 / land - mean * mean).max(0.0).sqrt();
                let id = if sea / n > 0.7 {
                    "sea"
                } else if sea / n > 0.45 {
                    "isles"
                } else if high > 5_000.0 {
                    "peaks"
                } else if temp < -6.0 {
                    "icefield"
                } else if temp < 2.0 {
                    "barrens"
                } else if mean > 1_500.0 || high > 3_000.0 {
                    "highlands"
                } else if wet < 0.28 {
                    if temp > 16.0 { "wastes" } else { "steppe" }
                } else if lakes / n > 0.08 {
                    "meres"
                } else if temp > 21.0 && wet > 0.6 {
                    "wilds"
                } else if wet > 0.55 {
                    "vale"
                } else if sea / n > 0.2 {
                    "coast"
                } else if rough > 250.0 {
                    "downs"
                } else {
                    "plains"
                };
                let k = kind(id);
                let a = pool.remove((next() % pool.len() as u64) as usize);
                Region {
                    kind: k,
                    name: format!("{a} {}", self.kinds[k].noun),
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn world() -> PlainWorld {
        PlainWorld::new(Plains::truncated_icosahedron(40_000.0), 11)
    }

    #[test]
    fn regions_are_read_from_the_land_and_named() {
        let w = world();
        assert_eq!(w.regions.len(), 32);
        let mut names: Vec<&str> = w.regions.iter().map(|r| r.name.as_str()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), 32, "names are distinct");
        let kinds: std::collections::HashSet<&str> =
            (0..32).map(|i| w.kind(i).id.as_str()).collect();
        assert!(kinds.len() >= 5, "a variety of regions: {kinds:?}");
    }

    #[test]
    fn poles_are_cold_and_the_equator_warm() {
        let w = world();
        assert!(w.sea_level_temperature(DVec3::Y) < -25.0);
        assert!(w.sea_level_temperature(DVec3::X) > 25.0);
        assert!(w.temperature(DVec3::X, 5_000.0) < w.temperature(DVec3::X, 0.0) - 30.0);
    }

    #[test]
    fn the_sea_is_wet_and_some_land_is_dry() {
        let w = world();
        let mut dry = 0;
        let mut wet = 0;
        for d in crate::terrain::fibonacci_directions(600) {
            let e = w.geo.elevation(&w.plains, d, 1_000.0);
            let m = w.wetness(d);
            if e < -500.0 {
                assert!(m > 0.6, "open water is wet: {m}");
            } else if e > 0.0 {
                if m < 0.3 {
                    dry += 1;
                }
                if m > 0.6 {
                    wet += 1;
                }
            }
        }
        assert!(dry > 10 && wet > 10, "dry land {dry}, wet land {wet}");
    }
}
