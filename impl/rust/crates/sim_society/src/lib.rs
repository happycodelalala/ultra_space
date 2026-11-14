//! Cultures, factions, settlements and population (Batch 4; design: living-planet.md).
//!
//! - [`lore`]: what the content director writes (culture profiles, faction and settlement
//!   lore, codex entries), as types it is validated against.
//!
//! Engine-independent: no rendering or engine dependencies.

pub mod lore;

pub use lore::{
    Attitude, CodexEntry, CultureProfile, FactionLore, NamingRules, Role, SettlementLore, TechTier,
};
