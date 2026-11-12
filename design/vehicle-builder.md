# Vehicle builder, rigid-body physics and saving (Batch 3)

How the player builds rovers and ships from parts, how built vehicles drive, fly, land and
collide, and how the world is saved. Builds on
[objects-vehicles-physics.md](objects-vehicles-physics.md) (parts, mass properties, thrust
allocation), [ships-stations-interiors.md](ships-stations-interiors.md) (building levels, design
compilation, data format additions) and [planet-surface.md](planet-surface.md) (terrain, landing,
walking). Tasks are in development plan §7.4.

## 1. Decisions

Decided with the developer on 2026-11-12.

| Question | Decision | Why |
|---|---|---|
| Physics | **rapier3d-f64** (0.36) in the simulation crates: rigid bodies, compound colliders, contacts, joints, raycast wheels, character controller | Built vehicles need real contacts (wheels, legs, hulls on the ground, later vehicle against vehicle) from the start. Rapier is engine-independent, `f64`, and uses the same glam as our code |
| Where to build | **Build mode anywhere:** a key next to a landed vehicle (or on open ground for a new one) opens the builder around it, in place | No shipyards until Batch 6. Building in the world shows the real ground, light and scale |
| Costs | Parts are free and unlimited in Batch 3 | Resources and shops come in Batches 4 and 6 |
| Fuel and power | Tanks and batteries have capacity and mass in the data; the builder shows estimates (Δv, range); **nothing runs dry until Batch 4** | Running out only matters once the player can refuel from gathered resources |
| Interiors | In the data format now (ships-stations-interiors §8); walking inside comes after the builder | Keeps Batch 3 on building, driving, flying and saving |
| UI | Bevy's own UI (`bevy_ui`, `bevy_ui_widgets`, `bevy_feathers`) and picking | Built into the pinned Bevy; no third-party UI crate to keep in step |
| Save format | Versioned **JSON** with a format version and a migration hook (development plan §7.1); compressed later if size needs it | Readable diffs while the format changes; same tooling as content |

## 2. Physics with rapier

### 2.1 One physics world per region

- Each region (one per body, objects-vehicles-physics §1.3) owns a rapier world: bodies,
  colliders, joints, broad phase and the query pipeline. Regions still tick in parallel; rapier
  itself runs single-threaded inside a region (its `parallel` feature stays off until a crowded
  region needs it).
- **Coordinates:** the body's inertial frame, `f64`, as now. At 600 km from the centre `f64`
  keeps sub-micrometre precision.
- **Gravity:** rapier's uniform gravity is zero; each dynamic body gets the force `−μ m r̂/r²`
  every step (patched conics, as now).
- **Which objects get a rapier body:** vehicles and walkers in the Full tier (objects-vehicles-
  physics §9.1). On-rails time warp, cruise and arcade flight keep their own kinematic movement;
  the rapier body follows them as kinematic and turns dynamic again when they end.
- **Region transfers** move the rigid body and its colliders to the new region's world, with
  state converted between frames as now; the `ObjectId` stays.
- **Orbits stay stable:** the orbital-energy and region-transfer tests from Batch 1 must keep
  passing with the rapier integrator (substeps as needed).

**As built (3.1–3.2):**

- Rapier splits each step into solver iterations that share one force. In free flight a step
  is one iteration (1/60 s substeps), so gravity is re-evaluated every substep and orbits stay
  symplectic; while any body is near the ground, steps use rapier's usual four iterations for
  good contacts and friction.
- Rapier's default 400 m/s speed cap is lifted (orbits need thousands).
- The object state stays the authority between ticks: each tick writes every vehicle into the
  scene and reads it back, so transfers and teleports need no extra bookkeeping.
- **Landed vehicles rest:** with idle controls on the ground, flight assist switches the
  engines off; after a second still on the ground with no thrust, the body is pinned to the
  turning ground (kinematic, exactly co-rotating, nearly free to simulate). Thrust, a terrain
  change under it or another body touching it wakes it, moving with the ground. Rapier's
  friction alone let a ship creep a few millimetres per second on a slope.

### 2.2 Terrain collision

- The ground is a set of **collision patches**: small triangle meshes (e.g. 64 × 64 m, 2 m
  grid) built from the terrain function, deltas included, so physics sees exactly the drawn
  heights.
- Patches are attached to one **kinematic body per region that turns with the planet**, so a
  contact's ground velocity is `spin × r` and landed vehicles ride along through friction, as
  in Batch 2.
- Patches are built around each body near the ground (radius plus 20 m, plus 1.5 s of travel)
  on two background threads (~4 ms each), cached by key and dropped 10 s after the last use; a
  body over ground without patches yet uses the Batch 2 height-function contact as a fallback.
- Adding a delta (a pad) rebuilds only the patches it touches; each old patch stays until its
  replacement is ready.
- The sea surface stays a floor for now.

### 2.3 Vehicles as rigid bodies

- A vehicle is **one rigid body** with a **compound collider** made from its parts' shapes
  (box, cylinder, sphere; simplified hulls later). No joints between structural parts.
- Mass and inertia come from our own mass properties (objects-vehicles-physics §2.3), set on
  the body directly so the builder, the allocator and physics agree.
- Thrusters apply the allocator's force and torque at the centre of mass each step; flight
  assist works as now.
- **Landing gear:** landing legs are parts with a sphere or capsule collider and suspension
  values; the Batch 2 ground contact (Coulomb friction, stick on slopes) is replaced by rapier
  contacts with friction from the material pair.
- Atmospheric drag stays our own force (planet-surface §5).

### 2.4 Wheels and rovers

- **Wheel parts:** radius, width, suspension rest length, stiffness and damping, maximum motor
  torque, maximum steering angle, brake torque, friction.
- A vehicle with wheels uses rapier's **raycast vehicle controller**: each wheel casts against
  the terrain patches and other colliders, and the controller applies suspension, drive,
  steering and friction forces to the one rigid body.
- Rover controls: W/S throttle and brake/reverse, A/D steer, Space handbrake. The same vehicle
  can have both wheels and thrusters (a hopper).

### 2.5 Walker

- The walker moves with rapier's **kinematic character controller**, so it collides with
  vehicles and can stand on them, with slope limits and steps (planet-surface §5 deferred these).
- It stays kinematic: gravity, jumps and riding the turning ground work as in Batch 2.

## 3. Data format additions

Added to the part and vehicle formats (ships-stations-interiors §8), with JSON Schemas:

| Addition | Batch 3 use |
|---|---|
| **Attach nodes** on parts: position, direction, size | Snapping parts together |
| **Attachment graph** in vehicles: each placed part names its parent part and the nodes joined | Validation (everything connected), removing a part removes what hangs off it |
| **Connectors** (size class, profile, interior passage) | Sections; joining interiors later |
| **Composite parts (sections):** a named group of placed parts | Save a group as a section, place it again, open and edit it |
| **Slots** (type, size) | In the format only; capital loadouts in Batch 11 |
| **Form:** visual model reference, material slots, style family | Placeholder meshes now; real art later without changing designs |
| **Interior definition** | In the format only |
| **Design hash** of the function data | Compiled-form cache (§4) |
| **Wheel, landing leg, battery** part kinds | Rovers, landing |
| **Collider** per part (defaults to its mass shape) | Compound colliders |

Vehicle instances carry their own part list (objects-vehicles-physics §2.1); the builder edits the
instance.

## 4. Design compilation

ships-stations-interiors §6, first real use:

- **Inputs:** the instance's part list and attachment graph.
- **Outputs:** mass properties and performance summary (`VehicleBody`, from Batch 1), thrust
  allocation data, compound collider, wheel set, and for rendering **one merged mesh per
  material** (later per spatial chunk), with a part index per vertex for Batch 5 damage.
- **Keyed by the design hash:** identical designs share one compiled form.
- **Runs on the worker pool** when leaving the builder or loading; the vehicle shows the old form
  until the new one is ready, so leaving the builder never hitches.
- **Rule:** nothing at runtime loops over parts every frame (ships-stations-interiors §6.4). A
  benchmark checks that a 300-part ship costs about the same per tick as the Kestrel.

**As built (3.4):**

- **Collider:** one compound per design, cached by hash and shared. Landing legs and wheels keep
  their exact shapes; every other part is merged into at most 32 boxes (a 4 × 2 × 4 grid over the
  vehicle, one box around the parts in each cell). Rapier's per-step cost grew with the number
  and complexity of a compound's pieces (300 shapes: 4×; convex hulls cost more than boxes).
- **Meshes:** one merged mesh per material, built on the worker pool and cached by design hash;
  plumes stay separate entities.
- **Measured** (`sim_vehicle/tests/part_count_cost.rs`, release): flying, the Kestrel 9 µs per
  tick and a 300-part ship 17 µs; landed at rest, 22 µs and about 80 µs (resting bodies skip
  ground contacts; the rest of the gap is open).
- **Ground patches** are built off the simulation thread including their collision trees, at
  most eight are added per tick, look-ahead is capped at 60 m, and only dynamic bodies ask for
  them (arcade flight and cruise follow the ground their own way). An earlier version streamed
  patches for every fast mover and built their trees on the simulation thread: multi-second
  frames on Windows.

## 5. Builder

### 5.1 Entering and leaving

- **B** next to a landed vehicle (within reach, on foot or in it) edits that vehicle; **B** on
  open ground starts a new vehicle from a root part (cockpit or rover chassis).
- While building, the vehicle is frozen (kinematic) where it stands; time keeps running. Leaving
  compiles the design and the vehicle drops onto its gear or wheels.

### 5.2 Editing

- **Camera:** orbits the vehicle (right mouse), zoom with the wheel, pan with the middle mouse.
- **Parts palette** grouped by kind; pick a part, it follows the mouse and **snaps to free attach
  nodes**; click to place. Rotate with keys in 90° (or finer) steps.
- **Select, move, remove** placed parts (removing takes the parts attached below it, with a
  confirmation when it is many).
- **Symmetry:** mirror, and radial ×2/×3/×4/×6/×8 around the vehicle's axis.
- **Sections:** select a group and save it as a section; sections appear in the palette.
- **Undo and redo.**

### 5.3 What the builder shows

- Mass, centre of mass marker, size.
- Thrust-to-weight on the current body, and whether the ship can **hover, strafe, turn** (thrust
  allocation shortfall per axis, objects-vehicles-physics §6).
- For rovers: wheel load, top speed estimate, tipping warning (centre of mass high above a narrow
  wheelbase).
- Fuel and battery capacity and estimated Δv and range (consumption comes in Batch 4).
- **Validation:** all parts connected, at least one control source (cockpit or probe core),
  no overlapping colliders; problems listed and highlighted.

## 6. Saving

- **Save file** (`saves/<name>.json`): format version, universe and game time, the `ObjectId`
  allocator, every object record (vehicles with instance part lists and states, the walker),
  terrain deltas per body, the player's state (which object they control, camera).
- Content (parts catalog, star system) is referenced by id, not copied; a missing part on load
  is reported and the part dropped, not a crash.
- **Quick save F5, quick load F9;** a save list in a simple menu.
- **Migration hook:** each format version has a step to the next; until Checkpoint A the format
  may also be reset freely (development plan §7.1).
- Tests: save → load round trip restores the same states, parts and deltas; golden files start
  at Checkpoint A.

## 7. Deferred

| Feature | Batch |
|---|---|
| Fuel and power consumption, refuelling | 4 |
| Part costs, shops, shipyards | 4 and 6 |
| Walking inside ships | After the builder (with capital ships in 11 at the latest) |
| Hull skins | When art direction needs them |
| Damage, parts breaking off | 5 |
| Vehicle-vs-vehicle collisions in combat, many ships | 5 and 6 (rapier already detects them from Batch 3) |
| Structural joints and flexing (hinges, rotors, pistons) | When a design needs moving parts |
