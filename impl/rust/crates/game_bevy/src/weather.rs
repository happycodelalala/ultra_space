//! Weather near the ground (Batch 5, task 5.5; visual-style.md): clear, rain and fog.
//!
//! Rain and fog turn the sky overcast and soften the sun; fog fades the land into a grey
//! within tens of metres, rain within a kilometre, with streaks of rain falling round the
//! camera and the ground wet. A dome of the fog's colour round the camera hides what the
//! distance fog can't reach (the far tiles and the sky have their own shaders), so near and far
//! fade into one colour. `ULTRA_SPACE_WEATHER=clear|rain|fog` sets it at the start; `;` cycles.

use bevy::camera::Exposure;
use bevy::mesh::Mesh;
use bevy::pbr::{DistanceFog, FogFalloff};
use bevy::prelude::*;

use crate::camera::SpaceCamera;
use crate::sim::RenderTime;
use crate::space_render::{MainCamera, STAR_LIGHT};
use crate::universe::{GameState, InGame, Universe};

/// Weather shows below this height above the ground's reference radius (m).
const WEATHER_CEILING: f64 = 3_000.0;
/// Rain: streaks in a box this wide round the camera (m), falling this fast (m/s).
const RAIN_BOX: f32 = 36.0;
const RAIN_SPEED: f32 = 11.0;
const RAIN_STREAKS: usize = 5_000;

#[derive(Resource, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Weather {
    #[default]
    Clear,
    Rain,
    Fog,
}

impl Weather {
    fn from_env() -> Self {
        match std::env::var("ULTRA_SPACE_WEATHER")
            .map(|v| v.to_lowercase())
            .as_deref()
        {
            Ok("rain") => Weather::Rain,
            Ok("fog") => Weather::Fog,
            _ => Weather::Clear,
        }
    }

    fn next(self) -> Self {
        match self {
            Weather::Clear => Weather::Rain,
            Weather::Rain => Weather::Fog,
            Weather::Fog => Weather::Clear,
        }
    }

    /// The sun's light left under the clouds.
    pub fn sun(self) -> f32 {
        match self {
            Weather::Clear => 1.0,
            Weather::Rain => 0.18,
            Weather::Fog => 0.45,
        }
    }

    /// The sky's light left.
    pub fn sky(self) -> f32 {
        match self {
            Weather::Clear => 1.0,
            Weather::Rain => 0.7,
            Weather::Fog => 0.95,
        }
    }

    /// Fog colour (linear, before daylight) and the distance where it closes (m).
    fn fog(self) -> Option<(Vec3, f32)> {
        match self {
            Weather::Clear => None,
            Weather::Rain => Some((Vec3::new(0.40, 0.44, 0.50), 700.0)),
            Weather::Fog => Some((Vec3::new(0.78, 0.76, 0.72), 70.0)),
        }
    }
}

pub struct WeatherPlugin;

impl Plugin for WeatherPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Weather::from_env())
            .add_systems(OnEnter(GameState::Running), spawn_weather)
            .add_systems(
                Update,
                (cycle_weather, apply_weather)
                    .chain()
                    .after(crate::camera::CameraSet)
                    .in_set(InGame),
            );
    }
}

/// Where a weather mesh is and whether it shows.
type Placed = (&'static mut Transform, &'static mut Visibility);

#[derive(Component)]
struct FogDome;

#[derive(Component)]
struct Rain;

#[derive(Resource)]
struct WeatherDraw {
    dome: Handle<StandardMaterial>,
}

fn spawn_weather(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let dome = materials.add(StandardMaterial {
        base_color: Color::BLACK,
        unlit: true,
        cull_mode: None,
        ..default()
    });
    commands.spawn((
        FogDome,
        Mesh3d(
            meshes.add(
                Sphere::new(1.0)
                    .mesh()
                    .ico(4)
                    .unwrap_or_else(|_| Sphere::new(1.0).mesh().uv(32, 16)),
            ),
        ),
        MeshMaterial3d(dome.clone()),
        bevy::light::NotShadowCaster,
        Transform::default(),
        Visibility::Hidden,
    ));
    // Rain: thin streaks scattered in a box, drawn twice stacked so the fall can wrap.
    let mut positions = Vec::with_capacity(RAIN_STREAKS * 4);
    let mut indices = Vec::with_capacity(RAIN_STREAKS * 6);
    let mut seed = 0x9e37_79b9_u32;
    let mut rnd = || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        (seed as f32 / u32::MAX as f32) - 0.5
    };
    for k in 0..RAIN_STREAKS {
        let (x, y, z) = (rnd() * RAIN_BOX, rnd() * RAIN_BOX, rnd() * RAIN_BOX);
        let len = 0.5 + 0.3 * (rnd() + 0.5);
        let w = 0.008;
        let base = (k * 4) as u32;
        positions.extend_from_slice(&[
            [x - w, y, z],
            [x + w, y, z],
            [x + w + 0.05, y - len, z + 0.02],
            [x - w + 0.05, y - len, z + 0.02],
        ]);
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    let normals = vec![[0.0, 0.0, 1.0]; positions.len()];
    let mesh = Mesh::new(
        bevy::mesh::PrimitiveTopology::TriangleList,
        bevy::asset::RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_indices(bevy::mesh::Indices::U32(indices));
    let streak = materials.add(StandardMaterial {
        base_color: Color::srgba(0.8, 0.85, 0.9, 0.35),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        cull_mode: None,
        ..default()
    });
    let mesh = meshes.add(mesh);
    for k in 0..2 {
        commands.spawn((
            Rain,
            Mesh3d(mesh.clone()),
            MeshMaterial3d(streak.clone()),
            bevy::light::NotShadowCaster,
            Transform::default(),
            Visibility::Hidden,
            Name::new(format!("rain {k}")),
        ));
    }
    commands.insert_resource(WeatherDraw { dome });
}

fn cycle_weather(keys: Res<ButtonInput<KeyCode>>, mut weather: ResMut<Weather>) {
    if keys.just_pressed(KeyCode::Semicolon) {
        *weather = weather.next();
        info!("weather: {:?}", *weather);
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_weather(
    mut commands: Commands,
    weather: Res<Weather>,
    draw: Option<Res<WeatherDraw>>,
    universe: Res<Universe>,
    render: Res<RenderTime>,
    camera: Res<SpaceCamera>,
    real: Res<Time<Real>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    cameras: Query<(Entity, Option<&DistanceFog>), With<MainCamera>>,
    mut dome: Query<Placed, (With<FogDome>, Without<Rain>)>,
    mut rain: Query<Placed, (With<Rain>, Without<FogDome>)>,
) {
    let Some(draw) = draw else { return };
    let system = &universe.0;
    let body = &system.bodies[camera.anchor];
    let height = camera.offset.length() - body.radius;
    let near = height < WEATHER_CEILING && body.terrain.is_some();
    // Daylight on the clouds: the sun's height over the camera's ground.
    let star = system.frames.transform_point(
        body.inertial_frame,
        system.star().inertial_frame,
        render.0,
        glam::DVec3::ZERO,
    );
    let up = camera.offset.normalize_or_zero();
    let sun = (star - camera.offset).normalize_or_zero().dot(up) as f32;
    let day = (sun * 4.0 + 0.3).clamp(0.03, 1.0);
    let fog = weather.fog().filter(|_| near);
    for (e, has) in &cameras {
        match fog {
            Some((colour, closes)) => {
                let c = colour * day * STAR_LIGHT * weather.sky();
                commands.entity(e).insert(DistanceFog {
                    color: Color::linear_rgb(c.x, c.y, c.z),
                    directional_light_color: Color::NONE,
                    directional_light_exponent: 8.0,
                    falloff: FogFalloff::from_visibility_colors(
                        closes,
                        Color::linear_rgb(c.x, c.y, c.z),
                        Color::linear_rgb(c.x, c.y, c.z),
                    ),
                });
            }
            None => {
                if has.is_some() {
                    commands.entity(e).remove::<DistanceFog>();
                }
            }
        }
    }
    let up = up.as_vec3();
    for (mut t, mut v) in &mut dome {
        match fog {
            Some((colour, closes)) => {
                *t = Transform::from_scale(Vec3::splat(closes * 1.6));
                *v = Visibility::Visible;
                let c = colour * day * STAR_LIGHT * weather.sky() / Exposure::default().exposure();
                if let Some(mut m) = materials.get_mut(&draw.dome) {
                    let want = LinearRgba::rgb(c.x, c.y, c.z);
                    if m.emissive != want {
                        m.emissive = want;
                    }
                }
            }
            None => *v = Visibility::Hidden,
        }
    }
    let raining = *weather == Weather::Rain && near;
    let fall = (real.elapsed_secs_wrapped() * RAIN_SPEED) % RAIN_BOX;
    for (k, (mut t, mut v)) in rain.iter_mut().enumerate() {
        *v = if raining {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
        if raining {
            let rotation = Quat::from_rotation_arc(Vec3::Y, up);
            t.rotation = rotation;
            t.translation = up * (RAIN_BOX * (k as f32) - fall);
        }
    }
}
