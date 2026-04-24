use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::Sender;

use janiform_calibration_artifacts::CalibrationOptimizerSettings;
use janiform_model_kernels::nig::{NigParameters, NigSharedPlusShiftConfiguration};
use puruspe::Inu_Knu;
use rayon::iter::{IndexedParallelIterator, IntoParallelRefIterator, ParallelIterator};

use super::super::contracts::{
    ReturnComponentsStageProgressPhase, ReturnComponentsStageProgressUpdate,
};
use super::super::progress::{
    send_return_components_convergence_progress, send_return_components_progress,
};
use super::bounds::{
    nig_search_point_from_configuration, relative_share_from_beta_ratios,
    sell_beta_ratio_from_relative_share,
};
use super::likelihood::{maximum_open_beta_ratio, minimum_open_beta_ratio};
use super::types::{
    NigShapeSearchPoint, ReturnComponentsStageContinuousReturnCalibrationSample, ScoredNigCandidate,
};
use crate::stages::return_components::execution::{
    round_negative_candidate_value, round_positive_candidate_value, round_signed_candidate_value,
};

struct ProxyEmProgressContext<'a> {
    progress_sender: Option<&'a Sender<ReturnComponentsStageProgressUpdate>>,
    completed_seed_count: &'a AtomicUsize,
    total_seed_count: usize,
    seed_label: String,
}

pub(super) fn build_effective_nig_em_search_points(
    calibration_sample: &ReturnComponentsStageContinuousReturnCalibrationSample,
    moment_seed_candidates: &[ScoredNigCandidate],
    solver_settings: &CalibrationOptimizerSettings,
    progress_sender: Option<&Sender<ReturnComponentsStageProgressUpdate>>,
) -> Vec<NigShapeSearchPoint> {
    let completed_seed_count = AtomicUsize::new(0);
    let total_seed_count = moment_seed_candidates.len();

    let mut refined_search_points: Vec<NigShapeSearchPoint> = moment_seed_candidates
        .par_iter()
        .enumerate()
        .filter_map(|moment_seed_candidate| {
            let (moment_seed_index, moment_seed_candidate) = moment_seed_candidate;

            let initial_parameters = proxy_initial_parameters(moment_seed_candidate.configuration)?;

            let seed_label = format!("proxy_em_seed_{:03}", moment_seed_index + 1);

            let progress_context = ProxyEmProgressContext {
                progress_sender,
                completed_seed_count: &completed_seed_count,
                total_seed_count,
                seed_label,
            };

            let refined_parameters = fit_effective_zero_drift_nig_via_em(
                calibration_sample.observed_returns.as_slice(),
                calibration_sample.business_time_scales.as_slice(),
                initial_parameters,
                solver_settings,
                Some(&progress_context),
            )?;

            let search_point = shared_search_point_from_effective_parameters(
                calibration_sample,
                moment_seed_candidate.configuration,
                refined_parameters,
            )?;

            let completed_count = completed_seed_count.fetch_add(1, Ordering::Relaxed) + 1;

            send_return_components_progress(
                progress_sender,
                ReturnComponentsStageProgressPhase::ContinuousNigProxyEmSeedGeneration,
                completed_count,
                total_seed_count,
                Some(progress_context.seed_label.as_str()),
            );

            Some(search_point)
        })
        .collect();

    refined_search_points.sort_by(|left, right| {
        left.log_alpha
            .total_cmp(&right.log_alpha)
            .then_with(|| {
                left.buy_skew_coordinate
                    .total_cmp(&right.buy_skew_coordinate)
            })
            .then_with(|| {
                left.sell_relative_coordinate
                    .total_cmp(&right.sell_relative_coordinate)
            })
            .then_with(|| left.log_delta_offset.total_cmp(&right.log_delta_offset))
    });

    refined_search_points.dedup_by(|left, right| {
        left.log_alpha.to_bits() == right.log_alpha.to_bits()
            && left.buy_skew_coordinate.to_bits() == right.buy_skew_coordinate.to_bits()
            && left.sell_relative_coordinate.to_bits() == right.sell_relative_coordinate.to_bits()
            && left.log_delta_offset.to_bits() == right.log_delta_offset.to_bits()
    });

    refined_search_points
}

fn proxy_initial_parameters(
    configuration: NigSharedPlusShiftConfiguration,
) -> Option<NigParameters> {
    let effective_beta = 0.5 * (configuration.beta_0 + configuration.derived_sell_beta());

    NigParameters::try_new(
        configuration.alpha,
        effective_beta,
        configuration.delta,
        0.0,
    )
    .ok()
}

fn fit_effective_zero_drift_nig_via_em(
    observed_returns: &[f64],
    business_time_scales: &[f64],
    initial_parameters: NigParameters,
    solver_settings: &CalibrationOptimizerSettings,
    progress_context: Option<&ProxyEmProgressContext>,
) -> Option<NigParameters> {
    if observed_returns.is_empty() || business_time_scales.is_empty() {
        return None;
    }

    let aligned_count = observed_returns.len().min(business_time_scales.len());
    if aligned_count == 0 {
        return None;
    }

    let aligned_observed_returns = &observed_returns[..aligned_count];
    let aligned_business_time_scales = &business_time_scales[..aligned_count];

    let mut current_parameters = initial_parameters;

    let mut current_objective = effective_zero_drift_negative_log_likelihood(
        current_parameters,
        aligned_observed_returns,
        aligned_business_time_scales,
    )?;

    let maximum_iteration_count = solver_settings.max_iterations.max(1);
    for iteration_index in 0..maximum_iteration_count {
        let e_step_statistics = build_e_step_statistics(
            current_parameters,
            aligned_observed_returns,
            aligned_business_time_scales,
        )?;

        let next_parameters = maximize_effective_zero_drift_parameters(
            aligned_observed_returns,
            aligned_business_time_scales,
            e_step_statistics,
        )?;

        let next_objective = effective_zero_drift_negative_log_likelihood(
            next_parameters,
            aligned_observed_returns,
            aligned_business_time_scales,
        )?;

        let objective_tolerance = solver_settings
            .absolute_tolerance
            .max(solver_settings.relative_tolerance * current_objective.abs().max(1.0));

        let objective_improvement = current_objective - next_objective;

        if let Some(progress_context) = progress_context {
            send_return_components_convergence_progress(
                progress_context.progress_sender,
                ReturnComponentsStageProgressPhase::ContinuousNigProxyEmSeedGeneration,
                progress_context
                    .completed_seed_count
                    .load(Ordering::Relaxed),
                progress_context.total_seed_count,
                Some(progress_context.seed_label.as_str()),
                iteration_index + 1,
                convergence_ratio(objective_improvement, objective_tolerance),
            );
        }

        if objective_improvement <= objective_tolerance {
            return Some(next_parameters);
        }

        current_parameters = next_parameters;
        current_objective = next_objective;
    }

    Some(current_parameters)
}

fn convergence_ratio(objective_improvement: f64, objective_tolerance: f64) -> Option<f64> {
    if !objective_improvement.is_finite()
        || !objective_tolerance.is_finite()
        || objective_tolerance <= 0.0
    {
        return None;
    }

    Some(objective_improvement / objective_tolerance)
}

#[derive(Debug, Clone, Copy)]
struct EffectiveZeroDriftEstepStatistics {
    expected_latent_increment_sum: f64,
    expected_inverse_latent_increment_weighted_scale_square_sum: f64,
}

fn build_e_step_statistics(
    parameters: NigParameters,
    observed_returns: &[f64],
    business_time_scales: &[f64],
) -> Option<EffectiveZeroDriftEstepStatistics> {
    let mut expected_latent_increment_sum = 0.0;
    let mut expected_inverse_latent_increment_weighted_scale_square_sum = 0.0;

    for (observed_return, business_time_scale) in
        observed_returns.iter().zip(business_time_scales.iter())
    {
        if !business_time_scale.is_finite() || *business_time_scale <= 0.0 {
            return None;
        }

        let scaled_delta = parameters.delta * *business_time_scale;
        let conditional_scale =
            (scaled_delta * scaled_delta + observed_return * observed_return).sqrt();
        if !conditional_scale.is_finite() || conditional_scale <= 0.0 {
            return None;
        }

        let bessel_argument = parameters.alpha * conditional_scale;
        if !bessel_argument.is_finite() || bessel_argument <= 0.0 {
            return None;
        }

        let (_, k0) = Inu_Knu(0.0, bessel_argument);
        let (_, k1) = Inu_Knu(1.0, bessel_argument);
        if !k0.is_finite() || !k1.is_finite() || k1 <= 0.0 {
            return None;
        }

        let bessel_ratio = k0 / k1;
        if !bessel_ratio.is_finite() {
            return None;
        }

        let expected_latent_increment = conditional_scale * bessel_ratio / parameters.alpha;
        let expected_inverse_latent_increment =
            parameters.alpha * bessel_ratio / conditional_scale + 2.0 / conditional_scale.powi(2);
        if !expected_latent_increment.is_finite() || !expected_inverse_latent_increment.is_finite()
        {
            return None;
        }

        expected_latent_increment_sum += expected_latent_increment;
        expected_inverse_latent_increment_weighted_scale_square_sum +=
            business_time_scale.powi(2) * expected_inverse_latent_increment;
    }

    Some(EffectiveZeroDriftEstepStatistics {
        expected_latent_increment_sum,
        expected_inverse_latent_increment_weighted_scale_square_sum,
    })
}

fn maximize_effective_zero_drift_parameters(
    observed_returns: &[f64],
    business_time_scales: &[f64],
    e_step_statistics: EffectiveZeroDriftEstepStatistics,
) -> Option<NigParameters> {
    let sample_count = observed_returns.len();
    if sample_count == 0 {
        return None;
    }

    let total_business_time_scale: f64 = business_time_scales.iter().copied().sum();
    let observed_return_sum: f64 = observed_returns.iter().copied().sum();
    if !total_business_time_scale.is_finite() || !observed_return_sum.is_finite() {
        return None;
    }

    if e_step_statistics.expected_latent_increment_sum <= 0.0
        || !e_step_statistics.expected_latent_increment_sum.is_finite()
    {
        return None;
    }

    let inverse_scale_denominator = e_step_statistics
        .expected_inverse_latent_increment_weighted_scale_square_sum
        - total_business_time_scale.powi(2) / e_step_statistics.expected_latent_increment_sum;
    if !inverse_scale_denominator.is_finite() || inverse_scale_denominator <= 0.0 {
        return None;
    }

    let delta = (sample_count as f64 / inverse_scale_denominator).sqrt();
    if !delta.is_finite() || delta <= 0.0 {
        return None;
    }

    let gamma = delta * total_business_time_scale / e_step_statistics.expected_latent_increment_sum;
    let beta = observed_return_sum / e_step_statistics.expected_latent_increment_sum;
    if !gamma.is_finite() || gamma <= 0.0 || !beta.is_finite() {
        return None;
    }

    let alpha = beta.hypot(gamma);
    NigParameters::try_new(alpha, beta, delta, 0.0).ok()
}

fn effective_zero_drift_negative_log_likelihood(
    parameters: NigParameters,
    observed_returns: &[f64],
    business_time_scales: &[f64],
) -> Option<f64> {
    let gamma_squared =
        (parameters.alpha - parameters.beta.abs()) * (parameters.alpha + parameters.beta.abs());
    if !gamma_squared.is_finite() || gamma_squared <= 0.0 {
        return None;
    }

    let gamma = gamma_squared.sqrt();

    let mut negative_log_likelihood = 0.0;

    for (observed_return, business_time_scale) in
        observed_returns.iter().zip(business_time_scales.iter())
    {
        if !business_time_scale.is_finite() || *business_time_scale <= 0.0 {
            return None;
        }

        let scaled_delta = parameters.delta * *business_time_scale;

        let conditional_scale =
            (scaled_delta * scaled_delta + observed_return * observed_return).sqrt();

        let bessel_argument = parameters.alpha * conditional_scale;
        if !conditional_scale.is_finite()
            || conditional_scale <= 0.0
            || !bessel_argument.is_finite()
            || bessel_argument <= 0.0
        {
            return None;
        }

        let (_, k1) = Inu_Knu(1.0, bessel_argument);
        if !k1.is_finite() || k1 <= 0.0 {
            return None;
        }

        let log_density = parameters.alpha.ln() + scaled_delta.ln()
            - std::f64::consts::PI.ln()
            - conditional_scale.ln()
            + scaled_delta * gamma
            + parameters.beta * observed_return
            + k1.ln();
        if !log_density.is_finite() {
            return None;
        }

        negative_log_likelihood -= log_density;
    }

    Some(negative_log_likelihood / observed_returns.len() as f64)
}

fn shared_search_point_from_effective_parameters(
    calibration_sample: &ReturnComponentsStageContinuousReturnCalibrationSample,
    seed_configuration: NigSharedPlusShiftConfiguration,
    effective_parameters: NigParameters,
) -> Option<NigShapeSearchPoint> {
    let seed_buy_beta_ratio = seed_configuration.beta_0 / seed_configuration.alpha;
    let seed_sell_beta_ratio = seed_configuration.derived_sell_beta() / seed_configuration.alpha;

    let relative_share =
        relative_share_from_beta_ratios(seed_buy_beta_ratio, seed_sell_beta_ratio)?;

    let effective_midpoint_ratio = effective_parameters.beta / effective_parameters.alpha;

    let bounded_midpoint_ratio =
        clamp_midpoint_ratio_to_relative_share(relative_share, effective_midpoint_ratio)?;

    let buy_beta_ratio =
        buy_beta_ratio_from_midpoint_and_relative_share(bounded_midpoint_ratio, relative_share)?;
    let sell_beta_ratio = sell_beta_ratio_from_relative_share(buy_beta_ratio, relative_share);

    if !sell_beta_ratio.is_finite() || sell_beta_ratio >= buy_beta_ratio {
        return None;
    }

    let configuration = NigSharedPlusShiftConfiguration::try_new(
        round_positive_candidate_value(effective_parameters.alpha),
        round_signed_candidate_value(effective_parameters.alpha * buy_beta_ratio),
        round_positive_candidate_value(effective_parameters.delta),
        0.0,
        round_negative_candidate_value(
            effective_parameters.alpha * (sell_beta_ratio - buy_beta_ratio),
        ),
    )
    .ok()?;

    nig_search_point_from_configuration(calibration_sample, configuration)
}

fn clamp_midpoint_ratio_to_relative_share(relative_share: f64, midpoint_ratio: f64) -> Option<f64> {
    if !relative_share.is_finite()
        || relative_share <= 0.0
        || relative_share >= 1.0
        || !midpoint_ratio.is_finite()
    {
        return None;
    }

    let lower_bound = minimum_open_beta_ratio();
    let upper_bound = maximum_open_beta_ratio();

    let maximum_midpoint_ratio =
        0.5 * (upper_bound + sell_beta_ratio_from_relative_share(upper_bound, relative_share));

    Some(midpoint_ratio.clamp(lower_bound, maximum_midpoint_ratio))
}

fn buy_beta_ratio_from_midpoint_and_relative_share(
    midpoint_ratio: f64,
    relative_share: f64,
) -> Option<f64> {
    if !midpoint_ratio.is_finite()
        || !relative_share.is_finite()
        || relative_share <= 0.0
        || relative_share >= 1.0
    {
        return None;
    }

    let lower_bound = minimum_open_beta_ratio();

    let buy_beta_ratio =
        (2.0 * midpoint_ratio - lower_bound * (1.0 - relative_share)) / (1.0 + relative_share);
    if !buy_beta_ratio.is_finite() {
        return None;
    }

    Some(buy_beta_ratio.clamp(lower_bound, maximum_open_beta_ratio()))
}

#[cfg(test)]
mod tests {
    use janiform_calibration_artifacts::CalibrationOptimizerSettings;
    use janiform_foundations::randomness::SplitMix64SimulationRandomnessStream;
    use janiform_model_kernels::nig::{NigParameters, NigSharedPlusShiftConfiguration};

    use super::build_effective_nig_em_search_points;
    use super::effective_zero_drift_negative_log_likelihood;
    use super::fit_effective_zero_drift_nig_via_em;
    use crate::stages::return_components::continuous_fit::bounds::{
        build_moment_seed_candidates, theoretical_search_bounds,
    };
    use crate::stages::return_components::continuous_fit::sample::build_continuous_return_optimizer_context;
    use crate::stages::return_components::continuous_fit::tests::synthetic_calibration_sample_from_configuration;
    use crate::stages::return_components::continuous_fit::types::BasinSeedFamily;

    #[test]
    fn effective_zero_drift_em_improves_proxy_likelihood() {
        let target_parameters = NigParameters::try_new(18.0, -4.0, 3.0e-4, 0.0)
            .expect("target parameters should validate");

        let business_time_scales: Vec<f64> = (0..128)
            .map(|sample_index| 8.0 + sample_index as f64 / 16.0)
            .collect();

        let observed_returns =
            sample_effective_nig_returns(target_parameters, business_time_scales.as_slice(), 19);

        let initial_parameters = NigParameters::try_new(36.0, -1.0, 1.0e-4, 0.0)
            .expect("initial parameters should validate");

        let solver_settings =
            CalibrationOptimizerSettings::try_new("proxy_em", 24, 1, 1.0e-9, 1.0e-7, 0.0)
                .expect("solver settings should validate");

        let initial_objective = effective_zero_drift_negative_log_likelihood(
            initial_parameters,
            observed_returns.as_slice(),
            business_time_scales.as_slice(),
        )
        .expect("initial objective should evaluate");

        let refined_parameters = fit_effective_zero_drift_nig_via_em(
            observed_returns.as_slice(),
            business_time_scales.as_slice(),
            initial_parameters,
            &solver_settings,
            None,
        )
        .expect("em fit should converge");

        let refined_objective = effective_zero_drift_negative_log_likelihood(
            refined_parameters,
            observed_returns.as_slice(),
            business_time_scales.as_slice(),
        )
        .expect("refined objective should evaluate");

        assert!(refined_objective < initial_objective);
    }

    #[test]
    fn proxy_em_seed_builder_returns_valid_search_points() {
        let configuration =
            NigSharedPlusShiftConfiguration::try_new(80.0, -12.0, 2.5e-4, 0.0, -6.0)
                .expect("configuration should validate");

        let calibration_sample = synthetic_calibration_sample_from_configuration(configuration);

        let optimizer_context = build_continuous_return_optimizer_context(&calibration_sample);

        let theoretical_bounds = theoretical_search_bounds(
            calibration_sample.observed_profile,
            BasinSeedFamily::ALL.len(),
        );

        let moment_seed_candidates = build_moment_seed_candidates(
            &optimizer_context,
            &calibration_sample,
            theoretical_bounds,
            BasinSeedFamily::ALL.len(),
            None,
        );

        let solver_settings =
            CalibrationOptimizerSettings::try_new("proxy_em", 24, 1, 1.0e-9, 1.0e-7, 0.0)
                .expect("solver settings should validate");

        let search_points = build_effective_nig_em_search_points(
            &calibration_sample,
            moment_seed_candidates.as_slice(),
            &solver_settings,
            None,
        );

        assert!(!search_points.is_empty());
    }

    fn sample_effective_nig_returns(
        parameters: NigParameters,
        business_time_scales: &[f64],
        seed: u64,
    ) -> Vec<f64> {
        let mut randomness_stream = SplitMix64SimulationRandomnessStream::from_seed(seed);

        business_time_scales
            .iter()
            .map(|business_time_scale| {
                parameters
                    .sample_increment(*business_time_scale, &mut randomness_stream)
                    .expect("synthetic NIG increment should sample")
            })
            .collect()
    }
}
