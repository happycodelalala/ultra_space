//! Planet terrain, climate and biomes (Batch 2; design: planet-surface.md).
//!
//! Engine-independent simulation code: no rendering or engine dependencies. One deterministic
//! function per body gives heights, climate and biomes, shared by rendering, collision and
//! gameplay.

pub mod climate;
pub mod noise;
pub mod terrain;

pub use climate::{Biome, Climate, latitude, subsolar_latitude};
pub use terrain::{Sample, Terrain, fibonacci_directions};
