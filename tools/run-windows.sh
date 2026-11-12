#!/usr/bin/env bash
# Runs the native Windows build from WSL with a test script, so a test flight runs on the real
# GPU in a window on the Windows desktop (WSL itself can only render in software).
#
#   tools/run-windows.sh [--export] <script.txt> [NAME=value ...]
#
# --export      rebuild and export the Windows build first (tools/export-windows.sh)
# script.txt    a test script (format: impl/rust/crates/game_bevy/src/script.rs; examples in
#               tools/test-scripts/); copied next to the game, where its screenshots land
# NAME=value    extra environment variables for the game, e.g. ULTRA_SPACE_START_ALTITUDE=3000
#
# RUN_NAME=<name> in the environment names the run folder (default: the script's name), so the
# same script can run with different settings, or several runs at once (tools/run-tests.sh).
# Each run writes its own log (game.log) into its folder.
#
# Waits for the game to exit, then prints the important log lines and the screenshot paths.
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel)"
DEST="${ULTRA_SPACE_EXPORT_DIR:-/mnt/c/Users/yangc/Apps/ultra_space}"

if [[ "${1:-}" == "--export" ]]; then
    shift
    "$ROOT/tools/export-windows.sh"
fi
SCRIPT="${1:?usage: tools/run-windows.sh [--export] <script.txt> [NAME=value ...]}"
shift

NAME="${RUN_NAME:-$(basename "$SCRIPT" .txt)}"
RUN_DIR="$DEST/runs/$NAME"
mkdir -p "$RUN_DIR"
find "$RUN_DIR" -maxdepth 1 \( -name '*.png' -o -name 'game.log' \) -delete
cp "$SCRIPT" "$RUN_DIR/script.txt"

# Pass variables to the Windows process through WSLENV.
export ULTRA_SPACE_SCRIPT
ULTRA_SPACE_SCRIPT="$(wslpath -w "$RUN_DIR/script.txt")"
export ULTRA_SPACE_LOG
ULTRA_SPACE_LOG="$(wslpath -w "$RUN_DIR")\\game.log"
WSLENV_LIST="ULTRA_SPACE_SCRIPT:ULTRA_SPACE_LOG"
for pair in "$@"; do
    var="${pair%%=*}"
    export "$var"="${pair#*=}"
    WSLENV_LIST="$WSLENV_LIST:$var"
done
export WSLENV="${WSLENV:+$WSLENV:}$WSLENV_LIST"

echo "Running $NAME on Windows (watch the game window)..."
# Start from the game folder (cmd can't use a WSL path as its working directory).
(cd "$DEST" && ./ultra_space.exe > /dev/null 2>&1) || echo "game exited with status $?"

LOG="$RUN_DIR/game.log"
echo "--- log ($LOG) ---"
grep -E "AdapterInfo|script:|hitch|startup frames|ERROR|panicked" "$LOG" | cut -c 1-240 || true
echo "--- screenshots ---"
find "$RUN_DIR" -maxdepth 1 -name '*.png' -print | sort
