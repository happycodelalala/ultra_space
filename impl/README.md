# Implementations

Each subfolder is one implementation of the game in a particular language and engine. It
reads the shared `design/`, `content/` and `assets/` folders at the repo root.

| Folder | Status | Stack |
|---|---|---|
| [`rust/`](rust/README.md) | Active | Rust, Bevy front end, engine-independent sim crates |

Switching engines usually means adding a front-end crate next to `game_bevy` (same language)
or a new folder here (different language). Design, content and assets stay where they are.
