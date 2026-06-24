use std::collections::BTreeMap;
use std::path::Path;

use janiform_calibration_artifacts::{
    CalibrationRunSpec, CalibrationStage, resolve_calibration_stage_seed_metadata,
};

use crate::stages::joint_refinement::contracts::JointRefinementStageExecutionFailure;

use super::pipeline::{
    BaselineEvaluationArtifacts, RestartSearchArtifacts, build_selectable_restart_outcomes,
    count_admissible_restart_outcomes, count_valid_restart_outcomes,
    ensure_valid_selected_restart_outcome, require_option_context_fingerprint,
};
use super::state::JointRefinementStageRuntimeHandoff;
use super::{JointRefinementContext, JointRefinementRestartOutcome, JointRefinementResumeStore};

pub(super) fn restart_search_artifacts_from_upstream_or_resume(
    run_spec: &CalibrationRunSpec,
    disk_checkpoint_directory: Option<&Path>,
    upstream_stage: CalibrationStage,
    upstream_handoff: Option<&JointRefinementStageRuntimeHandoff>,
    baseline_evaluation_artifacts: &BaselineEvaluationArtifacts,
    bootstrap_context: &JointRefinementContext,
) -> Result<RestartSearchArtifacts, JointRefinementStageExecutionFailure> {
    if let Some(handoff) = upstream_handoff {
        return Ok(restart_search_artifacts_from_runtime_handoff(handoff));
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
        CalibrationStage::JointRefinement => restart_search_artifacts_from_dynamics_resume_store(
            &upstream_resume_store,
            baseline_evaluation_artifacts,
        ),
        CalibrationStage::OptionRefinement => restart_search_artifacts_from_option_resume_store(
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

fn restart_search_artifacts_from_runtime_handoff(
    handoff: &JointRefinementStageRuntimeHandoff,
) -> RestartSearchArtifacts {
    // The downstream stage mutates restart artifacts, so the handoff is copied into owned state.
    RestartSearchArtifacts {
        restart_outcomes: handoff.restart_outcomes.clone(),
        restart_evaluation_timing_diagnostics: handoff
            .restart_evaluation_timing_diagnostics
            .clone(),
        valid_restart_count: handoff.valid_restart_count,
        best_restart: handoff.best_restart.clone(),
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
        coarse_consistency_artifacts_by_restart_id: BTreeMap::new(),
        warning: handoff.warning.clone(),
        population_refinement_wall_clock_millis: handoff.population_refinement_wall_clock_millis,
        coarse_consistency_wall_clock_millis: handoff.coarse_consistency_wall_clock_millis,
    }
}

fn restart_search_artifacts_from_dynamics_resume_store(
    resume_store: &JointRefinementResumeStore,
    baseline_evaluation_artifacts: &BaselineEvaluationArtifacts,
) -> Result<RestartSearchArtifacts, JointRefinementStageExecutionFailure> {
    let restart_outcomes = resume_store.restart_outcomes();
    let restart_selection = resume_store.restart_selection().cloned().ok_or_else(|| {
        JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
            error_message: "joint refinement resume state is missing restart selection".to_string(),
        }
    })?;

    let selectable_restart_outcomes = build_selectable_restart_outcomes(
        restart_outcomes.as_slice(),
        &baseline_evaluation_artifacts.baseline_restart_outcome,
    );

    let best_restart = restart_outcome_by_id(
        selectable_restart_outcomes.as_slice(),
        restart_selection.best_restart_id.as_str(),
    )?;

    let option_refinement_candidate_frontier = restart_selection
        .option_refinement_candidate_frontier_ids
        .iter()
        .filter_map(|restart_id| {
            selectable_restart_outcomes
                .iter()
                .find(|restart_outcome| restart_outcome.restart_id == *restart_id)
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

    Ok(RestartSearchArtifacts {
        valid_restart_count: count_valid_restart_outcomes(restart_outcomes.as_slice()),
        restart_outcomes,
        restart_evaluation_timing_diagnostics: Vec::new(),
        best_restart,
        option_refinement_candidate_frontier,
        selected_option_search_tier_pricing_artifact: None,
        selected_option_selection_tier_pricing_artifact: None,
        selected_option_audit_tier_pricing_artifact: None,
        option_search_selection_agreement: None,
        coarse_consistency_artifacts_by_restart_id: restart_selection
            .coarse_consistency_artifacts_by_restart_id,
        warning: resumed_selection_warning(
            &restart_selection.best_restart_id,
            selectable_restart_outcomes.as_slice(),
        ),
        population_refinement_wall_clock_millis: 0,
        coarse_consistency_wall_clock_millis: restart_selection
            .coarse_consistency_wall_clock_millis,
    })
}

fn restart_search_artifacts_from_option_resume_store(
    resume_store: &JointRefinementResumeStore,
    bootstrap_context: &JointRefinementContext,
) -> Result<RestartSearchArtifacts, JointRefinementStageExecutionFailure> {
    let option_context_fingerprint = require_option_context_fingerprint(bootstrap_context)?;

    let Some((selected_restart_id, search_artifact, selection_artifact)) =
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
            selected_restart_id.as_str(),
        )
        .ok_or_else(
            || JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
                error_message:
                    "option refinement resume state is missing selected candidate details"
                        .to_string(),
            },
        )?;

    let mut restart_outcomes = resume_store.restart_outcomes();

    // The selected resume candidate must remain available both as best restart and in the frontier.
    if let Some(position) = restart_outcomes
        .iter()
        .position(|restart_outcome| restart_outcome.restart_id == selected_restart_id)
    {
        restart_outcomes[position] = selected_candidate.restart_outcome.clone();
    } else {
        restart_outcomes.push(selected_candidate.restart_outcome.clone());
    }

    let option_refinement_candidate_frontier = restart_outcomes.clone();

    let audit_artifact =
        resume_store.selected_audit_tier_pricing_artifact(option_context_fingerprint.as_str());

    Ok(RestartSearchArtifacts {
        valid_restart_count: count_valid_restart_outcomes(restart_outcomes.as_slice()),
        restart_outcomes,
        restart_evaluation_timing_diagnostics: Vec::new(),
        best_restart: selected_candidate.restart_outcome,
        option_refinement_candidate_frontier,
        selected_option_search_tier_pricing_artifact: Some(search_artifact),
        selected_option_selection_tier_pricing_artifact: Some(selection_artifact),
        selected_option_audit_tier_pricing_artifact: audit_artifact,
        option_search_selection_agreement: None,
        coarse_consistency_artifacts_by_restart_id: BTreeMap::new(),
        warning: None,
        population_refinement_wall_clock_millis: 0,
        coarse_consistency_wall_clock_millis: 0,
    })
}

fn restart_outcome_by_id(
    restart_outcomes: &[JointRefinementRestartOutcome],
    restart_id: &str,
) -> Result<JointRefinementRestartOutcome, JointRefinementStageExecutionFailure> {
    restart_outcomes
        .iter()
        .find(|restart_outcome| restart_outcome.restart_id == restart_id)
        .cloned()
        .ok_or_else(
            || JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
                error_message: format!(
                    "joint refinement resume state references missing restart `{restart_id}`"
                ),
            },
        )
}

fn resumed_selection_warning(
    selected_restart_id: &str,
    selectable_restart_outcomes: &[JointRefinementRestartOutcome],
) -> Option<String> {
    let selected_restart = selectable_restart_outcomes
        .iter()
        .find(|restart_outcome| restart_outcome.restart_id == selected_restart_id)?;

    if selected_restart
        .final_evaluation
        .breakdown
        .admissibility
        .admissible
    {
        return None;
    }

    let valid_restart_count = count_valid_restart_outcomes(selectable_restart_outcomes);

    let restart_refs = selectable_restart_outcomes.iter().collect::<Vec<_>>();

    ensure_valid_selected_restart_outcome(
        selected_restart,
        valid_restart_count,
        count_admissible_restart_outcomes(restart_refs.as_slice()),
        false,
    )
    .ok()
    .flatten()
}
