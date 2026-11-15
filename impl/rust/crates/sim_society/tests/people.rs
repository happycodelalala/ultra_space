//! Task 4.8: people promoted in a settlement live out their day: indoors at night, at work by
//! day, at the square in the evening, walking the streets between.

use std::path::PathBuf;
use std::sync::Arc;

use ai_gateway::{ContentDirector, ReplayProvider};
use glam::DVec2;
use sim_core::StarSystem;
use sim_planet::Terrain;
use sim_society::{Activity, Crowd, Kits, Layout, RegionLayouts, RegionSociety};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../..")
}

fn meridian() -> (RegionSociety, RegionLayouts) {
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
    let society = RegionSociety::generate(
        &system,
        body,
        &director,
        &sim_society::SettlementPatterns::load_dir(
            &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../../../content/data/settlement_patterns"),
        )
        .unwrap(),
    );
    let kits = Arc::new(Kits::load_dir(&root().join("assets/export/buildings")).unwrap());
    let terrain = Terrain::of(&system.bodies[body]).unwrap();
    let layouts = RegionLayouts::generate(&society, kits, &terrain);
    (society, layouts)
}

/// Runs a crowd from `from` to `to` (fractions of a day) over `seconds` of real time, in
/// steps of 1/30 s, and returns the activities seen at the end.
fn run(crowd: &mut Crowd, from: f64, to: f64, seconds: f64) {
    let steps = (seconds * 30.0) as usize;
    for k in 0..steps {
        let day = from + (to - from) * k as f64 / steps as f64;
        crowd.step(1.0 / 30.0, day, None);
    }
}

fn count(crowd: &Crowd, a: Activity) -> usize {
    crowd.people.iter().filter(|p| p.activity == a).count()
}

#[test]
fn people_live_out_the_day_in_a_town_and_the_colony() {
    let (society, layouts) = meridian();
    for (culture, kind) in [("hollin", "town"), ("compact", "capital")] {
        let i = (0..society.settlements.len())
            .filter(|&i| {
                let s = &society.settlements[i];
                s.lore_kind() == kind && society.society.cultures[s.culture].profile.id == culture
            })
            .max_by_key(|&i| society.settlements[i].population)
            .unwrap();
        let s = &society.settlements[i];
        let layout = &layouts.layouts[&i];
        let kit = &layouts.kits.0[&layout.kit];
        let naming = &society.society.cultures[s.culture].profile.naming;
        let mut crowd = Crowd::promote(s, layout, kit, naming, 64);
        assert_eq!(crowd.people.len(), 64);
        // Names and roles in the culture's shares (the biggest role is there).
        let biggest = s.roles.iter().max_by_key(|r| r.1).unwrap();
        assert!(crowd.people.iter().any(|p| p.role == biggest.0));
        assert!(crowd.people.iter().all(|p| p.name.contains(' ')));
        // Same people every time.
        let again = Crowd::promote(s, layout, kit, naming, 64);
        assert_eq!(crowd.people, again.people);

        // Promoted in the evening: already at the square.
        let mut evening = again.clone();
        evening.settle(0.84);
        let there = count(&evening, Activity::Idle) + count(&evening, Activity::Talking);
        assert!(
            there >= 50,
            "{}: {there} at the square on promotion at 20:00",
            s.name
        );
        // Night: everyone home and indoors.
        run(&mut crowd, 0.05, 0.08, 400.0);
        assert_eq!(count(&crowd, Activity::Inside), 64, "{}: night", s.name);
        // Morning: off to work; by mid-morning most are there (outdoor workers visible).
        run(&mut crowd, 0.30, 0.40, 600.0);
        let out = count(&crowd, Activity::Working);
        let walking = count(&crowd, Activity::Walking);
        assert!(out >= 10, "{}: {out} working outdoors at 9:00", s.name);
        assert!(walking <= 16, "{}: {walking} still walking", s.name);
        // Evening: at the square.
        run(&mut crowd, 0.80, 0.85, 600.0);
        let gathered = count(&crowd, Activity::Idle) + count(&crowd, Activity::Talking);
        assert!(
            gathered >= 40,
            "{}: {gathered} at the square at 20:00",
            s.name
        );
        // Walkers keep to the streets and doors, out of the buildings.
        let rects: Vec<_> = layout
            .buildings
            .iter()
            .map(|p| Layout::footprint(kit, p))
            .collect();
        for p in &crowd.people {
            for (b, r) in rects.iter().enumerate() {
                let uses = &kit.buildings[layout.buildings[b].building].uses;
                if uses
                    .iter()
                    .any(|u| matches!(u.as_str(), "tree" | "prop" | "square" | "market" | "pad"))
                {
                    continue;
                }
                let d = p.position - DVec2::new(r.x, r.z);
                let (sn, c) = r.yaw.sin_cos();
                let local = DVec2::new(c * d.x - sn * d.y, sn * d.x + c * d.y);
                let inside = local.x.abs() < r.half[0] - 1.0 && local.y.abs() < r.half[1] - 1.0;
                assert!(!inside, "{}: {} stands inside building {b}", s.name, p.name);
            }
        }
    }
}
