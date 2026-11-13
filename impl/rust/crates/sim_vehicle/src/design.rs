//! Vehicle designs and instances (design: objects-vehicles-physics.md §2.1).
//!
//! A design (`content/data/vehicles/*.json`) places catalog parts in the vehicle frame
//! (forward −Z, up +Y, right +X) as a tree: the first part is the root, every other part names
//! its parent and joins it node to node or on its surface (vehicle-builder.md §3). A vehicle
//! instance carries its own copy of the part list, so player edits change the instance, never
//! the design.

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
    id: String,
    part: String,
    position_m: [f64; 3],
    #[serde(default)]
    rotation_deg: [f64; 3],
    #[serde(default)]
    parent: Option<String>,
    #[serde(default)]
    attach: Option<AttachFile>,
    #[serde(default)]
    symmetry: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum AttachFile {
    Surface(SurfaceWord),
    Node { node: String, to: String },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SurfaceWord {
    Surface,
}

/// Joined nodes must meet within this distance (m).
const NODE_TOLERANCE: f64 = 0.01;

/// Rotation from degrees about the fixed X, then Y, then Z axes.
pub fn rotation_from_degrees(deg: [f64; 3]) -> DQuat {
    let [x, y, z] = deg.map(f64::to_radians);
    (DQuat::from_rotation_z(z) * DQuat::from_rotation_y(y) * DQuat::from_rotation_x(x)).normalize()
}

/// How a part joins its parent.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum Attach {
    /// The root part (no parent).
    #[default]
    Root,
    /// Anywhere on (or inside) the parent's surface.
    Surface,
    /// This part's attach node `node` on the parent's node `to`.
    Node { node: String, to: String },
}

/// One placed part: a catalog part at a position and orientation in the vehicle frame, and
/// its place in the vehicle's attachment tree.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VehiclePart {
    /// Name within the vehicle (unique).
    pub name: String,
    pub def: Arc<PartDef>,
    pub position: DVec3,
    pub rotation: DQuat,
    /// Index of the parent part (always earlier in the list); `None` for the root.
    pub parent: Option<usize>,
    pub attach: Attach,
    /// Parts placed together by a symmetry tool share a group.
    pub symmetry: Option<String>,
}

impl VehiclePart {
    /// World (vehicle-frame) position and outward direction of one of this part's nodes.
    pub fn node_in_vehicle(&self, id: &str) -> Option<(DVec3, DVec3)> {
        let node = self.def.node(id)?;
        Some((
            self.position + self.rotation * node.position(),
            self.rotation * node.direction(),
        ))
    }
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

/// Checks that `part` can join `parent` the way it says: nodes exist, meet and face each other
/// with matching sizes; surface attachment is allowed.
pub fn check_join(part: &VehiclePart, parent: &VehiclePart) -> Option<String> {
    match &part.attach {
        Attach::Root => Some("only the root has no parent".into()),
        Attach::Surface => (!part.def.can_surface_attach())
            .then(|| format!("{} can't attach to surfaces", part.def.id)),
        Attach::Node { node, to } => {
            let (Some(a), Some(b)) = (part.def.node(node), parent.def.node(to)) else {
                return Some(format!("node {node:?} or parent node {to:?} doesn't exist"));
            };
            if a.size != b.size {
                return Some(format!("nodes {node:?} and {to:?} differ in size"));
            }
            let (pa, da) = part.node_in_vehicle(node)?;
            let (pb, db) = parent.node_in_vehicle(to)?;
            if pa.distance(pb) > NODE_TOLERANCE {
                return Some(format!(
                    "node {node:?} is {:.3} m from parent node {to:?}",
                    pa.distance(pb)
                ));
            }
            (da.dot(db) > -0.99).then(|| format!("nodes {node:?} and {to:?} don't face each other"))
        }
    }
}

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
        let mut parts: Vec<VehiclePart> = Vec::with_capacity(file.parts.len());
        let mut index = std::collections::HashMap::new();
        for (i, p) in file.parts.iter().enumerate() {
            let at = format!("{}: part {:?}", file.id, p.id);
            let Some(def) = catalog.get(&p.part) else {
                problems.push(format!("{at} uses unknown part {:?}", p.part));
                continue;
            };
            if p.position_m
                .iter()
                .chain(&p.rotation_deg)
                .any(|v| !v.is_finite())
            {
                problems.push(format!("{at} has a non-finite position or rotation"));
                continue;
            }
            if index.insert(p.id.clone(), parts.len()).is_some() {
                problems.push(format!("{at}: duplicate part id"));
                continue;
            }
            let parent = match (&p.parent, i) {
                (None, 0) => None,
                (None, _) => {
                    problems.push(format!(
                        "{at} needs a parent (only the first part is the root)"
                    ));
                    continue;
                }
                (Some(_), 0) => {
                    problems.push(format!("{at} is the root and can't have a parent"));
                    continue;
                }
                (Some(name), _) => match index.get(name) {
                    Some(&j) if j != parts.len() => Some(j),
                    _ => {
                        problems.push(format!("{at}: parent {name:?} must be listed before it"));
                        continue;
                    }
                },
            };
            let attach = match (&p.attach, parent) {
                (None, None) => Attach::Root,
                (Some(AttachFile::Surface(_)), Some(_)) => Attach::Surface,
                (Some(AttachFile::Node { node, to }), Some(_)) => Attach::Node {
                    node: node.clone(),
                    to: to.clone(),
                },
                _ => {
                    problems.push(format!("{at}: attach goes with parent"));
                    continue;
                }
            };
            let part = VehiclePart {
                name: p.id.clone(),
                def: def.clone(),
                position: DVec3::from(p.position_m),
                rotation: rotation_from_degrees(p.rotation_deg),
                parent,
                attach,
                symmetry: p.symmetry.clone(),
            };
            if let Some(j) = parent {
                problems.extend(check_join(&part, &parts[j]).map(|e| format!("{at}: {e}")));
            }
            parts.push(part);
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

/// Degrees about the fixed X, then Y, then Z axes (the inverse of [`rotation_from_degrees`]).
pub fn rotation_to_degrees(rotation: DQuat) -> [f64; 3] {
    let (z, y, x) = rotation.to_euler(glam::EulerRot::ZYX);
    [x, y, z].map(|a| {
        let d = a.to_degrees();
        // Tidy: whole degrees when within a millionth, no negative zero.
        let r = d.round();
        (if (d - r).abs() < 1e-6 { r } else { d }) + 0.0
    })
}

/// A vehicle's parts as a design file (`content/data/vehicles/*.json` format), one part per
/// line.
pub fn design_json(
    vehicle: &crate::vehicle::Vehicle,
    id: &str,
    name: &str,
    description: &str,
) -> String {
    let tidy = |v: f64| {
        let r = (v * 1e6).round() / 1e6;
        r + 0.0
    };
    let mut lines = vec![
        "{".to_string(),
        format!("  \"id\": {},", serde_json::to_string(id).expect("string")),
        format!(
            "  \"name\": {},",
            serde_json::to_string(name).expect("string")
        ),
        format!(
            "  \"description\": {},",
            serde_json::to_string(description).expect("string")
        ),
        "  \"parts\": [".to_string(),
    ];
    let n = vehicle.parts.len();
    let quoted = |text: &str| serde_json::to_string(text).expect("string");
    let numbers = |v: [f64; 3]| {
        let v = v.map(tidy);
        format!("[{}, {}, {}]", v[0], v[1], v[2])
    };
    for (i, p) in vehicle.parts.iter().enumerate() {
        let mut fields = vec![
            format!("\"id\": {}", quoted(&p.name)),
            format!("\"part\": {}", quoted(&p.def.id)),
            format!("\"position_m\": {}", numbers(p.position.to_array())),
        ];
        let rot = rotation_to_degrees(p.rotation);
        if rot != [0.0; 3] {
            fields.push(format!("\"rotation_deg\": {}", numbers(rot)));
        }
        if let Some(parent) = p.parent {
            fields.push(format!(
                "\"parent\": {}",
                quoted(&vehicle.parts[parent].name)
            ));
            fields.push(match &p.attach {
                Attach::Node { node, to } => format!(
                    "\"attach\": {{\"node\": {}, \"to\": {}}}",
                    quoted(node),
                    quoted(to)
                ),
                _ => "\"attach\": \"surface\"".into(),
            });
        }
        if let Some(group) = &p.symmetry {
            fields.push(format!("\"symmetry\": {}", quoted(group)));
        }
        let comma = if i + 1 < n { "," } else { "" };
        lines.push(format!("    {{{}}}{comma}", fields.join(", ")));
    }
    lines.push("  ]".into());
    lines.push("}".into());
    lines.join("\n") + "\n"
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn catalog() -> PartCatalog {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../../content/data");
        PartCatalog::load_dir(root.join("parts")).unwrap()
    }

    fn kestrel_json() -> String {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../../content/data");
        std::fs::read_to_string(root.join("vehicles/kestrel.json")).unwrap()
    }

    fn problems(json: &str) -> Vec<String> {
        match VehicleDesign::from_json_str(json, &catalog()) {
            Err(DesignError::Invalid(p)) => p,
            Err(e) => panic!("{e}"),
            Ok(_) => Vec::new(),
        }
    }

    #[test]
    fn kestrel_is_a_tree_rooted_at_the_cockpit() {
        let design = VehicleDesign::from_json_str(&kestrel_json(), &catalog()).unwrap();
        assert_eq!(design.parts[0].name, "cockpit");
        assert_eq!(design.parts[0].attach, Attach::Root);
        for (i, p) in design.parts.iter().enumerate().skip(1) {
            let parent = p.parent.expect("every other part has a parent");
            assert!(parent < i);
            assert!(check_join(p, &design.parts[parent]).is_none());
        }
        let engine = design
            .parts
            .iter()
            .find(|p| p.name == "main_engine")
            .unwrap();
        assert!(
            matches!(&engine.attach, Attach::Node { node, to } if node == "top" && to == "rear")
        );
    }

    #[test]
    fn rejects_broken_trees_and_joints() {
        let two = |second: &str| {
            format!(
                r#"{{"id": "t", "name": "T", "parts": [
                    {{"id": "a", "part": "hull.section.medium", "position_m": [0, 0, 0]}},
                    {second}
                ]}}"#
            )
        };
        // No parent.
        let p = problems(&two(
            r#"{"id": "b", "part": "hull.section.medium", "position_m": [0, 0, 3]}"#,
        ));
        assert!(p.iter().any(|e| e.contains("needs a parent")), "{p:?}");
        // Unknown parent.
        let p = problems(&two(
            r#"{"id": "b", "part": "hull.section.medium", "position_m": [0, 0, 3], "parent": "x", "attach": "surface"}"#,
        ));
        assert!(p.iter().any(|e| e.contains("listed before")), "{p:?}");
        // Nodes that don't meet.
        let p = problems(&two(
            r#"{"id": "b", "part": "hull.section.medium", "position_m": [0, 0, 3.5], "parent": "a", "attach": {"node": "front", "to": "rear"}}"#,
        ));
        assert!(p.iter().any(|e| e.contains("from parent node")), "{p:?}");
        // Nodes that meet.
        let p = problems(&two(
            r#"{"id": "b", "part": "hull.section.medium", "position_m": [0, 0, 3], "parent": "a", "attach": {"node": "front", "to": "rear"}}"#,
        ));
        assert!(p.is_empty(), "{p:?}");
        // Missing node.
        let p = problems(&two(
            r#"{"id": "b", "part": "hull.section.medium", "position_m": [0, 0, 3], "parent": "a", "attach": {"node": "nose", "to": "rear"}}"#,
        ));
        assert!(p.iter().any(|e| e.contains("doesn't exist")), "{p:?}");
    }

    #[test]
    fn designs_export_and_load_back_the_same() {
        let design = VehicleDesign::from_json_str(&kestrel_json(), &catalog()).unwrap();
        let v = crate::vehicle::Vehicle::from_design(&design);
        let json = design_json(&v, "kestrel_copy", "Kestrel copy", "");
        let back = VehicleDesign::from_json_str(&json, &catalog()).unwrap();
        let w = crate::vehicle::Vehicle::from_design(&back);
        assert_eq!(v.parts.len(), w.parts.len());
        for (a, b) in v.parts.iter().zip(&w.parts) {
            assert_eq!(a.name, b.name);
            assert_eq!(a.parent, b.parent);
            assert_eq!(a.attach, b.attach);
            assert!(a.position.distance(b.position) < 1e-6);
            assert!(a.rotation.dot(b.rotation).abs() > 1.0 - 1e-12, "{}", a.name);
        }
    }
}
