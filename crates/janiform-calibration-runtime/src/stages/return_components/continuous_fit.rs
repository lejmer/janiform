use std::collections::BTreeSet;

use janiform_calibration_artifacts::CalibrationOptimizerSettings;
use janiform_foundations::numeric::{arithmetic_mean_or_zero as arithmetic_mean, logistic};
use janiform_foundations::randomness::{
    SimulationRandomnessStream, SplitMix64SimulationRandomnessStream,
};
use janiform_foundations::splitmix64_output_mixer;
use janiform_model_kernels::nig::NigSharedPlusShiftConfiguration;

use super::constants::{
    RETURN_COMPONENTS_STAGE_MAX_NIG_CANDIDATES,
    RETURN_COMPONENTS_STAGE_MIN_REPRESENTABLE_POSITIVE_PARAMETER,
};
use super::contracts::{
    ReturnComponentsObservedOpenSessionReturn,
    ReturnComponentsStageObservedSignalSummaryStatistics,
    ReturnComponentsStageReturnDistributionDiagnostic,
};
use super::execution::{
    ReturnComponentsStageSortedSignalInputs, arithmetic_mean_signed,
    round_negative_candidate_value, round_positive_candidate_value, round_signed_candidate_value,
    sample_standard_deviation, theoretical_net_process_distribution_diagnostic,
    theoretical_net_process_moments,
};

const RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_MAX_SAMPLE_COUNT: usize = 2_048;
const RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_MIN_BROAD_CANDIDATE_COUNT: usize = 48;
const RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_ELITE_COUNT: usize = 6;
const RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_REFINEMENT_ROUNDS: usize = 2;

#[derive(Debug, Clone)]
pub(super) struct ReturnComponentsStageContinuousReturnCalibrationSample {
    business_time_scales: Vec<f64>,
    observed_profile: ContinuousReturnDistributionProfile,
    open_session_sell_share_proxy: f64,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct ContinuousReturnCandidateEvaluation {
    pub objective_score: f64,
    pub diagnostic: ReturnComponentsStageReturnDistributionDiagnostic,
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

#[derive(Debug, Clone, Copy)]
struct NigShapeSearchPoint {
    log_alpha: f64,
    buy_skew_coordinate: f64,
    sell_relative_coordinate: f64,
    log_delta_offset: f64,
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
    let aligned_pairs = deterministically_subsample_aligned_returns(
        sorted_signals.continuous_fit_returns.as_slice(),
        continuous_fit_business_time_scales,
        RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_MAX_SAMPLE_COUNT,
    );
    let observed_returns: Vec<f64> = aligned_pairs
        .iter()
        .map(|(observation, _)| observation.log_return)
        .collect();
    let business_time_scales: Vec<f64> = aligned_pairs.iter().map(|(_, scale)| *scale).collect();
    let observed_profile = build_distribution_profile(observed_returns.as_slice());

    ReturnComponentsStageContinuousReturnCalibrationSample {
        business_time_scales,
        observed_profile,
        open_session_sell_share_proxy,
    }
}

pub(super) fn build_nig_search_candidates(
    summary_statistics: ReturnComponentsStageObservedSignalSummaryStatistics,
    calibration_sample: &ReturnComponentsStageContinuousReturnCalibrationSample,
    stage_seed: u64,
    solver_settings: &CalibrationOptimizerSettings,
) -> Vec<NigSharedPlusShiftConfiguration> {
    let broad_candidate_count = solver_settings
        .max_iterations
        .max(RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_MIN_BROAD_CANDIDATE_COUNT);
    let local_candidate_count = (broad_candidate_count / 2).max(24);
    let mut search_randomness_stream =
        SplitMix64SimulationRandomnessStream::from_seed(stage_seed ^ 0xB5A2_AA90_63C6_1F0D);
    let mut scored_candidates: Vec<ScoredNigCandidate> = Vec::new();

    for anchor in anchor_search_points(calibration_sample) {
        if let Some(scored_candidate) =
            build_scored_candidate(summary_statistics, calibration_sample, stage_seed, anchor)
        {
            scored_candidates.push(scored_candidate);
        }
    }

    for _ in 0..broad_candidate_count {
        let candidate_point =
            sample_broad_search_point(calibration_sample, &mut search_randomness_stream);
        if let Some(scored_candidate) = build_scored_candidate(
            summary_statistics,
            calibration_sample,
            stage_seed,
            candidate_point,
        ) {
            scored_candidates.push(scored_candidate);
        }
    }

    for refinement_round in 0..RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_REFINEMENT_ROUNDS {
        let elite_points = elite_search_points(
            scored_candidates.as_slice(),
            summary_statistics,
            calibration_sample.open_session_sell_share_proxy,
        );
        let refinement_scale = 0.28 / (refinement_round + 1) as f64;

        for elite_point in elite_points {
            for _ in 0..local_candidate_count {
                let candidate_point = sample_local_search_point(
                    &mut search_randomness_stream,
                    elite_point,
                    refinement_scale,
                );
                if let Some(scored_candidate) = build_scored_candidate(
                    summary_statistics,
                    calibration_sample,
                    stage_seed,
                    candidate_point,
                ) {
                    scored_candidates.push(scored_candidate);
                }
            }
        }
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
    let diagnostic = theoretical_net_process_distribution_diagnostic(
        configuration,
        summary_statistics.open_session_sell_share_proxy,
    )
    .unwrap_or(ReturnComponentsStageReturnDistributionDiagnostic {
        sample_count: summary_statistics.continuous_fit_return_count,
        observed_mean_return: summary_statistics.continuous_return_mean,
        fitted_mean_return: 0.0,
        observed_return_volatility: summary_statistics.continuous_return_volatility,
        fitted_return_volatility: 0.0,
        observed_skewness: summary_statistics.continuous_return_skewness,
        fitted_skewness: 0.0,
        observed_excess_kurtosis: summary_statistics.continuous_return_excess_kurtosis,
        fitted_excess_kurtosis: 0.0,
    });

    let objective_score =
        matched_scale_distribution_fit_score(configuration, calibration_sample, stage_seed);

    ContinuousReturnCandidateEvaluation {
        objective_score,
        diagnostic: ReturnComponentsStageReturnDistributionDiagnostic {
            sample_count: summary_statistics.continuous_fit_return_count,
            observed_mean_return: summary_statistics.continuous_return_mean,
            fitted_mean_return: diagnostic.fitted_mean_return,
            observed_return_volatility: summary_statistics.continuous_return_volatility,
            fitted_return_volatility: diagnostic.fitted_return_volatility,
            observed_skewness: summary_statistics.continuous_return_skewness,
            fitted_skewness: diagnostic.fitted_skewness,
            observed_excess_kurtosis: summary_statistics.continuous_return_excess_kurtosis,
            fitted_excess_kurtosis: diagnostic.fitted_excess_kurtosis,
        },
    }
}

fn deterministically_subsample_aligned_returns(
    continuous_fit_returns: &[ReturnComponentsObservedOpenSessionReturn],
    business_time_scales: &[f64],
    maximum_count: usize,
) -> Vec<(ReturnComponentsObservedOpenSessionReturn, f64)> {
    let aligned_count = continuous_fit_returns.len().min(business_time_scales.len());
    if aligned_count <= maximum_count {
        return continuous_fit_returns
            .iter()
            .copied()
            .zip(business_time_scales.iter().copied())
            .take(aligned_count)
            .collect();
    }

    let mut retained = Vec::with_capacity(maximum_count);
    for sample_index in 0..maximum_count {
        let aligned_index = sample_index * aligned_count / maximum_count;
        retained.push((
            continuous_fit_returns[aligned_index],
            business_time_scales[aligned_index],
        ));
    }

    retained
}

fn anchor_search_points(
    calibration_sample: &ReturnComponentsStageContinuousReturnCalibrationSample,
) -> Vec<NigShapeSearchPoint> {
    let observed_profile = calibration_sample.observed_profile;
    let log_alpha_anchor = inferred_log_alpha_anchor(observed_profile);
    let buy_skew_coordinate_anchor = inferred_buy_skew_coordinate(
        observed_profile,
        calibration_sample.open_session_sell_share_proxy,
    );

    vec![
        NigShapeSearchPoint {
            log_alpha: log_alpha_anchor,
            buy_skew_coordinate: buy_skew_coordinate_anchor,
            sell_relative_coordinate: 0.0,
            log_delta_offset: 0.0,
        },
        NigShapeSearchPoint {
            log_alpha: log_alpha_anchor - 1.25,
            buy_skew_coordinate: 0.0,
            sell_relative_coordinate: -0.5,
            log_delta_offset: -0.5,
        },
        NigShapeSearchPoint {
            log_alpha: log_alpha_anchor + 1.25,
            buy_skew_coordinate: buy_skew_coordinate_anchor,
            sell_relative_coordinate: 0.5,
            log_delta_offset: 0.5,
        },
    ]
}

fn sample_broad_search_point<R>(
    calibration_sample: &ReturnComponentsStageContinuousReturnCalibrationSample,
    randomness_stream: &mut R,
) -> NigShapeSearchPoint
where
    R: SimulationRandomnessStream,
{
    let observed_profile = calibration_sample.observed_profile;
    let log_alpha_anchor = inferred_log_alpha_anchor(observed_profile);
    let alpha_span = 1.75 + observed_profile.abs_quantile_ratio_q99_q95.ln_1p();
    let skew_span = 1.25
        + observed_profile
            .bowley_skewness
            .abs()
            .max(observed_profile.l_moment_tau3.abs());
    let log_delta_span = 1.75 + observed_profile.abs_quantile_ratio_q99_q95.ln_1p();

    NigShapeSearchPoint {
        log_alpha: log_alpha_anchor
            + (randomness_stream.next_unit_interval_f64() - 0.5) * 2.0 * alpha_span,
        buy_skew_coordinate: inferred_buy_skew_coordinate(
            observed_profile,
            calibration_sample.open_session_sell_share_proxy,
        ) + (randomness_stream.next_unit_interval_f64() - 0.5)
            * 2.0
            * skew_span,
        sell_relative_coordinate: (randomness_stream.next_unit_interval_f64() - 0.5) * 6.0,
        log_delta_offset: (randomness_stream.next_unit_interval_f64() - 0.5) * 2.0 * log_delta_span,
    }
}

fn sample_local_search_point<R>(
    randomness_stream: &mut R,
    anchor: NigShapeSearchPoint,
    refinement_scale: f64,
) -> NigShapeSearchPoint
where
    R: SimulationRandomnessStream,
{
    NigShapeSearchPoint {
        log_alpha: anchor.log_alpha
            + refinement_scale * sample_centered_perturbation(randomness_stream),
        buy_skew_coordinate: anchor.buy_skew_coordinate
            + refinement_scale * sample_centered_perturbation(randomness_stream),
        sell_relative_coordinate: anchor.sell_relative_coordinate
            + refinement_scale * sample_centered_perturbation(randomness_stream),
        log_delta_offset: anchor.log_delta_offset
            + refinement_scale * sample_centered_perturbation(randomness_stream),
    }
}

fn sample_centered_perturbation<R>(randomness_stream: &mut R) -> f64
where
    R: SimulationRandomnessStream,
{
    randomness_stream.next_unit_interval_f64()
        + randomness_stream.next_unit_interval_f64()
        + randomness_stream.next_unit_interval_f64()
        - 1.5
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
    stage_seed: u64,
    shape_search_point: NigShapeSearchPoint,
) -> Option<ScoredNigCandidate> {
    let configuration = configuration_from_shape_search_point(
        summary_statistics,
        calibration_sample.open_session_sell_share_proxy,
        shape_search_point,
    )?;
    let objective_score =
        matched_scale_distribution_fit_score(configuration, calibration_sample, stage_seed);

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

fn matched_scale_distribution_fit_score(
    configuration: NigSharedPlusShiftConfiguration,
    calibration_sample: &ReturnComponentsStageContinuousReturnCalibrationSample,
    stage_seed: u64,
) -> f64 {
    let synthetic_returns =
        simulate_matched_scale_returns(configuration, calibration_sample, stage_seed);
    if synthetic_returns.is_empty() {
        return f64::INFINITY;
    }

    let fitted_profile = build_distribution_profile(synthetic_returns.as_slice());
    let observed_profile = calibration_sample.observed_profile;
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
    let boundary_penalty = alpha_beta_boundary_penalty(configuration);

    0.03 * scale_aware_absolute_error(
        fitted_profile.mean_return,
        observed_profile.mean_return,
        central_scale,
    ) + 0.14
        * scale_aware_absolute_error(
            fitted_profile.return_volatility,
            observed_profile.return_volatility,
            1.0e-8,
        )
        + 0.12
            * scale_aware_absolute_error(
                fitted_profile.median_return,
                observed_profile.median_return,
                central_scale,
            )
        + 0.08
            * scale_aware_absolute_error(
                fitted_profile.lower_quartile_return,
                observed_profile.lower_quartile_return,
                central_scale,
            )
        + 0.08
            * scale_aware_absolute_error(
                fitted_profile.upper_quartile_return,
                observed_profile.upper_quartile_return,
                central_scale,
            )
        + 0.08
            * scale_aware_absolute_error(
                fitted_profile.lower_tail_return_q05,
                observed_profile.lower_tail_return_q05,
                tail_scale,
            )
        + 0.08
            * scale_aware_absolute_error(
                fitted_profile.upper_tail_return_q95,
                observed_profile.upper_tail_return_q95,
                tail_scale,
            )
        + 0.07
            * scale_aware_absolute_error(
                fitted_profile.extreme_left_tail_return_q01,
                observed_profile.extreme_left_tail_return_q01,
                tail_scale,
            )
        + 0.07
            * scale_aware_absolute_error(
                fitted_profile.extreme_right_tail_return_q99,
                observed_profile.extreme_right_tail_return_q99,
                tail_scale,
            )
        + 0.08
            * scale_aware_absolute_error(
                fitted_profile.mean_abs_return,
                observed_profile.mean_abs_return,
                central_scale,
            )
        + 0.07
            * scale_aware_absolute_error(
                fitted_profile.median_abs_return,
                observed_profile.median_abs_return,
                central_scale,
            )
        + 0.05
            * scale_aware_absolute_error(
                fitted_profile.abs_quantile_ratio_q99_q95,
                observed_profile.abs_quantile_ratio_q99_q95,
                0.10,
            )
        + 0.04
            * scale_aware_absolute_error(
                fitted_profile.bowley_skewness,
                observed_profile.bowley_skewness,
                shape_scale,
            )
        + 0.04
            * scale_aware_absolute_error(
                fitted_profile.l_moment_lambda2,
                observed_profile.l_moment_lambda2,
                central_scale,
            )
        + 0.05
            * scale_aware_absolute_error(
                fitted_profile.l_moment_tau3,
                observed_profile.l_moment_tau3,
                shape_scale,
            )
        + boundary_penalty
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
    let sell_weight = calibration_sample
        .open_session_sell_share_proxy
        .clamp(0.0, 1.0);
    let buy_weight = 1.0 - sell_weight;
    let mut randomness_stream = SplitMix64SimulationRandomnessStream::from_seed(
        candidate_sampling_seed(stage_seed, configuration),
    );
    let mut synthetic_returns = Vec::with_capacity(calibration_sample.business_time_scales.len());

    for business_time_scale in &calibration_sample.business_time_scales {
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

fn compute_probability_weighted_moment(sorted_returns: &[f64], order: usize) -> Option<f64> {
    let sample_count = sorted_returns.len();
    if sample_count <= order {
        return None;
    }

    let denominator_index = sample_count - 1;

    let mut weighted_sum = 0.0;
    for (return_position, return_value) in sorted_returns.iter().enumerate() {
        if return_position < order {
            continue;
        }

        let weight = combination_ratio(return_position, denominator_index, order)?;
        weighted_sum += weight * *return_value;
    }

    let probability_weighted_moment = weighted_sum / sample_count as f64;
    probability_weighted_moment
        .is_finite()
        .then_some(probability_weighted_moment)
}

fn combination_ratio(
    numerator_index: usize,
    denominator_index: usize,
    order: usize,
) -> Option<f64> {
    if order == 0 {
        return Some(1.0);
    }

    if numerator_index < order || denominator_index < order {
        return Some(0.0);
    }

    let mut ratio = 1.0;
    for order_position in 0..order {
        let numerator = numerator_index.saturating_sub(order_position) as f64;
        let denominator = denominator_index.saturating_sub(order_position) as f64;
        if denominator <= 0.0 {
            return None;
        }

        ratio *= numerator / denominator;
    }

    ratio.is_finite().then_some(ratio)
}

#[cfg(test)]
mod tests {
    use super::{
        build_distribution_profile, coordinate_to_ratio, coordinate_to_share, ratio_to_coordinate,
        sell_beta_ratio_from_relative_share, sell_relative_share_from_beta_ratios,
    };

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
}
