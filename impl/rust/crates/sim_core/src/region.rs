//! Regions: independently simulated parts of the universe (development plan §2, rule 1).
//!
//! Each region (a star system, planet or battle area) owns its own ECS `World` and `Schedule`.
//! Regions tick in parallel on the compute task pool; systems run sequentially inside each
//! region. Benchmarked in Batch 0 (`sim_bench`) as the primary strategy.

use bevy_ecs::entity::Entity;
use bevy_ecs::query::Without;
use bevy_ecs::resource::IsResource;
use bevy_ecs::schedule::{Schedule, SingleThreadedExecutor};
use bevy_ecs::world::World;
use bevy_tasks::{ComputeTaskPool, TaskPool};

/// One independently simulated region.
pub struct Region {
    pub name: String,
    pub world: World,
    pub schedule: Schedule,
}

impl Region {
    /// Creates a region whose systems run sequentially (parallelism comes from ticking
    /// regions concurrently).
    pub fn new(name: impl Into<String>, world: World, mut schedule: Schedule) -> Self {
        schedule.set_executor(SingleThreadedExecutor::new());
        Self {
            name: name.into(),
            world,
            schedule,
        }
    }

    /// An empty region with no systems.
    pub fn empty(name: impl Into<String>) -> Self {
        Self::new(name, World::new(), Schedule::default())
    }

    pub fn tick(&mut self) {
        self.schedule.run(&mut self.world);
    }

    /// Simulated entities: excludes resource entities (Bevy 0.20 stores resources as entities)
    /// and disabled internal entities (hidden by default query filters). O(n); meant for
    /// debug displays, not hot paths.
    pub fn entity_count(&mut self) -> u64 {
        let mut query = self.world.query_filtered::<Entity, Without<IsResource>>();
        query.iter(&self.world).count() as u64
    }
}

/// All regions, ticked together.
#[derive(Default)]
pub struct RegionSet {
    regions: Vec<Region>,
}

impl RegionSet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, region: Region) {
        self.regions.push(region);
    }

    pub fn len(&self) -> usize {
        self.regions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.regions.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Region> {
        self.regions.iter()
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut Region> {
        self.regions.iter_mut()
    }

    /// Total simulated entities across all regions (see [`Region::entity_count`]).
    pub fn entity_count(&mut self) -> u64 {
        self.regions.iter_mut().map(Region::entity_count).sum()
    }

    /// Ticks every region once, in parallel on the compute task pool (initialized with
    /// default settings if nothing else has set it up yet).
    pub fn tick(&mut self) {
        match self.regions.len() {
            0 => {}
            1 => self.regions[0].tick(),
            _ => {
                let pool = ComputeTaskPool::get_or_init(TaskPool::default);
                pool.scope(|scope| {
                    for region in self.regions.iter_mut() {
                        scope.spawn(async move { region.tick() });
                    }
                });
            }
        }
    }

    /// Ticks every region once on the calling thread (baselines, debugging).
    pub fn tick_serial(&mut self) {
        self.regions.iter_mut().for_each(Region::tick);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy_ecs::prelude::*;

    #[derive(Resource, Default)]
    struct Ticks(u32);

    fn count(mut t: ResMut<Ticks>) {
        t.0 += 1;
    }

    fn counting_region(name: &str) -> Region {
        let mut world = World::new();
        world.init_resource::<Ticks>();
        world.spawn_empty();
        let mut schedule = Schedule::default();
        schedule.add_systems(count);
        Region::new(name, world, schedule)
    }

    #[test]
    fn parallel_and_serial_tick_every_region() {
        let mut set = RegionSet::new();
        for i in 0..8 {
            set.push(counting_region(&format!("r{i}")));
        }
        set.tick();
        set.tick_serial();
        for r in set.iter() {
            assert_eq!(r.world.resource::<Ticks>().0, 2, "{}", r.name);
        }
        assert_eq!(set.entity_count(), 8);
    }

    #[test]
    fn empty_set_ticks_without_work() {
        let mut set = RegionSet::new();
        set.tick();
        assert!(set.is_empty());
    }
}
