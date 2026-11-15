//! Settlement generation (living-planet.md §3, steps 2 and 4): sites scored by the body's own
//! terrain and climate in each people's home region, chosen with spacing, divided into
//! contiguous faction territories around seats far apart, given a kind, a population by role,
//! a name and lore. The same body, society and director answers give the same settlements.

use ai_gateway::{ContentDirector, ContentRequest, Source};
use glam::{DVec2, DVec3};
use serde_json::json;
use sim_core::StarSystem;
use sim_planet::map::direction;
use sim_planet::{Biome, Climate, Terrain};

use crate::lore::{SettlementLore, TechTier};
use crate::names;
use crate::patterns::{SettlementPattern, SettlementPatterns};
use crate::rng::Rng;
use crate::society::Society;
use serde::{Deserialize, Serialize};

/// How big a settlement is, the same for every culture and tier (living-planet.md §8b, the
/// settlement hierarchy). Drives spacing, layout scale and level of detail; never shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SizeClass {
    /// 1–3 households (decoration, not simulated).
    Dwelling,
    /// Up to ~100 people.
    Hamlet,
    /// 100–500.
    Village,
    /// 500–5,000.
    Town,
    /// 5,000–100,000.
    City,
    /// Over 100,000.
    Metropolis,
    /// Millions.
    Megalopolis,
}

impl SizeClass {
    pub fn name(self) -> &'static str {
        match self {
            Self::Dwelling => "dwelling",
            Self::Hamlet => "hamlet",
            Self::Village => "village",
            Self::Town => "town",
            Self::City => "city",
            Self::Metropolis => "metropolis",
            Self::Megalopolis => "megalopolis",
        }
    }
}

/// What a settlement is for, on top of its size (any number of them).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SettlementRole {
    /// A faction's seat.
    Seat,
    /// The people's capital (the first faction's seat).
    Capital,
    /// A port, by sea or for spacecraft.
    Port,
    Fort,
    Market,
    Mine,
    Farm,
    Temple,
    Research,
    /// A post without settled families.
    Outpost,
    /// A protected city in a wasteland.
    Enclave,
    Arcology,
    /// A settlement whose place is an orbit.
    Station,
}

impl SettlementRole {
    pub fn name(self) -> &'static str {
        match self {
            Self::Seat => "seat",
            Self::Capital => "capital",
            Self::Port => "port",
            Self::Fort => "fort",
            Self::Market => "market",
            Self::Mine => "mine",
            Self::Farm => "farm",
            Self::Temple => "temple",
            Self::Research => "research",
            Self::Outpost => "outpost",
            Self::Enclave => "enclave",
            Self::Arcology => "arcology",
            Self::Station => "station",
        }
    }

    /// Roles whose word names a settlement before its size's (the first a settlement has).
    const NAMING: [SettlementRole; 6] = [
        Self::Capital,
        Self::Seat,
        Self::Port,
        Self::Outpost,
        Self::Enclave,
        Self::Arcology,
    ];
}

/// The default word for a settlement of `size` at `tier`, when its culture's lore has none
/// (living-planet.md §8b, the hierarchy table).
pub fn default_word(tier: TechTier, size: SizeClass) -> &'static str {
    use SizeClass::*;
    use TechTier::*;
    match (tier, size) {
        (_, Dwelling) => match tier {
            StoneAge => "camp",
            Ancient | Medieval => "farmstead",
            EarlyModern | Industrial | Modern => "farm",
            Spacefaring | Interstellar => "hab",
        },
        (StoneAge, Hamlet) => "camp",
        (StoneAge, Town) => "hill fort",
        (Spacefaring | Interstellar, Hamlet) => "outpost",
        (Spacefaring | Interstellar, Village) => "settlement",
        (Spacefaring | Interstellar, Town) => "colony town",
        (Spacefaring | Interstellar, Metropolis | Megalopolis) => "arcology",
        _ => size.name(),
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
    pub size: SizeClass,
    /// What it is for (seat, port, outpost...): its roles in the settlement hierarchy.
    pub settlement_roles: Vec<SettlementRole>,
    /// What its people call a place like it ("hold", "outpost", "the Landing").
    pub word: String,
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

impl Settlement {
    pub fn has(&self, role: SettlementRole) -> bool {
        self.settlement_roles.contains(&role)
    }

    /// The key of its kind of lore (`settlement/<culture>/<kind>/*` in content/lore): its
    /// first of "capital", "port", "outpost", "enclave", else its size.
    pub fn lore_kind(&self) -> &'static str {
        use SettlementRole::*;
        [Capital, Port, Outpost, Enclave]
            .into_iter()
            .find(|r| self.has(*r))
            .map_or(self.size.name(), SettlementRole::name)
    }
}

/// The word a culture uses for a settlement: its lore's word for the first naming role it has
/// (capital, seat, port, outpost, ...), else for its size, else the tier's default.
pub fn settlement_word(
    words: &std::collections::BTreeMap<String, String>,
    tier: TechTier,
    size: SizeClass,
    roles: &[SettlementRole],
) -> String {
    SettlementRole::NAMING
        .iter()
        .filter(|r| roles.contains(r))
        .find_map(|r| words.get(r.name()))
        .or_else(|| words.get(size.name()))
        .cloned()
        .unwrap_or_else(|| default_word(tier, size).to_string())
}

/// Sites closer than this to the sea (m above sea level) are too low.
const MIN_HEIGHT: f64 = 5.0;
/// Steepest ground a settlement takes (radians).
const MAX_SLOPE: f64 = 0.14;

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

/// The settlements of every people on body `body`, in the society's culture order, each
/// people settling its home region by its settlement pattern (living-planet.md §8b):
///
/// 1. **Candidates** on a lattice over the home region (a third of the smallest spacing apart),
///    scored by the land for the people's tier; the best `settled_share` of the usable land is
///    settled, wherever in the region it lies.
/// 2. **Seats**, one per faction: the first the best site near the middle (the people's
///    capital), the others the best sites far from the seats before.
/// 3. **Classes**, biggest first: the settled land's best sites (with a little chance), each
///    keeping its class's spacing (0.8–1.2 times) from every settlement placed before.
/// 4. **Territories:** each settlement belongs to the faction whose seat is nearest, distances
///    shrunk by the faction's strength, so stronger factions hold more land.
///
/// Peoples without a pattern get no settlements (noted as a fallback). The same body,
/// society, patterns and director answers give the same settlements.
pub fn generate_settlements(
    system: &StarSystem,
    society: &mut Society,
    director: &ContentDirector,
    patterns: &SettlementPatterns,
    hazard: &(dyn Fn(DVec3) -> f64 + Sync),
) -> Vec<Settlement> {
    let body = &system.bodies[society.body];
    let (Some(terrain), Some(climate)) = (Terrain::of(body), Climate::of(body)) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for ci in 0..society.cultures.len() {
        let culture = society.cultures[ci].clone();
        let profile = &culture.profile;
        let tier = profile.tech_tier;
        let chosen = match &culture.people.pattern {
            Some(id) => patterns.by_id(id),
            None => patterns.for_people(tier, profile.polity),
        };
        let Some(pattern) = chosen.cloned() else {
            society.fallbacks.push(format!(
                "{}: no settlement pattern for a {:?} {:?}",
                profile.id, tier, profile.polity
            ));
            continue;
        };
        let home = culture.people.home;
        let factions: Vec<usize> = (0..society.factions.len())
            .filter(|&f| society.factions[f].culture == ci)
            .collect();
        let strengths: Vec<f64> = factions
            .iter()
            .map(|&f| society.factions[f].strength)
            .collect();
        let placed = place(
            &terrain,
            &climate,
            tier,
            &pattern,
            home,
            &strengths,
            society.seed,
            &profile.id,
            hazard,
        );
        if placed.is_empty() {
            society
                .fallbacks
                .push(format!("{}: no land to settle", profile.id));
            continue;
        }

        // Sizes, populations, names and lore.
        let mut rng = Rng::derive(society.seed, &format!("settlements/{}", profile.id));
        let mut used: std::collections::HashSet<String> = std::collections::HashSet::new();
        for (n, site) in placed.iter().enumerate() {
            let (site_score, height, dir, class) = (site.score, site.height, site.dir, site.class);
            let k = site.owner;
            let faction = factions.get(k).copied().unwrap_or(0);
            let seat = class.is_none();
            let capital = seat && k == 0;
            let (size, settlement_roles, range) = match class {
                None => {
                    let s = &pattern.seat;
                    let mut roles = s.roles.clone();
                    let mut range = s.people;
                    if capital {
                        roles.extend(s.capital_roles.iter().copied());
                        range = s.capital_people.unwrap_or(range);
                    }
                    (s.size, roles, range)
                }
                Some(c) => {
                    let c = &pattern.classes[c];
                    (c.size, c.roles.clone(), c.people)
                }
            };
            let spread = f64::from(range[1].saturating_sub(range[0]));
            let population =
                range[0] + (spread * (0.5 * site_score + 0.5 * rng.unit())).round() as u32;
            let name = {
                let mut name = String::new();
                for _ in 0..50 {
                    name = names::place_for(
                        &profile.naming,
                        kind_word(size, &settlement_roles),
                        &mut rng,
                    );
                    if !used.contains(&name) {
                        break;
                    }
                }
                used.insert(name.clone());
                name
            };
            let roles = role_counts(&profile.roles, population);
            let founded_years_ago = match tier {
                TechTier::Spacefaring | TechTier::Interstellar => {
                    if settlement_roles.contains(&SettlementRole::Capital) {
                        62
                    } else {
                        5 + (50.0 * rng.unit()) as u32
                    }
                }
                _ => {
                    if size >= SizeClass::Town {
                        600 + (280.0 * rng.unit()) as u32
                    } else {
                        60 + (700.0 * rng.unit()) as u32
                    }
                }
            };
            let id = format!("{}/{}", profile.id, n);
            let faction_name = society.factions[faction].name.clone();
            let word = settlement_word(&profile.settlement_words, tier, size, &settlement_roles);
            let mut settlement = Settlement {
                id,
                name: name.clone(),
                culture: ci,
                faction,
                seat,
                size,
                settlement_roles,
                word: word.clone(),
                direction: dir,
                height,
                population,
                roles,
                founded_years_ago,
                layout_seed: 0,
                lore: SettlementLore {
                    description: String::new(),
                    landmark: String::new(),
                },
                site_score,
            };
            let lore_kind = settlement.lore_kind();
            let request = ContentRequest::new(
                "settlement_lore",
                format!("settlement/{}/{lore_kind}/{n}", profile.id),
                json!({"name": name, "faction": faction_name, "kind": lore_kind, "word": word, "population": population}),
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
                    .replace("{people}", &profile.name)
                    .replace("{culture}", &profile.adjective)
                    .replace("{kind}", &word)
            };
            settlement.layout_seed = rng.next_u64();
            settlement.lore = SettlementLore {
                description: fill(&answer.value.description),
                landmark: fill(&answer.value.landmark),
            };
            society.factions[faction].settlements += 1;
            out.push(settlement);
        }
    }
    out
}

/// A site chosen by [`place`]: where, how good its land, its ground height (m), its class in
/// the pattern (`None`: a seat) and which of the people's factions holds it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PlacedSite {
    pub dir: DVec3,
    pub score: f64,
    pub height: f64,
    pub class: Option<usize>,
    pub owner: usize,
}

/// Places a people's settlements by `pattern` over its `home` region (steps 1–4 of
/// [`generate_settlements`]): one seat per faction (`strengths`), then the classes by spacing,
/// avoiding land where `hazard` (0–1) is 0.35 or more. `label` names its random choices.
#[allow(clippy::too_many_arguments)]
pub(crate) fn place(
    terrain: &Terrain,
    climate: &Climate,
    tier: TechTier,
    pattern: &SettlementPattern,
    home: sim_core::system::HomeRegion,
    strengths: &[f64],
    seed: u64,
    label: &str,
    hazard: &(dyn Fn(DVec3) -> f64 + Sync),
) -> Vec<PlacedSite> {
    let r = terrain.radius;
    let centre = direction(
        home.latitude_deg.to_radians(),
        home.longitude_deg.to_radians(),
    );
    let reach = home.radius_deg.to_radians();
    let (east, north) = {
        let (e, south) = sim_planet::tangent_frame(centre);
        (e, -south)
    };

    // 1. Candidates: a hex lattice on the region's tangent plane (km), scored in parallel.
    let smallest = pattern
        .classes
        .iter()
        .map(|c| c.spacing_km)
        .fold(f64::MAX, f64::min)
        .min(20.0);
    let step = (smallest / 3.0).max(0.8);
    let extent = reach * r / 1000.0;
    let rows = (extent / (step * 0.866)).ceil() as i64;
    let cols = (extent / step).ceil() as i64;
    let mut lattice: Vec<(DVec2, DVec3)> = Vec::new();
    for j in -rows..=rows {
        for i in -cols..=cols {
            let x = (i as f64 + if j % 2 == 0 { 0.0 } else { 0.5 }) * step;
            let y = j as f64 * step * 0.866;
            let d = (centre * r + (east * x + north * y) * 1000.0).normalize();
            if d.angle_between(centre) < reach {
                lattice.push((DVec2::new(x, y), d));
            }
        }
    }
    let threads = std::thread::available_parallelism()
        .map_or(4, |n| n.get())
        .min(16);
    let chunk = lattice.len().div_ceil(threads).max(1);
    let scored: Vec<Option<(f64, f64)>> = std::thread::scope(|scope| {
        let handles: Vec<_> = lattice
            .chunks(chunk)
            .map(|part| {
                let (terrain, climate) = (terrain, climate);
                scope.spawn(move || {
                    part.iter()
                        .map(|&(_, d)| {
                            // The land alone decides where people settle (away from
                            // hazards); only the capital prefers the middle.
                            let (score, height) = score_site(terrain, climate, tier, d)?;
                            let z = hazard(d);
                            (z < 0.35).then_some((score * (1.0 - z).powi(2), height))
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().expect("scoring thread"))
            .collect()
    });
    // The settled land: the best share of the usable sites.
    let mut usable: Vec<usize> = (0..lattice.len())
        .filter(|&i| scored[i].is_some())
        .collect();
    usable.sort_by(|&a, &b| {
        let s = |i: usize| scored[i].map_or(0.0, |x| x.0);
        s(b).total_cmp(&s(a))
    });
    let settled: Vec<usize> = usable
        .iter()
        .copied()
        .take(((usable.len() as f64) * pattern.settled_share).ceil() as usize)
        .collect();
    if settled.is_empty() {
        return Vec::new();
    }
    let score = |i: usize| scored[i].map_or(0.0, |x| x.0);
    let plane = |i: usize| lattice[i].0;

    // 2. Seats.
    let mut seats: Vec<usize> = Vec::new();
    for k in 0..strengths.len() {
        let best = settled.iter().copied().max_by(|&a, &b| {
            let worth = |i: usize| {
                if seats.is_empty() {
                    let edge = plane(i).length() / extent;
                    score(i) * (1.0 - 0.6 * edge)
                } else {
                    let far = seats
                        .iter()
                        .map(|&q| plane(q).distance(plane(i)))
                        .fold(f64::MAX, f64::min);
                    score(i) * (far / extent).min(1.0)
                }
            };
            worth(a).total_cmp(&worth(b))
        });
        if let Some(i) = best
            && !seats.contains(&i)
        {
            seats.push(i);
        } else {
            let _ = k;
            break;
        }
    }

    // 3. Classes by spacing, on a grid of what's placed so far.
    let mut pick = Rng::derive(seed, &format!("sites/{label}"));
    let mut placed: Vec<(usize, Option<usize>)> = seats.iter().map(|&i| (i, None)).collect();
    for (c, class) in pattern.classes.iter().enumerate() {
        let cell = class.spacing_km * 1.2;
        let key = |p: DVec2| ((p.x / cell).floor() as i64, (p.y / cell).floor() as i64);
        let mut grid: std::collections::HashMap<(i64, i64), Vec<DVec2>> =
            std::collections::HashMap::new();
        for &(i, _) in &placed {
            grid.entry(key(plane(i))).or_default().push(plane(i));
        }
        let mut order: Vec<(f64, usize, f64)> = settled
            .iter()
            .map(|&i| (score(i) + 0.15 * pick.unit(), i, 0.8 + 0.4 * pick.unit()))
            .collect();
        order.sort_by(|a, b| b.0.total_cmp(&a.0));
        for (_, i, own) in order {
            let p = plane(i);
            let need = class.spacing_km * own;
            let (kx, ky) = key(p);
            let clear = (-1..=1).all(|dy| {
                (-1..=1).all(|dx| {
                    grid.get(&(kx + dx, ky + dy))
                        .is_none_or(|v| v.iter().all(|q| q.distance(p) >= need))
                })
            });
            if clear {
                grid.entry((kx, ky)).or_default().push(p);
                placed.push((i, Some(c)));
            }
        }
    }

    // 4. Territories by strength-weighted distance to the seats.
    let strengths: Vec<f64> = strengths.iter().map(|s| s.sqrt()).collect();
    let owner = |i: usize| -> usize {
        if let Some(k) = seats.iter().position(|&q| q == i) {
            return k;
        }
        (0..seats.len())
            .min_by(|&a, &b| {
                let d = |k: usize| plane(seats[k]).distance(plane(i)) / strengths[k];
                d(a).total_cmp(&d(b))
            })
            .unwrap_or(0)
    };

    placed
        .iter()
        .map(|&(i, class)| {
            let (score, height) = scored[i].unwrap_or((0.0, 0.0));
            PlacedSite {
                dir: lattice[i].1,
                score,
                height,
                class,
                owner: owner(i),
            }
        })
        .collect()
}

/// The word a place-name pattern uses for this kind of settlement ("Port {name}" only for
/// ports, "{name} Station" for a capital).
fn kind_word(size: SizeClass, roles: &[SettlementRole]) -> &'static str {
    use SettlementRole::*;
    if roles.contains(&Capital) && roles.contains(&Port) {
        "Station"
    } else if roles.contains(&Port) {
        "Port"
    } else if roles.contains(&Outpost) {
        "Outpost"
    } else {
        match size {
            SizeClass::Hamlet | SizeClass::Dwelling => "Hamlet",
            SizeClass::Village => "Village",
            _ => "Town",
        }
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
