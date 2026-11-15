#!/usr/bin/env bash
# Exports the block models and the demo village from tools/blocks/ (headless Blender) into
# assets/export/blocks/ (block-world.md section 10).
#   tools/build-blocks.sh           # export, and write the stamp CI checks
#   tools/build-blocks.sh --check   # fails if the export is older than its scripts
# Blender: $BLENDER, else ~/.local/opt/blender-*/blender, else blender on the PATH.
set -euo pipefail
ROOT="$(git -C "$(dirname "$0")" rev-parse --show-toplevel)"
stamp() {
    python3 - "$ROOT" <<'PY'
import hashlib, sys
from pathlib import Path
root = Path(sys.argv[1])
h = hashlib.sha256()
for f in sorted((root / "tools" / "blocks").glob("*.py")):
    h.update(f.name.encode()); h.update(f.read_bytes())
print(h.hexdigest())
PY
}
OUT="$ROOT/assets/export/blocks"
if [[ "${1:-}" == "--check" ]]; then
    if [[ -f "$OUT/blocks.stamp" && "$(cat "$OUT/blocks.stamp")" == "$(stamp)" ]]; then
        echo "build-blocks: ok"; exit 0
    fi
    echo "build-blocks: block models out of date (run tools/build-blocks.sh)"; exit 1
fi
BLENDER=${BLENDER:-$(ls -d "$HOME"/.local/opt/blender-*/blender 2>/dev/null | tail -1)}
BLENDER=${BLENDER:-blender}
BLOCKS_EXPORT="$OUT" "$BLENDER" -b --factory-startup --python-exit-code 1 --python "$ROOT/tools/blocks/scene.py" -- day wide /dev/null 2>&1 \
    | grep -E "EXPORTED|Error|Traceback|^  " || true
ls "$OUT"/village.json >/dev/null
stamp > "$OUT/blocks.stamp"
