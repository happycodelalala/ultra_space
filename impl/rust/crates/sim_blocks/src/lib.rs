//! The block world (Batch 5; design: block-world.md).
//!
//! Engine-independent: cells of one size per grid (a power of two of a metre), each holding a
//! palette colour; shapes (wedges, outer and valley corners) derived from the cells by the slope
//! rule and a model's recipe of "up" directions; a mesher that draws the faces between solid
//! and empty cells, merged into large rectangles and slope planes; chunked storage; and block
//! models loaded from the engine-agnostic files the authoring scripts export.

pub mod grid;
pub mod mesh;
pub mod model;
pub mod shape;

pub use grid::{CHUNK, Cells, ChunkMap, Grid};
pub use mesh::{MeshData, mesh};
pub use model::{BlockModel, ModelError, Palette};
pub use shape::{Dir, Shape, shapes};

/// The edge of a block of size exponent `e` in metres (2^e: -5 is 3.125 cm, 3 is 8 m).
pub fn block_size(e: i32) -> f64 {
    2f64.powi(e)
}
