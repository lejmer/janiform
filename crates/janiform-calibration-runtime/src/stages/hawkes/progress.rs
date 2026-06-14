use std::sync::mpsc::Sender;

use super::contracts::{HawkesStageProgressPhase, HawkesStageProgressUpdate};

pub(crate) struct HawkesConvergenceProgress<'a> {
    pub(crate) phase: HawkesStageProgressPhase,
    pub(crate) completed_units: usize,
    pub(crate) total_units: usize,
    pub(crate) current_item_label: Option<&'a str>,
    pub(crate) current_evaluation_count: usize,
    pub(crate) total_evaluation_budget: usize,
    pub(crate) current_weighted_objective: Option<f64>,
    pub(crate) best_weighted_objective: Option<f64>,
    pub(crate) current_convergence_ratio: Option<f64>,
}

pub(crate) fn emit_hawkes_progress(
    progress_sender: Option<&Sender<HawkesStageProgressUpdate>>,
    phase: HawkesStageProgressPhase,
    completed_units: usize,
    total_units: usize,
    current_item_label: Option<&str>,
) {
    emit_hawkes_progress_update(
        progress_sender,
        HawkesStageProgressUpdate {
            current_item_label: current_item_label.map(str::to_string),
            ..HawkesStageProgressUpdate::new(phase, completed_units, total_units)
        },
    );
}

pub(crate) fn emit_hawkes_convergence_progress(
    progress_sender: Option<&Sender<HawkesStageProgressUpdate>>,
    progress: HawkesConvergenceProgress<'_>,
) {
    let evaluation_progress_percent = progress
        .current_evaluation_count
        .saturating_mul(100)
        .checked_div(progress.total_evaluation_budget.max(1));

    emit_hawkes_progress_update(
        progress_sender,
        HawkesStageProgressUpdate {
            current_item_label: progress.current_item_label.map(str::to_string),
            current_evaluation_count: Some(progress.current_evaluation_count),
            total_evaluation_budget: Some(progress.total_evaluation_budget),
            evaluation_progress_percent,
            current_weighted_objective: progress.current_weighted_objective,
            best_weighted_objective: progress.best_weighted_objective,
            current_convergence_ratio: progress.current_convergence_ratio,
            ..HawkesStageProgressUpdate::new(
                progress.phase,
                progress.completed_units,
                progress.total_units,
            )
        },
    );
}

fn emit_hawkes_progress_update(
    progress_sender: Option<&Sender<HawkesStageProgressUpdate>>,
    progress_update: HawkesStageProgressUpdate,
) {
    let Some(progress_sender) = progress_sender else {
        return;
    };

    let _ = progress_sender.send(progress_update);
}
