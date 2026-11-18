//! The land, climate and regions of a round planet (planet-geography.md §1b).
//!
//! Generated once at load as a short simulation of the planet's history on a mesh of about
//! 160 thousand points (`generate`): plates that turn about their own poles, crust and uplift
//! where they collide or part, rivers cutting the rising land, winds carrying rain from the sea and
//! wrung out over ranges, rivers, lakes and biomes; then regions as river basins (`regions`). At
//! runtime the fields are interpolated over the mesh and noise adds the detail below its scale.
//!
//! Heights are radial, above the reference radius (sea level).

mod generate;
mod regions;
pub mod sphere;

use glam::DVec3;
use serde::Deserialize;

pub use generate::Cover;
use generate::Fields;
use sphere::Sphere;

use crate::noise::{fbm, mix64, octaves_between};

const KINDS: &str = include_str!("../../../../../../content/data/region_types/standard.json");

/// Mesh subdivision: 10·4⁷ + 2 = 163,842 points (about 0.9 km apart on a 92 km planet).
const LEVEL: u32 = 7;

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
pub struct GlobeWorld {
    pub radius: f64,
    seed: u64,
    detail_seed: u64,
    sphere: Sphere,
    fields: Fields,
    wet: Vec<f64>,
    open: Vec<f64>,
    region: Vec<u32>,
    pub kinds: Vec<RegionKind>,
    pub regions: Vec<Region>,
}

impl std::fmt::Debug for GlobeWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GlobeWorld")
            .field("radius", &self.radius)
            .field("points", &self.sphere.len())
            .field("regions", &self.regions.len())
            .finish()
    }
}

impl GlobeWorld {
    pub fn new(radius: f64, plates: usize, seed: u64) -> Self {
        Self::with_level(radius, plates, seed, LEVEL)
    }

    /// With a coarser or finer mesh (tests use coarser ones).
    pub fn with_level(radius: f64, plates: usize, seed: u64, level: u32) -> Self {
        let sphere = Sphere::new(level);
        let fields = generate::generate(&sphere, radius, plates.max(2), seed);
        let kinds_file: KindsFile =
            serde_json::from_str(KINDS).expect("region_types/standard.json is valid");
        let temp = |i: usize| {
            generate::sea_level_temperature(sphere.points[i], seed) - 0.0065 * fields.h[i].max(0.0)
        };
        let (region, regions) = regions::grow(
            &sphere,
            &fields,
            radius,
            seed,
            &kinds_file.kinds,
            &kinds_file.adjectives,
            temp,
        );
        let wet = fields
            .rain
            .iter()
            .map(|r| (r / 1_800.0).clamp(0.0, 1.0))
            .collect();
        let open = fields
            .open
            .iter()
            .map(|&o| f64::from(u8::from(o)))
            .collect();
        Self {
            radius,
            seed,
            detail_seed: mix64(seed ^ 0x00DE_7A11),
            sphere,
            fields,
            wet,
            open,
            region,
            kinds: kinds_file.kinds,
            regions,
        }
    }

    /// Elevation (m above sea level) at the unit direction `dir`, detail down to `shortest`.
    pub fn elevation(&self, dir: DVec3, shortest: f64) -> f64 {
        let dir = dir.normalize();
        let (face, w) = self.sphere.locate(dir);
        let at = |f: &[f64]| (0..3).map(|k| f[face[k] as usize] * w[k]).sum::<f64>();
        let h = at(&self.fields.h);
        let rough = at(&self.fields.rough);
        // Detail below the mesh's scale, strongest where the ground is rough.
        let longest = 2.0 * self.sphere.spacing * self.radius;
        if shortest >= longest {
            return h;
        }
        let b = dir * self.radius;
        let octaves = octaves_between(longest, shortest);
        let land = generate::smooth(-300.0, 100.0, h);
        let amplitude = (30.0 + 0.6 * rough) * (0.3 + 0.7 * land);
        h + amplitude * 2.0 * fbm(b / longest, self.detail_seed, octaves, 0.5)
    }

    /// Whether `dir` lies in the open sea (not a basin below sea level inland).
    pub fn open_sea(&self, dir: DVec3) -> bool {
        self.sphere.sample(&self.open, dir.normalize()) > 0.0
    }

    /// The lowest and highest the ground can be (m above sea level).
    pub fn range(&self) -> (f64, f64) {
        let (mut lo, mut hi) = (f64::MAX, f64::MIN);
        for (h, r) in self.fields.h.iter().zip(&self.fields.rough) {
            let detail = 2.0 * (30.0 + 0.6 * r);
            lo = lo.min(h - detail);
            hi = hi.max(h + detail);
        }
        (lo, hi)
    }

    /// Warmth at sea level (°C) at `dir`.
    pub fn sea_level_temperature(&self, dir: DVec3) -> f64 {
        generate::sea_level_temperature(dir.normalize(), self.seed)
    }

    /// Temperature (°C) at `dir` and `elevation`.
    pub fn temperature(&self, dir: DVec3, elevation: f64) -> f64 {
        self.sea_level_temperature(dir) - 0.0065 * elevation.max(0.0)
    }

    /// Wetness (0–1; 1 is about 1.8 m of rain a year) at `dir`.
    pub fn wetness(&self, dir: DVec3) -> f64 {
        self.sphere.sample(&self.wet, dir.normalize())
    }

    /// Rain (mm a year) at `dir`.
    pub fn rain(&self, dir: DVec3) -> f64 {
        self.sphere.sample(&self.fields.rain, dir.normalize())
    }

    /// What covers the ground nearest `dir`.
    pub fn cover(&self, dir: DVec3) -> Cover {
        self.fields.cover[self.sphere.nearest(dir.normalize())]
    }

    /// What grows on the ground at `dir` and `height` (m), from the interpolated rain and
    /// warmth (smooth boundaries), `jitter` (about ±0.5) shifting the boundaries a little.
    pub fn ground_cover(&self, dir: DVec3, height: f64, jitter: f64) -> Cover {
        let d = dir.normalize();
        let t = self.temperature(d, height) + 2.0 * jitter;
        let p = self.rain(d) * (1.0 + 0.25 * jitter);
        generate::land_cover(t, p, height)
    }

    /// Whether a river runs at the point nearest `dir`.
    pub fn river(&self, dir: DVec3) -> bool {
        self.fields.river[self.sphere.nearest(dir.normalize())]
    }

    /// The region at `dir` (index into `regions`).
    pub fn region_at(&self, dir: DVec3) -> usize {
        self.region[self.sphere.nearest(dir.normalize())] as usize
    }

    /// The kind of region `r`.
    pub fn kind(&self, r: usize) -> &RegionKind {
        &self.kinds[self.regions[r].kind]
    }

    /// The plate under `dir` (for grouping things by area).
    pub fn plate_at(&self, dir: DVec3) -> usize {
        self.fields.plate[self.sphere.nearest(dir.normalize())] as usize
    }

    /// A plate's seed (unit direction).
    pub fn plate_seed(&self, plate: usize) -> DVec3 {
        self.fields.plate_seeds[plate]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terrain::fibonacci_directions;
    use std::sync::OnceLock;

    /// Tessera on a coarser mesh (fast enough for tests).
    fn tessera() -> &'static GlobeWorld {
        static W: OnceLock<GlobeWorld> = OnceLock::new();
        W.get_or_init(|| GlobeWorld::with_level(92_000.0, 12, 11, 6))
    }

    #[test]
    fn about_forty_percent_is_sea_and_the_ground_is_continuous() {
        let w = tessera();
        let n = 3_000;
        let sea = fibonacci_directions(n)
            .filter(|&d| w.elevation(d, 500.0) < 0.0)
            .count();
        let share = sea as f64 / n as f64;
        assert!((share - 0.40).abs() < 0.08, "sea share {share:.2}");
        let (lo, hi) = w.range();
        assert!(
            hi > 4_000.0 && hi < 10_000.0 && lo < -4_000.0,
            "range {lo} {hi}"
        );
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
        assert!(kinds.contains("sea"));
        let mut names: Vec<&str> = w.regions.iter().map(|r| r.name.as_str()).collect();
        names.sort_unstable();
        let before = names.len();
        names.dedup();
        assert_eq!(names.len(), before, "names are distinct");
    }

    #[test]
    fn climate_has_deserts_forests_rivers_and_cold_poles() {
        let w = tessera();
        assert!(w.sea_level_temperature(DVec3::Y) < -20.0);
        let mut seen = std::collections::HashSet::new();
        let mut rivers = 0;
        for d in fibonacci_directions(4_000) {
            seen.insert(w.cover(d));
            rivers += usize::from(w.river(d));
        }
        for c in [Cover::Desert, Cover::Forest, Cover::Ice, Cover::Sea] {
            assert!(seen.contains(&c), "{c:?} missing: {seen:?}");
        }
        assert!(rivers > 5, "rivers {rivers}");
    }
}
