//! Saving and loading the simulation (task 3.13; design: vehicle-builder.md §6).
//!
//! A [`SaveFile`] holds the format version, universe time, the object ID allocator, every
//! object as a record (vehicles as their instance part list in the design file format plus
//! flight, joint and walking state; walkers) and each body's terrain deltas (pads). Content is
//! referenced by id: a part missing from the catalog on load is reported and dropped with
//! everything attached to it. The game adds its own section (who the player controls, the
//! camera) under `player`.
//!
//! Frames and bodies are saved by name and id, not index, so a save survives content that
//! adds bodies. Each format version has a migration step to the next ([`migrate`]); a save
//! from a newer game is refused.

use std::sync::Arc;

use glam::{DQuat, DVec3};
use serde::{Deserialize, Serialize};
use sim_core::frames::FrameState;
use sim_core::objects::{ObjectId, ObjectState};
use sim_core::time::UniverseTime;
use sim_core::{Simulation, StarSystem};
use sim_planet::Pad;

use crate::catalog::PartCatalog;
use crate::design::{VehicleDesign, design_json};
use crate::flight::{AssistMode, FlightControl, FlightTelemetry, RegionTerrain};
use crate::gait::LegsOff;
use crate::joints::JointState;
use crate::vehicle::{Vehicle, VehicleBody};
use crate::walker::{Walker, WalkerControl};
use crate::{ArcadeFlight, Cruise};

/// The save format this build writes.
pub const SAVE_FORMAT: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SaveFile {
    pub format: u32,
    /// Universe time (ns since the epoch).
    pub universe_time_ns: u64,
    /// The next object ID to hand out.
    pub next_object_id: u64,
    pub objects: Vec<ObjectRecord>,
    /// Terrain deltas of each body that has any.
    #[serde(default)]
    pub terrain: Vec<BodyDeltas>,
    /// The game's own section (who the player controls, the camera).
    #[serde(default)]
    pub player: serde_json::Value,
}

/// One object: identity, where it is, and what it is.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ObjectRecord {
    pub id: u64,
    /// The body whose region holds it (body id).
    pub region: String,
    /// The frame its state is in (frame name).
    pub frame: String,
    pub position: [f64; 3],
    /// Quaternion xyzw.
    pub rotation: [f64; 4],
    pub velocity: [f64; 3],
    pub angular_velocity: [f64; 3],
    pub kind: ObjectKind,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ObjectKind {
    Vehicle(VehicleRecord),
    Walker(WalkerRecord),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VehicleRecord {
    /// The instance's parts, poses and gait in the design file format.
    pub design: serde_json::Value,
    /// Flight assist: "full", "rotation" or "off".
    pub assist: String,
    /// Arcade flight's held speed (m/s), when in arcade flight.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arcade_speed: Option<f64>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub legs_off: bool,
    /// Joint angles and targets (radians, one per rigid group), when jointed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub joints: Option<JointRecord>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct JointRecord {
    pub angles: Vec<f64>,
    pub targets: Vec<f64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub queue: Vec<Vec<f64>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WalkerRecord {
    pub velocity: [f64; 3],
    pub grounded: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BodyDeltas {
    pub body: String,
    pub pads: Vec<PadRecord>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PadRecord {
    /// Centre direction (unit, the body's surface frame).
    pub center: [f64; 3],
    pub radius: f64,
    pub blend: f64,
    pub height: f64,
}

#[derive(Debug)]
pub enum SaveError {
    Json(serde_json::Error),
    /// Written by a newer game.
    TooNew(u32),
    /// Older than any migration this build has.
    TooOld(u32),
    Invalid(String),
}

impl std::fmt::Display for SaveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Json(e) => write!(f, "save file is malformed: {e}"),
            Self::TooNew(v) => write!(
                f,
                "save format {v} is newer than this game's ({SAVE_FORMAT}): update the game"
            ),
            Self::TooOld(v) => write!(f, "save format {v} can no longer be loaded"),
            Self::Invalid(e) => write!(f, "save file is invalid: {e}"),
        }
    }
}

impl std::error::Error for SaveError {}

impl SaveFile {
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("save serialises")
    }

    /// Parses a save of any supported format, migrated to [`SAVE_FORMAT`].
    pub fn from_json(text: &str) -> Result<Self, SaveError> {
        let mut value: serde_json::Value = serde_json::from_str(text).map_err(SaveError::Json)?;
        migrate(&mut value)?;
        serde_json::from_value(value).map_err(SaveError::Json)
    }
}

/// Brings a save's JSON up to [`SAVE_FORMAT`], one version at a time (each format version adds
/// its step here). Until Checkpoint A the format may also be reset instead (development plan
/// §7.1).
pub fn migrate(save: &mut serde_json::Value) -> Result<(), SaveError> {
    let format = save
        .get("format")
        .and_then(|f| f.as_u64())
        .ok_or_else(|| SaveError::Invalid("no format version".into()))? as u32;
    if format > SAVE_FORMAT {
        return Err(SaveError::TooNew(format));
    }
    if format < 1 {
        return Err(SaveError::TooOld(format));
    }
    // Format 1 is the first: no steps yet.
    Ok(())
}

fn assist_name(mode: AssistMode) -> &'static str {
    match mode {
        AssistMode::Full => "full",
        AssistMode::Rotation => "rotation",
        AssistMode::Off => "off",
    }
}

fn assist_mode(name: &str) -> AssistMode {
    match name {
        "rotation" => AssistMode::Rotation,
        "off" => AssistMode::Off,
        _ => AssistMode::Full,
    }
}

/// Captures the simulation (the game fills in `player`). Objects are in ID order and pads in
/// body order, so the same state always gives the same file.
pub fn capture(sim: &Simulation, system: &StarSystem) -> SaveFile {
    let mut ids: Vec<ObjectId> = sim.objects.ids().collect();
    ids.sort();
    let mut objects = Vec::new();
    for id in ids {
        let Some(loc) = sim.objects.location(id) else {
            continue;
        };
        let Some(region) = sim.regions.get(loc.region) else {
            continue;
        };
        let world = &region.world;
        let Some(state) = world.get::<ObjectState>(loc.entity) else {
            continue;
        };
        let kind = if let Some(vehicle) = world.get::<Vehicle>(loc.entity) {
            let design = design_json(vehicle, &vehicle.design_id, &vehicle.name, "");
            let joints = world.get::<JointState>(loc.entity).map(|j| JointRecord {
                angles: j.angles.clone(),
                targets: j.targets.clone(),
                queue: j.queue.iter().cloned().collect(),
            });
            ObjectKind::Vehicle(VehicleRecord {
                design: serde_json::from_str(&design).expect("design JSON"),
                assist: assist_name(
                    world
                        .get::<FlightControl>(loc.entity)
                        .map(|c| c.assist)
                        .unwrap_or_default(),
                )
                .into(),
                arcade_speed: world.get::<ArcadeFlight>(loc.entity).map(|a| a.speed),
                legs_off: world.get::<LegsOff>(loc.entity).is_some(),
                joints,
            })
        } else if let Some(walker) = world.get::<Walker>(loc.entity) {
            ObjectKind::Walker(WalkerRecord {
                velocity: walker.velocity.to_array(),
                grounded: walker.grounded,
            })
        } else {
            continue;
        };
        let frame = system
            .frames
            .get(state.frame)
            .map(|f| f.name.clone())
            .unwrap_or_default();
        objects.push(ObjectRecord {
            id: id.0,
            region: system.bodies[loc.region].id.clone(),
            frame,
            position: state.state.position.to_array(),
            rotation: state.state.rotation.to_array(),
            velocity: state.state.velocity.to_array(),
            angular_velocity: state.state.angular_velocity.to_array(),
            kind,
        });
    }
    let terrain = system
        .bodies
        .iter()
        .enumerate()
        .filter_map(|(i, body)| {
            let terrain = sim
                .regions
                .get(i)?
                .world
                .get_resource::<RegionTerrain>()?
                .0
                .clone()?;
            (!terrain.pads().is_empty()).then(|| BodyDeltas {
                body: body.id.clone(),
                pads: terrain
                    .pads()
                    .iter()
                    .map(|p| PadRecord {
                        center: p.center.to_array(),
                        radius: p.radius,
                        blend: p.blend,
                        height: p.height,
                    })
                    .collect(),
            })
        })
        .collect();
    SaveFile {
        format: SAVE_FORMAT,
        universe_time_ns: sim.universe_time().as_nanos(),
        next_object_id: sim.objects.allocator().next(),
        objects,
        terrain,
        player: serde_json::Value::Null,
    }
}

/// `q` normalised if it isn't already (a saved rotation stays bit for bit).
fn unit(q: DQuat) -> DQuat {
    if (q.length_squared() - 1.0).abs() < 1e-12 {
        q
    } else {
        q.normalize()
    }
}

/// What a load restored and what it had to leave out.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LoadReport {
    /// Problems that dropped something (unknown parts, bodies or frames).
    pub problems: Vec<String>,
    /// Bodies whose terrain changed (index), so the renderer can rebuild their tiles.
    pub terrain_changed: Vec<usize>,
}

/// The design JSON with every part the catalog doesn't know dropped, together with the parts
/// attached to it (directly or further out); the problems say which.
fn drop_unknown_parts(
    design: &serde_json::Value,
    catalog: &PartCatalog,
    problems: &mut Vec<String>,
) -> serde_json::Value {
    let mut design = design.clone();
    let Some(parts) = design.get_mut("parts").and_then(|p| p.as_array_mut()) else {
        return design;
    };
    let mut dropped: std::collections::HashSet<String> = std::collections::HashSet::new();
    // Parents come before children in a design file, so one pass finds every subtree.
    for p in parts.iter() {
        let id = p.get("id").and_then(|v| v.as_str()).unwrap_or_default();
        let part = p.get("part").and_then(|v| v.as_str()).unwrap_or_default();
        let parent = p.get("parent").and_then(|v| v.as_str());
        if catalog.get(part).is_none() {
            problems.push(format!(
                "part {part:?} is no longer in the game: {id} dropped"
            ));
            dropped.insert(id.to_string());
        } else if parent.is_some_and(|parent| dropped.contains(parent)) {
            problems.push(format!("{id} dropped with the part it was attached to"));
            dropped.insert(id.to_string());
        }
    }
    parts.retain(|p| {
        let id = p.get("id").and_then(|v| v.as_str()).unwrap_or_default();
        !dropped.contains(id)
    });
    if dropped.is_empty() {
        return design;
    }
    // Poses (and staged moves) forget the dropped joints.
    for key in ["poses", "pose_stages"] {
        if let Some(map) = design.get_mut(key).and_then(|m| m.as_object_mut()) {
            for (_, entry) in map.iter_mut() {
                let angles: Vec<&mut serde_json::Value> = match entry {
                    serde_json::Value::Array(stages) => stages.iter_mut().collect(),
                    other => vec![other],
                };
                for a in angles {
                    if let Some(a) = a.as_object_mut() {
                        a.retain(|joint, _| !dropped.contains(joint));
                    }
                }
            }
        }
    }
    // A gait with a leg gone can't walk: it goes too.
    let lost_leg = design.get("gait").is_some_and(|g| {
        g.get("legs")
            .and_then(|l| l.as_array())
            .is_some_and(|legs| {
                legs.iter().any(|leg| {
                    leg.as_object().is_some_and(|leg| {
                        leg.values()
                            .any(|v| v.as_str().is_some_and(|name| dropped.contains(name)))
                    })
                })
            })
    });
    if lost_leg && let Some(d) = design.as_object_mut() {
        d.remove("gait");
        problems.push("it lost a leg: it can no longer walk".into());
    }
    design
}

/// Replaces the simulation's objects, time and terrain deltas with the save's. Each region's
/// terrain (from [`crate::flight::RegionTerrain`]) keeps its generated heights and gets the
/// saved pads. Things that no longer exist (parts, bodies, frames) are reported and left out.
pub fn restore(
    sim: &mut Simulation,
    system: &StarSystem,
    catalog: &PartCatalog,
    save: &SaveFile,
) -> LoadReport {
    let mut report = LoadReport::default();
    let body_index = |id: &str| system.bodies.iter().position(|b| b.id == id);

    sim.objects.clear(&mut sim.regions);
    sim.set_universe_time(UniverseTime::from_nanos(save.universe_time_ns));

    // Terrain deltas: every body with terrain gets exactly the saved pads (none if absent).
    for (i, _) in system.bodies.iter().enumerate() {
        let Some(region) = sim.regions.get_mut(i) else {
            continue;
        };
        let Some(current) = region
            .world
            .get_resource::<RegionTerrain>()
            .and_then(|t| t.0.clone())
        else {
            continue;
        };
        let pads: Vec<Pad> = save
            .terrain
            .iter()
            .find(|d| body_index(&d.body) == Some(i))
            .map(|d| {
                d.pads
                    .iter()
                    .map(|p| Pad {
                        center: DVec3::from_array(p.center).normalize(),
                        radius: p.radius,
                        blend: p.blend,
                        height: p.height,
                    })
                    .collect()
            })
            .unwrap_or_default();
        if current.pads() != pads.as_slice() {
            region
                .world
                .insert_resource(RegionTerrain(Some(Arc::new(current.with_pads(pads)))));
            report.terrain_changed.push(i);
        }
    }
    for d in &save.terrain {
        if body_index(&d.body).is_none() {
            report.problems.push(format!(
                "terrain changes on unknown body {:?} dropped",
                d.body
            ));
        }
    }

    for record in &save.objects {
        let Some(region) = body_index(&record.region) else {
            report.problems.push(format!(
                "object {} was on unknown body {:?}: dropped",
                record.id, record.region
            ));
            continue;
        };
        let Some(frame) = system.frames.find(&record.frame) else {
            report.problems.push(format!(
                "object {} was in unknown frame {:?}: dropped",
                record.id, record.frame
            ));
            continue;
        };
        let state = ObjectState {
            frame,
            state: FrameState {
                position: DVec3::from_array(record.position),
                rotation: unit(DQuat::from_array(record.rotation)),
                velocity: DVec3::from_array(record.velocity),
                angular_velocity: DVec3::from_array(record.angular_velocity),
            },
        };
        let id = ObjectId(record.id);
        let spawned = match &record.kind {
            ObjectKind::Walker(w) => sim.objects.spawn_with_id(
                &mut sim.regions,
                id,
                region,
                state,
                (
                    Walker {
                        velocity: DVec3::from_array(w.velocity),
                        grounded: w.grounded,
                        ground_altitude: 0.0,
                    },
                    WalkerControl::default(),
                ),
            ),
            ObjectKind::Vehicle(v) => {
                let design = drop_unknown_parts(&v.design, catalog, &mut report.problems);
                let design = match VehicleDesign::from_json_str(&design.to_string(), catalog) {
                    Ok(d) => d,
                    Err(e) => {
                        report
                            .problems
                            .push(format!("vehicle {} dropped: {e}", record.id));
                        continue;
                    }
                };
                let vehicle = Vehicle::from_design(&design);
                let Some(mut body) = VehicleBody::of(&vehicle) else {
                    report
                        .problems
                        .push(format!("vehicle {} has no mass left: dropped", record.id));
                    continue;
                };
                let joints = v.joints.as_ref().and_then(|j| {
                    let n = body.articulation.groups.len();
                    (body.articulation.is_jointed() && j.angles.len() == n && j.targets.len() == n)
                        .then(|| {
                            let mut s = JointState::rest(&body.articulation);
                            s.angles = j.angles.clone();
                            s.targets = j.targets.clone();
                            s.queue = j.queue.iter().filter(|q| q.len() == n).cloned().collect();
                            s.poses = body.articulation.poses(&s.angles);
                            s.generation += 1;
                            body.allocator = body.posed_allocator(&s.poses);
                            s
                        })
                });
                let control = FlightControl {
                    assist: assist_mode(&v.assist),
                    ..Default::default()
                };
                let result = sim.objects.spawn_with_id(
                    &mut sim.regions,
                    id,
                    region,
                    state,
                    (
                        vehicle,
                        body,
                        control,
                        FlightTelemetry::default(),
                        Cruise::default(),
                    ),
                );
                if result.is_ok()
                    && let Some(loc) = sim.objects.location(id)
                    && let Some(r) = sim.regions.get_mut(loc.region)
                {
                    let mut e = r.world.entity_mut(loc.entity);
                    if let Some(j) = joints {
                        e.insert(j);
                    }
                    if let Some(speed) = v.arcade_speed {
                        e.insert(ArcadeFlight { speed });
                    }
                    if v.legs_off {
                        e.insert(LegsOff);
                    }
                }
                result
            }
        };
        if let Err(e) = spawned {
            report
                .problems
                .push(format!("object {} not restored: {e}", record.id));
        }
    }
    sim.objects.resume_ids(save.next_object_id);
    report
}
