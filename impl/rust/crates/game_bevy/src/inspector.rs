//! In-game inspector (Batch 1, task 1.16): regions, the frame tree and one selected object.
//!
//! F4 toggles it (hidden at start); `[` and `]` select the previous or next object (the player
//! ship is selected first). `ULTRA_SPACE_INSPECTOR=1` shows it at start (useful with
//! capture). Read-only: it never changes the simulation.

use std::fmt::Write;
use std::time::Duration;

use bevy::prelude::*;
use glam::DVec3;
use sim_core::frames::FrameMotion;
use sim_core::{FrameId, FrameTree, ObjectId, Simulation, StarSystem};
use sim_vehicle::{Cruise, FlightControl, OnRails, Vehicle};

use crate::ship::PlayerShip;
use crate::sim::Sim;
use crate::universe::{InGame, Universe};

const REFRESH_INTERVAL: Duration = Duration::from_millis(250);

pub struct InspectorPlugin;

impl Plugin for InspectorPlugin {
    fn build(&self, app: &mut App) {
        let visible = std::env::var("ULTRA_SPACE_INSPECTOR").is_ok_and(|v| v == "1");
        app.insert_resource(Inspector {
            visible,
            selected: None,
            since_refresh: REFRESH_INTERVAL,
        })
        .add_systems(Startup, spawn_inspector)
        .add_systems(
            Update,
            (inspector_keys, update_inspector).chain().in_set(InGame),
        );
    }
}

#[derive(Resource, Debug)]
pub struct Inspector {
    pub visible: bool,
    /// Selected object; `None` selects the player ship.
    pub selected: Option<ObjectId>,
    since_refresh: Duration,
}

#[derive(Component)]
struct InspectorText;

fn spawn_inspector(mut commands: Commands, inspector: Res<Inspector>) {
    commands.spawn((
        InspectorText,
        Text::new(""),
        TextFont {
            font_size: FontSize::Px(12.0),
            ..default()
        },
        TextColor(Color::srgb(0.9, 0.95, 0.85)),
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.7)),
        if inspector.visible {
            Visibility::Visible
        } else {
            Visibility::Hidden
        },
        Node {
            position_type: PositionType::Absolute,
            top: px(8),
            left: px(8),
            padding: UiRect::all(px(6)),
            ..default()
        },
    ));
}

/// All object ids, sorted, so selection order is stable.
fn sorted_ids(sim: &Simulation) -> Vec<ObjectId> {
    let mut ids: Vec<_> = sim.objects.ids().collect();
    ids.sort();
    ids
}

/// The id `step` places after `current` in `ids`, wrapping around.
pub fn cycle(ids: &[ObjectId], current: Option<ObjectId>, step: isize) -> Option<ObjectId> {
    if ids.is_empty() {
        return None;
    }
    let n = ids.len() as isize;
    let at = current
        .and_then(|c| ids.iter().position(|&i| i == c))
        .map_or(0, |p| p as isize);
    Some(ids[(at + step).rem_euclid(n) as usize])
}

fn inspector_keys(
    keys: Res<ButtonInput<KeyCode>>,
    sim: Res<Sim>,
    ship: Option<Res<PlayerShip>>,
    mut inspector: ResMut<Inspector>,
    mut text: Query<&mut Visibility, With<InspectorText>>,
) {
    if keys.just_pressed(KeyCode::F4) {
        inspector.visible = !inspector.visible;
        inspector.since_refresh = REFRESH_INTERVAL;
        for mut v in &mut text {
            *v = if inspector.visible {
                Visibility::Visible
            } else {
                Visibility::Hidden
            };
        }
    }
    if !inspector.visible {
        return;
    }
    let step = if keys.just_pressed(KeyCode::BracketRight) {
        1
    } else if keys.just_pressed(KeyCode::BracketLeft) {
        -1
    } else {
        return;
    };
    let current = inspector.selected.or(ship.map(|s| s.id));
    inspector.selected = cycle(&sorted_ids(&sim.0), current, step);
    inspector.since_refresh = REFRESH_INTERVAL;
}

fn motion_label(motion: &FrameMotion) -> &'static str {
    match motion {
        FrameMotion::Fixed { .. } => "fixed",
        FrameMotion::Orbit(_) => "orbit",
        FrameMotion::Spin(_) => "spin",
    }
}

/// The frame tree as indented lines, depth first.
pub fn frame_tree_lines(frames: &FrameTree) -> Vec<String> {
    let mut lines = Vec::new();
    let mut stack: Vec<FrameId> = frames
        .iter()
        .filter(|(_, f)| f.parent.is_none())
        .map(|(id, _)| id)
        .collect();
    while let Some(id) = stack.pop() {
        let frame = frames.get(id).expect("listed frame");
        // A body's surface frame shares its line, to keep the tree short.
        let surface_name = format!("{}.surface", frame.name);
        let (surfaces, others): (Vec<_>, Vec<_>) = frames
            .children(id)
            .partition(|c| frames.get(*c).is_some_and(|f| f.name == surface_name));
        let surface = surfaces
            .first()
            .and_then(|c| frames.get(*c))
            .map_or(String::new(), |f| {
                format!("  + .surface ({})", motion_label(&f.motion))
            });
        lines.push(format!(
            "{}{} ({}){surface}",
            "  ".repeat(frame.depth() as usize),
            frame.name,
            motion_label(&frame.motion)
        ));
        // Push children in reverse so they come out in insertion order.
        stack.extend(others.into_iter().rev());
    }
    lines
}

fn vec_km(v: DVec3) -> String {
    format!(
        "({:.1}, {:.1}, {:.1}) km  |{:.1}|",
        v.x / 1e3,
        v.y / 1e3,
        v.z / 1e3,
        v.length() / 1e3
    )
}

fn vec_ms(v: DVec3) -> String {
    format!(
        "({:.1}, {:.1}, {:.1}) m/s  |{:.1}|",
        v.x,
        v.y,
        v.z,
        v.length()
    )
}

fn describe_object(sim: &Simulation, system: &StarSystem, id: ObjectId, out: &mut String) {
    let Some(loc) = sim.objects.location(id) else {
        let _ = writeln!(out, "object {id}: not found");
        return;
    };
    let Some(region) = sim.regions.get(loc.region) else {
        return;
    };
    let world = &region.world;
    let name = world
        .get::<Vehicle>(loc.entity)
        .map_or("object", |v| v.name.as_str());
    let _ = writeln!(out, "Selected: {name}  id {id}");
    let _ = writeln!(
        out,
        "  region {} ({})  entity {}",
        loc.region, region.name, loc.entity
    );
    let t = sim.universe_time().as_secs_f64();
    if let Some(state) = sim.objects.state(&sim.regions, id) {
        let frame = system
            .frames
            .get(state.frame)
            .map_or("?", |f| f.name.as_str());
        let _ = writeln!(out, "  frame {frame}");
        let _ = writeln!(out, "  pos {}", vec_km(state.state.position));
        let _ = writeln!(out, "  vel {}", vec_ms(state.state.velocity));
        let star = state.in_frame(&system.frames, system.star().inertial_frame, t);
        let _ = writeln!(out, "  star-frame pos {}", vec_km(star.state.position));
    }
    let _ = writeln!(
        out,
        "  on rails {}",
        if world.get::<OnRails>(loc.entity).is_some() {
            "yes"
        } else {
            "no"
        }
    );
    if let Some(cruise) = world.get::<Cruise>(loc.entity) {
        let _ = writeln!(out, "  cruise {:?}", cruise.phase);
    }
    if let Some(control) = world.get::<FlightControl>(loc.entity) {
        let held = match (control.desired_velocity, control.desired_body) {
            (Some((reference, v)), body) => format!(
                "{reference:?} rel. {} {}",
                body.map_or("?", |b| system.bodies[b].name.as_str()),
                vec_ms(v)
            ),
            (None, _) => "none".into(),
        };
        let _ = writeln!(out, "  assist {:?}, held velocity {held}", control.assist);
    }
}

fn update_inspector(
    real: Res<Time<Real>>,
    mut inspector: ResMut<Inspector>,
    mut sim: ResMut<Sim>,
    universe: Res<Universe>,
    ship: Option<Res<PlayerShip>>,
    mut text: Query<&mut Text, With<InspectorText>>,
) {
    inspector.since_refresh += real.delta();
    if !inspector.visible || inspector.since_refresh < REFRESH_INTERVAL {
        return;
    }
    inspector.since_refresh = Duration::ZERO;
    let system = &universe.0;
    let sim = &mut sim.0;

    let mut out = String::from("INSPECTOR (F4)   [ / ] select object\n\n");
    let mut objects_per_region = vec![0usize; sim.regions.len()];
    for id in sim.objects.ids() {
        if let Some(loc) = sim.objects.location(id) {
            objects_per_region[loc.region] += 1;
        }
    }
    let _ = writeln!(
        out,
        "Regions: {} (one per body), objects: {}; non-empty:",
        sim.regions.len(),
        sim.objects.len()
    );
    for (i, (region, objects)) in sim.regions.iter_mut().zip(&objects_per_region).enumerate() {
        let entities = region.entity_count();
        if *objects == 0 && entities == 0 {
            continue;
        }
        let _ = writeln!(
            out,
            "  {i} {:<10} objects {objects:<3} entities {entities}",
            region.name,
        );
    }

    let _ = writeln!(out, "\nFrames ({})", system.frames.len());
    for line in frame_tree_lines(&system.frames) {
        let _ = writeln!(out, "  {line}");
    }

    let ids = sorted_ids(sim);
    let selected = inspector
        .selected
        .filter(|id| sim.objects.location(*id).is_some())
        .or(ship.map(|s| s.id));
    out.push('\n');
    match selected {
        Some(id) => {
            let index = ids.iter().position(|&i| i == id).map_or(0, |p| p + 1);
            let _ = writeln!(out, "Object {index} of {}", ids.len());
            describe_object(sim, system, id, &mut out);
        }
        None => out.push_str("No objects\n"),
    }

    for mut t in &mut text {
        t.0 = out.trim_end().to_string();
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn cycling_wraps_both_ways() {
        let ids = [ObjectId(1), ObjectId(4), ObjectId(9)];
        assert_eq!(cycle(&ids, Some(ObjectId(4)), 1), Some(ObjectId(9)));
        assert_eq!(cycle(&ids, Some(ObjectId(9)), 1), Some(ObjectId(1)));
        assert_eq!(cycle(&ids, Some(ObjectId(1)), -1), Some(ObjectId(9)));
        assert_eq!(cycle(&ids, None, 1), Some(ObjectId(4)));
        assert_eq!(cycle(&[], None, 1), None);
    }

    #[test]
    fn frame_tree_lists_every_frame_under_its_parent() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../content/data/star_systems/halcyon.json");
        let sys = StarSystem::from_path(path).unwrap();
        let lines = frame_tree_lines(&sys.frames);
        // One line per body: its surface frame shares the line.
        assert_eq!(lines.len(), sys.bodies.len());
        assert!(lines.iter().all(|l| l.contains(".surface (spin)")));
        let pos = |name: &str| {
            lines
                .iter()
                .position(|l| l.trim_start().starts_with(&format!("{name} ")))
                .unwrap_or_else(|| panic!("{name} missing: {lines:#?}"))
        };
        assert!(pos("meridian") < pos("lantern"), "moon after its planet");
        let indent = |name: &str| lines[pos(name)].len() - lines[pos(name)].trim_start().len();
        assert!(indent("lantern") > indent("meridian"));
    }
}
