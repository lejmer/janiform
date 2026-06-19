use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::Sender;

use crate::stages::mean_reversion::{
    MeanReversionStageProgressPhase, MeanReversionStageProgressUpdate,
};
use crate::stages::progress::{StageProgressUpdate, emit_stage_progress, progress_percent};

pub(super) fn emit_predictive_evidence_progress(
    progress_sender: Option<&Sender<MeanReversionStageProgressUpdate>>,
    completed_units: usize,
    total_units: usize,
    progress_label: Option<&str>,
    current_evaluation_count: Option<usize>,
    total_evaluation_budget: Option<usize>,
) {
    let mut progress_update = StageProgressUpdate::new(
        MeanReversionStageProgressPhase::PredictiveEvidence,
        completed_units,
        total_units,
    );

    progress_update.current_item_label = progress_label.map(str::to_string);
    progress_update.current_evaluation_count = current_evaluation_count;
    progress_update.total_evaluation_budget = total_evaluation_budget;

    progress_update.evaluation_progress_percent =
        match (current_evaluation_count, total_evaluation_budget) {
            (Some(completed), Some(total)) => Some(progress_percent(completed, total)),
            _ => None,
        };

    emit_stage_progress(progress_sender, progress_update);
}

pub(super) fn complete_predictive_evidence_progress_unit(
    progress_sender: Option<&Sender<MeanReversionStageProgressUpdate>>,
    progress_counter: &AtomicUsize,
    total_progress_units: usize,
    progress_label: &str,
    current_evaluation_count: usize,
    total_evaluation_budget: usize,
) {
    let completed_units = progress_counter
        .fetch_add(1, Ordering::Relaxed)
        .saturating_add(1)
        .min(total_progress_units);

    emit_predictive_evidence_progress(
        progress_sender,
        completed_units,
        total_progress_units,
        Some(progress_label),
        Some(current_evaluation_count),
        Some(total_evaluation_budget),
    );
}
