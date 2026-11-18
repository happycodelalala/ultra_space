//! Ground cover planet-wide (design/ground-cover.md): species generated as block models with
//! variants, placed by recipes per biome with six layers of procedural variety, and merged per
//! ground chunk into one mesh in the chunk's own frame.
//!
//! Everything here is deterministic from the place and the body's seed: the same ground chunk
//! always gets the same things, at every level of detail (positions come from grids fixed to the
//! ground, not to chunks), so untouched cover is never saved.

pub mod generate;

use std::collections::HashMap;
use std::sync::Arc;

use glam::{DQuat, DVec2, DVec3, IVec3, Vec3};
use serde::Deserialize;
use sim_blocks::{Dir, MeshData, Palette};

use crate::blocks::{BlockGround, LodKey};
use crate::globe::{Cover, GlobeWorld};
use crate::noise::{gradient, hash3, mix64, unit};
use generate::{Params, Role};

const DATA: &str = include_str!("../../../../../../content/data/ground_cover/standard.json");
const PALETTE: &str = include_str!("../../../../../../assets/export/blocks/palette.json");

/// The coarsest ground level that carries cover.
pub const MAX_LEVEL: u8 = 4;

#[derive(Clone, Debug, Deserialize)]
pub struct Tint {
    pub hue: f64,
    pub value: f64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct SpeciesData {
    pub id: String,
    pub generator: String,
    pub block_m: f64,
    pub height_m: [f64; 2],
    pub width_m: [f64; 2],
    pub variants: u32,
    pub colours: HashMap<String, String>,
    pub tint: Tint,
    pub scale_spread: f64,
    pub collider: String,
    pub tilt_to_slope: bool,
    pub sink: f64,
    #[serde(default)]
    pub description: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Cluster {
    pub scale_m: f64,
    pub strength: f64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Entry {
    pub species: String,
    pub density_per_ha: f64,
    pub spacing_m: f64,
    pub cluster: Cluster,
    pub share: f64,
    pub slope_max_deg: f64,
    pub min_height_m: Option<f64>,
    pub max_height_m: Option<f64>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Rules {
    pub shore_height_m: f64,
    pub steep_slope_deg: f64,
    pub high_temperature_c: f64,
    pub tree_line_temperature_c: f64,
    pub warm_shore_temperature_c: f64,
}

#[derive(Clone, Debug, Deserialize)]
struct DataFile {
    #[serde(default)]
    #[allow(dead_code)]
    description: String,
    species: Vec<SpeciesData>,
    recipes: HashMap<String, Vec<Entry>>,
    rules: Rules,
}

/// Generators of things that grow tall (held back above the tree line).
const TREES: [&str; 11] = [
    "broadleaf",
    "broadleaf_tall",
    "broadleaf_wide",
    "birch",
    "willow",
    "giant",
    "acacia",
    "baobab",
    "conifer",
    "pine",
    "palm",
];

/// One variant: its meshes at three levels of voxel detail (full, merged 2×, merged 4×), its
/// size, and the colours of its roles (linear RGB).
struct Variant {
    meshes: [MeshData; 3],
    height: f64,
    radius: f64,
    colours: [[f32; 3]; 13],
}

/// A placed thing.
#[derive(Clone, Copy, Debug)]
pub struct Instance {
    pub species: u16,
    pub variant: u16,
    /// Face grid point (m) and the ground's height there (m above the reference radius).
    pub at: DVec2,
    pub height: f64,
    pub yaw: f64,
    pub scale: f64,
    /// The ground's normal (surface frame, unit) for things tilted to the slope.
    pub up: DVec3,
    pub hue: f64,
    pub value: f64,
    /// Dryness 0–1 (yellows leaves, dries moss) and warmth (°C).
    pub dry: f64,
}

/// A chunk's cover: one mesh in the chunk's own frame, colours final (linear RGBA).
#[derive(Default)]
pub struct CoverMesh {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub colours: Vec<[f32; 4]>,
    pub indices: Vec<u32>,
    pub instances: usize,
}

/// The land at a place, as the recipes read it.
struct Land {
    height: f64,
    slope_deg: f64,
    temperature: f64,
    wet: f64,
    recipe: Option<usize>,
    normal: DVec3,
}

/// Ground cover for one body.
pub struct CoverWorld {
    species: Vec<SpeciesData>,
    variants: Vec<Vec<Variant>>,
    recipes: Vec<Vec<(usize, Entry)>>,
    recipe_index: HashMap<String, usize>,
    rules: Rules,
    /// Per species: its grid spacing (the smallest of its recipes) and the highest chance a
    /// candidate is kept anywhere (early rejection before reading the land).
    spacing: Vec<f64>,
    max_keep: Vec<f64>,
    seed: u64,
    density: f64,
}

impl std::fmt::Debug for CoverWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CoverWorld")
            .field("species", &self.species.len())
            .finish_non_exhaustive()
    }
}

fn ups(kind: &str) -> Vec<Dir> {
    match kind {
        "conifer" | "pine" | "krummholz" | "log" | "stump" | "mound" => vec![Dir::PZ],
        "rock" | "ice" | "pebbles" | "outcrop" => vec![Dir::PZ, Dir::PX, Dir::NX, Dir::PY, Dir::NY],
        "tuft" | "flowers" | "reeds" | "fern" | "bamboo" | "crust" | "mushrooms" => Vec::new(),
        _ => vec![Dir::PZ, Dir::NZ],
    }
}

/// Merges voxels 2 × 2 × 2: a cell where at least two fine cells were (wood: one), coloured
/// as the most common.
fn coarsen(cells: &HashMap<IVec3, u16>) -> HashMap<IVec3, u16> {
    let mut counts: HashMap<IVec3, [u8; 13]> = HashMap::new();
    for (c, &v) in cells {
        counts
            .entry(c.div_euclid(IVec3::splat(2)))
            .or_insert([0; 13])[v as usize] += 1;
    }
    counts
        .into_iter()
        .filter_map(|(c, n)| {
            let total: u8 = n.iter().sum();
            let wood = n[Role::Wood as usize] + n[Role::Wood2 as usize];
            let (best, _) = n.iter().enumerate().skip(1).max_by_key(|(_, k)| **k)?;
            (total >= 2 || wood >= 1).then_some((c, best as u16))
        })
        .collect()
}

fn mesh_cells(cells: &HashMap<IVec3, u16>, ups: &[Dir], size: f64) -> MeshData {
    if cells.is_empty() {
        return MeshData::default();
    }
    let lo = cells.keys().fold(IVec3::MAX, |a, &c| a.min(c));
    let hi = cells.keys().fold(IVec3::MIN, |a, &c| a.max(c)) + IVec3::ONE;
    sim_blocks::mesh(cells, lo, hi, ups, size as f32)
}

/// sRGB-ish HSV adjustments of a linear colour: hue shift (turns), saturation and value factors.
fn adjust(c: [f32; 3], dh: f64, ds: f64, dv: f64) -> [f32; 3] {
    let g = c.map(|v| f64::from(v).max(0.0).powf(1.0 / 2.2));
    let (r, gg, b) = (g[0], g[1], g[2]);
    let max = r.max(gg).max(b);
    let min = r.min(gg).min(b);
    let d = max - min;
    let mut h = if d < 1e-9 {
        0.0
    } else if max == r {
        ((gg - b) / d).rem_euclid(6.0)
    } else if max == gg {
        (b - r) / d + 2.0
    } else {
        (r - gg) / d + 4.0
    } / 6.0;
    let s = if max < 1e-9 { 0.0 } else { d / max };
    h = (h + dh).rem_euclid(1.0);
    let s = (s * ds).clamp(0.0, 1.0);
    let v = (max * dv).clamp(0.0, 1.0);
    let k = |n: f64| {
        let kk = (n + h * 6.0).rem_euclid(6.0);
        v - v * s * kk.min(4.0 - kk).clamp(0.0, 1.0)
    };
    [k(5.0), k(3.0), k(1.0)].map(|x| x.powf(2.2) as f32)
}

impl CoverWorld {
    /// The cover of a body with `seed`, built from the game's data; `density` scales every
    /// density (testing).
    pub fn new(seed: u64, density: f64) -> Self {
        let data: DataFile =
            serde_json::from_str(DATA).expect("ground_cover/standard.json is valid");
        let palette = Palette::parse(PALETTE).expect("the block palette is valid");
        // Variants, built on threads (a few hundred block models).
        let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
        let jobs: Vec<(usize, u32)> = data
            .species
            .iter()
            .enumerate()
            .flat_map(|(i, s)| (0..s.variants).map(move |v| (i, v)))
            .collect();
        let chunk = jobs.len().div_ceil(threads).max(1);
        let built: Vec<(usize, u32, Variant)> = std::thread::scope(|scope| {
            let handles: Vec<_> = jobs
                .chunks(chunk)
                .map(|part| {
                    let (species, palette) = (&data.species, &palette);
                    scope.spawn(move || {
                        part.iter()
                            .map(|&(i, v)| (i, v, Self::variant(&species[i], v, seed, palette)))
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            handles
                .into_iter()
                .flat_map(|h| h.join().expect("cover variants"))
                .collect()
        });
        let mut variants: Vec<Vec<Variant>> = data.species.iter().map(|_| Vec::new()).collect();
        let mut sorted = built;
        sorted.sort_by_key(|(i, v, _)| (*i, *v));
        for (i, _, var) in sorted {
            variants[i].push(var);
        }
        let index: HashMap<&str, usize> = data
            .species
            .iter()
            .enumerate()
            .map(|(i, s)| (s.id.as_str(), i))
            .collect();
        let mut recipe_index = HashMap::new();
        let mut recipes = Vec::new();
        let mut names: Vec<&String> = data.recipes.keys().collect();
        names.sort();
        for name in names {
            let entries: Vec<(usize, Entry)> = data.recipes[name]
                .iter()
                .filter_map(|e| index.get(e.species.as_str()).map(|&i| (i, e.clone())))
                .collect();
            recipe_index.insert(name.clone(), recipes.len());
            recipes.push(entries);
        }
        let mut spacing = vec![f64::INFINITY; data.species.len()];
        let mut max_keep = vec![0.0f64; data.species.len()];
        for r in &recipes {
            for (i, e) in r {
                spacing[*i] = spacing[*i].min(e.spacing_m);
            }
        }
        for r in &recipes {
            for (i, e) in r {
                let s = spacing[*i];
                let p = e.density_per_ha * density * s * s / 10_000.0
                    * (1.0 + e.share)
                    * (1.0 + e.cluster.strength);
                max_keep[*i] = max_keep[*i].max(p.min(1.0));
            }
        }
        Self {
            species: data.species,
            variants,
            recipes,
            recipe_index,
            rules: data.rules,
            spacing,
            max_keep,
            seed: mix64(seed ^ 0xC0FE_7E55),
            density,
        }
    }

    fn variant(s: &SpeciesData, v: u32, seed: u64, palette: &Palette) -> Variant {
        let vseed = mix64(
            seed ^ mix64(u64::from(v) * 7919 + s.id.len() as u64 * 104_729)
                ^ s.id.bytes().fold(0u64, |a, b| mix64(a ^ u64::from(b))),
        );
        let mut rng = generate::Rng::new(vseed);
        let height = rng.range(s.height_m[0], s.height_m[1]);
        let width = rng.range(s.width_m[0], s.width_m[1]);
        let params = Params {
            height,
            width,
            has_accent: s.colours.contains_key("accent") && rng.unit() < 0.75,
        };
        let (vox, accent) = generate::build(&s.generator, s.block_m, &params, vseed);
        let up = ups(&s.generator);
        let l1 = coarsen(&vox.cells);
        let l2 = coarsen(&l1);
        let meshes = [
            mesh_cells(&vox.cells, &up, s.block_m),
            mesh_cells(&l1, &up, s.block_m * 2.0),
            mesh_cells(&l2, &up, s.block_m * 4.0),
        ];
        let radius = vox
            .cells
            .keys()
            .map(|c| ((c.as_dvec3().truncate() + 0.5) * s.block_m).length())
            .fold(0.0, f64::max);
        // Role colours: the species' names, with sensible fallbacks.
        let name = |role: Role| -> String {
            if role == Role::Accent
                && let Some(a) = accent
            {
                return a.to_string();
            }
            if role == Role::Centre {
                return if accent == Some("yellow") {
                    "white"
                } else {
                    "yellow"
                }
                .to_string();
            }
            let fallback = match role {
                Role::Wood | Role::Wood2 => "bark",
                Role::Rock | Role::RockHi | Role::RockDark => "rock",
                Role::Moss => "moss",
                Role::Accent => "red",
                _ => "leaf",
            };
            s.colours
                .get(role.key())
                .cloned()
                .or_else(|| match role {
                    Role::LeafHi | Role::LeafDark | Role::LeafVar => s.colours.get("leaf").cloned(),
                    Role::RockHi | Role::RockDark => s.colours.get("rock").cloned(),
                    Role::Wood2 => s.colours.get("wood").cloned(),
                    _ => None,
                })
                .unwrap_or_else(|| fallback.to_string())
        };
        let mut colours = [[1.0f32; 3]; 13];
        for role in Role::ALL {
            colours[role as usize] = palette.linear(&name(role));
        }
        Variant {
            meshes,
            height: vox.top(),
            radius,
            colours,
        }
    }

    /// A variant's mesh at voxel level `lod` (0–2) with its colours (untinted, linear) for
    /// review: positions, colours, triangle indices.
    pub fn variant_mesh(
        &self,
        i: usize,
        v: usize,
        lod: usize,
    ) -> (Vec<[f32; 3]>, Vec<[f32; 3]>, Vec<u32>) {
        let var = &self.variants[i][v];
        let m = &var.meshes[lod.min(2)];
        let cols = m
            .colours
            .iter()
            .map(|&c| var.colours[Role::from_index(c) as usize])
            .collect();
        (m.positions.clone(), cols, m.indices.clone())
    }

    /// The number of species.
    pub fn species_count(&self) -> usize {
        self.species.len()
    }

    /// Species `i`'s data.
    pub fn species(&self, i: usize) -> &SpeciesData {
        &self.species[i]
    }

    /// The tallest variant of species `i` (m).
    pub fn max_height(&self, i: usize) -> f64 {
        self.variants[i]
            .iter()
            .map(|v| v.height)
            .fold(0.0, f64::max)
    }

    /// Whether species `i` is drawn in chunks of `level`: things smaller than 0.6 m only at the
    /// finest level, else while they are at least 2^level × 0.25 m tall.
    fn drawn_at(&self, i: usize, level: u8) -> bool {
        let h = self.species[i].height_m[1];
        if h < 0.7 {
            return level == 0;
        }
        h >= 0.25 * f64::from(1u32 << level)
    }

    /// The land at grid point `p` of `face`, read with detail `detail` (m).
    fn land(&self, g: &BlockGround, globe: &GlobeWorld, face: u8, p: DVec2, detail: f64) -> Land {
        let d = 1.0f64.max(detail);
        let h0 = g.top_height(face, p, detail);
        let hx = g.top_height(face, p + DVec2::new(d, 0.0), detail);
        let hy = g.top_height(face, p + DVec2::new(0.0, d), detail);
        let grad = DVec2::new(hx - h0, hy - h0) / d;
        let slope_deg = grad.length().atan().to_degrees();
        let dir = g.dir_on(face, p.x, p.y);
        let (east, north) = g.face_axes(face, dir);
        let normal = (dir - east * grad.x - north * grad.y).normalize();
        let temperature = globe.temperature(dir, h0);
        let wet = globe.wetness(dir);
        let r = &self.rules;
        let recipe = if h0 < 0.3 {
            None
        } else if slope_deg > r.steep_slope_deg {
            self.recipe_index.get("steep").copied()
        } else if h0 < r.shore_height_m {
            self.recipe_index.get("shore").copied()
        } else {
            let jitter = gradient(dir * 900.0, self.seed ^ 0x51) * 0.5;
            let cover = globe.ground_cover(dir, h0, jitter);
            let id = match cover {
                Cover::DeepSea | Cover::Sea | Cover::Shelf => {
                    return Land {
                        height: h0,
                        slope_deg,
                        temperature,
                        wet,
                        recipe: None,
                        normal,
                    };
                }
                Cover::Lake | Cover::River => "shore",
                Cover::Salt => "salt",
                Cover::Ice => "ice",
                Cover::Snow => "snow",
                Cover::Tundra => "tundra",
                Cover::Taiga => "taiga",
                Cover::ColdDesert => "colddesert",
                Cover::Desert => "desert",
                Cover::Steppe => "steppe",
                Cover::Grass => "grass",
                Cover::Forest => "forest",
                Cover::Rainforest => "rainforest",
                Cover::Savanna => "savanna",
                Cover::Seasonal => "seasonal",
                Cover::Jungle => "jungle",
            };
            let high = h0 > 1_000.0
                && temperature < r.high_temperature_c
                && !matches!(cover, Cover::Ice | Cover::Snow);
            self.recipe_index
                .get(if high { "high" } else { id })
                .copied()
        };
        Land {
            height: h0,
            slope_deg,
            temperature,
            wet,
            recipe,
            normal,
        }
    }

    /// The things in chunk `key` of the ground (a level-of-detail chunk), deterministic.
    pub fn instances(&self, g: &BlockGround, key: LodKey) -> Vec<Instance> {
        let Some(globe) = g.globe() else {
            return Vec::new();
        };
        let (level, face, ci, cj) = key;
        if level > MAX_LEVEL {
            return Vec::new();
        }
        let size = BlockGround::lod_size(level);
        let (x0, y0) = (f64::from(ci) * size, f64::from(cj) * size);
        let detail = (crate::blocks::CELL * f64::from(1u32 << level)).max(0.5);
        let mut out = Vec::new();
        // All sea (the corners and the middle under water): nothing grows.
        let probe = [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0), (0.5, 0.5)];
        if probe.iter().all(|&(u, v)| {
            g.top_height(face, DVec2::new(x0 + u * size, y0 + v * size), size * 0.25) < -20.0
        }) {
            return out;
        }
        // The land read once per patch of ground (shared by every species there).
        let quantum = detail.max(1.0);
        let mut lands: HashMap<(i64, i64), Land> = HashMap::new();
        for si in 0..self.species.len() {
            if !self.drawn_at(si, level) || self.max_keep[si] <= 0.0 {
                continue;
            }
            let s = self.spacing[si];
            let salt = mix64(self.seed ^ ((si as u64 + 1) * 0x9E37_79B9));
            // Far away, dense kinds are thinned (their crowns a little larger).
            let dense = self
                .recipes
                .iter()
                .flatten()
                .filter(|(i, _)| *i == si)
                .map(|(_, e)| e.density_per_ha)
                .fold(0.0, f64::max)
                > 20.0;
            let keep = match (dense, level) {
                (true, 3) => 0.35,
                (true, 4..) => 0.12,
                (true, 2) => 0.7,
                _ => 1.0,
            };
            let (a0, a1) = ((x0 / s).floor() as i64, ((x0 + size) / s).ceil() as i64);
            let (b0, b1) = ((y0 / s).floor() as i64, ((y0 + size) / s).ceil() as i64);
            for b in b0..b1 {
                for a in a0..a1 {
                    let h = hash3(a, b, i64::from(face), salt);
                    let u = unit(h);
                    if u >= self.max_keep[si] {
                        continue;
                    }
                    let h2 = mix64(h);
                    let h3 = mix64(h2);
                    let p = DVec2::new(
                        (a as f64 + 0.1 + 0.8 * unit(h2)) * s,
                        (b as f64 + 0.1 + 0.8 * unit(h3)) * s,
                    );
                    if p.x < x0 || p.y < y0 || p.x >= x0 + size || p.y >= y0 + size {
                        continue;
                    }
                    let h4 = mix64(h3);
                    if unit(h4) >= keep {
                        continue;
                    }
                    if g.cleared(face, p) {
                        continue;
                    }
                    let qk = (
                        (p.x / quantum).floor() as i64,
                        (p.y / quantum).floor() as i64,
                    );
                    let land = lands.entry(qk).or_insert_with(|| {
                        let q = DVec2::new(
                            (qk.0 as f64 + 0.5) * quantum,
                            (qk.1 as f64 + 0.5) * quantum,
                        );
                        self.land(g, globe, face, q, detail)
                    });
                    let Some(r) = land.recipe else { continue };
                    let Some((_, e)) = self.recipes[r].iter().find(|(i, _)| *i == si) else {
                        continue;
                    };
                    if land.slope_deg > e.slope_max_deg
                        || e.min_height_m.is_some_and(|m| land.height < m)
                        || e.max_height_m.is_some_and(|m| land.height > m)
                    {
                        continue;
                    }
                    let kind = self.species[si].generator.as_str();
                    if TREES.contains(&kind)
                        && land.temperature < self.rules.tree_line_temperature_c
                    {
                        continue;
                    }
                    if kind == "palm" && land.temperature < self.rules.warm_shore_temperature_c {
                        continue;
                    }
                    // Clustering (stands and clearings) and the wandering species mix.
                    let n = gradient(
                        DVec3::new(
                            p.x / e.cluster.scale_m,
                            p.y / e.cluster.scale_m,
                            f64::from(face),
                        ),
                        salt ^ 0xC1,
                    );
                    let cluster = (1.0 - e.cluster.strength)
                        + e.cluster.strength * 2.0 * (0.5 + 0.9 * n).clamp(0.0, 1.0);
                    let m = gradient(
                        DVec3::new(p.x / 450.0, p.y / 450.0, f64::from(face) + 7.0),
                        salt ^ 0x5A,
                    );
                    let share = (1.0 + e.share * m).max(0.0);
                    let prob = e.density_per_ha * self.density * s * s / 10_000.0 * cluster * share;
                    if u >= prob {
                        continue;
                    }
                    let h5 = mix64(h4);
                    let h6 = mix64(h5);
                    let h7 = mix64(h6);
                    let sp = &self.species[si];
                    let variant = (h5 % u64::from(sp.variants.max(1))) as u16;
                    let mut scale = 1.0 + sp.scale_spread * (2.0 * unit(h6) - 1.0);
                    if keep < 1.0 {
                        scale *= keep.powf(-0.12);
                    }
                    // Footing: the lowest ground under the footprint (near), so nothing floats.
                    let var = &self.variants[si][variant as usize];
                    let foot = (var.radius * scale * 0.4).min(1.5);
                    let mut height = land.height;
                    if level <= 1 && foot > 0.3 && !sp.tilt_to_slope {
                        for k in 0..4 {
                            let ang = f64::from(k) * std::f64::consts::FRAC_PI_2;
                            let q = p + DVec2::new(ang.cos(), ang.sin()) * foot;
                            height = height.min(g.top_height(face, q, detail));
                        }
                    }
                    let tint_n = gradient(
                        DVec3::new(p.x / 70.0, p.y / 70.0, f64::from(face) + 3.0),
                        salt ^ 0x7,
                    );
                    let jit = 2.0 * unit(h7) - 1.0;
                    let dir = g.dir_on(face, p.x, p.y);
                    out.push(Instance {
                        species: si as u16,
                        variant,
                        at: p,
                        height,
                        yaw: unit(mix64(h7)) * std::f64::consts::TAU,
                        scale,
                        up: if sp.tilt_to_slope { land.normal } else { dir },
                        hue: sp.tint.hue * (0.7 * tint_n + 0.3 * jit),
                        value: 1.0 + sp.tint.value * (0.7 * tint_n + 0.3 * jit),
                        dry: ((0.5 - land.wet) * 2.0).clamp(0.0, 1.0),
                    });
                }
            }
        }
        out
    }

    /// The cover of chunk `key` as one mesh in the chunk's own frame (the ground chunk's), or
    /// `None` when it has nothing.
    pub fn chunk_mesh(&self, g: &BlockGround, key: LodKey) -> Option<CoverMesh> {
        let instances = self.instances(g, key);
        if instances.is_empty() {
            return None;
        }
        let (origin, basis) = g.chunk_frame(key);
        let inverse = basis.inverse();
        // Voxels merged with the level: each about the size a ground cell is drawn at.
        let lod = usize::from(key.0.min(2));
        let face = key.1;
        let mut m = CoverMesh {
            instances: instances.len(),
            ..CoverMesh::default()
        };
        for inst in &instances {
            let sp = &self.species[inst.species as usize];
            let var = &self.variants[inst.species as usize][inst.variant as usize];
            let mesh = &var.meshes[lod];
            if mesh.is_empty() {
                continue;
            }
            let dir = g.dir_on(face, inst.at.x, inst.at.y);
            let up = inst.up;
            let (e0, _) = g.face_axes(face, dir);
            let e = (e0 - up * e0.dot(up)).normalize();
            let n = up.cross(e);
            let rot = DQuat::from_axis_angle(up, inst.yaw);
            let (e, n) = (rot * e, rot * n);
            let sink = sp.sink * var.height * inst.scale;
            let anchor = dir * (g.radius + inst.height) - up * sink;
            let to_chunk = |v: DVec3| (inverse * (v - origin)).as_vec3();
            // Colours per role for this instance.
            let mut colours = [[0.0f32; 4]; 13];
            for role in Role::ALL {
                let mut c = var.colours[role as usize];
                if role == Role::Moss && inst.dry > 0.4 {
                    c = var.colours[Role::RockHi as usize];
                }
                let (mut dh, mut ds) = (inst.hue, 1.0);
                if role.leafy() {
                    dh -= 0.05 * inst.dry;
                    ds *= 1.0 - 0.3 * inst.dry;
                }
                let c = adjust(c, dh, ds, inst.value);
                colours[role as usize] = [c[0], c[1], c[2], 1.0];
            }
            let first = m.positions.len() as u32;
            let sc = inst.scale;
            for (p, nn) in mesh.positions.iter().zip(&mesh.normals) {
                let local = DVec3::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2])) * sc;
                let world = anchor + e * local.x + n * local.y + up * local.z;
                m.positions.push(to_chunk(world).to_array());
                let wn = e * f64::from(nn[0]) + n * f64::from(nn[1]) + up * f64::from(nn[2]);
                m.normals
                    .push((inverse * wn).as_vec3().normalize_or(Vec3::Y).to_array());
            }
            for &c in &mesh.colours {
                m.colours.push(colours[Role::from_index(c) as usize]);
            }
            m.indices.extend(mesh.indices.iter().map(|i| i + first));
        }
        (!m.indices.is_empty()).then_some(m)
    }
}

/// The cover of a body, shared.
pub type SharedCover = Arc<CoverWorld>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variants_differ_stay_in_range_and_repeat() {
        let a = CoverWorld::new(11, 1.0);
        let b = CoverWorld::new(11, 1.0);
        assert!(a.species_count() >= 30);
        for i in 0..a.species_count() {
            let s = a.species(i);
            assert_eq!(a.variants[i].len(), s.variants as usize, "{}", s.id);
            let hs: Vec<f64> = a.variants[i].iter().map(|v| v.height).collect();
            for (k, v) in a.variants[i].iter().enumerate() {
                assert!(!v.meshes[0].is_empty(), "{} variant {k} is empty", s.id);
                assert!(
                    v.height <= s.height_m[1] * 1.3 + 1.0,
                    "{} variant {k} is {:.1} m tall",
                    s.id,
                    v.height
                );
                // Deterministic.
                assert_eq!(
                    v.meshes[0].triangles(),
                    b.variants[i][k].meshes[0].triangles(),
                    "{} variant {k} not deterministic",
                    s.id
                );
                // Coarser levels have no more triangles.
                assert!(
                    v.meshes[2].triangles() <= v.meshes[0].triangles() + 12,
                    "{}",
                    s.id
                );
            }
            // Variants differ.
            let distinct = a.variants[i]
                .iter()
                .map(|v| v.meshes[0].triangles())
                .collect::<std::collections::HashSet<_>>()
                .len();
            assert!(distinct * 2 >= hs.len(), "{} variants too alike", s.id);
        }
    }
}
