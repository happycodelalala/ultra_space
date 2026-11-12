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

**Recommendation:** Unity or Godot for a solo developer or small team; Unreal 5 for a
visually ambitious team.

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

## 12. Scope and roadmap

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
9. **Broaden:** more tech tiers, mechs, airships, more star systems, sieges, faction
   diplomacy and the character layer.
