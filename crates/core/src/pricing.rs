//! Pricing-side hourly factor loading through the shared provider interface.

use std::error::Error;
use std::fmt::{Display, Formatter};

use crate::calendar::ExchangeCalendarRegistry;
use crate::hawkes::{
    HawkesExcitationState, HawkesIntensityProjection, HawkesKernelParameters,
    HawkesSeasonalityIntervalError, HawkesTransitionError, HawkesTransitionStep,
};
use crate::hourly_factor_provider::{
    HourlyFactorProviderError, HourlyFactorProviderRequest, provide_hourly_factor_contract,
};
use crate::hourly_factors::HourlyFactorContractOutput;
use crate::nig::{
    ComplexScalar, NigBuySellDriverParameters, NigTradingTimeCharacteristicExponentContributions,
};
use crate::return_decomposition::{
    ReturnDecomposition, ReturnDecompositionEdgeMode, ReturnDecompositionValidationError,
};

pub const RETURN_CHARACTERISTIC_FUNCTION_COMPOSITION_POLICY: &str =
    "exp_of_trading_time_weighted_exponent_times_explicit_gap_characteristic_factor.v1";
pub const RETURN_CHARACTERISTIC_FUNCTION_DOMAIN_GUARD_POLICY: &str = "finite_argument_and_gap_factor_with_valid_return_decomposition_and_finite_composition_result.v1";

/// Full return characteristic-function assembly output.
///
/// Equation reference:
/// \f[
/// \exp\!\left(A^+\psi_+(u) + A^-\psi_-(-u)\right)\,\varphi_{\Delta G}(u).
/// \f]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReturnCharacteristicFunctionAssembly {
    pub trading_time_exponent_contributions: NigTradingTimeCharacteristicExponentContributions,
    pub trading_time_characteristic_factor: ComplexScalar,
    pub gap_characteristic_factor: ComplexScalar,
    pub combined_characteristic_factor: ComplexScalar,
    pub composition_policy: &'static str,
    pub domain_guard_policy: &'static str,
}

/// Structured failure modes for full return characteristic-function assembly.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ReturnCharacteristicFunctionAssemblyError {
    InvalidCharacteristicFunctionArgument {
        argument: ComplexScalar,
    },
    InvalidGapCharacteristicFactor {
        value: ComplexScalar,
    },
    InvalidReturnDecomposition {
        source: ReturnDecompositionValidationError,
    },
    NonFiniteTradingTimeExponent {
        value: ComplexScalar,
    },
    NonFiniteTradingTimeCharacteristicFactor {
        value: ComplexScalar,
    },
    NonFiniteCombinedCharacteristicFactor {
        value: ComplexScalar,
    },
}

impl Display for ReturnCharacteristicFunctionAssemblyError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidCharacteristicFunctionArgument { argument } => write!(
                formatter,
                "characteristic-function argument must be finite, found {argument:?}"
            ),
            Self::InvalidGapCharacteristicFactor { value } => write!(
                formatter,
                "gap characteristic factor must be finite, found {value:?}"
            ),
            Self::InvalidReturnDecomposition { source } => {
                write!(
                    formatter,
                    "return decomposition payload is invalid for characteristic-function assembly: {source}"
                )
            }
            Self::NonFiniteTradingTimeExponent { value } => write!(
                formatter,
                "trading-time weighted exponent must be finite, found {value:?}"
            ),
            Self::NonFiniteTradingTimeCharacteristicFactor { value } => write!(
                formatter,
                "trading-time characteristic factor must be finite, found {value:?}"
            ),
            Self::NonFiniteCombinedCharacteristicFactor { value } => write!(
                formatter,
                "combined return characteristic factor must be finite, found {value:?}"
            ),
        }
    }
}

impl Error for ReturnCharacteristicFunctionAssemblyError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidReturnDecomposition { source } => Some(source),
            _ => None,
        }
    }
}

/// Resolve interval-aligned hourly factors for the pricing path.
pub fn load_hourly_factor_contract_for_pricing(
    calendar_registry: &ExchangeCalendarRegistry,
    provider_request: HourlyFactorProviderRequest<'_>,
) -> Result<HourlyFactorContractOutput, HourlyFactorProviderError> {
    provide_hourly_factor_contract(calendar_registry, provider_request)
}

/// Project state to interval intensities for coarse-time sufficient-statistics paths.
pub fn project_hawkes_intensities_for_coarse_interval(
    state: HawkesExcitationState,
    parameters: HawkesKernelParameters,
    hourly_factor_contract: &HourlyFactorContractOutput,
    interval_index: usize,
) -> Result<HawkesIntensityProjection, HawkesSeasonalityIntervalError> {
    state.project_intensities_for_hourly_interval(
        parameters.lambda_bar,
        hourly_factor_contract,
        interval_index,
    )
}

/// Compose trading-time characteristic exponent for pricing using canonical sign handling.
///
/// Equation reference:
/// \f[
/// A^+\psi_+(u) + A^-\psi_-(-u).
/// \f]
pub fn compose_nig_trading_time_exponent_for_pricing(
    driver_parameters: NigBuySellDriverParameters,
    argument: ComplexScalar,
    return_decomposition: ReturnDecomposition,
) -> NigTradingTimeCharacteristicExponentContributions {
    compose_nig_trading_time_exponent_for_pricing_with_edge_mode(
        driver_parameters,
        argument,
        return_decomposition,
        ReturnDecompositionEdgeMode::Disabled,
    )
}

/// Compose trading-time characteristic exponent for pricing with optional edge-mode gating.
pub fn compose_nig_trading_time_exponent_for_pricing_with_edge_mode(
    driver_parameters: NigBuySellDriverParameters,
    argument: ComplexScalar,
    return_decomposition: ReturnDecomposition,
    edge_mode: ReturnDecompositionEdgeMode,
) -> NigTradingTimeCharacteristicExponentContributions {
    return_decomposition.compose_nig_trading_time_exponent_with_edge_mode(
        driver_parameters,
        argument,
        edge_mode,
    )
}

/// Compose trading-time characteristic exponent for coarse-time transform integration.
///
/// This path delegates to the same helper as the pricing path to keep sell-side
/// sign convention logic in one place.
pub fn compose_nig_trading_time_exponent_for_coarse_transform_integration(
    driver_parameters: NigBuySellDriverParameters,
    argument: ComplexScalar,
    return_decomposition: ReturnDecomposition,
) -> NigTradingTimeCharacteristicExponentContributions {
    compose_nig_trading_time_exponent_for_coarse_transform_integration_with_edge_mode(
        driver_parameters,
        argument,
        return_decomposition,
        ReturnDecompositionEdgeMode::Disabled,
    )
}

/// Compose coarse-transform trading-time exponent with optional edge-mode gating.
pub fn compose_nig_trading_time_exponent_for_coarse_transform_integration_with_edge_mode(
    driver_parameters: NigBuySellDriverParameters,
    argument: ComplexScalar,
    return_decomposition: ReturnDecomposition,
    edge_mode: ReturnDecompositionEdgeMode,
) -> NigTradingTimeCharacteristicExponentContributions {
    return_decomposition.compose_nig_trading_time_exponent_with_edge_mode(
        driver_parameters,
        argument,
        edge_mode,
    )
}

/// Compose full return characteristic function for pricing through one shared assembler.
///
/// This path reuses the canonical trading-time exponent composition helper, which
/// keeps sell-side `-u` convention logic in a single module.
pub fn compose_total_return_characteristic_function_for_pricing(
    driver_parameters: NigBuySellDriverParameters,
    argument: ComplexScalar,
    return_decomposition: ReturnDecomposition,
    gap_characteristic_factor: ComplexScalar,
) -> Result<ReturnCharacteristicFunctionAssembly, ReturnCharacteristicFunctionAssemblyError> {
    compose_total_return_characteristic_function_for_pricing_with_edge_mode(
        driver_parameters,
        argument,
        return_decomposition,
        gap_characteristic_factor,
        ReturnDecompositionEdgeMode::Disabled,
    )
}

/// Compose full return characteristic function for pricing with optional edge-mode gating.
pub fn compose_total_return_characteristic_function_for_pricing_with_edge_mode(
    driver_parameters: NigBuySellDriverParameters,
    argument: ComplexScalar,
    return_decomposition: ReturnDecomposition,
    gap_characteristic_factor: ComplexScalar,
    edge_mode: ReturnDecompositionEdgeMode,
) -> Result<ReturnCharacteristicFunctionAssembly, ReturnCharacteristicFunctionAssemblyError> {
    compose_total_return_characteristic_function_shared(
        driver_parameters,
        argument,
        return_decomposition,
        gap_characteristic_factor,
        edge_mode,
    )
}

/// Compose full return characteristic function for coarse-time transform integration.
///
/// This path delegates to the same shared assembler as the pricing path.
pub fn compose_total_return_characteristic_function_for_coarse_transform_integration(
    driver_parameters: NigBuySellDriverParameters,
    argument: ComplexScalar,
    return_decomposition: ReturnDecomposition,
    gap_characteristic_factor: ComplexScalar,
) -> Result<ReturnCharacteristicFunctionAssembly, ReturnCharacteristicFunctionAssemblyError> {
    compose_total_return_characteristic_function_for_coarse_transform_integration_with_edge_mode(
        driver_parameters,
        argument,
        return_decomposition,
        gap_characteristic_factor,
        ReturnDecompositionEdgeMode::Disabled,
    )
}

/// Compose full return characteristic function for coarse transform with edge-mode gating.
pub fn compose_total_return_characteristic_function_for_coarse_transform_integration_with_edge_mode(
    driver_parameters: NigBuySellDriverParameters,
    argument: ComplexScalar,
    return_decomposition: ReturnDecomposition,
    gap_characteristic_factor: ComplexScalar,
    edge_mode: ReturnDecompositionEdgeMode,
) -> Result<ReturnCharacteristicFunctionAssembly, ReturnCharacteristicFunctionAssemblyError> {
    compose_total_return_characteristic_function_shared(
        driver_parameters,
        argument,
        return_decomposition,
        gap_characteristic_factor,
        edge_mode,
    )
}

/// Apply one coarse-time transform-path Hawkes state transition using canonical call order.
///
/// Call order is delegated to `HawkesExcitationState::apply_transition_step`:
/// drift, then optional endogenous jump, then optional exogenous jump.
pub fn evolve_hawkes_state_for_coarse_interval_step(
    state: HawkesExcitationState,
    kernel_parameters: HawkesKernelParameters,
    transition_step: HawkesTransitionStep,
) -> Result<HawkesExcitationState, HawkesTransitionError> {
    state.apply_transition_step(kernel_parameters, transition_step)
}

fn compose_total_return_characteristic_function_shared(
    driver_parameters: NigBuySellDriverParameters,
    argument: ComplexScalar,
    return_decomposition: ReturnDecomposition,
    gap_characteristic_factor: ComplexScalar,
    edge_mode: ReturnDecompositionEdgeMode,
) -> Result<ReturnCharacteristicFunctionAssembly, ReturnCharacteristicFunctionAssemblyError> {
    if !argument.is_finite() {
        return Err(
            ReturnCharacteristicFunctionAssemblyError::InvalidCharacteristicFunctionArgument {
                argument,
            },
        );
    }

    let effective_decomposition = return_decomposition.with_edge_mode(edge_mode);
    let effective_gap_characteristic_factor = if edge_mode.gap_component_enabled() {
        gap_characteristic_factor
    } else {
        ComplexScalar::new(1.0, 0.0)
    };

    if !effective_gap_characteristic_factor.is_finite() {
        return Err(
            ReturnCharacteristicFunctionAssemblyError::InvalidGapCharacteristicFactor {
                value: effective_gap_characteristic_factor,
            },
        );
    }

    effective_decomposition.validate().map_err(|source| {
        ReturnCharacteristicFunctionAssemblyError::InvalidReturnDecomposition { source }
    })?;

    let trading_time_exponent_contributions =
        effective_decomposition.compose_nig_trading_time_exponent(driver_parameters, argument);
    let combined_weighted_exponent = trading_time_exponent_contributions.combined_weighted_exponent;
    if !combined_weighted_exponent.is_finite() {
        return Err(
            ReturnCharacteristicFunctionAssemblyError::NonFiniteTradingTimeExponent {
                value: combined_weighted_exponent,
            },
        );
    }

    let trading_time_characteristic_factor = complex_exponential(combined_weighted_exponent);
    if !trading_time_characteristic_factor.is_finite() {
        return Err(
            ReturnCharacteristicFunctionAssemblyError::NonFiniteTradingTimeCharacteristicFactor {
                value: trading_time_characteristic_factor,
            },
        );
    }

    let combined_characteristic_factor = complex_multiply(
        trading_time_characteristic_factor,
        effective_gap_characteristic_factor,
    );
    if !combined_characteristic_factor.is_finite() {
        return Err(
            ReturnCharacteristicFunctionAssemblyError::NonFiniteCombinedCharacteristicFactor {
                value: combined_characteristic_factor,
            },
        );
    }

    Ok(ReturnCharacteristicFunctionAssembly {
        trading_time_exponent_contributions,
        trading_time_characteristic_factor,
        gap_characteristic_factor: effective_gap_characteristic_factor,
        combined_characteristic_factor,
        composition_policy: RETURN_CHARACTERISTIC_FUNCTION_COMPOSITION_POLICY,
        domain_guard_policy: RETURN_CHARACTERISTIC_FUNCTION_DOMAIN_GUARD_POLICY,
    })
}

fn complex_exponential(value: ComplexScalar) -> ComplexScalar {
    let real_exponential = value.real.exp();
    ComplexScalar::new(
        real_exponential * value.imaginary.cos(),
        real_exponential * value.imaginary.sin(),
    )
}

fn complex_multiply(left: ComplexScalar, right: ComplexScalar) -> ComplexScalar {
    ComplexScalar::new(
        left.real
            .mul_add(right.real, -(left.imaginary * right.imaginary)),
        left.real
            .mul_add(right.imaginary, left.imaginary * right.real),
    )
}

#[cfg(test)]
mod tests {
    use crate::nig::{ComplexScalar, NigSharedPlusShiftConfiguration};
    use crate::pricing::{
        RETURN_CHARACTERISTIC_FUNCTION_COMPOSITION_POLICY,
        RETURN_CHARACTERISTIC_FUNCTION_DOMAIN_GUARD_POLICY,
        ReturnCharacteristicFunctionAssemblyError,
        compose_total_return_characteristic_function_for_coarse_transform_integration,
        compose_total_return_characteristic_function_for_pricing,
        compose_total_return_characteristic_function_for_pricing_with_edge_mode,
    };
    use crate::return_decomposition::{
        ReturnDecomposition, ReturnDecompositionEdgeMode, ReturnDecompositionInput,
    };

    use super::{
        compose_nig_trading_time_exponent_for_coarse_transform_integration,
        compose_nig_trading_time_exponent_for_coarse_transform_integration_with_edge_mode,
        compose_nig_trading_time_exponent_for_pricing,
        compose_nig_trading_time_exponent_for_pricing_with_edge_mode,
    };

    #[test]
    fn pricing_and_coarse_transform_paths_share_sign_convention_helper() {
        let driver_parameters = NigSharedPlusShiftConfiguration::try_new(2.3, -0.8, 0.7, 0.1, -0.2)
            .expect("valid shared-plus-shift payload should construct")
            .derive_driver_parameters()
            .expect("valid shared-plus-shift payload should derive drivers");
        let argument = ComplexScalar::new(0.7, 0.2);
        let return_decomposition = ReturnDecomposition::try_new(ReturnDecompositionInput {
            a_plus: 1.3,
            a_minus: 0.9,
            gap: 0.0,
        })
        .expect("valid return decomposition payload should construct");

        let pricing_contributions = compose_nig_trading_time_exponent_for_pricing(
            driver_parameters,
            argument,
            return_decomposition,
        );
        let coarse_transform_contributions =
            compose_nig_trading_time_exponent_for_coarse_transform_integration(
                driver_parameters,
                argument,
                return_decomposition,
            );

        assert_eq!(pricing_contributions, coarse_transform_contributions);
    }

    #[test]
    fn pricing_and_coarse_adapters_match_shared_return_decomposition_interface() {
        let driver_parameters = NigSharedPlusShiftConfiguration::try_new(2.3, -0.8, 0.7, 0.1, -0.2)
            .expect("valid shared-plus-shift payload should construct")
            .derive_driver_parameters()
            .expect("valid shared-plus-shift payload should derive drivers");
        let argument = ComplexScalar::new(0.45, -0.15);
        let return_decomposition = ReturnDecomposition::try_new(ReturnDecompositionInput {
            a_plus: 1.8,
            a_minus: 0.6,
            gap: -0.25,
        })
        .expect("valid return decomposition payload should construct");

        let pricing_observed = compose_nig_trading_time_exponent_for_pricing_with_edge_mode(
            driver_parameters,
            argument,
            return_decomposition,
            ReturnDecompositionEdgeMode::SellOnly,
        );
        let coarse_observed =
            compose_nig_trading_time_exponent_for_coarse_transform_integration_with_edge_mode(
                driver_parameters,
                argument,
                return_decomposition,
                ReturnDecompositionEdgeMode::SellOnly,
            );
        let shared_expected = return_decomposition
            .compose_nig_trading_time_exponent_with_edge_mode(
                driver_parameters,
                argument,
                ReturnDecompositionEdgeMode::SellOnly,
            );

        assert_eq!(pricing_observed, shared_expected);
        assert_eq!(coarse_observed, shared_expected);
    }

    #[test]
    fn pricing_path_preserves_sell_side_minus_u_sign_handling() {
        let driver_parameters = NigSharedPlusShiftConfiguration::try_new(2.3, -0.8, 0.7, 0.1, -0.2)
            .expect("valid shared-plus-shift payload should construct")
            .derive_driver_parameters()
            .expect("valid shared-plus-shift payload should derive drivers");
        let argument = ComplexScalar::new(1.1, 0.0);
        let return_decomposition = ReturnDecomposition::try_new(ReturnDecompositionInput {
            a_plus: 0.0,
            a_minus: 1.0,
            gap: 0.0,
        })
        .expect("valid return decomposition payload should construct");

        let composed = compose_nig_trading_time_exponent_for_pricing(
            driver_parameters,
            argument,
            return_decomposition,
        );

        let expected_sell_side_minus_u = driver_parameters
            .sell_side_parameters
            .characteristic_exponent(argument.scale(-1.0));
        assert_complex_close(
            composed.sell_side_weighted_exponent,
            expected_sell_side_minus_u,
            1.0e-13,
        );

        let incorrect_positive_u = driver_parameters
            .sell_side_parameters
            .characteristic_exponent(argument);
        let regression_distance = composed
            .sell_side_weighted_exponent
            .subtract(incorrect_positive_u)
            .norm();
        assert!(
            regression_distance > 1.0e-6,
            "sell-side sign-convention regression detected with distance={regression_distance}"
        );
    }

    #[test]
    fn full_characteristic_function_assembly_uses_trading_time_factor_when_gap_is_unity() {
        let driver_parameters = NigSharedPlusShiftConfiguration::try_new(2.3, -0.8, 0.7, 0.1, -0.2)
            .expect("valid shared-plus-shift payload should construct")
            .derive_driver_parameters()
            .expect("valid shared-plus-shift payload should derive drivers");
        let argument = ComplexScalar::new(1.1, 0.0);
        let return_decomposition = ReturnDecomposition::try_new(ReturnDecompositionInput {
            a_plus: 0.0,
            a_minus: 1.0,
            gap: 0.0,
        })
        .expect("valid return decomposition payload should construct");
        let gap_characteristic_factor = ComplexScalar::new(1.0, 0.0);

        let assembled = compose_total_return_characteristic_function_for_pricing(
            driver_parameters,
            argument,
            return_decomposition,
            gap_characteristic_factor,
        )
        .expect("full CF assembly should succeed with valid finite inputs");

        let expected_sell_side_minus_u = driver_parameters
            .sell_side_parameters
            .characteristic_exponent(argument.scale(-1.0));
        assert_complex_close(
            assembled
                .trading_time_exponent_contributions
                .sell_side_weighted_exponent,
            expected_sell_side_minus_u,
            1.0e-13,
        );

        assert_complex_close(
            assembled.combined_characteristic_factor,
            assembled.trading_time_characteristic_factor,
            1.0e-13,
        );
        assert_eq!(
            assembled.composition_policy,
            RETURN_CHARACTERISTIC_FUNCTION_COMPOSITION_POLICY
        );
        assert_eq!(
            assembled.domain_guard_policy,
            RETURN_CHARACTERISTIC_FUNCTION_DOMAIN_GUARD_POLICY
        );
    }

    #[test]
    fn full_characteristic_function_assembly_multiplies_exp_trading_term_and_gap_factor() {
        let driver_parameters = NigSharedPlusShiftConfiguration::try_new(2.3, -0.8, 0.7, 0.1, -0.2)
            .expect("valid shared-plus-shift payload should construct")
            .derive_driver_parameters()
            .expect("valid shared-plus-shift payload should derive drivers");
        let argument = ComplexScalar::new(-0.4, 0.3);
        let return_decomposition = ReturnDecomposition::try_new(ReturnDecompositionInput {
            a_plus: 1.4,
            a_minus: 0.7,
            gap: -0.2,
        })
        .expect("valid return decomposition payload should construct");
        let gap_characteristic_factor = ComplexScalar::new(0.85, -0.25);

        let assembled = compose_total_return_characteristic_function_for_pricing(
            driver_parameters,
            argument,
            return_decomposition,
            gap_characteristic_factor,
        )
        .expect("full CF assembly should succeed with valid finite inputs");

        let expected_trading_exponent = compose_nig_trading_time_exponent_for_pricing(
            driver_parameters,
            argument,
            return_decomposition,
        )
        .combined_weighted_exponent;
        let expected_trading_factor = super::complex_exponential(expected_trading_exponent);
        let expected_combined =
            super::complex_multiply(expected_trading_factor, gap_characteristic_factor);

        assert_complex_close(
            assembled.trading_time_characteristic_factor,
            expected_trading_factor,
            1.0e-13,
        );
        assert_complex_close(
            assembled.combined_characteristic_factor,
            expected_combined,
            1.0e-13,
        );
    }

    #[test]
    fn full_characteristic_function_assembly_is_shared_between_pricing_and_coarse_paths() {
        let driver_parameters = NigSharedPlusShiftConfiguration::try_new(2.3, -0.8, 0.7, 0.1, -0.2)
            .expect("valid shared-plus-shift payload should construct")
            .derive_driver_parameters()
            .expect("valid shared-plus-shift payload should derive drivers");
        let argument = ComplexScalar::new(0.2, -0.15);
        let return_decomposition = ReturnDecomposition::try_new(ReturnDecompositionInput {
            a_plus: 1.1,
            a_minus: 0.9,
            gap: 0.0,
        })
        .expect("valid return decomposition payload should construct");
        let gap_characteristic_factor = ComplexScalar::new(0.98, 0.05);

        let pricing_assembled = compose_total_return_characteristic_function_for_pricing(
            driver_parameters,
            argument,
            return_decomposition,
            gap_characteristic_factor,
        )
        .expect("pricing full CF assembly should succeed");
        let coarse_assembled =
            compose_total_return_characteristic_function_for_coarse_transform_integration(
                driver_parameters,
                argument,
                return_decomposition,
                gap_characteristic_factor,
            )
            .expect("coarse full CF assembly should succeed");

        assert_eq!(pricing_assembled, coarse_assembled);
    }

    #[test]
    fn edge_mode_buy_only_disables_sell_and_gap_components_in_full_cf_assembly() {
        let driver_parameters = NigSharedPlusShiftConfiguration::try_new(2.3, -0.8, 0.7, 0.1, -0.2)
            .expect("valid shared-plus-shift payload should construct")
            .derive_driver_parameters()
            .expect("valid shared-plus-shift payload should derive drivers");
        let argument = ComplexScalar::new(-0.25, 0.1);

        let return_decomposition = ReturnDecomposition::try_new(ReturnDecompositionInput {
            a_plus: 1.2,
            a_minus: 0.9,
            gap: -0.35,
        })
        .expect("valid return decomposition payload should construct");

        let assembled = compose_total_return_characteristic_function_for_pricing_with_edge_mode(
            driver_parameters,
            argument,
            return_decomposition,
            ComplexScalar::new(0.8, -0.1),
            ReturnDecompositionEdgeMode::BuyOnly,
        )
        .expect("buy-only edge mode CF assembly should succeed");

        let expected_buy = driver_parameters
            .buy_side_parameters
            .characteristic_exponent(argument)
            .scale(1.2);
        let expected_trading = super::complex_exponential(expected_buy);

        assert_complex_close(
            assembled
                .trading_time_exponent_contributions
                .buy_side_weighted_exponent,
            expected_buy,
            1.0e-13,
        );
        assert_complex_close(
            assembled
                .trading_time_exponent_contributions
                .sell_side_weighted_exponent,
            ComplexScalar::new(0.0, 0.0),
            1.0e-13,
        );
        assert_complex_close(
            assembled.gap_characteristic_factor,
            ComplexScalar::new(1.0, 0.0),
            1.0e-13,
        );
        assert_complex_close(
            assembled.combined_characteristic_factor,
            expected_trading,
            1.0e-13,
        );
    }

    #[test]
    fn edge_mode_sell_only_preserves_minus_u_sign_and_disables_gap_component() {
        let driver_parameters = NigSharedPlusShiftConfiguration::try_new(2.3, -0.8, 0.7, 0.1, -0.2)
            .expect("valid shared-plus-shift payload should construct")
            .derive_driver_parameters()
            .expect("valid shared-plus-shift payload should derive drivers");
        let argument = ComplexScalar::new(0.9, -0.2);

        let return_decomposition = ReturnDecomposition::try_new(ReturnDecompositionInput {
            a_plus: 0.4,
            a_minus: 1.1,
            gap: -0.1,
        })
        .expect("valid return decomposition payload should construct");

        let assembled = compose_total_return_characteristic_function_for_pricing_with_edge_mode(
            driver_parameters,
            argument,
            return_decomposition,
            ComplexScalar::new(0.7, 0.05),
            ReturnDecompositionEdgeMode::SellOnly,
        )
        .expect("sell-only edge mode CF assembly should succeed");

        let expected_sell = driver_parameters
            .sell_side_parameters
            .characteristic_exponent(argument.scale(-1.0))
            .scale(1.1);
        let expected_trading = super::complex_exponential(expected_sell);

        assert_complex_close(
            assembled
                .trading_time_exponent_contributions
                .buy_side_weighted_exponent,
            ComplexScalar::new(0.0, 0.0),
            1.0e-13,
        );
        assert_complex_close(
            assembled
                .trading_time_exponent_contributions
                .sell_side_weighted_exponent,
            expected_sell,
            1.0e-13,
        );
        assert_complex_close(
            assembled.gap_characteristic_factor,
            ComplexScalar::new(1.0, 0.0),
            1.0e-13,
        );
        assert_complex_close(
            assembled.combined_characteristic_factor,
            expected_trading,
            1.0e-13,
        );
    }

    #[test]
    fn edge_mode_gap_only_isolates_gap_factor_in_full_cf_assembly() {
        let driver_parameters = NigSharedPlusShiftConfiguration::try_new(2.3, -0.8, 0.7, 0.1, -0.2)
            .expect("valid shared-plus-shift payload should construct")
            .derive_driver_parameters()
            .expect("valid shared-plus-shift payload should derive drivers");
        let argument = ComplexScalar::new(0.35, 0.25);

        let return_decomposition = ReturnDecomposition::try_new(ReturnDecompositionInput {
            a_plus: 0.9,
            a_minus: 0.7,
            gap: 0.6,
        })
        .expect("valid return decomposition payload should construct");
        let gap_characteristic_factor = ComplexScalar::new(0.65, -0.35);

        let assembled = compose_total_return_characteristic_function_for_pricing_with_edge_mode(
            driver_parameters,
            argument,
            return_decomposition,
            gap_characteristic_factor,
            ReturnDecompositionEdgeMode::GapOnly,
        )
        .expect("gap-only edge mode CF assembly should succeed");

        assert_complex_close(
            assembled
                .trading_time_exponent_contributions
                .combined_weighted_exponent,
            ComplexScalar::new(0.0, 0.0),
            1.0e-13,
        );
        assert_complex_close(
            assembled.trading_time_characteristic_factor,
            ComplexScalar::new(1.0, 0.0),
            1.0e-13,
        );
        assert_complex_close(
            assembled.gap_characteristic_factor,
            gap_characteristic_factor,
            1.0e-13,
        );
        assert_complex_close(
            assembled.combined_characteristic_factor,
            gap_characteristic_factor,
            1.0e-13,
        );
    }

    #[test]
    fn full_characteristic_function_assembly_rejects_non_finite_gap_factor() {
        let driver_parameters = NigSharedPlusShiftConfiguration::try_new(2.3, -0.8, 0.7, 0.1, -0.2)
            .expect("valid shared-plus-shift payload should construct")
            .derive_driver_parameters()
            .expect("valid shared-plus-shift payload should derive drivers");
        let argument = ComplexScalar::new(0.1, 0.0);
        let return_decomposition = ReturnDecomposition::try_new(ReturnDecompositionInput {
            a_plus: 1.0,
            a_minus: 0.4,
            gap: 0.0,
        })
        .expect("valid return decomposition payload should construct");

        let error = compose_total_return_characteristic_function_for_pricing(
            driver_parameters,
            argument,
            return_decomposition,
            ComplexScalar::new(f64::NAN, 0.0),
        )
        .expect_err("non-finite gap factor must fail full CF assembly");

        assert!(matches!(
            error,
            ReturnCharacteristicFunctionAssemblyError::InvalidGapCharacteristicFactor { value }
            if value.real.is_nan()
        ));
    }

    #[test]
    fn full_characteristic_function_assembly_rejects_invalid_return_decomposition_payload() {
        let driver_parameters = NigSharedPlusShiftConfiguration::try_new(2.3, -0.8, 0.7, 0.1, -0.2)
            .expect("valid shared-plus-shift payload should construct")
            .derive_driver_parameters()
            .expect("valid shared-plus-shift payload should derive drivers");
        let invalid_return_decomposition = ReturnDecomposition {
            a_plus: -0.1,
            a_minus: 0.3,
            gap: 0.0,
        };

        let error = compose_total_return_characteristic_function_for_pricing(
            driver_parameters,
            ComplexScalar::new(0.0, 0.0),
            invalid_return_decomposition,
            ComplexScalar::new(1.0, 0.0),
        )
        .expect_err("invalid return decomposition must fail full CF assembly");

        assert!(matches!(
            error,
            ReturnCharacteristicFunctionAssemblyError::InvalidReturnDecomposition { .. }
        ));
    }

    #[test]
    fn full_characteristic_function_regression_guard_detects_sell_side_sign_inversion() {
        let driver_parameters = NigSharedPlusShiftConfiguration::try_new(2.3, -0.8, 0.7, 0.1, -0.2)
            .expect("valid shared-plus-shift payload should construct")
            .derive_driver_parameters()
            .expect("valid shared-plus-shift payload should derive drivers");
        let argument = ComplexScalar::new(1.1, 0.0);
        let return_decomposition = ReturnDecomposition::try_new(ReturnDecompositionInput {
            a_plus: 0.0,
            a_minus: 1.0,
            gap: 0.0,
        })
        .expect("valid return decomposition payload should construct");

        let assembled = compose_total_return_characteristic_function_for_pricing(
            driver_parameters,
            argument,
            return_decomposition,
            ComplexScalar::new(1.0, 0.0),
        )
        .expect("full CF assembly should succeed");

        let incorrect_sell_side_positive_u = driver_parameters
            .sell_side_parameters
            .characteristic_exponent(argument);
        let incorrect_positive_u_combined = super::complex_exponential(
            assembled
                .trading_time_exponent_contributions
                .buy_side_weighted_exponent
                .add(incorrect_sell_side_positive_u),
        );
        let regression_distance = assembled
            .combined_characteristic_factor
            .subtract(incorrect_positive_u_combined)
            .norm();
        assert!(
            regression_distance > 1.0e-6,
            "sell-side sign-convention regression detected in full CF assembly with distance={regression_distance}"
        );
    }

    fn assert_complex_close(observed: ComplexScalar, expected: ComplexScalar, tolerance: f64) {
        let difference = observed.subtract(expected).norm();
        assert!(
            difference <= tolerance,
            "difference {difference} exceeded tolerance {tolerance} for observed {observed:?} and expected {expected:?}"
        );
    }
}
