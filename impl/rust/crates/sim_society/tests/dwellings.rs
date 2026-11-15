//! Task 4b.8: dwellings fill the settled land between settlements, the same wherever the
//! player comes from, each laid out as a farmstead of its people's stage.

use std::path::PathBuf;
use std::sync::Arc;

use ai_gateway::{ContentDirector, ReplayProvider};
use glam::DVec3;
use sim_core::StarSystem;
use sim_planet::Terrain;
use sim_society::{
    DWELLING_IDS, Kits, RegionSociety, SettlementPatterns, SizeClass, dwellings_near,
    generate_layout,
};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../..")
}

fn meridian() -> (RegionSociety, SettlementPatterns, Terrain, Arc<Kits>) {
    let content = root().join("content");
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
    let terrain = Terrain::of(&system.bodies[body]).unwrap();
    let kits = Arc::new(Kits::load_dir(&root().join("assets/export/buildings")).unwrap());
    (rs, patterns, terrain, kits)
}

/// A point `d` m from `dir` toward the east.
fn east_of(dir: DVec3, d: f64, r: f64) -> DVec3 {
    let (east, _) = sim_planet::tangent_frame(dir);
    (dir * r + east * d).normalize()
}

#[test]
fn dwellings_fill_the_settled_land_the_same_from_anywhere() {
    let (rs, patterns, terrain, kits) = meridian();
    let r = terrain.radius;
    // Round the biggest Hollish town.
    let town = rs
        .settlements
        .iter()
        .filter(|s| rs.society.cultures[s.culture].profile.id == "hollin")
        .max_by_key(|s| s.population)
        .unwrap();
    let here = east_of(town.direction, 6_000.0, r);
    let a = dwellings_near(&rs, &patterns, &terrain, here, 4_000.0);
    assert!(a.len() >= 5, "{} dwellings", a.len());
    for (id, d) in &a {
        assert!(id & DWELLING_IDS != 0);
        assert_eq!(d.size, SizeClass::Dwelling);
        assert!(d.direction.angle_between(here) * r <= 4_000.0);
    }
    // Asked from somewhere else, the shared ones are the same.
    let b = dwellings_near(&rs, &patterns, &terrain, east_of(here, 2_000.0, r), 4_000.0);
    let shared: Vec<_> = a
        .iter()
        .filter(|(id, _)| b.iter().any(|(k, _)| k == id))
        .collect();
    assert!(!shared.is_empty());
    for (id, d) in shared {
        let other = &b.iter().find(|(k, _)| k == id).unwrap().1;
        assert_eq!(d.direction, other.direction);
        assert_eq!(d.layout_seed, other.layout_seed);
    }
    // Each is a farmstead: a home at least, on the people's kit.
    let tier = rs.society.cultures[town.culture].profile.tech_tier;
    let kit = &kits.0[&rs.society.cultures[town.culture].profile.building_kit];
    for (_, d) in &a {
        let l = generate_layout(d, tier, kit, &terrain);
        let homes = l
            .buildings
            .iter()
            .filter(|p| kit.buildings[p.building].uses.iter().any(|u| u == "home"))
            .count();
        assert!(homes >= 1, "{}: no home", d.name);
    }
}

#[test]
fn something_built_within_about_a_kilometre_in_settled_land() {
    let (rs, patterns, terrain, _) = meridian();
    let r = terrain.radius;
    // Between the Hollish villages: points halfway between neighbouring villages.
    let villages: Vec<_> = rs
        .settlements
        .iter()
        .filter(|s| {
            rs.society.cultures[s.culture].profile.id == "hollin" && s.size == SizeClass::Village
        })
        .take(40)
        .collect();
    let mut checked = 0;
    let mut empty = 0;
    for v in &villages {
        let Some(w) = villages.iter().filter(|w| w.id != v.id).min_by(|a, b| {
            a.direction
                .angle_between(v.direction)
                .total_cmp(&b.direction.angle_between(v.direction))
        }) else {
            continue;
        };
        if w.direction.angle_between(v.direction) * r > 8_000.0 {
            continue;
        }
        let mid = (v.direction + w.direction).normalize();
        if terrain.height(mid, 50.0) < 3.0 {
            continue;
        }
        checked += 1;
        let near = dwellings_near(&rs, &patterns, &terrain, mid, 1_200.0);
        let settlement = rs
            .settlements
            .iter()
            .any(|s| s.direction.angle_between(mid) * r < 1_200.0);
        if near.is_empty() && !settlement {
            empty += 1;
        }
    }
    assert!(checked >= 10, "{checked} points");
    assert!(
        empty * 5 <= checked,
        "{empty} of {checked} points with nothing built within 1.2 km"
    );
}

#[test]
fn roads_lead_from_settlement_to_settlement() {
    let (rs, _, terrain, _) = meridian();
    let r = terrain.radius;
    let roads = sim_society::roads(&rs, r);
    // Every settlement on some road, roads within a people, none absurdly long.
    let mut on = vec![false; rs.settlements.len()];
    for &(a, b) in &roads {
        assert!(a < b);
        on[a] = true;
        on[b] = true;
        assert_eq!(rs.settlements[a].culture, rs.settlements[b].culture);
        assert!(
            rs.settlements[a]
                .direction
                .angle_between(rs.settlements[b].direction)
                * r
                < 40_000.0
        );
    }
    let lonely = on.iter().filter(|&&o| !o).count();
    assert!(
        lonely * 50 <= rs.settlements.len(),
        "{lonely} of {} settlements on no road",
        rs.settlements.len()
    );
    println!(
        "{} roads for {} settlements",
        roads.len(),
        rs.settlements.len()
    );
}

#[test]
fn fields_keep_clear_of_each_other() {
    let (rs, patterns, terrain, kits) = meridian();
    let town = rs
        .settlements
        .iter()
        .filter(|s| rs.society.cultures[s.culture].profile.id == "hollin")
        .max_by_key(|s| s.population)
        .unwrap();
    let here = east_of(town.direction, 6_000.0, terrain.radius);
    let tier = rs.society.cultures[town.culture].profile.tech_tier;
    let kit = &kits.0[&rs.society.cultures[town.culture].profile.building_kit];
    let mut checked = 0;
    for (_, d) in dwellings_near(&rs, &patterns, &terrain, here, 4_000.0)
        .iter()
        .chain(std::iter::once(&(0, town.clone())))
    {
        let l = generate_layout(d, tier, kit, &terrain);
        for (i, a) in l.fields.iter().enumerate() {
            for b in &l.fields[i + 1..] {
                // Centres at least the narrower width apart (rings don't overlap).
                let gap = glam::DVec2::new(a.x - b.x, a.z - b.z).length();
                assert!(
                    gap > 2.0 * a.half[0].min(b.half[0]) - 0.5,
                    "{}: fields {gap:.1} m apart",
                    d.name
                );
            }
            checked += 1;
        }
    }
    assert!(checked > 20, "{checked} fields");
}
