//! What the content director writes for the living planet (living-planet.md §1–2): types the
//! answers must parse into, and the rules they must follow. The same shapes are in
//! `content/schemas/lore.schema.json` for the recorded responses in `content/lore/`.
//!
//! The director writes text and picks among existing things (building kits, clothing sets,
//! roles of its tech tier); rules and seeds decide everything else (technical overview §13.2,
//! §13.9).

use ai_gateway::Validate;
use serde::{Deserialize, Serialize};

/// What a people can make, in historical order (living-planet.md §8b). Neutral names after
/// the eras; how a people is organised is its [`Polity`], and what one people calls another
/// ("barbarians", "sky-folk") belongs to its lore.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TechTier {
    /// Stone tools, fire, first farming, villages of huts.
    StoneAge,
    /// Bronze and iron, writing, walled cities.
    Ancient,
    /// Castles, mills, guilds.
    Medieval,
    /// Gunpowder, printing, ocean-going ships.
    EarlyModern,
    /// Steam to early electricity: factories, railways.
    Industrial,
    /// Electric, atomic, digital: cities of millions, aircraft.
    Modern,
    /// Between planets: landers, stations, colonies.
    Spacefaring,
    /// Between stars.
    Interstellar,
}

impl TechTier {
    /// Roles people of this tier can take.
    pub fn roles(self) -> &'static [&'static str] {
        match self {
            Self::StoneAge => &[
                "hunter", "gatherer", "crafter", "elder", "healer", "warrior",
            ],
            Self::Ancient => &[
                "farmer", "herder", "fisher", "laborer", "smith", "potter", "merchant", "soldier",
                "priest", "scribe", "noble",
            ],
            Self::Medieval => &[
                "farmer",
                "herder",
                "fisher",
                "laborer",
                "smith",
                "carpenter",
                "merchant",
                "guard",
                "priest",
                "healer",
                "noble",
            ],
            Self::EarlyModern => &[
                "farmer",
                "herder",
                "fisher",
                "laborer",
                "smith",
                "carpenter",
                "merchant",
                "sailor",
                "soldier",
                "priest",
                "scholar",
                "noble",
            ],
            Self::Industrial => &[
                "farmer", "worker", "engineer", "merchant", "clerk", "soldier", "doctor",
                "teacher", "official",
            ],
            Self::Modern => &[
                "farmer",
                "worker",
                "engineer",
                "merchant",
                "clerk",
                "soldier",
                "doctor",
                "teacher",
                "official",
                "scientist",
                "pilot",
            ],
            Self::Spacefaring | Self::Interstellar => &[
                "technician",
                "engineer",
                "scientist",
                "grower",
                "medic",
                "pilot",
                "security",
                "administrator",
                "trader",
                "laborer",
            ],
        }
    }

    /// Building kits made for this tier.
    pub fn building_kits(self) -> &'static [&'static str] {
        match self {
            Self::Medieval => &["medieval_village"],
            Self::Spacefaring => &["colony_standin"],
            _ => &[],
        }
    }

    /// Clothing sets made for this tier.
    pub fn clothing_sets(self) -> &'static [&'static str] {
        match self {
            Self::Medieval => &["medieval_commoner"],
            Self::Spacefaring => &["colony_workwear"],
            _ => &[],
        }
    }
}

/// How a people is organised, independent of its tech tier (a Stone Age chiefdom, an Ancient
/// tribal confederation, a Spacefaring charter colony). It adjusts the settlement pattern: an
/// empire has bigger capitals than a chiefdom.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Polity {
    Band,
    Tribe,
    Chiefdom,
    /// A kingdom or realm of lords.
    Realm,
    Empire,
    CityState,
    Republic,
    Nation,
    /// A colony under a charter (a company's or a government's).
    CharterColony,
    Federation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Attitude {
    Welcoming,
    Curious,
    Wary,
    Hostile,
}

/// How a culture names people, places and factions: syllables built from onsets, nuclei and
/// codas, and patterns for places and factions (`{name}` a generated name, `{ending}` one of
/// the place endings).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NamingRules {
    pub onsets: Vec<String>,
    pub nuclei: Vec<String>,
    pub codas: Vec<String>,
    /// Fewest and most syllables in a generated name.
    pub syllables: [u32; 2],
    pub place_endings: Vec<String>,
    pub place_patterns: Vec<String>,
    pub faction_patterns: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Role {
    /// One of the tech tier's roles ([`TechTier::roles`]).
    pub id: String,
    /// What the culture calls it.
    pub name: String,
    /// Share of the population (the shares add up to 1).
    pub share: f64,
}

/// A culture's profile (technical overview §6, §13.2).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CultureProfile {
    pub id: String,
    /// The people's name ("the Hollin").
    pub name: String,
    /// The adjective ("Hollish").
    pub adjective: String,
    pub tech_tier: TechTier,
    pub polity: Polity,
    /// How it is governed, in words.
    pub government: String,
    pub values: Vec<String>,
    pub taboos: Vec<String>,
    pub religion: String,
    pub attitude: Attitude,
    /// A history summary, a paragraph or two.
    pub history: String,
    pub naming: NamingRules,
    /// One of the tier's building kits.
    pub building_kit: String,
    /// One of the tier's clothing sets.
    pub clothing: String,
    /// Colours for banners, paint and cloth (sRGB 0–1).
    pub palette: Vec<[f64; 3]>,
    pub roles: Vec<Role>,
}

impl Validate for CultureProfile {
    fn validate(&self) -> Result<(), String> {
        let mut problems = Vec::new();
        for (what, text) in [
            ("id", &self.id),
            ("name", &self.name),
            ("adjective", &self.adjective),
            ("government", &self.government),
            ("religion", &self.religion),
            ("history", &self.history),
        ] {
            if text.trim().is_empty() {
                problems.push(format!("{what} is empty"));
            }
        }
        if !self
            .tech_tier
            .building_kits()
            .contains(&self.building_kit.as_str())
        {
            problems.push(format!(
                "building kit {:?} isn't one for a {:?} culture ({:?})",
                self.building_kit,
                self.tech_tier,
                self.tech_tier.building_kits()
            ));
        }
        if !self
            .tech_tier
            .clothing_sets()
            .contains(&self.clothing.as_str())
        {
            problems.push(format!(
                "clothing {:?} isn't one for a {:?} culture",
                self.clothing, self.tech_tier
            ));
        }
        let allowed = self.tech_tier.roles();
        for r in &self.roles {
            if !allowed.contains(&r.id.as_str()) {
                problems.push(format!(
                    "role {:?} doesn't exist at the {:?} tier",
                    r.id, self.tech_tier
                ));
            }
            if !(r.share > 0.0 && r.share <= 1.0) {
                problems.push(format!("role {:?} has share {}", r.id, r.share));
            }
        }
        let total: f64 = self.roles.iter().map(|r| r.share).sum();
        if (total - 1.0).abs() > 0.01 {
            problems.push(format!("role shares add up to {total:.3}, not 1"));
        }
        let n = &self.naming;
        if n.onsets.is_empty() || n.nuclei.is_empty() || n.codas.is_empty() {
            problems.push("naming needs onsets, nuclei and codas".into());
        }
        if n.syllables[0] == 0 || n.syllables[0] > n.syllables[1] {
            problems.push(format!("naming syllables {:?} aren't a range", n.syllables));
        }
        if n.place_patterns.is_empty() || n.faction_patterns.is_empty() {
            problems.push("naming needs place and faction patterns".into());
        }
        for p in n.place_patterns.iter().chain(&n.faction_patterns) {
            if !p.contains("{name}") {
                problems.push(format!("pattern {p:?} has no {{name}}"));
            }
        }
        if self.palette.is_empty()
            || self
                .palette
                .iter()
                .flatten()
                .any(|c| !(0.0..=1.0).contains(c))
        {
            problems.push("palette needs colours in 0–1".into());
        }
        if problems.is_empty() {
            Ok(())
        } else {
            Err(problems.join("; "))
        }
    }
}

/// A faction's lore.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FactionLore {
    pub motto: String,
    pub description: String,
    pub history: String,
}

impl Validate for FactionLore {
    fn validate(&self) -> Result<(), String> {
        non_empty(&[
            ("motto", &self.motto),
            ("description", &self.description),
            ("history", &self.history),
        ])
    }
}

/// A settlement's lore line and its landmark.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SettlementLore {
    pub description: String,
    pub landmark: String,
}

impl Validate for SettlementLore {
    fn validate(&self) -> Result<(), String> {
        non_empty(&[
            ("description", &self.description),
            ("landmark", &self.landmark),
        ])
    }
}

/// A codex entry: something the player can read about.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CodexEntry {
    pub title: String,
    pub body: String,
}

impl Validate for CodexEntry {
    fn validate(&self) -> Result<(), String> {
        non_empty(&[("title", &self.title), ("body", &self.body)])
    }
}

fn non_empty(fields: &[(&str, &String)]) -> Result<(), String> {
    let empty: Vec<&str> = fields
        .iter()
        .filter(|(_, v)| v.trim().is_empty())
        .map(|(k, _)| *k)
        .collect();
    if empty.is_empty() {
        Ok(())
    } else {
        Err(format!("empty: {}", empty.join(", ")))
    }
}
