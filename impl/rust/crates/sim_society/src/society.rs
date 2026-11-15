//! A body's society: its cultures (profiles from the content director) and their factions,
//! generated from the star system data and the body's seed (living-planet.md §3, steps 1 and
//! 3). Settlements (step 2, 4) come from [`crate`]'s settlement generation.

use ai_gateway::{ContentDirector, ContentRequest, Source};
use serde_json::json;
use sim_core::StarSystem;
use sim_core::system::PeopleData;

use crate::lore::{Attitude, CultureProfile, FactionLore, NamingRules, Polity, Role, TechTier};
use crate::names;
use crate::rng::Rng;

/// A faction: part of a culture, holding some of its settlements.
#[derive(Clone, Debug, PartialEq)]
pub struct Faction {
    /// `<culture>/<n>`.
    pub id: String,
    pub name: String,
    /// Index into [`Society::cultures`].
    pub culture: usize,
    /// Banner colour (sRGB 0–1).
    pub colour: [f64; 3],
    /// How many of the culture's settlements it holds (set when they are placed).
    pub settlements: u32,
    /// Its strength relative to its people's other factions (0.5–1.5): stronger factions hold
    /// more land.
    pub strength: f64,
    pub lore: FactionLore,
}

/// A culture living on the body, with where it lives.
#[derive(Clone, Debug, PartialEq)]
pub struct Culture {
    pub profile: CultureProfile,
    pub people: PeopleData,
}

/// A body's cultures, factions and the relations between factions.
#[derive(Clone, Debug, PartialEq)]
pub struct Society {
    pub body: usize,
    pub seed: u64,
    pub cultures: Vec<Culture>,
    pub factions: Vec<Faction>,
    /// Relations between factions, −100 (hostile) … +100 (allied), symmetric, by index.
    pub relations: Vec<Vec<i32>>,
    /// What the content director couldn't answer (fallbacks used).
    pub fallbacks: Vec<String>,
}

impl Society {
    pub fn relation(&self, a: usize, b: usize) -> i32 {
        self.relations[a][b]
    }

    pub fn culture_of(&self, faction: usize) -> &Culture {
        &self.cultures[self.factions[faction].culture]
    }
}

/// How an attitude to outsiders sets out relations (−100 … +100).
fn attitude_score(a: Attitude) -> i32 {
    match a {
        Attitude::Welcoming => 40,
        Attitude::Curious => 20,
        Attitude::Wary => -20,
        Attitude::Hostile => -60,
    }
}

/// The society of body `body`: one culture per entry in its `peoples`, each with its
/// factions. The same system, body and director answers give the same society.
pub fn generate(system: &StarSystem, body: usize, director: &ContentDirector) -> Society {
    let b = &system.bodies[body];
    let seed = b.terrain.as_ref().map_or(0, |t| t.seed) ^ fnv(&b.id);
    let mut fallbacks = Vec::new();
    let mut note = |source: &Source| {
        if let Source::Fallback(why) = source {
            fallbacks.push(why.clone());
        }
    };
    let mut cultures = Vec::new();
    let mut factions = Vec::new();
    for people in &b.peoples {
        let request = ContentRequest::new(
            "culture_profile",
            format!("culture/{}", people.culture),
            json!({"body": b.id}),
        );
        let answer = director.request(&request, || plain_profile(&people.culture));
        note(&answer.source);
        let profile = answer.value;
        let ci = cultures.len();
        let mut rng = Rng::derive(seed, &format!("factions/{}", people.culture));
        // Each faction's strength: stronger ones hold more land (settlements are placed later).
        let n = people.factions as usize;
        let weights: Vec<f64> = (0..n).map(|_| 0.5 + rng.unit()).collect();
        let mut used = Vec::new();
        for (k, &strength) in weights.iter().enumerate() {
            let name = loop {
                let root = names::word(&profile.naming, &mut rng);
                let name = names::faction(&profile.naming, &root, &mut rng);
                if !used.contains(&root) {
                    used.push(root);
                    break name;
                }
            };
            let id = format!("{}/{k}", people.culture);
            let request = ContentRequest::new(
                "faction_lore",
                format!("faction/{id}"),
                json!({"culture": profile.id, "name": name, "tech_tier": profile.tech_tier}),
            );
            let answer = director.request(&request, || FactionLore {
                motto: "We endure.".into(),
                description: "{faction} is one of the factions of {people}.".into(),
                history: "Little is recorded of its past.".into(),
            });
            note(&answer.source);
            let fill = |t: &str| {
                t.replace("{faction}", &name)
                    .replace("{people}", &profile.name)
                    .replace("{culture}", &profile.adjective)
            };
            let lore = FactionLore {
                motto: fill(&answer.value.motto),
                description: fill(&answer.value.description),
                history: fill(&answer.value.history),
            };
            let base = profile.palette[k % profile.palette.len()];
            let shade = 0.8 + 0.4 * rng.unit();
            factions.push(Faction {
                id,
                name,
                culture: ci,
                colour: base.map(|c| (c * shade).clamp(0.0, 1.0)),
                settlements: 0,
                strength,
                lore,
            });
        }
        cultures.push(Culture {
            profile,
            people: people.clone(),
        });
    }

    // Relations: factions of one culture start friendly, more so in an open culture; between
    // cultures, from both attitudes; each pair varies a little.
    let n = factions.len();
    let mut relations = vec![vec![100; n]; n];
    let mut rng = Rng::derive(seed, "relations");
    for i in 0..n {
        for j in (i + 1)..n {
            let (a, b) = (
                &cultures[factions[i].culture].profile,
                &cultures[factions[j].culture].profile,
            );
            let base = if factions[i].culture == factions[j].culture {
                40 + attitude_score(a.attitude) / 2
            } else {
                (attitude_score(a.attitude) + attitude_score(b.attitude)) / 2
            };
            let r = (base + rng.range(0, 30) as i32 - 15).clamp(-100, 100);
            relations[i][j] = r;
            relations[j][i] = r;
        }
    }
    Society {
        body,
        seed,
        cultures,
        factions,
        relations,
        fallbacks,
    }
}

/// A culture's profile when the director has none: plain, so the world still works.
fn plain_profile(id: &str) -> CultureProfile {
    CultureProfile {
        id: id.into(),
        name: format!("the {id} people"),
        adjective: id.into(),
        tech_tier: TechTier::Medieval,
        polity: Polity::Tribe,
        settlement_words: Default::default(),
        government: "Village elders.".into(),
        values: vec![],
        taboos: vec![],
        religion: "Unrecorded.".into(),
        attitude: Attitude::Wary,
        history: "Unrecorded.".into(),
        naming: NamingRules {
            onsets: ["b", "d", "k", "l", "m", "n", "r", "s", "t"]
                .map(String::from)
                .to_vec(),
            nuclei: ["a", "e", "i", "o", "u"].map(String::from).to_vec(),
            codas: ["", "n", "r", "s"].map(String::from).to_vec(),
            syllables: [2, 3],
            place_endings: vec![String::new()],
            place_patterns: vec!["{name}".into()],
            faction_patterns: vec!["{name} Clan".into()],
        },
        building_kit: "medieval_village".into(),
        clothing: "medieval_commoner".into(),
        palette: vec![[0.5, 0.45, 0.4]],
        roles: vec![Role {
            id: "farmer".into(),
            name: "farmer".into(),
            share: 1.0,
        }],
    }
}

fn fnv(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}
