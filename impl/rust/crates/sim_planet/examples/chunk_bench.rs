//! Times block chunk builds on Tessera, per level of detail:
//! `cargo run --release -p sim_planet --example chunk_bench`.

use std::sync::Arc;

fn main() {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../../content/data/star_systems/tessera.json");
    let sys = sim_core::StarSystem::from_path(path).expect("tessera");
    let body = sys
        .bodies
        .iter()
        .find(|b| b.terrain.is_some())
        .expect("a planet");
    let t = std::time::Instant::now();
    let terrain = Arc::new(sim_planet::Terrain::of(body).expect("terrain"));
    eprintln!("world generated in {:.1?}", t.elapsed());
    let globe = terrain.globe().expect("geography").clone();
    let r = globe
        .regions
        .iter()
        .position(|_| true)
        .and_then(|_| (0..globe.regions.len()).find(|&r| globe.kind(r).id == "vale"))
        .unwrap_or(0);
    let centre = globe.regions[r].centre;
    let g = sim_planet::BlockGround::new(terrain.clone(), centre, None);
    for level in 0..=4u8 {
        let keys = g.lod_around(
            centre,
            sim_planet::BlockGround::lod_size(level) * 3.0,
            level,
        );
        let mut sums = [0.0f32; 3];
        let mut tris = 0;
        let mut count = 0;
        let start = std::time::Instant::now();
        for k in keys.iter().take(12) {
            if let Some(c) = g.lod_chunk(*k) {
                for (s, t) in sums.iter_mut().zip(c.timings) {
                    *s += t;
                }
                tris += c.mesh.triangles();
                count += 1;
            }
        }
        let n = count.max(1) as f32;
        eprintln!(
            "level {level}: {count} chunks, {:.1} ms each (heights {:.1}, cells {:.1}, mesh {:.1}), {} triangles each",
            start.elapsed().as_secs_f32() * 1e3 / n,
            sums[0] / n,
            sums[1] / n,
            sums[2] / n,
            tris / count.max(1)
        );
    }
}
