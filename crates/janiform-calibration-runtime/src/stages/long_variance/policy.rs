use janiform_foundations::time::{NANOS_PER_FIVE_MINUTES, NANOS_PER_HOUR};
use janiform_model_kernels::hawkes::{HawkesKernelParameters, stationary_intensity_multipliers};

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

pub(crate) fn long_variance_retained_stationary_level_acceptance_floor() -> f64 {
    // The accepted boundary is one representable value below the mathematical
    // floor so parameters projected onto the floor round-trip through
    // independent event-drift multiplication and division.
    long_variance_retained_stationary_level_floor().next_down()
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

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LongVarianceSideEventLoadings {
    pub(crate) buy_event_loading: f64,
    pub(crate) sell_event_loading: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LongVarianceStationaryEventRates {
    pub(crate) buy_event_rate_per_hour: f64,
    pub(crate) sell_event_rate_per_hour: f64,
}

pub(crate) fn long_variance_stationary_event_rates_for_hawkes_kernel(
    kernel_parameters: HawkesKernelParameters,
) -> Option<LongVarianceStationaryEventRates> {
    let stationary_multipliers =
        stationary_intensity_multipliers(kernel_parameters.alpha, kernel_parameters.beta)?;

    let buy_event_rate_per_hour =
        kernel_parameters.lambda_bar * stationary_multipliers.buy_intensity_multiplier;
    let sell_event_rate_per_hour =
        kernel_parameters.lambda_bar * stationary_multipliers.sell_intensity_multiplier;

    if !buy_event_rate_per_hour.is_finite()
        || buy_event_rate_per_hour < 0.0
        || !sell_event_rate_per_hour.is_finite()
        || sell_event_rate_per_hour < 0.0
    {
        return None;
    }

    Some(LongVarianceStationaryEventRates {
        buy_event_rate_per_hour,
        sell_event_rate_per_hour,
    })
}

pub(crate) fn project_long_variance_event_loadings_to_stationary_budget(
    event_loadings: LongVarianceSideEventLoadings,
    stationary_event_rates: LongVarianceStationaryEventRates,
    mean_reversion_rate: f64,
) -> LongVarianceSideEventLoadings {
    let event_loading_drift = event_loadings.buy_event_loading
        * stationary_event_rates.buy_event_rate_per_hour
        + event_loadings.sell_event_loading * stationary_event_rates.sell_event_rate_per_hour;

    let Some(event_feedback_drift_budget) =
        long_variance_event_feedback_drift_budget(mean_reversion_rate)
    else {
        return LongVarianceSideEventLoadings {
            buy_event_loading: 0.0,
            sell_event_loading: 0.0,
        };
    };

    if !event_loading_drift.is_finite() || event_loading_drift <= event_feedback_drift_budget {
        return event_loadings;
    }

    let scale = event_feedback_drift_budget / event_loading_drift;

    LongVarianceSideEventLoadings {
        buy_event_loading: event_loadings.buy_event_loading * scale,
        sell_event_loading: event_loadings.sell_event_loading * scale,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        long_variance_retained_stationary_level_acceptance_floor,
        long_variance_retained_stationary_level_floor,
    };

    #[test]
    fn retained_stationary_level_acceptance_floor_closes_boundary_round_trips() {
        let retained_floor = long_variance_retained_stationary_level_floor();
        let acceptance_floor = long_variance_retained_stationary_level_acceptance_floor();

        assert!(acceptance_floor < retained_floor);
        assert_eq!(acceptance_floor.next_up(), retained_floor);
    }
}
