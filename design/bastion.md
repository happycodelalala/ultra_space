# The Bastion: functional layout

An original titan, about 100 m tall, carried in a cocoon of four quarter-shells that open into
flying platforms (vehicle data: `content/data/vehicles/bastion.json`; built and checked by
`impl/rust/crates/sim_vehicle/tests/bastion.rs`). This document says where its crew are, how
they move around it and what each visible detail is for, so the models stay honest: **a
window has a room behind it, a light lights something, a ladder or railing is on a route
people use, a vent is where heat or air leaves, a hatch opens into a space.** Anything that
fails that test is left off.

## Crew and spaces

About fourteen crew.

| Space | Where | Seen from outside as |
|---|---|---|
| Bridge: pilot, commander, sensor operator | Head, at the window band | The window band across the front and round the sides (the only windows on the machine) |
| Operations, crew quarters, galley | Upper torso, behind the chest armour | Nothing: armoured, no windows |
| Engineering: reactor control | Torso core, beside the reactor | Nothing |
| Reactor | Chest centre | The armoured intake ring with its louvres (cooling air in) |
| Lift shaft | Torso core to the neck | The neck bearing is hollow: the lift runs up into the head |
| Drone crews | Shell bays (when the bays are open they are worked from the platform) | Bay lights inside the bays |

## Routes

- **Boarding on the ground:** the crew lift is a cage on cables lowered by a davit on the
  waist gallery (left side), from the gallery to the ground. Standing, the gallery is about
  55 m up.
- **Waist gallery:** a catwalk with railings right round the torso at the waist. Doors into
  the torso on both flanks, each lit by a lamp over it. From the gallery, a caged ladder up
  each flank to the shoulder deck.
- **Shoulder decks:** the flat tops of the two shoulder slabs, railed round their outer edges,
  each with a deck hatch down into the torso. Crews work there on the shell-arm roots and the
  saddles. Deck lights on short posts light the deck.
- **Head:** reached by the lift. A rear hatch onto the shoulder decks is the bridge's
  emergency exit, with rungs down to the deck.
- **No one climbs the limbs, the shells or the arms while the machine moves.** They are
  serviced kneeling or parked, from outside gantries, through bolted service panels (not
  doors), so the limbs carry service panels and no ladders, railings or door lamps.

## Lights

- **Driving lights:** floodlight banks under the shoulder slabs and under the head's brow,
  aimed down and ahead: they light the ground where the machine walks.
- **Work lights:** on each forearm, aimed along it at the hand.
- **Door lamps:** one over each gallery door. **Deck lights:** on posts on the shoulder decks.
- **Bay lights:** inside each drone bay (lighting it when open); amber approach lights along
  the bay rim (drones homing on it).
- **Red obstruction beacons** only at the highest and outermost points: the top of the head
  mast, the tops of the shells and their outer hem corners.
- **No glows** anywhere else: the reactor is not visible through its intake.

## Heat and air

The reactor draws cooling air in through the chest intake; the loop's heat leaves through the
radiator fin bank on the back, the hot air out of the two exhaust stacks above it.

## Markings

Hazard stripes only on pinch points and moving edges: the gap between chest and abdomen (it
closes when the torso bends) and the drone bay doors' swinging edges.

## Surface

Big sculpted armour plates; the plate seams, fasteners and wear on them are drawn by the hull
detail shader at real sizes (plates about 3 m by 6 m), so the machine reads as 100 m from the
plating alone, without modelled tiles.
