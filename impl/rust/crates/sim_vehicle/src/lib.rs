//! Vehicles: part catalog and designs, mass properties, thrust allocation
//! (design: objects-vehicles-physics.md §2–3).
//!
//! Engine-independent simulation code: no rendering or engine dependencies.

pub mod allocation;
pub mod catalog;
pub mod design;
pub mod mass;
pub mod vehicle;

pub use allocation::{Allocation, Allocator, Thruster, Wrench};
pub use catalog::{PartCatalog, PartDef, PartKind, Shape};
pub use design::{VehicleDesign, VehiclePart};
pub use mass::MassProperties;
pub use vehicle::{Performance, Vehicle, VehicleBody};
