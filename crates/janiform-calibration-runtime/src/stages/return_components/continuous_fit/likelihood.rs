use janiform_affine_oracle::sufficient_statistics::SufficientStatisticsTransformSliceSettings;
use janiform_foundations::numeric::arithmetic_mean_or_zero as arithmetic_mean;
use janiform_foundations::randomness::SplitMix64SimulationRandomnessStream;
use janiform_foundations::splitmix64_output_mixer;
use janiform_model_kernels::nig::NigSharedPlusShiftConfiguration;
use rayon::iter::{IndexedParallelIterator, IntoParallelRefIterator, ParallelIterator};

use super::super::contracts::{
    ReturnComponentsStageExactLikelihoodEvaluatorKind,
    ReturnComponentsStageExactLikelihoodFailureDiagnostic,
    ReturnComponentsStageExactLikelihoodFailureReason,
    ReturnComponentsStageExactLikelihoodFailureScope,
};
use super::constants::RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_PROFILE_LIKELIHOOD_CHI_SQUARE_95;
use super::exact_block::{
    ExactBlockInputBuildInput, ExactLikelihoodFailureContext, build_exact_block_input,
    evaluate_exact_block_likelihood,
};
use super::exact_block_cache::{
    ExactBlockEvaluationCache, ExactBlockEvaluationCacheKey, cached_exact_block_evaluation,
    record_exact_block_evaluation,
};
use super::types::{
    ContinuousLikelihoodEvaluatorCounts, ContinuousLikelihoodFidelityTier,
    ContinuousLikelihoodObjective, ContinuousReturnOptimizerContext,
    ReturnComponentsStageContinuousReturnCalibrationSample,
};

#[derive(Debug, Clone, Copy)]
pub(super) struct ExactBlockLikelihoodEvaluation {
    pub(super) evaluator_kind: ReturnComponentsStageExactLikelihoodEvaluatorKind,
    pub(super) log_density_estimate: f64,
    pub(super) log_density_error_upper_bound: f64,
    pub(super) tilt_theta: Option<f64>,
    pub(super) tilt_domain_lower_theta_exclusive: Option<f64>,
    pub(super) tilt_domain_upper_theta_exclusive: Option<f64>,
    pub(super) block_standard_deviation: f64,
    pub(super) standardized_return: f64,
    pub(super) standardized_decay_scale: Option<f64>,
    pub(super) integration_upper_bound: Option<f64>,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct ExactLikelihoodScopeEvaluation {
    pub(super) average_negative_log_likelihood_estimate: f64,
    pub(super) average_log_likelihood_error_upper_bound: f64,
    pub(super) evaluator_block_counts: ExactEvaluatorBlockCounts,
    pub(super) worst_block_evaluation: ExactBlockLikelihoodEvaluation,
    pub(super) worst_block_member_index: usize,
}

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct ExactEvaluatorBlockCounts {
    pub(super) closed_form_single_side_buy_block_count: usize,
    pub(super) closed_form_single_side_sell_block_count: usize,
    pub(super) tilted_mixed_inversion_block_count: usize,
}

impl ExactEvaluatorBlockCounts {
    fn add_assign(&mut self, other: Self) {
        self.closed_form_single_side_buy_block_count = self
            .closed_form_single_side_buy_block_count
            .saturating_add(other.closed_form_single_side_buy_block_count);
        self.closed_form_single_side_sell_block_count = self
            .closed_form_single_side_sell_block_count
            .saturating_add(other.closed_form_single_side_sell_block_count);
        self.tilted_mixed_inversion_block_count = self
            .tilted_mixed_inversion_block_count
            .saturating_add(other.tilted_mixed_inversion_block_count);
    }
}

impl From<ExactEvaluatorBlockCounts> for ContinuousLikelihoodEvaluatorCounts {
    fn from(counts: ExactEvaluatorBlockCounts) -> Self {
        Self {
            closed_form_buy: counts.closed_form_single_side_buy_block_count,
            closed_form_sell: counts.closed_form_single_side_sell_block_count,
            tilted_mixed: counts.tilted_mixed_inversion_block_count,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(super) struct ExactLikelihoodDiagnosticsSummary {
    pub(super) failures: Vec<ReturnComponentsStageExactLikelihoodFailureDiagnostic>,
    pub(super) global_average_log_likelihood_error_upper_bound: Option<f64>,
    pub(super) conditional_average_log_likelihood_error_upper_bound: Option<f64>,
    pub(super) evaluator_block_counts: ExactEvaluatorBlockCounts,
    pub(super) structural_failure_count: usize,
    pub(super) uncertified_scope_count: usize,
}

#[derive(Debug, Clone)]
enum ExactLikelihoodScopeFailure {
    Structural(Box<ReturnComponentsStageExactLikelihoodFailureDiagnostic>),
    Uncertified {
        diagnostic: Box<ReturnComponentsStageExactLikelihoodFailureDiagnostic>,
        scope_evaluation: Box<ExactLikelihoodScopeEvaluation>,
    },
}

#[derive(Debug, Clone, Copy)]
struct ExactLikelihoodBlockContribution {
    member_index: usize,
    member_weight: f64,
    block_evaluation: ExactBlockLikelihoodEvaluation,
}

#[derive(Debug, Clone)]
pub(super) struct ExactBlockStructuralFailure {
    pub(super) block_member_index: usize,
    pub(super) failure_reason: ReturnComponentsStageExactLikelihoodFailureReason,
    pub(super) context: Box<ExactLikelihoodFailureContext>,
}

impl ExactBlockStructuralFailure {
    pub(super) fn new(
        block_member_index: usize,
        failure_reason: ReturnComponentsStageExactLikelihoodFailureReason,
        context: ExactLikelihoodFailureContext,
    ) -> Self {
        Self {
            block_member_index,
            failure_reason,
            context: Box::new(context),
        }
    }
}

pub(crate) fn evaluate_configuration_likelihood_objective_with_context_and_counts(
    configuration: NigSharedPlusShiftConfiguration,
    optimizer_context: &ContinuousReturnOptimizerContext,
    fidelity_tier: ContinuousLikelihoodFidelityTier,
) -> (
    ContinuousLikelihoodObjective,
    ContinuousLikelihoodEvaluatorCounts,
) {
    let (objective, diagnostics) = continuous_block_likelihood_objective_with_failures(
        configuration,
        optimizer_context,
        fidelity_tier,
    );

    (objective, diagnostics.evaluator_block_counts.into())
}

#[cfg(test)]
pub(super) fn profile_likelihood_score(objective: ContinuousLikelihoodObjective) -> f64 {
    arithmetic_mean(
        [
            objective.global_negative_log_likelihood,
            objective.conditional_negative_log_likelihood,
        ]
        .as_slice(),
    ) + objective.boundary_penalty
}

pub(super) fn maximum_profiled_open_beta_ratio(alpha: f64) -> f64 {
    let probe_gamma_floor = coarse_transform_probe_gamma_floor();
    if !alpha.is_finite() || alpha <= probe_gamma_floor {
        return 0.0;
    }

    (1.0 - (probe_gamma_floor / alpha).powi(2))
        .sqrt()
        .clamp(0.0, maximum_open_beta_ratio())
}

pub(super) fn continuous_block_likelihood_objective(
    configuration: NigSharedPlusShiftConfiguration,
    optimizer_context: &ContinuousReturnOptimizerContext,
    fidelity_tier: ContinuousLikelihoodFidelityTier,
) -> ContinuousLikelihoodObjective {
    continuous_block_likelihood_objective_with_failures(
        configuration,
        optimizer_context,
        fidelity_tier,
    )
    .0
}

pub(super) fn continuous_block_likelihood_objective_with_failures(
    configuration: NigSharedPlusShiftConfiguration,
    optimizer_context: &ContinuousReturnOptimizerContext,
    fidelity_tier: ContinuousLikelihoodFidelityTier,
) -> (
    ContinuousLikelihoodObjective,
    ExactLikelihoodDiagnosticsSummary,
) {
    let boundary_penalty = alpha_beta_boundary_penalty(configuration);
    let mut diagnostics_summary = ExactLikelihoodDiagnosticsSummary::default();
    let selected_member_indices =
        member_indices_for_fidelity_tier(optimizer_context, fidelity_tier);
    let exact_block_cache = ExactBlockEvaluationCache::default();

    let global_scope_evaluation = average_negative_log_likelihood_with_failures(
        configuration,
        optimizer_context,
        selected_member_indices,
        ReturnComponentsStageExactLikelihoodFailureScope::Global,
        None,
        &exact_block_cache,
    );

    let global_negative_log_likelihood = match global_scope_evaluation {
        Ok(scope_evaluation) => {
            diagnostics_summary.global_average_log_likelihood_error_upper_bound =
                Some(scope_evaluation.average_log_likelihood_error_upper_bound);
            diagnostics_summary.evaluator_block_counts = scope_evaluation.evaluator_block_counts;

            scope_evaluation.average_negative_log_likelihood_estimate
        }
        Err(ExactLikelihoodScopeFailure::Structural(failure)) => {
            diagnostics_summary.failures.push(*failure);
            diagnostics_summary.structural_failure_count += 1;

            f64::INFINITY
        }
        Err(ExactLikelihoodScopeFailure::Uncertified {
            diagnostic,
            scope_evaluation,
        }) => {
            diagnostics_summary.failures.push(*diagnostic);

            diagnostics_summary.global_average_log_likelihood_error_upper_bound =
                Some(scope_evaluation.average_log_likelihood_error_upper_bound);
            diagnostics_summary.evaluator_block_counts = scope_evaluation.evaluator_block_counts;

            diagnostics_summary.uncertified_scope_count += 1;

            f64::INFINITY
        }
    };

    let conditional_negative_log_likelihood = if fidelity_tier
        != ContinuousLikelihoodFidelityTier::Full
        || optimizer_context.scale_bucket_profiles.is_empty()
    {
        diagnostics_summary.conditional_average_log_likelihood_error_upper_bound =
            diagnostics_summary.global_average_log_likelihood_error_upper_bound;

        global_negative_log_likelihood
    } else {
        arithmetic_mean(
            optimizer_context
                .scale_bucket_profiles
                .iter()
                .enumerate()
                .filter_map(|(scale_bucket_index, scale_bucket_profile)| {
                    let bucket_member_indices: Vec<usize> = scale_bucket_profile
                        .member_indices
                        .iter()
                        .copied()
                        .filter(|member_index| {
                            selected_member_indices.binary_search(member_index).is_ok()
                        })
                        .collect();
                    if bucket_member_indices.is_empty() {
                        return None;
                    }

                    Some(match average_negative_log_likelihood_with_failures(
                        configuration,
                        optimizer_context,
                        bucket_member_indices.as_slice(),
                        ReturnComponentsStageExactLikelihoodFailureScope::ConditionalScaleBucket,
                        Some(scale_bucket_index),
                        &exact_block_cache,
                    ) {
                        Ok(scope_evaluation) => {
                            diagnostics_summary.conditional_average_log_likelihood_error_upper_bound =
                                Some(match diagnostics_summary
                                    .conditional_average_log_likelihood_error_upper_bound
                                {
                                    Some(current_bound) => current_bound.max(
                                            scope_evaluation
                                                .average_log_likelihood_error_upper_bound,
                                    ),
                                    None => scope_evaluation
                                        .average_log_likelihood_error_upper_bound,
                                });
                            diagnostics_summary
                                .evaluator_block_counts
                                .add_assign(scope_evaluation.evaluator_block_counts);

                            scope_evaluation.average_negative_log_likelihood_estimate
                        }
                        Err(ExactLikelihoodScopeFailure::Structural(failure)) => {
                            diagnostics_summary.structural_failure_count += 1;
                            diagnostics_summary.failures.push(*failure);

                            f64::INFINITY
                        }
                        Err(ExactLikelihoodScopeFailure::Uncertified {
                            diagnostic,
                            scope_evaluation,
                        }) => {
                            diagnostics_summary.uncertified_scope_count += 1;

                            diagnostics_summary.failures.push(*diagnostic);

                            diagnostics_summary.conditional_average_log_likelihood_error_upper_bound =
                                Some(match diagnostics_summary
                                    .conditional_average_log_likelihood_error_upper_bound
                                {
                                    Some(current_bound) => current_bound.max(
                                        scope_evaluation
                                            .average_log_likelihood_error_upper_bound,
                                    ),
                                    None => scope_evaluation
                                        .average_log_likelihood_error_upper_bound,
                                });
                            diagnostics_summary
                                .evaluator_block_counts
                                .add_assign(scope_evaluation.evaluator_block_counts);

                            f64::INFINITY
                        }
                    })
                })
                .collect::<Vec<_>>()
                .as_slice(),
        )
    };

    if diagnostics_summary.failures.iter().any(|failure| {
        failure.failure_reason
            == ReturnComponentsStageExactLikelihoodFailureReason::LogDensityErrorNotCertified
    }) {
        diagnostics_summary.uncertified_scope_count = diagnostics_summary
            .failures
            .iter()
            .filter(|failure| {
                failure.failure_reason
                    == ReturnComponentsStageExactLikelihoodFailureReason::LogDensityErrorNotCertified
            })
            .count();

        diagnostics_summary.structural_failure_count =
            diagnostics_summary.failures.len() - diagnostics_summary.uncertified_scope_count;
    }

    let log_likelihood_shift = optimizer_context.log_likelihood_shift;

    let blended_negative_log_likelihood = arithmetic_mean(
        [
            global_negative_log_likelihood,
            conditional_negative_log_likelihood,
        ]
        .as_slice(),
    );

    (
        ContinuousLikelihoodObjective {
            shifted_objective_score: log_likelihood_shift
                + blended_negative_log_likelihood
                + boundary_penalty,
            global_negative_log_likelihood,
            conditional_negative_log_likelihood,
            boundary_penalty,
            log_likelihood_shift,
        },
        diagnostics_summary,
    )
}

fn average_negative_log_likelihood_with_failures(
    configuration: NigSharedPlusShiftConfiguration,
    optimizer_context: &ContinuousReturnOptimizerContext,
    member_indices: &[usize],
    scope: ReturnComponentsStageExactLikelihoodFailureScope,
    scale_bucket_index: Option<usize>,
    exact_block_cache: &ExactBlockEvaluationCache,
) -> Result<ExactLikelihoodScopeEvaluation, ExactLikelihoodScopeFailure> {
    let driver_parameters = configuration.derive_driver_parameters().map_err(|_| {
        ExactLikelihoodScopeFailure::Structural(Box::new(
            build_exact_likelihood_failure_diagnostic(
                scope,
                scale_bucket_index,
                None,
                ReturnComponentsStageExactLikelihoodFailureReason::InvalidObservedInputs,
                None,
                None,
            ),
        ))
    })?;

    if member_indices.is_empty() {
        return Err(ExactLikelihoodScopeFailure::Structural(Box::new(
            build_exact_likelihood_failure_diagnostic(
                scope,
                scale_bucket_index,
                None,
                ReturnComponentsStageExactLikelihoodFailureReason::NoEvaluatedBlocks,
                None,
                None,
            ),
        )));
    }

    let Some(member_weight_sum) = likelihood_member_weight_sum(optimizer_context, member_indices)
    else {
        return Err(ExactLikelihoodScopeFailure::Structural(Box::new(
            build_exact_likelihood_failure_diagnostic(
                scope,
                scale_bucket_index,
                None,
                ReturnComponentsStageExactLikelihoodFailureReason::InvalidObservedInputs,
                None,
                None,
            ),
        )));
    };
    let log_density_resolution =
        average_negative_log_likelihood_resolution_for_weight_sum(member_weight_sum);

    let block_contributions = exact_likelihood_block_contributions(
        configuration,
        driver_parameters,
        optimizer_context,
        member_indices,
        scope,
        scale_bucket_index,
        log_density_resolution,
        exact_block_cache,
    );

    let mut weighted_log_likelihood_sum = 0.0;
    let mut weighted_log_likelihood_error_upper_bound_sum = 0.0;
    let mut evaluator_block_counts = ExactEvaluatorBlockCounts::default();
    let mut worst_block_member_index = None;
    let mut worst_block_evaluation: Option<ExactBlockLikelihoodEvaluation> = None;

    for block_contribution in block_contributions {
        let block_contribution = block_contribution?;
        let member_index = block_contribution.member_index;
        let member_weight = block_contribution.member_weight;
        let block_evaluation = block_contribution.block_evaluation;

        match block_evaluation.evaluator_kind {
            ReturnComponentsStageExactLikelihoodEvaluatorKind::ClosedFormSingleSideBuy => {
                evaluator_block_counts.closed_form_single_side_buy_block_count += 1;
            }
            ReturnComponentsStageExactLikelihoodEvaluatorKind::ClosedFormSingleSideSell => {
                evaluator_block_counts.closed_form_single_side_sell_block_count += 1;
            }
            ReturnComponentsStageExactLikelihoodEvaluatorKind::TiltedMixedInversion => {
                evaluator_block_counts.tilted_mixed_inversion_block_count += 1;
            }
        }

        weighted_log_likelihood_sum += member_weight * block_evaluation.log_density_estimate;
        weighted_log_likelihood_error_upper_bound_sum +=
            member_weight * block_evaluation.log_density_error_upper_bound;

        let should_replace_worst_block = match worst_block_evaluation {
            Some(current_worst) => {
                block_evaluation.log_density_error_upper_bound
                    > current_worst.log_density_error_upper_bound
            }
            None => true,
        };
        if should_replace_worst_block {
            worst_block_member_index = Some(member_index);
            worst_block_evaluation = Some(block_evaluation);
        }
    }

    let average_negative_log_likelihood_estimate = -weighted_log_likelihood_sum / member_weight_sum;

    let average_log_likelihood_error_upper_bound =
        weighted_log_likelihood_error_upper_bound_sum / member_weight_sum;

    let scope_evaluation = ExactLikelihoodScopeEvaluation {
        average_negative_log_likelihood_estimate,
        average_log_likelihood_error_upper_bound,
        evaluator_block_counts,
        worst_block_evaluation: worst_block_evaluation.ok_or_else(|| {
            ExactLikelihoodScopeFailure::Structural(Box::new(
                build_exact_likelihood_failure_diagnostic(
                    scope,
                    scale_bucket_index,
                    None,
                    ReturnComponentsStageExactLikelihoodFailureReason::NoEvaluatedBlocks,
                    None,
                    None,
                ),
            ))
        })?,
        worst_block_member_index: worst_block_member_index.ok_or_else(|| {
            ExactLikelihoodScopeFailure::Structural(Box::new(
                build_exact_likelihood_failure_diagnostic(
                    scope,
                    scale_bucket_index,
                    None,
                    ReturnComponentsStageExactLikelihoodFailureReason::NoEvaluatedBlocks,
                    None,
                    None,
                ),
            ))
        })?,
    };

    if scope_evaluation.average_log_likelihood_error_upper_bound > log_density_resolution {
        return Err(ExactLikelihoodScopeFailure::Uncertified {
            diagnostic: Box::new(build_exact_likelihood_failure_diagnostic(
                scope,
                scale_bucket_index,
                Some(scope_evaluation.worst_block_member_index),
                ReturnComponentsStageExactLikelihoodFailureReason::LogDensityErrorNotCertified,
                Some(ExactLikelihoodFailureContext {
                    evaluator_kind: Some(scope_evaluation.worst_block_evaluation.evaluator_kind),
                    observed_return: optimizer_context.observed_returns
                        [scope_evaluation.worst_block_member_index],
                    business_time_scale: optimizer_context.business_time_scales
                        [scope_evaluation.worst_block_member_index],
                    sell_share_proxy: optimizer_context.sell_share_proxies
                        [scope_evaluation.worst_block_member_index],
                    integrated_buy_intensity: optimizer_context.integrated_buy_intensities
                        [scope_evaluation.worst_block_member_index],
                    integrated_sell_intensity: optimizer_context.integrated_sell_intensities
                        [scope_evaluation.worst_block_member_index],
                    block_standard_deviation: Some(
                        scope_evaluation
                            .worst_block_evaluation
                            .block_standard_deviation,
                    ),
                    standardized_return: Some(
                        scope_evaluation.worst_block_evaluation.standardized_return,
                    ),
                    tilt_theta: scope_evaluation.worst_block_evaluation.tilt_theta,
                    tilt_domain_lower_theta_exclusive: scope_evaluation
                        .worst_block_evaluation
                        .tilt_domain_lower_theta_exclusive,
                    tilt_domain_upper_theta_exclusive: scope_evaluation
                        .worst_block_evaluation
                        .tilt_domain_upper_theta_exclusive,
                    standardized_decay_scale: scope_evaluation
                        .worst_block_evaluation
                        .standardized_decay_scale,
                    integration_upper_bound: scope_evaluation
                        .worst_block_evaluation
                        .integration_upper_bound,
                    log_density_resolution,
                }),
                Some(scope_evaluation.average_log_likelihood_error_upper_bound),
            )),
            scope_evaluation: Box::new(scope_evaluation),
        });
    }

    Ok(scope_evaluation)
}

fn exact_likelihood_block_contributions(
    configuration: NigSharedPlusShiftConfiguration,
    driver_parameters: janiform_model_kernels::nig::NigBuySellDriverParameters,
    optimizer_context: &ContinuousReturnOptimizerContext,
    member_indices: &[usize],
    scope: ReturnComponentsStageExactLikelihoodFailureScope,
    scale_bucket_index: Option<usize>,
    log_density_resolution: f64,
    exact_block_cache: &ExactBlockEvaluationCache,
) -> Vec<Result<ExactLikelihoodBlockContribution, ExactLikelihoodScopeFailure>> {
    let evaluate_member = |member_index: &usize| {
        exact_likelihood_block_contribution(
            configuration,
            driver_parameters,
            optimizer_context,
            *member_index,
            scope,
            scale_bucket_index,
            log_density_resolution,
            exact_block_cache,
        )
    };

    if member_indices.len() > 1 {
        let parallel_chunk_len = member_indices
            .len()
            .div_ceil(rayon::current_num_threads().max(1))
            .max(1);

        return member_indices
            .par_iter()
            .with_min_len(parallel_chunk_len)
            .map(evaluate_member)
            .collect();
    }

    member_indices.iter().map(evaluate_member).collect()
}

fn exact_likelihood_block_contribution(
    configuration: NigSharedPlusShiftConfiguration,
    driver_parameters: janiform_model_kernels::nig::NigBuySellDriverParameters,
    optimizer_context: &ContinuousReturnOptimizerContext,
    member_index: usize,
    scope: ReturnComponentsStageExactLikelihoodFailureScope,
    scale_bucket_index: Option<usize>,
    log_density_resolution: f64,
    exact_block_cache: &ExactBlockEvaluationCache,
) -> Result<ExactLikelihoodBlockContribution, ExactLikelihoodScopeFailure> {
    let observed_return = exact_likelihood_member_value(
        optimizer_context.observed_returns.as_slice(),
        member_index,
        scope,
        scale_bucket_index,
    )?;

    let business_time_scale = exact_likelihood_member_value(
        optimizer_context.business_time_scales.as_slice(),
        member_index,
        scope,
        scale_bucket_index,
    )?;

    let sell_share_proxy = exact_likelihood_member_value(
        optimizer_context.sell_share_proxies.as_slice(),
        member_index,
        scope,
        scale_bucket_index,
    )?;

    let integrated_buy_intensity = exact_likelihood_member_value(
        optimizer_context.integrated_buy_intensities.as_slice(),
        member_index,
        scope,
        scale_bucket_index,
    )?;

    let integrated_sell_intensity = exact_likelihood_member_value(
        optimizer_context.integrated_sell_intensities.as_slice(),
        member_index,
        scope,
        scale_bucket_index,
    )?;

    let block_input = build_exact_block_input(ExactBlockInputBuildInput {
        configuration,
        observed_return,
        business_time_scale,
        sell_share_proxy,
        integrated_buy_intensity,
        integrated_sell_intensity,
        block_member_index: member_index,
        log_density_resolution,
    })
    .map_err(|failure| {
        ExactLikelihoodScopeFailure::Structural(Box::new(
            build_exact_likelihood_failure_diagnostic(
                scope,
                scale_bucket_index,
                Some(failure.block_member_index),
                failure.failure_reason,
                Some(*failure.context),
                None,
            ),
        ))
    })?;

    let cache_key = ExactBlockEvaluationCacheKey::from_block_input(&block_input);
    if let Some(block_evaluation) =
        cached_exact_block_evaluation(exact_block_cache, cache_key, log_density_resolution)
    {
        return Ok(ExactLikelihoodBlockContribution {
            member_index,
            member_weight: exact_likelihood_member_weight(optimizer_context, member_index),
            block_evaluation,
        });
    }

    let block_evaluation = evaluate_exact_block_likelihood(
        configuration,
        driver_parameters,
        block_input,
        log_density_resolution,
    )
    .map_err(|failure| {
        ExactLikelihoodScopeFailure::Structural(Box::new(
            build_exact_likelihood_failure_diagnostic(
                scope,
                scale_bucket_index,
                Some(failure.block_member_index),
                failure.failure_reason,
                Some(*failure.context),
                None,
            ),
        ))
    })?;

    record_exact_block_evaluation(exact_block_cache, cache_key, block_evaluation);

    Ok(ExactLikelihoodBlockContribution {
        member_index,
        member_weight: exact_likelihood_member_weight(optimizer_context, member_index),
        block_evaluation,
    })
}

fn likelihood_member_weight_sum(
    optimizer_context: &ContinuousReturnOptimizerContext,
    member_indices: &[usize],
) -> Option<f64> {
    let member_weight_sum = member_indices
        .iter()
        .map(|member_index| exact_likelihood_member_weight(optimizer_context, *member_index))
        .sum::<f64>();

    (member_weight_sum.is_finite() && member_weight_sum > 0.0).then_some(member_weight_sum)
}

fn exact_likelihood_member_weight(
    optimizer_context: &ContinuousReturnOptimizerContext,
    member_index: usize,
) -> f64 {
    optimizer_context
        .member_weights
        .get(member_index)
        .copied()
        .filter(|weight| weight.is_finite() && *weight > 0.0)
        .unwrap_or(1.0)
}

fn exact_likelihood_member_value(
    values: &[f64],
    member_index: usize,
    scope: ReturnComponentsStageExactLikelihoodFailureScope,
    scale_bucket_index: Option<usize>,
) -> Result<f64, ExactLikelihoodScopeFailure> {
    values.get(member_index).copied().ok_or_else(|| {
        ExactLikelihoodScopeFailure::Structural(Box::new(
            build_exact_likelihood_failure_diagnostic(
                scope,
                scale_bucket_index,
                Some(member_index),
                ReturnComponentsStageExactLikelihoodFailureReason::InvalidObservedInputs,
                None,
                None,
            ),
        ))
    })
}

pub(super) fn average_negative_log_likelihood_resolution(sample_count: usize) -> f64 {
    average_negative_log_likelihood_resolution_for_weight_sum(sample_count.max(1) as f64)
}

fn average_negative_log_likelihood_resolution_for_weight_sum(sample_weight_sum: f64) -> f64 {
    RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_PROFILE_LIKELIHOOD_CHI_SQUARE_95
        / (2.0 * sample_weight_sum.max(1.0))
}

#[cfg(test)]
#[derive(Debug, Clone, Copy)]
pub(super) struct NetTradingTimeReturnLogDensityInput {
    pub(super) driver_parameters: janiform_model_kernels::nig::NigBuySellDriverParameters,
    pub(super) configuration: NigSharedPlusShiftConfiguration,
    pub(super) observed_return: f64,
    pub(super) business_time_scale: f64,
    pub(super) sell_share_proxy: f64,
    pub(super) integrated_buy_intensity: f64,
    pub(super) integrated_sell_intensity: f64,
    pub(super) log_density_resolution: f64,
}

#[cfg(test)]
pub(super) fn log_density_net_trading_time_return(
    input: NetTradingTimeReturnLogDensityInput,
) -> Option<f64> {
    let NetTradingTimeReturnLogDensityInput {
        driver_parameters,
        configuration,
        observed_return,
        business_time_scale,
        sell_share_proxy,
        integrated_buy_intensity,
        integrated_sell_intensity,
        log_density_resolution,
    } = input;

    let block_input = build_exact_block_input(ExactBlockInputBuildInput {
        configuration,
        observed_return,
        business_time_scale,
        sell_share_proxy,
        integrated_buy_intensity,
        integrated_sell_intensity,
        block_member_index: 0,
        log_density_resolution,
    })
    .ok()?;

    evaluate_exact_block_likelihood(
        configuration,
        driver_parameters,
        block_input,
        log_density_resolution,
    )
    .ok()
    .map(|evaluation| evaluation.log_density_estimate)
}

#[cfg(test)]
pub(super) fn log_density_net_trading_time_return_failure_reason(
    input: NetTradingTimeReturnLogDensityInput,
) -> Option<ReturnComponentsStageExactLikelihoodFailureReason> {
    let NetTradingTimeReturnLogDensityInput {
        driver_parameters,
        configuration,
        observed_return,
        business_time_scale,
        sell_share_proxy,
        integrated_buy_intensity,
        integrated_sell_intensity,
        log_density_resolution,
    } = input;

    let block_input = build_exact_block_input(ExactBlockInputBuildInput {
        configuration,
        observed_return,
        business_time_scale,
        sell_share_proxy,
        integrated_buy_intensity,
        integrated_sell_intensity,
        block_member_index: 0,
        log_density_resolution,
    })
    .ok()?;

    evaluate_exact_block_likelihood(
        configuration,
        driver_parameters,
        block_input,
        log_density_resolution,
    )
    .err()
    .map(|failure| failure.failure_reason)
}

pub(super) fn alpha_beta_boundary_penalty(configuration: NigSharedPlusShiftConfiguration) -> f64 {
    let buy_margin = configuration.alpha - configuration.beta_0.abs();
    let sell_margin = configuration.alpha - configuration.derived_sell_beta().abs();
    if !buy_margin.is_finite() || !sell_margin.is_finite() {
        return f64::INFINITY;
    }

    if buy_margin <= 0.0 || sell_margin <= 0.0 {
        return f64::INFINITY;
    }

    let probe_gamma_floor = coarse_transform_probe_gamma_floor();
    let minimum_gamma = boundary_distance_gamma(configuration.alpha, configuration.beta_0).min(
        boundary_distance_gamma(configuration.alpha, configuration.derived_sell_beta()),
    );
    if !minimum_gamma.is_finite() || minimum_gamma <= 0.0 {
        return f64::INFINITY;
    }

    if minimum_gamma >= probe_gamma_floor {
        return 0.0;
    }

    (probe_gamma_floor / minimum_gamma).ln()
}

pub(super) fn simulate_matched_scale_returns(
    configuration: NigSharedPlusShiftConfiguration,
    calibration_sample: &ReturnComponentsStageContinuousReturnCalibrationSample,
    stage_seed: u64,
) -> Vec<f64> {
    let Ok(driver_parameters) = configuration.derive_driver_parameters() else {
        return Vec::new();
    };

    let mut randomness_stream = SplitMix64SimulationRandomnessStream::from_seed(
        candidate_sampling_seed(stage_seed, configuration),
    );

    let mut synthetic_returns = Vec::with_capacity(calibration_sample.business_time_scales.len());

    for (business_time_scale, sell_share_proxy) in calibration_sample
        .business_time_scales
        .iter()
        .zip(calibration_sample.sell_share_proxies.iter())
    {
        let sell_weight = sell_share_proxy.clamp(0.0, 1.0);
        let buy_weight = 1.0 - sell_weight;

        let buy_business_time_scale = (business_time_scale * buy_weight).max(0.0);
        let sell_business_time_scale = (business_time_scale * sell_weight).max(0.0);

        let Ok(sampled_increment) = driver_parameters.sample_trading_time_return_increment(
            buy_business_time_scale,
            sell_business_time_scale,
            &mut randomness_stream,
        ) else {
            return Vec::new();
        };

        synthetic_returns.push(sampled_increment.combined_increment);
    }

    synthetic_returns
}

pub(super) fn minimum_open_beta_ratio() -> f64 {
    (-1.0f64).next_up()
}

pub(super) fn maximum_open_beta_ratio() -> f64 {
    1.0f64.next_down()
}

fn member_indices_for_fidelity_tier(
    optimizer_context: &ContinuousReturnOptimizerContext,
    fidelity_tier: ContinuousLikelihoodFidelityTier,
) -> &[usize] {
    match fidelity_tier {
        ContinuousLikelihoodFidelityTier::Seed => optimizer_context.seed_member_indices.as_slice(),
        ContinuousLikelihoodFidelityTier::Representative => {
            optimizer_context.full_member_indices.as_slice()
        }
        ContinuousLikelihoodFidelityTier::Local => {
            optimizer_context.local_member_indices.as_slice()
        }
        ContinuousLikelihoodFidelityTier::Full => optimizer_context.full_member_indices.as_slice(),
    }
}

fn build_exact_likelihood_failure_diagnostic(
    scope: ReturnComponentsStageExactLikelihoodFailureScope,
    scale_bucket_index: Option<usize>,
    block_member_index: Option<usize>,
    failure_reason: ReturnComponentsStageExactLikelihoodFailureReason,
    failure_context: Option<ExactLikelihoodFailureContext>,
    scope_average_log_likelihood_error_upper_bound: Option<f64>,
) -> ReturnComponentsStageExactLikelihoodFailureDiagnostic {
    ReturnComponentsStageExactLikelihoodFailureDiagnostic {
        scope,
        scale_bucket_index,
        block_member_index,
        failure_reason,
        evaluator_kind: failure_context.and_then(|context| context.evaluator_kind),
        observed_return: failure_context.map(|context| context.observed_return),
        business_time_scale: failure_context.map(|context| context.business_time_scale),
        sell_share_proxy: failure_context.map(|context| context.sell_share_proxy),
        integrated_buy_intensity: failure_context.map(|context| context.integrated_buy_intensity),
        integrated_sell_intensity: failure_context.map(|context| context.integrated_sell_intensity),
        block_standard_deviation: failure_context
            .and_then(|context| context.block_standard_deviation),
        standardized_return: failure_context.and_then(|context| context.standardized_return),
        tilt_theta: failure_context.and_then(|context| context.tilt_theta),
        tilt_domain_lower_theta_exclusive: failure_context
            .and_then(|context| context.tilt_domain_lower_theta_exclusive),
        tilt_domain_upper_theta_exclusive: failure_context
            .and_then(|context| context.tilt_domain_upper_theta_exclusive),
        standardized_decay_scale: failure_context
            .and_then(|context| context.standardized_decay_scale),
        integration_upper_bound: failure_context
            .and_then(|context| context.integration_upper_bound),
        log_density_resolution: failure_context.map(|context| context.log_density_resolution),
        scope_average_log_likelihood_error_upper_bound,
    }
}

fn boundary_distance_gamma(alpha: f64, beta: f64) -> f64 {
    let alpha_minus_abs_beta = alpha - beta.abs();
    let alpha_plus_abs_beta = alpha + beta.abs();
    if !alpha_minus_abs_beta.is_finite()
        || !alpha_plus_abs_beta.is_finite()
        || alpha_minus_abs_beta <= 0.0
        || alpha_plus_abs_beta <= 0.0
    {
        return f64::NAN;
    }

    (alpha_minus_abs_beta * alpha_plus_abs_beta).sqrt()
}

fn coarse_transform_probe_gamma_floor() -> f64 {
    SufficientStatisticsTransformSliceSettings::default().a_plus_u
}

fn candidate_sampling_seed(stage_seed: u64, configuration: NigSharedPlusShiftConfiguration) -> u64 {
    let mut mixed = stage_seed ^ 0xC6A4_A793_5BD1_E995;

    for parameter_bits in [
        configuration.alpha.to_bits(),
        configuration.beta_0.to_bits(),
        configuration.delta.to_bits(),
        configuration.mu.to_bits(),
        configuration.delta_beta.to_bits(),
    ] {
        mixed = splitmix64_output_mixer(mixed ^ parameter_bits);
    }

    mixed
}

#[cfg(test)]
pub(super) fn closed_form_single_side_log_density(
    evaluator_kind: ReturnComponentsStageExactLikelihoodEvaluatorKind,
    configuration: NigSharedPlusShiftConfiguration,
    observed_return: f64,
    integrated_intensity: f64,
) -> Option<f64> {
    let driver_parameters = configuration.derive_driver_parameters().ok()?;

    match evaluator_kind {
        ReturnComponentsStageExactLikelihoodEvaluatorKind::ClosedFormSingleSideBuy => {
            super::closed_form::closed_form_nig_log_density(
                driver_parameters.buy_side_parameters,
                observed_return,
                integrated_intensity,
            )
            .ok()
        }
        ReturnComponentsStageExactLikelihoodEvaluatorKind::ClosedFormSingleSideSell => {
            super::closed_form::closed_form_nig_log_density(
                driver_parameters.sell_side_parameters,
                -observed_return,
                integrated_intensity,
            )
            .ok()
        }
        ReturnComponentsStageExactLikelihoodEvaluatorKind::TiltedMixedInversion => None,
    }
}
