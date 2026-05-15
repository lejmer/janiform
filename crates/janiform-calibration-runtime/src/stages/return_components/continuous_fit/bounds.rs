use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::Sender;

use janiform_foundations::low_discrepancy::van_der_corput;
use janiform_foundations::numeric::root_mean_square_or_infinity;
use janiform_model_kernels::nig::NigSharedPlusShiftConfiguration;
use rayon::iter::{IntoParallelIterator, ParallelIterator};

use super::super::contracts::{
    ReturnComponentsStageProgressPhase, ReturnComponentsStageProgressUpdate,
};
use super::super::derived_contracts::return_components_stage_positive_domain_floor;
use super::super::execution::theoretical_net_process_moments;
use super::super::progress::send_return_components_evaluation_progress;
use super::constants::RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_PROFILE_CONFIDENCE_Z_SCORE;
use super::likelihood::maximum_profiled_open_beta_ratio;
use super::profile::scale_aware_absolute_error;
use super::sample::build_continuous_return_optimizer_context;
pub(super) use super::shape_mapping::{
    configuration_from_shape_search_point, coordinate_to_ratio, coordinate_to_share,
    inferred_delta_center, ratio_to_coordinate, relative_share_from_beta_ratios,
    sell_beta_ratio_from_relative_share, share_to_coordinate,
};
use super::types::{
    BasinSeedFamily, ContinuousMomentTargets, ContinuousReturnOptimizerContext, NigSearchRegime,
    NigShapeSearchBounds, NigShapeSearchPoint,
    ReturnComponentsStageContinuousReturnCalibrationSample, ScoredNigCandidate,
};

pub(crate) fn nig_search_point_from_configuration(
    calibration_sample: &ReturnComponentsStageContinuousReturnCalibrationSample,
    configuration: NigSharedPlusShiftConfiguration,
) -> Option<NigShapeSearchPoint> {
    let optimizer_context = build_continuous_return_optimizer_context(calibration_sample);

    let alpha = configuration.alpha;
    if !alpha.is_finite() || alpha <= return_components_stage_positive_domain_floor() {
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
    if !delta_offset.is_finite() || delta_offset <= return_components_stage_positive_domain_floor()
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

pub(crate) fn nig_configuration_search_point_mapping(
    calibration_sample: &ReturnComponentsStageContinuousReturnCalibrationSample,
    search_point: NigShapeSearchPoint,
) -> Option<super::shape_mapping::NigSharedPlusShiftConfigurationSearchPointMapping> {
    let optimizer_context = build_continuous_return_optimizer_context(calibration_sample);
    super::shape_mapping::exact_configuration_and_gradient_from_shape_search_point(
        &optimizer_context,
        search_point,
    )
}

pub(crate) fn nig_search_bounds_for_calibration_sample(
    calibration_sample: &ReturnComponentsStageContinuousReturnCalibrationSample,
    moment_candidate_count: usize,
) -> ([f64; 4], [f64; 4]) {
    let search_bounds =
        theoretical_search_bounds(calibration_sample.observed_profile, moment_candidate_count);

    (search_bounds.lower_bounds, search_bounds.upper_bounds)
}

pub(super) fn build_moment_seed_candidates(
    optimizer_context: &ContinuousReturnOptimizerContext,
    calibration_sample: &ReturnComponentsStageContinuousReturnCalibrationSample,
    theoretical_search_bounds: NigShapeSearchBounds,
    moment_candidate_count: usize,
    retained_candidate_count: usize,
    progress_sender: Option<&Sender<ReturnComponentsStageProgressUpdate>>,
) -> Vec<ScoredNigCandidate> {
    let moment_targets = ContinuousMomentTargets::from_profile(calibration_sample.observed_profile);
    let alpha_interval = theoretical_alpha_bounds(calibration_sample.observed_profile);
    let completed_candidate_count = AtomicUsize::new(0);

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

            let completed_count = completed_candidate_count.fetch_add(1, Ordering::Relaxed) + 1;
            let candidate_label = format!("moment_seed_{:03}", candidate_index + 1);

            send_return_components_evaluation_progress(
                progress_sender,
                ReturnComponentsStageProgressPhase::ContinuousNigMomentSeedGeneration,
                completed_count,
                moment_candidate_count,
                Some(candidate_label.as_str()),
                completed_count,
                moment_candidate_count,
            );

            Some(ScoredNigCandidate {
                search_point,
                configuration,
                objective_score,
            })
        })
        .collect();

    ranked_candidates.sort_by(ScoredNigCandidate::compare_by_objective_and_configuration);

    let retained_candidates = super::optimization::retain_diverse_scored_candidates(
        ranked_candidates,
        theoretical_search_bounds,
        retained_candidate_count.max(BasinSeedFamily::ALL.len()),
    );

    let retained_candidates =
        super::optimization::deduplicate_candidates_by_key(retained_candidates, |candidate| {
            format!(
                "{}:{}:{}:{}",
                candidate.search_point.log_alpha.to_bits(),
                candidate.search_point.buy_skew_coordinate.to_bits(),
                candidate.search_point.sell_relative_coordinate.to_bits(),
                candidate.search_point.log_delta_offset.to_bits(),
            )
        });

    if retained_candidates.is_empty() {
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

    retained_candidates
}

pub(super) fn build_search_regimes_from_moment_seeds(
    _optimizer_context: &ContinuousReturnOptimizerContext,
    moment_seed_candidates: &[ScoredNigCandidate],
    theoretical_search_bounds: NigShapeSearchBounds,
) -> Vec<NigSearchRegime> {
    let mut profiled_search_regimes: Vec<NigSearchRegime> = moment_seed_candidates
        .iter()
        .map(|moment_seed_candidate| {
            regime_from_seed_frontier(
                moment_seed_candidate.search_point,
                moment_seed_candidates,
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

fn regime_from_seed_frontier(
    seed_search_point: NigShapeSearchPoint,
    moment_seed_candidates: &[ScoredNigCandidate],
    theoretical_search_bounds: NigShapeSearchBounds,
) -> NigSearchRegime {
    let seed_vector = [
        seed_search_point.log_alpha,
        seed_search_point.buy_skew_coordinate,
        seed_search_point.sell_relative_coordinate,
        seed_search_point.log_delta_offset,
    ];

    let lower_bounds: [f64; 4] = std::array::from_fn(|coordinate_index| {
        nearest_lower_coordinate(
            seed_vector[coordinate_index],
            coordinate_index,
            moment_seed_candidates,
            theoretical_search_bounds.lower_bounds[coordinate_index],
        )
    });

    let upper_bounds: [f64; 4] = std::array::from_fn(|coordinate_index| {
        nearest_upper_coordinate(
            seed_vector[coordinate_index],
            coordinate_index,
            moment_seed_candidates,
            theoretical_search_bounds.upper_bounds[coordinate_index],
        )
    });

    NigSearchRegime {
        log_alpha_center: seed_search_point.log_alpha,
        buy_skew_center: seed_search_point.buy_skew_coordinate,
        sell_relative_center: seed_search_point.sell_relative_coordinate,
        log_delta_center: seed_search_point.log_delta_offset,
        log_alpha_half_span: symmetric_regime_half_span(
            seed_vector[0],
            lower_bounds[0],
            upper_bounds[0],
        ),
        buy_skew_half_span: symmetric_regime_half_span(
            seed_vector[1],
            lower_bounds[1],
            upper_bounds[1],
        ),
        sell_relative_half_span: symmetric_regime_half_span(
            seed_vector[2],
            lower_bounds[2],
            upper_bounds[2],
        ),
        log_delta_half_span: symmetric_regime_half_span(
            seed_vector[3],
            lower_bounds[3],
            upper_bounds[3],
        ),
    }
}

fn nearest_lower_coordinate(
    seed_coordinate: f64,
    coordinate_index: usize,
    moment_seed_candidates: &[ScoredNigCandidate],
    theoretical_lower_bound: f64,
) -> f64 {
    let nearest_lower_seed = moment_seed_candidates
        .iter()
        .map(|candidate| coordinate_at_index(candidate.search_point, coordinate_index))
        .filter(|candidate_coordinate| *candidate_coordinate < seed_coordinate)
        .max_by(|left, right| left.total_cmp(right));

    nearest_lower_seed
        .map(|neighbor_coordinate| 0.5 * (seed_coordinate + neighbor_coordinate))
        .unwrap_or(theoretical_lower_bound)
}

fn nearest_upper_coordinate(
    seed_coordinate: f64,
    coordinate_index: usize,
    moment_seed_candidates: &[ScoredNigCandidate],
    theoretical_upper_bound: f64,
) -> f64 {
    let nearest_upper_seed = moment_seed_candidates
        .iter()
        .map(|candidate| coordinate_at_index(candidate.search_point, coordinate_index))
        .filter(|candidate_coordinate| *candidate_coordinate > seed_coordinate)
        .min_by(|left, right| left.total_cmp(right));

    nearest_upper_seed
        .map(|neighbor_coordinate| 0.5 * (seed_coordinate + neighbor_coordinate))
        .unwrap_or(theoretical_upper_bound)
}

fn coordinate_at_index(search_point: NigShapeSearchPoint, coordinate_index: usize) -> f64 {
    match coordinate_index {
        0 => search_point.log_alpha,
        1 => search_point.buy_skew_coordinate,
        2 => search_point.sell_relative_coordinate,
        3 => search_point.log_delta_offset,
        _ => unreachable!("NIG search point has exactly four coordinates"),
    }
}

fn symmetric_regime_half_span(center: f64, lower_bound: f64, upper_bound: f64) -> f64 {
    (center - lower_bound)
        .abs()
        .max((upper_bound - center).abs())
}

pub(super) fn theoretical_search_bounds(
    observed_profile: super::types::ContinuousReturnDistributionProfile,
    moment_candidate_count: usize,
) -> NigShapeSearchBounds {
    let (alpha_lower_bound, alpha_upper_bound) = theoretical_alpha_bounds(observed_profile);

    let maximum_profiled_beta_ratio = maximum_profiled_open_beta_ratio(alpha_lower_bound);

    let relative_share_lower_bound = minimum_profiled_relative_share(moment_candidate_count);
    let relative_share_upper_bound = 1.0 - relative_share_lower_bound;

    NigShapeSearchBounds {
        lower_bounds: [
            alpha_lower_bound.ln(),
            ratio_to_coordinate(-maximum_profiled_beta_ratio),
            share_to_coordinate(relative_share_lower_bound),
            theoretical_log_delta_offset_bounds(observed_profile).0,
        ],
        upper_bounds: [
            alpha_upper_bound.ln(),
            ratio_to_coordinate(maximum_profiled_beta_ratio),
            share_to_coordinate(relative_share_upper_bound),
            theoretical_log_delta_offset_bounds(observed_profile).1,
        ],
    }
}

pub(super) fn theoretical_alpha_bounds(
    observed_profile: super::types::ContinuousReturnDistributionProfile,
) -> (f64, f64) {
    let observed_variance = observed_profile
        .return_volatility
        .powi(2)
        .max(return_components_stage_positive_domain_floor());

    let excess_kurtosis_standard_error = observed_profile.excess_kurtosis_standard_error();
    let observed_excess_kurtosis = observed_profile.adjusted_excess_kurtosis.max(0.0);
    let excess_kurtosis_confidence_half_width =
        RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_PROFILE_CONFIDENCE_Z_SCORE
            * excess_kurtosis_standard_error;
    let distinguishable_positive_excess_kurtosis =
        excess_kurtosis_confidence_half_width.max(return_components_stage_positive_domain_floor());

    let lower_plausible_excess_kurtosis =
        if observed_excess_kurtosis > excess_kurtosis_confidence_half_width {
            observed_excess_kurtosis - excess_kurtosis_confidence_half_width
        } else {
            distinguishable_positive_excess_kurtosis
        }
        .max(return_components_stage_positive_domain_floor());
    let upper_plausible_excess_kurtosis = (observed_excess_kurtosis
        + excess_kurtosis_confidence_half_width)
        .max(lower_plausible_excess_kurtosis);

    let lower_alpha_bound = (3.0 / (observed_variance * upper_plausible_excess_kurtosis))
        .sqrt()
        .max(return_components_stage_positive_domain_floor());
    let upper_alpha_bound = (3.0 / (observed_variance * lower_plausible_excess_kurtosis))
        .sqrt()
        .max(lower_alpha_bound);

    (lower_alpha_bound, upper_alpha_bound)
}

fn theoretical_log_delta_offset_bounds(
    observed_profile: super::types::ContinuousReturnDistributionProfile,
) -> (f64, f64) {
    let sample_count = observed_profile.sample_count.max(3) as f64;

    let degrees_of_freedom = (sample_count - 1.0).max(1.0);

    let lower_tail_quantile = approximate_chi_square_quantile(degrees_of_freedom, 0.025);
    let upper_tail_quantile = approximate_chi_square_quantile(degrees_of_freedom, 0.975);

    let lower_variance_ratio = (degrees_of_freedom / upper_tail_quantile)
        .max(return_components_stage_positive_domain_floor());
    let upper_variance_ratio = (degrees_of_freedom / lower_tail_quantile).max(lower_variance_ratio);

    (lower_variance_ratio.ln(), upper_variance_ratio.ln())
}

fn minimum_profiled_relative_share(moment_candidate_count: usize) -> f64 {
    let denominator = (moment_candidate_count.max(2) + 1) as f64;
    (1.0 / denominator).clamp(0.0f64.next_up(), 0.5)
}

fn moment_seed_objective_score(
    configuration: NigSharedPlusShiftConfiguration,
    optimizer_context: &ContinuousReturnOptimizerContext,
    moment_targets: ContinuousMomentTargets,
) -> Option<f64> {
    let (_, fitted_variance, fitted_skewness, fitted_excess_kurtosis) =
        theoretical_net_process_moments(
            configuration,
            optimizer_context.open_session_sell_share_proxy,
        )?;

    let variance_scale = moment_targets
        .return_volatility
        .powi(2)
        .max(return_components_stage_positive_domain_floor());

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

    Some(root_mean_square_or_infinity(
        [variance_fit, skewness_fit, excess_kurtosis_fit].as_slice(),
    ))
}

fn approximate_chi_square_quantile(degrees_of_freedom: f64, probability_level: f64) -> f64 {
    let standardized_quantile = standard_normal_quantile(probability_level);

    let wilson_hilferty_term = 1.0 - 2.0 / (9.0 * degrees_of_freedom)
        + standardized_quantile * (2.0 / (9.0 * degrees_of_freedom)).sqrt();

    (degrees_of_freedom * wilson_hilferty_term.powi(3))
        .max(return_components_stage_positive_domain_floor())
}

fn standard_normal_quantile(probability_level: f64) -> f64 {
    if probability_level <= 0.025 {
        -RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_PROFILE_CONFIDENCE_Z_SCORE
    } else {
        RETURN_COMPONENTS_STAGE_CONTINUOUS_FIT_PROFILE_CONFIDENCE_Z_SCORE
    }
}
