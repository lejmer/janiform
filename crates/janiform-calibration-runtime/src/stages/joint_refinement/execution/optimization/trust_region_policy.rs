use super::workflow::RestartObjectiveResidual;

pub(super) fn residual_squared_objective(residuals: &[RestartObjectiveResidual]) -> f64 {
    residuals
        .iter()
        .map(|residual| residual.objective_residual * residual.objective_residual)
        .sum()
}

pub(super) fn refined_trust_region_radius(
    current_radius: f64,
    refinement_divisor: usize,
) -> Option<f64> {
    if !current_radius.is_finite() || current_radius <= 0.0 || refinement_divisor < 2 {
        return None;
    }

    let refined_radius = current_radius / refinement_divisor as f64;

    (refined_radius.is_finite() && refined_radius > 0.0 && refined_radius < current_radius)
        .then_some(refined_radius)
}

pub(super) fn trust_region_radius_after_prediction(
    current_radius: f64,
    accepted_residual_objective: f64,
    predicted_residual_objective: f64,
    observed_residual_objective: f64,
    lower_bounds: &[f64],
    upper_bounds: &[f64],
) -> Option<f64> {
    if !current_radius.is_finite()
        || current_radius <= 0.0
        || !accepted_residual_objective.is_finite()
        || !predicted_residual_objective.is_finite()
        || !observed_residual_objective.is_finite()
    {
        return None;
    }

    let predicted_reduction = accepted_residual_objective - predicted_residual_objective;
    let observed_reduction = accepted_residual_objective - observed_residual_objective;
    if predicted_reduction <= 0.0 || observed_reduction <= 0.0 {
        return Some(current_radius);
    }

    let prediction_accuracy_scale = (observed_reduction / predicted_reduction).sqrt();
    if !prediction_accuracy_scale.is_finite() || prediction_accuracy_scale <= 0.0 {
        return None;
    }

    let maximum_radius = maximum_normalized_trust_region_radius(lower_bounds, upper_bounds)?;

    Some((current_radius * prediction_accuracy_scale).min(maximum_radius))
}

pub(super) fn trust_region_reliability_scale(
    observed_delta_sum: f64,
    prediction_error_sum: f64,
) -> Option<f64> {
    if !observed_delta_sum.is_finite()
        || !prediction_error_sum.is_finite()
        || observed_delta_sum <= 0.0
        || prediction_error_sum < 0.0
    {
        return None;
    }

    let observed_delta_norm = observed_delta_sum.sqrt();
    let prediction_error_norm = prediction_error_sum.sqrt();
    let denominator = observed_delta_norm + prediction_error_norm;

    (denominator > 0.0).then_some(observed_delta_norm / denominator)
}

pub(super) fn trust_region_radius_is_at_tolerance(
    trust_region_radius: f64,
    accepted_vector: &[f64],
    lower_bounds: &[f64],
    upper_bounds: &[f64],
    absolute_tolerance: f64,
    relative_tolerance: f64,
) -> bool {
    if !trust_region_radius.is_finite() || trust_region_radius <= 0.0 {
        return true;
    }

    accepted_vector
        .iter()
        .zip(lower_bounds.iter().zip(upper_bounds.iter()))
        .filter(|(_, (lower_bound, upper_bound))| {
            let width = *upper_bound - *lower_bound;
            lower_bound.is_finite() && upper_bound.is_finite() && width > 0.0
        })
        .all(|(accepted_value, (lower_bound, upper_bound))| {
            let width = upper_bound - lower_bound;
            let raw_radius = trust_region_radius * width;
            raw_radius <= absolute_tolerance.max(relative_tolerance * accepted_value.abs().max(1.0))
        })
}

fn maximum_normalized_trust_region_radius(
    lower_bounds: &[f64],
    upper_bounds: &[f64],
) -> Option<f64> {
    let movable_count = lower_bounds
        .iter()
        .zip(upper_bounds.iter())
        .filter(|(lower_bound, upper_bound)| {
            let width = *upper_bound - *lower_bound;
            lower_bound.is_finite() && upper_bound.is_finite() && width > 0.0
        })
        .count();

    (movable_count > 0).then_some((movable_count as f64).sqrt())
}
