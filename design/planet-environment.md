# Planet environment: review and plan

Review of Tessera's environment after ground cover (task 5.7d), from test round q0132 (commit
95ea5c5, 2026-11-20): the seven biome cover views, the village by day, dusk, night, rain and
fog, walking in the village, the planet from orbit, low flight, peaks, lowlands, the coast and
the far block ground. The developer's judgement: the planet is still far from a complete
environment. Decided 2026-11-20: animals come later; the planet environment comes first,
before digging and placing (5.8).

## 1. What holds up

- World generation: continents, ranges, climate belts, biomes and regions read from orbit and
  in flight.
- The faceted ground near the player: no seams or flicker, size-on-screen detail, streaming
  with dissolves that keeps up on foot and nearly keeps up in low flight at 600 m/s.
- Ground cover: 36 generated species with variants, recipes per biome, clustered stands and
  clearings, collision; within budget (`blocks-perf` 82 fps, q0131).
- Light: sun shadows, AgX tonemapping, lit windows and lanterns at night, manual rain and fog.
- The village by day reads close to the style renders.

## 2. Findings

### 2.1 Nothing moves
Trees, grass, reeds and bushes are static; no leaves fall, no dust blows, no chimney smoke, no
moving water beyond small glints on the sea. A still world reads as dead even when it is full.
The style asks for "wind that moves trees and grass" (visual-style.md §1).

### 2.2 The far landscape is empty
- Ground cover is drawn within about 1–2 km. Beyond it, the terrain tiles show only the
  biome's plain colour: a dense taiga ends at a bare green hill (cover-taiga), and the
  lowlands at 1.2 km above the ground are a featureless green plain (lowlands).
- Forest edges, tree lines and clearings vanish from altitude, so flight shows none of the
  biomes' character.
- Mountains are smooth white and grey shapes without cliffs, ridges, scree or rock bands
  (peaks); the land between ranges is gently rolling everywhere.

### 2.3 Water is a placeholder
- The sea is a flat blue plane with a hard edge on the shore: no shallow-to-deep colour, no
  foam, waves only as small glints (sea-clouds, cover-jungle).
- The world generator computes rivers and lakes (`globe` fields `river`, cover `Lake`,
  `River`), but rivers are not drawn at all and lakes are flat blue cut-outs.

### 2.4 Ground detail close up is thin
- On foot the ground is mostly flat colour with a fine pattern: no leaf litter, pebbles, mud,
  puddles, worn ground under trees or along paths.
- The desert is flat sand with scattered cacti: no dunes, rock pillars, dry riverbeds or
  cracked flats (cover-desert).
- Grass is sparse tufts, never a lawn or meadow.

### 2.5 Weather and atmosphere are fixed
Weather changes only by a switch (`ULTRA_SPACE_WEATHER`, `;`); it doesn't follow the climate or
change over time. No snow, no dust or sand storms, no valley fog at dawn, no haze by biome.

### 2.6 No effects or sound
None of the planned effects exist (engine plumes, landing dust, splashes, chimney smoke, rain
hitting the ground). There is no audio at all.

### 2.7 Life
Outside the village there is no life: no birds, insects, fish or animals; the village's people
and chickens stand still. **Deferred** by the developer (2026-11-20): animals come later, after
the environment.

### 2.8 Bugs
- **Cover glows at night:** in village-night the grass and trees are bright green against a
  dark ground, lit unlike the ground beneath them.
- **Blank tundra captures:** cover-tundra saved four 72-byte screenshots although the run was
  fine (60 fps, no errors).
- **The village's biome:** the land round the medieval village is tropical dry forest, with
  palms and baobab-like trees behind the house.

## 3. Plan: the living environment (before 5.8)

Each step is judged in a test round before the next; animals and small life (birds, insects,
fish) are out of scope.

| # | Step | Contents | Size |
|---|---|---|---|
| E1 | **Fixes** | Cover lit like the ground at night; tundra captures; the village's surroundings temperate (developer to confirm) | small |
| E2 | **Wind** | A planet wind field (direction and strength from the climate's wind bands, gusts travelling across the land); cover sways by height and stiffness per species (grass and leaves most, trunks least) in the vertex shader and its prepass; water ripples and cloud drift follow the same wind | small |
| E3 | **The far landscape** | Far tiles coloured by what grows there (forest canopy, scrub, grass, bare rock), with a canopy texture that fades into the near cover; cheap distant trees out to several km where the cover ends; mountains with rock bands, cliffs and scree from slope and height; more large landforms (outcrops, ridges, terraces) in the terrain function | medium |
| E4 | **Water** | Sea colour by depth, shore foam and breaking lines, low-poly waves moving with the wind; rivers drawn from the world's river field (beds carved in the faceted ground, flowing water surface, banks with reeds) and lakes as water surfaces at their own levels | medium–large |
| E5 | **Ground detail** | Near-ground dressing per biome: leaf litter under trees, pebbles, mud and puddles in wet ground, worn paths, meadow patches; desert dunes, rock pillars, dry riverbeds and cracked salt flats; tundra moss and frost | medium |
| E6 | **Weather and effects** | Weather by climate and changing over time (cloud cover, rain, snow, fog, dust storms; valley fog at dawn); effects: landing dust, engine plumes, splashes, chimney smoke, rain and snow on the ground | medium |
| E7 | **Ambient sound** *(optional, could wait for Batch 6)* | Wind, rain, water, birdsong as ambience, footsteps, the ship's engine | medium |

Then 5.8 (digging and placing) and 5.9 (review), with the planet-wide views of visual-style.md
§3 (low flight over each biome, the coast, snow) joining the review.

## 4. Budget

The frame budget holds throughout: `blocks-perf` stays within a few fps of 82 (q0131), no
frame over 50 ms flying low; every addition has a switch and a quality tier (visual-style.md
§4).
