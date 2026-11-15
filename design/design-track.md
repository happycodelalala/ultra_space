# Design track

Vehicles, mechs, ships, capital ships, buildings and objects are designed on their own track
(branch `design/*`, worktree `~/code/ultra_space-design`), reviewed in the design viewer, and
imported into the game at milestones (AGENTS.md, "Two tracks"). This is its plan and backlog.

## Principles

- **Concept before models:** a design starts as a one-page brief (role, how it works, what
  its silhouette says) with silhouette options; modelling starts once one is chosen.
- **Every detail has a function** that makes sense where it is (design/vehicle-builder.md
  §7.3): a light lights or marks something, a hatch opens into a space, a handrail is on a
  route crew use, a radiator is where heat leaves, an actuator spans the joint it moves.
- **Designed for its size:** a suit, a 20 m frame, a 100 m titan and a kilometre-long ship
  look different; forms are shaped for the loads at that size, details are at human sizes,
  surface detail at real sizes (the hull detail shader).
- **Review in the viewer** with screenshots; game rounds only at import.

## Viewer: making it the design tool

1. Studio lighting and a neutral background good enough to judge value and colour (the
   shots are too dark), with the hull detail shader as in the game.
2. Scale references: a person, a ground vehicle, a 100 m tower.
3. Contact sheets: a turnaround and a pose sequence in one image per review.
4. Fast loops: rebuild only changed models; the viewer reloads them.

## Bastion v2

A strict review of the Bastion as an exploration and combat titan (about 100 m;
interstellar-capable, plausible advanced technology) found, in order of weight:

1. **No clear concept or silhouette.** The four shells are as big as the body; the humanoid
   reads as a small figure inside pods; the poses read as a beetle, a parachute, a jellyfish.
   Why a humanoid, and what the shells and folding legs are for, is never shown.
2. **Mechanical dishonesty.** Rams start and end on the same rigid part, so they move
   nothing; the abdomen suggests a spine the rigid torso doesn't have; no visible path carries
   the shells' loads from the saddles down to the hips; joints are generic drums with caps.
3. **Proportions of a 22 m machine scaled up:** limbs and waist too slender for about
   22,000 t, feet far too small (ground pressure), the hands redundant beside four
   manipulators, the folding legs implausibly thin, the head a cartoon face.
4. **Missing space systems:** no propellant tanks, engines too small for the mass, the
   radiator orders of magnitude too small, engines facing the body when closed, sensors and
   ports unprotected for combat, no weapon mounts designed in.
5. **Surface:** no value contrast (dark green and near-black merge), soft pillow bevels where
   armour should have hard chamfers and visible thickness, one 3 × 6 m seam grid everywhere,
   no markings, details 4.5 times oversized (built at 22 m and scaled up).
6. **Small fittings:** lights as bare cubes, handrails reading as wiring and crossing the
   shell arms' sweep.

Plan:

1. **Concept brief** with two or three silhouette options (for example: a carrier titan whose
   shells are its drive, propellant tanks and radiators around a low, wide, heavily armoured
   core). Choose one.
2. **Proportions at true scale:** massive legs and feet, a heavy pelvis, shoulders that are
   the structure carrying the shell arms, an armoured sensor cluster instead of a face, the
   shells' size set against the body's.
3. **Mechanical honesty:** every actuator split across its joint (cylinder on one part, rod
   on the next), real articulation or none, joints as large bearings in heavy structure.
4. **Space systems shown:** tanks, radiators (the shells' inner faces), main engines sized
   for the mass, RCS, armoured shutters over sensors and ports, weapon mounts.
5. **Surface and colour:** hard-edged armour language, value zoning (structure, armour,
   machinery, one accent), seams laid out per part, markings and stencils, real lamp housings.
6. **Real-size modelling:** drop the 1/4.5 design units; builders work in real metres.
