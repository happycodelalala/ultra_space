//! Roads joining settlements (living-planet.md §8b, task 4b.8): every settlement to the
//! nearest of its people's that is at least its size (a hamlet to its village, a village to its
//! town), and each town or city to its two nearest peers, so roads lead from settlement to
//! settlement across a people's land. Generated from the records, the same every time; drawn
//! near the player.

use std::collections::HashMap;

use glam::DVec3;

use crate::aggregate::RegionSociety;
use crate::settlements::SizeClass;

/// Farthest a road joins two settlements (m).
const LONGEST: f64 = 40_000.0;

/// The roads of `rs`'s body, as pairs of settlement indices (the smaller first), on a body of
/// `radius` m.
pub fn roads(rs: &RegionSociety, radius: f64) -> Vec<(usize, usize)> {
    let s = &rs.settlements;
    // Settlements bucketed in cubes of the longest road, for neighbours.
    let key = |d: DVec3| {
        let p = d * radius / LONGEST;
        (p.x.floor() as i64, p.y.floor() as i64, p.z.floor() as i64)
    };
    let mut cells: HashMap<(i64, i64, i64), Vec<usize>> = HashMap::new();
    for (i, x) in s.iter().enumerate() {
        cells.entry(key(x.direction)).or_default().push(i);
    }
    let distance = |i: usize, j: usize| s[i].direction.angle_between(s[j].direction) * radius;
    let mut out = std::collections::BTreeSet::new();
    for (i, x) in s.iter().enumerate() {
        if x.size == SizeClass::Dwelling {
            continue;
        }
        let (cx, cy, cz) = key(x.direction);
        let mut near: Vec<(f64, usize)> = Vec::new();
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    for &j in cells
                        .get(&(cx + dx, cy + dy, cz + dz))
                        .into_iter()
                        .flatten()
                    {
                        if j != i && s[j].culture == x.culture && s[j].size >= x.size {
                            let d = distance(i, j);
                            if d < LONGEST {
                                near.push((d, j));
                            }
                        }
                    }
                }
            }
        }
        near.sort_by(|a, b| a.0.total_cmp(&b.0));
        let links = if x.size >= SizeClass::Town { 2 } else { 1 };
        for &(_, j) in near.iter().take(links) {
            out.insert((i.min(j), i.max(j)));
        }
    }
    out.into_iter().collect()
}
