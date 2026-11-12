//! Units and coordinate conventions (Batch 1, task 1.1).
//!
//! - **Length:** meters (`f64` for anything in world or frame space).
//! - **Time:** seconds (`f64` for durations and universe time derived from the tick counter).
//! - **Angles:** radians.
//! - **Gravity:** gravitational parameter `mu = G * M` in m³/s², not mass, since `mu` is known
//!   far more precisely and is what orbits need.
//! - **Axes:** right-handed, **Y up**, the same as Bevy and glam. A star system's reference
//!   plane (its ecliptic) is the XZ plane; +Y is ecliptic north.
//! - **Rotations:** `DQuat`, rotating vectors from a child frame into its parent frame.

/// One kilometer in meters.
pub const KM: f64 = 1_000.0;

/// Newtonian gravitational constant (m³ kg⁻¹ s⁻²). Only for converting a mass into `mu`.
pub const G: f64 = 6.674_30e-11;

/// Standard gravity at Earth's surface (m/s²).
pub const STANDARD_GRAVITY: f64 = 9.806_65;

/// Degrees to radians.
pub fn deg(degrees: f64) -> f64 {
    degrees.to_radians()
}

/// `mu` (m³/s²) for a body of the given radius (m) and surface gravity (m/s²): `mu = g * r²`.
/// Handy for compressed planets defined by size and gravity rather than mass.
pub fn mu_from_surface_gravity(radius: f64, surface_gravity: f64) -> f64 {
    surface_gravity * radius * radius
}

/// Surface gravity (m/s²) of a body with gravitational parameter `mu` and radius `radius`.
pub fn surface_gravity(mu: f64, radius: f64) -> f64 {
    mu / (radius * radius)
}

/// Speed of a circular orbit of radius `r` around a body with parameter `mu`.
pub fn circular_speed(mu: f64, r: f64) -> f64 {
    (mu / r).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compressed_home_world_numbers() {
        // A 600 km, 1g planet (design doc: universe-scale-time-and-flight.md §1).
        let r = 600.0 * KM;
        let mu = mu_from_surface_gravity(r, STANDARD_GRAVITY);
        assert!((surface_gravity(mu, r) - STANDARD_GRAVITY).abs() < 1e-12);
        // Low orbit (70 km up) is about 2.3 km/s, like KSP's Kerbin.
        let v = circular_speed(mu, r + 70.0 * KM);
        assert!((2_200.0..2_500.0).contains(&v), "{v}");
    }

    #[test]
    fn degrees() {
        assert!((deg(180.0) - std::f64::consts::PI).abs() < 1e-15);
    }
}
