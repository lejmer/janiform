use std::collections::BTreeSet;
use std::sync::mpsc::Sender;

use janiform_calibration_artifacts::CalibrationOptimizerSettings;
use janiform_foundations::numeric::{
    sample_adjusted_excess_kurtosis, sample_adjusted_skewness, sample_standard_deviation,
};
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};

use super::super::contracts::{
    ReturnComponentsStageObservedSignalSummaryStatistics,
    ReturnComponentsStageReturnDistributionDiagnostic,
};
use super::super::contracts::{
    ReturnComponentsStageProgressPhase, ReturnComponentsStageProgressUpdate,
};
use super::super::execution::arithmetic_mean_signed;
use super::super::progress::{
    send_return_components_evaluation_progress, send_return_components_progress,
};
use super::bounds::{
    build_moment_seed_candidates, build_search_regimes_from_moment_seeds,
    configuration_from_shape_search_point, theoretical_search_bounds,
};
use super::likelihood::{
    continuous_block_likelihood_objective, continuous_block_likelihood_objective_with_failures,
    simulate_matched_scale_returns,
};
use super::profile::{build_distribution_profile, distribution_profile_fit_breakdown};
use super::proxy_em::build_effective_nig_em_search_points;
use super::sample::build_continuous_return_optimizer_context;
use super::scoring::comparable_continuous_distribution_fit_score;
use super::types::{
    BasinSeedFamily, CachedSearchPointObjectiveEvaluation, ContinuousLikelihoodFidelityTier,
    ContinuousReturnCandidateEvaluation, ContinuousReturnOptimizerContext,
    ContinuousReturnOptimizerDiagnostic, NigSearchRegime, NigShapeSearchBounds,
    NigShapeSearchPoint, OptimizedNigCandidate, OptimizedNigSearchCandidate,
    ReturnComponentsStageContinuousReturnCalibrationSample, ScoredNigCandidate, SeededNigCandidate,
};

#[cfg(test)]
pub(crate) fn build_nig_search_candidates(
    calibration_sample: &ReturnComponentsStageContinuousReturnCalibrationSample,
    stage_seed: u64,
    solver_settings: &CalibrationOptimizerSettings,
) -> Vec<OptimizedNigCandidate> {
    build_nig_search_candidates_with_progress(calibration_sample, stage_seed, solver_settings, None)
}

pub(crate) fn build_nig_search_candidates_with_progress(
    calibration_sample: &ReturnComponentsStageContinuousReturnCalibrationSample,
    stage_seed: u64,
    solver_settings: &CalibrationOptimizerSettings,
    progress_sender: Option<&Sender<ReturnComponentsStageProgressUpdate>>,
) -> Vec<OptimizedNigCandidate> {
    let optimizer_context = build_continuous_return_optimizer_context(calibration_sample);

    with_continuous_fit_thread_pool(solver_settings.cpu_count, || {
        let candidate_budget = solver_settings
            .max_iterations
            .max(BasinSeedFamily::ALL.len());

        let moment_candidate_count =
            family_aligned_candidate_count(candidate_budget, BasinSeedFamily::ALL.len());

        let theoretical_search_bounds =
            theoretical_search_bounds(calibration_sample.observed_profile, moment_candidate_count);

        send_return_components_progress(
            progress_sender,
            ReturnComponentsStageProgressPhase::ContinuousNigMomentSeedGeneration,
            0,
            moment_candidate_count,
            Some("build moment seed candidates"),
        );

        let moment_seed_candidates = build_moment_seed_candidates(
            &optimizer_context,
            calibration_sample,
            theoretical_search_bounds,
            moment_candidate_count,
            progress_sender,
        );

        send_return_components_progress(
            progress_sender,
            ReturnComponentsStageProgressPhase::ContinuousNigMomentSeedGeneration,
            moment_candidate_count,
            moment_candidate_count,
            Some("moment seed candidates ready"),
        );

        send_return_components_progress(
            progress_sender,
            ReturnComponentsStageProgressPhase::ContinuousNigProxyEmSeedGeneration,
            0,
            1,
            Some("build proxy EM search points"),
        );

        let em_search_points = build_effective_nig_em_search_points(
            calibration_sample,
            moment_seed_candidates.as_slice(),
            solver_settings,
            progress_sender,
        );

        send_return_components_progress(
            progress_sender,
            ReturnComponentsStageProgressPhase::ContinuousNigProxyEmSeedGeneration,
            1,
            1,
            Some("proxy EM search points ready"),
        );

        send_return_components_progress(
            progress_sender,
            ReturnComponentsStageProgressPhase::ContinuousNigSeedScoring,
            0,
            em_search_points.len(),
            Some("score proxy EM search points"),
        );

        let em_scored_candidates = score_search_points(
            &optimizer_context,
            em_search_points.as_slice(),
            ContinuousLikelihoodFidelityTier::Seed,
            Some(ReturnComponentsStageProgressPhase::ContinuousNigSeedScoring),
            progress_sender,
        );

        let seed_frontier_candidates =
            merge_seed_frontier_candidates(moment_seed_candidates, em_scored_candidates);

        send_return_components_progress(
            progress_sender,
            ReturnComponentsStageProgressPhase::ContinuousNigSearchRegimeGeneration,
            0,
            1,
            Some("build search regimes"),
        );

        let broad_search_regimes = build_search_regimes_from_moment_seeds(
            &optimizer_context,
            seed_frontier_candidates.as_slice(),
            theoretical_search_bounds,
        );

        let search_bounds = search_bounds_from_regimes(broad_search_regimes.as_slice());

        send_return_components_progress(
            progress_sender,
            ReturnComponentsStageProgressPhase::ContinuousNigSearchRegimeGeneration,
            1,
            1,
            Some("search regimes ready"),
        );

        send_return_components_progress(
            progress_sender,
            ReturnComponentsStageProgressPhase::ContinuousNigBasinSeedGeneration,
            0,
            1,
            Some("build basin seed candidates"),
        );

        let seed_candidates = build_basin_seed_candidates(
            seed_frontier_candidates.as_slice(),
            broad_search_regimes.as_slice(),
        );

        send_return_components_progress(
            progress_sender,
            ReturnComponentsStageProgressPhase::ContinuousNigBasinSeedGeneration,
            1,
            1,
            Some("basin seed candidates ready"),
        );

        send_return_components_progress(
            progress_sender,
            ReturnComponentsStageProgressPhase::ContinuousNigBasinOptimization,
            0,
            seed_candidates.len(),
            Some("optimize basin seed candidates"),
        );

        let optimized_seed_candidates = optimize_seed_candidates(
            &optimizer_context,
            seed_candidates.as_slice(),
            progress_sender,
        );

        send_return_components_progress(
            progress_sender,
            ReturnComponentsStageProgressPhase::ContinuousNigBasinOptimization,
            seed_candidates.len(),
            seed_candidates.len(),
            Some("basin seed optimization complete"),
        );

        let retained_basin_candidates = retain_distinct_basin_candidates(
            optimized_seed_candidates,
            search_bounds,
            BasinSeedFamily::ALL.len(),
        );

        send_return_components_progress(
            progress_sender,
            ReturnComponentsStageProgressPhase::ContinuousNigBasinDiagnostics,
            0,
            1,
            Some("build basin diagnostics"),
        );

        let basin_diagnostics = build_optimizer_basin_diagnostics(
            retained_basin_candidates.as_slice(),
            calibration_sample,
            stage_seed,
        );

        send_return_components_progress(
            progress_sender,
            ReturnComponentsStageProgressPhase::ContinuousNigBasinDiagnostics,
            1,
            1,
            Some("basin diagnostics ready"),
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

fn merge_seed_frontier_candidates(
    moment_seed_candidates: Vec<ScoredNigCandidate>,
    em_scored_candidates: Vec<ScoredNigCandidate>,
) -> Vec<ScoredNigCandidate> {
    let mut combined_candidates = moment_seed_candidates;

    combined_candidates.extend(em_scored_candidates);
    combined_candidates.sort_by(ScoredNigCandidate::compare_by_objective_and_configuration);

    deduplicate_candidates_by_key(combined_candidates, |candidate| {
        candidate_configuration_key(candidate.configuration)
    })
}

pub(crate) fn build_nig_moment_seed_configurations(
    calibration_sample: &ReturnComponentsStageContinuousReturnCalibrationSample,
    solver_settings: &CalibrationOptimizerSettings,
) -> Vec<janiform_model_kernels::nig::NigSharedPlusShiftConfiguration> {
    with_continuous_fit_thread_pool(solver_settings.cpu_count, || {
        let optimizer_context = build_continuous_return_optimizer_context(calibration_sample);

        let broad_candidate_count = solver_settings
            .max_iterations
            .max(BasinSeedFamily::ALL.len());

        let moment_candidate_count =
            family_aligned_candidate_count(broad_candidate_count, BasinSeedFamily::ALL.len());

        let search_bounds =
            theoretical_search_bounds(calibration_sample.observed_profile, moment_candidate_count);

        build_moment_seed_candidates(
            &optimizer_context,
            calibration_sample,
            search_bounds,
            moment_candidate_count,
            None,
        )
        .into_iter()
        .map(|candidate| candidate.configuration)
        .collect()
    })
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

    let (likelihood_objective, exact_likelihood_diagnostics) =
        continuous_block_likelihood_objective_with_failures(
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

    let fit_breakdown =
        super::super::contracts::ReturnComponentsStageContinuousDistributionFitBreakdownDiagnostic {
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
            exact_likelihood_failures: exact_likelihood_diagnostics.failures,
            exact_closed_form_single_side_buy_block_count: exact_likelihood_diagnostics
                .evaluator_block_counts
                .closed_form_single_side_buy_block_count,
            exact_closed_form_single_side_sell_block_count: exact_likelihood_diagnostics
                .evaluator_block_counts
                .closed_form_single_side_sell_block_count,
            exact_tilted_mixed_inversion_block_count: exact_likelihood_diagnostics
                .evaluator_block_counts
                .tilted_mixed_inversion_block_count,
            exact_structural_failure_count: exact_likelihood_diagnostics.structural_failure_count,
            exact_uncertified_scope_count: exact_likelihood_diagnostics.uncertified_scope_count,
            exact_global_average_log_likelihood_error_upper_bound: exact_likelihood_diagnostics
                .global_average_log_likelihood_error_upper_bound,
            exact_conditional_average_log_likelihood_error_upper_bound:
                exact_likelihood_diagnostics
                    .conditional_average_log_likelihood_error_upper_bound,
            ..profile_fit_breakdown
        };

    ContinuousReturnCandidateEvaluation {
        objective_score: comparable_continuous_distribution_fit_score(
            likelihood_objective.shifted_objective_score,
            &fit_breakdown,
        ),
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
        fit_breakdown,
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

pub(super) fn score_search_points(
    optimizer_context: &ContinuousReturnOptimizerContext,
    search_points: &[NigShapeSearchPoint],
    fidelity_tier: ContinuousLikelihoodFidelityTier,
    progress_phase: Option<ReturnComponentsStageProgressPhase>,
    progress_sender: Option<&Sender<ReturnComponentsStageProgressUpdate>>,
) -> Vec<ScoredNigCandidate> {
    let completed_search_point_count = std::sync::atomic::AtomicUsize::new(0);
    let total_search_point_count = search_points.len();

    search_points
        .par_iter()
        .filter_map(|search_point| {
            let scored_candidate =
                evaluate_cached_search_point(optimizer_context, *search_point, fidelity_tier).map(
                    |cached_evaluation| ScoredNigCandidate {
                        search_point: *search_point,
                        configuration: cached_evaluation.configuration,
                        objective_score: cached_evaluation.objective.shifted_objective_score,
                    },
                );

            if let Some(progress_phase) = progress_phase {
                let completed_count = completed_search_point_count
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                    + 1;

                let search_point_label = format!("search_point_{completed_count:03}");

                send_return_components_evaluation_progress(
                    progress_sender,
                    progress_phase,
                    completed_count,
                    total_search_point_count,
                    Some(search_point_label.as_str()),
                    completed_count,
                    total_search_point_count,
                );
            }

            scored_candidate
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

pub(super) fn retain_diverse_scored_candidates(
    ranked_candidates: Vec<ScoredNigCandidate>,
    _search_bounds: NigShapeSearchBounds,
    maximum_count: usize,
) -> Vec<ScoredNigCandidate> {
    let deduplicated_candidates = deduplicate_candidates_by_key(ranked_candidates, |candidate| {
        candidate_configuration_key(candidate.configuration)
    });

    if deduplicated_candidates.is_empty() {
        return Vec::new();
    }

    let Some(feature_thresholds) =
        BroadSeedFeatureThresholds::from_candidates(deduplicated_candidates.as_slice())
    else {
        return Vec::new();
    };

    let mut retained_candidates: Vec<ScoredNigCandidate> = Vec::new();

    for family in BasinSeedFamily::ALL {
        let Some(seed_candidate) = select_family_seed_candidate(
            deduplicated_candidates.as_slice(),
            feature_thresholds,
            family,
        ) else {
            continue;
        };

        retained_candidates.push(seed_candidate.clone());
        if retained_candidates.len() >= maximum_count {
            return retained_candidates;
        }
    }

    for candidate in deduplicated_candidates {
        retained_candidates.push(candidate);
        if retained_candidates.len() >= maximum_count {
            break;
        }
    }

    retained_candidates
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

fn family_aligned_candidate_count(candidate_budget: usize, family_count: usize) -> usize {
    if family_count == 0 {
        return candidate_budget.max(1);
    }

    let requested_budget = candidate_budget.max(family_count);
    let rounded_family_count = requested_budget
        .div_ceil(family_count)
        .saturating_mul(family_count);

    rounded_family_count.max(family_count)
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
        if retained_candidates.len() >= BasinSeedFamily::ALL.len() {
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

    if let Ok(cache_guard) = optimizer_context.cache.lock()
        && let Some(cached_value) = cache_guard.get(&cache_key)
    {
        return *cached_value;
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
    progress_sender: Option<&Sender<ReturnComponentsStageProgressUpdate>>,
) -> Vec<OptimizedNigSearchCandidate> {
    let completed_seed_candidate_count = std::sync::atomic::AtomicUsize::new(0);
    let total_seed_candidate_count = seed_candidates.len();

    seed_candidates
        .par_iter()
        .filter_map(|seed_candidate| {
            let final_evaluation = evaluate_cached_search_point(
                optimizer_context,
                seed_candidate.scored_candidate.search_point,
                ContinuousLikelihoodFidelityTier::Full,
            )?;

            let optimized_candidate = OptimizedNigSearchCandidate {
                configuration: final_evaluation.configuration,
                family: seed_candidate.family,
                seed_search_point: seed_candidate.scored_candidate.search_point,
                final_search_point: seed_candidate.scored_candidate.search_point,
                optimizer_diagnostic: ContinuousReturnOptimizerDiagnostic {
                    seed_objective_score: seed_candidate.scored_candidate.objective_score,
                    local_objective_score: seed_candidate.scored_candidate.objective_score,
                    final_objective_score: final_evaluation.objective.shifted_objective_score,
                    local_evaluation_count: 0,
                    stop_reason: "proxy_em_frontier",
                    basin_diagnostics: Vec::new(),
                },
            };

            let completed_count = completed_seed_candidate_count
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                + 1;

            let seed_label = format!("basin_seed_{completed_count:03}");

            send_return_components_evaluation_progress(
                progress_sender,
                ReturnComponentsStageProgressPhase::ContinuousNigBasinOptimization,
                completed_count,
                total_seed_candidate_count,
                Some(seed_label.as_str()),
                completed_count,
                total_seed_candidate_count,
            );

            Some(optimized_candidate)
        })
        .collect()
}

fn retain_distinct_basin_candidates(
    mut optimized_candidates: Vec<OptimizedNigSearchCandidate>,
    _search_bounds: NigShapeSearchBounds,
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

fn search_point_to_vector(search_point: NigShapeSearchPoint) -> [f64; 4] {
    [
        search_point.log_alpha,
        search_point.buy_skew_coordinate,
        search_point.sell_relative_coordinate,
        search_point.log_delta_offset,
    ]
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
