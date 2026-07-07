use serde::{Deserialize, Serialize};

/// Simulation work phase for native progress events.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SimulationProgressPhase {
    /// Seeded tick generator interval loop.
    SeededTickIntervals,
    /// Coarse interval generator loop.
    CoarseIntervals,
}

/// Native simulation progress event.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SimulationProgressEvent {
    /// Current simulation phase.
    pub phase: SimulationProgressPhase,
    /// Completed phase work units.
    pub completed_units: usize,
    /// Total phase work units.
    pub total_units: usize,
    /// Fixed duration represented by one interval work unit.
    pub interval_duration_nanos: i64,
    /// Generated tick count for seeded tick simulations.
    pub generated_tick_count: Option<u64>,
}

impl SimulationProgressEvent {
    /// Create a native simulation progress event.
    pub fn new(
        phase: SimulationProgressPhase,
        completed_units: usize,
        total_units: usize,
        interval_duration_nanos: i64,
        generated_tick_count: Option<u64>,
    ) -> Self {
        Self {
            phase,
            completed_units,
            total_units,
            interval_duration_nanos,
            generated_tick_count,
        }
    }
}

pub(crate) fn emit_progress_event(
    observer: &mut Option<&mut dyn FnMut(SimulationProgressEvent)>,
    event: SimulationProgressEvent,
) {
    if let Some(observer) = observer.as_deref_mut() {
        observer(event);
    }
}
