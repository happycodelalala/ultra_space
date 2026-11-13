//! Jointed parts (Batch 3, task 3.10; design: vehicle-builder.md §7.1).
//!
//! A joint part (a hinge or rotor) splits a vehicle into **rigid groups**: the parts attached
//! beyond a joint's `end` node (and everything below them) turn about the joint's axis through
//! that node. Articulation is kinematic: each joint has an angle that moves toward a target at the
//! joint's speed, and each group's pose follows from the angles by forward kinematics down the
//! attachment tree. The vehicle stays one rigid body; its groups' colliders and meshes move.

use bevy_ecs::prelude::*;
use glam::{DQuat, DVec3};

use crate::catalog::JointDef;
use crate::design::{Attach, VehiclePart};

/// A rigid group: the parts that move together.
#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    /// The group it hangs from (`None` for the root group).
    pub parent: Option<usize>,
    /// The joint part it turns on (`None` for the root group).
    pub joint_part: Option<usize>,
    pub joint: Option<JointDef>,
    /// Pivot and axis of the joint in the vehicle's rest frame.
    pub pivot: DVec3,
    pub axis: DVec3,
}

/// How a vehicle splits into rigid groups.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Articulation {
    pub groups: Vec<Group>,
    /// The group of each part.
    pub part_group: Vec<usize>,
}

impl Articulation {
    /// Splits `parts` at their joints. Parents come before children, so one pass suffices.
    pub fn of(parts: &[VehiclePart]) -> Self {
        let mut groups = vec![Group {
            parent: None,
            joint_part: None,
            joint: None,
            pivot: DVec3::ZERO,
            axis: DVec3::X,
        }];
        let mut part_group = Vec::with_capacity(parts.len());
        for p in parts {
            // A parent not before this part (a hand-edited list) counts as the root group.
            let parent = p.parent.filter(|&q| q < part_group.len());
            let group = match (parent, &p.attach) {
                (Some(parent), Attach::Node { to, .. })
                    if to == "end" && parts[parent].def.joint.is_some() =>
                {
                    let joint_part = &parts[parent];
                    let def = joint_part.def.joint.expect("checked");
                    let (pivot, _) = joint_part.node_in_vehicle("end").unwrap_or_default();
                    groups.push(Group {
                        parent: Some(part_group[parent]),
                        joint_part: Some(parent),
                        joint: Some(def),
                        pivot,
                        axis: (joint_part.rotation * def.axis()).normalize(),
                    });
                    groups.len() - 1
                }
                (Some(parent), _) => part_group[parent],
                (None, _) => 0,
            };
            part_group.push(group);
        }
        Self { groups, part_group }
    }

    /// Whether the vehicle has any joints.
    pub fn is_jointed(&self) -> bool {
        self.groups.len() > 1
    }

    /// The group turned by joint part `part`, if it is one.
    pub fn group_of_joint(&self, part: usize) -> Option<usize> {
        self.groups.iter().position(|g| g.joint_part == Some(part))
    }

    /// Each group's pose (rotation, translation) relative to the vehicle's rest frame for joint
    /// `angles` (radians, one per group; the root's is ignored): a rest-frame point `x` of group
    /// `g` is at `rotation * x + translation`.
    pub fn poses(&self, angles: &[f64]) -> Vec<Pose> {
        let mut poses: Vec<Pose> = Vec::with_capacity(self.groups.len());
        for (g, group) in self.groups.iter().enumerate() {
            let pose = match group.parent {
                None => Pose::IDENTITY,
                Some(parent) => {
                    let angle = angles.get(g).copied().unwrap_or(0.0);
                    let turn = DQuat::from_axis_angle(group.axis, angle);
                    let local = Pose {
                        rotation: turn,
                        translation: group.pivot - turn * group.pivot,
                    };
                    poses[parent].then(local)
                }
            };
            poses.push(pose);
        }
        poses
    }
}

/// A rigid transform: `rotation * x + translation`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub rotation: DQuat,
    pub translation: DVec3,
}

impl Pose {
    pub const IDENTITY: Self = Self {
        rotation: DQuat::IDENTITY,
        translation: DVec3::ZERO,
    };

    pub fn apply(&self, x: DVec3) -> DVec3 {
        self.rotation * x + self.translation
    }

    /// This transform after `inner` (`self.apply(inner.apply(x))`).
    pub fn then(&self, inner: Pose) -> Pose {
        Pose {
            rotation: (self.rotation * inner.rotation).normalize(),
            translation: self.rotation * inner.translation + self.translation,
        }
    }
}

/// A vehicle's joint angles and their targets (radians, one per rigid group; the root's unused),
/// and the group poses they give.
#[derive(Component, Clone, Debug, Default, PartialEq)]
pub struct JointState {
    pub angles: Vec<f64>,
    pub targets: Vec<f64>,
    pub poses: Vec<Pose>,
    /// Bumped whenever the poses change (allocator and meshes follow).
    pub generation: u64,
}

impl JointState {
    /// All joints at rest.
    pub fn rest(articulation: &Articulation) -> Self {
        let n = articulation.groups.len();
        Self {
            angles: vec![0.0; n],
            targets: vec![0.0; n],
            poses: vec![Pose::IDENTITY; n],
            generation: 0,
        }
    }

    /// Moves every angle toward its target at its joint's speed for `dt` seconds. Returns
    /// whether anything moved (poses updated).
    pub fn advance(&mut self, articulation: &Articulation, dt: f64) -> bool {
        if self.angles.len() != articulation.groups.len() {
            *self = Self::rest(articulation);
        }
        let mut moved = false;
        for (g, group) in articulation.groups.iter().enumerate() {
            let Some(joint) = group.joint else { continue };
            let target = joint.clamp(self.targets[g]);
            let step = joint.speed_deg_s.to_radians() * dt;
            let delta = (target - self.angles[g]).clamp(-step, step);
            if delta != 0.0 {
                self.angles[g] += delta;
                moved = true;
            }
        }
        if moved || self.poses.len() != articulation.groups.len() {
            self.poses = articulation.poses(&self.angles);
            self.generation += 1;
        }
        moved
    }
}

/// Moves every jointed vehicle's joints toward their targets for one tick (and gives jointed
/// vehicles their joint state); when they move (or walking, which sets them directly, ends), the
/// thrust allocator follows the new poses.
pub fn joint_system(
    mut commands: Commands,
    clock: Res<sim_core::objects::SimClock>,
    mut stopped_walking: RemovedComponents<crate::gait::Walking>,
    mut vehicles: Query<(
        Entity,
        &mut crate::vehicle::VehicleBody,
        Option<&mut JointState>,
    )>,
) {
    let stopped: std::collections::HashSet<Entity> = stopped_walking.read().collect();
    for (entity, mut body, state) in &mut vehicles {
        if !body.articulation.is_jointed() {
            continue;
        }
        let Some(mut state) = state else {
            let mut rest = JointState::rest(&body.articulation);
            rest.advance(&body.articulation, 0.0);
            commands.entity(entity).insert(rest);
            continue;
        };
        if state.advance(&body.articulation, clock.dt) || stopped.contains(&entity) {
            body.allocator = body.posed_allocator(&state.poses);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::catalog::{AttachNode, JointType, PartDef, PartKind, Shape, SizeClass};

    fn node(id: &str, y: f64, dir: f64) -> AttachNode {
        AttachNode {
            id: id.into(),
            position_m: [0.0, y, 0.0],
            direction: [0.0, dir, 0.0],
            size: SizeClass::Large,
        }
    }

    /// A post (root), a hinge under it, and an arm hanging from the hinge.
    fn arm() -> Vec<VehiclePart> {
        let post = Arc::new(PartDef {
            id: "t.post".into(),
            mass_kg: 100.0,
            shape: Shape::Box {
                size_m: [1.0, 1.0, 1.0],
            },
            attach_nodes: vec![node("down", -0.5, -1.0)],
            ..Default::default()
        });
        let hinge = Arc::new(PartDef {
            id: "t.hinge".into(),
            kind: PartKind::Joint,
            mass_kg: 10.0,
            attach_nodes: vec![node("base", 0.25, 1.0), node("end", -0.25, -1.0)],
            joint: Some(JointDef {
                joint_type: JointType::Hinge,
                axis: [1.0, 0.0, 0.0],
                limits_deg: Some([-90.0, 90.0]),
                speed_deg_s: 90.0,
                max_torque_nm: 1000.0,
            }),
            ..Default::default()
        });
        let limb = Arc::new(PartDef {
            id: "t.limb".into(),
            mass_kg: 50.0,
            attach_nodes: vec![node("top", 1.0, 1.0)],
            ..Default::default()
        });
        vec![
            VehiclePart {
                name: "post".into(),
                def: post,
                ..Default::default()
            },
            VehiclePart {
                name: "hinge".into(),
                def: hinge,
                position: DVec3::new(0.0, -0.75, 0.0),
                parent: Some(0),
                attach: Attach::Node {
                    node: "base".into(),
                    to: "down".into(),
                },
                ..Default::default()
            },
            VehiclePart {
                name: "arm".into(),
                def: limb,
                position: DVec3::new(0.0, -2.0, 0.0),
                parent: Some(1),
                attach: Attach::Node {
                    node: "top".into(),
                    to: "end".into(),
                },
                ..Default::default()
            },
        ]
    }

    #[test]
    fn joints_split_parts_into_groups() {
        let a = Articulation::of(&arm());
        assert_eq!(a.groups.len(), 2);
        // The hinge itself stays with the post; the arm turns.
        assert_eq!(a.part_group, vec![0, 0, 1]);
        assert_eq!(a.group_of_joint(1), Some(1));
        assert!((a.groups[1].pivot - DVec3::new(0.0, -1.0, 0.0)).length() < 1e-12);
    }

    #[test]
    fn turning_a_hinge_swings_the_arm_about_its_pivot() {
        let a = Articulation::of(&arm());
        let poses = a.poses(&[0.0, std::f64::consts::FRAC_PI_2]);
        // The arm's centre was 1 m below the pivot; a quarter turn about X swings it to the
        // pivot's side (toward −Z for a positive turn about +X).
        let centre = poses[1].apply(DVec3::new(0.0, -2.0, 0.0));
        assert!(
            (centre - DVec3::new(0.0, -1.0, -1.0)).length() < 1e-12,
            "{centre:?}"
        );
        // The pivot stays put.
        assert!((poses[1].apply(a.groups[1].pivot) - a.groups[1].pivot).length() < 1e-12);
    }

    #[test]
    fn angles_move_toward_targets_at_the_joint_speed_within_limits() {
        let a = Articulation::of(&arm());
        let mut s = JointState::rest(&a);
        s.targets[1] = 3.0; // beyond the 90° limit
        assert!(s.advance(&a, 0.5));
        assert!((s.angles[1] - 45f64.to_radians()).abs() < 1e-12);
        for _ in 0..10 {
            s.advance(&a, 0.5);
        }
        assert!(
            (s.angles[1] - 90f64.to_radians()).abs() < 1e-12,
            "held at the limit"
        );
        assert!(!s.advance(&a, 0.5), "nothing moves once there");
    }
}
