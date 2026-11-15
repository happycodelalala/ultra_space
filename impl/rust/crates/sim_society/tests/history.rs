//! Task 4b.4: Meridian's history leaves the Kesh's land a wasteland: ruins of their old
//! cities, hazard zones on them, and survivors in enclaves away from the worst.

use std::path::PathBuf;

use ai_gateway::{ContentDirector, ReplayProvider};
use sim_core::StarSystem;
use sim_society::{HazardKind, RegionSociety, SettlementPatterns, SizeClass, hazard_at};

fn meridian() -> (StarSystem, RegionSociety) {
    let content = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../../content");
    let system = StarSystem::from_path(content.join("data/star_systems/halcyon.json")).unwrap();
    let body = system
        .bodies
        .iter()
        .position(|b| b.id == "meridian")
        .unwrap();
    let director = ContentDirector::new(Box::new(
        ReplayProvider::load_dir(content.join("lore")).unwrap(),
    ));
    let patterns = SettlementPatterns::load_dir(&content.join("data/settlement_patterns")).unwrap();
    let rs = RegionSociety::generate(&system, body, &director, &patterns);
    (system, rs)
}

#[test]
fn the_kesh_war_leaves_ruins_hazards_and_enclaves_away_from_them() {
    let (system, rs) = meridian();
    assert!(
        rs.society.fallbacks.is_empty(),
        "{:?}",
        rs.society.fallbacks
    );
    // The history, oldest first, from the Heron's children to the Landing.
    let titles: Vec<&str> = rs.history.events.iter().map(|e| e.title.as_str()).collect();
    assert_eq!(titles.first(), Some(&"The Heron's children"));
    assert_eq!(titles.last(), Some(&"The Landing"));
    // One condition: radiation over the Kesh's land.
    assert_eq!(rs.conditions.len(), 1);
    let c = &rs.conditions[0];
    assert_eq!(c.hazard, HazardKind::Radiation);
    let kesh = rs
        .society
        .cultures
        .iter()
        .position(|c| c.profile.id == "kesh")
        .unwrap();
    assert_eq!(c.culture, kesh);
    let radius = system.bodies[rs.society.body].radius;
    // Ruins of the old cities and towns, each in the waste.
    assert!(rs.ruins.len() >= 50, "{} ruins", rs.ruins.len());
    assert!(rs.ruins.iter().any(|r| r.size >= SizeClass::Metropolis));
    for r in &rs.ruins {
        assert_eq!(r.culture, kesh);
        assert!(
            hazard_at(&rs.conditions, r.direction, radius).0 > 0.0,
            "{}",
            r.name
        );
    }
    // The old cities are the hottest places; the survivors live away from them.
    let hottest = rs
        .ruins
        .iter()
        .filter(|r| r.size >= SizeClass::City)
        .map(|r| hazard_at(&rs.conditions, r.direction, radius).0)
        .fold(0.0, f64::max);
    assert!(hottest > 0.5, "hottest {hottest}");
    for s in rs.settlements.iter().filter(|s| s.culture == kesh) {
        let h = hazard_at(&rs.conditions, s.direction, radius).0;
        assert!(h < 0.35, "{} in hazard {h:.2}", s.name);
    }
    // Elsewhere on Meridian, no hazard.
    let hollin = rs.settlements.iter().find(|s| s.id == "hollin/0").unwrap();
    assert_eq!(hazard_at(&rs.conditions, hollin.direction, radius).0, 0.0);
}
