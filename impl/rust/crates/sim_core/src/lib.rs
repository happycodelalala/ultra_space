//! Core simulation: frame tree and 64-bit coordinates, Kepler orbits, units, regions ticked in
//! parallel, the fixed-rate clock and timing statistics.
//!
//! Engine-independent: no rendering or engine dependencies. Uses standalone `bevy_ecs` /
//! `bevy_tasks` for parallel scheduling. Conventions: see [`units`].

pub mod clock;
pub mod frames;
pub mod objects;
pub mod orbit;
pub mod region;
pub mod simulation;
pub mod stats;
pub mod system;
pub mod time;
pub mod units;

pub use clock::FixedClock;
pub use frames::{Frame, FrameError, FrameId, FrameMotion, FrameState, FrameTree, Spin};
pub use objects::{Object, ObjectId, ObjectState, Objects};
pub use orbit::{KeplerOrbit, OrbitError};
pub use region::{Region, RegionSet};
pub use simulation::Simulation;
pub use stats::RollingStats;
pub use system::{Body, BodyKind, StarSystem, SystemError};
pub use time::{GAME_TIME_SCALE, GameDateTime, UniverseTime};
