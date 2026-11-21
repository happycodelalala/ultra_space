//! Wind lines (task 5.7e E2): thin white wisps blowing past near the ground, the way the wind
//! is shown in stylised games. A few at a time round the camera, more where the planet's wind
//! is strong; each drifts downwind at the speed of the gusts that sway the cover, weaving a
//! little and now and then curling once in a loop, its trail fading toward the tail, and the
//! whole wisp fading in and out over a few seconds. Lit by daylight (gone at night).
//! `ULTRA_SPACE_WIND_LINES=0` turns them off.

use std::collections::VecDeque;

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use glam::DVec3;

use crate::blocks::BlockWorld;
use crate::camera::SpaceCamera;
use crate::sim::RenderTime;
use crate::universe::{GameState, InGame, Universe};

/// Most wisps at once (in the strongest wind).
const MAX_LINES: usize = 14;
/// Wisps show while the camera is this close to the ground (m).
const CEILING: f64 = 120.0;
/// How long a wisp's trail is (s of its path).
const TRAIL_SECONDS: f64 = 1.1;
/// Ribbon half width (m).
const HALF_WIDTH: f32 = 0.09;

pub struct WindLinesPlugin;

impl Plugin for WindLinesPlugin {
    fn build(&self, app: &mut App) {
        if std::env::var("ULTRA_SPACE_WIND_LINES").as_deref() == Ok("0") {
            return;
        }
        app.add_systems(OnEnter(GameState::Running), spawn_lines)
            .add_systems(
                Update,
                blow_lines.after(crate::camera::CameraSet).in_set(InGame),
            );
    }
}

/// One wisp: its head and trail in the body's surface frame.
struct Line {
    entity: Entity,
    mesh: Handle<Mesh>,
    alive: bool,
    head: DVec3,
    /// Height kept above the ground (m).
    lift: f64,
    trail: VecDeque<(f64, DVec3)>,
    age: f64,
    life: f64,
    /// Weave: phase and rate; the loop: when it starts (s of age, or past the life for none).
    phase: f64,
    rate: f64,
    curl_at: f64,
}

#[derive(Resource)]
struct WindLines {
    lines: Vec<Line>,
    material: Handle<StandardMaterial>,
    rng: u64,
    /// Seconds until the next wisp may start.
    wait: f64,
}

impl WindLines {
    fn unit(&mut self) -> f64 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng >> 11) as f64 / (1u64 << 53) as f64
    }
}

fn empty_mesh() -> Mesh {
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, Vec::<[f32; 3]>::new())
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, Vec::<[f32; 3]>::new())
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, Vec::<[f32; 4]>::new())
    .with_inserted_indices(Indices::U32(Vec::new()))
}

fn spawn_lines(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let material = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        cull_mode: None,
        ..default()
    });
    let lines = (0..MAX_LINES)
        .map(|k| {
            let mesh = meshes.add(empty_mesh());
            let entity = commands
                .spawn((
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(material.clone()),
                    bevy::light::NotShadowCaster,
                    NoFrustumCulling,
                    Transform::default(),
                    Visibility::Hidden,
                    Name::new(format!("wind line {k}")),
                ))
                .id();
            Line {
                entity,
                mesh,
                alive: false,
                head: DVec3::ZERO,
                lift: 0.0,
                trail: VecDeque::new(),
                age: 0.0,
                life: 0.0,
                phase: 0.0,
                rate: 0.0,
                curl_at: 0.0,
            }
        })
        .collect();
    commands.insert_resource(WindLines {
        lines,
        material,
        rng: 0x2545_F491_4F6C_DD1D,
        wait: 0.0,
    });
}

#[allow(clippy::too_many_arguments)]
fn blow_lines(
    lines: Option<ResMut<WindLines>>,
    world: Option<Res<BlockWorld>>,
    universe: Res<Universe>,
    render: Res<RenderTime>,
    camera: Res<SpaceCamera>,
    real: Res<Time<Real>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut visibility: Query<&mut Visibility>,
    eyes: Query<&GlobalTransform, With<crate::space_render::MainCamera>>,
) {
    let (Some(mut lines), Some(world)) = (lines, world) else {
        return;
    };
    let lines = &mut *lines;
    let dt = f64::from(real.delta_secs()).min(0.1);
    let system = &universe.0;
    let body = &system.bodies[world.body];
    let ground = &world.ground;
    let cam = crate::settlements::camera_on(&universe, &camera, world.body, render.0);
    let cam_dir = cam.normalize_or(DVec3::Y);
    let ground_at = |p: DVec3| ground.surface(p.normalize_or(DVec3::Y));
    let near = camera.anchor == world.body
        && ground_at(cam).is_some_and(|h| cam.length() - ground.radius - h < CEILING);
    let wind = ground.globe().map_or(DVec3::ZERO, |g| g.wind(cam_dir));
    let strength = 0.35 + 0.65 * wind.length().min(1.0);
    let flow = wind.normalize_or(cam_dir.any_orthonormal_vector());
    let speed = 6.0 * strength;
    // Daylight: the star's height over the camera's ground (wisps fade out at dusk).
    let star = system.frames.transform_point(
        body.surface_frame,
        system.star().inertial_frame,
        render.0,
        DVec3::ZERO,
    );
    let sun = (star - cam).normalize_or_zero().dot(cam_dir);
    let day = ((sun * 5.0 + 0.2).clamp(0.0, 1.0)) as f32;
    if let Some(mut m) = materials.get_mut(&lines.material) {
        let want = Color::linear_rgba(1.9 * day, 1.95 * day, 2.0 * day, 1.0);
        if m.base_color != want {
            m.base_color = want;
        }
    }
    // The render frame: surface frame to camera-relative.
    let anchor = system.bodies[camera.anchor].inertial_frame;
    let s = system.frames.state_in(anchor, body.surface_frame, render.0);
    // Start a wisp now and then, more in a strong wind.
    lines.wait -= dt;
    let want = (MAX_LINES as f64 * strength).round() as usize;
    let alive = lines.lines.iter().filter(|l| l.alive).count();
    if near && day > 0.05 && lines.wait <= 0.0 && alive < want {
        // In front of the camera (8–32 m, across most of the view), a little upwind so it
        // blows through the view.
        let look = eyes
            .iter()
            .next()
            .map(|g| s.rotation.inverse() * g.forward().as_dvec3())
            .unwrap_or(flow);
        let look = (look - cam_dir * look.dot(cam_dir)).normalize_or(flow);
        let side = cam_dir.cross(look);
        let d = 8.0 + 24.0 * lines.unit().sqrt();
        let across = (lines.unit() * 2.0 - 1.0) * d * 0.7;
        let start = cam + look * d + side * across - flow * (speed * 1.2);
        let lift = 0.8 + 4.5 * lines.unit() * lines.unit();
        let (life, phase, rate) = (
            2.5 + 2.0 * lines.unit(),
            lines.unit() * std::f64::consts::TAU,
            1.2 + 1.3 * lines.unit(),
        );
        let curl_at = if lines.unit() < 0.4 {
            0.6 + lines.unit() * 1.4
        } else {
            f64::MAX
        };
        if let Some(h) = ground_at(start) {
            let head = start.normalize() * (ground.radius + h + lift);
            if let Some(l) = lines.lines.iter_mut().find(|l| !l.alive) {
                *l = Line {
                    entity: l.entity,
                    mesh: l.mesh.clone(),
                    alive: true,
                    head,
                    lift,
                    trail: VecDeque::from([(0.0, head)]),
                    age: 0.0,
                    life,
                    phase,
                    rate,
                    curl_at,
                };
            }
        }
        lines.wait = 0.25 + 0.6 * lines.unit() / strength;
    }
    let to_render = |p: DVec3| (s.position + s.rotation * p - camera.offset).as_vec3();
    let eye = to_render(cam);
    for l in &mut lines.lines {
        if !l.alive {
            continue;
        }
        l.age += dt;
        if l.age > l.life || !near {
            l.alive = false;
            if let Ok(mut v) = visibility.get_mut(l.entity) {
                *v = Visibility::Hidden;
            }
            continue;
        }
        // Move: downwind, weaving side to side and up and down; one loop if it curls.
        let up = l.head.normalize();
        let side = up.cross(flow).normalize_or_zero();
        let t = l.age;
        let mut v = flow * speed
            + side * (0.9 * (l.rate * t + l.phase).sin())
            + up * (0.35 * (1.7 * l.rate * t + 2.0 * l.phase).cos());
        let curl = 1.1;
        if t > l.curl_at && t < l.curl_at + curl {
            // A loop in the vertical plane along the wind: it rises, rolls back over the top
            // (faster than it drifts) and comes round under, ending where it would have been.
            let w = std::f64::consts::TAU / curl;
            let r = speed * 1.4 / w;
            let a = (t - l.curl_at) * w;
            v += (flow * a.cos() + up * a.sin()) * (r * w);
        }
        let mut head = l.head + v * dt;
        // Keep its height over the ground (eased, so it rides over bumps smoothly).
        if let Some(h) = ground_at(head) {
            let r = head.length();
            let target = ground.radius + h + l.lift;
            head = head / r * (r + (target - r) * (dt * 2.0).min(1.0));
        }
        l.head = head;
        l.trail.push_back((t, head));
        while l.trail.front().is_some_and(|&(a, _)| t - a > TRAIL_SECONDS) {
            l.trail.pop_front();
        }
        // The ribbon, facing the camera, thin and clear at both ends.
        let fade = (t / 0.5).min(1.0).min((l.life - t) / 0.9).clamp(0.0, 1.0) as f32;
        let n = l.trail.len();
        let mut positions = Vec::with_capacity(n * 2);
        let mut colours = Vec::with_capacity(n * 2);
        let mut indices = Vec::with_capacity(n * 6);
        let points: Vec<Vec3> = l.trail.iter().map(|&(_, p)| to_render(p)).collect();
        for (k, &p) in points.iter().enumerate() {
            let next = points[(k + 1).min(n - 1)];
            let prev = points[k.saturating_sub(1)];
            let tangent = (next - prev).normalize_or_zero();
            let w = tangent.cross(eye - p).normalize_or_zero();
            let along = k as f32 / (n.max(2) - 1) as f32;
            // Tail (oldest) to head: thin and clear at the tail, fullest just behind the head.
            let shape = (along * 1.5).min(1.0) * ((1.0 - along) * 6.0).min(1.0);
            // About the same width on screen near and far (a near one isn't a broad band).
            let half = HALF_WIDTH.min(p.distance(eye) * 0.003) * (0.3 + 0.7 * shape);
            positions.push((p - w * half).to_array());
            positions.push((p + w * half).to_array());
            let a = 0.8 * fade * shape;
            colours.push([1.0, 1.0, 1.0, a]);
            colours.push([1.0, 1.0, 1.0, a]);
            if k + 1 < n {
                let b = (k * 2) as u32;
                indices.extend_from_slice(&[b, b + 1, b + 2, b + 1, b + 3, b + 2]);
            }
        }
        if let Some(mut mesh) = meshes.get_mut(&l.mesh) {
            let normals = vec![[0.0, 1.0, 0.0]; positions.len()];
            mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
            mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
            mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colours);
            mesh.insert_indices(Indices::U32(indices));
        }
        if let Ok(mut vis) = visibility.get_mut(l.entity)
            && *vis != Visibility::Visible
        {
            *vis = Visibility::Visible;
        }
    }
}
