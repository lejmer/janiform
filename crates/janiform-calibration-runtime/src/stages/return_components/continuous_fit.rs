use std::{collections::BTreeSet, f64::consts::PI};

use janiform_calibration_artifacts::CalibrationOptimizerSettings;
use janiform_foundations::complex_math::{ComplexScalar, complex_exponential, complex_multiply};
use janiform_foundations::numeric::{
    arithmetic_mean_or_zero as arithmetic_mean, compute_probability_weighted_moment, logistic,
};
use janiform_foundations::randomness::SplitMix64SimulationRandomnessStream;
use janiform_foundations::splitmix64_output_mixer;
use janiform_model_kernels::nig::{NigBuySellDriverParameters, NigSharedPlusShiftConfiguration};
use rayon::prelude::*;

use super::constants::{
    RETURN_COMPONENTS_STAGE_MAX_NIG_CANDIDATES,
    RETURN_COMPONENTS_STAGE_MIN_REPRESENTABLE_POSITIVE_PARAMETER,
};
use super::contracts::{
    ReturnComponentsObservedOpenSessionReturn,
    ReturnComponentsStageContinuousDistributionFitBreakdownDiagnostic,
    ReturnComponentsStageObservedSignalSummaryStatistics,
    ReturnComponentsStageReturnDistributionDiagnostic,
};
use super::execution::{
    ReturnComponentsStageReturnDistributionSummary, ReturnComponentsStageSortedSignalInputs,
    arithmetic_mean_signed, round_negative_candidate_value, round_positive_candidate_value,
    round_signed_candidate_value, sample_adjusted_excess_kurtosis, sample_adjusted_skewness,
    sample_standard_deviation, summarize_return_distribution, theoretical_net_process_moments,
};

const RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_MAX_SAMPLE_COUNT: usize = 2_048;
const RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_SEARCH_SAMPLE_COUNT: usize = 256;
const RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_MIN_BROAD_CANDIDATE_COUNT: usize = 48;
const RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_ELITE_COUNT: usize = 6;
const RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_REFINEMENT_ROUNDS: usize = 2;
const RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_REGIME_COUNT: usize = 7;
const RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_SEARCH_INTEGRATION_STEP_COUNT: usize = 192;
const RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_FINAL_INTEGRATION_STEP_COUNT: usize = 512;
const RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_STANDARDIZED_INTEGRATION_UPPER_BOUND: f64 = 64.0;

#[derive(Debug, Clone, Copy)]
enum ContinuousLikelihoodEvaluationMode {
    Search,
    Full,
}

#[derive(Debug, Clone, Copy)]
struct ContinuousLikelihoodObjective {
    shifted_objective_score: f64,
    global_negative_log_likelihood: f64,
    conditional_negative_log_likelihood: f64,
    boundary_penalty: f64,
    log_likelihood_shift: f64,
}

#[derive(Debug, Clone)]
pub(super) struct ReturnComponentsStageContinuousReturnCalibrationSample {
    pub(super) observed_returns: Vec<f64>,
    pub(super) business_time_scales: Vec<f64>,
    sell_share_proxies: Vec<f64>,
    observed_profile: ContinuousReturnDistributionProfile,
    scale_bucket_profiles: Vec<ContinuousReturnScaleBucketProfile>,
    open_session_sell_share_proxy: f64,
}

#[derive(Debug, Clone)]
pub(super) struct ContinuousReturnCandidateEvaluation {
    pub objective_score: f64,
    pub diagnostic: ReturnComponentsStageReturnDistributionDiagnostic,
    pub fitted_returns: Vec<f64>,
    pub fit_breakdown: ReturnComponentsStageContinuousDistributionFitBreakdownDiagnostic,
}

#[derive(Debug, Clone, Copy)]
struct ContinuousReturnDistributionProfile {
    mean_return: f64,
    return_volatility: f64,
    median_return: f64,
    lower_quartile_return: f64,
    upper_quartile_return: f64,
    lower_tail_return_q05: f64,
    upper_tail_return_q95: f64,
    extreme_left_tail_return_q01: f64,
    extreme_right_tail_return_q99: f64,
    mean_abs_return: f64,
    median_abs_return: f64,
    abs_quantile_ratio_q99_q95: f64,
    bowley_skewness: f64,
    l_moment_lambda2: f64,
    l_moment_tau3: f64,
}

#[derive(Debug, Clone)]
struct ContinuousReturnScaleBucketProfile {
    member_indices: Vec<usize>,
}

#[derive(Debug, Clone, Copy)]
struct NigShapeSearchPoint {
    log_alpha: f64,
    buy_skew_coordinate: f64,
    sell_relative_coordinate: f64,
    log_delta_offset: f64,
}

#[derive(Debug, Clone, Copy)]
struct NigSearchRegime {
    log_alpha_center: f64,
    buy_skew_center: f64,
    sell_relative_center: f64,
    log_delta_center: f64,
    log_alpha_half_span: f64,
    buy_skew_half_span: f64,
    sell_relative_half_span: f64,
    log_delta_half_span: f64,
}

#[derive(Debug, Clone)]
struct ScoredNigCandidate {
    configuration: NigSharedPlusShiftConfiguration,
    objective_score: f64,
}

pub(super) fn build_continuous_return_calibration_sample(
    sorted_signals: &ReturnComponentsStageSortedSignalInputs,
    continuous_fit_business_time_scales: &[f64],
    open_session_sell_share_proxy: f64,
) -> ReturnComponentsStageContinuousReturnCalibrationSample {
    let aligned_pairs = build_equal_business_time_blocks(
        sorted_signals.continuous_fit_returns.as_slice(),
        continuous_fit_business_time_scales,
        RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_MAX_SAMPLE_COUNT,
    );
    let observed_returns: Vec<f64> = aligned_pairs
        .iter()
        .map(|(observation, _)| observation.log_return)
        .collect();
    let business_time_scales: Vec<f64> = aligned_pairs.iter().map(|(_, scale)| *scale).collect();
    let sell_share_proxies: Vec<f64> = aligned_pairs
        .iter()
        .map(|(observation, _)| observation.sell_share_proxy)
        .collect();
    let observed_profile = build_distribution_profile(observed_returns.as_slice());
    let scale_bucket_profiles =
        build_scale_bucket_profiles(observed_returns.as_slice(), business_time_scales.as_slice());

    ReturnComponentsStageContinuousReturnCalibrationSample {
        observed_returns,
        business_time_scales,
        sell_share_proxies,
        observed_profile,
        scale_bucket_profiles,
        open_session_sell_share_proxy,
    }
}

pub(super) fn summarize_observed_continuous_return_calibration_sample(
    calibration_sample: &ReturnComponentsStageContinuousReturnCalibrationSample,
) -> ReturnComponentsStageReturnDistributionSummary {
    summarize_return_distribution(
        calibration_sample.observed_returns.as_slice(),
        calibration_sample.business_time_scales.as_slice(),
    )
}

pub(super) fn continuous_return_calibration_sample_count(
    calibration_sample: &ReturnComponentsStageContinuousReturnCalibrationSample,
) -> usize {
    calibration_sample.observed_returns.len()
}

pub(super) fn mean_continuous_return_calibration_business_time_scale(
    calibration_sample: &ReturnComponentsStageContinuousReturnCalibrationSample,
) -> f64 {
    arithmetic_mean(calibration_sample.business_time_scales.as_slice())
}

pub(super) fn build_nig_search_candidates(
    summary_statistics: ReturnComponentsStageObservedSignalSummaryStatistics,
    calibration_sample: &ReturnComponentsStageContinuousReturnCalibrationSample,
    _stage_seed: u64,
    solver_settings: &CalibrationOptimizerSettings,
) -> Vec<NigSharedPlusShiftConfiguration> {
    let broad_candidate_count = solver_settings
        .max_iterations
        .max(RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_MIN_BROAD_CANDIDATE_COUNT);
    let local_candidate_count = (broad_candidate_count / 2).max(24);
    let mut scored_candidates = score_search_points(
        summary_statistics,
        calibration_sample,
        build_broad_search_points(calibration_sample, broad_candidate_count).as_slice(),
        solver_settings.cpu_count,
    );

    for refinement_round in 0..RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_REFINEMENT_ROUNDS {
        let elite_points = elite_search_points(
            scored_candidates.as_slice(),
            summary_statistics,
            calibration_sample.open_session_sell_share_proxy,
        );
        let refinement_scale = 0.28 / (refinement_round + 1) as f64;
        let local_points = build_local_search_points(
            elite_points.as_slice(),
            local_candidate_count,
            refinement_scale,
        );
        scored_candidates.extend(score_search_points(
            summary_statistics,
            calibration_sample,
            local_points.as_slice(),
            solver_settings.cpu_count,
        ));
    }

    let mut ranked_candidates = deduplicate_scored_candidates(scored_candidates);
    ranked_candidates.sort_by(|left, right| {
        left.objective_score
            .total_cmp(&right.objective_score)
            .then_with(|| {
                left.configuration
                    .alpha
                    .total_cmp(&right.configuration.alpha)
            })
            .then_with(|| {
                left.configuration
                    .beta_0
                    .total_cmp(&right.configuration.beta_0)
            })
            .then_with(|| {
                left.configuration
                    .delta
                    .total_cmp(&right.configuration.delta)
            })
            .then_with(|| left.configuration.mu.total_cmp(&right.configuration.mu))
            .then_with(|| {
                left.configuration
                    .delta_beta
                    .total_cmp(&right.configuration.delta_beta)
            })
    });
    ranked_candidates.truncate(RETURN_COMPONENTS_STAGE_MAX_NIG_CANDIDATES);

    ranked_candidates
        .into_iter()
        .map(|candidate| candidate.configuration)
        .collect()
}

pub(super) fn evaluate_continuous_nig_candidate(
    configuration: NigSharedPlusShiftConfiguration,
    calibration_sample: &ReturnComponentsStageContinuousReturnCalibrationSample,
    summary_statistics: ReturnComponentsStageObservedSignalSummaryStatistics,
    stage_seed: u64,
) -> ContinuousReturnCandidateEvaluation {
    let likelihood_objective = continuous_block_likelihood_objective(
        configuration,
        calibration_sample,
        ContinuousLikelihoodEvaluationMode::Full,
    );
    let fitted_returns =
        simulate_matched_scale_returns(configuration, calibration_sample, stage_seed);
    let fitted_profile = build_distribution_profile(fitted_returns.as_slice());
    let profile_fit_breakdown =
        distribution_profile_fit_breakdown(fitted_profile, calibration_sample.observed_profile);
    let fitted_mean_return = arithmetic_mean_signed(fitted_returns.as_slice());
    let fitted_return_volatility = sample_standard_deviation(fitted_returns.as_slice());
    let fitted_skewness = sample_adjusted_skewness(fitted_returns.as_slice()).unwrap_or(0.0);
    let fitted_excess_kurtosis =
        sample_adjusted_excess_kurtosis(fitted_returns.as_slice()).unwrap_or(0.0);

    ContinuousReturnCandidateEvaluation {
        objective_score: likelihood_objective.shifted_objective_score,
        diagnostic: ReturnComponentsStageReturnDistributionDiagnostic {
            sample_count: summary_statistics.continuous_fit_return_count,
            observed_mean_return: summary_statistics.continuous_return_mean,
            fitted_mean_return,
            observed_return_volatility: summary_statistics.continuous_return_volatility,
            fitted_return_volatility,
            observed_skewness: summary_statistics.continuous_return_skewness,
            fitted_skewness,
            observed_excess_kurtosis: summary_statistics.continuous_return_excess_kurtosis,
            fitted_excess_kurtosis,
        },
        fitted_returns,
        fit_breakdown: ReturnComponentsStageContinuousDistributionFitBreakdownDiagnostic {
            global_fit_score: likelihood_objective.global_negative_log_likelihood,
            conditional_fit_score: likelihood_objective.conditional_negative_log_likelihood,
            boundary_penalty: likelihood_objective.boundary_penalty,
            profile_fit_score: profile_fit_breakdown.global_fit_score,
            log_likelihood_shift: likelihood_objective.log_likelihood_shift,
            ..profile_fit_breakdown
        },
    }
}

fn build_equal_business_time_blocks(
    continuous_fit_returns: &[ReturnComponentsObservedOpenSessionReturn],
    business_time_scales: &[f64],
    maximum_count: usize,
) -> Vec<(ReturnComponentsObservedOpenSessionReturn, f64)> {
    let aligned_count = continuous_fit_returns.len().min(business_time_scales.len());
    let aligned_pairs: Vec<(ReturnComponentsObservedOpenSessionReturn, f64)> =
        continuous_fit_returns
            .iter()
            .copied()
            .zip(business_time_scales.iter().copied())
            .take(aligned_count)
            .collect();
    if aligned_pairs.is_empty() {
        return Vec::new();
    }
    if aligned_pairs.len() == 1 || aligned_pairs.len() <= maximum_count {
        return aligned_pairs;
    }

    let total_business_time: f64 = aligned_pairs
        .iter()
        .map(|(_, business_time_scale)| {
            if business_time_scale.is_finite() && *business_time_scale > 0.0 {
                *business_time_scale
            } else {
                0.0
            }
        })
        .sum();
    if total_business_time <= 0.0 {
        return aligned_pairs;
    }

    let target_block_count = maximum_count.min(aligned_pairs.len());
    if target_block_count <= 1 {
        let total_log_return = aligned_pairs
            .iter()
            .map(|(observation, _)| observation.log_return)
            .sum::<f64>();
        let weighted_activity_lift = weighted_average(
            aligned_pairs
                .iter()
                .map(|(observation, business_time_scale)| {
                    (observation.post_shock_activity_lift, *business_time_scale)
                })
                .collect::<Vec<_>>()
                .as_slice(),
        );
        let weighted_sell_share = weighted_average(
            aligned_pairs
                .iter()
                .map(|(observation, business_time_scale)| {
                    (observation.sell_share_proxy, *business_time_scale)
                })
                .collect::<Vec<_>>()
                .as_slice(),
        );
        let last_observation = aligned_pairs
            .last()
            .map(|(observation, _)| *observation)
            .unwrap_or(continuous_fit_returns[0]);
        return vec![(
            ReturnComponentsObservedOpenSessionReturn {
                return_timestamp_utc_nanos: last_observation.return_timestamp_utc_nanos,
                log_return: total_log_return,
                post_shock_activity_lift: weighted_activity_lift,
                sell_share_proxy: weighted_sell_share,
            },
            total_business_time,
        )];
    }

    let mut aggregated_blocks: Vec<(ReturnComponentsObservedOpenSessionReturn, f64)> =
        Vec::with_capacity(target_block_count);
    let mut running_log_return = 0.0;
    let mut running_business_time = 0.0;
    let mut running_weighted_activity_lift = 0.0;
    let mut running_weighted_sell_share = 0.0;
    let mut running_weight_sum = 0.0;
    let mut emitted_block_count = 0usize;
    let mut cumulative_business_time = 0.0;

    for (aligned_index, (observation, business_time_scale)) in aligned_pairs.iter().enumerate() {
        let positive_business_time =
            if business_time_scale.is_finite() && *business_time_scale > 0.0 {
                *business_time_scale
            } else {
                0.0
            };
        running_log_return += observation.log_return;
        running_business_time += positive_business_time;
        running_weighted_activity_lift +=
            observation.post_shock_activity_lift * positive_business_time;
        running_weighted_sell_share += observation.sell_share_proxy * positive_business_time;
        running_weight_sum += positive_business_time;
        cumulative_business_time += positive_business_time;

        let remaining_observation_count = aligned_pairs.len().saturating_sub(aligned_index + 1);
        let remaining_block_count = target_block_count.saturating_sub(emitted_block_count + 1);
        let should_emit = aligned_index + 1 == aligned_pairs.len()
            || remaining_observation_count == remaining_block_count
            || cumulative_business_time
                >= total_business_time * (emitted_block_count + 1) as f64
                    / target_block_count as f64;
        if !should_emit {
            continue;
        }

        let weight_denominator = running_weight_sum.max(1.0);
        aggregated_blocks.push((
            ReturnComponentsObservedOpenSessionReturn {
                return_timestamp_utc_nanos: observation.return_timestamp_utc_nanos,
                log_return: running_log_return,
                post_shock_activity_lift: running_weighted_activity_lift / weight_denominator,
                sell_share_proxy: (running_weighted_sell_share / weight_denominator)
                    .clamp(0.0, 1.0),
            },
            running_business_time.max(RETURN_COMPONENTS_STAGE_MIN_REPRESENTABLE_POSITIVE_PARAMETER),
        ));
        emitted_block_count += 1;
        running_log_return = 0.0;
        running_business_time = 0.0;
        running_weighted_activity_lift = 0.0;
        running_weighted_sell_share = 0.0;
        running_weight_sum = 0.0;
    }

    aggregated_blocks
}

fn weighted_average(values_and_weights: &[(f64, f64)]) -> f64 {
    let total_weight: f64 = values_and_weights
        .iter()
        .map(|(_, weight)| {
            if weight.is_finite() && *weight > 0.0 {
                *weight
            } else {
                0.0
            }
        })
        .sum();
    if total_weight <= 0.0 {
        return arithmetic_mean(
            values_and_weights
                .iter()
                .map(|(value, _)| *value)
                .collect::<Vec<_>>()
                .as_slice(),
        );
    }

    values_and_weights
        .iter()
        .map(|(value, weight)| {
            let finite_weight = if weight.is_finite() && *weight > 0.0 {
                *weight
            } else {
                0.0
            };
            value * finite_weight
        })
        .sum::<f64>()
        / total_weight
}

fn build_broad_search_points(
    calibration_sample: &ReturnComponentsStageContinuousReturnCalibrationSample,
    broad_candidate_count: usize,
) -> Vec<NigShapeSearchPoint> {
    let observed_profile = calibration_sample.observed_profile;
    let log_alpha_anchor = inferred_log_alpha_anchor(observed_profile);
    let buy_skew_coordinate_anchor = inferred_buy_skew_coordinate(
        observed_profile,
        calibration_sample.open_session_sell_share_proxy,
    );
    let alpha_span = 1.75 + observed_profile.abs_quantile_ratio_q99_q95.ln_1p();
    let skew_span = 1.25
        + observed_profile
            .bowley_skewness
            .abs()
            .max(observed_profile.l_moment_tau3.abs());
    let log_delta_span = 1.75 + observed_profile.abs_quantile_ratio_q99_q95.ln_1p();
    let regimes = build_search_regimes(
        log_alpha_anchor,
        buy_skew_coordinate_anchor,
        alpha_span,
        skew_span,
        log_delta_span,
    );
    let mut search_points: Vec<NigShapeSearchPoint> = regimes
        .iter()
        .map(|regime| NigShapeSearchPoint {
            log_alpha: regime.log_alpha_center,
            buy_skew_coordinate: regime.buy_skew_center,
            sell_relative_coordinate: regime.sell_relative_center,
            log_delta_offset: regime.log_delta_center,
        })
        .collect();
    let extra_point_count = broad_candidate_count.saturating_sub(search_points.len());

    for sequence_index in 0..extra_point_count {
        let regime_index = sequence_index % regimes.len();
        let regime_sample_index = sequence_index / regimes.len();
        search_points.push(sample_regime_search_point(
            regimes[regime_index],
            regime_sample_index,
            regime_index,
        ));
    }

    search_points
}

fn build_search_regimes(
    log_alpha_anchor: f64,
    buy_skew_coordinate_anchor: f64,
    alpha_span: f64,
    skew_span: f64,
    log_delta_span: f64,
) -> [NigSearchRegime; RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_REGIME_COUNT] {
    [
        NigSearchRegime {
            log_alpha_center: log_alpha_anchor,
            buy_skew_center: buy_skew_coordinate_anchor,
            sell_relative_center: 0.0,
            log_delta_center: 0.0,
            log_alpha_half_span: 0.50 * alpha_span,
            buy_skew_half_span: 0.50 * skew_span,
            sell_relative_half_span: 1.25,
            log_delta_half_span: 0.50 * log_delta_span,
        },
        NigSearchRegime {
            log_alpha_center: log_alpha_anchor + 1.15 * alpha_span,
            buy_skew_center: 0.0,
            sell_relative_center: 0.0,
            log_delta_center: 0.45 * log_delta_span,
            log_alpha_half_span: 0.45 * alpha_span,
            buy_skew_half_span: 0.60 * skew_span,
            sell_relative_half_span: 1.00,
            log_delta_half_span: 0.55 * log_delta_span,
        },
        NigSearchRegime {
            log_alpha_center: log_alpha_anchor - 1.10 * alpha_span,
            buy_skew_center: 0.0,
            sell_relative_center: 0.0,
            log_delta_center: 0.25 * log_delta_span,
            log_alpha_half_span: 0.60 * alpha_span,
            buy_skew_half_span: 0.85 * skew_span,
            sell_relative_half_span: 1.25,
            log_delta_half_span: 0.75 * log_delta_span,
        },
        NigSearchRegime {
            log_alpha_center: log_alpha_anchor - 0.45 * alpha_span,
            buy_skew_center: buy_skew_coordinate_anchor,
            sell_relative_center: 0.0,
            log_delta_center: 0.90 * log_delta_span,
            log_alpha_half_span: 0.50 * alpha_span,
            buy_skew_half_span: 0.70 * skew_span,
            sell_relative_half_span: 1.10,
            log_delta_half_span: 0.60 * log_delta_span,
        },
        NigSearchRegime {
            log_alpha_center: log_alpha_anchor + 0.30 * alpha_span,
            buy_skew_center: buy_skew_coordinate_anchor + 0.70 * skew_span,
            sell_relative_center: 1.50,
            log_delta_center: 0.10 * log_delta_span,
            log_alpha_half_span: 0.45 * alpha_span,
            buy_skew_half_span: 0.55 * skew_span,
            sell_relative_half_span: 0.90,
            log_delta_half_span: 0.50 * log_delta_span,
        },
        NigSearchRegime {
            log_alpha_center: log_alpha_anchor + 0.30 * alpha_span,
            buy_skew_center: buy_skew_coordinate_anchor - 0.70 * skew_span,
            sell_relative_center: -1.50,
            log_delta_center: 0.10 * log_delta_span,
            log_alpha_half_span: 0.45 * alpha_span,
            buy_skew_half_span: 0.55 * skew_span,
            sell_relative_half_span: 0.90,
            log_delta_half_span: 0.50 * log_delta_span,
        },
        NigSearchRegime {
            log_alpha_center: log_alpha_anchor + 1.50 * alpha_span,
            buy_skew_center: 0.0,
            sell_relative_center: 0.0,
            log_delta_center: -0.30 * log_delta_span,
            log_alpha_half_span: 0.40 * alpha_span,
            buy_skew_half_span: 0.45 * skew_span,
            sell_relative_half_span: 0.85,
            log_delta_half_span: 0.50 * log_delta_span,
        },
    ]
}

fn sample_regime_search_point(
    regime: NigSearchRegime,
    regime_sample_index: usize,
    regime_index: usize,
) -> NigShapeSearchPoint {
    let sequence_index = regime_sample_index + regime_index * 997;

    NigShapeSearchPoint {
        log_alpha: regime.log_alpha_center
            + regime.log_alpha_half_span
                * centered_low_discrepancy_perturbation(sequence_index, 2, 3, 5),
        buy_skew_coordinate: regime.buy_skew_center
            + regime.buy_skew_half_span
                * centered_low_discrepancy_perturbation(sequence_index + 17, 7, 11, 13),
        sell_relative_coordinate: regime.sell_relative_center
            + regime.sell_relative_half_span
                * centered_low_discrepancy_perturbation(sequence_index + 37, 17, 19, 23),
        log_delta_offset: regime.log_delta_center
            + regime.log_delta_half_span
                * centered_low_discrepancy_perturbation(sequence_index + 53, 29, 31, 37),
    }
}

fn build_local_search_points(
    elite_points: &[NigShapeSearchPoint],
    points_per_elite: usize,
    refinement_scale: f64,
) -> Vec<NigShapeSearchPoint> {
    let mut local_points = Vec::new();

    for (elite_index, elite_point) in elite_points.iter().enumerate() {
        local_points.push(*elite_point);
        for local_index in 0..points_per_elite {
            let sequence_index = elite_index * points_per_elite + local_index + 1;
            local_points.push(NigShapeSearchPoint {
                log_alpha: elite_point.log_alpha
                    + refinement_scale
                        * centered_low_discrepancy_perturbation(sequence_index, 2, 3, 5),
                buy_skew_coordinate: elite_point.buy_skew_coordinate
                    + refinement_scale
                        * centered_low_discrepancy_perturbation(sequence_index + 11, 7, 11, 13),
                sell_relative_coordinate: elite_point.sell_relative_coordinate
                    + refinement_scale
                        * centered_low_discrepancy_perturbation(sequence_index + 23, 17, 19, 23),
                log_delta_offset: elite_point.log_delta_offset
                    + refinement_scale
                        * centered_low_discrepancy_perturbation(sequence_index + 41, 29, 31, 37),
            });
        }
    }

    local_points
}

fn centered_low_discrepancy_perturbation(
    sequence_index: usize,
    base_a: u64,
    base_b: u64,
    base_c: u64,
) -> f64 {
    van_der_corput(sequence_index as u64 + 1, base_a)
        + van_der_corput(sequence_index as u64 + 1, base_b)
        + van_der_corput(sequence_index as u64 + 1, base_c)
        - 1.5
}

fn van_der_corput(mut index: u64, base: u64) -> f64 {
    let mut fraction = 1.0 / base as f64;
    let mut value = 0.0;

    while index > 0 {
        value += fraction * (index % base) as f64;
        index /= base;
        fraction /= base as f64;
    }

    value
}

fn score_search_points(
    summary_statistics: ReturnComponentsStageObservedSignalSummaryStatistics,
    calibration_sample: &ReturnComponentsStageContinuousReturnCalibrationSample,
    search_points: &[NigShapeSearchPoint],
    cpu_count: usize,
) -> Vec<ScoredNigCandidate> {
    if search_points.is_empty() {
        return Vec::new();
    }

    let score_points = || {
        search_points
            .par_iter()
            .filter_map(|search_point| {
                build_scored_candidate(summary_statistics, calibration_sample, *search_point)
            })
            .collect::<Vec<_>>()
    };

    if cpu_count <= 1 {
        return search_points
            .iter()
            .filter_map(|search_point| {
                build_scored_candidate(summary_statistics, calibration_sample, *search_point)
            })
            .collect();
    }

    match rayon::ThreadPoolBuilder::new()
        .num_threads(cpu_count)
        .build()
    {
        Ok(thread_pool) => thread_pool.install(score_points),
        Err(_) => search_points
            .iter()
            .filter_map(|search_point| {
                build_scored_candidate(summary_statistics, calibration_sample, *search_point)
            })
            .collect(),
    }
}

fn elite_search_points(
    scored_candidates: &[ScoredNigCandidate],
    summary_statistics: ReturnComponentsStageObservedSignalSummaryStatistics,
    open_session_sell_share_proxy: f64,
) -> Vec<NigShapeSearchPoint> {
    let mut ranked_candidates = scored_candidates.to_vec();
    ranked_candidates.sort_by(|left, right| {
        left.objective_score
            .total_cmp(&right.objective_score)
            .then_with(|| {
                left.configuration
                    .alpha
                    .total_cmp(&right.configuration.alpha)
            })
            .then_with(|| {
                left.configuration
                    .beta_0
                    .total_cmp(&right.configuration.beta_0)
            })
            .then_with(|| {
                left.configuration
                    .delta_beta
                    .total_cmp(&right.configuration.delta_beta)
            })
    });
    ranked_candidates.truncate(RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_ELITE_COUNT);

    ranked_candidates
        .into_iter()
        .filter_map(|candidate| {
            let alpha = candidate
                .configuration
                .alpha
                .max(RETURN_COMPONENTS_STAGE_MIN_REPRESENTABLE_POSITIVE_PARAMETER);
            let buy_beta_ratio = clamp_open_beta_ratio(candidate.configuration.beta_0 / alpha);
            let sell_beta_ratio =
                clamp_open_beta_ratio(candidate.configuration.derived_sell_beta() / alpha);
            let sell_relative_share =
                sell_relative_share_from_beta_ratios(buy_beta_ratio, sell_beta_ratio)?;
            let delta_center = inferred_delta_center(
                alpha,
                candidate.configuration.beta_0,
                candidate.configuration.delta_beta,
                summary_statistics.continuous_return_volatility,
                open_session_sell_share_proxy,
            )?;

            Some(NigShapeSearchPoint {
                log_alpha: alpha.ln(),
                buy_skew_coordinate: ratio_to_coordinate(buy_beta_ratio),
                sell_relative_coordinate: share_to_coordinate(sell_relative_share),
                log_delta_offset: (candidate.configuration.delta / delta_center).ln(),
            })
        })
        .collect()
}

fn deduplicate_scored_candidates(
    scored_candidates: Vec<ScoredNigCandidate>,
) -> Vec<ScoredNigCandidate> {
    let mut retained = Vec::new();
    let mut seen = BTreeSet::new();

    for scored_candidate in scored_candidates {
        let candidate_key = format!(
            "{:.6}|{:.6}|{:.12}|{:.6}|{:.6}",
            scored_candidate.configuration.alpha,
            scored_candidate.configuration.beta_0,
            scored_candidate.configuration.delta,
            scored_candidate.configuration.mu,
            scored_candidate.configuration.delta_beta,
        );
        if seen.insert(candidate_key) {
            retained.push(scored_candidate);
        }
    }

    retained
}

fn build_scored_candidate(
    summary_statistics: ReturnComponentsStageObservedSignalSummaryStatistics,
    calibration_sample: &ReturnComponentsStageContinuousReturnCalibrationSample,
    shape_search_point: NigShapeSearchPoint,
) -> Option<ScoredNigCandidate> {
    let configuration = configuration_from_shape_search_point(
        summary_statistics,
        calibration_sample.open_session_sell_share_proxy,
        shape_search_point,
    )?;
    let objective_score = continuous_block_likelihood_objective(
        configuration,
        calibration_sample,
        ContinuousLikelihoodEvaluationMode::Search,
    )
    .shifted_objective_score;

    if !objective_score.is_finite() {
        return None;
    }

    Some(ScoredNigCandidate {
        configuration,
        objective_score,
    })
}

fn configuration_from_shape_search_point(
    summary_statistics: ReturnComponentsStageObservedSignalSummaryStatistics,
    open_session_sell_share_proxy: f64,
    shape_search_point: NigShapeSearchPoint,
) -> Option<NigSharedPlusShiftConfiguration> {
    let alpha = shape_search_point.log_alpha.exp();
    if !alpha.is_finite() || alpha <= RETURN_COMPONENTS_STAGE_MIN_REPRESENTABLE_POSITIVE_PARAMETER {
        return None;
    }
    let buy_beta_ratio = coordinate_to_ratio(shape_search_point.buy_skew_coordinate);
    let sell_relative_share = coordinate_to_share(shape_search_point.sell_relative_coordinate);
    let sell_beta_ratio = sell_beta_ratio_from_relative_share(buy_beta_ratio, sell_relative_share);
    let beta_0 = alpha * buy_beta_ratio;
    let sell_beta = alpha * sell_beta_ratio;
    let delta_beta = sell_beta - beta_0;
    let delta_center = inferred_delta_center(
        alpha,
        beta_0,
        delta_beta,
        summary_statistics.continuous_return_volatility,
        open_session_sell_share_proxy,
    )?;
    let delta = delta_center * shape_search_point.log_delta_offset.exp();

    NigSharedPlusShiftConfiguration::try_new(
        round_positive_candidate_value(alpha),
        round_signed_candidate_value(beta_0),
        round_positive_candidate_value(delta),
        0.0,
        round_negative_candidate_value(delta_beta),
    )
    .ok()
}

fn inferred_delta_center(
    alpha: f64,
    beta_0: f64,
    delta_beta: f64,
    observed_return_volatility: f64,
    open_session_sell_share_proxy: f64,
) -> Option<f64> {
    let (_, variance_per_unit, _, _) = theoretical_net_process_moments(
        NigSharedPlusShiftConfiguration {
            alpha,
            beta_0,
            delta: 1.0,
            mu: 0.0,
            delta_beta,
        },
        open_session_sell_share_proxy,
    )?;
    let target_variance = observed_return_volatility
        .powi(2)
        .max(RETURN_COMPONENTS_STAGE_MIN_REPRESENTABLE_POSITIVE_PARAMETER);
    let variance_per_unit =
        variance_per_unit.max(RETURN_COMPONENTS_STAGE_MIN_REPRESENTABLE_POSITIVE_PARAMETER);
    Some(
        (target_variance / variance_per_unit)
            .max(RETURN_COMPONENTS_STAGE_MIN_REPRESENTABLE_POSITIVE_PARAMETER),
    )
}

fn inferred_log_alpha_anchor(observed_profile: ContinuousReturnDistributionProfile) -> f64 {
    let tail_excess = (observed_profile.abs_quantile_ratio_q99_q95 - 1.0)
        .abs()
        .max(RETURN_COMPONENTS_STAGE_MIN_REPRESENTABLE_POSITIVE_PARAMETER.sqrt());
    let asymmetry_scale =
        1.0 + observed_profile.bowley_skewness.abs() + observed_profile.l_moment_tau3.abs();
    (1.0 + 1.0 / (tail_excess * asymmetry_scale)).ln()
}

fn inferred_buy_skew_coordinate(
    observed_profile: ContinuousReturnDistributionProfile,
    open_session_sell_share_proxy: f64,
) -> f64 {
    let robust_skew_center = 0.5
        * (observed_profile.bowley_skewness + observed_profile.l_moment_tau3)
        + (open_session_sell_share_proxy - 0.5) * 0.25;
    ratio_to_coordinate(clamp_open_beta_ratio(robust_skew_center * 0.60))
}

fn ratio_to_coordinate(ratio: f64) -> f64 {
    let bounded_ratio = clamp_open_beta_ratio(ratio);
    0.5 * ((1.0 + bounded_ratio) / (1.0 - bounded_ratio)).ln()
}

fn coordinate_to_ratio(coordinate: f64) -> f64 {
    clamp_open_beta_ratio(coordinate.tanh())
}

fn share_to_coordinate(share: f64) -> f64 {
    let bounded_share = share.clamp(0.0f64.next_up(), 1.0f64.next_down());
    (bounded_share / (1.0 - bounded_share)).ln()
}

fn coordinate_to_share(coordinate: f64) -> f64 {
    logistic(coordinate).clamp(0.0f64.next_up(), 1.0f64.next_down())
}

fn sell_beta_ratio_from_relative_share(buy_beta_ratio: f64, relative_share: f64) -> f64 {
    let lower_bound = minimum_open_beta_ratio();
    lower_bound + (buy_beta_ratio - lower_bound) * relative_share
}

fn sell_relative_share_from_beta_ratios(buy_beta_ratio: f64, sell_beta_ratio: f64) -> Option<f64> {
    let lower_bound = minimum_open_beta_ratio();
    let denominator = buy_beta_ratio - lower_bound;
    if !denominator.is_finite() || denominator <= 0.0 {
        return None;
    }

    let share = (sell_beta_ratio - lower_bound) / denominator;
    share
        .is_finite()
        .then_some(share.clamp(0.0f64.next_up(), 1.0f64.next_down()))
}

fn clamp_open_beta_ratio(value: f64) -> f64 {
    value.clamp(minimum_open_beta_ratio(), maximum_open_beta_ratio())
}

fn minimum_open_beta_ratio() -> f64 {
    (-1.0f64).next_up()
}

fn maximum_open_beta_ratio() -> f64 {
    1.0f64.next_down()
}

fn continuous_block_likelihood_objective(
    configuration: NigSharedPlusShiftConfiguration,
    calibration_sample: &ReturnComponentsStageContinuousReturnCalibrationSample,
    evaluation_mode: ContinuousLikelihoodEvaluationMode,
) -> ContinuousLikelihoodObjective {
    let boundary_penalty = alpha_beta_boundary_penalty(configuration);
    let integration_step_count = match evaluation_mode {
        ContinuousLikelihoodEvaluationMode::Search => {
            RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_SEARCH_INTEGRATION_STEP_COUNT
        }
        ContinuousLikelihoodEvaluationMode::Full => {
            RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_FINAL_INTEGRATION_STEP_COUNT
        }
    };
    let selected_member_indices = likelihood_member_indices(calibration_sample, evaluation_mode);
    let global_negative_log_likelihood = average_negative_log_likelihood(
        configuration,
        calibration_sample,
        selected_member_indices.as_slice(),
        integration_step_count,
    )
    .unwrap_or(f64::INFINITY);
    let conditional_negative_log_likelihood = if calibration_sample.scale_bucket_profiles.is_empty()
    {
        global_negative_log_likelihood
    } else {
        arithmetic_mean(
            calibration_sample
                .scale_bucket_profiles
                .iter()
                .filter_map(|scale_bucket_profile| {
                    let bucket_member_indices: Vec<usize> = scale_bucket_profile
                        .member_indices
                        .iter()
                        .copied()
                        .filter(|member_index| {
                            selected_member_indices.binary_search(member_index).is_ok()
                        })
                        .collect();
                    if bucket_member_indices.len() < 24 {
                        return None;
                    }

                    average_negative_log_likelihood(
                        configuration,
                        calibration_sample,
                        bucket_member_indices.as_slice(),
                        integration_step_count,
                    )
                })
                .collect::<Vec<_>>()
                .as_slice(),
        )
    };
    let log_likelihood_shift =
        log_likelihood_shift_from_observed_profile(calibration_sample.observed_profile);

    ContinuousLikelihoodObjective {
        shifted_objective_score: log_likelihood_shift
            + 0.45 * global_negative_log_likelihood
            + 0.55 * conditional_negative_log_likelihood
            + boundary_penalty,
        global_negative_log_likelihood,
        conditional_negative_log_likelihood,
        boundary_penalty,
        log_likelihood_shift,
    }
}

fn likelihood_member_indices(
    calibration_sample: &ReturnComponentsStageContinuousReturnCalibrationSample,
    evaluation_mode: ContinuousLikelihoodEvaluationMode,
) -> Vec<usize> {
    let total_count = calibration_sample.observed_returns.len();
    let target_count = match evaluation_mode {
        ContinuousLikelihoodEvaluationMode::Search => {
            RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_SEARCH_SAMPLE_COUNT
        }
        ContinuousLikelihoodEvaluationMode::Full => total_count,
    };
    if total_count <= target_count {
        return (0..total_count).collect();
    }

    let mut retained_indices = BTreeSet::new();
    for retained_position in 0..target_count {
        let scaled_index = if target_count <= 1 {
            0
        } else {
            (retained_position * (total_count - 1)) / (target_count - 1)
        };
        retained_indices.insert(scaled_index);
    }

    retained_indices.into_iter().collect()
}

fn average_negative_log_likelihood(
    configuration: NigSharedPlusShiftConfiguration,
    calibration_sample: &ReturnComponentsStageContinuousReturnCalibrationSample,
    member_indices: &[usize],
    integration_step_count: usize,
) -> Option<f64> {
    let driver_parameters = configuration.derive_driver_parameters().ok()?;
    let mut log_likelihood_sum = 0.0;
    let mut evaluated_count = 0usize;

    for member_index in member_indices {
        let observed_return = *calibration_sample.observed_returns.get(*member_index)?;
        let business_time_scale = *calibration_sample.business_time_scales.get(*member_index)?;
        let sell_share_proxy = *calibration_sample.sell_share_proxies.get(*member_index)?;
        let block_log_density = log_density_net_trading_time_return(
            driver_parameters,
            configuration,
            observed_return,
            business_time_scale,
            sell_share_proxy,
            integration_step_count,
        )?;

        log_likelihood_sum += block_log_density;
        evaluated_count += 1;
    }

    if evaluated_count == 0 {
        return None;
    }

    Some(-log_likelihood_sum / evaluated_count as f64)
}

fn log_density_net_trading_time_return(
    driver_parameters: NigBuySellDriverParameters,
    configuration: NigSharedPlusShiftConfiguration,
    observed_return: f64,
    business_time_scale: f64,
    sell_share_proxy: f64,
    integration_step_count: usize,
) -> Option<f64> {
    if !observed_return.is_finite()
        || !business_time_scale.is_finite()
        || business_time_scale <= 0.0
        || integration_step_count == 0
    {
        return None;
    }

    let sell_weight = sell_share_proxy.clamp(0.0, 1.0);
    let buy_weight = 1.0 - sell_weight;
    let integrated_buy_intensity = (business_time_scale * buy_weight)
        .max(RETURN_COMPONENTS_STAGE_MIN_REPRESENTABLE_POSITIVE_PARAMETER);
    let integrated_sell_intensity = (business_time_scale * sell_weight)
        .max(RETURN_COMPONENTS_STAGE_MIN_REPRESENTABLE_POSITIVE_PARAMETER);
    let block_standard_deviation =
        block_return_standard_deviation(configuration, business_time_scale, sell_share_proxy)?;
    let standardized_return = observed_return / block_standard_deviation;
    let integration_upper_bound = standardized_integration_upper_bound(standardized_return.abs());
    let integration_step_width = integration_upper_bound / integration_step_count as f64;
    if !integration_step_width.is_finite() || integration_step_width <= 0.0 {
        return None;
    }

    let mut integral_estimate = 0.0;
    for integration_index in 0..integration_step_count {
        let standardized_frequency = (integration_index as f64 + 0.5) * integration_step_width;
        let actual_frequency = standardized_frequency / block_standard_deviation;
        let characteristic_exponent = driver_parameters
            .compose_trading_time_weighted_characteristic_exponent(
                ComplexScalar::new(actual_frequency, 0.0),
                integrated_buy_intensity,
                integrated_sell_intensity,
            )
            .combined_weighted_exponent;
        if !characteristic_exponent.is_finite() {
            return None;
        }

        let characteristic_function = complex_exponential(characteristic_exponent);
        if !characteristic_function.is_finite() {
            return None;
        }

        let oscillation = complex_exponential(ComplexScalar::new(
            0.0,
            -standardized_return * standardized_frequency,
        ));
        let integrand = complex_multiply(characteristic_function, oscillation).real;
        if !integrand.is_finite() {
            return None;
        }

        integral_estimate += integrand;
    }

    let density = integration_step_width * integral_estimate / (PI * block_standard_deviation);
    if !density.is_finite() {
        return None;
    }

    // Truncation and finite-resolution inversion can push a valid density estimate slightly
    // below zero. Clamp to the smallest positive floor so likelihood evaluation stays defined.
    Some(density.max(f64::MIN_POSITIVE).ln())
}

fn block_return_standard_deviation(
    configuration: NigSharedPlusShiftConfiguration,
    business_time_scale: f64,
    sell_share_proxy: f64,
) -> Option<f64> {
    let (_, variance_per_unit, _, _) =
        theoretical_net_process_moments(configuration, sell_share_proxy)?;
    let variance = (variance_per_unit * business_time_scale)
        .max(RETURN_COMPONENTS_STAGE_MIN_REPRESENTABLE_POSITIVE_PARAMETER);
    let standard_deviation = variance.sqrt();
    standard_deviation.is_finite().then_some(standard_deviation)
}

fn standardized_integration_upper_bound(absolute_standardized_return: f64) -> f64 {
    RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_STANDARDIZED_INTEGRATION_UPPER_BOUND
        + 8.0 * absolute_standardized_return.min(4.0)
}

fn log_likelihood_shift_from_observed_profile(
    observed_profile: ContinuousReturnDistributionProfile,
) -> f64 {
    (-observed_profile.return_volatility.max(1.0e-8).ln()).max(1.0) + 4.0
}

fn distribution_profile_fit_breakdown(
    fitted_profile: ContinuousReturnDistributionProfile,
    observed_profile: ContinuousReturnDistributionProfile,
) -> ReturnComponentsStageContinuousDistributionFitBreakdownDiagnostic {
    let central_scale = (observed_profile.upper_quartile_return
        - observed_profile.lower_quartile_return)
        .abs()
        .max(observed_profile.l_moment_lambda2.abs())
        .max(1.0e-8);
    let tail_scale = observed_profile
        .extreme_right_tail_return_q99
        .abs()
        .max(observed_profile.extreme_left_tail_return_q01.abs())
        .max(central_scale);
    let shape_scale = 0.10;

    let mean_return_fit = 0.04
        * scale_aware_absolute_error(
            fitted_profile.mean_return,
            observed_profile.mean_return,
            central_scale,
        );
    let volatility_scale = observed_profile
        .return_volatility
        .abs()
        .max(central_scale)
        .max(1.0e-8);
    let volatility_fit = 0.16
        * scale_aware_absolute_error(
            fitted_profile.return_volatility,
            observed_profile.return_volatility,
            volatility_scale,
        );
    let median_return_fit = 0.10
        * scale_aware_absolute_error(
            fitted_profile.median_return,
            observed_profile.median_return,
            central_scale,
        );
    let lower_quartile_fit = 0.06
        * scale_aware_absolute_error(
            fitted_profile.lower_quartile_return,
            observed_profile.lower_quartile_return,
            central_scale,
        );
    let upper_quartile_fit = 0.06
        * scale_aware_absolute_error(
            fitted_profile.upper_quartile_return,
            observed_profile.upper_quartile_return,
            central_scale,
        );
    let lower_tail_q05_fit = 0.08
        * scale_aware_absolute_error(
            fitted_profile.lower_tail_return_q05,
            observed_profile.lower_tail_return_q05,
            tail_scale,
        );
    let upper_tail_q95_fit = 0.08
        * scale_aware_absolute_error(
            fitted_profile.upper_tail_return_q95,
            observed_profile.upper_tail_return_q95,
            tail_scale,
        );
    let extreme_left_tail_q01_fit = 0.08
        * scale_aware_absolute_error(
            fitted_profile.extreme_left_tail_return_q01,
            observed_profile.extreme_left_tail_return_q01,
            tail_scale,
        );
    let extreme_right_tail_q99_fit = 0.08
        * scale_aware_absolute_error(
            fitted_profile.extreme_right_tail_return_q99,
            observed_profile.extreme_right_tail_return_q99,
            tail_scale,
        );
    let mean_abs_return_fit = 0.10
        * scale_aware_absolute_error(
            fitted_profile.mean_abs_return,
            observed_profile.mean_abs_return,
            central_scale,
        );
    let median_abs_return_fit = 0.08
        * scale_aware_absolute_error(
            fitted_profile.median_abs_return,
            observed_profile.median_abs_return,
            central_scale,
        );
    let abs_quantile_ratio_fit = 0.03
        * scale_aware_absolute_error(
            fitted_profile.abs_quantile_ratio_q99_q95,
            observed_profile.abs_quantile_ratio_q99_q95,
            0.10,
        );
    let bowley_skewness_fit = 0.025
        * scale_aware_absolute_error(
            fitted_profile.bowley_skewness,
            observed_profile.bowley_skewness,
            shape_scale,
        );
    let l_moment_lambda2_fit = 0.025
        * scale_aware_absolute_error(
            fitted_profile.l_moment_lambda2,
            observed_profile.l_moment_lambda2,
            central_scale,
        );
    let l_moment_tau3_fit = 0.025
        * scale_aware_absolute_error(
            fitted_profile.l_moment_tau3,
            observed_profile.l_moment_tau3,
            shape_scale,
        );
    let global_fit_score = mean_return_fit
        + volatility_fit
        + median_return_fit
        + lower_quartile_fit
        + upper_quartile_fit
        + lower_tail_q05_fit
        + upper_tail_q95_fit
        + extreme_left_tail_q01_fit
        + extreme_right_tail_q99_fit
        + mean_abs_return_fit
        + median_abs_return_fit
        + abs_quantile_ratio_fit
        + bowley_skewness_fit
        + l_moment_lambda2_fit
        + l_moment_tau3_fit;

    ReturnComponentsStageContinuousDistributionFitBreakdownDiagnostic {
        global_fit_score,
        conditional_fit_score: 0.0,
        boundary_penalty: 0.0,
        profile_fit_score: global_fit_score,
        log_likelihood_shift: 0.0,
        mean_return_fit,
        volatility_fit,
        median_return_fit,
        lower_quartile_fit,
        upper_quartile_fit,
        lower_tail_q05_fit,
        upper_tail_q95_fit,
        extreme_left_tail_q01_fit,
        extreme_right_tail_q99_fit,
        mean_abs_return_fit,
        median_abs_return_fit,
        abs_quantile_ratio_fit,
        bowley_skewness_fit,
        l_moment_lambda2_fit,
        l_moment_tau3_fit,
    }
}

fn alpha_beta_boundary_penalty(configuration: NigSharedPlusShiftConfiguration) -> f64 {
    let buy_margin = configuration.alpha - configuration.beta_0.abs();
    let sell_margin = configuration.alpha - configuration.derived_sell_beta().abs();
    let minimum_margin_ratio = (buy_margin.min(sell_margin)
        / configuration
            .alpha
            .max(RETURN_COMPONENTS_STAGE_MIN_REPRESENTABLE_POSITIVE_PARAMETER))
    .max(RETURN_COMPONENTS_STAGE_MIN_REPRESENTABLE_POSITIVE_PARAMETER);
    let preferred_margin_ratio = 0.05;

    if minimum_margin_ratio >= preferred_margin_ratio {
        0.0
    } else {
        (preferred_margin_ratio / minimum_margin_ratio).ln_1p()
    }
}

fn scale_aware_absolute_error(observed: f64, target: f64, scale_floor: f64) -> f64 {
    let denominator = scale_floor
        .abs()
        .max(RETURN_COMPONENTS_STAGE_MIN_REPRESENTABLE_POSITIVE_PARAMETER);
    ((observed - target).abs() / denominator).max(0.0)
}

fn simulate_matched_scale_returns(
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

fn build_scale_bucket_profiles(
    observed_returns: &[f64],
    business_time_scales: &[f64],
) -> Vec<ContinuousReturnScaleBucketProfile> {
    let aligned_count = observed_returns.len().min(business_time_scales.len());
    if aligned_count < 96 {
        return Vec::new();
    }

    let finite_scales: Vec<f64> = business_time_scales
        .iter()
        .copied()
        .take(aligned_count)
        .filter(|value| value.is_finite() && *value > 0.0)
        .collect();
    if finite_scales.len() < 96 {
        return Vec::new();
    }

    let quantile_cutoffs = [
        sorted_quantile(
            sorted_finite_values(finite_scales.as_slice()).as_slice(),
            0.25,
        ),
        sorted_quantile(
            sorted_finite_values(finite_scales.as_slice()).as_slice(),
            0.50,
        ),
        sorted_quantile(
            sorted_finite_values(finite_scales.as_slice()).as_slice(),
            0.75,
        ),
    ];
    let mut bucket_member_indices = vec![Vec::new(), Vec::new(), Vec::new(), Vec::new()];

    for aligned_index in 0..aligned_count {
        let business_time_scale = business_time_scales[aligned_index];
        if !business_time_scale.is_finite() || business_time_scale <= 0.0 {
            continue;
        }

        let bucket_index = if business_time_scale <= quantile_cutoffs[0] {
            0
        } else if business_time_scale <= quantile_cutoffs[1] {
            1
        } else if business_time_scale <= quantile_cutoffs[2] {
            2
        } else {
            3
        };
        bucket_member_indices[bucket_index].push(aligned_index);
    }

    bucket_member_indices
        .into_iter()
        .filter_map(|member_indices| {
            if member_indices.len() < 24 {
                return None;
            }

            let bucket_returns: Vec<f64> = member_indices
                .iter()
                .filter_map(|member_index| observed_returns.get(*member_index).copied())
                .collect();
            let _bucket_profile = build_distribution_profile(bucket_returns.as_slice());

            Some(ContinuousReturnScaleBucketProfile { member_indices })
        })
        .collect()
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

fn build_distribution_profile(returns: &[f64]) -> ContinuousReturnDistributionProfile {
    let absolute_returns: Vec<f64> = returns.iter().map(|value| value.abs()).collect();
    let sorted_returns = sorted_finite_values(returns);
    let lower_quartile_return = sorted_quantile(sorted_returns.as_slice(), 0.25);
    let median_return = sorted_quantile(sorted_returns.as_slice(), 0.50);
    let upper_quartile_return = sorted_quantile(sorted_returns.as_slice(), 0.75);
    let lower_tail_return_q05 = sorted_quantile(sorted_returns.as_slice(), 0.05);
    let upper_tail_return_q95 = sorted_quantile(sorted_returns.as_slice(), 0.95);
    let extreme_left_tail_return_q01 = sorted_quantile(sorted_returns.as_slice(), 0.01);
    let extreme_right_tail_return_q99 = sorted_quantile(sorted_returns.as_slice(), 0.99);
    let sorted_absolute_returns = sorted_finite_values(absolute_returns.as_slice());
    let quantile_95_abs = sorted_quantile(sorted_absolute_returns.as_slice(), 0.95);
    let quantile_99_abs = sorted_quantile(sorted_absolute_returns.as_slice(), 0.99);

    ContinuousReturnDistributionProfile {
        mean_return: arithmetic_mean_signed(returns),
        return_volatility: sample_standard_deviation(returns),
        median_return,
        lower_quartile_return,
        upper_quartile_return,
        lower_tail_return_q05,
        upper_tail_return_q95,
        extreme_left_tail_return_q01,
        extreme_right_tail_return_q99,
        mean_abs_return: arithmetic_mean(absolute_returns.as_slice()),
        median_abs_return: sorted_quantile(sorted_absolute_returns.as_slice(), 0.50),
        abs_quantile_ratio_q99_q95: if quantile_95_abs.abs()
            <= RETURN_COMPONENTS_STAGE_MIN_REPRESENTABLE_POSITIVE_PARAMETER
        {
            1.0
        } else {
            quantile_99_abs / quantile_95_abs
        },
        bowley_skewness: compute_bowley_skewness_from_sorted(sorted_returns.as_slice())
            .unwrap_or(0.0),
        l_moment_lambda2: compute_l_moment_lambda2_from_sorted(sorted_returns.as_slice())
            .unwrap_or(0.0),
        l_moment_tau3: compute_l_moment_tau3_from_sorted(sorted_returns.as_slice()).unwrap_or(0.0),
    }
}

fn sorted_finite_values(values: &[f64]) -> Vec<f64> {
    let mut sorted_values: Vec<f64> = values
        .iter()
        .copied()
        .filter(|value| value.is_finite())
        .collect();
    sorted_values.sort_by(f64::total_cmp);
    sorted_values
}

fn sorted_quantile(sorted_values: &[f64], quantile_level: f64) -> f64 {
    if sorted_values.is_empty() {
        return 0.0;
    }

    let quantile_position = quantile_level.clamp(0.0, 1.0) * (sorted_values.len() - 1) as f64;
    let lower_position = quantile_position.floor() as usize;
    let upper_position = quantile_position.ceil() as usize;
    let interpolation_weight = quantile_position - lower_position as f64;

    if lower_position == upper_position {
        sorted_values[lower_position]
    } else {
        sorted_values[lower_position] * (1.0 - interpolation_weight)
            + sorted_values[upper_position] * interpolation_weight
    }
}

fn compute_bowley_skewness_from_sorted(sorted_returns: &[f64]) -> Option<f64> {
    if sorted_returns.len() < 3 {
        return None;
    }

    let lower_quartile = sorted_quantile(sorted_returns, 0.25);
    let median = sorted_quantile(sorted_returns, 0.50);
    let upper_quartile = sorted_quantile(sorted_returns, 0.75);
    let denominator = (upper_quartile - lower_quartile).abs();
    if denominator <= RETURN_COMPONENTS_STAGE_MIN_REPRESENTABLE_POSITIVE_PARAMETER {
        return None;
    }

    let value = (upper_quartile + lower_quartile - 2.0 * median) / denominator;
    value.is_finite().then_some(value)
}

fn compute_l_moment_lambda2_from_sorted(sorted_returns: &[f64]) -> Option<f64> {
    let probability_weighted_moment_0 = compute_probability_weighted_moment(sorted_returns, 0)?;
    let probability_weighted_moment_1 = compute_probability_weighted_moment(sorted_returns, 1)?;
    let lambda2 = 2.0 * probability_weighted_moment_1 - probability_weighted_moment_0;

    lambda2.is_finite().then_some(lambda2)
}

fn compute_l_moment_tau3_from_sorted(sorted_returns: &[f64]) -> Option<f64> {
    let probability_weighted_moment_0 = compute_probability_weighted_moment(sorted_returns, 0)?;
    let probability_weighted_moment_1 = compute_probability_weighted_moment(sorted_returns, 1)?;
    let probability_weighted_moment_2 = compute_probability_weighted_moment(sorted_returns, 2)?;

    let lambda2 = 2.0 * probability_weighted_moment_1 - probability_weighted_moment_0;
    if !lambda2.is_finite()
        || lambda2.abs() <= RETURN_COMPONENTS_STAGE_MIN_REPRESENTABLE_POSITIVE_PARAMETER
    {
        return None;
    }

    let lambda3 = 6.0 * probability_weighted_moment_2 - 6.0 * probability_weighted_moment_1
        + probability_weighted_moment_0;
    if !lambda3.is_finite() {
        return None;
    }

    let tau3 = lambda3 / lambda2;
    tau3.is_finite().then_some(tau3)
}

#[cfg(test)]
mod tests {
    use super::{
        ReturnComponentsStageContinuousReturnCalibrationSample, average_negative_log_likelihood,
        build_distribution_profile, build_equal_business_time_blocks, build_scale_bucket_profiles,
        coordinate_to_ratio, coordinate_to_share, log_density_net_trading_time_return,
        ratio_to_coordinate, sell_beta_ratio_from_relative_share,
        sell_relative_share_from_beta_ratios,
    };
    use crate::stages::return_components::contracts::ReturnComponentsObservedOpenSessionReturn;
    use janiform_foundations::randomness::SplitMix64SimulationRandomnessStream;
    use janiform_model_kernels::nig::NigSharedPlusShiftConfiguration;

    #[test]
    fn skew_ratio_coordinate_round_trip_stays_inside_open_domain() {
        let ratio = coordinate_to_ratio(ratio_to_coordinate(0.35));

        assert!(ratio.is_finite());
        assert!(ratio > -1.0);
        assert!(ratio < 1.0);
    }

    #[test]
    fn sell_relative_coordinate_round_trip_preserves_ordering() {
        let buy_ratio = 0.4;
        let sell_ratio = sell_beta_ratio_from_relative_share(buy_ratio, coordinate_to_share(0.2));
        let recovered_share = sell_relative_share_from_beta_ratios(buy_ratio, sell_ratio)
            .expect("relative share should be recoverable");

        assert!(sell_ratio < buy_ratio);
        assert!(recovered_share > 0.0);
        assert!(recovered_share < 1.0);
    }

    #[test]
    fn distribution_profile_reports_robust_quantile_and_l_moment_metrics() {
        let profile = build_distribution_profile(&[-0.03, -0.01, 0.0, 0.02, 0.08]);

        assert!(profile.abs_quantile_ratio_q99_q95 >= 1.0);
        assert!(profile.l_moment_lambda2.is_finite());
        assert!(profile.bowley_skewness.is_finite());
        assert!(profile.l_moment_tau3.is_finite());
    }

    #[test]
    fn scale_bucket_profiles_cover_multiple_business_time_regimes() {
        let observed_returns: Vec<f64> = (0..128)
            .map(|sample_index| sample_index as f64 / 10_000.0)
            .collect();
        let business_time_scales: Vec<f64> = (0..128)
            .map(|sample_index| 0.1 + sample_index as f64 / 32.0)
            .collect();

        let scale_bucket_profiles = build_scale_bucket_profiles(
            observed_returns.as_slice(),
            business_time_scales.as_slice(),
        );

        assert!(scale_bucket_profiles.len() >= 3);
        assert!(
            scale_bucket_profiles
                .iter()
                .all(|bucket| !bucket.member_indices.is_empty())
        );
    }

    #[test]
    fn equal_business_time_blocks_preserve_order_and_total_mass() {
        let observed_returns = vec![
            ReturnComponentsObservedOpenSessionReturn {
                return_timestamp_utc_nanos: 1,
                log_return: 0.01,
                post_shock_activity_lift: 1.0,
                sell_share_proxy: 0.0,
            },
            ReturnComponentsObservedOpenSessionReturn {
                return_timestamp_utc_nanos: 2,
                log_return: -0.02,
                post_shock_activity_lift: 2.0,
                sell_share_proxy: 1.0,
            },
            ReturnComponentsObservedOpenSessionReturn {
                return_timestamp_utc_nanos: 3,
                log_return: 0.03,
                post_shock_activity_lift: 3.0,
                sell_share_proxy: 0.0,
            },
            ReturnComponentsObservedOpenSessionReturn {
                return_timestamp_utc_nanos: 4,
                log_return: 0.04,
                post_shock_activity_lift: 4.0,
                sell_share_proxy: 1.0,
            },
        ];
        let business_time_scales = [1.0, 1.0, 1.0, 1.0];

        let aggregated_blocks = build_equal_business_time_blocks(
            observed_returns.as_slice(),
            business_time_scales.as_slice(),
            2,
        );

        assert_eq!(aggregated_blocks.len(), 2);
        assert_eq!(aggregated_blocks[0].0.return_timestamp_utc_nanos, 2);
        assert_eq!(aggregated_blocks[1].0.return_timestamp_utc_nanos, 4);
        assert!((aggregated_blocks[0].0.log_return + 0.01).abs() <= 1.0e-12);
        assert!((aggregated_blocks[1].0.log_return - 0.07).abs() <= 1.0e-12);
        assert!((aggregated_blocks[0].1 - 2.0).abs() <= 1.0e-12);
        assert!((aggregated_blocks[1].1 - 2.0).abs() <= 1.0e-12);
        assert!((aggregated_blocks[0].0.sell_share_proxy - 0.5).abs() <= 1.0e-12);
        assert!((aggregated_blocks[1].0.sell_share_proxy - 0.5).abs() <= 1.0e-12);
    }

    #[test]
    fn block_log_density_is_finite_for_representative_configuration() {
        let configuration =
            NigSharedPlusShiftConfiguration::try_new(80.0, -12.0, 2.0e-4, 0.0, -8.0)
                .expect("configuration should validate");
        let driver_parameters = configuration
            .derive_driver_parameters()
            .expect("driver parameters should derive");

        let log_density = log_density_net_trading_time_return(
            driver_parameters,
            configuration,
            1.5e-4,
            1.2,
            0.55,
            128,
        )
        .expect("log density should evaluate");

        assert!(log_density.is_finite());
    }

    #[test]
    fn likelihood_objective_prefers_the_data_generating_configuration() {
        let true_configuration =
            NigSharedPlusShiftConfiguration::try_new(80.0, -12.0, 2.0e-4, 0.0, -8.0)
                .expect("configuration should validate");
        let challenger_configuration =
            NigSharedPlusShiftConfiguration::try_new(9.0, -6.0, 8.0e-5, 0.0, -1.5)
                .expect("challenger configuration should validate");
        let driver_parameters = true_configuration
            .derive_driver_parameters()
            .expect("driver parameters should derive");
        let mut randomness_stream = SplitMix64SimulationRandomnessStream::from_seed(41);
        let business_time_scales: Vec<f64> = (0..128)
            .map(|sample_index| 0.6 + sample_index as f64 / 96.0)
            .collect();
        let sell_share_proxies: Vec<f64> = (0..128)
            .map(|sample_index| 0.35 + 0.30 * ((sample_index % 9) as f64 / 8.0))
            .collect();
        let observed_returns: Vec<f64> = business_time_scales
            .iter()
            .zip(sell_share_proxies.iter())
            .map(|(business_time_scale, sell_share_proxy)| {
                let sell_weight = sell_share_proxy.clamp(0.0, 1.0);
                let buy_weight = 1.0 - sell_weight;
                driver_parameters
                    .sample_trading_time_return_increment(
                        business_time_scale * buy_weight,
                        business_time_scale * sell_weight,
                        &mut randomness_stream,
                    )
                    .expect("synthetic increment should sample")
                    .combined_increment
            })
            .collect();
        let calibration_sample = ReturnComponentsStageContinuousReturnCalibrationSample {
            observed_returns: observed_returns.clone(),
            business_time_scales: business_time_scales.clone(),
            sell_share_proxies: sell_share_proxies.clone(),
            observed_profile: build_distribution_profile(observed_returns.as_slice()),
            scale_bucket_profiles: build_scale_bucket_profiles(
                observed_returns.as_slice(),
                business_time_scales.as_slice(),
            ),
            open_session_sell_share_proxy: 0.5,
        };

        let true_objective = average_negative_log_likelihood(
            true_configuration,
            &calibration_sample,
            &(0..observed_returns.len()).collect::<Vec<_>>(),
            128,
        )
        .expect("true likelihood should evaluate");
        let challenger_objective = average_negative_log_likelihood(
            challenger_configuration,
            &calibration_sample,
            &(0..observed_returns.len()).collect::<Vec<_>>(),
            128,
        )
        .expect("challenger likelihood should evaluate");

        assert!(
            true_objective < challenger_objective,
            "likelihood objective should prefer the data-generating configuration"
        );
    }
}
