//! The species generators (design/ground-cover.md §3): each builds one variant of a species as
//! voxels in its own frame (x, y across, z up, metres; the ground at z = 0), every cell holding a
//! colour *role* the species maps to a palette colour.
//!
//! Shapes follow the style scene's nature (tools/blocks/models.py: cushion-lobe crowns, tiered
//! conifers, rounded rocks) without its placement faults: every model's footing reaches below
//! z = 0 (so it never floats on uneven ground), nothing carries satellites at baked offsets, and
//! marks come from noise, not from regular patterns.

use std::collections::HashMap;

use glam::{DVec3, IVec3};

use crate::noise::{gradient, mix64, unit};

/// Colour roles of a model's cells (mapped to palette names per species).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Role {
    Wood = 1,
    Wood2,
    Leaf,
    LeafHi,
    LeafDark,
    LeafVar,
    Accent,
    Rock,
    RockHi,
    RockDark,
    Moss,
    Centre,
}

impl Role {
    pub const ALL: [Role; 12] = [
        Role::Wood,
        Role::Wood2,
        Role::Leaf,
        Role::LeafHi,
        Role::LeafDark,
        Role::LeafVar,
        Role::Accent,
        Role::Rock,
        Role::RockHi,
        Role::RockDark,
        Role::Moss,
        Role::Centre,
    ];

    pub fn from_index(i: u16) -> Role {
        Role::ALL[(i as usize).saturating_sub(1).min(11)]
    }

    /// The colour-role name in the data (`colours` of a species).
    pub fn key(self) -> &'static str {
        match self {
            Role::Wood => "wood",
            Role::Wood2 => "wood2",
            Role::Leaf => "leaf",
            Role::LeafHi => "leafhi",
            Role::LeafDark => "leafdark",
            Role::LeafVar => "leafvar",
            Role::Accent => "accent",
            Role::Rock => "rock",
            Role::RockHi => "rockhi",
            Role::RockDark => "rockdark",
            Role::Moss => "moss",
            Role::Centre => "centre",
        }
    }

    /// Living leaves (tinted by climate).
    pub fn leafy(self) -> bool {
        matches!(
            self,
            Role::Leaf | Role::LeafHi | Role::LeafDark | Role::LeafVar
        )
    }
}

/// A small seeded random source.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(mix64(seed ^ 0x6E7E_A7E5))
    }
    pub fn unit(&mut self) -> f64 {
        self.0 = mix64(self.0);
        unit(self.0)
    }
    pub fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.unit()
    }
    pub fn int(&mut self, lo: i32, hi: i32) -> i32 {
        lo + ((self.unit() * f64::from(hi - lo + 1)) as i32).min(hi - lo)
    }
    pub fn angle(&mut self) -> f64 {
        self.unit() * std::f64::consts::TAU
    }
}

/// Voxels of one size.
pub struct Vox {
    pub s: f64,
    pub cells: HashMap<IVec3, u16>,
    seed: u64,
}

impl Vox {
    pub fn new(s: f64, seed: u64) -> Self {
        Self {
            s,
            cells: HashMap::new(),
            seed,
        }
    }

    fn r(&self, lo: f64, hi: f64) -> std::ops::Range<i32> {
        (lo / self.s + 1e-6).floor() as i32..(hi / self.s - 1e-6).ceil() as i32
    }

    fn centre(&self, c: IVec3) -> DVec3 {
        (c.as_dvec3() + DVec3::splat(0.5)) * self.s
    }

    fn put(&mut self, c: IVec3, role: Role) {
        self.cells.insert(c, role as u16);
    }

    fn get(&self, c: IVec3) -> Option<Role> {
        self.cells.get(&c).map(|&v| Role::from_index(v))
    }

    /// Noise in about [-1, 1].
    fn noise(&self, p: DVec3, salt: u64) -> f64 {
        gradient(p, self.seed ^ salt)
    }

    pub fn fill(&mut self, lo: DVec3, hi: DVec3, role: Role) {
        for i in self.r(lo.x, hi.x) {
            for j in self.r(lo.y, hi.y) {
                for k in self.r(lo.z, hi.z) {
                    self.put(IVec3::new(i, j, k), role);
                }
            }
        }
    }

    /// A solid ellipsoid, its surface roughened by `wobble` (a share of the radius).
    pub fn ball(&mut self, c: DVec3, rad: DVec3, role: Role, wobble: f64) {
        let m = 1.0 + wobble;
        let salt = (c.x * 131.0 + c.y * 71.0 + c.z * 37.0) as u64;
        for i in self.r(c.x - rad.x * m, c.x + rad.x * m) {
            for j in self.r(c.y - rad.y * m, c.y + rad.y * m) {
                for k in self.r(c.z - rad.z * m, c.z + rad.z * m) {
                    let cell = IVec3::new(i, j, k);
                    let p = self.centre(cell);
                    let d = ((p - c) / rad).length();
                    let lim = 1.0 + wobble * self.noise(p * (1.6 / rad.max_element()), salt);
                    if d <= lim {
                        self.put(cell, role);
                    }
                }
            }
        }
    }

    /// A rod from `a` to `b`, its radius from `ra` to `rb`.
    pub fn rod(&mut self, a: DVec3, b: DVec3, ra: f64, rb: f64, role: Role) {
        let ab = b - a;
        let l2 = ab.length_squared().max(1e-12);
        let rm = ra.max(rb);
        let lo = a.min(b) - DVec3::splat(rm);
        let hi = a.max(b) + DVec3::splat(rm);
        for i in self.r(lo.x, hi.x) {
            for j in self.r(lo.y, hi.y) {
                for k in self.r(lo.z, hi.z) {
                    let cell = IVec3::new(i, j, k);
                    let p = self.centre(cell);
                    let t = ((p - a).dot(ab) / l2).clamp(0.0, 1.0);
                    let r = ra + (rb - ra) * t;
                    // At least 0.75 of a cell, so thin diagonal rods stay joined (no floating bits).
                    if (p - (a + ab * t)).length() <= r.max(self.s * 0.75) {
                        self.put(cell, role);
                    }
                }
            }
        }
    }

    /// A polyline of rods.
    pub fn path(&mut self, pts: &[DVec3], ra: f64, rb: f64, role: Role) {
        let n = pts.len().saturating_sub(1).max(1) as f64;
        for (k, w) in pts.windows(2).enumerate() {
            let t0 = k as f64 / n;
            let t1 = (k + 1) as f64 / n;
            self.rod(w[0], w[1], ra + (rb - ra) * t0, ra + (rb - ra) * t1, role);
        }
    }

    /// A crown lobe (the style scene's cushion): stacked rounded squares from `c.z` up, flat on
    /// top and underneath, narrowing toward both.
    pub fn lobe(&mut self, c: DVec3, r: f64, h: f64, role: Role) {
        let n = ((h / self.s).round() as i32).max(2);
        let k0 = (c.z / self.s).floor() as i32;
        for k in 0..n {
            let t = (f64::from(k) + 0.5) / f64::from(n);
            let rr = r * (1.0 - 0.55 * (2.0 * (t - 0.45).abs() - 0.3).max(0.0).powf(1.5));
            for i in self.r(c.x - rr, c.x + rr) {
                for j in self.r(c.y - rr, c.y + rr) {
                    let px = (f64::from(i) + 0.5) * self.s - c.x;
                    let py = (f64::from(j) + 0.5) * self.s - c.y;
                    if px.abs().max(py.abs()) <= rr && px.abs() + py.abs() <= rr * 1.45 {
                        self.put(IVec3::new(i, j, k0 + k), role);
                    }
                }
            }
        }
    }

    /// Recolours cells of `from` by light: open above `top`, open below `under`, the rest `body`,
    /// a noise share `k` of it `var`.
    pub fn shade(&mut self, from: &[Role], top: Role, body: Role, under: Role, var: Role, k: f64) {
        let keys: Vec<IVec3> = self
            .cells
            .iter()
            .filter(|(_, v)| from.contains(&Role::from_index(**v)))
            .map(|(c, _)| *c)
            .collect();
        let mut out = Vec::with_capacity(keys.len());
        for c in keys {
            let above = self.cells.contains_key(&(c + IVec3::Z));
            let below = self.cells.contains_key(&(c - IVec3::Z));
            let role = if !above {
                // Lighter and darker patches on the lit tops (blobs, not stripes).
                if self.noise(c.as_dvec3() * 0.18, 77) > 1.0 - 2.0 * k {
                    var
                } else {
                    top
                }
            } else if !below {
                under
            } else {
                body
            };
            out.push((c, role));
        }
        for (c, r) in out {
            self.put(c, r);
        }
    }

    /// The highest cell's top (m).
    pub fn top(&self) -> f64 {
        self.cells.keys().map(|c| c.z + 1).max().unwrap_or(0) as f64 * self.s
    }
}

/// The parameters one variant is built from: height and width (m), its seed.
pub struct Params {
    pub height: f64,
    pub width: f64,
    pub has_accent: bool,
}

/// Builds a variant with generator `kind`; returns the voxels and, for some generators, an
/// accent colour name chosen for this variant (flowers, mushrooms, berries).
pub fn build(kind: &str, s: f64, p: &Params, seed: u64) -> (Vox, Option<&'static str>) {
    let mut v = Vox::new(s, seed);
    let mut rng = Rng::new(seed);
    let mut accent = None;
    let (h, w) = (p.height, p.width);
    match kind {
        "broadleaf" | "broadleaf_tall" | "broadleaf_wide" => {
            broadleaf(&mut v, &mut rng, kind, h, w)
        }
        "birch" => birch(&mut v, &mut rng, h, w),
        "willow" => willow(&mut v, &mut rng, h, w),
        "giant" => giant(&mut v, &mut rng, h, w),
        "acacia" => acacia(&mut v, &mut rng, h, w),
        "baobab" => baobab(&mut v, &mut rng, h, w),
        "conifer" => conifer(&mut v, &mut rng, h, w),
        "pine" => pine(&mut v, &mut rng, h, w),
        "krummholz" => krummholz(&mut v, &mut rng, h, w),
        "palm" => palm(&mut v, &mut rng, h, w),
        "dead" => dead(&mut v, &mut rng, h, w),
        "log" => log(&mut v, &mut rng, h, w),
        "stump" => stump(&mut v, &mut rng, h, w),
        "driftwood" => driftwood(&mut v, &mut rng, h, w),
        "shrub" => {
            shrub(&mut v, &mut rng, h, w);
            if p.has_accent {
                berries(&mut v, &mut rng);
            }
        }
        "fern" => fern(&mut v, &mut rng, h, w),
        "dwarf" => dwarf(&mut v, &mut rng, h, w),
        "bamboo" => bamboo(&mut v, &mut rng, h, w),
        "reeds" => reeds(&mut v, &mut rng, h, w),
        "cactus" => cactus(&mut v, &mut rng, h, w),
        "padcactus" => padcactus(&mut v, &mut rng, h, w),
        "tuft" => tuft(&mut v, &mut rng, h, w),
        "flowers" => {
            const HEADS: [&str; 7] = ["pink", "yellow", "white", "red", "purple", "blue", "orange"];
            accent = Some(HEADS[rng.int(0, 6) as usize]);
            flowers(&mut v, &mut rng, h, w);
        }
        "mushrooms" => {
            const CAPS: [&str; 3] = ["red", "brown", "orange"];
            accent = Some(CAPS[rng.int(0, 2) as usize]);
            mushrooms(&mut v, &mut rng, h, w);
        }
        "pebbles" => pebbles(&mut v, &mut rng, h, w),
        "rock" | "ice" => rock(&mut v, &mut rng, h, w, kind == "ice"),
        "outcrop" => outcrop(&mut v, &mut rng, h, w),
        "mound" => mound(&mut v, &mut rng, h, w),
        "crust" => crust(&mut v, &mut rng, h, w),
        _ => v.ball(
            DVec3::new(0.0, 0.0, h * 0.5),
            DVec3::new(w, w, h) * 0.5,
            Role::Rock,
            0.1,
        ),
    }
    (v, accent)
}

fn dv(x: f64, y: f64, z: f64) -> DVec3 {
    DVec3::new(x, y, z)
}

/// A point at angle `a` and distance `r` round the vertical at height `z`.
fn polar(a: f64, r: f64, z: f64) -> DVec3 {
    dv(a.cos() * r, a.sin() * r, z)
}

/// A trunk from below the ground to `top`, leaning by `lean`, with marks from noise.
fn trunk(v: &mut Vox, top: DVec3, r0: f64, r1: f64, marks: Role) {
    let base = dv(0.0, 0.0, -0.75);
    v.rod(base, top, r0, r1, Role::Wood);
    let keys: Vec<IVec3> = v
        .cells
        .iter()
        .filter(|(_, r)| Role::from_index(**r) == Role::Wood)
        .map(|(c, _)| *c)
        .collect();
    for c in keys {
        if v.noise(c.as_dvec3() * 0.6, 11) > 0.45 {
            v.put(c, marks);
        }
    }
}

fn leaves(v: &mut Vox, k: f64) {
    v.shade(
        &[Role::Leaf],
        Role::LeafHi,
        Role::Leaf,
        Role::LeafDark,
        Role::LeafVar,
        k,
    );
}

fn broadleaf(v: &mut Vox, rng: &mut Rng, kind: &str, h: f64, w: f64) {
    let (trunk_share, crown_flat) = match kind {
        "broadleaf_tall" => (0.5, 0.9),
        "broadleaf_wide" => (0.45, 0.55),
        _ => (0.4, 0.75),
    };
    let th = h * trunk_share;
    let lean = polar(rng.angle(), rng.range(0.0, 0.08) * h, 0.0);
    let top = dv(0.0, 0.0, th) + lean;
    let r = (0.045 * h).clamp(0.25, 0.6);
    trunk(v, top, r * 1.25, r * 0.85, Role::Wood2);
    // Root flare.
    for k in 0..4 {
        let a = rng.angle() + f64::from(k) * 1.5;
        v.rod(
            dv(0.0, 0.0, 0.3),
            polar(a, r * 2.6, -0.2),
            r * 0.6,
            r * 0.35,
            Role::Wood,
        );
    }
    if kind == "broadleaf_wide" {
        // Buttresses.
        for k in 0..3 {
            let a = rng.angle() + f64::from(k) * 2.1;
            v.rod(
                dv(0.0, 0.0, 1.2),
                polar(a, r * 3.5, -0.3),
                r * 0.5,
                r * 0.3,
                Role::Wood,
            );
        }
    }
    // Limbs.
    let crown_h = h - th;
    let limbs = rng.int(2, 4);
    let mut ends = Vec::new();
    for _ in 0..limbs {
        let a = rng.angle();
        let from = dv(0.0, 0.0, th * rng.range(0.7, 0.95)) + lean * 0.8;
        let to = top + polar(a, w * rng.range(0.18, 0.3), crown_h * rng.range(0.2, 0.4));
        v.rod(from, to, r * 0.55, r * 0.35, Role::Wood);
        ends.push(to);
    }
    // Crown: a main lobe on top of the trunk and lobes at the limbs' ends.
    let base_z = th + crown_h * 0.1;
    let main_h = crown_h * crown_flat * rng.range(0.55, 0.7);
    v.lobe(
        top.with_z(base_z),
        w * rng.range(0.32, 0.4),
        main_h,
        Role::Leaf,
    );
    for e in &ends {
        v.lobe(
            *e - dv(0.0, 0.0, crown_h * 0.15),
            w * rng.range(0.18, 0.28),
            crown_h * crown_flat * rng.range(0.35, 0.5),
            Role::Leaf,
        );
    }
    // A cap lobe, off centre.
    v.lobe(
        top.with_z(base_z + main_h * 0.75) + polar(rng.angle(), w * 0.08, 0.0),
        w * rng.range(0.17, 0.25),
        crown_h * crown_flat * rng.range(0.3, 0.4),
        Role::Leaf,
    );
    leaves(v, 0.3);
}

fn birch(v: &mut Vox, rng: &mut Rng, h: f64, w: f64) {
    let lean = polar(rng.angle(), rng.range(0.0, 0.1) * h, 0.0);
    let top = dv(0.0, 0.0, h * 0.85) + lean;
    trunk(v, top, v.s * 0.5, v.s * 0.5, Role::Wood2);
    for _ in 0..rng.int(1, 3) {
        let z = h * rng.range(0.45, 0.65);
        let from = dv(0.0, 0.0, z) + lean * (z / h);
        v.rod(
            from,
            from + polar(rng.angle(), w * 0.3, h * 0.15),
            0.05,
            0.05,
            Role::Wood,
        );
    }
    let lobes = rng.int(3, 5);
    for k in 0..lobes {
        let t = f64::from(k) / f64::from(lobes);
        let z = h * (0.4 + 0.45 * t);
        let c = lean * (z / h) + polar(rng.angle(), w * 0.08, z);
        v.lobe(
            c,
            w * (0.5 - 0.22 * t) * rng.range(0.85, 1.1),
            h * 0.15,
            Role::Leaf,
        );
    }
    leaves(v, 0.15);
}

fn willow(v: &mut Vox, rng: &mut Rng, h: f64, w: f64) {
    let th = h * 0.4;
    let top = dv(0.0, 0.0, th) + polar(rng.angle(), rng.range(0.0, 0.3), 0.0);
    trunk(v, top, 0.35, 0.28, Role::Wood2);
    for _ in 0..3 {
        v.rod(
            top,
            top + polar(rng.angle(), w * 0.25, h * 0.2),
            0.15,
            0.1,
            Role::Wood,
        );
    }
    let cz = th + (h - th) * 0.15;
    v.lobe(top.with_z(cz), w * 0.45, (h - th) * 0.7, Role::Leaf);
    v.lobe(
        top.with_z(cz + (h - th) * 0.45),
        w * 0.28,
        (h - th) * 0.4,
        Role::Leaf,
    );
    // Hanging curtains from the crown's lower rim.
    let rim: Vec<IVec3> = v
        .cells
        .keys()
        .filter(|c| {
            let p = (c.as_dvec3() + DVec3::splat(0.5)) * v.s;
            !v.cells.contains_key(&(**c - IVec3::Z))
                && v.get(**c) == Some(Role::Leaf)
                && (p.truncate() - top.truncate()).length() > w * 0.25
        })
        .copied()
        .collect();
    let mut rim = rim;
    rim.sort_unstable_by_key(|c| (c.z, c.y, c.x));
    for c in rim {
        if rng.unit() < 0.55 {
            let n = (rng.range(0.8, 2.6) / v.s) as i32;
            for k in 1..=n {
                v.put(c - IVec3::Z * k, Role::LeafVar);
            }
        }
    }
    leaves(v, 0.2);
}

fn giant(v: &mut Vox, rng: &mut Rng, h: f64, w: f64) {
    let th = h * 0.72;
    let top = dv(0.0, 0.0, th) + polar(rng.angle(), rng.range(0.0, 0.5), 0.0);
    trunk(v, top, 1.0, 0.7, Role::Wood2);
    for k in 0..5 {
        let a = f64::from(k) * 1.25 + rng.range(0.0, 0.5);
        v.rod(dv(0.0, 0.0, 3.0), polar(a, 3.2, -0.5), 0.5, 0.3, Role::Wood);
    }
    for _ in 0..4 {
        let a = rng.angle();
        v.rod(
            top,
            top + polar(a, w * 0.35, h * 0.12),
            0.45,
            0.3,
            Role::Wood,
        );
        v.lobe(
            top + polar(a, w * 0.33, h * 0.08),
            w * 0.2,
            h * 0.1,
            Role::Leaf,
        );
    }
    v.lobe(top.with_z(th + h * 0.08), w * 0.32, h * 0.16, Role::Leaf);
    leaves(v, 0.25);
}

fn acacia(v: &mut Vox, rng: &mut Rng, h: f64, w: f64) {
    let fork = dv(0.0, 0.0, h * 0.35) + polar(rng.angle(), rng.range(0.2, 0.6), 0.0);
    trunk(v, fork, 0.25, 0.2, Role::Wood2);
    let crown_z = h * 0.78;
    for _ in 0..rng.int(2, 4) {
        let a = rng.angle();
        let end = polar(a, w * rng.range(0.15, 0.32), crown_z);
        v.rod(fork, end, 0.18, 0.1, Role::Wood);
        v.lobe(
            end,
            w * rng.range(0.2, 0.3),
            h * rng.range(0.1, 0.14),
            Role::Leaf,
        );
    }
    v.lobe(
        fork.with_z(crown_z + h * 0.05),
        w * 0.35,
        h * 0.12,
        Role::Leaf,
    );
    leaves(v, 0.25);
}

fn baobab(v: &mut Vox, rng: &mut Rng, h: f64, w: f64) {
    let r = w * 0.32;
    let th = h * 0.6;
    for k in 0..5 {
        let t = f64::from(k) / 4.0;
        // A bottle: widest a third of the way up, narrowing to the top.
        let bulge = 1.0 - 1.6 * (t - 0.3).powi(2);
        let rr = r * bulge.max(0.45) * rng.range(0.95, 1.05);
        v.ball(
            dv(0.0, 0.0, -0.4 + t * th),
            dv(rr, rr, th * 0.22),
            Role::Wood,
            0.05,
        );
    }
    let top = dv(0.0, 0.0, th);
    for _ in 0..rng.int(4, 6) {
        let end = top
            + polar(
                rng.angle(),
                w * rng.range(0.3, 0.45),
                h * rng.range(0.1, 0.25),
            );
        v.rod(top, end, 0.3, 0.15, Role::Wood2);
        v.lobe(end, w * 0.1, h * 0.06, Role::Leaf);
    }
    leaves(v, 0.2);
}

fn conifer(v: &mut Vox, rng: &mut Rng, h: f64, w: f64) {
    trunk(v, dv(0.0, 0.0, h * 0.9), 0.25, 0.15, Role::Wood2);
    let tiers = ((h / 1.6).round() as i32).clamp(4, 10);
    let r0 = w * 0.5;
    let step = (h * 0.82) / f64::from(tiers);
    for t in 0..tiers {
        let ft = f64::from(t) / f64::from(tiers);
        let z0 = h * 0.12 + f64::from(t) * step;
        // Each tier at least as tall as the step to the next, so tiers touch.
        let rr0 = (r0 * (1.0 - 0.8 * ft) * rng.range(0.85, 1.1)).max(step + v.s);
        let mut m = 0;
        loop {
            let rr = rr0 - f64::from(m) * v.s;
            if rr < v.s * 0.8 || m > 12 {
                break;
            }
            let k = ((z0 / v.s).floor() as i32) + m;
            for i in v.r(-rr, rr) {
                for j in v.r(-rr, rr) {
                    let px = ((f64::from(i) + 0.5) * v.s).abs();
                    let py = ((f64::from(j) + 0.5) * v.s).abs();
                    if px.max(py) <= rr && px + py <= rr * 1.35 {
                        v.put(IVec3::new(i, j, k), Role::Leaf);
                    }
                }
            }
            m += 1;
        }
    }
    // The leader: one more course on the top tier.
    let top = v.top();
    v.fill(dv(-v.s, -v.s, top), dv(v.s, v.s, top + v.s), Role::LeafHi);
    v.shade(
        &[Role::Leaf],
        Role::LeafHi,
        Role::Leaf,
        Role::LeafDark,
        Role::LeafDark,
        0.2,
    );
}

fn pine(v: &mut Vox, rng: &mut Rng, h: f64, w: f64) {
    let lean = polar(rng.angle(), rng.range(0.0, 0.05) * h, 0.0);
    trunk(v, dv(0.0, 0.0, h * 0.85) + lean, 0.22, 0.15, Role::Wood2);
    for _ in 0..rng.int(3, 5) {
        let z = h * rng.range(0.55, 0.9);
        let c = lean * (z / h) + polar(rng.angle(), w * rng.range(0.05, 0.2), z);
        v.lobe(c, w * rng.range(0.22, 0.32), h * 0.09, Role::Leaf);
    }
    leaves(v, 0.2);
}

fn krummholz(v: &mut Vox, rng: &mut Rng, h: f64, w: f64) {
    let wind = rng.angle();
    let top = polar(wind, w * 0.3, h * 0.6);
    trunk(v, top, 0.15, 0.1, Role::Wood2);
    for k in 0..rng.int(3, 5) {
        let t = f64::from(k) / 4.0;
        let c = polar(wind, w * (0.1 + 0.3 * t), h * (0.15 + 0.6 * t))
            + polar(rng.angle(), w * 0.1, 0.0);
        v.lobe(c, w * (0.35 - 0.12 * t), h * 0.3, Role::Leaf);
    }
    leaves(v, 0.25);
}

fn palm(v: &mut Vox, rng: &mut Rng, h: f64, w: f64) {
    let bend = polar(rng.angle(), rng.range(0.6, 1.8), 0.0);
    let pts: Vec<DVec3> = (0..=8)
        .map(|k| {
            let t = f64::from(k) / 8.0;
            dv(0.0, 0.0, -0.6 + (h + 0.6) * t) + bend * (t * t)
        })
        .collect();
    v.path(&pts, 0.25, 0.18, Role::Wood);
    // Rings from noise along the height.
    let keys: Vec<IVec3> = v.cells.keys().copied().collect();
    for c in keys {
        if (f64::from(c.z) * v.s * 1.6).fract() < 0.18 {
            v.put(c, Role::Wood2);
        }
    }
    let crown = *pts.last().unwrap_or(&dv(0.0, 0.0, h));
    let fronds = rng.int(5, 8);
    for k in 0..fronds {
        let a = f64::from(k) / f64::from(fronds) * std::f64::consts::TAU + rng.range(-0.3, 0.3);
        let reach = w * rng.range(0.4, 0.55);
        let pts = [
            crown,
            crown + polar(a, reach * 0.4, 0.6),
            crown + polar(a, reach * 0.75, 0.2),
            crown + polar(a, reach, -1.0),
        ];
        v.path(&pts, 0.3, 0.18, Role::Leaf);
    }
    leaves(v, 0.2);
}

fn dead(v: &mut Vox, rng: &mut Rng, h: f64, w: f64) {
    let lean = polar(rng.angle(), rng.range(0.0, 0.12) * h, 0.0);
    let top = dv(0.0, 0.0, h) + lean;
    trunk(v, top, 0.3, 0.1, Role::Wood2);
    for _ in 0..rng.int(2, 4) {
        let z = h * rng.range(0.4, 0.8);
        let from = lean * (z / h) + dv(0.0, 0.0, z);
        let to = from
            + polar(
                rng.angle(),
                w * rng.range(0.2, 0.45),
                h * rng.range(0.05, 0.25),
            );
        v.rod(from, to, 0.12, 0.06, Role::Wood);
    }
}

fn log(v: &mut Vox, rng: &mut Rng, h: f64, w: f64) {
    let r = h * 0.5;
    let half = w * 0.5;
    let sag = rng.range(-0.15, 0.15);
    v.path(
        &[
            dv(-half, 0.0, r * 0.8),
            dv(0.0, sag, r * 0.85),
            dv(half, 0.0, r * 0.75),
        ],
        r,
        r * 0.85,
        Role::Wood,
    );
    for _ in 0..rng.int(1, 3) {
        let x = rng.range(-half * 0.7, half * 0.7);
        let side = if rng.unit() < 0.5 { -1.0 } else { 1.0 };
        v.rod(
            dv(x, 0.0, r),
            dv(x + 0.2, side * r * 1.8, r * 1.6),
            0.08,
            0.05,
            Role::Wood,
        );
    }
    let keys: Vec<IVec3> = v.cells.keys().copied().collect();
    for c in keys {
        let p = (c.as_dvec3() + DVec3::splat(0.5)) * v.s;
        if p.x.abs() > half - v.s {
            v.put(c, Role::Wood2);
        } else if !v.cells.contains_key(&(c + IVec3::Z)) && v.noise(p * 1.3, 5) > 0.0 {
            v.put(c, Role::Leaf);
        }
    }
}

fn stump(v: &mut Vox, rng: &mut Rng, h: f64, w: f64) {
    let r = w * 0.3;
    v.rod(dv(0.0, 0.0, -0.5), dv(0.0, 0.0, h), r, r * 0.9, Role::Wood);
    for k in 0..rng.int(3, 5) {
        let a = rng.angle() + f64::from(k);
        v.rod(
            dv(0.0, 0.0, h * 0.3),
            polar(a, w * 0.55, -0.2),
            r * 0.4,
            r * 0.2,
            Role::Wood,
        );
    }
    let keys: Vec<IVec3> = v.cells.keys().copied().collect();
    for c in keys {
        if !v.cells.contains_key(&(c + IVec3::Z)) && (c.z as f64 + 1.0) * v.s > h - v.s {
            v.put(c, Role::Wood2);
        }
    }
}

fn driftwood(v: &mut Vox, rng: &mut Rng, h: f64, w: f64) {
    let half = w * 0.5;
    let r = h * 0.3;
    v.path(
        &[
            dv(-half, 0.0, r),
            dv(-half * 0.3, rng.range(-0.3, 0.3), r * 1.1),
            dv(half * 0.4, rng.range(-0.4, 0.4), r),
            dv(half, rng.range(-0.3, 0.3), r * 0.9),
        ],
        r,
        r * 0.6,
        Role::Wood,
    );
    let x = rng.range(-half * 0.5, half * 0.5);
    v.rod(
        dv(x, 0.0, r),
        dv(x + 0.3, rng.range(0.3, 0.7), r * 1.5),
        r * 0.5,
        r * 0.3,
        Role::Wood2,
    );
}

fn shrub(v: &mut Vox, rng: &mut Rng, h: f64, w: f64) {
    let balls = rng.int(2, 3);
    for k in 0..balls {
        let c = if k == 0 {
            dv(0.0, 0.0, h * 0.4)
        } else {
            polar(rng.angle(), w * 0.22, h * rng.range(0.4, 0.6))
        };
        let rad = dv(w * 0.32, w * 0.28, h * 0.45) * rng.range(0.75, 1.0);
        v.ball(c, rad, Role::Leaf, 0.2);
    }
    // Stems below the leaves reach into the ground.
    v.rod(
        dv(0.0, 0.0, -0.3),
        dv(0.0, 0.0, h * 0.3),
        v.s,
        v.s * 0.5,
        Role::Wood,
    );
    leaves(v, 0.25);
}

fn berries(v: &mut Vox, rng: &mut Rng) {
    let tops: Vec<IVec3> = v
        .cells
        .iter()
        .filter(|(_, r)| Role::from_index(**r) == Role::LeafHi)
        .map(|(c, _)| *c)
        .collect();
    let mut tops = tops;
    tops.sort_unstable_by_key(|c| (c.z, c.y, c.x));
    for c in tops {
        if rng.unit() < 0.12 {
            v.put(c, Role::Accent);
        }
    }
}

fn fern(v: &mut Vox, rng: &mut Rng, h: f64, w: f64) {
    let fronds = rng.int(6, 10);
    for k in 0..fronds {
        let a = f64::from(k) / f64::from(fronds) * std::f64::consts::TAU + rng.range(-0.25, 0.25);
        let reach = w * rng.range(0.38, 0.5);
        let rise = h * rng.range(0.8, 1.0);
        let pts = [
            dv(0.0, 0.0, -0.05),
            polar(a, reach * 0.35, rise * 0.8),
            polar(a, reach * 0.7, rise),
            polar(a, reach, rise * 0.55),
        ];
        v.path(&pts, v.s * 0.9, v.s * 0.5, Role::Leaf);
    }
    leaves(v, 0.3);
}

fn dwarf(v: &mut Vox, rng: &mut Rng, h: f64, w: f64) {
    for _ in 0..rng.int(2, 4) {
        let c = polar(rng.angle(), w * 0.2, h * 0.3);
        v.ball(c, dv(w * 0.3, w * 0.25, h * 0.6), Role::Leaf, 0.3);
    }
    v.fill(
        dv(-w * 0.2, -w * 0.2, -0.2),
        dv(w * 0.2, w * 0.2, 0.0),
        Role::Wood,
    );
    leaves(v, 0.45);
}

fn bamboo(v: &mut Vox, rng: &mut Rng, h: f64, w: f64) {
    let canes = rng.int(6, 12);
    for _ in 0..canes {
        let base = polar(rng.angle(), w * 0.35 * rng.unit().sqrt(), -0.4);
        let tip =
            base.with_z(h * rng.range(0.7, 1.0)) + polar(rng.angle(), rng.range(0.1, 0.5), 0.0);
        v.rod(base, tip, v.s * 0.5, v.s * 0.5, Role::Wood);
        v.lobe(tip - dv(0.0, 0.0, 0.6), 0.35, 0.6, Role::Leaf);
    }
    let keys: Vec<IVec3> = v
        .cells
        .iter()
        .filter(|(_, r)| Role::from_index(**r) == Role::Wood)
        .map(|(c, _)| *c)
        .collect();
    for c in keys {
        if (f64::from(c.z) * v.s / 0.5).fract() < 0.2 {
            v.put(c, Role::Wood2);
        }
    }
    leaves(v, 0.25);
}

fn reeds(v: &mut Vox, rng: &mut Rng, h: f64, w: f64) {
    for _ in 0..rng.int(14, 28) {
        let base = polar(rng.angle(), w * 0.45 * rng.unit().sqrt(), -0.2);
        let tip =
            base.with_z(h * rng.range(0.6, 1.0)) + polar(rng.angle(), rng.range(0.0, 0.15), 0.0);
        v.rod(base, tip, v.s * 0.5, v.s * 0.5, Role::Leaf);
        if rng.unit() < 0.4 {
            v.rod(
                tip - dv(0.0, 0.0, 0.18),
                tip,
                v.s * 0.9,
                v.s * 0.9,
                Role::Wood,
            );
        }
    }
    leaves(v, 0.3);
}

fn cactus(v: &mut Vox, rng: &mut Rng, h: f64, w: f64) {
    let r = (w * 0.14).max(v.s);
    v.rod(dv(0.0, 0.0, -0.5), dv(0.0, 0.0, h), r, r * 0.85, Role::Leaf);
    for _ in 0..rng.int(0, 4) {
        let a = rng.angle();
        let z = h * rng.range(0.3, 0.6);
        let out = polar(a, w * rng.range(0.3, 0.45), z);
        v.rod(dv(0.0, 0.0, z), out, r * 0.75, r * 0.7, Role::Leaf);
        v.rod(
            out,
            out + dv(0.0, 0.0, h * rng.range(0.2, 0.35)),
            r * 0.7,
            r * 0.6,
            Role::Leaf,
        );
    }
    v.shade(
        &[Role::Leaf],
        Role::LeafHi,
        Role::Leaf,
        Role::LeafDark,
        Role::LeafVar,
        0.2,
    );
    if rng.unit() < 0.5 {
        let tops: Vec<IVec3> = v
            .cells
            .iter()
            .filter(|(_, r)| Role::from_index(**r) == Role::LeafHi)
            .map(|(c, _)| *c)
            .collect();
        let mut tops = tops;
        tops.sort_unstable_by_key(|c| (c.z, c.y, c.x));
        for c in tops {
            if rng.unit() < 0.3 {
                v.put(c + IVec3::Z, Role::Accent);
            }
        }
    }
}

fn padcactus(v: &mut Vox, rng: &mut Rng, h: f64, w: f64) {
    let mut at = dv(0.0, 0.0, h * 0.2);
    for k in 0..rng.int(3, 6) {
        let a = rng.angle();
        let pad = dv(w * 0.22, 0.07, h * 0.25);
        let c = at
            + polar(
                a,
                if k == 0 { 0.0 } else { w * 0.12 },
                if k == 0 { 0.0 } else { h * 0.12 },
            );
        // A flat pad turned by `a` (approximated by axis choice).
        let rad = if a.cos().abs() > 0.7 {
            dv(pad.y, pad.x, pad.z)
        } else {
            pad
        };
        v.ball(c, rad, Role::Leaf, 0.05);
        if rng.unit() < 0.5 {
            v.put(
                ((c + dv(0.0, 0.0, pad.z)) / v.s).floor().as_ivec3(),
                Role::Accent,
            );
        }
        at = c;
    }
    v.rod(
        dv(0.0, 0.0, -0.3),
        dv(0.0, 0.0, h * 0.2),
        v.s,
        v.s,
        Role::Leaf,
    );
    v.shade(
        &[Role::Leaf],
        Role::LeafHi,
        Role::Leaf,
        Role::LeafDark,
        Role::LeafVar,
        0.2,
    );
}

fn tuft(v: &mut Vox, rng: &mut Rng, h: f64, w: f64) {
    for _ in 0..rng.int(5, 9) {
        let a = rng.angle();
        let base = polar(a, w * 0.3 * rng.unit(), -0.05);
        let tip = base + polar(a, w * rng.range(0.1, 0.35), h * rng.range(0.55, 1.0) + 0.05);
        v.rod(base, tip, v.s * 0.5, v.s * 0.5, Role::Leaf);
    }
    v.shade(
        &[Role::Leaf],
        Role::LeafHi,
        Role::Leaf,
        Role::Leaf,
        Role::LeafVar,
        0.3,
    );
}

fn flowers(v: &mut Vox, rng: &mut Rng, h: f64, w: f64) {
    for _ in 0..rng.int(3, 6) {
        let base = polar(rng.angle(), w * 0.4 * rng.unit(), -0.05);
        let top = base.with_z(h * rng.range(0.6, 1.0));
        v.rod(base, top, v.s * 0.5, v.s * 0.5, Role::Leaf);
        v.ball(top, dv(v.s * 1.6, v.s * 1.6, v.s * 0.8), Role::Accent, 0.0);
        v.put((top / v.s).floor().as_ivec3() + IVec3::Z, Role::Centre);
    }
}

fn mushrooms(v: &mut Vox, rng: &mut Rng, h: f64, w: f64) {
    for _ in 0..rng.int(2, 5) {
        let base = polar(rng.angle(), w * 0.4 * rng.unit(), -0.05);
        let stem = h * rng.range(0.5, 0.9);
        v.rod(base, base.with_z(stem), v.s * 0.7, v.s * 0.6, Role::Wood);
        v.ball(
            base.with_z(stem),
            dv(h * 0.45, h * 0.45, h * 0.22),
            Role::Accent,
            0.1,
        );
    }
    let keys: Vec<IVec3> = v
        .cells
        .iter()
        .filter(|(_, r)| Role::from_index(**r) == Role::Accent)
        .map(|(c, _)| *c)
        .collect();
    for c in keys {
        if !v.cells.contains_key(&(c + IVec3::Z)) && v.noise(c.as_dvec3() * 0.9, 3) > 0.5 {
            v.put(c, Role::LeafHi);
        }
    }
}

fn pebbles(v: &mut Vox, rng: &mut Rng, h: f64, w: f64) {
    for _ in 0..rng.int(3, 7) {
        let c = polar(rng.angle(), w * 0.45 * rng.unit(), 0.0);
        let r = h * rng.range(0.5, 1.0);
        v.ball(c, dv(r * 1.3, r, r), Role::Rock, 0.15);
    }
    v.shade(
        &[Role::Rock],
        Role::RockHi,
        Role::Rock,
        Role::RockDark,
        Role::Rock,
        0.0,
    );
}

fn rock(v: &mut Vox, rng: &mut Rng, h: f64, w: f64, ice: bool) {
    let c = dv(0.0, 0.0, h * 0.35);
    let rad = dv(w * 0.5, w * rng.range(0.35, 0.5), h * 0.65);
    v.ball(c, rad, Role::Rock, if ice { 0.25 } else { 0.14 });
    if rng.unit() < 0.4 {
        // A second lump.
        let c2 = c + polar(rng.angle(), w * 0.3, -h * 0.1);
        v.ball(c2, rad * 0.6, Role::Rock, 0.14);
    }
    v.shade(
        &[Role::Rock],
        Role::RockHi,
        Role::Rock,
        Role::RockDark,
        Role::Rock,
        0.0,
    );
    if !ice {
        // Moss on top in patches (shown as moss only in wet places; dry places tint it rock).
        let tops: Vec<IVec3> = v
            .cells
            .iter()
            .filter(|(_, r)| Role::from_index(**r) == Role::RockHi)
            .map(|(c, _)| *c)
            .collect();
        for c in tops {
            if v.noise(c.as_dvec3() * 0.5, 21) > 0.1 {
                v.put(c, Role::Moss);
            }
        }
    }
}

fn outcrop(v: &mut Vox, rng: &mut Rng, h: f64, w: f64) {
    // A stack of weathered slabs: flattened, rough ellipsoids, each smaller and shifted.
    let slabs = rng.int(3, 5);
    let mut z = -0.5;
    let mut half = dv(w * 0.5, w * rng.range(0.32, 0.45), 0.0);
    let mut off = DVec3::ZERO;
    for _ in 0..slabs {
        let th = h / f64::from(slabs) * rng.range(1.0, 1.3);
        v.ball(
            off.with_z(z + th * 0.45),
            dv(half.x, half.y, th * 0.6),
            Role::Rock,
            0.18,
        );
        z += th * 0.85;
        half = dv(
            half.x * rng.range(0.65, 0.85),
            half.y * rng.range(0.65, 0.9),
            0.0,
        );
        off += polar(rng.angle(), w * 0.1, 0.0);
    }
    v.shade(
        &[Role::Rock],
        Role::RockHi,
        Role::Rock,
        Role::RockDark,
        Role::RockDark,
        0.3,
    );
}

fn mound(v: &mut Vox, rng: &mut Rng, h: f64, w: f64) {
    for k in 0..rng.int(1, 3) {
        let c = if k == 0 {
            DVec3::ZERO
        } else {
            polar(rng.angle(), w * 0.25, 0.0)
        };
        let hh = if k == 0 { h } else { h * rng.range(0.4, 0.7) };
        let n = (hh / v.s) as i32;
        for z in -2..n {
            let t = f64::from(z.max(0)) / f64::from(n);
            let r = w * 0.45 * (1.0 - t).powf(0.8) * (if k == 0 { 1.0 } else { 0.6 });
            for i in v.r(c.x - r, c.x + r) {
                for j in v.r(c.y - r, c.y + r) {
                    let p = dv((f64::from(i) + 0.5) * v.s, (f64::from(j) + 0.5) * v.s, 0.0);
                    if (p - c).truncate().length() <= r {
                        v.put(IVec3::new(i, j, z), Role::Rock);
                    }
                }
            }
        }
    }
    v.shade(
        &[Role::Rock],
        Role::RockHi,
        Role::Rock,
        Role::RockDark,
        Role::RockDark,
        0.25,
    );
}

fn crust(v: &mut Vox, rng: &mut Rng, h: f64, w: f64) {
    let half = w * 0.5;
    let n = (h / v.s).max(1.0) as i32;
    let seed = rng.unit() * 50.0;
    for i in v.r(-half, half) {
        for j in v.r(-half, half) {
            let p = dv((f64::from(i) + 0.5) * v.s, (f64::from(j) + 0.5) * v.s, seed);
            // Cracks along cell borders of a noise pattern.
            let crack = (v.noise(p * 2.5, 31) * 4.0).fract().abs() < 0.12;
            if p.truncate().length() <= half && !crack {
                for k in -1..n {
                    v.put(IVec3::new(i, j, k), Role::Rock);
                }
            }
        }
    }
    v.shade(
        &[Role::Rock],
        Role::RockHi,
        Role::Rock,
        Role::RockDark,
        Role::Rock,
        0.0,
    );
}
