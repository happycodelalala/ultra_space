//! Prints a body's society: `cargo run -p sim_society --example society [body id]`.

use std::path::PathBuf;

use ai_gateway::{ContentDirector, ReplayProvider};
use sim_core::StarSystem;
use sim_society::rng::Rng;
use sim_society::{generate, names};

fn main() {
    let id = std::env::args().nth(1).unwrap_or_else(|| "meridian".into());
    let content = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../../content");
    let system = StarSystem::from_path(content.join("data/star_systems/halcyon.json")).unwrap();
    let body = system.bodies.iter().position(|b| b.id == id).expect("body");
    let director = ContentDirector::new(Box::new(
        ReplayProvider::load_dir(content.join("lore")).unwrap(),
    ));
    let s = generate(&system, body, &director);
    for (ci, c) in s.cultures.iter().enumerate() {
        let p = &c.profile;
        println!(
            "== {} ({:?}, {:?}), home {:.0}°, {:.0}° r {:.0}°",
            p.name,
            p.tech_tier,
            p.attitude,
            c.people.home.latitude_deg,
            c.people.home.longitude_deg,
            c.people.home.radius_deg
        );
        let mut rng = Rng::derive(s.seed, &p.id);
        let people: Vec<String> = (0..6).map(|_| names::person(&p.naming, &mut rng)).collect();
        let places: Vec<String> = (0..6).map(|_| names::place(&p.naming, &mut rng)).collect();
        println!("   people: {}", people.join(", "));
        println!("   places: {}", places.join(", "));
        for f in s.factions.iter().filter(|f| f.culture == ci) {
            println!(
                "   {} ({} settlements) \"{}\"\n     {}",
                f.name, f.settlements, f.lore.motto, f.lore.description
            );
        }
    }
    println!("relations:");
    for (i, f) in s.factions.iter().enumerate() {
        let row: Vec<String> = (0..s.factions.len())
            .map(|j| format!("{:4}", s.relation(i, j)))
            .collect();
        println!("  {:32} {}", f.name, row.join(""));
    }
    for w in &s.fallbacks {
        println!("fallback: {w}");
    }
}
