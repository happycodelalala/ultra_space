# Visual style: low-poly shapes, realistic light (Batch 5)

Decided 2026-11-15 (developer). The world still reads as a placeholder: bare land, flat biome
colours, a flat sea, plain light and no weather. Realism at Starfield's level, or even No Man's
Sky's, is out of reach for a game of this scale made by one developer. The target is the
**low-poly look of Dyson Sphere Program** and similar space sandboxes, made appealing by
**realistic light, atmosphere, weather and effects**, with room to raise the quality later.

## References (the developer's, 2026-11-15)

| Reference | What we take from it |
|---|---|
| **No Man's Sky, after the Worlds Part 1 update** | The sky, weather and light: volumetric clouds with real form and shadow, storms with lightning, rain and fog that change the mood of a whole planet, water that catches the sky, light that shifts with the hour. The bar for the "realistic" half |
| **Stonehearth** | The ground and everything on it: chunky low-poly shapes with toy-like proportions, warm saturated colours, soft light and long shadows, a cosy hand-made feel up close in villages, forests and fields. The bar for the "low-poly" half |
| **Dyson Sphere Program** | Low-poly that holds up at scale: a whole planet of simple shapes read clearly from orbit, effects and lighting doing the work |

In one line: **No Man's Sky skies and weather over a Stonehearth world.** Volumetric clouds and
storms are the costliest part, so they come in quality tiers (§4); they extend the clouds the
atmosphere shader already draws rather than adding a second system. How blocky the shapes go
(Stonehearth's near-voxel trees against Dyson Sphere Program's faceted forms) is decided with
the first ground cover and kit models (tasks 5.1 and 5.5) and set down here.

## 1. Principles

- **Shapes: low-poly and faceted.** Models are simple, readable at a distance, with their
  detail in the silhouette (a roofline, a chimney, a crane) rather than in texture. This keeps
  thousands of trees, rocks and buildings affordable on a whole planet, and lets the design
  track make the game's own models at the same level.
- **Surfaces: flat colour from the palette.** Colour comes from the shared palette slots of the
  model format (part materials, `content/data/part_materials.json`), with roughness and
  metalness to tell stone from metal from paint; little or no texture detail. Colours are
  chosen for harmony in the light (muted, natural ground; saturated accents for people's
  colours, lights and markings), not as the raw colours of a stand-in pack.
- **Light: as realistic as we can afford.** The style's appeal is in the light: a sun that
  casts shadows across planet scales; ambient occlusion where shapes meet; exposure that adapts
  between sunlit ground and the dark of space; filmic tonemapping; bloom on lights, engines and
  the sun; the sky's colour carried into the shadows; aerial haze that softens distance; ground
  fog in valleys at dawn.
- **Time of day and weather change the whole picture.** Warm, long light at dusk; cool, dark
  nights lit by towns and the stars; overcast days flat and grey; rain, snow and dust; wind
  that moves trees and grass.
- **Effects make things feel physical.** Engine plumes and heat shimmer, dust kicked up when
  landing, splashes, chimney smoke, sparks; each small and cheap, all together convincing.
- **Upgradeable by design.** Quality settings in tiers (shadows, ambient occlusion, effects,
  ground cover density, draw distances); effects as separate shader modules; assets kept to the
  model format (palette slots, metres, part axes), so models, materials and shaders can each be
  replaced later without touching the game's data.

## 2. The world, piece by piece

| Piece | Now | In the style |
|---|---|---|
| Terrain | Flat biome colours, hard rock and snow bands, angular distant mountains, staircase coasts | Faceted low-poly ground; biomes blended, rock on slopes and snow by height and slope with soft edges; smooth coasts; distant mountains softened by haze |
| Water | A flat plane | Depth colour (shallows to deep), foam along the shore, gentle low-poly waves, the sky reflected |
| Ground cover | Bare land | Low-poly trees, bushes, grass and rocks by biome (ground-cover.md), moving in the wind |
| Sky and atmosphere | Single scattering, clouds in the atmosphere shader | The same, tuned: cloud cover seen from the ground, haze and fog near the ground, sunsets that colour the land |
| Settlements and people | Kenney and Quaternius stand-ins | The game's own kits and characters on the design track, in this style; until then the stand-ins under the new light |
| Effects | A few (lights at night, engine glow) | Plumes, dust, splashes, smoke, precipitation |

## 3. Review views

The finish line of Batch 5 is a fixed set of **review views**, a test round
(`tools/test-scripts/`) whose screenshots are judged against this guide:

- from orbit by day and by night;
- low flight over forest, plains, desert, coast, snow and the Kesh waste;
- on foot in a forest, on a beach, in a village street, in a field;
- each at day, dusk and night, and in rain, snow and fog where the climate has them;
- the ship landing (dust) and taking off (plume).

Anything that looks off outside these views is a known issue for a later batch, not a new task.

## 4. Budgets

The frame budget is unchanged: no frame over 50 ms flying low at 300 m/s or landing, on the test
machine at its default quality. Every effect has an off switch and a quality tier; ground cover
densities and draw distances are budgets (ground-cover.md §3), not constants.

## 5. Tasks

Development plan §7.7 (Batch 5).
