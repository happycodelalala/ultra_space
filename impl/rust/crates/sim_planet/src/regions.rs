//! Regions of a body built of plains (block-world.md §5.1): each plain is one region of a kind
//! (grassland, mountains, sea …) from `content/data/region_types/standard.json`, with its own
//! climate, ground shape and name.
//!
//! The ground's elevation (m above the plain; sea level is 0) is each region's own function of
//! the point, blended near creases with the neighbours' (by `Plains::weights`), so it is
//! continuous everywhere and every region keeps its character away from its borders.

use glam::DVec3;
use serde::Deserialize;

use crate::noise::{fbm, mix64, octaves_between, ridged};
use crate::plains::{PlainKind, Plains};

/// The region types file, embedded so terrain is the same everywhere it is computed.
const STANDARD: &str = include_str!("../../../../../content/data/region_types/standard.json");

/// Width (m of radial difference between neighbouring planes) over which regions blend at a
/// crease: about 2.5 km of ground either side on a football.
pub const BLEND_BAND: f64 = 2_000.0;

/// One kind of region.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegionType {
    pub id: String,
    pub noun: String,
    pub temperature_c: f64,
    pub moisture: f64,
    pub base_m: f64,
    pub hills_m: f64,
    pub hill_wavelength_m: f64,
    #[serde(default)]
    pub mountains_m: f64,
    #[serde(default = "default_wavelength")]
    pub mountain_wavelength_m: f64,
    #[serde(default)]
    pub valleys_m: f64,
    #[serde(default = "default_wavelength")]
    pub valley_wavelength_m: f64,
}

fn default_wavelength() -> f64 {
    5_000.0
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TypesFile {
    types: Vec<RegionType>,
    adjectives: Vec<String>,
}

/// One region: the plain's kind of land and its name.
#[derive(Clone, Debug)]
pub struct Region {
    /// Index into `PlainWorld::types`.
    pub kind: usize,
    pub name: String,
}

/// A body's plains with their regions.
#[derive(Clone, Debug)]
pub struct PlainWorld {
    pub plains: Plains,
    pub types: Vec<RegionType>,
    /// One per plain, in the plains' order.
    pub regions: Vec<Region>,
    seeds: [u64; 4],
}

impl PlainWorld {
    /// The regions of `plains`, assigned from `seed`: the two pentagons on the spin axis are ice
    /// caps, their rings cold land, two seas of four hexagons each, the rest a mix by latitude.
    pub fn new(plains: Plains, seed: u64) -> Self {
        let file: TypesFile =
            serde_json::from_str(STANDARD).expect("region_types/standard.json is valid");
        let types = file.types;
        let kind = |id: &str| {
            types
                .iter()
                .position(|t| t.id == id)
                .unwrap_or_else(|| panic!("region type {id} exists"))
        };
        let n = plains.plains.len();
        let mut rng = mix64(seed ^ 0x005E_ED0F_9A1A);
        let mut next = move || {
            rng = mix64(rng);
            rng
        };
        let mut kinds: Vec<Option<usize>> = vec![None; n];
        let lat = |i: usize| plains.plains[i].normal.y;
        // Ice caps on the spin axis.
        for (i, p) in plains.plains.iter().enumerate() {
            if p.normal.y.abs() > 0.99 {
                kinds[i] = Some(kind("polar_ice"));
            }
        }
        // Two seas, grown from hexagons far apart in the middle latitudes.
        let middle: Vec<usize> = (0..n)
            .filter(|&i| plains.plains[i].kind == PlainKind::Hexagon && lat(i).abs() < 0.6)
            .collect();
        let first = middle[(next() % middle.len() as u64) as usize];
        let second = *middle
            .iter()
            .max_by(|&&a, &&b| {
                let da = plains.plains[a].normal.dot(plains.plains[first].normal);
                let db = plains.plains[b].normal.dot(plains.plains[first].normal);
                db.total_cmp(&da)
            })
            .expect("hexagons exist");
        let sea = kind("sea");
        for start in [first, second] {
            let mut grown = vec![start];
            kinds[start] = Some(sea);
            while grown.len() < 4 {
                let options: Vec<usize> = grown
                    .iter()
                    .flat_map(|&g| plains.plains[g].neighbours.iter().copied())
                    .filter(|&j| kinds[j].is_none() && lat(j).abs() < 0.8)
                    .collect();
                if options.is_empty() {
                    break;
                }
                let j = options[(next() % options.len() as u64) as usize];
                kinds[j] = Some(sea);
                grown.push(j);
            }
        }
        // The rest: cold kinds near the poles, a mix elsewhere, warm and dry near the equator.
        let cold = [
            "tundra",
            "forest_valleys",
            "mountains",
            "lake_district",
            "tundra",
        ];
        let temperate = [
            "grassland",
            "rolling_hills",
            "forest_valleys",
            "lake_district",
            "mountains",
            "rolling_hills",
            "grassland",
        ];
        let warm = [
            "badlands",
            "grassland",
            "mountains",
            "badlands",
            "rolling_hills",
        ];
        for (i, slot) in kinds.iter_mut().enumerate() {
            if slot.is_some() {
                continue;
            }
            let pool: &[&str] = if lat(i).abs() > 0.7 {
                &cold
            } else if lat(i).abs() < 0.25 {
                &warm
            } else {
                &temperate
            };
            *slot = Some(kind(pool[(next() % pool.len() as u64) as usize]));
        }
        // At least two hexagons of open grassland or downs (room for a village).
        let open = [kind("grassland"), kind("rolling_hills")];
        let mut have = (0..n)
            .filter(|&i| {
                plains.plains[i].kind == PlainKind::Hexagon
                    && kinds[i].is_some_and(|k| open.contains(&k))
            })
            .count();
        for (i, slot) in kinds.iter_mut().enumerate() {
            if have >= 2 {
                break;
            }
            if plains.plains[i].kind == PlainKind::Hexagon
                && lat(i).abs() < 0.6
                && *slot != Some(sea)
                && !slot.is_some_and(|k| open.contains(&k))
            {
                *slot = Some(open[have % 2]);
                have += 1;
            }
        }
        // Names: a distinct adjective each, then the kind's noun.
        let mut adjectives = file.adjectives.clone();
        let regions = kinds
            .iter()
            .map(|k| {
                let k = k.expect("every plain has a kind");
                let a = adjectives.remove((next() % adjectives.len() as u64) as usize);
                Region {
                    kind: k,
                    name: format!("{a} {}", types[k].noun),
                }
            })
            .collect();
        let seeds = std::array::from_fn(|i| mix64(seed ^ (0xA11CE + i as u64)));
        Self {
            plains,
            types,
            regions,
            seeds,
        }
    }

    /// The region type of plain `i`.
    pub fn kind(&self, i: usize) -> &RegionType {
        &self.types[self.regions[i].kind]
    }

    /// Elevation (m above the plain) of region `i`'s own ground at the point `p` (surface frame,
    /// m), with detail down to `shortest` metres.
    pub fn region_elevation(&self, i: usize, p: DVec3, shortest: f64) -> f64 {
        let t = self.kind(i);
        let mut e = t.base_m;
        if t.hills_m > 0.0 {
            let o = octaves_between(t.hill_wavelength_m, shortest).max(1);
            e += 2.0 * t.hills_m * fbm(p / t.hill_wavelength_m, self.seeds[0], o, 0.5);
        }
        if t.mountains_m > 0.0 {
            let o = octaves_between(t.mountain_wavelength_m, shortest.max(60.0)).max(1);
            // Ranges where a broad mask is high; ridged crests reach the full height.
            let mask = (0.5 + fbm(p / (2.5 * t.mountain_wavelength_m), self.seeds[1], 2, 0.5))
                .clamp(0.0, 1.0);
            let ridge = ridged(p / t.mountain_wavelength_m, self.seeds[2], o, 0.5);
            e += t.mountains_m * smooth(0.35, 0.65, mask) * (1.6 * ridge).min(1.0);
        }
        if t.valleys_m > 0.0 {
            // Channels where a noise field crosses zero.
            let o = octaves_between(t.valley_wavelength_m, shortest.max(30.0)).clamp(1, 3);
            let v = fbm(p / t.valley_wavelength_m, self.seeds[3], o, 0.5).abs();
            e -= t.valleys_m * (1.0 - smooth(0.0, 0.12, v));
        }
        e
    }

    /// Temperature at sea level (°C) and moisture of region `i`.
    pub fn climate(&self, i: usize) -> (f64, f64) {
        let t = self.kind(i);
        (t.temperature_c, t.moisture)
    }
}

fn smooth(e0: f64, e1: f64, x: f64) -> f64 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn world() -> PlainWorld {
        PlainWorld::new(Plains::truncated_icosahedron(40_000.0), 7)
    }

    #[test]
    fn regions_have_kinds_and_distinct_names() {
        let w = world();
        assert_eq!(w.regions.len(), 32);
        let mut names: Vec<&str> = w.regions.iter().map(|r| r.name.as_str()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), 32, "names are distinct");
        let count = |id: &str| (0..32).filter(|&i| w.kind(i).id == id).count();
        assert_eq!(count("polar_ice"), 2);
        assert!(count("sea") >= 6, "two seas");
        assert!(count("grassland") + count("rolling_hills") >= 2);
    }

    #[test]
    fn the_same_seed_gives_the_same_regions() {
        let a = world();
        let b = world();
        for (x, y) in a.regions.iter().zip(&b.regions) {
            assert_eq!((x.kind, &x.name), (y.kind, &y.name));
        }
    }

    #[test]
    fn region_elevations_stay_in_their_ranges() {
        let w = world();
        for i in 0..32 {
            let t = w.kind(i);
            let p = &w.plains.plains[i];
            let hi = t.base_m + t.hills_m + t.mountains_m + 1.0;
            let lo = t.base_m - t.hills_m - t.valleys_m - 1.0;
            for k in 0..200 {
                let a = k as f64 * 0.37;
                let q = p.from_local(
                    DVec3::new(9_000.0 * a.cos(), 9_000.0 * a.sin(), 0.0) * (k as f64 / 200.0),
                );
                let e = w.region_elevation(i, q, 1.0);
                assert!(e <= hi && e >= lo, "{} {e} outside {lo}..{hi}", t.id);
            }
        }
    }
}
