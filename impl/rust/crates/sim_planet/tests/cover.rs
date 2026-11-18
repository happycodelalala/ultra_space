//! Ground cover placement (design/ground-cover.md): deterministic, the same things at every
//! level of detail, nothing in the sea or the village.

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use sim_planet::blocks::LodKey;
use sim_planet::cover::CoverWorld;
use sim_planet::{BlockGround, GroundStyle, Terrain};

fn tessera() -> &'static (Arc<Terrain>, BlockGround) {
    static T: OnceLock<(Arc<Terrain>, BlockGround)> = OnceLock::new();
    T.get_or_init(|| {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../content/data/star_systems/tessera.json");
        let sys = sim_core::StarSystem::from_path(path).unwrap();
        let body = sys.bodies.iter().find(|b| b.terrain.is_some()).unwrap();
        let t = Arc::new(Terrain::of(body).unwrap());
        let layout: sim_planet::VillageLayout = serde_json::from_str(
            &std::fs::read_to_string(
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../../../../content/data/village_layouts/demo_village.json"),
            )
            .unwrap(),
        )
        .unwrap();
        // A green spot for the "village".
        let globe = t.globe().unwrap().clone();
        let centre = sim_planet::fibonacci_directions(4_000)
            .find(|&d| matches!(globe.cover(d), sim_planet::globe::Cover::Forest))
            .unwrap();
        let g = BlockGround::new(t.clone(), centre, Some(layout))
            .with_style(GroundStyle::Faceted)
            .with_cover(Arc::new(CoverWorld::new(11, 1.0)));
        (t, g)
    })
}

fn key_at(g: &BlockGround, dir: glam::DVec3, level: u8) -> LodKey {
    let (face, p) = g.locate(dir);
    let size = BlockGround::lod_size(level);
    (
        level,
        face,
        (p.x / size).floor() as i32,
        (p.y / size).floor() as i32,
    )
}

#[test]
fn cover_is_deterministic_and_the_same_at_every_level() {
    let (_, g) = tessera();
    let cover = g.cover.as_ref().unwrap();
    // A forest chunk 600 m from the village centre.
    let dir = g.dir_on(0, 600.0, 300.0);
    let coarse = key_at(g, dir, 2);
    let a = cover.instances(g, coarse);
    let b = cover.instances(g, coarse);
    assert!(!a.is_empty(), "something grows there");
    assert_eq!(a.len(), b.len());
    for (x, y) in a.iter().zip(&b) {
        assert_eq!((x.species, x.variant), (y.species, y.variant));
        assert!(x.at.distance(y.at) < 1e-9);
    }
    // Each thing of the coarse chunk is in one of its fine descendants, at the same place.
    let mut fine = Vec::new();
    for c in BlockGround::lod_children(coarse) {
        for cc in BlockGround::lod_children(c) {
            fine.extend(cover.instances(g, cc));
        }
    }
    let mut missing = 0;
    for x in &a {
        if !fine
            .iter()
            .any(|y| y.species == x.species && y.variant == x.variant && y.at.distance(x.at) < 1e-6)
        {
            missing += 1;
        }
    }
    assert_eq!(
        missing,
        0,
        "{missing} of {} coarse things not found near",
        a.len()
    );
}

#[test]
fn nothing_grows_in_the_sea_or_the_village() {
    let (t, g) = tessera();
    let cover = g.cover.as_ref().unwrap();
    // The village's square (face 0 round the centre).
    for (x, y) in [(-16.0, -16.0), (0.0, -16.0), (-16.0, 0.0), (0.0, 0.0)] {
        let key = (0u8, 0u8, (x / 32.0) as i32, (y / 32.0) as i32);
        let inside: Vec<_> = cover
            .instances(g, key)
            .into_iter()
            .filter(|i| i.at.x.abs() < 22.0 && i.at.y.abs() < 22.0)
            .collect();
        assert!(inside.is_empty(), "{} things in the village", inside.len());
    }
    // The sea.
    let globe = t.globe().unwrap();
    let sea = sim_planet::fibonacci_directions(4_000)
        .find(|&d| t.height(d, 100.0) < -500.0 && globe.open_sea(d))
        .unwrap();
    for level in 0..=4 {
        assert!(cover.instances(g, key_at(g, sea, level)).is_empty());
    }
}
