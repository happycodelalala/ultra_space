//! Task 4.2: Meridian's cultures and factions come from the star system data and the content
//! director, the same every time; names read as each culture's; relations make sense.

use std::path::PathBuf;

use ai_gateway::{ContentDirector, ReplayProvider};
use sim_core::StarSystem;
use sim_society::rng::Rng;
use sim_society::{Society, TechTier, generate, names};

fn content() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../../content")
}

fn meridian() -> Society {
    let system = StarSystem::from_path(content().join("data/star_systems/halcyon.json")).unwrap();
    let body = system
        .bodies
        .iter()
        .position(|b| b.id == "meridian")
        .unwrap();
    let director = ContentDirector::new(Box::new(
        ReplayProvider::load_dir(content().join("lore")).unwrap(),
    ));
    generate(&system, body, &director)
}

#[test]
fn meridian_has_both_cultures_and_their_factions() {
    let s = meridian();
    assert!(s.fallbacks.is_empty(), "{:?}", s.fallbacks);
    let tiers: Vec<TechTier> = s.cultures.iter().map(|c| c.profile.tech_tier).collect();
    assert_eq!(tiers, vec![TechTier::Medieval, TechTier::Spacefaring]);
    assert_eq!(s.factions.len(), 5);
    // Strengths decide how much land each holds once settlements are placed.
    for f in &s.factions {
        assert!(
            (0.5..=1.5).contains(&f.strength),
            "{}: {}",
            f.name,
            f.strength
        );
    }
    let mut names: Vec<&str> = s.factions.iter().map(|f| f.name.as_str()).collect();
    names.sort();
    names.dedup();
    assert_eq!(names.len(), 5, "unique faction names");
    for f in &s.factions {
        for text in [&f.lore.motto, &f.lore.description, &f.lore.history] {
            assert!(!text.contains('{'), "placeholders filled: {text}");
        }
        assert!(f.lore.description.contains(&f.name) || f.lore.history.contains(&f.name));
    }
}

#[test]
fn relations_are_symmetric_and_closer_within_a_culture() {
    let s = meridian();
    let n = s.factions.len();
    let (mut within, mut across) = (vec![], vec![]);
    for i in 0..n {
        for j in 0..n {
            assert_eq!(s.relation(i, j), s.relation(j, i));
            assert!((-100..=100).contains(&s.relation(i, j)));
            if i < j {
                if s.factions[i].culture == s.factions[j].culture {
                    within.push(s.relation(i, j));
                } else {
                    across.push(s.relation(i, j));
                }
            }
        }
    }
    let mean = |v: &[i32]| f64::from(v.iter().sum::<i32>()) / v.len() as f64;
    assert!(
        mean(&within) > mean(&across) + 15.0,
        "{within:?} {across:?}"
    );
}

#[test]
fn the_same_seed_gives_the_same_society() {
    assert_eq!(meridian(), meridian());
}

#[test]
fn names_read_as_each_cultures() {
    let s = meridian();
    for c in &s.cultures {
        let n = &c.profile.naming;
        let mut rng = Rng::new(7);
        for _ in 0..50 {
            let person = names::person(n, &mut rng);
            assert_eq!(person.split(' ').count(), 2, "{person}");
            assert!(person.chars().next().unwrap().is_uppercase());
            assert!(
                person.len() <= 2 * (n.syllables[1] as usize * 6) + 1,
                "{person}"
            );
            let place = names::place(n, &mut rng);
            assert!(!place.contains('{'), "{place}");
        }
    }
}
