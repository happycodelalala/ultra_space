//! Frame tree, 64-bit coordinates, orbits, tick scheduler.
//!
//! Engine-independent simulation code: no rendering or engine dependencies.
//! Uses standalone `bevy_ecs` / `bevy_tasks` for parallel scheduling.

pub mod clock;
pub mod region;
pub mod simulation;
pub mod stats;

pub use clock::FixedClock;
pub use region::{Region, RegionSet};
pub use simulation::Simulation;
pub use stats::RollingStats;
