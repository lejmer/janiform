//! Pricing-side hourly factor loading through the shared provider interface.

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
use crate::return_decomposition::ReturnDecomposition;

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
    driver_parameters.compose_trading_time_weighted_characteristic_exponent(
        argument,
        return_decomposition.integrated_buy_intensity(),
        return_decomposition.integrated_sell_intensity(),
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
    driver_parameters.compose_trading_time_weighted_characteristic_exponent(
        argument,
        return_decomposition.integrated_buy_intensity(),
        return_decomposition.integrated_sell_intensity(),
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

#[cfg(test)]
mod tests {
    use crate::nig::{ComplexScalar, NigSharedPlusShiftConfiguration};
    use crate::return_decomposition::{ReturnDecomposition, ReturnDecompositionInput};

    use super::{
        compose_nig_trading_time_exponent_for_coarse_transform_integration,
        compose_nig_trading_time_exponent_for_pricing,
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

    fn assert_complex_close(observed: ComplexScalar, expected: ComplexScalar, tolerance: f64) {
        let difference = observed.subtract(expected).norm();
        assert!(
            difference <= tolerance,
            "difference {difference} exceeded tolerance {tolerance} for observed {observed:?} and expected {expected:?}"
        );
    }
}
