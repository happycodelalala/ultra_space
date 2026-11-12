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
| Extra tooling work | Batches 2–4 estimated **~20–30% longer** than with Godot |

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
| **4** | **Survival-builder loop** | Resource gathering, base building on tangent grids, production chains (parallel factory networks), terrain deltas, simple creatures | 2.5–4 mo |
| | ⭐ **Checkpoint A** | **A playable sandbox:** explore, build, fly. Candidate for a public demo or early access | |
| **5** | **Combat core** | Unified damage pipeline, first-person weapons, per-part vehicle damage, hostile creatures and bandits | 1–2 mo |
| **6** | **Living galaxy** | Multiple star systems simulated in parallel, tiered simulation, economy, factions, trade ships, galaxy map | 2–4 mo |
| **7** | **LLM gateway + first content** | AI gateway with Anthropic/OpenAI/replay adapters, caching, validation, budgets, fallbacks; generated culture profiles; galaxy and system history; codex text | 1–2 mo |
| **8** | **Cultures and people** | Settlements from building kits, NPC schedules, notable characters with LLM dialogue, director-generated quests | 3–4 mo |
| **9** | **Medieval warfare** | Directional melee, mounts, formation AI, 200+ soldier battles (GPU crowd animation), campaign-map battle promotion, auto-resolve | 3–5 mo |
| | ⭐ **Checkpoint B** | **A full game loop:** explore, build, trade, talk, fight wars | |
| **10** | **Mixed-tech and orbital** | Morale and doctrine AI, supply limits, orbital strikes, layered battles | 2–3 mo |
| **11** | **Capital ships** | Ship interiors, hull walking, bridge stations, EVA, boarding, hangars | 3–4 mo |
| **12** | **Breadth and polish** | Mechs, airships, sieges, diplomacy, more tech tiers, performance, modding, release prep | ongoing |

**Honest total:** roughly 3–4+ years to Checkpoint B and 4–5+ years to the full vision, part
of it in early access. Checkpoint A should come within roughly the first year.

Until Batch 7, earlier systems use a simple placeholder interface for generated text
(names, codex entries), so they plug into the AI gateway later without changes.

### 7.2 Batch 1 tasks

Decisions behind these tasks: [universe-scale-time-and-flight.md](universe-scale-time-and-flight.md).

| # | Task | Done when |
|---|---|---|
| 1.1 | **Units and conventions:** meters, seconds, `f64` positions, right-handed Y-up | Documented; math helpers tested |
| 1.2 | **Frame tree:** frames with parents; relative position and rotation between any two frames in `f64` | Correct relative transforms; under 1 mm error at 10¹² m |
| 1.3 | **Orbits on rails:** Kepler orbits at time *t*, planet spin and axial tilt | Periods, energy conservation and reference values tested |
| 1.4 | **Clocks:** integer universe tick counter; Galactic Standard Time (16×); local planet time; debug time warp | Clock conversions tested; orbits visibly advance under warp |
| 1.5 | **Star system data:** engine-neutral JSON schema and the hand-made Halcyon system, with star mass tuned for 20–60 hour years | Halcyon loads into the frame tree; invalid data gives clear errors |
| 1.6 | **Floating origin rendering:** camera = frame + `f64` offset; camera-relative `f32` transforms for Bevy | No visible jitter 10⁹ m from the star |
| 1.7 | **Distant planets via shaders:** raymarched spheres with lighting and atmosphere rims; placeholder meshes up close | Planets look right from 1,000 km to the whole system; no depth glitches |
| 1.8 | **Star and lighting:** glare, light from the star's real direction, HDR and bloom | Correct day and night sides on every body |
| 1.9 | **Sky:** procedural star field and nebula, fixed to the galaxy | Stable while flying |
| 1.10 | **First persistent objects:** `ObjectId`, frame-referenced locations; ship and player as objects (7.1) | Tested |
| 1.11 | **Ship flight:** `f64` rigid body, 6 degrees of freedom, thrust limits, gravity from the current sphere of influence, sphere collision with planets; flight assist modes with automatic "standing still" reference | Physics and energy tests; assist holds speed and hovers correctly |
| 1.12 | **Moving between frames:** crossing spheres of influence moves the ship to the new frame and region at a tick boundary | Position and velocity preserved; no visible jump |
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

### 7.1 When the object system is built

The object model, persistence and streaming design (technical overview §14) is settled up
front because it shapes everything else, but **each part is implemented in the batch that first
needs it, together with the systems that use it**, so its shape comes from real use rather than
guesses.

| Batch | Object-system parts implemented | Why then |
|---|---|---|
| **1** Space-scale core | `ObjectId` separate from ECS entity IDs; locations that reference frames in the frame tree; moving the player and ship between frames; background region building; in-game inspector for frames and objects | The frame tree defines what an object's location is; the ship and player are the first persistent objects |
| **2** Seamless planet | Procedural keys and terrain deltas; streaming rules (gradual spawning, prefetch from travel); **hitch-budget stress test** (orbit → surface, no frame over 50 ms) | Terrain is the first procedural content and the first big streaming load |
| **3** Vehicle builder | Built structures as one persistent object holding a part graph; **first versioned save file** with a format version and migration hook | The first thing worth saving is a vehicle the player built |
| **4** Survival-builder loop | Base definitions loaded from `content/data/` with schemas; containers and inventory (stackable items and unique items); per-region saves of bases and changed terrain; expiry rules for dropped junk | Gathering, crafting and building create items, containers and lots of world changes |
| ⭐ **Checkpoint A** | **Save format frozen:** golden save files and strict migrations start with the first public build | Before this, no player has a save, so the format may be reset freely |
| **5+** | Quest references by `ObjectId`, NPC ownership, unique items with generated histories | Combat, factions, quests and LLM content need them |

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
