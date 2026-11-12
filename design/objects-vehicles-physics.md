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
joints, part templates in the UI.

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
| Fuel, power and heat | 3–4 |
| Containers and inventory | 4 |
| Procedural keys and terrain changes | 2 |
| Streaming regions in and out | 2 |
| Damage, parts breaking off | 5 |
| Atmospheric lift and drag | 2–3 |
