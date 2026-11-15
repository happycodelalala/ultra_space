//! The block ground round the demo village (block-world.md; task 5.3).

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use glam::{DVec2, DVec3};
use sim_planet::{BlockGround, Terrain, VillageGround, blocks::CELL};

fn meridian() -> Arc<Terrain> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../../content/data/star_systems/halcyon.json");
    let sys = sim_core::StarSystem::from_path(path).unwrap();
    Arc::new(Terrain::of(sys.body("meridian").unwrap()).unwrap())
}

/// A land spot on Meridian.
fn land(t: &Terrain) -> DVec3 {
    sim_planet::fibonacci_directions(20_000)
        .find(|&d| t.height(d, 1.0) > 40.0)
        .unwrap()
}

#[test]
fn the_plane_maps_both_ways() {
    let t = meridian();
    let g = BlockGround::new(t.clone(), land(&t), 1_000.0, None);
    for (x, y) in [(0.0, 0.0), (120.5, -40.25), (-700.0, 600.0)] {
        let p = g.plane(g.dir(x, y)).unwrap();
        assert!((p - DVec2::new(x, y)).length() < 1e-6, "{p} vs {x},{y}");
    }
    assert!(g.plane(g.dir(2_000.0, 0.0)).is_none(), "outside the disc");
}

#[test]
fn the_surface_is_the_natural_ground_stepped() {
    let t = meridian();
    let c = land(&t);
    let g = BlockGround::new(t.clone(), c, 1_000.0, None);
    let with = t.with_blocks(Arc::new(BlockGround::new(t.clone(), c, 1_000.0, None)));
    for (x, y) in [(3.1, 4.2), (50.0, -80.0), (-300.0, 200.0)] {
        let d = g.dir(x, y);
        let natural = t.surface_height(d, 1.0);
        let block = with.surface_height(d, 1.0);
        // The block top is the natural height rounded up to the grid (sampled at the column's
        // centre, so within a block or so).
        assert!(
            (block - natural).abs() < 2.0 * CELL + 0.3,
            "{block} vs {natural}"
        );
        let steps = (block - g.base) / CELL;
        assert!((steps - steps.round()).abs() < 1e-6, "on the grid");
        // Collision patches sit below the blocks.
        assert!(with.collision_height(d, 1.0) < block - 0.5);
    }
}

#[test]
fn the_village_ground_shows_inside_its_square() {
    let t = meridian();
    let c = land(&t);
    let n = 8;
    let village = VillageGround {
        origin: [-n, -n],
        dims: [2 * n, 2 * n],
        tops: vec![5; (4 * n * n) as usize],
        colours: vec!["path".into(); (4 * n * n) as usize],
        carved: HashSet::new(),
    };
    let g = BlockGround::new(t.clone(), c, 1_000.0, Some(village));
    assert!(g.top(0, 0).is_some());
    let mesh = g.chunk(0, 0).expect("a chunk at the centre");
    assert!(mesh.triangles() > 10);
    assert!(
        mesh.positions
            .iter()
            .all(|p| p.iter().all(|v| v.is_finite()))
    );
}

#[test]
fn chunks_follow_the_sphere() {
    let t = meridian();
    let g = BlockGround::new(t.clone(), land(&t), 1_500.0, None);
    // A chunk 1 km out: its blocks sit on the ground there, not on the centre's flat plane
    // (which would leave them ~0.8 m off).
    let (ci, cj) = (1_000 / 8, 0);
    let mesh = g.chunk(ci, cj).expect("chunk 1 km out");
    let (basis, origin) = (g.basis(), g.origin());
    let top = mesh
        .positions
        .iter()
        .map(|p| origin + basis * DVec3::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2])))
        .map(|w| w.length() - g.radius)
        .fold(f64::MIN, f64::max);
    let x = (f64::from(ci) + 0.5) * 8.0;
    let natural = t.surface_height(g.dir(x, 4.0), 1.0);
    assert!(
        (top - natural).abs() < 4.0,
        "chunk top {top:.2} m vs ground {natural:.2} m"
    );
}
