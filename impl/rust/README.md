# Rust implementation

Cargo workspace: engine-independent simulation crates plus a Bevy front end.

## Crates

| Crate | Role | May depend on |
|---|---|---|
| `sim_core` | Frame tree, orbits, clocks, star systems, objects and regions (one per body) with transfers, parallel region ticking | std, `bevy_ecs`, `bevy_tasks` |
| `sim_planet` | Terrain generation, player-made deltas | `sim_core` |
| `sim_vehicle` | Part catalog and vehicle designs, mass properties, thrust allocation, performance summary; flight next | `sim_core`, `bevy_ecs` |
| `sim_economy` | Factories, factions, tiered simulation | `sim_core` |
| `sim_combat` | Damage events, morale, combat AI | `sim_core` |
| `ai_gateway` | LLM providers (Anthropic, OpenAI, replay), caching, validation | `sim_core` |
| `game_bevy` | Bevy app: rendering, input, UI, audio (binary: `ultra_space`) | everything above + `bevy` |
| `sim_bench` | Parallel-scaling benchmark (see its README) | `bevy_ecs`, `bevy_tasks`, `glam` |

**Boundary rule:** only `game_bevy` may depend on `bevy` or any rendering crate. The `sim_*`
crates may use standalone `bevy_ecs` for parallel scheduling, which works with any renderer.

## Versions

- Rust toolchain pinned in `rust-toolchain.toml`.
- Bevy pinned exactly in the workspace `Cargo.toml` (`=0.20.1`). Upgrade deliberately
  between batches, never mid-batch.

## Setup (WSL2 Ubuntu 24.04)

1. Rust: `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
2. System packages (C toolchain, fast linker, Bevy's Linux dependencies, Windows cross-compiler):

   ```sh
   sudo apt install build-essential pkg-config clang lld \
       libasound2-dev libudev-dev libwayland-dev libxkbcommon-dev \
       mingw-w64
   ```

3. Git hooks (secret check): run `tools/setup.sh` from the repo root.

## Commands (run in `impl/rust/`)

| Task | Command |
|---|---|
| **All checks (run before merging)** | `../../tools/ci.sh` (add `--windows` for a Windows type-check) |
| Build everything | `cargo build` |
| Test sim crates (no GPU needed) | `cargo test --workspace --exclude game_bevy` |
| Test everything | `cargo test --workspace` |
| Lint | `cargo clippy --workspace --all-targets` |
| Format | `cargo fmt --all` |
| Run the game (WSL, via WSLg) | `cargo run -p game_bevy` |
| Save a screenshot and exit | `ULTRA_SPACE_CAPTURE=/path/shot.png cargo run -p game_bevy` |
| More/less logging | `RUST_LOG=debug cargo run -p game_bevy` |
| Export a Windows build | `../../tools/export-windows.sh` |

Windows builds are exported to `C:\Users\yangc\Apps\ultra_space\`
(`/mnt/c/Users/yangc/Apps/ultra_space/` from WSL). Judge visuals and performance there, not
in WSL.

## Content

The game loads `content/data/` at runtime. It looks for `ULTRA_SPACE_CONTENT`, then a
`content/` folder next to the executable or in any parent folder (the repo checkout). The
Windows export copies `content/data` next to the `.exe`.

## In-game debug tools

- **Flying the Kestrel** (default): W/S forward/back, A/D strafe, Space/Ctrl up/down, mouse to
    pitch/yaw (cursor locked), Q/E roll, T cycles flight assist (Full / Rotation / Off), X brakes,
    **N** arcade flight (debug; No Man's Sky style: no gravity, the ship flies where the nose
  points, W/S raise or lower the held speed up to 4 km/s, X stops),
  **Z** full stop (debug): instantly zero speed relative to the ground when low (hover) or to the
  planet when in orbit (drop straight down),
  **J** engages the cruise drive (3 s charge; steer with the mouse; J again or approaching a body
  drops out into orbit),
  V toggles chase/cockpit view, right mouse orbits the chase camera, Tab switches to the free
  camera. Thrust is disabled above ×4 time warp, where coasting ships go on rails.
  **Esc** releases the mouse (click in the window to capture it again); switching windows
  releases it too. **1–8** teleport the ship into a parking orbit around a body (debug, until
  the cruise drive).
- **Debug camera** (after Tab): hold right mouse to look; WASD move, R/F up/down, Q/E roll, Shift ×10,
  mouse wheel changes speed, **1–8** jump to Halcyon, Cinder, Meridian, Lantern, Hoarfell,
  Leviathan, Forge, Rime. `ULTRA_SPACE_VIEW=<body id>` picks the starting view.
- **, / .** change time warp.
- **HUD** (H toggles): reference body and frame, speed, vertical speed, altitude, apoapsis,
  periapsis and period, assist and cruise status (bottom); Galactic Standard Time, local solar
  time under the ship and time warp (top right). **O** cycles orbit lines: the ship's predicted
  path, plus the other bodies' orbits, or none.
- **F4** toggles the inspector (hidden at start; `ULTRA_SPACE_INSPECTOR=1` shows it): regions
  in use, the frame tree, and the selected object's `ObjectId`, region, frame, position and
  velocity (also in the star's frame), rails, cruise and assist state. `[` / `]` select objects.
- **F3** toggles the debug overlay (hidden at start): FPS and frame time, simulation tick time and budget use,
  regions, entities, total and dropped ticks.
- **Planet maps:** `cargo run --release -p sim_planet --example planet_map -- <body id>
  <out.ppm> [width]` renders a body's biome map with hill shading (equirectangular), for
  tuning terrain and climate parameters.
- **Loading:** the star system, starting ship and simulation are built on a worker thread
  while the window shows "Loading"; game systems run once loading finishes. Frames over the
  50 ms hitch budget are logged as warnings, and a "startup frames" summary is logged after
  the first 10 s of play (meaningful only in the native Windows build).
- The simulation (`sim_core::Simulation`) runs at a fixed 30 Hz, decoupled from the frame
  rate. It runs at most 4 ticks per frame; time beyond that is dropped and shown as "dropped".
- An over-budget tick logs a warning (at most every 5 s).
