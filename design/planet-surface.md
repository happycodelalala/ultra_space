# Planet surfaces: terrain, climate, landing and walking (Batch 2)

How a planet becomes a place you can land on and walk across, seamlessly from orbit. Builds on
technical overview §2 (seamless planets), development plan §4 (rendering approach) and
universe design §1 and §5 (planet scale, climate). Tasks are in development plan §7.3.

## 1. Decisions

| Question | Decision | Why |
|---|---|---|
| Where terrain is generated | **CPU, in `sim_planet`:** one deterministic `f64` height and climate function, run on worker threads | Collision, landing spots, resources and AI need the same heights as the picture; GPU generation would need slow readbacks. 16 cores make CPU tiles fast enough |
| How the orbit view matches | The planet shader samples **cube maps baked from the same function** (height, biome color) at load | "One function, used everywhere" (development plan §4) without porting noise to a shader |
| GPU's role | Shading only: lighting, fine detail noise, atmosphere, clouds, water | Detail that has no gameplay effect |
| Terrain shape | **Heightmap** over a cube-sphere for Batch 2; voxel patches for digging later (overview §2) | Cheap, looks good, and enough for landing and walking |
| Planet size | 600 km baseline stays; the 300 / 600 / 1,000 km comparison waits for a Windows session (universe design §1) | Needs judging on foot in a native build |
| Climate | Rules compute temperature and moisture from parameters written by hand in the star system data; the LLM proposes these parameters from Batch 7 (universe design §5) | Same data format before and after LLM generation |

## 2. Terrain function

`sim_planet` evaluates, for a unit direction from the planet's center:

- **Height** above the reference radius (m), from layered noise:
  - continents (low frequency) set land and ocean, with the threshold chosen so the ocean
    fraction matches `ocean_coverage`
  - mountain ranges (ridged noise) inside continents
  - hills and detail (fractal noise), fading with tile resolution
  - craters on airless bodies
- **Climate fields:** temperature (latitude, season, altitude lapse rate) and moisture (noise
  plus closeness to the ocean).
- **Biome**, from temperature, moisture, height and slope: ocean, beach, grassland, forest,
  desert, tundra, snow and ice, bare rock, lava.

Rules:

- **Deterministic:** the same seed and parameters give the same result on every machine
  (plain `f64` arithmetic, no platform-dependent functions in the hot path).
- **Procedural keys:** a terrain tile is identified by (body, face, level, x, y); its content
  is a pure function of the key, so it can be regenerated, cached and later have **deltas**
  (player changes) applied by key.
- **Parameters** live in the star system data (a `terrain` block per solid body), with a
  schema.

## 3. Cube-sphere quadtree

- Six cube faces, each a quadtree of square tiles. A tile has a fixed grid of vertices (e.g.
  33 × 33) projected onto the sphere, positions stored relative to the tile's center so `f32`
  stays precise; skirts hide cracks between levels.
- **Split and merge** by screen-space error: a tile splits when its geometric error, seen from
  the camera, exceeds about a pixel; levels go down to under a metre of vertex spacing near
  the player.
- **Generation on workers** with a per-frame budget for uploading new tiles (hitch budget,
  overview §14.6); parents stay visible until their children are ready, so no holes appear.
- **Prefetch from travel:** approaching a planet starts generating the tiles the descent will
  need (overview §14.4).
- The distant shader sphere hands over to tiles as the planet grows on screen, with the same
  baked colors so the switch is invisible.

## 4. Looks

- **Terrain material:** biome colors from the baked fields, rock on steep slopes, snow by
  altitude and latitude, fine detail noise in the shader.
- **Water:** a sea-level sphere with waves, reflections and depth color.
- **Atmosphere from the ground:** scattering for blue skies, sunsets and haze over distant
  terrain, consistent with the view from orbit.
- **Clouds:** a cloud layer whose coverage follows the moisture field, so deserts are clear
  and rain belts are cloudy.

## 5. Landing and walking

- **Ground contact** uses the terrain height and normal at the vehicle's position instead of a
  sphere; landed vehicles rest on slopes.
- **Atmospheric drag** for descent (simple; lift waits for wings in Batch 3).
- **Things on the ground live in the surface frame** (objects-vehicles-physics §1.2): a landed
  ship or a walking player turns with the planet without integration drift.
- **Walking on foot:** a kinematic character controller (capsule against the terrain height
  field), gravity toward the planet's center, first-person camera; exit and enter the ship;
  low-gravity jumps on small bodies.
- **Gravity transitions:** "down" and the camera's up direction follow local gravity, from
  standing on the ground to orbit, without a snap.

## 6. Deltas and streaming (object system, development plan §7.1)

- **Terrain deltas** keyed by tile key: Batch 2 implements the format and applies it (e.g. a
  flattened landing pad); saving them to disk comes with the first save file in Batch 3.
- **Hitch-budget stress test:** a scripted orbit → surface descent measuring the worst frame;
  no frame over 50 ms on Windows.

## 7. Deferred

| Feature | Batch |
|---|---|
| Voxel patches (digging, caves) | 4 |
| Vegetation, rocks and scattered props | 4 (with resources) |
| Rivers and erosion simulation | When terrain needs more realism; erosion would be precomputed per planet |
| Weather that changes over time | After clouds; with seasons and time skips |
| LLM-proposed terrain and climate parameters | 7 |
