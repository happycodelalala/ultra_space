//! The block ground, planet-wide (block-world.md §5.0, §7).

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use glam::{DVec2, DVec3};
use sim_planet::blocks::ChunkKey;
use sim_planet::{BlockGround, Terrain, VillageLayout, blocks::CELL};

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

fn layout() -> VillageLayout {
    serde_json::from_str(
        &std::fs::read_to_string(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../../../content/data/village_layouts/demo_village.json"),
        )
        .unwrap(),
    )
    .unwrap()
}

#[test]
fn the_village_is_laid_on_the_real_ground() {
    let t = meridian();
    let c = land(&t);
    let g = BlockGround::new(t.clone(), c, Some(layout()));
    // The yard is level: its columns' tops differ by at most a block.
    let tops: Vec<i32> = (0..10)
        .flat_map(|a| (0..10).map(move |b| (a, b)))
        .filter_map(|(a, b)| g.top(-4 + a, -6 + b))
        .collect();
    let (lo, hi) = (tops.iter().min().unwrap(), tops.iter().max().unwrap());
    assert!(hi - lo <= 1, "yard tops {lo}..{hi}");
    // The pit is dug: a cell below the ground at its centre is empty.
    let (i, j) = BlockGround::column(7.5, -8.5);
    let top = g.top(i, j).expect("ground at the pit");
    assert_eq!(g.cell(glam::IVec3::new(i, j, top - 2)), 0, "dug");
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

#[test]
fn the_chunk_plan_covers_the_ground_once_without_holes() {
    let t = meridian();
    let g = BlockGround::new(t.clone(), land(&t), None);
    let up = g.dir(40.0, -30.0);
    let camera = up * (g.radius + t.surface_height(up, 1.0) + 25.0);
    // 4 px per column at 1,344 pixels per radian (2560 x 1552, 60°).
    let reach = |level: u8| CELL * f64::from(1u32 << level) * 1_344.0 / 4.0;
    let near = |k: sim_planet::blocks::LodKey| {
        g.lod_centre_point(k).distance(camera)
            - BlockGround::lod_size(k.0) * std::f64::consts::FRAC_1_SQRT_2
    };
    let plan = g.lod_plan(camera, 4, reach, near);
    assert!(plan.len() > 20, "{} chunks", plan.len());
    // Every point within reach of the coarsest level lies in exactly one chunk.
    let (east, south) = sim_planet::tangent_frame(up);
    let outer = reach(4) * 0.9;
    for a in -20..=20 {
        for b in -20..=20 {
            let (x, y) = (f64::from(a) / 20.0 * outer, f64::from(b) / 20.0 * outer);
            if x * x + y * y > outer * outer {
                continue;
            }
            let d = (up * g.radius + east * x + south * y).normalize();
            let (face, p) = g.locate(d);
            let covering = plan
                .iter()
                .filter(|k| {
                    let size = BlockGround::lod_size(k.0);
                    k.1 == face
                        && (p.x / size).floor() as i32 == k.2
                        && (p.y / size).floor() as i32 == k.3
                })
                .count();
            assert_eq!(
                covering, 1,
                "point {x:.0},{y:.0} m covered by {covering} chunks"
            );
        }
    }
}
