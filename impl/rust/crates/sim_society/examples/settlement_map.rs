//! Draws a body's settlements on its biome map:
//! `cargo run --release -p sim_society --example settlement_map -- [body id] [out.ppm] [width]`.
//! Home regions are outlined in white; settlements are squares
//! in their faction's colour (seats larger, with a white border), sized by kind.

use std::io::Write;
use std::path::PathBuf;

use ai_gateway::{ContentDirector, ReplayProvider};
use sim_core::StarSystem;
use sim_planet::map::{biome_map, direction, pixel};
use sim_planet::{Climate, Terrain};
use sim_society::{SizeClass, generate, generate_settlements};

fn main() {
    let mut args = std::env::args().skip(1);
    let id = args.next().unwrap_or_else(|| "meridian".into());
    let out = PathBuf::from(
        args.next()
            .unwrap_or_else(|| format!("{id}-settlements.ppm")),
    );
    let width: usize = args.next().map_or(1024, |w| w.parse().expect("width"));
    let height = width / 2;
    let content = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../../content");
    let system = StarSystem::from_path(content.join("data/star_systems/halcyon.json")).unwrap();
    let body = system.bodies.iter().position(|b| b.id == id).expect("body");
    let director = ContentDirector::new(Box::new(
        ReplayProvider::load_dir(content.join("lore")).unwrap(),
    ));
    let mut society = generate(&system, body, &director);
    let settlements = generate_settlements(&system, &mut society, &director);
    let b = &system.bodies[body];
    let mut img = biome_map(&Terrain::of(b).unwrap(), &Climate::of(b).unwrap(), width);
    let mut put = |x: i64, y: i64, c: [f64; 3]| {
        let (x, y) = (
            x.rem_euclid(width as i64) as usize,
            y.clamp(0, height as i64 - 1) as usize,
        );
        let i = (y * width + x) * 3;
        for k in 0..3 {
            img[i + k] = (c[k].clamp(0.0, 1.0) * 255.0) as u8;
        }
    };
    for c in &society.cultures {
        let h = c.people.home;
        let centre = direction(h.latitude_deg.to_radians(), h.longitude_deg.to_radians());
        let axis = centre.any_orthonormal_vector();
        for k in 0..720 {
            let d = glam::DQuat::from_axis_angle(centre, k as f64 / 720.0 * std::f64::consts::TAU)
                * (glam::DQuat::from_axis_angle(axis, h.radius_deg.to_radians()) * centre);
            let (x, y) = pixel(d, width);
            put(x as i64, y as i64, [1.0, 1.0, 1.0]);
        }
    }
    for s in &settlements {
        let (x, y) = pixel(s.direction, width);
        let r: i64 = match s.size {
            SizeClass::Town | SizeClass::City | SizeClass::Metropolis | SizeClass::Megalopolis => 4,
            SizeClass::Village => 3,
            _ => 2,
        };
        let colour = society.factions[s.faction].colour;
        for dy in -r..=r {
            for dx in -r..=r {
                let edge = dx.abs() == r || dy.abs() == r;
                let c = if edge && s.seat {
                    [1.0, 1.0, 1.0]
                } else if edge {
                    [0.0; 3]
                } else {
                    colour
                };
                put(x as i64 + dx, y as i64 + dy, c);
            }
        }
    }
    let mut file = std::fs::File::create(&out).expect("create output");
    write!(file, "P6\n{width} {height}\n255\n").unwrap();
    file.write_all(&img).unwrap();
    for s in &settlements {
        println!(
            "{:12} {:22} {:8} {:5} people  {:24} score {:.2}{}",
            s.id,
            s.name,
            s.word,
            s.population,
            society.factions[s.faction].name,
            s.site_score,
            if s.seat { "  (seat)" } else { "" }
        );
    }
    for (ci, c) in society.cultures.iter().enumerate() {
        let mine: Vec<_> = settlements.iter().filter(|s| s.culture == ci).collect();
        let mut kinds: Vec<String> = Vec::new();
        let mut words: Vec<&str> = mine.iter().map(|s| s.word.as_str()).collect();
        words.sort_unstable();
        words.dedup();
        for w in words {
            let n = mine.iter().filter(|s| s.word == w).count();
            kinds.push(format!("{n} {w}"));
        }
        println!(
            "{}: {} settlements ({}), {} people",
            c.profile.name,
            mine.len(),
            kinds.join(", "),
            mine.iter().map(|s| s.population).sum::<u32>()
        );
    }
    for f in &society.fallbacks {
        println!("fallback: {f}");
    }
    eprintln!("wrote {}", out.display());
}
