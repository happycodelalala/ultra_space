//! The Bastion: an original heavy frame (~22 m, ~120 t) whose armour moves. A pod on each
//! shoulder carries two shell arms, one reaching down the front and one down the back; each arm
//! holds a faceted quarter-shell. Closed, the four quarters make a ring around the body seen from
//! above — a cocoon over the torso, head and thighs (wider at the shoulders, narrower at the
//! thighs), with the arms folded inside. Each arm opens its quarter like a petal. Thrusters on
//! the rear quarters point straight back when closed. Built only with the builder's operations,
//! on the heavy frame's walking body. `content/data/vehicles/bastion.json` is this design;
//! refresh it with `cargo test -p sim_vehicle --test bastion -- --ignored`.

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
const DESCRIPTION: &str = "An original heavy frame, about 22 m and 120 tonnes, whose armour \
     moves: a pod on each shoulder carries two shell arms, front and back, each holding a \
     faceted quarter-shell. Closed, the four quarters ring the body into a cocoon with the arms \
     folded inside and the engines on the rear quarters pointing straight back; each arm opens \
     its quarter like a petal. Forward is -Z.";

/// A quarter-shell and its arm: name, the shoulder pod's node, the arm's beam, and the
/// azimuths its facets span (left side; 0 behind, 90 left, 180 in front, degrees).
struct Quarter {
    name: &'static str,
    node: &'static str,
    beam: &'static str,
    from_deg: f64,
}

const QUARTERS: [Quarter; 2] = [
    Quarter {
        name: "front",
        node: "front",
        beam: "bastion.boom_stub",
        from_deg: 90.0,
    },
    Quarter {
        name: "back",
        node: "back",
        beam: "bastion.boom_short",
        from_deg: 0.0,
    },
];

/// The cocoon's two rings: radius (m), height of the middle (m), facet part. The upper ring
/// covers the torso and head, the lower the waist and thighs.
const RINGS: [(f64, f64, &str); 2] = [
    (5.0, 0.3, "bastion.shell_facet_upper"),
    (4.2, -8.0, "bastion.shell_facet_lower"),
];

/// The shells wrap around this vertical axis (vehicle frame, x = 0).
const SHELL_AXIS_Z: f64 = 0.4;
/// Each facet spans 45°: two per quarter and ring.
const FACET_DEG: f64 = 45.0;
/// Where the shoulder pods sit (left; on top of the shoulder block).
const POD_AT: DVec3 = DVec3::new(-3.3, 2.7, 0.0);
/// Height of each engine pair above (upper ring) or below (lower ring) the centre of mass
/// (m), and the spacing within a pair.
const ENGINE_GAP: f64 = 4.4;
const ENGINE_SPREAD: f64 = 2.4;

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

    /// Like [`Builder::join`], turned about the node so that the part's X axis (a hinge's
    /// axis) lies along `want_x` as near as whole degrees allow.
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
        let i = self.join_aligned(
            &format!("{parent}_left"),
            to,
            id,
            &format!("{base}_left"),
            want_x,
        );
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

    // A pod on each shoulder.
    b.surface_pair(
        "shoulder_left",
        "bastion.shoulder_pod",
        POD_AT,
        DQuat::IDENTITY,
        "pod",
    );

    // Four shell arms (built on the left, mirrored), each hanging from its pod down the inside
    // of its quarter: open (axis along the shell's curve: swings the quarter's bottom out like
    // a petal), sweep (turns it round the body), a beam, twist (turns it about the arm), and the
    // mount its facets are fixed to.
    for q in &QUARTERS {
        let middle = q.from_deg + FACET_DEG;
        let tangent = DVec3::Y.cross(radial_left(middle)).normalize();
        let name = |part: &str| format!("{}_{part}", q.name);
        let i = b.join_aligned(
            "pod_left",
            q.node,
            "bastion.arm_hinge",
            &format!("{}_left", name("open")),
            tangent,
        );
        b.mirror(i, &format!("{}_right", name("open")));
        b.pair_hinge(
            &name("open"),
            "end",
            "bastion.arm_hinge",
            &name("sweep"),
            radial_left(middle),
        );
        b.pair(&name("sweep"), "end", q.beam, "top", &name("beam"));
        b.pair(
            &name("beam"),
            "bottom",
            "bastion.arm_rotor",
            "base",
            &name("twist"),
        );
        b.pair(
            &name("twist"),
            "end",
            "bastion.shell_mount",
            "base",
            &name("mount"),
        );

        // The quarter: two facets per ring, around the axis.
        for (ring, (radius, height, facet)) in RINGS.iter().enumerate() {
            for k in 0..2 {
                let deg = q.from_deg + FACET_DEG * (k as f64 + 0.5);
                let out = radial_left(deg);
                let centre = DVec3::new(0.0, *height, SHELL_AXIS_Z)
                    + out * radius * (FACET_DEG.to_radians() / 2.0).cos();
                // Facet axes: X along the curve, Y up, Z outward.
                let rotation =
                    DQuat::from_mat3(&DMat3::from_cols(DVec3::Y.cross(out), DVec3::Y, out));
                b.surface_pair(
                    &format!("{}_left", name("mount")),
                    facet,
                    centre,
                    rotation,
                    &name(&format!("facet_{}_{}", ring + 1, k + 1)),
                );
            }
        }
    }

    // Engines: on the outer face of each rear quarter's rearmost facets, nozzles straight back;
    // the upper pair as far above the centre of mass as the lower pair below it, so the forward
    // thrust line passes through it.
    let com_y = VehicleBody::of(&b.v).unwrap().mass.center_of_mass.y;
    let nozzle_back = DQuat::from_rotation_x(-std::f64::consts::FRAC_PI_2);
    for (ring, (_, _, _)) in RINGS.iter().enumerate() {
        let facet = b.index(&format!("back_facet_{}_1_left", ring + 1));
        let (centre, rotation) = (b.v.parts[facet].position, b.v.parts[facet].rotation);
        let out = rotation * DVec3::Z;
        let y = if ring == 0 {
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
                &format!("back_facet_{}_1_left", ring + 1),
                "thruster.main.medium",
                at,
                nozzle_back,
                &format!("engine_{}_{}", ring + 1, n + 1),
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

/// One quarter-shell arm: its open joint's group, the group its facets move with, and the
/// facets.
struct ShellArm {
    open: usize,
    group: usize,
    facets: Vec<usize>,
}

fn shell_arm(v: &Vehicle, a: &Articulation, quarter: &str, side: &str) -> ShellArm {
    let part = |name: String| {
        v.parts
            .iter()
            .position(|p| p.name == name)
            .unwrap_or_else(|| panic!("no part {name}"))
    };
    let joint = |j: &str| {
        a.group_of_joint(part(format!("{quarter}_{j}_{side}")))
            .unwrap()
    };
    let facets: Vec<usize> = [(1, 1), (1, 2), (2, 1), (2, 2)]
        .iter()
        .map(|(r, k)| part(format!("{quarter}_facet_{r}_{k}_{side}")))
        .collect();
    ShellArm {
        open: joint("open"),
        group: a.part_group[facets[0]],
        facets,
    }
}

/// Where a quarter is for joint `angles`: the middle of its facets and its outward direction
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

/// The open angle (degrees, the sign that swings the quarter outward) of `magnitude`.
fn opened(v: &Vehicle, a: &Articulation, arm: &ShellArm, magnitude: f64) -> f64 {
    let n = a.groups.len();
    let reach = |deg: f64| {
        let mut angles = vec![0.0; n];
        angles[arm.open] = deg.to_radians();
        let (c, _) = shell_at(v, a, arm, &angles);
        DVec3::new(c.x, 0.0, c.z - SHELL_AXIS_Z).length()
    };
    if reach(magnitude) >= reach(-magnitude) {
        magnitude
    } else {
        -magnitude
    }
}

/// How far each pose opens the quarters (degrees).
const STAND_FRONT_OPEN: f64 = 70.0;
const BLOOM_OPEN: f64 = 75.0;

/// The Bastion's poses: cocoon (closed), stand (front quarters open, back closed: the front
/// free for work, the back armoured) and bloom (all four open like petals).
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
    let mut stand = vec![0.0; n];
    let mut bloom = vec![0.0; n];
    for side in ["left", "right"] {
        for q in &QUARTERS {
            let arm = shell_arm(v, a, q.name, side);
            if q.name == "front" {
                stand[arm.open] = opened(v, a, &arm, STAND_FRONT_OPEN).to_radians();
            }
            bloom[arm.open] = opened(v, a, &arm, BLOOM_OPEN).to_radians();
        }
    }
    vec![
        named("bloom", &bloom),
        named("cocoon", &vec![0.0; n]),
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
    // Each arm's mount ends inside its quarter, near the plates.
    for q in &QUARTERS {
        let mount = &v.parts[v
            .parts
            .iter()
            .position(|p| p.name == format!("{}_mount_left", q.name))
            .unwrap()];
        let face = mount.position + mount.rotation * DVec3::new(0.0, -0.2, 0.0);
        let r = DVec3::new(face.x, 0.0, face.z - SHELL_AXIS_Z).length();
        let inside = RINGS[0].0 * (FACET_DEG.to_radians() / 2.0).cos() - 0.25;
        println!(
            "{} mount at {:.2?}, r {r:.2} m; the quarter's inside is at {inside:.2} m",
            q.name, face
        );
        assert!(
            r < inside && r > inside - 1.0,
            "{}: mount at r {r:.2} m, shell at {inside:.2} m",
            q.name
        );
    }
}

#[test]
fn closed_the_quarters_ring_the_body_into_a_cocoon() {
    let v = assemble();
    let body = VehicleBody::of(&v).unwrap();
    let a = &body.articulation;
    let rest = vec![0.0; a.groups.len()];
    // Every other part is inside the rings at its height (the shins and feet walk out below).
    for p in &v.parts {
        if p.name.contains("facet") || p.name.contains("engine") {
            continue;
        }
        let r = DVec3::new(p.position.x, 0.0, p.position.z - SHELL_AXIS_Z).length();
        let limit = if p.position.y > -4.0 {
            RINGS[0].0
        } else if p.position.y > -12.0 {
            RINGS[1].0
        } else {
            continue;
        };
        assert!(
            r < limit - 0.25,
            "{} at r {r:.2} m pokes out of the cocoon ({limit} m)",
            p.name
        );
    }
    // Seen from above, the four quarters face the four diagonals.
    for (q, side, want) in [
        ("front", "left", DVec3::new(-1.0, 0.0, -1.0)),
        ("back", "left", DVec3::new(-1.0, 0.0, 1.0)),
        ("front", "right", DVec3::new(1.0, 0.0, -1.0)),
        ("back", "right", DVec3::new(1.0, 0.0, 1.0)),
    ] {
        let (_, out) = shell_at(&v, a, &shell_arm(&v, a, q, side), &rest);
        assert!(out.dot(want.normalize()) > 0.99, "{q} {side} faces {out:?}");
    }
}

#[test]
fn the_quarters_open_like_petals() {
    let v = design();
    let body = VehicleBody::of(&v).unwrap();
    let a = &body.articulation;
    let rest = vec![0.0; a.groups.len()];
    let (stand, bloom) = (pose(&v, a, "stand"), pose(&v, a, "bloom"));
    let radius = |c: DVec3| DVec3::new(c.x, 0.0, c.z - SHELL_AXIS_Z).length();
    for side in ["left", "right"] {
        for q in ["front", "back"] {
            let arm = shell_arm(&v, a, q, side);
            let (closed, _) = shell_at(&v, a, &arm, &rest);
            let (open, out) = shell_at(&v, a, &arm, &bloom);
            println!("bloom {q} {side}: middle {open:.1?} (closed {closed:.1?}), facing {out:.2?}");
            assert!(
                radius(open) > radius(closed) + 2.0,
                "bloom {q} {side} opens out"
            );
            assert!(
                out.y > 0.3,
                "bloom {q} {side}: the outer face turns up and out like a petal's"
            );
            let (s, _) = shell_at(&v, a, &arm, &stand);
            if q == "front" {
                assert!(radius(s) > radius(closed) + 2.0, "stand: front {side} open");
            } else {
                assert!(s.distance(closed) < 1e-6, "stand: back {side} closed");
            }
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
