use std::error::Error;
use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};

use janiform_affine_oracle::affine_oracle::{
    ExponentialAffineTransformNewsIntensityInput, ExponentialAffineTransformNewsIntensityInputError,
};
use janiform_foundations::complex_math::ComplexScalar;
use janiform_model_kernels::hawkes::HawkesExcitationState;
use janiform_model_kernels::nig::NigSharedPlusShiftConfigurationError;

use super::black_scholes::{intraday_discount_factor, intraday_forward_price};
use super::constants::{
    PRICING_INTEGRATION_QUALITY_DIAGNOSTICS_SCHEMA_VERSION,
    PRICING_INTEGRATION_QUALITY_GATE_POLICY, SPECTRAL_PRICING_COLLOCATION_POLICY,
    SPECTRAL_PRICING_DEVIATION_DOMAIN_POLICY, SPECTRAL_PRICING_INTEGRATION_COMPATIBILITY_POLICY,
};
use super::contracts::{
    PricingIntegrationNumericQualityDiagnostics, PricingIntegrationProjectionRule,
    PricingIntegrationQualityDiagnostics, PricingIntegrationQualityGate,
    SpectralPricingDiagnostics, VanillaOptionPricingDiagnostics,
    VanillaOptionPricingFailureDiagnostic, VanillaOptionPricingFailureStage,
    VanillaOptionPricingOptionalOutputSelection, VanillaOptionPricingRequest,
    VanillaOptionPricingRequestError, VanillaOptionPricingResponse,
    VanillaOptionPricingResponseError, VanillaOptionPricingRunResult,
    VanillaOptionPricingRunResultBuildError, VanillaOptionPricingRunResultBuildInput,
    VanillaOptionPricingStatus, build_vanilla_option_pricing_run_result,
};
use super::interval_schedule::PricingIntervalSchedule;
use super::progress::PricingProgressEvent;

/// One-interval pricing factors required to execute the canonical vanilla pricing workflow.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct VanillaOptionPricingIntervalFactors {
    pub s_h: f64,
    pub nu_h_known: f64,
    pub nu_h_surprise: f64,
}

impl VanillaOptionPricingIntervalFactors {
    pub const fn new(s_h: f64, nu_h_known: f64, nu_h_surprise: f64) -> Self {
        Self {
            s_h,
            nu_h_known,
            nu_h_surprise,
        }
    }

    pub fn try_new(
        s_h: f64,
        nu_h_known: f64,
        nu_h_surprise: f64,
    ) -> Result<Self, VanillaOptionPricingIntervalFactorsError> {
        let factors = Self::new(s_h, nu_h_known, nu_h_surprise);
        factors.validate()?;

        Ok(factors)
    }

    pub fn validate(self) -> Result<(), VanillaOptionPricingIntervalFactorsError> {
        if !self.s_h.is_finite() || self.s_h < 0.0 {
            return Err(
                VanillaOptionPricingIntervalFactorsError::InvalidSeasonalityFactor {
                    value: self.s_h,
                },
            );
        }

        ExponentialAffineTransformNewsIntensityInput::try_new(self.nu_h_known, self.nu_h_surprise)
            .map_err(
                |source| VanillaOptionPricingIntervalFactorsError::InvalidNewsIntensity { source },
            )?;

        Ok(())
    }

    pub fn news_intensity_input(
        self,
    ) -> Result<
        ExponentialAffineTransformNewsIntensityInput,
        VanillaOptionPricingIntervalFactorsError,
    > {
        ExponentialAffineTransformNewsIntensityInput::try_new(self.nu_h_known, self.nu_h_surprise)
            .map_err(
                |source| VanillaOptionPricingIntervalFactorsError::InvalidNewsIntensity { source },
            )
    }

    pub fn total_news_intensity(self) -> f64 {
        self.nu_h_known + self.nu_h_surprise
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum VanillaOptionPricingIntervalFactorsError {
    InvalidSeasonalityFactor {
        value: f64,
    },
    InvalidNewsIntensity {
        source: ExponentialAffineTransformNewsIntensityInputError,
    },
}

impl Display for VanillaOptionPricingIntervalFactorsError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidSeasonalityFactor { value } => write!(
                formatter,
                "interval_factors.s_h must be finite and non-negative, found {value}"
            ),
            Self::InvalidNewsIntensity { source } => {
                write!(
                    formatter,
                    "interval_factors news intensity is invalid: {source}"
                )
            }
        }
    }
}

impl Error for VanillaOptionPricingIntervalFactorsError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidNewsIntensity { source } => Some(source),
            _ => None,
        }
    }
}

/// Canonical execution input for the orchestrated vanilla pricing workflow.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VanillaOptionPricingExecutionInput {
    pub request: VanillaOptionPricingRequest,
    pub runtime_interval_schedule: PricingIntervalSchedule,
    pub interval_factors: Vec<VanillaOptionPricingIntervalFactors>,
    pub optional_output_selection: VanillaOptionPricingOptionalOutputSelection,
}

impl VanillaOptionPricingExecutionInput {
    pub fn new(
        request: VanillaOptionPricingRequest,
        runtime_interval_schedule: PricingIntervalSchedule,
        interval_factors: Vec<VanillaOptionPricingIntervalFactors>,
        optional_output_selection: VanillaOptionPricingOptionalOutputSelection,
    ) -> Self {
        Self {
            request,
            runtime_interval_schedule,
            interval_factors,
            optional_output_selection,
        }
    }

    pub fn try_new(
        request: VanillaOptionPricingRequest,
        runtime_interval_schedule: PricingIntervalSchedule,
        interval_factors: Vec<VanillaOptionPricingIntervalFactors>,
        optional_output_selection: VanillaOptionPricingOptionalOutputSelection,
    ) -> Result<Self, VanillaOptionPricingExecutionInputError> {
        let execution_input = Self::new(
            request,
            runtime_interval_schedule,
            interval_factors,
            optional_output_selection,
        );

        execution_input.validate()?;

        Ok(execution_input)
    }

    pub fn validate(&self) -> Result<(), VanillaOptionPricingExecutionInputError> {
        self.request
            .validate()
            .map_err(|source| VanillaOptionPricingExecutionInputError::InvalidRequest { source })?;

        if self.interval_factors.is_empty() {
            return Err(VanillaOptionPricingExecutionInputError::EmptyIntervalFactors);
        }

        for interval_factors in &self.interval_factors {
            interval_factors.validate().map_err(|source| {
                VanillaOptionPricingExecutionInputError::InvalidIntervalFactors { source }
            })?;
        }

        if self.interval_factors.len() != self.runtime_interval_schedule.interval_count() {
            return Err(
                VanillaOptionPricingExecutionInputError::IntervalFactorShapeMismatch {
                    expected_interval_factor_count: self.runtime_interval_schedule.interval_count(),
                    found_interval_factor_count: self.interval_factors.len(),
                },
            );
        }

        let first_runtime_interval_delta =
            self.runtime_interval_schedule
                .interval_delta_hours(0)
                .ok_or(VanillaOptionPricingExecutionInputError::EmptyRuntimeIntervalSchedule)?;
        if self.request.interval_delta != first_runtime_interval_delta {
            return Err(
                VanillaOptionPricingExecutionInputError::InitialIntervalDeltaMismatch {
                    requested_interval_delta: self.request.interval_delta,
                    runtime_interval_delta: first_runtime_interval_delta,
                },
            );
        }

        let maturity_interval_from_runtime_grid = self
            .runtime_interval_schedule
            .maturity_interval_hours(self.runtime_interval_schedule.interval_count() - 1)
            .map_err(|message| {
                VanillaOptionPricingExecutionInputError::InvalidRuntimeIntervalSchedule {
                    message: message.to_string(),
                }
            })?;

        let requested_maturity_interval = self
            .request
            .market_inputs
            .single_maturity_interval()
            .map_err(
                |source| VanillaOptionPricingExecutionInputError::InvalidRequest {
                    source: VanillaOptionPricingRequestError::InvalidMarketInputs { source },
                },
            )?;

        if maturity_interval_from_runtime_grid != requested_maturity_interval {
            return Err(
                VanillaOptionPricingExecutionInputError::MaturityIntervalMismatch {
                    requested_maturity_interval,
                    runtime_grid_maturity_interval: maturity_interval_from_runtime_grid,
                },
            );
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum VanillaOptionPricingExecutionInputError {
    InvalidRequest {
        source: VanillaOptionPricingRequestError,
    },
    EmptyIntervalFactors,
    InvalidIntervalFactors {
        source: VanillaOptionPricingIntervalFactorsError,
    },
    EmptyRuntimeIntervalSchedule,
    InvalidRuntimeIntervalSchedule {
        message: String,
    },
    InitialIntervalDeltaMismatch {
        requested_interval_delta: f64,
        runtime_interval_delta: f64,
    },
    IntervalFactorShapeMismatch {
        expected_interval_factor_count: usize,
        found_interval_factor_count: usize,
    },
    MaturityIntervalMismatch {
        requested_maturity_interval: f64,
        runtime_grid_maturity_interval: f64,
    },
}

impl Display for VanillaOptionPricingExecutionInputError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidRequest { source } => write!(
                formatter,
                "pricing execution request payload is invalid: {source}"
            ),
            Self::EmptyIntervalFactors => write!(
                formatter,
                "pricing execution interval_factors payload must contain at least one interval"
            ),
            Self::InvalidIntervalFactors { source } => write!(
                formatter,
                "pricing execution interval_factors payload is invalid: {source}"
            ),
            Self::EmptyRuntimeIntervalSchedule => write!(
                formatter,
                "pricing execution runtime_interval_schedule must contain at least one interval"
            ),
            Self::InvalidRuntimeIntervalSchedule { message } => write!(
                formatter,
                "pricing execution runtime_interval_schedule is invalid: {message}"
            ),
            Self::InitialIntervalDeltaMismatch {
                requested_interval_delta,
                runtime_interval_delta,
            } => write!(
                formatter,
                "request.interval_delta={requested_interval_delta} must equal the first runtime interval delta {runtime_interval_delta}"
            ),
            Self::IntervalFactorShapeMismatch {
                expected_interval_factor_count,
                found_interval_factor_count,
            } => write!(
                formatter,
                "pricing execution interval_factors must contain exactly {expected_interval_factor_count} entries implied by runtime_interval_schedule, found {found_interval_factor_count}"
            ),
            Self::MaturityIntervalMismatch {
                requested_maturity_interval,
                runtime_grid_maturity_interval,
            } => write!(
                formatter,
                "pricing execution maturity_interval {requested_maturity_interval} must equal interval_factors.len() * request.interval_delta = {runtime_grid_maturity_interval}"
            ),
        }
    }
}

impl Error for VanillaOptionPricingExecutionInputError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidRequest { source } => Some(source),
            Self::InvalidIntervalFactors { source } => Some(source),
            _ => None,
        }
    }
}

/// Canonical native runner for the public vanilla pricing workflow.
pub fn run_vanilla_option_pricing(
    execution_input: VanillaOptionPricingExecutionInput,
) -> Result<VanillaOptionPricingRunResult, VanillaOptionPricingRunError> {
    run_vanilla_option_pricing_with_observer(execution_input, None)
}

pub fn run_vanilla_option_pricing_with_observer(
    execution_input: VanillaOptionPricingExecutionInput,
    _observer: Option<&mut dyn FnMut(PricingProgressEvent)>,
) -> Result<VanillaOptionPricingRunResult, VanillaOptionPricingRunError> {
    execution_input.validate().map_err(|source| {
        VanillaOptionPricingRunError::InvalidExecutionInput {
            source: Box::new(source),
        }
    })?;

    let pricing_request = execution_input.request.clone();
    let output_selection = execution_input.optional_output_selection;

    let market_inputs_for_calibration_hooks = pricing_request.market_inputs.clone();

    let successful_result = super::spectral::run_single_option_pricing(&execution_input);

    let (response, integration_quality_diagnostics, spectral_diagnostics) = match successful_result
    {
        Ok(spectral_outcome) => {
            let maturity_interval = pricing_request
                .market_inputs
                .single_maturity_interval()
                .map_err(
                    |source| VanillaOptionPricingRunError::InvalidExecutionInput {
                        source: Box::new(VanillaOptionPricingExecutionInputError::InvalidRequest {
                            source: VanillaOptionPricingRequestError::InvalidMarketInputs {
                                source,
                            },
                        }),
                    },
                )?;

            let forward_price = intraday_forward_price(
                pricing_request.market_inputs.spot,
                pricing_request.market_inputs.risk_free_rate,
                pricing_request.market_inputs.dividend_yield,
                maturity_interval,
            )
            .ok_or_else(|| VanillaOptionPricingRunError::InternalNumericsFailure {
                message: "pricing diagnostics require a finite positive forward price".to_string(),
            })?;

            let discount_factor = intraday_discount_factor(
                pricing_request.market_inputs.risk_free_rate,
                maturity_interval,
            )
            .ok_or_else(|| VanillaOptionPricingRunError::InternalNumericsFailure {
                message: "pricing diagnostics require a finite positive discount factor"
                    .to_string(),
            })?;

            let response = VanillaOptionPricingResponse::try_new(
                VanillaOptionPricingStatus::Succeeded,
                None,
                Some(spectral_outcome.price),
                Some(
                    VanillaOptionPricingDiagnostics::try_new(
                        forward_price,
                        discount_factor,
                        maturity_interval,
                        hawkes_boundary_state_norm(
                            pricing_request.boundary_input.hawkes_boundary_state,
                        ),
                    )
                    .map_err(|source| {
                        VanillaOptionPricingRunError::InvalidResponseDiagnostics {
                            source: Box::new(source),
                        }
                    })?,
                ),
                spectral_outcome.implied_volatility,
                Some(spectral_outcome.implied_volatility_inversion_status),
            )
            .map_err(|source| VanillaOptionPricingRunError::InvalidResponse {
                source: Box::new(source),
            })?;

            (
                response,
                Some(build_spectral_compatibility_integration_diagnostics(
                    spectral_outcome.price,
                    maturity_interval,
                    &spectral_outcome.spectral_diagnostics,
                )),
                Some(spectral_outcome.spectral_diagnostics),
            )
        }
        Err(error) => {
            let response = build_failed_pricing_response(error)?;
            (response, None, None)
        }
    };

    build_vanilla_option_pricing_run_result(VanillaOptionPricingRunResultBuildInput {
        response,
        output_selection,
        market_inputs_for_calibration_hooks: Some(market_inputs_for_calibration_hooks),
        transform_diagnostics_source: None,
        integration_quality_source: integration_quality_diagnostics,
        spectral_source: spectral_diagnostics,
        runtime_interval_schedule: execution_input.runtime_interval_schedule.clone(),
        maturity_terminal_interval_indices: vec![
            execution_input.runtime_interval_schedule.interval_count() - 1,
        ],
    })
    .map_err(|source| VanillaOptionPricingRunError::InvalidRunResult {
        source: Box::new(source),
    })
}

fn hawkes_boundary_state_norm(boundary_state: HawkesExcitationState) -> f64 {
    (boundary_state.y_plus_plus * boundary_state.y_plus_plus
        + boundary_state.y_plus_minus * boundary_state.y_plus_minus
        + boundary_state.y_minus_plus * boundary_state.y_minus_plus
        + boundary_state.y_minus_minus * boundary_state.y_minus_minus)
        .sqrt()
}

pub(crate) fn build_spectral_compatibility_integration_diagnostics(
    price: f64,
    maturity_interval: f64,
    spectral_diagnostics: &SpectralPricingDiagnostics,
) -> PricingIntegrationQualityDiagnostics {
    let domain_width = spectral_diagnostics.deviation_domain_upper_bound
        - spectral_diagnostics.deviation_domain_lower_bound;

    let integration_step_count = spectral_diagnostics.collocation_node_count;

    let integration_interval_width = domain_width / integration_step_count as f64;

    let terminal_abs_integrand = 0.0;

    PricingIntegrationQualityDiagnostics {
        schema_version: PRICING_INTEGRATION_QUALITY_DIAGNOSTICS_SCHEMA_VERSION,
        engine_policy: SPECTRAL_PRICING_INTEGRATION_COMPATIBILITY_POLICY,
        domain_policy: SPECTRAL_PRICING_DEVIATION_DOMAIN_POLICY,
        projection_policy: SPECTRAL_PRICING_COLLOCATION_POLICY,
        stability_policy: SPECTRAL_PRICING_INTEGRATION_COMPATIBILITY_POLICY,
        projection_rule: PricingIntegrationProjectionRule::SpectralCosCollocation,
        integration_upper_bound: domain_width,
        integration_step_count,
        integration_interval_width,
        integrand_evaluation_count: integration_step_count,
        integral_estimate: price,
        absolute_integral_estimate: price.abs(),
        max_abs_integrand: price.abs(),
        terminal_abs_integrand,
        discounted_projection_price: price,
        numeric_quality_diagnostics: PricingIntegrationNumericQualityDiagnostics {
            stability_policy: SPECTRAL_PRICING_INTEGRATION_COMPATIBILITY_POLICY,
            convergence_check_policy: SPECTRAL_PRICING_COLLOCATION_POLICY,
            truncation_check_policy: SPECTRAL_PRICING_DEVIATION_DOMAIN_POLICY,
            quality_gate_policy: PRICING_INTEGRATION_QUALITY_GATE_POLICY,
            quality_gate: PricingIntegrationQualityGate::DiagnosticsOnly,
            convergence_absolute_tolerance: maturity_interval,
            convergence_relative_tolerance: 1.0,
            terminal_integrand_absolute_tolerance: maturity_interval,
            terminal_integrand_ratio_tolerance: 1.0,
            coarse_integral_estimate: price,
            convergence_absolute_difference: 0.0,
            convergence_relative_difference: 0.0,
            terminal_integrand_absolute_value: terminal_abs_integrand,
            terminal_integrand_ratio: 0.0,
            convergence_check_passed: true,
            truncation_check_passed: true,
            insufficient_convergence_resolution: false,
            numeric_quality_is_poor: false,
        },
    }
}

fn build_failed_pricing_response(
    error: VanillaOptionPricingRunError,
) -> Result<VanillaOptionPricingResponse, VanillaOptionPricingRunError> {
    match error {
        VanillaOptionPricingRunError::InvalidExecutionInput { .. }
        | VanillaOptionPricingRunError::InvalidDriverParameters { .. } => Err(error),
        other => {
            let failure_diagnostic = pricing_failure_diagnostic_from_run_error(&other);
            VanillaOptionPricingResponse::try_new(
                VanillaOptionPricingStatus::Failed,
                Some(failure_diagnostic),
                None,
                None,
                None,
                None,
            )
            .map_err(|source| VanillaOptionPricingRunError::InvalidResponse {
                source: Box::new(source),
            })
        }
    }
}

pub(crate) fn pricing_failure_diagnostic_from_run_error(
    error: &VanillaOptionPricingRunError,
) -> VanillaOptionPricingFailureDiagnostic {
    let stage = match error {
        VanillaOptionPricingRunError::InternalNumericsFailure { .. } => {
            VanillaOptionPricingFailureStage::PricingProjection
        }
        VanillaOptionPricingRunError::InvalidRiskNeutralCharacteristicFunction { .. } => {
            VanillaOptionPricingFailureStage::RiskNeutralCharacteristicFunction
        }
        VanillaOptionPricingRunError::TerminalAffineMeasureChangeFailure { .. } => {
            VanillaOptionPricingFailureStage::TerminalAffineMeasureChange
        }
        VanillaOptionPricingRunError::InvalidSpectralDiagnostics { .. } => {
            VanillaOptionPricingFailureStage::SpectralDiagnostics
        }
        VanillaOptionPricingRunError::InvalidResponseDiagnostics { .. } => {
            VanillaOptionPricingFailureStage::ResponseDiagnostics
        }
        VanillaOptionPricingRunError::InvalidResponse { .. } => {
            VanillaOptionPricingFailureStage::ResponseAssembly
        }
        VanillaOptionPricingRunError::InvalidRunResult { .. } => {
            VanillaOptionPricingFailureStage::RunResultAssembly
        }
        VanillaOptionPricingRunError::InvalidExecutionInput { .. } => {
            VanillaOptionPricingFailureStage::NodeSetup
        }
        VanillaOptionPricingRunError::InvalidDriverParameters { .. } => {
            VanillaOptionPricingFailureStage::NodeSetup
        }
    };

    VanillaOptionPricingFailureDiagnostic::new(stage, error.to_string())
}

#[derive(Debug, Clone, PartialEq)]
pub enum VanillaOptionPricingRunError {
    InvalidExecutionInput {
        source: Box<VanillaOptionPricingExecutionInputError>,
    },
    InternalNumericsFailure {
        message: String,
    },
    InvalidDriverParameters {
        source: Box<NigSharedPlusShiftConfigurationError>,
    },
    InvalidRiskNeutralCharacteristicFunction {
        numerator: ComplexScalar,
        denominator: ComplexScalar,
    },
    TerminalAffineMeasureChangeFailure {
        interval_index: usize,
        maturity_interval: f64,
        message: String,
    },
    InvalidSpectralDiagnostics {
        source: Box<super::contracts::SpectralPricingDiagnosticsError>,
    },
    InvalidResponseDiagnostics {
        source: Box<super::contracts::VanillaOptionPricingDiagnosticsError>,
    },
    InvalidResponse {
        source: Box<VanillaOptionPricingResponseError>,
    },
    InvalidRunResult {
        source: Box<VanillaOptionPricingRunResultBuildError>,
    },
}

impl Display for VanillaOptionPricingRunError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidExecutionInput { source } => {
                write!(formatter, "pricing execution input is invalid: {source}")
            }
            Self::InternalNumericsFailure { message } => {
                write!(formatter, "internal pricing numerics failed: {message}")
            }
            Self::InvalidDriverParameters { source } => {
                write!(
                    formatter,
                    "pricing request NIG driver parameters are invalid: {source}"
                )
            }
            Self::InvalidRiskNeutralCharacteristicFunction {
                numerator,
                denominator,
            } => write!(
                formatter,
                "state-dependent risk-neutral characteristic function became invalid with numerator {numerator:?} and denominator {denominator:?}"
            ),
            Self::TerminalAffineMeasureChangeFailure {
                interval_index,
                maturity_interval,
                message,
            } => write!(
                formatter,
                "terminal affine measure-change solve failed for interval_index={interval_index} and maturity_interval={maturity_interval}: {message}"
            ),
            Self::InvalidSpectralDiagnostics { source } => {
                write!(formatter, "spectral diagnostics are invalid: {source}")
            }
            Self::InvalidResponseDiagnostics { source } => write!(
                formatter,
                "pricing run produced invalid response diagnostics: {source}"
            ),
            Self::InvalidResponse { source } => {
                write!(
                    formatter,
                    "pricing run produced an invalid response: {source}"
                )
            }
            Self::InvalidRunResult { source } => {
                write!(formatter, "pricing run result payload is invalid: {source}")
            }
        }
    }
}

impl Error for VanillaOptionPricingRunError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidExecutionInput { source } => Some(source.as_ref()),
            Self::InvalidDriverParameters { source } => Some(source.as_ref()),
            Self::InvalidSpectralDiagnostics { source } => Some(source.as_ref()),
            Self::InvalidResponseDiagnostics { source } => Some(source.as_ref()),
            Self::InvalidResponse { source } => Some(source.as_ref()),
            Self::InvalidRunResult { source } => Some(source.as_ref()),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct VanillaOptionPricingIntervalFactorsPayload {
    s_h: f64,
    nu_h_known: f64,
    nu_h_surprise: f64,
}

impl<'de> Deserialize<'de> for VanillaOptionPricingIntervalFactors {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let payload = VanillaOptionPricingIntervalFactorsPayload::deserialize(deserializer)?;

        Self::try_new(payload.s_h, payload.nu_h_known, payload.nu_h_surprise)
            .map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct VanillaOptionPricingExecutionInputPayload {
    request: VanillaOptionPricingRequest,
    runtime_interval_schedule: PricingIntervalSchedule,
    interval_factors: Vec<VanillaOptionPricingIntervalFactors>,
    #[serde(default)]
    optional_output_selection: VanillaOptionPricingOptionalOutputSelection,
}

impl<'de> Deserialize<'de> for VanillaOptionPricingExecutionInput {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let payload = VanillaOptionPricingExecutionInputPayload::deserialize(deserializer)?;

        Self::try_new(
            payload.request,
            payload.runtime_interval_schedule,
            payload.interval_factors,
            payload.optional_output_selection,
        )
        .map_err(serde::de::Error::custom)
    }
}
