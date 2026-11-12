//! Thrust allocation: turn a requested force and torque into thruster throttles and
//! reaction-wheel torque (design §3).
//!
//! Solves a bounded least-squares problem with projected coordinate descent: thruster throttles
//! in [0, 1], reaction-wheel torque per axis in [−max, max]. Torque rows are weighted above
//! force rows so attitude control wins when actuators are scarce. If a request can't be met,
//! the closest achievable result and the shortfall are returned.

use glam::DVec3;

/// One force-producing actuator, relative to the center of mass, in vehicle axes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Thruster {
    /// Unit direction of the force the thruster applies to the vehicle.
    pub direction: DVec3,
    /// Position relative to the center of mass.
    pub position: DVec3,
    pub max_thrust: f64,
}

/// A requested or achieved force and torque (vehicle axes, about the center of mass).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Wrench {
    pub force: DVec3,
    pub torque: DVec3,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Allocation {
    /// Throttle per thruster, in [0, 1].
    pub throttles: Vec<f64>,
    /// Reaction-wheel torque.
    pub wheel_torque: DVec3,
    pub achieved: Wrench,
    /// Requested minus achieved.
    pub shortfall: Wrench,
}

/// Precomputed actuator geometry for one vehicle (thrusters only change in the builder).
#[derive(Clone, Debug, PartialEq)]
pub struct Allocator {
    thrusters: Vec<Thruster>,
    /// Per actuator: (force per unit throttle, torque per unit throttle).
    columns: Vec<(DVec3, DVec3)>,
    wheel_max: f64,
    /// Scales torque into force-comparable units (1 / characteristic length) times priority.
    torque_weight: f64,
}

/// Priority of torque over force in the least-squares weighting.
const TORQUE_PRIORITY: f64 = 4.0;
const SWEEPS: usize = 60;

impl Allocator {
    /// `size` is a characteristic vehicle length (m) used to weigh torque against force.
    pub fn new(thrusters: Vec<Thruster>, wheel_max_torque: f64, size: f64) -> Self {
        let columns = thrusters
            .iter()
            .map(|t| {
                let f = t.direction * t.max_thrust;
                (f, t.position.cross(f))
            })
            .collect();
        Self {
            thrusters,
            columns,
            wheel_max: wheel_max_torque.max(0.0),
            torque_weight: TORQUE_PRIORITY / size.max(1e-3),
        }
    }

    pub fn thrusters(&self) -> &[Thruster] {
        &self.thrusters
    }

    pub fn wheel_max_torque(&self) -> f64 {
        self.wheel_max
    }

    /// Best throttles for `request`. `warm_start` (e.g. last tick's throttles) speeds up
    /// convergence when requests change smoothly.
    pub fn solve(&self, request: Wrench, warm_start: Option<&[f64]>) -> Allocation {
        let n = self.columns.len();
        let w = self.torque_weight;
        let mut u: Vec<f64> = match warm_start {
            Some(s) if s.len() == n => s.iter().map(|x| x.clamp(0.0, 1.0)).collect(),
            _ => vec![0.0; n],
        };
        // Reaction wheel: three variables in [-1, 1], each a torque of wheel_max about an axis.
        let mut wheel = DVec3::ZERO;

        // Residual (achieved − requested), weighted: force rows ×1, torque rows ×w.
        let achieved = |u: &[f64], wheel: DVec3| {
            let mut a = Wrench {
                force: DVec3::ZERO,
                torque: wheel * self.wheel_max,
            };
            for (ui, (f, t)) in u.iter().zip(&self.columns) {
                a.force += *f * *ui;
                a.torque += *t * *ui;
            }
            a
        };
        let mut current = achieved(&u, wheel);

        for _ in 0..SWEEPS {
            let mut largest_step = 0.0f64;
            for (ui, &(f, t)) in u.iter_mut().zip(&self.columns) {
                let rf = current.force - request.force;
                let rt = current.torque - request.torque;
                let denom = f.length_squared() + w * w * t.length_squared();
                if denom <= 0.0 {
                    continue;
                }
                let gradient = f.dot(rf) + w * w * t.dot(rt);
                let new = (*ui - gradient / denom).clamp(0.0, 1.0);
                let delta = new - *ui;
                if delta != 0.0 {
                    *ui = new;
                    current.force += f * delta;
                    current.torque += t * delta;
                    largest_step = largest_step.max(delta.abs());
                }
            }
            if self.wheel_max > 0.0 {
                for axis in 0..3 {
                    let rt = current.torque[axis] - request.torque[axis];
                    let new = (wheel[axis] - rt / self.wheel_max).clamp(-1.0, 1.0);
                    let delta = new - wheel[axis];
                    if delta != 0.0 {
                        wheel[axis] = new;
                        current.torque[axis] += delta * self.wheel_max;
                        largest_step = largest_step.max(delta.abs());
                    }
                }
            }
            if largest_step < 1e-9 {
                break;
            }
        }

        let achieved = achieved(&u, wheel);
        Allocation {
            throttles: u,
            wheel_torque: wheel * self.wheel_max,
            achieved,
            shortfall: Wrench {
                force: request.force - achieved.force,
                torque: request.torque - achieved.torque,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A box with one thruster on each face center pushing inward-ish, plus pairs for torque.
    fn six_axis(max: f64) -> Allocator {
        let mut thrusters = Vec::new();
        for axis in [DVec3::X, DVec3::Y, DVec3::Z] {
            let side = axis.any_orthonormal_vector();
            for sign in [-1.0, 1.0] {
                // Two thrusters per direction, offset to either side, so torque is possible.
                for offset in [-1.0, 1.0] {
                    thrusters.push(Thruster {
                        direction: axis * sign,
                        position: side * offset,
                        max_thrust: max,
                    });
                }
            }
        }
        Allocator::new(thrusters, 0.0, 2.0)
    }

    #[test]
    fn meets_feasible_force_requests() {
        let a = six_axis(1_000.0);
        let request = Wrench {
            force: DVec3::new(500.0, -1_200.0, 300.0),
            torque: DVec3::ZERO,
        };
        let r = a.solve(request, None);
        assert!(r.shortfall.force.length() < 1e-3, "{:?}", r.shortfall);
        assert!(r.achieved.torque.length() < 1e-3);
        assert!(r.throttles.iter().all(|u| (0.0..=1.0).contains(u)));
    }

    #[test]
    fn reports_shortfall_when_infeasible() {
        let a = six_axis(1_000.0);
        // Two thrusters per direction: at most 2,000 N along +X.
        let r = a.solve(
            Wrench {
                force: DVec3::new(10_000.0, 0.0, 0.0),
                torque: DVec3::ZERO,
            },
            None,
        );
        assert!(
            (r.achieved.force.x - 2_000.0).abs() < 1e-6,
            "{:?}",
            r.achieved
        );
        assert!((r.shortfall.force.x - 8_000.0).abs() < 1e-6);
        assert!(r.throttles.iter().all(|u| (0.0..=1.0).contains(u)));
    }

    #[test]
    fn reaction_wheel_supplies_torque_without_force() {
        let a = Allocator::new(Vec::new(), 500.0, 1.0);
        let r = a.solve(
            Wrench {
                force: DVec3::ZERO,
                torque: DVec3::new(0.0, 300.0, -800.0),
            },
            None,
        );
        assert!((r.achieved.torque - DVec3::new(0.0, 300.0, -500.0)).length() < 1e-9);
        assert!((r.shortfall.torque.z + 300.0).abs() < 1e-9);
    }

    #[test]
    fn warm_start_gives_the_same_answer() {
        let a = six_axis(1_000.0);
        let req = Wrench {
            force: DVec3::new(100.0, 200.0, -50.0),
            torque: DVec3::new(10.0, 0.0, 5.0),
        };
        let cold = a.solve(req, None);
        let warm = a.solve(req, Some(&cold.throttles));
        assert!((cold.achieved.force - warm.achieved.force).length() < 1e-6);
    }
}
