use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::mpsc::Sender;

use janiform_foundations::numeric::{
    adjusted_shape_jackknife_standard_errors, arithmetic_mean_or_zero, bowley_skewness_from_sorted,
    central_skewness_or_zero, compute_sorted_quantile_or_zero, l_moment_lambda2_from_sorted,
    l_moment_tau3_from_sorted, l_moment_tau4_from_sorted, relative_rmse,
    sample_adjusted_excess_kurtosis, sample_mean_variance, sample_standard_deviation,
    sample_variance, sorted_finite_values, standardized_path_distance,
    winsorized_sample_standard_deviation_from_sorted,
};
use janiform_foundations::time::{
    NANOS_PER_DAY, NANOS_PER_FIFTEEN_MINUTES, NANOS_PER_FIVE_MINUTES, NANOS_PER_HOUR,
    NANOS_PER_WEEK, duration_nanos_to_hours,
};

use super::execution::{
    JOINT_REFINEMENT_POSITIVE_METRIC_FLOOR, JointRefinementAggregatedLikelihoodContextCacheKey,
    JointRefinementAggregatedLikelihoodContexts, JointRefinementCandidateEvaluation,
    JointRefinementContext, JointRefinementDeterministicCacheKey,
    JointRefinementDeterministicEvaluation, JointRefinementFidelityTier,
    JointRefinementObservedData, JointRefinementObservedReturnShapeStatisticCacheKey,
    JointRefinementRawAdmissibilityMetrics, JointRefinementReturnShapeHorizon,
    JointRefinementReturnShapeStatisticEstimate, JointRefinementSeedSummary,
    build_joint_coarse_interval_observations, elapsed_millis, ordered_parameter_bits,
};
use super::setup::{
    CandidateSeedSummariesCacheInput, cached_seed_summaries_for_candidate_seeds,
    parameter_vector_to_fitted_parameters,
};
use super::tick_shape_proxy::coarse_search_tick_shape_proxy_returns;
use crate::objective::CalibrationLossBuilder;
use crate::objective_families::{
    ObjectiveCoordinateInput, ObjectiveFamilyInput, block_diagonal_covariance,
    covariance_from_delete_block_vectors, diagonal_covariance, evaluate_objective_family,
    mean_vector, model_mean_covariance, share_contrast_covariance, share_contrast_vector,
    sum_covariance_matrices,
};
use crate::stages::joint_metrics::{
    JointRefinementBusinessTimeBucketReturnScaleSummaries, JointRefinementReturnScaleSummary,
    interval_log_returns, log_ratio_distance, mean_absolute_return_by_utc_hour,
    normalized_profile_distance, return_scale_summaries_by_business_time_bucket,
    return_scale_summary, return_scale_summary_from_returns, rolling_variance_activity_metric,
    scale_normalized_returns, tick_count_series,
};
use crate::stages::long_variance::metrics::{
    LongVarianceHorizonInputIntervals, aggregate_intervals_to_fixed_duration,
    long_variance_horizon_summaries,
};
use crate::stages::return_components::{
    ContinuousLikelihoodBlockScore, ContinuousLikelihoodEvaluatorCounts,
    ContinuousLikelihoodFidelityTier, ContinuousLikelihoodObjectiveEvaluation,
    build_continuous_return_optimizer_context,
    evaluate_configuration_likelihood_objective_with_context_and_counts,
    rescale_continuous_return_optimizer_context_business_time_scales,
};
use janiform_calibration_artifacts::{
    CalibrationLossEvaluation, CalibrationLossTermKind, CalibrationOptimizerSettings,
    JointRefinementStageFittedParameters, JointRefinementStageObjectiveBreakdown,
};
use janiform_foundations::complex_math::ComplexScalar;
use janiform_model_kernels::hawkes::{
    HawkesKernelLoadings, HawkesKernelParameters, spectral_radius_alpha_over_beta,
};

use super::contracts::{
    JointRefinementStageObjectiveTermDiagnostic, JointRefinementStageProgressPhase,
    JointRefinementStageProgressUpdate,
};

pub(super) use janiform_foundations::numeric::root_mean_square_or_infinity as root_mean_square;

mod long_variance;
mod return_shape;

const ACTIVITY_PROFILE_SESSION_HOURS: usize = 24;
const RETURN_SHAPE_WINSORIZED_VOLATILITY_LOWER_QUANTILE: f64 = 0.05;
const RETURN_SHAPE_WINSORIZED_VOLATILITY_UPPER_QUANTILE: f64 = 0.95;

#[derive(Debug, Clone, Copy)]
pub(super) struct JointRefinementLikelihoodFitWithCounts {
    pub(super) fit: f64,
    pub(super) component_shifted_average_negative_log_likelihoods: [f64; 3],
    pub(super) component_shifted_average_negative_log_likelihood_variances: [Option<f64>; 3],
    pub(super) exact_likelihood_evaluator_counts: ContinuousLikelihoodEvaluatorCounts,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct JointRefinementRestartAdmissionScreening {
    pub(super) aggregate_score: f64,
    pub(super) component_ids: Vec<String>,
    pub(super) component_scores: Vec<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct JointRefinementRestartAdmissionDiagnostics {
    pub(super) screening: JointRefinementRestartAdmissionScreening,
    pub(super) term_diagnostics: Vec<JointRefinementStageObjectiveTermDiagnostic>,
}

impl JointRefinementRestartAdmissionScreening {
    pub(super) fn maximum_component_score(&self) -> f64 {
        self.component_scores
            .iter()
            .copied()
            .fold(self.aggregate_score, f64::max)
    }

    pub(super) fn component_score_sum(&self) -> f64 {
        if self.component_scores.is_empty() {
            return self.aggregate_score;
        }

        self.component_scores.iter().sum()
    }
}

pub(super) fn evaluate_joint_candidate(
    parameter_vector: &[f64],
    context: &JointRefinementContext,
    fidelity_tier: JointRefinementFidelityTier,
    solver_settings: &CalibrationOptimizerSettings,
) -> JointRefinementCandidateEvaluation {
    let cache_key = super::execution::JointRefinementObjectiveCacheKey {
        fidelity_tier,
        parameter_bits: ordered_parameter_bits(parameter_vector),
    };

    if let Ok(cache_guard) = context.objective_cache.lock()
        && let Some(cached_value) = cache_guard.get(&cache_key)
    {
        context.profiling_state.record_objective_cache_hit();
        return cached_value.clone();
    }

    context.profiling_state.record_objective_cache_miss();

    let candidate_evaluation =
        build_joint_candidate_evaluation(parameter_vector, context, fidelity_tier, solver_settings);

    if let Ok(mut cache_guard) = context.objective_cache.lock() {
        cache_guard.insert(cache_key, candidate_evaluation.clone());
    }

    candidate_evaluation
}

pub(super) fn evaluate_joint_fitted_candidate_restart_admission_diagnostics(
    fitted_parameters: &JointRefinementStageFittedParameters,
    context: &JointRefinementContext,
) -> Result<JointRefinementRestartAdmissionDiagnostics, &'static str> {
    if violates_runtime_seasonality_scaled_stationarity(fitted_parameters, context) {
        context
            .profiling_state
            .record_invalid_candidate_evaluation();
        return Err("seasonality_scaled_stationarity_failed");
    }

    let deterministic_evaluation =
        build_restart_screening_deterministic_evaluation(fitted_parameters, context);
    if let Some(failure_reason) =
        deterministic_pre_simulation_dynamics_failure(&deterministic_evaluation)
    {
        return Err(failure_reason);
    }

    let loss_evaluation =
        build_joint_loss_evaluation(fitted_parameters, &deterministic_evaluation, &[], context);
    let component_ids = loss_evaluation
        .family_losses
        .iter()
        .map(|family_loss| family_loss.family_id.clone())
        .chain(
            loss_evaluation
                .hard_constraint_diagnostics
                .iter()
                .map(|constraint| constraint.constraint_id.clone()),
        )
        .collect::<Vec<_>>();
    let component_scores = loss_evaluation
        .family_losses
        .iter()
        .map(|family_loss| family_loss.loss)
        .chain(
            loss_evaluation
                .hard_constraint_diagnostics
                .iter()
                .map(|constraint| constraint.loss),
        )
        .collect::<Vec<_>>();

    if loss_evaluation.global_loss.is_finite()
        && component_scores
            .iter()
            .all(|component_score| component_score.is_finite())
    {
        Ok(JointRefinementRestartAdmissionDiagnostics {
            screening: JointRefinementRestartAdmissionScreening {
                aggregate_score: loss_evaluation.global_loss,
                component_ids,
                component_scores,
            },
            term_diagnostics: loss_evaluation.term_diagnostics,
        })
    } else {
        Err("deterministic_restart_global_loss_not_finite")
    }
}

fn build_restart_screening_deterministic_evaluation(
    fitted_parameters: &JointRefinementStageFittedParameters,
    context: &JointRefinementContext,
) -> JointRefinementDeterministicEvaluation {
    // Screening intentionally uses seed fidelity and avoids the full-fidelity cache.
    // The deterministic cache key does not encode likelihood fidelity.
    build_deterministic_evaluation_for_fidelity(
        fitted_parameters,
        context,
        ContinuousLikelihoodFidelityTier::Seed,
        LikelihoodContextCachePolicy::Uncached,
    )
}

pub(super) fn evaluate_joint_fitted_parameters(
    fitted_parameters: JointRefinementStageFittedParameters,
    parameter_bits: Option<&[u64]>,
    context: &JointRefinementContext,
    fidelity_tier: JointRefinementFidelityTier,
    solver_settings: &CalibrationOptimizerSettings,
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    progress_phase: Option<JointRefinementStageProgressPhase>,
) -> JointRefinementCandidateEvaluation {
    build_joint_candidate_evaluation_for_fitted_parameters(
        fitted_parameters,
        parameter_bits,
        context,
        fidelity_tier,
        solver_settings,
        progress_sender,
        progress_phase,
    )
}

fn build_joint_candidate_evaluation(
    parameter_vector: &[f64],
    context: &JointRefinementContext,
    fidelity_tier: JointRefinementFidelityTier,
    solver_settings: &CalibrationOptimizerSettings,
) -> JointRefinementCandidateEvaluation {
    let parameter_bits = ordered_parameter_bits(parameter_vector);

    let simulation_parameter_bits = if context.option_pricing_refinement_enabled {
        Some(&[][..])
    } else {
        Some(parameter_bits.as_slice())
    };

    let Some(fitted_parameters) = parameter_vector_to_fitted_parameters(parameter_vector, context)
    else {
        return invalid_candidate_evaluation(
            context,
            &context.baseline_hawkes_fitted_parameters,
            &context.baseline_return_components_fitted_parameters,
            solver_settings.constraint_penalty_weight,
            "parameter_vector_to_fitted_parameters_failed",
        );
    };

    build_joint_candidate_evaluation_for_fitted_parameters(
        fitted_parameters,
        simulation_parameter_bits,
        context,
        fidelity_tier,
        solver_settings,
        None,
        None,
    )
}

fn build_joint_candidate_evaluation_for_fitted_parameters(
    fitted_parameters: JointRefinementStageFittedParameters,
    parameter_bits: Option<&[u64]>,
    context: &JointRefinementContext,
    fidelity_tier: JointRefinementFidelityTier,
    solver_settings: &CalibrationOptimizerSettings,
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    progress_phase: Option<JointRefinementStageProgressPhase>,
) -> JointRefinementCandidateEvaluation {
    let simulation_seed_count = match fidelity_tier {
        JointRefinementFidelityTier::CoarseLocal => 1,
        JointRefinementFidelityTier::Local => 1,
        JointRefinementFidelityTier::Final => context.final_simulation_seeds.len(),
    };

    let total_progress_units = 1 + simulation_seed_count;

    if violates_runtime_seasonality_scaled_stationarity(&fitted_parameters, context) {
        return invalid_candidate_evaluation(
            context,
            &context.baseline_hawkes_fitted_parameters,
            &context.baseline_return_components_fitted_parameters,
            solver_settings.constraint_penalty_weight,
            "seasonality_scaled_stationarity_failed",
        );
    }

    emit_joint_refinement_candidate_evaluation_progress(
        progress_sender,
        progress_phase,
        0,
        total_progress_units,
        Some("deterministic_evaluation"),
    );

    let simulation_parameter_bits = if context.option_pricing_refinement_enabled {
        Some(&[][..])
    } else {
        parameter_bits
    };

    let deterministic_evaluation = resolve_deterministic_evaluation(
        &fitted_parameters,
        simulation_parameter_bits,
        context,
        fidelity_tier,
    );

    emit_joint_refinement_candidate_evaluation_progress(
        progress_sender,
        progress_phase,
        1,
        total_progress_units,
        Some("deterministic_evaluation"),
    );

    let raw_aggregated_likelihood_fit = deterministic_evaluation.raw_aggregated_likelihood_fit;

    if let Some(failure_reason) =
        deterministic_pre_simulation_dynamics_failure(&deterministic_evaluation)
    {
        return invalid_candidate_evaluation(
            context,
            &context.baseline_hawkes_fitted_parameters,
            &context.baseline_return_components_fitted_parameters,
            solver_settings.constraint_penalty_weight,
            failure_reason,
        );
    }

    let resolved_seed_summaries = match build_seed_summaries_for_fidelity(
        &fitted_parameters,
        simulation_parameter_bits,
        context,
        fidelity_tier,
        progress_sender,
        progress_phase,
        total_progress_units,
    ) {
        Ok(seed_summaries) => seed_summaries,
        Err(error_message) => {
            return invalid_candidate_evaluation(
                context,
                &context.baseline_hawkes_fitted_parameters,
                &context.baseline_return_components_fitted_parameters,
                solver_settings.constraint_penalty_weight,
                error_message.as_str(),
            );
        }
    };

    let raw_seed_metrics_started_at = std::time::Instant::now();

    let raw_aggregated_return_shape_fit = average_seed_score(
        resolved_seed_summaries
            .iter()
            .map(|seed_summary| aggregated_return_shape_fit(seed_summary, &context.observed_data)),
    );

    let raw_return_asymmetry_fit = average_seed_score(
        resolved_seed_summaries
            .iter()
            .map(|seed_summary| return_asymmetry_fit(seed_summary, &context.observed_data)),
    );

    let raw_tick_return_asymmetry_fit = average_seed_score(
        resolved_seed_summaries
            .iter()
            .map(|seed_summary| tick_return_asymmetry_fit(seed_summary, &context.observed_data)),
    );

    let raw_aggregated_return_scale_fit = average_seed_score(
        resolved_seed_summaries
            .iter()
            .map(|seed_summary| aggregated_return_scale_fit(seed_summary, &context.observed_data)),
    );

    let raw_tick_return_scale_fit = average_seed_score(
        resolved_seed_summaries
            .iter()
            .map(|seed_summary| tick_return_scale_fit(seed_summary, &context.observed_data)),
    );

    let raw_business_time_return_scale_fit =
        average_seed_score(resolved_seed_summaries.iter().map(|seed_summary| {
            business_time_return_scale_fit(seed_summary, &context.observed_data)
        }));

    let raw_intraday_return_level_fit = average_seed_score(
        resolved_seed_summaries
            .iter()
            .map(|seed_summary| intraday_return_level_fit(seed_summary, &context.observed_data)),
    );

    let raw_activity_shape_fit = average_seed_score(
        resolved_seed_summaries
            .iter()
            .map(|seed_summary| activity_shape_fit(seed_summary, &context.observed_data)),
    );

    let raw_activity_level_fit = average_seed_score(
        resolved_seed_summaries
            .iter()
            .map(|seed_summary| activity_level_fit(seed_summary, &context.observed_data)),
    );

    let raw_rv_activity_coupling_fit =
        average_seed_score(resolved_seed_summaries.iter().map(|seed_summary| {
            rv_activity_coupling_fit(
                seed_summary.simulated_15m_rv_activity_metric,
                context.observed_data.observed_15m_rv_activity_metric,
            )
        }));
    let raw_long_variance_horizon_fit =
        average_seed_score(resolved_seed_summaries.iter().map(|seed_summary| {
            long_variance::long_variance_horizon_fit(seed_summary, &context.observed_data)
        }));

    context
        .profiling_state
        .record_raw_seed_metrics_duration(elapsed_millis(raw_seed_metrics_started_at));

    emit_joint_refinement_candidate_evaluation_progress(
        progress_sender,
        progress_phase,
        total_progress_units,
        total_progress_units,
        Some("objective_evaluation"),
    );

    let raw_admissibility_metrics = JointRefinementRawAdmissibilityMetrics {
        aggregated_likelihood_fit: raw_aggregated_likelihood_fit,
        aggregated_return_shape_fit: raw_aggregated_return_shape_fit,
        return_asymmetry_fit: raw_return_asymmetry_fit,
        tick_return_asymmetry_fit: raw_tick_return_asymmetry_fit,
        aggregated_return_scale_fit: raw_aggregated_return_scale_fit,
        tick_return_scale_fit: raw_tick_return_scale_fit,
        business_time_return_scale_fit: raw_business_time_return_scale_fit,
        intraday_return_level_fit: raw_intraday_return_level_fit,
        activity_shape_fit: raw_activity_shape_fit,
        activity_level_fit: raw_activity_level_fit,
        rv_activity_coupling_fit: raw_rv_activity_coupling_fit,
        long_variance_horizon_fit: raw_long_variance_horizon_fit,
    };

    let loss_build_started_at = std::time::Instant::now();
    let loss_evaluation = match fidelity_tier {
        JointRefinementFidelityTier::CoarseLocal => build_joint_coarse_search_loss_evaluation(
            &fitted_parameters,
            &deterministic_evaluation,
            resolved_seed_summaries.as_slice(),
            context,
        ),
        JointRefinementFidelityTier::Local | JointRefinementFidelityTier::Final => {
            build_joint_loss_evaluation(
                &fitted_parameters,
                &deterministic_evaluation,
                resolved_seed_summaries.as_slice(),
                context,
            )
        }
    };

    context
        .profiling_state
        .record_loss_build_duration(elapsed_millis(loss_build_started_at));

    emit_joint_refinement_candidate_evaluation_progress(
        progress_sender,
        progress_phase,
        total_progress_units,
        total_progress_units,
        Some("objective_diagnostics"),
    );

    JointRefinementCandidateEvaluation {
        fitted_parameters,
        raw_admissibility_metrics,
        exact_likelihood_evaluator_counts: deterministic_evaluation
            .exact_likelihood_evaluator_counts,
        objective_terms: loss_evaluation.term_diagnostics.clone(),
        breakdown: loss_evaluation,
        invalid_reason: None,
    }
}

fn emit_joint_refinement_candidate_evaluation_progress(
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    progress_phase: Option<JointRefinementStageProgressPhase>,
    completed_units: usize,
    total_units: usize,
    current_item_label: Option<&str>,
) {
    let Some(progress_sender) = progress_sender else {
        return;
    };

    let Some(progress_phase) = progress_phase else {
        return;
    };

    let _ = progress_sender.send(JointRefinementStageProgressUpdate {
        current_item_label: current_item_label.map(str::to_string),
        ..JointRefinementStageProgressUpdate::new(progress_phase, completed_units, total_units)
    });
}

fn build_seed_summaries_for_fidelity(
    fitted_parameters: &JointRefinementStageFittedParameters,
    simulation_parameter_bits: Option<&[u64]>,
    context: &JointRefinementContext,
    fidelity_tier: JointRefinementFidelityTier,
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    progress_phase: Option<JointRefinementStageProgressPhase>,
    total_progress_units: usize,
) -> Result<Vec<JointRefinementSeedSummary>, String> {
    match fidelity_tier {
        JointRefinementFidelityTier::CoarseLocal => {
            emit_joint_refinement_candidate_evaluation_progress(
                progress_sender,
                progress_phase,
                1,
                total_progress_units,
                Some("coarse_interval_simulation"),
            );

            build_coarse_search_seed_summary(
                context,
                fitted_parameters,
                context.local_simulation_seed,
            )
            .map(|summary| vec![summary])
        }
        JointRefinementFidelityTier::Local | JointRefinementFidelityTier::Final => {
            let simulation_seeds = match fidelity_tier {
                JointRefinementFidelityTier::CoarseLocal => unreachable!(),
                JointRefinementFidelityTier::Local => vec![context.local_simulation_seed],
                JointRefinementFidelityTier::Final => context.final_simulation_seeds.clone(),
            };

            cached_seed_summaries_for_candidate_seeds(CandidateSeedSummariesCacheInput {
                parameter_bits: simulation_parameter_bits,
                fitted_parameters,
                context,
                simulation_seeds: simulation_seeds.as_slice(),
                progress_sender,
                progress_phase,
                completed_units_before_seed_summaries: 1,
                total_units: total_progress_units,
            })
            .map(|seed_summaries| {
                seed_summaries
                    .into_iter()
                    .map(|(_, summary)| summary)
                    .collect::<Vec<_>>()
            })
            .map_err(|error_message| error_message.to_string())
        }
    }
}

fn build_coarse_search_seed_summary(
    context: &JointRefinementContext,
    fitted_parameters: &JointRefinementStageFittedParameters,
    simulation_seed: u64,
) -> Result<JointRefinementSeedSummary, String> {
    let simulated_5m_intervals = build_joint_coarse_interval_observations(
        context,
        fitted_parameters,
        simulation_seed,
        NANOS_PER_FIVE_MINUTES,
        context
            .observed_data
            .simulated_5m_interval_template
            .as_slice(),
    )?;

    let simulated_15m_intervals = build_joint_coarse_interval_observations(
        context,
        fitted_parameters,
        simulation_seed,
        NANOS_PER_FIFTEEN_MINUTES,
        context
            .observed_data
            .simulated_15m_interval_template
            .as_slice(),
    )?;

    let simulated_hourly_intervals = build_joint_coarse_interval_observations(
        context,
        fitted_parameters,
        simulation_seed,
        NANOS_PER_HOUR,
        context
            .observed_data
            .simulated_hourly_interval_template
            .as_slice(),
    )?;

    let simulated_5m_log_returns = interval_log_returns(simulated_5m_intervals.as_slice());
    let simulated_15m_log_returns = interval_log_returns(simulated_15m_intervals.as_slice());
    let simulated_1h_log_returns = interval_log_returns(simulated_hourly_intervals.as_slice());

    let simulated_1d_intervals =
        aggregate_intervals_to_fixed_duration(simulated_hourly_intervals.as_slice(), NANOS_PER_DAY);
    let simulated_1w_intervals = aggregate_intervals_to_fixed_duration(
        simulated_hourly_intervals.as_slice(),
        NANOS_PER_WEEK,
    );

    let simulated_1d_log_returns = interval_log_returns(simulated_1d_intervals.as_slice());
    let simulated_1w_log_returns = interval_log_returns(simulated_1w_intervals.as_slice());

    let simulated_tick_returns =
        coarse_search_tick_shape_proxy_returns(context, fitted_parameters, simulation_seed);

    Ok(JointRefinementSeedSummary {
        simulated_5m_intervals: simulated_5m_intervals.clone(),
        simulated_15m_intervals: simulated_15m_intervals.clone(),
        simulated_tick_shape_returns: scale_normalized_returns(simulated_tick_returns.as_slice()),
        simulated_5m_shape_returns: scale_normalized_returns(simulated_5m_log_returns.as_slice()),
        simulated_15m_shape_returns: scale_normalized_returns(simulated_15m_log_returns.as_slice()),
        simulated_1h_shape_returns: scale_normalized_returns(simulated_1h_log_returns.as_slice()),
        simulated_1d_shape_returns: scale_normalized_returns(simulated_1d_log_returns.as_slice()),
        simulated_1w_shape_returns: scale_normalized_returns(simulated_1w_log_returns.as_slice()),
        simulated_tick_return_scale_summary: return_scale_summary_from_returns(
            simulated_tick_returns.as_slice(),
        ),
        simulated_5m_return_scale_summary: return_scale_summary(simulated_5m_intervals.as_slice()),
        simulated_15m_return_scale_summary: return_scale_summary(
            simulated_15m_intervals.as_slice(),
        ),
        simulated_1h_return_scale_summary: return_scale_summary(
            simulated_hourly_intervals.as_slice(),
        ),
        simulated_1d_return_scale_summary: return_scale_summary(simulated_1d_intervals.as_slice()),
        simulated_5m_business_time_return_scale_summaries:
            return_scale_summaries_by_business_time_bucket(
                simulated_5m_intervals.as_slice(),
                context
                    .observed_data
                    .observed_5m_business_time_bucket_thresholds,
            ),
        simulated_15m_business_time_return_scale_summaries:
            return_scale_summaries_by_business_time_bucket(
                simulated_15m_intervals.as_slice(),
                context
                    .observed_data
                    .observed_15m_business_time_bucket_thresholds,
            ),
        simulated_tick_intraday_mean_absolute_returns: Vec::new(),
        simulated_5m_intraday_mean_absolute_returns: mean_absolute_return_by_utc_hour(
            simulated_5m_intervals.as_slice(),
        ),
        simulated_15m_intraday_mean_absolute_returns: mean_absolute_return_by_utc_hour(
            simulated_15m_intervals.as_slice(),
        ),
        simulated_hourly_tick_counts: tick_count_series(simulated_hourly_intervals.as_slice()),
        simulated_15m_rv_activity_metric: rolling_variance_activity_metric(
            simulated_15m_intervals.as_slice(),
        ),
        simulated_long_variance_horizon_summaries: long_variance_horizon_summaries(
            LongVarianceHorizonInputIntervals {
                five_minute_return_intervals: simulated_5m_intervals.as_slice(),
                five_minute_activity_intervals: simulated_5m_intervals.as_slice(),
                hourly_return_intervals: simulated_hourly_intervals.as_slice(),
                hourly_activity_intervals: simulated_hourly_intervals.as_slice(),
            },
        ),
    })
}

pub(super) fn joint_primary_metrics_from_breakdown(
    breakdown: &JointRefinementStageObjectiveBreakdown,
) -> Vec<f64> {
    breakdown
        .family_losses
        .iter()
        .map(|family| family.loss)
        .chain(
            breakdown
                .hard_constraint_diagnostics
                .iter()
                .map(|constraint| constraint.loss),
        )
        .collect()
}

pub(super) fn compare_joint_candidate_breakdowns(
    input: JointCandidateBreakdownComparisonInput<'_>,
) -> Ordering {
    let JointCandidateBreakdownComparisonInput {
        left_pareto_rank,
        left_breakdown,
        left_admissible,
        left_restart_id,
        right_pareto_rank,
        right_breakdown,
        right_admissible,
        right_restart_id,
        has_admissible_candidate,
    } = input;

    let left_primary_metrics = joint_primary_metrics_from_breakdown(left_breakdown);
    let right_primary_metrics = joint_primary_metrics_from_breakdown(right_breakdown);

    let left_max_primary_metric = left_primary_metrics
        .iter()
        .copied()
        .fold(f64::NEG_INFINITY, f64::max);

    let right_max_primary_metric = right_primary_metrics
        .iter()
        .copied()
        .fold(f64::NEG_INFINITY, f64::max);

    let left_primary_metric_sum = left_primary_metrics.iter().sum::<f64>();
    let right_primary_metric_sum = right_primary_metrics.iter().sum::<f64>();

    (!left_admissible)
        .cmp(&!right_admissible)
        .then_with(|| {
            if has_admissible_candidate {
                left_pareto_rank.cmp(&right_pareto_rank)
            } else {
                left_breakdown
                    .global_loss
                    .total_cmp(&right_breakdown.global_loss)
            }
        })
        .then_with(|| {
            if has_admissible_candidate {
                left_max_primary_metric.total_cmp(&right_max_primary_metric)
            } else {
                Ordering::Equal
            }
        })
        .then_with(|| {
            if has_admissible_candidate {
                left_primary_metric_sum.total_cmp(&right_primary_metric_sum)
            } else {
                Ordering::Equal
            }
        })
        .then_with(|| {
            left_breakdown
                .global_loss
                .total_cmp(&right_breakdown.global_loss)
        })
        .then_with(|| left_pareto_rank.cmp(&right_pareto_rank))
        .then_with(|| left_restart_id.cmp(right_restart_id))
}

pub(super) struct JointCandidateBreakdownComparisonInput<'a> {
    pub(super) left_pareto_rank: usize,
    pub(super) left_breakdown: &'a JointRefinementStageObjectiveBreakdown,
    pub(super) left_admissible: bool,
    pub(super) left_restart_id: &'a str,
    pub(super) right_pareto_rank: usize,
    pub(super) right_breakdown: &'a JointRefinementStageObjectiveBreakdown,
    pub(super) right_admissible: bool,
    pub(super) right_restart_id: &'a str,
    pub(super) has_admissible_candidate: bool,
}

pub(super) fn business_time_bucket_return_scale_fit(
    candidate_summaries: JointRefinementBusinessTimeBucketReturnScaleSummaries,
    observed_summaries: JointRefinementBusinessTimeBucketReturnScaleSummaries,
) -> f64 {
    candidate_summaries
        .bucket_summaries
        .iter()
        .zip(observed_summaries.bucket_summaries.iter())
        .map(
            |(candidate_summary, observed_summary)| match (candidate_summary, observed_summary) {
                (Some(candidate_summary), Some(observed_summary)) => {
                    return_scale_horizon_fit(*candidate_summary, *observed_summary)
                }
                (None, Some(observed_summary)) => {
                    missing_business_time_bucket_fit(*observed_summary)
                }
                (Some(candidate_summary), None) => {
                    missing_business_time_bucket_fit(*candidate_summary)
                }
                (None, None) => 0.0,
            },
        )
        .sum::<f64>()
        / candidate_summaries.bucket_summaries.len() as f64
}

pub(super) fn intraday_return_level_horizon_fit(
    candidate_profile: &[f64],
    observed_profile: &[f64],
) -> f64 {
    let candidate_total = candidate_profile.iter().sum::<f64>();
    let observed_total = observed_profile.iter().sum::<f64>();

    let candidate_peak = candidate_profile.iter().copied().fold(0.0, f64::max);

    let observed_peak = observed_profile.iter().copied().fold(0.0, f64::max);
    let observed_mean = observed_total / observed_profile.len().max(1) as f64;

    let path_fit = relative_rmse(
        candidate_profile,
        observed_profile,
        observed_mean.max(JOINT_REFINEMENT_POSITIVE_METRIC_FLOOR),
    );

    let total_fit = log_ratio_distance(
        candidate_total,
        observed_total,
        JOINT_REFINEMENT_POSITIVE_METRIC_FLOOR,
    );

    let peak_fit = log_ratio_distance(
        candidate_peak,
        observed_peak,
        JOINT_REFINEMENT_POSITIVE_METRIC_FLOOR,
    );

    root_mean_square([path_fit, total_fit, peak_fit].as_slice())
}

pub(super) fn return_asymmetry_horizon_fit(
    candidate_profile: &[f64],
    observed_profile: &[f64],
) -> f64 {
    let candidate_central_skewness = skewness_or_zero(candidate_profile);
    let observed_central_skewness = skewness_or_zero(observed_profile);

    let candidate_bowley_skewness = bowley_skewness_or_zero(candidate_profile);
    let observed_bowley_skewness = bowley_skewness_or_zero(observed_profile);

    root_mean_square(
        [
            (candidate_central_skewness - observed_central_skewness).abs(),
            (candidate_bowley_skewness - observed_bowley_skewness).abs(),
        ]
        .as_slice(),
    )
}

fn average_seed_score(values: impl Iterator<Item = f64>) -> f64 {
    let collected = values.collect::<Vec<_>>();
    collected.iter().sum::<f64>() / collected.len().max(1) as f64
}

pub(crate) fn invalid_candidate_evaluation(
    context: &JointRefinementContext,
    baseline_hawkes_fitted_parameters: &janiform_calibration_artifacts::HawkesStageFittedParameters,
    baseline_return_components_fitted_parameters: &janiform_calibration_artifacts::ReturnComponentsStageFittedParameters,
    _constraint_penalty_weight: f64,
    reason: &str,
) -> JointRefinementCandidateEvaluation {
    context
        .profiling_state
        .record_invalid_candidate_evaluation();

    let breakdown = invalid_loss_evaluation(reason);

    JointRefinementCandidateEvaluation {
        fitted_parameters: JointRefinementStageFittedParameters {
            hawkes_fitted_parameters: baseline_hawkes_fitted_parameters.clone(),
            return_components_fitted_parameters: baseline_return_components_fitted_parameters
                .clone(),
            pricing_block_fitted_parameters: None,
        },
        raw_admissibility_metrics: JointRefinementRawAdmissibilityMetrics {
            aggregated_likelihood_fit: 100.0,
            aggregated_return_shape_fit: 100.0,
            return_asymmetry_fit: 100.0,
            tick_return_asymmetry_fit: 100.0,
            aggregated_return_scale_fit: 100.0,
            tick_return_scale_fit: 100.0,
            business_time_return_scale_fit: 100.0,
            intraday_return_level_fit: 100.0,
            activity_shape_fit: 100.0,
            activity_level_fit: 100.0,
            rv_activity_coupling_fit: 100.0,
            long_variance_horizon_fit: 100.0,
        },
        exact_likelihood_evaluator_counts: ContinuousLikelihoodEvaluatorCounts::default(),
        objective_terms: breakdown.term_diagnostics.clone(),
        breakdown,
        invalid_reason: Some(reason.to_string()),
    }
}

fn invalid_loss_evaluation(reason: &str) -> JointRefinementStageObjectiveBreakdown {
    let mut builder = CalibrationLossBuilder::new();

    builder.add_invalid_candidate_sentinel(format!("invalid_candidate.{reason}"));

    builder.build()
}

fn resolve_deterministic_evaluation(
    fitted_parameters: &JointRefinementStageFittedParameters,
    parameter_bits: Option<&[u64]>,
    context: &JointRefinementContext,
    fidelity_tier: JointRefinementFidelityTier,
) -> JointRefinementDeterministicEvaluation {
    let likelihood_fidelity_tier = likelihood_fidelity_tier_for_joint_fidelity(fidelity_tier);

    let cache_key = parameter_bits.map(|bits| JointRefinementDeterministicCacheKey {
        likelihood_fidelity_tier,
        parameter_bits: bits.to_vec(),
    });

    if let Some(deterministic_cache_key) = cache_key.as_ref()
        && let Ok(cache_guard) = context.deterministic_evaluation_cache.lock()
        && let Some(cached_value) = cache_guard.get(deterministic_cache_key)
    {
        context.profiling_state.record_deterministic_cache_hit();
        return cached_value.clone();
    }

    context.profiling_state.record_deterministic_cache_miss();

    let deterministic_evaluation = build_deterministic_evaluation_for_fidelity(
        fitted_parameters,
        context,
        likelihood_fidelity_tier,
        likelihood_context_cache_policy_for_joint_fidelity(fidelity_tier),
    );

    if let Some(deterministic_cache_key) = cache_key
        && let Ok(mut cache_guard) = context.deterministic_evaluation_cache.lock()
    {
        cache_guard.insert(deterministic_cache_key, deterministic_evaluation.clone());
    }

    deterministic_evaluation
}

fn likelihood_fidelity_tier_for_joint_fidelity(
    fidelity_tier: JointRefinementFidelityTier,
) -> ContinuousLikelihoodFidelityTier {
    match fidelity_tier {
        JointRefinementFidelityTier::CoarseLocal => {
            ContinuousLikelihoodFidelityTier::Representative
        }
        JointRefinementFidelityTier::Local | JointRefinementFidelityTier::Final => {
            ContinuousLikelihoodFidelityTier::Full
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LikelihoodContextCachePolicy {
    CachedByModeledRate,
    Uncached,
}

fn likelihood_context_cache_policy_for_joint_fidelity(
    fidelity_tier: JointRefinementFidelityTier,
) -> LikelihoodContextCachePolicy {
    match fidelity_tier {
        JointRefinementFidelityTier::CoarseLocal => {
            LikelihoodContextCachePolicy::CachedByModeledRate
        }
        JointRefinementFidelityTier::Local | JointRefinementFidelityTier::Final => {
            LikelihoodContextCachePolicy::Uncached
        }
    }
}

fn build_deterministic_evaluation_for_fidelity(
    fitted_parameters: &JointRefinementStageFittedParameters,
    context: &JointRefinementContext,
    likelihood_fidelity_tier: ContinuousLikelihoodFidelityTier,
    context_cache_policy: LikelihoodContextCachePolicy,
) -> JointRefinementDeterministicEvaluation {
    let deterministic_evaluation_started_at = std::time::Instant::now();

    let modeled_rate_started_at = std::time::Instant::now();

    let modeled_open_event_rate_per_hour =
        modeled_open_event_rate_per_hour(fitted_parameters, context);

    context
        .profiling_state
        .record_modeled_rate_duration(elapsed_millis(modeled_rate_started_at));

    let aggregated_likelihood_fit = raw_aggregated_likelihood_fit_for_fidelity_with_counts(
        fitted_parameters,
        modeled_open_event_rate_per_hour,
        context,
        likelihood_fidelity_tier,
        context_cache_policy,
    );

    let expected_activity_counts_started_at = std::time::Instant::now();

    let expected_hourly_tick_counts =
        expected_hourly_tick_counts(modeled_open_event_rate_per_hour, context);

    context
        .profiling_state
        .record_expected_activity_counts_duration(elapsed_millis(
            expected_activity_counts_started_at,
        ));

    let deterministic_activity_shape_started_at = std::time::Instant::now();

    let raw_expected_activity_shape_fit =
        expected_hourly_tick_counts
            .as_ref()
            .map(|expected_hourly_tick_counts| {
                activity_shape_fit_for_counts(
                    expected_hourly_tick_counts.as_slice(),
                    context.observed_data.observed_hourly_tick_counts.as_slice(),
                )
            });

    context
        .profiling_state
        .record_deterministic_activity_shape_duration(elapsed_millis(
            deterministic_activity_shape_started_at,
        ));

    let deterministic_activity_level_started_at = std::time::Instant::now();

    let raw_expected_activity_level_fit =
        expected_hourly_tick_counts
            .as_ref()
            .map(|expected_hourly_tick_counts| {
                activity_level_fit_for_counts(
                    expected_hourly_tick_counts.as_slice(),
                    context.observed_data.observed_hourly_tick_counts.as_slice(),
                )
            });

    context
        .profiling_state
        .record_deterministic_activity_level_duration(elapsed_millis(
            deterministic_activity_level_started_at,
        ));

    let deterministic_evaluation = JointRefinementDeterministicEvaluation {
        modeled_open_event_rate_per_hour,
        raw_aggregated_likelihood_fit: aggregated_likelihood_fit.fit,
        component_shifted_average_negative_log_likelihoods: aggregated_likelihood_fit
            .component_shifted_average_negative_log_likelihoods,
        component_shifted_average_negative_log_likelihood_variances: aggregated_likelihood_fit
            .component_shifted_average_negative_log_likelihood_variances,
        exact_likelihood_evaluator_counts: aggregated_likelihood_fit
            .exact_likelihood_evaluator_counts,
        expected_hourly_tick_counts: expected_hourly_tick_counts.clone(),
        raw_expected_activity_shape_fit,
        raw_expected_activity_level_fit,
    };

    context
        .profiling_state
        .record_deterministic_evaluation_duration(
            deterministic_evaluation_started_at
                .elapsed()
                .as_millis()
                .min(u128::from(u64::MAX)) as u64,
        );

    deterministic_evaluation
}

fn deterministic_pre_simulation_dynamics_failure(
    deterministic_evaluation: &JointRefinementDeterministicEvaluation,
) -> Option<&'static str> {
    if !deterministic_evaluation
        .raw_aggregated_likelihood_fit
        .is_finite()
    {
        return Some("deterministic_pre_simulation_aggregated_likelihood_not_finite");
    }

    None
}

fn expected_hourly_tick_counts(
    modeled_open_event_rate_per_hour: f64,
    context: &JointRefinementContext,
) -> Option<Vec<f64>> {
    if !modeled_open_event_rate_per_hour.is_finite()
        || modeled_open_event_rate_per_hour < 0.0
        || context.runtime_interval_seasonality_factors.len()
            != context
                .observed_data
                .simulated_hourly_interval_template
                .len()
    {
        return None;
    }

    context
        .observed_data
        .simulated_hourly_interval_template
        .iter()
        .zip(context.runtime_interval_seasonality_factors.iter())
        .map(|(interval, interval_factor_grid)| {
            if !interval_factor_grid.is_finite()
                || *interval_factor_grid < 0.0
                || interval.interval_end_utc_nanos < interval.interval_start_utc_nanos
            {
                return None;
            }

            let duration_hours = duration_nanos_to_hours(
                interval.interval_end_utc_nanos - interval.interval_start_utc_nanos,
            );

            let expected_count =
                modeled_open_event_rate_per_hour * interval_factor_grid * duration_hours;

            expected_count.is_finite().then_some(expected_count)
        })
        .collect()
}

fn modeled_open_event_rate_per_hour(
    fitted_parameters: &JointRefinementStageFittedParameters,
    context: &JointRefinementContext,
) -> f64 {
    crate::stages::hawkes::stationary_open_event_rate_per_hour(
        &fitted_parameters.hawkes_fitted_parameters,
    )
    .unwrap_or(context.observed_data.observed_open_event_rate_per_hour)
}

fn raw_aggregated_likelihood_fit_for_fidelity_with_counts(
    fitted_parameters: &JointRefinementStageFittedParameters,
    modeled_open_event_rate_per_hour: f64,
    context: &JointRefinementContext,
    fidelity_tier: ContinuousLikelihoodFidelityTier,
    context_cache_policy: LikelihoodContextCachePolicy,
) -> JointRefinementLikelihoodFitWithCounts {
    let context_resolution_started_at = std::time::Instant::now();

    let likelihood_contexts = resolve_joint_refinement_aggregated_likelihood_contexts(
        modeled_open_event_rate_per_hour,
        context,
        fidelity_tier,
        context_cache_policy,
    );

    context
        .profiling_state
        .record_likelihood_context_resolution_duration(elapsed_millis(
            context_resolution_started_at,
        ));

    raw_aggregated_likelihood_fit_for_contexts_with_counts(
        context,
        fitted_parameters
            .return_components_fitted_parameters
            .nig_shared_plus_shift_configuration,
        likelihood_contexts.as_ref(),
        fidelity_tier,
    )
}

fn resolve_joint_refinement_aggregated_likelihood_contexts(
    modeled_open_event_rate_per_hour: f64,
    context: &JointRefinementContext,
    fidelity_tier: ContinuousLikelihoodFidelityTier,
    cache_policy: LikelihoodContextCachePolicy,
) -> Arc<JointRefinementAggregatedLikelihoodContexts> {
    match cache_policy {
        LikelihoodContextCachePolicy::CachedByModeledRate => {
            let cache_key = JointRefinementAggregatedLikelihoodContextCacheKey {
                likelihood_fidelity_tier: fidelity_tier,
                modeled_open_event_rate_per_hour_bits: modeled_open_event_rate_per_hour.to_bits(),
            };
            if let Ok(cache_guard) = context.aggregated_likelihood_context_cache.lock()
                && let Some(cached_contexts) = cache_guard.get(&cache_key)
            {
                context
                    .profiling_state
                    .record_aggregated_likelihood_context_cache_hit();

                return Arc::clone(cached_contexts);
            }

            context
                .profiling_state
                .record_aggregated_likelihood_context_cache_miss();

            let contexts = Arc::new(build_joint_refinement_aggregated_likelihood_contexts(
                modeled_open_event_rate_per_hour,
                context,
                fidelity_tier,
            ));

            if let Ok(mut cache_guard) = context.aggregated_likelihood_context_cache.lock() {
                cache_guard.insert(cache_key, Arc::clone(&contexts));
            }

            contexts
        }
        LikelihoodContextCachePolicy::Uncached => {
            context
                .profiling_state
                .record_aggregated_likelihood_context_cache_miss();

            Arc::new(build_joint_refinement_aggregated_likelihood_contexts(
                modeled_open_event_rate_per_hour,
                context,
                fidelity_tier,
            ))
        }
    }
}

fn build_joint_refinement_aggregated_likelihood_contexts(
    modeled_open_event_rate_per_hour: f64,
    context: &JointRefinementContext,
    fidelity_tier: ContinuousLikelihoodFidelityTier,
) -> JointRefinementAggregatedLikelihoodContexts {
    let observed_1h_sample_started_at = std::time::Instant::now();

    let observed_1h_sample =
        crate::stages::joint_metrics::build_continuous_calibration_sample_from_aggregated_intervals(
            context.observed_data.observed_1h_intervals.as_slice(),
            modeled_open_event_rate_per_hour,
            context.observed_data.observed_open_event_rate_per_hour,
            context.observed_data.fallback_sell_share_proxy,
        );

    context
        .profiling_state
        .record_likelihood_context_1h_sample_duration(elapsed_millis(
            observed_1h_sample_started_at,
        ));

    let observed_1d_sample_started_at = std::time::Instant::now();

    let observed_1d_sample =
        crate::stages::joint_metrics::build_continuous_calibration_sample_from_aggregated_intervals(
            context.observed_data.observed_1d_intervals.as_slice(),
            modeled_open_event_rate_per_hour,
            context.observed_data.observed_open_event_rate_per_hour,
            context.observed_data.fallback_sell_share_proxy,
        );

    context
        .profiling_state
        .record_likelihood_context_1d_sample_duration(elapsed_millis(
            observed_1d_sample_started_at,
        ));

    let observed_tick_sample_started_at = std::time::Instant::now();

    let tick_context_source = match fidelity_tier {
        ContinuousLikelihoodFidelityTier::Seed
        | ContinuousLikelihoodFidelityTier::Representative
        | ContinuousLikelihoodFidelityTier::Local => {
            &context
                .observed_data
                .weighted_tick_continuous_return_optimizer_context
        }
        ContinuousLikelihoodFidelityTier::Full => {
            &context
                .observed_data
                .observed_tick_continuous_return_optimizer_context
        }
    };

    context
        .profiling_state
        .record_likelihood_context_tick_sample_duration(elapsed_millis(
            observed_tick_sample_started_at,
        ));

    let tick_context_started_at = std::time::Instant::now();

    let tick = rescale_continuous_return_optimizer_context_business_time_scales(
        tick_context_source,
        modeled_open_event_rate_per_hour / context.baseline_modeled_open_event_rate_per_hour,
    );

    context
        .profiling_state
        .record_likelihood_context_tick_optimizer_duration(elapsed_millis(tick_context_started_at));

    let one_hour_context_started_at = std::time::Instant::now();
    let one_hour = build_continuous_return_optimizer_context(&observed_1h_sample);

    context
        .profiling_state
        .record_likelihood_context_1h_optimizer_duration(elapsed_millis(
            one_hour_context_started_at,
        ));

    let one_day_context_started_at = std::time::Instant::now();
    let one_day = build_continuous_return_optimizer_context(&observed_1d_sample);

    context
        .profiling_state
        .record_likelihood_context_1d_optimizer_duration(elapsed_millis(
            one_day_context_started_at,
        ));

    JointRefinementAggregatedLikelihoodContexts {
        tick,
        one_hour,
        one_day,
    }
}

fn raw_aggregated_likelihood_fit_for_contexts_with_counts(
    context: &JointRefinementContext,
    configuration: janiform_model_kernels::nig::NigSharedPlusShiftConfiguration,
    likelihood_contexts: &JointRefinementAggregatedLikelihoodContexts,
    fidelity_tier: ContinuousLikelihoodFidelityTier,
) -> JointRefinementLikelihoodFitWithCounts {
    let tick_likelihood_started_at = std::time::Instant::now();

    let tick_evaluation = evaluate_configuration_likelihood_objective_with_context_and_counts(
        configuration,
        &likelihood_contexts.tick,
        fidelity_tier,
    );

    context
        .profiling_state
        .record_tick_likelihood_duration(elapsed_millis(tick_likelihood_started_at));

    let one_hour_likelihood_started_at = std::time::Instant::now();

    let one_hour_evaluation = evaluate_configuration_likelihood_objective_with_context_and_counts(
        configuration,
        &likelihood_contexts.one_hour,
        fidelity_tier,
    );

    context
        .profiling_state
        .record_1h_likelihood_duration(elapsed_millis(one_hour_likelihood_started_at));

    let one_day_likelihood_started_at = std::time::Instant::now();

    let one_day_evaluation = evaluate_configuration_likelihood_objective_with_context_and_counts(
        configuration,
        &likelihood_contexts.one_day,
        fidelity_tier,
    );

    context
        .profiling_state
        .record_1d_likelihood_duration(elapsed_millis(one_day_likelihood_started_at));

    let component_shifted_average_negative_log_likelihoods = [
        global_shifted_average_negative_log_likelihood(&tick_evaluation),
        global_shifted_average_negative_log_likelihood(&one_hour_evaluation),
        global_shifted_average_negative_log_likelihood(&one_day_evaluation),
    ];

    let temporal_block_count = context.observed_data.hourly_simulation_interval_count;
    let component_shifted_average_negative_log_likelihood_variances = [
        likelihood_block_mean_variance(
            tick_evaluation.global_block_scores.as_slice(),
            temporal_block_count,
        ),
        likelihood_block_mean_variance(
            one_hour_evaluation.global_block_scores.as_slice(),
            temporal_block_count,
        ),
        likelihood_block_mean_variance(
            one_day_evaluation.global_block_scores.as_slice(),
            temporal_block_count,
        ),
    ];

    JointRefinementLikelihoodFitWithCounts {
        fit: [
            tick_evaluation.objective.shifted_objective_score,
            one_hour_evaluation.objective.shifted_objective_score,
            one_day_evaluation.objective.shifted_objective_score,
        ]
        .into_iter()
        .sum::<f64>()
            / 3.0,
        component_shifted_average_negative_log_likelihoods,
        component_shifted_average_negative_log_likelihood_variances,
        exact_likelihood_evaluator_counts: tick_evaluation
            .evaluator_counts
            .add(one_hour_evaluation.evaluator_counts)
            .add(one_day_evaluation.evaluator_counts),
    }
}

fn global_shifted_average_negative_log_likelihood(
    evaluation: &ContinuousLikelihoodObjectiveEvaluation,
) -> f64 {
    evaluation.objective.global_negative_log_likelihood
        + evaluation.objective.log_likelihood_shift
        + evaluation.objective.boundary_penalty
}

fn likelihood_block_mean_variance(
    block_scores: &[ContinuousLikelihoodBlockScore],
    temporal_block_count: usize,
) -> Option<f64> {
    let finite_scores = block_scores
        .iter()
        .copied()
        .filter(|score| {
            score.shifted_negative_log_likelihood.is_finite()
                && score.member_weight.is_finite()
                && score.member_weight > 0.0
        })
        .collect::<Vec<_>>();

    let block_count =
        return_shape_effective_sample_count(finite_scores.len(), temporal_block_count);
    if block_count < 2 {
        return None;
    }

    let mut block_means = Vec::with_capacity(block_count);

    for block_position in 0..block_count {
        let block_start = block_position
            .checked_mul(finite_scores.len())
            .unwrap_or(finite_scores.len())
            / block_count;

        let block_end = (block_position + 1)
            .checked_mul(finite_scores.len())
            .unwrap_or(finite_scores.len())
            / block_count;

        if block_end <= block_start {
            continue;
        }

        if let Some(block_mean) =
            weighted_likelihood_block_mean(&finite_scores[block_start..block_end])
        {
            block_means.push(block_mean);
        }
    }

    if block_means.len() < 2 {
        return None;
    }

    let average_variance = sample_mean_variance(block_means.as_slice())?;
    (average_variance.is_finite() && average_variance >= 0.0).then_some(average_variance)
}

fn weighted_likelihood_block_mean(block_scores: &[ContinuousLikelihoodBlockScore]) -> Option<f64> {
    let mut weighted_sum = 0.0;
    let mut weight_sum = 0.0;

    for block_score in block_scores {
        if !block_score.shifted_negative_log_likelihood.is_finite()
            || !block_score.member_weight.is_finite()
            || block_score.member_weight <= 0.0
        {
            continue;
        }

        weighted_sum += block_score.shifted_negative_log_likelihood * block_score.member_weight;
        weight_sum += block_score.member_weight;
    }

    (weight_sum > 0.0).then_some(weighted_sum / weight_sum)
}

fn aggregated_return_shape_fit(
    seed_summary: &JointRefinementSeedSummary,
    observed_data: &JointRefinementObservedData,
) -> f64 {
    average_seed_score(
        return_shape_horizon_value_pairs(seed_summary, observed_data)
            .into_iter()
            .map(|(simulated_values, observed_values)| {
                crate::stages::return_components::distribution_profile_fit_score(
                    simulated_values,
                    observed_values,
                )
                .profile_fit_score
            }),
    )
}

fn aggregated_return_scale_fit(
    seed_summary: &JointRefinementSeedSummary,
    observed_data: &JointRefinementObservedData,
) -> f64 {
    let fit_1d = return_scale_horizon_fit(
        seed_summary.simulated_1d_return_scale_summary,
        observed_data.observed_1d_return_scale_summary,
    );

    let fit_1h = return_scale_horizon_fit(
        seed_summary.simulated_1h_return_scale_summary,
        observed_data.observed_1h_return_scale_summary,
    );

    let fit_tick = return_scale_horizon_fit(
        seed_summary.simulated_tick_return_scale_summary,
        observed_data.observed_tick_return_scale_summary,
    );

    root_mean_square([fit_tick, fit_1h, fit_1d].as_slice())
}

fn tick_return_scale_fit(
    seed_summary: &JointRefinementSeedSummary,
    observed_data: &JointRefinementObservedData,
) -> f64 {
    return_scale_horizon_fit(
        seed_summary.simulated_tick_return_scale_summary,
        observed_data.observed_tick_return_scale_summary,
    )
}

fn return_asymmetry_fit(
    seed_summary: &JointRefinementSeedSummary,
    observed_data: &JointRefinementObservedData,
) -> f64 {
    let horizon_fits = return_shape_horizon_value_pairs(seed_summary, observed_data)
        .into_iter()
        .map(|(simulated_values, observed_values)| {
            return_asymmetry_horizon_fit(simulated_values, observed_values)
        })
        .collect::<Vec<_>>();

    root_mean_square(horizon_fits.as_slice())
}

fn tick_return_asymmetry_fit(
    seed_summary: &JointRefinementSeedSummary,
    observed_data: &JointRefinementObservedData,
) -> f64 {
    return_asymmetry_horizon_fit(
        seed_summary.simulated_tick_shape_returns.as_slice(),
        observed_data.observed_tick_shape_returns.as_slice(),
    )
}

fn return_shape_horizon_value_pairs<'a>(
    seed_summary: &'a JointRefinementSeedSummary,
    observed_data: &'a JointRefinementObservedData,
) -> [(&'a [f64], &'a [f64]); 6] {
    [
        (
            seed_summary.simulated_tick_shape_returns.as_slice(),
            observed_data.observed_tick_shape_returns.as_slice(),
        ),
        (
            seed_summary.simulated_5m_shape_returns.as_slice(),
            observed_data.observed_5m_shape_returns.as_slice(),
        ),
        (
            seed_summary.simulated_15m_shape_returns.as_slice(),
            observed_data.observed_15m_shape_returns.as_slice(),
        ),
        (
            seed_summary.simulated_1h_shape_returns.as_slice(),
            observed_data.observed_1h_shape_returns.as_slice(),
        ),
        (
            seed_summary.simulated_1d_shape_returns.as_slice(),
            observed_data.observed_1d_shape_returns.as_slice(),
        ),
        (
            seed_summary.simulated_1w_shape_returns.as_slice(),
            observed_data.observed_1w_shape_returns.as_slice(),
        ),
    ]
}

fn business_time_return_scale_fit(
    seed_summary: &JointRefinementSeedSummary,
    observed_data: &JointRefinementObservedData,
) -> f64 {
    let fit_15m = business_time_bucket_return_scale_fit(
        seed_summary.simulated_15m_business_time_return_scale_summaries,
        observed_data.observed_15m_business_time_return_scale_summaries,
    );

    let fit_5m = business_time_bucket_return_scale_fit(
        seed_summary.simulated_5m_business_time_return_scale_summaries,
        observed_data.observed_5m_business_time_return_scale_summaries,
    );

    root_mean_square([fit_5m, fit_15m].as_slice())
}

fn intraday_return_level_fit(
    seed_summary: &JointRefinementSeedSummary,
    observed_data: &JointRefinementObservedData,
) -> f64 {
    let fit_tick = intraday_return_level_horizon_fit(
        seed_summary
            .simulated_tick_intraday_mean_absolute_returns
            .as_slice(),
        observed_data
            .observed_tick_intraday_mean_absolute_returns
            .as_slice(),
    );

    let fit_15m = intraday_return_level_horizon_fit(
        seed_summary
            .simulated_15m_intraday_mean_absolute_returns
            .as_slice(),
        observed_data
            .observed_15m_intraday_mean_absolute_returns
            .as_slice(),
    );

    let fit_5m = intraday_return_level_horizon_fit(
        seed_summary
            .simulated_5m_intraday_mean_absolute_returns
            .as_slice(),
        observed_data
            .observed_5m_intraday_mean_absolute_returns
            .as_slice(),
    );

    (fit_tick + fit_15m + fit_5m) / 3.0
}

fn return_scale_horizon_fit(
    candidate_summary: JointRefinementReturnScaleSummary,
    observed_summary: JointRefinementReturnScaleSummary,
) -> f64 {
    let mean_abs_fit = log_ratio_distance(
        candidate_summary.mean_absolute_return,
        observed_summary.mean_absolute_return,
        JOINT_REFINEMENT_POSITIVE_METRIC_FLOOR,
    );

    let realized_variance_fit = log_ratio_distance(
        candidate_summary.realized_variance,
        observed_summary.realized_variance,
        JOINT_REFINEMENT_POSITIVE_METRIC_FLOOR,
    );

    let tail_fit = log_ratio_distance(
        candidate_summary.absolute_return_quantile_q90,
        observed_summary.absolute_return_quantile_q90,
        JOINT_REFINEMENT_POSITIVE_METRIC_FLOOR,
    );

    root_mean_square([mean_abs_fit, realized_variance_fit, tail_fit].as_slice())
}

fn missing_business_time_bucket_fit(available_summary: JointRefinementReturnScaleSummary) -> f64 {
    return_scale_horizon_fit(zero_liquidity_return_scale_summary(), available_summary)
}

fn zero_liquidity_return_scale_summary() -> JointRefinementReturnScaleSummary {
    JointRefinementReturnScaleSummary {
        mean_absolute_return: JOINT_REFINEMENT_POSITIVE_METRIC_FLOOR,
        realized_variance: JOINT_REFINEMENT_POSITIVE_METRIC_FLOOR,
        absolute_return_quantile_q90: JOINT_REFINEMENT_POSITIVE_METRIC_FLOOR,
    }
}

fn activity_shape_fit(
    seed_summary: &JointRefinementSeedSummary,
    observed_data: &JointRefinementObservedData,
) -> f64 {
    activity_shape_fit_for_counts(
        seed_summary.simulated_hourly_tick_counts.as_slice(),
        observed_data.observed_hourly_tick_counts.as_slice(),
    )
}

fn activity_shape_fit_for_counts(candidate_counts: &[f64], observed_counts: &[f64]) -> f64 {
    let profile_fit = normalized_profile_distance(candidate_counts, observed_counts);
    let path_fit = standardized_path_distance(candidate_counts, observed_counts);

    root_mean_square([profile_fit, path_fit].as_slice())
}

fn activity_level_fit(
    seed_summary: &JointRefinementSeedSummary,
    observed_data: &JointRefinementObservedData,
) -> f64 {
    activity_level_fit_for_counts(
        seed_summary.simulated_hourly_tick_counts.as_slice(),
        observed_data.observed_hourly_tick_counts.as_slice(),
    )
}

fn activity_level_fit_for_counts(candidate_counts: &[f64], observed_counts: &[f64]) -> f64 {
    let candidate_total = candidate_counts.iter().sum::<f64>();
    let observed_total = observed_counts.iter().sum::<f64>();

    let candidate_mean = candidate_total / candidate_counts.len().max(1) as f64;
    let observed_mean = observed_total / observed_counts.len().max(1) as f64;

    let candidate_peak = candidate_counts.iter().copied().fold(0.0, f64::max);
    let observed_peak = observed_counts.iter().copied().fold(0.0, f64::max);

    let total_fit = log_ratio_distance(
        candidate_total,
        observed_total,
        JOINT_REFINEMENT_POSITIVE_METRIC_FLOOR,
    );

    let mean_fit = log_ratio_distance(
        candidate_mean,
        observed_mean,
        JOINT_REFINEMENT_POSITIVE_METRIC_FLOOR,
    );

    let peak_fit = log_ratio_distance(
        candidate_peak,
        observed_peak,
        JOINT_REFINEMENT_POSITIVE_METRIC_FLOOR,
    );

    let path_fit = relative_rmse(
        candidate_counts,
        observed_counts,
        observed_mean.max(JOINT_REFINEMENT_POSITIVE_METRIC_FLOOR),
    );

    root_mean_square([total_fit, mean_fit, peak_fit, path_fit].as_slice())
}

fn rv_activity_coupling_fit(
    candidate_metric: crate::stages::joint_metrics::JointRefinementRollingVarianceActivityMetric,
    observed_metric: crate::stages::joint_metrics::JointRefinementRollingVarianceActivityMetric,
) -> f64 {
    root_mean_square(
        [
            (candidate_metric.correlation - observed_metric.correlation).abs(),
            (candidate_metric.standardized_path_distance
                - observed_metric.standardized_path_distance)
                .abs(),
        ]
        .as_slice(),
    )
}

fn build_joint_loss_evaluation(
    fitted_parameters: &JointRefinementStageFittedParameters,
    deterministic_evaluation: &JointRefinementDeterministicEvaluation,
    seed_summaries: &[JointRefinementSeedSummary],
    context: &JointRefinementContext,
) -> CalibrationLossEvaluation {
    let observed_data = &context.observed_data;

    let mut builder = CalibrationLossBuilder::new();

    add_likelihood_terms(&mut builder, deterministic_evaluation);
    add_domain_margin_priors(&mut builder, fitted_parameters, observed_data);

    if !seed_summaries.is_empty() {
        add_return_shape_terms(&mut builder, seed_summaries, context);
        add_return_scale_terms(&mut builder, seed_summaries, observed_data);
        add_business_time_return_scale_terms(&mut builder, seed_summaries, observed_data);
        add_intraday_return_level_terms(&mut builder, seed_summaries, observed_data);
        add_activity_terms(
            &mut builder,
            fitted_parameters,
            deterministic_evaluation,
            seed_summaries,
            observed_data,
        );
        add_rv_activity_coupling_terms(&mut builder, seed_summaries, observed_data);
        long_variance::add_long_variance_horizon_terms(&mut builder, seed_summaries, observed_data);
    }

    builder.build()
}

fn build_joint_coarse_search_loss_evaluation(
    fitted_parameters: &JointRefinementStageFittedParameters,
    deterministic_evaluation: &JointRefinementDeterministicEvaluation,
    seed_summaries: &[JointRefinementSeedSummary],
    context: &JointRefinementContext,
) -> CalibrationLossEvaluation {
    let observed_data = &context.observed_data;

    let mut builder = CalibrationLossBuilder::new();

    add_likelihood_terms(&mut builder, deterministic_evaluation);
    add_domain_margin_priors(&mut builder, fitted_parameters, observed_data);

    add_deterministic_activity_terms(
        &mut builder,
        fitted_parameters,
        deterministic_evaluation,
        observed_data,
    );

    if !seed_summaries.is_empty() {
        add_return_shape_terms(&mut builder, seed_summaries, context);
        add_coarse_return_scale_terms(&mut builder, seed_summaries, observed_data);
        add_business_time_return_scale_terms(&mut builder, seed_summaries, observed_data);
        add_coarse_intraday_return_level_terms(&mut builder, seed_summaries, observed_data);
        add_rv_activity_coupling_terms(&mut builder, seed_summaries, observed_data);
        long_variance::add_long_variance_horizon_terms(&mut builder, seed_summaries, observed_data);
    }

    builder.build()
}

fn add_likelihood_terms(
    builder: &mut CalibrationLossBuilder,
    deterministic_evaluation: &JointRefinementDeterministicEvaluation,
) {
    let likelihood_coordinates = [
        (
            "returns.nig_likelihood.tick",
            deterministic_evaluation.component_shifted_average_negative_log_likelihoods[0],
            deterministic_evaluation.component_shifted_average_negative_log_likelihood_variances[0],
        ),
        (
            "returns.nig_likelihood.1h",
            deterministic_evaluation.component_shifted_average_negative_log_likelihoods[1],
            deterministic_evaluation.component_shifted_average_negative_log_likelihood_variances[1],
        ),
        (
            "returns.nig_likelihood.1d",
            deterministic_evaluation.component_shifted_average_negative_log_likelihoods[2],
            deterministic_evaluation.component_shifted_average_negative_log_likelihood_variances[2],
        ),
    ];

    let coordinates = likelihood_coordinates
        .iter()
        .filter_map(
            |(term_id, shifted_average_negative_log_likelihood, variance)| {
                let variance = (*variance).filter(|value| value.is_finite() && *value >= 0.0)?;

                shifted_average_negative_log_likelihood
                    .is_finite()
                    .then_some((
                        ObjectiveCoordinateInput {
                            coordinate_id: (*term_id).to_string(),
                            observed_value: 0.0,
                            model_value: *shifted_average_negative_log_likelihood,
                        },
                        variance,
                    ))
            },
        )
        .collect::<Vec<_>>();

    if coordinates.is_empty() {
        return;
    }

    let coordinate_inputs = coordinates
        .iter()
        .map(|(coordinate, _)| coordinate.clone())
        .collect::<Vec<_>>();
    let variances = coordinates
        .iter()
        .map(|(_, variance)| *variance)
        .collect::<Vec<_>>();

    let Some(covariance_rows) = diagonal_covariance(variances.as_slice()) else {
        return;
    };

    let Some(family) = evaluate_objective_family(ObjectiveFamilyInput {
        family_id: "returns.nig_likelihood".to_string(),
        coordinates: coordinate_inputs,
        covariance_rows,
    }) else {
        return;
    };

    builder.add_objective_family_with_term_kind(family, CalibrationLossTermKind::ExactLikelihood);
}

fn add_objective_family_from_seed_vectors(
    builder: &mut CalibrationLossBuilder,
    family_id: impl Into<String>,
    coordinate_ids: Vec<String>,
    observed_vector: Vec<f64>,
    observed_covariance_rows: Vec<Vec<f64>>,
    model_seed_vectors: Vec<Vec<f64>>,
) {
    let Some(model_vector) = mean_vector(model_seed_vectors.as_slice()) else {
        return;
    };

    let model_covariance_rows = model_mean_covariance(model_seed_vectors.as_slice());

    add_objective_family_from_model_vector(
        builder,
        family_id,
        coordinate_ids,
        observed_vector,
        observed_covariance_rows,
        model_vector,
        model_covariance_rows,
    );
}

fn add_objective_family_from_model_vector(
    builder: &mut CalibrationLossBuilder,
    family_id: impl Into<String>,
    coordinate_ids: Vec<String>,
    observed_vector: Vec<f64>,
    observed_covariance_rows: Vec<Vec<f64>>,
    model_vector: Vec<f64>,
    model_covariance_rows: Option<Vec<Vec<f64>>>,
) {
    if coordinate_ids.is_empty()
        || coordinate_ids.len() != observed_vector.len()
        || coordinate_ids.len() != model_vector.len()
    {
        return;
    }

    let mut covariance_sources = vec![observed_covariance_rows];
    if let Some(model_covariance_rows) = model_covariance_rows {
        covariance_sources.push(model_covariance_rows);
    }

    let Some(covariance_rows) = sum_covariance_matrices(covariance_sources.as_slice()) else {
        return;
    };

    let coordinates = coordinate_ids
        .into_iter()
        .zip(observed_vector)
        .zip(model_vector)
        .map(
            |((coordinate_id, observed_value), model_value)| ObjectiveCoordinateInput {
                coordinate_id,
                observed_value,
                model_value,
            },
        )
        .collect::<Vec<_>>();

    let Some(family) = evaluate_objective_family(ObjectiveFamilyInput {
        family_id: family_id.into(),
        coordinates,
        covariance_rows,
    }) else {
        return;
    };

    builder.add_objective_family(family);
}

fn diagonal_covariance_from_standard_errors(standard_errors: &[f64]) -> Option<Vec<Vec<f64>>> {
    let variances = standard_errors
        .iter()
        .map(|standard_error| {
            (standard_error.is_finite() && *standard_error >= 0.0)
                .then_some(standard_error * standard_error)
        })
        .collect::<Option<Vec<_>>>()?;

    diagonal_covariance(variances.as_slice())
}

fn add_return_shape_terms(
    builder: &mut CalibrationLossBuilder,
    seed_summaries: &[JointRefinementSeedSummary],
    context: &JointRefinementContext,
) {
    return_shape::add_return_shape_terms(builder, seed_summaries, context);
}

struct ReturnShapeHorizonTermInput<'a> {
    horizon: JointRefinementReturnShapeHorizon,
    observed_values: &'a [f64],
    simulated_values_by_seed: Vec<&'a [f64]>,
}

fn return_shape_horizon_term_inputs<'a>(
    seed_summaries: &'a [JointRefinementSeedSummary],
    observed_data: &'a JointRefinementObservedData,
) -> Vec<ReturnShapeHorizonTermInput<'a>> {
    vec![
        return_shape_horizon_term_input(
            JointRefinementReturnShapeHorizon::Tick,
            observed_data.observed_tick_shape_returns.as_slice(),
            seed_summaries,
            |summary| summary.simulated_tick_shape_returns.as_slice(),
        ),
        return_shape_horizon_term_input(
            JointRefinementReturnShapeHorizon::FiveMinute,
            observed_data.observed_5m_shape_returns.as_slice(),
            seed_summaries,
            |summary| summary.simulated_5m_shape_returns.as_slice(),
        ),
        return_shape_horizon_term_input(
            JointRefinementReturnShapeHorizon::FifteenMinute,
            observed_data.observed_15m_shape_returns.as_slice(),
            seed_summaries,
            |summary| summary.simulated_15m_shape_returns.as_slice(),
        ),
        return_shape_horizon_term_input(
            JointRefinementReturnShapeHorizon::OneHour,
            observed_data.observed_1h_shape_returns.as_slice(),
            seed_summaries,
            |summary| summary.simulated_1h_shape_returns.as_slice(),
        ),
        return_shape_horizon_term_input(
            JointRefinementReturnShapeHorizon::OneDay,
            observed_data.observed_1d_shape_returns.as_slice(),
            seed_summaries,
            |summary| summary.simulated_1d_shape_returns.as_slice(),
        ),
        return_shape_horizon_term_input(
            JointRefinementReturnShapeHorizon::OneWeek,
            observed_data.observed_1w_shape_returns.as_slice(),
            seed_summaries,
            |summary| summary.simulated_1w_shape_returns.as_slice(),
        ),
    ]
}

fn return_shape_horizon_term_input<'a, F>(
    horizon: JointRefinementReturnShapeHorizon,
    observed_values: &'a [f64],
    seed_summaries: &'a [JointRefinementSeedSummary],
    simulated_values_for_seed: F,
) -> ReturnShapeHorizonTermInput<'a>
where
    F: Fn(&'a JointRefinementSeedSummary) -> &'a [f64],
{
    ReturnShapeHorizonTermInput {
        horizon,
        observed_values,
        simulated_values_by_seed: seed_summaries
            .iter()
            .map(simulated_values_for_seed)
            .collect(),
    }
}

fn cached_observed_return_shape_statistic_estimates(
    horizon: JointRefinementReturnShapeHorizon,
    temporal_block_count: usize,
    observed_values: &[f64],
    context: &JointRefinementContext,
) -> Arc<[JointRefinementReturnShapeStatisticEstimate]> {
    let cache_key = JointRefinementObservedReturnShapeStatisticCacheKey {
        horizon,
        temporal_block_count,
        metric_names: Vec::new(),
    };

    if let Ok(cache_guard) = context.observed_return_shape_statistic_cache.lock()
        && let Some(cached_estimates) = cache_guard.get(&cache_key)
    {
        return Arc::clone(cached_estimates);
    }

    let estimates = Arc::from(return_shape_statistic_estimates(
        observed_values,
        temporal_block_count,
    ));

    if let Ok(mut cache_guard) = context.observed_return_shape_statistic_cache.lock() {
        cache_guard.insert(cache_key, Arc::clone(&estimates));
    }

    estimates
}

fn cached_observed_return_shape_covariance(
    horizon: JointRefinementReturnShapeHorizon,
    temporal_block_count: usize,
    observed_values: &[f64],
    metric_names: &[&'static str],
    observed_vector: &[f64],
    context: &JointRefinementContext,
) -> Option<Arc<Vec<Vec<f64>>>> {
    let cache_key = JointRefinementObservedReturnShapeStatisticCacheKey {
        horizon,
        temporal_block_count,
        metric_names: metric_names.to_vec(),
    };

    if let Ok(cache_guard) = context.observed_return_shape_covariance_cache.lock()
        && let Some(cached_covariance) = cache_guard.get(&cache_key)
    {
        return Some(Arc::clone(cached_covariance));
    }

    let covariance = Arc::new(return_shape_delete_block_covariance(
        observed_values,
        temporal_block_count,
        metric_names,
        observed_vector,
    )?);

    if let Ok(mut cache_guard) = context.observed_return_shape_covariance_cache.lock() {
        cache_guard.insert(cache_key, Arc::clone(&covariance));
    }

    Some(covariance)
}

fn return_shape_horizon_label(horizon: JointRefinementReturnShapeHorizon) -> &'static str {
    match horizon {
        JointRefinementReturnShapeHorizon::Tick => "tick",
        JointRefinementReturnShapeHorizon::FiveMinute => "5m",
        JointRefinementReturnShapeHorizon::FifteenMinute => "15m",
        JointRefinementReturnShapeHorizon::OneHour => "1h",
        JointRefinementReturnShapeHorizon::OneDay => "1d",
        JointRefinementReturnShapeHorizon::OneWeek => "1w",
    }
}

fn add_return_scale_terms(
    builder: &mut CalibrationLossBuilder,
    seed_summaries: &[JointRefinementSeedSummary],
    observed_data: &JointRefinementObservedData,
) {
    let tick_summaries = seed_summaries
        .iter()
        .map(|summary| summary.simulated_tick_return_scale_summary)
        .collect::<Vec<_>>();
    let one_hour_summaries = seed_summaries
        .iter()
        .map(|summary| summary.simulated_1h_return_scale_summary)
        .collect::<Vec<_>>();
    let one_day_summaries = seed_summaries
        .iter()
        .map(|summary| summary.simulated_1d_return_scale_summary)
        .collect::<Vec<_>>();

    add_return_scale_mechanism_terms(
        builder,
        &[
            ReturnScaleSummaryTermInput {
                horizon_label: "tick",
                observed_summary: observed_data.observed_tick_return_scale_summary,
                model_summaries: tick_summaries.as_slice(),
                observed_sample_count: observed_data.observed_tick_shape_returns.len(),
            },
            ReturnScaleSummaryTermInput {
                horizon_label: "1h",
                observed_summary: observed_data.observed_1h_return_scale_summary,
                model_summaries: one_hour_summaries.as_slice(),
                observed_sample_count: observed_data.observed_1h_intervals.len(),
            },
            ReturnScaleSummaryTermInput {
                horizon_label: "1d",
                observed_summary: observed_data.observed_1d_return_scale_summary,
                model_summaries: one_day_summaries.as_slice(),
                observed_sample_count: observed_data.observed_1d_intervals.len(),
            },
        ],
        "return_scale",
    );
}

fn add_coarse_return_scale_terms(
    builder: &mut CalibrationLossBuilder,
    seed_summaries: &[JointRefinementSeedSummary],
    observed_data: &JointRefinementObservedData,
) {
    let one_hour_summaries = seed_summaries
        .iter()
        .map(|summary| summary.simulated_1h_return_scale_summary)
        .collect::<Vec<_>>();
    let one_day_summaries = seed_summaries
        .iter()
        .map(|summary| summary.simulated_1d_return_scale_summary)
        .collect::<Vec<_>>();

    add_return_scale_mechanism_terms(
        builder,
        &[
            ReturnScaleSummaryTermInput {
                horizon_label: "1h",
                observed_summary: observed_data.observed_1h_return_scale_summary,
                model_summaries: one_hour_summaries.as_slice(),
                observed_sample_count: observed_data.observed_1h_intervals.len(),
            },
            ReturnScaleSummaryTermInput {
                horizon_label: "1d",
                observed_summary: observed_data.observed_1d_return_scale_summary,
                model_summaries: one_day_summaries.as_slice(),
                observed_sample_count: observed_data.observed_1d_intervals.len(),
            },
        ],
        "return_scale",
    );
}

fn add_business_time_return_scale_terms(
    builder: &mut CalibrationLossBuilder,
    seed_summaries: &[JointRefinementSeedSummary],
    observed_data: &JointRefinementObservedData,
) {
    add_business_time_return_scale_horizon_terms(
        builder,
        "5m",
        observed_data.observed_5m_business_time_return_scale_summaries,
        seed_summaries
            .iter()
            .map(|summary| summary.simulated_5m_business_time_return_scale_summaries)
            .collect::<Vec<_>>()
            .as_slice(),
        observed_data.observed_5m_intervals.len(),
    );

    add_business_time_return_scale_horizon_terms(
        builder,
        "15m",
        observed_data.observed_15m_business_time_return_scale_summaries,
        seed_summaries
            .iter()
            .map(|summary| summary.simulated_15m_business_time_return_scale_summaries)
            .collect::<Vec<_>>()
            .as_slice(),
        observed_data.observed_15m_intervals.len(),
    );
}

fn add_intraday_return_level_terms(
    builder: &mut CalibrationLossBuilder,
    seed_summaries: &[JointRefinementSeedSummary],
    observed_data: &JointRefinementObservedData,
) {
    add_intraday_profile_terms(
        builder,
        "tick",
        observed_data
            .observed_tick_intraday_mean_absolute_returns
            .as_slice(),
        seed_summaries
            .iter()
            .map(|summary| {
                summary
                    .simulated_tick_intraday_mean_absolute_returns
                    .as_slice()
            })
            .collect::<Vec<_>>()
            .as_slice(),
    );

    add_intraday_profile_terms(
        builder,
        "5m",
        observed_data
            .observed_5m_intraday_mean_absolute_returns
            .as_slice(),
        seed_summaries
            .iter()
            .map(|summary| {
                summary
                    .simulated_5m_intraday_mean_absolute_returns
                    .as_slice()
            })
            .collect::<Vec<_>>()
            .as_slice(),
    );

    add_intraday_profile_terms(
        builder,
        "15m",
        observed_data
            .observed_15m_intraday_mean_absolute_returns
            .as_slice(),
        seed_summaries
            .iter()
            .map(|summary| {
                summary
                    .simulated_15m_intraday_mean_absolute_returns
                    .as_slice()
            })
            .collect::<Vec<_>>()
            .as_slice(),
    );
}

fn add_coarse_intraday_return_level_terms(
    builder: &mut CalibrationLossBuilder,
    seed_summaries: &[JointRefinementSeedSummary],
    observed_data: &JointRefinementObservedData,
) {
    add_intraday_profile_terms(
        builder,
        "5m",
        observed_data
            .observed_5m_intraday_mean_absolute_returns
            .as_slice(),
        seed_summaries
            .iter()
            .map(|summary| {
                summary
                    .simulated_5m_intraday_mean_absolute_returns
                    .as_slice()
            })
            .collect::<Vec<_>>()
            .as_slice(),
    );

    add_intraday_profile_terms(
        builder,
        "15m",
        observed_data
            .observed_15m_intraday_mean_absolute_returns
            .as_slice(),
        seed_summaries
            .iter()
            .map(|summary| {
                summary
                    .simulated_15m_intraday_mean_absolute_returns
                    .as_slice()
            })
            .collect::<Vec<_>>()
            .as_slice(),
    );
}

fn add_activity_terms(
    builder: &mut CalibrationLossBuilder,
    fitted_parameters: &JointRefinementStageFittedParameters,
    deterministic_evaluation: &JointRefinementDeterministicEvaluation,
    seed_summaries: &[JointRefinementSeedSummary],
    observed_data: &JointRefinementObservedData,
) {
    let simulated_hourly_counts = mean_profile_by_seed(
        seed_summaries
            .iter()
            .map(|summary| summary.simulated_hourly_tick_counts.as_slice())
            .collect::<Vec<_>>()
            .as_slice(),
    );

    let fano_factor = hawkes_count_fano_factor(fitted_parameters).unwrap_or(1.0);

    let model_hourly_counts = deterministic_evaluation
        .expected_hourly_tick_counts
        .as_deref()
        .unwrap_or(simulated_hourly_counts.as_slice());

    add_activity_level_terms(
        builder,
        observed_data.observed_hourly_tick_counts.as_slice(),
        model_hourly_counts,
        fano_factor,
    );

    add_activity_shape_terms(
        builder,
        observed_data.observed_hourly_tick_counts.as_slice(),
        model_hourly_counts,
    );
}

fn add_deterministic_activity_terms(
    builder: &mut CalibrationLossBuilder,
    fitted_parameters: &JointRefinementStageFittedParameters,
    deterministic_evaluation: &JointRefinementDeterministicEvaluation,
    observed_data: &JointRefinementObservedData,
) {
    let Some(model_hourly_counts) = deterministic_evaluation
        .expected_hourly_tick_counts
        .as_deref()
    else {
        return;
    };

    let fano_factor = hawkes_count_fano_factor(fitted_parameters).unwrap_or(1.0);

    add_activity_level_terms(
        builder,
        observed_data.observed_hourly_tick_counts.as_slice(),
        model_hourly_counts,
        fano_factor,
    );

    add_activity_shape_terms(
        builder,
        observed_data.observed_hourly_tick_counts.as_slice(),
        model_hourly_counts,
    );
}

fn add_rv_activity_coupling_terms(
    builder: &mut CalibrationLossBuilder,
    seed_summaries: &[JointRefinementSeedSummary],
    observed_data: &JointRefinementObservedData,
) {
    let observed_metric = observed_data.observed_15m_rv_activity_metric;

    let sample_count = observed_data.observed_15m_intervals.len();

    let Some(observed_covariance_rows) = diagonal_covariance_from_standard_errors(
        [
            unit_scale_standard_error(sample_count),
            unit_scale_standard_error(sample_count),
        ]
        .as_slice(),
    ) else {
        return;
    };

    let model_seed_vectors = seed_summaries
        .iter()
        .map(|summary| {
            vec![
                summary.simulated_15m_rv_activity_metric.correlation,
                summary
                    .simulated_15m_rv_activity_metric
                    .standardized_path_distance,
            ]
        })
        .collect::<Vec<_>>();

    add_objective_family_from_seed_vectors(
        builder,
        "rv_activity_coupling",
        vec![
            "rv_activity_coupling.correlation".to_string(),
            "rv_activity_coupling.standardized_path_distance".to_string(),
        ],
        vec![
            observed_metric.correlation,
            observed_metric.standardized_path_distance,
        ],
        observed_covariance_rows,
        model_seed_vectors,
    );
}

fn add_domain_margin_priors(
    builder: &mut CalibrationLossBuilder,
    fitted_parameters: &JointRefinementStageFittedParameters,
    observed_data: &JointRefinementObservedData,
) {
    if let Some(runtime_effective_branching_ratio) =
        hawkes_runtime_effective_branching_ratio(fitted_parameters)
    {
        let target_margin = (observed_data.observed_hourly_tick_counts.len().max(1) as f64)
            .recip()
            .sqrt();

        builder.add_domain_margin_constraint(
            "domain_margin.hawkes_stationarity",
            1.0 - runtime_effective_branching_ratio,
            target_margin,
        );
    }

    let nig_configuration = fitted_parameters.pricing_nig_shared_plus_shift_configuration();

    if let Ok(driver_parameters) = nig_configuration.derive_driver_parameters() {
        let buy_report = driver_parameters
            .buy_side_parameters
            .characteristic_exponent_with_guardrails(ComplexScalar::new(0.0, 0.0));

        builder.add_domain_margin_constraint(
            "domain_margin.nig_buy_alpha_beta",
            buy_report.alpha_beta_margin,
            buy_report.alpha_beta_margin_warning_threshold,
        );

        let sell_report = driver_parameters
            .sell_side_parameters
            .characteristic_exponent_with_guardrails(ComplexScalar::new(0.0, 0.0));

        builder.add_domain_margin_constraint(
            "domain_margin.nig_sell_alpha_beta",
            sell_report.alpha_beta_margin,
            sell_report.alpha_beta_margin_warning_threshold,
        );
    }
}

fn return_shape_statistic_estimates(
    values: &[f64],
    temporal_block_count: usize,
) -> Vec<JointRefinementReturnShapeStatisticEstimate> {
    let sorted_values = sorted_finite_values(values);
    if sorted_values.is_empty() {
        return Vec::new();
    }

    let finite_values = sorted_values.as_slice();

    let effective_sample_count =
        return_shape_effective_sample_count(finite_values.len(), temporal_block_count);

    let mut estimates =
        base_return_shape_statistic_estimates(finite_values, effective_sample_count);

    let block_standard_errors =
        return_shape_temporal_block_standard_errors(values, temporal_block_count);

    for estimate in estimates.iter_mut() {
        if let Some(block_standard_error) = block_standard_errors.get(estimate.metric_name) {
            estimate.standard_error = estimate.standard_error.max(*block_standard_error);
        }
    }

    estimates
}

fn base_return_shape_statistic_estimates(
    finite_values: &[f64],
    effective_sample_count: usize,
) -> Vec<JointRefinementReturnShapeStatisticEstimate> {
    let sample_standard_deviation_value = sample_standard_deviation(finite_values);

    let winsorized_volatility = winsorized_sample_standard_deviation_from_sorted(
        finite_values,
        RETURN_SHAPE_WINSORIZED_VOLATILITY_LOWER_QUANTILE,
        RETURN_SHAPE_WINSORIZED_VOLATILITY_UPPER_QUANTILE,
    );

    let mut estimates = Vec::new();

    estimates.push(JointRefinementReturnShapeStatisticEstimate {
        metric_name: "mean",
        value: arithmetic_mean_or_zero(finite_values),
        standard_error: standard_error_from_scale(
            sample_standard_deviation_value,
            effective_sample_count,
        ),
    });

    estimates.push(JointRefinementReturnShapeStatisticEstimate {
        metric_name: "winsorized_volatility_q05_q95",
        value: winsorized_volatility,
        standard_error: standard_deviation_standard_error(
            winsorized_volatility,
            effective_sample_count,
        ),
    });

    for (metric_name, quantile_level) in [
        ("q01", 0.01),
        ("q05", 0.05),
        ("q25", 0.25),
        ("median", 0.50),
        ("q75", 0.75),
        ("q95", 0.95),
        ("q99", 0.99),
    ] {
        estimates.push(JointRefinementReturnShapeStatisticEstimate {
            metric_name,
            value: compute_sorted_quantile_or_zero(finite_values, quantile_level),
            standard_error: quantile_standard_error(
                finite_values,
                quantile_level,
                effective_sample_count,
            ),
        });
    }

    let absolute_values = sorted_finite_values(
        finite_values
            .iter()
            .copied()
            .map(f64::abs)
            .collect::<Vec<_>>()
            .as_slice(),
    );

    estimates.push(JointRefinementReturnShapeStatisticEstimate {
        metric_name: "mean_abs",
        value: arithmetic_mean_or_zero(absolute_values.as_slice()),
        standard_error: standard_error_from_scale(
            sample_standard_deviation(absolute_values.as_slice()),
            effective_sample_count,
        ),
    });

    estimates.push(JointRefinementReturnShapeStatisticEstimate {
        metric_name: "median_abs",
        value: compute_sorted_quantile_or_zero(absolute_values.as_slice(), 0.50),
        standard_error: quantile_standard_error(
            absolute_values.as_slice(),
            0.50,
            effective_sample_count,
        ),
    });

    estimates.push(JointRefinementReturnShapeStatisticEstimate {
        metric_name: "bowley_skewness",
        value: bowley_skewness_or_zero(finite_values),
        standard_error: bowley_skewness_standard_error(finite_values, effective_sample_count),
    });

    if let Some(excess_kurtosis) = sample_adjusted_excess_kurtosis(finite_values)
        && let Some(standard_error) =
            adjusted_shape_jackknife_standard_errors(finite_values).excess_kurtosis
    {
        estimates.push(JointRefinementReturnShapeStatisticEstimate {
            metric_name: "excess_kurtosis",
            value: excess_kurtosis,
            standard_error,
        });
    }

    if let Some(lambda2) = l_moment_lambda2_from_sorted(finite_values) {
        estimates.push(JointRefinementReturnShapeStatisticEstimate {
            metric_name: "l_moment_lambda2",
            value: lambda2,
            standard_error: standard_error_from_scale(
                sample_standard_deviation_value,
                effective_sample_count,
            ),
        });
    }

    if let Some(tau3) =
        l_moment_tau3_from_sorted(finite_values, JOINT_REFINEMENT_POSITIVE_METRIC_FLOOR)
    {
        estimates.push(JointRefinementReturnShapeStatisticEstimate {
            metric_name: "l_moment_tau3",
            value: tau3,
            standard_error: unit_scale_standard_error(effective_sample_count),
        });
    }

    if let Some(tau4) =
        l_moment_tau4_from_sorted(finite_values, JOINT_REFINEMENT_POSITIVE_METRIC_FLOOR)
    {
        estimates.push(JointRefinementReturnShapeStatisticEstimate {
            metric_name: "l_moment_tau4",
            value: tau4,
            standard_error: unit_scale_standard_error(effective_sample_count),
        });
    }

    estimates
}

fn return_shape_temporal_block_standard_errors(
    values: &[f64],
    temporal_block_count: usize,
) -> BTreeMap<&'static str, f64> {
    let finite_values = values
        .iter()
        .copied()
        .filter(|value| value.is_finite())
        .collect::<Vec<_>>();

    let block_count =
        return_shape_effective_sample_count(finite_values.len(), temporal_block_count);
    if block_count < 2 {
        return BTreeMap::new();
    }

    let mut block_values_by_metric = BTreeMap::<&'static str, Vec<f64>>::new();

    for block_index in 0..block_count {
        let block_start = block_index
            .checked_mul(finite_values.len())
            .unwrap_or(finite_values.len())
            / block_count;

        let block_end = (block_index + 1)
            .checked_mul(finite_values.len())
            .unwrap_or(finite_values.len())
            / block_count;

        if block_end <= block_start {
            continue;
        }

        let sorted_block_values = sorted_finite_values(&finite_values[block_start..block_end]);

        let block_estimates = base_return_shape_statistic_estimates(
            sorted_block_values.as_slice(),
            block_end - block_start,
        );

        for block_estimate in block_estimates {
            if block_estimate.value.is_finite() {
                block_values_by_metric
                    .entry(block_estimate.metric_name)
                    .or_default()
                    .push(block_estimate.value);
            }
        }
    }

    block_values_by_metric
        .into_iter()
        .filter_map(|(metric_name, block_values)| {
            if block_values.len() < 2 {
                return None;
            }

            let standard_error = sample_standard_deviation(block_values.as_slice())
                / (block_values.len() as f64).sqrt();

            (standard_error.is_finite() && standard_error > 0.0)
                .then_some((metric_name, standard_error))
        })
        .collect()
}

fn return_shape_effective_sample_count(value_count: usize, temporal_block_count: usize) -> usize {
    // Shape diagnostics compare clustered market-time distributions. The effective
    // independent evidence is bounded by temporal blocks, while sparse horizons
    // cannot contribute more observations than they contain.
    value_count.min(temporal_block_count.max(1))
}

fn statistic_estimate_by_name(
    estimates: &[JointRefinementReturnShapeStatisticEstimate],
    metric_name: &str,
) -> Option<JointRefinementReturnShapeStatisticEstimate> {
    estimates
        .iter()
        .copied()
        .find(|estimate| estimate.metric_name == metric_name)
}

fn estimate_vector_by_metric_names(
    estimates: &[JointRefinementReturnShapeStatisticEstimate],
    metric_names: &[&'static str],
) -> Option<Vec<f64>> {
    metric_names
        .iter()
        .map(|metric_name| {
            statistic_estimate_by_name(estimates, metric_name)
                .map(|estimate| estimate.value)
                .filter(|value| value.is_finite())
        })
        .collect()
}

fn return_shape_delete_block_covariance(
    values: &[f64],
    temporal_block_count: usize,
    metric_names: &[&'static str],
    full_sample_vector: &[f64],
) -> Option<Vec<Vec<f64>>> {
    if metric_names.is_empty() {
        return None;
    }

    let delete_block_vectors = return_shape_delete_block_vectors(
        values,
        temporal_block_count,
        |retained_values, retained_effective_sample_count| {
            let retained_sorted_values = sorted_finite_values(retained_values);
            let retained_estimates = base_return_shape_statistic_estimates(
                retained_sorted_values.as_slice(),
                retained_effective_sample_count,
            );

            estimate_vector_by_metric_names(retained_estimates.as_slice(), metric_names)
        },
    )?;

    covariance_from_delete_block_vectors(full_sample_vector, delete_block_vectors.as_slice())
}

pub(super) fn return_shape_delete_block_vectors<F>(
    values: &[f64],
    temporal_block_count: usize,
    mut retained_vector: F,
) -> Option<Vec<Vec<f64>>>
where
    F: FnMut(&[f64], usize) -> Option<Vec<f64>>,
{
    let finite_values = values
        .iter()
        .copied()
        .filter(|value| value.is_finite())
        .collect::<Vec<_>>();

    let block_count =
        return_shape_effective_sample_count(finite_values.len(), temporal_block_count);
    if block_count < 2 {
        return None;
    }

    let mut delete_block_vectors = Vec::with_capacity(block_count);

    for block_position in 0..block_count {
        let block_start = block_position
            .checked_mul(finite_values.len())
            .unwrap_or(finite_values.len())
            / block_count;

        let block_end = (block_position + 1)
            .checked_mul(finite_values.len())
            .unwrap_or(finite_values.len())
            / block_count;

        if block_end <= block_start || block_end - block_start >= finite_values.len() {
            continue;
        }

        let mut retained_values =
            Vec::with_capacity(finite_values.len() - (block_end - block_start));
        retained_values.extend_from_slice(&finite_values[..block_start]);
        retained_values.extend_from_slice(&finite_values[block_end..]);

        let retained_effective_sample_count =
            return_shape_effective_sample_count(retained_values.len(), block_count - 1);

        if let Some(vector) =
            retained_vector(retained_values.as_slice(), retained_effective_sample_count)
        {
            delete_block_vectors.push(vector);
        }
    }

    Some(delete_block_vectors)
}

struct ReturnScaleSummaryTermInput<'a> {
    horizon_label: &'static str,
    observed_summary: JointRefinementReturnScaleSummary,
    model_summaries: &'a [JointRefinementReturnScaleSummary],
    observed_sample_count: usize,
}

fn add_return_scale_mechanism_terms(
    builder: &mut CalibrationLossBuilder,
    inputs: &[ReturnScaleSummaryTermInput<'_>],
    family_id: &str,
) {
    let mut coordinate_ids = Vec::new();
    let mut observed_vector = Vec::new();
    let mut observed_covariance_blocks = Vec::new();
    let mut model_seed_vectors: Option<Vec<Vec<f64>>> = None;

    for input in inputs {
        let block_observed_vector = return_scale_summary_vector(input.observed_summary);
        let observed_standard_errors = block_observed_vector
            .iter()
            .map(|value| standard_error_from_positive_value(*value, input.observed_sample_count))
            .collect::<Vec<_>>();

        let Some(observed_covariance_rows) =
            diagonal_covariance_from_standard_errors(observed_standard_errors.as_slice())
        else {
            continue;
        };

        let block_model_seed_vectors = input
            .model_summaries
            .iter()
            .copied()
            .map(return_scale_summary_vector)
            .collect::<Vec<_>>();
        if block_model_seed_vectors.is_empty() {
            continue;
        }

        if let Some(model_seed_vectors) = model_seed_vectors.as_mut() {
            if model_seed_vectors.len() != block_model_seed_vectors.len() {
                continue;
            }

            for (target_vector, source_vector) in model_seed_vectors
                .iter_mut()
                .zip(block_model_seed_vectors.iter())
            {
                target_vector.extend_from_slice(source_vector.as_slice());
            }
        } else {
            model_seed_vectors = Some(block_model_seed_vectors);
        }

        coordinate_ids.extend(
            return_scale_summary_metric_names()
                .iter()
                .map(|metric_name| format!("{family_id}.{}.{}", input.horizon_label, metric_name)),
        );
        observed_vector.extend_from_slice(block_observed_vector.as_slice());
        observed_covariance_blocks.push(observed_covariance_rows);
    }

    let Some(model_seed_vectors) = model_seed_vectors else {
        return;
    };

    let Some(observed_covariance_rows) =
        block_diagonal_covariance(observed_covariance_blocks.as_slice())
    else {
        return;
    };

    add_objective_family_from_seed_vectors(
        builder,
        family_id.to_string(),
        coordinate_ids,
        observed_vector,
        observed_covariance_rows,
        model_seed_vectors,
    );
}

fn add_return_scale_summary_terms(
    builder: &mut CalibrationLossBuilder,
    horizon_label: &str,
    observed_summary: JointRefinementReturnScaleSummary,
    model_summaries: &[JointRefinementReturnScaleSummary],
    observed_sample_count: usize,
    family_id: &str,
) {
    let observed_vector = return_scale_summary_vector(observed_summary);
    let observed_standard_errors = observed_vector
        .iter()
        .map(|value| standard_error_from_positive_value(*value, observed_sample_count))
        .collect::<Vec<_>>();

    let Some(observed_covariance_rows) =
        diagonal_covariance_from_standard_errors(observed_standard_errors.as_slice())
    else {
        return;
    };

    let model_seed_vectors = model_summaries
        .iter()
        .copied()
        .map(return_scale_summary_vector)
        .collect::<Vec<_>>();

    let family_id = format!("{family_id}.{horizon_label}");
    let coordinate_ids = return_scale_summary_metric_names()
        .into_iter()
        .map(|metric_name| format!("{family_id}.{metric_name}"))
        .collect::<Vec<_>>();

    add_objective_family_from_seed_vectors(
        builder,
        family_id,
        coordinate_ids,
        observed_vector,
        observed_covariance_rows,
        model_seed_vectors,
    );
}

fn return_scale_summary_vector(summary: JointRefinementReturnScaleSummary) -> Vec<f64> {
    vec![
        summary.mean_absolute_return,
        summary.realized_variance,
        summary.absolute_return_quantile_q90,
    ]
}

fn return_scale_summary_metric_names() -> [&'static str; 3] {
    [
        "mean_absolute_return",
        "realized_variance",
        "absolute_return_quantile_q90",
    ]
}

fn add_business_time_return_scale_horizon_terms(
    builder: &mut CalibrationLossBuilder,
    horizon_label: &str,
    observed_summaries: JointRefinementBusinessTimeBucketReturnScaleSummaries,
    simulated_summaries_by_seed: &[JointRefinementBusinessTimeBucketReturnScaleSummaries],
    observed_sample_count: usize,
) {
    for (bucket_index, observed_summary) in observed_summaries.bucket_summaries.iter().enumerate() {
        let Some(observed_summary) = observed_summary else {
            continue;
        };

        let model_summaries = simulated_summaries_by_seed
            .iter()
            .filter_map(|summaries| summaries.bucket_summaries[bucket_index])
            .collect::<Vec<_>>();

        add_return_scale_summary_terms(
            builder,
            format!("{horizon_label}.business_time_bucket_{bucket_index}").as_str(),
            *observed_summary,
            model_summaries.as_slice(),
            observed_sample_count
                .checked_div(observed_summaries.bucket_summaries.len().max(1))
                .unwrap_or(0)
                .max(1),
            "business_time_return_scale",
        );
    }
}

fn add_intraday_profile_terms(
    builder: &mut CalibrationLossBuilder,
    horizon_label: &str,
    observed_profile: &[f64],
    simulated_profiles_by_seed: &[&[f64]],
) {
    let Some(profile_length) = simulated_profiles_by_seed
        .iter()
        .map(|profile| profile.len())
        .chain(std::iter::once(observed_profile.len()))
        .min()
    else {
        return;
    };

    if profile_length == 0 {
        return;
    }

    let observed_vector = observed_profile[..profile_length].to_vec();
    let observed_standard_errors = observed_vector
        .iter()
        .map(|value| standard_error_from_positive_value(*value, profile_length))
        .collect::<Vec<_>>();

    let Some(observed_covariance_rows) =
        diagonal_covariance_from_standard_errors(observed_standard_errors.as_slice())
    else {
        return;
    };

    let model_seed_vectors = simulated_profiles_by_seed
        .iter()
        .map(|profile| profile[..profile_length].to_vec())
        .collect::<Vec<_>>();

    let family_id = format!("intraday_return_level.{horizon_label}");
    let coordinate_ids = (0..profile_length)
        .map(|hour_index| format!("{family_id}.hour_{hour_index:02}"))
        .collect::<Vec<_>>();

    add_objective_family_from_seed_vectors(
        builder,
        family_id,
        coordinate_ids,
        observed_vector,
        observed_covariance_rows,
        model_seed_vectors,
    );
}

fn add_activity_level_terms(
    builder: &mut CalibrationLossBuilder,
    observed_counts: &[f64],
    model_counts: &[f64],
    fano_factor: f64,
) {
    let observed_total = observed_counts.iter().sum::<f64>();
    let model_total = model_counts.iter().sum::<f64>();
    let count = observed_counts.len().min(model_counts.len()).max(1);

    let observed_standard_errors = activity_level_standard_errors(observed_counts, fano_factor);
    let model_standard_errors = activity_level_standard_errors(model_counts, fano_factor);

    let observed_zero_count_share = zero_count_share(observed_counts);
    let model_zero_count_share = zero_count_share(model_counts);

    let observed_vector = vec![
        observed_total,
        observed_total / count as f64,
        coefficient_of_variation(observed_counts),
        observed_zero_count_share,
    ];
    let model_vector = vec![
        model_total,
        model_total / count as f64,
        coefficient_of_variation(model_counts),
        model_zero_count_share,
    ];

    let Some(observed_covariance_rows) = diagonal_covariance_from_standard_errors(
        [
            observed_standard_errors.total_count,
            observed_standard_errors.mean_hourly_count,
            unit_scale_standard_error(count),
            binomial_share_standard_error(observed_zero_count_share, count),
        ]
        .as_slice(),
    ) else {
        return;
    };

    let Some(model_covariance_rows) = diagonal_covariance_from_standard_errors(
        [
            model_standard_errors.total_count,
            model_standard_errors.mean_hourly_count,
            unit_scale_standard_error(count),
            binomial_share_standard_error(model_zero_count_share, count),
        ]
        .as_slice(),
    ) else {
        return;
    };

    add_objective_family_from_model_vector(
        builder,
        "activity_level_timing",
        vec![
            "activity_level_timing.total_count".to_string(),
            "activity_level_timing.mean_hourly_count".to_string(),
            "activity_level_timing.hourly_count_cv".to_string(),
            "activity_level_timing.zero_count_share".to_string(),
        ],
        observed_vector,
        observed_covariance_rows,
        model_vector,
        Some(model_covariance_rows),
    );
}

#[derive(Debug, Clone, Copy)]
struct ActivityLevelStandardErrors {
    total_count: f64,
    mean_hourly_count: f64,
}

fn activity_level_standard_errors(counts: &[f64], fano_factor: f64) -> ActivityLevelStandardErrors {
    let total_count = counts.iter().sum::<f64>();
    let count = counts.len().max(1);

    let hawkes_total_standard_error = hawkes_count_standard_error(total_count, fano_factor);
    let hawkes_mean_standard_error = hawkes_total_standard_error / count as f64;

    let block_mean_standard_error =
        activity_session_block_mean_standard_error(counts).unwrap_or(0.0);

    let block_total_standard_error = block_mean_standard_error * count as f64;

    ActivityLevelStandardErrors {
        total_count: hawkes_total_standard_error.max(block_total_standard_error),
        mean_hourly_count: hawkes_mean_standard_error.max(block_mean_standard_error),
    }
}

fn activity_session_block_mean_standard_error(counts: &[f64]) -> Option<f64> {
    if counts.len() < ACTIVITY_PROFILE_SESSION_HOURS * 2 {
        return None;
    }

    let block_rates = counts
        .chunks(ACTIVITY_PROFILE_SESSION_HOURS)
        .filter_map(|session_counts| {
            if session_counts.is_empty() {
                return None;
            }

            let session_total = session_counts.iter().sum::<f64>();

            session_total
                .is_finite()
                .then_some(session_total / session_counts.len() as f64)
        })
        .collect::<Vec<_>>();
    if block_rates.len() < 2 {
        return None;
    }

    let block_standard_deviation = sample_standard_deviation(block_rates.as_slice());
    let standard_error = block_standard_deviation / (block_rates.len() as f64).sqrt();

    (standard_error.is_finite() && standard_error > 0.0).then_some(standard_error)
}

fn add_activity_shape_terms(
    builder: &mut CalibrationLossBuilder,
    observed_counts: &[f64],
    model_counts: &[f64],
) {
    let observed_total = observed_counts.iter().sum::<f64>();
    let model_total = model_counts.iter().sum::<f64>();

    if observed_total <= 0.0 || model_total <= 0.0 {
        return;
    }

    let concentration = dirichlet_multinomial_concentration_from_hourly_counts(observed_counts);
    let count = observed_counts.len().min(model_counts.len());

    let observed_shares = observed_counts[..count]
        .iter()
        .map(|count| *count / observed_total)
        .collect::<Vec<_>>();
    let model_shares = model_counts[..count]
        .iter()
        .map(|count| *count / model_total)
        .collect::<Vec<_>>();

    let Some(observed_vector) = share_contrast_vector(observed_shares.as_slice()) else {
        return;
    };

    let Some(model_vector) = share_contrast_vector(model_shares.as_slice()) else {
        return;
    };

    let Some(observed_covariance_rows) = dirichlet_multinomial_share_covariance(
        observed_shares.as_slice(),
        observed_total,
        concentration,
    )
    .and_then(|covariance| share_contrast_covariance(covariance.as_slice())) else {
        return;
    };

    let Some(model_covariance_rows) =
        dirichlet_multinomial_share_covariance(model_shares.as_slice(), model_total, concentration)
            .and_then(|covariance| share_contrast_covariance(covariance.as_slice()))
    else {
        return;
    };

    let coordinate_ids = (0..count)
        .map(|hour_index| format!("activity_shape.hourly_profile_share_{hour_index:03}"))
        .collect::<Vec<_>>();

    add_objective_family_from_model_vector(
        builder,
        "activity_shape",
        coordinate_ids,
        observed_vector,
        observed_covariance_rows,
        model_vector,
        Some(model_covariance_rows),
    );
}

fn mean_profile_by_seed(profiles_by_seed: &[&[f64]]) -> Vec<f64> {
    let Some(profile_length) = profiles_by_seed.iter().map(|profile| profile.len()).min() else {
        return Vec::new();
    };

    (0..profile_length)
        .map(|position| {
            average_seed_score(profiles_by_seed.iter().map(|profile| profile[position]))
        })
        .collect()
}

fn standard_error_from_scale(scale: f64, sample_count: usize) -> f64 {
    if !scale.is_finite() || scale <= 0.0 || sample_count == 0 {
        return f64::INFINITY;
    }

    scale / (sample_count as f64).sqrt()
}

fn standard_error_from_positive_value(value: f64, sample_count: usize) -> f64 {
    standard_error_from_scale(value.abs(), sample_count)
}

fn standard_deviation_standard_error(standard_deviation: f64, sample_count: usize) -> f64 {
    if sample_count <= 1 {
        return f64::INFINITY;
    }

    standard_deviation / (2.0 * (sample_count - 1) as f64).sqrt()
}

fn quantile_standard_error(
    sorted_values: &[f64],
    quantile_level: f64,
    effective_sample_count: usize,
) -> f64 {
    if sorted_values.len() <= 1 || effective_sample_count <= 1 || !quantile_level.is_finite() {
        return f64::INFINITY;
    }

    let sample_count = sorted_values.len();

    let rank_position = quantile_level.clamp(0.0, 1.0) * (sample_count - 1) as f64;
    let rank_index = rank_position.round() as usize;
    let rank_window = (sample_count as f64).sqrt().round().max(1.0) as usize;

    let lower_index = rank_index.saturating_sub(rank_window);
    let upper_index = rank_index.saturating_add(rank_window).min(sample_count - 1);

    let value_spacing = sorted_values[upper_index] - sorted_values[lower_index];

    let rank_count = upper_index.saturating_sub(lower_index).max(1) as f64;

    if value_spacing > 0.0 {
        let density_estimate = rank_count / (sample_count as f64 * value_spacing);
        return (quantile_level * (1.0 - quantile_level) / effective_sample_count as f64).sqrt()
            / density_estimate;
    }

    standard_error_from_scale(
        sample_standard_deviation(sorted_values),
        effective_sample_count,
    )
}

fn bowley_skewness_standard_error(sorted_values: &[f64], effective_sample_count: usize) -> f64 {
    if sorted_values.len() < 3 || effective_sample_count <= 1 {
        return f64::INFINITY;
    }

    let lower_quartile = compute_sorted_quantile_or_zero(sorted_values, 0.25);
    let median = compute_sorted_quantile_or_zero(sorted_values, 0.50);
    let upper_quartile = compute_sorted_quantile_or_zero(sorted_values, 0.75);

    let denominator = upper_quartile - lower_quartile;
    if !denominator.is_finite() || denominator.abs() <= JOINT_REFINEMENT_POSITIVE_METRIC_FLOOR {
        return f64::INFINITY;
    }

    let numerator = upper_quartile + lower_quartile - 2.0 * median;
    let denominator_squared = denominator * denominator;

    let lower_quartile_sensitivity = (denominator + numerator) / denominator_squared;
    let median_sensitivity = -2.0 / denominator;
    let upper_quartile_sensitivity = (denominator - numerator) / denominator_squared;

    let lower_quartile_standard_error =
        quantile_standard_error(sorted_values, 0.25, effective_sample_count);

    let median_standard_error =
        quantile_standard_error(sorted_values, 0.50, effective_sample_count);

    let upper_quartile_standard_error =
        quantile_standard_error(sorted_values, 0.75, effective_sample_count);

    let variance = [
        lower_quartile_sensitivity * lower_quartile_standard_error,
        median_sensitivity * median_standard_error,
        upper_quartile_sensitivity * upper_quartile_standard_error,
    ]
    .into_iter()
    .map(|component| component * component)
    .sum::<f64>();

    if variance.is_finite() && variance > 0.0 {
        variance.sqrt()
    } else {
        f64::INFINITY
    }
}

fn unit_scale_standard_error(sample_count: usize) -> f64 {
    standard_error_from_scale(1.0, sample_count)
}

fn binomial_share_standard_error(share: f64, sample_count: usize) -> f64 {
    if sample_count == 0 || !share.is_finite() {
        return f64::INFINITY;
    }

    (share.clamp(0.0, 1.0) * (1.0 - share.clamp(0.0, 1.0)) / sample_count as f64).sqrt()
}

fn hawkes_count_standard_error(expected_count: f64, fano_factor: f64) -> f64 {
    if !expected_count.is_finite() || expected_count < 0.0 || !fano_factor.is_finite() {
        return f64::INFINITY;
    }

    (expected_count * fano_factor.max(1.0)).sqrt()
}

fn coefficient_of_variation(values: &[f64]) -> f64 {
    let mean_value = arithmetic_mean_or_zero(values);
    if mean_value <= 0.0 {
        return 0.0;
    }

    sample_standard_deviation(values) / mean_value
}

fn zero_count_share(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }

    values.iter().filter(|value| **value == 0.0).count() as f64 / values.len() as f64
}

fn dirichlet_multinomial_concentration_from_hourly_counts(hourly_counts: &[f64]) -> f64 {
    let total_count = hourly_counts.iter().sum::<f64>();
    if hourly_counts.len() < 48 || total_count <= 0.0 {
        return total_count.max(1.0);
    }

    let session_length = ACTIVITY_PROFILE_SESSION_HOURS;
    let session_count = hourly_counts.len() / session_length;
    if session_count < 2 {
        return total_count.max(1.0);
    }

    let mut session_totals = Vec::with_capacity(session_count);
    let mut share_by_session = Vec::with_capacity(session_count);

    for session_counts in hourly_counts.chunks(session_length).take(session_count) {
        let session_total = session_counts.iter().sum::<f64>();
        if session_total <= 0.0 {
            continue;
        }

        session_totals.push(session_total);

        share_by_session.push(
            session_counts
                .iter()
                .map(|count| *count / session_total)
                .collect::<Vec<_>>(),
        );
    }

    if share_by_session.len() < 2 {
        return total_count.max(1.0);
    }

    let mean_session_total = arithmetic_mean_or_zero(session_totals.as_slice());
    if mean_session_total <= 1.0 {
        return total_count.max(1.0);
    }

    let mut rho_estimates = Vec::new();

    for hour_index in 0..session_length {
        let shares = share_by_session
            .iter()
            .filter_map(|session_shares| session_shares.get(hour_index).copied())
            .collect::<Vec<_>>();
        if shares.len() < 2 {
            continue;
        }

        let mean_share = arithmetic_mean_or_zero(shares.as_slice());

        let multinomial_variance = mean_share * (1.0 - mean_share) / mean_session_total;
        if multinomial_variance <= 0.0 {
            continue;
        }

        let observed_variance = sample_variance(shares.as_slice()).unwrap_or(0.0);

        let rho = ((observed_variance / multinomial_variance) - 1.0) / (mean_session_total - 1.0);
        if rho.is_finite() && rho > 0.0 && rho < 1.0 {
            rho_estimates.push(rho);
        }
    }

    let rho = arithmetic_mean_or_zero(rho_estimates.as_slice());
    if rho > 0.0 {
        (1.0 / rho) - 1.0
    } else {
        total_count.max(1.0)
    }
}

fn dirichlet_multinomial_share_covariance(
    shares: &[f64],
    total_count: f64,
    concentration: f64,
) -> Option<Vec<Vec<f64>>> {
    if shares.is_empty()
        || shares.iter().any(|share| !share.is_finite())
        || !total_count.is_finite()
        || !concentration.is_finite()
        || total_count <= 0.0
        || concentration <= 0.0
    {
        return None;
    }

    let overdispersion = (total_count + concentration) / (1.0 + concentration);
    let covariance_scale = overdispersion / total_count;
    let mut covariance = vec![vec![0.0; shares.len()]; shares.len()];

    for row_position in 0..shares.len() {
        let row_share = shares[row_position].clamp(0.0, 1.0);

        for column_position in 0..shares.len() {
            let column_share = shares[column_position].clamp(0.0, 1.0);

            covariance[row_position][column_position] = if row_position == column_position {
                row_share * (1.0 - row_share) * covariance_scale
            } else {
                -row_share * column_share * covariance_scale
            };
        }
    }

    Some(covariance)
}

fn hawkes_count_fano_factor(
    fitted_parameters: &JointRefinementStageFittedParameters,
) -> Option<f64> {
    let branching_ratio = hawkes_runtime_effective_branching_ratio(fitted_parameters)?;
    if !(0.0..1.0).contains(&branching_ratio) {
        return None;
    }

    Some((1.0 - branching_ratio).recip().powi(2))
}

fn hawkes_runtime_effective_branching_ratio(
    fitted_parameters: &JointRefinementStageFittedParameters,
) -> Option<f64> {
    let derived_alpha = HawkesKernelLoadings::try_from_structured_alpha(
        fitted_parameters.hawkes_fitted_parameters.structured_alpha,
    )
    .ok()?;

    Some(spectral_radius_alpha_over_beta(
        derived_alpha,
        fitted_parameters
            .hawkes_fitted_parameters
            .kernel_parameters
            .beta,
    ))
}

fn skewness_or_zero(values: &[f64]) -> f64 {
    central_skewness_or_zero(values, JOINT_REFINEMENT_POSITIVE_METRIC_FLOOR)
}

fn bowley_skewness_or_zero(values: &[f64]) -> f64 {
    bowley_skewness_from_sorted(
        sorted_finite_values(values).as_slice(),
        JOINT_REFINEMENT_POSITIVE_METRIC_FLOOR,
    )
    .unwrap_or(0.0)
}

fn violates_runtime_seasonality_scaled_stationarity(
    fitted_parameters: &JointRefinementStageFittedParameters,
    _context: &JointRefinementContext,
) -> bool {
    let Ok(stability_check) = HawkesKernelParameters::check_spectral_radius_stability(
        fitted_parameters
            .hawkes_fitted_parameters
            .kernel_parameters
            .beta,
        fitted_parameters
            .hawkes_fitted_parameters
            .kernel_parameters
            .alpha,
        fitted_parameters
            .hawkes_fitted_parameters
            .stability_numeric_tolerance,
    ) else {
        return true;
    };

    stability_check.spectral_radius_alpha_over_beta >= 1.0
}

#[cfg(test)]
mod tests {
    use super::{
        LikelihoodContextCachePolicy, ReturnScaleSummaryTermInput, activity_level_fit_for_counts,
        activity_level_standard_errors, activity_shape_fit_for_counts,
        add_return_scale_mechanism_terms, add_return_scale_summary_terms,
        bowley_skewness_standard_error, likelihood_block_mean_variance,
        likelihood_context_cache_policy_for_joint_fidelity,
        likelihood_fidelity_tier_for_joint_fidelity, return_shape_effective_sample_count,
        return_shape_statistic_estimates, statistic_estimate_by_name,
    };
    use crate::objective::CalibrationLossBuilder;
    use crate::stages::joint_metrics::JointRefinementReturnScaleSummary;
    use crate::stages::joint_refinement::execution::JointRefinementFidelityTier;
    use crate::stages::return_components::{
        ContinuousLikelihoodBlockScore, ContinuousLikelihoodFidelityTier,
    };
    use janiform_foundations::numeric::{
        sample_standard_deviation, winsorized_sample_standard_deviation_from_sorted,
    };

    #[test]
    fn expected_activity_fit_helpers_match_hourly_count_identity() {
        let observed_counts = [10.0, 20.0, 30.0];

        assert_eq!(
            activity_shape_fit_for_counts(&observed_counts, &observed_counts),
            0.0
        );
        assert_eq!(
            activity_level_fit_for_counts(&observed_counts, &observed_counts),
            0.0
        );
    }

    #[test]
    fn return_shape_uncertainty_uses_temporal_block_effective_sample_count() {
        let returns = (-500..500)
            .map(|value| f64::from(value) / 100.0)
            .collect::<Vec<_>>();

        let full_sample_estimates =
            return_shape_statistic_estimates(returns.as_slice(), returns.len());
        let full_sample_winsorized_volatility = statistic_estimate_by_name(
            full_sample_estimates.as_slice(),
            "winsorized_volatility_q05_q95",
        )
        .expect("winsorized volatility estimate should be present");

        let temporal_blocked_estimates = return_shape_statistic_estimates(returns.as_slice(), 10);
        let temporal_blocked_winsorized_volatility = statistic_estimate_by_name(
            temporal_blocked_estimates.as_slice(),
            "winsorized_volatility_q05_q95",
        )
        .expect("winsorized volatility estimate should be present");

        assert_eq!(return_shape_effective_sample_count(5, 10), 5);
        assert_eq!(return_shape_effective_sample_count(1_000, 10), 10);
        assert!(
            temporal_blocked_winsorized_volatility.standard_error
                > full_sample_winsorized_volatility.standard_error
        );
    }

    #[test]
    fn return_shape_winsorized_volatility_limits_tail_leverage() {
        let base_returns = [-1.0, -0.5, 0.0, 0.5, 1.0];
        let tail_contaminated_returns = [-1.0, -0.5, 0.0, 0.5, 100.0];

        let base_winsorized_volatility =
            winsorized_sample_standard_deviation_from_sorted(&base_returns, 0.20, 0.80);

        let contaminated_winsorized_volatility = winsorized_sample_standard_deviation_from_sorted(
            &tail_contaminated_returns,
            0.20,
            0.80,
        );

        let contaminated_sample_volatility =
            sample_standard_deviation(tail_contaminated_returns.as_slice());

        assert!(contaminated_winsorized_volatility > base_winsorized_volatility);
        assert!(contaminated_winsorized_volatility < contaminated_sample_volatility);
    }

    #[test]
    fn return_shape_statistics_include_l_kurtosis() {
        let returns = [-3.0, -1.0, 0.0, 1.0, 3.0, 8.0];
        let estimates = return_shape_statistic_estimates(returns.as_slice(), returns.len());

        let l_kurtosis = statistic_estimate_by_name(estimates.as_slice(), "l_moment_tau4")
            .expect("return-shape statistics should include L-kurtosis");

        assert!(l_kurtosis.value.is_finite());
        assert!(l_kurtosis.standard_error.is_finite());
    }

    #[test]
    fn return_scale_terms_use_q90_label_and_seed_covariance() {
        let observed_summary = JointRefinementReturnScaleSummary {
            mean_absolute_return: 1.0,
            realized_variance: 1.0,
            absolute_return_quantile_q90: 1.0,
        };

        let seed_summaries = [
            JointRefinementReturnScaleSummary {
                mean_absolute_return: 1.0,
                realized_variance: 1.0,
                absolute_return_quantile_q90: 1.0,
            },
            JointRefinementReturnScaleSummary {
                mean_absolute_return: 3.0,
                realized_variance: 3.0,
                absolute_return_quantile_q90: 3.0,
            },
        ];

        let mut builder = CalibrationLossBuilder::new();

        add_return_scale_summary_terms(
            &mut builder,
            "1d",
            observed_summary,
            seed_summaries.as_slice(),
            4,
            "return_scale",
        );

        let loss = builder.build();

        let q90_term = loss
            .term_diagnostics
            .iter()
            .find(|term| term.term_id == "return_scale.1d.absolute_return_quantile_q90")
            .expect("q90 return-scale term should be emitted");

        assert_eq!(q90_term.model_value, Some(2.0));
        assert_eq!(q90_term.covariance_diagonal, Some(1.25));
    }

    #[test]
    fn return_scale_mechanism_terms_group_horizons_without_renaming_coordinates() {
        let observed_summary = JointRefinementReturnScaleSummary {
            mean_absolute_return: 1.0,
            realized_variance: 2.0,
            absolute_return_quantile_q90: 3.0,
        };
        let one_hour_seed_summaries = [JointRefinementReturnScaleSummary {
            mean_absolute_return: 1.5,
            realized_variance: 2.5,
            absolute_return_quantile_q90: 3.5,
        }];
        let one_day_seed_summaries = [JointRefinementReturnScaleSummary {
            mean_absolute_return: 2.0,
            realized_variance: 3.0,
            absolute_return_quantile_q90: 4.0,
        }];

        let mut builder = CalibrationLossBuilder::new();
        add_return_scale_mechanism_terms(
            &mut builder,
            &[
                ReturnScaleSummaryTermInput {
                    horizon_label: "1h",
                    observed_summary,
                    model_summaries: one_hour_seed_summaries.as_slice(),
                    observed_sample_count: 8,
                },
                ReturnScaleSummaryTermInput {
                    horizon_label: "1d",
                    observed_summary,
                    model_summaries: one_day_seed_summaries.as_slice(),
                    observed_sample_count: 4,
                },
            ],
            "return_scale",
        );
        let loss = builder.build();

        assert_eq!(loss.family_losses[0].family_id, "return_scale");
        assert_eq!(loss.family_losses[0].coordinate_count, 6);
        assert_eq!(
            loss.term_diagnostics
                .iter()
                .map(|term| term.term_id.as_str())
                .collect::<Vec<_>>(),
            vec![
                "return_scale.1h.mean_absolute_return",
                "return_scale.1h.realized_variance",
                "return_scale.1h.absolute_return_quantile_q90",
                "return_scale.1d.mean_absolute_return",
                "return_scale.1d.realized_variance",
                "return_scale.1d.absolute_return_quantile_q90",
            ]
        );
    }

    #[test]
    fn likelihood_block_variance_uses_temporal_block_means() {
        let block_scores = [
            ContinuousLikelihoodBlockScore {
                shifted_negative_log_likelihood: 1.0,
                member_weight: 1.0,
            },
            ContinuousLikelihoodBlockScore {
                shifted_negative_log_likelihood: 3.0,
                member_weight: 1.0,
            },
            ContinuousLikelihoodBlockScore {
                shifted_negative_log_likelihood: 2.0,
                member_weight: 1.0,
            },
            ContinuousLikelihoodBlockScore {
                shifted_negative_log_likelihood: 4.0,
                member_weight: 1.0,
            },
        ];

        let average_variance = likelihood_block_mean_variance(block_scores.as_slice(), 2);

        assert_eq!(average_variance, Some(0.25));
    }

    #[test]
    fn bowley_uncertainty_reflects_discrete_quantile_instability() {
        let mut returns = vec![-1.0; 1_180];
        returns.extend(vec![1.0; 1_220]);

        let standard_error = bowley_skewness_standard_error(returns.as_slice(), 168);

        assert!(standard_error > 1.0);
    }

    #[test]
    fn activity_level_uncertainty_uses_session_block_variation() {
        let smooth_counts = vec![100.0; 72];

        let clustered_counts = (0..72)
            .map(|hour_index| {
                if hour_index < 24 {
                    50.0
                } else if hour_index < 48 {
                    100.0
                } else {
                    150.0
                }
            })
            .collect::<Vec<_>>();

        let smooth_errors = activity_level_standard_errors(smooth_counts.as_slice(), 1.0);
        let clustered_errors = activity_level_standard_errors(clustered_counts.as_slice(), 1.0);

        assert!(clustered_errors.total_count > smooth_errors.total_count);
        assert!(clustered_errors.mean_hourly_count > smooth_errors.mean_hourly_count);
    }

    #[test]
    fn joint_search_uses_tiered_likelihood_fidelity_for_return_terms() {
        assert_eq!(
            likelihood_fidelity_tier_for_joint_fidelity(JointRefinementFidelityTier::CoarseLocal),
            ContinuousLikelihoodFidelityTier::Representative
        );
        assert_eq!(
            likelihood_fidelity_tier_for_joint_fidelity(JointRefinementFidelityTier::Local),
            ContinuousLikelihoodFidelityTier::Full
        );
        assert_eq!(
            likelihood_fidelity_tier_for_joint_fidelity(JointRefinementFidelityTier::Final),
            ContinuousLikelihoodFidelityTier::Full
        );
        assert_eq!(
            likelihood_context_cache_policy_for_joint_fidelity(
                JointRefinementFidelityTier::CoarseLocal
            ),
            LikelihoodContextCachePolicy::CachedByModeledRate
        );
        assert_eq!(
            likelihood_context_cache_policy_for_joint_fidelity(JointRefinementFidelityTier::Local),
            LikelihoodContextCachePolicy::Uncached
        );
    }
}
