use std::time::Instant;

use crate::optimizer_adapter::CalibrationOptimizerAdapterRunMetadata;
use crate::stages::joint_refinement::contracts::{
    JointRefinementStageExecutionFailure, JointRefinementStageExecutionMetadata,
    JointRefinementStageExecutionReport, JointRefinementStageFittedPayload,
};
use crate::stages::seasonality::CalibrationStageExecutionStatus;
use janiform_calibration_artifacts::{
    CALIBRATION_JOINT_REFINEMENT_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
    CALIBRATION_JOINT_REFINEMENT_STAGE_HANDOFF_POLICY,
    CALIBRATION_JOINT_REFINEMENT_STAGE_OUTPUT_SCHEMA_VERSION,
    CALIBRATION_STAGE_SEED_DERIVATION_POLICY, CalibrationRunSpec, CalibrationStage,
    JointRefinementStageOptimizerDiagnostics, calibration_stage_token,
};

use super::super::elapsed_millis;
use super::super::{
    JointRefinementCandidateOutcome, JointRefinementContext, build_joint_candidate_diagnostics,
    build_joint_refinement_like_stage_failure_report_with_runtime_profile,
};
use super::preparation::return_components_fitted_parameters_with_mean_reversion;
use super::types::{
    BaselineEvaluationArtifacts, CandidateSearchArtifacts, FinalAssemblyArtifacts,
    ResolvedStagePayloads,
};

const JOINT_REFINEMENT_SELECTION_POLICY: &str =
    "validity_then_admissibility_then_global_loss_with_diversity_and_coarse_consistency.v1";

pub(super) fn build_failure_report_with_runtime_profile(
    run_spec: &CalibrationRunSpec,
    bootstrap_context: &JointRefinementContext,
    stage_started_at: Instant,
    stage: CalibrationStage,
    failure: JointRefinementStageExecutionFailure,
) -> JointRefinementStageExecutionReport {
    let runtime_profile = bootstrap_context.profiling_state.snapshot_runtime_profile(
        elapsed_millis(stage_started_at),
        0,
        0,
        0,
    );

    build_joint_refinement_like_stage_failure_report_with_runtime_profile(
        run_spec,
        stage,
        failure,
        Some(runtime_profile),
    )
}

pub(super) fn build_stage_failure_report(
    run_spec: &CalibrationRunSpec,
    stage: CalibrationStage,
    failure: JointRefinementStageExecutionFailure,
) -> JointRefinementStageExecutionReport {
    build_joint_refinement_like_stage_failure_report_with_runtime_profile(
        run_spec, stage, failure, None,
    )
}

pub(super) fn build_success_report(
    run_spec: &CalibrationRunSpec,
    resolved_stage_payloads: &ResolvedStagePayloads<'_>,
    bootstrap_context: &JointRefinementContext,
    baseline_evaluation_artifacts: &BaselineEvaluationArtifacts,
    candidate_search_artifacts: &CandidateSearchArtifacts,
    final_assembly_artifacts: FinalAssemblyArtifacts,
    stage: CalibrationStage,
) -> JointRefinementStageExecutionReport {
    let fitted_stage_payload = build_fitted_stage_payload(
        resolved_stage_payloads,
        bootstrap_context,
        candidate_search_artifacts,
        final_assembly_artifacts,
        stage,
    );

    let metadata = build_execution_metadata(
        resolved_stage_payloads,
        candidate_search_artifacts,
        &fitted_stage_payload,
    );

    let warnings = candidate_search_artifacts
        .warning
        .iter()
        .cloned()
        .collect::<Vec<_>>();

    let status = if warnings.is_empty() {
        CalibrationStageExecutionStatus::Success
    } else {
        CalibrationStageExecutionStatus::SuccessWithWarnings
    };

    JointRefinementStageExecutionReport {
        schema_version: CALIBRATION_JOINT_REFINEMENT_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
        stage,
        status,
        run_id: run_spec.run_id.clone(),
        execution_mode: run_spec.execution_mode,
        handoff_policy: CALIBRATION_JOINT_REFINEMENT_STAGE_HANDOFF_POLICY,
        fitted_stage_payload: Some(fitted_stage_payload),
        metadata: Some(metadata),
        runtime_profile: None,
        warnings,
        failure: None,
        runtime_handoff: Some(build_runtime_handoff(
            &baseline_evaluation_artifacts.baseline_candidate_outcome,
            candidate_search_artifacts,
        )),
    }
}

fn build_runtime_handoff(
    baseline_candidate_outcome: &JointRefinementCandidateOutcome,
    candidate_search_artifacts: &CandidateSearchArtifacts,
) -> super::super::state::JointRefinementStageRuntimeHandoff {
    // Stage reports are retained after this function returns, so the handoff owns its artifacts.
    super::super::state::JointRefinementStageRuntimeHandoff {
        baseline_candidate_outcome: baseline_candidate_outcome.clone(),
        candidate_outcomes: candidate_search_artifacts.candidate_outcomes.clone(),
        candidate_evaluation_timing_diagnostics: candidate_search_artifacts
            .candidate_evaluation_timing_diagnostics
            .clone(),
        valid_candidate_count: candidate_search_artifacts.valid_candidate_count,
        best_candidate: candidate_search_artifacts.best_candidate.clone(),
        option_refinement_candidate_frontier: candidate_search_artifacts
            .option_refinement_candidate_frontier
            .clone(),
        selected_option_search_tier_pricing_artifact: candidate_search_artifacts
            .selected_option_search_tier_pricing_artifact
            .clone(),
        selected_option_selection_tier_pricing_artifact: candidate_search_artifacts
            .selected_option_selection_tier_pricing_artifact
            .clone(),
        selected_option_audit_tier_pricing_artifact: candidate_search_artifacts
            .selected_option_audit_tier_pricing_artifact
            .clone(),
        option_search_selection_agreement: candidate_search_artifacts
            .option_search_selection_agreement
            .clone(),
        warning: candidate_search_artifacts.warning.clone(),
        population_refinement_wall_clock_millis: candidate_search_artifacts
            .population_refinement_wall_clock_millis,
        coarse_consistency_wall_clock_millis: candidate_search_artifacts
            .coarse_consistency_wall_clock_millis,
    }
}

fn build_fitted_stage_payload(
    resolved_stage_payloads: &ResolvedStagePayloads<'_>,
    bootstrap_context: &JointRefinementContext,
    candidate_search_artifacts: &CandidateSearchArtifacts,
    final_assembly_artifacts: FinalAssemblyArtifacts,
    stage: CalibrationStage,
) -> JointRefinementStageFittedPayload {
    let candidate_diagnostics =
        build_joint_candidate_diagnostics(candidate_search_artifacts.candidate_outcomes.as_slice());

    let runtime_profile = bootstrap_context.profiling_state.snapshot_runtime_profile(
        candidate_search_artifacts.population_refinement_wall_clock_millis,
        candidate_search_artifacts.coarse_consistency_wall_clock_millis,
        final_assembly_artifacts.final_seed_evaluation_wall_clock_millis,
        final_assembly_artifacts.final_assembly_wall_clock_millis,
    );

    JointRefinementStageFittedPayload {
        schema_version: CALIBRATION_JOINT_REFINEMENT_STAGE_OUTPUT_SCHEMA_VERSION,
        selected_candidate_id: format!(
            "{}_{}",
            calibration_stage_token(stage),
            candidate_search_artifacts.best_candidate.candidate_id
        ),
        fitted_parameters: candidate_search_artifacts
            .best_candidate
            .final_evaluation
            .fitted_parameters
            .clone(),
        baseline_hawkes_fitted_parameters: resolved_stage_payloads
            .hawkes_payload
            .fitted_parameters
            .clone(),
        baseline_return_components_fitted_parameters:
            return_components_fitted_parameters_with_mean_reversion(
                resolved_stage_payloads.return_components_payload,
                resolved_stage_payloads.mean_reversion_payload,
            ),
        objective_breakdown: candidate_search_artifacts
            .best_candidate
            .final_evaluation
            .breakdown
            .clone(),
        optimizer_diagnostics: JointRefinementStageOptimizerDiagnostics {
            local_objective: candidate_search_artifacts
                .best_candidate
                .local_evaluation
                .breakdown
                .global_loss,
            final_objective: candidate_search_artifacts
                .best_candidate
                .final_evaluation
                .breakdown
                .global_loss,
            local_evaluation_count: candidate_search_artifacts
                .best_candidate
                .local_evaluation_count,
            final_simulation_seeds: bootstrap_context.final_simulation_seeds.clone(),
            stop_reason: candidate_search_artifacts
                .best_candidate
                .stop_reason
                .to_string(),
            candidate_diagnostics,
            runtime_profile,
            // The success report builds metadata from the same candidate artifacts after
            // payload assembly, so the timing diagnostics stay borrowed here.
            candidate_evaluation_timing_diagnostics: candidate_search_artifacts
                .candidate_evaluation_timing_diagnostics
                .clone(),
        },
        candidate_frontier: final_assembly_artifacts.candidate_frontier,
        simulation_summaries: final_assembly_artifacts.simulation_summaries,
        coarse_tick_consistency: final_assembly_artifacts.coarse_tick_consistency,
        option_calibration_diagnostics: final_assembly_artifacts.option_calibration_diagnostics,
    }
}

fn build_execution_metadata(
    resolved_stage_payloads: &ResolvedStagePayloads<'_>,
    candidate_search_artifacts: &CandidateSearchArtifacts,
    fitted_stage_payload: &JointRefinementStageFittedPayload,
) -> JointRefinementStageExecutionMetadata {
    JointRefinementStageExecutionMetadata {
        stage_output_schema_version: CALIBRATION_JOINT_REFINEMENT_STAGE_OUTPUT_SCHEMA_VERSION,
        run_global_seed: resolved_stage_payloads.resolved_stage_seed.run_global_seed,
        stage_seed: resolved_stage_payloads.resolved_stage_seed.stage_seed,
        stage_seed_source: resolved_stage_payloads
            .resolved_stage_seed
            .stage_seed_source,
        stage_seed_derivation_policy: CALIBRATION_STAGE_SEED_DERIVATION_POLICY,
        solver_settings: resolved_stage_payloads.stage_solver_settings.clone(),
        optimizer_adapter_metadata: CalibrationOptimizerAdapterRunMetadata {
            schema_version: "calibration.optimizer.adapter_run.v1",
            backend_id: "joint_refinement.block_coordinate_reconciliation.v1".to_string(),
            random_seed: resolved_stage_payloads.resolved_stage_seed.stage_seed,
            selection_policy: JOINT_REFINEMENT_SELECTION_POLICY,
            penalty_policy: "invalid_candidates_are_screened_before_lexicographic_frontier_selection.v1",
            bounds_policy: "bounded_joint_block_coordinate_refinement.v1",
            solver_settings: resolved_stage_payloads.stage_solver_settings.clone(),
        },
        input_candidate_count: candidate_search_artifacts.candidate_outcomes.len(),
        valid_candidate_count: candidate_search_artifacts.valid_candidate_count,
        selected_candidate_raw_objective: fitted_stage_payload.objective_breakdown.global_loss,
        selected_candidate_penalized_objective: fitted_stage_payload
            .objective_breakdown
            .global_loss,
    }
}
