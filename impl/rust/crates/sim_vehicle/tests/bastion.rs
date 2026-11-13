//! The Bastion: an original heavy frame (~22 m, ~110 t) whose armour moves. A saddle bolted
//! on each shoulder carries two shell arms, one at the front and one at the back; each arm
//! reaches up over the shoulder and holds a curved quarter-shell by its crown. Closed, the four
//! quarters make a cocoon around the body — domed over the head, widest at the shoulders,
//! tapering to mid-thigh so the legs walk free below — with the arms inside the dome, where
//! there is room. Hung from their crowns, the quarters stay closed under their own weight. Each
//! arm opens its quarter like a petal. Thrusters on the rear quarters point straight back when
//! closed. Every pose and every move between poses is checked free of interference.
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
const DESCRIPTION: &str = "An original heavy frame, about 22 m and 110 tonnes, whose armour \
     moves: a saddle on each shoulder carries two shell arms, front and back, each holding a \
     curved quarter-shell by its crown. Closed, the quarters make a cocoon domed over the head \
     and tapering to mid-thigh, the arms inside the dome and the engines on the rear quarters \
     pointing straight back; each arm opens its quarter like a petal. Forward is -Z.";

/// The quarters on the left: name, the saddle node of its arm, and the azimuth of its middle
/// (0 behind, 90 left, 180 in front, degrees).
const QUARTERS: [(&str, &str, f64); 2] = [("front", "front", 135.0), ("back", "back", 45.0)];

/// The cocoon wraps around this vertical axis (vehicle frame, x = 0).
const SHELL_AXIS_Z: f64 = 0.3;
/// Where the shoulder saddles sit (left; on top of the shoulder block, clear of the torso).
const SADDLE_AT: DVec3 = DVec3::new(-3.35, 2.55, 0.0);
/// Overlap allowed between parts that aren't joined (m): the seams between neighbouring
/// quarters.
const TOLERANCE: f64 = 0.05;
/// Height of each engine pair above (upper) or below (lower) the centre of mass, the spacing
/// within a pair (m), and the azimuth they sit at (near the back seam, degrees).
const ENGINE_GAP: f64 = 3.5;
const ENGINE_SPREAD: f64 = 2.0;
const ENGINE_AZIMUTH: f64 = 30.0;
/// How far the poses open the quarters at most (degrees).
const OPEN_LIMIT: f64 = 100.0;

/// Outward direction of the left side at azimuth `deg`: 0 behind (+Z), 90 left (−X), 180 in
/// front (−Z).
fn radial_left(deg: f64) -> DVec3 {
    let phi = deg.to_radians();
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

    /// Like [`Builder::join`] by the part's `base` node, turned about the node so that its X
    /// axis (a hinge's axis) lies along `want_x` as near as whole degrees allow.
    fn join_aligned(
        &mut self,
        parent: &str,
        to: &str,
        id: &str,
        name: &str,
        want_x: DVec3,
    ) -> usize {
        let at = self.free(parent, to);
        let def = self.part(id);
        let mut best = (f64::MIN, 0.0);
        for d in 0..360 {
            let roll = f64::from(d).to_radians();
            let i = attach_at_node(&mut self.v, &def, "base", &at, roll).unwrap();
            let score = (self.v.parts[i].rotation * DVec3::X).dot(want_x);
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

    /// Like [`Builder::pair`] for a hinge whose axis (on the left) lies along `want_x`.
    fn pair_hinge(&mut self, parent: &str, to: &str, id: &str, base: &str, want_x: DVec3) {
        let i = self.join_aligned(parent, to, id, &format!("{base}_left"), want_x);
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

    // A saddle bolted on each shoulder block.
    b.surface_pair(
        "shoulder_left",
        "bastion.saddle",
        SADDLE_AT,
        DQuat::IDENTITY,
        "saddle",
    );

    // Four shell arms (built on the left, mirrored), each from its saddle up over the head to
    // the inside of its quarter's dome: swing and lift at the shoulder (move the whole quarter),
    // a beam, and the open hinge the quarter hangs from (axis along the shell's curve, near the
    // crown: the quarter swings out from its top like a petal, nearly all of it below the
    // pivot, so its dome doesn't swing in onto the head or its neighbours).
    for (quarter, node, middle) in QUARTERS {
        let name = |part: &str| format!("{quarter}_{part}");
        let tangent = DVec3::Y.cross(radial_left(middle)).normalize();
        let along = b.free("saddle_left", node).direction;
        b.pair_hinge(
            "saddle_left",
            node,
            "bastion.arm_hinge",
            &name("swing"),
            DVec3::Y,
        );
        b.pair_hinge(
            &format!("{}_left", name("swing")),
            "end",
            "bastion.arm_hinge",
            &name("lift"),
            DVec3::Y.cross(along).normalize(),
        );
        b.pair(
            &name("lift"),
            "end",
            "bastion.arm_beam",
            "top",
            &name("beam"),
        );
        b.pair_hinge(
            &format!("{}_left", name("beam")),
            "bottom",
            "bastion.arm_hinge",
            &name("open"),
            tangent,
        );
        // The quarter hangs from the open hinge by its node near the crown, facing its
        // diagonal; its axis lands on the cocoon's axis. Each side has its own (handed) part.
        for (side, deg) in [("left", middle), ("right", -middle)] {
            let parent = format!("{}_{side}", name("open"));
            let at = b.free(&parent, "end");
            let def = b.part(&format!("bastion.shell_quarter_{quarter}_{side}"));
            let want = radial_left(deg);
            let mut best = (f64::MIN, 0.0);
            for d in 0..360 {
                let roll = f64::from(d).to_radians();
                let i = attach_at_node(&mut b.v, &def, "hang", &at, roll).unwrap();
                let score = (b.v.parts[i].rotation * DVec3::Z).dot(want);
                b.v.parts.pop();
                if score > best.0 {
                    best = (score, roll);
                }
            }
            let i = attach_at_node(&mut b.v, &def, "hang", &at, best.1).unwrap();
            b.v.parts[i].name = format!("{}_{side}", name("shell"));
        }
    }

    // Engines: on the outer face of each rear quarter near the back seam, nozzles straight
    // back; the upper pair as far above the centre of mass as the lower pair below it, so the
    // forward thrust line passes through it.
    let com_y = VehicleBody::of(&b.v).unwrap().mass.center_of_mass.y;
    let shell = b.part("bastion.shell_quarter_back_left").shape;
    let Shape::Shell { thickness_m, .. } = shell else {
        unreachable!("the quarter is a shell")
    };
    let nozzle_back = DQuat::from_rotation_x(-std::f64::consts::FRAC_PI_2);
    for (level, y) in [("upper", com_y + ENGINE_GAP), ("lower", com_y - ENGINE_GAP)] {
        for (n, dy) in [ENGINE_SPREAD / 2.0, -ENGINE_SPREAD / 2.0]
            .into_iter()
            .enumerate()
        {
            let y = y + dy;
            let r = profile_radius(&shell, y) + thickness_m / 2.0 + 0.9;
            let at = DVec3::new(0.0, y, SHELL_AXIS_Z) + radial_left(ENGINE_AZIMUTH) * r;
            // On each side by hand, on its own quarter (the quarters aren't mirrored copies).
            let engine = b.part("thruster.main.medium");
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

/// Opens the named quarters' petals from `start` as far as `limit` degrees (5° steps), keeping
/// every step free of interference. Returns the angles.
fn open_quarters(
    v: &Vehicle,
    a: &Articulation,
    start: &[f64],
    quarters: &[&str],
    limit: f64,
) -> Vec<f64> {
    let mut angles = start.to_vec();
    for &quarter in quarters {
        // Both sides together, mirror images (a pair opens symmetrically).
        let joints: Vec<(usize, f64)> = ["left", "right"]
            .iter()
            .map(|side| {
                let g = joint(v, a, &format!("{quarter}_open_{side}"));
                // The sign that swings the quarter outward.
                let out = |deg: f64| {
                    let mut t = angles.clone();
                    t[g] = deg.to_radians();
                    radius(quarter_at(v, a, quarter, side, &t))
                };
                (g, if out(10.0) > out(-10.0) { 1.0 } else { -1.0 })
            })
            .collect();
        let mut deg = 0.0;
        while deg + 5.0 <= limit {
            let mut next = angles.clone();
            for &(g, sign) in &joints {
                next[g] = sign * (deg + 5.0f64).to_radians();
            }
            if !clear_path(v, a, &angles, &next) {
                break;
            }
            angles = next;
            deg += 5.0;
        }
    }
    angles
}

/// The Bastion's poses: cocoon (closed), stand (the front quarters open: the front free for
/// work, the back armoured) and bloom (all four open like petals).
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
    let closed = vec![0.0; n];
    let stand = open_quarters(v, a, &closed, &["front"], OPEN_LIMIT);
    let bloom = open_quarters(v, a, &stand, &["back"], OPEN_LIMIT);
    vec![
        named("bloom", &bloom),
        named("cocoon", &closed),
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
    // Hung from their arms, the quarters sit around the cocoon's axis.
    for q in ["front", "back"] {
        for side in ["left", "right"] {
            let p = &v.parts[part(&v, &format!("{q}_shell_{side}"))];
            let axis = p.position - DVec3::new(0.0, 0.0, SHELL_AXIS_Z);
            let up = p.rotation * DVec3::Y;
            println!(
                "{q} {side} quarter: axis off by {:.3} m, tilted {:.2} deg",
                axis.length(),
                up.angle_between(DVec3::Y).to_degrees()
            );
            assert!(
                axis.length() < 0.06 && up.dot(DVec3::Y) > 0.9995,
                "{q} {side}"
            );
        }
    }
}

#[test]
fn closed_the_quarters_make_a_cocoon_with_nothing_in_the_way() {
    let v = assemble();
    let body = VehicleBody::of(&v).unwrap();
    let a = &body.articulation;
    let rest = vec![0.0; a.groups.len()];
    let found = interferences(&v, a, &rest, TOLERANCE);
    assert!(found.is_empty(), "closed: {}", report(&v, &found));
    // Seen from above, the four quarters' middles lie on the four diagonals.
    for (quarter, side, want) in [
        ("front", "left", DVec3::new(-1.0, 0.0, -1.0)),
        ("back", "left", DVec3::new(-1.0, 0.0, 1.0)),
        ("front", "right", DVec3::new(1.0, 0.0, -1.0)),
        ("back", "right", DVec3::new(1.0, 0.0, 1.0)),
    ] {
        let c = quarter_at(&v, a, quarter, side, &rest);
        let flat = DVec3::new(c.x, 0.0, c.z - SHELL_AXIS_Z).normalize();
        assert!(flat.dot(want.normalize()) > 0.99, "{quarter} {side}: {c:?}");
    }
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
    // The petals open well: the front quarters in stand, all four in bloom.
    let rest = vec![0.0; a.groups.len()];
    for side in ["left", "right"] {
        for (quarter, pose_name) in [("front", "stand"), ("front", "bloom"), ("back", "bloom")] {
            let closed = radius(quarter_at(&v, a, quarter, side, &rest));
            let open = radius(quarter_at(&v, a, quarter, side, &pose(&v, a, pose_name)));
            println!(
                "{pose_name} {quarter} {side}: middle {closed:.1} m -> {open:.1} m from the axis"
            );
            assert!(open > closed + 2.0, "{pose_name}: {quarter} {side} opens");
        }
    }
}

#[test]
fn the_legs_walk_inside_the_closed_cocoon() {
    let v = assemble();
    let body = VehicleBody::of(&v).unwrap();
    let a = &body.articulation;
    // Strides like the gait's (4 m steps, legs bent to 90 % when standing): hips swing about
    // ±20°, knees bend up to about 50°.
    for hip in [-20.0f64, -10.0, 0.0, 10.0, 20.0] {
        for knee in [0.0f64, -25.0, -50.0] {
            let mut angles = vec![0.0; a.groups.len()];
            for side in ["left", "right"] {
                angles[joint(&v, a, &format!("hip_joint_{side}"))] = hip.to_radians();
                angles[joint(&v, a, &format!("knee_joint_{side}"))] = knee.to_radians();
            }
            let found = interferences(&v, a, &angles, TOLERANCE);
            assert!(
                found.is_empty(),
                "hip {hip}, knee {knee}: {}",
                report(&v, &found)
            );
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
    let v = design();
    let body = VehicleBody::of(&v).unwrap();
    let a = &body.articulation;
    let rest = vec![0.0; a.groups.len()];
    println!(
        "rest: {}",
        report(&v, &interferences(&v, a, &rest, TOLERANCE))
    );
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

#[test]
#[ignore = "diagnostic: what blocks each petal from opening"]
fn what_blocks_opening() {
    let v = assemble();
    let body = VehicleBody::of(&v).unwrap();
    let a = &body.articulation;
    for q in ["front", "back"] {
        let g = joint(&v, a, &format!("{q}_open_left"));
        for deg in [-10.0f64, -15.0, -20.0, -30.0, -45.0, -60.0] {
            let mut angles = vec![0.0; a.groups.len()];
            angles[g] = deg.to_radians();
            let c = quarter_at(&v, a, q, "left", &angles);
            println!(
                "{q} open {deg}: middle r {:.2} -> {}",
                radius(c),
                report(&v, &interferences(&v, a, &angles, TOLERANCE))
            );
        }
    }
}
