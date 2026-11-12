//! Wheels (Batch 3, task 3.5; design: vehicle-builder.md §2.4).
//!
//! A vehicle with wheel parts drives with rapier's raycast vehicle controller: each wheel casts
//! a ray along its suspension against the ground patches and other colliders, and the
//! controller applies suspension, drive, brake, steering and tyre friction to the vehicle's one
//! rigid body. Wheels are not part of the vehicle's collider.

use glam::DVec3;
use rapier3d_f64::control::{DynamicRayCastVehicleController, WheelTuning};
use rapier3d_f64::prelude::RigidBodyHandle;

use crate::catalog::WheelDef;
use crate::design::VehiclePart;
use crate::flight::FlightControl;

/// Below this speed (m/s) with idle controls the parking brake holds the vehicle.
const PARKING_SPEED: f64 = 1.0;

/// A wheel as mounted on a vehicle (vehicle axes).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WheelMount {
    /// Hub position at rest (the part's position).
    pub position: DVec3,
    /// Suspension direction (the part's −Y): the way the wheel moves when the spring extends.
    pub down: DVec3,
    /// Axle direction (the part's +X).
    pub axle: DVec3,
    pub def: WheelDef,
}

impl WheelMount {
    pub fn of(part: &VehiclePart) -> Option<Self> {
        Some(Self {
            position: part.position,
            down: part.rotation * DVec3::NEG_Y,
            axle: part.rotation * DVec3::X,
            def: part.def.wheel?,
        })
    }
}

/// What the driver asks of the wheels: throttle and steering in [−1, 1] (forward, right) and
/// the brake.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Drive {
    pub throttle: f64,
    pub steer: f64,
    pub brake: bool,
}

impl Drive {
    /// From the flight controls: W/S (forward/back) drive, A/D steer, X or Space brake.
    pub fn from_control(control: &FlightControl) -> Self {
        Self {
            throttle: (-control.translate.z).clamp(-1.0, 1.0),
            steer: control.translate.x.clamp(-1.0, 1.0),
            brake: control.brake || control.translate.y > 0.5,
        }
    }

    pub fn is_idle(&self) -> bool {
        self.throttle == 0.0 && self.steer == 0.0
    }
}

/// Builds the raycast vehicle controller for a chassis of `mass` kg with these wheels.
pub(crate) fn controller(
    chassis: RigidBodyHandle,
    wheels: &[WheelMount],
    mass: f64,
) -> DynamicRayCastVehicleController {
    let mut c = DynamicRayCastVehicleController::new(chassis);
    c.index_up_axis = 1;
    c.index_forward_axis = 2;
    let per_wheel_load = mass * 30.0 / wheels.len().max(1) as f64;
    for w in wheels {
        let d = w.def;
        // Rapier's spring and damper forces are multiplied by the chassis mass.
        let tuning = WheelTuning {
            suspension_stiffness: d.stiffness_n_per_m / mass,
            suspension_compression: d.damping_n_s_per_m / mass,
            suspension_damping: d.damping_n_s_per_m / mass,
            max_suspension_travel: d.suspension_m,
            side_friction_stiffness: 1.0,
            friction_slip: d.friction * 10.0,
            max_suspension_force: per_wheel_load,
        };
        // The suspension starts its rest length above the hub.
        let mount = w.position - w.down * d.suspension_m;
        c.add_wheel(mount, w.down, w.axle, d.suspension_m, d.radius_m, &tuning);
    }
    c
}

/// Sets each wheel's drive force, brake and steering for the next step of `dt` seconds.
pub(crate) fn apply(
    c: &mut DynamicRayCastVehicleController,
    wheels: &[WheelMount],
    drive: Drive,
    dt: f64,
) {
    // Parking brake: idle controls and nearly stopped hold the vehicle (it doesn't roll away
    // down a gentle slope).
    let parked = drive.is_idle() && c.current_vehicle_speed.abs() < PARKING_SPEED;
    for (wheel, mount) in c.wheels_mut().iter_mut().zip(wheels) {
        let d = mount.def;
        wheel.engine_force = drive.throttle * d.max_motor_torque_nm / d.radius_m;
        wheel.brake = if drive.brake || parked {
            d.max_brake_torque_nm / d.radius_m * dt
        } else {
            0.0
        };
        // Front wheels steer (positive steering turns left about the vehicle.s up axis).
        wheel.steering = if mount.position.z < 0.0 {
            -drive.steer * d.max_steer_deg.to_radians()
        } else {
            0.0
        };
    }
}
