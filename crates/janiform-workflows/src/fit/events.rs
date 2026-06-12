use std::path::Path;

use janiform_calibration_artifacts::{
    CalibrationStage, JointRefinementStageRestartEvaluationTimingDiagnostic,
    calibration_stage_token,
};
use janiform_calibration_runtime::{
    CalibrationStageProgressPhase, CalibrationStagedRunFailure, CalibrationStagedRunObserverEvent,
    CalibrationStagedRunReport, CalibrationStagedRunStageProgressEvent, HawkesStageProgressPhase,
    JointRefinementProgressCandidateAssessment, JointRefinementStageProgressPhase,
    ReturnComponentsStageProgressPhase,
};
use janiform_event_log::{
    EventDeliveryPriority, ProgressMeasure as EventProgressMeasure, RunArtifactEvent,
    RunErrorEvent, RunEvent, RunEventLogWriter, RunEventPayload, RunLifecycleEvent,
    RunMetricUpdateEvent, RunPhaseRef, RunProgressEvent, RunStageRef, WorkflowRef,
};
use janiform_terminal_ui::{
    ProgressEvent, ProgressMeasure as UiProgressMeasure, ProgressWorkItemColumn,
    ProgressWorkItemMetric,
};

use crate::error::WorkflowError;
use crate::progress::{progress_count_label, progress_position_label};

use super::labels::{fit_progress_scope_label, stage_execution_status_label, stage_label};

/// Convert a staged runtime observer event into durable run events.
pub fn run_events_from_staged_event(
    event: &CalibrationStagedRunObserverEvent,
    workflow: WorkflowRef,
    scope_id: &str,
    scope_label: &str,
    input_paths: &[String],
    output_directory: &str,
) -> Result<Vec<RunEvent>, WorkflowError> {
    let run_events = match event {
        CalibrationStagedRunObserverEvent::RunStarted {
            run_id,
            stage_sequence,
            ..
        } => vec![RunEvent::new(
            run_id.clone(),
            workflow,
            EventDeliveryPriority::Required,
            RunEventPayload::Lifecycle(RunLifecycleEvent::RunStarted {
                scope_id: scope_id.to_string(),
                scope_label: Some(scope_label.to_string()),
                inputs: input_paths.to_vec(),
                output_directory: Some(output_directory.to_string()),
                stage_sequence: stage_sequence.iter().copied().map(stage_ref).collect(),
            }),
        )],
        CalibrationStagedRunObserverEvent::StageStarted {
            run_id,
            stage,
            stage_index,
            stage_count,
        } => vec![RunEvent::new(
            run_id.clone(),
            workflow,
            EventDeliveryPriority::Required,
            RunEventPayload::Lifecycle(RunLifecycleEvent::StageStarted {
                stage: stage_ref(*stage),
                stage_index: *stage_index,
                stage_count: *stage_count,
            }),
        )],
        CalibrationStagedRunObserverEvent::StageCompleted {
            run_id,
            stage,
            stage_index,
            stage_count,
            runtime_seconds,
            ..
        } => vec![RunEvent::new(
            run_id.clone(),
            workflow,
            EventDeliveryPriority::Required,
            RunEventPayload::Lifecycle(RunLifecycleEvent::StageCompleted {
                stage: stage_ref(*stage),
                stage_index: *stage_index,
                stage_count: *stage_count,
                runtime_seconds: *runtime_seconds,
            }),
        )],
        CalibrationStagedRunObserverEvent::StageProgress(progress) => {
            let stage = stage_ref(progress.stage);

            let phase = phase_ref(
                phase_token(&progress.progress_phase),
                phase_label(&progress.progress_phase),
            );

            let mut events = vec![RunEvent::new(
                progress.run_id.clone(),
                workflow.clone(),
                EventDeliveryPriority::DroppableVolatile,
                RunEventPayload::Progress(RunProgressEvent {
                    stage: Some(stage.clone()),
                    phase: Some(phase.clone()),
                    subphase: None,
                    measures: vec![EventProgressMeasure::BoundedCount {
                        metric_id: "stage_units".to_string(),
                        label: Some("stage units".to_string()),
                        completed_units: progress.completed_units,
                        total_units: progress.total_units,
                    }],
                }),
            )];

            if let Some(timing_diagnostic) = &progress.restart_evaluation_timing_diagnostic {
                events.push(RunEvent::new(
                    progress.run_id.clone(),
                    workflow,
                    EventDeliveryPriority::Required,
                    RunEventPayload::MetricUpdate(RunMetricUpdateEvent {
                        stage: Some(stage),
                        phase: Some(phase),
                        measures: restart_evaluation_timing_measures(timing_diagnostic),
                    }),
                ));
            }

            events
        }
        CalibrationStagedRunObserverEvent::RunCompleted {
            run_id,
            total_runtime_seconds,
            ..
        } => vec![RunEvent::new(
            run_id.clone(),
            workflow,
            EventDeliveryPriority::Required,
            RunEventPayload::Lifecycle(RunLifecycleEvent::RunCompleted {
                total_runtime_seconds: *total_runtime_seconds,
            }),
        )],
        CalibrationStagedRunObserverEvent::RunFailed {
            run_id,
            total_runtime_seconds,
            failed_stage,
            failure,
            ..
        } => {
            let reason = serialized_failure_reason(failure)?;

            vec![
                RunEvent::new(
                    run_id.clone(),
                    workflow.clone(),
                    EventDeliveryPriority::Required,
                    RunEventPayload::Lifecycle(RunLifecycleEvent::RunFailed {
                        total_runtime_seconds: *total_runtime_seconds,
                        failed_stage: failed_stage.map(stage_ref),
                        failed_phase: None,
                        reason: reason.clone(),
                    }),
                ),
                RunEvent::new(
                    run_id.clone(),
                    workflow,
                    EventDeliveryPriority::Required,
                    RunEventPayload::Error(RunErrorEvent {
                        error_code: "fit_execution_failed".to_string(),
                        reason,
                        stage: failed_stage.map(stage_ref),
                        phase: None,
                        input_surface: None,
                        field: None,
                        artifact_paths: Vec::new(),
                    }),
                ),
            ]
        }
        CalibrationStagedRunObserverEvent::RunCancelled {
            run_id,
            total_runtime_seconds,
            cancelled_before_stage,
            ..
        } => vec![RunEvent::new(
            run_id.clone(),
            workflow,
            EventDeliveryPriority::Required,
            RunEventPayload::Lifecycle(RunLifecycleEvent::RunCancelled {
                total_runtime_seconds: *total_runtime_seconds,
                cancelled_before_stage: cancelled_before_stage.map(stage_ref),
            }),
        )],
    };

    Ok(run_events)
}

/// Convert a staged runtime observer event into a terminal progress event.
pub fn progress_event_from_staged_event(
    event: &CalibrationStagedRunObserverEvent,
) -> Result<Option<ProgressEvent>, WorkflowError> {
    let progress_event = match event {
        CalibrationStagedRunObserverEvent::RunStarted {
            run_id,
            stage_sequence,
            completed_stage_count,
            ..
        } => Some(ProgressEvent::RunStarted {
            run_id: run_id.clone(),
            scope: fit_progress_scope_label().to_string(),
            input_count: 1,
            output_path: None,
            stage_sequence: stage_sequence
                .iter()
                .copied()
                .map(stage_label)
                .map(str::to_string)
                .collect(),
            completed_stage_count: *completed_stage_count,
        }),
        CalibrationStagedRunObserverEvent::StageStarted {
            stage,
            stage_index,
            stage_count,
            ..
        } => Some(ProgressEvent::StageStarted {
            stage: stage_label(*stage).to_string(),
            stage_number: *stage_index,
            stage_count: *stage_count,
        }),
        CalibrationStagedRunObserverEvent::StageProgress(progress) => {
            let work_item_metrics = work_item_table_metrics(progress);
            let work_item_label = progress_work_item_label(progress, work_item_metrics.as_slice());
            let planned_work_item_labels = progress.planned_work_item_labels.clone();
            let work_item_columns = work_item_columns(progress, &work_item_label);
            let work_item_metrics = display_work_item_metrics(work_item_metrics, &work_item_label);

            let latest_signal =
                stage_progress_signal_with_metrics(progress, work_item_metrics.as_slice());

            Some(ProgressEvent::StageProgress {
                phase: phase_label(&progress.progress_phase).to_string(),
                measure: stage_progress_measure(progress)?,
                work_item_label,
                work_item_progress_percent: progress.evaluation_progress_percent,
                work_item_status: progress.current_work_item_status.clone(),
                planned_work_item_labels,
                work_item_columns,
                work_item_metrics,
                latest_signal,
            })
        }
        CalibrationStagedRunObserverEvent::StageCompleted {
            stage,
            stage_report,
            runtime_seconds,
            ..
        } => Some(ProgressEvent::StageCompleted {
            stage: stage_label(*stage).to_string(),
            status: stage_execution_status_label(stage_report.status()).to_string(),
            runtime_seconds: *runtime_seconds,
        }),
        CalibrationStagedRunObserverEvent::RunCompleted { .. } => None,
        CalibrationStagedRunObserverEvent::RunFailed { failure, .. } => {
            Some(ProgressEvent::RunFailed {
                reason: terminal_failure_reason(failure),
            })
        }
        CalibrationStagedRunObserverEvent::RunCancelled { .. } => {
            Some(ProgressEvent::RunCompleted {
                status: "cancelled".to_string(),
            })
        }
    };

    Ok(progress_event)
}

fn stage_progress_measure(
    progress: &CalibrationStagedRunStageProgressEvent,
) -> Result<UiProgressMeasure, WorkflowError> {
    if let Some(aggregate_progress_percent) = progress.aggregate_progress_percent {
        return Ok(UiProgressMeasure::percent(aggregate_progress_percent));
    }

    if progress.total_units == 0 {
        return Ok(UiProgressMeasure::Unbounded);
    }

    UiProgressMeasure::bounded(progress.completed_units, progress.total_units)
        .map_err(|source| WorkflowError::OutputProgress { source })
}

pub(crate) fn append_artifact_event(
    event_writer: &mut RunEventLogWriter,
    workflow: WorkflowRef,
    run_id: &str,
    artifact_kind: &str,
    path: &Path,
    stage: Option<CalibrationStage>,
) -> Result<(), WorkflowError> {
    event_writer
        .append_event(&RunEvent::new(
            run_id.to_string(),
            workflow,
            EventDeliveryPriority::Required,
            RunEventPayload::Artifact(RunArtifactEvent {
                artifact_kind: artifact_kind.to_string(),
                path: path.display().to_string(),
                stage: stage.map(stage_ref),
                phase: None,
                label: None,
            }),
        ))
        .map_err(|source| WorkflowError::FitEventLog { source })
}

fn restart_evaluation_timing_measures(
    diagnostic: &JointRefinementStageRestartEvaluationTimingDiagnostic,
) -> Vec<EventProgressMeasure> {
    let mut measures = vec![
        EventProgressMeasure::Text {
            metric_id: "joint_refinement.restart_eval.restart_id".to_string(),
            label: Some("restart id".to_string()),
            value: diagnostic.restart_id.clone(),
        },
        EventProgressMeasure::Text {
            metric_id: "joint_refinement.restart_eval.schedule_category".to_string(),
            label: Some("schedule category".to_string()),
            value: diagnostic.schedule_category.clone(),
        },
        EventProgressMeasure::EvaluationCount {
            metric_id: "joint_refinement.restart_eval.candidate_evaluation_count".to_string(),
            label: Some("candidate evals".to_string()),
            completed_evaluations: diagnostic.candidate_evaluation_count,
            evaluation_budget: None,
        },
        EventProgressMeasure::Text {
            metric_id: "joint_refinement.restart_eval.poll_direction_label".to_string(),
            label: Some("poll direction".to_string()),
            value: diagnostic.poll_direction_label.clone(),
        },
        EventProgressMeasure::Text {
            metric_id: "joint_refinement.restart_eval.elapsed_millis".to_string(),
            label: Some("elapsed millis".to_string()),
            value: diagnostic.elapsed_millis.to_string(),
        },
    ];

    if let Some(poll_direction_position) = diagnostic.poll_direction_position {
        measures.push(EventProgressMeasure::Text {
            metric_id: "joint_refinement.restart_eval.poll_direction_position".to_string(),
            label: Some("poll direction position".to_string()),
            value: poll_direction_position.to_string(),
        });
    }

    if let Some(objective) = diagnostic.objective {
        measures.push(EventProgressMeasure::ObjectiveValue {
            metric_id: "joint_refinement.restart_eval.objective".to_string(),
            label: Some("objective".to_string()),
            value: objective,
        });
    }

    if let Some(admissibility_blocker_count) = diagnostic.admissibility_blocker_count {
        measures.push(EventProgressMeasure::Text {
            metric_id: "joint_refinement.restart_eval.admissibility_blocker_count".to_string(),
            label: Some("admissibility blockers".to_string()),
            value: admissibility_blocker_count.to_string(),
        });
    }

    if let Some(max_standardized_residual) = diagnostic.max_standardized_residual {
        measures.push(EventProgressMeasure::Scalar {
            metric_id: "joint_refinement.restart_eval.max_standardized_residual".to_string(),
            label: Some("max standardized residual".to_string()),
            value: max_standardized_residual,
        });
    }

    if let Some(exact_closed_form_buy_count) = diagnostic.exact_closed_form_buy_count {
        measures.push(EventProgressMeasure::Text {
            metric_id: "joint_refinement.restart_eval.exact_closed_form_buy_count".to_string(),
            label: Some("CF buy count".to_string()),
            value: exact_closed_form_buy_count.to_string(),
        });
    }

    if let Some(exact_closed_form_sell_count) = diagnostic.exact_closed_form_sell_count {
        measures.push(EventProgressMeasure::Text {
            metric_id: "joint_refinement.restart_eval.exact_closed_form_sell_count".to_string(),
            label: Some("CF sell count".to_string()),
            value: exact_closed_form_sell_count.to_string(),
        });
    }

    if let Some(exact_tilted_mixed_count) = diagnostic.exact_tilted_mixed_count {
        measures.push(EventProgressMeasure::Text {
            metric_id: "joint_refinement.restart_eval.exact_tilted_mixed_count".to_string(),
            label: Some("mixed count".to_string()),
            value: exact_tilted_mixed_count.to_string(),
        });
    }

    measures
}

/// Resolve a user-facing failure reason from a staged run report.
pub fn failure_reason(report: &CalibrationStagedRunReport) -> Result<String, WorkflowError> {
    match report.failure.as_ref() {
        Some(failure) => Ok(terminal_failure_reason(failure)),
        None => Ok("staged calibration did not complete successfully".to_string()),
    }
}

/// Resolve the failed stage label for a staged run report.
pub fn failed_stage_label(report: &CalibrationStagedRunReport) -> String {
    match report.failure.as_ref() {
        Some(CalibrationStagedRunFailure::MissingStageInput { stage })
        | Some(CalibrationStagedRunFailure::MissingInMemoryUpstreamPayload {
            consuming_stage: stage,
            ..
        })
        | Some(CalibrationStagedRunFailure::StageExecutionFailed { stage, .. })
        | Some(CalibrationStagedRunFailure::StageParameterArtifactLoadFailed { stage, .. })
        | Some(CalibrationStagedRunFailure::StageParameterArtifactBuildFailed { stage, .. })
        | Some(CalibrationStagedRunFailure::StageParameterArtifactPersistFailed {
            stage, ..
        })
        | Some(CalibrationStagedRunFailure::MissingCheckpointMetadataForSuccessfulStage {
            stage,
        }) => stage_label(*stage).to_string(),
        _ => "unknown".to_string(),
    }
}

/// Resolve the latest available resume checkpoint label.
pub fn resume_availability(report: &CalibrationStagedRunReport) -> String {
    report
        .emitted_checkpoints
        .last()
        .map(|checkpoint| checkpoint.checkpoint_reference.clone())
        .unwrap_or_else(|| "not available".to_string())
}

fn serialized_failure_reason(
    failure: &CalibrationStagedRunFailure,
) -> Result<String, WorkflowError> {
    serde_json::to_string(failure).map_err(|source| WorkflowError::OutputSerialize { source })
}

fn terminal_failure_reason(failure: &CalibrationStagedRunFailure) -> String {
    match failure {
        CalibrationStagedRunFailure::InvalidRunContract { error_message } => {
            format!("invalid run contract: {error_message}")
        }
        CalibrationStagedRunFailure::MissingMemoryCheckpointStore => {
            "memory-mode resume requires an in-memory checkpoint store".to_string()
        }
        CalibrationStagedRunFailure::MissingDiskCheckpointDirectory => {
            "disk-mode execution requires a checkpoint directory".to_string()
        }
        CalibrationStagedRunFailure::MissingResumeCheckpointForRun { run_id } => {
            format!("no resume checkpoint is available for run `{run_id}`")
        }
        CalibrationStagedRunFailure::DatasetSnapshotPersistFailed {
            artifact_reference,
            error_message,
        } => format!("failed to persist dataset snapshot `{artifact_reference}`: {error_message}"),
        CalibrationStagedRunFailure::StageParameterArtifactLoadFailed {
            stage,
            artifact_reference,
            error_message,
        } => format!(
            "failed to load {} artifact `{artifact_reference}`: {error_message}",
            stage_label(*stage)
        ),
        CalibrationStagedRunFailure::StageParameterArtifactBuildFailed {
            stage,
            artifact_reference,
            error_message,
        } => format!(
            "failed to build {} artifact `{artifact_reference}`: {error_message}",
            stage_label(*stage)
        ),
        CalibrationStagedRunFailure::StageParameterArtifactPersistFailed {
            stage,
            artifact_reference,
            error_message,
        } => format!(
            "failed to persist {} artifact `{artifact_reference}`: {error_message}",
            stage_label(*stage)
        ),
        CalibrationStagedRunFailure::ArtifactManifestPersistFailed { error_message } => {
            format!("failed to persist calibration artifact manifest: {error_message}")
        }
        CalibrationStagedRunFailure::MissingStageInput { stage } => {
            format!("missing input for {}", stage_label(*stage))
        }
        CalibrationStagedRunFailure::MissingInMemoryUpstreamPayload {
            required_stage,
            consuming_stage,
        } => format!(
            "{} requires an in-memory payload from {}",
            stage_label(*consuming_stage),
            stage_label(*required_stage)
        ),
        CalibrationStagedRunFailure::InvalidCheckpointReference {
            checkpoint_reference,
            error_message,
        } => format!("invalid checkpoint reference `{checkpoint_reference}`: {error_message}"),
        CalibrationStagedRunFailure::CheckpointLoadFailed {
            checkpoint_reference,
            error_message,
        } => format!("failed to load checkpoint `{checkpoint_reference}`: {error_message}"),
        CalibrationStagedRunFailure::CheckpointCompatibilityFailed {
            checkpoint_reference,
            error_message,
        } => format!("checkpoint `{checkpoint_reference}` is not compatible: {error_message}"),
        CalibrationStagedRunFailure::CheckpointPersistFailed {
            checkpoint_reference,
            error_message,
        } => format!("failed to persist checkpoint `{checkpoint_reference}`: {error_message}"),
        CalibrationStagedRunFailure::MissingCheckpointMetadataForSuccessfulStage { stage } => {
            format!(
                "{} completed without checkpoint metadata",
                stage_label(*stage)
            )
        }
        CalibrationStagedRunFailure::StageExecutionFailed {
            stage,
            error_message,
        } => match error_message {
            Some(message) => format!("{} failed: {message}", stage_label(*stage)),
            None => format!("{} failed", stage_label(*stage)),
        },
    }
}

fn stage_progress_signal_with_metrics(
    progress: &CalibrationStagedRunStageProgressEvent,
    work_item_metrics: &[ProgressWorkItemMetric],
) -> Option<String> {
    if progress_work_item_label(progress, work_item_metrics).is_some()
        && let Some(signal) = parallel_stage_progress_signal(
            progress.active_restart_count,
            progress.valid_completed_units,
            progress.cumulative_local_evaluation_count,
            progress.best_global_loss,
            progress.best_coarse_consistency_fit,
        )
    {
        return Some(signal);
    }

    if work_item_phase_uses_row_scoped_evaluations(&progress.progress_phase)
        && progress_work_item_label(progress, work_item_metrics).is_some()
    {
        return None;
    }

    let mut parts = Vec::new();

    if progress_signal_includes_current_item_label(&progress.progress_phase)
        && let Some(current_item_label) = progress.current_item_label.as_deref()
    {
        parts.push(current_item_label.to_string());
    }

    push_evaluation_signal(
        &mut parts,
        progress.current_evaluation_count,
        progress.total_evaluation_budget,
    );

    if let Some(current_global_loss) = progress.current_global_loss {
        let score_label =
            if work_item_table_uses_restart_optimization_columns(&progress.progress_phase) {
                "loss"
            } else {
                "global loss"
            };

        parts.push(format!("{score_label} {current_global_loss:.6}"));
    }

    if let Some(best_global_loss) = progress.best_global_loss {
        parts.push(format!("best {best_global_loss:.6}"));
    }

    if let Some(current_coarse_consistency_fit) = progress.current_coarse_consistency_fit {
        parts.push(format!("coarse fit {current_coarse_consistency_fit:.6}"));
    }

    if let Some(best_coarse_consistency_fit) = progress.best_coarse_consistency_fit {
        parts.push(format!("best coarse {best_coarse_consistency_fit:.6}"));
    }

    if let Some(current_convergence_ratio) = progress.current_convergence_ratio {
        parts.push(format!("conv. {current_convergence_ratio:.6}"));
    }

    (!parts.is_empty()).then(|| parts.join(", "))
}

fn progress_signal_includes_current_item_label(phase: &CalibrationStageProgressPhase) -> bool {
    !matches!(
        phase,
        CalibrationStageProgressPhase::JointRefinement(
            JointRefinementStageProgressPhase::OptionSearchTierRefinement
                | JointRefinementStageProgressPhase::OptionSelectionTierEvaluation
                | JointRefinementStageProgressPhase::OptionAuditTierEvaluation
        )
    )
}

fn work_item_phase_uses_row_scoped_evaluations(phase: &CalibrationStageProgressPhase) -> bool {
    matches!(
        phase,
        CalibrationStageProgressPhase::JointRefinement(
            JointRefinementStageProgressPhase::OptionSearchTierRefinement
                | JointRefinementStageProgressPhase::OptionSelectionTierEvaluation
                | JointRefinementStageProgressPhase::OptionAuditTierEvaluation
        )
    )
}

fn parallel_stage_progress_signal(
    active_restart_count: Option<usize>,
    valid_completed_units: Option<usize>,
    cumulative_local_evaluation_count: Option<usize>,
    best_global_loss: Option<f64>,
    best_coarse_consistency_fit: Option<f64>,
) -> Option<String> {
    let mut parts = Vec::new();

    if let Some(active_restart_count) = active_restart_count {
        parts.push(format!("active restarts {active_restart_count}"));
    }

    if let Some(valid_completed_units) = valid_completed_units {
        parts.push(format!("valid {valid_completed_units}"));
    }

    if let Some(cumulative_local_evaluation_count) = cumulative_local_evaluation_count {
        parts.push(format!(
            "cumulative evals {cumulative_local_evaluation_count}"
        ));
    }

    if let Some(best_global_loss) = best_global_loss {
        parts.push(format!("best loss {best_global_loss:.6}"));
    }

    if let Some(best_coarse_consistency_fit) = best_coarse_consistency_fit {
        parts.push(format!("best coarse {best_coarse_consistency_fit:.6}"));
    }

    (!parts.is_empty()).then(|| parts.join(", "))
}

fn candidate_assessment_label(assessment: &JointRefinementProgressCandidateAssessment) -> String {
    match assessment {
        JointRefinementProgressCandidateAssessment::Champion => "champion",
        JointRefinementProgressCandidateAssessment::Admissible => "admissible",
        JointRefinementProgressCandidateAssessment::Inadmissible => "inadmissible",
        JointRefinementProgressCandidateAssessment::Invalid => "invalid",
    }
    .to_string()
}

fn work_item_table_columns(
    progress: &CalibrationStagedRunStageProgressEvent,
) -> Vec<ProgressWorkItemColumn> {
    let mut columns = Vec::new();

    let uses_restart_optimization_columns =
        work_item_table_uses_restart_optimization_columns(&progress.progress_phase);

    if uses_restart_optimization_columns
        || progress.current_evaluation_count.is_some()
        || progress.total_evaluation_budget.is_some()
    {
        columns.push(ProgressWorkItemColumn::new("eval", "Evals"));
    }

    if progress.current_pricing_interval_index.is_some()
        || progress.total_pricing_interval_count.is_some()
    {
        columns.push(ProgressWorkItemColumn::new("pricing_interval", "Interval"));
    }

    if progress.completed_pricing_row_count.is_some() || progress.total_pricing_row_count.is_some()
    {
        columns.push(ProgressWorkItemColumn::new("pricing_rows", "Rows"));
    }

    if uses_restart_optimization_columns || progress.current_global_loss.is_some() {
        let current_objective_label = if uses_restart_optimization_columns {
            "Trial loss"
        } else {
            "Cur. loss"
        };

        columns.push(ProgressWorkItemColumn::new(
            "current_objective",
            current_objective_label,
        ));
    }

    if uses_restart_optimization_columns || progress.current_restart_best_global_loss.is_some() {
        let best_objective_label = if uses_restart_optimization_columns {
            "Accepted loss"
        } else {
            "Best loss"
        };

        columns.push(ProgressWorkItemColumn::new(
            "best_objective",
            best_objective_label,
        ));
    }

    if uses_restart_optimization_columns || progress.current_global_loss.is_some() {
        columns.push(ProgressWorkItemColumn::new("final_objective", "Final loss"));
    }

    if progress.current_candidate_assessment.is_some() {
        columns.push(ProgressWorkItemColumn::new("assessment", "Assessment"));
    }

    if progress.current_coarse_consistency_fit.is_some() {
        columns.push(ProgressWorkItemColumn::new("coarse_fit", "Coarse fit"));
    }

    if progress.current_convergence_ratio.is_some() {
        columns.push(ProgressWorkItemColumn::new("convergence", "Conv."));
    }

    if progress.exact_closed_form_buy_count.is_some() {
        columns.push(ProgressWorkItemColumn::new("exact_buy", "CF-Buy"));
    }

    if progress.exact_closed_form_sell_count.is_some() {
        columns.push(ProgressWorkItemColumn::new("exact_sell", "CF-Sell"));
    }

    if progress.exact_tilted_mixed_count.is_some() {
        columns.push(ProgressWorkItemColumn::new("exact_mixed", "Mix"));
    }

    columns
}

fn work_item_columns(
    progress: &CalibrationStagedRunStageProgressEvent,
    work_item_label: &Option<String>,
) -> Vec<ProgressWorkItemColumn> {
    if work_item_label.is_none() && progress.planned_work_item_labels.is_empty() {
        return Vec::new();
    }

    work_item_table_columns(progress)
}

fn display_work_item_metrics(
    work_item_metrics: Vec<ProgressWorkItemMetric>,
    work_item_label: &Option<String>,
) -> Vec<ProgressWorkItemMetric> {
    if work_item_label.is_none() {
        return Vec::new();
    }

    work_item_metrics
}

fn work_item_table_uses_restart_optimization_columns(
    progress_phase: &CalibrationStageProgressPhase,
) -> bool {
    matches!(
        progress_phase,
        CalibrationStageProgressPhase::JointRefinement(
            JointRefinementStageProgressPhase::RestartOptimization
        )
    )
}

fn work_item_table_metrics(
    progress: &CalibrationStagedRunStageProgressEvent,
) -> Vec<ProgressWorkItemMetric> {
    let mut metrics = Vec::new();

    if let Some(evaluation_label) = evaluation_label(
        progress.current_evaluation_count,
        progress.total_evaluation_budget,
    ) {
        metrics.push(ProgressWorkItemMetric::new("eval", evaluation_label));
    }

    if let Some(interval_label) = pricing_interval_label(progress) {
        metrics.push(ProgressWorkItemMetric::new(
            "pricing_interval",
            interval_label,
        ));
    }

    if let Some(row_label) = progress_count_label(
        progress.completed_pricing_row_count,
        progress.total_pricing_row_count,
    ) {
        metrics.push(ProgressWorkItemMetric::new("pricing_rows", row_label));
    }

    if let Some(current_global_loss) = progress.current_global_loss {
        let metric_key = if progress.evaluation_progress_percent == Some(100) {
            "final_objective"
        } else {
            "current_objective"
        };

        metrics.push(ProgressWorkItemMetric::new(
            metric_key,
            format!("{current_global_loss:.6}"),
        ));
    }

    if let Some(current_restart_best_global_loss) = progress.current_restart_best_global_loss {
        metrics.push(ProgressWorkItemMetric::new(
            "best_objective",
            format!("{current_restart_best_global_loss:.6}"),
        ));
    }

    if let Some(current_candidate_assessment) = progress.current_candidate_assessment {
        metrics.push(ProgressWorkItemMetric::new(
            "assessment",
            candidate_assessment_label(&current_candidate_assessment),
        ));
    }

    if let Some(current_coarse_consistency_fit) = progress.current_coarse_consistency_fit {
        metrics.push(ProgressWorkItemMetric::new(
            "coarse_fit",
            format!("{current_coarse_consistency_fit:.6}"),
        ));
    }

    if let Some(current_convergence_ratio) = progress.current_convergence_ratio {
        metrics.push(ProgressWorkItemMetric::new(
            "convergence",
            format!("{current_convergence_ratio:.6}"),
        ));
    }

    if let Some(exact_closed_form_buy_count) = progress.exact_closed_form_buy_count {
        metrics.push(ProgressWorkItemMetric::new(
            "exact_buy",
            exact_closed_form_buy_count.to_string(),
        ));
    }

    if let Some(exact_closed_form_sell_count) = progress.exact_closed_form_sell_count {
        metrics.push(ProgressWorkItemMetric::new(
            "exact_sell",
            exact_closed_form_sell_count.to_string(),
        ));
    }

    if let Some(exact_tilted_mixed_count) = progress.exact_tilted_mixed_count {
        metrics.push(ProgressWorkItemMetric::new(
            "exact_mixed",
            exact_tilted_mixed_count.to_string(),
        ));
    }

    metrics
}

fn push_evaluation_signal(
    parts: &mut Vec<String>,
    current_evaluation_count: Option<usize>,
    total_evaluation_budget: Option<usize>,
) {
    if let Some(current_evaluation_count) = current_evaluation_count {
        match total_evaluation_budget {
            Some(total_evaluation_budget) => {
                parts.push(format!(
                    "evals {current_evaluation_count}/{total_evaluation_budget}"
                ));
            }
            None => parts.push(format!("evals {current_evaluation_count}")),
        }
    }
}

fn evaluation_label(
    current_evaluation_count: Option<usize>,
    total_evaluation_budget: Option<usize>,
) -> Option<String> {
    current_evaluation_count.map(|current_evaluation_count| match total_evaluation_budget {
        Some(total_evaluation_budget) => {
            format!("{current_evaluation_count}/{total_evaluation_budget}")
        }
        None => current_evaluation_count.to_string(),
    })
}

fn pricing_interval_label(progress: &CalibrationStagedRunStageProgressEvent) -> Option<String> {
    if progress.completed_pricing_row_count.is_some() || progress.total_pricing_row_count.is_some()
    {
        return progress_position_label(
            progress.current_pricing_interval_index,
            progress.total_pricing_interval_count,
        );
    }

    progress_count_label(
        progress.current_pricing_interval_index,
        progress.total_pricing_interval_count,
    )
}

fn progress_work_item_label(
    progress: &CalibrationStagedRunStageProgressEvent,
    work_item_metrics: &[ProgressWorkItemMetric],
) -> Option<String> {
    let current_item_label = progress.current_item_label.as_deref()?;

    if progress.evaluation_progress_percent.is_none() && work_item_metrics.is_empty() {
        return None;
    }

    match &progress.progress_phase {
        CalibrationStageProgressPhase::ReturnComponents(
            ReturnComponentsStageProgressPhase::ContinuousNigSeedScoring,
        )
        | CalibrationStageProgressPhase::Hawkes(
            HawkesStageProgressPhase::LikelihoodShortlistEvaluation,
        )
        | CalibrationStageProgressPhase::JointRefinement(
            JointRefinementStageProgressPhase::RestartOptimization
            | JointRefinementStageProgressPhase::CoarseConsistencyEvaluation
            | JointRefinementStageProgressPhase::OptionSearchTierRefinement
            | JointRefinementStageProgressPhase::OptionSelectionTierEvaluation
            | JointRefinementStageProgressPhase::OptionAuditTierEvaluation
            | JointRefinementStageProgressPhase::FinalSeedEvaluation,
        ) => Some(current_item_label.to_string()),
        _ => None,
    }
}

fn stage_ref(stage: CalibrationStage) -> RunStageRef {
    RunStageRef::labeled(calibration_stage_token(stage), stage_label(stage))
}

fn phase_ref(token: &str, label: &str) -> RunPhaseRef {
    RunPhaseRef::labeled(token, label)
}

fn phase_token(phase: &CalibrationStageProgressPhase) -> &'static str {
    match phase {
        CalibrationStageProgressPhase::Hawkes(HawkesStageProgressPhase::InputPreflight) => {
            "hawkes_input_preflight"
        }
        CalibrationStageProgressPhase::Hawkes(HawkesStageProgressPhase::EventTapePreparation) => {
            "hawkes_event_tape_preparation"
        }
        CalibrationStageProgressPhase::Hawkes(HawkesStageProgressPhase::CandidatePreparation) => {
            "hawkes_candidate_preparation"
        }
        CalibrationStageProgressPhase::Hawkes(HawkesStageProgressPhase::CandidateEvaluation) => {
            "hawkes_candidate_evaluation"
        }
        CalibrationStageProgressPhase::Hawkes(HawkesStageProgressPhase::CandidateRefinement) => {
            "hawkes_candidate_refinement"
        }
        CalibrationStageProgressPhase::Hawkes(
            HawkesStageProgressPhase::LikelihoodShortlistEvaluation,
        ) => "hawkes_likelihood_shortlist_evaluation",
        CalibrationStageProgressPhase::Hawkes(HawkesStageProgressPhase::CandidateSelection) => {
            "hawkes_candidate_selection"
        }
        CalibrationStageProgressPhase::Hawkes(HawkesStageProgressPhase::ReportAssembly) => {
            "hawkes_report_assembly"
        }
        CalibrationStageProgressPhase::ReturnComponents(
            ReturnComponentsStageProgressPhase::ContinuousNigProxyEmSeedGeneration,
        ) => "continuous_nig_proxy_em",
        CalibrationStageProgressPhase::ReturnComponents(
            ReturnComponentsStageProgressPhase::ContinuousNigBasinOptimization,
        ) => "continuous_nig_basin_optimization",
        CalibrationStageProgressPhase::JointRefinement(
            JointRefinementStageProgressPhase::RestartOptimization,
        ) => "restart_optimization",
        CalibrationStageProgressPhase::JointRefinement(
            JointRefinementStageProgressPhase::StageInputResolution,
        ) => "stage_input_resolution",
        CalibrationStageProgressPhase::JointRefinement(
            JointRefinementStageProgressPhase::ResumeStateResolution,
        ) => "resume_state_resolution",
        CalibrationStageProgressPhase::JointRefinement(
            JointRefinementStageProgressPhase::ObservedInputPreparation,
        ) => "observed_input_preparation",
        CalibrationStageProgressPhase::JointRefinement(
            JointRefinementStageProgressPhase::ParameterizationPreparation,
        ) => "parameterization_preparation",
        CalibrationStageProgressPhase::JointRefinement(
            JointRefinementStageProgressPhase::RuntimeMaterialization,
        ) => "runtime_materialization",
        CalibrationStageProgressPhase::JointRefinement(
            JointRefinementStageProgressPhase::RuntimeContextAssembly,
        ) => "runtime_context_assembly",
        CalibrationStageProgressPhase::JointRefinement(
            JointRefinementStageProgressPhase::OptionSearchTierRefinement,
        ) => "option_search_tier_refinement",
        CalibrationStageProgressPhase::JointRefinement(
            JointRefinementStageProgressPhase::OptionSelectionTierEvaluation,
        ) => "option_selection_tier_evaluation",
        CalibrationStageProgressPhase::JointRefinement(
            JointRefinementStageProgressPhase::OptionAuditTierEvaluation,
        ) => "option_audit_tier_evaluation",
        _ => "stage_progress",
    }
}

fn phase_label(phase: &CalibrationStageProgressPhase) -> &'static str {
    match phase {
        CalibrationStageProgressPhase::Hawkes(HawkesStageProgressPhase::InputPreflight) => {
            "input preflight"
        }
        CalibrationStageProgressPhase::Hawkes(HawkesStageProgressPhase::EventTapePreparation) => {
            "event tape preparation"
        }
        CalibrationStageProgressPhase::Hawkes(HawkesStageProgressPhase::CandidatePreparation) => {
            "candidate preparation"
        }
        CalibrationStageProgressPhase::Hawkes(HawkesStageProgressPhase::CandidateEvaluation) => {
            "candidate evaluation"
        }
        CalibrationStageProgressPhase::Hawkes(HawkesStageProgressPhase::CandidateRefinement) => {
            "candidate refinement"
        }
        CalibrationStageProgressPhase::Hawkes(
            HawkesStageProgressPhase::LikelihoodShortlistEvaluation,
        ) => "signed likelihood shortlist",
        CalibrationStageProgressPhase::Hawkes(HawkesStageProgressPhase::CandidateSelection) => {
            "candidate selection"
        }
        CalibrationStageProgressPhase::Hawkes(HawkesStageProgressPhase::ReportAssembly) => {
            "report assembly"
        }
        CalibrationStageProgressPhase::ReturnComponents(
            ReturnComponentsStageProgressPhase::InputPreflight,
        ) => "input preflight",
        CalibrationStageProgressPhase::ReturnComponents(
            ReturnComponentsStageProgressPhase::CloseOpenGapPreparation,
        ) => "close-open gap preparation",
        CalibrationStageProgressPhase::ReturnComponents(
            ReturnComponentsStageProgressPhase::SignalPartitionFit,
        ) => "signal partitioning",
        CalibrationStageProgressPhase::ReturnComponents(
            ReturnComponentsStageProgressPhase::SurpriseEpisodeSelectionFit,
        ) => "surprise selection",
        CalibrationStageProgressPhase::ReturnComponents(
            ReturnComponentsStageProgressPhase::ContinuousNigMomentSeedGeneration,
        ) => "continuous NIG moment seed generation",
        CalibrationStageProgressPhase::ReturnComponents(
            ReturnComponentsStageProgressPhase::ContinuousNigProxyEmSeedGeneration,
        ) => "continuous NIG proxy EM",
        CalibrationStageProgressPhase::ReturnComponents(
            ReturnComponentsStageProgressPhase::ContinuousNigSeedScoring,
        ) => "continuous NIG seed scoring",
        CalibrationStageProgressPhase::ReturnComponents(
            ReturnComponentsStageProgressPhase::ContinuousNigSearchRegimeGeneration,
        ) => "search regime generation",
        CalibrationStageProgressPhase::ReturnComponents(
            ReturnComponentsStageProgressPhase::ContinuousNigBasinSeedGeneration,
        ) => "basin seed generation",
        CalibrationStageProgressPhase::ReturnComponents(
            ReturnComponentsStageProgressPhase::ContinuousNigBasinOptimization,
        ) => "basin optimization",
        CalibrationStageProgressPhase::ReturnComponents(
            ReturnComponentsStageProgressPhase::ContinuousNigFinalEvaluation,
        ) => "final evaluation",
        CalibrationStageProgressPhase::ReturnComponents(
            ReturnComponentsStageProgressPhase::CandidateEnumeration,
        ) => "candidate enumeration",
        CalibrationStageProgressPhase::ReturnComponents(
            ReturnComponentsStageProgressPhase::CandidateConstraintEvaluation,
        ) => "constraint evaluation",
        CalibrationStageProgressPhase::ReturnComponents(
            ReturnComponentsStageProgressPhase::CandidateSelection,
        ) => "candidate selection",
        CalibrationStageProgressPhase::ReturnComponents(
            ReturnComponentsStageProgressPhase::ReportAssembly,
        ) => "report assembly",
        CalibrationStageProgressPhase::JointRefinement(
            JointRefinementStageProgressPhase::StageInputResolution,
        ) => "stage input resolution",
        CalibrationStageProgressPhase::JointRefinement(
            JointRefinementStageProgressPhase::ResumeStateResolution,
        ) => "resume state resolution",
        CalibrationStageProgressPhase::JointRefinement(
            JointRefinementStageProgressPhase::ObservedInputPreparation,
        ) => "observed input preparation",
        CalibrationStageProgressPhase::JointRefinement(
            JointRefinementStageProgressPhase::ParameterizationPreparation,
        ) => "parameterization preparation",
        CalibrationStageProgressPhase::JointRefinement(
            JointRefinementStageProgressPhase::RuntimeMaterialization,
        ) => "runtime materialization",
        CalibrationStageProgressPhase::JointRefinement(
            JointRefinementStageProgressPhase::RuntimeContextAssembly,
        ) => "runtime context assembly",
        CalibrationStageProgressPhase::JointRefinement(
            JointRefinementStageProgressPhase::BaselineLocalEvaluation,
        ) => "baseline local evaluation",
        CalibrationStageProgressPhase::JointRefinement(
            JointRefinementStageProgressPhase::BaselineFinalEvaluation,
        ) => "baseline final evaluation",
        CalibrationStageProgressPhase::JointRefinement(
            JointRefinementStageProgressPhase::BaselineCoarseConsistencyEvaluation,
        ) => "baseline coarse consistency",
        CalibrationStageProgressPhase::JointRefinement(
            JointRefinementStageProgressPhase::RestartSeedGeneration,
        ) => "restart generation",
        CalibrationStageProgressPhase::JointRefinement(
            JointRefinementStageProgressPhase::RestartCandidateScreening,
        ) => "restart screening",
        CalibrationStageProgressPhase::JointRefinement(
            JointRefinementStageProgressPhase::RestartOptimization,
        ) => "restart optimization",
        CalibrationStageProgressPhase::JointRefinement(
            JointRefinementStageProgressPhase::CoarseConsistencyEvaluation,
        ) => "coarse consistency evaluation",
        CalibrationStageProgressPhase::JointRefinement(
            JointRefinementStageProgressPhase::OptionSearchTierRefinement,
        ) => "option search tier",
        CalibrationStageProgressPhase::JointRefinement(
            JointRefinementStageProgressPhase::OptionSelectionTierEvaluation,
        ) => "option selection tier",
        CalibrationStageProgressPhase::JointRefinement(
            JointRefinementStageProgressPhase::OptionAuditTierEvaluation,
        ) => "option audit tier",
        CalibrationStageProgressPhase::JointRefinement(
            JointRefinementStageProgressPhase::FinalSeedEvaluation,
        ) => "final seed evaluation",
        CalibrationStageProgressPhase::JointRefinement(
            JointRefinementStageProgressPhase::FinalAssembly,
        ) => "final assembly",
        _ => "stage progress",
    }
}

#[cfg(test)]
mod tests;
