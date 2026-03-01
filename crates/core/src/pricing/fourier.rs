use std::error::Error;
use std::f64::consts::PI;
use std::fmt::{Display, Formatter};

use serde::Deserialize;
use serde::Serialize;

use crate::complex_math::ComplexScalar;
use crate::complex_math::complex_multiply;

use super::constants::{
    DEFAULT_FOURIER_INVERSION_CONVERGENCE_ABSOLUTE_TOLERANCE,
    DEFAULT_FOURIER_INVERSION_CONVERGENCE_RELATIVE_TOLERANCE,
    DEFAULT_FOURIER_INVERSION_DAMPING_FACTOR, DEFAULT_FOURIER_INVERSION_STEP_COUNT,
    DEFAULT_FOURIER_INVERSION_TERMINAL_INTEGRAND_ABSOLUTE_TOLERANCE,
    DEFAULT_FOURIER_INVERSION_TERMINAL_INTEGRAND_RATIO_TOLERANCE,
    DEFAULT_FOURIER_INVERSION_UPPER_INTEGRATION_BOUND, FOURIER_INVERSION_CONTOUR_POLICY,
    FOURIER_INVERSION_CONVERGENCE_CHECK_POLICY, FOURIER_INVERSION_DIAGNOSTICS_SCHEMA_VERSION,
    FOURIER_INVERSION_ENGINE_POLICY, FOURIER_INVERSION_INTEGRATION_STABILITY_POLICY,
    FOURIER_INVERSION_NUMERIC_QUALITY_GATE_POLICY, FOURIER_INVERSION_NUMERIC_SETTINGS_POLICY,
    FOURIER_INVERSION_TRUNCATION_CHECK_POLICY, FOURIER_NON_NEGATIVE_PRICE_CLAMP_TOLERANCE,
};
use super::contracts::{
    VanillaOptionPayoffConfiguration, VanillaOptionPayoffType, VanillaOptionPricingMarketInputs,
    VanillaOptionPricingMarketInputsError,
};
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
