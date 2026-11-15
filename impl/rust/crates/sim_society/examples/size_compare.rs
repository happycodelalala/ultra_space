//! Planet size with settlements (task 4.10): generates Meridian's societies at several radii
//! (surface gravity kept, as `ULTRA_SPACE_BODY_RADIUS` does) and prints what changes:
//! `cargo run --release -p sim_society --example size_compare -- [km ...]` (default 300 600 1000).

use std::path::PathBuf;

use ai_gateway::{ContentDirector, ReplayProvider};
use sim_core::StarSystem;
use sim_society::RegionSociety;

fn main() {
    let radii: Vec<f64> = {
        let v: Vec<f64> = std::env::args()
            .skip(1)
            .filter_map(|a| a.parse().ok())
            .collect();
        if v.is_empty() {
            vec![300.0, 600.0, 1000.0]
        } else {
            v
        }
    };
    let content = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../../content");
    let text = std::fs::read_to_string(content.join("data/star_systems/halcyon.json")).unwrap();
    let director = ContentDirector::new(Box::new(
        ReplayProvider::load_dir(content.join("lore")).unwrap(),
    ));
    println!(
        "{:>6} {:>10} {:>8} {:>8} {:>10} {:>10} {:>10} {:>9} {:>9}",
        "km",
        "circ km",
        "horizon",
        "orbit",
        "Hollin r",
        "Hollin nn",
        "Compact nn",
        "seats km",
        "at 300m/s"
    );
    for km in radii {
        let mut v: serde_json::Value = serde_json::from_str(&text).unwrap();
        let body = v["bodies"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|b| b["id"] == "meridian")
            .unwrap();
        body["radius_km"] = serde_json::json!(km);
        let system = StarSystem::from_json_str(&v.to_string()).unwrap();
        let i = system
            .bodies
            .iter()
            .position(|b| b.id == "meridian")
            .unwrap();
        let b = &system.bodies[i];
        let r = b.radius;
        let rs = RegionSociety::generate(
            &system,
            i,
            &director,
            &sim_society::SettlementPatterns::load_dir(
                &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../../../../content/data/settlement_patterns"),
            )
            .unwrap(),
        );
        // Mean distance to the nearest settlement of the same culture.
        let nn = |culture: usize| {
            let own: Vec<_> = rs
                .settlements
                .iter()
                .filter(|s| s.culture == culture)
                .collect();
            let sum: f64 = own
                .iter()
                .map(|a| {
                    own.iter()
                        .filter(|b| b.id != a.id)
                        .map(|b| a.direction.angle_between(b.direction) * r)
                        .fold(f64::MAX, f64::min)
                })
                .sum();
            sum / own.len().max(1) as f64 / 1000.0
        };
        let seat = |culture: usize| {
            rs.settlements
                .iter()
                .filter(|s| s.culture == culture)
                .max_by_key(|s| s.population)
                .map(|s| s.direction)
                .unwrap()
        };
        let seats = seat(0).angle_between(seat(1)) * r / 1000.0;
        let home = rs.society.cultures[0].people.home.radius_deg.to_radians() * r / 1000.0;
        let horizon = (2.0 * r * 1.7).sqrt() / 1000.0;
        let orbit = (b.mu / (r + 70_000.0)).sqrt() / 1000.0;
        println!(
            "{km:>6.0} {:>10.0} {:>7.1}k {:>6.2}k/s {:>9.0}k {:>9.1}k {:>9.1}k {:>9.0} {:>8.0}s",
            std::f64::consts::TAU * r / 1000.0,
            horizon,
            orbit,
            home,
            nn(0),
            nn(1),
            seats,
            seats * 1000.0 / 300.0
        );
    }
    println!(
        "\nhorizon: standing (1.7 m eyes); orbit: circular speed 70 km up; Hollin r: home region \
         radius; nn: mean distance to the nearest settlement of the same people; seats: the two \
         peoples' biggest settlements apart, and the flight between them at 300 m/s."
    );
}
