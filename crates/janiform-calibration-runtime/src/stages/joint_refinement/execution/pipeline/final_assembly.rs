use std::sync::mpsc::Sender;
use std::time::Instant;

use janiform_calibration_artifacts::JointRefinementStageOptionCalibrationDiagnostics;

use crate::stages::joint_refinement::contracts::{
    JointRefinementStageCandidateDiagnostic, JointRefinementStageExecutionFailure,
    JointRefinementStageProgressPhase, JointRefinementStageProgressUpdate,
};

use super::super::{
    JointRefinementContext, JointRefinementSeedSummary, build_joint_candidate_frontier_diagnostics,
    build_joint_finalist_seed_summaries, build_joint_simulation_summaries,
    build_option_calibration_diagnostics, elapsed_millis, ordered_parameter_bits,
};
use super::candidate_selection::build_selectable_candidate_outcomes;
use super::progress::emit_final_assembly_progress;
use super::types::{BaselineEvaluationArtifacts, CandidateSearchArtifacts, FinalAssemblyArtifacts};

const FINAL_ASSEMBLY_CANDIDATE_FRONTIER_LABEL: &str = "candidate_frontier";
const FINAL_ASSEMBLY_SIMULATION_SUMMARIES_LABEL: &str = "simulation_summaries";
const FINAL_ASSEMBLY_COARSE_TICK_CONSISTENCY_LABEL: &str = "coarse_tick_consistency";
const FINAL_ASSEMBLY_OPTION_CALIBRATION_DIAGNOSTICS_LABEL: &str = "option_calibration_diagnostics";
const FINAL_ASSEMBLY_BASE_UNIT_LABELS: [&str; 3] = [
    FINAL_ASSEMBLY_CANDIDATE_FRONTIER_LABEL,
    FINAL_ASSEMBLY_SIMULATION_SUMMARIES_LABEL,
    FINAL_ASSEMBLY_COARSE_TICK_CONSISTENCY_LABEL,
];

struct FinalAssemblyProgressTracker<'a> {
    progress_sender: Option<&'a Sender<JointRefinementStageProgressUpdate>>,
    completed_units: usize,
    total_units: usize,
}

impl<'a> FinalAssemblyProgressTracker<'a> {
    fn new(
        progress_sender: Option<&'a Sender<JointRefinementStageProgressUpdate>>,
        total_units: usize,
    ) -> Self {
        FinalAssemblyProgressTracker {
            progress_sender,
            completed_units: 0,
            total_units,
        }
    }

    fn emit(&self, current_item_label: &str) {
        emit_final_assembly_progress(
            self.progress_sender,
            self.completed_units,
            self.total_units,
            current_item_label,
        );
    }

    fn complete_and_emit(&mut self, current_item_label: &str) {
        self.completed_units = self.completed_units.saturating_add(1);
        self.emit(current_item_label);
    }
}

struct CandidateFrontierArtifacts {
    candidate_frontier: Vec<JointRefinementStageCandidateDiagnostic>,
    wall_clock_millis: u64,
}

struct FinalistSeedSummaryArtifacts {
    finalist_seed_summaries: Vec<(u64, JointRefinementSeedSummary)>,
    final_seed_evaluation_wall_clock_millis: u64,
}

pub(super) fn empty_final_assembly_artifacts() -> FinalAssemblyArtifacts {
    FinalAssemblyArtifacts {
        candidate_frontier: Vec::new(),
        simulation_summaries: Vec::new(),
        coarse_tick_consistency: Vec::new(),
        option_calibration_diagnostics: None,
        final_seed_evaluation_wall_clock_millis: 0,
        final_assembly_wall_clock_millis: 0,
    }
}

pub(super) fn assemble_final_stage_outputs(
    bootstrap_context: &JointRefinementContext,
    baseline_evaluation_artifacts: &BaselineEvaluationArtifacts,
    candidate_search_artifacts: &CandidateSearchArtifacts,
    diagnostics_request: &crate::diagnostics::types::CalibrationDiagnosticsRequest,
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
) -> Result<FinalAssemblyArtifacts, JointRefinementStageExecutionFailure> {
    let mut final_assembly_progress = FinalAssemblyProgressTracker::new(
        progress_sender,
        final_assembly_progress_unit_count(candidate_search_artifacts, diagnostics_request),
    );

    final_assembly_progress.emit(FINAL_ASSEMBLY_CANDIDATE_FRONTIER_LABEL);

    let CandidateFrontierArtifacts {
        candidate_frontier,
        wall_clock_millis: candidate_frontier_wall_clock_millis,
    } = build_candidate_frontier_artifacts(
        bootstrap_context,
        baseline_evaluation_artifacts,
        candidate_search_artifacts,
    );

    final_assembly_progress.complete_and_emit(FINAL_ASSEMBLY_CANDIDATE_FRONTIER_LABEL);

    let FinalistSeedSummaryArtifacts {
        finalist_seed_summaries,
        final_seed_evaluation_wall_clock_millis,
    } = build_finalist_seed_summary_artifacts(
        bootstrap_context,
        candidate_search_artifacts,
        progress_sender,
    );

    let remaining_final_assembly_started_at = Instant::now();
    let simulation_summaries = build_joint_simulation_summaries(finalist_seed_summaries.as_slice());

    final_assembly_progress.complete_and_emit(FINAL_ASSEMBLY_SIMULATION_SUMMARIES_LABEL);
    final_assembly_progress.emit(FINAL_ASSEMBLY_COARSE_TICK_CONSISTENCY_LABEL);

    let coarse_tick_consistency = resolve_selected_coarse_tick_consistency(
        bootstrap_context,
        baseline_evaluation_artifacts,
        candidate_search_artifacts,
        finalist_seed_summaries.as_slice(),
        progress_sender,
    );

    final_assembly_progress.complete_and_emit(FINAL_ASSEMBLY_COARSE_TICK_CONSISTENCY_LABEL);

    if should_build_option_calibration_diagnostics(diagnostics_request, candidate_search_artifacts)
    {
        final_assembly_progress.emit(FINAL_ASSEMBLY_OPTION_CALIBRATION_DIAGNOSTICS_LABEL);
    }

    let option_calibration_diagnostics = build_option_calibration_diagnostics_if_requested(
        bootstrap_context,
        candidate_search_artifacts,
        diagnostics_request,
    )?;

    if should_build_option_calibration_diagnostics(diagnostics_request, candidate_search_artifacts)
    {
        final_assembly_progress
            .complete_and_emit(FINAL_ASSEMBLY_OPTION_CALIBRATION_DIAGNOSTICS_LABEL);
    }

    let final_assembly_wall_clock_millis =
        candidate_frontier_wall_clock_millis + elapsed_millis(remaining_final_assembly_started_at);

    Ok(FinalAssemblyArtifacts {
        candidate_frontier,
        simulation_summaries,
        coarse_tick_consistency,
        option_calibration_diagnostics,
        final_seed_evaluation_wall_clock_millis,
        final_assembly_wall_clock_millis,
    })
}

fn build_candidate_frontier_artifacts(
    bootstrap_context: &JointRefinementContext,
    baseline_evaluation_artifacts: &BaselineEvaluationArtifacts,
    candidate_search_artifacts: &CandidateSearchArtifacts,
) -> CandidateFrontierArtifacts {
    let selectable_candidate_outcomes = build_selectable_candidate_outcomes(
        candidate_search_artifacts.candidate_outcomes.as_slice(),
        &baseline_evaluation_artifacts.baseline_candidate_outcome,
    );

    let candidate_frontier_started_at = Instant::now();

    let candidate_frontier = build_joint_candidate_frontier_diagnostics(
        selectable_candidate_outcomes.as_slice(),
        bootstrap_context,
    );

    CandidateFrontierArtifacts {
        candidate_frontier,
        wall_clock_millis: elapsed_millis(candidate_frontier_started_at),
    }
}

fn build_finalist_seed_summary_artifacts(
    bootstrap_context: &JointRefinementContext,
    candidate_search_artifacts: &CandidateSearchArtifacts,
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
) -> FinalistSeedSummaryArtifacts {
    let final_seed_evaluation_started_at = Instant::now();

    let finalist_seed_summaries = build_joint_finalist_seed_summaries(
        bootstrap_context,
        Some(
            ordered_parameter_bits(
                candidate_search_artifacts
                    .best_candidate
                    .parameter_vector
                    .as_slice(),
            )
            .as_slice(),
        ),
        &candidate_search_artifacts
            .best_candidate
            .final_evaluation
            .fitted_parameters,
        progress_sender,
    );

    FinalistSeedSummaryArtifacts {
        finalist_seed_summaries,
        final_seed_evaluation_wall_clock_millis: elapsed_millis(final_seed_evaluation_started_at),
    }
}

fn build_option_calibration_diagnostics_if_requested(
    bootstrap_context: &JointRefinementContext,
    candidate_search_artifacts: &CandidateSearchArtifacts,
    diagnostics_request: &crate::diagnostics::types::CalibrationDiagnosticsRequest,
) -> Result<
    Option<JointRefinementStageOptionCalibrationDiagnostics>,
    JointRefinementStageExecutionFailure,
> {
    if !should_build_option_calibration_diagnostics(diagnostics_request, candidate_search_artifacts)
    {
        return Ok(None);
    }

    build_option_calibration_diagnostics(
        bootstrap_context,
        candidate_search_artifacts
            .selected_option_search_tier_pricing_artifact
            .as_deref(),
        candidate_search_artifacts
            .selected_option_selection_tier_pricing_artifact
            .as_deref(),
        candidate_search_artifacts
            .selected_option_audit_tier_pricing_artifact
            .as_deref(),
        candidate_search_artifacts
            .option_search_selection_agreement
            .clone(),
    )
    .map_err(|error_message| {
        JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput { error_message }
    })
}

fn final_assembly_progress_unit_count(
    candidate_search_artifacts: &CandidateSearchArtifacts,
    diagnostics_request: &crate::diagnostics::types::CalibrationDiagnosticsRequest,
) -> usize {
    FINAL_ASSEMBLY_BASE_UNIT_LABELS.len()
        + usize::from(should_build_option_calibration_diagnostics(
            diagnostics_request,
            candidate_search_artifacts,
        ))
}

fn should_build_option_calibration_diagnostics(
    diagnostics_request: &crate::diagnostics::types::CalibrationDiagnosticsRequest,
    candidate_search_artifacts: &CandidateSearchArtifacts,
) -> bool {
    diagnostics_request.option_surface_shape_requested()
        && candidate_search_artifacts
            .selected_option_selection_tier_pricing_artifact
            .is_some()
}

fn resolve_selected_coarse_tick_consistency(
    bootstrap_context: &JointRefinementContext,
    baseline_evaluation_artifacts: &BaselineEvaluationArtifacts,
    candidate_search_artifacts: &CandidateSearchArtifacts,
    finalist_seed_summaries: &[(u64, super::super::JointRefinementSeedSummary)],
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
) -> Vec<crate::stages::joint_refinement::contracts::JointRefinementStageCoarseTickConsistencyMetric>
{
    if candidate_search_artifacts.best_candidate.candidate_id
        == baseline_evaluation_artifacts
            .baseline_candidate_outcome
            .candidate_id
    {
        return baseline_evaluation_artifacts
            .baseline_coarse_consistency_artifacts
            .as_ref()
            .map(|artifacts| artifacts.metrics.clone())
            .unwrap_or_else(|| {
                super::super::build_joint_coarse_tick_consistency_metrics(
                    bootstrap_context,
                    &candidate_search_artifacts
                        .best_candidate
                        .final_evaluation
                        .fitted_parameters,
                    finalist_seed_summaries,
                    Some(
                        super::super::coarse_consistency::JointRefinementCoarseConsistencyProgress {
                            sender: progress_sender,
                            phase: JointRefinementStageProgressPhase::FinalAssembly,
                            current_item_label: Some("coarse_tick_consistency"),
                        },
                    ),
                )
            });
    }

    candidate_search_artifacts
        .coarse_consistency_artifacts_by_candidate_id
        .get(
            candidate_search_artifacts
                .best_candidate
                .candidate_id
                .as_str(),
        )
        .map(|artifacts| artifacts.metrics.clone())
        .unwrap_or_else(|| {
            super::super::build_joint_coarse_tick_consistency_metrics(
                bootstrap_context,
                &candidate_search_artifacts
                    .best_candidate
                    .final_evaluation
                    .fitted_parameters,
                finalist_seed_summaries,
                Some(
                    super::super::coarse_consistency::JointRefinementCoarseConsistencyProgress {
                        sender: progress_sender,
                        phase: JointRefinementStageProgressPhase::FinalAssembly,
                        current_item_label: Some("coarse_tick_consistency"),
                    },
                ),
            )
        })
}
