//! Cube-sphere quadtree (planet-surface.md §3; task 2.2).
//!
//! Each of the six cube faces is a quadtree of square tiles. A tile is identified by a
//! [`TileKey`] (its procedural key: content is a pure function of the key and the planet), and
//! covers a square of face coordinates `[0, 1]²` that is mapped onto the sphere with an
//! area-preserving ("spherified cube") mapping, so tiles stay similar in size everywhere.
//!
//! [`select`] picks the tiles to draw for a camera: a tile splits while its vertex spacing,
//! seen from the camera, is larger than a few pixels. Tiles hidden behind the horizon are not
//! refined.

use glam::{DVec2, DVec3};

use crate::terrain::Terrain;

/// Vertices along a tile edge (tiles have `GRID × GRID` vertices, `GRID - 1` quads per edge).
pub const GRID: u32 = 33;
/// Deepest level (vertex spacing on a 600 km planet ≈ 0.9 m).
pub const MAX_LEVEL: u8 = 15;

/// One of the six cube faces: outward normal and the two in-face axes (u, v), right-handed
/// (u × v = normal).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Face {
    pub normal: DVec3,
    pub u: DVec3,
    pub v: DVec3,
}

/// +X, −X, +Y, −Y, +Z, −Z.
pub const FACES: [Face; 6] = [
    Face {
        normal: DVec3::X,
        u: DVec3::NEG_Z,
        v: DVec3::Y,
    },
    Face {
        normal: DVec3::NEG_X,
        u: DVec3::Z,
        v: DVec3::Y,
    },
    Face {
        normal: DVec3::Y,
        u: DVec3::X,
        v: DVec3::NEG_Z,
    },
    Face {
        normal: DVec3::NEG_Y,
        u: DVec3::X,
        v: DVec3::Z,
    },
    Face {
        normal: DVec3::Z,
        u: DVec3::X,
        v: DVec3::Y,
    },
    Face {
        normal: DVec3::NEG_Z,
        u: DVec3::NEG_X,
        v: DVec3::Y,
    },
];

/// A tile's procedural key: face, level and position within the face's grid at that level.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TileKey {
    pub face: u8,
    pub level: u8,
    pub x: u32,
    pub y: u32,
}

impl TileKey {
    pub fn root(face: u8) -> Self {
        Self {
            face,
            level: 0,
            x: 0,
            y: 0,
        }
    }

    /// Tiles per face edge at this level.
    pub fn tiles_per_edge(self) -> u32 {
        1 << self.level
    }

    /// Size of the tile in face coordinates.
    pub fn size(self) -> f64 {
        1.0 / f64::from(self.tiles_per_edge())
    }

    /// Lower-left corner in face coordinates.
    pub fn origin(self) -> DVec2 {
        DVec2::new(f64::from(self.x), f64::from(self.y)) * self.size()
    }

    pub fn children(self) -> [TileKey; 4] {
        let (x, y, level) = (self.x * 2, self.y * 2, self.level + 1);
        [(0, 0), (1, 0), (0, 1), (1, 1)].map(|(dx, dy)| TileKey {
            face: self.face,
            level,
            x: x + dx,
            y: y + dy,
        })
    }

    pub fn parent(self) -> Option<TileKey> {
        (self.level > 0).then(|| TileKey {
            face: self.face,
            level: self.level - 1,
            x: self.x / 2,
            y: self.y / 2,
        })
    }

    /// Unit direction (surface frame) at face coordinates `(s, t)` relative to this tile, each
    /// in `[0, 1]`.
    pub fn direction(self, s: f64, t: f64) -> DVec3 {
        let uv = self.origin() + DVec2::new(s, t) * self.size();
        face_direction(self.face, uv)
    }

    /// Direction of the tile's centre.
    pub fn center(self) -> DVec3 {
        self.direction(0.5, 0.5)
    }

    /// Arc length of a tile edge on a sphere of `radius` (approximate; tiles are nearly
    /// square).
    pub fn edge_length(self, radius: f64) -> f64 {
        let a = self.direction(0.0, 0.5);
        let b = self.direction(1.0, 0.5);
        a.angle_between(b) * radius
    }

    /// Nominal distance between neighbouring vertices on a sphere of `radius`. Depends only
    /// on the level, so neighbouring tiles use exactly the same value.
    pub fn vertex_spacing(self, radius: f64) -> f64 {
        level_spacing(self.level, radius)
    }
}

/// Nominal vertex spacing at `level` on a sphere of `radius` (a face edge is a quarter of a
/// great circle).
pub fn level_spacing(level: u8, radius: f64) -> f64 {
    std::f64::consts::FRAC_PI_2 * radius / (f64::from(GRID - 1) * f64::from(1u32 << level))
}

/// Unit direction for face coordinates `uv ∈ [0, 1]²` on `face`.
pub fn face_direction(face: u8, uv: DVec2) -> DVec3 {
    let f = FACES[face as usize];
    let a = uv * 2.0 - DVec2::ONE;
    let cube = f.normal + f.u * a.x + f.v * a.y;
    spherify(cube)
}

/// The face whose region of the sphere contains `dir` (the cube-to-sphere mapping keeps
/// points within their face's cone: the largest coordinate picks the face).
pub fn face_of(dir: DVec3) -> u8 {
    let a = dir.abs();
    if a.x >= a.y && a.x >= a.z {
        if dir.x >= 0.0 { 0 } else { 1 }
    } else if a.y >= a.z {
        if dir.y >= 0.0 { 2 } else { 3 }
    } else if dir.z >= 0.0 {
        4
    } else {
        5
    }
}

/// Face coordinates `uv ∈ [0, 1]²` of unit direction `dir` on `face` (inverse of
/// [`face_direction`], by Newton's method from the gnomonic projection).
pub fn face_uv(face: u8, dir: DVec3) -> DVec2 {
    let f = FACES[face as usize];
    let n = dir.dot(f.normal).max(1e-9);
    let mut uv = (DVec2::new(dir.dot(f.u), dir.dot(f.v)) / n + DVec2::ONE) * 0.5;
    let error = |uv: DVec2| {
        let d = dir - face_direction(face, uv);
        DVec2::new(d.dot(f.u), d.dot(f.v))
    };
    const H: f64 = 1e-7;
    for _ in 0..8 {
        let e = error(uv);
        if e.length() < 1e-15 {
            break;
        }
        // Jacobian of the error with respect to uv (numerical).
        let du = (error(uv + DVec2::new(H, 0.0)) - e) / H;
        let dv = (error(uv + DVec2::new(0.0, H)) - e) / H;
        let det = du.x * dv.y - dv.x * du.y;
        if det.abs() < 1e-30 {
            break;
        }
        let step = DVec2::new(e.x * dv.y - dv.x * e.y, du.x * e.y - e.x * du.y) / det;
        uv = (uv - step).clamp(DVec2::ZERO, DVec2::ONE);
    }
    uv
}

/// The tile at `level` that contains unit direction `dir`.
pub fn locate(dir: DVec3, level: u8) -> TileKey {
    let face = face_of(dir);
    let uv = face_uv(face, dir);
    let n = 1u32 << level;
    let cell = |c: f64| ((c * f64::from(n)) as u32).min(n - 1);
    TileKey {
        face,
        level,
        x: cell(uv.x),
        y: cell(uv.y),
    }
}

/// The level whose vertex spacing on a sphere of `radius` is closest to (not above) `spacing`.
pub fn level_for_spacing(radius: f64, spacing: f64) -> u8 {
    let mut level = 0;
    while level < 30 && level_spacing(level, radius) > spacing {
        level += 1;
    }
    level
}

/// Area-preserving cube-to-sphere mapping (for points on the unit cube's surface).
pub fn spherify(p: DVec3) -> DVec3 {
    let (x2, y2, z2) = (p.x * p.x, p.y * p.y, p.z * p.z);
    DVec3::new(
        p.x * (1.0 - y2 / 2.0 - z2 / 2.0 + y2 * z2 / 3.0).sqrt(),
        p.y * (1.0 - z2 / 2.0 - x2 / 2.0 + z2 * x2 / 3.0).sqrt(),
        p.z * (1.0 - x2 / 2.0 - y2 / 2.0 + x2 * y2 / 3.0).sqrt(),
    )
    .normalize()
}

/// Default split threshold: vertex spacing seen as larger than this many pixels splits a tile.
/// Each level then leaves about 3π·(ppr / (px · (GRID - 1)))² tiles around the camera (~55 at
/// 1080p), so a whole planet view stays under about a thousand tiles.
pub const DEFAULT_SPACING_PIXELS: f64 = 12.0;

/// What the camera sees, for tile selection. Positions are in the body's surface frame (m).
#[derive(Clone, Copy, Debug)]
pub struct View {
    pub camera: DVec3,
    /// Vertical field of view (radians).
    pub fov_y: f64,
    /// Viewport height (pixels).
    pub viewport_height: f64,
    /// Split while the vertex spacing appears larger than this many pixels.
    pub max_spacing_pixels: f64,
}

impl View {
    /// Pixels per radian of view angle at the screen centre.
    fn pixels_per_radian(&self) -> f64 {
        self.viewport_height / (2.0 * (self.fov_y / 2.0).tan())
    }
}

/// A body's size for tile selection: reference radius and the global height range of its
/// surface (heights relative to the radius; `min_height` is negative below it).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Extent {
    pub radius: f64,
    pub min_height: f64,
    pub max_height: f64,
}

/// The tiles to draw for `view`. `heights` gives each tile's height range (min, max): exact
/// for tiles already generated, otherwise an estimate such as [`estimated_heights`].
pub fn select(
    view: &View,
    extent: &Extent,
    heights: &impl Fn(TileKey) -> (f64, f64),
) -> Vec<TileKey> {
    let mut out = Vec::new();
    for face in 0..6u8 {
        refine(TileKey::root(face), view, extent, heights, &mut out);
    }
    out
}

/// Height range estimate for a tile not generated yet: the terrain height at its centre, plus
/// or minus half its size (terrain is rarely steeper than 45° over a whole tile), within the
/// body's global range.
pub fn estimated_heights(terrain: &Terrain, extent: Extent) -> impl Fn(TileKey) -> (f64, f64) + '_ {
    move |key: TileKey| {
        let spacing = key.vertex_spacing(extent.radius);
        let h = terrain.height(key.center(), 2.0 * spacing);
        let e = 0.5 * spacing * f64::from(GRID - 1);
        (
            (h - e).max(extent.min_height),
            (h + e).min(extent.max_height),
        )
    }
}

/// Bounding sphere of a tile's terrain with heights in `(min, max)`: centre (surface frame)
/// and radius.
pub fn tile_bounds(key: TileKey, radius: f64, heights: (f64, f64)) -> (DVec3, f64) {
    let (lo, hi) = heights;
    let center = key.center() * (radius + 0.5 * (lo + hi));
    let points = [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0), (0.5, 0.5)];
    let mut extent: f64 = 0.0;
    for (s, t) in points {
        let d = key.direction(s, t);
        for r in [radius + lo, radius + hi] {
            extent = extent.max((d * r - center).length());
        }
    }
    (center, extent)
}

/// True if the whole tile is below the horizon seen from `camera`: every point of it is
/// hidden behind the body's lowest possible surface (conservative).
fn below_horizon(key: TileKey, camera: DVec3, extent: &Extent) -> bool {
    let cam_dist = camera.length();
    let occluder = extent.radius + extent.min_height;
    if cam_dist <= occluder {
        return false;
    }
    // A point at distance r from the centre is visible over the horizon if the angle between
    // it and the camera is below acos(occluder / cam) + acos(occluder / r).
    let horizon = (occluder / cam_dist).clamp(-1.0, 1.0).acos()
        + (occluder / (extent.radius + extent.max_height))
            .clamp(-1.0, 1.0)
            .acos();
    // Largest angle from the tile centre to any of its corners.
    let c = key.center();
    let spread = [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)]
        .iter()
        .map(|&(s, t)| c.angle_between(key.direction(s, t)))
        .fold(0.0, f64::max);
    c.angle_between(camera) - spread > horizon
}

fn refine(
    key: TileKey,
    view: &View,
    extent: &Extent,
    heights: &impl Fn(TileKey) -> (f64, f64),
    out: &mut Vec<TileKey>,
) {
    if key.level > 0 && below_horizon(key, view.camera, extent) {
        return;
    }
    if key.level < MAX_LEVEL && wants_split(key, view, extent.radius, heights(key)) {
        for child in key.children() {
            refine(child, view, extent, heights, out);
        }
    } else {
        out.push(key);
    }
}

/// Whether a tile's vertex spacing, seen from the camera, is larger than allowed.
pub fn wants_split(key: TileKey, view: &View, radius: f64, heights: (f64, f64)) -> bool {
    let (center, extent) = tile_bounds(key, radius, heights);
    let distance = ((view.camera - center).length() - extent).max(1.0);
    let spacing = key.vertex_spacing(radius);
    spacing / distance * view.pixels_per_radian() > view.max_spacing_pixels
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn faces_are_right_handed_and_cover_the_sphere() {
        for f in FACES {
            assert!((f.u.cross(f.v) - f.normal).length() < 1e-12, "{f:?}");
        }
        // Every axis direction is the centre of exactly one face.
        for (i, f) in FACES.iter().enumerate() {
            let c = face_direction(i as u8, DVec2::splat(0.5));
            assert!((c - f.normal).length() < 1e-12);
        }
    }

    #[test]
    fn children_tile_their_parent() {
        let key = TileKey {
            face: 3,
            level: 4,
            x: 5,
            y: 9,
        };
        for child in key.children() {
            assert_eq!(child.parent(), Some(key));
        }
        let c = key.children();
        // The children's corners meet the parent's corners and centre.
        assert!((c[0].direction(0.0, 0.0) - key.direction(0.0, 0.0)).length() < 1e-12);
        assert!((c[3].direction(1.0, 1.0) - key.direction(1.0, 1.0)).length() < 1e-12);
        assert!((c[0].direction(1.0, 1.0) - key.center()).length() < 1e-12);
        assert_eq!(TileKey::root(2).parent(), None);
    }

    #[test]
    fn neighbouring_faces_share_their_edges() {
        // Sample points along every face's edges; each must coincide with a point on the
        // adjacent face's edge (no gaps or overlaps between faces).
        for face in 0..6u8 {
            for k in 0..=16 {
                let s = f64::from(k) / 16.0;
                for uv in [
                    DVec2::new(s, 0.0),
                    DVec2::new(s, 1.0),
                    DVec2::new(0.0, s),
                    DVec2::new(1.0, s),
                ] {
                    let p = face_direction(face, uv);
                    let matched = (0..6u8).filter(|&f| f != face).any(|other| {
                        [0.0, 1.0].iter().any(|&edge| {
                            (0..=16).any(|j| {
                                let q = f64::from(j) / 16.0;
                                [DVec2::new(q, edge), DVec2::new(edge, q)]
                                    .iter()
                                    .any(|&o| (face_direction(other, o) - p).length() < 1e-12)
                            })
                        })
                    });
                    assert!(matched, "face {face} edge point {uv:?} has no partner");
                }
            }
        }
    }

    #[test]
    fn mapping_keeps_tiles_similar_in_size() {
        // Area-preserving mapping: corner and centre tiles at the same level differ little.
        let level = 5;
        let n = 1u32 << level;
        let size = |x, y| {
            TileKey {
                face: 0,
                level,
                x,
                y,
            }
            .edge_length(1.0)
        };
        let ratio = size(n / 2, n / 2) / size(0, 0);
        assert!(
            (0.7..1.45).contains(&ratio),
            "centre/corner edge ratio {ratio}"
        );
    }

    fn view(camera: DVec3) -> View {
        View {
            camera,
            fov_y: 60f64.to_radians(),
            viewport_height: 1080.0,
            max_spacing_pixels: DEFAULT_SPACING_PIXELS,
        }
    }

    const R: f64 = 600_000.0;
    const EXTENT: Extent = Extent {
        radius: R,
        min_height: -5_000.0,
        max_height: 8_000.0,
    };
    /// A flat planet at height 0, for selection tests independent of terrain.
    fn flat(_: TileKey) -> (f64, f64) {
        (0.0, 0.0)
    }

    #[test]
    fn far_away_needs_few_tiles_and_close_up_goes_deep() {
        let far = select(&view(DVec3::X * R * 20.0), &EXTENT, &flat);
        assert!(far.len() <= 24, "{} tiles from far away", far.len());

        let close = select(&view(DVec3::X * (R + 2.0)), &EXTENT, &flat);
        let deepest = close.iter().map(|k| k.level).max().unwrap();
        assert_eq!(
            deepest, MAX_LEVEL,
            "standing on the ground reaches the finest level"
        );
        assert!(close.len() < 1_500, "{} tiles on the ground", close.len());
        // Fine tiles are near the camera; the far side stays coarse.
        let under = close.iter().find(|k| k.level == MAX_LEVEL).unwrap();
        assert!(under.center().angle_between(DVec3::X) < 1e-3);
    }

    #[test]
    fn standing_on_real_terrain_selects_a_modest_set() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../content/data/star_systems/halcyon.json");
        let sys = sim_core::StarSystem::from_path(path).unwrap();
        let terrain = Terrain::of(sys.body("hoarfell").unwrap()).unwrap();
        let extent = Extent {
            radius: terrain.radius,
            min_height: -terrain.params.relief,
            max_height: 1.25 * terrain.params.relief,
        };
        // Stand 2 m above the highest of a few sampled spots (likely a mountain side).
        let spot = crate::terrain::fibonacci_directions(400)
            .max_by(|a, b| terrain.height(*a, 1.0).total_cmp(&terrain.height(*b, 1.0)))
            .unwrap();
        let camera = spot * (terrain.radius + terrain.height(spot, 1.0) + 2.0);
        let tiles = select(&view(camera), &extent, &estimated_heights(&terrain, extent));
        let deepest = tiles.iter().map(|k| k.level).max().unwrap();
        assert!(deepest >= MAX_LEVEL - 1, "deepest level {deepest}");
        assert!(tiles.len() < 1_500, "{} tiles", tiles.len());
    }

    #[test]
    fn horizon_hides_the_far_side() {
        let low = select(&view(DVec3::X * (R + 1_000.0)), &EXTENT, &flat);
        // Nothing beyond the horizon is refined: no tile below level 2 far from the camera.
        for k in &low {
            if k.center().angle_between(DVec3::X) > 0.6 {
                assert!(k.level <= 1, "far tile refined: {k:?}");
            }
        }
    }

    #[test]
    fn spacing_at_the_finest_level_is_about_a_metre() {
        let key = TileKey {
            face: 0,
            level: MAX_LEVEL,
            x: 1 << (MAX_LEVEL - 1),
            y: 1 << (MAX_LEVEL - 1),
        };
        let spacing = key.vertex_spacing(R);
        assert!((0.5..1.5).contains(&spacing), "{spacing} m");
    }

    #[test]
    fn locate_finds_the_tile_containing_a_direction() {
        for (n, dir) in crate::fibonacci_directions(2000).enumerate() {
            let level = (n % 16) as u8;
            let key = locate(dir, level);
            let uv = face_uv(key.face, dir);
            assert!(
                face_direction(key.face, uv).distance(dir) < 1e-12,
                "{dir:?}"
            );
            let local = (uv - key.origin()) / key.size();
            assert!(
                (-1e-9..=1.0 + 1e-9).contains(&local.x) && (-1e-9..=1.0 + 1e-9).contains(&local.y),
                "{key:?} {local:?}"
            );
        }
    }

    #[test]
    fn level_for_spacing_reaches_the_spacing() {
        let level = level_for_spacing(600_000.0, 2.0);
        assert!(level_spacing(level, 600_000.0) <= 2.0);
        assert!(level_spacing(level - 1, 600_000.0) > 2.0);
    }
}
