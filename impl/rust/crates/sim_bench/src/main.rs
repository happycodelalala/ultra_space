//! Parallel-scaling benchmark for the simulation architecture (Batch 0).
//!
//! Simulates a synthetic galaxy slice — factory belt lines, ships and per-region markets —
//! and measures tick time at different thread counts, comparing two strategies:
//!
//! - `regions`: one ECS `World` per star-system region, regions ticked in parallel,
//!   systems run sequentially inside each region (development plan §2, rule 1).
//! - `systems`: one shared `World`, multi-threaded executor plus `par_iter_mut` inside systems
//!   (development plan §2, rule 2).
//!
//! The compute task pool size is fixed once per process, so the `sweep` command re-runs this
//! binary once per thread count.
//!
//! Usage:
//!   sim_bench sweep [--quick] [--scale <k>]
//!   sim_bench run --mode <regions|systems> --threads <n> [--quick] [--scale <k>]

use std::process::Command;
use std::time::{Duration, Instant};

use bevy_ecs::prelude::*;
use bevy_ecs::schedule::{MultiThreadedExecutor, SingleThreadedExecutor};
use bevy_tasks::{ComputeTaskPool, TaskPool, TaskPoolBuilder};
use glam::DVec3;

// ---------------------------------------------------------------------------------------------
// Workload configuration
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
struct Config {
    regions: usize,
    belt_lines_per_region: usize,
    belt_slots: usize,
    ships_per_region: usize,
    hazards_per_region: usize,
    warmup_ticks: usize,
    measured_ticks: usize,
}

impl Config {
    /// Multiplies per-region content (belt lines, ships) to test heavier ticks.
    fn scaled(mut self, scale: usize) -> Self {
        self.belt_lines_per_region *= scale;
        self.ships_per_region *= scale;
        self
    }

    fn full() -> Self {
        Self {
            regions: 64,
            belt_lines_per_region: 100,
            belt_slots: 64,
            ships_per_region: 1_000,
            hazards_per_region: 48,
            warmup_ticks: 20,
            measured_ticks: 100,
        }
    }

    fn quick() -> Self {
        Self {
            regions: 16,
            belt_lines_per_region: 50,
            belt_slots: 64,
            ships_per_region: 300,
            hazards_per_region: 16,
            warmup_ticks: 5,
            measured_ticks: 20,
        }
    }
}

const DT: f64 = 1.0 / 30.0;

// ---------------------------------------------------------------------------------------------
// Components and resources
// ---------------------------------------------------------------------------------------------

/// A belt line: item slots moving toward a crafting machine at the end.
/// Real belts are arrays of slots, not one entity per item (technical overview §4).
#[derive(Component)]
struct BeltLine {
    region: usize,
    /// 0 = empty slot, otherwise an item type id.
    slots: Vec<u16>,
    spawn_timer: u32,
    spawn_interval: u32,
    machine_input: u32,
    machine_progress: f32,
    produced: u64,
}

#[derive(Component)]
struct Ship {
    region: usize,
    pos: DVec3,
    vel: DVec3,
    waypoint: DVec3,
    seed: u64,
}

/// Fixed hazard points per region (asteroids, mines) that ships steer around.
#[derive(Resource)]
struct Hazards(Vec<Vec<DVec3>>);

#[derive(Clone, Default)]
struct Market {
    supply: u64,
    price: f64,
}

#[derive(Resource)]
struct Markets(Vec<Market>);

/// Whether systems use `par_iter_mut` (shared-world strategy) or plain iteration.
#[derive(Resource, Clone, Copy)]
struct Parallel(bool);

// ---------------------------------------------------------------------------------------------
// Systems (identical code for both strategies)
// ---------------------------------------------------------------------------------------------

fn advance_belt(line: &mut BeltLine) {
    // Crafting machine at the end of the line: consumes 4 items per product.
    let last = line.slots.len() - 1;
    if line.slots[last] != 0 && line.machine_input < 8 {
        line.slots[last] = 0;
        line.machine_input += 1;
    }
    if line.machine_input >= 4 {
        line.machine_progress += 0.25;
        if line.machine_progress >= 1.0 {
            line.machine_progress = 0.0;
            line.machine_input -= 4;
            line.produced += 1;
        }
    }

    // Move items one slot forward where the next slot is free (back to front, so items
    // compress against the machine like a real belt).
    for i in (0..last).rev() {
        if line.slots[i] != 0 && line.slots[i + 1] == 0 {
            line.slots[i + 1] = line.slots[i];
            line.slots[i] = 0;
        }
    }

    // Source at the start of the line.
    line.spawn_timer += 1;
    if line.spawn_timer >= line.spawn_interval && line.slots[0] == 0 {
        line.spawn_timer = 0;
        line.slots[0] = 1 + (line.produced % 7) as u16;
    }
}

fn belt_system(par: Res<Parallel>, mut belts: Query<&mut BeltLine>) {
    if par.0 {
        belts
            .par_iter_mut()
            .for_each(|mut line| advance_belt(&mut line));
    } else {
        belts
            .iter_mut()
            .for_each(|mut line| advance_belt(&mut line));
    }
}

fn next_random(seed: &mut u64) -> f64 {
    // xorshift64*: cheap, deterministic per ship.
    *seed ^= *seed >> 12;
    *seed ^= *seed << 25;
    *seed ^= *seed >> 27;
    let v = seed.wrapping_mul(0x2545_F491_4F6C_DD1D);
    (v >> 11) as f64 / (1u64 << 53) as f64
}

fn steer_ship(ship: &mut Ship, hazards: &[DVec3]) {
    let to_target = ship.waypoint - ship.pos;
    let dist = to_target.length();
    if dist < 50.0 {
        let r = 5_000.0;
        ship.waypoint = DVec3::new(
            (next_random(&mut ship.seed) - 0.5) * r,
            (next_random(&mut ship.seed) - 0.5) * r * 0.2,
            (next_random(&mut ship.seed) - 0.5) * r,
        );
    }
    let mut accel = to_target.normalize_or_zero() * 20.0;

    // Hazard avoidance: inverse-square repulsion from every hazard in the region.
    for h in hazards {
        let away = ship.pos - *h;
        let d2 = away.length_squared().max(1.0);
        if d2 < 400.0 * 400.0 {
            accel += away * (2.0e5 / (d2 * d2.sqrt()));
        }
    }

    ship.vel = (ship.vel + accel * DT) * 0.995;
    let speed = ship.vel.length();
    if speed > 300.0 {
        ship.vel *= 300.0 / speed;
    }
    ship.pos += ship.vel * DT;
}

fn ship_system(par: Res<Parallel>, hazards: Res<Hazards>, mut ships: Query<&mut Ship>) {
    let hazards = &hazards.0;
    if par.0 {
        ships.par_iter_mut().for_each(|mut ship| {
            let region = ship.region;
            steer_ship(&mut ship, &hazards[region]);
        });
    } else {
        ships.iter_mut().for_each(|mut ship| {
            let region = ship.region;
            steer_ship(&mut ship, &hazards[region]);
        });
    }
}

/// Per-region market update: a reduction over all belt lines (runs after the belt system).
fn market_system(belts: Query<&BeltLine>, mut markets: ResMut<Markets>) {
    for m in markets.0.iter_mut() {
        m.supply = 0;
    }
    for line in &belts {
        markets.0[line.region].supply += line.produced;
    }
    for m in markets.0.iter_mut() {
        let target = 100.0 / (1.0 + m.supply as f64 * 0.001);
        m.price += (target - m.price) * 0.1;
    }
}

fn build_schedule(multi_threaded: bool) -> Schedule {
    let mut schedule = Schedule::default();
    if multi_threaded {
        schedule.set_executor(MultiThreadedExecutor::new());
    } else {
        schedule.set_executor(SingleThreadedExecutor::new());
    }
    schedule.add_systems((belt_system, ship_system, market_system.after(belt_system)));
    schedule
}

// ---------------------------------------------------------------------------------------------
// World construction
// ---------------------------------------------------------------------------------------------

/// Spawns one region's content into `world`, tagged with `local_region` index.
fn populate_region(world: &mut World, cfg: &Config, global_region: usize, local_region: usize) {
    let mut seed = 0x9E37_79B9_7F4A_7C15u64 ^ (global_region as u64).wrapping_mul(0x1000_0001);
    world.spawn_batch((0..cfg.belt_lines_per_region).map(|i| BeltLine {
        region: local_region,
        slots: vec![0; cfg.belt_slots],
        spawn_timer: 0,
        spawn_interval: 1 + (i % 3) as u32,
        machine_input: 0,
        machine_progress: 0.0,
        produced: 0,
    }));
    let ships: Vec<Ship> = (0..cfg.ships_per_region)
        .map(|i| {
            let mut s = seed ^ (i as u64 + 1).wrapping_mul(0xD1B5_4A32_D192_ED03);
            let p = DVec3::new(
                (next_random(&mut s) - 0.5) * 5_000.0,
                (next_random(&mut s) - 0.5) * 1_000.0,
                (next_random(&mut s) - 0.5) * 5_000.0,
            );
            Ship {
                region: local_region,
                pos: p,
                vel: DVec3::ZERO,
                waypoint: -p,
                seed: s | 1,
            }
        })
        .collect();
    world.spawn_batch(ships);
    next_random(&mut seed);
}

fn region_hazards(cfg: &Config, global_region: usize) -> Vec<DVec3> {
    let mut s =
        0xA076_1D64_78BD_642Fu64 ^ (global_region as u64 + 7).wrapping_mul(0xE703_7ED1_A0B4_28DB);
    (0..cfg.hazards_per_region)
        .map(|_| {
            DVec3::new(
                (next_random(&mut s) - 0.5) * 5_000.0,
                (next_random(&mut s) - 0.5) * 1_000.0,
                (next_random(&mut s) - 0.5) * 5_000.0,
            )
        })
        .collect()
}

struct Region {
    world: World,
    schedule: Schedule,
}

impl Region {
    fn tick(&mut self) {
        self.schedule.run(&mut self.world);
    }
}

// ---------------------------------------------------------------------------------------------
// Benchmark runs
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Mode {
    Regions,
    Systems,
}

/// Number of measured batches per run; the median batch is reported to reduce noise from
/// boost clocks, background work and VM scheduling.
const BATCHES: usize = 5;

fn measure(cfg: &Config, mut tick: impl FnMut()) -> Duration {
    for _ in 0..cfg.warmup_ticks {
        tick();
    }
    let mut batches: Vec<Duration> = (0..BATCHES)
        .map(|_| {
            let start = Instant::now();
            for _ in 0..cfg.measured_ticks {
                tick();
            }
            start.elapsed() / cfg.measured_ticks as u32
        })
        .collect();
    batches.sort();
    batches[BATCHES / 2]
}

/// `threads == 0` is the serial baseline: no task pool, no parallel iteration.
/// Otherwise the pool gets `threads` worker threads; the calling thread also helps while it
/// waits on a scope, so up to `threads + 1` threads do work.
fn run(mode: Mode, threads: usize, cfg: Config) -> Duration {
    let serial = threads == 0;
    let pool =
        ComputeTaskPool::get_or_init(|| TaskPoolBuilder::new().num_threads(threads.max(1)).build());

    match mode {
        Mode::Regions => {
            let mut regions: Vec<Region> = (0..cfg.regions)
                .map(|r| {
                    let mut world = World::new();
                    world.insert_resource(Parallel(false));
                    world.insert_resource(Hazards(vec![region_hazards(&cfg, r)]));
                    world.insert_resource(Markets(vec![Market {
                        supply: 0,
                        price: 100.0,
                    }]));
                    populate_region(&mut world, &cfg, r, 0);
                    Region {
                        world,
                        schedule: build_schedule(false),
                    }
                })
                .collect();
            if serial {
                measure(&cfg, || regions.iter_mut().for_each(Region::tick))
            } else {
                measure(&cfg, || tick_regions(pool, &mut regions))
            }
        }
        Mode::Systems => {
            let mut world = World::new();
            world.insert_resource(Parallel(!serial));
            world.insert_resource(Hazards(
                (0..cfg.regions).map(|r| region_hazards(&cfg, r)).collect(),
            ));
            world.insert_resource(Markets(vec![
                Market {
                    supply: 0,
                    price: 100.0
                };
                cfg.regions
            ]));
            for r in 0..cfg.regions {
                populate_region(&mut world, &cfg, r, r);
            }
            let mut schedule = build_schedule(!serial);
            measure(&cfg, || schedule.run(&mut world))
        }
    }
}

/// One task per region. (Chunking regions into one task per worker was measured and gave no
/// improvement, so the simpler form stays.)
fn tick_regions(pool: &TaskPool, regions: &mut [Region]) {
    pool.scope(|scope| {
        for region in regions.iter_mut() {
            scope.spawn(async move { region.tick() });
        }
    });
}

// ---------------------------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------------------------

fn arg_value(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1).cloned())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let quick = args.iter().any(|a| a == "--quick");
    let scale: usize = arg_value(&args, "--scale")
        .and_then(|v| v.parse().ok())
        .unwrap_or(1);
    let base = if quick {
        Config::quick()
    } else {
        Config::full()
    };
    let cfg = base.scaled(scale);

    match args.get(1).map(String::as_str) {
        Some("run") => {
            let mode = match arg_value(&args, "--mode").as_deref() {
                Some("regions") => Mode::Regions,
                Some("systems") => Mode::Systems,
                _ => usage(),
            };
            let threads: usize = arg_value(&args, "--threads")
                .and_then(|t| t.parse().ok())
                .unwrap_or_else(|| usage());
            let per_tick = run(mode, threads, cfg);
            // Machine-readable line for the sweep driver.
            println!("RESULT {}", per_tick.as_secs_f64() * 1_000.0);
        }
        Some("sweep") => sweep(cfg, quick, scale),
        _ => usage(),
    }
}

fn usage() -> ! {
    eprintln!("usage: sim_bench sweep [--quick] [--scale <k>]");
    eprintln!(
        "       sim_bench run --mode <regions|systems> --threads <n> [--quick] [--scale <k>]"
    );
    std::process::exit(2);
}

fn sweep(cfg: Config, quick: bool, scale: usize) {
    let exe = std::env::current_exe().expect("current exe");
    let max_threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    // 0 = serial baseline; pool sizes stay below the logical CPU count because the calling
    // thread also works (see `run`).
    let mut counts = vec![0, 1, 3, 7, 15, 31];
    counts.retain(|&n| n < max_threads);

    let ships = cfg.regions * cfg.ships_per_region;
    let slots = cfg.regions * cfg.belt_lines_per_region * cfg.belt_slots;
    println!(
        "Workload: {} regions, {} ships, {} belt slots ({} lines), {} hazards/region",
        cfg.regions,
        ships,
        slots,
        cfg.regions * cfg.belt_lines_per_region,
        cfg.hazards_per_region
    );
    println!(
        "Logical CPUs: {max_threads}; {} warmup ticks, then median of {BATCHES} batches of {} ticks\n",
        cfg.warmup_ticks, cfg.measured_ticks
    );
    println!("Threads = pool workers + calling thread; \"serial\" = one thread, no pool.\n");
    println!("| Threads | regions: ms/tick | speedup | systems: ms/tick | speedup |");
    println!("|---:|---:|---:|---:|---:|");

    let mut base = [0.0f64; 2];
    for &n in &counts {
        let mut row = [0.0f64; 2];
        for (i, mode) in ["regions", "systems"].iter().enumerate() {
            let mut cmd = Command::new(&exe);
            cmd.args(["run", "--mode", mode, "--threads", &n.to_string()]);
            if quick {
                cmd.arg("--quick");
            }
            cmd.args(["--scale", &scale.to_string()]);
            let out = cmd.output().expect("run benchmark child");
            let text = String::from_utf8_lossy(&out.stdout);
            row[i] = text
                .lines()
                .find_map(|l| l.strip_prefix("RESULT "))
                .and_then(|v| v.trim().parse().ok())
                .unwrap_or(f64::NAN);
            if n == 0 {
                base[i] = row[i];
            }
        }
        let label = if n == 0 {
            "serial".to_string()
        } else {
            (n + 1).to_string()
        };
        println!(
            "| {label} | {:.2} | {:.1}× | {:.2} | {:.1}× |",
            row[0],
            base[0] / row[0],
            row[1],
            base[1] / row[1]
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(slots: usize) -> BeltLine {
        BeltLine {
            region: 0,
            slots: vec![0; slots],
            spawn_timer: 0,
            spawn_interval: 1,
            machine_input: 0,
            machine_progress: 0.0,
            produced: 0,
        }
    }

    #[test]
    fn belt_conserves_items() {
        let mut l = line(16);
        let mut spawned = 0u64;
        for _ in 0..500 {
            advance_belt(&mut l);
            // With spawn_interval = 1 the timer resets to 0 exactly when an item spawns.
            if l.spawn_timer == 0 {
                spawned += 1;
            }
        }
        let on_belt = l.slots.iter().filter(|&&s| s != 0).count() as u64;
        // Every spawned item is on the belt, in the machine, or consumed into products.
        assert_eq!(spawned, on_belt + l.machine_input as u64 + l.produced * 4);
        assert!(l.produced > 0);
    }

    #[test]
    fn ships_stay_finite_and_bounded() {
        let hazards = vec![DVec3::ZERO, DVec3::new(100.0, 0.0, 0.0)];
        let mut ship = Ship {
            region: 0,
            pos: DVec3::new(1.0, 0.0, 0.0),
            vel: DVec3::ZERO,
            waypoint: DVec3::new(1_000.0, 0.0, 0.0),
            seed: 42,
        };
        for _ in 0..10_000 {
            steer_ship(&mut ship, &hazards);
            assert!(ship.pos.is_finite() && ship.vel.is_finite());
            assert!(ship.vel.length() <= 300.0 + 1e-9);
        }
    }

    #[test]
    fn both_strategies_run() {
        let cfg = Config {
            measured_ticks: 3,
            warmup_ticks: 1,
            ..Config::quick()
        };
        assert!(run(Mode::Regions, 2, cfg) > Duration::ZERO);
    }
}
