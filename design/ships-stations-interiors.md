# Ships, stations and interiors

How ships and stations are designed, built, bought and modified; how their interiors scale
from a few rooms to a floating city; how a freely built ship runs as fast as a hand-made one;
and what that means for art and performance.

Builds on [objects-vehicles-physics.md](objects-vehicles-physics.md) (parts, designs, mass
properties, thrust allocation, simulation tiers) and technical overview §5 (vehicles) and §12
(capital ships, hull walking, EVA, boarding).

## 1. Decisions

| Question | Decision |
|---|---|
| Look | **Homeworld/X4-style silhouettes**: strong, readable shapes per faction |
| Freedom | Players can still **modify and build ships KSP- or Starship-style**, down to single parts |
| Build or buy | **Both.** Factions have default ships to buy or capture; players can buy or make parts and build their own |
| Interiors | Part of the same part graph, scaling from a shuttle to a station city |
| Runtime cost | Ships are built freely but **compiled** into an efficient runtime form (§6) |
| Art | Hybrid: meshes for parts, a shared procedural material system, procedural hull skins made at build time, shader effects for FX only (§7) |

## 2. Function and form

Every ship, station and vehicle is **one part graph** (objects-vehicles-physics §2). Each part
has two separate layers:

- **Function:** the simulation data: mass, shape, thrust, power, health, crew capacity,
  interior spaces. Physics, damage and AI only read this.
- **Form:** how it looks: a **visual model reference**, material slots, and the style family
  it belongs to. The form never affects the simulation.

Keeping them separate means the same functional ship can wear different looks, placeholder
art can be replaced later without touching designs or saves, and a smooth hull can cover a
messy part layout.

## 3. Building levels

Players and factions build at whichever level suits the ship. All levels produce the same
part graph, so a ship can mix them.

### 3.1 Fine parts (KSP-style)

- Single parts: tanks, thrusters, wheels, beams, cockpits, wings.
- Full engineering freedom; best for small craft, rovers, landers and experiments.
- Attached at **attach nodes** with symmetry tools.

### 3.2 Section kits (medium ships)

- A **section** is a composite part: a pre-assembled group of fine parts with an outer shape,
  for example a cockpit section, a cargo spine, an engine block.
- Sections join at **connectors** with a size class and shape profile, so they fit together
  cleanly and silhouettes stay readable.
- A section can be opened in the builder and edited at the fine-part level; it then becomes a
  custom section.

### 3.3 Capital hulls with module slots

- A capital ship is a **hull** (the large silhouette, designed per faction) with **slots**:
  engine slots, weapon hardpoints, hangar bays, internal module bays.
- Players customize capitals by filling and swapping slots, like X4 ship loadouts; the hull
  itself can still be edited in the builder, but most players won't need to.

### 3.4 Stations

- Stations are built from **grid modules** (X4-style): habitat, production, docking,
  storage, defense, connector modules on a 3D grid.
- Modules are sections with larger connectors; everything from §3.2 applies.
- Most stations are on rails (objects-vehicles-physics §9.2).

### 3.5 Style families

- Each culture or faction has a **style family**: shape language, proportions, colour
  schemes, decals, greebles.
- Section kits, hulls and modules belong to a style family, so a faction's ships look related
  however they're assembled.
- Generated cultures (overview §13.2) choose or combine style families.

## 4. Faction ships, buying, capturing and building

- **Designs are blueprints.** A faction's default ships are ordinary designs built from that
  faction's parts, sections and hulls.
- **Shops and shipyards** sell parts, section kits, hulls, modules and complete designs, with
  availability depending on faction, tech level and reputation.
- **Buying a ship** gives you an instance of its design. **Capturing a ship** (boarding,
  overview §12.6) gives you the instance with its damage.
- **Every ship opens in the builder**, including bought and captured ones. Edit the design,
  then build or refit it at a shipyard from parts you own or buy.
- **NPC fleets** are assembled from kits by **style and role rules** (a faction's "light
  frigate" = these sections in this order with these loadouts). An LLM can propose new
  designs (overview §13.5); the builder's validation rules decide whether they fly.

## 5. Interiors

### 5.1 Modules carry interiors

Each section or module carries an **interior definition** next to its function and form:

| Module provides | Example |
|---|---|
| **Spaces** | Cockpit, crew bunk, corridor, cargo bay, hangar, reactor room, market hall, residential block |
| **Interior connectors** | Doors and hatches lined up with the structural connectors, so attaching two modules joins their interiors |
| **Properties** | Gravity, air volume, power and life-support needs, capacity (people, vehicle size class) |

When modules attach, their interiors join automatically; small gaps are filled with generated
corridors.

### 5.2 Interior graph

From the modules, the game builds an **interior graph**:

- **Nodes:** spaces (rooms, corridors, halls, hangars, district plazas).
- **Edges:** passages (doors, hatches, airlocks, lifts, transit lines, vehicle ramps), each with
  a capacity and a size class.

One structure serves many systems:

| System | Uses the graph for |
|---|---|
| Navigation | Hierarchical pathfinding: districts → rooms → walkable mesh inside the room |
| Air and damage | Breaches vent rooms, bulkheads seal (overview §12.2) |
| Combat | Chokepoints, breach points, defended zones, security doors |
| Power and life support | A damaged reactor darkens the rooms it feeds |
| Rendering | **Portal culling:** doors are portals; only rooms visible through open doors are drawn |
| Economy and people | Shops, workplaces, homes, services, NPC schedules |
| LLM content | District identities, residents, local rumours |

### 5.3 Scale tiers

| Size | Interior | Simulation |
|---|---|---|
| Small ship (e.g. the Kestrel) | A few hand-made rooms built into its sections | Full detail when aboard |
| Medium ship | Rooms from section kits plus generated corridors | Full detail near the player |
| Capital ship or station | **Districts** (decks, sectors), each made of modules; rooms laid out by shape grammars in the culture's style, like settlement generation (overview §6–§7) | **Streamed and tiered:** the player's district at full detail, neighbours reduced, the rest abstract (headcounts, room states) |

A floating city works like a planetary city inside walls: the same settlement generator, NPC
schedules, economy and LLM-generated identities, one district at a time. Thousands of residents
use the crowd tiers planned for battles: individual agents near the player, aggregates farther
away.

### 5.4 Frames, gravity, regions and vehicles inside

- A large ship or station is **its own frame** (overview §12.1). People, crates and vehicles
  inside move in the ship's local frame; its flight never disturbs interior physics.
- Very large ones can be **split into several regions** (one per district), simulated in
  parallel like body regions and battle regions.
- **Gravity per volume:** artificial gravity inside, zero-g in shafts and on the hull, rotating
  rings if a design has them.
- **Vehicles inside are normal vehicles** in the interior frame: rovers in a hangar, cargo
  loaders, rail cars between districts, mechs in a large bay. Passage size classes tell
  pathfinding where a vehicle fits.

### 5.5 Combat inside

- Boarding actions and station sieges reuse **first-person and melee combat** (overview §9–§10)
  and the unified damage pipeline.
- The interior graph is the tactical map: chokepoints, sealable bulkheads, venting a room as a
  tactic, sabotaging power or life support.
- Large interior battles use the **same tiered battle system** as planetary battles: the fight
  near the player in full detail, other districts resolved abstractly, reinforcements moving
  along connected passages.
- Internal defenses (turrets, security doors, drones) are parts placed in rooms.

### 5.6 Inside and outside stay consistent

- Each section kit defines **both exterior and interior**, so the interior always fits inside
  the hull.
- Windows and viewports link them: the battle outside seen from the bridge, the planet from an
  observation deck.
- The procedural hull skin (§7.2) wraps the interior volumes as well as the functional parts.

### 5.7 Persistence

- Furniture, crates, terminals and NPC homes are persistent objects in the ship's frame
  (overview §14).
- When the player isn't aboard, an interior is records only: room states, crew counts, stored
  items, simulated in the cheap tiers.

## 6. Design compilation

Players build ships KSP-style, but the game doesn't run the part graph directly. Each design
is **compiled** into a runtime form, so a ship with hundreds of parts costs about as much as a
hand-made one. The Batch 1 `VehicleBody` (mass properties, thrust allocator, performance summary
computed once from the parts) is the first piece of this.

### 6.1 Two forms of one ship

| | **Editable form (design)** | **Compiled form (runtime)** |
|---|---|---|
| Used by | The builder, saves, blueprints | Flight, combat, rendering |
| Structure | Part graph: every part, connector and slot | A few flat, cache-friendly blocks |
| Cost | Doesn't matter; only used while editing | Close to a hand-made ship |

The compiled form contains:

| Block | Contents |
|---|---|
| Physics | **One rigid body** with a compound collider of simplified hulls, not one shape per part; no joints between parts (no KSP wobble) |
| Flight | Mass properties, thrust allocator, performance summary |
| Rendering | One merged mesh per material per spatial chunk, LODs, the hull skin, a distant impostor |
| Damage | Per-part health in a compact array; a map from ship regions to part indices. Parts are data, not ECS entities |
| Interior | Interior graph, navigation data, portals |
| Systems | Power, heat and life-support networks reduced to small solved graphs |

### 6.2 Sharing and caching

- Compiled forms are keyed by a **design hash**. Fifty frigates of one design share one compiled
  form and render instanced.
- Compiling runs **in the background**: estimated milliseconds for small craft, up to about a
  second for a capital ship. It happens when leaving the builder, buying a ship or loading a
  save; results can be cached on disk.

### 6.3 Damage without losing the benefit

| Change | Handling |
|---|---|
| A part is destroyed | Vertices carry a part index; the shader hides destroyed parts using a per-ship visibility mask. Instant, no rebuild |
| Stats change (mass, thrust, power) | Recomputed incrementally from the compact arrays on the simulation thread |
| Structural change (a section blown off, the ship splitting) | Only the affected chunks recompile in the background; a broken-off piece becomes its own object with its own compiled form |
| Repair or refit at a shipyard | Edits the design, then recompiles |

### 6.4 Rule

**Nothing at runtime loops over parts every frame.** Per-part work happens only at compile
time or in response to an event (a hit, a part destroyed). Code review and benchmarks check
this.

## 7. Art and performance

### 7.1 Options compared

| Option | Runtime cost | Notes |
|---|---|---|
| Asset packs | Low: ordinary triangle meshes | Polygon counts vary, some need generated LODs; styles clash across packs; licences need checking |
| Own modeling | Low: same as asset packs | Full control over polygon counts and style; the largest time cost for a solo developer |
| Procedural meshes made at build time (e.g. an SDF hull skin meshed once) | Low at runtime once cached; milliseconds to seconds per design to generate, in the background | A unique look per design without hand modeling |
| Procedural materials (triplanar panel lines, wear, faction colours in the shader) | Moderate, per pixel, similar to the planet shader | No UV unwrapping, little texture memory; parts from different sources look consistent |
| Raymarched geometry every frame (Shadertoy-style SDFs) | **High:** grows with screen coverage and resolution | Hard to combine with shadows, depth and lighting; **effects only** (shields, plumes, nebulae, atmosphere) |

### 7.2 Approach

- **Meshes for parts and sections.** Simple shapes and free CC0 kits as placeholders first;
  signature pieces modeled later.
- **One shared procedural material system** with faction colour schemes and decals, so mixed
  parts look like one ship.
- **Procedural hull skins**, generated at compile time and cached, for smooth Homeworld/X4-style
  silhouettes over the part graph. Optional per design.
- **Shader effects** for FX only.
- Content files reference visual models by ID, so placeholder art can be replaced without
  changing designs or saves.

### 7.3 The real risk: part count

The art source matters less than how many parts are drawn: 50 ships with 300 parts each would
be 15,000 draw calls if drawn naively. Mitigations, independent of art source:

1. **Merge each design** into one mesh per material per chunk (compilation, §6).
2. **Instance identical designs:** a fleet of one NPC frigate costs about one draw per material
   per LOD.
3. **LODs, then impostors** at distance (objects-vehicles-physics §9.5).
4. **Virtual geometry** (engine support for very dense meshes) may help later; evaluate it, don't
   rely on it.

Estimate: the development machine's GPU handles a few million triangles and a few thousand draw
calls per frame comfortably, so X4-scale battles fit with merging and instancing. To be confirmed
by a benchmark when fleets arrive (Batch 6).

## 8. Data format additions

Added to the part and vehicle formats in **Batch 3** with the builder, so the format supports
everything above from the start, even before each feature is implemented:

| Addition | Used by |
|---|---|
| Attach nodes (position, direction, size) | Fine-part building (§3.1) |
| Connectors (size class, profile, interior passage) | Sections and modules (§3.2–§3.4), joining interiors (§5.1) |
| Composite parts (sections as groups of parts) | Section kits, hulls, modules |
| Slots (type, size) | Capital loadouts (§3.3) |
| Visual model reference, material slots, style family | Form layer (§2), art swaps (§7.2) |
| Interior definition (spaces, interior connectors, properties) | Interiors (§5) |
| Design hash | Compiled-form cache (§6.2) |

## 9. When each piece is built

| Piece | Batch |
|---|---|
| Compiled flight data (`VehicleBody`) | 1 (done) |
| Builder with fine parts, attach nodes, connectors, sections; design compilation for physics and rendering (merged meshes, colliders); data format additions (§8) | 3 |
| Walking inside small ships (cockpit, a few rooms) | 2–3, once walking on foot exists |
| Per-part damage with visibility masks and incremental recompiles | 5 |
| Instancing, LODs and impostors for fleets; shipyards, shops, buying designs; NPC fleets from kits | 6 |
| Style families from generated cultures | 7–8 |
| Districts, generated layouts, crowds, station economy | 8 and 11 |
| Interior combat and boarding | 9–11 |
| Interior graph (navigation, portals, air and breaches), capital hulls with slots, stations as cities | 11 |
| Procedural hull skins | When art direction needs it; not before Batch 3 |

## 10. Open questions

- Hull skin method (SDF meshing, lofted surfaces, or both) and how much control the player has.
- Interior layout generator for districts: shape grammar details per culture.
- Grid size and connector size classes for stations and sections.
- Art sources: which CC0 kits for placeholders; which signature pieces to model first.
- Whether virtual geometry is mature enough by Batch 6.
