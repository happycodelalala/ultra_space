//! Regions (planet-geography.md §1b, stage 7): on land, river basins bounded by divides and coasts,
//! small ones merged and big ones split; at sea, basins of open water split where coasts pinch
//! them. Each is named from what it holds.

use std::collections::HashMap;

use glam::DVec3;

use super::generate::{Cover, Fields, Rng, distances};
use super::sphere::Sphere;
use super::{Region, RegionKind};

/// Land and sea regions wanted (about).
const LAND_REGIONS: f64 = 24.0;
const SEA_REGIONS: f64 = 9.0;

/// Merges labels smaller than `min` into the neighbour they share most border with (`weights`
/// adds weaker links, such as across straits), smallest first, keeping merges under `max`.
fn merge_small(label: &mut [u32], pairs: &[(u32, u32, f64)], min: usize, max: usize) {
    for _ in 0..80 {
        let mut size: HashMap<u32, usize> = HashMap::new();
        for &l in label.iter() {
            if l != u32::MAX {
                *size.entry(l).or_default() += 1;
            }
        }
        let mut border: HashMap<(u32, u32), f64> = HashMap::new();
        for &(i, j, w) in pairs {
            let (a, b) = (label[i as usize], label[j as usize]);
            if a == b || a == u32::MAX || b == u32::MAX {
                continue;
            }
            for (u, v) in [(a, b), (b, a)] {
                if size[&u] < min {
                    *border.entry((u, v)).or_default() += w;
                }
            }
        }
        let mut choice: HashMap<u32, (u32, f64)> = HashMap::new();
        for (&(u, v), &w) in &border {
            // Prefer the smaller neighbour, so regions stay even.
            let score = w / (1.0 + size[&v] as f64 / min as f64);
            if choice.get(&u).is_none_or(|c| score > c.1) {
                choice.insert(u, (v, score));
            }
        }
        let mut smalls: Vec<u32> = choice.keys().copied().collect();
        smalls.sort_by_key(|u| (size[u], *u));
        let mut remap: HashMap<u32, u32> = HashMap::new();
        for u in smalls {
            let v = choice[&u].0;
            if remap.contains_key(&v) || remap.values().any(|&t| t == u) {
                continue;
            }
            if size[&u] + size[&v] > max && size[&u] * 6 > min {
                continue;
            }
            remap.insert(u, v);
        }
        if remap.is_empty() {
            return;
        }
        for l in label.iter_mut() {
            if let Some(&t) = remap.get(l) {
                *l = t;
            }
        }
    }
}

/// Splits labels bigger than 1.5 × `target` points into pieces by k-means.
fn split_big(s: &Sphere, label: &mut [u32], target: usize, rng: &mut Rng, next_id: &mut u32) {
    let mut members: HashMap<u32, Vec<usize>> = HashMap::new();
    for (i, &l) in label.iter().enumerate() {
        if l != u32::MAX {
            members.entry(l).or_default().push(i);
        }
    }
    let mut ids: Vec<u32> = members.keys().copied().collect();
    ids.sort_unstable();
    for id in ids {
        let m = &members[&id];
        let k = (m.len() as f64 / target as f64).round() as usize;
        if k < 2 {
            continue;
        }
        let mut centres: Vec<DVec3> = (0..k).map(|_| s.points[m[rng.below(m.len())]]).collect();
        let mut part = vec![0usize; m.len()];
        for _ in 0..10 {
            for (a, &i) in m.iter().enumerate() {
                let p = s.points[i];
                part[a] = (0..k)
                    .max_by(|&x, &y| p.dot(centres[x]).total_cmp(&p.dot(centres[y])))
                    .unwrap_or(0);
            }
            for (c, centre) in centres.iter_mut().enumerate() {
                let sum: DVec3 = m
                    .iter()
                    .zip(&part)
                    .filter(|(_, p)| **p == c)
                    .map(|(&i, _)| s.points[i])
                    .sum();
                *centre = sum.normalize_or(*centre);
            }
        }
        let new: Vec<u32> = (0..k)
            .map(|c| {
                if c == 0 {
                    id
                } else {
                    *next_id += 1;
                    *next_id
                }
            })
            .collect();
        for (a, &i) in m.iter().enumerate() {
            label[i] = new[part[a]];
        }
    }
}

/// Region per point, and the regions.
pub(super) fn grow(
    s: &Sphere,
    f: &Fields,
    radius: f64,
    seed: u64,
    kinds: &[RegionKind],
    adjectives: &[String],
    temperature: impl Fn(usize) -> f64,
) -> (Vec<u32>, Vec<Region>) {
    let n = s.len();
    let edge = s.spacing * radius;
    let mut rng = Rng::new(seed ^ 0x04E6_13E5);
    let land_points = f.open.iter().filter(|&&o| !o).count();
    let sea_points = n - land_points;

    // Land: river basins (the outlet's basin, taken downstream first).
    let mut label: Vec<u32> = (0..n as u32).collect();
    for &i in &f.order {
        let iu = i as usize;
        let r = f.receiver[iu] as usize;
        if r != iu && !f.open[iu] {
            label[iu] = label[r];
        }
    }
    for (i, l) in label.iter_mut().enumerate() {
        if f.open[i] {
            *l = u32::MAX;
        }
    }
    let mut pairs: Vec<(u32, u32, f64)> = Vec::new();
    for i in 0..n {
        for &j in s.neighbours(i) {
            if (i as u32) < j && !f.open[i] && !f.open[j as usize] {
                pairs.push((i as u32, j, 1.0));
            }
        }
        // Land points across a narrow strait count as neighbours, weakly.
        if f.open[i] {
            let land: Vec<u32> = s
                .neighbours(i)
                .iter()
                .copied()
                .filter(|&j| !f.open[j as usize])
                .collect();
            for a in 0..land.len() {
                for b in a + 1..land.len() {
                    pairs.push((land[a], land[b], 0.2));
                }
            }
        }
    }
    let target = (land_points as f64 / LAND_REGIONS) as usize;
    merge_small(&mut label, &pairs, target / 2, target * 8 / 5);
    // Small islands still on their own join the nearest bigger land region within reach.
    let mut size: HashMap<u32, usize> = HashMap::new();
    for &l in label.iter().filter(|&&l| l != u32::MAX) {
        *size.entry(l).or_default() += 1;
    }
    let mut small: Vec<u32> = size
        .iter()
        .filter(|(_, c)| **c < target / 2)
        .map(|(&l, _)| l)
        .collect();
    small.sort_unstable();
    for l in small {
        let members: Vec<usize> = (0..n).filter(|&i| label[i] == l).collect();
        let centre = members
            .iter()
            .map(|&i| s.points[i])
            .sum::<DVec3>()
            .normalize_or(DVec3::Y);
        let nearest = (0..n)
            .filter(|&i| label[i] != u32::MAX && label[i] != l && size[&label[i]] >= target / 2)
            .max_by(|&a, &b| s.points[a].dot(centre).total_cmp(&s.points[b].dot(centre)));
        if let Some(j) = nearest
            && s.points[j].angle_between(centre) * radius < 30_000.0
        {
            let into = label[j];
            for &i in &members {
                label[i] = into;
            }
            *size.entry(into).or_default() += members.len();
        }
    }
    let mut next_id = n as u32;
    split_big(s, &mut label, target, &mut rng, &mut next_id);

    // Sea: watershed of the distance from land, so seas part where coasts pinch them.
    let shore: Vec<u32> = (0..n as u32).filter(|&i| !f.open[i as usize]).collect();
    let (from_land, _) = distances(s, edge, &shore);
    let mut by_depth: Vec<usize> = (0..n).filter(|&i| f.open[i]).collect();
    by_depth.sort_by(|&a, &b| from_land[b].total_cmp(&from_land[a]));
    let mut sea_label = vec![u32::MAX; n];
    for &i in &by_depth {
        let up = s
            .neighbours(i)
            .iter()
            .filter(|&&j| sea_label[j as usize] != u32::MAX)
            .max_by(|&&a, &&b| from_land[a as usize].total_cmp(&from_land[b as usize]));
        sea_label[i] = match up {
            Some(&j) => sea_label[j as usize],
            None => {
                next_id += 1;
                next_id
            }
        };
    }
    let sea_pairs: Vec<(u32, u32, f64)> = (0..n)
        .flat_map(|i| {
            s.neighbours(i)
                .iter()
                .filter(move |&&j| (i as u32) < j)
                .map(move |&j| (i as u32, j, 1.0))
        })
        .filter(|&(i, j, _)| f.open[i as usize] && f.open[j as usize])
        .collect();
    let sea_target = (sea_points as f64 / SEA_REGIONS) as usize;
    merge_small(&mut sea_label, &sea_pairs, sea_target / 2, sea_target * 2);
    split_big(
        s,
        &mut sea_label,
        sea_target * 3 / 2,
        &mut rng,
        &mut next_id,
    );
    for i in 0..n {
        if f.open[i] {
            label[i] = sea_label[i];
        }
    }

    // Compact ids.
    let mut ids: Vec<u32> = label.clone();
    ids.sort_unstable();
    ids.dedup();
    let region: Vec<u32> = label
        .iter()
        .map(|l| ids.binary_search(l).expect("an id") as u32)
        .collect();

    // What each region holds, and its kind.
    let kind = |id: &str| {
        kinds
            .iter()
            .position(|k| k.id == id)
            .unwrap_or_else(|| panic!("region kind {id} exists"))
    };
    let mut members: Vec<Vec<usize>> = vec![Vec::new(); ids.len()];
    for (i, &r) in region.iter().enumerate() {
        members[r as usize].push(i);
    }
    let mut pool: Vec<String> = adjectives.to_vec();
    let mut regions = Vec::with_capacity(ids.len());
    for m in &members {
        let count = m.len().max(1) as f64;
        let sea = m.iter().filter(|&&i| f.open[i]).count() as f64 / count;
        let land: Vec<usize> = m.iter().copied().filter(|&i| !f.open[i]).collect();
        let ln = land.len().max(1) as f64;
        let share =
            |c: &[Cover]| land.iter().filter(|&&i| c.contains(&f.cover[i])).count() as f64 / ln;
        let temp = land.iter().map(|&i| temperature(i)).sum::<f64>() / ln;
        let mut heights: Vec<f64> = land.iter().map(|&i| f.h[i]).collect();
        heights.sort_by(f64::total_cmp);
        let pct = |q: f64| {
            heights
                .get(((heights.len() as f64 * q) as usize).min(heights.len().saturating_sub(1)))
                .copied()
                .unwrap_or(0.0)
        };
        let coast = land
            .iter()
            .filter(|&&i| s.neighbours(i).iter().any(|&j| f.open[j as usize]))
            .count() as f64
            / ln;
        let id = if sea > 0.6 {
            if (m.len() as f64) < 0.6 * sea_target as f64 {
                "bay"
            } else {
                "sea"
            }
        } else if coast > 0.5 && (land.len() as f64) < 0.4 * target as f64 {
            "isles"
        } else if pct(0.8) > 3_000.0 {
            "peaks"
        } else if share(&[Cover::Ice, Cover::Snow]) > 0.4 {
            "icefield"
        } else if share(&[Cover::Tundra, Cover::ColdDesert]) > 0.4 {
            "barrens"
        } else if pct(0.5) > 1_200.0 {
            "highlands"
        } else if share(&[Cover::Lake, Cover::Salt]) > 0.06 {
            "meres"
        } else if share(&[Cover::Desert]) > 0.4 {
            if temp > 14.0 { "wastes" } else { "steppe" }
        } else if share(&[Cover::Steppe, Cover::Savanna]) > 0.4 {
            "steppe"
        } else if share(&[Cover::Jungle, Cover::Rainforest, Cover::Seasonal]) > 0.4 {
            "wilds"
        } else if share(&[Cover::Forest, Cover::Taiga]) > 0.4 {
            "vale"
        } else if coast > 0.25 {
            "coast"
        } else if pct(0.9) - pct(0.1) > 500.0 {
            "downs"
        } else {
            "plains"
        };
        let k = kind(id);
        let adjective = if pool.is_empty() {
            String::from("Far")
        } else {
            pool.remove(rng.below(pool.len()))
        };
        let centre = m
            .iter()
            .map(|&i| s.points[i])
            .sum::<DVec3>()
            .normalize_or(DVec3::Y);
        regions.push(Region {
            kind: k,
            name: format!("{adjective} {}", kinds[k].noun),
            centre,
        });
    }
    (region, regions)
}
