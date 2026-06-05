use std::sync::mpsc::Sender;

#[derive(Debug, Clone, PartialEq)]
pub struct StageProgressUpdate<Phase> {
    pub phase: Phase,
    pub completed_units: usize,
    pub total_units: usize,
    pub aggregate_progress_percent: Option<usize>,
    pub current_item_label: Option<String>,
    pub current_evaluation_count: Option<usize>,
    pub total_evaluation_budget: Option<usize>,
    pub evaluation_progress_percent: Option<usize>,
}

impl<Phase> StageProgressUpdate<Phase> {
    pub(crate) fn new(phase: Phase, completed_units: usize, total_units: usize) -> Self {
        Self {
            phase,
            completed_units,
            total_units,
            aggregate_progress_percent: None,
            current_item_label: None,
            current_evaluation_count: None,
            total_evaluation_budget: None,
            evaluation_progress_percent: None,
        }
    }

    pub(crate) fn stage_progress_percent(&self) -> usize {
        self.aggregate_progress_percent
            .unwrap_or_else(|| progress_percent(self.completed_units, self.total_units))
    }
}

pub(crate) fn progress_percent(completed_units: usize, total_units: usize) -> usize {
    if total_units == 0 {
        return 0;
    }

    completed_units
        .min(total_units)
        .saturating_mul(100)
        .checked_div(total_units)
        .unwrap_or(0)
        .min(100)
}

pub(crate) fn emit_stage_progress<Phase>(
    progress_sender: Option<&Sender<StageProgressUpdate<Phase>>>,
    progress_update: StageProgressUpdate<Phase>,
) {
    let Some(progress_sender) = progress_sender else {
        return;
    };

    let _ = progress_sender.send(progress_update);
}
