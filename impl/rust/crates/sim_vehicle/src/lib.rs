//! Vehicles: part catalog and designs, mass properties, thrust allocation and flight
//! (design: objects-vehicles-physics.md §2–4).
//!
//! Engine-independent simulation code: no rendering or engine dependencies.

pub mod allocation;
pub mod catalog;
pub mod cruise;
pub mod design;
pub mod flight;
pub mod mass;
pub mod vehicle;

pub use allocation::{Allocation, Allocator, Thruster, Wrench};
pub use catalog::{PartCatalog, PartDef, PartKind, Shape};
pub use cruise::{Cruise, CruiseBlocked, CruisePhase};
pub use design::{VehicleDesign, VehiclePart};
pub use flight::{AssistMode, FlightControl, FlightTelemetry, OnRails, Reference};
pub use mass::MassProperties;
pub use vehicle::{Performance, Vehicle, VehicleBody};
