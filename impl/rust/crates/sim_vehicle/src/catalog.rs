//! Part catalog: reusable part types loaded from `content/data/parts/*.json`
//! (design: objects-vehicles-physics.md §2.1–2.2).

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;
use std::sync::Arc;

use glam::DVec3;
use serde::Deserialize;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PartKind {
    Cockpit,
    #[default]
    Hull,
    Structure,
    Tank,
    Thruster,
    ReactionWheel,
    CruiseDrive,
    LandingLeg,
    Wheel,
    Battery,
}

/// Size class of attach nodes, connectors and slots: only matching sizes join.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SizeClass {
    Tiny,
    Small,
    Medium,
    Large,
    Huge,
}

/// A point where another part's node can join this one (part axes).
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttachNode {
    pub id: String,
    pub position_m: [f64; 3],
    /// Outward direction.
    pub direction: [f64; 3],
    pub size: SizeClass,
}

impl AttachNode {
    pub fn position(&self) -> DVec3 {
        DVec3::from(self.position_m)
    }

    pub fn direction(&self) -> DVec3 {
        DVec3::from(self.direction).normalize_or_zero()
    }
}

/// Interior passage through a connector.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Passage {
    #[default]
    None,
    Hatch,
    Door,
    Corridor,
    VehicleRamp,
}

/// A structural connector of a section or module (ships-stations-interiors §3.2).
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Connector {
    pub id: String,
    pub position_m: [f64; 3],
    pub direction: [f64; 3],
    pub size: SizeClass,
    pub profile: String,
    #[serde(default)]
    pub passage: Passage,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SlotType {
    Engine,
    Weapon,
    Hangar,
    Module,
    Utility,
}

/// A loadout slot on a hull (capital ships, Batch 11).
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Slot {
    pub id: String,
    #[serde(rename = "type")]
    pub slot_type: SlotType,
    pub size: SizeClass,
    #[serde(default)]
    pub position_m: Option<[f64; 3]>,
    #[serde(default)]
    pub direction: Option<[f64; 3]>,
}

/// How a part looks; never affects the simulation.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Form {
    pub model: String,
    #[serde(default)]
    pub material_slots: Vec<String>,
    #[serde(default)]
    pub style: String,
}

/// A space inside a part (interiors come after Batch 3).
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Space {
    pub id: String,
    pub kind: String,
    #[serde(default)]
    pub size_m: Option<[f64; 3]>,
    #[serde(default)]
    pub position_m: Option<[f64; 3]>,
}

/// Interior definition (ships-stations-interiors §5.1); in the format from Batch 3.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Interior {
    #[serde(default)]
    pub spaces: Vec<Space>,
    #[serde(default)]
    pub properties: serde_json::Map<String, serde_json::Value>,
}

#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TankDef {
    pub capacity_kg: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BatteryDef {
    pub capacity_wh: f64,
}

/// A leg reaching down along the part's local −Y axis to a foot.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LandingLegDef {
    pub length_m: f64,
    pub foot_radius_m: f64,
    #[serde(default)]
    pub stiffness_n_per_m: Option<f64>,
    #[serde(default)]
    pub damping_n_s_per_m: Option<f64>,
}

/// A driven wheel on suspension along the part's local −Y axis, rolling along −Z (task 3.5).
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WheelDef {
    pub radius_m: f64,
    pub width_m: f64,
    pub suspension_m: f64,
    pub stiffness_n_per_m: f64,
    pub damping_n_s_per_m: f64,
    pub max_motor_torque_nm: f64,
    #[serde(default)]
    pub max_steer_deg: f64,
    pub max_brake_torque_nm: f64,
    pub friction: f64,
}

/// Shape used for the part's inertia (and collision, unless the part has its own collider).
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

impl Default for Shape {
    fn default() -> Self {
        Self::Sphere { radius_m: 1.0 }
    }
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

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
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
    pub tank: Option<TankDef>,
    #[serde(default)]
    pub battery: Option<BatteryDef>,
    #[serde(default)]
    pub landing_leg: Option<LandingLegDef>,
    #[serde(default)]
    pub wheel: Option<WheelDef>,
    #[serde(default)]
    pub attach_nodes: Vec<AttachNode>,
    /// Can attach to other parts' surfaces (default true).
    #[serde(default)]
    pub surface_attach: Option<bool>,
    /// A control source (default: cockpits are).
    #[serde(default)]
    pub control: Option<bool>,
    /// Collision shape when different from the mass shape.
    #[serde(default)]
    pub collider: Option<Shape>,
    #[serde(default)]
    pub connectors: Vec<Connector>,
    #[serde(default)]
    pub slots: Vec<Slot>,
    #[serde(default)]
    pub form: Option<Form>,
    #[serde(default)]
    pub interior: Option<Interior>,
    #[serde(default)]
    pub description: String,
}

impl PartDef {
    pub fn node(&self, id: &str) -> Option<&AttachNode> {
        self.attach_nodes.iter().find(|n| n.id == id)
    }

    pub fn can_surface_attach(&self) -> bool {
        self.surface_attach.unwrap_or(true)
    }

    pub fn is_control(&self) -> bool {
        self.control.unwrap_or(self.kind == PartKind::Cockpit)
    }

    /// The shape used for collisions.
    pub fn collision_shape(&self) -> Shape {
        self.collider.unwrap_or(self.shape)
    }
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
    let shape_ok = |shape: Shape| match shape {
        Shape::Box { size_m } => size_m.iter().all(|&s| positive(s)),
        Shape::Cylinder { radius_m, length_m } => positive(radius_m) && positive(length_m),
        Shape::Sphere { radius_m } => positive(radius_m),
    };
    if !shape_ok(p.shape) || p.collider.is_some_and(|c| !shape_ok(c)) {
        problems.push(format!("{}: shape dimensions must be positive", p.id));
    }
    let mut node_ids = std::collections::BTreeSet::new();
    for node in &p.attach_nodes {
        if !node_ids.insert(node.id.as_str()) {
            problems.push(format!("{}: duplicate attach node {:?}", p.id, node.id));
        }
        if node.direction() == DVec3::ZERO {
            problems.push(format!(
                "{}: attach node {:?} needs a direction",
                p.id, node.id
            ));
        }
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
    problems.extend(needs(
        p.landing_leg.is_some(),
        "landing_leg",
        PartKind::LandingLeg,
    ));
    problems.extend(needs(p.wheel.is_some(), "wheel", PartKind::Wheel));
    problems.extend(needs(p.battery.is_some(), "battery", PartKind::Battery));
    if p.thruster.is_some_and(|t| !positive(t.max_thrust_n)) {
        problems.push(format!("{}: max_thrust_n must be positive", p.id));
    }
    if p.reaction_wheel.is_some_and(|w| !positive(w.max_torque_nm)) {
        problems.push(format!("{}: max_torque_nm must be positive", p.id));
    }
    problems
}
