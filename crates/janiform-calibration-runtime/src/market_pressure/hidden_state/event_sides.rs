use janiform_calibration_artifacts::CalibrationOptimizerSettings;
use janiform_foundations::cancellation::CancellationToken;
use janiform_model_kernels::market_pressure::MarketPressureTransitionRates;
use janiform_work_scheduler::{StaticWorkPoolInput, run_static_work_pool};

use super::hmm::{
    OrderedHmmProgress, OrderedHmmProgressObserver, ScaledStateEmission,
    forward_backward_ordered_model, neutral_ordered_probabilities, update_ordered_transition_rates,
};
use super::{
    MarketPressureFitProgressObserver, SidePredictionFit, emit_market_pressure_fit_progress,
    validate_side_imbalance_level, validate_strictly_increasing_support,
};
use crate::market_pressure::errors::MarketPressureFitError;
use crate::market_pressure::fit::MarketPressureSideBalanceObservation;
use crate::stages::parallelism::{
    ContiguousWorkChunk, bounded_stage_worker_count, build_stage_thread_pool,
    contiguous_work_chunks,
};

const ORDERED_SIDE_MODEL_FORWARD_BACKWARD_PASS_COUNT: usize = 3;
const ORDERED_SIDE_MODEL_EMISSION_PASS_COUNT: usize = 1;

pub(super) fn fit_ordered_side_balance_model(
    observations: &[MarketPressureSideBalanceObservation],
    initial_support_levels: Vec<f64>,
    initial_transition_rates: MarketPressureTransitionRates,
    solver_settings: &CalibrationOptimizerSettings,
    progress_observer: &mut Option<MarketPressureFitProgressObserver<'_>>,
) -> Result<SidePredictionFit, MarketPressureFitError> {
    let mut support_levels = initial_support_levels;
    let mut transition_rates = initial_transition_rates;

    let transition_deltas = side_observation_transition_deltas(observations);
    let interval_transition_deltas = side_observation_interval_transition_deltas(observations);
    let progress_chunks =
        ordered_side_model_progress_chunks(observations.len(), solver_settings.cpu_count);
    let progress_checkpoints = progress_chunks
        .iter()
        .map(|chunk| chunk.end_position)
        .collect::<Vec<_>>();
    let progress_total_units =
        ordered_side_model_progress_total_units(observations.len(), solver_settings);
    let emission_worker_count =
        bounded_stage_worker_count(solver_settings.cpu_count, progress_chunks.len());
    let emission_thread_pool = (emission_worker_count > 1)
        .then(|| build_stage_thread_pool(solver_settings.cpu_count))
        .flatten();
    let forward_backward_context = OrderedSideForwardBackwardContext {
        progress_chunks: progress_chunks.as_slice(),
        progress_checkpoints: progress_checkpoints.as_slice(),
        emission_thread_pool: emission_thread_pool.as_deref(),
        emission_worker_count,
    };

    let mut progress = OrderedSideModelProgress::new(progress_observer, progress_total_units);
    progress.emit_current();

    let mut fit = forward_backward_ordered_side_model(
        observations,
        support_levels.as_slice(),
        &transition_rates,
        transition_deltas.as_slice(),
        &forward_backward_context,
        &mut progress,
    )?;

    for _iteration in 0..solver_settings.max_iterations {
        support_levels = update_ordered_support(observations, &fit.state_probabilities)?;

        transition_rates = update_ordered_transition_rates(
            interval_transition_deltas.as_slice(),
            &fit.interval_joint_probabilities,
        )?;

        let updated_fit = forward_backward_ordered_side_model(
            observations,
            support_levels.as_slice(),
            &transition_rates,
            transition_deltas.as_slice(),
            &forward_backward_context,
            &mut progress,
        )?;

        let convergence_tolerance = solver_settings
            .absolute_tolerance
            .max(solver_settings.relative_tolerance * fit.log_likelihood.abs().max(1.0));

        let log_likelihood_improvement = updated_fit.log_likelihood - fit.log_likelihood;

        fit = updated_fit;

        if log_likelihood_improvement.abs() <= convergence_tolerance {
            break;
        }
    }

    progress.finish();

    Ok(SidePredictionFit {
        parameter_count: ordered_side_model_parameter_count(support_levels.len()),
        support_levels,
        transition_rates,
        state_probabilities: fit.state_probabilities,
        interval_joint_probabilities: fit.interval_joint_probabilities,
        log_likelihood: fit.log_likelihood,
        selection_diagnostics: Vec::new(),
    })
}

fn forward_backward_ordered_side_model(
    observations: &[MarketPressureSideBalanceObservation],
    support_levels: &[f64],
    transition_rates: &MarketPressureTransitionRates,
    transition_deltas: &[f64],
    context: &OrderedSideForwardBackwardContext<'_>,
    progress: &mut OrderedSideModelProgress<'_, '_>,
) -> Result<SidePredictionFit, MarketPressureFitError> {
    validate_strictly_increasing_support(support_levels)?;

    let emissions = scaled_side_observation_emissions(
        observations,
        support_levels,
        context.progress_chunks,
        context.emission_thread_pool,
        context.emission_worker_count,
        progress,
    )?;

    let initial_probabilities = neutral_ordered_probabilities(support_levels)?;

    let mut progress_observer = |completed_units| progress.record_units(completed_units);
    let mut progress_observer: Option<OrderedHmmProgressObserver<'_>> =
        Some(&mut progress_observer);
    let mut hmm_progress =
        OrderedHmmProgress::new(context.progress_checkpoints, &mut progress_observer);

    let hidden_fit = forward_backward_ordered_model(
        transition_deltas,
        emissions.as_slice(),
        initial_probabilities,
        transition_rates,
        &mut hmm_progress,
    )?;

    // The pass result stores the transition-rate snapshot used to compute its probabilities.
    let fitted_transition_rates = transition_rates.clone();

    Ok(SidePredictionFit {
        support_levels: support_levels.to_vec(),
        transition_rates: fitted_transition_rates,
        state_probabilities: hidden_fit.state_probabilities,
        interval_joint_probabilities: hidden_fit.interval_joint_probabilities,
        log_likelihood: hidden_fit.log_likelihood,
        parameter_count: ordered_side_model_parameter_count(support_levels.len()),
        selection_diagnostics: Vec::new(),
    })
}

struct OrderedSideForwardBackwardContext<'a> {
    progress_chunks: &'a [ContiguousWorkChunk],
    progress_checkpoints: &'a [usize],
    emission_thread_pool: Option<&'a rayon::ThreadPool>,
    emission_worker_count: usize,
}

struct OrderedSideModelProgress<'a, 'b> {
    progress_observer: &'a mut Option<MarketPressureFitProgressObserver<'b>>,
    completed_units: usize,
    total_units: usize,
}

impl<'a, 'b> OrderedSideModelProgress<'a, 'b> {
    fn new(
        progress_observer: &'a mut Option<MarketPressureFitProgressObserver<'b>>,
        total_units: usize,
    ) -> Self {
        Self {
            progress_observer,
            completed_units: 0,
            total_units,
        }
    }

    fn emit_current(&mut self) {
        emit_market_pressure_fit_progress(
            self.progress_observer,
            self.completed_units,
            self.total_units,
            "fit ordered side-balance HMM",
        );
    }

    fn record_units(&mut self, completed_units: usize) {
        self.completed_units = self
            .completed_units
            .saturating_add(completed_units)
            .min(self.total_units);
        self.emit_current();
    }

    fn finish(&mut self) {
        if self.completed_units < self.total_units {
            self.completed_units = self.total_units;
            self.emit_current();
        }
    }
}

#[derive(Debug)]
struct EmissionChunkOutput {
    start_position: usize,
    result: Result<Vec<ScaledStateEmission>, MarketPressureFitError>,
}

fn update_ordered_support(
    observations: &[MarketPressureSideBalanceObservation],
    state_probabilities: &[Vec<f64>],
) -> Result<Vec<f64>, MarketPressureFitError> {
    let Some(first_state_probabilities) = state_probabilities.first() else {
        return Err(MarketPressureFitError::InvalidStateAssignments);
    };

    let state_count = first_state_probabilities.len();
    let mut weighted_side_imbalance_sums = vec![0.0; state_count];
    let mut state_weight_sums = vec![0.0; state_count];

    for (observation, probabilities) in observations.iter().zip(state_probabilities.iter()) {
        if probabilities.len() != state_count {
            return Err(MarketPressureFitError::InvalidStateAssignments);
        }

        let observation_weight = observation.signed_balance_exposure_scale;

        for state_position in 0..state_count {
            weighted_side_imbalance_sums[state_position] +=
                probabilities[state_position] * observation_weight * observation.side_imbalance;

            state_weight_sums[state_position] += probabilities[state_position] * observation_weight;
        }
    }

    if state_weight_sums
        .iter()
        .any(|state_weight_sum| *state_weight_sum <= 0.0)
    {
        return Err(MarketPressureFitError::DegenerateSideBalanceState);
    }

    let support_levels = weighted_side_imbalance_sums
        .iter()
        .zip(state_weight_sums.iter())
        .map(|(weighted_sum, weight_sum)| weighted_sum / weight_sum)
        .collect::<Vec<_>>();

    validate_strictly_increasing_support(support_levels.as_slice())?;

    Ok(support_levels)
}

fn scaled_side_observation_emission(
    observation: &MarketPressureSideBalanceObservation,
    support_levels: &[f64],
) -> Result<ScaledStateEmission, MarketPressureFitError> {
    let log_likelihoods = support_levels
        .iter()
        .copied()
        .map(|support_level| side_observation_log_likelihood(observation, support_level))
        .collect::<Result<Vec<_>, _>>()?;

    ScaledStateEmission::from_log_likelihoods(log_likelihoods)
}

fn scaled_side_observation_emissions(
    observations: &[MarketPressureSideBalanceObservation],
    support_levels: &[f64],
    progress_chunks: &[ContiguousWorkChunk],
    emission_thread_pool: Option<&rayon::ThreadPool>,
    emission_worker_count: usize,
    progress: &mut OrderedSideModelProgress<'_, '_>,
) -> Result<Vec<ScaledStateEmission>, MarketPressureFitError> {
    if emission_worker_count <= 1 {
        return scaled_side_observation_emissions_sequential(
            observations,
            support_levels,
            progress_chunks,
            progress,
        );
    }

    let mut outputs = Vec::with_capacity(progress_chunks.len());

    run_static_work_pool(
        emission_thread_pool,
        StaticWorkPoolInput::new(
            &mut outputs,
            CancellationToken::uncancelled(),
            progress_chunks.to_vec(),
            emission_worker_count,
            |chunk: ContiguousWorkChunk, _cancellation_token| EmissionChunkOutput {
                start_position: chunk.start_position,
                result: scaled_side_observation_emission_chunk(observations, support_levels, chunk),
            },
            |outputs: &mut Vec<EmissionChunkOutput>, output, _status| {
                outputs.push(output);
                progress.record_units(1);
            },
        ),
    );

    outputs.sort_by_key(|output| output.start_position);

    let mut emissions = Vec::with_capacity(observations.len());
    for output in outputs {
        emissions.extend(output.result?);
    }

    Ok(emissions)
}

fn scaled_side_observation_emissions_sequential(
    observations: &[MarketPressureSideBalanceObservation],
    support_levels: &[f64],
    progress_chunks: &[ContiguousWorkChunk],
    progress: &mut OrderedSideModelProgress<'_, '_>,
) -> Result<Vec<ScaledStateEmission>, MarketPressureFitError> {
    let mut emissions = Vec::with_capacity(observations.len());

    for chunk in progress_chunks {
        emissions.extend(scaled_side_observation_emission_chunk(
            observations,
            support_levels,
            *chunk,
        )?);
        progress.record_units(1);
    }

    Ok(emissions)
}

fn scaled_side_observation_emission_chunk(
    observations: &[MarketPressureSideBalanceObservation],
    support_levels: &[f64],
    chunk: ContiguousWorkChunk,
) -> Result<Vec<ScaledStateEmission>, MarketPressureFitError> {
    observations[chunk.start_position..chunk.end_position]
        .iter()
        .map(|observation| scaled_side_observation_emission(observation, support_levels))
        .collect()
}

fn ordered_side_model_progress_chunks(
    observation_count: usize,
    cpu_count: usize,
) -> Vec<ContiguousWorkChunk> {
    let worker_count = bounded_stage_worker_count(cpu_count, observation_count);

    contiguous_work_chunks(observation_count, worker_count)
}

fn ordered_side_model_progress_units_per_fit(progress_chunks: &[ContiguousWorkChunk]) -> usize {
    progress_chunks.len().saturating_mul(
        ORDERED_SIDE_MODEL_EMISSION_PASS_COUNT + ORDERED_SIDE_MODEL_FORWARD_BACKWARD_PASS_COUNT,
    )
}

pub(super) fn ordered_side_model_progress_total_units(
    observation_count: usize,
    solver_settings: &CalibrationOptimizerSettings,
) -> usize {
    let progress_chunks =
        ordered_side_model_progress_chunks(observation_count, solver_settings.cpu_count);
    let progress_units_per_fit = ordered_side_model_progress_units_per_fit(&progress_chunks);

    solver_settings
        .max_iterations
        .saturating_add(1)
        .max(1)
        .saturating_mul(progress_units_per_fit.max(1))
}

fn side_observation_transition_deltas(
    observations: &[MarketPressureSideBalanceObservation],
) -> Vec<f64> {
    observations
        .iter()
        .map(|observation| observation.elapsed_same_session_hours)
        .collect()
}

fn side_observation_interval_transition_deltas(
    observations: &[MarketPressureSideBalanceObservation],
) -> Vec<f64> {
    observations
        .iter()
        .skip(1)
        .map(|observation| observation.elapsed_same_session_hours)
        .collect()
}

pub(super) fn side_observation_log_likelihood(
    observation: &MarketPressureSideBalanceObservation,
    side_imbalance_level: f64,
) -> Result<f64, MarketPressureFitError> {
    validate_side_imbalance_level(side_imbalance_level)?;

    if observation.signed_balance_exposure_scale == 0.0 {
        return Ok(0.0);
    }

    let buy_probability = 0.5 * (1.0 + side_imbalance_level);
    let sell_probability = 1.0 - buy_probability;

    let buy_share = 0.5 * (1.0 + observation.side_imbalance);
    let sell_share = 1.0 - buy_share;

    let log_likelihood = observation.signed_balance_exposure_scale
        * (weighted_log_probability(buy_share, buy_probability)?
            + weighted_log_probability(sell_share, sell_probability)?);

    if log_likelihood.is_nan() {
        return Err(MarketPressureFitError::InvalidObservationLikelihood {
            value: log_likelihood,
        });
    }

    Ok(log_likelihood)
}

fn weighted_log_probability(
    probability_weight: f64,
    probability: f64,
) -> Result<f64, MarketPressureFitError> {
    if probability_weight == 0.0 {
        return Ok(0.0);
    }

    if !probability.is_finite() || !(0.0..=1.0).contains(&probability) {
        return Err(MarketPressureFitError::InvalidObservationLikelihood { value: probability });
    }

    if probability == 0.0 {
        return Ok(f64::NEG_INFINITY);
    }

    Ok(probability_weight * probability.ln())
}

pub(super) fn ordered_side_model_parameter_count(state_count: usize) -> usize {
    state_count + state_count.saturating_sub(1) * 2
}
