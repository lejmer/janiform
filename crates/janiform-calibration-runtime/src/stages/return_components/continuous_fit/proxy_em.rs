use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::Sender;

use janiform_calibration_artifacts::CalibrationOptimizerSettings;
use janiform_model_kernels::nig::{NigParameters, NigSharedPlusShiftConfiguration};
use rayon::iter::{IndexedParallelIterator, IntoParallelRefIterator, ParallelIterator};

use super::super::contracts::{
    ReturnComponentsStageProgressPhase, ReturnComponentsStageProgressUpdate,
};
use super::super::progress::{
    ReturnComponentsConvergenceProgress, send_return_components_convergence_progress,
    send_return_components_progress,
};
use super::bessel::{log_modified_bessel_k1, modified_bessel_k0_to_k1_ratio};
use super::bounds::nig_search_point_from_configuration;
use super::likelihood::{maximum_open_beta_ratio, minimum_open_beta_ratio};
use super::proxy_em_diagnostics::{
    ProxyEmBestObjectiveDiagnostic, ProxyEmFitOutcome, ProxyEmFitStopReason, ProxyEmSeedCounters,
};
use super::proxy_em_sample::{ProxyEmFittingSample, build_proxy_em_fitting_sample};
use super::types::{
    ContinuousReturnProxyEmDiagnostic, NigShapeSearchPoint,
    ReturnComponentsStageContinuousReturnCalibrationSample, ScoredNigCandidate,
};
use crate::stages::return_components::execution::{
    round_negative_candidate_value, round_positive_candidate_value, round_signed_candidate_value,
};

struct ProxyEmProgressContext<'a> {
    progress_sender: Option<&'a Sender<ReturnComponentsStageProgressUpdate>>,
    completed_seed_count: &'a AtomicUsize,
    completed_evaluation_count: &'a AtomicUsize,
    best_objective: &'a Mutex<ProxyEmBestObjectiveDiagnostic>,
    total_seed_count: usize,
    seed_label: String,
}

struct ProxyEmSearchPointBuildContext<'a> {
    solver_settings: &'a CalibrationOptimizerSettings,
    maximum_iteration_count: usize,
    progress_sender: Option<&'a Sender<ReturnComponentsStageProgressUpdate>>,
    seed_counters: &'a ProxyEmSeedCounters,
    completed_evaluation_count: &'a AtomicUsize,
    best_objective: &'a Mutex<ProxyEmBestObjectiveDiagnostic>,
    total_seed_count: usize,
}

pub(super) struct EffectiveNigProxyEmSearchPointBuildOutput {
    pub(super) search_points: Vec<NigShapeSearchPoint>,
    pub(super) diagnostic: ContinuousReturnProxyEmDiagnostic,
}

pub(super) fn build_effective_nig_em_search_points(
    calibration_sample: &ReturnComponentsStageContinuousReturnCalibrationSample,
    moment_seed_candidates: &[ScoredNigCandidate],
    solver_settings: &CalibrationOptimizerSettings,
    maximum_iteration_count: usize,
    progress_sender: Option<&Sender<ReturnComponentsStageProgressUpdate>>,
) -> EffectiveNigProxyEmSearchPointBuildOutput {
    let seed_counters = ProxyEmSeedCounters::new();
    let completed_evaluation_count = AtomicUsize::new(0);
    let best_objective = Mutex::new(ProxyEmBestObjectiveDiagnostic::default());
    let total_seed_count = moment_seed_candidates.len();
    let fitting_sample = build_proxy_em_fitting_sample(calibration_sample);

    let mut refined_search_points: Vec<NigShapeSearchPoint> = moment_seed_candidates
        .par_iter()
        .enumerate()
        .filter_map(|moment_seed_candidate| {
            let (moment_seed_index, moment_seed_candidate) = moment_seed_candidate;

            let seed_label = format!("proxy_em_seed_{:03}", moment_seed_index + 1);

            let search_point = build_single_effective_nig_em_search_point(
                calibration_sample,
                &fitting_sample,
                moment_seed_candidate,
                &ProxyEmSearchPointBuildContext {
                    solver_settings,
                    maximum_iteration_count,
                    progress_sender,
                    seed_counters: &seed_counters,
                    completed_evaluation_count: &completed_evaluation_count,
                    best_objective: &best_objective,
                    total_seed_count,
                },
                seed_label.as_str(),
            );

            let completed_count = seed_counters
                .completed_seed_count
                .fetch_add(1, Ordering::Relaxed)
                + 1;

            send_return_components_progress(
                progress_sender,
                ReturnComponentsStageProgressPhase::ContinuousNigProxyEmSeedGeneration,
                completed_count,
                total_seed_count,
                Some(seed_label.as_str()),
            );

            search_point
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

    EffectiveNigProxyEmSearchPointBuildOutput {
        search_points: refined_search_points,
        diagnostic: seed_counters.diagnostic(
            &best_objective,
            &completed_evaluation_count,
            total_seed_count,
            maximum_iteration_count.max(1),
            fitting_sample.sample_count(),
            fitting_sample.weight_sum(),
        ),
    }
}

fn build_single_effective_nig_em_search_point(
    calibration_sample: &ReturnComponentsStageContinuousReturnCalibrationSample,
    fitting_sample: &ProxyEmFittingSample,
    moment_seed_candidate: &ScoredNigCandidate,
    build_context: &ProxyEmSearchPointBuildContext,
    seed_label: &str,
) -> Option<NigShapeSearchPoint> {
    let Some(initial_parameters) = proxy_initial_parameters(moment_seed_candidate.configuration)
    else {
        build_context
            .seed_counters
            .invalid_initial_seed_count
            .fetch_add(1, Ordering::Relaxed);
        return None;
    };

    let progress_context = ProxyEmProgressContext {
        progress_sender: build_context.progress_sender,
        completed_seed_count: &build_context.seed_counters.completed_seed_count,
        completed_evaluation_count: build_context.completed_evaluation_count,
        best_objective: build_context.best_objective,
        total_seed_count: build_context.total_seed_count,
        seed_label: seed_label.to_string(),
    };

    let Some(fit_outcome) = fit_effective_zero_drift_nig_via_em(
        fitting_sample.observed_returns(),
        fitting_sample.business_time_scales(),
        fitting_sample.weights(),
        initial_parameters,
        build_context.solver_settings,
        build_context.maximum_iteration_count,
        Some(&progress_context),
    ) else {
        build_context
            .seed_counters
            .record_initial_numerical_failure();
        return None;
    };

    build_context
        .seed_counters
        .record_fit_stop_reason(fit_outcome.stop_reason);

    let search_point = shared_search_point_from_effective_parameters(
        calibration_sample,
        moment_seed_candidate.configuration,
        fit_outcome.parameters,
    );

    if search_point.is_some() {
        build_context
            .seed_counters
            .refined_seed_count
            .fetch_add(1, Ordering::Relaxed);
    } else {
        build_context
            .seed_counters
            .projection_failure_seed_count
            .fetch_add(1, Ordering::Relaxed);
    }

    search_point
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
    weights: &[f64],
    initial_parameters: NigParameters,
    solver_settings: &CalibrationOptimizerSettings,
    maximum_iteration_count: usize,
    progress_context: Option<&ProxyEmProgressContext>,
) -> Option<ProxyEmFitOutcome> {
    if observed_returns.is_empty() || business_time_scales.is_empty() || weights.is_empty() {
        return None;
    }

    let aligned_count = observed_returns
        .len()
        .min(business_time_scales.len())
        .min(weights.len());
    if aligned_count == 0 {
        return None;
    }

    let aligned_observed_returns = &observed_returns[..aligned_count];
    let aligned_business_time_scales = &business_time_scales[..aligned_count];
    let aligned_weights = &weights[..aligned_count];

    let mut current_parameters = initial_parameters;

    let mut current_objective = effective_zero_drift_negative_log_likelihood(
        current_parameters,
        aligned_observed_returns,
        aligned_business_time_scales,
        aligned_weights,
    )?;

    if let Some(progress_context) = progress_context {
        record_proxy_em_best_objective(progress_context.best_objective, current_objective, None);
    }

    for _ in 0..maximum_iteration_count.max(1) {
        let Some(e_step_statistics) = build_e_step_statistics(
            current_parameters,
            aligned_observed_returns,
            aligned_business_time_scales,
            aligned_weights,
        ) else {
            return Some(ProxyEmFitOutcome {
                parameters: current_parameters,
                stop_reason: ProxyEmFitStopReason::NumericalFailure,
            });
        };

        let Some(next_parameters) = maximize_effective_zero_drift_parameters(
            aligned_observed_returns,
            aligned_business_time_scales,
            aligned_weights,
            e_step_statistics,
        ) else {
            return Some(ProxyEmFitOutcome {
                parameters: current_parameters,
                stop_reason: ProxyEmFitStopReason::NumericalFailure,
            });
        };

        let Some(next_objective) = effective_zero_drift_negative_log_likelihood(
            next_parameters,
            aligned_observed_returns,
            aligned_business_time_scales,
            aligned_weights,
        ) else {
            return Some(ProxyEmFitOutcome {
                parameters: current_parameters,
                stop_reason: ProxyEmFitStopReason::NumericalFailure,
            });
        };

        let objective_tolerance = solver_settings
            .absolute_tolerance
            .max(solver_settings.relative_tolerance * current_objective.abs().max(1.0));

        let objective_improvement = current_objective - next_objective;

        if let Some(progress_context) = progress_context {
            let completed_evaluation_count = progress_context
                .completed_evaluation_count
                .fetch_add(1, Ordering::Relaxed)
                + 1;

            let current_convergence_ratio =
                convergence_ratio(objective_improvement, objective_tolerance);

            let best_objective = record_proxy_em_best_objective(
                progress_context.best_objective,
                next_objective,
                current_convergence_ratio,
            );

            send_return_components_convergence_progress(
                progress_context.progress_sender,
                ReturnComponentsConvergenceProgress {
                    phase: ReturnComponentsStageProgressPhase::ContinuousNigProxyEmSeedGeneration,
                    completed_units: progress_context
                        .completed_seed_count
                        .load(Ordering::Relaxed),
                    total_units: progress_context.total_seed_count,
                    current_item_label: Some(progress_context.seed_label.as_str()),
                    current_evaluation_count: completed_evaluation_count,
                    current_weighted_objective: None,
                    best_weighted_objective: best_objective,
                    current_convergence_ratio,
                },
            );
        }

        if objective_improvement <= 0.0 {
            return Some(ProxyEmFitOutcome {
                parameters: current_parameters,
                stop_reason: ProxyEmFitStopReason::NonImproving,
            });
        }

        if objective_improvement <= objective_tolerance {
            return Some(ProxyEmFitOutcome {
                parameters: next_parameters,
                stop_reason: ProxyEmFitStopReason::Converged,
            });
        }

        current_parameters = next_parameters;
        current_objective = next_objective;
    }

    Some(ProxyEmFitOutcome {
        parameters: current_parameters,
        stop_reason: ProxyEmFitStopReason::IterationBudgetExhausted,
    })
}

fn record_proxy_em_best_objective(
    best_objective: &Mutex<ProxyEmBestObjectiveDiagnostic>,
    objective: f64,
    convergence_ratio: Option<f64>,
) -> Option<f64> {
    if !objective.is_finite() {
        return best_objective
            .lock()
            .ok()
            .and_then(|guard| guard.best_objective);
    }

    let Ok(mut best_objective_guard) = best_objective.lock() else {
        return None;
    };

    match best_objective_guard.best_objective {
        Some(current_best_objective) if current_best_objective <= objective => {}
        _ => {
            best_objective_guard.best_objective = Some(objective);
            best_objective_guard.final_convergence_ratio =
                convergence_ratio.filter(|ratio| ratio.is_finite());
        }
    }

    best_objective_guard.best_objective
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
    weights: &[f64],
) -> Option<EffectiveZeroDriftEstepStatistics> {
    let mut expected_latent_increment_sum = 0.0;
    let mut expected_inverse_latent_increment_weighted_scale_square_sum = 0.0;

    for ((observed_return, business_time_scale), weight) in observed_returns
        .iter()
        .zip(business_time_scales.iter())
        .zip(weights.iter())
    {
        if !business_time_scale.is_finite() || *business_time_scale <= 0.0 {
            return None;
        }

        if !weight.is_finite() || *weight <= 0.0 {
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

        let bessel_ratio = modified_bessel_k0_to_k1_ratio(bessel_argument)?;

        let expected_latent_increment = conditional_scale * bessel_ratio / parameters.alpha;
        let expected_inverse_latent_increment =
            parameters.alpha * bessel_ratio / conditional_scale + 2.0 / conditional_scale.powi(2);
        if !expected_latent_increment.is_finite() || !expected_inverse_latent_increment.is_finite()
        {
            return None;
        }

        expected_latent_increment_sum += weight * expected_latent_increment;
        expected_inverse_latent_increment_weighted_scale_square_sum +=
            weight * business_time_scale.powi(2) * expected_inverse_latent_increment;
    }

    Some(EffectiveZeroDriftEstepStatistics {
        expected_latent_increment_sum,
        expected_inverse_latent_increment_weighted_scale_square_sum,
    })
}

fn maximize_effective_zero_drift_parameters(
    observed_returns: &[f64],
    business_time_scales: &[f64],
    weights: &[f64],
    e_step_statistics: EffectiveZeroDriftEstepStatistics,
) -> Option<NigParameters> {
    let total_weight = total_weight(weights)?;
    if total_weight <= 0.0 {
        return None;
    }

    let total_business_time_scale: f64 = business_time_scales
        .iter()
        .zip(weights.iter())
        .map(|(business_time_scale, weight)| business_time_scale * weight)
        .sum();
    let observed_return_sum: f64 = observed_returns
        .iter()
        .zip(weights.iter())
        .map(|(observed_return, weight)| observed_return * weight)
        .sum();
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

    let delta = (total_weight / inverse_scale_denominator).sqrt();
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
    weights: &[f64],
) -> Option<f64> {
    let gamma_squared =
        (parameters.alpha - parameters.beta.abs()) * (parameters.alpha + parameters.beta.abs());
    if !gamma_squared.is_finite() || gamma_squared <= 0.0 {
        return None;
    }

    let gamma = gamma_squared.sqrt();

    let mut negative_log_likelihood = 0.0;
    let mut weight_sum = 0.0;

    for ((observed_return, business_time_scale), weight) in observed_returns
        .iter()
        .zip(business_time_scales.iter())
        .zip(weights.iter())
    {
        if !business_time_scale.is_finite() || *business_time_scale <= 0.0 {
            return None;
        }

        if !weight.is_finite() || *weight <= 0.0 {
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

        let log_bessel_k1 = log_modified_bessel_k1(bessel_argument)?;

        let log_density = parameters.alpha.ln() + scaled_delta.ln()
            - std::f64::consts::PI.ln()
            - conditional_scale.ln()
            + scaled_delta * gamma
            + parameters.beta * observed_return
            + log_bessel_k1;
        if !log_density.is_finite() {
            return None;
        }

        negative_log_likelihood -= weight * log_density;
        weight_sum += weight;
    }

    if !weight_sum.is_finite() || weight_sum <= 0.0 {
        return None;
    }

    Some(negative_log_likelihood / weight_sum)
}

fn total_weight(weights: &[f64]) -> Option<f64> {
    let total_weight: f64 = weights.iter().copied().sum();
    if !total_weight.is_finite() || total_weight <= 0.0 {
        return None;
    }

    Some(total_weight)
}

fn shared_search_point_from_effective_parameters(
    calibration_sample: &ReturnComponentsStageContinuousReturnCalibrationSample,
    seed_configuration: NigSharedPlusShiftConfiguration,
    effective_parameters: NigParameters,
) -> Option<NigShapeSearchPoint> {
    let seed_buy_beta_ratio = seed_configuration.beta_0 / seed_configuration.alpha;
    let seed_sell_beta_ratio = seed_configuration.derived_sell_beta() / seed_configuration.alpha;

    let seed_beta_ratio_spread = seed_buy_beta_ratio - seed_sell_beta_ratio;
    if !seed_beta_ratio_spread.is_finite() || seed_beta_ratio_spread <= 0.0 {
        return None;
    }

    let effective_beta_ratio = effective_parameters.beta / effective_parameters.alpha;

    let (buy_beta_ratio, sell_beta_ratio) = project_effective_beta_ratio_to_shared_shift_ratios(
        calibration_sample.open_session_sell_share_proxy,
        seed_beta_ratio_spread,
        effective_beta_ratio,
    )?;

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

fn project_effective_beta_ratio_to_shared_shift_ratios(
    open_session_sell_share_proxy: f64,
    seed_beta_ratio_spread: f64,
    effective_beta_ratio: f64,
) -> Option<(f64, f64)> {
    let buy_weight = 1.0 - open_session_sell_share_proxy.clamp(0.0, 1.0);
    let sell_weight = open_session_sell_share_proxy.clamp(0.0, 1.0);
    if !buy_weight.is_finite()
        || !sell_weight.is_finite()
        || !seed_beta_ratio_spread.is_finite()
        || seed_beta_ratio_spread <= 0.0
        || !effective_beta_ratio.is_finite()
    {
        return None;
    }

    let lower_bound = minimum_open_beta_ratio();
    let upper_bound = maximum_open_beta_ratio();

    let bounded_effective_beta_ratio = effective_beta_ratio.clamp(lower_bound, upper_bound);

    let upper_limited_spread = if sell_weight > 0.0 {
        (upper_bound - bounded_effective_beta_ratio) / sell_weight
    } else {
        upper_bound - lower_bound
    };

    let lower_limited_spread = if buy_weight > 0.0 {
        (bounded_effective_beta_ratio - lower_bound) / buy_weight
    } else {
        upper_bound - lower_bound
    };

    let beta_ratio_spread = seed_beta_ratio_spread
        .min(upper_limited_spread)
        .min(lower_limited_spread);
    if !beta_ratio_spread.is_finite() || beta_ratio_spread <= 0.0 {
        return None;
    }

    Some((
        bounded_effective_beta_ratio + sell_weight * beta_ratio_spread,
        bounded_effective_beta_ratio - buy_weight * beta_ratio_spread,
    ))
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
        let weights = vec![1.0; observed_returns.len()];

        let initial_parameters = NigParameters::try_new(36.0, -1.0, 1.0e-4, 0.0)
            .expect("initial parameters should validate");

        let solver_settings =
            CalibrationOptimizerSettings::try_new("proxy_em", 24, 1, 1.0e-9, 1.0e-7, 0.0)
                .expect("solver settings should validate");

        let initial_objective = effective_zero_drift_negative_log_likelihood(
            initial_parameters,
            observed_returns.as_slice(),
            business_time_scales.as_slice(),
            weights.as_slice(),
        )
        .expect("initial objective should evaluate");

        let fit_outcome = fit_effective_zero_drift_nig_via_em(
            observed_returns.as_slice(),
            business_time_scales.as_slice(),
            weights.as_slice(),
            initial_parameters,
            &solver_settings,
            24,
            None,
        )
        .expect("em fit should converge");

        let refined_objective = effective_zero_drift_negative_log_likelihood(
            fit_outcome.parameters,
            observed_returns.as_slice(),
            business_time_scales.as_slice(),
            weights.as_slice(),
        )
        .expect("refined objective should evaluate");

        assert!(refined_objective < initial_objective);
    }

    #[test]
    fn weighted_proxy_em_matches_repeated_observations() {
        let observed_returns = vec![-0.015, -0.002, 0.004, 0.017];
        let business_time_scales = vec![2.0, 4.0, 3.0, 5.0];
        let compressed_weights = vec![3.0, 2.0, 4.0, 1.0];

        let mut repeated_returns = Vec::new();
        let mut repeated_business_time_scales = Vec::new();
        for ((observed_return, business_time_scale), weight) in observed_returns
            .iter()
            .zip(business_time_scales.iter())
            .zip(compressed_weights.iter())
        {
            for _ in 0..*weight as usize {
                repeated_returns.push(*observed_return);
                repeated_business_time_scales.push(*business_time_scale);
            }
        }
        let repeated_weights = vec![1.0; repeated_returns.len()];

        let initial_parameters = NigParameters::try_new(20.0, -2.0, 0.005, 0.0)
            .expect("initial parameters should validate");

        let solver_settings =
            CalibrationOptimizerSettings::try_new("proxy_em", 48, 1, 1.0e-10, 1.0e-8, 0.0)
                .expect("solver settings should validate");

        let compressed_fit = fit_effective_zero_drift_nig_via_em(
            observed_returns.as_slice(),
            business_time_scales.as_slice(),
            compressed_weights.as_slice(),
            initial_parameters,
            &solver_settings,
            48,
            None,
        )
        .expect("compressed weighted fit should evaluate");
        let repeated_fit = fit_effective_zero_drift_nig_via_em(
            repeated_returns.as_slice(),
            repeated_business_time_scales.as_slice(),
            repeated_weights.as_slice(),
            initial_parameters,
            &solver_settings,
            48,
            None,
        )
        .expect("repeated unweighted fit should evaluate");

        assert!((compressed_fit.parameters.alpha - repeated_fit.parameters.alpha).abs() <= 1.0e-10);
        assert!((compressed_fit.parameters.beta - repeated_fit.parameters.beta).abs() <= 1.0e-10);
        assert!((compressed_fit.parameters.delta - repeated_fit.parameters.delta).abs() <= 1.0e-10);
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
            BasinSeedFamily::ALL.len(),
            None,
        );

        let solver_settings =
            CalibrationOptimizerSettings::try_new("proxy_em", 24, 1, 1.0e-9, 1.0e-7, 0.0)
                .expect("solver settings should validate");

        let proxy_em_output = build_effective_nig_em_search_points(
            &calibration_sample,
            moment_seed_candidates.as_slice(),
            &solver_settings,
            24,
            None,
        );

        assert!(!proxy_em_output.search_points.is_empty());
        assert!(proxy_em_output.diagnostic.best_objective.is_some());
        assert!(proxy_em_output.diagnostic.final_convergence_ratio.is_some());
        assert_eq!(
            proxy_em_output.diagnostic.requested_seed_count,
            moment_seed_candidates.len()
        );
        assert_eq!(
            proxy_em_output.diagnostic.completed_seed_count,
            moment_seed_candidates.len()
        );
        assert_eq!(proxy_em_output.diagnostic.iteration_budget_per_seed, 24);
        assert!(proxy_em_output.diagnostic.fitting_sample_count > 0);
        assert!(proxy_em_output.diagnostic.fitting_sample_weight_sum > 0.0);
        assert!(proxy_em_output.diagnostic.evaluation_count > 0);
        assert!(proxy_em_output.diagnostic.refined_seed_count > 0);
        assert_eq!(proxy_em_output.diagnostic.invalid_initial_seed_count, 0);
        assert_eq!(proxy_em_output.diagnostic.projection_failure_seed_count, 0);
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
