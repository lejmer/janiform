use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::mpsc::Sender;

use janiform_calibration_artifacts::{
    CalibrationOptimizerSettings, MeanReversionStagePredictiveEvidenceDiagnostics,
};
use janiform_foundations::cancellation::CancellationToken;
use janiform_work_scheduler::{DynamicWorkPoolStatus, StaticWorkPoolInput, run_static_work_pool};

use super::super::{
    PREDICTIVE_EVIDENCE_GATE_TOO_THIN_STATUS, PREDICTIVE_EVIDENCE_REFIT_SCHEDULE_POLICY,
    PredictiveEvidenceRecord,
};
use crate::stages::mean_reversion::evidence_statistics::{
    minimum_supported_sign_test_block_count, one_sided_exact_sign_test_p_value,
    random_walk_innovation_dependence_block_length, random_walk_residuals, signed_block_counts,
    validation_block_improvements,
};
use crate::stages::mean_reversion::{
    FLAT_ANCHOR_MEAN_REVERSION_CANDIDATE_FAMILY, FULL_PRESSURE_TO_LEVEL_CANDIDATE_FAMILY,
    MeanReversionGridCandidateEvaluation, MeanReversionStageCandidateEvaluation,
    MeanReversionStageExecutionFailure, MeanReversionStageProgressUpdate,
    OBSERVED_PRESSURE_REPLAY_CANDIDATE_FAMILY, candidate_supports_quasi_likelihood_model_selection,
    materialize_pressure_state_path, observed_pressure_replay,
    observed_pressure_replay_interval_deltas, profile_flat_anchor_mean_reversion_candidate,
    profile_full_pressure_to_level_candidate,
};
use crate::stages::parallelism::{bounded_stage_worker_count, build_stage_thread_pool};

use super::jobs::{PredictiveEvidenceJob, predictive_evidence_jobs};
use super::progress::{
    complete_predictive_evidence_progress_unit, emit_predictive_evidence_progress,
};
use super::schedule::{
    PredictiveValidationWindow, minimum_training_observation_count, predictive_validation_window,
    rolling_origin_refit_training_observation_counts,
};

struct PredictiveEvidencePoolState {
    records: Vec<PredictiveEvidenceRecord>,
    failure: Option<MeanReversionStageExecutionFailure>,
}

pub(in crate::stages::mean_reversion::evidence) fn predictive_evidence_records(
    grid_evaluations: &[MeanReversionGridCandidateEvaluation],
    solver_settings: &CalibrationOptimizerSettings,
    predictive_evidence_alpha: f64,
    progress_sender: Option<&Sender<MeanReversionStageProgressUpdate>>,
) -> Result<Vec<PredictiveEvidenceRecord>, MeanReversionStageExecutionFailure> {
    let jobs = predictive_evidence_jobs(grid_evaluations);
    let total_jobs = jobs.len();

    let total_progress_units = jobs
        .iter()
        .map(|job| job.progress_unit_count)
        .sum::<usize>();

    emit_predictive_evidence_progress(
        progress_sender,
        0,
        total_progress_units,
        Some("start predictive evidence"),
        None,
        None,
    );

    if jobs.is_empty() {
        return Ok(Vec::new());
    }

    let thread_pool = build_stage_thread_pool(solver_settings.cpu_count);
    let worker_count = bounded_stage_worker_count(solver_settings.cpu_count, total_jobs);

    let progress_counter = Arc::new(AtomicUsize::new(0));

    let mut pool_state = PredictiveEvidencePoolState {
        records: Vec::with_capacity(total_jobs),
        failure: None,
    };

    let progress_counter_for_jobs = Arc::clone(&progress_counter);

    run_static_work_pool(
        thread_pool.as_deref(),
        StaticWorkPoolInput::new(
            &mut pool_state,
            CancellationToken::uncancelled(),
            jobs,
            worker_count,
            |job: PredictiveEvidenceJob<'_>, _cancellation_token| {
                let diagnostics = evaluate_predictive_evidence(PredictiveEvidenceEvaluationInput {
                    grid_evaluation: job.grid_evaluation,
                    candidate: job.candidate,
                    solver_settings,
                    predictive_evidence_alpha,
                    progress_sender,
                    progress_label: job.progress_label.as_str(),
                    progress_counter: &progress_counter_for_jobs,
                    total_progress_units,
                });

                diagnostics.map(|diagnostics| PredictiveEvidenceRecord {
                    grid_index: job.grid_index,
                    candidate_index: job.candidate_index,
                    diagnostics,
                })
            },
            |state: &mut PredictiveEvidencePoolState,
             result: Result<PredictiveEvidenceRecord, MeanReversionStageExecutionFailure>,
             _status: DynamicWorkPoolStatus| {
                match result {
                    Ok(record) => state.records.push(record),
                    Err(failure) if state.failure.is_none() => state.failure = Some(failure),
                    Err(_) => {}
                }
            },
        ),
    );

    if let Some(failure) = pool_state.failure {
        return Err(failure);
    }

    let mut evidence_records = pool_state.records;

    if evidence_records.len() != total_jobs {
        return Err(MeanReversionStageExecutionFailure::InvalidRunSpec {
            error_message: "mean-reversion predictive evidence comparison failed".to_string(),
        });
    }

    evidence_records.sort_by_key(|record| (record.grid_index, record.candidate_index));

    Ok(evidence_records)
}

struct PredictiveValidationAccumulator {
    transition_improvements: Vec<f64>,
    random_walk_validation_loss: f64,
    candidate_validation_loss: f64,
}

impl PredictiveValidationAccumulator {
    fn new() -> Self {
        Self {
            transition_improvements: Vec::new(),
            random_walk_validation_loss: 0.0,
            candidate_validation_loss: 0.0,
        }
    }

    fn record_transition(
        &mut self,
        current_observation: f64,
        next_observation: f64,
        prediction: f64,
    ) {
        let random_walk_error = next_observation - current_observation;
        let candidate_error = next_observation - prediction;
        let random_walk_loss = random_walk_error * random_walk_error;
        let candidate_loss = candidate_error * candidate_error;

        self.random_walk_validation_loss += random_walk_loss;
        self.candidate_validation_loss += candidate_loss;

        self.transition_improvements
            .push(random_walk_loss - candidate_loss);
    }

    fn transition_count(&self) -> usize {
        self.transition_improvements.len()
    }

    fn relative_loss_reduction(&self) -> Option<f64> {
        if self.random_walk_validation_loss <= 0.0 {
            return None;
        }

        Some(
            (self.random_walk_validation_loss - self.candidate_validation_loss)
                / self.random_walk_validation_loss,
        )
    }
}

struct PredictiveEvidenceEvaluationInput<'a> {
    grid_evaluation: &'a MeanReversionGridCandidateEvaluation,
    candidate: &'a MeanReversionStageCandidateEvaluation,
    solver_settings: &'a CalibrationOptimizerSettings,
    predictive_evidence_alpha: f64,
    progress_sender: Option<&'a Sender<MeanReversionStageProgressUpdate>>,
    progress_label: &'a str,
    progress_counter: &'a AtomicUsize,
    total_progress_units: usize,
}

fn evaluate_predictive_evidence(
    input: PredictiveEvidenceEvaluationInput<'_>,
) -> Result<MeanReversionStagePredictiveEvidenceDiagnostics, MeanReversionStageExecutionFailure> {
    let random_walk_residuals =
        random_walk_residuals(input.grid_evaluation.observation_levels.as_slice());

    let validation_block_length =
        random_walk_innovation_dependence_block_length(random_walk_residuals.as_slice());

    let refit_training_observation_counts = rolling_origin_refit_training_observation_counts(
        input.grid_evaluation.observation_levels.len(),
        minimum_training_observation_count(input.candidate.candidate_family),
    );

    let mut validation_accumulator = PredictiveValidationAccumulator::new();

    let total_refit_count = refit_training_observation_counts.len();

    for (refit_position, training_observation_count) in refit_training_observation_counts
        .iter()
        .copied()
        .enumerate()
    {
        if let Some(validation_window) = predictive_validation_window(
            refit_training_observation_counts.as_slice(),
            refit_position,
            training_observation_count,
            input.grid_evaluation.observation_levels.len(),
        ) {
            evaluate_refit_validation_window(
                input.grid_evaluation,
                input.candidate,
                input.solver_settings,
                &validation_window,
                &mut validation_accumulator,
            )?;
        }

        complete_predictive_evidence_progress_unit(
            input.progress_sender,
            input.progress_counter,
            input.total_progress_units,
            input.progress_label,
            refit_position.saturating_add(1),
            total_refit_count,
        );
    }

    if total_refit_count == 0 {
        complete_predictive_evidence_progress_unit(
            input.progress_sender,
            input.progress_counter,
            input.total_progress_units,
            input.progress_label,
            0,
            0,
        );
    }

    build_predictive_evidence_diagnostics(
        validation_accumulator,
        validation_block_length,
        total_refit_count,
        input.predictive_evidence_alpha,
    )
}

fn evaluate_refit_validation_window(
    grid_evaluation: &MeanReversionGridCandidateEvaluation,
    candidate: &MeanReversionStageCandidateEvaluation,
    solver_settings: &CalibrationOptimizerSettings,
    validation_window: &PredictiveValidationWindow,
    validation_accumulator: &mut PredictiveValidationAccumulator,
) -> Result<(), MeanReversionStageExecutionFailure> {
    let Some(training_observation_levels) = grid_evaluation
        .observation_levels
        .get(..validation_window.training_observation_count)
    else {
        return Ok(());
    };

    let Some(training_candidate) = fit_training_candidate(
        grid_evaluation,
        candidate.candidate_family,
        training_observation_levels,
        solver_settings,
    )?
    else {
        return Ok(());
    };

    if !candidate_supports_quasi_likelihood_model_selection(&training_candidate) {
        return Ok(());
    }

    if training_candidate.candidate_family == OBSERVED_PRESSURE_REPLAY_CANDIDATE_FAMILY {
        return evaluate_observed_pressure_replay_refit_validation_window(
            grid_evaluation,
            &training_candidate,
            validation_window,
            validation_accumulator,
        );
    }

    let pressure_state_path = prediction_pressure_state_path(
        grid_evaluation,
        &training_candidate,
        validation_window.end_transition_index_exclusive,
    )?;

    for validation_transition_index in
        validation_window.start_transition_index..validation_window.end_transition_index_exclusive
    {
        record_validation_transition_prediction(
            grid_evaluation,
            &training_candidate,
            validation_transition_index,
            pressure_state_path.as_deref(),
            validation_accumulator,
        )?;
    }

    Ok(())
}

fn record_validation_transition_prediction(
    grid_evaluation: &MeanReversionGridCandidateEvaluation,
    training_candidate: &MeanReversionStageCandidateEvaluation,
    validation_transition_index: usize,
    pressure_state_path: Option<&[f64]>,
    validation_accumulator: &mut PredictiveValidationAccumulator,
) -> Result<(), MeanReversionStageExecutionFailure> {
    let Some(candidate_prediction) = predict_next_observation(
        grid_evaluation,
        training_candidate,
        validation_transition_index,
        pressure_state_path,
    )?
    else {
        return Ok(());
    };

    let Some(current_observation) = grid_evaluation
        .observation_levels
        .get(validation_transition_index)
        .copied()
    else {
        return Ok(());
    };

    let Some(next_observation) = grid_evaluation
        .observation_levels
        .get(validation_transition_index.saturating_add(1))
        .copied()
    else {
        return Ok(());
    };

    validation_accumulator.record_transition(
        current_observation,
        next_observation,
        candidate_prediction,
    );

    Ok(())
}

fn evaluate_observed_pressure_replay_refit_validation_window(
    grid_evaluation: &MeanReversionGridCandidateEvaluation,
    training_candidate: &MeanReversionStageCandidateEvaluation,
    validation_window: &PredictiveValidationWindow,
    validation_accumulator: &mut PredictiveValidationAccumulator,
) -> Result<(), MeanReversionStageExecutionFailure> {
    let Some(current_observation) = grid_evaluation
        .observation_levels
        .get(validation_window.start_transition_index)
        .copied()
    else {
        return Ok(());
    };

    let mut boundary_state =
        observed_pressure_replay::observed_pressure_replay_boundary_state_at_candidate_end(
            training_candidate,
            current_observation,
        )?;

    for validation_transition_index in
        validation_window.start_transition_index..validation_window.end_transition_index_exclusive
    {
        let Some(current_observation) = grid_evaluation
            .observation_levels
            .get(validation_transition_index)
            .copied()
        else {
            return Ok(());
        };

        let Some(next_observation) = grid_evaluation
            .observation_levels
            .get(validation_transition_index.saturating_add(1))
            .copied()
        else {
            return Ok(());
        };

        let interval_delta =
            validation_interval_delta(grid_evaluation, validation_transition_index)?;

        let candidate_prediction =
            observed_pressure_replay::predict_causal_observed_pressure_replay_next_observation(
                &boundary_state,
                training_candidate,
                interval_delta,
            )?;

        validation_accumulator.record_transition(
            current_observation,
            next_observation,
            candidate_prediction,
        );

        boundary_state =
            observed_pressure_replay::update_causal_observed_pressure_replay_boundary_state(
                boundary_state,
                training_candidate,
                interval_delta,
                next_observation,
            )?;
    }

    Ok(())
}

fn build_predictive_evidence_diagnostics(
    validation_accumulator: PredictiveValidationAccumulator,
    validation_block_length: usize,
    total_refit_count: usize,
    predictive_evidence_alpha: f64,
) -> Result<MeanReversionStagePredictiveEvidenceDiagnostics, MeanReversionStageExecutionFailure> {
    let block_improvements = validation_block_improvements(
        validation_accumulator.transition_improvements.as_slice(),
        validation_block_length,
    );

    let block_counts = signed_block_counts(block_improvements.as_slice());
    let signed_block_count = block_counts.positive_block_count + block_counts.negative_block_count;

    let minimum_supported_block_count =
        minimum_supported_sign_test_block_count(predictive_evidence_alpha);

    let raw_sign_test_p_value = if signed_block_count >= minimum_supported_block_count {
        Some(one_sided_exact_sign_test_p_value(
            block_counts.positive_block_count,
            signed_block_count,
        ))
    } else {
        None
    };

    Ok(MeanReversionStagePredictiveEvidenceDiagnostics {
        gate_status: PREDICTIVE_EVIDENCE_GATE_TOO_THIN_STATUS.to_string(),
        significance_level: predictive_evidence_alpha,
        validation_refit_schedule_policy: PREDICTIVE_EVIDENCE_REFIT_SCHEDULE_POLICY.to_string(),
        validation_refit_count: total_refit_count,
        validation_transition_count: validation_accumulator.transition_count(),
        validation_block_length,
        validation_block_count: block_improvements.len(),
        positive_block_count: block_counts.positive_block_count,
        negative_block_count: block_counts.negative_block_count,
        tied_block_count: block_counts.tied_block_count,
        random_walk_validation_loss: Some(validation_accumulator.random_walk_validation_loss),
        candidate_validation_loss: Some(validation_accumulator.candidate_validation_loss),
        relative_loss_reduction: validation_accumulator.relative_loss_reduction(),
        raw_sign_test_p_value,
        holm_adjusted_p_value: None,
    })
}

fn fit_training_candidate(
    grid_evaluation: &MeanReversionGridCandidateEvaluation,
    candidate_family: &str,
    training_observation_levels: &[f64],
    solver_settings: &CalibrationOptimizerSettings,
) -> Result<Option<MeanReversionStageCandidateEvaluation>, MeanReversionStageExecutionFailure> {
    match candidate_family {
        FULL_PRESSURE_TO_LEVEL_CANDIDATE_FAMILY => {
            let Some(calibration_context) = grid_evaluation.calibration_context.as_ref() else {
                return Ok(None);
            };

            let training_transition_count = training_observation_levels.len().saturating_sub(1);

            let Some(training_core_expected_increments) = calibration_context
                .core_expected_increments
                .get(..training_transition_count)
            else {
                return Ok(None);
            };

            let Some(training_interval_deltas) = calibration_context
                .interval_deltas
                .get(..training_transition_count)
            else {
                return Ok(None);
            };

            profile_full_pressure_to_level_candidate(
                training_observation_levels,
                grid_evaluation
                    .calibration_grid
                    .calibration_grid_interval_hours,
                training_core_expected_increments,
                training_interval_deltas,
                solver_settings,
            )
            .map(Some)
        }
        FLAT_ANCHOR_MEAN_REVERSION_CANDIDATE_FAMILY => {
            profile_flat_anchor_mean_reversion_candidate(
                training_observation_levels,
                grid_evaluation
                    .calibration_grid
                    .calibration_grid_interval_hours,
                solver_settings,
            )
            .map(Some)
        }
        OBSERVED_PRESSURE_REPLAY_CANDIDATE_FAMILY => {
            let training_transition_count = training_observation_levels.len().saturating_sub(1);
            let training_interval_deltas = observed_pressure_replay_interval_deltas(
                grid_evaluation.calibration_context.as_ref(),
                grid_evaluation
                    .calibration_grid
                    .calibration_grid_interval_hours,
                grid_evaluation.observation_levels.len(),
            );

            let Some(training_interval_deltas) =
                training_interval_deltas.get(..training_transition_count)
            else {
                return Ok(None);
            };

            observed_pressure_replay::profile_causal_observed_pressure_replay_candidate(
                training_observation_levels,
                grid_evaluation
                    .calibration_grid
                    .calibration_grid_interval_hours,
                training_interval_deltas,
                solver_settings,
            )
            .map(Some)
        }
        _ => Ok(None),
    }
}

fn predict_next_observation(
    grid_evaluation: &MeanReversionGridCandidateEvaluation,
    candidate: &MeanReversionStageCandidateEvaluation,
    validation_transition_index: usize,
    pressure_state_path: Option<&[f64]>,
) -> Result<Option<f64>, MeanReversionStageExecutionFailure> {
    let Some(current_observation) = grid_evaluation
        .observation_levels
        .get(validation_transition_index)
        .copied()
    else {
        return Err(
            MeanReversionStageExecutionFailure::InvalidObservationTimestampGrid {
                error_message: format!(
                    "mean-reversion predictive validation received missing current observation at transition_index={validation_transition_index}"
                ),
            },
        );
    };

    match candidate.candidate_family {
        FULL_PRESSURE_TO_LEVEL_CANDIDATE_FAMILY => {
            let Some(pressure_state_path) = pressure_state_path else {
                return Err(
                    MeanReversionStageExecutionFailure::EndogenousAnchorMaterializationFailed {
                        error_message:
                            "mean-reversion predictive validation missing pressure state path"
                                .to_string(),
                    },
                );
            };

            predict_full_pressure_to_level_next_observation(
                grid_evaluation,
                candidate,
                validation_transition_index,
                current_observation,
                pressure_state_path,
            )
            .map(Some)
        }
        FLAT_ANCHOR_MEAN_REVERSION_CANDIDATE_FAMILY => {
            let anchor_origin_level = candidate.anchor_origin_level.unwrap_or(0.0);

            Ok(Some(
                candidate.phi_reference * current_observation
                    + (1.0 - candidate.phi_reference) * anchor_origin_level,
            ))
        }
        _ => Ok(None),
    }
}

fn prediction_pressure_state_path(
    grid_evaluation: &MeanReversionGridCandidateEvaluation,
    candidate: &MeanReversionStageCandidateEvaluation,
    validation_end_transition_index_exclusive: usize,
) -> Result<Option<Vec<f64>>, MeanReversionStageExecutionFailure> {
    if candidate.candidate_family != FULL_PRESSURE_TO_LEVEL_CANDIDATE_FAMILY {
        return Ok(None);
    }

    let Some(calibration_context) = grid_evaluation.calibration_context.as_ref() else {
        return Err(
            MeanReversionStageExecutionFailure::EndogenousAnchorMaterializationFailed {
                error_message:
                    "mean-reversion predictive validation requires endogenous calibration context"
                        .to_string(),
            },
        );
    };

    let Some(core_expected_increments) = calibration_context
        .core_expected_increments
        .get(..validation_end_transition_index_exclusive)
    else {
        return Err(
            MeanReversionStageExecutionFailure::EndogenousAnchorMaterializationFailed {
                error_message: format!(
                    "mean-reversion predictive validation missing core increments through transition_count={validation_end_transition_index_exclusive}"
                ),
            },
        );
    };

    let Some(interval_deltas) = calibration_context
        .interval_deltas
        .get(..validation_end_transition_index_exclusive)
    else {
        return Err(
            MeanReversionStageExecutionFailure::EndogenousAnchorMaterializationFailed {
                error_message: format!(
                    "mean-reversion predictive validation missing interval deltas through transition_count={validation_end_transition_index_exclusive}"
                ),
            },
        );
    };

    materialize_pressure_state_path(
        grid_evaluation
            .calibration_grid
            .calibration_grid_interval_hours,
        candidate
            .anchor_model
            .pressure_persistence_per_reference_interval,
        core_expected_increments,
        interval_deltas,
    )
    .map(Some)
}

fn validation_interval_delta(
    grid_evaluation: &MeanReversionGridCandidateEvaluation,
    validation_transition_index: usize,
) -> Result<f64, MeanReversionStageExecutionFailure> {
    if let Some(calibration_context) = grid_evaluation.calibration_context.as_ref() {
        return calibration_context
            .interval_deltas
            .get(validation_transition_index)
            .copied()
            .ok_or_else(|| {
                MeanReversionStageExecutionFailure::EndogenousAnchorMaterializationFailed {
                    error_message: format!(
                        "mean-reversion predictive validation missing interval delta for transition_index={validation_transition_index}"
                    ),
                }
            });
    }

    Ok(grid_evaluation
        .calibration_grid
        .calibration_grid_interval_hours)
}

fn predict_full_pressure_to_level_next_observation(
    grid_evaluation: &MeanReversionGridCandidateEvaluation,
    candidate: &MeanReversionStageCandidateEvaluation,
    validation_transition_index: usize,
    current_observation: f64,
    pressure_state_path: &[f64],
) -> Result<f64, MeanReversionStageExecutionFailure> {
    let Some(calibration_context) = grid_evaluation.calibration_context.as_ref() else {
        return Err(
            MeanReversionStageExecutionFailure::EndogenousAnchorMaterializationFailed {
                error_message:
                    "mean-reversion predictive validation requires endogenous calibration context"
                        .to_string(),
            },
        );
    };

    let Some(current_pressure_state) = pressure_state_path
        .get(validation_transition_index)
        .copied()
    else {
        return Err(
            MeanReversionStageExecutionFailure::EndogenousAnchorMaterializationFailed {
                error_message: format!(
                    "mean-reversion predictive validation missing current pressure state for transition_index={validation_transition_index}"
                ),
            },
        );
    };

    let Some(next_pressure_state) = pressure_state_path
        .get(validation_transition_index.saturating_add(1))
        .copied()
    else {
        return Err(
            MeanReversionStageExecutionFailure::EndogenousAnchorMaterializationFailed {
                error_message: format!(
                    "mean-reversion predictive validation missing next pressure state for transition_index={validation_transition_index}"
                ),
            },
        );
    };

    let Some(interval_delta) = calibration_context
        .interval_deltas
        .get(validation_transition_index)
        .copied()
    else {
        return Err(
            MeanReversionStageExecutionFailure::EndogenousAnchorMaterializationFailed {
                error_message: format!(
                    "mean-reversion predictive validation missing prediction interval delta for transition_index={validation_transition_index}"
                ),
            },
        );
    };

    let phi = candidate.phi_reference.powf(
        interval_delta
            / grid_evaluation
                .calibration_grid
                .calibration_grid_interval_hours,
    );

    let anchor_origin_level = candidate.anchor_origin_level.unwrap_or(0.0);

    Ok(phi * current_observation
        + (1.0 - phi) * anchor_origin_level
        + candidate.anchor_model.pressure_level_gain
            * (next_pressure_state - phi * current_pressure_state))
}
