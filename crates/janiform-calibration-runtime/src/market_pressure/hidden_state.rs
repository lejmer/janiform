use janiform_calibration_artifacts::CalibrationOptimizerSettings;
use janiform_foundations::cancellation::CancellationToken;
use janiform_foundations::numeric::usize_to_f64_exact;
use janiform_model_kernels::market_pressure::MarketPressureTransitionRates;
use janiform_work_scheduler::{
    StreamingWorkContext, StreamingWorkOutput, StreamingWorkPoolInput, StreamingWorkProgress,
    run_streaming_work_pool,
};

use super::errors::MarketPressureFitError;
use super::fit::{
    MarketPressureCoarseReturnLaw, MarketPressureCoarseReturnObservation,
    MarketPressureFitProgressObserver, MarketPressureSideBalanceObservation,
    emit_market_pressure_fit_progress,
};
use crate::stages::model_selection::bayesian_information_score_for_effective_sample_count;
use crate::stages::parallelism::bounded_stage_worker_count;
use crate::stages::return_components::ReturnComponentsStageMarketPressureSelectionDiagnostic;
use coarse_returns::fit_coarse_augmented_single_side_model;
use event_sides::{
    fit_ordered_side_balance_model, ordered_side_model_parameter_count,
    ordered_side_model_progress_total_units, side_observation_log_likelihood,
};

mod coarse_returns;
mod event_sides;
mod hmm;

#[derive(Debug, Clone, PartialEq)]
pub(super) struct HiddenSideBalanceFit {
    pub(super) support_levels: Vec<f64>,
    pub(super) transition_rates: MarketPressureTransitionRates,
    pub(super) state_probabilities: Vec<Vec<f64>>,
    pub(super) coarse_state_probabilities: Vec<Vec<f64>>,
    pub(super) selection_diagnostics: Vec<ReturnComponentsStageMarketPressureSelectionDiagnostic>,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct SidePredictionFit {
    pub(super) support_levels: Vec<f64>,
    pub(super) transition_rates: MarketPressureTransitionRates,
    pub(super) state_probabilities: Vec<Vec<f64>>,
    pub(super) interval_joint_probabilities: Vec<Vec<Vec<f64>>>,
    pub(super) log_likelihood: f64,
    pub(super) parameter_count: usize,
    pub(super) selection_diagnostics: Vec<ReturnComponentsStageMarketPressureSelectionDiagnostic>,
}

#[derive(Debug, Clone, PartialEq)]
struct OrderedStartJob {
    start_position: usize,
    transition_rates: MarketPressureTransitionRates,
}

#[derive(Debug, Clone, PartialEq)]
struct OrderedStartOutput {
    start_position: usize,
    fit_result: Result<SidePredictionFit, MarketPressureFitError>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct OrderedStartProgress {
    completed_units: usize,
    total_units: usize,
}

struct OrderedStartPoolState<'a, 'b> {
    outputs: Vec<OrderedStartOutput>,
    completed_units_by_start: Vec<usize>,
    total_units_by_start: Vec<usize>,
    progress_observer: &'a mut Option<MarketPressureFitProgressObserver<'b>>,
}

#[derive(Debug, Clone, PartialEq)]
struct StateSplitCandidate {
    state_position: usize,
    residual_score: f64,
    lower_level: f64,
    upper_level: f64,
}

pub(super) fn fit_hidden_side_balance_model(
    observations: &[MarketPressureSideBalanceObservation],
    coarse_return_observations: &[MarketPressureCoarseReturnObservation],
    coarse_return_law: Option<MarketPressureCoarseReturnLaw>,
    open_event_rate_per_hour: f64,
    solver_settings: &CalibrationOptimizerSettings,
    progress_observer: &mut Option<MarketPressureFitProgressObserver<'_>>,
) -> Result<HiddenSideBalanceFit, MarketPressureFitError> {
    emit_market_pressure_fit_progress(progress_observer, 0, 1, "fit one-state side-balance model");

    let one_state_fit = fit_one_state_side_balance_model(observations)?;

    let side_prediction_fit = fit_ordered_side_prediction_model(
        observations,
        one_state_fit,
        solver_settings,
        progress_observer,
    )?;

    hidden_side_balance_fit_from_side_prediction_fit(
        observations,
        coarse_return_observations,
        coarse_return_law,
        side_prediction_fit,
        open_event_rate_per_hour,
        solver_settings,
        progress_observer,
    )
}

pub(super) fn fit_hidden_side_balance_model_from_cached_fit(
    observations: &[MarketPressureSideBalanceObservation],
    coarse_return_observations: &[MarketPressureCoarseReturnObservation],
    coarse_return_law: Option<MarketPressureCoarseReturnLaw>,
    open_event_rate_per_hour: f64,
    cached_fit: &HiddenSideBalanceFit,
    solver_settings: &CalibrationOptimizerSettings,
    progress_observer: &mut Option<MarketPressureFitProgressObserver<'_>>,
) -> Result<HiddenSideBalanceFit, MarketPressureFitError> {
    emit_market_pressure_fit_progress(progress_observer, 0, 1, "reuse side-balance pressure fit");

    let side_prediction_fit = cached_side_prediction_fit(cached_fit)?;

    hidden_side_balance_fit_from_side_prediction_fit(
        observations,
        coarse_return_observations,
        coarse_return_law,
        side_prediction_fit,
        open_event_rate_per_hour,
        solver_settings,
        progress_observer,
    )
}

fn hidden_side_balance_fit_from_side_prediction_fit(
    observations: &[MarketPressureSideBalanceObservation],
    coarse_return_observations: &[MarketPressureCoarseReturnObservation],
    coarse_return_law: Option<MarketPressureCoarseReturnLaw>,
    side_prediction_fit: SidePredictionFit,
    open_event_rate_per_hour: f64,
    solver_settings: &CalibrationOptimizerSettings,
    progress_observer: &mut Option<MarketPressureFitProgressObserver<'_>>,
) -> Result<HiddenSideBalanceFit, MarketPressureFitError> {
    if side_prediction_fit.support_levels.len() > 1 {
        return Ok(side_prediction_fit.into_hidden_fit());
    }

    let coarse_augmented_decision = fit_coarse_augmented_single_side_model(
        observations,
        coarse_return_observations,
        coarse_return_law,
        &side_prediction_fit,
        open_event_rate_per_hour,
        solver_settings,
        progress_observer,
    )?;

    if let Some(mut coarse_augmented_fit) = coarse_augmented_decision.hidden_fit {
        let mut selection_diagnostics = side_prediction_fit.selection_diagnostics;
        selection_diagnostics.extend(coarse_augmented_decision.selection_diagnostics);
        coarse_augmented_fit.selection_diagnostics = selection_diagnostics;

        return Ok(coarse_augmented_fit);
    }

    let mut hidden_fit = side_prediction_fit.into_hidden_fit();
    hidden_fit
        .selection_diagnostics
        .extend(coarse_augmented_decision.selection_diagnostics);

    Ok(hidden_fit)
}

fn fit_ordered_side_prediction_model(
    observations: &[MarketPressureSideBalanceObservation],
    mut current_fit: SidePredictionFit,
    solver_settings: &CalibrationOptimizerSettings,
    progress_observer: &mut Option<MarketPressureFitProgressObserver<'_>>,
) -> Result<SidePredictionFit, MarketPressureFitError> {
    loop {
        emit_market_pressure_fit_progress(
            progress_observer,
            0,
            1,
            "evaluate side-balance state splits",
        );

        let effective_sample_count = side_evidence_sample_count(observations);

        let Some(split_support) = split_largest_residual_state(observations, &current_fit)? else {
            let current_score = penalized_side_prediction_score(&current_fit, effective_sample_count);
            current_fit.selection_diagnostics.push(
                market_pressure_selection_diagnostic(SelectionDiagnosticInput {
                    evidence_source: "side_balance",
                    decision: "skipped",
                    reason: "no_split_candidate",
                    current_state_count: current_fit.support_levels.len(),
                    candidate_state_count: current_fit.support_levels.len(),
                    effective_sample_count,
                    current_log_likelihood: Some(current_fit.log_likelihood),
                    candidate_log_likelihood: None,
                    current_penalized_score: Some(current_score),
                    candidate_penalized_score: None,
                    current_support_levels: current_fit.support_levels.clone(),
                    candidate_support_levels: current_fit.support_levels.clone(),
                }),
            );

            return Ok(current_fit);
        };

        let initial_rate_sets =
            initial_ordered_transition_rate_sets(observations, split_support.as_slice())?;
        let candidate_support_levels = split_support.clone();

        let Some(mut split_fit) = fit_ordered_start_candidates(
            observations,
            split_support,
            initial_rate_sets,
            solver_settings,
            progress_observer,
        )?
        else {
            current_fit.selection_diagnostics.push(
                market_pressure_selection_diagnostic(SelectionDiagnosticInput {
                    evidence_source: "side_balance",
                    decision: "rejected",
                    reason: "split_fit_failed",
                    current_state_count: current_fit.support_levels.len(),
                    candidate_state_count: candidate_support_levels.len(),
                    effective_sample_count,
                    current_log_likelihood: Some(current_fit.log_likelihood),
                    candidate_log_likelihood: None,
                    current_penalized_score: Some(penalized_side_prediction_score(
                        &current_fit,
                        effective_sample_count,
                    )),
                    candidate_penalized_score: None,
                    current_support_levels: current_fit.support_levels.clone(),
                    candidate_support_levels,
                }),
            );

            return Ok(current_fit);
        };

        emit_market_pressure_fit_progress(
            progress_observer,
            0,
            1,
            "select side-balance state split",
        );

        let current_score = penalized_side_prediction_score(&current_fit, effective_sample_count);
        let split_score = penalized_side_prediction_score(&split_fit, effective_sample_count);
        let accepted = split_score > current_score;
        let selection_diagnostic =
            market_pressure_selection_diagnostic(SelectionDiagnosticInput {
                evidence_source: "side_balance",
                decision: if accepted { "accepted" } else { "rejected" },
                reason: if accepted {
                    "penalized_score_improved"
                } else {
                    "penalized_score_not_improved"
                },
                current_state_count: current_fit.support_levels.len(),
                candidate_state_count: split_fit.support_levels.len(),
                effective_sample_count,
                current_log_likelihood: Some(current_fit.log_likelihood),
                candidate_log_likelihood: Some(split_fit.log_likelihood),
                current_penalized_score: Some(current_score),
                candidate_penalized_score: Some(split_score),
                current_support_levels: current_fit.support_levels.clone(),
                candidate_support_levels: split_fit.support_levels.clone(),
            });

        if !accepted {
            current_fit.selection_diagnostics.push(selection_diagnostic);

            return Ok(current_fit);
        }

        let mut selection_diagnostics = current_fit.selection_diagnostics;
        selection_diagnostics.push(selection_diagnostic);
        split_fit.selection_diagnostics = selection_diagnostics;
        current_fit = split_fit;
    }
}

fn fit_ordered_start_candidates(
    observations: &[MarketPressureSideBalanceObservation],
    split_support: Vec<f64>,
    initial_rate_sets: Vec<MarketPressureTransitionRates>,
    solver_settings: &CalibrationOptimizerSettings,
    progress_observer: &mut Option<MarketPressureFitProgressObserver<'_>>,
) -> Result<Option<SidePredictionFit>, MarketPressureFitError> {
    let start_jobs = initial_rate_sets
        .into_iter()
        .enumerate()
        .map(|(start_position, transition_rates)| OrderedStartJob {
            start_position,
            transition_rates,
        })
        .collect::<Vec<_>>();

    let worker_count = bounded_stage_worker_count(solver_settings.cpu_count, start_jobs.len());
    let progress_units_per_start =
        ordered_side_model_progress_total_units(observations.len(), solver_settings);

    emit_market_pressure_fit_progress(
        progress_observer,
        0,
        progress_units_per_start
            .saturating_mul(start_jobs.len())
            .max(1),
        "fit ordered side-balance HMM start candidates",
    );

    let start_outputs = if worker_count <= 1 {
        fit_ordered_start_candidates_sequential(
            observations,
            split_support,
            start_jobs,
            solver_settings,
            progress_observer,
        )
    } else {
        fit_ordered_start_candidates_parallel(
            observations,
            split_support,
            start_jobs,
            solver_settings,
            worker_count,
            progress_units_per_start,
            progress_observer,
        )?
    };

    Ok(select_best_ordered_start(start_outputs))
}

fn fit_ordered_start_candidates_sequential(
    observations: &[MarketPressureSideBalanceObservation],
    split_support: Vec<f64>,
    start_jobs: Vec<OrderedStartJob>,
    solver_settings: &CalibrationOptimizerSettings,
    progress_observer: &mut Option<MarketPressureFitProgressObserver<'_>>,
) -> Vec<OrderedStartOutput> {
    let mut outputs = Vec::with_capacity(start_jobs.len());

    for start_job in start_jobs {
        // Split support vectors are tiny model-selection candidates, so each start owns its copy.
        outputs.push(fit_ordered_start_candidate(
            observations,
            split_support.clone(),
            start_job,
            solver_settings,
            progress_observer,
        ));
    }

    outputs
}

fn fit_ordered_start_candidates_parallel(
    observations: &[MarketPressureSideBalanceObservation],
    split_support: Vec<f64>,
    start_jobs: Vec<OrderedStartJob>,
    solver_settings: &CalibrationOptimizerSettings,
    worker_count: usize,
    progress_units_per_start: usize,
    progress_observer: &mut Option<MarketPressureFitProgressObserver<'_>>,
) -> Result<Vec<OrderedStartOutput>, MarketPressureFitError> {
    let mut state = OrderedStartPoolState {
        outputs: Vec::with_capacity(start_jobs.len()),
        completed_units_by_start: vec![0; start_jobs.len()],
        total_units_by_start: vec![progress_units_per_start; start_jobs.len()],
        progress_observer,
    };

    // Split support vectors are tiny model-selection candidates, so each worker owns its copy.
    let summary = run_streaming_work_pool(StreamingWorkPoolInput::new(
        &mut state,
        CancellationToken::uncancelled(),
        start_jobs,
        worker_count,
        |start_job, context: StreamingWorkContext<OrderedStartProgress, OrderedStartOutput>| {
            fit_ordered_start_candidate_streaming(
                observations,
                split_support.clone(),
                start_job,
                solver_settings,
                context,
            )
        },
        record_ordered_start_progress,
        record_ordered_start_output,
    ))?;

    if summary.completed_work_count != summary.scheduled_work_count {
        return Err(MarketPressureFitError::InvalidStateAssignments);
    }

    Ok(state.outputs)
}

fn fit_ordered_start_candidate_streaming(
    observations: &[MarketPressureSideBalanceObservation],
    split_support: Vec<f64>,
    start_job: OrderedStartJob,
    solver_settings: &CalibrationOptimizerSettings,
    context: StreamingWorkContext<OrderedStartProgress, OrderedStartOutput>,
) -> OrderedStartOutput {
    let mut progress_observer = move |progress: super::fit::MarketPressureFitProgress| {
        let _ = context.emit_progress(OrderedStartProgress {
            completed_units: progress.completed_units,
            total_units: progress.total_units,
        });
    };
    let mut progress_observer: Option<MarketPressureFitProgressObserver<'_>> =
        Some(&mut progress_observer);

    fit_ordered_start_candidate(
        observations,
        split_support,
        start_job,
        solver_settings,
        &mut progress_observer,
    )
}

fn record_ordered_start_progress(
    state: &mut OrderedStartPoolState<'_, '_>,
    progress: StreamingWorkProgress<OrderedStartProgress>,
) -> Result<(), MarketPressureFitError> {
    record_ordered_start_progress_units(
        state,
        progress.job_position,
        progress.progress.completed_units,
        progress.progress.total_units,
    )
}

fn record_ordered_start_output(
    state: &mut OrderedStartPoolState<'_, '_>,
    output: StreamingWorkOutput<OrderedStartOutput>,
) -> Result<(), MarketPressureFitError> {
    let completed_units = state
        .total_units_by_start
        .get(output.job_position)
        .copied()
        .ok_or(MarketPressureFitError::InvalidStateAssignments)?;

    record_ordered_start_progress_units(
        state,
        output.job_position,
        completed_units,
        completed_units,
    )?;

    state.outputs.push(output.output);

    Ok(())
}

fn record_ordered_start_progress_units(
    state: &mut OrderedStartPoolState<'_, '_>,
    start_position: usize,
    completed_units: usize,
    total_units: usize,
) -> Result<(), MarketPressureFitError> {
    let Some(completed_units_slot) = state.completed_units_by_start.get_mut(start_position) else {
        return Err(MarketPressureFitError::InvalidStateAssignments);
    };

    let Some(total_units_slot) = state.total_units_by_start.get_mut(start_position) else {
        return Err(MarketPressureFitError::InvalidStateAssignments);
    };

    *total_units_slot = total_units.max(*total_units_slot).max(1);
    *completed_units_slot = completed_units.min(*total_units_slot);

    emit_ordered_start_pool_progress(state);

    Ok(())
}

fn emit_ordered_start_pool_progress(state: &mut OrderedStartPoolState<'_, '_>) {
    let completed_units = state.completed_units_by_start.iter().sum::<usize>();
    let total_units = state.total_units_by_start.iter().sum::<usize>().max(1);

    emit_market_pressure_fit_progress(
        state.progress_observer,
        completed_units.min(total_units),
        total_units,
        "fit ordered side-balance HMM start candidates",
    );
}

fn fit_ordered_start_candidate(
    observations: &[MarketPressureSideBalanceObservation],
    split_support: Vec<f64>,
    start_job: OrderedStartJob,
    solver_settings: &CalibrationOptimizerSettings,
    progress_observer: &mut Option<MarketPressureFitProgressObserver<'_>>,
) -> OrderedStartOutput {
    let fit_result = fit_ordered_side_balance_model(
        observations,
        split_support,
        start_job.transition_rates,
        solver_settings,
        progress_observer,
    );

    OrderedStartOutput {
        start_position: start_job.start_position,
        fit_result,
    }
}

fn select_best_ordered_start(start_outputs: Vec<OrderedStartOutput>) -> Option<SidePredictionFit> {
    let mut best_start: Option<(usize, SidePredictionFit)> = None;

    for start_output in start_outputs {
        let Ok(fitted_candidate) = start_output.fit_result else {
            continue;
        };

        let replace_best = best_start.as_ref().is_none_or(|(best_position, best_fit)| {
            fitted_candidate.log_likelihood > best_fit.log_likelihood
                || (fitted_candidate.log_likelihood == best_fit.log_likelihood
                    && start_output.start_position < *best_position)
        });

        if replace_best {
            best_start = Some((start_output.start_position, fitted_candidate));
        }
    }

    best_start.map(|(_start_position, fit)| fit)
}

fn fit_one_state_side_balance_model(
    observations: &[MarketPressureSideBalanceObservation],
) -> Result<SidePredictionFit, MarketPressureFitError> {
    let mut side_bucket = SignedSideBucket::default();

    for observation in observations {
        side_bucket.push(observation);
    }

    let side_imbalance_level = side_bucket.mean().unwrap_or(0.0);

    validate_side_imbalance_level(side_imbalance_level)?;

    let support_levels = vec![side_imbalance_level];
    let state_probabilities = vec![vec![1.0]; observations.len()];
    let transition_rates = MarketPressureTransitionRates::single_state();

    let log_likelihood = observations
        .iter()
        .map(|observation| side_observation_log_likelihood(observation, side_imbalance_level))
        .sum::<Result<f64, _>>()?;

    Ok(SidePredictionFit {
        support_levels,
        transition_rates,
        state_probabilities,
        interval_joint_probabilities: Vec::new(),
        log_likelihood,
        parameter_count: 1,
        selection_diagnostics: Vec::new(),
    })
}

fn split_largest_residual_state(
    observations: &[MarketPressureSideBalanceObservation],
    fit: &SidePredictionFit,
) -> Result<Option<Vec<f64>>, MarketPressureFitError> {
    let Some(split_candidate) = largest_residual_split_candidate(observations, fit)? else {
        return Ok(None);
    };

    let mut support_levels = Vec::with_capacity(fit.support_levels.len() + 1);

    for (state_position, support_level) in fit.support_levels.iter().copied().enumerate() {
        if state_position == split_candidate.state_position {
            support_levels.push(split_candidate.lower_level);
            support_levels.push(split_candidate.upper_level);
        } else {
            support_levels.push(support_level);
        }
    }

    validate_strictly_increasing_support(support_levels.as_slice())?;

    Ok(Some(support_levels))
}

fn largest_residual_split_candidate(
    observations: &[MarketPressureSideBalanceObservation],
    fit: &SidePredictionFit,
) -> Result<Option<StateSplitCandidate>, MarketPressureFitError> {
    let state_count = fit.support_levels.len();
    let mut lower_buckets = vec![SignedSideBucket::default(); state_count];
    let mut upper_buckets = vec![SignedSideBucket::default(); state_count];
    let mut residual_scores = vec![0.0; state_count];

    for (observation, probabilities) in observations.iter().zip(fit.state_probabilities.iter()) {
        if probabilities.len() != state_count {
            return Err(MarketPressureFitError::InvalidStateAssignments);
        }

        for state_position in 0..state_count {
            let state_probability = probabilities[state_position];
            let support_level = fit.support_levels[state_position];
            let weighted_exposure = state_probability * observation.signed_balance_exposure_scale;

            let residual = observation.side_imbalance - support_level;
            residual_scores[state_position] += weighted_exposure * residual * residual;

            if observation.side_imbalance <= support_level {
                lower_buckets[state_position].push_weighted(observation, state_probability);
            } else {
                upper_buckets[state_position].push_weighted(observation, state_probability);
            }
        }
    }

    let mut best_candidate = None;

    for state_position in 0..state_count {
        let Some(lower_level) = lower_buckets[state_position].mean() else {
            continue;
        };

        let Some(upper_level) = upper_buckets[state_position].mean() else {
            continue;
        };

        let candidate = StateSplitCandidate {
            state_position,
            residual_score: residual_scores[state_position],
            lower_level,
            upper_level,
        };

        if candidate.lower_level >= candidate.upper_level {
            continue;
        }

        if state_position > 0 && candidate.lower_level <= fit.support_levels[state_position - 1] {
            continue;
        }

        if state_position + 1 < state_count
            && candidate.upper_level >= fit.support_levels[state_position + 1]
        {
            continue;
        }

        let replace_best = best_candidate
            .as_ref()
            .is_none_or(|best: &StateSplitCandidate| {
                candidate.residual_score > best.residual_score
                    || (candidate.residual_score == best.residual_score
                        && candidate.state_position < best.state_position)
            });

        if replace_best {
            best_candidate = Some(candidate);
        }
    }

    Ok(best_candidate)
}

fn cached_side_prediction_fit(
    cached_fit: &HiddenSideBalanceFit,
) -> Result<SidePredictionFit, MarketPressureFitError> {
    if cached_fit.support_levels.is_empty() {
        return Err(MarketPressureFitError::InvalidStateAssignments);
    }

    // Cached refits own immutable snapshots so the returned runtime state can be reused
    // independently of the source stage payload.
    Ok(SidePredictionFit {
        support_levels: cached_fit.support_levels.clone(),
        transition_rates: cached_fit.transition_rates.clone(),
        state_probabilities: cached_fit.state_probabilities.clone(),
        interval_joint_probabilities: Vec::new(),
        log_likelihood: 0.0,
        parameter_count: ordered_side_model_parameter_count(cached_fit.support_levels.len()),
        selection_diagnostics: cached_fit.selection_diagnostics.clone(),
    })
}

fn initial_ordered_transition_rate_sets(
    observations: &[MarketPressureSideBalanceObservation],
    support_levels: &[f64],
) -> Result<Vec<MarketPressureTransitionRates>, MarketPressureFitError> {
    let persistent_rate = one_transition_per_observed_exposure_rate(observations)?;

    let transition_edge_count = support_levels.len().saturating_sub(1);
    let persistent_rates = MarketPressureTransitionRates::new(
        vec![persistent_rate; transition_edge_count],
        vec![persistent_rate; transition_edge_count],
    );

    let direct_assignment_rates = direct_assignment_transition_rates(observations, support_levels)?;

    if direct_assignment_rates == persistent_rates {
        return Ok(vec![persistent_rates]);
    }

    Ok(vec![persistent_rates, direct_assignment_rates])
}

fn one_transition_per_observed_exposure_rate(
    observations: &[MarketPressureSideBalanceObservation],
) -> Result<f64, MarketPressureFitError> {
    let observed_exposure = observations
        .iter()
        .map(|observation| observation.elapsed_same_session_hours)
        .sum::<f64>();

    if !observed_exposure.is_finite() || observed_exposure <= 0.0 {
        return Err(MarketPressureFitError::InvalidTransitionExposure {
            value: observed_exposure,
        });
    }

    Ok(1.0 / observed_exposure)
}

fn direct_assignment_transition_rates(
    observations: &[MarketPressureSideBalanceObservation],
    support_levels: &[f64],
) -> Result<MarketPressureTransitionRates, MarketPressureFitError> {
    let mut state_assignments = Vec::with_capacity(observations.len());

    for observation in observations {
        state_assignments.push(nearest_support_state_position(
            observation.side_imbalance,
            support_levels,
        )?);
    }

    estimate_transition_rates(observations, support_levels.len(), &state_assignments)
}

fn nearest_support_state_position(
    side_imbalance: f64,
    support_levels: &[f64],
) -> Result<usize, MarketPressureFitError> {
    support_levels
        .iter()
        .enumerate()
        .min_by(
            |(left_position, left_level), (right_position, right_level)| {
                let left_distance = (side_imbalance - **left_level).abs();
                let right_distance = (side_imbalance - **right_level).abs();

                left_distance
                    .total_cmp(&right_distance)
                    .then_with(|| left_position.cmp(right_position))
            },
        )
        .map(|(state_position, _support_level)| state_position)
        .ok_or(MarketPressureFitError::InvalidStateAssignments)
}

fn estimate_transition_rates(
    observations: &[MarketPressureSideBalanceObservation],
    state_count: usize,
    state_assignments: &[usize],
) -> Result<MarketPressureTransitionRates, MarketPressureFitError> {
    if state_assignments
        .iter()
        .any(|state_position| *state_position >= state_count)
    {
        return Err(MarketPressureFitError::InvalidStateAssignments);
    }

    if state_count == 1 {
        return Ok(MarketPressureTransitionRates::single_state());
    }

    let mut state_exposure_hours = vec![0.0; state_count];
    let mut upward_transition_counts = vec![0_usize; state_count - 1];
    let mut downward_transition_counts = vec![0_usize; state_count - 1];

    for (observation, &state_position) in observations.iter().zip(state_assignments.iter()) {
        state_exposure_hours[state_position] += observation.elapsed_same_session_hours;
    }

    for transition in state_assignments.windows(2) {
        let previous_state = transition[0];
        let next_state = transition[1];

        if previous_state < next_state {
            for transition_count in upward_transition_counts
                .iter_mut()
                .take(next_state)
                .skip(previous_state)
            {
                *transition_count = transition_count.saturating_add(1);
            }
        } else if previous_state > next_state {
            for transition_count in downward_transition_counts
                .iter_mut()
                .take(previous_state)
                .skip(next_state)
            {
                *transition_count = transition_count.saturating_add(1);
            }
        }
    }

    let upward_transition_rates = upward_transition_counts
        .iter()
        .enumerate()
        .map(|(state_position, &transition_count)| {
            transition_rate_from_count_and_exposure(
                transition_count,
                state_exposure_hours[state_position],
            )
        })
        .collect::<Result<Vec<_>, _>>()?;

    let downward_transition_rates = downward_transition_counts
        .iter()
        .enumerate()
        .map(|(edge_position, &transition_count)| {
            transition_rate_from_count_and_exposure(
                transition_count,
                state_exposure_hours[edge_position + 1],
            )
        })
        .collect::<Result<Vec<_>, _>>()?;

    let transition_rates =
        MarketPressureTransitionRates::new(upward_transition_rates, downward_transition_rates);

    transition_rates
        .validate_for_state_count(state_count)
        .map_err(|source| MarketPressureFitError::InvalidTransitionRates { source })?;

    Ok(transition_rates)
}

pub(super) fn validate_strictly_increasing_support(
    support_levels: &[f64],
) -> Result<(), MarketPressureFitError> {
    let mut previous_level: Option<f64> = None;

    for &support_level in support_levels {
        validate_side_imbalance_level(support_level)?;

        if previous_level.is_some_and(|previous_level| support_level <= previous_level) {
            return Err(MarketPressureFitError::NonIncreasingSupport);
        }

        previous_level = Some(support_level);
    }

    Ok(())
}

pub(super) fn validate_side_imbalance_level(
    support_level: f64,
) -> Result<(), MarketPressureFitError> {
    if !support_level.is_finite() || !(-1.0..=1.0).contains(&support_level) {
        return Err(MarketPressureFitError::InvalidSupportLevel {
            value: support_level,
        });
    }

    Ok(())
}

fn penalized_side_prediction_score(fit: &SidePredictionFit, effective_sample_count: f64) -> f64 {
    bayesian_information_score_for_effective_sample_count(
        effective_sample_count,
        fit.parameter_count,
        fit.log_likelihood,
    )
}

fn side_evidence_sample_count(observations: &[MarketPressureSideBalanceObservation]) -> f64 {
    observations
        .iter()
        .map(|observation| observation.signed_balance_exposure_scale)
        .sum()
}

pub(super) struct SelectionDiagnosticInput {
    pub(super) evidence_source: &'static str,
    pub(super) decision: &'static str,
    pub(super) reason: &'static str,
    pub(super) current_state_count: usize,
    pub(super) candidate_state_count: usize,
    pub(super) effective_sample_count: f64,
    pub(super) current_log_likelihood: Option<f64>,
    pub(super) candidate_log_likelihood: Option<f64>,
    pub(super) current_penalized_score: Option<f64>,
    pub(super) candidate_penalized_score: Option<f64>,
    pub(super) current_support_levels: Vec<f64>,
    pub(super) candidate_support_levels: Vec<f64>,
}

pub(super) fn market_pressure_selection_diagnostic(
    input: SelectionDiagnosticInput,
) -> ReturnComponentsStageMarketPressureSelectionDiagnostic {
    ReturnComponentsStageMarketPressureSelectionDiagnostic {
        evidence_source: input.evidence_source.to_string(),
        decision: input.decision.to_string(),
        reason: input.reason.to_string(),
        current_state_count: input.current_state_count,
        candidate_state_count: input.candidate_state_count,
        effective_sample_count: input.effective_sample_count,
        current_log_likelihood: input.current_log_likelihood,
        candidate_log_likelihood: input.candidate_log_likelihood,
        current_penalized_score: input.current_penalized_score,
        candidate_penalized_score: input.candidate_penalized_score,
        score_improvement: match (
            input.current_penalized_score,
            input.candidate_penalized_score,
        ) {
            (Some(current_score), Some(candidate_score)) => Some(candidate_score - current_score),
            _ => None,
        },
        current_support_levels: input.current_support_levels,
        candidate_support_levels: input.candidate_support_levels,
    }
}

fn transition_rate_from_count_and_exposure(
    transition_count: usize,
    exposure_hours: f64,
) -> Result<f64, MarketPressureFitError> {
    if transition_count == 0 {
        return Ok(0.0);
    }

    if !exposure_hours.is_finite() || exposure_hours <= 0.0 {
        return Err(MarketPressureFitError::InvalidTransitionExposure {
            value: exposure_hours,
        });
    }

    let transition_count_float =
        usize_to_f64_exact(transition_count, "market_pressure_transition_count")
            .map_err(|source| MarketPressureFitError::InvalidTransitionCount { source })?;

    transition_rate_from_expected_count_and_exposure(transition_count_float, exposure_hours)
}

pub(super) fn transition_rate_from_expected_count_and_exposure(
    transition_count: f64,
    exposure_hours: f64,
) -> Result<f64, MarketPressureFitError> {
    if !transition_count.is_finite() || transition_count < 0.0 {
        return Err(MarketPressureFitError::InvalidTransitionRate {
            value: transition_count,
        });
    }

    if transition_count == 0.0 {
        return Ok(0.0);
    }

    if !exposure_hours.is_finite() || exposure_hours <= 0.0 {
        return Err(MarketPressureFitError::InvalidTransitionExposure {
            value: exposure_hours,
        });
    }

    let transition_rate = transition_count / exposure_hours;
    if !transition_rate.is_finite() || transition_rate < 0.0 {
        return Err(MarketPressureFitError::InvalidTransitionRate {
            value: transition_rate,
        });
    }

    Ok(transition_rate)
}

impl SidePredictionFit {
    fn into_hidden_fit(self) -> HiddenSideBalanceFit {
        HiddenSideBalanceFit {
            support_levels: self.support_levels,
            transition_rates: self.transition_rates,
            state_probabilities: self.state_probabilities,
            coarse_state_probabilities: Vec::new(),
            selection_diagnostics: self.selection_diagnostics,
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq)]
struct SignedSideBucket {
    weighted_sum: f64,
    weight_sum: f64,
}

impl SignedSideBucket {
    fn push(&mut self, observation: &MarketPressureSideBalanceObservation) {
        self.push_weighted(observation, 1.0);
    }

    fn push_weighted(
        &mut self,
        observation: &MarketPressureSideBalanceObservation,
        state_probability: f64,
    ) {
        let weight = state_probability * observation.signed_balance_exposure_scale;
        self.weighted_sum += weight * observation.side_imbalance;
        self.weight_sum += weight;
    }

    fn mean(self) -> Option<f64> {
        if self.weight_sum == 0.0 {
            return None;
        }

        Some(self.weighted_sum / self.weight_sum)
    }
}
