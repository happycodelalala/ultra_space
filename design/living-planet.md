# A living planet (Batch 4)

Meridian gets people: two cultures at different tech levels living side by side, their
factions, settlements from hamlets to towns and colonies, and a population that lives its days
whether or not the player is there. This batch brings forward a first slice of the living
galaxy, the LLM content and the cultures-and-people batches (development plan §7: Batches 7–9)
so the world has places to go and the planet size can be decided with settlements on it
(universe design §1).

Scope: what is needed to land at a medieval village and at a spacefaring colony and walk among
people going about their day. Not in scope: trade, dialogue, quests, combat, player bases,
other planets' populations, the real LLM gateway.

## 1. Decisions

| Topic | Decision |
|---|---|
| Cultures | Two on Meridian, side by side: a **medieval** culture and a **spacefaring colony** culture, different tech tiers on one planet (overview §6) |
| Generated lore and text | Through the **content director** interface (overview §13): requests for culture profiles, faction and settlement lore, names and codex text. Until the real gateway (Batch 8), a **replay provider** answers them from recorded responses in `content/lore/`, written in this batch the way the LLM would answer (structured JSON, validated by the same schemas). Swapping in the real gateway changes no caller |
| What generation decides | The rules and seeds place settlements, size populations and pick names from naming rules; the content director only writes text and picks among existing kits and parameters (overview §13.2, §13.9) |
| People | Third-party CC0 humanoids on one shared skeleton with its shared animation set (walk, run, idle, sit, work), dressed per culture; replaced by original characters on the same skeleton later (design track) |
| Buildings | Building kits per culture: the medieval kit from CC0 village and town packs; the colony kit a CC0 sci-fi kit as a stand-in until the design track's own |
| Simulation | Two tiers (overview §3): **aggregate** for every settlement all the time; **local** (buildings as objects, people with schedules) only near the player |
| Persistence | Generation is deterministic from seeds, so untouched content isn't saved; the save keeps each settlement's changing state (population, stocks) and anything the player changed |

## 2. Data

All in `content/data/` with JSON schemas; generated records follow the same formats.

- **Culture profile:** id, name, **tech tier** (tribal, medieval, industrial, spacefaring),
  government, values and taboos, attitude to outsiders, **naming rules** (syllable sets and
  patterns for people, places and factions), **building kit**, **clothing set** and palette,
  **settlement style** (layout rules, sizes), roles its people take (farmer, smith, guard,
  priest; technician, pilot, scientist, medic), and its lore (history summary, beliefs).
- **Faction:** id, name, culture, government, seat (a settlement), colours, relations with
  every other faction (−100 hostile … +100 allied), lore.
- **Settlement:** id, name, faction, kind (hamlet, village, town; outpost, colony, port),
  place (a direction on the body), founding date, layout seed, and its state: population by
  role, stocks (food, goods), mood. Its lore line comes from the content director.
- **Person (notable or met):** name (culture's naming rules), role, home, age; created lazily
  when the player meets someone, and kept once named.

## 3. Generation (top-down, lazy)

1. **Cultures on the planet:** the star system data names which cultures live on a body and
   roughly where (a home region each). The two on Meridian start on the same continent so
   their borders meet.
2. **Sites:** candidate spots sampled over each home region, scored by the planet's own
   climate and terrain (planet-surface.md): flat enough, above the sea, near water, in good
   biomes for the culture (grassland and forest for the villages; the colony prefers flat,
   open ground and cares less about climate). Spacing grows with settlement size.
3. **Factions** divide each culture's settlements (a few factions per culture, each with a
   seat), with relations from culture attitudes and distance.
4. **Settlements:** kind and population from the site's score and the faction's size; names
   from naming rules; a lore line from the content director.
5. **Lazy detail:** layouts, buildings and people are made only when the player comes near,
   always the same from the layout seed.

## 4. Aggregate simulation (every settlement, always)

A few seconds per step, cheap, parallel by region, at any time warp:

- **Food and goods:** roles produce and consume per day; stocks rise and fall; a shortage
  lowers mood and growth.
- **Population** grows with food and mood, shrinks with shortage; new people take the roles
  the settlement lacks.
- **Factions:** relations drift slowly towards what cultures and borders suggest. (Wars,
  trade and migration come later.)

## 5. Settlements on the ground

- **Tangent-plane grid** (overview §7): each settlement has a local frame at its centre,
  flat to the surface; its layout is laid out on that plane.
- **Layout by culture:** the medieval style grows around a square or a chapel along winding
  streets, fields outside, walls and a gate for towns; the colony style is a planned grid of
  modules around a landing pad, with power and comms.
- **Terrain:** foundations flatten the ground under each building (terrain deltas, like
  landing pads), generated from the layout, not saved. Many flattened spots in one place need a
  spatial index so terrain sampling stays fast.
- **Buildings** are objects with collision (the walker and vehicles stop at walls), drawn
  instanced from the kit's models.
- **From the air:** settlements show at a distance in a simplified form, so they don't pop
  into existence on approach, and windows and streets are lit at night.

## 6. People near the player

- **Promotion:** within about 2 km a settlement's buildings appear and some of its people
  (a few dozen at most) are spawned from its counts by role; leaving demotes them back.
- **Schedules** by local time of day (planet-surface and universe time design): sleep at home,
  work at their role's place (fields, smithy, workshop, landing pad), eat, gather at the square
  or the mess in the evening. A simple utility choice picks the next activity.
- **Movement** along the settlement's street graph on the terrain, keeping out of each other's
  and the player's way; animation follows (walk, idle, work, sit).
- **Level of detail:** people far from the player update rarely and simply; close ones every
  frame.
- **Meeting people:** looking at someone shows their name and role (and their culture's look);
  conversation comes with dialogue (Batch 9).

## 7. Player-facing

- Entering a settlement shows its name, faction and culture; the inspector shows its state and
  lore.
- Day and night visibly change what people do.
- Save and load keep each settlement's state.

## 8. Budgets

| Item | Budget |
|---|---|
| Aggregate step, all of Meridian's settlements | < 1 ms per step, off the frame |
| Promoting a settlement (layout, buildings, people) | Built on workers; no frame over 50 ms |
| People simulated in detail | ≤ 64 per settlement near the player |
| Settlement draw cost | A few draw calls per building kind (instancing) |

## 9. Open questions

- Planet size (300, 600 or 1000 km): decided at the end of the batch with settlements on the
  ground (universe design §1).
- How cultures interact (trade, uplift, conflict): Batches 7 and 9.
