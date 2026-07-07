use std::sync::mpsc::Sender;

use janiform_calibration_artifacts::CalibrationOptimizerSettings;

use crate::stages::return_components::activity_coupling_fit;
use crate::stages::return_components::continuous_fit::ReturnComponentsStageContinuousReturnCalibrationSample;
use crate::stages::return_components::continuous_layer_fit;
use crate::stages::return_components::contracts::{
    ReturnComponentsStageCandidateGenerationInput,
    ReturnComponentsStageCandidateGenerationInputError,
    ReturnComponentsStageObservedSignalSummaryStatistics, ReturnComponentsStageProgressPhase,
    ReturnComponentsStageProgressUpdate,
};
use crate::stages::return_components::execution::signals::ReturnComponentsStageSortedSignalInputs;
use crate::stages::return_components::mark_layer_fit;
use crate::stages::return_components::progress::emit_return_components_labeled_progress;
use crate::stages::return_components::surprise_intensity_fit;

pub(super) struct CandidateLayerFit {
    pub(super) fitted_surprise_arrival_layer:
        surprise_intensity_fit::FittedSurpriseNewsArrivalLayer,
    pub(super) fitted_return_jump_mark_layer: mark_layer_fit::FittedReturnJumpMarkLayer,
    pub(super) fitted_activity_coupling_layer: activity_coupling_fit::FittedActivityCouplingLayer,
    pub(super) fitted_continuous_return_layer: continuous_layer_fit::FittedContinuousReturnLayer,
}

pub(super) struct CandidateLayerFitInput<'a> {
    pub(super) candidate_generation_input: &'a ReturnComponentsStageCandidateGenerationInput,
    pub(super) sorted_signals: &'a ReturnComponentsStageSortedSignalInputs,
    pub(super) summary_statistics: ReturnComponentsStageObservedSignalSummaryStatistics,
    pub(super) continuous_return_calibration_sample:
        &'a ReturnComponentsStageContinuousReturnCalibrationSample,
    pub(super) source_window_open_exposure_hours: f64,
    pub(super) source_window_closed_exposure_hours: f64,
    pub(super) stage_seed: u64,
    pub(super) stage_solver_settings: &'a CalibrationOptimizerSettings,
    pub(super) progress_sender: Option<&'a Sender<ReturnComponentsStageProgressUpdate>>,
}

pub(super) fn fit_candidate_layers(
    input: CandidateLayerFitInput<'_>,
) -> Result<CandidateLayerFit, ReturnComponentsStageCandidateGenerationInputError> {
    let fitted_surprise_arrival_layer = fit_surprise_arrival_layer(
        input.sorted_signals,
        input.source_window_open_exposure_hours,
        input.source_window_closed_exposure_hours,
        input.progress_sender,
    );

    let fitted_return_jump_mark_layer = fit_return_jump_mark_layer(
        input.candidate_generation_input,
        input.summary_statistics,
        input.progress_sender,
    );

    let fitted_activity_coupling_layer =
        fit_activity_coupling_layer(input.summary_statistics, input.progress_sender);

    let fitted_continuous_return_layer = fit_continuous_return_layer(
        input.summary_statistics,
        input.continuous_return_calibration_sample,
        input.stage_seed,
        input.stage_solver_settings,
        input.progress_sender,
    )?;

    Ok(CandidateLayerFit {
        fitted_surprise_arrival_layer,
        fitted_return_jump_mark_layer,
        fitted_activity_coupling_layer,
        fitted_continuous_return_layer,
    })
}

fn fit_surprise_arrival_layer(
    sorted_signals: &ReturnComponentsStageSortedSignalInputs,
    source_window_open_exposure_hours: f64,
    source_window_closed_exposure_hours: f64,
    progress_sender: Option<&Sender<ReturnComponentsStageProgressUpdate>>,
) -> surprise_intensity_fit::FittedSurpriseNewsArrivalLayer {
    emit_return_components_labeled_progress(
        progress_sender,
        ReturnComponentsStageProgressPhase::SurpriseArrivalFit,
        0,
        1,
        "fit surprise arrival layer",
    );

    let selected_episode_timestamps = sorted_signals
        .selected_surprise_jump_episodes
        .iter()
        .map(|episode| episode.terminal_timestamp_utc_nanos)
        .collect::<Vec<_>>();

    surprise_intensity_fit::fit_surprise_news_arrival_layer(
        sorted_signals.source_window.window_start_utc_nanos,
        sorted_signals.source_window.window_end_utc_nanos,
        &selected_episode_timestamps,
        sorted_signals.close_open_gap_candidates.len(),
        source_window_open_exposure_hours,
        source_window_closed_exposure_hours,
    )
}

fn fit_return_jump_mark_layer(
    candidate_generation_input: &ReturnComponentsStageCandidateGenerationInput,
    summary_statistics: ReturnComponentsStageObservedSignalSummaryStatistics,
    progress_sender: Option<&Sender<ReturnComponentsStageProgressUpdate>>,
) -> mark_layer_fit::FittedReturnJumpMarkLayer {
    emit_return_components_labeled_progress(
        progress_sender,
        ReturnComponentsStageProgressPhase::ReturnJumpMarkFit,
        0,
        1,
        "fit return jump mark layer",
    );

    mark_layer_fit::fit_return_jump_mark_layer(
        candidate_generation_input.shared_mark_law_template,
        summary_statistics,
    )
}

fn fit_activity_coupling_layer(
    summary_statistics: ReturnComponentsStageObservedSignalSummaryStatistics,
    progress_sender: Option<&Sender<ReturnComponentsStageProgressUpdate>>,
) -> activity_coupling_fit::FittedActivityCouplingLayer {
    emit_return_components_labeled_progress(
        progress_sender,
        ReturnComponentsStageProgressPhase::ActivityCouplingFit,
        0,
        1,
        "fit activity coupling layer",
    );

    activity_coupling_fit::fit_activity_coupling_layer(summary_statistics)
}

fn fit_continuous_return_layer(
    summary_statistics: ReturnComponentsStageObservedSignalSummaryStatistics,
    continuous_return_calibration_sample: &ReturnComponentsStageContinuousReturnCalibrationSample,
    stage_seed: u64,
    stage_solver_settings: &CalibrationOptimizerSettings,
    progress_sender: Option<&Sender<ReturnComponentsStageProgressUpdate>>,
) -> Result<
    continuous_layer_fit::FittedContinuousReturnLayer,
    ReturnComponentsStageCandidateGenerationInputError,
> {
    emit_return_components_labeled_progress(
        progress_sender,
        ReturnComponentsStageProgressPhase::ContinuousReturnLayerFit,
        0,
        1,
        "fit continuous return layer",
    );

    let fitted_continuous_return_layer = continuous_layer_fit::fit_continuous_return_layer(
        summary_statistics,
        continuous_return_calibration_sample,
        stage_seed,
        stage_solver_settings,
        progress_sender,
    )?;

    Ok(fitted_continuous_return_layer)
}
