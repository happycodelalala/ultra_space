//! Planet terrain, climate and biomes (Batch 2; design: planet-surface.md).
//!
//! Engine-independent simulation code: no rendering or engine dependencies. One deterministic
//! function per body gives heights, climate and biomes, shared by rendering, collision and
//! gameplay.

pub mod blocks;
pub mod climate;
pub mod foundations;
pub mod map;
pub mod mesh;
pub mod noise;
pub mod plains;
pub mod quadtree;
pub mod regions;
pub mod terrain;

pub use blocks::{BlockGround, VillageGround};
pub use climate::{Biome, Climate, latitude, subsolar_latitude};
pub use foundations::{Footprint, Foundations, PlacedBox, Site, Structure, tangent_frame};
pub use mesh::{CollisionPatch, TileMesh, build_tile, collision_patch, tile_indices};
pub use plains::{Plain, PlainKind, Plains};
pub use quadtree::{Extent, TileKey, View, estimated_heights, locate, select};
pub use regions::{PlainWorld, Region, RegionType};
pub use terrain::{Pad, Sample, Terrain, fibonacci_directions};
