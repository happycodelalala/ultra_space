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

## Two tracks: game and design

Work runs on two tracks, each on its own branch and worktree (and its own sessions):

- **Game track** (main checkout, `~/code/ultra_space`): engine and gameplay, `impl/rust/`,
  system design docs, performance, Windows test rounds.
- **Design track** (`~/code/ultra_space-design`, branch `design/*`): vehicles, mechs, ships,
  capital ships, buildings and objects: `tools/blender/`, `assets/export/models/`, the design
  data in `content/data/parts/` and `content/data/vehicles/` (and their builder tests, e.g.
  `sim_vehicle/tests/bastion.rs`), vehicle design docs (`design/bastion.md`), the palette and
  the viewer (`impl/viewer/`). Review in the design viewer with screenshots
  (`tools/viewer-shots.sh`); **no game test rounds** while iterating. Plan and backlog:
  `design/design-track.md`.
- **The handoff** between them is fixed: the part and vehicle schemas, the model format
  (glb, metres, part axes, material slot names, COLOR_0 occlusion, TEXCOORD_0 metres), the
  palette slots and the joint/pose data. Changing any of it touches both tracks: agree it
  first and change both sides together.
- **Importing designs into the game:** the design branch merges at milestones, after
  `tools/ci.sh` passes (it checks models, schemas and the design tests: interference in every
  pose and move, balance, lift); the game branch takes it in by merging, and an occasional
  test round shows it in the game.
- The game keeps the last imported designs as placeholders meanwhile; don't edit design files
  on the game track (or engine code on the design track) except for the handoff.

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
- **rapier3d-f64 is pinned to `=0.36.1`** (simulation physics, Batch 3; it uses our glam 0.33).
  It's also newer than most training data: check `~/.cargo/registry/src/*/rapier3d-f64-0.36.1/`
  and `parry3d-f64-0.31.*` before writing rapier code.

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

- **Inspector** (F4 toggles; `ULTRA_SPACE_INSPECTOR=1` shows it at start): regions, frame tree,
  and a selected object's id, region, frame, position and velocity. `[` / `]` change selection.
- **Hitches:** a frame over 50 ms logs `hitch frame … ms: outside …, First …, PreUpdate …`
  (time per main schedule; *outside* is render extraction, waiting on the render thread and
  presenting), a render frame over 30 ms logs its stages (`slow render frame`), a frame adding
  50+ mesh entities says so, and `SlowSystem::new("name")` at the top of a system warns when
  it takes over 8 ms. For system-level detail build with a Chrome trace:
  `ULTRA_SPACE_FEATURES=trace tools/test-queue.sh run` (feature `trace` of game_bevy) writes
  `trace-<time>.json` next to the executable: about 20 GB for a two-minute run, so stream it
  for long spans rather than opening it, and delete it afterwards.
- **Log file:** every run writes `ultra_space.log` next to the executable (replaced each run);
  testers send that file. Hitches during startup are all logged with their time since launch.
- **Generation threads:** heavy background work (tiles, planet maps, sky) runs on the
  `workers()` pool in `game_bevy/src/workers.rs`, never on Bevy's `AsyncComputeTaskPool`, which
  Bevy needs for its own work (pipeline compilation).
- **Debug overlay** (F3 toggles, hidden at start): FPS and frame time, simulation tick time and budget, region
  and entity counts. Logging uses Bevy's `LogPlugin`; override the filter with `RUST_LOG`.
- **Ship controls** (flying is the default): see `impl/rust/README.md`. Tab switches to the
  debug camera.
- **Debug camera:** hold right mouse to look, WASD/R/F to move, Q/E roll, Shift ×10, wheel for
  speed, 1–8 to jump to a body. `ULTRA_SPACE_VIEW=<body id>` sets the starting view (useful
  with capture).
- **Shaders** are WESL files in `crates/game_bevy/src/shaders/`, embedded in the binary.
  Material bindings use `@group(constants::MATERIAL_BIND_GROUP)`; imports use
  `bevy_pbr::render::...`. `AlphaMode::Add` uses premultiplied blending: output alpha 0 for
  pure additive light.
- **Chase camera angle:** `ULTRA_SPACE_CHASE_ORBIT=<yaw>,<pitch>` (degrees; e.g. `0,-70` looks
  down at the ground below the ship) sets the starting chase-camera orbit, for captures.
- **Start altitude:** `ULTRA_SPACE_START_ALTITUDE=<m>` starts the ship in a circular orbit at
  that altitude (e.g. low over the terrain).
- **Planet size comparison:** `ULTRA_SPACE_BODY_RADIUS=<body id>:<km>` (e.g. `meridian:300`)
  changes one body's radius at load, keeping its surface gravity (task 2.13).
- **Start time of day:** `ULTRA_SPACE_START_SUN_ANGLE=<deg>` starts that far along the orbit
  from the point under the star (0 noon, ~89 sunset ahead, 120 night side, 180 midnight); the
  sun is ahead along the flight direction, so the default chase view looks towards it.
- **Test runs on the real GPU (preferred for visual and performance checks):**
  `tools/run-windows.sh [--export] <script.txt> [NAME=value ...]` launches the native Windows
  build from WSL with a test script (timed key presses, mouse moves, screenshots; format in
  `game_bevy/src/script.rs`, examples in `tools/test-scripts/`). A window opens on the
  developer's desktop (tell them before launching); it doesn't grab the mouse. The script logs a
  frame-time summary; screenshots land in `C:\Users\yangc\Apps\ultra_space\runs\<script>\`.
      **Test rounds:** `tools/run-tests.sh [--no-export] [--parallel N] [--visual | --perf] [name ...]`
  runs the test list `tools/test-scripts/tests.txt` (name, script, settings, kind, what to check):
  one build and export, visual tests several at a time (own window, folder and `game.log` each,
  via `ULTRA_SPACE_LOG`), performance tests alone afterwards, a `done` marker per finished test
  and a `summary.txt`. Visual rounds can run while building at low priority (`nice`); `--perf`
  rounds need a quiet machine.
  **Test queue (use this for rounds):** `tools/test-queue.sh add [options] [names]` queues the
  commit at HEAD (full id) and prints a request id; `tools/test-queue.sh run` (as a background
  command) works through requests first in, first out: it checks out exactly that commit in
  `.claude/worktrees/round` (refusing on a mismatch), builds it with the shared cache
  (`CARGO_TARGET_DIR=$HOME/.cache/ultra_space/test-target`), exports to
  `ULTRA_SPACE_EXPORT_DIR=/mnt/c/Users/yangc/Apps/ultra_space-test` (never the developer's
  build) and writes results to `runs/q<id>/` with a `manifest.txt` (id, commit, tests, times,
  status). `tools/test-queue.sh status` lists requests. Commit before queueing: uncommitted
  changes aren't tested.
  WSL itself can only render in software (OpenGL on Mesa's D3D12 driver can't present to a
  window or run the atmosphere shader), so in-WSL captures are a fallback.
- **Design viewer** (`impl/viewer`, three.js): `tools/viewer.sh` serves http://localhost:8765/
  (open it in a Windows browser) showing designs and part models from `content/` and
  `assets/export/models/` with joints, poses, collision boxes, centre of mass and live reload.
  `tools/viewer-shots.sh <design> [pose] [views]` (or `--part <id>`) saves review shots to
  `.viewer-shots/` with headless Chromium, no window: use it to check geometry before a game
  round. `FOCUS=<part name prefixes>` frames the shots on those parts (close-ups of details).
  The viewer writes only design JSON and `.glb` models in the game's folders and
  formats.
- **Part models:** `tools/build-models.sh [names...] [--preview DIR]` builds them in headless
  Blender (`~/.local/opt/blender-4.2.3-linux-x64`, or `$BLENDER`) from `tools/blender/*.py`
  into `assets/export/models/` (~8 min for all; `--preview` adds a studio render per model,
  `--view x,y,z` its direction).
  A full build writes `models.stamp`; CI fails when the scripts, parts or palette changed
  since. Materials are the slots in `content/data/part_materials.json`.
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
