//! Vehicles: part catalog and designs, mass properties, thrust allocation and flight
//! (design: objects-vehicles-physics.md §2–4).
//!
//! Engine-independent simulation code: no rendering or engine dependencies.

pub mod allocation;
pub mod arcade;
pub mod builder;
pub mod catalog;
pub mod cruise;
pub mod design;
pub mod flight;
pub mod gait;
pub mod ground;
pub mod joints;
pub mod mass;
pub mod physics;
pub mod vehicle;
pub mod walker;
pub mod wheels;

pub use allocation::{Allocation, Allocator, Thruster, Wrench};
pub use arcade::ArcadeFlight;
pub use catalog::{PartCatalog, PartDef, PartKind, Shape};
pub use cruise::{Cruise, CruiseBlocked, CruisePhase};
pub use design::{Attach, VehicleDesign, VehiclePart};
pub use flight::{AssistMode, FlightControl, FlightTelemetry, Frozen, OnRails, Reference};
pub use mass::MassProperties;
pub use physics::{PhysicsScene, PhysicsStats, RegionPhysics};
pub use vehicle::{PartCollider, Performance, Vehicle, VehicleBody};
pub use walker::{Walker, WalkerControl};
pub use wheels::{Drive, WheelMount};
