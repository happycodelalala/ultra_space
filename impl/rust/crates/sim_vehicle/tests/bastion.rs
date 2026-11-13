//! The Bastion: an original heavy frame (~22 m, ~130 t) with four manipulator arms on a shoulder
//! yoke and six armour slabs hinged under the yoke's rim that close into a six-sided shell around
//! the body. Built only with the builder's operations, on the heavy frame's walking body (torso,
//! arms, hinged legs with ankles). `content/data/vehicles/bastion.json` is this design; refresh
//! it with `cargo test -p sim_vehicle --test bastion -- --ignored`.

use std::path::PathBuf;
use std::sync::Arc;

use glam::{DQuat, DVec3};
use sim_vehicle::builder::{
    FreeNode, Symmetry, attach_at_node, attach_to_surface, free_nodes, place_symmetric, stats,
};
use sim_vehicle::design::{NamedPose, check_join, design_json};
use sim_vehicle::gait::{GaitDef, LegDef};
use sim_vehicle::joints::{Articulation, JointState};
use sim_vehicle::{PartCatalog, PartDef, Vehicle, VehicleBody, VehicleDesign, VehiclePart};

fn content() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../../content/data")
}

const NAME: &str = "Bastion";
const DESCRIPTION: &str = "An original heavy frame, about 22 m and 130 tonnes: a walking \
     humanoid body, a wide yoke over the shoulders carrying four manipulator arms with three-jaw \
     clamps, and six armour slabs hinged under the yoke that close into a six-sided shell. \
     Engines on the three rear slabs. Forward is -Z.";

/// The six shell slabs, clockwise from the front seen from above (azimuth 60° apart).
const SHELLS: [&str; 6] = [
    "front",
    "front_right",
    "back_right",
    "back",
    "back_left",
    "front_left",
];

/// How far down each rear slab its two engines sit, from the slab's centre (m along the slab),
/// chosen so forward thrust passes through the centre of mass.
const ENGINE_ALONG: f64 = -1.6;

struct Builder {
    v: Vehicle,
    catalog: PartCatalog,
}

impl Builder {
    fn part(&self, id: &str) -> Arc<PartDef> {
        self.catalog.get(id).unwrap().clone()
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

    /// Like [`join`], turned about the node so that the part's X axis lies along `want_x`
    /// (as near as whole degrees allow).
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
        let roll = best.1;
        let i = attach_at_node(&mut self.v, &def, own, &at, roll).unwrap();
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

    // The yoke over the shoulders, on the torso's top.
    let yoke = b.part("bastion.yoke");
    let i = attach_to_surface(
        &mut b.v,
        &yoke,
        0,
        DVec3::new(0.0, 2.9, 0.0),
        DQuat::IDENTITY,
    )
    .unwrap();
    b.v.parts[i].name = "yoke".into();

    // Shell: a long hinge and a slab under each of the yoke's six rim nodes, the hinge's axis
    // along the rim so the slab swings in and out.
    for (k, shell) in SHELLS.iter().enumerate() {
        let azimuth = f64::from(k as u32 * 60).to_radians();
        let tangent = DVec3::new(azimuth.cos(), 0.0, azimuth.sin());
        b.join_aligned(
            "yoke",
            &format!("shell_{shell}"),
            "bastion.shell_hinge",
            "base",
            &format!("shell_hinge_{shell}"),
            tangent,
        );
        b.join(
            &format!("shell_hinge_{shell}"),
            "end",
            "bastion.shell_slab",
            "root",
            &format!("shell_{shell}"),
        );
    }

    // Engines: two on the outer face of each rear slab, nozzles back.
    let engine = b.part("thruster.main.medium");
    for shell in ["back_right", "back", "back_left"] {
        let s = b.index(&format!("shell_{shell}"));
        let (centre, rotation) = (b.v.parts[s].position, b.v.parts[s].rotation);
        // The slab's outer face: its local Z axis or the opposite, whichever points away from
        // the body's axis.
        let normal = rotation * DVec3::Z;
        let outward = if normal.dot(DVec3::new(centre.x, 0.0, centre.z)) > 0.0 {
            normal
        } else {
            -normal
        };
        for (n, across) in [-1.4, 1.4].into_iter().enumerate() {
            let at =
                centre + rotation * DVec3::new(across, ENGINE_ALONG, 0.0) + outward * (0.4 + 0.9);
            let nozzle_back = DQuat::from_rotation_x(-std::f64::consts::FRAC_PI_2);
            let i = attach_to_surface(&mut b.v, &engine, s, at, nozzle_back).unwrap();
            b.v.parts[i].name = format!("engine_{shell}_{}", n + 1);
        }
    }

    // Arm turrets on the yoke, mirrored.
    let i = b.join(
        "yoke",
        "turret_left",
        "bastion.turret",
        "bottom",
        "turret_left",
    );
    b.mirror(i, "turret_right");

    // Four manipulator arms: an upper one rising from each turret's top and a lower one from
    // its outer side. Rotor (twist), shoulder hinge, segment, elbow hinge, segment, wrist hinge,
    // palm, three jaws.
    for (arm, node) in [("upper", "up"), ("lower", "left")] {
        let name = |part: &str| format!("{arm}_{part}");
        let i = b.join(
            "turret_left",
            node,
            "bastion.arm_rotor",
            "base",
            &format!("{}_left", name("rotor")),
        );
        b.mirror(i, &format!("{}_right", name("rotor")));
        let chain = [
            ("rotor", "bastion.arm_hinge", "base", "shoulder"),
            ("shoulder", "bastion.arm_segment", "top", "boom"),
            ("boom", "bastion.arm_hinge", "base", "elbow"),
            ("elbow", "bastion.arm_segment", "top", "reach"),
            ("reach", "bastion.arm_hinge", "base", "wrist"),
            ("wrist", "bastion.clamp_palm", "top", "palm"),
        ];
        for (parent, id, own, part) in chain {
            // Joints hand on at their end node, segments at their bottom.
            let to = if matches!(parent, "boom" | "reach") {
                "bottom"
            } else {
                "end"
            };
            b.pair(&name(parent), to, id, own, &name(part));
        }
        // Jaws: each hinge turned so a positive angle opens its jaw outward.
        for jaw in ["a", "b", "c"] {
            let palm = b.index(&format!("{}_left", name("palm")));
            let (pp, pr) = (b.v.parts[palm].position, b.v.parts[palm].rotation);
            let node = b.v.parts[palm]
                .node_in_vehicle(&format!("jaw_{jaw}"))
                .unwrap()
                .0;
            let up = pr * DVec3::Y;
            let radial = (node - pp) - up * (node - pp).dot(up);
            let opens = radial.normalize().cross(up);
            let i = b.join_aligned(
                &format!("{}_left", name("palm")),
                &format!("jaw_{jaw}"),
                "bastion.jaw_hinge",
                "base",
                &format!("{}_left", name(&format!("jaw_hinge_{jaw}"))),
                opens,
            );
            b.mirror(i, &format!("{}_right", name(&format!("jaw_hinge_{jaw}"))));
            b.pair(
                &name(&format!("jaw_hinge_{jaw}")),
                "end",
                "bastion.jaw",
                "top",
                &name(&format!("jaw_{jaw}")),
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

/// Every group's joint angle that brings `point` of `part` (rest frame) nearest `target`, among
/// a few joints searched on a grid of whole steps (a tiny brute-force solver for posing arms).
fn reach(
    a: &Articulation,
    angles: &mut [f64],
    groups: &[usize],
    part_group: usize,
    point: DVec3,
    target: DVec3,
) {
    let steps: Vec<f64> = (-12..=12).map(|k| f64::from(k * 15).to_radians()).collect();
    let mut best = (f64::MAX, vec![0.0; groups.len()]);
    let mut trial = angles.to_vec();
    let mut pick = vec![0usize; groups.len()];
    loop {
        for (g, &k) in groups.iter().zip(&pick) {
            trial[*g] = steps[k];
        }
        let poses = a.poses(&trial);
        let d = poses[part_group].apply(point).distance(target);
        if d < best.0 {
            best = (d, groups.iter().map(|&g| trial[g]).collect());
        }
        // Next combination.
        let mut j = 0;
        loop {
            if j == pick.len() {
                for (g, angle) in groups.iter().zip(&best.1) {
                    angles[*g] = *angle;
                }
                return;
            }
            pick[j] += 1;
            if pick[j] < steps.len() {
                break;
            }
            pick[j] = 0;
            j += 1;
        }
    }
}

/// The Bastion's poses, solved for its geometry: shell closed and open, arms deployed and
/// reaching forward to grab, arms stowed.
fn poses(v: &Vehicle) -> Vec<NamedPose> {
    let body = VehicleBody::of(v).unwrap();
    let a = &body.articulation;
    let group = |name: &str| {
        let part = v.parts.iter().position(|p| p.name == name).unwrap();
        a.group_of_joint(part).unwrap_or_else(|| a.part_group[part])
    };
    let rest = || vec![0.0; a.groups.len()];
    let named = |name: &str, angles: &[f64]| NamedPose {
        name: name.into(),
        angles: v
            .parts
            .iter()
            .enumerate()
            .filter_map(|(i, p)| {
                let g = a.group_of_joint(i)?;
                let deg = angles[g].to_degrees();
                (deg.abs() > 1e-9).then(|| (p.name.clone(), deg.round()))
            })
            .collect(),
    };

    // Shell: the angle that hangs each slab straight down (closed), and 45° further out (open).
    let shell_closed: Vec<(usize, f64)> = SHELLS
        .iter()
        .map(|s| {
            let g = group(&format!("shell_hinge_{s}"));
            let slab = v
                .parts
                .iter()
                .position(|p| p.name == format!("shell_{s}"))
                .unwrap();
            let pivot = a.groups[g].pivot;
            let hang = v.parts[slab].position - pivot;
            let best = (-90..=90)
                .map(f64::from)
                .min_by(|x, y| {
                    let down = |deg: f64| {
                        let d = DQuat::from_axis_angle(a.groups[g].axis, deg.to_radians()) * hang;
                        -d.normalize().y
                    };
                    down(*y).total_cmp(&down(*x))
                })
                .unwrap();
            (g, best.to_radians())
        })
        .collect();
    let mut closed = rest();
    let mut open = rest();
    for &(g, angle) in &shell_closed {
        closed[g] = angle;
        open[g] = -angle.signum() * 45f64.to_radians();
    }

    // Arms: rotor, shoulder and elbow searched so each palm reaches a target point.
    let arm_groups = |arm: &str, side: &str| {
        [
            group(&format!("{arm}_rotor_{side}")),
            group(&format!("{arm}_shoulder_{side}")),
            group(&format!("{arm}_elbow_{side}")),
        ]
    };
    let palm = |arm: &str, side: &str| {
        let i = v
            .parts
            .iter()
            .position(|p| p.name == format!("{arm}_palm_{side}"))
            .unwrap();
        (a.part_group[i], v.parts[i].position)
    };
    let jaws = |angles: &mut Vec<f64>, deg: f64| {
        for arm in ["upper", "lower"] {
            for side in ["left", "right"] {
                for jaw in ["a", "b", "c"] {
                    let g = group(&format!("{arm}_jaw_hinge_{jaw}_{side}"));
                    // Mirrored hinges turn the other way: check which sign opens.
                    let axis = a.groups[g].axis;
                    let jaw_part = v
                        .parts
                        .iter()
                        .position(|p| p.name == format!("{arm}_jaw_{jaw}_{side}"))
                        .unwrap();
                    let hang = v.parts[jaw_part].position - a.groups[g].pivot;
                    let (pg, pp) = palm(arm, side);
                    let _ = pg;
                    let outward = a.groups[g].pivot - pp;
                    let sign = axis.cross(hang).dot(outward).signum();
                    angles[g] = sign * deg.to_radians();
                }
            }
        }
    };
    let arms = |angles: &mut Vec<f64>, targets: [(&str, DVec3); 2]| {
        for (arm, target) in targets {
            for (side, x) in [("left", 1.0), ("right", -1.0)] {
                let (pg, pp) = palm(arm, side);
                let t = DVec3::new(target.x * x, target.y, target.z);
                reach(a, angles, &arm_groups(arm, side), pg, pp, t);
            }
        }
    };

    let mut deployed = rest();
    arms(
        &mut deployed,
        [
            ("upper", DVec3::new(-8.0, 11.0, 1.0)),
            ("lower", DVec3::new(-11.0, 3.0, -1.0)),
        ],
    );
    jaws(&mut deployed, 35.0);
    let mut grab = rest();
    arms(
        &mut grab,
        [
            ("upper", DVec3::new(-3.0, 3.0, -6.0)),
            ("lower", DVec3::new(-4.5, -1.0, -6.5)),
        ],
    );
    jaws(&mut grab, -20.0);
    let mut stowed = rest();
    arms(
        &mut stowed,
        [
            ("upper", DVec3::new(-2.5, 5.0, 4.5)),
            ("lower", DVec3::new(-3.0, 1.0, 5.0)),
        ],
    );
    // Shell closed with the arms stowed inside its top.
    let mut closed_stowed = stowed.clone();
    for &(g, angle) in &shell_closed {
        closed_stowed[g] = angle;
    }
    vec![
        // Standing (the pose it starts in): arms folded back.
        named("stand", &stowed),
        named("arms_deployed", &deployed),
        named("arms_grab", &grab),
        named("arms_stowed", &stowed),
        named("shell_closed", &closed_stowed),
        named("shell_open", &open),
    ]
}

fn design() -> Vehicle {
    let mut v = assemble();
    v.poses = poses(&v);
    v
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
    // Six shell hinges with their axes along the yoke's rim.
    for s in SHELLS {
        let part = v
            .parts
            .iter()
            .position(|p| p.name == format!("shell_hinge_{s}"))
            .unwrap();
        let g = body.articulation.group_of_joint(part).unwrap();
        let radial = DVec3::new(v.parts[part].position.x, 0.0, v.parts[part].position.z);
        let axis = body.articulation.groups[g].axis;
        assert!(
            axis.dot(radial.normalize()).abs() < 0.05 && axis.y.abs() < 0.05,
            "{s}: axis {axis:?} along the rim"
        );
    }
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
    assert!(s.thrust_to_weight > 1.2);
}

#[test]
fn the_bastion_closes_its_shell_and_moves_its_arms() {
    let v = design();
    let body = VehicleBody::of(&v).unwrap();
    let a = &body.articulation;
    let at = |pose: &str| {
        let mut s = JointState::rest(a);
        for (joint, deg) in &v.poses.iter().find(|p| p.name == pose).unwrap().angles {
            let part = v.parts.iter().position(|p| &p.name == joint).unwrap();
            s.angles[a.group_of_joint(part).unwrap()] = deg.to_radians();
        }
        a.poses(&s.angles)
    };
    let where_is = |poses: &[sim_vehicle::joints::Pose], name: &str| {
        let i = v.parts.iter().position(|p| p.name == name).unwrap();
        poses[a.part_group[i]].apply(v.parts[i].position)
    };
    // Closed: every slab hangs straight down, its centre on a ring around the body; open: out.
    let closed = at("shell_closed");
    let open = at("shell_open");
    for s in SHELLS {
        let c = where_is(&closed, &format!("shell_{s}"));
        let o = where_is(&open, &format!("shell_{s}"));
        let r = |p: DVec3| DVec3::new(p.x, 0.0, p.z).length();
        println!(
            "shell {s}: closed {c:.1?} (r {:.1}), open {o:.1?} (r {:.1})",
            r(c),
            r(o)
        );
        assert!((4.0..6.5).contains(&r(c)), "{s} closed around the body");
        assert!(r(o) > r(c) + 3.0, "{s} swings out when open");
    }
    // Arms: palms reach their targets within a couple of metres.
    for (pose, arm, target) in [
        ("arms_deployed", "upper", DVec3::new(-8.0, 11.0, 1.0)),
        ("arms_deployed", "lower", DVec3::new(-11.0, 3.0, -1.0)),
        ("arms_grab", "upper", DVec3::new(-3.0, 3.0, -6.0)),
        ("arms_grab", "lower", DVec3::new(-4.5, -1.0, -6.5)),
    ] {
        let poses = at(pose);
        for (side, x) in [("left", 1.0), ("right", -1.0)] {
            let p = where_is(&poses, &format!("{arm}_palm_{side}"));
            let t = DVec3::new(target.x * x, target.y, target.z);
            println!("{pose} {arm} {side}: palm {p:.1?}, target {t:.1?}");
            assert!(p.distance(t) < 2.5, "{pose} {arm} {side}: {p:?} vs {t:?}");
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

/// Walks a few seconds with the shell open on a slope on Meridian, stops, and hands the frame
/// to physics: it must stay standing.
#[test]
fn the_bastion_walks_and_stops_standing_with_its_shell_open() {
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
    for (joint, deg) in &v
        .poses
        .iter()
        .find(|p| p.name == "shell_open")
        .unwrap()
        .angles
    {
        let part = v.parts.iter().position(|p| &p.name == joint).unwrap();
        let g = a.group_of_joint(part).unwrap();
        joints.angles[g] = deg.to_radians();
        joints.targets[g] = deg.to_radians();
    }
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
    let rotation = DQuat::from_mat3(&glam::DMat3::from_cols(north.cross(up), up, -north));
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
        if k % 30 == 0 {
            let tilt = (r.rotation * DVec3::Y).angle_between(r.position.normalize());
            println!("t {} tilt {:.1}", k / 30, tilt.to_degrees());
        }
    }
    let tilt = (r.rotation * DVec3::Y)
        .angle_between(r.position.normalize())
        .to_degrees();
    assert!(tilt < 10.0, "stands after stopping: tilted {tilt:.1} deg");
}
