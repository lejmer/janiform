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
// sell-event feedback drift, return-energy feedback drift, and the retained
// mean-reversion level.
const LONG_VARIANCE_STATIONARY_COMPONENT_COUNT: usize = 4;

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
    1.0 / long_variance_stationary_component_count_as_f64()
}

pub(crate) fn long_variance_retained_stationary_level_acceptance_floor() -> f64 {
    let mut acceptance_floor = long_variance_retained_stationary_level_floor();

    // Boundary parameters are projected through the stationary mean
    // components and then decoded through independent event-drift products,
    // summation, division by mean reversion, and subtraction from unit mean.
    // Accept one adjacent representable value for each stationary component.
    for _ in 0..LONG_VARIANCE_STATIONARY_COMPONENT_COUNT {
        acceptance_floor = acceptance_floor.next_down();
    }

    acceptance_floor
}

pub(crate) fn long_variance_feedback_drift_budget(mean_reversion_rate: f64) -> Option<f64> {
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

    let budget = mean_reversion_rate / long_variance_stationary_component_count_as_f64();

    (budget.is_finite() && budget >= 0.0).then_some(budget)
}

fn long_variance_stationary_component_count_as_f64() -> f64 {
    LONG_VARIANCE_STATIONARY_COMPONENT_COUNT as f64
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LongVarianceSideEventLoadings {
    pub(crate) buy_event_loading: f64,
    pub(crate) sell_event_loading: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LongVarianceFeedbackLoadings {
    pub(crate) stationary_return_energy_rate_per_hour: f64,
    pub(crate) return_energy_loading: f64,
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

pub(crate) fn project_long_variance_feedback_loadings_to_stationary_budget(
    feedback_loadings: LongVarianceFeedbackLoadings,
    stationary_event_rates: LongVarianceStationaryEventRates,
    mean_reversion_rate: f64,
) -> LongVarianceFeedbackLoadings {
    let feedback_loading_drift = feedback_loadings.buy_event_loading
        * stationary_event_rates.buy_event_rate_per_hour
        + feedback_loadings.sell_event_loading * stationary_event_rates.sell_event_rate_per_hour
        + feedback_loadings.return_energy_loading
            * feedback_loadings.stationary_return_energy_rate_per_hour;

    let Some(feedback_drift_budget) = long_variance_feedback_drift_budget(mean_reversion_rate)
    else {
        return LongVarianceFeedbackLoadings {
            stationary_return_energy_rate_per_hour: feedback_loadings
                .stationary_return_energy_rate_per_hour,
            return_energy_loading: 0.0,
            buy_event_loading: 0.0,
            sell_event_loading: 0.0,
        };
    };

    if !feedback_loading_drift.is_finite() || feedback_loading_drift <= feedback_drift_budget {
        return feedback_loadings;
    }

    let scale = feedback_drift_budget / feedback_loading_drift;

    LongVarianceFeedbackLoadings {
        stationary_return_energy_rate_per_hour: feedback_loadings
            .stationary_return_energy_rate_per_hour,
        return_energy_loading: feedback_loadings.return_energy_loading * scale,
        buy_event_loading: feedback_loadings.buy_event_loading * scale,
        sell_event_loading: feedback_loadings.sell_event_loading * scale,
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

        let mut round_trip_boundary = retained_floor;

        for _ in 0..super::LONG_VARIANCE_STATIONARY_COMPONENT_COUNT {
            round_trip_boundary = round_trip_boundary.next_down();
        }

        assert!(acceptance_floor < retained_floor);
        assert_eq!(acceptance_floor, round_trip_boundary);

        let mut recovered_floor = acceptance_floor;

        for _ in 0..super::LONG_VARIANCE_STATIONARY_COMPONENT_COUNT {
            recovered_floor = recovered_floor.next_up();
        }

        assert_eq!(recovered_floor, retained_floor);
    }
}
