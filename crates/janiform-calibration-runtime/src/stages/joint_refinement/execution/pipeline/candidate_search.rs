use std::collections::BTreeMap;
use std::sync::mpsc::Sender;
use std::time::Instant;

use crate::stages::joint_refinement::contracts::{
    JointRefinementStageExecutionFailure, JointRefinementStageExecutionInput,
    JointRefinementStageProgressUpdate,
};

use super::super::super::setup::{
    CandidateSeedsBuildInput, build_candidate_seeds, observed_activity_log_shift,
    observed_activity_log_shift_for_joint_context, selected_return_components_candidate,
};
use super::super::{
    JointRefinementCandidateOutcome, JointRefinementResumeStore,
    desired_joint_refinement_candidate_budget, elapsed_millis, joint_refinement_budget_schedule,
    optimize_candidate_seeds,
};
use super::candidate_selection::select_candidate_outcome;
use super::infrastructure::ensure_simulation_infrastructure_available;
use super::types::{
    BaselineEvaluationArtifacts, CandidateSearchArtifacts, CandidateSearchPlan,
    PreparedBootstrapContext, ResolvedStagePayloads,
};

pub(super) fn run_candidate_search(
    stage_input: &JointRefinementStageExecutionInput,
    resolved_stage_payloads: &ResolvedStagePayloads<'_>,
    prepared_bootstrap: &PreparedBootstrapContext,
    baseline_evaluation_artifacts: &BaselineEvaluationArtifacts,
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    resume_store: &mut JointRefinementResumeStore,
) -> Result<CandidateSearchArtifacts, JointRefinementStageExecutionFailure> {
    let candidate_search_plan = build_candidate_search_plan(
        stage_input,
        resolved_stage_payloads,
        prepared_bootstrap,
        baseline_evaluation_artifacts,
        progress_sender,
    );

    let persisted_candidate_outcomes = candidate_search_plan
        .candidate_seeds
        .iter()
        .filter_map(|(candidate_id, _)| {
            resume_store
                .candidate_outcome(candidate_id.as_str())
                .map(|candidate_outcome| (candidate_id.clone(), candidate_outcome))
        })
        .collect::<BTreeMap<_, _>>();

    let remaining_candidate_seeds = candidate_search_plan
        .candidate_seeds
        .iter()
        .filter(|(candidate_id, _)| {
            !persisted_candidate_outcomes.contains_key(candidate_id.as_str())
        })
        .cloned()
        .collect::<Vec<_>>();

    let population_refinement_started_at = Instant::now();

    let remaining_population_refinement_output = if remaining_candidate_seeds.is_empty() {
        super::super::optimization::JointRefinementPopulationRefinementOutput {
            candidate_outcomes: Vec::new(),
            evaluation_timing_diagnostics: Vec::new(),
        }
    } else {
        optimize_candidate_seeds(
            remaining_candidate_seeds.as_slice(),
            &prepared_bootstrap.bootstrap_context,
            &resolved_stage_payloads.stage_solver_settings,
            candidate_search_plan.budget_schedule,
            progress_sender,
            Some(resume_store),
        )
    };

    let remaining_candidate_outcomes = remaining_population_refinement_output
        .candidate_outcomes
        .as_slice();

    let candidate_outcomes = candidate_search_plan
        .candidate_seeds
        .iter()
        .filter_map(|(candidate_id, _)| {
            persisted_candidate_outcomes
                .get(candidate_id.as_str())
                .cloned()
                .or_else(|| {
                    remaining_candidate_outcomes
                        .iter()
                        .find(|candidate_outcome| candidate_outcome.candidate_id == *candidate_id)
                        .cloned()
                })
        })
        .collect::<Vec<_>>();

    let population_refinement_wall_clock_millis = elapsed_millis(population_refinement_started_at);

    ensure_simulation_infrastructure_available(&prepared_bootstrap.bootstrap_context)?;

    let valid_candidate_count = count_valid_candidate_outcomes(&candidate_outcomes);

    let candidate_selection_artifacts = match select_candidate_outcome(
        stage_input,
        prepared_bootstrap,
        baseline_evaluation_artifacts,
        candidate_outcomes.as_slice(),
        progress_sender,
        resume_store,
    ) {
        Ok(artifacts) => artifacts,
        Err(failure) => {
            let runtime_profile = prepared_bootstrap
                .bootstrap_context
                .profiling_state
                .snapshot_runtime_profile(population_refinement_wall_clock_millis, 0, 0, 0);

            resume_store.record_runtime_profile(runtime_profile);

            return Err(failure);
        }
    };

    Ok(CandidateSearchArtifacts {
        candidate_outcomes,
        candidate_evaluation_timing_diagnostics: remaining_population_refinement_output
            .evaluation_timing_diagnostics,
        valid_candidate_count,
        best_candidate: candidate_selection_artifacts.best_candidate,
        option_refinement_candidate_frontier: candidate_selection_artifacts
            .option_refinement_candidate_frontier,
        selected_option_search_tier_pricing_artifact: None,
        selected_option_selection_tier_pricing_artifact: None,
        selected_option_audit_tier_pricing_artifact: None,
        option_search_selection_agreement: None,
        coarse_consistency_artifacts_by_candidate_id: candidate_selection_artifacts
            .coarse_consistency_artifacts_by_candidate_id,
        warning: candidate_selection_artifacts.warning,
        population_refinement_wall_clock_millis,
        coarse_consistency_wall_clock_millis: candidate_selection_artifacts
            .coarse_consistency_wall_clock_millis,
    })
}

fn build_candidate_search_plan(
    stage_input: &JointRefinementStageExecutionInput,
    resolved_stage_payloads: &ResolvedStagePayloads<'_>,
    prepared_bootstrap: &PreparedBootstrapContext,
    baseline_evaluation_artifacts: &BaselineEvaluationArtifacts,
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
) -> CandidateSearchPlan {
    let activity_log_shift =
        observed_activity_log_shift_for_joint_context(&prepared_bootstrap.bootstrap_context)
            .unwrap_or_else(|| {
                observed_activity_log_shift(
                    prepared_bootstrap
                        .bootstrap_context
                        .observed_data
                        .observed_open_event_rate_per_hour,
                    resolved_stage_payloads.baseline_modeled_open_event_rate_per_hour,
                )
            });

    let candidate_seeds = build_candidate_seeds(CandidateSeedsBuildInput {
        baseline_parameter_vector: prepared_bootstrap.baseline_parameter_vector.as_slice(),
        active_parameter_layout: &prepared_bootstrap.bootstrap_context.active_parameter_layout,
        lower_bounds: prepared_bootstrap.bootstrap_context.lower_bounds.as_slice(),
        upper_bounds: prepared_bootstrap.bootstrap_context.upper_bounds.as_slice(),
        baseline_15m_calibration_sample: &prepared_bootstrap.baseline_15m_calibration_sample,
        return_component_basin_diagnostics: selected_return_components_candidate(
            resolved_stage_payloads.return_components_payload,
        )
        .map(|candidate| {
            candidate
                .continuous_distribution_fit_breakdown
                .optimizer_basin_diagnostics
                .as_slice()
        })
        .unwrap_or(&[]),
        baseline_final_objective_terms: baseline_evaluation_artifacts
            .baseline_candidate_outcome
            .final_evaluation
            .objective_terms
            .as_slice(),
        activity_log_shift,
        joint_context: &prepared_bootstrap.bootstrap_context,
        solver_settings: &resolved_stage_payloads.stage_solver_settings,
        candidate_budget: desired_joint_refinement_candidate_budget(
            stage_input.candidate_budget_override,
            stage_input.calibration_effort,
        ),
        progress_sender,
    });

    let budget_schedule = joint_refinement_budget_schedule(
        &resolved_stage_payloads.stage_solver_settings,
        candidate_seeds.len(),
        stage_input.calibration_effort,
    );

    CandidateSearchPlan {
        candidate_seeds,
        budget_schedule,
    }
}

pub(in crate::stages::joint_refinement::execution) fn count_valid_candidate_outcomes(
    candidate_outcomes: &[JointRefinementCandidateOutcome],
) -> usize {
    candidate_outcomes
        .iter()
        .filter(|candidate_outcome| {
            candidate_outcome.local_evaluation.invalid_reason.is_none()
                && candidate_outcome.final_evaluation.invalid_reason.is_none()
        })
        .count()
}
