//! Task 4.3: settlements land in sensible places in each people's home region, divided into
//! faction territories with one seat each, the same every time.

use std::path::PathBuf;

use ai_gateway::{ContentDirector, ReplayProvider};
use sim_core::StarSystem;
use sim_planet::Terrain;
use sim_planet::map::direction;
use sim_society::{Settlement, SettlementRole, SizeClass, Society, generate, generate_settlements};

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
    let settlements = generate_settlements(
        &system,
        &mut society,
        &director,
        &sim_society::SettlementPatterns::load_dir(
            &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../../../content/data/settlement_patterns"),
        )
        .unwrap(),
    );
    (system, society, settlements)
}

#[test]
fn settlements_land_on_good_ground_in_their_home_regions() {
    let (system, society, settlements) = meridian();
    assert!(society.fallbacks.is_empty(), "{:?}", society.fallbacks);
    let terrain = Terrain::of(&system.bodies[society.body]).unwrap();
    for (ci, c) in society.cultures.iter().enumerate() {
        let mine: Vec<&Settlement> = settlements.iter().filter(|s| s.culture == ci).collect();
        // Counts follow from the pattern and the land: the medieval realm in the thousands,
        // the young colony and the wasteland a score or so.
        let expected = match c.profile.id.as_str() {
            "hollin" => 1_000..=2_500,
            "tayu" => 150..=600,
            "velani" => 300..=1_200,
            "corvantine" => 700..=2_500,
            "aldmark" => 600..=2_000,
            _ => 5..=40,
        };
        assert!(
            expected.contains(&mine.len()),
            "{}: {}",
            c.profile.id,
            mine.len()
        );
        let held: u32 = society
            .factions
            .iter()
            .filter(|f| f.culture == ci)
            .map(|f| f.settlements)
            .sum();
        assert_eq!(held as usize, mine.len(), "{}", c.profile.id);
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
        // Spread out, not stacked: no two closer than 0.8 of the smallest class's spacing
        // (3.6 km for steadings, 40 km for outposts).
        let least = match c.profile.id.as_str() {
            "hollin" => 3.6,
            "tayu" => 5.0,
            "velani" | "aldmark" => 4.0,
            "corvantine" => 3.0,
            "kesh" => 25.0,
            _ => 40.0,
        };
        for (i, a) in mine.iter().enumerate() {
            for b in &mine[i + 1..] {
                let km = a.direction.angle_between(b.direction) * terrain.radius / 1000.0;
                assert!(
                    km > 0.8 * least - 0.3,
                    "{} and {} {km:.1} km apart",
                    a.name,
                    b.name
                );
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
        assert!(seats[0].size >= SizeClass::Town, "{}", seats[0].name);
        assert!(seats[0].has(SettlementRole::Seat), "{}", seats[0].name);
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
        if s.has(SettlementRole::Outpost) {
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

#[test]
fn settlements_have_a_size_roles_and_their_people_s_word() {
    let (_, society, settlements) = meridian();
    for s in &settlements {
        let culture = &society.cultures[s.culture].profile.id;
        let expect = match (culture.as_str(), s.size, s.lore_kind()) {
            ("hollin", SizeClass::City, _) => "city",
            ("hollin", SizeClass::Town, _) => "hold",
            ("hollin", SizeClass::Village, _) => "village",
            ("hollin", SizeClass::Hamlet, _) => "steading",
            ("compact", _, "capital") => "landing",
            ("compact", _, "port") => "port",
            ("compact", _, "outpost") => "outpost",
            ("tayu", SizeClass::Town, _) => "chief's hill",
            ("tayu", SizeClass::Hamlet, _) => "camp",
            ("velani", SizeClass::City, _) => "polis",
            ("velani", SizeClass::Hamlet, _) => "farmstead",
            ("corvantine", _, "capital") => "crown city",
            ("corvantine", _, "town") => "villa",
            ("corvantine", _, "port") => "port",
            ("aldmark", _, "capital") => "capital",
            ("aldmark", SizeClass::Metropolis, _) => "provincial capital",
            ("kesh", _, "capital") => "first enclave",
            ("kesh", _, "outpost") => "scavenger camp",
            ("kesh", _, "enclave") => "enclave",
            _ => s.size.name(),
        };
        assert_eq!(s.word, expect, "{} ({culture})", s.name);
    }
    // The capital is the first faction's seat, and only one per people.
    for (ci, c) in society.cultures.iter().enumerate() {
        let capitals = settlements
            .iter()
            .filter(|s| s.culture == ci && s.has(SettlementRole::Capital))
            .count();
        // Peoples whose pattern names a capital (the colony, the kingdom, the republic, the
        // wasteland's first enclave); realms of equals and city-states have none.
        let expected = usize::from(matches!(
            c.profile.id.as_str(),
            "compact" | "corvantine" | "aldmark" | "kesh"
        ));
        assert_eq!(capitals, expected, "{}", c.profile.id);
    }
    // The tiers' defaults where a culture has no word.
    use sim_society::{TechTier, default_word};
    assert_eq!(
        default_word(TechTier::StoneAge, SizeClass::Town),
        "hill fort"
    );
    assert_eq!(
        default_word(TechTier::Medieval, SizeClass::Dwelling),
        "farmstead"
    );
    assert_eq!(
        default_word(TechTier::Spacefaring, SizeClass::Metropolis),
        "arcology"
    );
    assert_eq!(default_word(TechTier::Industrial, SizeClass::City), "city");
}

#[test]
fn each_people_has_a_settlement_pattern() {
    use sim_society::{Polity, SettlementPatterns, TechTier};
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../../content/data/settlement_patterns");
    let patterns = SettlementPatterns::load_dir(&dir).unwrap();
    let hollin = patterns
        .for_people(TechTier::Medieval, Polity::Realm)
        .unwrap();
    assert_eq!(hollin.id, "medieval");
    // A charter colony settles as a young colony, whatever its tier.
    for tier in [TechTier::Spacefaring, TechTier::Industrial] {
        let p = patterns.for_people(tier, Polity::CharterColony).unwrap();
        assert_eq!(p.id, "young_colony");
    }
    // Every tier on Meridian has one; a wasteland's is chosen by name.
    for (tier, id) in [
        (TechTier::StoneAge, "stone_age"),
        (TechTier::Ancient, "ancient"),
        (TechTier::EarlyModern, "early_modern"),
        (TechTier::Industrial, "industrial"),
    ] {
        assert_eq!(patterns.for_people(tier, Polity::Realm).unwrap().id, id);
    }
    assert!(
        patterns
            .for_people(TechTier::Interstellar, Polity::Federation)
            .is_none()
    );
    assert!(patterns.by_id("wasteland_enclaves").is_some());
    for p in &patterns.0 {
        // Classes biggest first, each a size below the seat's, spacing growing with size.
        for w in p.classes.windows(2) {
            assert!(
                w[0].size > w[1].size && w[0].spacing_km > w[1].spacing_km,
                "{}",
                p.id
            );
        }
        for c in &p.classes {
            assert!(
                c.size <= p.seat.size && c.people[0] <= c.people[1],
                "{}",
                p.id
            );
        }
    }
}
