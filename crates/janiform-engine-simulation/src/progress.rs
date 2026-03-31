use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SimulationProgressPhase {
    SeededTickIntervals,
    CoarseIntervals,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SimulationProgressEvent {
    pub phase: SimulationProgressPhase,
    pub completed_units: usize,
    pub total_units: usize,
}

impl SimulationProgressEvent {
    pub fn new(phase: SimulationProgressPhase, completed_units: usize, total_units: usize) -> Self {
        Self {
            phase,
            completed_units,
            total_units,
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
