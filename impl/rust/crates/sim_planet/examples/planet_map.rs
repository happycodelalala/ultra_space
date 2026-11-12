//! Renders a body's biome map with hill shading as an equirectangular PPM image.
//!
//! `cargo run --release -p sim_planet --example planet_map -- <body id> <out.ppm> [width]`
//!
//! A development aid for tuning terrain and climate parameters.

use std::io::Write;
use std::path::PathBuf;

use glam::DVec3;
use sim_core::StarSystem;
use sim_planet::{Biome, Climate, Terrain};

fn color(b: Biome) -> [f64; 3] {
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

fn main() {
    let mut args = std::env::args().skip(1);
    let id = args.next().unwrap_or_else(|| "meridian".into());
    let out = PathBuf::from(args.next().unwrap_or_else(|| format!("{id}.ppm")));
    let width: usize = args.next().map_or(720, |w| w.parse().expect("width"));
    let height = width / 2;

    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../../content/data/star_systems/halcyon.json");
    let system = StarSystem::from_path(path).expect("halcyon");
    let body = system.body(&id).expect("body id");
    let terrain = Terrain::of(body).expect("body has terrain");
    let climate = Climate::of(body).expect("body has a climate");
    let pixel = std::f64::consts::TAU * terrain.radius / width as f64;

    let dir = |x: f64, y: f64| {
        let lon = (x / width as f64) * std::f64::consts::TAU - std::f64::consts::PI;
        let lat = std::f64::consts::FRAC_PI_2 - (y / height as f64) * std::f64::consts::PI;
        DVec3::new(lat.cos() * lon.cos(), lat.sin(), -lat.cos() * lon.sin())
    };
    let sun = DVec3::new(-1.0, 1.0, 0.5).normalize();
    let mut bytes = Vec::with_capacity(width * height * 3);
    for y in 0..height {
        for x in 0..width {
            let d = dir(x as f64 + 0.5, y as f64 + 0.5);
            let s = terrain.sample(d, pixel);
            let t = climate.temperature_at(&terrain, d, s.height, 0.0);
            let m = climate.moisture(&terrain, d, &s);
            let slope = terrain.slope(d, pixel, pixel);
            let biome = climate.biome(&terrain, &s, t, m, slope);
            // Hill shading from height differences to the east and north neighbours.
            let he = terrain.height(dir(x as f64 + 1.5, y as f64 + 0.5), pixel);
            let hn = terrain.height(dir(x as f64 + 0.5, y as f64 - 0.5), pixel);
            let shade = if !terrain.has_ocean() || s.height > 0.0 {
                let n =
                    DVec3::new(-(he - s.height) / pixel, -(hn - s.height) / pixel, 1.0).normalize();
                (0.55 + 0.6 * n.dot(sun)).clamp(0.3, 1.3)
            } else {
                1.0
            };
            for c in color(biome) {
                bytes.push(((c * shade).clamp(0.0, 1.0) * 255.0) as u8);
            }
        }
    }
    let mut file = std::fs::File::create(&out).expect("create output");
    write!(file, "P6\n{width} {height}\n255\n").unwrap();
    file.write_all(&bytes).unwrap();
    eprintln!("wrote {} ({width}x{height})", out.display());
}
