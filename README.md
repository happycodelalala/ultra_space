# Ultra Space

A sci-fi sandbox: seamless procedural planets and star systems, player-built vehicles,
settlements and factories, cultures at different tech levels, factions and large-scale
ground and space battles.

## Repository layout

The repo keeps **what the game is** apart from **how it is built**, so the engine or
framework can change without touching the design, content or assets.

```
ultra_space/
├─ design/     Game design, ideas and plans. Engine-agnostic, no code.
├─ content/    Game data: schemas, definitions, LLM prompts, lore. Engine-agnostic formats.
├─ assets/     Art and audio: source files and engine-neutral exports (glTF, PNG, OGG).
├─ impl/       Implementations. Everything engine- or language-specific lives here.
│   └─ rust/   Current implementation: Cargo workspace (Bevy front end + sim crates).
└─ tools/      Repo-wide scripts: git hooks, secret checks, build/export helpers.
```

**Rules**

- `design/`, `content/` and `assets/` never reference engine APIs or engine file formats.
- Engine-specific code, config and docs live only under `impl/<implementation>/`.
- Inside `impl/rust/`, the `sim_*` and `ai_gateway` crates are engine-independent. Only
  `game_bevy` depends on Bevy. A future engine means a new front-end crate (or a new
  `impl/<name>/`) reusing the same design, content and assets.

## Getting started

See [`impl/rust/README.md`](impl/rust/README.md) for setup, build, test and Windows export.

## Key documents

- [Technical overview](design/technical-overview.md): game systems and how they work
- [Development plan](design/development-plan.md): engine decision, architecture, batches
- [Agent instructions](AGENTS.md): working rules for AI coding assistants
