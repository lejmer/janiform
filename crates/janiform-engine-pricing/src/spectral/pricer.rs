mod domain;
mod execution;
mod maturity_law;
mod payoff;
mod strike;
mod terminal_affine;

pub(super) use super::coordinates::asset_reference_log_price_from_observed_spot;
pub(super) use super::sensitivity::{
    NodePriceSharedPlusShiftComplexSensitivity, NodePriceSharedPlusShiftSensitivity,
};
pub(super) use crate::constants::MINIMUM_SPECTRAL_COSINE_COLLOCATION_NODE_COUNT;
pub(super) use crate::contracts::{
    SpectralPricingDiagnostics, SpectralPricingIntervalDiagnostics, TerminalAffineCumulantStatus,
    TerminalAffineMeasureChangeDiagnostics, TerminalAffineOptionSmileContributionDiagnostics,
    VanillaOptionPayoffType, VanillaOptionPricingFailureStage, VanillaOptionPricingMarketInputs,
    VanillaOptionPricingOptionalOutputSelection, VanillaOptionPricingRequest,
    VanillaOptionPricingStatus,
};
pub(super) use crate::grids::{
    SpectralThetaWarmStartKey, SpectralThetaWarmStartState, VanillaOptionGridBatchPricingRunError,
    VanillaOptionGridRequest, VanillaOptionMaturitySummary, VanillaOptionNodePriceSensitivity,
};
pub(super) use crate::implied_volatility::{
    VanillaOptionImpliedVolatilityInversionOutcome, VanillaOptionImpliedVolatilityInversionStatus,
    invert_vanilla_option_implied_volatility,
};
pub(super) use crate::intraday_black_scholes_no_arbitrage_price_bounds;
pub(super) use crate::progress::PricingProgressEvent;
pub(super) use crate::runner::{
    VanillaOptionPricingIntervalFactors, VanillaOptionPricingRunError,
    build_spectral_compatibility_integration_diagnostics,
};
pub(super) use crate::thread_pool::{install_pricing_parallelism, resolved_pricing_cpu_count};
pub(in crate::spectral::pricer) use execution::emit_pricing_progress;
pub(crate) use execution::run_single_option_pricing;
pub use execution::run_vanilla_option_grid_batch_pricing_with_observer_and_theta_warm_start_state;
pub(super) use janiform_foundations::complex_math::{
    ComplexScalar, complex_exponential, complex_multiply,
};
