# Universe: scale, time and flight

Decisions made on 2026-11-12, before Batch 1. Engine-agnostic. Numbers marked *provisional*
are tuned by feel later.

---

## 1. Planet scale (compressed)

Real scale makes planets enormous and travel impractically slow. Planets are compressed to
roughly KSP scale, with near-Earth gravity on habitable worlds.

| Type | Radius | Gravity | Role |
|---|---|---|---|
| Asteroids, small moons | 5–50 km | Very low | Mining, outposts |
| Moons, small planets | 100–300 km | Low | Single-culture worlds, outposts, quick landings |
| **Main habitable planets** | **400–800 km (typical 600)** | **~1g** | Multiple civilizations, cities, large wars |
| Super-earths | 1,000–1,500 km | High | Rare; vast, harder to reach orbit from |
| Gas giants | 5,000–20,000 km | No surface | Stations, moons, atmospheric skimming |

What a 600 km, 1g planet gives:

| Quantity | Value |
|---|---|
| Horizon, standing / from a 100 m hill | 1.5 km / 11 km |
| Speed for low orbit | 2.4 km/s (same as KSP's Kerbin) |
| Circumference | 3,770 km |
| Land area at 30% land | ~1.36 million km² (about Peru), room for 10–25 medieval-scale kingdoms |
| Atmosphere height | ~70 km |

- Gravity varies naturally by planet: near 1g on habitable worlds, lower on moons and
  asteroids, higher on super-earths.
- Smaller planets look like toys (close horizon, visible curvature); bigger ones need denser
  content to avoid feeling empty.
- *Provisional:* in Batch 2 the same planet is compared at 300, 600 and 1,000 km on Windows,
  and the ranges are locked in by feel.

## 2. Distances and travel

| Range | Distance (compressed, ~1/10–1/20 of real) | Travel mode |
|---|---|---|
| Near a planet | 0–1,000 km | **Normal flight:** Newtonian thrusters. Takeoff, landing, docking, combat |
| Moons from their planet | 5,000–50,000 km | Normal flight or cruise drive |
| Planets from the star | ~2–60 million km | **Cruise drive** |
| Between star systems | Light-years | **Interstellar jump** |

**Cruise drive** (in-system, seamless):

- Speed scales with distance from the nearest planet or star: fast in open space, slowing
  automatically on approach. No overshooting; it can't be used inside an atmosphere.
- Neighbouring planets in **1–3 minutes**; across the whole system in **5–8 minutes**.
- Drops to normal flight near the target or on request. Combat, docking and landing happen in
  normal flight.
- Its known destination lets streaming start minutes ahead (hitch budget, overview §14.6).
- Built as a ship module a ship has or lacks. **Deferred:** whether it's unlocked through
  technology or available from the start.

**Interstellar jump:** a jump sequence of **a few seconds** is a development placeholder, and
the one deliberate transition (overview §14.4). **Deferred:** the final duration. Too fast
makes escaping combat too easy; too slow is boring. Levers to tune later: spool-up time,
jumps blocked near planets or large ships, interdiction, a cooldown after combat.

## 3. Flight model

- **Newtonian physics with flight assist.** Real momentum and inertia.
- **Assist modes:** full assist (default: holds the set speed and stops rotation), rotation
  damping only, and off (raw thrust). KSP-style orbit helpers (hold prograde, circularize)
  come later.
- **Assist uses the vehicle's actual thrusters** through control allocation (overview §5): a
  ship with weak side thrusters really does strafe badly.
- **"Standing still" reference switches automatically:**
  - near a planet's surface: relative to the rotating ground, so letting go hovers over the
    same spot
  - in space: relative to the local orbital frame, so letting go coasts along the orbit
- **No speed limit** in normal flight except the speed of light, for anything but warp drives.
  Flight assist holds the speed the pilot sets; it doesn't enforce a cap. Relativity itself
  isn't simulated.
- **Deferred:** combat behavior and balance.
- Atmospheric lift and drag are added in Batches 2–3 on top of the same model.

## 4. Time

### 4.1 One universe clock

- **Universe time** is one integer tick counter (no drift over long saves), running 1:1 with
  real time. Physics, orbits and planet rotation use it.
- **Every celestial motion is a formula of universe time:** orbits are on rails, rotation is a
  constant spin. Unvisited star systems cost nothing; on arrival their planets are evaluated
  at the current time.
- Faction, economy and caravan simulation elsewhere runs in the tiered simulation
  (overview §3), stamped with universe time.
- **Relativity is ignored:** one absolute time, no time dilation.
- **Time warp** speeds up the one clock for everything (single-player). During warp, distant
  areas run in the cheap tiers. **Future multiplayer:** the server owns the clock; shared warp
  is an open problem.

### 4.2 Scaled game clock

The game clock is a **scaled view of universe time**. It doesn't change physics.

| Clock | Scale | Used for |
|---|---|---|
| Universe time | 1:1 real | Physics, orbits, rotation, flight, combat |
| **Galactic Standard Time** | **16×** (1 real minute = 16 game minutes) | Contracts, faction news, travel estimates, anything crossing planets or systems |
| Local time | Fitted per planet: one rotation = 24 local hours | Time of day, NPC schedules, shops, guard shifts |

- **Day length** is per planet: habitable worlds typically **60–120 real minutes**, baseline
  **90 minutes** (= 24 game hours at 16×). On a planet with a 2-hour day, a local hour lasts 5
  real minutes.
- A 90-minute day on a 600 km planet means: equator moving at ~700 m/s (eastward launches get a
  boost), low orbit ~30 minutes per lap, stationary orbit ~780 km up.
- **Physical events run in real seconds** (falling, projectiles, thrust). **World events run in
  game time** (crops, caravans, contracts, opening hours).
- **Waiting and sleeping** ("wait 8 hours") skip the equivalent real time (30 minutes at 16×)
  with time warp, allowed outside combat.
- **Strategy pacing** (wars, economy growth) is tuned in real play time, independent of day
  length.

### 4.3 Seasons and years

*Revised 2026-11-12 during Batch 1.* The original plan (years of 20–60 real hours) needed stars
thousands of times heavier, which shrinks every planet's sphere of influence to a few planet
radii: moons couldn't exist and rockets couldn't travel between planets. **A self-consistent
solar system takes priority over short years.**

- Stars are light (KSP-like), so spheres of influence are realistic, moons sit where intended,
  and rocket transfers between planets stay possible.
- **Years come from the real orbit** and are long: a habitable world's year lasts weeks to
  months of real time at 1:1 (hundreds to thousands of galactic days).
- **Seasons still come from physics** (axial tilt plus orbit). Within one session a planet's
  season is roughly fixed; seasons change over long play and **time skips**.
- **Time skips** (sleeping, idling, hibernation or similar) are how players see seasons change.
  How they work is not designed yet; distant areas run in the cheap simulation tiers during
  any skip.
- Every latitude still has its own climate from axial tilt, so a planet always has hot and cold
  regions.
- Calendars, month names and festivals are built by cultures on top of the real day and year.

## 5. Planet climate and types

The LLM rule from overview §13 applies: **the LLM proposes concept and parameters; physics and
rules compute the results.**

| The LLM proposes | Rules compute |
|---|---|
| Concept: frozen ocean world, lava world, gas giant with storm bands, airless rock, temperate world with harsh winters | Temperature by latitude and season, from star distance, axial tilt and atmosphere |
| Parameters within allowed ranges: axial tilt, atmosphere type and thickness, water coverage, greenhouse strength | Hot and cold regions, ice caps, deserts, habitable zones |
| Habitats, biomes, how life and cultures adapt | Season timing; day and night length by latitude |
| Names, descriptions, myths | Survivability (heat, cold, pressure, air) for gameplay |

Planet types include gas giants, planets with atmospheres, and bare airless worlds. Until LLM
generation (Batch 7), parameters are written by hand in the same data format.

## 6. First star system: Halcyon (hand-made)

A fixed test bed with one body per feature to exercise, in the same data format the procedural
generator and LLM will produce later. Procedural systems come in Batch 6. These are the names
outsiders use; each culture may name its own world differently.

| Body | Type | Radius | Tests |
|---|---|---|---|
| **Halcyon** (star) | Yellow-white main-sequence | ~300,000 km | Lighting, glare; light (KSP-like) mass for realistic spheres of influence |
| **Cinder** | Hot airless rock close to the star | ~200 km | Bare ground, extreme day/night temperatures, low gravity |
| **Meridian** (home world) | Temperate, atmosphere and oceans, 23° axial tilt | **600 km** | Baseline planet: seasons, climate zones, atmosphere; future home for cultures |
| ↳ **Lantern** | Airless moon, tidally locked | ~150 km | Moving between spheres of influence; a moon that always shows one face |
| **Hoarfell** | Cold desert, thin atmosphere, strong axial tilt | ~400 km | Extreme seasons, ice caps |
| **Leviathan** | Banded gas giant | ~8,000 km | No surface, large sphere of influence, distant scale |
| ↳ **Rime** | Icy moon | ~250 km | Ice worlds around a giant |
| ↳ **Forge** | Volcanic moon | ~180 km | Volcanic worlds around a giant |

## 7. Deferred decisions

| Decision | Revisit when |
|---|---|
| Final planet size ranges | Batch 2, comparing sizes on foot |
| Cruise drive as unlockable technology or standard | Technology tree design |
| Interstellar jump duration and escape rules | Batch 6 (living galaxy) and combat design |
| Combat flight behavior and balance | Batch 5 (combat core) |
| Shared time warp in multiplayer | If multiplayer is pursued |
| Time skips: sleeping, idling, hibernation | Survival and NPC design (Batches 4 and 8) |
