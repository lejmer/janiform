//! Pricing engine contracts, transforms, and execution entrypoints.

mod black_scholes;
mod characteristic;
mod constants;
mod contracts;
mod forecast;
mod grids;
mod hawkes;
mod implied_volatility;
mod interval_schedule;
mod maturity_inputs;
mod pricing_kernel;
mod progress;
mod runner;
mod spectral;
mod thread_pool;
mod time_scaling;
mod vanilla_payoff;

pub use black_scholes::{
    black_scholes_log_return_characteristic_function, black_scholes_no_arbitrage_price_bounds,
    black_scholes_price, intraday_black_scholes_log_return_characteristic_function,
    intraday_black_scholes_no_arbitrage_price_bounds, intraday_black_scholes_price,
    intraday_discount_factor, intraday_forward_price,
};
pub use characteristic::{
    AnchoredLocalTiltSolveError, PricingTransformInputBuildError,
    ReturnCharacteristicFunctionAssembly, ReturnCharacteristicFunctionAssemblyError,
    ReturnCharacteristicFunctionFactorComposition, build_pricing_transform_input,
    compose_nig_trading_time_exponent, compose_nig_trading_time_exponent_with_edge_mode,
    compose_total_return_characteristic_function,
    compose_total_return_characteristic_function_from_oracle_output,
    compose_total_return_characteristic_function_with_edge_mode, solve_anchored_local_pricing_tilt,
};
pub use constants::{
    DEFAULT_INTRADAY_PRICING_INTERVAL_DELTA, INTRADAY_PRICING_CONTINUOUS_RATE_TIME_SCALING_POLICY,
    INTRADAY_PRICING_HOURS_PER_YEAR, PRICING_INTEGRATION_QUALITY_DIAGNOSTICS_SCHEMA_VERSION,
    PRICING_INTEGRATION_QUALITY_GATE_POLICY, PRICING_LOCAL_ANCHORED_TILT_SOLVE_POLICY,
    PRICING_MARTINGALE_CONDITION_VALIDATION_POLICY,
    RETURN_CHARACTERISTIC_FUNCTION_COMPOSITION_POLICY,
    RETURN_CHARACTERISTIC_FUNCTION_DOMAIN_GUARD_POLICY, SPECTRAL_PRICING_COLLOCATION_POLICY,
    SPECTRAL_PRICING_DEVIATION_DOMAIN_POLICY, SPECTRAL_PRICING_INTEGRATION_COMPATIBILITY_POLICY,
    SPECTRAL_PRICING_TERMINAL_AFFINE_POLICY, VANILLA_OPTION_CALIBRATION_HOOK_POLICY,
    VANILLA_OPTION_PAYOFF_CONFIGURATION_POLICY,
    VANILLA_OPTION_PRICING_CALIBRATION_HOOKS_SCHEMA_VERSION,
    VANILLA_OPTION_PRICING_RUN_DIAGNOSTICS_SCHEMA_VERSION,
    VANILLA_OPTION_PRICING_RUN_OUTPUT_SCHEMA_METADATA_VERSION,
    VANILLA_OPTION_PRICING_RUN_RESULT_SCHEMA_VERSION, VANILLA_OPTION_REQUEST_VALIDATION_POLICY,
    VANILLA_OPTION_RESPONSE_VALIDATION_POLICY,
};
pub use contracts::{
    AnchoredLocalTiltAdmissibilityStatus, AnchoredLocalTiltBoundaryStateSummary,
    AnchoredLocalTiltDomainStatus, AnchoredLocalTiltSolveDiagnostics,
    AnchoredVanillaOptionPricingBoundaryInput, AnchoredVanillaOptionPricingBoundaryInputError,
    PricingIntegrationNumericQualityDiagnostics, PricingIntegrationProjectionRule,
    PricingIntegrationQualityDiagnostics, PricingIntegrationQualityGate,
    SpectralPricingDiagnostics, SpectralPricingDiagnosticsError,
    SpectralPricingIntervalDiagnostics, TerminalAffineCumulantStatus,
    TerminalAffineMeasureChangeDiagnostics, TerminalAffineOptionSmileContributionDiagnostics,
    VANILLA_OPTION_PRICING_RUN_OUTPUT_SCHEMA_METADATA, VanillaOptionPayoffConfiguration,
    VanillaOptionPayoffType, VanillaOptionPricingCalibrationHooks,
    VanillaOptionPricingCalibrationHooksError, VanillaOptionPricingDiagnostics,
    VanillaOptionPricingDiagnosticsError, VanillaOptionPricingFailureDiagnostic,
    VanillaOptionPricingFailureDiagnosticError, VanillaOptionPricingFailureStage,
    VanillaOptionPricingMarketInputs, VanillaOptionPricingMarketInputsError,
    VanillaOptionPricingModelParameters, VanillaOptionPricingModelParametersError,
    VanillaOptionPricingOptionalOutputSelection, VanillaOptionPricingRequest,
    VanillaOptionPricingRequestError, VanillaOptionPricingResponse,
    VanillaOptionPricingResponseError, VanillaOptionPricingRunDiagnostics,
    VanillaOptionPricingRunDiagnosticsError, VanillaOptionPricingRunOutputSchemaMetadata,
    VanillaOptionPricingRunResult, VanillaOptionPricingRunResultBuildError,
    VanillaOptionPricingRunResultBuildInput, VanillaOptionPricingRunResultError,
    VanillaOptionPricingStatus, build_vanilla_option_pricing_run_result,
};
pub use forecast::{
    AffineForecastExpectedLogReturn, AffineForecastExpectedLogReturnInput,
    AffineForecastExpectedLogReturnProjector, AffineForecastExpectedLogReturnProjectorInput,
    AffineForecastMeasure, AffineForecastTerminalLaw, AffineForecastTerminalLawInput,
    solve_affine_forecast_expected_log_return, solve_affine_forecast_terminal_law,
};
pub use grids::{
    SpectralThetaWarmStartState, VanillaOptionGridBatchPricingExecutionInput,
    VanillaOptionGridBatchPricingExecutionInputError, VanillaOptionGridBatchPricingRequest,
    VanillaOptionGridBatchPricingRequestError, VanillaOptionGridBatchPricingRunError,
    VanillaOptionGridBatchPricingRunResult, VanillaOptionGridPricingResult,
    VanillaOptionGridRequest, VanillaOptionMaturitySummary, VanillaOptionNodePriceSensitivity,
    VanillaOptionSurfacePricingExecutionInput, VanillaOptionSurfacePricingExecutionInputError,
    VanillaOptionSurfacePricingRequest, VanillaOptionSurfacePricingResponse,
    VanillaOptionSurfacePricingRunError, VanillaOptionSurfacePricingRunResult,
    run_vanilla_option_grid_batch_pricing, run_vanilla_option_grid_batch_pricing_with_observer,
    run_vanilla_option_surface_pricing, run_vanilla_option_surface_pricing_with_observer,
};
pub use hawkes::project_hawkes_intensities_coarse_interval;
pub use implied_volatility::{
    VanillaOptionImpliedVolatilityInversionOutcome, VanillaOptionImpliedVolatilityInversionStatus,
    invert_vanilla_option_implied_volatility,
};
pub use interval_schedule::{
    DerivedPricingIntervalSchedule, PricingIntervalSchedule, PricingIntervalScheduleError,
    PricingIntervalScheduleMetadata, PricingIntervalSchedulePolicy,
    derive_activity_compressed_pricing_interval_schedule, derive_pricing_interval_schedule,
    materialize_schedule_interval_factor_grids,
    materialize_schedule_interval_factor_grids_with_cpu_count,
};
pub use janiform_market_time::interval_factor_grids::provide_interval_factor_grid as load_interval_factor_grid;
pub use janiform_model_kernels::hawkes::evolve_hawkes_state;
pub use maturity_inputs::{
    HighLevelPricingMaturityResolution, HighLevelPricingMaturityResolutionError,
    HighLevelPricingMaturityResolutionRequest, resolve_high_level_pricing_maturity_inputs,
};
pub use pricing_kernel::{
    VanillaOptionPricingKernelParameters, VanillaOptionPricingKernelParametersError,
};
pub use progress::{PricingProgressEvent, PricingProgressPhase, pricing_progress_phase_label};
pub use runner::{
    VanillaOptionPricingExecutionInput, VanillaOptionPricingExecutionInputError,
    VanillaOptionPricingIntervalFactors, VanillaOptionPricingIntervalFactorsError,
    VanillaOptionPricingRunError, run_vanilla_option_pricing,
    run_vanilla_option_pricing_with_observer,
};
pub use time_scaling::intraday_year_fraction;
pub use vanilla_payoff::vanilla_option_terminal_payoff;

#[doc(hidden)]
pub mod internal {
    pub use super::grids::SpectralThetaWarmStartState;
    pub use super::spectral::run_vanilla_option_grid_batch_pricing_with_observer_and_theta_warm_start_state;
}

#[cfg(test)]
mod tests;
