use std::error::Error;
use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};

use janiform_affine_oracle::affine_oracle::{
    ExponentialAffineTransformIntegrationResult, ExponentialAffineTransformIntegrationStatus,
};

use super::fourier::{FourierInversionDiagnostics, FourierInversionEngineSettings};
use super::runner::{
    VanillaOptionPricingExecutionInput, VanillaOptionPricingIntervalFactors,
    VanillaOptionPricingRunError, run_vanilla_option_pricing,
};
use super::{
    AnchoredPricingRecursionDiagnostics, AnchoredVanillaOptionPricingBoundaryInput,
    VanillaOptionPayoffConfiguration, VanillaOptionPayoffType,
    VanillaOptionPricingCalibrationHooks, VanillaOptionPricingModelParameters,
    VanillaOptionPricingOptionalOutputSelection, VanillaOptionPricingRequest,
    VanillaOptionPricingResponse, VanillaOptionPricingRunResult,
};

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VanillaOptionBatchPricingRequest {
    pub spot: f64,
    pub strikes: Vec<f64>,
    pub maturity_intervals: Vec<f64>,
    pub payoff_types: Vec<VanillaOptionPayoffType>,
    pub risk_free_rates: Vec<f64>,
    pub dividend_yields: Vec<f64>,
}

impl VanillaOptionBatchPricingRequest {
    pub const fn new(
        spot: f64,
        strikes: Vec<f64>,
        maturity_intervals: Vec<f64>,
        payoff_types: Vec<VanillaOptionPayoffType>,
        risk_free_rates: Vec<f64>,
        dividend_yields: Vec<f64>,
    ) -> Self {
        Self {
            spot,
            strikes,
            maturity_intervals,
            payoff_types,
            risk_free_rates,
            dividend_yields,
        }
    }

    pub fn try_new(
        spot: f64,
        strikes: Vec<f64>,
        maturity_intervals: Vec<f64>,
        payoff_types: Vec<VanillaOptionPayoffType>,
        risk_free_rates: Vec<f64>,
        dividend_yields: Vec<f64>,
    ) -> Result<Self, VanillaOptionBatchPricingRequestError> {
        let request = Self::new(
            spot,
            strikes,
            maturity_intervals,
            payoff_types,
            risk_free_rates,
            dividend_yields,
        );
        request.validate()?;
        Ok(request)
    }

    pub fn validate(&self) -> Result<(), VanillaOptionBatchPricingRequestError> {
        validate_positive_finite("spot", self.spot)?;

        let node_count = self.node_count();
        if node_count == 0 {
            return Err(VanillaOptionBatchPricingRequestError::EmptyNodes);
        }

        validate_aligned_node_field_count(
            "maturity_intervals",
            self.maturity_intervals.len(),
            node_count,
        )?;
        validate_aligned_node_field_count("payoff_types", self.payoff_types.len(), node_count)?;
        validate_aligned_node_field_count(
            "risk_free_rates",
            self.risk_free_rates.len(),
            node_count,
        )?;
        validate_aligned_node_field_count(
            "dividend_yields",
            self.dividend_yields.len(),
            node_count,
        )?;

        for strike in &self.strikes {
            validate_positive_finite("strikes", *strike)?;
        }
        for maturity_interval in &self.maturity_intervals {
            validate_positive_finite("maturity_intervals", *maturity_interval)?;
        }
        for risk_free_rate in &self.risk_free_rates {
            validate_finite("risk_free_rates", *risk_free_rate)?;
        }
        for dividend_yield in &self.dividend_yields {
            validate_finite("dividend_yields", *dividend_yield)?;
        }

        Ok(())
    }

    pub fn node_count(&self) -> usize {
        self.strikes.len()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum VanillaOptionBatchPricingRequestError {
    EmptyNodes,
    InvalidAlignedFieldCount {
        field: &'static str,
        expected: usize,
        found: usize,
    },
    InvalidFiniteField {
        field: &'static str,
        value: f64,
    },
    InvalidPositiveField {
        field: &'static str,
        value: f64,
    },
}

impl Display for VanillaOptionBatchPricingRequestError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyNodes => write!(
                formatter,
                "batch pricing request must contain at least one option node"
            ),
            Self::InvalidAlignedFieldCount {
                field,
                expected,
                found,
            } => write!(
                formatter,
                "{field} must contain {expected} aligned values, found {found}"
            ),
            Self::InvalidFiniteField { field, value } => {
                write!(formatter, "{field} values must be finite, found {value}")
            }
            Self::InvalidPositiveField { field, value } => write!(
                formatter,
                "{field} values must be finite and strictly positive, found {value}"
            ),
        }
    }
}

impl Error for VanillaOptionBatchPricingRequestError {}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VanillaOptionBatchPricingExecutionInput {
    pub request: VanillaOptionBatchPricingRequest,
    pub boundary_input: AnchoredVanillaOptionPricingBoundaryInput,
    pub model_parameters: VanillaOptionPricingModelParameters,
    pub interval_delta: f64,
    pub fourier_inversion_settings: Option<FourierInversionEngineSettings>,
    pub node_interval_counts: Vec<usize>,
    pub interval_factors: Vec<VanillaOptionPricingIntervalFactors>,
    pub optional_output_selection: VanillaOptionPricingOptionalOutputSelection,
}

impl VanillaOptionBatchPricingExecutionInput {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        request: VanillaOptionBatchPricingRequest,
        boundary_input: AnchoredVanillaOptionPricingBoundaryInput,
        model_parameters: VanillaOptionPricingModelParameters,
        interval_delta: f64,
        fourier_inversion_settings: Option<FourierInversionEngineSettings>,
        node_interval_counts: Vec<usize>,
        interval_factors: Vec<VanillaOptionPricingIntervalFactors>,
        optional_output_selection: VanillaOptionPricingOptionalOutputSelection,
    ) -> Self {
        Self {
            request,
            boundary_input,
            model_parameters,
            interval_delta,
            fourier_inversion_settings,
            node_interval_counts,
            interval_factors,
            optional_output_selection,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn try_new(
        request: VanillaOptionBatchPricingRequest,
        boundary_input: AnchoredVanillaOptionPricingBoundaryInput,
        model_parameters: VanillaOptionPricingModelParameters,
        interval_delta: f64,
        fourier_inversion_settings: Option<FourierInversionEngineSettings>,
        node_interval_counts: Vec<usize>,
        interval_factors: Vec<VanillaOptionPricingIntervalFactors>,
        optional_output_selection: VanillaOptionPricingOptionalOutputSelection,
    ) -> Result<Self, VanillaOptionBatchPricingExecutionInputError> {
        let execution_input = Self::new(
            request,
            boundary_input,
            model_parameters,
            interval_delta,
            fourier_inversion_settings,
            node_interval_counts,
            interval_factors,
            optional_output_selection,
        );

        execution_input.validate()?;

        Ok(execution_input)
    }

    pub fn validate(&self) -> Result<(), VanillaOptionBatchPricingExecutionInputError> {
        self.request.validate().map_err(|source| {
            VanillaOptionBatchPricingExecutionInputError::InvalidRequest { source }
        })?;

        self.boundary_input.validate().map_err(|source| {
            VanillaOptionBatchPricingExecutionInputError::InvalidBoundaryInput { source }
        })?;

        self.model_parameters.validate().map_err(|source| {
            VanillaOptionBatchPricingExecutionInputError::InvalidModelParameters { source }
        })?;

        validate_positive_finite("interval_delta", self.interval_delta).map_err(|source| {
            VanillaOptionBatchPricingExecutionInputError::InvalidIntervalDelta { source }
        })?;

        if self.boundary_input.interval_delta != self.interval_delta {
            return Err(
                VanillaOptionBatchPricingExecutionInputError::BoundaryIntervalDeltaMismatch {
                    request_interval_delta: self.interval_delta,
                    boundary_interval_delta: self.boundary_input.interval_delta,
                },
            );
        }

        if let Some(fourier_inversion_settings) = self.fourier_inversion_settings {
            fourier_inversion_settings.validate().map_err(|source| {
                VanillaOptionBatchPricingExecutionInputError::InvalidFourierInversionSettings {
                    source,
                }
            })?;
        }

        let node_count = self.request.node_count();

        validate_aligned_node_field_count(
            "node_interval_counts",
            self.node_interval_counts.len(),
            node_count,
        )
        .map_err(|source| {
            VanillaOptionBatchPricingExecutionInputError::InvalidNodeIntervalCounts { source }
        })?;

        if self.interval_factors.is_empty() {
            return Err(VanillaOptionBatchPricingExecutionInputError::EmptyIntervalFactors);
        }

        for interval_factor in &self.interval_factors {
            interval_factor.validate().map_err(|source| {
                VanillaOptionBatchPricingExecutionInputError::InvalidIntervalFactors { source }
            })?;
        }

        let mut factor_offset = 0usize;
        for (node_index, (maturity_interval, interval_count)) in self
            .request
            .maturity_intervals
            .iter()
            .zip(self.node_interval_counts.iter())
            .enumerate()
        {
            if *interval_count == 0 {
                return Err(
                    VanillaOptionBatchPricingExecutionInputError::InvalidNodeIntervalCount {
                        node_index,
                        interval_count: *interval_count,
                    },
                );
            }

            factor_offset = factor_offset.saturating_add(*interval_count);
            if factor_offset > self.interval_factors.len() {
                return Err(
                    VanillaOptionBatchPricingExecutionInputError::IntervalFactorShapeMismatch {
                        expected_interval_factor_count: factor_offset,
                        found_interval_factor_count: self.interval_factors.len(),
                    },
                );
            }

            let expected_maturity_interval = self.interval_delta * *interval_count as f64;
            if expected_maturity_interval != *maturity_interval {
                return Err(
                    VanillaOptionBatchPricingExecutionInputError::MaturityIntervalMismatch {
                        node_index,
                        requested_maturity_interval: *maturity_interval,
                        interval_grid_maturity_interval: expected_maturity_interval,
                    },
                );
            }
        }

        if factor_offset != self.interval_factors.len() {
            return Err(
                VanillaOptionBatchPricingExecutionInputError::IntervalFactorShapeMismatch {
                    expected_interval_factor_count: factor_offset,
                    found_interval_factor_count: self.interval_factors.len(),
                },
            );
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum VanillaOptionBatchPricingExecutionInputError {
    InvalidRequest {
        source: VanillaOptionBatchPricingRequestError,
    },
    InvalidBoundaryInput {
        source: super::AnchoredVanillaOptionPricingBoundaryInputError,
    },
    InvalidModelParameters {
        source: super::VanillaOptionPricingModelParametersError,
    },
    InvalidIntervalDelta {
        source: VanillaOptionBatchPricingRequestError,
    },
    BoundaryIntervalDeltaMismatch {
        request_interval_delta: f64,
        boundary_interval_delta: f64,
    },
    InvalidFourierInversionSettings {
        source: super::fourier::FourierInversionEngineSettingsError,
    },
    InvalidNodeIntervalCounts {
        source: VanillaOptionBatchPricingRequestError,
    },
    InvalidNodeIntervalCount {
        node_index: usize,
        interval_count: usize,
    },
    EmptyIntervalFactors,
    InvalidIntervalFactors {
        source: super::runner::VanillaOptionPricingIntervalFactorsError,
    },
    IntervalFactorShapeMismatch {
        expected_interval_factor_count: usize,
        found_interval_factor_count: usize,
    },
    MaturityIntervalMismatch {
        node_index: usize,
        requested_maturity_interval: f64,
        interval_grid_maturity_interval: f64,
    },
}

impl Display for VanillaOptionBatchPricingExecutionInputError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidRequest { source } => {
                write!(formatter, "batch pricing request is invalid: {source}")
            }
            Self::InvalidBoundaryInput { source } => {
                write!(
                    formatter,
                    "batch pricing boundary_input is invalid: {source}"
                )
            }
            Self::InvalidModelParameters { source } => {
                write!(
                    formatter,
                    "batch pricing model_parameters is invalid: {source}"
                )
            }
            Self::InvalidIntervalDelta { source } => {
                write!(
                    formatter,
                    "batch pricing interval_delta is invalid: {source}"
                )
            }
            Self::BoundaryIntervalDeltaMismatch {
                request_interval_delta,
                boundary_interval_delta,
            } => write!(
                formatter,
                "interval_delta must match boundary_input.interval_delta, found request={request_interval_delta} boundary_input={boundary_interval_delta}"
            ),
            Self::InvalidFourierInversionSettings { source } => write!(
                formatter,
                "batch pricing fourier_inversion_settings is invalid: {source}"
            ),
            Self::InvalidNodeIntervalCounts { source } => write!(
                formatter,
                "batch pricing node_interval_counts are invalid: {source}"
            ),
            Self::InvalidNodeIntervalCount {
                node_index,
                interval_count,
            } => write!(
                formatter,
                "batch pricing node_interval_counts[{node_index}] must be strictly positive, found {interval_count}"
            ),
            Self::EmptyIntervalFactors => write!(
                formatter,
                "batch pricing interval_factors must contain at least one interval"
            ),
            Self::InvalidIntervalFactors { source } => write!(
                formatter,
                "batch pricing interval_factors are invalid: {source}"
            ),
            Self::IntervalFactorShapeMismatch {
                expected_interval_factor_count,
                found_interval_factor_count,
            } => write!(
                formatter,
                "batch pricing interval_factors must contain exactly {expected_interval_factor_count} entries implied by node_interval_counts, found {found_interval_factor_count}"
            ),
            Self::MaturityIntervalMismatch {
                node_index,
                requested_maturity_interval,
                interval_grid_maturity_interval,
            } => write!(
                formatter,
                "batch pricing node {node_index} maturity_interval {requested_maturity_interval} must equal node_interval_counts[{node_index}] * interval_delta = {interval_grid_maturity_interval}"
            ),
        }
    }
}

impl Error for VanillaOptionBatchPricingExecutionInputError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidRequest { source } => Some(source),
            Self::InvalidBoundaryInput { source } => Some(source),
            Self::InvalidModelParameters { source } => Some(source),
            Self::InvalidIntervalDelta { source } => Some(source),
            Self::InvalidFourierInversionSettings { source } => Some(source),
            Self::InvalidNodeIntervalCounts { source } => Some(source),
            Self::InvalidIntervalFactors { source } => Some(source),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VanillaOptionBatchPricingResponse {
    pub spot: f64,
    pub strikes: Vec<f64>,
    pub maturity_intervals: Vec<f64>,
    pub payoff_types: Vec<VanillaOptionPayoffType>,
    pub risk_free_rates: Vec<f64>,
    pub dividend_yields: Vec<f64>,
    pub prices: Vec<f64>,
    pub forward_prices: Vec<f64>,
    pub discount_factors: Vec<f64>,
    pub boundary_state_norms: Vec<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VanillaOptionBatchPricingRunDiagnostics {
    pub transform: Vec<Option<ExponentialAffineTransformIntegrationResult>>,
    pub integration_quality: Vec<Option<FourierInversionDiagnostics>>,
    pub anchored_recursion: Vec<Option<AnchoredPricingRecursionDiagnostics>>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VanillaOptionBatchPricingCalibrationHooks {
    pub option_prices: Vec<f64>,
    pub log_moneyness: Vec<f64>,
    pub forward_moneyness: Vec<f64>,
    pub maturity_intervals: Vec<f64>,
    pub transform_status: Vec<Option<ExponentialAffineTransformIntegrationStatus>>,
    pub integration_numeric_quality_is_poor: Vec<Option<bool>>,
    pub integration_convergence_check_passed: Vec<Option<bool>>,
    pub integration_truncation_check_passed: Vec<Option<bool>>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VanillaOptionBatchPricingRunResult {
    pub response: VanillaOptionBatchPricingResponse,
    pub run_diagnostics: Option<VanillaOptionBatchPricingRunDiagnostics>,
    pub calibration_hooks: Option<VanillaOptionBatchPricingCalibrationHooks>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum VanillaOptionBatchPricingRunError {
    InvalidExecutionInput {
        source: VanillaOptionBatchPricingExecutionInputError,
    },
    NodePricingFailure {
        node_index: usize,
        source: VanillaOptionPricingRunError,
    },
    MissingRunDiagnostics {
        node_index: usize,
    },
    MissingCalibrationHooks {
        node_index: usize,
    },
}

impl Display for VanillaOptionBatchPricingRunError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidExecutionInput { source } => {
                write!(
                    formatter,
                    "batch pricing execution input is invalid: {source}"
                )
            }
            Self::NodePricingFailure { node_index, source } => write!(
                formatter,
                "batch pricing failed for node_index={node_index}: {source}"
            ),
            Self::MissingRunDiagnostics { node_index } => write!(
                formatter,
                "batch pricing requested run diagnostics but node_index={node_index} returned none"
            ),
            Self::MissingCalibrationHooks { node_index } => write!(
                formatter,
                "batch pricing requested calibration hooks but node_index={node_index} returned none"
            ),
        }
    }
}

impl Error for VanillaOptionBatchPricingRunError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidExecutionInput { source } => Some(source),
            Self::NodePricingFailure { source, .. } => Some(source),
            _ => None,
        }
    }
}

pub fn run_vanilla_option_batch_pricing(
    execution_input: VanillaOptionBatchPricingExecutionInput,
) -> Result<VanillaOptionBatchPricingRunResult, VanillaOptionBatchPricingRunError> {
    execution_input
        .validate()
        .map_err(|source| VanillaOptionBatchPricingRunError::InvalidExecutionInput { source })?;

    let request = execution_input.request.clone();
    let node_interval_counts = execution_input.node_interval_counts.clone();

    let optional_output_selection = execution_input.optional_output_selection;

    let mut interval_factor_offset = 0usize;

    let mut prices: Vec<f64> = Vec::with_capacity(request.node_count());
    let mut forward_prices: Vec<f64> = Vec::with_capacity(request.node_count());
    let mut discount_factors: Vec<f64> = Vec::with_capacity(request.node_count());
    let mut boundary_state_norms: Vec<f64> = Vec::with_capacity(request.node_count());

    let mut transform_diagnostics: Vec<Option<ExponentialAffineTransformIntegrationResult>> =
        Vec::with_capacity(request.node_count());
    let mut integration_quality_diagnostics: Vec<Option<FourierInversionDiagnostics>> =
        Vec::with_capacity(request.node_count());
    let mut anchored_recursion_diagnostics: Vec<Option<AnchoredPricingRecursionDiagnostics>> =
        Vec::with_capacity(request.node_count());

    let mut hook_option_prices: Vec<f64> = Vec::with_capacity(request.node_count());
    let mut hook_log_moneyness: Vec<f64> = Vec::with_capacity(request.node_count());
    let mut hook_forward_moneyness: Vec<f64> = Vec::with_capacity(request.node_count());
    let mut hook_maturity_intervals: Vec<f64> = Vec::with_capacity(request.node_count());
    let mut hook_transform_status: Vec<Option<ExponentialAffineTransformIntegrationStatus>> =
        Vec::with_capacity(request.node_count());
    let mut hook_numeric_quality_is_poor: Vec<Option<bool>> =
        Vec::with_capacity(request.node_count());
    let mut hook_convergence_check_passed: Vec<Option<bool>> =
        Vec::with_capacity(request.node_count());
    let mut hook_truncation_check_passed: Vec<Option<bool>> =
        Vec::with_capacity(request.node_count());

    for node_index in 0..request.node_count() {
        let interval_count = node_interval_counts[node_index];
        let interval_factor_slice = &execution_input.interval_factors
            [interval_factor_offset..interval_factor_offset + interval_count];

        interval_factor_offset += interval_count;

        let run_result = run_single_batch_node(
            &execution_input,
            request.spot,
            request.strikes[node_index],
            request.maturity_intervals[node_index],
            request.payoff_types[node_index],
            request.risk_free_rates[node_index],
            request.dividend_yields[node_index],
            interval_factor_slice.to_vec(),
        )
        .map_err(
            |source| VanillaOptionBatchPricingRunError::NodePricingFailure { node_index, source },
        )?;

        collect_batch_run_result(
            &run_result,
            optional_output_selection,
            node_index,
            &mut prices,
            &mut forward_prices,
            &mut discount_factors,
            &mut boundary_state_norms,
            &mut transform_diagnostics,
            &mut integration_quality_diagnostics,
            &mut anchored_recursion_diagnostics,
            &mut hook_option_prices,
            &mut hook_log_moneyness,
            &mut hook_forward_moneyness,
            &mut hook_maturity_intervals,
            &mut hook_transform_status,
            &mut hook_numeric_quality_is_poor,
            &mut hook_convergence_check_passed,
            &mut hook_truncation_check_passed,
        )?;
    }

    Ok(VanillaOptionBatchPricingRunResult {
        response: VanillaOptionBatchPricingResponse {
            spot: request.spot,
            strikes: request.strikes,
            maturity_intervals: request.maturity_intervals,
            payoff_types: request.payoff_types,
            risk_free_rates: request.risk_free_rates,
            dividend_yields: request.dividend_yields,
            prices,
            forward_prices,
            discount_factors,
            boundary_state_norms,
        },
        run_diagnostics: if optional_output_selection.include_transform_diagnostics
            || optional_output_selection.include_integration_quality_diagnostics
        {
            Some(VanillaOptionBatchPricingRunDiagnostics {
                transform: transform_diagnostics,
                integration_quality: integration_quality_diagnostics,
                anchored_recursion: anchored_recursion_diagnostics,
            })
        } else {
            None
        },
        calibration_hooks: if optional_output_selection.include_calibration_hooks {
            Some(VanillaOptionBatchPricingCalibrationHooks {
                option_prices: hook_option_prices,
                log_moneyness: hook_log_moneyness,
                forward_moneyness: hook_forward_moneyness,
                maturity_intervals: hook_maturity_intervals,
                transform_status: hook_transform_status,
                integration_numeric_quality_is_poor: hook_numeric_quality_is_poor,
                integration_convergence_check_passed: hook_convergence_check_passed,
                integration_truncation_check_passed: hook_truncation_check_passed,
            })
        } else {
            None
        },
    })
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VanillaOptionSurfacePricingRequest {
    pub spot: f64,
    pub strikes: Vec<f64>,
    pub maturity_intervals: Vec<f64>,
    pub payoff_type: VanillaOptionPayoffType,
    pub risk_free_rate: f64,
    pub dividend_yield: f64,
}

impl VanillaOptionSurfacePricingRequest {
    pub const fn new(
        spot: f64,
        strikes: Vec<f64>,
        maturity_intervals: Vec<f64>,
        payoff_type: VanillaOptionPayoffType,
        risk_free_rate: f64,
        dividend_yield: f64,
    ) -> Self {
        Self {
            spot,
            strikes,
            maturity_intervals,
            payoff_type,
            risk_free_rate,
            dividend_yield,
        }
    }

    pub fn try_new(
        spot: f64,
        strikes: Vec<f64>,
        maturity_intervals: Vec<f64>,
        payoff_type: VanillaOptionPayoffType,
        risk_free_rate: f64,
        dividend_yield: f64,
    ) -> Result<Self, VanillaOptionSurfacePricingRequestError> {
        let request = Self::new(
            spot,
            strikes,
            maturity_intervals,
            payoff_type,
            risk_free_rate,
            dividend_yield,
        );

        request.validate()?;

        Ok(request)
    }

    pub fn validate(&self) -> Result<(), VanillaOptionSurfacePricingRequestError> {
        validate_positive_finite("spot", self.spot).map_err(|source| {
            VanillaOptionSurfacePricingRequestError::InvalidRequestField { source }
        })?;

        if self.strikes.is_empty() {
            return Err(VanillaOptionSurfacePricingRequestError::EmptyStrikeAxis);
        }
        if self.maturity_intervals.is_empty() {
            return Err(VanillaOptionSurfacePricingRequestError::EmptyMaturityAxis);
        }

        for strike in &self.strikes {
            validate_positive_finite("strikes", *strike).map_err(|source| {
                VanillaOptionSurfacePricingRequestError::InvalidRequestField { source }
            })?;
        }

        for maturity_interval in &self.maturity_intervals {
            validate_positive_finite("maturity_intervals", *maturity_interval).map_err(
                |source| VanillaOptionSurfacePricingRequestError::InvalidRequestField { source },
            )?;
        }

        validate_finite("risk_free_rate", self.risk_free_rate).map_err(|source| {
            VanillaOptionSurfacePricingRequestError::InvalidRequestField { source }
        })?;
        validate_finite("dividend_yield", self.dividend_yield).map_err(|source| {
            VanillaOptionSurfacePricingRequestError::InvalidRequestField { source }
        })?;

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum VanillaOptionSurfacePricingRequestError {
    InvalidRequestField {
        source: VanillaOptionBatchPricingRequestError,
    },
    EmptyStrikeAxis,
    EmptyMaturityAxis,
}

impl Display for VanillaOptionSurfacePricingRequestError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidRequestField { source } => {
                write!(formatter, "surface pricing request is invalid: {source}")
            }
            Self::EmptyStrikeAxis => write!(
                formatter,
                "surface pricing request must contain at least one strike"
            ),
            Self::EmptyMaturityAxis => write!(
                formatter,
                "surface pricing request must contain at least one maturity_interval"
            ),
        }
    }
}

impl Error for VanillaOptionSurfacePricingRequestError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidRequestField { source } => Some(source),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VanillaOptionSurfacePricingExecutionInput {
    pub request: VanillaOptionSurfacePricingRequest,
    pub boundary_input: AnchoredVanillaOptionPricingBoundaryInput,
    pub model_parameters: VanillaOptionPricingModelParameters,
    pub interval_delta: f64,
    pub fourier_inversion_settings: Option<FourierInversionEngineSettings>,
    pub maturity_interval_counts: Vec<usize>,
    pub interval_factors: Vec<VanillaOptionPricingIntervalFactors>,
    pub optional_output_selection: VanillaOptionPricingOptionalOutputSelection,
}

impl VanillaOptionSurfacePricingExecutionInput {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        request: VanillaOptionSurfacePricingRequest,
        boundary_input: AnchoredVanillaOptionPricingBoundaryInput,
        model_parameters: VanillaOptionPricingModelParameters,
        interval_delta: f64,
        fourier_inversion_settings: Option<FourierInversionEngineSettings>,
        maturity_interval_counts: Vec<usize>,
        interval_factors: Vec<VanillaOptionPricingIntervalFactors>,
        optional_output_selection: VanillaOptionPricingOptionalOutputSelection,
    ) -> Self {
        Self {
            request,
            boundary_input,
            model_parameters,
            interval_delta,
            fourier_inversion_settings,
            maturity_interval_counts,
            interval_factors,
            optional_output_selection,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn try_new(
        request: VanillaOptionSurfacePricingRequest,
        boundary_input: AnchoredVanillaOptionPricingBoundaryInput,
        model_parameters: VanillaOptionPricingModelParameters,
        interval_delta: f64,
        fourier_inversion_settings: Option<FourierInversionEngineSettings>,
        maturity_interval_counts: Vec<usize>,
        interval_factors: Vec<VanillaOptionPricingIntervalFactors>,
        optional_output_selection: VanillaOptionPricingOptionalOutputSelection,
    ) -> Result<Self, VanillaOptionSurfacePricingExecutionInputError> {
        let execution_input = Self::new(
            request,
            boundary_input,
            model_parameters,
            interval_delta,
            fourier_inversion_settings,
            maturity_interval_counts,
            interval_factors,
            optional_output_selection,
        );

        execution_input.validate()?;

        Ok(execution_input)
    }

    pub fn validate(&self) -> Result<(), VanillaOptionSurfacePricingExecutionInputError> {
        self.request.validate().map_err(|source| {
            VanillaOptionSurfacePricingExecutionInputError::InvalidRequest { source }
        })?;

        self.boundary_input.validate().map_err(|source| {
            VanillaOptionSurfacePricingExecutionInputError::InvalidBoundaryInput { source }
        })?;

        self.model_parameters.validate().map_err(|source| {
            VanillaOptionSurfacePricingExecutionInputError::InvalidModelParameters { source }
        })?;

        validate_positive_finite("interval_delta", self.interval_delta).map_err(|source| {
            VanillaOptionSurfacePricingExecutionInputError::InvalidIntervalDelta { source }
        })?;

        if self.boundary_input.interval_delta != self.interval_delta {
            return Err(
                VanillaOptionSurfacePricingExecutionInputError::BoundaryIntervalDeltaMismatch {
                    request_interval_delta: self.interval_delta,
                    boundary_interval_delta: self.boundary_input.interval_delta,
                },
            );
        }

        if let Some(fourier_inversion_settings) = self.fourier_inversion_settings {
            fourier_inversion_settings.validate().map_err(|source| {
                VanillaOptionSurfacePricingExecutionInputError::InvalidFourierInversionSettings {
                    source,
                }
            })?;
        }

        if self.maturity_interval_counts.len() != self.request.maturity_intervals.len() {
            return Err(
                VanillaOptionSurfacePricingExecutionInputError::InvalidMaturityIntervalCountShape {
                    expected: self.request.maturity_intervals.len(),
                    found: self.maturity_interval_counts.len(),
                },
            );
        }

        if self.interval_factors.is_empty() {
            return Err(VanillaOptionSurfacePricingExecutionInputError::EmptyIntervalFactors);
        }

        for interval_factor in &self.interval_factors {
            interval_factor.validate().map_err(|source| {
                VanillaOptionSurfacePricingExecutionInputError::InvalidIntervalFactors { source }
            })?;
        }

        let mut factor_offset = 0usize;
        for (maturity_index, (maturity_interval, interval_count)) in self
            .request
            .maturity_intervals
            .iter()
            .zip(self.maturity_interval_counts.iter())
            .enumerate()
        {
            if *interval_count == 0 {
                return Err(
                    VanillaOptionSurfacePricingExecutionInputError::InvalidMaturityIntervalCount {
                        maturity_index,
                        interval_count: *interval_count,
                    },
                );
            }

            factor_offset = factor_offset.saturating_add(*interval_count);
            if factor_offset > self.interval_factors.len() {
                return Err(
                    VanillaOptionSurfacePricingExecutionInputError::IntervalFactorShapeMismatch {
                        expected_interval_factor_count: factor_offset,
                        found_interval_factor_count: self.interval_factors.len(),
                    },
                );
            }

            let expected_maturity_interval = self.interval_delta * *interval_count as f64;
            if expected_maturity_interval != *maturity_interval {
                return Err(
                    VanillaOptionSurfacePricingExecutionInputError::MaturityIntervalMismatch {
                        maturity_index,
                        requested_maturity_interval: *maturity_interval,
                        interval_grid_maturity_interval: expected_maturity_interval,
                    },
                );
            }
        }

        if factor_offset != self.interval_factors.len() {
            return Err(
                VanillaOptionSurfacePricingExecutionInputError::IntervalFactorShapeMismatch {
                    expected_interval_factor_count: factor_offset,
                    found_interval_factor_count: self.interval_factors.len(),
                },
            );
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum VanillaOptionSurfacePricingExecutionInputError {
    InvalidRequest {
        source: VanillaOptionSurfacePricingRequestError,
    },
    InvalidBoundaryInput {
        source: super::AnchoredVanillaOptionPricingBoundaryInputError,
    },
    InvalidModelParameters {
        source: super::VanillaOptionPricingModelParametersError,
    },
    InvalidIntervalDelta {
        source: VanillaOptionBatchPricingRequestError,
    },
    BoundaryIntervalDeltaMismatch {
        request_interval_delta: f64,
        boundary_interval_delta: f64,
    },
    InvalidFourierInversionSettings {
        source: super::fourier::FourierInversionEngineSettingsError,
    },
    InvalidMaturityIntervalCountShape {
        expected: usize,
        found: usize,
    },
    InvalidMaturityIntervalCount {
        maturity_index: usize,
        interval_count: usize,
    },
    EmptyIntervalFactors,
    InvalidIntervalFactors {
        source: super::runner::VanillaOptionPricingIntervalFactorsError,
    },
    IntervalFactorShapeMismatch {
        expected_interval_factor_count: usize,
        found_interval_factor_count: usize,
    },
    MaturityIntervalMismatch {
        maturity_index: usize,
        requested_maturity_interval: f64,
        interval_grid_maturity_interval: f64,
    },
}

impl Display for VanillaOptionSurfacePricingExecutionInputError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidRequest { source } => {
                write!(formatter, "surface pricing request is invalid: {source}")
            }
            Self::InvalidBoundaryInput { source } => {
                write!(
                    formatter,
                    "surface pricing boundary_input is invalid: {source}"
                )
            }
            Self::InvalidModelParameters { source } => {
                write!(
                    formatter,
                    "surface pricing model_parameters are invalid: {source}"
                )
            }
            Self::InvalidIntervalDelta { source } => {
                write!(
                    formatter,
                    "surface pricing interval_delta is invalid: {source}"
                )
            }
            Self::BoundaryIntervalDeltaMismatch {
                request_interval_delta,
                boundary_interval_delta,
            } => write!(
                formatter,
                "interval_delta must match boundary_input.interval_delta, found request={request_interval_delta} boundary_input={boundary_interval_delta}"
            ),
            Self::InvalidFourierInversionSettings { source } => write!(
                formatter,
                "surface pricing fourier_inversion_settings are invalid: {source}"
            ),
            Self::InvalidMaturityIntervalCountShape { expected, found } => write!(
                formatter,
                "surface pricing maturity_interval_counts must contain {expected} aligned values, found {found}"
            ),
            Self::InvalidMaturityIntervalCount {
                maturity_index,
                interval_count,
            } => write!(
                formatter,
                "surface pricing maturity_interval_counts[{maturity_index}] must be strictly positive, found {interval_count}"
            ),
            Self::EmptyIntervalFactors => write!(
                formatter,
                "surface pricing interval_factors must contain at least one interval"
            ),
            Self::InvalidIntervalFactors { source } => write!(
                formatter,
                "surface pricing interval_factors are invalid: {source}"
            ),
            Self::IntervalFactorShapeMismatch {
                expected_interval_factor_count,
                found_interval_factor_count,
            } => write!(
                formatter,
                "surface pricing interval_factors must contain exactly {expected_interval_factor_count} entries implied by maturity_interval_counts, found {found_interval_factor_count}"
            ),
            Self::MaturityIntervalMismatch {
                maturity_index,
                requested_maturity_interval,
                interval_grid_maturity_interval,
            } => write!(
                formatter,
                "surface pricing maturity_interval[{maturity_index}] {requested_maturity_interval} must equal maturity_interval_counts[{maturity_index}] * interval_delta = {interval_grid_maturity_interval}"
            ),
        }
    }
}

impl Error for VanillaOptionSurfacePricingExecutionInputError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidRequest { source } => Some(source),
            Self::InvalidBoundaryInput { source } => Some(source),
            Self::InvalidModelParameters { source } => Some(source),
            Self::InvalidIntervalDelta { source } => Some(source),
            Self::InvalidFourierInversionSettings { source } => Some(source),
            Self::InvalidIntervalFactors { source } => Some(source),
            _ => None,
        }
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
    pub price_grid: Vec<Vec<f64>>,
    pub forward_price_grid: Vec<Vec<f64>>,
    pub discount_factor_grid: Vec<Vec<f64>>,
    pub boundary_state_norm_grid: Vec<Vec<f64>>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VanillaOptionSurfacePricingRunDiagnostics {
    pub transform_grid: Vec<Vec<Option<ExponentialAffineTransformIntegrationResult>>>,
    pub integration_quality_grid: Vec<Vec<Option<FourierInversionDiagnostics>>>,
    pub anchored_recursion_grid: Vec<Vec<Option<AnchoredPricingRecursionDiagnostics>>>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VanillaOptionSurfacePricingCalibrationHooks {
    pub option_price_grid: Vec<Vec<f64>>,
    pub log_moneyness_grid: Vec<Vec<f64>>,
    pub forward_moneyness_grid: Vec<Vec<f64>>,
    pub maturity_interval_grid: Vec<Vec<f64>>,
    pub transform_status_grid: Vec<Vec<Option<ExponentialAffineTransformIntegrationStatus>>>,
    pub integration_numeric_quality_is_poor_grid: Vec<Vec<Option<bool>>>,
    pub integration_convergence_check_passed_grid: Vec<Vec<Option<bool>>>,
    pub integration_truncation_check_passed_grid: Vec<Vec<Option<bool>>>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VanillaOptionSurfacePricingRunResult {
    pub response: VanillaOptionSurfacePricingResponse,
    pub run_diagnostics: Option<VanillaOptionSurfacePricingRunDiagnostics>,
    pub calibration_hooks: Option<VanillaOptionSurfacePricingCalibrationHooks>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum VanillaOptionSurfacePricingRunError {
    InvalidExecutionInput {
        source: VanillaOptionSurfacePricingExecutionInputError,
    },
    NodePricingFailure {
        maturity_index: usize,
        strike_index: usize,
        source: VanillaOptionPricingRunError,
    },
    MissingRunDiagnostics {
        maturity_index: usize,
        strike_index: usize,
    },
    MissingCalibrationHooks {
        maturity_index: usize,
        strike_index: usize,
    },
}

impl Display for VanillaOptionSurfacePricingRunError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidExecutionInput { source } => {
                write!(
                    formatter,
                    "surface pricing execution input is invalid: {source}"
                )
            }
            Self::NodePricingFailure {
                maturity_index,
                strike_index,
                source,
            } => write!(
                formatter,
                "surface pricing failed for maturity_index={maturity_index} strike_index={strike_index}: {source}"
            ),
            Self::MissingRunDiagnostics {
                maturity_index,
                strike_index,
            } => write!(
                formatter,
                "surface pricing requested run diagnostics but maturity_index={maturity_index} strike_index={strike_index} returned none"
            ),
            Self::MissingCalibrationHooks {
                maturity_index,
                strike_index,
            } => write!(
                formatter,
                "surface pricing requested calibration hooks but maturity_index={maturity_index} strike_index={strike_index} returned none"
            ),
        }
    }
}

impl Error for VanillaOptionSurfacePricingRunError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidExecutionInput { source } => Some(source),
            Self::NodePricingFailure { source, .. } => Some(source),
            _ => None,
        }
    }
}

pub fn run_vanilla_option_surface_pricing(
    execution_input: VanillaOptionSurfacePricingExecutionInput,
) -> Result<VanillaOptionSurfacePricingRunResult, VanillaOptionSurfacePricingRunError> {
    execution_input
        .validate()
        .map_err(|source| VanillaOptionSurfacePricingRunError::InvalidExecutionInput { source })?;

    let request = execution_input.request.clone();

    let optional_output_selection = execution_input.optional_output_selection;

    let mut interval_factor_offset = 0usize;

    let mut price_grid: Vec<Vec<f64>> = Vec::with_capacity(request.maturity_intervals.len());
    let mut forward_price_grid: Vec<Vec<f64>> =
        Vec::with_capacity(request.maturity_intervals.len());
    let mut discount_factor_grid: Vec<Vec<f64>> =
        Vec::with_capacity(request.maturity_intervals.len());
    let mut boundary_state_norm_grid: Vec<Vec<f64>> =
        Vec::with_capacity(request.maturity_intervals.len());

    let mut transform_grid: Vec<Vec<Option<ExponentialAffineTransformIntegrationResult>>> =
        Vec::with_capacity(request.maturity_intervals.len());
    let mut integration_quality_grid: Vec<Vec<Option<FourierInversionDiagnostics>>> =
        Vec::with_capacity(request.maturity_intervals.len());
    let mut anchored_recursion_grid: Vec<Vec<Option<AnchoredPricingRecursionDiagnostics>>> =
        Vec::with_capacity(request.maturity_intervals.len());

    let mut option_price_grid: Vec<Vec<f64>> = Vec::with_capacity(request.maturity_intervals.len());
    let mut log_moneyness_grid: Vec<Vec<f64>> =
        Vec::with_capacity(request.maturity_intervals.len());
    let mut forward_moneyness_grid: Vec<Vec<f64>> =
        Vec::with_capacity(request.maturity_intervals.len());
    let mut maturity_interval_grid: Vec<Vec<f64>> =
        Vec::with_capacity(request.maturity_intervals.len());
    let mut transform_status_grid: Vec<Vec<Option<ExponentialAffineTransformIntegrationStatus>>> =
        Vec::with_capacity(request.maturity_intervals.len());
    let mut numeric_quality_is_poor_grid: Vec<Vec<Option<bool>>> =
        Vec::with_capacity(request.maturity_intervals.len());
    let mut convergence_check_passed_grid: Vec<Vec<Option<bool>>> =
        Vec::with_capacity(request.maturity_intervals.len());
    let mut truncation_check_passed_grid: Vec<Vec<Option<bool>>> =
        Vec::with_capacity(request.maturity_intervals.len());

    for (maturity_index, maturity_interval) in request.maturity_intervals.iter().enumerate() {
        let interval_count = execution_input.maturity_interval_counts[maturity_index];
        let interval_factor_slice = execution_input.interval_factors
            [interval_factor_offset..interval_factor_offset + interval_count]
            .to_vec();

        interval_factor_offset += interval_count;

        let mut price_row: Vec<f64> = Vec::with_capacity(request.strikes.len());
        let mut forward_price_row: Vec<f64> = Vec::with_capacity(request.strikes.len());
        let mut discount_factor_row: Vec<f64> = Vec::with_capacity(request.strikes.len());
        let mut boundary_state_norm_row: Vec<f64> = Vec::with_capacity(request.strikes.len());

        let mut transform_row: Vec<Option<ExponentialAffineTransformIntegrationResult>> =
            Vec::with_capacity(request.strikes.len());
        let mut integration_quality_row: Vec<Option<FourierInversionDiagnostics>> =
            Vec::with_capacity(request.strikes.len());
        let mut anchored_recursion_row: Vec<Option<AnchoredPricingRecursionDiagnostics>> =
            Vec::with_capacity(request.strikes.len());

        let mut option_price_row: Vec<f64> = Vec::with_capacity(request.strikes.len());
        let mut log_moneyness_row: Vec<f64> = Vec::with_capacity(request.strikes.len());
        let mut forward_moneyness_row: Vec<f64> = Vec::with_capacity(request.strikes.len());
        let mut maturity_interval_row: Vec<f64> = Vec::with_capacity(request.strikes.len());
        let mut transform_status_row: Vec<Option<ExponentialAffineTransformIntegrationStatus>> =
            Vec::with_capacity(request.strikes.len());
        let mut numeric_quality_is_poor_row: Vec<Option<bool>> =
            Vec::with_capacity(request.strikes.len());
        let mut convergence_check_passed_row: Vec<Option<bool>> =
            Vec::with_capacity(request.strikes.len());
        let mut truncation_check_passed_row: Vec<Option<bool>> =
            Vec::with_capacity(request.strikes.len());

        for (strike_index, strike) in request.strikes.iter().enumerate() {
            let run_result = run_single_batch_node(
                &VanillaOptionBatchPricingExecutionInput {
                    request: VanillaOptionBatchPricingRequest {
                        spot: request.spot,
                        strikes: vec![],
                        maturity_intervals: vec![],
                        payoff_types: vec![],
                        risk_free_rates: vec![],
                        dividend_yields: vec![],
                    },
                    boundary_input: execution_input.boundary_input.clone(),
                    model_parameters: execution_input.model_parameters.clone(),
                    interval_delta: execution_input.interval_delta,
                    fourier_inversion_settings: execution_input.fourier_inversion_settings,
                    node_interval_counts: vec![],
                    interval_factors: vec![],
                    optional_output_selection,
                },
                request.spot,
                *strike,
                *maturity_interval,
                request.payoff_type,
                request.risk_free_rate,
                request.dividend_yield,
                interval_factor_slice.clone(),
            )
            .map_err(|source| {
                VanillaOptionSurfacePricingRunError::NodePricingFailure {
                    maturity_index,
                    strike_index,
                    source,
                }
            })?;

            collect_surface_run_result(
                &run_result,
                optional_output_selection,
                maturity_index,
                strike_index,
                &mut price_row,
                &mut forward_price_row,
                &mut discount_factor_row,
                &mut boundary_state_norm_row,
                &mut transform_row,
                &mut integration_quality_row,
                &mut anchored_recursion_row,
                &mut option_price_row,
                &mut log_moneyness_row,
                &mut forward_moneyness_row,
                &mut maturity_interval_row,
                &mut transform_status_row,
                &mut numeric_quality_is_poor_row,
                &mut convergence_check_passed_row,
                &mut truncation_check_passed_row,
            )?;
        }

        price_grid.push(price_row);
        forward_price_grid.push(forward_price_row);
        discount_factor_grid.push(discount_factor_row);
        boundary_state_norm_grid.push(boundary_state_norm_row);
        transform_grid.push(transform_row);
        integration_quality_grid.push(integration_quality_row);
        anchored_recursion_grid.push(anchored_recursion_row);
        option_price_grid.push(option_price_row);
        log_moneyness_grid.push(log_moneyness_row);
        forward_moneyness_grid.push(forward_moneyness_row);
        maturity_interval_grid.push(maturity_interval_row);
        transform_status_grid.push(transform_status_row);
        numeric_quality_is_poor_grid.push(numeric_quality_is_poor_row);
        convergence_check_passed_grid.push(convergence_check_passed_row);
        truncation_check_passed_grid.push(truncation_check_passed_row);
    }

    Ok(VanillaOptionSurfacePricingRunResult {
        response: VanillaOptionSurfacePricingResponse {
            spot: request.spot,
            strikes: request.strikes,
            maturity_intervals: request.maturity_intervals,
            payoff_type: request.payoff_type,
            risk_free_rate: request.risk_free_rate,
            dividend_yield: request.dividend_yield,
            price_grid,
            forward_price_grid,
            discount_factor_grid,
            boundary_state_norm_grid,
        },
        run_diagnostics: if optional_output_selection.include_transform_diagnostics
            || optional_output_selection.include_integration_quality_diagnostics
        {
            Some(VanillaOptionSurfacePricingRunDiagnostics {
                transform_grid,
                integration_quality_grid,
                anchored_recursion_grid,
            })
        } else {
            None
        },
        calibration_hooks: if optional_output_selection.include_calibration_hooks {
            Some(VanillaOptionSurfacePricingCalibrationHooks {
                option_price_grid,
                log_moneyness_grid,
                forward_moneyness_grid,
                maturity_interval_grid,
                transform_status_grid,
                integration_numeric_quality_is_poor_grid: numeric_quality_is_poor_grid,
                integration_convergence_check_passed_grid: convergence_check_passed_grid,
                integration_truncation_check_passed_grid: truncation_check_passed_grid,
            })
        } else {
            None
        },
    })
}

fn run_single_batch_node(
    execution_input: &VanillaOptionBatchPricingExecutionInput,
    spot: f64,
    strike: f64,
    maturity_interval: f64,
    payoff_type: VanillaOptionPayoffType,
    risk_free_rate: f64,
    dividend_yield: f64,
    interval_factors: Vec<VanillaOptionPricingIntervalFactors>,
) -> Result<VanillaOptionPricingRunResult, VanillaOptionPricingRunError> {
    let request = VanillaOptionPricingRequest::new_with_interval_delta(
        super::VanillaOptionPricingMarketInputs::new(
            spot,
            strike,
            maturity_interval,
            risk_free_rate,
            dividend_yield,
        ),
        VanillaOptionPayoffConfiguration { payoff_type },
        execution_input.boundary_input.clone(),
        execution_input.model_parameters.clone(),
        execution_input.interval_delta,
    );

    let request = match execution_input.fourier_inversion_settings {
        Some(fourier_inversion_settings) => {
            request.with_fourier_inversion_settings(fourier_inversion_settings)
        }
        None => request,
    };

    let single_execution_input = VanillaOptionPricingExecutionInput::new(
        request,
        interval_factors,
        execution_input.optional_output_selection,
    );

    run_vanilla_option_pricing(single_execution_input)
}

#[allow(clippy::too_many_arguments)]
fn collect_batch_run_result(
    run_result: &VanillaOptionPricingRunResult,
    optional_output_selection: VanillaOptionPricingOptionalOutputSelection,
    node_index: usize,
    prices: &mut Vec<f64>,
    forward_prices: &mut Vec<f64>,
    discount_factors: &mut Vec<f64>,
    boundary_state_norms: &mut Vec<f64>,
    transform_diagnostics: &mut Vec<Option<ExponentialAffineTransformIntegrationResult>>,
    integration_quality_diagnostics: &mut Vec<Option<FourierInversionDiagnostics>>,
    anchored_recursion_diagnostics: &mut Vec<Option<AnchoredPricingRecursionDiagnostics>>,
    hook_option_prices: &mut Vec<f64>,
    hook_log_moneyness: &mut Vec<f64>,
    hook_forward_moneyness: &mut Vec<f64>,
    hook_maturity_intervals: &mut Vec<f64>,
    hook_transform_status: &mut Vec<Option<ExponentialAffineTransformIntegrationStatus>>,
    hook_numeric_quality_is_poor: &mut Vec<Option<bool>>,
    hook_convergence_check_passed: &mut Vec<Option<bool>>,
    hook_truncation_check_passed: &mut Vec<Option<bool>>,
) -> Result<(), VanillaOptionBatchPricingRunError> {
    let VanillaOptionPricingResponse { price, diagnostics } = run_result.response;

    prices.push(price);
    forward_prices.push(diagnostics.forward_price);
    discount_factors.push(diagnostics.discount_factor);
    boundary_state_norms.push(diagnostics.boundary_state_norm);

    if optional_output_selection.include_transform_diagnostics
        || optional_output_selection.include_integration_quality_diagnostics
    {
        let run_diagnostics = run_result
            .run_diagnostics
            .clone()
            .ok_or(VanillaOptionBatchPricingRunError::MissingRunDiagnostics { node_index })?;

        transform_diagnostics.push(run_diagnostics.transform);
        integration_quality_diagnostics.push(run_diagnostics.integration_quality);
        anchored_recursion_diagnostics.push(run_diagnostics.anchored_recursion);
    }

    if optional_output_selection.include_calibration_hooks {
        let calibration_hooks = run_result
            .calibration_hooks
            .clone()
            .ok_or(VanillaOptionBatchPricingRunError::MissingCalibrationHooks { node_index })?;

        collect_calibration_hooks(
            calibration_hooks,
            hook_option_prices,
            hook_log_moneyness,
            hook_forward_moneyness,
            hook_maturity_intervals,
            hook_transform_status,
            hook_numeric_quality_is_poor,
            hook_convergence_check_passed,
            hook_truncation_check_passed,
        );
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn collect_surface_run_result(
    run_result: &VanillaOptionPricingRunResult,
    optional_output_selection: VanillaOptionPricingOptionalOutputSelection,
    maturity_index: usize,
    strike_index: usize,
    price_row: &mut Vec<f64>,
    forward_price_row: &mut Vec<f64>,
    discount_factor_row: &mut Vec<f64>,
    boundary_state_norm_row: &mut Vec<f64>,
    transform_row: &mut Vec<Option<ExponentialAffineTransformIntegrationResult>>,
    integration_quality_row: &mut Vec<Option<FourierInversionDiagnostics>>,
    anchored_recursion_row: &mut Vec<Option<AnchoredPricingRecursionDiagnostics>>,
    option_price_row: &mut Vec<f64>,
    log_moneyness_row: &mut Vec<f64>,
    forward_moneyness_row: &mut Vec<f64>,
    maturity_interval_row: &mut Vec<f64>,
    transform_status_row: &mut Vec<Option<ExponentialAffineTransformIntegrationStatus>>,
    numeric_quality_is_poor_row: &mut Vec<Option<bool>>,
    convergence_check_passed_row: &mut Vec<Option<bool>>,
    truncation_check_passed_row: &mut Vec<Option<bool>>,
) -> Result<(), VanillaOptionSurfacePricingRunError> {
    let VanillaOptionPricingResponse { price, diagnostics } = run_result.response;

    price_row.push(price);
    forward_price_row.push(diagnostics.forward_price);
    discount_factor_row.push(diagnostics.discount_factor);
    boundary_state_norm_row.push(diagnostics.boundary_state_norm);

    if optional_output_selection.include_transform_diagnostics
        || optional_output_selection.include_integration_quality_diagnostics
    {
        let run_diagnostics = run_result.run_diagnostics.clone().ok_or(
            VanillaOptionSurfacePricingRunError::MissingRunDiagnostics {
                maturity_index,
                strike_index,
            },
        )?;

        transform_row.push(run_diagnostics.transform);
        integration_quality_row.push(run_diagnostics.integration_quality);
        anchored_recursion_row.push(run_diagnostics.anchored_recursion);
    }

    if optional_output_selection.include_calibration_hooks {
        let calibration_hooks = run_result.calibration_hooks.clone().ok_or(
            VanillaOptionSurfacePricingRunError::MissingCalibrationHooks {
                maturity_index,
                strike_index,
            },
        )?;

        collect_calibration_hooks(
            calibration_hooks,
            option_price_row,
            log_moneyness_row,
            forward_moneyness_row,
            maturity_interval_row,
            transform_status_row,
            numeric_quality_is_poor_row,
            convergence_check_passed_row,
            truncation_check_passed_row,
        );
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn collect_calibration_hooks(
    calibration_hooks: VanillaOptionPricingCalibrationHooks,
    option_prices: &mut Vec<f64>,
    log_moneyness: &mut Vec<f64>,
    forward_moneyness: &mut Vec<f64>,
    maturity_intervals: &mut Vec<f64>,
    transform_status: &mut Vec<Option<ExponentialAffineTransformIntegrationStatus>>,
    integration_numeric_quality_is_poor: &mut Vec<Option<bool>>,
    integration_convergence_check_passed: &mut Vec<Option<bool>>,
    integration_truncation_check_passed: &mut Vec<Option<bool>>,
) {
    option_prices.push(calibration_hooks.option_price);
    log_moneyness.push(calibration_hooks.log_moneyness);
    forward_moneyness.push(calibration_hooks.forward_moneyness);
    maturity_intervals.push(calibration_hooks.maturity_interval);
    transform_status.push(calibration_hooks.transform_status);
    integration_numeric_quality_is_poor.push(calibration_hooks.integration_numeric_quality_is_poor);
    integration_convergence_check_passed
        .push(calibration_hooks.integration_convergence_check_passed);
    integration_truncation_check_passed.push(calibration_hooks.integration_truncation_check_passed);
}

fn validate_aligned_node_field_count(
    field: &'static str,
    found: usize,
    expected: usize,
) -> Result<(), VanillaOptionBatchPricingRequestError> {
    if found != expected {
        return Err(
            VanillaOptionBatchPricingRequestError::InvalidAlignedFieldCount {
                field,
                expected,
                found,
            },
        );
    }

    Ok(())
}

fn validate_finite(
    field: &'static str,
    value: f64,
) -> Result<(), VanillaOptionBatchPricingRequestError> {
    if !value.is_finite() {
        return Err(VanillaOptionBatchPricingRequestError::InvalidFiniteField { field, value });
    }

    Ok(())
}

fn validate_positive_finite(
    field: &'static str,
    value: f64,
) -> Result<(), VanillaOptionBatchPricingRequestError> {
    if !value.is_finite() || value <= 0.0 {
        return Err(VanillaOptionBatchPricingRequestError::InvalidPositiveField { field, value });
    }

    Ok(())
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct VanillaOptionBatchPricingRequestPayload {
    spot: f64,
    strikes: Vec<f64>,
    maturity_intervals: Vec<f64>,
    payoff_types: Vec<VanillaOptionPayoffType>,
    risk_free_rates: Vec<f64>,
    dividend_yields: Vec<f64>,
}

impl<'de> Deserialize<'de> for VanillaOptionBatchPricingRequest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let payload = VanillaOptionBatchPricingRequestPayload::deserialize(deserializer)?;

        Self::try_new(
            payload.spot,
            payload.strikes,
            payload.maturity_intervals,
            payload.payoff_types,
            payload.risk_free_rates,
            payload.dividend_yields,
        )
        .map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct VanillaOptionBatchPricingExecutionInputPayload {
    request: VanillaOptionBatchPricingRequest,
    boundary_input: AnchoredVanillaOptionPricingBoundaryInput,
    model_parameters: VanillaOptionPricingModelParameters,
    interval_delta: f64,
    #[serde(default)]
    fourier_inversion_settings: Option<FourierInversionEngineSettings>,
    node_interval_counts: Vec<usize>,
    interval_factors: Vec<VanillaOptionPricingIntervalFactors>,
    #[serde(default)]
    optional_output_selection: VanillaOptionPricingOptionalOutputSelection,
}

impl<'de> Deserialize<'de> for VanillaOptionBatchPricingExecutionInput {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let payload = VanillaOptionBatchPricingExecutionInputPayload::deserialize(deserializer)?;

        Self::try_new(
            payload.request,
            payload.boundary_input,
            payload.model_parameters,
            payload.interval_delta,
            payload.fourier_inversion_settings,
            payload.node_interval_counts,
            payload.interval_factors,
            payload.optional_output_selection,
        )
        .map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct VanillaOptionSurfacePricingRequestPayload {
    spot: f64,
    strikes: Vec<f64>,
    maturity_intervals: Vec<f64>,
    payoff_type: VanillaOptionPayoffType,
    risk_free_rate: f64,
    dividend_yield: f64,
}

impl<'de> Deserialize<'de> for VanillaOptionSurfacePricingRequest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let payload = VanillaOptionSurfacePricingRequestPayload::deserialize(deserializer)?;

        Self::try_new(
            payload.spot,
            payload.strikes,
            payload.maturity_intervals,
            payload.payoff_type,
            payload.risk_free_rate,
            payload.dividend_yield,
        )
        .map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct VanillaOptionSurfacePricingExecutionInputPayload {
    request: VanillaOptionSurfacePricingRequest,
    boundary_input: AnchoredVanillaOptionPricingBoundaryInput,
    model_parameters: VanillaOptionPricingModelParameters,
    interval_delta: f64,
    #[serde(default)]
    fourier_inversion_settings: Option<FourierInversionEngineSettings>,
    maturity_interval_counts: Vec<usize>,
    interval_factors: Vec<VanillaOptionPricingIntervalFactors>,
    #[serde(default)]
    optional_output_selection: VanillaOptionPricingOptionalOutputSelection,
}

impl<'de> Deserialize<'de> for VanillaOptionSurfacePricingExecutionInput {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let payload = VanillaOptionSurfacePricingExecutionInputPayload::deserialize(deserializer)?;

        Self::try_new(
            payload.request,
            payload.boundary_input,
            payload.model_parameters,
            payload.interval_delta,
            payload.fourier_inversion_settings,
            payload.maturity_interval_counts,
            payload.interval_factors,
            payload.optional_output_selection,
        )
        .map_err(serde::de::Error::custom)
    }
}
