//! Universe time and game clocks (Batch 1, task 1.4; design: universe-scale-time-and-flight.md §4).
//!
//! - [`UniverseTime`]: one integer clock (nanoseconds) for the whole universe, running 1:1 with
//!   real time (times time warp). Physics, orbits and rotation use it. Integer, so long saves
//!   never accumulate floating-point error.
//! - **Galactic Standard Time:** a scaled view of universe time, [`GAME_TIME_SCALE`]× faster,
//!   for contracts, news and anything that crosses planets.
//! - **Local time:** the time of day at a place on a planet, from where its star is in the sky;
//!   24 local hours per solar day.

use std::f64::consts::TAU;
use std::fmt;
use std::time::Duration;

use glam::DVec3;

use crate::frames::{FrameId, FrameTree};

/// Galactic Standard Time runs this many times faster than universe time
/// (1 real minute = 16 game minutes, so a 90-minute day reads as 24 hours).
pub const GAME_TIME_SCALE: u64 = 16;

const NANOS_PER_SEC: u64 = 1_000_000_000;

/// Universe time since the save's epoch, in integer nanoseconds (enough for ~584 years).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UniverseTime {
    nanos: u64,
}

impl UniverseTime {
    pub const EPOCH: Self = Self { nanos: 0 };

    pub fn from_nanos(nanos: u64) -> Self {
        Self { nanos }
    }

    pub fn from_secs(secs: u64) -> Self {
        Self {
            nanos: secs.saturating_mul(NANOS_PER_SEC),
        }
    }

    pub fn as_nanos(self) -> u64 {
        self.nanos
    }

    /// Seconds as `f64`, for physics and orbits.
    pub fn as_secs_f64(self) -> f64 {
        self.nanos as f64 / NANOS_PER_SEC as f64
    }

    /// Adds `duration`, saturating at the maximum representable time.
    pub fn advance(&mut self, duration: Duration) {
        let add = u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX);
        self.nanos = self.nanos.saturating_add(add);
    }

    /// Whole seconds of Galactic Standard Time since the epoch.
    pub fn galactic_seconds(self) -> u64 {
        (self.nanos as u128 * GAME_TIME_SCALE as u128 / NANOS_PER_SEC as u128) as u64
    }

    /// Galactic Standard Time as a calendar date and time.
    pub fn galactic_standard(self) -> GameDateTime {
        GameDateTime::from_game_seconds(self.galactic_seconds())
    }
}

/// A date and time on a 24-hour game clock. Days count from 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct GameDateTime {
    pub day: u64,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

impl GameDateTime {
    pub fn from_game_seconds(secs: u64) -> Self {
        Self {
            day: secs / 86_400 + 1,
            hour: (secs / 3_600 % 24) as u8,
            minute: (secs / 60 % 60) as u8,
            second: (secs % 60) as u8,
        }
    }
}

impl fmt::Display for GameDateTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Day {}, {:02}:{:02}", self.day, self.hour, self.minute)
    }
}

/// Length of a solar day (noon to noon) for a planet with the given sidereal spin period and
/// orbital period, both in seconds and prograde. Infinite for a tidally locked body.
pub fn solar_day(sidereal_period: f64, orbital_period: f64) -> f64 {
    let rate = 1.0 / sidereal_period - 1.0 / orbital_period;
    if rate.abs() < 1e-15 {
        f64::INFINITY
    } else {
        (1.0 / rate).abs()
    }
}

/// A point on a body's surface frame at `longitude` and `latitude` (radians) and `radius`.
/// Longitude is measured from the surface frame's +X axis, positive **east** (the direction
/// of rotation); +Y is north.
pub fn surface_point(longitude: f64, latitude: f64, radius: f64) -> DVec3 {
    let (sin_lat, cos_lat) = latitude.sin_cos();
    let (sin_lon, cos_lon) = longitude.sin_cos();
    DVec3::new(cos_lat * cos_lon, sin_lat, -cos_lat * sin_lon) * radius
}

/// Local solar time at `longitude` on a body, as a fraction of the solar day in `[0, 1)`:
/// 0 = midnight, 0.25 = 6:00, 0.5 = noon (star at its highest), 0.75 = 18:00.
///
/// `surface` is the body's rotating surface frame and `star` the frame of the star lighting it.
pub fn local_solar_fraction(
    tree: &FrameTree,
    surface: FrameId,
    star: FrameId,
    longitude: f64,
    t: UniverseTime,
) -> f64 {
    let sun = tree.transform_point(surface, star, t.as_secs_f64(), DVec3::ZERO);
    // Longitude directly under the star (same east-positive convention as `surface_point`).
    let sun_longitude = (-sun.z).atan2(sun.x);
    let hour_angle = longitude - sun_longitude;
    (hour_angle / TAU + 0.5).rem_euclid(1.0)
}

/// Local 24-hour clock reading for a fraction of the solar day.
pub fn local_clock(fraction: f64) -> (u8, u8) {
    let minutes = (fraction.rem_euclid(1.0) * 1_440.0).floor() as u32 % 1_440;
    ((minutes / 60) as u8, (minutes % 60) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frames::{FrameMotion, Spin};
    use crate::orbit::KeplerOrbit;
    use crate::units::deg;

    #[test]
    fn universe_time_is_integer_and_exact() {
        let mut t = UniverseTime::EPOCH;
        let tick = Duration::from_secs(1) / 30;
        for _ in 0..30 * 3_600 {
            t.advance(tick);
        }
        // 30 ticks of 33,333,333 ns: integer sums, no floating-point accumulation.
        assert_eq!(t.as_nanos(), 30 * 3_600 * 33_333_333);
        assert!((t.as_secs_f64() - 3_600.0).abs() < 1e-3);
    }

    #[test]
    fn galactic_standard_time_runs_16x() {
        // 90 real minutes = one 24-hour galactic day.
        let t = UniverseTime::from_secs(90 * 60);
        assert_eq!(
            t.galactic_standard(),
            GameDateTime {
                day: 2,
                hour: 0,
                minute: 0,
                second: 0
            }
        );
        let t = UniverseTime::from_secs(60);
        assert_eq!(t.galactic_standard().to_string(), "Day 1, 00:16");
        assert_eq!(
            UniverseTime::EPOCH.galactic_standard().to_string(),
            "Day 1, 00:00"
        );
    }

    #[test]
    fn saturates_instead_of_overflowing() {
        let mut t = UniverseTime::from_nanos(u64::MAX - 5);
        t.advance(Duration::from_secs(10));
        assert_eq!(t.as_nanos(), u64::MAX);
    }

    #[test]
    fn solar_day_lengths() {
        // Spin 90 min, orbit 40 h: the solar day is slightly longer than the spin period.
        let d = solar_day(5_400.0, 144_000.0);
        assert!(d > 5_400.0 && d < 5_700.0, "{d}");
        assert!(solar_day(1_000.0, 1_000.0).is_infinite());
    }

    #[test]
    fn surface_point_convention() {
        let p = surface_point(deg(90.0), 0.0, 1.0);
        // 90° east of +X is -Z: the direction a positive spin about +Y carries +X.
        assert!((p - DVec3::new(0.0, 0.0, -1.0)).length() < 1e-12);
        let north = surface_point(0.0, deg(90.0), 1.0);
        assert!((north - DVec3::Y).length() < 1e-12);
    }

    /// Star at the root, one planet with a 90-minute spin on a 40-hour orbit.
    fn star_and_planet() -> (FrameTree, FrameId, FrameId) {
        let mut tree = FrameTree::new("star");
        let star = tree.root();
        let orbit = KeplerOrbit::circular(1.0e20, 1.0e10, 0.0, 0.0, 0.0);
        let planet = tree.add("planet", star, FrameMotion::Orbit(orbit)).unwrap();
        let surface = tree
            .add(
                "planet.surface",
                planet,
                FrameMotion::Spin(Spin::tilted(deg(23.0), 5_400.0, 0.0)),
            )
            .unwrap();
        (tree, star, surface)
    }

    #[test]
    fn local_noon_is_when_the_star_is_highest() {
        let (tree, star, surface) = star_and_planet();
        let t = UniverseTime::from_secs(1_000);
        // Find the longitude where it's noon, and check the star is overhead-ish there
        // (highest elevation along the equator).
        let fraction_at_zero = local_solar_fraction(&tree, surface, star, 0.0, t);
        let noon_longitude = (0.5 - fraction_at_zero) * TAU;
        let at_noon = local_solar_fraction(&tree, surface, star, noon_longitude, t);
        assert!((at_noon - 0.5).abs() < 1e-9);
        let sun = tree
            .transform_point(surface, star, t.as_secs_f64(), DVec3::ZERO)
            .normalize();
        let up = surface_point(noon_longitude, 0.0, 1.0);
        for k in 1..12 {
            let other = surface_point(noon_longitude + k as f64 * 0.5, 0.0, 1.0);
            assert!(sun.dot(up) >= sun.dot(other) - 1e-12);
        }
    }

    #[test]
    fn local_time_advances_one_day_per_solar_day() {
        let (tree, star, surface) = star_and_planet();
        let orbit_period = KeplerOrbit::circular(1.0e20, 1.0e10, 0.0, 0.0, 0.0).period();
        let day = solar_day(5_400.0, orbit_period);
        let at = |secs: f64| {
            local_solar_fraction(
                &tree,
                surface,
                star,
                0.3,
                UniverseTime::from_nanos((secs * 1e9) as u64),
            )
        };
        let f0 = at(10_000.0);
        // A quarter solar day later it's 6 local hours later; a full solar day later, the same time.
        assert!(((at(10_000.0 + day / 4.0) - f0).rem_euclid(1.0) - 0.25).abs() < 1e-3);
        assert!(((at(10_000.0 + day) - f0 + 0.5).rem_euclid(1.0) - 0.5).abs() < 1e-3);
        // Further east is later in the day.
        let east = local_solar_fraction(
            &tree,
            surface,
            star,
            0.3 + deg(90.0),
            UniverseTime::from_secs(10_000),
        );
        assert!(((east - f0).rem_euclid(1.0) - 0.25).abs() < 1e-3);
    }

    #[test]
    fn local_clock_reading() {
        assert_eq!(local_clock(0.0), (0, 0));
        assert_eq!(local_clock(0.5), (12, 0));
        assert_eq!(local_clock(0.75 + 0.5 / 1_440.0), (18, 0));
        assert_eq!(local_clock(1.0), (0, 0));
    }
}
