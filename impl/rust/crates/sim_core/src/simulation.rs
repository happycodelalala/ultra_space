//! The simulation: regions driven by a fixed-rate clock, with tick timing statistics.
//! Front ends call [`Simulation::advance`] once per frame and read the stats for debug UI.
//!
//! [`Simulation::for_star_system`] creates one region per celestial body with the object
//! registry: each tick sets every region's [`SimClock`], ticks all regions in parallel, then
//! applies object transfers at the tick boundary.

use std::sync::Arc;
use std::time::{Duration, Instant};

use bevy_ecs::prelude::*;

use crate::objects::{
    Objects, RegionBody, RegionStage, SimClock, SystemRef, TransferRequests, soi_transfer_system,
};
use crate::region::Region;
use crate::system::StarSystem;
use crate::time::UniverseTime;
use crate::{FixedClock, RegionSet, RollingStats};

/// Default simulation rate (ticks per second).
pub const DEFAULT_TICK_HZ: u32 = 30;

/// Maximum ticks run in one `advance` call before excess time is dropped.
pub const MAX_TICKS_PER_ADVANCE: u32 = 4;

/// Time warp levels (universe seconds per real second). Warp skips universe time per tick;
/// things on rails stay exact at any level.
pub const TIME_WARP_LEVELS: [u32; 5] = [1, 10, 100, 1_000, 10_000];

pub struct Simulation {
    pub regions: RegionSet,
    clock: FixedClock,
    tick_stats: RollingStats,
    ticks_total: u64,
    universe_time: UniverseTime,
    time_warp: u32,
    /// The star system, when regions are its bodies (see [`Self::for_star_system`]).
    system: Option<Arc<StarSystem>>,
    pub objects: Objects,
}

impl Simulation {
    /// One region per body of `system` (region index = body index), each with the shared
    /// system data, the tick clock and the sphere-of-influence transfer check. `configure`
    /// adds per-region systems (physics and so on) to each region's schedule, normally in
    /// [`RegionStage::Simulate`].
    pub fn for_star_system(
        system: Arc<StarSystem>,
        configure: impl Fn(usize, &mut Schedule),
    ) -> Self {
        let mut regions = RegionSet::new();
        for (i, body) in system.bodies.iter().enumerate() {
            let mut world = World::new();
            world.insert_resource(RegionBody(i));
            world.insert_resource(SystemRef(system.clone()));
            world.init_resource::<SimClock>();
            world.init_resource::<TransferRequests>();
            let mut schedule = Schedule::default();
            schedule.configure_sets((RegionStage::Simulate, RegionStage::Transfers).chain());
            schedule.add_systems(soi_transfer_system.in_set(RegionStage::Transfers));
            configure(i, &mut schedule);
            regions.push(Region::new(body.id.clone(), world, schedule));
        }
        let mut sim = Self::new(regions);
        sim.system = Some(system);
        sim
    }

    /// The star system whose bodies the regions are, if any.
    pub fn star_system(&self) -> Option<&Arc<StarSystem>> {
        self.system.as_ref()
    }

    pub fn new(regions: RegionSet) -> Self {
        Self::with_rate(regions, DEFAULT_TICK_HZ)
    }

    pub fn with_rate(regions: RegionSet, tick_hz: u32) -> Self {
        Self {
            regions,
            clock: FixedClock::new(tick_hz, MAX_TICKS_PER_ADVANCE),
            // About 2 seconds of ticks at the default rate.
            tick_stats: RollingStats::new(tick_hz as usize * 2),
            ticks_total: 0,
            universe_time: UniverseTime::EPOCH,
            time_warp: 1,
            system: None,
            objects: Objects::new(),
        }
    }

    /// Adds `elapsed` real time and runs however many fixed ticks are due.
    /// Returns the number of ticks run.
    pub fn advance(&mut self, elapsed: Duration) -> u32 {
        let ticks = self.clock.advance(elapsed);
        for _ in 0..ticks {
            let start = Instant::now();
            let step = self.clock.tick_duration() * self.time_warp;
            let clock = SimClock {
                time: self.universe_time,
                dt: step.as_secs_f64(),
            };
            for region in self.regions.iter_mut() {
                if let Some(mut c) = region.world.get_resource_mut::<SimClock>() {
                    *c = clock;
                }
            }
            self.regions.tick();
            self.universe_time.advance(step);
            if let Some(system) = &self.system {
                let t = self.universe_time.as_secs_f64();
                self.objects.apply_transfers(&mut self.regions, system, t);
            }
            self.tick_stats.push(start.elapsed());
            self.ticks_total += 1;
        }
        ticks
    }

    /// Sets universe time (loading a save); warp goes back to 1×.
    pub fn set_universe_time(&mut self, time: UniverseTime) {
        self.universe_time = time;
        self.time_warp = 1;
    }

    /// Current universe time (advances by one tick duration × time warp per tick).
    pub fn universe_time(&self) -> UniverseTime {
        self.universe_time
    }

    /// Universe time (s) to render at: one tick behind the simulation, plus the fraction of the
    /// next tick that real time has already covered. Always between the last two ticks, so
    /// rendering can interpolate object states and stays smooth at any frame rate.
    pub fn render_time(&self) -> f64 {
        let step = self.clock.tick_duration().as_secs_f64() * f64::from(self.time_warp);
        self.universe_time.as_secs_f64() - step * (1.0 - self.clock.alpha())
    }

    pub fn time_warp(&self) -> u32 {
        self.time_warp
    }

    /// Sets time warp (universe seconds per real second, at least 1).
    pub fn set_time_warp(&mut self, warp: u32) {
        self.time_warp = warp.max(1);
    }

    /// Steps time warp up or down through [`TIME_WARP_LEVELS`].
    pub fn step_time_warp(&mut self, up: bool) {
        let i = TIME_WARP_LEVELS
            .iter()
            .position(|&w| w >= self.time_warp)
            .unwrap_or(0);
        let i = if up {
            (i + 1).min(TIME_WARP_LEVELS.len() - 1)
        } else {
            i.saturating_sub(1)
        };
        self.time_warp = TIME_WARP_LEVELS[i];
    }

    pub fn clock(&self) -> &FixedClock {
        &self.clock
    }

    /// Wall-clock time per tick over the recent window.
    pub fn tick_stats(&self) -> &RollingStats {
        &self.tick_stats
    }

    pub fn ticks_total(&self) -> u64 {
        self.ticks_total
    }

    /// Fraction of the tick budget used by the average recent tick (1.0 = exactly on budget).
    pub fn budget_used(&self) -> Option<f64> {
        let avg = self.tick_stats.average()?;
        Some(avg.as_secs_f64() / self.clock.tick_duration().as_secs_f64())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Region;

    #[test]
    fn advance_runs_due_ticks_and_records_stats() {
        let mut regions = RegionSet::new();
        regions.push(Region::empty("a"));
        regions.push(Region::empty("b"));
        let mut sim = Simulation::with_rate(regions, 10);
        assert_eq!(sim.advance(Duration::from_millis(250)), 2);
        assert_eq!(sim.ticks_total(), 2);
        assert_eq!(sim.tick_stats().len(), 2);
        assert!(sim.budget_used().unwrap() < 1.0);
    }

    #[test]
    fn render_time_lies_between_the_last_two_ticks() {
        let mut sim = Simulation::with_rate(RegionSet::new(), 10);
        sim.set_time_warp(10);
        sim.advance(Duration::from_millis(230));
        // Two ticks of 1 s (warp 10) done, 30 ms of real time (0.3 of a tick) into the next.
        let now = sim.universe_time().as_secs_f64();
        assert!((now - 2.0).abs() < 1e-9);
        assert!(
            (sim.render_time() - 1.3).abs() < 1e-6,
            "{}",
            sim.render_time()
        );
    }

    #[test]
    fn universe_time_follows_ticks_and_warp() {
        let mut sim = Simulation::with_rate(RegionSet::new(), 10);
        sim.advance(Duration::from_secs(1));
        // Capped at MAX_TICKS_PER_ADVANCE ticks of 100 ms.
        assert_eq!(sim.universe_time().as_nanos(), 400_000_000);
        sim.set_time_warp(1_000);
        sim.advance(Duration::from_millis(100));
        assert_eq!(
            sim.universe_time().as_nanos(),
            400_000_000 + 100_000_000_000
        );
    }

    #[test]
    fn star_system_regions_tick_and_transfer_objects() {
        use std::path::PathBuf;

        use glam::{DQuat, DVec3};

        use crate::objects::{ObjectState, SimClock};

        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../content/data/star_systems/halcyon.json");
        let system = Arc::new(StarSystem::from_path(path).unwrap());
        let mut sim = Simulation::for_star_system(system.clone(), |_, _| {});
        assert_eq!(sim.regions.len(), system.bodies.len());

        // An object placed inside Lantern's sphere but registered in Meridian's region moves
        // to Lantern's region at the first tick boundary.
        let (m, l) = (2, 3);
        assert_eq!(
            (system.bodies[m].id.as_str(), system.bodies[l].id.as_str()),
            ("meridian", "lantern")
        );
        let p = system.frames.transform_point(
            system.bodies[m].inertial_frame,
            system.bodies[l].inertial_frame,
            0.0,
            DVec3::new(400_000.0, 0.0, 0.0),
        );
        let id = sim
            .objects
            .spawn(
                &mut sim.regions,
                m,
                ObjectState::at_rest(system.bodies[m].inertial_frame, p, DQuat::IDENTITY),
                (),
            )
            .unwrap();
        sim.advance(Duration::from_millis(40));
        assert_eq!(sim.objects.location(id).unwrap().region, l);
        // Every region saw this tick's clock.
        let clock = *sim.regions.get(0).unwrap().world.resource::<SimClock>();
        assert_eq!(clock.time, UniverseTime::EPOCH);
        assert!((clock.dt - 1.0 / 30.0).abs() < 1e-6);
    }

    #[test]
    fn warp_steps_through_levels() {
        let mut sim = Simulation::new(RegionSet::new());
        sim.step_time_warp(false);
        assert_eq!(sim.time_warp(), 1);
        for expected in [10, 100, 1_000, 10_000, 10_000] {
            sim.step_time_warp(true);
            assert_eq!(sim.time_warp(), expected);
        }
        sim.step_time_warp(false);
        assert_eq!(sim.time_warp(), 1_000);
    }
}
