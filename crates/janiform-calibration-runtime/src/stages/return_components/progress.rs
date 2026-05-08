use std::sync::mpsc::Sender;

use super::contracts::{ReturnComponentsStageProgressPhase, ReturnComponentsStageProgressUpdate};

pub(crate) struct ReturnComponentsConvergenceProgress<'a> {
    pub(crate) phase: ReturnComponentsStageProgressPhase,
    pub(crate) completed_units: usize,
    pub(crate) total_units: usize,
    pub(crate) current_item_label: Option<&'a str>,
    pub(crate) current_evaluation_count: usize,
    pub(crate) current_weighted_objective: Option<f64>,
    pub(crate) best_weighted_objective: Option<f64>,
    pub(crate) current_convergence_ratio: Option<f64>,
}

pub(crate) fn send_return_components_progress(
    progress_sender: Option<&Sender<ReturnComponentsStageProgressUpdate>>,
    phase: ReturnComponentsStageProgressPhase,
    completed_units: usize,
    total_units: usize,
    current_item_label: Option<&str>,
) {
    send_return_components_progress_update(
        progress_sender,
        ReturnComponentsStageProgressUpdate {
            current_item_label: current_item_label.map(str::to_string),
            ..ReturnComponentsStageProgressUpdate::new(phase, completed_units, total_units)
        },
    );
}

pub(crate) fn send_return_components_evaluation_progress(
    progress_sender: Option<&Sender<ReturnComponentsStageProgressUpdate>>,
    phase: ReturnComponentsStageProgressPhase,
    completed_units: usize,
    total_units: usize,
    current_item_label: Option<&str>,
    current_evaluation_count: usize,
    total_evaluation_budget: usize,
) {
    let evaluation_progress_percent = current_evaluation_count
        .saturating_mul(100)
        .checked_div(total_evaluation_budget);

    send_return_components_progress_update(
        progress_sender,
        ReturnComponentsStageProgressUpdate {
            current_item_label: current_item_label.map(str::to_string),
            current_evaluation_count: Some(current_evaluation_count),
            total_evaluation_budget: Some(total_evaluation_budget),
            evaluation_progress_percent,
            ..ReturnComponentsStageProgressUpdate::new(phase, completed_units, total_units)
        },
    );
}

pub(crate) fn send_return_components_convergence_progress(
    progress_sender: Option<&Sender<ReturnComponentsStageProgressUpdate>>,
    progress: ReturnComponentsConvergenceProgress<'_>,
) {
    send_return_components_progress_update(
        progress_sender,
        ReturnComponentsStageProgressUpdate {
            current_item_label: progress.current_item_label.map(str::to_string),
            current_evaluation_count: Some(progress.current_evaluation_count),
            current_weighted_objective: progress.current_weighted_objective,
            best_weighted_objective: progress.best_weighted_objective,
            current_convergence_ratio: progress.current_convergence_ratio,
            ..ReturnComponentsStageProgressUpdate::new(
                progress.phase,
                progress.completed_units,
                progress.total_units,
            )
        },
    );
}

fn send_return_components_progress_update(
    progress_sender: Option<&Sender<ReturnComponentsStageProgressUpdate>>,
    progress_update: ReturnComponentsStageProgressUpdate,
) {
    let Some(progress_sender) = progress_sender else {
        return;
    };

    let _ = progress_sender.send(progress_update);
}
