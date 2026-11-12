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
}
