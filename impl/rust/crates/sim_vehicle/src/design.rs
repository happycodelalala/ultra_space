//! Vehicle designs and instances (design: objects-vehicles-physics.md §2.1).
//!
//! A design (`content/data/vehicles/*.json`) places catalog parts in the vehicle frame
//! (forward −Z, up +Y, right +X). A vehicle instance carries its own copy of the part list,
//! so player edits (Batch 3) change the instance, never the design.

use std::fmt;
use std::path::Path;
use std::sync::Arc;

use glam::{DQuat, DVec3};
use serde::Deserialize;

use crate::catalog::{PartCatalog, PartDef};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DesignFile {
    id: String,
    name: String,
    #[serde(default)]
    description: String,
    parts: Vec<PlacedPartFile>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PlacedPartFile {
    part: String,
    position_m: [f64; 3],
    #[serde(default)]
    rotation_deg: [f64; 3],
}

/// Rotation from degrees about the fixed X, then Y, then Z axes.
pub fn rotation_from_degrees(deg: [f64; 3]) -> DQuat {
    let [x, y, z] = deg.map(f64::to_radians);
    (DQuat::from_rotation_z(z) * DQuat::from_rotation_y(y) * DQuat::from_rotation_x(x)).normalize()
}

/// One placed part: a catalog part at a position and orientation in the vehicle frame.
#[derive(Clone, Debug, PartialEq)]
pub struct VehiclePart {
    pub def: Arc<PartDef>,
    pub position: DVec3,
    pub rotation: DQuat,
}

#[derive(Clone, Debug, PartialEq)]
pub struct VehicleDesign {
    pub id: String,
    pub name: String,
    pub description: String,
    pub parts: Vec<VehiclePart>,
}

#[derive(Debug)]
pub enum DesignError {
    Io(String),
    Json(serde_json::Error),
    Invalid(Vec<String>),
}

impl fmt::Display for DesignError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "cannot read vehicle design: {e}"),
            Self::Json(e) => write!(f, "vehicle design is malformed: {e}"),
            Self::Invalid(problems) => {
                write!(f, "invalid vehicle design ({} problems):", problems.len())?;
                for p in problems {
                    write!(f, "\n  - {p}")?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for DesignError {}

impl VehicleDesign {
    pub fn from_path(path: impl AsRef<Path>, catalog: &PartCatalog) -> Result<Self, DesignError> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path)
            .map_err(|e| DesignError::Io(format!("{}: {e}", path.display())))?;
        Self::from_json_str(&text, catalog)
    }

    pub fn from_json_str(json: &str, catalog: &PartCatalog) -> Result<Self, DesignError> {
        let file: DesignFile = serde_json::from_str(json).map_err(DesignError::Json)?;
        let mut problems = Vec::new();
        if file.parts.is_empty() {
            problems.push(format!("{}: a vehicle needs at least one part", file.id));
        }
        let mut parts = Vec::with_capacity(file.parts.len());
        for (i, p) in file.parts.iter().enumerate() {
            let Some(def) = catalog.get(&p.part) else {
                problems.push(format!(
                    "{}: part #{i} uses unknown part {:?}",
                    file.id, p.part
                ));
                continue;
            };
            if p.position_m
                .iter()
                .chain(&p.rotation_deg)
                .any(|v| !v.is_finite())
            {
                problems.push(format!(
                    "{}: part #{i} has a non-finite position or rotation",
                    file.id
                ));
                continue;
            }
            parts.push(VehiclePart {
                def: def.clone(),
                position: DVec3::from(p.position_m),
                rotation: rotation_from_degrees(p.rotation_deg),
            });
        }
        if !problems.is_empty() {
            return Err(DesignError::Invalid(problems));
        }
        Ok(Self {
            id: file.id,
            name: file.name,
            description: file.description,
            parts,
        })
    }
}
