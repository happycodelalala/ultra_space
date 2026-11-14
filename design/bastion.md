# The Bastion: functional layout

An original titan, about 100 m tall, for **interplanetary exploration and combat**: it works in
vacuum, on airless and atmospheric worlds, in low and full gravity, and under fire. Four
quarter-shells close round it as a cocoon (armour in transit and in a fight) and open into
platforms carrying its main engines and drone bays (vehicle data:
`content/data/vehicles/bastion.json`; built and checked by
`impl/rust/crates/sim_vehicle/tests/bastion.rs`).

This document says where its crew are, how they move, how it stays alive in space and what
each visible detail is for, so the models stay honest: **a light lights or marks something,
a hatch opens into a space, a handrail is on a route suited crew use, a radiator is where
heat leaves.** Nothing open-air: no walkways, railings, deck lights, cranes, air intakes or
exhaust stacks, which would only work on a planet with air.

## Crew and spaces

About fourteen crew, all inside one pressurised, armoured **crew module** in the upper torso,
behind the thickest chest armour, shielded from the reactor below by its shadow shield:
command deck, operations, quarters, galley, airlock. **No windows** anywhere: the crew see
through the sensors (a window is a hole in the armour and useless at combat ranges).

| What | Where | Seen from outside as |
|---|---|---|
| Crew module | Upper torso, behind the chest armour | Nothing but armour (and its two airlocks) |
| Reactor (fusion) | Pelvis / waist, below the crew module | The armoured waist; its heat leaves through the radiators |
| Sensors | The head (no crew): radar, optical and infrared turrets, lidar, star trackers, searchlights | A sensor head |
| Radiators | Back of the torso, fin banks between armoured blades | The radiator fins, the only place heat leaves |
| Communications | High-gain dish on the back (folded flat against it), whip antennas on the head | The dish |
| Drones | Bays in the shells | Bay doors; lights inside the bays |

## Getting in and out

- **Docking port** on the back, above the radiators: a docking collar for shuttles and
  stations, ringed by docking lights.
- **Surface airlock** on the left flank of the torso: a door in an armoured frame with a
  stowed hoist under it (a platform on cables lowered to the ground on a planet).
- **EVA** (suited crew outside, any gravity): handrails from each airlock to the work sites
  that are serviced outside (the shoulder bearings and the sensor head), tether rings along
  them, work floodlights at each airlock and work site. **Nothing else outside is a route**:
  limbs and shells are serviced docked or parked, through bolted service panels.

## Moving

- **Main engines:** the four lift engines on each shell (in flight the shells are platforms).
- **Attitude:** RCS thruster quads (four nozzles each) at the extremities: the outer corners
  of the shoulder slabs and of each shell; at 100 m a small push far from the centre turns it.
- **Walking:** sealed hydraulic rams with dust boots over their rods (regolith gets into
  everything); the feet have retractable anchor spikes for low gravity.

## Lights

- **Navigation lights:** red on the left, green on the right (on the shoulder slabs), white
  strobes on the shells' outer corners and the head mast: so other craft see which way it
  faces and how big it is.
- **Terrain lights:** under the chest and on the shins, aimed down and ahead, for walking on
  the dark side of a world.
- **Searchlights:** in the sensor head, aimed where the sensors look.
- **Work lights:** at the airlocks, the docking port and the EVA work sites.
- **Bay lights:** inside each drone bay; amber approach lights along the bay rims (drones
  homing on it).
- **Red beacon** on the top of the head mast (the highest point when standing on a planet).
- No other glow: the reactor and engines show nothing until they fire.

## Heat

In vacuum heat leaves only by radiation: the reactor's coolant runs from the waist up the back
to the radiator fin banks between the back armour; nothing vents.

## Markings

Hazard stripes only at pinch points and moving edges (chest–abdomen gap, bay door edges) and
round the airlocks and the docking collar (keep clear).

## Surface

Big sculpted armour plates; the plate seams, fasteners and wear on them are drawn by the hull
detail shader at real sizes (plates about 3 m by 6 m), so the machine reads as 100 m from the
plating alone.
