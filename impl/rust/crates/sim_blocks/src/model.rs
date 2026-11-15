//! Block models: the engine-agnostic files the authoring scripts export (`tools/blocks/`,
//! block-world.md section 10), and the shared palette.
//!
//! A model is one or more parts (a house's 25 cm structure and its 6.25 cm fixtures), each a
//! dense box of cells in one block size with its own list of palette colour names, run-length
//! encoded (x fastest, then y, then z; index 0 is empty), its recipe of up directions and the
//! colours that glow.

use std::collections::HashMap;
use std::path::Path;

use glam::IVec3;
use serde::Deserialize;

use crate::grid::Grid;
use crate::mesh::{MeshData, mesh};
use crate::shape::Dir;

#[derive(Debug)]
pub enum ModelError {
    Io(String),
    Format(String),
}

impl std::fmt::Display for ModelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ModelError::Io(s) | ModelError::Format(s) => f.write_str(s),
        }
    }
}

impl std::error::Error for ModelError {}

#[derive(Deserialize)]
struct PartFile {
    size: f64,
    ups: Vec<String>,
    origin: [i32; 3],
    dims: [i32; 3],
    palette: Vec<String>,
    runs: Vec<u32>,
    #[serde(default)]
    glow: Vec<String>,
    #[serde(default)]
    reference: Option<Reference>,
}

/// What the authoring scripts' own mesher made of a part, to check this one against.
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Reference {
    /// Visible area per colour name (m²).
    pub area: HashMap<String, f64>,
}

#[derive(Deserialize)]
struct ModelFile {
    name: String,
    parts: Vec<PartFile>,
}

/// One part of a model: cells of one size.
#[derive(Clone, Debug)]
pub struct Part {
    /// Block edge (m).
    pub size: f64,
    pub ups: Vec<Dir>,
    /// Cells hold 1-based indices into `palette`.
    pub grid: Grid,
    pub palette: Vec<String>,
    pub glow: Vec<String>,
    pub reference: Option<Reference>,
}

impl Part {
    /// The part's mesh; vertex colours are 1-based indices into `palette`.
    pub fn mesh(&self) -> MeshData {
        mesh(
            &self.grid,
            self.grid.origin,
            self.grid.end(),
            &self.ups,
            self.size as f32,
        )
    }

    /// The palette name of a mesh colour index.
    pub fn colour_name(&self, index: u16) -> &str {
        &self.palette[index as usize - 1]
    }
}

#[derive(Clone, Debug)]
pub struct BlockModel {
    pub name: String,
    pub parts: Vec<Part>,
}

impl BlockModel {
    pub fn parse(text: &str) -> Result<Self, ModelError> {
        let f: ModelFile =
            serde_json::from_str(text).map_err(|e| ModelError::Format(e.to_string()))?;
        let mut parts = Vec::new();
        for (n, p) in f.parts.into_iter().enumerate() {
            let what = |s: &str| ModelError::Format(format!("{} part {n}: {s}", f.name));
            let ups = p
                .ups
                .iter()
                .map(|u| Dir::parse(u).ok_or_else(|| what(&format!("bad up {u:?}"))))
                .collect::<Result<Vec<_>, _>>()?;
            let mut grid = Grid::new(IVec3::from_array(p.origin), IVec3::from_array(p.dims));
            if p.runs.len() % 2 != 0 {
                return Err(what("odd run list"));
            }
            let mut at = 0usize;
            for run in p.runs.chunks(2) {
                let (count, index) = (run[0] as usize, run[1]);
                if index as usize > p.palette.len() {
                    return Err(what("palette index out of range"));
                }
                if at + count > grid.data.len() {
                    return Err(what("runs overflow the box"));
                }
                grid.data[at..at + count].fill(index as u16);
                at += count;
            }
            if at != grid.data.len() {
                return Err(what("runs don't fill the box"));
            }
            if !(p.size > 0.0 && p.size.log2().fract() == 0.0) {
                return Err(what("block size is not a power of two"));
            }
            parts.push(Part {
                size: p.size,
                ups,
                grid,
                palette: p.palette,
                glow: p.glow,
                reference: p.reference,
            });
        }
        Ok(Self {
            name: f.name,
            parts,
        })
    }

    pub fn load(path: &Path) -> Result<Self, ModelError> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| ModelError::Io(format!("{}: {e}", path.display())))?;
        Self::parse(&text)
    }
}

/// The shared palette: colour name to sRGB.
#[derive(Clone, Debug, Default)]
pub struct Palette {
    pub colours: HashMap<String, [u8; 3]>,
}

impl Palette {
    pub fn parse(text: &str) -> Result<Self, ModelError> {
        let colours: HashMap<String, [u8; 3]> =
            serde_json::from_str(text).map_err(|e| ModelError::Format(e.to_string()))?;
        Ok(Self { colours })
    }

    pub fn load(path: &Path) -> Result<Self, ModelError> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| ModelError::Io(format!("{}: {e}", path.display())))?;
        Self::parse(&text)
    }

    /// Linear RGB of a colour name (magenta when missing, so it shows).
    pub fn linear(&self, name: &str) -> [f32; 3] {
        let c = self.colours.get(name).copied().unwrap_or([255, 0, 255]);
        c.map(|v| (f32::from(v) / 255.0).powf(2.2))
    }
}
