//! Equirectangular maps of a body (for tuning terrain and climate, and for showing what is
//! placed on it): longitude −180°…180° left to right, latitude 90°…−90° top to bottom.

use glam::DVec3;

use crate::{Biome, Climate, Terrain};

/// The surface-frame direction at `latitude` and `longitude` (radians; longitude east from the
/// surface frame's +X axis, +Y north).
pub fn direction(latitude: f64, longitude: f64) -> DVec3 {
    DVec3::new(
        latitude.cos() * longitude.cos(),
        latitude.sin(),
        -latitude.cos() * longitude.sin(),
    )
}

/// Latitude and longitude (radians) of a surface-frame direction.
pub fn lat_lon(dir: DVec3) -> (f64, f64) {
    let d = dir.normalize();
    (d.y.clamp(-1.0, 1.0).asin(), (-d.z).atan2(d.x))
}

/// The map pixel (x, y) of a direction on a `width` × `width / 2` map.
pub fn pixel(dir: DVec3, width: usize) -> (f64, f64) {
    let (lat, lon) = lat_lon(dir);
    let height = (width / 2) as f64;
    (
        (lon + std::f64::consts::PI) / std::f64::consts::TAU * width as f64,
        (std::f64::consts::FRAC_PI_2 - lat) / std::f64::consts::PI * height,
    )
}

pub fn biome_color(b: Biome) -> [f64; 3] {
    match b {
        Biome::Ocean => [0.05, 0.2, 0.45],
        Biome::SeaIce => [0.8, 0.88, 0.95],
        Biome::Beach => [0.86, 0.8, 0.6],
        Biome::Desert => [0.85, 0.7, 0.45],
        Biome::Grassland => [0.5, 0.65, 0.3],
        Biome::Forest => [0.2, 0.45, 0.18],
        Biome::Jungle => [0.1, 0.38, 0.12],
        Biome::Taiga => [0.25, 0.38, 0.3],
        Biome::Tundra => [0.55, 0.55, 0.45],
        Biome::Ice => [0.95, 0.97, 1.0],
        Biome::Rock => [0.45, 0.42, 0.4],
        Biome::Regolith => [0.6, 0.58, 0.55],
    }
}

/// A biome map with hill shading, `width` × `width / 2` RGB bytes.
pub fn biome_map(terrain: &Terrain, climate: &Climate, width: usize) -> Vec<u8> {
    let height = width / 2;
    let px = std::f64::consts::TAU * terrain.radius / width as f64;
    let dir = |x: f64, y: f64| {
        let lon = (x / width as f64) * std::f64::consts::TAU - std::f64::consts::PI;
        let lat = std::f64::consts::FRAC_PI_2 - (y / height as f64) * std::f64::consts::PI;
        direction(lat, lon)
    };
    let sun = DVec3::new(-1.0, 1.0, 0.5).normalize();
    let mut bytes = Vec::with_capacity(width * height * 3);
    for y in 0..height {
        for x in 0..width {
            let d = dir(x as f64 + 0.5, y as f64 + 0.5);
            let s = terrain.sample(d, px);
            let t = climate.temperature_at(terrain, d, s.height, 0.0);
            let m = climate.moisture(terrain, d, &s);
            let slope = terrain.slope(d, px, px);
            let biome = climate.biome(terrain, &s, t, m, slope);
            // Hill shading from height differences to the east and north neighbours.
            let he = terrain.height(dir(x as f64 + 1.5, y as f64 + 0.5), px);
            let hn = terrain.height(dir(x as f64 + 0.5, y as f64 - 0.5), px);
            let shade = if !terrain.has_ocean() || s.height > 0.0 {
                let n = DVec3::new(-(he - s.height) / px, -(hn - s.height) / px, 1.0).normalize();
                (0.55 + 0.6 * n.dot(sun)).clamp(0.3, 1.3)
            } else {
                1.0
            };
            for c in biome_color(biome) {
                bytes.push(((c * shade).clamp(0.0, 1.0) * 255.0) as u8);
            }
        }
    }
    bytes
}
