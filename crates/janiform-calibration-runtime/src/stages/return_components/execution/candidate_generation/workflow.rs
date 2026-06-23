use std::sync::Arc;
use std::sync::mpsc::Sender;

use janiform_calibration_artifacts::CalibrationOptimizerSettings;
use janiform_model_kernels::market_pressure::MarketPressureParameters;
use janiform_model_kernels::return_clock::ReturnClockExposureParameters;

use super::enumeration::{CandidateInputsEnumerationInput, enumerate_candidate_inputs};
use super::families::{build_candidate_family_counts, build_candidate_family_sets};
use super::layers::{CandidateLayerFitInput, fit_candidate_layers};
use super::objective_policies::fit_objective_policies;
use super::provenance::{
    CandidateProvenanceTemplateBuildInput, build_candidate_provenance_template,
};
use crate::market_data::loading::MarketDataNormalizedCoarseDataRecord;
use crate::market_pressure::{
    MarketPressureFitInput, MarketPressureFitProgress, MarketPressureRuntimeState,
    fit_market_pressure,
};
use crate::stages::return_components::continuous_fit::ReturnComponentsStageContinuousReturnCalibrationSample;
use crate::stages::return_components::contracts::{
    ReturnComponentsDetectedCloseOpenGapCandidate, ReturnComponentsStageCandidateFamilyCounts,
    ReturnComponentsStageCandidateGenerationInput,
    ReturnComponentsStageCandidateGenerationInputError, ReturnComponentsStageCandidateInput,
    ReturnComponentsStageObservedSignalSummaryStatistics, ReturnComponentsStageProgressPhase,
    ReturnComponentsStageProgressUpdate, ReturnComponentsStageQuantileBinningPolicy,
};
use crate::stages::return_components::execution::signals::{
    ReturnComponentsStageActivityCalibrationContext, ReturnComponentsStageSortedSignalInputs,
    sort_candidate_generation_signals, summarize_candidate_generation_signals,
};
use crate::stages::return_components::progress::emit_return_components_labeled_progress;
use crate::stages::return_components::quantile_binning_fit;
use crate::stages::return_components::return_clock_exposure_fit::fit_return_clock_exposure_parameters;
use crate::stages::return_components::signal_partition_fit;
use crate::stages::return_components::surprise_episode_selection_fit;
use crate::stages::return_components::surprise_intensity_fit;

pub(in crate::stages::return_components::execution) struct ReturnComponentsStageCandidateGenerationOutput
{
    pub(in crate::stages::return_components::execution) candidate_inputs:
        Vec<ReturnComponentsStageCandidateInput>,
    pub(in crate::stages::return_components::execution) market_pressure_runtime_state:
        Arc<MarketPressureRuntimeState>,
    pub(in crate::stages::return_components::execution) candidate_family_counts:
        ReturnComponentsStageCandidateFamilyCounts,
    pub(in crate::stages::return_components::execution) fitted_surprise_arrival_layer:
        surprise_intensity_fit::FittedSurpriseNewsArrivalLayer,
}

pub(in crate::stages::return_components::execution) struct ReturnComponentsStageCandidateGenerationRequest<
    'a,
> {
    pub(in crate::stages::return_components::execution) candidate_generation_input:
        &'a ReturnComponentsStageCandidateGenerationInput,
    pub(in crate::stages::return_components::execution) derived_close_open_gap_candidates:
        &'a [ReturnComponentsDetectedCloseOpenGapCandidate],
    pub(in crate::stages::return_components::execution) close_open_gap_detection_filter: &'a str,
    pub(in crate::stages::return_components::execution) source_window_open_exposure_hours: f64,
    pub(in crate::stages::return_components::execution) source_window_closed_exposure_hours: f64,
    pub(in crate::stages::return_components::execution) activity_calibration_context:
        Option<ReturnComponentsStageActivityCalibrationContext>,
    pub(in crate::stages::return_components::execution) coarse_data_records:
        Option<&'a [MarketDataNormalizedCoarseDataRecord]>,
    pub(in crate::stages::return_components::execution) stage_seed: u64,
    pub(in crate::stages::return_components::execution) stage_solver_settings:
        &'a CalibrationOptimizerSettings,
    pub(in crate::stages::return_components::execution) progress_sender:
        Option<&'a Sender<ReturnComponentsStageProgressUpdate>>,
}

pub(in crate::stages::return_components::execution) fn generate_return_components_stage_candidate_inputs(
    request: ReturnComponentsStageCandidateGenerationRequest<'_>,
) -> Result<
    ReturnComponentsStageCandidateGenerationOutput,
    ReturnComponentsStageCandidateGenerationInputError,
> {
    let candidate_generation_input = request.candidate_generation_input;
    let progress_sender = request.progress_sender;

    let signal_layers = fit_signal_selection_layers(candidate_generation_input, progress_sender);

    validate_candidate_generation_input(candidate_generation_input, progress_sender)?;

    let sorted_signal_fit =
        sort_signals_and_fit_market_inputs(&request, &signal_layers, progress_sender)?;

    let signal_summary_fit = summarize_sorted_signals(
        candidate_generation_input,
        &sorted_signal_fit.sorted_signals,
        request.activity_calibration_context,
        progress_sender,
    );

    let fitted_quantile_binning_policy =
        fit_quantile_binning_policy(&sorted_signal_fit.sorted_signals, progress_sender);

    let fitted_layers = fit_candidate_layers(CandidateLayerFitInput {
        candidate_generation_input,
        sorted_signals: &sorted_signal_fit.sorted_signals,
        summary_statistics: signal_summary_fit.summary_statistics,
        continuous_return_calibration_sample: &signal_summary_fit
            .continuous_return_calibration_sample,
        source_window_open_exposure_hours: request.source_window_open_exposure_hours,
        source_window_closed_exposure_hours: request.source_window_closed_exposure_hours,
        stage_seed: request.stage_seed,
        stage_solver_settings: request.stage_solver_settings,
        progress_sender,
    })?;

    let fitted_objective_policies =
        fit_objective_policies(signal_summary_fit.summary_statistics, progress_sender);

    let candidate_family_sets = build_candidate_family_sets(
        candidate_generation_input,
        fitted_layers.fitted_continuous_return_layer,
        &fitted_layers.fitted_surprise_arrival_layer,
        &fitted_layers.fitted_return_jump_mark_layer,
        &fitted_layers.fitted_activity_coupling_layer,
        &sorted_signal_fit.fitted_market_pressure_parameters,
    );

    let candidate_family_counts = build_candidate_family_counts(&candidate_family_sets);

    let candidate_provenance_template =
        build_candidate_provenance_template(CandidateProvenanceTemplateBuildInput {
            source_window: sorted_signal_fit.sorted_signals.source_window,
            stage_seed: request.stage_seed,
            summary_statistics: signal_summary_fit.summary_statistics,
            close_open_gap_detection_filter: request.close_open_gap_detection_filter,
            fitted_signal_partition_layer: signal_layers.fitted_signal_partition_layer,
            fitted_surprise_episode_selection_policy: signal_layers
                .fitted_surprise_episode_selection_layer
                .policy
                .clone(),
            fitted_quantile_binning_policy,
            fitted_diagnostic_policy: fitted_objective_policies.diagnostic_policy.clone(),
            fitted_objective_scoring_policy: fitted_objective_policies
                .objective_scoring_policy
                .clone(),
            fitted_price_range_sanity_policy: fitted_objective_policies
                .price_range_sanity_policy
                .clone(),
        });

    let candidate_inputs = enumerate_candidate_inputs(CandidateInputsEnumerationInput {
        candidate_generation_input,
        candidate_family_sets: &candidate_family_sets,
        stage_solver_settings: request.stage_solver_settings,
        return_clock_exposure_parameters: sorted_signal_fit.fitted_return_clock_exposure_parameters,
        summary_statistics: signal_summary_fit.summary_statistics,
        fitted_surprise_deterministic_intensity_profile: &fitted_layers
            .fitted_surprise_arrival_layer
            .deterministic_intensity_profile,
        candidate_provenance_template: &candidate_provenance_template,
        fitted_diagnostic_policy: &fitted_objective_policies.diagnostic_policy,
        fitted_objective_scoring_policy: &fitted_objective_policies.objective_scoring_policy,
        fitted_price_range_sanity_policy: &fitted_objective_policies.price_range_sanity_policy,
        progress_sender,
    })?;

    Ok(ReturnComponentsStageCandidateGenerationOutput {
        candidate_inputs,
        market_pressure_runtime_state: sorted_signal_fit.market_pressure_runtime_state,
        candidate_family_counts,
        fitted_surprise_arrival_layer: fitted_layers.fitted_surprise_arrival_layer,
    })
}

struct SignalSelectionLayerFit {
    fitted_signal_partition_layer: signal_partition_fit::FittedSignalPartitionLayer,
    fitted_surprise_episode_selection_layer:
        surprise_episode_selection_fit::FittedSurpriseEpisodeSelectionLayer,
}

fn fit_signal_selection_layers(
    candidate_generation_input: &ReturnComponentsStageCandidateGenerationInput,
    progress_sender: Option<&Sender<ReturnComponentsStageProgressUpdate>>,
) -> SignalSelectionLayerFit {
    emit_return_components_labeled_progress(
        progress_sender,
        ReturnComponentsStageProgressPhase::SignalPartitionFit,
        0,
        1,
        "fit signal partition",
    );

    let fitted_signal_partition_layer = signal_partition_fit::fit_signal_partition_layer(
        candidate_generation_input
            .observed_open_session_returns
            .as_slice(),
        candidate_generation_input
            .realized_known_news_return_samples
            .as_slice(),
        candidate_generation_input
            .detected_intraday_jump_candidates
            .as_slice(),
        &candidate_generation_input.price_discretization_surface,
    );

    emit_return_components_labeled_progress(
        progress_sender,
        ReturnComponentsStageProgressPhase::SurpriseEpisodeSelectionFit,
        0,
        1,
        "fit surprise episode selection",
    );

    let fitted_surprise_episode_selection_layer =
        surprise_episode_selection_fit::fit_surprise_episode_selection_layer(
            candidate_generation_input
                .detected_intraday_jump_candidates
                .as_slice(),
            fitted_signal_partition_layer.continuous_return_filter_threshold,
        );

    SignalSelectionLayerFit {
        fitted_signal_partition_layer,
        fitted_surprise_episode_selection_layer,
    }
}

fn validate_candidate_generation_input(
    candidate_generation_input: &ReturnComponentsStageCandidateGenerationInput,
    progress_sender: Option<&Sender<ReturnComponentsStageProgressUpdate>>,
) -> Result<(), ReturnComponentsStageCandidateGenerationInputError> {
    emit_return_components_labeled_progress(
        progress_sender,
        ReturnComponentsStageProgressPhase::CandidateInputValidation,
        0,
        1,
        "validate candidate generation input",
    );

    candidate_generation_input.validate()?;

    Ok(())
}

struct SortedSignalFit {
    sorted_signals: ReturnComponentsStageSortedSignalInputs,
    fitted_return_clock_exposure_parameters: ReturnClockExposureParameters,
    fitted_market_pressure_parameters: MarketPressureParameters,
    market_pressure_runtime_state: Arc<MarketPressureRuntimeState>,
}

fn sort_signals_and_fit_market_inputs(
    request: &ReturnComponentsStageCandidateGenerationRequest<'_>,
    signal_layers: &SignalSelectionLayerFit,
    progress_sender: Option<&Sender<ReturnComponentsStageProgressUpdate>>,
) -> Result<SortedSignalFit, ReturnComponentsStageCandidateGenerationInputError> {
    emit_return_components_labeled_progress(
        progress_sender,
        ReturnComponentsStageProgressPhase::SignalSorting,
        0,
        1,
        "sort observed signals",
    );

    let sorted_signals = sort_candidate_generation_signals(
        request.candidate_generation_input,
        request.derived_close_open_gap_candidates,
        signal_layers.fitted_signal_partition_layer,
        &signal_layers.fitted_surprise_episode_selection_layer,
    );

    let fitted_return_clock_exposure_parameters =
        fit_return_clock_exposure_parameters(sorted_signals.continuous_fit_returns.as_slice());

    let market_pressure_progress_sender = progress_sender.cloned();
    let mut market_pressure_progress_observer = move |progress: MarketPressureFitProgress| {
        emit_return_components_labeled_progress(
            market_pressure_progress_sender.as_ref(),
            ReturnComponentsStageProgressPhase::MarketPressureFit,
            progress.completed_units,
            progress.total_units,
            progress.current_item_label,
        );
    };

    let fitted_market_pressure_output = fit_market_pressure(MarketPressureFitInput {
        observed_open_session_returns: sorted_signals.observed_open_session_returns.as_slice(),
        coarse_data_records: request.coarse_data_records,
        coarse_return_law: None,
        source_window_open_exposure_hours: request.source_window_open_exposure_hours,
        solver_settings: request.stage_solver_settings,
        progress_observer: Some(&mut market_pressure_progress_observer),
    })
    .map_err(|source| {
        ReturnComponentsStageCandidateGenerationInputError::MarketPressureFitFailed {
            error_message: source.to_string(),
        }
    })?;

    Ok(SortedSignalFit {
        sorted_signals,
        fitted_return_clock_exposure_parameters,
        fitted_market_pressure_parameters: fitted_market_pressure_output.parameters,
        market_pressure_runtime_state: fitted_market_pressure_output.runtime_state,
    })
}

struct SignalSummaryFit {
    summary_statistics: ReturnComponentsStageObservedSignalSummaryStatistics,
    continuous_return_calibration_sample: ReturnComponentsStageContinuousReturnCalibrationSample,
}

fn summarize_sorted_signals(
    candidate_generation_input: &ReturnComponentsStageCandidateGenerationInput,
    sorted_signals: &ReturnComponentsStageSortedSignalInputs,
    activity_calibration_context: Option<ReturnComponentsStageActivityCalibrationContext>,
    progress_sender: Option<&Sender<ReturnComponentsStageProgressUpdate>>,
) -> SignalSummaryFit {
    emit_return_components_labeled_progress(
        progress_sender,
        ReturnComponentsStageProgressPhase::SignalSummary,
        0,
        1,
        "summarize observed signals",
    );

    let (summary_statistics, continuous_return_calibration_sample) =
        summarize_candidate_generation_signals(
            candidate_generation_input,
            sorted_signals,
            activity_calibration_context,
        );

    SignalSummaryFit {
        summary_statistics,
        continuous_return_calibration_sample,
    }
}

fn fit_quantile_binning_policy(
    sorted_signals: &ReturnComponentsStageSortedSignalInputs,
    progress_sender: Option<&Sender<ReturnComponentsStageProgressUpdate>>,
) -> ReturnComponentsStageQuantileBinningPolicy {
    emit_return_components_labeled_progress(
        progress_sender,
        ReturnComponentsStageProgressPhase::QuantileBinningPolicyFit,
        0,
        1,
        "fit quantile binning policy",
    );

    quantile_binning_fit::fit_quantile_binning_policy(
        sorted_signals.surprise_open_jump_magnitudes.as_slice(),
        sorted_signals.combined_gap_magnitudes.as_slice(),
    )
}
