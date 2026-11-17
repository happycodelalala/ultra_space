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
const TERRAIN_PREPASS_SHADER: &str = "embedded://ultra_space/shaders/terrain_prepass.wesl";

pub struct TerrainMaterialPlugin;

impl Plugin for TerrainMaterialPlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "shaders/terrain.wesl");
        bevy::asset::embedded_asset!(app, "shaders/water.wesl");
        bevy::asset::embedded_asset!(app, "shaders/terrain_prepass.wesl");
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
    /// x = baked map present, y/z = distance range (m) blending vertex fields into the map,
    /// w = unused (detail now fades by pixel footprint in the shader).
    pub params: Vec4,
    /// x = has ocean, y = has atmosphere (biome rules).
    pub surface: Vec4,
    /// Linear biome colours by biome index.
    pub palette: [Vec4; 12],
    /// The block ground (Batch 5): xyz = its centre (unit, body frame), w = cosine of its
    /// angular radius.
    pub blocks: Vec4,
    /// x = round the camera, tiles within this horizontal distance (m) inside the block ground
    /// are not drawn (the blocks are); 0 = off.
    pub hide: Vec4,
    /// A body of plains: each plain's plane (body frame: xyz normal, w distance), so slopes are
    /// judged against the plain's "up"; `plain_count.x` of them (0: a sphere).
    pub planes: [Vec4; crate::space_render::MAX_DECK_PLANES],
    pub plain_count: Vec4,
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct TerrainMaterial {
    #[uniform(0)]
    pub data: TerrainUniform,
    /// The body's baked field map (same as the distant planet's).
    #[texture(1, dimension = "cube")]
    #[sampler(2)]
    pub field_map: Option<Handle<Image>>,
}

impl Material for TerrainMaterial {
    // Planets, air and ground don't cast shadows: only vehicles do (an atmosphere or cloud
    // shell would otherwise shade everything inside it).
    fn enable_shadows() -> bool {
        false
    }

    fn fragment_shader() -> ShaderRef {
        TERRAIN_SHADER.into()
    }

    // Tiles over the block ground are discarded, in the depth prepass too (a mask, so the prepass
    // runs this fragment shader).
    fn prepass_fragment_shader() -> ShaderRef {
        TERRAIN_PREPASS_SHADER.into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Mask(0.5)
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
    // Planets, air and ground don't cast shadows: only vehicles do (an atmosphere or cloud
    // shell would otherwise shade everything inside it).
    fn enable_shadows() -> bool {
        false
    }

    fn fragment_shader() -> ShaderRef {
        WATER_SHADER.into()
    }
}
