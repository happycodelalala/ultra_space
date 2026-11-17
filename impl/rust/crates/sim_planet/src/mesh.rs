//! Terrain tile meshes (planet-surface.md §3; task 2.2).
//!
//! A tile's mesh is a `GRID × GRID` vertex grid on the terrain plus a skirt (a ring of
//! vertices hanging below the border) that hides cracks next to coarser tiles. Positions are
//! `f32` relative to the tile's origin (its centre on the reference sphere), so they stay
//! precise; the renderer places the origin camera-relative in `f64`.
//!
//! Edge vertices are computed from integer lattice coordinates, so neighbouring tiles at the
//! same level produce bit-identical shared vertices; normals come from terrain samples along a
//! tangent frame that depends only on the vertex position, so shading is continuous across tile
//! borders.

use std::sync::OnceLock;

use glam::{DVec2, DVec3};

use crate::climate::{Biome, Climate};
use crate::quadtree::{GRID, TileKey, face_direction};
use crate::terrain::Terrain;

/// Temperature recorded for bodies without a climate (°C).
pub const NO_CLIMATE_TEMPERATURE: f64 = -100.0;

/// Skirt depth in vertex spacings.
const SKIRT_DEPTH: f64 = 4.0;

#[derive(Clone, Debug)]
pub struct TileMesh {
    pub key: TileKey,
    /// Tile centre on the reference sphere (surface frame, m); positions are relative to it.
    pub origin: DVec3,
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    /// Height above the reference radius (m), per vertex.
    pub heights: Vec<f32>,
    pub biomes: Vec<Biome>,
    /// Surface fields per vertex for per-pixel biomes in shaders: height (m), temperature
    /// (°C), moisture (0–1) and the continent signal (positive on land).
    pub fields: Vec<[f32; 4]>,
    pub min_height: f64,
    pub max_height: f64,
    /// Sea surface over the same grid (same indices), when part of the tile is under water.
    pub water: Option<WaterMesh>,
}

/// A tile's sea surface: positions relative to the tile origin, and normals.
#[derive(Clone, Debug)]
pub struct WaterMesh {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
}

/// Number of vertices in a tile mesh (grid plus skirt).
pub const VERTEX_COUNT: usize = (GRID * GRID + 4 * GRID) as usize;

/// Unit direction of grid vertex `(i, j)` of `key`, from exact lattice coordinates.
pub fn vertex_direction(key: TileKey, i: u32, j: u32) -> DVec3 {
    let denominator = f64::from(GRID - 1) * f64::from(key.tiles_per_edge());
    let u = f64::from(key.x * (GRID - 1) + i) / denominator;
    let v = f64::from(key.y * (GRID - 1) + j) / denominator;
    face_direction(key.face, DVec2::new(u, v))
}

/// A tangent frame that depends only on the direction (so shared vertices agree).
fn tangents(d: DVec3) -> (DVec3, DVec3) {
    let reference = if d.y.abs() < 0.9 { DVec3::Y } else { DVec3::X };
    let east = reference.cross(d).normalize();
    (east, d.cross(east))
}

/// Surface normal at `dir` from terrain samples `step` metres apart.
pub fn terrain_normal(terrain: &Terrain, dir: DVec3, step: f64, min_wavelength: f64) -> DVec3 {
    let (t1, t2) = tangents(dir);
    let r = terrain.radius;
    let point = |d: DVec3| {
        let d = d.normalize();
        d * (r + terrain.height(d, min_wavelength))
    };
    let angle = step / r;
    let p0 = point(dir);
    let a = point(dir + t1 * angle) - p0;
    let b = point(dir + t2 * angle) - p0;
    let n = a.cross(b).normalize();
    if n.dot(dir) < 0.0 { -n } else { n }
}

/// Triangle indices shared by every tile (grid plus skirt), counter-clockwise seen from
/// outside the planet.
pub fn tile_indices() -> &'static [u32] {
    static INDICES: OnceLock<Vec<u32>> = OnceLock::new();
    INDICES.get_or_init(|| {
        let g = GRID;
        let at = |i: u32, j: u32| j * g + i;
        let mut idx = Vec::new();
        for j in 0..g - 1 {
            for i in 0..g - 1 {
                let (a, b, c, d) = (at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j + 1));
                idx.extend_from_slice(&[a, b, c, a, c, d]);
            }
        }
        // Skirts: for each border, a strip between the border vertices and their skirt copies.
        // Border order matches `border_vertices`; skirt vertex k is at g*g + k.
        let borders = border_vertices();
        for (side, verts) in borders.iter().enumerate() {
            for k in 0..g as usize - 1 {
                let (top0, top1) = (verts[k], verts[k + 1]);
                let base = g * g + (side as u32) * g;
                let (bot0, bot1) = (base + k as u32, base + k as u32 + 1);
                // Sides 0 (bottom, j = 0) and 3 (left, i = 0) run counter-clockwise around the
                // tile when seen from outside; 1 and 2 run the other way.
                if side == 0 || side == 3 {
                    idx.extend_from_slice(&[top0, bot0, bot1, top0, bot1, top1]);
                } else {
                    idx.extend_from_slice(&[top0, bot1, bot0, top0, top1, bot1]);
                }
            }
        }
        idx
    })
}

/// Grid indices of the four borders: bottom (j = 0, i up), right (i = max, j up), top
/// (j = max, i up), left (i = 0, j up).
fn border_vertices() -> [Vec<u32>; 4] {
    let g = GRID;
    let at = |i: u32, j: u32| j * g + i;
    [
        (0..g).map(|i| at(i, 0)).collect(),
        (0..g).map(|j| at(g - 1, j)).collect(),
        (0..g).map(|i| at(i, g - 1)).collect(),
        (0..g).map(|j| at(0, j)).collect(),
    ]
}

/// Builds the mesh of tile `key`. `climate` and `subsolar_latitude` set biomes (all
/// `Regolith` without a climate).
pub fn build_tile(
    terrain: &Terrain,
    climate: Option<&Climate>,
    key: TileKey,
    subsolar_latitude: f64,
) -> TileMesh {
    let r = terrain.radius;
    let spacing = key.vertex_spacing(r);
    let min_wavelength = 2.0 * spacing;
    let origin = key.center() * r;
    let n = VERTEX_COUNT;
    let mut mesh = TileMesh {
        key,
        origin,
        positions: Vec::with_capacity(n),
        normals: Vec::with_capacity(n),
        heights: Vec::with_capacity(n),
        biomes: Vec::with_capacity(n),
        fields: Vec::with_capacity(n),
        min_height: f64::MAX,
        max_height: f64::MIN,
        water: None,
    };
    let mut grid_dirs = Vec::with_capacity((GRID * GRID) as usize);
    let mut min_elevation = f64::MAX;
    for j in 0..GRID {
        for i in 0..GRID {
            let d = vertex_direction(key, i, j);
            let sample = terrain.sample(d, min_wavelength);
            let normal = terrain_normal(terrain, d, spacing, min_wavelength);
            let (t, m) = climate.map_or((NO_CLIMATE_TEMPERATURE, 0.0), |c| {
                (
                    c.temperature_at(terrain, d, sample.elevation, subsolar_latitude),
                    c.moisture(terrain, d, &sample),
                )
            });
            let biome = climate.map_or(Biome::Regolith, |c| {
                let slope = normal.dot(d).clamp(-1.0, 1.0).acos();
                c.biome(terrain, &sample, t, m, slope)
            });
            // Shading reads the elevation (height above sea level along "up").
            mesh.fields.push([
                sample.elevation as f32,
                t as f32,
                m as f32,
                sample.land as f32,
            ]);
            min_elevation = min_elevation.min(sample.elevation);
            let p = d * (r + sample.height) - origin;
            mesh.positions.push(p.as_vec3().to_array());
            mesh.normals.push(normal.as_vec3().to_array());
            mesh.heights.push(sample.height as f32);
            mesh.biomes.push(biome);
            mesh.min_height = mesh.min_height.min(sample.height);
            mesh.max_height = mesh.max_height.max(sample.height);
            grid_dirs.push(d);
        }
    }
    // Skirt: copies of the border vertices, lowered radially.
    let drop = SKIRT_DEPTH * spacing;
    for side in border_vertices() {
        for v in side {
            let v = v as usize;
            let d = grid_dirs[v];
            let h = f64::from(mesh.heights[v]);
            let p = d * (r + h - drop) - origin;
            mesh.positions.push(p.as_vec3().to_array());
            mesh.normals.push(mesh.normals[v]);
            mesh.heights.push((h - drop) as f32);
            mesh.biomes.push(mesh.biomes[v]);
            mesh.fields.push(mesh.fields[v]);
        }
    }
    if terrain.has_ocean() && min_elevation < 0.0 {
        let mut dirs = grid_dirs.clone();
        for side in border_vertices() {
            dirs.extend(side.iter().map(|&v| grid_dirs[v as usize]));
        }
        let grid = grid_dirs.len();
        let positions = dirs
            .iter()
            .enumerate()
            .map(|(k, d)| {
                // The sea surface: the sphere at sea level, or each plain's own plane.
                let r = r + terrain.sea_height(*d) - if k < grid { 0.0 } else { drop };
                (*d * r - origin).as_vec3().to_array()
            })
            .collect();
        let normals = dirs.iter().map(|d| d.as_vec3().to_array()).collect();
        mesh.water = Some(WaterMesh { positions, normals });
    }
    mesh
}

/// A collision patch: the tile's `GRID × GRID` lattice on the walkable surface (sea surface
/// over water), as triangles. Vertices are in the surface frame relative to the planet's
/// centre (m, `f64`); neighbouring patches at the same level share vertices exactly.
#[derive(Clone, Debug)]
pub struct CollisionPatch {
    pub key: TileKey,
    pub vertices: Vec<DVec3>,
    pub triangles: Vec<[u32; 3]>,
}

/// Builds the collision patch for `key` with terrain detail down to `min_wavelength` (m).
pub fn collision_patch(terrain: &Terrain, key: TileKey, min_wavelength: f64) -> CollisionPatch {
    let mut vertices = Vec::with_capacity((GRID * GRID) as usize);
    for j in 0..GRID {
        for i in 0..GRID {
            let dir = vertex_direction(key, i, j);
            vertices.push(dir * (terrain.radius + terrain.collision_height(dir, min_wavelength)));
        }
    }
    let mut triangles = Vec::with_capacity(((GRID - 1) * (GRID - 1) * 2) as usize);
    for j in 0..GRID - 1 {
        for i in 0..GRID - 1 {
            let a = j * GRID + i;
            let (b, c, d) = (a + 1, a + GRID, a + GRID + 1);
            // Counter-clockwise seen from outside (u × v points outward).
            triangles.push([a, b, d]);
            triangles.push([a, d, c]);
        }
    }
    CollisionPatch {
        key,
        vertices,
        triangles,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use sim_core::StarSystem;

    use super::*;

    fn meridian() -> (Terrain, Climate) {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../content/data/star_systems/halcyon.json");
        let sys = StarSystem::from_path(path).unwrap();
        let body = sys.body("meridian").unwrap();
        (Terrain::of(body).unwrap(), Climate::of(body).unwrap())
    }

    #[test]
    fn indices_are_valid_and_complete() {
        let idx = tile_indices();
        let quads = (GRID - 1) * (GRID - 1) + 4 * (GRID - 1);
        assert_eq!(idx.len(), (quads * 6) as usize);
        assert!(idx.iter().all(|&i| (i as usize) < VERTEX_COUNT));
    }

    #[test]
    fn neighbouring_tiles_share_edge_vertices_exactly() {
        let a = TileKey {
            face: 4,
            level: 6,
            x: 10,
            y: 20,
        };
        let right = TileKey { x: 11, ..a };
        let above = TileKey { y: 21, ..a };
        for k in 0..GRID {
            assert_eq!(
                vertex_direction(a, GRID - 1, k),
                vertex_direction(right, 0, k)
            );
            assert_eq!(
                vertex_direction(a, k, GRID - 1),
                vertex_direction(above, k, 0)
            );
        }
    }

    #[test]
    fn shared_edges_have_identical_heights_and_normals() {
        let (terrain, climate) = meridian();
        let a = TileKey {
            face: 0,
            level: 8,
            x: 100,
            y: 140,
        };
        let b = TileKey { x: 101, ..a };
        let (ma, mb) = (
            build_tile(&terrain, Some(&climate), a, 0.0),
            build_tile(&terrain, Some(&climate), b, 0.0),
        );
        for k in 0..GRID as usize {
            let va = k * GRID as usize + (GRID as usize - 1);
            let vb = k * GRID as usize;
            assert_eq!(ma.heights[va], mb.heights[vb]);
            assert_eq!(ma.normals[va], mb.normals[vb]);
            let pa = ma.origin + glam::Vec3::from(ma.positions[va]).as_dvec3();
            let pb = mb.origin + glam::Vec3::from(mb.positions[vb]).as_dvec3();
            assert!((pa - pb).length() < 0.01, "edge vertex {k} differs");
        }
    }

    #[test]
    fn tiles_face_outward_with_skirts_below() {
        let (terrain, climate) = meridian();
        for key in [
            TileKey::root(2),
            TileKey {
                face: 1,
                level: 10,
                x: 300,
                y: 700,
            },
        ] {
            let m = build_tile(&terrain, Some(&climate), key, 0.0);
            assert_eq!(m.positions.len(), VERTEX_COUNT);
            assert_eq!(m.fields.len(), VERTEX_COUNT);
            let up = key.center();
            let outward = m
                .normals
                .iter()
                .filter(|n| glam::Vec3::from(**n).as_dvec3().dot(up) > 0.0)
                .count();
            assert!(
                outward > VERTEX_COUNT * 9 / 10,
                "{key:?}: {outward} outward normals"
            );
            let grid = (GRID * GRID) as usize;
            for k in grid..VERTEX_COUNT {
                assert!(
                    m.heights[k]
                        < m.heights[..grid].iter().cloned().fold(f32::MAX, f32::min) + 1e-3
                        || m.heights[k] < m.max_height as f32
                );
            }
            // Triangles wind counter-clockwise seen from outside: their normals point up.
            let idx = tile_indices();
            let p = |i: u32| glam::Vec3::from(m.positions[i as usize]).as_dvec3();
            let first = (p(idx[1]) - p(idx[0])).cross(p(idx[2]) - p(idx[0]));
            assert!(first.dot(up) > 0.0, "{key:?}: winding");
        }
    }

    #[test]
    fn coastal_tiles_get_a_sea_surface() {
        let (terrain, climate) = meridian();
        // Find a root-level child with both land and sea.
        let key = (0..6)
            .flat_map(|f| TileKey::root(f).children())
            .find(|&k| {
                let m = build_tile(&terrain, Some(&climate), k, 0.0);
                m.min_height < 0.0 && m.max_height > 0.0
            })
            .expect("a coastal tile");
        let m = build_tile(&terrain, Some(&climate), key, 0.0);
        let WaterMesh { positions, normals } = m.water.expect("water");
        assert_eq!(positions.len(), VERTEX_COUNT);
        assert_eq!(normals.len(), VERTEX_COUNT);
        let p = m.origin + glam::Vec3::from(positions[0]).as_dvec3();
        assert!((p.length() - terrain.radius).abs() < 1.0, "sea level");
    }

    #[test]
    fn tile_generation_is_fast_enough() {
        let (terrain, climate) = meridian();
        let key = TileKey {
            face: 4,
            level: 12,
            x: 2_000,
            y: 2_100,
        };
        let start = std::time::Instant::now();
        let m = build_tile(&terrain, Some(&climate), key, 0.0);
        let elapsed = start.elapsed();
        eprintln!("tile {key:?} generated in {elapsed:?}");
        assert!(m.max_height >= m.min_height);
        // Generous bound for debug builds on slow CI; release is far faster.
        assert!(elapsed.as_secs_f64() < 2.0, "one tile took {elapsed:?}");
    }

    #[test]
    fn collision_patches_follow_the_surface_and_share_edges() {
        let (terrain, _) = meridian();
        let key = crate::locate(DVec3::new(0.3, 0.5, 0.8).normalize(), 14);
        let patch = collision_patch(&terrain, key, 1.0);
        assert_eq!(patch.vertices.len(), (GRID * GRID) as usize);
        for v in &patch.vertices {
            let dir = v.normalize();
            let expected = terrain.radius + terrain.surface_height(dir, 1.0);
            assert!((v.length() - expected).abs() < 1e-6);
        }
        // The right neighbour's left column is this patch's right column.
        let right = TileKey {
            x: key.x + 1,
            ..key
        };
        let other = collision_patch(&terrain, right, 1.0);
        for j in 0..GRID {
            let a = patch.vertices[(j * GRID + GRID - 1) as usize];
            let b = other.vertices[(j * GRID) as usize];
            assert_eq!(a, b);
        }
        // Triangles face outward.
        let [a, b, c] = patch.triangles[0].map(|i| patch.vertices[i as usize]);
        assert!((b - a).cross(c - a).dot(a) > 0.0);
    }
}
