#!/usr/bin/env bash
# Builds the building kits (content/data/building_kits/*.json) from the third-party pieces in
# assets/third_party/ into assets/export/buildings/<kit>/ (one .glb per building and kit.json
# with footprints), in headless Blender.
#   tools/build-kits.sh                         # every kit, and the stamp CI checks
#   tools/build-kits.sh [kit ids...] [--preview DIR]
#   tools/build-kits.sh --check                 # fails if the kits are older than their inputs
# Blender: $BLENDER, else ~/.local/opt/blender-*/blender, else blender on the PATH.
set -euo pipefail
ROOT="$(git -C "$(dirname "$0")" rev-parse --show-toplevel)"

stamp() {
    python3 - "$ROOT" "$1" <<'PY'
import hashlib, sys
from pathlib import Path
root, mode = Path(sys.argv[1]), sys.argv[2]
h = hashlib.sha256()
for f in sorted((root / "tools/kits").glob("*.py")) + sorted((root / "content/data/building_kits").glob("*.json")):
    h.update(f.name.encode()); h.update(f.read_bytes())
stamp = root / "assets/export/buildings/kits.stamp"
if mode == "write":
    stamp.write_text(h.hexdigest() + "\n")
elif not stamp.exists() or stamp.read_text().strip() != h.hexdigest():
    print("build-kits: building kits out of date (run tools/build-kits.sh)")
    sys.exit(1)
else:
    print("build-kits: ok")
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
"$BLENDER" -b --factory-startup --python-exit-code 1 --python "$ROOT/tools/kits/assemble.py" -- "$@" \
    >"$log" 2>&1 || status=$?
grep -E "^build-kits|Error|Traceback|assemble:" "$log" || true
if [[ $status -ne 0 ]]; then
    echo "build-kits: Blender failed ($status)" >&2
    exit "$status"
fi
# A full build (no kits named) brings the stamp up to date.
if [[ $# -eq 0 ]]; then
    stamp write
fi
