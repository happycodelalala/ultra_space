//! Task 4.5: settlement layouts from the building kits, and the foundations under them.

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::Instant;

use ai_gateway::{ContentDirector, ReplayProvider};
use glam::DVec2;
use sim_core::StarSystem;
use sim_planet::Terrain;
use sim_society::{Kits, Layout, RegionLayouts, RegionSociety, Settlement, SizeClass};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../..")
}

struct World {
    society: RegionSociety,
    layouts: RegionLayouts,
    terrain: Terrain,
}

/// Meridian's society and layouts (made once for every test).
fn world() -> &'static World {
    static WORLD: OnceLock<World> = OnceLock::new();
    WORLD.get_or_init(|| {
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
        World {
            society,
            layouts,
            terrain,
        }
    })
}

#[test]
fn kits_load_with_their_buildings() {
    let w = world();
    for id in ["medieval_village", "colony_standin"] {
        let kit = &w.layouts.kits.0[id];
        assert!(kit.buildings.len() >= 10, "{id}");
        for b in &kit.buildings {
            let size = [0, 1, 2].map(|k| b.max_m[k] - b.min_m[k]);
            assert!(
                size.iter().all(|&s| s > 0.3 && s < 40.0),
                "{id}.{}: {size:?}",
                b.id
            );
            assert!(
                root()
                    .join("assets/export/buildings")
                    .join(id)
                    .join(&b.model)
                    .is_file(),
                "{id}.{} model",
                b.id
            );
        }
    }
}

#[test]
fn every_settlement_is_laid_out_with_what_it_needs() {
    let w = world();
    assert_eq!(w.layouts.layouts.len(), w.society.settlements.len());
    for (s, l) in w
        .society
        .settlements
        .iter()
        .zip(w.layouts.layouts.values().map(|l| l.as_ref()))
    {
        let Some(kit) = w.layouts.kits.0.get(&l.kit) else {
            continue; // a people without a kit yet (task 4b.7)
        };
        let count = |use_: &str| {
            l.buildings
                .iter()
                .filter(|p| kit.buildings[p.building].uses.iter().any(|u| u == use_))
                .count()
        };
        let wanted = l.buildings.len() + l.unplaced;
        assert!(
            l.unplaced * 10 <= wanted,
            "{}: {} of {wanted} unplaced",
            s.name,
            l.unplaced
        );
        assert!(!l.streets.is_empty(), "{}: no streets", s.name);
        match s.lore_kind() {
            "town" | "city" => {
                for u in [
                    "square", "hall", "shrine", "inn", "market", "smithy", "home",
                ] {
                    assert!(count(u) >= 1, "{} ({:?}) has no {u}", s.name, s.lore_kind());
                }
            }
            "village" => {
                for u in ["square", "shrine", "home"] {
                    assert!(count(u) >= 1, "{} ({:?}) has no {u}", s.name, s.lore_kind());
                }
            }
            "hamlet" => assert!(count("home") >= 3, "{}", s.name),
            "capital" | "port" => {
                for u in ["pad", "port", "home", "hall", "power", "comms"] {
                    assert!(count(u) >= 1, "{} ({:?}) has no {u}", s.name, s.lore_kind());
                }
            }
            _ => {
                for u in ["pad", "home", "power", "comms"] {
                    assert!(count(u) >= 1, "{} ({:?}) has no {u}", s.name, s.lore_kind());
                }
            }
        }
        // Homes for everyone: medieval households of about five, colony habitats of forty.
        let homes = count("home") + count("shop");
        let per_home = f64::from(s.population) / homes as f64;
        assert!(
            (2.0..=60.0).contains(&per_home),
            "{}: {per_home:.1} people per home",
            s.name
        );
    }
}

#[test]
fn layouts_are_the_same_every_time() {
    let w = world();
    let again = RegionLayouts::generate(&w.society, w.layouts.kits.clone(), &w.terrain);
    assert_eq!(again.layouts, w.layouts.layouts);
}

/// The footprint's own rectangle (without its margin): distance from tangent-plane point `p`.
fn distance(f: &sim_planet::Footprint, p: DVec2, margin: f64) -> f64 {
    let d = p - DVec2::new(f.x, f.z);
    let (s, c) = f.yaw.sin_cos();
    let local = DVec2::new(c * d.x - s * d.y, s * d.x + c * d.y);
    (local.abs() - DVec2::new(f.half[0] - margin, f.half[1] - margin))
        .max(DVec2::ZERO)
        .length()
}

/// Every city and every 20th other settlement (towns, villages, steadings, outposts): the
/// expensive checks don't need all two thousand.
fn sample(w: &World) -> impl Iterator<Item = (&Settlement, &Layout)> {
    w.society
        .settlements
        .iter()
        .zip(w.layouts.layouts.values().map(|l| l.as_ref()))
        .enumerate()
        .filter(|(i, (s, _))| s.size >= SizeClass::City || s.seat || i % 20 == 0)
        .map(|(_, p)| p)
}

#[test]
fn buildings_keep_clear_of_each_other_and_the_streets() {
    let w = world();
    for (s, l) in sample(w) {
        let Some(kit) = w.layouts.kits.0.get(&l.kit) else {
            continue; // a people without a kit yet (task 4b.7)
        };
        let rects: Vec<_> = l
            .buildings
            .iter()
            .map(|p| Layout::footprint(kit, p))
            .collect();
        // Corners of each building (without margin) stay outside every other footprint's own
        // rectangle, and every street point stays a street's half width off the walls.
        for (i, a) in rects.iter().enumerate() {
            let (sn, cs) = a.yaw.sin_cos();
            let corners = [
                (-1.0, -1.0),
                (1.0, -1.0),
                (-1.0, 1.0),
                (1.0, 1.0),
                (0.0, 0.0),
            ]
            .map(|(x, z)| {
                let v = DVec2::new(x * (a.half[0] - 0.8), z * (a.half[1] - 0.8));
                DVec2::new(a.x + cs * v.x + sn * v.y, a.z - sn * v.x + cs * v.y)
            });
            for (j, b) in rects.iter().enumerate() {
                if i != j && (b.x - a.x).abs() < 40.0 && (b.z - a.z).abs() < 40.0 {
                    for c in corners {
                        assert!(
                            distance(b, c, 0.8) > 0.0,
                            "{}: buildings {i} and {j} overlap",
                            s.name
                        );
                    }
                }
            }
        }
        for &(a, b) in &l.streets {
            let (a, b) = (l.nodes[a as usize], l.nodes[b as usize]);
            for k in 0..=10 {
                let p = a.lerp(b, k as f64 / 10.0);
                for (i, r) in rects.iter().enumerate() {
                    if (r.x - p.x).abs() > 40.0 || (r.z - p.y).abs() > 40.0 {
                        continue;
                    }
                    let kind = &kit.buildings[l.buildings[i].building];
                    // The colony's streets run between plots; small props may stand by them.
                    if kind.uses.iter().any(|u| u == "prop" || u == "tree") {
                        continue;
                    }
                    assert!(
                        distance(r, p, 0.8) > 1.0,
                        "{}: street point {p} inside building {i} ({})",
                        s.name,
                        kind.id
                    );
                }
            }
        }
    }
}

#[test]
fn foundations_level_the_ground_under_every_building() {
    let w = world();
    let flat = w.terrain.with_foundations(w.layouts.foundations.clone());
    let mut worst: f64 = 0.0;
    let mut cut: f64 = 0.0;
    for l in w.layouts.layouts.values() {
        let Some(kit) = w.layouts.kits.0.get(&l.kit) else {
            continue; // a people without a kit yet (task 4b.7)
        };
        for p in &l.buildings {
            let f = Layout::footprint(kit, p);
            let (sn, cs) = f.yaw.sin_cos();
            for (x, z) in [
                (0.0, 0.0),
                (-1.0, -1.0),
                (1.0, -1.0),
                (-1.0, 1.0),
                (1.0, 1.0),
            ] {
                // The building's own rectangle, just inside.
                let v = DVec2::new(x * (f.half[0] - 0.9), z * (f.half[1] - 0.9));
                let q = DVec2::new(f.x + cs * v.x + sn * v.y, f.z - sn * v.x + cs * v.y);
                let dir = l.direction(q.x, q.y, w.terrain.radius);
                worst = worst.max((flat.height(dir, 1.0) - p.height).abs());
                cut = cut.max((w.terrain.height(dir, 1.0) - p.height).abs());
            }
        }
    }
    // Level to the centimetre (overlapping blends of neighbours can nudge a corner a little).
    assert!(
        worst < 0.02,
        "ground under a building is {worst:.3} m off level"
    );
    // A plot isn't levelled more than the steepness limit allows.
    assert!(cut <= 3.0 + 0.1, "a foundation cuts or fills {cut:.1} m");
    println!("foundations: level to {worst:.4} m, deepest cut or fill {cut:.2} m");
}

#[test]
fn terrain_sampling_stays_fast_with_foundations() {
    let w = world();
    let flat = w.terrain.with_foundations(w.layouts.foundations.clone());
    // Points all over the planet, and points inside the biggest town.
    let global: Vec<_> = sim_planet::fibonacci_directions(100_000).collect();
    let town = w
        .layouts
        .layouts
        .values()
        .max_by_key(|l| l.buildings.len())
        .unwrap();
    let local: Vec<_> = (0..100_000)
        .map(|i| {
            let a = i as f64 * 2.399_963;
            let r = 300.0 * ((i as f64 + 0.5) / 100_000.0).sqrt();
            town.direction(r * a.cos(), r * a.sin(), w.terrain.radius)
        })
        .collect();
    let time = |t: &Terrain, points: &[glam::DVec3]| {
        let start = Instant::now();
        let mut sum = 0.0;
        for &d in points {
            sum += t.height(d, 1.0);
        }
        std::hint::black_box(sum);
        start.elapsed().as_secs_f64()
    };
    // Warm up, then the better of three runs each.
    let best = |t: &Terrain, p: &[glam::DVec3]| (0..3).map(|_| time(t, p)).fold(f64::MAX, f64::min);
    let (g0, g1) = (best(&w.terrain, &global), best(&flat, &global));
    let (l0, l1) = (best(&w.terrain, &local), best(&flat, &local));
    println!(
        "terrain samples: planet {:.2} → {:.2} µs, in a town {:.2} → {:.2} µs",
        g0 * 10.0,
        g1 * 10.0,
        l0 * 10.0,
        l1 * 10.0
    );
    // Typically +1–8 %; the bounds allow for a busy machine but catch a scan of every
    // footprint (10× and more in a town of 600 buildings).
    assert!(g1 < g0 * 2.0, "planet-wide sampling slowed {:.2}×", g1 / g0);
    assert!(l1 < l0 * 3.0, "sampling in a town slowed {:.2}×", l1 / l0);
}

#[test]
fn layouts_come_and_go_and_the_ground_follows() {
    let w = world();
    // Start empty, as the game does, and make two settlements' layouts.
    let mut rl = RegionLayouts::new(w.layouts.kits.clone(), Arc::new(w.terrain.clone()));
    assert!(rl.foundations.sites().is_empty());
    let hollin: Vec<usize> = (0..w.society.settlements.len())
        .filter(|&i| {
            w.society.society.cultures[w.society.settlements[i].culture]
                .profile
                .id
                == "hollin"
        })
        .take(2)
        .collect();
    for &i in &hollin {
        let l = RegionLayouts::layout_of(&w.society, &rl.kits, &rl.natural, i);
        rl.insert(i, Arc::new(l));
    }
    rl.rebuild_foundations();
    let ids: Vec<u32> = rl.foundations.sites().iter().map(|s| s.id()).collect();
    assert_eq!(ids, hollin.iter().map(|&i| i as u32).collect::<Vec<_>>());
    // The same as laying out everything at once, for these two.
    for &i in &hollin {
        assert_eq!(*rl.layouts[&i], *w.layouts.layouts[&i]);
    }
    // A building's box is found by its settlement's id.
    let first = rl.foundations.sites()[0].id();
    assert!(rl.foundations.structure(first, 0).is_some());
    // Dropping one: its ground is natural again, the other's still level.
    let gone = hollin[0];
    let flat = w.terrain.with_foundations(rl.foundations.clone());
    rl.remove(gone);
    rl.rebuild_foundations();
    let after = w.terrain.with_foundations(rl.foundations.clone());
    let p = &w.layouts.layouts[&gone].buildings[0];
    let at = w.layouts.layouts[&gone].direction(p.x, p.z, w.terrain.radius);
    assert!((flat.height(at, 1.0) - p.height).abs() < 0.02);
    assert_eq!(after.height(at, 1.0), w.terrain.height(at, 1.0));
    assert!(rl.foundations.structure(gone as u32, 0).is_none());
}
