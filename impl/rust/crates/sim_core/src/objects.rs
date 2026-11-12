//! Objects: identity, frame-relative state, regions and transfers
//! (design: objects-vehicles-physics.md §1).
//!
//! - [`ObjectId`] is the permanent identity; the ECS `Entity` changes when an object moves to
//!   another region's world.
//! - [`ObjectState`] places an object relative to a frame in the frame tree.
//! - [`Objects`] is the registry: which region and entity hold each object. Invariant: every
//!   object is in exactly one region.
//! - Transfers between regions are requested during a tick (by [`soi_transfer_system`]) and
//!   applied at the tick boundary by [`Objects::apply_transfers`].

use std::any::type_name;
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use bevy_ecs::prelude::*;
use bevy_ecs::world::EntityWorldMut;
use glam::{DQuat, DVec3};

use crate::frames::{FrameId, FrameState, FrameTree};
use crate::region::RegionSet;
use crate::system::StarSystem;
use crate::time::UniverseTime;

// ---------------------------------------------------------------------------------------------
// Identity and state
// ---------------------------------------------------------------------------------------------

/// Permanent object identity. Never reused; `0` is invalid.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ObjectId(pub u64);

impl fmt::Display for ObjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "obj#{}", self.0)
    }
}

/// Hands out increasing object IDs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectIdAllocator {
    next: u64,
}

impl Default for ObjectIdAllocator {
    fn default() -> Self {
        Self { next: 1 }
    }
}

impl ObjectIdAllocator {
    pub fn allocate(&mut self) -> ObjectId {
        let id = ObjectId(self.next);
        self.next = self.next.checked_add(1).expect("ObjectId space exhausted");
        id
    }
}

/// Marks an entity as a persistent object and records its identity.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Object {
    pub id: ObjectId,
}

/// An object's state relative to a frame: position, rotation, velocity and spin of the
/// object's origin, all expressed in `frame`'s coordinates (velocity is relative to the frame).
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct ObjectState {
    pub frame: FrameId,
    pub state: FrameState,
}

impl ObjectState {
    /// At rest relative to `frame`.
    pub fn at_rest(frame: FrameId, position: DVec3, rotation: DQuat) -> Self {
        Self {
            frame,
            state: FrameState {
                position,
                rotation,
                ..FrameState::IDENTITY
            },
        }
    }

    /// The same physical state expressed relative to `target` at universe time `t` (seconds).
    pub fn in_frame(&self, frames: &FrameTree, target: FrameId, t: f64) -> Self {
        if target == self.frame {
            return *self;
        }
        let state = frames.state_in(target, self.frame, t).compose(&self.state);
        Self {
            frame: target,
            state,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Region resources
// ---------------------------------------------------------------------------------------------

/// The celestial body (index into [`StarSystem::bodies`]) whose region this world is.
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegionBody(pub usize);

/// Shared, read-only star system data available inside every region.
#[derive(Resource, Clone)]
pub struct SystemRef(pub Arc<StarSystem>);

/// Time for the tick being run: `time` is the start of the tick, `dt` its length in universe
/// seconds (tick duration × time warp).
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq)]
pub struct SimClock {
    pub time: UniverseTime,
    pub dt: f64,
}

impl SimClock {
    /// Universe time at the end of this tick, in seconds.
    pub fn end_secs(&self) -> f64 {
        self.time.as_secs_f64() + self.dt
    }
}

/// Transfers requested during this tick: (entity, destination region).
#[derive(Resource, Default, Debug)]
pub struct TransferRequests(pub Vec<(Entity, usize)>);

/// Stages of a region's schedule. Physics and other per-object systems go in `Simulate`;
/// transfer checks run afterwards, on the end-of-tick state.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RegionStage {
    Simulate,
    Transfers,
}

/// Hysteresis around sphere-of-influence boundaries: enter a child's region below
/// `1 - SOI_HYSTERESIS` of its radius, leave a region above `1 + SOI_HYSTERESIS`.
pub const SOI_HYSTERESIS: f64 = 0.01;

/// Requests transfers for objects that crossed a sphere-of-influence boundary this tick.
pub fn soi_transfer_system(
    body: Res<RegionBody>,
    system: Res<SystemRef>,
    clock: Res<SimClock>,
    objects: Query<(Entity, &ObjectState), With<Object>>,
    mut requests: ResMut<TransferRequests>,
) {
    let system = &system.0;
    let here = &system.bodies[body.0];
    let t = clock.end_secs();
    for (entity, state) in &objects {
        let position = system.frames.transform_point(
            here.inertial_frame,
            state.frame,
            t,
            state.state.position,
        );
        if let Some(destination) = destination_region(system, body.0, position, t) {
            requests.0.push((entity, destination));
        }
    }
}

/// Which region an object at `position` (in body `region`'s inertial frame) should move to,
/// if any: the parent when outside this body's sphere of influence, or a child when inside
/// one of the children's spheres.
pub fn destination_region(
    system: &StarSystem,
    region: usize,
    position: DVec3,
    t: f64,
) -> Option<usize> {
    let here = &system.bodies[region];
    if let Some(parent) = here.parent
        && position.length() > here.soi_radius * (1.0 + SOI_HYSTERESIS)
    {
        return Some(parent);
    }
    system.bodies.iter().enumerate().find_map(|(i, child)| {
        if child.parent != Some(region) {
            return None;
        }
        let child_position = system.frames.transform_point(
            here.inertial_frame,
            child.inertial_frame,
            t,
            DVec3::ZERO,
        );
        ((position - child_position).length() < child.soi_radius * (1.0 - SOI_HYSTERESIS))
            .then_some(i)
    })
}

// ---------------------------------------------------------------------------------------------
// Transferable components
// ---------------------------------------------------------------------------------------------

/// A component removed from an object, ready to insert into its new entity.
type TakenComponent = Box<dyn for<'w> FnOnce(&mut EntityWorldMut<'w>) + Send>;
type TakeFn = fn(&mut EntityWorldMut<'_>) -> Option<TakenComponent>;

fn take_component<T: Component>(entity: &mut EntityWorldMut<'_>) -> Option<TakenComponent> {
    entity.take::<T>().map(|c| {
        Box::new(move |dst: &mut EntityWorldMut<'_>| {
            dst.insert(c);
        }) as TakenComponent
    })
}

/// A pending move of one object to another region.
struct Package {
    id: ObjectId,
    state: ObjectState,
    components: Vec<TakenComponent>,
    to: usize,
}

// ---------------------------------------------------------------------------------------------
// Registry
// ---------------------------------------------------------------------------------------------

/// Where an object currently lives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObjectLocation {
    pub region: usize,
    pub entity: Entity,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ObjectError {
    UnknownObject(ObjectId),
    UnknownRegion(usize),
}

impl fmt::Display for ObjectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownObject(id) => write!(f, "unknown object {id}"),
            Self::UnknownRegion(r) => write!(f, "unknown region {r}"),
        }
    }
}

impl std::error::Error for ObjectError {}

/// The object registry. Every object is in exactly one region.
#[derive(Default)]
pub struct Objects {
    allocator: ObjectIdAllocator,
    locations: HashMap<ObjectId, ObjectLocation>,
    transferable: Vec<(&'static str, TakeFn)>,
}

impl Objects {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a component type that moves with objects between regions. Components that
    /// aren't registered are dropped on transfer (region-local caches only).
    pub fn register_transferable<T: Component>(&mut self) {
        let name = type_name::<T>();
        if !self.transferable.iter().any(|(n, _)| *n == name) {
            self.transferable.push((name, take_component::<T>));
        }
    }

    pub fn len(&self) -> usize {
        self.locations.len()
    }

    pub fn is_empty(&self) -> bool {
        self.locations.is_empty()
    }

    pub fn location(&self, id: ObjectId) -> Option<ObjectLocation> {
        self.locations.get(&id).copied()
    }

    pub fn ids(&self) -> impl Iterator<Item = ObjectId> + '_ {
        self.locations.keys().copied()
    }

    /// Spawns a new object in `region` with `state` and any extra components.
    pub fn spawn(
        &mut self,
        regions: &mut RegionSet,
        region: usize,
        state: ObjectState,
        extra: impl Bundle,
    ) -> Result<ObjectId, ObjectError> {
        let world = &mut regions
            .get_mut(region)
            .ok_or(ObjectError::UnknownRegion(region))?
            .world;
        let id = self.allocator.allocate();
        let entity = world.spawn((Object { id }, state, extra)).id();
        self.locations.insert(id, ObjectLocation { region, entity });
        Ok(id)
    }

    pub fn despawn(&mut self, regions: &mut RegionSet, id: ObjectId) -> Result<(), ObjectError> {
        let loc = self
            .locations
            .remove(&id)
            .ok_or(ObjectError::UnknownObject(id))?;
        if let Some(region) = regions.get_mut(loc.region) {
            region.world.despawn(loc.entity);
        }
        Ok(())
    }

    /// An object's current state.
    pub fn state(&self, regions: &RegionSet, id: ObjectId) -> Option<ObjectState> {
        let loc = self.location(id)?;
        regions
            .get(loc.region)?
            .world
            .get::<ObjectState>(loc.entity)
            .copied()
    }

    /// Applies every transfer requested during the last tick. `t` is the universe time (s) at
    /// the tick boundary; states are converted to the destination body's inertial frame.
    pub fn apply_transfers(&mut self, regions: &mut RegionSet, system: &StarSystem, t: f64) {
        let mut packages = Vec::new();
        for region in regions.iter_mut() {
            let requests = match region.world.get_resource_mut::<TransferRequests>() {
                Some(mut r) if !r.0.is_empty() => std::mem::take(&mut r.0),
                _ => continue,
            };
            for (entity, to) in requests {
                let Ok(mut e) = region.world.get_entity_mut(entity) else {
                    continue;
                };
                let (Some(object), Some(state)) = (e.take::<Object>(), e.take::<ObjectState>())
                else {
                    continue;
                };
                let components = self
                    .transferable
                    .iter()
                    .filter_map(|(_, take)| take(&mut e))
                    .collect();
                e.despawn();
                packages.push(Package {
                    id: object.id,
                    state,
                    components,
                    to,
                });
            }
        }

        for package in packages {
            let target_frame = system.bodies[package.to].inertial_frame;
            let state = package.state.in_frame(&system.frames, target_frame, t);
            let Some(region) = regions.get_mut(package.to) else {
                // Should be impossible (destinations come from the star system); keep the
                // invariant by forgetting the object rather than leaving a dangling entry.
                self.locations.remove(&package.id);
                continue;
            };
            let mut e = region.world.spawn((Object { id: package.id }, state));
            for insert in package.components {
                insert(&mut e);
            }
            let entity = e.id();
            self.locations.insert(
                package.id,
                ObjectLocation {
                    region: package.to,
                    entity,
                },
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::region::Region;

    fn halcyon() -> Arc<StarSystem> {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../content/data/star_systems/halcyon.json");
        Arc::new(StarSystem::from_path(path).unwrap())
    }

    fn index(sys: &StarSystem, id: &str) -> usize {
        sys.bodies.iter().position(|b| b.id == id).unwrap()
    }

    /// One region per body with the transfer check, like `Simulation::for_star_system`.
    fn body_regions(sys: &Arc<StarSystem>, t: f64) -> RegionSet {
        let mut regions = RegionSet::new();
        for (i, body) in sys.bodies.iter().enumerate() {
            let mut world = World::new();
            world.insert_resource(RegionBody(i));
            world.insert_resource(SystemRef(sys.clone()));
            world.insert_resource(SimClock {
                time: UniverseTime::from_nanos((t * 1e9) as u64),
                dt: 0.0,
            });
            world.init_resource::<TransferRequests>();
            let mut schedule = Schedule::default();
            schedule.add_systems(soi_transfer_system.in_set(RegionStage::Transfers));
            regions.push(Region::new(body.id.clone(), world, schedule));
        }
        regions
    }

    #[derive(Component, Clone, Debug, PartialEq)]
    struct Cargo(u32);

    #[test]
    fn spawn_locate_despawn() {
        let sys = halcyon();
        let mut regions = body_regions(&sys, 0.0);
        let mut objects = Objects::new();
        let m = index(&sys, "meridian");
        let state = ObjectState::at_rest(
            sys.bodies[m].inertial_frame,
            DVec3::new(2.0e6, 0.0, 0.0),
            DQuat::IDENTITY,
        );
        let a = objects.spawn(&mut regions, m, state, Cargo(5)).unwrap();
        let b = objects.spawn(&mut regions, m, state, ()).unwrap();
        assert_ne!(a, b);
        assert_eq!(objects.location(a).unwrap().region, m);
        assert_eq!(objects.state(&regions, a), Some(state));
        objects.despawn(&mut regions, a).unwrap();
        assert_eq!(objects.location(a), None);
        assert_eq!(
            objects.despawn(&mut regions, a),
            Err(ObjectError::UnknownObject(a))
        );
        assert_eq!(objects.len(), 1);
        assert!(objects.spawn(&mut regions, 99, state, ()).is_err());
    }

    #[test]
    fn crossing_into_a_moon_transfers_once_and_keeps_position() {
        let sys = halcyon();
        let (m, l) = (index(&sys, "meridian"), index(&sys, "lantern"));
        let t = 1_000.0;
        let mut regions = body_regions(&sys, t);
        let mut objects = Objects::new();
        objects.register_transferable::<Cargo>();

        // 500 km from Lantern's center: inside Lantern's sphere of influence (~1,800 km).
        let lantern_frame = sys.bodies[l].inertial_frame;
        let meridian_frame = sys.bodies[m].inertial_frame;
        let near_lantern = sys.frames.transform_point(
            meridian_frame,
            lantern_frame,
            t,
            DVec3::new(500_000.0, 0.0, 0.0),
        );
        let mut state = ObjectState::at_rest(meridian_frame, near_lantern, DQuat::IDENTITY);
        state.state.velocity = DVec3::new(10.0, 20.0, -30.0);
        let id = objects.spawn(&mut regions, m, state, Cargo(7)).unwrap();
        let before = state.in_frame(&sys.frames, sys.star().inertial_frame, t);

        regions.tick();
        objects.apply_transfers(&mut regions, &sys, t);

        let loc = objects.location(id).unwrap();
        assert_eq!(loc.region, l, "moved to Lantern's region");
        let world = &regions.get(l).unwrap().world;
        assert_eq!(
            world.get::<Cargo>(loc.entity),
            Some(&Cargo(7)),
            "registered components travel"
        );
        let after = objects.state(&regions, id).unwrap();
        assert_eq!(after.frame, lantern_frame);
        let after_star = after.in_frame(&sys.frames, sys.star().inertial_frame, t);
        assert!((after_star.state.position - before.state.position).length() < 1e-3);
        assert!((after_star.state.velocity - before.state.velocity).length() < 1e-6);
        // Exactly one copy remains.
        let in_meridian = regions
            .get_mut(m)
            .unwrap()
            .world
            .query::<&Object>()
            .iter(&regions.get(m).unwrap().world)
            .count();
        assert_eq!(in_meridian, 0);

        // Next tick: already in the right region, nothing moves.
        regions.tick();
        objects.apply_transfers(&mut regions, &sys, t);
        assert_eq!(objects.location(id).unwrap(), loc);
    }

    #[test]
    fn leaving_a_sphere_of_influence_goes_to_the_parent() {
        let sys = halcyon();
        let m = index(&sys, "meridian");
        let t = 0.0;
        let mut regions = body_regions(&sys, t);
        let mut objects = Objects::new();
        let far = DVec3::new(sys.bodies[m].soi_radius * 1.05, 0.0, 0.0);
        let id = objects
            .spawn(
                &mut regions,
                m,
                ObjectState::at_rest(sys.bodies[m].inertial_frame, far, DQuat::IDENTITY),
                (),
            )
            .unwrap();
        regions.tick();
        objects.apply_transfers(&mut regions, &sys, t);
        assert_eq!(objects.location(id).unwrap().region, 0, "the star's region");
    }

    #[test]
    fn hysteresis_prevents_flicker_at_the_boundary() {
        let sys = halcyon();
        let m = index(&sys, "meridian");
        let soi = sys.bodies[m].soi_radius;
        // Just outside Meridian's sphere but within the hysteresis band: stays in Meridian's
        // region; seen from the star's region it is not inside the entry threshold either.
        let outside = DVec3::new(soi * 1.005, 0.0, 0.0);
        assert_eq!(destination_region(&sys, m, outside, 0.0), None);
        let star_frame = sys.star().inertial_frame;
        let in_star =
            sys.frames
                .transform_point(star_frame, sys.bodies[m].inertial_frame, 0.0, outside);
        assert_eq!(destination_region(&sys, 0, in_star, 0.0), None);
        let inside = DVec3::new(soi * 0.995, 0.0, 0.0);
        let in_star =
            sys.frames
                .transform_point(star_frame, sys.bodies[m].inertial_frame, 0.0, inside);
        assert_eq!(
            destination_region(&sys, 0, in_star, 0.0),
            None,
            "inside the band, not yet below 99%"
        );
    }

    #[test]
    fn many_objects_spread_over_regions() {
        let sys = halcyon();
        let mut regions = body_regions(&sys, 0.0);
        let mut objects = Objects::new();
        for i in 0..1_000 {
            let region = i % sys.bodies.len();
            let body = &sys.bodies[region];
            let p = DVec3::new(body.radius * 2.0 + i as f64, 0.0, 0.0);
            objects
                .spawn(
                    &mut regions,
                    region,
                    ObjectState::at_rest(body.inertial_frame, p, DQuat::IDENTITY),
                    (),
                )
                .unwrap();
        }
        regions.tick();
        objects.apply_transfers(&mut regions, &sys, 0.0);
        assert_eq!(objects.len(), 1_000);
        // Invariant: each object appears exactly once across all regions.
        let mut seen = std::collections::HashSet::new();
        for region in regions.iter_mut() {
            let world = &mut region.world;
            for object in world.query::<&Object>().iter(world) {
                assert!(seen.insert(object.id), "{} in two regions", object.id);
            }
        }
        assert_eq!(seen.len(), 1_000);
    }
}
