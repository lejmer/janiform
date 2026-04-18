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
};
use janiform_affine_oracle::endogenous_anchor::{
    EndogenousAnchorIntervalFactors, derive_expected_interval_statistics_from_first_moment_dynamics,
};
use janiform_affine_oracle::sufficient_statistics::{
    SufficientStatisticsExpectedIntervalStatistics, SufficientStatisticsTransformSliceContext,
    SufficientStatisticsTransformSliceSettings, SufficientStatisticsTransformSlices,
    derive_expected_interval_statistics_from_transform_slices,
};
use janiform_foundations::complex_math::ComplexScalar;
use janiform_foundations::matrix2::solve_two_by_two_linear_source_system;
use janiform_model_kernels::hawkes::HawkesExcitationState;
use janiform_model_kernels::mean_reversion::{
    MeanReversionBoundaryState, MeanReversionRuntimeIntervalAnchors,
    evolve_mean_reversion_boundary_state_with_endogenous_pressure,
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
    VanillaOptionPricingRunResultBuildError, VanillaOptionPricingStatus,
    build_vanilla_option_pricing_run_result,
};
use super::fourier::{
    FourierInversionDiagnostics, FourierInversionNumericQualityDiagnostics,
    FourierInversionNumericQualityGate, FourierInversionPricingError,
    FourierInversionQuadratureRule,
};
use super::interval_schedule::PricingIntervalSchedule;
use super::news_loading_projection::project_nonnegative_exogenous_news_loading;
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
    pub(crate) expected_news_loading: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CorePureImaginaryLogMomentEvaluation {
    pub(crate) observed_log_moment_exponent: f64,
    pub(crate) transform_diagnostics: ExponentialAffineTransformIntegrationResult,
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
                source: VanillaOptionPricingExecutionInputError::InvalidIntervalFactors { source },
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
                    source,
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
        let observed_log_moment_exponent = core_evaluation.observed_log_moment_exponent
            + transition_row.local_martingale_deviation_adjustment;

        Ok(LocalTiltResidualEvaluation {
            observed_log_moment_exponent,
            residual: observed_log_moment_exponent - transition_row.target_log_moment_exponent,
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
    execution_input
        .validate()
        .map_err(|source| VanillaOptionPricingRunError::InvalidExecutionInput { source })?;

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
                        source: VanillaOptionPricingExecutionInputError::InvalidRequest {
                            source: VanillaOptionPricingRequestError::InvalidMarketInputs {
                                source,
                            },
                        },
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
                        VanillaOptionPricingRunError::InvalidResponseDiagnostics { source }
                    })?,
                ),
                spectral_outcome.implied_volatility,
                Some(spectral_outcome.implied_volatility_inversion_status),
            )
            .map_err(|source| VanillaOptionPricingRunError::InvalidResponse { source })?;

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

    build_vanilla_option_pricing_run_result(
        response,
        output_selection,
        Some(market_inputs_for_calibration_hooks),
        representative_transform_diagnostics,
        integration_quality_diagnostics,
        spectral_diagnostics,
        execution_input.runtime_interval_schedule.clone(),
        vec![execution_input.runtime_interval_schedule.interval_count() - 1],
    )
    .map_err(|source| VanillaOptionPricingRunError::InvalidRunResult { source })
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

fn core_target_log_moment_exponent_for_transition_row(
    transition_row: &LocalTransitionRowState,
) -> f64 {
    transition_row.target_log_moment_exponent - transition_row.local_martingale_deviation_adjustment
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
                        source,
                    },
                );
            }
        }
    }

    let core_target_log_moment_exponent =
        core_target_log_moment_exponent_for_transition_row(transition_row);
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
                source,
            },
        )?;
    interval_tilt_curve.last_solved_theta = Some(theta);

    let evaluation = LocalTiltResidualEvaluation {
        observed_log_moment_exponent: core_evaluation.observed_log_moment_exponent
            + transition_row.local_martingale_deviation_adjustment,
        residual: core_evaluation.residual,
    };
    let diagnostics = build_local_tilt_diagnostics_for_row(
        interval_tilt_curve.transition_context,
        transition_row,
        theta,
        evaluation,
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
                source,
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
                        source,
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

pub(crate) fn build_pricing_risk_neutral_anchor_materialization(
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
    let tilted_statistics = derive_local_tilted_expected_interval_statistics_for_row(
        transition_context,
        transition_row,
        driver_parameters,
        theta,
        pricing_thread_pool,
        cpu_count,
    )?;

    let core_expected_increment = tilted_core_expected_increment_from_statistics(
        driver_parameters,
        tilted_statistics,
        theta,
    )?;
    let expected_news_loading = infer_expected_news_loading_under_local_measure_for_row(
        transition_context,
        transition_row,
        tilted_statistics,
    )?;

    Ok(LocalTiltedIntervalExpectations {
        expected_statistics: tilted_statistics,
        core_expected_increment,
        expected_news_loading,
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

    derive_expected_interval_statistics_from_first_moment_dynamics(
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
        .add(extra_u_plus);
    let sell_side_shift = driver_parameters
        .sell_side_characteristic_exponent_for_return_composition(shifted_argument)
        .scale(-1.0)
        .add(extra_u_minus);

    let transform_input =
        transition_context.build_transform_input(buy_side_shift, sell_side_shift, z_plus, z_minus);

    let transform_diagnostics =
        solve_exponential_affine_transform_with_internal_selection(transform_input).map_err(
            |source| VanillaOptionPricingRunError::TransformSolveFailure {
                argument: shifted_argument,
                source,
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

fn tilted_core_expected_increment_from_statistics(
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
        theta,
    )?;

    Ok(tilted_buy_expected_increment - tilted_sell_expected_increment)
}

fn tilted_nig_expected_increment(
    physical_parameters: janiform_model_kernels::nig::NigParameters,
    business_time_increment_scale: f64,
    theta: f64,
) -> Result<f64, VanillaOptionPricingRunError> {
    janiform_model_kernels::nig::NigParameters::try_new(
        physical_parameters.alpha,
        physical_parameters.beta - theta,
        physical_parameters.delta,
        physical_parameters.mu,
    )
    .map_err(
        |source| VanillaOptionPricingRunError::EndogenousAnchorMaterializationFailure {
            message: source.to_string(),
        },
    )?
    .expected_increment(business_time_increment_scale)
    .map_err(
        |source| VanillaOptionPricingRunError::EndogenousAnchorMaterializationFailure {
            message: source.to_string(),
        },
    )
}

pub(crate) fn infer_expected_news_loading_under_local_measure_for_row(
    transition_context: &LocalTransitionRequestContext,
    transition_row: &LocalTransitionRowState,
    expected_statistics: SufficientStatisticsExpectedIntervalStatistics,
) -> Result<f64, VanillaOptionPricingRunError> {
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

    if transition_row.uses_nested_transition_measure() {
        let expected_severity_mark = transition_context
            .model_parameters
            .news_process_configuration
            .shared_mark_law
            .gamma_severity_marks
            .expected_severity_mark()
            .map_err(|source| {
                VanillaOptionPricingRunError::EndogenousAnchorMaterializationFailure {
                    message: source.to_string(),
                }
            })?;

        return Ok(expected_scaled_news_loading(
            &transition_context.model_parameters,
            endogenous_factors,
            expected_severity_mark,
        ));
    }

    if transition_context
        .model_parameters
        .news_process_configuration
        .known_component_mark_scales
        .activity_coupling_scale
        == 0.0
        && transition_context
            .model_parameters
            .news_process_configuration
            .surprise_component_mark_scales
            .activity_coupling_scale
            == 0.0
    {
        return Ok(0.0);
    }

    if transition_context.interval_factors.s_h == 0.0 {
        let expected_severity_mark = transition_context
            .model_parameters
            .news_process_configuration
            .shared_mark_law
            .gamma_severity_marks
            .expected_severity_mark()
            .map_err(|source| {
                VanillaOptionPricingRunError::EndogenousAnchorMaterializationFailure {
                    message: source.to_string(),
                }
            })?;

        return Ok(expected_scaled_news_loading(
            &transition_context.model_parameters,
            endogenous_factors,
            expected_severity_mark,
        ));
    }

    let kernel_parameters = transition_context.model_parameters.hawkes_kernel_parameters;

    let target_plus_before = transition_context.hawkes_boundary_state.y_plus_plus
        + transition_context.hawkes_boundary_state.y_plus_minus;
    let target_minus_before = transition_context.hawkes_boundary_state.y_minus_plus
        + transition_context.hawkes_boundary_state.y_minus_minus;

    let baseline_expected_business_time =
        kernel_parameters.lambda_bar * endogenous_factors.s_h * endogenous_factors.delta;

    let observed_integrated_target_plus = (expected_statistics.expected_a_plus
        - baseline_expected_business_time)
        / endogenous_factors.s_h;
    let observed_integrated_target_minus = (expected_statistics.expected_a_minus
        - baseline_expected_business_time)
        / endogenous_factors.s_h;

    let matrix_a =
        -kernel_parameters.beta + endogenous_factors.s_h * kernel_parameters.alpha.plus_plus;
    let matrix_b = endogenous_factors.s_h * kernel_parameters.alpha.plus_minus;
    let matrix_c = endogenous_factors.s_h * kernel_parameters.alpha.minus_plus;
    let matrix_d =
        -kernel_parameters.beta + endogenous_factors.s_h * kernel_parameters.alpha.minus_minus;

    let base_source_plus = endogenous_factors.s_h
        * kernel_parameters.lambda_bar
        * (kernel_parameters.alpha.plus_plus + kernel_parameters.alpha.plus_minus);
    let base_source_minus = endogenous_factors.s_h
        * kernel_parameters.lambda_bar
        * (kernel_parameters.alpha.minus_plus + kernel_parameters.alpha.minus_minus);

    let base_solution = solve_two_by_two_linear_source_system(
        matrix_a,
        matrix_b,
        matrix_c,
        matrix_d,
        target_plus_before,
        target_minus_before,
        base_source_plus,
        base_source_minus,
        endogenous_factors.delta,
    );
    let unit_loading_solution = solve_two_by_two_linear_source_system(
        matrix_a,
        matrix_b,
        matrix_c,
        matrix_d,
        0.0,
        0.0,
        kernel_parameters.rho.plus_plus + kernel_parameters.rho.plus_minus,
        kernel_parameters.rho.minus_plus + kernel_parameters.rho.minus_minus,
        endogenous_factors.delta,
    );

    project_nonnegative_exogenous_news_loading(
        observed_integrated_target_plus,
        observed_integrated_target_minus,
        base_solution.integrated_first_component,
        base_solution.integrated_second_component,
        unit_loading_solution.integrated_first_component,
        unit_loading_solution.integrated_second_component,
    )
    .map_err(
        |source| VanillaOptionPricingRunError::ExpectedHawkesStateUpdateFailure {
            message: source.to_string(),
        },
    )
}

fn expected_scaled_news_loading(
    model_parameters: &VanillaOptionPricingModelParameters,
    interval_factors: EndogenousAnchorIntervalFactors,
    expected_severity_mark: f64,
) -> f64 {
    let known_loading = interval_factors.expected_known_arrival_count()
        * model_parameters
            .news_process_configuration
            .known_component_mark_scales
            .activity_coupling_scale
        * expected_severity_mark;
    let surprise_loading = interval_factors.expected_surprise_arrival_count()
        * model_parameters
            .news_process_configuration
            .surprise_component_mark_scales
            .activity_coupling_scale
        * expected_severity_mark;

    known_loading + surprise_loading
}

fn evolve_expected_hawkes_state_under_local_measure(
    pricing_request: &VanillaOptionPricingRequest,
    interval_factors: VanillaOptionPricingIntervalFactors,
    state_before_update: HawkesExcitationState,
    local_expectations: LocalTiltedIntervalExpectations,
) -> Result<HawkesExcitationState, VanillaOptionPricingRunError> {
    let kernel_parameters = pricing_request.model_parameters.hawkes_kernel_parameters;
    let drifted_state = state_before_update
        .apply_shared_decay_drift(kernel_parameters.beta, pricing_request.interval_delta);

    let post_buy_state = drifted_state.apply_expected_endogenous_event_jump(
        janiform_model_kernels::hawkes::HawkesEventSide::Buy,
        kernel_parameters.alpha,
        local_expectations.expected_statistics.expected_n_plus,
    );
    let post_sell_state = post_buy_state.apply_expected_endogenous_event_jump(
        janiform_model_kernels::hawkes::HawkesEventSide::Sell,
        kernel_parameters.alpha,
        local_expectations.expected_statistics.expected_n_minus,
    );

    if !local_expectations.expected_news_loading.is_finite()
        || local_expectations.expected_news_loading < 0.0
    {
        return Err(
            VanillaOptionPricingRunError::EndogenousAnchorMaterializationFailure {
                message: format!(
                    "local-measure expected news loading must be finite and non-negative at interval_index={} interval_delta={} s_h={} nu_h_known={} nu_h_surprise={} expected_a_plus={} expected_a_minus={} expected_n_plus={} expected_n_minus={} expected_news_loading={}",
                    pricing_request.boundary_input.interval_index,
                    pricing_request.interval_delta,
                    interval_factors.s_h,
                    interval_factors.nu_h_known,
                    interval_factors.nu_h_surprise,
                    local_expectations.expected_statistics.expected_a_plus,
                    local_expectations.expected_statistics.expected_a_minus,
                    local_expectations.expected_statistics.expected_n_plus,
                    local_expectations.expected_statistics.expected_n_minus,
                    local_expectations.expected_news_loading,
                ),
            },
        );
    }

    post_sell_state
        .apply_exogenous_news_loading_jump(
            kernel_parameters.rho,
            local_expectations.expected_news_loading,
        )
        .map_err(|source| {
            VanillaOptionPricingRunError::EndogenousAnchorMaterializationFailure {
                message: format!(
                    "{source}; interval_index={} interval_delta={} s_h={} nu_h_known={} nu_h_surprise={} expected_a_plus={} expected_a_minus={} expected_n_plus={} expected_n_minus={} expected_news_loading={}",
                    pricing_request.boundary_input.interval_index,
                    pricing_request.interval_delta,
                    interval_factors.s_h,
                    interval_factors.nu_h_known,
                    interval_factors.nu_h_surprise,
                    local_expectations.expected_statistics.expected_a_plus,
                    local_expectations.expected_statistics.expected_a_minus,
                    local_expectations.expected_statistics.expected_n_plus,
                    local_expectations.expected_statistics.expected_n_minus,
                    local_expectations.expected_news_loading,
                ),
            }
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
                        source: VanillaOptionPricingExecutionInputError::InvalidRequest {
                            source: VanillaOptionPricingRequestError::InvalidMarketInputs {
                                source,
                            },
                        },
                    },
                )?,
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
        pricing_request.model_parameters.clone(),
        interval_delta,
    )
    .map_err(
        |source| VanillaOptionPricingRunError::InvalidExecutionInput {
            source: VanillaOptionPricingExecutionInputError::InvalidRequest { source },
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
            .map_err(|source| VanillaOptionPricingRunError::InvalidResponse { source })
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

    let transform_diagnostics = solve_exponential_affine_transform_with_internal_selection(
        transform_input,
    )
    .map_err(|source| VanillaOptionPricingRunError::TransformSolveFailure { argument, source })?;

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
        source: VanillaOptionPricingExecutionInputError,
    },
    InternalNumericsFailure {
        message: String,
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
        context: String,
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
    EndogenousAnchorMaterializationFailure {
        message: String,
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
    InvalidSpectralDiagnostics {
        source: super::contracts::SpectralPricingDiagnosticsError,
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
            Self::InvalidExecutionInput { source } => Some(source),
            Self::InvalidDriverParameters { source } => Some(source),
            Self::TransformInputBuildFailure { source, .. } => Some(source),
            Self::TransformSolveFailure { source, .. } => Some(source),
            Self::CharacteristicFunctionAssemblyFailure { source, .. } => Some(source),
            Self::FourierPricingFailure { source } => Some(source),
            Self::AnchoredLocalTiltSolveFailure { source, .. } => Some(source),
            Self::InvalidSpectralDiagnostics { source } => Some(source),
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
