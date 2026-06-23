use janiform_model_kernels::market_pressure::MarketPressureTransitionRates;
use nalgebra::DMatrix;

use super::transition_rate_from_expected_count_and_exposure;
use crate::market_pressure::errors::MarketPressureFitError;

#[derive(Debug, Clone, PartialEq)]
pub(super) struct OrderedHiddenFit {
    pub(super) state_probabilities: Vec<Vec<f64>>,
    pub(super) interval_joint_probabilities: Vec<Vec<Vec<f64>>>,
    pub(super) log_likelihood: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct ScaledStateEmission {
    pub(super) likelihoods: Vec<f64>,
    pub(super) log_scale: f64,
}

pub(super) type OrderedHmmProgressObserver<'a> = &'a mut dyn FnMut(usize);

pub(super) struct OrderedHmmProgress<'a, 'b> {
    progress_checkpoints: &'a [usize],
    progress_observer: &'a mut Option<OrderedHmmProgressObserver<'b>>,
}

impl<'a, 'b> OrderedHmmProgress<'a, 'b> {
    pub(super) fn new(
        progress_checkpoints: &'a [usize],
        progress_observer: &'a mut Option<OrderedHmmProgressObserver<'b>>,
    ) -> Self {
        Self {
            progress_checkpoints,
            progress_observer,
        }
    }

    fn pass(&mut self) -> HmmPassProgress<'_, 'b> {
        HmmPassProgress::new(self.progress_checkpoints, self.progress_observer)
    }
}

impl ScaledStateEmission {
    pub(super) fn from_log_likelihoods(
        log_likelihoods: Vec<f64>,
    ) -> Result<Self, MarketPressureFitError> {
        let log_scale = log_likelihoods
            .iter()
            .copied()
            .fold(f64::NEG_INFINITY, f64::max);
        if !log_scale.is_finite() {
            return Err(MarketPressureFitError::InvalidObservationLikelihood { value: log_scale });
        }

        let likelihoods = log_likelihoods
            .iter()
            .map(|log_likelihood| (*log_likelihood - log_scale).exp())
            .collect::<Vec<_>>();

        Ok(Self {
            likelihoods,
            log_scale,
        })
    }
}

pub(super) fn forward_backward_ordered_model(
    transition_deltas: &[f64],
    emissions: &[ScaledStateEmission],
    initial_probabilities: Vec<f64>,
    transition_rates: &MarketPressureTransitionRates,
    progress: &mut OrderedHmmProgress<'_, '_>,
) -> Result<OrderedHiddenFit, MarketPressureFitError> {
    let state_count = validate_forward_backward_inputs(
        transition_deltas,
        emissions,
        initial_probabilities.as_slice(),
        transition_rates,
    )?;

    let mut forward_probabilities = Vec::with_capacity(emissions.len());
    let mut normalizers = Vec::with_capacity(emissions.len());
    let mut log_likelihood = 0.0;

    let first_emission = emissions
        .first()
        .ok_or(MarketPressureFitError::InvalidStateAssignments)?;
    let initial_unnormalized = initial_probabilities
        .iter()
        .zip(first_emission.likelihoods.iter())
        .map(|(probability, likelihood)| probability * likelihood)
        .collect::<Vec<_>>();

    let initial_normalizer = normalize_probabilities(initial_unnormalized.as_slice())?;

    log_likelihood += first_emission.log_scale + initial_normalizer.ln();
    normalizers.push(initial_normalizer);
    forward_probabilities.push(normalized_probabilities(
        initial_unnormalized,
        initial_normalizer,
    ));

    let mut forward_progress = progress.pass();

    for observation_position in 1..emissions.len() {
        let transition_matrix = ordered_transition_matrix(
            transition_rates,
            state_count,
            transition_deltas[observation_position],
        )?;

        let previous_forward = forward_probabilities
            .last()
            .ok_or(MarketPressureFitError::InvalidStateAssignments)?;

        let prior = next_state_prior(previous_forward, &transition_matrix);
        let emission = &emissions[observation_position];
        let unnormalized = prior
            .iter()
            .zip(emission.likelihoods.iter())
            .map(|(probability, likelihood)| probability * likelihood)
            .collect::<Vec<_>>();

        let normalizer = normalize_probabilities(unnormalized.as_slice())?;

        log_likelihood += emission.log_scale + normalizer.ln();
        normalizers.push(normalizer);
        forward_probabilities.push(normalized_probabilities(unnormalized, normalizer));

        forward_progress.record_completed_observation_count(observation_position + 1);
    }

    forward_progress.finish();

    let backward_probabilities = backward_probabilities(
        transition_deltas,
        emissions,
        &normalizers,
        transition_rates,
        state_count,
        progress,
    )?;

    let state_probabilities =
        state_probabilities(&forward_probabilities, backward_probabilities.as_slice())?;

    let interval_joint_probabilities = interval_joint_probabilities(
        transition_deltas,
        emissions,
        &forward_probabilities,
        backward_probabilities.as_slice(),
        transition_rates,
        state_count,
        progress,
    )?;

    Ok(OrderedHiddenFit {
        state_probabilities,
        interval_joint_probabilities,
        log_likelihood,
    })
}

struct HmmPassProgress<'a, 'b> {
    progress_checkpoints: &'a [usize],
    next_checkpoint_position: usize,
    progress_observer: &'a mut Option<OrderedHmmProgressObserver<'b>>,
}

impl<'a, 'b> HmmPassProgress<'a, 'b> {
    fn new(
        progress_checkpoints: &'a [usize],
        progress_observer: &'a mut Option<OrderedHmmProgressObserver<'b>>,
    ) -> Self {
        Self {
            progress_checkpoints,
            next_checkpoint_position: 0,
            progress_observer,
        }
    }

    fn record_completed_observation_count(&mut self, completed_observation_count: usize) {
        while self
            .progress_checkpoints
            .get(self.next_checkpoint_position)
            .is_some_and(|checkpoint| completed_observation_count >= *checkpoint)
        {
            self.emit_progress_unit();
            self.next_checkpoint_position += 1;
        }
    }

    fn finish(&mut self) {
        while self.next_checkpoint_position < self.progress_checkpoints.len() {
            self.emit_progress_unit();
            self.next_checkpoint_position += 1;
        }
    }

    fn emit_progress_unit(&mut self) {
        if let Some(progress_observer) = self.progress_observer.as_mut() {
            progress_observer(1);
        }
    }
}

pub(super) fn update_ordered_transition_rates(
    transition_deltas: &[f64],
    interval_joint_probabilities: &[Vec<Vec<f64>>],
) -> Result<MarketPressureTransitionRates, MarketPressureFitError> {
    if transition_deltas.len() != interval_joint_probabilities.len() {
        return Err(MarketPressureFitError::InvalidStateAssignments);
    }

    let Some(first_joint_probabilities) = interval_joint_probabilities.first() else {
        return Ok(MarketPressureTransitionRates::single_state());
    };

    let state_count = first_joint_probabilities.len();
    if state_count <= 1 {
        return Ok(MarketPressureTransitionRates::single_state());
    }

    let mut upward_transition_counts = vec![0.0; state_count - 1];
    let mut downward_transition_counts = vec![0.0; state_count - 1];
    let mut state_exposures = vec![0.0; state_count];

    for (interval_delta, joint_probabilities) in transition_deltas
        .iter()
        .copied()
        .zip(interval_joint_probabilities.iter())
    {
        validate_joint_probability_shape(joint_probabilities, state_count)?;

        for state_position in 0..state_count {
            let state_probability = joint_probabilities[state_position].iter().sum::<f64>();
            state_exposures[state_position] += state_probability * interval_delta;
        }

        for (previous_state_position, row) in joint_probabilities.iter().enumerate() {
            for (next_state_position, joint_probability) in row.iter().copied().enumerate() {
                if previous_state_position < next_state_position {
                    for transition_count in upward_transition_counts
                        .iter_mut()
                        .take(next_state_position)
                        .skip(previous_state_position)
                    {
                        *transition_count += joint_probability;
                    }
                } else if previous_state_position > next_state_position {
                    for transition_count in downward_transition_counts
                        .iter_mut()
                        .take(previous_state_position)
                        .skip(next_state_position)
                    {
                        *transition_count += joint_probability;
                    }
                }
            }
        }
    }

    let upward_transition_rates = upward_transition_counts
        .iter()
        .enumerate()
        .map(|(state_position, &transition_count)| {
            transition_rate_from_expected_count_and_exposure(
                transition_count,
                state_exposures[state_position],
            )
        })
        .collect::<Result<Vec<_>, _>>()?;

    let downward_transition_rates = downward_transition_counts
        .iter()
        .enumerate()
        .map(|(edge_position, &transition_count)| {
            transition_rate_from_expected_count_and_exposure(
                transition_count,
                state_exposures[edge_position + 1],
            )
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(MarketPressureTransitionRates::new(
        upward_transition_rates,
        downward_transition_rates,
    ))
}

pub(super) fn neutral_ordered_probabilities(
    support_levels: &[f64],
) -> Result<Vec<f64>, MarketPressureFitError> {
    validate_support_not_empty(support_levels)?;

    let state_count = support_levels.len();
    if state_count == 1 {
        return Ok(vec![1.0]);
    }

    if let Some(zero_state_position) = support_levels
        .iter()
        .position(|side_imbalance| *side_imbalance == 0.0)
    {
        return Ok(one_hot_probabilities(state_count, zero_state_position));
    }

    for (upper_state_position, upper_side_imbalance) in support_levels.iter().copied().enumerate() {
        if upper_side_imbalance <= 0.0 {
            continue;
        }

        if upper_state_position == 0 {
            return Ok(one_hot_probabilities(state_count, upper_state_position));
        }

        let lower_state_position = upper_state_position - 1;
        let lower_side_imbalance = support_levels[lower_state_position];
        let support_span = upper_side_imbalance - lower_side_imbalance;

        let mut probabilities = vec![0.0; state_count];
        probabilities[lower_state_position] = upper_side_imbalance / support_span;
        probabilities[upper_state_position] = -lower_side_imbalance / support_span;

        normalize_probabilities(probabilities.as_slice())?;

        return Ok(probabilities);
    }

    Ok(one_hot_probabilities(state_count, state_count - 1))
}

pub(super) fn normalize_probabilities(unnormalized: &[f64]) -> Result<f64, MarketPressureFitError> {
    let normalizer = unnormalized.iter().sum::<f64>();
    if !normalizer.is_finite() || normalizer <= 0.0 {
        return Err(MarketPressureFitError::InvalidObservationLikelihood { value: normalizer });
    }

    Ok(normalizer)
}

fn validate_forward_backward_inputs(
    transition_deltas: &[f64],
    emissions: &[ScaledStateEmission],
    initial_probabilities: &[f64],
    transition_rates: &MarketPressureTransitionRates,
) -> Result<usize, MarketPressureFitError> {
    if transition_deltas.len() != emissions.len() {
        return Err(MarketPressureFitError::InvalidStateAssignments);
    }

    let state_count = initial_probabilities.len();
    if state_count == 0 || emissions.is_empty() {
        return Err(MarketPressureFitError::InvalidStateAssignments);
    }

    for emission in emissions {
        if emission.likelihoods.len() != state_count {
            return Err(MarketPressureFitError::InvalidStateAssignments);
        }
    }

    transition_rates
        .validate_for_state_count(state_count)
        .map_err(|source| MarketPressureFitError::InvalidTransitionRates { source })?;

    normalize_probabilities(initial_probabilities)?;

    Ok(state_count)
}

fn normalized_probabilities(unnormalized: Vec<f64>, normalizer: f64) -> Vec<f64> {
    unnormalized
        .into_iter()
        .map(|probability| probability / normalizer)
        .collect()
}

fn next_state_prior(previous_probabilities: &[f64], transition_matrix: &[Vec<f64>]) -> Vec<f64> {
    let state_count = previous_probabilities.len();
    let mut prior = vec![0.0; state_count];

    for (previous_state_position, previous_probability) in
        previous_probabilities.iter().copied().enumerate()
    {
        for (next_state_position, transition_probability) in transition_matrix
            [previous_state_position]
            .iter()
            .copied()
            .enumerate()
        {
            prior[next_state_position] += previous_probability * transition_probability;
        }
    }

    prior
}

fn backward_probabilities(
    transition_deltas: &[f64],
    emissions: &[ScaledStateEmission],
    normalizers: &[f64],
    transition_rates: &MarketPressureTransitionRates,
    state_count: usize,
    progress: &mut OrderedHmmProgress<'_, '_>,
) -> Result<Vec<Vec<f64>>, MarketPressureFitError> {
    let mut backward_probabilities = vec![vec![1.0; state_count]; emissions.len()];
    let mut progress = progress.pass();

    for observation_position in (1..emissions.len()).rev() {
        let transition_matrix = ordered_transition_matrix(
            transition_rates,
            state_count,
            transition_deltas[observation_position],
        )?;

        let (previous_backward_probabilities, next_backward_probabilities) =
            backward_probabilities.split_at_mut(observation_position);
        let previous_backward = &mut previous_backward_probabilities[observation_position - 1];
        let next_backward = &next_backward_probabilities[0];
        let emission = &emissions[observation_position].likelihoods;
        let normalizer = normalizers[observation_position];

        for previous_state_position in 0..state_count {
            let mut probability = 0.0;

            for next_state_position in 0..state_count {
                probability += transition_matrix[previous_state_position][next_state_position]
                    * emission[next_state_position]
                    * next_backward[next_state_position];
            }

            previous_backward[previous_state_position] = probability / normalizer;
        }

        progress.record_completed_observation_count(emissions.len() - observation_position + 1);
    }

    progress.finish();

    Ok(backward_probabilities)
}

fn state_probabilities(
    forward_probabilities: &[Vec<f64>],
    backward_probabilities: &[Vec<f64>],
) -> Result<Vec<Vec<f64>>, MarketPressureFitError> {
    forward_probabilities
        .iter()
        .zip(backward_probabilities.iter())
        .map(|(forward, backward)| {
            let unnormalized = forward
                .iter()
                .zip(backward.iter())
                .map(|(forward_probability, backward_probability)| {
                    forward_probability * backward_probability
                })
                .collect::<Vec<_>>();

            let normalizer = normalize_probabilities(unnormalized.as_slice())?;

            Ok(normalized_probabilities(unnormalized, normalizer))
        })
        .collect()
}

fn interval_joint_probabilities(
    transition_deltas: &[f64],
    emissions: &[ScaledStateEmission],
    forward_probabilities: &[Vec<f64>],
    backward_probabilities: &[Vec<f64>],
    transition_rates: &MarketPressureTransitionRates,
    state_count: usize,
    progress: &mut OrderedHmmProgress<'_, '_>,
) -> Result<Vec<Vec<Vec<f64>>>, MarketPressureFitError> {
    let mut interval_joint_probabilities = Vec::with_capacity(emissions.len().saturating_sub(1));
    let mut progress = progress.pass();

    for observation_position in 1..emissions.len() {
        let transition_matrix = ordered_transition_matrix(
            transition_rates,
            state_count,
            transition_deltas[observation_position],
        )?;

        let previous_forward = &forward_probabilities[observation_position - 1];
        let next_backward = &backward_probabilities[observation_position];
        let emission = &emissions[observation_position].likelihoods;

        let mut unnormalized = vec![vec![0.0; state_count]; state_count];

        for previous_state_position in 0..state_count {
            for next_state_position in 0..state_count {
                unnormalized[previous_state_position][next_state_position] = previous_forward
                    [previous_state_position]
                    * transition_matrix[previous_state_position][next_state_position]
                    * emission[next_state_position]
                    * next_backward[next_state_position];
            }
        }

        let normalizer = unnormalized.iter().flatten().sum::<f64>();
        if !normalizer.is_finite() || normalizer <= 0.0 {
            return Err(MarketPressureFitError::InvalidObservationLikelihood { value: normalizer });
        }

        for row in &mut unnormalized {
            for probability in row {
                *probability /= normalizer;
            }
        }

        interval_joint_probabilities.push(unnormalized);
        progress.record_completed_observation_count(observation_position + 1);
    }

    progress.finish();

    Ok(interval_joint_probabilities)
}

fn ordered_transition_matrix(
    transition_rates: &MarketPressureTransitionRates,
    state_count: usize,
    interval_delta: f64,
) -> Result<Vec<Vec<f64>>, MarketPressureFitError> {
    transition_rates
        .validate_for_state_count(state_count)
        .map_err(|source| MarketPressureFitError::InvalidTransitionRates { source })?;

    if !interval_delta.is_finite() || interval_delta < 0.0 {
        return Err(MarketPressureFitError::InvalidTransitionExposure {
            value: interval_delta,
        });
    }

    if interval_delta == 0.0 {
        return Ok(identity_transition_matrix(state_count));
    }

    match state_count {
        0 => Err(MarketPressureFitError::InvalidStateAssignments),
        1 => Ok(vec![vec![1.0]]),
        2 => ordered_binary_transition_matrix(transition_rates, interval_delta),
        _ => ordered_matrix_exponential_transition_matrix(
            transition_rates,
            state_count,
            interval_delta,
        ),
    }
}

fn ordered_binary_transition_matrix(
    transition_rates: &MarketPressureTransitionRates,
    interval_delta: f64,
) -> Result<Vec<Vec<f64>>, MarketPressureFitError> {
    let upward_rate = transition_rates
        .upward_transition_rates
        .first()
        .copied()
        .ok_or(MarketPressureFitError::InvalidStateAssignments)?;

    let downward_rate = transition_rates
        .downward_transition_rates
        .first()
        .copied()
        .ok_or(MarketPressureFitError::InvalidStateAssignments)?;

    let total_rate = upward_rate + downward_rate;
    if total_rate == 0.0 {
        return Ok(identity_transition_matrix(2));
    }

    let persistence = (-total_rate * interval_delta).exp();

    let upward_transition_probability = upward_rate / total_rate * (1.0 - persistence);
    let downward_transition_probability = downward_rate / total_rate * (1.0 - persistence);

    Ok(vec![
        vec![
            1.0 - upward_transition_probability,
            upward_transition_probability,
        ],
        vec![
            downward_transition_probability,
            1.0 - downward_transition_probability,
        ],
    ])
}

fn ordered_matrix_exponential_transition_matrix(
    transition_rates: &MarketPressureTransitionRates,
    state_count: usize,
    interval_delta: f64,
) -> Result<Vec<Vec<f64>>, MarketPressureFitError> {
    let generator = ordered_row_generator_matrix(transition_rates, state_count)?;
    let transition = (generator * interval_delta).exp();

    let mut transition_matrix = vec![vec![0.0; state_count]; state_count];

    for row_position in 0..state_count {
        for column_position in 0..state_count {
            transition_matrix[row_position][column_position] =
                transition[(row_position, column_position)];
        }
    }

    Ok(transition_matrix)
}

fn ordered_row_generator_matrix(
    transition_rates: &MarketPressureTransitionRates,
    state_count: usize,
) -> Result<DMatrix<f64>, MarketPressureFitError> {
    transition_rates
        .validate_for_state_count(state_count)
        .map_err(|source| MarketPressureFitError::InvalidTransitionRates { source })?;

    let mut generator = DMatrix::<f64>::zeros(state_count, state_count);

    for state_position in 0..state_count {
        let mut exit_rate = 0.0;

        if state_position + 1 < state_count {
            let upward_rate = transition_rates.upward_transition_rates[state_position];
            generator[(state_position, state_position + 1)] = upward_rate;
            exit_rate += upward_rate;
        }

        if state_position > 0 {
            let downward_rate = transition_rates.downward_transition_rates[state_position - 1];
            generator[(state_position, state_position - 1)] = downward_rate;
            exit_rate += downward_rate;
        }

        generator[(state_position, state_position)] = -exit_rate;
    }

    Ok(generator)
}

fn identity_transition_matrix(state_count: usize) -> Vec<Vec<f64>> {
    let mut transition_matrix = vec![vec![0.0; state_count]; state_count];

    for (state_position, row) in transition_matrix.iter_mut().enumerate() {
        row[state_position] = 1.0;
    }

    transition_matrix
}

fn validate_joint_probability_shape(
    joint_probabilities: &[Vec<f64>],
    state_count: usize,
) -> Result<(), MarketPressureFitError> {
    if joint_probabilities.len() != state_count {
        return Err(MarketPressureFitError::InvalidStateAssignments);
    }

    for row in joint_probabilities {
        if row.len() != state_count {
            return Err(MarketPressureFitError::InvalidStateAssignments);
        }
    }

    Ok(())
}

fn validate_support_not_empty(support_levels: &[f64]) -> Result<(), MarketPressureFitError> {
    if support_levels.is_empty() {
        return Err(MarketPressureFitError::InvalidStateAssignments);
    }

    Ok(())
}

fn one_hot_probabilities(state_count: usize, state_position: usize) -> Vec<f64> {
    let mut probabilities = vec![0.0; state_count];
    probabilities[state_position] = 1.0;

    probabilities
}
