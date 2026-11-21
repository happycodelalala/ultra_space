# Ultra Space: Development Plan

This document covers how the game will actually be built: the engine decision, the
simulation architecture, the development environment, and the order of work. The design
itself is in [technical-overview.md](technical-overview.md).

**Context:** one developer, working with heavy AI coding assistance, on a very large
simulation-heavy game.

---

## 1. Engine decision: Bevy + engine-independent Rust simulation

### 1.1 Decision

- **Engine:** Bevy (Rust).
- **Simulation:** separate Rust crates with **no rendering dependencies**. They are tested
  and benchmarked headless, and could be moved to another renderer if ever needed.
- **Not** a from-scratch engine, and not Shadertoy-only rendering (see 1.4 and 4).

### 1.2 Why: multi-core simulation is the core requirement

Large-scale simulation games usually hit a CPU wall because their simulation can't use
more than a core or two:

| Game | Bottleneck |
|---|---|
| Supreme Commander | Deterministic multiplayer simulation on one thread; late-game "sim speed" drops below real time |
| Kerbal Space Program | Part-joint physics on one thread per vessel; high part counts slow everything |
| X4 | Large battles, economy and AI scripts are heavily main-thread; big fleet fights collapse frame rate |
| Factorio / Dyson Sphere Program | Factorio's update loop stays largely single-threaded (determinism) and survives through extreme optimization; DSP gained a lot by adding multithreading later |

**Lesson:** adding parallelism after the fact is extremely hard. It must be in the
architecture from day one. Rust and Bevy support this best:

- **The Rust compiler rejects data races.** In C#, a race compiles fine and shows up later as
  a rare, hard-to-reproduce bug. This matters a lot when much of the parallel code is
  AI-written.
- **No garbage collector**, so no frame-time spikes from many threads allocating memory.
- **Built-in parallel scheduling:** Bevy's ECS runs non-conflicting systems in parallel
  automatically. `bevy_ecs` can also be used on its own.
- **Everything is code**, the best fit for AI-assisted development: text files, compiler
  checks, headless tests.

### 1.3 Options considered

| | Godot 4 + C# | Bevy (Rust) | Custom engine |
|---|---|---|---|
| Time to first playable | Fast | Medium | Very slow |
| Fit with AI-written code | Good | **Best** | Good |
| Editor for visual work | **Yes** | No (custom tools) | No |
| Animation, UI, audio, export | **Built in** | Partly | Build it all |
| Large-scale parallel simulation | Possible, manual, garbage collector risk | **Native ECS, compiler-checked** | Whatever you build |
| API stability | **Stable** | Frequent breaking changes | Yours |
| Risk for a solo developer | Lowest | Medium | Highest |

Runtime speed comparison (Godot + C# vs. Bevy):

| Area | Verdict |
|---|---|
| GPU work (rendering, raymarching, compute shaders) | About equal; shaders run at the same speed. Godot's renderer is more mature |
| Simulation code in tight loops | Rust typically ~1.2–2× faster; careful C# comes close |
| Garbage collection hitches | None in Rust; avoidable in C# only with discipline |
| Multithreading | Bevy: automatic. C#: manual |
| Game code ↔ engine calls | Bevy: no boundary. Godot: each C# → engine call crosses a native boundary |
| Physics | Godot ships Jolt; Bevy uses Avian or Rapier. Matters less because vehicles are single rigid bodies |
| Compile times | C# faster; Rust slower (dynamic linking helps) |

Architecture choices (tiered simulation, LOD, instancing, merged rigid bodies, GPU crowds)
have 10–100× effects, more than the language difference. The deciding factor is
**safe, easy parallelism at scale**.

**Rejected:**

- **Custom engine from scratch:** one to three years of solo work rebuilding solved problems
  (renderer, animation, physics, audio, UI, assets, export) before the game itself starts.
  The hard parts of this game live in the simulation, which works in any engine.
- **Godot + C#:** a strong option with an editor, but parallel simulation is manual and
  harder to make safe.
- **Rust simulation + Godot (via godot-rust/gdext):** keeps Godot's editor, but means two
  languages and a boundary between them.

### 1.4 Costs and mitigations

| Cost | Mitigation |
|---|---|
| No production editor | In-game debug and inspector tools (egui-based); Blender for assets (glTF); data files for content; in-game builder tools the game needs anyway |
| Less mature 3D animation, UI and asset tooling | Asset packs, Mixamo animations, vertex animation textures for crowds, egui for tools |
| Frequent Bevy API changes | Pin one Bevy version; upgrade deliberately between batches, never mid-batch; check docs and source for the pinned version |
| Slower Rust compiles | Dynamic linking in dev builds, a fast linker, a small crate per subsystem |
| Extra tooling work | Batches 2–5 estimated **~20–30% longer** than with Godot |

---

## 2. Parallel simulation architecture

Applies to every simulation crate.

1. **Spatial partitioning through the frame tree.** Each star system, planet and battle
   region simulates on its own core. Regions exchange messages only at tick boundaries.
2. **ECS with declared reads and writes.** Systems that touch different data run in
   parallel automatically.
3. **Double-buffered state.** Systems read last tick's state and write the next. No locks
   in hot paths.
4. **Simulation separate from rendering.** The simulation runs at a fixed tick on worker
   threads; rendering interpolates between ticks.
5. **Background tiers on staggered schedules.** Distant economies and factions tick at lower
   rates, spread across frames.
6. **Separate physics worlds per region**, each running on its own threads.
7. **Factory networks as independent units** (per belt or power network), processed in
   parallel.
8. **No lockstep determinism.** Single-player first; any future multiplayer uses a
   server-authoritative model, avoiding the single-thread trap.
9. **Long-running work is asynchronous:** pathfinding, terrain generation and LLM calls run
   as background tasks spread across frames.

**Target hardware:** 16 cores / 32 threads (development machine). A parallel design should get
up to ~10× more simulation per frame than a single-threaded one.

**Validation (Batch 0, 2026-11-12):** a synthetic benchmark (factory belts, 64k–512k ships,
per-region markets) scaled **15.4× on heavy ticks and 8.7× on light ticks at 32
threads natively on Windows** on the 16-core development machine (WSL2 understates scaling:
3–6×). **One world per region, ticked in parallel** scaled
better than one shared world, and is the primary strategy. Parallel iteration inside systems
is reserved for very large single regions. Light ticks have a ~0.5 ms fixed per-tick cost natively.
Details and raw numbers: `impl/rust/crates/sim_bench/README.md`.

---

## 3. Repository layout

Design and ideas are kept separate from the implementation, so the engine or framework can
change without touching design, content or assets.

```
ultra_space/
├─ design/            game design, ideas and plans (engine-agnostic, no code)
├─ content/           game data in engine-neutral formats (JSON/TOML)
│   ├─ schemas/       JSON Schemas for all data types
│   ├─ data/          parts, buildings, items, tech tiers, culture kits
│   ├─ prompts/       LLM prompt templates
│   └─ lore/          world bible
├─ assets/
│   ├─ source/        Blender files, layered textures, raw audio
│   └─ export/        glTF/GLB, PNG/KTX2, OGG loaded by the game
├─ impl/              implementations (all engine- or language-specific code)
│   └─ rust/          Cargo workspace
│       └─ crates/
│           ├─ sim_core/      frame tree, f64 math, orbits, tick scheduler
│           ├─ sim_planet/    terrain generation, deltas
│           ├─ sim_vehicle/   part graphs, mass properties, control allocation
│           ├─ sim_economy/   factories, factions, tiered simulation
│           ├─ sim_combat/    damage events, morale, AI
│           ├─ ai_gateway/    LLM providers (Anthropic, OpenAI, replay), caching, validation
│           └─ game_bevy/     Bevy app: rendering, input, UI, audio
└─ tools/             repo-wide scripts: git hooks, secret check, Windows export
```

- `design/`, `content/` and `assets/` never reference engine APIs or engine file formats.
- Only `game_bevy` depends on Bevy. `sim_*` crates may use standalone `bevy_ecs` for parallel
  scheduling, but nothing from rendering.
- **Changing engines:** a new front-end crate next to `game_bevy` (same language, e.g.
  Godot via godot-rust), or a new `impl/<name>/` (different language). Design, content and
  assets stay where they are.
- Each crate has unit tests; performance-critical crates also have benchmarks.

---

## 4. Rendering approach: engine meshes plus Shadertoy techniques

Shadertoy-style rendering (raymarching signed distance fields in one shader) cannot replace
an engine. It has no physics, audio, input, UI, animation or asset support, is poor for
animated characters, and its cost grows with scene complexity. It is excellent for specific
parts of this game, so it's used **inside** the engine:

| Raymarched / procedural shaders | Engine meshes |
|---|---|
| Distant planets (raymarched stand-ins) | Terrain near the player (quadtree tiles) |
| Atmosphere, clouds, sky, sunsets | Characters, creatures, horses, mechs |
| Nebulae, star fields, gas giants | Player-built vehicles and buildings |
| Explosions, shields, beams, warp effects | Settlements, ship interiors, factories |
| Procedural materials and textures | Asset-pack content |

**One noise function, used three ways:**

1. A raymarching shader draws the planet from orbit.
2. A compute shader builds close-up terrain tiles.
3. Simulation code evaluates the same function for collision, landing spots and resource
   placement.

The planet then looks the same from orbit and from the ground, and gameplay matches what
the player sees.

**Shader verification loop:** the AI assistant writes a shader, renders a test scene and
inspects the screenshot. The developer reviews the final look in a native Windows build.

Reference projects for engines built on unusual rendering: *Dreams* (SDF), *Claybook* (SDF),
*Teardown* (voxel raymarching). Each needed expert graphics programmers or a large
specialist team.

---

## 5. Development environment

### 5.1 Hardware

- **Machine:** ASUS ROG Flow Z13, AMD Ryzen AI Max+ 395 (16 cores / 32 threads), Radeon 8060S
  integrated GPU, 64 GB unified memory.
- **The GPU is roughly a mid-range laptop graphics card.** It is the "recommended spec"
  baseline. A low-end test profile (lower resolution, fewer terrain tiles, smaller battles)
  is added later.
- **Benchmark in a consistent power mode** (e.g. Performance, plugged in). Tablet performance
  varies a lot with the power profile.

### 5.2 WSL / Windows split

```
WSL2 (Ubuntu 24.04)                        Windows
├─ repo (~/code/ultra_space), git          ├─ exported .exe builds
├─ AI-assisted development                 │   └─ play-testing, visuals and
├─ cargo build / test / bench              │      performance (native GPU)
├─ headless simulation tests & benchmarks  └─ Blender, asset tools
├─ debug screenshots (test scenes)
└─ cross-compile → Windows build ────────► C:\Users\yangc\Apps\ultra_space\
```

- **The repo stays on the Linux filesystem.** File access across the Windows/Linux boundary
  is slow.
- **GPU inside WSL** goes through a generic Direct3D 12 translation layer. It's fine for
  automated runs and screenshots, not for judging visuals or performance.
- **Windows builds:** cross-compile with `cargo build --target x86_64-pc-windows-gnu` and export
  to `C:\Users\yangc\Apps\ultra_space\` (`/mnt/c/Users/yangc/Apps/ultra_space/` from WSL); run the
  `.exe` natively.
- `.gitattributes` with LF line endings; build outputs gitignored.
- Repo-wide scripts in `tools/` (secret check, Windows export); Rust commands in
  `impl/rust/README.md`.

---

## 6. LLM integration: cloud APIs

LLM features use the **OpenAI and Anthropic APIs**. No local models.

### 6.1 Provider layer

```
Game systems ──► AI Gateway ──► Provider interface
                  (context, schema       ├─ Anthropic adapter
                   validation, cache,    ├─ OpenAI adapter
                   budget, fallback)     └─ Replay adapter (tests: recorded responses)
```

- **One internal request format** (task type, context, output schema), translated by each
  adapter, including that provider's structured-output features.
- **Model choice per task type, set in config:** a stronger model for build-time lore, a fast,
  cheap one for live dialogue. Providers and models can be switched without code changes.
- **Replay adapter:** tests run against recorded responses. They are free, fast, repeatable and
  offline, and call the real API only when explicitly requested.

### 6.2 API keys

- **Development:** keys in environment variables or a gitignored `.env` file, never in the
  repo. A check blocks committing anything that looks like a key.
- **Shipping: never put an API key in the game build.** Options (decide later):
  - a small hosted backend proxy that holds the keys and enforces per-player rate limits and
    spending caps
  - "bring your own key," where players enter their own API key
  - both

### 6.3 Cost control

- Cache every response; identical requests reuse saved results.
- Batch APIs for bulk content generation.
- Prompt caching for the stable world-bible prefix.
- A spending budget per session, with token and cost logs per task type.
- Cheap models while iterating on prompts; stronger ones for final content runs.

### 6.4 Offline fallback

Templates and procedural content remain the offline mode. The game is fully playable
without network access or keys.

---

## 7. Development batches

Order: technical risk first, then a playable loop early (for motivation and feedback), then
breadth. Estimates are rough sizes for one person working steadily with heavy AI help. They
include the Bevy tooling overhead and are relative sizes, not commitments.

| # | Batch | Deliverable | Est. |
|---|---|---|---|
| **0** | **Foundation** | Cargo workspace and crates; tests, CI, benchmark harness; **parallel-scaling benchmark** (100k+ factory items and many ships across several star-system regions, measured at 1/4/8/16 threads); debug overlay; logging; `AGENTS.md` (AI assistant rules); key-leak check; Windows cross-compile. *(The in-game inspector and save-format versioning moved to the batches that first need them; see 7.1.)* | 3–5 wk |
| **1** | **Space-scale core** | Frame tree, floating origin, orbits on rails, the hand-made Halcyon system with raymarched distant planets, star field and nebula skybox, universe and game clocks, fly a simple ship between planets with a placeholder cruise drive (see 7.2) | 1.5–2.5 mo |
| **2** | **Seamless planet** | Cube-sphere LOD terrain, biomes, atmosphere and clouds matched to terrain, land from orbit, walk on foot, gravity transitions | 2.5–4 mo |
| **3** | **Vehicle builder** | Part graph, merged rigid body, thruster/wheel control allocation, builder UI, a rover and a ship you built, save/load | 2.5–4 mo |
| **4** | **Living planet** *(brought forward 2026-11-14)* | Two cultures at different tech levels on Meridian (a medieval culture and a spacefaring colony), factions, settlements generated from climate and terrain, aggregate population simulation, settlements on tangent grids from building kits, people with daily schedules near the player; lore through the content director with recorded responses ([living-planet.md](living-planet.md)) | 2–3 mo |
| **4b** | **Density and the peoples of Meridian** *(2026-11-14)* | Meridian as the test bed for every stage: seven peoples from Stone Age to a ruined Modern nation, settlements from each people's settlement pattern and the land's area, the settlement hierarchy, a wasteland from generated history, layouts on approach, distant forms batched, CC0 stand-in kits and characters for every tier, roads and dwellings ([living-planet.md](living-planet.md) §8b) | 6–9 wk |
| **5** | **The world, refined** *(2026-11-15)* | The block world ([block-world.md](block-world.md)) in realistic light ([visual-style.md](visual-style.md)), proven on a **demo village** on Meridian where the player starts: the block core and model format, the ground as blocks near the player, the village built from block models, light and weather matching the style renders, digging and placing; water, planet-wide ground cover and effects after the village is reviewed | 2.5–3.5 mo |
| **6** | **First playable loop** *(2026-11-15)* | Game shell (menus, save slots, pause, settings), the player's record (money, standing, journal), discoveries, contracts from settlements, basic audio, first-time guidance | 1.5–2.5 mo |
| **7** | **Survival-builder loop** | Resource gathering (the ground cover's trees and rocks), inventory, crafting, base building on tangent grids, production chains (parallel factory networks), fuel and power consumption, terrain deltas, simple creatures | 2.5–4 mo |
| | ⭐ **Checkpoint A** | **A playable sandbox:** explore, build, fly. Candidate for a public demo or early access | |
| **8** | **Combat core** | Unified damage pipeline, first-person weapons, per-part vehicle damage, hostile creatures and bandits | 1–2 mo |
| **9** | **Living galaxy** | Multiple star systems simulated in parallel, tiered simulation, economy, factions, trade ships, galaxy map | 2–4 mo |
| **10** | **LLM gateway + first content** | AI gateway with Anthropic/OpenAI/replay adapters, caching, validation, budgets, fallbacks; generated culture profiles; galaxy and system history; codex text | 1–2 mo |
| **11** | **Cultures and people** | Settlements from building kits, NPC schedules, notable characters with LLM dialogue, director-generated quests | 3–4 mo |
| **12** | **Medieval warfare** | Directional melee, mounts, formation AI, 200+ soldier battles (GPU crowd animation), campaign-map battle promotion, auto-resolve | 3–5 mo |
| | ⭐ **Checkpoint B** | **A full game loop:** explore, build, trade, talk, fight wars | |
| **13** | **Mixed-tech and orbital** | Morale and doctrine AI, supply limits, orbital strikes, layered battles | 2–3 mo |
| **14** | **Capital ships** | Ship interiors, hull walking, bridge stations, EVA, boarding, hangars | 3–4 mo |
| **15** | **Breadth and polish** | Mechs, airships, sieges, diplomacy, more tech tiers, performance, modding, release prep | ongoing |

**Honest total:** roughly 3–4+ years to Checkpoint B and 4–5+ years to the full vision, part
of it in early access. Checkpoint A should come within roughly the first year and a half
(2026-11-15: the refined world and the first playable loop, Batches 5 and 6, now come before
the survival loop: about 4–6 months more before Checkpoint A).

Until Batch 10, earlier systems use a simple placeholder interface for generated text
(names, codex entries), so they plug into the AI gateway later without changes.

### 7.2 Batch 1 tasks

Decisions behind these tasks: [universe-scale-time-and-flight.md](universe-scale-time-and-flight.md).

| # | Task | Done when |
|---|---|---|
| 1.1 | **Units and conventions:** meters, seconds, `f64` positions, right-handed Y-up | Documented; math helpers tested |
| 1.2 | **Frame tree:** frames with parents; relative position and rotation between any two frames in `f64` | Correct relative transforms; under 1 mm error at 10¹² m |
| 1.3 | **Orbits on rails:** Kepler orbits at time *t*, planet spin and axial tilt | Periods, energy conservation and reference values tested |
| 1.4 | **Clocks:** integer universe tick counter; Galactic Standard Time (16×); local planet time; debug time warp | Clock conversions tested; orbits visibly advance under warp |
| 1.5 | **Star system data:** engine-neutral JSON schema and the hand-made Halcyon system with a light, KSP-like star (self-consistent spheres of influence) | Halcyon loads into the frame tree; invalid data gives clear errors |
| 1.6 | **Floating origin rendering:** camera = frame + `f64` offset; camera-relative `f32` transforms for Bevy | No visible jitter 10⁹ m from the star |
| 1.7 | **Distant planets via shaders:** raymarched spheres with lighting and atmosphere rims; placeholder meshes up close | Planets look right from 1,000 km to the whole system; no depth glitches |
| 1.8 | **Star and lighting:** glare, light from the star's real direction, HDR and bloom | Correct day and night sides on every body |
| 1.9 | **Sky:** procedural star field and nebula, fixed to the galaxy | Stable while flying |
| 1.10 | **Objects and vehicles core:** `ObjectId`, frame-relative state, object registry, one region per body; part catalog and vehicle designs (data + schemas), vehicle instances, mass properties, thrust allocation ([objects-vehicles-physics.md](objects-vehicles-physics.md) §1–3) | Invariant tests in §7 of that doc |
| 1.11 | **Ship flight:** symplectic `f64` rigid-body integration, gravity from the region's body, on-rails coasting under time warp, flight assist modes with automatic "standing still" reference, sphere collision with planets | Orbit-stability and on-rails tests; assist holds speed and hovers correctly |
| 1.12 | **Moving between regions:** transfers at tick boundaries with hysteresis | Position and velocity preserved; no flicker; no visible jump |
| 1.13 | **Placeholder cruise drive:** speed scaled by distance to the nearest body, auto-slowdown, drop to normal flight | Neighbouring planets reached in 1–3 minutes |
| 1.14 | **Controls and camera:** keyboard and mouse, chase camera, assist toggle | Developer judges the feel on Windows |
| 1.15 | **HUD:** speed, reference body, altitude, galactic and local time, time warp; orbit lines | Readable at a glance |
| 1.16 | **Inspector:** frames, regions, a selected object's `ObjectId`, frame and position | Usable for debugging 1.10–1.13 |
| 1.17 | **Background loading:** Halcyon is built on a worker thread at startup | No startup frame over the 50 ms hitch budget |
| 1.18 | **Batch review:** fly Meridian → Lantern → Leviathan on Windows; check jitter, transitions, frame times | Exit criteria met |

**Exit criteria:** flight between planets with no jitter and no frame over 50 ms; orbit physics
tests pass; sphere-of-influence transitions without visible jumps; CI passes.

**Not in Batch 1:** planet surfaces, climate computation and landing (Batch 2); building
vehicles (Batch 3).

**Status:** done. The 1.18 review on Windows (2026-11-12) passed: flight, cruise, transitions,
time warp and steering feel good; no hitches in play. Startup still has two ~300 ms frames in
the first second after launch (likely first-use shader compilation); pipeline warm-up moved to
task 2.12.

### 7.3 Batch 2 tasks

Decisions behind these tasks: [planet-surface.md](planet-surface.md).

| # | Task | Done when |
|---|---|---|
| 2.1 | **Terrain function:** deterministic `f64` height, climate and biome fields in `sim_planet`; `terrain` parameters in the star system data | Determinism and range tests; ocean fraction matches the data |
| 2.2 | **Cube-sphere quadtree:** tile keys, split and merge by screen-space error, tile meshes with skirts | Tests for keys, neighbours and crack-free edges |
| 2.3 | **Tile streaming:** generation on worker threads, per-frame upload budget, parents kept until children are ready | No holes; no frame over 50 ms in a descent |
| 2.4 | **Orbit view from the same function:** baked cube maps for the distant planet shader; seamless handover to tiles | No visible switch from orbit to low altitude |
| 2.5 | **Terrain look:** biome colors, slope rock, snow, detail noise; water | Readable biomes from orbit and on the ground |
| 2.6 | **Atmosphere from the ground:** sky color, sunsets, aerial haze | Matches the orbit view |
| 2.7 | **Clouds** following the moisture field | Cloud belts and clear deserts |
| 2.8 | **Landing:** ground contact on terrain, drag, landed vehicles in the surface frame | Land on a slope and stay put; tests |
| 2.9 | **Walking on foot:** character controller, first-person camera, exit and enter the ship, gravity-aligned up | Walk from the ship over hills and back |
| 2.10 | **Gravity transitions:** camera and controls follow local up from orbit to ground | No snap during descent |
| 2.11 | **Terrain deltas:** keyed by tile; a flattened landing pad as the first use | Delta applies identically on regeneration |
| 2.12 | **Hitch stress test:** scripted orbit → surface descent with frame-time report | Report on Windows |
| 2.13 | **Planet size comparison:** 300 / 600 / 1,000 km on foot (universe design §1) | Tool done (`ULTRA_SPACE_BODY_RADIUS`); decision **deferred** to when there is content to judge by (universe design §1) |
| 2.14 | **Batch review** | Exit criteria met |

**Exit criteria:** land anywhere on Meridian from orbit and walk around with no loading screen
and no frame over 50 ms; terrain matches from orbit to the ground; CI passes.

**Status:** done on branch `batch2/terrain`. The 2.14 review (2026-11-12, scripted test
rounds on Windows) passed:
- Descent from orbit to the ground: 0 frames over 50 ms in 96 s (worst 48 ms), no loading screen
  after the start-up warm-up.
- Landing (stays put), arcade flight near the ground, horizon levelling, walking out, running,
  jumping and boarding, the flattened pad and a sunset over the sea all look right.
- CI passes.

2.13's tool (`ULTRA_SPACE_BODY_RADIUS`) is done; the size decision is deferred (universe design
§1). Known polish for later:
- distant mountains look angular, with hard color bands
- staircase coastlines
- arcade flight near the ground at 300 m/s runs at ~46 fps
- no slope limits or swimming on foot

Debug aids added along the way: full stop (Z), arcade flight (N), flattened pad (K), start
position, time of day and camera angle by environment variables, scripted test rounds on Windows
(`AGENTS.md`).

### 7.4 Batch 3 tasks

Decisions behind these tasks: [vehicle-builder.md](vehicle-builder.md).

| # | Task | Done when |
|---|---|---|
| 3.1 | **Rapier core:** rapier3d-f64 in the simulation crates, one world per region, per-body gravity, the Kestrel as one rigid body with a compound collider flown by the existing assist and allocator | Orbit-stability and region-transfer tests pass; flight feels unchanged |
| 3.2 | **Terrain collision:** collision patches from the terrain function on a body turning with the planet; rapier contacts replace the Batch 2 ground contact (kept as a fallback while patches build) | Landing test (stays put on a slope) passes; landing, pad and descent tests pass on Windows |
| 3.3 | **Data format additions:** attach nodes, attachment graph, connectors, sections, slots, form, interior placeholder, collider, new part kinds (landing legs, wheels, batteries); design hash; schemas; Kestrel converted, with landing legs | Schema checks; graph and hash tests |
| 3.4 | **Design compilation:** compiled form (mass, allocation, compound collider, wheel set, merged meshes per material) on workers, cached by hash | No hitch when a design changes; 300-part benchmark ≈ the Kestrel per tick |
| 3.5 | **Wheels and a rover:** wheel parts, raycast vehicle controller, a rover design, drive controls, boarding any vehicle | Drive over hills and back without tipping or sinking |
| 3.6 | **Walker on the character controller:** collides with and stands on vehicles; slope limit and steps | Walk around and onto the rover; tests |
| 3.7 | **Builder mode:** enter and leave (B), orbit camera, parts palette, snap to attach nodes, place, rotate, select, remove, undo | Build a small lander from parts and fly it |
| 3.8 | **Builder tools:** symmetry, sections, stats (mass, centre of mass, thrust-to-weight, hover/strafe/turn, rover stability, Δv estimate), validation | Stats match flight; invalid designs explained |
| 3.9 | **New vehicles:** start from a root part on open ground | Build a rover and a ship from scratch, drive and fly them |
| 3.10 | **Jointed parts** (brought forward 2026-11-13): hinge and rotor parts, rigid groups turning about joints (kinematic articulation), moving colliders and meshes, joint angles and poses | The heavy frame raises its arms and swings its binders on Windows; tests |
| 3.11 | **Animated walking:** legs declared in the design, a locomotion controller and gait, two-joint leg solving onto the terrain, walk controls | The heavy frame walks over hills and turns |
| 3.12 | **Part models:** glTF models in `assets/models/` merged into vehicle meshes; generated original low-poly models for the frame and core parts | The frame looks like armour, not boxes; draw calls stay a few per vehicle |
| 3.13 | **Save and load:** versioned JSON, object records, `ObjectId` allocator, terrain deltas, quick save and load | Round-trip tests; reload puts everything back as it was |
| 3.14 | **Per-ship cruise characteristics** from the drive part and the ship (objects-vehicles-physics §8) | Heavier ships charge and turn slower in cruise |
| 3.15 | **Batch review** | Exit criteria met |

**Exit criteria:** build a rover and a ship from parts in the world, drive and fly them, land on
legs and wheels, walk the heavy frame with jointed limbs, save and load everything; no frame over 50 ms when entering or leaving the
builder; a 300-part ship costs about as much per tick as the Kestrel; CI passes.

**Status:** done on branch `batch3/builder`. The 3.15 review (2026-11-14, a full scripted
round on Windows, q0046–q0047) passed:
- **Build and fly or drive from parts in the world:** a rover and a lander built from scratch,
  boarded, driven and flown; the builder's place, remove, undo, redo, symmetry and sections.
- **Land on legs and wheels:** the Kestrel lands and stays put, from 3 km and from orbit (orbit to
  the ground in about 5 minutes, then stepping out); the rover drives on its wheels.
- **Walk the heavy frame with jointed limbs:** poses, walking, turning and stopping standing;
  mechs stand by themselves on slopes.
- **Save and load everything:** F5 / F9 put the world back (objects, states, poses, pads,
  time); round-trip tests.
- **No frame over 50 ms entering or leaving the builder:** three enter / change / leave cycles,
  run alone: 0 frames over 50 ms, worst 33 ms.
- **300 parts per tick:** flying 18 µs against the Kestrel's 10 µs; landed 96 µs against 20 µs
  (the gap noted at 3.4; both far inside the 33 ms tick).
- CI passes.

What Batch 3 built:
- 3.1–3.9: rapier physics per region, terrain collision patches, the part and design format with
  landing legs, design compilation, wheels and a rover, the walker colliding with vehicles,
  the builder (keys and mouse picking, symmetry, sections, stats and balance warnings) and new
  vehicles from scratch.
- 3.10–3.12 (brought forward): jointed parts and poses (staged moves, sliders), animated
  walking, part models built in Blender with a shared palette, baked occlusion and a hull
  detail shader, and support for 100 m vehicles.
- 3.13 save and load; 3.14 per-ship cruise characteristics from the drive's rating and the
  ship's mass.
- Vehicle and building design now runs on its own track (AGENTS.md, design/design-track.md);
  the Bastion in the game is a placeholder until its redesign is imported. The builder lists
  the general parts first, then each design family's.

Known gaps for later:
- builder stats measure part centres, not extents; the lander parts are overpowered (~10 g)
- wheels don't spin or move with their suspension; soles don't roll to side slopes
- a landed 300-part ship costs ~5× the Kestrel per tick (fine at this size)
- the descent test now has one or two frames just over 50 ms (Batch 2 had none)
- the Bastion's frame rate fell to ~22 fps after its stance change and is back to 66–89 fps in
  this round, for reasons not pinned down
- no save list menu yet (with the first menus, Batch 6); fuel and power come in Batch 7

### 7.5 Batch 4 tasks

Decisions behind these tasks: [living-planet.md](living-planet.md). Brought forward on
2026-11-14 (a first slice of what were Batches 6–8, now 7–9), before the survival-builder
loop, so the planet has places and people and its size can be decided with them.

| # | Task | Done when |
|---|---|---|
| 4.1 | **Content director with recorded responses:** the request interface for generated text and choices (culture profiles, faction and settlement lore, codex text) in `ai_gateway`, a replay provider answering from `content/lore/`, validation against schemas, the cache key; the two cultures' profiles and lore written as recorded responses | A request returns its recorded, validated response; an unknown request falls back to a plain default; tests |
| 4.2 | **Cultures and factions:** data and schemas (culture profile, faction, relations), naming rules and a name generator, the two cultures and their factions on Meridian in the star system data | Names read as each culture's; schema checks; tests |
| 4.3 | **Settlement generation:** sites scored by the planet's climate and terrain in each culture's home region, spacing by size, factions dividing them, names and lore; shown on the planet map | Settlements land in sensible places (flat, dry, near water; the colony on open ground); deterministic; a map with both cultures' settlements |
| 4.4 | **Aggregate simulation:** food, goods, population and mood per settlement, faction relations drifting; parallel by region; settlement state in saves | Populations grow and shrink believably over game days, at any time warp; save and load keep them; < 1 ms per step |
| 4.5 | **Building kits and layouts:** the medieval kit (CC0 village and town packs) and a stand-in colony kit imported with their licences; layouts on a tangent grid per culture style; foundations flatten the terrain (with a spatial index for many flattened spots); building collision | Layouts look like a village and a colony from above; the walker and vehicles stop at walls; terrain sampling stays fast |
| 4.6 | **Settlements on screen:** buildings drawn instanced near the player, a simplified form at a distance (no popping), lights at night | Flying in from orbit, settlements appear smoothly; lit at night; no frame over 50 ms |
| 4.7 | **Animated people:** CC0 humanoids on one skeleton, dressed per culture, with the shared animation set (walk, run, idle, work, sit), skinned and animated in the game | A few characters walk, idle and work in place, both cultures' clothing |
| 4.8 | **People with schedules:** promotion near the player from counts by role and demotion on leaving; daily schedules by local time (home, work, meals, gathering); walking the street graph, keeping clear of each other and the player; level of detail | People go about their day and change activity with day and night; ≤ 64 per settlement; no frame over 50 ms when a settlement fills |
| 4.9 | **Meeting the world:** entering a settlement shows its name, faction and culture; the inspector shows its state and lore; looking at a person shows name and role | Visible in a test round |
| 4.10 | **Planet size with settlements:** compare Meridian at 300, 600 and 1000 km with settlements on it (`ULTRA_SPACE_BODY_RADIUS`) | The developer decides the size (universe design §1) |
| 4.11 | **Batch review** | Exit criteria met |

**Status (2026-11-14):** 4.1–4.11 done on branch `batch4/living-planet`, waiting for the
developer's go-ahead to merge. 4.10 decided: sizes vary per planet, Meridian stays at 600 km;
density comes from settlement patterns (living-planet.md §8b). The 4.11 review (Windows rounds
q0049–q0060): both cultures' settlements are visible from the air (distant forms out to
250 km, lit at night) and sit on sensible ground; at the biggest Hollish town and the
Compact's colony people walk the streets to work in the morning and fill the town square in
the evening; entering shows the banner, the inspector the live state and lore, looking at
someone their name and role; populations change over game days and survive save and load;
flying in at ~300 m/s while a town streams in and fills with people had no frame over 50 ms
(one at the end screenshot); the earlier tests (smoke, landing, walk, pad, rover, bump,
save-load, deorbit-land) still pass, descent with one 53 ms frame; CI passes. Known for later:
- **density**: 24 + 8 settlements leave the land between them empty; next comes the density
  task for Meridian's two patterns (living-planet.md §8b);
- the Kenney and Quaternius stand-ins (bright cartoon palette, the colony's identical domes,
  coveralls) until the design track's kits and characters;
- towns have no walls or gates and no fields yet; trees are single models;
- people don't sit (no benches), carry nothing and don't talk to the player (Batch 11).

**Exit criteria:** flying over Meridian, both cultures' settlements are visible and sensibly
placed; land at a medieval village and at a colony and walk among people going about their
day, changing with day and night; entering shows names, factions and cultures; populations
change over game days and survive save and load; no frame over 50 ms when settlements stream
in or fill with people; CI passes.

### 7.6 Batch 4b tasks

Make Meridian the test bed for every stage of the game's peoples (developer, 2026-11-14), each
at its own density (living-planet.md §8b), without loading or drawing everything at once.

| People | Tier and polity | Where |
|---|---|---|
| The Hollin | Medieval realm | Central continent, 20°N 3°E |
| The Compact | Spacefaring charter colony | Eastern continent, 9°N 77°E |
| New | Stone Age chiefdoms | Eastern continent's southern jungle, ~12°S 82°E (near the Compact: contact) |
| New | Ancient city-states | Western continent's south coast, ~8°N 140°W |
| New | Early Modern maritime kingdom | Southern continent's north, ~30°S 100°W |
| New | Industrial nation | North-eastern continent, ~40°N 140°E |
| New | A Modern nation that destroyed itself: a wasteland with enclaves | South-east island continent, ~38°S 128°E |

| # | Task | Done when |
|---|---|---|
| 4b.1 | **Settlement hierarchy and patterns:** size class and roles replace the settlement kind; each culture's words; settlement patterns as data per tier and polity | Done (2ed075b) |
| 4b.2 | **Counts from area:** settlements placed by each people's pattern over its home region's good land (seats, then classes by spacing), territories by faction strength, populations by class; the aggregate simulation at that scale (stepped in slices); saves from before start their societies fresh | The Hollin ~1,500–2,000 settlements, the Compact ~20; generation within the load budget; aggregate cost per tick < 1 ms; tests |
| 4b.3 | **The peoples of Meridian:** five new cultures with profiles, factions and settlement lore (recorded responses), settlement patterns for the Stone Age, Ancient, Early Modern, Industrial and wasteland, home regions; the planet map | Seven peoples placed sensibly, each at its density (from tens of thousands to millions); a map; tests |
| 4b.4 | **A wasteland from history:** the Modern nation's catastrophe as generated history: hazard zones as a condition overlay, ruins of its old pattern, survivors in enclaves; hazards shown and felt (a warning, the look of the land) | The south-east continent reads as a ruined land with a few guarded enclaves |
| 4b.5 | **Layouts and foundations on approach:** a settlement's layout, foundations and collision made on workers within a few km and dropped after; terrain tiles and physics follow | Loading no slower than Batch 4; no frame over 50 ms when a village appears; walls still stop the walker and vehicles |
| 4b.6 | **Distant forms batched:** settlements far away drawn as merged forms per terrain region from their records, night lights from orbit, the real layout replacing them near; ruins drawn; hazard zones blight the land's look (a terrain field the shaders read) | Done (905ec8a): streaming hitches found and mostly fixed (near cells, models only close, people a few a frame, primers); the fly-in 2–3 frames over 50 ms |
| 4b.7 | **Stand-in kits and characters for every tier:** CC0 packs (downloaded as before) built into kits and characters for the Stone Age, Ancient, Early Modern, Industrial and the wasteland, with layouts for their styles (camps, walled cities, ports, industrial grids) | Done: five kits and clothing sets, camp/grid/harbour layouts, ports on the coast (living-planet.md §8b as built) |
| 4b.8 | **Roads, dwellings and surroundings** *(widened 2026-11-15)*: roads joining settlements, dwellings of each era between them; each settlement's surroundings (living-planet.md §8d): fields as a terrain layer, boundaries, walls and gates where the pattern calls for them, landmarks; all made on approach | Done: walls, fields, dwellings and roads (living-planet.md §8d as built); a test checks something built within 1.2 km between villages |
| 4b.9 | **Batch review** | Done (2026-11-15): see the review below |

**Exit criteria:** seven peoples on Meridian, each visitable and at its own density, each
settlement in its surroundings; flying low
over settled land, settlements every few minutes and several in view from a height; from orbit
at night, the settled lands lit (the industrial nation brightest, the Stone Age barely, the
wasteland dark but for its enclaves); loading no slower than Batch 4; no frame over 50 ms
flying low at 300 m/s or landing by a settlement; CI passes.

**Review (task 4b.9, 2026-11-15, round q0083 of all 59 tests, perf re-run q0084).** Seven
peoples on Meridian (5,962 settlements, 5.2 M people), each laid out in its stage's style
with its kit and people: Tayu camps round fires behind palisades, Velani stone towns with red
roofs and colonnaded temples, the Hollin's walled towns in their hedged fields, Corvantine
towns with churches, windmills and harbours, Aldmark grids of houses, tenements and factories,
Kesh enclaves behind barricades in the dead land, the Compact's colony. Between them
farmsteads every kilometre or so and roads from settlement to settlement; from orbit the
settled lands lit at night. Every test ran to its end. Performance (tests run alone): the
descent from orbit 1 frame over 50 ms (56 ms) and the fly-in to the biggest town at 300 m/s
1 frame (61 ms), both at the final screenshot as in Batch 4; settlements cost about 1 ms a
frame on average against the bare fly-in. Loading: the loading overlay's longest frame about
1.2 s (generation of seven peoples' societies and the road networks). CI passes. Known for
later: city density and sprawl (no core-to-suburb gradient), the Kenney palette (teal flat
roofs, pink stone), skin-tight painted suits for the modern peoples, roads straight between
settlements, hedges as slabs from the ground, ports by role only where the coast allows, the
bare land between it all (ground cover, Batch 5: the world, refined).

**Batch 4 is closed (2026-11-15).** Batch 4 grew from two cultures (Batch 4) to seven peoples
with density, kits, walls, fields, dwellings and roads (4b); a planned Batch 4c of ground cover
was folded into Batch 5, the world refined (2026-11-15: the world before the shell and the
survival loop, Batches 6 and 7). The rough edges
above are **known issues, not tasks**: each is taken up when a later batch works on that system
(or on the design track, for the art), not as an extension of Batch 4:

| Known issue | Where it belongs |
|---|---|
| Big cities sprawl beyond their walls; no dense core | Batch 11 (cultures and people: city life) |
| Kenney stand-in palette (teal flat roofs, pink stone) | Design track (the game's own kits) |
| Modern and industrial people in painted, skin-tight suits | Design track (characters) |
| Roads straight between settlements: no routing round hills, no bridges | Batch 9 (traffic and trade on the road graph) |
| Hedges and field walls as plain boxes | After the demo village (ground cover in the block style) |
| Props and signs of life (smoke, cooking fires) | After the demo village (effects) |

### 7.7 Batch 5 tasks: the world, refined

Decided 2026-11-15 (developer): the world still reads as a placeholder, and it comes before the
game shell and the survival loop. Revised the same day after the style renders: the world is
made of **shaped blocks** ([block-world.md](block-world.md)) in realistic light
([visual-style.md](visual-style.md)), and the batch proves it on **one demo village** on
Meridian before anything planet-wide. The village is the scene of the style renders made real:
a timber house in its yard with a garden, a woodshed, a well and a wagon, a path out to the
player's landed ship and a dug pit, woods around, villagers and chickens. **The player starts
there.** Only this village is built from block models; every other settlement keeps its
stand-in kit until the design track replaces them. The finish line is a fixed set of review
views of the village; anything off outside them is a known issue, not a new task.

| # | Task | Done when |
|---|---|---|
| 5.1 | **Block core** (`sim_blocks`, engine-independent): cells in power-of-two sizes aligned to their own multiples, the shape set (block, wedge, outer and valley corners in any orientation) and the slope rule with per-model recipes, a mesher merging flat faces and runs of slopes, chunked sparse storage | Unit tests for the shape rule and meshing; a model exported by the style scripts meshes to the same faces as in Blender |
| 5.2 | **Block models:** the model format (size, cells, palette colours, shape recipe; engine-agnostic files under `assets/` with a schema), the style scripts moved into the repo as the authoring tool (`tools/blocks/`) exporting every village model; the game loads and draws them | CI validates the exported models; each village model drawn in the game matches its render |
| 5.3 | **The ground as blocks:** chunks of 25 cm blocks round the player (grass, soil, rock from the terrain function, the slope rule), made on workers and streamed, nearest first; the far tiles stepped to match; colliders (rapier voxels and slope pieces) for walking and landing; pads levelled under structures | Land, walk and drive on block ground near the village; flying low at 300 m/s holds the frame budget |
| 5.4 | **The demo village:** one village of the Meridian peoples built from block models on levelled pads, its paths, gardens and props; nature round it (trees, bushes, rocks, flowers, grass) placed through an occupancy plot so nothing overlaps, on the grid in quarter turns; villagers and chickens standing or in simple idle poses (block groups on a skeleton; full animation later); the player starts there on foot beside their landed ship | A new game starts in the village; it reads as the style renders from the same views |
| 5.5 | **Light and weather to match the renders:** sun shadows, ambient occlusion, exposure and AgX tonemapping, bloom, haze and ground fog; day, golden hour, night with lit windows and lanterns, rain, fog; quality tiers | The review views read as the reference renders in each light and weather |
| 5.6 | **Review views:** a test round of the village (wide, house, close, people, ship) in day, golden hour, fog, rain and night, plus the descent from orbit to the village | The views run in one round; no frame over 50 ms |
| 5.7 | **The plains test planet** (developer's request, 2026-11-16): the game's world becomes one small planet of plains, the football of block-world.md §5.1 (12 pentagons and 20 hexagons, 40 km edges), each plain a region with its own climate and land, the demo village in a grassland region; Halcyon kept as test data | A new game starts in the village on the new planet; the football shows from orbit; walking and flying work on the plains; all earlier tests still pass on Halcyon |
| 5.7a | **The world as a simulation** (developer's request, 2026-11-17): Tessera round again, its land, climate and regions generated on a mesh (planet-geography.md §1b: plates, uplift, erosion, rain, rivers, biomes, regions as river basins) | The maps and test views show continents, ranges, rivers, climate belts and natural regions |
| 5.7b | **The streamed block zone:** the block ground planet-wide on a bent cube-sphere grid (six faces, equal angles, face 0 centred on the village), chunks made round the player anywhere, top colours from the world's cover, tiles hidden round the camera; the block surface stands within 2 km of the focus (the camera) | Blocks show and carry the player anywhere on Tessera (`blocks-far`), the village unchanged |
| 5.7c | **The faceted ground** (developer, 2026-11-18; adopted as the default): the ground near the player a density field meshed with surface nets, flat-shaded and coloured per facet, procedural detail (grass, rock, sand, earth, snow), the village's pads, paths, pit and tunnel carved, sphere digs and fills in code; size-on-screen detail (8 px), the far tiles beneath it and beyond; blocks kept as `ULTRA_SPACE_GROUND=blocks` | The village, Rook Vale and flight views at 2560 × 1552 without seams or flicker; walking into the pit; ~100 fps alone |
| 5.7d | **Ground cover planet-wide** (developer, 2026-11-18; [ground-cover.md](ground-cover.md)): 32 species from generators with variants, recipes per biome and for shore, steep and high ground, six layers of procedural variety, merged per ground chunk with size-on-screen levels, collision near bodies | Forest, taiga, desert, savanna, tundra, coast and village views read as their landscapes; `blocks-perf` within budget |
| 5.7e | **The planet environment** (developer, 2026-11-20; [planet-environment.md](planet-environment.md)): E1 fixes (cover lit like the ground at night, tundra captures, the village's surroundings temperate forest), E2 wind (a planet wind field; cover sways, water and clouds follow), E3 the far landscape (far ground coloured by its cover, distant trees, rocky mountains, larger landforms), E4 water (depth colour, foam, waves; rivers and lakes drawn), E5 ground detail (litter, pebbles, puddles, paths; dunes, rock pillars, dry beds; moss and frost), E6 weather by climate over time and effects (dust, plumes, splashes, smoke); animals and sound later | Each step judged in a test round; flight shows each biome's character from altitude; `blocks-perf` within a few fps of 82 (q0131) |
| 5.8 | **Digging and placing** (for the faceted ground): a dig tool removing (and a fill tool adding) a sphere of ground under the crosshair, timed by hardness, drops to a small inventory (stacks by material, a simple panel, saved with the player); placing block models (built things) on their own grids; edits kept per chunk and saved; the touched chunks and colliders rebuilt within a frame or two | Dig a hole and build a wall in the village; both survive save and load |
| 5.9 | **Batch review** | The views judged against the renders by the developer, from screenshots |

**Progress (2026-11-18):** 5.1–5.7 done; Tessera round again with world generation as a
simulation (5.7a), the ground streamed planet-wide (5.7b) and, after comparing blocks and facets
in the game, the faceted density ground adopted (5.7c; terrain blocks 50 cm remain an option);
the village's ground is the game's own layout (`content/data/village_layouts`) on the real ground;
test rounds at 2560 × 1552. 5.7c confirmed (q0117: no seams or flicker). 5.7d done (q0122):
36 generated species planet-wide with collision near bodies; `blocks-perf` 83 fps with cover,
99 without (Balanced power plan, plugged in). Review of the environment (q0132,
[planet-environment.md](planet-environment.md)): still a still life; 5.7e (E1–E6) under way:
E1 fixes done (q0133), E2 wind done (q0137–q0142: cover sways by species, wind lines, the
village's trees and grass generated; `blocks-perf` 78–79 fps, 95 without cover, same as before);
next E3 (the far landscape), then E4–E6, 5.8 (digging and placing on the faceted ground), 5.9
(review).

- **5.1** `sim_blocks`: cells in power-of-two sizes, the shape set and slope rule (a single slope
  also needs a neighbour along its edge), a mesher merging flat faces and slope planes and
  hiding faces a neighbour covers, chunked storage, block models. Every exported model meshes
  to the same area per colour as the scripts' own mesher (`tests/models.rs`).
- **5.2** `tools/blocks/` (the style scene, now the authoring tool) exports 134 models, the
  palette and the village to `assets/export/blocks/` (`tools/build-blocks.sh`; schemas
  `block_model` and `block_village`; CI checks the export is current and valid).
- **5.3** `sim_planet::BlockGround`: columns of 25 cm blocks in a 1.5 km disc round the village,
  in the village's tangent grid mapped onto the sphere per vertex (the planet-wide layout of
  flat 40 km plains, block-world.md §5.1, is later); the village's exported ground eases into the natural ground over 5 m. The
  terrain reads its surface (landing, walking, placing); collision patches sink 0.75 m below it
  and chunk meshes near bodies carry contact; the village's buildings, props and trunks are
  boxes walkers bump into. Chunks within 90 m of the camera are meshed on workers; tiles are
  hidden only where the blocks are ready (in the depth prepass too).
- **5.4** The village on Meridian (temperate grassland near the medieval peoples, away from face
  seams, settlements and dwellings): 655 placed models, villagers and chickens standing; a new
  game starts on foot beside the landed ship in the morning (`ULTRA_SPACE_START=orbit` and the
  debug starts keep the old start; `ULTRA_SPACE_START_TIME`, `ULTRA_SPACE_VILLAGE_VIEW`).
- **5.5** AgX tonemapping; lanterns and windows lit from dusk with point lights; weather
  (`ULTRA_SPACE_WEATHER=clear|rain|fog`, `;` cycles): overcast sun and sky, distance fog and a
  fog dome, rain streaks, wet blocks.
- **5.6** Review round: `village`, `-dusk`, `-night`, `-rain`, `-fog`, `-walk`, `-perf` in
  `tools/test-scripts/tests.txt`. Village views about 140 fps; no frame over 50 ms in play
  except screenshot frames.
- **5.7** Tessera (`content/data/star_systems/tessera.json`, a star and one planet; the default;
  `ULTRA_SPACE_SYSTEM=halcyon` loads the old system, and the tests written for Meridian do):
  `sim_planet::plains` (the football's flat plains, ray hits, crease weights), `regions` (nine
  region types in `content/data/region_types/standard.json`; ice caps on the spin axis, two seas,
  the rest by latitude, named), the terrain's plains mode (the regions' elevations blended
  over ~2.5 km at creases, ground on the plain moved out by the elevation, a flat sea per
  plain, region climates), gravity and "up" along the plain near the ground
  (`BodyEnv::up`/`gravity`), tiles always drawn (the faceted outline shows), the village on the
  flattest grassland at least 6 km inside its plain, lying exactly flat; HUD altitude above sea
  level and "Entering <region>" on crossing a crease. Tests: `planet-day`, `planet-dusk`,
  `creases`.
- **5.7, arcade direction** (developer, 2026-11-17: the toy world goes arcade): extreme land
  (peaks to ~13 km, seas ~4.5 km deep with trenches, high downs and vales, mesas and canyons;
  `region_types/standard.json`); a flat cloud deck 7 km over each plain (the atmosphere shader
  meets the plains' planes; clouds ten times smaller than Meridian's, brighter); No Man's Sky
  flight by default (`FlightModel`; a landed ship lifts off into arcade flight when the controls
  move, L sets it down when low and slow, then it settles and rests; 600 m/s in the air, 4 km/s in
  space; J is the pulse drive above the atmosphere; no orbit line or Ap/Pe in arcade flight).
  Newtonian flight and orbits stay as a debug mode (N, `ULTRA_SPACE_FLIGHT=newtonian`, used by
  the Halcyon tests). `ULTRA_SPACE_START_REGION=<type>` starts over a plain of that kind. Tests:
  `fly-tessera`, `peaks`.

Known issues (not tasks): the inspector doesn't show the region yet; where two faces of the
ground grid meet (about 72 km from the village) the chunks of each face mesh alone (the tiles
beneath fill any crack); the far tiles have no detail patterns (they fade out
there anyway); blocks aren't made on a body of plains (the plains mode is terrain only);
visual rounds' frame rates come from several windows sharing the GPU (only the `-perf` tests
run alone); the player's ship is the
Kestrel's part model on the block ship's pad; villagers don't move yet; walls are boxes (a
building's eaves are part of its box); frame times on the test machine vary between runs (40 to
140 fps on the same views, the same with the blocks switched off).

**Exit criteria:** a new game starts in the demo village; it reads as the style renders by day,
dusk, night, in rain and fog; the player can walk, dig and place blocks, and edits persist; no
frame over 50 ms landing at or flying low over the village at default quality; loading no slower
than Batch 4b; CI passes.

**After the village (planned at its review, in this batch or the next):** water (depth colour,
shore foam, low-poly waves), planet-wide ground cover in the block style
([ground-cover.md](ground-cover.md)), weather changing over time by climate, effects (engine
plumes, landing dust, splashes, chimney smoke), the block zone across the plains' seams
(block-world.md §5.1) and the general Goldberg layouts for larger bodies, and the
other settlements' kits in blocks (design track).

**Then (planned when it starts):** Batch 6, the first playable loop: the game shell (main menu,
new game and continue, save slots, pause, settings, controls), the player's record (money,
standing with factions, journal), discoveries that fill the journal, contracts from settlements
(carry, visit, survey, passengers; rule-made templates, the format the director later writes in
Batch 10), basic audio (with the planet's ambient sound: wind, rain, water, footsteps, engines), first-time guidance. Batch 7, the survival-builder loop, follows.

### 7.1 When the object system is built

The object model, persistence and streaming design (technical overview §14) is settled up
front because it shapes everything else, but **each part is implemented in the batch that first
needs it, together with the systems that use it**, so its shape comes from real use rather than
guesses.

| Batch | Object-system parts implemented | Why then |
|---|---|---|
| **1** Space-scale core | `ObjectId` and the object registry (objects moving between region worlds keep their identity); frame-relative state; one region per body with transfers at tick boundaries; **vehicles made of parts** (part catalog, designs, instances), mass properties and thrust allocation; background region building; in-game inspector | Moving the ship between regions needs a stable identity; a ship built from parts from day one means the Batch 3 builder extends the flight model instead of replacing it. Design: [objects-vehicles-physics.md](objects-vehicles-physics.md) |
| **2** Seamless planet | Procedural keys and terrain deltas; streaming rules (gradual spawning, prefetch from travel); **hitch-budget stress test** (orbit → surface, no frame over 50 ms) | Terrain is the first procedural content and the first big streaming load |
| **3** Vehicle builder | Builder UI editing a vehicle instance's parts; attachment graph; object records and the **first versioned save file** with a format version and migration hook | The first thing worth saving is a vehicle the player built. Builder levels, data format additions and design compilation: [ships-stations-interiors.md](ships-stations-interiors.md) |
| **4** Living planet | Culture, faction and settlement records (generated from seeds, so untouched ones aren't saved); settlement state (population, stocks) and met people in saves; people promoted from counts near the player and demoted back | The first population: the simulation must keep places alive whether or not the player is there ([living-planet.md](living-planet.md)) |
| **6** First playable loop | Save slots with their names, times and places; the player's record (money, standing, journal, contracts) in saves; discovered places | The first menus and the first thing the player owns besides vehicles |
| **7** Survival-builder loop | Base definitions loaded from `content/data/` with schemas; containers and inventory (stackable items and unique items); per-region saves of bases and changed terrain; expiry rules for dropped junk | Gathering, crafting and building create items, containers and lots of world changes |
| ⭐ **Checkpoint A** | **Save format frozen:** golden save files and strict migrations start with the first public build | Before this, no player has a save, so the format may be reset freely |
| **7+** | Quest references by `ObjectId`, NPC ownership, unique items with generated histories | Combat, factions, quests and LLM content need them |

A first implementation attempt from Batch 0 is parked on the `batch0/object-model` branch for
reference. It isn't merged; each batch rebuilds what it needs from it.

---

## 8. Working with AI assistance

### 8.1 Division of labor

| AI assistant is strong at | Developer owns |
|---|---|
| Systems code and math (orbits, quadtrees, IK, control allocation), shaders | **Game feel:** whether flying, driving and combat feel good |
| Unit tests, test harnesses, benchmarks, debug tools | **Visual judgment** from native builds |
| Data schemas, procedural generators, content pipelines, the LLM gateway | **Art direction and assets:** choosing and assembling asset packs, animations |
| Refactors, bug hunts from logs and stack traces, docs | **Design decisions, priorities and scope cuts** |
| Several independent crates in parallel | **Play-testing**, the final say on fun |

**Assets:** start with asset packs (Kenney, Quaternius, Synty or similar), Mixamo
animations and procedural or AI-generated textures. A consistent low-poly or stylized look
is realistic for one person and scales well.

### 8.2 Working habits

1. **One batch at a time, in small tasks** (e.g. "quadtree split/merge for one cube face, with
   tests").
2. **Test simulation crates heavily.** Tests are how AI-written code is checked without
   looking at the screen.
3. **Debug overlays and logging from day one:** frame-tree inspector, LOD tile view, physics
   debug, thread and tick timings.
4. **Data-driven content** (parts, buildings, cultures as data files), so content can be
   generated and validated in bulk.
5. **A git branch per feature;** review changes as diffs.
6. **Keep the docs and `AGENTS.md` (AI assistant rules) updated,** so each session starts with
   full context.
7. **Cut scope ruthlessly.** If a batch runs past twice its estimate, simplify it and move on.
8. **Design ahead, implement when needed.** Settle cross-cutting designs early in `design/`,
   but implement each system in the batch that first uses it, together with the modules that
   depend on it. Don't build infrastructure for systems that don't exist yet.
