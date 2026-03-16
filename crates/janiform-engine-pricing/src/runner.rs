use std::cell::RefCell;
use std::error::Error;
use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};

use janiform_affine_oracle::affine_oracle::{
    ExponentialAffineTransformIntegrationResult, ExponentialAffineTransformIntegrationStatus,
    ExponentialAffineTransformNewsIntensityInput,
    ExponentialAffineTransformNewsIntensityInputError, ExponentialAffineTransformSolverError,
};
use janiform_foundations::complex_math::ComplexScalar;
use janiform_model_kernels::nig::NigSharedPlusShiftConfigurationError;

use super::characteristic::{
    PricingTransformInputBuildError, build_pricing_transform_input,
    compose_total_return_characteristic_function_from_oracle_output,
    solve_exponential_affine_transform,
};
use super::contracts::{
    VanillaOptionPricingDiagnostics, VanillaOptionPricingOptionalOutputSelection,
    VanillaOptionPricingRequest, VanillaOptionPricingRequestError, VanillaOptionPricingResponse,
    VanillaOptionPricingResponseError, VanillaOptionPricingRunResult,
    VanillaOptionPricingRunResultBuildError, build_vanilla_option_pricing_run_result,
};
use super::fourier::{
    FourierInversionEngineSettings, FourierInversionPricingError, FourierInversionPricingResult,
    price_vanilla_option_via_fourier_inversion,
};

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
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct VanillaOptionPricingExecutionInput {
    pub request: VanillaOptionPricingRequest,
    pub interval_factors: VanillaOptionPricingIntervalFactors,
    pub optional_output_selection: VanillaOptionPricingOptionalOutputSelection,
}

impl VanillaOptionPricingExecutionInput {
    pub const fn new(
        request: VanillaOptionPricingRequest,
        interval_factors: VanillaOptionPricingIntervalFactors,
        optional_output_selection: VanillaOptionPricingOptionalOutputSelection,
    ) -> Self {
        Self {
            request,
            interval_factors,
            optional_output_selection,
        }
    }

    pub fn try_new(
        request: VanillaOptionPricingRequest,
        interval_factors: VanillaOptionPricingIntervalFactors,
        optional_output_selection: VanillaOptionPricingOptionalOutputSelection,
    ) -> Result<Self, VanillaOptionPricingExecutionInputError> {
        let execution_input = Self::new(request, interval_factors, optional_output_selection);
        execution_input.validate()?;
        Ok(execution_input)
    }

    pub fn validate(self) -> Result<(), VanillaOptionPricingExecutionInputError> {
        self.request
            .validate()
            .map_err(|source| VanillaOptionPricingExecutionInputError::InvalidRequest { source })?;

        self.interval_factors.validate().map_err(|source| {
            VanillaOptionPricingExecutionInputError::InvalidIntervalFactors { source }
        })?;

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum VanillaOptionPricingExecutionInputError {
    InvalidRequest {
        source: VanillaOptionPricingRequestError,
    },
    InvalidIntervalFactors {
        source: VanillaOptionPricingIntervalFactorsError,
    },
}

impl Display for VanillaOptionPricingExecutionInputError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidRequest { source } => write!(
                formatter,
                "pricing execution request payload is invalid: {source}"
            ),
            Self::InvalidIntervalFactors { source } => write!(
                formatter,
                "pricing execution interval_factors payload is invalid: {source}"
            ),
        }
    }
}

impl Error for VanillaOptionPricingExecutionInputError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidRequest { source } => Some(source),
            Self::InvalidIntervalFactors { source } => Some(source),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct PricingCharacteristicFunctionEvaluation {
    value: ComplexScalar,
    transform_diagnostics: ExponentialAffineTransformIntegrationResult,
}

/// Canonical native runner for the public vanilla pricing workflow.
pub fn run_vanilla_option_pricing(
    execution_input: VanillaOptionPricingExecutionInput,
) -> Result<VanillaOptionPricingRunResult, VanillaOptionPricingRunError> {
    execution_input
        .validate()
        .map_err(|source| VanillaOptionPricingRunError::InvalidExecutionInput { source })?;

    let pricing_request = execution_input.request;
    let market_inputs = pricing_request.market_inputs;
    let payoff = pricing_request.payoff;
    let interval_factors = execution_input.interval_factors;
    let output_selection = execution_input.optional_output_selection;
    let driver_parameters = pricing_request
        .model_parameters
        .nig_shared_plus_shift_configuration
        .derive_driver_parameters()
        .map_err(|source| VanillaOptionPricingRunError::InvalidDriverParameters { source })?;
    let news_intensity = interval_factors.news_intensity_input().map_err(|source| {
        VanillaOptionPricingRunError::InvalidExecutionInput {
            source: VanillaOptionPricingExecutionInputError::InvalidIntervalFactors { source },
        }
    })?;
    let representative_transform_diagnostics: RefCell<
        Option<ExponentialAffineTransformIntegrationResult>,
    > = RefCell::new(None);
    let deferred_characteristic_function_error: RefCell<Option<VanillaOptionPricingRunError>> =
        RefCell::new(None);

    let pricing_result = price_vanilla_option_via_fourier_inversion(
        market_inputs,
        payoff,
        FourierInversionEngineSettings::default(),
        |argument| match evaluate_pricing_log_return_characteristic_function(
            pricing_request,
            driver_parameters,
            interval_factors,
            news_intensity,
            argument,
        ) {
            Ok(evaluation) => {
                if representative_transform_diagnostics.borrow().is_none() {
                    *representative_transform_diagnostics.borrow_mut() =
                        Some(evaluation.transform_diagnostics);
                }
                evaluation.value
            }
            Err(error) => {
                if deferred_characteristic_function_error.borrow().is_none() {
                    *deferred_characteristic_function_error.borrow_mut() = Some(error);
                }
                ComplexScalar::new(f64::NAN, f64::NAN)
            }
        },
    );

    if let Some(error) = deferred_characteristic_function_error.into_inner() {
        return Err(error);
    }

    let pricing_result = pricing_result
        .map_err(|source| VanillaOptionPricingRunError::FourierPricingFailure { source })?;
    let response = build_pricing_response(pricing_request, pricing_result)?;
    let representative_transform_diagnostics = representative_transform_diagnostics.into_inner();
    let transform_diagnostics_source = if output_selection.include_transform_diagnostics
        || output_selection.include_calibration_hooks
    {
        Some(
            representative_transform_diagnostics
                .ok_or(VanillaOptionPricingRunError::MissingRepresentativeTransformDiagnostics)?,
        )
    } else {
        None
    };

    build_vanilla_option_pricing_run_result(
        response,
        output_selection,
        Some(market_inputs),
        transform_diagnostics_source,
        Some(pricing_result.diagnostics),
    )
    .map_err(|source| VanillaOptionPricingRunError::InvalidRunResult { source })
}

fn build_pricing_response(
    pricing_request: VanillaOptionPricingRequest,
    pricing_result: FourierInversionPricingResult,
) -> Result<VanillaOptionPricingResponse, VanillaOptionPricingRunError> {
    let market_inputs = pricing_request.market_inputs;
    let forward_price = market_inputs.spot
        * ((market_inputs.risk_free_rate - market_inputs.dividend_yield)
            * market_inputs.maturity_interval)
            .exp();
    let discount_factor = (-market_inputs.risk_free_rate * market_inputs.maturity_interval).exp();
    let boundary_state = pricing_request.boundary_state;
    let boundary_state_norm = (boundary_state.y_plus_plus * boundary_state.y_plus_plus
        + boundary_state.y_plus_minus * boundary_state.y_plus_minus
        + boundary_state.y_minus_plus * boundary_state.y_minus_plus
        + boundary_state.y_minus_minus * boundary_state.y_minus_minus)
        .sqrt();

    let diagnostics = VanillaOptionPricingDiagnostics::try_new(
        forward_price,
        discount_factor,
        market_inputs.maturity_interval,
        boundary_state_norm,
    )
    .map_err(|source| VanillaOptionPricingRunError::InvalidResponseDiagnostics { source })?;

    VanillaOptionPricingResponse::try_new(pricing_result.price, diagnostics)
        .map_err(|source| VanillaOptionPricingRunError::InvalidResponse { source })
}

fn evaluate_pricing_log_return_characteristic_function(
    pricing_request: VanillaOptionPricingRequest,
    driver_parameters: janiform_model_kernels::nig::NigBuySellDriverParameters,
    interval_factors: VanillaOptionPricingIntervalFactors,
    news_intensity: ExponentialAffineTransformNewsIntensityInput,
    argument: ComplexScalar,
) -> Result<PricingCharacteristicFunctionEvaluation, VanillaOptionPricingRunError> {
    let buy_side_characteristic_exponent =
        driver_parameters.buy_side_characteristic_exponent(argument);
    let sell_side_characteristic_exponent =
        driver_parameters.sell_side_characteristic_exponent_for_return_composition(argument);

    let transform_input = build_pricing_transform_input(
        pricing_request,
        buy_side_characteristic_exponent.scale(-1.0),
        sell_side_characteristic_exponent.scale(-1.0),
        ComplexScalar::new(1.0, 0.0),
        ComplexScalar::new(1.0, 0.0),
        interval_factors.s_h,
        news_intensity,
    )
    .map_err(
        |source| VanillaOptionPricingRunError::TransformInputBuildFailure { argument, source },
    )?;
    let transform_diagnostics =
        solve_exponential_affine_transform(transform_input).map_err(|source| {
            VanillaOptionPricingRunError::TransformSolveFailure { argument, source }
        })?;
    let transform_output = transform_diagnostics.output.ok_or(
        VanillaOptionPricingRunError::MissingTransformOutput {
            argument,
            status: transform_diagnostics.status,
        },
    )?;

    let assembled =
        compose_total_return_characteristic_function_from_oracle_output(
            transform_output,
            ComplexScalar::new(1.0, 0.0),
            ComplexScalar::new(1.0, 0.0),
        )
        .map_err(|source| {
            VanillaOptionPricingRunError::CharacteristicFunctionAssemblyFailure { argument, source }
        })?;

    Ok(PricingCharacteristicFunctionEvaluation {
        value: assembled.combined_characteristic_factor,
        transform_diagnostics,
    })
}

#[derive(Debug, Clone, PartialEq)]
pub enum VanillaOptionPricingRunError {
    InvalidExecutionInput {
        source: VanillaOptionPricingExecutionInputError,
    },
    InvalidDriverParameters {
        source: NigSharedPlusShiftConfigurationError,
    },
    TransformInputBuildFailure {
        argument: ComplexScalar,
        source: PricingTransformInputBuildError,
    },
    TransformSolveFailure {
        argument: ComplexScalar,
        source: ExponentialAffineTransformSolverError,
    },
    MissingTransformOutput {
        argument: ComplexScalar,
        status: ExponentialAffineTransformIntegrationStatus,
    },
    CharacteristicFunctionAssemblyFailure {
        argument: ComplexScalar,
        source: super::characteristic::ReturnCharacteristicFunctionAssemblyError,
    },
    FourierPricingFailure {
        source: FourierInversionPricingError,
    },
    InvalidResponseDiagnostics {
        source: super::contracts::VanillaOptionPricingDiagnosticsError,
    },
    InvalidResponse {
        source: VanillaOptionPricingResponseError,
    },
    MissingRepresentativeTransformDiagnostics,
    InvalidRunResult {
        source: VanillaOptionPricingRunResultBuildError,
    },
}

impl Display for VanillaOptionPricingRunError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidExecutionInput { source } => {
                write!(formatter, "pricing execution input is invalid: {source}")
            }
            Self::InvalidDriverParameters { source } => {
                write!(
                    formatter,
                    "pricing request NIG driver parameters are invalid: {source}"
                )
            }
            Self::TransformInputBuildFailure { argument, source } => write!(
                formatter,
                "failed to build transform input for characteristic-function argument {argument:?}: {source}"
            ),
            Self::TransformSolveFailure { argument, source } => write!(
                formatter,
                "failed to solve affine transform for characteristic-function argument {argument:?}: {source}"
            ),
            Self::MissingTransformOutput { argument, status } => write!(
                formatter,
                "affine transform returned no output for characteristic-function argument {argument:?} with status {status:?}"
            ),
            Self::CharacteristicFunctionAssemblyFailure { argument, source } => write!(
                formatter,
                "failed to assemble return characteristic function for argument {argument:?}: {source}"
            ),
            Self::FourierPricingFailure { source } => {
                write!(
                    formatter,
                    "vanilla pricing Fourier inversion failed: {source}"
                )
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
            Self::MissingRepresentativeTransformDiagnostics => write!(
                formatter,
                "pricing run requested transform diagnostics but no representative transform solve was captured"
            ),
            Self::InvalidRunResult { source } => {
                write!(formatter, "pricing run result payload is invalid: {source}")
            }
        }
    }
}

impl Error for VanillaOptionPricingRunError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidExecutionInput { source } => Some(source),
            Self::InvalidDriverParameters { source } => Some(source),
            Self::TransformInputBuildFailure { source, .. } => Some(source),
            Self::TransformSolveFailure { source, .. } => Some(source),
            Self::CharacteristicFunctionAssemblyFailure { source, .. } => Some(source),
            Self::FourierPricingFailure { source } => Some(source),
            Self::InvalidResponseDiagnostics { source } => Some(source),
            Self::InvalidResponse { source } => Some(source),
            Self::InvalidRunResult { source } => Some(source),
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

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct VanillaOptionPricingExecutionInputPayload {
    request: VanillaOptionPricingRequest,
    interval_factors: VanillaOptionPricingIntervalFactors,
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
            payload.interval_factors,
            payload.optional_output_selection,
        )
        .map_err(serde::de::Error::custom)
    }
}
