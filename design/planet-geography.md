# Planet geography: land, climate, water and clouds on a planet of plains

Drafted 2026-11-17 for the developer's review, after the first arcade Tessera (development plan
5.7). The developer judged the land and climate incoherent and asked for a systematic overhaul,
and for cotton clouds instead of one thin deck. Decided so far: **the planet decides, regions name
what emerges** (not hand-assigned types); **cotton clouds as puffy meshes**; a design and mock
renders before code. Geometry and blocks are in block-world.md §5.1; this document is what the
ground and sky are like on that geometry.

**Decided after the mock renders (2026-11-17):**

| Question | Decision |
|---|---|
| Direction | Plates and creases shape the land and the computed climate decides the rest, **plus ranges and highlands inside plains** (old mountain belts, volcanoes), so not every range follows a crease |
| Sea | **About 40 %** of the surface: two or three continents with inland seas and island chains |
| Cloud amount | **About a third** of the surface under cloud: systems stand out, most places clear most of the time |
| Cloud look | **Soft cotton:** soft, slightly see-through edges, bright sunlit tops, blue-grey bottoms, puffs merging |
| Highest ranges | **About 8 km** (lesser ranges and highlands 2–5 km); cloud bases ~2.5 km, tops 4–6 km |

## 1. What was wrong

| Problem | Why it looked wrong |
|---|---|
| Geography followed the plains | Seas were hexagons, coastlines straight creases, a "mountain region" a 69 km blob of peaks |
| Region types dealt at random | Hot wastes beside ice caps; nothing caused any climate |
| Water ignored climate | Anything below sea level was water, so desert canyons held lakes |
| Mountains were noise, not ranges | No chains, passes or foothills |
| Small things | Flat pentagon ice domes, dull grassland, rock judged against the wrong "up", one flat cloud deck |

## 1a. On a round planet (2026-11-17)

Tessera goes back to round (block-world.md §5.0). The layers below carry over; what changes:

- **Plates are hidden geology, with curved borders.** About 30 plates are cells on the sphere
  (seeds spread evenly and jittered), their borders warped by noise so they curve; a point's plate
  is the nearest seed to its warped direction. Each **plate boundary** gets a character from the
  two plates' drift as before (§3.1), and features are shaped by the distance to the boundary
  (half the angle between the two nearest seeds, times the radius), so ranges curve and branch.
  Mountain belts and volcanoes sit inside plates.
- **Sea level is a sphere;** heights are radial; gravity toward the centre; clouds have flat bases
  at a height above sea level in the local tangent frame.
- **Climate on a latitude–longitude grid** (1°), computed at load on all cores.
- **Regions are natural areas grown from the land** (§3.6 below, replacing plain-shaped regions).

## 1b. Generation as a short simulation (2026-11-17, replaces the noise layers of §3.1–3.6)

The first round planet still stacked hand-made noise layers (plate bumps, ridged noise, ranges
laid along borders, thresholds patching the results), and its plate count (32) was a leftover of
the football layout. The developer asked for a systematic overhaul. World generation becomes a
**short simulation of the planet's history, run once at load** on a mesh over the sphere, and the
terrain at runtime reads its fields and adds only small detail.

**The mesh.** An icosphere of about 100–160 thousand points (about 1 km² each on Tessera), with
fields per point: plate, crust, uplift, height, drainage, temperature, rain, biome, region. At
runtime a point's fields are interpolated over its triangle; noise adds detail only below the
mesh's scale, its strength set by the fields (rough rock in ranges, smooth plains and floors).
Generation takes about 1–3 s at load on the workers, deterministic from the seed (cacheable).

**Stages** (each reads only the ones before it):

1. **Plates:** a planet's plate count and size spread come from its data (Tessera: about 12,
   from a few large to several small), grown by flood fill from random seeds at random rates, so
   outlines are irregular and sizes vary. Each plate turns rigidly about its own pole (a rotation,
   so motion is coherent across it); at every boundary point the relative motion splits into
   closing, opening and sliding, so a boundary can be a collision at one end and a slide at the
   other.
2. **Crust and uplift:** continental crust as continents within plates (plates carry both, as
   on Earth) and oceanic crust elsewhere. Ocean floor deepens with its age, the distance from the
   spreading ridges, giving mid-ocean ridges and abyssal plains. Collisions raise ranges (higher
   for harder collisions, a plateau behind the strongest); an ocean plate sinking under a
   continent makes a coastal range and an offshore trench; ocean against ocean makes an island arc;
   opening continents make rift valleys. A few fixed hotspots leave chains of volcanic islands,
   older and lower away from the hotspot, along the plate's motion.
3. **Erosion:** water flow over the mesh (pits filled, steepest descent, drainage area), and a
   stream-power erosion model run over a number of steps while the uplift builds the land: ranges
   are carved into branching valleys and sharp ridges, foothills form, sediment smooths lowlands,
   coasts are shaped by drainage. This is what makes the land look formed rather than drawn.
4. **Climate:** warmth from latitude and height; three circulation cells per hemisphere give the
   winds and rain belts (a wet equator, dry belts near 30° latitude, wet westerlies, dry poles);
   moisture is carried from the sea along the wind across the mesh and rains out where air rises
   up slopes (windward rain, lee rain shadows); cold currents keep subtropical west coasts dry.
5. **Water:** rivers where enough rain gathers along the erosion channels; lakes where a basin
   gets more water than evaporates, salt flats where it doesn't; deltas where rivers meet the sea.
   Rivers carve the land in any case; drawing them as water in the game is later work.
6. **Biomes** from temperature and rainfall (the ecologists' Whittaker chart) and height (tree
   line, snow line, glaciers down cold valleys); the same fields later set ground cover density.
7. **Regions are river basins:** a region is a drainage area bounded by divides (crests) and
   coasts; large basins split by biome, small coastal ones merged; seas split where coasts pinch
   them. Named from what they hold.
8. **Clouds** from the climate's rising air (the equatorial belt, storm tracks, windward ranges),
   as in §3.7.

**Arcade exaggeration** is a few knobs on the stages (stronger uplift, deeper trenches, faster
erosion), so ranges are tall but placed and shaped believably. A Python prototype renders maps
and views of Tessera for review before the Rust implementation (stages 1–2, 3 and 5, 4 and 6,
7), each followed by a test round.

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

Ranges also rise **inside plains**: a few seeded old mountain belts (curving ridge lines across
a plain, 2–5 km) and volcanoes (single cones, some in island chains), so the land isn't only
lined up with the creases.


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

On a round planet regions are **natural areas**, not geometry:

1. **Cells:** about 600 small cells evenly over the sphere (about 13 km across on Tessera), each
   with its character: sea or land, height, steepness, biome, wetness.
2. **Growing:** neighbouring cells merge into regions by similarity (cheapest first); merging
   across a **range crest** or a **coastline** costs a lot, so those become borders, as do sharp
   changes of climate. Regions stop growing at a size cap; ones left too small join their
   cheapest neighbour. Target about 2,000–5,000 km² each, about 30–40 regions on Tessera.
3. **Seas** are regions too: oceans, bays and straits, split where coasts pinch them.
4. **Names** are read from what a region holds (its dominant ground and climate: sea, isles,
   coast, peaks, highlands, icefield, barrens, wastes, steppe, meres, wilds, vale, downs,
   plains), with an adjective: "Ashen Steppe", "Silver Bay".
5. **In play:** a point's region is its nearest cell's; "Entering …" fires when crossing a region
   border — cresting a pass, reaching a coast, leaving the desert.

Regions are gameplay (identity, welcome messages, later quests and ownership); plates are only
the geology that shaped the land.

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
- Arcade defaults for Tessera: ranges 8 km (inner ranges and highlands 2–5 km), trenches 6 km,
  continental base 300 m, oceanic −4 km, sea about 40 % (plate mix and the continent field tuned
  to it), cloud cover about a third, cloud base 2.5 km.

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
