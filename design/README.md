# Design

Game design, ideas and plans. **Engine-agnostic:** describe what the game does and why, not
which engine API does it. Engine-specific notes belong in `impl/<implementation>/`.

| Document | Contents |
|---|---|
| [technical-overview.md](technical-overview.md) | Game systems and how they work: scale, planets, simulation tiers, vehicles, cultures, combat, capital ships, AI/LLM systems, object model, persistence and streaming |
| [universe-scale-time-and-flight.md](universe-scale-time-and-flight.md) | Planet scale, distances and travel modes, flight model, universe and game clocks, seasons, planet climate, the first star system (Halcyon) |
| [objects-vehicles-physics.md](objects-vehicles-physics.md) | Objects (identity, frame-relative state, regions, transfers), vehicles made of parts, mass properties, thrust allocation, flight and cruise; how the Batch 3 builder plugs in |
| [ships-stations-interiors.md](ships-stations-interiors.md) | Ship and station design: fine parts, section kits, capital hulls with slots, station modules, style families; buying, capturing and building ships; interiors from a few rooms to station cities; design compilation; art and performance |
| [planet-surface.md](planet-surface.md) | Batch 2 planet surfaces: CPU terrain and climate function, cube-sphere quadtree, orbit view from baked maps, looks, landing, walking, deltas and streaming |
| [vehicle-builder.md](vehicle-builder.md) | Batch 3: rigid-body physics with rapier (per-region worlds, terrain collision, wheels, walker), data format additions, design compilation, the builder, saving |
| [living-planet.md](living-planet.md) | Batch 4: two cultures on Meridian, factions, settlements, aggregate population, people with schedules, lore through the content director |
| [design-track.md](design-track.md) | The design track (vehicles, mechs, ships, buildings, objects designed in the viewer and imported at milestones): principles, viewer plan, the Bastion v2 review and plan |
| [bastion.md](bastion.md) | The Bastion titan's functional layout: crew spaces, routes, lights, heat, markings (what each visible detail is for) |
| [development-plan.md](development-plan.md) | Engine decision and rationale, parallel simulation architecture, environment, LLM integration, development batches |

Suggested additions as the design grows:

- `ideas/`: loose ideas and inspiration that aren't committed to yet
- `systems/`: one document per system when a section of the overview gets too large
- `decisions/`: short records of major decisions (what was decided, why, alternatives)
