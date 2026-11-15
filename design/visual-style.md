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
atmosphere shader already draws rather than adding a second system.

**The shapes are blocks** (decided 2026-11-15 after comparison renders): voxel art in the
Stonehearth manner, with a standard set of shaped blocks (block, 45-degree wedge, outer and
valley corners) so roofs, hulls and hillsides are clean slopes rather than staircases, and block
sizes that double from 3 cm to 8 m, chosen per object by its size and role. The world itself is
made of them and can be dug, built and destroyed: design/block-world.md. Faceted low-poly in the
manner of Dyson Sphere Program was the alternative; mixing the two was rejected because two shape
languages side by side always show the seam.

## 1. Principles

- **Shapes: blocks, chunky and readable.** Models are block art with toy-like proportions,
  their detail in the silhouette (a roofline, a chimney, a crane) and in finer blocks where the
  eye goes (windows, doors, faces) rather than in texture. Block size follows an object's size
  and role, about 10 to 30 blocks across: 25 cm for building structure, terrain, trees and rocks;
  6.25 cm for fixtures and people; 3 cm for small things; metres for ship hulls and stations.
  One object uses only about three neighbouring sizes, and every slope is one of the standard
  shapes (block-world.md §3–4). Low-poly does not mean simple: a house has its timber frame,
  stones, shutters, flower boxes and lantern.
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
| Terrain | Flat biome colours, hard rock and snow bands, angular distant mountains, staircase coasts | 25 cm blocks with slopes between terraces (block-world.md), grass on top, soil and rock in every cut; biomes blended, rock on slopes and snow by height; distant land the same blocks, softened by haze |
| Water | A flat plane | Depth colour (shallows to deep), foam along the shore, gentle low-poly waves, the sky reflected |
| Ground cover | Bare land | Block trees, bushes, grass and rocks by biome (ground-cover.md), moving in the wind |
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
