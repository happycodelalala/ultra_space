//! Fixed-rate simulation clock (development plan §2, rule 4: simulation runs at a fixed tick,
//! decoupled from the render frame rate).

use std::time::Duration;

/// Converts variable frame times into a whole number of fixed simulation ticks.
#[derive(Clone, Debug)]
pub struct FixedClock {
    tick: Duration,
    accumulator: Duration,
    max_ticks_per_advance: u32,
    dropped: Duration,
}

impl FixedClock {
    /// `tick_hz` ticks per second; at most `max_ticks_per_advance` ticks per call, so a long
    /// stall can't trigger a runaway catch-up ("spiral of death"). Excess time is dropped.
    pub fn new(tick_hz: u32, max_ticks_per_advance: u32) -> Self {
        assert!(tick_hz > 0, "tick rate must be positive");
        Self {
            tick: Duration::from_secs(1) / tick_hz,
            accumulator: Duration::ZERO,
            max_ticks_per_advance: max_ticks_per_advance.max(1),
            dropped: Duration::ZERO,
        }
    }

    pub fn tick_duration(&self) -> Duration {
        self.tick
    }

    pub fn tick_hz(&self) -> f64 {
        1.0 / self.tick.as_secs_f64()
    }

    /// Total simulation time dropped because the simulation fell too far behind.
    pub fn dropped(&self) -> Duration {
        self.dropped
    }

    /// How far real time has run into the next tick, in `[0, 1)`. Rendering uses it to
    /// interpolate between the last two ticks.
    pub fn alpha(&self) -> f64 {
        (self.accumulator.as_secs_f64() / self.tick.as_secs_f64()).clamp(0.0, 1.0)
    }

    /// Adds `elapsed` real time and returns how many ticks to run now.
    pub fn advance(&mut self, elapsed: Duration) -> u32 {
        self.accumulator += elapsed;
        let mut ticks = 0;
        while self.accumulator >= self.tick && ticks < self.max_ticks_per_advance {
            self.accumulator -= self.tick;
            ticks += 1;
        }
        if self.accumulator >= self.tick {
            // Still behind after the cap: drop whole ticks, keep the remainder.
            let behind = self.accumulator.as_nanos() / self.tick.as_nanos();
            let drop = self.tick * behind as u32;
            self.dropped += drop;
            self.accumulator -= drop;
        }
        ticks
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_one_tick_per_tick_duration() {
        let mut c = FixedClock::new(30, 4);
        let t = c.tick_duration();
        let half = t / 2;
        assert_eq!(c.advance(half), 0);
        // 1/30 s isn't a whole number of nanoseconds; add the exact remainder.
        assert_eq!(c.advance(t - half), 1);
        assert_eq!(c.advance(t * 2), 2);
    }

    #[test]
    fn keeps_fractional_remainder() {
        let mut c = FixedClock::new(10, 8);
        let ms = Duration::from_millis(1);
        let ticks: u32 = (0..1000).map(|_| c.advance(ms * 7)).sum();
        // 7 s at 10 Hz = 70 ticks.
        assert_eq!(ticks, 70);
        assert_eq!(c.dropped(), Duration::ZERO);
    }

    #[test]
    fn alpha_is_the_fraction_into_the_next_tick() {
        let mut c = FixedClock::new(10, 8);
        assert_eq!(c.alpha(), 0.0);
        c.advance(Duration::from_millis(125));
        assert!((c.alpha() - 0.25).abs() < 1e-9);
    }

    #[test]
    fn caps_catch_up_and_drops_excess() {
        let mut c = FixedClock::new(30, 3);
        let t = c.tick_duration();
        assert_eq!(c.advance(t * 10), 3);
        assert_eq!(c.dropped(), t * 7);
        assert_eq!(c.advance(Duration::ZERO), 0);
    }
}
