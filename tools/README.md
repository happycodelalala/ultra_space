# Tools

Repo-wide scripts. Run from anywhere inside the repo.

| Script | Purpose |
|---|---|
| `ci.sh` | Runs every check: secrets, engine boundary, format, Clippy (warnings as errors), tests. `--windows` adds a Windows type-check |
| `check-boundaries.py` | Fails if a non-`game_*` crate depends on Bevy, a renderer, windowing or GPU crates (only `bevy_ecs` and `bevy_tasks` allowed) |
| `setup.sh` | One-time setup: enables the git hooks in `git-hooks/` |
| `check-secrets.sh` | Blocks commits containing API keys (`--all` scans all tracked files) |
| `export-windows.sh` | Cross-compiles a Windows release build and copies it, with assets, to `C:\Users\yangc\Apps\ultra_space\` (override with `ULTRA_SPACE_EXPORT_DIR`) |
| `git-hooks/pre-commit` | Runs `check-secrets.sh` on every commit |
| `git-hooks/commit-msg` | Blocks AI attribution lines (AI co-authors, session links, "generated with") in commit messages |
