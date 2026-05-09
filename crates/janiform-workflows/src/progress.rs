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
    completed_units
        .saturating_mul(100)
        .checked_div(total_units)
        .unwrap_or(0)
}

pub(crate) fn format_cpu_count(cpu_count: usize) -> String {
    match cpu_count {
        1 => "1 CPU".to_string(),
        _ => format!("{cpu_count} CPUs"),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ProgressUnitTracker {
    completed_units: usize,
    total_units: usize,
}

impl ProgressUnitTracker {
    pub(crate) const fn new(total_units: usize) -> Self {
        Self {
            completed_units: 0,
            total_units,
        }
    }

    pub(crate) fn planned_for_totals(total_units: &[usize]) -> Vec<Self> {
        total_units.iter().copied().map(Self::new).collect()
    }

    pub(crate) const fn completed_units(&self) -> usize {
        self.completed_units
    }

    pub(crate) const fn total_units(&self) -> usize {
        self.total_units
    }

    pub(crate) fn record(&mut self, completed_units: usize) -> usize {
        let previous_completed_units = self.completed_units;
        self.completed_units = self
            .completed_units
            .max(completed_units)
            .min(self.total_units);

        self.completed_units - previous_completed_units
    }

    pub(crate) fn complete(&mut self) -> usize {
        self.record(self.total_units)
    }
}

pub(crate) fn total_tracked_progress_units(trackers: &[ProgressUnitTracker]) -> usize {
    trackers
        .iter()
        .map(ProgressUnitTracker::total_units)
        .fold(0usize, usize::saturating_add)
}

pub(crate) fn completed_tracked_progress_units(trackers: &[ProgressUnitTracker]) -> usize {
    trackers
        .iter()
        .map(ProgressUnitTracker::completed_units)
        .fold(0usize, usize::saturating_add)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;
    use std::time::Instant;

    use super::{
        ProgressUnitTracker, VolatileProgressGate, completed_tracked_progress_units,
        format_cpu_count, progress_percent, total_tracked_progress_units,
    };

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

    #[test]
    fn format_cpu_count_uses_singular_for_one_cpu() {
        assert_eq!(format_cpu_count(1), "1 CPU");
        assert_eq!(format_cpu_count(4), "4 CPUs");
    }

    #[test]
    fn progress_unit_tracker_records_monotonic_bounded_progress() {
        let mut trackers = ProgressUnitTracker::planned_for_totals(&[100, 200]);

        assert_eq!(trackers[0].record(25), 25);
        assert_eq!(trackers[0].record(20), 0);
        assert_eq!(trackers[1].record(250), 200);

        assert_eq!(completed_tracked_progress_units(&trackers), 225);
        assert_eq!(total_tracked_progress_units(&trackers), 300);
    }
}
