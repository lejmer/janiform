use janiform_foundations::linear_algebra::mean_diagonal_damped_least_squares_step_4;

use janiform_engine_pricing::internal::SpectralThetaWarmStartState;

use crate::stages::joint_metrics::build_continuous_calibration_sample_from_aggregated_intervals;
use crate::stages::joint_refinement::option_surface_tiering::JointRefinementOptionSurfaceTierKind;
use crate::stages::return_components::{
    NigSharedPlusShiftConfigurationSearchPointMapping, nig_configuration_search_point_mapping,
};

use super::super::super::setup::{
    parameter_vector_to_fitted_parameters, parameter_vector_to_nig_search_point,
};
use super::super::{JointRefinementContext, JointRefinementRestartOutcome, elapsed_millis};
use super::residual_jacobian::weighted_option_residual_jacobian_rows;
use super::workflow::{
    OptionRefinementProgressContext, SearchTierCandidate,
    option_refinement_effective_search_sample_count, option_score_resolution,
    pricing_block_search_coordinate_count, scored_pricing_vector,
};

// The Levenberg term is one mean diagonal of the local normal matrix.
const OPTION_PRICING_BLOCK_NORMAL_EQUATION_DAMPING_MEAN_DIAGONAL_MULTIPLIER: f64 = 1.0;

#[derive(Debug, Clone, Copy)]
pub(super) struct OptionRefinementCandidateWallClockBudget {
    pub(super) started_at: std::time::Instant,
    pub(super) budget_millis: u64,
    pub(super) reference_oracle_call_millis: u64,
}

impl OptionRefinementCandidateWallClockBudget {
    fn can_start_full_oracle_call(self) -> bool {
        let elapsed_millis = elapsed_millis(self.started_at);

        elapsed_millis.saturating_add(self.reference_oracle_call_millis) <= self.budget_millis
    }
}

#[derive(Debug, Clone)]
struct PricingBlockProposal {
    parameter_vector: Vec<f64>,
    predicted_score: f64,
}

#[derive(Debug, Clone, Copy)]
struct PricingBlockWeightedResidualJacobianRow {
    residual: f64,
    gradient: [f64; 4],
}

pub(super) fn optimize_pricing_block_from_search_tier_sensitivities(
    refinement_context: &JointRefinementContext,
    dynamics_candidate: &JointRefinementRestartOutcome,
    initial_candidate: SearchTierCandidate,
    source_dynamics_parameter_bits: &[u64],
    theta_warm_start_state: &mut SpectralThetaWarmStartState,
    progress_context: Option<&OptionRefinementProgressContext<'_>>,
    wall_clock_budget: OptionRefinementCandidateWallClockBudget,
) -> Result<SearchTierCandidate, String> {
    let score_resolution = option_score_resolution(
        initial_candidate.search_tier_score,
        option_refinement_effective_search_sample_count(&initial_candidate),
    );
    let proposals =
        pricing_block_proposals(refinement_context, &initial_candidate, score_resolution)?;

    let mut best_candidate = initial_candidate;
    let mut budget_limited = false;
    let mut prediction_rejected = false;

    for proposal in proposals {
        if !wall_clock_budget.can_start_full_oracle_call() {
            budget_limited = true;
            break;
        }

        if proposal.predicted_score >= best_candidate.search_tier_score {
            prediction_rejected = true;
            continue;
        }

        let evaluated_candidate = scored_pricing_vector(
            refinement_context,
            dynamics_candidate,
            proposal.parameter_vector.as_slice(),
            source_dynamics_parameter_bits,
            JointRefinementOptionSurfaceTierKind::Search,
            false,
            "option_pricing_block_refined",
            Some(theta_warm_start_state),
            progress_context,
        )?;

        if evaluated_candidate.search_tier_score < best_candidate.search_tier_score {
            let validated_improvement =
                best_candidate.search_tier_score - evaluated_candidate.search_tier_score;
            best_candidate = evaluated_candidate;

            if validated_improvement <= score_resolution {
                best_candidate.restart_outcome.stop_reason =
                    "option_pricing_block_validated_improvement_below_resolution".to_string();
                break;
            }

            continue;
        }

        prediction_rejected = true;
    }

    if budget_limited {
        best_candidate.restart_outcome.stop_reason =
            "option_pricing_block_budget_limited".to_string();
    } else if prediction_rejected {
        best_candidate.restart_outcome.stop_reason =
            "option_pricing_block_reduced_model_prediction_rejected".to_string();
    }

    Ok(best_candidate)
}

fn pricing_block_proposals(
    refinement_context: &JointRefinementContext,
    initial_candidate: &SearchTierCandidate,
    score_resolution: f64,
) -> Result<Vec<PricingBlockProposal>, String> {
    let least_squares_proposals = pricing_block_least_squares_proposals(
        refinement_context,
        initial_candidate,
        score_resolution,
    )?;

    if least_squares_proposals
        .iter()
        .any(|proposal| proposal.predicted_score < initial_candidate.search_tier_score)
    {
        return Ok(least_squares_proposals);
    }

    pricing_block_gradient_proposals(refinement_context, initial_candidate, score_resolution)
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

    let current_search_point = parameter_vector_to_nig_search_point(
        current_vector,
        &refinement_context.active_parameter_layout,
    );

    let calibration_sample = pricing_block_calibration_sample(current_vector, refinement_context)?;

    let mapping = nig_configuration_search_point_mapping(&calibration_sample, current_search_point)
        .ok_or_else(|| "option pricing NIG search-point Jacobian mapping failed".to_string())?;

    let weighted_rows = weighted_option_residual_jacobian_rows(node_sensitivities.as_slice())?;

    let mut search_coordinate_rows = Vec::with_capacity(weighted_rows.len());
    let mut residual_vector = Vec::with_capacity(weighted_rows.len());

    for weighted_row in weighted_rows {
        if weighted_row.gradient.len() != pricing_block_search_coordinate_count() {
            return Err(
                "option pricing residual Jacobian dimension does not match NIG pricing block"
                    .to_string(),
            );
        }

        residual_vector.push(weighted_row.residual);

        search_coordinate_rows.push(PricingBlockWeightedResidualJacobianRow {
            residual: weighted_row.residual,
            gradient: search_coordinate_gradient_from_raw_nig_gradient(
                weighted_row.gradient.as_slice(),
                &mapping,
            ),
        });
    }

    let sensitivity_matrix = search_coordinate_rows
        .iter()
        .map(|weighted_row| weighted_row.gradient)
        .collect::<Vec<_>>();

    let Some(least_squares_step) = mean_diagonal_damped_least_squares_step_4(
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

    if raw_gradient.len() != pricing_block_search_coordinate_count() {
        return Err(
            "option pricing gradient dimension does not match NIG pricing block".to_string(),
        );
    }

    let current_vector = initial_candidate
        .restart_outcome
        .parameter_vector
        .as_slice();

    let current_search_point = parameter_vector_to_nig_search_point(
        current_vector,
        &refinement_context.active_parameter_layout,
    );

    let calibration_sample = pricing_block_calibration_sample(current_vector, refinement_context)?;

    let mapping = nig_configuration_search_point_mapping(&calibration_sample, current_search_point)
        .ok_or_else(|| "option pricing NIG search-point gradient mapping failed".to_string())?;

    let search_gradient =
        search_coordinate_gradient_from_raw_nig_gradient(raw_gradient.as_slice(), &mapping);

    let direction = search_gradient.map(|gradient| -gradient);

    let maximum_step = maximum_bounded_line_step(current_vector, &direction, refinement_context);
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

        let predicted_improvement = current_score - proposal.predicted_score;
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
    search_gradient: &[f64; 4],
    direction: &[f64; 4],
    step: f64,
    current_score: f64,
    refinement_context: &JointRefinementContext,
) -> Option<PricingBlockProposal> {
    let requested_delta = direction.map(|direction_value| step * direction_value);

    let (proposal_vector, applied_delta) =
        pricing_block_proposal_vector(current_vector, &requested_delta, refinement_context);

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
        predicted_score: predicted_score.max(0.0),
    })
}

fn pricing_block_least_squares_proposal(
    current_vector: &[f64],
    weighted_rows: &[PricingBlockWeightedResidualJacobianRow],
    least_squares_step: &[f64; 4],
    step_scale: f64,
    refinement_context: &JointRefinementContext,
) -> Option<PricingBlockProposal> {
    let requested_delta = least_squares_step.map(|component| step_scale * component);

    let (proposal_vector, applied_delta) =
        pricing_block_proposal_vector(current_vector, &requested_delta, refinement_context);

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
            predicted_score: predicted_squared_norm.max(0.0).sqrt(),
        })
}

fn pricing_block_proposal_vector(
    current_vector: &[f64],
    requested_delta: &[f64; 4],
    refinement_context: &JointRefinementContext,
) -> (Vec<f64>, [f64; 4]) {
    let mut proposal_vector = current_vector.to_vec();
    let mut applied_delta = [0.0; 4];

    for (coordinate_position, parameter_index) in refinement_context
        .active_parameter_layout
        .nig_block
        .iter()
        .enumerate()
    {
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

fn pricing_block_calibration_sample(
    parameter_vector: &[f64],
    refinement_context: &JointRefinementContext,
) -> Result<
    crate::stages::return_components::ReturnComponentsStageContinuousReturnCalibrationSample,
    String,
> {
    let fitted_parameters =
        parameter_vector_to_fitted_parameters(parameter_vector, refinement_context)
            .ok_or_else(|| "option pricing parameter vector failed to decode".to_string())?;

    let modeled_open_event_rate_per_hour =
        crate::stages::hawkes::stationary_open_event_rate_per_hour(
            &fitted_parameters.hawkes_fitted_parameters,
        )
        .ok_or_else(|| "option pricing modeled open event rate is unavailable".to_string())?;

    Ok(
        build_continuous_calibration_sample_from_aggregated_intervals(
            refinement_context
                .observed_data
                .observed_15m_intervals
                .as_slice(),
            modeled_open_event_rate_per_hour,
            refinement_context
                .observed_data
                .observed_open_event_rate_per_hour,
            refinement_context.observed_data.fallback_sell_share_proxy,
        ),
    )
}

fn search_coordinate_gradient_from_raw_nig_gradient(
    raw_gradient: &[f64],
    mapping: &NigSharedPlusShiftConfigurationSearchPointMapping,
) -> [f64; 4] {
    std::array::from_fn(|coordinate_index| {
        raw_gradient[0] * mapping.gradient.alpha_gradient[coordinate_index]
            + raw_gradient[1] * mapping.gradient.beta_0_gradient[coordinate_index]
            + raw_gradient[2] * mapping.gradient.delta_gradient[coordinate_index]
            + raw_gradient[3] * mapping.gradient.delta_beta_gradient[coordinate_index]
    })
}

fn maximum_bounded_line_step(
    current_vector: &[f64],
    direction: &[f64; 4],
    refinement_context: &JointRefinementContext,
) -> f64 {
    refinement_context
        .active_parameter_layout
        .nig_block
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
