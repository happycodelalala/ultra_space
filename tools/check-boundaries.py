#!/usr/bin/env python3
"""Enforces the engine boundary in the Rust workspace.

Only front-end crates (game_*) may depend on an engine or renderer. Every other workspace
crate may use only the standalone Bevy crates in ALLOWED_BEVY as direct dependencies, and
must not pull in windowing or GPU crates even transitively.
"""
import json
import subprocess
import sys
from pathlib import Path

WORKSPACE = Path(__file__).resolve().parent.parent / "impl" / "rust"
FRONT_END_PREFIX = "game_"
ALLOWED_BEVY = {"bevy_ecs", "bevy_tasks"}
FORBIDDEN_ANYWHERE = {"bevy", "bevy_render", "bevy_winit", "bevy_window", "wgpu", "winit"}


def main() -> int:
    result = subprocess.run(
        ["cargo", "metadata", "--format-version", "1"],
        cwd=WORKSPACE, capture_output=True, text=True,
    )
    if result.returncode != 0:
        print("check-boundaries: cargo metadata failed:", file=sys.stderr)
        print(result.stderr, file=sys.stderr)
        return 1
    meta = json.loads(result.stdout)
    packages = {p["id"]: p for p in meta["packages"]}
    nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
    errors = []

    for member_id in meta["workspace_members"]:
        pkg = packages[member_id]
        name = pkg["name"]
        if name.startswith(FRONT_END_PREFIX):
            continue

        # Direct dependencies: only allowlisted bevy_* crates.
        for dep in pkg["dependencies"]:
            dep_name = dep["name"]
            if dep_name.startswith("bevy") and dep_name not in ALLOWED_BEVY:
                errors.append(f"{name}: direct dependency on '{dep_name}' (allowed: {sorted(ALLOWED_BEVY)})")

        # Transitive (normal) dependencies: no engine, windowing or GPU crates.
        seen, stack = set(), [member_id]
        while stack:
            node = nodes[stack.pop()]
            for dep in node["deps"]:
                if not any(k["kind"] in (None, "normal") for k in dep["dep_kinds"]):
                    continue
                if dep["pkg"] in seen:
                    continue
                seen.add(dep["pkg"])
                dep_name = packages[dep["pkg"]]["name"]
                if dep_name in FORBIDDEN_ANYWHERE:
                    errors.append(f"{name}: depends (transitively) on '{dep_name}'")
                stack.append(dep["pkg"])

    if errors:
        print("check-boundaries: engine boundary violated:", file=sys.stderr)
        for e in errors:
            print(f"  {e}", file=sys.stderr)
        return 1
    print("check-boundaries: ok")
    return 0


if __name__ == "__main__":
    sys.exit(main())
