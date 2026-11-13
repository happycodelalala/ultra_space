//! Editing a vehicle's parts (Batch 3, task 3.7; design: vehicle-builder.md §5).
//!
//! Engine-independent operations on a vehicle instance's part list, used by the builder UI:
//! free attach nodes, attaching a part node to node or on a surface, removing a part with
//! everything attached below it. Every operation keeps the attachment tree valid (parents come
//! before their children; joined nodes meet and face each other).

use std::sync::Arc;

use glam::{DQuat, DVec3};

use crate::catalog::{PartDef, SizeClass};
use crate::design::{Attach, VehiclePart, check_join};
use crate::vehicle::Vehicle;

/// An attach node on a placed part, in vehicle axes.
#[derive(Clone, Debug, PartialEq)]
pub struct FreeNode {
    /// Index of the part it belongs to.
    pub part: usize,
    pub node: String,
    pub position: DVec3,
    /// Outward direction.
    pub direction: DVec3,
    pub size: SizeClass,
}

/// Nodes of the vehicle's parts that nothing is attached to yet.
pub fn free_nodes(vehicle: &Vehicle) -> Vec<FreeNode> {
    let mut used: Vec<(usize, &str)> = Vec::new();
    for (i, p) in vehicle.parts.iter().enumerate() {
        if let (Some(parent), Attach::Node { node, to }) = (p.parent, &p.attach) {
            used.push((i, node.as_str()));
            used.push((parent, to.as_str()));
        }
    }
    let mut free = Vec::new();
    for (i, p) in vehicle.parts.iter().enumerate() {
        for n in &p.def.attach_nodes {
            if used.contains(&(i, n.id.as_str())) {
                continue;
            }
            free.push(FreeNode {
                part: i,
                node: n.id.clone(),
                position: p.position + p.rotation * n.position(),
                direction: (p.rotation * n.direction()).normalize(),
                size: n.size,
            });
        }
    }
    free
}

/// Where a new part goes when its node `own` joins the free node `at`, turned `roll` radians
/// about the joint's axis: position and rotation in vehicle axes, or `None` if the part has no
/// such node or the sizes differ.
pub fn placement_at_node(
    def: &PartDef,
    own: &str,
    at: &FreeNode,
    roll: f64,
) -> Option<(DVec3, DQuat)> {
    let node = def.node(own)?;
    if node.size != at.size {
        return None;
    }
    // Turn the part so its node faces back into the parent's node, then roll about that axis.
    let align = DQuat::from_rotation_arc(node.direction(), -at.direction);
    let rotation = (DQuat::from_axis_angle(at.direction, roll) * align).normalize();
    let position = at.position - rotation * node.position();
    Some((position, rotation))
}

/// The nodes of `def` that can join `at` (matching size), in catalog order.
pub fn fitting_nodes<'a>(def: &'a PartDef, at: &FreeNode) -> Vec<&'a str> {
    def.attach_nodes
        .iter()
        .filter(|n| n.size == at.size)
        .map(|n| n.id.as_str())
        .collect()
}

/// A name for a new part of type `def` that no part of the vehicle has yet.
fn new_name(vehicle: &Vehicle, def: &PartDef) -> String {
    let base: String = def
        .id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    (1..)
        .map(|n| format!("{base}_{n}"))
        .find(|name| vehicle.parts.iter().all(|p| &p.name != name))
        .expect("an unused name")
}

/// Attaches a new part by its node `own` to the free node `at` (turned `roll` radians about the
/// joint). Returns the new part's index.
pub fn attach_at_node(
    vehicle: &mut Vehicle,
    def: &Arc<PartDef>,
    own: &str,
    at: &FreeNode,
    roll: f64,
) -> Result<usize, String> {
    let (position, rotation) = placement_at_node(def, own, at, roll)
        .ok_or_else(|| format!("{} has no {own:?} node of size {:?}", def.id, at.size))?;
    let part = VehiclePart {
        name: new_name(vehicle, def),
        def: Arc::clone(def),
        position,
        rotation,
        parent: Some(at.part),
        attach: Attach::Node {
            node: own.to_string(),
            to: at.node.clone(),
        },
        symmetry: None,
    };
    if let Some(problem) = check_join(&part, &vehicle.parts[at.part]) {
        return Err(problem);
    }
    vehicle.parts.push(part);
    Ok(vehicle.parts.len() - 1)
}

/// Attaches a new part to the surface of part `parent` at `position` (vehicle axes) with
/// `rotation`. Returns its index.
pub fn attach_to_surface(
    vehicle: &mut Vehicle,
    def: &Arc<PartDef>,
    parent: usize,
    position: DVec3,
    rotation: DQuat,
) -> Result<usize, String> {
    if !def.can_surface_attach() {
        return Err(format!("{} can't attach to surfaces", def.id));
    }
    if parent >= vehicle.parts.len() {
        return Err("no such parent part".into());
    }
    vehicle.parts.push(VehiclePart {
        name: new_name(vehicle, def),
        def: Arc::clone(def),
        position,
        rotation,
        parent: Some(parent),
        attach: Attach::Surface,
        symmetry: None,
    });
    Ok(vehicle.parts.len() - 1)
}

/// Removes part `index` and everything attached below it. The root can't be removed. Returns
/// how many parts were removed.
pub fn remove_part(vehicle: &mut Vehicle, index: usize) -> Result<usize, String> {
    if index == 0 {
        return Err("the root part can't be removed".into());
    }
    if index >= vehicle.parts.len() {
        return Err("no such part".into());
    }
    // Parents come before children, so one pass finds the whole subtree.
    let mut doomed = vec![false; vehicle.parts.len()];
    doomed[index] = true;
    for i in index + 1..vehicle.parts.len() {
        if let Some(p) = vehicle.parts[i].parent {
            doomed[i] = doomed[p];
        }
    }
    // New indices for the parts that stay.
    let mut new_index = vec![usize::MAX; vehicle.parts.len()];
    let mut next = 0;
    for (i, gone) in doomed.iter().enumerate() {
        if !gone {
            new_index[i] = next;
            next += 1;
        }
    }
    let removed = doomed.iter().filter(|d| **d).count();
    let parts = std::mem::take(&mut vehicle.parts);
    vehicle.parts = parts
        .into_iter()
        .enumerate()
        .filter(|(i, _)| !doomed[*i])
        .map(|(_, mut p)| {
            p.parent = p.parent.map(|q| new_index[q]);
            p
        })
        .collect();
    Ok(removed)
}

/// Parts below (attached directly or indirectly to) part `index`, including it.
pub fn subtree(vehicle: &Vehicle, index: usize) -> Vec<usize> {
    let mut inside = vec![false; vehicle.parts.len()];
    if index < inside.len() {
        inside[index] = true;
    }
    for i in index + 1..vehicle.parts.len() {
        if let Some(p) = vehicle.parts[i].parent {
            inside[i] = inside[p];
        }
    }
    (0..inside.len()).filter(|&i| inside[i]).collect()
}

/// Symmetry applied when placing parts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Symmetry {
    #[default]
    None,
    /// Mirrored left and right (across the vehicle's YZ plane).
    Mirror,
    /// `n` copies spaced evenly around the vehicle's long axis (Z).
    Radial(u8),
}

impl Symmetry {
    /// The next setting: none, mirror, radial ×2, ×3, ×4, ×6, ×8, none...
    pub fn next(self) -> Self {
        match self {
            Self::None => Self::Mirror,
            Self::Mirror => Self::Radial(2),
            Self::Radial(2) => Self::Radial(3),
            Self::Radial(3) => Self::Radial(4),
            Self::Radial(4) => Self::Radial(6),
            Self::Radial(6) => Self::Radial(8),
            Self::Radial(_) => Self::None,
        }
    }

    /// The transforms of the copies (besides the original): position and rotation maps.
    fn copies(self) -> Vec<(DQuat, bool)> {
        match self {
            Self::None => Vec::new(),
            Self::Mirror => vec![(DQuat::IDENTITY, true)],
            Self::Radial(n) => (1..n)
                .map(|k| {
                    let a = std::f64::consts::TAU * f64::from(k) / f64::from(n);
                    (DQuat::from_rotation_z(a), false)
                })
                .collect(),
        }
    }
}

/// Mirrors a pose across the vehicle's YZ plane (X → −X), as a proper rotation.
fn mirror_pose(position: DVec3, rotation: DQuat) -> (DVec3, DQuat) {
    // M R M with M = diag(−1, 1, 1): for a quaternion (x, y, z, w) that is (x, −y, −z, w).
    let q = DQuat::from_xyzw(rotation.x, -rotation.y, -rotation.z, rotation.w);
    (
        DVec3::new(-position.x, position.y, position.z),
        q.normalize(),
    )
}

/// The part at the copy's place of part `of` (itself if it maps onto itself), if any.
fn counterpart(vehicle: &Vehicle, of: usize, map: &dyn Fn(DVec3) -> DVec3) -> Option<usize> {
    let p = &vehicle.parts[of];
    let target = map(p.position);
    if target.distance(p.position) < 1e-3 {
        return Some(of);
    }
    vehicle
        .parts
        .iter()
        .position(|q| Arc::ptr_eq(&q.def, &p.def) && q.position.distance(target) < 1e-3)
}

/// Places symmetric copies of the just-placed part `index` (and names them into one symmetry
/// group with it). Each copy hangs on its parent's counterpart (or the parent itself when that
/// maps onto itself), by the same nodes when they meet, else on the surface. Returns how many
/// copies were placed.
pub fn place_symmetric(vehicle: &mut Vehicle, index: usize, symmetry: Symmetry) -> usize {
    let copies = symmetry.copies();
    if copies.is_empty() {
        return 0;
    }
    let original = vehicle.parts[index].clone();
    let Some(parent) = original.parent else {
        return 0;
    };
    let group = original
        .symmetry
        .clone()
        .unwrap_or_else(|| format!("{}_sym", original.name));
    vehicle.parts[index].symmetry = Some(group.clone());
    let mut placed = 0;
    for (turn, mirror) in copies {
        let map = |p: DVec3| {
            if mirror {
                DVec3::new(-p.x, p.y, p.z)
            } else {
                turn * p
            }
        };
        let (position, rotation) = if mirror {
            mirror_pose(original.position, original.rotation)
        } else {
            (
                turn * original.position,
                (turn * original.rotation).normalize(),
            )
        };
        if position.distance(original.position) < 1e-3 {
            continue; // on the axis or the plane: the copy would coincide
        }
        let Some(copy_parent) = counterpart(vehicle, parent, &map) else {
            continue;
        };
        let mut part = VehiclePart {
            name: new_name(vehicle, &original.def),
            def: Arc::clone(&original.def),
            position,
            rotation,
            parent: Some(copy_parent),
            attach: original.attach.clone(),
            symmetry: Some(group.clone()),
        };
        if check_join(&part, &vehicle.parts[copy_parent]).is_some() {
            // Mirroring swaps nodes (left ↔ right): find the pair that meets now.
            let parent_part = &vehicle.parts[copy_parent];
            let joined = matches!(original.attach, Attach::Node { .. })
                .then(|| {
                    part.def.attach_nodes.iter().find_map(|own| {
                        parent_part.def.attach_nodes.iter().find_map(|theirs| {
                            let attach = Attach::Node {
                                node: own.id.clone(),
                                to: theirs.id.clone(),
                            };
                            let candidate = VehiclePart {
                                attach: attach.clone(),
                                ..part.clone()
                            };
                            check_join(&candidate, parent_part)
                                .is_none()
                                .then_some(attach)
                        })
                    })
                })
                .flatten();
            match joined {
                Some(attach) => part.attach = attach,
                None if part.def.can_surface_attach() => part.attach = Attach::Surface,
                None => continue,
            }
        }
        vehicle.parts.push(part);
        placed += 1;
    }
    placed
}

/// Removes part `index`, the other members of its symmetry group, and everything below them.
/// Returns how many parts were removed.
pub fn remove_with_symmetry(vehicle: &mut Vehicle, index: usize) -> Result<usize, String> {
    let group = vehicle
        .parts
        .get(index)
        .ok_or("no such part")?
        .symmetry
        .clone();
    let names: Vec<String> = match &group {
        Some(g) => vehicle
            .parts
            .iter()
            .filter(|p| p.symmetry.as_ref() == Some(g))
            .map(|p| p.name.clone())
            .collect(),
        None => vec![vehicle.parts[index].name.clone()],
    };
    let mut removed = 0;
    for name in names {
        if let Some(i) = vehicle.parts.iter().position(|p| p.name == name) {
            removed += remove_part(vehicle, i)?;
        }
    }
    Ok(removed)
}

/// A saved group of parts: a part and everything attached below it, in the root's axes (the
/// root at the origin, unrotated). Placed like a part, through the root's nodes.
#[derive(Clone, Debug)]
pub struct Section {
    pub name: String,
    pub parts: Vec<VehiclePart>,
}

impl Section {
    /// The part `index` of `vehicle` and everything below it.
    pub fn of(vehicle: &Vehicle, index: usize, name: impl Into<String>) -> Option<Self> {
        let root = vehicle.parts.get(index)?;
        let inverse = root.rotation.inverse();
        let members = subtree(vehicle, index);
        let mut parts: Vec<VehiclePart> = members
            .iter()
            .map(|&i| {
                let p = &vehicle.parts[i];
                VehiclePart {
                    position: inverse * (p.position - root.position),
                    rotation: (inverse * p.rotation).normalize(),
                    parent: p.parent.and_then(|q| members.iter().position(|&m| m == q)),
                    ..p.clone()
                }
            })
            .collect();
        parts[0].attach = Attach::Root;
        parts[0].parent = None;
        Some(Self {
            name: name.into(),
            parts,
        })
    }

    pub fn root(&self) -> &Arc<PartDef> {
        &self.parts[0].def
    }
}

/// Places a section with its root at `position`/`rotation` (vehicle axes), the root attached to
/// `parent` the way `attach` says. Returns the root's index.
pub fn place_section(
    vehicle: &mut Vehicle,
    section: &Section,
    parent: usize,
    attach: Attach,
    position: DVec3,
    rotation: DQuat,
) -> Result<usize, String> {
    let base = vehicle.parts.len();
    for (k, p) in section.parts.iter().enumerate() {
        let mut part = VehiclePart {
            name: new_name(vehicle, &p.def),
            position: position + rotation * p.position,
            rotation: (rotation * p.rotation).normalize(),
            ..p.clone()
        };
        if k == 0 {
            part.parent = Some(parent);
            part.attach = attach.clone();
            if let Some(problem) = check_join(&part, &vehicle.parts[parent]) {
                vehicle.parts.truncate(base);
                return Err(problem);
            }
        } else {
            part.parent = p.parent.map(|q| base + q);
        }
        part.symmetry = None;
        vehicle.parts.push(part);
    }
    Ok(base)
}

/// What the builder shows about a vehicle (design: vehicle-builder.md §5.3).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BuildStats {
    pub parts: usize,
    pub mass_kg: f64,
    pub center_of_mass: DVec3,
    /// Size of the box around the parts (m).
    pub size: DVec3,
    /// Forward thrust over weight on the current body.
    pub thrust_to_weight: f64,
    /// Upward (vehicle +Y) and sideways acceleration (m/s²) and the weakest turn (rad/s²).
    pub up_acceleration: f64,
    pub side_acceleration: f64,
    pub turn_acceleration: f64,
    pub can_hover: bool,
    pub fuel_kg: f64,
    pub battery_wh: f64,
    pub wheels: usize,
    /// Centre of mass height over half the narrowest wheel track (above 1 tips easily).
    pub tipping: Option<f64>,
    /// Problems that stop the vehicle working.
    pub problems: Vec<String>,
}

/// Stats for `vehicle` on a body with surface gravity `g` (m/s²).
pub fn stats(vehicle: &Vehicle, g: f64) -> BuildStats {
    let Some(body) = crate::vehicle::VehicleBody::of(vehicle) else {
        return BuildStats {
            problems: vec!["no parts".into()],
            ..Default::default()
        };
    };
    let perf = &body.performance;
    let (lo, hi) = vehicle.parts.iter().fold(
        (DVec3::splat(f64::MAX), DVec3::splat(f64::MIN)),
        |(lo, hi), p| (lo.min(p.position), hi.max(p.position)),
    );
    let com = body.mass.center_of_mass;
    let wheels = &body.wheels;
    let tipping = (wheels.len() >= 3).then(|| {
        let half_track = wheels
            .iter()
            .map(|w| (w.position.x - com.x).abs())
            .fold(f64::MAX, f64::min)
            .max(0.1);
        let lowest = wheels
            .iter()
            .map(|w| w.position.y - w.def.radius_m)
            .fold(f64::MAX, f64::min);
        (com.y - lowest) / half_track
    });
    let mut problems = Vec::new();
    if !vehicle.parts.iter().any(|p| p.def.is_control()) {
        problems.push("no control source (a cockpit)".into());
    }
    if perf.forward_acceleration() <= 0.0 && wheels.is_empty() && perf.max_acceleration[2] <= 0.0 {
        problems.push("no way to move (no engines or wheels)".into());
    }
    // Engines pushing one way must push through the centre of mass, or full thrust turns the
    // vehicle faster than it can turn back.
    let inertia = body.mass.inertia;
    let alpha = perf.max_angular_acceleration;
    let counter = (alpha.x * inertia.col(0).x)
        .min(alpha.y * inertia.col(1).y)
        .min(alpha.z * inertia.col(2).z);
    for (axis, name) in [(DVec3::NEG_Z, "forward"), (DVec3::Y, "up")] {
        let torque: DVec3 = body
            .allocator
            .thrusters()
            .iter()
            .filter(|t| t.direction.dot(axis) > 0.9)
            .map(|t| t.position.cross(t.direction * t.max_thrust))
            .sum();
        if torque.length() > counter.max(0.0) * 1.2 + 1.0 {
            problems.push(format!(
                "{name} engines push {:.0} kN m off the centre of mass (it can counter {:.0})",
                torque.length() / 1000.0,
                counter / 1000.0
            ));
        }
    }
    BuildStats {
        parts: vehicle.parts.len(),
        mass_kg: body.mass.mass,
        center_of_mass: com,
        size: hi - lo,
        thrust_to_weight: perf.forward_acceleration() / g.max(1e-9),
        up_acceleration: perf.max_acceleration[2],
        side_acceleration: perf.max_acceleration[0].min(perf.max_acceleration[1]),
        turn_acceleration: perf.max_angular_acceleration.min_element(),
        can_hover: perf.max_acceleration[2] > g,
        fuel_kg: vehicle
            .parts
            .iter()
            .filter_map(|p| p.def.tank)
            .map(|t| t.capacity_kg)
            .fold(0.0, |a, b| a + b),
        battery_wh: vehicle
            .parts
            .iter()
            .filter_map(|p| p.def.battery)
            .map(|b| b.capacity_wh)
            .fold(0.0, |a, b| a + b),
        wheels: wheels.len(),
        tipping,
        problems,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::catalog::PartCatalog;
    use crate::design::VehicleDesign;
    use crate::vehicle::VehicleBody;

    fn content() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../../content/data")
    }

    fn catalog() -> PartCatalog {
        PartCatalog::load_dir(content().join("parts")).unwrap()
    }

    fn kestrel() -> Vehicle {
        let design =
            VehicleDesign::from_path(content().join("vehicles/kestrel.json"), &catalog()).unwrap();
        Vehicle::from_design(&design)
    }

    /// A vehicle of just one part.
    fn lone(id: &str) -> Vehicle {
        let def = catalog().get(id).unwrap().clone();
        Vehicle {
            design_id: "test".into(),
            name: "Test".into(),
            parts: vec![VehiclePart {
                name: "root".into(),
                def,
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    #[test]
    fn the_kestrel_has_no_free_nodes_but_the_cockpit_front() {
        // Every node is used except where nothing is attached: the hull's free ends are taken,
        // the main engine's top is joined, the tank's nodes are free (it sits inside the hull).
        let free = free_nodes(&kestrel());
        assert!(
            free.iter().all(|n| n.node != "rear" || n.part != 0),
            "cockpit rear is used"
        );
        assert!(
            free.iter().any(|n| n.node == "top" || n.node == "bottom"),
            "{free:?}"
        );
    }

    #[test]
    fn attaching_at_a_node_makes_the_nodes_meet() {
        let mut v = lone("cockpit.small");
        let hull = catalog().get("hull.section.medium").unwrap().clone();
        let at = free_nodes(&v)
            .into_iter()
            .find(|n| n.node == "rear")
            .unwrap();
        let i = attach_at_node(&mut v, &hull, "front", &at, 0.0).unwrap();
        // The hull's front node sits on the cockpit's rear node, facing it.
        let (p, d) = v.parts[i].node_in_vehicle("front").unwrap();
        assert!(p.distance(at.position) < 1e-9);
        assert!(d.dot(at.direction) < -0.999);
        // The new rear end is free, the joined nodes aren't.
        let free = free_nodes(&v);
        assert!(free.iter().any(|n| n.part == i && n.node == "rear"));
        assert!(!free.iter().any(|n| n.part == 0 && n.node == "rear"));
        // It builds: mass grows, the tree is valid.
        assert!(VehicleBody::of(&v).unwrap().mass.mass > 4_000.0);
        assert!(check_join(&v.parts[i], &v.parts[0]).is_none());
    }

    #[test]
    fn rolling_turns_the_part_about_the_joint() {
        let v = lone("cockpit.small");
        let hull = catalog().get("hull.section.medium").unwrap().clone();
        let at = free_nodes(&v)
            .into_iter()
            .find(|n| n.node == "rear")
            .unwrap();
        let (_, r0) = placement_at_node(&hull, "front", &at, 0.0).unwrap();
        let (p1, r1) = placement_at_node(&hull, "front", &at, std::f64::consts::FRAC_PI_2).unwrap();
        // Up turns 90° about the joint (the vehicle's Z axis), the joint stays put.
        assert!(((r0 * DVec3::Y).dot(r1 * DVec3::Y)).abs() < 1e-9);
        let front = p1 + r1 * hull.node("front").unwrap().position();
        assert!(front.distance(at.position) < 1e-9);
    }

    #[test]
    fn removing_a_part_takes_what_hangs_below_it() {
        let mut v = kestrel();
        let n = v.parts.len();
        // hull_rear (index 2) carries the tank, the cruise drive, the main engine, the rear
        // thrusters and two legs.
        let below = subtree(&v, 2);
        assert!(below.len() > 5, "{below:?}");
        let removed = remove_part(&mut v, 2).unwrap();
        assert_eq!(removed, below.len());
        assert_eq!(v.parts.len(), n - removed);
        // Still a valid tree.
        for (i, p) in v.parts.iter().enumerate().skip(1) {
            let parent = p.parent.unwrap();
            assert!(parent < i);
            assert!(check_join(p, &v.parts[parent]).is_none(), "{}", p.name);
        }
        assert!(remove_part(&mut v, 0).is_err());
    }

    #[test]
    fn surface_attachment_and_names() {
        let mut v = lone("hull.section.medium");
        let leg = catalog().get("landing_leg.small").unwrap().clone();
        let a = attach_to_surface(&mut v, &leg, 0, DVec3::new(1.0, -1.5, 0.0), DQuat::IDENTITY)
            .unwrap();
        let b = attach_to_surface(
            &mut v,
            &leg,
            0,
            DVec3::new(-1.0, -1.5, 0.0),
            DQuat::IDENTITY,
        )
        .unwrap();
        assert_ne!(v.parts[a].name, v.parts[b].name);
        assert_eq!(v.parts[a].attach, Attach::Surface);
    }

    #[test]
    fn mirror_symmetry_places_the_matching_part() {
        let mut v = lone("hull.section.medium");
        let leg = catalog().get("landing_leg.small").unwrap().clone();
        let i = attach_to_surface(&mut v, &leg, 0, DVec3::new(1.3, -1.5, 1.0), DQuat::IDENTITY)
            .unwrap();
        assert_eq!(place_symmetric(&mut v, i, Symmetry::Mirror), 1);
        let copy = v.parts.last().unwrap();
        assert!(copy.position.distance(DVec3::new(-1.3, -1.5, 1.0)) < 1e-9);
        assert_eq!(copy.symmetry, v.parts[i].symmetry);
        // Removing one removes the group.
        assert_eq!(remove_with_symmetry(&mut v, i).unwrap(), 2);
        assert_eq!(v.parts.len(), 1);
    }

    #[test]
    fn radial_symmetry_spaces_copies_around_the_axis() {
        let mut v = lone("hull.section.medium");
        let thruster = catalog().get("thruster.rcs.small").unwrap().clone();
        let i = attach_to_surface(
            &mut v,
            &thruster,
            0,
            DVec3::new(1.6, 0.0, 0.0),
            DQuat::IDENTITY,
        )
        .unwrap();
        assert_eq!(place_symmetric(&mut v, i, Symmetry::Radial(4)), 3);
        let mut xs: Vec<(i64, i64)> = v.parts[1..]
            .iter()
            .map(|p| {
                (
                    (p.position.x * 10.0).round() as i64,
                    (p.position.y * 10.0).round() as i64,
                )
            })
            .collect();
        xs.sort();
        assert_eq!(xs, vec![(-16, 0), (0, -16), (0, 16), (16, 0)]);
        // Balanced: the centre of mass stays on the axis.
        let com = crate::vehicle::VehicleBody::of(&v)
            .unwrap()
            .mass
            .center_of_mass;
        assert!(com.x.abs() < 1e-9 && com.y.abs() < 1e-9);
    }

    #[test]
    fn mirrored_node_attachments_meet_their_nodes() {
        let mut v = lone("hull.section.medium");
        let hull = catalog().get("hull.section.medium").unwrap().clone();
        let at = free_nodes(&v)
            .into_iter()
            .find(|n| n.node == "right")
            .unwrap();
        let i = attach_at_node(&mut v, &hull, "left", &at, 0.0).unwrap();
        assert_eq!(place_symmetric(&mut v, i, Symmetry::Mirror), 1);
        let copy = v.parts.last().unwrap();
        assert!(
            matches!(copy.attach, Attach::Node { .. }),
            "{:?}",
            copy.attach
        );
        assert!(check_join(copy, &v.parts[0]).is_none());
    }

    #[test]
    fn sections_save_and_place_a_subtree() {
        let k = kestrel();
        // The rear hull with everything below it.
        let section = Section::of(&k, 2, "engine block").unwrap();
        assert_eq!(section.parts.len(), subtree(&k, 2).len());
        assert_eq!(section.parts[0].position, DVec3::ZERO);
        // Placed at the end of a lone cockpit, it joins and keeps its shape.
        let mut v = lone("cockpit.small");
        let at = free_nodes(&v)
            .into_iter()
            .find(|n| n.node == "rear")
            .unwrap();
        let (p, r) = placement_at_node(section.root(), "front", &at, 0.0).unwrap();
        let attach = Attach::Node {
            node: "front".into(),
            to: "rear".into(),
        };
        place_section(&mut v, &section, 0, attach, p, r).unwrap();
        assert_eq!(v.parts.len(), 1 + section.parts.len());
        for (i, part) in v.parts.iter().enumerate().skip(1) {
            let parent = part.parent.unwrap();
            assert!(parent < i);
            assert!(
                check_join(part, &v.parts[parent]).is_none(),
                "{}",
                part.name
            );
        }
    }

    #[test]
    fn stats_describe_what_the_vehicle_can_do() {
        let s = stats(&kestrel(), 9.81);
        assert!(
            (s.thrust_to_weight - 3.07).abs() < 0.05,
            "{}",
            s.thrust_to_weight
        );
        assert!(
            !s.can_hover,
            "the Kestrel can't hover on its belly thrusters"
        );
        assert!(s.fuel_kg > 0.0);
        assert!(s.problems.is_empty(), "{:?}", s.problems);
        let rover =
            VehicleDesign::from_path(content().join("vehicles/rover.json"), &catalog()).unwrap();
        let r = stats(&Vehicle::from_design(&rover), 9.81);
        assert_eq!(r.wheels, 4);
        assert!(
            r.tipping.unwrap() < 1.0,
            "the rover is stable: {:?}",
            r.tipping
        );
        let lone_tank = stats(&lone("tank.medium"), 9.81);
        assert_eq!(lone_tank.problems.len(), 2, "{:?}", lone_tank.problems);
    }
}
