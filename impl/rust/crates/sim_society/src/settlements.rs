//! Settlement generation (living-planet.md §3, steps 2 and 4): sites scored by the body's own
//! terrain and climate in each people's home region, chosen with spacing, divided into
//! contiguous faction territories around seats far apart, given a kind, a population by role,
//! a name and lore. The same body, society and director answers give the same settlements.

use ai_gateway::{ContentDirector, ContentRequest, Source};
use glam::DVec3;
use serde_json::json;
use sim_core::StarSystem;
use sim_planet::map::direction;
use sim_planet::{Biome, Climate, Terrain, fibonacci_directions};

use crate::lore::{SettlementLore, TechTier};
use crate::names;
use crate::rng::Rng;
use crate::society::Society;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SettlementKind {
    Hamlet,
    Village,
    Town,
    Outpost,
    Port,
    Colony,
}

impl SettlementKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Hamlet => "hamlet",
            Self::Village => "village",
            Self::Town => "town",
            Self::Outpost => "outpost",
            Self::Port => "port",
            Self::Colony => "colony",
        }
    }
}

/// A settlement as generated: where, whose, what kind, how many people in which roles.
/// (Its changing state, population over time and stocks, is the aggregate simulation's.)
#[derive(Clone, Debug, PartialEq)]
pub struct Settlement {
    /// `<culture>/<n>`.
    pub id: String,
    pub name: String,
    /// Index into [`Society::cultures`].
    pub culture: usize,
    /// Index into [`Society::factions`].
    pub faction: usize,
    /// The faction's seat.
    pub seat: bool,
    pub kind: SettlementKind,
    /// Centre (unit direction in the body's surface frame).
    pub direction: DVec3,
    /// Ground height at the centre (m above the reference radius).
    pub height: f64,
    pub population: u32,
    /// People by role (the culture's role ids), adding up to the population.
    pub roles: Vec<(String, u32)>,
    pub founded_years_ago: u32,
    /// Seed of its layout (buildings and streets, made when the player comes near).
    pub layout_seed: u64,
    pub lore: SettlementLore,
    /// How good its site was (0–1).
    pub site_score: f64,
}

/// Sites closer than this to the sea (m above sea level) are too low.
const MIN_HEIGHT: f64 = 5.0;
/// Steepest ground a settlement takes (radians).
const MAX_SLOPE: f64 = 0.14;
/// Candidate sites: this many directions over the whole sphere (those inside a home region).
const CANDIDATES: usize = 200_000;

/// Scores a site for a people of `tier` (0 unusable … 1 ideal) and returns its height.
fn score_site(
    terrain: &Terrain,
    climate: &Climate,
    tier: TechTier,
    dir: DVec3,
) -> Option<(f64, f64)> {
    let px = 200.0;
    let s = terrain.sample(dir, px);
    if s.height < MIN_HEIGHT {
        return None;
    }
    let slope = terrain.slope(dir, px, px);
    if slope > MAX_SLOPE {
        return None;
    }
    let t = climate.temperature_at(terrain, dir, s.height, 0.0);
    let m = climate.moisture(terrain, dir, &s);
    let biome = climate.biome(terrain, &s, t, m, slope);
    let flat = 1.0 - slope / MAX_SLOPE;
    let score = match tier {
        TechTier::Spacefaring => {
            // Flat, open ground for landers and modules; climate matters little.
            let ground = match biome {
                Biome::Grassland | Biome::Desert | Biome::Tundra | Biome::Beach => 1.0,
                Biome::Forest | Biome::Taiga => 0.6,
                Biome::Jungle => 0.3,
                _ => return None,
            };
            0.7 * flat * flat + 0.3 * ground
        }
        _ => {
            // Farmland: good soil, a mild climate, water, near the coast.
            let land = match biome {
                Biome::Grassland => 1.0,
                Biome::Forest => 0.75,
                Biome::Beach => 0.5,
                Biome::Taiga | Biome::Jungle => 0.4,
                Biome::Tundra | Biome::Desert => 0.15,
                _ => return None,
            };
            let mild = 1.0 - ((t - 14.0).abs() / 20.0).min(1.0);
            let coast = 1.0 - (s.land / 0.15).clamp(0.0, 1.0);
            0.35 * land + 0.2 * mild + 0.2 * m + 0.15 * coast + 0.1 * flat
        }
    };
    Some((score.clamp(0.0, 1.0), s.height))
}

/// The settlements of every people on body `body`, in the society's culture order.
pub fn generate_settlements(
    system: &StarSystem,
    society: &mut Society,
    director: &ContentDirector,
) -> Vec<Settlement> {
    let body = &system.bodies[society.body];
    let (Some(terrain), Some(climate)) = (Terrain::of(body), Climate::of(body)) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for ci in 0..society.cultures.len() {
        let culture = society.cultures[ci].clone();
        let tier = culture.profile.tech_tier;
        let home = culture.people.home;
        let centre = direction(
            home.latitude_deg.to_radians(),
            home.longitude_deg.to_radians(),
        );
        let reach = home.radius_deg.to_radians();
        let count = culture.people.settlements as usize;

        // Candidate sites with their scores (the home region's edge counts less).
        let sites: Vec<(f64, DVec3, f64)> = fibonacci_directions(CANDIDATES)
            .filter(|d| d.angle_between(centre) < reach)
            .filter_map(|d| {
                let (score, height) = score_site(&terrain, &climate, tier, d)?;
                let edge = d.angle_between(centre) / reach;
                Some((score * (1.0 - 0.5 * edge * edge), d, height))
            })
            .collect();
        // Best sites first (with a little chance, so the pattern isn't a lattice), each keeping
        // its own distance (between 0.6 and 1.4 times the spacing) from those before it;
        // closer spacing if the region is too small.
        let mut pick = Rng::derive(society.seed, &format!("sites/{}", culture.profile.id));
        let mut ranked: Vec<(f64, usize, f64)> = sites
            .iter()
            .enumerate()
            .map(|(i, s)| (s.0 + 0.15 * pick.unit(), i, 0.6 + 0.8 * pick.unit()))
            .collect();
        ranked.sort_by(|a, b| b.0.total_cmp(&a.0));
        let mut spacing = reach * 1.6 / (count as f64).sqrt();
        let chosen = loop {
            let mut chosen: Vec<(f64, DVec3, f64)> = Vec::new();
            for &(_, i, own) in &ranked {
                if chosen.len() == count {
                    break;
                }
                let s = sites[i];
                if chosen
                    .iter()
                    .all(|c| c.1.angle_between(s.1) >= spacing * own)
                {
                    chosen.push(s);
                }
            }
            if chosen.len() == count || spacing < reach * 0.05 {
                break chosen;
            }
            spacing *= 0.8;
        };

        // Faction territories: wedges around the people's centre, in bearing order, sized to
        // each faction's share (always contiguous, always the right size); the first faction's
        // wedge holds the best site. Each faction's seat is the best site near the middle of
        // its wedge.
        let factions: Vec<usize> = (0..society.factions.len())
            .filter(|&f| society.factions[f].culture == ci)
            .collect();
        let middle = chosen.iter().map(|c| c.1).sum::<DVec3>().normalize();
        let east = DVec3::Y
            .cross(middle)
            .normalize_or(middle.any_orthonormal_vector());
        let north = middle.cross(east);
        let bearing = |d: DVec3| d.dot(north).atan2(d.dot(east));
        let mut around: Vec<usize> = (0..chosen.len()).collect();
        around.sort_by(|&x, &y| bearing(chosen[x].1).total_cmp(&bearing(chosen[y].1)));
        let best = (0..chosen.len())
            .max_by(|&x, &y| chosen[x].0.total_cmp(&chosen[y].0))
            .unwrap_or(0);
        let at = around.iter().position(|&i| i == best).unwrap_or(0);
        let first = factions
            .first()
            .map_or(0, |&f| society.factions[f].settlements as usize);
        around.rotate_left((at + chosen.len() - first / 2) % chosen.len().max(1));
        let mut owner: Vec<usize> = vec![0; chosen.len()];
        let mut seats: Vec<usize> = Vec::new();
        let mut next = 0;
        for (k, &f) in factions.iter().enumerate() {
            let take = (society.factions[f].settlements as usize).min(chosen.len() - next);
            let wedge = &around[next..next + take];
            next += take;
            for &i in wedge {
                owner[i] = k;
            }
            let mid = wedge
                .iter()
                .map(|&i| chosen[i].1)
                .sum::<DVec3>()
                .normalize_or(middle);
            let spread = wedge
                .iter()
                .map(|&i| chosen[i].1.angle_between(mid))
                .fold(1e-6, f64::max);
            let seat = wedge.iter().copied().max_by(|&x, &y| {
                let worth =
                    |i: usize| chosen[i].0 * (1.0 - 0.5 * chosen[i].1.angle_between(mid) / spread);
                worth(x).total_cmp(&worth(y))
            });
            if let Some(i) = seat {
                seats.push(i);
            }
        }
        let owner: Vec<Option<usize>> = owner.into_iter().map(Some).collect();

        // Kinds, populations, names and lore.
        let mut rng = Rng::derive(society.seed, &format!("settlements/{}", culture.profile.id));
        let mut used: Vec<String> = Vec::new();
        for (i, &(score, dir, height)) in chosen.iter().enumerate() {
            let k = owner[i].unwrap_or(0);
            let faction = factions.get(k).copied().unwrap_or(0);
            let seat = seats.get(k) == Some(&i);
            let (kind, population) = match tier {
                TechTier::Spacefaring => match (seat, k) {
                    (true, 0) => (SettlementKind::Colony, 2500 + (2500.0 * score) as u32),
                    (true, _) => (SettlementKind::Port, 900 + (900.0 * score) as u32),
                    _ => (
                        SettlementKind::Outpost,
                        80 + (400.0 * score * rng.unit()) as u32,
                    ),
                },
                _ => {
                    if seat {
                        (
                            SettlementKind::Town,
                            900 + (1600.0 * score * (0.6 + 0.4 * rng.unit())) as u32,
                        )
                    } else if score * (0.6 + 0.8 * rng.unit()) > 0.55 {
                        (
                            SettlementKind::Village,
                            150 + (350.0 * score * rng.unit()) as u32,
                        )
                    } else {
                        (SettlementKind::Hamlet, 25 + (90.0 * rng.unit()) as u32)
                    }
                }
            };
            let name = loop {
                let n = names::place_for(&culture.profile.naming, kind_word(kind), &mut rng);
                if !used.contains(&n) {
                    used.push(n.clone());
                    break n;
                }
            };
            let roles = role_counts(&culture.profile.roles, population);
            let founded_years_ago = match tier {
                TechTier::Spacefaring => match kind {
                    SettlementKind::Colony => 62,
                    _ => 5 + (50.0 * rng.unit()) as u32,
                },
                _ => match kind {
                    SettlementKind::Town => 600 + (280.0 * rng.unit()) as u32,
                    _ => 60 + (700.0 * rng.unit()) as u32,
                },
            };
            let id = format!("{}/{}", culture.profile.id, i);
            let faction_name = society.factions[faction].name.clone();
            let request = ContentRequest::new(
                "settlement_lore",
                format!("settlement/{}/{}/{i}", culture.profile.id, kind.name()),
                json!({"name": name, "faction": faction_name, "kind": kind.name(), "population": population}),
            );
            let answer = director.request(&request, || SettlementLore {
                description: "{settlement} is a {kind} of {people}.".into(),
                landmark: "Nothing stands out.".into(),
            });
            if let Source::Fallback(why) = &answer.source {
                society.fallbacks.push(why.clone());
            }
            let fill = |t: &str| {
                t.replace("{settlement}", &name)
                    .replace("{faction}", &faction_name)
                    .replace("{people}", &culture.profile.name)
                    .replace("{culture}", &culture.profile.adjective)
                    .replace("{kind}", kind.name())
            };
            out.push(Settlement {
                id,
                name: name.clone(),
                culture: ci,
                faction,
                seat,
                kind,
                direction: dir,
                height,
                population,
                roles,
                founded_years_ago,
                layout_seed: rng.next_u64(),
                lore: SettlementLore {
                    description: fill(&answer.value.description),
                    landmark: fill(&answer.value.landmark),
                },
                site_score: score,
            });
        }
    }
    out
}

/// The word a place pattern uses for this kind of settlement ("Port {name}" only for ports).
fn kind_word(kind: SettlementKind) -> &'static str {
    match kind {
        SettlementKind::Port => "Port",
        SettlementKind::Outpost => "Outpost",
        SettlementKind::Colony => "Station",
        SettlementKind::Town => "Town",
        SettlementKind::Village => "Village",
        SettlementKind::Hamlet => "Hamlet",
    }
}

/// People by role: the culture's shares of `population`, rounded so they add up.
fn role_counts(roles: &[crate::lore::Role], population: u32) -> Vec<(String, u32)> {
    let mut counts: Vec<(String, u32, f64)> = roles
        .iter()
        .map(|r| {
            let exact = r.share * f64::from(population);
            (r.id.clone(), exact.floor() as u32, exact.fract())
        })
        .collect();
    let mut left = population - counts.iter().map(|c| c.1).sum::<u32>();
    let mut order: Vec<usize> = (0..counts.len()).collect();
    order.sort_by(|&a, &b| counts[b].2.total_cmp(&counts[a].2));
    for i in order.into_iter().cycle() {
        if left == 0 {
            break;
        }
        counts[i].1 += 1;
        left -= 1;
    }
    counts.into_iter().map(|(id, n, _)| (id, n)).collect()
}
