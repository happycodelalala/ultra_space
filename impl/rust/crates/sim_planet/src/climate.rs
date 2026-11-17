//! Climate and biomes (planet-surface.md §2, universe design §5; task 2.1).
//!
//! Rules compute temperature and moisture from the body's hand-written (later LLM-proposed)
//! climate parameters; biomes follow from temperature, moisture, height and slope.

use glam::DVec3;
use sim_core::{Body, ClimateData, StarSystem};

use crate::terrain::{Sample, Terrain};

/// Temperature drop with altitude in an atmosphere (K per metre).
pub const LAPSE_RATE: f64 = 6.5e-3;
/// Regional temperature variation (°C, about ± this much), so climate zones have irregular
/// edges instead of following latitude lines exactly.
const REGIONAL_VARIATION: f64 = 16.0;
/// The same within a region of a body of plains (°C).
const REGION_VARIATION: f64 = 4.0;
/// How far the warm belt follows the subsolar point (fraction of its latitude).
const SEASONAL_SHIFT: f64 = 0.5;
/// Height of the beach band above sea level (m).
const BEACH_HEIGHT: f64 = 25.0;
/// Slopes steeper than this are bare rock (radians, ~40°).
const ROCK_SLOPE: f64 = 0.7;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Biome {
    Ocean,
    SeaIce,
    Beach,
    Desert,
    Grassland,
    Forest,
    Jungle,
    Taiga,
    Tundra,
    Ice,
    Rock,
    /// Bare dust and rubble of airless bodies.
    Regolith,
}

/// A body's climate: its parameters plus what the rules need from the body.
#[derive(Clone, Debug)]
pub struct Climate {
    pub params: ClimateData,
    pub has_atmosphere: bool,
    /// Blighted land (a wasteland's hazard zones, living-planet.md §8b): centre (unit
    /// direction), angular radius and strength (0–1); moisture there falls toward nothing, so
    /// the land dries and dies. Empty for most bodies.
    pub blight: std::sync::Arc<Vec<(DVec3, f64, f64)>>,
}

impl Climate {
    pub fn of(body: &Body) -> Option<Self> {
        Some(Self {
            params: body.climate.clone()?,
            has_atmosphere: body.atmosphere.is_some(),
            blight: Default::default(),
        })
    }

    /// This climate with blighted land (see [`Climate::blight`]).
    pub fn with_blight(mut self, zones: Vec<(DVec3, f64, f64)>) -> Self {
        self.blight = std::sync::Arc::new(zones);
        self
    }

    /// How blighted the land is at unit direction `dir` (0–1).
    pub fn blight_at(&self, dir: DVec3) -> f64 {
        self.blight
            .iter()
            .map(|&(c, radius, strength)| {
                let t = (dir.angle_between(c) / radius).min(1.0);
                strength * (1.0 - t * t * (3.0 - 2.0 * t))
            })
            .fold(0.0, f64::max)
    }

    /// Temperature (°C) at `dir` (surface frame) and `height`, when the star stands over
    /// latitude `subsolar_latitude` (radians; the season), including regional variation.
    pub fn temperature_at(
        &self,
        terrain: &Terrain,
        dir: DVec3,
        height: f64,
        subsolar_latitude: f64,
    ) -> f64 {
        if let Some((sea_level, _)) = terrain.region_climate(dir) {
            // A body of plains: each region has its own climate; `height` is the elevation.
            let air = if self.has_atmosphere {
                LAPSE_RATE * height.max(0.0)
            } else {
                0.0
            };
            return sea_level - air + REGION_VARIATION * terrain.climate_noise(dir, 1);
        }
        self.temperature(dir, height, subsolar_latitude)
            + REGIONAL_VARIATION * terrain.climate_noise(dir, 1)
    }

    /// Zonal temperature (°C): latitude, season and altitude only, without regional variation.
    pub fn temperature(&self, dir: DVec3, height: f64, subsolar_latitude: f64) -> f64 {
        let p = &self.params;
        let latitude = latitude(dir);
        let effective = (latitude - SEASONAL_SHIFT * subsolar_latitude)
            .clamp(-std::f64::consts::FRAC_PI_2, std::f64::consts::FRAC_PI_2);
        let sea_level = p.pole_temperature_c
            + (p.equator_temperature_c - p.pole_temperature_c) * effective.cos();
        if self.has_atmosphere {
            sea_level - LAPSE_RATE * height.max(0.0)
        } else {
            sea_level
        }
    }

    /// Moisture, 0–1: rain belts at the equator and around 60° latitude, dry belts around 30°
    /// and at the poles, wetter near coasts, plus regional variation.
    pub fn moisture(&self, terrain: &Terrain, dir: DVec3, sample: &Sample) -> f64 {
        if let Some((_, wet)) = terrain.region_climate(dir) {
            let coast = 1.0 - smoothstep(0.0, 0.05, sample.land);
            let variation = terrain.climate_noise(dir, 0);
            return (wet + 0.15 * coast + 0.2 * variation).clamp(0.0, 1.0);
        }
        let wetness = self.params.moisture;
        if wetness <= 0.0 {
            return 0.0;
        }
        let band = 0.5 + 0.5 * (6.0 * latitude(dir)).cos();
        let coast = if terrain.has_ocean() {
            1.0 - smoothstep(0.0, 0.1, sample.land)
        } else {
            0.0
        };
        // Fractal sums span about ±0.5; stretch to [0, 1].
        let variation = (0.5 + terrain.climate_noise(dir, 0)).clamp(0.0, 1.0);
        let wet = (wetness * (0.5 * band + 0.4 * coast + 0.7 * variation)).clamp(0.0, 1.0);
        if self.blight.is_empty() {
            wet
        } else {
            wet * (1.0 - self.blight_at(dir))
        }
    }

    /// Biome from the climate fields, height and slope (radians).
    pub fn biome(
        &self,
        terrain: &Terrain,
        sample: &Sample,
        temperature: f64,
        moisture: f64,
        slope: f64,
    ) -> Biome {
        if terrain.has_ocean() && sample.elevation < 0.0 {
            return if temperature < -2.0 {
                Biome::SeaIce
            } else {
                Biome::Ocean
            };
        }
        if !self.has_atmosphere {
            return if temperature < -150.0 {
                Biome::Ice
            } else {
                Biome::Regolith
            };
        }
        if slope > ROCK_SLOPE {
            return Biome::Rock;
        }
        if temperature < -50.0 {
            return Biome::Ice;
        }
        if moisture < 0.2 {
            return Biome::Desert;
        }
        if temperature < -10.0 {
            return Biome::Ice;
        }
        if temperature < 0.0 {
            return Biome::Tundra;
        }
        if terrain.has_ocean() && sample.elevation < BEACH_HEIGHT && sample.land < 0.03 {
            return Biome::Beach;
        }
        if temperature < 8.0 {
            return if moisture > 0.4 {
                Biome::Taiga
            } else {
                Biome::Tundra
            };
        }
        if temperature > 22.0 && moisture > 0.6 {
            return Biome::Jungle;
        }
        if moisture > 0.45 {
            return Biome::Forest;
        }
        Biome::Grassland
    }
}

/// Latitude (radians) of a surface-frame direction; +Y is north.
pub fn latitude(dir: DVec3) -> f64 {
    dir.normalize().y.clamp(-1.0, 1.0).asin()
}

/// Latitude (radians) over which the star stands on body `index` at universe time `t`: the
/// season. Ranges between ± the axial tilt over a year.
pub fn subsolar_latitude(system: &StarSystem, index: usize, t: f64) -> f64 {
    let body = &system.bodies[index];
    let star = system.frames.transform_point(
        body.surface_frame,
        system.star().inertial_frame,
        t,
        DVec3::ZERO,
    );
    latitude(star)
}

fn smoothstep(e0: f64, e1: f64, x: f64) -> f64 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::PathBuf;

    use super::*;
    use crate::terrain::fibonacci_directions;

    fn halcyon() -> StarSystem {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../content/data/star_systems/halcyon.json");
        StarSystem::from_path(path).unwrap()
    }

    fn biome_counts(id: &str) -> HashMap<Biome, usize> {
        let sys = halcyon();
        let body = sys.body(id).unwrap();
        let (terrain, climate) = (Terrain::of(body).unwrap(), Climate::of(body).unwrap());
        let mut counts = HashMap::new();
        for d in fibonacci_directions(6_000) {
            let s = terrain.sample(d, 500.0);
            let t = climate.temperature_at(&terrain, d, s.elevation, 0.0);
            let m = climate.moisture(&terrain, d, &s);
            let slope = terrain.slope(d, 500.0, 500.0);
            *counts
                .entry(climate.biome(&terrain, &s, t, m, slope))
                .or_insert(0) += 1;
        }
        counts
    }

    #[test]
    fn equator_is_warmer_than_poles_and_mountains_are_colder() {
        let sys = halcyon();
        let c = Climate::of(sys.body("meridian").unwrap()).unwrap();
        let equator = c.temperature(DVec3::X, 0.0, 0.0);
        let pole = c.temperature(DVec3::Y, 0.0, 0.0);
        assert!((equator - 27.0).abs() < 1e-9 && (pole + 25.0).abs() < 1e-9);
        let peak = c.temperature(DVec3::X, 4_000.0, 0.0);
        assert!((equator - peak - 26.0).abs() < 1e-9, "6.5 K per km");
    }

    #[test]
    fn seasons_warm_the_hemisphere_facing_the_star() {
        let sys = halcyon();
        let c = Climate::of(sys.body("hoarfell").unwrap()).unwrap();
        let north = DVec3::new(1.0, 1.0, 0.0).normalize();
        let summer = c.temperature(north, 0.0, 0.5);
        let winter = c.temperature(north, 0.0, -0.5);
        assert!(
            summer > winter + 10.0,
            "summer {summer:.1}, winter {winter:.1}"
        );
    }

    #[test]
    fn subsolar_latitude_stays_within_the_axial_tilt() {
        let sys = halcyon();
        let m = sys.bodies.iter().position(|b| b.id == "meridian").unwrap();
        let tilt = 23f64.to_radians();
        let year = sys.bodies[m].orbital_period().unwrap();
        let mut max: f64 = 0.0;
        for k in 0..40 {
            let lat = subsolar_latitude(&sys, m, year * f64::from(k) / 40.0);
            assert!(lat.abs() <= tilt + 1e-6);
            max = max.max(lat.abs());
        }
        assert!(max > 0.9 * tilt, "seasons reach the tropics: {max}");
    }

    #[test]
    fn meridian_has_varied_biomes() {
        let counts = biome_counts("meridian");
        let total: usize = counts.values().sum();
        let share = |b: Biome| *counts.get(&b).unwrap_or(&0) as f64 / total as f64;
        assert!(
            share(Biome::Ocean) + share(Biome::SeaIce) > 0.5,
            "{counts:?}"
        );
        for b in [Biome::Forest, Biome::Grassland, Biome::Desert, Biome::Ice] {
            assert!(share(b) > 0.005, "{b:?} missing: {counts:?}");
        }
    }

    #[test]
    fn airless_and_icy_bodies() {
        let lantern = biome_counts("lantern");
        assert_eq!(
            lantern.keys().copied().collect::<Vec<_>>(),
            vec![Biome::Regolith]
        );
        let rime = biome_counts("rime");
        assert_eq!(rime.keys().copied().collect::<Vec<_>>(), vec![Biome::Ice]);
    }

    #[test]
    fn hoarfell_is_a_cold_desert_with_ice_caps() {
        let counts = biome_counts("hoarfell");
        let total: usize = counts.values().sum();
        let share = |b: Biome| *counts.get(&b).unwrap_or(&0) as f64 / total as f64;
        assert!(share(Biome::Desert) > 0.3, "{counts:?}");
        assert!(share(Biome::Ice) > 0.05, "{counts:?}");
        assert_eq!(share(Biome::Forest), 0.0);
    }
}
