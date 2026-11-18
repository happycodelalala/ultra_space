//! World generation as a short simulation on the mesh (planet-geography.md §1b): plates, crust and
//! uplift, erosion, climate, water and biomes. Each stage reads only the ones before it.

use std::cmp::Ordering;
use std::collections::BinaryHeap;

use glam::DVec3;

use super::sphere::Sphere;
use crate::noise::{fbm, mix64, unit};

/// Share of the crust that is continental (the sea covers about 40% after erosion).
const LAND_SHARE: f64 = 0.72;
/// Plates under this share of the surface join a neighbour (tiny plates make rings).
const MIN_PLATE: f64 = 0.03;
/// Height of the strongest collision ranges before erosion (m).
const RANGE_HEIGHT: f64 = 7_000.0;
/// Erosion: steps, stream-power constant, hillslope creep.
const STEPS: usize = 40;
const STREAM_POWER: f64 = 0.03;
const CREEP: f64 = 0.12;
/// The highest peaks approach this (arcade cap, m).
const PEAK_CAP: f64 = 8_500.0;
/// Moisture is carried about this far over flat land (m).
const MOISTURE_REACH: f64 = 150_000.0;
/// Rising air loses its moisture over this much climb (m).
const MOISTURE_CLIMB: f64 = 2_500.0;
/// Relief narrower than this doesn't lift air (m).
const LIFT_WIDTH: f64 = 12_000.0;
/// Pits deeper than this hold a lake or a salt flat (m).
const PIT_DEPTH: f64 = 25.0;

/// What grows on the land (Whittaker's chart, plus water and ice).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Cover {
    DeepSea,
    Sea,
    Shelf,
    Lake,
    River,
    Salt,
    Ice,
    Snow,
    Tundra,
    Taiga,
    ColdDesert,
    Desert,
    Steppe,
    Grass,
    Forest,
    Rainforest,
    Savanna,
    Seasonal,
    Jungle,
}

impl Cover {
    pub fn is_water(self) -> bool {
        matches!(self, Cover::DeepSea | Cover::Sea | Cover::Shelf)
    }
}

/// The fields over the mesh.
pub struct Fields {
    /// Height above sea level (m).
    pub h: Vec<f64>,
    pub plate: Vec<u32>,
    pub plate_seeds: Vec<DVec3>,
    /// Part of the open sea (not a basin below sea level inland).
    pub open: Vec<bool>,
    /// Rain (mm a year).
    pub rain: Vec<f64>,
    pub river: Vec<bool>,
    pub cover: Vec<Cover>,
    /// Local relief (m): how rough the ground is round each point.
    pub rough: Vec<f64>,
    /// Drainage: each point's receiver, and the points from the outlets upstream.
    pub receiver: Vec<u32>,
    pub order: Vec<u32>,
}

/// A small random number generator from the seed.
pub(super) struct Rng(u64);

impl Rng {
    pub(super) fn new(seed: u64) -> Self {
        Self(mix64(seed ^ 0x0060_10BE_57A7))
    }

    pub(super) fn next(&mut self) -> f64 {
        self.0 = mix64(self.0);
        unit(self.0)
    }

    pub(super) fn below(&mut self, n: usize) -> usize {
        ((self.next() * n as f64) as usize).min(n - 1)
    }

    fn normal(&mut self) -> f64 {
        let (a, b) = (self.next().max(1e-12), self.next());
        (-2.0 * a.ln()).sqrt() * (std::f64::consts::TAU * b).cos()
    }

    fn direction(&mut self) -> DVec3 {
        DVec3::new(self.normal(), self.normal(), self.normal()).normalize_or(DVec3::Y)
    }
}

pub(super) fn smooth(e0: f64, e1: f64, x: f64) -> f64 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Fractal noise scaled to about [-1, 1].
pub(super) fn noise(p: DVec3, seed: u64, octaves: u32) -> f64 {
    (2.0 * fbm(p, seed, octaves, 0.5)).clamp(-1.0, 1.0)
}

/// A heap entry ordered smallest first.
struct Item(f64, u32, u32);

impl PartialEq for Item {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}
impl Eq for Item {}
impl PartialOrd for Item {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Item {
    fn cmp(&self, other: &Self) -> Ordering {
        other.0.total_cmp(&self.0).then(other.1.cmp(&self.1))
    }
}

/// Shortest distance (m) over the mesh from any source, and which source is nearest.
pub(super) fn distances(s: &Sphere, edge: f64, sources: &[u32]) -> (Vec<f64>, Vec<u32>) {
    let mut dist = vec![f64::INFINITY; s.len()];
    let mut from = vec![u32::MAX; s.len()];
    let mut heap = BinaryHeap::new();
    for (k, &i) in sources.iter().enumerate() {
        dist[i as usize] = 0.0;
        from[i as usize] = k as u32;
        heap.push(Item(0.0, i, k as u32));
    }
    while let Some(Item(d, i, k)) = heap.pop() {
        if d > dist[i as usize] {
            continue;
        }
        for &j in s.neighbours(i as usize) {
            let nd = d + edge * s.points[i as usize].distance(s.points[j as usize]) / s.spacing;
            if nd < dist[j as usize] {
                dist[j as usize] = nd;
                from[j as usize] = k;
                heap.push(Item(nd, j, k));
            }
        }
    }
    (dist, from)
}

fn quantile(values: &[f64], q: f64) -> f64 {
    let mut v: Vec<f64> = values.to_vec();
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(f64::total_cmp);
    v[((v.len() as f64 * q) as usize).min(v.len() - 1)]
}

/// Drainage over the filled surface: priority flood from the sea (below sea level), each point's
/// receiver, the order from the outlets upstream, drainage area (points) and the filled height.
struct Flow {
    receiver: Vec<u32>,
    order: Vec<u32>,
    area: Vec<f64>,
    filled: Vec<f64>,
}

fn flow(s: &Sphere, h: &[f64]) -> Flow {
    let n = s.len();
    let mut filled = h.to_vec();
    let mut receiver: Vec<u32> = (0..n as u32).collect();
    let mut visited: Vec<bool> = h.iter().map(|&x| x < 0.0).collect();
    let mut order: Vec<u32> = (0..n as u32).filter(|&i| h[i as usize] < 0.0).collect();
    let mut heap: BinaryHeap<Item> = order.iter().map(|&i| Item(h[i as usize], i, 0)).collect();
    while let Some(Item(z, i, _)) = heap.pop() {
        for &j in s.neighbours(i as usize) {
            let ju = j as usize;
            if !visited[ju] {
                visited[ju] = true;
                filled[ju] = h[ju].max(z + 0.01);
                receiver[ju] = i;
                order.push(j);
                heap.push(Item(filled[ju], j, 0));
            }
        }
    }
    let mut area = vec![1.0; n];
    for &i in order.iter().rev() {
        let r = receiver[i as usize];
        if r != i {
            area[r as usize] += area[i as usize];
        }
    }
    Flow {
        receiver,
        order,
        area,
        filled,
    }
}

/// Sea-level warmth (°C) at a unit direction: from the latitude, with wandering belt edges.
pub(super) fn sea_level_temperature(dir: DVec3, seed: u64) -> f64 {
    let lat = wandering_latitude(dir, seed).abs();
    30.0 - 62.0 * (lat / 90.0).powi(2)
}

/// Latitude (degrees) nudged by noise, so climate belts don't follow straight lines.
pub(super) fn wandering_latitude(dir: DVec3, seed: u64) -> f64 {
    let lat = dir.y.clamp(-1.0, 1.0).asin().to_degrees();
    lat + 7.0 * noise(dir * 3.0, seed ^ 0x7E3A, 3)
}

/// The fields for a planet of `radius` with `plates` plates.
pub fn generate(s: &Sphere, radius: f64, plates: usize, seed: u64) -> Fields {
    let n = s.len();
    let x = &s.points;
    let edge = s.spacing * radius;
    let mut rng = Rng::new(seed);
    let sd = |k: u64| mix64(seed ^ (0x5EED_0000 + k));

    // ---- 1. plates: grown from seeds at different rates, so sizes and outlines vary ----------
    let min_angle = 0.5 * (4.0 * std::f64::consts::PI / plates as f64).sqrt();
    let mut seeds: Vec<usize> = Vec::new();
    while seeds.len() < plates {
        let c = rng.below(n);
        if seeds.iter().all(|&t| x[c].angle_between(x[t]) > min_angle) {
            seeds.push(c);
        }
    }
    let speed: Vec<f64> = (0..plates).map(|_| (0.45 * rng.normal()).exp()).collect();
    let rough_growth: Vec<f64> = x
        .iter()
        .map(|&p| 1.0 + 0.6 * (0.5 + 0.5 * noise(p * 3.0, sd(3), 4)))
        .collect();
    let mut plate = vec![u32::MAX; n];
    let mut best = vec![f64::INFINITY; n];
    let mut heap = BinaryHeap::new();
    for (k, &c) in seeds.iter().enumerate() {
        best[c] = 0.0;
        heap.push(Item(0.0, c as u32, k as u32));
    }
    while let Some(Item(d, i, k)) = heap.pop() {
        let iu = i as usize;
        if plate[iu] != u32::MAX {
            continue;
        }
        plate[iu] = k;
        for &j in s.neighbours(iu) {
            let ju = j as usize;
            if plate[ju] == u32::MAX {
                let nd = d + edge * rough_growth[ju] / speed[k as usize];
                if nd < best[ju] {
                    best[ju] = nd;
                    heap.push(Item(nd, j, k));
                }
            }
        }
    }
    loop {
        let mut count = vec![0usize; plates];
        for &p in &plate {
            count[p as usize] += 1;
        }
        let Some(k) = (0..plates)
            .filter(|&k| count[k] > 0 && (count[k] as f64) < MIN_PLATE * n as f64)
            .min_by_key(|&k| count[k])
        else {
            break;
        };
        let mut touch = vec![0usize; plates];
        for i in (0..n).filter(|&i| plate[i] as usize == k) {
            for &j in s.neighbours(i) {
                let pj = plate[j as usize] as usize;
                if pj != k {
                    touch[pj] += 1;
                }
            }
        }
        let into = (0..plates).max_by_key(|&q| touch[q]).unwrap_or(0) as u32;
        for p in plate.iter_mut().filter(|p| **p as usize == k) {
            *p = into;
        }
    }
    // Each plate turns about its own pole.
    let axes: Vec<DVec3> = (0..plates).map(|_| rng.direction()).collect();
    let omega: Vec<f64> = (0..plates).map(|_| 0.5 + 0.5 * rng.next()).collect();
    let velocity: Vec<DVec3> = (0..n)
        .map(|i| (axes[plate[i] as usize] * omega[plate[i] as usize]).cross(x[i]))
        .collect();

    // ---- 2. crust and uplift ------------------------------------------------------------------
    let bias: Vec<f64> = (0..plates).map(|_| -0.6 + 1.5 * rng.next()).collect();
    let score: Vec<f64> = (0..n)
        .map(|i| 0.9 * bias[plate[i] as usize] + 0.8 * noise(x[i] * 2.0, sd(7), 4))
        .collect();
    let threshold = quantile(&score, 1.0 - LAND_SHARE);
    let mut crust: Vec<f64> = score
        .iter()
        .map(|&v| f64::from(u8::from(v > threshold)))
        .collect();
    for _ in 0..8 {
        let m = s.neighbour_mean(&crust);
        crust = crust
            .iter()
            .zip(&m)
            .map(|(a, b)| 0.5 * a + 0.5 * b)
            .collect();
    }
    // Boundary points: closing speed across their cross-plate edges, the other side's crust.
    let mut closing = vec![0.0; n];
    let mut other = vec![0.0; n];
    let mut boundary = vec![false; n];
    for i in 0..n {
        let (mut sum, mut count, mut oc) = (0.0, 0.0, 0.0);
        for &j in s.neighbours(i) {
            let ju = j as usize;
            if plate[ju] != plate[i] {
                let t = (x[ju] - x[i] * x[ju].dot(x[i])).normalize_or(DVec3::ZERO);
                sum += -(velocity[ju] - velocity[i]).dot(t);
                oc += crust[ju];
                count += 1.0;
            }
        }
        if count > 0.0 {
            boundary[i] = true;
            closing[i] = sum / count;
            other[i] = oc / count;
        }
    }
    // Smoothed along the boundary, so its character changes gradually.
    for _ in 0..12 {
        let mut nc = closing.clone();
        let mut no = other.clone();
        for i in (0..n).filter(|&i| boundary[i]) {
            let (mut sc, mut so, mut cnt) = (0.0, 0.0, 0.0);
            for &j in s.neighbours(i) {
                if boundary[j as usize] {
                    sc += closing[j as usize];
                    so += other[j as usize];
                    cnt += 1.0;
                }
            }
            if cnt > 0.0 {
                nc[i] = 0.5 * closing[i] + 0.5 * sc / cnt;
                no[i] = 0.5 * other[i] + 0.5 * so / cnt;
            }
        }
        closing = nc;
        other = no;
    }
    let bnodes: Vec<u32> = (0..n as u32).filter(|&i| boundary[i as usize]).collect();
    let scale = quantile(
        &bnodes
            .iter()
            .map(|&i| closing[i as usize].abs())
            .collect::<Vec<_>>(),
        0.9,
    )
    .max(1e-9);
    for c in &mut closing {
        *c /= scale;
    }
    let (d_b, from) = distances(s, edge, &bnodes);
    let ridges: Vec<u32> = bnodes
        .iter()
        .copied()
        .filter(|&i| closing[i as usize] < -0.1)
        .collect();
    let d_ridge = if ridges.is_empty() {
        vec![1e9; n]
    } else {
        distances(s, edge, &ridges).0
    };
    let mut base = vec![0.0; n];
    let mut uplift = vec![0.0; n];
    for i in 0..n {
        let src = bnodes[from[i] as usize] as usize;
        let (c, own, oth) = (closing[src], crust[src], other[src]);
        let (coll, open) = (smooth(0.05, 0.7, c), smooth(0.05, 0.7, -c));
        let d = d_b[i];
        let p = x[i];
        let floor = -2_600.0 - 2_600.0 * (d_ridge[i] / 45_000.0).clamp(0.0, 1.0).sqrt();
        let land = 350.0 + 250.0 * noise(p * 6.0, sd(9), 4) + 120.0 * noise(p * 40.0, sd(29), 3);
        let mut b = crust[i] * land + (1.0 - crust[i]) * floor;
        let along = 0.7 + 0.3 * noise(p * 9.0, sd(21), 3);
        let w = (-(d / 10_000.0).powi(2)).exp();
        let plateau = 0.3 * (-(d / 24_000.0).powi(2)).exp();
        let mut u = RANGE_HEIGHT * coll * own * oth * (w + plateau) * along;
        u += 0.65
            * RANGE_HEIGHT
            * coll
            * own
            * (1.0 - oth)
            * (-((d - 6_000.0) / 8_000.0).powi(2)).exp()
            * along;
        b -= 4_000.0 * coll * (1.0 - own) * oth * (-((d - 5_000.0) / 6_000.0).powi(2)).exp();
        let isl = ((noise(p * 25.0, sd(13), 3) + 0.1) * 3.0).clamp(0.0, 1.0);
        u += coll
            * (1.0 - own)
            * (1.0 - oth)
            * ((-b).max(0.0) + 1_500.0)
            * (-(d / 6_000.0).powi(2)).exp()
            * isl;
        b -= 650.0 * open * own * oth * (-(d / 6_000.0).powi(2)).exp();
        base[i] = b;
        uplift[i] = u;
    }
    // Hotspots: chains of volcanoes trailing behind their plate's motion.
    for _ in 0..3 {
        let p = rng.direction();
        let k = plate[s.nearest(p)] as usize;
        for step in 0..6 {
            let q = glam::DQuat::from_axis_angle(axes[k], -(step as f64) * 0.06 * omega[k]) * p;
            let deep = (-base[s.nearest(q)]).max(0.0);
            let height = (4_500.0 - 650.0 * step as f64).max(500.0) + deep;
            for i in 0..n {
                let d = radius * x[i].angle_between(q);
                if d < 6_500.0 {
                    uplift[i] += height * (1.0 - d / 6_500.0).powf(1.5);
                }
            }
        }
    }
    let uplift_noise: Vec<f64> = x
        .iter()
        .map(|&p| 1.0 + 0.35 * noise(p * 30.0, sd(31), 3))
        .collect();

    // ---- 3. erosion: uplift builds the land while rivers cut it --------------------------------
    let cell_area = 4.0 * std::f64::consts::PI * radius * radius / n as f64;
    let mut h = base;
    for _ in 0..STEPS {
        for i in 0..n {
            h[i] += uplift[i] * uplift_noise[i] / STEPS as f64;
        }
        let f = flow(s, &h);
        for &i in &f.order {
            let iu = i as usize;
            let r = f.receiver[iu] as usize;
            if r == iu || h[iu] < 0.0 {
                continue;
            }
            if h[iu] > h[r] {
                let k = STREAM_POWER * (f.area[iu] * cell_area).sqrt() / edge;
                h[iu] = (h[iu] + k * h[r]) / (1.0 + k);
            }
        }
        let m = s.neighbour_mean(&h);
        for i in 0..n {
            if h[i] > 0.0 {
                h[i] += CREEP * (m[i] - h[i]);
            }
        }
    }
    for v in &mut h {
        if *v > 0.0 {
            *v = PEAK_CAP * (*v / PEAK_CAP).tanh();
        }
    }
    let fl = flow(s, &h);
    // The open sea: below-sea areas joined into a big body of water.
    let mut comp = vec![u32::MAX; n];
    let mut sizes = Vec::new();
    for s0 in 0..n {
        if h[s0] >= 0.0 || comp[s0] != u32::MAX {
            continue;
        }
        let id = sizes.len() as u32;
        let mut stack = vec![s0];
        comp[s0] = id;
        let mut count = 0usize;
        while let Some(i) = stack.pop() {
            count += 1;
            for &j in s.neighbours(i) {
                let ju = j as usize;
                if h[ju] < 0.0 && comp[ju] == u32::MAX {
                    comp[ju] = id;
                    stack.push(ju);
                }
            }
        }
        sizes.push(count);
    }
    let open: Vec<bool> = (0..n)
        .map(|i| h[i] < 0.0 && sizes[comp[i] as usize] as f64 > 0.01 * n as f64)
        .collect();

    // ---- 4. climate: circulation, moisture carried from the sea, wrung out by rising air -------
    let temp: Vec<f64> = (0..n)
        .map(|i| sea_level_temperature(x[i], seed) - 0.0065 * h[i].max(0.0))
        .collect();
    // Each point takes its air from the neighbours upwind, weighted by how far upwind they lie
    // (so moisture spreads sideways a little, as real air does).
    let upwind: Vec<Vec<(u32, f64)>> = (0..n)
        .map(|i| {
            let p = x[i];
            let east = DVec3::Y.cross(p).normalize_or(DVec3::X);
            let north = p.cross(east);
            let lat = wandering_latitude(p, seed);
            let a = lat.abs();
            // Easterly trades toward the equator, westerlies poleward from 30° to 60°, polar
            // easterlies; calm at the subtropical highs.
            let u = ((a - 30.0) * (60.0 - a) / 225.0).clamp(-1.0, 1.0);
            let v = -0.4 * (6.0 * a).to_radians().sin();
            let wind = (east * (u + 0.05) + north * v * lat.signum()).normalize_or(east);
            let mut w: Vec<(u32, f64)> = s
                .neighbours(i)
                .iter()
                .map(|&j| {
                    (
                        j,
                        (x[j as usize] - p).normalize().dot(-wind).max(0.0).powi(2),
                    )
                })
                .filter(|&(_, w)| w > 0.0)
                .collect();
            let total: f64 = w.iter().map(|e| e.1).sum();
            for e in &mut w {
                e.1 /= total;
            }
            w
        })
        .collect();
    let from_upwind = |f: &[f64], i: usize| {
        upwind[i]
            .iter()
            .map(|&(j, w)| f[j as usize] * w)
            .sum::<f64>()
    };
    let sea: Vec<bool> = h.iter().map(|&v| v < 0.0).collect();
    let mut hs: Vec<f64> = h.iter().map(|&v| v.max(0.0)).collect();
    for _ in 0..((LIFT_WIDTH / edge).powi(2) as usize) {
        let m = s.neighbour_mean(&hs);
        hs = hs.iter().zip(&m).map(|(a, b)| 0.5 * a + 0.5 * b).collect();
    }
    let base_loss = edge / MOISTURE_REACH;
    let lift: Vec<f64> = (0..n)
        .map(|i| ((hs[i] - from_upwind(&hs, i)).max(0.0) / MOISTURE_CLIMB).clamp(0.0, 0.5))
        .collect();
    let mut q: Vec<f64> = sea.iter().map(|&w| f64::from(u8::from(w))).collect();
    for _ in 0..((400.0 * 1_729.0 / edge) as usize) {
        q = (0..n)
            .map(|i| {
                if sea[i] {
                    1.0
                } else {
                    from_upwind(&q, i) * (1.0 - base_loss - lift[i])
                }
            })
            .collect();
    }
    let rain: Vec<f64> = (0..n)
        .map(|i| {
            let a = wandering_latitude(x[i], seed).abs();
            let belt = 1.0
                + 0.9 * (-(a / 12.0).powi(2)).exp()
                + 0.3 * (-((a - 52.0) / 12.0).powi(2)).exp()
                - 0.7 * (-((a - 27.0) / 8.0).powi(2)).exp()
                - 0.5 * smooth(65.0, 85.0, a);
            let p = if sea[i] {
                1.0
            } else {
                from_upwind(&q, i) * (1.0 + lift[i] / base_loss * 0.08)
            } * belt;
            2_600.0 * p / (0.7 + p)
        })
        .collect();

    // ---- 5. water: rivers where rain gathers, lakes and salt flats in pits ----------------------
    let mut discharge: Vec<f64> = rain.iter().map(|r| r / 1000.0 * cell_area).collect();
    for &i in fl.order.iter().rev() {
        let r = fl.receiver[i as usize];
        if r != i {
            discharge[r as usize] += discharge[i as usize];
        }
    }
    let land_discharge: Vec<f64> = (0..n).filter(|&i| !open[i]).map(|i| discharge[i]).collect();
    let river_min = quantile(&land_discharge, 0.985);
    let river: Vec<bool> = (0..n)
        .map(|i| !open[i] && discharge[i] > river_min)
        .collect();
    let mut lake = vec![false; n];
    let mut salt = vec![false; n];
    for i in 0..n {
        let pit = (!sea[i] && fl.filled[i] - h[i] > PIT_DEPTH) || (sea[i] && !open[i]);
        if pit {
            let evaporation = (0.3 + temp[i] / 30.0).clamp(0.1, 1.5);
            if rain[i] / 1000.0 > 0.55 * evaporation {
                lake[i] = true;
            } else {
                salt[i] = true;
            }
        }
    }

    // ---- 6. biomes ------------------------------------------------------------------------------
    let mean = s.neighbour_mean(&h);
    let cover: Vec<Cover> = (0..n)
        .map(|i| {
            let (hi, t, p) = (h[i], temp[i], rain[i]);
            if open[i] {
                if hi > -400.0 {
                    Cover::Shelf
                } else if hi > -3_500.0 {
                    Cover::Sea
                } else {
                    Cover::DeepSea
                }
            } else if lake[i] {
                Cover::Lake
            } else if river[i] {
                Cover::River
            } else if salt[i] {
                Cover::Salt
            } else if t < -8.0 {
                Cover::Ice
            } else if t < -3.0 && hi > 1_500.0 {
                Cover::Snow
            } else if t < 0.0 {
                Cover::Tundra
            } else if t < 7.0 {
                if p < 300.0 {
                    Cover::ColdDesert
                } else {
                    Cover::Taiga
                }
            } else if t < 20.0 {
                match p {
                    p if p < 250.0 => Cover::Desert,
                    p if p < 550.0 => Cover::Steppe,
                    p if p < 900.0 => Cover::Grass,
                    p if p < 1_700.0 => Cover::Forest,
                    _ => Cover::Rainforest,
                }
            } else {
                match p {
                    p if p < 250.0 => Cover::Desert,
                    p if p < 900.0 => Cover::Savanna,
                    p if p < 1_800.0 => Cover::Seasonal,
                    _ => Cover::Jungle,
                }
            }
        })
        .collect();
    let mut rough: Vec<f64> = (0..n).map(|i| (h[i] - mean[i]).abs()).collect();
    for _ in 0..3 {
        let m = s.neighbour_mean(&rough);
        rough = rough
            .iter()
            .zip(&m)
            .map(|(a, b)| 0.5 * a + 0.5 * b)
            .collect();
    }
    Fields {
        h,
        plate,
        plate_seeds: seeds.iter().map(|&c| x[c]).collect(),
        open,
        rain,
        river,
        cover,
        rough,
        receiver: fl.receiver,
        order: fl.order,
    }
}
