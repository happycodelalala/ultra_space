//! Cell storage: a dense box of cells, and chunks of them for large or unbounded volumes.
//!
//! A cell holds a palette index; 0 is empty. Positions are cell coordinates (integers) in the
//! grid's own frame.

use std::collections::HashMap;

use glam::IVec3;

/// Edge of a chunk in cells (block-world.md section 7).
pub const CHUNK: i32 = 32;

/// Anything cells can be read from.
pub trait Cells {
    /// The palette index at `p`, 0 when empty.
    fn get(&self, p: IVec3) -> u16;

    fn solid(&self, p: IVec3) -> bool {
        self.get(p) != 0
    }
}

/// A dense box of cells from `origin` (inclusive) spanning `dims`.
#[derive(Clone, Debug, PartialEq)]
pub struct Grid {
    pub origin: IVec3,
    pub dims: IVec3,
    pub data: Vec<u16>,
}

impl Grid {
    pub fn new(origin: IVec3, dims: IVec3) -> Self {
        let n = (dims.x.max(0) * dims.y.max(0) * dims.z.max(0)) as usize;
        Self {
            origin,
            dims,
            data: vec![0; n],
        }
    }

    fn index(&self, p: IVec3) -> Option<usize> {
        let q = p - self.origin;
        if q.cmplt(IVec3::ZERO).any() || q.cmpge(self.dims).any() {
            return None;
        }
        Some((q.x + self.dims.x * (q.y + self.dims.y * q.z)) as usize)
    }

    pub fn set(&mut self, p: IVec3, v: u16) {
        if let Some(i) = self.index(p) {
            self.data[i] = v;
        }
    }

    /// The highest corner (exclusive).
    pub fn end(&self) -> IVec3 {
        self.origin + self.dims
    }

    /// Number of solid cells.
    pub fn count(&self) -> usize {
        self.data.iter().filter(|&&v| v != 0).count()
    }
}

impl Cells for Grid {
    fn get(&self, p: IVec3) -> u16 {
        self.index(p).map_or(0, |i| self.data[i])
    }
}

/// Chunks of `CHUNK`³ cells keyed by chunk coordinate; cells outside every chunk are empty.
#[derive(Clone, Debug, Default)]
pub struct ChunkMap {
    pub chunks: HashMap<IVec3, Grid>,
}

impl ChunkMap {
    pub fn chunk_of(p: IVec3) -> IVec3 {
        p.div_euclid(IVec3::splat(CHUNK))
    }

    pub fn insert(&mut self, key: IVec3, grid: Grid) {
        self.chunks.insert(key, grid);
    }

    pub fn set(&mut self, p: IVec3, v: u16) {
        let key = Self::chunk_of(p);
        self.chunks
            .entry(key)
            .or_insert_with(|| Grid::new(key * CHUNK, IVec3::splat(CHUNK)))
            .set(p, v);
    }
}

impl Cells for ChunkMap {
    fn get(&self, p: IVec3) -> u16 {
        self.chunks.get(&Self::chunk_of(p)).map_or(0, |g| g.get(p))
    }
}

impl<F: Fn(IVec3) -> u16> Cells for F {
    fn get(&self, p: IVec3) -> u16 {
        self(p)
    }
}

/// The cells of `a` laid over `b` (where `a` is empty, `b` shows).
pub struct Overlay<'a, A: Cells, B: Cells>(pub &'a A, pub &'a B);

impl<A: Cells, B: Cells> Cells for Overlay<'_, A, B> {
    fn get(&self, p: IVec3) -> u16 {
        match self.0.get(p) {
            0 => self.1.get(p),
            v => v,
        }
    }
}

/// Cells collected from a sparse map (handy for tests and small models).
impl Cells for HashMap<IVec3, u16> {
    fn get(&self, p: IVec3) -> u16 {
        self.get(&p).copied().unwrap_or(0)
    }
}
