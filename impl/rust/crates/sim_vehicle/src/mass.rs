//! Mass properties: parts combined into one rigid body (design §2.3).

use glam::{DMat3, DVec3};

use crate::catalog::Shape;
use crate::design::VehiclePart;

/// Total mass, center of mass and inertia tensor (about the center of mass, vehicle axes).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MassProperties {
    pub mass: f64,
    pub center_of_mass: DVec3,
    pub inertia: DMat3,
}

/// Where a placed part's mass is centred (vehicle frame): its shape's centroid.
fn centre_of(p: &VehiclePart) -> DVec3 {
    p.position + p.rotation * p.def.shape.centroid()
}

/// Inertia tensor of a uniform solid shape about its own center, in its local axes.
pub fn shape_inertia(shape: Shape, mass: f64) -> DMat3 {
    let diag = match shape {
        Shape::Box { size_m: [x, y, z] } => {
            DVec3::new(y * y + z * z, x * x + z * z, x * x + y * y) * (mass / 12.0)
        }
        Shape::Cylinder {
            radius_m: r,
            length_m: h,
        } => {
            let side = mass * (3.0 * r * r + h * h) / 12.0;
            DVec3::new(side, mass * r * r / 2.0, side)
        }
        Shape::Sphere { radius_m: r } => DVec3::splat(0.4 * mass * r * r),
        // A shell: as a box of its bounds (about its own centre of mass, near enough).
        Shape::Shell { .. } => {
            let size = shape.bounds().1 * 2.0;
            DVec3::new(
                size.y * size.y + size.z * size.z,
                size.x * size.x + size.z * size.z,
                size.x * size.x + size.y * size.y,
            ) * (mass / 12.0)
        }
    };
    DMat3::from_diagonal(diag)
}

impl MassProperties {
    /// Combines placed parts. Returns `None` for an empty part list.
    pub fn of_parts(parts: &[VehiclePart]) -> Option<Self> {
        let mass: f64 = parts.iter().map(|p| p.def.mass_kg).sum();
        if parts.is_empty() || mass <= 0.0 {
            return None;
        }
        let center_of_mass = parts
            .iter()
            .map(|p| centre_of(p) * p.def.mass_kg)
            .sum::<DVec3>()
            / mass;
        let mut inertia = DMat3::ZERO;
        for p in parts {
            let m = p.def.mass_kg;
            let r = DMat3::from_quat(p.rotation);
            let local = shape_inertia(p.def.shape, m);
            // Rotate into vehicle axes, then shift to the center of mass (parallel axis theorem).
            let d = centre_of(p) - center_of_mass;
            let shift = (DMat3::IDENTITY * d.length_squared() - outer(d, d)) * m;
            inertia += r * local * r.transpose() + shift;
        }
        Some(Self {
            mass,
            center_of_mass,
            inertia,
        })
    }
}

fn outer(a: DVec3, b: DVec3) -> DMat3 {
    DMat3::from_cols(a * b.x, a * b.y, a * b.z)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use glam::DQuat;

    use super::*;
    use crate::catalog::{PartDef, PartKind};

    fn part(mass: f64, shape: Shape, position: DVec3, rotation: DQuat) -> VehiclePart {
        VehiclePart {
            def: Arc::new(PartDef {
                id: "test.part".into(),
                name: "Test".into(),
                kind: PartKind::Hull,
                mass_kg: mass,
                shape,
                ..Default::default()
            }),
            position,
            rotation,
            ..VehiclePart::default()
        }
    }

    fn close(a: DMat3, b: DMat3) -> bool {
        (0..3).all(|i| (a.col(i) - b.col(i)).length() < 1e-9 * (1.0 + b.col(i).length()))
    }

    #[test]
    fn single_box_at_an_offset() {
        let shape = Shape::Box {
            size_m: [2.0, 4.0, 6.0],
        };
        let p = MassProperties::of_parts(&[part(
            12.0,
            shape,
            DVec3::new(5.0, 0.0, 0.0),
            DQuat::IDENTITY,
        )])
        .unwrap();
        assert_eq!(p.mass, 12.0);
        assert_eq!(p.center_of_mass, DVec3::new(5.0, 0.0, 0.0));
        // About its own center: m/12 (b²+c²) etc.
        assert!(close(
            p.inertia,
            DMat3::from_diagonal(DVec3::new(52.0, 40.0, 20.0))
        ));
    }

    #[test]
    fn two_spheres_use_the_parallel_axis_theorem() {
        let s = Shape::Sphere { radius_m: 1.0 };
        let parts = [
            part(10.0, s, DVec3::new(-3.0, 0.0, 0.0), DQuat::IDENTITY),
            part(10.0, s, DVec3::new(3.0, 0.0, 0.0), DQuat::IDENTITY),
        ];
        let p = MassProperties::of_parts(&parts).unwrap();
        assert_eq!(p.center_of_mass, DVec3::ZERO);
        let own = 0.4 * 10.0;
        let shifted = own + 10.0 * 9.0;
        assert!(close(
            p.inertia,
            DMat3::from_diagonal(DVec3::new(2.0 * own, 2.0 * shifted, 2.0 * shifted))
        ));
    }

    #[test]
    fn rotating_a_part_rotates_its_inertia() {
        // A cylinder along Y, rotated 90° about X, lies along Z.
        let shape = Shape::Cylinder {
            radius_m: 1.0,
            length_m: 4.0,
        };
        let along_y = MassProperties::of_parts(&[part(6.0, shape, DVec3::ZERO, DQuat::IDENTITY)])
            .unwrap()
            .inertia;
        let along_z = MassProperties::of_parts(&[part(
            6.0,
            shape,
            DVec3::ZERO,
            DQuat::from_rotation_x(std::f64::consts::FRAC_PI_2),
        )])
        .unwrap()
        .inertia;
        assert!((along_y.y_axis.y - along_z.z_axis.z).abs() < 1e-9);
        assert!((along_y.x_axis.x - along_z.y_axis.y).abs() < 1e-9);
    }

    #[test]
    fn empty_has_no_mass_properties() {
        assert!(MassProperties::of_parts(&[]).is_none());
    }
}
