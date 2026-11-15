//! Settlement patterns (living-planet.md §8b): how a people settles its land, as data
//! (`content/data/settlement_patterns/*.json`). A pattern gives each faction's seat, then
//! classes of settlement that fill the settled land at their spacing, and the spacing of the
//! dwellings between them. A people's pattern is chosen by its polity first (a charter colony
//! settles as a young colony at any tier), else by its tech tier.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::lore::{Polity, TechTier};
use crate::settlements::{SettlementRole, SizeClass};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Seat {
    pub size: SizeClass,
    /// Fewest and most people.
    pub people: [u32; 2],
    pub roles: Vec<SettlementRole>,
    /// The people's capital (the first faction's seat), when it differs.
    #[serde(default)]
    pub capital_people: Option<[u32; 2]>,
    #[serde(default)]
    pub capital_roles: Vec<SettlementRole>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SettlementClass {
    pub size: SizeClass,
    pub spacing_km: f64,
    pub people: [u32; 2],
    pub roles: Vec<SettlementRole>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SettlementPattern {
    pub id: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub tiers: Vec<TechTier>,
    #[serde(default)]
    pub polities: Vec<Polity>,
    /// Share of the home region's usable land that is settled (the best sites first).
    pub settled_share: f64,
    pub seat: Seat,
    /// Biggest first.
    pub classes: Vec<SettlementClass>,
    /// Dwellings between settlements (decoration); none without it.
    #[serde(default)]
    pub dwelling_spacing_km: Option<f64>,
}

/// Every settlement pattern.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SettlementPatterns(pub Vec<SettlementPattern>);

impl SettlementPatterns {
    /// The patterns in `dir` (`*.json`).
    pub fn load_dir(dir: &Path) -> Result<Self, String> {
        let mut out = Vec::new();
        let mut paths: Vec<_> = std::fs::read_dir(dir)
            .map_err(|e| format!("{}: {e}", dir.display()))?
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "json"))
            .collect();
        paths.sort();
        for path in paths {
            let text =
                std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            let p: SettlementPattern =
                serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
            out.push(p);
        }
        Ok(Self(out))
    }

    /// A pattern by id.
    pub fn by_id(&self, id: &str) -> Option<&SettlementPattern> {
        self.0.iter().find(|p| p.id == id)
    }

    /// The pattern of a people of `tier` and `polity`: one listing its polity, else one listing
    /// its tier.
    pub fn for_people(&self, tier: TechTier, polity: Polity) -> Option<&SettlementPattern> {
        self.0
            .iter()
            .find(|p| p.polities.contains(&polity))
            .or_else(|| {
                self.0
                    .iter()
                    .find(|p| p.polities.is_empty() && p.tiers.contains(&tier))
            })
    }
}
