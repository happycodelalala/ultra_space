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
    /// The worked land round it (living-planet.md §8d).
    pub fields: Vec<Field>,
}

/// A worked field round a settlement: a rectangle on the tangent plane (centre, half extents
/// across and along, turned `yaw` like a building), its crop (an index the renderer colours)
/// and what bounds it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Field {
    pub x: f64,
    pub z: f64,
    pub half: [f64; 2],
    pub yaw: f64,
    pub crop: u8,
    pub edge: Edge,
}

/// What bounds a field: nothing, a hedge, a fence or a low stone wall.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    Open,
    Hedge,
    Fence,
    Wall,
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
        // Harbours stand over the water: no foundation, no box (the dock and ship would be one).
        let harbour = |p: &Placed| {
            kit.buildings[p.building]
                .uses
                .iter()
                .any(|u| u == "harbour")
        };
        let footprints = self
            .buildings
            .iter()
            .filter(|p| !harbour(p))
            .map(|p| Self::footprint(kit, p))
            .collect();
        let boxes = (0..self.buildings.len())
            .filter(|&i| {
                let b = &kit.buildings[self.buildings[i].building];
                // Gates are passable (their towers are thin enough to leave out).
                !b.uses
                    .iter()
                    .any(|u| u == "tree" || u == "harbour" || u == "gate")
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
    /// Placing a ring of wall: its pieces may overlap each other (they join end to end).
    ring: bool,
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
            ring: false,
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
        self.place_checked(building, at, yaw, true)
    }

    /// As [`Self::try_place`], over the streets when `streets` is false (a gate on a road).
    fn place_checked(
        &mut self,
        building: usize,
        at: DVec2,
        yaw: f64,
        streets: bool,
    ) -> Option<usize> {
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
        let wall = |i: usize| {
            self.kit.buildings[self.placed[i].building]
                .uses
                .iter()
                .any(|u| matches!(u.as_str(), "wall" | "tower" | "gate"))
        };
        if candidates
            .iter()
            .any(|&i| overlaps(&rect, &self.rects[i]) && !(self.ring && wall(i)))
        {
            return None;
        }
        let mut near: Vec<usize> = Self::cells(&rect)
            .filter_map(|c| self.street_grid.get(&c))
            .flatten()
            .copied()
            .collect();
        near.sort_unstable();
        near.dedup();
        if streets
            && near.iter().any(|&i| {
                let (a, b) = self.streets[i];
                segment_hits(&rect, a, b, STREET_HALF - MARGIN)
            })
        {
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

impl Plots<'_> {
    /// Places `building` (a harbour: its back at `at` on the shore, reaching out over the
    /// water in front) clear of other buildings, at the shore's height.
    fn place_on_shore(&mut self, building: usize, at: DVec2, yaw: f64) -> Option<usize> {
        let p = Placed {
            building,
            x: at.x,
            z: at.y,
            yaw,
            height: self.ground(at).max(MIN_HEIGHT),
        };
        let rect = Footprint {
            height: p.height,
            ..Layout::footprint(self.kit, &p)
        };
        let clash = Self::cells(&rect)
            .filter_map(|c| self.grid.get(&c))
            .flatten()
            .any(|&i| overlaps(&rect, &self.rects[i]))
            || Self::cells(&rect)
                .filter_map(|c| self.street_grid.get(&c))
                .flatten()
                .any(|&i| {
                    let (a, b) = self.streets[i];
                    segment_hits(&rect, a, b, STREET_HALF - MARGIN)
                });
        if clash {
            return None;
        }
        let i = self.placed.len();
        for c in Self::cells(&rect) {
            self.grid.entry(c).or_default().push(i);
        }
        self.placed.push(p);
        self.rects.push(rect);
        Some(i)
    }
}

/// How far the placed buildings (not trees or props) reach from the centre (m).
fn outer_reach(plots: &Plots) -> f64 {
    plots
        .placed
        .iter()
        .zip(&plots.rects)
        .filter(|(p, _)| {
            !plots.kit.buildings[p.building]
                .uses
                .iter()
                .any(|u| u == "tree" || u == "prop")
        })
        .map(|(_, r)| DVec2::new(r.x, r.z).length() + DVec2::new(r.half[0], r.half[1]).length())
        .fold(20.0, f64::max)
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

/// Most buildings in one layout: bigger places have bigger buildings (tenements, apartment
/// blocks), not more of them (a metropolis is mostly statistics; living-planet.md §6.4).
const MOST_BUILDINGS: usize = 4_500;

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
    let port = s.has(SettlementRole::Port);
    // A dwelling (task 4b.8): a home of its stage, a barn where it farms, a few trees.
    if s.size == SizeClass::Dwelling {
        let farms = role("farmer") + role("herder") + role("grower") > 0;
        return match tier {
            TechTier::StoneAge => vec![("square", 1), ("home", 2), ("prop", 1), ("tree", 3)],
            TechTier::Spacefaring => vec![("home", 1), ("power", 1), ("prop", 1)],
            _ => vec![
                ("home", 1),
                ("farm", usize::from(farms)),
                ("prop", 1),
                ("tree", 4),
            ],
        };
    }
    let wanted = match tier {
        TechTier::Spacefaring => {
            let colony = s.has(SettlementRole::Capital);
            let port = colony || port;
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
        // Camps: a fire for every few families, huts round them.
        TechTier::StoneAge => {
            let huts = ceil(pop, 5).max(3);
            vec![
                ("square", 1 + pop / 60),
                ("hall", usize::from(pop >= 150)),
                ("shrine", usize::from(pop >= 60)),
                ("smithy", ceil(role("crafter"), 8)),
                ("home", huts),
                ("prop", 2 + pop / 25),
                ("tree", 6 + huts / 4),
            ]
        }
        // Towns on a grid: houses and tenements, shops, factories at the edge.
        TechTier::Industrial | TechTier::Modern => {
            let per_home = if tier == TechTier::Modern { 8 } else { 6 };
            let homes = ceil(pop, per_home).max(3);
            let shops = ceil(role("merchant"), 8).min(homes / 3);
            vec![
                ("square", 1),
                (
                    "hall",
                    usize::from(s.size >= SizeClass::Town) + pop / 50_000,
                ),
                ("power", 1 + pop / 8_000),
                ("shrine", usize::from(s.size >= SizeClass::Town)),
                ("clinic", ceil(role("doctor"), 12)),
                ("shop", shops),
                ("home", homes - shops),
                ("workshop", ceil(role("worker"), 120)),
                ("farm", ceil(role("farmer"), 15)),
                ("prop", 2 + pop / 300),
                ("tree", 6 + homes / 6),
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
                ("port", if port { 1 + pop / 30_000 } else { 0 }),
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
                // Farmers live in town and work the fields; a barn for every few of them.
                (
                    "farm",
                    ((role("farmer") + role("herder")) / 8).min(households / 4),
                ),
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
    };
    // Big places: fewer, bigger buildings (each count scaled down, none to nothing).
    let total: usize = wanted.iter().map(|w| w.1).sum();
    if total <= MOST_BUILDINGS {
        return wanted;
    }
    let k = MOST_BUILDINGS as f64 / total as f64;
    wanted
        .into_iter()
        .map(|(u, n)| {
            (
                u,
                if n == 0 {
                    0
                } else {
                    ((n as f64 * k) as usize).max(1)
                },
            )
        })
        .collect()
}

/// Whether a settlement is walled (living-planet.md §8d): the towns, cities and forts of the
/// walled stages, the Stone Age hill forts (a palisade), the wasteland's enclaves (barricades).
fn walled(s: &Settlement, tier: TechTier) -> bool {
    match tier {
        TechTier::StoneAge => s.size >= SizeClass::Town,
        TechTier::Ancient | TechTier::Medieval | TechTier::EarlyModern => {
            s.size >= SizeClass::Town || s.has(SettlementRole::Fort)
        }
        TechTier::Industrial | TechTier::Modern => s.has(SettlementRole::Enclave),
        _ => false,
    }
}

/// A ring of wall at `radius` round the centre: the kit's wall sections end to end, a tower
/// every sixth (if the kit has one), a gate where a street crosses (or a gap, without one);
/// sections that don't fit (a building, steep or wet ground) leave a gap. `gap`: leave an
/// opening anyway (a ring with no street through it).
fn ring_wall(kit: &Kit, plots: &mut Plots, radius: f64, rng: &mut Rng, gap: bool) {
    let Some(wall) = pick(kit, "wall", None) else {
        return;
    };
    let (tower, gate) = (pick(kit, "tower", None), pick(kit, "gate", None));
    let tau = std::f64::consts::TAU;
    let length = (kit.buildings[wall].max_m[0] - kit.buildings[wall].min_m[0]).max(2.0);
    let n = ((tau * radius / length).ceil() as usize).max(8);
    let start = rng.unit() * tau;
    plots.ring = true;
    for k in 0..n {
        if gap && k < 2 {
            continue;
        }
        let a = start + tau * k as f64 / n as f64;
        let out = DVec2::new(a.cos(), a.sin());
        let at = out * radius;
        let yaw = facing(out);
        // A street through here: a gate over it.
        let crossing = plots
            .streets
            .iter()
            .any(|&(p, q)| segment_distance(at, p, q) < 0.5 * length);
        if crossing {
            if let Some(g) = gate {
                plots.place_checked(g, at, yaw, false);
            }
            continue;
        }
        if k % 6 == 0
            && let Some(t) = tower
            && plots.try_place(t, at, yaw).is_some()
        {
            continue;
        }
        plots.try_place(wall, at, yaw);
    }
    plots.ring = false;
}

/// The radius of a walled town's core: room for its buildings (their footprints, with streets
/// and gaps) round the square; the rest grows outside the walls.
fn core_radius(kit: &Kit, wanted: &[(&'static str, usize)], square: f64) -> f64 {
    let area: f64 = wanted
        .iter()
        .filter(|w| !matches!(w.0, "tree" | "prop"))
        .map(|&(u, n)| {
            let of = kit.with_use(u);
            if of.is_empty() {
                return 0.0;
            }
            let mean = of
                .iter()
                .map(|&b| {
                    let h = kit.buildings[b].half();
                    4.0 * h.x * h.y
                })
                .sum::<f64>()
                / of.len() as f64;
            mean * n as f64
        })
        .sum::<f64>()
        * 2.5;
    (area / std::f64::consts::PI).sqrt().max(square + 30.0)
}

/// The layout of `s` (a culture of `tier`) with `kit` on `terrain`.
pub fn generate_layout(s: &Settlement, tier: TechTier, kit: &Kit, terrain: &Terrain) -> Layout {
    let mut rng = Rng::new(s.layout_seed);
    let mut plots = Plots::new(kit, terrain, s.direction);
    let wanted = demand(s, tier);
    let walls = walled(s, tier);
    let (nodes, streets, unplaced) = match tier {
        _ if s.size == SizeClass::Dwelling
            && !matches!(tier, TechTier::StoneAge | TechTier::Spacefaring) =>
        {
            farmstead(&wanted, kit, &mut plots, &mut rng)
        }
        TechTier::Spacefaring | TechTier::Industrial | TechTier::Modern => {
            grid(s, &wanted, kit, &mut plots, &mut rng, walls)
        }
        TechTier::StoneAge => camp(&wanted, kit, &mut plots, &mut rng, walls),
        _ => village(s, &wanted, kit, &mut plots, &mut rng, walls),
    };
    let fields = fields(s, tier, &plots, &mut rng);
    Layout {
        kit: kit.id.clone(),
        center: s.direction,
        buildings: plots.placed,
        nodes,
        streets,
        unplaced,
        fields,
    }
}

/// Most fields round one settlement.
const MOST_FIELDS: usize = 2_500;

/// The worked land round a settlement (living-planet.md §8d): rings of fields outside its
/// buildings, out to a depth growing with its people, clear of buildings, streets and water;
/// their shape and bounds by its stage (Stone Age garden plots, Ancient squares behind stone
/// walls, medieval strips between hedges, big fenced industrial fields). Some lie fallow.
fn fields(s: &Settlement, tier: TechTier, plots: &Plots, rng: &mut Rng) -> Vec<Field> {
    let (w, l, edge, crops) = match tier {
        TechTier::StoneAge => (12.0, 14.0, Edge::Open, 3),
        TechTier::Ancient => (40.0, 45.0, Edge::Wall, 5),
        TechTier::Medieval => (24.0, 90.0, Edge::Hedge, 5),
        TechTier::EarlyModern => (32.0, 80.0, Edge::Hedge, 5),
        TechTier::Industrial => (70.0, 120.0, Edge::Fence, 5),
        _ => return Vec::new(),
    };
    let pop = f64::from(s.population);
    let depth = if tier == TechTier::StoneAge {
        if pop < 60.0 {
            return Vec::new();
        }
        25.0
    } else {
        60.0 + 4.0 * pop.sqrt()
    };
    let r0 = outer_reach(plots) + 8.0;
    let mut out = Vec::new();
    let tau = std::f64::consts::TAU;
    let mut r = r0 + 0.5 * l;
    while r < r0 + depth + 0.5 * l && out.len() < MOST_FIELDS {
        let n = ((tau * r / (w + 3.0)).floor() as usize).max(4);
        let start = rng.unit() * tau;
        for k in 0..n {
            let a = start + tau * k as f64 / n as f64;
            let out_dir = DVec2::new(a.cos(), a.sin());
            let c = out_dir * r;
            let yaw = facing(out_dir);
            let rect = Footprint {
                x: c.x,
                z: c.y,
                half: [0.5 * w, 0.5 * l],
                yaw,
                height: 0.0,
            };
            let built = Plots::cells(&rect)
                .filter_map(|cell| plots.grid.get(&cell))
                .flatten()
                .any(|&i| overlaps(&rect, &plots.rects[i]));
            let street = Plots::cells(&rect)
                .filter_map(|cell| plots.street_grid.get(&cell))
                .flatten()
                .any(|&i| {
                    let (p, q) = plots.streets[i];
                    segment_hits(&rect, p, q, STREET_HALF)
                });
            if built || street {
                continue;
            }
            // Dry and not too steep (a field may follow a gentle slope).
            let (hx, hz) = (0.5 * w, 0.5 * l);
            let mut lo = f64::MAX;
            let mut hi = f64::MIN;
            for (sx, sz) in [
                (0.0, 0.0),
                (-1.0, -1.0),
                (1.0, -1.0),
                (-1.0, 1.0),
                (1.0, 1.0),
            ] {
                let h = plots.ground(c + rotate(DVec2::new(sx * hx, sz * hz), yaw));
                lo = lo.min(h);
                hi = hi.max(h);
            }
            if lo < MIN_HEIGHT || hi - lo > 0.25 * l {
                continue;
            }
            // Some fallow, some given to grazing (no crop drawn).
            if rng.unit() < 0.12 {
                continue;
            }
            out.push(Field {
                x: c.x,
                z: c.y,
                half: [0.5 * w, 0.5 * l],
                yaw,
                crop: (rng.next_u64() % crops) as u8,
                edge,
            });
            if out.len() >= MOST_FIELDS {
                break;
            }
        }
        r += l + 3.0;
    }
    out
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
    walls: bool,
) -> Graph {
    // Everything that stands along the streets.
    let households = wanted
        .iter()
        .filter(|(u, _)| matches!(*u, "home" | "shop" | "farm" | "mill"))
        .map(|w| w.1)
        .sum::<usize>();
    let (square, mains, length) = match s.size {
        SizeClass::City | SizeClass::Metropolis | SizeClass::Megalopolis => {
            (22.0, 8 + rng.range(0, 2) as usize, 100.0)
        }
        SizeClass::Town => (18.0, 5 + rng.range(0, 1) as usize, 100.0),
        SizeClass::Village => (12.0, 3, 90.0),
        _ => (8.0, 2, 45.0),
    };
    // Longer streets for more households (each street side holds a house every ~10 m).
    let length = length * (households as f64 / 40.0).sqrt().clamp(0.6, 2.2);
    // At least enough street for every household and farm: the kit's homes' mean width plus
    // a gap of frontage each (about 7 m for the medieval cottages), both sides of each street.
    let homes = kit.with_use("home");
    let frontage = if homes.is_empty() {
        7.0
    } else {
        homes
            .iter()
            .map(|&i| 2.0 * kit.buildings[i].half().x + 0.6)
            .sum::<f64>()
            / homes.len() as f64
    }
    .max(7.0);
    // Side lanes (two levels in towns, three in cities) hold a share of the frontage
    // in a big place, so its main streets need not run for kilometres.
    let lanes = match s.size {
        SizeClass::City | SizeClass::Metropolis | SizeClass::Megalopolis => 1.8,
        SizeClass::Town => 1.0,
        _ => 1.0,
    };
    let length = length.max(frontage * (households + 4) as f64 / (mains as f64 * lanes));
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
        let depth = match s.size {
            SizeClass::City | SizeClass::Metropolis | SizeClass::Megalopolis => 3,
            SizeClass::Town => 2,
            _ => 1,
        };
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
    let mut walls = walls;

    for &(use_, count) in wanted {
        // The walls after the square and the harbour (and its street), before the rest: round
        // the core, which houses fill from the square out; the rest grows outside them.
        if walls && !matches!(use_, "square" | "port") {
            walls = false;
            ring_wall(kit, plots, core_radius(kit, wanted, square), rng, false);
        }
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
                // Harbours on the nearest shore, reaching out to sea, with a street to them.
                "port" => {
                    let b = pick(kit, "port", Some(rng));
                    match (b, shore(plots, crate::settlements::SHORE_REACH)) {
                        (Some(b), Some((at, out))) => {
                            let ok = plots.place_on_shore(b, at, facing(out)).is_some();
                            // The street ends short of the harbour's back.
                            let back = kit.buildings[b].min_m[2] - STREET_HALF - MARGIN - 1.0;
                            let at = at + out * back;
                            if ok && at.length() > square + 12.0 {
                                let from = (out * square).clamp_length_max(square);
                                let steps = ((at.length() - square) / 12.0).ceil() as usize;
                                let mut prev = nodes.len();
                                nodes.push(from);
                                for k in 1..=steps {
                                    let q = from.lerp(at, k as f64 / steps as f64);
                                    nodes.push(q);
                                    let i = nodes.len() - 1;
                                    streets.push((prev as u32, i as u32));
                                    plots.add_street(nodes[prev], q);
                                    prev = i;
                                }
                            }
                            ok
                        }
                        _ => false,
                    }
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

/// The camp style (Stone Age): huts in rings round fires, the first fire at the centre and the
/// others round it, the chief's tent by the central fire, the stone circle a little apart, trails
/// from each fire to the centre and one leading away.
fn camp(
    wanted: &[(&'static str, usize)],
    kit: &Kit,
    plots: &mut Plots,
    rng: &mut Rng,
    walls: bool,
) -> Graph {
    let tau = std::f64::consts::TAU;
    let fires = wanted
        .iter()
        .find(|w| w.0 == "square")
        .map_or(1, |w| w.1.max(1));
    // Fire places: the centre, then outwards on a loose spiral.
    let mut centres = vec![DVec2::ZERO];
    let start = rng.unit() * tau;
    for k in 1..fires {
        let r = 30.0 * (k as f64).sqrt() * (0.85 + 0.3 * rng.unit());
        let a = start + k as f64 * 2.4 + 0.3 * (rng.unit() - 0.5);
        let p = DVec2::new(a.cos(), a.sin()) * r;
        if plots.ground(p) >= MIN_HEIGHT + 0.5 {
            centres.push(p);
        }
    }
    let mut unplaced = 0;
    // The fires first (moved a little where the ground is rough); those that won't go are left.
    let fire = pick(kit, "square", None);
    let mut lit = Vec::new();
    for &c in &centres {
        let placed = fire.and_then(|b| {
            (0..12).find_map(|t| {
                let a = rng.unit() * tau;
                let p = c + DVec2::new(a.cos(), a.sin()) * (2.0 * t as f64);
                plots.try_place(b, p, rng.unit() * tau).map(|_| p)
            })
        });
        match placed {
            Some(p) => lit.push(p),
            None => unplaced += 1,
        }
    }
    let centres = if lit.is_empty() {
        vec![DVec2::ZERO]
    } else {
        lit
    };
    // Trails from each fire's edge to the first fire's, and one leading away (towards water or
    // the next camp; anywhere dry).
    // Clear of the fire's footprint (about 15 m across its corners) and the trail's width.
    let edge = 13.0;
    let mut nodes = Vec::new();
    let mut streets = Vec::new();
    let mut trail = |nodes: &mut Vec<DVec2>, plots: &mut Plots, a: DVec2, b: DVec2| -> bool {
        let d = (b - a).normalize_or(DVec2::X);
        let (p, q) = (a + d * edge, b - d * edge);
        // Not through another fire.
        if (q - p).dot(d) <= 0.0
            || plots.ground(q) < MIN_HEIGHT
            || plots
                .rects
                .iter()
                .any(|r| segment_hits(r, p, q, STREET_HALF))
        {
            return false;
        }
        nodes.extend([p, q]);
        let n = nodes.len() as u32;
        streets.push((n - 2, n - 1));
        plots.add_street(p, q);
        true
    };
    // Each fire to the nearest earlier one.
    for k in 1..centres.len() {
        let near = (0..k)
            .min_by(|&a, &b| {
                centres[a]
                    .distance(centres[k])
                    .total_cmp(&centres[b].distance(centres[k]))
            })
            .unwrap_or(0);
        trail(&mut nodes, plots, centres[near], centres[k]);
    }
    let reach = centres.iter().map(|c| c.length()).fold(0.0, f64::max) + 40.0;
    let start = rng.unit() * tau;
    for k in 0..8 {
        let away = start + tau * k as f64 / 8.0;
        if trail(
            &mut nodes,
            plots,
            centres[0],
            centres[0] + DVec2::new(away.cos(), away.sin()) * reach,
        ) {
            break;
        }
    }
    let mut ring_of = vec![0usize; centres.len()];
    let mut grove: Vec<DVec2> = Vec::new();
    for &(use_, count) in wanted {
        if use_ == "square" {
            continue; // the fires, above
        }
        // A hill fort's palisade round the huts, before the trees.
        if walls && use_ == "tree" {
            let r = outer_reach(plots) + 8.0;
            ring_wall(kit, plots, r, rng, false);
        }
        for n in 0..count {
            let Some(b) = pick(kit, use_, Some(rng)) else {
                continue;
            };
            let ok = match use_ {
                // Round a fire (homes in turn round each), facing it.
                "home" | "hall" | "smithy" | "prop" => {
                    let k = if use_ == "hall" { 0 } else { n % centres.len() };
                    let c = centres[k];
                    (0..80).any(|t| {
                        let ring = ring_of[k] + t / 10;
                        let near = match use_ {
                            "hall" => 14.0,
                            "smithy" | "prop" => 11.0,
                            _ => 12.0,
                        };
                        let r = near + 5.5 * ring as f64 + 2.0 * rng.unit();
                        let a = rng.unit() * tau;
                        let p = c + DVec2::new(a.cos(), a.sin()) * r;
                        let placed = plots.try_place(b, p, facing(c - p)).is_some();
                        if placed && t >= 10 && use_ == "home" {
                            ring_of[k] = ring;
                        }
                        placed
                    })
                }
                // The stone circle a little apart; trees in groves among and round the camp (most
                // start next to an earlier tree).
                _ => {
                    let edge = reach - 25.0;
                    (0..40).any(|_| {
                        let a = rng.unit() * tau;
                        let p = if use_ == "shrine" {
                            DVec2::new(a.cos(), a.sin()) * (edge + 15.0)
                        } else if !grove.is_empty() && rng.unit() < 0.7 {
                            *rng.pick(&grove)
                                + DVec2::new(a.cos(), a.sin()) * (5.0 + 7.0 * rng.unit())
                        } else {
                            DVec2::new(a.cos(), a.sin()) * (reach * 1.3 * rng.unit().sqrt())
                        };
                        let placed = plots.try_place(b, p, rng.unit() * tau).is_some();
                        if placed && use_ == "tree" {
                            grove.push(p);
                        }
                        placed
                    })
                }
            };
            if !ok {
                unplaced += 1;
            }
        }
    }
    (nodes, streets, unplaced)
}

/// A dwelling's farmstead: the home near the centre (moved a little where the ground is rough),
/// its barn beside it, a prop by the door, trees round about, and a track leading off.
fn farmstead(
    wanted: &[(&'static str, usize)],
    kit: &Kit,
    plots: &mut Plots,
    rng: &mut Rng,
) -> Graph {
    let tau = std::f64::consts::TAU;
    let count = |u: &str| wanted.iter().find(|w| w.0 == u).map_or(0, |w| w.1);
    let mut unplaced = 0;
    let yaw = rng.unit() * tau;
    let front = rotate(DVec2::Y, yaw);
    // The track: out from the front, so nothing stands on it.
    let (a, b) = (front * 8.0, front * 45.0);
    plots.add_street(a, b);
    let mut home_at = None;
    if let Some(home) = pick(kit, "home", Some(rng)) {
        let back = kit.buildings[home].max_m[2] + STREET_HALF + 1.0;
        home_at = (0..16).find_map(|t| {
            let a = rng.unit() * tau;
            let p = DVec2::new(a.cos(), a.sin()) * (2.5 * t as f64) - front * (back - 8.0);
            plots.try_place(home, p, yaw).map(|_| p)
        });
    }
    let Some(home) = home_at else {
        let total: usize = wanted.iter().map(|w| w.1).sum();
        return (vec![a, b], vec![(0, 1)], total);
    };
    let side = rotate(DVec2::X, yaw);
    for (use_, n) in [("farm", count("farm")), ("prop", count("prop"))] {
        for _ in 0..n {
            let Some(bld) = pick(kit, use_, Some(rng)) else {
                continue;
            };
            let s = if rng.unit() < 0.5 { 1.0 } else { -1.0 };
            let placed = (0..12).any(|t| {
                let d = 12.0 + 3.0 * t as f64;
                let p = home + side * s * d - front * (4.0 * rng.unit());
                plots.try_place(bld, p, yaw + s * 0.2).is_some()
            });
            if !placed {
                unplaced += 1;
            }
        }
    }
    let mut grove: Vec<DVec2> = Vec::new();
    for _ in 0..count("tree") {
        let Some(tree) = pick(kit, "tree", Some(rng)) else {
            break;
        };
        let placed = (0..20).any(|_| {
            let a = rng.unit() * tau;
            let p = match grove.last() {
                Some(&g) if rng.unit() < 0.6 => {
                    g + DVec2::new(a.cos(), a.sin()) * (5.0 + 5.0 * rng.unit())
                }
                _ => home + DVec2::new(a.cos(), a.sin()) * (18.0 + 20.0 * rng.unit()),
            };
            let ok = plots.try_place(tree, p, rng.unit() * tau).is_some();
            if ok {
                grove.push(p);
            }
            ok
        });
        if !placed {
            unplaced += 1;
        }
    }
    (vec![a, b], vec![(0, 1)], unplaced)
}

/// Where the land meets the sea nearest the centre: the last dry point (tangent plane, m) and
/// the way out to sea, if within `most` m.
fn shore(plots: &Plots, most: f64) -> Option<(DVec2, DVec2)> {
    let mut best: Option<(f64, DVec2, DVec2)> = None;
    for k in 0..24 {
        let a = std::f64::consts::TAU * k as f64 / 24.0;
        let dir = DVec2::new(a.cos(), a.sin());
        let mut last = DVec2::ZERO;
        let mut r = 20.0;
        while r <= most && best.is_none_or(|b| r < b.0) {
            let p = dir * r;
            if plots.ground(p) < 0.0 {
                best = Some((r, last, dir));
                break;
            }
            last = p;
            r += 15.0;
        }
    }
    best.map(|(_, p, d)| (p, d))
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

/// Width of the streets between blocks of plots (m): the street and a margin each side.
const AVENUE: f64 = 2.0 * (STREET_HALF + MARGIN) + 2.0;

/// The grid style: the colony's blocks round its pads, the industrial and modern towns' blocks
/// round their squares (the first uses wanted at the centre, the last at the edge). Blocks of
/// 3 × 3 plots with streets round the blocks; a plot is sized for most of the kit's buildings,
/// and a bigger one (a factory, a pad) takes a whole block.
fn grid(
    _s: &Settlement,
    wanted: &[(&'static str, usize)],
    kit: &Kit,
    plots: &mut Plots,
    rng: &mut Rng,
    walls: bool,
) -> Graph {
    let size = |b: usize| kit.buildings[b].half().max_element() * 2.0;
    // Plot pitch: three in four of the kit's buildings (not trees or props) fit one plot.
    let mut sizes: Vec<f64> = (0..kit.buildings.len())
        .filter(|&b| {
            !kit.buildings[b]
                .uses
                .iter()
                .any(|u| u == "tree" || u == "prop")
        })
        .map(size)
        .collect();
    sizes.sort_by(f64::total_cmp);
    let pitch = sizes
        .get(sizes.len() * 3 / 4)
        .copied()
        .unwrap_or(10.0)
        .max(8.0)
        + 2.0 * MARGIN
        + 1.0;
    let span = 3.0 * pitch;
    let total: usize = wanted.iter().map(|w| w.1).sum();
    // Room to spare (plots on steep or wet ground are passed over): blocks per side.
    let side = ((total as f64 * 4.0).sqrt() / 3.0).ceil() as i32 + 3;
    let half = side / 2;
    let turn = (rng.unit() - 0.5) * std::f64::consts::FRAC_PI_2;
    let origin = |b: i32| f64::from(b) * (span + AVENUE);
    let block = |i: i32| (i + 1).div_euclid(3);
    let local = |i: i32| (i + 1).rem_euclid(3) - 1;
    let unturned = |i: i32, j: i32| {
        DVec2::new(
            origin(block(i)) + f64::from(local(i)) * pitch,
            origin(block(j)) + f64::from(local(j)) * pitch,
        )
    };
    let plot_at = |i: i32, j: i32| rotate(unturned(i, j), turn);
    // The street line before block b (between blocks b - 1 and b).
    let line = |b: i32| origin(b) - 0.5 * span - 0.5 * AVENUE;
    let reach = 3 * half + 1;
    let mut cells: Vec<(f64, i32, i32)> = (-reach..=reach)
        .flat_map(|j| (-reach..=reach).map(move |i| (i, j)))
        .map(|(i, j)| (unturned(i, j).length() / pitch + 0.8 * rng.unit(), i, j))
        .collect();
    cells.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut blocks: Vec<(f64, i32, i32)> = (-half..=half)
        .flat_map(|j| (-half..=half).map(move |i| (i, j)))
        .map(|(i, j)| ((i * i + j * j) as f64 + 0.5 * rng.unit(), i, j))
        .collect();
    blocks.sort_by(|a, b| a.0.total_cmp(&b.0));
    // Which plots are taken, and by which building (index into the placed).
    let mut used: HashMap<(i32, i32), usize> = HashMap::new();
    let mut unplaced = 0;
    // Plots are taken nearest first; `next` is the first not yet taken.
    let mut next = 0;
    // The smallest building that failed on a plot or a block (steep, wet): smaller ones may
    // still fit.
    let mut failed: HashMap<(i32, i32), f64> = HashMap::new();
    let mut failed_block: HashMap<(i32, i32), f64> = HashMap::new();
    // Facing the street: the plot's side of its block (the middle plot toward the town's
    // centre).
    let facing_of = |i: i32, j: i32| {
        let (li, lj) = (local(i), local(j));
        let dir = if li == 0 && lj == 0 {
            let toward = |v: i32| if v >= 0 { -1.0 } else { 1.0 };
            if block(i).abs() > block(j).abs() {
                DVec2::new(toward(block(i)), 0.0)
            } else {
                DVec2::new(0.0, toward(block(j)))
            }
        } else if li.abs() >= lj.abs() {
            DVec2::new(f64::from(li), 0.0)
        } else {
            DVec2::new(0.0, f64::from(lj))
        };
        facing(rotate(dir, turn))
    };
    for &(use_, count) in wanted {
        // A use the kit has no building for is passed over (as in the village style).
        if pick(kit, use_, None).is_none() {
            continue;
        }
        // An enclave's barricade round the built, before the trees and props (an opening
        // left: no street crosses it).
        if walls && use_ == "prop" {
            // Outside the streets round the built blocks (their farthest corner, and a street).
            let r = used
                .keys()
                .flat_map(|&(i, j)| {
                    let (bi, bj) = (block(i), block(j));
                    [(bi, bj), (bi + 1, bj), (bi, bj + 1), (bi + 1, bj + 1)]
                })
                .map(|(bi, bj)| DVec2::new(line(bi), line(bj)).length())
                .fold(outer_reach(plots), f64::max)
                + AVENUE;
            ring_wall(kit, plots, r, rng, true);
        }
        // Trees and props: in spare plots near the built, without taking them.
        if matches!(use_, "tree" | "prop") {
            let spare: Vec<(i32, i32)> = cells
                .iter()
                .map(|c| (c.1, c.2))
                .filter(|c| !used.contains_key(c))
                .take(4 * count + 16)
                .collect();
            for _ in 0..count {
                let b = pick(kit, use_, Some(rng)).unwrap_or(0);
                let mut placed = false;
                for _ in 0..12 {
                    if spare.is_empty() {
                        break;
                    }
                    let k = rng.range(0, spare.len() as u32 - 1) as usize;
                    let (i, j) = spare[k];
                    let jitter = DVec2::new(rng.unit() - 0.5, rng.unit() - 0.5) * (0.5 * pitch);
                    let at = plot_at(i, j) + rotate(jitter, turn);
                    if plots
                        .try_place(b, at, rng.unit() * std::f64::consts::TAU)
                        .is_some()
                    {
                        placed = true;
                        break;
                    }
                }
                if !placed {
                    unplaced += 1;
                }
            }
            continue;
        }
        // The smallest building of this use, tried where the chosen one fits nowhere.
        let smallest = kit
            .with_use(use_)
            .into_iter()
            .min_by(|&x, &y| size(x).total_cmp(&size(y)))
            .unwrap_or(0);
        for _ in 0..count {
            let chosen = pick(kit, use_, Some(rng)).unwrap_or(0);
            let mut placed = false;
            for b in [chosen, smallest] {
                if placed {
                    break;
                }
                let sz = size(b);
                if sz > pitch - 2.0 * MARGIN {
                    // A big building: a whole free block, at its middle.
                    for &(_, bi, bj) in &blocks {
                        let plots_of = (-1..=1)
                            .flat_map(|dj| (-1..=1).map(move |di| (3 * bi + di, 3 * bj + dj)));
                        if plots_of.clone().any(|c| used.contains_key(&c))
                            || failed_block.get(&(bi, bj)).is_some_and(|&f| sz >= f)
                        {
                            continue;
                        }
                        let (i, j) = (3 * bi, 3 * bj);
                        let yaw = facing_of(i, j);
                        let c = rotate(kit.buildings[b].centre(), yaw);
                        if let Some(k) = plots.try_place(b, plot_at(i, j) - c, yaw) {
                            for c in plots_of {
                                used.insert(c, k);
                            }
                            placed = true;
                            break;
                        }
                        failed_block.insert((bi, bj), sz);
                    }
                } else {
                    let mut k = next;
                    while k < cells.len() {
                        let (_, i, j) = cells[k];
                        k += 1;
                        if used.contains_key(&(i, j))
                            || failed.get(&(i, j)).is_some_and(|&f| sz >= f)
                        {
                            continue;
                        }
                        let yaw = facing_of(i, j);
                        // The building's centre on the plot's centre.
                        let c = rotate(kit.buildings[b].centre(), yaw);
                        if let Some(k) = plots.try_place(b, plot_at(i, j) - c, yaw) {
                            used.insert((i, j), k);
                            placed = true;
                            break;
                        }
                        failed.insert((i, j), sz);
                    }
                }
                if chosen == smallest {
                    break;
                }
            }
            while next < cells.len() && used.contains_key(&(cells[next].1, cells[next].2)) {
                next += 1;
            }
            if !placed {
                unplaced += 1;
            }
        }
    }
    // Streets: round every block with something built (corner nodes).
    let mut nodes = Vec::new();
    let mut index: HashMap<(i32, i32), u32> = HashMap::new();
    let mut node = |nodes: &mut Vec<DVec2>, bi: i32, bj: i32| {
        *index.entry((bi, bj)).or_insert_with(|| {
            nodes.push(rotate(DVec2::new(line(bi), line(bj)), turn));
            (nodes.len() - 1) as u32
        })
    };
    let mut built: Vec<(i32, i32)> = used.keys().map(|&(i, j)| (block(i), block(j))).collect();
    built.sort();
    built.dedup();
    let mut edges = std::collections::BTreeSet::new();
    for (bi, bj) in built {
        let c = [
            node(&mut nodes, bi, bj),
            node(&mut nodes, bi + 1, bj),
            node(&mut nodes, bi + 1, bj + 1),
            node(&mut nodes, bi, bj + 1),
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
                fields: Vec::new(),
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
