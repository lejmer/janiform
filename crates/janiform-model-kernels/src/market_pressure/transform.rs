use janiform_foundations::complex_math::{ComplexScalar, complex_log_principal};
use janiform_foundations::numeric::{validate_finite_number, validate_non_negative_number};
use nalgebra::{Complex, DMatrix, DVector};

use super::contracts::{MarketPressureParameters, MarketPressureState};
use super::errors::{
    MarketPressureCtmcTransitionError, MarketPressureStateEvolutionError,
    MarketPressureStateValidationError,
};

/// Inputs for one finite-state pressure transform interval.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MarketPressureTransformValuesInput<'a> {
    pub parameters: &'a MarketPressureParameters,
    pub interval_delta: f64,
    pub signed_balance_exposure_scale: f64,
    pub return_loading: ComplexScalar,
    pub pressure_signed_imbalance_exposure_risk_price: f64,
    pub terminal_values: &'a [ComplexScalar],
}

/// Inputs for one finite-state pressure transform interval and one tangent.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MarketPressureTransformValuesAndDerivativeInput<'a> {
    pub parameters: &'a MarketPressureParameters,
    pub interval_delta: f64,
    pub signed_balance_exposure_scale: f64,
    pub return_loading: ComplexScalar,
    pub return_loading_derivative: ComplexScalar,
    pub pressure_signed_imbalance_exposure_risk_price: f64,
    pub pressure_signed_imbalance_exposure_risk_price_derivative: f64,
    pub terminal_values: &'a [ComplexScalar],
    pub terminal_value_derivatives: &'a [ComplexScalar],
}

/// Inputs for evaluating a pressure transform factor at a boundary distribution.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MarketPressureTransformLogFactorInput<'a> {
    pub values_input: MarketPressureTransformValuesInput<'a>,
    pub initial_state: &'a MarketPressureState,
}

/// Inputs for evaluating a pressure transform log factor and one tangent.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MarketPressureTransformLogFactorAndDerivativeInput<'a> {
    pub values_input: MarketPressureTransformValuesAndDerivativeInput<'a>,
    pub initial_state: &'a MarketPressureState,
}

/// Propagate terminal pressure transform values over one CTMC interval.
pub fn market_pressure_transform_values(
    input: MarketPressureTransformValuesInput<'_>,
) -> Result<Vec<ComplexScalar>, MarketPressureStateEvolutionError> {
    validate_transform_values_input(&input)?;

    let tilted_generator = build_backward_tilted_generator(
        input.parameters,
        input.return_loading,
        input.pressure_signed_imbalance_exposure_risk_price,
        input.signed_balance_exposure_scale,
    )?;

    let terminal_vector = complex_vector_from_values(input.terminal_values)?;

    let propagated =
        ((tilted_generator * Complex::new(input.interval_delta, 0.0)).exp()) * terminal_vector;

    propagated
        .iter()
        .copied()
        .map(complex_scalar_from_nalgebra)
        .map(validate_transform_value)
        .collect()
}

/// Propagate terminal pressure transform values and one tangent over one CTMC interval.
pub fn market_pressure_transform_values_and_derivative(
    input: MarketPressureTransformValuesAndDerivativeInput<'_>,
) -> Result<(Vec<ComplexScalar>, Vec<ComplexScalar>), MarketPressureStateEvolutionError> {
    validate_transform_values_and_derivative_input(&input)?;

    let tilted_generator = build_backward_tilted_generator(
        input.parameters,
        input.return_loading,
        input.pressure_signed_imbalance_exposure_risk_price,
        input.signed_balance_exposure_scale,
    )?;

    let state_count = input.parameters.state_count();

    let mut augmented_generator = DMatrix::<Complex<f64>>::zeros(state_count * 2, state_count * 2);

    for row_position in 0..state_count {
        for column_position in 0..state_count {
            let value = tilted_generator[(row_position, column_position)];

            augmented_generator[(row_position, column_position)] = value;
            augmented_generator[(row_position + state_count, column_position + state_count)] =
                value;
        }

        let side_imbalance = input.parameters.side_imbalance_levels[row_position];

        let diagonal_derivative = input
            .return_loading_derivative
            .scale(input.parameters.anchor_drift_per_signed_balance)
            .subtract(ComplexScalar::new(
                input.pressure_signed_imbalance_exposure_risk_price_derivative,
                0.0,
            ))
            .scale(input.signed_balance_exposure_scale * side_imbalance);

        augmented_generator[(row_position + state_count, row_position)] =
            complex_to_nalgebra(diagonal_derivative);
    }

    let mut terminal_vector = DVector::<Complex<f64>>::zeros(state_count * 2);

    for (state_position, value) in input.terminal_values.iter().copied().enumerate() {
        terminal_vector[state_position] = complex_to_nalgebra(value);
    }

    for (state_position, value) in input.terminal_value_derivatives.iter().copied().enumerate() {
        terminal_vector[state_position + state_count] = complex_to_nalgebra(value);
    }

    let propagated =
        ((augmented_generator * Complex::new(input.interval_delta, 0.0)).exp()) * terminal_vector;

    let values = propagated
        .rows(0, state_count)
        .iter()
        .copied()
        .map(complex_scalar_from_nalgebra)
        .map(validate_transform_value)
        .collect::<Result<Vec<_>, _>>()?;

    let derivatives = propagated
        .rows(state_count, state_count)
        .iter()
        .copied()
        .map(complex_scalar_from_nalgebra)
        .map(validate_transform_value)
        .collect::<Result<Vec<_>, _>>()?;

    Ok((values, derivatives))
}

/// Return the principal log transform factor for a boundary pressure distribution.
pub fn market_pressure_transform_log_factor(
    input: MarketPressureTransformLogFactorInput<'_>,
) -> Result<ComplexScalar, MarketPressureStateEvolutionError> {
    input
        .initial_state
        .validate_for_parameters(input.values_input.parameters)
        .map_err(MarketPressureStateEvolutionError::InvalidState)?;

    let values = market_pressure_transform_values(input.values_input)?;

    let probabilities = input
        .initial_state
        .normalized_probabilities()
        .map_err(MarketPressureStateEvolutionError::InvalidState)?;

    let factor = complex_weighted_sum(probabilities.as_slice(), values.as_slice())?;
    if factor.norm() == 0.0 {
        return Err(MarketPressureStateEvolutionError::InvalidTransformValue {
            field: "market_pressure_transform_factor",
            value: factor,
        });
    }

    validate_transform_value(complex_log_principal(factor))
}

/// Return the principal log transform factor and its tangent at a boundary distribution.
pub fn market_pressure_transform_log_factor_and_derivative(
    input: MarketPressureTransformLogFactorAndDerivativeInput<'_>,
) -> Result<(ComplexScalar, ComplexScalar), MarketPressureStateEvolutionError> {
    input
        .initial_state
        .validate_for_parameters(input.values_input.parameters)
        .map_err(MarketPressureStateEvolutionError::InvalidState)?;

    let (values, derivatives) =
        market_pressure_transform_values_and_derivative(input.values_input)?;

    let probabilities = input
        .initial_state
        .normalized_probabilities()
        .map_err(MarketPressureStateEvolutionError::InvalidState)?;

    let factor = complex_weighted_sum(probabilities.as_slice(), values.as_slice())?;
    if factor.norm() == 0.0 {
        return Err(MarketPressureStateEvolutionError::InvalidTransformValue {
            field: "market_pressure_transform_factor",
            value: factor,
        });
    }

    let derivative_factor = complex_weighted_sum(probabilities.as_slice(), derivatives.as_slice())?;
    let log_factor = validate_transform_value(complex_log_principal(factor))?;

    let log_factor_derivative = derivative_factor.divide(factor).ok_or(
        MarketPressureStateEvolutionError::InvalidTransformValue {
            field: "market_pressure_transform_factor_derivative",
            value: derivative_factor,
        },
    )?;

    Ok((log_factor, validate_transform_value(log_factor_derivative)?))
}

fn validate_transform_values_input(
    input: &MarketPressureTransformValuesInput<'_>,
) -> Result<(), MarketPressureStateEvolutionError> {
    validate_common_transform_input(
        input.parameters,
        input.interval_delta,
        input.signed_balance_exposure_scale,
        input.return_loading,
        input.pressure_signed_imbalance_exposure_risk_price,
    )?;

    validate_terminal_values(input.parameters, input.terminal_values)
}

fn validate_transform_values_and_derivative_input(
    input: &MarketPressureTransformValuesAndDerivativeInput<'_>,
) -> Result<(), MarketPressureStateEvolutionError> {
    validate_common_transform_input(
        input.parameters,
        input.interval_delta,
        input.signed_balance_exposure_scale,
        input.return_loading,
        input.pressure_signed_imbalance_exposure_risk_price,
    )?;

    if !input.return_loading_derivative.is_finite() {
        return Err(MarketPressureStateEvolutionError::InvalidTransformValue {
            field: "return_loading_derivative",
            value: input.return_loading_derivative,
        });
    }

    validate_finite_number(
        input.pressure_signed_imbalance_exposure_risk_price_derivative,
        "pressure_signed_imbalance_exposure_risk_price_derivative",
    )
    .map_err(
        |source| MarketPressureStateEvolutionError::InvalidTransformValue {
            field: "pressure_signed_imbalance_exposure_risk_price_derivative",
            value: ComplexScalar::new(source.value, 0.0),
        },
    )?;

    validate_terminal_values(input.parameters, input.terminal_values)?;
    validate_terminal_values(input.parameters, input.terminal_value_derivatives)
}

fn validate_common_transform_input(
    parameters: &MarketPressureParameters,
    interval_delta: f64,
    signed_balance_exposure_scale: f64,
    return_loading: ComplexScalar,
    pressure_signed_imbalance_exposure_risk_price: f64,
) -> Result<(), MarketPressureStateEvolutionError> {
    parameters
        .validate()
        .map_err(MarketPressureStateEvolutionError::InvalidParameters)?;

    validate_non_negative_number(interval_delta, "interval_delta").map_err(|source| {
        MarketPressureStateEvolutionError::InvalidIntervalDelta {
            value: source.value,
        }
    })?;

    validate_non_negative_number(
        signed_balance_exposure_scale,
        "signed_balance_exposure_scale",
    )
    .map_err(|source| {
        MarketPressureStateEvolutionError::InvalidSignedBalanceExposureScale {
            value: source.value,
        }
    })?;

    if !return_loading.is_finite() {
        return Err(MarketPressureStateEvolutionError::InvalidTransformValue {
            field: "return_loading",
            value: return_loading,
        });
    }

    validate_finite_number(
        pressure_signed_imbalance_exposure_risk_price,
        "pressure_signed_imbalance_exposure_risk_price",
    )
    .map_err(
        |source| MarketPressureStateEvolutionError::InvalidTransformValue {
            field: "pressure_signed_imbalance_exposure_risk_price",
            value: ComplexScalar::new(source.value, 0.0),
        },
    )?;

    Ok(())
}

fn validate_terminal_values(
    parameters: &MarketPressureParameters,
    terminal_values: &[ComplexScalar],
) -> Result<(), MarketPressureStateEvolutionError> {
    let expected_state_count = parameters.state_count();
    let actual_state_count = terminal_values.len();
    if actual_state_count != expected_state_count {
        return Err(MarketPressureStateEvolutionError::InvalidState(
            MarketPressureStateValidationError::StateCountMismatch {
                expected_state_count,
                actual_state_count,
            },
        ));
    }

    for (state_position, value) in terminal_values.iter().copied().enumerate() {
        if !value.is_finite() {
            return Err(
                MarketPressureStateEvolutionError::InvalidTerminalTransformValue {
                    state_position,
                    value,
                },
            );
        }
    }

    Ok(())
}

fn build_backward_tilted_generator(
    parameters: &MarketPressureParameters,
    return_loading: ComplexScalar,
    pressure_signed_imbalance_exposure_risk_price: f64,
    signed_balance_exposure_scale: f64,
) -> Result<DMatrix<Complex<f64>>, MarketPressureStateEvolutionError> {
    parameters
        .transition_rates
        .validate_for_state_count(parameters.state_count())
        .map_err(|source| {
            MarketPressureStateEvolutionError::InvalidCtmcTransition(
                MarketPressureCtmcTransitionError::InvalidTransitionRates { source },
            )
        })?;

    let state_count = parameters.state_count();

    let mut generator = DMatrix::<Complex<f64>>::zeros(state_count, state_count);

    for state_position in 0..state_count {
        let mut exit_rate = 0.0;

        if state_position + 1 < state_count {
            let upward_rate = parameters.transition_rates.upward_transition_rates[state_position];

            generator[(state_position, state_position + 1)] = Complex::new(upward_rate, 0.0);
            exit_rate += upward_rate;
        }

        if state_position > 0 {
            let downward_rate =
                parameters.transition_rates.downward_transition_rates[state_position - 1];

            generator[(state_position, state_position - 1)] = Complex::new(downward_rate, 0.0);
            exit_rate += downward_rate;
        }

        let side_imbalance = parameters.side_imbalance_levels[state_position];

        validate_finite_number(side_imbalance, "side_imbalance_level").map_err(|source| {
            MarketPressureStateEvolutionError::InvalidExpectedSideImbalance {
                value: source.value,
            }
        })?;

        let pressure_loading = return_loading
            .scale(parameters.anchor_drift_per_signed_balance)
            .subtract(ComplexScalar::new(
                pressure_signed_imbalance_exposure_risk_price,
                0.0,
            ))
            .scale(signed_balance_exposure_scale * side_imbalance);

        generator[(state_position, state_position)] =
            complex_to_nalgebra(pressure_loading) - Complex::new(exit_rate, 0.0);
    }

    Ok(generator)
}

fn complex_vector_from_values(
    values: &[ComplexScalar],
) -> Result<DVector<Complex<f64>>, MarketPressureStateEvolutionError> {
    let mut vector = DVector::<Complex<f64>>::zeros(values.len());

    for (position, value) in values.iter().copied().enumerate() {
        if !value.is_finite() {
            return Err(
                MarketPressureStateEvolutionError::InvalidTerminalTransformValue {
                    state_position: position,
                    value,
                },
            );
        }

        vector[position] = complex_to_nalgebra(value);
    }

    Ok(vector)
}

fn complex_weighted_sum(
    probabilities: &[f64],
    values: &[ComplexScalar],
) -> Result<ComplexScalar, MarketPressureStateEvolutionError> {
    if probabilities.len() != values.len() {
        return Err(MarketPressureStateEvolutionError::InvalidState(
            MarketPressureStateValidationError::StateCountMismatch {
                expected_state_count: values.len(),
                actual_state_count: probabilities.len(),
            },
        ));
    }

    let mut total = ComplexScalar::new(0.0, 0.0);

    for (probability, value) in probabilities.iter().zip(values.iter()) {
        total = total.plus(value.scale(*probability));
    }

    validate_transform_value(total)
}

fn validate_transform_value(
    value: ComplexScalar,
) -> Result<ComplexScalar, MarketPressureStateEvolutionError> {
    if !value.is_finite() {
        return Err(MarketPressureStateEvolutionError::InvalidTransformValue {
            field: "market_pressure_transform_value",
            value,
        });
    }

    Ok(value)
}

fn complex_to_nalgebra(value: ComplexScalar) -> Complex<f64> {
    Complex::new(value.real, value.imaginary)
}

fn complex_scalar_from_nalgebra(value: Complex<f64>) -> ComplexScalar {
    ComplexScalar::new(value.re, value.im)
}

#[cfg(test)]
mod tests {
    use janiform_foundations::complex_math::{ComplexScalar, complex_exponential};

    use super::{
        MarketPressureTransformLogFactorInput, MarketPressureTransformValuesAndDerivativeInput,
        MarketPressureTransformValuesInput, market_pressure_transform_log_factor,
        market_pressure_transform_values, market_pressure_transform_values_and_derivative,
    };
    use crate::market_pressure::{
        MarketPressureParameters, MarketPressureState, MarketPressureTransitionRates,
    };

    #[test]
    fn one_state_transform_matches_scalar_exponential() {
        let parameters = MarketPressureParameters::try_new(
            vec![0.5],
            MarketPressureTransitionRates::single_state(),
            2.0,
        )
        .expect("parameters should construct");

        let values = market_pressure_transform_values(MarketPressureTransformValuesInput {
            parameters: &parameters,
            interval_delta: 3.0,
            signed_balance_exposure_scale: 4.0,
            return_loading: ComplexScalar::new(0.25, 0.0),
            pressure_signed_imbalance_exposure_risk_price: 0.0,
            terminal_values: &[ComplexScalar::new(1.0, 0.0)],
        })
        .expect("transform should solve");

        assert_eq!(
            values,
            vec![complex_exponential(ComplexScalar::new(3.0, 0.0))]
        );
    }

    #[test]
    fn zero_loading_preserves_ctmc_terminal_ones() {
        let parameters = binary_pressure_parameters();

        let values = market_pressure_transform_values(MarketPressureTransformValuesInput {
            parameters: &parameters,
            interval_delta: 2.0,
            signed_balance_exposure_scale: 1.0,
            return_loading: ComplexScalar::new(0.0, 0.0),
            pressure_signed_imbalance_exposure_risk_price: 0.0,
            terminal_values: &[ComplexScalar::new(1.0, 0.0), ComplexScalar::new(1.0, 0.0)],
        })
        .expect("transform should solve");

        assert_eq!(
            values,
            vec![ComplexScalar::new(1.0, 0.0), ComplexScalar::new(1.0, 0.0)]
        );
    }

    #[test]
    fn log_factor_weights_initial_state_distribution() {
        let parameters = binary_pressure_parameters();

        let initial_state =
            MarketPressureState::try_new(vec![0.75, 0.25]).expect("state should construct");

        let log_factor =
            market_pressure_transform_log_factor(MarketPressureTransformLogFactorInput {
                values_input: MarketPressureTransformValuesInput {
                    parameters: &parameters,
                    interval_delta: 0.0,
                    signed_balance_exposure_scale: 1.0,
                    return_loading: ComplexScalar::new(1.0, 0.0),
                    pressure_signed_imbalance_exposure_risk_price: 0.0,
                    terminal_values: &[ComplexScalar::new(2.0, 0.0), ComplexScalar::new(4.0, 0.0)],
                },
                initial_state: &initial_state,
            })
            .expect("log factor should solve");

        assert_eq!(log_factor, ComplexScalar::new(2.5_f64.ln(), 0.0));
    }

    #[test]
    fn transform_derivative_matches_one_state_closed_form() {
        let parameters = MarketPressureParameters::try_new(
            vec![0.5],
            MarketPressureTransitionRates::single_state(),
            2.0,
        )
        .expect("parameters should construct");

        let (values, derivatives) = market_pressure_transform_values_and_derivative(
            MarketPressureTransformValuesAndDerivativeInput {
                parameters: &parameters,
                interval_delta: 3.0,
                signed_balance_exposure_scale: 4.0,
                return_loading: ComplexScalar::new(0.25, 0.0),
                return_loading_derivative: ComplexScalar::new(1.0, 0.0),
                pressure_signed_imbalance_exposure_risk_price: 0.0,
                pressure_signed_imbalance_exposure_risk_price_derivative: 0.0,
                terminal_values: &[ComplexScalar::new(1.0, 0.0)],
                terminal_value_derivatives: &[ComplexScalar::new(0.0, 0.0)],
            },
        )
        .expect("transform should solve");

        assert_eq!(derivatives, vec![values[0].scale(12.0)]);
    }

    #[test]
    fn pressure_risk_price_derivative_matches_one_state_closed_form() {
        let parameters = MarketPressureParameters::try_new(
            vec![0.5],
            MarketPressureTransitionRates::single_state(),
            2.0,
        )
        .expect("parameters should construct");

        let (values, derivatives) = market_pressure_transform_values_and_derivative(
            MarketPressureTransformValuesAndDerivativeInput {
                parameters: &parameters,
                interval_delta: 3.0,
                signed_balance_exposure_scale: 4.0,
                return_loading: ComplexScalar::new(0.25, 0.0),
                return_loading_derivative: ComplexScalar::new(0.0, 0.0),
                pressure_signed_imbalance_exposure_risk_price: 0.0,
                pressure_signed_imbalance_exposure_risk_price_derivative: 1.0,
                terminal_values: &[ComplexScalar::new(1.0, 0.0)],
                terminal_value_derivatives: &[ComplexScalar::new(0.0, 0.0)],
            },
        )
        .expect("transform should solve");

        assert_eq!(derivatives, vec![values[0].scale(-6.0)]);
    }

    fn binary_pressure_parameters() -> MarketPressureParameters {
        MarketPressureParameters::try_new(
            vec![-0.25, 0.25],
            MarketPressureTransitionRates::new(vec![0.5], vec![0.25]),
            0.75,
        )
        .expect("parameters should construct")
    }
}
