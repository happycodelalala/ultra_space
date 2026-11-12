//! Part catalog: reusable part types loaded from `content/data/parts/*.json`
//! (design: objects-vehicles-physics.md §2.1–2.2).

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;
use std::sync::Arc;

use serde::Deserialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PartKind {
    Cockpit,
    Hull,
    Tank,
    Thruster,
    ReactionWheel,
    CruiseDrive,
}

/// Shape used for the part's inertia (and later collision).
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Shape {
    Box {
        size_m: [f64; 3],
    },
    /// Cylinder along the part's local Y axis.
    Cylinder {
        radius_m: f64,
        length_m: f64,
    },
    Sphere {
        radius_m: f64,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThrusterDef {
    /// Maximum force, pushing the vehicle along the part's local +Y axis.
    pub max_thrust_n: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReactionWheelDef {
    /// Maximum torque about any axis.
    pub max_torque_nm: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CruiseDriveDef {
    pub max_speed_ms: f64,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartDef {
    pub id: String,
    pub name: String,
    pub kind: PartKind,
    pub mass_kg: f64,
    pub shape: Shape,
    #[serde(default)]
    pub thruster: Option<ThrusterDef>,
    #[serde(default)]
    pub reaction_wheel: Option<ReactionWheelDef>,
    #[serde(default)]
    pub cruise_drive: Option<CruiseDriveDef>,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CatalogFile {
    #[serde(default)]
    #[allow(dead_code)]
    description: String,
    parts: Vec<PartDef>,
}

#[derive(Debug)]
pub enum CatalogError {
    Io(String),
    Json {
        file: String,
        error: serde_json::Error,
    },
    Invalid(Vec<String>),
}

impl fmt::Display for CatalogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "cannot read part catalog: {e}"),
            Self::Json { file, error } => write!(f, "part catalog {file} is malformed: {error}"),
            Self::Invalid(problems) => {
                write!(f, "invalid part catalog ({} problems):", problems.len())?;
                for p in problems {
                    write!(f, "\n  - {p}")?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for CatalogError {}

/// All known part types, by id.
#[derive(Clone, Debug, Default)]
pub struct PartCatalog {
    parts: BTreeMap<String, Arc<PartDef>>,
}

impl PartCatalog {
    pub fn get(&self, id: &str) -> Option<&Arc<PartDef>> {
        self.parts.get(id)
    }

    pub fn len(&self) -> usize {
        self.parts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.parts.is_empty()
    }

    /// Loads every `*.json` file in `dir` (sorted by name), checking all files before failing.
    pub fn load_dir(dir: impl AsRef<Path>) -> Result<Self, CatalogError> {
        let dir = dir.as_ref();
        let mut files: Vec<_> = std::fs::read_dir(dir)
            .map_err(|e| CatalogError::Io(format!("{}: {e}", dir.display())))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "json"))
            .collect();
        files.sort();
        let mut sources = Vec::new();
        for path in files {
            let text = std::fs::read_to_string(&path)
                .map_err(|e| CatalogError::Io(format!("{}: {e}", path.display())))?;
            sources.push((path.display().to_string(), text));
        }
        Self::from_sources(sources.iter().map(|(n, t)| (n.as_str(), t.as_str())))
    }

    /// Builds a catalog from (file name, JSON text) pairs.
    pub fn from_sources<'a>(
        sources: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> Result<Self, CatalogError> {
        let mut catalog = Self::default();
        let mut problems = Vec::new();
        for (file, text) in sources {
            let parsed: CatalogFile =
                serde_json::from_str(text).map_err(|error| CatalogError::Json {
                    file: file.to_owned(),
                    error,
                })?;
            for part in parsed.parts {
                problems.extend(validate(&part).into_iter().map(|p| format!("{file}: {p}")));
                let id = part.id.clone();
                if catalog.parts.insert(id.clone(), Arc::new(part)).is_some() {
                    problems.push(format!("{file}: duplicate part id {id:?}"));
                }
            }
        }
        if problems.is_empty() {
            Ok(catalog)
        } else {
            Err(CatalogError::Invalid(problems))
        }
    }
}

fn positive(x: f64) -> bool {
    x > 0.0 && x.is_finite()
}

fn validate(p: &PartDef) -> Vec<String> {
    let mut problems = Vec::new();
    if !positive(p.mass_kg) {
        problems.push(format!("{}: mass_kg must be positive", p.id));
    }
    let shape_ok = match p.shape {
        Shape::Box { size_m } => size_m.iter().all(|&s| positive(s)),
        Shape::Cylinder { radius_m, length_m } => positive(radius_m) && positive(length_m),
        Shape::Sphere { radius_m } => positive(radius_m),
    };
    if !shape_ok {
        problems.push(format!("{}: shape dimensions must be positive", p.id));
    }
    let needs = |present: bool, field: &str, kind: PartKind| {
        (p.kind == kind && !present)
            .then(|| format!("{}: {field} parts need a {field:?} block", p.id))
    };
    problems.extend(needs(p.thruster.is_some(), "thruster", PartKind::Thruster));
    problems.extend(needs(
        p.reaction_wheel.is_some(),
        "reaction_wheel",
        PartKind::ReactionWheel,
    ));
    problems.extend(needs(
        p.cruise_drive.is_some(),
        "cruise_drive",
        PartKind::CruiseDrive,
    ));
    if p.thruster.is_some_and(|t| !positive(t.max_thrust_n)) {
        problems.push(format!("{}: max_thrust_n must be positive", p.id));
    }
    if p.reaction_wheel.is_some_and(|w| !positive(w.max_torque_nm)) {
        problems.push(format!("{}: max_torque_nm must be positive", p.id));
    }
    problems
}
