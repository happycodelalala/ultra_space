//! Cultures, factions, settlements and population (Batch 4; design: living-planet.md).
//!
//! - [`lore`]: what the content director writes (culture profiles, faction and settlement
//!   lore, codex entries), as types it is validated against.
//! - [`society`]: a body's cultures and factions, generated from the star system data, the
//!   director's answers and the body's seed; [`names`] from each culture's naming rules.
//! - [`settlements`]: where each people's settlements are (sites scored by the body's terrain
//!   and climate), whose, what kind, and how many people in which roles.
//!
//! Engine-independent: no rendering or engine dependencies.

pub mod lore;
pub mod names;
pub mod rng;
pub mod settlements;
pub mod society;

pub use lore::{
    Attitude, CodexEntry, CultureProfile, FactionLore, NamingRules, Role, SettlementLore, TechTier,
};
pub use settlements::{Settlement, SettlementKind, generate_settlements};
pub use society::{Culture, Faction, Society, generate};
