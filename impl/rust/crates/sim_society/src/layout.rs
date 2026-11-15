//! Settlement layouts (living-planet.md §5): buildings and streets on each settlement's tangent
//! plane, by culture style, from its population by role and its layout seed. The same
//! settlement, kit and terrain give the same layout.
//!
//! - **Medieval** (organic): a square with a well at the centre, a few winding streets leaving
//!   it and side lanes branching off; the hall, shrine and inn take the square, workshops and
//!   townhouses the inner streets, cottages and barns the outer ones, a mill at the edge and
//!   trees round about. Buildings face their street.
//! - **Colony** (planned): a square grid of plots around the landing pads at the centre: hangars
//!   and the port by the pads, then administration, labs, workshops, habitats, greenhouses, with
//!   power and comms at the edge; streets between the plots.
//!
//! Buildings sit on foundations: the ground under each is levelled at the height of its centre
//! ([`Layout::site`]), and plots on steep ground or under water are skipped.
//!
//! Tangent plane: x east, z south (m), from [`sim_planet::tangent_frame`]; a building's front
//! (+Z of its model) faces its street.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use glam::{DVec2, DVec3};
use serde::{Deserialize, Serialize};
use sim_core::Simulation;
use sim_planet::{Footprint, Foundations, PlacedBox, Site, Terrain, tangent_frame};

use crate::aggregate::RegionSociety;
use crate::lore::TechTier;
use crate::rng::Rng;
use crate::settlements::{Settlement, SettlementRole, SizeClass};

/// A building of a kit, as `tools/build-kits.sh` measured it (kit.json).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct KitBuilding {
    pub id: String,
    pub uses: Vec<String>,
    /// Model file, next to kit.json.
    pub model: String,
    /// Model bounds (m, model axes: Y up, front +Z).
    pub min_m: [f64; 3],
    pub max_m: [f64; 3],
}

impl KitBuilding {
    fn centre(&self) -> DVec2 {
        DVec2::new(
            0.5 * (self.min_m[0] + self.max_m[0]),
            0.5 * (self.min_m[2] + self.max_m[2]),
        )
    }

    fn half(&self) -> DVec2 {
        DVec2::new(
            0.5 * (self.max_m[0] - self.min_m[0]),
            0.5 * (self.max_m[2] - self.min_m[2]),
        )
    }
}

/// A culture's building kit (`assets/export/buildings/<id>/kit.json`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Kit {
    pub id: String,
    pub source: String,
    pub licence: String,
    pub buildings: Vec<KitBuilding>,
}

impl Kit {
    pub fn load(path: &Path) -> Result<Kit, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// Buildings with this use.
    pub fn with_use(&self, use_: &str) -> Vec<usize> {
        (0..self.buildings.len())
            .filter(|&i| self.buildings[i].uses.iter().any(|u| u == use_))
            .collect()
    }

    pub fn index(&self, id: &str) -> Option<usize> {
        self.buildings.iter().position(|b| b.id == id)
    }
}

/// Every kit, by id.
#[derive(Clone, Debug, Default)]
pub struct Kits(pub HashMap<String, Kit>);

impl Kits {
    /// The kits in `dir` (`<dir>/<kit>/kit.json`); folders without one are skipped.
    pub fn load_dir(dir: &Path) -> Result<Kits, String> {
        let mut kits = HashMap::new();
        let entries = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        for entry in entries.flatten() {
            let path = entry.path().join("kit.json");
            if path.is_file() {
                let kit = Kit::load(&path)?;
                kits.insert(kit.id.clone(), kit);
            }
        }
        Ok(Kits(kits))
    }
}

/// A building placed in a layout: kit building, model origin on the tangent plane (m), turn
/// about the vertical (radians, counter-clockwise from above; 0 faces +Z, south) and the
/// height of its foundation (m above the reference radius).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placed {
    pub building: usize,
    pub x: f64,
    pub z: f64,
    pub yaw: f64,
    pub height: f64,
}

/// A settlement's buildings and streets.
#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    pub kit: String,
    /// Centre (unit direction, surface frame); the tangent plane is its.
    pub center: DVec3,
    pub buildings: Vec<Placed>,
    /// Street graph: points on the tangent plane (m) and the streets between them.
    pub nodes: Vec<DVec2>,
    pub streets: Vec<(u32, u32)>,
    /// Buildings asked for that found no plot.
    pub unplaced: usize,
}

/// Space kept round a footprint (m): half the gap between neighbours, and the plinth.
const MARGIN: f64 = 0.8;
/// Streets: half width (m) and how far buildings stand back from them.
const STREET_HALF: f64 = 2.5;
const SETBACK: f64 = 1.0;
/// Most the ground may rise across a plot before it's too steep (m).
const MAX_RISE: f64 = 3.0;
/// Plots lower than this (m above sea level) are too wet.
const MIN_HEIGHT: f64 = 1.0;
/// How far foundations ease back to the natural ground (m).
pub const FOUNDATION_BLEND: f64 = 4.0;
/// Detail of the terrain used for plots (m).
const PLOT_DETAIL: f64 = 2.0;

impl Layout {
    /// The footprint of placed building `p` (tangent plane, with its margin).
    pub fn footprint(kit: &Kit, p: &Placed) -> Footprint {
        let b = &kit.buildings[p.building];
        let c = rotate(b.centre(), p.yaw);
        let half = b.half() + DVec2::splat(MARGIN);
        Footprint {
            x: p.x + c.x,
            z: p.z + c.y,
            yaw: p.yaw,
            half: [half.x, half.y],
            height: p.height,
        }
    }

    /// The foundations of every building, with their collision boxes (trees and small props
    /// have none), on a body of `radius`.
    pub fn site(&self, kit: &Kit, radius: f64) -> Site {
        let footprints = self
            .buildings
            .iter()
            .map(|p| Self::footprint(kit, p))
            .collect();
        let boxes = (0..self.buildings.len())
            .filter(|&i| {
                let b = &kit.buildings[self.buildings[i].building];
                !b.uses.iter().any(|u| u == "tree")
            })
            .map(|i| {
                let (c, yaw, half) = self.collision_box(kit, i);
                PlacedBox {
                    center: c + DVec3::Y * self.buildings[i].height,
                    yaw,
                    half,
                }
            })
            .collect();
        Site::with_boxes(self.center, footprints, boxes, FOUNDATION_BLEND, radius)
    }

    /// Building `i`'s collision box: centre (tangent plane x, height above its foundation, z;
    /// m), turn and half extents (m), from the model's bounds.
    pub fn collision_box(&self, kit: &Kit, i: usize) -> (DVec3, f64, DVec3) {
        let p = &self.buildings[i];
        let b = &kit.buildings[p.building];
        let c = rotate(b.centre(), p.yaw);
        let half = b.half();
        let h = 0.5 * (b.max_m[1] - b.min_m[1].max(0.0));
        (
            DVec3::new(
                p.x + c.x,
                0.5 * (b.max_m[1] + b.min_m[1].max(0.0)),
                p.z + c.y,
            ),
            p.yaw,
            DVec3::new(half.x, h, half.y),
        )
    }

    /// The unit direction of tangent-plane point (`x`, `z`) on a body of `radius`.
    pub fn direction(&self, x: f64, z: f64, radius: f64) -> DVec3 {
        let (east, south) = tangent_frame(self.center);
        (self.center * radius + east * x + south * z).normalize()
    }
}

/// A turn of `v` (x, z) by `yaw` about the vertical (counter-clockwise from above).
fn rotate(v: DVec2, yaw: f64) -> DVec2 {
    let (s, c) = yaw.sin_cos();
    DVec2::new(c * v.x + s * v.y, -s * v.x + c * v.y)
}

/// The yaw that turns a model's front (+Z) to face `dir` (x, z).
fn facing(dir: DVec2) -> f64 {
    dir.x.atan2(dir.y)
}

/// Placed footprints, indexed on a coarse grid for overlap tests.
struct Plots<'a> {
    kit: &'a Kit,
    terrain: &'a Terrain,
    center: DVec3,
    east: DVec3,
    south: DVec3,
    placed: Vec<Placed>,
    rects: Vec<Footprint>,
    grid: HashMap<(i32, i32), Vec<usize>>,
    /// Street segments that buildings keep off, and the grid cells each touches.
    streets: Vec<(DVec2, DVec2)>,
    street_grid: HashMap<(i32, i32), Vec<usize>>,
}

const GRID: f64 = 24.0;

impl<'a> Plots<'a> {
    fn new(kit: &'a Kit, terrain: &'a Terrain, center: DVec3) -> Self {
        let (east, south) = tangent_frame(center);
        Self {
            kit,
            terrain,
            center,
            east,
            south,
            placed: Vec::new(),
            rects: Vec::new(),
            grid: HashMap::new(),
            streets: Vec::new(),
            street_grid: HashMap::new(),
        }
    }

    fn add_street(&mut self, a: DVec2, b: DVec2) {
        let i = self.streets.len();
        self.streets.push((a, b));
        let lo = (a.min(b) - DVec2::splat(STREET_HALF)) / GRID;
        let hi = (a.max(b) + DVec2::splat(STREET_HALF)) / GRID;
        for z in lo.y.floor() as i32..=hi.y.floor() as i32 {
            for x in lo.x.floor() as i32..=hi.x.floor() as i32 {
                self.street_grid.entry((x, z)).or_default().push(i);
            }
        }
    }

    fn ground(&self, p: DVec2) -> f64 {
        let r = self.terrain.radius;
        let dir = (self.center * r + self.east * p.x + self.south * p.y).normalize();
        self.terrain.height(dir, PLOT_DETAIL)
    }

    fn cells(r: &Footprint) -> impl Iterator<Item = (i32, i32)> {
        let reach = DVec2::new(r.half[0], r.half[1]).length();
        let lo = ((r.x - reach) / GRID).floor() as i32;
        let hi = ((r.x + reach) / GRID).floor() as i32;
        let lz = ((r.z - reach) / GRID).floor() as i32;
        let hz = ((r.z + reach) / GRID).floor() as i32;
        (lz..=hz).flat_map(move |z| (lo..=hi).map(move |x| (x, z)))
    }

    /// Places `building` with its origin at `at`, turned `yaw`, if it fits: clear of other
    /// buildings and streets, on ground that is dry and not too steep. Returns its index.
    fn try_place(&mut self, building: usize, at: DVec2, yaw: f64) -> Option<usize> {
        let mut p = Placed {
            building,
            x: at.x,
            z: at.y,
            yaw,
            height: 0.0,
        };
        let rect = Layout::footprint(self.kit, &p);
        let candidates: Vec<usize> = Self::cells(&rect)
            .filter_map(|c| self.grid.get(&c))
            .flatten()
            .copied()
            .collect();
        if candidates.iter().any(|&i| overlaps(&rect, &self.rects[i])) {
            return None;
        }
        let mut near: Vec<usize> = Self::cells(&rect)
            .filter_map(|c| self.street_grid.get(&c))
            .flatten()
            .copied()
            .collect();
        near.sort_unstable();
        near.dedup();
        if near.iter().any(|&i| {
            let (a, b) = self.streets[i];
            segment_hits(&rect, a, b, STREET_HALF - MARGIN)
        }) {
            return None;
        }
        // Ground: the centre and corners (the footprint without its margin).
        let centre = DVec2::new(rect.x, rect.z);
        let (hx, hz) = (rect.half[0] - MARGIN, rect.half[1] - MARGIN);
        let mut lo = f64::MAX;
        let mut hi = f64::MIN;
        for (sx, sz) in [
            (0.0, 0.0),
            (-1.0, -1.0),
            (1.0, -1.0),
            (-1.0, 1.0),
            (1.0, 1.0),
        ] {
            let h = self.ground(centre + rotate(DVec2::new(sx * hx, sz * hz), yaw));
            lo = lo.min(h);
            hi = hi.max(h);
            if sx == 0.0 {
                p.height = h;
            }
        }
        if hi - lo > MAX_RISE || lo < MIN_HEIGHT {
            return None;
        }
        let rect = Footprint {
            height: p.height,
            ..rect
        };
        let i = self.placed.len();
        for c in Self::cells(&rect) {
            self.grid.entry(c).or_default().push(i);
        }
        self.placed.push(p);
        self.rects.push(rect);
        Some(i)
    }
}

/// Whether two footprints overlap (separating axes).
fn overlaps(a: &Footprint, b: &Footprint) -> bool {
    let reach = |r: &Footprint| DVec2::new(r.half[0], r.half[1]).length();
    let d = DVec2::new(b.x - a.x, b.z - a.z);
    if d.length() > reach(a) + reach(b) {
        return false;
    }
    let axes = |r: &Footprint| {
        let (s, c) = r.yaw.sin_cos();
        [DVec2::new(c, -s), DVec2::new(s, c)]
    };
    let (aa, ba) = (axes(a), axes(b));
    for axis in aa.iter().chain(ba.iter()) {
        let project = |r: &Footprint, ax: &[DVec2; 2]| {
            r.half[0] * ax[0].dot(*axis).abs() + r.half[1] * ax[1].dot(*axis).abs()
        };
        if d.dot(*axis).abs() > project(a, &aa) + project(b, &ba) {
            return false;
        }
    }
    true
}

/// Whether segment `a`–`b`, widened by `half`, touches footprint `r`.
fn segment_hits(r: &Footprint, a: DVec2, b: DVec2, half: f64) -> bool {
    let c = DVec2::new(r.x, r.z);
    let reach = DVec2::new(r.half[0], r.half[1]).length() + half;
    let ab = b - a;
    let t = ((c - a).dot(ab) / ab.length_squared().max(1e-9)).clamp(0.0, 1.0);
    if (a + ab * t).distance(c) > reach {
        return false;
    }
    // Into the footprint's axes; sample the segment (fine for streets against buildings).
    let n = (ab.length() / 1.0).ceil().max(1.0) as usize;
    (0..=n).any(|k| {
        let p = a + ab * (k as f64 / n as f64) - c;
        let local = rotate(p, -r.yaw);
        let out = (local.abs() - DVec2::new(r.half[0], r.half[1])).max(DVec2::ZERO);
        out.length() < half
    })
}

/// How many of each building a settlement needs (use, count), most important first.
fn demand(s: &Settlement, tier: TechTier) -> Vec<(&'static str, usize)> {
    let role = |id: &str| {
        s.roles
            .iter()
            .find(|(r, _)| r == id)
            .map_or(0, |(_, n)| *n as usize)
    };
    let pop = s.population as usize;
    let ceil = |n: usize, per: usize| n.div_ceil(per);
    match tier {
        TechTier::Spacefaring => {
            let colony = s.has(SettlementRole::Capital);
            let port = colony || s.has(SettlementRole::Port);
            vec![
                ("pad", 1 + role("pilot") / 15),
                ("hangar", ceil(role("pilot"), 10)),
                ("port", usize::from(port)),
                ("hall", 1 + pop / 1500),
                ("lab", ceil(role("scientist"), 15)),
                (
                    "workshop",
                    ceil(role("technician") + role("engineer") + role("laborer"), 30),
                ),
                ("home", ceil(pop, 40)),
                ("farm", ceil(role("grower"), 10)),
                ("power", 1 + pop / 800),
                ("comms", 1 + usize::from(colony)),
                ("prop", 1 + pop / 150),
            ]
        }
        _ => {
            let (town, village) = (s.size >= SizeClass::Town, s.size == SizeClass::Village);
            let households = (pop / 5).max(3);
            let shops = if town || village {
                (role("merchant") * 2 / 3).min(households / 2)
            } else {
                0
            };
            vec![
                ("square", 1),
                ("hall", usize::from(town)),
                ("shrine", usize::from(town || village)),
                (
                    "inn",
                    if town {
                        1 + pop / 800
                    } else {
                        usize::from(pop >= 120)
                    },
                ),
                ("market", if town { 6 } else { 2 * usize::from(village) }),
                (
                    "smithy",
                    ceil(role("smith"), 3).max(usize::from(town || village)),
                ),
                ("shop", shops),
                ("home", households - shops),
                ("farm", (role("farmer") + role("herder")) / 8),
                (
                    "mill",
                    if town {
                        2
                    } else {
                        usize::from(village && pop > 100)
                    },
                ),
                ("prop", if town { 4 } else { 1 }),
                ("tree", 12 + households / 3),
            ]
        }
    }
}

/// The layout of `s` (a culture of `tier`) with `kit` on `terrain`.
pub fn generate_layout(s: &Settlement, tier: TechTier, kit: &Kit, terrain: &Terrain) -> Layout {
    let mut rng = Rng::new(s.layout_seed);
    let mut plots = Plots::new(kit, terrain, s.direction);
    let wanted = demand(s, tier);
    let (nodes, streets, unplaced) = match tier {
        TechTier::Spacefaring => colony(s, &wanted, kit, &mut plots, &mut rng),
        _ => village(s, &wanted, kit, &mut plots, &mut rng),
    };
    Layout {
        kit: kit.id.clone(),
        center: s.direction,
        buildings: plots.placed,
        nodes,
        streets,
        unplaced,
    }
}

type Graph = (Vec<DVec2>, Vec<(u32, u32)>, usize);

/// A kit building for `use_`: one of those with it, picked by `rng` (the first when `rng` is
/// `None`); `None` when the kit has none.
fn pick(kit: &Kit, use_: &str, rng: Option<&mut Rng>) -> Option<usize> {
    let all = kit.with_use(use_);
    if all.is_empty() {
        return None;
    }
    Some(match rng {
        Some(r) => *r.pick(&all),
        None => all[0],
    })
}

/// The medieval style.
fn village(
    s: &Settlement,
    wanted: &[(&'static str, usize)],
    kit: &Kit,
    plots: &mut Plots,
    rng: &mut Rng,
) -> Graph {
    let households = wanted
        .iter()
        .filter(|(u, _)| matches!(*u, "home" | "shop"))
        .map(|w| w.1)
        .sum::<usize>();
    let (square, mains, length) = match s.size {
        SizeClass::Town | SizeClass::City | SizeClass::Metropolis | SizeClass::Megalopolis => {
            (18.0, 5 + rng.range(0, 1) as usize, 100.0)
        }
        SizeClass::Village => (12.0, 3, 90.0),
        _ => (8.0, 2, 45.0),
    };
    // Longer streets for more households (each street side holds a house every ~10 m).
    let length = length * (households as f64 / 40.0).sqrt().clamp(0.6, 2.2);
    // At least enough street for every household and farm (about 7 m of frontage each, both
    // sides of each street).
    let length = length.max(7.0 * (households + 4) as f64 / mains as f64);
    let mut nodes: Vec<DVec2> = Vec::new();
    let mut streets: Vec<(u32, u32)> = Vec::new();
    // Street points with their direction, distance along from the square, and a slot side.
    let mut slots: Vec<(f64, DVec2, DVec2)> = Vec::new();

    // The square: a ring of nodes.
    let ring = 8;
    for k in 0..ring {
        let a = std::f64::consts::TAU * k as f64 / ring as f64;
        nodes.push(DVec2::new(a.cos(), a.sin()) * square);
        streets.push((k as u32, ((k + 1) % ring) as u32));
    }
    for k in 0..ring {
        let (a, b) = (nodes[k], nodes[(k + 1) % ring]);
        plots.add_street(a, b);
        // Plots facing the square (outside the ring).
        let mid = 0.5 * (a + b);
        slots.push((0.0, mid, mid.normalize()));
    }
    let start_bearing = rng.unit() * std::f64::consts::TAU;
    let mut walk = |nodes: &mut Vec<DVec2>,
                    streets: &mut Vec<(u32, u32)>,
                    rng: &mut Rng,
                    from: u32,
                    mut heading: f64,
                    len: f64|
     -> Vec<(u32, f64)> {
        let mut out = Vec::new();
        let mut at = from;
        let mut p = nodes[from as usize];
        let base = heading;
        let step = 12.0;
        let mut gone = 0.0;
        while gone < len {
            heading += (rng.unit() - 0.5) * 0.5 + 0.15 * (base - heading);
            let q = p + DVec2::new(heading.cos(), heading.sin()) * step;
            let h = plots.ground(q);
            if h < MIN_HEIGHT + 1.0 || (h - plots.ground(p)).abs() > 0.3 * step {
                break;
            }
            // Reaching another street: a junction with it ends the lane (no crossings).
            if let Some(k) = junction(plots, streets, nodes, at, p, q) {
                streets.push((at, k));
                plots.add_street(p, nodes[k as usize]);
                break;
            }
            nodes.push(q);
            let i = (nodes.len() - 1) as u32;
            streets.push((at, i));
            plots.add_street(p, q);
            out.push((i, heading));
            at = i;
            p = q;
            gone += step;
        }
        out
    };
    let mut lanes = Vec::new();
    for m in 0..mains {
        let bearing = start_bearing
            + std::f64::consts::TAU * (m as f64 + 0.3 * (rng.unit() - 0.5)) / mains as f64;
        let from = ((bearing.rem_euclid(std::f64::consts::TAU) / std::f64::consts::TAU
            * ring as f64)
            .round() as usize
            % ring) as u32;
        let len = length * (0.7 + 0.6 * rng.unit());
        let main = walk(&mut nodes, &mut streets, rng, from, bearing, len);
        // Side lanes off the main street (and, in towns, off those).
        let depth = if s.size >= SizeClass::Town { 2 } else { 1 };
        let mut queue = vec![(main, 0)];
        while let Some((lane, d)) = queue.pop() {
            if d < depth {
                let branch = if s.size >= SizeClass::Town { 0.8 } else { 0.55 };
                for (k, &(node, heading)) in lane.iter().enumerate() {
                    if k % 2 == 1 && rng.unit() < branch / (d + 1) as f64 {
                        let side = if rng.unit() < 0.5 { 1.0 } else { -1.0 };
                        let h = heading
                            + side * (std::f64::consts::FRAC_PI_2 + (rng.unit() - 0.5) * 0.6);
                        let len = length * (0.25 + 0.35 * rng.unit()) / (d + 1) as f64;
                        let side_lane = walk(&mut nodes, &mut streets, rng, node, h, len);
                        queue.push((side_lane, d + 1));
                    }
                }
            }
            lanes.push(lane);
        }
    }
    for lane in &lanes {
        for (k, &(node, heading)) in lane.iter().enumerate() {
            let p0 = if k == 0 {
                nodes[node as usize] - DVec2::new(heading.cos(), heading.sin()) * 12.0
            } else {
                nodes[lane[k - 1].0 as usize]
            };
            let p1 = nodes[node as usize];
            let t = (p1 - p0).normalize_or(DVec2::X);
            for f in [0.25, 0.5, 0.75, 1.0] {
                let p = p0.lerp(p1, f);
                for side in [1.0, -1.0] {
                    let n = DVec2::new(-t.y, t.x) * side;
                    slots.push((p.length(), p, n));
                }
            }
        }
    }
    // Inner plots first, with a little jitter so streets fill unevenly.
    let mut order: Vec<(f64, usize)> = slots
        .iter()
        .enumerate()
        .map(|(i, s)| (s.0 * (0.85 + 0.3 * rng.unit()), i))
        .collect();
    order.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut used = vec![false; slots.len()];
    let mut unplaced = 0;
    let mut grove: Vec<DVec2> = Vec::new();

    for &(use_, count) in wanted {
        for _ in 0..count {
            let ok = match use_ {
                "square" => pick(kit, "square", None)
                    .and_then(|b| plots.try_place(b, DVec2::ZERO, 0.0))
                    .is_some(),
                // Stalls round the square, facing the well.
                "market" => {
                    let b = pick(kit, "market", Some(rng));
                    b.is_some_and(|b| {
                        (0..12).any(|_| {
                            let a = rng.unit() * std::f64::consts::TAU;
                            let p =
                                DVec2::new(a.cos(), a.sin()) * (square * (0.4 + 0.25 * rng.unit()));
                            plots.try_place(b, p, facing(-p)).is_some()
                        })
                    })
                }
                // Trees and props scattered round the edge.
                // Trees in groves just outside the houses (most start next to an earlier tree),
                // props among the streets.
                "tree" | "prop" => {
                    let b = pick(kit, use_, Some(rng));
                    let reach = plots
                        .placed
                        .iter()
                        .filter(|p| !kit.buildings[p.building].uses.iter().any(|u| u == "tree"))
                        .map(|p| DVec2::new(p.x, p.z).length())
                        .fold(square, f64::max)
                        + 25.0;
                    b.is_some_and(|b| {
                        (0..40).any(|_| {
                            let a = rng.unit() * std::f64::consts::TAU;
                            let p = if use_ == "tree" && !grove.is_empty() && rng.unit() < 0.75 {
                                *rng.pick(&grove)
                                    + DVec2::new(a.cos(), a.sin()) * (5.0 + 8.0 * rng.unit())
                            } else if use_ == "tree" {
                                DVec2::new(a.cos(), a.sin()) * reach * (0.85 + 0.3 * rng.unit())
                            } else {
                                DVec2::new(a.cos(), a.sin()) * (square + length * 0.5 * rng.unit())
                            };
                            // Groves stay round the settlement.
                            if p.length() > reach * 1.3 {
                                return false;
                            }
                            let placed = plots
                                .try_place(b, p, rng.unit() * std::f64::consts::TAU)
                                .is_some();
                            if placed && use_ == "tree" {
                                grove.push(p);
                            }
                            placed
                        })
                    })
                }
                _ => {
                    let Some(b) = pick(kit, use_, Some(rng)) else {
                        continue;
                    };
                    // Big public buildings by the square, the mill and farms further out.
                    let skip = match use_ {
                        "farm" | "mill" => order.len() / 3,
                        "home" => order.len() / 10,
                        _ => 0,
                    };
                    let front = kit.buildings[b].max_m[2];
                    let mut placed = false;
                    // Each plot is tried once: one that didn't take a building (taken by a
                    // neighbour, too steep, too wet) is passed over after.
                    for &(_, i) in order.iter().skip(skip) {
                        if used[i] {
                            continue;
                        }
                        used[i] = true;
                        let (_, p, n) = slots[i];
                        let at = p + n * (STREET_HALF + SETBACK + front);
                        if plots.try_place(b, at, facing(-n)).is_some() {
                            placed = true;
                            break;
                        }
                    }
                    placed
                }
            };
            if !ok {
                unplaced += 1;
            }
        }
    }
    let streets = prune(&nodes, streets, plots, ring);
    (nodes, streets, unplaced)
}

/// Where a lane stepping from node `at` (at `p`) to `q` meets another street: within 8 m of
/// `q` or crossing the step. Returns the other street's node nearer `q` (not `at`).
fn junction(
    plots: &Plots,
    streets: &[(u32, u32)],
    nodes: &[DVec2],
    at: u32,
    p: DVec2,
    q: DVec2,
) -> Option<u32> {
    let cell = |v: f64| (v / GRID).floor() as i32;
    let mut best: Option<(f64, u32)> = None;
    for z in cell(q.y.min(p.y) - 8.0)..=cell(q.y.max(p.y) + 8.0) {
        for x in cell(q.x.min(p.x) - 8.0)..=cell(q.x.max(p.x) + 8.0) {
            for &j in plots.street_grid.get(&(x, z)).into_iter().flatten() {
                let (a, b) = streets[j];
                if a == at || b == at {
                    continue;
                }
                let (pa, pb) = (nodes[a as usize], nodes[b as usize]);
                let near = segment_distance(q, pa, pb) < 8.0 || crosses(p, q, pa, pb);
                if !near {
                    continue;
                }
                let k = if pa.distance(q) <= pb.distance(q) {
                    a
                } else {
                    b
                };
                let d = nodes[k as usize].distance(q);
                if best.is_none_or(|(bd, _)| d < bd) {
                    best = Some((d, k));
                }
            }
        }
    }
    best.map(|(_, k)| k)
}

fn segment_distance(p: DVec2, a: DVec2, b: DVec2) -> f64 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_squared().max(1e-9)).clamp(0.0, 1.0);
    (a + ab * t).distance(p)
}

/// Whether segments `p`–`q` and `a`–`b` cross.
fn crosses(p: DVec2, q: DVec2, a: DVec2, b: DVec2) -> bool {
    let side = |o: DVec2, u: DVec2, v: DVec2| (u - o).perp_dot(v - o);
    let (d1, d2) = (side(a, b, p), side(a, b, q));
    let (d3, d4) = (side(p, q, a), side(p, q, b));
    d1 * d2 < 0.0 && d3 * d4 < 0.0
}

/// Keeps only the streets buildings use: the square's ring (the first `ring` nodes), streets
/// between two nodes with a building within 12 m (trees and props don't count), and each such
/// node's shortest way back to the square.
fn prune(nodes: &[DVec2], streets: Vec<(u32, u32)>, plots: &Plots, ring: usize) -> Vec<(u32, u32)> {
    let served = |n: usize| {
        let p = nodes[n];
        let cell = ((p.x / GRID).floor() as i32, (p.y / GRID).floor() as i32);
        (-1..=1).any(|dz| {
            (-1..=1).any(|dx| {
                plots
                    .grid
                    .get(&(cell.0 + dx, cell.1 + dz))
                    .into_iter()
                    .flatten()
                    .any(|&i| {
                        let uses = &plots.kit.buildings[plots.placed[i].building].uses;
                        if uses.iter().any(|u| u == "tree" || u == "prop") {
                            return false;
                        }
                        let r = &plots.rects[i];
                        let reach = DVec2::new(r.half[0], r.half[1]).length();
                        DVec2::new(r.x, r.z).distance(p) < reach + 12.0
                    })
            })
        })
    };
    let served: Vec<bool> = (0..nodes.len()).map(|n| n < ring || served(n)).collect();
    // Shortest ways (in steps) from the square, breadth first.
    let mut links: Vec<Vec<(usize, usize)>> = vec![Vec::new(); nodes.len()];
    for (e, &(a, b)) in streets.iter().enumerate() {
        links[a as usize].push((b as usize, e));
        links[b as usize].push((a as usize, e));
    }
    let mut parent: Vec<Option<usize>> = vec![None; nodes.len()];
    let mut seen = vec![false; nodes.len()];
    let mut queue: std::collections::VecDeque<usize> = (0..ring.min(nodes.len())).collect();
    for &n in &queue {
        seen[n] = true;
    }
    while let Some(n) = queue.pop_front() {
        for &(m, e) in &links[n] {
            if !seen[m] {
                seen[m] = true;
                parent[m] = Some(e);
                queue.push_back(m);
            }
        }
    }
    let mut keep = vec![false; streets.len()];
    for (e, &(a, b)) in streets.iter().enumerate() {
        if served[a as usize] && served[b as usize] {
            keep[e] = true;
        }
    }
    for n in (ring..nodes.len()).filter(|&n| served[n]) {
        let mut at = n;
        while let Some(e) = parent[at] {
            if keep[e] && at != n {
                break;
            }
            keep[e] = true;
            let (a, b) = streets[e];
            at = if a as usize == at {
                b as usize
            } else {
                a as usize
            };
        }
    }
    streets
        .into_iter()
        .zip(keep)
        .filter_map(|(s, k)| k.then_some(s))
        .collect()
}

/// Extra width of the colony's avenues between blocks of plots (m).
const AVENUE: f64 = 14.0;

/// The colony style.
fn colony(
    _s: &Settlement,
    wanted: &[(&'static str, usize)],
    kit: &Kit,
    plots: &mut Plots,
    rng: &mut Rng,
) -> Graph {
    // Plot pitch: the largest building plus a street.
    let pitch = kit
        .buildings
        .iter()
        .map(|b| b.half().max_element() * 2.0)
        .fold(0.0, f64::max)
        + 2.0 * (STREET_HALF + MARGIN)
        + 1.0;
    let total: usize = wanted.iter().map(|w| w.1).sum();
    let side = ((total as f64 * 1.6).sqrt().ceil() as i32 + 2) | 1;
    let half = side / 2;
    let turn = (rng.unit() - 0.5) * std::f64::consts::FRAC_PI_2;
    // Blocks of 3 × 3 plots with an avenue between blocks.
    let g = |i: i32| f64::from(i) * pitch + f64::from((i + 1).div_euclid(3)) * AVENUE;
    let plot_at = |i: i32, j: i32| rotate(DVec2::new(g(i), g(j)), turn);
    // The street line before plot i (between plots i - 1 and i).
    let line = |i: i32| 0.5 * (g(i - 1) + g(i));
    let mut cells: Vec<(f64, i32, i32)> = (-half..=half)
        .flat_map(|j| (-half..=half).map(move |i| (i, j)))
        .map(|(i, j)| ((i * i + j * j) as f64 + 0.8 * rng.unit(), i, j))
        .collect();
    cells.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut used: HashMap<(i32, i32), usize> = HashMap::new();
    let mut unplaced = 0;
    for &(use_, count) in wanted {
        let Some(_) = pick(kit, use_, None) else {
            unplaced += count;
            continue;
        };
        for _ in 0..count {
            let b = pick(kit, use_, Some(rng)).unwrap_or(0);
            let mut placed = false;
            for &(_, i, j) in &cells {
                if used.contains_key(&(i, j)) {
                    continue;
                }
                // Face the street toward the centre (the nearer axis).
                let toward = |v: i32| if v >= 0 { -1.0 } else { 1.0 };
                let dir = if i.abs() > j.abs() {
                    DVec2::new(toward(i), 0.0)
                } else {
                    DVec2::new(0.0, toward(j))
                };
                let yaw = facing(rotate(dir, turn));
                // The building's centre on the plot's centre.
                let c = rotate(kit.buildings[b].centre(), yaw);
                if let Some(k) = plots.try_place(b, plot_at(i, j) - c, yaw) {
                    used.insert((i, j), k);
                    placed = true;
                    break;
                }
                // A plot that didn't fit (steep, wet) stays empty.
                used.insert((i, j), usize::MAX);
            }
            if !placed {
                unplaced += 1;
            }
        }
    }
    // Streets: the grid lines round every built plot (corner nodes).
    let mut nodes = Vec::new();
    let mut index: HashMap<(i32, i32), u32> = HashMap::new();
    let mut node = |nodes: &mut Vec<DVec2>, i: i32, j: i32| {
        *index.entry((i, j)).or_insert_with(|| {
            nodes.push(rotate(DVec2::new(line(i), line(j)), turn));
            (nodes.len() - 1) as u32
        })
    };
    let mut edges = std::collections::BTreeSet::new();
    let mut built: Vec<_> = used
        .iter()
        .filter(|(_, k)| **k != usize::MAX)
        .map(|(c, _)| *c)
        .collect();
    built.sort();
    for (i, j) in built {
        let c = [
            node(&mut nodes, i, j),
            node(&mut nodes, i + 1, j),
            node(&mut nodes, i + 1, j + 1),
            node(&mut nodes, i, j + 1),
        ];
        for k in 0..4 {
            let (a, b) = (c[k], c[(k + 1) % 4]);
            edges.insert((a.min(b), a.max(b)));
        }
    }
    (nodes, edges.into_iter().collect(), unplaced)
}

/// A body's settlements on the ground, made on approach (task 4b.5): the layouts made so far
/// (by settlement index), the kits they use, the natural terrain they are made on, and the
/// foundations levelling the ground under them.
#[derive(bevy_ecs::prelude::Resource, Clone, Debug)]
pub struct RegionLayouts {
    pub kits: Arc<Kits>,
    /// The body's terrain without foundations (layouts read the natural ground).
    pub natural: Arc<Terrain>,
    pub layouts: std::collections::BTreeMap<usize, Arc<Layout>>,
    pub foundations: Arc<Foundations>,
}

impl RegionLayouts {
    /// No layouts yet.
    pub fn new(kits: Arc<Kits>, natural: Arc<Terrain>) -> Self {
        Self {
            kits,
            natural,
            layouts: Default::default(),
            foundations: Arc::new(Foundations::default()),
        }
    }

    /// The layout of settlement `i` of `society` (empty when its culture has no kit yet),
    /// made on the natural ground. Takes milliseconds: run it on a worker.
    pub fn layout_of(society: &RegionSociety, kits: &Kits, natural: &Terrain, i: usize) -> Layout {
        let s = &society.settlements[i];
        let profile = &society.society.cultures[s.culture].profile;
        match kits.0.get(&profile.building_kit) {
            Some(kit) => generate_layout(s, profile.tech_tier, kit, natural),
            None => Layout {
                kit: profile.building_kit.clone(),
                center: s.direction,
                buildings: Vec::new(),
                nodes: Vec::new(),
                streets: Vec::new(),
                unplaced: 0,
            },
        }
    }

    /// Every settlement laid out at once, several at a time on the machine's cores (tests and
    /// tools; the game makes them on approach).
    pub fn generate(society: &RegionSociety, kits: Arc<Kits>, terrain: &Terrain) -> Self {
        let n = society.settlements.len();
        let threads = std::thread::available_parallelism()
            .map_or(4, |n| n.get())
            .min(16);
        let chunk = n.div_ceil(threads).max(1);
        let indices: Vec<usize> = (0..n).collect();
        let layouts: Vec<Layout> = std::thread::scope(|scope| {
            let handles: Vec<_> = indices
                .chunks(chunk)
                .map(|part| {
                    let kits = &kits;
                    scope.spawn(move || {
                        part.iter()
                            .map(|&i| Self::layout_of(society, kits, terrain, i))
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            handles
                .into_iter()
                .flat_map(|h| h.join().expect("layout thread"))
                .collect()
        });
        let mut out = Self::new(kits, Arc::new(terrain.clone()));
        out.layouts = layouts.into_iter().map(Arc::new).enumerate().collect();
        out.rebuild_foundations();
        out
    }

    /// Adds (or replaces) settlement `i`'s layout; call [`Self::rebuild_foundations`] after.
    pub fn insert(&mut self, i: usize, layout: Arc<Layout>) {
        self.layouts.insert(i, layout);
    }

    /// Drops settlement `i`'s layout; call [`Self::rebuild_foundations`] after.
    pub fn remove(&mut self, i: usize) -> Option<Arc<Layout>> {
        self.layouts.remove(&i)
    }

    /// The foundations of the layouts made, each site keyed by its settlement's index.
    pub fn rebuild_foundations(&mut self) {
        let radius = self.natural.radius;
        let sites = self
            .layouts
            .iter()
            .filter(|(_, l)| !l.buildings.is_empty())
            .filter_map(|(&i, l)| Some(l.site(self.kits.0.get(&l.kit)?, radius).with_id(i as u32)))
            .collect();
        self.foundations = Arc::new(Foundations::new(sites));
    }

    /// Buildings placed, over the layouts made.
    pub fn buildings(&self) -> usize {
        self.layouts.values().map(|l| l.buildings.len()).sum()
    }
}

/// Puts an empty [`RegionLayouts`] (no layouts yet) in every region with a society (after
/// [`crate::aggregate::install`]); the game fills it on approach.
pub fn install(sim: &mut Simulation, kits: Arc<Kits>) {
    let Some(system) = sim.star_system().cloned() else {
        return;
    };
    for (i, region) in sim.regions.iter_mut().enumerate() {
        if region.world.get_resource::<RegionSociety>().is_none() {
            continue;
        }
        let Some(terrain) = system.bodies.get(i).and_then(Terrain::of) else {
            continue;
        };
        region
            .world
            .insert_resource(RegionLayouts::new(kits.clone(), Arc::new(terrain)));
    }
}
