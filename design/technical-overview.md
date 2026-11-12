# Ultra Space: Technical Overview

A hybrid space/planetary sandbox inspired by No Man's Sky, Homeworld, Starfield, Skyrim,
Dyson Sphere Program, X4 and Kerbal Space Program.

**Core pillars**

- Visit procedurally generated planets with cultures at different social stages
  (tribal, medieval, industrial, spacefaring).
- Play on foot (first person), or drive or pilot player-built vehicles: rovers, mechs,
  airships and spaceships.
- Build settlements, cities, factories and factions across planets and star systems.

Story is out of scope for now. This document covers how the game can be built technically.

Every system here has shipped in some game:

| Feature | Prior art |
|---|---|
| Seamless procedural planets | No Man's Sky, Elite Dangerous |
| Buildable vehicles with real physics | KSP, Space Engineers |
| Voxel planets with buildable ships | Space Engineers, Empyrion |
| Living economy and factions | X4 |
| Factories at huge scale | Dyson Sphere Program, Factorio |

The challenge is making all of these systems share one world, not inventing any one of them.

---

## 1. Scale: the foundation

> Concrete decisions on planet size, distances, travel, flight and time are in
> [universe-scale-time-and-flight.md](universe-scale-time-and-flight.md).

32-bit floats lose precision a few kilometers from the origin, which causes physics jitter
and "shaking" models. Planet- and system-scale worlds need several layers of fixes:

- **Floating origin:** keep the player near (0,0,0) and shift the universe around them.
  KSP, Space Engineers and Outer Wilds do this.
- **64-bit world positions** for simulation, converted to 32-bit camera-relative positions
  for rendering. Unreal 5 supports this natively (Large World Coordinates). Godot 4 can be
  compiled with double precision.
- **Nested reference frames:** galaxy → star system → planet → local grid → vehicle
  interior. Physics runs only in the local frame. Distant bodies follow analytic Kepler
  orbits ("on rails") instead of physics.
- **Scaled-space rendering:** distant planets and stars are drawn as scaled-down proxies
  so the depth buffer doesn't break down.

This layer must be built first. Every other system depends on it.

## 2. Seamless procedural planets

- **Cube-sphere with quadtree LOD:** a cube projected onto a sphere, with each face split
  into finer tiles as the camera gets closer. Terrain is generated on the GPU (compute
  shaders) from layered noise, erosion and biome rules.
- **Heightmap vs. voxel:**
  - Heightmaps are cheap and look good, but can't have caves or overhangs.
  - Voxels or SDFs (Space Engineers, Astroneer) allow digging and caves, at higher cost.
  - Middle ground: a heightmap planet, with voxel "patches" where the player digs or builds.
- **Deterministic seeds:** a planet is a seed plus rules. Only player-made changes
  (**deltas**) are saved. This keeps saves small even with huge numbers of planets.
- **Atmospheric scattering shaders** for convincing space-to-ground transitions.

## 3. Simulation level of detail

Not everything can be simulated at full detail, so simulation runs in tiers:

| Distance from player | Simulation |
|---|---|
| Near (about 2 km) | Full physics, animated NPCs, combat AI |
| Same planet or system | Simplified agents: ships follow paths without physics; combat resolved by stats |
| Elsewhere | Pure aggregate math: production and consumption rates, faction strength, trade flows ticking every few seconds |

When the player arrives somewhere, its abstract state is "promoted" into real objects.
X4, Mount & Blade and Stellaris use this approach. Faction wars, economies and off-screen
factories all run in the bottom tier.

## 4. Factories and automation

- Belts and items are **not physics objects**. A belt is an array of item slots updated in
  bulk, and rendering uses GPU instancing.
- This is a natural fit for an **Entity Component System (ECS)**, such as Unity DOTS, Bevy,
  Flecs or Unreal Mass, which can update hundreds of thousands of entities per tick.

## 5. Buildable vehicles: ships, rovers, mechs, airships

Key design decision: **every vehicle is the same kind of thing.** It is a graph of parts
connected on a grid or at snap points.

- Each part carries data: mass, hit points, power draw, thrust vector, wheel, leg, lift,
  seat, and so on.
- At runtime, **parts merge into a single rigid body** with computed mass, center of mass
  and inertia tensor. Avoid chains of physics joints, which caused KSP's "wobbly rockets."
  A craft is split into separate bodies only when it breaks apart.
- **Generic control:** input produces a desired motion. A flight or drive computer then
  distributes that motion across whatever thrusters, wheels or legs the craft has, which is
  a small control-allocation optimization. The same system drives a rover, flies a rocket
  or balances an airship using lift and buoyancy.
- **Mechs** are the hardest case. They need procedural animation, inverse kinematics (IK)
  for foot placement, and a balance controller. Start with wheels and thrusters.
- **Walking inside moving ships:** a ship's interior gets its own local physics frame, so
  the player can walk around a cockpit while the ship is in orbit.

## 6. Cultures at different tech levels

Cultures are **data-driven**, not hand-built:

- A **civilization profile** defines:
  - tech tier (tribal, medieval, industrial, spacefaring)
  - architecture kit
  - weapon and item tables
  - government type
  - attitude toward outsiders
- **Procedural settlements** use shape grammars or wave function collapse for layouts,
  filled from the culture's building kit.
- **NPC AI:** Utility AI or GOAP drives daily schedules and decisions (in the spirit of
  Skyrim's Radiant AI). AI also has levels of detail, so only nearby NPCs "think" in detail.
- Gameplay emerges from tech gaps between cultures, such as trading technology with or
  uplifting a medieval culture. This is mostly economy and faction simulation, not rendering.

## 7. Building settlements on a sphere

A flat grid can't tile a sphere. Instead, use **local tangent-plane grids**:

- Each settlement gets its own flat grid, aligned to the surface at its location.
- Terrain is flattened under foundations.
- Large cities become their own local reference frame.

## 8. Engine options

| Engine | Strengths | Weaknesses |
|---|---|---|
| Unreal Engine 5 | Best visuals; native 64-bit coordinates; World Partition; Mass ECS | Heavy; C++; steep for a solo developer |
| Unity (DOTS + HDRP) | Strong ECS for factories and simulation; large ecosystem | Floating origin must be implemented by hand |
| Godot 4 (double precision) | Free; lightweight; fast iteration | Weaker large-world rendering; more custom low-level work |
| Custom / Bevy (Rust) | Total control | Only worthwhile if building engine tech is the goal |

**Decision:** Bevy (Rust), with the simulation in separate engine-independent Rust
crates. The deciding factor is safe, automatic multi-core parallelism for large-scale
simulation. See [development-plan.md](development-plan.md) for the full rationale and the
options considered.

---

## 9. Melee, mounted combat and Mount & Blade-style factions

Much of this comes from systems already described:

- **Campaign map:** the tiered simulation (Section 3) is the Mount & Blade model. Armies
  and towns run as aggregate data. When the player meets an army, it is promoted into real
  soldiers on a battlefield. Battles the player isn't in are auto-resolved.
- **Medieval cultures:** a civilization profile (Section 6) with a medieval tech tier,
  weapon tables and architecture kit.
- **Mounts:** these use the generic vehicle control model (input → desired motion). A horse
  is a "vehicle" whose movement comes from animation instead of wheels or thrusters.
- **ECS:** already required for factories; it also handles hundreds of soldiers per battle.

### 9.1 Melee combat system (separate from gunplay)

- **Directional attacks and blocks:** attack direction comes from mouse movement, and the
  matching block direction stops it.
- **Weapon-sweep hit detection:** trace the weapon's path along its animation each frame,
  instead of the instant raycast a gun uses. Damage scales with swing speed. Mount speed adds
  a bonus (couched lances, cavalry charges).
- **Hit reactions:** stagger, interrupts, chambering, shield bashes. This is mostly
  animation state-machine work and the largest art and animation cost of the feature.
- **Camera:** first-person and third-person. Third-person reads better for melee and
  mounted combat.

### 9.2 Large battles

- **Battle size cap plus reinforcement waves:** e.g. 300–1000 soldiers on the field at once,
  refilled from the abstract army as soldiers fall.
- **Animation and AI levels of detail:** distant soldiers update less often and use cheaper
  skeletons or GPU-instanced animation.
- **Formation AI:** squads hold lines, wedges and shield walls and follow player orders.
  Individual soldiers fight duels locally. This is a separate AI layer from NPC daily
  schedules.
- **Sieges:** destructible or scripted walls, ladders and siege engines, reusing settlement
  building pieces (Section 7).

### 9.3 Character layer in the faction simulation

Aggregate math alone can't produce feudal politics. Add a lightweight agent layer between
the aggregate economy and full 3D NPCs:

- **Notable characters:** lords, generals, guild masters. They are named and persistent,
  with traits, relationships with the player, loyalty, marriages and claims to land.
- **Scale:** a few thousand notable characters per region, each making decisions on the
  campaign map (declare war, defect, marry, rebel).
- They appear as full 3D NPCs only when the player meets them.

## 10. Mixed-tech battles (tech faction vs. medieval faction)

### 10.1 Unified damage pipeline

Every attack produces a **damage event** with:

- type (slash, pierce, blunt, ballistic, energy, explosive, fire)
- amount, penetration and impact force

Armor and parts have resistances to each type. All weapons feed the same damage resolver:

| Weapon | Hit detection |
|---|---|
| Sword, spear, axe | Weapon-sweep trace |
| Arrow, bolt, bullet, rocket | Shared ECS projectile system (speed, drag, gravity settings) |
| Laser, beam | Instant raycast |
| Explosion, orbital strike | Area damage event |

Vehicles and mechs take **per-part damage** through their part graph (Section 5). A
medieval army can disable a tank by wrecking its wheels or leave a mech without power by
destroying the reactor.

### 10.2 Morale and shock

Morale is what makes asymmetric battles interesting:

- Each soldier and squad has a morale value. Unfamiliar weapons (explosions, flying
  machines) cause fear spikes, scaled by the culture's familiarity with the tech.
- Medieval armies win the way real underdogs do: numbers, terrain, night, ambush, closing
  distance quickly, and fire or traps against vehicles.

### 10.3 Supply as the balancing lever

The tech side has real limits that come from the abstract economy simulation:

- ammo, power cells, fuel, maintenance and supply lines
- an outpost cut off from resupply gradually runs out of ammunition

Supply is the main way asymmetric battles are balanced, and it comes from systems that
already exist.

### 10.4 Doctrine-based AI

Both sides use the same agent AI with different **doctrine profiles** tied to tech tier:

- **Medieval:** formations, charges, shield walls, cavalry flanking.
- **Tech:** cover-seeking, suppression fire, keeping distance, calling vehicles or air
  support.
- **Adaptation:** factions that have fought advanced weapons before switch tactics
  (dispersed formations, night raids). This is a flag stored on the faction.

### 10.5 Range scale and LOD

- Melee happens at about 2 m; rifles at 500 m or more. Mixed-tech battle maps need to be
  larger, which streamed planet terrain (Section 2) supports.
- LOD rules key off distance *and* relevance. A unit the player is aiming at through a scope
  stays at full detail.

### 10.6 Auto-resolve

Each unit has a power rating with counter modifiers: cavalry vs. infantry, armor vs. small
arms, numbers vs. ammo supply. Off-screen mixed-tech wars resolve from these.

## 11. Orbital strikes and layered battles

### 11.1 Strikes are simulation events, not physics objects

Ships in orbit and soldiers on the ground are in different reference frames (Section 1). An
orbital strike is never simulated as a physics projectile across that distance. Instead:

1. **Targeting:** the target is stored in **planet-fixed 64-bit coordinates**. The planet
   rotates, so the target must be fixed to the surface, not to space.
2. **Firing window:** orbital mechanics determine whether the ship is overhead (orbits on
   rails), unless it's in a stationary orbit. Example: "the next strike window is in
   4 minutes."
3. **Flight time and attenuation:**
   - Lasers are near-instant but weakened by atmosphere, clouds and smoke.
   - Kinetic rods take time to fall and can be seen incoming.
4. **Impact resolution:**
   - **Near the player:** a real explosion in the local frame, with a damage event, physics
     impulse, crater and debris.
   - **Elsewhere:** abstract resolution, e.g. "settlement loses 30% of its buildings,
     garrison morale breaks."

### 11.2 Persistent destruction

Craters and destroyed structures are saved as **deltas** (Section 2): voxel patches for
terrain and damage states for buildings. A bombarded town stays ruined when the player
returns.

### 11.3 Layered battles

A planetary invasion is a single battle object with three linked layers:

| Layer | Content | Effect on the other layers |
|---|---|---|
| **Orbit** | Fleet battle (Homeworld-style) | Controlling orbit unlocks bombardment and landings |
| **Air** | Fighters, airships, dropships | Air superiority unlocks close air support |
| **Ground** | Infantry, cavalry, vehicles | Ground spotters and anti-air make the upper layers more or less effective |

- The layer the player is in runs at full detail. The other layers run in a cheaper mode and
  communicate through events (e.g. "orbital superiority achieved, strikes available").
- The player can fight the space battle while the ground war auto-resolves, or fight on the
  ground with the space battle visible overhead.

### 11.4 Counterplay and consequences

- **Tech defenders:** planetary shields, anti-air, ground-to-orbit missiles, orbital defense
  platforms, jamming target designators.
- **Medieval defenders:** they can't shoot back, so their counterplay is on the ground: kill
  spotters or designators, hide in caves and forests, mix with civilians, take hostages.
- **Diplomacy:** bombarding a primitive culture can carry reputation penalties with other
  factions, through the character layer (Section 9.3).

### 11.5 Ground designation

Accurate strikes require a **ground designator** (laser pointer, beacon or scout). This ties
the layers together and gives first-person play a role inside large battles.

### 11.6 New systems summary

| System | Builds on |
|---|---|
| Unified damage-event pipeline | ECS, vehicle part graph |
| Morale and doctrine AI | NPC AI, civilization profiles |
| Supply/ammo link from economy to combat | Tiered simulation |
| Layered battle object with cross-frame strike events | Reference frames, delta saves, tiered simulation |

---

## 12. Capital ships, hull walking, EVA and boarding

Inspired by X4: the player can pilot a huge battleship, stand on its outer hull to watch a
space battle, or EVA to other ships and stations.

### 12.1 Frame tree

The nested reference frames from Section 1 become a **frame tree**. Any ship or station
can be a frame that other things live inside:

```
Star system frame (64-bit, orbits on rails)
 └─ Battle region frame (e.g. a 50 km bubble around the fight)
     ├─ Player battleship frame
     │   ├─ Interior: bridge, corridors, crew, player
     │   ├─ Outer hull: magnetic-boot surface
     │   └─ Hangar → fighters docked inside
     ├─ Enemy carrier frame
     │   └─ Interior (not loaded until boarded)
     ├─ Station frame
     └─ Player on EVA (a small rigid body in the region frame)
```

- **Moving between frames means reparenting:**
  - Exiting an airlock: ship frame → battle region frame.
  - Flying into a hangar: region frame → carrier frame.
  - Docking: ship frame becomes a child of the station frame.
- Each switch converts position and velocity between frames, so it is seamless: an object
  leaving a ship keeps the velocity it had on exit.
- The floating origin always follows the player, wherever in the tree they are.

### 12.2 Standing on a moving ship

- The capital ship is **one rigid body** (part graph merged, Section 5) moving through the
  battle region.
- **The interior is simulated in ship-local coordinates.** The physics engine never sees the
  ship's real velocity inside, so crew, cargo and the player stay stable even at high speed.
- **Gravity is per volume:**
  - Interior: artificial gravity with a constant "down" in ship-local space.
  - Outer hull: no gravity. **Magnetic boots** align the player to the hull's surface
    direction. Stepping off the hull starts EVA.
- **Fictitious forces (optional):** hard maneuvers or heavy hits apply a scripted jolt or
  stagger to interior occupants. This gives the feel of motion without physics instability.
- **Hull breaches:** the interior is a **room graph of atmosphere volumes**:
  - A breach vents a room and pulls loose objects and players toward the hole.
  - Doors seal automatically.
  - This connects per-part damage (Section 10.1) to the interior.

### 12.3 Bridge control stations

Sitting in a station switches the player's input mode:

| Station | Input mode |
|---|---|
| Helm | Direct flight, using the generic thruster control (Section 5) |
| Turret seat | Manual gunnery for one turret; the others stay AI-controlled |
| Command table | **Tactical RTS view** (Homeworld-style): orders for the fleet |
| Walk away | The ship's AI captain carries out the last orders |

Orders are data, so the player can issue them from the bridge, then walk to the hull and
watch them play out.

### 12.4 Watching the battle: simulation vs. visuals

- **Simulation and visuals are kept separate** (X4's "high attention vs. low attention"):
  - **High attention** (near the player, or being looked at): full projectile simulation,
    physics, per-part damage.
  - **Low attention:** stat-based hit rolls, while **still showing** tracers, beams and
    explosions as visual-only effects.
- **What the player looks at counts:** looking at a distant ship through binoculars or a
  turret scope promotes it to high attention.
- **Large-scale rendering:**
  - GPU-instanced tracers and missiles.
  - Lower-detail meshes, then flat impostors, for distant ships.
  - Explosion effects whose detail scales with distance.
  - Logarithmic or reversed depth buffer, so 5 m to 50 km fits in one view.

### 12.5 EVA between ships

- On EVA the player is a small rigid body with a **thruster pack**, flown with the same
  generic thruster control.
- **The "too slow" problem.** Capital ships fight 2–10 km apart; at 10 m/s, a crossing
  takes 3–15 minutes. Options that keep it realistic but playable:
  - **EVA sled or boarding pod:** a small buildable vehicle.
  - **Grapple or tether lines** for short hops along hulls and between nearby ships.
  - **Booster packs** with limited fuel for fast but risky crossings.
  - **Time compression** when out of combat (single-player only).

### 12.6 Boarding actions

- Marines cross in boarding pods or on EVA, breach hull points, and fight inside the enemy
  ship using the **same first-person and melee combat systems** as on planets
  (Sections 9–10).
- The enemy interior loads as the player approaches the breach point, like streaming in a
  level.
- Without the player, boarding falls back to the stat-based resolver (crew vs. marine
  strength).

### 12.7 Streaming and performance

- **Interiors:** only the interior the player is in runs at full detail. Other interiors load
  only when the player approaches an airlock or breach point.
- **Crew as two tiers:**
  - Player aboard: 3D agents with jobs (e.g. repair crews running to damaged sections).
  - Player not aboard: crew is stats: skill, morale, headcount.
- **Hangars:** docked fighters are frozen children of the carrier frame with no physics.
  When launched, they are reparented into the battle region and become active.

### 12.8 Integration summary

| Need | Existing system it reuses |
|---|---|
| Ships, stations and hulls as frames | Nested reference frames, generalized into a tree |
| Capital ship flight and damage | Vehicle part graph with per-part damage |
| Fleet command | Doctrine AI and the orbit battle layer (Section 11.3) |
| Boarding combat | FPS/melee combat and the unified damage pipeline |
| Distant battle detail | Simulation LOD (attention levels) |
| Crew | Two-tier agents (3D when present, stats when absent) |

**New systems:** reparenting between frames, per-volume gravity with magnetic boots,
room-graph atmosphere and breaches, visual-only effects for low-attention combat, and
streaming ship interiors.

---

## 13. AI / LLM systems

**Core principle:** use the LLM to create, interpret and describe things, but never let it
decide what is true in the game. The simulation stays authoritative. The LLM produces
cultures, creatures, dialogue and events as **structured data the game validates**. In short:
the LLM proposes, the simulation decides.

### 13.1 AI gateway architecture

```
Game simulation (authoritative)
   │  facts, state, allowed actions
   ▼
AI Gateway
   ├─ Context builder    (relevant facts: character card, faction state, memories)
   ├─ Prompt templates   (stored as data files; moddable)
   ├─ Schema validator   (rejects or clamps rule-breaking output)
   ├─ Cache + save store (generated content becomes part of the save)
   ├─ Budget / rate limiter
   └─ Fallback           (templates / procedural content when no model is available)
   │
   ▼
Model backends: cloud APIs (Anthropic, OpenAI) | build-time batch generation | replay (tests)
```

Rules:

- **Never block a frame.** All calls are asynchronous. Content is generated ahead of time;
  for example, a system's planet cultures start generating when the player enters the system.
- **What's generated becomes canon.** LLM output is not reproducible from a seed, so it is
  saved like terrain deltas (Section 2). Once written, a culture's history is fixed.
- **Output is structured, not free text:** schema-checked JSON, validated against tech tier,
  available assets and balance limits.
- **Fallbacks:** on validation failure, retry once, then use procedural or template content.
  The game must be fully playable offline with no model.
- **Prompts are data**, so modders can change culture styles and NPC personalities.

### 13.2 Cultures

- The LLM fills in the **civilization profile** (Section 6): values, taboos, religion,
  government, history summary, naming style, aesthetic choices.
- It **chooses among existing asset kits and parameters** (architecture kit, palette,
  clothing set). It does not invent meshes.
- It generates **naming rules** for each culture's language; names are then produced
  procedurally from those rules.
- **Validation:** content must match the tech tier (e.g. no gunpowder in a medieval culture
  unless the profile allows early firearms).

### 13.3 Creatures

- The LLM writes a **creature spec**: body plan from a procedural part library (legs, size,
  armor, diet), ecological role (grazer, pack hunter, ambush predator), utility-AI behavior
  profile, description text.
- The **ecosystem simulation validates it** (predator/prey balance, size vs. gravity) and
  requests changes if needed.
- Animation and movement come from procedural rigs and IK; the LLM only sets parameters.

### 13.4 Dialogue

- **Only notable characters** (Section 9.3) use the LLM. Generic NPCs use templated lines.
- **Context pack for each line:** character card (traits, goals, speech style), relevant
  memories, current faction state from the simulation, relationship with the player, and
  world facts retrieved from a lore database.
- **Actions through tool calls:** the LLM can say anything but can only *do* allowed
  actions, e.g. `offer_quest`, `trade`, `change_opinion(±small)`, `refuse`, `call_guards`,
  `share_rumor`. The game validates every action.
- **Memory:** after each conversation, a short summary is written to that NPC's memory.
- **Negotiation (translation pattern):** player free text → LLM converts it to a structured
  offer → faction utility AI decides → LLM writes the reply in character. The LLM never makes
  the decision.
- **Voice (optional):** speech recognition for player input and streamed text-to-speech per
  character voice.

### 13.5 Vehicles

- **Design assistant:** a request such as "a two-seat rover that climbs 30° slopes" → the LLM
  writes a part graph (Section 5) → a **physics validator** checks thrust-to-weight, center
  of mass and power budget → errors go back to the LLM → repeat until it passes. The player
  then edits the result in the normal builder.
- **AI factions use the same pipeline** to design ships in their culture's style, generated
  once and cached.
- Also generates names, manufacturer descriptions and ship-class histories.

### 13.6 Game events: an LLM "director"

- The **simulation detects possible events**: famine, contested succession, a derelict ship,
  a rebellion forming.
- An **LLM director** picks which to promote, based on pacing and player history, and
  **dramatizes** them: quest text, characters involved, rumors.
- **Quests are graphs of objectives** built only from supported verbs (go to, retrieve,
  escort, defend, persuade, sabotage). The LLM fills in details; the quest system runs them.
- Major events get **chronicle entries**, so each save builds its own history.

### 13.7 Generating the universe: top-down and lazy

```
Galaxy history ──► Regions ──► Star systems ──► Planets ──► Sites/settlements ──► Characters/items
  (once per save)   (on approach)  (on entry)     (on landing)   (on discovery)       (on interaction)
```

- **Each level is conditioned on the one above:** a planet's lore knows its system's
  history, which knows the galaxy's history.
- **Lazy generation:** nothing is generated until a player is about to see it, so cost
  scales with what players actually visit.
- **Saved once generated:** output becomes canon in the save.

### 13.8 What can be generated at each level

| Level | LLM generates | Rules/procedural generate |
|---|---|---|
| **Galaxy** | Deep history timeline: precursor civilizations, ancient wars, collapses, migrations. Region themes. | Star positions, spiral arms, star density |
| **Star system** | System concept and story hook, names, anomalies, who claims it and why | Star type, orbits, planet count, asteroid belts (astrophysics rules) |
| **Planet** | Planet concept → chosen parameters (biome mix, climate), landmarks, habitability rationale, codex text | Terrain noise, erosion, atmosphere physics, gravity, day length |
| **Points of interest** | Ruins and their story, derelict ships and their logs, crash sites, shrines, hidden caches with clues | Placement on terrain, layout from building kits |
| **Settlements** | Local politics, trade specialty, problems, festivals, laws and crimes, rumors | Layout (grammars / wave function collapse), population from simulation |
| **Factions & corporations** | Ideology, leadership, rivalries, founding myth, propaganda, slogans, ship naming styles | Strength, territory, economy |
| **Religions & myths** | Gods, creation stories, rituals, taboos, holy sites, sects and splits | Spread via the culture simulation |
| **Languages** | Sound and spelling rules and style per culture | Name and word generation from those rules |
| **Technologies** | Tech descriptions, each culture's own names and explanations for technologies | Mechanics from templates; balance numbers clamped by tier |
| **Items & artifacts** | Unique artifacts with history and maker; books, letters, scrolls, data pads | Stats from loot tables |
| **Characters** | Personalities, backstories, companions, crew quirks, rivals who remember the player | Skills, stats, schedules |
| **Media & world chatter** | News broadcasts, faction radio, propaganda posters, tavern songs, bards singing about the player's deeds, graffiti | Timing and placement, triggered by simulation events |
| **Save history** | Chronicles of wars, the player's legend, faction annals, an end-of-game history book | The actual events (from simulation) |

### 13.9 What the LLM should not generate

- **Orbits, terrain heightmaps, physics values:** use real rules.
- **Balance numbers** (damage, prices, production rates): use tables, or clamp proposals to
  tier-based limits.
- **Economy and simulation state:** the LLM only describes it, never sets the numbers.
- **Anything needed every frame:** the LLM is for slow, high-value content.

### 13.10 Other generative AI besides LLMs

| Model type | Use | Maturity |
|---|---|---|
| **Image models** | Flags, heraldry, posters, book covers, paintings, alien plant textures, codex illustrations | Usable; best offline or cached, with art direction rules |
| **Text-to-speech** | Character voices per culture, reading LLM-written dialogue | Usable at runtime |
| **Music models** | Culture-specific themes, tavern music, ambient tracks | Usable offline; runtime still costly |
| **Text-to-3D** | Props, artifacts, decorative items | Improving; offline, with human cleanup for important assets |

### 13.11 Example generation chain

> Galaxy history: a precursor empire collapsed 10,000 years ago → a system is generated as one
> of its abandoned colonies → planet concept "jungle reclaiming megastructures" (biome
> parameters for the terrain generator) → a ruin site with a derelict archive and readable
> logs → the local tribal culture worships the ruins as gods' tombs (religion) → a priest NPC
> warns the player not to enter (dialogue) → the player takes an artifact → the tribe's
> opinion of the player drops (simulation) → a bard in a nearby town later sings about
> "the sky-thief" (media).

Each step uses the output of the step before, so the galaxy reads as if one author wrote it.

### 13.12 Model backends

| Workload | Backend | Why |
|---|---|---|
| Content shipped with the game (base cultures, lore, item text) | Large cloud model, **batch processing** at build time | Highest quality; batch is cheaper; no runtime cost |
| Per-planet cultures, creatures, faction ship designs | Cloud model in the background on discovery, cached forever | One-time cost per planet; latency hidden |
| Live NPC dialogue | Small, fast cloud model | Low latency and low cost per line |
| Offline / no account / no key | Templates and procedural content | Game stays fully playable |

- **Providers:** OpenAI and Anthropic APIs through a provider interface, plus a replay
  adapter for tests. No local models. See
  [development-plan.md](development-plan.md#6-llm-integration-cloud-apis) for API key
  handling and cost control.

- **Prompt caching:** keep the world bible, culture profile and character card as a stable
  prompt prefix so repeat calls cost much less.
- **Running cost is a business decision.** Cloud calls cost money per player. Options: a
  subscription tier, "bring your own API key," or limiting cloud calls to rare, high-value
  moments.

### 13.13 Risks and mitigations

| Risk | Mitigation |
|---|---|
| Players trick NPCs into breaking the game | Allowed-action list validated by the game; worst case is an odd line, never a broken state |
| Contradicting the lore | Lore database as source of truth, fact retrieval, consistency checks on generated history |
| Latency | Generate ahead, stream text, show filler (NPC "thinking" animation, idle lines) |
| Bland or repetitive output | Strong character cards, per-culture variety settings, recent outputs kept in context |
| Inappropriate content | Moderation on player input and model output; a rating setting |
| Regressions | Test sets of scenarios with checks that responses stay in character and valid |

---

## 14. Object model, persistence and streaming

Goal: Bethesda-style persistence (anything the player moves, drops, builds or kills stays that
way) at galaxy scale, **without loading screens**.

### 14.1 Lessons from Creation Engine (Starfield)

Commonly cited causes of Starfield's frequent loading screens. Outside analysis can't be
certain, but these are widely agreed on:

| Cause | Effect |
|---|---|
| Cell-based world | Interiors, ship interiors, landing zones and space are separate cells or worldspaces; every move between them is an unload/load |
| 32-bit coordinates | Worldspace size is limited, so a planet can't be continuous and space can't contain the surface |
| Heavy objects | Each placed reference carries scripts, physics and 3D state; loading a cell sets up thousands of them, much of it on the main thread |
| Persistent references kept loaded | Quest-relevant objects stay in memory globally, so load work and memory grow with content |
| Engine heritage | Built around discrete cells; space and planets were added on top rather than rebuilding the foundations |

**Design stance:** Creation Engine treats places as rooms you enter. This game treats the
universe as **one continuous space whose detail streams in and out.**

### 14.2 Object model

**Base definitions:** templates in `content/data/` with stable, namespaced IDs
(`item.iron_sword`, `part.thruster.small`). They define stats, model and value once. Mods
override or add definitions in load order.

**Persistent identity:** every object that can persist has an `ObjectId`, a 64-bit value that
is never reused. It is separate from the temporary internal ECS entity ID. Saves, quests,
ownership and NPC memories refer to `ObjectId`s.

**Three levels of existence:**

| Level | Example | Stored as | Cost |
|---|---|---|---|
| **Active** | A sword the player just dropped nearby | Full entity in its region's world, with physics | Highest |
| **Persistent, unloaded** | The player's stash on another planet | Compact record: ID, base, placement, state | Small |
| **Procedural, untouched** | Rocks, trees and ruins nobody has interacted with | **Nothing:** regenerated from seeds | Zero |

Interacting with a procedural object (mining a rock, looting a ruin) makes it persistent: the
change is saved as a delta (Section 2).

**Inventory and containers:**

- Stackable generic items (ore, ammo) are counts in a container, with no `ObjectId`.
- Unique items (named weapons, artifacts with generated histories) keep their `ObjectId` in
  inventories, so they stay the same object when dropped, sold or stolen.

**Built structures:** a ship or base is **one** persistent object holding its part graph
(Section 5). Parts become separate objects only when they break off.

**Moving between regions:** the player and any other object move between regions by being
removed from one region's world at a tick boundary and spawned into the next (matching
frame-tree reparenting, Section 12).

**Quests and scripts reference IDs, not loaded objects.** A quest can check an object's record
("is the artifact in the player's inventory?") without loading it.

**Physics for clutter:** only objects near the player have active physics. Objects at rest
sleep, and become records when their region unloads.

### 14.3 Saves

- **Per region:** changed and created objects, IDs of destroyed objects, removed procedural
  objects, and the region's procedural seed. Loading one planet never reads the whole galaxy.
- **Versioned:** each save file starts with a format version. Older saves are upgraded by a
  chain of migrations; saves from newer game versions are refused with a clear error.
- **Golden files:** a saved fixture from each format version stays in the repo, and tests
  check it still loads after every change.
- **Bloat control:** junk dropped outside player-owned areas expires after some in-game time;
  anything in a player home, base, ship or container persists forever.

### 14.4 Streaming rules

| Rule | Purpose |
|---|---|
| **Never block a frame on loading** | All loading and generation runs on background threads |
| **Build regions in the background** | A region's whole world is built on a worker thread, then handed to the simulation at a tick boundary |
| **Spawn gradually** | Promoting records to active entities happens a set number per frame, never thousands at once |
| **Prefetch from travel** | Approaching a planet or ship starts streaming its terrain, regions and assets early; travel time hides loading |
| **Level of detail everywhere** | Terrain, models, textures, AI and simulation fidelity all scale with distance |
| **Warm up GPU pipelines** | Precompile shaders and pipelines at startup or during travel, avoiding first-use stutter |
| **Disguised transitions only as a deliberate fallback** | For a place too dense to stream smoothly (a huge city, a capital ship), an airlock cycle, elevator or docking clamp may hide a short load. A design choice for specific places, not a limit that applies to every door |

### 14.5 Remaining risks

| Risk | Mitigation |
|---|---|
| Spawn spikes | Gradual spawning with a per-frame budget |
| Shader compilation stutter | Pipeline warm-up |
| Asset bandwidth and video memory | Level-of-detail tiers, compressed textures, predictive prefetch |
| Dense places exceed streaming capacity | Disguised transitions, chosen per place |
| Bevy's asset streaming is less proven than older engines' | Measure early with the hitch budget below |
| Save size growth | Per-region saves, compression, expiry rules |

### 14.6 Hitch budget

"No loading screens" is a measurable requirement:

- **No frame over 50 ms during any transition:** orbit → surface, entering or leaving a ship,
  docking, entering a settlement.
- **Stress test (Batches 1–2):** fly from orbit to the surface and into a settlement while
  recording frame times with the debug overlay; fix any spike over budget before moving on.

---

## 15. Scope and roadmap

> The ordered development batches, with estimates and checkpoints, are in
> [development-plan.md](development-plan.md#7-development-batches). The milestones below are
> the design-level vertical slices those batches deliver.

The full scope is studio-sized. To keep it achievable, build it as **vertical slices**,
where each milestone is a playable toy on its own:

1. **One planet: walk → fly → orbit, with no loading screens.** Proves the floating origin,
   planet LOD and orbital rails.
2. **Vehicle builder:** snap parts together, then drive or fly the result. Proves the
   single rigid body and control allocation.
3. **A tiny base:** place buildings, run one production chain, save and load the changes.
4. **A second planet plus abstract simulation:** a faction there grows or trades while the
   player is away.
5. **One populated culture:** a procedural village with simple NPC schedules and on-foot
   first-person combat.
6. **One medieval kingdom:** directional melee duels, then mounted combat, then a
   200-soldier field battle promoted from the campaign map. Proves the melee system,
   formation AI and battle promotion.
7. **Mixed-tech skirmish:** a small tech squad with limited ammo vs. a medieval warband.
   Proves the unified damage pipeline, morale and the supply link.
8. **Orbital layer:** a ship in orbit calls a designated strike on a ground battle, with a
   persistent crater. Proves cross-frame strike events and layered battles.
9. **Capital ship battle:** walk from a battleship's bridge out onto the hull during a
   20-ship battle, EVA to a nearby ship, and breach it. Proves the frame tree with
   reparenting, hull walking, attention-based battle LOD and boarding.
10. **LLM-driven world:** one lord the player can talk to and negotiate with (character
    profile, memory, allowed actions), a planet with an LLM-generated culture and religion,
    and a director-generated event (e.g. a succession crisis) from simulation state. Proves the
    AI gateway, validation, caching and fallbacks.
11. **Broaden:** more tech tiers, mechs, airships, more star systems, sieges, faction
   diplomacy and the character layer.
