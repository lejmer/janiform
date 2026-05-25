use std::error::Error;
use std::fmt::{Display, Formatter};

use janiform_affine_oracle::sufficient_statistics::SufficientStatisticsExpectedIntervalStatistics;
use janiform_foundations::linear_algebra::solve_two_by_two_linear_source_system;
use janiform_foundations::numeric::{validate_non_negative_number, validate_positive_number};
use janiform_model_kernels::hawkes::{HawkesExcitationState, HawkesKernelParameters};
use nalgebra::{Matrix2, Vector2};

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ExpectedHawkesStateError {
    InvalidExpectedCountMoment { field: &'static str, value: f64 },
    InvalidExpectedNewsLoadingRate { value: f64 },
    InvalidIntervalDelta { value: f64 },
    InvalidSourceRate { field: &'static str, value: f64 },
    InvalidExpectedState { field: &'static str, value: f64 },
}

impl Display for ExpectedHawkesStateError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidExpectedCountMoment { field, value } => write!(
                formatter,
                "expected Hawkes count moment {field} must be finite and non-negative, found {value}"
            ),
            Self::InvalidExpectedNewsLoadingRate { value } => write!(
                formatter,
                "expected Hawkes news loading rate must be finite and non-negative, found {value}"
            ),
            Self::InvalidIntervalDelta { value } => write!(
                formatter,
                "expected Hawkes state update interval delta must be finite and strictly positive, found {value}"
            ),
            Self::InvalidSourceRate { field, value } => write!(
                formatter,
                "expected Hawkes source rate {field} must be finite and non-negative, found {value}"
            ),
            Self::InvalidExpectedState { field, value } => write!(
                formatter,
                "expected Hawkes terminal state component {field} must be finite and non-negative, found {value}"
            ),
        }
    }
}

impl Error for ExpectedHawkesStateError {}

pub(crate) fn evolve_expected_hawkes_state_from_expected_counts(
    kernel_parameters: HawkesKernelParameters,
    expected_news_loading_rate: f64,
    interval_delta: f64,
    state_before_update: HawkesExcitationState,
    expected_statistics: SufficientStatisticsExpectedIntervalStatistics,
) -> Result<HawkesExcitationState, ExpectedHawkesStateError> {
    validate_expected_count_moment("expected_n_plus", expected_statistics.expected_n_plus)?;
    validate_expected_count_moment("expected_n_minus", expected_statistics.expected_n_minus)?;
    validate_expected_news_loading_rate(expected_news_loading_rate)?;
    validate_interval_delta(interval_delta)?;

    let expected_event_rate_plus = expected_statistics.expected_n_plus / interval_delta;
    let expected_event_rate_minus = expected_statistics.expected_n_minus / interval_delta;

    validate_source_rate("expected_event_rate_plus", expected_event_rate_plus)?;
    validate_source_rate("expected_event_rate_minus", expected_event_rate_minus)?;

    let source_plus_plus = kernel_parameters.alpha.plus_plus * expected_event_rate_plus
        + kernel_parameters.rho.plus_plus * expected_news_loading_rate;
    let source_plus_minus = kernel_parameters.alpha.plus_minus * expected_event_rate_minus
        + kernel_parameters.rho.plus_minus * expected_news_loading_rate;
    let source_minus_plus = kernel_parameters.alpha.minus_plus * expected_event_rate_plus
        + kernel_parameters.rho.minus_plus * expected_news_loading_rate;
    let source_minus_minus = kernel_parameters.alpha.minus_minus * expected_event_rate_minus
        + kernel_parameters.rho.minus_minus * expected_news_loading_rate;

    validate_source_rate("source_plus_plus", source_plus_plus)?;
    validate_source_rate("source_plus_minus", source_plus_minus)?;
    validate_source_rate("source_minus_plus", source_minus_plus)?;
    validate_source_rate("source_minus_minus", source_minus_minus)?;

    let system_matrix = Matrix2::new(-kernel_parameters.beta, 0.0, 0.0, -kernel_parameters.beta);

    let (plus_target_components_after, _) = solve_two_by_two_linear_source_system(
        system_matrix,
        Vector2::new(
            state_before_update.y_plus_plus,
            state_before_update.y_plus_minus,
        ),
        Vector2::new(source_plus_plus, source_plus_minus),
        interval_delta,
    );

    let (minus_target_components_after, _) = solve_two_by_two_linear_source_system(
        system_matrix,
        Vector2::new(
            state_before_update.y_minus_plus,
            state_before_update.y_minus_minus,
        ),
        Vector2::new(source_minus_plus, source_minus_minus),
        interval_delta,
    );

    validate_expected_state_component("y_plus_plus_after", plus_target_components_after[0])?;
    validate_expected_state_component("y_plus_minus_after", plus_target_components_after[1])?;
    validate_expected_state_component("y_minus_plus_after", minus_target_components_after[0])?;
    validate_expected_state_component("y_minus_minus_after", minus_target_components_after[1])?;

    Ok(HawkesExcitationState {
        y_plus_plus: plus_target_components_after[0],
        y_plus_minus: plus_target_components_after[1],
        y_minus_plus: minus_target_components_after[0],
        y_minus_minus: minus_target_components_after[1],
    })
}

fn validate_expected_count_moment(
    field: &'static str,
    value: f64,
) -> Result<(), ExpectedHawkesStateError> {
    validate_non_negative_number(value, field).map_err(|source| {
        ExpectedHawkesStateError::InvalidExpectedCountMoment {
            field: source.field,
            value: source.value,
        }
    })
}

fn validate_expected_news_loading_rate(value: f64) -> Result<(), ExpectedHawkesStateError> {
    validate_non_negative_number(value, "expected_news_loading_rate").map_err(|source| {
        ExpectedHawkesStateError::InvalidExpectedNewsLoadingRate {
            value: source.value,
        }
    })
}

fn validate_interval_delta(value: f64) -> Result<(), ExpectedHawkesStateError> {
    validate_positive_number(value, "interval_delta").map_err(|source| {
        ExpectedHawkesStateError::InvalidIntervalDelta {
            value: source.value,
        }
    })
}

fn validate_source_rate(field: &'static str, value: f64) -> Result<(), ExpectedHawkesStateError> {
    validate_non_negative_number(value, field).map_err(|source| {
        ExpectedHawkesStateError::InvalidSourceRate {
            field: source.field,
            value: source.value,
        }
    })
}

fn validate_expected_state_component(
    field: &'static str,
    value: f64,
) -> Result<(), ExpectedHawkesStateError> {
    validate_non_negative_number(value, field).map_err(|source| {
        ExpectedHawkesStateError::InvalidExpectedState {
            field: source.field,
            value: source.value,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use janiform_model_kernels::hawkes::HawkesKernelLoadings;

    #[test]
    fn expected_count_update_accepts_activity_below_physical_baseline() {
        let kernel_parameters = test_kernel_parameters();

        let state_before_update = HawkesExcitationState {
            y_plus_plus: 2.0,
            y_plus_minus: 3.0,
            y_minus_plus: 5.0,
            y_minus_minus: 7.0,
        };

        let expected_statistics =
            SufficientStatisticsExpectedIntervalStatistics::try_new(1.0, 1.5, 5.0, 7.0)
                .expect("expected statistics should construct");

        let observed = evolve_expected_hawkes_state_from_expected_counts(
            kernel_parameters,
            0.4,
            2.0,
            state_before_update,
            expected_statistics,
        )
        .expect("count-driven expected state update should succeed");

        let decay = (-kernel_parameters.beta * 2.0).exp();
        let source_weight = (1.0 - decay) / kernel_parameters.beta;

        let expected = HawkesExcitationState {
            y_plus_plus: decay.mul_add(
                2.0,
                source_weight
                    * (kernel_parameters.alpha.plus_plus * 2.5
                        + kernel_parameters.rho.plus_plus * 0.4),
            ),
            y_plus_minus: decay.mul_add(
                3.0,
                source_weight
                    * (kernel_parameters.alpha.plus_minus * 3.5
                        + kernel_parameters.rho.plus_minus * 0.4),
            ),
            y_minus_plus: decay.mul_add(
                5.0,
                source_weight
                    * (kernel_parameters.alpha.minus_plus * 2.5
                        + kernel_parameters.rho.minus_plus * 0.4),
            ),
            y_minus_minus: decay.mul_add(
                7.0,
                source_weight
                    * (kernel_parameters.alpha.minus_minus * 3.5
                        + kernel_parameters.rho.minus_minus * 0.4),
            ),
        };

        assert_state_close(observed, expected);
    }

    #[test]
    fn expected_count_update_rejects_non_finite_count_moment() {
        let expected_statistics = SufficientStatisticsExpectedIntervalStatistics {
            expected_a_plus: 1.0,
            expected_a_minus: 1.0,
            expected_n_plus: f64::NAN,
            expected_n_minus: 1.0,
        };

        let observed = evolve_expected_hawkes_state_from_expected_counts(
            test_kernel_parameters(),
            0.0,
            1.0,
            HawkesExcitationState::from_target_sums(0.0, 0.0),
            expected_statistics,
        );

        assert!(matches!(
            observed,
            Err(ExpectedHawkesStateError::InvalidExpectedCountMoment {
                field: "expected_n_plus",
                value
            }) if value.is_nan()
        ));
    }

    fn test_kernel_parameters() -> HawkesKernelParameters {
        HawkesKernelParameters {
            lambda_bar: 100.0,
            beta: 4.0,
            alpha: HawkesKernelLoadings {
                plus_plus: 0.7,
                plus_minus: 0.2,
                minus_plus: 0.3,
                minus_minus: 0.8,
            },
            rho: HawkesKernelLoadings {
                plus_plus: 0.11,
                plus_minus: 0.13,
                minus_plus: 0.17,
                minus_minus: 0.19,
            },
        }
    }

    fn assert_state_close(observed: HawkesExcitationState, expected: HawkesExcitationState) {
        let tolerance = f64::EPSILON.sqrt();

        for (observed_component, expected_component) in observed
            .as_model_ordered_array()
            .into_iter()
            .zip(expected.as_model_ordered_array())
        {
            assert!(
                (observed_component - expected_component).abs()
                    <= tolerance * expected_component.abs().max(1.0),
                "observed={observed_component} expected={expected_component}"
            );
        }
    }
}
