# A block world: everything built from shaped blocks

Decided 2026-11-15 (developer), after comparison renders of voxel art, faceted low-poly, the two
mixed, and voxel art with shaped blocks. The world goes the Stonehearth route all the way: the
ground and everything on it is made of **blocks the player can mine, dig, build and destroy**,
"a higher-poly Minecraft, as a universe with far more to explore". Blocks come in a **standard
set of shapes** (so roofs, hulls and hillsides are clean 45-degree slopes, not staircases) and
in **sizes that double** from one to the next, so the same system covers a personal item, a
house, a planet's ground and a kilometre-long station.

This replaces the heightmap-only terrain near the player (planet-surface.md) and the "voxel
patches" it deferred; the terrain function stays the source of the untouched world. The look is
set in visual-style.md; this document is the system.

## 1. Decisions

| Question | Decision |
|---|---|
| Route | **Voxel art with shaped blocks.** Faceted low-poly was the alternative (better at a distance, lighter on the GPU); blocks won because the world, what is dug and what is built are then one language. A mix of block ground and faceted scenery was rejected: two shape languages side by side always show the seam |
| Shapes | A **standard set** (§3) in every orientation; no free-form angles anywhere |
| Sizes | **Powers of two**, each size twice the one below and aligned to its own multiples (§4), so sizes nest exactly |
| Terrain block | **25 cm**; hills read as slopes between terraces, taller drops stay crisp cuts |
| Far terrain | **Blocky everywhere:** distant terrain is drawn from the terrain function stepped to the block grid with the same slopes, so near and far are one look (only the near zone is editable blocks) |
| Dig depth | **Tens of metres** below the surface near the player; caves and deep layers designed now, generated later |
| Mining | **Drops resources:** mined blocks become items in the player's inventory; placing a block uses one |
| Grids | **Every object has its own grid:** the planet's ground, each building, ship, vehicle and station (§5) |
| Round bodies | **Flat plains, about 40 km a side:** 12 pentagons and as many hexagons as the body's size needs (a Goldberg layout); each plain is a flat, exact square grid and one region of the world; plains meet at creases of generated, shared **seam blocks**, shown as fault rock (§5.1; decided 2026-11-16) |
| Block size | **Exactly the same everywhere:** a 25 cm block is 25 cm on every plain, at every depth, on every body (the reason for the plains: a single bent grid varies block widths by up to 1.41 times) |
| Small bodies | Below about 25–50 km across: **one 3D square grid**, "up" from the surface (§5.2) |

## 2. Why blocks of many sizes

- **Mixing sizes always fits.** A block is exactly 2 × 2 × 2 blocks of the size below, so small
  parts meet big ones flush, with no gaps or odd offsets (unlike games whose small and large
  grids don't snap, such as a ratio of 5 between them).
- **Destruction and digging come free.** Damage or a mining tool splits a block into its eight
  children and removes some; a wall crumbles into chunks, then smaller chunks, down to the
  smallest size.
- **Distance, memory and physics are cheap.** Eight equal children merge into one parent, so a
  plain hull or untouched rock is one node, not thousands, and distant detail falls away level by
  level (a sparse octree).

## 3. Shapes

Every cell holds one shape, in any of its orientations:

| Shape | What it is | Used for |
|---|---|---|
| Block | The full cube | Most of everything |
| Wedge | A 45-degree slope (two top corners down) | Roofs, hull plates, ramps, hillsides |
| Outer corner | Three top corners down | Where two slopes meet at a hip or ridge end |
| Valley corner | One top corner down | The inside corner between two slopes |

A shape is stored as the four corner heights of its top face (full or zero) along one of six
"up" directions: one extra byte per cell beside the material. Later additions are allowed only
if they stay on the grid and on the same angles (a half block and a 2:1 slope are the likely
ones), never a free-form part.

**Shapes come from a rule** as well as from placing them by hand: where solid blocks form a
staircase one block high, the step becomes a wedge, two adjacent steps an outer corner, and the
inside of an L a valley corner; steps taller than one block stay sharp edges, so dug pits and
cliffs keep crisp cuts. Each model or material has a **recipe**: which up directions it smooths
(the ground and roofs upward only, roof undersides and tree crowns up and down, trunks, barrels
and rocks also around their sides, ship hulls in every direction, people not at all). Features
one or two blocks wide are left as blocks (a cell slopes only where the solid continues behind
it), so small bumps don't turn into spikes.

**Designing for the shapes** (learned from the style renders): a slope runs in one direction at a
time, so a form should narrow in plan *or* in profile, not both at once (a nose that does both
breaks into spiky corners; a wedge nose that narrows in plan under a flat top reads cleanly);
and a colour border can only follow the grid or a slope edge, so stripes and bands run along
the grid (a band along a 45-degree edge always zigzags).

## 4. Sizes

A block of size *s* sits only at positions that are multiples of *s* in its grid, so any block
splits into eight children and eight equal children merge into their parent. The working range:

| Size | Typical use |
|---|---|
| 3.125 cm | People (about 16 blocks across the head), personal items, grass, small fittings |
| 6.25 cm | Building fixtures (windows, doors, shutters, lanterns), props |
| 12.5 cm | Bushes, props, small-ship detail |
| 25 cm | Terrain, building structure, trees, rocks |
| 50 cm – 2 m | Vehicles, ship hulls, large buildings |
| 4 – 8 m | Capital ships, stations, megastructures |

Larger things are many of the largest blocks, kept affordable by merging and by distance detail.
**Above the working range** the same tree goes on, 16 m, 32 m … up to about 2 km, as **giant
bricks** for storage, collision and distance, never as a size anyone places by hand: uniform
volume (an asteroid's core, a plain's deep rock, a capital ship's armour belt) is kept as the
largest aligned brick that fits and split only where something touches it. A big brick's visible
face may carry a pattern at the size the eye expects (panel lines, stones); breaking it turns the
pattern into real blocks. Merging back is automatic where a volume is uniform again.
A model designed at some other size moves to the next **finer** size of the set, never the
coarser one: rounding up loses whatever was thinner than the new block (fence rails, caps, trim).

**The visual rule (from Stonehearth):** block size follows an object's size and role, about 10
to 30 blocks across each object; and within one object only **about three neighbouring sizes**
are used (structure coarse, fittings finer). A house is 25 cm structure with 6.25 cm fittings; a
capital ship is 4 m hull blocks with 1 m and 50 cm detail, never 4 m beside 3 cm. Mixing distant
sizes in one view is what makes "Lego with non-standard parts".

## 5. Grids

- **A round body's ground** (planets and moons) is a polyhedron of flat plains, each with its own
  exact square grid (§5.1).
- **A small body's ground** (asteroids, debris) is one 3D square grid (§5.2).
- **Every structure has its own grid** in its own frame of the frame tree: a building, a ship, a
  vehicle, a station. Grids don't line up with each other; they meet only where things touch
  (landing, docking, a building on the ground), and the physics handles that contact.
- **Moving parts** (doors, turrets, legs and arms of a mech) are separate block groups on joints,
  each with its own grid, posed by the joint data.

### 5.1 Round bodies: a polyhedron of flat plains

**Why.** Identical square blocks cannot cover a sphere: each row of blocks nearer a pole is
shorter than the last (at 45 degrees latitude on Meridian, by about 1.1 m a row), so a
planet-wide grid must give somewhere. The options compared (renders and measurements, 2026-11-16):

| Option | Where the mismatch goes | Verdict |
|---|---|---|
| One bent grid per cube face (the demo village's mapping, planet-wide) | Block widths vary up to 1.41 times (equal-angle mapping), squares skew to 60/120 degrees near the 8 cube corners | Rejected: blocks must be one size |
| A 3D grid for the whole planet | "Down" meets the grid at up to 55 degrees: staircase plains, tilted walls, diagonal shafts | Rejected for planets (kept for small bodies) |
| Rings of blocks along the latitudes (igloo) | Rows offset like brickwork everywhere, special polar caps | Rejected |
| Hexagonal blocks | Fit hexagonal plains, but walls, roofs and hulls lose their flat planes; no exact nesting into bigger bricks | Rejected (village rebuilt in hexagons for the comparison) |
| Icosahedron (or cube, or rounded cube) planet | Exact everywhere, but the planet is a polyhedron of 6–26 big faces with 35–90 degree turns | Rejected: planets stay round |
| **Flat plains with generated seam blocks** | **One row of irregular, shared blocks along each crease** | **Chosen** |

**The layout.** Every round body is a Goldberg polyhedron: **12 pentagons plus 10(T - 1)
hexagons** (T = m² + mn + n²), the only way flat plains of five and six sides close into a ball
(Euler). The count is chosen per body so the plains are about **40 km a side** (about 69 km across,
4,200 km²):

| Body radius | Plains | Turn at a crease |
|---|---|---|
| 50 km | 12 (a dodecahedron) | about 63 degrees |
| 100 km | 32 (a football) | about 37–42 degrees |
| 200 km | 122 | about 20 degrees |
| 600 km (Meridian, T = 109) | 1,092 | about 6–7 degrees |
| 1,000 km | 3,012 | about 4 degrees |

Plains are evened out by relaxation (hexagons within about 10–20 % of each other's area; the
pentagons are 30–40 % smaller). Small moons are strongly faceted toy worlds; large planets read as
round. Each plain is a flat polygon in its own plane; neighbouring plains meet along a straight
crease.

**Inside a plain:** an exact square grid of 25 cm blocks, one axis along one of the plain's edges.
Columns are parallel, along the plain's own "down", so blocks are the same size at every depth.
A block is addressed by plain, i, j and k. Each plain holds one brick tree (§4): giant bricks fill
its inside and split to 25 cm only near its seams.

**Seams.** Along each crease one row of **seam blocks** is shared by the two plains: each is the
whole cell between plain A's grid and plain B's, folded along the crease, its footprint computed
from the geometry when its chunk is made (never stored: a dug or changed seam block is an edit
like any other).
- **Underground** a seam is a wall one block thick, down the plane that bisects the crease. A
  plain's columns lean against it by half the crease angle, so the seam shifts one block across
  the grid about every 4 m of depth (Meridian); its blocks change shape slowly with depth.
- **Where three plains meet**, the three walls meet in a column of three-way corner blocks.
- **They are fault rock:** their own materials (fault stone, crystal veins, richer ore), a ridge
  of exposed stone at the surface and a vein to follow below; world generation favours caves,
  springs and passes along them. Players dig them like any block but cannot place new ones; a dug
  seam cell can only be refilled with its own generated shape.
- **Cost:** about 0.5 billion seam blocks per layer on Meridian, about 0.0007 % of its columns and
  about 1 chunk in 2,500; none exist until their chunk is generated near the player. The real cost
  is the code (meshing, face hiding, collision, editing and navigation across a seam), roughly
  1–2k lines with tests.

**Regions.** Each plain is a **region** of the world with its own climate, land type, sea,
settlements and name; crossing a crease brings a welcome message ("Entering the Ashen Steppe"),
marked on the ground by the fault ridge, the change of land and the turn. Below the surface each
plain's column splits into depth layers (soil and caves, deep caverns, the fault zone), each a
region too. A **mantle** a few km down cannot be dug, so no one reaches the narrow depths where a
plain's seam walls converge.

**Down.** Near the ground "down" is the plain's normal, so everything on a plain is exactly level
and seas are flat; at a crease it turns by the crease angle, blended over a few metres so the
camera never snaps. Well above the ground (a few km) it blends to the true direction to the
centre; from far away the body is a point mass and orbits are unchanged.

**What creases mean for play** (Meridian, 6–7 degrees):
- **Sight:** within a plain there is no curvature, so views reach the plain's edge; a crease is a
  ridge that hides the land beyond it (about 11 m per 100 m past it), so regions are also natural
  theatres with their borders as crests.
- **Fast ground vehicles** leave the ground briefly crossing a crest; the collision surface
  rounds each crease over a few metres.
- **Long builds:** a structure stays on one plain's grid; it may cross a crease on a foundation
  that makes up the drop. Roads, rails and walls bend once at each crease.

The demo village's ground (a flat 25 cm grid in the tangent plane, bent onto the sphere at the
vertices) is the inside of one plain without the bending; it moves to the plain layout when
planet-wide ground cover is built.

### 5.2 Small bodies: one 3D grid

Bodies below about 25–50 km across (too small for 12 plains of 40 km) are one 3D square grid in
the body's frame, the same grid as ships and stations. "Up" is the surface under the feet (the
direction away from the rock's shape), with grip boots and thrusters, since gravity is too weak
to stand on. A small rock is kinematic and on rails (§9); a piece mined off becomes a new body
with the velocity the original had at that point. Round small bodies look stepped at the block
scale, softened by the slope shapes. A building on one stands on its own grid and foundation; on
a rock a few hundred metres across a big base becomes sections joined by corridors.

## 6. The ground: what a block is made of

The untouched ground is a function, like the terrain heights, so it is never stored:

- **Above the surface** (the terrain function's height, rounded to the grid): air, or water up to
  sea level.
- **Below:** a topsoil layer by biome (grass, sand, snow, ash in the waste), subsoil, then rock;
  layer depths by climate and slope.
- **Resources:** ore bodies and veins by noise and by the planet's data (which ores a world has,
  how rich); clay, sand and gravel near water; (later) caves, crystal pockets, buried ruins.
- **Edits** override the function block by block (§8).

Block materials are data (`content/data/blocks.json` with a schema): name, palette colour,
hardness (how long mining takes), what it drops, whether it falls (sand, gravel: later), and its
shape recipe.

## 7. Chunks, meshing and distance

- **The block zone** is the ground within about 96 m of the player (and of a vehicle being driven
  or landed), down tens of metres and up to the highest edit: chunks of 32 × 32 × 32 cells made
  on the worker threads from the function plus edits. Chunks come and go as the player moves, a
  few a frame (planet-surface.md §6), nearest first, and an edited chunk is re-meshed within a
  frame or two.
- **Meshing:** flat faces between solid and empty cells are merged into the largest rectangles
  of one colour (greedy meshing); runs of identical slopes are merged the same way, so a roof
  plane or a hillside is a few large faces. Faces between two cells that both cover them are
  never drawn.
- **Far terrain:** beyond the block zone the quadtree tiles stay, drawn from the same function
  stepped to the grid with the same slope rule, so the land reads as the same blocks from low
  flight; where block chunks are, the tiles are cut away under them. From orbit nothing changes
  but the look.
- **Structures** mesh the same way per chunk of their own grid, with coarser merged levels for
  distance (a parent drawn in place of its eight children).

## 8. Editing, destruction and saving

- **Mining:** a tool removes blocks under the crosshair, a brush of several at once, in a time set
  by their hardness; their drops go to the inventory. A big block is split into children first,
  so a tool removes what it touches, not a whole 4 m block.
- **Placing:** puts blocks of a material from the inventory, aligned to the grid, in a chosen size,
  shape and orientation.
- **Destruction:** damage splits blocks down the sizes and removes the broken ones; pieces cut off
  from their structure become debris (later batches: combat, collapse).
- **Edits** are kept per chunk (a sparse list of changed cells) and saved with the region (the
  object system's terrain deltas, technical overview §14): untouched chunks cost nothing.
- **Settlement foundations** become edits made from the layouts (levelled pads under buildings),
  procedural like the layouts and never saved.

## 9. Physics

The ground and structures near moving bodies are colliders: full cells as the physics library's
voxel shape (rapier's `Voxels`, parry 0.31), and shaped cells as small extra convex pieces in the
same chunk, so the walker climbs slopes smoothly instead of stepping. Farther away and in fast
flight the terrain function's height stays the contact surface (as now), since only the block
zone can differ from it. Large structures use merged parents for their collision too.

## 10. Models made of blocks

Buildings, props, nature, vehicles, ships and people are block models in this system: designed
as procedural recipes (code that places blocks, as in the style renders) or built in an editor,
and stored as block data (sizes, shapes, orientations, palette materials) rather than meshes.
People and mechs are block groups on their skeleton's bones, so they animate as rigid parts.

**The handoff between the tracks** (AGENTS.md) gains the **block model format** (agreed
2026-11-15): engine-agnostic files under `assets/` with a schema, holding a model's block size,
cells, palette colours and shape recipe, meshed by the game with the rules above. The style
scripts (`tools/blocks/`) are the first authoring tool and export the demo village's models on
the game track (development plan §7.7); the design track takes the format up for buildings and
objects afterwards. Vehicles, mechs and ships stay glb part models for now; moving them to block
data is decided separately, with both tracks.

## 11. Inventory (brought forward)

A small inventory: stacks of materials by kind with a capacity, shown in a simple panel; saved
with the player. Items beyond blocks (tools, parts), containers and crafting stay in the survival
batch.

## 12. Budgets and risks

- **Triangles:** in the style test scene (a house, a shed, 30 trees, a ship, people, grass),
  plain blocks came to about 256,000 triangles against 94,000 for faceted low-poly; the land and
  grass are most of it, and they repeat across a planet. Merged slopes cost less than the
  staircases they replace; distance detail (merged parents) is required for forests and the far
  ground, not optional.
- **Memory:** a chunk is 32 KB of cell ids plus shapes; the zone is a few thousand chunks, tens
  of MB. Structures are sparse octrees.
- **Meshing:** a chunk in a few milliseconds on a worker; the zone fills in a second or two on
  arrival, nearest first, behind the far tiles.
- **Physics:** voxel colliders plus slope pieces only near moving bodies; measured in the
  prototype.
- **Seams** between plains (§5.1): the riskiest part of the ground grid; the batch that first
  builds planet-wide ground must cross one (and a three-way corner) before anything else.
- **Large block bodies in contact** (a capital ship landing on an asteroid): mesh against mesh is
  slow and unstable in the physics library, so moving bodies collide as merged boxes, big bodies
  are kinematic, and a landed ship is pinned rather than balanced on contacts; a prototype comes
  first in the batch that needs it.
- **25 cm blocks** are 64 times as many as Minecraft's per volume; the zone's radius and depth are
  budgets, set by measurement, not constants.

## 13. First step: the demo village

The system is proven on the smallest scope that shows it whole (development plan §7.7): one
village on Meridian where the player starts, built from block models on block ground, with
digging and placing; frame times and memory measured on the test machine. The village sits away
from where seams would be; the plain layout, crossing a seam, planet-wide ground cover, small
bodies, large structures, ships and stations come in the batches that first need them.
