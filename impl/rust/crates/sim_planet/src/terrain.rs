//! The terrain function (planet-surface.md §2; task 2.1).
//!
//! One deterministic function of a direction from the planet's center gives the surface
//! height; rendering, collision and gameplay all use it. Directions are unit vectors in the
//! body's **surface frame** (+Y is the north pole).
//!
//! Detail is requested by wavelength: `min_wavelength` drops noise octaves shorter than it, and
//! because fractal sums are normalised by the full series, coarse samples are the same terrain
//! minus its finest detail. Land is always above sea level and ocean floor always below, so the
//! coastline is set by the continent field alone and the ocean fraction matches the data.

use std::sync::Arc;

use glam::DVec3;
use sim_core::{Body, TerrainData};

use crate::blocks::BlockGround;
use crate::foundations::Foundations;
use crate::noise::{fbm, hash3, mix64, octaves_between, ridged, unit};

/// Continents: about 1.3 cycles across the radius.
const CONTINENT_FREQUENCY: f64 = 1.3;
const CONTINENT_OCTAVES: u32 = 5;
/// Mountain-range placement mask.
const RANGE_FREQUENCY: f64 = 2.5;
/// Mountain ridges: wavelength = radius / 6.
const RIDGE_FREQUENCY: f64 = 6.0;
/// Shortest ridge wavelength (m); finer relief comes from hills.
const RIDGE_SHORTEST: f64 = 200.0;
/// Ridged sums peak near 0.6 of their normalisation; this brings crests to the full relief.
const RIDGE_GAIN: f64 = 1.6;
/// Hills: wavelength = radius / 40.
const HILL_FREQUENCY: f64 = 40.0;
/// Shortest wavelength generated at all (m).
pub const FINEST_WAVELENGTH: f64 = 1.0;
/// Samples used to place the coastline at the requested ocean fraction.
const COAST_SAMPLES: usize = 8_192;
/// Crater layers: cell size as a fraction of the radius, and how strongly density applies.
const CRATER_LAYERS: [(f64, f64); 3] = [(1.0 / 8.0, 0.35), (1.0 / 30.0, 0.6), (1.0 / 120.0, 0.9)];

/// One terrain sample.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    /// Height above the reference radius (m); negative below sea level.
    pub height: f64,
    /// Continent signal: positive on land, negative under the ocean; distance from the coast
    /// in noise units (used for moisture and beaches).
    pub land: f64,
}

/// A body's terrain: parameters plus values derived once (the coastline threshold).
#[derive(Clone, Debug)]
pub struct Terrain {
    pub radius: f64,
    pub params: TerrainData,
    /// Continent value at the coastline.
    coast: f64,
    seeds: [u64; 6],
    /// Player-made changes (terrain deltas, planet-surface.md §6), applied in order on top of
    /// the generated heights, so regeneration always gives the same result.
    pads: Vec<Pad>,
    /// Settlement foundations (generated from the layouts, not saved), under the pads.
    foundations: Option<Arc<Foundations>>,
    /// The ground as blocks round the demo village (block-world.md): its surface replaces the
    /// terrain's there.
    blocks: Option<Arc<BlockGround>>,
}

/// A flattened area (task 2.11): inside `radius` the ground is at `height`; over the next
/// `blend` metres it eases back to the natural terrain.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pad {
    /// Centre direction (unit, surface frame).
    pub center: DVec3,
    pub radius: f64,
    pub blend: f64,
    /// Ground height inside the pad (m above the reference radius).
    pub height: f64,
}

impl Pad {
    /// How much of the pad's height applies at `dir` (1 inside, easing to 0 at the edge of the
    /// blend), on a body of `radius`.
    fn weight(&self, dir: DVec3, radius: f64) -> f64 {
        let distance = self.center.angle_between(dir) * radius;
        1.0 - smoothstep(self.radius, self.radius + self.blend, distance)
    }

    /// Angular reach (radians) on a body of `radius`.
    pub fn reach(&self, radius: f64) -> f64 {
        (self.radius + self.blend) / radius
    }
}

fn smoothstep(e0: f64, e1: f64, x: f64) -> f64 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Evenly spread unit directions (Fibonacci sphere).
pub fn fibonacci_directions(n: usize) -> impl Iterator<Item = DVec3> {
    let golden = std::f64::consts::PI * (3.0 - 5f64.sqrt());
    (0..n).map(move |i| {
        let y = 1.0 - 2.0 * (i as f64 + 0.5) / n as f64;
        let r = (1.0 - y * y).sqrt();
        let a = golden * i as f64;
        DVec3::new(r * a.cos(), y, r * a.sin())
    })
}

impl Terrain {
    /// The terrain of a body with a `terrain` block; `None` for stars, gas giants and bodies
    /// without terrain data.
    pub fn of(body: &Body) -> Option<Self> {
        Some(Self::new(body.radius, body.terrain.clone()?))
    }

    pub fn new(radius: f64, params: TerrainData) -> Self {
        let base = mix64(params.seed);
        let seeds = std::array::from_fn(|i| mix64(base ^ (i as u64 + 1)));
        let mut terrain = Self {
            radius,
            params,
            coast: 0.0,
            seeds,
            pads: Vec::new(),
            foundations: None,
            blocks: None,
        };
        terrain.coast = terrain.coast_threshold();
        terrain
    }

    /// This terrain with one more pad (copy-on-write: the original stays valid for anything
    /// still using it).
    pub fn with_pad(&self, pad: Pad) -> Terrain {
        let mut t = self.clone();
        t.pads.push(pad);
        t
    }

    /// This terrain with its pads replaced by `pads` (a loaded save's deltas).
    pub fn with_pads(&self, pads: Vec<Pad>) -> Terrain {
        let mut t = self.clone();
        t.pads = pads;
        t
    }

    pub fn pads(&self) -> &[Pad] {
        &self.pads
    }

    /// This terrain with the settlements' foundations (the pads stay).
    pub fn with_foundations(&self, foundations: Arc<Foundations>) -> Terrain {
        let mut t = self.clone();
        t.foundations = Some(foundations);
        t
    }

    pub fn foundations(&self) -> Option<&Arc<Foundations>> {
        self.foundations.as_ref()
    }

    /// This terrain with the block ground (the pads and foundations stay).
    pub fn with_blocks(&self, blocks: Arc<BlockGround>) -> Terrain {
        let mut t = self.clone();
        t.blocks = Some(blocks);
        t
    }

    pub fn blocks(&self) -> Option<&Arc<BlockGround>> {
        self.blocks.as_ref()
    }

    /// Height of the coarse collision patches at `dir`: the surface, sunk below the block
    /// ground (whose own collision meshes carry contact there).
    pub fn collision_height(&self, dir: DVec3, min_wavelength: f64) -> f64 {
        match self.blocks.as_ref().and_then(|b| b.surface(dir)) {
            Some(h) => h - crate::blocks::PATCH_SINK,
            None => self.surface_height(dir, min_wavelength),
        }
    }

    /// Whether `pad` reaches any point within `spread` radians of the unit direction `center`
    /// (e.g. a tile's centre and its corner spread).
    pub fn pad_touches(&self, pad: &Pad, center: DVec3, spread: f64) -> bool {
        pad.center.angle_between(center) <= spread + pad.reach(self.radius)
    }

    /// Whether the body has an ocean (sea level at height 0).
    pub fn has_ocean(&self) -> bool {
        self.params.ocean_coverage > 0.0
    }

    /// Height of what vehicles and walkers stand on at `dir` (m): the ground, or the sea surface
    /// where there is water (a floor until swimming and floating exist).
    pub fn surface_height(&self, dir: DVec3, min_wavelength: f64) -> f64 {
        if let Some(h) = self.blocks.as_ref().and_then(|b| b.surface(dir)) {
            return h;
        }
        let h = self.height(dir, min_wavelength);
        if self.has_ocean() { h.max(0.0) } else { h }
    }

    fn continent(&self, dir: DVec3) -> f64 {
        fbm(
            dir * CONTINENT_FREQUENCY,
            self.seeds[0],
            CONTINENT_OCTAVES,
            0.5,
        )
    }

    /// The continent value below which a fraction `ocean_coverage` of the surface lies.
    fn coast_threshold(&self) -> f64 {
        let mut values: Vec<f64> = fibonacci_directions(COAST_SAMPLES)
            .map(|d| self.continent(d))
            .collect();
        values.sort_by(f64::total_cmp);
        let q = self.params.ocean_coverage.clamp(0.0, 1.0);
        let i = ((values.len() as f64 * q) as usize).min(values.len() - 1);
        values[i]
    }

    /// Height and continent signal at `dir` (unit, surface frame), with detail down to
    /// `min_wavelength` metres.
    pub fn sample(&self, dir: DVec3, min_wavelength: f64) -> Sample {
        let p = &self.params;
        let r = self.radius;
        let shortest = min_wavelength.max(FINEST_WAVELENGTH);
        let mut land = self.continent(dir) - self.coast;
        if !self.has_ocean() {
            // No ocean: everything is land (the lowest point sits at the old coastline).
            land = land.max(0.0) + 0.02;
        }

        let hill_octaves = octaves_between(r / HILL_FREQUENCY, shortest);
        // Fractal sums span about ±0.5; stretch to [0, 1].
        let hills01 =
            (0.5 + fbm(dir * HILL_FREQUENCY, self.seeds[3], hill_octaves, 0.5)).clamp(0.0, 1.0);

        let mut height = if land > 0.0 {
            let lowland = p.relief * 0.04 * smoothstep(0.0, 0.25, land) + 2.0;
            let range = (0.5 + fbm(dir * RANGE_FREQUENCY, self.seeds[1], 3, 0.5)).clamp(0.0, 1.0);
            let cutoff = 1.0 - p.mountains;
            let mask = smoothstep(0.02, 0.2, land) * smoothstep(cutoff - 0.1, cutoff + 0.1, range);
            let peaks = if mask > 0.0 {
                let octaves = octaves_between(r / RIDGE_FREQUENCY, shortest.max(RIDGE_SHORTEST));
                let ridge = ridged(dir * RIDGE_FREQUENCY, self.seeds[2], octaves, 0.5);
                // Ridged sums peak near 0.6; scale so crests reach the full relief.
                p.relief * mask * (ridge * RIDGE_GAIN).min(1.0)
            } else {
                0.0
            };
            let hills =
                p.relief * 0.08 * p.roughness * hills01 * smoothstep(0.0, 0.05, land).max(0.05);
            lowland + peaks + hills
        } else {
            let floor = p.ocean_depth * smoothstep(0.0, 0.3, -land) + 5.0;
            -(floor + p.relief * 0.02 * hills01)
        };

        if p.craters > 0.0 {
            // Overlapping craters add up; keep the total within the body's relief.
            height += self.craters(dir, shortest).clamp(-p.relief, 0.5 * p.relief);
        }
        if let Some(f) = &self.foundations {
            height = f.apply(dir, r, height);
        }
        for pad in &self.pads {
            let w = pad.weight(dir, r);
            if w > 0.0 {
                height += (pad.height - height) * w;
            }
        }
        Sample { height, land }
    }

    /// Surface height (m above the reference radius) at full detail down to `min_wavelength`.
    pub fn height(&self, dir: DVec3, min_wavelength: f64) -> f64 {
        self.sample(dir, min_wavelength).height
    }

    /// Ground point (surface frame, m) under `dir`; the ocean floor where there is water.
    pub fn ground_point(&self, dir: DVec3, min_wavelength: f64) -> DVec3 {
        let d = dir.normalize();
        d * (self.radius + self.height(d, min_wavelength))
    }

    /// Bowl-shaped craters with raised rims, in layers of decreasing size.
    fn craters(&self, dir: DVec3, shortest: f64) -> f64 {
        let mut total = 0.0;
        for (layer, &(cell_fraction, weight)) in CRATER_LAYERS.iter().enumerate() {
            let cell_m = self.radius * cell_fraction;
            if cell_m < 4.0 * shortest {
                break;
            }
            let scale = 1.0 / cell_fraction;
            let p = dir * scale;
            let cell = p.floor();
            let seed = self.seeds[4] ^ (layer as u64 + 1).wrapping_mul(0x9E37_79B9);
            let probability = (self.params.craters * weight).clamp(0.0, 1.0);
            for dz in -1..=1 {
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        let c = cell + DVec3::new(dx as f64, dy as f64, dz as f64);
                        let h = hash3(c.x as i64, c.y as i64, c.z as i64, seed);
                        if unit(h) >= probability {
                            continue;
                        }
                        let jitter =
                            DVec3::new(unit(mix64(h ^ 1)), unit(mix64(h ^ 2)), unit(mix64(h ^ 3)));
                        let raw = c + jitter;
                        if raw.length_squared() < 1e-12 {
                            continue;
                        }
                        // Crater centre on the sphere (same units as `p`).
                        let center = raw.normalize() * scale;
                        let radius = 0.15 + 0.3 * unit(mix64(h ^ 4));
                        let x = (p - center).length() / radius;
                        if x > 1.6 {
                            continue;
                        }
                        let radius_m = radius * cell_m;
                        let depth = (0.12 * radius_m).min(0.8 * self.params.relief);
                        let bowl = if x < 1.0 { depth * (x * x - 1.0) } else { 0.0 };
                        let rim = 0.25 * depth * (-((x - 1.0) / 0.25).powi(2)).exp();
                        total += bowl + rim;
                    }
                }
            }
        }
        total
    }

    /// Approximate surface slope (radians from horizontal) at `dir`, from finite differences
    /// over `step` metres.
    pub fn slope(&self, dir: DVec3, step: f64, min_wavelength: f64) -> f64 {
        let d = dir.normalize();
        let t1 = d.any_orthonormal_vector();
        let t2 = d.cross(t1);
        let angle = step / self.radius;
        let h = |v: DVec3| self.height(v.normalize(), min_wavelength);
        let h0 = h(d);
        let g1 = (h(d + t1 * angle) - h0) / step;
        let g2 = (h(d + t2 * angle) - h0) / step;
        (g1 * g1 + g2 * g2).sqrt().atan()
    }

    /// Regional variation used by climate, seeded per planet; `channel` picks an independent
    /// field (0 = moisture, 1 = temperature). Roughly ±0.5.
    pub fn climate_noise(&self, dir: DVec3, channel: u64) -> f64 {
        fbm(
            dir * 3.0,
            self.seeds[5].wrapping_add(channel.wrapping_mul(0x632B_E5AB)),
            4,
            0.5,
        )
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use sim_core::StarSystem;

    use super::*;

    fn halcyon() -> StarSystem {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../content/data/star_systems/halcyon.json");
        StarSystem::from_path(path).unwrap()
    }

    fn terrain(id: &str) -> Terrain {
        Terrain::of(halcyon().body(id).unwrap()).unwrap()
    }

    #[test]
    fn only_rocky_bodies_with_data_have_terrain() {
        let sys = halcyon();
        for body in &sys.bodies {
            let has = Terrain::of(body).is_some();
            let expected = !matches!(body.id.as_str(), "halcyon" | "leviathan");
            assert_eq!(has, expected, "{}", body.id);
        }
    }

    #[test]
    fn deterministic() {
        let a = terrain("meridian");
        let b = terrain("meridian");
        let d = DVec3::new(0.3, 0.5, -0.8).normalize();
        assert_eq!(a.height(d, 1.0), b.height(d, 1.0));
    }

    #[test]
    fn ocean_fraction_matches_the_data() {
        let t = terrain("meridian");
        // Independent of the coastline samples: a different, larger set.
        let n = 20_011;
        let wet = fibonacci_directions(n)
            .map(|d| DVec3::new(d.z, d.x, d.y))
            .filter(|&d| t.height(d, 2_000.0) < 0.0)
            .count();
        let fraction = wet as f64 / n as f64;
        assert!(
            (fraction - 0.62).abs() < 0.02,
            "ocean fraction {fraction:.3}"
        );
    }

    #[test]
    fn heights_stay_within_the_relief() {
        for id in ["cinder", "meridian", "lantern", "hoarfell", "forge", "rime"] {
            let t = terrain(id);
            let (mut lo, mut hi) = (f64::MAX, f64::MIN);
            for d in fibonacci_directions(5_000) {
                let h = t.height(d, 50.0);
                lo = lo.min(h);
                hi = hi.max(h);
            }
            let p = &t.params;
            assert!(
                hi <= 1.25 * p.relief,
                "{id}: max {hi:.0} m > relief {:.0}",
                p.relief
            );
            assert!(
                hi > 0.3 * p.relief,
                "{id}: max {hi:.0} m barely uses the relief"
            );
            assert!(
                lo >= -(p.ocean_depth + 1.0 * p.relief),
                "{id}: min {lo:.0} m too deep"
            );
        }
    }

    #[test]
    fn coarse_detail_is_the_same_terrain_minus_fine_detail() {
        let t = terrain("meridian");
        let mut worst: f64 = 0.0;
        for d in fibonacci_directions(2_000) {
            let fine = t.height(d, 1.0);
            let coarse = t.height(d, 1_000.0);
            if fine.signum() == coarse.signum() {
                worst = worst.max((fine - coarse).abs());
            }
        }
        assert!(
            worst < 0.03 * t.params.relief,
            "LOD difference {worst:.1} m"
        );
    }

    #[test]
    fn continuous_at_walking_scale() {
        let t = terrain("meridian");
        let d = DVec3::new(0.1, 0.2, 0.97).normalize();
        let step = d.any_orthonormal_vector() * (1.0 / t.radius); // 1 m
        let dh = (t.height((d + step).normalize(), 1.0) - t.height(d, 1.0)).abs();
        assert!(dh < 5.0, "1 m step changes height by {dh:.2} m");
    }

    #[test]
    fn airless_bodies_have_craters() {
        // Saturated cratering is realistic; check that bowls cover a sensible share and that
        // rims rise above the surroundings somewhere.
        let t = terrain("lantern");
        let n = 4_000;
        let (mut bowls, mut rims) = (0usize, 0usize);
        for d in fibonacci_directions(n) {
            let c = t.craters(d, 50.0);
            bowls += usize::from(c < -1.0);
            rims += usize::from(c > 1.0);
        }
        let share = bowls as f64 / n as f64;
        assert!((0.1..0.85).contains(&share), "bowls cover {share:.2}");
        assert!(rims > n / 50, "rims at {rims} samples");
    }

    #[test]
    fn a_pad_flattens_the_ground_and_regenerates_identically() {
        let t = terrain("hoarfell");
        // A rough spot: the steepest of a few samples.
        let center = fibonacci_directions(2_000)
            .max_by(|a, b| t.slope(*a, 50.0, 1.0).total_cmp(&t.slope(*b, 50.0, 1.0)))
            .unwrap();
        let pad = Pad {
            center,
            radius: 30.0,
            blend: 20.0,
            height: t.height(center, 1.0),
        };
        let flat = t.with_pad(pad);
        let side = center.any_orthonormal_vector();
        let at = |m: f64| (center + side * (m / t.radius)).normalize();
        // Inside the pad: level at the pad height.
        for m in [0.0, 10.0, 25.0] {
            assert!((flat.height(at(m), 1.0) - pad.height).abs() < 1e-6, "{m} m");
        }
        // Outside the blend: untouched.
        assert_eq!(flat.height(at(60.0), 1.0), t.height(at(60.0), 1.0));
        // The original terrain is unchanged, and the edit regenerates identically.
        assert_ne!(t.height(at(10.0), 1.0), flat.height(at(10.0), 1.0));
        let again = t.with_pad(pad);
        assert_eq!(again.height(at(40.0), 1.0), flat.height(at(40.0), 1.0));
        assert!(flat.pad_touches(&pad, at(45.0), 0.0));
        assert!(!flat.pad_touches(&pad, at(500.0), 0.0));
    }

    #[test]
    fn slope_is_steeper_in_mountains() {
        let t = terrain("hoarfell");
        let mut slopes: Vec<f64> = fibonacci_directions(3_000)
            .map(|d| t.slope(d, 200.0, 50.0))
            .collect();
        slopes.sort_by(f64::total_cmp);
        let median = slopes[slopes.len() / 2];
        let top = slopes[slopes.len() * 99 / 100];
        assert!(
            top > 3.0 * median && top > 0.2,
            "median {median:.3}, 99th {top:.3}"
        );
    }
}
