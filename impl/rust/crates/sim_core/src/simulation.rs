//! The simulation: regions driven by a fixed-rate clock, with tick timing statistics.
//! Front ends call [`Simulation::advance`] once per frame and read the stats for debug UI.

use std::time::{Duration, Instant};

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
}

impl Simulation {
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
        }
    }

    /// Adds `elapsed` real time and runs however many fixed ticks are due.
    /// Returns the number of ticks run.
    pub fn advance(&mut self, elapsed: Duration) -> u32 {
        let ticks = self.clock.advance(elapsed);
        for _ in 0..ticks {
            let start = Instant::now();
            self.regions.tick();
            self.tick_stats.push(start.elapsed());
            self.ticks_total += 1;
            self.universe_time
                .advance(self.clock.tick_duration() * self.time_warp);
        }
        ticks
    }

    /// Current universe time (advances by one tick duration × time warp per tick).
    pub fn universe_time(&self) -> UniverseTime {
        self.universe_time
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
