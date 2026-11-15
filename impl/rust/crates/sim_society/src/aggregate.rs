//! The aggregate simulation (living-planet.md §4): every settlement, all the time, cheaply.
//! Each region (body) keeps its society in a [`RegionSociety`] resource and steps it every tick
//! by the tick's length in game days, so it runs in parallel with other regions and at any time
//! warp.
//!
//! - **Food and goods:** roles produce per person per day (farmers, herders, fishers and
//!   growers food; smiths, wrights, technicians and engineers goods), everyone eats a day's food
//!   and uses a little goods; stocks spoil slowly and are capped.
//! - **Mood** follows the food and goods in store.
//! - **Population** grows about 1 % a year when content, falls in a famine; growth and loss
//!   follow the culture's role shares, so a settlement keeps the roles it needs.
//! - **Relations** between factions drift toward a target: the generated starting value, less
//!   friction between factions whose seats are near (more across cultures), plus a slow
//!   monthly swing.

use std::sync::Arc;

use ai_gateway::ContentDirector;
use bevy_ecs::prelude::*;
use serde::{Deserialize, Serialize};
use sim_core::objects::{RegionStage, SimClock};
use sim_core::time::GAME_TIME_SCALE;
use sim_core::{Simulation, StarSystem};

use crate::lore::TechTier;
use crate::patterns::SettlementPatterns;
use crate::rng::Rng;
use crate::settlements::{Settlement, generate_settlements};
use crate::society::{Society, generate};

/// Food a person in this role produces per game day (a person eats 1).
fn food_per_day(role: &str) -> f64 {
    match role {
        "farmer" => 3.0,
        "grower" => 7.5, // hydroponics: a colony grows most of its food in a few hands
        "herder" | "fisher" => 2.4,
        "hunter" | "gatherer" => 1.8,
        _ => 0.0,
    }
}

/// Goods a person in this role produces per game day (a person uses [`GOODS_USE`]).
fn goods_per_day(role: &str) -> f64 {
    match role {
        "smith" | "carpenter" | "crafter" | "potter" | "engineer" => 3.0,
        "technician" | "worker" => 2.0,
        "laborer" => 0.5,
        _ => 0.0,
    }
}

const GOODS_USE: f64 = 0.3;
/// Share of the stocks lost per day (food spoils, goods wear).
const FOOD_SPOIL: f64 = 0.005;
const GOODS_WEAR: f64 = 0.001;
/// Stocks are capped at this many days of use.
const MAX_FOOD_DAYS: f64 = 400.0;
const MAX_GOODS_DAYS: f64 = 400.0;
/// Days for mood to move most of the way to where food and goods put it.
const MOOD_DAYS: f64 = 20.0;
/// Days per year of growth.
const YEAR_DAYS: f64 = 365.0;
/// Days for a relation to move most of the way to its target.
const RELATION_DAYS: f64 = 120.0;
/// Seats closer than this (m) rub against each other.
const BORDER: f64 = 400_000.0;

/// A settlement's changing state.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SettlementState {
    /// People by role (the settlement's roles, in order), fractional as they grow.
    pub people: Vec<f64>,
    /// Food in store (person-days).
    pub food: f64,
    /// Goods in store (units).
    pub goods: f64,
    /// 0 (miserable) … 1 (content).
    pub mood: f64,
}

impl SettlementState {
    pub fn population(&self) -> f64 {
        self.people.iter().sum()
    }

    fn start(s: &Settlement) -> Self {
        let n = f64::from(s.population);
        Self {
            people: s.roles.iter().map(|r| f64::from(r.1)).collect(),
            food: 90.0 * n,
            goods: 30.0 * n * GOODS_USE,
            mood: 0.7,
        }
    }
}

/// A body's society and its settlements, stepping in the body's region.
#[derive(Resource, Clone, Debug)]
pub struct RegionSociety {
    pub society: Society,
    pub settlements: Vec<Settlement>,
    pub states: Vec<SettlementState>,
    /// Relations between factions (−100 … 100), simulated; `society.relations` holds them
    /// rounded.
    pub relations: Vec<Vec<f64>>,
    /// What relations drift toward before the monthly swing.
    targets: Vec<Vec<f64>>,
    /// Each settlement's food yield from its land (1 for domes and hydroponics).
    soil: Vec<f64>,
    /// Game days simulated so far.
    pub days: f64,
    /// Days each slice of the settlements is behind (see [`RegionSociety::step_slice`]).
    behind: [f64; SLICES],
    /// The slice stepped next.
    next_slice: usize,
}

/// The settlements are stepped in this many slices, one per tick (each about every second at
/// 30 Hz), so thousands of settlements cost a tick what a few hundred would.
const SLICES: usize = 32;

/// How settlements are generated; a save from another generation starts its societies fresh
/// (its ids would name other places). 2: settlement patterns (Batch 4b).
pub const GENERATION: u32 = 2;

impl RegionSociety {
    /// The society of body `body` with its settlements, at their starting state.
    pub fn generate(
        system: &StarSystem,
        body: usize,
        director: &ContentDirector,
        patterns: &SettlementPatterns,
    ) -> Self {
        let mut society = generate(system, body, director);
        let settlements = generate_settlements(system, &mut society, director, patterns);
        let radius = system.bodies[body].radius;
        let seat = |f: usize| settlements.iter().find(|s| s.faction == f && s.seat);
        let n = society.factions.len();
        let mut targets = vec![vec![100.0; n]; n];
        for (i, row) in targets.iter_mut().enumerate() {
            for (j, t) in row.iter_mut().enumerate() {
                if i == j {
                    continue;
                }
                let base = f64::from(society.relations[i][j]);
                let friction = match (seat(i), seat(j)) {
                    (Some(a), Some(b)) => {
                        let d = a.direction.angle_between(b.direction) * radius;
                        let near = (1.0 - d / BORDER).max(0.0);
                        let across = society.factions[i].culture != society.factions[j].culture;
                        near * if across { 25.0 } else { 10.0 }
                    }
                    _ => 0.0,
                };
                *t = base - friction;
            }
        }
        let soil = settlements
            .iter()
            .map(|s| match society.cultures[s.culture].profile.tech_tier {
                TechTier::Spacefaring => 1.0,
                _ => 0.5 + 0.8 * s.site_score,
            })
            .collect();
        let states = settlements.iter().map(SettlementState::start).collect();
        let relations = society
            .relations
            .iter()
            .map(|r| r.iter().map(|&v| f64::from(v)).collect())
            .collect();
        Self {
            society,
            settlements,
            states,
            relations,
            targets,
            soil,
            days: 0.0,
            behind: [0.0; SLICES],
            next_slice: 0,
        }
    }

    pub fn population(&self) -> f64 {
        self.states.iter().map(SettlementState::population).sum()
    }

    /// Advances everything by `days` game days.
    pub fn step(&mut self, days: f64) {
        if days <= 0.0 {
            return;
        }
        for (i, s) in self.settlements.iter().enumerate() {
            let culture = &self.society.cultures[s.culture].profile;
            step_settlement(&mut self.states[i], s, culture, self.soil[i], days);
        }
        self.behind = [0.0; SLICES];
        self.step_relations(days);
    }

    /// Advances by `days` game days, stepping one slice of the settlements (by the days it
    /// is behind) and the relations; over [`SLICES`] calls every settlement catches up.
    pub fn step_slice(&mut self, days: f64) {
        if days <= 0.0 {
            return;
        }
        for b in &mut self.behind {
            *b += days;
        }
        let slice = self.next_slice;
        self.next_slice = (slice + 1) % SLICES;
        let behind = std::mem::take(&mut self.behind[slice]);
        for i in (slice..self.settlements.len()).step_by(SLICES) {
            let s = &self.settlements[i];
            let culture = &self.society.cultures[s.culture].profile;
            step_settlement(&mut self.states[i], s, culture, self.soil[i], behind);
        }
        self.step_relations(days);
    }

    fn step_relations(&mut self, days: f64) {
        // Relations: toward the target, which swings a little month to month.
        let month = (self.days / 30.0).floor() as u64;
        let pull = 1.0 - (-days / RELATION_DAYS).exp();
        let n = self.relations.len();
        for i in 0..n {
            for j in (i + 1)..n {
                let swing = Rng::derive(self.society.seed ^ month, &format!("relation/{i}/{j}"))
                    .unit()
                    * 2.0
                    - 1.0;
                let target = (self.targets[i][j] + 8.0 * swing).clamp(-100.0, 100.0);
                let r = self.relations[i][j] + (target - self.relations[i][j]) * pull;
                self.relations[i][j] = r;
                self.relations[j][i] = r;
                self.society.relations[i][j] = r.round() as i32;
                self.society.relations[j][i] = r.round() as i32;
            }
        }
        self.days += days;
    }
}

fn step_settlement(
    state: &mut SettlementState,
    s: &Settlement,
    culture: &crate::lore::CultureProfile,
    soil: f64,
    days: f64,
) {
    let n = state.population().max(1.0);
    // Production and use.
    let (mut food, mut goods) = (0.0, 0.0);
    for (k, (role, _)) in s.roles.iter().enumerate() {
        food += food_per_day(role) * soil * state.people[k];
        goods += goods_per_day(role) * state.people[k];
    }
    state.food += (food - n) * days;
    state.goods += (goods - n * GOODS_USE) * days;
    state.food *= (1.0 - FOOD_SPOIL).powf(days);
    state.goods *= (1.0 - GOODS_WEAR).powf(days);
    let starving = state.food <= 0.0;
    state.food = state.food.clamp(0.0, MAX_FOOD_DAYS * n);
    state.goods = state.goods.clamp(0.0, MAX_GOODS_DAYS * n * GOODS_USE);

    // Mood toward what the stores support.
    let food_days = state.food / n;
    let goods_days = state.goods / (n * GOODS_USE);
    let target = if starving {
        0.1
    } else {
        0.45 + 0.35 * (food_days / 60.0).min(1.0) + 0.2 * (goods_days / 30.0).min(1.0)
    };
    state.mood += (target - state.mood) * (1.0 - (-days / MOOD_DAYS).exp());

    // Growth (a year's rate, per day), shared out by the culture's role shares: new people go
    // where the settlement is short of its usual share, losses fall in proportion.
    let rate = if starving {
        -0.08
    } else {
        0.03 * (state.mood - 0.6)
    };
    let change = n * rate * days / YEAR_DAYS;
    let total: f64 = culture.roles.iter().map(|r| r.share).sum::<f64>().max(1e-9);
    let weights: Vec<f64> = s
        .roles
        .iter()
        .enumerate()
        .map(|(k, (role, _))| {
            let share = culture
                .roles
                .iter()
                .find(|r| &r.id == role)
                .map_or(0.0, |r| r.share)
                / total;
            if change >= 0.0 {
                (share * (n + change) - state.people[k]).max(0.0) + share * 0.1
            } else {
                state.people[k] / n
            }
        })
        .collect();
    let sum = weights.iter().sum::<f64>().max(1e-9);
    for (p, w) in state.people.iter_mut().zip(&weights) {
        *p = (*p + change * w / sum).max(0.0);
    }
}

/// Steps every region's society by the tick's length in game days.
pub fn society_system(clock: Res<SimClock>, society: Option<ResMut<RegionSociety>>) {
    if let Some(mut s) = society {
        let days = clock.dt * GAME_TIME_SCALE as f64 / 86_400.0;
        s.step_slice(days);
    }
}

/// Adds the aggregate simulation to a region's schedule.
pub fn add_society_systems(schedule: &mut Schedule) {
    schedule.add_systems(society_system.in_set(RegionStage::Simulate));
}

/// Generates the society of every body with peoples and puts it in the body's region. Returns
/// the bodies' societies (index, people, settlements) for logs.
pub fn install(
    sim: &mut Simulation,
    director: &Arc<ContentDirector>,
    patterns: &SettlementPatterns,
) -> Vec<(usize, u32, usize)> {
    let Some(system) = sim.star_system().cloned() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (i, body) in system.bodies.iter().enumerate() {
        if body.peoples.is_empty() {
            continue;
        }
        let rs = RegionSociety::generate(&system, i, director, patterns);
        out.push((i, rs.population().round() as u32, rs.settlements.len()));
        if let Some(region) = sim.regions.get_mut(i) {
            region.world.insert_resource(rs);
        }
    }
    out
}

/// The saved part of a body's society: settlement states by id, relations by faction id.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedSociety {
    pub body: String,
    /// How the settlements were generated ([`GENERATION`]); saves from before it have 1.
    #[serde(default = "first_generation")]
    pub generation: u32,
    pub days: f64,
    pub settlements: Vec<(String, SettlementState)>,
    pub relations: Vec<(String, String, f64)>,
}

fn first_generation() -> u32 {
    1
}

/// Every body's society state, for a save.
pub fn capture(sim: &Simulation, system: &StarSystem) -> Vec<SavedSociety> {
    let mut out = Vec::new();
    for (i, body) in system.bodies.iter().enumerate() {
        let Some(rs) = sim
            .regions
            .get(i)
            .and_then(|r| r.world.get_resource::<RegionSociety>())
        else {
            continue;
        };
        let f = &rs.society.factions;
        let mut relations = Vec::new();
        for a in 0..f.len() {
            for b in (a + 1)..f.len() {
                relations.push((f[a].id.clone(), f[b].id.clone(), rs.relations[a][b]));
            }
        }
        out.push(SavedSociety {
            body: body.id.clone(),
            generation: GENERATION,
            days: rs.days,
            settlements: rs
                .settlements
                .iter()
                .zip(&rs.states)
                .map(|(s, st)| (s.id.clone(), st.clone()))
                .collect(),
            relations,
        });
    }
    out
}

/// Puts saved society states back (settlements and factions matched by id; anything the save
/// doesn't know keeps its generated start). Returns what couldn't be matched.
pub fn restore(sim: &mut Simulation, system: &StarSystem, saved: &[SavedSociety]) -> Vec<String> {
    let mut problems = Vec::new();
    for s in saved {
        if s.generation != GENERATION {
            problems.push(format!(
                "society of {} saved by an older generation of settlements: it starts fresh",
                s.body
            ));
            continue;
        }
        let Some(i) = system.bodies.iter().position(|b| b.id == s.body) else {
            problems.push(format!("society of unknown body {:?} dropped", s.body));
            continue;
        };
        let Some(mut rs) = sim
            .regions
            .get_mut(i)
            .and_then(|r| r.world.get_resource_mut::<RegionSociety>())
        else {
            problems.push(format!("{} has no society now", s.body));
            continue;
        };
        rs.days = s.days;
        for (id, state) in &s.settlements {
            match rs.settlements.iter().position(|x| &x.id == id) {
                Some(k) if rs.states[k].people.len() == state.people.len() => {
                    rs.states[k] = state.clone()
                }
                _ => problems.push(format!("settlement {id} no longer exists")),
            }
        }
        for (a, b, v) in &s.relations {
            let find = |id: &str| rs.society.factions.iter().position(|f| f.id == id);
            match (find(a), find(b)) {
                (Some(x), Some(y)) => {
                    rs.relations[x][y] = *v;
                    rs.relations[y][x] = *v;
                    rs.society.relations[x][y] = v.round() as i32;
                    rs.society.relations[y][x] = v.round() as i32;
                }
                _ => problems.push(format!("relation {a}–{b}: a faction no longer exists")),
            }
        }
    }
    problems
}
