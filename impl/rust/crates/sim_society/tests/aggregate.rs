//! Task 4.4: the aggregate simulation keeps settlements alive believably over game years, at
//! any time step, and its state survives a save.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use ai_gateway::{ContentDirector, ReplayProvider};
use sim_core::StarSystem;
use sim_society::aggregate::{capture, restore};
use sim_society::{RegionSociety, TechTier};

fn content() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../../content")
}

fn system() -> StarSystem {
    StarSystem::from_path(content().join("data/star_systems/halcyon.json")).unwrap()
}

fn director() -> ContentDirector {
    ContentDirector::new(Box::new(
        ReplayProvider::load_dir(content().join("lore")).unwrap(),
    ))
}

fn meridian() -> RegionSociety {
    let system = system();
    let body = system
        .bodies
        .iter()
        .position(|b| b.id == "meridian")
        .unwrap();
    RegionSociety::generate(
        &system,
        body,
        &director(),
        &sim_society::SettlementPatterns::load_dir(
            &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../../../content/data/settlement_patterns"),
        )
        .unwrap(),
    )
}

fn per_culture(rs: &RegionSociety, tier: TechTier) -> f64 {
    rs.settlements
        .iter()
        .zip(&rs.states)
        .filter(|(s, _)| rs.society.cultures[s.culture].profile.tech_tier == tier)
        .map(|(_, st)| st.population())
        .sum()
}

#[test]
fn well_fed_peoples_grow_slowly_over_years() {
    let mut rs = meridian();
    let tiers: Vec<TechTier> = rs
        .society
        .cultures
        .iter()
        .map(|c| c.profile.tech_tier)
        .collect();
    let before: Vec<f64> = tiers.iter().map(|&t| per_culture(&rs, t)).collect();
    for _ in 0..(10 * 365) {
        rs.step(1.0);
    }
    let after: Vec<f64> = tiers.iter().map(|&t| per_culture(&rs, t)).collect();
    for (c, (b, a)) in rs.society.cultures.iter().zip(before.iter().zip(&after)) {
        println!("10 years: {} {b:.0} -> {a:.0}", c.profile.name);
    }
    // Every people holds its own (the wasteland's included: rationed, not starving).
    for (grown, start) in after.into_iter().zip(before) {
        let yearly = (grown / start).powf(0.1) - 1.0;
        assert!(
            (0.002..0.03).contains(&yearly),
            "{:.2}% a year",
            yearly * 100.0
        );
    }
    for (s, st) in rs.settlements.iter().zip(&rs.states) {
        assert!(
            st.mood > 0.6 && st.food > 0.0,
            "{}: mood {:.2}, food {:.0}",
            s.name,
            st.mood,
            st.food
        );
    }
}

#[test]
fn a_settlement_without_farmers_runs_out_and_shrinks() {
    let mut rs = meridian();
    let i = rs
        .settlements
        .iter()
        .position(|s| rs.society.cultures[s.culture].profile.tech_tier == TechTier::Medieval)
        .unwrap();
    let farmers: Vec<usize> = rs.settlements[i]
        .roles
        .iter()
        .enumerate()
        .filter(|(_, r)| ["farmer", "herder", "fisher"].contains(&r.0.as_str()))
        .map(|(k, _)| k)
        .collect();
    for _ in 0..(2 * 365) {
        for &k in &farmers {
            rs.states[i].people[k] = 0.0; // the fields lie empty
        }
        rs.step(1.0);
    }
    let st = &rs.states[i];
    let start = f64::from(rs.settlements[i].population);
    assert!(
        st.food < 1.0 && st.mood < 0.3,
        "food {:.0}, mood {:.2}",
        st.food,
        st.mood
    );
    assert!(
        st.population() < 0.95 * start,
        "{:.0} of {start}",
        st.population()
    );
}

#[test]
fn the_time_step_barely_matters() {
    // A year in tiny steps (normal play) and in big ones (time warp).
    let (mut fine, mut coarse) = (meridian(), meridian());
    for _ in 0..(365 * 50) {
        fine.step(0.02);
    }
    for _ in 0..365 {
        coarse.step(1.0);
    }
    let (a, b) = (fine.population(), coarse.population());
    assert!((a - b).abs() / a < 0.002, "{a:.1} vs {b:.1}");
    for (x, y) in fine.states.iter().zip(&coarse.states) {
        assert!((x.mood - y.mood).abs() < 0.02);
    }
}

#[test]
fn relations_drift_but_stay_in_range_and_symmetric() {
    let mut rs = meridian();
    let start = rs.relations.clone();
    for _ in 0..(3 * 365) {
        rs.step(1.0);
    }
    let mut moved = 0.0;
    for (i, row) in rs.relations.iter().enumerate() {
        for (j, &r) in row.iter().enumerate() {
            assert_eq!(r, rs.relations[j][i]);
            assert!((-100.0..=100.0).contains(&r));
            moved += (r - start[i][j]).abs();
        }
    }
    assert!(moved > 1.0, "relations change over the years");
}

#[test]
fn society_state_survives_a_save() {
    let system = Arc::new(system());
    let make = || {
        let mut sim = sim_core::Simulation::for_star_system(system.clone(), |_, s| {
            sim_society::aggregate::add_society_systems(s)
        });
        sim_society::aggregate::install(
            &mut sim,
            &Arc::new(director()),
            &sim_society::SettlementPatterns::load_dir(
                &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../../../../content/data/settlement_patterns"),
            )
            .unwrap(),
        );
        sim
    };
    let mut sim = make();
    let body = system
        .bodies
        .iter()
        .position(|b| b.id == "meridian")
        .unwrap();
    // Some years go by.
    sim.regions
        .get_mut(body)
        .unwrap()
        .world
        .resource_mut::<RegionSociety>()
        .step(800.0);
    let saved = capture(&sim, &system);
    let text = serde_json::to_string(&saved).unwrap();
    let mut fresh = make();
    let problems = restore(
        &mut fresh,
        &system,
        &serde_json::from_str::<Vec<_>>(&text).unwrap(),
    );
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(capture(&fresh, &system), saved);
}

#[test]
fn a_tick_of_the_aggregate_costs_well_under_a_millisecond() {
    let mut rs = meridian();
    // A tick steps one slice of the settlements (each catches up about once a second).
    let start = Instant::now();
    for _ in 0..1000 {
        rs.step_slice(0.0002);
    }
    let per_tick = start.elapsed().as_secs_f64() * 1e3 / 1000.0;
    // Every settlement at once, for comparison.
    let start = Instant::now();
    for _ in 0..100 {
        rs.step(0.02);
    }
    let per_step = start.elapsed().as_secs_f64() * 1e3 / 100.0;
    println!(
        "{} settlements: {per_tick:.4} ms per tick, {per_step:.3} ms for all at once",
        rs.settlements.len()
    );
    assert!(per_tick < 0.5, "{per_tick} ms per tick");
}
