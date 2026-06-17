use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationModelsStageProgressPhase {
    InputPreflight,
    NoMoveEventModelFit,
    TimestampBatchingModelFit,
    ReportAssembly,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ObservationModelsStageProgressUpdate {
    pub phase: ObservationModelsStageProgressPhase,
    pub completed_units: usize,
    pub total_units: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aggregate_progress_percent: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_item_label: Option<String>,
}

impl ObservationModelsStageProgressUpdate {
    pub const fn stage_progress_percent(&self) -> usize {
        if self.total_units == 0 {
            return 100;
        }

        self.completed_units.saturating_mul(100) / self.total_units
    }
}

pub(super) fn emit_observation_models_progress(
    progress_observer: &mut Option<&mut dyn FnMut(ObservationModelsStageProgressUpdate)>,
    phase: ObservationModelsStageProgressPhase,
    completed_units: usize,
    total_units: usize,
    current_item_label: Option<&str>,
) {
    let Some(progress_observer) = progress_observer.as_deref_mut() else {
        return;
    };

    progress_observer(ObservationModelsStageProgressUpdate {
        phase,
        completed_units,
        total_units,
        aggregate_progress_percent: None,
        current_item_label: current_item_label.map(str::to_string),
    });
}
