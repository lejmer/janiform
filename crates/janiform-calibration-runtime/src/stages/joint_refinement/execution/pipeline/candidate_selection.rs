use std::collections::BTreeSet;
use std::sync::mpsc::Sender;
use std::time::Instant;

use crate::stages::joint_refinement::contracts::{
    JointRefinementStageExecutionFailure, JointRefinementStageExecutionInput,
    JointRefinementStageProgressUpdate,
};

use super::super::selection::JointRefinementLazyCoarseConsistencySelection;
use super::super::{
    JointRefinementCandidateOutcome, JointRefinementResumeStore,
    PersistedCandidateSelectionArtifacts,
    desired_joint_refinement_option_refinement_candidate_count, elapsed_millis,
    joint_candidate_outcome_is_admissible, option_pricing_context_fingerprint,
    select_candidate_outcomes_with_lazy_coarse_consistency,
};
use super::baseline::ensure_valid_selected_candidate_outcome;
use super::progress::emit_candidate_screening_progress;
use super::types::{
    BaselineEvaluationArtifacts, CandidateSelectionArtifacts, PreparedBootstrapContext,
};

pub(in crate::stages::joint_refinement::execution) fn count_admissible_candidate_outcomes(
    candidate_outcomes: &[&JointRefinementCandidateOutcome],
) -> usize {
    candidate_outcomes
        .iter()
        .copied()
        .filter(|candidate_outcome| joint_candidate_outcome_is_admissible(candidate_outcome))
        .count()
}

fn filter_admissible_candidate_outcomes<'a>(
    candidate_outcomes: &[&'a JointRefinementCandidateOutcome],
) -> Vec<&'a JointRefinementCandidateOutcome> {
    candidate_outcomes
        .iter()
        .copied()
        .filter(|candidate_outcome| joint_candidate_outcome_is_admissible(candidate_outcome))
        .collect()
}

pub(super) fn select_candidate_outcome(
    stage_input: &JointRefinementStageExecutionInput,
    prepared_bootstrap: &PreparedBootstrapContext,
    baseline_evaluation_artifacts: &BaselineEvaluationArtifacts,
    candidate_outcomes: &[JointRefinementCandidateOutcome],
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    resume_store: &mut JointRefinementResumeStore,
) -> Result<CandidateSelectionArtifacts, JointRefinementStageExecutionFailure> {
    emit_candidate_screening_progress(progress_sender, 0, 4, "build selectable candidate outcomes");

    let selectable_candidate_outcomes = build_selectable_candidate_outcomes(
        candidate_outcomes,
        &baseline_evaluation_artifacts.baseline_candidate_outcome,
    );

    let valid_selectable_candidate_outcome_refs =
        valid_selectable_candidate_outcome_refs(selectable_candidate_outcomes.as_slice());

    emit_candidate_screening_progress(progress_sender, 1, 4, "rank valid candidate outcomes");

    let coarse_consistency_started_at = Instant::now();
    let selected_candidate_count = selected_candidate_count(stage_input, prepared_bootstrap);
    let option_context_fingerprint = option_context_fingerprint_for_selection(prepared_bootstrap);

    let persisted_selection = persisted_candidate_selection_for_context(
        resume_store,
        valid_selectable_candidate_outcome_refs.as_slice(),
        selected_candidate_count,
        option_context_fingerprint.as_deref(),
    );

    emit_candidate_screening_progress(progress_sender, 2, 4, "resolve coarse consistency frontier");

    let selection_artifacts = resolve_candidate_selection_artifacts(
        valid_selectable_candidate_outcome_refs.as_slice(),
        prepared_bootstrap,
        baseline_evaluation_artifacts,
        selected_candidate_count,
        persisted_selection,
        progress_sender,
    );

    emit_candidate_screening_progress(progress_sender, 3, 4, "select best candidate");

    let diagnostic_best_candidate =
        diagnostic_best_candidate(&selection_artifacts, baseline_evaluation_artifacts);

    let option_refinement_candidate_frontier =
        option_refinement_candidate_frontier_from_selected_outcomes(
            selection_artifacts.selected_candidate_outcomes.as_slice(),
        );

    let warning = ensure_valid_selected_candidate_outcome(
        &diagnostic_best_candidate,
        valid_selectable_candidate_outcome_refs.len(),
        count_admissible_candidate_outcomes(valid_selectable_candidate_outcome_refs.as_slice()),
        stage_input.treat_invalid_candidate_fallback_warning_as_error,
    )?;

    let best_candidate = best_candidate_for_selection(
        warning.as_ref(),
        baseline_evaluation_artifacts,
        option_refinement_candidate_frontier.as_slice(),
        &diagnostic_best_candidate,
    );

    record_candidate_selection_artifacts(
        resume_store,
        &best_candidate,
        option_refinement_candidate_frontier.as_slice(),
        selected_candidate_count,
        option_context_fingerprint,
        &selection_artifacts,
        coarse_consistency_started_at,
    );

    emit_candidate_screening_progress(progress_sender, 4, 4, "candidate selection ready");

    Ok(CandidateSelectionArtifacts {
        best_candidate,
        option_refinement_candidate_frontier,
        coarse_consistency_artifacts_by_candidate_id: selection_artifacts
            .coarse_consistency_artifacts_by_candidate_id,
        warning,
        coarse_consistency_wall_clock_millis: elapsed_millis(coarse_consistency_started_at),
    })
}

fn valid_selectable_candidate_outcome_refs(
    selectable_candidate_outcomes: &[JointRefinementCandidateOutcome],
) -> Vec<&JointRefinementCandidateOutcome> {
    selectable_candidate_outcomes
        .iter()
        .filter(|candidate_outcome| {
            candidate_outcome.local_evaluation.invalid_reason.is_none()
                && candidate_outcome.final_evaluation.invalid_reason.is_none()
        })
        .collect()
}

fn selected_candidate_count(
    stage_input: &JointRefinementStageExecutionInput,
    prepared_bootstrap: &PreparedBootstrapContext,
) -> usize {
    if prepared_bootstrap
        .bootstrap_context
        .option_pricing_context
        .is_some()
    {
        return desired_joint_refinement_option_refinement_candidate_count(
            stage_input.option_refinement_candidate_count_override,
        );
    }

    1
}

fn option_context_fingerprint_for_selection(
    prepared_bootstrap: &PreparedBootstrapContext,
) -> Option<String> {
    prepared_bootstrap
        .bootstrap_context
        .option_pricing_context
        .as_ref()
        .map(option_pricing_context_fingerprint)
}

fn persisted_candidate_selection_for_context(
    resume_store: &JointRefinementResumeStore,
    valid_selectable_candidate_outcome_refs: &[&JointRefinementCandidateOutcome],
    selected_candidate_count: usize,
    option_context_fingerprint: Option<&str>,
) -> Option<PersistedCandidateSelectionArtifacts> {
    let valid_candidate_ids = valid_selectable_candidate_outcome_refs
        .iter()
        .map(|candidate_outcome| candidate_outcome.candidate_id.clone())
        .collect::<BTreeSet<_>>();

    resume_store
        .candidate_selection()
        .cloned()
        .filter(|selection| {
            selection.matches_resume_context(
                &valid_candidate_ids,
                selected_candidate_count,
                option_context_fingerprint,
            )
        })
}

fn resolve_candidate_selection_artifacts<'a>(
    valid_selectable_candidate_outcome_refs: &'a [&'a JointRefinementCandidateOutcome],
    prepared_bootstrap: &'a PreparedBootstrapContext,
    baseline_evaluation_artifacts: &'a BaselineEvaluationArtifacts,
    selected_candidate_count: usize,
    persisted_selection: Option<PersistedCandidateSelectionArtifacts>,
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
) -> JointRefinementLazyCoarseConsistencySelection<'a> {
    persisted_selection.map_or_else(
        || {
            select_candidate_outcomes_with_lazy_coarse_consistency(
                valid_selectable_candidate_outcome_refs,
                &prepared_bootstrap.bootstrap_context,
                baseline_evaluation_artifacts
                    .baseline_coarse_consistency_artifacts
                    .as_ref(),
                selected_candidate_count,
                progress_sender,
            )
        },
        |selection| JointRefinementLazyCoarseConsistencySelection {
            selected_candidate_outcomes: selection
                .option_refinement_candidate_frontier_ids
                .iter()
                .filter_map(|candidate_id| {
                    valid_selectable_candidate_outcome_refs
                        .iter()
                        .copied()
                        .find(|candidate_outcome| candidate_outcome.candidate_id == *candidate_id)
                })
                .collect(),
            coarse_consistency_artifacts_by_candidate_id: selection
                .coarse_consistency_artifacts_by_candidate_id,
        },
    )
}

fn diagnostic_best_candidate(
    selection_artifacts: &JointRefinementLazyCoarseConsistencySelection<'_>,
    baseline_evaluation_artifacts: &BaselineEvaluationArtifacts,
) -> JointRefinementCandidateOutcome {
    selection_artifacts
        .selected_candidate_outcomes
        .first()
        .copied()
        .cloned()
        .unwrap_or_else(|| {
            baseline_evaluation_artifacts
                .baseline_candidate_outcome
                .clone()
        })
}

fn best_candidate_for_selection(
    warning: Option<&String>,
    baseline_evaluation_artifacts: &BaselineEvaluationArtifacts,
    option_refinement_candidate_frontier: &[JointRefinementCandidateOutcome],
    diagnostic_best_candidate: &JointRefinementCandidateOutcome,
) -> JointRefinementCandidateOutcome {
    if warning.is_some() {
        return baseline_evaluation_artifacts
            .baseline_candidate_outcome
            .clone();
    }

    option_refinement_candidate_frontier
        .first()
        .cloned()
        .unwrap_or_else(|| diagnostic_best_candidate.clone())
}

fn record_candidate_selection_artifacts(
    resume_store: &mut JointRefinementResumeStore,
    best_candidate: &JointRefinementCandidateOutcome,
    option_refinement_candidate_frontier: &[JointRefinementCandidateOutcome],
    selected_candidate_count: usize,
    option_context_fingerprint: Option<String>,
    selection_artifacts: &JointRefinementLazyCoarseConsistencySelection<'_>,
    coarse_consistency_started_at: Instant,
) {
    resume_store.record_candidate_selection(PersistedCandidateSelectionArtifacts {
        best_candidate_id: best_candidate.candidate_id.clone(),
        option_refinement_candidate_frontier_ids: option_refinement_candidate_frontier
            .iter()
            .map(|candidate_outcome| candidate_outcome.candidate_id.clone())
            .collect(),
        selected_candidate_count,
        option_pricing_context_fingerprint: option_context_fingerprint,
        coarse_consistency_artifacts_by_candidate_id: selection_artifacts
            .coarse_consistency_artifacts_by_candidate_id
            .clone(),
        coarse_consistency_wall_clock_millis: elapsed_millis(coarse_consistency_started_at),
    });
}

pub(crate) fn option_refinement_candidate_frontier_from_selected_outcomes(
    selected_candidate_outcomes: &[&JointRefinementCandidateOutcome],
) -> Vec<JointRefinementCandidateOutcome> {
    let admissible_outcomes = filter_admissible_candidate_outcomes(selected_candidate_outcomes);
    admissible_outcomes.into_iter().cloned().collect()
}

pub(in crate::stages::joint_refinement::execution) fn build_selectable_candidate_outcomes(
    candidate_outcomes: &[JointRefinementCandidateOutcome],
    baseline_candidate_outcome: &JointRefinementCandidateOutcome,
) -> Vec<JointRefinementCandidateOutcome> {
    let mut selectable_candidate_outcomes = Vec::with_capacity(candidate_outcomes.len() + 1);

    selectable_candidate_outcomes.push(baseline_candidate_outcome.clone());
    selectable_candidate_outcomes.extend(candidate_outcomes.iter().cloned());

    selectable_candidate_outcomes
}
