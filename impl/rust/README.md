# Rust implementation

Cargo workspace: engine-independent simulation crates plus a Bevy front end.

## Crates

| Crate | Role | May depend on |
|---|---|---|
| `sim_core` | Frame tree, 64-bit coordinates, orbits, tick scheduler | std, math/ECS crates (e.g. `bevy_ecs`) |
| `sim_planet` | Terrain generation, player-made deltas | `sim_core` |
| `sim_vehicle` | Part graphs, mass properties, control allocation | `sim_core` |
| `sim_economy` | Factories, factions, tiered simulation | `sim_core` |
| `sim_combat` | Damage events, morale, combat AI | `sim_core` |
| `ai_gateway` | LLM providers (Anthropic, OpenAI, replay), caching, validation | `sim_core` |
| `game_bevy` | Bevy app: rendering, input, UI, audio (binary: `ultra_space`) | everything above + `bevy` |

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
| Build everything | `cargo build` |
| Test sim crates (no GPU needed) | `cargo test --workspace --exclude game_bevy` |
| Test everything | `cargo test --workspace` |
| Lint | `cargo clippy --workspace --all-targets` |
| Format | `cargo fmt --all` |
| Run the game (WSL, via WSLg) | `cargo run -p game_bevy` |
| Export a Windows build | `../../tools/export-windows.sh` |

Windows builds are exported to `C:\Users\yangc\Apps\ultra_space\`
(`/mnt/c/Users/yangc/Apps/ultra_space/` from WSL). Judge visuals and performance there, not
in WSL.
