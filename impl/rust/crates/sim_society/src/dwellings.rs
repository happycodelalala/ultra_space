//! Dwellings between settlements (living-planet.md §8b, task 4b.8): the decoration layer,
//! placed from the seed near the player and never simulated (their people count toward the
//! nearest settlement). Each people's settled land has one every `dwelling_spacing_km` (its
//! settlement pattern), a farmstead, croft, cabin or hunting camp of its stage, laid out like
//! a tiny settlement when the player comes near.
//!
//! The places are a lattice over latitude and longitude at that spacing, so the same place
//! always has the same dwelling, whatever the player's path: a point (jittered) holds one
//! when it is in the people's settled land (near its settlements but outside them), dry and
//! out of any hazard.

use glam::DVec3;
use sim_planet::Terrain;
use sim_planet::map::{direction, lat_lon};

use crate::aggregate::RegionSociety;
use crate::history::hazard_at;
use crate::lore::SettlementLore;
use crate::patterns::SettlementPatterns;
use crate::rng::Rng;
use crate::settlements::{Settlement, SizeClass, role_counts, settlement_word};

/// Ids of dwellings (as sites in the foundations and keys of the layouts): this bit set, so
/// they never meet a settlement's index.
pub const DWELLING_IDS: u32 = 0x8000_0000;

/// How far a dwelling may lie from its people's nearest settlement, in that people's smallest
/// settlement spacings (beyond that is wild land).
const SETTLED_REACH: f64 = 0.9;

/// The dwellings within `radius` m of unit direction `centre` on `rs`'s body: each with its id
/// (with [`DWELLING_IDS`] set) and a settlement record of size [`SizeClass::Dwelling`] to lay it
/// out from.
pub fn dwellings_near(
    rs: &RegionSociety,
    patterns: &SettlementPatterns,
    terrain: &Terrain,
    centre: DVec3,
    radius: f64,
) -> Vec<(u32, Settlement)> {
    let r = terrain.radius;
    let society = &rs.society;
    let mut out = Vec::new();
    // Settlements that could matter here: their extents, and those of each people near.
    let near: Vec<&Settlement> = rs
        .settlements
        .iter()
        .filter(|s| s.direction.angle_between(centre) * r < radius + 30_000.0)
        .collect();
    if near.is_empty() {
        return out;
    }
    for (ci, culture) in society.cultures.iter().enumerate() {
        let profile = &culture.profile;
        let pattern = match &culture.people.pattern {
            Some(id) => patterns.by_id(id),
            None => patterns.for_people(profile.tech_tier, profile.polity),
        };
        let Some(pattern) = pattern else { continue };
        let Some(spacing) = pattern.dwelling_spacing_km.map(|s| s * 1000.0) else {
            continue;
        };
        let smallest = pattern
            .classes
            .iter()
            .map(|c| c.spacing_km * 1000.0)
            .fold(f64::MAX, f64::min)
            .min(20_000.0);
        let own: Vec<&&Settlement> = near.iter().filter(|s| s.culture == ci).collect();
        if own.is_empty() {
            continue;
        }
        let step = spacing / r;
        let (lat0, lon0) = lat_lon(centre);
        let reach = radius / r;
        let (i0, i1) = (
            ((lat0 - reach) / step).floor() as i64,
            ((lat0 + reach) / step).ceil() as i64,
        );
        for i in i0..=i1 {
            let lat = (i as f64 + 0.5) * step;
            if lat.abs() > 1.5 {
                continue;
            }
            let lon_step = step / lat.cos().max(0.05);
            let span = reach / lat0.cos().max(0.05) + lon_step;
            let (j0, j1) = (
                ((lon0 - span) / lon_step).floor() as i64,
                ((lon0 + span) / lon_step).ceil() as i64,
            );
            for j in j0..=j1 {
                let mut rng = Rng::derive(society.seed, &format!("dwelling/{ci}/{i}/{j}"));
                // About three in four places are lived in.
                if rng.unit() > 0.75 {
                    continue;
                }
                let dir = direction(
                    lat + (rng.unit() - 0.5) * 0.7 * step,
                    j as f64 * lon_step + (rng.unit() - 0.5) * 0.7 * lon_step,
                );
                if dir.angle_between(centre) * r > radius {
                    continue;
                }
                // In this people's settled land: near one of its settlements, outside every
                // settlement.
                let distance = |s: &Settlement| s.direction.angle_between(dir) * r;
                let Some(nearest) = own
                    .iter()
                    .min_by(|a, b| distance(a).total_cmp(&distance(b)))
                else {
                    continue;
                };
                if distance(nearest) > SETTLED_REACH * smallest {
                    continue;
                }
                if near
                    .iter()
                    .any(|s| distance(s) < extent(s.size, s.population) + 150.0)
                {
                    continue;
                }
                let height = terrain.height(dir, 20.0);
                if height < 3.0 || hazard_at(&rs.conditions, dir, r).0 > 0.2 {
                    continue;
                }
                let population = 3 + (rng.next_u64() % 6) as u32;
                let id = DWELLING_IDS | (rng.next_u64() as u32 & !DWELLING_IDS);
                let word = settlement_word(
                    &profile.settlement_words,
                    profile.tech_tier,
                    SizeClass::Dwelling,
                    &[],
                );
                out.push((
                    id,
                    Settlement {
                        id: format!("dwelling/{id}"),
                        name: format!("a {word} near {}", nearest.name),
                        culture: ci,
                        faction: nearest.faction,
                        seat: false,
                        size: SizeClass::Dwelling,
                        settlement_roles: Vec::new(),
                        word,
                        direction: dir,
                        height,
                        population,
                        roles: role_counts(&profile.roles, population),
                        founded_years_ago: 0,
                        layout_seed: rng.next_u64(),
                        lore: SettlementLore {
                            description: String::new(),
                            landmark: String::new(),
                        },
                        site_score: 0.5,
                    },
                ));
            }
        }
    }
    out
}

/// A settlement's reach before its layout is made (m): from its size and people (the layout
/// spreads with them).
fn extent(size: SizeClass, population: u32) -> f64 {
    let by_size: f64 = match size {
        SizeClass::Dwelling => 60.0,
        SizeClass::Hamlet => 200.0,
        SizeClass::Village => 400.0,
        SizeClass::Town => 800.0,
        SizeClass::City => 1_500.0,
        SizeClass::Metropolis | SizeClass::Megalopolis => 4_000.0,
    };
    by_size.max(8.0 * f64::from(population).sqrt())
}
