use std::path::Path;
use std::sync::mpsc::Sender;

use crate::stages::joint_refinement::contracts::{
    JointRefinementStageExecutionFailure, JointRefinementStageExecutionInput,
    JointRefinementStageProgressPhase, JointRefinementStageProgressUpdate,
};
use crate::stages::joint_refinement::option_surface_tiering::JointRefinementOptionSurfaceTierKind;
use janiform_calibration_artifacts::{CalibrationRunSpec, CalibrationStage};

use super::super::stage_resume::candidate_search_artifacts_from_upstream_or_resume;
use super::super::{
    JointRefinementContext, JointRefinementResumeStore, OptionPricingProgressInput,
    OptionSurfaceTierPricingArtifactResolutionInput, option_pricing_context_fingerprint,
    resolve_option_surface_tier_pricing_artifact, run_option_refinement,
};
use super::candidate_search::run_candidate_search;
use super::types::{
    BaselineEvaluationArtifacts, CandidateSearchArtifacts, JointRefinementPipelineStage,
    PreparedBootstrapContext, ResolvedStagePayloads,
};

pub(super) struct CandidateSearchArtifactResolutionInput<'a> {
    pub(super) run_spec: &'a CalibrationRunSpec,
    pub(super) stage_input: &'a JointRefinementStageExecutionInput,
    pub(super) resolved_stage_payloads: &'a ResolvedStagePayloads<'a>,
    pub(super) prepared_bootstrap: &'a PreparedBootstrapContext,
    pub(super) baseline_evaluation_artifacts: &'a BaselineEvaluationArtifacts,
    pub(super) progress_sender: Option<&'a Sender<JointRefinementStageProgressUpdate>>,
    pub(super) resume_store: &'a mut JointRefinementResumeStore,
    pub(super) disk_checkpoint_directory: Option<&'a Path>,
    pub(super) pipeline_stage: JointRefinementPipelineStage,
    pub(super) upstream_joint_refinement_handoff:
        Option<&'a super::super::state::JointRefinementStageRuntimeHandoff>,
    pub(super) upstream_option_refinement_handoff:
        Option<&'a super::super::state::JointRefinementStageRuntimeHandoff>,
}

pub(super) fn resolve_candidate_search_artifacts_for_stage(
    input: CandidateSearchArtifactResolutionInput<'_>,
) -> Result<CandidateSearchArtifacts, JointRefinementStageExecutionFailure> {
    let CandidateSearchArtifactResolutionInput {
        run_spec,
        stage_input,
        resolved_stage_payloads,
        prepared_bootstrap,
        baseline_evaluation_artifacts,
        progress_sender,
        resume_store,
        disk_checkpoint_directory,
        pipeline_stage,
        upstream_joint_refinement_handoff,
        upstream_option_refinement_handoff,
    } = input;

    match pipeline_stage {
        JointRefinementPipelineStage::JointRefinement => run_candidate_search(
            stage_input,
            resolved_stage_payloads,
            prepared_bootstrap,
            baseline_evaluation_artifacts,
            progress_sender,
            resume_store,
        ),
        JointRefinementPipelineStage::OptionRefinement => {
            candidate_search_artifacts_from_upstream_or_resume(
                run_spec,
                disk_checkpoint_directory,
                CalibrationStage::JointRefinement,
                upstream_joint_refinement_handoff,
                baseline_evaluation_artifacts,
                &prepared_bootstrap.bootstrap_context,
            )
        }
        JointRefinementPipelineStage::Finalization => {
            if run_spec
                .selected_stages
                .contains(&CalibrationStage::OptionRefinement)
            {
                return candidate_search_artifacts_from_upstream_or_resume(
                    run_spec,
                    disk_checkpoint_directory,
                    CalibrationStage::OptionRefinement,
                    upstream_option_refinement_handoff,
                    baseline_evaluation_artifacts,
                    &prepared_bootstrap.bootstrap_context,
                );
            }

            candidate_search_artifacts_from_upstream_or_resume(
                run_spec,
                disk_checkpoint_directory,
                CalibrationStage::JointRefinement,
                upstream_joint_refinement_handoff,
                baseline_evaluation_artifacts,
                &prepared_bootstrap.bootstrap_context,
            )
        }
    }
}

pub(super) fn should_run_option_pricing_stage(
    pipeline_stage: JointRefinementPipelineStage,
) -> bool {
    match pipeline_stage {
        JointRefinementPipelineStage::OptionRefinement => true,
        JointRefinementPipelineStage::JointRefinement
        | JointRefinementPipelineStage::Finalization => false,
    }
}

pub(super) fn run_option_refinement_if_enabled(
    stage_input: &JointRefinementStageExecutionInput,
    bootstrap_context: &JointRefinementContext,
    candidate_search_artifacts: &mut CandidateSearchArtifacts,
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    resume_store: &mut JointRefinementResumeStore,
) -> Result<(), JointRefinementStageExecutionFailure> {
    if stage_input.option_pricing_input.is_none() {
        return Ok(());
    }

    for candidate_outcome in &candidate_search_artifacts.option_refinement_candidate_frontier {
        resume_store.record_candidate_outcome(candidate_outcome);
    }

    let option_context_fingerprint = require_option_context_fingerprint(bootstrap_context)?;

    if let Some((selected_candidate_id, search_tier_artifact, selection_tier_artifact)) =
        resume_store.selected_option_refinement(option_context_fingerprint.as_str())
        && let Some(selected_candidate_outcome) = candidate_search_artifacts
            .option_refinement_candidate_frontier
            .iter()
            .find(|candidate_outcome| candidate_outcome.candidate_id == selected_candidate_id)
            .cloned()
    {
        candidate_search_artifacts.best_candidate = selected_candidate_outcome;

        candidate_search_artifacts.selected_option_search_tier_pricing_artifact =
            Some(search_tier_artifact);
        candidate_search_artifacts.selected_option_selection_tier_pricing_artifact =
            Some(selection_tier_artifact);

        return Ok(());
    }

    let option_refinement_result = run_option_refinement(
        bootstrap_context,
        candidate_search_artifacts
            .option_refinement_candidate_frontier
            .as_slice(),
        stage_input.option_refinement_candidate_count_override,
        stage_input.option_refinement_wall_clock_budget_millis,
        option_context_fingerprint.as_str(),
        progress_sender,
        Some(resume_store),
    )?;

    candidate_search_artifacts.best_candidate = option_refinement_result.selected_candidate_outcome;

    candidate_search_artifacts.selected_option_search_tier_pricing_artifact =
        Some(option_refinement_result.selected_search_tier_pricing_artifact);

    candidate_search_artifacts.selected_option_selection_tier_pricing_artifact =
        Some(option_refinement_result.selected_selection_tier_pricing_artifact);

    candidate_search_artifacts.option_search_selection_agreement =
        option_refinement_result.search_selection_agreement;

    if let (Some(search_artifact), Some(selection_artifact)) = (
        candidate_search_artifacts
            .selected_option_search_tier_pricing_artifact
            .as_ref(),
        candidate_search_artifacts
            .selected_option_selection_tier_pricing_artifact
            .as_ref(),
    ) {
        resume_store.record_selected_option_refinement(
            option_context_fingerprint.as_str(),
            candidate_search_artifacts
                .best_candidate
                .candidate_id
                .as_str(),
            search_artifact.as_ref(),
            selection_artifact.as_ref(),
        );
    }

    Ok(())
}

pub(super) fn run_audit_option_pricing_if_enabled(
    stage_input: &JointRefinementStageExecutionInput,
    bootstrap_context: &JointRefinementContext,
    candidate_search_artifacts: &mut CandidateSearchArtifacts,
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    resume_store: &mut JointRefinementResumeStore,
) -> Result<(), JointRefinementStageExecutionFailure> {
    if !stage_input.audit_option_pricing_enabled {
        return Ok(());
    }

    let option_context_fingerprint = require_option_context_fingerprint(bootstrap_context)?;

    if let Some(audit_artifact) =
        resume_store.selected_audit_tier_pricing_artifact(option_context_fingerprint.as_str())
    {
        candidate_search_artifacts.selected_option_audit_tier_pricing_artifact =
            Some(audit_artifact);
        return Ok(());
    }

    let selection_artifact = candidate_search_artifacts
        .selected_option_selection_tier_pricing_artifact
        .as_ref()
        .ok_or_else(|| {
            JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
                error_message:
                    "joint option audit pricing requires the selected selection-tier pricing artifact"
                        .to_string(),
            }
        })?;

    let audit_artifact = resolve_option_surface_tier_pricing_artifact(
        OptionSurfaceTierPricingArtifactResolutionInput {
            context: bootstrap_context,
            fitted_parameters: &candidate_search_artifacts
                .best_candidate
                .final_evaluation
                .fitted_parameters,
            tier_kind: JointRefinementOptionSurfaceTierKind::Audit,
            source_dynamics_parameter_bits: selection_artifact
                .cache_key
                .source_dynamics_parameter_bits
                .as_slice(),
            pricing_parameter_bits: selection_artifact
                .cache_key
                .pricing_parameter_bits
                .as_slice(),
            include_node_price_sensitivities: false,
            theta_warm_start_state: None,
            progress: OptionPricingProgressInput {
                progress_sender,
                progress_phase: Some(JointRefinementStageProgressPhase::OptionAuditTierEvaluation),
                completed_units: Some(0),
                total_units: Some(1),
                ..OptionPricingProgressInput::none()
            },
        },
    )
    .map_err(|error_message| {
        JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput { error_message }
    })?;

    candidate_search_artifacts.selected_option_audit_tier_pricing_artifact =
        Some(audit_artifact.clone());

    resume_store.record_audit_tier_pricing_artifact(
        option_context_fingerprint.as_str(),
        audit_artifact.as_ref(),
    );

    Ok(())
}

pub(in crate::stages::joint_refinement::execution) fn require_option_context_fingerprint(
    bootstrap_context: &JointRefinementContext,
) -> Result<String, JointRefinementStageExecutionFailure> {
    bootstrap_context
        .option_pricing_context
        .as_ref()
        .map(option_pricing_context_fingerprint)
        .ok_or_else(
            || JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
                error_message: "joint option refinement requires an option pricing context"
                    .to_string(),
            },
        )
}
