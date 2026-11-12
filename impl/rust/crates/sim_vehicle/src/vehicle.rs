//! Vehicle instances: an object's own part list plus everything derived from it
//! (design §2–3, §9.1).

use bevy_ecs::prelude::*;
use glam::{DQuat, DVec3};

use crate::allocation::{Allocator, Thruster, Wrench};
use crate::catalog::{CruiseDriveDef, PartKind, Shape};
use crate::design::{VehicleDesign, VehiclePart};
use crate::mass::MassProperties;

/// A vehicle's own parts. Spawned from a design; later edited by the builder (Batch 3).
#[derive(Component, Clone, Debug, PartialEq)]
pub struct Vehicle {
    pub design_id: String,
    pub name: String,
    pub parts: Vec<VehiclePart>,
}

impl Vehicle {
    pub fn from_design(design: &VehicleDesign) -> Self {
        Self {
            design_id: design.id.clone(),
            name: design.name.clone(),
            parts: design.parts.clone(),
        }
    }
}

/// What a vehicle can do, from its parts (used by AI and lower simulation tiers).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Performance {
    /// Maximum linear acceleration (m/s²) along vehicle +X, −X, +Y, −Y, +Z, −Z.
    pub max_acceleration: [f64; 6],
    /// Maximum angular acceleration (rad/s²) about vehicle X, Y, Z (smaller of the two signs).
    pub max_angular_acceleration: DVec3,
    /// Cruise drive, if fitted.
    pub cruise: Option<CruiseDriveDef>,
}

impl Performance {
    /// Forward (−Z) acceleration.
    pub fn forward_acceleration(&self) -> f64 {
        self.max_acceleration[5]
    }
}

/// Physical properties derived from a [`Vehicle`]'s parts. Recompute with [`VehicleBody::of`]
/// whenever the parts change.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct VehicleBody {
    pub mass: MassProperties,
    pub allocator: Allocator,
    pub performance: Performance,
    /// One collision shape per part, in vehicle axes (the compound collider).
    pub colliders: Vec<PartCollider>,
    /// [`Vehicle::design_hash`] of the parts this was compiled from: identical designs share
    /// compiled data (colliders, meshes), and a changed hash means the vehicle was rebuilt.
    pub hash: u64,
}

/// A part's collision shape, placed in the vehicle frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PartCollider {
    pub shape: Shape,
    pub position: DVec3,
    pub rotation: DQuat,
    /// Keep this exact shape in the compiled collider (feet and wheels); other parts are merged
    /// into a few boxes.
    pub exact: bool,
}

/// Directions matching [`Performance::max_acceleration`].
pub const AXIS_DIRECTIONS: [DVec3; 6] = [
    DVec3::X,
    DVec3::NEG_X,
    DVec3::Y,
    DVec3::NEG_Y,
    DVec3::Z,
    DVec3::NEG_Z,
];

impl Vehicle {
    /// Hash of the vehicle's function data (parts, placement, attachment; not looks): equal
    /// for identical designs, so their compiled forms can be shared (vehicle-builder.md §4).
    /// Stable across runs and machines (FNV-1a over a canonical byte stream).
    pub fn design_hash(&self) -> u64 {
        let mut h = Fnv(0xcbf2_9ce4_8422_2325);
        h.write_u64(self.parts.len() as u64);
        for p in &self.parts {
            h.write_str(&p.def.id);
            for v in p
                .position
                .to_array()
                .into_iter()
                .chain(p.rotation.to_array())
            {
                // +0.0 and -0.0 hash alike.
                h.write_u64((v + 0.0).to_bits());
            }
            h.write_u64(p.parent.map_or(u64::MAX, |i| i as u64));
            match &p.attach {
                crate::design::Attach::Root => h.write_u64(0),
                crate::design::Attach::Surface => h.write_u64(1),
                crate::design::Attach::Node { node, to } => {
                    h.write_u64(2);
                    h.write_str(node);
                    h.write_str(to);
                }
            }
        }
        h.0
    }
}

struct Fnv(u64);

impl Fnv {
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 ^= u64::from(b);
            self.0 = self.0.wrapping_mul(0x0100_0000_01b3);
        }
    }

    fn write_u64(&mut self, v: u64) {
        self.write(&v.to_le_bytes());
    }

    fn write_str(&mut self, s: &str) {
        self.write_u64(s.len() as u64);
        self.write(s.as_bytes());
    }
}

impl VehicleBody {
    /// `None` if the vehicle has no parts.
    pub fn of(vehicle: &Vehicle) -> Option<Self> {
        let mass = MassProperties::of_parts(&vehicle.parts)?;
        let com = mass.center_of_mass;
        let thrusters = vehicle
            .parts
            .iter()
            .filter_map(|p| {
                p.def.thruster.map(|t| Thruster {
                    direction: (p.rotation * DVec3::Y).normalize(),
                    position: p.position - com,
                    max_thrust: t.max_thrust_n,
                })
            })
            .collect();
        let wheel: f64 = vehicle
            .parts
            .iter()
            .filter_map(|p| p.def.reaction_wheel)
            .map(|w| w.max_torque_nm)
            .sum();
        let size = vehicle
            .parts
            .iter()
            .map(|p| (p.position - com).length())
            .fold(1.0, f64::max)
            * 2.0;
        let allocator = Allocator::new(thrusters, wheel, size);
        let cruise = vehicle.parts.iter().find_map(|p| p.def.cruise_drive);
        let performance = performance(&allocator, &mass, cruise);
        let colliders = vehicle
            .parts
            .iter()
            .map(|p| PartCollider {
                shape: p.def.collision_shape(),
                position: p.position,
                rotation: p.rotation,
                exact: matches!(p.def.kind, PartKind::LandingLeg | PartKind::Wheel),
            })
            .collect();
        Some(Self {
            mass,
            allocator,
            performance,
            colliders,
            hash: vehicle.design_hash(),
        })
    }
}

fn performance(
    allocator: &Allocator,
    mass: &MassProperties,
    cruise: Option<CruiseDriveDef>,
) -> Performance {
    const HUGE: f64 = 1.0e15;
    let mut max_acceleration = [0.0; 6];
    for (i, d) in AXIS_DIRECTIONS.iter().enumerate() {
        let a = allocator.solve(
            Wrench {
                force: *d * HUGE,
                torque: DVec3::ZERO,
            },
            None,
        );
        max_acceleration[i] = (a.achieved.force.dot(*d) / mass.mass).max(0.0);
    }
    let mut angular = DVec3::ZERO;
    for axis in 0..3 {
        let dir = DVec3::AXES[axis];
        let moment = dir.dot(mass.inertia * dir);
        let reach = |sign: f64| {
            let a = allocator.solve(
                Wrench {
                    force: DVec3::ZERO,
                    torque: dir * sign * HUGE,
                },
                None,
            );
            (a.achieved.torque.dot(dir * sign) / moment).max(0.0)
        };
        angular[axis] = reach(1.0).min(reach(-1.0));
    }
    Performance {
        max_acceleration,
        max_angular_acceleration: angular,
        cruise,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::catalog::PartCatalog;

    fn content() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../../content/data")
    }

    /// Parts' total mass (kg), landing legs included.
    const KESTREL_MASS: f64 = 20_120.0;

    fn kestrel() -> Vehicle {
        let catalog =
            PartCatalog::load_dir(content().join("parts")).unwrap_or_else(|e| panic!("{e}"));
        let design = VehicleDesign::from_path(content().join("vehicles/kestrel.json"), &catalog)
            .unwrap_or_else(|e| panic!("{e}"));
        Vehicle::from_design(&design)
    }

    #[test]
    fn kestrel_matches_the_design_doc() {
        // objects-vehicles-physics.md §10: about 20 tonnes, lifts off Meridian at ~3× its weight.
        let v = kestrel();
        assert_eq!(v.parts.len(), 7 + 16 + 4);
        let body = VehicleBody::of(&v).unwrap();
        assert!(
            (body.mass.mass - KESTREL_MASS).abs() < 1e-6,
            "{}",
            body.mass.mass
        );
        // Symmetric left/right: center of mass on the YZ plane, a little low (the legs).
        let com = body.mass.center_of_mass;
        assert!(
            com.x.abs() < 1e-9 && (-0.1..0.0).contains(&com.y),
            "{com:?}"
        );
        let twr = body.performance.forward_acceleration() / 9.81;
        assert!((3.0..3.3).contains(&twr), "thrust-to-weight {twr}");
        assert!(body.performance.cruise.is_some());
    }

    #[test]
    fn kestrel_strafes_weakly_and_turns() {
        let body = VehicleBody::of(&kestrel()).unwrap();
        let p = body.performance;
        // Two 4 kN thrusters push each lateral direction: ~0.41 m/s².
        for i in 0..2 {
            assert!(
                (p.max_acceleration[i] - 8_000.0 / KESTREL_MASS).abs() < 1e-3,
                "{:?}",
                p.max_acceleration
            );
        }
        // Four up thrusters: ~0.81 m/s² (hovering needs the main engine).
        assert!((p.max_acceleration[2] - 16_000.0 / KESTREL_MASS).abs() < 1e-3);
        // Forward = main engine plus two rear maneuvering thrusters.
        assert!((p.forward_acceleration() - 608_000.0 / KESTREL_MASS).abs() < 1e-3);
        assert!(
            p.max_angular_acceleration.min_element() > 0.0,
            "can turn about every axis"
        );
    }

    #[test]
    fn allocation_holds_attitude_while_strafing() {
        let body = VehicleBody::of(&kestrel()).unwrap();
        let r = body.allocator.solve(
            Wrench {
                force: DVec3::new(5_000.0, 0.0, 0.0),
                torque: DVec3::ZERO,
            },
            None,
        );
        assert!(r.shortfall.force.length() < 1.0, "{:?}", r.shortfall);
        assert!(
            r.achieved.torque.length() < 1.0,
            "no unwanted rotation: {:?}",
            r.achieved.torque
        );
    }

    #[test]
    fn instance_edits_dont_change_the_design() {
        let catalog = PartCatalog::load_dir(content().join("parts")).unwrap();
        let design =
            VehicleDesign::from_path(content().join("vehicles/kestrel.json"), &catalog).unwrap();
        let mut v = Vehicle::from_design(&design);
        v.parts.pop();
        assert_eq!(design.parts.len(), 27);
        assert_eq!(v.parts.len(), 26);
    }

    #[test]
    fn rejects_unknown_parts_and_bad_catalogs() {
        let catalog = PartCatalog::load_dir(content().join("parts")).unwrap();
        let bad = r#"{"id": "x", "name": "X", "parts": [{"part": "no.such.part", "position_m": [0, 0, 0]}]}"#;
        assert!(VehicleDesign::from_json_str(bad, &catalog).is_err());
        let thruster_without_block = r#"{"parts": [{"id": "t.x", "name": "T", "kind": "thruster", "mass_kg": 1,
            "shape": {"type": "sphere", "radius_m": 1}}]}"#;
        assert!(PartCatalog::from_sources([("x.json", thruster_without_block)]).is_err());
    }

    #[test]
    fn design_hash_follows_function_not_identity() {
        let a = kestrel();
        let b = kestrel();
        assert_eq!(a.design_hash(), b.design_hash());
        let mut moved = kestrel();
        moved.parts[3].position.x += 0.01;
        assert_ne!(a.design_hash(), moved.design_hash());
        let mut fewer = kestrel();
        fewer.parts.pop();
        assert_ne!(a.design_hash(), fewer.design_hash());
        // Renaming a part doesn't change what the vehicle is.
        let mut renamed = kestrel();
        renamed.parts[0].name = "bridge".into();
        assert_eq!(a.design_hash(), renamed.design_hash());
    }
}
