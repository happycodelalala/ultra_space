//! Deterministic 3D gradient noise in `f64` (planet-surface.md §2).
//!
//! Plain arithmetic and integer hashing only, so the same seed gives the same terrain on every
//! machine. Coordinates are unbounded (no permutation table to wrap around).

use glam::{DMat3, DVec3};

/// SplitMix64 finaliser: a strong 64-bit mix.
pub fn mix64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

/// Hash of an integer lattice point and a seed.
pub fn hash3(x: i64, y: i64, z: i64, seed: u64) -> u64 {
    let mut h = mix64(seed ^ 0x51_7CC1_B727_220A);
    h = mix64(h ^ x as u64);
    h = mix64(h ^ y as u64);
    mix64(h ^ z as u64)
}

/// A hash as a uniform number in [0, 1).
pub fn unit(h: u64) -> f64 {
    (h >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
}

/// The 12 edge directions of a cube (improved Perlin noise gradients).
const GRADIENTS: [[f64; 3]; 12] = [
    [1.0, 1.0, 0.0],
    [-1.0, 1.0, 0.0],
    [1.0, -1.0, 0.0],
    [-1.0, -1.0, 0.0],
    [1.0, 0.0, 1.0],
    [-1.0, 0.0, 1.0],
    [1.0, 0.0, -1.0],
    [-1.0, 0.0, -1.0],
    [0.0, 1.0, 1.0],
    [0.0, -1.0, 1.0],
    [0.0, 1.0, -1.0],
    [0.0, -1.0, -1.0],
];

fn fade(t: f64) -> f64 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

/// Gradient noise at `p`, roughly in [-1, 1], zero at lattice points.
pub fn gradient(p: DVec3, seed: u64) -> f64 {
    let cell = p.floor();
    let f = p - cell;
    let (ix, iy, iz) = (cell.x as i64, cell.y as i64, cell.z as i64);
    let corner = |dx: i64, dy: i64, dz: i64| {
        let g = GRADIENTS[(hash3(ix + dx, iy + dy, iz + dz, seed) % 12) as usize];
        let d = f - DVec3::new(dx as f64, dy as f64, dz as f64);
        g[0] * d.x + g[1] * d.y + g[2] * d.z
    };
    let (u, v, w) = (fade(f.x), fade(f.y), fade(f.z));
    let x00 = lerp(corner(0, 0, 0), corner(1, 0, 0), u);
    let x10 = lerp(corner(0, 1, 0), corner(1, 1, 0), u);
    let x01 = lerp(corner(0, 0, 1), corner(1, 0, 1), u);
    let x11 = lerp(corner(0, 1, 1), corner(1, 1, 1), u);
    lerp(lerp(x00, x10, v), lerp(x01, x11, v), w)
}

/// A fixed irrational rotation applied between octaves so lattice axes don't line up
/// (avoids visible grid directions and seams).
fn octave_rotation() -> DMat3 {
    DMat3::from_euler(glam::EulerRot::XYZ, 0.73, 1.21, 0.37)
}

/// Normalisation for a fractal sum: the full (infinite) series' amplitude, so a sum with fewer
/// octaves is exactly the same terrain minus its finest details (stable across detail levels).
fn series_norm(gain: f64) -> f64 {
    1.0 / (1.0 - gain.clamp(0.0, 0.95))
}

/// Fractal sum of gradient noise. `p` is in units of the first octave's wavelength; octaves
/// halve the wavelength and multiply the amplitude by `gain`. Returns roughly [-1, 1]; fewer
/// octaves give a prefix of the same sum.
pub fn fbm(p: DVec3, seed: u64, octaves: u32, gain: f64) -> f64 {
    let rot = octave_rotation();
    let (mut sum, mut amplitude) = (0.0, 1.0);
    let mut q = p;
    for k in 0..octaves {
        sum += amplitude * gradient(q, seed.wrapping_add(u64::from(k) * 0x9E37));
        amplitude *= gain;
        q = rot * q * 2.0 + DVec3::splat(17.31);
    }
    sum / series_norm(gain)
}

/// Ridged fractal noise for mountain ranges: sharp crests, in [0, 1]. Fewer octaves give a
/// prefix of the same sum.
pub fn ridged(p: DVec3, seed: u64, octaves: u32, gain: f64) -> f64 {
    let rot = octave_rotation();
    let (mut sum, mut amplitude) = (0.0, 1.0);
    let mut weight = 1.0;
    let mut q = p;
    for k in 0..octaves {
        let n = 1.0 - gradient(q, seed.wrapping_add(u64::from(k) * 0x7F4A)).abs() * 1.4;
        let n = n.clamp(0.0, 1.0);
        let n = n * n * weight;
        // Detail is strongest along crests.
        weight = n.clamp(0.0, 1.0);
        sum += amplitude * n;
        amplitude *= gain;
        q = rot * q * 2.0 + DVec3::splat(31.7);
    }
    sum / series_norm(gain)
}

/// Octaves needed to go from wavelength `longest` down to `shortest` (at least 1).
pub fn octaves_between(longest: f64, shortest: f64) -> u32 {
    if shortest <= 0.0 || longest <= shortest {
        return 1;
    }
    ((longest / shortest).log2().floor() as u32 + 1).clamp(1, 40)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_and_seed_dependent() {
        let p = DVec3::new(12.3, -4.56, 789.01);
        assert_eq!(gradient(p, 7), gradient(p, 7));
        assert_ne!(gradient(p, 7), gradient(p, 8));
        assert_eq!(fbm(p, 1, 8, 0.5), fbm(p, 1, 8, 0.5));
    }

    #[test]
    fn zero_on_lattice_points_and_bounded() {
        assert_eq!(gradient(DVec3::new(3.0, -2.0, 5.0), 1), 0.0);
        let mut seen = (f64::MAX, f64::MIN);
        for i in 0..20_000 {
            let p = DVec3::new(
                unit(mix64(i)) * 100.0,
                unit(mix64(i + 1_000_000)) * 100.0,
                unit(mix64(i + 2_000_000)) * 100.0,
            );
            let n = gradient(p, 3);
            seen = (seen.0.min(n), seen.1.max(n));
            let r = ridged(p, 3, 6, 0.5);
            assert!((0.0..=1.0).contains(&r));
        }
        assert!(seen.0 > -1.2 && seen.1 < 1.2, "{seen:?}");
        assert!(seen.0 < -0.5 && seen.1 > 0.5, "uses its range: {seen:?}");
    }

    #[test]
    fn continuous() {
        let p = DVec3::new(1.234_567, 8.9, -3.21);
        let e = DVec3::splat(1e-7);
        assert!((fbm(p, 2, 6, 0.5) - fbm(p + e, 2, 6, 0.5)).abs() < 1e-5);
    }

    #[test]
    fn works_far_from_the_origin() {
        // Planet-scale coordinates in metres / short wavelengths.
        let p = DVec3::new(6.0e5, -3.0e5, 2.5e5);
        let a = gradient(p + DVec3::new(0.25, 0.5, 0.75), 9);
        assert!(a.is_finite() && a != 0.0);
    }

    #[test]
    fn octave_counts() {
        assert_eq!(octaves_between(1000.0, 1000.0), 1);
        assert_eq!(octaves_between(1024.0, 1.0), 11);
        assert_eq!(octaves_between(1.0e6, 10.0), 17);
    }
}
