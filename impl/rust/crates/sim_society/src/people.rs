//! People with schedules (task 4.8; living-planet.md §6): when the player comes near a
//! settlement, some of its people are promoted from its counts by role into a [`Crowd`] that
//! lives out the day on the settlement's tangent plane (x east, z south), and are demoted (the
//! crowd dropped) when the player leaves. The same settlement gives the same people.
//!
//! - **Who:** at most [`MAX_PEOPLE`], in the settlement's role shares, each with a name from the
//!   culture's naming rules, a home (a dwelling of the layout) and a workplace for their role.
//! - **When:** a schedule by local solar time with a few minutes' personal offset: asleep at
//!   home at night, work in the morning and afternoon, a meal at home at midday and in the
//!   evening, then the square (or the colony's centre) until bedtime.
//! - **How they get there:** along the street graph (shortest way), at walking pace, from
//!   their door to the destination's; inside a building they are hidden.
//! - **Keeping clear:** walkers step aside from each other and stop for the player.
//! - **Level of detail:** people far from the player are stepped less often.

use std::collections::{BinaryHeap, HashMap};

use glam::DVec2;

use crate::layout::{Kit, Layout};
use crate::lore::NamingRules;
use crate::names;
use crate::rng::Rng;
use crate::settlements::Settlement;

/// Most people simulated in detail per settlement.
pub const MAX_PEOPLE: usize = 64;
/// Walking pace (m/s).
pub const WALK_SPEED: f64 = 1.35;
/// People this far from the player (m) are stepped a quarter as often.
const FAR: f64 = 120.0;
/// How close (m) walkers let each other come before stepping aside, and the player.
const PERSONAL: f64 = 0.9;
const PLAYER: f64 = 1.6;

/// What a person is doing; the game picks an animation for each.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Activity {
    /// Indoors (asleep, eating, at an indoor job): not shown.
    Inside,
    Walking,
    /// At their workplace outdoors.
    Working,
    /// At the square, or waiting about.
    Idle,
    Talking,
}

/// Where a part of the day is spent.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Place {
    Home,
    Work,
    Square,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Person {
    pub name: String,
    /// The culture's role id.
    pub role: String,
    /// Layout building indices of their home and workplace.
    pub home: usize,
    pub work: usize,
    /// Works indoors (hidden while at work) or out of doors by the building.
    pub works_inside: bool,
    /// Their own shift of the schedule (fraction of a day, about ±25 minutes).
    offset: f64,
    /// Where they stand at the square (tangent plane).
    gathering: DVec2,
    // Now.
    pub position: DVec2,
    /// Facing (radians, the yaw that turns +Z: as the layout's buildings).
    pub heading: f64,
    pub activity: Activity,
    place: Option<Place>,
    path: Vec<DVec2>,
    /// Seconds of stepping saved up (far people step in larger, rarer steps).
    pending: f64,
}

/// A settlement's promoted people.
#[derive(Clone, Debug)]
pub struct Crowd {
    pub people: Vec<Person>,
    graph: Graph,
    /// Door (tangent plane, just outside) of every building.
    doors: Vec<DVec2>,
    ticks: u64,
}

/// The street graph, for shortest ways.
#[derive(Clone, Debug)]
struct Graph {
    nodes: Vec<DVec2>,
    links: Vec<Vec<(usize, f64)>>,
    /// Nodes on some street.
    used: Vec<usize>,
}

impl Graph {
    fn new(layout: &Layout) -> Self {
        let mut links = vec![Vec::new(); layout.nodes.len()];
        for &(a, b) in &layout.streets {
            let (a, b) = (a as usize, b as usize);
            let d = layout.nodes[a].distance(layout.nodes[b]);
            links[a].push((b, d));
            links[b].push((a, d));
        }
        let used = (0..layout.nodes.len())
            .filter(|&n| !links[n].is_empty())
            .collect();
        Self {
            nodes: layout.nodes.clone(),
            links,
            used,
        }
    }

    fn nearest(&self, p: DVec2) -> Option<usize> {
        self.used.iter().copied().min_by(|&a, &b| {
            self.nodes[a]
                .distance_squared(p)
                .total_cmp(&self.nodes[b].distance_squared(p))
        })
    }

    /// The way from `from` to `to`: via the nearest street nodes, along the shortest street
    /// path, straight across when there are no streets or it's shorter.
    fn route(&self, from: DVec2, to: DVec2) -> Vec<DVec2> {
        let (Some(a), Some(b)) = (self.nearest(from), self.nearest(to)) else {
            return vec![to];
        };
        // Close by: straight across (further, the streets keep people out of the houses).
        if from.distance(to) < 12.0 || a == b {
            return vec![self.nodes[a], to];
        }
        // Dijkstra.
        #[derive(PartialEq)]
        struct Item(f64, usize);
        impl Eq for Item {}
        impl Ord for Item {
            fn cmp(&self, o: &Self) -> std::cmp::Ordering {
                o.0.total_cmp(&self.0)
            }
        }
        impl PartialOrd for Item {
            fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
                Some(self.cmp(o))
            }
        }
        let mut dist: HashMap<usize, f64> = HashMap::new();
        let mut prev: HashMap<usize, usize> = HashMap::new();
        let mut heap = BinaryHeap::new();
        dist.insert(a, 0.0);
        heap.push(Item(0.0, a));
        while let Some(Item(d, n)) = heap.pop() {
            if n == b {
                break;
            }
            if d > dist.get(&n).copied().unwrap_or(f64::MAX) {
                continue;
            }
            for &(m, w) in &self.links[n] {
                let nd = d + w;
                if nd < dist.get(&m).copied().unwrap_or(f64::MAX) {
                    dist.insert(m, nd);
                    prev.insert(m, n);
                    heap.push(Item(nd, m));
                }
            }
        }
        if !dist.contains_key(&b) {
            return vec![to];
        }
        let mut way = vec![to, self.nodes[b]];
        let mut n = b;
        while let Some(&p) = prev.get(&n) {
            way.push(self.nodes[p]);
            n = p;
        }
        way.reverse();
        way
    }
}

/// The building uses where a role works (in order of preference) and whether indoors.
fn workplace(role: &str) -> (&'static [&'static str], bool) {
    match role {
        "farmer" | "herder" => (&["farm", "mill"], false),
        "fisher" => (&["mill", "farm"], false),
        "smith" => (&["smithy"], false),
        "carpenter" | "laborer" => (&["smithy", "farm", "mill", "workshop", "hangar"], false),
        "merchant" | "trader" => (&["market", "shop", "inn", "port"], false),
        "guard" => (&["hall", "square"], false),
        "priest" => (&["shrine"], true),
        "noble" | "administrator" => (&["hall"], true),
        "healer" | "medic" => (&["shrine", "hall"], true),
        "grower" => (&["farm"], true),
        "technician" | "engineer" => (&["workshop", "hangar", "power"], false),
        "scientist" => (&["lab"], true),
        "pilot" => (&["pad", "hangar"], false),
        "security" => (&["comms", "hall", "pad"], false),
        _ => (&["square"], false),
    }
}

/// The settlement's door spot of a building: just outside the middle of its front (+Z).
fn door(kit: &Kit, layout: &Layout, i: usize) -> DVec2 {
    let p = &layout.buildings[i];
    let b = &kit.buildings[p.building];
    let local = DVec2::new(0.5 * (b.min_m[0] + b.max_m[0]), b.max_m[2] + 1.2);
    let (s, c) = p.yaw.sin_cos();
    DVec2::new(
        p.x + c * local.x + s * local.y,
        p.z - s * local.x + c * local.y,
    )
}

impl Crowd {
    /// Promotes some of settlement `s`'s people (`count`, at most [`MAX_PEOPLE`]) into a crowd
    /// on its `layout` (with `kit`), named by `naming`.
    pub fn promote(
        s: &Settlement,
        layout: &Layout,
        kit: &Kit,
        naming: &NamingRules,
        count: usize,
    ) -> Self {
        let count = count.min(MAX_PEOPLE);
        let mut rng = Rng::derive(s.layout_seed, "people");
        let doors: Vec<DVec2> = (0..layout.buildings.len())
            .map(|i| door(kit, layout, i))
            .collect();
        let with_use = |u: &str| -> Vec<usize> {
            (0..layout.buildings.len())
                .filter(|&i| {
                    kit.buildings[layout.buildings[i].building]
                        .uses
                        .iter()
                        .any(|x| x == u)
                })
                .collect()
        };
        // Homes among the dwellings nearest the centre (the part of a big town the player is
        // in), twice as many as people.
        let mut homes = with_use("home");
        homes.extend(with_use("shop"));
        homes.sort_by(|&a, &b| {
            let d = |i: usize| DVec2::new(layout.buildings[i].x, layout.buildings[i].z).length();
            d(a).total_cmp(&d(b))
        });
        homes.truncate((2 * count).max(1));
        // The square: the well (medieval) or the middle of the colony.
        let centre = with_use("square").first().map_or(DVec2::ZERO, |&i| {
            DVec2::new(layout.buildings[i].x, layout.buildings[i].z)
        });
        // People in the role shares (largest remainders).
        let total: u32 = s.roles.iter().map(|r| r.1).sum::<u32>().max(1);
        let mut roles: Vec<String> = Vec::new();
        let mut shares: Vec<(f64, &str)> = s
            .roles
            .iter()
            .map(|(r, n)| (f64::from(*n) / f64::from(total) * count as f64, r.as_str()))
            .collect();
        for (want, r) in &shares {
            for _ in 0..want.floor() as usize {
                roles.push((*r).to_string());
            }
        }
        shares.sort_by(|a, b| b.0.fract().total_cmp(&a.0.fract()));
        for (_, r) in shares.iter().take(count.saturating_sub(roles.len())) {
            roles.push((*r).to_string());
        }
        let graph = Graph::new(layout);
        let mut people = Vec::with_capacity(count);
        for role in roles {
            let home = if homes.is_empty() {
                0
            } else {
                *rng.pick(&homes)
            };
            let (uses, inside) = workplace(&role);
            let work = uses
                .iter()
                .map(|u| with_use(u))
                .find(|v| !v.is_empty())
                .map_or(home, |v| *rng.pick(&v));
            let a = rng.unit() * std::f64::consts::TAU;
            let gathering = centre + DVec2::new(a.cos(), a.sin()) * (5.0 + 8.0 * rng.unit());
            let position = doors.get(home).copied().unwrap_or(centre);
            people.push(Person {
                name: names::person(naming, &mut rng),
                role,
                home,
                work,
                works_inside: inside,
                offset: (rng.unit() - 0.5) * 0.035,
                gathering,
                position,
                heading: rng.unit() * std::f64::consts::TAU,
                activity: Activity::Inside,
                place: None,
                path: Vec::new(),
                pending: 0.0,
            });
        }
        Self {
            people,
            graph,
            doors,
            ticks: 0,
        }
    }

    /// Puts everyone where their day has them at local time `day` (on promotion: people
    /// are found about their business, not all setting out from home).
    pub fn settle(&mut self, day: f64) {
        for (i, p) in self.people.iter_mut().enumerate() {
            let place = schedule((day + p.offset).rem_euclid(1.0));
            let (at, activity) = match place {
                Place::Home => (self.doors.get(p.home).copied(), Activity::Inside),
                Place::Work if p.works_inside => {
                    (self.doors.get(p.work).copied(), Activity::Inside)
                }
                Place::Work => (self.doors.get(p.work).copied(), Activity::Working),
                Place::Square => (
                    Some(p.gathering),
                    if i % 3 == 0 {
                        Activity::Idle
                    } else {
                        Activity::Talking
                    },
                ),
            };
            p.position = at.unwrap_or(p.position);
            p.place = Some(place);
            p.path.clear();
            p.activity = activity;
        }
    }

    /// Advances everyone by `dt` seconds at local solar time `day` (fraction of the day, 0
    /// midnight), with the player at `player` (tangent plane) if near.
    pub fn step(&mut self, dt: f64, day: f64, player: Option<DVec2>) {
        self.ticks += 1;
        let tick = self.ticks;
        // Destinations by the clock.
        for i in 0..self.people.len() {
            let p = &self.people[i];
            let want = schedule((day + p.offset).rem_euclid(1.0));
            if p.place != Some(want) {
                let target = match want {
                    Place::Home => self.doors.get(p.home).copied(),
                    Place::Work => self.doors.get(p.work).copied(),
                    Place::Square => Some(p.gathering),
                }
                .unwrap_or(p.position);
                let from = p.position;
                let path = self.graph.route(from, target);
                let p = &mut self.people[i];
                p.place = Some(want);
                p.path = path;
                p.activity = Activity::Walking;
            }
        }
        // Moving, keeping clear of each other and the player (people far from the player step
        // every fourth call, by the time saved up).
        let positions: Vec<DVec2> = self.people.iter().map(|p| p.position).collect();
        for (i, p) in self.people.iter_mut().enumerate() {
            p.pending += dt;
            let far = player.is_none_or(|q| q.distance(p.position) > FAR);
            if far && !(tick + i as u64).is_multiple_of(4) {
                continue;
            }
            let dt = std::mem::take(&mut p.pending);
            if p.path.is_empty() {
                if p.activity == Activity::Walking {
                    p.activity = match p.place {
                        Some(Place::Home) => Activity::Inside,
                        Some(Place::Work) if p.works_inside => Activity::Inside,
                        Some(Place::Work) => Activity::Working,
                        _ => {
                            if i % 3 == 0 {
                                Activity::Idle
                            } else {
                                Activity::Talking
                            }
                        }
                    };
                }
                continue;
            }
            // The player in the way: wait.
            if let Some(q) = player
                && q.distance(p.position) < PLAYER
                && (q - p.position).dot(p.path[0] - p.position) > 0.0
            {
                p.activity = Activity::Idle;
                continue;
            }
            p.activity = Activity::Walking;
            let mut step = (p.path[0] - p.position).clamp_length_max(WALK_SPEED * dt);
            // Another walker close ahead: step to the right a little.
            for (j, &o) in positions.iter().enumerate() {
                if j != i && o.distance(p.position) < PERSONAL && (o - p.position).dot(step) > 0.0 {
                    let right = DVec2::new(-step.y, step.x).normalize_or_zero();
                    step += right * step.length() * 0.5;
                }
            }
            if step.length_squared() > 1e-9 {
                p.heading = step.x.atan2(step.y);
            }
            p.position += step;
            if p.position.distance(p.path[0]) < 0.6 {
                p.path.remove(0);
            }
        }
    }
}

/// Where the day is spent at time `day` (fraction, 0 midnight).
fn schedule(day: f64) -> Place {
    let h = day * 24.0;
    match h {
        h if !(6.0..22.0).contains(&h) => Place::Home, // asleep
        h if h < 7.5 => Place::Home,                   // breakfast
        h if h < 12.0 => Place::Work,
        h if h < 13.0 => Place::Home, // midday meal
        h if h < 17.5 => Place::Work,
        h if h < 19.0 => Place::Home, // supper
        _ => Place::Square,           // the evening at the square
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_day_runs_home_work_home_work_home_square_home() {
        let at = |h: f64| schedule(h / 24.0);
        assert_eq!(at(3.0), Place::Home);
        assert_eq!(at(9.0), Place::Work);
        assert_eq!(at(12.5), Place::Home);
        assert_eq!(at(15.0), Place::Work);
        assert_eq!(at(18.0), Place::Home);
        assert_eq!(at(20.0), Place::Square);
        assert_eq!(at(23.0), Place::Home);
    }
}
