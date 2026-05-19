use std::error::Error;
use std::fmt::{Display, Formatter};

use janiform_affine_oracle::sufficient_statistics::SufficientStatisticsExpectedIntervalStatistics;
use janiform_foundations::matrix2::{
    solve_two_by_two_linear_source_system, solve_two_by_two_linear_system,
};
use janiform_model_kernels::hawkes::{HawkesExcitationState, HawkesKernelParameters};

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ExpectedHawkesStateError {
    InvalidIntegratedTargetMoment {
        field: &'static str,
        value: f64,
    },
    SingularIntegratedSourceSystem {
        integrated_plus_from_plus_source: f64,
        integrated_plus_from_minus_source: f64,
        integrated_minus_from_plus_source: f64,
        integrated_minus_from_minus_source: f64,
    },
    InvalidExpectedState {
        field: &'static str,
        value: f64,
    },
}

impl Display for ExpectedHawkesStateError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidIntegratedTargetMoment { field, value } => write!(
                formatter,
                "expected Hawkes integrated target moment {field} must be finite and non-negative, found {value}"
            ),
            Self::SingularIntegratedSourceSystem {
                integrated_plus_from_plus_source,
                integrated_plus_from_minus_source,
                integrated_minus_from_plus_source,
                integrated_minus_from_minus_source,
            } => write!(
                formatter,
                "expected Hawkes state update could not invert integrated source system: [{integrated_plus_from_plus_source}, {integrated_plus_from_minus_source}; {integrated_minus_from_plus_source}, {integrated_minus_from_minus_source}]"
            ),
            Self::InvalidExpectedState { field, value } => write!(
                formatter,
                "expected Hawkes terminal state component {field} must be finite and non-negative, found {value}"
            ),
        }
    }
}

impl Error for ExpectedHawkesStateError {}

pub(crate) fn evolve_expected_hawkes_state_from_integrated_statistics(
    kernel_parameters: HawkesKernelParameters,
    seasonality_factor: f64,
    interval_delta: f64,
    state_before_update: HawkesExcitationState,
    expected_statistics: SufficientStatisticsExpectedIntervalStatistics,
) -> Result<HawkesExcitationState, ExpectedHawkesStateError> {
    let baseline_expected_business_time =
        kernel_parameters.lambda_bar * seasonality_factor * interval_delta;

    let integrated_target_plus =
        expected_statistics.expected_a_plus - baseline_expected_business_time;
    let integrated_target_minus =
        expected_statistics.expected_a_minus - baseline_expected_business_time;

    validate_integrated_target_moment("integrated_target_plus", integrated_target_plus)?;
    validate_integrated_target_moment("integrated_target_minus", integrated_target_minus)?;

    let matrix_a = -kernel_parameters.beta + kernel_parameters.alpha.plus_plus;
    let matrix_b = kernel_parameters.alpha.plus_minus;
    let matrix_c = kernel_parameters.alpha.minus_plus;
    let matrix_d = -kernel_parameters.beta + kernel_parameters.alpha.minus_minus;

    let target_plus_before = state_before_update.target_plus_sum();
    let target_minus_before = state_before_update.target_minus_sum();

    let base_solution = solve_two_by_two_linear_source_system(
        matrix_a,
        matrix_b,
        matrix_c,
        matrix_d,
        target_plus_before,
        target_minus_before,
        0.0,
        0.0,
        interval_delta,
    );

    let unit_plus_source_solution = solve_two_by_two_linear_source_system(
        matrix_a,
        matrix_b,
        matrix_c,
        matrix_d,
        0.0,
        0.0,
        1.0,
        0.0,
        interval_delta,
    );

    let unit_minus_source_solution = solve_two_by_two_linear_source_system(
        matrix_a,
        matrix_b,
        matrix_c,
        matrix_d,
        0.0,
        0.0,
        0.0,
        1.0,
        interval_delta,
    );

    let source_solution = solve_two_by_two_linear_system(
        unit_plus_source_solution.integrated_first_component,
        unit_minus_source_solution.integrated_first_component,
        unit_plus_source_solution.integrated_second_component,
        unit_minus_source_solution.integrated_second_component,
        [
            integrated_target_plus - base_solution.integrated_first_component,
            integrated_target_minus - base_solution.integrated_second_component,
        ],
    )
    .ok_or(ExpectedHawkesStateError::SingularIntegratedSourceSystem {
        integrated_plus_from_plus_source: unit_plus_source_solution.integrated_first_component,
        integrated_plus_from_minus_source: unit_minus_source_solution.integrated_first_component,
        integrated_minus_from_plus_source: unit_plus_source_solution.integrated_second_component,
        integrated_minus_from_minus_source: unit_minus_source_solution.integrated_second_component,
    })?;

    let target_plus_after = base_solution.first_component_after_interval
        + source_solution[0] * unit_plus_source_solution.first_component_after_interval
        + source_solution[1] * unit_minus_source_solution.first_component_after_interval;
    let target_minus_after = base_solution.second_component_after_interval
        + source_solution[0] * unit_plus_source_solution.second_component_after_interval
        + source_solution[1] * unit_minus_source_solution.second_component_after_interval;

    validate_expected_state_component("target_plus_after", target_plus_after)?;
    validate_expected_state_component("target_minus_after", target_minus_after)?;

    Ok(HawkesExcitationState::from_target_sums(
        target_plus_after,
        target_minus_after,
    ))
}

fn validate_integrated_target_moment(
    field: &'static str,
    value: f64,
) -> Result<(), ExpectedHawkesStateError> {
    if value.is_finite() && value >= 0.0 {
        return Ok(());
    }

    Err(ExpectedHawkesStateError::InvalidIntegratedTargetMoment { field, value })
}

fn validate_expected_state_component(
    field: &'static str,
    value: f64,
) -> Result<(), ExpectedHawkesStateError> {
    if value.is_finite() && value >= 0.0 {
        return Ok(());
    }

    Err(ExpectedHawkesStateError::InvalidExpectedState { field, value })
}
