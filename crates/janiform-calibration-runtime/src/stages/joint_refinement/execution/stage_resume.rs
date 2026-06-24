use std::collections::BTreeMap;
use std::path::Path;

use janiform_calibration_artifacts::{
    CalibrationRunSpec, CalibrationStage, resolve_calibration_stage_seed_metadata,
};

use crate::stages::joint_refinement::contracts::JointRefinementStageExecutionFailure;

use super::pipeline::{
    BaselineEvaluationArtifacts, CandidateSearchArtifacts, build_selectable_candidate_outcomes,
    count_admissible_candidate_outcomes, count_valid_candidate_outcomes,
    ensure_valid_selected_candidate_outcome, require_option_context_fingerprint,
};
use super::state::JointRefinementStageRuntimeHandoff;
use super::{JointRefinementCandidateOutcome, JointRefinementContext, JointRefinementResumeStore};

pub(super) fn candidate_search_artifacts_from_upstream_or_resume(
    run_spec: &CalibrationRunSpec,
    disk_checkpoint_directory: Option<&Path>,
    upstream_stage: CalibrationStage,
    upstream_handoff: Option<&JointRefinementStageRuntimeHandoff>,
    baseline_evaluation_artifacts: &BaselineEvaluationArtifacts,
    bootstrap_context: &JointRefinementContext,
) -> Result<CandidateSearchArtifacts, JointRefinementStageExecutionFailure> {
    if let Some(handoff) = upstream_handoff {
        return Ok(candidate_search_artifacts_from_runtime_handoff(handoff));
    }

    let Some(disk_checkpoint_directory) = disk_checkpoint_directory else {
        return Err(
            JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
                error_message: format!(
                    "{upstream_stage:?} runtime handoff is required for this stage"
                ),
            },
        );
    };

    let stage_seed = resolve_calibration_stage_seed_metadata(run_spec, upstream_stage).stage_seed;
    let upstream_resume_store = JointRefinementResumeStore::load(
        Some(disk_checkpoint_directory),
        run_spec.run_id.as_str(),
        upstream_stage,
        stage_seed,
    );

    match upstream_stage {
        CalibrationStage::JointRefinement => candidate_search_artifacts_from_dynamics_resume_store(
            &upstream_resume_store,
            baseline_evaluation_artifacts,
        ),
        CalibrationStage::OptionRefinement => candidate_search_artifacts_from_option_resume_store(
            &upstream_resume_store,
            bootstrap_context,
        ),
        _ => Err(
            JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
                error_message: format!(
                    "{upstream_stage:?} cannot provide joint-refinement runtime handoff"
                ),
            },
        ),
    }
}

fn candidate_search_artifacts_from_runtime_handoff(
    handoff: &JointRefinementStageRuntimeHandoff,
) -> CandidateSearchArtifacts {
    // The downstream stage mutates candidate artifacts, so the handoff is copied into owned state.
    CandidateSearchArtifacts {
        candidate_outcomes: handoff.candidate_outcomes.clone(),
        candidate_evaluation_timing_diagnostics: handoff
            .candidate_evaluation_timing_diagnostics
            .clone(),
        valid_candidate_count: handoff.valid_candidate_count,
        best_candidate: handoff.best_candidate.clone(),
        option_refinement_candidate_frontier: handoff.option_refinement_candidate_frontier.clone(),
        selected_option_search_tier_pricing_artifact: handoff
            .selected_option_search_tier_pricing_artifact
            .clone(),
        selected_option_selection_tier_pricing_artifact: handoff
            .selected_option_selection_tier_pricing_artifact
            .clone(),
        selected_option_audit_tier_pricing_artifact: handoff
            .selected_option_audit_tier_pricing_artifact
            .clone(),
        option_search_selection_agreement: handoff.option_search_selection_agreement.clone(),
        coarse_consistency_artifacts_by_candidate_id: BTreeMap::new(),
        warning: handoff.warning.clone(),
        population_refinement_wall_clock_millis: handoff.population_refinement_wall_clock_millis,
        coarse_consistency_wall_clock_millis: handoff.coarse_consistency_wall_clock_millis,
    }
}

fn candidate_search_artifacts_from_dynamics_resume_store(
    resume_store: &JointRefinementResumeStore,
    baseline_evaluation_artifacts: &BaselineEvaluationArtifacts,
) -> Result<CandidateSearchArtifacts, JointRefinementStageExecutionFailure> {
    let candidate_outcomes = resume_store.candidate_outcomes();
    let candidate_selection = resume_store.candidate_selection().cloned().ok_or_else(|| {
        JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
            error_message: "joint refinement resume state is missing candidate selection"
                .to_string(),
        }
    })?;

    let selectable_candidate_outcomes = build_selectable_candidate_outcomes(
        candidate_outcomes.as_slice(),
        &baseline_evaluation_artifacts.baseline_candidate_outcome,
    );

    let best_candidate = candidate_outcome_by_id(
        selectable_candidate_outcomes.as_slice(),
        candidate_selection.best_candidate_id.as_str(),
    )?;

    let option_refinement_candidate_frontier = candidate_selection
        .option_refinement_candidate_frontier_ids
        .iter()
        .filter_map(|candidate_id| {
            selectable_candidate_outcomes
                .iter()
                .find(|candidate_outcome| candidate_outcome.candidate_id == *candidate_id)
                .cloned()
        })
        .collect::<Vec<_>>();

    if option_refinement_candidate_frontier.is_empty() {
        return Err(
            JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
                error_message:
                    "joint refinement resume state is missing option refinement candidates"
                        .to_string(),
            },
        );
    }

    Ok(CandidateSearchArtifacts {
        valid_candidate_count: count_valid_candidate_outcomes(candidate_outcomes.as_slice()),
        candidate_outcomes,
        candidate_evaluation_timing_diagnostics: Vec::new(),
        best_candidate,
        option_refinement_candidate_frontier,
        selected_option_search_tier_pricing_artifact: None,
        selected_option_selection_tier_pricing_artifact: None,
        selected_option_audit_tier_pricing_artifact: None,
        option_search_selection_agreement: None,
        coarse_consistency_artifacts_by_candidate_id: candidate_selection
            .coarse_consistency_artifacts_by_candidate_id,
        warning: resumed_selection_warning(
            &candidate_selection.best_candidate_id,
            selectable_candidate_outcomes.as_slice(),
        ),
        population_refinement_wall_clock_millis: 0,
        coarse_consistency_wall_clock_millis: candidate_selection
            .coarse_consistency_wall_clock_millis,
    })
}

fn candidate_search_artifacts_from_option_resume_store(
    resume_store: &JointRefinementResumeStore,
    bootstrap_context: &JointRefinementContext,
) -> Result<CandidateSearchArtifacts, JointRefinementStageExecutionFailure> {
    let option_context_fingerprint = require_option_context_fingerprint(bootstrap_context)?;

    let Some((selected_candidate_id, search_artifact, selection_artifact)) =
        resume_store.selected_option_refinement(option_context_fingerprint.as_str())
    else {
        return Err(
            JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
                error_message: "option refinement resume state is missing selected option pricing"
                    .to_string(),
            },
        );
    };

    let selected_candidate = resume_store
        .selection_tier_candidate(
            option_context_fingerprint.as_str(),
            selected_candidate_id.as_str(),
        )
        .ok_or_else(
            || JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
                error_message:
                    "option refinement resume state is missing selected candidate details"
                        .to_string(),
            },
        )?;

    let mut candidate_outcomes = resume_store.candidate_outcomes();

    // The selected resume candidate must remain available both as best candidate and in the frontier.
    if let Some(position) = candidate_outcomes
        .iter()
        .position(|candidate_outcome| candidate_outcome.candidate_id == selected_candidate_id)
    {
        candidate_outcomes[position] = selected_candidate.candidate_outcome.clone();
    } else {
        candidate_outcomes.push(selected_candidate.candidate_outcome.clone());
    }

    let option_refinement_candidate_frontier = candidate_outcomes.clone();

    let audit_artifact =
        resume_store.selected_audit_tier_pricing_artifact(option_context_fingerprint.as_str());

    Ok(CandidateSearchArtifacts {
        valid_candidate_count: count_valid_candidate_outcomes(candidate_outcomes.as_slice()),
        candidate_outcomes,
        candidate_evaluation_timing_diagnostics: Vec::new(),
        best_candidate: selected_candidate.candidate_outcome,
        option_refinement_candidate_frontier,
        selected_option_search_tier_pricing_artifact: Some(search_artifact),
        selected_option_selection_tier_pricing_artifact: Some(selection_artifact),
        selected_option_audit_tier_pricing_artifact: audit_artifact,
        option_search_selection_agreement: None,
        coarse_consistency_artifacts_by_candidate_id: BTreeMap::new(),
        warning: None,
        population_refinement_wall_clock_millis: 0,
        coarse_consistency_wall_clock_millis: 0,
    })
}

fn candidate_outcome_by_id(
    candidate_outcomes: &[JointRefinementCandidateOutcome],
    candidate_id: &str,
) -> Result<JointRefinementCandidateOutcome, JointRefinementStageExecutionFailure> {
    candidate_outcomes
        .iter()
        .find(|candidate_outcome| candidate_outcome.candidate_id == candidate_id)
        .cloned()
        .ok_or_else(
            || JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
                error_message: format!(
                    "joint refinement resume state references missing candidate `{candidate_id}`"
                ),
            },
        )
}

fn resumed_selection_warning(
    selected_candidate_id: &str,
    selectable_candidate_outcomes: &[JointRefinementCandidateOutcome],
) -> Option<String> {
    let selected_candidate = selectable_candidate_outcomes
        .iter()
        .find(|candidate_outcome| candidate_outcome.candidate_id == selected_candidate_id)?;

    if selected_candidate
        .final_evaluation
        .breakdown
        .admissibility
        .admissible
    {
        return None;
    }

    let valid_candidate_count = count_valid_candidate_outcomes(selectable_candidate_outcomes);

    let candidate_refs = selectable_candidate_outcomes.iter().collect::<Vec<_>>();

    ensure_valid_selected_candidate_outcome(
        selected_candidate,
        valid_candidate_count,
        count_admissible_candidate_outcomes(candidate_refs.as_slice()),
        false,
    )
    .ok()
    .flatten()
}
