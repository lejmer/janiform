use janiform_model_kernels::hawkes::{HawkesKernelParameters, HawkesTransitionError};

use crate::error::SeededTickSimulationRunError;

pub(crate) fn compute_max_effective_endogenous_branching_ratio(
    kernel_parameters: HawkesKernelParameters,
    seasonality_factors: &[f64],
) -> Option<(f64, f64, f64)> {
    let max_seasonality_factor = seasonality_factors
        .iter()
        .copied()
        .reduce(f64::max)?
        .max(0.0);

    let scaled_plus_plus = kernel_parameters.alpha.plus_plus / kernel_parameters.beta;
    let scaled_plus_minus = kernel_parameters.alpha.plus_minus / kernel_parameters.beta;
    let scaled_minus_plus = kernel_parameters.alpha.minus_plus / kernel_parameters.beta;
    let scaled_minus_minus = kernel_parameters.alpha.minus_minus / kernel_parameters.beta;
    let trace = scaled_plus_plus + scaled_minus_minus;

    let discriminant = ((scaled_plus_plus - scaled_minus_minus).powi(2)
        + 4.0 * scaled_plus_minus * scaled_minus_plus)
        .sqrt();

    let spectral_radius_alpha_over_beta = 0.5 * (trace + discriminant);
    let effective_branching_ratio = max_seasonality_factor * spectral_radius_alpha_over_beta;

    Some((
        max_seasonality_factor,
        spectral_radius_alpha_over_beta,
        effective_branching_ratio,
    ))
}

pub(crate) fn map_transition_error_to_seeded_simulation_run_error(
    source: HawkesTransitionError,
) -> SeededTickSimulationRunError {
    match source {
        HawkesTransitionError::InvalidNewsSeverityMark { source } => {
            SeededTickSimulationRunError::InvalidGeneratedNewsActivityCouplingMark {
                value: source.value,
            }
        }
        _ => SeededTickSimulationRunError::InvalidTransitionStep { source },
    }
}

pub(crate) fn validate_hourly_factor_field_length(
    field_name: &'static str,
    expected_interval_count: usize,
    observed_interval_count: usize,
) -> Result<(), SeededTickSimulationRunError> {
    if observed_interval_count != expected_interval_count {
        return Err(
            SeededTickSimulationRunError::HourlyFactorFieldLengthMismatch {
                field_name,
                expected_interval_count,
                observed_interval_count,
            },
        );
    }

    Ok(())
}

pub(crate) fn validate_hourly_factor_interval_coverage(
    interval_start_utc_nanos: &[i64],
    interval_end_utc_nanos: &[i64],
    simulated_interval_count: usize,
) -> Result<(), SeededTickSimulationRunError> {
    for interval_index in 0..simulated_interval_count {
        let current_interval_start_utc_nanos = interval_start_utc_nanos[interval_index];
        let current_interval_end_utc_nanos = interval_end_utc_nanos[interval_index];

        if current_interval_end_utc_nanos <= current_interval_start_utc_nanos {
            return Err(
                SeededTickSimulationRunError::InvalidHourlyFactorIntervalBounds {
                    interval_index,
                    interval_start_utc_nanos: current_interval_start_utc_nanos,
                    interval_end_utc_nanos: current_interval_end_utc_nanos,
                },
            );
        }

        if interval_index == 0 {
            continue;
        }

        let previous_interval_index = interval_index - 1;
        let previous_interval_end_utc_nanos = interval_end_utc_nanos[previous_interval_index];
        if current_interval_start_utc_nanos != previous_interval_end_utc_nanos {
            return Err(
                SeededTickSimulationRunError::NonConsecutiveHourlyFactorCoverage {
                    previous_interval_index,
                    previous_interval_end_utc_nanos,
                    current_interval_index: interval_index,
                    current_interval_start_utc_nanos,
                },
            );
        }
    }

    Ok(())
}
