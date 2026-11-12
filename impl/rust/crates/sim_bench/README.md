# sim_bench: parallel-scaling benchmark

Batch 0 check of the multi-core simulation architecture (development plan §2) before
anything is built on it.

## What it simulates

A synthetic galaxy slice with three systems:

- **Belt lines:** arrays of item slots feeding a crafting machine (belts are arrays, not one
  entity per item).
- **Ships:** `f64` positions; each ship steers toward waypoints and away from every hazard in
  its region.
- **Markets:** one per region, updated from factory output (a reduction after the belt
  system).

The same system code runs under two strategies:

| Strategy | How it runs |
|---|---|
| `regions` | One ECS `World` per star-system region; regions ticked in parallel on the compute task pool; systems run sequentially inside each region |
| `systems` | One shared `World`; multi-threaded executor plus `par_iter_mut` inside systems |

## Running

From `impl/rust/`:

```sh
cargo build --release -p sim_bench
./target/release/sim_bench sweep              # 64 regions, 64k ships, 410k belt slots
./target/release/sim_bench sweep --scale 8    # 8x content per region
./target/release/sim_bench sweep --quick      # tiny smoke run
```

The compute task pool size is fixed once per process, so `sweep` re-runs the binary once per
thread count. Each run does warmup ticks, then reports the **median of 5 batches**.
"Threads" counts pool workers plus the calling thread, which also does work while waiting.
"Serial" means one thread, no pool, no parallel iteration.

## Results: 2026-11-12, ROG Flow Z13 (Ryzen AI Max+ 395, 16C/32T), WSL2

Power mode was not controlled. Two runs agreed within ~10%; an earlier run (before
median-of-batches) reached ~10× on the heavy workload, so results depend on machine state.

**Normal workload** (64 regions, 64k ships, 410k belt slots):

| Threads | regions ms/tick | speedup | systems ms/tick | speedup |
|---:|---:|---:|---:|---:|
| serial | 4.9 | 1.0× | 4.8–4.9 | 1.0× |
| 4 | 1.6–1.9 | 2.6–3.0× | 2.1–2.4 | 2.0–2.4× |
| 8 | 1.2–1.4 | 3.5–4.1× | 1.6–1.7 | 3.0–3.1× |
| 32 | 1.2–1.5 | 3.2–4.1× | 1.4–1.6 | 3.1–3.3× |

**Heavy workload** (`--scale 8`: 512k ships, 3.3M belt slots):

| Threads | regions ms/tick | speedup | systems ms/tick | speedup |
|---:|---:|---:|---:|---:|
| serial | 40.0–40.5 | 1.0× | 39.3–39.8 | 1.0× |
| 4 | 12.8–13.2 | 3.0–3.2× | 16.0–16.3 | 2.4–2.5× |
| 8 | 8.8–9.5 | 4.3–4.6× | 10.8–10.9 | 3.6–3.7× |
| 16 | 8.7 | 4.6–4.7× | 9.4–10.4 | 3.8–4.2× |
| 32 | 6.8–6.9 | 5.8–6.0× | 7.8–7.9 | 5.0× |

## Findings

1. **The architecture scales:** 5–6× on heavy ticks (up to ~10× in one earlier run),
   3–4× on light ticks, with no locks and no data races (compiler-checked).
2. **Per-region worlds (`regions`) scale better** than one shared world, and they map directly
   onto the frame tree. They become the primary strategy. `par_iter_mut` stays available for
   hot systems inside one very large region (e.g. a huge battle).
3. **Light ticks hit a floor of ~1 ms**, a fixed per-tick cost (scheduling and thread
   wake-ups, likely higher inside the WSL2 VM). That's acceptable against a 33 ms budget at
   30 Hz; real game ticks carry more work per tick, where scaling is better.
4. **Chunking regions into one task per worker** (fewer spawns) gave no improvement, so the
   simpler one-task-per-region form stays.
5. **8 → 16 threads gains little.** Hypotheses, not yet tested: all-core clock drops under the
   tablet's power limit, and crossing between the chip's two 8-core clusters (CCDs).

## Follow-ups

- Run the Windows build of `sim_bench` natively to separate WSL2 overhead from hardware limits.
- Re-run in a fixed power mode (Performance, plugged in) and record it with the results.
- Re-run as real simulation systems replace the synthetic ones.
