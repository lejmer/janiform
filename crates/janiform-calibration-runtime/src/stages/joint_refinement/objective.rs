use std::cmp::Ordering;
use std::sync::mpsc::Sender;

use janiform_foundations::numeric::{
    bowley_skewness_from_sorted, central_skewness_or_zero, relative_rmse, sorted_finite_values,
    standardized_path_distance,
};

use super::execution::{
    JOINT_REFINEMENT_OBJECTIVE_NORMALIZATION_FLOOR, JOINT_REFINEMENT_POSITIVE_METRIC_FLOOR,
    JointRefinementBaselineBuckets, JointRefinementCandidateEvaluation, JointRefinementContext,
    JointRefinementDeterministicCacheKey, JointRefinementDeterministicEvaluation,
    JointRefinementFidelityTier, JointRefinementObservedData, JointRefinementSeedSummary,
    ordered_parameter_bits,
};
use super::setup::{
    cached_seed_summaries_for_candidate_seeds, compute_cumulative_log_price_span,
    parameter_vector_to_fitted_parameters,
};
use crate::stages::joint_metrics::{
    JointRefinementBusinessTimeBucketReturnScaleSummaries, JointRefinementReturnScaleSummary,
    log_ratio_distance, normalized_profile_distance, zero_aware_normalized_bucket_value,
};
use crate::stages::return_components::{
    ContinuousLikelihoodFidelityTier, evaluate_configuration_likelihood_objective,
    rescale_continuous_return_calibration_sample_business_time_scales,
};
use janiform_calibration_artifacts::{
    CalibrationOptimizerSettings, JointRefinementStageFittedParameters,
    JointRefinementStageObjectiveBreakdown,
};
use janiform_model_kernels::hawkes::HawkesKernelParameters;

use super::contracts::{JointRefinementStageProgressPhase, JointRefinementStageProgressUpdate};

pub(super) use janiform_foundations::numeric::root_mean_square_or_infinity as root_mean_square;

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

pub(super) fn evaluate_joint_candidate_nig_block_search_objective(
    parameter_vector: &[f64],
    context: &JointRefinementContext,
) -> Option<f64> {
    let fitted_parameters = parameter_vector_to_fitted_parameters(parameter_vector, context)?;
    if violates_runtime_seasonality_scaled_stationarity(&fitted_parameters, context) {
        context
            .profiling_state
            .record_invalid_candidate_evaluation();
        return None;
    }

    let modeled_open_event_rate_per_hour =
        modeled_open_event_rate_per_hour(&fitted_parameters, context);
    let raw_aggregated_likelihood_fit = raw_aggregated_likelihood_fit_for_fidelity(
        &fitted_parameters,
        modeled_open_event_rate_per_hour,
        context,
        ContinuousLikelihoodFidelityTier::Local,
    );
    let normalized_likelihood_fit = normalize_bucket(
        raw_aggregated_likelihood_fit,
        context.local_baseline_buckets.aggregated_likelihood_fit,
    );

    normalized_likelihood_fit
        .is_finite()
        .then_some(normalized_likelihood_fit)
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

    let deterministic_evaluation =
        resolve_deterministic_evaluation(&fitted_parameters, simulation_parameter_bits, context);

    emit_joint_refinement_candidate_evaluation_progress(
        progress_sender,
        progress_phase,
        1,
        total_progress_units,
        Some("deterministic_evaluation"),
    );

    let raw_aggregated_likelihood_fit = deterministic_evaluation.raw_aggregated_likelihood_fit;

    let simulation_seeds = match fidelity_tier {
        JointRefinementFidelityTier::Local => vec![context.local_simulation_seed],
        JointRefinementFidelityTier::Final => context.final_simulation_seeds.clone(),
    };

    let seed_summaries = match cached_seed_summaries_for_candidate_seeds(
        simulation_parameter_bits,
        &fitted_parameters,
        context,
        simulation_seeds.as_slice(),
        progress_sender,
        progress_phase,
        1,
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

    let resolved_seed_summaries = seed_summaries
        .into_iter()
        .map(|(_, summary)| summary)
        .collect::<Vec<_>>();

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

    let raw_price_span_sanity_fit =
        average_seed_score(resolved_seed_summaries.iter().map(|seed_summary| {
            (compute_cumulative_log_price_span(seed_summary.simulated_15m_intervals.as_slice())
                - context.observed_data.observed_log_price_span)
                .abs()
                / context.observed_data.observed_log_price_span.max(1.0e-8)
        }));

    let baseline_buckets = match fidelity_tier {
        JointRefinementFidelityTier::Local => &context.local_baseline_buckets,
        JointRefinementFidelityTier::Final => &context.final_baseline_buckets,
    };

    emit_joint_refinement_candidate_evaluation_progress(
        progress_sender,
        progress_phase,
        total_progress_units,
        total_progress_units,
        Some("objective_normalization"),
    );

    let normalized_aggregated_likelihood_fit = normalize_bucket(
        raw_aggregated_likelihood_fit,
        baseline_buckets.aggregated_likelihood_fit,
    );

    let normalized_aggregated_return_shape_fit = normalize_bucket(
        raw_aggregated_return_shape_fit,
        baseline_buckets.aggregated_return_shape_fit,
    );

    let normalized_return_asymmetry_fit = normalize_bucket(
        raw_return_asymmetry_fit,
        baseline_buckets.return_asymmetry_fit,
    );

    let normalized_tick_return_asymmetry_fit = normalize_bucket(
        raw_tick_return_asymmetry_fit,
        baseline_buckets.tick_return_asymmetry_fit,
    );

    let normalized_aggregated_return_scale_fit = normalize_bucket(
        raw_aggregated_return_scale_fit,
        baseline_buckets.aggregated_return_scale_fit,
    );

    let normalized_tick_return_scale_fit = normalize_bucket(
        raw_tick_return_scale_fit,
        baseline_buckets.tick_return_scale_fit,
    );

    let normalized_business_time_return_scale_fit = normalize_bucket(
        raw_business_time_return_scale_fit,
        baseline_buckets.business_time_return_scale_fit,
    );

    let normalized_intraday_return_level_fit = normalize_bucket(
        raw_intraday_return_level_fit,
        baseline_buckets.intraday_return_level_fit,
    );

    let normalized_activity_shape_fit =
        normalize_bucket(raw_activity_shape_fit, baseline_buckets.activity_shape_fit);

    let normalized_activity_level_fit =
        normalize_bucket(raw_activity_level_fit, baseline_buckets.activity_level_fit);

    let normalized_rv_activity_coupling_fit = normalize_bucket(
        raw_rv_activity_coupling_fit,
        baseline_buckets.rv_activity_coupling_fit,
    );

    let normalized_price_span_sanity_fit = normalize_bucket(
        raw_price_span_sanity_fit,
        baseline_buckets.price_span_sanity_fit,
    );

    let hard_constraint_penalty = 0.0;

    let normalized_objective_terms = vec![
        normalized_aggregated_likelihood_fit,
        normalized_aggregated_return_shape_fit,
        normalized_return_asymmetry_fit,
        normalized_tick_return_asymmetry_fit,
        normalized_aggregated_return_scale_fit,
        normalized_tick_return_scale_fit,
        normalized_business_time_return_scale_fit,
        normalized_intraday_return_level_fit,
        normalized_activity_shape_fit,
        normalized_activity_level_fit,
        normalized_rv_activity_coupling_fit,
        normalized_price_span_sanity_fit,
    ];

    let weighted_objective =
        root_mean_square(normalized_objective_terms.as_slice()) + hard_constraint_penalty;

    let use_raw_bucket_values = baseline_bucket_values_are_unity(baseline_buckets);

    emit_joint_refinement_candidate_evaluation_progress(
        progress_sender,
        progress_phase,
        total_progress_units,
        total_progress_units,
        Some("objective_diagnostics"),
    );

    JointRefinementCandidateEvaluation {
        fitted_parameters,
        objective_terms: build_joint_objective_term_diagnostics(
            resolved_seed_summaries.as_slice(),
            &context.observed_data,
        ),
        breakdown: JointRefinementStageObjectiveBreakdown {
            aggregated_likelihood_fit: maybe_raw(
                use_raw_bucket_values,
                raw_aggregated_likelihood_fit,
                normalized_aggregated_likelihood_fit,
            ),
            aggregated_return_shape_fit: maybe_raw(
                use_raw_bucket_values,
                raw_aggregated_return_shape_fit,
                normalized_aggregated_return_shape_fit,
            ),
            return_asymmetry_fit: maybe_raw(
                use_raw_bucket_values,
                raw_return_asymmetry_fit,
                normalized_return_asymmetry_fit,
            ),
            tick_return_asymmetry_fit: maybe_raw(
                use_raw_bucket_values,
                raw_tick_return_asymmetry_fit,
                normalized_tick_return_asymmetry_fit,
            ),
            aggregated_return_scale_fit: maybe_raw(
                use_raw_bucket_values,
                raw_aggregated_return_scale_fit,
                normalized_aggregated_return_scale_fit,
            ),
            tick_return_scale_fit: maybe_raw(
                use_raw_bucket_values,
                raw_tick_return_scale_fit,
                normalized_tick_return_scale_fit,
            ),
            business_time_return_scale_fit: maybe_raw(
                use_raw_bucket_values,
                raw_business_time_return_scale_fit,
                normalized_business_time_return_scale_fit,
            ),
            intraday_return_level_fit: maybe_raw(
                use_raw_bucket_values,
                raw_intraday_return_level_fit,
                normalized_intraday_return_level_fit,
            ),
            activity_shape_fit: maybe_raw(
                use_raw_bucket_values,
                raw_activity_shape_fit,
                normalized_activity_shape_fit,
            ),
            activity_level_fit: maybe_raw(
                use_raw_bucket_values,
                raw_activity_level_fit,
                normalized_activity_level_fit,
            ),
            rv_activity_coupling_fit: maybe_raw(
                use_raw_bucket_values,
                raw_rv_activity_coupling_fit,
                normalized_rv_activity_coupling_fit,
            ),
            price_span_sanity_fit: maybe_raw(
                use_raw_bucket_values,
                raw_price_span_sanity_fit,
                normalized_price_span_sanity_fit,
            ),
            weighted_objective,
            hard_constraint_penalty,
        },
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

pub(super) fn joint_primary_metrics_from_breakdown(
    breakdown: &JointRefinementStageObjectiveBreakdown,
) -> [f64; 12] {
    [
        breakdown.aggregated_likelihood_fit,
        breakdown.aggregated_return_shape_fit,
        breakdown.return_asymmetry_fit,
        breakdown.tick_return_asymmetry_fit,
        breakdown.aggregated_return_scale_fit,
        breakdown.tick_return_scale_fit,
        breakdown.business_time_return_scale_fit,
        breakdown.intraday_return_level_fit,
        breakdown.activity_shape_fit,
        breakdown.activity_level_fit,
        breakdown.rv_activity_coupling_fit,
        breakdown.price_span_sanity_fit,
    ]
}

pub(super) fn compare_joint_candidate_breakdowns(
    left_pareto_rank: usize,
    left_breakdown: &JointRefinementStageObjectiveBreakdown,
    left_admissible: bool,
    left_restart_id: &str,
    right_pareto_rank: usize,
    right_breakdown: &JointRefinementStageObjectiveBreakdown,
    right_admissible: bool,
    right_restart_id: &str,
    has_admissible_candidate: bool,
) -> Ordering {
    let left_max_primary_metric = joint_primary_metrics_from_breakdown(left_breakdown)
        .into_iter()
        .fold(f64::NEG_INFINITY, f64::max);
    let right_max_primary_metric = joint_primary_metrics_from_breakdown(right_breakdown)
        .into_iter()
        .fold(f64::NEG_INFINITY, f64::max);

    let left_primary_metric_sum = joint_primary_metrics_from_breakdown(left_breakdown)
        .into_iter()
        .sum::<f64>();
    let right_primary_metric_sum = joint_primary_metrics_from_breakdown(right_breakdown)
        .into_iter()
        .sum::<f64>();

    (!left_admissible)
        .cmp(&!right_admissible)
        .then_with(|| {
            if has_admissible_candidate {
                left_pareto_rank.cmp(&right_pareto_rank)
            } else {
                left_breakdown
                    .weighted_objective
                    .total_cmp(&right_breakdown.weighted_objective)
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
                .weighted_objective
                .total_cmp(&right_breakdown.weighted_objective)
        })
        .then_with(|| {
            left_breakdown
                .price_span_sanity_fit
                .total_cmp(&right_breakdown.price_span_sanity_fit)
        })
        .then_with(|| left_pareto_rank.cmp(&right_pareto_rank))
        .then_with(|| left_restart_id.cmp(right_restart_id))
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

fn normalize_bucket(raw_value: f64, baseline_bucket: f64) -> f64 {
    zero_aware_normalized_bucket_value(
        raw_value,
        baseline_bucket,
        JOINT_REFINEMENT_OBJECTIVE_NORMALIZATION_FLOOR,
    )
}

fn maybe_raw(use_raw: bool, raw_value: f64, normalized_value: f64) -> f64 {
    if use_raw { raw_value } else { normalized_value }
}

fn average_seed_score(values: impl Iterator<Item = f64>) -> f64 {
    let collected = values.collect::<Vec<_>>();
    collected.iter().sum::<f64>() / collected.len().max(1) as f64
}

fn invalid_candidate_evaluation(
    context: &JointRefinementContext,
    baseline_hawkes_fitted_parameters: &janiform_calibration_artifacts::HawkesStageFittedParameters,
    baseline_return_components_fitted_parameters: &janiform_calibration_artifacts::ReturnComponentsStageFittedParameters,
    constraint_penalty_weight: f64,
    reason: &str,
) -> JointRefinementCandidateEvaluation {
    context
        .profiling_state
        .record_invalid_candidate_evaluation();

    JointRefinementCandidateEvaluation {
        fitted_parameters: JointRefinementStageFittedParameters {
            hawkes_fitted_parameters: baseline_hawkes_fitted_parameters.clone(),
            return_components_fitted_parameters: baseline_return_components_fitted_parameters
                .clone(),
            pricing_block_fitted_parameters: None,
        },
        objective_terms: Vec::new(),
        breakdown: JointRefinementStageObjectiveBreakdown {
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
            price_span_sanity_fit: 100.0,
            weighted_objective: 100.0 + constraint_penalty_weight,
            hard_constraint_penalty: constraint_penalty_weight,
        },
        invalid_reason: Some(reason.to_string()),
    }
}

fn resolve_deterministic_evaluation(
    fitted_parameters: &JointRefinementStageFittedParameters,
    parameter_bits: Option<&[u64]>,
    context: &JointRefinementContext,
) -> JointRefinementDeterministicEvaluation {
    let cache_key = parameter_bits.map(|bits| JointRefinementDeterministicCacheKey {
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

    let deterministic_evaluation_started_at = std::time::Instant::now();

    let modeled_open_event_rate_per_hour =
        modeled_open_event_rate_per_hour(fitted_parameters, context);
    let raw_aggregated_likelihood_fit = raw_aggregated_likelihood_fit_for_fidelity(
        fitted_parameters,
        modeled_open_event_rate_per_hour,
        context,
        ContinuousLikelihoodFidelityTier::Full,
    );

    let deterministic_evaluation = JointRefinementDeterministicEvaluation {
        modeled_open_event_rate_per_hour,
        raw_aggregated_likelihood_fit,
    };

    context
        .profiling_state
        .record_deterministic_evaluation_duration(
            deterministic_evaluation_started_at
                .elapsed()
                .as_millis()
                .min(u128::from(u64::MAX)) as u64,
        );

    if let Some(deterministic_cache_key) = cache_key
        && let Ok(mut cache_guard) = context.deterministic_evaluation_cache.lock()
    {
        cache_guard.insert(deterministic_cache_key, deterministic_evaluation.clone());
    }

    deterministic_evaluation
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

fn raw_aggregated_likelihood_fit_for_fidelity(
    fitted_parameters: &JointRefinementStageFittedParameters,
    modeled_open_event_rate_per_hour: f64,
    context: &JointRefinementContext,
    fidelity_tier: ContinuousLikelihoodFidelityTier,
) -> f64 {
    let observed_5m_sample =
        crate::stages::joint_metrics::build_continuous_calibration_sample_from_aggregated_intervals(
            context.observed_data.observed_5m_intervals.as_slice(),
            modeled_open_event_rate_per_hour,
            context.observed_data.observed_open_event_rate_per_hour,
            context.observed_data.fallback_sell_share_proxy,
        );
    let observed_15m_sample =
        crate::stages::joint_metrics::build_continuous_calibration_sample_from_aggregated_intervals(
            context.observed_data.observed_15m_intervals.as_slice(),
            modeled_open_event_rate_per_hour,
            context.observed_data.observed_open_event_rate_per_hour,
            context.observed_data.fallback_sell_share_proxy,
        );

    let observed_tick_sample = rescale_continuous_return_calibration_sample_business_time_scales(
        &context
            .observed_data
            .observed_tick_continuous_return_calibration_sample,
        modeled_open_event_rate_per_hour
            / context
                .baseline_modeled_open_event_rate_per_hour
                .max(1.0e-8),
    );

    [
        evaluate_configuration_likelihood_objective(
            fitted_parameters
                .return_components_fitted_parameters
                .nig_shared_plus_shift_configuration,
            &observed_tick_sample,
            fidelity_tier,
        )
        .shifted_objective_score,
        evaluate_configuration_likelihood_objective(
            fitted_parameters
                .return_components_fitted_parameters
                .nig_shared_plus_shift_configuration,
            &observed_5m_sample,
            fidelity_tier,
        )
        .shifted_objective_score,
        evaluate_configuration_likelihood_objective(
            fitted_parameters
                .return_components_fitted_parameters
                .nig_shared_plus_shift_configuration,
            &observed_15m_sample,
            fidelity_tier,
        )
        .shifted_objective_score,
    ]
    .into_iter()
    .sum::<f64>()
        / 3.0
}

fn baseline_bucket_values_are_unity(baseline_buckets: &JointRefinementBaselineBuckets) -> bool {
    baseline_buckets.aggregated_likelihood_fit == 1.0
        && baseline_buckets.aggregated_return_shape_fit == 1.0
        && baseline_buckets.return_asymmetry_fit == 1.0
        && baseline_buckets.tick_return_asymmetry_fit == 1.0
        && baseline_buckets.aggregated_return_scale_fit == 1.0
        && baseline_buckets.tick_return_scale_fit == 1.0
        && baseline_buckets.business_time_return_scale_fit == 1.0
        && baseline_buckets.intraday_return_level_fit == 1.0
        && baseline_buckets.activity_shape_fit == 1.0
        && baseline_buckets.activity_level_fit == 1.0
        && baseline_buckets.rv_activity_coupling_fit == 1.0
        && baseline_buckets.price_span_sanity_fit == 1.0
}

fn aggregated_return_shape_fit(
    seed_summary: &JointRefinementSeedSummary,
    observed_data: &JointRefinementObservedData,
) -> f64 {
    let shape_5m = crate::stages::return_components::distribution_profile_fit_score(
        seed_summary.simulated_5m_shape_returns.as_slice(),
        observed_data.observed_5m_shape_returns.as_slice(),
    )
    .profile_fit_score;
    let shape_15m = crate::stages::return_components::distribution_profile_fit_score(
        seed_summary.simulated_15m_shape_returns.as_slice(),
        observed_data.observed_15m_shape_returns.as_slice(),
    )
    .profile_fit_score;
    let shape_tick = crate::stages::return_components::distribution_profile_fit_score(
        seed_summary.simulated_tick_shape_returns.as_slice(),
        observed_data.observed_tick_shape_returns.as_slice(),
    )
    .profile_fit_score;

    (shape_tick + shape_5m + shape_15m) / 3.0
}

fn aggregated_return_scale_fit(
    seed_summary: &JointRefinementSeedSummary,
    observed_data: &JointRefinementObservedData,
) -> f64 {
    let fit_15m = return_scale_horizon_fit(
        seed_summary.simulated_15m_return_scale_summary,
        observed_data.observed_15m_return_scale_summary,
    );
    let fit_5m = return_scale_horizon_fit(
        seed_summary.simulated_5m_return_scale_summary,
        observed_data.observed_5m_return_scale_summary,
    );
    let fit_tick = return_scale_horizon_fit(
        seed_summary.simulated_tick_return_scale_summary,
        observed_data.observed_tick_return_scale_summary,
    );

    root_mean_square([fit_tick, fit_5m, fit_15m].as_slice())
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
    let fit_15m = return_asymmetry_horizon_fit(
        seed_summary.simulated_15m_shape_returns.as_slice(),
        observed_data.observed_15m_shape_returns.as_slice(),
    );
    let fit_5m = return_asymmetry_horizon_fit(
        seed_summary.simulated_5m_shape_returns.as_slice(),
        observed_data.observed_5m_shape_returns.as_slice(),
    );
    let fit_tick = return_asymmetry_horizon_fit(
        seed_summary.simulated_tick_shape_returns.as_slice(),
        observed_data.observed_tick_shape_returns.as_slice(),
    );

    root_mean_square([fit_tick, fit_5m, fit_15m].as_slice())
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
    let profile_fit = normalized_profile_distance(
        seed_summary.simulated_hourly_tick_counts.as_slice(),
        observed_data.observed_hourly_tick_counts.as_slice(),
    );
    let path_fit = standardized_path_distance(
        seed_summary.simulated_hourly_tick_counts.as_slice(),
        observed_data.observed_hourly_tick_counts.as_slice(),
    );

    root_mean_square([profile_fit, path_fit].as_slice())
}

fn activity_level_fit(
    seed_summary: &JointRefinementSeedSummary,
    observed_data: &JointRefinementObservedData,
) -> f64 {
    let candidate_counts = seed_summary.simulated_hourly_tick_counts.as_slice();
    let observed_counts = observed_data.observed_hourly_tick_counts.as_slice();

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

fn build_joint_objective_term_diagnostics(
    seed_summaries: &[JointRefinementSeedSummary],
    observed_data: &JointRefinementObservedData,
) -> Vec<crate::stages::joint_refinement::contracts::JointRefinementStageObjectiveTermDiagnostic> {
    if seed_summaries.is_empty() {
        return Vec::new();
    }

    let observed_total_hourly_tick_count = observed_data
        .observed_hourly_tick_counts
        .iter()
        .sum::<f64>();

    let observed_mean_hourly_tick_count = observed_total_hourly_tick_count
        / observed_data.observed_hourly_tick_counts.len().max(1) as f64;

    let observed_peak_hourly_tick_count = observed_data
        .observed_hourly_tick_counts
        .iter()
        .copied()
        .fold(0.0, f64::max);

    let observed_peak_hourly_tick_share = observed_peak_hourly_tick_count
        / observed_total_hourly_tick_count.max(JOINT_REFINEMENT_POSITIVE_METRIC_FLOOR);

    let observed_return_scale_summary = observed_data.observed_15m_return_scale_summary;
    let observed_tick_return_scale_summary = observed_data.observed_tick_return_scale_summary;

    let observed_return_shape_skewness =
        skewness_or_zero(observed_data.observed_15m_shape_returns.as_slice());

    let observed_return_shape_bowley_skewness =
        bowley_skewness_or_zero(observed_data.observed_15m_shape_returns.as_slice());

    let observed_tick_shape_skewness =
        skewness_or_zero(observed_data.observed_tick_shape_returns.as_slice());

    let observed_tick_shape_bowley_skewness =
        bowley_skewness_or_zero(observed_data.observed_tick_shape_returns.as_slice());

    let observed_rv_activity_metric = observed_data.observed_15m_rv_activity_metric;
    let observed_log_price_span = observed_data.observed_log_price_span;

    let simulated_total_hourly_tick_count =
        average_seed_score(seed_summaries.iter().map(|seed_summary| {
            seed_summary
                .simulated_hourly_tick_counts
                .iter()
                .sum::<f64>()
        }));

    let simulated_mean_hourly_tick_count =
        average_seed_score(seed_summaries.iter().map(|seed_summary| {
            let total_hourly_tick_count = seed_summary
                .simulated_hourly_tick_counts
                .iter()
                .sum::<f64>();
            total_hourly_tick_count / seed_summary.simulated_hourly_tick_counts.len().max(1) as f64
        }));

    let simulated_peak_hourly_tick_share =
        average_seed_score(seed_summaries.iter().map(|seed_summary| {
            let total_hourly_tick_count = seed_summary
                .simulated_hourly_tick_counts
                .iter()
                .sum::<f64>();
            let peak_hourly_tick_count = seed_summary
                .simulated_hourly_tick_counts
                .iter()
                .copied()
                .fold(0.0, f64::max);
            peak_hourly_tick_count
                / total_hourly_tick_count.max(JOINT_REFINEMENT_POSITIVE_METRIC_FLOOR)
        }));

    let simulated_realized_variance_15m =
        average_seed_score(seed_summaries.iter().map(|seed_summary| {
            seed_summary
                .simulated_15m_return_scale_summary
                .realized_variance
        }));

    let simulated_mean_absolute_return_15m =
        average_seed_score(seed_summaries.iter().map(|seed_summary| {
            seed_summary
                .simulated_15m_return_scale_summary
                .mean_absolute_return
        }));

    let simulated_realized_variance_tick =
        average_seed_score(seed_summaries.iter().map(|seed_summary| {
            seed_summary
                .simulated_tick_return_scale_summary
                .realized_variance
        }));

    let simulated_mean_absolute_return_tick =
        average_seed_score(seed_summaries.iter().map(|seed_summary| {
            seed_summary
                .simulated_tick_return_scale_summary
                .mean_absolute_return
        }));

    let simulated_return_shape_skewness =
        average_seed_score(seed_summaries.iter().map(|seed_summary| {
            skewness_or_zero(seed_summary.simulated_15m_shape_returns.as_slice())
        }));

    let simulated_return_shape_bowley_skewness =
        average_seed_score(seed_summaries.iter().map(|seed_summary| {
            bowley_skewness_or_zero(seed_summary.simulated_15m_shape_returns.as_slice())
        }));

    let simulated_tick_shape_skewness =
        average_seed_score(seed_summaries.iter().map(|seed_summary| {
            skewness_or_zero(seed_summary.simulated_tick_shape_returns.as_slice())
        }));

    let simulated_tick_shape_bowley_skewness =
        average_seed_score(seed_summaries.iter().map(|seed_summary| {
            bowley_skewness_or_zero(seed_summary.simulated_tick_shape_returns.as_slice())
        }));

    let simulated_rv_activity_correlation = average_seed_score(
        seed_summaries
            .iter()
            .map(|seed_summary| seed_summary.simulated_15m_rv_activity_metric.correlation),
    );

    let simulated_rv_activity_path_distance =
        average_seed_score(seed_summaries.iter().map(|seed_summary| {
            seed_summary
                .simulated_15m_rv_activity_metric
                .standardized_path_distance
        }));

    let simulated_log_price_span = average_seed_score(seed_summaries.iter().map(|seed_summary| {
        compute_cumulative_log_price_span(seed_summary.simulated_15m_intervals.as_slice())
    }));

    vec![
        build_positive_joint_objective_term(
            "joint.activity_level",
            "joint.activity_level",
            "activity_level",
            observed_total_hourly_tick_count,
            simulated_total_hourly_tick_count,
        ),
        build_positive_joint_objective_term(
            "joint.activity_level.mean_hourly_tick_count",
            "joint.activity_level.mean_hourly_tick_count",
            "activity_level",
            observed_mean_hourly_tick_count,
            simulated_mean_hourly_tick_count,
        ),
        build_positive_joint_objective_term(
            "joint.activity_shape",
            "joint.activity_shape",
            "activity_shape",
            observed_peak_hourly_tick_share,
            simulated_peak_hourly_tick_share,
        ),
        build_positive_joint_objective_term(
            "joint.return_scale",
            "joint.return_scale",
            "return_scale",
            observed_return_scale_summary.realized_variance,
            simulated_realized_variance_15m,
        ),
        build_positive_joint_objective_term(
            "joint.return_scale.mean_absolute_return_15m",
            "joint.return_scale.mean_absolute_return_15m",
            "return_scale",
            observed_return_scale_summary.mean_absolute_return,
            simulated_mean_absolute_return_15m,
        ),
        build_positive_joint_objective_term(
            "joint.return_scale.tick_realized_variance",
            "joint.return_scale.tick_realized_variance",
            "return_scale",
            observed_tick_return_scale_summary.realized_variance,
            simulated_realized_variance_tick,
        ),
        build_positive_joint_objective_term(
            "joint.return_scale.tick_mean_absolute_return",
            "joint.return_scale.tick_mean_absolute_return",
            "return_scale",
            observed_tick_return_scale_summary.mean_absolute_return,
            simulated_mean_absolute_return_tick,
        ),
        build_signed_joint_objective_term(
            "joint.return_asymmetry.central_skewness",
            "joint.return_asymmetry.central_skewness",
            "return_asymmetry",
            observed_return_shape_skewness,
            simulated_return_shape_skewness,
        ),
        build_signed_joint_objective_term(
            "joint.return_asymmetry.bowley_skewness",
            "joint.return_asymmetry.bowley_skewness",
            "return_asymmetry",
            observed_return_shape_bowley_skewness,
            simulated_return_shape_bowley_skewness,
        ),
        build_signed_joint_objective_term(
            "joint.return_asymmetry.tick_central_skewness",
            "joint.return_asymmetry.tick_central_skewness",
            "return_asymmetry",
            observed_tick_shape_skewness,
            simulated_tick_shape_skewness,
        ),
        build_signed_joint_objective_term(
            "joint.return_asymmetry.tick_bowley_skewness",
            "joint.return_asymmetry.tick_bowley_skewness",
            "return_asymmetry",
            observed_tick_shape_bowley_skewness,
            simulated_tick_shape_bowley_skewness,
        ),
        build_signed_joint_objective_term(
            "joint.rv_activity_coupling",
            "joint.rv_activity_coupling",
            "activity_vs_volatility_coupling",
            observed_rv_activity_metric.correlation,
            simulated_rv_activity_correlation,
        ),
        build_signed_joint_objective_term(
            "joint.rv_activity_coupling.path_distance",
            "joint.rv_activity_coupling.path_distance",
            "activity_vs_volatility_coupling",
            observed_rv_activity_metric.standardized_path_distance,
            simulated_rv_activity_path_distance,
        ),
        build_positive_joint_objective_term(
            "joint.price_span_sanity",
            "joint.price_span_sanity",
            "diagnostic",
            observed_log_price_span,
            simulated_log_price_span,
        ),
    ]
}

fn build_positive_joint_objective_term(
    metric_id: &str,
    target_id: &str,
    target_family: &str,
    observed_value: f64,
    simulated_value: f64,
) -> crate::stages::joint_refinement::contracts::JointRefinementStageObjectiveTermDiagnostic {
    let uncertainty_scale = observed_value
        .abs()
        .max(JOINT_REFINEMENT_POSITIVE_METRIC_FLOOR);

    crate::stages::joint_refinement::contracts::JointRefinementStageObjectiveTermDiagnostic {
        metric_id: metric_id.to_string(),
        target_id: target_id.to_string(),
        target_family: target_family.to_string(),
        observed_value,
        simulated_value,
        uncertainty_scale,
        normalized_error: log_ratio_distance(
            simulated_value,
            observed_value,
            JOINT_REFINEMENT_POSITIVE_METRIC_FLOOR,
        ),
    }
}

fn build_signed_joint_objective_term(
    metric_id: &str,
    target_id: &str,
    target_family: &str,
    observed_value: f64,
    simulated_value: f64,
) -> crate::stages::joint_refinement::contracts::JointRefinementStageObjectiveTermDiagnostic {
    let uncertainty_scale = observed_value
        .abs()
        .max(simulated_value.abs())
        .max(JOINT_REFINEMENT_POSITIVE_METRIC_FLOOR);

    crate::stages::joint_refinement::contracts::JointRefinementStageObjectiveTermDiagnostic {
        metric_id: metric_id.to_string(),
        target_id: target_id.to_string(),
        target_family: target_family.to_string(),
        observed_value,
        simulated_value,
        uncertainty_scale,
        normalized_error: (simulated_value - observed_value).abs() / uncertainty_scale,
    }
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
    context: &JointRefinementContext,
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

    context.max_runtime_hourly_seasonality_factor * stability_check.spectral_radius_alpha_over_beta
        >= 1.0
}
