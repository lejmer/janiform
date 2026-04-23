use std::collections::BTreeSet;

use janiform_calibration_artifacts::CalibrationOptimizerSettings;
use janiform_foundations::low_discrepancy::van_der_corput;
use janiform_foundations::numeric::{
    sample_adjusted_excess_kurtosis, sample_adjusted_skewness, sample_standard_deviation,
};
use janiform_foundations::vector_math::vector4_subtract;
use rayon::iter::{IntoParallelIterator, IntoParallelRefIterator, ParallelIterator};

use super::super::contracts::{
    ReturnComponentsStageObservedSignalSummaryStatistics,
    ReturnComponentsStageReturnDistributionDiagnostic,
};
use super::super::derived_contracts::return_components_stage_positive_domain_floor;
use super::super::execution::arithmetic_mean_signed;
use super::bounds::{
    build_moment_seed_candidates, build_search_regimes_from_moment_seeds,
    configuration_from_shape_search_point, theoretical_search_bounds,
};
use super::constants::{
    RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_ELITE_COUNT,
    RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_MAX_BROAD_CANDIDATE_COUNT,
    RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_MIN_BROAD_CANDIDATE_COUNT,
    RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_MIN_MOMENT_SEED_CANDIDATE_COUNT,
    RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_MOMENT_SEED_CANDIDATE_MULTIPLIER,
    RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_PROFILE_BISECTION_STEP_COUNT,
    RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_PROFILE_LIKELIHOOD_CHI_SQUARE_95,
    RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_PROFILE_SCAN_STEP_COUNT,
};
use super::likelihood::{
    continuous_block_likelihood_objective, profile_likelihood_score, simulate_matched_scale_returns,
};
use super::profile::{build_distribution_profile, distribution_profile_fit_breakdown};
use super::sample::build_continuous_return_optimizer_context;
use super::types::{
    BasinSeedFamily, CachedSearchPointObjectiveEvaluation, ContinuousLikelihoodFidelityTier,
    ContinuousReturnCandidateEvaluation, ContinuousReturnOptimizerContext,
    ContinuousReturnOptimizerDiagnostic, LocalSearchOptimizationOutcome, NigSearchRegime,
    NigShapeSearchBounds, NigShapeSearchPoint, OptimizedNigCandidate, OptimizedNigSearchCandidate,
    ReturnComponentsStageContinuousReturnCalibrationSample, ScanDirection, ScoredNigCandidate,
    SeededNigCandidate,
};

pub(crate) fn build_nig_search_candidates(
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

pub(crate) fn build_nig_moment_seed_configurations(
    calibration_sample: &ReturnComponentsStageContinuousReturnCalibrationSample,
    solver_settings: &CalibrationOptimizerSettings,
) -> Vec<janiform_model_kernels::nig::NigSharedPlusShiftConfiguration> {
    let optimizer_context = build_continuous_return_optimizer_context(calibration_sample);
    let broad_candidate_count = solver_settings.max_iterations.clamp(
        RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_MIN_BROAD_CANDIDATE_COUNT,
        RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_MAX_BROAD_CANDIDATE_COUNT,
    );
    let moment_candidate_count = broad_candidate_count
        .saturating_mul(RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_MOMENT_SEED_CANDIDATE_MULTIPLIER)
        .max(RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_MIN_MOMENT_SEED_CANDIDATE_COUNT);
    let search_bounds =
        theoretical_search_bounds(calibration_sample.observed_profile, moment_candidate_count);

    build_moment_seed_candidates(
        &optimizer_context,
        calibration_sample,
        search_bounds,
        moment_candidate_count,
    )
    .into_iter()
    .map(|candidate| candidate.configuration)
    .collect()
}

pub(crate) fn evaluate_continuous_nig_candidate(
    configuration: janiform_model_kernels::nig::NigSharedPlusShiftConfiguration,
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
        fit_breakdown: super::super::contracts::ReturnComponentsStageContinuousDistributionFitBreakdownDiagnostic {
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

pub(super) fn build_profiled_search_regime(
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

pub(super) fn score_search_points(
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

pub(super) fn deduplicate_candidates_by_key<T>(
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

pub(super) fn candidate_configuration_key(
    configuration: janiform_model_kernels::nig::NigSharedPlusShiftConfiguration,
) -> String {
    format!(
        "{:.6}|{:.6}|{:.12}|{:.6}|{:.6}",
        configuration.alpha,
        configuration.beta_0,
        configuration.delta,
        configuration.mu,
        configuration.delta_beta,
    )
}

pub(super) fn build_broad_search_points(
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
            ScanDirection::Lower | ScanDirection::Upper => {
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
            low_alpha: janiform_foundations::numeric::compute_sorted_quantile_or_zero(
                log_alpha_values.as_slice(),
                1.0 / 3.0,
            ),
            high_alpha: janiform_foundations::numeric::compute_sorted_quantile_or_zero(
                log_alpha_values.as_slice(),
                2.0 / 3.0,
            ),
            low_delta: janiform_foundations::numeric::compute_sorted_quantile_or_zero(
                log_delta_values.as_slice(),
                1.0 / 3.0,
            ),
            high_delta: janiform_foundations::numeric::compute_sorted_quantile_or_zero(
                log_delta_values.as_slice(),
                2.0 / 3.0,
            ),
            low_skew_magnitude: janiform_foundations::numeric::compute_sorted_quantile_or_zero(
                skew_magnitudes.as_slice(),
                1.0 / 3.0,
            ),
            high_skew_magnitude: janiform_foundations::numeric::compute_sorted_quantile_or_zero(
                skew_magnitudes.as_slice(),
                2.0 / 3.0,
            ),
            low_skew_asymmetry: janiform_foundations::numeric::compute_sorted_quantile_or_zero(
                skew_asymmetries.as_slice(),
                1.0 / 3.0,
            ),
            high_skew_asymmetry: janiform_foundations::numeric::compute_sorted_quantile_or_zero(
                skew_asymmetries.as_slice(),
                2.0 / 3.0,
            ),
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
    let buy_beta_ratio = super::bounds::coordinate_to_ratio(search_point.buy_skew_coordinate);
    let relative_share = super::bounds::coordinate_to_share(search_point.sell_relative_coordinate);
    let sell_beta_ratio =
        super::bounds::sell_beta_ratio_from_relative_share(buy_beta_ratio, relative_share);

    Some((
        (buy_beta_ratio - sell_beta_ratio).abs(),
        0.5 * (buy_beta_ratio.abs() + sell_beta_ratio.abs()),
    ))
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
    let cache_key = super::types::ContinuousLikelihoodObjectiveCacheKey {
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
            let seed_final_evaluation = evaluate_cached_search_point(
                optimizer_context,
                seed_candidate.scored_candidate.search_point,
                ContinuousLikelihoodFidelityTier::Full,
            )?;

            let local_outcome = optimize_local_search_point(
                optimizer_context,
                seed_candidate.scored_candidate.search_point,
                search_bounds,
                solver_settings,
            )?;

            let local_final_evaluation = evaluate_cached_search_point(
                optimizer_context,
                local_outcome.best_point,
                ContinuousLikelihoodFidelityTier::Full,
            )?;

            let (final_search_point, final_evaluation) =
                if local_final_evaluation.objective.shifted_objective_score
                    < seed_final_evaluation.objective.shifted_objective_score
                {
                    (local_outcome.best_point, local_final_evaluation)
                } else {
                    (
                        seed_candidate.scored_candidate.search_point,
                        seed_final_evaluation,
                    )
                };

            Some(OptimizedNigSearchCandidate {
                configuration: final_evaluation.configuration,
                family: seed_candidate.family,
                seed_search_point: seed_candidate.scored_candidate.search_point,
                final_search_point,
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
) -> Vec<super::super::contracts::ReturnComponentsStageContinuousOptimizerBasinDiagnostic> {
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

            super::super::contracts::ReturnComponentsStageContinuousOptimizerBasinDiagnostic {
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

fn optimize_local_search_point(
    optimizer_context: &ContinuousReturnOptimizerContext,
    seed_search_point: NigShapeSearchPoint,
    search_bounds: NigShapeSearchBounds,
    solver_settings: &CalibrationOptimizerSettings,
) -> Option<LocalSearchOptimizationOutcome> {
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

    let coordinate_spans: [f64; 4] = std::array::from_fn(|coordinate_index| {
        (search_bounds.upper_bounds[coordinate_index]
            - search_bounds.lower_bounds[coordinate_index])
            .abs()
            .max(solver_settings.absolute_tolerance)
    });

    let step_floor = solver_settings
        .absolute_tolerance
        .max(return_components_stage_positive_domain_floor());

    let mut step_radii: [f64; 4] = coordinate_spans.map(|span| (span * 0.18).max(step_floor));

    let search_directions: [[f64; 4]; 10] = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0, -1.0],
        [0.0, 1.0, 1.0, 0.0],
        [0.0, 1.0, -1.0, 0.0],
        [1.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 1.0],
    ];

    let mut shrink_only_cycle_count = 0usize;
    while local_evaluation_count < local_evaluation_budget {
        let candidate_vectors: Vec<[f64; 4]> = search_directions
            .iter()
            .flat_map(|direction| [-1.0, 1.0].into_iter().map(move |sign| (*direction, sign)))
            .filter_map(|(direction, sign)| {
                let mut displaced_vector = current_vector;
                let mut any_displacement = false;

                for coordinate_index in 0..4 {
                    let direction_component = direction[coordinate_index];
                    if direction_component.abs() <= return_components_stage_positive_domain_floor()
                    {
                        continue;
                    }

                    displaced_vector[coordinate_index] +=
                        sign * direction_component * step_radii[coordinate_index];

                    any_displacement = true;
                }

                any_displacement
                    .then_some(clamp_search_point_vector(displaced_vector, search_bounds))
            })
            .filter(|candidate_vector| {
                !candidate_vector.iter().zip(current_vector.iter()).all(
                    |(candidate_value, current_value)| {
                        (candidate_value - current_value).abs() <= step_floor
                    },
                )
            })
            .collect();

        if candidate_vectors.is_empty() {
            return Some(LocalSearchOptimizationOutcome {
                best_point: vector_to_search_point(best_vector),
                local_objective_score: best_evaluation.objective.shifted_objective_score,
                local_evaluation_count,
                stop_reason: "tolerance",
            });
        }

        let candidate_evaluations: Vec<([f64; 4], CachedSearchPointObjectiveEvaluation)> =
            candidate_vectors
                .into_par_iter()
                .filter_map(|candidate_vector| {
                    evaluate_cached_search_point(
                        optimizer_context,
                        vector_to_search_point(candidate_vector),
                        ContinuousLikelihoodFidelityTier::Local,
                    )
                    .map(|evaluation| (candidate_vector, evaluation))
                })
                .collect();

        local_evaluation_count += candidate_evaluations.len();

        let best_neighbor = candidate_evaluations.into_iter().min_by(|left, right| {
            left.1
                .objective
                .shifted_objective_score
                .total_cmp(&right.1.objective.shifted_objective_score)
        });

        let objective_tolerance = solver_settings.absolute_tolerance.max(
            solver_settings.relative_tolerance
                * current_evaluation
                    .objective
                    .shifted_objective_score
                    .abs()
                    .max(1.0),
        );

        if let Some((neighbor_vector, neighbor_evaluation)) = best_neighbor {
            let improvement = current_evaluation.objective.shifted_objective_score
                - neighbor_evaluation.objective.shifted_objective_score;
            if improvement > objective_tolerance {
                let displacement = vector4_subtract(neighbor_vector, current_vector);

                current_vector = neighbor_vector;
                current_evaluation = neighbor_evaluation;

                if current_evaluation.objective.shifted_objective_score
                    < best_evaluation.objective.shifted_objective_score
                {
                    best_vector = current_vector;
                    best_evaluation = current_evaluation;
                }

                for coordinate_index in 0..4 {
                    if displacement[coordinate_index].abs() > step_floor {
                        step_radii[coordinate_index] = (step_radii[coordinate_index] * 1.15)
                            .min(coordinate_spans[coordinate_index] * 0.5);
                    } else {
                        step_radii[coordinate_index] =
                            (step_radii[coordinate_index] * 0.92).max(step_floor);
                    }
                }

                shrink_only_cycle_count = 0;

                continue;
            }
        }

        for coordinate_index in 0..4 {
            step_radii[coordinate_index] = (step_radii[coordinate_index] * 0.5).max(step_floor);
        }

        shrink_only_cycle_count += 1;

        let maximum_radius = step_radii.iter().copied().fold(0.0, f64::max);
        if maximum_radius <= step_floor || shrink_only_cycle_count >= 3 {
            return Some(LocalSearchOptimizationOutcome {
                best_point: vector_to_search_point(best_vector),
                local_objective_score: best_evaluation.objective.shifted_objective_score,
                local_evaluation_count,
                stop_reason: "tolerance",
            });
        }
    }

    Some(LocalSearchOptimizationOutcome {
        best_point: vector_to_search_point(best_vector),
        local_objective_score: best_evaluation.objective.shifted_objective_score,
        local_evaluation_count,
        stop_reason: "budget",
    })
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
