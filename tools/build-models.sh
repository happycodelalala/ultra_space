#!/usr/bin/env bash
# Builds the part models with Blender (headless) from tools/blender/*.py.
#   tools/build-models.sh                     # every model, and the stamp CI checks
#   tools/build-models.sh NAME... [--preview DIR]   # some models (and studio renders)
#   tools/build-models.sh --check             # fails if the models are older than their inputs
# Blender: $BLENDER, else ~/.local/opt/blender-*/blender, else blender on the PATH.
set -euo pipefail
ROOT="$(git -C "$(dirname "$0")" rev-parse --show-toplevel)"
if [[ "${1:-}" == "--check" ]]; then
    exec python3 - "$ROOT" <<'PY'
import hashlib, sys
from pathlib import Path
root = Path(sys.argv[1])
here = root / "tools" / "blender"
h = hashlib.sha256()
files = sorted(here.glob("*.py")) + [root / "content/data/part_materials.json"] + sorted((root / "content/data/parts").glob("*.json"))
for f in files:
    h.update(f.name.encode()); h.update(f.read_bytes())
stamp = root / "assets/export/models/models.stamp"
if not stamp.exists() or stamp.read_text().strip() != h.hexdigest():
    print("build-models: models out of date (run tools/build-models.sh)")
    sys.exit(1)
print("build-models: ok")
PY
fi
BLENDER=${BLENDER:-$(ls -d "$HOME"/.local/opt/blender-*/blender 2>/dev/null | tail -1)}
BLENDER=${BLENDER:-blender}
exec "$BLENDER" -b --factory-startup --python-exit-code 1 --python "$ROOT/tools/blender/build.py" -- "$@" 2>&1 \
    | grep -v "^Fra:\|^$\|^Blender quit\|^Read prefs\|^Blender 4\|Saved:\|^Time:"
