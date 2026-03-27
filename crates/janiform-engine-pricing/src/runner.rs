use std::cell::RefCell;
use std::error::Error;
use std::f64::consts::PI;
use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};

use janiform_affine_oracle::affine_oracle::{
    ExponentialAffineTransformIntegrationResult, ExponentialAffineTransformIntegrationStatus,
    ExponentialAffineTransformNewsIntensityInput,
    ExponentialAffineTransformNewsIntensityInputError, ExponentialAffineTransformSolverError,
};
use janiform_foundations::complex_math::{ComplexScalar, complex_exponential, complex_multiply};
use janiform_model_kernels::hawkes::{
    HawkesEventSide, HawkesExcitationState, HawkesKernelLoadings,
};
use janiform_model_kernels::mean_reversion::MeanReversionBoundaryState;
use janiform_model_kernels::nig::{
    NigBuySellDriverParameters, NigSharedPlusShiftConfigurationError,
};
use janiform_model_kernels::return_decomposition::{ReturnDecomposition, ReturnDecompositionInput};

use super::characteristic::{
    PricingTransformInputBuildError, build_pricing_transform_input,
    compose_total_return_characteristic_function_from_oracle_output,
    solve_anchored_local_pricing_tilt, solve_exponential_affine_transform,
};
use super::contracts::{
    AnchoredPricingRecursionDiagnostics, AnchoredPricingRecursionIntervalDiagnostics,
    AnchoredVanillaOptionPricingBoundaryInput, VanillaOptionPayoffType,
    VanillaOptionPricingDiagnostics, VanillaOptionPricingOptionalOutputSelection,
    VanillaOptionPricingRequest, VanillaOptionPricingRequestError, VanillaOptionPricingResponse,
    VanillaOptionPricingResponseError, VanillaOptionPricingRunResult,
    VanillaOptionPricingRunResultBuildError, build_vanilla_option_pricing_run_result,
};
use super::fourier::{
    FourierInversionEngineSettings, FourierInversionPricingError,
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
    pub interval_factors: Vec<VanillaOptionPricingIntervalFactors>,
    pub optional_output_selection: VanillaOptionPricingOptionalOutputSelection,
}

impl VanillaOptionPricingExecutionInput {
    pub fn new(
        request: VanillaOptionPricingRequest,
        interval_factors: Vec<VanillaOptionPricingIntervalFactors>,
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
        interval_factors: Vec<VanillaOptionPricingIntervalFactors>,
        optional_output_selection: VanillaOptionPricingOptionalOutputSelection,
    ) -> Result<Self, VanillaOptionPricingExecutionInputError> {
        let execution_input = Self::new(request, interval_factors, optional_output_selection);
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

        let available_interval_count = self
            .request
            .boundary_input
            .mean_reversion_runtime_interval_anchors
            .end_anchor_values
            .len();
        let covered_interval_count = self
            .request
            .boundary_input
            .interval_index
            .saturating_add(self.interval_factors.len());
        if covered_interval_count > available_interval_count {
            return Err(
                VanillaOptionPricingExecutionInputError::IntervalFactorCountExceedsAvailableAnchors {
                    requested_interval_count: self.interval_factors.len(),
                    available_interval_count: available_interval_count
                        .saturating_sub(self.request.boundary_input.interval_index),
                },
            );
        }

        let maturity_interval_from_runtime_grid: f64 = self
            .request
            .boundary_input
            .mean_reversion_runtime_interval_anchors
            .interval_deltas[self.request.boundary_input.interval_index
            ..self.request.boundary_input.interval_index + self.interval_factors.len()]
            .iter()
            .sum();

        if maturity_interval_from_runtime_grid != self.request.market_inputs.maturity_interval {
            return Err(
                VanillaOptionPricingExecutionInputError::MaturityIntervalMismatch {
                    requested_maturity_interval: self.request.market_inputs.maturity_interval,
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
    IntervalFactorCountExceedsAvailableAnchors {
        requested_interval_count: usize,
        available_interval_count: usize,
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
            Self::IntervalFactorCountExceedsAvailableAnchors {
                requested_interval_count,
                available_interval_count,
            } => write!(
                formatter,
                "pricing execution interval_factors length {requested_interval_count} exceeds the available runtime anchor interval count {available_interval_count}"
            ),
            Self::MaturityIntervalMismatch {
                requested_maturity_interval,
                runtime_grid_maturity_interval,
            } => write!(
                formatter,
                "pricing execution maturity_interval {requested_maturity_interval} must equal the covered runtime anchor interval delta sum {runtime_grid_maturity_interval}"
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

#[derive(Debug, Clone, Copy, PartialEq)]
struct PricingCharacteristicFunctionEvaluation {
    value: ComplexScalar,
    transform_diagnostics: ExponentialAffineTransformIntegrationResult,
}

#[derive(Debug, Clone, PartialEq)]
struct AnchoredPricingEngineResult {
    price: f64,
    anchored_recursion_diagnostics: AnchoredPricingRecursionDiagnostics,
    representative_transform_diagnostics: Option<ExponentialAffineTransformIntegrationResult>,
    integration_quality_diagnostics: Option<super::fourier::FourierInversionDiagnostics>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct DeviationGrid {
    lower_bound: f64,
    upper_bound: f64,
    node_count: usize,
}

impl DeviationGrid {
    fn nodes(self) -> Vec<f64> {
        if self.node_count == 1 {
            return vec![self.lower_bound];
        }

        let step = (self.upper_bound - self.lower_bound) / (self.node_count - 1) as f64;

        (0..self.node_count)
            .map(|node_index| self.lower_bound + step * node_index as f64)
            .collect()
    }

    fn boundaries(self) -> Vec<f64> {
        let nodes = self.nodes();
        if nodes.len() <= 1 {
            return Vec::new();
        }

        (0..nodes.len() - 1)
            .map(|interval_index| 0.5 * (nodes[interval_index] + nodes[interval_index + 1]))
            .collect()
    }

    fn interpolate(
        self,
        values: &[f64],
        deviation: f64,
    ) -> Result<f64, VanillaOptionPricingRunError> {
        if values.len() != self.node_count {
            return Err(
                VanillaOptionPricingRunError::InvalidGridInterpolationShape {
                    expected_node_count: self.node_count,
                    found_value_count: values.len(),
                },
            );
        }

        if self.node_count == 1 {
            return Ok(values[0]);
        }

        if deviation <= self.lower_bound {
            return Ok(values[0]);
        }
        if deviation >= self.upper_bound {
            return Ok(values[self.node_count - 1]);
        }

        let step = (self.upper_bound - self.lower_bound) / (self.node_count - 1) as f64;

        let lower_index = ((deviation - self.lower_bound) / step).floor() as usize;
        let upper_index = lower_index + 1;

        let lower_node = self.lower_bound + step * lower_index as f64;

        let interpolation_weight = (deviation - lower_node) / step;

        Ok(values[lower_index].mul_add(
            1.0 - interpolation_weight,
            values[upper_index] * interpolation_weight,
        ))
    }
}

/// Canonical native runner for the public vanilla pricing workflow.
pub fn run_vanilla_option_pricing(
    execution_input: VanillaOptionPricingExecutionInput,
) -> Result<VanillaOptionPricingRunResult, VanillaOptionPricingRunError> {
    execution_input
        .validate()
        .map_err(|source| VanillaOptionPricingRunError::InvalidExecutionInput { source })?;

    let pricing_request = execution_input.request.clone();
    let output_selection = execution_input.optional_output_selection;

    let driver_parameters = pricing_request
        .model_parameters
        .nig_shared_plus_shift_configuration
        .derive_driver_parameters()
        .map_err(|source| VanillaOptionPricingRunError::InvalidDriverParameters { source })?;

    let anchored_result = if should_use_nested_single_interval_pricing(
        &pricing_request,
        execution_input.interval_factors.len(),
    )? {
        run_single_interval_nested_fourier_pricing(
            &pricing_request,
            execution_input.interval_factors[0],
            output_selection,
            driver_parameters,
        )?
    } else if execution_input.interval_factors.len() == 1 {
        run_single_interval_anchored_fourier_pricing(
            &pricing_request,
            execution_input.interval_factors[0],
            driver_parameters,
        )?
    } else {
        run_multi_interval_anchored_pricing(
            &pricing_request,
            &execution_input.interval_factors,
            driver_parameters,
        )?
    };

    let response = build_pricing_response(&pricing_request, anchored_result.price)?;

    let representative_transform_diagnostics = if output_selection.include_transform_diagnostics
        || output_selection.include_calibration_hooks
    {
        anchored_result.representative_transform_diagnostics
    } else {
        None
    };

    build_vanilla_option_pricing_run_result(
        response,
        output_selection,
        Some(pricing_request.market_inputs),
        representative_transform_diagnostics,
        anchored_result.integration_quality_diagnostics,
        Some(anchored_result.anchored_recursion_diagnostics),
    )
    .map_err(|source| VanillaOptionPricingRunError::InvalidRunResult { source })
}

fn should_use_nested_single_interval_pricing(
    pricing_request: &VanillaOptionPricingRequest,
    interval_factor_count: usize,
) -> Result<bool, VanillaOptionPricingRunError> {
    if interval_factor_count != 1 {
        return Ok(false);
    }

    if !pricing_request
        .boundary_input
        .mean_reversion_fitted_parameters
        .is_enabled()
    {
        return Ok(true);
    }

    let observed_return_adjustment = pricing_request
        .boundary_input
        .observed_return_adjustment()
        .map_err(
            |source| VanillaOptionPricingRunError::InvalidExecutionInput {
                source: VanillaOptionPricingExecutionInputError::InvalidRequest {
                    source: VanillaOptionPricingRequestError::InvalidBoundaryInput { source },
                },
            },
        )?;

    Ok(observed_return_adjustment == 0.0)
}

fn run_single_interval_nested_fourier_pricing(
    pricing_request: &VanillaOptionPricingRequest,
    interval_factors: VanillaOptionPricingIntervalFactors,
    output_selection: VanillaOptionPricingOptionalOutputSelection,
    driver_parameters: NigBuySellDriverParameters,
) -> Result<AnchoredPricingEngineResult, VanillaOptionPricingRunError> {
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
        pricing_request.market_inputs,
        pricing_request.payoff,
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

    let anchored_diagnostics = build_single_interval_recursion_diagnostics(
        pricing_request.boundary_input.clone(),
        Vec::new(),
        0.0,
        0.0,
        1,
        1,
        pricing_request.boundary_input.hawkes_boundary_state,
        expected_next_hawkes_state(
            pricing_request.model_parameters.hawkes_kernel_parameters,
            pricing_request
                .model_parameters
                .gamma_severity_mark_configuration
                .shape_k
                * pricing_request
                    .model_parameters
                    .gamma_severity_mark_configuration
                    .scale_theta,
            interval_factors,
            pricing_request
                .boundary_input
                .interval_delta()
                .map_err(
                    |source| VanillaOptionPricingRunError::InvalidExecutionInput {
                        source: VanillaOptionPricingExecutionInputError::InvalidRequest {
                            source: VanillaOptionPricingRequestError::InvalidBoundaryInput {
                                source,
                            },
                        },
                    },
                )?,
            pricing_request.boundary_input.hawkes_boundary_state,
        )?,
    )?;

    let representative_transform_diagnostics = representative_transform_diagnostics.into_inner();

    if (output_selection.include_transform_diagnostics
        || output_selection.include_calibration_hooks)
        && representative_transform_diagnostics.is_none()
    {
        return Err(VanillaOptionPricingRunError::MissingRepresentativeTransformDiagnostics);
    }

    Ok(AnchoredPricingEngineResult {
        price: pricing_result.price,
        anchored_recursion_diagnostics: anchored_diagnostics,
        representative_transform_diagnostics,
        integration_quality_diagnostics: Some(pricing_result.diagnostics),
    })
}

fn run_single_interval_anchored_fourier_pricing(
    pricing_request: &VanillaOptionPricingRequest,
    interval_factors: VanillaOptionPricingIntervalFactors,
    driver_parameters: NigBuySellDriverParameters,
) -> Result<AnchoredPricingEngineResult, VanillaOptionPricingRunError> {
    let local_boundary_input = pricing_request.boundary_input.clone();

    let approximate_return_decomposition =
        approximate_interval_return_decomposition(pricing_request, interval_factors);

    let local_tilt_diagnostics = solve_anchored_local_pricing_tilt(
        driver_parameters,
        approximate_return_decomposition,
        pricing_request.clone(),
        local_tilt_tolerance(),
    )
    .map_err(
        |source| VanillaOptionPricingRunError::AnchoredLocalTiltSolveFailure {
            interval_index: local_boundary_input.interval_index,
            deviation: local_boundary_input.observed_boundary_state.deviation,
            source,
        },
    )?;

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

    let observed_return_adjustment = pricing_request
        .boundary_input
        .observed_return_adjustment()
        .map_err(
            |source| VanillaOptionPricingRunError::InvalidExecutionInput {
                source: VanillaOptionPricingExecutionInputError::InvalidRequest {
                    source: VanillaOptionPricingRequestError::InvalidBoundaryInput { source },
                },
            },
        )?;

    let pricing_result = price_vanilla_option_via_fourier_inversion(
        pricing_request.market_inputs,
        pricing_request.payoff,
        FourierInversionEngineSettings::default(),
        |argument| match evaluate_observed_log_return_characteristic_function_under_local_measure(
            pricing_request,
            driver_parameters,
            interval_factors,
            news_intensity,
            local_tilt_diagnostics.theta,
            observed_return_adjustment,
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

    let anchored_diagnostics = build_single_interval_recursion_diagnostics(
        local_boundary_input.clone(),
        vec![local_tilt_diagnostics.clone()],
        pricing_result
            .diagnostics
            .numeric_quality_diagnostics
            .terminal_integrand_absolute_value,
        0.0,
        1,
        1,
        local_boundary_input.hawkes_boundary_state,
        expected_next_hawkes_state(
            pricing_request.model_parameters.hawkes_kernel_parameters,
            pricing_request
                .model_parameters
                .gamma_severity_mark_configuration
                .shape_k
                * pricing_request
                    .model_parameters
                    .gamma_severity_mark_configuration
                    .scale_theta,
            interval_factors,
            pricing_request
                .boundary_input
                .interval_delta()
                .map_err(
                    |source| VanillaOptionPricingRunError::InvalidExecutionInput {
                        source: VanillaOptionPricingExecutionInputError::InvalidRequest {
                            source: VanillaOptionPricingRequestError::InvalidBoundaryInput {
                                source,
                            },
                        },
                    },
                )?,
            local_boundary_input.hawkes_boundary_state,
        )?,
    )?;

    Ok(AnchoredPricingEngineResult {
        price: pricing_result.price,
        anchored_recursion_diagnostics: anchored_diagnostics,
        representative_transform_diagnostics: representative_transform_diagnostics.into_inner(),
        integration_quality_diagnostics: Some(pricing_result.diagnostics),
    })
}

fn run_multi_interval_anchored_pricing(
    pricing_request: &VanillaOptionPricingRequest,
    interval_factors: &[VanillaOptionPricingIntervalFactors],
    driver_parameters: NigBuySellDriverParameters,
) -> Result<AnchoredPricingEngineResult, VanillaOptionPricingRunError> {
    let coarse_grid = build_deviation_grid(pricing_request, interval_factors.len(), false)?;
    let refined_grid = build_deviation_grid(pricing_request, interval_factors.len(), true)?;

    let expected_hawkes_state_path =
        build_expected_hawkes_state_path(pricing_request, interval_factors)?;

    let mut representative_transform_diagnostics: Option<
        ExponentialAffineTransformIntegrationResult,
    > = None;

    let (coarse_price, coarse_diagnostics) = run_anchored_backward_induction(
        pricing_request,
        interval_factors,
        driver_parameters,
        &expected_hawkes_state_path,
        coarse_grid,
        true,
        &mut representative_transform_diagnostics,
    )?;

    let (refined_price, _) = run_anchored_backward_induction(
        pricing_request,
        interval_factors,
        driver_parameters,
        &expected_hawkes_state_path,
        refined_grid,
        false,
        &mut representative_transform_diagnostics,
    )?;

    let refinement_absolute_difference = (coarse_price - refined_price).abs();
    let refinement_relative_difference = if refined_price == 0.0 {
        refinement_absolute_difference
    } else {
        refinement_absolute_difference / refined_price.abs()
    };

    let anchored_recursion_diagnostics = AnchoredPricingRecursionDiagnostics::try_new(
        coarse_grid.lower_bound,
        coarse_grid.upper_bound,
        coarse_grid.node_count,
        refined_grid.node_count,
        refinement_absolute_difference,
        refinement_relative_difference,
        coarse_diagnostics,
    )
    .map_err(
        |source| VanillaOptionPricingRunError::InvalidAnchoredRecursionDiagnostics { source },
    )?;

    Ok(AnchoredPricingEngineResult {
        price: coarse_price,
        anchored_recursion_diagnostics,
        representative_transform_diagnostics,
        integration_quality_diagnostics: None,
    })
}

fn run_anchored_backward_induction(
    pricing_request: &VanillaOptionPricingRequest,
    interval_factors: &[VanillaOptionPricingIntervalFactors],
    driver_parameters: NigBuySellDriverParameters,
    expected_hawkes_state_path: &[HawkesExcitationState],
    deviation_grid: DeviationGrid,
    collect_diagnostics: bool,
    representative_transform_diagnostics: &mut Option<ExponentialAffineTransformIntegrationResult>,
) -> Result<(f64, Vec<AnchoredPricingRecursionIntervalDiagnostics>), VanillaOptionPricingRunError> {
    let nodes = deviation_grid.nodes();
    let boundaries = deviation_grid.boundaries();

    let interval_offset = pricing_request.boundary_input.interval_index;

    let runtime_anchors = &pricing_request
        .boundary_input
        .mean_reversion_runtime_interval_anchors;

    let terminal_anchor =
        runtime_anchors.end_anchor_values[interval_offset + interval_factors.len() - 1];

    let mut continuation_values: Vec<f64> = nodes
        .iter()
        .map(|deviation| terminal_option_payoff(pricing_request, terminal_anchor, *deviation))
        .collect();

    let mut interval_diagnostics: Vec<AnchoredPricingRecursionIntervalDiagnostics> = Vec::new();

    for reverse_interval_index in (0..interval_factors.len()).rev() {
        let global_interval_index = interval_offset + reverse_interval_index;

        let interval_factors = interval_factors[reverse_interval_index];

        let expected_hawkes_state_before = expected_hawkes_state_path[reverse_interval_index];
        let expected_hawkes_state_after = expected_hawkes_state_path[reverse_interval_index + 1];

        let current_anchor_value = if global_interval_index == 0 {
            runtime_anchors.start_anchor_value
        } else {
            runtime_anchors.end_anchor_values[global_interval_index - 1]
        };

        let next_anchor_value = runtime_anchors.end_anchor_values[global_interval_index];

        let interval_delta = runtime_anchors.interval_deltas[global_interval_index];

        let mut interval_values: Vec<f64> = Vec::with_capacity(nodes.len());

        let mut local_tilt_diagnostics: Vec<super::contracts::AnchoredLocalTiltSolveDiagnostics> =
            Vec::with_capacity(nodes.len());

        let mut maximum_tail_probability_mass: f64 = 0.0;
        let mut maximum_row_probability_mass_error: f64 = 0.0;
        let mut minimum_solved_theta = f64::INFINITY;
        let mut maximum_solved_theta = f64::NEG_INFINITY;

        for deviation in &nodes {
            let local_boundary_input = build_local_boundary_input(
                pricing_request,
                global_interval_index,
                expected_hawkes_state_before,
                current_anchor_value,
                *deviation,
            )?;

            let local_request = build_local_pricing_request(
                pricing_request,
                local_boundary_input.clone(),
                interval_delta,
            )?;
            let local_transition_theta =
                if use_nested_transition_measure_for_local_request(&local_request)? {
                    minimum_solved_theta = minimum_solved_theta.min(0.0);
                    maximum_solved_theta = maximum_solved_theta.max(0.0);
                    0.0
                } else {
                    let approximate_return_decomposition =
                        approximate_interval_return_decomposition(&local_request, interval_factors);

                    let local_tilt = solve_anchored_local_pricing_tilt(
                        driver_parameters,
                        approximate_return_decomposition,
                        local_request.clone(),
                        local_tilt_tolerance(),
                    )
                    .map_err(|source| {
                        VanillaOptionPricingRunError::AnchoredLocalTiltSolveFailure {
                            interval_index: global_interval_index,
                            deviation: *deviation,
                            source,
                        }
                    })?;

                    minimum_solved_theta = minimum_solved_theta.min(local_tilt.theta);
                    maximum_solved_theta = maximum_solved_theta.max(local_tilt.theta);

                    if collect_diagnostics {
                        local_tilt_diagnostics.push(local_tilt.clone());
                    }

                    local_tilt.theta
                };

            let row = transition_probability_row(
                &local_request,
                interval_factors,
                driver_parameters,
                local_transition_theta,
                &boundaries,
                representative_transform_diagnostics,
            )?;

            maximum_tail_probability_mass =
                maximum_tail_probability_mass.max(row[0].max(row[row.len() - 1]));

            let row_mass: f64 = row.iter().sum();

            maximum_row_probability_mass_error =
                maximum_row_probability_mass_error.max((row_mass - 1.0).abs());

            let discounted_interval_value =
                (-pricing_request.market_inputs.risk_free_rate * interval_delta).exp()
                    * row
                        .iter()
                        .zip(&continuation_values)
                        .map(|(probability_mass, continuation_value)| {
                            probability_mass * continuation_value
                        })
                        .sum::<f64>();

            interval_values.push(discounted_interval_value);
        }

        continuation_values = interval_values;

        if collect_diagnostics {
            interval_diagnostics.push(
                AnchoredPricingRecursionIntervalDiagnostics::try_new(
                    global_interval_index,
                    current_anchor_value,
                    next_anchor_value,
                    interval_delta,
                    expected_hawkes_state_before,
                    expected_hawkes_state_after,
                    maximum_tail_probability_mass,
                    maximum_row_probability_mass_error,
                    minimum_solved_theta,
                    maximum_solved_theta,
                    local_tilt_diagnostics,
                )
                .map_err(|source| {
                    VanillaOptionPricingRunError::InvalidAnchoredRecursionDiagnostics { source }
                })?,
            );
        }
    }

    interval_diagnostics.reverse();

    let price = deviation_grid.interpolate(
        &continuation_values,
        pricing_request
            .boundary_input
            .observed_boundary_state
            .deviation,
    )?;

    Ok((price, interval_diagnostics))
}

pub(crate) fn use_nested_transition_measure_for_local_request(
    pricing_request: &VanillaOptionPricingRequest,
) -> Result<bool, VanillaOptionPricingRunError> {
    let observed_return_adjustment = pricing_request
        .boundary_input
        .observed_return_adjustment()
        .map_err(
            |source| VanillaOptionPricingRunError::InvalidExecutionInput {
                source: VanillaOptionPricingExecutionInputError::InvalidRequest {
                    source: VanillaOptionPricingRequestError::InvalidBoundaryInput { source },
                },
            },
        )?;

    Ok(observed_return_adjustment == 0.0)
}

fn build_deviation_grid(
    pricing_request: &VanillaOptionPricingRequest,
    interval_count: usize,
    refined: bool,
) -> Result<DeviationGrid, VanillaOptionPricingRunError> {
    let interval_offset = pricing_request.boundary_input.interval_index;

    let runtime_anchors = &pricing_request
        .boundary_input
        .mean_reversion_runtime_interval_anchors;

    let terminal_anchor = runtime_anchors.end_anchor_values[interval_offset + interval_count - 1];

    let current_observed_log_price = pricing_request
        .boundary_input
        .observed_boundary_state
        .observed_log_price;

    let terminal_strike_deviation = current_observed_log_price
        + (pricing_request.market_inputs.strike / pricing_request.market_inputs.spot).ln()
        - terminal_anchor;

    let current_deviation = pricing_request
        .boundary_input
        .observed_boundary_state
        .deviation;

    let mut anchor_min = pricing_request
        .boundary_input
        .current_anchor_value()
        .map_err(
            |source| VanillaOptionPricingRunError::InvalidExecutionInput {
                source: VanillaOptionPricingExecutionInputError::InvalidRequest {
                    source: VanillaOptionPricingRequestError::InvalidBoundaryInput { source },
                },
            },
        )?;

    let mut anchor_max = anchor_min;

    for anchor_value in
        &runtime_anchors.end_anchor_values[interval_offset..interval_offset + interval_count]
    {
        anchor_min = anchor_min.min(*anchor_value);
        anchor_max = anchor_max.max(*anchor_value);
    }

    let half_width = (terminal_strike_deviation - current_deviation).abs()
        + (anchor_max - anchor_min)
        + pricing_request.market_inputs.maturity_interval;

    let center = 0.5 * (current_deviation + terminal_strike_deviation);

    let node_count = if refined {
        4 * interval_count + 1
    } else {
        2 * interval_count + 1
    };

    Ok(DeviationGrid {
        lower_bound: center - half_width,
        upper_bound: center + half_width,
        node_count,
    })
}

fn build_expected_hawkes_state_path(
    pricing_request: &VanillaOptionPricingRequest,
    interval_factors: &[VanillaOptionPricingIntervalFactors],
) -> Result<Vec<HawkesExcitationState>, VanillaOptionPricingRunError> {
    let mut state_path = Vec::with_capacity(interval_factors.len() + 1);

    state_path.push(pricing_request.boundary_input.hawkes_boundary_state);

    let expected_news_severity_mean = pricing_request
        .model_parameters
        .gamma_severity_mark_configuration
        .shape_k
        * pricing_request
            .model_parameters
            .gamma_severity_mark_configuration
            .scale_theta;

    for (interval_index, interval_factors) in interval_factors.iter().enumerate() {
        let global_interval_index = pricing_request.boundary_input.interval_index + interval_index;

        let interval_delta = pricing_request
            .boundary_input
            .mean_reversion_runtime_interval_anchors
            .interval_deltas[global_interval_index];

        let next_state = expected_next_hawkes_state(
            pricing_request.model_parameters.hawkes_kernel_parameters,
            expected_news_severity_mean,
            *interval_factors,
            interval_delta,
            *state_path.last().expect("state path always has one value"),
        )?;

        state_path.push(next_state);
    }

    Ok(state_path)
}

fn expected_next_hawkes_state(
    kernel_parameters: janiform_model_kernels::hawkes::HawkesKernelParameters,
    expected_news_severity_mean: f64,
    interval_factors: VanillaOptionPricingIntervalFactors,
    interval_delta: f64,
    state_before_update: HawkesExcitationState,
) -> Result<HawkesExcitationState, VanillaOptionPricingRunError> {
    let intensity_projection =
        state_before_update.project_intensities(kernel_parameters.lambda_bar, interval_factors.s_h);

    let drifted_state =
        state_before_update.apply_shared_decay_drift(kernel_parameters.beta, interval_delta);

    let post_buy_state = apply_expected_endogenous_jump(
        drifted_state,
        kernel_parameters.alpha,
        HawkesEventSide::Buy,
        intensity_projection.seasonality_scaled.lambda_plus * interval_delta,
    );
    let post_sell_state = apply_expected_endogenous_jump(
        post_buy_state,
        kernel_parameters.alpha,
        HawkesEventSide::Sell,
        intensity_projection.seasonality_scaled.lambda_minus * interval_delta,
    );

    let expected_news_severity =
        interval_factors.total_news_intensity() * interval_delta * expected_news_severity_mean;

    post_sell_state
        .apply_exogenous_news_loading_jump(kernel_parameters.rho, expected_news_severity)
        .map_err(
            |source| VanillaOptionPricingRunError::ExpectedHawkesStateUpdateFailure {
                message: source.to_string(),
            },
        )
}

fn apply_expected_endogenous_jump(
    state: HawkesExcitationState,
    alpha: HawkesKernelLoadings,
    side: HawkesEventSide,
    expected_count: f64,
) -> HawkesExcitationState {
    if expected_count == 0.0 {
        return state;
    }

    let scaled_alpha = HawkesKernelLoadings {
        plus_plus: alpha.plus_plus * expected_count,
        plus_minus: alpha.plus_minus * expected_count,
        minus_plus: alpha.minus_plus * expected_count,
        minus_minus: alpha.minus_minus * expected_count,
    };

    state.apply_endogenous_event_jump(side, scaled_alpha)
}

fn build_local_boundary_input(
    pricing_request: &VanillaOptionPricingRequest,
    interval_index: usize,
    hawkes_boundary_state: HawkesExcitationState,
    anchor_value: f64,
    deviation: f64,
) -> Result<AnchoredVanillaOptionPricingBoundaryInput, VanillaOptionPricingRunError> {
    let observed_boundary_state =
        MeanReversionBoundaryState::try_new(anchor_value + deviation, anchor_value, deviation)
            .map_err(|source| {
                VanillaOptionPricingRunError::InvalidObservedBoundaryStateDuringRecursion {
                    interval_index,
                    deviation,
                    message: source.to_string(),
                }
            })?;

    AnchoredVanillaOptionPricingBoundaryInput::try_new(
        interval_index,
        hawkes_boundary_state,
        pricing_request
            .boundary_input
            .mean_reversion_fitted_parameters
            .clone(),
        pricing_request
            .boundary_input
            .mean_reversion_runtime_interval_anchors
            .clone(),
        observed_boundary_state,
    )
    .map_err(|source| {
        VanillaOptionPricingRunError::InvalidObservedBoundaryStateDuringRecursion {
            interval_index,
            deviation,
            message: source.to_string(),
        }
    })
}

fn build_local_pricing_request(
    pricing_request: &VanillaOptionPricingRequest,
    boundary_input: AnchoredVanillaOptionPricingBoundaryInput,
    interval_delta: f64,
) -> Result<VanillaOptionPricingRequest, VanillaOptionPricingRunError> {
    VanillaOptionPricingRequest::try_new_with_interval_delta(
        super::contracts::VanillaOptionPricingMarketInputs::try_new(
            pricing_request.market_inputs.spot,
            pricing_request.market_inputs.strike,
            interval_delta,
            pricing_request.market_inputs.risk_free_rate,
            pricing_request.market_inputs.dividend_yield,
        )
        .map_err(
            |source| VanillaOptionPricingRunError::InvalidExecutionInput {
                source: VanillaOptionPricingExecutionInputError::InvalidRequest {
                    source: VanillaOptionPricingRequestError::InvalidMarketInputs { source },
                },
            },
        )?,
        pricing_request.payoff,
        boundary_input,
        pricing_request.model_parameters,
        interval_delta,
    )
    .map_err(
        |source| VanillaOptionPricingRunError::InvalidExecutionInput {
            source: VanillaOptionPricingExecutionInputError::InvalidRequest { source },
        },
    )
}

fn approximate_interval_return_decomposition(
    pricing_request: &VanillaOptionPricingRequest,
    interval_factors: VanillaOptionPricingIntervalFactors,
) -> ReturnDecomposition {
    let intensity_projection = pricing_request
        .boundary_input
        .hawkes_boundary_state
        .project_intensities(
            pricing_request
                .model_parameters
                .hawkes_kernel_parameters
                .lambda_bar,
            interval_factors.s_h,
        );

    ReturnDecomposition::try_new(ReturnDecompositionInput {
        a_plus: intensity_projection.seasonality_scaled.lambda_plus
            * pricing_request.interval_delta,
        a_minus: intensity_projection.seasonality_scaled.lambda_minus
            * pricing_request.interval_delta,
        open_jump: 0.0,
        closed_gap: 0.0,
    })
    .expect("non-negative projected intensity decomposition must validate")
}

fn transition_probability_row(
    pricing_request: &VanillaOptionPricingRequest,
    interval_factors: VanillaOptionPricingIntervalFactors,
    driver_parameters: NigBuySellDriverParameters,
    theta: f64,
    target_boundaries: &[f64],
    representative_transform_diagnostics: &mut Option<ExponentialAffineTransformIntegrationResult>,
) -> Result<Vec<f64>, VanillaOptionPricingRunError> {
    let current_anchor_value = pricing_request
        .boundary_input
        .current_anchor_value()
        .map_err(
            |source| VanillaOptionPricingRunError::InvalidExecutionInput {
                source: VanillaOptionPricingExecutionInputError::InvalidRequest {
                    source: VanillaOptionPricingRequestError::InvalidBoundaryInput { source },
                },
            },
        )?;

    let next_anchor_value =
        pricing_request
            .boundary_input
            .next_anchor_value()
            .map_err(
                |source| VanillaOptionPricingRunError::InvalidExecutionInput {
                    source: VanillaOptionPricingExecutionInputError::InvalidRequest {
                        source: VanillaOptionPricingRequestError::InvalidBoundaryInput { source },
                    },
                },
            )?;

    let deviation_decay_factor = pricing_request
        .boundary_input
        .deviation_decay_factor()
        .map_err(
            |source| VanillaOptionPricingRunError::InvalidExecutionInput {
                source: VanillaOptionPricingExecutionInputError::InvalidRequest {
                    source: VanillaOptionPricingRequestError::InvalidBoundaryInput { source },
                },
            },
        )?;

    let news_intensity = interval_factors.news_intensity_input().map_err(|source| {
        VanillaOptionPricingRunError::InvalidExecutionInput {
            source: VanillaOptionPricingExecutionInputError::InvalidIntervalFactors { source },
        }
    })?;

    let shift = deviation_decay_factor
        * pricing_request
            .boundary_input
            .observed_boundary_state
            .deviation
        + current_anchor_value
        - next_anchor_value;

    let mut projected_cdf_values: Vec<f64> = Vec::with_capacity(target_boundaries.len());

    for boundary in target_boundaries {
        projected_cdf_values.push(evaluate_shifted_core_cdf_under_local_measure(
            pricing_request,
            driver_parameters,
            interval_factors,
            news_intensity,
            theta,
            *boundary - shift,
            representative_transform_diagnostics,
        )?);
    }

    let mut row = Vec::with_capacity(target_boundaries.len() + 1);

    if projected_cdf_values.is_empty() {
        row.push(1.0);
        return Ok(row);
    }

    row.push(projected_cdf_values[0]);

    for interval_index in 1..projected_cdf_values.len() {
        row.push(projected_cdf_values[interval_index] - projected_cdf_values[interval_index - 1]);
    }

    row.push(1.0 - projected_cdf_values[projected_cdf_values.len() - 1]);

    Ok(row)
}

fn evaluate_shifted_core_cdf_under_local_measure(
    pricing_request: &VanillaOptionPricingRequest,
    driver_parameters: NigBuySellDriverParameters,
    interval_factors: VanillaOptionPricingIntervalFactors,
    news_intensity: ExponentialAffineTransformNewsIntensityInput,
    theta: f64,
    evaluation_point: f64,
    representative_transform_diagnostics: &mut Option<ExponentialAffineTransformIntegrationResult>,
) -> Result<f64, VanillaOptionPricingRunError> {
    let settings = FourierInversionEngineSettings::default();

    let integration_upper_bound = settings.numeric_settings.integration_upper_bound;
    let integration_step_count = settings.numeric_settings.integration_step_count;

    let integration_step = integration_upper_bound / integration_step_count as f64;

    let mut integral = 0.0;

    for integration_index in 0..integration_step_count {
        let frequency = (integration_index as f64 + 0.5) * integration_step;

        let characteristic_argument = ComplexScalar::new(frequency, 0.0);

        let evaluation = evaluate_core_log_return_characteristic_function_under_local_measure(
            pricing_request,
            driver_parameters,
            interval_factors,
            news_intensity,
            theta,
            characteristic_argument,
        )?;

        if representative_transform_diagnostics.is_none() {
            *representative_transform_diagnostics = Some(evaluation.transform_diagnostics);
        }

        let exponential_factor =
            complex_exponential(ComplexScalar::new(0.0, -frequency * evaluation_point));

        let integrand = complex_multiply(exponential_factor, evaluation.value);

        integral += integrand.imaginary / frequency;
    }

    Ok((0.5 - integration_step * integral / PI).clamp(0.0, 1.0))
}

fn evaluate_core_log_return_characteristic_function_under_local_measure(
    pricing_request: &VanillaOptionPricingRequest,
    driver_parameters: NigBuySellDriverParameters,
    interval_factors: VanillaOptionPricingIntervalFactors,
    news_intensity: ExponentialAffineTransformNewsIntensityInput,
    theta: f64,
    argument: ComplexScalar,
) -> Result<PricingCharacteristicFunctionEvaluation, VanillaOptionPricingRunError> {
    let shifted_argument = ComplexScalar::new(argument.real, argument.imaginary - theta);

    let numerator = evaluate_pricing_log_return_characteristic_function(
        pricing_request,
        driver_parameters,
        interval_factors,
        news_intensity,
        shifted_argument,
    )?;
    let denominator = evaluate_pricing_log_return_characteristic_function(
        pricing_request,
        driver_parameters,
        interval_factors,
        news_intensity,
        ComplexScalar::new(0.0, -theta),
    )?;

    let risk_neutral_value = numerator.value.divide(denominator.value).ok_or(
        VanillaOptionPricingRunError::InvalidRiskNeutralCharacteristicFunction {
            numerator: numerator.value,
            denominator: denominator.value,
        },
    )?;

    Ok(PricingCharacteristicFunctionEvaluation {
        value: risk_neutral_value,
        transform_diagnostics: numerator.transform_diagnostics,
    })
}

fn evaluate_observed_log_return_characteristic_function_under_local_measure(
    pricing_request: &VanillaOptionPricingRequest,
    driver_parameters: NigBuySellDriverParameters,
    interval_factors: VanillaOptionPricingIntervalFactors,
    news_intensity: ExponentialAffineTransformNewsIntensityInput,
    theta: f64,
    observed_return_adjustment: f64,
    argument: ComplexScalar,
) -> Result<PricingCharacteristicFunctionEvaluation, VanillaOptionPricingRunError> {
    let core_evaluation = evaluate_core_log_return_characteristic_function_under_local_measure(
        pricing_request,
        driver_parameters,
        interval_factors,
        news_intensity,
        theta,
        argument,
    )?;

    let translation_factor = complex_exponential(ComplexScalar::new(
        -observed_return_adjustment * argument.imaginary,
        observed_return_adjustment * argument.real,
    ));

    Ok(PricingCharacteristicFunctionEvaluation {
        value: complex_multiply(translation_factor, core_evaluation.value),
        transform_diagnostics: core_evaluation.transform_diagnostics,
    })
}

fn terminal_option_payoff(
    pricing_request: &VanillaOptionPricingRequest,
    terminal_anchor_value: f64,
    terminal_deviation: f64,
) -> f64 {
    let current_observed_log_price = pricing_request
        .boundary_input
        .observed_boundary_state
        .observed_log_price;

    let terminal_spot = pricing_request.market_inputs.spot
        * ((terminal_anchor_value + terminal_deviation) - current_observed_log_price).exp();

    match pricing_request.payoff.payoff_type {
        VanillaOptionPayoffType::Call => {
            (terminal_spot - pricing_request.market_inputs.strike).max(0.0)
        }
        VanillaOptionPayoffType::Put => {
            (pricing_request.market_inputs.strike - terminal_spot).max(0.0)
        }
    }
}

fn build_single_interval_recursion_diagnostics(
    boundary_input: AnchoredVanillaOptionPricingBoundaryInput,
    local_tilt_solve_diagnostics: Vec<super::contracts::AnchoredLocalTiltSolveDiagnostics>,
    maximum_tail_probability_mass: f64,
    maximum_row_probability_mass_error: f64,
    deviation_grid_node_count: usize,
    refined_deviation_grid_node_count: usize,
    expected_hawkes_boundary_state_before: HawkesExcitationState,
    expected_hawkes_boundary_state_after: HawkesExcitationState,
) -> Result<AnchoredPricingRecursionDiagnostics, VanillaOptionPricingRunError> {
    let current_anchor_value = boundary_input.current_anchor_value().map_err(|source| {
        VanillaOptionPricingRunError::InvalidExecutionInput {
            source: VanillaOptionPricingExecutionInputError::InvalidRequest {
                source: VanillaOptionPricingRequestError::InvalidBoundaryInput { source },
            },
        }
    })?;

    let next_anchor_value = boundary_input.next_anchor_value().map_err(|source| {
        VanillaOptionPricingRunError::InvalidExecutionInput {
            source: VanillaOptionPricingExecutionInputError::InvalidRequest {
                source: VanillaOptionPricingRequestError::InvalidBoundaryInput { source },
            },
        }
    })?;

    let interval_delta = boundary_input.interval_delta().map_err(|source| {
        VanillaOptionPricingRunError::InvalidExecutionInput {
            source: VanillaOptionPricingExecutionInputError::InvalidRequest {
                source: VanillaOptionPricingRequestError::InvalidBoundaryInput { source },
            },
        }
    })?;

    let minimum_solved_theta = local_tilt_solve_diagnostics
        .iter()
        .map(|diagnostics| diagnostics.theta)
        .min_by(|left, right| left.total_cmp(right))
        .unwrap_or(0.0);
    let maximum_solved_theta = local_tilt_solve_diagnostics
        .iter()
        .map(|diagnostics| diagnostics.theta)
        .max_by(|left, right| left.total_cmp(right))
        .unwrap_or(0.0);

    AnchoredPricingRecursionDiagnostics::try_new(
        boundary_input.observed_boundary_state.deviation,
        boundary_input.observed_boundary_state.deviation + interval_delta,
        deviation_grid_node_count,
        refined_deviation_grid_node_count,
        0.0,
        0.0,
        vec![
            AnchoredPricingRecursionIntervalDiagnostics::try_new(
                boundary_input.interval_index,
                current_anchor_value,
                next_anchor_value,
                interval_delta,
                expected_hawkes_boundary_state_before,
                expected_hawkes_boundary_state_after,
                maximum_tail_probability_mass,
                maximum_row_probability_mass_error,
                minimum_solved_theta,
                maximum_solved_theta,
                local_tilt_solve_diagnostics,
            )
            .map_err(|source| {
                VanillaOptionPricingRunError::InvalidAnchoredRecursionDiagnostics { source }
            })?,
        ],
    )
    .map_err(|source| VanillaOptionPricingRunError::InvalidAnchoredRecursionDiagnostics { source })
}

fn local_tilt_tolerance() -> f64 {
    f64::EPSILON.sqrt()
}

fn build_pricing_response(
    pricing_request: &VanillaOptionPricingRequest,
    price: f64,
) -> Result<VanillaOptionPricingResponse, VanillaOptionPricingRunError> {
    let market_inputs = pricing_request.market_inputs;

    let forward_price = market_inputs.spot
        * ((market_inputs.risk_free_rate - market_inputs.dividend_yield)
            * market_inputs.maturity_interval)
            .exp();

    let discount_factor = (-market_inputs.risk_free_rate * market_inputs.maturity_interval).exp();

    let boundary_state = pricing_request.boundary_input.hawkes_boundary_state;
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

    VanillaOptionPricingResponse::try_new(price, diagnostics)
        .map_err(|source| VanillaOptionPricingRunError::InvalidResponse { source })
}

fn evaluate_pricing_log_return_characteristic_function(
    pricing_request: &VanillaOptionPricingRequest,
    driver_parameters: NigBuySellDriverParameters,
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
    InvalidRiskNeutralCharacteristicFunction {
        numerator: ComplexScalar,
        denominator: ComplexScalar,
    },
    ExpectedHawkesStateUpdateFailure {
        message: String,
    },
    AnchoredLocalTiltSolveFailure {
        interval_index: usize,
        deviation: f64,
        source: super::characteristic::AnchoredLocalTiltSolveError,
    },
    InvalidObservedBoundaryStateDuringRecursion {
        interval_index: usize,
        deviation: f64,
        message: String,
    },
    InvalidAnchoredRecursionDiagnostics {
        source: super::contracts::AnchoredPricingRecursionDiagnosticsError,
    },
    InvalidGridInterpolationShape {
        expected_node_count: usize,
        found_value_count: usize,
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
            Self::InvalidRiskNeutralCharacteristicFunction {
                numerator,
                denominator,
            } => write!(
                formatter,
                "state-dependent risk-neutral characteristic function became invalid with numerator {numerator:?} and denominator {denominator:?}"
            ),
            Self::ExpectedHawkesStateUpdateFailure { message } => {
                write!(formatter, "expected Hawkes state update failed: {message}")
            }
            Self::AnchoredLocalTiltSolveFailure {
                interval_index,
                deviation,
                source,
            } => write!(
                formatter,
                "anchored local tilt solve failed for interval_index={interval_index} and deviation={deviation}: {source}"
            ),
            Self::InvalidObservedBoundaryStateDuringRecursion {
                interval_index,
                deviation,
                message,
            } => write!(
                formatter,
                "anchored recursion could not build an observed boundary state for interval_index={interval_index} and deviation={deviation}: {message}"
            ),
            Self::InvalidAnchoredRecursionDiagnostics { source } => {
                write!(
                    formatter,
                    "anchored recursion diagnostics are invalid: {source}"
                )
            }
            Self::InvalidGridInterpolationShape {
                expected_node_count,
                found_value_count,
            } => write!(
                formatter,
                "anchored deviation-grid interpolation expected {expected_node_count} node values and received {found_value_count}"
            ),
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
            Self::AnchoredLocalTiltSolveFailure { source, .. } => Some(source),
            Self::InvalidAnchoredRecursionDiagnostics { source } => Some(source),
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

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct VanillaOptionPricingExecutionInputPayload {
    request: VanillaOptionPricingRequest,
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
            payload.interval_factors,
            payload.optional_output_selection,
        )
        .map_err(serde::de::Error::custom)
    }
}
