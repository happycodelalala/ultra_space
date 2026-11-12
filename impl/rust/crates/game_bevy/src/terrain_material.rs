//! Terrain and water materials for close-up tiles (Batch 2, task 2.5).
//!
//! Both use the distant planet shader's lighting formula, so the handover from the shader
//! sphere to tiles shows no brightness change. Uniforms are per body and updated every frame
//! by the terrain module (star and body positions are camera-relative).

use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;

const TERRAIN_SHADER: &str = "embedded://ultra_space/shaders/terrain.wesl";
const WATER_SHADER: &str = "embedded://ultra_space/shaders/water.wesl";

pub struct TerrainMaterialPlugin;

impl Plugin for TerrainMaterialPlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "shaders/terrain.wesl");
        bevy::asset::embedded_asset!(app, "shaders/water.wesl");
        app.add_plugins((
            MaterialPlugin::<TerrainMaterial>::default(),
            MaterialPlugin::<WaterMaterial>::default(),
        ));
    }
}

#[derive(Clone, Copy, Debug, Default, ShaderType)]
pub struct TerrainUniform {
    /// xyz = star position relative to the camera, w = starlight brightness.
    pub star: Vec4,
    /// xyz = body centre relative to the camera, w = radius.
    pub center: Vec4,
    /// Body rotation (quaternion xyzw).
    pub rotation: Vec4,
    /// x = baked map present, y/z = distance range (m) blending vertex colours into the map,
    /// w = distance (m) over which fine detail fades out.
    pub params: Vec4,
    /// Linear rock colour for steep slopes.
    pub rock: Vec4,
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct TerrainMaterial {
    #[uniform(0)]
    pub data: TerrainUniform,
    /// The body's baked colour map (same as the distant planet's).
    #[texture(1, dimension = "cube")]
    #[sampler(2)]
    pub color_map: Option<Handle<Image>>,
}

impl Material for TerrainMaterial {
    fn fragment_shader() -> ShaderRef {
        TERRAIN_SHADER.into()
    }
}

#[derive(Clone, Copy, Debug, Default, ShaderType)]
pub struct WaterUniform {
    /// xyz = star position relative to the camera, w = starlight brightness.
    pub star: Vec4,
    /// xyz = body centre relative to the camera, w = radius.
    pub center: Vec4,
    /// Body rotation (quaternion xyzw).
    pub rotation: Vec4,
    /// rgb = ocean colour (linear), w = time (s) for waves.
    pub color: Vec4,
    /// rgb = sky colour reflected at grazing angles (linear).
    pub sky: Vec4,
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct WaterMaterial {
    #[uniform(0)]
    pub data: WaterUniform,
}

impl Material for WaterMaterial {
    fn fragment_shader() -> ShaderRef {
        WATER_SHADER.into()
    }
}
