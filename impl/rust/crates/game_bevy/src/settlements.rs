//! Settlements on screen (task 4.6; design: living-planet.md §5).
//!
//! Every settlement of the body the camera is at has a root entity in its own tangent frame
//! (x east, y up, z south at its centre), placed camera-relative every frame, with:
//!
//! - **near** (the camera within about a kilometre of its edge): one entity per building with
//!   its kit model; models share one vertex-coloured material, so the same building drawn many
//!   times batches into instanced draws;
//! - **far** (out to 250 km): one merged mesh of boxes, each building's footprint and height in
//!   its roof and wall colours, so a settlement never pops into existence on approach; the boxes
//!   reach a few metres into the ground so the coarser distant terrain doesn't show gaps;
//! - **lights**: small glowing lanterns at every door and lamps along the colony's streets, lit
//!   by night (the sun's height at the settlement) and drawn at every distance.
//!
//! Models are the kits' glTF binaries; their colours (a palette texture or plain material
//! colours) are baked into vertex colours when loaded.

use std::collections::HashMap;
use std::path::Path;

use bevy::asset::RenderAssetUsages;
use bevy::camera::Exposure;
use bevy::image::{CompressedImageFormats, ImageSampler, ImageType};
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use glam::{DMat3, DQuat, DVec3};
use sim_core::{ObjectState, Simulation, StarSystem};
use sim_planet::tangent_frame;
use sim_society::{Kit, Kits, Layout, RegionLayouts};

use crate::camera::{CameraSet, SpaceCamera};
use crate::sim::{RenderTime, Sim};
use crate::universe::{GameState, InGame, Universe, buildings_dir};

/// The camera this close to a settlement's edge (m) shows its buildings; they go again beyond
/// [`NEAR_OFF`] (a margin so they don't flicker at the boundary).
const NEAR_ON: f64 = 1_200.0;
const NEAR_OFF: f64 = 1_500.0;
/// Most buildings spawned per settlement per frame.
const NEAR_PER_FRAME: usize = 300;
/// Farthest a settlement is drawn (m).
const FAR: f64 = 250_000.0;
/// How far the distant boxes reach into the ground (m).
const SKIRT: f32 = 3.0;
/// Night-light brightness on screen (relative to a white surface in full starlight).
const NIGHT_GLOW: f32 = 6.0;

pub struct SettlementsPlugin;

/// Streaming and placing settlements (people follow it).
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct SettlementSet;

impl Plugin for SettlementsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(GameState::Running), setup_settlements)
            .add_systems(
                Update,
                (stream_layouts, stream_settlements, place_settlements)
                    .chain()
                    .after(CameraSet)
                    .in_set(SettlementSet)
                    .in_set(InGame),
            );
    }
}

/// A building model as loaded: its mesh and colours seen from above (roof) and the side.
struct Model {
    mesh: Handle<Mesh>,
    roof: [f32; 3],
    wall: [f32; 3],
}

pub(crate) struct Site {
    pub(crate) body: usize,
    /// Its index among its body's settlements (in `RegionSociety` and `RegionLayouts`).
    pub(crate) index: usize,
    /// Centre on the ground (surface frame, m) and the tangent frame's rotation there.
    pub(crate) origin: DVec3,
    pub(crate) basis: DQuat,
    /// Distance from the centre to the farthest building (m): estimated from its size until
    /// its layout is made.
    pub(crate) extent: f64,
    pub(crate) root: Entity,
    /// Its buildings, while the camera is near (spawned a few hundred a frame: `near_next` is
    /// the next building of its layout to spawn).
    pub(crate) near: Option<Vec<Entity>>,
    near_next: usize,
    /// Its layout once made (on approach, task 4b.5), and the entities drawn from it far away
    /// (distant form, streets, lights; the first is the distant form).
    pub(crate) layout: Option<std::sync::Arc<Layout>>,
    pub(crate) drawn: Vec<Entity>,
    /// Its people: the culture's clothing set and a seed for choices.
    pub(crate) clothing: String,
    pub(crate) seed: u64,
    pub(crate) settlement: sim_society::Settlement,
    pub(crate) tier: sim_society::TechTier,
    pub(crate) kit: String,
    pub(crate) naming: sim_society::NamingRules,
    /// Its culture (name and adjective), its faction, and its culture's word for each role.
    pub(crate) culture: (String, String),
    pub(crate) faction: String,
    pub(crate) role_names: HashMap<String, String>,
    /// Its culture's colours (sRGB 0–1), for its distant form.
    pub(crate) palette: Vec<[f64; 3]>,
}

/// Each building model's roof and wall colours (for distant forms built on workers).
type Colours = HashMap<(String, usize), ([f32; 3], [f32; 3])>;

/// A layout made on a worker, with its distant form, lights and streets (and whether it is a
/// colony's, for its lamps).
type Made = (Layout, Option<(Mesh, Mesh, Mesh, bool)>);

#[derive(Resource)]
pub(crate) struct SettlementDraw {
    pub(crate) kits: std::sync::Arc<Kits>,
    models: HashMap<(String, usize), Model>,
    material: Handle<StandardMaterial>,
    /// Lights: warm lanterns (medieval) and white lamps (colony).
    lanterns: Handle<StandardMaterial>,
    lamps: Handle<StandardMaterial>,
    pub(crate) sites: Vec<Site>,
    /// Each body's terrain as last applied (with the foundations of the layouts made).
    pub(crate) terrains: HashMap<usize, std::sync::Arc<sim_planet::Terrain>>,
    /// Layouts being made on workers, by site, and the meshes of those made, waiting to be
    /// drawn.
    pending: HashMap<usize, bevy::tasks::Task<Made>>,
    ready: HashMap<usize, (Mesh, Mesh, Mesh, bool)>,
    colours: std::sync::Arc<Colours>,
}

impl SettlementDraw {
    pub(crate) fn terrain(&self, body: usize) -> Option<&std::sync::Arc<sim_planet::Terrain>> {
        self.terrains.get(&body)
    }
}

/// The bytes of glTF buffer view `view` in the binary chunk `blob`.
fn view_bytes<'a>(view: &gltf::buffer::View, blob: &'a [u8]) -> Option<&'a [u8]> {
    blob.get(view.offset()..view.offset() + view.length())
}

fn linear(c: Color) -> [f32; 3] {
    let l = c.to_linear();
    [l.red, l.green, l.blue]
}

/// Loads a building model: one mesh, colours baked into vertex colours (each primitive's
/// material colour times its texture at the vertex's texture coordinate).
fn load_model(path: &Path) -> Result<(Mesh, [f32; 3], [f32; 3]), String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let gltf = gltf::Gltf::from_slice(&bytes).map_err(|e| e.to_string())?;
    let blob = gltf.blob.as_deref().ok_or("no binary chunk")?;
    // Images, decoded once.
    let mut images: Vec<Option<Image>> = Vec::new();
    for image in gltf.images() {
        let decoded = match image.source() {
            gltf::image::Source::View { view, mime_type } => {
                view_bytes(&view, blob).and_then(|b| {
                    Image::from_buffer(
                        b,
                        ImageType::MimeType(mime_type),
                        CompressedImageFormats::NONE,
                        true,
                        ImageSampler::Default,
                        RenderAssetUsages::MAIN_WORLD,
                    )
                    .ok()
                })
            }
            gltf::image::Source::Uri { .. } => None,
        };
        images.push(decoded);
    }
    let mesh = gltf.meshes().next().ok_or("no mesh")?;
    let (mut positions, mut normals, mut colors, mut indices) = (
        Vec::<[f32; 3]>::new(),
        Vec::<[f32; 3]>::new(),
        Vec::<[f32; 4]>::new(),
        Vec::<u32>::new(),
    );
    let (mut roof, mut roofs, mut wall, mut walls) = ([0.0f32; 3], 0.0f32, [0.0f32; 3], 0.0f32);
    for primitive in mesh.primitives() {
        let reader = primitive.reader(|_| Some(blob));
        let p: Vec<[f32; 3]> = reader.read_positions().ok_or("no positions")?.collect();
        let n: Vec<[f32; 3]> = reader.read_normals().ok_or("no normals")?.collect();
        let uv: Vec<[f32; 2]> = reader
            .read_tex_coords(0)
            .map_or_else(|| vec![[0.0; 2]; p.len()], |t| t.into_f32().collect());
        let pbr = primitive.material().pbr_metallic_roughness();
        let factor = pbr.base_color_factor();
        let texture = pbr
            .base_color_texture()
            .and_then(|t| images.get(t.texture().source().index())?.as_ref());
        let base = positions.len() as u32;
        for k in 0..p.len() {
            let mut c = [factor[0], factor[1], factor[2]];
            if let Some(img) = texture {
                let (w, h) = (img.width(), img.height());
                let x = ((uv[k][0].rem_euclid(1.0) * w as f32) as u32).min(w - 1);
                let y = ((uv[k][1].rem_euclid(1.0) * h as f32) as u32).min(h - 1);
                if let Ok(t) = img.get_color_at(x, y) {
                    let t = linear(t);
                    c = [c[0] * t[0], c[1] * t[1], c[2] * t[2]];
                }
            }
            // Roof and wall colours for the distant boxes (by facing).
            if n[k][1] > 0.5 {
                roof = [roof[0] + c[0], roof[1] + c[1], roof[2] + c[2]];
                roofs += 1.0;
            } else if n[k][1].abs() < 0.3 {
                wall = [wall[0] + c[0], wall[1] + c[1], wall[2] + c[2]];
                walls += 1.0;
            }
            colors.push([c[0], c[1], c[2], 1.0]);
        }
        positions.extend(p);
        normals.extend(n);
        indices.extend(
            reader
                .read_indices()
                .ok_or("no indices")?
                .into_u32()
                .map(|i| i + base),
        );
    }
    let mean = |c: [f32; 3], n: f32| {
        if n > 0.0 { c.map(|v| v / n) } else { [0.5; 3] }
    };
    let mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
    .with_inserted_indices(Indices::U32(indices));
    Ok((mesh, mean(roof, roofs), mean(wall, walls)))
}

/// Adds a box (`centre`, `half` extents, turned `yaw` about Y) to a vertex-coloured mesh being
/// built: top in `top`, sides in `side`, no bottom.
#[allow(clippy::too_many_arguments)]
pub(crate) fn add_box(
    positions: &mut Vec<[f32; 3]>,
    normals: &mut Vec<[f32; 3]>,
    colors: &mut Vec<[f32; 4]>,
    indices: &mut Vec<u32>,
    centre: Vec3,
    half: Vec3,
    yaw: f32,
    top: [f32; 3],
    side: [f32; 3],
) {
    let q = Quat::from_rotation_y(yaw);
    let faces: [(Vec3, Vec3, Vec3, [f32; 3]); 5] = [
        (Vec3::Y, Vec3::X, Vec3::Z, top),
        (Vec3::X, Vec3::Z, Vec3::Y, side),
        (Vec3::NEG_X, Vec3::NEG_Z, Vec3::Y, side),
        (Vec3::Z, Vec3::NEG_X, Vec3::Y, side),
        (Vec3::NEG_Z, Vec3::X, Vec3::Y, side),
    ];
    for (n, u, v, c) in faces {
        let base = positions.len() as u32;
        let o = n * half;
        let (hu, hv) = (u * half, v * half);
        for (su, sv) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
            positions.push((centre + q * (o + hu * su + hv * sv)).to_array());
            normals.push((q * n).to_array());
            colors.push([c[0], c[1], c[2], 1.0]);
        }
        // Counter-clockwise seen from outside (u × v = n for the top; sides use v = up).
        if u.cross(v).dot(n) > 0.0 {
            indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
        } else {
            indices.extend([base, base + 2, base + 1, base, base + 3, base + 2]);
        }
    }
}

pub(crate) fn vertex_mesh(
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
) -> Mesh {
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
    .with_inserted_indices(Indices::U32(indices))
}

/// Rough reach of a settlement of each size before its layout is made (m).
pub(crate) fn estimated_extent(size: sim_society::SizeClass) -> f64 {
    use sim_society::SizeClass::*;
    match size {
        Dwelling => 60.0,
        Hamlet => 200.0,
        Village => 400.0,
        Town => 800.0,
        City => 1_500.0,
        Metropolis | Megalopolis => 4_000.0,
    }
}

/// The camera this close to a settlement's edge (m) has its layout made (on a worker), and this
/// far drops it again.
const LAYOUT_ON: f64 = 4_000.0;
const LAYOUT_OFF: f64 = 7_000.0;
/// Most layouts being made at once.
const LAYOUTS_IN_FLIGHT: usize = 6;

/// A settlement's frame: its centre on the ground (surface frame, m) and its tangent frame's
/// rotation (what its meshes are drawn relative to).
#[derive(Clone, Copy)]
pub(crate) struct Frame {
    origin: DVec3,
    basis: DQuat,
}

impl Site {
    fn frame(&self) -> Frame {
        Frame {
            origin: self.origin,
            basis: self.basis,
        }
    }

    fn building_pose(&self, layout: &Layout, i: usize, radius: f64) -> (Vec3, Quat) {
        self.frame().building_pose(layout, i, radius)
    }
}

impl Frame {
    /// Building `i`'s model origin in the settlement's frame (curvature included) and its
    /// turn.
    fn building_pose(&self, layout: &Layout, i: usize, radius: f64) -> (Vec3, Quat) {
        let p = &layout.buildings[i];
        let up = layout.direction(p.x, p.z, radius);
        let world = up * (radius + p.height);
        let local = self.basis.inverse() * (world - self.origin);
        // The ground's tilt at the building against the centre's (a fraction of a degree).
        let tilt = self.basis.inverse() * DQuat::from_rotation_arc(layout.center, up) * self.basis;
        (
            local.as_vec3(),
            (tilt * DQuat::from_rotation_y(p.yaw)).as_quat().normalize(),
        )
    }
}

fn setup_settlements(
    mut commands: Commands,
    cameras: Query<Entity, With<crate::space_render::MainCamera>>,
    sim: Res<Sim>,
    universe: Res<Universe>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let system = &universe.0;
    // Debug (performance comparisons): `ULTRA_SPACE_NO_SETTLEMENTS=1` draws none.
    if std::env::var_os("ULTRA_SPACE_NO_SETTLEMENTS").is_some() {
        info!("debug: settlements not drawn");
        return;
    }
    let Some(dir) = buildings_dir() else {
        return;
    };
    let material = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.9,
        ..default()
    });
    // Lights: lantern glass by day, glowing by night (the emissive is set every frame).
    let glow = |c: Color| StandardMaterial {
        base_color: c,
        perceptual_roughness: 1.0,
        reflectance: 0.0,
        ..default()
    };
    let lanterns = materials.add(glow(Color::srgb(0.35, 0.25, 0.12)));
    let lamps = materials.add(glow(Color::srgb(0.3, 0.32, 0.35)));
    // Their pipelines compiled during the warm-up, not when the first settlement appears.
    let camera = cameras.iter().next();
    let (mut p, mut n, mut c, mut i) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    add_box(
        &mut p,
        &mut n,
        &mut c,
        &mut i,
        Vec3::ZERO,
        Vec3::ONE,
        0.0,
        [1.0; 3],
        [1.0; 3],
    );
    let primer = meshes.add(vertex_mesh(p, n, c, i));
    for m in [&material, &lanterns, &lamps] {
        crate::warmup::spawn_primer(
            &mut commands,
            camera,
            (Mesh3d(primer.clone()), MeshMaterial3d(m.clone())),
        );
    }
    crate::distant::spawn_primers(&mut commands, camera, &mut materials, primer.clone());
    let mut kits: Option<std::sync::Arc<Kits>> = None;
    let mut models = HashMap::new();
    let mut sites = Vec::new();
    let mut terrains = HashMap::new();
    for (body, region) in sim.0.regions.iter().enumerate() {
        let (Some(layouts), Some(society)) = (
            region.world.get_resource::<RegionLayouts>(),
            region.world.get_resource::<sim_society::RegionSociety>(),
        ) else {
            continue;
        };
        let kits = kits.get_or_insert_with(|| layouts.kits.clone());
        // Every kit's models, loaded once.
        for kit in kits.0.values() {
            for (i, b) in kit.buildings.iter().enumerate() {
                let key = (kit.id.clone(), i);
                if models.contains_key(&key) {
                    continue;
                }
                match load_model(&dir.join(&kit.id).join(&b.model)) {
                    Ok((mesh, roof, wall)) => {
                        models.insert(
                            key,
                            Model {
                                mesh: meshes.add(mesh),
                                roof,
                                wall,
                            },
                        );
                    }
                    Err(e) => warn!("building model {}/{}: {e}", kit.id, b.model),
                }
            }
        }
        if let Some(t) = region
            .world
            .get_resource::<sim_vehicle::flight::RegionTerrain>()
            .and_then(|t| t.0.clone())
        {
            terrains.insert(body, t);
        }
        let radius = system.bodies[body].radius;
        for (index, settlement) in society.settlements.iter().enumerate() {
            let profile = &society.society.cultures[settlement.culture].profile;
            let origin = settlement.direction * (radius + settlement.height);
            let (east, south) = tangent_frame(settlement.direction);
            let basis = DQuat::from_mat3(&DMat3::from_cols(east, settlement.direction, south));
            let root = commands
                .spawn((
                    Name::new("settlement"),
                    Transform::IDENTITY,
                    Visibility::Hidden,
                ))
                .id();
            sites.push(Site {
                body,
                index,
                origin,
                basis,
                extent: estimated_extent(settlement.size),
                root,
                near: None,
                near_next: 0,
                layout: None,
                drawn: Vec::new(),
                clothing: profile.clothing.clone(),
                seed: settlement.layout_seed,
                settlement: settlement.clone(),
                tier: profile.tech_tier,
                kit: profile.building_kit.clone(),
                naming: profile.naming.clone(),
                culture: (profile.name.clone(), profile.adjective.clone()),
                faction: society.society.factions[settlement.faction].name.clone(),
                role_names: profile
                    .roles
                    .iter()
                    .map(|r| (r.id.clone(), r.name.clone()))
                    .collect(),
                palette: profile.palette.clone(),
            });
        }
    }
    let Some(kits) = kits else {
        return;
    };
    info!(
        "settlements: {} sites, {} building models; layouts made on approach",
        sites.len(),
        models.len()
    );
    let colours: Colours = models
        .iter()
        .map(|(k, m)| (k.clone(), (m.roof, m.wall)))
        .collect();
    commands.insert_resource(SettlementDraw {
        kits,
        models,
        material,
        lanterns,
        lamps,
        sites,
        terrains,
        pending: HashMap::new(),
        ready: HashMap::new(),
        colours: std::sync::Arc::new(colours),
    });
}

/// Sites in one foundations and not the other (by id): their centres and angular reach.
fn foundation_changes(
    old: Option<&std::sync::Arc<sim_planet::Foundations>>,
    new: Option<&std::sync::Arc<sim_planet::Foundations>>,
) -> Vec<(DVec3, f64)> {
    let ids =
        |f: Option<&std::sync::Arc<sim_planet::Foundations>>| -> std::collections::HashSet<u32> {
            f.map(|f| f.sites().iter().map(|s| s.id()).collect())
                .unwrap_or_default()
        };
    let (a, b) = (ids(old), ids(new));
    let mut out = Vec::new();
    for (f, other) in [(old, &b), (new, &a)] {
        for s in f.map(|f| f.sites()).unwrap_or_default() {
            if !other.contains(&s.id()) {
                out.push((s.center(), s.reach()));
            }
        }
    }
    out
}

/// Makes the layouts of settlements the camera comes near (on workers) and drops those it has
/// left far behind; when the made set changes, rebuilds the region's foundations and gives the
/// terrain (physics and drawn tiles near the changes) the new ones, then draws the new
/// layouts' distant forms, streets and lights.
#[allow(clippy::too_many_arguments)]
fn stream_layouts(
    mut commands: Commands,
    draw: Option<ResMut<SettlementDraw>>,
    mut sim: ResMut<Sim>,
    mut terrains: ResMut<crate::terrain::Terrains>,
    universe: Res<Universe>,
    render: Res<RenderTime>,
    camera: Res<SpaceCamera>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let _slow = crate::debug_overlay::SlowSystem::new("stream_layouts");
    let Some(mut draw) = draw else { return };
    let draw = &mut *draw;
    let body = camera.anchor;
    let Some(rl) = sim
        .0
        .regions
        .get(body)
        .and_then(|r| r.world.get_resource::<RegionLayouts>())
        .cloned()
    else {
        return;
    };
    let mut rl = rl;
    // The region's terrain changed elsewhere (a debug start, a loaded save): follow it.
    if let Some(current) = sim
        .0
        .regions
        .get(body)
        .and_then(|r| r.world.get_resource::<sim_vehicle::flight::RegionTerrain>())
        .and_then(|t| t.0.clone())
        && draw
            .terrains
            .get(&body)
            .is_none_or(|t| !std::sync::Arc::ptr_eq(t, &current))
    {
        let near = match draw.terrains.get(&body) {
            Some(t) => foundation_changes(t.foundations(), current.foundations()),
            None => Vec::new(),
        };
        crate::terrain::update_terrain_near(
            &mut terrains,
            &mut commands,
            body,
            current.clone(),
            &near,
        );
        draw.terrains.insert(body, current);
    }
    let cam = camera_on(&universe, &camera, body, render.0);
    let mut changed = false;
    // Requests and drops.
    for (k, site) in draw.sites.iter().enumerate() {
        if site.body != body {
            continue;
        }
        let distance = (cam.distance(site.origin) - site.extent).max(0.0);
        let made = rl.layouts.contains_key(&site.index);
        if !made
            && distance < LAYOUT_ON
            && !draw.pending.contains_key(&k)
            && draw.pending.len() < LAYOUTS_IN_FLIGHT
        {
            let (s, tier, kits, natural) = (
                site.settlement.clone(),
                site.tier,
                rl.kits.clone(),
                rl.natural.clone(),
            );
            let kit_id = site.kit.clone();
            let (frame, colours, index) = (site.frame(), draw.colours.clone(), site.index);
            // The layout and its meshes, all on the worker.
            let task = crate::workers::workers().spawn(async move {
                let Some(kit) = kits.0.get(&kit_id) else {
                    let empty = Layout {
                        kit: kit_id,
                        center: s.direction,
                        buildings: Vec::new(),
                        nodes: Vec::new(),
                        streets: Vec::new(),
                        unplaced: 0,
                    };
                    return (empty, None);
                };
                let layout = sim_society::generate_layout(&s, tier, kit, &natural);
                let radius = natural.radius;
                let (far, lights) = distant_meshes(frame, &layout, kit, &colours, radius);
                // Streets on its own foundations.
                let own = sim_planet::Foundations::new(vec![
                    layout.site(kit, radius).with_id(index as u32),
                ]);
                let ground = natural.with_foundations(std::sync::Arc::new(own));
                let streets = street_mesh(frame, &layout, kit, &ground);
                let colony = kit
                    .buildings
                    .iter()
                    .any(|b| b.uses.iter().any(|u| u == "pad"));
                (layout, Some((far, lights, streets, colony)))
            });
            draw.pending.insert(k, task);
        } else if made && distance > LAYOUT_OFF && site.near.is_none() {
            rl.remove(site.index);
            changed = true;
        }
    }
    // Layouts made.
    let done: Vec<usize> = draw
        .pending
        .iter_mut()
        .filter_map(|(&k, t)| t.is_finished().then_some(k))
        .collect();
    for k in done {
        if let Some(task) = draw.pending.remove(&k) {
            let (layout, made) = bevy::tasks::block_on(task);
            if let Some(m) = made {
                draw.ready.insert(k, m);
            }
            rl.insert(draw.sites[k].index, std::sync::Arc::new(layout));
            changed = true;
        }
    }
    if changed {
        let old = rl.foundations.clone();
        rl.rebuild_foundations();
        let Some(region) = sim.0.regions.get_mut(body).map(|r| &mut r.world) else {
            return;
        };
        let current = region
            .get_resource::<sim_vehicle::flight::RegionTerrain>()
            .and_then(|t| t.0.clone())
            .unwrap_or_else(|| rl.natural.clone());
        let terrain = std::sync::Arc::new(current.with_foundations(rl.foundations.clone()));
        region.insert_resource(sim_vehicle::flight::RegionTerrain(Some(terrain.clone())));
        // Tiles near sites that came or went.
        let near = foundation_changes(Some(&old), Some(&rl.foundations));
        crate::terrain::update_terrain_near(
            &mut terrains,
            &mut commands,
            body,
            terrain.clone(),
            &near,
        );
        draw.terrains.insert(body, terrain);
        region.insert_resource(rl.clone());
    }
    // Draw new layouts, forget dropped ones.
    let radius = universe.0.bodies[body].radius;
    let Some(terrain) = draw.terrains.get(&body).cloned() else {
        return;
    };
    let material = draw.material.clone();
    let (lanterns, lamps) = (draw.lanterns.clone(), draw.lamps.clone());
    let mut ready = std::mem::take(&mut draw.ready);
    let colours = draw.colours.clone();
    for (k, site) in draw.sites.iter_mut().enumerate() {
        if site.body != body {
            continue;
        }
        let made = rl.layouts.get(&site.index).cloned();
        match (&site.layout, made) {
            (None, Some(layout)) => {
                let Some(kit) = draw.kits.0.get(&layout.kit) else {
                    site.layout = Some(layout);
                    continue;
                };
                site.extent = layout
                    .buildings
                    .iter()
                    .map(|p| (p.x * p.x + p.z * p.z).sqrt() + 20.0)
                    .fold(50.0, f64::max);
                // Made on a worker with the layout, or (a layout made elsewhere, such as the
                // debug start's) here.
                let (far, lights, streets, colony) = ready.remove(&k).unwrap_or_else(|| {
                    let (far, lights) =
                        distant_meshes(site.frame(), &layout, kit, &colours, radius);
                    let streets = street_mesh(site.frame(), &layout, kit, &terrain);
                    let colony = kit
                        .buildings
                        .iter()
                        .any(|b| b.uses.iter().any(|u| u == "pad"));
                    (far, lights, streets, colony)
                });
                let far = commands
                    .spawn((
                        FarForm,
                        Mesh3d(meshes.add(far)),
                        MeshMaterial3d(material.clone()),
                        NotShadowCaster,
                        Transform::IDENTITY,
                        ChildOf(site.root),
                    ))
                    .id();
                let streets = commands
                    .spawn((
                        Mesh3d(meshes.add(streets)),
                        MeshMaterial3d(material.clone()),
                        NotShadowCaster,
                        Transform::IDENTITY,
                        ChildOf(site.root),
                    ))
                    .id();
                let lights = commands
                    .spawn((
                        Mesh3d(meshes.add(lights)),
                        MeshMaterial3d(if colony {
                            lamps.clone()
                        } else {
                            lanterns.clone()
                        }),
                        NotShadowCaster,
                        Transform::IDENTITY,
                        ChildOf(site.root),
                    ))
                    .id();
                site.drawn = vec![far, streets, lights];
                site.layout = Some(layout);
            }
            (Some(_), None) => {
                for e in site.drawn.drain(..) {
                    commands.entity(e).despawn();
                }
                site.layout = None;
            }
            _ => {}
        }
    }
    draw.ready = ready;
}

/// Marks a settlement's distant form (hidden while its buildings are shown).
#[derive(Component)]
struct FarForm;

/// A settlement's distant boxes and its lights, in its own frame.
fn distant_meshes(
    site: Frame,
    layout: &Layout,
    kit: &Kit,
    colours: &Colours,
    radius: f64,
) -> (Mesh, Mesh) {
    let (mut p, mut n, mut c, mut idx) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let (mut lp, mut ln, mut lc, mut li) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let colony = kit
        .buildings
        .iter()
        .any(|b| b.uses.iter().any(|u| u == "pad"));
    for (i, placed) in layout.buildings.iter().enumerate() {
        let b = &kit.buildings[placed.building];
        let (origin, rotation) = site.building_pose(layout, i, radius);
        let yaw = placed.yaw as f32;
        let (lo, hi) = (
            Vec3::new(b.min_m[0] as f32, 0.0, b.min_m[2] as f32),
            Vec3::new(b.max_m[0] as f32, b.max_m[1] as f32, b.max_m[2] as f32),
        );
        let (roof, wall) = colours
            .get(&(kit.id.clone(), placed.building))
            .copied()
            .unwrap_or(([0.5; 3], [0.5; 3]));
        // Trees as narrow cones would be fussy: a slim box in their colour.
        let tree = b.uses.iter().any(|u| u == "tree");
        let shrink = if tree { 0.45 } else { 1.0 };
        let half = Vec3::new(
            0.5 * (hi.x - lo.x) * shrink,
            0.5 * (hi.y + SKIRT),
            0.5 * (hi.z - lo.z) * shrink,
        );
        let centre_local = Vec3::new(
            0.5 * (lo.x + hi.x),
            0.5 * (hi.y - SKIRT),
            0.5 * (lo.z + hi.z),
        );
        let centre = origin + rotation * centre_local;
        add_box(
            &mut p, &mut n, &mut c, &mut idx, centre, half, yaw, roof, wall,
        );
        // A lantern by the door of every building people use (not trees, wells or carts).
        let lit = b.uses.iter().any(|u| {
            !matches!(
                u.as_str(),
                "tree" | "prop" | "square" | "market" | "pad" | "power" | "comms"
            )
        });
        if lit {
            let door = origin + rotation * Vec3::new(0.5 * (lo.x + hi.x), 2.4, hi.z + 0.3);
            add_box(
                &mut lp,
                &mut ln,
                &mut lc,
                &mut li,
                door,
                Vec3::splat(0.35),
                yaw,
                [1.0; 3],
                [1.0; 3],
            );
        }
    }
    // Colony street lamps: at every street corner, on 6 m posts.
    if colony {
        for node in &layout.nodes {
            let up = layout.direction(node.x, node.y, radius);
            let ground = layout.buildings.first().map_or(0.0, |b| b.height);
            let world = up * (radius + ground + 6.0);
            let local = (site.basis.inverse() * (world - site.origin)).as_vec3();
            add_box(
                &mut lp,
                &mut ln,
                &mut lc,
                &mut li,
                local,
                Vec3::splat(0.4),
                0.0,
                [1.0; 3],
                [1.0; 3],
            );
        }
    }
    (vertex_mesh(p, n, c, idx), vertex_mesh(lp, ln, lc, li))
}

/// Street surfaces: a ribbon along every street following the ground (dirt lanes and a paved
/// square for the medieval style, concrete for the colony), a little above it.
fn street_mesh(site: Frame, layout: &Layout, kit: &Kit, terrain: &sim_planet::Terrain) -> Mesh {
    let colony = kit
        .buildings
        .iter()
        .any(|b| b.uses.iter().any(|u| u == "pad"));
    let (width, colour) = if colony {
        (6.0, [0.32, 0.32, 0.31])
    } else {
        (4.0, [0.28, 0.21, 0.13])
    };
    let r = terrain.radius;
    let l = layout;
    let to_local = |x: f64, z: f64| {
        let up = l.direction(x, z, r);
        let world = up * (r + terrain.surface_height(up, 2.0) + 0.2);
        (site.basis.inverse() * (world - site.origin)).as_vec3()
    };
    let (mut p, mut n, mut c, mut idx) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut quad = |corners: [Vec3; 4], colour: [f32; 3]| {
        let base = p.len() as u32;
        for v in corners {
            p.push(v.to_array());
            n.push([0.0, 1.0, 0.0]);
            c.push([colour[0], colour[1], colour[2], 1.0]);
        }
        // Counter-clockwise seen from above.
        idx.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    };
    for &(a, b) in &l.streets {
        let (a, b) = (l.nodes[a as usize], l.nodes[b as usize]);
        let length = a.distance(b);
        if length < 0.1 {
            continue;
        }
        let t = (b - a) / length;
        let side = glam::DVec2::new(-t.y, t.x) * (0.5 * width);
        // Overlapping a little at both ends, so turns don't leave gaps.
        let (a, b) = (a - t * 0.5 * width, b + t * 0.5 * width);
        let pieces = (a.distance(b) / 6.0).ceil().max(1.0) as usize;
        for k in 0..pieces {
            let (f0, f1) = (k as f64 / pieces as f64, (k + 1) as f64 / pieces as f64);
            let (p0, p1) = (a.lerp(b, f0), a.lerp(b, f1));
            let v = |q: glam::DVec2| to_local(q.x, q.y);
            quad(
                [v(p0 + side), v(p1 + side), v(p1 - side), v(p0 - side)],
                colour,
            );
        }
    }
    // The medieval square: paved out to its ring street.
    if !colony && l.nodes.len() >= 8 {
        let radius = l.nodes[0].length() + 0.5 * width;
        let centre = to_local(0.0, 0.0);
        let steps = 16;
        for k in 0..steps {
            let a0 = std::f64::consts::TAU * k as f64 / steps as f64;
            let a1 = std::f64::consts::TAU * (k + 1) as f64 / steps as f64;
            let e0 = to_local(radius * a0.cos(), radius * a0.sin());
            let e1 = to_local(radius * a1.cos(), radius * a1.sin());
            quad([centre, e1, e0, centre], [0.36, 0.33, 0.29]);
        }
    }
    vertex_mesh(p, n, c, idx)
}

/// The camera's position in the surface frame of `body` at the render time.
pub(crate) fn camera_on(universe: &Universe, camera: &SpaceCamera, body: usize, t: f64) -> DVec3 {
    let system = &universe.0;
    let anchor = system.bodies[camera.anchor].inertial_frame;
    let s = system
        .frames
        .state_in(anchor, system.bodies[body].surface_frame, t);
    s.rotation.inverse() * (camera.offset - s.position)
}

/// Shows each settlement's buildings near the camera and its distant form elsewhere.
fn stream_settlements(
    mut commands: Commands,
    draw: Option<ResMut<SettlementDraw>>,
    universe: Res<Universe>,
    render: Res<RenderTime>,
    camera: Res<SpaceCamera>,
    mut visibility: Query<&mut Visibility>,
) {
    let _slow = crate::debug_overlay::SlowSystem::new("stream_settlements");
    let Some(mut draw) = draw else {
        return;
    };
    let draw = &mut *draw;
    let mut cameras: HashMap<usize, DVec3> = HashMap::new();
    for site in &mut draw.sites {
        let radius = universe.0.bodies[site.body].radius;
        let cam = *cameras
            .entry(site.body)
            .or_insert_with(|| camera_on(&universe, &camera, site.body, render.0));
        let distance = (cam.distance(site.origin) - site.extent).max(0.0);
        let shown = distance < FAR;
        if let Ok(mut v) = visibility.get_mut(site.root) {
            let want = if shown {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
            if *v != want {
                *v = want;
            }
        }
        // Buildings near, once the layout is made (and its kit exists).
        let near = site.layout.is_some()
            && match site.near {
                Some(_) => distance < NEAR_OFF,
                None => distance < NEAR_ON,
            };
        match (&site.near, near) {
            (None, true) => {
                site.near = Some(Vec::new());
                site.near_next = 0;
            }
            (Some(_), false) => {
                for e in site.near.take().unwrap_or_default() {
                    commands.entity(e).despawn();
                }
            }
            _ => {}
        }
        // Spawn the next few hundred of its buildings (a city has thousands).
        if site.near.is_some()
            && let Some(layout) = site.layout.clone()
            && site.near_next < layout.buildings.len()
            && let Some(kit) = draw.kits.0.get(&layout.kit)
        {
            let end = (site.near_next + NEAR_PER_FRAME).min(layout.buildings.len());
            let mut spawned = Vec::with_capacity(end - site.near_next);
            for i in site.near_next..end {
                let placed = &layout.buildings[i];
                let Some(model) = draw.models.get(&(kit.id.clone(), placed.building)) else {
                    continue;
                };
                let (translation, rotation) = site.building_pose(&layout, i, radius);
                spawned.push(
                    commands
                        .spawn((
                            Mesh3d(model.mesh.clone()),
                            MeshMaterial3d(draw.material.clone()),
                            Transform {
                                translation,
                                rotation,
                                ..default()
                            },
                            ChildOf(site.root),
                        ))
                        .id(),
                );
            }
            site.near_next = end;
            if let Some(near) = &mut site.near {
                near.extend(spawned);
            }
        }
        // The distant form stays until every building is there.
        let complete = site
            .layout
            .as_ref()
            .is_some_and(|l| site.near_next >= l.buildings.len());
        let far_visible = if site.near.is_some() && complete {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
        if let Some(&far) = site.drawn.first()
            && let Ok(mut v) = visibility.get_mut(far)
            && *v != far_visible
        {
            *v = far_visible;
        }
    }
}

/// Places every shown settlement camera-relative and sets the lights by the sun at the one
/// nearest the camera.
fn place_settlements(
    draw: Option<Res<SettlementDraw>>,
    universe: Res<Universe>,
    render: Res<RenderTime>,
    camera: Res<SpaceCamera>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut roots: Query<(&Visibility, &mut Transform)>,
) {
    let _slow = crate::debug_overlay::SlowSystem::new("place_settlements");
    let Some(draw) = draw else {
        return;
    };
    let system = &universe.0;
    let anchor = system.bodies[camera.anchor].inertial_frame;
    let star =
        system
            .frames
            .transform_point(anchor, system.star().inertial_frame, render.0, DVec3::ZERO);
    let mut poses: HashMap<usize, (DVec3, DQuat)> = HashMap::new();
    let mut nearest = (f64::MAX, 0.0);
    for site in &draw.sites {
        let Ok((visibility, mut transform)) = roots.get_mut(site.root) else {
            continue;
        };
        if *visibility == Visibility::Hidden {
            continue;
        }
        let (position, rotation) = *poses.entry(site.body).or_insert_with(|| {
            let s =
                system
                    .frames
                    .state_in(anchor, system.bodies[site.body].surface_frame, render.0);
            (s.position, s.rotation)
        });
        let at = position + rotation * site.origin;
        transform.translation = (at - camera.offset).as_vec3();
        transform.rotation = (rotation * site.basis).as_quat().normalize();
        let d = at.distance(camera.offset);
        if d < nearest.0 {
            // The sun's height there: above the horizon, no lights; dusk brings them up.
            let up = (rotation * site.origin).normalize();
            let sun = (star - at).normalize_or_zero().dot(up);
            nearest = (d, 1.0 - smoothstep(-0.12, 0.05, sun));
        }
    }
    let night = nearest.1 as f32;
    let exposure = Exposure::default().exposure();
    for (handle, color) in [
        (&draw.lanterns, LinearRgba::rgb(1.0, 0.55, 0.2)),
        (&draw.lamps, LinearRgba::rgb(0.8, 0.9, 1.0)),
    ] {
        let want = color * (night * NIGHT_GLOW / exposure);
        if let Some(m) = materials.get(handle)
            && m.emissive == want
        {
            continue;
        }
        if let Some(mut m) = materials.get_mut(handle) {
            m.emissive = want;
        }
    }
}

fn smoothstep(e0: f64, e1: f64, x: f64) -> f64 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// A body's climate with its conditions' blight (task 4b.6): hazard zones dry and kill the
/// land, the hot zones most, so drawn terrain and the maps seen from orbit show the waste.
pub(crate) fn climate_of(
    sim: &Simulation,
    index: usize,
    body: &sim_core::Body,
) -> Option<sim_planet::Climate> {
    let climate = sim_planet::Climate::of(body)?;
    let Some(rs) = sim
        .regions
        .get(index)
        .and_then(|r| r.world.get_resource::<sim_society::RegionSociety>())
    else {
        return Some(climate);
    };
    let zones: Vec<(DVec3, f64, f64)> = rs
        .conditions
        .iter()
        .flat_map(|c| c.zones.iter())
        .map(|z| {
            (
                z.center,
                z.radius / body.radius,
                (z.intensity * 2.5).min(1.0),
            )
        })
        .collect();
    Some(if zones.is_empty() {
        climate
    } else {
        climate.with_blight(zones)
    })
}

/// Whether settlement `s` is the one asked for: by name, or by kind (lore kind, size or its
/// people's word), optionally of one people (`hollin:city`).
fn wanted(society: &sim_society::RegionSociety, s: &sim_society::Settlement, what: &str) -> bool {
    let (people, what) = match what.split_once(':') {
        Some((p, w)) => (Some(p), w),
        None => (None, what),
    };
    if let Some(p) = people
        && society.society.cultures[s.culture].profile.id != p
    {
        return false;
    }
    s.name.to_lowercase() == what
        || s.lore_kind() == what
        || s.size.name() == what
        || s.word.to_lowercase() == what
}

/// Debug start (tasks 4.6–4.9): `ULTRA_SPACE_START_NEAR=<settlement name or kind>[,<distance
/// m>[,<height m>[,day|dusk|night|<local hour>]]]` (a kind may name its people: `hollin:city`)
/// starts the ship `distance` m (default 300) south of that
/// settlement (by name, or the biggest of a kind such as `town` or `colony`), `height` m
/// (default 0: on the ground) above the ground there, level, nose toward it, keeping still over
/// the ground. With a time of day, the universe time is first moved to when the sun stands so
/// there (morning sun 35° up, sunset, or midnight). Returns the state (in body `body`'s
/// inertial frame), the universe time (s) to start at, and whether it starts in the air (the
/// ship then starts in arcade flight, holding still).
pub fn start_near(
    sim: &mut Simulation,
    system: &StarSystem,
    body: usize,
) -> Option<(ObjectState, f64, bool)> {
    let spec = std::env::var("ULTRA_SPACE_START_NEAR").ok()?;
    let mut parts = spec.split(',').map(str::trim);
    let what = parts.next()?.to_lowercase();
    let distance: f64 = parts.next().and_then(|v| v.parse().ok()).unwrap_or(300.0);
    let height: f64 = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0.0);
    let when = parts.next().map(str::to_lowercase);
    // Lay out the settlement now (it would be made on approach anyway), so the landing spot
    // keeps clear of its buildings.
    {
        let region = &mut sim.regions.get_mut(body)?.world;
        let society = region.get_resource::<sim_society::RegionSociety>()?.clone();
        let i = society
            .settlements
            .iter()
            .enumerate()
            .filter(|(_, s)| wanted(&society, s, &what))
            .max_by_key(|(_, s)| s.population)
            .map(|(i, _)| i);
        if let (Some(i), Some(mut rl)) = (i, region.get_resource::<RegionLayouts>().cloned()) {
            let layout = RegionLayouts::layout_of(&society, &rl.kits, &rl.natural, i);
            rl.insert(i, std::sync::Arc::new(layout));
            rl.rebuild_foundations();
            if let Some(t) = region
                .get_resource::<sim_vehicle::flight::RegionTerrain>()
                .and_then(|t| t.0.clone())
            {
                region.insert_resource(sim_vehicle::flight::RegionTerrain(Some(
                    std::sync::Arc::new(t.with_foundations(rl.foundations.clone())),
                )));
            }
            region.insert_resource(rl);
        }
    }
    let region = sim.regions.get(body)?;
    let society = region.world.get_resource::<sim_society::RegionSociety>()?;
    let terrain = region
        .world
        .get_resource::<sim_vehicle::flight::RegionTerrain>()?
        .0
        .clone()?;
    // A settlement by name or kind, or a ruin ("ruin": the biggest; or by name).
    let settlement = society
        .settlements
        .iter()
        .filter(|s| wanted(society, s, &what))
        .max_by_key(|s| s.population)
        .map(|s| (s.name.clone(), s.word.clone(), s.direction));
    let ruin = || {
        society
            .ruins
            .iter()
            .filter(|r| what == "ruin" || r.name.to_lowercase() == what)
            .max_by_key(|r| r.size)
            .map(|r| (r.name.clone(), "ruin".to_string(), r.direction))
    };
    let (name, word, direction) = settlement.or_else(ruin)?;
    let (east, south) = tangent_frame(direction);
    let r = terrain.radius;
    // Landing: the first spot from `distance` outward with no building within 12 m (not on a
    // roof).
    let clear = |d: f64| {
        let up = (direction * r + south * d).normalize();
        terrain
            .foundations()
            .is_none_or(|f| f.structures_near(up, 12.0).next().is_none())
    };
    let distance = if height > 0.0 {
        distance
    } else {
        (0..200)
            .map(|k| distance + 5.0 * f64::from(k))
            .find(|&d| clear(d))
            .unwrap_or(distance)
    };
    let up = (direction * r + south * distance).normalize();
    let ground = terrain.surface_height(up, 1.0);
    // On the ground: the ship's lowest point a little above it (it settles on its legs).
    let lift = if height > 0.0 { height } else { 4.0 };
    let surface = up * (r + ground + lift);
    let north = up.cross(east).normalize();
    let t = match when.as_deref() {
        Some(w) => match w.parse::<f64>() {
            Ok(hour) => at_local_hour(system, body, up, hour),
            Err(_) => time_of_day(system, body, up, w),
        },
        None => sim.universe_time().as_secs_f64(),
    };
    let frame = system.frames.state_in(
        system.bodies[body].inertial_frame,
        system.bodies[body].surface_frame,
        t,
    );
    let position = frame.rotation * surface;
    let rotation = crate::camera::look_rotation(frame.rotation * north, frame.rotation * up);
    let mut state = ObjectState::at_rest(system.bodies[body].inertial_frame, position, rotation);
    state.state.velocity = frame.angular_velocity.cross(position);
    state.state.angular_velocity = frame.angular_velocity;
    info!("debug: starting {distance:.0} m south of {name} ({word}), {lift:.0} m up, at {t:.0} s");
    Some((state, t, height > 0.0))
}

/// The first universe time (s) when it is `hour` (local solar time) at surface direction `up`.
fn at_local_hour(system: &StarSystem, body: usize, up: DVec3, hour: f64) -> f64 {
    let b = &system.bodies[body];
    let longitude = (-up.z).atan2(up.x);
    let fraction = |t: f64| {
        sim_core::time::local_solar_fraction(
            &system.frames,
            b.surface_frame,
            system.star().inertial_frame,
            longitude,
            sim_core::time::UniverseTime::from_nanos((t * 1e9) as u64),
        )
    };
    let spin = system
        .frames
        .state_in(b.inertial_frame, b.surface_frame, 0.0)
        .angular_velocity
        .length()
        .max(1e-9);
    let day = std::f64::consts::TAU / spin * 1.1;
    let want = (hour / 24.0).rem_euclid(1.0);
    (0..=2880)
        .map(|k| day * f64::from(k) / 2880.0)
        .min_by(|&a, &b| {
            let d = |t: f64| {
                let x = (fraction(t) - want).abs();
                x.min(1.0 - x)
            };
            d(a).total_cmp(&d(b))
        })
        .unwrap_or(0.0)
}

/// The first universe time (s) in the body's first day when the sun at surface direction `up`
/// stands as `when` says: "day" (35° up, rising), "dusk" (3° up, setting), "night" (lowest).
fn time_of_day(system: &StarSystem, body: usize, up: DVec3, when: &str) -> f64 {
    let b = &system.bodies[body];
    let spin = system
        .frames
        .state_in(b.inertial_frame, b.surface_frame, 0.0)
        .angular_velocity
        .length()
        .max(1e-9);
    let day = std::f64::consts::TAU / spin;
    let elevation = |t: f64| {
        let f = system.frames.state_in(b.inertial_frame, b.surface_frame, t);
        let star = system.frames.transform_point(
            b.inertial_frame,
            system.star().inertial_frame,
            t,
            DVec3::ZERO,
        );
        let u = f.rotation * up;
        (star - u * b.radius).normalize().dot(u)
    };
    let steps = 1440;
    let samples: Vec<(f64, f64)> = (0..=steps)
        .map(|k| {
            let t = day * k as f64 / steps as f64;
            (t, elevation(t))
        })
        .collect();
    let crossing = |target: f64, rising: bool| {
        samples
            .windows(2)
            .find(|w| {
                let (a, b) = (w[0].1 - target, w[1].1 - target);
                if rising {
                    a < 0.0 && b >= 0.0
                } else {
                    a > 0.0 && b <= 0.0
                }
            })
            .map(|w| w[1].0)
    };
    match when {
        "day" => crossing(35f64.to_radians().sin(), true),
        "dusk" => crossing(3f64.to_radians().sin(), false),
        _ => samples
            .iter()
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|s| s.0),
    }
    .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use bevy::mesh::VertexAttributeValues;

    use super::*;

    #[test]
    fn every_kit_model_loads_with_its_colours() {
        let dir =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../../assets/export/buildings");
        let kits = Kits::load_dir(&dir).unwrap();
        assert!(kits.0.len() >= 2);
        for kit in kits.0.values() {
            for b in &kit.buildings {
                let (mesh, roof, wall) = load_model(&dir.join(&kit.id).join(&b.model))
                    .unwrap_or_else(|e| panic!("{}/{}: {e}", kit.id, b.model));
                let Some(VertexAttributeValues::Float32x4(colors)) =
                    mesh.attribute(Mesh::ATTRIBUTE_COLOR)
                else {
                    panic!("{}/{}: no colours", kit.id, b.model);
                };
                // Several colours (roof, walls, trim), none black from a missed texture.
                let mut distinct: Vec<[u32; 3]> = colors
                    .iter()
                    .map(|c| [c[0], c[1], c[2]].map(|v| (v * 50.0) as u32))
                    .collect();
                distinct.sort();
                distinct.dedup();
                assert!(distinct.len() >= 2, "{}/{}: one colour", kit.id, b.model);
                for c in [roof, wall] {
                    assert!(c.iter().any(|&v| v > 0.02), "{}/{}: black", kit.id, b.model);
                }
            }
        }
    }
}
