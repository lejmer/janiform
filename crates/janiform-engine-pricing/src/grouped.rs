use std::error::Error;
use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};

use janiform_affine_oracle::affine_oracle::ExponentialAffineTransformIntegrationResult;
use janiform_model_kernels::nig::{
    NigBuySellDriverParameters, NigSharedPlusShiftConfigurationError,
};

use super::contracts::{
    AnchoredPricingRecursionIntervalDiagnostics, AnchoredVanillaOptionPricingBoundaryInput,
    VanillaOptionPayoffConfiguration, VanillaOptionPayoffType, VanillaOptionPricingMarketInputs,
    VanillaOptionPricingModelParameters, VanillaOptionPricingOptionalOutputSelection,
    VanillaOptionPricingRequest, VanillaOptionPricingRequestError,
};
use super::forward_law::{
    ForwardLawTerminalSnapshot, price_terminal_snapshot_strikes, solve_forward_law_scope,
};
use super::fourier::{FourierInversionDiagnostics, FourierInversionEngineSettings};
use super::implied_volatility::VanillaOptionImpliedVolatilityInversionStatus;
use super::progress::{PricingProgressEvent, emit_progress_event};
use super::runner::{
    VanillaOptionPricingExecutionInput, VanillaOptionPricingIntervalFactors,
    VanillaOptionPricingRunError,
    run_vanilla_option_pricing_with_observer,
};
use super::thread_pool::with_pricing_thread_pool;

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
    pub expected_hawkes_boundary_state_before: janiform_model_kernels::hawkes::HawkesExcitationState,
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
pub struct VanillaOptionGroupedBatchGroupRequest {
    pub payoff_type: VanillaOptionPayoffType,
    pub risk_free_rate: f64,
    pub dividend_yield: f64,
    pub strikes: Vec<f64>,
    pub maturity_intervals: Vec<f64>,
}

impl VanillaOptionGroupedBatchGroupRequest {
    pub fn validate(&self) -> Result<(), VanillaOptionGroupedBatchPricingRequestError> {
        if self.strikes.is_empty() {
            return Err(VanillaOptionGroupedBatchPricingRequestError::EmptyStrikeAxis);
        }
        if self.maturity_intervals.is_empty() {
            return Err(VanillaOptionGroupedBatchPricingRequestError::EmptyMaturityAxis);
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
pub struct VanillaOptionGroupedBatchPricingRequest {
    pub spot: f64,
    pub groups: Vec<VanillaOptionGroupedBatchGroupRequest>,
}

impl VanillaOptionGroupedBatchPricingRequest {
    pub fn validate(&self) -> Result<(), VanillaOptionGroupedBatchPricingRequestError> {
        validate_positive_finite("spot", self.spot)?;
        if self.groups.is_empty() {
            return Err(VanillaOptionGroupedBatchPricingRequestError::EmptyGroups);
        }
        for group in &self.groups {
            group.validate()?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum VanillaOptionGroupedBatchPricingRequestError {
    EmptyGroups,
    EmptyStrikeAxis,
    EmptyMaturityAxis,
    InvalidFiniteField { field: &'static str, value: f64 },
    InvalidPositiveField { field: &'static str, value: f64 },
}

impl Display for VanillaOptionGroupedBatchPricingRequestError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyGroups => write!(formatter, "grouped batch pricing request must contain at least one group"),
            Self::EmptyStrikeAxis => write!(formatter, "pricing group must contain at least one strike"),
            Self::EmptyMaturityAxis => write!(formatter, "pricing group must contain at least one maturity_interval"),
            Self::InvalidFiniteField { field, value } => {
                write!(formatter, "{field} values must be finite, found {value}")
            }
            Self::InvalidPositiveField { field, value } => {
                write!(formatter, "{field} values must be finite and strictly positive, found {value}")
            }
        }
    }
}

impl Error for VanillaOptionGroupedBatchPricingRequestError {}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VanillaOptionGroupedBatchPricingExecutionInput {
    pub request: VanillaOptionGroupedBatchPricingRequest,
    pub boundary_input: AnchoredVanillaOptionPricingBoundaryInput,
    pub model_parameters: VanillaOptionPricingModelParameters,
    pub interval_delta: f64,
    pub fourier_inversion_settings: Option<FourierInversionEngineSettings>,
    pub interval_factors: Vec<VanillaOptionPricingIntervalFactors>,
    pub optional_output_selection: VanillaOptionPricingOptionalOutputSelection,
    pub cpu_count: Option<usize>,
}

impl VanillaOptionGroupedBatchPricingExecutionInput {
    pub fn validate(&self) -> Result<(), VanillaOptionGroupedBatchPricingExecutionInputError> {
        self.request
            .validate()
            .map_err(|source| VanillaOptionGroupedBatchPricingExecutionInputError::InvalidRequest {
                source,
            })?;
        self.boundary_input.validate().map_err(|source| {
            VanillaOptionGroupedBatchPricingExecutionInputError::InvalidBoundaryInput { source }
        })?;
        self.model_parameters.validate().map_err(|source| {
            VanillaOptionGroupedBatchPricingExecutionInputError::InvalidModelParameters { source }
        })?;
        validate_positive_finite("interval_delta", self.interval_delta).map_err(|source| {
            VanillaOptionGroupedBatchPricingExecutionInputError::InvalidRequest { source }
        })?;
        if self.boundary_input.interval_delta != self.interval_delta {
            return Err(
                VanillaOptionGroupedBatchPricingExecutionInputError::BoundaryIntervalDeltaMismatch {
                    request_interval_delta: self.interval_delta,
                    boundary_interval_delta: self.boundary_input.interval_delta,
                },
            );
        }
        if self.interval_factors.is_empty() {
            return Err(VanillaOptionGroupedBatchPricingExecutionInputError::EmptyIntervalFactors);
        }
        for interval_factor in &self.interval_factors {
            interval_factor.validate().map_err(|source| {
                VanillaOptionGroupedBatchPricingExecutionInputError::InvalidIntervalFactors {
                    source,
                }
            })?;
        }
        if let Some(cpu_count) = self.cpu_count {
            if cpu_count == 0 {
                return Err(VanillaOptionGroupedBatchPricingExecutionInputError::InvalidCpuCount {
                    value: cpu_count,
                });
            }
        }
        if let Some(settings) = self.fourier_inversion_settings {
            settings.validate().map_err(|source| {
                VanillaOptionGroupedBatchPricingExecutionInputError::InvalidFourierInversionSettings {
                    source,
                }
            })?;
        }

        let max_interval_count = self
            .request
            .groups
            .iter()
            .flat_map(|group| group.maturity_intervals.iter())
            .map(|maturity_interval| (*maturity_interval / self.interval_delta) as usize)
            .max()
            .unwrap_or(0);
        if self.interval_factors.len() != max_interval_count {
            return Err(
                VanillaOptionGroupedBatchPricingExecutionInputError::IntervalFactorShapeMismatch {
                    expected_interval_factor_count: max_interval_count,
                    found_interval_factor_count: self.interval_factors.len(),
                },
            );
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum VanillaOptionGroupedBatchPricingExecutionInputError {
    InvalidRequest { source: VanillaOptionGroupedBatchPricingRequestError },
    InvalidBoundaryInput { source: super::contracts::AnchoredVanillaOptionPricingBoundaryInputError },
    InvalidModelParameters { source: super::contracts::VanillaOptionPricingModelParametersError },
    InvalidFourierInversionSettings { source: super::fourier::FourierInversionEngineSettingsError },
    InvalidIntervalFactors { source: super::runner::VanillaOptionPricingIntervalFactorsError },
    BoundaryIntervalDeltaMismatch { request_interval_delta: f64, boundary_interval_delta: f64 },
    EmptyIntervalFactors,
    IntervalFactorShapeMismatch { expected_interval_factor_count: usize, found_interval_factor_count: usize },
    InvalidCpuCount { value: usize },
}

impl Display for VanillaOptionGroupedBatchPricingExecutionInputError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidRequest { source } => write!(formatter, "grouped batch pricing request is invalid: {source}"),
            Self::InvalidBoundaryInput { source } => write!(formatter, "grouped batch pricing boundary_input is invalid: {source}"),
            Self::InvalidModelParameters { source } => write!(formatter, "grouped batch pricing model_parameters are invalid: {source}"),
            Self::InvalidFourierInversionSettings { source } => write!(formatter, "grouped batch pricing fourier_inversion_settings are invalid: {source}"),
            Self::InvalidIntervalFactors { source } => write!(formatter, "grouped batch pricing interval_factors are invalid: {source}"),
            Self::BoundaryIntervalDeltaMismatch { request_interval_delta, boundary_interval_delta } => write!(formatter, "grouped batch pricing interval_delta {request_interval_delta} must equal boundary_input.interval_delta {boundary_interval_delta}"),
            Self::EmptyIntervalFactors => write!(formatter, "grouped batch pricing interval_factors must contain at least one interval"),
            Self::IntervalFactorShapeMismatch { expected_interval_factor_count, found_interval_factor_count } => write!(formatter, "grouped batch pricing interval_factors must contain exactly {expected_interval_factor_count} entries implied by the maximum maturity, found {found_interval_factor_count}"),
            Self::InvalidCpuCount { value } => write!(formatter, "grouped batch pricing cpu_count must be strictly positive when provided, found {value}"),
        }
    }
}

impl Error for VanillaOptionGroupedBatchPricingExecutionInputError {}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VanillaOptionGroupedBatchPricingGroupResult {
    pub payoff_type: VanillaOptionPayoffType,
    pub risk_free_rate: f64,
    pub dividend_yield: f64,
    pub strikes: Vec<f64>,
    pub maturity_intervals: Vec<f64>,
    pub price_grid: Vec<Vec<f64>>,
    pub implied_volatility_grid: Vec<Vec<Option<f64>>>,
    pub implied_volatility_inversion_status_grid: Vec<Vec<VanillaOptionImpliedVolatilityInversionStatus>>,
    pub forward_price_grid: Vec<Vec<f64>>,
    pub discount_factor_grid: Vec<Vec<f64>>,
    pub boundary_state_norm_grid: Vec<Vec<f64>>,
    pub maturity_summaries: Vec<VanillaOptionMaturitySummary>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VanillaOptionGroupedBatchPricingRunResult {
    pub spot: f64,
    pub groups: Vec<VanillaOptionGroupedBatchPricingGroupResult>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum VanillaOptionGroupedBatchPricingRunError {
    InvalidExecutionInput { source: VanillaOptionGroupedBatchPricingExecutionInputError },
    InvalidDriverParameters { source: NigSharedPlusShiftConfigurationError },
    NodePricingFailure { message: String },
}

impl Display for VanillaOptionGroupedBatchPricingRunError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidExecutionInput { source } => write!(formatter, "grouped batch pricing execution input is invalid: {source}"),
            Self::InvalidDriverParameters { source } => write!(formatter, "grouped batch pricing driver parameters are invalid: {source}"),
            Self::NodePricingFailure { message } => write!(formatter, "grouped batch pricing failed: {message}"),
        }
    }
}

impl Error for VanillaOptionGroupedBatchPricingRunError {
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
    pub spot: f64,
    pub strikes: Vec<f64>,
    pub maturity_intervals: Vec<f64>,
    pub payoff_type: VanillaOptionPayoffType,
    pub risk_free_rate: f64,
    pub dividend_yield: f64,
}

impl VanillaOptionSurfacePricingRequest {
    pub fn validate(&self) -> Result<(), VanillaOptionGroupedBatchPricingRequestError> {
        VanillaOptionGroupedBatchPricingRequest {
            spot: self.spot,
            groups: vec![VanillaOptionGroupedBatchGroupRequest {
                payoff_type: self.payoff_type,
                risk_free_rate: self.risk_free_rate,
                dividend_yield: self.dividend_yield,
                strikes: self.strikes.clone(),
                maturity_intervals: self.maturity_intervals.clone(),
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
    pub interval_delta: f64,
    pub fourier_inversion_settings: Option<FourierInversionEngineSettings>,
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
    pub price_grid: Vec<Vec<f64>>,
    pub implied_volatility_grid: Vec<Vec<Option<f64>>>,
    pub implied_volatility_inversion_status_grid: Vec<Vec<VanillaOptionImpliedVolatilityInversionStatus>>,
    pub forward_price_grid: Vec<Vec<f64>>,
    pub discount_factor_grid: Vec<Vec<f64>>,
    pub boundary_state_norm_grid: Vec<Vec<f64>>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VanillaOptionSurfacePricingRunResult {
    pub response: VanillaOptionSurfacePricingResponse,
    pub maturity_summaries: Vec<VanillaOptionMaturitySummary>,
}

pub type VanillaOptionSurfacePricingExecutionInputError = VanillaOptionGroupedBatchPricingExecutionInputError;

#[derive(Debug, Clone, PartialEq)]
pub enum VanillaOptionSurfacePricingRunError {
    InvalidExecutionInput { source: VanillaOptionSurfacePricingExecutionInputError },
    InvalidDriverParameters { source: NigSharedPlusShiftConfigurationError },
    NodePricingFailure { message: String },
}

impl Display for VanillaOptionSurfacePricingRunError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidExecutionInput { source } => write!(formatter, "surface pricing execution input is invalid: {source}"),
            Self::InvalidDriverParameters { source } => write!(formatter, "surface pricing driver parameters are invalid: {source}"),
            Self::NodePricingFailure { message } => write!(formatter, "surface pricing failed: {message}"),
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
        VanillaOptionGroupedBatchPricingExecutionInput {
            request: VanillaOptionGroupedBatchPricingRequest {
                spot: self.request.spot,
                groups: vec![VanillaOptionGroupedBatchGroupRequest {
                    payoff_type: self.request.payoff_type,
                    risk_free_rate: self.request.risk_free_rate,
                    dividend_yield: self.request.dividend_yield,
                    strikes: self.request.strikes.clone(),
                    maturity_intervals: self.request.maturity_intervals.clone(),
                }],
            },
            boundary_input: self.boundary_input.clone(),
            model_parameters: self.model_parameters.clone(),
            interval_delta: self.interval_delta,
            fourier_inversion_settings: self.fourier_inversion_settings,
            interval_factors: self.interval_factors.clone(),
            optional_output_selection: self.optional_output_selection,
            cpu_count: self.cpu_count,
        }
        .validate()
    }
}

pub fn run_vanilla_option_grouped_batch_pricing(
    execution_input: VanillaOptionGroupedBatchPricingExecutionInput,
) -> Result<VanillaOptionGroupedBatchPricingRunResult, VanillaOptionGroupedBatchPricingRunError> {
    run_vanilla_option_grouped_batch_pricing_with_observer(execution_input, None)
}

pub fn run_vanilla_option_grouped_batch_pricing_with_observer(
    execution_input: VanillaOptionGroupedBatchPricingExecutionInput,
    mut observer: Option<&mut dyn FnMut(PricingProgressEvent)>,
) -> Result<VanillaOptionGroupedBatchPricingRunResult, VanillaOptionGroupedBatchPricingRunError> {
    execution_input
        .validate()
        .map_err(|source| VanillaOptionGroupedBatchPricingRunError::InvalidExecutionInput {
            source,
        })?;

    let total_groups = execution_input.request.groups.len();
    emit_progress_event(
        &mut observer,
        PricingProgressEvent::group_progress(0, total_groups, Some(0)),
    );

    let driver_parameters = execution_input
        .model_parameters
        .nig_shared_plus_shift_configuration
        .derive_driver_parameters()
        .map_err(|source| VanillaOptionGroupedBatchPricingRunError::InvalidDriverParameters {
            source,
        })?;

    let mut groups = Vec::with_capacity(total_groups);

    if observer.is_some() {
        run_grouped_batch_body(
            &execution_input,
            total_groups,
            driver_parameters,
            &mut observer,
            &mut groups,
        )?;
    } else {
        with_pricing_thread_pool(execution_input.cpu_count, || {
            let mut observer = None;
            run_grouped_batch_body(
                &execution_input,
                total_groups,
                driver_parameters,
                &mut observer,
                &mut groups,
            )
        })?;
    }

    Ok(VanillaOptionGroupedBatchPricingRunResult {
        spot: execution_input.request.spot,
        groups,
    })
}

fn run_grouped_batch_body(
    execution_input: &VanillaOptionGroupedBatchPricingExecutionInput,
    total_groups: usize,
    driver_parameters: NigBuySellDriverParameters,
    observer: &mut Option<&mut dyn FnMut(PricingProgressEvent)>,
    groups: &mut Vec<VanillaOptionGroupedBatchPricingGroupResult>,
) -> Result<(), VanillaOptionGroupedBatchPricingRunError> {
    for (group_index, group) in execution_input.request.groups.iter().enumerate() {
        let group_result = run_homogeneous_scope(
            execution_input.request.spot,
            group,
            execution_input.boundary_input.clone(),
            execution_input.model_parameters.clone(),
            execution_input.interval_delta,
            execution_input.fourier_inversion_settings,
            execution_input.interval_factors.as_slice(),
            execution_input.optional_output_selection,
            driver_parameters,
            observer,
        )
        .map_err(|source| VanillaOptionGroupedBatchPricingRunError::NodePricingFailure {
            message: source,
        })?;

        groups.push(group_result);
        let next_group_index = if group_index + 1 < total_groups {
            Some(group_index + 1)
        } else {
            None
        };
        emit_progress_event(
            observer,
            PricingProgressEvent::group_progress(group_index + 1, total_groups, next_group_index),
        );
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
        .map_err(|source| VanillaOptionSurfacePricingRunError::InvalidExecutionInput {
            source,
        })?;

    let grouped_result = run_vanilla_option_grouped_batch_pricing_with_observer(
        VanillaOptionGroupedBatchPricingExecutionInput {
            request: VanillaOptionGroupedBatchPricingRequest {
                spot: execution_input.request.spot,
                groups: vec![VanillaOptionGroupedBatchGroupRequest {
                    payoff_type: execution_input.request.payoff_type,
                    risk_free_rate: execution_input.request.risk_free_rate,
                    dividend_yield: execution_input.request.dividend_yield,
                    strikes: execution_input.request.strikes.clone(),
                    maturity_intervals: execution_input.request.maturity_intervals.clone(),
                }],
            },
            boundary_input: execution_input.boundary_input,
            model_parameters: execution_input.model_parameters,
            interval_delta: execution_input.interval_delta,
            fourier_inversion_settings: execution_input.fourier_inversion_settings,
            interval_factors: execution_input.interval_factors,
            optional_output_selection: execution_input.optional_output_selection,
            cpu_count: execution_input.cpu_count,
        },
        observer,
    )
    .map_err(|source| match source {
        VanillaOptionGroupedBatchPricingRunError::InvalidExecutionInput { source } => {
            VanillaOptionSurfacePricingRunError::InvalidExecutionInput { source }
        }
        VanillaOptionGroupedBatchPricingRunError::InvalidDriverParameters { source } => {
            VanillaOptionSurfacePricingRunError::InvalidDriverParameters { source }
        }
        VanillaOptionGroupedBatchPricingRunError::NodePricingFailure { message } => {
            VanillaOptionSurfacePricingRunError::NodePricingFailure { message }
        }
    })?;

    let group = grouped_result.groups.into_iter().next().ok_or(
        VanillaOptionSurfacePricingRunError::NodePricingFailure {
            message: "missing surface pricing group result".to_string(),
        },
    )?;

    Ok(VanillaOptionSurfacePricingRunResult {
        response: VanillaOptionSurfacePricingResponse {
            spot: grouped_result.spot,
            strikes: group.strikes,
            maturity_intervals: group.maturity_intervals,
            payoff_type: group.payoff_type,
            risk_free_rate: group.risk_free_rate,
            dividend_yield: group.dividend_yield,
            price_grid: group.price_grid,
            implied_volatility_grid: group.implied_volatility_grid,
            implied_volatility_inversion_status_grid: group.implied_volatility_inversion_status_grid,
            forward_price_grid: group.forward_price_grid,
            discount_factor_grid: group.discount_factor_grid,
            boundary_state_norm_grid: group.boundary_state_norm_grid,
        },
        maturity_summaries: group.maturity_summaries,
    })
}

fn run_homogeneous_scope(
    spot: f64,
    group: &VanillaOptionGroupedBatchGroupRequest,
    boundary_input: AnchoredVanillaOptionPricingBoundaryInput,
    model_parameters: VanillaOptionPricingModelParameters,
    interval_delta: f64,
    fourier_inversion_settings: Option<FourierInversionEngineSettings>,
    interval_factors: &[VanillaOptionPricingIntervalFactors],
    optional_output_selection: VanillaOptionPricingOptionalOutputSelection,
    driver_parameters: NigBuySellDriverParameters,
    observer: &mut Option<&mut dyn FnMut(PricingProgressEvent)>,
) -> Result<VanillaOptionGroupedBatchPricingGroupResult, String> {
    let maturity_interval_counts = group
        .maturity_intervals
        .iter()
        .map(|maturity_interval| {
            pricing_interval_count_from_maturity_interval(*maturity_interval, interval_delta)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut price_grid = Vec::with_capacity(group.maturity_intervals.len());
    let mut implied_volatility_grid = Vec::with_capacity(group.maturity_intervals.len());
    let mut implied_volatility_inversion_status_grid = Vec::with_capacity(group.maturity_intervals.len());
    let mut forward_price_grid = Vec::with_capacity(group.maturity_intervals.len());
    let mut discount_factor_grid = Vec::with_capacity(group.maturity_intervals.len());
    let mut boundary_state_norm_grid = Vec::with_capacity(group.maturity_intervals.len());
    let mut maturity_summaries = Vec::with_capacity(group.maturity_intervals.len());

    let max_interval_count = maturity_interval_counts.iter().copied().max().unwrap_or(0);
    let base_request = base_scope_request(
        spot,
        group.risk_free_rate,
        group.dividend_yield,
        group.payoff_type,
        max_interval_count as f64 * interval_delta,
        boundary_input.clone(),
        model_parameters.clone(),
        interval_delta,
        fourier_inversion_settings,
    )
    .map_err(|source| source.to_string())?;

    let filtered_multi_interval_counts = maturity_interval_counts
        .iter()
        .copied()
        .filter(|interval_count| *interval_count > 1)
        .collect::<Vec<_>>();

    let forward_law_solution = if filtered_multi_interval_counts.is_empty() {
        None
    } else {
        Some(
            solve_forward_law_scope(
                &base_request,
                group.strikes.as_slice(),
                filtered_multi_interval_counts.as_slice(),
                &interval_factors[..max_interval_count],
                driver_parameters,
                observer,
            )
            .map_err(|source| source.to_string())?,
        )
    };

    for (maturity_index, maturity_interval_count) in maturity_interval_counts.iter().enumerate() {
        emit_progress_event(
            observer,
            PricingProgressEvent::maturity_progress(
                maturity_index,
                group.maturity_intervals.len(),
                Some(maturity_index),
                group.maturity_intervals.len(),
            ),
        );

        if *maturity_interval_count == 1 {
            let direct_row = price_direct_fourier_row(
                spot,
                group,
                maturity_index,
                boundary_input.clone(),
                model_parameters.clone(),
                interval_delta,
                fourier_inversion_settings,
                optional_output_selection,
                interval_factors[0],
                observer,
            )
            .map_err(|source| source.to_string())?;
            price_grid.push(direct_row.price_row);
            implied_volatility_grid.push(direct_row.implied_volatility_row);
            implied_volatility_inversion_status_grid.push(direct_row.status_row);
            forward_price_grid.push(direct_row.forward_price_row);
            discount_factor_grid.push(direct_row.discount_factor_row);
            boundary_state_norm_grid.push(direct_row.boundary_state_norm_row);
            maturity_summaries.push(direct_row.maturity_summary);
            continue;
        }

        let terminal_snapshot = forward_law_solution
            .as_ref()
            .and_then(|solution| {
                solution.terminal_snapshots.iter().find(|snapshot| {
                    snapshot.maturity_interval == group.maturity_intervals[maturity_index]
                })
            })
            .ok_or_else(|| {
                format!(
                    "missing forward-law terminal snapshot for maturity interval {}",
                    group.maturity_intervals[maturity_index]
                )
            })?;
        let priced_strikes = price_terminal_snapshot_strikes(
            &base_request,
            terminal_snapshot,
            group.strikes.as_slice(),
            maturity_index,
            group.maturity_intervals.len(),
            observer,
        );

        price_grid.push(priced_strikes.iter().map(|node| node.price).collect());
        implied_volatility_grid.push(
            priced_strikes
                .iter()
                .map(|node| node.implied_volatility_outcome.implied_volatility)
                .collect(),
        );
        implied_volatility_inversion_status_grid.push(
            priced_strikes
                .iter()
                .map(|node| node.implied_volatility_outcome.inversion_status)
                .collect(),
        );
        forward_price_grid.push(priced_strikes.iter().map(|node| node.forward_price).collect());
        discount_factor_grid.push(priced_strikes.iter().map(|node| node.discount_factor).collect());
        boundary_state_norm_grid.push(
            priced_strikes
                .iter()
                .map(|node| node.boundary_state_norm)
                .collect(),
        );
        maturity_summaries.push(maturity_summary_from_terminal_snapshot(
            terminal_snapshot,
            optional_output_selection,
        ));
    }

    emit_progress_event(
        observer,
        PricingProgressEvent::maturity_progress(
            group.maturity_intervals.len(),
            group.maturity_intervals.len(),
            None,
            group.maturity_intervals.len(),
        ),
    );

    Ok(VanillaOptionGroupedBatchPricingGroupResult {
        payoff_type: group.payoff_type,
        risk_free_rate: group.risk_free_rate,
        dividend_yield: group.dividend_yield,
        strikes: group.strikes.clone(),
        maturity_intervals: group.maturity_intervals.clone(),
        price_grid,
        implied_volatility_grid,
        implied_volatility_inversion_status_grid,
        forward_price_grid,
        discount_factor_grid,
        boundary_state_norm_grid,
        maturity_summaries,
    })
}

struct DirectFourierRow {
    price_row: Vec<f64>,
    implied_volatility_row: Vec<Option<f64>>,
    status_row: Vec<VanillaOptionImpliedVolatilityInversionStatus>,
    forward_price_row: Vec<f64>,
    discount_factor_row: Vec<f64>,
    boundary_state_norm_row: Vec<f64>,
    maturity_summary: VanillaOptionMaturitySummary,
}

fn price_direct_fourier_row(
    spot: f64,
    group: &VanillaOptionGroupedBatchGroupRequest,
    maturity_index: usize,
    boundary_input: AnchoredVanillaOptionPricingBoundaryInput,
    model_parameters: VanillaOptionPricingModelParameters,
    interval_delta: f64,
    fourier_inversion_settings: Option<FourierInversionEngineSettings>,
    optional_output_selection: VanillaOptionPricingOptionalOutputSelection,
    interval_factor: VanillaOptionPricingIntervalFactors,
    observer: &mut Option<&mut dyn FnMut(PricingProgressEvent)>,
) -> Result<DirectFourierRow, VanillaOptionPricingRunError> {
    let mut price_row = Vec::with_capacity(group.strikes.len());
    let mut implied_volatility_row = Vec::with_capacity(group.strikes.len());
    let mut status_row = Vec::with_capacity(group.strikes.len());
    let mut forward_price_row = Vec::with_capacity(group.strikes.len());
    let mut discount_factor_row = Vec::with_capacity(group.strikes.len());
    let mut boundary_state_norm_row = Vec::with_capacity(group.strikes.len());
    let mut representative_transform = None;
    let mut integration_quality = None;
    let mut interval_summaries = Vec::new();

    let internal_output_selection = VanillaOptionPricingOptionalOutputSelection::new(true, true, false);

    for (strike_index, strike) in group.strikes.iter().enumerate() {
        emit_progress_event(
            observer,
            PricingProgressEvent::strike_pricing_progress(
                strike_index,
                group.strikes.len(),
                Some(maturity_index),
                Some(group.maturity_intervals.len()),
                Some(strike_index),
                group.strikes.len(),
            ),
        );

        let mut request = VanillaOptionPricingRequest::new_with_interval_delta(
            VanillaOptionPricingMarketInputs::new(
                spot,
                *strike,
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
        if let Some(settings) = fourier_inversion_settings {
            request = request.with_fourier_inversion_settings(settings);
        }

        let run_result = run_vanilla_option_pricing_with_observer(
            VanillaOptionPricingExecutionInput::new(
                request,
                vec![interval_factor],
                internal_output_selection,
            ),
            None,
        )?;

        price_row.push(run_result.response.price);
        implied_volatility_row.push(run_result.response.implied_volatility);
        status_row.push(run_result.response.implied_volatility_inversion_status);
        forward_price_row.push(run_result.response.diagnostics.forward_price);
        discount_factor_row.push(run_result.response.diagnostics.discount_factor);
        boundary_state_norm_row.push(run_result.response.diagnostics.boundary_state_norm);

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

    emit_progress_event(
        observer,
        PricingProgressEvent::strike_pricing_progress(
            group.strikes.len(),
            group.strikes.len(),
            Some(maturity_index),
            Some(group.maturity_intervals.len()),
            None,
            group.strikes.len(),
        ),
    );

    Ok(DirectFourierRow {
        price_row,
        implied_volatility_row,
        status_row,
        forward_price_row,
        discount_factor_row,
        boundary_state_norm_row,
        maturity_summary: VanillaOptionMaturitySummary {
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
    fourier_inversion_settings: Option<FourierInversionEngineSettings>,
) -> Result<VanillaOptionPricingRequest, super::contracts::VanillaOptionPricingRequestError> {
    let mut request = VanillaOptionPricingRequest::try_new_with_interval_delta(
        VanillaOptionPricingMarketInputs::try_new(
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
    )?;
    if let Some(settings) = fourier_inversion_settings {
        request = request.with_fourier_inversion_settings(settings);
    }

    Ok(request)
}

fn pricing_interval_count_from_maturity_interval(
    maturity_interval: f64,
    interval_delta: f64,
) -> Result<usize, String> {
    let interval_count_float = maturity_interval / interval_delta;
    let interval_count = interval_count_float as usize;
    if interval_count == 0 || interval_count as f64 != interval_count_float {
        return Err(format!(
            "maturity_interval {maturity_interval} must be a positive exact multiple of interval_delta {interval_delta}"
        ));
    }
    Ok(interval_count)
}

fn validate_finite(
    field: &'static str,
    value: f64,
) -> Result<(), VanillaOptionGroupedBatchPricingRequestError> {
    if !value.is_finite() {
        return Err(VanillaOptionGroupedBatchPricingRequestError::InvalidFiniteField {
            field,
            value,
        });
    }
    Ok(())
}

fn validate_positive_finite(
    field: &'static str,
    value: f64,
) -> Result<(), VanillaOptionGroupedBatchPricingRequestError> {
    if !value.is_finite() || value <= 0.0 {
        return Err(VanillaOptionGroupedBatchPricingRequestError::InvalidPositiveField {
            field,
            value,
        });
    }
    Ok(())
}
