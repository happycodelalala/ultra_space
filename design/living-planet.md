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

**As built (task 4.1):** the content director is `ai_gateway::ContentDirector`: a request
(kind, key, context) returns a typed answer the caller validates (`ai_gateway::Validate`), from
its cache, from the provider, or from the caller's fallback (the answer says which). The replay
provider reads `content/lore/*.json` (schema `lore.schema.json`, checked in CI); a recorded key
is exact or a pattern ending in `*` whose responses form a pool, picked by the request key's
hash, so lore for settlements generated later still has recorded answers. The types and their
rules (roles of the tech tier, kits and clothing made for it, shares adding to 1, naming
patterns) are in `sim_society::lore`. Recorded so far: both cultures' profiles (the Hollin, a
medieval people descended from the survivors of a ship that fell nine centuries ago and revere
its wreck, the Keel; the Compact, a chartered survey colony that arrived sixty-two years ago and
keeps them under quarantine) and codex entries for both and the Keel.

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

**As built (task 4.2):** a body's `peoples` in the star system data name a culture, its home
region (latitude, longitude and an angular radius, so it fits any body size) and its numbers of
factions and settlements. On Meridian the Hollin live around 20°N 3°E (radius 14°; 3 factions,
24 settlements) and the Compact around 9°N 77°E (radius 9°; 2 factions, 8 settlements), the
western and eastern parts of the large central continent. `sim_society::generate` builds the
society from that, the director's profiles and the body's seed: factions (names from the
culture's faction patterns, colours from its palette, settlements shared out), faction lore from
pools (`faction/<culture>/*`, with `{faction}`, `{people}` and `{culture}` filled in), and
starting relations: factions of one culture from 30 (a wary people) to 50 (a curious one),
between cultures from both attitudes, each pair ±15. Names are drawn syllable by syllable and
redrawn when hard to say (three vowels or four consonants in a row, a tripled letter, longer
than ten letters; places at most eleven with their ending). `cargo run -p sim_society --example
society` prints a body's society.

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

**As built (task 4.3):** `sim_society::generate_settlements` scores points of an even spread
over the sphere inside each home region by the body's own terrain and climate: no sea, coast's
edge (below 5 m), slopes over 8°, ice or rock; the medieval people weigh soil (grassland best,
then forest), a mild climate, moisture, nearness to the coast and flatness; the colony mostly
flatness and open ground. The edge of the home region counts less. Sites are taken best first,
with a little chance and each keeping its own distance (0.6–1.4 × a spacing from the region's
size and the count), so they scatter rather than form a lattice. Faction territories are
wedges around the people's centre in bearing order, sized to each faction's share (always
contiguous); the first faction's wedge holds the best site (the Compact's hub). Each seat is the
best site near the middle of its wedge: a town (Hollin), the colony hub or the port (Compact);
the rest are villages and hamlets, or outposts, sized by their sites. Place names never use
another kind's word ("Port …" only for a port). Lore comes from pools by culture and kind
(`settlement/<culture>/<kind>/*`). On Meridian: the Hollin's 24 settlements (3 towns, 9 villages, 12 hamlets; about 8,600 people)
on the farmland of their region, three towns as seats; the Compact's hub (about 4,800 people; 7,400 in all),
port and six outposts on open ground. `cargo run --release -p sim_society --example
settlement_map` draws them on the biome map (`sim_planet::map`). Relations don't yet depend on
how close factions are; that comes with the simulation of relations (4.4).

## 4. Aggregate simulation (every settlement, always)

A few seconds per step, cheap, parallel by region, at any time warp:

- **Food and goods:** roles produce and consume per day; stocks rise and fall; a shortage
  lowers mood and growth.
- **Population** grows with food and mood, shrinks with shortage; new people take the roles
  the settlement lacks.
- **Factions:** relations drift slowly towards what cultures and borders suggest. (Wars,
  trade and migration come later.)

**As built (task 4.4):** `sim_society::aggregate`. Per person per game day: farmers make 3 food,
herders and fishers 2.4, Compact growers 7.5 (hydroponics: a colony grows most of its food in a
few hands); smiths, wrights and engineers 3 goods, technicians 2, labourers 0.5; everyone eats
1 food and uses 0.3 goods. Hollish yields scale with the site's soil (0.5–1.3). Food spoils
0.5 % a day, goods wear 0.1 %, both capped at 400 days. Mood (0–1) follows food (60 days
stored is plenty) and goods (30 days) with a 20-day lag; it falls to 0.1 when food runs out.
Population grows 3 % × (mood − 0.6) a year (about 1.2 % when content) and falls 8 % a year
in a famine; growth goes to the roles short of the culture's shares, losses in proportion.
Relations drift toward their targets over about 120 days: the generated value less friction
between seats within 400 km (up to 25 across cultures, 10 within one), swung ±8 month to
month. A `RegionSociety` resource holds each body's society in its region and steps every tick
by the tick's game days (time warp included); a step of all 32 settlements on Meridian costs
about 0.01 ms. Over ten game years the Hollin grow from about 8,600 to 9,700 and the Compact
from 7,400 to 7,700. Saves keep each settlement's state (by id) and each pair's relation (by
faction ids); a save without them starts every society fresh. The game generates societies
while loading, from the recorded responses exported with it (`content/lore`).

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

**As built (task 4.5):**

- **Kits.** Each culture's kit is a recipe file (`content/data/building_kits/<id>.json`) that
  `tools/build-kits.sh` assembles in headless Blender (`tools/kits/assemble.py`) from
  third-party pieces in `assets/third_party/<pack>/` (CC0, licences alongside) into one model per
  building in `assets/export/buildings/<kit>/` plus a generated `kit.json` with each building's
  uses and measured bounds. Houses are recipes on cells (walls round each storey, door and
  windows at the front, a hipped roof, chimney, banners, lanterns, a tower); the rest are
  placed pieces. The medieval kit (Kenney Fantasy Town Kit, 3 m cells) has 18 buildings, from
  a 6 m cottage to an 18 m two-storey hall, mills, stalls, a well, trees and a cart; the
  stand-in colony kit (Kenney Space Kit, 6 m cells) has 12: habitat domes, greenhouse, hangar,
  workshop, lab, modules, landing pad, comms dish, generator, a rocket and crates. Fronts face
  +Z. CI checks the kits were rebuilt after their recipes changed (`kits.stamp`) and the recipes
  against `building_kit.schema.json`. Placeholders until the design track's own kits.
- **Layouts** (`sim_society::layout`): what a settlement needs comes from its people by role
  (medieval households of five, a barn per eight farmers and herders, a smithy per three
  smiths, an inn, a shrine and mills for villages and towns, a hall and six market stalls for
  towns; colony habitats of forty, a greenhouse per ten growers, a hangar per ten pilots,
  workshops, labs, pads, power and comms). The **medieval** style starts from a square (a ring
  street with the well at the centre), sends four to six winding streets out from it (a random
  walk that turns back toward its bearing and stops at water or steep ground) with side lanes
  branching off (and, in towns, lanes off those), sized so every household finds frontage.
  Plots along the streets, both sides, fill from the square outward: public buildings take the
  square, homes the streets beyond, farms and mills the outer third; buildings face their
  street. Stalls stand in the square, trees and carts round the edge. The **colony** style is
  a grid of plots (the largest building plus a street) in blocks of 3 × 3 with avenues between,
  turned to a random bearing, filled from the centre out: pads and hangars, the port, the
  administration module, labs, workshops, habitats, greenhouses, then power and comms; each
  building faces the street toward the centre. A plot is skipped when the ground rises more
  than 3 m across it or lies within 1 m of sea level. Meridian's 32 settlements give about 4,000
  buildings (towns about 670, the Compact's colony about 400) in about 50 ms on one core,
  laid out on the loading worker on all cores.
- **Foundations** (`sim_planet::foundations`): every building's footprint (plus 0.8 m) is
  levelled at the ground height of its centre and eases back to the natural ground over 4 m.
  Where neighbours' blends overlap, their heights are averaged weighted by nearness, so each
  building's own rectangle stays level to the millimetre and the ground stays continuous. They
  apply under the player's pads, are generated with the layouts and never saved. **Spatial
  index:** a sample first tests each settlement's cap (one dot product), then the footprints
  in its 16 m cell of that settlement's grid; sampling costs the same as before planet-wide and
  a few percent more inside a town.
- **Collision:** each building has a box from its model's bounds (trees have none). Physics
  streams the boxes near vehicles in as cuboid colliders on the ground body, with the terrain
  patches (they turn with the planet); the walker is pushed out of the boxes near it and slides
  along their walls. Both come with the region's terrain (`Terrain::foundations`), so physics,
  the walker and the drawn terrain agree. The `layout_map` example draws layouts from above.

**As built (task 4.6):** `game_bevy::settlements`. Each settlement of the body has a root
entity in its tangent frame, placed camera-relative each frame. Within 1.2 km of its edge (off
again beyond 1.5 km) its buildings are spawned as kit models: their colours (Kenney's palette
texture or plain material colours) are baked into vertex colours when loaded, so every building
shares one material and identical buildings batch into instanced draws. Elsewhere, out to
250 km, a merged mesh of boxes stands in (each building's bounds in its roof and wall colours,
reaching 3 m into the ground to hide the coarser distant terrain), so nothing pops in. Streets
are ribbons on the ground (4 m dirt lanes and a paved square for the Hollin, 6 m concrete for
the colony). Lights are small glowing lanterns at every door (warm) and lamps on 6 m posts at
the colony's street corners (white); their glow follows the sun's height at the nearest
settlement (none by day, full from 7° below the horizon). Layouts were tidied for the view
from above: lanes end in a junction when they reach another street (no crossings), streets
serving no building are dropped, and trees grow in groves beside the settlement. Debug:
`ULTRA_SPACE_START_NEAR=<name|kind>[,distance[,height[,day|dusk|night]]]` starts near a
settlement at a chosen time of day (in arcade flight when in the air);
`ULTRA_SPACE_NO_SETTLEMENTS=1` draws none (for comparisons).
Measured on Windows (round q0051, quiet machine): flying at the biggest town from 30 km at
~300 m/s, 18.4 ms a frame on average against 17.4 ms with no settlements drawn, one frame over
50 ms in each.

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

**As built (task 4.7):** characters are assembled offline like the kits
(`tools/build-characters.sh`, recipes in `content/data/clothing_sets/`) from three CC0
Quaternius packs on one 65-bone skeleton: the base character's head (vertices weighted to the
head and neck) and hair with an outfit for the Hollin (peasant men and women, ranger-clad
guards), the base body with a coverall painted on by bone (suit, gloves, boots) for the
Compact (blue-grey; wardens orange). Every model carries the shared animations from the
Universal Animation Library (idle, walk, run, work, use, carry, sit, talk), textures at
512 px, 2–4.5 MB each. The packs stay outside the repo (`assets/third_party/README.md`). The
game loads them with Bevy's glTF loader, one animation graph per character.

**As built (task 4.8):** `sim_society::people` and `game_bevy::people`. When a settlement's
buildings appear, one in 25 of its people (8 to 64) is promoted in its role shares, living in
the dwellings nearest the centre, with a name and a workplace by role (smiths at the smithy,
farmers by the barns and mills, merchants at the stalls, Keelwardens in the shrine, growers in
the greenhouses, pilots at the pads...). They are placed where their day has them, then follow
it by the settlement's local solar time: home 22:00–7:30, at midday and for supper, work
7:30–12 and 13–17:30, the square 19–22, each shifted by up to ±25 minutes; walking the street
graph (shortest way) at 1.35 m/s, hidden indoors, side-stepping each other, waiting for the
player; those over 120 m from the player step a quarter as often. Each is drawn as a character
who takes their role, with the animation for what they do (outdoor work by role: use, carry,
talk or work). Leaving demotes them. In a Windows round the town's square fills at 20:00 and
empties for work at 9:00; flying in at ~300 m/s while the town fills had no hitch beyond the
end screenshot.

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
