//! Kepler orbits on rails (Batch 1, task 1.3).
//!
//! Celestial bodies follow fixed elliptical orbits around their parent: position and velocity
//! are a closed-form function of universe time, so nothing is integrated and unvisited star
//! systems cost nothing (design: universe-scale-time-and-flight.md §4.1).
//!
//! Elements use the usual astronomical definitions in a **Z-up** reference frame internally,
//! converted to the project's **Y-up** convention (see [`crate::units`]) at the boundary:
//! the reference plane is XZ and +Y is its north. Prograde orbits have angular momentum
//! along +Y.

use std::f64::consts::TAU;
use std::fmt;

use glam::{DQuat, DVec3};

/// Elliptical orbit around a parent with gravitational parameter `mu`. Time `t` is universe
/// time in seconds; the epoch is `t = 0`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KeplerOrbit {
    /// Parent's gravitational parameter (m³/s²).
    pub mu: f64,
    /// Semi-major axis `a` (m).
    pub semi_major_axis: f64,
    /// Eccentricity `e`, `0 <= e < 1`.
    pub eccentricity: f64,
    /// Inclination `i` to the reference plane (rad). Above `π/2` means retrograde.
    pub inclination: f64,
    /// Longitude of the ascending node `Ω` (rad).
    pub ascending_node: f64,
    /// Argument of periapsis `ω` (rad).
    pub arg_periapsis: f64,
    /// Mean anomaly at the epoch `M0` (rad).
    pub mean_anomaly_at_epoch: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum OrbitError {
    NonPositiveMu(f64),
    NonPositiveSemiMajorAxis(f64),
    /// Only elliptical orbits (`0 <= e < 1`) are supported on rails.
    UnsupportedEccentricity(f64),
    /// The state is not a bound (elliptical) orbit: escaping or degenerate.
    NotBound,
}

impl fmt::Display for OrbitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonPositiveMu(v) => {
                write!(f, "gravitational parameter must be positive, got {v}")
            }
            Self::NonPositiveSemiMajorAxis(v) => {
                write!(f, "semi-major axis must be positive, got {v}")
            }
            Self::UnsupportedEccentricity(e) => {
                write!(
                    f,
                    "eccentricity must be in [0, 1) for orbits on rails, got {e}"
                )
            }
            Self::NotBound => write!(f, "state is not a bound elliptical orbit"),
        }
    }
}

impl std::error::Error for OrbitError {}

/// Converts a vector from the internal Z-up frame to the project's Y-up frame.
fn zup_to_yup(v: DVec3) -> DVec3 {
    DVec3::new(v.x, v.z, -v.y)
}

/// Converts a vector from the project's Y-up frame to the internal Z-up frame.
fn yup_to_zup(v: DVec3) -> DVec3 {
    DVec3::new(v.x, -v.z, v.y)
}

/// True for finite or infinite values above zero; false for zero, negatives and NaN.
fn is_positive(x: f64) -> bool {
    x > 0.0
}

/// Wraps an angle into `[0, 2π)`.
fn wrap_angle(a: f64) -> f64 {
    a.rem_euclid(TAU)
}

impl KeplerOrbit {
    /// A circular orbit of `radius` with the given orientation, starting at `phase`
    /// (angle from the ascending node at `t = 0`).
    pub fn circular(
        mu: f64,
        radius: f64,
        inclination: f64,
        ascending_node: f64,
        phase: f64,
    ) -> Self {
        Self {
            mu,
            semi_major_axis: radius,
            eccentricity: 0.0,
            inclination,
            ascending_node,
            arg_periapsis: 0.0,
            mean_anomaly_at_epoch: phase,
        }
    }

    pub fn validate(&self) -> Result<(), OrbitError> {
        if !is_positive(self.mu) {
            return Err(OrbitError::NonPositiveMu(self.mu));
        }
        if !is_positive(self.semi_major_axis) {
            return Err(OrbitError::NonPositiveSemiMajorAxis(self.semi_major_axis));
        }
        if !(0.0..1.0).contains(&self.eccentricity) {
            return Err(OrbitError::UnsupportedEccentricity(self.eccentricity));
        }
        Ok(())
    }

    /// Orbital period (s).
    pub fn period(&self) -> f64 {
        TAU / self.mean_motion()
    }

    /// Mean motion `n` (rad/s).
    pub fn mean_motion(&self) -> f64 {
        (self.mu / self.semi_major_axis.powi(3)).sqrt()
    }

    pub fn periapsis(&self) -> f64 {
        self.semi_major_axis * (1.0 - self.eccentricity)
    }

    pub fn apoapsis(&self) -> f64 {
        self.semi_major_axis * (1.0 + self.eccentricity)
    }

    /// Mean anomaly at universe time `t`, wrapped to `[0, 2π)`.
    pub fn mean_anomaly(&self, t: f64) -> f64 {
        wrap_angle(self.mean_anomaly_at_epoch + self.mean_motion() * t)
    }

    /// Solves Kepler's equation `M = E - e sin E` for the eccentric anomaly `E`.
    pub fn eccentric_anomaly(&self, mean_anomaly: f64) -> f64 {
        solve_kepler(mean_anomaly, self.eccentricity)
    }

    /// Rotation from the perifocal frame (periapsis along +X, motion toward +Y, Z-up) into
    /// the reference frame (still Z-up).
    fn perifocal_rotation(&self) -> DQuat {
        DQuat::from_rotation_z(self.ascending_node)
            * DQuat::from_rotation_x(self.inclination)
            * DQuat::from_rotation_z(self.arg_periapsis)
    }

    /// Orientation of the orbit as a Y-up frame: rotates the orbit-local frame (orbit normal
    /// along +Y, periapsis along +X) into the parent frame. Motion is from +X toward -Z, i.e.
    /// a positive rotation about +Y.
    pub fn orientation(&self) -> DQuat {
        // The Y-up/Z-up axis swap as a rotation: -90° about X maps +Z to +Y.
        let zup_to_yup = DQuat::from_rotation_x(-std::f64::consts::FRAC_PI_2);
        (zup_to_yup * self.perifocal_rotation() * zup_to_yup.inverse()).normalize()
    }

    /// Position and velocity relative to the parent at universe time `t` (Y-up).
    pub fn state_at(&self, t: f64) -> (DVec3, DVec3) {
        let e = self.eccentricity;
        let a = self.semi_major_axis;
        let big_e = self.eccentric_anomaly(self.mean_anomaly(t));
        let (sin_e, cos_e) = big_e.sin_cos();
        let root = (1.0 - e * e).sqrt();
        let r = a * (1.0 - e * cos_e);

        let pos = DVec3::new(a * (cos_e - e), a * root * sin_e, 0.0);
        let speed_factor = (self.mu * a).sqrt() / r;
        let vel = DVec3::new(-speed_factor * sin_e, speed_factor * root * cos_e, 0.0);

        let rot = self.perifocal_rotation();
        (zup_to_yup(rot * pos), zup_to_yup(rot * vel))
    }

    /// Elements of the bound orbit with the given state (Y-up, relative to the parent) at
    /// universe time `t`. Handles circular, equatorial and retrograde orbits.
    pub fn from_state(
        mu: f64,
        position: DVec3,
        velocity: DVec3,
        t: f64,
    ) -> Result<Self, OrbitError> {
        if !is_positive(mu) {
            return Err(OrbitError::NonPositiveMu(mu));
        }
        let r = yup_to_zup(position);
        let v = yup_to_zup(velocity);
        let r_len = r.length();
        let h = r.cross(v);
        let h_len = h.length();
        let energy = v.length_squared() / 2.0 - mu / r_len;
        if !is_positive(-energy) || h_len <= f64::EPSILON * r_len * v.length() {
            return Err(OrbitError::NotBound);
        }
        let h_hat = h / h_len;
        let a = -mu / (2.0 * energy);
        let e_vec = (v.length_squared() - mu / r_len) * r / mu - r.dot(v) * v / mu;
        let e = e_vec.length();
        if e >= 1.0 {
            return Err(OrbitError::NotBound);
        }
        let inclination = h_hat.z.clamp(-1.0, 1.0).acos();

        // Node line; for equatorial orbits use +X as the reference direction.
        const TINY: f64 = 1e-11;
        let node = DVec3::Z.cross(h);
        let (ascending_node, node_hat) = if node.length() > TINY * h_len {
            let n = node.normalize();
            (wrap_angle(n.y.atan2(n.x)), n)
        } else {
            (0.0, DVec3::X)
        };

        // Signed angle from `from` to `to`, measured in the orbit plane along the motion.
        let angle_in_plane = |from: DVec3, to: DVec3| from.cross(to).dot(h_hat).atan2(from.dot(to));

        // For circular orbits periapsis is undefined: measure from the node instead.
        let (arg_periapsis, true_anomaly) = if e > TINY {
            (
                wrap_angle(angle_in_plane(node_hat, e_vec)),
                angle_in_plane(e_vec, r),
            )
        } else {
            (0.0, angle_in_plane(node_hat, r))
        };

        let (sin_nu, cos_nu) = true_anomaly.sin_cos();
        let big_e = ((1.0 - e * e).sqrt() * sin_nu).atan2(e + cos_nu);
        let mean_anomaly = big_e - e * big_e.sin();
        let n = (mu / a.powi(3)).sqrt();

        Ok(Self {
            mu,
            semi_major_axis: a,
            eccentricity: e,
            inclination,
            ascending_node,
            arg_periapsis,
            mean_anomaly_at_epoch: wrap_angle(mean_anomaly - n * t),
        })
    }
}

/// Solves Kepler's equation `M = E - e sin E` (elliptical, `0 <= e < 1`) by Newton's method.
pub fn solve_kepler(mean_anomaly: f64, e: f64) -> f64 {
    let m = wrap_angle(mean_anomaly);
    // Starting guess robust for high eccentricity.
    let mut big_e = if e < 0.8 { m } else { std::f64::consts::PI };
    for _ in 0..50 {
        let f = big_e - e * big_e.sin() - m;
        let step = f / (1.0 - e * big_e.cos());
        big_e -= step;
        if step.abs() < 1e-15 {
            break;
        }
    }
    big_e
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::{KM, STANDARD_GRAVITY, deg, mu_from_surface_gravity};

    fn home_mu() -> f64 {
        mu_from_surface_gravity(600.0 * KM, STANDARD_GRAVITY)
    }

    fn close(a: f64, b: f64, rel: f64) -> bool {
        (a - b).abs() <= rel * a.abs().max(b.abs()).max(1.0)
    }

    fn vec_close(a: DVec3, b: DVec3, tol: f64) -> bool {
        (a - b).length() <= tol
    }

    /// A spread of orbits covering the special cases.
    fn test_orbits() -> Vec<KeplerOrbit> {
        let mu = home_mu();
        let base = KeplerOrbit {
            mu,
            semi_major_axis: 2_000.0 * KM,
            eccentricity: 0.3,
            inclination: deg(28.0),
            ascending_node: deg(40.0),
            arg_periapsis: deg(110.0),
            mean_anomaly_at_epoch: deg(15.0),
        };
        vec![
            base,
            KeplerOrbit {
                eccentricity: 0.0,
                ..base
            }, // circular inclined
            KeplerOrbit {
                inclination: 0.0,
                ..base
            }, // equatorial
            KeplerOrbit {
                inclination: 0.0,
                eccentricity: 0.0,
                ..base
            }, // circular equatorial
            KeplerOrbit {
                inclination: deg(150.0),
                ..base
            }, // retrograde
            KeplerOrbit {
                inclination: std::f64::consts::PI,
                ..base
            }, // retrograde equatorial
            KeplerOrbit {
                eccentricity: 0.95,
                ..base
            }, // very eccentric
            KeplerOrbit {
                inclination: deg(90.0),
                ..base
            }, // polar
        ]
    }

    #[test]
    fn kepler_solver_is_accurate() {
        for &e in &[0.0, 0.1, 0.5, 0.9, 0.99] {
            for i in 0..100 {
                let m = i as f64 * TAU / 100.0;
                let big_e = solve_kepler(m, e);
                let residual = big_e - e * big_e.sin() - wrap_angle(m);
                assert!(residual.abs() < 1e-12, "e={e} M={m} residual={residual}");
            }
        }
    }

    #[test]
    fn circular_orbit_has_constant_radius_and_speed() {
        let mu = home_mu();
        let r = 670.0 * KM;
        let orbit = KeplerOrbit::circular(mu, r, deg(10.0), deg(30.0), 0.0);
        for i in 0..50 {
            let (p, v) = orbit.state_at(i as f64 * 97.0);
            assert!(close(p.length(), r, 1e-12));
            assert!(close(v.length(), (mu / r).sqrt(), 1e-12));
        }
        // Low orbit of a 600 km 1g planet: about 30 minutes (design doc §4.2).
        let minutes = orbit.period() / 60.0;
        assert!((28.0..33.0).contains(&minutes), "{minutes}");
    }

    #[test]
    fn returns_to_start_after_one_period() {
        for orbit in test_orbits() {
            let (p0, v0) = orbit.state_at(1234.5);
            let (p1, v1) = orbit.state_at(1234.5 + orbit.period());
            assert!(vec_close(p0, p1, 1e-6 * p0.length()), "{orbit:?}");
            assert!(vec_close(v0, v1, 1e-6 * v0.length()), "{orbit:?}");
        }
    }

    #[test]
    fn energy_and_angular_momentum_are_conserved() {
        for orbit in test_orbits() {
            let energy = -orbit.mu / (2.0 * orbit.semi_major_axis);
            let (p0, v0) = orbit.state_at(0.0);
            let h0 = p0.cross(v0);
            for i in 0..40 {
                let (p, v) = orbit.state_at(i as f64 * orbit.period() / 37.0);
                let e = v.length_squared() / 2.0 - orbit.mu / p.length();
                assert!(close(e, energy, 1e-10), "{orbit:?}");
                assert!(vec_close(p.cross(v), h0, 1e-9 * h0.length()), "{orbit:?}");
            }
        }
    }

    #[test]
    fn periapsis_and_apoapsis_distances() {
        let orbit = KeplerOrbit {
            mean_anomaly_at_epoch: 0.0,
            ..test_orbits()[0]
        };
        let (peri, _) = orbit.state_at(0.0);
        let (apo, _) = orbit.state_at(orbit.period() / 2.0);
        assert!(close(peri.length(), orbit.periapsis(), 1e-12));
        assert!(close(apo.length(), orbit.apoapsis(), 1e-12));
    }

    #[test]
    fn velocity_matches_finite_difference() {
        for orbit in test_orbits() {
            let t = 777.0;
            let h = 1e-3;
            let (p_minus, _) = orbit.state_at(t - h);
            let (p_plus, _) = orbit.state_at(t + h);
            let (_, v) = orbit.state_at(t);
            let fd = (p_plus - p_minus) / (2.0 * h);
            assert!(
                vec_close(fd, v, 1e-5 * v.length()),
                "{orbit:?}: {fd} vs {v}"
            );
        }
    }

    #[test]
    fn y_up_convention() {
        // Prograde equatorial orbit: angular momentum along +Y, motion stays in the XZ plane.
        let orbit = KeplerOrbit::circular(home_mu(), 1_000.0 * KM, 0.0, 0.0, 0.0);
        let (p, v) = orbit.state_at(100.0);
        assert!(p.y.abs() < 1e-6 && v.y.abs() < 1e-9);
        assert!(p.cross(v).normalize().dot(DVec3::Y) > 0.999_999);
        // A 90° inclined circular orbit reaches the full radius above the reference plane.
        let polar = KeplerOrbit::circular(home_mu(), 1_000.0 * KM, deg(90.0), 0.0, 0.0);
        let (top, _) = polar.state_at(polar.period() / 4.0);
        assert!(close(top.y, 1_000.0 * KM, 1e-9), "{top}");
    }

    #[test]
    fn orientation_matches_orbit_geometry() {
        for orbit in test_orbits() {
            let q = orbit.orientation();
            let (p, v) = orbit.state_at(0.0);
            let normal = p.cross(v).normalize();
            assert!((q * DVec3::Y).dot(normal) > 1.0 - 1e-12, "{orbit:?}");
            if orbit.eccentricity > 0.0 {
                let peri = KeplerOrbit {
                    mean_anomaly_at_epoch: 0.0,
                    ..orbit
                }
                .state_at(0.0)
                .0;
                assert!(
                    (q * DVec3::X).dot(peri.normalize()) > 1.0 - 1e-12,
                    "{orbit:?}"
                );
            }
        }
    }

    #[test]
    fn from_state_round_trips() {
        for orbit in test_orbits() {
            let t0 = 500.0;
            let (p, v) = orbit.state_at(t0);
            let back = KeplerOrbit::from_state(orbit.mu, p, v, t0).unwrap();
            assert!(
                close(back.semi_major_axis, orbit.semi_major_axis, 1e-9),
                "{orbit:?}"
            );
            assert!(
                (back.eccentricity - orbit.eccentricity).abs() < 1e-9,
                "{orbit:?}"
            );
            // Compare states rather than angles (angles are ambiguous for special orbits).
            for k in 0..10 {
                let t = t0 + k as f64 * orbit.period() / 7.3;
                let (pa, va) = orbit.state_at(t);
                let (pb, vb) = back.state_at(t);
                assert!(
                    vec_close(pa, pb, 1e-6 * pa.length()),
                    "{orbit:?} → {back:?}"
                );
                assert!(
                    vec_close(va, vb, 1e-6 * va.length()),
                    "{orbit:?} → {back:?}"
                );
            }
        }
    }

    #[test]
    fn from_state_rejects_unbound_states() {
        let mu = home_mu();
        let r = DVec3::new(1_000.0 * KM, 0.0, 0.0);
        let escape = (2.0 * mu / r.length()).sqrt();
        let fast = DVec3::new(0.0, 0.0, escape * 1.01);
        assert_eq!(
            KeplerOrbit::from_state(mu, r, fast, 0.0),
            Err(OrbitError::NotBound)
        );
        // Straight up and down: no angular momentum.
        let radial = DVec3::new(100.0, 0.0, 0.0);
        assert_eq!(
            KeplerOrbit::from_state(mu, r, radial, 0.0),
            Err(OrbitError::NotBound)
        );
    }

    #[test]
    fn validation() {
        let ok = test_orbits()[0];
        assert!(ok.validate().is_ok());
        assert!(
            KeplerOrbit {
                eccentricity: 1.0,
                ..ok
            }
            .validate()
            .is_err()
        );
        assert!(
            KeplerOrbit {
                semi_major_axis: 0.0,
                ..ok
            }
            .validate()
            .is_err()
        );
        assert!(KeplerOrbit { mu: -1.0, ..ok }.validate().is_err());
    }
}
