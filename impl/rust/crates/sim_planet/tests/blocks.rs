//! The block ground, planet-wide (block-world.md §5.0, §7).

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use glam::{DVec2, DVec3};
use sim_planet::blocks::ChunkKey;
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

fn world(c: &sim_planet::blocks::ChunkMesh) -> impl Iterator<Item = DVec3> + '_ {
    c.mesh
        .positions
        .iter()
        .map(|p| c.origin + c.basis * DVec3::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2])))
}

#[test]
fn every_face_maps_both_ways_and_face_0_is_the_village_plane() {
    let t = meridian();
    let g = BlockGround::new(t.clone(), land(&t), None);
    for face in 0..6u8 {
        for (x, y) in [
            (0.0, 0.0),
            (120.5, -40.25),
            (-70_000.0, 60_000.0),
            (3_000.0, 55_000.0),
        ] {
            let (f, p) = g.locate(g.dir_on(face, x, y));
            assert_eq!(f, face);
            assert!((p - DVec2::new(x, y)).length() < 1e-6, "{p} vs {x},{y}");
        }
    }
    // Near the village the grid is its tangent grid (well under a millimetre off).
    for (x, y) in [(10.0, 20.0), (-60.0, 45.0)] {
        let tangent = (g.center * g.radius + g.east * x + g.north * y).normalize();
        assert!(g.dir(x, y).angle_between(tangent) * g.radius < 1e-3);
    }
    // Every direction lands on one face, inside its square.
    for d in sim_planet::fibonacci_directions(2_000) {
        let (_, p) = g.locate(d);
        let half = std::f64::consts::FRAC_PI_4 * g.radius + 1e-6;
        assert!(p.x.abs() <= half && p.y.abs() <= half);
    }
}

#[test]
fn the_surface_is_the_natural_ground_stepped() {
    let t = meridian();
    let c = land(&t);
    let g = BlockGround::new(t.clone(), c, None);
    let with = t.with_blocks(Arc::new(BlockGround::new(t.clone(), c, None)));
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
fn the_focus_bounds_the_block_surface() {
    let t = meridian();
    let c = land(&t);
    let g = Arc::new(BlockGround::new(t.clone(), c, None));
    let with = t.with_blocks(g.clone());
    let far = g.dir(5_000.0, 0.0);
    g.set_focus(Some(c));
    assert!(g.surface(c).is_some());
    assert!(g.surface(far).is_none(), "beyond the focus: natural ground");
    assert!((with.surface_height(far, 1.0) - t.surface_height(far, 1.0)).abs() < 1e-9);
    g.set_focus(Some(far));
    assert!(g.surface(far).is_some());
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
    let g = BlockGround::new(t.clone(), c, Some(village));
    assert!(g.top(0, 0).is_some());
    let chunk = g.chunk((0, 0, 0)).expect("a chunk at the centre");
    assert!(chunk.mesh.triangles() > 10);
    assert!(world(&chunk).all(|p| p.is_finite()));
}

#[test]
fn chunks_sit_on_the_ground_anywhere() {
    let t = meridian();
    let g = BlockGround::new(t.clone(), land(&t), None);
    // A chunk 1 km out on the village's face, and chunks on land on every other face: their
    // blocks sit on the ground there.
    let mut keys: Vec<ChunkKey> = vec![(0, 1_000 / 8, 0)];
    for face in 1..6u8 {
        if let Some(d) = sim_planet::fibonacci_directions(4_000)
            .find(|&d| g.locate(d).0 == face && t.height(d, 1.0) > 40.0)
        {
            let (f, p) = g.locate(d);
            let size = BlockGround::chunk_size();
            keys.push((f, (p.x / size).floor() as i32, (p.y / size).floor() as i32));
        }
    }
    assert!(keys.len() >= 3, "land on several faces");
    for key in keys {
        let chunk = g.chunk(key).unwrap_or_else(|| panic!("chunk {key:?}"));
        let top = world(&chunk)
            .map(|w| w.length() - g.radius)
            .fold(f64::MIN, f64::max);
        let natural = t.surface_height(g.chunk_centre(key), 1.0);
        assert!(
            (top - natural).abs() < 6.0,
            "chunk {key:?} top {top:.2} m vs ground {natural:.2} m"
        );
    }
}

#[test]
fn chunks_round_a_point_cover_it_across_a_face_edge() {
    let t = meridian();
    let g = BlockGround::new(t.clone(), land(&t), None);
    // On the edge between faces 0 and 2 (x = πR/4).
    let edge = g.dir_on(0, std::f64::consts::FRAC_PI_4 * g.radius - 1.0, 0.0);
    let keys = g.chunks_around(edge, 40.0);
    let faces: HashSet<u8> = keys.iter().map(|k| k.0).collect();
    assert!(faces.len() >= 2, "both faces: {faces:?}");
    assert!(
        keys.iter()
            .any(|&k| g.chunk_centre(k).angle_between(edge) * g.radius < 8.0)
    );
}

#[test]
fn the_column_cache_forgets_old_chunks_and_still_answers() {
    let t = meridian();
    let g = BlockGround::new(t.clone(), land(&t), None);
    let first = g.top(3, 4);
    // Touch more chunks than the cache keeps (one column each, 8 m apart).
    for k in 0..3_200 {
        let _ = g.top(k * 32, 4);
    }
    assert_eq!(g.top(3, 4), first, "worked out again the same");
}

#[test]
fn coarse_chunks_sit_on_the_ground_and_their_children_fill_them() {
    let t = meridian();
    let g = BlockGround::new(t.clone(), land(&t), None);
    for level in 1..=2u8 {
        let key = (level, 0u8, 3, -2);
        let chunk = g.lod_chunk(key).expect("a coarse chunk");
        let top = world(&chunk)
            .map(|w| w.length() - g.radius)
            .fold(f64::MIN, f64::max);
        let natural = t.surface_height(g.lod_centre(key), 1.0);
        assert!(
            (top - natural).abs() < 12.0,
            "level {level}: top {top:.2} vs ground {natural:.2}"
        );
        // Its four children tile it exactly.
        let size = BlockGround::lod_size(level);
        let child = BlockGround::lod_size(level - 1);
        assert!((size - 2.0 * child).abs() < 1e-9);
        let centre = g.lod_centre(key);
        for c in BlockGround::lod_children(key) {
            let off = g.lod_centre(c).angle_between(centre) * g.radius;
            assert!(
                (off - child * std::f64::consts::FRAC_1_SQRT_2).abs() < 0.01,
                "{off}"
            );
        }
    }
}
