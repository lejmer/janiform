use std::{
    collections::{BTreeMap, BTreeSet},
    f64::consts::PI,
    sync::{Arc, Mutex},
};

use janiform_calibration_artifacts::CalibrationOptimizerSettings;
use janiform_foundations::complex_math::{ComplexScalar, complex_exponential, complex_multiply};
use janiform_foundations::numeric::{
    arithmetic_mean_or_zero as arithmetic_mean, compute_probability_weighted_moment, logistic,
};
use janiform_foundations::randomness::SplitMix64SimulationRandomnessStream;
use janiform_foundations::splitmix64_output_mixer;
use janiform_foundations::vector_math::{
    normalize_vector4, vector4_add, vector4_scale, vector4_subtract,
};
use janiform_model_kernels::nig::{NigBuySellDriverParameters, NigSharedPlusShiftConfiguration};
use rayon::prelude::*;

use super::constants::RETURN_COMPONENTS_STAGE_MIN_REPRESENTABLE_POSITIVE_PARAMETER;
use super::contracts::{
    ReturnComponentsObservedOpenSessionReturn,
    ReturnComponentsStageContinuousDistributionFitBreakdownDiagnostic,
    ReturnComponentsStageContinuousOptimizerBasinDiagnostic,
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
const RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_MIN_BROAD_CANDIDATE_COUNT: usize = 48;
const RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_MAX_BROAD_CANDIDATE_COUNT: usize = 384;
const RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_ELITE_COUNT: usize = 6;
const RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_REGIME_COUNT: usize = 7;
const RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_MIN_MOMENT_SEED_CANDIDATE_COUNT: usize = 96;
const RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_MOMENT_SEED_CANDIDATE_MULTIPLIER: usize = 3;
const RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_SEED_SAMPLE_COUNT: usize = 64;
const RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_LOCAL_SAMPLE_COUNT: usize = 256;
const RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_SEED_INTEGRATION_STEP_COUNT: usize = 96;
const RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_LOCAL_INTEGRATION_STEP_COUNT: usize = 192;
const RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_FINAL_INTEGRATION_STEP_COUNT: usize = 512;
const RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_STANDARDIZED_INTEGRATION_UPPER_BOUND: f64 = 64.0;
const RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_LINE_SEARCH_EVALUATION_CAP: usize = 8;
const RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_MAX_POWELL_CYCLES: usize = 12;
const RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_PROFILE_SCAN_STEP_COUNT: usize = 12;
const RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_PROFILE_BISECTION_STEP_COUNT: usize = 8;
const RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_PROFILE_CONFIDENCE_Z_SCORE: f64 =
    1.959_963_984_540_054;
const RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_PROFILE_LIKELIHOOD_CHI_SQUARE_95: f64 =
    3.841_458_820_694_124;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ContinuousLikelihoodFidelityTier {
    Seed,
    Local,
    Full,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ContinuousLikelihoodObjective {
    pub(crate) shifted_objective_score: f64,
    pub(crate) global_negative_log_likelihood: f64,
    pub(crate) conditional_negative_log_likelihood: f64,
    pub(crate) boundary_penalty: f64,
    pub(crate) log_likelihood_shift: f64,
}

#[derive(Debug, Clone)]
pub(super) struct ContinuousReturnOptimizerDiagnostic {
    pub seed_objective_score: f64,
    pub local_objective_score: f64,
    pub final_objective_score: f64,
    pub local_evaluation_count: usize,
    pub stop_reason: &'static str,
    pub basin_diagnostics: Vec<ReturnComponentsStageContinuousOptimizerBasinDiagnostic>,
}

#[derive(Debug, Clone)]
pub(crate) struct ReturnComponentsStageContinuousReturnCalibrationSample {
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
    pub optimizer_diagnostic: ContinuousReturnOptimizerDiagnostic,
}

#[derive(Debug, Clone, Copy)]
struct ContinuousReturnDistributionProfile {
    sample_count: usize,
    mean_return: f64,
    return_volatility: f64,
    adjusted_skewness: f64,
    adjusted_excess_kurtosis: f64,
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
pub(crate) struct NigShapeSearchPoint {
    pub(crate) log_alpha: f64,
    pub(crate) buy_skew_coordinate: f64,
    pub(crate) sell_relative_coordinate: f64,
    pub(crate) log_delta_offset: f64,
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

#[derive(Debug, Clone, Copy)]
struct ContinuousMomentTargets {
    sample_count: usize,
    mean_return: f64,
    return_volatility: f64,
    adjusted_skewness: f64,
    adjusted_excess_kurtosis: f64,
    skewness_standard_error: f64,
    excess_kurtosis_standard_error: f64,
}

impl ContinuousReturnDistributionProfile {
    fn skewness_standard_error(self) -> f64 {
        (6.0 / self.sample_count.max(3) as f64).sqrt().max(0.10)
    }

    fn excess_kurtosis_standard_error(self) -> f64 {
        (24.0 / self.sample_count.max(4) as f64).sqrt().max(0.20)
    }
}

impl ContinuousMomentTargets {
    fn from_profile(observed_profile: ContinuousReturnDistributionProfile) -> Self {
        Self {
            sample_count: observed_profile.sample_count,
            mean_return: observed_profile.mean_return,
            return_volatility: observed_profile.return_volatility,
            adjusted_skewness: observed_profile.adjusted_skewness,
            adjusted_excess_kurtosis: observed_profile.adjusted_excess_kurtosis.max(0.0),
            skewness_standard_error: observed_profile.skewness_standard_error(),
            excess_kurtosis_standard_error: observed_profile.excess_kurtosis_standard_error(),
        }
    }
}

#[derive(Debug, Clone)]
struct ScoredNigCandidate {
    search_point: NigShapeSearchPoint,
    configuration: NigSharedPlusShiftConfiguration,
    objective_score: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum BasinSeedFamily {
    Overall,
    HighAlphaLowDelta,
    MediumAlphaMediumDelta,
    LowAlphaHighDelta,
    SkewLight,
    SkewHeavy,
    SymmetricLike,
    AsymmetricLike,
}

impl BasinSeedFamily {
    fn label(self) -> &'static str {
        match self {
            Self::Overall => "overall",
            Self::HighAlphaLowDelta => "high_alpha_low_delta",
            Self::MediumAlphaMediumDelta => "medium_alpha_medium_delta",
            Self::LowAlphaHighDelta => "low_alpha_high_delta",
            Self::SkewLight => "skew_light",
            Self::SkewHeavy => "skew_heavy",
            Self::SymmetricLike => "symmetric_like",
            Self::AsymmetricLike => "asymmetric_like",
        }
    }
}

#[derive(Debug, Clone)]
struct SeededNigCandidate {
    family: BasinSeedFamily,
    scored_candidate: ScoredNigCandidate,
}

#[derive(Debug, Clone)]
pub(super) struct OptimizedNigCandidate {
    pub configuration: NigSharedPlusShiftConfiguration,
    pub optimizer_diagnostic: ContinuousReturnOptimizerDiagnostic,
}

#[derive(Debug, Clone)]
struct OptimizedNigSearchCandidate {
    configuration: NigSharedPlusShiftConfiguration,
    family: BasinSeedFamily,
    seed_search_point: NigShapeSearchPoint,
    final_search_point: NigShapeSearchPoint,
    optimizer_diagnostic: ContinuousReturnOptimizerDiagnostic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct ContinuousLikelihoodObjectiveCacheKey {
    fidelity_tier: ContinuousLikelihoodFidelityTier,
    log_alpha_bits: u64,
    buy_skew_coordinate_bits: u64,
    sell_relative_coordinate_bits: u64,
    log_delta_offset_bits: u64,
}

#[derive(Debug, Clone, Copy)]
struct CachedSearchPointObjectiveEvaluation {
    configuration: NigSharedPlusShiftConfiguration,
    objective: ContinuousLikelihoodObjective,
}

#[derive(Debug, Clone, Copy)]
struct NigShapeSearchBounds {
    lower_bounds: [f64; 4],
    upper_bounds: [f64; 4],
}

#[derive(Debug, Clone)]
struct ContinuousReturnOptimizerContext {
    observed_returns: Vec<f64>,
    business_time_scales: Vec<f64>,
    sell_share_proxies: Vec<f64>,
    integrated_buy_intensities: Vec<f64>,
    integrated_sell_intensities: Vec<f64>,
    scale_bucket_profiles: Vec<ContinuousReturnScaleBucketProfile>,
    observed_return_volatility: f64,
    log_likelihood_shift: f64,
    open_session_sell_share_proxy: f64,
    seed_member_indices: Vec<usize>,
    local_member_indices: Vec<usize>,
    full_member_indices: Vec<usize>,
    cache: Arc<
        Mutex<
            BTreeMap<
                ContinuousLikelihoodObjectiveCacheKey,
                Option<CachedSearchPointObjectiveEvaluation>,
            >,
        >,
    >,
}

#[derive(Debug, Clone, Copy)]
struct PowellOptimizationOutcome {
    best_point: NigShapeSearchPoint,
    local_objective_score: f64,
    local_evaluation_count: usize,
    stop_reason: &'static str,
}

#[derive(Debug, Clone)]
struct LineSearchOutcome {
    best_vector: [f64; 4],
    best_evaluation: Option<CachedSearchPointObjectiveEvaluation>,
    evaluation_count: usize,
}

#[derive(Debug, Clone, Copy)]
enum ScanDirection {
    Lower,
    Upper,
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

pub(crate) fn build_continuous_return_calibration_sample_from_components(
    observed_returns: Vec<f64>,
    business_time_scales: Vec<f64>,
    sell_share_proxies: Vec<f64>,
    open_session_sell_share_proxy: f64,
) -> ReturnComponentsStageContinuousReturnCalibrationSample {
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

pub(crate) fn evaluate_configuration_likelihood_objective(
    configuration: NigSharedPlusShiftConfiguration,
    calibration_sample: &ReturnComponentsStageContinuousReturnCalibrationSample,
    fidelity_tier: ContinuousLikelihoodFidelityTier,
) -> ContinuousLikelihoodObjective {
    let optimizer_context = build_continuous_return_optimizer_context(calibration_sample);
    continuous_block_likelihood_objective(configuration, &optimizer_context, fidelity_tier)
}

pub(crate) fn distribution_profile_fit_score(
    fitted_returns: &[f64],
    observed_returns: &[f64],
) -> ReturnComponentsStageContinuousDistributionFitBreakdownDiagnostic {
    distribution_profile_fit_breakdown(
        build_distribution_profile(fitted_returns),
        build_distribution_profile(observed_returns),
    )
}

pub(crate) fn nig_search_point_from_configuration(
    calibration_sample: &ReturnComponentsStageContinuousReturnCalibrationSample,
    configuration: NigSharedPlusShiftConfiguration,
) -> Option<NigShapeSearchPoint> {
    let optimizer_context = build_continuous_return_optimizer_context(calibration_sample);
    let alpha = configuration.alpha;
    if !alpha.is_finite() || alpha <= RETURN_COMPONENTS_STAGE_MIN_REPRESENTABLE_POSITIVE_PARAMETER {
        return None;
    }

    let buy_beta_ratio = configuration.beta_0 / alpha;
    let sell_beta_ratio = (configuration.beta_0 + configuration.delta_beta) / alpha;
    let relative_share = relative_share_from_beta_ratios(buy_beta_ratio, sell_beta_ratio)?;
    let delta_center = inferred_delta_center(
        &optimizer_context,
        configuration.alpha,
        configuration.beta_0,
        configuration.delta_beta,
    )?;
    let delta_offset = configuration.delta / delta_center;
    if !delta_offset.is_finite()
        || delta_offset <= RETURN_COMPONENTS_STAGE_MIN_REPRESENTABLE_POSITIVE_PARAMETER
    {
        return None;
    }

    Some(NigShapeSearchPoint {
        log_alpha: configuration.alpha.ln(),
        buy_skew_coordinate: ratio_to_coordinate(buy_beta_ratio),
        sell_relative_coordinate: share_to_coordinate(relative_share),
        log_delta_offset: delta_offset.ln(),
    })
}

pub(crate) fn nig_configuration_from_search_point(
    calibration_sample: &ReturnComponentsStageContinuousReturnCalibrationSample,
    search_point: NigShapeSearchPoint,
) -> Option<NigSharedPlusShiftConfiguration> {
    let optimizer_context = build_continuous_return_optimizer_context(calibration_sample);
    configuration_from_shape_search_point(&optimizer_context, search_point)
}

fn build_continuous_return_optimizer_context(
    calibration_sample: &ReturnComponentsStageContinuousReturnCalibrationSample,
) -> ContinuousReturnOptimizerContext {
    let integrated_buy_intensities: Vec<f64> = calibration_sample
        .business_time_scales
        .iter()
        .zip(calibration_sample.sell_share_proxies.iter())
        .map(|(business_time_scale, sell_share_proxy)| {
            business_time_scale * (1.0 - sell_share_proxy.clamp(0.0, 1.0))
        })
        .collect();
    let integrated_sell_intensities: Vec<f64> = calibration_sample
        .business_time_scales
        .iter()
        .zip(calibration_sample.sell_share_proxies.iter())
        .map(|(business_time_scale, sell_share_proxy)| {
            business_time_scale * sell_share_proxy.clamp(0.0, 1.0)
        })
        .collect();

    ContinuousReturnOptimizerContext {
        observed_returns: calibration_sample.observed_returns.clone(),
        business_time_scales: calibration_sample.business_time_scales.clone(),
        sell_share_proxies: calibration_sample.sell_share_proxies.clone(),
        integrated_buy_intensities,
        integrated_sell_intensities,
        scale_bucket_profiles: calibration_sample.scale_bucket_profiles.clone(),
        observed_return_volatility: calibration_sample.observed_profile.return_volatility,
        log_likelihood_shift: log_likelihood_shift_from_observed_profile(
            calibration_sample.observed_profile,
        ),
        open_session_sell_share_proxy: calibration_sample.open_session_sell_share_proxy,
        seed_member_indices: deterministic_member_indices(
            calibration_sample.observed_returns.len(),
            RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_SEED_SAMPLE_COUNT,
        ),
        local_member_indices: deterministic_member_indices(
            calibration_sample.observed_returns.len(),
            RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_LOCAL_SAMPLE_COUNT,
        ),
        full_member_indices: deterministic_member_indices(
            calibration_sample.observed_returns.len(),
            calibration_sample.observed_returns.len(),
        ),
        cache: Arc::new(Mutex::new(BTreeMap::new())),
    }
}

pub(super) fn build_nig_search_candidates(
    calibration_sample: &ReturnComponentsStageContinuousReturnCalibrationSample,
    stage_seed: u64,
    solver_settings: &CalibrationOptimizerSettings,
) -> Vec<OptimizedNigCandidate> {
    let optimizer_context = build_continuous_return_optimizer_context(calibration_sample);
    let broad_candidate_count = solver_settings.max_iterations.clamp(
        RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_MIN_BROAD_CANDIDATE_COUNT,
        RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_MAX_BROAD_CANDIDATE_COUNT,
    );

    with_continuous_fit_thread_pool(solver_settings.cpu_count, || {
        let moment_candidate_count = broad_candidate_count
            .saturating_mul(RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_MOMENT_SEED_CANDIDATE_MULTIPLIER)
            .max(RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_MIN_MOMENT_SEED_CANDIDATE_COUNT);
        let theoretical_search_bounds =
            theoretical_search_bounds(calibration_sample.observed_profile, moment_candidate_count);
        let moment_seed_candidates = build_moment_seed_candidates(
            &optimizer_context,
            calibration_sample,
            theoretical_search_bounds,
            moment_candidate_count,
        );
        let broad_search_regimes = build_search_regimes_from_moment_seeds(
            &optimizer_context,
            moment_seed_candidates.as_slice(),
            theoretical_search_bounds,
        );
        let search_bounds = search_bounds_from_regimes(broad_search_regimes.as_slice());
        let broad_search_points =
            build_broad_search_points(broad_search_regimes.as_slice(), broad_candidate_count);
        let broad_scored_candidates = score_search_points(
            &optimizer_context,
            broad_search_points.as_slice(),
            ContinuousLikelihoodFidelityTier::Seed,
        );

        let seed_candidates = build_basin_seed_candidates(
            broad_scored_candidates.as_slice(),
            broad_search_regimes.as_slice(),
        );

        let optimized_seed_candidates = optimize_seed_candidates(
            &optimizer_context,
            seed_candidates.as_slice(),
            search_bounds,
            solver_settings,
        );
        let retained_basin_candidates = retain_distinct_basin_candidates(
            optimized_seed_candidates,
            search_bounds,
            RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_ELITE_COUNT,
        );
        let basin_diagnostics = build_optimizer_basin_diagnostics(
            retained_basin_candidates.as_slice(),
            calibration_sample,
            stage_seed,
        );

        let mut optimized_candidates: Vec<OptimizedNigCandidate> = retained_basin_candidates
            .into_iter()
            .map(|candidate| OptimizedNigCandidate {
                configuration: candidate.configuration,
                optimizer_diagnostic: ContinuousReturnOptimizerDiagnostic {
                    basin_diagnostics: basin_diagnostics.clone(),
                    ..candidate.optimizer_diagnostic
                },
            })
            .collect();
        optimized_candidates.sort_by(|left, right| {
            left.optimizer_diagnostic
                .final_objective_score
                .total_cmp(&right.optimizer_diagnostic.final_objective_score)
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

        deduplicate_candidates_by_key(optimized_candidates, |candidate| {
            candidate_configuration_key(candidate.configuration)
        })
    })
}

pub(super) fn evaluate_continuous_nig_candidate(
    configuration: NigSharedPlusShiftConfiguration,
    calibration_sample: &ReturnComponentsStageContinuousReturnCalibrationSample,
    summary_statistics: ReturnComponentsStageObservedSignalSummaryStatistics,
    stage_seed: u64,
) -> ContinuousReturnCandidateEvaluation {
    let optimizer_context = build_continuous_return_optimizer_context(calibration_sample);

    let seed_objective = continuous_block_likelihood_objective(
        configuration,
        &optimizer_context,
        ContinuousLikelihoodFidelityTier::Seed,
    );
    let local_objective = continuous_block_likelihood_objective(
        configuration,
        &optimizer_context,
        ContinuousLikelihoodFidelityTier::Local,
    );
    let likelihood_objective = continuous_block_likelihood_objective(
        configuration,
        &optimizer_context,
        ContinuousLikelihoodFidelityTier::Full,
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
            optimizer_seed_objective: seed_objective.shifted_objective_score,
            optimizer_local_objective: local_objective.shifted_objective_score,
            optimizer_final_objective: likelihood_objective.shifted_objective_score,
            optimizer_local_evaluation_count: 0,
            optimizer_stop_reason: "not_optimized".to_string(),
            ..profile_fit_breakdown
        },
        optimizer_diagnostic: ContinuousReturnOptimizerDiagnostic {
            seed_objective_score: seed_objective.shifted_objective_score,
            local_objective_score: local_objective.shifted_objective_score,
            final_objective_score: likelihood_objective.shifted_objective_score,
            local_evaluation_count: 0,
            stop_reason: "not_optimized",
            basin_diagnostics: Vec::new(),
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
    regimes: &[NigSearchRegime],
    broad_candidate_count: usize,
) -> Vec<NigShapeSearchPoint> {
    if regimes.is_empty() {
        return Vec::new();
    }

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

fn build_search_regimes_from_moment_seeds(
    optimizer_context: &ContinuousReturnOptimizerContext,
    moment_seed_candidates: &[ScoredNigCandidate],
    theoretical_search_bounds: NigShapeSearchBounds,
) -> Vec<NigSearchRegime> {
    let mut profiled_search_regimes: Vec<NigSearchRegime> = moment_seed_candidates
        .par_iter()
        .filter_map(|moment_seed_candidate| {
            build_profiled_search_regime(
                optimizer_context,
                moment_seed_candidate.search_point,
                theoretical_search_bounds,
            )
        })
        .collect();
    profiled_search_regimes.sort_by(|left, right| {
        left.log_alpha_center
            .total_cmp(&right.log_alpha_center)
            .then_with(|| left.buy_skew_center.total_cmp(&right.buy_skew_center))
            .then_with(|| {
                left.sell_relative_center
                    .total_cmp(&right.sell_relative_center)
            })
            .then_with(|| left.log_delta_center.total_cmp(&right.log_delta_center))
    });

    profiled_search_regimes
}

fn build_profiled_search_regime(
    optimizer_context: &ContinuousReturnOptimizerContext,
    seed_search_point: NigShapeSearchPoint,
    theoretical_search_bounds: NigShapeSearchBounds,
) -> Option<NigSearchRegime> {
    let seed_evaluation = evaluate_cached_search_point(
        optimizer_context,
        seed_search_point,
        ContinuousLikelihoodFidelityTier::Local,
    )?;

    let seed_profile_score = profile_likelihood_score(seed_evaluation.objective);

    let profile_score_threshold = seed_profile_score
        + profile_likelihood_score_threshold(optimizer_context.local_member_indices.len());

    let seed_vector = search_point_to_vector(seed_search_point);

    let mut lower_bounds = [0.0; 4];
    let mut upper_bounds = [0.0; 4];

    for coordinate_index in 0..4 {
        lower_bounds[coordinate_index] = profiled_coordinate_bound(
            optimizer_context,
            seed_vector,
            coordinate_index,
            theoretical_search_bounds.lower_bounds[coordinate_index],
            profile_score_threshold,
            ScanDirection::Lower,
        )?;

        upper_bounds[coordinate_index] = profiled_coordinate_bound(
            optimizer_context,
            seed_vector,
            coordinate_index,
            theoretical_search_bounds.upper_bounds[coordinate_index],
            profile_score_threshold,
            ScanDirection::Upper,
        )?;
    }

    Some(NigSearchRegime {
        log_alpha_center: seed_search_point.log_alpha,
        buy_skew_center: seed_search_point.buy_skew_coordinate,
        sell_relative_center: seed_search_point.sell_relative_coordinate,
        log_delta_center: seed_search_point.log_delta_offset,
        log_alpha_half_span: 0.5 * (upper_bounds[0] - lower_bounds[0]).max(1.0e-6),
        buy_skew_half_span: 0.5 * (upper_bounds[1] - lower_bounds[1]).max(1.0e-6),
        sell_relative_half_span: 0.5 * (upper_bounds[2] - lower_bounds[2]).max(1.0e-6),
        log_delta_half_span: 0.5 * (upper_bounds[3] - lower_bounds[3]).max(1.0e-6),
    })
}

fn theoretical_search_bounds(
    observed_profile: ContinuousReturnDistributionProfile,
    moment_candidate_count: usize,
) -> NigShapeSearchBounds {
    let (alpha_lower_bound, alpha_upper_bound) = theoretical_alpha_bounds(observed_profile);

    let relative_share_lower_bound = minimum_profiled_relative_share(moment_candidate_count);
    let relative_share_upper_bound = 1.0 - relative_share_lower_bound;

    NigShapeSearchBounds {
        lower_bounds: [
            alpha_lower_bound.ln(),
            ratio_to_coordinate(-maximum_profiled_open_beta_ratio()),
            share_to_coordinate(relative_share_lower_bound),
            theoretical_log_delta_offset_bounds(observed_profile).0,
        ],
        upper_bounds: [
            alpha_upper_bound.ln(),
            ratio_to_coordinate(maximum_profiled_open_beta_ratio()),
            share_to_coordinate(relative_share_upper_bound),
            theoretical_log_delta_offset_bounds(observed_profile).1,
        ],
    }
}

fn theoretical_alpha_bounds(observed_profile: ContinuousReturnDistributionProfile) -> (f64, f64) {
    let observed_variance = observed_profile
        .return_volatility
        .powi(2)
        .max(RETURN_COMPONENTS_STAGE_MIN_REPRESENTABLE_POSITIVE_PARAMETER);

    let excess_kurtosis_standard_error = observed_profile.excess_kurtosis_standard_error();
    let observed_excess_kurtosis = observed_profile.adjusted_excess_kurtosis.max(0.0);

    let lower_plausible_excess_kurtosis = (observed_excess_kurtosis
        - RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_PROFILE_CONFIDENCE_Z_SCORE
            * excess_kurtosis_standard_error)
        .max(0.5 * excess_kurtosis_standard_error)
        .max(RETURN_COMPONENTS_STAGE_MIN_REPRESENTABLE_POSITIVE_PARAMETER);
    let upper_plausible_excess_kurtosis = (observed_excess_kurtosis
        + RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_PROFILE_CONFIDENCE_Z_SCORE
            * excess_kurtosis_standard_error)
        .max(lower_plausible_excess_kurtosis * 1.25);

    let lower_alpha_bound = (3.0 / (observed_variance * upper_plausible_excess_kurtosis))
        .sqrt()
        .max(RETURN_COMPONENTS_STAGE_MIN_REPRESENTABLE_POSITIVE_PARAMETER);
    let upper_alpha_bound = (3.0 / (observed_variance * lower_plausible_excess_kurtosis))
        .sqrt()
        .max(lower_alpha_bound * 1.5);

    (lower_alpha_bound, upper_alpha_bound)
}

fn theoretical_log_delta_offset_bounds(
    observed_profile: ContinuousReturnDistributionProfile,
) -> (f64, f64) {
    let sample_count = observed_profile.sample_count.max(3) as f64;

    let degrees_of_freedom = (sample_count - 1.0).max(1.0);

    let lower_tail_quantile = approximate_chi_square_quantile(degrees_of_freedom, 0.025);
    let upper_tail_quantile = approximate_chi_square_quantile(degrees_of_freedom, 0.975);

    let lower_variance_ratio = (degrees_of_freedom / upper_tail_quantile)
        .max(RETURN_COMPONENTS_STAGE_MIN_REPRESENTABLE_POSITIVE_PARAMETER);
    let upper_variance_ratio =
        (degrees_of_freedom / lower_tail_quantile).max(lower_variance_ratio * 1.5);

    (lower_variance_ratio.ln(), upper_variance_ratio.ln())
}

fn minimum_profiled_relative_share(moment_candidate_count: usize) -> f64 {
    let denominator = (moment_candidate_count.max(2) + 1) as f64;

    (1.0 / denominator).clamp(0.0f64.next_up(), 0.5)
}

fn build_moment_seed_candidates(
    optimizer_context: &ContinuousReturnOptimizerContext,
    calibration_sample: &ReturnComponentsStageContinuousReturnCalibrationSample,
    theoretical_search_bounds: NigShapeSearchBounds,
    moment_candidate_count: usize,
) -> Vec<ScoredNigCandidate> {
    let moment_targets = ContinuousMomentTargets::from_profile(calibration_sample.observed_profile);

    let alpha_interval = theoretical_alpha_bounds(calibration_sample.observed_profile);

    let relative_share_lower_bound = coordinate_to_share(theoretical_search_bounds.lower_bounds[2]);
    let relative_share_upper_bound = coordinate_to_share(theoretical_search_bounds.upper_bounds[2]);

    let beta_ratio_lower_bound = coordinate_to_ratio(theoretical_search_bounds.lower_bounds[1]);
    let beta_ratio_upper_bound = coordinate_to_ratio(theoretical_search_bounds.upper_bounds[1]);

    let mut ranked_candidates: Vec<ScoredNigCandidate> = (0..moment_candidate_count)
        .into_par_iter()
        .filter_map(|candidate_index| {
            let alpha_quantile =
                van_der_corput(candidate_index as u64 + 1, 2).clamp(0.0f64.next_up(), 1.0);

            let buy_ratio_quantile =
                van_der_corput(candidate_index as u64 + 1, 3).clamp(0.0f64.next_up(), 1.0);
            let sell_ratio_quantile = van_der_corput(candidate_index as u64 + 1, 5)
                .clamp(relative_share_lower_bound, relative_share_upper_bound);

            let alpha =
                alpha_interval.0 * (alpha_interval.1 / alpha_interval.0).powf(alpha_quantile);

            let buy_beta_ratio = beta_ratio_lower_bound
                + (beta_ratio_upper_bound - beta_ratio_lower_bound) * buy_ratio_quantile;
            let sell_beta_ratio = beta_ratio_lower_bound
                + (buy_beta_ratio - beta_ratio_lower_bound) * sell_ratio_quantile;

            let search_point = NigShapeSearchPoint {
                log_alpha: alpha.ln(),
                buy_skew_coordinate: ratio_to_coordinate(buy_beta_ratio),
                sell_relative_coordinate: share_to_coordinate(relative_share_from_beta_ratios(
                    buy_beta_ratio,
                    sell_beta_ratio,
                )?),
                log_delta_offset: 0.0,
            };

            let configuration =
                configuration_from_shape_search_point(optimizer_context, search_point)?;

            let objective_score =
                moment_seed_objective_score(configuration, optimizer_context, moment_targets)?;

            Some(ScoredNigCandidate {
                search_point,
                configuration,
                objective_score,
            })
        })
        .collect();

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
            .then_with(|| {
                left.configuration
                    .delta_beta
                    .total_cmp(&right.configuration.delta_beta)
            })
    });

    let deduplicated_candidates = deduplicate_candidates_by_key(ranked_candidates, |candidate| {
        candidate_configuration_key(candidate.configuration)
    });
    if deduplicated_candidates.is_empty() {
        let midpoint_search_point = NigShapeSearchPoint {
            log_alpha: 0.5
                * (theoretical_search_bounds.lower_bounds[0]
                    + theoretical_search_bounds.upper_bounds[0]),
            buy_skew_coordinate: 0.5
                * (theoretical_search_bounds.lower_bounds[1]
                    + theoretical_search_bounds.upper_bounds[1]),
            sell_relative_coordinate: 0.5
                * (theoretical_search_bounds.lower_bounds[2]
                    + theoretical_search_bounds.upper_bounds[2]),
            log_delta_offset: 0.0,
        };

        let Some(configuration) =
            configuration_from_shape_search_point(optimizer_context, midpoint_search_point)
        else {
            return Vec::new();
        };

        let Some(objective_score) =
            moment_seed_objective_score(configuration, optimizer_context, moment_targets)
        else {
            return Vec::new();
        };

        return vec![ScoredNigCandidate {
            search_point: midpoint_search_point,
            configuration,
            objective_score,
        }];
    }

    deduplicated_candidates
        .into_iter()
        .take(RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_REGIME_COUNT)
        .collect()
}

fn moment_seed_objective_score(
    configuration: NigSharedPlusShiftConfiguration,
    optimizer_context: &ContinuousReturnOptimizerContext,
    moment_targets: ContinuousMomentTargets,
) -> Option<f64> {
    let (fitted_mean_return, fitted_variance, fitted_skewness, fitted_excess_kurtosis) =
        theoretical_net_process_moments(
            configuration,
            optimizer_context.open_session_sell_share_proxy,
        )?;

    let mean_scale = (moment_targets.return_volatility
        / (moment_targets.sample_count.max(2) as f64).sqrt())
    .max(RETURN_COMPONENTS_STAGE_MIN_REPRESENTABLE_POSITIVE_PARAMETER.sqrt());

    let variance_scale = moment_targets
        .return_volatility
        .powi(2)
        .max(RETURN_COMPONENTS_STAGE_MIN_REPRESENTABLE_POSITIVE_PARAMETER);

    let mean_fit =
        scale_aware_absolute_error(fitted_mean_return, moment_targets.mean_return, mean_scale);
    let variance_fit = scale_aware_absolute_error(fitted_variance, variance_scale, variance_scale);
    let skewness_fit = scale_aware_absolute_error(
        fitted_skewness,
        moment_targets.adjusted_skewness,
        moment_targets.skewness_standard_error,
    );
    let excess_kurtosis_fit = scale_aware_absolute_error(
        fitted_excess_kurtosis,
        moment_targets.adjusted_excess_kurtosis,
        moment_targets.excess_kurtosis_standard_error,
    );

    Some(0.15 * mean_fit + 0.10 * variance_fit + 0.35 * skewness_fit + 0.40 * excess_kurtosis_fit)
}

fn profile_likelihood_score(objective: ContinuousLikelihoodObjective) -> f64 {
    0.45 * objective.global_negative_log_likelihood
        + 0.55 * objective.conditional_negative_log_likelihood
        + objective.boundary_penalty
}

fn profile_likelihood_score_threshold(local_sample_count: usize) -> f64 {
    RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_PROFILE_LIKELIHOOD_CHI_SQUARE_95
        / (2.0 * local_sample_count.max(1) as f64)
}

fn profiled_coordinate_bound(
    optimizer_context: &ContinuousReturnOptimizerContext,
    seed_vector: [f64; 4],
    coordinate_index: usize,
    coordinate_bound: f64,
    profile_score_threshold: f64,
    scan_direction: ScanDirection,
) -> Option<f64> {
    let seed_evaluation = evaluate_cached_search_point(
        optimizer_context,
        vector_to_search_point(seed_vector),
        ContinuousLikelihoodFidelityTier::Local,
    )?;

    let seed_profile_score = profile_likelihood_score(seed_evaluation.objective);

    let mut accepted_coordinate = seed_vector[coordinate_index];
    let mut rejected_coordinate: Option<f64> = None;

    for scan_step_index in 1..=RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_PROFILE_SCAN_STEP_COUNT {
        let scan_fraction = scan_step_index as f64
            / RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_PROFILE_SCAN_STEP_COUNT as f64;

        let candidate_coordinate = match scan_direction {
            ScanDirection::Lower => {
                seed_vector[coordinate_index]
                    + (coordinate_bound - seed_vector[coordinate_index]) * scan_fraction
            }
            ScanDirection::Upper => {
                seed_vector[coordinate_index]
                    + (coordinate_bound - seed_vector[coordinate_index]) * scan_fraction
            }
        };

        let mut candidate_vector = seed_vector;
        candidate_vector[coordinate_index] = candidate_coordinate;

        let candidate_evaluation = evaluate_cached_search_point(
            optimizer_context,
            vector_to_search_point(candidate_vector),
            ContinuousLikelihoodFidelityTier::Local,
        );

        let candidate_profile_score = candidate_evaluation
            .map(|evaluation| profile_likelihood_score(evaluation.objective))
            .unwrap_or(f64::INFINITY);
        if candidate_profile_score <= seed_profile_score + profile_score_threshold {
            accepted_coordinate = candidate_coordinate;
            continue;
        }

        rejected_coordinate = Some(candidate_coordinate);

        break;
    }

    let Some(mut rejected_coordinate) = rejected_coordinate else {
        return Some(coordinate_bound);
    };
    let mut accepted_coordinate = accepted_coordinate;

    for _ in 0..RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_PROFILE_BISECTION_STEP_COUNT {
        let midpoint_coordinate = 0.5 * (accepted_coordinate + rejected_coordinate);

        let mut candidate_vector = seed_vector;
        candidate_vector[coordinate_index] = midpoint_coordinate;

        let candidate_evaluation = evaluate_cached_search_point(
            optimizer_context,
            vector_to_search_point(candidate_vector),
            ContinuousLikelihoodFidelityTier::Local,
        );

        let candidate_profile_score = candidate_evaluation
            .map(|evaluation| profile_likelihood_score(evaluation.objective))
            .unwrap_or(f64::INFINITY);
        if candidate_profile_score <= seed_profile_score + profile_score_threshold {
            accepted_coordinate = midpoint_coordinate;
        } else {
            rejected_coordinate = midpoint_coordinate;
        }
    }

    Some(accepted_coordinate)
}

fn approximate_chi_square_quantile(degrees_of_freedom: f64, probability_level: f64) -> f64 {
    let standardized_quantile = standard_normal_quantile(probability_level);
    let wilson_hilferty_term = 1.0 - 2.0 / (9.0 * degrees_of_freedom)
        + standardized_quantile * (2.0 / (9.0 * degrees_of_freedom)).sqrt();

    (degrees_of_freedom * wilson_hilferty_term.powi(3))
        .max(RETURN_COMPONENTS_STAGE_MIN_REPRESENTABLE_POSITIVE_PARAMETER)
}

fn standard_normal_quantile(probability_level: f64) -> f64 {
    if probability_level <= 0.025 {
        -RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_PROFILE_CONFIDENCE_Z_SCORE
    } else {
        RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_PROFILE_CONFIDENCE_Z_SCORE
    }
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
    optimizer_context: &ContinuousReturnOptimizerContext,
    search_points: &[NigShapeSearchPoint],
    fidelity_tier: ContinuousLikelihoodFidelityTier,
) -> Vec<ScoredNigCandidate> {
    search_points
        .par_iter()
        .filter_map(|search_point| {
            evaluate_cached_search_point(optimizer_context, *search_point, fidelity_tier).map(
                |cached_evaluation| ScoredNigCandidate {
                    search_point: *search_point,
                    configuration: cached_evaluation.configuration,
                    objective_score: cached_evaluation.objective.shifted_objective_score,
                },
            )
        })
        .collect()
}

fn build_basin_seed_candidates(
    scored_candidates: &[ScoredNigCandidate],
    broad_search_regimes: &[NigSearchRegime],
) -> Vec<SeededNigCandidate> {
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

    let deduplicated_candidates = deduplicate_candidates_by_key(ranked_candidates, |candidate| {
        candidate_configuration_key(candidate.configuration)
    });

    let Some(feature_thresholds) =
        BroadSeedFeatureThresholds::from_candidates(deduplicated_candidates.as_slice())
    else {
        return Vec::new();
    };

    let mut retained_candidates: Vec<SeededNigCandidate> = Vec::new();
    let mut seen_seed_keys = BTreeSet::new();

    for family in [
        BasinSeedFamily::Overall,
        BasinSeedFamily::HighAlphaLowDelta,
        BasinSeedFamily::MediumAlphaMediumDelta,
        BasinSeedFamily::LowAlphaHighDelta,
        BasinSeedFamily::SkewLight,
        BasinSeedFamily::SkewHeavy,
        BasinSeedFamily::SymmetricLike,
        BasinSeedFamily::AsymmetricLike,
    ] {
        let Some(seed_candidate) = select_family_seed_candidate(
            deduplicated_candidates.as_slice(),
            feature_thresholds,
            family,
        ) else {
            continue;
        };

        let seed_key = search_point_bits_key(seed_candidate.search_point);
        if seen_seed_keys.insert(seed_key) {
            retained_candidates.push(SeededNigCandidate {
                family,
                scored_candidate: seed_candidate.clone(),
            });
        }
    }

    let mut retained_regimes = BTreeSet::new();
    for scored_candidate in deduplicated_candidates {
        if retained_candidates.len() >= RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_ELITE_COUNT {
            break;
        }
        let regime_index =
            nearest_search_regime_index(scored_candidate.search_point, broad_search_regimes);
        if !retained_regimes.insert(regime_index) {
            continue;
        }

        let seed_key = search_point_bits_key(scored_candidate.search_point);
        if seen_seed_keys.insert(seed_key) {
            retained_candidates.push(SeededNigCandidate {
                family: BasinSeedFamily::Overall,
                scored_candidate,
            });
        }
    }

    retained_candidates
}

#[derive(Debug, Clone, Copy)]
struct BroadSeedFeatureThresholds {
    low_alpha: f64,
    high_alpha: f64,
    low_delta: f64,
    high_delta: f64,
    low_skew_magnitude: f64,
    high_skew_magnitude: f64,
    low_skew_asymmetry: f64,
    high_skew_asymmetry: f64,
}

impl BroadSeedFeatureThresholds {
    fn from_candidates(candidates: &[ScoredNigCandidate]) -> Option<Self> {
        if candidates.is_empty() {
            return None;
        }

        let mut log_alpha_values: Vec<f64> = candidates
            .iter()
            .map(|candidate| candidate.search_point.log_alpha)
            .collect();
        let mut log_delta_values: Vec<f64> = candidates
            .iter()
            .map(|candidate| candidate.search_point.log_delta_offset)
            .collect();

        let mut skew_magnitudes: Vec<f64> = candidates
            .iter()
            .filter_map(|candidate| {
                basin_candidate_features(candidate.search_point).map(|(_, value)| value)
            })
            .collect();
        let mut skew_asymmetries: Vec<f64> = candidates
            .iter()
            .filter_map(|candidate| {
                basin_candidate_features(candidate.search_point).map(|(value, _)| value)
            })
            .collect();

        log_alpha_values.sort_by(|left, right| left.total_cmp(right));
        log_delta_values.sort_by(|left, right| left.total_cmp(right));

        skew_magnitudes.sort_by(|left, right| left.total_cmp(right));
        skew_asymmetries.sort_by(|left, right| left.total_cmp(right));

        Some(Self {
            low_alpha: sorted_quantile(log_alpha_values.as_slice(), 1.0 / 3.0),
            high_alpha: sorted_quantile(log_alpha_values.as_slice(), 2.0 / 3.0),
            low_delta: sorted_quantile(log_delta_values.as_slice(), 1.0 / 3.0),
            high_delta: sorted_quantile(log_delta_values.as_slice(), 2.0 / 3.0),
            low_skew_magnitude: sorted_quantile(skew_magnitudes.as_slice(), 1.0 / 3.0),
            high_skew_magnitude: sorted_quantile(skew_magnitudes.as_slice(), 2.0 / 3.0),
            low_skew_asymmetry: sorted_quantile(skew_asymmetries.as_slice(), 1.0 / 3.0),
            high_skew_asymmetry: sorted_quantile(skew_asymmetries.as_slice(), 2.0 / 3.0),
        })
    }
}

fn select_family_seed_candidate(
    candidates: &[ScoredNigCandidate],
    thresholds: BroadSeedFeatureThresholds,
    family: BasinSeedFamily,
) -> Option<&ScoredNigCandidate> {
    candidates
        .iter()
        .find(|candidate| family_accepts_candidate(candidate.search_point, thresholds, family))
}

fn family_accepts_candidate(
    search_point: NigShapeSearchPoint,
    thresholds: BroadSeedFeatureThresholds,
    family: BasinSeedFamily,
) -> bool {
    let Some((skew_asymmetry, skew_magnitude)) = basin_candidate_features(search_point) else {
        return false;
    };

    match family {
        BasinSeedFamily::Overall => true,
        BasinSeedFamily::HighAlphaLowDelta => {
            search_point.log_alpha >= thresholds.high_alpha
                && search_point.log_delta_offset <= thresholds.low_delta
        }
        BasinSeedFamily::MediumAlphaMediumDelta => {
            search_point.log_alpha >= thresholds.low_alpha
                && search_point.log_alpha <= thresholds.high_alpha
                && search_point.log_delta_offset >= thresholds.low_delta
                && search_point.log_delta_offset <= thresholds.high_delta
        }
        BasinSeedFamily::LowAlphaHighDelta => {
            search_point.log_alpha <= thresholds.low_alpha
                && search_point.log_delta_offset >= thresholds.high_delta
        }
        BasinSeedFamily::SkewLight => skew_magnitude <= thresholds.low_skew_magnitude,
        BasinSeedFamily::SkewHeavy => skew_magnitude >= thresholds.high_skew_magnitude,
        BasinSeedFamily::SymmetricLike => skew_asymmetry <= thresholds.low_skew_asymmetry,
        BasinSeedFamily::AsymmetricLike => skew_asymmetry >= thresholds.high_skew_asymmetry,
    }
}

fn basin_candidate_features(search_point: NigShapeSearchPoint) -> Option<(f64, f64)> {
    let buy_beta_ratio = coordinate_to_ratio(search_point.buy_skew_coordinate);
    let relative_share = coordinate_to_share(search_point.sell_relative_coordinate);
    let sell_beta_ratio: f64 = sell_beta_ratio_from_relative_share(buy_beta_ratio, relative_share);

    Some((
        (buy_beta_ratio - sell_beta_ratio).abs(),
        0.5 * (buy_beta_ratio.abs() + sell_beta_ratio.abs()),
    ))
}

fn deduplicate_candidates_by_key<T>(
    candidates: Vec<T>,
    key_builder: impl Fn(&T) -> String,
) -> Vec<T> {
    let mut retained = Vec::new();
    let mut seen = BTreeSet::new();

    for candidate in candidates {
        let candidate_key = key_builder(&candidate);
        if seen.insert(candidate_key) {
            retained.push(candidate);
        }
    }

    retained
}

fn candidate_configuration_key(configuration: NigSharedPlusShiftConfiguration) -> String {
    format!(
        "{:.6}|{:.6}|{:.12}|{:.6}|{:.6}",
        configuration.alpha,
        configuration.beta_0,
        configuration.delta,
        configuration.mu,
        configuration.delta_beta,
    )
}

fn search_point_bits_key(search_point: NigShapeSearchPoint) -> String {
    format!(
        "{}|{}|{}|{}",
        search_point.log_alpha.to_bits(),
        search_point.buy_skew_coordinate.to_bits(),
        search_point.sell_relative_coordinate.to_bits(),
        search_point.log_delta_offset.to_bits(),
    )
}

fn evaluate_cached_search_point(
    optimizer_context: &ContinuousReturnOptimizerContext,
    shape_search_point: NigShapeSearchPoint,
    fidelity_tier: ContinuousLikelihoodFidelityTier,
) -> Option<CachedSearchPointObjectiveEvaluation> {
    let cache_key = ContinuousLikelihoodObjectiveCacheKey {
        fidelity_tier,
        log_alpha_bits: shape_search_point.log_alpha.to_bits(),
        buy_skew_coordinate_bits: shape_search_point.buy_skew_coordinate.to_bits(),
        sell_relative_coordinate_bits: shape_search_point.sell_relative_coordinate.to_bits(),
        log_delta_offset_bits: shape_search_point.log_delta_offset.to_bits(),
    };
    if let Ok(cache_guard) = optimizer_context.cache.lock() {
        if let Some(cached_value) = cache_guard.get(&cache_key) {
            return *cached_value;
        }
    }

    let cached_value = configuration_from_shape_search_point(optimizer_context, shape_search_point)
        .map(|configuration| CachedSearchPointObjectiveEvaluation {
            configuration,
            objective: continuous_block_likelihood_objective(
                configuration,
                optimizer_context,
                fidelity_tier,
            ),
        })
        .filter(|cached_evaluation| {
            cached_evaluation
                .objective
                .shifted_objective_score
                .is_finite()
        });

    if let Ok(mut cache_guard) = optimizer_context.cache.lock() {
        cache_guard.insert(cache_key, cached_value);
    }

    cached_value
}

fn configuration_from_shape_search_point(
    optimizer_context: &ContinuousReturnOptimizerContext,
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
    let delta_center = inferred_delta_center(optimizer_context, alpha, beta_0, delta_beta)?;
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
    optimizer_context: &ContinuousReturnOptimizerContext,
    alpha: f64,
    beta_0: f64,
    delta_beta: f64,
) -> Option<f64> {
    let (_, variance_per_unit, _, _) = theoretical_net_process_moments(
        NigSharedPlusShiftConfiguration {
            alpha,
            beta_0,
            delta: 1.0,
            mu: 0.0,
            delta_beta,
        },
        optimizer_context.open_session_sell_share_proxy,
    )?;

    let target_variance = optimizer_context
        .observed_return_volatility
        .powi(2)
        .max(RETURN_COMPONENTS_STAGE_MIN_REPRESENTABLE_POSITIVE_PARAMETER);
    let variance_per_unit =
        variance_per_unit.max(RETURN_COMPONENTS_STAGE_MIN_REPRESENTABLE_POSITIVE_PARAMETER);

    Some(
        (target_variance / variance_per_unit)
            .max(RETURN_COMPONENTS_STAGE_MIN_REPRESENTABLE_POSITIVE_PARAMETER),
    )
}

fn ratio_to_coordinate(ratio: f64) -> f64 {
    let bounded_ratio = clamp_open_beta_ratio(ratio);
    0.5 * ((1.0 + bounded_ratio) / (1.0 - bounded_ratio)).ln()
}

fn share_to_coordinate(share: f64) -> f64 {
    let bounded_share = share.clamp(0.0f64.next_up(), 1.0f64.next_down());
    (bounded_share / (1.0 - bounded_share)).ln()
}

fn coordinate_to_ratio(coordinate: f64) -> f64 {
    clamp_open_beta_ratio(coordinate.tanh())
}

fn coordinate_to_share(coordinate: f64) -> f64 {
    logistic(coordinate).clamp(0.0f64.next_up(), 1.0f64.next_down())
}

fn sell_beta_ratio_from_relative_share(buy_beta_ratio: f64, relative_share: f64) -> f64 {
    let lower_bound = minimum_open_beta_ratio();
    lower_bound + (buy_beta_ratio - lower_bound) * relative_share
}

fn relative_share_from_beta_ratios(buy_beta_ratio: f64, sell_beta_ratio: f64) -> Option<f64> {
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

fn maximum_profiled_open_beta_ratio() -> f64 {
    1.0 - preferred_alpha_beta_margin_ratio()
}

fn minimum_open_beta_ratio() -> f64 {
    (-1.0f64).next_up()
}

fn maximum_open_beta_ratio() -> f64 {
    1.0f64.next_down()
}

fn continuous_block_likelihood_objective(
    configuration: NigSharedPlusShiftConfiguration,
    optimizer_context: &ContinuousReturnOptimizerContext,
    fidelity_tier: ContinuousLikelihoodFidelityTier,
) -> ContinuousLikelihoodObjective {
    let boundary_penalty = alpha_beta_boundary_penalty(configuration);

    let integration_step_count = integration_step_count_for_fidelity_tier(fidelity_tier);

    let selected_member_indices =
        member_indices_for_fidelity_tier(optimizer_context, fidelity_tier);

    let global_negative_log_likelihood = average_negative_log_likelihood(
        configuration,
        optimizer_context,
        selected_member_indices,
        integration_step_count,
    )
    .unwrap_or(f64::INFINITY);

    let conditional_negative_log_likelihood = if optimizer_context.scale_bucket_profiles.is_empty()
    {
        global_negative_log_likelihood
    } else {
        arithmetic_mean(
            optimizer_context
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
                        optimizer_context,
                        bucket_member_indices.as_slice(),
                        integration_step_count,
                    )
                })
                .collect::<Vec<_>>()
                .as_slice(),
        )
    };

    let log_likelihood_shift = optimizer_context.log_likelihood_shift;

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

fn member_indices_for_fidelity_tier(
    optimizer_context: &ContinuousReturnOptimizerContext,
    fidelity_tier: ContinuousLikelihoodFidelityTier,
) -> &[usize] {
    match fidelity_tier {
        ContinuousLikelihoodFidelityTier::Seed => optimizer_context.seed_member_indices.as_slice(),
        ContinuousLikelihoodFidelityTier::Local => {
            optimizer_context.local_member_indices.as_slice()
        }
        ContinuousLikelihoodFidelityTier::Full => optimizer_context.full_member_indices.as_slice(),
    }
}

fn integration_step_count_for_fidelity_tier(
    fidelity_tier: ContinuousLikelihoodFidelityTier,
) -> usize {
    match fidelity_tier {
        ContinuousLikelihoodFidelityTier::Seed => {
            RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_SEED_INTEGRATION_STEP_COUNT
        }
        ContinuousLikelihoodFidelityTier::Local => {
            RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_LOCAL_INTEGRATION_STEP_COUNT
        }
        ContinuousLikelihoodFidelityTier::Full => {
            RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_FINAL_INTEGRATION_STEP_COUNT
        }
    }
}

fn average_negative_log_likelihood(
    configuration: NigSharedPlusShiftConfiguration,
    optimizer_context: &ContinuousReturnOptimizerContext,
    member_indices: &[usize],
    integration_step_count: usize,
) -> Option<f64> {
    let driver_parameters = configuration.derive_driver_parameters().ok()?;
    let mut log_likelihood_sum = 0.0;
    let mut evaluated_count = 0usize;

    for member_index in member_indices {
        let observed_return = *optimizer_context.observed_returns.get(*member_index)?;
        let business_time_scale = *optimizer_context.business_time_scales.get(*member_index)?;
        let sell_share_proxy = *optimizer_context.sell_share_proxies.get(*member_index)?;
        let integrated_buy_intensity = *optimizer_context
            .integrated_buy_intensities
            .get(*member_index)?;
        let integrated_sell_intensity = *optimizer_context
            .integrated_sell_intensities
            .get(*member_index)?;

        let block_log_density = log_density_net_trading_time_return(
            driver_parameters,
            configuration,
            observed_return,
            business_time_scale,
            sell_share_proxy,
            integrated_buy_intensity,
            integrated_sell_intensity,
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
    integrated_buy_intensity: f64,
    integrated_sell_intensity: f64,
    integration_step_count: usize,
) -> Option<f64> {
    if !observed_return.is_finite()
        || !business_time_scale.is_finite()
        || business_time_scale <= 0.0
        || !integrated_buy_intensity.is_finite()
        || !integrated_sell_intensity.is_finite()
        || integrated_buy_intensity < 0.0
        || integrated_sell_intensity < 0.0
        || integration_step_count == 0
    {
        return None;
    }

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

fn deterministic_member_indices(total_count: usize, target_count: usize) -> Vec<usize> {
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

fn search_bounds_from_regimes(search_regimes: &[NigSearchRegime]) -> NigShapeSearchBounds {
    let mut lower_bounds = [f64::INFINITY; 4];
    let mut upper_bounds = [f64::NEG_INFINITY; 4];

    for search_regime in search_regimes {
        let regime_lower_bounds = [
            search_regime.log_alpha_center - search_regime.log_alpha_half_span,
            search_regime.buy_skew_center - search_regime.buy_skew_half_span,
            search_regime.sell_relative_center - search_regime.sell_relative_half_span,
            search_regime.log_delta_center - search_regime.log_delta_half_span,
        ];
        let regime_upper_bounds = [
            search_regime.log_alpha_center + search_regime.log_alpha_half_span,
            search_regime.buy_skew_center + search_regime.buy_skew_half_span,
            search_regime.sell_relative_center + search_regime.sell_relative_half_span,
            search_regime.log_delta_center + search_regime.log_delta_half_span,
        ];

        for coordinate_index in 0..4 {
            lower_bounds[coordinate_index] =
                lower_bounds[coordinate_index].min(regime_lower_bounds[coordinate_index]);
            upper_bounds[coordinate_index] =
                upper_bounds[coordinate_index].max(regime_upper_bounds[coordinate_index]);
        }
    }

    NigShapeSearchBounds {
        lower_bounds,
        upper_bounds,
    }
}

fn nearest_search_regime_index(
    search_point: NigShapeSearchPoint,
    search_regimes: &[NigSearchRegime],
) -> usize {
    let search_point_vector = search_point_to_vector(search_point);
    let mut nearest_regime_index = 0usize;
    let mut nearest_distance = f64::INFINITY;

    for (regime_index, search_regime) in search_regimes.iter().enumerate() {
        let regime_center = [
            search_regime.log_alpha_center,
            search_regime.buy_skew_center,
            search_regime.sell_relative_center,
            search_regime.log_delta_center,
        ];

        let distance = search_point_vector
            .iter()
            .zip(regime_center.iter())
            .map(|(coordinate, center)| (coordinate - center).powi(2))
            .sum::<f64>();

        if distance < nearest_distance {
            nearest_distance = distance;
            nearest_regime_index = regime_index;
        }
    }

    nearest_regime_index
}

fn optimize_seed_candidates(
    optimizer_context: &ContinuousReturnOptimizerContext,
    seed_candidates: &[SeededNigCandidate],
    search_bounds: NigShapeSearchBounds,
    solver_settings: &CalibrationOptimizerSettings,
) -> Vec<OptimizedNigSearchCandidate> {
    seed_candidates
        .par_iter()
        .filter_map(|seed_candidate| {
            let local_outcome = optimize_powell_search_point(
                optimizer_context,
                seed_candidate.scored_candidate.search_point,
                search_bounds,
                solver_settings,
            )?;

            let final_evaluation = evaluate_cached_search_point(
                optimizer_context,
                local_outcome.best_point,
                ContinuousLikelihoodFidelityTier::Full,
            )?;

            Some(OptimizedNigSearchCandidate {
                configuration: final_evaluation.configuration,
                family: seed_candidate.family,
                seed_search_point: seed_candidate.scored_candidate.search_point,
                final_search_point: local_outcome.best_point,
                optimizer_diagnostic: ContinuousReturnOptimizerDiagnostic {
                    seed_objective_score: seed_candidate.scored_candidate.objective_score,
                    local_objective_score: local_outcome.local_objective_score,
                    final_objective_score: final_evaluation.objective.shifted_objective_score,
                    local_evaluation_count: local_outcome.local_evaluation_count,
                    stop_reason: local_outcome.stop_reason,
                    basin_diagnostics: Vec::new(),
                },
            })
        })
        .collect()
}

fn retain_distinct_basin_candidates(
    mut optimized_candidates: Vec<OptimizedNigSearchCandidate>,
    search_bounds: NigShapeSearchBounds,
    maximum_basin_count: usize,
) -> Vec<OptimizedNigSearchCandidate> {
    optimized_candidates.sort_by(|left, right| {
        left.optimizer_diagnostic
            .final_objective_score
            .total_cmp(&right.optimizer_diagnostic.final_objective_score)
            .then_with(|| {
                left.final_search_point
                    .log_alpha
                    .total_cmp(&right.final_search_point.log_alpha)
            })
            .then_with(|| {
                left.final_search_point
                    .buy_skew_coordinate
                    .total_cmp(&right.final_search_point.buy_skew_coordinate)
            })
            .then_with(|| {
                left.final_search_point
                    .sell_relative_coordinate
                    .total_cmp(&right.final_search_point.sell_relative_coordinate)
            })
            .then_with(|| {
                left.final_search_point
                    .log_delta_offset
                    .total_cmp(&right.final_search_point.log_delta_offset)
            })
    });

    let mut retained_candidates: Vec<OptimizedNigSearchCandidate> = Vec::new();
    for optimized_candidate in optimized_candidates {
        if retained_candidates.iter().any(|retained_candidate| {
            basin_search_points_overlap(
                optimized_candidate.final_search_point,
                retained_candidate.final_search_point,
                search_bounds,
            )
        }) {
            continue;
        }
        retained_candidates.push(optimized_candidate);
        if retained_candidates.len() >= maximum_basin_count {
            break;
        }
    }

    retained_candidates
}

fn build_optimizer_basin_diagnostics(
    optimized_candidates: &[OptimizedNigSearchCandidate],
    calibration_sample: &ReturnComponentsStageContinuousReturnCalibrationSample,
    stage_seed: u64,
) -> Vec<ReturnComponentsStageContinuousOptimizerBasinDiagnostic> {
    optimized_candidates
        .iter()
        .map(|candidate| {
            let fitted_returns = simulate_matched_scale_returns(
                candidate.configuration,
                calibration_sample,
                stage_seed,
            );
            let fitted_profile = build_distribution_profile(fitted_returns.as_slice());
            let profile_fit_breakdown = distribution_profile_fit_breakdown(
                fitted_profile,
                calibration_sample.observed_profile,
            );

            ReturnComponentsStageContinuousOptimizerBasinDiagnostic {
                family_label: candidate.family.label().to_string(),
                seed_objective: candidate.optimizer_diagnostic.seed_objective_score,
                local_objective: candidate.optimizer_diagnostic.local_objective_score,
                final_objective: candidate.optimizer_diagnostic.final_objective_score,
                local_evaluation_count: candidate.optimizer_diagnostic.local_evaluation_count,
                stop_reason: candidate.optimizer_diagnostic.stop_reason.to_string(),
                seed_log_alpha: candidate.seed_search_point.log_alpha,
                seed_buy_skew_coordinate: candidate.seed_search_point.buy_skew_coordinate,
                seed_sell_relative_coordinate: candidate.seed_search_point.sell_relative_coordinate,
                seed_log_delta_offset: candidate.seed_search_point.log_delta_offset,
                final_log_alpha: candidate.final_search_point.log_alpha,
                final_buy_skew_coordinate: candidate.final_search_point.buy_skew_coordinate,
                final_sell_relative_coordinate: candidate
                    .final_search_point
                    .sell_relative_coordinate,
                final_log_delta_offset: candidate.final_search_point.log_delta_offset,
                final_alpha: candidate.configuration.alpha,
                final_beta_0: candidate.configuration.beta_0,
                final_delta: candidate.configuration.delta,
                final_delta_beta: candidate.configuration.delta_beta,
                profile_fit_score: profile_fit_breakdown.global_fit_score,
                fitted_return_volatility: sample_standard_deviation(fitted_returns.as_slice()),
                fitted_skewness: sample_adjusted_skewness(fitted_returns.as_slice()).unwrap_or(0.0),
            }
        })
        .collect()
}

fn basin_search_points_overlap(
    left: NigShapeSearchPoint,
    right: NigShapeSearchPoint,
    search_bounds: NigShapeSearchBounds,
) -> bool {
    let left_vector = search_point_to_vector(left);
    let right_vector = search_point_to_vector(right);

    let mut maximum_normalized_distance: f64 = 0.0;

    for coordinate_index in 0..4 {
        let coordinate_range = (search_bounds.upper_bounds[coordinate_index]
            - search_bounds.lower_bounds[coordinate_index])
            .abs()
            .max(1.0);

        let normalized_distance = (left_vector[coordinate_index] - right_vector[coordinate_index])
            .abs()
            / coordinate_range;
        maximum_normalized_distance = maximum_normalized_distance.max(normalized_distance);
    }

    maximum_normalized_distance <= 0.05
}

fn optimize_powell_search_point(
    optimizer_context: &ContinuousReturnOptimizerContext,
    seed_search_point: NigShapeSearchPoint,
    search_bounds: NigShapeSearchBounds,
    solver_settings: &CalibrationOptimizerSettings,
) -> Option<PowellOptimizationOutcome> {
    let mut current_vector =
        clamp_search_point_vector(search_point_to_vector(seed_search_point), search_bounds);

    let mut current_evaluation = evaluate_cached_search_point(
        optimizer_context,
        vector_to_search_point(current_vector),
        ContinuousLikelihoodFidelityTier::Local,
    )?;

    let mut best_vector = current_vector;
    let mut best_evaluation = current_evaluation;
    let mut local_evaluation_count = 1usize;

    let local_evaluation_budget = solver_settings.max_iterations.clamp(
        RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_MIN_BROAD_CANDIDATE_COUNT,
        RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_MAX_BROAD_CANDIDATE_COUNT,
    );

    let mut directions = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];

    for _ in 0..RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_MAX_POWELL_CYCLES {
        let cycle_start_vector = current_vector;
        let cycle_start_objective = current_evaluation.objective.shifted_objective_score;

        let mut largest_direction_improvement = 0.0;
        let mut replaced_direction_index = 0usize;

        for (direction_index, direction) in directions.iter().enumerate() {
            let line_search_outcome = minimize_along_direction(
                optimizer_context,
                current_vector,
                *direction,
                search_bounds,
                solver_settings,
            );

            local_evaluation_count += line_search_outcome.evaluation_count;
            if local_evaluation_count >= local_evaluation_budget {
                return Some(PowellOptimizationOutcome {
                    best_point: vector_to_search_point(best_vector),
                    local_objective_score: best_evaluation.objective.shifted_objective_score,
                    local_evaluation_count,
                    stop_reason: "budget",
                });
            }

            if let Some(best_line_evaluation) = line_search_outcome.best_evaluation {
                let direction_improvement = current_evaluation.objective.shifted_objective_score
                    - best_line_evaluation.objective.shifted_objective_score;
                if direction_improvement > largest_direction_improvement {
                    largest_direction_improvement = direction_improvement;
                    replaced_direction_index = direction_index;
                }

                current_vector = line_search_outcome.best_vector;

                current_evaluation = best_line_evaluation;
                if current_evaluation.objective.shifted_objective_score
                    < best_evaluation.objective.shifted_objective_score
                {
                    best_vector = current_vector;
                    best_evaluation = current_evaluation;
                }
            }
        }

        let cycle_displacement = vector4_subtract(current_vector, cycle_start_vector);
        let maximum_coordinate_displacement = cycle_displacement
            .iter()
            .map(|coordinate| coordinate.abs())
            .fold(0.0, f64::max);
        let cycle_improvement =
            cycle_start_objective - current_evaluation.objective.shifted_objective_score;
        let objective_tolerance = solver_settings
            .absolute_tolerance
            .max(solver_settings.relative_tolerance * cycle_start_objective.abs().max(1.0));
        if cycle_improvement <= objective_tolerance
            || maximum_coordinate_displacement <= solver_settings.absolute_tolerance
        {
            return Some(PowellOptimizationOutcome {
                best_point: vector_to_search_point(best_vector),
                local_objective_score: best_evaluation.objective.shifted_objective_score,
                local_evaluation_count,
                stop_reason: "tolerance",
            });
        }

        if maximum_coordinate_displacement
            <= RETURN_COMPONENTS_STAGE_MIN_REPRESENTABLE_POSITIVE_PARAMETER
        {
            return Some(PowellOptimizationOutcome {
                best_point: vector_to_search_point(best_vector),
                local_objective_score: best_evaluation.objective.shifted_objective_score,
                local_evaluation_count,
                stop_reason: "invalid_step",
            });
        }

        let composite_direction = cycle_displacement;
        let composite_line_search_outcome = minimize_along_direction(
            optimizer_context,
            current_vector,
            composite_direction,
            search_bounds,
            solver_settings,
        );

        local_evaluation_count += composite_line_search_outcome.evaluation_count;

        if local_evaluation_count >= local_evaluation_budget {
            return Some(PowellOptimizationOutcome {
                best_point: vector_to_search_point(best_vector),
                local_objective_score: best_evaluation.objective.shifted_objective_score,
                local_evaluation_count,
                stop_reason: "budget",
            });
        }

        if let Some(composite_evaluation) = composite_line_search_outcome.best_evaluation {
            if composite_evaluation.objective.shifted_objective_score
                < current_evaluation.objective.shifted_objective_score
            {
                current_vector = composite_line_search_outcome.best_vector;
                current_evaluation = composite_evaluation;
                if current_evaluation.objective.shifted_objective_score
                    < best_evaluation.objective.shifted_objective_score
                {
                    best_vector = current_vector;
                    best_evaluation = current_evaluation;
                }
            }

            let composite_improvement =
                cycle_start_objective - composite_evaluation.objective.shifted_objective_score;
            if composite_improvement > largest_direction_improvement {
                directions[replaced_direction_index] = normalize_vector4(
                    composite_direction,
                    RETURN_COMPONENTS_STAGE_MIN_REPRESENTABLE_POSITIVE_PARAMETER,
                );
            }
        }
    }

    Some(PowellOptimizationOutcome {
        best_point: vector_to_search_point(best_vector),
        local_objective_score: best_evaluation.objective.shifted_objective_score,
        local_evaluation_count,
        stop_reason: "budget",
    })
}

fn minimize_along_direction(
    optimizer_context: &ContinuousReturnOptimizerContext,
    current_vector: [f64; 4],
    direction: [f64; 4],
    search_bounds: NigShapeSearchBounds,
    solver_settings: &CalibrationOptimizerSettings,
) -> LineSearchOutcome {
    let Some((lower_step, upper_step)) =
        bounded_step_interval(current_vector, direction, search_bounds)
    else {
        return LineSearchOutcome {
            best_vector: current_vector,
            best_evaluation: evaluate_cached_search_point(
                optimizer_context,
                vector_to_search_point(current_vector),
                ContinuousLikelihoodFidelityTier::Local,
            ),
            evaluation_count: 0,
        };
    };

    if (upper_step - lower_step).abs() <= solver_settings.absolute_tolerance {
        return LineSearchOutcome {
            best_vector: current_vector,
            best_evaluation: evaluate_cached_search_point(
                optimizer_context,
                vector_to_search_point(current_vector),
                ContinuousLikelihoodFidelityTier::Local,
            ),
            evaluation_count: 0,
        };
    }

    let mut best_vector = current_vector;
    let mut best_evaluation = evaluate_cached_search_point(
        optimizer_context,
        vector_to_search_point(current_vector),
        ContinuousLikelihoodFidelityTier::Local,
    );

    let mut evaluation_count = 0usize;

    let golden_ratio = (5.0_f64.sqrt() - 1.0) * 0.5;

    let mut left = lower_step;
    let mut right = upper_step;

    let mut left_inner = right - golden_ratio * (right - left);
    let mut right_inner = left + golden_ratio * (right - left);

    let mut left_inner_evaluation = evaluate_line_search_step(
        optimizer_context,
        current_vector,
        direction,
        left_inner,
        search_bounds,
    );
    let mut right_inner_evaluation = evaluate_line_search_step(
        optimizer_context,
        current_vector,
        direction,
        right_inner,
        search_bounds,
    );

    evaluation_count += 2;

    update_best_line_search_evaluation(
        left_inner_evaluation,
        &mut best_vector,
        &mut best_evaluation,
    );
    update_best_line_search_evaluation(
        right_inner_evaluation,
        &mut best_vector,
        &mut best_evaluation,
    );

    while evaluation_count < RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_LINE_SEARCH_EVALUATION_CAP {
        let left_inner_objective = left_inner_evaluation
            .map(|evaluation| evaluation.1.objective.shifted_objective_score)
            .unwrap_or(f64::INFINITY);
        let right_inner_objective = right_inner_evaluation
            .map(|evaluation| evaluation.1.objective.shifted_objective_score)
            .unwrap_or(f64::INFINITY);

        if left_inner_objective <= right_inner_objective {
            right = right_inner;
            right_inner = left_inner;
            right_inner_evaluation = left_inner_evaluation;

            left_inner = right - golden_ratio * (right - left);
            left_inner_evaluation = evaluate_line_search_step(
                optimizer_context,
                current_vector,
                direction,
                left_inner,
                search_bounds,
            );

            evaluation_count += 1;

            update_best_line_search_evaluation(
                left_inner_evaluation,
                &mut best_vector,
                &mut best_evaluation,
            );
        } else {
            left = left_inner;
            left_inner = right_inner;
            left_inner_evaluation = right_inner_evaluation;

            right_inner = left + golden_ratio * (right - left);
            right_inner_evaluation = evaluate_line_search_step(
                optimizer_context,
                current_vector,
                direction,
                right_inner,
                search_bounds,
            );

            evaluation_count += 1;

            update_best_line_search_evaluation(
                right_inner_evaluation,
                &mut best_vector,
                &mut best_evaluation,
            );
        }

        if (right - left).abs() <= solver_settings.absolute_tolerance {
            break;
        }
    }

    LineSearchOutcome {
        best_vector,
        best_evaluation,
        evaluation_count,
    }
}

fn update_best_line_search_evaluation(
    evaluated_step: Option<([f64; 4], CachedSearchPointObjectiveEvaluation)>,
    best_vector: &mut [f64; 4],
    best_evaluation: &mut Option<CachedSearchPointObjectiveEvaluation>,
) {
    let Some((candidate_vector, candidate_evaluation)) = evaluated_step else {
        return;
    };

    let should_replace = best_evaluation
        .map(|current_best| {
            candidate_evaluation.objective.shifted_objective_score
                < current_best.objective.shifted_objective_score
        })
        .unwrap_or(true);
    if should_replace {
        *best_vector = candidate_vector;
        *best_evaluation = Some(candidate_evaluation);
    }
}

fn evaluate_line_search_step(
    optimizer_context: &ContinuousReturnOptimizerContext,
    current_vector: [f64; 4],
    direction: [f64; 4],
    step: f64,
    search_bounds: NigShapeSearchBounds,
) -> Option<([f64; 4], CachedSearchPointObjectiveEvaluation)> {
    let candidate_vector = clamp_search_point_vector(
        vector4_add(current_vector, vector4_scale(direction, step)),
        search_bounds,
    );

    let candidate_evaluation = evaluate_cached_search_point(
        optimizer_context,
        vector_to_search_point(candidate_vector),
        ContinuousLikelihoodFidelityTier::Local,
    )?;

    Some((candidate_vector, candidate_evaluation))
}

fn bounded_step_interval(
    current_vector: [f64; 4],
    direction: [f64; 4],
    search_bounds: NigShapeSearchBounds,
) -> Option<(f64, f64)> {
    let mut lower_step = f64::NEG_INFINITY;
    let mut upper_step = f64::INFINITY;

    for coordinate_index in 0..4 {
        let direction_component = direction[coordinate_index];
        if direction_component.abs() <= RETURN_COMPONENTS_STAGE_MIN_REPRESENTABLE_POSITIVE_PARAMETER
        {
            continue;
        }

        let lower_candidate = (search_bounds.lower_bounds[coordinate_index]
            - current_vector[coordinate_index])
            / direction_component;
        let upper_candidate = (search_bounds.upper_bounds[coordinate_index]
            - current_vector[coordinate_index])
            / direction_component;

        lower_step = lower_step.max(lower_candidate.min(upper_candidate));
        upper_step = upper_step.min(lower_candidate.max(upper_candidate));
    }

    if !lower_step.is_finite() || !upper_step.is_finite() || lower_step > upper_step {
        return None;
    }

    Some((lower_step, upper_step))
}

fn clamp_search_point_vector(
    mut search_point_vector: [f64; 4],
    search_bounds: NigShapeSearchBounds,
) -> [f64; 4] {
    for coordinate_index in 0..4 {
        search_point_vector[coordinate_index] = search_point_vector[coordinate_index].clamp(
            search_bounds.lower_bounds[coordinate_index],
            search_bounds.upper_bounds[coordinate_index],
        );
    }

    search_point_vector
}

fn search_point_to_vector(search_point: NigShapeSearchPoint) -> [f64; 4] {
    [
        search_point.log_alpha,
        search_point.buy_skew_coordinate,
        search_point.sell_relative_coordinate,
        search_point.log_delta_offset,
    ]
}

fn vector_to_search_point(search_point_vector: [f64; 4]) -> NigShapeSearchPoint {
    NigShapeSearchPoint {
        log_alpha: search_point_vector[0],
        buy_skew_coordinate: search_point_vector[1],
        sell_relative_coordinate: search_point_vector[2],
        log_delta_offset: search_point_vector[3],
    }
}

fn with_continuous_fit_thread_pool<T: Send>(
    cpu_count: usize,
    body: impl FnOnce() -> T + Send,
) -> T {
    if cpu_count <= 1 {
        return body();
    }

    match rayon::ThreadPoolBuilder::new()
        .num_threads(cpu_count)
        .build()
    {
        Ok(thread_pool) => thread_pool.install(body),
        Err(_) => body(),
    }
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
        optimizer_seed_objective: 0.0,
        optimizer_local_objective: 0.0,
        optimizer_final_objective: 0.0,
        optimizer_local_evaluation_count: 0,
        optimizer_stop_reason: "not_optimized".to_string(),
        optimizer_basin_diagnostics: Vec::new(),
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
    let preferred_margin_ratio = preferred_alpha_beta_margin_ratio();

    if minimum_margin_ratio >= preferred_margin_ratio {
        0.0
    } else {
        (preferred_margin_ratio / minimum_margin_ratio).ln_1p()
    }
}

fn preferred_alpha_beta_margin_ratio() -> f64 {
    0.05
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
        sample_count: returns.iter().filter(|value| value.is_finite()).count(),
        mean_return: arithmetic_mean_signed(returns),
        return_volatility: sample_standard_deviation(returns),
        adjusted_skewness: sample_adjusted_skewness(returns).unwrap_or(0.0),
        adjusted_excess_kurtosis: sample_adjusted_excess_kurtosis(returns).unwrap_or(0.0),
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
    use std::collections::BTreeSet;

    use super::{
        ContinuousLikelihoodFidelityTier, RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_ELITE_COUNT,
        ReturnComponentsStageContinuousReturnCalibrationSample, average_negative_log_likelihood,
        build_broad_search_points, build_continuous_return_optimizer_context,
        build_distribution_profile, build_equal_business_time_blocks, build_moment_seed_candidates,
        build_nig_search_candidates, build_scale_bucket_profiles,
        build_search_regimes_from_moment_seeds, continuous_block_likelihood_objective,
        coordinate_to_ratio, coordinate_to_share, log_density_net_trading_time_return,
        ratio_to_coordinate, relative_share_from_beta_ratios, sell_beta_ratio_from_relative_share,
        theoretical_search_bounds,
    };
    use crate::stages::return_components::contracts::ReturnComponentsObservedOpenSessionReturn;
    use janiform_calibration_artifacts::CalibrationOptimizerSettings;
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
        let recovered_share = relative_share_from_beta_ratios(buy_ratio, sell_ratio)
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
            0.54,
            0.66,
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
        let optimizer_context = build_continuous_return_optimizer_context(&calibration_sample);

        let true_objective = average_negative_log_likelihood(
            true_configuration,
            &optimizer_context,
            &(0..observed_returns.len()).collect::<Vec<_>>(),
            128,
        )
        .expect("true likelihood should evaluate");
        let challenger_objective = average_negative_log_likelihood(
            challenger_configuration,
            &optimizer_context,
            &(0..observed_returns.len()).collect::<Vec<_>>(),
            128,
        )
        .expect("challenger likelihood should evaluate");

        assert!(
            true_objective < challenger_objective,
            "likelihood objective should prefer the data-generating configuration"
        );
    }

    #[test]
    fn broad_search_is_deterministic_across_parallel_cpu_counts() {
        let calibration_sample = test_optimizer_calibration_sample();
        let single_thread_solver_settings =
            CalibrationOptimizerSettings::try_new("adaptive_search", 64, 1, 1.0e-8, 1.0e-6, 1.0)
                .expect("solver settings should validate");
        let multi_thread_solver_settings =
            CalibrationOptimizerSettings::try_new("adaptive_search", 64, 4, 1.0e-8, 1.0e-6, 1.0)
                .expect("solver settings should validate");

        let single_thread_candidates =
            build_nig_search_candidates(&calibration_sample, 17, &single_thread_solver_settings);
        let multi_thread_candidates =
            build_nig_search_candidates(&calibration_sample, 17, &multi_thread_solver_settings);

        assert_eq!(
            single_thread_candidates[0].configuration,
            multi_thread_candidates[0].configuration
        );
        assert_eq!(
            single_thread_candidates[0]
                .optimizer_diagnostic
                .final_objective_score
                .to_bits(),
            multi_thread_candidates[0]
                .optimizer_diagnostic
                .final_objective_score
                .to_bits()
        );
    }

    #[test]
    fn optimized_candidate_is_no_worse_than_best_broad_seed() {
        let calibration_sample = test_optimizer_calibration_sample();
        let optimizer_context = build_continuous_return_optimizer_context(&calibration_sample);
        let solver_settings =
            CalibrationOptimizerSettings::try_new("adaptive_search", 64, 1, 1.0e-8, 1.0e-6, 1.0)
                .expect("solver settings should validate");
        let theoretical_bounds = theoretical_search_bounds(calibration_sample.observed_profile, 96);
        let moment_seed_candidates = build_moment_seed_candidates(
            &optimizer_context,
            &calibration_sample,
            theoretical_bounds,
            96,
        );
        let broad_search_regimes = build_search_regimes_from_moment_seeds(
            &optimizer_context,
            moment_seed_candidates.as_slice(),
            theoretical_bounds,
        );

        let broad_search_points = build_broad_search_points(broad_search_regimes.as_slice(), 64);
        let broad_seed_candidates = super::score_search_points(
            &optimizer_context,
            broad_search_points.as_slice(),
            ContinuousLikelihoodFidelityTier::Seed,
        );
        let best_broad_seed = broad_seed_candidates
            .iter()
            .map(|candidate| {
                continuous_block_likelihood_objective(
                    candidate.configuration,
                    &optimizer_context,
                    ContinuousLikelihoodFidelityTier::Full,
                )
                .shifted_objective_score
            })
            .fold(f64::INFINITY, f64::min);
        let optimized_candidates =
            build_nig_search_candidates(&calibration_sample, 17, &solver_settings);

        assert!(
            optimized_candidates[0]
                .optimizer_diagnostic
                .final_objective_score
                <= best_broad_seed + 1.0e-10
        );
    }

    #[test]
    fn optimized_candidates_surface_distinct_basin_diagnostics() {
        let calibration_sample = test_optimizer_calibration_sample();
        let solver_settings =
            CalibrationOptimizerSettings::try_new("adaptive_search", 64, 4, 1.0e-8, 1.0e-6, 1.0)
                .expect("solver settings should validate");

        let optimized_candidates =
            build_nig_search_candidates(&calibration_sample, 17, &solver_settings);
        let basin_diagnostics = &optimized_candidates[0]
            .optimizer_diagnostic
            .basin_diagnostics;

        assert!(!basin_diagnostics.is_empty());
        assert!(basin_diagnostics.len() <= RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_ELITE_COUNT);

        let family_labels: BTreeSet<String> = basin_diagnostics
            .iter()
            .map(|diagnostic| diagnostic.family_label.clone())
            .collect();
        assert_eq!(family_labels.len(), basin_diagnostics.len());
        assert!(
            basin_diagnostics
                .iter()
                .all(|diagnostic| diagnostic.final_objective.is_finite())
        );
    }

    #[test]
    fn optimizer_stop_reason_can_report_tolerance_and_budget() {
        let calibration_sample = test_optimizer_calibration_sample();
        let tolerance_solver_settings =
            CalibrationOptimizerSettings::try_new("adaptive_search", 64, 1, 1.0, 1.0, 1.0)
                .expect("solver settings should validate");
        let budget_solver_settings =
            CalibrationOptimizerSettings::try_new("adaptive_search", 64, 1, 1.0e-12, 1.0e-12, 1.0)
                .expect("solver settings should validate");

        let tolerance_candidates =
            build_nig_search_candidates(&calibration_sample, 17, &tolerance_solver_settings);
        let budget_candidates =
            build_nig_search_candidates(&calibration_sample, 17, &budget_solver_settings);

        assert_eq!(
            tolerance_candidates[0].optimizer_diagnostic.stop_reason,
            "tolerance"
        );
        assert_eq!(
            budget_candidates[0].optimizer_diagnostic.stop_reason,
            "budget"
        );
    }

    #[test]
    fn repeated_builds_keep_objective_cache_observable_results_stable() {
        let calibration_sample = test_optimizer_calibration_sample();
        let solver_settings =
            CalibrationOptimizerSettings::try_new("adaptive_search", 64, 2, 1.0e-8, 1.0e-6, 1.0)
                .expect("solver settings should validate");

        let first_candidates =
            build_nig_search_candidates(&calibration_sample, 17, &solver_settings);
        let second_candidates =
            build_nig_search_candidates(&calibration_sample, 17, &solver_settings);

        assert_eq!(
            first_candidates[0].configuration,
            second_candidates[0].configuration
        );
        assert_eq!(
            first_candidates[0]
                .optimizer_diagnostic
                .local_evaluation_count,
            second_candidates[0]
                .optimizer_diagnostic
                .local_evaluation_count
        );
    }

    fn test_optimizer_calibration_sample() -> ReturnComponentsStageContinuousReturnCalibrationSample
    {
        let configuration =
            NigSharedPlusShiftConfiguration::try_new(120.0, -24.0, 2.5e-4, 0.0, -10.0)
                .expect("configuration should validate");
        let driver_parameters = configuration
            .derive_driver_parameters()
            .expect("driver parameters should derive");
        let mut randomness_stream = SplitMix64SimulationRandomnessStream::from_seed(91);
        let business_time_scales: Vec<f64> = (0..320)
            .map(|sample_index| 0.8 + sample_index as f64 / 80.0)
            .collect();
        let sell_share_proxies: Vec<f64> = (0..320)
            .map(|sample_index| 0.25 + 0.50 * ((sample_index % 13) as f64 / 12.0))
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

        ReturnComponentsStageContinuousReturnCalibrationSample {
            observed_returns: observed_returns.clone(),
            business_time_scales: business_time_scales.clone(),
            sell_share_proxies: sell_share_proxies.clone(),
            observed_profile: build_distribution_profile(observed_returns.as_slice()),
            scale_bucket_profiles: build_scale_bucket_profiles(
                observed_returns.as_slice(),
                business_time_scales.as_slice(),
            ),
            open_session_sell_share_proxy: super::arithmetic_mean(sell_share_proxies.as_slice()),
        }
    }
}
