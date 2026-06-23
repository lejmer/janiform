use janiform_calibration_artifacts::CalibrationOptimizerSettings;
use janiform_foundations::cancellation::CancellationToken;
use janiform_foundations::numeric::usize_to_f64_exact;
use janiform_model_kernels::market_pressure::MarketPressureTransitionRates;
use janiform_work_scheduler::{StaticWorkPoolInput, run_static_work_pool};

use super::event_sides::side_observation_log_likelihood;
use super::hmm::{
    OrderedHmmProgress, ScaledStateEmission, forward_backward_ordered_model,
    neutral_ordered_probabilities, normalize_probabilities, update_ordered_transition_rates,
};
use super::{
    HiddenSideBalanceFit, MarketPressureFitProgressObserver, SidePredictionFit,
    emit_market_pressure_fit_progress, transition_rate_from_expected_count_and_exposure,
    validate_strictly_increasing_support,
};
use crate::market_pressure::errors::MarketPressureFitError;
use crate::market_pressure::fit::{
    MarketPressureCoarseReturnLaw, MarketPressureCoarseReturnObservation,
    MarketPressureSideBalanceObservation,
};
use crate::stages::model_selection::bayesian_information_score_for_effective_sample_count;
use crate::stages::parallelism::{
    ContiguousWorkChunk, bounded_stage_worker_count, build_stage_thread_pool,
    contiguous_work_chunks,
};
use crate::stages::return_components::{
    NetTradingTimeReturnLogDensityInput, log_density_net_trading_time_return,
};

#[derive(Debug, Clone, PartialEq)]
struct CoarseReturnFit {
    transition_rates: MarketPressureTransitionRates,
    state_probabilities: Vec<Vec<f64>>,
    interval_joint_probabilities: Vec<Vec<Vec<f64>>>,
    log_likelihood: f64,
    parameter_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct OneStateCoarseReturnFit {
    log_likelihood: f64,
    parameter_count: usize,
}

#[derive(Debug, PartialEq)]
struct CoarseEmissionChunk<T> {
    start_position: usize,
    emissions: Result<Vec<T>, MarketPressureFitError>,
}

struct CoarseForwardBackwardInput<'a> {
    observations: &'a [MarketPressureCoarseReturnObservation],
    support_levels: [f64; 2],
    open_event_rate_per_hour: f64,
    coarse_return_law: MarketPressureCoarseReturnLaw,
    anchor_drift_per_signed_balance: f64,
    transition_rates: &'a MarketPressureTransitionRates,
    transition_deltas: &'a [f64],
    solver_settings: &'a CalibrationOptimizerSettings,
}

struct CoarseBinaryFitInput<'a> {
    observations: &'a [MarketPressureCoarseReturnObservation],
    support_levels: [f64; 2],
    open_event_rate_per_hour: f64,
    coarse_return_law: MarketPressureCoarseReturnLaw,
    initial_anchor_drift_per_signed_balance: f64,
    initial_transition_rates: MarketPressureTransitionRates,
    solver_settings: &'a CalibrationOptimizerSettings,
}

pub(super) fn fit_coarse_augmented_single_side_model(
    observations: &[MarketPressureSideBalanceObservation],
    coarse_return_observations: &[MarketPressureCoarseReturnObservation],
    coarse_return_law: Option<MarketPressureCoarseReturnLaw>,
    one_state_fit: &SidePredictionFit,
    open_event_rate_per_hour: f64,
    solver_settings: &CalibrationOptimizerSettings,
    progress_observer: &mut Option<MarketPressureFitProgressObserver<'_>>,
) -> Result<Option<HiddenSideBalanceFit>, MarketPressureFitError> {
    let Some(coarse_return_law) = coarse_return_law else {
        return Ok(None);
    };

    if coarse_return_observations.len() < 2
        || !open_event_rate_per_hour.is_finite()
        || open_event_rate_per_hour <= 0.0
    {
        return Ok(None);
    }

    let side_imbalance_level = one_state_fit.support_levels[0];
    if side_imbalance_level == 0.0 {
        return Ok(None);
    }

    let support_magnitude = side_imbalance_level.abs();
    let support_levels = [-support_magnitude, support_magnitude];

    validate_strictly_increasing_support(&support_levels)?;

    let Some(initial_anchor_drift_per_signed_balance) = initial_anchor_drift_per_signed_balance(
        observations,
        coarse_return_observations,
        side_imbalance_level,
        support_magnitude,
        open_event_rate_per_hour,
    )?
    else {
        return Ok(None);
    };

    let Some(initial_transition_rates) =
        initial_coarse_transition_rates(coarse_return_observations)?
    else {
        return Ok(None);
    };

    emit_market_pressure_fit_progress(
        progress_observer,
        0,
        1,
        "fit one-state coarse-return pressure reference",
    );

    let Some(one_state_coarse_fit) = fit_one_state_coarse_return_model(
        coarse_return_observations,
        side_imbalance_level,
        open_event_rate_per_hour,
        coarse_return_law,
        solver_settings,
    )?
    else {
        return Ok(None);
    };

    emit_market_pressure_fit_progress(
        progress_observer,
        0,
        1,
        "fit binary coarse-return pressure model",
    );

    let Some(coarse_fit) = fit_binary_coarse_return_model(
        CoarseBinaryFitInput {
            observations: coarse_return_observations,
            support_levels,
            open_event_rate_per_hour,
            coarse_return_law,
            initial_anchor_drift_per_signed_balance,
            initial_transition_rates,
            solver_settings,
        },
        progress_observer,
    )?
    else {
        return Ok(None);
    };

    let effective_sample_count = usize_to_f64_exact(
        coarse_return_observations.len(),
        "market_pressure_coarse_return_count",
    )
    .map_err(|source| MarketPressureFitError::InvalidCoarseReturnObservationCount { source })?;

    if penalized_coarse_return_score(&coarse_fit, effective_sample_count)
        <= penalized_one_state_coarse_return_score(&one_state_coarse_fit, effective_sample_count)
    {
        return Ok(None);
    }

    let state_probabilities =
        side_state_probabilities_from_likelihood(observations, support_levels)?;

    Ok(Some(HiddenSideBalanceFit {
        support_levels: support_levels.to_vec(),
        transition_rates: coarse_fit.transition_rates,
        state_probabilities,
        coarse_state_probabilities: coarse_fit.state_probabilities,
    }))
}

fn fit_binary_coarse_return_model(
    input: CoarseBinaryFitInput<'_>,
    progress_observer: &mut Option<MarketPressureFitProgressObserver<'_>>,
) -> Result<Option<CoarseReturnFit>, MarketPressureFitError> {
    let CoarseBinaryFitInput {
        observations,
        support_levels,
        open_event_rate_per_hour,
        coarse_return_law,
        initial_anchor_drift_per_signed_balance,
        initial_transition_rates,
        solver_settings,
    } = input;

    let transition_deltas = coarse_return_transition_deltas(observations);
    let interval_transition_deltas = coarse_return_interval_transition_deltas(observations);

    let mut fit = forward_backward_coarse_return_model(CoarseForwardBackwardInput {
        observations,
        support_levels,
        open_event_rate_per_hour,
        coarse_return_law,
        anchor_drift_per_signed_balance: initial_anchor_drift_per_signed_balance,
        transition_rates: &initial_transition_rates,
        transition_deltas: transition_deltas.as_slice(),
        solver_settings,
    })?;

    let progress_total_units = solver_settings.max_iterations.saturating_add(1).max(1);
    emit_market_pressure_fit_progress(
        progress_observer,
        1,
        progress_total_units,
        "fit coarse-return pressure HMM",
    );

    for iteration in 0..solver_settings.max_iterations {
        let Some(updated_anchor_drift_per_signed_balance) = update_anchor_drift_per_signed_balance(
            observations,
            support_levels,
            open_event_rate_per_hour,
            &fit.state_probabilities,
        )?
        else {
            return Ok(None);
        };

        let updated_transition_rates = update_ordered_transition_rates(
            interval_transition_deltas.as_slice(),
            &fit.interval_joint_probabilities,
        )?;

        let updated_fit = forward_backward_coarse_return_model(CoarseForwardBackwardInput {
            observations,
            support_levels,
            open_event_rate_per_hour,
            coarse_return_law,
            anchor_drift_per_signed_balance: updated_anchor_drift_per_signed_balance,
            transition_rates: &updated_transition_rates,
            transition_deltas: transition_deltas.as_slice(),
            solver_settings,
        })?;

        let convergence_tolerance = solver_settings
            .absolute_tolerance
            .max(solver_settings.relative_tolerance * fit.log_likelihood.abs().max(1.0));

        let log_likelihood_improvement = updated_fit.log_likelihood - fit.log_likelihood;

        fit = updated_fit;

        let completed_units = iteration.saturating_add(2);
        emit_market_pressure_fit_progress(
            progress_observer,
            completed_units,
            progress_total_units,
            "fit coarse-return pressure HMM",
        );

        if log_likelihood_improvement.abs() <= convergence_tolerance {
            break;
        }
    }

    Ok(Some(fit))
}

fn coarse_return_transition_deltas(
    observations: &[MarketPressureCoarseReturnObservation],
) -> Vec<f64> {
    observations
        .iter()
        .map(|observation| observation.elapsed_hours)
        .collect()
}

fn coarse_return_interval_transition_deltas(
    observations: &[MarketPressureCoarseReturnObservation],
) -> Vec<f64> {
    observations
        .iter()
        .skip(1)
        .map(|observation| observation.elapsed_hours)
        .collect()
}

fn forward_backward_coarse_return_model(
    input: CoarseForwardBackwardInput<'_>,
) -> Result<CoarseReturnFit, MarketPressureFitError> {
    let CoarseForwardBackwardInput {
        observations,
        support_levels,
        open_event_rate_per_hour,
        coarse_return_law,
        anchor_drift_per_signed_balance,
        transition_rates,
        transition_deltas,
        solver_settings,
    } = input;

    validate_strictly_increasing_support(&support_levels)?;

    let emissions = scaled_coarse_return_emissions(
        observations,
        support_levels,
        open_event_rate_per_hour,
        coarse_return_law,
        anchor_drift_per_signed_balance,
        solver_settings,
    )?;

    let initial_probabilities = neutral_ordered_probabilities(&support_levels)?;

    let mut progress_observer = None;
    let mut progress = OrderedHmmProgress::new(&[], &mut progress_observer);
    let hidden_fit = forward_backward_ordered_model(
        transition_deltas,
        emissions.as_slice(),
        initial_probabilities,
        transition_rates,
        &mut progress,
    )?;

    // Fit diagnostics own the transition-rate snapshot used for this pass.
    Ok(CoarseReturnFit {
        transition_rates: transition_rates.clone(),
        state_probabilities: hidden_fit.state_probabilities,
        interval_joint_probabilities: hidden_fit.interval_joint_probabilities,
        log_likelihood: hidden_fit.log_likelihood,
        parameter_count: 3,
    })
}

fn update_anchor_drift_per_signed_balance(
    observations: &[MarketPressureCoarseReturnObservation],
    support_levels: [f64; 2],
    open_event_rate_per_hour: f64,
    state_probabilities: &[Vec<f64>],
) -> Result<Option<f64>, MarketPressureFitError> {
    let mut numerator = 0.0;
    let mut denominator = 0.0;

    for (observation, probabilities) in observations.iter().zip(state_probabilities.iter()) {
        if probabilities.len() != 2 {
            return Err(MarketPressureFitError::InvalidStateAssignments);
        }

        let expected_side_imbalance =
            probabilities[0] * support_levels[0] + probabilities[1] * support_levels[1];

        let signed_balance_exposure =
            expected_side_imbalance * open_event_rate_per_hour * observation.elapsed_hours;

        numerator += signed_balance_exposure * observation.log_return;
        denominator += signed_balance_exposure * signed_balance_exposure;
    }

    if denominator == 0.0 {
        return Ok(None);
    }

    let anchor_drift_per_signed_balance = numerator / denominator;
    if !anchor_drift_per_signed_balance.is_finite() {
        return Err(MarketPressureFitError::InvalidAnchorDriftPerSignedBalance {
            value: anchor_drift_per_signed_balance,
        });
    }

    Ok(Some(anchor_drift_per_signed_balance))
}

fn initial_anchor_drift_per_signed_balance(
    observations: &[MarketPressureSideBalanceObservation],
    coarse_return_observations: &[MarketPressureCoarseReturnObservation],
    side_imbalance_level: f64,
    support_magnitude: f64,
    open_event_rate_per_hour: f64,
) -> Result<Option<f64>, MarketPressureFitError> {
    if let Some(side_anchor_drift_per_signed_balance) =
        side_anchor_drift_per_signed_balance(observations, side_imbalance_level)?
        && side_anchor_drift_per_signed_balance != 0.0
    {
        return Ok(Some(side_anchor_drift_per_signed_balance));
    }

    let total_elapsed_hours = coarse_return_observations
        .iter()
        .map(|observation| observation.elapsed_hours)
        .sum::<f64>();

    let total_log_return = coarse_return_observations
        .iter()
        .map(|observation| observation.log_return)
        .sum::<f64>();

    let signed_balance_exposure =
        support_magnitude * open_event_rate_per_hour * total_elapsed_hours;

    if signed_balance_exposure == 0.0 || total_log_return == 0.0 {
        return Ok(None);
    }

    let anchor_drift_per_signed_balance = total_log_return / signed_balance_exposure;
    if !anchor_drift_per_signed_balance.is_finite() {
        return Err(MarketPressureFitError::InvalidAnchorDriftPerSignedBalance {
            value: anchor_drift_per_signed_balance,
        });
    }

    Ok(Some(anchor_drift_per_signed_balance))
}

fn side_anchor_drift_per_signed_balance(
    observations: &[MarketPressureSideBalanceObservation],
    side_imbalance_level: f64,
) -> Result<Option<f64>, MarketPressureFitError> {
    let mut numerator = 0.0;
    let mut denominator = 0.0;

    for observation in observations {
        let signed_balance_exposure =
            side_imbalance_level * observation.signed_balance_exposure_scale;

        numerator += signed_balance_exposure * observation.log_return;
        denominator += signed_balance_exposure * signed_balance_exposure;
    }

    if denominator == 0.0 {
        return Ok(None);
    }

    let anchor_drift_per_signed_balance = numerator / denominator;
    if !anchor_drift_per_signed_balance.is_finite() {
        return Err(MarketPressureFitError::InvalidAnchorDriftPerSignedBalance {
            value: anchor_drift_per_signed_balance,
        });
    }

    Ok(Some(anchor_drift_per_signed_balance))
}

fn initial_coarse_transition_rates(
    observations: &[MarketPressureCoarseReturnObservation],
) -> Result<Option<MarketPressureTransitionRates>, MarketPressureFitError> {
    let total_elapsed_hours = observations
        .iter()
        .map(|observation| observation.elapsed_hours)
        .sum::<f64>();

    if !total_elapsed_hours.is_finite() || total_elapsed_hours <= 0.0 {
        return Ok(None);
    }

    let transition_rate =
        transition_rate_from_expected_count_and_exposure(1.0, total_elapsed_hours)?;

    Ok(Some(MarketPressureTransitionRates::new(
        vec![transition_rate],
        vec![transition_rate],
    )))
}

fn fit_one_state_coarse_return_model(
    observations: &[MarketPressureCoarseReturnObservation],
    side_imbalance_level: f64,
    open_event_rate_per_hour: f64,
    coarse_return_law: MarketPressureCoarseReturnLaw,
    solver_settings: &CalibrationOptimizerSettings,
) -> Result<Option<OneStateCoarseReturnFit>, MarketPressureFitError> {
    let Some(anchor_drift_per_signed_balance) = one_state_anchor_drift_per_signed_balance(
        observations,
        side_imbalance_level,
        open_event_rate_per_hour,
    )?
    else {
        return Ok(None);
    };

    let log_likelihood = one_state_coarse_return_log_likelihood(
        observations,
        side_imbalance_level,
        open_event_rate_per_hour,
        coarse_return_law,
        anchor_drift_per_signed_balance,
        solver_settings,
    )?;

    Ok(Some(OneStateCoarseReturnFit {
        log_likelihood,
        parameter_count: 1,
    }))
}

fn one_state_anchor_drift_per_signed_balance(
    observations: &[MarketPressureCoarseReturnObservation],
    side_imbalance_level: f64,
    open_event_rate_per_hour: f64,
) -> Result<Option<f64>, MarketPressureFitError> {
    let mut numerator = 0.0;
    let mut denominator = 0.0;

    for observation in observations {
        let signed_balance_exposure =
            side_imbalance_level * open_event_rate_per_hour * observation.elapsed_hours;

        numerator += signed_balance_exposure * observation.log_return;
        denominator += signed_balance_exposure * signed_balance_exposure;
    }

    if denominator == 0.0 {
        return Ok(None);
    }

    let anchor_drift_per_signed_balance = numerator / denominator;
    if !anchor_drift_per_signed_balance.is_finite() {
        return Err(MarketPressureFitError::InvalidAnchorDriftPerSignedBalance {
            value: anchor_drift_per_signed_balance,
        });
    }

    Ok(Some(anchor_drift_per_signed_balance))
}

fn side_state_probabilities_from_likelihood(
    observations: &[MarketPressureSideBalanceObservation],
    support_levels: [f64; 2],
) -> Result<Vec<Vec<f64>>, MarketPressureFitError> {
    let initial_probabilities = neutral_ordered_probabilities(&support_levels)?;
    let mut state_probabilities = Vec::with_capacity(observations.len());

    for observation in observations {
        let emission = ScaledStateEmission::from_log_likelihoods(vec![
            side_observation_log_likelihood(observation, support_levels[0])?,
            side_observation_log_likelihood(observation, support_levels[1])?,
        ])?;

        let unnormalized = [
            initial_probabilities[0] * emission.likelihoods[0],
            initial_probabilities[1] * emission.likelihoods[1],
        ];

        let normalizer = normalize_probabilities(&unnormalized)?;

        state_probabilities.push(vec![
            unnormalized[0] / normalizer,
            unnormalized[1] / normalizer,
        ]);
    }

    Ok(state_probabilities)
}

fn one_state_coarse_return_log_likelihood(
    observations: &[MarketPressureCoarseReturnObservation],
    side_imbalance_level: f64,
    open_event_rate_per_hour: f64,
    coarse_return_law: MarketPressureCoarseReturnLaw,
    anchor_drift_per_signed_balance: f64,
    solver_settings: &CalibrationOptimizerSettings,
) -> Result<f64, MarketPressureFitError> {
    let emissions = coarse_return_log_likelihoods(
        observations,
        side_imbalance_level,
        open_event_rate_per_hour,
        coarse_return_law,
        anchor_drift_per_signed_balance,
        solver_settings,
    )?;

    Ok(emissions.into_iter().sum())
}

fn scaled_coarse_return_emissions(
    observations: &[MarketPressureCoarseReturnObservation],
    support_levels: [f64; 2],
    open_event_rate_per_hour: f64,
    coarse_return_law: MarketPressureCoarseReturnLaw,
    anchor_drift_per_signed_balance: f64,
    solver_settings: &CalibrationOptimizerSettings,
) -> Result<Vec<ScaledStateEmission>, MarketPressureFitError> {
    let build_emission = |observation: &MarketPressureCoarseReturnObservation| {
        scaled_coarse_return_emission(
            observation,
            support_levels,
            open_event_rate_per_hour,
            coarse_return_law,
            anchor_drift_per_signed_balance,
        )
    };

    build_coarse_emission_chunks(observations, solver_settings, build_emission)
}

fn coarse_return_log_likelihoods(
    observations: &[MarketPressureCoarseReturnObservation],
    side_imbalance_level: f64,
    open_event_rate_per_hour: f64,
    coarse_return_law: MarketPressureCoarseReturnLaw,
    anchor_drift_per_signed_balance: f64,
    solver_settings: &CalibrationOptimizerSettings,
) -> Result<Vec<f64>, MarketPressureFitError> {
    let build_log_likelihood = |observation: &MarketPressureCoarseReturnObservation| {
        coarse_return_log_likelihood(
            observation,
            side_imbalance_level,
            open_event_rate_per_hour,
            coarse_return_law,
            anchor_drift_per_signed_balance,
        )
    };

    build_coarse_emission_chunks(observations, solver_settings, build_log_likelihood)
}

fn build_coarse_emission_chunks<T, F>(
    observations: &[MarketPressureCoarseReturnObservation],
    solver_settings: &CalibrationOptimizerSettings,
    build_value: F,
) -> Result<Vec<T>, MarketPressureFitError>
where
    T: Send + 'static,
    F: Fn(&MarketPressureCoarseReturnObservation) -> Result<T, MarketPressureFitError>
        + Copy
        + Send
        + Sync,
{
    let worker_count = bounded_stage_worker_count(solver_settings.cpu_count, observations.len());
    if worker_count <= 1 {
        return observations.iter().map(build_value).collect();
    }

    let jobs = contiguous_work_chunks(observations.len(), worker_count);
    let thread_pool = build_stage_thread_pool(solver_settings.cpu_count);
    let mut chunks = Vec::with_capacity(jobs.len());

    run_static_work_pool(
        thread_pool.as_deref(),
        StaticWorkPoolInput::new(
            &mut chunks,
            CancellationToken::uncancelled(),
            jobs,
            worker_count,
            |job: ContiguousWorkChunk, _cancellation_token| CoarseEmissionChunk {
                start_position: job.start_position,
                emissions: observations[job.start_position..job.end_position]
                    .iter()
                    .map(build_value)
                    .collect(),
            },
            |chunks: &mut Vec<CoarseEmissionChunk<T>>, chunk, _status| chunks.push(chunk),
        ),
    );

    chunks.sort_by_key(|chunk| chunk.start_position);

    let mut values = Vec::with_capacity(observations.len());
    for chunk in chunks {
        values.extend(chunk.emissions?);
    }

    Ok(values)
}

fn scaled_coarse_return_emission(
    observation: &MarketPressureCoarseReturnObservation,
    support_levels: [f64; 2],
    open_event_rate_per_hour: f64,
    coarse_return_law: MarketPressureCoarseReturnLaw,
    anchor_drift_per_signed_balance: f64,
) -> Result<ScaledStateEmission, MarketPressureFitError> {
    let lower_log_likelihood = coarse_return_log_likelihood(
        observation,
        support_levels[0],
        open_event_rate_per_hour,
        coarse_return_law,
        anchor_drift_per_signed_balance,
    )?;

    let upper_log_likelihood = coarse_return_log_likelihood(
        observation,
        support_levels[1],
        open_event_rate_per_hour,
        coarse_return_law,
        anchor_drift_per_signed_balance,
    )?;

    ScaledStateEmission::from_log_likelihoods(vec![lower_log_likelihood, upper_log_likelihood])
}

fn coarse_return_log_likelihood(
    observation: &MarketPressureCoarseReturnObservation,
    side_imbalance_level: f64,
    open_event_rate_per_hour: f64,
    coarse_return_law: MarketPressureCoarseReturnLaw,
    anchor_drift_per_signed_balance: f64,
) -> Result<f64, MarketPressureFitError> {
    let signed_balance_exposure =
        side_imbalance_level * open_event_rate_per_hour * observation.elapsed_hours;

    let mean_return = anchor_drift_per_signed_balance * signed_balance_exposure;
    let residual_return = observation.log_return - mean_return;

    let total_business_time_scale = open_event_rate_per_hour * observation.elapsed_hours;
    let buy_share = 0.5 * (1.0 + side_imbalance_level);
    let sell_share = 1.0 - buy_share;

    let log_likelihood = log_density_net_trading_time_return(NetTradingTimeReturnLogDensityInput {
        driver_parameters: coarse_return_law.driver_parameters,
        configuration: coarse_return_law.configuration,
        observed_return: residual_return,
        business_time_scale: total_business_time_scale,
        sell_share_proxy: sell_share,
        integrated_buy_intensity: total_business_time_scale * buy_share,
        integrated_sell_intensity: total_business_time_scale * sell_share,
        log_density_resolution: coarse_return_law.log_density_resolution,
    })
    .map_err(
        |source| MarketPressureFitError::InvalidCoarseReturnLikelihood {
            error_message: format!("{source:?}"),
        },
    )?;

    if !log_likelihood.is_finite() {
        return Err(MarketPressureFitError::InvalidObservationLikelihood {
            value: log_likelihood,
        });
    }

    Ok(log_likelihood)
}

fn penalized_coarse_return_score(fit: &CoarseReturnFit, effective_sample_count: f64) -> f64 {
    bayesian_information_score_for_effective_sample_count(
        effective_sample_count,
        fit.parameter_count,
        fit.log_likelihood,
    )
}

fn penalized_one_state_coarse_return_score(
    fit: &OneStateCoarseReturnFit,
    effective_sample_count: f64,
) -> f64 {
    bayesian_information_score_for_effective_sample_count(
        effective_sample_count,
        fit.parameter_count,
        fit.log_likelihood,
    )
}
