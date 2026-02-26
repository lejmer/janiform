//! Pricing-side hourly factor loading through the shared provider interface.

use std::error::Error;
use std::f64::consts::PI;
use std::fmt::{Display, Formatter};

use serde::Deserialize;
use serde::Serialize;

use crate::affine_oracle::{
    ExponentialAffineTransformInput, ExponentialAffineTransformIntegrationResult,
    ExponentialAffineTransformOutput, ExponentialAffineTransformOutputError,
    ExponentialAffineTransformSolverError, ExponentialAffineTransformSolverSettings,
    solve_exponential_affine_transform_with_optional_settings,
};
use crate::complex_math::{complex_exponential, complex_multiply};
use crate::hawkes::{
    HawkesExcitationState, HawkesIntensityProjection, HawkesKernelParameterError,
    HawkesKernelParameters, HawkesSeasonalityIntervalError,
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
use crate::risk_neutral::{
    EsscherMartingaleConditionError, EsscherMartingaleConditionInput,
    EsscherMartingaleConditionReport, EsscherTransformError, NigEsscherTransformationInput,
    evaluate_esscher_martingale_condition,
    transform_nig_characteristic_exponent_under_esscher_tilt,
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
pub const PRICING_MARTINGALE_CONDITION_VALIDATION_POLICY: &str = "trading_time_log_moment_under_esscher_tilt_must_match_r_minus_q_maturity_target_within_tolerance.v1";
pub const FOURIER_INVERSION_ENGINE_POLICY: &str =
    "carr_madan_damped_call_inversion_with_put_call_parity_extension.v1";
pub const FOURIER_INVERSION_CONTOUR_POLICY: &str =
    "single_damping_factor_contour_shift_with_positive_alpha_requirement.v1";
pub const FOURIER_INVERSION_NUMERIC_SETTINGS_POLICY: &str =
    "bounded_frequency_quadrature_with_explicit_rule_selection_and_step_count.v1";
pub const FOURIER_INVERSION_INTEGRATION_STABILITY_POLICY: &str =
    "interval_tolerance_and_tail_truncation_quality_controls_with_tunable_quality_gate.v1";
pub const FOURIER_INVERSION_CONVERGENCE_CHECK_POLICY: &str =
    "half_resolution_integral_agreement_with_absolute_and_relative_tolerance.v1";
pub const FOURIER_INVERSION_TRUNCATION_CHECK_POLICY: &str =
    "terminal_integrand_absolute_and_peak_ratio_tail_checks.v1";
pub const FOURIER_INVERSION_NUMERIC_QUALITY_GATE_POLICY: &str =
    "diagnostics_only_or_explicit_quality_gate_failure_based_on_stability_controls.v1";
pub const FOURIER_INVERSION_DIAGNOSTICS_SCHEMA_VERSION: &str =
    "pricing.fourier_inversion.diagnostics.v1";

const DEFAULT_FOURIER_INVERSION_DAMPING_FACTOR: f64 = 1.5;
const DEFAULT_FOURIER_INVERSION_UPPER_INTEGRATION_BOUND: f64 = 150.0;
const DEFAULT_FOURIER_INVERSION_STEP_COUNT: usize = 4_096;
const DEFAULT_FOURIER_INVERSION_CONVERGENCE_ABSOLUTE_TOLERANCE: f64 = 1.0e-5;
const DEFAULT_FOURIER_INVERSION_CONVERGENCE_RELATIVE_TOLERANCE: f64 = 1.0e-4;
const DEFAULT_FOURIER_INVERSION_TERMINAL_INTEGRAND_ABSOLUTE_TOLERANCE: f64 = 5.0e-4;
const DEFAULT_FOURIER_INVERSION_TERMINAL_INTEGRAND_RATIO_TOLERANCE: f64 = 1.0e-2;
const FOURIER_NON_NEGATIVE_PRICE_CLAMP_TOLERANCE: f64 = 1.0e-12;

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

/// Quadrature rule for Fourier inversion integration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FourierInversionQuadratureRule {
    Midpoint,
    Trapezoid,
}

/// Integration contour controls for Fourier inversion.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct FourierInversionContour {
    pub damping_factor: f64,
}

impl FourierInversionContour {
    pub const fn new(damping_factor: f64) -> Self {
        Self { damping_factor }
    }

    pub fn try_new(damping_factor: f64) -> Result<Self, FourierInversionContourError> {
        let contour = Self::new(damping_factor);
        contour.validate()?;
        Ok(contour)
    }

    pub fn validate(self) -> Result<(), FourierInversionContourError> {
        if !self.damping_factor.is_finite() || self.damping_factor <= 0.0 {
            return Err(FourierInversionContourError::InvalidDampingFactor {
                value: self.damping_factor,
            });
        }

        Ok(())
    }

    pub const fn policy() -> &'static str {
        FOURIER_INVERSION_CONTOUR_POLICY
    }
}

impl Default for FourierInversionContour {
    fn default() -> Self {
        Self::new(DEFAULT_FOURIER_INVERSION_DAMPING_FACTOR)
    }
}

/// Validation errors for Fourier inversion contour controls.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FourierInversionContourError {
    InvalidDampingFactor { value: f64 },
}

impl Display for FourierInversionContourError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidDampingFactor { value } => write!(
                formatter,
                "damping_factor must be finite and strictly positive, found {value}"
            ),
        }
    }
}

impl Error for FourierInversionContourError {}

/// Numerical controls for Fourier inversion integration.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct FourierInversionNumericSettings {
    pub integration_upper_bound: f64,
    pub integration_step_count: usize,
    pub quadrature_rule: FourierInversionQuadratureRule,
}

impl FourierInversionNumericSettings {
    pub const fn new(
        integration_upper_bound: f64,
        integration_step_count: usize,
        quadrature_rule: FourierInversionQuadratureRule,
    ) -> Self {
        Self {
            integration_upper_bound,
            integration_step_count,
            quadrature_rule,
        }
    }

    pub fn try_new(
        integration_upper_bound: f64,
        integration_step_count: usize,
        quadrature_rule: FourierInversionQuadratureRule,
    ) -> Result<Self, FourierInversionNumericSettingsError> {
        let settings = Self::new(
            integration_upper_bound,
            integration_step_count,
            quadrature_rule,
        );
        settings.validate()?;
        Ok(settings)
    }

    pub fn validate(self) -> Result<(), FourierInversionNumericSettingsError> {
        if !self.integration_upper_bound.is_finite() || self.integration_upper_bound <= 0.0 {
            return Err(
                FourierInversionNumericSettingsError::InvalidIntegrationUpperBound {
                    value: self.integration_upper_bound,
                },
            );
        }

        if self.integration_step_count == 0 {
            return Err(
                FourierInversionNumericSettingsError::InvalidIntegrationStepCount {
                    value: self.integration_step_count,
                },
            );
        }

        Ok(())
    }

    pub const fn policy() -> &'static str {
        FOURIER_INVERSION_NUMERIC_SETTINGS_POLICY
    }
}

impl Default for FourierInversionNumericSettings {
    fn default() -> Self {
        Self::new(
            DEFAULT_FOURIER_INVERSION_UPPER_INTEGRATION_BOUND,
            DEFAULT_FOURIER_INVERSION_STEP_COUNT,
            FourierInversionQuadratureRule::Midpoint,
        )
    }
}

/// Validation errors for Fourier inversion numerical controls.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FourierInversionNumericSettingsError {
    InvalidIntegrationUpperBound { value: f64 },
    InvalidIntegrationStepCount { value: usize },
}

impl Display for FourierInversionNumericSettingsError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidIntegrationUpperBound { value } => write!(
                formatter,
                "integration_upper_bound must be finite and strictly positive, found {value}"
            ),
            Self::InvalidIntegrationStepCount { value } => write!(
                formatter,
                "integration_step_count must be strictly positive, found {value}"
            ),
        }
    }
}

impl Error for FourierInversionNumericSettingsError {}

/// Quality-gate behavior for numerical stability checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FourierInversionNumericQualityGate {
    DiagnosticsOnly,
    EnforceFailure,
}

impl Default for FourierInversionNumericQualityGate {
    fn default() -> Self {
        Self::DiagnosticsOnly
    }
}

/// Tuning controls for convergence and truncation quality checks.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct FourierInversionIntegrationStabilityControls {
    pub convergence_absolute_tolerance: f64,
    pub convergence_relative_tolerance: f64,
    pub terminal_integrand_absolute_tolerance: f64,
    pub terminal_integrand_ratio_tolerance: f64,
    pub quality_gate: FourierInversionNumericQualityGate,
}

impl FourierInversionIntegrationStabilityControls {
    pub const fn new(
        convergence_absolute_tolerance: f64,
        convergence_relative_tolerance: f64,
        terminal_integrand_absolute_tolerance: f64,
        terminal_integrand_ratio_tolerance: f64,
        quality_gate: FourierInversionNumericQualityGate,
    ) -> Self {
        Self {
            convergence_absolute_tolerance,
            convergence_relative_tolerance,
            terminal_integrand_absolute_tolerance,
            terminal_integrand_ratio_tolerance,
            quality_gate,
        }
    }

    pub fn try_new(
        convergence_absolute_tolerance: f64,
        convergence_relative_tolerance: f64,
        terminal_integrand_absolute_tolerance: f64,
        terminal_integrand_ratio_tolerance: f64,
        quality_gate: FourierInversionNumericQualityGate,
    ) -> Result<Self, FourierInversionIntegrationStabilityControlsError> {
        let controls = Self::new(
            convergence_absolute_tolerance,
            convergence_relative_tolerance,
            terminal_integrand_absolute_tolerance,
            terminal_integrand_ratio_tolerance,
            quality_gate,
        );
        controls.validate()?;
        Ok(controls)
    }

    pub fn validate(self) -> Result<(), FourierInversionIntegrationStabilityControlsError> {
        validate_positive_finite_tolerance(self.convergence_absolute_tolerance)
        .map_err(|value| {
            FourierInversionIntegrationStabilityControlsError::InvalidConvergenceAbsoluteTolerance {
                value,
            }
        })?;
        validate_positive_finite_tolerance(self.convergence_relative_tolerance)
        .map_err(|value| {
            FourierInversionIntegrationStabilityControlsError::InvalidConvergenceRelativeTolerance {
                value,
            }
        })?;
        validate_positive_finite_tolerance(self.terminal_integrand_absolute_tolerance)
        .map_err(|value| {
            FourierInversionIntegrationStabilityControlsError::InvalidTerminalIntegrandAbsoluteTolerance {
                value,
            }
        })?;
        validate_positive_finite_tolerance(self.terminal_integrand_ratio_tolerance)
        .map_err(|value| {
            FourierInversionIntegrationStabilityControlsError::InvalidTerminalIntegrandRatioTolerance {
                value,
            }
        })?;

        Ok(())
    }

    pub const fn policy() -> &'static str {
        FOURIER_INVERSION_INTEGRATION_STABILITY_POLICY
    }
}

impl Default for FourierInversionIntegrationStabilityControls {
    fn default() -> Self {
        Self::new(
            DEFAULT_FOURIER_INVERSION_CONVERGENCE_ABSOLUTE_TOLERANCE,
            DEFAULT_FOURIER_INVERSION_CONVERGENCE_RELATIVE_TOLERANCE,
            DEFAULT_FOURIER_INVERSION_TERMINAL_INTEGRAND_ABSOLUTE_TOLERANCE,
            DEFAULT_FOURIER_INVERSION_TERMINAL_INTEGRAND_RATIO_TOLERANCE,
            FourierInversionNumericQualityGate::default(),
        )
    }
}

/// Validation errors for integration stability controls.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FourierInversionIntegrationStabilityControlsError {
    InvalidConvergenceAbsoluteTolerance { value: f64 },
    InvalidConvergenceRelativeTolerance { value: f64 },
    InvalidTerminalIntegrandAbsoluteTolerance { value: f64 },
    InvalidTerminalIntegrandRatioTolerance { value: f64 },
}

impl Display for FourierInversionIntegrationStabilityControlsError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidConvergenceAbsoluteTolerance { value } => write!(
                formatter,
                "convergence_absolute_tolerance must be finite and strictly positive, found {value}"
            ),
            Self::InvalidConvergenceRelativeTolerance { value } => write!(
                formatter,
                "convergence_relative_tolerance must be finite and strictly positive, found {value}"
            ),
            Self::InvalidTerminalIntegrandAbsoluteTolerance { value } => write!(
                formatter,
                "terminal_integrand_absolute_tolerance must be finite and strictly positive, found {value}"
            ),
            Self::InvalidTerminalIntegrandRatioTolerance { value } => write!(
                formatter,
                "terminal_integrand_ratio_tolerance must be finite and strictly positive, found {value}"
            ),
        }
    }
}

impl Error for FourierInversionIntegrationStabilityControlsError {}

fn validate_positive_finite_tolerance(value: f64) -> Result<(), f64> {
    if !value.is_finite() || value <= 0.0 {
        return Err(value);
    }

    Ok(())
}

/// Full configuration payload for vanilla-option Fourier inversion.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct FourierInversionEngineSettings {
    pub contour: FourierInversionContour,
    pub numeric_settings: FourierInversionNumericSettings,
    pub stability_controls: FourierInversionIntegrationStabilityControls,
}

impl FourierInversionEngineSettings {
    pub const fn new(
        contour: FourierInversionContour,
        numeric_settings: FourierInversionNumericSettings,
    ) -> Self {
        Self::new_with_stability_controls(
            contour,
            numeric_settings,
            FourierInversionIntegrationStabilityControls::new(
                DEFAULT_FOURIER_INVERSION_CONVERGENCE_ABSOLUTE_TOLERANCE,
                DEFAULT_FOURIER_INVERSION_CONVERGENCE_RELATIVE_TOLERANCE,
                DEFAULT_FOURIER_INVERSION_TERMINAL_INTEGRAND_ABSOLUTE_TOLERANCE,
                DEFAULT_FOURIER_INVERSION_TERMINAL_INTEGRAND_RATIO_TOLERANCE,
                FourierInversionNumericQualityGate::DiagnosticsOnly,
            ),
        )
    }

    pub const fn new_with_stability_controls(
        contour: FourierInversionContour,
        numeric_settings: FourierInversionNumericSettings,
        stability_controls: FourierInversionIntegrationStabilityControls,
    ) -> Self {
        Self {
            contour,
            numeric_settings,
            stability_controls,
        }
    }

    pub fn try_new(
        contour: FourierInversionContour,
        numeric_settings: FourierInversionNumericSettings,
    ) -> Result<Self, FourierInversionEngineSettingsError> {
        let settings = Self::new(contour, numeric_settings);
        settings.validate()?;
        Ok(settings)
    }

    pub fn try_new_with_stability_controls(
        contour: FourierInversionContour,
        numeric_settings: FourierInversionNumericSettings,
        stability_controls: FourierInversionIntegrationStabilityControls,
    ) -> Result<Self, FourierInversionEngineSettingsError> {
        let settings =
            Self::new_with_stability_controls(contour, numeric_settings, stability_controls);
        settings.validate()?;
        Ok(settings)
    }

    pub fn validate(self) -> Result<(), FourierInversionEngineSettingsError> {
        self.contour
            .validate()
            .map_err(|source| FourierInversionEngineSettingsError::InvalidContour { source })?;
        self.numeric_settings.validate().map_err(|source| {
            FourierInversionEngineSettingsError::InvalidNumericSettings { source }
        })?;
        self.stability_controls.validate().map_err(|source| {
            FourierInversionEngineSettingsError::InvalidStabilityControls { source }
        })?;

        Ok(())
    }

    pub const fn policy() -> &'static str {
        FOURIER_INVERSION_ENGINE_POLICY
    }
}

impl Default for FourierInversionEngineSettings {
    fn default() -> Self {
        Self::new(
            FourierInversionContour::default(),
            FourierInversionNumericSettings::default(),
        )
    }
}

/// Validation errors for Fourier inversion engine settings.
#[derive(Debug, Clone, PartialEq)]
pub enum FourierInversionEngineSettingsError {
    InvalidContour {
        source: FourierInversionContourError,
    },
    InvalidNumericSettings {
        source: FourierInversionNumericSettingsError,
    },
    InvalidStabilityControls {
        source: FourierInversionIntegrationStabilityControlsError,
    },
}

impl Display for FourierInversionEngineSettingsError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidContour { source } => {
                write!(formatter, "contour settings are invalid: {source}")
            }
            Self::InvalidNumericSettings { source } => {
                write!(formatter, "numeric settings are invalid: {source}")
            }
            Self::InvalidStabilityControls { source } => {
                write!(
                    formatter,
                    "integration stability controls are invalid: {source}"
                )
            }
        }
    }
}

impl Error for FourierInversionEngineSettingsError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidContour { source } => Some(source),
            Self::InvalidNumericSettings { source } => Some(source),
            Self::InvalidStabilityControls { source } => Some(source),
        }
    }
}

/// Deterministic diagnostics emitted by one Fourier inversion pricing run.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct FourierInversionNumericQualityDiagnostics {
    pub stability_policy: &'static str,
    pub convergence_check_policy: &'static str,
    pub truncation_check_policy: &'static str,
    pub quality_gate_policy: &'static str,
    pub quality_gate: FourierInversionNumericQualityGate,
    pub convergence_absolute_tolerance: f64,
    pub convergence_relative_tolerance: f64,
    pub terminal_integrand_absolute_tolerance: f64,
    pub terminal_integrand_ratio_tolerance: f64,
    pub coarse_integral_estimate: f64,
    pub convergence_absolute_difference: f64,
    pub convergence_relative_difference: f64,
    pub terminal_integrand_absolute_value: f64,
    pub terminal_integrand_ratio: f64,
    pub convergence_check_passed: bool,
    pub truncation_check_passed: bool,
    pub insufficient_convergence_resolution: bool,
    pub numeric_quality_is_poor: bool,
}

/// Deterministic diagnostics emitted by one Fourier inversion pricing run.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct FourierInversionDiagnostics {
    pub schema_version: &'static str,
    pub engine_policy: &'static str,
    pub contour_policy: &'static str,
    pub numeric_settings_policy: &'static str,
    pub stability_policy: &'static str,
    pub quadrature_rule: FourierInversionQuadratureRule,
    pub contour_damping_factor: f64,
    pub contour_imaginary_shift: f64,
    pub integration_upper_bound: f64,
    pub integration_step_count: usize,
    pub integration_interval_width: f64,
    pub integrand_evaluation_count: usize,
    pub integral_estimate: f64,
    pub absolute_integral_estimate: f64,
    pub max_abs_integrand: f64,
    pub terminal_abs_integrand: f64,
    pub discounted_call_price: f64,
    pub numeric_quality_diagnostics: FourierInversionNumericQualityDiagnostics,
}

/// Result payload for Fourier inversion vanilla-option pricing.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct FourierInversionPricingResult {
    pub price: f64,
    pub diagnostics: FourierInversionDiagnostics,
}

/// Structured failure modes for Fourier inversion pricing.
#[derive(Debug, Clone, PartialEq)]
pub enum FourierInversionPricingError {
    InvalidMarketInputs {
        source: VanillaOptionPricingMarketInputsError,
    },
    InvalidSettings {
        source: FourierInversionEngineSettingsError,
    },
    NonFiniteLogMoneyness {
        value: f64,
    },
    NonFiniteDiscountFactor {
        value: f64,
    },
    NonFiniteDividendDiscountFactor {
        value: f64,
    },
    NonFiniteDampingMultiplier {
        value: f64,
    },
    IntegrandDivisionByNearZeroDenominator {
        frequency: f64,
    },
    NonFiniteCharacteristicFunctionValue {
        argument: ComplexScalar,
        value: ComplexScalar,
    },
    NonFiniteIntegrandValue {
        frequency: f64,
        value: f64,
    },
    NonFiniteIntegralEstimate {
        value: f64,
    },
    NonFiniteDiscountedCallPrice {
        value: f64,
    },
    NonFiniteFinalPrice {
        value: f64,
    },
    NegativePriceBeyondTolerance {
        value: f64,
        tolerance: f64,
    },
    NumericQualityFailure {
        diagnostics: FourierInversionNumericQualityDiagnostics,
    },
}

impl Display for FourierInversionPricingError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidMarketInputs { source } => {
                write!(formatter, "market_inputs payload is invalid: {source}")
            }
            Self::InvalidSettings { source } => {
                write!(formatter, "fourier settings are invalid: {source}")
            }
            Self::NonFiniteLogMoneyness { value } => {
                write!(formatter, "log_moneyness must be finite, found {value}")
            }
            Self::NonFiniteDiscountFactor { value } => {
                write!(
                    formatter,
                    "discount factor must be finite and strictly positive, found {value}"
                )
            }
            Self::NonFiniteDividendDiscountFactor { value } => write!(
                formatter,
                "dividend discount factor must be finite and strictly positive, found {value}"
            ),
            Self::NonFiniteDampingMultiplier { value } => write!(
                formatter,
                "damping multiplier must be finite and strictly positive, found {value}"
            ),
            Self::IntegrandDivisionByNearZeroDenominator { frequency } => write!(
                formatter,
                "fourier integrand denominator was not invertible at frequency {frequency}"
            ),
            Self::NonFiniteCharacteristicFunctionValue { argument, value } => write!(
                formatter,
                "characteristic function returned non-finite value {value:?} at argument {argument:?}"
            ),
            Self::NonFiniteIntegrandValue { frequency, value } => write!(
                formatter,
                "fourier integrand must be finite at frequency {frequency}, found {value}"
            ),
            Self::NonFiniteIntegralEstimate { value } => {
                write!(formatter, "integral estimate must be finite, found {value}")
            }
            Self::NonFiniteDiscountedCallPrice { value } => write!(
                formatter,
                "discounted call estimate must be finite, found {value}"
            ),
            Self::NonFiniteFinalPrice { value } => {
                write!(
                    formatter,
                    "final payoff price must be finite, found {value}"
                )
            }
            Self::NegativePriceBeyondTolerance { value, tolerance } => write!(
                formatter,
                "price is negative beyond tolerance with value={value} and tolerance={tolerance}"
            ),
            Self::NumericQualityFailure { diagnostics } => write!(
                formatter,
                "integration numeric quality failed with convergence_passed={}, truncation_passed={}, insufficient_convergence_resolution={}, convergence_absolute_difference={}, convergence_relative_difference={}, terminal_integrand_absolute_value={}, terminal_integrand_ratio={}",
                diagnostics.convergence_check_passed,
                diagnostics.truncation_check_passed,
                diagnostics.insufficient_convergence_resolution,
                diagnostics.convergence_absolute_difference,
                diagnostics.convergence_relative_difference,
                diagnostics.terminal_integrand_absolute_value,
                diagnostics.terminal_integrand_ratio
            ),
        }
    }
}

impl Error for FourierInversionPricingError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidMarketInputs { source } => Some(source),
            Self::InvalidSettings { source } => Some(source),
            _ => None,
        }
    }
}

/// Price a baseline vanilla option from a log-return characteristic function.
///
/// The engine computes a damped-call inversion and maps to the requested payoff
/// via put-call parity when needed.
pub fn price_vanilla_option_via_fourier_inversion<FCharacteristicFunction>(
    market_inputs: VanillaOptionPricingMarketInputs,
    payoff: VanillaOptionPayoffConfiguration,
    settings: FourierInversionEngineSettings,
    log_return_characteristic_function: FCharacteristicFunction,
) -> Result<FourierInversionPricingResult, FourierInversionPricingError>
where
    FCharacteristicFunction: Fn(ComplexScalar) -> ComplexScalar,
{
    market_inputs
        .validate()
        .map_err(|source| FourierInversionPricingError::InvalidMarketInputs { source })?;
    settings
        .validate()
        .map_err(|source| FourierInversionPricingError::InvalidSettings { source })?;

    let log_moneyness = (market_inputs.strike / market_inputs.spot).ln();
    if !log_moneyness.is_finite() {
        return Err(FourierInversionPricingError::NonFiniteLogMoneyness {
            value: log_moneyness,
        });
    }

    let discount_factor = (-market_inputs.risk_free_rate * market_inputs.maturity_interval).exp();
    if !discount_factor.is_finite() || discount_factor <= 0.0 {
        return Err(FourierInversionPricingError::NonFiniteDiscountFactor {
            value: discount_factor,
        });
    }

    let dividend_discount_factor =
        (-market_inputs.dividend_yield * market_inputs.maturity_interval).exp();
    if !dividend_discount_factor.is_finite() || dividend_discount_factor <= 0.0 {
        return Err(
            FourierInversionPricingError::NonFiniteDividendDiscountFactor {
                value: dividend_discount_factor,
            },
        );
    }

    let integration_report = integrate_discounted_call_price_with_fourier_inversion(
        market_inputs,
        settings,
        log_moneyness,
        discount_factor,
        &log_return_characteristic_function,
    )?;

    let price = map_discounted_call_to_vanilla_payoff(
        payoff.payoff_type,
        integration_report.discounted_call_price,
        market_inputs,
        discount_factor,
        dividend_discount_factor,
    )?;

    Ok(FourierInversionPricingResult {
        price,
        diagnostics: FourierInversionDiagnostics {
            schema_version: FOURIER_INVERSION_DIAGNOSTICS_SCHEMA_VERSION,
            engine_policy: FOURIER_INVERSION_ENGINE_POLICY,
            contour_policy: FOURIER_INVERSION_CONTOUR_POLICY,
            numeric_settings_policy: FOURIER_INVERSION_NUMERIC_SETTINGS_POLICY,
            stability_policy: FOURIER_INVERSION_INTEGRATION_STABILITY_POLICY,
            quadrature_rule: settings.numeric_settings.quadrature_rule,
            contour_damping_factor: settings.contour.damping_factor,
            contour_imaginary_shift: settings.contour.damping_factor + 1.0,
            integration_upper_bound: settings.numeric_settings.integration_upper_bound,
            integration_step_count: settings.numeric_settings.integration_step_count,
            integration_interval_width: integration_report.integration_interval_width,
            integrand_evaluation_count: integration_report.integrand_evaluation_count,
            integral_estimate: integration_report.integral_estimate,
            absolute_integral_estimate: integration_report.absolute_integral_estimate,
            max_abs_integrand: integration_report.max_abs_integrand,
            terminal_abs_integrand: integration_report.terminal_abs_integrand,
            discounted_call_price: integration_report.discounted_call_price,
            numeric_quality_diagnostics: integration_report.numeric_quality_diagnostics,
        },
    })
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct FourierDiscountedCallIntegrationReport {
    discounted_call_price: f64,
    integration_interval_width: f64,
    integrand_evaluation_count: usize,
    integral_estimate: f64,
    absolute_integral_estimate: f64,
    max_abs_integrand: f64,
    terminal_abs_integrand: f64,
    numeric_quality_diagnostics: FourierInversionNumericQualityDiagnostics,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct FourierIntegralStatistics {
    integration_interval_width: f64,
    integrand_evaluation_count: usize,
    integral_estimate: f64,
    absolute_integral_estimate: f64,
    max_abs_integrand: f64,
    terminal_abs_integrand: f64,
}

fn integrate_discounted_call_price_with_fourier_inversion<FCharacteristicFunction>(
    market_inputs: VanillaOptionPricingMarketInputs,
    settings: FourierInversionEngineSettings,
    log_moneyness: f64,
    discount_factor: f64,
    log_return_characteristic_function: &FCharacteristicFunction,
) -> Result<FourierDiscountedCallIntegrationReport, FourierInversionPricingError>
where
    FCharacteristicFunction: Fn(ComplexScalar) -> ComplexScalar,
{
    let full_resolution_statistics = evaluate_fourier_integral_statistics(
        settings.contour.damping_factor,
        settings.numeric_settings.quadrature_rule,
        settings.numeric_settings.integration_upper_bound,
        settings.numeric_settings.integration_step_count,
        log_moneyness,
        log_return_characteristic_function,
    )?;

    let numeric_quality_diagnostics = evaluate_fourier_numeric_quality(
        settings,
        full_resolution_statistics,
        log_moneyness,
        log_return_characteristic_function,
    )?;
    if numeric_quality_diagnostics.numeric_quality_is_poor
        && settings.stability_controls.quality_gate
            == FourierInversionNumericQualityGate::EnforceFailure
    {
        return Err(FourierInversionPricingError::NumericQualityFailure {
            diagnostics: numeric_quality_diagnostics,
        });
    }

    let damping_multiplier = (-(settings.contour.damping_factor * log_moneyness)).exp();
    if !damping_multiplier.is_finite() || damping_multiplier <= 0.0 {
        return Err(FourierInversionPricingError::NonFiniteDampingMultiplier {
            value: damping_multiplier,
        });
    }

    let discounted_call_price = discount_factor
        * market_inputs.spot
        * damping_multiplier
        * full_resolution_statistics.integral_estimate
        / PI;
    if !discounted_call_price.is_finite() {
        return Err(FourierInversionPricingError::NonFiniteDiscountedCallPrice {
            value: discounted_call_price,
        });
    }

    let discounted_call_price = clamp_small_negative_price(discounted_call_price)?;

    Ok(FourierDiscountedCallIntegrationReport {
        discounted_call_price,
        integration_interval_width: full_resolution_statistics.integration_interval_width,
        integrand_evaluation_count: full_resolution_statistics.integrand_evaluation_count,
        integral_estimate: full_resolution_statistics.integral_estimate,
        absolute_integral_estimate: full_resolution_statistics.absolute_integral_estimate,
        max_abs_integrand: full_resolution_statistics.max_abs_integrand,
        terminal_abs_integrand: full_resolution_statistics.terminal_abs_integrand,
        numeric_quality_diagnostics,
    })
}

fn evaluate_fourier_integral_statistics<FCharacteristicFunction>(
    damping_factor: f64,
    quadrature_rule: FourierInversionQuadratureRule,
    integration_upper_bound: f64,
    integration_step_count: usize,
    log_moneyness: f64,
    log_return_characteristic_function: &FCharacteristicFunction,
) -> Result<FourierIntegralStatistics, FourierInversionPricingError>
where
    FCharacteristicFunction: Fn(ComplexScalar) -> ComplexScalar,
{
    let integration_interval_width = integration_upper_bound / integration_step_count as f64;

    let mut weighted_integrand_sum: f64 = 0.0;
    let mut weighted_absolute_integrand_sum: f64 = 0.0;
    let mut max_abs_integrand: f64 = 0.0;
    let mut terminal_abs_integrand: f64 = 0.0;
    let mut integrand_evaluation_count = 0usize;

    match quadrature_rule {
        FourierInversionQuadratureRule::Midpoint => {
            for step_index in 0..integration_step_count {
                let frequency = (step_index as f64 + 0.5) * integration_interval_width;
                let integrand_value = evaluate_damped_call_fourier_integrand(
                    damping_factor,
                    log_moneyness,
                    frequency,
                    log_return_characteristic_function,
                )?;

                weighted_integrand_sum += integrand_value;
                let absolute_integrand_value = integrand_value.abs();
                weighted_absolute_integrand_sum += absolute_integrand_value;
                max_abs_integrand = max_abs_integrand.max(absolute_integrand_value);
                terminal_abs_integrand = absolute_integrand_value;
                integrand_evaluation_count += 1;
            }
        }
        FourierInversionQuadratureRule::Trapezoid => {
            for step_index in 0..=integration_step_count {
                let frequency = step_index as f64 * integration_interval_width;
                let integrand_value = evaluate_damped_call_fourier_integrand(
                    damping_factor,
                    log_moneyness,
                    frequency,
                    log_return_characteristic_function,
                )?;
                let integration_weight = if step_index == 0 || step_index == integration_step_count
                {
                    0.5
                } else {
                    1.0
                };

                weighted_integrand_sum += integration_weight * integrand_value;
                let absolute_integrand_value = integrand_value.abs();
                weighted_absolute_integrand_sum += integration_weight * absolute_integrand_value;
                max_abs_integrand = max_abs_integrand.max(absolute_integrand_value);
                terminal_abs_integrand = absolute_integrand_value;
                integrand_evaluation_count += 1;
            }
        }
    }

    let integral_estimate = weighted_integrand_sum * integration_interval_width;
    if !integral_estimate.is_finite() {
        return Err(FourierInversionPricingError::NonFiniteIntegralEstimate {
            value: integral_estimate,
        });
    }

    let absolute_integral_estimate = weighted_absolute_integrand_sum * integration_interval_width;
    if !absolute_integral_estimate.is_finite() {
        return Err(FourierInversionPricingError::NonFiniteIntegralEstimate {
            value: absolute_integral_estimate,
        });
    }

    Ok(FourierIntegralStatistics {
        integration_interval_width,
        integrand_evaluation_count,
        integral_estimate,
        absolute_integral_estimate,
        max_abs_integrand,
        terminal_abs_integrand,
    })
}

fn evaluate_fourier_numeric_quality<FCharacteristicFunction>(
    settings: FourierInversionEngineSettings,
    full_resolution_statistics: FourierIntegralStatistics,
    log_moneyness: f64,
    log_return_characteristic_function: &FCharacteristicFunction,
) -> Result<FourierInversionNumericQualityDiagnostics, FourierInversionPricingError>
where
    FCharacteristicFunction: Fn(ComplexScalar) -> ComplexScalar,
{
    let stability_controls = settings.stability_controls;
    let coarse_step_count = settings.numeric_settings.integration_step_count / 2;
    let insufficient_convergence_resolution = coarse_step_count == 0;

    let (
        coarse_integral_estimate,
        convergence_absolute_difference,
        convergence_relative_difference,
    ) = if insufficient_convergence_resolution {
        (full_resolution_statistics.integral_estimate, 0.0, 0.0)
    } else {
        let coarse_resolution_statistics = evaluate_fourier_integral_statistics(
            settings.contour.damping_factor,
            settings.numeric_settings.quadrature_rule,
            settings.numeric_settings.integration_upper_bound,
            coarse_step_count,
            log_moneyness,
            log_return_characteristic_function,
        )?;
        let absolute_difference = (full_resolution_statistics.integral_estimate
            - coarse_resolution_statistics.integral_estimate)
            .abs();
        let normalization = full_resolution_statistics.integral_estimate.abs().max(1.0);
        (
            coarse_resolution_statistics.integral_estimate,
            absolute_difference,
            absolute_difference / normalization,
        )
    };

    let convergence_check_passed = !insufficient_convergence_resolution
        && convergence_absolute_difference <= stability_controls.convergence_absolute_tolerance
        && convergence_relative_difference <= stability_controls.convergence_relative_tolerance;

    let terminal_integrand_ratio = if full_resolution_statistics.max_abs_integrand > 0.0 {
        full_resolution_statistics.terminal_abs_integrand
            / full_resolution_statistics.max_abs_integrand
    } else {
        0.0
    };

    let truncation_check_passed = full_resolution_statistics.terminal_abs_integrand
        <= stability_controls.terminal_integrand_absolute_tolerance
        && terminal_integrand_ratio <= stability_controls.terminal_integrand_ratio_tolerance;

    let numeric_quality_is_poor = insufficient_convergence_resolution
        || !convergence_check_passed
        || !truncation_check_passed;

    Ok(FourierInversionNumericQualityDiagnostics {
        stability_policy: FOURIER_INVERSION_INTEGRATION_STABILITY_POLICY,
        convergence_check_policy: FOURIER_INVERSION_CONVERGENCE_CHECK_POLICY,
        truncation_check_policy: FOURIER_INVERSION_TRUNCATION_CHECK_POLICY,
        quality_gate_policy: FOURIER_INVERSION_NUMERIC_QUALITY_GATE_POLICY,
        quality_gate: stability_controls.quality_gate,
        convergence_absolute_tolerance: stability_controls.convergence_absolute_tolerance,
        convergence_relative_tolerance: stability_controls.convergence_relative_tolerance,
        terminal_integrand_absolute_tolerance: stability_controls
            .terminal_integrand_absolute_tolerance,
        terminal_integrand_ratio_tolerance: stability_controls.terminal_integrand_ratio_tolerance,
        coarse_integral_estimate,
        convergence_absolute_difference,
        convergence_relative_difference,
        terminal_integrand_absolute_value: full_resolution_statistics.terminal_abs_integrand,
        terminal_integrand_ratio,
        convergence_check_passed,
        truncation_check_passed,
        insufficient_convergence_resolution,
        numeric_quality_is_poor,
    })
}

fn evaluate_damped_call_fourier_integrand<FCharacteristicFunction>(
    damping_factor: f64,
    log_moneyness: f64,
    frequency: f64,
    log_return_characteristic_function: &FCharacteristicFunction,
) -> Result<f64, FourierInversionPricingError>
where
    FCharacteristicFunction: Fn(ComplexScalar) -> ComplexScalar,
{
    let characteristic_function_argument = ComplexScalar::new(frequency, -(damping_factor + 1.0));
    let characteristic_function_value =
        log_return_characteristic_function(characteristic_function_argument);

    if !characteristic_function_value.is_finite() {
        return Err(
            FourierInversionPricingError::NonFiniteCharacteristicFunctionValue {
                argument: characteristic_function_argument,
                value: characteristic_function_value,
            },
        );
    }

    let phase_argument = frequency * log_moneyness;
    let phase_factor = ComplexScalar::new(phase_argument.cos(), -phase_argument.sin());
    let numerator = complex_multiply(phase_factor, characteristic_function_value);
    let denominator = ComplexScalar::new(
        damping_factor.mul_add(damping_factor + 1.0, -(frequency * frequency)),
        (2.0 * damping_factor + 1.0) * frequency,
    );
    let integrand_complex = numerator.divide(denominator).ok_or(
        FourierInversionPricingError::IntegrandDivisionByNearZeroDenominator { frequency },
    )?;
    let integrand_value = integrand_complex.real;

    if !integrand_value.is_finite() {
        return Err(FourierInversionPricingError::NonFiniteIntegrandValue {
            frequency,
            value: integrand_value,
        });
    }

    Ok(integrand_value)
}

fn map_discounted_call_to_vanilla_payoff(
    payoff_type: VanillaOptionPayoffType,
    discounted_call_price: f64,
    market_inputs: VanillaOptionPricingMarketInputs,
    discount_factor: f64,
    dividend_discount_factor: f64,
) -> Result<f64, FourierInversionPricingError> {
    match payoff_type {
        VanillaOptionPayoffType::Call => clamp_small_negative_price(discounted_call_price),
        VanillaOptionPayoffType::Put => {
            let discounted_spot = market_inputs.spot * dividend_discount_factor;
            let discounted_strike = market_inputs.strike * discount_factor;
            let put_price = discounted_call_price - discounted_spot + discounted_strike;
            if !put_price.is_finite() {
                return Err(FourierInversionPricingError::NonFiniteFinalPrice { value: put_price });
            }
            clamp_small_negative_price(put_price)
        }
    }
}

fn clamp_small_negative_price(price: f64) -> Result<f64, FourierInversionPricingError> {
    if !price.is_finite() {
        return Err(FourierInversionPricingError::NonFiniteFinalPrice { value: price });
    }
    if price < -FOURIER_NON_NEGATIVE_PRICE_CLAMP_TOLERANCE {
        return Err(FourierInversionPricingError::NegativePriceBeyondTolerance {
            value: price,
            tolerance: FOURIER_NON_NEGATIVE_PRICE_CLAMP_TOLERANCE,
        });
    }
    if price < 0.0 {
        return Ok(0.0);
    }

    Ok(price)
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

/// Deterministic factor-only composition payload for oracle-derived trading-time CF.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReturnCharacteristicFunctionFactorComposition {
    pub trading_time_characteristic_factor: ComplexScalar,
    pub gap_characteristic_factor: ComplexScalar,
    pub combined_characteristic_factor: ComplexScalar,
    pub composition_policy: &'static str,
    pub domain_guard_policy: &'static str,
}

/// Structured failure modes for full return characteristic-function assembly.
#[derive(Debug, Clone, PartialEq)]
pub enum ReturnCharacteristicFunctionAssemblyError {
    InvalidCharacteristicFunctionArgument {
        argument: ComplexScalar,
    },
    InvalidOracleTransformOutput {
        source: ExponentialAffineTransformOutputError,
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
            Self::InvalidOracleTransformOutput { source } => write!(
                formatter,
                "oracle transform output is invalid for characteristic-function assembly: {source}"
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
            Self::InvalidOracleTransformOutput { source } => Some(source),
            Self::InvalidReturnDecomposition { source } => Some(source),
            _ => None,
        }
    }
}

/// Deterministic diagnostics for one pricing-path martingale-condition validation run.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PricingMartingaleConditionValidationReport {
    pub theta: f64,
    pub buy_side_log_moment_exponent: ComplexScalar,
    pub sell_side_log_moment_exponent: ComplexScalar,
    pub combined_trading_log_moment_exponent: ComplexScalar,
    pub martingale_condition: EsscherMartingaleConditionReport,
    pub policy: &'static str,
}

/// Structured failure modes for pricing-path martingale-condition validation.
#[derive(Debug, Clone, PartialEq)]
pub enum PricingMartingaleConditionValidationError {
    InvalidMarketInputs {
        source: VanillaOptionPricingMarketInputsError,
    },
    InvalidReturnDecomposition {
        source: ReturnDecompositionValidationError,
    },
    BuySideEsscherTransformFailure {
        source: EsscherTransformError,
    },
    SellSideEsscherTransformFailure {
        source: EsscherTransformError,
    },
    InvalidMartingaleInput {
        source: EsscherMartingaleConditionError,
    },
    MartingaleConditionViolation {
        observed_log_moment_exponent: ComplexScalar,
        target_log_moment_exponent: f64,
        residual_norm: f64,
        tolerance: f64,
    },
}

impl Display for PricingMartingaleConditionValidationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidMarketInputs { source } => {
                write!(formatter, "market_inputs payload is invalid: {source}")
            }
            Self::InvalidReturnDecomposition { source } => write!(
                formatter,
                "return decomposition payload is invalid for martingale validation: {source}"
            ),
            Self::BuySideEsscherTransformFailure { source } => {
                write!(
                    formatter,
                    "buy-side Esscher transform failed during martingale validation: {source}"
                )
            }
            Self::SellSideEsscherTransformFailure { source } => {
                write!(
                    formatter,
                    "sell-side Esscher transform failed during martingale validation: {source}"
                )
            }
            Self::InvalidMartingaleInput { source } => {
                write!(formatter, "martingale input is invalid: {source}")
            }
            Self::MartingaleConditionViolation {
                observed_log_moment_exponent,
                target_log_moment_exponent,
                residual_norm,
                tolerance,
            } => write!(
                formatter,
                "pricing martingale condition failed with observed={observed_log_moment_exponent:?}, target={target_log_moment_exponent}, residual_norm={residual_norm}, tolerance={tolerance}"
            ),
        }
    }
}

impl Error for PricingMartingaleConditionValidationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidMarketInputs { source } => Some(source),
            Self::InvalidReturnDecomposition { source } => Some(source),
            Self::BuySideEsscherTransformFailure { source } => Some(source),
            Self::SellSideEsscherTransformFailure { source } => Some(source),
            Self::InvalidMartingaleInput { source } => Some(source),
            _ => None,
        }
    }
}

/// Resolve interval-aligned hourly factors for the pricing path.
pub use crate::hourly_factor_provider::provide_hourly_factor_contract as load_hourly_factor_contract;

/// Project state to interval intensities for coarse-time sufficient-statistics paths.
pub fn project_hawkes_intensities_coarse_interval(
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
pub fn solve_exponential_affine_transform(
    transform_input: ExponentialAffineTransformInput,
) -> Result<ExponentialAffineTransformIntegrationResult, ExponentialAffineTransformSolverError> {
    solve_exponential_affine_transform_with_optional_settings(transform_input, None)
}

/// Solve one-interval exponential-affine transform for pricing workflows with explicit settings.
pub fn solve_exponential_affine_transform_with_settings(
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
pub fn compose_nig_trading_time_exponent(
    driver_parameters: NigBuySellDriverParameters,
    argument: ComplexScalar,
    return_decomposition: ReturnDecomposition,
) -> NigTradingTimeCharacteristicExponentContributions {
    compose_nig_trading_time_exponent_with_edge_mode(
        driver_parameters,
        argument,
        return_decomposition,
        ReturnDecompositionEdgeMode::Disabled,
    )
}

/// Compose trading-time characteristic exponent for pricing with optional edge-mode gating.
pub use crate::return_decomposition::compose_nig_trading_time_exponent_with_edge_mode;

/// Compose full return characteristic function for pricing through one shared assembler.
///
/// This path reuses the canonical trading-time exponent composition helper, which
/// keeps sell-side `-u` convention logic in a single module.
pub fn compose_total_return_characteristic_function(
    driver_parameters: NigBuySellDriverParameters,
    argument: ComplexScalar,
    return_decomposition: ReturnDecomposition,
    gap_characteristic_factor: ComplexScalar,
) -> Result<ReturnCharacteristicFunctionAssembly, ReturnCharacteristicFunctionAssemblyError> {
    compose_total_return_characteristic_function_with_edge_mode(
        driver_parameters,
        argument,
        return_decomposition,
        gap_characteristic_factor,
        ReturnDecompositionEdgeMode::Disabled,
    )
}

/// Validate pricing-path martingale wiring for Esscher-tilted NIG components.
///
/// This check is trading-time only.
/// Gap martingale adjustments are intentionally left to the gap-pricing path.
pub fn validate_pricing_martingale_condition_under_esscher_tilt(
    driver_parameters: NigBuySellDriverParameters,
    return_decomposition: ReturnDecomposition,
    market_inputs: VanillaOptionPricingMarketInputs,
    theta: f64,
    tolerance: f64,
) -> Result<PricingMartingaleConditionValidationReport, PricingMartingaleConditionValidationError> {
    market_inputs.validate().map_err(|source| {
        PricingMartingaleConditionValidationError::InvalidMarketInputs { source }
    })?;
    return_decomposition.validate().map_err(|source| {
        PricingMartingaleConditionValidationError::InvalidReturnDecomposition { source }
    })?;

    let buy_side_log_moment_exponent =
        transform_nig_characteristic_exponent_under_esscher_tilt(
            NigEsscherTransformationInput::new(
                driver_parameters.buy_side_parameters,
                ComplexScalar::new(0.0, -1.0),
                theta,
            ),
        )
        .map_err(|source| {
            PricingMartingaleConditionValidationError::BuySideEsscherTransformFailure { source }
        })?
        .transformed_exponent;

    let sell_side_log_moment_exponent =
        transform_nig_characteristic_exponent_under_esscher_tilt(
            NigEsscherTransformationInput::new(
                driver_parameters.sell_side_parameters,
                ComplexScalar::new(0.0, 1.0),
                theta,
            ),
        )
        .map_err(|source| {
            PricingMartingaleConditionValidationError::SellSideEsscherTransformFailure { source }
        })?
        .transformed_exponent;

    let combined_trading_log_moment_exponent = buy_side_log_moment_exponent
        .scale(return_decomposition.a_plus)
        .add(sell_side_log_moment_exponent.scale(return_decomposition.a_minus));
    let target_log_moment_exponent = (market_inputs.risk_free_rate - market_inputs.dividend_yield)
        * market_inputs.maturity_interval;
    let martingale_condition =
        evaluate_esscher_martingale_condition(EsscherMartingaleConditionInput::new(
            combined_trading_log_moment_exponent,
            target_log_moment_exponent,
            tolerance,
        ))
        .map_err(|source| {
            PricingMartingaleConditionValidationError::InvalidMartingaleInput { source }
        })?;

    if !martingale_condition.is_satisfied {
        return Err(
            PricingMartingaleConditionValidationError::MartingaleConditionViolation {
                observed_log_moment_exponent: combined_trading_log_moment_exponent,
                target_log_moment_exponent,
                residual_norm: martingale_condition.residual_norm,
                tolerance: martingale_condition.tolerance,
            },
        );
    }

    Ok(PricingMartingaleConditionValidationReport {
        theta,
        buy_side_log_moment_exponent,
        sell_side_log_moment_exponent,
        combined_trading_log_moment_exponent,
        martingale_condition,
        policy: PRICING_MARTINGALE_CONDITION_VALIDATION_POLICY,
    })
}

/// Apply one coarse-time transform-path Hawkes state transition using canonical call order.
pub use crate::hawkes::evolve_hawkes_state;

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
struct FourierInversionContourPayload {
    damping_factor: f64,
}

impl<'de> Deserialize<'de> for FourierInversionContour {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let payload = FourierInversionContourPayload::deserialize(deserializer)?;
        Self::try_new(payload.damping_factor).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct FourierInversionNumericSettingsPayload {
    integration_upper_bound: f64,
    integration_step_count: usize,
    quadrature_rule: FourierInversionQuadratureRule,
}

impl<'de> Deserialize<'de> for FourierInversionNumericSettings {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let payload = FourierInversionNumericSettingsPayload::deserialize(deserializer)?;
        Self::try_new(
            payload.integration_upper_bound,
            payload.integration_step_count,
            payload.quadrature_rule,
        )
        .map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct FourierInversionIntegrationStabilityControlsPayload {
    convergence_absolute_tolerance: f64,
    convergence_relative_tolerance: f64,
    terminal_integrand_absolute_tolerance: f64,
    terminal_integrand_ratio_tolerance: f64,
    #[serde(default)]
    quality_gate: FourierInversionNumericQualityGate,
}

impl<'de> Deserialize<'de> for FourierInversionIntegrationStabilityControls {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let payload =
            FourierInversionIntegrationStabilityControlsPayload::deserialize(deserializer)?;
        Self::try_new(
            payload.convergence_absolute_tolerance,
            payload.convergence_relative_tolerance,
            payload.terminal_integrand_absolute_tolerance,
            payload.terminal_integrand_ratio_tolerance,
            payload.quality_gate,
        )
        .map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct FourierInversionEngineSettingsPayload {
    contour: FourierInversionContour,
    numeric_settings: FourierInversionNumericSettings,
    #[serde(default)]
    stability_controls: FourierInversionIntegrationStabilityControls,
}

impl<'de> Deserialize<'de> for FourierInversionEngineSettings {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let payload = FourierInversionEngineSettingsPayload::deserialize(deserializer)?;
        Self::try_new_with_stability_controls(
            payload.contour,
            payload.numeric_settings,
            payload.stability_controls,
        )
        .map_err(serde::de::Error::custom)
    }
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

/// Compose the full return characteristic function with explicit edge-mode control.
pub fn compose_total_return_characteristic_function_with_edge_mode(
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
    let factor_composition = compose_return_characteristic_factor_shared(
        trading_time_characteristic_factor,
        effective_gap_characteristic_factor,
    )?;

    Ok(ReturnCharacteristicFunctionAssembly {
        trading_time_exponent_contributions,
        trading_time_characteristic_factor: factor_composition.trading_time_characteristic_factor,
        gap_characteristic_factor: factor_composition.gap_characteristic_factor,
        combined_characteristic_factor: factor_composition.combined_characteristic_factor,
        composition_policy: factor_composition.composition_policy,
        domain_guard_policy: factor_composition.domain_guard_policy,
    })
}

/// Compose full return characteristic factors from one oracle solve output.
pub fn compose_total_return_characteristic_function_from_oracle_output(
    oracle_output: ExponentialAffineTransformOutput,
    gap_characteristic_factor: ComplexScalar,
) -> Result<ReturnCharacteristicFunctionFactorComposition, ReturnCharacteristicFunctionAssemblyError>
{
    oracle_output.validate().map_err(|source| {
        ReturnCharacteristicFunctionAssemblyError::InvalidOracleTransformOutput { source }
    })?;

    compose_return_characteristic_factor_shared(
        oracle_output.transform_value,
        gap_characteristic_factor,
    )
}

fn compose_return_characteristic_factor_shared(
    trading_time_characteristic_factor: ComplexScalar,
    gap_characteristic_factor: ComplexScalar,
) -> Result<ReturnCharacteristicFunctionFactorComposition, ReturnCharacteristicFunctionAssemblyError>
{
    if !trading_time_characteristic_factor.is_finite() {
        return Err(
            ReturnCharacteristicFunctionAssemblyError::NonFiniteTradingTimeCharacteristicFactor {
                value: trading_time_characteristic_factor,
            },
        );
    }

    if !gap_characteristic_factor.is_finite() {
        return Err(
            ReturnCharacteristicFunctionAssemblyError::InvalidGapCharacteristicFactor {
                value: gap_characteristic_factor,
            },
        );
    }

    let combined_characteristic_factor = complex_multiply(
        trading_time_characteristic_factor,
        gap_characteristic_factor,
    );
    if !combined_characteristic_factor.is_finite() {
        return Err(
            ReturnCharacteristicFunctionAssemblyError::NonFiniteCombinedCharacteristicFactor {
                value: combined_characteristic_factor,
            },
        );
    }

    Ok(ReturnCharacteristicFunctionFactorComposition {
        trading_time_characteristic_factor,
        gap_characteristic_factor,
        combined_characteristic_factor,
        composition_policy: RETURN_CHARACTERISTIC_FUNCTION_COMPOSITION_POLICY,
        domain_guard_policy: RETURN_CHARACTERISTIC_FUNCTION_DOMAIN_GUARD_POLICY,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::affine_oracle::{
        ExponentialAffineTransformDVector, ExponentialAffineTransformOutput,
    };
    use crate::hawkes::{HawkesExcitationState, HawkesKernelLoadings, HawkesKernelParameters};
    use crate::news::GammaSeverityMarkConfiguration;
    use crate::nig::{ComplexScalar, NigSharedPlusShiftConfiguration};
    use crate::pricing::{
        FOURIER_INVERSION_DIAGNOSTICS_SCHEMA_VERSION, FourierInversionContour,
        FourierInversionContourError, FourierInversionEngineSettings,
        FourierInversionIntegrationStabilityControls,
        FourierInversionIntegrationStabilityControlsError, FourierInversionNumericQualityGate,
        FourierInversionNumericSettings, FourierInversionPricingError,
        FourierInversionQuadratureRule, PRICING_MARTINGALE_CONDITION_VALIDATION_POLICY,
        PricingMartingaleConditionValidationError,
        RETURN_CHARACTERISTIC_FUNCTION_COMPOSITION_POLICY,
        RETURN_CHARACTERISTIC_FUNCTION_DOMAIN_GUARD_POLICY,
        ReturnCharacteristicFunctionAssemblyError, VanillaOptionPayoffConfiguration,
        VanillaOptionPayoffType, VanillaOptionPricingDiagnostics,
        VanillaOptionPricingDiagnosticsError, VanillaOptionPricingMarketInputs,
        VanillaOptionPricingMarketInputsError, VanillaOptionPricingModelParameters,
        VanillaOptionPricingRequest, VanillaOptionPricingRequestError,
        VanillaOptionPricingResponse, VanillaOptionPricingResponseError,
        compose_total_return_characteristic_function,
        compose_total_return_characteristic_function_from_oracle_output,
        compose_total_return_characteristic_function_with_edge_mode,
        price_vanilla_option_via_fourier_inversion,
        validate_pricing_martingale_condition_under_esscher_tilt,
    };
    use crate::return_decomposition::{
        ReturnDecomposition, ReturnDecompositionEdgeMode, ReturnDecompositionInput,
    };
    use crate::risk_neutral::{
        NigEsscherTransformationInput, transform_nig_characteristic_exponent_under_esscher_tilt,
    };

    use super::compose_nig_trading_time_exponent;

    #[test]
    fn fourier_inversion_call_price_matches_black_scholes_sanity_case() {
        // Note: This validates Fourier inversion mechanics against a closed-form benchmark and does
        // NOT assume Janiform dynamics are Black-Scholes.

        let market_inputs =
            VanillaOptionPricingMarketInputs::try_new(100.0, 100.0, 1.0, 0.02, 0.01)
                .expect("market inputs should construct");
        let volatility = 0.2;
        let settings = FourierInversionEngineSettings::try_new(
            FourierInversionContour::try_new(1.5).expect("contour settings should construct"),
            FourierInversionNumericSettings::try_new(
                160.0,
                8_192,
                FourierInversionQuadratureRule::Midpoint,
            )
            .expect("numeric settings should construct"),
        )
        .expect("engine settings should construct");

        let result = price_vanilla_option_via_fourier_inversion(
            market_inputs,
            VanillaOptionPayoffConfiguration {
                payoff_type: VanillaOptionPayoffType::Call,
            },
            settings,
            |argument| {
                black_scholes_log_return_characteristic_function(
                    market_inputs,
                    volatility,
                    argument,
                )
            },
        )
        .expect("fourier inversion call pricing should succeed");

        let expected_call_price =
            black_scholes_price(market_inputs, volatility, VanillaOptionPayoffType::Call);
        let absolute_error = (result.price - expected_call_price).abs();
        assert!(
            absolute_error <= 1.0e-3,
            "call absolute pricing error {absolute_error} exceeded tolerance"
        );
        assert_eq!(
            result.diagnostics.schema_version,
            FOURIER_INVERSION_DIAGNOSTICS_SCHEMA_VERSION
        );
    }

    #[test]
    fn fourier_inversion_put_price_matches_black_scholes_sanity_case() {
        // Note: This validates Fourier inversion mechanics against a closed-form benchmark and does
        // NOT assume Janiform dynamics are Black-Scholes.

        let market_inputs =
            VanillaOptionPricingMarketInputs::try_new(100.0, 100.0, 1.0, 0.02, 0.01)
                .expect("market inputs should construct");
        let volatility = 0.2;
        let settings = FourierInversionEngineSettings::try_new(
            FourierInversionContour::try_new(1.5).expect("contour settings should construct"),
            FourierInversionNumericSettings::try_new(
                160.0,
                8_192,
                FourierInversionQuadratureRule::Trapezoid,
            )
            .expect("numeric settings should construct"),
        )
        .expect("engine settings should construct");

        let result = price_vanilla_option_via_fourier_inversion(
            market_inputs,
            VanillaOptionPayoffConfiguration {
                payoff_type: VanillaOptionPayoffType::Put,
            },
            settings,
            |argument| {
                black_scholes_log_return_characteristic_function(
                    market_inputs,
                    volatility,
                    argument,
                )
            },
        )
        .expect("fourier inversion put pricing should succeed");

        let expected_put_price =
            black_scholes_price(market_inputs, volatility, VanillaOptionPayoffType::Put);
        let absolute_error = (result.price - expected_put_price).abs();
        assert!(
            absolute_error <= 1.0e-3,
            "put absolute pricing error {absolute_error} exceeded tolerance"
        );
        assert_eq!(
            result.diagnostics.integrand_evaluation_count,
            result.diagnostics.integration_step_count + 1
        );
    }

    #[test]
    fn fourier_inversion_rejects_invalid_integration_settings() {
        let contour_error = FourierInversionContour::try_new(0.0)
            .expect_err("zero damping factor must fail contour validation");
        assert!(matches!(
            contour_error,
            FourierInversionContourError::InvalidDampingFactor { value } if value == 0.0
        ));

        let market_inputs =
            VanillaOptionPricingMarketInputs::try_new(100.0, 100.0, 1.0, 0.02, 0.01)
                .expect("market inputs should construct");
        let invalid_settings = FourierInversionEngineSettings::new(
            FourierInversionContour::new(1.0),
            FourierInversionNumericSettings::new(
                120.0,
                0,
                FourierInversionQuadratureRule::Midpoint,
            ),
        );

        let error = price_vanilla_option_via_fourier_inversion(
            market_inputs,
            VanillaOptionPayoffConfiguration {
                payoff_type: VanillaOptionPayoffType::Call,
            },
            invalid_settings,
            |_argument| ComplexScalar::new(1.0, 0.0),
        )
        .expect_err("invalid integration settings must fail pricing");
        assert!(matches!(
            error,
            FourierInversionPricingError::InvalidSettings { .. }
        ));
    }

    #[test]
    fn fourier_inversion_stability_controls_validate_and_deserialize_with_defaults() {
        let controls_error = FourierInversionIntegrationStabilityControls::try_new(
            f64::NAN,
            1.0e-4,
            1.0e-4,
            1.0e-2,
            FourierInversionNumericQualityGate::DiagnosticsOnly,
        )
        .expect_err("non-finite convergence tolerance must fail stability-control validation");
        assert!(matches!(
            controls_error,
            FourierInversionIntegrationStabilityControlsError::InvalidConvergenceAbsoluteTolerance { value } if value.is_nan()
        ));

        let parsed_settings: FourierInversionEngineSettings = serde_json::from_value(json!({
            "contour": {
                "damping_factor": 1.5
            },
            "numeric_settings": {
                "integration_upper_bound": 120.0,
                "integration_step_count": 4096,
                "quadrature_rule": "midpoint"
            }
        }))
        .expect("engine settings should deserialize with default stability controls");

        assert_eq!(
            parsed_settings.stability_controls,
            FourierInversionIntegrationStabilityControls::default()
        );
    }

    #[test]
    fn fourier_inversion_emits_numeric_quality_diagnostics_for_poor_settings() {
        let market_inputs =
            VanillaOptionPricingMarketInputs::try_new(100.0, 100.0, 1.0, 0.02, 0.01)
                .expect("market inputs should construct");
        let volatility = 0.2;

        let strict_controls = FourierInversionIntegrationStabilityControls::try_new(
            1.0e-8,
            1.0e-8,
            1.0e-8,
            1.0e-6,
            FourierInversionNumericQualityGate::DiagnosticsOnly,
        )
        .expect("strict stability controls should construct");
        let poor_settings = FourierInversionEngineSettings::try_new_with_stability_controls(
            FourierInversionContour::try_new(1.2).expect("contour settings should construct"),
            FourierInversionNumericSettings::try_new(
                3.0,
                2,
                FourierInversionQuadratureRule::Midpoint,
            )
            .expect("numeric settings should construct"),
            strict_controls,
        )
        .expect("engine settings should construct");

        let result = price_vanilla_option_via_fourier_inversion(
            market_inputs,
            VanillaOptionPayoffConfiguration {
                payoff_type: VanillaOptionPayoffType::Call,
            },
            poor_settings,
            |argument| {
                black_scholes_log_return_characteristic_function(
                    market_inputs,
                    volatility,
                    argument,
                )
            },
        )
        .expect("diagnostics-only quality gate should not fail pricing");

        assert!(
            result
                .diagnostics
                .numeric_quality_diagnostics
                .numeric_quality_is_poor
        );
    }

    #[test]
    fn fourier_inversion_quality_gate_can_fail_under_poor_settings() {
        let market_inputs =
            VanillaOptionPricingMarketInputs::try_new(100.0, 100.0, 1.0, 0.02, 0.01)
                .expect("market inputs should construct");
        let volatility = 0.2;

        let strict_controls = FourierInversionIntegrationStabilityControls::try_new(
            1.0e-8,
            1.0e-8,
            1.0e-8,
            1.0e-6,
            FourierInversionNumericQualityGate::EnforceFailure,
        )
        .expect("strict stability controls should construct");
        let poor_settings = FourierInversionEngineSettings::try_new_with_stability_controls(
            FourierInversionContour::try_new(1.2).expect("contour settings should construct"),
            FourierInversionNumericSettings::try_new(
                3.0,
                2,
                FourierInversionQuadratureRule::Midpoint,
            )
            .expect("numeric settings should construct"),
            strict_controls,
        )
        .expect("engine settings should construct");

        let error = price_vanilla_option_via_fourier_inversion(
            market_inputs,
            VanillaOptionPayoffConfiguration {
                payoff_type: VanillaOptionPayoffType::Call,
            },
            poor_settings,
            |argument| {
                black_scholes_log_return_characteristic_function(
                    market_inputs,
                    volatility,
                    argument,
                )
            },
        )
        .expect_err("enforced quality gate must fail on poor numeric quality");

        assert!(matches!(
            error,
            FourierInversionPricingError::NumericQualityFailure { diagnostics }
            if diagnostics.numeric_quality_is_poor
        ));
    }

    #[test]
    fn fourier_inversion_quality_diagnostics_converge_on_reference_fixture() {
        let market_inputs =
            VanillaOptionPricingMarketInputs::try_new(100.0, 100.0, 1.0, 0.02, 0.01)
                .expect("market inputs should construct");
        let volatility = 0.2;
        let settings = FourierInversionEngineSettings::try_new(
            FourierInversionContour::try_new(1.5).expect("contour settings should construct"),
            FourierInversionNumericSettings::try_new(
                160.0,
                8_192,
                FourierInversionQuadratureRule::Midpoint,
            )
            .expect("numeric settings should construct"),
        )
        .expect("engine settings should construct");

        let result = price_vanilla_option_via_fourier_inversion(
            market_inputs,
            VanillaOptionPayoffConfiguration {
                payoff_type: VanillaOptionPayoffType::Call,
            },
            settings,
            |argument| {
                black_scholes_log_return_characteristic_function(
                    market_inputs,
                    volatility,
                    argument,
                )
            },
        )
        .expect("reference settings should produce converged diagnostics");

        assert!(
            !result
                .diagnostics
                .numeric_quality_diagnostics
                .numeric_quality_is_poor
        );
        assert!(
            result
                .diagnostics
                .numeric_quality_diagnostics
                .convergence_check_passed
        );
        assert!(
            result
                .diagnostics
                .numeric_quality_diagnostics
                .truncation_check_passed
        );
    }

    #[test]
    fn fourier_inversion_surfaces_non_finite_characteristic_function_boundary_failures() {
        let market_inputs = VanillaOptionPricingMarketInputs::try_new(100.0, 95.0, 0.5, 0.01, 0.0)
            .expect("market inputs should construct");
        let settings = FourierInversionEngineSettings::try_new(
            FourierInversionContour::try_new(1.2).expect("contour settings should construct"),
            FourierInversionNumericSettings::try_new(
                10.0,
                16,
                FourierInversionQuadratureRule::Midpoint,
            )
            .expect("numeric settings should construct"),
        )
        .expect("engine settings should construct");

        let error = price_vanilla_option_via_fourier_inversion(
            market_inputs,
            VanillaOptionPayoffConfiguration {
                payoff_type: VanillaOptionPayoffType::Call,
            },
            settings,
            |argument| {
                if argument.real > 9.0 {
                    return ComplexScalar::new(f64::NAN, 0.0);
                }
                ComplexScalar::new(1.0, 0.0)
            },
        )
        .expect_err("non-finite characteristic-function output should fail pricing");

        assert!(matches!(
            error,
            FourierInversionPricingError::NonFiniteCharacteristicFunctionValue { .. }
        ));
    }

    #[test]
    fn fourier_inversion_output_is_deterministic_for_reference_fixture() {
        let market_inputs =
            VanillaOptionPricingMarketInputs::try_new(103.0, 100.0, 0.75, 0.03, 0.01)
                .expect("market inputs should construct");
        let volatility = 0.25;
        let settings = FourierInversionEngineSettings::default();
        let payoff = VanillaOptionPayoffConfiguration {
            payoff_type: VanillaOptionPayoffType::Call,
        };

        let first_result = price_vanilla_option_via_fourier_inversion(
            market_inputs,
            payoff,
            settings,
            |argument| {
                black_scholes_log_return_characteristic_function(
                    market_inputs,
                    volatility,
                    argument,
                )
            },
        )
        .expect("first deterministic pricing run should succeed");
        let second_result = price_vanilla_option_via_fourier_inversion(
            market_inputs,
            payoff,
            settings,
            |argument| {
                black_scholes_log_return_characteristic_function(
                    market_inputs,
                    volatility,
                    argument,
                )
            },
        )
        .expect("second deterministic pricing run should succeed");

        assert_eq!(first_result, second_result);
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

        let composed =
            compose_nig_trading_time_exponent(driver_parameters, argument, return_decomposition);

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

        let assembled = compose_total_return_characteristic_function(
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

        let assembled = compose_total_return_characteristic_function(
            driver_parameters,
            argument,
            return_decomposition,
            gap_characteristic_factor,
        )
        .expect("full CF assembly should succeed with valid finite inputs");

        let expected_trading_exponent =
            compose_nig_trading_time_exponent(driver_parameters, argument, return_decomposition)
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
    fn oracle_output_cf_assembly_matches_canonical_pricing_path_for_fixed_trading_factor() {
        let driver_parameters = NigSharedPlusShiftConfiguration::try_new(2.3, -0.8, 0.7, 0.1, -0.2)
            .expect("valid shared-plus-shift payload should construct")
            .derive_driver_parameters()
            .expect("valid shared-plus-shift payload should derive drivers");
        let argument = ComplexScalar::new(0.35, -0.15);
        let return_decomposition = ReturnDecomposition::try_new(ReturnDecompositionInput {
            a_plus: 0.8,
            a_minus: 1.1,
            gap: 0.0,
        })
        .expect("valid return decomposition payload should construct");
        let gap_characteristic_factor = ComplexScalar::new(0.92, -0.08);

        let canonical_assembly = compose_total_return_characteristic_function(
            driver_parameters,
            argument,
            return_decomposition,
            gap_characteristic_factor,
        )
        .expect("canonical pricing assembly should succeed");

        let oracle_factor_assembly =
            compose_total_return_characteristic_function_from_oracle_output(
                oracle_output_with_transform_value(
                    canonical_assembly.trading_time_characteristic_factor,
                ),
                gap_characteristic_factor,
            )
            .expect("oracle-output assembly should succeed");

        assert_complex_close(
            oracle_factor_assembly.trading_time_characteristic_factor,
            canonical_assembly.trading_time_characteristic_factor,
            1.0e-13,
        );
        assert_complex_close(
            oracle_factor_assembly.combined_characteristic_factor,
            canonical_assembly.combined_characteristic_factor,
            1.0e-13,
        );
        assert_eq!(
            oracle_factor_assembly.composition_policy,
            RETURN_CHARACTERISTIC_FUNCTION_COMPOSITION_POLICY
        );
        assert_eq!(
            oracle_factor_assembly.domain_guard_policy,
            RETURN_CHARACTERISTIC_FUNCTION_DOMAIN_GUARD_POLICY
        );
    }

    #[test]
    fn oracle_output_cf_assembly_isolates_trading_only_and_gap_only_paths() {
        let trading_only = compose_total_return_characteristic_function_from_oracle_output(
            oracle_output_with_transform_value(ComplexScalar::new(0.84, -0.22)),
            ComplexScalar::new(1.0, 0.0),
        )
        .expect("trading-only oracle composition should succeed");

        assert_complex_close(
            trading_only.combined_characteristic_factor,
            trading_only.trading_time_characteristic_factor,
            1.0e-13,
        );

        let gap_characteristic_factor = ComplexScalar::new(0.66, 0.19);
        let gap_only = compose_total_return_characteristic_function_from_oracle_output(
            oracle_output_with_transform_value(ComplexScalar::new(1.0, 0.0)),
            gap_characteristic_factor,
        )
        .expect("gap-only oracle composition should succeed");

        assert_complex_close(
            gap_only.combined_characteristic_factor,
            gap_characteristic_factor,
            1.0e-13,
        );
    }

    #[test]
    fn oracle_output_cf_assembly_regression_guard_detects_positive_u_sell_side_sign_inversion() {
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

        let correct_trading_exponent =
            compose_nig_trading_time_exponent(driver_parameters, argument, return_decomposition)
                .combined_weighted_exponent;
        let correct_trading_factor = super::complex_exponential(correct_trading_exponent);
        let incorrect_positive_u_trading_factor = super::complex_exponential(
            driver_parameters
                .sell_side_parameters
                .characteristic_exponent(argument),
        );

        let correct_assembly = compose_total_return_characteristic_function_from_oracle_output(
            oracle_output_with_transform_value(correct_trading_factor),
            ComplexScalar::new(1.0, 0.0),
        )
        .expect("correct-sign oracle assembly should succeed");
        let incorrect_assembly = compose_total_return_characteristic_function_from_oracle_output(
            oracle_output_with_transform_value(incorrect_positive_u_trading_factor),
            ComplexScalar::new(1.0, 0.0),
        )
        .expect("incorrect-sign oracle assembly should still compose deterministically");

        let regression_distance = correct_assembly
            .combined_characteristic_factor
            .subtract(incorrect_assembly.combined_characteristic_factor)
            .norm();
        assert!(
            regression_distance > 1.0e-6,
            "sell-side sign-convention regression detected in oracle-output CF assembly with distance={regression_distance}"
        );
    }

    #[test]
    fn oracle_output_cf_assembly_rejects_invalid_output_payload() {
        let error = compose_total_return_characteristic_function_from_oracle_output(
            ExponentialAffineTransformOutput {
                c_at_delta: ComplexScalar::new(f64::NAN, 0.0),
                d_at_delta: ExponentialAffineTransformDVector::new(
                    ComplexScalar::new(0.0, 0.0),
                    ComplexScalar::new(0.0, 0.0),
                    ComplexScalar::new(0.0, 0.0),
                    ComplexScalar::new(0.0, 0.0),
                ),
                transform_value: ComplexScalar::new(1.0, 0.0),
            },
            ComplexScalar::new(1.0, 0.0),
        )
        .expect_err("invalid oracle output must fail full CF assembly");

        assert!(matches!(
            error,
            ReturnCharacteristicFunctionAssemblyError::InvalidOracleTransformOutput { .. }
        ));
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

        let assembled = compose_total_return_characteristic_function_with_edge_mode(
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

        let assembled = compose_total_return_characteristic_function_with_edge_mode(
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

        let assembled = compose_total_return_characteristic_function_with_edge_mode(
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

        let error = compose_total_return_characteristic_function(
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

        let error = compose_total_return_characteristic_function(
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

        let assembled = compose_total_return_characteristic_function(
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
    fn pricing_martingale_validation_accepts_matching_esscher_target() {
        let driver_parameters = NigSharedPlusShiftConfiguration::try_new(2.3, -0.8, 0.7, 0.1, -0.2)
            .expect("valid shared-plus-shift payload should construct")
            .derive_driver_parameters()
            .expect("valid shared-plus-shift payload should derive drivers");
        let return_decomposition = ReturnDecomposition::try_new(ReturnDecompositionInput {
            a_plus: 1.1,
            a_minus: 0.7,
            gap: 0.0,
        })
        .expect("valid return decomposition payload should construct");

        let theta = -0.05;
        let maturity_interval = 0.5;

        let buy_side_log_moment_exponent =
            transform_nig_characteristic_exponent_under_esscher_tilt(
                NigEsscherTransformationInput::new(
                    driver_parameters.buy_side_parameters,
                    ComplexScalar::new(0.0, -1.0),
                    theta,
                ),
            )
            .expect("buy-side transformed exponent should construct")
            .transformed_exponent;
        let sell_side_log_moment_exponent =
            transform_nig_characteristic_exponent_under_esscher_tilt(
                NigEsscherTransformationInput::new(
                    driver_parameters.sell_side_parameters,
                    ComplexScalar::new(0.0, 1.0),
                    theta,
                ),
            )
            .expect("sell-side transformed exponent should construct")
            .transformed_exponent;
        let combined = buy_side_log_moment_exponent
            .scale(return_decomposition.a_plus)
            .add(sell_side_log_moment_exponent.scale(return_decomposition.a_minus));

        let market_inputs = VanillaOptionPricingMarketInputs::try_new(
            101.0,
            100.0,
            maturity_interval,
            combined.real / maturity_interval,
            0.0,
        )
        .expect("market inputs should construct");
        let tolerance = combined.imaginary.abs() + 1.0e-12;

        let report = validate_pricing_martingale_condition_under_esscher_tilt(
            driver_parameters,
            return_decomposition,
            market_inputs,
            theta,
            tolerance,
        )
        .expect("matching target should satisfy martingale validation");

        assert_eq!(
            report.policy,
            PRICING_MARTINGALE_CONDITION_VALIDATION_POLICY
        );
        assert!(report.martingale_condition.is_satisfied);
        assert!(report.martingale_condition.residual_norm <= report.martingale_condition.tolerance);
    }

    #[test]
    fn pricing_martingale_validation_rejects_mismatched_target() {
        let driver_parameters = NigSharedPlusShiftConfiguration::try_new(2.3, -0.8, 0.7, 0.1, -0.2)
            .expect("valid shared-plus-shift payload should construct")
            .derive_driver_parameters()
            .expect("valid shared-plus-shift payload should derive drivers");
        let return_decomposition = ReturnDecomposition::try_new(ReturnDecompositionInput {
            a_plus: 0.9,
            a_minus: 1.2,
            gap: 0.0,
        })
        .expect("valid return decomposition payload should construct");

        let error = validate_pricing_martingale_condition_under_esscher_tilt(
            driver_parameters,
            return_decomposition,
            VanillaOptionPricingMarketInputs::try_new(101.0, 100.0, 0.5, 0.25, 0.01)
                .expect("market inputs should construct"),
            -0.05,
            1.0e-8,
        )
        .expect_err("mismatched target should fail martingale validation");

        assert!(matches!(
            error,
            PricingMartingaleConditionValidationError::MartingaleConditionViolation {
                residual_norm,
                tolerance,
                ..
            } if residual_norm > tolerance
        ));
    }

    #[test]
    fn pricing_martingale_validation_rejects_invalid_transform_parameter() {
        let driver_parameters = NigSharedPlusShiftConfiguration::try_new(2.3, -0.8, 0.7, 0.1, -0.2)
            .expect("valid shared-plus-shift payload should construct")
            .derive_driver_parameters()
            .expect("valid shared-plus-shift payload should derive drivers");
        let return_decomposition = ReturnDecomposition::try_new(ReturnDecompositionInput {
            a_plus: 1.0,
            a_minus: 1.0,
            gap: 0.0,
        })
        .expect("valid return decomposition payload should construct");

        let error = validate_pricing_martingale_condition_under_esscher_tilt(
            driver_parameters,
            return_decomposition,
            VanillaOptionPricingMarketInputs::try_new(101.0, 100.0, 0.5, 0.03, 0.01)
                .expect("market inputs should construct"),
            f64::NAN,
            1.0e-6,
        )
        .expect_err("non-finite theta should fail transform validation");

        assert!(matches!(
            error,
            PricingMartingaleConditionValidationError::BuySideEsscherTransformFailure { .. }
        ));
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

    fn black_scholes_log_return_characteristic_function(
        market_inputs: VanillaOptionPricingMarketInputs,
        volatility: f64,
        argument: ComplexScalar,
    ) -> ComplexScalar {
        let variance = volatility * volatility * market_inputs.maturity_interval;
        let drift = (market_inputs.risk_free_rate
            - market_inputs.dividend_yield
            - 0.5 * volatility * volatility)
            * market_inputs.maturity_interval;
        let imaginary_unit_term = ComplexScalar::new(-argument.imaginary, argument.real);
        let drift_term = imaginary_unit_term.scale(drift);
        let variance_term = argument.square().scale(-0.5 * variance);

        super::complex_exponential(drift_term.add(variance_term))
    }

    fn black_scholes_price(
        market_inputs: VanillaOptionPricingMarketInputs,
        volatility: f64,
        payoff_type: VanillaOptionPayoffType,
    ) -> f64 {
        let sigma_square_root_time = volatility * market_inputs.maturity_interval.sqrt();
        let log_moneyness = (market_inputs.spot / market_inputs.strike).ln();
        let carry = (market_inputs.risk_free_rate - market_inputs.dividend_yield)
            * market_inputs.maturity_interval;
        let d1 = (log_moneyness + carry + 0.5 * sigma_square_root_time * sigma_square_root_time)
            / sigma_square_root_time;
        let d2 = d1 - sigma_square_root_time;
        let discounted_spot = market_inputs.spot
            * (-market_inputs.dividend_yield * market_inputs.maturity_interval).exp();
        let discounted_strike = market_inputs.strike
            * (-market_inputs.risk_free_rate * market_inputs.maturity_interval).exp();

        match payoff_type {
            VanillaOptionPayoffType::Call => {
                discounted_spot * standard_normal_cdf(d1)
                    - discounted_strike * standard_normal_cdf(d2)
            }
            VanillaOptionPayoffType::Put => {
                discounted_strike * standard_normal_cdf(-d2)
                    - discounted_spot * standard_normal_cdf(-d1)
            }
        }
    }

    fn standard_normal_cdf(value: f64) -> f64 {
        if value < 0.0 {
            return 1.0 - standard_normal_cdf(-value);
        }

        let polynomial_scale = 1.0 / (1.0 + 0.231_641_9 * value);
        let polynomial_value = polynomial_scale
            * (0.319_381_53
                + polynomial_scale
                    * (-0.356_563_782
                        + polynomial_scale
                            * (1.781_477_937
                                + polynomial_scale
                                    * (-1.821_255_978 + polynomial_scale * 1.330_274_429))));
        let density = 0.398_942_280_401_432_7 * (-0.5 * value * value).exp();

        1.0 - density * polynomial_value
    }

    fn assert_complex_close(observed: ComplexScalar, expected: ComplexScalar, tolerance: f64) {
        let difference = observed.subtract(expected).norm();
        assert!(
            difference <= tolerance,
            "difference {difference} exceeded tolerance {tolerance} for observed {observed:?} and expected {expected:?}"
        );
    }

    fn oracle_output_with_transform_value(
        transform_value: ComplexScalar,
    ) -> ExponentialAffineTransformOutput {
        ExponentialAffineTransformOutput::try_new(
            ComplexScalar::new(0.0, 0.0),
            ExponentialAffineTransformDVector::new(
                ComplexScalar::new(0.0, 0.0),
                ComplexScalar::new(0.0, 0.0),
                ComplexScalar::new(0.0, 0.0),
                ComplexScalar::new(0.0, 0.0),
            ),
            transform_value,
        )
        .expect("oracle output helper payload should validate")
    }
}
