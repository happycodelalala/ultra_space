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

**As built (3.6):** the controller sweeps the walker's capsule against **vehicles only**
(blocked by hulls, standing on decks); the ground stays the terrain height function, which the
patches match to about a centimetre. Sweeping against the patches got stuck on their triangles
(rapier only pushes a shape out of overlaps on a zero move, and fixing internal edges made it
worse), and walkers then don't need patches built around them. Walkers move first in each tick,
against the vehicles as the last step left them, and turn with the planet once for the whole
tick: turning after each substep put them about a metre (the ground's travel) out of step with
the hulls they were swept against, and the push out of the resulting overlap flung them away
at hundreds of m/s on Windows. Slope limits stay
for later (the height function has none).

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
- Patches live 2 s after their last use and at most 64 are kept: they all hang on the turning
  ground body, so rapier moves every one each step (200 patches cost 35–60 ms per tick).
- When a simulation tick runs over budget, the game logs each region's physics breakdown
  (ground update, world step, bodies, patches); `sim_vehicle/tests/ground_tick_cost.rs`
  measures whole-region ticks near the ground (and runs natively on Windows with
  `ULTRA_SPACE_CONTENT` set).

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

## 7. Jointed parts, walking and part models (tasks 3.10–3.12)

Brought forward on 2026-11-13 to make a mech testable (validated with the original heavy frame).
Decided with the developer: animated walking; models generated by script.

### 7.1 Jointed parts (kinematic articulation)

- **Joint parts:** a `joint` part kind with a `joint` block: type (`hinge` with angle limits,
  `rotor` turning freely, or `slider` moving along its axis within `limits_m`: a telescoping
  stage), axis (part axes), speed (`speed_deg_s`, or `speed_m_s` for a slider) and torque (a
  slider's force). A joint part has a `base` node (joins its parent) and an `end` node;
  everything attached beyond `end` forms a **rigid group** that turns about the joint's axis
  through the `end` node, or slides along it. Poses store hinge and rotor angles in degrees and
  slider positions in metres.
- **Interference:** parts may overlap only where they are joined: one hangs on the other
  directly or through a chain of joint parts (a gimballed engine in its socket), and a beam
  riding a slider counts as the sleeve it runs in.
- **Poses and stages:** a design names poses (joint part -> value). A pose can list stages,
  in-between joint sets it passes through on the way to it, in order, and back through in
  reverse when leaving it (`pose_stages`), so a folded mechanism can swing clear before it
  unfolds. The game's P key steps through the poses in order (alphabetical), following each
  move's route; a staged move waits for every joint to reach one stage before the next.
- **State:** each joint's current angle and target live with the vehicle; angles move toward
  their targets at the joint's speed. Group poses come from the angles by forward kinematics
  down the attachment tree.
- **Physics:** the vehicle stays **one rigid body**; each group's collider pieces move with its
  pose every tick (mass properties stay those of the design's rest pose). Reaction forces of
  swinging limbs are ignored; rapier multibody joints can replace this when physical limbs
  matter (e.g. grappling).
- **Thrust:** thrusters on moving groups push along their current direction; the allocator is
  rebuilt when joint angles change.
- **Rendering:** one merged mesh per group per material, placed at the group's pose.
- **Control:** a design names **poses** (joint angles); a key steps through them. Walking
  drives the legs.

### 7.2 Animated walking

- A design may declare **legs** (hip and knee joints and a foot each) and walking parameters
  (stand height, step length and height, speed).
- In **walk mode** a locomotion controller moves the vehicle body along the terrain at stand
  height (kinematic, like arcade flight), turns it, and runs a **gait**: feet are planted on the
  ground and stepped one at a time to targets ahead; a **two-joint leg solver** sets hip and knee
  angles so each foot reaches its target on the real terrain. WASD walks and turns; leaving walk
  mode (or no ground under the feet) returns to flight.
- **As built (3.11, `sim_vehicle::gait`):**
  - The design's `gait` block lists legs by part id (hip, knee, optional ankle joint, foot) and
    `step_length_m`, `step_height_m`, `speed_m_s`, `turn_rate_deg_s` and `stance`. `stance` is
    the standing hip-to-ankle distance as a fraction of the leg's length; the stand height
    follows from it.
  - The `Walking` component turns walk mode on. `walking_system` runs before the joint system
    and sets the leg angles directly, ignoring joint speeds; `flight_system` places the body as
    kinematic, like arcade flight. The allocator is rebuilt when walking ends.
  - Feet are planted points in the surface frame, so they turn with the planet. A foot steps
    once it is more than half a step length from where it wants to be (0.4 m when standing),
    only one foot at a time. A step takes 0.55 s along a sine arc over the terrain.
  - The leg solver works in the leg's plane (YZ of the vehicle) and knees bend backward. The
    ankle joint cancels the thigh's and shin's tilt, then pitches the sole to the ground's
    fore-and-aft slope (up to 20°). A flat sole on a slope digs in at one end, and the push-out
    tipped the stopped frame over.
  - Feet stand fore and aft under the centre of mass, not under the hips: the heavy frame's
    centre of mass is 1.2 m behind its hips.
  - Stopping (M again) sets `Walking::stopping`: the frame slows to a halt, steps its feet to
    within 0.15 m of their places, and only then leaves walk mode, so physics takes over a
    frame standing on both feet. Sim test: 10 s of physics after stopping on a 6° slope, tilt
    0.7°.
  - The heavy frame has ankle hinges and walks at 3 m/s with 4 m steps. Sim test on Meridian
    over 40 s: 89 m walked, 7.7 m climbed, a 160° turn, 60 steps, planted feet within 0.16 m
    of the ground.
  - Not yet: soles rolling to side slopes, body sway, slope limits, walking reacting to
    collisions.

### 7.3 Part models

- A part's form names a model; the game uses `assets/models/<model>.glb` when it exists (glTF
  binary, metres, part axes, Y up) in the vehicle's merged meshes, else the placeholder shape.
- Models are built by scripts run in headless Blender (`tools/blender/`, `tools/build-models.sh`):
  original hard-surface designs from what each part does, cut with booleans, their ambient
  occlusion baked into vertex colours. Each primitive carries a material slot from
  `content/data/part_materials.json` (ceramic armour, graphite plates, actuator metal, polished
  rods, dark frame, grip pads, accent, lenses, energy glow), which the game and the viewer both
  read. Hand-made models with the same names and slots can replace them.
- **Visual language:** near-future industrial military machines (dark military green armour,
  dark metallic grey machinery; sculpted armour plates with rounded edges over exposed
  machinery: hydraulic rams with polished rods, servo drums, hose runs), neither sleek
  science fiction nor retro.
- **Every detail has a function** that makes sense where it is: a window has a room behind
  it, a light lights something (or marks an extremity), a ladder or railing is on a route
  people use, a hatch opens into a space, a vent is where heat or air leaves, hazard stripes
  mark pinch points and moving edges. No decorative glows, grooves or greebles.
- **Designed for its size:** a power suit, a 20 m frame, a 100 m titan and a kilometre-long
  capital ship look different. Big forms stay big at every size (large armour plates and
  parts); what grows with size is how much the crew's scale shows (railings, doors, ladders,
  windows, lights, all at human sizes) and the surface detail.
- **Surface detail at real sizes:** plate seams, fasteners and wear are drawn by the hull
  detail shader from each model's first UV set (the surface position in metres along each
  plate), with parameters per palette slot (`detail` in `part_materials.json`); the same
  detail makes a capital ship's hull read as kilometres. A vehicle's functional layout (crew
  spaces, routes, lights) is written down before its models: for the Bastion in
  [bastion.md](bastion.md).
- Every part's model is rigid: an actuator that would span a joint is modelled inside one of the
  two parts. Parts that are mirror images have their own models (`form.model` = the part id).

## 8. Deferred

| Feature | Batch |
|---|---|
| Fuel and power consumption, refuelling | 4 |
| Part costs, shops, shipyards | 4 and 6 |
| Walking inside ships | After the builder (with capital ships in 11 at the latest) |
| Hull skins | When art direction needs them |
| Damage, parts breaking off | 5 |
| Vehicle-vs-vehicle collisions in combat, many ships | 5 and 6 (rapier already detects them from Batch 3) |
| Pistons, physical (multibody) joints, flexing | When a design needs them |
