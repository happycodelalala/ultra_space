//! The simulation: regions driven by a fixed-rate clock, with tick timing statistics.
//! Front ends call [`Simulation::advance`] once per frame and read the stats for debug UI.

use std::time::{Duration, Instant};

use crate::{FixedClock, RegionSet, RollingStats};

/// Default simulation rate (ticks per second).
pub const DEFAULT_TICK_HZ: u32 = 30;

/// Maximum ticks run in one `advance` call before excess time is dropped.
pub const MAX_TICKS_PER_ADVANCE: u32 = 4;

pub struct Simulation {
    pub regions: RegionSet,
    clock: FixedClock,
    tick_stats: RollingStats,
    ticks_total: u64,
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
        }
        ticks
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
}
