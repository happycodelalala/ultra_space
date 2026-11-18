use std::sync::Arc;
fn main() {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../../content/data/star_systems/tessera.json");
    let sys = sim_core::StarSystem::from_path(path).unwrap();
    let body = sys.bodies.iter().find(|b| b.terrain.is_some()).unwrap();
    let t = Arc::new(sim_planet::Terrain::of(body).unwrap());
    let village = glam::DVec3::new(-0.784465465, -0.532025000, 0.318690028);
    let t0 = std::time::Instant::now();
    let cover = Arc::new(sim_planet::cover::CoverWorld::new(11, 1.0));
    println!("cover built in {:?}", t0.elapsed());
    let g = sim_planet::BlockGround::new(t.clone(), village, None)
        .with_style(sim_planet::GroundStyle::Faceted)
        .with_cover(cover.clone());
    let globe = t.globe().unwrap().clone();
    let mut by_kind: std::collections::BTreeMap<String, glam::DVec3> =
        std::collections::BTreeMap::new();
    for d in sim_planet::fibonacci_directions(20000) {
        if t.height(d, 50.0) > 5.0 {
            by_kind.entry(format!("{:?}", globe.cover(d))).or_insert(d);
        }
    }
    let plan_levels = [(0u8, 30usize), (1, 30), (2, 30), (3, 30), (4, 30)];
    for (kind, dir) in &by_kind {
        let (face, p) = g.locate(*dir);
        let mut line = format!("{kind:10}");
        let mut total = 0usize;
        for (level, n) in plan_levels {
            let size = sim_planet::BlockGround::lod_size(level);
            let key = (
                level,
                face,
                (p.x / size).floor() as i32,
                (p.y / size).floor() as i32,
            );
            let s = std::time::Instant::now();
            let m = cover.chunk_mesh(&g, key);
            let ms = s.elapsed().as_secs_f64() * 1e3;
            let (k, tri) = m.map_or((0, 0), |m| (m.instances, m.indices.len() / 3));
            total += tri * n;
            line += &format!(" | L{level}: {k} {}k {ms:.0}ms", tri / 1000);
        }
        println!("{line} | ~{:.1}M in view", total as f64 / 1e6);
    }
}
