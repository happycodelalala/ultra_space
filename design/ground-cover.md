# Ground cover: vegetation, rocks and scattered things (Batch 5, task 5.5)

Decided 2026-11-15. The planet surface (Batch 2) has shape and biome colours but nothing
growing or lying on it, so the land reads as empty from the ground and from low flight however
dense the settlements are. Ground cover fills it: trees, bushes, grass, rocks, logs, and, where
history left them, wrecks and debris. Planned for Batch 2 ("Vegetation, rocks and scattered
props", planet-surface.md §7); planned as a Batch 4c, now task 5.5 of the refined world
(Batch 5), in the low-poly style of visual-style.md. Batch 7's gathering will take the same
trees and rocks.

## 1. Decisions

- **Deterministic from the terrain:** what grows where is a function of the tile, the biome
  fields (temperature, moisture, height, slope), the climate's blight and the body's seed. The
  same place always has the same trees, so untouched cover is never saved (technical overview
  §14, the "procedural, untouched" tier). Taking or destroying one makes it a delta (Batch 7).
- **Placed on workers, per terrain tile:** when a tile at the detail levels is made, its cover
  is scattered with it on the worker threads, from the same terrain function the collision
  uses, so trees stand on the ground the player stands on.
- **Instanced, never one entity per thing:** a tile's cover of one kind is one draw (the
  model's mesh and a buffer of per-instance transforms and tints). Batch 4b showed that
  thousands of entities moved every frame by the camera-relative placement cost the renderer
  10–25 ms; a forest is millions of things. The instance buffers are built on workers and
  uploaded once; only the tile's root moves.
- **Levels of detail by distance:** models near; simplified models or camera-facing cards
  further out; beyond that the cover is the terrain's colour (the forest biome already reads
  dark green from orbit). Each level's reach is a budget, not a constant: tuned so a frame
  stays within its time on the test machine.
- **Collision only near the player:** tree trunks and big rocks get static colliders within
  about 100 m of the player and vehicles (made from the same instances); small things have
  none.
- **Settlements clear it:** foundations, streets, fields and roads (living-planet.md §8d) have
  no wild cover; their own planted things (orchards, gardens, hedges) come from the layout.

## 2. What grows where

Each biome has a cover recipe (data, `content/data/ground_cover/<biome>.json` with a schema):
kinds of thing, each with a density per hectare, size range, clustering (forests come in
stands with clearings; rocks in scree below slopes), and limits (slope, height, moisture).

| Biome | Cover |
|---|---|
| Forest | Dense trees in stands and clearings, undergrowth, fallen logs, mossy rocks |
| Grassland | Grass, scattered trees and copses, bushes along water, a few boulders |
| Desert | Sparse shrubs, rocks and boulders, dunes left to the terrain |
| Tundra | Low shrubs, lichen-coloured rocks, a few stunted trees |
| Snow and ice, bare rock | Rocks and scree, nothing growing |
| Beach | Driftwood, rocks, grass at the dune line |
| Blighted (hazard zones) | Dead trees and stumps, bare ground; the hot zones nothing at all |

Density follows the fields smoothly (no hard borders between biomes); a local noise breaks it
into stands and clearings so the land has places, not uniform fill.

## 3. Levels and budgets (first targets)

| Level | Reach | Drawn as | Budget |
|---|---|---|---|
| Near | ~150 m | Full models, shadows, wind sway on trees | ~20,000 instances |
| Middle | ~150–800 m | Simplified models, no shadows for small things | ~100,000 instances |
| Far | ~0.8–3 km | Trees only, as cards or very low models | ~100,000 instances |
| Beyond | — | The terrain's biome colour | — |

Grass is a separate, cheaper system: short cards around the camera only (tens of metres),
faded out with distance, generated each frame from the terrain on the GPU or reused by tile.

## 4. Assets

Stand-ins from CC0 packs (Quaternius nature packs, Kenney's nature kit), downloaded and
converted like the building kits, until the design track makes the game's own. Each kind has
LOD variants (or a card made by rendering the model).

## 5. Tasks (Batch 5)

Task 5.5 in development plan §7.7 (instancing, recipes, assets, levels and grass, collision
near the player, condensed into one task).
