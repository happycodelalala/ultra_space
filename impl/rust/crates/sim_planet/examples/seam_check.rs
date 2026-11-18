use glam::DVec2;
use std::sync::Arc;
fn covered(c: &sim_planet::blocks::ChunkMesh, p: DVec2) -> bool {
    let g = &c.ground;
    c.mesh.indices.chunks(3).any(|t| {
        let [a, b, d] = [t[0], t[1], t[2]]
            .map(|i| DVec2::new(g[i as usize][0] as f64, g[i as usize][1] as f64));
        let s = |p1: DVec2, p2: DVec2, p3: DVec2| {
            (p1.x - p3.x) * (p2.y - p3.y) - (p2.x - p3.x) * (p1.y - p3.y)
        };
        let (d1, d2, d3) = (s(p, a, b), s(p, b, d), s(p, d, a));
        let neg = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
        let pos = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
        !(neg && pos)
    })
}
fn main() {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../../content/data/star_systems/tessera.json");
    let sys = sim_core::StarSystem::from_path(path).unwrap();
    let body = sys.bodies.iter().find(|b| b.terrain.is_some()).unwrap();
    let t = Arc::new(sim_planet::Terrain::of(body).unwrap());
    let village = glam::DVec3::new(-0.784465465, -0.532025000, 0.318690028);
    let g = sim_planet::BlockGround::new(t.clone(), village, None)
        .with_style(sim_planet::GroundStyle::Faceted);
    let rook = t
        .globe()
        .unwrap()
        .regions
        .iter()
        .find(|r| r.name == "Rook Vale")
        .unwrap()
        .centre;
    let (face, p) = g.locate(rook);
    for level in [1u8, 2, 3] {
        let size = sim_planet::BlockGround::lod_size(level);
        let (i0, j) = ((p.x / size).floor() as i32, (p.y / size).floor() as i32);
        for side in [1i32, -1] {
            // A fine chunk whose coarse neighbour on `side` is a different coarse chunk.
            let i = if side > 0 { i0 | 1 } else { i0 & !1 };
            let fine = g.lod_chunk((level, face, i, j)).unwrap();
            let ni = if side > 0 { i + 1 } else { i - 1 };
            let coarse = g.lod_chunk((level + 1, face, ni >> 1, j >> 1)).unwrap();
            let x = if side > 0 {
                (i + 1) as f64 * size
            } else {
                i as f64 * size
            };
            let mut gaps = 0;
            let n = 400;
            for k in 8..n - 8 {
                for dx in [-1.0, -0.6, -0.3, 0.0, 0.3, 0.6, 1.0] {
                    let q = DVec2::new(
                        x + dx * size / 64.0,
                        j as f64 * size + (k as f64 + 0.5) / n as f64 * size,
                    );
                    if !covered(&fine, q) && !covered(&coarse, q) {
                        gaps += 1;
                    }
                }
            }
            println!(
                "fine level {level} / coarse {} on side {side}: {gaps} uncovered of {}",
                level + 1,
                (n - 16) * 7
            );
        }
    }
}
