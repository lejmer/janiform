use std::collections::BTreeMap;
use std::error::Error;
use std::fmt::{Display, Formatter};

use rayon::iter::{IntoParallelIterator, ParallelIterator};
use rayon::join;
use serde::{Deserialize, Serialize};

use janiform_affine_oracle::affine_oracle::{
    ExponentialAffineTransformEquationSpecification, ExponentialAffineTransformInput,
    ExponentialAffineTransformIntegrationResult, ExponentialAffineTransformIntegrationStatus,
    ExponentialAffineTransformNewsIntensityInput,
    ExponentialAffineTransformNewsIntensityInputError, ExponentialAffineTransformSolverError,
    ExponentialAffineTransformSolverSettings,
    solve_exponential_affine_transform_input_sensitivities_with_settings,
};
use janiform_affine_oracle::endogenous_anchor::{
    EndogenousAnchorIntervalFactors, derive_expected_interval_projection_from_first_moment_dynamics,
};
use janiform_affine_oracle::sufficient_statistics::{
    SufficientStatisticsExpectedIntervalStatistics, SufficientStatisticsTransformSliceContext,
    SufficientStatisticsTransformSliceSettings, SufficientStatisticsTransformSlices,
    derive_expected_interval_statistics_from_transform_slices,
};
use janiform_foundations::complex_math::ComplexScalar;
use janiform_foundations::complex_math::complex_multiply;
use janiform_model_kernels::hawkes::HawkesExcitationState;
use janiform_model_kernels::mean_reversion::{
    MeanReversionBoundaryState, MeanReversionRuntimeIntervalAnchors,
    evolve_mean_reversion_boundary_state_with_endogenous_pressure,
    evolve_mean_reversion_deviation_with_decay_on_interval_grid,
};
use janiform_model_kernels::nig::{
    NigBuySellDriverParameters, NigSharedPlusShiftConfigurationError,
};

use super::characteristic::{
    LocalTiltResidualEvaluation, LocalTiltSearchDomain, PricingTransformInputBuildError,
    build_local_tilt_bracket_with_search_domain_and_residual_evaluator,
    build_pricing_equation_specification,
    compose_total_return_characteristic_function_from_oracle_output,
    resolve_local_tilt_search_domain, should_retry_local_tilt_side_evaluation,
    solve_exponential_affine_transform_with_internal_selection,
};
use super::constants::{
    FOURIER_INVERSION_DIAGNOSTICS_SCHEMA_VERSION, FOURIER_INVERSION_NUMERIC_QUALITY_GATE_POLICY,
    SPECTRAL_PRICING_COLLOCATION_POLICY, SPECTRAL_PRICING_DEVIATION_DOMAIN_POLICY,
    SPECTRAL_PRICING_INTEGRATION_COMPATIBILITY_POLICY,
};
use super::contracts::{
    AnchoredLocalTiltBoundaryStateSummary, AnchoredLocalTiltSolveDiagnostics,
    AnchoredVanillaOptionPricingBoundaryInput, SpectralPricingDiagnostics,
    VanillaOptionPricingDiagnostics, VanillaOptionPricingFailureDiagnostic,
    VanillaOptionPricingFailureStage, VanillaOptionPricingModelParameters,
    VanillaOptionPricingOptionalOutputSelection, VanillaOptionPricingRequest,
    VanillaOptionPricingRequestError, VanillaOptionPricingResponse,
    VanillaOptionPricingResponseError, VanillaOptionPricingRunResult,
    VanillaOptionPricingRunResultBuildError, VanillaOptionPricingRunResultBuildInput,
    VanillaOptionPricingStatus, build_vanilla_option_pricing_run_result,
};
use super::expected_hawkes_state::evolve_expected_hawkes_state_from_integrated_statistics;
use super::fourier::{
    FourierInversionDiagnostics, FourierInversionNumericQualityDiagnostics,
    FourierInversionNumericQualityGate, FourierInversionPricingError,
    FourierInversionQuadratureRule,
};
use super::interval_schedule::PricingIntervalSchedule;
use super::progress::{PricingNodeProgress, PricingNodeProgressPhase, emit_node_progress};
use super::thread_pool::{install_pricing_parallelism, resolved_pricing_cpu_count};
use super::time_scaling::intraday_year_fraction;

const OBSERVED_LOG_MOMENT_ARGUMENT_IMAGINARY: f64 = -1.0;

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

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PricingCharacteristicFunctionEvaluation {
    pub(crate) value: ComplexScalar,
    pub(crate) transform_diagnostics: ExponentialAffineTransformIntegrationResult,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RiskNeutralAnchorMaterialization {
    pub(crate) runtime_interval_anchors: MeanReversionRuntimeIntervalAnchors,
    pub(crate) expected_hawkes_state_path: Vec<HawkesExcitationState>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LocalTiltedIntervalExpectations {
    pub(crate) expected_statistics: SufficientStatisticsExpectedIntervalStatistics,
    pub(crate) core_expected_increment: f64,
    pub(crate) expected_hawkes_state_after: Option<HawkesExcitationState>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CorePureImaginaryLogMomentEvaluation {
    pub(crate) observed_log_moment_exponent: f64,
    pub(crate) transform_diagnostics: ExponentialAffineTransformIntegrationResult,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CorePureImaginaryLogMomentSharedPlusShiftSensitivity {
    pub(crate) observed_log_moment_exponent: f64,
    pub(crate) alpha_derivative: f64,
    pub(crate) beta_0_derivative: f64,
    pub(crate) delta_derivative: f64,
    pub(crate) mu_derivative: f64,
    pub(crate) delta_beta_derivative: f64,
    pub(crate) theta_derivative: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CoreLocalTiltImplicitThetaSharedPlusShiftSensitivity {
    pub(crate) alpha_derivative: f64,
    pub(crate) beta_0_derivative: f64,
    pub(crate) delta_derivative: f64,
    pub(crate) delta_beta_derivative: f64,
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PreparedTransitionRowKernel {
    pub(crate) transition_row: LocalTransitionRowState,
    pub(crate) theta: f64,
    pub(crate) tilt_diagnostics: Option<AnchoredLocalTiltSolveDiagnostics>,
    pub(crate) denominator: ComplexScalar,
    pub(crate) local_expectations: LocalTiltedIntervalExpectations,
    pub(crate) representative_transform_diagnostics:
        Option<ExponentialAffineTransformIntegrationResult>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LocalTransitionRequestContext {
    pub(crate) interval_index: usize,
    pub(crate) interval_delta: f64,
    current_anchor_value: f64,
    current_pressure_state: f64,
    pub(crate) hawkes_boundary_state: HawkesExcitationState,
    pub(crate) mean_reversion_fitted_parameters:
        janiform_model_kernels::mean_reversion::MeanReversionFittedParameters,
    risk_free_rate: f64,
    dividend_yield: f64,
    pub(crate) model_parameters: VanillaOptionPricingModelParameters,
    pub(crate) interval_factors: VanillaOptionPricingIntervalFactors,
    pub(crate) news_intensity: ExponentialAffineTransformNewsIntensityInput,
    pub(crate) equation_specification: ExponentialAffineTransformEquationSpecification,
    pub(crate) deviation_decay_factor: f64,
}

impl LocalTransitionRequestContext {
    pub(crate) fn from_pricing_request(
        pricing_request: &VanillaOptionPricingRequest,
        interval_factors: VanillaOptionPricingIntervalFactors,
    ) -> Result<Self, VanillaOptionPricingRunError> {
        Self::from_interval_state(
            pricing_request,
            interval_factors,
            pricing_request.boundary_input.interval_index,
            pricing_request.boundary_input.hawkes_boundary_state,
            pricing_request
                .boundary_input
                .observed_boundary_state
                .anchor_value,
            pricing_request
                .boundary_input
                .observed_boundary_state
                .pressure_state,
            pricing_request.interval_delta,
        )
    }

    pub(crate) fn from_interval_state(
        pricing_request: &VanillaOptionPricingRequest,
        interval_factors: VanillaOptionPricingIntervalFactors,
        interval_index: usize,
        hawkes_boundary_state: HawkesExcitationState,
        current_anchor_value: f64,
        current_pressure_state: f64,
        interval_delta: f64,
    ) -> Result<Self, VanillaOptionPricingRunError> {
        let news_intensity = interval_factors.news_intensity_input().map_err(|source| {
            VanillaOptionPricingRunError::InvalidExecutionInput {
                source: Box::new(
                    VanillaOptionPricingExecutionInputError::InvalidIntervalFactors { source },
                ),
            }
        })?;

        let mean_reversion_fitted_parameters = pricing_request
            .boundary_input
            .mean_reversion_fitted_parameters
            .clone();

        let deviation_decay_factor =
            (-mean_reversion_fitted_parameters.kappa * interval_delta).exp();

        Ok(Self {
            interval_index,
            interval_delta,
            current_anchor_value,
            current_pressure_state,
            hawkes_boundary_state,
            mean_reversion_fitted_parameters,
            risk_free_rate: pricing_request.market_inputs.risk_free_rate,
            dividend_yield: pricing_request.market_inputs.dividend_yield,
            model_parameters: pricing_request.model_parameters.clone(),
            interval_factors,
            news_intensity,
            equation_specification: build_pricing_equation_specification(pricing_request),
            deviation_decay_factor,
        })
    }

    fn build_transform_input(
        &self,
        u_plus: ComplexScalar,
        u_minus: ComplexScalar,
        z_plus: ComplexScalar,
        z_minus: ComplexScalar,
    ) -> ExponentialAffineTransformInput {
        ExponentialAffineTransformInput::new(
            self.equation_specification,
            u_plus,
            u_minus,
            z_plus,
            z_minus,
            self.interval_factors.s_h,
            self.news_intensity,
            self.interval_delta,
            self.hawkes_boundary_state,
        )
    }

    pub(crate) fn build_row_state(
        &self,
        deviation: f64,
    ) -> Result<LocalTransitionRowState, VanillaOptionPricingRunError> {
        let boundary_state = MeanReversionBoundaryState::try_new(
            self.current_anchor_value + deviation,
            self.current_anchor_value,
            self.current_pressure_state,
            deviation,
        )
        .map_err(|source| {
            VanillaOptionPricingRunError::InvalidObservedBoundaryStateDuringRecursion {
                interval_index: self.interval_index,
                deviation,
                message: source.to_string(),
            }
        })?;
        let local_martingale_deviation_adjustment = (self.deviation_decay_factor - 1.0) * deviation;

        Ok(LocalTransitionRowState {
            boundary_state,
            local_martingale_deviation_adjustment,
            target_log_moment_exponent: (self.risk_free_rate - self.dividend_yield)
                * intraday_year_fraction(self.interval_delta)
                - local_martingale_deviation_adjustment,
        })
    }

    pub(crate) fn next_deviation_center_before_core_increment(
        &self,
        deviation: f64,
        next_anchor_value: f64,
    ) -> Result<f64, VanillaOptionPricingRunError> {
        evolve_mean_reversion_deviation_with_decay_on_interval_grid(
            deviation,
            self.deviation_decay_factor,
            0.0,
            self.current_anchor_value,
            next_anchor_value,
        )
        .map_err(
            |source| VanillaOptionPricingRunError::InternalNumericsFailure {
                message: format!("spectral interval deviation center is invalid: {source}"),
            },
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LocalTransitionRowState {
    pub(crate) boundary_state: MeanReversionBoundaryState,
    pub(crate) local_martingale_deviation_adjustment: f64,
    pub(crate) target_log_moment_exponent: f64,
}

impl LocalTransitionRowState {
    fn anchored_boundary_state_summary(
        &self,
        transition_context: &LocalTransitionRequestContext,
    ) -> AnchoredLocalTiltBoundaryStateSummary {
        AnchoredLocalTiltBoundaryStateSummary {
            interval_index: transition_context.interval_index,
            hawkes_boundary_state: transition_context.hawkes_boundary_state,
            observed_boundary_state: self.boundary_state.clone(),
            current_anchor_value: transition_context.current_anchor_value,
            interval_delta: transition_context.interval_delta,
            kappa: transition_context.mean_reversion_fitted_parameters.kappa,
            deviation_decay_factor: transition_context.deviation_decay_factor,
            local_martingale_deviation_adjustment: self.local_martingale_deviation_adjustment,
        }
    }

    pub(crate) fn uses_nested_transition_measure(&self) -> bool {
        self.local_martingale_deviation_adjustment == 0.0
    }
}

pub(crate) struct IntervalTiltCurve<'a> {
    transition_context: &'a LocalTransitionRequestContext,
    driver_parameters: NigBuySellDriverParameters,
    pricing_thread_pool: Option<&'a rayon::ThreadPool>,
    cpu_count: Option<usize>,
    search_domain: LocalTiltSearchDomain,
    core_log_moment_cache: BTreeMap<u64, CorePureImaginaryLogMomentEvaluation>,
    last_solved_theta: Option<f64>,
}

impl<'a> IntervalTiltCurve<'a> {
    pub(crate) fn try_new(
        transition_context: &'a LocalTransitionRequestContext,
        driver_parameters: NigBuySellDriverParameters,
        pricing_thread_pool: Option<&'a rayon::ThreadPool>,
        cpu_count: Option<usize>,
    ) -> Result<Self, VanillaOptionPricingRunError> {
        let search_domain =
            resolve_local_tilt_search_domain(driver_parameters).map_err(|source| {
                VanillaOptionPricingRunError::AnchoredLocalTiltSolveFailure {
                    interval_index: transition_context.interval_index,
                    deviation: 0.0,
                    source: Box::new(source),
                }
            })?;

        Ok(Self {
            transition_context,
            driver_parameters,
            pricing_thread_pool,
            cpu_count,
            search_domain,
            core_log_moment_cache: BTreeMap::new(),
            last_solved_theta: None,
        })
    }

    fn evaluate_core_log_moment(
        &mut self,
        theta: f64,
    ) -> Result<
        CorePureImaginaryLogMomentEvaluation,
        super::characteristic::AnchoredLocalTiltSolveError,
    > {
        let cache_key = theta.to_bits();
        if let Some(evaluation) = self.core_log_moment_cache.get(&cache_key).copied() {
            return Ok(evaluation);
        }

        let evaluation = evaluate_core_pure_imaginary_log_moment_exponent_for_tilt(
            self.transition_context,
            self.driver_parameters,
            theta,
            self.pricing_thread_pool,
            self.cpu_count,
        )?;

        self.core_log_moment_cache.insert(cache_key, evaluation);

        Ok(evaluation)
    }

    fn evaluate_row_residual(
        &mut self,
        transition_row: &LocalTransitionRowState,
        theta: f64,
    ) -> Result<LocalTiltResidualEvaluation, super::characteristic::AnchoredLocalTiltSolveError>
    {
        let core_evaluation = self.evaluate_core_log_moment(theta)?;

        Ok(LocalTiltResidualEvaluation {
            observed_log_moment_exponent: core_evaluation.observed_log_moment_exponent,
            residual: core_evaluation.observed_log_moment_exponent
                - transition_row.target_log_moment_exponent,
        })
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
    _observer: Option<&mut dyn FnMut(PricingNodeProgress)>,
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

    let (
        response,
        representative_transform_diagnostics,
        integration_quality_diagnostics,
        spectral_diagnostics,
    ) = match successful_result {
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

            let maturity_year_fraction = intraday_year_fraction(maturity_interval);

            let response = VanillaOptionPricingResponse::try_new(
                VanillaOptionPricingStatus::Succeeded,
                None,
                Some(spectral_outcome.price),
                Some(
                    VanillaOptionPricingDiagnostics::try_new(
                        pricing_request.market_inputs.spot
                            * ((pricing_request.market_inputs.risk_free_rate
                                - pricing_request.market_inputs.dividend_yield)
                                * maturity_year_fraction)
                                .exp(),
                        (-pricing_request.market_inputs.risk_free_rate * maturity_year_fraction)
                            .exp(),
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
                spectral_outcome.representative_transform,
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
            (response, None, None, None)
        }
    };

    build_vanilla_option_pricing_run_result(VanillaOptionPricingRunResultBuildInput {
        response,
        output_selection,
        market_inputs_for_calibration_hooks: Some(market_inputs_for_calibration_hooks),
        transform_diagnostics_source: representative_transform_diagnostics,
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
) -> FourierInversionDiagnostics {
    let domain_width = spectral_diagnostics.deviation_domain_upper_bound
        - spectral_diagnostics.deviation_domain_lower_bound;

    let integration_step_count = spectral_diagnostics.collocation_node_count.max(1);

    let integration_interval_width = domain_width / integration_step_count as f64;

    let terminal_abs_integrand = 0.0;

    FourierInversionDiagnostics {
        schema_version: FOURIER_INVERSION_DIAGNOSTICS_SCHEMA_VERSION,
        engine_policy: SPECTRAL_PRICING_INTEGRATION_COMPATIBILITY_POLICY,
        contour_policy: SPECTRAL_PRICING_DEVIATION_DOMAIN_POLICY,
        numeric_settings_policy: SPECTRAL_PRICING_COLLOCATION_POLICY,
        stability_policy: SPECTRAL_PRICING_INTEGRATION_COMPATIBILITY_POLICY,
        quadrature_rule: FourierInversionQuadratureRule::Midpoint,
        contour_damping_factor: 1.0,
        contour_imaginary_shift: 1.0,
        integration_upper_bound: domain_width,
        integration_step_count,
        integration_interval_width,
        integrand_evaluation_count: integration_step_count,
        integral_estimate: price,
        absolute_integral_estimate: price.abs(),
        max_abs_integrand: price.abs(),
        terminal_abs_integrand,
        discounted_call_price: price,
        numeric_quality_diagnostics: FourierInversionNumericQualityDiagnostics {
            stability_policy: SPECTRAL_PRICING_INTEGRATION_COMPATIBILITY_POLICY,
            convergence_check_policy: SPECTRAL_PRICING_COLLOCATION_POLICY,
            truncation_check_policy: SPECTRAL_PRICING_DEVIATION_DOMAIN_POLICY,
            quality_gate_policy: FOURIER_INVERSION_NUMERIC_QUALITY_GATE_POLICY,
            quality_gate: FourierInversionNumericQualityGate::DiagnosticsOnly,
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

pub(crate) fn evaluate_core_pure_imaginary_log_moment_exponent_for_tilt(
    transition_context: &LocalTransitionRequestContext,
    driver_parameters: NigBuySellDriverParameters,
    theta: f64,
    pricing_thread_pool: Option<&rayon::ThreadPool>,
    cpu_count: Option<usize>,
) -> Result<CorePureImaginaryLogMomentEvaluation, super::characteristic::AnchoredLocalTiltSolveError>
{
    let evaluation =
        evaluate_core_log_return_characteristic_function_under_local_measure_with_parallelism(
            transition_context,
            driver_parameters,
            theta,
            ComplexScalar::new(0.0, OBSERVED_LOG_MOMENT_ARGUMENT_IMAGINARY),
            pricing_thread_pool,
            cpu_count,
        )
        .map_err(|error| {
            super::characteristic::AnchoredLocalTiltSolveError::TransformEvaluationFailure {
                theta,
                message: error.to_string(),
            }
        })?;

    let observed_log_moment_exponent =
        recover_pure_imaginary_log_moment_exponent(evaluation.value, theta)?;

    Ok(CorePureImaginaryLogMomentEvaluation {
        observed_log_moment_exponent,
        transform_diagnostics: evaluation.transform_diagnostics,
    })
}

pub(crate) fn differentiate_core_pure_imaginary_log_moment_exponent_for_tilt(
    transition_context: &LocalTransitionRequestContext,
    driver_parameters: NigBuySellDriverParameters,
    theta: f64,
) -> Result<CorePureImaginaryLogMomentSharedPlusShiftSensitivity, VanillaOptionPricingRunError> {
    let shifted_argument = ComplexScalar::new(0.0, OBSERVED_LOG_MOMENT_ARGUMENT_IMAGINARY - theta);
    let denominator_argument = ComplexScalar::new(0.0, -theta);

    let numerator_sensitivity =
        evaluate_pricing_log_return_characteristic_function_shared_plus_shift_sensitivity_for_context(
            transition_context,
            driver_parameters,
            shifted_argument,
        )?;

    let denominator_sensitivity =
        evaluate_pricing_log_return_characteristic_function_shared_plus_shift_sensitivity_for_context(
            transition_context,
            driver_parameters,
            denominator_argument,
        )?;

    let risk_neutral_value = numerator_sensitivity
        .value
        .divide(denominator_sensitivity.value)
        .ok_or(
            VanillaOptionPricingRunError::InvalidRiskNeutralCharacteristicFunction {
                numerator: numerator_sensitivity.value,
                denominator: denominator_sensitivity.value,
            },
        )?;

    let alpha_derivative = log_characteristic_modulus_derivative(
        risk_neutral_value,
        pricing_ratio_derivative(
            numerator_sensitivity.value,
            numerator_sensitivity.alpha_derivative,
            denominator_sensitivity.value,
            denominator_sensitivity.alpha_derivative,
        )?,
    )?;

    let beta_0_derivative = log_characteristic_modulus_derivative(
        risk_neutral_value,
        pricing_ratio_derivative(
            numerator_sensitivity.value,
            numerator_sensitivity.beta_0_derivative,
            denominator_sensitivity.value,
            denominator_sensitivity.beta_0_derivative,
        )?,
    )?;

    let delta_derivative = log_characteristic_modulus_derivative(
        risk_neutral_value,
        pricing_ratio_derivative(
            numerator_sensitivity.value,
            numerator_sensitivity.delta_derivative,
            denominator_sensitivity.value,
            denominator_sensitivity.delta_derivative,
        )?,
    )?;

    let mu_derivative = log_characteristic_modulus_derivative(
        risk_neutral_value,
        pricing_ratio_derivative(
            numerator_sensitivity.value,
            numerator_sensitivity.mu_derivative,
            denominator_sensitivity.value,
            denominator_sensitivity.mu_derivative,
        )?,
    )?;

    let delta_beta_derivative = log_characteristic_modulus_derivative(
        risk_neutral_value,
        pricing_ratio_derivative(
            numerator_sensitivity.value,
            numerator_sensitivity.delta_beta_derivative,
            denominator_sensitivity.value,
            denominator_sensitivity.delta_beta_derivative,
        )?,
    )?;

    let theta_derivative = log_characteristic_modulus_derivative(
        risk_neutral_value,
        pricing_ratio_derivative(
            numerator_sensitivity.value,
            numerator_sensitivity.theta_derivative,
            denominator_sensitivity.value,
            denominator_sensitivity.theta_derivative,
        )?,
    )?;

    Ok(CorePureImaginaryLogMomentSharedPlusShiftSensitivity {
        observed_log_moment_exponent: risk_neutral_value.norm().ln(),
        alpha_derivative,
        beta_0_derivative,
        delta_derivative,
        mu_derivative,
        delta_beta_derivative,
        theta_derivative,
    })
}

pub(crate) fn differentiate_core_local_tilt_theta_for_tilt(
    transition_context: &LocalTransitionRequestContext,
    driver_parameters: NigBuySellDriverParameters,
    theta: f64,
) -> Result<CoreLocalTiltImplicitThetaSharedPlusShiftSensitivity, VanillaOptionPricingRunError> {
    let sensitivity = differentiate_core_pure_imaginary_log_moment_exponent_for_tilt(
        transition_context,
        driver_parameters,
        theta,
    )?;
    if !sensitivity.theta_derivative.is_finite() || sensitivity.theta_derivative == 0.0 {
        return Err(VanillaOptionPricingRunError::InternalNumericsFailure {
            message: format!(
                "core local-tilt implicit theta sensitivity is undefined at interval={} theta={} with theta_derivative={}",
                transition_context.interval_index, theta, sensitivity.theta_derivative,
            ),
        });
    }

    let inverse_theta_derivative = -1.0 / sensitivity.theta_derivative;

    Ok(CoreLocalTiltImplicitThetaSharedPlusShiftSensitivity {
        alpha_derivative: sensitivity.alpha_derivative * inverse_theta_derivative,
        beta_0_derivative: sensitivity.beta_0_derivative * inverse_theta_derivative,
        delta_derivative: sensitivity.delta_derivative * inverse_theta_derivative,
        delta_beta_derivative: sensitivity.delta_beta_derivative * inverse_theta_derivative,
    })
}

fn build_local_tilt_diagnostics_for_row(
    transition_context: &LocalTransitionRequestContext,
    transition_row: &LocalTransitionRowState,
    theta: f64,
    evaluation: LocalTiltResidualEvaluation,
) -> AnchoredLocalTiltSolveDiagnostics {
    AnchoredLocalTiltSolveDiagnostics {
        boundary_state_summary: transition_row.anchored_boundary_state_summary(transition_context),
        target_log_moment_exponent: transition_row.target_log_moment_exponent,
        observed_log_moment_exponent: evaluation.observed_log_moment_exponent,
        theta,
        residual: evaluation.residual,
        absolute_residual: evaluation.residual.abs(),
        tolerance: local_tilt_tolerance(),
        domain_status: super::contracts::AnchoredLocalTiltDomainStatus::InDomain,
        admissibility_status: super::contracts::AnchoredLocalTiltAdmissibilityStatus::Admissible,
        policy: super::constants::PRICING_LOCAL_ANCHORED_TILT_SOLVE_POLICY.to_string(),
    }
}

pub(crate) fn resolve_local_transition_theta_for_row_with_curve(
    interval_tilt_curve: &mut IntervalTiltCurve<'_>,
    transition_row: &LocalTransitionRowState,
) -> Result<(f64, Option<AnchoredLocalTiltSolveDiagnostics>), VanillaOptionPricingRunError> {
    if transition_row.uses_nested_transition_measure() {
        interval_tilt_curve.last_solved_theta = Some(0.0);
        return Ok((0.0, None));
    }

    if let Some(previous_theta) = interval_tilt_curve.last_solved_theta {
        match interval_tilt_curve.evaluate_row_residual(transition_row, previous_theta) {
            Ok(previous_evaluation) => {
                if previous_evaluation.residual.abs() <= local_tilt_tolerance() {
                    let diagnostics = build_local_tilt_diagnostics_for_row(
                        interval_tilt_curve.transition_context,
                        transition_row,
                        previous_theta,
                        previous_evaluation,
                    );

                    return Ok((previous_theta, Some(diagnostics)));
                }
            }
            Err(source) if should_retry_local_tilt_side_evaluation(&source) => {
                interval_tilt_curve.last_solved_theta = None;
            }
            Err(source) => {
                return Err(
                    VanillaOptionPricingRunError::AnchoredLocalTiltSolveFailure {
                        interval_index: interval_tilt_curve.transition_context.interval_index,
                        deviation: transition_row.boundary_state.deviation,
                        source: Box::new(source),
                    },
                );
            }
        }
    }

    let core_target_log_moment_exponent = transition_row.target_log_moment_exponent;
    let (theta, core_evaluation) =
        build_local_tilt_bracket_with_search_domain_and_residual_evaluator(
            interval_tilt_curve.search_domain,
            core_target_log_moment_exponent,
            local_tilt_tolerance(),
            |theta| {
                let core_log_moment_evaluation =
                    interval_tilt_curve.evaluate_core_log_moment(theta)?;

                Ok(LocalTiltResidualEvaluation {
                    observed_log_moment_exponent: core_log_moment_evaluation
                        .observed_log_moment_exponent,
                    residual: core_log_moment_evaluation.observed_log_moment_exponent
                        - core_target_log_moment_exponent,
                })
            },
        )
        .map_err(
            |source| VanillaOptionPricingRunError::AnchoredLocalTiltSolveFailure {
                interval_index: interval_tilt_curve.transition_context.interval_index,
                deviation: transition_row.boundary_state.deviation,
                source: Box::new(source),
            },
        )?;

    interval_tilt_curve.last_solved_theta = Some(theta);

    let diagnostics = build_local_tilt_diagnostics_for_row(
        interval_tilt_curve.transition_context,
        transition_row,
        theta,
        core_evaluation,
    );

    Ok((theta, Some(diagnostics)))
}

pub(crate) fn resolve_local_transition_theta(
    pricing_request: &VanillaOptionPricingRequest,
    interval_factors: VanillaOptionPricingIntervalFactors,
    driver_parameters: NigBuySellDriverParameters,
) -> Result<(f64, Option<AnchoredLocalTiltSolveDiagnostics>), VanillaOptionPricingRunError> {
    resolve_local_transition_theta_with_parallelism(
        pricing_request,
        interval_factors,
        driver_parameters,
        None,
        None,
    )
}

pub(crate) fn resolve_local_transition_theta_with_parallelism(
    pricing_request: &VanillaOptionPricingRequest,
    interval_factors: VanillaOptionPricingIntervalFactors,
    driver_parameters: NigBuySellDriverParameters,
    pricing_thread_pool: Option<&rayon::ThreadPool>,
    cpu_count: Option<usize>,
) -> Result<(f64, Option<AnchoredLocalTiltSolveDiagnostics>), VanillaOptionPricingRunError> {
    let transition_context =
        LocalTransitionRequestContext::from_pricing_request(pricing_request, interval_factors)?;

    let transition_row = transition_context.build_row_state(
        pricing_request
            .boundary_input
            .observed_boundary_state
            .deviation,
    )?;

    resolve_local_transition_theta_for_row_with_parallelism(
        &transition_context,
        &transition_row,
        driver_parameters,
        pricing_thread_pool,
        cpu_count,
    )
}

pub(crate) fn resolve_local_transition_theta_for_row_with_parallelism(
    transition_context: &LocalTransitionRequestContext,
    transition_row: &LocalTransitionRowState,
    driver_parameters: NigBuySellDriverParameters,
    pricing_thread_pool: Option<&rayon::ThreadPool>,
    cpu_count: Option<usize>,
) -> Result<(f64, Option<AnchoredLocalTiltSolveDiagnostics>), VanillaOptionPricingRunError> {
    resolve_local_transition_theta_for_row_with_parallelism_and_hint(
        transition_context,
        transition_row,
        driver_parameters,
        None,
        pricing_thread_pool,
        cpu_count,
    )
}

pub(crate) fn resolve_local_transition_theta_for_row_with_parallelism_and_hint(
    transition_context: &LocalTransitionRequestContext,
    transition_row: &LocalTransitionRowState,
    driver_parameters: NigBuySellDriverParameters,
    initial_theta_hint: Option<f64>,
    pricing_thread_pool: Option<&rayon::ThreadPool>,
    cpu_count: Option<usize>,
) -> Result<(f64, Option<AnchoredLocalTiltSolveDiagnostics>), VanillaOptionPricingRunError> {
    let mut interval_tilt_curve = IntervalTiltCurve::try_new(
        transition_context,
        driver_parameters,
        pricing_thread_pool,
        cpu_count,
    )?;

    interval_tilt_curve.last_solved_theta = initial_theta_hint;

    resolve_local_transition_theta_for_row_with_curve(&mut interval_tilt_curve, transition_row)
}

pub(crate) fn certify_local_transition_theta_for_row_with_parallelism(
    transition_context: &LocalTransitionRequestContext,
    transition_row: &LocalTransitionRowState,
    driver_parameters: NigBuySellDriverParameters,
    theta_candidate: f64,
    pricing_thread_pool: Option<&rayon::ThreadPool>,
    cpu_count: Option<usize>,
) -> Result<bool, VanillaOptionPricingRunError> {
    let mut interval_tilt_curve = IntervalTiltCurve::try_new(
        transition_context,
        driver_parameters,
        pricing_thread_pool,
        cpu_count,
    )?;

    match interval_tilt_curve.evaluate_row_residual(transition_row, theta_candidate) {
        Ok(evaluation) => Ok(evaluation.residual.abs() <= local_tilt_tolerance()),
        Err(source) if should_retry_local_tilt_side_evaluation(&source) => Ok(false),
        Err(source) => Err(
            VanillaOptionPricingRunError::AnchoredLocalTiltSolveFailure {
                interval_index: transition_context.interval_index,
                deviation: transition_row.boundary_state.deviation,
                source: Box::new(source),
            },
        ),
    }
}

#[cfg(test)]
pub(crate) fn prepare_transition_row_kernel_with_curve(
    interval_tilt_curve: &mut IntervalTiltCurve<'_>,
    transition_row: LocalTransitionRowState,
) -> Result<PreparedTransitionRowKernel, VanillaOptionPricingRunError> {
    let (theta, tilt_diagnostics) =
        resolve_local_transition_theta_for_row_with_curve(interval_tilt_curve, &transition_row)?;

    let local_expectations = derive_local_tilted_interval_expectations_for_row_with_parallelism(
        interval_tilt_curve.transition_context,
        &transition_row,
        interval_tilt_curve.driver_parameters,
        theta,
        interval_tilt_curve.pricing_thread_pool,
        interval_tilt_curve.cpu_count,
    )?;

    let representative_transform_diagnostics = if transition_row.uses_nested_transition_measure() {
        None
    } else {
        Some(
            interval_tilt_curve
                .evaluate_core_log_moment(theta)
                .map_err(
                    |source| VanillaOptionPricingRunError::AnchoredLocalTiltSolveFailure {
                        interval_index: interval_tilt_curve.transition_context.interval_index,
                        deviation: transition_row.boundary_state.deviation,
                        source: Box::new(source),
                    },
                )?
                .transform_diagnostics,
        )
    };

    Ok(PreparedTransitionRowKernel {
        transition_row,
        theta,
        tilt_diagnostics,
        denominator: ComplexScalar::new(1.0, 0.0),
        local_expectations,
        representative_transform_diagnostics,
    })
}

pub(crate) fn recover_pure_imaginary_log_moment_exponent(
    characteristic_value: ComplexScalar,
    theta: f64,
) -> Result<f64, super::characteristic::AnchoredLocalTiltSolveError> {
    if !characteristic_value.is_finite() {
        return Err(
            super::characteristic::AnchoredLocalTiltSolveError::NonFiniteTransformInput {
                theta,
                value: characteristic_value,
            },
        );
    }

    // Along the pure-imaginary axis this quantity is an exponential moment,
    // so the exact value is strictly positive real and the log moment is the
    // logarithm of the characteristic-factor modulus.
    let log_moment_exponent = characteristic_value.norm().ln();
    if !log_moment_exponent.is_finite() {
        return Err(
            super::characteristic::AnchoredLocalTiltSolveError::NonFiniteTransformInput {
                theta,
                value: characteristic_value,
            },
        );
    }

    Ok(log_moment_exponent)
}

pub(crate) fn build_pricing_sufficient_statistics_neutral_anchor_materialization(
    pricing_request: &VanillaOptionPricingRequest,
    runtime_interval_schedule: &PricingIntervalSchedule,
    interval_factors: &[VanillaOptionPricingIntervalFactors],
    driver_parameters: NigBuySellDriverParameters,
    observer: &mut Option<&mut dyn FnMut(PricingNodeProgress)>,
) -> Result<RiskNeutralAnchorMaterialization, VanillaOptionPricingRunError> {
    let mut current_boundary_state = pricing_request
        .boundary_input
        .observed_boundary_state
        .clone();

    let mut current_hawkes_state = pricing_request.boundary_input.hawkes_boundary_state;

    let mut end_anchor_values = Vec::with_capacity(interval_factors.len());
    let mut end_pressure_states = Vec::with_capacity(interval_factors.len());
    let mut interval_deltas = Vec::with_capacity(interval_factors.len());
    let mut expected_hawkes_state_path = Vec::with_capacity(interval_factors.len() + 1);

    expected_hawkes_state_path.push(current_hawkes_state);

    for (interval_index, interval_factor) in interval_factors.iter().copied().enumerate() {
        let interval_delta = runtime_interval_schedule
            .interval_delta_hours(interval_index)
            .ok_or_else(|| VanillaOptionPricingRunError::EndogenousAnchorMaterializationFailure {
                message: format!(
                    "runtime interval schedule is missing interval delta for interval_index={interval_index}"
                ),
            })?;

        emit_node_progress(
            observer,
            PricingNodeProgress::for_interval(
                PricingNodeProgressPhase::MaterializingRiskNeutralAnchors,
                interval_index,
                interval_factors.len(),
            ),
        );

        let local_boundary_input = build_local_boundary_input(
            pricing_request,
            pricing_request.boundary_input.interval_index + interval_index,
            current_hawkes_state,
            current_boundary_state.anchor_value,
            current_boundary_state.pressure_state,
            current_boundary_state.deviation,
            interval_delta,
        )?;

        let local_request = build_local_pricing_request(
            pricing_request,
            local_boundary_input.clone(),
            interval_delta,
        )?;

        let (local_transition_theta, _) =
            resolve_local_transition_theta(&local_request, interval_factor, driver_parameters)?;

        let local_expectations = derive_local_tilted_interval_expectations(
            &local_request,
            driver_parameters,
            interval_factor,
            local_transition_theta,
        )?;

        current_boundary_state = evolve_mean_reversion_boundary_state_with_endogenous_pressure(
            current_boundary_state,
            &pricing_request
                .boundary_input
                .mean_reversion_fitted_parameters,
            interval_delta,
            local_expectations.core_expected_increment,
            local_expectations.core_expected_increment,
        )
        .map_err(|source| {
            VanillaOptionPricingRunError::EndogenousAnchorMaterializationFailure {
                message: source.to_string(),
            }
        })?;

        current_hawkes_state = evolve_expected_hawkes_state_under_local_measure(
            &local_request,
            interval_factor,
            current_hawkes_state,
            local_expectations,
        )?;

        end_anchor_values.push(current_boundary_state.anchor_value);
        end_pressure_states.push(current_boundary_state.pressure_state);
        interval_deltas.push(interval_delta);
        expected_hawkes_state_path.push(current_hawkes_state);
    }

    let runtime_interval_anchors = MeanReversionRuntimeIntervalAnchors::try_new(
        "pricing_internal_endogenous_q_anchor_recursion.v1",
        pricing_request
            .boundary_input
            .observed_boundary_state
            .anchor_value,
        pricing_request
            .boundary_input
            .observed_boundary_state
            .pressure_state,
        end_anchor_values,
        end_pressure_states,
        interval_deltas,
    )
    .map_err(|source| {
        VanillaOptionPricingRunError::EndogenousAnchorMaterializationFailure {
            message: source.to_string(),
        }
    })?;

    Ok(RiskNeutralAnchorMaterialization {
        runtime_interval_anchors,
        expected_hawkes_state_path,
    })
}

pub(crate) fn derive_local_tilted_interval_expectations(
    pricing_request: &VanillaOptionPricingRequest,
    driver_parameters: NigBuySellDriverParameters,
    interval_factors: VanillaOptionPricingIntervalFactors,
    theta: f64,
) -> Result<LocalTiltedIntervalExpectations, VanillaOptionPricingRunError> {
    derive_local_tilted_interval_expectations_with_parallelism(
        pricing_request,
        driver_parameters,
        interval_factors,
        theta,
        None,
        None,
    )
}

pub(crate) fn derive_local_tilted_interval_expectations_with_parallelism(
    pricing_request: &VanillaOptionPricingRequest,
    driver_parameters: NigBuySellDriverParameters,
    interval_factors: VanillaOptionPricingIntervalFactors,
    theta: f64,
    pricing_thread_pool: Option<&rayon::ThreadPool>,
    cpu_count: Option<usize>,
) -> Result<LocalTiltedIntervalExpectations, VanillaOptionPricingRunError> {
    let transition_context =
        LocalTransitionRequestContext::from_pricing_request(pricing_request, interval_factors)?;

    let transition_row = transition_context.build_row_state(
        pricing_request
            .boundary_input
            .observed_boundary_state
            .deviation,
    )?;

    derive_local_tilted_interval_expectations_for_row_with_parallelism(
        &transition_context,
        &transition_row,
        driver_parameters,
        theta,
        pricing_thread_pool,
        cpu_count,
    )
}

pub(crate) fn derive_local_tilted_interval_expectations_for_row_with_parallelism(
    transition_context: &LocalTransitionRequestContext,
    transition_row: &LocalTransitionRowState,
    driver_parameters: NigBuySellDriverParameters,
    theta: f64,
    pricing_thread_pool: Option<&rayon::ThreadPool>,
    cpu_count: Option<usize>,
) -> Result<LocalTiltedIntervalExpectations, VanillaOptionPricingRunError> {
    let nested_projection = if transition_row.uses_nested_transition_measure() {
        Some(derive_nested_local_expected_interval_projection_from_context(transition_context)?)
    } else {
        None
    };

    let (tilted_statistics, expected_hawkes_state_after) = match nested_projection {
        Some(projection) => (
            projection.expected_statistics,
            Some(projection.structural_state_after),
        ),
        None => derive_local_tilted_expected_interval_statistics_for_row(
            transition_context,
            transition_row,
            driver_parameters,
            theta,
            pricing_thread_pool,
            cpu_count,
        )
        .map(|expected_statistics| (expected_statistics, None))?,
    };

    let core_expected_increment = tilted_core_expected_increment_from_statistics(
        driver_parameters,
        tilted_statistics,
        theta,
    )?;

    Ok(LocalTiltedIntervalExpectations {
        expected_statistics: tilted_statistics,
        core_expected_increment,
        expected_hawkes_state_after,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TransformSliceKind {
    Baseline,
    APlus,
    AMinus,
    NPlus,
    NMinus,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct TransformSliceRequest {
    kind: TransformSliceKind,
    extra_u_plus: ComplexScalar,
    extra_u_minus: ComplexScalar,
    z_plus: ComplexScalar,
    z_minus: ComplexScalar,
}

impl TransformSliceRequest {
    fn baseline() -> Self {
        Self {
            kind: TransformSliceKind::Baseline,
            extra_u_plus: ComplexScalar::new(0.0, 0.0),
            extra_u_minus: ComplexScalar::new(0.0, 0.0),
            z_plus: ComplexScalar::new(1.0, 0.0),
            z_minus: ComplexScalar::new(1.0, 0.0),
        }
    }

    fn a_plus(a_plus_u: f64) -> Self {
        Self {
            kind: TransformSliceKind::APlus,
            extra_u_plus: ComplexScalar::new(a_plus_u, 0.0),
            extra_u_minus: ComplexScalar::new(0.0, 0.0),
            z_plus: ComplexScalar::new(1.0, 0.0),
            z_minus: ComplexScalar::new(1.0, 0.0),
        }
    }

    fn a_minus(a_minus_u: f64) -> Self {
        Self {
            kind: TransformSliceKind::AMinus,
            extra_u_plus: ComplexScalar::new(0.0, 0.0),
            extra_u_minus: ComplexScalar::new(a_minus_u, 0.0),
            z_plus: ComplexScalar::new(1.0, 0.0),
            z_minus: ComplexScalar::new(1.0, 0.0),
        }
    }

    fn n_plus(n_plus_z: f64) -> Self {
        Self {
            kind: TransformSliceKind::NPlus,
            extra_u_plus: ComplexScalar::new(0.0, 0.0),
            extra_u_minus: ComplexScalar::new(0.0, 0.0),
            z_plus: ComplexScalar::new(n_plus_z, 0.0),
            z_minus: ComplexScalar::new(1.0, 0.0),
        }
    }

    fn n_minus(n_minus_z: f64) -> Self {
        Self {
            kind: TransformSliceKind::NMinus,
            extra_u_plus: ComplexScalar::new(0.0, 0.0),
            extra_u_minus: ComplexScalar::new(0.0, 0.0),
            z_plus: ComplexScalar::new(1.0, 0.0),
            z_minus: ComplexScalar::new(n_minus_z, 0.0),
        }
    }
}

fn derive_local_tilted_expected_interval_statistics_for_row(
    transition_context: &LocalTransitionRequestContext,
    transition_row: &LocalTransitionRowState,
    driver_parameters: NigBuySellDriverParameters,
    theta: f64,
    pricing_thread_pool: Option<&rayon::ThreadPool>,
    cpu_count: Option<usize>,
) -> Result<SufficientStatisticsExpectedIntervalStatistics, VanillaOptionPricingRunError> {
    if transition_row.uses_nested_transition_measure() {
        return derive_nested_local_expected_interval_statistics_from_context(transition_context);
    }

    let settings = SufficientStatisticsTransformSliceSettings::default();

    let context = SufficientStatisticsTransformSliceContext::try_new(
        transition_context.interval_index,
        transition_context.interval_factors.s_h,
        transition_context.news_intensity,
        transition_context.interval_delta,
        transition_context.hawkes_boundary_state,
    )
    .map_err(|source| {
        VanillaOptionPricingRunError::EndogenousAnchorMaterializationFailure {
            message: source.to_string(),
        }
    })?;

    let slice_requests = [
        TransformSliceRequest::baseline(),
        TransformSliceRequest::a_plus(settings.a_plus_u),
        TransformSliceRequest::a_minus(settings.a_minus_u),
        TransformSliceRequest::n_plus(settings.n_plus_z),
        TransformSliceRequest::n_minus(settings.n_minus_z),
    ];

    let slice_results = if resolved_pricing_cpu_count(cpu_count) > 1 {
        install_pricing_parallelism(pricing_thread_pool, || {
            slice_requests
                .into_par_iter()
                .map(|request| {
                    Ok::<_, VanillaOptionPricingRunError>((
                        request.kind,
                        solve_local_tilted_transform_slice_value(
                            transition_context,
                            driver_parameters,
                            theta,
                            request.extra_u_plus,
                            request.extra_u_minus,
                            request.z_plus,
                            request.z_minus,
                        )?,
                    ))
                })
                .collect::<Result<Vec<_>, _>>()
        })?
    } else {
        slice_requests
            .into_iter()
            .map(|request| {
                Ok::<_, VanillaOptionPricingRunError>((
                    request.kind,
                    solve_local_tilted_transform_slice_value(
                        transition_context,
                        driver_parameters,
                        theta,
                        request.extra_u_plus,
                        request.extra_u_minus,
                        request.z_plus,
                        request.z_minus,
                    )?,
                ))
            })
            .collect::<Result<Vec<_>, _>>()?
    };

    let mut baseline_transform = None;
    let mut a_plus_transform = None;
    let mut a_minus_transform = None;
    let mut n_plus_transform = None;
    let mut n_minus_transform = None;

    for (kind, value) in slice_results {
        match kind {
            TransformSliceKind::Baseline => baseline_transform = Some(value),
            TransformSliceKind::APlus => a_plus_transform = Some(value),
            TransformSliceKind::AMinus => a_minus_transform = Some(value),
            TransformSliceKind::NPlus => n_plus_transform = Some(value),
            TransformSliceKind::NMinus => n_minus_transform = Some(value),
        }
    }

    let slices = SufficientStatisticsTransformSlices::try_new(
        context,
        settings,
        baseline_transform.ok_or_else(|| {
            VanillaOptionPricingRunError::InternalNumericsFailure {
                message: "missing baseline transform slice".to_string(),
            }
        })?,
        a_plus_transform.ok_or_else(|| VanillaOptionPricingRunError::InternalNumericsFailure {
            message: "missing a_plus transform slice".to_string(),
        })?,
        a_minus_transform.ok_or_else(|| VanillaOptionPricingRunError::InternalNumericsFailure {
            message: "missing a_minus transform slice".to_string(),
        })?,
        n_plus_transform.ok_or_else(|| VanillaOptionPricingRunError::InternalNumericsFailure {
            message: "missing n_plus transform slice".to_string(),
        })?,
        n_minus_transform.ok_or_else(|| VanillaOptionPricingRunError::InternalNumericsFailure {
            message: "missing n_minus transform slice".to_string(),
        })?,
    )
    .map_err(|source| {
        VanillaOptionPricingRunError::EndogenousAnchorMaterializationFailure {
            message: source.to_string(),
        }
    })?;

    derive_expected_interval_statistics_from_transform_slices(slices).map_err(|source| {
        VanillaOptionPricingRunError::EndogenousAnchorMaterializationFailure {
            message: source.to_string(),
        }
    })
}

fn derive_nested_local_expected_interval_statistics_from_context(
    transition_context: &LocalTransitionRequestContext,
) -> Result<SufficientStatisticsExpectedIntervalStatistics, VanillaOptionPricingRunError> {
    derive_nested_local_expected_interval_projection_from_context(transition_context)
        .map(|projection| projection.expected_statistics)
}

fn derive_nested_local_expected_interval_projection_from_context(
    transition_context: &LocalTransitionRequestContext,
) -> Result<
    janiform_affine_oracle::endogenous_anchor::EndogenousAnchorFirstMomentIntervalProjection,
    VanillaOptionPricingRunError,
> {
    let endogenous_factors = EndogenousAnchorIntervalFactors::try_new(
        transition_context.interval_factors.s_h,
        transition_context.interval_factors.nu_h_known,
        transition_context.interval_factors.nu_h_surprise,
        transition_context.interval_delta,
    )
    .map_err(|source| {
        VanillaOptionPricingRunError::EndogenousAnchorMaterializationFailure {
            message: source.to_string(),
        }
    })?;

    let expected_severity_mark = transition_context
        .model_parameters
        .news_process_configuration
        .shared_mark_law
        .gamma_severity_marks
        .expected_severity_mark()
        .map_err(
            |source| VanillaOptionPricingRunError::EndogenousAnchorMaterializationFailure {
                message: source.to_string(),
            },
        )?;

    derive_expected_interval_projection_from_first_moment_dynamics(
        transition_context.interval_index,
        transition_context.equation_specification,
        expected_severity_mark,
        endogenous_factors,
        transition_context.hawkes_boundary_state,
    )
    .map_err(
        |source| VanillaOptionPricingRunError::EndogenousAnchorMaterializationFailure {
            message: source.to_string(),
        },
    )
}

#[allow(clippy::too_many_arguments)]
fn solve_local_tilted_transform_slice_value(
    transition_context: &LocalTransitionRequestContext,
    driver_parameters: NigBuySellDriverParameters,
    theta: f64,
    extra_u_plus: ComplexScalar,
    extra_u_minus: ComplexScalar,
    z_plus: ComplexScalar,
    z_minus: ComplexScalar,
) -> Result<ComplexScalar, VanillaOptionPricingRunError> {
    let shifted_argument = ComplexScalar::new(0.0, -theta);

    let buy_side_shift = driver_parameters
        .buy_side_characteristic_exponent(shifted_argument)
        .scale(-1.0)
        .plus(extra_u_plus);

    let sell_side_shift = driver_parameters
        .sell_side_characteristic_exponent_for_return_composition(shifted_argument)
        .scale(-1.0)
        .plus(extra_u_minus);

    let transform_input =
        transition_context.build_transform_input(buy_side_shift, sell_side_shift, z_plus, z_minus);

    let transform_diagnostics =
        solve_exponential_affine_transform_with_internal_selection(transform_input).map_err(
            |source| VanillaOptionPricingRunError::TransformSolveFailure {
                argument: shifted_argument,
                source: Box::new(source),
            },
        )?;

    let transform_output = transform_diagnostics.output.ok_or(
        VanillaOptionPricingRunError::MissingTransformOutput {
            argument: shifted_argument,
            status: transform_diagnostics.status,
            context: format!(
                "local_tilted_transform_slice interval_index={} interval_delta={} u_plus={buy_side_shift:?} u_minus={sell_side_shift:?} z_plus={z_plus:?} z_minus={z_minus:?} guard_failure_code={:?}",
                transition_context.interval_index,
                transition_context.interval_delta,
                transform_diagnostics
                    .metadata
                    .guard_failure
                    .map(|guard_failure| guard_failure.code),
            ),
        },
    )?;

    Ok(transform_output.transform_value)
}

pub(crate) fn tilted_core_expected_increment_from_statistics(
    driver_parameters: NigBuySellDriverParameters,
    expected_statistics: SufficientStatisticsExpectedIntervalStatistics,
    theta: f64,
) -> Result<f64, VanillaOptionPricingRunError> {
    let tilted_buy_expected_increment = tilted_nig_expected_increment(
        driver_parameters.buy_side_parameters,
        expected_statistics.expected_a_plus,
        theta,
    )?;

    let tilted_sell_expected_increment = tilted_nig_expected_increment(
        driver_parameters.sell_side_parameters,
        expected_statistics.expected_a_minus,
        -theta,
    )?;

    Ok(tilted_buy_expected_increment - tilted_sell_expected_increment)
}

fn tilted_nig_expected_increment(
    physical_parameters: janiform_model_kernels::nig::NigParameters,
    business_time_increment_scale: f64,
    beta_shift: f64,
) -> Result<f64, VanillaOptionPricingRunError> {
    let tilted_expected_increment = janiform_model_kernels::nig::NigParameters::try_new(
        physical_parameters.alpha,
        physical_parameters.beta + beta_shift,
        physical_parameters.delta,
        physical_parameters.mu,
    )
    .map_err(
        |source| VanillaOptionPricingRunError::EndogenousAnchorMaterializationFailure {
            message: source.to_string(),
        },
    )?
    .expected_increment(business_time_increment_scale)
    .map_err(|source| {
        VanillaOptionPricingRunError::EndogenousAnchorMaterializationFailure {
            message: source.to_string(),
        }
    })?;

    let physical_expected_increment = physical_parameters
        .expected_increment(business_time_increment_scale)
        .map_err(
            |source| VanillaOptionPricingRunError::EndogenousAnchorMaterializationFailure {
                message: source.to_string(),
            },
        )?;

    Ok(tilted_expected_increment - physical_expected_increment)
}

fn evolve_expected_hawkes_state_under_local_measure(
    pricing_request: &VanillaOptionPricingRequest,
    interval_factors: VanillaOptionPricingIntervalFactors,
    state_before_update: HawkesExcitationState,
    local_expectations: LocalTiltedIntervalExpectations,
) -> Result<HawkesExcitationState, VanillaOptionPricingRunError> {
    if let Some(expected_hawkes_state_after) = local_expectations.expected_hawkes_state_after {
        return Ok(expected_hawkes_state_after);
    }

    let kernel_parameters = pricing_request.model_parameters.hawkes_kernel_parameters;

    evolve_expected_hawkes_state_from_integrated_statistics(
        kernel_parameters,
        interval_factors.s_h,
        pricing_request.interval_delta,
        state_before_update,
        local_expectations.expected_statistics,
    )
    .map_err(|source| VanillaOptionPricingRunError::ExpectedHawkesStateUpdateFailure {
        message: format!(
            "{source}; interval_index={} interval_delta={} s_h={} nu_h_known={} nu_h_surprise={} expected_a_plus={} expected_a_minus={} expected_n_plus={} expected_n_minus={}",
            pricing_request.boundary_input.interval_index,
            pricing_request.interval_delta,
            interval_factors.s_h,
            interval_factors.nu_h_known,
            interval_factors.nu_h_surprise,
            local_expectations.expected_statistics.expected_a_plus,
            local_expectations.expected_statistics.expected_a_minus,
            local_expectations.expected_statistics.expected_n_plus,
            local_expectations.expected_statistics.expected_n_minus,
        ),
    })
}

pub(crate) fn build_local_boundary_input(
    pricing_request: &VanillaOptionPricingRequest,
    interval_index: usize,
    hawkes_boundary_state: HawkesExcitationState,
    anchor_value: f64,
    pressure_state: f64,
    deviation: f64,
    interval_delta: f64,
) -> Result<AnchoredVanillaOptionPricingBoundaryInput, VanillaOptionPricingRunError> {
    let observed_boundary_state = MeanReversionBoundaryState::try_new(
        anchor_value + deviation,
        anchor_value,
        pressure_state,
        deviation,
    )
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
        interval_delta,
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

pub(crate) fn build_local_pricing_request(
    pricing_request: &VanillaOptionPricingRequest,
    boundary_input: AnchoredVanillaOptionPricingBoundaryInput,
    interval_delta: f64,
) -> Result<VanillaOptionPricingRequest, VanillaOptionPricingRunError> {
    VanillaOptionPricingRequest::try_new(
        super::contracts::VanillaOptionPricingMarketInputs::try_new_single_node(
            pricing_request.market_inputs.spot,
            pricing_request
                .market_inputs
                .single_strike()
                .map_err(
                    |source| VanillaOptionPricingRunError::InvalidExecutionInput {
                        source: Box::new(VanillaOptionPricingExecutionInputError::InvalidRequest {
                            source: VanillaOptionPricingRequestError::InvalidMarketInputs {
                                source,
                            },
                        }),
                    },
                )?,
            interval_delta,
            pricing_request.market_inputs.risk_free_rate,
            pricing_request.market_inputs.dividend_yield,
        )
        .map_err(
            |source| VanillaOptionPricingRunError::InvalidExecutionInput {
                source: Box::new(VanillaOptionPricingExecutionInputError::InvalidRequest {
                    source: VanillaOptionPricingRequestError::InvalidMarketInputs { source },
                }),
            },
        )?,
        pricing_request.payoff,
        boundary_input,
        pricing_request.model_parameters.clone(),
        interval_delta,
    )
    .map_err(
        |source| VanillaOptionPricingRunError::InvalidExecutionInput {
            source: Box::new(VanillaOptionPricingExecutionInputError::InvalidRequest { source }),
        },
    )
}

pub(crate) fn evaluate_core_log_return_characteristic_function_under_local_measure_with_parallelism(
    transition_context: &LocalTransitionRequestContext,
    driver_parameters: NigBuySellDriverParameters,
    theta: f64,
    argument: ComplexScalar,
    pricing_thread_pool: Option<&rayon::ThreadPool>,
    cpu_count: Option<usize>,
) -> Result<PricingCharacteristicFunctionEvaluation, VanillaOptionPricingRunError> {
    let shifted_argument = ComplexScalar::new(argument.real, argument.imaginary - theta);
    let denominator_argument = ComplexScalar::new(0.0, -theta);

    let (numerator, denominator) = if resolved_pricing_cpu_count(cpu_count) > 1 {
        install_pricing_parallelism(pricing_thread_pool, || {
            join(
                || {
                    evaluate_pricing_log_return_characteristic_function_for_context(
                        transition_context,
                        driver_parameters,
                        shifted_argument,
                    )
                },
                || {
                    evaluate_pricing_log_return_characteristic_function_for_context(
                        transition_context,
                        driver_parameters,
                        denominator_argument,
                    )
                },
            )
        })
    } else {
        (
            evaluate_pricing_log_return_characteristic_function_for_context(
                transition_context,
                driver_parameters,
                shifted_argument,
            ),
            evaluate_pricing_log_return_characteristic_function_for_context(
                transition_context,
                driver_parameters,
                denominator_argument,
            ),
        )
    };

    let numerator = numerator?;
    let denominator = denominator?;

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

fn local_tilt_tolerance() -> f64 {
    f64::EPSILON.sqrt()
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
        VanillaOptionPricingRunError::TransformInputBuildFailure { .. } => {
            VanillaOptionPricingFailureStage::TransformInputBuild
        }
        VanillaOptionPricingRunError::TransformSolveFailure { .. } => {
            VanillaOptionPricingFailureStage::TransformSolve
        }
        VanillaOptionPricingRunError::MissingTransformOutput { .. } => {
            VanillaOptionPricingFailureStage::TransformOutput
        }
        VanillaOptionPricingRunError::CharacteristicFunctionAssemblyFailure { .. } => {
            VanillaOptionPricingFailureStage::CharacteristicFunctionAssembly
        }
        VanillaOptionPricingRunError::InternalNumericsFailure { .. } => {
            VanillaOptionPricingFailureStage::FourierPricing
        }
        VanillaOptionPricingRunError::FourierPricingFailure { .. } => {
            VanillaOptionPricingFailureStage::FourierPricing
        }
        VanillaOptionPricingRunError::InvalidRiskNeutralCharacteristicFunction { .. } => {
            VanillaOptionPricingFailureStage::RiskNeutralCharacteristicFunction
        }
        VanillaOptionPricingRunError::EndogenousAnchorMaterializationFailure { .. } => {
            VanillaOptionPricingFailureStage::EndogenousAnchorMaterialization
        }
        VanillaOptionPricingRunError::ExpectedHawkesStateUpdateFailure { .. } => {
            VanillaOptionPricingFailureStage::ExpectedHawkesStateUpdate
        }
        VanillaOptionPricingRunError::AnchoredLocalTiltSolveFailure { .. } => {
            VanillaOptionPricingFailureStage::AnchoredLocalTiltSolve
        }
        VanillaOptionPricingRunError::InvalidObservedBoundaryStateDuringRecursion { .. } => {
            VanillaOptionPricingFailureStage::SpectralObservedBoundaryState
        }
        VanillaOptionPricingRunError::InvalidSpectralDiagnostics { .. } => {
            VanillaOptionPricingFailureStage::SpectralDiagnostics
        }
        VanillaOptionPricingRunError::InvalidGridInterpolationShape { .. } => {
            VanillaOptionPricingFailureStage::SpectralOperatorAssembly
        }
        VanillaOptionPricingRunError::InvalidResponseDiagnostics { .. } => {
            VanillaOptionPricingFailureStage::ResponseDiagnostics
        }
        VanillaOptionPricingRunError::InvalidResponse { .. } => {
            VanillaOptionPricingFailureStage::ResponseAssembly
        }
        VanillaOptionPricingRunError::MissingRepresentativeTransformDiagnostics
        | VanillaOptionPricingRunError::InvalidRunResult { .. } => {
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

pub(crate) fn evaluate_pricing_log_return_characteristic_function_for_context(
    transition_context: &LocalTransitionRequestContext,
    driver_parameters: NigBuySellDriverParameters,
    argument: ComplexScalar,
) -> Result<PricingCharacteristicFunctionEvaluation, VanillaOptionPricingRunError> {
    let buy_side_characteristic_exponent =
        driver_parameters.buy_side_characteristic_exponent(argument);

    let sell_side_characteristic_exponent =
        driver_parameters.sell_side_characteristic_exponent_for_return_composition(argument);

    let transform_input = transition_context.build_transform_input(
        buy_side_characteristic_exponent.scale(-1.0),
        sell_side_characteristic_exponent.scale(-1.0),
        ComplexScalar::new(1.0, 0.0),
        ComplexScalar::new(1.0, 0.0),
    );

    let transform_diagnostics =
        solve_exponential_affine_transform_with_internal_selection(transform_input).map_err(
            |source| VanillaOptionPricingRunError::TransformSolveFailure {
                argument,
                source: Box::new(source),
            },
        )?;

    let transform_output = transform_diagnostics.output.ok_or(
        VanillaOptionPricingRunError::MissingTransformOutput {
            argument,
            status: transform_diagnostics.status,
            context: format!(
                "pricing_log_return_characteristic_function interval_index={} interval_delta={} u_plus={:?} u_minus={:?} z_plus={:?} z_minus={:?} guard_failure_code={:?}",
                transition_context.interval_index,
                transition_context.interval_delta,
                buy_side_characteristic_exponent.scale(-1.0),
                sell_side_characteristic_exponent.scale(-1.0),
                ComplexScalar::new(1.0, 0.0),
                ComplexScalar::new(1.0, 0.0),
                transform_diagnostics
                    .metadata
                    .guard_failure
                    .map(|guard_failure| guard_failure.code),
            ),
        },
    )?;

    let assembled = compose_total_return_characteristic_function_from_oracle_output(
        transform_output,
        ComplexScalar::new(1.0, 0.0),
        ComplexScalar::new(1.0, 0.0),
    )
    .map_err(
        |source| VanillaOptionPricingRunError::CharacteristicFunctionAssemblyFailure {
            argument,
            source: Box::new(source),
        },
    )?;

    Ok(PricingCharacteristicFunctionEvaluation {
        value: assembled.combined_characteristic_factor,
        transform_diagnostics,
    })
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PricingCharacteristicFunctionSharedPlusShiftSensitivity {
    pub(crate) value: ComplexScalar,
    pub(crate) alpha_derivative: ComplexScalar,
    pub(crate) beta_0_derivative: ComplexScalar,
    pub(crate) delta_derivative: ComplexScalar,
    pub(crate) mu_derivative: ComplexScalar,
    pub(crate) delta_beta_derivative: ComplexScalar,
    pub(crate) theta_derivative: ComplexScalar,
}

fn solve_affine_transform_value_input_sensitivities_for_context(
    transition_context: &LocalTransitionRequestContext,
    buy_side_shift: ComplexScalar,
    sell_side_shift: ComplexScalar,
) -> Result<
    janiform_affine_oracle::affine_oracle::ExponentialAffineTransformInputSensitivityOutput,
    VanillaOptionPricingRunError,
> {
    let transform_input = transition_context.build_transform_input(
        buy_side_shift,
        sell_side_shift,
        ComplexScalar::new(1.0, 0.0),
        ComplexScalar::new(1.0, 0.0),
    );

    let transform_diagnostics =
        solve_exponential_affine_transform_with_internal_selection(transform_input).map_err(
            |source| VanillaOptionPricingRunError::TransformSolveFailure {
                argument: ComplexScalar::new(0.0, 0.0),
                source: Box::new(source),
            },
        )?;

    let solver_settings = ExponentialAffineTransformSolverSettings::try_new(
        transform_diagnostics.metadata.integrator_method,
        transform_diagnostics.metadata.configured_step_count,
    )
    .map_err(|source| VanillaOptionPricingRunError::InternalNumericsFailure {
        message: format!(
            "failed to reconstruct affine transform solver settings for sensitivity solve: {source}"
        ),
    })?;

    solve_exponential_affine_transform_input_sensitivities_with_settings(
        transform_input,
        solver_settings,
    )
    .map_err(
        |source| VanillaOptionPricingRunError::InternalNumericsFailure {
            message: format!("affine transform input sensitivity solve failed: {source}"),
        },
    )
}

fn validate_finite_characteristic_sensitivity(
    label: &'static str,
    value: ComplexScalar,
) -> Result<(), VanillaOptionPricingRunError> {
    if !value.is_finite() {
        return Err(VanillaOptionPricingRunError::InternalNumericsFailure {
            message: format!(
                "non-finite pricing characteristic-function sensitivity {label}: {value:?}"
            ),
        });
    }

    Ok(())
}

pub(crate) fn pricing_ratio_derivative(
    numerator: ComplexScalar,
    numerator_derivative: ComplexScalar,
    denominator: ComplexScalar,
    denominator_derivative: ComplexScalar,
) -> Result<ComplexScalar, VanillaOptionPricingRunError> {
    let ratio = numerator.divide(denominator).ok_or(
        VanillaOptionPricingRunError::InvalidRiskNeutralCharacteristicFunction {
            numerator,
            denominator,
        },
    )?;

    let adjusted_numerator =
        numerator_derivative.subtract(complex_multiply(ratio, denominator_derivative));

    adjusted_numerator.divide(denominator).ok_or(
        VanillaOptionPricingRunError::InvalidRiskNeutralCharacteristicFunction {
            numerator: adjusted_numerator,
            denominator,
        },
    )
}

fn log_characteristic_modulus_derivative(
    characteristic_value: ComplexScalar,
    characteristic_derivative: ComplexScalar,
) -> Result<f64, VanillaOptionPricingRunError> {
    let logarithmic_derivative = characteristic_derivative
        .divide(characteristic_value)
        .ok_or(
            VanillaOptionPricingRunError::InvalidRiskNeutralCharacteristicFunction {
                numerator: characteristic_derivative,
                denominator: characteristic_value,
            },
        )?;
    if !logarithmic_derivative.is_finite() {
        return Err(VanillaOptionPricingRunError::InternalNumericsFailure {
            message: format!(
                "non-finite logarithmic derivative for characteristic modulus: {:?}",
                logarithmic_derivative
            ),
        });
    }

    Ok(logarithmic_derivative.real)
}

pub(crate) fn evaluate_pricing_log_return_characteristic_function_shared_plus_shift_sensitivity_for_context(
    transition_context: &LocalTransitionRequestContext,
    driver_parameters: NigBuySellDriverParameters,
    shifted_argument: ComplexScalar,
) -> Result<PricingCharacteristicFunctionSharedPlusShiftSensitivity, VanillaOptionPricingRunError> {
    let buy_side_sensitivity = driver_parameters
        .differentiate_buy_side_characteristic_exponent_for_shared_plus_shift_configuration_with_negative_imaginary_shift(
            shifted_argument,
        )
        .map_err(|source| VanillaOptionPricingRunError::InternalNumericsFailure {
            message: format!(
                "buy-side characteristic-exponent sensitivity failed for shifted_argument={shifted_argument:?}: {source}"
            ),
        })?;

    let sell_side_sensitivity = driver_parameters
        .differentiate_sell_side_characteristic_exponent_for_return_composition_for_shared_plus_shift_configuration_with_negative_imaginary_shift(
            shifted_argument,
        )
        .map_err(|source| VanillaOptionPricingRunError::InternalNumericsFailure {
            message: format!(
                "sell-side characteristic-exponent sensitivity failed for shifted_argument={shifted_argument:?}: {source}"
            ),
        })?;

    let buy_side_shift = buy_side_sensitivity.exponent.scale(-1.0);
    let sell_side_shift = sell_side_sensitivity.exponent.scale(-1.0);

    let transform_sensitivities = solve_affine_transform_value_input_sensitivities_for_context(
        transition_context,
        buy_side_shift,
        sell_side_shift,
    )?;

    let alpha_derivative = complex_multiply(
        transform_sensitivities
            .u_plus_sensitivity
            .transform_value_derivative,
        buy_side_sensitivity.alpha_derivative.scale(-1.0),
    )
    .plus(complex_multiply(
        transform_sensitivities
            .u_minus_sensitivity
            .transform_value_derivative,
        sell_side_sensitivity.alpha_derivative.scale(-1.0),
    ));

    let beta_0_derivative = complex_multiply(
        transform_sensitivities
            .u_plus_sensitivity
            .transform_value_derivative,
        buy_side_sensitivity.beta_0_derivative.scale(-1.0),
    )
    .plus(complex_multiply(
        transform_sensitivities
            .u_minus_sensitivity
            .transform_value_derivative,
        sell_side_sensitivity.beta_0_derivative.scale(-1.0),
    ));

    let delta_derivative = complex_multiply(
        transform_sensitivities
            .u_plus_sensitivity
            .transform_value_derivative,
        buy_side_sensitivity.delta_derivative.scale(-1.0),
    )
    .plus(complex_multiply(
        transform_sensitivities
            .u_minus_sensitivity
            .transform_value_derivative,
        sell_side_sensitivity.delta_derivative.scale(-1.0),
    ));

    let mu_derivative = complex_multiply(
        transform_sensitivities
            .u_plus_sensitivity
            .transform_value_derivative,
        buy_side_sensitivity.mu_derivative.scale(-1.0),
    )
    .plus(complex_multiply(
        transform_sensitivities
            .u_minus_sensitivity
            .transform_value_derivative,
        sell_side_sensitivity.mu_derivative.scale(-1.0),
    ));

    let delta_beta_derivative = complex_multiply(
        transform_sensitivities
            .u_plus_sensitivity
            .transform_value_derivative,
        buy_side_sensitivity.delta_beta_derivative.scale(-1.0),
    )
    .plus(complex_multiply(
        transform_sensitivities
            .u_minus_sensitivity
            .transform_value_derivative,
        sell_side_sensitivity.delta_beta_derivative.scale(-1.0),
    ));

    let theta_derivative = complex_multiply(
        transform_sensitivities
            .u_plus_sensitivity
            .transform_value_derivative,
        buy_side_sensitivity.theta_shift_derivative.scale(-1.0),
    )
    .plus(complex_multiply(
        transform_sensitivities
            .u_minus_sensitivity
            .transform_value_derivative,
        sell_side_sensitivity.theta_shift_derivative.scale(-1.0),
    ));

    validate_finite_characteristic_sensitivity("alpha", alpha_derivative)?;
    validate_finite_characteristic_sensitivity("beta_0", beta_0_derivative)?;
    validate_finite_characteristic_sensitivity("delta", delta_derivative)?;
    validate_finite_characteristic_sensitivity("mu", mu_derivative)?;
    validate_finite_characteristic_sensitivity("delta_beta", delta_beta_derivative)?;
    validate_finite_characteristic_sensitivity("theta", theta_derivative)?;

    Ok(PricingCharacteristicFunctionSharedPlusShiftSensitivity {
        value: transform_sensitivities.output.transform_value,
        alpha_derivative,
        beta_0_derivative,
        delta_derivative,
        mu_derivative,
        delta_beta_derivative,
        theta_derivative,
    })
}

pub(crate) fn evaluate_pricing_log_return_characteristic_function_batch_shared_plus_shift_sensitivities_for_context(
    transition_context: &LocalTransitionRequestContext,
    driver_parameters: NigBuySellDriverParameters,
    shifted_arguments: &[ComplexScalar],
) -> Result<
    Vec<PricingCharacteristicFunctionSharedPlusShiftSensitivity>,
    VanillaOptionPricingRunError,
> {
    let mut sensitivities = Vec::with_capacity(shifted_arguments.len());

    for shifted_argument in shifted_arguments {
        sensitivities.push(
            evaluate_pricing_log_return_characteristic_function_shared_plus_shift_sensitivity_for_context(
                transition_context,
                driver_parameters,
                *shifted_argument,
            )?,
        );
    }

    Ok(sensitivities)
}

pub(crate) fn evaluate_pricing_log_return_characteristic_function_batch_for_context(
    transition_context: &LocalTransitionRequestContext,
    driver_parameters: NigBuySellDriverParameters,
    arguments: &[ComplexScalar],
) -> Result<Vec<PricingCharacteristicFunctionEvaluation>, VanillaOptionPricingRunError> {
    let mut evaluations = Vec::with_capacity(arguments.len());

    for argument in arguments {
        evaluations.push(
            evaluate_pricing_log_return_characteristic_function_for_context(
                transition_context,
                driver_parameters,
                *argument,
            )?,
        );
    }

    Ok(evaluations)
}

#[cfg(test)]
pub(crate) fn evaluate_pricing_log_return_characteristic_function(
    pricing_request: &VanillaOptionPricingRequest,
    driver_parameters: NigBuySellDriverParameters,
    interval_factors: VanillaOptionPricingIntervalFactors,
    _news_intensity: ExponentialAffineTransformNewsIntensityInput,
    argument: ComplexScalar,
) -> Result<PricingCharacteristicFunctionEvaluation, VanillaOptionPricingRunError> {
    let transition_context =
        LocalTransitionRequestContext::from_pricing_request(pricing_request, interval_factors)?;

    evaluate_pricing_log_return_characteristic_function_for_context(
        &transition_context,
        driver_parameters,
        argument,
    )
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
    TransformInputBuildFailure {
        argument: ComplexScalar,
        source: Box<PricingTransformInputBuildError>,
    },
    TransformSolveFailure {
        argument: ComplexScalar,
        source: Box<ExponentialAffineTransformSolverError>,
    },
    MissingTransformOutput {
        argument: ComplexScalar,
        status: ExponentialAffineTransformIntegrationStatus,
        context: String,
    },
    CharacteristicFunctionAssemblyFailure {
        argument: ComplexScalar,
        source: Box<super::characteristic::ReturnCharacteristicFunctionAssemblyError>,
    },
    FourierPricingFailure {
        source: Box<FourierInversionPricingError>,
    },
    InvalidRiskNeutralCharacteristicFunction {
        numerator: ComplexScalar,
        denominator: ComplexScalar,
    },
    EndogenousAnchorMaterializationFailure {
        message: String,
    },
    ExpectedHawkesStateUpdateFailure {
        message: String,
    },
    AnchoredLocalTiltSolveFailure {
        interval_index: usize,
        deviation: f64,
        source: Box<super::characteristic::AnchoredLocalTiltSolveError>,
    },
    InvalidObservedBoundaryStateDuringRecursion {
        interval_index: usize,
        deviation: f64,
        message: String,
    },
    InvalidSpectralDiagnostics {
        source: Box<super::contracts::SpectralPricingDiagnosticsError>,
    },
    InvalidGridInterpolationShape {
        expected_node_count: usize,
        found_value_count: usize,
    },
    InvalidResponseDiagnostics {
        source: Box<super::contracts::VanillaOptionPricingDiagnosticsError>,
    },
    InvalidResponse {
        source: Box<VanillaOptionPricingResponseError>,
    },
    MissingRepresentativeTransformDiagnostics,
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
            Self::TransformInputBuildFailure { argument, source } => write!(
                formatter,
                "failed to build transform input for characteristic-function argument {argument:?}: {source}"
            ),
            Self::TransformSolveFailure { argument, source } => write!(
                formatter,
                "failed to solve affine transform for characteristic-function argument {argument:?}: {source}"
            ),
            Self::MissingTransformOutput {
                argument,
                status,
                context,
            } => write!(
                formatter,
                "affine transform returned no output for characteristic-function argument {argument:?} with status {status:?}: {context}"
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
            Self::EndogenousAnchorMaterializationFailure { message } => write!(
                formatter,
                "endogenous-anchor pricing materialization failed: {message}"
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
            Self::InvalidSpectralDiagnostics { source } => {
                write!(formatter, "spectral diagnostics are invalid: {source}")
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
            Self::InvalidExecutionInput { source } => Some(source.as_ref()),
            Self::InvalidDriverParameters { source } => Some(source.as_ref()),
            Self::TransformInputBuildFailure { source, .. } => Some(source.as_ref()),
            Self::TransformSolveFailure { source, .. } => Some(source.as_ref()),
            Self::CharacteristicFunctionAssemblyFailure { source, .. } => Some(source.as_ref()),
            Self::FourierPricingFailure { source } => Some(source.as_ref()),
            Self::AnchoredLocalTiltSolveFailure { source, .. } => Some(source.as_ref()),
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
