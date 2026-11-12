//! Rolling timing statistics over a fixed window of samples.

use std::collections::VecDeque;
use std::time::Duration;

/// Keeps the most recent `capacity` duration samples and reports average, max and last.
#[derive(Clone, Debug)]
pub struct RollingStats {
    samples: VecDeque<Duration>,
    capacity: usize,
    total: Duration,
}

impl RollingStats {
    /// Creates an empty window. `capacity` is clamped to at least 1.
    pub fn new(capacity: usize) -> Self {
        let capacity = capacity.max(1);
        Self {
            samples: VecDeque::with_capacity(capacity),
            capacity,
            total: Duration::ZERO,
        }
    }

    pub fn push(&mut self, sample: Duration) {
        if self.samples.len() == self.capacity
            && let Some(old) = self.samples.pop_front()
        {
            self.total -= old;
        }
        self.samples.push_back(sample);
        self.total += sample;
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    pub fn last(&self) -> Option<Duration> {
        self.samples.back().copied()
    }

    pub fn average(&self) -> Option<Duration> {
        (!self.samples.is_empty()).then(|| self.total / self.samples.len() as u32)
    }

    pub fn max(&self) -> Option<Duration> {
        self.samples.iter().max().copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: Duration = Duration::from_millis(1);

    #[test]
    fn empty_has_no_values() {
        let s = RollingStats::new(4);
        assert!(s.is_empty());
        assert_eq!((s.average(), s.max(), s.last()), (None, None, None));
    }

    #[test]
    fn window_drops_oldest_samples() {
        let mut s = RollingStats::new(3);
        for ms in [10, 1, 2, 3] {
            s.push(MS * ms);
        }
        assert_eq!(s.len(), 3);
        assert_eq!(s.average(), Some(MS * 2));
        assert_eq!(s.max(), Some(MS * 3));
        assert_eq!(s.last(), Some(MS * 3));
    }

    #[test]
    fn zero_capacity_is_clamped() {
        let mut s = RollingStats::new(0);
        s.push(MS);
        s.push(MS * 5);
        assert_eq!(s.len(), 1);
        assert_eq!(s.last(), Some(MS * 5));
    }
}
