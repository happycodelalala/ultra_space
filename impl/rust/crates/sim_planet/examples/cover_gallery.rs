//! Writes every ground-cover species (4 variants each, one row per species) as a PLY with
//! vertex colours, for review renders: `cargo run -p sim_planet --example cover_gallery -- out.ply`.
use std::io::Write;

fn main() {
    let out = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "gallery.ply".into());
    let lod: usize = std::env::args()
        .nth(2)
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let from: usize = std::env::args()
        .nth(3)
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let to: usize = std::env::args()
        .nth(4)
        .and_then(|v| v.parse().ok())
        .unwrap_or(1000);
    let w = sim_planet::cover::CoverWorld::new(11, 1.0);
    let (mut verts, mut faces) = (Vec::new(), Vec::new());
    let mut y = 0.0f32;
    let mut labels = Vec::new();
    for i in from..to.min(w.species_count()) {
        let s = w.species(i);
        let step = (s.width_m[1] as f32 * 1.3).max(1.0);
        labels.push(format!("{} {}", y, s.id));
        for v in 0..4.min(s.variants as usize) {
            let (pos, col, idx) = w.variant_mesh(i, v, lod);
            let base = verts.len() as u32;
            for (p, c) in pos.iter().zip(&col) {
                verts.push(([p[0] + v as f32 * step, p[1] + y, p[2]], *c));
            }
            for t in idx.chunks(3) {
                faces.push([t[0] + base, t[1] + base, t[2] + base]);
            }
        }
        y += (s.width_m[1] as f32 * 1.4).max(1.2);
    }
    let mut f = std::io::BufWriter::new(std::fs::File::create(&out).unwrap());
    writeln!(f, "ply\nformat ascii 1.0\nelement vertex {}\nproperty float x\nproperty float y\nproperty float z\nproperty uchar red\nproperty uchar green\nproperty uchar blue\nelement face {}\nproperty list uchar int vertex_indices\nend_header", verts.len(), faces.len()).unwrap();
    for (p, c) in &verts {
        let s = |x: f32| (x.max(0.0).powf(1.0 / 2.2) * 255.0).round().min(255.0) as u8;
        writeln!(
            f,
            "{} {} {} {} {} {}",
            p[0],
            p[1],
            p[2],
            s(c[0]),
            s(c[1]),
            s(c[2])
        )
        .unwrap();
    }
    for t in &faces {
        writeln!(f, "3 {} {} {}", t[0], t[1], t[2]).unwrap();
    }
    eprintln!(
        "{} vertices, {} triangles\n{}",
        verts.len(),
        faces.len(),
        labels.join("\n")
    );
}
