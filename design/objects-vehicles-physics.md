# Objects, vehicles and the physics core

Design for Batch 1 tasks 1.10–1.13, written so later batches extend it rather than replace
it. Engine-agnostic. Builds on the object model in technical overview §14 and the decisions
in [universe-scale-time-and-flight.md](universe-scale-time-and-flight.md).

**Principle:** the core abstractions are built in their final form now; only the features
that hang off them wait for the batch that needs them.

---

## 1. Objects

### 1.1 Identity

- **`ObjectId`:** a 64-bit identity, never reused, separate from the temporary ECS entity
  ID. Anything that refers to an object across time or regions (camera, HUD, quests, NPC
  memory, saves) uses the `ObjectId`.
- **Allocator:** hands out increasing IDs; it's part of the save from Batch 3 so IDs are never
  reused across sessions.

### 1.2 Frame-relative state

Every placed object has a **state relative to a frame** in the frame tree:

| Field | Meaning |
|---|---|
| `frame` | The frame the state is expressed in (a body's inertial frame, or its surface frame) |
| `position` | `f64` position of the object's origin in that frame |
| `rotation` | Orientation (object axes → frame axes) |
| `velocity` | Velocity **relative to the frame** (an object at rest on the ground has zero velocity in the surface frame) |
| `angular_velocity` | Spin relative to the frame, in frame axes |

Converting a state to another frame uses the frame tree's full transform (position, rotation,
velocity and spin of one frame relative to the other), so nothing jumps when the reference
changes.

### 1.3 Regions

- **One region per celestial body**, covering its sphere of influence minus the spheres of
  its children. The star's region is everything else. Each region has its own ECS world
  (development plan §2), and regions tick in parallel.
- **Gravity follows the region (patched conics):** an object feels only its region's body.
- Objects in a region express their state in that body's frames: usually the inertial frame
  (flight, orbit); the surface frame for things on the ground (from Batch 2).

### 1.4 Registry and transfers

The **object registry** maps each `ObjectId` to the region and entity that currently hold it.

- **Invariant: every object is in exactly one region.**
- **Transfers** between regions are queued during a tick and applied at the **tick
  boundary**: the object's components move to the new region's world, its state is converted
  to the new frame, and the registry is updated. Its `ObjectId` never changes.
- **When to transfer:** an object enters a child body's region when it comes inside that
  body's sphere of influence, and leaves when it goes outside. A small **hysteresis margin**
  (enter below 99%, leave above 101% of the sphere's radius) stops an object on the boundary
  from flickering between regions.
- **State that refers to a body moves with it.** Flight assist's held velocity is relative to
  the region's body, so on transfer it is re-expressed relative to the new body; otherwise
  assist would see the moon's orbital speed as an error and fire the thrusters (found and fixed
  in task 1.12, covered by `sim_vehicle/tests/region_transfer.rs`).
- The same mechanism later moves objects for streaming, docking (ship into a station's
  frame) and boarding.

## 2. Vehicles

### 2.1 Parts and vehicles

A vehicle is **one object made of parts**, as technical overview §5 and §14.2 require.

- **Part catalog** (`content/data/parts/*.json`): reusable part types with their stats:
  hull sections, cockpits, fuel tanks, thrusters, reaction wheels, cruise drives. Defined once;
  mods add more.
- **Vehicle designs** (`content/data/vehicles/*.json`): a named list of placed parts, each
  referencing a catalog part with a position and orientation in the vehicle's frame.
- **Vehicle instances:** a vehicle object carries its **own** part list, copied from a design
  when spawned. Player edits (Batch 3) change the instance, never the design, so every built
  ship can be unique.

**Vehicle frame convention:** forward is −Z, up is +Y, right is +X (the same as cameras), so a
cockpit looks along −Z.

### 2.2 Part data (Batch 1 subset)

| Field | All parts |
|---|---|
| `mass_kg` | Dry mass |
| `shape` | Box (size), cylinder (radius, length along Y) or sphere (radius), used for inertia |

| Part kind | Extra fields |
|---|---|
| Thruster | `max_thrust_n`; direction of the force it applies to the vehicle (part's local axis) |
| Reaction wheel | `max_torque_nm` (any axis) |
| Cruise drive | Placeholder parameters (§5); presence means the vehicle can cruise |
| Hull, cockpit, tank | No extra fields yet |

Fuel, power, heat and damage come with their batches (§8).

### 2.3 Mass properties

On spawn and whenever parts change (Batch 3 builder), the vehicle's parts are combined into
**one rigid body**:

- total mass and center of mass
- inertia tensor about the center of mass: each part's shape inertia, rotated into the
  vehicle frame, shifted with the parallel-axis theorem, then summed

Physics works on the center of mass; the vehicle's state refers to its origin, with the
offset known from the mass properties.

## 3. Thrust allocation

Flight assist and the pilot ask for a **force and torque**; the allocator turns that into a
throttle for every actual thruster, plus reaction-wheel torque.

- Each thruster `i` at position `r_i` (relative to the center of mass) pushing along `d_i`
  with maximum `T_i` contributes force `u_i·T_i·d_i` and torque `r_i × u_i·T_i·d_i`, with
  throttle `0 ≤ u_i ≤ 1`.
- **Solve** for throttles that best achieve the requested force and torque: a
  bounded least-squares problem (6 equations, N throttles in [0, 1]), with rotation weighted
  above translation so attitude control wins when thrusters are scarce. Solved iteratively;
  small problems (tens to hundreds of thrusters) take microseconds.
- **Graceful degradation:** if the request can't be met, the allocator returns the closest
  achievable result and the shortfall, so assist can adapt. A ship with weak side thrusters
  really does strafe badly.
- **Allocation is precomputed per vehicle** (thruster geometry only changes in the builder),
  so per-tick cost is just the solve.

## 4. Flight (task 1.11)

- **Integration:** semi-implicit (symplectic) integration of position, velocity, rotation and
  spin at the fixed simulation tick, with substeps, so orbits stay stable.
- **Gravity:** from the region's body only (§1.3).
- **Time warp:** above a threshold (e.g. ×4), a coasting vehicle goes **on rails**: its state
  becomes a Kepler orbit and is evaluated in closed form, like planets. Thrusting is only
  allowed at low warp (as in KSP).
- **Flight assist modes:** full assist (holds set velocity, stops rotation), rotation damping
  only, off. The "standing still" reference switches automatically: the body's **surface
  frame** inside its atmosphere or below a set altitude, its **inertial frame** above that.
- **Collision (Batch 1):** a vehicle can't go below its body's surface radius; it's pushed
  out and its inward velocity removed. Real terrain collision and landing gear come with
  Batch 2.

## 5. Cruise drive (task 1.13, placeholder)

- **Kinematic mode:** while cruising, the vehicle ignores gravity and thrusters and moves
  along its heading at a speed set by its distance to the nearest body surface: fast in open
  space, slowing automatically on approach, with a maximum speed.
- **Engage** only outside atmospheres and away from surfaces. **Drop out** on request or near
  the target, with velocity matched to the local orbital frame, then normal flight resumes.
- Region transfers work the same while cruising.
- Tuned so neighbouring planets take 1–3 minutes (universe design §2).

**As built (task 1.13):**

| Parameter | Value |
|---|---|
| Speed | 0.25/s × distance to the nearest body surface, capped by the drive's top speed (the small drive: 200,000 km/s, two-thirds of light speed) |
| Engage | J; the ship's charge time (below); refused within 1.2× any body's drop altitude, without a drive, or with the drive overloaded |
| Drop altitude | Above the atmosphere (or 5% of the radius if airless) + 10 km; e.g. Meridian 80 km |
| Drop out | J, or automatically below a drop altitude; velocity set to a circular orbit around the nearest body, along the nose's horizontal direction |
| Steering | Rotation stick turns the ship directly, at the ship's cruise turn rate |
| Time warp | Substeps, and no step covers more than 5% of the distance to a surface, so even ×10,000 can't overshoot |
| Measured | Meridian → Hoarfell 137 s; Cinder → Leviathan (across the system) 275 s |

**Per-ship characteristics (task 3.14):** a drive part states its top speed, the ship mass it
is rated for, and its charge time and turn rate at that mass. A ship's **load** is its mass
over its drives' combined rated mass (several drives add up; the biggest sets the other
figures). Charge time and the speed ramp grow, and the turn rate falls, with the square root
of the load (loads under 0.25 count as 0.25); the top speed falls in proportion to any load
above 1; above 4 the drive won't engage. The small drive is rated for 24 t (3 s, 30°/s): the
20 t Kestrel charges in 2.8 s and turns 33°/s, three times heavier 4.8 s and 19°/s. The
builder shows the load, charge, turn and top speed, and flags an overloaded drive. Ship class
and final tuning come in Batch 9.

Because speed is proportional to distance, a ship heading straight at a body slows
exponentially and can never reach it; the drop altitude ends the approach about 15 s after
slowing begins.

## 6. Builder integration (Batch 3)

The Batch 3 builder edits a vehicle instance's part list. Everything above is already shaped
for it:

| Builder action | Core system it uses |
|---|---|
| Add, move, remove parts | Instance part list (§2.1); catalog stats |
| Show mass, center of mass, balance | Mass properties (§2.3) |
| Show "can this ship hover / strafe / turn?" | Thrust allocation (§3) and its shortfall |
| Save a ship | Object record with `ObjectId` and the instance part list (Batch 3 save format) |

Added in Batch 3: attachment graph (which part connects to which), symmetry tools, structural
joints, part templates in the UI. Section kits, connectors, slots, interiors and design
compilation: [ships-stations-interiors.md](ships-stations-interiors.md).

## 7. Invariants and tests

| Invariant | Test |
|---|---|
| Every object is in exactly one region | Registry tests over spawns, transfers, despawns |
| Transfers don't move objects | Position and velocity in a common frame identical before and after a transfer |
| No flicker at sphere boundaries | Object oscillating near a boundary transfers once, not every tick |
| Mass properties are correct | Symmetric layouts and single parts against hand-calculated values |
| Allocation meets requests within limits | Requested force/torque achieved when feasible; throttles always in [0, 1]; shortfall reported when not |
| Orbits stay stable | A coasting vehicle's orbital energy drifts by less than a set tolerance over many orbits |
| On-rails and physics agree | Switching to on-rails warp and back preserves the state |

## 8. Deferred

| Feature | Batch |
|---|---|
| Object records and saving to disk | 3 |
| Builder UI, attachment graph, symmetry, structural joints | 3 |
| Fuel, power and heat | 3 (data and estimates); consumption 7 |
| Containers and inventory | 7 |
| Procedural keys and terrain changes | 2 |
| Streaming regions in and out | 2 |
| Damage, parts breaking off | 8 |
| Atmospheric lift and drag | 2–3 |
| Per-ship cruise characteristics: from mass done in Batch 3 (task 3.14, §5); ship class and tuning | 9 |

## 9. Many ships and stations

The game will have whole fleets and many stations outside planets (X4-style). Every ship and
station uses the same object and vehicle model as the player's ship; what makes thousands of
them affordable is **simulation level of detail** (technical overview §3).

### 9.1 Simulation tiers for vehicles

| Tier | Which vehicles | Simulation | Cost |
|---|---|---|---|
| **Full** | Near the player, in combat, being watched | Rigid body, per-thruster allocation, collisions, `f64` | Highest |
| **Reduced** | Same star system, far from the player | Point mass following simple autopilot maneuvers (burn, coast on rails, arrive) using the vehicle's **performance summary** | Tiny |
| **Abstract** | Other star systems | Fleets as data: route, cargo, arrival time; no positions per tick | Almost none |

- **Performance summary:** computed with the mass properties (§2.3): maximum acceleration
  per axis, turn rates, cruise capability. Full tier uses per-thruster allocation; the other
  tiers use the summary, so a weak hauler is slow in every tier.
- **Changing tier** adds or removes physics components on the same object. On-rails coasting
  under time warp (§4) is the first use of this. Approaching the player promotes vehicles;
  arriving in an abstract system places its vehicles from their records (overview §3).

### 9.2 Stations

- Stations are vehicles: objects made of parts (often thousands); mass properties are computed
  once since they rarely change.
- **Most stations are on rails** (orbiting a body or fixed in a frame), so like planets their
  position is a formula of time and costs almost nothing per tick.
- **Large stations and capital ships get their own frame** (overview §12): docking moves a ship
  into the station's frame; interiors live there (Batch 14).

### 9.3 Fleets

- A fleet is an object: an `ObjectId` with member ships, orders and a formation.
- Formation members store their offset in the leader's frame.
- In the Abstract tier a fleet is one entry: strength, cargo, route.

### 9.4 Crowded regions

- Vehicles spread across body regions, which run on different cores (Batch 0 benchmark: 512,000
  simple ships in 2.7 ms per tick natively on the development machine).
- A single crowded place (a large battle, a busy hub) gets its own region frame (overview §11.3,
  §12), and heavy systems inside one region use parallel iteration.
- Each region keeps a **spatial index** (grid rebuilt in parallel each tick) for sensors,
  targeting and collision checks.

### 9.5 Rendering many vehicles

Only nearby Full-tier vehicles are drawn in detail; distant ones use simpler models, then
sprites, with identical models batched (overview §12.4). Floating origin keeps all of them
precise.

### 9.6 Built now so nothing blocks later

| Shaped now | Why |
|---|---|
| Physics state as components that can be added or removed | Tier changes; on-rails coasting is the first use |
| Performance summary computed with mass properties | Lower tiers and AI need it; cheap and testable |
| Regions hold any number of objects; no single-player-ship assumptions | Fleets and stations use the same paths |

Reduced and Abstract tiers, fleets, stations and spatial indexes are implemented in Batch 9
(living galaxy), when there are other ships.

## 10. Batch 1 defaults: starting ship, controls, camera

Decided 2026-11-12.

**Starting ship: Kestrel**, a small shuttle of about 20 tonnes, defined as a vehicle design
from catalog parts:

| Parts | Purpose |
|---|---|
| Cockpit, hull sections, fuel tank | Structure and mass (fuel not simulated yet) |
| 1 main engine at the rear, ~600 kN | Lift-off from Meridian at about 3× its weight |
| 16 small thrusters in 4 clusters, ~4 kN each | Strafing and fine control |
| Reaction wheel | Rotation without fuel |
| Cruise drive | Travel between planets |

**Controls** (keyboard and mouse):

| Input | Action |
|---|---|
| W / S | Thrust forward / back |
| A / D | Strafe left / right |
| Space / Ctrl | Thrust up / down |
| Mouse | Pitch and yaw (cursor locked while flying) |
| Q / E | Roll |
| T | Cycle flight-assist modes |
| J | Engage / drop out of cruise drive |
| F | Step out of a landed ship / board it again (within 12 m) |
| K | Debug: flatten a 60 m landing pad under the player (first terrain delta) |
| N | Debug arcade flight: no gravity, flies where the nose points, W/S set the held speed |
| Z | Debug full stop: instantly "standing still" for assist (hover low, drop from orbit) |
| Tab | Switch between flying the ship and the free debug camera |
| Esc | Release the mouse (click to capture again); losing window focus also releases it |
| 1–8 | Debug: teleport the ship into a parking orbit around a body (until the cruise drive) |

**Camera:** chase camera behind and above the ship, orbited by holding the right mouse button;
**V** toggles a cockpit view.
