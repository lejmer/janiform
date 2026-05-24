mod domain;
mod execution;
mod interval_operator;
mod math;
mod payoff;
mod row_frequency;
mod strike;

pub(super) use super::coordinates::asset_reference_log_price_from_observed_spot;
pub(super) use super::cosine_transform::{
    values_to_cosine_coefficients, values_to_cosine_coefficients_sensitivities,
};
pub(super) use super::sensitivity::{
    NodePriceSharedPlusShiftComplexSensitivity, NodePriceSharedPlusShiftSensitivity,
};
pub(super) use crate::constants::MINIMUM_SPECTRAL_COSINE_COLLOCATION_NODE_COUNT;
pub(super) use crate::contracts::{
    SpectralPricingDiagnostics, SpectralPricingIntervalDiagnostics, VanillaOptionPayoffType,
    VanillaOptionPricingFailureStage, VanillaOptionPricingMarketInputs,
    VanillaOptionPricingOptionalOutputSelection, VanillaOptionPricingRequest,
    VanillaOptionPricingStatus,
};
pub(super) use crate::grids::{
    SpectralThetaWarmStartDriverConfiguration, SpectralThetaWarmStartRowState,
    SpectralThetaWarmStartSensitivity, SpectralThetaWarmStartState,
    VanillaOptionGridBatchPricingRunError, VanillaOptionGridRequest, VanillaOptionMaturitySummary,
    VanillaOptionNodePriceSensitivity,
};
pub(super) use crate::implied_volatility::{
    VanillaOptionImpliedVolatilityInversionOutcome, VanillaOptionImpliedVolatilityInversionStatus,
    invert_vanilla_option_implied_volatility,
};
pub(super) use crate::intraday_black_scholes_no_arbitrage_price_bounds;
pub(super) use crate::progress::PricingProgressEvent;
pub(super) use crate::runner::{
    CoreLocalTiltImplicitThetaSharedPlusShiftSensitivity,
    CorePureImaginaryLogMomentSharedPlusShiftSensitivity, LocalTransitionRequestContext,
    LocalTransitionRowState, PricingCharacteristicFunctionSharedPlusShiftSensitivity,
    RiskNeutralAnchorMaterialization, VanillaOptionPricingIntervalFactors,
    VanillaOptionPricingRunError, build_spectral_compatibility_integration_diagnostics,
    certify_local_transition_theta_for_row_with_parallelism,
    differentiate_core_local_tilt_theta_for_tilt,
    differentiate_core_pure_imaginary_log_moment_exponent_for_tilt,
    evaluate_pricing_log_return_characteristic_function_batch_for_context,
    evaluate_pricing_log_return_characteristic_function_batch_shared_plus_shift_sensitivities_for_context,
    pricing_ratio_derivative, resolve_local_transition_theta_for_row_with_parallelism_and_hint,
};
pub(super) use crate::thread_pool::{install_pricing_parallelism, resolved_pricing_cpu_count};
pub(in crate::spectral::pricer) use execution::emit_pricing_progress;
pub(crate) use execution::run_single_option_pricing;
pub use execution::run_vanilla_option_grid_batch_pricing_with_observer_and_theta_warm_start_state;
pub(super) use janiform_foundations::complex_math::{
    ComplexScalar, complex_exponential, complex_multiply,
};
