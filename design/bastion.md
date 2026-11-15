# The Bastion: functional layout

An original titan, about 100 m tall, for **interstellar exploration and combat**: it works in
vacuum, on airless and atmospheric worlds, in low and full gravity, and under fire. Its
technology is **interstellar-capable, plausible advanced technology**: mature engineering and
strong materials, but no magic. Heat still needs radiator area, loads still need visible
structure, and joints, access and servicing stay visible. Fittings are integrated and
conformal, not present-day hardware bolted on, and flush never means featureless. Four
quarter-shells close round it as a cocoon (armour in transit and in a fight) and open into
platforms carrying its main engines and drone bays (vehicle data:
`content/data/vehicles/bastion.json`; built and checked by
`impl/rust/crates/sim_vehicle/tests/bastion.rs`).

This document says where its pilot and crew are, how they move, how it stays alive in space
and what each visible detail is for, so the models stay honest: **a light lights or marks
something, a hatch opens into a space, a handrail is where a suited person holds on at a
door, a radiator is where heat leaves.** Nothing open-air: no walkways, railings, deck lights, cranes, air intakes or
exhaust stacks, which would only work on a planet with air.

## Crew and spaces

**One pilot** drives it through an enhanced neural link, like a huge version of their own
body (the four shell arms are extra limbs in the same body map). The pilot's capsule and the
control core sit in the upper torso behind the central chest shield, the thickest armour,
shielded from the reactor below by its shadow shield. Around them, in the same pressurised
**crew module**, are rooms for **a few more crew** (passengers or specialists): cabins,
galley, stores. There is one pilot and one control station; nobody else drives it. **No
windows** anywhere: the pilot sees through the sensors (a window is a hole in the armour and
useless at combat ranges).

| What | Where | Seen from outside as |
|---|---|---|
| Pilot capsule and crew module | Upper torso, behind the chest armour | The chest boarding door in the central shield; otherwise armour |
| Reactor (fusion) | Pelvis / waist, below the crew module | The armoured waist; its heat leaves through the radiators |
| Sensors | The head (no crew): radar, optical and infrared turrets, lidar, star trackers, searchlights | A sensor head |
| Radiators | Back of the torso, a flat finely ribbed panel between armoured blades (standby and life support); the shells when deployed (reactor power) | The radiator panel, its louvre box and guide tracks, the return header |
| Communications | The comms deck on the top back ledge behind the head: two optical (laser) terminals for long range, a conformal RF phased array for near and planetary links; whip antennas on the head | Two domes in armoured wells, the array panel |
| Drones | Bays in the shells | Bay doors; lights inside the bays |

## Getting in and out

- **Chest boarding door** (the main way in, "the chest opens"): a large armoured door,
  5.2 × 7.2 m, cut into the central shield in front of the crew module, sized for people
  and small cargo. It is a stepped plug as thick as the shield (two seams show it) and locks
  from inside, behind the plug seam: no outside dogs. It sits back in a dark door frame, the
  load ring round the opening: a jamb up each side and a lintel across the head, 0.6 m wide
  and 0.35 m proud of the shield, so the opening reads as a doorway at a glance. It hinges at
  its foot on a hinge line across the sill (two knuckles on the leaf between the frame's,
  in a recessed housing; the opening actuators are inside) and opens down and out into a
  landing. The external release (a yellow pull bar on a dark plate, the rescue arrow over
  it) is on the right jamb, low: part of the door, not a separate hatch.
- **Boarding from the ground: the mech's own hand.** The pilot cups a palm and raises it to
  the chest, like a person lifting something to their chest; the open leaf meets it as a
  landing. On the torso this needs only the landing fittings: the lintel light in the door
  frame's lintel, aimed down at the open leaf, and a recessed mooring socket under each
  jamb's foot, at the ends of the sill, where
  the hand (or a shell arm's grapple with a platform, or a utility craft from the shell
  bays) locks on. The boarding pose itself (the hand at the chest) is for later: it is new
  pose data, and until then the route is documented, not shown.
- **Reactor-bay service hatches** on both flank plates, one a side (mirrored): an armoured
  panel, 3.4 × 4.2 m, over the bay in the lower torso that holds the reactor's power
  conversion and coolant pumps (the reactor itself is in the waist below). It lifts up and
  out on two hinges at its head, a roof over the worker; the release handle is in a well at
  its foot; a stencil under it.
- **No dedicated airlock.** The docking port, or any hatch (the chest door, a service
  hatch), works as one: each opens into an airlock chamber behind it, inside the hull. EVA
  and hatch service from a flank use the service pose stage below.
- **Service pose stage** (pose backlog, not yet pose data): one shared stage in which the arm
  on that side swings out on its shoulder roll, clearing the service hatch's 4.2 m lift-up
  swing and the space in front of it (in stand the hanging upper arm is about 2.7 m from the
  plate's face, so the hatch can't open fully). It serves hatch service and EVA from either
  flank.
- **Docking port** on the lower back, below the radiator: a docking collar for shuttles and
  stations, recessed into the lower back plate inside a 0.3 m armour lip, docking-light
  lenses set into the collar face, a narrow striped keep-clear band round it, a work-light
  lens over it and a recessed rail channel either side.
- **Outside, nothing is a route:** rail channels (a dark slot 0.4 m wide with a recessed bar,
  the hand and tether point) only at the docking port.
  The torso's edges are rigid (the shoulders and waist bolt on), so there are no outside
  work sites; limbs and shells are serviced docked or parked, through bolted service panels.

## Moving

- **Main engines:** the four lift engines on each shell (in flight the shells are platforms).
- **Attitude:** RCS thruster quads (four nozzles each) at the true extremities, on the
  shells (none on the torso: its corners sit under the shoulders and inside the cocoon); at
  100 m a small push far from the centre turns it.
- **Walking:** sealed hydraulic rams with dust boots over their rods (regolith gets into
  everything); the feet have retractable anchor spikes for low gravity.

## Lights

- **Navigation lights:** red on the left, cyan on the right (not green, which would merge
  with the green armour; the palette slot keeps its name `nav_green`), as wraparound lenses
  set into the outer front corners of the torso's shoulder slabs, seen from the front and the
  side; white strobes on the shells' outer corners and the head mast: so other craft see which way
  it faces and how big it is. The shells' outer faces are their final home (in cocoon the
  torso ones are hidden); the torso ones go when the shell audit adds them there.
- **Terrain lights:** lens strips set into the underside of the chest plates, and on the
  shins, aimed down and ahead, for walking on the dark side of a world.
- **Searchlights:** in the sensor head, aimed where the sensors look.
- **Work lights:** a lens strip over the docking port; the lintel light in the chest door's
  lintel, aimed down at its open leaf.
- **Lamp colours:** the torso's floods and work lights (terrain lenses, lintel light,
  docking-port work light and docking lights) are white (`strobe` slot); orange (`light`) is
  kept for the reactor and the visor.
- **Bay lights:** inside each drone bay; amber approach lights along the bay rims (drones
  homing on it).
- **Red beacon** on the top of the head mast (the highest point when standing on a planet).
- No other glow: the reactor and engines show nothing until they fire.

## Heat

In vacuum heat leaves only by radiation; nothing vents. The torso's radiator is the
**standby and life-support radiator**: a flat emitting panel between the back blades, facing
aft, its fine ribs the heat pipes' fins (about 63 m², a few MW). The coolant runs inside the
frame: in through the supply header in the louvre box over the panel, out through the
armoured return header across its foot, whose ends go into the frame through flush collars.
Armoured louvres run down over the panel in a fight on guide tracks either side. Where the
coolant crosses into the waist, a flush bolted service hatch on the lower back plate's lower
corners gives access to disconnect it. The reactor's power is rejected by the deployed
shells, so with the cocoon closed the heat budget relies on the shells too (closed only in
extreme conditions or storage).

## Markings

Hazard yellow only at pinch points and moving edges (bay door edges), round the docking
collar (keep clear), on the chest door's external release and on the service hatches'
release handles. The hull number (01) is a low-visibility stencil on the left shoulder slab;
small keep-clear stencils under the service hatches and at the docking port give the human
scale.

## Structure

The torso's dark frame carries the chest, both arms and all four shells down into the waist:
steel longerons on its front and back faces (two a side, converging into the waist), tied on
the back by a frame rib at each lower band line (in front the abdomen bands cover the frame),
show that load path. Each shoulder hangs on a mounting flange ring on the frame's side (its
face and fasteners are hidden under the shoulder).
The two flank plates (armour over the lower torso's sides) each hang from a head tucked under
the chest plate, a dark seam between head and plate, and the frame carries them: an armoured
lip closes the space between the frame's side and the plate down to the plate's foot (its
soffit slopes from the foot onto the frame), and under each plate edge, front and back, a
tapered gusset (the plate's own cast foot) runs from the frame's side up to the plate's lower
inner corner, where the hanging plate's outward moment is taken.

## Surface

Big sculpted armour plates, shaded with hard facets (each large flat face stays flat to its
edge: chest and flank plates, central shield and door leaf, shoulder slabs, back blades,
abdomen bands); the plate seams, fasteners and wear on them are drawn by the hull
detail shader at real sizes (plates about 3 m by 6 m), so the machine reads as 100 m from the
plating alone.
