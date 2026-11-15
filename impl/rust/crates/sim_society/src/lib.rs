//! Cultures, factions, settlements and population (Batch 4; design: living-planet.md).
//!
//! - [`lore`]: what the content director writes (culture profiles, faction and settlement
//!   lore, codex entries), as types it is validated against.
//! - [`society`]: a body's cultures and factions, generated from the star system data, the
//!   director's answers and the body's seed; [`names`] from each culture's naming rules.
//! - [`settlements`]: where each people's settlements are (sites scored by the body's terrain
//!   and climate), whose, what kind, and how many people in which roles.
//! - [`layout`]: each settlement's buildings and streets from its culture's building kit, and
//!   the foundations that level the ground under them.
//! - [`people`]: the people promoted near the player, living out their day on the streets.
//! - [`aggregate`]: every settlement's food, goods, mood and people, and faction relations,
//!   stepping in each body's region at any time warp.
//!
//! Engine-independent: no rendering or engine dependencies.

pub mod aggregate;
pub mod history;
pub mod layout;
pub mod lore;
pub mod names;
pub mod patterns;
pub mod people;
pub mod rng;
pub mod settlements;
pub mod society;

pub use aggregate::{RegionSociety, SettlementState};
pub use history::{Condition, HazardKind, History, HistoryEvent, Ruin, hazard_at};
pub use layout::{
    Edge, Field, Kit, KitBuilding, Kits, Layout, Placed, RegionLayouts, generate_layout,
};
pub use lore::{
    Attitude, CodexEntry, CultureProfile, FactionLore, NamingRules, Polity, Role, SettlementLore,
    TechTier,
};
pub use patterns::{SettlementPattern, SettlementPatterns};
pub use people::{Activity, Crowd, Person};
pub use settlements::{
    Settlement, SettlementRole, SizeClass, default_word, generate_settlements, settlement_word,
};
pub use society::{Culture, Faction, Society, generate};
