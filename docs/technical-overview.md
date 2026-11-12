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

## 9. Scope and roadmap

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
6. **Broaden:** more tech tiers, mechs, airships, more star systems, faction diplomacy.
