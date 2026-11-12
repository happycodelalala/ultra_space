//! Procedural sky: star field, galactic band and nebulae (Batch 1, task 1.9).
//!
//! Generated once from a seed into an HDR cubemap on a background thread (never blocking a
//! frame), then shown with Bevy's skybox. The sky is fixed to the galaxy: it rotates with the
//! camera but never moves as you travel.

use bevy::asset::RenderAssetUsages;
use bevy::camera::Exposure;
use bevy::light::Skybox;
use bevy::prelude::*;
use bevy::render::render_resource::{
    Extent3d, TextureDimension, TextureFormat, TextureViewDescriptor, TextureViewDimension,
};
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, poll_once};
use glam::DVec3;

use crate::space_render::MainCamera;

/// Cubemap face size in pixels.
const FACE_SIZE: usize = 1024;
const SKY_SEED: u64 = 0x5EED_4A1C_0FF1_CE00;
const STAR_COUNT: usize = 14_000;

pub struct SkyPlugin;

impl Plugin for SkyPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, start_sky_generation)
            .add_systems(Update, attach_sky_when_ready);
    }
}

#[derive(Resource)]
struct SkyTask(Task<Vec<u8>>);

fn start_sky_generation(mut commands: Commands) {
    let task = AsyncComputeTaskPool::get().spawn(async move {
        let start = std::time::Instant::now();
        let bytes = generate_sky(FACE_SIZE, SKY_SEED);
        info!("sky generated in {:.2} s", start.elapsed().as_secs_f64());
        bytes
    });
    commands.insert_resource(SkyTask(task));
}

fn attach_sky_when_ready(
    mut commands: Commands,
    task: Option<ResMut<SkyTask>>,
    mut images: ResMut<Assets<Image>>,
    cameras: Query<Entity, With<MainCamera>>,
) {
    let Some(mut task) = task else { return };
    let Some(bytes) = block_on(poll_once(&mut task.0)) else {
        return;
    };
    commands.remove_resource::<SkyTask>();

    let size = Extent3d {
        width: FACE_SIZE as u32,
        height: FACE_SIZE as u32,
        depth_or_array_layers: 6,
    };
    let mut image = Image::new(
        size,
        TextureDimension::D2,
        bytes,
        TextureFormat::Rgb9e5Ufloat,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_view_descriptor = Some(TextureViewDescriptor {
        dimension: Some(TextureViewDimension::Cube),
        ..default()
    });
    let handle = images.add(image);
    // The skybox is scaled by camera exposure; undo it so sky values share the planets' units.
    let brightness = 1.0 / Exposure::default().exposure();
    for camera in &cameras {
        commands.entity(camera).insert(Skybox {
            image: Some(handle.clone()),
            brightness,
            ..default()
        });
    }
}

// ---------------------------------------------------------------------------------------------
// Generation (pure, deterministic)
// ---------------------------------------------------------------------------------------------

/// Direction for a point on cube face `face` (order +X, −X, +Y, −Y, +Z, −Z) at face
/// coordinates `s`, `t` in [−1, 1] (`t` grows downward), using the standard GPU convention.
pub fn face_direction(face: usize, s: f64, t: f64) -> DVec3 {
    let v = match face {
        0 => DVec3::new(1.0, -t, -s),
        1 => DVec3::new(-1.0, -t, s),
        2 => DVec3::new(s, 1.0, t),
        3 => DVec3::new(s, -1.0, -t),
        4 => DVec3::new(s, -t, 1.0),
        _ => DVec3::new(-s, -t, -1.0),
    };
    v.normalize()
}

/// Inverse of [`face_direction`]: the face and (`s`, `t`) a direction falls on.
pub fn direction_face(d: DVec3) -> (usize, f64, f64) {
    let a = d.abs();
    if a.x >= a.y && a.x >= a.z {
        if d.x > 0.0 {
            (0, -d.z / a.x, -d.y / a.x)
        } else {
            (1, d.z / a.x, -d.y / a.x)
        }
    } else if a.y >= a.z {
        if d.y > 0.0 {
            (2, d.x / a.y, d.z / a.y)
        } else {
            (3, d.x / a.y, -d.z / a.y)
        }
    } else if d.z > 0.0 {
        (4, d.x / a.z, -d.y / a.z)
    } else {
        (5, -d.x / a.z, -d.y / a.z)
    }
}

/// Small deterministic PRNG (SplitMix64).
struct Rng(u64);

impl Rng {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in [0, 1).
    fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    fn unit_vector(&mut self) -> DVec3 {
        let z = self.next_f64() * 2.0 - 1.0;
        let a = self.next_f64() * std::f64::consts::TAU;
        let r = (1.0 - z * z).sqrt();
        DVec3::new(r * a.cos(), z, r * a.sin())
    }
}

fn hash3(p: DVec3, seed: u64) -> f64 {
    let x = (p.x as i64 as u64).wrapping_mul(0x9E37_79B1);
    let y = (p.y as i64 as u64).wrapping_mul(0x85EB_CA77);
    let z = (p.z as i64 as u64).wrapping_mul(0xC2B2_AE3D);
    let mut h = x ^ y.rotate_left(21) ^ z.rotate_left(42) ^ seed;
    h = (h ^ (h >> 33)).wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    h ^= h >> 33;
    (h >> 11) as f64 / (1u64 << 53) as f64
}

fn value_noise(p: DVec3, seed: u64) -> f64 {
    let i = p.floor();
    let f = p - i;
    let u = f * f * (DVec3::splat(3.0) - 2.0 * f);
    let n = |dx: f64, dy: f64, dz: f64| hash3(i + DVec3::new(dx, dy, dz), seed);
    let x00 = n(0.0, 0.0, 0.0) + (n(1.0, 0.0, 0.0) - n(0.0, 0.0, 0.0)) * u.x;
    let x10 = n(0.0, 1.0, 0.0) + (n(1.0, 1.0, 0.0) - n(0.0, 1.0, 0.0)) * u.x;
    let x01 = n(0.0, 0.0, 1.0) + (n(1.0, 0.0, 1.0) - n(0.0, 0.0, 1.0)) * u.x;
    let x11 = n(0.0, 1.0, 1.0) + (n(1.0, 1.0, 1.0) - n(0.0, 1.0, 1.0)) * u.x;
    let y0 = x00 + (x10 - x00) * u.y;
    let y1 = x01 + (x11 - x01) * u.y;
    y0 + (y1 - y0) * u.z
}

fn fbm(p: DVec3, seed: u64, octaves: u32) -> f64 {
    let (mut sum, mut amp, mut x) = (0.0, 0.5, p);
    for o in 0..octaves {
        sum += amp * value_noise(x, seed.wrapping_add(u64::from(o)));
        x = x * 2.03 + DVec3::new(1.7, -2.3, 0.9);
        amp *= 0.5;
    }
    sum
}

/// Normal of the galactic plane, tilted relative to the star system's reference plane.
fn galaxy_normal() -> DVec3 {
    DVec3::new(0.35, 0.82, 0.45).normalize()
}

/// Diffuse light (galactic band and nebulae) in direction `d`, linear RGB.
fn diffuse_sky(d: DVec3, seed: u64) -> DVec3 {
    let height = d.dot(galaxy_normal());
    let band = (-height * height / 0.018).exp();
    let dust = fbm(d * 3.0, seed ^ 0xD057, 5);
    let lanes = 1.0 - 0.7 * smoothstep(0.55, 0.75, dust) * band;
    let galaxy =
        DVec3::new(0.85, 0.82, 0.75) * (0.006 + 0.03 * band * fbm(d * 9.0, seed, 4)) * lanes;

    let cloud = smoothstep(0.52, 0.85, fbm(d * 2.2, seed ^ 0x4EB1, 6));
    let hue = fbm(d * 1.3, seed ^ 0xC010, 3);
    let nebula_color =
        DVec3::new(0.55, 0.25, 0.65).lerp(DVec3::new(0.2, 0.45, 0.7), smoothstep(0.35, 0.65, hue));
    let nebula = nebula_color * cloud * (0.015 + 0.03 * band);
    galaxy + nebula
}

fn smoothstep(e0: f64, e1: f64, x: f64) -> f64 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

const STAR_COLORS: [[f64; 3]; 5] = [
    [0.65, 0.75, 1.0],
    [0.85, 0.9, 1.0],
    [1.0, 1.0, 1.0],
    [1.0, 0.9, 0.7],
    [1.0, 0.7, 0.45],
];

/// Generates the six HDR faces and packs them as `Rgb9e5Ufloat` (4 bytes per texel).
pub fn generate_sky(size: usize, seed: u64) -> Vec<u8> {
    let texel = |i: usize| (i as f64 + 0.5) / size as f64 * 2.0 - 1.0;

    // Diffuse light, one face per thread.
    let mut faces: Vec<Vec<DVec3>> = vec![Vec::new(); 6];
    std::thread::scope(|scope| {
        for (face, out) in faces.iter_mut().enumerate() {
            scope.spawn(move || {
                *out = (0..size * size)
                    .map(|i| {
                        diffuse_sky(face_direction(face, texel(i % size), texel(i / size)), seed)
                    })
                    .collect();
            });
        }
    });

    // Point stars, splatted bilinearly; more of them toward the galactic band.
    let mut rng = Rng(seed);
    let mut placed = 0;
    while placed < STAR_COUNT {
        let d = rng.unit_vector();
        let height = d.dot(galaxy_normal());
        if rng.next_f64() > 0.35 + 0.65 * (-height * height / 0.05).exp() {
            continue;
        }
        placed += 1;
        let u = rng.next_f64().max(1e-6);
        let brightness = (0.04 * u.powf(-1.25)).min(25.0);
        let color = DVec3::from(STAR_COLORS[(rng.next_u64() % 5) as usize]);
        let (face, s, t) = direction_face(d);
        let x = (s + 1.0) * 0.5 * size as f64 - 0.5;
        let y = (t + 1.0) * 0.5 * size as f64 - 0.5;
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = (x - x0, y - y0);
        for (dx, dy, w) in [
            (0, 0, (1.0 - fx) * (1.0 - fy)),
            (1, 0, fx * (1.0 - fy)),
            (0, 1, (1.0 - fx) * fy),
            (1, 1, fx * fy),
        ] {
            let (px, py) = (x0 as i64 + dx, y0 as i64 + dy);
            if (0..size as i64).contains(&px) && (0..size as i64).contains(&py) {
                faces[face][py as usize * size + px as usize] += color * brightness * w;
            }
        }
    }

    faces
        .iter()
        .flatten()
        .flat_map(|c| pack_rgb9e5(c.x as f32, c.y as f32, c.z as f32).to_le_bytes())
        .collect()
}

/// Packs linear RGB into the shared-exponent `Rgb9e5Ufloat` format.
pub fn pack_rgb9e5(r: f32, g: f32, b: f32) -> u32 {
    const MANTISSA_BITS: i32 = 9;
    const BIAS: i32 = 15;
    const MAX_EXP: i32 = 31;
    let max_value =
        ((1 << MANTISSA_BITS) - 1) as f32 / (1 << MANTISSA_BITS) as f32 * 2f32.powi(MAX_EXP - BIAS);
    let (r, g, b) = (
        r.clamp(0.0, max_value),
        g.clamp(0.0, max_value),
        b.clamp(0.0, max_value),
    );
    let max_c = r.max(g).max(b);
    if max_c <= 0.0 {
        return 0;
    }
    let mut exp = (max_c.log2().floor() as i32).max(-BIAS - 1) + 1 + BIAS;
    let mut denom = 2f32.powi(exp - BIAS - MANTISSA_BITS);
    if (max_c / denom + 0.5).floor() as i32 == 1 << MANTISSA_BITS {
        denom *= 2.0;
        exp += 1;
    }
    let m = |c: f32| ((c / denom + 0.5).floor() as u32).min(511);
    m(r) | (m(g) << 9) | (m(b) << 18) | ((exp as u32) << 27)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unpack_rgb9e5(v: u32) -> (f32, f32, f32) {
        let exp = (v >> 27) as i32 - 15 - 9;
        let scale = 2f32.powi(exp);
        (
            (v & 511) as f32 * scale,
            ((v >> 9) & 511) as f32 * scale,
            ((v >> 18) & 511) as f32 * scale,
        )
    }

    #[test]
    fn rgb9e5_round_trips_hdr_values() {
        for &(r, g, b) in &[
            (0.0, 0.0, 0.0),
            (1.0, 0.5, 0.25),
            (20.0, 3.0, 0.01),
            (0.004, 0.006, 0.008),
        ] {
            let (r2, g2, b2) = unpack_rgb9e5(pack_rgb9e5(r, g, b));
            let max = r.max(g).max(b);
            for (a, b) in [(r, r2), (g, g2), (b, b2)] {
                assert!((a - b).abs() <= max / 256.0 + 1e-9, "{a} vs {b}");
            }
        }
    }

    #[test]
    fn face_mapping_round_trips() {
        let mut rng = Rng(7);
        for _ in 0..10_000 {
            let d = rng.unit_vector();
            let (face, s, t) = direction_face(d);
            assert!((-1.0..=1.0).contains(&s) && (-1.0..=1.0).contains(&t));
            assert!((face_direction(face, s, t) - d).length() < 1e-9);
        }
    }

    #[test]
    fn faces_meet_without_gaps() {
        // Directions just either side of a cube edge are close together.
        let a = face_direction(0, 0.9999, 0.3);
        let b = face_direction(5, -0.9999, 0.3);
        assert!(
            a.angle_between(b) < 1e-3,
            "+X right edge meets -Z left edge"
        );
        let c = face_direction(2, 0.2, 0.9999);
        let e = face_direction(4, 0.2, -0.9999);
        assert!(
            c.angle_between(e) < 1e-3,
            "+Y bottom edge meets +Z top edge"
        );
    }

    #[test]
    fn generation_is_deterministic_and_sized() {
        let a = generate_sky(16, 42);
        let b = generate_sky(16, 42);
        assert_eq!(a.len(), 16 * 16 * 6 * 4);
        assert_eq!(a, b);
        assert_ne!(a, generate_sky(16, 43));
    }
}
