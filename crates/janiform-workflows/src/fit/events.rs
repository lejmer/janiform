use std::path::Path;

use janiform_calibration_artifacts::CalibrationStage;
use janiform_calibration_runtime::{
    CalibrationStageProgressPhase, CalibrationStagedRunFailure, CalibrationStagedRunObserverEvent,
    CalibrationStagedRunReport, CalibrationStagedRunStageProgressEvent,
    JointRefinementProgressCandidateAssessment, JointRefinementStageProgressPhase,
    ReturnComponentsStageProgressPhase,
};
use janiform_event_log::{
    EventDeliveryPriority, ProgressMeasure as EventProgressMeasure, RunArtifactEvent,
    RunErrorEvent, RunEvent, RunEventLogWriter, RunEventPayload, RunLifecycleEvent, RunPhaseRef,
    RunProgressEvent, RunStageRef, WorkflowRef,
};
use janiform_terminal_ui::{
    ProgressEvent, ProgressMeasure as UiProgressMeasure, ProgressWorkItemColumn,
    ProgressWorkItemMetric,
};

use crate::error::WorkflowError;

use super::labels::stage_label;

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
        CalibrationStagedRunObserverEvent::StageProgress(progress) => vec![RunEvent::new(
            progress.run_id.clone(),
            workflow,
            EventDeliveryPriority::DroppableVolatile,
            RunEventPayload::Progress(RunProgressEvent {
                stage: Some(stage_ref(progress.stage)),
                phase: Some(phase_ref(
                    phase_token(&progress.progress_phase),
                    phase_label(&progress.progress_phase),
                )),
                subphase: None,
                measures: vec![EventProgressMeasure::BoundedCount {
                    metric_id: "stage_units".to_string(),
                    label: Some("stage units".to_string()),
                    completed_units: progress.completed_units,
                    total_units: progress.total_units,
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

/// Convert a staged runtime observer event into a terminal progress event.
pub fn progress_event_from_staged_event(
    event: &CalibrationStagedRunObserverEvent,
) -> Result<Option<ProgressEvent>, WorkflowError> {
    let progress_event = match event {
        CalibrationStagedRunObserverEvent::RunStarted {
            run_id,
            stage_sequence,
            ..
        } => Some(ProgressEvent::RunStarted {
            run_id: run_id.clone(),
            scope: "Calibrating".to_string(),
            input_count: 1,
            output_path: None,
            stage_sequence: stage_sequence
                .iter()
                .copied()
                .map(stage_label)
                .map(str::to_string)
                .collect(),
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
            Some(ProgressEvent::StageProgress {
                phase: phase_label(&progress.progress_phase).to_string(),
                measure: stage_progress_measure(progress)?,
                work_item_label: progress_work_item_label(
                    &progress.progress_phase,
                    progress.current_item_label.as_deref(),
                ),
                work_item_progress_percent: progress.evaluation_progress_percent,
                planned_work_item_labels: progress.planned_work_item_labels.clone(),
                work_item_columns: work_item_table_columns(progress),
                work_item_metrics: work_item_table_metrics(progress),
                latest_signal: stage_progress_signal(progress),
            })
        }
        CalibrationStagedRunObserverEvent::StageCompleted {
            stage,
            runtime_seconds,
            ..
        } => Some(ProgressEvent::StageCompleted {
            stage: stage_label(*stage).to_string(),
            runtime_seconds: *runtime_seconds,
        }),
        CalibrationStagedRunObserverEvent::RunCompleted { .. } => None,
        CalibrationStagedRunObserverEvent::RunFailed { failure, .. } => {
            Some(ProgressEvent::RunFailed {
                reason: serialized_failure_reason(failure)?,
            })
        }
    };

    Ok(progress_event)
}

fn stage_progress_measure(
    progress: &CalibrationStagedRunStageProgressEvent,
) -> Result<UiProgressMeasure, WorkflowError> {
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

/// Resolve a user-facing failure reason from a staged run report.
pub fn failure_reason(report: &CalibrationStagedRunReport) -> Result<String, WorkflowError> {
    match report.failure.as_ref() {
        Some(failure) => serialized_failure_reason(failure),
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
        | Some(CalibrationStagedRunFailure::StageParameterArtifactIoFailed { stage, .. })
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

fn stage_progress_signal(progress: &CalibrationStagedRunStageProgressEvent) -> Option<String> {
    if progress_work_item_label(
        &progress.progress_phase,
        progress.current_item_label.as_deref(),
    )
    .is_some()
        && let Some(signal) = parallel_stage_progress_signal(
            progress.active_restart_count,
            progress.valid_completed_units,
            progress.cumulative_local_evaluation_count,
            progress.best_weighted_objective,
            progress.best_coarse_consistency_fit,
        )
    {
        return Some(signal);
    }

    let mut parts = Vec::new();

    if let Some(current_item_label) = progress.current_item_label.as_deref() {
        parts.push(current_item_label.to_string());
    }
    push_evaluation_signal(
        &mut parts,
        progress.current_evaluation_count,
        progress.total_evaluation_budget,
    );

    if let Some(current_weighted_objective) = progress.current_weighted_objective {
        parts.push(format!("objective {current_weighted_objective:.6}"));
    }
    if let Some(best_weighted_objective) = progress.best_weighted_objective {
        parts.push(format!("best {best_weighted_objective:.6}"));
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

fn parallel_stage_progress_signal(
    active_restart_count: Option<usize>,
    valid_completed_units: Option<usize>,
    cumulative_local_evaluation_count: Option<usize>,
    best_weighted_objective: Option<f64>,
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
    if let Some(best_weighted_objective) = best_weighted_objective {
        parts.push(format!("best objective {best_weighted_objective:.6}"));
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
    if uses_restart_optimization_columns || progress.current_parameter_block.is_some() {
        columns.push(ProgressWorkItemColumn::new("block", "Blk"));
    }
    if uses_restart_optimization_columns || progress.current_weighted_objective.is_some() {
        columns.push(ProgressWorkItemColumn::new(
            "current_objective",
            "Cur. obj.",
        ));
    }
    if uses_restart_optimization_columns
        || progress.current_restart_best_weighted_objective.is_some()
    {
        columns.push(ProgressWorkItemColumn::new("best_objective", "Best obj."));
    }
    if uses_restart_optimization_columns || progress.current_weighted_objective.is_some() {
        columns.push(ProgressWorkItemColumn::new("final_objective", "Final obj."));
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
        columns.push(ProgressWorkItemColumn::new("exact_buy", "CF-B"));
    }
    if progress.exact_closed_form_sell_count.is_some() {
        columns.push(ProgressWorkItemColumn::new("exact_sell", "CF-S"));
    }
    if progress.exact_tilted_mixed_count.is_some() {
        columns.push(ProgressWorkItemColumn::new("exact_mixed", "Mix"));
    }

    columns
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
    if let Some(current_parameter_block) = progress.current_parameter_block.as_deref() {
        metrics.push(ProgressWorkItemMetric::new(
            "block",
            current_parameter_block.to_string(),
        ));
    }

    if let Some(current_weighted_objective) = progress.current_weighted_objective {
        let metric_key = if progress.evaluation_progress_percent == Some(100) {
            "final_objective"
        } else {
            "current_objective"
        };

        metrics.push(ProgressWorkItemMetric::new(
            metric_key,
            format!("{current_weighted_objective:.6}"),
        ));
    }
    if let Some(current_restart_best_weighted_objective) =
        progress.current_restart_best_weighted_objective
    {
        metrics.push(ProgressWorkItemMetric::new(
            "best_objective",
            format!("{current_restart_best_weighted_objective:.6}"),
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

fn progress_work_item_label(
    progress_phase: &CalibrationStageProgressPhase,
    current_item_label: Option<&str>,
) -> Option<String> {
    let current_item_label = current_item_label?;

    match progress_phase {
        CalibrationStageProgressPhase::ReturnComponents(
            ReturnComponentsStageProgressPhase::ContinuousNigSeedScoring
            | ReturnComponentsStageProgressPhase::ContinuousNigBasinOptimization,
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

#[cfg(test)]
mod tests;
