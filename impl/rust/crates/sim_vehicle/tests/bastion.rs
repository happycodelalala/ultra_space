//! The Bastion: an original heavy frame (~24 m, ~240 t) carried in a cocoon of four curved
//! quarter-shells, each on a shell arm. Each arm: a shoulder hinge on a saddle, a
//! long link up under the dome, an elbow folding the arm into a V, a long forearm back down to
//! a wrist at the quarter's upper middle, and the carrier the quarter is fixed to; the forearm
//! telescopes (a slide collar drives an inner beam out of the sleeve). The lift,
//! elbow and wrist turn about parallel axes, so the arm moves its quarter in one plane and the
//! wrist keeps it upright (walking: held out clear of the legs) or turns it on its back
//! (flying: pushed out over the four corners as platforms, each with four gimballed lift engines
//! round the carrier). Each quarter's bulge holds a drone bay under a hinged cover. From
//! the carrier a long folding leg (six segments, thinner and thinner, ending in a grapple)
//! lies folded flat along the inside of the quarter's lower part. Closed, the quarters make a cocoon over the
//! head, shoulders and arm bases, tapering to just above the ankles. Every pose and every move
//! between poses is checked free of interference.
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
const DESCRIPTION: &str = "An original heavy frame, about 24 m tall, in a cocoon of four thick \
     curved quarter-shells. Each quarter rides on a long shell arm from a shoulder housing (link, \
     elbow, telescoping forearm, wrist), hides a long folding leg with a grapple along its \
     inside, carries four gimballed lift engines and has a drone bay in its bulge under a \
     hinged cover; walking, the arms hold the quarters clear of the legs; flying, they push \
     them out over the four corners as platforms, engines blowing down. Forward is -Z.";

/// The quarters on the left: name and the azimuth of its middle (0 behind, 90 left, 180 in
/// front, degrees). The arm's parts and saddle node carry the same name.
const QUARTERS: [(&str, f64); 2] = [("front", 135.0), ("back", 45.0)];
/// Which way along the curve each quarter's leg folds (from its centreline end toward the
/// side): +1 along [`tangent_left`], −1 against it.
fn fold_direction(quarter: &str) -> f64 {
    if quarter == "front" { 1.0 } else { -1.0 }
}
/// Folding-leg segments after the drop (7.3 m, then 6.3 m each), side by side when folded.
const LEG_SEGMENTS: usize = 6;
/// The folded leg lies in a plane leaning in toward the hem with the shell's inner face, this
/// far from the vertical (degrees): its segments run along the shell, close to it.
const FOLD_LEAN_DEG: f64 = 12.2;

/// The cocoon wraps around this vertical axis (vehicle frame, x = 0).
const SHELL_AXIS_Z: f64 = 0.3;
/// Where the shoulder saddles sit (left; on top of the shoulder block, clear of the torso).
const SADDLE_AT: DVec3 = DVec3::new(-4.0, 2.9, 0.0);
/// Overlap allowed between parts that aren't joined (m): the seams between neighbouring
/// quarters.
const TOLERANCE: f64 = 0.05;
/// The lift engines inside each quarter: two rows at these heights (m), each with an engine
/// this far either side of the quarter's middle (degrees): a block of two either side of the
/// carrier and the folded leg's drop, below the shell arm's shoulder and clear of the forearm.
const LIFT_ENGINES: [(f64, f64); 2] = [(-0.3, 31.0), (1.9, 31.0)];
/// The drone bay cover's hinge in its quarter's axes: height, and distance from the cocoon's
/// axis (just outside the shell's outer face; matches `bastion.bay_cover`'s hinge node).
const BAY_HINGE: (f64, f64) = (5.0, 6.095);
/// How far the bay covers open in the "bays" pose (degrees).
const BAY_OPEN_DEG: f64 = 75.0;
/// How deep the engine mounts sit in the shell, past its inner face (m).
const ENGINE_RECESS: f64 = 0.55;
/// The wrist the quarter turns on: at this height on its upper middle (m).
const WRIST_HEIGHT: f64 = 2.4;
/// How far inside the shell's inner face the wrist sits (m).
const WRIST_INSET: f64 = 1.3;
/// The forearm extensions the pose searches try (m; the slide runs 0 to 2.5).
const ARM_REACH: [f64; 3] = [0.0, 1.25, 2.5];
/// How far from the cocoon's axis the walking pose holds the quarters' middles at most (m).
const HELD_OUT: f64 = 9.0;

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
    b.pair(
        "elbow_joint",
        "end",
        "bastion.forearm",
        "top",
        "forearm",
        any,
    );
    b.pair("forearm", "bottom", "bastion.hand", "wrist", "hand", any);
    b.pair("waist", "hip_left", "frame.hinge", "base", "hip_joint", any);
    b.pair("hip_joint", "end", "bastion.leg", "top", "thigh", any);
    b.pair("thigh", "bottom", "frame.hinge", "base", "knee_joint", any);
    b.pair("knee_joint", "end", "bastion.shin", "top", "shin", any);
    b.pair("shin", "bottom", "frame.hinge", "base", "ankle_joint", any);
    b.pair("ankle_joint", "end", "bastion.foot", "top", "foot", any);

    // The wrist sits this far from the axis: inside the shell's upper middle.
    let wrist_radius = {
        let shell = b.part("bastion.shell_quarter_front_left").shape;
        let Shape::Shell { thickness_m, .. } = shell else {
            unreachable!("the quarter is a shell")
        };
        profile_radius(&shell, WRIST_HEIGHT) - thickness_m / 2.0 - WRIST_INSET
    };
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
            let lean = FOLD_LEAN_DEG.to_radians();
            let fold_down = DVec3::NEG_Y * lean.cos() - mirror(radial_left(middle)) * lean.sin();
            // Shoulder: the lift hinge, turning along the curve (parallel to elbow and wrist).
            b.join_best(
                &named("saddle"),
                quarter,
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
            // The elbow folds the arm into a V: its far side heads back down to the wrist at the
            // quarter's upper middle.
            let wrist_at = DVec3::new(0.0, WRIST_HEIGHT, SHELL_AXIS_Z)
                + mirror(radial_left(middle)) * wrist_radius;
            b.join_best(
                &name("link"),
                "bottom",
                &format!("bastion.elbow_{quarter}_{side}"),
                "base",
                &name("elbow"),
                move |p| {
                    let (at, dir) = p.node_in_vehicle("end").unwrap();
                    dir.dot((wrist_at - at).normalize())
                },
            );
            b.join_best(
                &name("elbow"),
                "end",
                &format!("bastion.forearm_{quarter}"),
                "top",
                &name("forearm"),
                any,
            );
            // The forearm telescopes: a slide collar on the sleeve drives the inner beam out.
            b.join_best(
                &name("forearm"),
                "bottom",
                "bastion.arm_slide",
                "base",
                &name("slide"),
                any,
            );
            b.join_best(
                &name("slide"),
                "end",
                "bastion.forearm_beam",
                "top",
                &name("beam"),
                any,
            );
            b.join_best(
                &name("beam"),
                "bottom",
                "bastion.arm_hinge",
                "base",
                &name("wrist"),
                axis_along(tangent),
            );
            b.join_best(
                &name("wrist"),
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
                move |p| p.node_in_vehicle("end").unwrap().1.dot(fold_down),
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
                    b.join_best(&seg, far, "bastion.arm_rotor", "base", &name("hand"), any);
                    b.join_best(
                        &name("hand"),
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

    // Lift engines: four inside each quarter round the carrier, each hanging from a two-axis
    // gimbal (mount, then cross) on the inner face, exhaust toward the quarter's inside
    // (thrust outward). Tilted up into a platform (fly), a quarter blows down.
    let shell = b.part("bastion.shell_quarter_front_left").shape;
    let Shape::Shell { thickness_m, .. } = shell else {
        unreachable!("the quarter is a shell")
    };
    let mount = b.part("bastion.engine_mount");
    for side in ["left", "right"] {
        let sx = if side == "left" { 1.0 } else { -1.0 };
        let mirror = |v: DVec3| DVec3::new(v.x * sx, v.y, v.z);
        for (quarter, middle) in QUARTERS {
            let parent = b.index(&format!("{quarter}_shell_{side}"));
            for (row, (y, spread)) in LIFT_ENGINES.into_iter().enumerate() {
                for (col, da) in [-spread, spread].into_iter().enumerate() {
                    let out = mirror(radial_left(middle + da));
                    let along = mirror(tangent_left(middle + da));
                    // The mount and cross sit in a socket in the shell; the engine hangs from
                    // the inner face.
                    let r = profile_radius(&shell, y) - thickness_m / 2.0 + ENGINE_RECESS;
                    let at = DVec3::new(0.0, y, SHELL_AXIS_Z) + out * r;
                    let turn = DQuat::from_mat3(&DMat3::from_cols(along, out, along.cross(out)));
                    let i = attach_to_surface(&mut b.v, &mount, parent, at, turn).unwrap();
                    let name = |what: &str| {
                        format!(
                            "{quarter}_engine_{what}_{}{}_{side}",
                            row + 1,
                            ["a", "b"][col]
                        )
                    };
                    b.v.parts[i].name = name("mount");
                    b.join_best(
                        &name("mount"),
                        "end",
                        "bastion.engine_gimbal",
                        "base",
                        &name("gimbal"),
                        // The cross turns across the mount: its axis along the curve's height.
                        move |p| (p.rotation * DVec3::Z).dot(DVec3::Y).abs(),
                    );
                    b.join_best(
                        &name("gimbal"),
                        "end",
                        "bastion.lift_engine",
                        "top",
                        &format!("{quarter}_engine_{}{}_{side}", row + 1, ["a", "b"][col]),
                        any,
                    );
                }
            }
        }
    }

    // Drone bays: in each quarter's bulge, under a cover (the bulge's outer layer from the
    // quarter's middle up to below the dome) that a hinge along its top edge swings up and out.
    let hinge = b.part("bastion.bay_hinge");
    for side in ["left", "right"] {
        for (quarter, _) in QUARTERS {
            let parent = b.index(&format!("{quarter}_shell_{side}"));
            let (position, rotation) = (b.v.parts[parent].position, b.v.parts[parent].rotation);
            let at = position + rotation * DVec3::new(0.0, BAY_HINGE.0, BAY_HINGE.1);
            // Axis along the curve, base node into the shell.
            let turn = rotation
                * DQuat::from_mat3(&DMat3::from_cols(DVec3::NEG_X, DVec3::NEG_Z, DVec3::NEG_Y));
            let i = attach_to_surface(&mut b.v, &hinge, parent, at, turn).unwrap();
            let name = |what: &str| format!("{quarter}_{what}_{side}");
            b.v.parts[i].name = name("bay_hinge");
            let up = rotation * DVec3::Y;
            b.join_best(
                &name("bay_hinge"),
                "end",
                "bastion.bay_cover",
                "hinge",
                &name("bay_cover"),
                move |p| (p.rotation * DVec3::Y).dot(up),
            );
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

/// An arm's three hinges in the plane of its quarter: shoulder lift, elbow and wrist (degrees,
/// left side; the right side mirrors them).
type ArmAngles = (f64, f64, f64);

/// `base` with both arms of `quarter` set to `arm`, the right one mirrored.
fn with_arm(
    v: &Vehicle,
    a: &Articulation,
    base: &[f64],
    quarter: &str,
    arm: ArmAngles,
) -> Vec<f64> {
    let mut t = base.to_vec();
    for (side, s) in [("left", 1.0), ("right", -1.0)] {
        t[joint(v, a, &format!("{quarter}_lift_{side}"))] = s * arm.0.to_radians();
        t[joint(v, a, &format!("{quarter}_elbow_{side}"))] = s * arm.1.to_radians();
        t[joint(v, a, &format!("{quarter}_wrist_{side}"))] = s * arm.2.to_radians();
    }
    t
}

/// How far `quarter`'s telescoping forearms are run out (m).
fn with_reach(v: &Vehicle, a: &Articulation, base: &[f64], quarter: &str, reach: f64) -> Vec<f64> {
    let mut t = base.to_vec();
    for side in ["left", "right"] {
        t[joint(v, a, &format!("{quarter}_slide_{side}"))] = reach;
    }
    t
}

/// Moves `quarter`'s arms from `start` to the best pose they can reach: searches the shoulder
/// lift and the elbow (10° grid), turns the wrist to make `facing` (how well the quarter faces
/// the way it should, 1 best) as large as it gets, and keeps poses facing better than `least`.
/// Of those, the one with the largest `score` that is free of interference and reachable from
/// `from` without any (every joint moving at once: `from` is where the whole move starts, which
/// can be before other quarters moved to `start`) wins; `start` itself when none is.
fn place_quarter(
    v: &Vehicle,
    a: &Articulation,
    (start, from): (&[f64], &[f64]),
    quarter: &str,
    least: f64,
    facing: impl Fn(&[f64]) -> f64,
    score: impl Fn(&[f64]) -> f64,
) -> Vec<f64> {
    let mut candidates = Vec::new();
    for reach in ARM_REACH {
        for l in (-150..=150).step_by(10) {
            for e in (-180..=180).step_by(10) {
                let (l, e) = (f64::from(l), f64::from(e));
                let at = |w: f64| {
                    with_reach(
                        v,
                        a,
                        &with_arm(v, a, start, quarter, (l, e, w)),
                        quarter,
                        reach,
                    )
                };
                // Coarse then fine search for the wrist.
                let best = |from: f64, to: f64, step: f64| {
                    let mut w = from;
                    let mut best = (f64::MIN, from);
                    while w <= to + 1e-9 {
                        let f = facing(&at(w));
                        if f > best.0 {
                            best = (f, w);
                        }
                        w += step;
                    }
                    best
                };
                let (_, w) = best(-180.0, 180.0, 10.0);
                let (f, w) = best(w - 9.0, w + 9.0, 1.0);
                if f > least {
                    let t = at(w);
                    candidates.push((score(&t), t));
                }
            }
        }
    }
    candidates.sort_by(|x, y| y.0.total_cmp(&x.0));
    candidates
        .into_iter()
        .filter(|(_, t)| interferences(v, a, t, TOLERANCE).is_empty())
        .find(|(_, t)| clear_path(v, a, from, t))
        .map_or_else(|| start.to_vec(), |(_, t)| t)
}

/// How upright a quarter stands (its axis against the vertical, 1 upright).
fn upright(v: &Vehicle, a: &Articulation, quarter: &str, angles: &[f64]) -> f64 {
    let i = part(v, &format!("{quarter}_shell_left"));
    (a.poses(angles)[a.part_group[i]].rotation * (v.parts[i].rotation * DVec3::Y)).y
}

/// The walking pose: each quarter held out upright, as far from the body as the arms take it
/// (up to `HELD_OUT` from the axis) and no lower than closed, so the legs stride clear of it.
fn hold_out(v: &Vehicle, a: &Articulation) -> Vec<f64> {
    let rest = vec![0.0; a.groups.len()];
    let mut angles = rest.clone();
    for (quarter, _) in QUARTERS {
        let closed = quarter_at(v, a, quarter, "left", &rest);
        angles = place_quarter(
            v,
            a,
            (&angles, &angles),
            quarter,
            0.998,
            |t| upright(v, a, quarter, t),
            |t| {
                let at = quarter_at(v, a, quarter, "left", t);
                let sag = (closed.y - at.y).max(0.0);
                radius(at).min(HELD_OUT) - 2.0 * sag - 0.05 * (at.y - closed.y).abs()
            },
        );
    }
    angles
}

/// How the lift engines on a quarter point (their thrust direction, mean) for joint `angles`.
fn lift_direction(
    v: &Vehicle,
    a: &Articulation,
    quarter: &str,
    side: &str,
    angles: &[f64],
) -> DVec3 {
    let poses = a.poses(angles);
    v.parts
        .iter()
        .enumerate()
        .filter(|(_, p)| {
            p.def.id == "bastion.lift_engine"
                && p.name.starts_with(&format!("{quarter}_engine_"))
                && p.name.ends_with(side)
        })
        .map(|(i, p)| poses[a.part_group[i]].rotation * (p.rotation * DVec3::Y))
        .sum::<DVec3>()
        .normalize()
}

/// The flying pose: from `start`, each quarter turned on its back into a platform, its lift
/// engines pushing up, pushed out from the body over its own corner, like a ladybug's open
/// wing cases (not stacked above it).
fn tilt_up(v: &Vehicle, a: &Articulation, start: &[f64]) -> Vec<f64> {
    let mut angles = start.to_vec();
    for (quarter, middle) in QUARTERS {
        let corner = radial_left(middle);
        angles = place_quarter(
            v,
            a,
            (&angles, start),
            quarter,
            0.97,
            |t| lift_direction(v, a, quarter, "left", t).y,
            |t| {
                // Pushed out from the body, over its own corner, rather than raised.
                let at = quarter_at(v, a, quarter, "left", t);
                let flat = DVec3::new(at.x, 0.0, at.z - SHELL_AXIS_Z).normalize_or_zero();
                radius(at) - 0.3 * at.y + 4.0 * flat.dot(corner)
            },
        );
    }
    angles
}

/// `start` with every drone bay's cover swung up and out by [`BAY_OPEN_DEG`] (each hinge
/// turned the way that lifts its cover away from the shell).
fn open_bays(v: &Vehicle, a: &Articulation, start: &[f64]) -> Vec<f64> {
    let mut t = start.to_vec();
    for (i, p) in v.parts.iter().enumerate() {
        if p.def.id != "bastion.bay_hinge" {
            continue;
        }
        let g = a.group_of_joint(i).unwrap();
        let cover = v.parts.iter().position(|c| c.parent == Some(i)).unwrap();
        let shell = p.parent.unwrap();
        let away = |angle: f64| {
            let mut u = t.clone();
            u[g] = angle;
            let poses = a.poses(&u);
            let c = &v.parts[cover];
            let at =
                poses[a.part_group[cover]].apply(c.position + c.rotation * c.def.shape.centroid());
            let s = &v.parts[shell];
            let middle =
                poses[a.part_group[shell]].apply(s.position + s.rotation * s.def.shape.centroid());
            (at - middle).length()
        };
        let open = BAY_OPEN_DEG.to_radians();
        t[g] = if away(open) > away(-open) {
            open
        } else {
            -open
        };
    }
    t
}

/// The Bastion's poses: bays (standing, the drone bays open); cocoon (closed); stand (the quarters held out clear of the legs: the
/// pose it walks in and starts in); fly (the quarters held out and tilted up into four
/// platforms, their lift engines blowing down).
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
                let value = a.groups[g].joint?.to_data(angles[g]);
                (value != 0.0).then(|| (p.name.clone(), value))
            })
            .collect(),
    };
    let stand = hold_out(v, a);
    let fly = tilt_up(v, a, &stand);
    let bays = open_bays(v, a, &stand);
    vec![
        named("bays", &bays),
        named("cocoon", &vec![0.0; n]),
        named("fly", &fly),
        named("stand", &stand),
    ]
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
    // The lift engines face out of the cocoon at rest and only push in the fly pose (checked
    // by `flying_the_four_platforms_lift_it`), so their torque at rest doesn't matter.
    let problems: Vec<_> = s
        .problems
        .iter()
        .filter(|p| !p.starts_with("forward engines"))
        .collect();
    assert!(problems.is_empty(), "{problems:?}");
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
    assert!(reach > 40.0, "{reach}");
}

#[test]
fn flying_the_four_platforms_lift_it() {
    let v = design();
    let body = VehicleBody::of(&v).unwrap();
    let a = &body.articulation;
    let fly = pose(&v, a, "fly");
    for (q, _) in QUARTERS {
        for side in ["left", "right"] {
            let d = lift_direction(&v, a, q, side, &fly);
            println!("fly {q} {side}: engines push along {d:.2?}");
            assert!(d.y > 0.95, "{q} {side} platform level");
        }
    }
    let allocator = body.posed_allocator(&a.poses(&fly));
    let up: f64 = allocator
        .thrusters()
        .iter()
        .map(|t| t.direction.y * t.max_thrust)
        .sum();
    let weight = body.mass.mass * 9.81;
    println!(
        "lift {:.0} kN, weight {:.0} kN: {:.2} g",
        up / 1e3,
        weight / 1e3,
        up / weight
    );
    assert!(up > 1.5 * weight);
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
        if p.name.contains("front")
            || p.name.contains("back")
            || p.name.starts_with("saddle")
            || p.name.starts_with("upper_arm")
            || p.name.starts_with("fore")
        {
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
#[ignore = "diagnostic: what stops the quarters being held out"]
fn what_blocks_holding_out() {
    let v = assemble();
    let body = VehicleBody::of(&v).unwrap();
    let a = &body.articulation;
    let rest = vec![0.0; a.groups.len()];
    let closed = quarter_at(&v, a, "front", "left", &rest);
    for (l, e) in [
        (20.0, -20.0),
        (40.0, -40.0),
        (-20.0, 20.0),
        (-40.0, 40.0),
        (30.0, 0.0),
        (-30.0, 0.0),
        (0.0, 30.0),
        (0.0, -30.0),
    ] {
        let mut best = (f64::MIN, 0.0);
        for w in -180..=180 {
            let t = with_arm(&v, a, &rest, "front", (l, e, f64::from(w)));
            let u = upright(&v, a, "front", &t);
            if u > best.0 {
                best = (u, f64::from(w));
            }
        }
        let t = with_arm(&v, a, &rest, "front", (l, e, best.1));
        let at = quarter_at(&v, a, "front", "left", &t);
        let right = quarter_at(&v, a, "front", "right", &t);
        let found = interferences(&v, a, &t, TOLERANCE);
        println!(
            "l {l} e {e}: wrist {} upright {:.4}, at {:.2?} (closed {:.2?}) r {:.2}, right {:.2?}: {}",
            best.1,
            best.0,
            at,
            closed,
            radius(at),
            right,
            report(&v, &found)
        );
    }
}

#[test]
#[ignore = "diagnostic: what stops a quarter tilting up into a platform"]
fn what_blocks_tilting_up() {
    let v = assemble();
    let body = VehicleBody::of(&v).unwrap();
    let a = &body.articulation;
    let held = hold_out(&v, a);
    for q in ["front", "back"] {
        let at = quarter_at(&v, a, q, "left", &held);
        println!("held {q}: at {at:.2?} r {:.2}", radius(at));
    }
    for quarter in ["front", "back"] {
        let mut found = Vec::new();
        for reach in ARM_REACH {
            for l in (-150..=150).step_by(10) {
                for e in (-180..=180).step_by(10) {
                    let (l, e) = (f64::from(l), f64::from(e));
                    let mut best = (f64::MIN, 0.0);
                    for w in (-180..=180).step_by(2) {
                        let t = with_reach(
                            &v,
                            a,
                            &with_arm(&v, a, &held, quarter, (l, e, f64::from(w))),
                            quarter,
                            reach,
                        );
                        let f = lift_direction(&v, a, quarter, "left", &t).y;
                        if f > best.0 {
                            best = (f, f64::from(w));
                        }
                    }
                    if best.0 > 0.97 {
                        let t = with_reach(
                            &v,
                            a,
                            &with_arm(&v, a, &held, quarter, (l, e, best.1)),
                            quarter,
                            reach,
                        );
                        let hits = interferences(&v, a, &t, TOLERANCE);
                        let at = quarter_at(&v, a, quarter, "left", &t);
                        found.push((hits.len(), reach, l, e, best.1, at, report(&v, &hits)));
                    }
                }
            }
        }
        found.sort_by_key(|f| f.0);
        println!(
            "{quarter}: {} level poses, {} free",
            found.len(),
            found.iter().filter(|f| f.0 == 0).count()
        );
        for (n, reach, l, e, w, at, r) in found.iter().take(5) {
            println!(
                "  reach {reach} l {l} e {e} w {w}: at {at:.2?} r {:.2}, {n} hits: {r}",
                radius(*at)
            );
        }
    }
}
