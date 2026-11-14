# Design viewer

A browser tool for designing mechs, ships, vehicles, buildings and other objects quickly,
without building the game: it renders the game's own designs and part models, with their
joints and poses.

    tools/viewer.sh                         # http://localhost:8765/ (works from Windows too)
    tools/viewer-shots.sh bastion wings     # review shots to .viewer-shots/, no window
    tools/viewer-shots.sh --part frame.foot

## What it shows

- A design (`content/data/vehicles/<id>.json`) assembled from the part catalog
  (`content/data/parts/*.json`), every part drawn with its model
  (`assets/export/models/<form.model>.glb`) or its placeholder shape, exactly as in the game.
- Joints: a slider per joint within its limits, and the design's named poses. It starts in the
  design's `stand` pose, like the game.
- Overlays: collision boxes, attach nodes, centre of mass, the ground at the lowest point.
- One part on its own, with its collision box and attach nodes (for modelling parts).
- Live reload: when a design, part file or model changes on disk (a generator rewrote it, or
  you edited it), the view updates within a second.

## Getting things into the game

The viewer reads and writes only the game's engine-neutral formats, in the game's folders, so
there is no import step: whatever it shows or saves, the game loads on its next start (and
`tools/export-windows.sh` copies to the Windows build).

| What | Format | Where | The game uses it |
|---|---|---|---|
| A design | design JSON (`content/schemas/vehicle.schema.json`) | `content/data/vehicles/<id>.json` | spawns it (Shift+G cycles designs) |
| A pose | `poses` in the design JSON (joint part -> degrees) | the design file | P cycles poses; `stand` is the start |
| A part model | glTF binary: part axes, metres, Y up; each primitive's material named `armor`, `trim`, `dark` or `glass` | `assets/export/models/<model>.glb` | draws parts whose form names `<model>` |

- **Save pose as…** writes the current joint angles into the design file in the game's layout
  (one part per line, like `design_json`).
- **Export model (.glb)** (part view) writes the shown model in the game's model format.
- Designs and models made by code are owned by their generator: `tests/heavy_frame.rs`,
  `tests/bastion.rs` (`-- --ignored` rewrites the file) and `tools/gen-models.py`. A pose or
  model saved over a generated file is replaced the next time the generator runs; give
  hand-made models their own names (and point a part's `form.model` at them) or move the
  pose into the generator.

## Pieces

- `articulation.mjs`: designs, rigid groups and joint poses, ported from `sim_vehicle`
  (`design.rs`, `joints.rs`); tested against the game's numbers (`node --test`, run by CI).
- `viewer.mjs`, `index.html`: the three.js page (three.js from a CDN, pinned).
- `server.py`: serves the page and the data read-only (localhost only) and saves to the three
  places above.
