use janiform_calibration_artifacts::CalibrationLossTermDiagnostic;

use janiform_foundations::linear_algebra::mean_diagonal_damped_least_squares_step;

// Damping is measured in the mean diagonal units of J'J, so a unit multiplier
// keeps the repair between an undamped Gauss-Newton step and a gradient step.
const NORMAL_EQUATION_DAMPING_MEAN_DIAGONAL_MULTIPLIER: f64 = 1.0;

// Trust-region candidates use the standard bisection ladder: 1, 1/2, 1/4, ...
const TRUST_REGION_SCALE_BISECTION_BASE: f64 = 2.0;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SensitivityRepairEvaluation {
    pub(crate) aggregate_score: f64,
    pub(crate) term_diagnostics: Vec<CalibrationLossTermDiagnostic>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SensitivityRepairSeed {
    pub(crate) restart_id: String,
    pub(crate) parameter_vector: Vec<f64>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct SensitivityRepairSeedPolicy {
    pub(crate) active_anchor_count: usize,
    pub(crate) trust_region_scale_count: usize,
}

#[derive(Debug, Clone, Copy)]
struct ActiveResidual<'a> {
    term_id: &'a str,
    standardized_residual: f64,
}

pub(crate) fn sensitivity_repair_seed_candidates<F>(
    anchor_restart_id: &str,
    anchor_parameter_vector: &[f64],
    lower_bounds: &[f64],
    upper_bounds: &[f64],
    admissibility_z_threshold: f64,
    policy: SensitivityRepairSeedPolicy,
    evaluate: &F,
) -> Vec<SensitivityRepairSeed>
where
    F: Fn(&[f64]) -> Option<SensitivityRepairEvaluation> + Sync + ?Sized,
{
    if anchor_parameter_vector.len() != lower_bounds.len()
        || anchor_parameter_vector.len() != upper_bounds.len()
    {
        return Vec::new();
    }

    let Some(anchor_evaluation) = evaluate(anchor_parameter_vector) else {
        return Vec::new();
    };

    let active_residuals = active_residuals(
        anchor_evaluation.term_diagnostics.as_slice(),
        admissibility_z_threshold,
    );
    if active_residuals.is_empty() {
        return Vec::new();
    }

    let active_parameter_positions =
        active_parameter_positions(anchor_parameter_vector, lower_bounds, upper_bounds);
    if active_parameter_positions.is_empty() {
        return Vec::new();
    }

    let normalized_anchor =
        normalize_parameter_vector(anchor_parameter_vector, lower_bounds, upper_bounds);
    let finite_difference_radius = simplex_coordinate_radius(active_parameter_positions.len());

    let sensitivity_matrix = residual_sensitivity_matrix(
        active_residuals.as_slice(),
        normalized_anchor.as_slice(),
        lower_bounds,
        upper_bounds,
        active_parameter_positions.as_slice(),
        finite_difference_radius,
        evaluate,
    );
    if sensitivity_matrix.is_empty() {
        return Vec::new();
    }

    let residual_vector = active_residuals
        .iter()
        .map(|residual| residual.standardized_residual)
        .collect::<Vec<_>>();

    let Some(repair_step) = mean_diagonal_damped_least_squares_step(
        sensitivity_matrix.as_slice(),
        residual_vector.as_slice(),
        NORMAL_EQUATION_DAMPING_MEAN_DIAGONAL_MULTIPLIER,
    ) else {
        return Vec::new();
    };

    let bounded_repair_step = bound_normalized_repair_step(
        normalized_anchor.as_slice(),
        active_parameter_positions.as_slice(),
        repair_step.as_slice(),
        finite_difference_radius,
    );
    if repair_step_is_zero(bounded_repair_step.as_slice()) {
        return Vec::new();
    }

    trust_region_scale_factors(policy.trust_region_scale_count)
        .into_iter()
        .enumerate()
        .filter_map(|(scale_position, scale_factor)| {
            let normalized_candidate = apply_scaled_repair_step(
                normalized_anchor.as_slice(),
                active_parameter_positions.as_slice(),
                bounded_repair_step.as_slice(),
                scale_factor,
            );

            let parameter_vector = denormalize_parameter_vector(
                normalized_candidate.as_slice(),
                lower_bounds,
                upper_bounds,
            );
            (parameter_vector != anchor_parameter_vector).then(|| SensitivityRepairSeed {
                restart_id: format!(
                    "{anchor_restart_id}_sensitivity_repair_{:02}",
                    scale_position + 1
                ),
                parameter_vector,
            })
        })
        .collect()
}

pub(crate) fn sensitivity_repair_screening_evaluation_budget(
    parameter_count: usize,
    policy: SensitivityRepairSeedPolicy,
) -> usize {
    let per_anchor_budget = 1usize
        .saturating_add(parameter_count.saturating_mul(2))
        .saturating_add(policy.trust_region_scale_count);

    policy.active_anchor_count.saturating_mul(per_anchor_budget)
}

fn active_residuals(
    term_diagnostics: &[CalibrationLossTermDiagnostic],
    admissibility_z_threshold: f64,
) -> Vec<ActiveResidual<'_>> {
    term_diagnostics
        .iter()
        .filter_map(|term| {
            let standardized_residual = term.standardized_residual?;
            let residual_is_active = standardized_residual.is_finite()
                && standardized_residual.abs() > admissibility_z_threshold;

            residual_is_active.then_some(ActiveResidual {
                term_id: term.term_id.as_str(),
                standardized_residual,
            })
        })
        .collect()
}

fn active_parameter_positions(
    parameter_vector: &[f64],
    lower_bounds: &[f64],
    upper_bounds: &[f64],
) -> Vec<usize> {
    parameter_vector
        .iter()
        .zip(lower_bounds.iter().zip(upper_bounds.iter()))
        .enumerate()
        .filter_map(
            |(parameter_position, (parameter_value, (lower_bound, upper_bound)))| {
                let width = upper_bound - lower_bound;
                (parameter_value.is_finite()
                    && lower_bound.is_finite()
                    && upper_bound.is_finite()
                    && width > 0.0)
                    .then_some(parameter_position)
            },
        )
        .collect()
}

fn simplex_coordinate_radius(active_parameter_count: usize) -> f64 {
    (active_parameter_count.saturating_add(1) as f64).recip()
}

fn residual_sensitivity_matrix<F>(
    active_residuals: &[ActiveResidual<'_>],
    normalized_anchor: &[f64],
    lower_bounds: &[f64],
    upper_bounds: &[f64],
    active_parameter_positions: &[usize],
    finite_difference_radius: f64,
    evaluate: &F,
) -> Vec<Vec<f64>>
where
    F: Fn(&[f64]) -> Option<SensitivityRepairEvaluation> + Sync + ?Sized,
{
    let mut sensitivity_matrix =
        vec![vec![0.0; active_parameter_positions.len()]; active_residuals.len()];

    for (column_position, parameter_position) in
        active_parameter_positions.iter().copied().enumerate()
    {
        let mut positive_probe = normalized_anchor.to_vec();
        positive_probe[parameter_position] =
            (positive_probe[parameter_position] + finite_difference_radius).min(1.0);

        let mut negative_probe = normalized_anchor.to_vec();
        negative_probe[parameter_position] =
            (negative_probe[parameter_position] - finite_difference_radius).max(0.0);

        let positive_step =
            positive_probe[parameter_position] - normalized_anchor[parameter_position];
        let negative_step =
            normalized_anchor[parameter_position] - negative_probe[parameter_position];

        let positive_evaluation = (positive_step > 0.0)
            .then(|| {
                denormalize_parameter_vector(positive_probe.as_slice(), lower_bounds, upper_bounds)
            })
            .and_then(|probe| evaluate(probe.as_slice()));

        let negative_evaluation = (negative_step > 0.0)
            .then(|| {
                denormalize_parameter_vector(negative_probe.as_slice(), lower_bounds, upper_bounds)
            })
            .and_then(|probe| evaluate(probe.as_slice()));

        for (row_position, active_residual) in active_residuals.iter().enumerate() {
            if let Some(sensitivity) = residual_sensitivity(
                active_residual,
                positive_evaluation.as_ref(),
                positive_step,
                negative_evaluation.as_ref(),
                negative_step,
            ) {
                sensitivity_matrix[row_position][column_position] = sensitivity;
            }
        }
    }

    sensitivity_matrix
}

fn residual_sensitivity(
    active_residual: &ActiveResidual<'_>,
    positive_evaluation: Option<&SensitivityRepairEvaluation>,
    positive_step: f64,
    negative_evaluation: Option<&SensitivityRepairEvaluation>,
    negative_step: f64,
) -> Option<f64> {
    match (positive_evaluation, negative_evaluation) {
        (Some(positive), Some(negative)) => {
            let positive_residual = residual_by_term_id(positive, active_residual.term_id)?;
            let negative_residual = residual_by_term_id(negative, active_residual.term_id)?;
            let denominator = positive_step + negative_step;
            (denominator > 0.0).then_some((positive_residual - negative_residual) / denominator)
        }
        (Some(positive), None) => {
            let positive_residual = residual_by_term_id(positive, active_residual.term_id)?;
            (positive_step > 0.0).then_some(
                (positive_residual - active_residual.standardized_residual) / positive_step,
            )
        }
        (None, Some(negative)) => {
            let negative_residual = residual_by_term_id(negative, active_residual.term_id)?;
            (negative_step > 0.0).then_some(
                (active_residual.standardized_residual - negative_residual) / negative_step,
            )
        }
        (None, None) => None,
    }
}

fn residual_by_term_id(evaluation: &SensitivityRepairEvaluation, term_id: &str) -> Option<f64> {
    evaluation
        .term_diagnostics
        .iter()
        .find(|term| term.term_id == term_id)
        .and_then(|term| term.standardized_residual)
        .filter(|residual| residual.is_finite())
}

fn bound_normalized_repair_step(
    normalized_anchor: &[f64],
    active_parameter_positions: &[usize],
    repair_step: &[f64],
    finite_difference_radius: f64,
) -> Vec<f64> {
    let trust_region_radius =
        finite_difference_radius * (active_parameter_positions.len().max(1) as f64).sqrt();

    let repair_norm = repair_step
        .iter()
        .map(|component| component.powi(2))
        .sum::<f64>()
        .sqrt();

    let trust_region_scale = if repair_norm > trust_region_radius && repair_norm > 0.0 {
        trust_region_radius / repair_norm
    } else {
        1.0
    };

    repair_step
        .iter()
        .zip(active_parameter_positions.iter())
        .map(|(component, parameter_position)| {
            let scaled_component = component * trust_region_scale;
            let lower_step = -normalized_anchor[*parameter_position];
            let upper_step = 1.0 - normalized_anchor[*parameter_position];

            scaled_component.clamp(lower_step, upper_step)
        })
        .collect()
}

fn repair_step_is_zero(repair_step: &[f64]) -> bool {
    repair_step.iter().all(|component| *component == 0.0)
}

fn trust_region_scale_factors(scale_count: usize) -> Vec<f64> {
    if scale_count == 0 {
        return Vec::new();
    }

    (0..scale_count)
        .rev()
        .map(|scale_position| TRUST_REGION_SCALE_BISECTION_BASE.powi(-(scale_position as i32)))
        .collect()
}

fn apply_scaled_repair_step(
    normalized_anchor: &[f64],
    active_parameter_positions: &[usize],
    repair_step: &[f64],
    scale_factor: f64,
) -> Vec<f64> {
    let mut normalized_candidate = normalized_anchor.to_vec();

    for (component, parameter_position) in repair_step.iter().zip(active_parameter_positions.iter())
    {
        normalized_candidate[*parameter_position] =
            (normalized_candidate[*parameter_position] + component * scale_factor).clamp(0.0, 1.0);
    }

    normalized_candidate
}

fn normalize_parameter_vector(
    parameter_vector: &[f64],
    lower_bounds: &[f64],
    upper_bounds: &[f64],
) -> Vec<f64> {
    parameter_vector
        .iter()
        .zip(lower_bounds.iter().zip(upper_bounds.iter()))
        .map(|(parameter_value, (lower_bound, upper_bound))| {
            let width = upper_bound - lower_bound;
            if width > 0.0 {
                ((parameter_value - lower_bound) / width).clamp(0.0, 1.0)
            } else {
                0.0
            }
        })
        .collect()
}

fn denormalize_parameter_vector(
    normalized_parameter_vector: &[f64],
    lower_bounds: &[f64],
    upper_bounds: &[f64],
) -> Vec<f64> {
    normalized_parameter_vector
        .iter()
        .zip(lower_bounds.iter().zip(upper_bounds.iter()))
        .map(|(normalized_value, (lower_bound, upper_bound))| {
            lower_bound + normalized_value.clamp(0.0, 1.0) * (upper_bound - lower_bound)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use janiform_calibration_artifacts::{CalibrationLossTermDiagnostic, CalibrationLossTermKind};

    use super::{
        SensitivityRepairEvaluation, SensitivityRepairSeedPolicy,
        sensitivity_repair_seed_candidates,
    };

    fn residual_term(term_id: &str, standardized_residual: f64) -> CalibrationLossTermDiagnostic {
        CalibrationLossTermDiagnostic {
            term_id: term_id.to_string(),
            family_id: "test".to_string(),
            term_kind: CalibrationLossTermKind::StylizedFact,
            observed_value: None,
            model_value: None,
            observed_standard_error: None,
            model_standard_error: None,
            uncertainty_scale: None,
            standardized_residual: Some(standardized_residual),
            loss: standardized_residual.abs(),
            admissibility_blocker: standardized_residual.abs() > 1.0,
        }
    }

    #[test]
    fn sensitivity_repair_seed_moves_against_general_residual_jacobian() {
        let evaluate = |parameter_vector: &[f64]| -> Option<SensitivityRepairEvaluation> {
            let first_residual = 4.0 + 2.0 * parameter_vector[0] - parameter_vector[1];
            let second_residual = -3.0 + parameter_vector[0] + 2.0 * parameter_vector[1];

            Some(SensitivityRepairEvaluation {
                aggregate_score: first_residual.abs() + second_residual.abs(),
                term_diagnostics: vec![
                    residual_term("shape.first", first_residual),
                    residual_term("shape.second", second_residual),
                ],
            })
        };

        let seeds = sensitivity_repair_seed_candidates(
            "anchor",
            &[0.0, 0.0],
            &[-2.0, -2.0],
            &[2.0, 2.0],
            1.0,
            SensitivityRepairSeedPolicy {
                active_anchor_count: 1,
                trust_region_scale_count: 3,
            },
            &evaluate,
        );

        assert_eq!(seeds.len(), 3);

        let anchor_score = evaluate(&[0.0, 0.0])
            .expect("anchor evaluation should exist")
            .aggregate_score;
        let best_seed_score = seeds
            .iter()
            .filter_map(|seed| evaluate(seed.parameter_vector.as_slice()))
            .map(|evaluation| evaluation.aggregate_score)
            .fold(f64::INFINITY, f64::min);

        assert!(best_seed_score < anchor_score);
    }

    #[test]
    fn sensitivity_repair_seed_uses_only_active_residual_terms() {
        let evaluate = |parameter_vector: &[f64]| -> Option<SensitivityRepairEvaluation> {
            Some(SensitivityRepairEvaluation {
                aggregate_score: parameter_vector[0].abs(),
                term_diagnostics: vec![
                    residual_term("inactive", 0.1 + parameter_vector[0] * 100.0),
                    residual_term("active", 4.0 + parameter_vector[0]),
                ],
            })
        };

        let seeds = sensitivity_repair_seed_candidates(
            "anchor",
            &[0.0],
            &[-2.0],
            &[2.0],
            1.0,
            SensitivityRepairSeedPolicy {
                active_anchor_count: 1,
                trust_region_scale_count: 1,
            },
            &evaluate,
        );

        assert_eq!(seeds.len(), 1);
        assert!(seeds[0].parameter_vector[0] < 0.0);
    }
}
