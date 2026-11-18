//! Renders a planet's generated world as maps (relief with rivers, cover, regions) to a PNG:
//! `cargo run --release -p sim_planet --example globe_map -- out.png [plates] [seed]`.

use std::io::Write;

use glam::DVec3;
use sim_planet::globe::{Cover, GlobeWorld};

fn cover_colour(c: Cover) -> [f64; 3] {
    match c {
        Cover::DeepSea => [0.04, 0.16, 0.38],
        Cover::Sea => [0.06, 0.24, 0.5],
        Cover::Shelf => [0.12, 0.42, 0.6],
        Cover::Lake => [0.1, 0.38, 0.62],
        Cover::River => [0.12, 0.42, 0.7],
        Cover::Salt => [0.9, 0.88, 0.82],
        Cover::Ice => [0.94, 0.96, 0.99],
        Cover::Snow => [0.92, 0.94, 0.97],
        Cover::Tundra => [0.55, 0.53, 0.44],
        Cover::Taiga => [0.22, 0.33, 0.26],
        Cover::ColdDesert => [0.66, 0.62, 0.5],
        Cover::Desert => [0.84, 0.7, 0.46],
        Cover::Steppe => [0.7, 0.66, 0.4],
        Cover::Grass => [0.45, 0.58, 0.27],
        Cover::Forest => [0.22, 0.4, 0.18],
        Cover::Rainforest => [0.14, 0.33, 0.14],
        Cover::Savanna => [0.66, 0.62, 0.32],
        Cover::Seasonal => [0.3, 0.45, 0.18],
        Cover::Jungle => [0.1, 0.3, 0.1],
    }
}

fn png(path: &str, w: usize, h: usize, rgb: &[u8]) {
    fn crc(data: &[u8]) -> u32 {
        let mut c = 0xFFFF_FFFFu32;
        for &b in data {
            c ^= u32::from(b);
            for _ in 0..8 {
                c = if c & 1 != 0 {
                    0xEDB8_8320 ^ (c >> 1)
                } else {
                    c >> 1
                };
            }
        }
        !c
    }
    let mut raw = Vec::with_capacity((w * 3 + 1) * h);
    for row in rgb.chunks(w * 3) {
        raw.push(0);
        raw.extend_from_slice(row);
    }
    let mut z = vec![0x78, 0x01];
    for (k, block) in raw.chunks(65_535).enumerate() {
        let last = (k + 1) * 65_535 >= raw.len();
        z.push(u8::from(last));
        let len = block.len() as u16;
        z.extend_from_slice(&len.to_le_bytes());
        z.extend_from_slice(&(!len).to_le_bytes());
        z.extend_from_slice(block);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &x in &raw {
        a = (a + u32::from(x)) % 65_521;
        b = (b + a) % 65_521;
    }
    z.extend_from_slice(&((b << 16) | a).to_be_bytes());
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut chunk = |kind: &[u8], data: &[u8]| {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut c = kind.to_vec();
        c.extend_from_slice(data);
        out.extend_from_slice(&c);
        out.extend_from_slice(&crc(&c).to_be_bytes());
    };
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&(w as u32).to_be_bytes());
    ihdr.extend_from_slice(&(h as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    chunk(b"IHDR", &ihdr);
    chunk(b"IDAT", &z);
    chunk(b"IEND", &[]);
    std::fs::File::create(path)
        .and_then(|mut f| f.write_all(&out))
        .expect("write png");
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let out = args.get(1).cloned().unwrap_or_else(|| "globe.png".into());
    let plates = args.get(2).and_then(|v| v.parse().ok()).unwrap_or(12);
    let seed = args.get(3).and_then(|v| v.parse().ok()).unwrap_or(11);
    let t = std::time::Instant::now();
    let w = GlobeWorld::new(92_000.0, plates, seed);
    eprintln!("generated in {:.1?}", t.elapsed());
    let mut kinds = std::collections::BTreeMap::new();
    for r in 0..w.regions.len() {
        *kinds.entry(w.kind(r).id.clone()).or_insert(0) += 1;
    }
    eprintln!("{} regions {kinds:?}", w.regions.len());
    let (lo, hi) = w.range();
    eprintln!("range {lo:.0} .. {hi:.0}");
    let (width, height) = (1440usize, 720usize);
    let dir = |x: usize, y: usize| {
        let lat = (0.5 - (y as f64 + 0.5) / height as f64) * std::f64::consts::PI;
        let lon = (x as f64 + 0.5) / width as f64 * std::f64::consts::TAU - std::f64::consts::PI;
        DVec3::new(lat.cos() * lon.cos(), lat.sin(), lat.cos() * lon.sin())
    };
    let mut elev = vec![0.0; width * height];
    let mut cover = vec![Cover::Sea; width * height];
    let mut region = vec![0usize; width * height];
    let mut river = vec![false; width * height];
    let mut sea = 0usize;
    for y in 0..height {
        for x in 0..width {
            let d = dir(x, y);
            let i = y * width + x;
            elev[i] = w.elevation(d, 300.0);
            cover[i] = w.cover(d);
            region[i] = w.region_at(d);
            river[i] = w.river(d);
            sea += usize::from(elev[i] < 0.0);
        }
    }
    eprintln!(
        "sea share (by map area) {:.2}",
        sea as f64 / (width * height) as f64
    );
    let shade = |x: usize, y: usize| {
        let e = |xx: usize, yy: usize| elev[yy.min(height - 1) * width + (xx % width)].max(0.0);
        let dx = e(x + 1, y) - e(x + width - 1, y);
        let dy = e(x, y + 1) - e(x, y.saturating_sub(1));
        (1.0 + (dy - dx) / 1_200.0).clamp(0.4, 1.4)
    };
    let mut hue: Vec<[f64; 3]> = (0..w.regions.len())
        .map(|r| {
            let h = (r as f64 * 0.618_034) % 1.0;
            let c = |o: f64| 0.5 + 0.35 * ((h + o) * std::f64::consts::TAU).cos();
            [c(0.0), c(0.33), c(0.67)]
        })
        .collect();
    hue.push([0.0; 3]);
    let mut rgb = Vec::with_capacity(width * height * 9);
    for panel in 0..3 {
        for y in 0..height {
            for x in 0..width {
                let i = y * width + x;
                let e = elev[i];
                let c = match panel {
                    0 => {
                        if river[i] && e >= 0.0 {
                            [0.15, 0.4, 0.85]
                        } else if e < 0.0 {
                            let z = (e / 6_000.0).max(-1.0);
                            [
                                0.05 + 0.1 * (1.0 + z),
                                0.2 + 0.25 * (1.0 + z),
                                0.45 + 0.25 * (1.0 + z),
                            ]
                        } else {
                            let z = (e / 7_000.0).min(1.0);
                            let s = shade(x, y);
                            [
                                (0.35 + 0.6 * z) * s,
                                (0.5 + 0.35 * z) * s,
                                (0.3 + 0.6 * z) * s,
                            ]
                        }
                    }
                    1 => {
                        let c = cover_colour(cover[i]);
                        let s = if e >= 0.0 { shade(x, y) } else { 1.0 };
                        [c[0] * s, c[1] * s, c[2] * s]
                    }
                    _ => {
                        let r = region[i];
                        let edge = region[y * width + (x + 1) % width] != r
                            || region[(y + 1).min(height - 1) * width + x] != r;
                        if edge {
                            [0.05; 3]
                        } else if e < 0.0 && !w.open_sea(dir(x, y)) {
                            hue[r]
                        } else if e < 0.0 {
                            hue[r].map(|v| v * 0.55)
                        } else {
                            hue[r]
                        }
                    }
                };
                rgb.extend(c.iter().map(|v| (v.clamp(0.0, 1.0) * 255.0) as u8));
            }
        }
    }
    png(&out, width, height * 3, &rgb);
    eprintln!("wrote {out}");
}
