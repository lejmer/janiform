use janiform_calibration_artifacts::ReturnComponentsStageFittedParameters;
use janiform_model_kernels::nig::NigSharedPlusShiftConfiguration;

use super::super::contracts::{
    ReturnComponentsActivityCouplingCandidateFamily,
    ReturnComponentsReturnJumpMarkScaleCandidateFamily,
    ReturnComponentsStageActivityCalibrationContextSnapshot,
    ReturnComponentsStageCandidateGenerationInput,
    ReturnComponentsStageCandidateGenerationInputError,
    ReturnComponentsStageExactCandidateEvaluation,
    ReturnComponentsStageExactCandidateEvaluationError,
    ReturnComponentsStageObservedSignalSummaryStatistics,
    ReturnComponentsSurpriseNewsArrivalCandidateFamily,
};
use super::super::diagnostic_policy_fit;
use super::super::objective_scoring_policy_fit;
use super::super::price_range_policy_fit;
use super::super::{continuous_fit, signal_partition_fit, surprise_episode_selection_fit};
use super::diagnostics::{
    build_activity_coupling_diagnostic, build_gap_diagnostic, build_jump_diagnostic,
    build_price_range_sanity_diagnostic, build_surprise_arrival_diagnostic,
    collect_return_components_exact_candidate_constraint_violations,
};
use super::signals::{
    ReturnComponentsStageActivityCalibrationContext, derive_continuous_fit_business_time_scales,
    sort_candidate_generation_signals, summarize_candidate_generation_signals,
};
use janiform_market_time::calendar::ExchangeCalendarRegistry;

pub(crate) fn prepare_observed_signal_inputs_for_joint_refinement(
    exchange_code: &str,
    calendar_registry: &ExchangeCalendarRegistry,
    candidate_generation_input: ReturnComponentsStageCandidateGenerationInput,
    activity_calibration_context: Option<ReturnComponentsStageActivityCalibrationContext>,
) -> Result<
    ReturnComponentsStageJointRefinementObservedSignalPreparation,
    ReturnComponentsStageCandidateGenerationInputError,
> {
    let prepared_candidate_generation_input =
        super::close_open_gap::prepare_candidate_generation_input(
            exchange_code,
            calendar_registry,
            candidate_generation_input,
        )?;

    let fitted_signal_partition_layer = signal_partition_fit::fit_signal_partition_layer(
        prepared_candidate_generation_input
            .candidate_generation_input
            .observed_open_session_returns
            .as_slice(),
        prepared_candidate_generation_input
            .candidate_generation_input
            .realized_known_news_return_samples
            .as_slice(),
        prepared_candidate_generation_input
            .candidate_generation_input
            .detected_intraday_jump_candidates
            .as_slice(),
        &prepared_candidate_generation_input
            .candidate_generation_input
            .tick_grid_observation_model,
    );
    let fitted_surprise_episode_selection_layer =
        surprise_episode_selection_fit::fit_surprise_episode_selection_layer(
            prepared_candidate_generation_input
                .candidate_generation_input
                .detected_intraday_jump_candidates
                .as_slice(),
            fitted_signal_partition_layer.continuous_return_filter_threshold,
        );

    let sorted_signals = sort_candidate_generation_signals(
        &prepared_candidate_generation_input.candidate_generation_input,
        prepared_candidate_generation_input
            .derived_close_open_gap_candidates
            .as_slice(),
        fitted_signal_partition_layer,
        &fitted_surprise_episode_selection_layer,
    );

    let (_, continuous_return_calibration_sample) = summarize_candidate_generation_signals(
        &prepared_candidate_generation_input.candidate_generation_input,
        &sorted_signals,
        activity_calibration_context,
    );

    Ok(
        ReturnComponentsStageJointRefinementObservedSignalPreparation {
            continuous_return_calibration_sample,
        },
    )
}

#[derive(Debug, Clone)]
pub(crate) struct ReturnComponentsStageJointRefinementObservedSignalPreparation {
    pub continuous_return_calibration_sample:
        continuous_fit::ReturnComponentsStageContinuousReturnCalibrationSample,
}

pub fn evaluate_return_components_exact_candidate(
    candidate_generation_input: ReturnComponentsStageCandidateGenerationInput,
    activity_calibration_context: Option<ReturnComponentsStageActivityCalibrationContextSnapshot>,
    observed_summary_statistics: ReturnComponentsStageObservedSignalSummaryStatistics,
    fitted_parameters: ReturnComponentsStageFittedParameters,
    stage_seed: u64,
) -> Result<
    ReturnComponentsStageExactCandidateEvaluation,
    ReturnComponentsStageExactCandidateEvaluationError,
> {
    if let Err(source) = candidate_generation_input.validate() {
        return Err(
            ReturnComponentsStageExactCandidateEvaluationError::InvalidCandidateGenerationInput {
                error_message: source.to_string(),
            },
        );
    }

    if let Err(source) = NigSharedPlusShiftConfiguration::try_new(
        fitted_parameters.nig_shared_plus_shift_configuration.alpha,
        fitted_parameters.nig_shared_plus_shift_configuration.beta_0,
        fitted_parameters.nig_shared_plus_shift_configuration.delta,
        fitted_parameters.nig_shared_plus_shift_configuration.mu,
        fitted_parameters
            .nig_shared_plus_shift_configuration
            .delta_beta,
    ) {
        return Err(
            ReturnComponentsStageExactCandidateEvaluationError::InvalidFittedParameters {
                error_message: source.to_string(),
            },
        );
    }

    if let Err(source) = fitted_parameters.news_process_configuration.validate() {
        return Err(
            ReturnComponentsStageExactCandidateEvaluationError::InvalidFittedParameters {
                error_message: source.to_string(),
            },
        );
    }

    let activity_calibration_context = activity_calibration_context.map(|context| {
        ReturnComponentsStageActivityCalibrationContext {
            modeled_open_event_rate_per_hour: context.modeled_open_event_rate_per_hour,
            representative_interarrival_hours: context.representative_interarrival_hours,
        }
    });

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
        &candidate_generation_input.tick_grid_observation_model,
    );
    let fitted_surprise_episode_selection_layer =
        surprise_episode_selection_fit::fit_surprise_episode_selection_layer(
            candidate_generation_input
                .detected_intraday_jump_candidates
                .as_slice(),
            fitted_signal_partition_layer.continuous_return_filter_threshold,
        );

    let sorted_signals = sort_candidate_generation_signals(
        &candidate_generation_input,
        &[],
        fitted_signal_partition_layer,
        &fitted_surprise_episode_selection_layer,
    );

    let continuous_fit_business_time_scales =
        derive_continuous_fit_business_time_scales(&sorted_signals, activity_calibration_context);

    let continuous_return_calibration_sample =
        continuous_fit::build_continuous_return_calibration_sample(
            &sorted_signals,
            continuous_fit_business_time_scales.as_slice(),
            candidate_generation_input.open_session_sell_share_proxy,
        );

    let continuous_return_evaluation = continuous_fit::evaluate_continuous_nig_candidate(
        fitted_parameters.nig_shared_plus_shift_configuration,
        &continuous_return_calibration_sample,
        observed_summary_statistics,
        stage_seed,
    );

    let surprise_arrival_family = ReturnComponentsSurpriseNewsArrivalCandidateFamily {
        open_baseline_intensity: fitted_parameters
            .news_process_configuration
            .arrivals
            .surprise
            .open_baseline_intensity,
        closed_baseline_intensity: fitted_parameters
            .news_process_configuration
            .arrivals
            .surprise
            .closed_baseline_intensity,
        weekly_utc_profile_multipliers: fitted_parameters
            .surprise_deterministic_intensity_profile
            .weekly_utc_profile_multipliers
            .clone(),
        profile_resolution_hours: fitted_parameters
            .surprise_deterministic_intensity_profile
            .profile_resolution_hours,
    };

    let return_jump_family = ReturnComponentsReturnJumpMarkScaleCandidateFamily {
        known_return_jump_drift: fitted_parameters
            .news_process_configuration
            .known_component_mark_scales
            .return_jump_drift,
        known_return_jump_volatility: fitted_parameters
            .news_process_configuration
            .known_component_mark_scales
            .return_jump_volatility,
        surprise_return_jump_drift: fitted_parameters
            .news_process_configuration
            .surprise_component_mark_scales
            .return_jump_drift,
        surprise_return_jump_volatility: fitted_parameters
            .news_process_configuration
            .surprise_component_mark_scales
            .return_jump_volatility,
    };

    let activity_coupling_family = ReturnComponentsActivityCouplingCandidateFamily {
        known_activity_coupling_scale: fitted_parameters
            .news_process_configuration
            .known_component_mark_scales
            .activity_coupling_scale,
        surprise_activity_coupling_scale: fitted_parameters
            .news_process_configuration
            .surprise_component_mark_scales
            .activity_coupling_scale,
    };

    let fitted_diagnostic_policy = diagnostic_policy_fit::fit_diagnostic_policy();
    let fitted_objective_scoring_policy =
        objective_scoring_policy_fit::fit_objective_scoring_policy(observed_summary_statistics);

    let jump_diagnostic = build_jump_diagnostic(
        &fitted_diagnostic_policy,
        observed_summary_statistics,
        &return_jump_family,
        candidate_generation_input.shared_mark_law_template,
    );

    let surprise_arrival_diagnostic = build_surprise_arrival_diagnostic(
        &fitted_diagnostic_policy,
        observed_summary_statistics,
        &surprise_arrival_family,
        &fitted_parameters.surprise_deterministic_intensity_profile,
    );

    let gap_diagnostic = build_gap_diagnostic(
        &fitted_diagnostic_policy,
        observed_summary_statistics,
        &return_jump_family,
        candidate_generation_input.shared_mark_law_template,
    );

    let activity_coupling_diagnostic = build_activity_coupling_diagnostic(
        &fitted_diagnostic_policy,
        observed_summary_statistics,
        &activity_coupling_family,
    );

    let fitted_price_range_sanity_policy =
        price_range_policy_fit::fit_price_range_sanity_policy(observed_summary_statistics);

    let price_range_sanity_diagnostic = build_price_range_sanity_diagnostic(
        &fitted_diagnostic_policy,
        observed_summary_statistics,
        continuous_return_evaluation.diagnostic,
        &surprise_arrival_family,
        &return_jump_family,
        candidate_generation_input.shared_mark_law_template,
        &fitted_price_range_sanity_policy,
    );

    let objective_metric_buckets = objective_scoring_policy_fit::score_candidate_metric_buckets(
        &fitted_objective_scoring_policy,
        continuous_return_evaluation.objective_score,
        &surprise_arrival_diagnostic,
        &jump_diagnostic,
        &gap_diagnostic,
        &activity_coupling_diagnostic,
        &price_range_sanity_diagnostic,
    );

    let dominant_scale_driver =
        objective_scoring_policy_fit::dominant_scale_driver_from_metric_buckets(
            &fitted_objective_scoring_policy,
            objective_metric_buckets,
        );

    let raw_objective_value = objective_scoring_policy_fit::weighted_objective_value(
        candidate_generation_input.objective_metric_weights,
        objective_metric_buckets,
    );

    let constraint_violations =
        collect_return_components_exact_candidate_constraint_violations(&fitted_parameters);

    Ok(ReturnComponentsStageExactCandidateEvaluation {
        raw_objective_value,
        objective_metric_buckets,
        return_distribution_diagnostic: continuous_return_evaluation.diagnostic,
        continuous_return_sample_diagnostic:
            super::super::contracts::ReturnComponentsStageContinuousReturnSampleDiagnostic {
                observed_returns:
                    continuous_fit::continuous_return_calibration_sample_observed_returns(
                        &continuous_return_calibration_sample,
                    )
                    .to_vec(),
                fitted_returns: continuous_return_evaluation.fitted_returns.clone(),
                business_time_scales:
                    continuous_fit::continuous_return_calibration_sample_business_time_scales(
                        &continuous_return_calibration_sample,
                    )
                    .to_vec(),
            },
        continuous_distribution_fit_breakdown: continuous_return_evaluation.fit_breakdown,
        surprise_arrival_diagnostic,
        jump_diagnostic,
        gap_diagnostic,
        activity_coupling_diagnostic,
        price_range_sanity_diagnostic,
        constraint_violation_count: constraint_violations.len(),
        constraint_violations,
        dominant_scale_driver,
    })
}
