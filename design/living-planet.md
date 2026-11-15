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

- **Culture profile:** id, name, **tech tier** (Stone Age, Ancient, Medieval, Early Modern,
  Industrial, Modern, Spacefaring, Interstellar), **polity** (band, tribe, chiefdom, realm,
  empire, city-state, republic, nation, charter colony, federation), government in words,
  values and taboos, attitude to outsiders, **naming rules** (syllable sets and
  patterns for people, places and factions), **building kit**, **clothing set** and palette,
  **settlement style** (layout rules, sizes), roles its people take (farmer, smith, guard,
  priest; technician, pilot, scientist, medic), and its lore (history summary, beliefs).
- **Faction:** id, name, culture, government, seat (a settlement), colours, relations with
  every other faction (−100 hostile … +100 allied), lore.
- **Settlement:** id, name, faction, **size class** and **roles** (the settlement hierarchy,
  §8b; as built in 4.3 a single kind: hamlet, village, town; outpost, colony, port), the word
  its culture uses for it, place (a direction on the body, or an orbit for a station),
  founding date, layout seed, and its state: population by role, stocks (food, goods), mood.
  Its lore line comes from the content director.
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

### 6.1 The people model (design ahead, decided 2026-11-14)

How people behave, scale to a metropolis, react when things go wrong, and remember the player.
Sections 6.3–6.8 build on two rules (6.2) that keep it cheap at any population.

### 6.2 Two rules: what costs tokens, and who is remembered

**The token rule.** The LLM (the content director) writes only text a player is about to read,
once, cached in the save; the simulation never calls it. So:

- routines, choices, movement, reactions, situations: rules and data, **no tokens**;
- millions of people: counts in districts; names from the naming rules, looks from seeds,
  **no tokens**;
- remembered people: plain records (ids, numbers, flags, structured memories); their lives
  off-screen follow rules, **no tokens**;
- tokens are spent only when new text appears: a conversation, a place's description the first
  time it is visited (one call per district or settlement visited, never per person), a lore
  revision when it is read (§8c).

Cost therefore scales with what the player reads and says, not with population or with how
many people the world remembers. Anything whose token cost grows with population or per tick
is a design error. Three controls bound it further:

1. **Everything has a path without an LLM:** templates and recorded responses (the replay
   provider) make the game fully playable offline; the LLM adds richness, nothing depends on it.
2. **Caching:** every generated text is keyed and saved, never generated twice.
3. **A budget per session** with priorities: conversations with known people first, new places
   next, background lore revisions last and lazily; a small model for summaries and
   descriptions, the stronger one for important conversations; local models where they serve
   (technical overview §13.12).

Rough cost: a conversation is about 1,500–3,000 tokens in (character card, relationship
summary, the 3–5 most relevant memories, local news) and 200–400 out; a two-hour session with
20–40 conversations, a few new places and some lore comes to about 100–200k tokens.

**Persistence levels.** Only people the player has affected are saved, in four levels:

| Level | Who | Saved | Tokens |
|---|---|---|---|
| **Ambient** | Everyone the player hasn't affected | Nothing: regenerated from seed and state | 0 |
| **Noted** | Anyone the world must stay consistent about: killed, robbed, hired once, name learned | ~100 bytes: seed id, a few flags and numbers | 0 |
| **Known** | A relationship past a threshold, or marked by the player: friend, rival, lover, quest giver | A few KB: relationship, key memories as data, a short summary | Only when talked to |
| **Companion, crew** | The few who travel with the player | A full simulated object, moving between regions and planets like a vehicle | Only when talked to |

- An ambient person's identity is already fixed by the seed (name, looks, role, home,
  workplace); promotion freezes it and adds what's new, so nobody is ever regenerated
  differently. Ambient generation reads the state: a dead baker isn't generated again; the
  vacancy is filled later by a successor with a new seed.
- **Noted people are forgotten** after a while (they fall back to ambient) unless the fact
  matters, like a death.
- **Known people are capped** (about 100–300 active); past the cap the weakest relationships
  fade back to noted, as acquaintances do.

### 6.3 Routines and places

- **Schedule templates** per culture and role, as data, with variants: workday, market day,
  rest or holy day, festival, season, shift work (the colony doesn't keep village hours).
- **Places advertise activities** ("smart objects"): a bench offers sitting, a tavern table
  eating and drinking with company, an anvil smithing, a stall trading, a shrine prayer, a
  field work; each has slots and an animation. Layouts place them with the buildings.
- **A small utility choice** within the schedule's window, weighed by a few needs (hunger,
  rest, company, faith), made when a person finishes something (every few game minutes), never
  per frame.
- **Work is real:** time spent at work feeds the settlement's production in the aggregate
  simulation, and a stall sells from the settlement's stocks, so the economy and what the
  player sees agree.
- **Detail by distance:** near the player, fully animated people moving every frame; promoted
  but distant, positions only, stepped rarely; everyone else a count by place and hour, used
  to fill a place when the player arrives; the planet keeps its aggregate.

### 6.4 The metropolis: millions as statistics

Nobody perceives millions at once (a busy street shows a few hundred, a great plaza a few
thousand), so a city of millions is simulated as statistics and made into individuals only
where the player can see.

- **Districts, not people.** A metropolis is a few hundred districts and blocks, each holding
  people by role, where they are at each hour (the schedule templates applied to the counts),
  flows between districts (a commuting matrix), production, stocks and mood: a few hundred
  small records stepped once per game hour.
- **Ambient people from the statistics.** The block the player is in says how many people are
  on this street, in this shop, on this plaza at this hour; they are spawned out of sight
  (doors, corners, distance) and demoted when they walk out of view. They are deterministic:
  the person behind a counter comes from the building and the hour, the same baker every
  morning, stored nowhere. Interiors are filled from the building's share when entered.
- **The crowd in layers:**

| Distance | What | How many | Cost |
|---|---|---|---|
| Near (to ~50 m) | Full people: schedules, places, avoidance, talk | ~100–300 | CPU microseconds each; skinned animation on the GPU |
| Middle (50–300 m) | Crowd agents on a shared flow over the street graph, simple choices | ~1,000–5,000 | Batched movement; low-poly, animation baked into textures, instanced |
| Far | No agents: crowd density on streets and plazas, traffic as flows of lights, lit windows from occupancy | the millions | Shader cost only |

- **Traffic** (carts, cars, trams, shuttles) is the same: flows on the road and air-lane graph,
  individual vehicles near the player, consistent with the district statistics.
- **Limits:** drawing comes first (about 300 detailed people and 5,000 crowd agents near the
  player fit a mid-range GPU); memory stays small because ambient people aren't stored;
  thinking scales with what's near the player, so a metropolis costs about a big town plus its
  crowd layers.
- **Consistency:** nobody appears or vanishes in view; someone the player follows stays an
  individual until lost; what is seen matches the counts (a famine empties the market, a
  festival fills the square, a war leaves the streets quiet).

### 6.5 Interruptions and consequences

The world is a generated baseline plus saved changes, and every change, small or huge, flows
through the same levels; no case needs its own script.

| Level | Holds | Changes by |
|---|---|---|
| Faction, world | Owners, relations, wars, armies on the map | Decisions and battles, as chronicle events (§8c) |
| Settlement, district | Counts by role, stocks, mood, its **situation**, building states | Aggregate rules, events |
| Building | Intact, damaged, burning, destroyed (rubble), under construction, new | Saved changes keyed by the layout's building id, like terrain pads |
| Remembered person | Their record (6.2) | Saved |
| Ambient person | Nothing | Read from the levels above |

- **Events flow up and down.** Up: a guard is killed in the street; if ambient he becomes a
  noted record (a body, a name, a family) and the settlement's guard count drops; witnesses
  spread it as rumour; the faction's view of the player and the settlement's mood change;
  notable events enter the chronicle. Down: a decision above (war, famine, plague) changes
  what's below: curfews, conscription, rationing, evacuation.
- **Situations override the routine:** each settlement or district has one (normal, festival,
  market day, alarm, battle, siege, fire, disaster, occupation, plague, aftermath), and each
  swaps the schedule templates and activities, as data. In an alarm civilians run home or to a
  refuge (the keep, a shrine, a bunker) and guards to the walls; in the aftermath people gather
  at the ruins, the injured go to healers, carpenters and masons work at damaged buildings.
- **Individuals react before their schedule:** danger (flee, hide or fight, by role and
  courage), injury, a witnessed crime, a blocked way (destroyed buildings change the street
  graph and paths update); still a small choice made when something happens.
- **Building and growing:** new buildings appear as construction sites that go up in stages
  over game days, worked by people whose jobs come from the economy; layouts grow by the rules
  that made them (new plots along streets, lanes at the edge); repair is the same path (rubble,
  site, house), or the ruin stays if the settlement can't afford it or has shrunk.
- **War between factions:** armies are aggregate units moving between settlements on the map;
  battles away from the player are resolved from their numbers, near the player they are
  promoted into real battles (technical overview §9, §10) and the outcome writes back.
  Conquest changes owner, garrison, mood and population (refugees flee to other settlements),
  perhaps the name; the chronicle and lore record it, each side its own way.
- **A bombardment from orbit** (a spaceship strikes a medieval city) uses the same levels,
  bigger: craters become terrain changes, buildings within each impact's radius rubble and
  further out damaged, fires spread over buildings by a simple rule (wood burns, stone
  resists, wind matters); casualties come from the district counts at that hour (homes at
  night, the market at noon), and around the player they are those numbers made visible; the
  situation goes to disaster, then aftermath; survivors flee as refugees to nearby settlements
  (which crowd and go hungry); relations with the attacker fall to war; it is a major chronicle
  event, and lore revises it (the Hollin may read it as the sky-folk returning in wrath, a new
  taboo or sect); the city is rebuilt, abandoned or left a ruin with a condition overlay (§8b).
  Away from the player all of it is applied statistically and heard of later as rumour.
- **Cost:** events are rare, their consequences updates to counts and a list of building
  changes; the expensive local simulations (fire, fleeing crowds, battles) run only near the
  player. Saves hold the changes and the remembered people, never whole cities.

### 6.6 Known people and memory

- **The record:** identity (fixed by the seed), relationship numbers (affinity, trust, respect,
  fear) and tags (friend, rival, lover, debtor, companion, crew), separate from the player's
  reputation with their faction (a Hollish smith can like the player while his reave doesn't);
  **memories** as small structured entries: what happened, when and where, how it felt, how
  important, linked to chronicle events when part of history ("you saved my son at the ford",
  "you sold me a bad blade"); unimportant ones fade, important ones stay; and a short
  **summary** of the relationship.
- **They live on while the player is away,** by rules: where they are at any hour comes from
  their schedule template, computed when asked; life events (marriage, promotion, moving,
  illness, conscription, losing a home in a bombardment) follow the world around them and are
  dated, so the world has moved on when the player returns. Approaching their settlement, they
  are always among those promoted (a reserved slot), where they should be.
- **Relationship unlocks behaviour,** as data: greeting, coming over, sharing rumours, prices,
  lending, jobs from the quest system's supported verbs, invitations (a festival, a wedding),
  messages in the space age; close friends can become companions or crew. Rivals and enemies
  have their own: warning others, sabotage, revenge.
- **Talking (dialogue, Batch 9):** the director gets a character card (personality, voice,
  background), the relationship and its summary, the few memories most relevant now, and what
  they know of the world (news travels along the roads, §8c); it answers with lines plus
  structured intents the game validates (offers, requests, quest proposals). Afterwards the
  summary is updated once if the conversation mattered (a small model, a few hundred tokens),
  or by template when nothing much happened.
- **Finding them:** their record always knows where they are; the journal shows the people the
  player knows with their last known place, and their neighbours know where they went.
- **Interruptions touch them too:** a bombardment's casualty numbers decide whether known
  people are among them, weighted by where their schedule had them; a friend's death is a
  chronicle event and a memory for the player; friends hear of the player's deeds through
  rumour and their people's lore.
- **Scale:** at most a few hundred known people active, a few thousand noted ones before they
  are forgotten: a few MB in the save; only those near the player are simulated in detail.

### 6.7 Customs: from lore to routines

Routines change with the world through two channels. **Mechanical consequences** (rules):
situations, counts, buildings, refugees, relations (6.5). **Cultural consequences**, how a
people reads an event, which is the lore's domain but must change what people do: a new
festival, a mourning day, a taboo, a law, a pilgrimage, a sect.

- **Customs are data the routines read.** Every culture, faction and settlement has a book of
  customs in a closed vocabulary: festivals and holy days (date, place, activities), mourning
  and memorial days, taboos (a forbidden activity, place or time), laws (curfews, bans, levies,
  conscription), rites (which activity, where, how often), holy or cursed places, pilgrimage
  routes, shifts in role shares within limits. Schedule templates read them: a festival
  replaces work, a curfew empties the evening square, a taboo filters activities, a
  pilgrimage sends people along a road. No tokens, however many people follow them.
- **The vocabulary fits the tier:** taboos, rites and elders' decrees for early tiers; laws,
  agencies and regulations later.
- **Lore revisions propose customs.** After an event, the director proposes customs along with
  the lore, as part of its consequences (technical overview §13.14); the game validates them
  (supported activities and places, sensible dates, magnitudes clamped by tier and event) and
  saves them, versioned with the lore. Each takes effect on its own timescale: a curfew
  tonight, a festival from next year. Without an LLM, a rule table per event kind proposes the
  same kinds (a bombardment: a memorial day and a curfew; a conquest: the occupiers' laws; a
  famine: rationing and a harvest rite).
- **The loop runs both ways:** the simulation makes events, the director reads them into
  customs, the customs change routines, which make new events (pilgrims draw bandits, a curfew
  breeds resentment).

**Two thought experiments (2026-11-14): a freighter falls.**

| | On a Stone Age chiefdom (60,000) | On an industrial city (400,000) |
|---|---|---|
| Impact (rules) | Crater, forest fire, a camp burned, a reactor leak's hazard zone | Crater, a factory district destroyed at noon, a fuel depot and chemical works add a larger drifting hazard; power, rail and water fail in districts around |
| Response | Flight to the hill fort | Organised: fire brigades, police cordons, hospitals to capacity, the army at the wreck, evacuation on the roads |
| News | Weeks along trails, garbled | Hours by telegraph and radio |
| Reading | Myth (the Burning Star): the crater holy and forbidden, new-moon vigils, elders and healers grow in share | Science and politics: a commission, martial law, rationing, a mourning day, a research agency; a minority sect reads an omen |
| Tech | Metal from the wreck: slow drift toward the Ancient tier over generations | Reverse engineering: a leap within years, an arms race, spies |
| Later | A town grows by the wreck ("Fallen Star"); centuries on, a people with a Keel of its own | A memorial, a museum, a research town, perhaps a rocket programme |
| The player | Trespass on holy ground; revered or feared as sky-folk | A cordon, under suspicion, newspapers to read |
| Cost | One LLM call at the event, a few when read | The same; the city at district level |

The same pipeline gives a different world because the tier and culture data differ. What the
experiments showed the simulation must express is listed in technical overview §13.14.

### 6.8 Where each part lands

Built in Batch 4: promotion from counts, schedules by local time, street paths, the near level
of detail, looking at people. Next, with the density task: schedule variants and places that
offer activities. With trade and factions (Batches 7, 9): real work and stalls, situations,
war and conquest, the chronicle, noted and known people, dialogue. With cities (Batch 9): the
metropolis model and crowd layers. With orbital strikes (technical overview §11): the
bombardment chain.

## 7. Player-facing

- Entering a settlement shows its name, faction and culture; the inspector shows its state and
  lore.
- Day and night visibly change what people do.
- Save and load keep each settlement's state.

**As built (task 4.9):** `game_bevy::meeting`. Entering a settlement (the camera inside its
extent; leaving needs 200 m more) shows a banner for 6 s: its name, "a Hollish town of the
Gostkod March, its seat", its people (logged). The inspector (F4) adds the settlement the camera
is in or nearest within 3 km: kind, faction, seat, founding, live people, food and goods (in
days) and mood from the aggregate simulation, people by role in the culture's words (chapman,
reave's man, Keelwarden...), its description and landmark. Looking at someone (the person
nearest the middle of the view within 30 m, not behind a building: the line of sight is tested
against the buildings' boxes) shows "name, role (culture)" under the crosshair.

## 8. Budgets

| Item | Budget |
|---|---|
| Aggregate step, all of Meridian's settlements | < 1 ms per step, off the frame |
| Promoting a settlement (layout, buildings, people) | Built on workers; no frame over 50 ms |
| People simulated in detail | ≤ 64 per settlement near the player |
| Settlement draw cost | A few draw calls per building kind (instancing) |

## 8a. Planet size with settlements (task 4.10, for the developer's decision)

`cargo run --release -p sim_society --example size_compare` regenerates Meridian's societies at
each radius (surface gravity kept); the test views `size-<km>-low|high` show the biggest town
from 1.5 km up and 3 km out, and from 25 km up (round q0058).

| Radius | Circumference | Low orbit | Hollish home region | Nearest Hollish settlement | Nearest Compact | Seat to seat (at 300 m/s) |
|---|---|---|---|---|---|---|
| 300 km | 1,885 km | 1.5 km/s | 73 km radius | 17 km | 23 km | 344 km (19 min) |
| 600 km | 3,770 km | 2.3 km/s | 147 km | 35 km | 41 km | 737 km (41 min) |
| 1,000 km | 6,283 km | 3.0 km/s | 244 km | 60 km | 65 km | 1,225 km (68 min) |

What it shows: each people has a fixed number of settlements (Hollin 24, Compact 8) spread
over a home region sized in degrees, so the radius only stretches the gaps. At every size the
land is far emptier than a lived-in one (medieval villages stood 2–5 km apart): from 3 km a
town of 1,900 is a small patch, from 25 km up no settlement can be seen at all. The views
differ mainly in curvature (300 km reads as a small world from 25 km up).

**Decided (2026-11-14):** planet size is not one global choice. In a universe of many planets,
size is data per body and varies freely (universe design §1); Meridian stays at 600 km as the
baseline. What makes a world feel lived in is its settlement density, which comes from each
culture's settlement pattern and the land's area, not from the radius (§8b).

## 8b. Settlement patterns and density

Decided 2026-11-14 (design ahead: only the patterns Meridian uses are implemented now).

**Three densities, three costs.** What the player *sees* decides whether a world feels vacant;
the *simulated population* is numbers in the aggregate simulation (cheap at any size); people
and buildings *in detail* are expensive but exist only near the player and are capped. So the
world can be dense to the eye and large in numbers while the expensive part stays bounded.

**Settlement patterns by tech tier.** Each culture profile carries a settlement pattern (data,
read by one generator): the share of its region it settles, spacing and size ranges per
settlement tier, its urban share, road type, and how much of its people live off the ground.
Typical values for the same ~15,000 km² of territory (a realm of about 70 km radius):

| Tier | How they settle | Spacing (villages / towns / cities) | Largest places | People per km² | Population | Feel |
|---|---|---|---|---|---|---|
| Stone Age | Camps and small villages, some seasonal; hill forts as proto-towns | 8–12 km / none / 1–2 hill forts | 1,000–3,000 | 3–8 | 50–100k | Wild land, smoke on the horizon, trails rather than roads |
| Ancient | Farming villages, walled towns, a few city-states | 5–7 km / 30–40 km / 120–150 km | 5–30k | ~10 | ~150k | Fields round walled hills, temples on the skyline |
| Medieval | Villages, market towns, walled cities, fields between | 4–5 km / 20–30 km / 80–150 km | 10–30k | ~20 | ~300k | Settled countryside, a village every few minutes |
| Early Modern | Villages, many market towns, port cities | 3–5 km / 15–20 km / 60–80 km | 50–200k | 30–60 | 0.5–1 M | Roads busy with carts, harbours with tall ships, star forts |
| Industrial | Fewer, bigger villages; many towns; cities with factories, mines, railways | 3–4 km / 10–15 km / 40–60 km | 50k–500k | 50–150 | 1–2 M | Smokestacks and rail lines; cities seen from orbit |
| Modern | Most people in cities and their suburbs; farms large and few | farms, towns 8–10 km / cities 30–50 km | 1–10 M | 100–300 | 2–5 M | Highways and sprawl, lit coastlines at night |
| Spacefaring homeworld | A few dense arcologies and spaceports, research and agricultural stations, the land left wild; most people in orbit | arcologies 150–300 km, outposts between | 20–100k on the ground | 1–5 on the ground | 20–80k on the ground, many more on stations | Pristine nature with islands of high tech; a sky full of stations |
| Interstellar | As the spacefaring homeworld, with still fewer on the ground; parks, monuments, ruins of earlier eras | arcologies 300 km and more | 50–200k on the ground | < 1 on the ground | most of the people in orbit and other systems | A garden world under a sky of traffic |
| Young colony (the Compact) | A landing-site town and outposts | outposts 30–50 km | 2–5k | < 1 | 5–10k | A frontier: everything new and far apart |

Tiers are named after the eras, for what a people can make; how they are organised is their
**polity**, which adjusts the pattern (an empire has bigger capitals and more towns than a
chiefdom; a city-state packs its people into one city and its fields; a charter colony is the
young-colony pattern at any tier). What one people calls another ("barbarians", "sky-folk")
belongs to its lore, written by the content director, not to the tiers.

**The settlement hierarchy.** A settlement's size and its purpose are separate, as tech tier
and polity are, and neither is named for one era. Sizes follow geography's settlement
hierarchy, the same for every culture and tier; they drive the systems (spacing, layout scale,
what is simulated, level of detail) and never show on screen:

| Size class | People | Stone Age | Ancient, Medieval | Early Modern, Industrial, Modern | Spacefaring, Interstellar |
|---|---|---|---|---|---|
| **Dwelling** | 1–3 households | hunting camp, shelter | farmstead, croft, shepherd's hut | farm, ranch, cabin, rail halt | hab module, relay post, survey camp |
| **Hamlet** | up to ~100 | camp, band site | hamlet | settlement, company row | outpost, site |
| **Village** | 100–500 | village of huts | village | village, mining camp | settlement, base |
| **Town** | 500–5,000 | hill fort, gathering place | town, burgh, market town | town, mill town | colony town, port |
| **City** | 5,000–100,000 | (rare: an earthwork city) | city, walled city | city | city, spaceport |
| **Metropolis** | over 100,000 | — | (rare: an imperial capital) | metropolis, conurbation | arcology |
| **Megalopolis** | millions | — | — | megalopolis | arcology cluster, ecumenopolis |

- **Roles** say what a settlement is for, on top of its size, any number of them: seat or
  capital, port (sea or space), fort, market, mine, farm, temple or monastery, research,
  outpost (a post without settled families), enclave (in a wasteland), arcology, station (a
  settlement whose place is an orbit). Roles choose buildings and people's work.
- **The word shown** comes from the culture: its tech tier gives the defaults in the table,
  and its lore (the content director) its own words: a Hollish town is a "hold", the Compact's
  capital "the Landing", a Stone Age village whatever that people calls it. The word can
  change with the chronicle (§8c): a village grown into a town, a ruined city now "the old
  walls".
- **Dwellings** are the decoration layer: placed from the seed, not simulated (their people
  count toward the nearest hamlet or village), and of their era (a hunting camp, a farmstead
  or a relay post by who lives there). Every 1–1.5 km in settled land, with roads joining the
  settlements and travellers on the roads near the player, they keep the land between
  settlements from feeling empty.
- As built (4.3), `SettlementKind` mixes size and purpose (a colony is a town that is a
  capital and a port; an outpost is a hamlet with the outpost role); the density task replaces
  it with size class and roles.

**A medieval world in numbers.** On a 600 km planet (1.36 million km² of land), 10–15 realms of
the medieval pattern settle about 11–17% of the land; the rest is wilderness, frontier, ruins,
wrecks and resource sites, where exploration lives. Per realm: about 700 villages (avg 250
people), 30 towns (avg 2,500), 1–3 cities (10–30k): about 290,000 people. For the planet:

| | 10 realms | 12 realms | 15 realms |
|---|---|---|---|
| Cities | ~20 | ~24 | ~36 |
| Towns | ~300 | ~360 | ~450 |
| Villages | ~7,000 | ~8,400 | ~10,500 |
| Population | ~2.9 M | ~3.5 M | ~4.4 M |

That is ~19 people per km² inside a realm (medieval countryside held 20–40) and ~2.6 over all
land. Settling all usable land would mean ~35,000 villages and 15–17 M people and leave
nothing wild to explore, so settled shares stay at 10–20%. The number of cities is its own
choice: more developed or more urban cultures have more and bigger towns and cities, and
capitals may reach 50–100k.

**Conditions.** A condition is an overlay on a region of a planet, on any tier: a
**wasteland** (nuclear, chemical or biological), and later war-torn land, plague, famine. A
wasteland brings:
- **hazard zones**: radiation or poison over the terrain and climate, changing the biomes and
  harming walkers and vehicles;
- **enclaves**: a few fortified, domed or walled cities 100 km or more apart, heavily guarded
  and short of resources;
- **between them**: scavenger camps, wrecks, and the **ruins of the old pattern** (generated
  from the earlier tier's layout, then decayed), holding what the survivors can't reach;
- under 1 person per km², most of them in the enclaves.

Conditions come two ways, and both use the same representation:
- **From history (generated):** a planet's lore (from the content director) can record a
  catastrophe; generation then lays the earlier pattern, applies the condition, ruins and
  depopulates by its severity, and places the survivors' pattern on top, all deterministic
  from the seed.
- **During play:** wars between factions, disasters and the player's own actions can create
  a condition. It is then a *saved* overlay (like the terrain deltas): zones with a kind,
  severity and start date; settlements caught in it change state (damaged, abandoned, ruined)
  in the aggregate simulation, and their layouts decay accordingly when next generated
  (layout seed plus damage). Hazards fade over game years by their kind.

Conditions are one kind of consequence among many: what the world's history leaves behind,
told by its lore, which itself keeps changing (§8c).

**Space-age populations live mostly on stations.** A station is a settlement in orbit
(ships-stations-interiors §3.4) and uses the same aggregate simulation (food, goods, mood,
people by role); its interior is generated and populated near the player like a town.

**Cost.** The aggregate simulation scales with settlements, not people: ~10,000 settlements
for a planet (an industrial world has fewer and bigger, a Stone Age one more and tiny) are
~10 MB of records and ~10 ms per step, needed once per game hour, split by region over the
cores. Detail stays bounded: layouts, buildings and people are made only within a few
kilometres (generation on approach, on workers, like terrain tiles, rather than all at load);
distant forms are batched per terrain tile, and night lights seen from orbit come from a
baked map per realm. Big industrial cities need one more level of detail (districts at a
distance, buildings near). Gameplay above the settlement (armies, trade, politics) works on
counties and provinces of 20–50 settlements.

**Scope.** Next: the density task for Meridian's two patterns (medieval and young colony):
counts from area, size classes and roles, the dwellings and roads, lazy layouts, batched
distant forms. The
other tiers' patterns, conditions and stations come with the batches
that first need them (development plan §7.1).

## 8c. Living lore: history that keeps being written

Decided 2026-11-14 (design ahead). Lore isn't written once: the world's background, its
peoples' beliefs, its places' descriptions and its factions' histories change as the world and
the player change it. A wasteland is one example; a conquered town renamed, a heresy that
splits a church, a famine remembered in songs, a colony grown into a city, and the player's own
legend are others.

**One record of what happened: the chronicle.** An append-only, saved list of events: when,
where (body, region, settlement), who (factions, people, the player), what kind (founding,
war, conquest, famine, plague, disaster, catastrophe, migration, discovery, contact, a
succession, a deed of the player's...), how big, and its consequences as state changes the
simulation made (population, owner, condition, ruin, a new name). The simulation decides what
happens and its consequences; the chronicle only records them (technical overview §13.6,
§13.9).

**The past and the present are the same record.** Generating a world writes its history as
chronicle events too: the galaxy's and the planet's timeline from the content director (a
catastrophe, a migration, a fallen ship like the Hollin's Keel) becomes events whose
consequences generation applies (ruins of an older pattern, a wasteland, a people's home
region). So play simply continues the history, and everything downstream treats an event
from 900 years ago and one from yesterday alike.

**Lore is versioned.** Every piece of lore (a culture profile's beliefs and history, a
faction's history and motto, a settlement's description and landmark, codex entries, names)
is a base version from generation plus revisions, each caused by chronicle events and dated.
The current text is the latest revision; older versions stay, because the world remembers:
an old book in a ruin, an elder's tale or an archive shows the place as it was.

**Revising lore.** Significant events are adjudicated by the director into structured
consequences and narration (technical overview §13.14), including customs (§6.7). When events
touch a piece of lore, it is marked stale; the content director
rewrites it from the current version, the events and the present state (the same request,
validation, cache and fallback as all generated text, §3 and technical overview §13.1). To
keep the cost to what the player meets, revisions are lazy: a stale settlement is rewritten
when the player comes near it or asks about it, a faction when its annals are read, a culture
when enough has piled up; only major events are written up at once (and become rumours and
news). Without an LLM, templates fill in from the events ("{settlement} was burned by {faction}
in {year}"), and the tests replay recorded responses.

**Who tells it.** The same events read differently by whom you ask: each culture and faction
keeps its own version (a victory to one is a massacre to the other; propaganda, myths,
songs). Knowledge travels: people know what happened near them and recently, and hear of the
rest later and garbled, along trade routes and roads (rumours), so the player learns of a war
from refugees before any herald. The player's deeds become a **legend** per culture
(reputation, songs, place names, monuments, enemies who remember), told differently by
friends and foes.

**Cost and authority.** The chronicle is small (events, not text); revisions are generated
rarely and cached in the save; the simulation stays authoritative and the director only
words what happened and picks among existing things. Saves keep the chronicle and every lore
version.

**Scope.** Batch 4 writes lore once (recorded responses). The chronicle and versioned lore
come with the first systems that make events: faction conflict and trade (Batches 7, 9), with
the generated planetary history (wasteland and ruins) when a planet first needs one.

## 9. Open questions

- Planet size: decided (§8a); sizes vary per planet.
- How cultures interact (trade, uplift, conflict): Batches 7 and 9.
