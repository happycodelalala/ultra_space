# Planet geography: land, climate, water and clouds on a planet of plains

Drafted 2026-11-17 for the developer's review, after the first arcade Tessera (development plan
5.7). The developer judged the land and climate incoherent and asked for a systematic overhaul,
and for cotton clouds instead of one thin deck. Decided so far: **the planet decides, regions name
what emerges** (not hand-assigned types); **cotton clouds as puffy meshes**; a design and mock
renders before code. Geometry and blocks are in block-world.md §5.1; this document is what the
ground and sky are like on that geometry.

## 1. What was wrong

| Problem | Why it looked wrong |
|---|---|
| Geography followed the plains | Seas were hexagons, coastlines straight creases, a "mountain region" a 69 km blob of peaks |
| Region types dealt at random | Hot wastes beside ice caps; nothing caused any climate |
| Water ignored climate | Anything below sea level was water, so desert canyons held lakes |
| Mountains were noise, not ranges | No chains, passes or foothills |
| Small things | Flat pentagon ice domes, dull grassland, rock judged against the wrong "up", one flat cloud deck |

## 2. Principles

- **Plains are geometry and identity, not geography.** A plain is a flat piece of ground with a
  name and a region panel; land, sea, ranges and climate are planet-wide fields that flow across
  creases. A region's type and name are read off what is on it.
- **Causes, then effects.** Tectonics makes ranges, rifts and trenches; sun and height make
  warmth; sea, wind and ranges make wetness; warmth and wetness make biomes, ice, lakes and
  clouds. Each layer reads only the ones before it, so changing a cause changes everything
  downstream consistently.
- **Arcade exaggeration, placed sensibly.** Ranges to ~12 km, trenches to ~6 km, cliffs and
  canyons, but where geology would put them, so the world reads at a glance.
- **Deterministic and cheap per point.** Every layer is a function of position (plus a few
  per-plain or per-crease values computed once at load), so tiles, collision, maps and clouds
  ask the same function (planet-surface.md's "one function").

## 3. Layers

### 3.1 Plates and creases (tectonics)

- **Each plain is a plate**, seeded as **continental** (about 55 %) or **oceanic**, with a drift:
  a random direction and speed in its plane.
- **Each crease** gets a character from the two plates' relative drift across it:

| Drift across the crease | Both continental | Continental and oceanic | Both oceanic |
|---|---|---|---|
| Converging | **Mountain range** along the crease, peaks to ~12 km, foothills fading over ~15 km each side | **Coastal range** on the land side, **trench** (~6 km) offshore | **Island arc**: a chain of volcanic islands |
| Diverging | **Rift valley**: a long valley floor, cliffs either side, lakes where wet | Wide continental shelf, gentle coast | **Mid-ocean ridge**: an undersea crest |
| Sliding past | Low hills and a fault line (the crease's fault rock) | Straight cliffy coast | Nothing much |

- Profiles are functions of the distance from the crease (and of position along it, so ranges
  have passes and high massifs): ridged noise inside a range envelope, a cliff step for rifts.
- The fault-line seam blocks of block-world.md are this: the crease is where the plates meet.

### 3.2 Continents and seas

- **Base level** per plate: continental about +300 m, oceanic about −4 km, blended across creases
  (except where a range, rift or trench profile takes over).
- **A planet-wide continent field** (noise over the polyhedron's surface, coherent across
  creases) moves coastlines off the creases: continental plates get bays and inland seas,
  oceanic plates get islands and shallow banks, so coasts are irregular and seas span parts of
  several plains.
- **Sea level is each plain's plane** (block-world.md §5.1): seas are flat, and meet at creases.

### 3.3 Relief

- **Hills** by a roughness field (planet-wide, higher on old continental interiors and near
  ranges), valleys carved where a channel noise crosses zero; **mesas and canyons** where dry
  and high (a terraced profile); **plateaus** on continental plates far from creases.
- All relief is measured along each plain's normal (elevation), as now.

### 3.4 Climate

- **Warmth:** from latitude (the sun's angle; a warm belt at the equator, cold poles) minus the
  lapse with elevation; no seasons for now (the axial tilt can add them later).
- **Wind:** three bands per hemisphere like Earth's (trade winds toward the west near the
  equator, westerlies at mid latitudes, polar easterlies), turned into each plain's plane.
- **Wetness:** high over sea; carried downwind over land, dropping with distance from the
  upwind coast; **ranges wring it out** — the windward side of a range is wet, the lee side dry
  (rain shadow). Computed by marching upwind from the point over a few dozen kilometres and
  counting sea and the highest ground crossed.

### 3.5 Biomes, ice and water

- **Biomes per point** from warmth, wetness, elevation and slope (the existing rules: forest,
  grassland, desert, tundra, taiga, jungle, beach, rock, snow), with slope judged against the
  plain's "up".
- **Ice** where it is cold enough, not by plain shape: polar caps follow the latitude, and
  glaciers come down the cold sides of high ranges.
- **Lakes** fill basins below the local land only where it is wet; in dry land basins stay dry
  as salt flats or dust bowls. The sea is the sea level of each plain wherever the land is below
  it and connected to the open sea; inland basins below sea level are lakes or dry by wetness.
- **Rivers** later: down the steepest way from wet highlands to the sea or a lake.

### 3.6 Regions

- A plain's **region type** is read from what is on it: mostly sea → a sea or bay; a range along
  it → highlands or peaks; dry → wastes; cold → barrens or icefield; wet warm lowland → forest
  vales; and so on. Its **name** combines an adjective with the type's noun, as now.
- Region climate is no longer stored per plain; the HUD's welcome message and the inspector read
  the type.

### 3.7 Cotton clouds

- **Clouds are objects:** each cloud is a cluster of 5–40 soft, round puffs (ellipsoids of
  different sizes, flattened at the base), 0.3–3 km across, with a flat base at the condensation
  height (2–4 km above the ground below) and tops reaching higher where it is wet and warm
  (towering cumulus near the equator and against ranges).
- **Where: weather systems first, then clouds inside them.** A large-scale cloudiness field
  decides where clouds gather: latitude bands (a stormy belt at the equator, clear belts at
  25–35° over the deserts, storm tracks at mid latitudes), a few drifting **storms** (swirling
  lows with curved frontal bands and clear gaps), cloud banks against the windward side of
  ranges, low sheets over cool seas. Clouds are placed by that field **squared**, so they clump
  into systems; inside a system they are big and merge into clusters and sheets, outside it they
  are small scattered fair-weather puffs. Placing each cloud independently by local wetness alone
  (the first mock) gives an even scatter with no systems, which looks wrong.
- **Look:** bright, slightly warm tops, soft blue-grey undersides, a bright rim against the sun,
  no hard edges (wrap lighting and a soft fresnel); they cast soft shadows on the ground near the
  camera; they drift slowly with the wind.
- **Cost:** a few thousand puffs in view as instanced meshes; far clouds merged into one
  billboard per cloud, very far ones into the planet's colour seen from orbit. Flying through a
  cloud fades it out around the camera (a short fog), arcade style.
- The flat cloud deck of the atmosphere shader goes; the atmosphere keeps its scattering.

## 4. Data and parameters

- `content/data/region_types/` becomes **region kinds** (noun, which conditions they describe,
  palette hints), not ground shapes; the ground's numbers move to a **planet geography** block in
  the star system data (plate mix, range height and width, trench depth, roughness, wind bands,
  cloud amount), so each planet is a few parameters plus its seed.
- Arcade defaults for Tessera: ranges 12 km, trenches 6 km, continental base 300 m, oceanic −4 km,
  cloud base 2.5 km.

## 5. Implementation (when approved)

1. Tectonics, continents and relief in `sim_planet` (plates and creases computed at load, the
   elevation function), with tests: ranges along converging continental creases, trenches
   offshore, coasts off the creases, continuity at creases.
2. Climate (warmth, wind, wetness with rain shadow), biomes, ice and lakes; region kinds from
   what emerges; tile shading with the plain's "up".
3. Cotton clouds: placement from the climate (a deterministic field of cloud seeds per plain),
   the puff mesh and material, levels of detail, shadows near the camera, drift; the old deck
   removed.
4. A test round: orbit views, a range with its rain shadow, a coast, clouds from below, beside and
   above.

Mock renders of the plan (a geography map of Tessera and cotton clouds over a range) come first,
for review.
