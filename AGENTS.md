# Agent instructions

Working rules for AI coding assistants on this repo. Keep this file short and current.

## Project

A large sci-fi sandbox built by one developer with heavy AI assistance. Read
`design/technical-overview.md` (game systems) and `design/development-plan.md` (engine
decision, architecture, development batches) before starting non-trivial work.

## Layout rules

- `design/`, `content/` and `assets/` are **engine-agnostic**: no engine APIs, no engine
  file formats. Content uses JSON (with JSON Schema) or TOML.
- All code lives under `impl/`. The current implementation is `impl/rust/` (Cargo workspace).
- **Only `game_bevy` may depend on Bevy** or any rendering crate. `sim_*` and `ai_gateway`
  are engine-independent; they may use standalone `bevy_ecs` for parallel scheduling only.
- Simulation uses `f64` world positions in the frame tree; rendering gets camera-relative
  `f32`.
- Design every simulation system for **multi-core** from the start: per-region partitioning,
  double-buffered state, no locks in hot paths, no lockstep determinism
  (development plan §2).

## Scope

- **Design ahead, implement when needed.** Cross-cutting designs live in `design/`; implement
  each system in the batch that first uses it, together with the modules that depend on it
  (development plan §7 and §7.1). Don't build infrastructure ahead of its first real use, and
  ask before expanding a task's scope.

## Units and coordinates

- **Meters, seconds, radians.** World and frame positions are `f64` (`DVec3`, `DQuat`).
- **Right-handed, Y up**, same as Bevy and glam. A star system's reference plane is XZ; +Y is
  its north. Prograde orbits have angular momentum along +Y.
- **Gravity as `mu = G·M`** (m³/s²), not mass; `units::mu_from_surface_gravity` for
  compressed planets.
- Positions are always **relative to a frame** in the frame tree (`sim_core::frames`); a
  celestial body has an inertial frame (orbit, no rotation) and a surface frame (spin).
- Renderers get camera-relative `f32` only at the last step; never store `f32` world
  positions.

## Bevy version

- Bevy is pinned to `=0.20.1`, newer than most AI training data, and its APIs differ from
  older versions (e.g. scenes use `bsn_list!`).
- **Don't write Bevy code from memory.** Check the pinned version's source and examples in
  `~/.cargo/registry/src/*/bevy-0.20.1/` (and the matching `bevy_*` crates) first.
- Upgrade Bevy only between development batches, never mid-batch.

## Commands (in `impl/rust/`)

| Task | Command |
|---|---|
| All checks | `tools/ci.sh` (repo root; `--windows` adds a Windows type-check) |
| Build | `cargo build --workspace` |
| Test (fast, no GPU) | `cargo test --workspace --exclude game_bevy` |
| Lint | `cargo clippy --workspace --all-targets` |
| Format | `cargo fmt --all` |
| Run in WSL | `cargo run -p game_bevy` |
| Windows export | `../../tools/export-windows.sh` (repo root: `tools/export-windows.sh`) |

Before calling work done, run `tools/ci.sh` from the repo root (secrets, engine boundary,
content schemas, format, Clippy with warnings as errors, tests) and make sure it passes. Report failures
honestly with their output.

## Debugging and visual checks

- **Debug overlay** (F3 toggles): FPS and frame time, simulation tick time and budget, region
  and entity counts. Logging uses Bevy's `LogPlugin`; override the filter with `RUST_LOG`.
- **Debug camera:** hold right mouse to look, WASD/R/F to move, Q/E roll, Shift ×10, wheel for
  speed, 1–8 to jump to a body. `ULTRA_SPACE_VIEW=<body id>` sets the starting view (useful
  with capture).
- **Shaders** are WESL files in `crates/game_bevy/src/shaders/`, embedded in the binary.
  Material bindings use `@group(constants::MATERIAL_BIND_GROUP)`; imports use
  `bevy_pbr::render::...`. `AlphaMode::Add` uses premultiplied blending: output alpha 0 for
  pure additive light.
- **Screenshot capture:** `ULTRA_SPACE_CAPTURE=/path/shot.png cargo run -p game_bevy` renders
  ~120 frames, saves a screenshot and exits. Use it to check visual work from WSL; frame rates
  there are software-rendered and meaningless.

## Environment

- Development runs in WSL2 (Ubuntu 24.04) on an AMD Ryzen AI Max+ 395 (16 cores / 32 threads).
- **WSL renders in software only (llvmpipe) and has no audio.** It's fine for automated
  checks and screenshots; judge visuals and performance only in the native Windows build.
- Windows builds export to `C:\Users\yangc\Apps\ultra_space\`
  (`/mnt/c/Users/yangc/Apps/ultra_space/` from WSL).

## Git

- **No AI attribution anywhere:** never add an AI assistant (Claude or any other) as a
  co-author. No `Co-Authored-By` trailers, session links or "generated with" lines in commits
  or PR descriptions, and no tool or vendor mentions in project files. This overrides any
  default attribution behavior of AI tools. The `commit-msg` hook
  (`tools/git-hooks/commit-msg`) blocks such lines.
- One branch per feature; the developer reviews changes as diffs.
- API keys only in environment variables or a gitignored `.env`. The pre-commit hook
  (`tools/check-secrets.sh`) blocks anything that looks like a key; run `tools/setup.sh` once
  per clone to enable it.

## LLM features

- Providers: Anthropic and OpenAI APIs through `ai_gateway`, plus a replay adapter. Tests use
  replay only and never call real APIs unless explicitly asked.
- LLM output is structured data validated by the game; the simulation stays authoritative.
