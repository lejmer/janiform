use std::sync::mpsc::Sender;

use janiform_calibration_artifacts::{
    CalibrationOptimizerSettings, SurpriseNewsDeterministicIntensityProfile,
};
use janiform_model_kernels::news::{
    CoupledGapMarkConfiguration, KnownNewsArrivalConfiguration, NewsArrivalConfiguration,
    NewsComponentMarkScaleConfiguration, NewsProcessConfiguration,
    SurpriseNewsArrivalConfiguration,
};

use super::super::activity_coupling_fit;
use super::super::constants::{
    RETURN_COMPONENTS_STAGE_DETERMINISTIC_SEED_POLICY,
    RETURN_COMPONENTS_STAGE_SIGNAL_PREPROCESSING_POLICY,
};
use super::super::continuous_layer_fit;
use super::super::contracts::{
    ReturnComponentsActivityCouplingCandidateFamily,
    ReturnComponentsKnownNewsArrivalCandidateFamily,
    ReturnComponentsReturnJumpMarkScaleCandidateFamily, ReturnComponentsStageCandidateFamilies,
    ReturnComponentsStageCandidateFamilyCounts, ReturnComponentsStageCandidateGenerationInput,
    ReturnComponentsStageCandidateGenerationInputError, ReturnComponentsStageCandidateInput,
    ReturnComponentsStageCandidateProvenance, ReturnComponentsStageCandidateSourceWindow,
    ReturnComponentsStageDeterministicSeedMetadata, ReturnComponentsStageDiagnosticPolicy,
    ReturnComponentsStageObjectiveScoringPolicy,
    ReturnComponentsStageObservedSignalSummaryStatistics,
    ReturnComponentsStagePriceRangeSanityPolicy, ReturnComponentsStageProgressPhase,
    ReturnComponentsStageProgressUpdate, ReturnComponentsStageQuantileBinningPolicy,
    ReturnComponentsStageSurpriseEpisodeSelectionPolicy,
    ReturnComponentsSurpriseNewsArrivalCandidateFamily,
};
use super::super::diagnostic_policy_fit;
use super::super::known_arrival_fit;
use super::super::mark_layer_fit;
use super::super::objective_scoring_policy_fit;
use super::super::price_range_policy_fit;
use super::super::quantile_binning_fit;
use super::super::signal_partition_fit;
use super::super::surprise_episode_selection_fit;
use super::super::surprise_intensity_fit;
use super::diagnostics::{
    build_activity_coupling_diagnostic, build_gap_diagnostic, build_jump_diagnostic,
    build_price_range_sanity_diagnostic, build_surprise_arrival_diagnostic, round_candidate_value,
};
use super::signals::{
    ReturnComponentsStageActivityCalibrationContext, sort_candidate_generation_signals,
    summarize_candidate_generation_signals,
};
use crate::stages::return_components::progress::send_return_components_progress;

pub(super) struct ReturnComponentsStageCandidateGenerationOutput {
    pub(super) candidate_inputs: Vec<ReturnComponentsStageCandidateInput>,
    pub(super) candidate_family_counts: ReturnComponentsStageCandidateFamilyCounts,
    pub(super) fitted_surprise_arrival_layer:
        surprise_intensity_fit::FittedSurpriseNewsArrivalLayer,
}

#[derive(Debug, Clone)]
struct ReturnComponentsStageCandidateFamilySets {
    fitted_continuous_return_layer: continuous_layer_fit::FittedContinuousReturnLayer,
    fitted_known_arrival_layer: known_arrival_fit::FittedKnownNewsArrivalLayer,
    fitted_surprise_arrival_layer: ReturnComponentsSurpriseNewsArrivalCandidateFamily,
    fitted_return_jump_mark_layer: ReturnComponentsReturnJumpMarkScaleCandidateFamily,
    fitted_activity_coupling_layer: ReturnComponentsActivityCouplingCandidateFamily,
}

pub(super) fn generate_return_components_stage_candidate_inputs(
    candidate_generation_input: &ReturnComponentsStageCandidateGenerationInput,
    derived_close_open_gap_candidates: &[super::super::contracts::ReturnComponentsDetectedCloseOpenGapCandidate],
    close_open_gap_detection_filter: &str,
    activity_calibration_context: Option<ReturnComponentsStageActivityCalibrationContext>,
    stage_seed: u64,
    stage_solver_settings: &CalibrationOptimizerSettings,
    progress_sender: Option<&Sender<ReturnComponentsStageProgressUpdate>>,
) -> Result<
    ReturnComponentsStageCandidateGenerationOutput,
    ReturnComponentsStageCandidateGenerationInputError,
> {
    send_return_components_progress(
        progress_sender,
        ReturnComponentsStageProgressPhase::SignalPartitionFit,
        0,
        1,
        Some("fit signal partition"),
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
    );

    send_return_components_progress(
        progress_sender,
        ReturnComponentsStageProgressPhase::SignalPartitionFit,
        1,
        1,
        Some("signal partition fitted"),
    );

    send_return_components_progress(
        progress_sender,
        ReturnComponentsStageProgressPhase::SurpriseEpisodeSelectionFit,
        0,
        1,
        Some("fit surprise episode selection"),
    );

    let fitted_surprise_episode_selection_layer =
        surprise_episode_selection_fit::fit_surprise_episode_selection_layer(
            candidate_generation_input
                .detected_intraday_jump_candidates
                .as_slice(),
            fitted_signal_partition_layer.continuous_return_filter_threshold,
        );

    send_return_components_progress(
        progress_sender,
        ReturnComponentsStageProgressPhase::SurpriseEpisodeSelectionFit,
        1,
        1,
        Some("surprise episode selection fitted"),
    );

    send_return_components_progress(
        progress_sender,
        ReturnComponentsStageProgressPhase::CandidateInputValidation,
        0,
        1,
        Some("validate candidate generation input"),
    );

    candidate_generation_input.validate()?;

    send_return_components_progress(
        progress_sender,
        ReturnComponentsStageProgressPhase::CandidateInputValidation,
        1,
        1,
        Some("candidate generation input valid"),
    );

    send_return_components_progress(
        progress_sender,
        ReturnComponentsStageProgressPhase::SignalSorting,
        0,
        1,
        Some("sort observed signals"),
    );

    let sorted_signals = sort_candidate_generation_signals(
        candidate_generation_input,
        derived_close_open_gap_candidates,
        fitted_signal_partition_layer,
        &fitted_surprise_episode_selection_layer,
    );

    send_return_components_progress(
        progress_sender,
        ReturnComponentsStageProgressPhase::SignalSorting,
        1,
        1,
        Some("observed signals sorted"),
    );

    send_return_components_progress(
        progress_sender,
        ReturnComponentsStageProgressPhase::SignalSummary,
        0,
        1,
        Some("summarize observed signals"),
    );

    let (summary_statistics, continuous_return_calibration_sample) =
        summarize_candidate_generation_signals(
            candidate_generation_input,
            &sorted_signals,
            activity_calibration_context,
        );

    send_return_components_progress(
        progress_sender,
        ReturnComponentsStageProgressPhase::SignalSummary,
        1,
        1,
        Some("observed signals summarized"),
    );

    send_return_components_progress(
        progress_sender,
        ReturnComponentsStageProgressPhase::QuantileBinningPolicyFit,
        0,
        1,
        Some("fit quantile binning policy"),
    );

    let fitted_quantile_binning_policy = quantile_binning_fit::fit_quantile_binning_policy(
        sorted_signals.surprise_open_jump_magnitudes.as_slice(),
        sorted_signals.combined_gap_magnitudes.as_slice(),
    );

    send_return_components_progress(
        progress_sender,
        ReturnComponentsStageProgressPhase::QuantileBinningPolicyFit,
        1,
        1,
        Some("quantile binning policy fitted"),
    );

    send_return_components_progress(
        progress_sender,
        ReturnComponentsStageProgressPhase::SurpriseArrivalFit,
        0,
        1,
        Some("fit surprise arrival layer"),
    );

    let fitted_surprise_arrival_layer = surprise_intensity_fit::fit_surprise_news_arrival_layer(
        sorted_signals.source_window.window_start_utc_nanos,
        sorted_signals.source_window.window_end_utc_nanos,
        &sorted_signals
            .selected_surprise_jump_episodes
            .iter()
            .map(|episode| episode.terminal_timestamp_utc_nanos)
            .collect::<Vec<_>>(),
    );

    send_return_components_progress(
        progress_sender,
        ReturnComponentsStageProgressPhase::SurpriseArrivalFit,
        1,
        1,
        Some("surprise arrival layer fitted"),
    );

    send_return_components_progress(
        progress_sender,
        ReturnComponentsStageProgressPhase::ReturnJumpMarkFit,
        0,
        1,
        Some("fit return jump mark layer"),
    );

    let fitted_return_jump_mark_layer = mark_layer_fit::fit_return_jump_mark_layer(
        candidate_generation_input.shared_mark_law_template,
        summary_statistics,
    );

    send_return_components_progress(
        progress_sender,
        ReturnComponentsStageProgressPhase::ReturnJumpMarkFit,
        1,
        1,
        Some("return jump mark layer fitted"),
    );

    send_return_components_progress(
        progress_sender,
        ReturnComponentsStageProgressPhase::ActivityCouplingFit,
        0,
        1,
        Some("fit activity coupling layer"),
    );

    let fitted_activity_coupling_layer =
        activity_coupling_fit::fit_activity_coupling_layer(summary_statistics);

    send_return_components_progress(
        progress_sender,
        ReturnComponentsStageProgressPhase::ActivityCouplingFit,
        1,
        1,
        Some("activity coupling layer fitted"),
    );

    send_return_components_progress(
        progress_sender,
        ReturnComponentsStageProgressPhase::ContinuousReturnLayerFit,
        0,
        1,
        Some("fit continuous return layer"),
    );

    let fitted_continuous_return_layer = continuous_layer_fit::fit_continuous_return_layer(
        summary_statistics,
        &continuous_return_calibration_sample,
        stage_seed,
        stage_solver_settings,
        progress_sender,
    )?;

    send_return_components_progress(
        progress_sender,
        ReturnComponentsStageProgressPhase::ContinuousReturnLayerFit,
        1,
        1,
        Some("continuous return layer fitted"),
    );

    send_return_components_progress(
        progress_sender,
        ReturnComponentsStageProgressPhase::ObjectivePolicyFit,
        0,
        3,
        Some("fit diagnostic policy"),
    );

    let fitted_diagnostic_policy = diagnostic_policy_fit::fit_diagnostic_policy();

    send_return_components_progress(
        progress_sender,
        ReturnComponentsStageProgressPhase::ObjectivePolicyFit,
        1,
        3,
        Some("fit objective scoring policy"),
    );

    let fitted_objective_scoring_policy =
        objective_scoring_policy_fit::fit_objective_scoring_policy(summary_statistics);

    send_return_components_progress(
        progress_sender,
        ReturnComponentsStageProgressPhase::ObjectivePolicyFit,
        2,
        3,
        Some("fit price range sanity policy"),
    );

    let fitted_price_range_sanity_policy =
        price_range_policy_fit::fit_price_range_sanity_policy(summary_statistics);

    send_return_components_progress(
        progress_sender,
        ReturnComponentsStageProgressPhase::ObjectivePolicyFit,
        3,
        3,
        Some("objective policies fitted"),
    );

    let candidate_family_sets = build_candidate_family_sets(
        candidate_generation_input,
        fitted_continuous_return_layer,
        &fitted_surprise_arrival_layer,
        &fitted_return_jump_mark_layer,
        &fitted_activity_coupling_layer,
    );

    let candidate_provenance_template = build_candidate_provenance_template(
        sorted_signals.source_window,
        stage_seed,
        summary_statistics,
        close_open_gap_detection_filter,
        fitted_signal_partition_layer,
        fitted_surprise_episode_selection_layer.policy.clone(),
        fitted_quantile_binning_policy,
        fitted_diagnostic_policy.clone(),
        fitted_objective_scoring_policy.clone(),
        fitted_price_range_sanity_policy.clone(),
    );

    let candidate_family_counts =
        build_candidate_family_counts(&candidate_family_sets.fitted_continuous_return_layer);

    let generated_candidates = enumerate_candidate_inputs(
        candidate_generation_input,
        &candidate_family_sets,
        summary_statistics,
        &continuous_return_calibration_sample,
        &fitted_surprise_arrival_layer.deterministic_intensity_profile,
        stage_seed,
        &candidate_provenance_template,
        &fitted_diagnostic_policy,
        &fitted_objective_scoring_policy,
        &fitted_price_range_sanity_policy,
        progress_sender,
    )?;

    Ok(ReturnComponentsStageCandidateGenerationOutput {
        candidate_inputs: generated_candidates,
        candidate_family_counts,
        fitted_surprise_arrival_layer,
    })
}

fn build_candidate_family_sets(
    candidate_generation_input: &ReturnComponentsStageCandidateGenerationInput,
    fitted_continuous_return_layer: continuous_layer_fit::FittedContinuousReturnLayer,
    fitted_surprise_arrival_layer: &surprise_intensity_fit::FittedSurpriseNewsArrivalLayer,
    fitted_return_jump_mark_layer: &mark_layer_fit::FittedReturnJumpMarkLayer,
    fitted_activity_coupling_layer: &activity_coupling_fit::FittedActivityCouplingLayer,
) -> ReturnComponentsStageCandidateFamilySets {
    ReturnComponentsStageCandidateFamilySets {
        fitted_continuous_return_layer,
        fitted_known_arrival_layer: known_arrival_fit::fit_known_news_arrival_layer(
            &candidate_generation_input.known_news_calendar,
        ),
        fitted_surprise_arrival_layer: build_surprise_arrival_families(
            fitted_surprise_arrival_layer,
        ),
        fitted_return_jump_mark_layer: build_return_jump_mark_scale_families(
            fitted_return_jump_mark_layer,
        ),
        fitted_activity_coupling_layer: build_activity_coupling_families(
            fitted_activity_coupling_layer,
        ),
    }
}

fn build_surprise_arrival_families(
    fitted_surprise_arrival_layer: &surprise_intensity_fit::FittedSurpriseNewsArrivalLayer,
) -> ReturnComponentsSurpriseNewsArrivalCandidateFamily {
    ReturnComponentsSurpriseNewsArrivalCandidateFamily {
        baseline_intensity: round_candidate_value(
            fitted_surprise_arrival_layer.baseline_intensity_per_hour,
        ),
        weekly_hour_utc_multipliers: fitted_surprise_arrival_layer
            .deterministic_intensity_profile
            .weekly_hour_utc_multipliers
            .clone(),
        profile_resolution_hours: fitted_surprise_arrival_layer
            .deterministic_intensity_profile
            .profile_resolution_hours,
    }
}

fn build_return_jump_mark_scale_families(
    fitted_return_jump_mark_layer: &mark_layer_fit::FittedReturnJumpMarkLayer,
) -> ReturnComponentsReturnJumpMarkScaleCandidateFamily {
    ReturnComponentsReturnJumpMarkScaleCandidateFamily {
        sigma_g: fitted_return_jump_mark_layer.sigma_g,
    }
}

fn build_activity_coupling_families(
    fitted_activity_coupling_layer: &activity_coupling_fit::FittedActivityCouplingLayer,
) -> ReturnComponentsActivityCouplingCandidateFamily {
    ReturnComponentsActivityCouplingCandidateFamily {
        known_activity_coupling_scale: fitted_activity_coupling_layer.known_activity_coupling_scale,
        surprise_activity_coupling_scale: fitted_activity_coupling_layer
            .surprise_activity_coupling_scale,
    }
}

fn build_candidate_provenance_template(
    source_window: super::super::contracts::ReturnComponentsStageSignalSourceWindow,
    stage_seed: u64,
    summary_statistics: ReturnComponentsStageObservedSignalSummaryStatistics,
    close_open_gap_detection_filter: &str,
    fitted_signal_partition_layer: signal_partition_fit::FittedSignalPartitionLayer,
    fitted_surprise_episode_selection_policy: ReturnComponentsStageSurpriseEpisodeSelectionPolicy,
    fitted_quantile_binning_policy: ReturnComponentsStageQuantileBinningPolicy,
    fitted_diagnostic_policy: ReturnComponentsStageDiagnosticPolicy,
    fitted_objective_scoring_policy: ReturnComponentsStageObjectiveScoringPolicy,
    fitted_price_range_sanity_policy: ReturnComponentsStagePriceRangeSanityPolicy,
) -> ReturnComponentsStageCandidateProvenance {
    ReturnComponentsStageCandidateProvenance {
        source_windows: vec![
            ReturnComponentsStageCandidateSourceWindow {
                source_name: "known_news_calendar".to_string(),
                window_start_utc_nanos: source_window.window_start_utc_nanos,
                window_end_utc_nanos: source_window.window_end_utc_nanos,
            },
            ReturnComponentsStageCandidateSourceWindow {
                source_name: "realized_known_news_return_samples".to_string(),
                window_start_utc_nanos: source_window.window_start_utc_nanos,
                window_end_utc_nanos: source_window.window_end_utc_nanos,
            },
            ReturnComponentsStageCandidateSourceWindow {
                source_name: "observed_open_session_returns".to_string(),
                window_start_utc_nanos: source_window.window_start_utc_nanos,
                window_end_utc_nanos: source_window.window_end_utc_nanos,
            },
            ReturnComponentsStageCandidateSourceWindow {
                source_name: "detected_intraday_jump_candidates".to_string(),
                window_start_utc_nanos: source_window.window_start_utc_nanos,
                window_end_utc_nanos: source_window.window_end_utc_nanos,
            },
            ReturnComponentsStageCandidateSourceWindow {
                source_name: "observed_close_open_boundary_observations".to_string(),
                window_start_utc_nanos: source_window.window_start_utc_nanos,
                window_end_utc_nanos: source_window.window_end_utc_nanos,
            },
        ],
        preprocessing_filter_policy: RETURN_COMPONENTS_STAGE_SIGNAL_PREPROCESSING_POLICY,
        preprocessing_filters: vec![
            "reject_non_finite_known_news_returns".to_string(),
            "reject_non_finite_observed_open_session_returns".to_string(),
            "exclude_known_news_attributed_open_returns_from_continuous_nig_fit".to_string(),
            "exclude_selected_exogenous_surprise_episodes_from_continuous_nig_fit".to_string(),
            format!(
                "continuous_return_filter_policy={}",
                fitted_signal_partition_layer.continuous_return_filter_policy
            ),
            format!(
                "continuous_return_filter_threshold={:.12}",
                fitted_signal_partition_layer.continuous_return_filter_threshold
            ),
            "reject_non_finite_intraday_jump_returns".to_string(),
            "reject_non_finite_and_non_positive_close_open_boundary_prices".to_string(),
            "enforce_non_negative_post_shock_activity_lift".to_string(),
            "sort_observed_open_session_returns_by_timestamp".to_string(),
            "sort_intraday_jumps_by_jump_timestamp".to_string(),
            "validate_close_open_session_boundary_transitions".to_string(),
            close_open_gap_detection_filter.to_string(),
            "sort_close_open_gaps_by_close_then_open_timestamp".to_string(),
        ],
        surprise_episode_selection_policy: fitted_surprise_episode_selection_policy,
        quantile_binning_policy: fitted_quantile_binning_policy,
        diagnostic_policy: fitted_diagnostic_policy,
        objective_scoring_policy: fitted_objective_scoring_policy,
        price_range_sanity_policy: fitted_price_range_sanity_policy,
        summary_statistics,
        deterministic_seed_metadata: ReturnComponentsStageDeterministicSeedMetadata {
            stage_seed,
            candidate_generation_seed: stage_seed,
            deterministic_seed_policy: RETURN_COMPONENTS_STAGE_DETERMINISTIC_SEED_POLICY,
        },
    }
}

fn build_candidate_family_counts(
    fitted_continuous_return_layer: &continuous_layer_fit::FittedContinuousReturnLayer,
) -> ReturnComponentsStageCandidateFamilyCounts {
    ReturnComponentsStageCandidateFamilyCounts {
        nig_candidate_count: fitted_continuous_return_layer.candidates.len(),
        known_news_arrival_family_count: 1,
        surprise_news_arrival_family_count: 1,
        return_jump_mark_scale_family_count: 1,
        activity_coupling_family_count: 1,
    }
}

fn enumerate_candidate_inputs(
    candidate_generation_input: &ReturnComponentsStageCandidateGenerationInput,
    candidate_family_sets: &ReturnComponentsStageCandidateFamilySets,
    summary_statistics: ReturnComponentsStageObservedSignalSummaryStatistics,
    _continuous_return_calibration_sample: &super::super::continuous_fit::ReturnComponentsStageContinuousReturnCalibrationSample,
    fitted_surprise_deterministic_intensity_profile: &SurpriseNewsDeterministicIntensityProfile,
    _stage_seed: u64,
    candidate_provenance_template: &ReturnComponentsStageCandidateProvenance,
    fitted_diagnostic_policy: &ReturnComponentsStageDiagnosticPolicy,
    fitted_objective_scoring_policy: &ReturnComponentsStageObjectiveScoringPolicy,
    fitted_price_range_sanity_policy: &ReturnComponentsStagePriceRangeSanityPolicy,
    progress_sender: Option<&Sender<ReturnComponentsStageProgressUpdate>>,
) -> Result<
    Vec<ReturnComponentsStageCandidateInput>,
    ReturnComponentsStageCandidateGenerationInputError,
> {
    let known_arrival_family = candidate_family_sets
        .fitted_known_arrival_layer
        .as_candidate_family();
    let surprise_arrival_family = &candidate_family_sets.fitted_surprise_arrival_layer;
    let return_jump_family = &candidate_family_sets.fitted_return_jump_mark_layer;
    let activity_coupling_family = &candidate_family_sets.fitted_activity_coupling_layer;

    let jump_diagnostic = build_jump_diagnostic(
        fitted_diagnostic_policy,
        summary_statistics,
        return_jump_family,
        candidate_generation_input.shared_mark_law_template,
    );
    let surprise_arrival_diagnostic = build_surprise_arrival_diagnostic(
        fitted_diagnostic_policy,
        summary_statistics,
        surprise_arrival_family,
        fitted_surprise_deterministic_intensity_profile,
    );
    let gap_diagnostic = build_gap_diagnostic(
        fitted_diagnostic_policy,
        summary_statistics,
        return_jump_family,
        candidate_generation_input.shared_mark_law_template,
    );
    let activity_coupling_diagnostic = build_activity_coupling_diagnostic(
        fitted_diagnostic_policy,
        summary_statistics,
        activity_coupling_family,
    );

    let total_candidate_count = candidate_family_sets
        .fitted_continuous_return_layer
        .candidates
        .len();

    send_return_components_progress(
        progress_sender,
        ReturnComponentsStageProgressPhase::CandidateEnumeration,
        0,
        total_candidate_count,
        Some("enumerate generated candidates"),
    );

    candidate_family_sets
        .fitted_continuous_return_layer
        .candidates
        .iter()
        .enumerate()
        .map(
            |(nig_candidate_index, continuous_return_candidate)| -> Result<
                ReturnComponentsStageCandidateInput,
                ReturnComponentsStageCandidateGenerationInputError,
            > {
                let candidate_families = ReturnComponentsStageCandidateFamilies {
                    known_news_arrival_controls: known_arrival_family.clone(),
                    surprise_news_arrival_controls: surprise_arrival_family.clone(),
                    return_jump_mark_scale_controls: *return_jump_family,
                    news_to_hawkes_activity_coupling_controls: *activity_coupling_family,
                };

                let return_distribution_diagnostic =
                    continuous_return_candidate.evaluation.diagnostic;
                let price_range_sanity_diagnostic = build_price_range_sanity_diagnostic(
                    fitted_diagnostic_policy,
                    summary_statistics,
                    return_distribution_diagnostic,
                    surprise_arrival_family,
                    return_jump_family,
                    candidate_generation_input.shared_mark_law_template,
                    fitted_price_range_sanity_policy,
                );

                let objective_metric_buckets =
                    objective_scoring_policy_fit::score_candidate_metric_buckets(
                        fitted_objective_scoring_policy,
                        continuous_return_candidate.evaluation.objective_score,
                        &surprise_arrival_diagnostic,
                        &jump_diagnostic,
                        &gap_diagnostic,
                        &activity_coupling_diagnostic,
                        &price_range_sanity_diagnostic,
                    );

                let dominant_scale_driver =
                    objective_scoring_policy_fit::dominant_scale_driver_from_metric_buckets(
                        fitted_objective_scoring_policy,
                        objective_metric_buckets,
                    );

                let objective_terms = objective_scoring_policy_fit::build_candidate_objective_terms(
                    candidate_generation_input.objective_metric_weights,
                    objective_metric_buckets,
                );

                let news_process_configuration =
                    build_news_process_configuration_from_candidate_families(
                        candidate_generation_input,
                        &known_arrival_family,
                        surprise_arrival_family,
                        return_jump_family,
                        activity_coupling_family,
                    )?;

                let candidate_id = format!(
                    "generated_nig_{:03}_known_001_surprise_001_mark_001_coupling_001",
                    nig_candidate_index + 1
                );

                let candidate_input = ReturnComponentsStageCandidateInput {
                    candidate_id: candidate_id.clone(),
                    nig_shared_plus_shift_configuration: continuous_return_candidate
                        .nig_configuration,
                    news_process_configuration,
                    objective_metric_buckets,
                    return_distribution_diagnostic,
                    continuous_distribution_fit_breakdown: continuous_return_candidate
                        .evaluation
                        .fit_breakdown
                        .clone(),
                    surprise_arrival_diagnostic: surprise_arrival_diagnostic.clone(),
                    jump_diagnostic: jump_diagnostic.clone(),
                    gap_diagnostic: gap_diagnostic.clone(),
                    activity_coupling_diagnostic: activity_coupling_diagnostic.clone(),
                    price_range_sanity_diagnostic,
                    objective_terms,
                    candidate_families,
                    candidate_provenance: candidate_provenance_template.clone(),
                    dominant_scale_driver,
                };

                send_return_components_progress(
                    progress_sender,
                    ReturnComponentsStageProgressPhase::CandidateEnumeration,
                    nig_candidate_index + 1,
                    total_candidate_count,
                    Some(candidate_id.as_str()),
                );

                Ok(candidate_input)
            },
        )
        .collect()
}

fn build_news_process_configuration_from_candidate_families(
    candidate_generation_input: &ReturnComponentsStageCandidateGenerationInput,
    known_arrival_family: &ReturnComponentsKnownNewsArrivalCandidateFamily,
    surprise_arrival_family: &ReturnComponentsSurpriseNewsArrivalCandidateFamily,
    return_jump_family: &ReturnComponentsReturnJumpMarkScaleCandidateFamily,
    activity_coupling_family: &ReturnComponentsActivityCouplingCandidateFamily,
) -> Result<NewsProcessConfiguration, ReturnComponentsStageCandidateGenerationInputError> {
    let mut shared_mark_law = candidate_generation_input.shared_mark_law_template;
    shared_mark_law.coupled_gap_marks =
        CoupledGapMarkConfiguration::new(return_jump_family.sigma_g);

    NewsProcessConfiguration::try_new(
        NewsArrivalConfiguration {
            known: KnownNewsArrivalConfiguration {
                scheduled_event_timestamps_utc_nanos: known_arrival_family
                    .retained_known_event_timestamps_utc_nanos
                    .clone(),
            },
            surprise: SurpriseNewsArrivalConfiguration {
                baseline_intensity: surprise_arrival_family.baseline_intensity,
            },
        },
        shared_mark_law,
        NewsComponentMarkScaleConfiguration {
            activity_coupling_scale: activity_coupling_family.known_activity_coupling_scale,
            return_jump_scale: 1.0,
        },
        NewsComponentMarkScaleConfiguration {
            activity_coupling_scale: activity_coupling_family.surprise_activity_coupling_scale,
            return_jump_scale: 1.0,
        },
    )
    .map_err(|source| {
        ReturnComponentsStageCandidateGenerationInputError::InvalidSharedMarkLawTemplate {
            error_message: source.to_string(),
        }
    })
}
