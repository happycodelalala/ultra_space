#!/usr/bin/env python3
"""Validates content data files against their JSON Schemas.

Mapping: content/data/<kind>s/*.json is checked against content/schemas/<kind>.schema.json
(e.g. data/star_systems/halcyon.json -> schemas/star_system.schema.json), and the recorded
content-director responses content/lore/*.json against schemas/lore.schema.json. Cross-field
rules (parents exist, moons fit their parent, roles fit the tech tier, ...) are checked by the
game's loaders and tests.
"""
import json
import sys
from pathlib import Path

try:
    import jsonschema
except ImportError:
    print("check-content: needs the Python 'jsonschema' package (sudo apt install python3-jsonschema)", file=sys.stderr)
    sys.exit(1)

ROOT = Path(__file__).resolve().parent.parent / "content"


def main() -> int:
    errors = []
    checked = 0
    folders = [(f, f.name[:-1] if f.name.endswith("s") else f.name)
               for f in sorted((ROOT / "data").iterdir()) if f.is_dir()]
    if (ROOT / "lore").is_dir():
        folders.append((ROOT / "lore", "lore"))
    for folder, kind in folders:
        schema_path = ROOT / "schemas" / f"{kind}.schema.json"
        files = sorted(folder.glob("*.json"))
        if not files:
            continue
        if not schema_path.exists():
            errors.append(f"{folder.relative_to(ROOT)}: no schema at {schema_path.relative_to(ROOT)}")
            continue
        schema = json.loads(schema_path.read_text())
        jsonschema.Draft202012Validator.check_schema(schema)
        validator = jsonschema.Draft202012Validator(schema)
        for path in files:
            checked += 1
            try:
                data = json.loads(path.read_text())
            except json.JSONDecodeError as e:
                errors.append(f"{path.relative_to(ROOT)}: invalid JSON: {e}")
                continue
            for err in sorted(validator.iter_errors(data), key=lambda e: list(e.path)):
                where = "/".join(str(p) for p in err.path) or "(root)"
                errors.append(f"{path.relative_to(ROOT)}: {where}: {err.message}")

    if errors:
        print("check-content: content doesn't match its schema:", file=sys.stderr)
        for e in errors:
            print(f"  {e}", file=sys.stderr)
        return 1
    print(f"check-content: ok ({checked} files)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
