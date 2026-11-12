# Tools

Repo-wide scripts. Run from anywhere inside the repo.

| Script | Purpose |
|---|---|
| `setup.sh` | One-time setup: enables the git hooks in `git-hooks/` |
| `check-secrets.sh` | Blocks commits containing API keys (`--all` scans all tracked files) |
| `export-windows.sh` | Cross-compiles a Windows release build and copies it, with assets, to `C:\Users\yangc\Apps\ultra_space\` (override with `ULTRA_SPACE_EXPORT_DIR`) |
| `git-hooks/pre-commit` | Runs `check-secrets.sh` on every commit |
