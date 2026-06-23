use janiform_foundations::linear_algebra::mean_diagonal_damped_least_squares_step;

use janiform_engine_pricing::internal::SpectralThetaWarmStartState;

use crate::stages::joint_refinement::option_surface_tiering::JointRefinementOptionSurfaceTierKind;

use super::super::{JointRefinementContext, JointRefinementRestartOutcome, elapsed_millis};
use super::residual_jacobian::weighted_option_residual_jacobian_rows;
use super::workflow::{
    OptionRefinementProgressContext, ScoredPricingVectorInput, SearchTierCandidate,
    option_refinement_effective_search_sample_count, option_score_resolution,
    pricing_block_search_coordinate_count, scored_pricing_vector,
};

// The Levenberg term is one mean diagonal of the local normal matrix.
const OPTION_PRICING_BLOCK_NORMAL_EQUATION_DAMPING_MEAN_DIAGONAL_MULTIPLIER: f64 = 1.0;
#[derive(Debug, Clone, Copy)]
pub(super) struct OptionRefinementCandidateSearchBudget {
    pub(super) total_evaluation_budget: usize,
    pub(super) wall_clock_guard: Option<OptionRefinementCandidateWallClockGuard>,
}

impl OptionRefinementCandidateSearchBudget {
    fn can_start_proposal_evaluation(self, used_evaluation_count: usize) -> bool {
        if used_evaluation_count >= self.total_evaluation_budget.max(1) {
            return false;
        }

        self.wall_clock_guard
            .map(OptionRefinementCandidateWallClockGuard::can_start_full_oracle_call)
            .unwrap_or(true)
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct OptionRefinementCandidateWallClockGuard {
    pub(super) started_at: std::time::Instant,
    pub(super) budget_millis: u64,
    pub(super) reference_oracle_call_millis: u64,
}

impl OptionRefinementCandidateWallClockGuard {
    fn can_start_full_oracle_call(self) -> bool {
        let elapsed_millis = elapsed_millis(self.started_at);

        elapsed_millis.saturating_add(self.reference_oracle_call_millis) <= self.budget_millis
    }
}

#[derive(Debug, Clone)]
struct PricingBlockProposal {
    parameter_vector: Vec<f64>,
    predicted_score: Option<f64>,
}

#[derive(Debug, Clone)]
struct PricingBlockWeightedResidualJacobianRow {
    residual: f64,
    gradient: Vec<f64>,
}

pub(super) fn optimize_pricing_block_from_search_tier_sensitivities(
    refinement_context: &JointRefinementContext,
    dynamics_candidate: &JointRefinementRestartOutcome,
    initial_candidate: SearchTierCandidate,
    source_dynamics_parameter_bits: &[u64],
    theta_warm_start_state: &mut SpectralThetaWarmStartState,
    progress_context: Option<&OptionRefinementProgressContext<'_>>,
    search_budget: OptionRefinementCandidateSearchBudget,
) -> Result<SearchTierCandidate, String> {
    let score_resolution = option_score_resolution(
        initial_candidate.search_tier_score,
        option_refinement_effective_search_sample_count(&initial_candidate),
    );
    let proposals = pricing_block_sensitivity_proposals(
        refinement_context,
        &initial_candidate,
        score_resolution,
    )?;

    let mut best_candidate = initial_candidate;
    let mut budget_limited = false;
    let mut prediction_rejected = false;
    let mut used_evaluation_count = 1usize;

    evaluate_pricing_block_proposals(
        PricingBlockProposalEvaluationInput {
            refinement_context,
            dynamics_candidate,
            source_dynamics_parameter_bits,
            theta_warm_start_state,
            progress_context,
            search_budget,
            used_evaluation_count: &mut used_evaluation_count,
            best_candidate: &mut best_candidate,
            budget_limited: &mut budget_limited,
            prediction_rejected: &mut prediction_rejected,
        },
        proposals,
    )?;

    let bound_proposals =
        pricing_block_bound_exploration_proposals(refinement_context, &best_candidate);

    evaluate_pricing_block_proposals(
        PricingBlockProposalEvaluationInput {
            refinement_context,
            dynamics_candidate,
            source_dynamics_parameter_bits,
            theta_warm_start_state,
            progress_context,
            search_budget,
            used_evaluation_count: &mut used_evaluation_count,
            best_candidate: &mut best_candidate,
            budget_limited: &mut budget_limited,
            prediction_rejected: &mut prediction_rejected,
        },
        bound_proposals,
    )?;

    if budget_limited {
        best_candidate.restart_outcome.stop_reason =
            "option_pricing_block_budget_limited".to_string();
    } else if prediction_rejected {
        best_candidate.restart_outcome.stop_reason =
            "option_pricing_block_reduced_model_prediction_rejected".to_string();
    }

    Ok(best_candidate)
}

struct PricingBlockProposalEvaluationInput<'a> {
    refinement_context: &'a JointRefinementContext,
    dynamics_candidate: &'a JointRefinementRestartOutcome,
    source_dynamics_parameter_bits: &'a [u64],
    theta_warm_start_state: &'a mut SpectralThetaWarmStartState,
    progress_context: Option<&'a OptionRefinementProgressContext<'a>>,
    search_budget: OptionRefinementCandidateSearchBudget,
    used_evaluation_count: &'a mut usize,
    best_candidate: &'a mut SearchTierCandidate,
    budget_limited: &'a mut bool,
    prediction_rejected: &'a mut bool,
}

fn evaluate_pricing_block_proposals(
    input: PricingBlockProposalEvaluationInput<'_>,
    proposals: Vec<PricingBlockProposal>,
) -> Result<(), String> {
    let score_resolution = option_score_resolution(
        input.best_candidate.search_tier_score,
        option_refinement_effective_search_sample_count(input.best_candidate),
    );

    for proposal in proposals {
        if proposal.predicted_score.is_some_and(|predicted_score| {
            predicted_score >= input.best_candidate.search_tier_score
        }) {
            *input.prediction_rejected = true;
            continue;
        }

        if !input
            .search_budget
            .can_start_proposal_evaluation(*input.used_evaluation_count)
        {
            *input.budget_limited = true;
            break;
        }

        let evaluated_candidate = accept_or_reject_pricing_block_proposal(
            scored_pricing_vector(ScoredPricingVectorInput {
                refinement_context: input.refinement_context,
                dynamics_candidate: input.dynamics_candidate,
                pricing_vector: proposal.parameter_vector.as_slice(),
                source_dynamics_parameter_bits: input.source_dynamics_parameter_bits,
                tier_kind: JointRefinementOptionSurfaceTierKind::Search,
                include_node_price_sensitivities: false,
                stop_reason: "option_pricing_block_refined",
                theta_warm_start_state: Some(&mut *input.theta_warm_start_state),
                progress_context: input.progress_context,
            }),
            input.used_evaluation_count,
            input.prediction_rejected,
        );

        let Some(evaluated_candidate) = evaluated_candidate else {
            continue;
        };

        if evaluated_candidate.search_tier_score < input.best_candidate.search_tier_score {
            let validated_improvement =
                input.best_candidate.search_tier_score - evaluated_candidate.search_tier_score;

            *input.best_candidate = evaluated_candidate;

            if validated_improvement <= score_resolution {
                input.best_candidate.restart_outcome.stop_reason =
                    "option_pricing_block_validated_improvement_below_resolution".to_string();

                break;
            }

            continue;
        }

        *input.prediction_rejected = true;
    }

    Ok(())
}

fn accept_or_reject_pricing_block_proposal(
    evaluation_result: Result<SearchTierCandidate, String>,
    used_evaluation_count: &mut usize,
    prediction_rejected: &mut bool,
) -> Option<SearchTierCandidate> {
    *used_evaluation_count = (*used_evaluation_count).saturating_add(1);

    match evaluation_result {
        Ok(evaluated_candidate) => Some(evaluated_candidate),
        Err(_) => {
            *prediction_rejected = true;
            None
        }
    }
}

fn pricing_block_sensitivity_proposals(
    refinement_context: &JointRefinementContext,
    initial_candidate: &SearchTierCandidate,
    score_resolution: f64,
) -> Result<Vec<PricingBlockProposal>, String> {
    let least_squares_proposals = pricing_block_least_squares_proposals(
        refinement_context,
        initial_candidate,
        score_resolution,
    )?;

    let mut proposals = least_squares_proposals;
    if !proposals.iter().any(|proposal| {
        proposal
            .predicted_score
            .is_some_and(|predicted_score| predicted_score < initial_candidate.search_tier_score)
    }) {
        proposals.extend(pricing_block_gradient_proposals(
            refinement_context,
            initial_candidate,
            score_resolution,
        )?);
    }

    Ok(proposals)
}

fn pricing_block_least_squares_proposals(
    refinement_context: &JointRefinementContext,
    initial_candidate: &SearchTierCandidate,
    score_resolution: f64,
) -> Result<Vec<PricingBlockProposal>, String> {
    let Some(node_sensitivities) = initial_candidate
        .search_tier_pricing_artifact
        .node_sensitivities
        .as_ref()
    else {
        return Ok(Vec::new());
    };

    if node_sensitivities.is_empty() {
        return Ok(Vec::new());
    }

    let current_vector = initial_candidate
        .restart_outcome
        .parameter_vector
        .as_slice();

    let weighted_rows = weighted_option_residual_jacobian_rows(node_sensitivities.as_slice())?;

    let mut search_coordinate_rows = Vec::with_capacity(weighted_rows.len());
    let mut residual_vector = Vec::with_capacity(weighted_rows.len());

    for weighted_row in weighted_rows {
        if weighted_row.gradient.len() != pricing_block_search_coordinate_count(refinement_context)
        {
            return Err(
                "option pricing residual Jacobian dimension does not match pricing block"
                    .to_string(),
            );
        }

        residual_vector.push(weighted_row.residual);

        search_coordinate_rows.push(PricingBlockWeightedResidualJacobianRow {
            residual: weighted_row.residual,
            gradient: weighted_row.gradient,
        });
    }

    let sensitivity_matrix = search_coordinate_rows
        .iter()
        .map(|weighted_row| weighted_row.gradient.clone())
        .collect::<Vec<_>>();

    let Some(least_squares_step) = mean_diagonal_damped_least_squares_step(
        sensitivity_matrix.as_slice(),
        residual_vector.as_slice(),
        OPTION_PRICING_BLOCK_NORMAL_EQUATION_DAMPING_MEAN_DIAGONAL_MULTIPLIER,
    ) else {
        return Ok(Vec::new());
    };

    let maximum_step_scale =
        maximum_bounded_line_step(current_vector, &least_squares_step, refinement_context).min(1.0);
    if maximum_step_scale <= 0.0 || !maximum_step_scale.is_finite() {
        return Ok(Vec::new());
    }

    Ok(pricing_block_line_search_proposals(
        maximum_step_scale,
        initial_candidate.search_tier_score,
        score_resolution,
        |step_scale| {
            pricing_block_least_squares_proposal(
                current_vector,
                search_coordinate_rows.as_slice(),
                &least_squares_step,
                step_scale,
                refinement_context,
            )
        },
    ))
}

fn pricing_block_gradient_proposals(
    refinement_context: &JointRefinementContext,
    initial_candidate: &SearchTierCandidate,
    score_resolution: f64,
) -> Result<Vec<PricingBlockProposal>, String> {
    let Some(score_gradient_artifact) = initial_candidate
        .search_tier_pricing_artifact
        .score_gradient_artifact
        .as_ref()
    else {
        return Ok(Vec::new());
    };

    let Some(raw_gradient) = score_gradient_artifact.gradient.as_ref() else {
        return Ok(Vec::new());
    };

    if raw_gradient.len() != pricing_block_search_coordinate_count(refinement_context) {
        return Err("option pricing gradient dimension does not match pricing block".to_string());
    }

    let current_vector = initial_candidate
        .restart_outcome
        .parameter_vector
        .as_slice();

    let search_gradient = raw_gradient.clone();

    let direction = search_gradient
        .iter()
        .map(|gradient| -gradient)
        .collect::<Vec<_>>();

    let maximum_step =
        maximum_bounded_line_step(current_vector, direction.as_slice(), refinement_context);
    if maximum_step <= 0.0 || !maximum_step.is_finite() {
        return Ok(Vec::new());
    }

    Ok(pricing_block_line_search_proposals(
        maximum_step,
        initial_candidate.search_tier_score,
        score_resolution,
        |step| {
            pricing_block_gradient_proposal(
                current_vector,
                &search_gradient,
                &direction,
                step,
                initial_candidate.search_tier_score,
                refinement_context,
            )
        },
    ))
}

fn pricing_block_line_search_proposals(
    maximum_step: f64,
    current_score: f64,
    score_resolution: f64,
    mut proposal_at_step: impl FnMut(f64) -> Option<PricingBlockProposal>,
) -> Vec<PricingBlockProposal> {
    let mut proposals = Vec::new();
    let mut step = maximum_step;

    while step > 0.0 && step.is_finite() {
        let Some(proposal) = proposal_at_step(step) else {
            break;
        };

        let Some(predicted_score) = proposal.predicted_score else {
            proposals.push(proposal);
            continue;
        };

        let predicted_improvement = current_score - predicted_score;
        if predicted_improvement > 0.0 && predicted_improvement <= score_resolution {
            break;
        }

        let predicted_change = predicted_improvement.abs();
        proposals.push(proposal);

        if predicted_change <= score_resolution {
            break;
        }

        let next_step = step / 2.0;
        if next_step == step {
            break;
        }

        step = next_step;
    }

    proposals
}

fn pricing_block_gradient_proposal(
    current_vector: &[f64],
    search_gradient: &[f64],
    direction: &[f64],
    step: f64,
    current_score: f64,
    refinement_context: &JointRefinementContext,
) -> Option<PricingBlockProposal> {
    let requested_delta = direction
        .iter()
        .map(|direction_value| step * direction_value)
        .collect::<Vec<_>>();

    let (proposal_vector, applied_delta) = pricing_block_proposal_vector(
        current_vector,
        requested_delta.as_slice(),
        refinement_context,
    );

    let mut predicted_score = current_score;

    if applied_delta
        .iter()
        .all(|coordinate_delta| *coordinate_delta == 0.0)
    {
        return None;
    }

    for (gradient_value, coordinate_delta) in search_gradient.iter().zip(applied_delta.iter()) {
        predicted_score += gradient_value * coordinate_delta;
    }

    predicted_score.is_finite().then_some(PricingBlockProposal {
        parameter_vector: proposal_vector,
        predicted_score: Some(predicted_score.max(0.0)),
    })
}

fn pricing_block_least_squares_proposal(
    current_vector: &[f64],
    weighted_rows: &[PricingBlockWeightedResidualJacobianRow],
    least_squares_step: &[f64],
    step_scale: f64,
    refinement_context: &JointRefinementContext,
) -> Option<PricingBlockProposal> {
    let requested_delta = least_squares_step
        .iter()
        .map(|component| step_scale * component)
        .collect::<Vec<_>>();

    let (proposal_vector, applied_delta) = pricing_block_proposal_vector(
        current_vector,
        requested_delta.as_slice(),
        refinement_context,
    );

    if applied_delta
        .iter()
        .all(|coordinate_delta| *coordinate_delta == 0.0)
    {
        return None;
    }

    let predicted_squared_norm = weighted_rows
        .iter()
        .map(|weighted_row| {
            let predicted_residual = weighted_row.residual
                + weighted_row
                    .gradient
                    .iter()
                    .zip(applied_delta.iter())
                    .map(|(sensitivity, coordinate_delta)| sensitivity * coordinate_delta)
                    .sum::<f64>();

            predicted_residual * predicted_residual
        })
        .sum::<f64>();

    predicted_squared_norm
        .is_finite()
        .then_some(PricingBlockProposal {
            parameter_vector: proposal_vector,
            predicted_score: Some(predicted_squared_norm.max(0.0).sqrt()),
        })
}

fn pricing_block_proposal_vector(
    current_vector: &[f64],
    requested_delta: &[f64],
    refinement_context: &JointRefinementContext,
) -> (Vec<f64>, Vec<f64>) {
    let mut proposal_vector = current_vector.to_vec();

    let pricing_parameter_indices = pricing_block_parameter_indices(refinement_context);

    let mut applied_delta = vec![0.0; pricing_parameter_indices.len()];

    for (coordinate_position, parameter_index) in pricing_parameter_indices.iter().enumerate() {
        proposal_vector[*parameter_index] =
            (current_vector[*parameter_index] + requested_delta[coordinate_position]).clamp(
                refinement_context.lower_bounds[*parameter_index],
                refinement_context.upper_bounds[*parameter_index],
            );

        applied_delta[coordinate_position] =
            proposal_vector[*parameter_index] - current_vector[*parameter_index];
    }

    (proposal_vector, applied_delta)
}

fn maximum_bounded_line_step(
    current_vector: &[f64],
    direction: &[f64],
    refinement_context: &JointRefinementContext,
) -> f64 {
    pricing_block_parameter_indices(refinement_context)
        .iter()
        .enumerate()
        .filter_map(|(coordinate_position, parameter_index)| {
            let direction_value = direction[coordinate_position];

            if direction_value > 0.0 {
                return Some(
                    (refinement_context.upper_bounds[*parameter_index]
                        - current_vector[*parameter_index])
                        / direction_value,
                );
            }

            if direction_value < 0.0 {
                return Some(
                    (refinement_context.lower_bounds[*parameter_index]
                        - current_vector[*parameter_index])
                        / direction_value,
                );
            }

            None
        })
        .filter(|step| step.is_finite() && *step > 0.0)
        .min_by(f64::total_cmp)
        .unwrap_or(0.0)
}

fn pricing_block_bound_exploration_proposals(
    refinement_context: &JointRefinementContext,
    initial_candidate: &SearchTierCandidate,
) -> Vec<PricingBlockProposal> {
    let current_vector = initial_candidate
        .restart_outcome
        .parameter_vector
        .as_slice();

    let pricing_parameter_indices = pricing_block_parameter_indices(refinement_context);

    let mut proposals = Vec::new();

    for parameter_index in pricing_parameter_indices {
        for bound in [
            refinement_context.lower_bounds[parameter_index],
            refinement_context.upper_bounds[parameter_index],
        ] {
            if bound == current_vector[parameter_index] {
                continue;
            }

            let mut proposal_vector = current_vector.to_vec();

            proposal_vector[parameter_index] = bound;

            proposals.push(PricingBlockProposal {
                parameter_vector: proposal_vector,
                predicted_score: None,
            });
        }
    }

    proposals
}

fn pricing_block_parameter_indices(refinement_context: &JointRefinementContext) -> Vec<usize> {
    refinement_context
        .active_parameter_layout
        .option_pricing_kernel_block
        .to_vec()
}

#[cfg(test)]
mod tests {
    use std::time::Duration;
    use std::time::Instant;

    use super::{
        OptionRefinementCandidateSearchBudget, OptionRefinementCandidateWallClockGuard,
        accept_or_reject_pricing_block_proposal,
    };

    #[test]
    fn candidate_search_budget_allows_proposals_until_evaluation_budget_is_used() {
        let search_budget = OptionRefinementCandidateSearchBudget {
            total_evaluation_budget: 5,
            wall_clock_guard: None,
        };

        assert!(search_budget.can_start_proposal_evaluation(1));
        assert!(search_budget.can_start_proposal_evaluation(4));
        assert!(!search_budget.can_start_proposal_evaluation(5));
    }

    #[test]
    fn candidate_search_budget_respects_explicit_wall_clock_guard() {
        let started_at = Instant::now()
            .checked_sub(Duration::from_millis(20))
            .expect("test duration is smaller than instant range");

        let search_budget = OptionRefinementCandidateSearchBudget {
            total_evaluation_budget: 5,
            wall_clock_guard: Some(OptionRefinementCandidateWallClockGuard {
                started_at,
                budget_millis: 25,
                reference_oracle_call_millis: 10,
            }),
        };

        assert!(!search_budget.can_start_proposal_evaluation(1));
    }

    #[test]
    fn failed_pricing_block_proposal_consumes_budget_without_failing_candidate_search() {
        let mut used_evaluation_count = 1;
        let mut prediction_rejected = false;

        let evaluated_candidate = accept_or_reject_pricing_block_proposal(
            Err("proposal is outside pricing domain".to_string()),
            &mut used_evaluation_count,
            &mut prediction_rejected,
        );

        assert!(evaluated_candidate.is_none());
        assert_eq!(used_evaluation_count, 2);
        assert!(prediction_rejected);
    }
}
