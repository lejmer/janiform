//! Pricing-side hourly factor loading through the shared provider interface.

use std::error::Error;
use std::fmt::{Display, Formatter};

use serde::Deserialize;
use serde::Serialize;

use crate::affine_oracle::{
    ExponentialAffineTransformInput, ExponentialAffineTransformIntegrationResult,
    ExponentialAffineTransformSolverError, ExponentialAffineTransformSolverSettings,
    solve_exponential_affine_transform_with_optional_settings,
};
use crate::calendar::ExchangeCalendarRegistry;
use crate::complex_math::{complex_exponential, complex_multiply};
use crate::hawkes::{
    HawkesExcitationState, HawkesIntensityProjection, HawkesKernelParameterError,
    HawkesKernelParameters, HawkesSeasonalityIntervalError, HawkesTransitionError,
    HawkesTransitionStep,
};
use crate::hourly_factor_provider::{
    HourlyFactorProviderError, HourlyFactorProviderRequest, provide_hourly_factor_contract,
};
use crate::hourly_factors::HourlyFactorContractOutput;
use crate::news::{GammaSeverityMarkConfiguration, NewsMarkConfigurationError};
use crate::nig::{
    ComplexScalar, NigBuySellDriverParameters, NigSharedPlusShiftConfiguration,
    NigSharedPlusShiftConfigurationError, NigTradingTimeCharacteristicExponentContributions,
};
use crate::return_decomposition::{
    ReturnDecomposition, ReturnDecompositionEdgeMode, ReturnDecompositionValidationError,
};

pub const RETURN_CHARACTERISTIC_FUNCTION_COMPOSITION_POLICY: &str =
    "exp_of_trading_time_weighted_exponent_times_explicit_gap_characteristic_factor.v1";
pub const RETURN_CHARACTERISTIC_FUNCTION_DOMAIN_GUARD_POLICY: &str = "finite_argument_and_gap_factor_with_valid_return_decomposition_and_finite_composition_result.v1";
pub const VANILLA_OPTION_PAYOFF_CONFIGURATION_POLICY: &str =
    "baseline_european_vanilla_call_and_put_payoff_type_selection_only.v1";
pub const VANILLA_OPTION_REQUEST_VALIDATION_POLICY: &str =
    "validate_market_inputs_boundary_state_and_model_parameter_domains_before_engine_dispatch.v1";
pub const VANILLA_OPTION_RESPONSE_VALIDATION_POLICY: &str =
    "validate_non_negative_price_and_finite_diagnostics_payload_before_response_publication.v1";

/// Baseline vanilla payoff kind supported for v0.1 pricing workflows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VanillaOptionPayoffType {
    Call,
    Put,
}

/// Public payoff selection payload for vanilla option pricing requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VanillaOptionPayoffConfiguration {
    pub payoff_type: VanillaOptionPayoffType,
}

impl VanillaOptionPayoffConfiguration {
    pub const fn policy() -> &'static str {
        VANILLA_OPTION_PAYOFF_CONFIGURATION_POLICY
    }
}

/// Public market-input payload for vanilla option pricing.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct VanillaOptionPricingMarketInputs {
    pub spot: f64,
    pub strike: f64,
    pub maturity_interval: f64,
    pub risk_free_rate: f64,
    pub dividend_yield: f64,
}

impl VanillaOptionPricingMarketInputs {
    pub const fn new(
        spot: f64,
        strike: f64,
        maturity_interval: f64,
        risk_free_rate: f64,
        dividend_yield: f64,
    ) -> Self {
        Self {
            spot,
            strike,
            maturity_interval,
            risk_free_rate,
            dividend_yield,
        }
    }

    pub fn try_new(
        spot: f64,
        strike: f64,
        maturity_interval: f64,
        risk_free_rate: f64,
        dividend_yield: f64,
    ) -> Result<Self, VanillaOptionPricingMarketInputsError> {
        let market_inputs = Self::new(
            spot,
            strike,
            maturity_interval,
            risk_free_rate,
            dividend_yield,
        );
        market_inputs.validate()?;
        Ok(market_inputs)
    }

    pub fn validate(self) -> Result<(), VanillaOptionPricingMarketInputsError> {
        if !self.spot.is_finite() || self.spot <= 0.0 {
            return Err(VanillaOptionPricingMarketInputsError::InvalidSpot { value: self.spot });
        }
        if !self.strike.is_finite() || self.strike <= 0.0 {
            return Err(VanillaOptionPricingMarketInputsError::InvalidStrike {
                value: self.strike,
            });
        }
        if !self.maturity_interval.is_finite() || self.maturity_interval <= 0.0 {
            return Err(
                VanillaOptionPricingMarketInputsError::InvalidMaturityInterval {
                    value: self.maturity_interval,
                },
            );
        }
        if !self.risk_free_rate.is_finite() {
            return Err(VanillaOptionPricingMarketInputsError::InvalidRiskFreeRate {
                value: self.risk_free_rate,
            });
        }
        if !self.dividend_yield.is_finite() {
            return Err(
                VanillaOptionPricingMarketInputsError::InvalidDividendYield {
                    value: self.dividend_yield,
                },
            );
        }

        Ok(())
    }
}

/// Validation errors for public vanilla option market inputs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum VanillaOptionPricingMarketInputsError {
    InvalidSpot { value: f64 },
    InvalidStrike { value: f64 },
    InvalidMaturityInterval { value: f64 },
    InvalidRiskFreeRate { value: f64 },
    InvalidDividendYield { value: f64 },
}

impl Display for VanillaOptionPricingMarketInputsError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidSpot { value } => write!(
                formatter,
                "spot must be finite and strictly positive, found {value}"
            ),
            Self::InvalidStrike { value } => write!(
                formatter,
                "strike must be finite and strictly positive, found {value}"
            ),
            Self::InvalidMaturityInterval { value } => write!(
                formatter,
                "maturity_interval must be finite and strictly positive, found {value}"
            ),
            Self::InvalidRiskFreeRate { value } => {
                write!(formatter, "risk_free_rate must be finite, found {value}")
            }
            Self::InvalidDividendYield { value } => {
                write!(formatter, "dividend_yield must be finite, found {value}")
            }
        }
    }
}

impl Error for VanillaOptionPricingMarketInputsError {}

/// Public model-parameter payload for baseline vanilla option pricing.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct VanillaOptionPricingModelParameters {
    pub hawkes_kernel_parameters: HawkesKernelParameters,
    pub gamma_severity_mark_configuration: GammaSeverityMarkConfiguration,
    pub nig_shared_plus_shift_configuration: NigSharedPlusShiftConfiguration,
}

impl VanillaOptionPricingModelParameters {
    pub const fn new(
        hawkes_kernel_parameters: HawkesKernelParameters,
        gamma_severity_mark_configuration: GammaSeverityMarkConfiguration,
        nig_shared_plus_shift_configuration: NigSharedPlusShiftConfiguration,
    ) -> Self {
        Self {
            hawkes_kernel_parameters,
            gamma_severity_mark_configuration,
            nig_shared_plus_shift_configuration,
        }
    }

    pub fn try_new(
        hawkes_kernel_parameters: HawkesKernelParameters,
        gamma_severity_mark_configuration: GammaSeverityMarkConfiguration,
        nig_shared_plus_shift_configuration: NigSharedPlusShiftConfiguration,
    ) -> Result<Self, VanillaOptionPricingModelParametersError> {
        let model_parameters = Self::new(
            hawkes_kernel_parameters,
            gamma_severity_mark_configuration,
            nig_shared_plus_shift_configuration,
        );
        model_parameters.validate()?;
        Ok(model_parameters)
    }

    pub fn validate(self) -> Result<(), VanillaOptionPricingModelParametersError> {
        HawkesKernelParameters::try_new(
            self.hawkes_kernel_parameters.lambda_bar,
            self.hawkes_kernel_parameters.beta,
            self.hawkes_kernel_parameters.alpha,
            self.hawkes_kernel_parameters.rho,
        )
        .map_err(|source| {
            VanillaOptionPricingModelParametersError::InvalidHawkesKernelParameters { source }
        })?;

        self.gamma_severity_mark_configuration
            .validate()
            .map_err(|source| {
                VanillaOptionPricingModelParametersError::InvalidGammaSeverityMarkConfiguration {
                    source,
                }
            })?;

        self.nig_shared_plus_shift_configuration
            .derive_driver_parameters()
            .map_err(|source| {
                VanillaOptionPricingModelParametersError::InvalidNigSharedPlusShiftConfiguration {
                    source,
                }
            })?;

        Ok(())
    }
}

/// Validation errors for public model-parameter payloads.
#[derive(Debug, Clone, PartialEq)]
pub enum VanillaOptionPricingModelParametersError {
    InvalidHawkesKernelParameters {
        source: HawkesKernelParameterError,
    },
    InvalidGammaSeverityMarkConfiguration {
        source: NewsMarkConfigurationError,
    },
    InvalidNigSharedPlusShiftConfiguration {
        source: NigSharedPlusShiftConfigurationError,
    },
}

impl Display for VanillaOptionPricingModelParametersError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidHawkesKernelParameters { source } => write!(
                formatter,
                "model_parameters.hawkes_kernel_parameters is invalid: {source}"
            ),
            Self::InvalidGammaSeverityMarkConfiguration { source } => write!(
                formatter,
                "model_parameters.gamma_severity_mark_configuration is invalid: {source}"
            ),
            Self::InvalidNigSharedPlusShiftConfiguration { source } => write!(
                formatter,
                "model_parameters.nig_shared_plus_shift_configuration is invalid: {source}"
            ),
        }
    }
}

impl Error for VanillaOptionPricingModelParametersError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidHawkesKernelParameters { source } => Some(source),
            Self::InvalidGammaSeverityMarkConfiguration { source } => Some(source),
            Self::InvalidNigSharedPlusShiftConfiguration { source } => Some(source),
        }
    }
}

/// Public request contract for baseline vanilla option pricing.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct VanillaOptionPricingRequest {
    pub market_inputs: VanillaOptionPricingMarketInputs,
    pub payoff: VanillaOptionPayoffConfiguration,
    pub boundary_state: HawkesExcitationState,
    pub model_parameters: VanillaOptionPricingModelParameters,
}

impl VanillaOptionPricingRequest {
    pub const fn new(
        market_inputs: VanillaOptionPricingMarketInputs,
        payoff: VanillaOptionPayoffConfiguration,
        boundary_state: HawkesExcitationState,
        model_parameters: VanillaOptionPricingModelParameters,
    ) -> Self {
        Self {
            market_inputs,
            payoff,
            boundary_state,
            model_parameters,
        }
    }

    pub fn try_new(
        market_inputs: VanillaOptionPricingMarketInputs,
        payoff: VanillaOptionPayoffConfiguration,
        boundary_state: HawkesExcitationState,
        model_parameters: VanillaOptionPricingModelParameters,
    ) -> Result<Self, VanillaOptionPricingRequestError> {
        let request = Self::new(market_inputs, payoff, boundary_state, model_parameters);
        request.validate()?;
        Ok(request)
    }

    pub fn validate(self) -> Result<(), VanillaOptionPricingRequestError> {
        self.market_inputs
            .validate()
            .map_err(|source| VanillaOptionPricingRequestError::InvalidMarketInputs { source })?;

        validate_boundary_state(self.boundary_state)?;

        self.model_parameters.validate().map_err(|source| {
            VanillaOptionPricingRequestError::InvalidModelParameters { source }
        })?;

        Ok(())
    }

    pub const fn validation_policy() -> &'static str {
        VANILLA_OPTION_REQUEST_VALIDATION_POLICY
    }
}

/// Validation errors for vanilla option pricing requests.
#[derive(Debug, Clone, PartialEq)]
pub enum VanillaOptionPricingRequestError {
    InvalidMarketInputs {
        source: VanillaOptionPricingMarketInputsError,
    },
    InvalidBoundaryStateComponent {
        field: &'static str,
        value: f64,
    },
    InvalidModelParameters {
        source: VanillaOptionPricingModelParametersError,
    },
}

impl Display for VanillaOptionPricingRequestError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidMarketInputs { source } => {
                write!(formatter, "market_inputs payload is invalid: {source}")
            }
            Self::InvalidBoundaryStateComponent { field, value } => write!(
                formatter,
                "boundary_state.{field} must be finite and non-negative, found {value}"
            ),
            Self::InvalidModelParameters { source } => {
                write!(formatter, "model_parameters payload is invalid: {source}")
            }
        }
    }
}

impl Error for VanillaOptionPricingRequestError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidMarketInputs { source } => Some(source),
            Self::InvalidModelParameters { source } => Some(source),
            _ => None,
        }
    }
}

/// Public diagnostics payload returned with vanilla option prices.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct VanillaOptionPricingDiagnostics {
    pub forward_price: f64,
    pub discount_factor: f64,
    pub maturity_interval: f64,
    pub boundary_state_norm: f64,
}

impl VanillaOptionPricingDiagnostics {
    pub const fn new(
        forward_price: f64,
        discount_factor: f64,
        maturity_interval: f64,
        boundary_state_norm: f64,
    ) -> Self {
        Self {
            forward_price,
            discount_factor,
            maturity_interval,
            boundary_state_norm,
        }
    }

    pub fn try_new(
        forward_price: f64,
        discount_factor: f64,
        maturity_interval: f64,
        boundary_state_norm: f64,
    ) -> Result<Self, VanillaOptionPricingDiagnosticsError> {
        let diagnostics = Self::new(
            forward_price,
            discount_factor,
            maturity_interval,
            boundary_state_norm,
        );
        diagnostics.validate()?;
        Ok(diagnostics)
    }

    pub fn validate(self) -> Result<(), VanillaOptionPricingDiagnosticsError> {
        if !self.forward_price.is_finite() || self.forward_price <= 0.0 {
            return Err(VanillaOptionPricingDiagnosticsError::InvalidForwardPrice {
                value: self.forward_price,
            });
        }
        if !self.discount_factor.is_finite() || self.discount_factor <= 0.0 {
            return Err(
                VanillaOptionPricingDiagnosticsError::InvalidDiscountFactor {
                    value: self.discount_factor,
                },
            );
        }
        if !self.maturity_interval.is_finite() || self.maturity_interval <= 0.0 {
            return Err(
                VanillaOptionPricingDiagnosticsError::InvalidMaturityInterval {
                    value: self.maturity_interval,
                },
            );
        }
        if !self.boundary_state_norm.is_finite() || self.boundary_state_norm < 0.0 {
            return Err(
                VanillaOptionPricingDiagnosticsError::InvalidBoundaryStateNorm {
                    value: self.boundary_state_norm,
                },
            );
        }

        Ok(())
    }
}

/// Validation errors for vanilla option diagnostics payloads.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum VanillaOptionPricingDiagnosticsError {
    InvalidForwardPrice { value: f64 },
    InvalidDiscountFactor { value: f64 },
    InvalidMaturityInterval { value: f64 },
    InvalidBoundaryStateNorm { value: f64 },
}

impl Display for VanillaOptionPricingDiagnosticsError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidForwardPrice { value } => write!(
                formatter,
                "diagnostics.forward_price must be finite and strictly positive, found {value}"
            ),
            Self::InvalidDiscountFactor { value } => write!(
                formatter,
                "diagnostics.discount_factor must be finite and strictly positive, found {value}"
            ),
            Self::InvalidMaturityInterval { value } => write!(
                formatter,
                "diagnostics.maturity_interval must be finite and strictly positive, found {value}"
            ),
            Self::InvalidBoundaryStateNorm { value } => write!(
                formatter,
                "diagnostics.boundary_state_norm must be finite and non-negative, found {value}"
            ),
        }
    }
}

impl Error for VanillaOptionPricingDiagnosticsError {}

/// Public response contract for baseline vanilla option pricing.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct VanillaOptionPricingResponse {
    pub price: f64,
    pub diagnostics: VanillaOptionPricingDiagnostics,
}

impl VanillaOptionPricingResponse {
    pub const fn new(price: f64, diagnostics: VanillaOptionPricingDiagnostics) -> Self {
        Self { price, diagnostics }
    }

    pub fn try_new(
        price: f64,
        diagnostics: VanillaOptionPricingDiagnostics,
    ) -> Result<Self, VanillaOptionPricingResponseError> {
        let response = Self::new(price, diagnostics);
        response.validate()?;
        Ok(response)
    }

    pub fn validate(self) -> Result<(), VanillaOptionPricingResponseError> {
        if !self.price.is_finite() || self.price < 0.0 {
            return Err(VanillaOptionPricingResponseError::InvalidPrice { value: self.price });
        }

        self.diagnostics
            .validate()
            .map_err(|source| VanillaOptionPricingResponseError::InvalidDiagnostics { source })?;

        Ok(())
    }

    pub const fn validation_policy() -> &'static str {
        VANILLA_OPTION_RESPONSE_VALIDATION_POLICY
    }
}

/// Validation errors for vanilla option pricing responses.
#[derive(Debug, Clone, PartialEq)]
pub enum VanillaOptionPricingResponseError {
    InvalidPrice {
        value: f64,
    },
    InvalidDiagnostics {
        source: VanillaOptionPricingDiagnosticsError,
    },
}

impl Display for VanillaOptionPricingResponseError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidPrice { value } => write!(
                formatter,
                "price must be finite and non-negative, found {value}"
            ),
            Self::InvalidDiagnostics { source } => {
                write!(formatter, "diagnostics payload is invalid: {source}")
            }
        }
    }
}

impl Error for VanillaOptionPricingResponseError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidDiagnostics { source } => Some(source),
            _ => None,
        }
    }
}

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

/// Solve one-interval exponential-affine transform for pricing workflows.
///
/// This adapter keeps the pricing layer conversion-focused and delegates to the
/// shared solver entrypoint in `affine_oracle`.
pub fn solve_exponential_affine_transform_for_pricing(
    transform_input: ExponentialAffineTransformInput,
) -> Result<ExponentialAffineTransformIntegrationResult, ExponentialAffineTransformSolverError> {
    solve_exponential_affine_transform_with_optional_settings(transform_input, None)
}

/// Solve one-interval exponential-affine transform for pricing workflows with explicit settings.
pub fn solve_exponential_affine_transform_for_pricing_with_settings(
    transform_input: ExponentialAffineTransformInput,
    solver_settings: ExponentialAffineTransformSolverSettings,
) -> Result<ExponentialAffineTransformIntegrationResult, ExponentialAffineTransformSolverError> {
    solve_exponential_affine_transform_with_optional_settings(
        transform_input,
        Some(solver_settings),
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

fn validate_boundary_state(
    boundary_state: HawkesExcitationState,
) -> Result<(), VanillaOptionPricingRequestError> {
    validate_boundary_state_component("y_plus_plus", boundary_state.y_plus_plus)?;
    validate_boundary_state_component("y_plus_minus", boundary_state.y_plus_minus)?;
    validate_boundary_state_component("y_minus_plus", boundary_state.y_minus_plus)?;
    validate_boundary_state_component("y_minus_minus", boundary_state.y_minus_minus)?;

    Ok(())
}

fn validate_boundary_state_component(
    field: &'static str,
    value: f64,
) -> Result<(), VanillaOptionPricingRequestError> {
    if !value.is_finite() || value < 0.0 {
        return Err(
            VanillaOptionPricingRequestError::InvalidBoundaryStateComponent { field, value },
        );
    }

    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct VanillaOptionPricingMarketInputsPayload {
    spot: f64,
    strike: f64,
    maturity_interval: f64,
    risk_free_rate: f64,
    dividend_yield: f64,
}

impl<'de> Deserialize<'de> for VanillaOptionPricingMarketInputs {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let payload = VanillaOptionPricingMarketInputsPayload::deserialize(deserializer)?;
        Self::try_new(
            payload.spot,
            payload.strike,
            payload.maturity_interval,
            payload.risk_free_rate,
            payload.dividend_yield,
        )
        .map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct VanillaOptionPricingModelParametersPayload {
    hawkes_kernel_parameters: HawkesKernelParameters,
    gamma_severity_mark_configuration: GammaSeverityMarkConfiguration,
    nig_shared_plus_shift_configuration: NigSharedPlusShiftConfiguration,
}

impl<'de> Deserialize<'de> for VanillaOptionPricingModelParameters {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let payload = VanillaOptionPricingModelParametersPayload::deserialize(deserializer)?;
        Self::try_new(
            payload.hawkes_kernel_parameters,
            payload.gamma_severity_mark_configuration,
            payload.nig_shared_plus_shift_configuration,
        )
        .map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct VanillaOptionPricingRequestPayload {
    market_inputs: VanillaOptionPricingMarketInputs,
    payoff: VanillaOptionPayoffConfiguration,
    boundary_state: HawkesExcitationState,
    model_parameters: VanillaOptionPricingModelParameters,
}

impl<'de> Deserialize<'de> for VanillaOptionPricingRequest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let payload = VanillaOptionPricingRequestPayload::deserialize(deserializer)?;
        Self::try_new(
            payload.market_inputs,
            payload.payoff,
            payload.boundary_state,
            payload.model_parameters,
        )
        .map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct VanillaOptionPricingDiagnosticsPayload {
    forward_price: f64,
    discount_factor: f64,
    maturity_interval: f64,
    boundary_state_norm: f64,
}

impl<'de> Deserialize<'de> for VanillaOptionPricingDiagnostics {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let payload = VanillaOptionPricingDiagnosticsPayload::deserialize(deserializer)?;
        Self::try_new(
            payload.forward_price,
            payload.discount_factor,
            payload.maturity_interval,
            payload.boundary_state_norm,
        )
        .map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct VanillaOptionPricingResponsePayload {
    price: f64,
    diagnostics: VanillaOptionPricingDiagnostics,
}

impl<'de> Deserialize<'de> for VanillaOptionPricingResponse {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let payload = VanillaOptionPricingResponsePayload::deserialize(deserializer)?;
        Self::try_new(payload.price, payload.diagnostics).map_err(serde::de::Error::custom)
    }
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

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::hawkes::{HawkesExcitationState, HawkesKernelLoadings, HawkesKernelParameters};
    use crate::news::GammaSeverityMarkConfiguration;
    use crate::nig::{ComplexScalar, NigSharedPlusShiftConfiguration};
    use crate::pricing::{
        RETURN_CHARACTERISTIC_FUNCTION_COMPOSITION_POLICY,
        RETURN_CHARACTERISTIC_FUNCTION_DOMAIN_GUARD_POLICY,
        ReturnCharacteristicFunctionAssemblyError, VanillaOptionPayoffConfiguration,
        VanillaOptionPayoffType, VanillaOptionPricingDiagnostics,
        VanillaOptionPricingDiagnosticsError, VanillaOptionPricingMarketInputs,
        VanillaOptionPricingMarketInputsError, VanillaOptionPricingModelParameters,
        VanillaOptionPricingRequest, VanillaOptionPricingRequestError,
        VanillaOptionPricingResponse, VanillaOptionPricingResponseError,
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

    #[test]
    fn vanilla_option_request_roundtrip_serialization_preserves_contract_fields() {
        let request = build_valid_vanilla_option_request();

        let serialized_value =
            serde_json::to_value(request).expect("request payload should serialize");
        let parsed_request: VanillaOptionPricingRequest = serde_json::from_value(serialized_value)
            .expect("serialized request payload should deserialize");

        assert_eq!(parsed_request, request);
    }

    #[test]
    fn vanilla_option_response_roundtrip_serialization_preserves_contract_fields() {
        let response = VanillaOptionPricingResponse::try_new(
            12.4,
            VanillaOptionPricingDiagnostics::try_new(104.8, 0.97, 0.5, 0.4)
                .expect("diagnostics should construct"),
        )
        .expect("response should construct");

        let serialized_value =
            serde_json::to_value(response).expect("response payload should serialize");
        let parsed_response: VanillaOptionPricingResponse =
            serde_json::from_value(serialized_value)
                .expect("serialized response payload should deserialize");

        assert_eq!(parsed_response, response);
    }

    #[test]
    fn vanilla_option_request_rejects_invalid_market_input_with_structured_error() {
        let market_inputs = VanillaOptionPricingMarketInputs::new(100.0, -90.0, 0.5, 0.02, 0.01);

        let error = VanillaOptionPricingRequest::try_new(
            market_inputs,
            VanillaOptionPayoffConfiguration {
                payoff_type: VanillaOptionPayoffType::Call,
            },
            HawkesExcitationState {
                y_plus_plus: 0.2,
                y_plus_minus: 0.1,
                y_minus_plus: 0.3,
                y_minus_minus: 0.4,
            },
            valid_model_parameters(),
        )
        .expect_err("negative strike should fail request validation");

        assert!(matches!(
            error,
            VanillaOptionPricingRequestError::InvalidMarketInputs {
                source: VanillaOptionPricingMarketInputsError::InvalidStrike { value }
            } if value == -90.0
        ));
    }

    #[test]
    fn vanilla_option_request_deserialize_rejects_invalid_boundary_state_with_actionable_error() {
        let invalid_payload = json!({
            "market_inputs": {
                "spot": 101.0,
                "strike": 100.0,
                "maturity_interval": 0.5,
                "risk_free_rate": 0.02,
                "dividend_yield": 0.01
            },
            "payoff": {
                "payoff_type": "put"
            },
            "boundary_state": {
                "y_plus_plus": -0.1,
                "y_plus_minus": 0.2,
                "y_minus_plus": 0.3,
                "y_minus_minus": 0.4
            },
            "model_parameters": {
                "hawkes_kernel_parameters": {
                    "lambda_bar": 0.8,
                    "beta": 1.6,
                    "alpha": {
                        "plus_plus": 0.2,
                        "plus_minus": 0.1,
                        "minus_plus": 0.1,
                        "minus_minus": 0.25
                    },
                    "rho": {
                        "plus_plus": 0.1,
                        "plus_minus": 0.05,
                        "minus_plus": 0.05,
                        "minus_minus": 0.1
                    }
                },
                "gamma_severity_mark_configuration": {
                    "shape_k": 1.8,
                    "scale_theta": 0.7
                },
                "nig_shared_plus_shift_configuration": {
                    "alpha": 2.3,
                    "beta_0": -0.8,
                    "delta": 0.7,
                    "mu": 0.1,
                    "delta_beta": -0.2
                }
            }
        });

        let error = serde_json::from_value::<VanillaOptionPricingRequest>(invalid_payload)
            .expect_err("invalid boundary state should fail request deserialization");
        let error_message = error.to_string();

        assert!(error_message.contains("boundary_state.y_plus_plus"));
        assert!(error_message.contains("non-negative"));
    }

    #[test]
    fn vanilla_option_request_deserialize_rejects_incomplete_payload() {
        let incomplete_payload = json!({
            "market_inputs": {
                "spot": 101.0,
                "maturity_interval": 0.5,
                "risk_free_rate": 0.02,
                "dividend_yield": 0.01
            },
            "payoff": {
                "payoff_type": "call"
            },
            "boundary_state": {
                "y_plus_plus": 0.1,
                "y_plus_minus": 0.2,
                "y_minus_plus": 0.3,
                "y_minus_minus": 0.4
            },
            "model_parameters": {
                "hawkes_kernel_parameters": {
                    "lambda_bar": 0.8,
                    "beta": 1.6,
                    "alpha": {
                        "plus_plus": 0.2,
                        "plus_minus": 0.1,
                        "minus_plus": 0.1,
                        "minus_minus": 0.25
                    },
                    "rho": {
                        "plus_plus": 0.1,
                        "plus_minus": 0.05,
                        "minus_plus": 0.05,
                        "minus_minus": 0.1
                    }
                },
                "gamma_severity_mark_configuration": {
                    "shape_k": 1.8,
                    "scale_theta": 0.7
                },
                "nig_shared_plus_shift_configuration": {
                    "alpha": 2.3,
                    "beta_0": -0.8,
                    "delta": 0.7,
                    "mu": 0.1,
                    "delta_beta": -0.2
                }
            }
        });

        let error = serde_json::from_value::<VanillaOptionPricingRequest>(incomplete_payload)
            .expect_err("incomplete request payload should fail deserialization");
        let error_message = error.to_string();
        assert!(error_message.contains("missing field `strike`"));
    }

    #[test]
    fn vanilla_option_response_rejects_invalid_price_with_structured_error() {
        let diagnostics = VanillaOptionPricingDiagnostics::try_new(104.8, 0.97, 0.5, 0.4)
            .expect("diagnostics should construct");

        let error = VanillaOptionPricingResponse::try_new(-0.01, diagnostics)
            .expect_err("negative option prices must fail response validation");

        assert!(matches!(
            error,
            VanillaOptionPricingResponseError::InvalidPrice { value } if value == -0.01
        ));
    }

    #[test]
    fn vanilla_option_response_rejects_invalid_diagnostics_with_structured_error() {
        let invalid_diagnostics = VanillaOptionPricingDiagnostics::new(0.0, 0.97, 0.5, 0.4);

        let error = VanillaOptionPricingResponse::try_new(1.0, invalid_diagnostics)
            .expect_err("invalid diagnostics must fail response validation");

        assert!(matches!(
            error,
            VanillaOptionPricingResponseError::InvalidDiagnostics {
                source: VanillaOptionPricingDiagnosticsError::InvalidForwardPrice { value }
            } if value == 0.0
        ));
    }

    fn build_valid_vanilla_option_request() -> VanillaOptionPricingRequest {
        VanillaOptionPricingRequest::try_new(
            VanillaOptionPricingMarketInputs::try_new(101.0, 100.0, 0.5, 0.02, 0.01)
                .expect("market inputs should construct"),
            VanillaOptionPayoffConfiguration {
                payoff_type: VanillaOptionPayoffType::Call,
            },
            HawkesExcitationState {
                y_plus_plus: 0.2,
                y_plus_minus: 0.1,
                y_minus_plus: 0.3,
                y_minus_minus: 0.4,
            },
            valid_model_parameters(),
        )
        .expect("request should construct")
    }

    fn valid_model_parameters() -> VanillaOptionPricingModelParameters {
        VanillaOptionPricingModelParameters::try_new(
            HawkesKernelParameters::try_new(
                0.8,
                1.6,
                HawkesKernelLoadings {
                    plus_plus: 0.2,
                    plus_minus: 0.1,
                    minus_plus: 0.1,
                    minus_minus: 0.25,
                },
                HawkesKernelLoadings {
                    plus_plus: 0.1,
                    plus_minus: 0.05,
                    minus_plus: 0.05,
                    minus_minus: 0.1,
                },
            )
            .expect("kernel parameters should construct"),
            GammaSeverityMarkConfiguration::try_new(1.8, 0.7)
                .expect("gamma severity mark configuration should construct"),
            NigSharedPlusShiftConfiguration::try_new(2.3, -0.8, 0.7, 0.1, -0.2)
                .expect("nig shared-plus-shift configuration should construct"),
        )
        .expect("model parameters should construct")
    }

    fn assert_complex_close(observed: ComplexScalar, expected: ComplexScalar, tolerance: f64) {
        let difference = observed.subtract(expected).norm();
        assert!(
            difference <= tolerance,
            "difference {difference} exceeded tolerance {tolerance} for observed {observed:?} and expected {expected:?}"
        );
    }
}
