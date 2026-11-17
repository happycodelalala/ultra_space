//! Planet maps for the orbit view (Batch 2, task 2.4; design: planet-surface.md §1, §3).
//!
//! The distant planet shader must show the same terrain as the close-up tiles, so each body
//! with terrain gets two cube maps baked from the same terrain and climate function on worker
//! threads at startup:
//! - fields: height, temperature, moisture and continent signal, from which shaders decide
//!   biomes per pixel with the same rules as the tiles;
//! - normals: surface normals in the body's frame, so mountains catch the light from orbit.
//!
//! The terrain is a function of direction, so the maps are baked directly in the GPU's cube
//! map layout (`sky::face_direction`), with no projection mismatch.

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{
    Extent3d, TextureDimension, TextureFormat, TextureViewDescriptor, TextureViewDimension,
};
use bevy::tasks::{Task, block_on, poll_once};
use sim_planet::mesh::{NO_CLIMATE_TEMPERATURE, terrain_normal};
use sim_planet::{Climate, Terrain, subsolar_latitude};

use crate::sky::face_direction;
use crate::space_render::{AtmosphereMaterial, BodyVisual, Layer, PlanetMaterial};
use crate::universe::{GameState, InGame, Universe};

/// Texels per cube face edge.
pub const MAP_SIZE: usize = 256;

pub struct PlanetMapsPlugin;

impl Plugin for PlanetMapsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BakedMapHandles>()
            .add_systems(OnEnter(GameState::Running), start_baking)
            .add_systems(Update, attach_maps.in_set(InGame));
    }
}

/// Baked field maps by body index, for other materials (terrain tiles) to share.
#[derive(Resource, Default)]
pub struct BakedMapHandles {
    pub fields: std::collections::HashMap<usize, Handle<Image>>,
}

/// Baked maps for one body: RGBA16F surface fields and RGBA8 normals, six faces each.
pub struct BakedMaps {
    pub body: usize,
    pub fields: Vec<u8>,
    pub normal: Vec<u8>,
}

#[derive(Resource)]
struct BakeTasks(Vec<Task<BakedMaps>>);

/// Bakes the field and normal cube maps of a body (pure; runs on a worker). Fields are
/// RGBA16F: height (m), temperature (°C), moisture, continent signal; shaders decide biomes per
/// pixel from them with the same rules as the tiles.
pub fn bake(
    body: usize,
    terrain: &Terrain,
    climate: Option<&Climate>,
    season: f64,
    size: usize,
) -> BakedMaps {
    let texel = std::f64::consts::FRAC_PI_2 * terrain.radius / size as f64;
    let mut fields = Vec::with_capacity(size * size * 6 * 8);
    let mut normal = Vec::with_capacity(size * size * 6 * 4);
    for face in 0..6 {
        for y in 0..size {
            for x in 0..size {
                let s = (x as f64 + 0.5) / size as f64 * 2.0 - 1.0;
                let t = (y as f64 + 0.5) / size as f64 * 2.0 - 1.0;
                let d = face_direction(face, s, t);
                let sample = terrain.sample(d, texel);
                let n = terrain_normal(terrain, d, texel, texel);
                let (temp, moist) = climate.map_or((NO_CLIMATE_TEMPERATURE, 0.0), |c| {
                    (
                        c.temperature_at(terrain, d, sample.elevation, season),
                        c.moisture(terrain, d, &sample),
                    )
                });
                for v in [sample.elevation, temp, moist, sample.land] {
                    fields.extend_from_slice(&f16_bits(v as f32).to_le_bytes());
                }
                let enc = |c: f64| ((c * 0.5 + 0.5).clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                normal.extend_from_slice(&[enc(n.x), enc(n.y), enc(n.z), 255]);
            }
        }
    }
    BakedMaps {
        body,
        fields,
        normal,
    }
}

/// IEEE 754 half-precision bits of `x` (round to nearest; out-of-range values saturate to
/// infinity, tiny values flush to zero; enough for terrain fields).
pub fn f16_bits(x: f32) -> u16 {
    let bits = x.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exponent = ((bits >> 23) & 0xff) as i32 - 127 + 15;
    let mantissa = bits & 0x007f_ffff;
    if x.is_nan() {
        return sign | 0x7e00;
    }
    if exponent >= 0x1f {
        return sign | 0x7c00;
    }
    if exponent <= 0 {
        return sign;
    }
    // Round the 23-bit mantissa to 10 bits (a carry correctly bumps the exponent).
    let rounded = ((exponent as u32) << 10 | (mantissa >> 13)) + ((mantissa >> 12) & 1);
    sign | rounded.min(0x7c00) as u16
}

fn start_baking(mut commands: Commands, universe: Res<Universe>, sim: Res<crate::sim::Sim>) {
    let system = &universe.0;
    let pool = crate::workers::workers();
    let tasks = system
        .bodies
        .iter()
        .enumerate()
        .filter_map(|(i, body)| {
            let terrain = Terrain::of(body)?;
            let climate = crate::settlements::climate_of(&sim.0, i, body);
            let season = subsolar_latitude(system, i, 0.0);
            Some(pool.spawn(async move {
                let start = std::time::Instant::now();
                let maps = bake(i, &terrain, climate.as_ref(), season, MAP_SIZE);
                debug!(
                    "baked planet maps for body {i} in {:.2} s",
                    start.elapsed().as_secs_f64()
                );
                maps
            }))
        })
        .collect();
    commands.insert_resource(BakeTasks(tasks));
}

fn cube_image(bytes: Vec<u8>, format: TextureFormat) -> Image {
    let size = Extent3d {
        width: MAP_SIZE as u32,
        height: MAP_SIZE as u32,
        depth_or_array_layers: 6,
    };
    let mut image = Image::new(
        size,
        TextureDimension::D2,
        bytes,
        format,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_view_descriptor = Some(TextureViewDescriptor {
        dimension: Some(TextureViewDimension::Cube),
        ..default()
    });
    image
}

fn attach_maps(
    mut handles: ResMut<BakedMapHandles>,
    mut tasks: Option<ResMut<BakeTasks>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<PlanetMaterial>>,
    mut atmospheres: ResMut<Assets<AtmosphereMaterial>>,
    visuals: Query<(&BodyVisual, &MeshMaterial3d<PlanetMaterial>)>,
    atmosphere_visuals: Query<(&BodyVisual, &MeshMaterial3d<AtmosphereMaterial>)>,
) {
    let Some(tasks) = tasks.as_mut() else { return };
    let mut i = 0;
    while i < tasks.0.len() {
        let Some(maps) = block_on(poll_once(&mut tasks.0[i])) else {
            i += 1;
            continue;
        };
        drop(tasks.0.swap_remove(i));
        info!("planet maps: attaching maps for body {}", maps.body);
        let fields = images.add(cube_image(maps.fields, TextureFormat::Rgba16Float));
        let normal = images.add(cube_image(maps.normal, TextureFormat::Rgba8Unorm));
        handles.fields.insert(maps.body, fields.clone());
        for (visual, handle) in &visuals {
            if visual.body == maps.body
                && visual.layer == Layer::Surface
                && let Some(mut m) = materials.get_mut(&handle.0)
            {
                m.field_map = Some(fields.clone());
                m.normal_map = Some(normal.clone());
                m.data.params.w = 1.0;
            }
        }
        // Clouds follow the moisture field.
        for (visual, handle) in &atmosphere_visuals {
            if visual.body == maps.body
                && let Some(mut m) = atmospheres.get_mut(&handle.0)
            {
                m.field_map = Some(fields.clone());
                // A body of plains has cotton clouds (clouds.rs), not the round deck.
                if m.data.deck.x < 0.5 {
                    m.data.clouds.y = 1.0;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use sim_core::StarSystem;

    use super::*;

    fn half(bytes: &[u8]) -> f32 {
        // Decode IEEE half bits (normal numbers only; enough for the test).
        let h = u16::from_le_bytes([bytes[0], bytes[1]]);
        let sign = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
        let exp = i32::from((h >> 10) & 0x1f);
        let man = f32::from(h & 0x3ff);
        if exp == 0 {
            return 0.0;
        }
        sign * (1.0 + man / 1024.0) * 2f32.powi(exp - 15)
    }

    #[test]
    fn half_floats_round_trip() {
        for x in [
            0.0f32, 1.0, -2.5, 0.62, 25.0, -150.0, 6_000.0, -5_123.0, 350.0,
        ] {
            let y = half(&f16_bits(x).to_le_bytes());
            assert!((y - x).abs() <= x.abs() * 1e-3 + 1e-3, "{x} -> {y}");
        }
        assert_eq!(f16_bits(1e9), 0x7c00, "overflow saturates to infinity");
    }

    #[test]
    fn baked_maps_match_the_terrain() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../content/data/star_systems/halcyon.json");
        let sys = StarSystem::from_path(path).unwrap();
        let body = sys.body("meridian").unwrap();
        let terrain = Terrain::of(body).unwrap();
        let climate = Climate::of(body).unwrap();
        let size = 32;
        let maps = bake(2, &terrain, Some(&climate), 0.0, size);
        assert_eq!(maps.fields.len(), size * size * 6 * 8);
        assert_eq!(maps.normal.len(), size * size * 6 * 4);
        // Below-sea-level texels cover about the ocean fraction (cube texels aren't
        // equal-area, so allow some slack).
        let wet = maps
            .fields
            .chunks(8)
            .filter(|c| half(&c[0..2]) < 0.0)
            .count();
        let fraction = wet as f64 / (size * size * 6) as f64;
        assert!((fraction - 0.62).abs() < 0.08, "water {fraction:.2}");
        // A texel's height agrees with the terrain at its direction.
        let (face, x, y) = (4, 10, 20);
        let s = (x as f64 + 0.5) / size as f64 * 2.0 - 1.0;
        let t = (y as f64 + 0.5) / size as f64 * 2.0 - 1.0;
        let d = face_direction(face, s, t);
        let texel = (face * size + y) * size + x;
        let texel_size = std::f64::consts::FRAC_PI_2 * terrain.radius / size as f64;
        let h = terrain.sample(d, texel_size).height;
        let baked = f64::from(half(&maps.fields[texel * 8..texel * 8 + 2]));
        assert!((baked - h).abs() < 8.0, "baked {baked} vs {h}");
        // Normals point roughly outward.
        let n = &maps.normal[texel * 4..texel * 4 + 3];
        let n = glam::DVec3::new(n[0] as f64, n[1] as f64, n[2] as f64) / 127.5 - 1.0;
        assert!(n.normalize().dot(d) > 0.5);
    }
}
