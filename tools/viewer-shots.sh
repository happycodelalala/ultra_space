#!/usr/bin/env bash
# Review shots of a design or part without opening a window: renders the standard views in
# headless Chromium (software WebGL) and saves them to .viewer-shots/.
#   tools/viewer-shots.sh bastion [pose] [views]     e.g. tools/viewer-shots.sh bastion wings front,left
#   tools/viewer-shots.sh --part frame.foot
#   FOCUS=torso,head tools/viewer-shots.sh bastion stand    # frame the shots on those parts
set -euo pipefail
ROOT="$(git -C "$(dirname "$0")" rev-parse --show-toplevel)"
PORT=${VIEWER_PORT:-8799}
if [[ "${1:-}" == "--part" ]]; then
    QUERY="part=$2"
else
    QUERY="design=${1:?design id}"
    [[ -n "${2:-}" ]] && QUERY+="&pose=$2"
fi
[[ -n "${3:-}" ]] && QUERY+="&views=$3"
[[ -n "${FOCUS:-}" ]] && QUERY+="&focus=$FOCUS"
CHROME=$(ls -d "$HOME"/.cache/ms-playwright/chromium_headless_shell-*/chrome-headless-shell-linux64/chrome-headless-shell 2>/dev/null | tail -1)
[[ -x "$CHROME" ]] || { echo "viewer-shots: no headless Chromium (npx playwright install chromium-headless-shell)"; exit 1; }
python3 "$ROOT/impl/viewer/server.py" "$PORT" >/dev/null 2>&1 &
SERVER=$!
trap 'kill $SERVER 2>/dev/null' EXIT
sleep 0.5
mkdir -p "$ROOT/.viewer-shots"
before=$(date +%s)
"$CHROME" --headless --no-sandbox --use-angle=swiftshader --enable-unsafe-swiftshader \
    --window-size=1600,1000 --virtual-time-budget=60000 --dump-dom \
    "http://localhost:$PORT/?$QUERY&shots=1&bare=1" 2>/dev/null | grep -o "<title>[^<]*</title>" || true
find "$ROOT/.viewer-shots" -name '*.png' -newermt "@$before" | sort
