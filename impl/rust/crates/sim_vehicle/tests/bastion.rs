//! The Bastion: an original heavy frame (~22 m, ~130 t) whose armour moves. A dorsal spine on
//! its back carries four shell arms; each arm holds a faceted half-shell. The upper pair
//! covers the torso and head, the lower pair the thighs; closed, each pair meets its mirror
//! twin at seams front and back and the four make a cocoon (travel, landing, taking fire),
//! with the arms folded inside. Thrusters on the shells' rear facets point straight back when
//! closed; the arms aim them with the shells when open. Built only with the builder's
//! operations, on the heavy frame's walking body. `content/data/vehicles/bastion.json` is this
//! design; refresh it with `cargo test -p sim_vehicle --test bastion -- --ignored`.

use std::path::PathBuf;
use std::sync::Arc;

use glam::{DMat3, DQuat, DVec3};
use sim_vehicle::builder::{
    FreeNode, Symmetry, attach_at_node, attach_to_surface, free_nodes, place_symmetric, stats,
};
use sim_vehicle::design::{NamedPose, check_join, design_json};
use sim_vehicle::gait::{GaitDef, LegDef};
use sim_vehicle::joints::{Articulation, JointState, Pose};
use sim_vehicle::{PartCatalog, PartDef, Vehicle, VehicleBody, VehicleDesign, VehiclePart};

fn content() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../../content/data")
}

const NAME: &str = "Bastion";
const DESCRIPTION: &str = "An original heavy frame, about 22 m and 130 tonnes, whose armour \
     moves: a dorsal spine carries four shell arms, each holding a faceted half-shell. Closed, \
     the shells make a cocoon around the body with the arms folded inside and the engines on \
     the rear facets pointing straight back; open, they swing back like wing cases, or the \
     upper pair comes round in front as a shield wall. Forward is -Z.";

/// A half-shell and its arm: name, the spine node of its arm, its beams, radius (m), the
/// height of the shell's middle (m, vehicle frame) and its facet part.
struct ShellSpec {
    name: &'static str,
    node: &'static str,
    /// The beam from the shoulder to the knuckle, and the forearm after the elbow.
    boom: &'static str,
    forearm: &'static str,
    radius: f64,
    middle: f64,
    facet: &'static str,
}

const SHELLS: [ShellSpec; 2] = [
    ShellSpec {
        name: "upper",
        node: "arm_upper",
        boom: "bastion.boom_short",
        forearm: "bastion.boom",
        radius: 5.0,
        middle: 0.3,
        facet: "bastion.shell_facet_upper",
    },
    ShellSpec {
        name: "lower",
        node: "arm_lower",
        boom: "bastion.boom_stub",
        forearm: "bastion.boom",
        radius: 4.2,
        middle: -8.0,
        facet: "bastion.shell_facet_lower",
    },
];

/// The shells wrap around this vertical axis (vehicle frame, x = 0).
const SHELL_AXIS_Z: f64 = 0.4;
/// Facets per half-shell (each spans 180° / FACETS).
const FACETS: usize = 4;
/// Height of each engine pair above (upper shells) or below (lower shells) the centre of mass
/// (m), and the spacing within a pair.
const ENGINE_GAP: f64 = 4.4;
const ENGINE_SPREAD: f64 = 2.4;

/// Outward direction of the left side at azimuth `phi` (radians): 0 behind (+Z), π/2 left
/// (−X), π in front (−Z).
fn radial_left(phi: f64) -> DVec3 {
    DVec3::new(-phi.sin(), 0.0, phi.cos())
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

    /// Attaches part `id` by its node `own` to `parent`'s node `to`, named `name`.
    fn join(&mut self, parent: &str, to: &str, id: &str, own: &str, name: &str) -> usize {
        let at = self.free(parent, to);
        let def = self.part(id);
        let i = attach_at_node(&mut self.v, &def, own, &at, 0.0).unwrap();
        self.v.parts[i].name = name.into();
        i
    }

    /// Like [`Builder::join`], turned about the node so that the part's X axis (a hinge's
    /// axis) lies along `want_x` as near as whole degrees allow.
    fn join_aligned(
        &mut self,
        parent: &str,
        to: &str,
        id: &str,
        own: &str,
        name: &str,
        want_x: DVec3,
    ) -> usize {
        let at = self.free(parent, to);
        let def = self.part(id);
        let mut best = (f64::MIN, 0.0);
        for d in 0..360 {
            let roll = f64::from(d).to_radians();
            let i = attach_at_node(&mut self.v, &def, own, &at, roll).unwrap();
            let score = (self.v.parts[i].rotation * DVec3::X).dot(want_x);
            self.v.parts.pop();
            if score > best.0 {
                best = (score, roll);
            }
        }
        let i = attach_at_node(&mut self.v, &def, own, &at, best.1).unwrap();
        self.v.parts[i].name = name.into();
        i
    }

    /// Like [`Builder::join`], turned about the node so that the new part's `end` node points
    /// as nearly along `want` as whole degrees allow (a bent bracket).
    fn join_bent(&mut self, parent: &str, to: &str, id: &str, name: &str, want: DVec3) -> usize {
        let at = self.free(parent, to);
        let def = self.part(id);
        let mut best = (f64::MIN, 0.0);
        for d in 0..360 {
            let roll = f64::from(d).to_radians();
            let i = attach_at_node(&mut self.v, &def, "base", &at, roll).unwrap();
            let score = self.v.parts[i].node_in_vehicle("end").unwrap().1.dot(want);
            self.v.parts.pop();
            if score > best.0 {
                best = (score, roll);
            }
        }
        let i = attach_at_node(&mut self.v, &def, "base", &at, best.1).unwrap();
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

    /// Joins on the left side and mirrors to the right: names `{base}_left` and `{base}_right`.
    fn pair(&mut self, parent: &str, to: &str, id: &str, own: &str, base: &str) {
        let i = self.join(
            &format!("{parent}_left"),
            to,
            id,
            own,
            &format!("{base}_left"),
        );
        self.mirror(i, &format!("{base}_right"));
    }

    /// Like [`Builder::pair`] with the left part's X axis along `want_x`.
    fn pair_aligned(&mut self, parent: &str, to: &str, id: &str, base: &str, want_x: DVec3) {
        let i = self.join_aligned(parent, to, id, "base", &format!("{base}_left"), want_x);
        self.mirror(i, &format!("{base}_right"));
    }

    /// Fixes part `id` on the surface of `parent` (left side) at `position` with `rotation`,
    /// and mirrors it: names `{base}_left` and `{base}_right`.
    fn surface_pair(
        &mut self,
        parent: &str,
        id: &str,
        position: DVec3,
        rotation: DQuat,
        base: &str,
    ) {
        let p = self.index(parent);
        let def = self.part(id);
        let i = attach_to_surface(&mut self.v, &def, p, position, rotation).unwrap();
        self.v.parts[i].name = format!("{base}_left");
        self.mirror(i, &format!("{base}_right"));
    }
}

/// Assembles the Bastion with the builder's operations.
fn assemble() -> Vehicle {
    let catalog = PartCatalog::load_dir(content().join("parts")).unwrap();
    let torso = catalog.get("frame.torso").unwrap().clone();
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

    // Body: head, waist, two arms and two hinged legs (the heavy frame's walking body).
    b.join("torso", "neck", "frame.head", "neck", "head");
    b.join("torso", "waist", "frame.waist", "top", "waist");
    let i = b.join(
        "torso",
        "shoulder_left",
        "frame.shoulder",
        "right",
        "shoulder_left",
    );
    b.mirror(i, "shoulder_right");
    b.pair("shoulder", "down", "frame.hinge", "base", "shoulder_joint");
    b.pair("shoulder_joint", "end", "frame.limb", "top", "upper_arm");
    b.pair("upper_arm", "bottom", "frame.hinge", "base", "elbow_joint");
    b.pair("elbow_joint", "end", "frame.limb", "top", "forearm");
    let i = b.join("waist", "hip_left", "frame.hinge", "base", "hip_joint_left");
    b.mirror(i, "hip_joint_right");
    b.pair("hip_joint", "end", "frame.leg", "top", "thigh");
    b.pair("thigh", "bottom", "frame.hinge", "base", "knee_joint");
    b.pair("knee_joint", "end", "frame.leg", "top", "shin");
    b.pair("shin", "bottom", "frame.hinge", "base", "ankle_joint");
    b.pair("ankle_joint", "end", "frame.foot", "top", "foot");

    // The dorsal spine on the torso's back, over the torso and waist.
    b.join("torso", "back", "bastion.spine", "front", "spine");

    // Four shell arms (built on the left, mirrored): shoulder swing (vertical axis: opens the
    // shell like a door), shoulder lift, a beam outward, a right-angle knuckle (folded, the arm
    // turns here to run forward along the inside of its shell), elbow (vertical: straightened,
    // it brings the shell round to the front), forearm, wrist twist (turns the shell), and the
    // mount the shell's facets are fixed to.
    for spec in &SHELLS {
        let along = b.free("spine", &format!("{}_left", spec.node)).direction;
        let name = |part: &str| format!("{}_{part}", spec.name);
        let i = b.join_aligned(
            "spine",
            &format!("{}_left", spec.node),
            "bastion.arm_hinge",
            "base",
            &format!("{}_left", name("swing")),
            DVec3::Y,
        );
        b.mirror(i, &format!("{}_right", name("swing")));
        b.pair_aligned(
            &format!("{}_left", name("swing")),
            "end",
            "bastion.arm_hinge",
            &name("lift"),
            DVec3::Y.cross(along).normalize(),
        );
        b.pair(&name("lift"), "end", spec.boom, "top", &name("boom"));
        let i = b.join_bent(
            &format!("{}_left", name("boom")),
            "bottom",
            "bastion.knuckle",
            &format!("{}_left", name("knuckle")),
            DVec3::NEG_Z,
        );
        b.mirror(i, &format!("{}_right", name("knuckle")));
        b.pair_aligned(
            &format!("{}_left", name("knuckle")),
            "end",
            "bastion.arm_hinge",
            &name("elbow"),
            DVec3::Y,
        );
        b.pair(&name("elbow"), "end", spec.forearm, "top", &name("forearm"));
        b.pair(
            &name("forearm"),
            "bottom",
            "bastion.arm_rotor",
            "base",
            &name("wrist"),
        );
        b.pair(
            &name("wrist"),
            "end",
            "bastion.shell_mount",
            "base",
            &name("mount"),
        );

        // The half-shell: flat facets around the axis, from behind to in front.
        let span = std::f64::consts::PI / FACETS as f64;
        for k in 0..FACETS {
            let phi = span * (k as f64 + 0.5);
            let out = radial_left(phi);
            let centre =
                DVec3::new(0.0, spec.middle, SHELL_AXIS_Z) + out * spec.radius * (span / 2.0).cos();
            // Facet axes: X along the shell's curve, Y up, Z outward.
            let rotation = DQuat::from_mat3(&DMat3::from_cols(DVec3::Y.cross(out), DVec3::Y, out));
            b.surface_pair(
                &format!("{}_left", name("mount")),
                spec.facet,
                centre,
                rotation,
                &name(&format!("facet_{}", k + 1)),
            );
        }
    }

    // Engines: two on the outer face of each shell's rear facet, nozzles straight back; the
    // upper pairs as far above the centre of mass as the lower pairs below it, so the forward
    // thrust line passes through it.
    let com_y = VehicleBody::of(&b.v).unwrap().mass.center_of_mass.y;
    let nozzle_back = DQuat::from_rotation_x(-std::f64::consts::FRAC_PI_2);
    for spec in &SHELLS {
        let facet = b.index(&format!("{}_facet_1_left", spec.name));
        let (centre, rotation) = (b.v.parts[facet].position, b.v.parts[facet].rotation);
        let out = rotation * DVec3::Z;
        let y = if spec.name == "upper" {
            com_y + ENGINE_GAP
        } else {
            com_y - ENGINE_GAP
        };
        for (n, dy) in [ENGINE_SPREAD / 2.0, -ENGINE_SPREAD / 2.0]
            .into_iter()
            .enumerate()
        {
            let at = DVec3::new(centre.x, y + dy, centre.z) + out * (0.25 + 0.9);
            b.surface_pair(
                &format!("{}_facet_1_left", spec.name),
                "thruster.main.medium",
                at,
                nozzle_back,
                &format!("{}_engine_{}", spec.name, n + 1),
            );
        }
    }

    // The gyro inside the torso.
    let gyro = b.part("frame.gyro");
    let i = attach_to_surface(&mut b.v, &gyro, 0, DVec3::ZERO, DQuat::IDENTITY).unwrap();
    b.v.parts[i].name = "gyro".into();

    // Walking: the heavy frame's legs and gait.
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

/// One shell arm: its joint groups, the group its facets move with, and the facets.
struct ShellArm {
    swing: usize,
    lift: usize,
    elbow: usize,
    wrist: usize,
    group: usize,
    facets: Vec<usize>,
}

fn shell_arm(v: &Vehicle, a: &Articulation, shell: &str, side: &str) -> ShellArm {
    let part = |name: String| {
        v.parts
            .iter()
            .position(|p| p.name == name)
            .unwrap_or_else(|| panic!("no part {name}"))
    };
    let joint = |j: &str| {
        a.group_of_joint(part(format!("{shell}_{j}_{side}")))
            .unwrap()
    };
    let facets: Vec<usize> = (1..=FACETS)
        .map(|k| part(format!("{shell}_facet_{k}_{side}")))
        .collect();
    ShellArm {
        swing: joint("swing"),
        lift: joint("lift"),
        elbow: joint("elbow"),
        wrist: joint("wrist"),
        group: a.part_group[facets[0]],
        facets,
    }
}

/// Where a shell is for joint `angles`: the middle of its facets and its outward direction
/// (the mean of the facets' outward normals).
fn shell_at(v: &Vehicle, a: &Articulation, arm: &ShellArm, angles: &[f64]) -> (DVec3, DVec3) {
    let poses: Vec<Pose> = a.poses(angles);
    let pose = poses[arm.group];
    let n = arm.facets.len() as f64;
    let centre = arm
        .facets
        .iter()
        .map(|&f| pose.apply(v.parts[f].position))
        .sum::<DVec3>()
        / n;
    let out = arm
        .facets
        .iter()
        .map(|&f| pose.rotation * (v.parts[f].rotation * DVec3::Z))
        .sum::<DVec3>()
        .normalize();
    (centre, out)
}

/// Sets `joints` (each searched in whole 15° steps within ±range degrees) to bring the shell's
/// middle nearest `target`, facing `facing`.
fn place_shell(
    v: &Vehicle,
    a: &Articulation,
    arm: &ShellArm,
    angles: &mut [f64],
    joints: &[(usize, f64)],
    target: DVec3,
    facing: DVec3,
) {
    let steps: Vec<Vec<f64>> = joints
        .iter()
        .map(|&(_, range)| {
            let n = (range / 15.0) as i32;
            (-n..=n).map(|k| f64::from(k * 15).to_radians()).collect()
        })
        .collect();
    let mut best = (f64::MAX, vec![0.0; joints.len()]);
    let mut pick = vec![0usize; joints.len()];
    let mut trial = angles.to_vec();
    'search: loop {
        for ((g, _), (k, s)) in joints.iter().zip(pick.iter().zip(&steps)) {
            trial[*g] = s[*k];
        }
        let (centre, out) = shell_at(v, a, arm, &trial);
        let cost = centre.distance(target) + 4.0 * (1.0 - out.dot(facing));
        if cost < best.0 {
            best = (cost, joints.iter().map(|(g, _)| trial[*g]).collect());
        }
        let mut j = 0;
        loop {
            if j == pick.len() {
                break 'search;
            }
            pick[j] += 1;
            if pick[j] < steps[j].len() {
                break;
            }
            pick[j] = 0;
            j += 1;
        }
    }
    for ((g, _), angle) in joints.iter().zip(best.1) {
        angles[*g] = angle;
    }
}

/// Where the poses put the shells' middles (left side; the right mirrors x).
const STAND_UPPER: DVec3 = DVec3::new(-3.6, 0.3, 4.0);
const STAND_LOWER: DVec3 = DVec3::new(-3.0, -8.0, 3.6);
/// Wings: the shells lifted out to the sides (upper up and out, lower out and a little up).
const WINGS_UPPER_FACING: DVec3 = DVec3::new(-0.6, 0.8, 0.0);
const WINGS_LOWER_FACING: DVec3 = DVec3::new(-0.9, 0.44, 0.0);

fn mirrored(p: DVec3, side: &str) -> DVec3 {
    if side == "left" {
        p
    } else {
        DVec3::new(-p.x, p.y, p.z)
    }
}

/// The Bastion's poses, solved for its geometry.
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
    // Stand: the shells swung back behind the body like folded wing cases, facing outward
    // and back; the front is open for work.
    let mut stand = vec![0.0; n];
    // Wings: the shells lifted out to the sides about their fore-and-aft shoulder axes; the
    // engines on their rear facets keep pointing straight back.
    let mut wings = vec![0.0; n];
    for side in ["left", "right"] {
        let outward = mirrored(DVec3::new(-1.0, 0.0, 1.0).normalize(), side);
        for (shell, target) in [("upper", STAND_UPPER), ("lower", STAND_LOWER)] {
            let arm = shell_arm(v, a, shell, side);
            place_shell(
                v,
                a,
                &arm,
                &mut stand,
                &[(arm.swing, 120.0), (arm.elbow, 120.0), (arm.wrist, 90.0)],
                mirrored(target, side),
                outward,
            );
        }
        for (shell, facing) in [("upper", WINGS_UPPER_FACING), ("lower", WINGS_LOWER_FACING)] {
            let arm = shell_arm(v, a, shell, side);
            let facing = mirrored(facing, side).normalize();
            // Only the lift moves: the best facing, wherever that puts the shell.
            let mut best = (f64::MIN, 0.0);
            for d in -120..=120 {
                let mut trial = wings.clone();
                trial[arm.lift] = f64::from(d).to_radians();
                let (_, out) = shell_at(v, a, &arm, &trial);
                if out.dot(facing) > best.0 {
                    best = (out.dot(facing), trial[arm.lift]);
                }
            }
            wings[arm.lift] = best.1;
        }
    }
    vec![
        named("cocoon", &vec![0.0; n]),
        named("stand", &stand),
        named("wings", &wings),
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
        "{} parts, {} joints, {:.1} t, {:.1} x {:.1} x {:.1} m, com {com:?}, thrust/weight {:.2}, turn {:.2} rad/s2, problems {:?}",
        s.parts,
        body.articulation.groups.len() - 1,
        s.mass_kg / 1000.0,
        s.size.x,
        s.size.y,
        s.size.z,
        s.thrust_to_weight,
        s.turn_acceleration,
        s.problems
    );
    assert!(s.problems.is_empty(), "{:?}", s.problems);
    assert!(s.thrust_to_weight > 1.5);
    // Each arm's mount meets the inside of its shell.
    for spec in &SHELLS {
        let mount = &v.parts[v
            .parts
            .iter()
            .position(|p| p.name == format!("{}_mount_left", spec.name))
            .unwrap()];
        let face = mount.position + mount.rotation * DVec3::new(0.0, -0.2, 0.0);
        let r = DVec3::new(face.x, 0.0, face.z - SHELL_AXIS_Z).length();
        let inside = spec.radius * (std::f64::consts::PI / FACETS as f64 / 2.0).cos() - 0.25;
        println!(
            "{} mount reaches r {r:.2} m; the shell's inside is at {inside:.2} m",
            spec.name
        );
        assert!(
            r < inside && r > inside - 0.7,
            "{}: mount at {r:.2} m, shell at {inside:.2} m",
            spec.name
        );
    }
}

#[test]
fn closed_the_shells_make_a_cocoon_around_the_body() {
    let v = assemble();
    let body = VehicleBody::of(&v).unwrap();
    let a = &body.articulation;
    let rest = vec![0.0; a.groups.len()];
    // Every other part is inside the shells at its height (the shins and feet walk below the
    // lower shells; the arms that carry the shells fold inside them).
    for p in &v.parts {
        if p.name.contains("facet") || p.name.contains("engine") {
            continue;
        }
        let r = DVec3::new(p.position.x, 0.0, p.position.z - SHELL_AXIS_Z).length();
        let limit = if p.position.y > -4.0 {
            5.0
        } else if p.position.y > -12.0 {
            4.2
        } else {
            continue;
        };
        assert!(
            r < limit - 0.25,
            "{} at r {r:.2} m pokes out of the shell ({limit} m)",
            p.name
        );
    }
    // The two halves of each pair are mirror images facing out to the sides.
    for shell in ["upper", "lower"] {
        let (lc, lo) = shell_at(&v, a, &shell_arm(&v, a, shell, "left"), &rest);
        let (rc, ro) = shell_at(&v, a, &shell_arm(&v, a, shell, "right"), &rest);
        assert!(
            (lc.x + rc.x).abs() < 1e-6 && (lc.z - rc.z).abs() < 1e-6,
            "{shell} mirrored"
        );
        assert!(
            lo.x < -0.9 && ro.x > 0.9,
            "{shell}: halves face out to the sides"
        );
    }
}

#[test]
fn the_poses_open_the_shells_back_and_spread_them_as_wings() {
    let v = design();
    let body = VehicleBody::of(&v).unwrap();
    let a = &body.articulation;
    let (stand, wings) = (pose(&v, a, "stand"), pose(&v, a, "wings"));
    for side in ["left", "right"] {
        for (shell, target) in [("upper", STAND_UPPER), ("lower", STAND_LOWER)] {
            let (c, o) = shell_at(&v, a, &shell_arm(&v, a, shell, side), &stand);
            let t = mirrored(target, side);
            println!("stand {shell} {side}: middle {c:.1?} (target {t:.1?}), facing {o:.2?}");
            assert!(c.distance(t) < 2.0, "stand {shell} {side}");
            assert!(
                c.z > 2.5,
                "stand {shell} {side}: swung back behind the body"
            );
        }
        for shell in ["upper", "lower"] {
            let (c, o) = shell_at(&v, a, &shell_arm(&v, a, shell, side), &wings);
            println!("wings {shell} {side}: middle {c:.1?}, facing {o:.2?}");
            let sx = if side == "left" { -1.0 } else { 1.0 };
            assert!(
                o.x * sx > 0.5,
                "wings {shell} {side}: out to the side ({o:?})"
            );
        }
        let (_, o) = shell_at(&v, a, &shell_arm(&v, a, "upper", side), &wings);
        assert!(o.y > 0.5, "wings: the upper shells lifted ({o:?})");
    }
    // Spreading the wings keeps the engines pointing straight back: forward thrust stays.
    let forward = |angles: &[f64]| {
        body.posed_allocator(&a.poses(angles))
            .thrusters()
            .iter()
            .map(|t| -t.direction.z * t.max_thrust)
            .filter(|f| *f > 0.0)
            .sum::<f64>()
    };
    let (closed, spread) = (forward(&vec![0.0; a.groups.len()]), forward(&wings));
    println!(
        "forward thrust: cocoon {:.0} kN, wings {:.0} kN",
        closed / 1e3,
        spread / 1e3
    );
    assert!(spread > 0.95 * closed, "wings keep the thrust forward");
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
        position: up * (ground + 15.0),
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
