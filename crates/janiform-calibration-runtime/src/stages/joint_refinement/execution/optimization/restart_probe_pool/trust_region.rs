use std::collections::BTreeMap;

use janiform_foundations::linear_algebra::{
    dot_product, least_squares_cauchy_step, least_squares_dogleg_step,
    mean_diagonal_damped_least_squares_step, minimum_norm_least_squares,
    scale_vector_to_euclidean_radius,
};

use super::super::parameter_space::{
    denormalize_parameter_vector, normalized_parameter_step, normalized_parameter_vector,
    parameter_step_is_zero,
};
use super::super::trust_region_policy::trust_region_reliability_scale;
use super::repair::restart_repair_score_from_components;
use super::{
    RestartObjectiveResidual, RestartProbePrediction, RestartProbeRecord, RestartProbeState,
};

#[derive(Debug, Clone)]
pub(super) struct TrustRegionCandidate {
    pub(super) parameter_vector: Vec<f64>,
    pub(super) label: String,
    pub(super) prediction: RestartProbePrediction,
}

pub(super) fn trust_region_candidate_vectors(
    state: &RestartProbeState,
    records: &[RestartProbeRecord],
    lower_bounds: &[f64],
    upper_bounds: &[f64],
) -> Vec<TrustRegionCandidate> {
    let Some(accepted_residuals) = state.accepted_residuals.as_ref() else {
        return Vec::new();
    };

    let Some(normalized_accepted) =
        normalized_parameter_vector(state.accepted_vector.as_slice(), lower_bounds, upper_bounds)
    else {
        return Vec::new();
    };

    let usable_records = usable_residual_probe_records(
        state.accepted_vector.as_slice(),
        records,
        lower_bounds,
        upper_bounds,
    );
    if usable_records.is_empty() {
        return Vec::new();
    }

    let Some(model) =
        fitted_residual_trust_region_model(accepted_residuals.as_slice(), usable_records)
    else {
        return Vec::new();
    };

    let Some(reliability_scale) =
        trust_region_reliability_scale(model.observed_delta_sum, model.prediction_error_sum)
    else {
        return Vec::new();
    };

    let trust_region_radius = state.trust_region_radius * reliability_scale;

    let candidate_projection = TrustRegionCandidateProjection {
        trust_region_radius,
        normalized_accepted: normalized_accepted.as_slice(),
        accepted_vector: state.accepted_vector.as_slice(),
        lower_bounds,
        upper_bounds,
        model: &model,
    };

    let mut candidates = Vec::new();

    let gauss_newton_step = minimum_norm_least_squares(
        model.sensitivity_rows.as_slice(),
        model.residual_rhs.as_slice(),
    );

    if let Some(gauss_newton_step) = gauss_newton_step.as_ref() {
        push_trust_region_candidate(
            &mut candidates,
            "gauss_newton",
            gauss_newton_step,
            &candidate_projection,
        );
    }

    if let Some(cauchy_step) = least_squares_cauchy_step(
        model.sensitivity_rows.as_slice(),
        model.accepted_residual_values.as_slice(),
    ) {
        push_trust_region_candidate(
            &mut candidates,
            "cauchy",
            cauchy_step.as_slice(),
            &candidate_projection,
        );

        if let Some(gauss_newton_step) = gauss_newton_step.as_ref()
            && let Some(dogleg_step) = least_squares_dogleg_step(
                cauchy_step.as_slice(),
                gauss_newton_step,
                trust_region_radius,
            )
        {
            push_trust_region_candidate(
                &mut candidates,
                "dogleg",
                dogleg_step.as_slice(),
                &candidate_projection,
            );
        }
    }

    if let Some(levenberg_marquardt_step) = mean_diagonal_damped_least_squares_step(
        model.sensitivity_rows.as_slice(),
        model.accepted_residual_values.as_slice(),
        TRUST_REGION_LEVENBERG_MARQUARDT_DAMPING_MEAN_DIAGONAL_MULTIPLIER,
    ) {
        push_trust_region_candidate(
            &mut candidates,
            "levenberg_marquardt",
            levenberg_marquardt_step.as_slice(),
            &candidate_projection,
        );
    }

    rank_trust_region_candidates(candidates.as_mut_slice());

    candidates
}

/// Unit mean-diagonal damping uses the average Jacobian curvature as the LM penalty scale.
const TRUST_REGION_LEVENBERG_MARQUARDT_DAMPING_MEAN_DIAGONAL_MULTIPLIER: f64 = 1.0;

#[derive(Debug)]
struct FittedResidualTrustRegionModel {
    residual_family_ids: Vec<String>,
    sensitivity_rows: Vec<Vec<f64>>,
    residual_rhs: Vec<f64>,
    accepted_residual_values: Vec<f64>,
    observed_delta_sum: f64,
    prediction_error_sum: f64,
}

#[derive(Debug)]
struct UsableResidualProbeRecord<'a> {
    normalized_step: Vec<f64>,
    residual_by_key: BTreeMap<(&'a str, &'a str), f64>,
}

struct TrustRegionCandidateProjection<'a> {
    trust_region_radius: f64,
    normalized_accepted: &'a [f64],
    accepted_vector: &'a [f64],
    lower_bounds: &'a [f64],
    upper_bounds: &'a [f64],
    model: &'a FittedResidualTrustRegionModel,
}

impl FittedResidualTrustRegionModel {
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

    fn predicted_repair_loss(&self, normalized_step: &[f64]) -> Option<f64> {
        let predicted_residual_values = self.predicted_residual_values(normalized_step)?;

        restart_repair_score_from_components(
            self.residual_family_ids
                .iter()
                .map(String::as_str)
                .zip(predicted_residual_values),
        )
        .map(|score| score.loss())
    }
}

fn fitted_residual_trust_region_model(
    accepted_residuals: &[RestartObjectiveResidual],
    usable_records: Vec<UsableResidualProbeRecord<'_>>,
) -> Option<FittedResidualTrustRegionModel> {
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

        residual_family_ids.push(residual.family_id.clone());
        sensitivity_rows.push(sensitivity_row);
        residual_rhs.push(-accepted_residual);
        accepted_residual_values.push(accepted_residual);
    }

    if observed_delta_sum <= 0.0 || !observed_delta_sum.is_finite() {
        return None;
    }

    Some(FittedResidualTrustRegionModel {
        residual_family_ids,
        sensitivity_rows,
        residual_rhs,
        accepted_residual_values,
        observed_delta_sum,
        prediction_error_sum,
    })
}

fn usable_residual_probe_records<'a>(
    accepted_vector: &[f64],
    records: &'a [RestartProbeRecord],
    lower_bounds: &[f64],
    upper_bounds: &[f64],
) -> Vec<UsableResidualProbeRecord<'a>> {
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
                UsableResidualProbeRecord {
                    normalized_step,
                    residual_by_key: residual_map(record.residuals.as_slice()),
                },
            )
        })
        .collect()
}

fn common_residuals<'a>(
    accepted_residuals: &'a [RestartObjectiveResidual],
    records: &[UsableResidualProbeRecord<'_>],
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

fn push_trust_region_candidate(
    candidates: &mut Vec<TrustRegionCandidate>,
    label: &'static str,
    step: &[f64],
    projection: &TrustRegionCandidateProjection<'_>,
) {
    let Some(trusted_step) = scale_vector_to_euclidean_radius(step, projection.trust_region_radius)
    else {
        return;
    };

    if parameter_step_is_zero(trusted_step.as_slice()) {
        return;
    }

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

    let Some(parameter_vector) = denormalize_parameter_vector(
        normalized_candidate.as_slice(),
        projection.lower_bounds,
        projection.upper_bounds,
    ) else {
        return;
    };

    if parameter_vector == projection.accepted_vector
        || candidates
            .iter()
            .any(|candidate| candidate.parameter_vector == parameter_vector)
    {
        return;
    }

    let predicted_residual_objective = projection
        .model
        .predicted_residual_objective(projected_step.as_slice());
    let predicted_repair_loss = projection
        .model
        .predicted_repair_loss(projected_step.as_slice());

    candidates.push(TrustRegionCandidate {
        parameter_vector,
        label: label.to_string(),
        prediction: RestartProbePrediction {
            residual_objective: predicted_residual_objective,
            repair_loss: predicted_repair_loss,
        },
    });
}

fn rank_trust_region_candidates(candidates: &mut [TrustRegionCandidate]) {
    candidates.sort_by(|left_candidate, right_candidate| {
        match (
            left_candidate.prediction.repair_loss,
            right_candidate.prediction.repair_loss,
        ) {
            (Some(left_objective), Some(right_objective)) => {
                left_objective.total_cmp(&right_objective)
            }
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => compare_predicted_residual_objective(left_candidate, right_candidate),
        }
    });
}

fn compare_predicted_residual_objective(
    left_candidate: &TrustRegionCandidate,
    right_candidate: &TrustRegionCandidate,
) -> std::cmp::Ordering {
    match (
        left_candidate.prediction.residual_objective,
        right_candidate.prediction.residual_objective,
    ) {
        (Some(left_objective), Some(right_objective)) => left_objective.total_cmp(&right_objective),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => left_candidate.label.cmp(&right_candidate.label),
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::trust_region_policy::trust_region_reliability_scale;
    use super::super::{
        RestartObjectiveResidual, RestartProbeRecord, restart_probe_state_for_test,
    };
    use super::trust_region_candidate_vectors;

    #[test]
    fn trust_region_reliability_uses_prediction_error_geometry() {
        assert_eq!(trust_region_reliability_scale(4.0, 0.0), Some(1.0));
        assert_eq!(trust_region_reliability_scale(4.0, 4.0), Some(0.5));
    }

    #[test]
    fn trust_region_candidates_use_existing_model_records() {
        let mut state = restart_probe_state_for_test(vec![0.5, 0.5], 0.25);

        state.accepted_residuals = Some(vec![
            RestartObjectiveResidual {
                family_id: "shape".to_string(),
                residual_id: "a".to_string(),
                objective_residual: 2.0,
            },
            RestartObjectiveResidual {
                family_id: "shape".to_string(),
                residual_id: "b".to_string(),
                objective_residual: 2.0,
            },
        ]);

        let records = vec![
            RestartProbeRecord {
                parameter_vector: vec![0.75, 0.5],
                residuals: vec![
                    RestartObjectiveResidual {
                        family_id: "shape".to_string(),
                        residual_id: "a".to_string(),
                        objective_residual: 1.0,
                    },
                    RestartObjectiveResidual {
                        family_id: "shape".to_string(),
                        residual_id: "b".to_string(),
                        objective_residual: 2.0,
                    },
                ],
            },
            RestartProbeRecord {
                parameter_vector: vec![0.5, 0.75],
                residuals: vec![
                    RestartObjectiveResidual {
                        family_id: "shape".to_string(),
                        residual_id: "a".to_string(),
                        objective_residual: 2.0,
                    },
                    RestartObjectiveResidual {
                        family_id: "shape".to_string(),
                        residual_id: "b".to_string(),
                        objective_residual: 1.0,
                    },
                ],
            },
        ];

        let candidates =
            trust_region_candidate_vectors(&state, records.as_slice(), &[0.0, 0.0], &[1.0, 1.0]);

        assert!(!candidates.is_empty());
        assert!(candidates[0].parameter_vector[0] > state.accepted_vector[0]);
        assert!(candidates[0].parameter_vector[1] > state.accepted_vector[1]);
        assert!(candidates[0].parameter_vector[0] <= 0.75);
        assert!(candidates[0].parameter_vector[1] <= 0.75);
    }
}
