//! Task 4.3: settlements land in sensible places in each people's home region, divided into
//! faction territories with one seat each, the same every time.

use std::path::PathBuf;

use ai_gateway::{ContentDirector, ReplayProvider};
use sim_core::StarSystem;
use sim_planet::Terrain;
use sim_planet::map::direction;
use sim_society::{Settlement, SettlementKind, Society, generate, generate_settlements};

fn content() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../../content")
}

fn meridian() -> (StarSystem, Society, Vec<Settlement>) {
    let system = StarSystem::from_path(content().join("data/star_systems/halcyon.json")).unwrap();
    let body = system
        .bodies
        .iter()
        .position(|b| b.id == "meridian")
        .unwrap();
    let director = ContentDirector::new(Box::new(
        ReplayProvider::load_dir(content().join("lore")).unwrap(),
    ));
    let mut society = generate(&system, body, &director);
    let settlements = generate_settlements(&system, &mut society, &director);
    (system, society, settlements)
}

#[test]
fn settlements_land_on_good_ground_in_their_home_regions() {
    let (system, society, settlements) = meridian();
    assert!(society.fallbacks.is_empty(), "{:?}", society.fallbacks);
    let terrain = Terrain::of(&system.bodies[society.body]).unwrap();
    for (ci, c) in society.cultures.iter().enumerate() {
        let mine: Vec<&Settlement> = settlements.iter().filter(|s| s.culture == ci).collect();
        assert_eq!(mine.len() as u32, c.people.settlements, "{}", c.profile.id);
        let h = c.people.home;
        let centre = direction(h.latitude_deg.to_radians(), h.longitude_deg.to_radians());
        for s in &mine {
            assert!(
                s.direction.angle_between(centre) < h.radius_deg.to_radians(),
                "{} in its region",
                s.name
            );
            assert!(
                terrain.height(s.direction, 200.0) > 5.0,
                "{} on dry land",
                s.name
            );
            assert!(
                terrain.slope(s.direction, 200.0, 200.0) < 0.15,
                "{} on flat ground",
                s.name
            );
        }
        // Spread out, not stacked.
        for (i, a) in mine.iter().enumerate() {
            for b in &mine[i + 1..] {
                let km = a.direction.angle_between(b.direction) * terrain.radius / 1000.0;
                assert!(km > 5.0, "{} and {} {km:.1} km apart", a.name, b.name);
            }
        }
    }
}

#[test]
fn each_faction_holds_its_share_around_one_seat() {
    let (_, society, settlements) = meridian();
    for (fi, f) in society.factions.iter().enumerate() {
        let held: Vec<&Settlement> = settlements.iter().filter(|s| s.faction == fi).collect();
        assert_eq!(held.len() as u32, f.settlements, "{}", f.name);
        let seats: Vec<&&Settlement> = held.iter().filter(|s| s.seat).collect();
        assert_eq!(seats.len(), 1, "{} has one seat", f.name);
        assert!(matches!(
            seats[0].kind,
            SettlementKind::Town | SettlementKind::Colony | SettlementKind::Port
        ));
        // A faction's settlements are nearer its own seat than most others' (contiguous).
        let others: Vec<&Settlement> = settlements
            .iter()
            .filter(|s| s.seat && s.faction != fi && s.culture == f.culture)
            .collect();
        let near = held
            .iter()
            .filter(|s| {
                let own = s.direction.angle_between(seats[0].direction);
                others
                    .iter()
                    .all(|o| own <= o.direction.angle_between(s.direction) * 1.3)
            })
            .count();
        assert!(
            near * 10 >= held.len() * 7,
            "{}: {near} of {} near their seat",
            f.name,
            held.len()
        );
    }
}

#[test]
fn populations_names_and_lore_are_complete() {
    let (_, society, settlements) = meridian();
    let mut names: Vec<&str> = settlements.iter().map(|s| s.name.as_str()).collect();
    names.sort();
    names.dedup();
    assert_eq!(names.len(), settlements.len(), "unique names");
    for s in &settlements {
        assert_eq!(
            s.roles.iter().map(|r| r.1).sum::<u32>(),
            s.population,
            "{}",
            s.name
        );
        assert!(
            !s.lore.description.contains('{') && s.lore.description.contains(&s.name),
            "{}",
            s.lore.description
        );
        if s.kind == SettlementKind::Outpost {
            assert!(!s.name.starts_with("Port "), "an outpost named {}", s.name);
        }
    }
    let compact: u32 = settlements
        .iter()
        .filter(|s| society.cultures[s.culture].profile.id == "compact")
        .map(|s| s.population)
        .sum();
    assert!(
        (5000..12000).contains(&compact),
        "the Compact is about nine thousand people: {compact}"
    );
}

#[test]
fn the_same_seed_gives_the_same_settlements() {
    assert_eq!(meridian().2, meridian().2);
}
