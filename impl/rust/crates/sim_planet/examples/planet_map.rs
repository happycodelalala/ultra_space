//! Renders a body's biome map with hill shading as an equirectangular PPM image.
//!
//! `cargo run --release -p sim_planet --example planet_map -- <body id> <out.ppm> [width]`
//!
//! A development aid for tuning terrain and climate parameters.

use std::io::Write;
use std::path::PathBuf;

use sim_core::StarSystem;
use sim_planet::{Climate, Terrain};

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
    let bytes = sim_planet::map::biome_map(&terrain, &climate, width);
    let mut file = std::fs::File::create(&out).expect("create output");
    write!(file, "P6\n{width} {height}\n255\n").unwrap();
    file.write_all(&bytes).unwrap();
    eprintln!("wrote {} ({width}x{height})", out.display());
}
