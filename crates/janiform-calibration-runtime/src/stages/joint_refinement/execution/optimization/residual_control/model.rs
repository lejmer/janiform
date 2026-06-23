use std::cmp::Ordering;
use std::collections::BTreeMap;

use janiform_foundations::linear_algebra::{
    dot_product, least_squares_cauchy_step, least_squares_dogleg_step,
    mean_diagonal_damped_least_squares_step, minimum_norm_least_squares,
    scale_vector_to_euclidean_radius,
};

use super::super::super::JointRefinementContext;
use super::super::parameter_groups::parameter_group_probe_directions;
use super::super::parameter_space::{
    denormalize_parameter_vector, movable_parameter_count, normalized_parameter_step,
    normalized_parameter_vector, parameter_step_is_zero,
};
use super::super::residual_profile::ResidualTailProfile;
use super::super::trust_region_policy::trust_region_reliability_scale;
use super::super::workflow::{
    RestartObjectiveResidual, joint_refinement_restart_full_vector_initial_mesh_divisor,
};
use super::{
    RESIDUAL_CONTROL_LEVENBERG_MARQUARDT_DAMPING_MEAN_DIAGONAL_MULTIPLIER,
    ResidualControlCandidate, ResidualControlPrediction, ResidualControlRecord,
    ResidualControlState,
};

pub(super) fn control_probe_candidate_vectors(
    state: &ResidualControlState,
    lower_bounds: &[f64],
    upper_bounds: &[f64],
    context: &JointRefinementContext,
) -> Vec<ResidualControlCandidate> {
    let Some(normalized_accepted) =
        normalized_parameter_vector(state.accepted_vector.as_slice(), lower_bounds, upper_bounds)
    else {
        return Vec::new();
    };

    let directions = parameter_group_probe_directions(
        "control_probe",
        state.accepted_vector.as_slice(),
        lower_bounds,
        upper_bounds,
        &context.active_parameter_layout,
    );

    let mut candidates = Vec::with_capacity(directions.len());

    for direction in directions {
        push_projected_residual_control_candidate(
            &mut candidates,
            ProjectedResidualControlCandidateInput {
                label: direction.label.as_str(),
                step: direction.direction.as_slice(),
                trust_region_radius: state.trust_region_radius,
                normalized_accepted: normalized_accepted.as_slice(),
                accepted_vector: state.accepted_vector.as_slice(),
                lower_bounds,
                upper_bounds,
                prediction: ResidualControlPrediction::default(),
            },
        );
    }

    candidates
}

pub(super) fn residual_repair_candidate_vectors(
    state: &ResidualControlState,
    lower_bounds: &[f64],
    upper_bounds: &[f64],
) -> Vec<ResidualControlCandidate> {
    let Some(accepted_residuals) = state.accepted_residuals.as_ref() else {
        return Vec::new();
    };

    let Some(normalized_accepted) =
        normalized_parameter_vector(state.accepted_vector.as_slice(), lower_bounds, upper_bounds)
    else {
        return Vec::new();
    };

    let usable_records = usable_residual_control_records(
        state.accepted_vector.as_slice(),
        state.records.as_slice(),
        lower_bounds,
        upper_bounds,
    );
    if usable_records.is_empty() {
        return Vec::new();
    }

    let Some(model) = fitted_residual_control_model(accepted_residuals.as_slice(), usable_records)
    else {
        return Vec::new();
    };

    let Some(reliability_scale) =
        trust_region_reliability_scale(model.observed_delta_sum, model.prediction_error_sum)
    else {
        return Vec::new();
    };

    let trust_region_radius = state.trust_region_radius * reliability_scale;

    let projection = ResidualControlCandidateProjection {
        trust_region_radius,
        normalized_accepted: normalized_accepted.as_slice(),
        accepted_vector: state.accepted_vector.as_slice(),
        lower_bounds,
        upper_bounds,
        model: &model,
    };

    let mut candidates = Vec::new();

    if let Some(gauss_newton_step) = minimum_norm_least_squares(
        model.sensitivity_rows.as_slice(),
        model.residual_rhs.as_slice(),
    ) {
        push_residual_repair_candidate(
            &mut candidates,
            "gauss_newton",
            gauss_newton_step.as_slice(),
            &projection,
        );
    }

    if let Some((weighted_rows, weighted_rhs)) = weighted_residual_control_system(&model)
        && let Some(weighted_step) =
            minimum_norm_least_squares(weighted_rows.as_slice(), weighted_rhs.as_slice())
    {
        push_residual_repair_candidate(
            &mut candidates,
            "tail_weighted_gauss_newton",
            weighted_step.as_slice(),
            &projection,
        );
    }

    if let Some(cauchy_step) = least_squares_cauchy_step(
        model.sensitivity_rows.as_slice(),
        model.accepted_residual_values.as_slice(),
    ) {
        push_residual_repair_candidate(
            &mut candidates,
            "cauchy",
            cauchy_step.as_slice(),
            &projection,
        );

        if let Some(gauss_newton_step) = minimum_norm_least_squares(
            model.sensitivity_rows.as_slice(),
            model.residual_rhs.as_slice(),
        ) && let Some(dogleg_step) = least_squares_dogleg_step(
            cauchy_step.as_slice(),
            gauss_newton_step.as_slice(),
            trust_region_radius,
        ) {
            push_residual_repair_candidate(
                &mut candidates,
                "dogleg",
                dogleg_step.as_slice(),
                &projection,
            );
        }
    }

    if let Some(levenberg_marquardt_step) = mean_diagonal_damped_least_squares_step(
        model.sensitivity_rows.as_slice(),
        model.accepted_residual_values.as_slice(),
        RESIDUAL_CONTROL_LEVENBERG_MARQUARDT_DAMPING_MEAN_DIAGONAL_MULTIPLIER,
    ) {
        push_residual_repair_candidate(
            &mut candidates,
            "levenberg_marquardt",
            levenberg_marquardt_step.as_slice(),
            &projection,
        );
    }

    rank_residual_repair_candidates(candidates.as_mut_slice());

    candidates
}

fn weighted_residual_control_system(
    model: &FittedResidualControlModel,
) -> Option<(Vec<Vec<f64>>, Vec<f64>)> {
    let mut weighted_rows = Vec::new();
    let mut weighted_rhs = Vec::new();

    for ((sensitivity_row, residual_rhs), accepted_residual) in model
        .sensitivity_rows
        .iter()
        .zip(model.residual_rhs.iter())
        .zip(model.accepted_residual_values.iter())
    {
        let weight = accepted_residual.abs();
        if weight == 0.0 || !weight.is_finite() {
            continue;
        }

        weighted_rows.push(
            sensitivity_row
                .iter()
                .map(|component| component * weight)
                .collect::<Vec<_>>(),
        );
        weighted_rhs.push(residual_rhs * weight);
    }

    (!weighted_rows.is_empty()).then_some((weighted_rows, weighted_rhs))
}

fn push_residual_repair_candidate(
    candidates: &mut Vec<ResidualControlCandidate>,
    label: &'static str,
    step: &[f64],
    projection: &ResidualControlCandidateProjection<'_>,
) {
    let Some(trusted_step) = scale_vector_to_euclidean_radius(step, projection.trust_region_radius)
    else {
        return;
    };

    let normalized_candidate = projection
        .normalized_accepted
        .iter()
        .zip(trusted_step.iter())
        .map(|(coordinate, step)| (*coordinate + *step).clamp(0.0, 1.0))
        .collect::<Vec<_>>();

    let projected_step = normalized_candidate
        .iter()
        .zip(projection.normalized_accepted.iter())
        .map(|(candidate_coordinate, accepted_coordinate)| {
            candidate_coordinate - accepted_coordinate
        })
        .collect::<Vec<_>>();

    let prediction = ResidualControlPrediction {
        profile: projection
            .model
            .predicted_profile(projected_step.as_slice()),
        residual_objective: projection
            .model
            .predicted_residual_objective(projected_step.as_slice()),
    };

    push_normalized_residual_control_candidate(
        candidates,
        NormalizedResidualControlCandidateInput {
            label,
            normalized_candidate: normalized_candidate.as_slice(),
            accepted_vector: projection.accepted_vector,
            lower_bounds: projection.lower_bounds,
            upper_bounds: projection.upper_bounds,
            prediction,
        },
    );
}

fn push_projected_residual_control_candidate(
    candidates: &mut Vec<ResidualControlCandidate>,
    input: ProjectedResidualControlCandidateInput<'_>,
) {
    let Some(trusted_step) =
        scale_vector_to_euclidean_radius(input.step, input.trust_region_radius)
    else {
        return;
    };

    if parameter_step_is_zero(trusted_step.as_slice()) {
        return;
    }

    let normalized_candidate = input
        .normalized_accepted
        .iter()
        .zip(trusted_step.iter())
        .map(|(coordinate, step)| (*coordinate + *step).clamp(0.0, 1.0))
        .collect::<Vec<_>>();

    push_normalized_residual_control_candidate(
        candidates,
        NormalizedResidualControlCandidateInput {
            label: input.label,
            normalized_candidate: normalized_candidate.as_slice(),
            accepted_vector: input.accepted_vector,
            lower_bounds: input.lower_bounds,
            upper_bounds: input.upper_bounds,
            prediction: input.prediction,
        },
    );
}

fn push_normalized_residual_control_candidate(
    candidates: &mut Vec<ResidualControlCandidate>,
    input: NormalizedResidualControlCandidateInput<'_>,
) {
    let Some(parameter_vector) = denormalize_parameter_vector(
        input.normalized_candidate,
        input.lower_bounds,
        input.upper_bounds,
    ) else {
        return;
    };

    if parameter_vector == input.accepted_vector
        || candidates
            .iter()
            .any(|candidate| candidate.parameter_vector == parameter_vector)
    {
        return;
    }

    candidates.push(ResidualControlCandidate {
        parameter_vector,
        label: input.label.to_string(),
        prediction: input.prediction,
    });
}

fn rank_residual_repair_candidates(candidates: &mut [ResidualControlCandidate]) {
    candidates.sort_by(|left_candidate, right_candidate| {
        compare_predicted_profile(
            left_candidate.prediction.profile.as_ref(),
            right_candidate.prediction.profile.as_ref(),
        )
        .then_with(|| compare_predicted_residual_objective(left_candidate, right_candidate))
        .then_with(|| left_candidate.label.cmp(&right_candidate.label))
    });
}

fn compare_predicted_profile(
    left_profile: Option<&ResidualTailProfile>,
    right_profile: Option<&ResidualTailProfile>,
) -> Ordering {
    match (left_profile, right_profile) {
        (Some(left_profile), Some(right_profile)) => left_profile.compare(right_profile),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

fn compare_predicted_residual_objective(
    left_candidate: &ResidualControlCandidate,
    right_candidate: &ResidualControlCandidate,
) -> Ordering {
    match (
        left_candidate.prediction.residual_objective,
        right_candidate.prediction.residual_objective,
    ) {
        (Some(left_objective), Some(right_objective)) => left_objective.total_cmp(&right_objective),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

#[derive(Debug)]
struct FittedResidualControlModel {
    residual_family_ids: Vec<String>,
    residual_ids: Vec<String>,
    sensitivity_rows: Vec<Vec<f64>>,
    residual_rhs: Vec<f64>,
    accepted_residual_values: Vec<f64>,
    observed_delta_sum: f64,
    prediction_error_sum: f64,
}

#[derive(Debug)]
struct UsableResidualControlRecord<'a> {
    normalized_step: Vec<f64>,
    residual_by_key: BTreeMap<(&'a str, &'a str), f64>,
}

struct ResidualControlCandidateProjection<'a> {
    trust_region_radius: f64,
    normalized_accepted: &'a [f64],
    accepted_vector: &'a [f64],
    lower_bounds: &'a [f64],
    upper_bounds: &'a [f64],
    model: &'a FittedResidualControlModel,
}

struct ProjectedResidualControlCandidateInput<'a> {
    label: &'a str,
    step: &'a [f64],
    trust_region_radius: f64,
    normalized_accepted: &'a [f64],
    accepted_vector: &'a [f64],
    lower_bounds: &'a [f64],
    upper_bounds: &'a [f64],
    prediction: ResidualControlPrediction,
}

struct NormalizedResidualControlCandidateInput<'a> {
    label: &'a str,
    normalized_candidate: &'a [f64],
    accepted_vector: &'a [f64],
    lower_bounds: &'a [f64],
    upper_bounds: &'a [f64],
    prediction: ResidualControlPrediction,
}

impl FittedResidualControlModel {
    fn predicted_residual_values(&self, normalized_step: &[f64]) -> Option<Vec<f64>> {
        if normalized_step.len() != self.sensitivity_rows.first()?.len() {
            return None;
        }

        self.accepted_residual_values
            .iter()
            .zip(self.sensitivity_rows.iter())
            .map(|(accepted_residual, sensitivity_row)| {
                dot_product(sensitivity_row.as_slice(), normalized_step)
                    .map(|predicted_delta| accepted_residual + predicted_delta)
            })
            .collect()
    }

    fn predicted_residuals(
        &self,
        normalized_step: &[f64],
    ) -> Option<Vec<RestartObjectiveResidual>> {
        // Predicted residuals own identifiers so they can reuse the common tail-profile contract.
        self.predicted_residual_values(normalized_step)?
            .into_iter()
            .enumerate()
            .map(|(residual_position, objective_residual)| {
                Some(RestartObjectiveResidual {
                    family_id: self.residual_family_ids.get(residual_position)?.clone(),
                    residual_id: self.residual_ids.get(residual_position)?.clone(),
                    objective_residual,
                })
            })
            .collect()
    }

    fn predicted_profile(&self, normalized_step: &[f64]) -> Option<ResidualTailProfile> {
        ResidualTailProfile::from_residuals(self.predicted_residuals(normalized_step)?.as_slice())
    }

    fn predicted_residual_objective(&self, normalized_step: &[f64]) -> Option<f64> {
        let predicted_residual_sum = self
            .predicted_residual_values(normalized_step)?
            .into_iter()
            .map(|predicted_residual| predicted_residual * predicted_residual)
            .sum::<f64>();

        predicted_residual_sum
            .is_finite()
            .then_some(predicted_residual_sum)
    }
}

fn fitted_residual_control_model(
    accepted_residuals: &[RestartObjectiveResidual],
    usable_records: Vec<UsableResidualControlRecord<'_>>,
) -> Option<FittedResidualControlModel> {
    let accepted_residual_by_key = residual_map(accepted_residuals);

    let common_residuals = common_residuals(accepted_residuals, usable_records.as_slice());
    if common_residuals.is_empty() {
        return None;
    }

    let mut design_rows = Vec::with_capacity(usable_records.len());
    let mut residual_maps = Vec::with_capacity(usable_records.len());

    for record in usable_records {
        design_rows.push(record.normalized_step);
        residual_maps.push(record.residual_by_key);
    }

    let mut residual_family_ids = Vec::with_capacity(common_residuals.len());
    let mut residual_ids = Vec::with_capacity(common_residuals.len());
    let mut sensitivity_rows = Vec::with_capacity(common_residuals.len());
    let mut residual_rhs = Vec::with_capacity(common_residuals.len());
    let mut accepted_residual_values = Vec::with_capacity(common_residuals.len());

    let mut prediction_error_sum = 0.0;
    let mut observed_delta_sum = 0.0;

    for residual in common_residuals {
        let residual_key = residual_key(residual);
        let accepted_residual = *accepted_residual_by_key.get(&residual_key)?;

        let observed_deltas = residual_maps
            .iter()
            .map(|residual_by_key| residual_by_key.get(&residual_key).copied())
            .map(|residual| residual.map(|value| value - accepted_residual))
            .collect::<Option<Vec<_>>>()?;

        let sensitivity_row =
            minimum_norm_least_squares(design_rows.as_slice(), observed_deltas.as_slice())?;

        for (normalized_step, observed_delta) in design_rows.iter().zip(observed_deltas.iter()) {
            let predicted_delta =
                dot_product(normalized_step.as_slice(), sensitivity_row.as_slice())?;

            let prediction_error = predicted_delta - observed_delta;

            prediction_error_sum += prediction_error * prediction_error;
            observed_delta_sum += observed_delta * observed_delta;
        }

        // The fitted model owns residual identifiers after the accepted residual slice is gone.
        residual_family_ids.push(residual.family_id.clone());
        residual_ids.push(residual.residual_id.clone());
        sensitivity_rows.push(sensitivity_row);
        residual_rhs.push(-accepted_residual);
        accepted_residual_values.push(accepted_residual);
    }

    if observed_delta_sum <= 0.0 || !observed_delta_sum.is_finite() {
        return None;
    }

    Some(FittedResidualControlModel {
        residual_family_ids,
        residual_ids,
        sensitivity_rows,
        residual_rhs,
        accepted_residual_values,
        observed_delta_sum,
        prediction_error_sum,
    })
}

fn usable_residual_control_records<'a>(
    accepted_vector: &[f64],
    records: &'a [ResidualControlRecord],
    lower_bounds: &[f64],
    upper_bounds: &[f64],
) -> Vec<UsableResidualControlRecord<'a>> {
    records
        .iter()
        .filter_map(|record| {
            let normalized_step = normalized_parameter_step(
                accepted_vector,
                record.parameter_vector.as_slice(),
                lower_bounds,
                upper_bounds,
            )?;

            (!parameter_step_is_zero(normalized_step.as_slice())).then_some(
                UsableResidualControlRecord {
                    normalized_step,
                    residual_by_key: residual_map(record.residuals.as_slice()),
                },
            )
        })
        .collect()
}

fn common_residuals<'a>(
    accepted_residuals: &'a [RestartObjectiveResidual],
    records: &[UsableResidualControlRecord<'_>],
) -> Vec<&'a RestartObjectiveResidual> {
    accepted_residuals
        .iter()
        .filter(|residual| {
            let residual_key = residual_key(residual);

            records
                .iter()
                .all(|record| record.residual_by_key.contains_key(&residual_key))
        })
        .collect()
}

fn residual_map(residuals: &[RestartObjectiveResidual]) -> BTreeMap<(&str, &str), f64> {
    residuals
        .iter()
        .map(|residual| (residual_key(residual), residual.objective_residual))
        .collect()
}

fn residual_key(residual: &RestartObjectiveResidual) -> (&str, &str) {
    (residual.family_id.as_str(), residual.residual_id.as_str())
}

pub(super) fn initial_residual_control_trust_region_radius(
    lower_bounds: &[f64],
    upper_bounds: &[f64],
) -> f64 {
    let active_parameter_count = movable_parameter_count(lower_bounds, upper_bounds).max(1);

    (joint_refinement_restart_full_vector_initial_mesh_divisor(active_parameter_count) as f64)
        .recip()
}
