use std::sync::mpsc::Sender;

use janiform_market_time::business_time_clock::{
    FourierBusinessTimeClockFitProgress, FourierBusinessTimeClockFitProgressPhase,
};

use crate::stages::progress::{StageProgressUpdate, progress_percent};

use super::{SeasonalityStageProgressPhase, SeasonalityStageProgressUpdate};

pub(super) fn forward_clock_fit_progress(
    progress_sender: Option<&Sender<SeasonalityStageProgressUpdate>>,
    progress: FourierBusinessTimeClockFitProgress,
) {
    let phase = match progress.phase {
        FourierBusinessTimeClockFitProgressPhase::ExposureAssembly => {
            SeasonalityStageProgressPhase::ClockFitPreparation
        }
        FourierBusinessTimeClockFitProgressPhase::CandidateEvaluation => {
            SeasonalityStageProgressPhase::ClockCandidateEvaluation
        }
        FourierBusinessTimeClockFitProgressPhase::ModelFinalization => {
            SeasonalityStageProgressPhase::ClockFitPreparation
        }
    };

    let evaluation_progress_percent = match progress.phase {
        FourierBusinessTimeClockFitProgressPhase::CandidateEvaluation => Some(progress_percent(
            progress.completed_units,
            progress.total_units,
        )),
        _ => None,
    };

    emit_seasonality_progress_update(
        progress_sender,
        SeasonalityStageProgressUpdate {
            phase,
            completed_units: progress.completed_units,
            total_units: progress.total_units,
            aggregate_progress_percent: None,
            current_item_label: progress.current_item_label,
            current_evaluation_count: matches!(
                progress.phase,
                FourierBusinessTimeClockFitProgressPhase::CandidateEvaluation
            )
            .then_some(progress.completed_units),
            total_evaluation_budget: matches!(
                progress.phase,
                FourierBusinessTimeClockFitProgressPhase::CandidateEvaluation
            )
            .then_some(progress.total_units),
            evaluation_progress_percent,
            current_objective_value: progress.current_objective_value,
            best_objective_value: progress.best_objective_value,
        },
    );
}

pub(super) fn emit_seasonality_progress(
    progress_sender: Option<&Sender<SeasonalityStageProgressUpdate>>,
    phase: SeasonalityStageProgressPhase,
    completed_units: usize,
    total_units: usize,
    current_item_label: Option<&str>,
) {
    emit_seasonality_progress_update(
        progress_sender,
        SeasonalityStageProgressUpdate {
            current_item_label: current_item_label.map(str::to_string),
            ..SeasonalityStageProgressUpdate::new(phase, completed_units, total_units)
        },
    );
}

pub(super) fn emit_seasonality_progress_update(
    progress_sender: Option<&Sender<SeasonalityStageProgressUpdate>>,
    mut progress_update: SeasonalityStageProgressUpdate,
) {
    progress_update.aggregate_progress_percent = Some(
        StageProgressUpdate {
            phase: progress_update.phase,
            completed_units: progress_update.completed_units,
            total_units: progress_update.total_units,
            aggregate_progress_percent: progress_update.aggregate_progress_percent,
            current_item_label: progress_update.current_item_label.clone(),
            current_evaluation_count: progress_update.current_evaluation_count,
            total_evaluation_budget: progress_update.total_evaluation_budget,
            evaluation_progress_percent: progress_update.evaluation_progress_percent,
        }
        .stage_progress_percent(),
    );

    if let Some(sender) = progress_sender {
        let _ = sender.send(progress_update);
    }
}
