//! Interference checks for designs: which parts overlap other parts in a given pose. A
//! design's parts should only touch where they are joined; anything else that overlaps (an arm
//! through a shell, a shell through a leg) is a design error the builder and the design tests
//! report. Parts are boxes here: a procedural shape's tiles, the box around any other shape.

use glam::{DQuat, DVec3};

use crate::joints::{Articulation, Pose};
use crate::vehicle::Vehicle;

/// An oriented box in the vehicle frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Obb {
    pub centre: DVec3,
    pub rotation: DQuat,
    pub half: DVec3,
}

impl Obb {
    fn axes(&self) -> [DVec3; 3] {
        [
            self.rotation * DVec3::X,
            self.rotation * DVec3::Y,
            self.rotation * DVec3::Z,
        ]
    }

    fn reach(&self, axis: DVec3) -> f64 {
        let [x, y, z] = self.axes();
        self.half.x * x.dot(axis).abs()
            + self.half.y * y.dot(axis).abs()
            + self.half.z * z.dot(axis).abs()
    }
}

/// How deep two boxes overlap (m): the smallest overlap over the separating axes, or `None`
/// when they don't.
pub fn overlap(a: &Obb, b: &Obb) -> Option<f64> {
    let (aa, ba) = (a.axes(), b.axes());
    let mut axes: Vec<DVec3> = aa.iter().chain(ba.iter()).copied().collect();
    for x in aa {
        for y in ba {
            let c = x.cross(y);
            if c.length_squared() > 1e-12 {
                axes.push(c.normalize());
            }
        }
    }
    let d = b.centre - a.centre;
    let mut least = f64::MAX;
    for axis in axes {
        let gap = a.reach(axis) + b.reach(axis) - d.dot(axis).abs();
        if gap <= 0.0 {
            return None;
        }
        least = least.min(gap);
    }
    Some(least)
}

/// Each part's boxes in the rest frame: its procedural shape's tiles, or the box around its
/// shape.
pub fn part_boxes(vehicle: &Vehicle) -> Vec<Vec<Obb>> {
    vehicle
        .parts
        .iter()
        .map(|p| {
            let tiles = p.def.shape.tiles();
            if tiles.is_empty() {
                let (mid, half) = p.def.shape.bounds();
                vec![Obb {
                    centre: p.position + p.rotation * mid,
                    rotation: p.rotation,
                    half,
                }]
            } else {
                tiles
                    .into_iter()
                    .map(|t| Obb {
                        centre: p.position + p.rotation * t.centre,
                        rotation: (p.rotation * t.rotation).normalize(),
                        half: t.half,
                    })
                    .collect()
            }
        })
        .collect()
}

/// Two parts overlapping in a pose: their indices and the deepest overlap (m).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Interference {
    pub a: usize,
    pub b: usize,
    pub depth: f64,
}

/// Whether parts `a` and `b` are joined: one is the other's parent, or both hang on the same
/// joint part from either side (the limbs meeting at a hinge touch there).
fn joined(vehicle: &Vehicle, a: usize, b: usize) -> bool {
    let parent = |i: usize| vehicle.parts[i].parent;
    let is_joint = |i: usize| vehicle.parts[i].def.joint.is_some();
    parent(a) == Some(b)
        || parent(b) == Some(a)
        || parent(b).is_some_and(|j| is_joint(j) && parent(j) == Some(a))
        || parent(a).is_some_and(|j| is_joint(j) && parent(j) == Some(b))
}

/// Every pair of parts that overlap by more than `tolerance` (m) with the joints at `angles`
/// (one per rigid group), except parts that are joined (they meet where they join).
pub fn interferences(
    vehicle: &Vehicle,
    articulation: &Articulation,
    angles: &[f64],
    tolerance: f64,
) -> Vec<Interference> {
    let poses = articulation.poses(angles);
    let posed = |b: &Obb, pose: &Pose| Obb {
        centre: pose.apply(b.centre),
        rotation: (pose.rotation * b.rotation).normalize(),
        half: b.half,
    };
    let boxes: Vec<Vec<Obb>> = part_boxes(vehicle)
        .iter()
        .enumerate()
        .map(|(i, bs)| {
            let pose = &poses[articulation.part_group[i]];
            bs.iter().map(|b| posed(b, pose)).collect()
        })
        .collect();
    // A sphere around each part's boxes, to skip far pairs quickly.
    let spheres: Vec<(DVec3, f64)> = boxes
        .iter()
        .map(|bs| {
            let centre = bs.iter().map(|b| b.centre).sum::<DVec3>() / bs.len() as f64;
            let radius = bs
                .iter()
                .map(|b| b.centre.distance(centre) + b.half.length())
                .fold(0.0, f64::max);
            (centre, radius)
        })
        .collect();
    let mut found = Vec::new();
    for a in 0..vehicle.parts.len() {
        for b in a + 1..vehicle.parts.len() {
            if joined(vehicle, a, b) {
                continue;
            }
            let ((ca, ra), (cb, rb)) = (spheres[a], spheres[b]);
            if ca.distance(cb) > ra + rb {
                continue;
            }
            let depth = boxes[a]
                .iter()
                .flat_map(|x| boxes[b].iter().filter_map(move |y| overlap(x, y)))
                .fold(0.0, f64::max);
            if depth > tolerance {
                found.push(Interference { a, b, depth });
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cube(at: DVec3, turn: f64) -> Obb {
        Obb {
            centre: at,
            rotation: DQuat::from_rotation_y(turn),
            half: DVec3::splat(0.5),
        }
    }

    #[test]
    fn boxes_overlap_by_their_shared_depth() {
        let a = cube(DVec3::ZERO, 0.0);
        let depth = overlap(&a, &cube(DVec3::new(0.8, 0.0, 0.0), 0.0)).unwrap();
        assert!((depth - 0.2).abs() < 1e-9, "{depth}");
        assert!(overlap(&a, &cube(DVec3::new(1.1, 0.0, 0.0), 0.0)).is_none());
        // Turned 45°: its corner reaches 0.707 m, so they touch closer than 1.207 m apart.
        assert!(overlap(&a, &cube(DVec3::new(1.25, 0.0, 0.0), 0.785)).is_none());
        assert!(overlap(&a, &cube(DVec3::new(1.15, 0.0, 0.0), 0.785)).is_some());
    }
}
