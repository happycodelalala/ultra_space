//! The shape rule and the mesher (block-world.md sections 3 and 7).

use std::collections::HashMap;

use glam::IVec3;
use sim_blocks::{ChunkMap, Dir, Grid, mesh, shapes};

fn cells(list: &[(i32, i32, i32)]) -> HashMap<IVec3, u16> {
    list.iter()
        .map(|&(x, y, z)| (IVec3::new(x, y, z), 1))
        .collect()
}

fn bounds(c: &HashMap<IVec3, u16>) -> (IVec3, IVec3) {
    let lo = c.keys().fold(IVec3::splat(i32::MAX), |a, &b| a.min(b));
    let hi = c.keys().fold(IVec3::splat(i32::MIN), |a, &b| a.max(b)) + IVec3::ONE;
    (lo, hi)
}

fn area(m: &sim_blocks::MeshData) -> f64 {
    m.area_by_colour().values().sum()
}

#[test]
fn a_cube_is_six_faces() {
    let c = cells(&[(0, 0, 0)]);
    let m = mesh(&c, IVec3::ZERO, IVec3::ONE, &[Dir::PZ], 1.0);
    assert_eq!(m.triangles(), 12);
    assert!((area(&m) - 6.0).abs() < 1e-6);
}

#[test]
fn flat_runs_merge() {
    // A 4 x 3 x 1 slab: six rectangles.
    let mut list = vec![];
    for x in 0..4 {
        for y in 0..3 {
            list.push((x, y, 0));
        }
    }
    let c = cells(&list);
    let (lo, hi) = bounds(&c);
    let m = mesh(&c, lo, hi, &[], 0.25);
    assert_eq!(m.triangles(), 12);
    let want = 2.0 * (4.0 * 3.0 + 4.0 + 3.0) * 0.0625;
    assert!((area(&m) - want).abs() < 1e-6);
}

#[test]
fn faces_between_cells_are_hidden() {
    let c = cells(&[(0, 0, 0), (1, 0, 0)]);
    let m = mesh(&c, IVec3::ZERO, IVec3::new(2, 1, 1), &[], 1.0);
    assert!((area(&m) - 10.0).abs() < 1e-6);
}

#[test]
fn a_one_block_step_becomes_a_wedge() {
    // Ground two blocks deep, with a one-block step on top: the step's edge slopes down.
    let mut list = vec![];
    for x in 0..6 {
        for y in 0..3 {
            list.push((x, y, 0));
            if x < 3 {
                list.push((x, y, 1));
            }
        }
    }
    let c = cells(&list);
    let (lo, hi) = bounds(&c);
    let s = shapes(&c, lo, hi, &[Dir::PZ]);
    for y in 0..3 {
        let w = s
            .get(&IVec3::new(2, y, 1))
            .expect("the step's edge is a wedge");
        assert_eq!(w.highs(), 2);
        assert!(
            !s.contains_key(&IVec3::new(1, y, 1)),
            "behind the edge stays a block"
        );
    }
}

#[test]
fn small_bumps_stay_blocks() {
    // A one-block and a two-block bump on a slab: no spikes.
    let mut list = vec![];
    for x in 0..8 {
        for y in 0..8 {
            list.push((x, y, 0));
        }
    }
    list.push((2, 2, 1));
    list.push((5, 5, 1));
    list.push((6, 5, 1));
    let c = cells(&list);
    let (lo, hi) = bounds(&c);
    let s = shapes(&c, lo, hi, &[Dir::PZ]);
    assert!(!s.contains_key(&IVec3::new(2, 2, 1)));
    assert!(!s.contains_key(&IVec3::new(5, 5, 1)));
    assert!(!s.contains_key(&IVec3::new(6, 5, 1)));
}

#[test]
fn a_gable_roof_is_two_planes() {
    // A roof two blocks thick stepping one block per course: each side's slope merges into one
    // plane (one quad, two triangles) with the up and down recipe.
    let mut list = vec![];
    for x in -6..6i32 {
        let top = 6 - (if x < 0 { -x - 1 } else { x });
        for y in 0..10 {
            list.push((x, y, top));
            list.push((x, y, top - 1));
        }
    }
    let c = cells(&list);
    let (lo, hi) = bounds(&c);
    let m = mesh(&c, lo, hi, &[Dir::PZ, Dir::NZ], 0.25);
    let up_slopes = m
        .normals
        .iter()
        .filter(|n| n[2] > 0.5 && n[2] < 0.9)
        .count();
    // Two planes of four vertices each (the eave and ridge courses are full blocks).
    assert_eq!(up_slopes, 8, "the top of each side is one merged slope");
}

#[test]
fn a_valley_fills_an_inside_corner() {
    // An L-shaped plateau one block high on a slab: the inside corner is a valley corner.
    let mut list = vec![];
    for x in 0..10 {
        for y in 0..10 {
            list.push((x, y, 0));
            if x < 5 || y < 5 {
                list.push((x, y, 1));
            }
        }
    }
    let c = cells(&list);
    let (lo, hi) = bounds(&c);
    let s = shapes(&c, lo, hi, &[Dir::PZ]);
    let v = s.get(&IVec3::new(4, 4, 1)).expect("inside corner shaped");
    assert_eq!(v.highs(), 3);
}

#[test]
fn chunks_read_across_borders() {
    let mut map = ChunkMap::default();
    for x in 30..34 {
        map.set(IVec3::new(x, 0, 0), 1);
    }
    assert_eq!(map.chunks.len(), 2);
    let m = mesh(&map, IVec3::new(30, 0, 0), IVec3::new(32, 1, 1), &[], 1.0);
    // The face at x = 32 is hidden by the next chunk's cell.
    assert!((area(&m) - (2.0 * 4.0 + 1.0)).abs() < 1e-6);
}

#[test]
fn dense_grid_round_trip() {
    let mut g = Grid::new(IVec3::new(-2, -2, 0), IVec3::new(4, 4, 2));
    g.set(IVec3::new(-2, 1, 1), 3);
    g.set(IVec3::new(5, 5, 5), 3);
    assert_eq!(g.count(), 1);
    assert_eq!(sim_blocks::Cells::get(&g, IVec3::new(-2, 1, 1)), 3);
}

#[test]
fn dirs_parse() {
    assert_eq!(Dir::parse("+z"), Some(Dir::PZ));
    assert_eq!(Dir::parse("-x"), Some(Dir::NX));
    assert_eq!(Dir::parse("z"), None);
}
