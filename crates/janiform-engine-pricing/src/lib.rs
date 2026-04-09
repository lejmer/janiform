//! Pricing-side hourly factor loading through the shared provider interface.

mod black_scholes;
mod characteristic;
mod constants;
mod contracts;
mod forward_law;
mod fourier;
mod grids;
mod implied_volatility;
mod interval_schedule;
mod maturity_inputs;
mod probability_projection;
mod progress;
mod progress_runtime;
mod runner;
mod thread_pool;
mod time_scaling;

pub use black_scholes::{
    black_scholes_log_return_characteristic_function, black_scholes_no_arbitrage_price_bounds,
    black_scholes_price, intraday_black_scholes_log_return_characteristic_function,
    intraday_black_scholes_no_arbitrage_price_bounds, intraday_black_scholes_price,
};
pub use characteristic::{
    AnchoredLocalTiltSolveError, PricingTransformInputBuildError,
    ReturnCharacteristicFunctionAssembly, ReturnCharacteristicFunctionAssemblyError,
    ReturnCharacteristicFunctionFactorComposition, build_pricing_transform_input,
    compose_nig_trading_time_exponent, compose_nig_trading_time_exponent_with_edge_mode,
    compose_total_return_characteristic_function,
    compose_total_return_characteristic_function_from_oracle_output,
    compose_total_return_characteristic_function_with_edge_mode, evolve_hawkes_state,
    load_hourly_factor_contract, project_hawkes_intensities_coarse_interval,
    solve_anchored_local_pricing_tilt, solve_exponential_affine_transform,
    solve_exponential_affine_transform_with_settings,
};
pub use constants::{
    ANCHORED_PRICING_CONTINUATION_RECURSION_POLICY, ANCHORED_PRICING_DEVIATION_GRID_POLICY,
    ANCHORED_PRICING_EXPECTED_HAWKES_STATE_UPDATE_POLICY, DEFAULT_INTRADAY_PRICING_INTERVAL_DELTA,
    FOURIER_INVERSION_CONTOUR_POLICY, FOURIER_INVERSION_CONVERGENCE_CHECK_POLICY,
    FOURIER_INVERSION_DIAGNOSTICS_SCHEMA_VERSION, FOURIER_INVERSION_ENGINE_POLICY,
    FOURIER_INVERSION_INTEGRATION_STABILITY_POLICY, FOURIER_INVERSION_NUMERIC_QUALITY_GATE_POLICY,
    FOURIER_INVERSION_NUMERIC_SETTINGS_POLICY, FOURIER_INVERSION_TRUNCATION_CHECK_POLICY,
    INTRADAY_PRICING_CONTINUOUS_RATE_TIME_SCALING_POLICY, INTRADAY_PRICING_HOURS_PER_YEAR,
    PRICING_LOCAL_ANCHORED_TILT_SOLVE_POLICY, PRICING_MARTINGALE_CONDITION_VALIDATION_POLICY,
    RETURN_CHARACTERISTIC_FUNCTION_COMPOSITION_POLICY,
    RETURN_CHARACTERISTIC_FUNCTION_DOMAIN_GUARD_POLICY, VANILLA_OPTION_CALIBRATION_HOOK_POLICY,
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
    AnchoredPricingRecursionDiagnostics, AnchoredPricingRecursionDiagnosticsError,
    AnchoredPricingRecursionIntervalDiagnostics, AnchoredVanillaOptionPricingBoundaryInput,
    AnchoredVanillaOptionPricingBoundaryInputError,
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
    VanillaOptionPricingRunResultError, VanillaOptionPricingStatus,
    build_vanilla_option_pricing_run_result,
};
pub use fourier::{
    FourierInversionContour, FourierInversionContourError, FourierInversionDiagnostics,
    FourierInversionEngineSettings, FourierInversionEngineSettingsError,
    FourierInversionIntegrationStabilityControls,
    FourierInversionIntegrationStabilityControlsError, FourierInversionNumericQualityDiagnostics,
    FourierInversionNumericQualityGate, FourierInversionNumericSettings,
    FourierInversionNumericSettingsError, FourierInversionPricingError,
    FourierInversionPricingResult, FourierInversionQuadratureRule,
    price_vanilla_option_via_fourier_inversion,
};
pub use grids::{
    VanillaOptionGridBatchPricingExecutionInput, VanillaOptionGridBatchPricingExecutionInputError,
    VanillaOptionGridBatchPricingRequest, VanillaOptionGridBatchPricingRequestError,
    VanillaOptionGridBatchPricingRunError, VanillaOptionGridBatchPricingRunResult,
    VanillaOptionGridPricingResult, VanillaOptionGridRequest, VanillaOptionMaturityIntervalSummary,
    VanillaOptionMaturityPricingEngineKind, VanillaOptionMaturitySummary,
    VanillaOptionSurfacePricingExecutionInput, VanillaOptionSurfacePricingExecutionInputError,
    VanillaOptionSurfacePricingRequest, VanillaOptionSurfacePricingResponse,
    VanillaOptionSurfacePricingRunError, VanillaOptionSurfacePricingRunResult,
    run_vanilla_option_grid_batch_pricing, run_vanilla_option_grid_batch_pricing_with_observer,
    run_vanilla_option_surface_pricing, run_vanilla_option_surface_pricing_with_observer,
};
pub use implied_volatility::VanillaOptionImpliedVolatilityInversionStatus;
pub use interval_schedule::{
    DerivedPricingIntervalSchedule, PricingIntervalSchedule, PricingIntervalScheduleError,
    PricingIntervalScheduleMetadata, PricingIntervalSchedulePolicy,
    derive_pricing_interval_schedule, materialize_schedule_interval_factors,
};
pub use maturity_inputs::{
    HighLevelPricingMaturityResolution, HighLevelPricingMaturityResolutionError,
    HighLevelPricingMaturityResolutionRequest, resolve_high_level_pricing_maturity_inputs,
};
pub use progress::{
    PricingNodeProgress, PricingNodeProgressPhase, PricingProgressEvent, PricingProgressPhase,
};
pub use runner::{
    VanillaOptionPricingExecutionInput, VanillaOptionPricingExecutionInputError,
    VanillaOptionPricingIntervalFactors, VanillaOptionPricingIntervalFactorsError,
    VanillaOptionPricingRunError, run_vanilla_option_pricing,
    run_vanilla_option_pricing_with_observer,
};

#[cfg(test)]
mod tests;
