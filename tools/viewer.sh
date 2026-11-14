#!/usr/bin/env bash
# Starts the design viewer (impl/viewer) on http://localhost:${1:-8765}/ — open it in a browser
# on Windows or in WSL. It shows content/data designs and assets/export/models part models,
# reloads when they change, and saves only in the game's formats.
set -euo pipefail
ROOT="$(git -C "$(dirname "$0")" rev-parse --show-toplevel)"
exec python3 "$ROOT/impl/viewer/server.py" "${1:-8765}"
