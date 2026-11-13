//! The Bastion: an original heavy frame (~24 m, ~180 t) carried in a cocoon of four curved
//! quarter-shells, each on a shell arm. Each arm: a thick shoulder link from a saddle on the
//! shoulder up under the dome; an elbow; the carrier the quarter is fixed to; and from the
//! carrier a long folding leg (eight segments, thinner and thinner, ending in a grapple) that
//! lies folded flat inside its quarter. The shoulder lift and the elbow turn about parallel
//! axes, so swinging the link out while the elbow turns back moves a quarter outward without
//! tilting it: walking, the quarters are held clear of the legs. Closed, the quarters make a
//! cocoon over the head, shoulders and arm bases, tapering to just above the ankles. Every pose
//! and every move between poses is checked free of interference.
//! `content/data/vehicles/bastion.json` is this design; refresh it with
//! `cargo test -p sim_vehicle --test bastion -- --ignored`.

use std::path::PathBuf;
use std::sync::Arc;

use glam::{DMat3, DQuat, DVec3};
use sim_vehicle::builder::{
    FreeNode, Symmetry, attach_at_node, attach_to_surface, free_nodes, place_symmetric, stats,
};
use sim_vehicle::catalog::{SHELL_PROFILE_STEPS, Shape, shell_profile};
use sim_vehicle::design::{NamedPose, check_join, design_json};
use sim_vehicle::gait::{GaitDef, LegDef};
use sim_vehicle::interference::{Interference, interferences};
use sim_vehicle::joints::{Articulation, JointState};
use sim_vehicle::{PartCatalog, PartDef, Vehicle, VehicleBody, VehicleDesign, VehiclePart};

fn content() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../../content/data")
}

const NAME: &str = "Bastion";
const DESCRIPTION: &str = "An original heavy frame, about 24 m tall, in a cocoon of four curved \
     quarter-shells. Each quarter rides on a shell arm from the shoulder (a thick link, an elbow \
     and the carrier it is fixed to) and hides a long folding leg with a grapple; walking, the \
     arms hold the quarters clear of the legs. Forward is -Z.";

/// The quarters on the left: name and the azimuth of its middle (0 behind, 90 left, 180 in
/// front, degrees). The arm's parts and saddle node carry the same name.
const QUARTERS: [(&str, f64); 2] = [("front", 135.0), ("back", 45.0)];
/// Which way along the curve each quarter's leg folds (from its centreline end toward the
/// side): +1 along [`tangent_left`], −1 against it.
fn fold_direction(quarter: &str) -> f64 {
    if quarter == "front" { 1.0 } else { -1.0 }
}
/// Folding-leg segments after the drop (each 8 m), side by side when folded.
const LEG_SEGMENTS: usize = 7;

/// The cocoon wraps around this vertical axis (vehicle frame, x = 0).
const SHELL_AXIS_Z: f64 = 0.3;
/// Where the shoulder saddles sit (left; on top of the shoulder block, clear of the torso).
const SADDLE_AT: DVec3 = DVec3::new(-4.0, 2.9, 0.0);
/// Overlap allowed between parts that aren't joined (m): the seams between neighbouring
/// quarters.
const TOLERANCE: f64 = 0.05;
/// Height of each engine pair above (upper) or below (lower) the centre of mass, the spacing
/// within a pair (m), and the azimuth they sit at (degrees).
const ENGINE_GAP: f64 = 4.0;
const ENGINE_SPREAD: f64 = 2.0;
const ENGINE_AZIMUTH: f64 = 30.0;
/// How far the walking pose swings the shoulder links out at most (degrees).
const WALK_LIMIT: f64 = 110.0;

/// Outward direction of the left side at azimuth `deg`: 0 behind (+Z), 90 left (−X), 180 in
/// front (−Z).
fn radial_left(deg: f64) -> DVec3 {
    let phi = deg.to_radians();
    DVec3::new(-phi.sin(), 0.0, phi.cos())
}

/// Along the cocoon's curve at azimuth `deg` (left side), towards larger azimuths' far side.
fn tangent_left(deg: f64) -> DVec3 {
    DVec3::Y.cross(radial_left(deg)).normalize()
}

struct Builder {
    v: Vehicle,
    catalog: PartCatalog,
}

impl Builder {
    fn part(&self, id: &str) -> Arc<PartDef> {
        self.catalog
            .get(id)
            .unwrap_or_else(|| panic!("no part {id}"))
            .clone()
    }

    fn index(&self, name: &str) -> usize {
        self.v
            .parts
            .iter()
            .position(|p| p.name == name)
            .unwrap_or_else(|| panic!("no part {name}"))
    }

    fn free(&self, parent: &str, node: &str) -> FreeNode {
        let p = self.index(parent);
        free_nodes(&self.v)
            .into_iter()
            .find(|n| n.part == p && n.node == node)
            .unwrap_or_else(|| panic!("{parent}.{node} is free"))
    }

    /// Attaches part `id` by its node `own` to `parent`'s node `to`, turned about the node to
    /// maximise `score` of the placed part (whole degrees). Returns its index.
    fn join_best(
        &mut self,
        parent: &str,
        to: &str,
        id: &str,
        own: &str,
        name: &str,
        score: impl Fn(&VehiclePart) -> f64,
    ) -> usize {
        let at = self.free(parent, to);
        let def = self.part(id);
        let mut best = (f64::MIN, 0.0);
        for d in 0..360 {
            let roll = f64::from(d).to_radians();
            let i = attach_at_node(&mut self.v, &def, own, &at, roll).unwrap();
            let s = score(&self.v.parts[i]);
            self.v.parts.pop();
            if s > best.0 {
                best = (s, roll);
            }
        }
        let i = attach_at_node(&mut self.v, &def, own, &at, best.1).unwrap();
        self.v.parts[i].name = name.into();
        i
    }

    /// Mirrors the just-placed part `i` to the right side as `name`.
    fn mirror(&mut self, i: usize, name: &str) {
        assert_eq!(
            place_symmetric(&mut self.v, i, Symmetry::Mirror),
            1,
            "{name}"
        );
        let last = self.v.parts.len() - 1;
        self.v.parts[last].name = name.into();
    }

    /// Joins on the left side (turned to maximise `score`) and mirrors to the right: names
    /// `{base}_left` and `{base}_right`; the parent is `{parent}_left` (or `parent` when it
    /// has no side).
    fn pair(
        &mut self,
        parent: &str,
        to: &str,
        id: &str,
        own: &str,
        base: &str,
        score: impl Fn(&VehiclePart) -> f64,
    ) {
        let parent = if self.v.parts.iter().any(|p| p.name == parent) {
            parent.to_string()
        } else {
            format!("{parent}_left")
        };
        let i = self.join_best(&parent, to, id, own, &format!("{base}_left"), score);
        self.mirror(i, &format!("{base}_right"));
    }
}

/// A hinge's axis lies along `want` (left side).
fn axis_along(want: DVec3) -> impl Fn(&VehiclePart) -> f64 {
    move |p: &VehiclePart| (p.rotation * DVec3::X).dot(want)
}

/// Any turn will do (straight beams).
fn any(_: &VehiclePart) -> f64 {
    0.0
}

/// The quarter's profile radius at height `y` (m; linear between the curve's samples).
fn profile_radius(shape: &Shape, y: f64) -> f64 {
    let Shape::Shell { profile_m, .. } = *shape else {
        panic!("not a shell")
    };
    let samples = shell_profile(profile_m, SHELL_PROFILE_STEPS * 4);
    for w in samples.windows(2) {
        let ([y0, r0], [y1, r1]) = (w[0], w[1]);
        if (y1..=y0).contains(&y) {
            return r0 + (r1 - r0) * (y - y0) / (y1 - y0);
        }
    }
    panic!("height {y} outside the shell")
}

/// Assembles the Bastion with the builder's operations.
fn assemble() -> Vehicle {
    let catalog = PartCatalog::load_dir(content().join("parts")).unwrap();
    let torso = catalog.get("bastion.torso").unwrap().clone();
    let mut b = Builder {
        v: Vehicle {
            design_id: "bastion".into(),
            name: NAME.into(),
            parts: vec![VehiclePart {
                name: "torso".into(),
                def: torso,
                ..Default::default()
            }],
            ..Default::default()
        },
        catalog,
    };

    // Body: the Bastion's heavier head, waist, arms and legs (the walking body's layout).
    b.join_best("torso", "neck", "bastion.head", "neck", "head", any);
    b.join_best("torso", "waist", "bastion.waist", "top", "waist", any);
    let i = b.join_best(
        "torso",
        "shoulder_left",
        "bastion.shoulder",
        "right",
        "shoulder_left",
        any,
    );
    b.mirror(i, "shoulder_right");
    b.pair(
        "shoulder",
        "down",
        "frame.hinge",
        "base",
        "shoulder_joint",
        any,
    );
    b.pair(
        "shoulder_joint",
        "end",
        "bastion.limb",
        "top",
        "upper_arm",
        any,
    );
    b.pair(
        "upper_arm",
        "bottom",
        "frame.hinge",
        "base",
        "elbow_joint",
        any,
    );
    b.pair("elbow_joint", "end", "bastion.limb", "top", "forearm", any);
    b.pair("waist", "hip_left", "frame.hinge", "base", "hip_joint", any);
    b.pair("hip_joint", "end", "bastion.leg", "top", "thigh", any);
    b.pair("thigh", "bottom", "frame.hinge", "base", "knee_joint", any);
    b.pair("knee_joint", "end", "bastion.leg", "top", "shin", any);
    b.pair("shin", "bottom", "frame.hinge", "base", "ankle_joint", any);
    b.pair("ankle_joint", "end", "bastion.foot", "top", "foot", any);

    // Each side by hand (several arm parts are handed: their mirror image is a different
    // part, which the symmetry tool can't make by turning a copy).
    for side in ["left", "right"] {
        let sx = if side == "left" { 1.0 } else { -1.0 };
        let mirror = |v: DVec3| DVec3::new(v.x * sx, v.y, v.z);
        let named = |part: &str| format!("{part}_{side}");
        // A saddle bolted on the shoulder block.
        let shoulder = b.index(&named("shoulder"));
        let saddle = b.part(&named("bastion.saddle"));
        let i = attach_to_surface(
            &mut b.v,
            &saddle,
            shoulder,
            mirror(SADDLE_AT),
            DQuat::IDENTITY,
        )
        .unwrap();
        b.v.parts[i].name = named("saddle");
        for (quarter, middle) in QUARTERS {
            let name = |part: &str| format!("{quarter}_{part}_{side}");
            let tangent = mirror(tangent_left(middle));
            let fold_along = tangent * fold_direction(quarter);
            // Shoulder: swing (vertical axis) and lift (along the curve, parallel to the elbow).
            b.join_best(
                &named("saddle"),
                quarter,
                "bastion.arm_hinge",
                "base",
                &name("swing"),
                axis_along(DVec3::Y),
            );
            b.join_best(
                &name("swing"),
                "end",
                "bastion.arm_hinge",
                "base",
                &name("lift"),
                axis_along(tangent),
            );
            b.join_best(
                &name("lift"),
                "end",
                &format!("bastion.link_{quarter}"),
                "top",
                &name("link"),
                any,
            );
            b.join_best(
                &name("link"),
                "bottom",
                "bastion.arm_hinge",
                "base",
                &name("elbow"),
                axis_along(tangent),
            );
            b.join_best(
                &name("elbow"),
                "end",
                &format!("bastion.carrier_{quarter}_{side}"),
                "base",
                &name("carrier"),
                // A right-handed part is the left one reflected: its X axis points the other
                // way from the mirrored direction.
                axis_along(tangent * sx),
            );
            // The quarter, fixed to the carrier, upright (its node faces along its diagonal,
            // so only upright picks the turn).
            b.join_best(
                &name("carrier"),
                "shell",
                &format!("bastion.shell_quarter_{quarter}_{side}"),
                "hang",
                &name("shell"),
                |p| (p.rotation * DVec3::Y).dot(DVec3::Y),
            );
            // The folding leg: deploy (swings the folded leg out from under its quarter), the
            // drop, a bend down into the fold, then segments side by side joined by fold
            // knees, a wrist and the grapple.
            b.join_best(
                &name("carrier"),
                "leg",
                "bastion.leg_hinge",
                "base",
                &name("deploy"),
                axis_along(tangent),
            );
            b.join_best(
                &name("deploy"),
                "end",
                &format!("bastion.drop_{quarter}"),
                "top",
                &name("drop"),
                any,
            );
            b.join_best(
                &name("drop"),
                "bottom",
                &format!("bastion.bend_{quarter}_{side}"),
                "base",
                &name("bend"),
                |p| p.node_in_vehicle("end").unwrap().1.dot(DVec3::NEG_Y),
            );
            let mut parent = name("bend");
            for k in 0..LEG_SEGMENTS {
                // Even segments hang down from their top; odd ones stand up on their bottom.
                let (own, far) = if k % 2 == 0 {
                    ("top", "bottom")
                } else {
                    ("bottom", "top")
                };
                let seg = name(&format!("leg_{}", k + 1));
                b.join_best(
                    &parent,
                    "end",
                    &format!("bastion.leg_{}", k + 1),
                    own,
                    &seg,
                    any,
                );
                if k + 1 < LEG_SEGMENTS {
                    // The next segment lies beside this one, further along the fold.
                    let knee = name(&format!("fold_{}", k + 1));
                    b.join_best(&seg, far, &named("bastion.fold_knee"), "base", &knee, |p| {
                        let (end, _) = p.node_in_vehicle("end").unwrap();
                        let (base, _) = p.node_in_vehicle("base").unwrap();
                        (end - base).dot(fold_along)
                    });
                    parent = knee;
                } else {
                    b.join_best(&seg, far, "bastion.arm_rotor", "base", &name("wrist"), any);
                    b.join_best(
                        &name("wrist"),
                        "end",
                        "bastion.grapple",
                        "top",
                        &name("grapple"),
                        any,
                    );
                }
            }
        }
    }

    // Engines: on the outer face of each rear quarter, nozzles straight back; the upper pair as
    // far above the centre of mass as the lower pair below it, so the forward thrust line
    // passes through it.
    let com_y = VehicleBody::of(&b.v).unwrap().mass.center_of_mass.y;
    let shell = b.part("bastion.shell_quarter_back_left").shape;
    let Shape::Shell { thickness_m, .. } = shell else {
        unreachable!("the quarter is a shell")
    };
    let nozzle_back = DQuat::from_rotation_x(-std::f64::consts::FRAC_PI_2);
    let engine = b.part("thruster.main.medium");
    for (level, y) in [("upper", com_y + ENGINE_GAP), ("lower", com_y - ENGINE_GAP)] {
        for (n, dy) in [ENGINE_SPREAD / 2.0, -ENGINE_SPREAD / 2.0]
            .into_iter()
            .enumerate()
        {
            let y = y + dy;
            let r = profile_radius(&shell, y) + thickness_m / 2.0 + 0.9;
            let at = DVec3::new(0.0, y, SHELL_AXIS_Z) + radial_left(ENGINE_AZIMUTH) * r;
            for (side, at) in [("left", at), ("right", DVec3::new(-at.x, at.y, at.z))] {
                let parent = b.index(&format!("back_shell_{side}"));
                let i = attach_to_surface(&mut b.v, &engine, parent, at, nozzle_back).unwrap();
                b.v.parts[i].name = format!("engine_{level}_{}_{side}", n + 1);
            }
        }
    }

    // The gyro inside the torso.
    let gyro = b.part("frame.gyro");
    let i = attach_to_surface(&mut b.v, &gyro, 0, DVec3::ZERO, DQuat::IDENTITY).unwrap();
    b.v.parts[i].name = "gyro".into();

    // Walking: the legs and gait of the heavy frame.
    let leg = |side: &str| LegDef {
        hip: format!("hip_joint_{side}"),
        knee: format!("knee_joint_{side}"),
        ankle: Some(format!("ankle_joint_{side}")),
        foot: format!("foot_{side}"),
    };
    b.v.gait = Some(GaitDef {
        legs: vec![leg("left"), leg("right")],
        step_length_m: 4.0,
        step_height_m: 1.0,
        speed_m_s: 3.0,
        turn_rate_deg_s: 20.0,
        stance: 0.9,
    });
    b.v
}

fn part(v: &Vehicle, name: &str) -> usize {
    v.parts
        .iter()
        .position(|p| p.name == name)
        .unwrap_or_else(|| panic!("no part {name}"))
}

fn joint(v: &Vehicle, a: &Articulation, name: &str) -> usize {
    a.group_of_joint(part(v, name)).unwrap()
}

/// Where a quarter's middle (its centre of mass) is for joint `angles`.
fn quarter_at(v: &Vehicle, a: &Articulation, quarter: &str, side: &str, angles: &[f64]) -> DVec3 {
    let i = part(v, &format!("{quarter}_shell_{side}"));
    let p = &v.parts[i];
    a.poses(angles)[a.part_group[i]].apply(p.position + p.rotation * p.def.shape.centroid())
}

/// Distance of a point from the cocoon's axis.
fn radius(p: DVec3) -> f64 {
    DVec3::new(p.x, 0.0, p.z - SHELL_AXIS_Z).length()
}

/// Whether moving the joints from `from` to `to` in small steps stays free of interference.
fn clear_path(v: &Vehicle, a: &Articulation, from: &[f64], to: &[f64]) -> bool {
    let steps = 12;
    (0..=steps).all(|k| {
        let t = f64::from(k) / f64::from(steps);
        let angles: Vec<f64> = from.iter().zip(to).map(|(x, y)| x + (y - x) * t).collect();
        interferences(v, a, &angles, TOLERANCE).is_empty()
    })
}

/// Swings every shoulder link out (the elbow turning back the same, so the quarters move out
/// without tilting) in 5° steps as far as `limit`, every step free of interference.
fn hold_out(v: &Vehicle, a: &Articulation, limit: f64) -> Vec<f64> {
    let mut angles = vec![0.0; a.groups.len()];
    // For each arm: lift and elbow groups and the sign that moves the quarter outward.
    let arms: Vec<(usize, usize, f64)> = QUARTERS
        .iter()
        .flat_map(|&(quarter, _)| {
            ["left", "right"].map(|side| {
                let lift = joint(v, a, &format!("{quarter}_lift_{side}"));
                let elbow = joint(v, a, &format!("{quarter}_elbow_{side}"));
                let out = |deg: f64| {
                    let mut t = vec![0.0; a.groups.len()];
                    t[lift] = deg.to_radians();
                    t[elbow] = -deg.to_radians();
                    radius(quarter_at(v, a, quarter, side, &t))
                };
                (lift, elbow, if out(10.0) > out(-10.0) { 1.0 } else { -1.0 })
            })
        })
        .collect();
    let mut deg = 0.0;
    while deg + 5.0 <= limit {
        let mut next = angles.clone();
        for &(lift, elbow, sign) in &arms {
            next[lift] = sign * (deg + 5.0f64).to_radians();
            next[elbow] = -sign * (deg + 5.0f64).to_radians();
        }
        if !clear_path(v, a, &angles, &next) {
            break;
        }
        angles = next;
        deg += 5.0;
    }
    angles
}

/// The Bastion's poses: cocoon (closed) and stand (the quarters held out clear of the legs:
/// the pose it walks in and starts in).
fn poses(v: &Vehicle) -> Vec<NamedPose> {
    let body = VehicleBody::of(v).unwrap();
    let a = &body.articulation;
    let n = a.groups.len();
    let named = |name: &str, angles: &[f64]| NamedPose {
        name: name.into(),
        angles: v
            .parts
            .iter()
            .enumerate()
            .filter_map(|(i, p)| {
                let g = a.group_of_joint(i)?;
                let deg = angles[g].to_degrees().round();
                (deg != 0.0).then(|| (p.name.clone(), deg))
            })
            .collect(),
    };
    let stand = hold_out(v, a, WALK_LIMIT);
    vec![named("cocoon", &vec![0.0; n]), named("stand", &stand)]
}

fn design() -> Vehicle {
    let mut v = assemble();
    v.poses = poses(&v);
    v
}

fn pose(v: &Vehicle, a: &Articulation, name: &str) -> Vec<f64> {
    sim_vehicle::joints::pose_angles(v, a, name).unwrap_or_else(|| panic!("pose {name}"))
}

fn report(v: &Vehicle, found: &[Interference]) -> String {
    found
        .iter()
        .map(|i| {
            format!(
                "{} x {} ({:.2} m)",
                v.parts[i.a].name, v.parts[i.b].name, i.depth
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

#[test]
fn the_bastion_assembles_into_a_valid_balanced_design() {
    let v = assemble();
    for (i, p) in v.parts.iter().enumerate().skip(1) {
        let parent = p.parent.unwrap();
        assert!(parent < i);
        assert!(check_join(p, &v.parts[parent]).is_none(), "{}", p.name);
    }
    let names: std::collections::HashSet<&str> = v.parts.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names.len(), v.parts.len(), "unique names");
    let body = VehicleBody::of(&v).unwrap();
    let com = body.mass.center_of_mass;
    assert!(com.x.abs() < 1e-6, "balanced left and right: {com:?}");
    let s = stats(&v, 9.81);
    println!(
        "{} parts, {} joints, {:.1} t, {:.1} x {:.1} x {:.1} m, com {com:?}, thrust/weight {:.2}, problems {:?}",
        s.parts,
        body.articulation.groups.len() - 1,
        s.mass_kg / 1000.0,
        s.size.x,
        s.size.y,
        s.size.z,
        s.thrust_to_weight,
        s.problems
    );
    assert!(s.problems.is_empty(), "{:?}", s.problems);
    assert!(s.thrust_to_weight > 1.5);
    // Fixed to their carriers, the quarters sit around the cocoon's axis.
    for (q, _) in QUARTERS {
        for side in ["left", "right"] {
            let p = &v.parts[part(&v, &format!("{q}_shell_{side}"))];
            let off = p.position - DVec3::new(0.0, 0.0, SHELL_AXIS_Z);
            let up = p.rotation * DVec3::Y;
            println!(
                "{q} {side} quarter: axis off by {:.3} m, tilted {:.2} deg",
                off.length(),
                up.angle_between(DVec3::Y).to_degrees()
            );
            assert!(
                off.length() < 0.06 && up.dot(DVec3::Y) > 0.9995,
                "{q} {side}"
            );
        }
    }
    // Unfolded, a leg reaches several times the body's height from the shoulder.
    let reach: f64 = v
        .parts
        .iter()
        .filter(|p| p.name.starts_with("front_") && p.name.ends_with("_left"))
        .filter(|p| p.def.id.starts_with("bastion.leg_") || p.def.id.starts_with("bastion.drop"))
        .map(|p| p.def.shape.bounds().1.y * 2.0)
        .sum();
    println!("a leg's segments add up to {reach:.0} m");
    assert!(reach > 50.0, "{reach}");
}

#[test]
fn closed_the_quarters_make_a_cocoon_with_nothing_in_the_way() {
    let v = assemble();
    let body = VehicleBody::of(&v).unwrap();
    let a = &body.articulation;
    let rest = vec![0.0; a.groups.len()];
    let found = interferences(&v, a, &rest, TOLERANCE);
    assert!(found.is_empty(), "closed: {}", report(&v, &found));
}

#[test]
fn every_pose_and_every_move_between_them_is_free_of_interference() {
    let v = design();
    let body = VehicleBody::of(&v).unwrap();
    let a = &body.articulation;
    let names: Vec<&str> = v.poses.iter().map(|p| p.name.as_str()).collect();
    for from in &names {
        for to in &names {
            let (x, y) = (pose(&v, a, from), pose(&v, a, to));
            assert!(clear_path(&v, a, &x, &y), "{from} -> {to}");
        }
    }
    // Standing, the quarters are held out from the body.
    let rest = vec![0.0; a.groups.len()];
    for (q, _) in QUARTERS {
        for side in ["left", "right"] {
            let closed = radius(quarter_at(&v, a, q, side, &rest));
            let held = radius(quarter_at(&v, a, q, side, &pose(&v, a, "stand")));
            println!("stand {q} {side}: middle {closed:.1} m -> {held:.1} m from the axis");
            assert!(held > closed + 1.5, "stand: {q} {side} held out");
        }
    }
}

#[test]
fn the_legs_stride_clear_of_the_held_out_quarters() {
    use sim_vehicle::FlightControl;
    use sim_vehicle::flight::{RegionTerrain, region_env};
    use sim_vehicle::gait::{WalkStep, Walking, resolve_legs, walk_tick};

    // Walk the real gait in the standing pose (forward, then turning) and check every tick's
    // leg pose against everything else.
    let v = design();
    let body = VehicleBody::of(&v).unwrap();
    let a = &body.articulation;
    let gait = v.gait.clone().unwrap();
    let legs = resolve_legs(&v, &body, &gait).unwrap();
    let mut joints = JointState::rest(a);
    let stand = pose(&v, a, "stand");
    joints.angles = stand.clone();
    joints.targets = stand;
    let sys = sim_core::StarSystem::from_path(content().join("star_systems/halcyon.json")).unwrap();
    let i = sys.bodies.iter().position(|b| b.id == "meridian").unwrap();
    let terrain = RegionTerrain(sim_planet::Terrain::of(&sys.bodies[i]).map(Arc::new));
    let env = region_env(&sys, i, 0.0, Some(&terrain));
    let up = sim_planet::fibonacci_directions(20_000)
        .map(|d| env.surface_rotation * d)
        .find(|&d| env.ground_height(d * env.radius) > 100.0)
        .unwrap();
    let north = up.cross(DVec3::Y.cross(up)).normalize();
    let ground = env.radius + env.ground_height(up * env.radius);
    let mut s = sim_core::frames::FrameState {
        position: up * (ground + 16.0),
        rotation: DQuat::from_mat3(&DMat3::from_cols(north.cross(up), up, -north)),
        velocity: env.spin.cross(up * ground),
        angular_velocity: env.spin,
    };
    let mut walking = Walking::default();
    let mut control = FlightControl {
        translate: DVec3::NEG_Z,
        ..Default::default()
    };
    let dt = 1.0 / 30.0;
    for tick in 0..(30 * 12) {
        if tick == 30 * 8 {
            control.translate = DVec3::new(1.0, 0.0, -0.5);
        }
        let env = region_env(&sys, i, f64::from(tick) * dt, Some(&terrain));
        let step = walk_tick(
            &mut s,
            &mut walking,
            &mut joints,
            &body,
            &legs,
            &gait,
            &control,
            &env,
            dt,
        );
        assert_eq!(step, WalkStep::Walking);
        if tick % 3 == 0 {
            let found = interferences(&v, a, &joints.angles, TOLERANCE);
            assert!(found.is_empty(), "tick {tick}: {}", report(&v, &found));
        }
    }
}

#[test]
fn the_design_file_is_this_design() {
    let catalog = PartCatalog::load_dir(content().join("parts")).unwrap();
    let file = VehicleDesign::from_path(content().join("vehicles/bastion.json"), &catalog)
        .expect("content/data/vehicles/bastion.json (refresh with --ignored)");
    let saved = Vehicle::from_design(&file);
    let built = assemble();
    assert_eq!(
        saved.parts.len(),
        built.parts.len(),
        "refresh with --ignored"
    );
    assert_eq!(saved.gait, built.gait, "refresh with --ignored");
    for (a, b) in saved.parts.iter().zip(&built.parts) {
        assert_eq!(
            (&a.name, &a.def.id, a.parent),
            (&b.name, &b.def.id, b.parent)
        );
        assert!(
            a.position.distance(b.position) < 1e-5,
            "{} (refresh with --ignored)",
            a.name
        );
    }
}

#[test]
#[ignore = "writes content/data/vehicles/bastion.json"]
fn write_the_design_file() {
    let json = design_json(&design(), "bastion", NAME, DESCRIPTION);
    std::fs::write(content().join("vehicles/bastion.json"), json).unwrap();
}

#[test]
#[ignore = "diagnostic: lists overlapping parts closed and in each pose"]
fn list_interferences() {
    let v = assemble();
    let body = VehicleBody::of(&v).unwrap();
    let a = &body.articulation;
    let rest = vec![0.0; a.groups.len()];
    println!(
        "rest: {}",
        report(&v, &interferences(&v, a, &rest, TOLERANCE))
    );
    let v = design();
    for p in &v.poses {
        let angles = pose(&v, a, &p.name);
        println!("{}: {:?}", p.name, p.angles);
        println!(
            "  {}",
            report(&v, &interferences(&v, a, &angles, TOLERANCE))
        );
    }
}

/// Walks a few seconds in its standing pose on a slope on Meridian, stops, and hands the
/// frame to physics: it must stay standing.
#[test]
fn the_bastion_walks_and_stops_standing() {
    use sim_core::ObjectId;
    use sim_vehicle::flight::{RegionTerrain, RigidState, region_env};
    use sim_vehicle::gait::{WalkStep, Walking, resolve_legs, walk_tick};
    use sim_vehicle::{FlightControl, PhysicsScene};

    let v = design();
    let body = VehicleBody::of(&v).unwrap();
    let a = &body.articulation;
    let gait = v.gait.clone().unwrap();
    let legs = resolve_legs(&v, &body, &gait).unwrap();
    let mut joints = JointState::rest(a);
    let stand = pose(&v, a, "stand");
    joints.angles = stand.clone();
    joints.targets = stand;
    let sys = sim_core::StarSystem::from_path(content().join("star_systems/halcyon.json")).unwrap();
    let i = sys.bodies.iter().position(|b| b.id == "meridian").unwrap();
    let terrain = RegionTerrain(sim_planet::Terrain::of(&sys.bodies[i]).map(Arc::new));
    let env_at = |t: f64| region_env(&sys, i, t, Some(&terrain));
    let env = env_at(0.0);
    let up = sim_planet::fibonacci_directions(20_000)
        .map(|d| env.surface_rotation * d)
        .find(|&d| {
            let p = d * env.radius;
            env.ground_height(p) > 100.0 && env.ground_normal(p).dot(d) < 0.995
        })
        .unwrap();
    let north = up.cross(DVec3::Y.cross(up)).normalize();
    let rotation = DQuat::from_mat3(&DMat3::from_cols(north.cross(up), up, -north));
    let ground = env.radius + env.ground_height(up * env.radius);
    let mut s = sim_core::frames::FrameState {
        position: up * (ground + 16.0),
        rotation,
        velocity: env.spin.cross(up * ground),
        angular_velocity: env.spin,
    };
    let mut walking = Walking::default();
    let mut control = FlightControl {
        translate: DVec3::NEG_Z,
        ..Default::default()
    };
    let dt = 1.0 / 30.0;
    let mut tick = 0u32;
    loop {
        tick += 1;
        if tick == 30 * 6 {
            walking.stopping = true;
            control.translate = DVec3::ZERO;
        }
        let step = walk_tick(
            &mut s,
            &mut walking,
            &mut joints,
            &body,
            &legs,
            &gait,
            &control,
            &env_at(f64::from(tick) * dt),
            dt,
        );
        if step == WalkStep::Stopped {
            break;
        }
        assert_eq!(step, WalkStep::Walking);
        assert!(tick < 30 * 14, "stopped");
    }
    let com = body.mass.center_of_mass;
    let mut r = RigidState::from_origin(&s, com);
    let mut scene = PhysicsScene::with_inline_patches();
    let id = ObjectId(1);
    for k in 0..(30 * 10) {
        let env = env_at(f64::from(tick + k + 1) * dt);
        scene.place_dynamic(id, &body, &r, Default::default());
        scene.set_poses(id, &joints.poses, joints.generation);
        scene.step(&env, dt);
        r = scene.state(id).unwrap().0;
    }
    let tilt = (r.rotation * DVec3::Y)
        .angle_between(r.position.normalize())
        .to_degrees();
    assert!(tilt < 10.0, "stands after stopping: tilted {tilt:.1} deg");
}

#[test]
#[ignore = "diagnostic: where the arm parts landed"]
fn arm_layout() {
    let v = assemble();
    for p in &v.parts {
        if p.name.contains("front") || p.name.contains("back") || p.name.starts_with("saddle") {
            if p.name.ends_with("_right") && !p.name.contains("shell") {
                continue;
            }
            let c = p.position + p.rotation * p.def.shape.centroid();
            let r = DVec3::new(c.x, 0.0, c.z - SHELL_AXIS_Z).length();
            let y_axis = p.rotation * DVec3::Y;
            println!(
                "{:22} at {:6.2} {:6.2} {:6.2}  r {:5.2}  Y {:5.2} {:5.2} {:5.2}",
                p.name, c.x, c.y, c.z, r, y_axis.x, y_axis.y, y_axis.z
            );
        }
    }
}

#[test]
#[ignore = "diagnostic: what stops the quarters being held further out"]
fn what_blocks_holding_out() {
    let v = assemble();
    let body = VehicleBody::of(&v).unwrap();
    let a = &body.articulation;
    for deg in [55.0f64, 60.0, 70.0, 80.0] {
        let mut angles = vec![0.0; a.groups.len()];
        for (q, _) in QUARTERS {
            for (side, s) in [("left", 1.0), ("right", -1.0)] {
                angles[joint(&v, a, &format!("{q}_lift_{side}"))] = s * deg.to_radians();
                angles[joint(&v, a, &format!("{q}_elbow_{side}"))] = -s * deg.to_radians();
            }
        }
        let found: Vec<Interference> = interferences(&v, a, &angles, TOLERANCE)
            .into_iter()
            .filter(|i| {
                v.parts[i.a].name.ends_with("_left") || v.parts[i.b].name.ends_with("_left")
            })
            .collect();
        println!("{deg}: {}", report(&v, &found));
    }
}
