use std::collections::BTreeMap;
use std::error::Error;
use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};

use janiform_affine_oracle::affine_oracle::ExponentialAffineTransformIntegrationResult;
use janiform_model_kernels::nig::NigSharedPlusShiftConfigurationError;

use super::contracts::{
    AnchoredVanillaOptionPricingBoundaryInput, SpectralPricingIntervalDiagnostics,
    VanillaOptionPayoffConfiguration, VanillaOptionPayoffType, VanillaOptionPricingFailureStage,
    VanillaOptionPricingMarketInputs, VanillaOptionPricingModelParameters,
    VanillaOptionPricingOptionalOutputSelection, VanillaOptionPricingStatus,
};
use super::fourier::FourierInversionDiagnostics;
use super::implied_volatility::VanillaOptionImpliedVolatilityInversionStatus;
use super::interval_schedule::PricingIntervalSchedule;
use super::progress::PricingProgressEvent;
use super::runner::{
    VanillaOptionPricingIntervalFactors, VanillaOptionPricingIntervalFactorsError,
};

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VanillaOptionMaturitySummary {
    pub maturity_interval: f64,
    pub terminal_anchor_value: f64,
    pub deviation_domain_lower_bound: f64,
    pub deviation_domain_upper_bound: f64,
    pub collocation_node_count: usize,
    pub collocation_nodes: Vec<f64>,
    pub interval_summaries: Vec<SpectralPricingIntervalDiagnostics>,
    pub representative_transform: Option<ExponentialAffineTransformIntegrationResult>,
    pub integration_quality: Option<FourierInversionDiagnostics>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct VanillaOptionNodePriceSensitivity {
    pub alpha_derivative: f64,
    pub beta_0_derivative: f64,
    pub delta_derivative: f64,
    pub delta_beta_derivative: f64,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spectral_collocation_node_count_override: Option<usize>,
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

        if self.interval_factors.len() != self.runtime_interval_schedule.interval_count() {
            return Err(
                VanillaOptionGridBatchPricingExecutionInputError::IntervalFactorShapeMismatch {
                    expected_interval_factor_count: self.runtime_interval_schedule.interval_count(),
                    found_interval_factor_count: self.interval_factors.len(),
                },
            );
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

        if let Some(collocation_node_count) = self.spectral_collocation_node_count_override {
            if collocation_node_count == 0 {
                return Err(
                    VanillaOptionGridBatchPricingExecutionInputError::InvalidCollocationNodeCount {
                        value: collocation_node_count,
                    },
                );
            }
        }

        for grid in &self.request.grids {
            for maturity_interval in &grid.maturity_intervals {
                self.runtime_interval_schedule
                    .terminal_interval_index_for_maturity_interval(*maturity_interval)
                    .map_err(
                        |source| VanillaOptionGridBatchPricingExecutionInputError::InvalidRuntimeIntervalSchedule {
                            message: source.to_string(),
                        },
                    )?;
            }
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
        source: VanillaOptionPricingIntervalFactorsError,
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
    InvalidCollocationNodeCount {
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
                "grid batch pricing interval_factors must contain exactly {expected_interval_factor_count} entries implied by the runtime schedule, found {found_interval_factor_count}"
            ),
            Self::InvalidCpuCount { value } => write!(
                formatter,
                "grid batch pricing cpu_count must be strictly positive when provided, found {value}"
            ),
            Self::InvalidCollocationNodeCount { value } => write!(
                formatter,
                "grid batch pricing spectral_collocation_node_count_override must be strictly positive when provided, found {value}"
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
    #[serde(skip_serializing)]
    pub node_price_sensitivity_grid: Option<Vec<Vec<Option<VanillaOptionNodePriceSensitivity>>>>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spectral_collocation_node_count_override: Option<usize>,
    pub cpu_count: Option<usize>,
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
            spectral_collocation_node_count_override: self.spectral_collocation_node_count_override,
            cpu_count: self.cpu_count,
        }
        .validate()
    }
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

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SpectralThetaWarmStartState {
    row_theta_states_by_carry_group: BTreeMap<(u64, u64), Vec<Vec<SpectralThetaWarmStartRowState>>>,
    last_driver_configuration_by_carry_group:
        BTreeMap<(u64, u64), SpectralThetaWarmStartDriverConfiguration>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SpectralThetaWarmStartDriverConfiguration {
    pub(crate) alpha: f64,
    pub(crate) beta_0: f64,
    pub(crate) delta: f64,
    pub(crate) mu: f64,
    pub(crate) delta_beta: f64,
}

impl SpectralThetaWarmStartDriverConfiguration {
    fn parameter_delta_from(self, previous: Self) -> SpectralThetaWarmStartSensitivity {
        SpectralThetaWarmStartSensitivity {
            alpha_derivative: self.alpha - previous.alpha,
            beta_0_derivative: self.beta_0 - previous.beta_0,
            delta_derivative: self.delta - previous.delta,
            mu_derivative: self.mu - previous.mu,
            delta_beta_derivative: self.delta_beta - previous.delta_beta,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SpectralThetaWarmStartSensitivity {
    pub(crate) alpha_derivative: f64,
    pub(crate) beta_0_derivative: f64,
    pub(crate) delta_derivative: f64,
    pub(crate) mu_derivative: f64,
    pub(crate) delta_beta_derivative: f64,
}

impl SpectralThetaWarmStartSensitivity {
    fn predicted_theta_shift(self, parameter_delta: Self) -> f64 {
        self.alpha_derivative.mul_add(
            parameter_delta.alpha_derivative,
            self.beta_0_derivative.mul_add(
                parameter_delta.beta_0_derivative,
                self.delta_derivative.mul_add(
                    parameter_delta.delta_derivative,
                    self.mu_derivative.mul_add(
                        parameter_delta.mu_derivative,
                        self.delta_beta_derivative * parameter_delta.delta_beta_derivative,
                    ),
                ),
            ),
        )
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct SpectralThetaWarmStartRowState {
    pub(crate) theta_hint: Option<f64>,
    pub(crate) local_theta_sensitivity: Option<SpectralThetaWarmStartSensitivity>,
}

impl SpectralThetaWarmStartRowState {
    pub(crate) fn from_theta_hint(theta_hint: f64) -> Self {
        Self {
            theta_hint: Some(theta_hint),
            local_theta_sensitivity: None,
        }
    }

    pub(crate) fn with_local_theta_sensitivity(
        mut self,
        local_theta_sensitivity: SpectralThetaWarmStartSensitivity,
    ) -> Self {
        self.local_theta_sensitivity = Some(local_theta_sensitivity);
        self
    }

    fn predicted_theta(
        self,
        parameter_delta: Option<SpectralThetaWarmStartSensitivity>,
    ) -> Option<f64> {
        let theta_hint = self.theta_hint?;

        let Some(parameter_delta) = parameter_delta else {
            return Some(theta_hint);
        };

        let Some(local_theta_sensitivity) = self.local_theta_sensitivity else {
            return Some(theta_hint);
        };

        let predicted_theta =
            theta_hint + local_theta_sensitivity.predicted_theta_shift(parameter_delta);

        if predicted_theta.is_finite() {
            Some(predicted_theta)
        } else {
            Some(theta_hint)
        }
    }
}

impl SpectralThetaWarmStartState {
    pub fn new() -> Self {
        Self::default()
    }

    pub(crate) fn interval_theta_hints(
        &mut self,
        carry_group_key: (u64, u64),
        interval_index: usize,
        row_count: usize,
        current_driver_configuration: SpectralThetaWarmStartDriverConfiguration,
    ) -> Vec<Option<f64>> {
        self.ensure_interval_shape(carry_group_key, interval_index, row_count);

        let parameter_delta = self
            .last_driver_configuration_by_carry_group
            .get(&carry_group_key)
            .copied()
            .map(|previous_driver_configuration| {
                current_driver_configuration.parameter_delta_from(previous_driver_configuration)
            });

        self.row_theta_states_by_carry_group[&carry_group_key][interval_index]
            .iter()
            .map(|row_state| row_state.predicted_theta(parameter_delta))
            .collect()
    }

    pub(crate) fn record_interval_theta_states(
        &mut self,
        carry_group_key: (u64, u64),
        interval_index: usize,
        current_driver_configuration: SpectralThetaWarmStartDriverConfiguration,
        solved_row_states: impl IntoIterator<Item = SpectralThetaWarmStartRowState>,
    ) {
        let solved_row_states = solved_row_states.into_iter().collect::<Vec<_>>();

        self.ensure_interval_shape(carry_group_key, interval_index, solved_row_states.len());

        self.row_theta_states_by_carry_group
            .get_mut(&carry_group_key)
            .expect("carry-group theta warm-start storage should exist")[interval_index] =
            solved_row_states;

        self.last_driver_configuration_by_carry_group
            .insert(carry_group_key, current_driver_configuration);
    }

    fn ensure_interval_shape(
        &mut self,
        carry_group_key: (u64, u64),
        interval_index: usize,
        row_count: usize,
    ) {
        let row_theta_states_by_interval = self
            .row_theta_states_by_carry_group
            .entry(carry_group_key)
            .or_default();

        if row_theta_states_by_interval.len() <= interval_index {
            row_theta_states_by_interval.resize_with(interval_index + 1, Vec::new);
        }

        if row_theta_states_by_interval[interval_index].len() != row_count {
            row_theta_states_by_interval[interval_index] =
                vec![SpectralThetaWarmStartRowState::default(); row_count];
        }
    }
}

pub fn run_vanilla_option_grid_batch_pricing(
    execution_input: VanillaOptionGridBatchPricingExecutionInput,
) -> Result<VanillaOptionGridBatchPricingRunResult, VanillaOptionGridBatchPricingRunError> {
    run_vanilla_option_grid_batch_pricing_with_observer(execution_input, None)
}

pub fn run_vanilla_option_grid_batch_pricing_with_observer(
    execution_input: VanillaOptionGridBatchPricingExecutionInput,
    observer: Option<&mut dyn FnMut(PricingProgressEvent)>,
) -> Result<VanillaOptionGridBatchPricingRunResult, VanillaOptionGridBatchPricingRunError> {
    super::spectral::run_vanilla_option_grid_batch_pricing_with_observer_and_theta_warm_start_state(
        execution_input,
        observer,
        None,
    )
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
            spectral_collocation_node_count_override: execution_input
                .spectral_collocation_node_count_override,
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
    if !value.is_finite() {
        return Err(VanillaOptionGridBatchPricingRequestError::InvalidFiniteField { field, value });
    }

    if value <= 0.0 {
        return Err(
            VanillaOptionGridBatchPricingRequestError::InvalidPositiveField { field, value },
        );
    }

    Ok(())
}
