pub(super) fn stable_sum_f64(values: &[f64]) -> f64 {
    let mut running_sum = 0.0;
    let mut compensation = 0.0;

    for value in values.iter().copied() {
        let tentative_sum = running_sum + value;

        if running_sum.abs() >= value.abs() {
            compensation += (running_sum - tentative_sum) + value;
        } else {
            compensation += (value - tentative_sum) + running_sum;
        }

        running_sum = tentative_sum;
    }

    running_sum + compensation
}

pub(super) fn stable_mean_f64(values: &[f64]) -> f64 {
    if values.is_empty() {
        return f64::NAN;
    }

    stable_sum_f64(values) / values.len() as f64
}

pub(super) fn derived_mean_anchor_tolerance_f64(
    fit_window_event_count: usize,
    target_mean_factor: f64,
    normalization_constant: f64,
    post_normalization_mean_factor: f64,
) -> f64 {
    if fit_window_event_count == 0
        || !target_mean_factor.is_finite()
        || !normalization_constant.is_finite()
        || normalization_constant <= 0.0
        || !post_normalization_mean_factor.is_finite()
    {
        return f64::NAN;
    }

    let stable_mean_relative_error =
        relative_rounding_error_bound(stable_mean_rounding_step_count(fit_window_event_count));

    let normalized_factor_relative_error = relative_rounding_error_bound(fit_window_event_count);

    let normalization_constant_relative_resolution =
        local_relative_machine_resolution(normalization_constant);

    let combined_relative_error = combine_relative_error_bounds(&[
        stable_mean_relative_error,
        normalization_constant_relative_resolution,
        normalized_factor_relative_error,
        stable_mean_relative_error,
    ]);

    let scale_reference = target_mean_factor
        .abs()
        .max(post_normalization_mean_factor.abs());

    scale_reference * combined_relative_error
}

fn stable_mean_rounding_step_count(fit_window_event_count: usize) -> usize {
    // `stable_mean_f64` executes one compensated summation and one final division by the
    // support count. The Neumaier update introduces two rounded additions per input term.
    fit_window_event_count.saturating_mul(2).saturating_add(1)
}

fn relative_rounding_error_bound(rounding_step_count: usize) -> f64 {
    if rounding_step_count == 0 {
        return 0.0;
    }

    let unit_roundoff = f64::EPSILON / 2.0;
    let accumulated_roundoff = rounding_step_count as f64 * unit_roundoff;

    if accumulated_roundoff >= 1.0 {
        return f64::INFINITY;
    }

    accumulated_roundoff / (1.0 - accumulated_roundoff)
}

fn combine_relative_error_bounds(relative_error_bounds: &[f64]) -> f64 {
    let mut compounded_error_budget = 1.0;

    for relative_error_bound in relative_error_bounds.iter().copied() {
        if !relative_error_bound.is_finite() || relative_error_bound < 0.0 {
            return f64::INFINITY;
        }

        compounded_error_budget *= 1.0 + relative_error_bound;

        if !compounded_error_budget.is_finite() {
            return f64::INFINITY;
        }
    }

    compounded_error_budget - 1.0
}

fn local_relative_machine_resolution(value: f64) -> f64 {
    let magnitude = value.abs();
    if magnitude == 0.0 {
        return f64::INFINITY;
    }

    local_machine_step_width(magnitude) / magnitude
}

fn local_machine_step_width(value: f64) -> f64 {
    if !value.is_finite() || value <= 0.0 {
        return f64::INFINITY;
    }

    let value_bits = value.to_bits();
    let maximum_finite_bits = f64::INFINITY.to_bits() - 1;

    if value_bits >= maximum_finite_bits {
        return value - f64::from_bits(value_bits - 1);
    }

    f64::from_bits(value_bits + 1) - value
}
