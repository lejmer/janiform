use std::sync::Arc;
use std::sync::mpsc::Sender;

use janiform_calibration_artifacts::CalibrationOptimizerSettings;

use crate::stages::joint_refinement::contracts::{
    JointRefinementStageExecutionFailure, JointRefinementStageProgressPhase,
    JointRefinementStageProgressUpdate,
};
use crate::stages::joint_refinement::option_surface_tiering::JointRefinementOptionSurfaceTierKind;
use crate::stages::optimization::{PowellOptimizerSettings, optimize_bounded_powell};

use super::super::setup::parameter_vector_to_fitted_parameters;
use super::{
    JointRefinementContext, JointRefinementOptionTierPricingArtifact,
    JointRefinementRestartOutcome, ordered_parameter_bits,
    resolve_option_surface_tier_pricing_artifact,
};

pub(crate) const JOINT_REFINEMENT_DEFAULT_OPTION_REFINEMENT_CANDIDATE_COUNT: usize = 5;

#[derive(Debug, Clone)]
struct OptionRefinedCandidate {
    source_rank: usize,
    search_tier_score: f64,
    selection_tier_score: f64,
    search_tier_pricing_artifact: Arc<JointRefinementOptionTierPricingArtifact>,
    selection_tier_pricing_artifact: Arc<JointRefinementOptionTierPricingArtifact>,
    restart_outcome: JointRefinementRestartOutcome,
}

#[derive(Debug, Clone)]
struct SearchTierCandidate {
    source_rank: usize,
    source_dynamics_parameter_bits: Vec<u64>,
    search_tier_score: f64,
    search_tier_pricing_artifact: Arc<JointRefinementOptionTierPricingArtifact>,
    restart_outcome: JointRefinementRestartOutcome,
}

#[derive(Debug, Clone)]
pub(crate) struct JointRefinementOptionRefinementResult {
    pub(crate) selected_restart_outcome: JointRefinementRestartOutcome,
    pub(crate) selected_search_tier_pricing_artifact: Arc<JointRefinementOptionTierPricingArtifact>,
    pub(crate) selected_selection_tier_pricing_artifact:
        Arc<JointRefinementOptionTierPricingArtifact>,
}

pub(crate) fn desired_joint_refinement_option_refinement_candidate_count(
    candidate_count_override: Option<usize>,
) -> usize {
    candidate_count_override
        .filter(|candidate_count| *candidate_count > 0)
        .unwrap_or(JOINT_REFINEMENT_DEFAULT_OPTION_REFINEMENT_CANDIDATE_COUNT)
}

pub(crate) fn run_option_refinement(
    context: &JointRefinementContext,
    solver_settings: &CalibrationOptimizerSettings,
    dynamics_candidates: &[JointRefinementRestartOutcome],
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
) -> Result<JointRefinementOptionRefinementResult, JointRefinementStageExecutionFailure> {
    if context.option_pricing_context.is_none() {
        return Err(
            JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
                error_message: "joint option refinement requires an option pricing context"
                    .to_string(),
            },
        );
    }
    if dynamics_candidates.is_empty() {
        return Err(
            JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
                error_message:
                    "joint option refinement requires at least one coarse-consistent dynamics candidate"
                        .to_string(),
            },
        );
    }

    let search_candidates = refine_search_tier_candidates(
        context,
        solver_settings,
        dynamics_candidates,
        progress_sender,
    );
    if search_candidates.is_empty() {
        return Err(
            JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
                error_message:
                    "joint option refinement failed for every search-tier dynamics candidate"
                        .to_string(),
            },
        );
    }

    let selection_candidates =
        evaluate_selection_tier_candidates(context, search_candidates, progress_sender);
    let Some(best_candidate) = best_selection_tier_candidate(selection_candidates) else {
        return Err(
            JointRefinementStageExecutionFailure::InvalidCandidateGenerationInput {
                error_message:
                    "joint option refinement failed for every selection-tier dynamics candidate"
                        .to_string(),
            },
        );
    };

    Ok(JointRefinementOptionRefinementResult {
        selected_restart_outcome: best_candidate.restart_outcome,
        selected_search_tier_pricing_artifact: best_candidate.search_tier_pricing_artifact,
        selected_selection_tier_pricing_artifact: best_candidate.selection_tier_pricing_artifact,
    })
}

fn refine_search_tier_candidates(
    context: &JointRefinementContext,
    solver_settings: &CalibrationOptimizerSettings,
    dynamics_candidates: &[JointRefinementRestartOutcome],
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
) -> Vec<SearchTierCandidate> {
    let total_units = dynamics_candidates.len();
    let mut refined_candidates = Vec::new();
    let mut best_search_tier_score: Option<f64> = None;

    emit_option_refinement_progress(
        progress_sender,
        JointRefinementStageProgressPhase::OptionSearchTierRefinement,
        0,
        total_units,
        None,
        None,
        None,
    );

    for (source_rank, dynamics_candidate) in dynamics_candidates.iter().enumerate() {
        let refined_candidate =
            refine_single_search_tier_candidate(context, solver_settings, dynamics_candidate).ok();

        if let Some(candidate) = refined_candidate {
            best_search_tier_score = Some(match best_search_tier_score {
                Some(current_best) => current_best.min(candidate.search_tier_score),
                None => candidate.search_tier_score,
            });
            refined_candidates.push(SearchTierCandidate {
                source_rank,
                source_dynamics_parameter_bits: candidate.source_dynamics_parameter_bits,
                search_tier_score: candidate.search_tier_score,
                search_tier_pricing_artifact: candidate.search_tier_pricing_artifact,
                restart_outcome: candidate.restart_outcome,
            });
        }

        let current_score = refined_candidates
            .last()
            .filter(|candidate| candidate.source_rank == source_rank)
            .map(|candidate| candidate.search_tier_score);

        emit_option_refinement_progress(
            progress_sender,
            JointRefinementStageProgressPhase::OptionSearchTierRefinement,
            source_rank + 1,
            total_units,
            Some(dynamics_candidate.restart_id.as_str()),
            current_score,
            best_search_tier_score,
        );
    }

    refined_candidates
}

fn refine_single_search_tier_candidate(
    context: &JointRefinementContext,
    solver_settings: &CalibrationOptimizerSettings,
    dynamics_candidate: &JointRefinementRestartOutcome,
) -> Result<SearchTierCandidate, String> {
    let refinement_context = pricing_block_refinement_context(context, dynamics_candidate);
    let source_dynamics_parameter_bits =
        ordered_parameter_bits(dynamics_candidate.parameter_vector.as_slice());

    let initial_candidate = scored_pricing_vector(
        &refinement_context,
        dynamics_candidate,
        dynamics_candidate.parameter_vector.as_slice(),
        source_dynamics_parameter_bits.as_slice(),
        JointRefinementOptionSurfaceTierKind::Search,
    )?;

    let optimized_candidate = optimize_pricing_block(
        &refinement_context,
        solver_settings,
        dynamics_candidate,
        source_dynamics_parameter_bits.as_slice(),
    )
    .ok();

    Ok(match optimized_candidate {
        Some(candidate) if candidate.search_tier_score < initial_candidate.search_tier_score => {
            candidate
        }
        _ => initial_candidate,
    })
}

fn optimize_pricing_block(
    refinement_context: &JointRefinementContext,
    solver_settings: &CalibrationOptimizerSettings,
    dynamics_candidate: &JointRefinementRestartOutcome,
    source_dynamics_parameter_bits: &[u64],
) -> Result<SearchTierCandidate, String> {
    let block_indices = refinement_context
        .active_parameter_layout
        .nig_block
        .as_slice();
    if block_indices.is_empty() {
        return scored_pricing_vector(
            refinement_context,
            dynamics_candidate,
            dynamics_candidate.parameter_vector.as_slice(),
            source_dynamics_parameter_bits,
            JointRefinementOptionSurfaceTierKind::Search,
        );
    }

    let initial_point = block_indices
        .iter()
        .map(|parameter_index| dynamics_candidate.parameter_vector[*parameter_index])
        .collect::<Vec<_>>();
    let lower_bounds = block_indices
        .iter()
        .map(|parameter_index| refinement_context.lower_bounds[*parameter_index])
        .collect::<Vec<_>>();
    let upper_bounds = block_indices
        .iter()
        .map(|parameter_index| refinement_context.upper_bounds[*parameter_index])
        .collect::<Vec<_>>();

    let mut evaluate_block = |block_vector: &[f64]| -> Option<f64> {
        let pricing_vector = substitute_pricing_block(
            dynamics_candidate.parameter_vector.as_slice(),
            block_indices,
            block_vector,
        );
        scored_pricing_vector(
            refinement_context,
            dynamics_candidate,
            pricing_vector.as_slice(),
            source_dynamics_parameter_bits,
            JointRefinementOptionSurfaceTierKind::Search,
        )
        .ok()
        .map(|candidate| candidate.search_tier_score)
    };

    let optimizer_result = optimize_bounded_powell(
        initial_point.as_slice(),
        lower_bounds.as_slice(),
        upper_bounds.as_slice(),
        option_pricing_block_powell_settings(solver_settings, block_indices.len()),
        &mut evaluate_block,
    )
    .ok_or_else(|| "pricing block optimizer found no valid option-pricing step".to_string())?;

    let pricing_vector = substitute_pricing_block(
        dynamics_candidate.parameter_vector.as_slice(),
        block_indices,
        optimizer_result.best_point.as_slice(),
    );

    scored_pricing_vector(
        refinement_context,
        dynamics_candidate,
        pricing_vector.as_slice(),
        source_dynamics_parameter_bits,
        JointRefinementOptionSurfaceTierKind::Search,
    )
}

fn evaluate_selection_tier_candidates(
    context: &JointRefinementContext,
    search_candidates: Vec<SearchTierCandidate>,
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
) -> Vec<OptionRefinedCandidate> {
    let total_units = search_candidates.len();
    let mut selection_candidates = Vec::new();
    let mut best_selection_tier_score: Option<f64> = None;

    emit_option_refinement_progress(
        progress_sender,
        JointRefinementStageProgressPhase::OptionSelectionTierEvaluation,
        0,
        total_units,
        None,
        None,
        None,
    );

    for (candidate_position, search_candidate) in search_candidates.into_iter().enumerate() {
        let restart_id = search_candidate.restart_outcome.restart_id.clone();
        let pricing_parameter_bits = pricing_block_parameter_bits(
            search_candidate.restart_outcome.parameter_vector.as_slice(),
            context,
        );

        let selection_artifact = resolve_option_surface_tier_pricing_artifact(
            context,
            &search_candidate
                .restart_outcome
                .final_evaluation
                .fitted_parameters,
            JointRefinementOptionSurfaceTierKind::Selection,
            search_candidate.source_dynamics_parameter_bits.as_slice(),
            pricing_parameter_bits.as_slice(),
            None,
            None,
            None,
            None,
        )
        .ok();
        let selection_score = selection_artifact
            .as_ref()
            .map(|artifact| artifact.score.root_mean_square_spread_normalized_residual);

        if let Some(selection_tier_pricing_artifact) = selection_artifact {
            let selection_tier_score = selection_tier_pricing_artifact
                .score
                .root_mean_square_spread_normalized_residual;
            best_selection_tier_score = Some(match best_selection_tier_score {
                Some(current_best) => current_best.min(selection_tier_score),
                None => selection_tier_score,
            });
            selection_candidates.push(OptionRefinedCandidate {
                source_rank: search_candidate.source_rank,
                search_tier_score: search_candidate.search_tier_score,
                selection_tier_score,
                search_tier_pricing_artifact: search_candidate.search_tier_pricing_artifact,
                selection_tier_pricing_artifact,
                restart_outcome: search_candidate.restart_outcome,
            });
        }

        emit_option_refinement_progress(
            progress_sender,
            JointRefinementStageProgressPhase::OptionSelectionTierEvaluation,
            candidate_position + 1,
            total_units,
            Some(restart_id.as_str()),
            selection_score,
            best_selection_tier_score,
        );
    }

    selection_candidates
}

fn scored_pricing_vector(
    refinement_context: &JointRefinementContext,
    dynamics_candidate: &JointRefinementRestartOutcome,
    pricing_vector: &[f64],
    source_dynamics_parameter_bits: &[u64],
    tier_kind: JointRefinementOptionSurfaceTierKind,
) -> Result<SearchTierCandidate, String> {
    let fitted_parameters =
        parameter_vector_to_fitted_parameters(pricing_vector, refinement_context)
            .ok_or_else(|| "pricing block parameter conversion failed".to_string())?;
    let pricing_parameter_bits = pricing_block_parameter_bits(pricing_vector, refinement_context);
    let pricing_artifact = resolve_option_surface_tier_pricing_artifact(
        refinement_context,
        &fitted_parameters,
        tier_kind,
        source_dynamics_parameter_bits,
        pricing_parameter_bits.as_slice(),
        None,
        None,
        None,
        None,
    )?;
    let search_tier_score = pricing_artifact
        .score
        .root_mean_square_spread_normalized_residual;

    Ok(SearchTierCandidate {
        source_rank: 0,
        source_dynamics_parameter_bits: source_dynamics_parameter_bits.to_vec(),
        search_tier_score,
        search_tier_pricing_artifact: pricing_artifact,
        restart_outcome: restart_outcome_with_pricing_block(
            dynamics_candidate,
            pricing_vector,
            fitted_parameters,
        ),
    })
}

fn best_selection_tier_candidate(
    mut candidates: Vec<OptionRefinedCandidate>,
) -> Option<OptionRefinedCandidate> {
    candidates.sort_by(|left, right| {
        left.selection_tier_score
            .total_cmp(&right.selection_tier_score)
            .then_with(|| left.search_tier_score.total_cmp(&right.search_tier_score))
            .then_with(|| left.source_rank.cmp(&right.source_rank))
            .then_with(|| {
                left.restart_outcome
                    .restart_id
                    .cmp(&right.restart_outcome.restart_id)
            })
    });

    candidates.into_iter().next()
}

fn pricing_block_refinement_context(
    context: &JointRefinementContext,
    dynamics_candidate: &JointRefinementRestartOutcome,
) -> JointRefinementContext {
    let mut refinement_context = context.clone();
    refinement_context.baseline_hawkes_fitted_parameters = dynamics_candidate
        .final_evaluation
        .fitted_parameters
        .hawkes_fitted_parameters
        .clone();
    refinement_context.baseline_return_components_fitted_parameters = dynamics_candidate
        .final_evaluation
        .fitted_parameters
        .return_components_fitted_parameters
        .clone();
    refinement_context.baseline_mean_reversion_fitted_parameters = dynamics_candidate
        .final_evaluation
        .fitted_parameters
        .return_components_fitted_parameters
        .mean_reversion_fitted_parameters
        .clone();
    refinement_context.option_pricing_refinement_enabled = true;
    refinement_context
}

fn restart_outcome_with_pricing_block(
    dynamics_candidate: &JointRefinementRestartOutcome,
    pricing_vector: &[f64],
    fitted_parameters: janiform_calibration_artifacts::JointRefinementStageFittedParameters,
) -> JointRefinementRestartOutcome {
    let mut local_evaluation = dynamics_candidate.local_evaluation.clone();
    local_evaluation.fitted_parameters = fitted_parameters.clone();

    let mut final_evaluation = dynamics_candidate.final_evaluation.clone();
    final_evaluation.fitted_parameters = fitted_parameters;

    JointRefinementRestartOutcome {
        restart_id: dynamics_candidate.restart_id.clone(),
        parameter_vector: pricing_vector.to_vec(),
        local_evaluation,
        final_evaluation,
        local_evaluation_count: dynamics_candidate.local_evaluation_count,
        stop_reason: "option_pricing_block_refined",
    }
}

fn option_pricing_block_powell_settings(
    solver_settings: &CalibrationOptimizerSettings,
    active_nig_block_dimension: usize,
) -> PowellOptimizerSettings {
    let line_search_evaluation_cap = active_nig_block_dimension + 1;
    PowellOptimizerSettings {
        absolute_tolerance: solver_settings.absolute_tolerance,
        relative_tolerance: solver_settings.relative_tolerance,
        min_step_norm: solver_settings.absolute_tolerance,
        max_cycle_count: 1,
        line_search_evaluation_cap,
        evaluation_budget: active_nig_block_dimension * line_search_evaluation_cap,
    }
}

fn pricing_block_parameter_bits(
    parameter_vector: &[f64],
    context: &JointRefinementContext,
) -> Vec<u64> {
    context
        .active_parameter_layout
        .nig_block
        .iter()
        .map(|parameter_index| parameter_vector[*parameter_index].to_bits())
        .collect()
}

fn substitute_pricing_block(
    current_vector: &[f64],
    block_indices: &[usize],
    block_vector: &[f64],
) -> Vec<f64> {
    let mut updated = current_vector.to_vec();
    for (block_position, parameter_index) in block_indices.iter().enumerate() {
        updated[*parameter_index] = block_vector[block_position];
    }

    updated
}

fn emit_option_refinement_progress(
    progress_sender: Option<&Sender<JointRefinementStageProgressUpdate>>,
    phase: JointRefinementStageProgressPhase,
    completed_units: usize,
    total_units: usize,
    current_item_label: Option<&str>,
    current_weighted_objective: Option<f64>,
    best_weighted_objective: Option<f64>,
) {
    if let Some(progress_sender) = progress_sender {
        let _ = progress_sender.send(JointRefinementStageProgressUpdate {
            current_item_label: current_item_label.map(str::to_string),
            current_weighted_objective,
            best_weighted_objective,
            ..JointRefinementStageProgressUpdate::new(phase, completed_units, total_units)
        });
    }
}

#[cfg(test)]
mod tests {
    use janiform_calibration_artifacts::CalibrationOptimizerSettings;

    use super::{
        JOINT_REFINEMENT_DEFAULT_OPTION_REFINEMENT_CANDIDATE_COUNT,
        desired_joint_refinement_option_refinement_candidate_count,
        option_pricing_block_powell_settings,
    };

    #[test]
    fn option_refinement_candidate_count_defaults_to_five() {
        assert_eq!(
            desired_joint_refinement_option_refinement_candidate_count(None),
            JOINT_REFINEMENT_DEFAULT_OPTION_REFINEMENT_CANDIDATE_COUNT,
        );
    }

    #[test]
    fn option_refinement_candidate_count_honors_positive_override() {
        assert_eq!(
            desired_joint_refinement_option_refinement_candidate_count(Some(7)),
            7,
        );
    }

    #[test]
    fn pricing_block_budget_uses_one_dimension_minimal_powell_cycle() {
        let solver_settings =
            CalibrationOptimizerSettings::try_new("joint-test", 256, 4, 1.0e-6, 2.0e-6, 10.0)
                .expect("solver settings should validate");

        let powell_settings = option_pricing_block_powell_settings(&solver_settings, 4);

        assert_eq!(powell_settings.max_cycle_count, 1);
        assert_eq!(powell_settings.line_search_evaluation_cap, 5);
        assert_eq!(powell_settings.evaluation_budget, 20);
        assert_eq!(powell_settings.absolute_tolerance, 1.0e-6);
        assert_eq!(powell_settings.relative_tolerance, 2.0e-6);
    }
}
