//! Cotton clouds on a body of plains (planet-geography.md §3.7): where they are and their puffs.
//!
//! Weather systems first: a large-scale cloudiness from latitude bands (a stormy equator, clear
//! belts at 25–35°, storm tracks at mid latitudes), a few storms (swirling lows with curved arms)
//! and a slow variation. Clouds are then placed by that cloudiness **squared** (so they clump
//! into systems), helped by wetness, until about the requested share of the surface is under
//! cloud. Inside a system clouds are big clusters of many puffs; outside it small fair-weather
//! ones. Every cloud has a flat base at the condensation height above sea level (or the ground
//! below it), in its local frame (east, north, up).

use glam::DVec3;

use crate::globe::GlobeWorld;
use crate::noise::{fbm, mix64, unit};

/// One puff: its centre and half sizes in the cloud's frame (x east, y north, z up; m).
#[derive(Clone, Copy, Debug)]
pub struct Puff {
    pub offset: DVec3,
    pub radius: DVec3,
}

/// One cloud: its base centre (surface frame, m), the area it is drawn with (`chunk`: the
/// plate it is over), its frame and its puffs (in that frame).
#[derive(Clone, Debug)]
pub struct Cloud {
    pub chunk: usize,
    pub base: DVec3,
    pub east: DVec3,
    pub north: DVec3,
    pub up: DVec3,
    /// Footprint radius (m).
    pub size: f64,
    pub puffs: Vec<Puff>,
}

/// Lowest cloud base above sea level, and the least gap above the ground below (m).
const BASE: f64 = 2_500.0;
const GROUND_GAP: f64 = 1_400.0;
/// No clouds whose base would be this high (inside the high ranges).
const HIGHEST_BASE: f64 = 7_000.0;

/// A drifting storm: its centre (unit), size (radians) and hemisphere (spin).
struct Storm {
    centre: DVec3,
    size: f64,
    spin: f64,
}

fn smooth(e0: f64, e1: f64, x: f64) -> f64 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn storms(seed: u64) -> Vec<Storm> {
    let mut rng = mix64(seed ^ 0x0057_0A45);
    let mut next = move || {
        rng = mix64(rng);
        unit(rng)
    };
    (0..6)
        .map(|k| {
            let lat = (38.0 + 24.0 * next()).to_radians() * if k % 2 == 0 { 1.0 } else { -1.0 };
            let lon = next() * std::f64::consts::TAU;
            Storm {
                centre: DVec3::new(lat.cos() * lon.cos(), lat.sin(), lat.cos() * lon.sin()),
                size: 0.25 + 0.2 * next(),
                spin: lat.signum(),
            }
        })
        .collect()
}

/// Large-scale cloudiness (0–1) at the unit direction `dir`.
fn cloudiness(dir: DVec3, storms: &[Storm], seed: u64) -> f64 {
    let lat = dir.y.clamp(-1.0, 1.0).asin().to_degrees();
    let a = lat.abs();
    let band = 0.85 * (-(lat / 9.0).powi(2)).exp()
        + 0.1 * smooth(18.0, 25.0, a) * (1.0 - smooth(32.0, 40.0, a))
        + 0.55 * (-((a - 52.0) / 11.0).powi(2)).exp()
        + 0.15;
    let mut swirl: f64 = 0.0;
    for s in storms {
        let ang = dir.dot(s.centre).clamp(-1.0, 1.0).acos();
        let east = DVec3::Y.cross(s.centre).normalize_or(DVec3::X);
        let north = s.centre.cross(east);
        let theta = dir.dot(north).atan2(dir.dot(east));
        let r = ang / s.size;
        let arms = 0.5 + 0.5 * (2.0 * theta * s.spin - 6.0 * (r + 0.05).ln()).cos();
        let core = (-(r / 0.25).powi(2)).exp();
        swirl =
            swirl.max((arms * arms * (-r * r * 1.3).exp() + core) * (1.0 - smooth(0.0, 1.6, r)));
    }
    (0.7 * band + 0.9 * swirl + 0.25 * fbm(dir * 3.0, seed, 3, 0.5)).clamp(0.0, 1.0)
}

/// The clouds of `world`, covering about `cover` of the surface.
pub fn clouds(world: &GlobeWorld, seed: u64, cover: f64) -> Vec<Cloud> {
    let storms = storms(seed);
    let radius = world.radius;
    let area = 4.0 * std::f64::consts::PI * radius * radius;
    // Candidates about 2 km apart, with how likely and how big a cloud there is.
    struct Candidate {
        dir: DVec3,
        p: f64,
        size: f64,
        weather: f64,
    }
    let mut rng = mix64(seed ^ 0x000C_10D5);
    let mut next = move || {
        rng = mix64(rng);
        unit(rng)
    };
    let candidates: Vec<Candidate> = crate::terrain::fibonacci_directions(30_000)
        .filter_map(|dir| {
            let e = world.elevation(dir, 2_000.0);
            if e.max(0.0) + GROUND_GAP > HIGHEST_BASE {
                return None;
            }
            let w = cloudiness(dir, &storms, seed);
            let m = world.wetness(dir);
            let t = world.temperature(dir, 0.0);
            let mut p = w.powf(2.2) * (0.55 + 0.6 * m);
            if t < -12.0 {
                p *= 0.4;
            }
            let size = (1_300.0 + 1_700.0 * next()) * (0.7 + 1.6 * w * w);
            Some(Candidate {
                dir,
                p,
                size,
                weather: w,
            })
        })
        .collect();
    // Scale the chances so the expected footprint is `cover` of the surface.
    let footprint = |s: f64| {
        candidates
            .iter()
            .map(|c| (c.p * s).min(1.0) * std::f64::consts::PI * c.size * c.size * 0.55)
            .sum::<f64>()
    };
    let (mut lo, mut hi) = (0.0, 50.0);
    for _ in 0..30 {
        let mid = 0.5 * (lo + hi);
        if footprint(mid) < cover * area {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let scale = 0.5 * (lo + hi);
    let mut out = Vec::new();
    for c in &candidates {
        if next() >= (c.p * scale).min(1.0) {
            continue;
        }
        let ground = world.elevation(c.dir, 2_000.0).max(0.0);
        let base_height = BASE.max(ground + GROUND_GAP);
        let base = c.dir * (radius + base_height);
        let east = DVec3::Y.cross(c.dir).normalize_or(DVec3::X);
        let north = c.dir.cross(east);
        let tower = c.size * (0.5 + 0.5 * next()) * (0.6 + 1.2 * c.weather);
        // Few, large puffs: enough to read as cotton, cheap to draw.
        let count = (5.0 + 4.0 * next() + 6.0 * c.weather) as usize;
        let puffs = (0..count)
            .map(|_| {
                let a = next() * std::f64::consts::TAU;
                let r = c.size * next().sqrt();
                let edge = r / c.size;
                let size = (0.26 + 0.2 * next()) * c.size * (1.15 - 0.6 * edge);
                let height = size * 0.55 + next() * tower * (1.0 - edge).powf(1.5);
                Puff {
                    offset: DVec3::new(r * a.cos(), r * a.sin(), height),
                    radius: DVec3::new(size * 1.15, size * 1.15, size * 0.85),
                }
            })
            .collect();
        out.push(Cloud {
            chunk: world.plate_at(c.dir),
            base,
            east,
            north,
            up: c.dir,
            size: c.size,
            puffs,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clouds_cover_about_a_third_and_clump() {
        let world = GlobeWorld::new(92_000.0, 32, 11);
        let clouds = clouds(&world, 11, 1.0 / 3.0);
        assert!(clouds.len() > 300, "{} clouds", clouds.len());
        let puffs: usize = clouds.iter().map(|c| c.puffs.len()).sum();
        assert!(puffs < 30_000, "{puffs} puffs: too many to draw");
        let radius = world.radius;
        let area = 4.0 * std::f64::consts::PI * radius * radius;
        let covered: f64 = clouds
            .iter()
            .map(|c| std::f64::consts::PI * c.size * c.size * 0.55)
            .sum();
        assert!(
            (covered / area - 1.0 / 3.0).abs() < 0.08,
            "cover {}",
            covered / area
        );
        // Bases are above the ground and below the high ranges' crests.
        for c in &clouds {
            let h = c.base.length() - radius;
            assert!((2_000.0..8_000.0).contains(&h), "base at {h} m");
        }
        // Clumped: many plains with many clouds, some with few.
        let mut per: Vec<usize> = vec![0; 32];
        for c in &clouds {
            per[c.chunk] += 1;
        }
        per.sort_unstable();
        assert!(
            per[28] > 3 * per[3].max(1),
            "clouds gather in systems: {per:?}"
        );
    }
}
