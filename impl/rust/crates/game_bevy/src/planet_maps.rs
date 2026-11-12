//! Planet maps for the orbit view (Batch 2, task 2.4; design: planet-surface.md §1, §3).
//!
//! The distant planet shader must show the same terrain as the close-up tiles, so each body
//! with terrain gets two cube maps baked from the same terrain and climate function on worker
//! threads at startup:
//! - colour: the biome palette shared with the tiles (alpha marks water, for highlights);
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
use sim_planet::mesh::terrain_normal;
use sim_planet::{Climate, Terrain, subsolar_latitude};

use crate::sky::face_direction;
use crate::space_render::{BodyVisual, Layer, PlanetMaterial};
use crate::terrain::biome_srgb;
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

/// Baked colour maps by body index, for other materials (terrain tiles) to share.
#[derive(Resource, Default)]
pub struct BakedMapHandles {
    pub color: std::collections::HashMap<usize, Handle<Image>>,
}

/// Baked maps for one body: RGBA8 sRGB colour and RGBA8 normal, six faces each.
pub struct BakedMaps {
    pub body: usize,
    pub color: Vec<u8>,
    pub normal: Vec<u8>,
}

#[derive(Resource)]
struct BakeTasks(Vec<Task<BakedMaps>>);

/// Bakes the colour and normal cube maps of a body (pure; runs on a worker).
pub fn bake(
    body: usize,
    terrain: &Terrain,
    climate: Option<&Climate>,
    season: f64,
    size: usize,
) -> BakedMaps {
    let texel = std::f64::consts::FRAC_PI_2 * terrain.radius / size as f64;
    let mut color = Vec::with_capacity(size * size * 6 * 4);
    let mut normal = Vec::with_capacity(size * size * 6 * 4);
    for face in 0..6 {
        for y in 0..size {
            for x in 0..size {
                let s = (x as f64 + 0.5) / size as f64 * 2.0 - 1.0;
                let t = (y as f64 + 0.5) / size as f64 * 2.0 - 1.0;
                let d = face_direction(face, s, t);
                let sample = terrain.sample(d, texel);
                let n = terrain_normal(terrain, d, texel, texel);
                let biome = climate.map_or(sim_planet::Biome::Regolith, |c| {
                    let temp = c.temperature_at(terrain, d, sample.height, season);
                    let moist = c.moisture(terrain, d, &sample);
                    let slope = n.dot(d).clamp(-1.0, 1.0).acos();
                    c.biome(terrain, &sample, temp, moist, slope)
                });
                let water = terrain.has_ocean() && sample.height < 0.0;
                let [r, g, b] = biome_srgb(biome);
                let byte = |c: f32| (c.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                color.extend_from_slice(&[byte(r), byte(g), byte(b), if water { 255 } else { 0 }]);
                let enc = |c: f64| ((c * 0.5 + 0.5).clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                normal.extend_from_slice(&[enc(n.x), enc(n.y), enc(n.z), 255]);
            }
        }
    }
    BakedMaps {
        body,
        color,
        normal,
    }
}

fn start_baking(mut commands: Commands, universe: Res<Universe>) {
    let system = &universe.0;
    let pool = crate::workers::workers();
    let tasks = system
        .bodies
        .iter()
        .enumerate()
        .filter_map(|(i, body)| {
            let terrain = Terrain::of(body)?;
            let climate = Climate::of(body);
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
    visuals: Query<(&BodyVisual, &MeshMaterial3d<PlanetMaterial>)>,
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
        let color = images.add(cube_image(maps.color, TextureFormat::Rgba8UnormSrgb));
        let normal = images.add(cube_image(maps.normal, TextureFormat::Rgba8Unorm));
        handles.color.insert(maps.body, color.clone());
        for (visual, handle) in &visuals {
            if visual.body == maps.body
                && visual.layer == Layer::Surface
                && let Some(mut m) = materials.get_mut(&handle.0)
            {
                m.color_map = Some(color.clone());
                m.normal_map = Some(normal.clone());
                m.data.params.w = 1.0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use sim_core::StarSystem;

    use super::*;

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
        assert_eq!(maps.color.len(), size * size * 6 * 4);
        assert_eq!(maps.normal.len(), maps.color.len());
        // The water mask covers about the ocean fraction (cube texels aren't equal-area, so
        // allow some slack).
        let wet = maps.color.chunks(4).filter(|c| c[3] == 255).count();
        let fraction = wet as f64 / (size * size * 6) as f64;
        assert!((fraction - 0.62).abs() < 0.08, "water {fraction:.2}");
        // A texel's water flag agrees with the terrain at its direction.
        let (face, x, y) = (4, 10, 20);
        let s = (x as f64 + 0.5) / size as f64 * 2.0 - 1.0;
        let t = (y as f64 + 0.5) / size as f64 * 2.0 - 1.0;
        let d = face_direction(face, s, t);
        let texel = ((face * size + y) * size + x) * 4;
        let texel_size = std::f64::consts::FRAC_PI_2 * terrain.radius / size as f64;
        let wet_here = terrain.sample(d, texel_size).height < 0.0;
        assert_eq!(maps.color[texel + 3] == 255, wet_here);
        // Normals point roughly outward.
        let n = &maps.normal[texel..texel + 3];
        let n = glam::DVec3::new(n[0] as f64, n[1] as f64, n[2] as f64) / 127.5 - 1.0;
        assert!(n.normalize().dot(d) > 0.5);
    }
}
