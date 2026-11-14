//! Foundations (living-planet.md §5): the ground levelled under every building of a body's
//! settlements, and the buildings' collision boxes standing on it. Generated from the layouts
//! (never saved), applied on top of the natural terrain and under the player's landing pads.
//!
//! Many flattened spots in one place need a spatial index so terrain sampling stays fast: a
//! sample first finds the few sites (settlements) whose cap holds it (one dot product each),
//! then the footprints in its cell of the site's grid.

use glam::{DMat3, DQuat, DVec2, DVec3};

/// A site's tangent frame at `center` (unit direction, surface frame): x east, y up, z south
/// (right-handed, Y up). East is along the planet's spin (+Y is north).
pub fn tangent_frame(center: DVec3) -> (DVec3, DVec3) {
    let up = center.normalize();
    let east = DVec3::Y
        .cross(up)
        .try_normalize()
        .unwrap_or_else(|| up.any_orthonormal_vector());
    let south = east.cross(up);
    (east, south)
}

/// A levelled rectangle in its site's tangent plane: centre (x east, z south, m), turn about
/// the vertical (radians, counter-clockwise seen from above), half extents along its own x and
/// z, and the ground height inside it (m above the reference radius).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Footprint {
    pub x: f64,
    pub z: f64,
    pub yaw: f64,
    pub half: [f64; 2],
    pub height: f64,
}

impl Footprint {
    /// Distance (m) from the rectangle at tangent-plane point `p` (0 inside), with the turn's
    /// sine and cosine.
    fn distance(&self, p: DVec2, (s, c): (f64, f64)) -> f64 {
        let d = p - DVec2::new(self.x, self.z);
        // Into the footprint's own axes (the inverse turn).
        let local = DVec2::new(c * d.x - s * d.y, s * d.x + c * d.y);
        let out = (local.abs() - DVec2::new(self.half[0], self.half[1])).max(DVec2::ZERO);
        out.length()
    }

    /// Radius (m) of a circle round the centre holding the rectangle and its blend.
    fn reach(&self, blend: f64) -> f64 {
        DVec2::new(self.half[0], self.half[1]).length() + blend
    }
}

/// A building's collision box: centre (surface frame, m), rotation (surface frame; its Y is the
/// local up) and half extents (m).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Structure {
    pub center: DVec3,
    pub rotation: DQuat,
    pub half: DVec3,
}

/// A box placed on a site's tangent plane: centre (x east, height above the reference radius,
/// z south; m), turn about the vertical (radians, counter-clockwise from above), half extents.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlacedBox {
    pub center: DVec3,
    pub yaw: f64,
    pub half: DVec3,
}

/// Grid cell size of a site's index (m).
const CELL: f64 = 16.0;

/// One settlement's foundations.
#[derive(Clone, Debug)]
pub struct Site {
    center: DVec3,
    east: DVec3,
    south: DVec3,
    /// Cosine of the cap's angular radius: samples with `dir · center` below it are outside.
    cos_reach: f64,
    blend: f64,
    footprints: Vec<Footprint>,
    structures: Vec<Structure>,
    /// Each footprint's turn as (sine, cosine).
    turns: Vec<(f64, f64)>,
    /// Grid origin (tangent plane, m), size in cells, and per cell the footprints touching it.
    origin: DVec2,
    size: [usize; 2],
    cells: Vec<Vec<u32>>,
}

impl Site {
    /// Foundations at `center` (unit direction) for `footprints` (tangent plane of `center`),
    /// each easing back to the natural ground over `blend` metres, on a body of `radius`.
    pub fn new(center: DVec3, footprints: Vec<Footprint>, blend: f64, radius: f64) -> Self {
        Self::with_boxes(center, footprints, Vec::new(), blend, radius)
    }

    /// Foundations with the buildings' collision boxes (`boxes`, on the tangent plane).
    pub fn with_boxes(
        center: DVec3,
        footprints: Vec<Footprint>,
        boxes: Vec<PlacedBox>,
        blend: f64,
        radius: f64,
    ) -> Self {
        let center = center.normalize();
        let (east, south) = tangent_frame(center);
        let plane = DQuat::from_mat3(&DMat3::from_cols(east, center, south));
        let structures = boxes
            .iter()
            .map(|b| {
                let x = b.center.x;
                let z = b.center.z;
                // The vertical through the box's centre, up from the reference radius.
                let up = (center * radius + east * x + south * z).normalize();
                let turned = DQuat::from_rotation_arc(center, up) * plane;
                Structure {
                    center: up * (radius + b.center.y),
                    rotation: (turned * DQuat::from_rotation_y(b.yaw)).normalize(),
                    half: b.half,
                }
            })
            .collect();
        let mut lo = DVec2::splat(f64::MAX);
        let mut hi = DVec2::splat(f64::MIN);
        for f in &footprints {
            let r = f.reach(blend);
            lo = lo.min(DVec2::new(f.x - r, f.z - r));
            hi = hi.max(DVec2::new(f.x + r, f.z + r));
        }
        if footprints.is_empty() {
            (lo, hi) = (DVec2::ZERO, DVec2::ZERO);
        }
        let extent = lo.abs().max(hi.abs()).length();
        let size = [
            ((hi.x - lo.x) / CELL).ceil().max(1.0) as usize,
            ((hi.y - lo.y) / CELL).ceil().max(1.0) as usize,
        ];
        let mut cells = vec![Vec::new(); size[0] * size[1]];
        for (i, f) in footprints.iter().enumerate() {
            let r = f.reach(blend);
            let cell =
                |v: f64, o: f64, n: usize| (((v - o) / CELL).floor().max(0.0) as usize).min(n - 1);
            for cz in cell(f.z - r, lo.y, size[1])..=cell(f.z + r, lo.y, size[1]) {
                for cx in cell(f.x - r, lo.x, size[0])..=cell(f.x + r, lo.x, size[0]) {
                    cells[cz * size[0] + cx].push(i as u32);
                }
            }
        }
        Self {
            center,
            east,
            south,
            turns: footprints.iter().map(|f| f.yaw.sin_cos()).collect(),
            cos_reach: ((extent + CELL) / radius).min(std::f64::consts::PI).cos(),
            blend,
            footprints,
            structures,
            origin: lo,
            size,
            cells,
        }
    }

    pub fn center(&self) -> DVec3 {
        self.center
    }

    pub fn footprints(&self) -> &[Footprint] {
        &self.footprints
    }

    pub fn structures(&self) -> &[Structure] {
        &self.structures
    }

    /// Angular radius (radians) of the cap holding every footprint and its blend.
    pub fn reach(&self) -> f64 {
        self.cos_reach.clamp(-1.0, 1.0).acos()
    }

    /// `height` (the natural ground at unit direction `dir`) with this site's foundations.
    ///
    /// Where blends overlap, the foundations' heights are averaged weighted by nearness, so a
    /// building's own rectangle keeps its level (to millimetres) beside a neighbour on another
    /// level, and the ground stays continuous between them.
    fn apply(&self, dir: DVec3, radius: f64, height: f64) -> f64 {
        if dir.dot(self.center) < self.cos_reach {
            return height;
        }
        // Tangent-plane point (planar to well under a millimetre within a few km).
        let off = dir * radius - self.center * radius;
        let p = DVec2::new(off.dot(self.east), off.dot(self.south));
        let c = ((p - self.origin) / CELL).floor();
        if c.x < 0.0 || c.y < 0.0 || c.x >= self.size[0] as f64 || c.y >= self.size[1] as f64 {
            return height;
        }
        let (mut most, mut sum, mut weights) = (0.0f64, 0.0, 0.0);
        for &i in &self.cells[c.y as usize * self.size[0] + c.x as usize] {
            let i = i as usize;
            let f = &self.footprints[i];
            let d = f.distance(p, self.turns[i]);
            let w = 1.0 - smoothstep(0.0, self.blend, d);
            if w > 0.0 {
                let k = w / (d + 0.05).powi(2);
                most = most.max(w);
                sum += k * f.height;
                weights += k;
            }
        }
        if weights == 0.0 {
            return height;
        }
        height + (sum / weights - height) * most
    }
}

/// Every settlement's foundations on one body.
#[derive(Clone, Debug, Default)]
pub struct Foundations {
    sites: Vec<Site>,
}

impl Foundations {
    pub fn new(sites: Vec<Site>) -> Self {
        Self { sites }
    }

    pub fn sites(&self) -> &[Site] {
        &self.sites
    }

    /// `height` (natural ground at unit direction `dir`, body `radius`) with the foundations.
    pub fn apply(&self, dir: DVec3, radius: f64, height: f64) -> f64 {
        self.sites
            .iter()
            .fold(height, |h, s| s.apply(dir, radius, h))
    }

    /// Buildings within `distance` m (horizontally, by their bounding spheres) of unit
    /// direction `dir`, as (site, index in the site) with the box.
    pub fn structures_near(
        &self,
        dir: DVec3,
        distance: f64,
    ) -> impl Iterator<Item = ((u32, u32), &Structure)> {
        self.sites
            .iter()
            .enumerate()
            .filter(move |(_, s)| {
                let r = s.structures.first().map_or(1.0, |b| b.center.length());
                dir.angle_between(s.center) * r <= s.reach() * r + distance
            })
            .flat_map(move |(i, s)| {
                s.structures.iter().enumerate().filter_map(move |(k, b)| {
                    let r = b.center.length();
                    (dir.angle_between(b.center) * r <= b.half.length() + distance)
                        .then_some(((i as u32, k as u32), b))
                })
            })
    }

    /// Whether any foundation lies within `spread` radians of `center`.
    pub fn touches(&self, center: DVec3, spread: f64) -> bool {
        self.sites
            .iter()
            .any(|s| s.center.angle_between(center) <= spread + s.reach())
    }
}

fn smoothstep(e0: f64, e1: f64, x: f64) -> f64 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    const R: f64 = 600_000.0;

    /// The unit direction `x` m east and `z` m south of `center`.
    fn at(center: DVec3, x: f64, z: f64) -> DVec3 {
        let (e, s) = tangent_frame(center);
        (center * R + e * x + s * z).normalize()
    }

    #[test]
    fn tangent_frame_is_east_up_south() {
        let c = DVec3::X;
        let (e, s) = tangent_frame(c);
        // On the equator at +X the spin (about +Y) carries the ground toward -Z: east.
        assert!(e.abs_diff_eq(-DVec3::Z, 1e-12));
        assert!(s.abs_diff_eq(-DVec3::Y, 1e-12));
        // Right-handed: east × up = south.
        assert!(e.cross(c).abs_diff_eq(s, 1e-12));
    }

    #[test]
    fn a_footprint_levels_its_rectangle_and_blends_out() {
        let c = DVec3::new(1.0, 0.4, 0.2).normalize();
        let f = Footprint {
            x: 30.0,
            z: -10.0,
            yaw: 0.5,
            half: [4.0, 9.0],
            height: 120.0,
        };
        let site = Site::new(c, vec![f], 3.0, R);
        let foundations = Foundations::new(vec![site]);
        let natural = 100.0;
        let h = |x, z| foundations.apply(at(c, x, z), R, natural);
        // Centre and a corner just inside (along the turned axes).
        assert!((h(30.0, -10.0) - 120.0).abs() < 1e-6);
        let (s, co) = 0.5f64.sin_cos();
        // The footprint's local (3.9, 8.9) in the tangent plane: turned counter-clockwise.
        let (lx, lz) = (3.9, 8.9);
        let (x, z) = (30.0 + co * lx + s * lz, -10.0 - s * lx + co * lz);
        assert!((h(x, z) - 120.0).abs() < 0.01, "{}", h(x, z));
        // Past the blend: natural ground.
        assert_eq!(h(30.0, 10.0), natural);
        assert_eq!(h(-500.0, 0.0), natural);
        // Far away on the planet: natural, and the site doesn't touch there.
        assert_eq!(foundations.apply(-c, R, natural), natural);
        assert!(!foundations.touches(-c, 0.1));
        assert!(foundations.touches(c, 0.0));
    }
}
