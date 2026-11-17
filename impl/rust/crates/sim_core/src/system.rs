//! Star system data (Batch 1, task 1.5).
//!
//! Loads an engine-neutral star system definition (`content/data/star_systems/*.json`,
//! schema `content/schemas/star_system.schema.json`) into a [`FrameTree`] plus body
//! properties. Field names carry human units (`_km`, `_deg`, `_hours`) so files are easy to
//! write by hand or generate with an LLM; everything is converted to SI on load.
//!
//! Frame layout per body: an inertial frame named `<id>` (orbit, no rotation) and a surface
//! frame named `<id>.surface` (spin). The star's inertial frame is the tree's root.

use std::collections::HashMap;
use std::fmt;
use std::path::Path;

use serde::Deserialize;

use crate::frames::{FrameId, FrameMotion, FrameTree, Spin};
use crate::orbit::KeplerOrbit;
use crate::units::{KM, deg, mu_from_surface_gravity};

// ---------------------------------------------------------------------------------------------
// File format (mirrors content/schemas/star_system.schema.json)
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SystemFile {
    id: String,
    name: String,
    #[serde(default)]
    description: String,
    bodies: Vec<BodyFile>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BodyFile {
    id: String,
    name: String,
    kind: BodyKind,
    #[serde(default)]
    parent: Option<String>,
    radius_km: f64,
    surface_gravity_ms2: f64,
    #[serde(default)]
    orbit: Option<OrbitFile>,
    rotation: RotationFile,
    #[serde(default)]
    atmosphere: Option<AtmosphereFile>,
    #[serde(default)]
    appearance: Option<AppearanceFile>,
    #[serde(default)]
    terrain: Option<TerrainFile>,
    #[serde(default)]
    climate: Option<ClimateFile>,
    #[serde(default)]
    peoples: Vec<PeopleData>,
    #[serde(default)]
    description: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TerrainFile {
    seed: u64,
    relief_km: f64,
    #[serde(default)]
    ocean_depth_km: f64,
    #[serde(default)]
    ocean_coverage: f64,
    #[serde(default)]
    mountains: f64,
    #[serde(default)]
    roughness: f64,
    #[serde(default)]
    craters: f64,
    #[serde(default)]
    plains: Option<PlainsFile>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PlainsFile {
    layout: PlainsLayout,
    edge_km: f64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ClimateFile {
    equator_temperature_c: f64,
    pole_temperature_c: f64,
    #[serde(default)]
    moisture: f64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AppearanceFile {
    colors: [[f64; 3]; 2],
    #[serde(default)]
    ocean_color: Option<[f64; 3]>,
    #[serde(default)]
    polar_ice: f64,
    #[serde(default)]
    glow_color: Option<[f64; 3]>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct OrbitFile {
    semi_major_axis_km: f64,
    eccentricity: f64,
    #[serde(default)]
    inclination_deg: f64,
    #[serde(default)]
    ascending_node_deg: f64,
    #[serde(default)]
    arg_periapsis_deg: f64,
    #[serde(default)]
    mean_anomaly_deg: f64,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RotationFile {
    Locked(LockedRotationFile),
    Spinning(SpinningRotationFile),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LockedRotationFile {
    tidally_locked: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SpinningRotationFile {
    /// Sidereal period in real hours; negative = retrograde.
    period_hours: f64,
    #[serde(default)]
    axial_tilt_deg: f64,
    #[serde(default)]
    phase_deg: f64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AtmosphereFile {
    height_km: f64,
    surface_pressure_atm: f64,
    color_rgb: [f64; 3],
}

// ---------------------------------------------------------------------------------------------
// Loaded system
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BodyKind {
    Star,
    Rocky,
    GasGiant,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Atmosphere {
    /// Height above the surface where the atmosphere ends (m).
    pub height: f64,
    pub surface_pressure_atm: f64,
    /// Linear RGB, 0–1, for rendering.
    pub color: [f64; 3],
}

/// A people living on a body (living-planet.md §3): which culture (its profile comes from the
/// content director, `culture/<culture>`), its home region and how many factions it has; how
/// many settlements follows from its settlement pattern and the land (living-planet.md §8b).
/// Interpreted by `sim_society`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PeopleData {
    pub culture: String,
    pub home: HomeRegion,
    pub factions: u32,
    /// Its settlement pattern, when not its tier's and polity's (a ruined nation settles as a
    /// wasteland's enclaves).
    #[serde(default)]
    pub pattern: Option<String>,
}

/// A region on a body's surface: a centre (latitude and longitude, degrees; longitude east
/// from the surface frame's +X) and an angular radius (degrees, so it fits any body size).
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HomeRegion {
    pub latitude_deg: f64,
    pub longitude_deg: f64,
    pub radius_deg: f64,
}

/// Parameters of a solid body's terrain (interpreted by `sim_planet`). SI units.
#[derive(Clone, Debug, PartialEq)]
pub struct TerrainData {
    pub seed: u64,
    /// Height of the tallest mountains above sea level (m).
    pub relief: f64,
    /// Depth of the deepest ocean floor below sea level (m).
    pub ocean_depth: f64,
    /// Fraction of the surface below sea level, 0–1.
    pub ocean_coverage: f64,
    /// How much of the land carries mountain ranges, 0–1.
    pub mountains: f64,
    /// Strength of hills and small-scale relief, 0–1.
    pub roughness: f64,
    /// Crater density, 0–1 (airless bodies).
    pub craters: f64,
    /// The body's ground as flat plains with regions (block-world.md §5.1); `None` for a
    /// spherical body (the heights above are then used).
    pub plains: Option<PlainsData>,
}

/// Layouts of flat plains.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlainsLayout {
    /// 12 pentagons and 20 hexagons (the "football").
    TruncatedIcosahedron,
}

/// A body built of flat plains.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlainsData {
    pub layout: PlainsLayout,
    /// Length of every plain's edge (m).
    pub edge: f64,
}

/// Climate parameters; rules in `sim_planet` compute temperature and moisture from them.
#[derive(Clone, Debug, PartialEq)]
pub struct ClimateData {
    /// Annual mean temperature at sea level on the equator (°C).
    pub equator_temperature_c: f64,
    /// Annual mean temperature at sea level at the poles (°C).
    pub pole_temperature_c: f64,
    /// Overall wetness, 0 (none) to 1 (very wet).
    pub moisture: f64,
}

/// How a body looks from a distance. Engine-neutral parameters a renderer interprets.
#[derive(Clone, Debug, PartialEq)]
pub struct Appearance {
    /// Low and high terrain colors (rocky) or band colors (gas giant), linear RGB 0–1.
    pub colors: [[f64; 3]; 2],
    /// Fraction of the surface covered by ocean, 0–1 (from the terrain data).
    pub ocean_coverage: f64,
    pub ocean_color: [f64; 3],
    /// Fraction of the pole-to-equator range covered by ice caps, 0–1.
    pub polar_ice: f64,
    /// Self-emitted light: the star's color, or glowing lava on a volcanic world.
    pub glow_color: Option<[f64; 3]>,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            colors: [[0.35, 0.35, 0.35], [0.6, 0.6, 0.6]],
            ocean_coverage: 0.0,
            ocean_color: DEFAULT_OCEAN_COLOR,
            polar_ice: 0.0,
            glow_color: None,
        }
    }
}

const DEFAULT_OCEAN_COLOR: [f64; 3] = [0.03, 0.12, 0.3];

#[derive(Clone, Debug, PartialEq)]
pub struct Body {
    pub id: String,
    pub name: String,
    pub kind: BodyKind,
    /// Index of the parent body in [`StarSystem::bodies`]; `None` for the star.
    pub parent: Option<usize>,
    pub radius: f64,
    /// Gravitational parameter (m³/s²).
    pub mu: f64,
    /// Sphere of influence radius (m); infinite for the star.
    pub soi_radius: f64,
    pub orbit: Option<KeplerOrbit>,
    pub spin: Spin,
    pub atmosphere: Option<Atmosphere>,
    pub appearance: Appearance,
    pub terrain: Option<TerrainData>,
    pub climate: Option<ClimateData>,
    /// Peoples living there (living-planet.md).
    pub peoples: Vec<PeopleData>,
    pub description: String,
    /// Non-rotating frame following the orbit.
    pub inertial_frame: FrameId,
    /// Rotating surface frame.
    pub surface_frame: FrameId,
}

impl Body {
    pub fn surface_gravity(&self) -> f64 {
        self.mu / (self.radius * self.radius)
    }

    /// Orbital period (s) around the parent; `None` for the star.
    pub fn orbital_period(&self) -> Option<f64> {
        self.orbit.map(|o| o.period())
    }
}

#[derive(Clone, Debug)]
pub struct StarSystem {
    pub id: String,
    pub name: String,
    pub description: String,
    pub frames: FrameTree,
    pub bodies: Vec<Body>,
}

impl StarSystem {
    pub fn body(&self, id: &str) -> Option<&Body> {
        self.bodies.iter().find(|b| b.id == id)
    }

    pub fn star(&self) -> &Body {
        &self.bodies[0]
    }

    /// Direct children of a body, in file order.
    pub fn children(&self, index: usize) -> impl Iterator<Item = &Body> {
        self.bodies.iter().filter(move |b| b.parent == Some(index))
    }

    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, SystemError> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path)
            .map_err(|e| SystemError::Io(format!("{}: {e}", path.display())))?;
        Self::from_json_str(&text)
    }

    pub fn from_json_str(json: &str) -> Result<Self, SystemError> {
        let file: SystemFile = serde_json::from_str(json).map_err(SystemError::Json)?;
        build(file)
    }
}

#[derive(Debug)]
pub enum SystemError {
    Io(String),
    Json(serde_json::Error),
    /// Every validation problem found, so data authors can fix them all at once.
    Invalid(Vec<String>),
}

impl fmt::Display for SystemError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "cannot read star system: {e}"),
            Self::Json(e) => write!(f, "star system data is malformed: {e}"),
            Self::Invalid(problems) => {
                write!(f, "invalid star system ({} problems):", problems.len())?;
                for p in problems {
                    write!(f, "\n  - {p}")?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for SystemError {}

/// Laplace sphere of influence of a body with `mu` orbiting a parent with `parent_mu`.
pub fn sphere_of_influence(semi_major_axis: f64, mu: f64, parent_mu: f64) -> f64 {
    semi_major_axis * (mu / parent_mu).powf(0.4)
}

/// Positive and finite (false for NaN).
fn positive(x: f64) -> bool {
    x > 0.0 && x.is_finite()
}

fn valid_id(id: &str) -> bool {
    let mut bytes = id.bytes();
    matches!(bytes.next(), Some(b'a'..=b'z'))
        && bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

fn build(file: SystemFile) -> Result<StarSystem, SystemError> {
    let mut problems = Vec::new();

    if !valid_id(&file.id) {
        problems.push(format!(
            "system id {:?} must match [a-z][a-z0-9_]*",
            file.id
        ));
    }
    let mut index_of = HashMap::new();
    for (i, b) in file.bodies.iter().enumerate() {
        if !valid_id(&b.id) {
            problems.push(format!("body id {:?} must match [a-z][a-z0-9_]*", b.id));
        }
        if index_of.insert(b.id.as_str(), i).is_some() {
            problems.push(format!("duplicate body id {:?}", b.id));
        }
        if !positive(b.radius_km) {
            problems.push(format!("{}: radius_km must be positive", b.id));
        }
        if !positive(b.surface_gravity_ms2) {
            problems.push(format!("{}: surface_gravity_ms2 must be positive", b.id));
        }
        if let Some(a) = &b.atmosphere {
            if !positive(a.height_km) {
                problems.push(format!("{}: atmosphere height_km must be positive", b.id));
            }
            if a.surface_pressure_atm.is_nan() || a.surface_pressure_atm < 0.0 {
                problems.push(format!(
                    "{}: atmosphere surface_pressure_atm must be >= 0",
                    b.id
                ));
            }
            if a.color_rgb.iter().any(|c| !(0.0..=1.0).contains(c)) {
                problems.push(format!(
                    "{}: atmosphere color_rgb values must be in [0, 1]",
                    b.id
                ));
            }
        }
        if let Some(a) = &b.appearance {
            let colors = a.colors.iter().chain(&a.ocean_color).chain(&a.glow_color);
            if colors.flatten().any(|c| !(0.0..=1.0).contains(c)) {
                problems.push(format!("{}: appearance colors must be in [0, 1]", b.id));
            }
            if !(0.0..=1.0).contains(&a.polar_ice) {
                problems.push(format!("{}: polar_ice must be in [0, 1]", b.id));
            }
        }
        if let Some(t) = &b.terrain {
            if b.kind != BodyKind::Rocky {
                problems.push(format!("{}: only rocky bodies have terrain", b.id));
            }
            if !positive(t.relief_km) || t.ocean_depth_km.is_nan() || t.ocean_depth_km < 0.0 {
                problems.push(format!(
                    "{}: terrain relief_km must be positive and ocean_depth_km >= 0",
                    b.id
                ));
            }
            let fractions = [t.ocean_coverage, t.mountains, t.roughness, t.craters];
            if fractions.iter().any(|f| !(0.0..=1.0).contains(f)) {
                problems.push(format!(
                    "{}: terrain ocean_coverage, mountains, roughness and craters must be in [0, 1]",
                    b.id
                ));
            }
            if t.ocean_coverage > 0.0 && !positive(t.ocean_depth_km) {
                problems.push(format!(
                    "{}: an ocean needs a positive ocean_depth_km",
                    b.id
                ));
            }
        }
        if let Some(c) = &b.climate {
            if !c.equator_temperature_c.is_finite()
                || !c.pole_temperature_c.is_finite()
                || c.pole_temperature_c > c.equator_temperature_c
            {
                problems.push(format!(
                    "{}: climate temperatures must be finite, poles no warmer than the equator",
                    b.id
                ));
            }
            if !(0.0..=1.0).contains(&c.moisture) {
                problems.push(format!("{}: climate moisture must be in [0, 1]", b.id));
            }
        }
        match &b.rotation {
            RotationFile::Locked(l) if !l.tidally_locked => problems.push(format!(
                "{}: rotation must be {{\"tidally_locked\": true}} or give period_hours",
                b.id
            )),
            RotationFile::Spinning(s) if s.period_hours == 0.0 || !s.period_hours.is_finite() => {
                problems.push(format!("{}: rotation period_hours must be non-zero", b.id))
            }
            _ => {}
        }
    }

    // Exactly one star: the only body without a parent, listed first.
    let roots: Vec<&BodyFile> = file.bodies.iter().filter(|b| b.parent.is_none()).collect();
    match roots.as_slice() {
        [star] => {
            if star.kind != BodyKind::Star {
                problems.push(format!(
                    "{}: the body without a parent must be a star",
                    star.id
                ));
            }
            if star.orbit.is_some() {
                problems.push(format!("{}: the star can't have an orbit", star.id));
            }
            if file.bodies.first().map(|b| b.id.as_str()) != Some(star.id.as_str()) {
                problems.push(format!("{}: the star must be the first body", star.id));
            }
            if matches!(star.rotation, RotationFile::Locked(_)) {
                problems.push(format!("{}: the star can't be tidally locked", star.id));
            }
        }
        [] => problems.push("no star: exactly one body must have no parent".into()),
        many => problems.push(format!(
            "{} bodies have no parent; exactly one (the star) may",
            many.len()
        )),
    }
    for b in &file.bodies {
        if let Some(parent) = &b.parent {
            if !index_of.contains_key(parent.as_str()) {
                problems.push(format!("{}: unknown parent {parent:?}", b.id));
            }
            if b.orbit.is_none() {
                problems.push(format!("{}: bodies with a parent need an orbit", b.id));
            }
            if b.kind == BodyKind::Star {
                problems.push(format!("{}: only the root body can be a star", b.id));
            }
        }
    }
    if !problems.is_empty() {
        return Err(SystemError::Invalid(problems));
    }

    // Order bodies so parents come before children (and catch cycles).
    let mut order = vec![0usize];
    let mut placed = vec![false; file.bodies.len()];
    placed[0] = true;
    while order.len() < file.bodies.len() {
        let before = order.len();
        for (i, b) in file.bodies.iter().enumerate() {
            if !placed[i]
                && let Some(p) = &b.parent
                && placed[index_of[p.as_str()]]
            {
                placed[i] = true;
                order.push(i);
            }
        }
        if order.len() == before {
            let stuck: Vec<&str> = file
                .bodies
                .iter()
                .enumerate()
                .filter(|(i, _)| !placed[*i])
                .map(|(_, b)| b.id.as_str())
                .collect();
            return Err(SystemError::Invalid(vec![format!(
                "parent cycle among bodies: {}",
                stuck.join(", ")
            )]));
        }
    }

    let star_file = &file.bodies[0];
    let mut frames = FrameTree::new(star_file.id.clone());
    let mut bodies: Vec<Option<Body>> = vec![None; file.bodies.len()];

    for &i in &order {
        let b = &file.bodies[i];
        let radius = b.radius_km * KM;
        let mu = mu_from_surface_gravity(radius, b.surface_gravity_ms2);
        let parent = b.parent.as_ref().map(|p| index_of[p.as_str()]);

        let (orbit, inertial_frame, soi_radius) = match (parent, &b.orbit) {
            (Some(p), Some(o)) => {
                let parent_body = bodies[p].as_ref().expect("parents placed first");
                let orbit = KeplerOrbit {
                    mu: parent_body.mu,
                    semi_major_axis: o.semi_major_axis_km * KM,
                    eccentricity: o.eccentricity,
                    inclination: deg(o.inclination_deg),
                    ascending_node: deg(o.ascending_node_deg),
                    arg_periapsis: deg(o.arg_periapsis_deg),
                    mean_anomaly_at_epoch: deg(o.mean_anomaly_deg),
                };
                if let Err(e) = orbit.validate() {
                    problems.push(format!("{}: {e}", b.id));
                    continue;
                }
                let clearance = parent_body.radius
                    + parent_body.atmosphere.as_ref().map_or(0.0, |a| a.height)
                    + radius;
                if orbit.periapsis() <= clearance {
                    problems.push(format!(
                        "{}: periapsis {:.0} km is inside {} (needs > {:.0} km including both radii and atmosphere)",
                        b.id,
                        orbit.periapsis() / KM,
                        parent_body.id,
                        clearance / KM
                    ));
                }
                if orbit.apoapsis() >= parent_body.soi_radius {
                    problems.push(format!(
                        "{}: apoapsis {:.0} km is outside {}'s sphere of influence ({:.0} km)",
                        b.id,
                        orbit.apoapsis() / KM,
                        parent_body.id,
                        parent_body.soi_radius / KM
                    ));
                }
                let frame = frames
                    .add(
                        b.id.clone(),
                        parent_body.inertial_frame,
                        FrameMotion::Orbit(orbit),
                    )
                    .map_err(|e| SystemError::Invalid(vec![e.to_string()]))?;
                let soi = sphere_of_influence(orbit.semi_major_axis, mu, parent_body.mu);
                (Some(orbit), frame, soi)
            }
            _ => (None, frames.root(), f64::INFINITY),
        };

        let spin = match (&b.rotation, &orbit) {
            (RotationFile::Locked(_), Some(o)) => Spin::tidally_locked(o),
            (RotationFile::Locked(_), None) => unreachable!("validated: star isn't locked"),
            (RotationFile::Spinning(s), _) => Spin::tilted(
                deg(s.axial_tilt_deg),
                s.period_hours * 3_600.0,
                deg(s.phase_deg),
            ),
        };
        let surface_frame = frames
            .add(
                format!("{}.surface", b.id),
                inertial_frame,
                FrameMotion::Spin(spin),
            )
            .map_err(|e| SystemError::Invalid(vec![e.to_string()]))?;

        if !b.peoples.is_empty() && b.terrain.is_none() {
            problems.push(format!("{}: peoples need a body with terrain", b.id));
        }
        for p in &b.peoples {
            let h = p.home;
            if !(-90.0..=90.0).contains(&h.latitude_deg)
                || !(h.radius_deg > 0.0 && h.radius_deg <= 90.0)
            {
                problems.push(format!(
                    "{}: {}'s home region is out of range",
                    b.id, p.culture
                ));
            }
            if p.factions == 0 {
                problems.push(format!(
                    "{}: {} needs at least one faction",
                    b.id, p.culture
                ));
            }
        }
        bodies[i] = Some(Body {
            id: b.id.clone(),
            name: b.name.clone(),
            kind: b.kind,
            parent,
            radius,
            mu,
            soi_radius,
            orbit,
            spin,
            atmosphere: b.atmosphere.as_ref().map(|a| Atmosphere {
                height: a.height_km * KM,
                surface_pressure_atm: a.surface_pressure_atm,
                color: a.color_rgb,
            }),
            appearance: b
                .appearance
                .as_ref()
                .map_or_else(Appearance::default, |a| Appearance {
                    colors: a.colors,
                    ocean_coverage: b.terrain.as_ref().map_or(0.0, |t| t.ocean_coverage),
                    ocean_color: a.ocean_color.unwrap_or(DEFAULT_OCEAN_COLOR),
                    polar_ice: a.polar_ice,
                    glow_color: a.glow_color,
                }),
            terrain: b.terrain.as_ref().map(|t| TerrainData {
                seed: t.seed,
                relief: t.relief_km * KM,
                ocean_depth: t.ocean_depth_km * KM,
                ocean_coverage: t.ocean_coverage,
                mountains: t.mountains,
                roughness: t.roughness,
                craters: t.craters,
                plains: t.plains.as_ref().map(|p| PlainsData {
                    layout: p.layout,
                    edge: p.edge_km * KM,
                }),
            }),
            climate: b.climate.as_ref().map(|c| ClimateData {
                equator_temperature_c: c.equator_temperature_c,
                pole_temperature_c: c.pole_temperature_c,
                moisture: c.moisture,
            }),
            peoples: b.peoples.clone(),
            description: b.description.clone(),
            inertial_frame,
            surface_frame,
        });
    }
    if !problems.is_empty() {
        return Err(SystemError::Invalid(problems));
    }

    Ok(StarSystem {
        id: file.id,
        name: file.name,
        description: file.description,
        frames,
        bodies: bodies
            .into_iter()
            .map(|b| b.expect("every body built"))
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use glam::DVec3;

    use super::*;
    use crate::time::solar_day;
    use crate::units::STANDARD_GRAVITY;

    fn halcyon_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../content/data/star_systems/halcyon.json")
    }

    fn halcyon() -> StarSystem {
        StarSystem::from_path(halcyon_path()).unwrap_or_else(|e| panic!("{e}"))
    }

    #[test]
    fn halcyon_loads_with_every_body() {
        let sys = halcyon();
        let ids: Vec<&str> = sys.bodies.iter().map(|b| b.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "halcyon",
                "cinder",
                "meridian",
                "lantern",
                "hoarfell",
                "leviathan",
                "forge",
                "rime"
            ]
        );
        assert_eq!(sys.star().kind, BodyKind::Star);
        // Two frames per body.
        assert_eq!(sys.frames.len(), 2 * sys.bodies.len());
        let meridian = sys.bodies.iter().position(|b| b.id == "meridian").unwrap();
        let moons: Vec<&str> = sys.children(meridian).map(|b| b.id.as_str()).collect();
        assert_eq!(moons, ["lantern"]);
    }

    #[test]
    fn meridian_matches_the_design_numbers() {
        // design/universe-scale-time-and-flight.md §1 and §4.
        let sys = halcyon();
        let m = sys.body("meridian").unwrap();
        assert!((m.radius - 600.0 * KM).abs() < 1e-6);
        assert!((m.surface_gravity() - 9.81).abs() < 1e-9);
        assert!(
            (m.spin.period - 5_400.0).abs() < 1e-9,
            "90-minute sidereal day"
        );
        let year = m.orbital_period().unwrap();
        let day = solar_day(m.spin.period, year);
        assert!((day - 5_400.0).abs() < 5.0, "solar day ≈ 90 min: {day}");
        // Self-consistent system (§4.3): a long year and a sphere of influence that holds
        // Lantern comfortably.
        let days = year / 86_400.0;
        assert!(
            (30.0..365.0).contains(&days),
            "Meridian year: {days:.1} real days"
        );
        let lantern = sys.body("lantern").unwrap();
        assert!(lantern.orbit.unwrap().apoapsis() < m.soi_radius / 2.0);
        assert!(m.soi_radius > 50_000.0 * KM, "{}", m.soi_radius / KM);
        // Low orbit ≈ 30 minutes, ≈ 2.3 km/s.
        let low = KeplerOrbit::circular(m.mu, m.radius + 70.0 * KM, 0.0, 0.0, 0.0);
        assert!((28.0..33.0).contains(&(low.period() / 60.0)));
        assert!(m.surface_gravity() > 0.9 * STANDARD_GRAVITY);
    }

    #[test]
    fn tidally_locked_moons_face_their_planet() {
        let sys = halcyon();
        for id in ["lantern", "forge", "rime"] {
            let moon = sys.body(id).unwrap();
            let parent = &sys.bodies[moon.parent.unwrap()];
            let facing = |t: f64| {
                sys.frames
                    .transform_point(moon.surface_frame, parent.inertial_frame, t, DVec3::ZERO)
                    .normalize()
            };
            let d0 = facing(0.0);
            for k in 1..8 {
                assert!(facing(k as f64 * 10_000.0).dot(d0) > 0.999_99, "{id}");
            }
        }
    }

    #[test]
    fn every_moon_fits_its_parent() {
        let sys = halcyon();
        for b in sys
            .bodies
            .iter()
            .filter(|b| b.parent.is_some_and(|p| p != 0))
        {
            let parent = &sys.bodies[b.parent.unwrap()];
            let o = b.orbit.unwrap();
            assert!(o.periapsis() > parent.radius + b.radius, "{}", b.id);
            assert!(o.apoapsis() < parent.soi_radius, "{}", b.id);
        }
    }

    fn minimal(bodies: &str) -> String {
        format!(r#"{{"id": "test", "name": "Test", "bodies": [{bodies}]}}"#)
    }

    const STAR: &str = r#"{"id": "sun", "name": "Sun", "kind": "star", "radius_km": 300000,
        "surface_gravity_ms2": 16.7, "rotation": {"period_hours": 600}}"#;

    fn problems(json: &str) -> Vec<String> {
        match StarSystem::from_json_str(json) {
            Err(SystemError::Invalid(p)) => p,
            other => panic!("expected validation problems, got {other:?}"),
        }
    }

    #[test]
    fn rejects_unknown_fields() {
        let bad = minimal(&STAR.replace("\"radius_km\"", "\"radius_kms\""));
        assert!(matches!(
            StarSystem::from_json_str(&bad),
            Err(SystemError::Json(_))
        ));
    }

    #[test]
    fn reports_all_problems_at_once() {
        let planet = |id: &str, parent: &str, a_km: f64| {
            format!(
                r#"{{"id": "{id}", "name": "X", "kind": "rocky", "parent": "{parent}",
                "radius_km": 100, "surface_gravity_ms2": 5,
                "orbit": {{"semi_major_axis_km": {a_km}, "eccentricity": 0.1}},
                "rotation": {{"period_hours": 2}}}}"#
            )
        };
        let json = minimal(
            &[
                STAR.to_string(),
                planet("p1", "nowhere", 1e7),
                planet("p1", "sun", 1e7),
                planet("Bad-Id", "sun", 1e7),
            ]
            .join(","),
        );
        let p = problems(&json);
        assert!(p.iter().any(|m| m.contains("unknown parent")), "{p:?}");
        assert!(p.iter().any(|m| m.contains("duplicate body id")), "{p:?}");
        assert!(p.iter().any(|m| m.contains("must match")), "{p:?}");
    }

    #[test]
    fn rejects_moons_outside_the_sphere_of_influence_or_inside_the_planet() {
        let body = |id: &str, parent: &str, a_km: f64, g: f64| {
            format!(
                r#"{{"id": "{id}", "name": "X", "kind": "rocky", "parent": "{parent}",
                "radius_km": 100, "surface_gravity_ms2": {g},
                "orbit": {{"semi_major_axis_km": {a_km}, "eccentricity": 0.0}},
                "rotation": {{"tidally_locked": true}}}}"#
            )
        };
        let far = minimal(
            &[
                STAR.into(),
                body("planet", "sun", 1e7, 9.8),
                body("moon", "planet", 5e6, 1.0),
            ]
            .join(","),
        );
        assert!(
            problems(&far)
                .iter()
                .any(|m| m.contains("sphere of influence"))
        );
        let inside = minimal(
            &[
                STAR.into(),
                body("planet", "sun", 1e7, 9.8),
                body("moon", "planet", 150.0, 1.0),
            ]
            .join(","),
        );
        assert!(
            problems(&inside)
                .iter()
                .any(|m| m.contains("is inside planet"))
        );
    }

    #[test]
    fn needs_exactly_one_star_first() {
        assert!(problems(&minimal("")).iter().any(|m| m.contains("no star")));
        let two = minimal(&[STAR, &STAR.replace("\"sun\"", "\"sun2\"")].join(","));
        assert!(problems(&two).iter().any(|m| m.contains("have no parent")));
    }
}
