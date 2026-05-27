use janiform_foundations::time::{NANOS_PER_FIVE_MINUTES, NANOS_PER_HOUR};

// Unit-stationary CIR variance is sigma_z^2 / (2 kappa_z). The standard
// finite-variance boundary uses variance proxy one.
pub(crate) const LONG_VARIANCE_STANDARD_CIR_VARIANCE_PROXY_BOUNDARY: f64 = 1.0;

// The long-variance factor is identified from horizon diagnostics whose
// shortest member is five minutes. Faster variance bursts belong to Hawkes
// excitation and NIG return-clock exposure, so calibration keeps this factor's
// half-life no shorter than that shortest horizon.
const LONG_VARIANCE_SHORTEST_IDENTIFICATION_HORIZON_HOURS: f64 =
    NANOS_PER_FIVE_MINUTES as f64 / NANOS_PER_HOUR as f64;

// Stationary unit mean is decomposed into buy-event feedback drift,
// sell-event feedback drift, and the retained mean-reversion level.
const LONG_VARIANCE_STATIONARY_COMPONENT_COUNT: f64 = 3.0;

pub(crate) fn long_variance_mean_reversion_rate_calibration_upper_bound() -> f64 {
    std::f64::consts::LN_2 / LONG_VARIANCE_SHORTEST_IDENTIFICATION_HORIZON_HOURS
}

pub(crate) fn long_variance_vol_of_vol_upper_bound_for_mean_reversion_rate(
    mean_reversion_rate: f64,
) -> Option<f64> {
    if !mean_reversion_rate.is_finite() || mean_reversion_rate <= 0.0 {
        return None;
    }

    let upper_bound =
        (2.0 * mean_reversion_rate * LONG_VARIANCE_STANDARD_CIR_VARIANCE_PROXY_BOUNDARY).sqrt();

    (upper_bound.is_finite() && upper_bound >= 0.0).then_some(upper_bound)
}

pub(crate) fn long_variance_retained_stationary_level_floor() -> f64 {
    1.0 / LONG_VARIANCE_STATIONARY_COMPONENT_COUNT
}

pub(crate) fn long_variance_event_feedback_drift_budget(mean_reversion_rate: f64) -> Option<f64> {
    if !mean_reversion_rate.is_finite() || mean_reversion_rate <= 0.0 {
        return None;
    }

    let budget = mean_reversion_rate * (1.0 - long_variance_retained_stationary_level_floor());

    (budget.is_finite() && budget >= 0.0).then_some(budget)
}

pub(crate) fn long_variance_side_event_feedback_drift_budget(
    mean_reversion_rate: f64,
) -> Option<f64> {
    if !mean_reversion_rate.is_finite() || mean_reversion_rate <= 0.0 {
        return None;
    }

    let budget = mean_reversion_rate / LONG_VARIANCE_STATIONARY_COMPONENT_COUNT;

    (budget.is_finite() && budget >= 0.0).then_some(budget)
}
