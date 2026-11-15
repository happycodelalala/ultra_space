//! A body's history and the conditions it left (living-planet.md §8b conditions, §8c the
//! chronicle): the content director writes the history (`history/<body>`, recorded responses
//! in content/lore) as dated events; generation applies their consequences. A catastrophe
//! lays out the people's old settlement pattern, leaves its cities and towns as ruins, and
//! leaves hazard zones centred on them (a wide fallout over the whole land, hot zones on the
//! old cities); the survivors settle away from the worst of them.
//!
//! Generated, not saved: the same body, seeds and answers give the same history,
//! conditions and ruins. (Conditions made during play will be saved; §8c.)

use ai_gateway::Validate;
use glam::DVec3;
use serde::{Deserialize, Serialize};
use sim_planet::map::direction;
use sim_planet::{Climate, Terrain};

use crate::names;
use crate::patterns::SettlementPatterns;
use crate::rng::Rng;
use crate::settlements::{SizeClass, place};
use crate::society::Society;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    /// A ship fell or landed: a people's beginning on the body.
    Fall,
    Landing,
    Founding,
    War,
    Migration,
    Discovery,
    /// A disaster that leaves a condition on the land.
    Catastrophe,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HazardKind {
    Radiation,
    Chemical,
    Biological,
}

impl HazardKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Radiation => "radiation",
            Self::Chemical => "chemical",
            Self::Biological => "biological",
        }
    }
}

/// What a catastrophe left: a hazard of this kind and severity (0–1, as it is now) over the
/// people's home region, where it had settled by its old pattern (`old_pattern`, an id in
/// content/data/settlement_patterns).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConditionSpec {
    pub hazard: HazardKind,
    pub severity: f64,
    pub old_pattern: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryEvent {
    pub years_ago: u32,
    pub kind: EventKind,
    /// The people it concerns (a culture id), if one.
    #[serde(default)]
    pub people: Option<String>,
    pub title: String,
    pub text: String,
    #[serde(default)]
    pub condition: Option<ConditionSpec>,
}

/// A body's history, oldest first.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct History {
    pub events: Vec<HistoryEvent>,
}

impl Validate for History {
    fn validate(&self) -> Result<(), String> {
        let mut problems = Vec::new();
        for (i, e) in self.events.iter().enumerate() {
            if e.title.trim().is_empty() || e.text.trim().is_empty() {
                problems.push(format!("event {i} has no title or text"));
            }
            if let Some(c) = &e.condition {
                if !(0.0..=1.0).contains(&c.severity) {
                    problems.push(format!("event {i}: severity {} not in 0–1", c.severity));
                }
                if e.kind != EventKind::Catastrophe {
                    problems.push(format!("event {i}: only a catastrophe leaves a condition"));
                }
            }
        }
        if self
            .events
            .windows(2)
            .any(|w| w[0].years_ago < w[1].years_ago)
        {
            problems.push("events aren't oldest first".into());
        }
        if problems.is_empty() {
            Ok(())
        } else {
            Err(problems.join("; "))
        }
    }
}

/// A place where the hazard is: centre (unit direction, surface frame), radius (m) and
/// strength at the centre (0–1), easing to nothing at the radius.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HazardZone {
    pub center: DVec3,
    pub radius: f64,
    pub intensity: f64,
}

/// A condition on the land, left by a catastrophe.
#[derive(Clone, Debug, PartialEq)]
pub struct Condition {
    pub hazard: HazardKind,
    /// The people whose land it is (index into the society's cultures).
    pub culture: usize,
    pub years_ago: u32,
    pub zones: Vec<HazardZone>,
}

/// The ruin of a settlement from before a catastrophe.
#[derive(Clone, Debug, PartialEq)]
pub struct Ruin {
    pub name: String,
    pub culture: usize,
    pub direction: DVec3,
    /// Ground height (m above the reference radius).
    pub height: f64,
    pub size: SizeClass,
    pub years_ago: u32,
    pub layout_seed: u64,
}

/// How strong a body's hazards are at unit direction `dir` (0–1, the strongest kind) and of
/// what kind, on a body of `radius`.
pub fn hazard_at(conditions: &[Condition], dir: DVec3, radius: f64) -> (f64, Option<HazardKind>) {
    let mut best = (0.0, None);
    for c in conditions {
        let mut level: f64 = 0.0;
        for z in &c.zones {
            let d = dir.angle_between(z.center) * radius;
            if d < z.radius {
                let t = d / z.radius;
                level = level.max(z.intensity * (1.0 - t * t * (3.0 - 2.0 * t)));
            }
        }
        if level > best.0 {
            best = (level, Some(c.hazard));
        }
    }
    best
}

/// The conditions and ruins `history` leaves on `society`'s body: for each catastrophe, its
/// people's old pattern laid out (as it was) gives the ruins (towns and bigger) and the hazard
/// zones (fallout over the whole home region at a quarter of the severity; hot zones on the
/// old cities and metropolises, and on some towns).
pub fn apply(
    history: &History,
    society: &Society,
    terrain: &Terrain,
    climate: &Climate,
    patterns: &SettlementPatterns,
) -> (Vec<Condition>, Vec<Ruin>) {
    let (mut conditions, mut ruins) = (Vec::new(), Vec::new());
    let r = terrain.radius;
    for e in &history.events {
        let Some(spec) = &e.condition else { continue };
        let Some(ci) = e
            .people
            .as_ref()
            .and_then(|p| society.cultures.iter().position(|c| &c.profile.id == p))
        else {
            continue;
        };
        let Some(old) = patterns.by_id(&spec.old_pattern) else {
            continue;
        };
        let culture = &society.cultures[ci];
        let home = culture.people.home;
        let tier = culture.profile.tech_tier;
        let strengths = vec![1.0; culture.people.factions.max(1) as usize];
        let label = format!("history/{}", culture.profile.id);
        let before = place(
            terrain,
            climate,
            tier,
            old,
            home,
            &strengths,
            society.seed,
            &label,
            &|_| 0.0,
        );
        let mut rng = Rng::derive(society.seed, &label);
        let centre = direction(
            home.latitude_deg.to_radians(),
            home.longitude_deg.to_radians(),
        );
        let mut zones = vec![HazardZone {
            center: centre,
            radius: home.radius_deg.to_radians() * r * 1.1,
            intensity: 0.25 * spec.severity,
        }];
        let mut used = std::collections::HashSet::new();
        for site in &before {
            let size = site.class.map_or(old.seat.size, |c| old.classes[c].size);
            if size < SizeClass::Town {
                continue;
            }
            let hot = match size {
                SizeClass::Megalopolis | SizeClass::Metropolis => Some(30_000.0),
                SizeClass::City => Some(12_000.0),
                _ => (rng.unit() < 0.3).then_some(4_000.0),
            };
            if let Some(radius) = hot {
                zones.push(HazardZone {
                    center: site.dir,
                    radius,
                    intensity: spec.severity * (0.6 + 0.4 * rng.unit()),
                });
            }
            let name = {
                let mut name = String::new();
                for _ in 0..30 {
                    name = names::place(&culture.profile.naming, &mut rng);
                    if used.insert(name.clone()) {
                        break;
                    }
                }
                name
            };
            ruins.push(Ruin {
                name,
                culture: ci,
                direction: site.dir,
                height: site.height,
                size,
                years_ago: e.years_ago,
                layout_seed: rng.next_u64(),
            });
        }
        conditions.push(Condition {
            hazard: spec.hazard,
            culture: ci,
            years_ago: e.years_ago,
            zones,
        });
    }
    (conditions, ruins)
}
