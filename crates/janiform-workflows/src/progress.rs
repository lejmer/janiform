use std::time::Duration;
use std::time::Instant;

const VOLATILE_PROGRESS_MIN_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Debug)]
pub(crate) struct VolatileProgressGate<K> {
    minimum_interval: Duration,
    last_emitted_at: Option<Instant>,
    last_key: Option<K>,
}

impl<K> VolatileProgressGate<K> {
    pub(crate) const fn new(minimum_interval: Duration) -> Self {
        Self {
            minimum_interval,
            last_emitted_at: None,
            last_key: None,
        }
    }
}

impl<K: PartialEq> VolatileProgressGate<K> {
    pub(crate) fn should_emit(
        &mut self,
        now: Instant,
        key: K,
        completed_units: usize,
        total_units: usize,
    ) -> bool {
        let should_emit = self
            .last_key
            .as_ref()
            .is_none_or(|last_key| *last_key != key)
            || self.last_emitted_at.is_none_or(|last_emitted_at| {
                now.duration_since(last_emitted_at) >= self.minimum_interval
            })
            || (total_units > 0 && completed_units >= total_units);

        if should_emit {
            self.last_emitted_at = Some(now);
            self.last_key = Some(key);
        }

        should_emit
    }
}

impl<K> Default for VolatileProgressGate<K> {
    fn default() -> Self {
        Self::new(VOLATILE_PROGRESS_MIN_INTERVAL)
    }
}

pub(crate) fn progress_percent(completed_units: usize, total_units: usize) -> usize {
    if total_units == 0 {
        return 0;
    }

    completed_units.saturating_mul(100) / total_units
}

#[cfg(test)]
mod tests {
    use std::time::Duration;
    use std::time::Instant;

    use super::{VolatileProgressGate, progress_percent};

    #[test]
    fn volatile_progress_gate_emits_first_update() {
        let mut gate = VolatileProgressGate::new(Duration::from_millis(100));

        assert!(gate.should_emit(Instant::now(), "phase-a", 0, 10));
    }

    #[test]
    fn volatile_progress_gate_suppresses_same_phase_inside_interval() {
        let mut gate = VolatileProgressGate::new(Duration::from_millis(100));
        let start = Instant::now();

        assert!(gate.should_emit(start, "phase-a", 1, 10));
        assert!(!gate.should_emit(start + Duration::from_millis(50), "phase-a", 2, 10));
    }

    #[test]
    fn volatile_progress_gate_emits_phase_changes_and_completion() {
        let mut gate = VolatileProgressGate::new(Duration::from_millis(100));
        let start = Instant::now();

        assert!(gate.should_emit(start, "phase-a", 1, 10));
        assert!(gate.should_emit(start + Duration::from_millis(10), "phase-b", 1, 10));
        assert!(gate.should_emit(start + Duration::from_millis(20), "phase-b", 10, 10));
    }

    #[test]
    fn progress_percent_uses_bounded_integer_units() {
        assert_eq!(progress_percent(4, 10), 40);
        assert_eq!(progress_percent(1, 0), 0);
    }
}
