//! Renders the star system camera-relative (Batch 1, tasks 1.6–1.8).
//!
//! Every frame, each body's pose is computed in `f64` relative to the camera's anchor frame,
//! the camera's offset is subtracted, and only then converted to `f32` for Bevy. The camera
//! itself stays at the render origin, so nearby things are always precise.
//!
//! Planets use [`PlanetMaterial`]: procedural surface colors and lighting from the star's real
//! position (each planet gets its own correct day side, which a single directional light can't
//! do across a solar system). Atmospheres are additive shells ([`AtmosphereMaterial`]). The star
//! is a bright emissive sphere; bloom produces its glare.

use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;
use glam::{DQuat, DVec3};
use sim_core::{BodyKind, StarSystem};

use crate::camera::SpaceCamera;
use crate::sim::RenderTime;
use crate::universe::{GameState, InGame, Universe};

const PLANET_SHADER: &str = "embedded://ultra_space/shaders/planet.wesl";
const ATMOSPHERE_SHADER: &str = "embedded://ultra_space/shaders/atmosphere.wesl";

/// Brightness of starlight in shader units (lit surfaces ≈ albedo × this).
pub const STAR_LIGHT: f32 = 1.2;
/// Brightness of the star's own surface (HDR, drives bloom).
const STAR_SURFACE: f32 = 40.0;

pub struct SpaceRenderPlugin;

impl Plugin for SpaceRenderPlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "shaders/planet.wesl");
        bevy::asset::embedded_asset!(app, "shaders/atmosphere.wesl");
        app.add_plugins((
            MaterialPlugin::<PlanetMaterial>::default(),
            MaterialPlugin::<AtmosphereMaterial>::default(),
        ))
        .add_systems(OnEnter(GameState::Running), (spawn_camera, spawn_bodies))
        .add_systems(
            Update,
            place_bodies.after(crate::camera::CameraSet).in_set(InGame),
        );
    }
}

#[derive(Clone, Copy, Debug, Default, ShaderType)]
pub struct PlanetUniform {
    pub color_low: Vec4,
    pub color_high: Vec4,
    /// rgb = ocean color, w = ocean coverage.
    pub ocean: Vec4,
    /// rgb = glow color, w = glow strength.
    pub glow: Vec4,
    /// xyz = star position relative to the camera (render space), w = starlight brightness.
    pub star: Vec4,
    /// The body's rotation (quaternion xyzw), to keep surface patterns fixed to the ground.
    pub rotation: Vec4,
    /// x = kind (0 rocky, 1 gas giant, 2 star), y = seed, z = polar ice, w = 1 when baked maps
    /// are present.
    pub params: Vec4,
    /// x = has ocean, y = has atmosphere (biome rules).
    pub surface: Vec4,
    /// Linear biome colours by biome index.
    pub palette: [Vec4; 12],
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct PlanetMaterial {
    #[uniform(0)]
    pub data: PlanetUniform,
    /// Baked surface fields (height, temperature, moisture, continent signal) in the body's
    /// frame; set `params.w` = 1 when present.
    #[texture(1, dimension = "cube")]
    #[sampler(2)]
    pub field_map: Option<Handle<Image>>,
    /// Baked surface normals (body frame, encoded 0–1).
    #[texture(3, dimension = "cube")]
    #[sampler(4)]
    pub normal_map: Option<Handle<Image>>,
}

impl Material for PlanetMaterial {
    // Planets, air and ground don't cast shadows: only vehicles do (an atmosphere or cloud
    // shell would otherwise shade everything inside it).
    fn enable_shadows() -> bool {
        false
    }

    fn fragment_shader() -> ShaderRef {
        PLANET_SHADER.into()
    }
}

#[derive(Clone, Copy, Debug, Default, ShaderType)]
pub struct AtmosphereUniform {
    /// xyz = star position relative to the camera, w = starlight brightness.
    pub star: Vec4,
    /// xyz = planet center relative to the camera, w = planet radius (sea level).
    pub center: Vec4,
    /// x = atmosphere top radius, y = Rayleigh scale height, z = Mie scale height, w = Mie
    /// asymmetry g.
    pub shell: Vec4,
    /// rgb = Rayleigh scattering coefficients at sea level (1/m), w = Mie coefficient.
    pub scatter: Vec4,
    /// Body rotation (quaternion xyzw), so clouds stay fixed to the planet.
    pub rotation: Vec4,
    /// x = cloud deck radius, y = 1 when clouds are on (needs the baked moisture map),
    /// z = time (s) for drift, w = noise seed.
    pub clouds: Vec4,
}

/// Cloud deck altitude as a fraction of the atmosphere's height (≈ 5.6 km on Meridian).
pub const CLOUD_ALTITUDE: f64 = 0.08;

/// Rayleigh scattering of Earth's air in blue light at sea level (1/m).
const EARTH_RAYLEIGH_BLUE: f64 = 33.1e-6;
/// Mie (haze) scattering at sea level for 1 atm (1/m).
const MIE_SCATTERING: f64 = 4.0e-6;
/// Earth's Rayleigh scale height relative to its effective atmosphere height (8 km of 70 km).
const RAYLEIGH_SCALE: f64 = 8.0 / 70.0;
/// Mie scale height relative to the Rayleigh one (1.2 km vs 8 km on Earth).
const MIE_SCALE: f64 = 0.15;
/// Forward scattering of haze (Earth ≈ 0.76).
const MIE_G: f32 = 0.76;

/// Scattering parameters for an atmosphere: `(shell, scatter)` uniforms. The data colour sets
/// the scattering strength per channel as its square (Meridian's (0.45, 0.65, 1.0) gives
/// Earth's Rayleigh ratios); surface pressure scales the amount of air.
pub fn atmosphere_scattering(radius: f64, atm: &sim_core::system::Atmosphere) -> (Vec4, Vec4) {
    let h_r = atm.height * RAYLEIGH_SCALE;
    let p = atm.surface_pressure_atm;
    let beta = |c: f64| (EARTH_RAYLEIGH_BLUE * c * c * p) as f32;
    (
        Vec4::new(
            (radius + atm.height) as f32,
            h_r as f32,
            (h_r * MIE_SCALE) as f32,
            MIE_G,
        ),
        Vec4::new(
            beta(atm.color[0]),
            beta(atm.color[1]),
            beta(atm.color[2]),
            (MIE_SCATTERING * p) as f32,
        ),
    )
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct AtmosphereMaterial {
    #[uniform(0)]
    pub data: AtmosphereUniform,
    /// The body's baked surface fields; clouds follow the moisture channel.
    #[texture(1, dimension = "cube")]
    #[sampler(2)]
    pub field_map: Option<Handle<Image>>,
}

impl Material for AtmosphereMaterial {
    // Planets, air and ground don't cast shadows: only vehicles do (an atmosphere or cloud
    // shell would otherwise shade everything inside it).
    fn enable_shadows() -> bool {
        false
    }

    fn fragment_shader() -> ShaderRef {
        ATMOSPHERE_SHADER.into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Premultiplied
    }

    /// The shell is seen from inside too (the sky from the ground): draw both faces; the shader
    /// keeps the right one. From inside, the visible faces are on the far side of the planet,
    /// behind the terrain, so the depth test is off: the shader stops each ray at the scene
    /// depth itself (depth prepass).
    fn specialize(
        _pipeline: &bevy::pbr::MaterialPipeline,
        descriptor: &mut bevy::render::render_resource::RenderPipelineDescriptor,
        _layout: &bevy::mesh::MeshVertexBufferLayoutRef,
        _key: bevy::pbr::MaterialPipelineKey<Self>,
    ) -> Result<(), bevy::render::render_resource::SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = None;
        if let Some(depth) = descriptor.depth_stencil.as_mut() {
            depth.depth_compare = Some(bevy::render::render_resource::CompareFunction::Always);
            depth.depth_write_enabled = Some(false);
        }
        Ok(())
    }
}

/// Which body an entity draws, and whether it's the solid body or its atmosphere shell.
#[derive(Component)]
pub struct BodyVisual {
    pub body: usize,
    pub layer: Layer,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Layer {
    Surface,
    Atmosphere,
}

/// Marker for the main space camera.
#[derive(Component)]
pub struct MainCamera;

/// The sky's light on ordinary (PBR) meshes: a sky-over-ground gradient for ambient light and
/// reflections, turned to the local up and dimmed at night and in space (`ship::aim_sky`).
pub fn sky_light(images: &mut Assets<Image>) -> bevy::light::EnvironmentMapLight {
    bevy::light::EnvironmentMapLight::hemispherical_gradient(
        images,
        Color::srgb(0.55, 0.65, 0.85),
        Color::srgb(0.7, 0.68, 0.62),
        Color::srgb(0.3, 0.27, 0.24),
    )
}

fn spawn_camera(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    commands.spawn((
        MainCamera,
        sky_light(&mut images),
        // Contact shadows in creases and between parts (needs MSAA off; SMAA smooths edges).
        bevy::pbr::ScreenSpaceAmbientOcclusion::default(),
        Msaa::Off,
        bevy::anti_alias::smaa::Smaa::default(),
        Camera3d::default(),
        Camera {
            clear_color: ClearColorConfig::Custom(Color::BLACK),
            ..default()
        },
        Projection::from(PerspectiveProjection {
            fov: 60f32.to_radians(),
            near: 0.5,
            // Reversed-Z depth makes the far plane irrelevant for precision; it only bounds
            // culling, so it covers the whole system.
            far: 1.0e14,
            ..default()
        }),
        // `ULTRA_SPACE_TONEMAP=agx` tries AgX (the style renders' tonemapper).
        if std::env::var("ULTRA_SPACE_TONEMAP").is_ok_and(|v| v.eq_ignore_ascii_case("agx")) {
            Tonemapping::AgX
        } else {
            Tonemapping::TonyMcMapface
        },
        Bloom::NATURAL,
        // Scene depth for the atmosphere: haze only covers the air in front of surfaces.
        bevy::core_pipeline::prepass::DepthPrepass,
        Transform::IDENTITY,
    ));
}

fn rgb(c: [f64; 3]) -> Vec3 {
    Vec3::new(c[0] as f32, c[1] as f32, c[2] as f32)
}

fn spawn_bodies(
    mut commands: Commands,
    universe: Res<Universe>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut planets: ResMut<Assets<PlanetMaterial>>,
    mut atmospheres: ResMut<Assets<AtmosphereMaterial>>,
) {
    // Fine enough that the horizon looks round from low orbit; close-range terrain with level of
    // detail replaces this in Batch 2.
    let sphere = meshes.add(
        Sphere::new(1.0)
            .mesh()
            .ico(7)
            .expect("valid ico subdivisions"),
    );
    for (i, body) in universe.0.bodies.iter().enumerate() {
        let a = &body.appearance;
        let kind = match body.kind {
            BodyKind::Rocky => 0.0,
            BodyKind::GasGiant => 1.0,
            BodyKind::Star => 2.0,
        };
        let (glow, glow_strength) = match (a.glow_color, body.kind) {
            (Some(c), BodyKind::Star) => (rgb(c), STAR_SURFACE),
            (Some(c), _) => (rgb(c), 3.0),
            (None, _) => (Vec3::ZERO, 0.0),
        };
        let material = planets.add(PlanetMaterial {
            data: PlanetUniform {
                color_low: rgb(a.colors[0]).extend(1.0),
                color_high: rgb(a.colors[1]).extend(1.0),
                ocean: rgb(a.ocean_color).extend(a.ocean_coverage as f32),
                glow: glow.extend(glow_strength),
                star: Vec4::ZERO,
                rotation: Vec4::new(0.0, 0.0, 0.0, 1.0),
                params: Vec4::new(kind, (i as f32) * 17.31 + 3.7, a.polar_ice as f32, 0.0),
                surface: crate::terrain::surface_flags(body),
                palette: crate::terrain::biome_palette(),
            },
            field_map: None,
            normal_map: None,
        });
        commands.spawn((
            Name::new(body.name.clone()),
            BodyVisual {
                body: i,
                layer: Layer::Surface,
            },
            Mesh3d(sphere.clone()),
            MeshMaterial3d(material),
            Transform::IDENTITY,
        ));
        if let Some(atm) = &body.atmosphere {
            let (shell, scatter) = atmosphere_scattering(body.radius, atm);
            let material = atmospheres.add(AtmosphereMaterial {
                data: AtmosphereUniform {
                    star: Vec4::ZERO,
                    center: Vec4::ZERO,
                    shell,
                    scatter,
                    rotation: Vec4::W,
                    clouds: Vec4::new(
                        (body.radius + CLOUD_ALTITUDE * atm.height) as f32,
                        0.0,
                        0.0,
                        (i as f32) * 37.7 + 11.3,
                    ),
                },
                field_map: None,
            });
            commands.spawn((
                Name::new(format!("{} atmosphere", body.name)),
                BodyVisual {
                    body: i,
                    layer: Layer::Atmosphere,
                },
                Mesh3d(sphere.clone()),
                MeshMaterial3d(material),
                Transform::IDENTITY,
            ));
        }
    }
}

/// Converts an `f64` render-space vector to `f32`. Only ever called on values already made
/// relative to the camera.
fn to_render(v: DVec3) -> Vec3 {
    v.as_vec3()
}

fn to_render_rotation(q: DQuat) -> Quat {
    q.as_quat().normalize()
}

/// Position of every body's surface frame relative to the camera, and its rotation, in the
/// camera anchor's (inertial) axes.
pub fn body_poses(system: &StarSystem, camera: &SpaceCamera, t: f64) -> Vec<(DVec3, DQuat)> {
    let anchor = system.bodies[camera.anchor].inertial_frame;
    system
        .bodies
        .iter()
        .map(|b| {
            let s = system.frames.state_in(anchor, b.surface_frame, t);
            (s.position - camera.offset, s.rotation)
        })
        .collect()
}

/// Each body visual with its transform and whichever material it uses.
type VisualQuery = (
    &'static BodyVisual,
    &'static mut Transform,
    AnyOf<(
        &'static MeshMaterial3d<PlanetMaterial>,
        &'static MeshMaterial3d<AtmosphereMaterial>,
    )>,
);

#[allow(clippy::too_many_arguments)]
fn place_bodies(
    universe: Res<Universe>,
    render: Res<RenderTime>,
    camera: Res<SpaceCamera>,
    mut cameras: Query<&mut Transform, (With<MainCamera>, Without<BodyVisual>)>,
    mut visuals: Query<VisualQuery>,
    mut planets: ResMut<Assets<PlanetMaterial>>,
    mut atmospheres: ResMut<Assets<AtmosphereMaterial>>,
    real: Res<Time<Real>>,
) {
    let system = &universe.0;
    let t = render.0;
    let poses = body_poses(system, &camera, t);
    let star_position = to_render(poses[0].0);

    for mut transform in &mut cameras {
        transform.translation = Vec3::ZERO;
        transform.rotation = to_render_rotation(camera.rotation);
    }

    for (visual, mut transform, (planet, atmosphere)) in &mut visuals {
        let body = &system.bodies[visual.body];
        let (position, rotation) = poses[visual.body];
        let radius = match visual.layer {
            Layer::Surface => body.radius,
            Layer::Atmosphere => body.radius + body.atmosphere.as_ref().map_or(0.0, |a| a.height),
        };
        transform.translation = to_render(position);
        transform.rotation = to_render_rotation(rotation);
        transform.scale = Vec3::splat(radius as f32);

        let star = star_position.extend(STAR_LIGHT);
        if let Some(handle) = planet
            && let Some(mut m) = planets.get_mut(&handle.0)
        {
            m.data.star = star;
            let q = transform.rotation;
            m.data.rotation = Vec4::new(q.x, q.y, q.z, q.w);
        }
        if let Some(handle) = atmosphere
            && let Some(mut m) = atmospheres.get_mut(&handle.0)
        {
            m.data.star = star;
            m.data.center = transform.translation.extend(body.radius as f32);
            let q = transform.rotation;
            m.data.rotation = Vec4::new(q.x, q.y, q.z, q.w);
            m.data.clouds.z = real.elapsed_secs_wrapped();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn meridian_air_scatters_blue_most_like_earth() {
        let sys = halcyon();
        let body = sys.body("meridian").unwrap();
        let (shell, scatter) =
            atmosphere_scattering(body.radius, body.atmosphere.as_ref().unwrap());
        assert!(scatter.z > scatter.y && scatter.y > scatter.x, "{scatter}");
        // Close to Earth's ratios (5.8 : 13.5 : 33.1).
        assert!((scatter.x / scatter.z - 5.8 / 33.1).abs() < 0.05);
        assert!((scatter.y / scatter.z - 13.5 / 33.1).abs() < 0.05);
        assert!((shell.y - 8_000.0).abs() < 1.0, "Earth-like scale height");
        let thin = sys.body("hoarfell").unwrap();
        let (_, thin_scatter) =
            atmosphere_scattering(thin.radius, thin.atmosphere.as_ref().unwrap());
        assert!(thin_scatter.x > thin_scatter.z, "dusty red sky");
        assert!(thin_scatter.x < scatter.z, "thin");
    }
    use crate::camera::view_of_body;

    fn halcyon() -> StarSystem {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../content/data/star_systems/halcyon.json");
        StarSystem::from_path(path).unwrap()
    }

    #[test]
    fn camera_relative_positions_are_precise_far_from_the_star() {
        // Camera near Leviathan, ~4.5e10 m from the star: Leviathan's render position must be
        // exact to well under a millimetre, and the f32 conversion must not jitter.
        let sys = halcyon();
        let lev = sys.bodies.iter().position(|b| b.id == "leviathan").unwrap();
        let cam = view_of_body(&sys, lev, 0.0);
        for k in 0..5 {
            let t = 100.0 + k as f64 * 0.016;
            let poses = body_poses(&sys, &cam, t);
            let expected = -cam.offset;
            assert!((poses[lev].0 - expected).length() < 1e-3);
            let f = to_render(poses[lev].0);
            assert!(
                (f.as_dvec3() - expected).length() < 2.0,
                "f32 error under 2 m at 24,000 km"
            );
        }
    }

    #[test]
    fn star_is_far_and_lit_side_faces_it() {
        let sys = halcyon();
        let m = sys.bodies.iter().position(|b| b.id == "meridian").unwrap();
        let cam = view_of_body(&sys, m, 0.0);
        let poses = body_poses(&sys, &cam, 0.0);
        let to_star = poses[0].0 - poses[m].0;
        assert!(to_star.length() > 1.0e10);
        // Camera sits on the lit side of Meridian.
        assert!((-poses[m].0).dot(to_star) > 0.0);
    }
}
