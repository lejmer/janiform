use std::path::Path;

use janiform::calibration::{
    CalibrationStage, CalibrationStageProgressPhase, CalibrationStagedRunFailure,
    CalibrationStagedRunObserverEvent, CalibrationStagedRunReport,
    JointRefinementStageProgressPhase, ReturnComponentsStageProgressPhase,
};
use janiform_event_log::{
    EventDeliveryPriority, ProgressMeasure as EventProgressMeasure, RunArtifactEvent,
    RunErrorEvent, RunEvent, RunEventLogWriter, RunEventPayload, RunLifecycleEvent, RunPhaseRef,
    RunProgressEvent, RunStageRef, WorkflowRef,
};
use janiform_terminal_ui::{ProgressEvent, ProgressMeasure as UiProgressMeasure};

use crate::commands::fit::labels::stage_label;
use crate::error::CliError;

pub(super) fn run_events_from_staged_event(
    event: &CalibrationStagedRunObserverEvent,
    workflow: WorkflowRef,
    scope_id: &str,
    scope_label: &str,
    input_paths: &[String],
    output_directory: &str,
) -> Result<Vec<RunEvent>, CliError> {
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
        CalibrationStagedRunObserverEvent::StageProgress {
            run_id,
            stage,
            progress_phase,
            completed_units,
            total_units,
            ..
        } => vec![RunEvent::new(
            run_id.clone(),
            workflow,
            EventDeliveryPriority::DroppableVolatile,
            RunEventPayload::Progress(RunProgressEvent {
                stage: Some(stage_ref(*stage)),
                phase: Some(phase_ref(
                    phase_token(progress_phase),
                    phase_label(progress_phase),
                )),
                subphase: None,
                measures: vec![EventProgressMeasure::BoundedCount {
                    metric_id: "stage_units".to_string(),
                    label: Some("stage units".to_string()),
                    completed_units: *completed_units,
                    total_units: *total_units,
                }],
            }),
        )],
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
    };

    Ok(run_events)
}

pub(super) fn progress_event_from_staged_event(
    event: &CalibrationStagedRunObserverEvent,
) -> Result<Option<ProgressEvent>, CliError> {
    let progress_event = match event {
        CalibrationStagedRunObserverEvent::RunStarted { run_id, .. } => {
            Some(ProgressEvent::RunStarted {
                run_id: run_id.clone(),
                scope: "fit".to_string(),
                input_count: 1,
                output_path: None,
            })
        }
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
        CalibrationStagedRunObserverEvent::StageProgress {
            progress_phase,
            completed_units,
            total_units,
            current_item_label,
            current_evaluation_count,
            total_evaluation_budget,
            current_weighted_objective,
            best_weighted_objective,
            current_convergence_ratio,
            ..
        } => Some(ProgressEvent::StageProgress {
            phase: phase_label(progress_phase).to_string(),
            measure: UiProgressMeasure::bounded(*completed_units, *total_units)
                .map_err(|source| CliError::OutputProgress { source })?,
            latest_signal: progress_signal(
                current_item_label.as_deref(),
                *current_evaluation_count,
                *total_evaluation_budget,
                *current_weighted_objective,
                *best_weighted_objective,
                *current_convergence_ratio,
            ),
        }),
        CalibrationStagedRunObserverEvent::StageCompleted { stage, .. } => {
            Some(ProgressEvent::StageCompleted {
                stage: stage_label(*stage).to_string(),
            })
        }
        CalibrationStagedRunObserverEvent::RunCompleted { .. } => {
            Some(ProgressEvent::RunCompleted {
                status: "completed".to_string(),
            })
        }
        CalibrationStagedRunObserverEvent::RunFailed { failure, .. } => {
            Some(ProgressEvent::RunFailed {
                reason: serialized_failure_reason(failure)?,
            })
        }
    };

    Ok(progress_event)
}

pub(super) fn append_artifact_event(
    event_writer: &mut RunEventLogWriter,
    workflow: WorkflowRef,
    run_id: &str,
    artifact_kind: &str,
    path: &Path,
    stage: Option<CalibrationStage>,
) -> Result<(), CliError> {
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
        .map_err(|source| CliError::FitEventLog { source })
}

pub(super) fn failure_reason(report: &CalibrationStagedRunReport) -> Result<String, CliError> {
    match report.failure.as_ref() {
        Some(failure) => serialized_failure_reason(failure),
        None => Ok("staged calibration did not complete successfully".to_string()),
    }
}

pub(super) fn failed_stage_label(report: &CalibrationStagedRunReport) -> String {
    match report.failure.as_ref() {
        Some(CalibrationStagedRunFailure::MissingStageInput { stage })
        | Some(CalibrationStagedRunFailure::MissingInMemoryUpstreamPayload {
            consuming_stage: stage,
            ..
        })
        | Some(CalibrationStagedRunFailure::StageExecutionFailed { stage, .. })
        | Some(CalibrationStagedRunFailure::StageParameterArtifactIoFailed { stage, .. })
        | Some(CalibrationStagedRunFailure::MissingCheckpointMetadataForSuccessfulStage {
            stage,
        }) => stage_label(*stage).to_string(),
        _ => "unknown".to_string(),
    }
}

pub(super) fn resume_availability(report: &CalibrationStagedRunReport) -> String {
    report
        .emitted_checkpoints
        .last()
        .map(|checkpoint| checkpoint.checkpoint_reference.clone())
        .unwrap_or_else(|| "not available".to_string())
}

fn serialized_failure_reason(failure: &CalibrationStagedRunFailure) -> Result<String, CliError> {
    serde_json::to_string(failure).map_err(|source| CliError::OutputSerialize { source })
}

fn progress_signal(
    current_item_label: Option<&str>,
    current_evaluation_count: Option<usize>,
    total_evaluation_budget: Option<usize>,
    current_weighted_objective: Option<f64>,
    best_weighted_objective: Option<f64>,
    current_convergence_ratio: Option<f64>,
) -> Option<String> {
    let mut parts = Vec::new();

    if let Some(current_item_label) = current_item_label {
        parts.push(current_item_label.to_string());
    }
    if let Some(current_evaluation_count) = current_evaluation_count {
        match total_evaluation_budget {
            Some(total_evaluation_budget) => {
                parts.push(format!(
                    "eval {current_evaluation_count}/{total_evaluation_budget}"
                ));
            }
            None => parts.push(format!("eval {current_evaluation_count}")),
        }
    }
    if let Some(current_weighted_objective) = current_weighted_objective {
        parts.push(format!("objective {current_weighted_objective:.6}"));
    }
    if let Some(best_weighted_objective) = best_weighted_objective {
        parts.push(format!("best {best_weighted_objective:.6}"));
    }
    if let Some(current_convergence_ratio) = current_convergence_ratio {
        parts.push(format!("convergence {current_convergence_ratio:.6}"));
    }

    (!parts.is_empty()).then(|| parts.join(", "))
}

fn stage_ref(stage: CalibrationStage) -> RunStageRef {
    RunStageRef::labeled(stage_token(stage), stage_label(stage))
}

fn phase_ref(token: &str, label: &str) -> RunPhaseRef {
    RunPhaseRef::labeled(token, label)
}

fn stage_token(stage: CalibrationStage) -> &'static str {
    match stage {
        CalibrationStage::Seasonality => "seasonality",
        CalibrationStage::Hawkes => "hawkes",
        CalibrationStage::ReturnComponents => "return_components",
        CalibrationStage::MeanReversion => "mean_reversion",
        CalibrationStage::JointRefinement => "joint_refinement",
    }
}

fn phase_token(phase: &CalibrationStageProgressPhase) -> &'static str {
    match phase {
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
