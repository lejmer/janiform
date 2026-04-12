use std::f64::consts::PI;

pub(crate) fn estimate_asymptotic_tail_integral_error(
    penultimate_frequency: f64,
    penultimate_abs_integrand: f64,
    terminal_frequency: f64,
    terminal_abs_integrand: f64,
) -> f64 {
    if penultimate_abs_integrand <= 0.0
        || terminal_abs_integrand <= 0.0
        || penultimate_frequency <= 0.0
        || terminal_frequency <= penultimate_frequency
    {
        return f64::INFINITY;
    }

    let logarithmic_ratio = (penultimate_abs_integrand / terminal_abs_integrand).ln();

    let exponential_tail_estimate = {
        let local_decay_rate = logarithmic_ratio / (terminal_frequency - penultimate_frequency);
        if local_decay_rate.is_finite() && local_decay_rate > 0.0 {
            terminal_abs_integrand / local_decay_rate
        } else {
            f64::INFINITY
        }
    };

    let algebraic_tail_estimate = {
        let algebraic_order = logarithmic_ratio / (terminal_frequency / penultimate_frequency).ln();
        if algebraic_order.is_finite() && algebraic_order > 1.0 {
            terminal_abs_integrand * terminal_frequency / (algebraic_order - 1.0)
        } else {
            f64::INFINITY
        }
    };

    exponential_tail_estimate.min(algebraic_tail_estimate)
}

pub(crate) fn initial_midpoint_upper_bound_from_phase_scale(phase_scale: f64) -> f64 {
    if phase_scale.is_finite() && phase_scale > 0.0 {
        PI / phase_scale
    } else {
        PI
    }
}

pub(crate) fn next_power_of_two_at_least(value: usize) -> Option<usize> {
    match value {
        0 => Some(1),
        value if value.is_power_of_two() => Some(value),
        _ => value.checked_next_power_of_two(),
    }
}
