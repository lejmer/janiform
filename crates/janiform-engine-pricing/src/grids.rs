use std::error::Error;
use std::fmt::{Display, Formatter};

use rayon::iter::{IndexedParallelIterator, IntoParallelRefIterator, ParallelIterator};
use serde::{Deserialize, Serialize};

use janiform_affine_oracle::affine_oracle::ExponentialAffineTransformIntegrationResult;
use janiform_model_kernels::nig::{
    NigBuySellDriverParameters, NigSharedPlusShiftConfigurationError,
};

use super::contracts::{
    AnchoredPricingRecursionIntervalDiagnostics, AnchoredVanillaOptionPricingBoundaryInput,
    VanillaOptionPayoffConfiguration, VanillaOptionPayoffType,
    VanillaOptionPricingFailureDiagnostic, VanillaOptionPricingFailureStage,
    VanillaOptionPricingMarketInputs, VanillaOptionPricingModelParameters,
    VanillaOptionPricingOptionalOutputSelection, VanillaOptionPricingRequest,
    VanillaOptionPricingRequestError, VanillaOptionPricingResponse, VanillaOptionPricingStatus,
};
use super::forward_law::{
    ForwardLawScopeSolution, ForwardLawTerminalSnapshot, ForwardPricedStrike,
    price_terminal_snapshot_strikes, solve_forward_law_scope,
};
use super::fourier::FourierInversionDiagnostics;
use super::implied_volatility::VanillaOptionImpliedVolatilityInversionStatus;
use super::interval_schedule::PricingIntervalSchedule;
use super::progress::PricingProgressEvent;
use super::progress_runtime::PricingProgressReporter;
use super::runner::{
    VanillaOptionPricingExecutionInput, VanillaOptionPricingIntervalFactors,
    VanillaOptionPricingRunError, pricing_failure_diagnostic_from_run_error,
    run_vanilla_option_pricing_with_observer,
};
use super::thread_pool::{
    build_pricing_thread_pool, install_pricing_parallelism, resolved_pricing_cpu_count,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VanillaOptionMaturityPricingEngineKind {
    DirectFourier,
    ForwardLaw,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VanillaOptionMaturityIntervalSummary {
    pub interval_index: usize,
    pub current_anchor_value: f64,
    pub next_anchor_value: f64,
    pub interval_delta: f64,
    pub expected_hawkes_boundary_state_before:
        janiform_model_kernels::hawkes::HawkesExcitationState,
    pub expected_hawkes_boundary_state_after: janiform_model_kernels::hawkes::HawkesExcitationState,
    pub maximum_tail_probability_mass: f64,
    pub maximum_row_probability_mass_error: f64,
    pub minimum_solved_theta: f64,
    pub maximum_solved_theta: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VanillaOptionMaturitySummary {
    pub maturity_interval: f64,
    pub engine_kind: VanillaOptionMaturityPricingEngineKind,
    pub terminal_anchor_value: Option<f64>,
    pub deviation_grid_lower_bound: Option<f64>,
    pub deviation_grid_upper_bound: Option<f64>,
    pub deviation_grid_node_count: Option<usize>,
    pub terminal_deviation_nodes: Option<Vec<f64>>,
    pub terminal_probability_mass: Option<Vec<f64>>,
    pub interval_summaries: Vec<VanillaOptionMaturityIntervalSummary>,
    pub representative_transform: Option<ExponentialAffineTransformIntegrationResult>,
    pub integration_quality: Option<FourierInversionDiagnostics>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VanillaOptionGridRequest {
    pub payoff_type: VanillaOptionPayoffType,
    pub risk_free_rate: f64,
    pub dividend_yield: f64,
    pub strikes: Vec<f64>,
    pub maturity_intervals: Vec<f64>,
}

impl VanillaOptionGridRequest {
    pub fn validate(&self) -> Result<(), VanillaOptionGridBatchPricingRequestError> {
        if self.strikes.is_empty() {
            return Err(VanillaOptionGridBatchPricingRequestError::EmptyStrikeAxis);
        }
        if self.maturity_intervals.is_empty() {
            return Err(VanillaOptionGridBatchPricingRequestError::EmptyMaturityAxis);
        }
        for strike in &self.strikes {
            validate_positive_finite("strikes", *strike)?;
        }
        for maturity_interval in &self.maturity_intervals {
            validate_positive_finite("maturity_intervals", *maturity_interval)?;
        }
        validate_finite("risk_free_rate", self.risk_free_rate)?;
        validate_finite("dividend_yield", self.dividend_yield)?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VanillaOptionGridBatchPricingRequest {
    pub spot: f64,
    pub grids: Vec<VanillaOptionGridRequest>,
}

impl VanillaOptionGridBatchPricingRequest {
    pub fn validate(&self) -> Result<(), VanillaOptionGridBatchPricingRequestError> {
        validate_positive_finite("spot", self.spot)?;
        if self.grids.is_empty() {
            return Err(VanillaOptionGridBatchPricingRequestError::EmptyGrids);
        }
        for grid in &self.grids {
            grid.validate()?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum VanillaOptionGridBatchPricingRequestError {
    EmptyGrids,
    EmptyStrikeAxis,
    EmptyMaturityAxis,
    InvalidFiniteField { field: &'static str, value: f64 },
    InvalidPositiveField { field: &'static str, value: f64 },
}

impl Display for VanillaOptionGridBatchPricingRequestError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyGrids => write!(
                formatter,
                "grid batch pricing request must contain at least one grid"
            ),
            Self::EmptyStrikeAxis => {
                write!(formatter, "pricing grid must contain at least one strike")
            }
            Self::EmptyMaturityAxis => write!(
                formatter,
                "pricing grid must contain at least one maturity_interval"
            ),
            Self::InvalidFiniteField { field, value } => {
                write!(formatter, "{field} values must be finite, found {value}")
            }
            Self::InvalidPositiveField { field, value } => {
                write!(
                    formatter,
                    "{field} values must be finite and strictly positive, found {value}"
                )
            }
        }
    }
}

impl Error for VanillaOptionGridBatchPricingRequestError {}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VanillaOptionGridBatchPricingExecutionInput {
    pub request: VanillaOptionGridBatchPricingRequest,
    pub boundary_input: AnchoredVanillaOptionPricingBoundaryInput,
    pub model_parameters: VanillaOptionPricingModelParameters,
    pub runtime_interval_schedule: PricingIntervalSchedule,
    pub interval_delta: f64,
    pub interval_factors: Vec<VanillaOptionPricingIntervalFactors>,
    pub optional_output_selection: VanillaOptionPricingOptionalOutputSelection,
    pub cpu_count: Option<usize>,
}

impl VanillaOptionGridBatchPricingExecutionInput {
    pub fn validate(&self) -> Result<(), VanillaOptionGridBatchPricingExecutionInputError> {
        self.request.validate().map_err(|source| {
            VanillaOptionGridBatchPricingExecutionInputError::InvalidRequest { source }
        })?;
        self.boundary_input.validate().map_err(|source| {
            VanillaOptionGridBatchPricingExecutionInputError::InvalidBoundaryInput { source }
        })?;
        self.model_parameters.validate().map_err(|source| {
            VanillaOptionGridBatchPricingExecutionInputError::InvalidModelParameters { source }
        })?;
        validate_positive_finite("interval_delta", self.interval_delta).map_err(|source| {
            VanillaOptionGridBatchPricingExecutionInputError::InvalidRequest { source }
        })?;
        let first_runtime_interval_delta = self
            .runtime_interval_schedule
            .interval_delta_hours(0)
            .ok_or(
            VanillaOptionGridBatchPricingExecutionInputError::InvalidRuntimeIntervalSchedule {
                message: "must contain at least one interval".to_string(),
            },
        )?;
        if self.boundary_input.interval_delta != self.interval_delta {
            return Err(
                VanillaOptionGridBatchPricingExecutionInputError::BoundaryIntervalDeltaMismatch {
                    request_interval_delta: self.interval_delta,
                    boundary_interval_delta: self.boundary_input.interval_delta,
                },
            );
        }
        if self.interval_delta != first_runtime_interval_delta {
            return Err(
                VanillaOptionGridBatchPricingExecutionInputError::InvalidRuntimeIntervalSchedule {
                    message: format!(
                        "first runtime interval delta {first_runtime_interval_delta} must equal interval_delta {}",
                        self.interval_delta
                    ),
                },
            );
        }
        if self.interval_factors.is_empty() {
            return Err(VanillaOptionGridBatchPricingExecutionInputError::EmptyIntervalFactors);
        }
        for interval_factor in &self.interval_factors {
            interval_factor.validate().map_err(|source| {
                VanillaOptionGridBatchPricingExecutionInputError::InvalidIntervalFactors { source }
            })?;
        }
        if let Some(cpu_count) = self.cpu_count {
            if cpu_count == 0 {
                return Err(
                    VanillaOptionGridBatchPricingExecutionInputError::InvalidCpuCount {
                        value: cpu_count,
                    },
                );
            }
        }
        for grid in &self.request.grids {
            for maturity_interval in &grid.maturity_intervals {
                self.runtime_interval_schedule
                    .terminal_interval_index_for_maturity_interval(*maturity_interval)
                    .map_err(|source| {
                        VanillaOptionGridBatchPricingExecutionInputError::InvalidRuntimeIntervalSchedule {
                            message: source.to_string(),
                        }
                    })?;
            }
        }

        if self.interval_factors.len() != self.runtime_interval_schedule.interval_count() {
            return Err(
                VanillaOptionGridBatchPricingExecutionInputError::IntervalFactorShapeMismatch {
                    expected_interval_factor_count: self.runtime_interval_schedule.interval_count(),
                    found_interval_factor_count: self.interval_factors.len(),
                },
            );
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum VanillaOptionGridBatchPricingExecutionInputError {
    InvalidRequest {
        source: VanillaOptionGridBatchPricingRequestError,
    },
    InvalidBoundaryInput {
        source: super::contracts::AnchoredVanillaOptionPricingBoundaryInputError,
    },
    InvalidModelParameters {
        source: super::contracts::VanillaOptionPricingModelParametersError,
    },
    InvalidIntervalFactors {
        source: super::runner::VanillaOptionPricingIntervalFactorsError,
    },
    InvalidRuntimeIntervalSchedule {
        message: String,
    },
    BoundaryIntervalDeltaMismatch {
        request_interval_delta: f64,
        boundary_interval_delta: f64,
    },
    EmptyIntervalFactors,
    IntervalFactorShapeMismatch {
        expected_interval_factor_count: usize,
        found_interval_factor_count: usize,
    },
    InvalidCpuCount {
        value: usize,
    },
}

impl Display for VanillaOptionGridBatchPricingExecutionInputError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidRequest { source } => {
                write!(formatter, "grid batch pricing request is invalid: {source}")
            }
            Self::InvalidBoundaryInput { source } => write!(
                formatter,
                "grid batch pricing boundary_input is invalid: {source}"
            ),
            Self::InvalidModelParameters { source } => write!(
                formatter,
                "grid batch pricing model_parameters are invalid: {source}"
            ),
            Self::InvalidIntervalFactors { source } => write!(
                formatter,
                "grid batch pricing interval_factors are invalid: {source}"
            ),
            Self::InvalidRuntimeIntervalSchedule { message } => write!(
                formatter,
                "grid batch pricing runtime_interval_schedule is invalid: {message}"
            ),
            Self::BoundaryIntervalDeltaMismatch {
                request_interval_delta,
                boundary_interval_delta,
            } => write!(
                formatter,
                "grid batch pricing interval_delta {request_interval_delta} must equal boundary_input.interval_delta {boundary_interval_delta}"
            ),
            Self::EmptyIntervalFactors => write!(
                formatter,
                "grid batch pricing interval_factors must contain at least one interval"
            ),
            Self::IntervalFactorShapeMismatch {
                expected_interval_factor_count,
                found_interval_factor_count,
            } => write!(
                formatter,
                "grid batch pricing interval_factors must contain exactly {expected_interval_factor_count} entries implied by the maximum maturity, found {found_interval_factor_count}"
            ),
            Self::InvalidCpuCount { value } => write!(
                formatter,
                "grid batch pricing cpu_count must be strictly positive when provided, found {value}"
            ),
        }
    }
}

impl Error for VanillaOptionGridBatchPricingExecutionInputError {}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VanillaOptionGridPricingResult {
    pub payoff_type: VanillaOptionPayoffType,
    pub risk_free_rate: f64,
    pub dividend_yield: f64,
    pub strikes: Vec<f64>,
    pub maturity_intervals: Vec<f64>,
    pub maturity_terminal_interval_indices: Vec<usize>,
    pub price_grid: Vec<Vec<Option<f64>>>,
    pub implied_volatility_grid: Vec<Vec<Option<f64>>>,
    pub implied_volatility_inversion_status_grid:
        Vec<Vec<Option<VanillaOptionImpliedVolatilityInversionStatus>>>,
    pub pricing_status_grid: Vec<Vec<VanillaOptionPricingStatus>>,
    pub pricing_failure_stage_grid: Vec<Vec<Option<VanillaOptionPricingFailureStage>>>,
    pub pricing_failure_reason_grid: Vec<Vec<Option<String>>>,
    pub forward_price_grid: Vec<Vec<Option<f64>>>,
    pub discount_factor_grid: Vec<Vec<Option<f64>>>,
    pub boundary_state_norm_grid: Vec<Vec<Option<f64>>>,
    pub maturity_summaries: Vec<Option<VanillaOptionMaturitySummary>>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VanillaOptionGridBatchPricingRunResult {
    pub spot: f64,
    pub grids: Vec<VanillaOptionGridPricingResult>,
    pub runtime_interval_schedule: PricingIntervalSchedule,
}

#[derive(Debug, Clone, PartialEq)]
pub enum VanillaOptionGridBatchPricingRunError {
    InvalidExecutionInput {
        source: VanillaOptionGridBatchPricingExecutionInputError,
    },
    InvalidDriverParameters {
        source: NigSharedPlusShiftConfigurationError,
    },
    NodePricingFailure {
        message: String,
    },
}

impl Display for VanillaOptionGridBatchPricingRunError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidExecutionInput { source } => write!(
                formatter,
                "grid batch pricing execution input is invalid: {source}"
            ),
            Self::InvalidDriverParameters { source } => write!(
                formatter,
                "grid batch pricing driver parameters are invalid: {source}"
            ),
            Self::NodePricingFailure { message } => {
                write!(formatter, "grid batch pricing failed: {message}")
            }
        }
    }
}

impl Error for VanillaOptionGridBatchPricingRunError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidExecutionInput { source } => Some(source),
            Self::InvalidDriverParameters { source } => Some(source),
            Self::NodePricingFailure { .. } => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VanillaOptionSurfacePricingRequest {
    pub market_inputs: VanillaOptionPricingMarketInputs,
    pub payoff: VanillaOptionPayoffConfiguration,
}

impl VanillaOptionSurfacePricingRequest {
    pub fn validate(&self) -> Result<(), VanillaOptionGridBatchPricingRequestError> {
        VanillaOptionGridBatchPricingRequest {
            spot: self.market_inputs.spot,
            grids: vec![VanillaOptionGridRequest {
                payoff_type: self.payoff.payoff_type,
                risk_free_rate: self.market_inputs.risk_free_rate,
                dividend_yield: self.market_inputs.dividend_yield,
                strikes: self.market_inputs.strikes.clone(),
                maturity_intervals: self.market_inputs.maturity_intervals.clone(),
            }],
        }
        .validate()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VanillaOptionSurfacePricingExecutionInput {
    pub request: VanillaOptionSurfacePricingRequest,
    pub boundary_input: AnchoredVanillaOptionPricingBoundaryInput,
    pub model_parameters: VanillaOptionPricingModelParameters,
    pub runtime_interval_schedule: PricingIntervalSchedule,
    pub interval_delta: f64,
    pub interval_factors: Vec<VanillaOptionPricingIntervalFactors>,
    pub optional_output_selection: VanillaOptionPricingOptionalOutputSelection,
    pub cpu_count: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VanillaOptionSurfacePricingResponse {
    pub spot: f64,
    pub strikes: Vec<f64>,
    pub maturity_intervals: Vec<f64>,
    pub payoff_type: VanillaOptionPayoffType,
    pub risk_free_rate: f64,
    pub dividend_yield: f64,
    pub price_grid: Vec<Vec<Option<f64>>>,
    pub implied_volatility_grid: Vec<Vec<Option<f64>>>,
    pub implied_volatility_inversion_status_grid:
        Vec<Vec<Option<VanillaOptionImpliedVolatilityInversionStatus>>>,
    pub pricing_status_grid: Vec<Vec<VanillaOptionPricingStatus>>,
    pub pricing_failure_stage_grid: Vec<Vec<Option<VanillaOptionPricingFailureStage>>>,
    pub pricing_failure_reason_grid: Vec<Vec<Option<String>>>,
    pub forward_price_grid: Vec<Vec<Option<f64>>>,
    pub discount_factor_grid: Vec<Vec<Option<f64>>>,
    pub boundary_state_norm_grid: Vec<Vec<Option<f64>>>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VanillaOptionSurfacePricingRunResult {
    pub response: VanillaOptionSurfacePricingResponse,
    pub maturity_summaries: Vec<Option<VanillaOptionMaturitySummary>>,
    pub runtime_interval_schedule: PricingIntervalSchedule,
    pub maturity_terminal_interval_indices: Vec<usize>,
}

pub type VanillaOptionSurfacePricingExecutionInputError =
    VanillaOptionGridBatchPricingExecutionInputError;

#[derive(Debug, Clone, PartialEq)]
pub enum VanillaOptionSurfacePricingRunError {
    InvalidExecutionInput {
        source: VanillaOptionSurfacePricingExecutionInputError,
    },
    InvalidDriverParameters {
        source: NigSharedPlusShiftConfigurationError,
    },
    NodePricingFailure {
        message: String,
    },
}

impl Display for VanillaOptionSurfacePricingRunError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidExecutionInput { source } => write!(
                formatter,
                "surface pricing execution input is invalid: {source}"
            ),
            Self::InvalidDriverParameters { source } => write!(
                formatter,
                "surface pricing driver parameters are invalid: {source}"
            ),
            Self::NodePricingFailure { message } => {
                write!(formatter, "surface pricing failed: {message}")
            }
        }
    }
}

impl Error for VanillaOptionSurfacePricingRunError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidExecutionInput { source } => Some(source),
            Self::InvalidDriverParameters { source } => Some(source),
            Self::NodePricingFailure { .. } => None,
        }
    }
}

impl VanillaOptionSurfacePricingExecutionInput {
    pub fn validate(&self) -> Result<(), VanillaOptionSurfacePricingExecutionInputError> {
        VanillaOptionGridBatchPricingExecutionInput {
            request: VanillaOptionGridBatchPricingRequest {
                spot: self.request.market_inputs.spot,
                grids: vec![VanillaOptionGridRequest {
                    payoff_type: self.request.payoff.payoff_type,
                    risk_free_rate: self.request.market_inputs.risk_free_rate,
                    dividend_yield: self.request.market_inputs.dividend_yield,
                    strikes: self.request.market_inputs.strikes.clone(),
                    maturity_intervals: self.request.market_inputs.maturity_intervals.clone(),
                }],
            },
            boundary_input: self.boundary_input.clone(),
            model_parameters: self.model_parameters.clone(),
            runtime_interval_schedule: self.runtime_interval_schedule.clone(),
            interval_delta: self.interval_delta,
            interval_factors: self.interval_factors.clone(),
            optional_output_selection: self.optional_output_selection,
            cpu_count: self.cpu_count,
        }
        .validate()
    }
}

pub fn run_vanilla_option_grid_batch_pricing(
    execution_input: VanillaOptionGridBatchPricingExecutionInput,
) -> Result<VanillaOptionGridBatchPricingRunResult, VanillaOptionGridBatchPricingRunError> {
    run_vanilla_option_grid_batch_pricing_with_observer(execution_input, None)
}

pub fn run_vanilla_option_grid_batch_pricing_with_observer(
    execution_input: VanillaOptionGridBatchPricingExecutionInput,
    mut observer: Option<&mut dyn FnMut(PricingProgressEvent)>,
) -> Result<VanillaOptionGridBatchPricingRunResult, VanillaOptionGridBatchPricingRunError> {
    execution_input.validate().map_err(|source| {
        VanillaOptionGridBatchPricingRunError::InvalidExecutionInput { source }
    })?;

    let total_grids = execution_input.request.grids.len();

    let driver_parameters = execution_input
        .model_parameters
        .nig_shared_plus_shift_configuration
        .derive_driver_parameters()
        .map_err(
            |source| VanillaOptionGridBatchPricingRunError::InvalidDriverParameters { source },
        )?;

    let mut grids = Vec::with_capacity(total_grids);

    let pricing_thread_pool = build_pricing_thread_pool(execution_input.cpu_count);

    let progress_reporter = if observer.is_some() {
        PricingProgressReporter::shared()
    } else {
        PricingProgressReporter::disabled()
    };

    if let Some(observer) = observer.as_deref_mut() {
        std::thread::scope(|scope| {
            let worker_progress_reporter = progress_reporter.clone();
            let execution_input_ref = &execution_input;
            let grids_ref = &mut grids;

            let worker = scope.spawn(move || {
                struct FinishOnDrop(PricingProgressReporter);

                impl Drop for FinishOnDrop {
                    fn drop(&mut self) {
                        self.0.finish();
                    }
                }

                let _finish_guard = FinishOnDrop(worker_progress_reporter.clone());

                run_grid_batch_body(
                    execution_input_ref,
                    total_grids,
                    driver_parameters,
                    pricing_thread_pool.as_ref(),
                    &worker_progress_reporter,
                    grids_ref,
                )
            });

            progress_reporter.poll_until_finished(observer);

            worker.join().map_err(|_| {
                VanillaOptionGridBatchPricingRunError::NodePricingFailure {
                    message: "pricing progress worker terminated unexpectedly".to_string(),
                }
            })?
        })?;
    } else {
        run_grid_batch_body(
            &execution_input,
            total_grids,
            driver_parameters,
            pricing_thread_pool.as_ref(),
            &progress_reporter,
            &mut grids,
        )?;
    }

    Ok(VanillaOptionGridBatchPricingRunResult {
        spot: execution_input.request.spot,
        grids,
        runtime_interval_schedule: execution_input.runtime_interval_schedule,
    })
}

fn run_grid_batch_body(
    execution_input: &VanillaOptionGridBatchPricingExecutionInput,
    total_grids: usize,
    driver_parameters: NigBuySellDriverParameters,
    pricing_thread_pool: Option<&rayon::ThreadPool>,
    progress_reporter: &PricingProgressReporter,
    grids: &mut Vec<VanillaOptionGridPricingResult>,
) -> Result<(), VanillaOptionGridBatchPricingRunError> {
    progress_reporter.emit(PricingProgressEvent::group_progress(
        0,
        total_grids,
        Some(0),
    ));

    if !progress_reporter.is_enabled()
        && resolved_pricing_cpu_count(execution_input.cpu_count) > 1
        && total_grids > 1
    {
        let grid_results = install_pricing_parallelism(pricing_thread_pool, || {
            execution_input
                .request
                .grids
                .par_iter()
                .map(|grid| {
                    run_homogeneous_scope(
                        execution_input.request.spot,
                        grid,
                        execution_input.boundary_input.clone(),
                        execution_input.model_parameters.clone(),
                        &execution_input.runtime_interval_schedule,
                        execution_input.interval_delta,
                        execution_input.interval_factors.as_slice(),
                        execution_input.optional_output_selection,
                        driver_parameters,
                        pricing_thread_pool,
                        execution_input.cpu_count,
                        progress_reporter,
                    )
                    .map_err(|message| {
                        VanillaOptionGridBatchPricingRunError::NodePricingFailure { message }
                    })
                })
                .collect::<Result<Vec<_>, _>>()
        })?;

        grids.extend(grid_results);
        progress_reporter.emit(PricingProgressEvent::group_progress(
            total_grids,
            total_grids,
            None,
        ));
        return Ok(());
    }

    for (grid_index, grid) in execution_input.request.grids.iter().enumerate() {
        let grid_result = run_homogeneous_scope(
            execution_input.request.spot,
            grid,
            execution_input.boundary_input.clone(),
            execution_input.model_parameters.clone(),
            &execution_input.runtime_interval_schedule,
            execution_input.interval_delta,
            execution_input.interval_factors.as_slice(),
            execution_input.optional_output_selection,
            driver_parameters,
            pricing_thread_pool,
            execution_input.cpu_count,
            progress_reporter,
        )
        .map_err(
            |source| VanillaOptionGridBatchPricingRunError::NodePricingFailure { message: source },
        )?;

        grids.push(grid_result);

        let next_grid_index = if grid_index + 1 < total_grids {
            Some(grid_index + 1)
        } else {
            None
        };

        progress_reporter.emit(PricingProgressEvent::group_progress(
            grid_index + 1,
            total_grids,
            next_grid_index,
        ));
    }

    Ok(())
}

pub fn run_vanilla_option_surface_pricing(
    execution_input: VanillaOptionSurfacePricingExecutionInput,
) -> Result<VanillaOptionSurfacePricingRunResult, VanillaOptionSurfacePricingRunError> {
    run_vanilla_option_surface_pricing_with_observer(execution_input, None)
}

pub fn run_vanilla_option_surface_pricing_with_observer(
    execution_input: VanillaOptionSurfacePricingExecutionInput,
    observer: Option<&mut dyn FnMut(PricingProgressEvent)>,
) -> Result<VanillaOptionSurfacePricingRunResult, VanillaOptionSurfacePricingRunError> {
    execution_input
        .validate()
        .map_err(|source| VanillaOptionSurfacePricingRunError::InvalidExecutionInput { source })?;

    let grid_result = run_vanilla_option_grid_batch_pricing_with_observer(
        VanillaOptionGridBatchPricingExecutionInput {
            request: VanillaOptionGridBatchPricingRequest {
                spot: execution_input.request.market_inputs.spot,
                grids: vec![VanillaOptionGridRequest {
                    payoff_type: execution_input.request.payoff.payoff_type,
                    risk_free_rate: execution_input.request.market_inputs.risk_free_rate,
                    dividend_yield: execution_input.request.market_inputs.dividend_yield,
                    strikes: execution_input.request.market_inputs.strikes.clone(),
                    maturity_intervals: execution_input
                        .request
                        .market_inputs
                        .maturity_intervals
                        .clone(),
                }],
            },
            boundary_input: execution_input.boundary_input,
            model_parameters: execution_input.model_parameters,
            runtime_interval_schedule: execution_input.runtime_interval_schedule,
            interval_delta: execution_input.interval_delta,
            interval_factors: execution_input.interval_factors,
            optional_output_selection: execution_input.optional_output_selection,
            cpu_count: execution_input.cpu_count,
        },
        observer,
    )
    .map_err(|source| match source {
        VanillaOptionGridBatchPricingRunError::InvalidExecutionInput { source } => {
            VanillaOptionSurfacePricingRunError::InvalidExecutionInput { source }
        }
        VanillaOptionGridBatchPricingRunError::InvalidDriverParameters { source } => {
            VanillaOptionSurfacePricingRunError::InvalidDriverParameters { source }
        }
        VanillaOptionGridBatchPricingRunError::NodePricingFailure { message } => {
            VanillaOptionSurfacePricingRunError::NodePricingFailure { message }
        }
    })?;

    let grid = grid_result.grids.into_iter().next().ok_or(
        VanillaOptionSurfacePricingRunError::NodePricingFailure {
            message: "missing surface pricing grid result".to_string(),
        },
    )?;

    Ok(VanillaOptionSurfacePricingRunResult {
        response: VanillaOptionSurfacePricingResponse {
            spot: grid_result.spot,
            strikes: grid.strikes,
            maturity_intervals: grid.maturity_intervals,
            payoff_type: grid.payoff_type,
            risk_free_rate: grid.risk_free_rate,
            dividend_yield: grid.dividend_yield,
            price_grid: grid.price_grid,
            implied_volatility_grid: grid.implied_volatility_grid,
            implied_volatility_inversion_status_grid: grid.implied_volatility_inversion_status_grid,
            pricing_status_grid: grid.pricing_status_grid,
            pricing_failure_stage_grid: grid.pricing_failure_stage_grid,
            pricing_failure_reason_grid: grid.pricing_failure_reason_grid,
            forward_price_grid: grid.forward_price_grid,
            discount_factor_grid: grid.discount_factor_grid,
            boundary_state_norm_grid: grid.boundary_state_norm_grid,
        },
        maturity_summaries: grid.maturity_summaries,
        runtime_interval_schedule: grid_result.runtime_interval_schedule,
        maturity_terminal_interval_indices: grid.maturity_terminal_interval_indices,
    })
}

fn run_homogeneous_scope(
    spot: f64,
    grid: &VanillaOptionGridRequest,
    boundary_input: AnchoredVanillaOptionPricingBoundaryInput,
    model_parameters: VanillaOptionPricingModelParameters,
    runtime_interval_schedule: &PricingIntervalSchedule,
    interval_delta: f64,
    interval_factors: &[VanillaOptionPricingIntervalFactors],
    optional_output_selection: VanillaOptionPricingOptionalOutputSelection,
    driver_parameters: NigBuySellDriverParameters,
    pricing_thread_pool: Option<&rayon::ThreadPool>,
    cpu_count: Option<usize>,
    progress_reporter: &PricingProgressReporter,
) -> Result<VanillaOptionGridPricingResult, String> {
    let maturity_interval_counts = maturity_interval_counts(
        grid.maturity_intervals.as_slice(),
        runtime_interval_schedule,
    );

    let forward_law_scope = prepare_forward_law_scope(
        spot,
        grid,
        boundary_input.clone(),
        model_parameters.clone(),
        runtime_interval_schedule,
        interval_delta,
        interval_factors,
        driver_parameters,
        pricing_thread_pool,
        cpu_count,
        progress_reporter,
        maturity_interval_counts.as_slice(),
    )?;

    let rows = if !progress_reporter.is_enabled()
        && resolved_pricing_cpu_count(cpu_count) > 1
        && maturity_interval_counts.len() > 1
    {
        install_pricing_parallelism(pricing_thread_pool, || {
            maturity_interval_counts
                .par_iter()
                .enumerate()
                .map(|(maturity_index, maturity_interval_count)| {
                    group_pricing_row_for_maturity(
                        spot,
                        grid,
                        maturity_index,
                        maturity_interval_count,
                        &boundary_input,
                        &model_parameters,
                        runtime_interval_schedule,
                        interval_delta,
                        optional_output_selection,
                        interval_factors,
                        &forward_law_scope,
                        pricing_thread_pool,
                        cpu_count,
                        progress_reporter,
                    )
                })
                .collect::<Result<Vec<_>, _>>()
        })?
    } else {
        let mut rows = Vec::with_capacity(maturity_interval_counts.len());

        for (maturity_index, maturity_interval_count) in maturity_interval_counts.iter().enumerate()
        {
            progress_reporter.emit(PricingProgressEvent::maturity_progress(
                maturity_index,
                grid.maturity_intervals.len(),
                Some(maturity_index),
                grid.maturity_intervals.len(),
            ));

            rows.push(group_pricing_row_for_maturity(
                spot,
                grid,
                maturity_index,
                maturity_interval_count,
                &boundary_input,
                &model_parameters,
                runtime_interval_schedule,
                interval_delta,
                optional_output_selection,
                interval_factors,
                &forward_law_scope,
                pricing_thread_pool,
                cpu_count,
                progress_reporter,
            )?);
        }

        progress_reporter.emit(PricingProgressEvent::maturity_progress(
            grid.maturity_intervals.len(),
            grid.maturity_intervals.len(),
            None,
            grid.maturity_intervals.len(),
        ));

        rows
    };

    let mut result_builder = GroupResultBuilder::new(grid);

    for row in rows {
        result_builder.push_row(row);
    }

    result_builder.finish(grid, runtime_interval_schedule)
}

struct ForwardLawScope {
    base_request: Option<VanillaOptionPricingRequest>,
    outcome: Option<Result<ForwardLawScopeSolution, VanillaOptionPricingFailureDiagnostic>>,
}

struct GroupResultBuilder {
    price_grid: Vec<Vec<Option<f64>>>,
    implied_volatility_grid: Vec<Vec<Option<f64>>>,
    implied_volatility_inversion_status_grid:
        Vec<Vec<Option<VanillaOptionImpliedVolatilityInversionStatus>>>,
    pricing_status_grid: Vec<Vec<VanillaOptionPricingStatus>>,
    pricing_failure_stage_grid: Vec<Vec<Option<VanillaOptionPricingFailureStage>>>,
    pricing_failure_reason_grid: Vec<Vec<Option<String>>>,
    forward_price_grid: Vec<Vec<Option<f64>>>,
    discount_factor_grid: Vec<Vec<Option<f64>>>,
    boundary_state_norm_grid: Vec<Vec<Option<f64>>>,
    maturity_summaries: Vec<Option<VanillaOptionMaturitySummary>>,
}

impl GroupResultBuilder {
    fn new(group: &VanillaOptionGridRequest) -> Self {
        Self {
            price_grid: Vec::with_capacity(group.maturity_intervals.len()),
            implied_volatility_grid: Vec::with_capacity(group.maturity_intervals.len()),
            implied_volatility_inversion_status_grid: Vec::with_capacity(
                group.maturity_intervals.len(),
            ),
            pricing_status_grid: Vec::with_capacity(group.maturity_intervals.len()),
            pricing_failure_stage_grid: Vec::with_capacity(group.maturity_intervals.len()),
            pricing_failure_reason_grid: Vec::with_capacity(group.maturity_intervals.len()),
            forward_price_grid: Vec::with_capacity(group.maturity_intervals.len()),
            discount_factor_grid: Vec::with_capacity(group.maturity_intervals.len()),
            boundary_state_norm_grid: Vec::with_capacity(group.maturity_intervals.len()),
            maturity_summaries: Vec::with_capacity(group.maturity_intervals.len()),
        }
    }

    fn push_row(&mut self, row: GroupPricingRow) {
        self.price_grid.push(row.price_row);
        self.implied_volatility_grid
            .push(row.implied_volatility_row);
        self.implied_volatility_inversion_status_grid
            .push(row.inversion_status_row);
        self.pricing_status_grid.push(row.pricing_status_row);
        self.pricing_failure_stage_grid
            .push(row.pricing_failure_stage_row);
        self.pricing_failure_reason_grid
            .push(row.pricing_failure_reason_row);
        self.forward_price_grid.push(row.forward_price_row);
        self.discount_factor_grid.push(row.discount_factor_row);
        self.boundary_state_norm_grid
            .push(row.boundary_state_norm_row);
        self.maturity_summaries.push(row.maturity_summary);
    }

    fn finish(
        self,
        group: &VanillaOptionGridRequest,
        runtime_interval_schedule: &PricingIntervalSchedule,
    ) -> Result<VanillaOptionGridPricingResult, String> {
        let maturity_terminal_interval_indices = group
            .maturity_intervals
            .iter()
            .copied()
            .map(|maturity_interval| {
                runtime_interval_schedule
                    .terminal_interval_index_for_maturity_interval(maturity_interval)
                    .map_err(|source| {
                        format!("grid maturity terminal interval mapping failed: {source}")
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok(VanillaOptionGridPricingResult {
            payoff_type: group.payoff_type,
            risk_free_rate: group.risk_free_rate,
            dividend_yield: group.dividend_yield,
            strikes: group.strikes.clone(),
            maturity_intervals: group.maturity_intervals.clone(),
            maturity_terminal_interval_indices,
            price_grid: self.price_grid,
            implied_volatility_grid: self.implied_volatility_grid,
            implied_volatility_inversion_status_grid: self.implied_volatility_inversion_status_grid,
            pricing_status_grid: self.pricing_status_grid,
            pricing_failure_stage_grid: self.pricing_failure_stage_grid,
            pricing_failure_reason_grid: self.pricing_failure_reason_grid,
            forward_price_grid: self.forward_price_grid,
            discount_factor_grid: self.discount_factor_grid,
            boundary_state_norm_grid: self.boundary_state_norm_grid,
            maturity_summaries: self.maturity_summaries,
        })
    }
}

struct GroupPricingRow {
    price_row: Vec<Option<f64>>,
    implied_volatility_row: Vec<Option<f64>>,
    inversion_status_row: Vec<Option<VanillaOptionImpliedVolatilityInversionStatus>>,
    pricing_status_row: Vec<VanillaOptionPricingStatus>,
    pricing_failure_stage_row: Vec<Option<VanillaOptionPricingFailureStage>>,
    pricing_failure_reason_row: Vec<Option<String>>,
    forward_price_row: Vec<Option<f64>>,
    discount_factor_row: Vec<Option<f64>>,
    boundary_state_norm_row: Vec<Option<f64>>,
    maturity_summary: Option<VanillaOptionMaturitySummary>,
}

struct GroupPricingRowBuilder {
    row: GroupPricingRow,
    has_successful_node: bool,
}

impl GroupPricingRowBuilder {
    fn new(node_count: usize) -> Self {
        Self {
            row: GroupPricingRow {
                price_row: Vec::with_capacity(node_count),
                implied_volatility_row: Vec::with_capacity(node_count),
                inversion_status_row: Vec::with_capacity(node_count),
                pricing_status_row: Vec::with_capacity(node_count),
                pricing_failure_stage_row: Vec::with_capacity(node_count),
                pricing_failure_reason_row: Vec::with_capacity(node_count),
                forward_price_row: Vec::with_capacity(node_count),
                discount_factor_row: Vec::with_capacity(node_count),
                boundary_state_norm_row: Vec::with_capacity(node_count),
                maturity_summary: None,
            },
            has_successful_node: false,
        }
    }

    fn push_pricing_response(&mut self, response: &VanillaOptionPricingResponse) {
        self.row.price_row.push(response.price);
        self.row
            .implied_volatility_row
            .push(response.implied_volatility);
        self.row
            .inversion_status_row
            .push(response.implied_volatility_inversion_status);
        self.row.pricing_status_row.push(response.pricing_status);
        self.row.pricing_failure_stage_row.push(
            response
                .pricing_failure_diagnostic
                .as_ref()
                .map(|diagnostic| diagnostic.stage),
        );
        self.row.pricing_failure_reason_row.push(
            response
                .pricing_failure_diagnostic
                .as_ref()
                .map(|diagnostic| diagnostic.reason.clone()),
        );
        self.row.forward_price_row.push(
            response
                .diagnostics
                .as_ref()
                .map(|diagnostics| diagnostics.forward_price),
        );
        self.row.discount_factor_row.push(
            response
                .diagnostics
                .as_ref()
                .map(|diagnostics| diagnostics.discount_factor),
        );
        self.row.boundary_state_norm_row.push(
            response
                .diagnostics
                .as_ref()
                .map(|diagnostics| diagnostics.boundary_state_norm),
        );

        if response.is_successful() {
            self.has_successful_node = true;
        }
    }

    fn has_successful_node(&self) -> bool {
        self.has_successful_node
    }

    fn finish(mut self, maturity_summary: Option<VanillaOptionMaturitySummary>) -> GroupPricingRow {
        self.row.maturity_summary = maturity_summary;
        self.row
    }
}

fn maturity_interval_counts(
    maturity_intervals: &[f64],
    runtime_interval_schedule: &PricingIntervalSchedule,
) -> Vec<Result<usize, String>> {
    maturity_intervals
        .iter()
        .map(|maturity_interval| {
            runtime_interval_schedule
                .terminal_interval_index_for_maturity_interval(*maturity_interval)
                .map(|terminal_interval_index| terminal_interval_index + 1)
                .map_err(|source| source.to_string())
        })
        .collect()
}

fn prepare_forward_law_scope(
    spot: f64,
    group: &VanillaOptionGridRequest,
    boundary_input: AnchoredVanillaOptionPricingBoundaryInput,
    model_parameters: VanillaOptionPricingModelParameters,
    runtime_interval_schedule: &PricingIntervalSchedule,
    interval_delta: f64,
    interval_factors: &[VanillaOptionPricingIntervalFactors],
    driver_parameters: NigBuySellDriverParameters,
    pricing_thread_pool: Option<&rayon::ThreadPool>,
    cpu_count: Option<usize>,
    progress_reporter: &PricingProgressReporter,
    maturity_interval_counts: &[Result<usize, String>],
) -> Result<ForwardLawScope, String> {
    let max_interval_count = maturity_interval_counts
        .iter()
        .filter_map(|interval_count| interval_count.as_ref().ok().copied())
        .max()
        .unwrap_or(0);

    let base_request = if max_interval_count == 0 {
        None
    } else {
        Some(
            base_scope_request(
                spot,
                group.risk_free_rate,
                group.dividend_yield,
                group.payoff_type,
                runtime_interval_schedule
                    .maturity_interval_hours(max_interval_count - 1)
                    .map_err(|source| source.to_string())?,
                boundary_input,
                model_parameters,
                interval_delta,
            )
            .map_err(|source| source.to_string())?,
        )
    };

    let multi_interval_counts = maturity_interval_counts
        .iter()
        .filter_map(|interval_count| interval_count.as_ref().ok().copied())
        .filter(|interval_count| *interval_count > 1)
        .collect::<Vec<_>>();

    let outcome = if multi_interval_counts.is_empty() {
        None
    } else {
        let base_request = base_request.as_ref().ok_or_else(|| {
            "grid batch pricing requires at least one valid maturity interval".to_string()
        })?;

        Some(
            match solve_forward_law_scope(
                base_request,
                runtime_interval_schedule,
                group.strikes.as_slice(),
                multi_interval_counts.as_slice(),
                &interval_factors[..max_interval_count],
                driver_parameters,
                pricing_thread_pool,
                cpu_count,
                progress_reporter,
            ) {
                Ok(solution) => Ok(solution),
                Err(error) => Err(pricing_failure_diagnostic_from_run_error(&error)),
            },
        )
    };

    Ok(ForwardLawScope {
        base_request,
        outcome,
    })
}

fn group_pricing_row_for_maturity(
    spot: f64,
    group: &VanillaOptionGridRequest,
    maturity_index: usize,
    maturity_interval_count: &Result<usize, String>,
    boundary_input: &AnchoredVanillaOptionPricingBoundaryInput,
    model_parameters: &VanillaOptionPricingModelParameters,
    runtime_interval_schedule: &PricingIntervalSchedule,
    interval_delta: f64,
    optional_output_selection: VanillaOptionPricingOptionalOutputSelection,
    interval_factors: &[VanillaOptionPricingIntervalFactors],
    forward_law_scope: &ForwardLawScope,
    pricing_thread_pool: Option<&rayon::ThreadPool>,
    cpu_count: Option<usize>,
    progress_reporter: &PricingProgressReporter,
) -> Result<GroupPricingRow, String> {
    let maturity_interval_count = match maturity_interval_count {
        Ok(maturity_interval_count) => *maturity_interval_count,
        Err(reason) => {
            return Ok(failed_group_row_for_reason(
                group.strikes.len(),
                VanillaOptionPricingFailureStage::NodeSetup,
                reason.clone(),
            ));
        }
    };

    if maturity_interval_count == 1 {
        return price_direct_fourier_row(
            spot,
            group,
            maturity_index,
            boundary_input.clone(),
            model_parameters.clone(),
            runtime_interval_schedule,
            interval_delta,
            optional_output_selection,
            interval_factors[0],
            pricing_thread_pool,
            cpu_count,
            progress_reporter,
        )
        .map_err(|source| source.to_string());
    }

    price_forward_law_row(
        group,
        maturity_index,
        optional_output_selection,
        forward_law_scope,
        pricing_thread_pool,
        cpu_count,
        progress_reporter,
    )
}

fn price_forward_law_row(
    group: &VanillaOptionGridRequest,
    maturity_index: usize,
    optional_output_selection: VanillaOptionPricingOptionalOutputSelection,
    forward_law_scope: &ForwardLawScope,
    pricing_thread_pool: Option<&rayon::ThreadPool>,
    cpu_count: Option<usize>,
    progress_reporter: &PricingProgressReporter,
) -> Result<GroupPricingRow, String> {
    let terminal_snapshot = match forward_law_scope.outcome.as_ref() {
        Some(Ok(solution)) => solution.terminal_snapshots.iter().find(|snapshot| {
            snapshot.maturity_interval == group.maturity_intervals[maturity_index]
        }),
        Some(Err(failure_diagnostic)) => {
            return Ok(failed_group_row_from_diagnostic(
                group.strikes.len(),
                failure_diagnostic.clone(),
            ));
        }
        None => None,
    };

    let Some(terminal_snapshot) = terminal_snapshot else {
        return Ok(failed_group_row_for_reason(
            group.strikes.len(),
            VanillaOptionPricingFailureStage::ForwardLaw,
            format!(
                "missing forward-law terminal snapshot for maturity interval {}",
                group.maturity_intervals[maturity_index]
            ),
        ));
    };

    let base_request = forward_law_scope
        .base_request
        .as_ref()
        .ok_or_else(|| "forward-law pricing requires a valid base request".to_string())?;

    let priced_strikes = price_terminal_snapshot_strikes(
        base_request,
        terminal_snapshot,
        group.strikes.as_slice(),
        maturity_index,
        group.maturity_intervals.len(),
        pricing_thread_pool,
        cpu_count,
        progress_reporter,
    );

    Ok(forward_law_group_row(
        priced_strikes.as_slice(),
        Some(maturity_summary_from_terminal_snapshot(
            terminal_snapshot,
            optional_output_selection,
        )),
    ))
}

fn forward_law_group_row(
    priced_strikes: &[ForwardPricedStrike],
    maturity_summary: Option<VanillaOptionMaturitySummary>,
) -> GroupPricingRow {
    GroupPricingRow {
        price_row: priced_strikes.iter().map(|node| Some(node.price)).collect(),
        implied_volatility_row: priced_strikes
            .iter()
            .map(|node| node.implied_volatility_outcome.implied_volatility)
            .collect(),
        inversion_status_row: priced_strikes
            .iter()
            .map(|node| Some(node.implied_volatility_outcome.inversion_status))
            .collect(),
        pricing_status_row: priced_strikes
            .iter()
            .map(|_| VanillaOptionPricingStatus::Succeeded)
            .collect(),
        pricing_failure_stage_row: priced_strikes.iter().map(|_| None).collect(),
        pricing_failure_reason_row: priced_strikes.iter().map(|_| None).collect(),
        forward_price_row: priced_strikes
            .iter()
            .map(|node| Some(node.forward_price))
            .collect(),
        discount_factor_row: priced_strikes
            .iter()
            .map(|node| Some(node.discount_factor))
            .collect(),
        boundary_state_norm_row: priced_strikes
            .iter()
            .map(|node| Some(node.boundary_state_norm))
            .collect(),
        maturity_summary,
    }
}

fn failed_group_row_for_reason(
    strike_count: usize,
    stage: VanillaOptionPricingFailureStage,
    reason: String,
) -> GroupPricingRow {
    failed_group_row_from_diagnostic(
        strike_count,
        VanillaOptionPricingFailureDiagnostic::new(stage, reason),
    )
}

fn failed_group_row_from_diagnostic(
    strike_count: usize,
    failure_diagnostic: VanillaOptionPricingFailureDiagnostic,
) -> GroupPricingRow {
    GroupPricingRow {
        price_row: (0..strike_count).map(|_| None).collect(),
        implied_volatility_row: (0..strike_count).map(|_| None).collect(),
        inversion_status_row: (0..strike_count).map(|_| None).collect(),
        pricing_status_row: (0..strike_count)
            .map(|_| VanillaOptionPricingStatus::Failed)
            .collect(),
        pricing_failure_stage_row: (0..strike_count)
            .map(|_| Some(failure_diagnostic.stage))
            .collect(),
        pricing_failure_reason_row: (0..strike_count)
            .map(|_| Some(failure_diagnostic.reason.clone()))
            .collect(),
        forward_price_row: (0..strike_count).map(|_| None).collect(),
        discount_factor_row: (0..strike_count).map(|_| None).collect(),
        boundary_state_norm_row: (0..strike_count).map(|_| None).collect(),
        maturity_summary: None,
    }
}

fn price_direct_fourier_row(
    spot: f64,
    group: &VanillaOptionGridRequest,
    maturity_index: usize,
    boundary_input: AnchoredVanillaOptionPricingBoundaryInput,
    model_parameters: VanillaOptionPricingModelParameters,
    runtime_interval_schedule: &PricingIntervalSchedule,
    interval_delta: f64,
    optional_output_selection: VanillaOptionPricingOptionalOutputSelection,
    interval_factor: VanillaOptionPricingIntervalFactors,
    pricing_thread_pool: Option<&rayon::ThreadPool>,
    cpu_count: Option<usize>,
    progress_reporter: &PricingProgressReporter,
) -> Result<GroupPricingRow, VanillaOptionPricingRunError> {
    let internal_output_selection =
        VanillaOptionPricingOptionalOutputSelection::new(true, true, false);

    let solve_strike = |strike: f64| {
        let request = VanillaOptionPricingRequest::new(
            VanillaOptionPricingMarketInputs::new_single_node(
                spot,
                strike,
                group.maturity_intervals[maturity_index],
                group.risk_free_rate,
                group.dividend_yield,
            ),
            VanillaOptionPayoffConfiguration {
                payoff_type: group.payoff_type,
            },
            boundary_input.clone(),
            model_parameters.clone(),
            interval_delta,
        );

        let run_result = run_vanilla_option_pricing_with_observer(
            VanillaOptionPricingExecutionInput::new(
                request,
                PricingIntervalSchedule::try_new(
                    runtime_interval_schedule.valuation_time_utc_nanos,
                    runtime_interval_schedule.metadata.clone(),
                    runtime_interval_schedule.interval_end_timestamps_utc_nanos
                        [..1]
                        .to_vec(),
                )
                .map_err(|source| VanillaOptionPricingRunError::InvalidExecutionInput {
                    source: super::runner::VanillaOptionPricingExecutionInputError::InvalidRuntimeIntervalSchedule {
                        message: source.to_string(),
                    },
                })?,
                vec![interval_factor],
                internal_output_selection,
            ),
            None,
        )?;

        Ok::<_, VanillaOptionPricingRunError>(run_result)
    };

    progress_reporter.start_strike_pricing(
        maturity_index,
        group.maturity_intervals.len(),
        group.strikes.len(),
    );

    let solved_strikes = if resolved_pricing_cpu_count(cpu_count) <= 1 {
        group
            .strikes
            .iter()
            .copied()
            .map(|strike| {
                let run_result = solve_strike(strike)?;
                progress_reporter.advance_strike_pricing(1);
                Ok::<_, VanillaOptionPricingRunError>(run_result)
            })
            .collect::<Result<Vec<_>, _>>()?
    } else {
        install_pricing_parallelism(pricing_thread_pool, || {
            group
                .strikes
                .par_iter()
                .copied()
                .map(|strike| {
                    let run_result = solve_strike(strike)?;
                    progress_reporter.advance_strike_pricing(1);
                    Ok::<_, VanillaOptionPricingRunError>(run_result)
                })
                .collect::<Result<Vec<_>, _>>()
        })?
    };

    progress_reporter.finish_strike_pricing();

    let mut row_builder = GroupPricingRowBuilder::new(group.strikes.len());
    let mut representative_transform = None;
    let mut integration_quality = None;
    let mut interval_summaries = Vec::new();

    for run_result in solved_strikes {
        row_builder.push_pricing_response(&run_result.response);

        if let Some(run_diagnostics) = run_result.run_diagnostics {
            if representative_transform.is_none() {
                representative_transform = run_diagnostics.transform;
            }

            if integration_quality.is_none() {
                integration_quality = run_diagnostics.integration_quality;
            }

            if interval_summaries.is_empty() {
                if let Some(anchored_recursion) = run_diagnostics.anchored_recursion {
                    interval_summaries = anchored_recursion
                        .interval_diagnostics
                        .into_iter()
                        .map(interval_summary_from_recursion_diagnostics)
                        .collect();
                }
            }
        }
    }

    let has_successful_node = row_builder.has_successful_node();

    Ok(row_builder.finish(direct_fourier_maturity_summary(
        group,
        maturity_index,
        optional_output_selection,
        has_successful_node,
        interval_summaries,
        representative_transform,
        integration_quality,
    )))
}

fn direct_fourier_maturity_summary(
    group: &VanillaOptionGridRequest,
    maturity_index: usize,
    optional_output_selection: VanillaOptionPricingOptionalOutputSelection,
    has_successful_node: bool,
    interval_summaries: Vec<VanillaOptionMaturityIntervalSummary>,
    representative_transform: Option<ExponentialAffineTransformIntegrationResult>,
    integration_quality: Option<FourierInversionDiagnostics>,
) -> Option<VanillaOptionMaturitySummary> {
    if !has_successful_node {
        return None;
    }

    Some(VanillaOptionMaturitySummary {
        maturity_interval: group.maturity_intervals[maturity_index],
        engine_kind: VanillaOptionMaturityPricingEngineKind::DirectFourier,
        terminal_anchor_value: None,
        deviation_grid_lower_bound: None,
        deviation_grid_upper_bound: None,
        deviation_grid_node_count: None,
        terminal_deviation_nodes: None,
        terminal_probability_mass: None,
        interval_summaries,
        representative_transform: if optional_output_selection.include_transform_diagnostics {
            representative_transform
        } else {
            None
        },
        integration_quality: if optional_output_selection.include_integration_quality_diagnostics {
            integration_quality
        } else {
            None
        },
    })
}

fn maturity_summary_from_terminal_snapshot(
    terminal_snapshot: &ForwardLawTerminalSnapshot,
    optional_output_selection: VanillaOptionPricingOptionalOutputSelection,
) -> VanillaOptionMaturitySummary {
    VanillaOptionMaturitySummary {
        maturity_interval: terminal_snapshot.maturity_interval,
        engine_kind: VanillaOptionMaturityPricingEngineKind::ForwardLaw,
        terminal_anchor_value: Some(terminal_snapshot.terminal_anchor_value),
        deviation_grid_lower_bound: Some(terminal_snapshot.deviation_grid.lower_bound),
        deviation_grid_upper_bound: Some(terminal_snapshot.deviation_grid.upper_bound),
        deviation_grid_node_count: Some(terminal_snapshot.deviation_grid.node_count),
        terminal_deviation_nodes: Some(terminal_snapshot.terminal_deviation_nodes.clone()),
        terminal_probability_mass: Some(terminal_snapshot.terminal_probability_mass.clone()),
        interval_summaries: terminal_snapshot
            .anchored_recursion_diagnostics
            .interval_diagnostics
            .iter()
            .cloned()
            .map(interval_summary_from_recursion_diagnostics)
            .collect(),
        representative_transform: if optional_output_selection.include_transform_diagnostics {
            terminal_snapshot.representative_transform_diagnostics
        } else {
            None
        },
        integration_quality: None,
    }
}

fn interval_summary_from_recursion_diagnostics(
    diagnostics: AnchoredPricingRecursionIntervalDiagnostics,
) -> VanillaOptionMaturityIntervalSummary {
    VanillaOptionMaturityIntervalSummary {
        interval_index: diagnostics.interval_index,
        current_anchor_value: diagnostics.current_anchor_value,
        next_anchor_value: diagnostics.next_anchor_value,
        interval_delta: diagnostics.interval_delta,
        expected_hawkes_boundary_state_before: diagnostics.expected_hawkes_boundary_state_before,
        expected_hawkes_boundary_state_after: diagnostics.expected_hawkes_boundary_state_after,
        maximum_tail_probability_mass: diagnostics.maximum_tail_probability_mass,
        maximum_row_probability_mass_error: diagnostics.maximum_row_probability_mass_error,
        minimum_solved_theta: diagnostics.minimum_solved_theta,
        maximum_solved_theta: diagnostics.maximum_solved_theta,
    }
}

fn base_scope_request(
    spot: f64,
    risk_free_rate: f64,
    dividend_yield: f64,
    payoff_type: VanillaOptionPayoffType,
    maturity_interval: f64,
    boundary_input: AnchoredVanillaOptionPricingBoundaryInput,
    model_parameters: VanillaOptionPricingModelParameters,
    interval_delta: f64,
) -> Result<VanillaOptionPricingRequest, super::contracts::VanillaOptionPricingRequestError> {
    VanillaOptionPricingRequest::try_new(
        VanillaOptionPricingMarketInputs::try_new_single_node(
            spot,
            spot,
            maturity_interval,
            risk_free_rate,
            dividend_yield,
        )
        .map_err(|source| VanillaOptionPricingRequestError::InvalidMarketInputs { source })?,
        VanillaOptionPayoffConfiguration { payoff_type },
        boundary_input,
        model_parameters,
        interval_delta,
    )
}

fn validate_finite(
    field: &'static str,
    value: f64,
) -> Result<(), VanillaOptionGridBatchPricingRequestError> {
    if !value.is_finite() {
        return Err(VanillaOptionGridBatchPricingRequestError::InvalidFiniteField { field, value });
    }

    Ok(())
}

fn validate_positive_finite(
    field: &'static str,
    value: f64,
) -> Result<(), VanillaOptionGridBatchPricingRequestError> {
    if !value.is_finite() || value <= 0.0 {
        return Err(
            VanillaOptionGridBatchPricingRequestError::InvalidPositiveField { field, value },
        );
    }

    Ok(())
}
