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
    /// A hinge or rotor: what is attached beyond its `end` node turns about its axis.
    Joint,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JointType {
    /// Turns within angle limits.
    Hinge,
    /// Turns freely.
    Rotor,
}

/// A joint (task 3.10): parts attached beyond its `end` node turn about `axis` (part axes)
/// through that node.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JointDef {
    #[serde(rename = "type")]
    pub joint_type: JointType,
    pub axis: [f64; 3],
    /// Hinge limits (degrees).
    #[serde(default)]
    pub limits_deg: Option<[f64; 2]>,
    /// Fastest turn (degrees per second).
    pub speed_deg_s: f64,
    pub max_torque_nm: f64,
}

impl JointDef {
    pub fn axis(&self) -> DVec3 {
        DVec3::from(self.axis).normalize_or(DVec3::X)
    }

    /// `angle` (radians) kept within the hinge limits.
    pub fn clamp(&self, angle: f64) -> f64 {
        match (self.joint_type, self.limits_deg) {
            (JointType::Hinge, Some([lo, hi])) => angle.clamp(lo.to_radians(), hi.to_radians()),
            _ => angle,
        }
    }
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
    /// A curved shell: part of a surface of revolution about the part's Y axis. Its side
    /// profile is a smooth curve through four points (height, radius), top to bottom; it spans
    /// the azimuths `arc_deg` (0 along +Z, 90 along −X) and is `thickness_m` thick. A
    /// procedural shape: collisions use tiles that follow the curve ([`Shape::tiles`]).
    Shell {
        profile_m: [[f64; 2]; 4],
        arc_deg: [f64; 2],
        thickness_m: f64,
    },
}

/// A flat box piece of a shape's collision surface: centre and rotation in part axes, half
/// extents (X along the surface's width, Y along its height, Z its thickness).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tile {
    pub centre: DVec3,
    pub rotation: glam::DQuat,
    pub half: DVec3,
}

/// Steps per profile span (between two of the four profile points) for tiles and models.
pub const SHELL_PROFILE_STEPS: usize = 6;
/// Largest azimuth step of a shell's tiles (degrees).
pub const SHELL_ARC_STEP_DEG: f64 = 15.0;

/// A shell's side profile sampled top to bottom: (height, radius) on a Catmull-Rom curve
/// through the four points (end points repeated), `steps` samples per span. The game's model
/// generator (tools/gen-models.py) and the design viewer use the same curve.
pub fn shell_profile(points: [[f64; 2]; 4], steps: usize) -> Vec<[f64; 2]> {
    let p = |i: isize| points[i.clamp(0, 3) as usize];
    let mut out = Vec::new();
    for span in 0..3isize {
        let (p0, p1, p2, p3) = (p(span - 1), p(span), p(span + 1), p(span + 2));
        for k in 0..steps {
            let t = k as f64 / steps as f64;
            let (t2, t3) = (t * t, t * t * t);
            let c = |a: f64, b: f64, c: f64, d: f64| {
                0.5 * (2.0 * b
                    + (c - a) * t
                    + (2.0 * a - 5.0 * b + 4.0 * c - d) * t2
                    + (3.0 * b - a - 3.0 * c + d) * t3)
            };
            out.push([c(p0[0], p1[0], p2[0], p3[0]), c(p0[1], p1[1], p2[1], p3[1])]);
        }
    }
    out.push(points[3]);
    out
}

/// A point of a shell's mid-surface at azimuth `deg` and profile sample (height, radius).
fn shell_point(deg: f64, [y, r]: [f64; 2]) -> DVec3 {
    let phi = deg.to_radians();
    DVec3::new(-r * phi.sin(), y, r * phi.cos())
}

impl Shape {
    /// The collision tiles of a procedural shape (empty for the simple shapes, which collide
    /// as themselves).
    pub fn tiles(&self) -> Vec<Tile> {
        let Shape::Shell {
            profile_m,
            arc_deg,
            thickness_m,
        } = *self
        else {
            return Vec::new();
        };
        let profile = shell_profile(profile_m, SHELL_PROFILE_STEPS);
        let span = arc_deg[1] - arc_deg[0];
        let n = ((span.abs() / SHELL_ARC_STEP_DEG).ceil() as usize).max(1);
        let mut tiles = Vec::new();
        for i in 0..n {
            let (a, b) = (
                arc_deg[0] + span * i as f64 / n as f64,
                arc_deg[0] + span * (i + 1) as f64 / n as f64,
            );
            for w in profile.windows(2) {
                let (top, bottom) = (w[0], w[1]);
                let corners = [
                    shell_point(a, top),
                    shell_point(b, top),
                    shell_point(a, bottom),
                    shell_point(b, bottom),
                ];
                let centre = corners.iter().sum::<DVec3>() / 4.0;
                let across = (corners[1] + corners[3] - corners[0] - corners[2]) / 2.0;
                let down = (corners[2] + corners[3] - corners[0] - corners[1]) / 2.0;
                let x = across.normalize();
                let y = (-down - x * (-down).dot(x)).normalize();
                let mut z = x.cross(y);
                let radial = DVec3::new(centre.x, 0.0, centre.z);
                let (x, z) = if z.dot(radial) < 0.0 {
                    z = -z;
                    (-x, z)
                } else {
                    (x, z)
                };
                tiles.push(Tile {
                    centre,
                    rotation: glam::DQuat::from_mat3(&glam::DMat3::from_cols(x, y, z)).normalize(),
                    half: DVec3::new(
                        across.length() / 2.0,
                        down.length() / 2.0,
                        thickness_m / 2.0,
                    ),
                });
            }
        }
        tiles
    }

    /// The box around the shape in part axes: centre and half extents.
    pub fn bounds(&self) -> (DVec3, DVec3) {
        match *self {
            Shape::Box { size_m } => (DVec3::ZERO, DVec3::from(size_m) / 2.0),
            Shape::Cylinder { radius_m, length_m } => {
                (DVec3::ZERO, DVec3::new(radius_m, length_m / 2.0, radius_m))
            }
            Shape::Sphere { radius_m } => (DVec3::ZERO, DVec3::splat(radius_m)),
            Shape::Shell { .. } => {
                let (mut lo, mut hi) = (DVec3::splat(f64::MAX), DVec3::splat(f64::MIN));
                for t in self.tiles() {
                    for sx in [-1.0, 1.0] {
                        for sy in [-1.0, 1.0] {
                            for sz in [-1.0, 1.0] {
                                let c = t.centre + t.rotation * (t.half * DVec3::new(sx, sy, sz));
                                lo = lo.min(c);
                                hi = hi.max(c);
                            }
                        }
                    }
                }
                ((lo + hi) / 2.0, (hi - lo) / 2.0)
            }
        }
    }

    /// The centre of mass of the shape in part axes (uniform: the shell's area-weighted
    /// middle; the simple shapes' centres).
    pub fn centroid(&self) -> DVec3 {
        let tiles = self.tiles();
        if tiles.is_empty() {
            return DVec3::ZERO;
        }
        let area: f64 = tiles.iter().map(|t| t.half.x * t.half.y).sum();
        tiles
            .iter()
            .map(|t| t.centre * (t.half.x * t.half.y))
            .sum::<DVec3>()
            / area
    }
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
    pub joint: Option<JointDef>,
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

    /// All parts, in id order.
    pub fn iter(&self) -> impl Iterator<Item = &Arc<PartDef>> {
        self.parts.values()
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
        Shape::Shell {
            profile_m,
            arc_deg,
            thickness_m,
        } => {
            positive(thickness_m)
                && profile_m.iter().all(|[y, r]| y.is_finite() && *r >= 0.0)
                && profile_m.windows(2).all(|w| w[0][0] > w[1][0])
                && arc_deg[1] > arc_deg[0]
        }
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
    problems.extend(needs(p.joint.is_some(), "joint", PartKind::Joint));
    if p.joint.is_some() && (p.node("base").is_none() || p.node("end").is_none()) {
        problems.push(format!("{}: joints need \"base\" and \"end\" nodes", p.id));
    }
    if p.thruster.is_some_and(|t| !positive(t.max_thrust_n)) {
        problems.push(format!("{}: max_thrust_n must be positive", p.id));
    }
    if p.reaction_wheel.is_some_and(|w| !positive(w.max_torque_nm)) {
        problems.push(format!("{}: max_torque_nm must be positive", p.id));
    }
    problems
}
