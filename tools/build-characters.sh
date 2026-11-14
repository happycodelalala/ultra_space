#!/usr/bin/env bash
# Builds the characters (content/data/clothing_sets/*.json) from the Quaternius packs in
# $ASSET_LIBRARY into assets/export/characters/<set>/ (one .glb per character and set.json
# with roles and animations), in headless Blender.
#   tools/build-characters.sh                         # every set, and the stamp CI checks
#   tools/build-characters.sh [set ids...] [--preview DIR]
#   tools/build-characters.sh --check                 # fails if the characters are older than their inputs
# Blender: $BLENDER, else ~/.local/opt/blender-*/blender, else blender on the PATH.
set -euo pipefail
ROOT="$(git -C "$(dirname "$0")" rev-parse --show-toplevel)"

stamp() {
    python3 - "$ROOT" "$1" <<'PY'
import hashlib, sys
from pathlib import Path
root, mode = Path(sys.argv[1]), sys.argv[2]
h = hashlib.sha256()
for f in sorted((root / "tools/characters").glob("*.py")) + sorted((root / "content/data/clothing_sets").glob("*.json")):
    h.update(f.name.encode()); h.update(f.read_bytes())
stamp = root / "assets/export/characters/characters.stamp"
if mode == "write":
    stamp.write_text(h.hexdigest() + "\n")
elif not stamp.exists() or stamp.read_text().strip() != h.hexdigest():
    print("build-characters: characters out of date (run tools/build-characters.sh)")
    sys.exit(1)
else:
    print("build-characters: ok")
PY
}

if [[ "${1:-}" == "--check" ]]; then
    stamp check
    exit
fi
BLENDER=${BLENDER:-$(ls -d "$HOME"/.local/opt/blender-*/blender 2>/dev/null | tail -1)}
BLENDER=${BLENDER:-blender}
log=$(mktemp)
trap 'rm -f "$log"' EXIT
status=0
"$BLENDER" -b --factory-startup --python-exit-code 1 --python "$ROOT/tools/characters/assemble.py" -- "$@" \
    >"$log" 2>&1 || status=$?
grep -E "^build-characters|Error|Traceback|^characters:" "$log" || true
if [[ $status -ne 0 ]]; then
    echo "build-characters: Blender failed ($status)" >&2
    exit "$status"
fi
# A full build (no sets named) brings the stamp up to date.
if [[ $# -eq 0 ]]; then
    stamp write
fi
