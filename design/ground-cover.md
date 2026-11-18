# Ground cover: the landscape's vegetation, rocks and scattered things (Batch 5, task 5.7d)

Rewritten 2026-11-18 for the faceted ground (block-world.md, the density field) and the world
generated as a simulation (planet-geography.md §1b). The planet's shape, climate and biomes are
done; outside the demo village the land is bare. Ground cover fills it: trees, shrubs, grass,
flowers, rocks, logs and the small things of each landscape, everywhere on the planet, from the
same data as the land, with enough procedural variety that no two stretches of land look alike.
(History: planned for Batch 2, then a Batch 4c, then after the demo village; the block-style
nature of the village, `tools/blocks/models.py`, is its model.)

## 1. Decisions

- **Deterministic from the land:** what grows where is a function of the place (face-grid
  metres), the world's fields there (temperature, rain, height, slope, distance to the sea), the
  biome and the body's seed. The same place always has the same tree, so untouched cover is never
  saved (technical overview §14, "procedural, untouched"). Taking or destroying one makes it an
  edit (digging, 5.8; gathering, Batch 7).
- **Generated in the game, not authored one by one:** every kind of thing (a *species*) is a
  generator with parameter ranges in data (`content/data/ground_cover/standard.json`, with a
  schema). At load the game builds a number of *variants* of each species from seeds (a dozen
  trees of each kind, more of rocks), as block models in the village's style (built from voxels,
  meshed with the block mesher and its slopes). Generators are Rust ports of the style scene's
  shapes (`tools/blocks/models.py`: lobed crowns, tiered conifers, rounded rocks) plus new ones.
- **Block models on the faceted ground:** the living and lying things stay block art (the
  developer's style of 2026-11-15); only the ground is faceted. Things stand at any angle about
  the vertical (the ground has no grid to align to), upright for things that grow, tilted to the
  slope and partly sunk for rocks and logs.
- **Merged per ground chunk:** a ground chunk's cover is one mesh in the chunk's own frame, built
  on the workers with the chunk and shown and hidden with it (never one entity per thing: a
  forest is millions of things; Batch 4b measured 10–25 ms for thousands of moving entities).
  Colours are final per vertex (the palette colour times the instance's tint), so per-instance
  variety costs nothing to draw.
- **Size on screen decides what is drawn:** a chunk of level L (columns 2^L cells wide) only
  carries the species tall enough to cover a few pixels at the distance that level is drawn at
  (height ≥ 2^L × 0.25 m: grass and flowers near, shrubs to the middle, trees far); far levels
  draw coarser variants (the voxels merged 2× and 4×) and thin dense stands (keeping the tallest
  share, crowns a little larger), so the triangle count stays within budget. Beyond the cover's
  last level, the tiles' biome colour carries the forest (dark green from orbit already).
- **The same tree at every level:** positions come from a grid fixed to the ground, not to the
  chunk, so a tree drawn far away is the same tree, in the same place, when the player gets near.
- **Collision only near bodies:** tree trunks, big rocks, logs and cacti get collision boxes
  within ~40 m of moving bodies (walkers bump into trunks; ships land between trees); grass,
  flowers, ferns and small shrubs have none.
- **Places clear it:** no wild cover in the village's square, on its paths and pads, in the sea
  or under water, or where the ground is dug. Future settlements' foundations, streets and fields
  clear it the same way (living-planet.md §8d).
- **No palette change:** new hues (cactus, acacia, dry grass, lichen) are existing palette
  colours tinted per species and per instance, so the design track's palette (the handoff) is
  untouched.

## 2. Procedural variety: the layers of randomness

Repetition is the enemy: a forest of one tree copied a thousand times reads as wallpaper. Six
layers, each from its own seed, multiply into practically unique stands.

1. **Variants:** each species has N variants (trees 12, shrubs 10, rocks 16, small things 8),
   each built by the generator from a seed: different heights, trunk lean and bend, branch count
   and angles, crown lobe count, size and arrangement, tier counts, lump shapes, colour noise.
2. **Instance transform:** a random turn about the vertical (any angle), scale within the
   species' range (±15–25 %), for rocks and logs a tilt to the slope and a random sink.
3. **Instance tint:** hue and brightness shifted by a smooth noise field (neighbours share a tint,
   so a stand reads as one population) plus a small per-instance jitter; drier and colder places
   push leaves yellower and duller, wet places greener; rocks take the local stone tint.
4. **Species mix that varies over the land:** each biome's recipe has several species with base
   shares; a low-frequency noise per species reshuffles the shares across the land, so one
   stand is mostly birch, the next mostly oak, the next mixed.
5. **Clustering and spacing:** each species is placed on a jittered grid of its own spacing
   (Poisson-like: never two trunks on top of each other) and kept with a probability from its
   density times a clustering noise (stands and clearings for trees, patches for flowers, scree
   below steep ground for rocks, drifts of grass), so density varies from dense groves to open
   ground on several scales.
6. **Ecology:** rules tie species to their places and to each other: the tree line (warmth), the
   shore (height above the sea), slope (rock on steep ground, no trees on cliffs), shade
   (ferns, mushrooms and logs under a canopy, flowers in clearings), edges (shrubs and young
   trees at forest edges, big old trees inside), age (sizes skewed small at edges, large inside).

## 3. What the landscape has: the species

Thirty-two species in eight groups. Heights are typical (the variants and instances spread
them). "Gen" is the generator.

| Group | Species | Gen | Height | Where | Look |
|---|---|---|---|---|---|
| **Broadleaf trees** | Oak | broadleaf | 5–9 m | temperate grass, forest | thick trunk with root flare, 2–3 limbs, 3–6 cushion lobes, rounded crown |
| | Beech / lime | broadleaf | 7–12 m | temperate forest | taller straighter trunk, high oval crown of stacked lobes |
| | Birch | birch | 5–9 m | forest edges, taiga, cool grass | slim white trunk with dark marks, narrow light-green lobes |
| | Willow | willow | 5–8 m | wet ground, shores | short trunk, wide crown with hanging curtains |
| | Tropical broadleaf | broadleaf | 8–14 m | seasonal, jungle | wide flat crown, light green, buttress flare |
| | Jungle giant | giant | 18–30 m | jungle (rare) | tall straight trunk with buttress roots, broad emergent crown |
| | Acacia | acacia | 4–7 m | savanna | forked leaning trunk, flat umbrella crown |
| | Baobab | baobab | 6–10 m | savanna (rare) | fat bottle trunk, small stubby crown |
| **Conifers and palms** | Spruce | conifer | 8–16 m | taiga, cold forest, mountains | tiered cone of 4–8 tiers, narrow, dark |
| | Pine | pine | 8–14 m | taiga, dry forest | tall bare trunk, high rounded tiers |
| | Krummholz | krummholz | 1–3 m | tree line | stunted, bent away from the wind, flat-topped |
| | Palm | palm | 6–12 m | warm coasts, jungle | curved ringed trunk, 5–8 drooping fronds |
| **Dead wood** | Dead tree / snag | dead | 4–10 m | dry edges, tundra edges, burnt | bare grey trunk, a few broken limbs |
| | Fallen log | log | 0.6–1 m | forest floor | a lying trunk with stub branches and moss |
| | Stump | stump | 0.4–0.8 m | forest | cut or broken, roots |
| | Driftwood | driftwood | 0.3–0.6 m | beaches | bleached bent branch, lying |
| **Shrubs** | Shrub | shrub | 0.6–1.5 m | grass, forest edges | two or three leafy balls |
| | Berry bush | shrub | 0.8–1.4 m | forest edges, grass | shrub with berries of one colour |
| | Fern | fern | 0.4–0.9 m | forest floor, jungle, rainforest | fan of arched fronds from one point |
| | Heather / dwarf shrub | dwarf | 0.2–0.5 m | tundra, moor, cold heights | low mat, purple-brown or berry-red |
| | Sagebrush | shrub (grey) | 0.5–1 m | steppe, dry grass, cold desert | grey-green rounded tufts |
| | Bamboo | bamboo | 4–8 m | seasonal, jungle | a clump of segmented green canes |
| | Reeds | reeds | 1–2 m | shores, wet lowland | dense upright stems with brown heads |
| **Desert plants** | Column cactus | cactus | 2–6 m | hot desert | column with 0–4 upturned arms |
| | Pad cactus | padcactus | 0.6–1.2 m | desert, dry savanna | stacked flat pads, a few flowers |
| **Ground plants** | Grass tuft | tuft | 0.2–0.5 m | all green land | a tuft of blades, green |
| | Dry grass | tuft (straw) | 0.3–0.6 m | steppe, savanna, desert edges | straw-coloured tuft |
| | Meadow flowers | flowers | 0.1–0.3 m | clearings, grassland patches | a few stems with heads of one colour |
| | Mushrooms | mushrooms | 0.1–0.3 m | forest floor, near logs | a ring or clump of capped stems |
| **Rocks** | Pebbles | pebbles | 0.1–0.2 m | everywhere a little, beaches more | a scatter of small stones |
| | Rock | rock | 0.4–1 m | everywhere, more on slopes | rounded lump, moss on top in wet places |
| | Boulder | rock | 1–3 m | slopes, tundra (erratics), kopjes | big rounded rock, part sunk |
| | Outcrop | outcrop | 3–8 m | steep ground, ridges, savanna kopjes (rare) | a stack of blocky slabs, a landmark |
| | Ice chunk | rock (ice) | 0.5–2 m | ice and snow | white-blue angular block |
| **Other** | Termite mound | mound | 1–3 m | savanna | tall clay cone with ridges |
| | Salt crust | crust | 0.1–0.3 m | salt flats | white polygon crust plates |

## 4. What grows where: the recipes

A recipe per biome (`Cover` of the world, globe/generate.rs), each a list of species with a
density (per hectare), a share, a clustering scale and limits; plus three recipes that apply on
top of any biome by place: **shore** (0–3 m above the sea), **steep** (slope over ~35°) and
**high** (above the tree line). Densities are first targets, tuned in the review rounds.

| Biome | Trees (per ha) | Shrubs and plants | Ground and rocks |
|---|---|---|---|
| Grass (temperate grassland) | oak, birch in copses (8, clustered) | shrub, berry bush along copses (20) | grass tufts (dense), meadow flowers in patches, rocks (few) |
| Forest (temperate) | oak, beech, birch stands (300, stands and clearings) | shrubs at edges, ferns under canopy (200) | logs, stumps, mushrooms, mossy rocks; flowers in clearings |
| Seasonal (tropical dry forest) | tropical broadleaf, palm (150) | bamboo clumps, shrubs (60) | dry grass, rocks |
| Rainforest (temperate) | spruce, beech (350) | ferns (dense), shrubs | logs, mossy rocks, mushrooms |
| Jungle | tropical broadleaf (300), jungle giant (rare), palm | ferns, bamboo, shrubs (dense) | logs, mossy rocks |
| Savanna | acacia (6, spread), baobab (rare) | pad cactus at the dry edge, sagebrush | dry grass (dense), termite mounds, kopje boulders |
| Steppe | dead tree (rare) | sagebrush (40) | dry grass (dense), rocks |
| Desert (hot) | — | column cactus (3), pad cactus (5), sagebrush (5) | pebbles, rocks, boulders |
| Cold desert | — | sagebrush, dwarf shrub (sparse) | pebbles, rocks, boulders |
| Taiga | spruce, pine, birch stands (250) | dwarf shrub, berry bush | logs, mossy rocks, mushrooms |
| Tundra | krummholz near the tree line, dead tree (rare) | dwarf shrub (dense) | rocks, boulders (erratics), pebbles |
| Snow, ice | — | — | rocks poking through, ice chunks |
| Salt flat | — | — | salt crust |
| **Shore** (any warm or temperate) | palms on warm coasts, willow on wet coasts | reeds (wet), grass | driftwood, pebbles, rocks |
| **Steep** (any) | none over ~35° | krummholz on cold steep ground | outcrops, boulders, rocks (scree) |
| **High** (above the tree line, not ice) | none | dwarf shrub | rocks, boulders, alpine flowers |

Biome borders aren't hard lines: the world's `ground_cover` already dithers them by noise, and
each recipe's densities fade with the fields (a forest thins toward the dry side before it turns
to savanna).

## 5. Levels, budgets and drawing

| Ground level | Chunk side | Drawn | Voxels | Shadows |
|---|---|---|---|---|
| 0 | 32 m | everything (≥ 0.25 m) | full | yes |
| 1 | 64 m | ≥ 0.5 m (no grass, flowers, pebbles) | full | yes |
| 2 | 128 m | ≥ 1 m (shrubs, trees, rocks, logs) | merged 2× | no |
| 3 | 256 m | ≥ 2 m (trees, boulders, outcrops, cacti), dense stands thinned to ~½ | merged 4× | no |
| 4 | 512 m | ≥ 4 m (trees, outcrops), dense stands thinned to ~¼ | merged 4× | no |

First budget: about 6 M triangles of cover in view at 2560 × 1552, measured in `blocks-perf`;
thinning and the reach of the last level are the knobs (`ULTRA_SPACE_COVER_DENSITY` scales
densities for testing; `ULTRA_SPACE_COVER=0` turns cover off). Grass and flowers near the
camera are the same merged meshes (no separate grass system for now); wind sway later.

## 6. Data

`content/data/ground_cover/standard.json` (schema `ground_cover.schema.json`):

- `species`: id, generator, block size (m), height range (m), width range (m), scale spread,
  variants, colours (palette names by role: wood, leaf, accent), tint (hue and value spread),
  collider (none, trunk, box), whether it tilts to the slope and how far it sinks.
- `recipes`: per biome and for shore, steep and high: entries of species, density (per ha),
  spacing (m), clustering (scale in m and strength), share-noise scale, and limits (slope,
  height above sea, warmth).

## 7. Task 5.7d (this batch)

1. The data and schema; the generators and variants (with tests that variants differ, stay in
   their height ranges and are deterministic).
2. Placement: candidates on fixed grids, recipes, ecology and clearing rules (tests: the same
   instances for the same place at every level, none in the sea or the village, densities per
   biome in range).
3. Merged chunk meshes with tints and voxel LOD; streamed with the ground chunks in the game.
4. Collision boxes for trunks, rocks, logs and cacti near bodies.
5. A round: village, Rook Vale, a forest, a taiga, a desert, a savanna and a tundra view, a
   coast, and `blocks-perf` alone at 2560 × 1552.

Later: wind sway, seasons (leaf colour, snow), rivers and lakes drawn with their banks' reeds and
willows, falling leaves and other effects, gathering (Batch 7), creatures (Batch 7).
