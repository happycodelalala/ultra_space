//! Lays out every settlement of a body and draws a few from above:
//! `cargo run --release -p sim_society --example layout_map -- [body id] [out.ppm]`.
//! Prints each settlement's buildings and the time taken; the picture shows one of each kind
//! (panels of 400 px, north up): ground shaded by height (blue under water), streets grey,
//! buildings by use, foundations outlined.

use std::io::Write;
use std::path::PathBuf;
use std::time::Instant;

use ai_gateway::{ContentDirector, ReplayProvider};
use glam::DVec2;
use sim_core::StarSystem;
use sim_planet::{Foundations, Terrain};
use sim_society::layout::FOUNDATION_BLEND;
use sim_society::{Kits, Layout, generate, generate_layout, generate_settlements};

const PANEL: usize = 400;

fn colour(uses: &[String]) -> [u8; 3] {
    match uses.first().map(String::as_str).unwrap_or("") {
        "home" => [200, 120, 90],
        "shop" => [220, 160, 80],
        "farm" => [150, 120, 60],
        "hall" | "port" => [230, 230, 120],
        "shrine" => [240, 240, 240],
        "inn" => [230, 90, 160],
        "smithy" => [60, 60, 60],
        "mill" => [120, 80, 200],
        "square" | "market" => [80, 200, 230],
        "tree" => [40, 140, 50],
        "pad" => [250, 150, 40],
        "hangar" | "workshop" => [140, 150, 170],
        "lab" => [120, 200, 220],
        "power" | "comms" => [250, 60, 60],
        _ => [255, 0, 255],
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let id = args.next().unwrap_or_else(|| "meridian".into());
    let out = PathBuf::from(args.next().unwrap_or_else(|| format!("{id}-layouts.ppm")));
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../..");
    let content = root.join("content");
    let system = StarSystem::from_path(content.join("data/star_systems/halcyon.json")).unwrap();
    let body = system.bodies.iter().position(|b| b.id == id).expect("body");
    let director = ContentDirector::new(Box::new(
        ReplayProvider::load_dir(content.join("lore")).unwrap(),
    ));
    let kits = Kits::load_dir(&root.join("assets/export/buildings")).unwrap();
    let mut society = generate(&system, body, &director);
    let settlements = generate_settlements(
        &system,
        &mut society,
        &director,
        &sim_society::SettlementPatterns::load_dir(
            &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../../../content/data/settlement_patterns"),
        )
        .unwrap(),
    );
    let terrain = Terrain::of(&system.bodies[body]).unwrap();

    let start = Instant::now();
    let mut layouts = Vec::new();
    for s in &settlements {
        let profile = &society.cultures[s.culture].profile;
        let kit = &kits.0[&profile.building_kit];
        let t = Instant::now();
        let l = generate_layout(s, profile.tech_tier, kit, &terrain);
        println!(
            "{:<12} {:<8} {:>5} people: {:>3} buildings, {:>2} unplaced, {:>3} streets, {:.1} ms",
            s.name,
            s.lore_kind(),
            s.population,
            l.buildings.len(),
            l.unplaced,
            l.streets.len(),
            t.elapsed().as_secs_f64() * 1e3
        );
        layouts.push(l);
    }
    println!("all layouts: {:.0} ms", start.elapsed().as_secs_f64() * 1e3);
    let sites = layouts
        .iter()
        .map(|l| l.site(&kits.0[&l.kit], terrain.radius))
        .collect();
    let flat = terrain.with_foundations(std::sync::Arc::new(Foundations::new(sites)));

    // One of each kind (the biggest).
    let mut shown: Vec<usize> = Vec::new();
    for kind in ["town", "village", "hamlet", "colony", "port", "outpost"] {
        if let Some(i) = (0..settlements.len())
            .filter(|&i| settlements[i].lore_kind() == kind)
            .max_by_key(|&i| settlements[i].population)
        {
            shown.push(i);
        }
    }
    let (w, h) = (PANEL * 3, PANEL * shown.len().div_ceil(3));
    let mut img = vec![30u8; w * h * 3];
    for (n, &i) in shown.iter().enumerate() {
        let (ox, oy) = ((n % 3) * PANEL, (n / 3) * PANEL);
        let l: &Layout = &layouts[i];
        let kit = &kits.0[&l.kit];
        let extent = l
            .buildings
            .iter()
            .map(|p| DVec2::new(p.x, p.z).length() + 12.0)
            .fold(60.0, f64::max);
        let scale = PANEL as f64 / (2.0 * extent); // px per m
        let to_m = |px: usize, py: usize| {
            DVec2::new(
                (px as f64 - PANEL as f64 / 2.0) / scale,
                (py as f64 - PANEL as f64 / 2.0) / scale,
            )
        };
        let base = settlements[i].height;
        let mut put = |px: i64, py: i64, c: [u8; 3]| {
            if px >= 0 && py >= 0 && (px as usize) < PANEL && (py as usize) < PANEL {
                let k = ((oy + py as usize) * w + ox + px as usize) * 3;
                img[k..k + 3].copy_from_slice(&c);
            }
        };
        // Ground (x east to the right, z south down: north up).
        for py in 0..PANEL {
            for px in 0..PANEL {
                let m = to_m(px, py);
                let dir = l.direction(m.x, m.y, terrain.radius);
                let g = flat.height(dir, 2.0);
                let c = if g < 0.0 {
                    [30, 60, 140]
                } else {
                    let v = (110.0 + (g - base) * 3.0).clamp(40.0, 220.0) as u8;
                    [v / 2, v, v / 3]
                };
                put(px as i64, py as i64, c);
            }
        }
        let px = |m: DVec2| {
            (
                (m.x * scale) as i64 + PANEL as i64 / 2,
                (m.y * scale) as i64 + PANEL as i64 / 2,
            )
        };
        for &(a, b) in &l.streets {
            let (a, b) = (l.nodes[a as usize], l.nodes[b as usize]);
            let steps = ((b - a).length() * scale).ceil().max(1.0) as usize;
            for k in 0..=steps {
                let (x, y) = px(a.lerp(b, k as f64 / steps as f64));
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    put(x + dx, y + dy, [150, 150, 140]);
                }
            }
        }
        for p in &l.buildings {
            let f = Layout::footprint(kit, p);
            let c = colour(&kit.buildings[p.building].uses);
            let (s, co) = f.yaw.sin_cos();
            let half = [f.half[0] - 0.8, f.half[1] - 0.8];
            let r = DVec2::new(f.half[0] + FOUNDATION_BLEND, f.half[1] + FOUNDATION_BLEND).length();
            let (cx, cy) = px(DVec2::new(f.x, f.z));
            let rp = (r * scale).ceil() as i64;
            for y in -rp..=rp {
                for x in -rp..=rp {
                    let d = DVec2::new(x as f64, y as f64) / scale;
                    let local = DVec2::new(co * d.x - s * d.y, s * d.x + co * d.y);
                    let (ax, az) = (local.x.abs(), local.y.abs());
                    if ax <= half[0] && az <= half[1] {
                        // The front (+Z) edge darker, to show which way it faces.
                        let front = local.y > half[1] - 1.5;
                        let c = if front { c.map(|v| v / 2) } else { c };
                        put(cx + x, cy + y, c);
                    }
                }
            }
        }
        println!(
            "panel {n}: {} ({}), {:.0} m across",
            settlements[i].name,
            settlements[i].lore_kind(),
            2.0 * extent
        );
    }
    let mut f = std::fs::File::create(&out).unwrap();
    write!(f, "P6\n{w} {h}\n255\n").unwrap();
    f.write_all(&img).unwrap();
    println!("wrote {}", out.display());
}
