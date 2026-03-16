use std::collections::BTreeSet;

use janiform_calibration_artifacts::CalibrationOptimizerSettings;
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
    ReturnComponentsStageSortedSignalInputs, arithmetic_mean, arithmetic_mean_signed,
    round_negative_candidate_value, round_positive_candidate_value, round_signed_candidate_value,
    sample_adjusted_excess_kurtosis, sample_adjusted_skewness, sample_standard_deviation,
    theoretical_net_process_distribution_diagnostic, theoretical_net_process_moments,
};

const RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_MAX_SAMPLE_COUNT: usize = 2_048;
const RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_MIN_BROAD_CANDIDATE_COUNT: usize = 48;
const RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_ELITE_COUNT: usize = 6;
const RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_REFINEMENT_ROUNDS: usize = 2;
const RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_DOMAIN_MARGIN_RATIO: f64 = 0.01;

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
    skewness: f64,
    excess_kurtosis: f64,
    mean_abs_return: f64,
    median_abs_return: f64,
    upper_decile_abs_return: f64,
    upper_tail_abs_return: f64,
}

#[derive(Debug, Clone, Copy)]
struct NigShapeSearchPoint {
    log_alpha: f64,
    buy_beta_ratio: f64,
    sell_beta_ratio: f64,
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

    for anchor in anchor_search_points(summary_statistics) {
        if let Some(scored_candidate) =
            build_scored_candidate(summary_statistics, calibration_sample, stage_seed, anchor)
        {
            scored_candidates.push(scored_candidate);
        }
    }

    for _ in 0..broad_candidate_count {
        let candidate_point = sample_broad_search_point(&mut search_randomness_stream);
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
        let elite_points = elite_search_points(scored_candidates.as_slice());
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
    summary_statistics: ReturnComponentsStageObservedSignalSummaryStatistics,
) -> Vec<NigShapeSearchPoint> {
    let alpha_center =
        (2.0 + summary_statistics.continuous_return_excess_kurtosis.abs() * 0.5).clamp(1.05, 64.0);
    let buy_beta_center = (-summary_statistics.continuous_return_skewness * 0.20
        + (summary_statistics.open_session_sell_share_proxy - 0.5) * 0.30)
        .clamp(-0.75, 0.75);
    let sell_beta_center = (buy_beta_center
        - (0.05
            + summary_statistics.continuous_return_skewness.abs() * 0.10
            + (summary_statistics.open_session_sell_share_proxy - 0.5).abs() * 0.20))
        .clamp(-0.95, buy_beta_center - 0.01);

    vec![
        NigShapeSearchPoint {
            log_alpha: alpha_center.ln(),
            buy_beta_ratio: buy_beta_center.clamp(-0.90, 0.90),
            sell_beta_ratio: sell_beta_center.clamp(-0.95, buy_beta_center - 0.01),
        },
        NigShapeSearchPoint {
            log_alpha: 2.0_f64.ln(),
            buy_beta_ratio: 0.0,
            sell_beta_ratio: -0.05,
        },
        NigShapeSearchPoint {
            log_alpha: 8.0_f64.ln(),
            buy_beta_ratio: buy_beta_center.clamp(-0.90, 0.90),
            sell_beta_ratio: sell_beta_center.clamp(-0.95, buy_beta_center - 0.01),
        },
    ]
}

fn sample_broad_search_point<R>(randomness_stream: &mut R) -> NigShapeSearchPoint
where
    R: SimulationRandomnessStream,
{
    let log_alpha = 1.05_f64.ln() + randomness_stream.next_unit_interval_f64() * 5.4;
    let buy_beta_ratio = -0.90 + 1.80 * randomness_stream.next_unit_interval_f64();
    let sell_upper = (buy_beta_ratio - 0.01).min(0.90);
    let sell_lower = -0.95;
    let sell_beta_ratio = sell_lower
        + (sell_upper - sell_lower).max(0.01) * randomness_stream.next_unit_interval_f64();

    NigShapeSearchPoint {
        log_alpha,
        buy_beta_ratio,
        sell_beta_ratio: sell_beta_ratio.min(buy_beta_ratio - 0.01),
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
    let perturbed_buy_ratio =
        anchor.buy_beta_ratio + refinement_scale * sample_centered_perturbation(randomness_stream);
    let perturbed_sell_ratio =
        anchor.sell_beta_ratio + refinement_scale * sample_centered_perturbation(randomness_stream);

    NigShapeSearchPoint {
        log_alpha: anchor.log_alpha
            + refinement_scale * sample_centered_perturbation(randomness_stream),
        buy_beta_ratio: perturbed_buy_ratio.clamp(-0.90, 0.90),
        sell_beta_ratio: perturbed_sell_ratio.min(perturbed_buy_ratio - 0.01),
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

fn elite_search_points(scored_candidates: &[ScoredNigCandidate]) -> Vec<NigShapeSearchPoint> {
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
        .map(|candidate| {
            let alpha = candidate.configuration.alpha.max(1.05);
            NigShapeSearchPoint {
                log_alpha: alpha.ln(),
                buy_beta_ratio: (candidate.configuration.beta_0 / alpha).clamp(-0.90, 0.90),
                sell_beta_ratio: (candidate.configuration.derived_sell_beta() / alpha)
                    .clamp(-0.95, 0.90),
            }
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
    let alpha = shape_search_point.log_alpha.exp().clamp(1.05, 256.0);
    let buy_beta_ratio = shape_search_point.buy_beta_ratio.clamp(-0.90, 0.90);
    let sell_beta_ratio = shape_search_point.sell_beta_ratio.clamp(
        -0.95,
        buy_beta_ratio - RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_DOMAIN_MARGIN_RATIO,
    );
    let beta_0 = alpha * buy_beta_ratio;
    let sell_beta = alpha * sell_beta_ratio;
    let delta_beta = sell_beta - beta_0;

    let (mean_without_mu, variance_per_unit, _, _) = theoretical_net_process_moments(
        NigSharedPlusShiftConfiguration {
            alpha,
            beta_0,
            delta: 1.0,
            mu: 0.0,
            delta_beta,
        },
        open_session_sell_share_proxy,
    )?;

    let target_variance = summary_statistics
        .continuous_return_volatility
        .powi(2)
        .max(1.0e-12);
    let delta = (target_variance / variance_per_unit.max(1.0e-10)).clamp(
        RETURN_COMPONENTS_STAGE_MIN_REPRESENTABLE_POSITIVE_PARAMETER,
        1_000.0,
    );
    let buy_weight = 1.0 - open_session_sell_share_proxy.clamp(0.0, 1.0);
    let sell_weight = open_session_sell_share_proxy.clamp(0.0, 1.0);
    let mu_weight = buy_weight - sell_weight;
    let mu = if mu_weight.abs() >= 0.10 {
        ((summary_statistics.continuous_return_mean - delta * mean_without_mu) / mu_weight).clamp(
            -summary_statistics.continuous_return_volatility.max(1.0e-6) * 25.0,
            summary_statistics.continuous_return_volatility.max(1.0e-6) * 25.0,
        )
    } else {
        0.0
    };

    NigSharedPlusShiftConfiguration::try_new(
        round_positive_candidate_value(alpha),
        round_signed_candidate_value(beta_0),
        round_positive_candidate_value(delta),
        round_signed_candidate_value(mu),
        round_negative_candidate_value(delta_beta),
    )
    .ok()
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

    0.05 * zero_aware_relative_error(
        fitted_profile.mean_return,
        observed_profile.mean_return,
        observed_profile.return_volatility * 0.10 + 1.0e-8,
    ) + 0.25
        * zero_aware_relative_error(
            fitted_profile.return_volatility,
            observed_profile.return_volatility,
            1.0e-8,
        )
        + 0.15
            * zero_aware_relative_error(
                fitted_profile.mean_abs_return,
                observed_profile.mean_abs_return,
                1.0e-8,
            )
        + 0.15
            * zero_aware_relative_error(
                fitted_profile.median_abs_return,
                observed_profile.median_abs_return,
                1.0e-8,
            )
        + 0.20
            * zero_aware_relative_error(
                fitted_profile.upper_decile_abs_return,
                observed_profile.upper_decile_abs_return,
                1.0e-8,
            )
        + 0.15
            * zero_aware_relative_error(
                fitted_profile.upper_tail_abs_return,
                observed_profile.upper_tail_abs_return,
                1.0e-8,
            )
        + 0.03 * zero_aware_relative_error(fitted_profile.skewness, observed_profile.skewness, 0.25)
        + 0.02
            * zero_aware_relative_error(
                fitted_profile.excess_kurtosis,
                observed_profile.excess_kurtosis,
                1.0,
            )
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

    ContinuousReturnDistributionProfile {
        mean_return: arithmetic_mean_signed(returns),
        return_volatility: sample_standard_deviation(returns),
        skewness: sample_adjusted_skewness(returns).unwrap_or(0.0),
        excess_kurtosis: sample_adjusted_excess_kurtosis(returns).unwrap_or(0.0),
        mean_abs_return: arithmetic_mean(absolute_returns.as_slice()),
        median_abs_return: quantile(absolute_returns.as_slice(), 0.50),
        upper_decile_abs_return: quantile(absolute_returns.as_slice(), 0.90),
        upper_tail_abs_return: quantile(absolute_returns.as_slice(), 0.99),
    }
}

fn quantile(values: &[f64], quantile_level: f64) -> f64 {
    let mut sorted_values: Vec<f64> = values
        .iter()
        .copied()
        .filter(|value| value.is_finite())
        .collect();
    if sorted_values.is_empty() {
        return 0.0;
    }

    sorted_values.sort_by(f64::total_cmp);
    let quantile_index = (quantile_level.clamp(0.0, 1.0)
        * (sorted_values.len().saturating_sub(1)) as f64)
        .round() as usize;

    sorted_values[quantile_index]
}

fn zero_aware_relative_error(observed: f64, target: f64, zero_floor: f64) -> f64 {
    let denominator = if target.abs() <= zero_floor {
        zero_floor
    } else {
        target.abs()
    };

    ((observed - target).abs() / denominator).max(0.0)
}
