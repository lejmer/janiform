use std::error::Error;
use std::fmt::{Display, Formatter};

use serde::Deserialize;
use serde::Serialize;

use janiform_affine_oracle::affine_oracle::{
    ExponentialAffineTransformIntegrationResult, ExponentialAffineTransformIntegrationStatus,
};
use janiform_foundations::numeric::{
    validate_finite_number, validate_non_negative_number, validate_positive_number,
};
use janiform_model_kernels::hawkes::{
    HawkesExcitationState, HawkesKernelParameterError, HawkesKernelParameters,
};
use janiform_model_kernels::mean_reversion::{
    MeanReversionBoundaryState, MeanReversionBoundaryStateValidationError,
    MeanReversionFittedParameters, MeanReversionFittedParametersValidationError,
};
use janiform_model_kernels::news::{NewsProcessConfiguration, NewsProcessConfigurationError};
use janiform_model_kernels::nig::{
    NigSharedPlusShiftConfiguration, NigSharedPlusShiftConfigurationError,
};

use super::constants::{
    ANCHORED_PRICING_CONTINUATION_RECURSION_POLICY, ANCHORED_PRICING_DEVIATION_GRID_POLICY,
    ANCHORED_PRICING_EXPECTED_HAWKES_STATE_UPDATE_POLICY, DEFAULT_INTRADAY_PRICING_INTERVAL_DELTA,
    VANILLA_OPTION_CALIBRATION_HOOK_POLICY, VANILLA_OPTION_PAYOFF_CONFIGURATION_POLICY,
    VANILLA_OPTION_PRICING_CALIBRATION_HOOKS_REQUIRED_FIELDS,
    VANILLA_OPTION_PRICING_CALIBRATION_HOOKS_SCHEMA_VERSION,
    VANILLA_OPTION_PRICING_RUN_DIAGNOSTICS_REQUIRED_FIELDS,
    VANILLA_OPTION_PRICING_RUN_DIAGNOSTICS_SCHEMA_VERSION,
    VANILLA_OPTION_PRICING_RUN_OUTPUT_SCHEMA_METADATA_VERSION,
    VANILLA_OPTION_PRICING_RUN_RESULT_REQUIRED_FIELDS,
    VANILLA_OPTION_PRICING_RUN_RESULT_SCHEMA_VERSION, VANILLA_OPTION_REQUEST_VALIDATION_POLICY,
    VANILLA_OPTION_RESPONSE_VALIDATION_POLICY,
};
use super::fourier::{
    FourierInversionDiagnostics, FourierInversionEngineSettings,
    FourierInversionEngineSettingsError,
};
use super::implied_volatility::VanillaOptionImpliedVolatilityInversionStatus;

const PRICING_MEAN_REVERSION_ALIGNMENT_TOLERANCE: f64 = 1.0e-12;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VanillaOptionPayoffType {
    Call,
    Put,
}

/// Public payoff selection payload for vanilla option pricing requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VanillaOptionPayoffConfiguration {
    pub payoff_type: VanillaOptionPayoffType,
}

impl VanillaOptionPayoffConfiguration {
    pub const fn policy() -> &'static str {
        VANILLA_OPTION_PAYOFF_CONFIGURATION_POLICY
    }
}

/// Public market-input payload for vanilla option pricing.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct VanillaOptionPricingMarketInputs {
    pub spot: f64,
    pub strike: f64,
    pub maturity_interval: f64,
    pub risk_free_rate: f64,
    pub dividend_yield: f64,
}

impl VanillaOptionPricingMarketInputs {
    pub const fn new(
        spot: f64,
        strike: f64,
        maturity_interval: f64,
        risk_free_rate: f64,
        dividend_yield: f64,
    ) -> Self {
        Self {
            spot,
            strike,
            maturity_interval,
            risk_free_rate,
            dividend_yield,
        }
    }

    pub fn try_new(
        spot: f64,
        strike: f64,
        maturity_interval: f64,
        risk_free_rate: f64,
        dividend_yield: f64,
    ) -> Result<Self, VanillaOptionPricingMarketInputsError> {
        let market_inputs = Self::new(
            spot,
            strike,
            maturity_interval,
            risk_free_rate,
            dividend_yield,
        );

        market_inputs.validate()?;

        Ok(market_inputs)
    }

    pub fn validate(self) -> Result<(), VanillaOptionPricingMarketInputsError> {
        if !self.spot.is_finite() || self.spot <= 0.0 {
            return Err(VanillaOptionPricingMarketInputsError::InvalidSpot { value: self.spot });
        }
        if !self.strike.is_finite() || self.strike <= 0.0 {
            return Err(VanillaOptionPricingMarketInputsError::InvalidStrike {
                value: self.strike,
            });
        }
        if !self.maturity_interval.is_finite() || self.maturity_interval <= 0.0 {
            return Err(
                VanillaOptionPricingMarketInputsError::InvalidMaturityInterval {
                    value: self.maturity_interval,
                },
            );
        }
        if !self.risk_free_rate.is_finite() {
            return Err(VanillaOptionPricingMarketInputsError::InvalidRiskFreeRate {
                value: self.risk_free_rate,
            });
        }
        if !self.dividend_yield.is_finite() {
            return Err(
                VanillaOptionPricingMarketInputsError::InvalidDividendYield {
                    value: self.dividend_yield,
                },
            );
        }

        Ok(())
    }
}

/// Validation errors for public vanilla option market inputs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum VanillaOptionPricingMarketInputsError {
    InvalidSpot { value: f64 },
    InvalidStrike { value: f64 },
    InvalidMaturityInterval { value: f64 },
    InvalidRiskFreeRate { value: f64 },
    InvalidDividendYield { value: f64 },
}

impl Display for VanillaOptionPricingMarketInputsError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidSpot { value } => write!(
                formatter,
                "spot must be finite and strictly positive, found {value}"
            ),
            Self::InvalidStrike { value } => write!(
                formatter,
                "strike must be finite and strictly positive, found {value}"
            ),
            Self::InvalidMaturityInterval { value } => write!(
                formatter,
                "maturity_interval must be finite and strictly positive, found {value}"
            ),
            Self::InvalidRiskFreeRate { value } => {
                write!(formatter, "risk_free_rate must be finite, found {value}")
            }
            Self::InvalidDividendYield { value } => {
                write!(formatter, "dividend_yield must be finite, found {value}")
            }
        }
    }
}

impl Error for VanillaOptionPricingMarketInputsError {}

/// Public model-parameter payload for baseline vanilla option pricing.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VanillaOptionPricingModelParameters {
    pub hawkes_kernel_parameters: HawkesKernelParameters,
    pub news_process_configuration: NewsProcessConfiguration,
    pub nig_shared_plus_shift_configuration: NigSharedPlusShiftConfiguration,
}

impl VanillaOptionPricingModelParameters {
    pub const fn new(
        hawkes_kernel_parameters: HawkesKernelParameters,
        news_process_configuration: NewsProcessConfiguration,
        nig_shared_plus_shift_configuration: NigSharedPlusShiftConfiguration,
    ) -> Self {
        Self {
            hawkes_kernel_parameters,
            news_process_configuration,
            nig_shared_plus_shift_configuration,
        }
    }

    pub fn try_new(
        hawkes_kernel_parameters: HawkesKernelParameters,
        news_process_configuration: NewsProcessConfiguration,
        nig_shared_plus_shift_configuration: NigSharedPlusShiftConfiguration,
    ) -> Result<Self, VanillaOptionPricingModelParametersError> {
        let model_parameters = Self::new(
            hawkes_kernel_parameters,
            news_process_configuration,
            nig_shared_plus_shift_configuration,
        );

        model_parameters.validate()?;

        Ok(model_parameters)
    }

    pub fn validate(&self) -> Result<(), VanillaOptionPricingModelParametersError> {
        HawkesKernelParameters::try_new(
            self.hawkes_kernel_parameters.lambda_bar,
            self.hawkes_kernel_parameters.beta,
            self.hawkes_kernel_parameters.alpha,
            self.hawkes_kernel_parameters.rho,
        )
        .map_err(|source| {
            VanillaOptionPricingModelParametersError::InvalidHawkesKernelParameters { source }
        })?;

        self.news_process_configuration
            .validate()
            .map_err(|source| {
                VanillaOptionPricingModelParametersError::InvalidNewsProcessConfiguration { source }
            })?;

        self.nig_shared_plus_shift_configuration
            .derive_driver_parameters()
            .map_err(|source| {
                VanillaOptionPricingModelParametersError::InvalidNigSharedPlusShiftConfiguration {
                    source,
                }
            })?;

        Ok(())
    }
}

/// Validation errors for public model-parameter payloads.
#[derive(Debug, Clone, PartialEq)]
pub enum VanillaOptionPricingModelParametersError {
    InvalidHawkesKernelParameters {
        source: HawkesKernelParameterError,
    },
    InvalidNewsProcessConfiguration {
        source: NewsProcessConfigurationError,
    },
    InvalidNigSharedPlusShiftConfiguration {
        source: NigSharedPlusShiftConfigurationError,
    },
}

impl Display for VanillaOptionPricingModelParametersError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidHawkesKernelParameters { source } => write!(
                formatter,
                "model_parameters.hawkes_kernel_parameters is invalid: {source}"
            ),
            Self::InvalidNewsProcessConfiguration { source } => write!(
                formatter,
                "model_parameters.news_process_configuration is invalid: {source}"
            ),
            Self::InvalidNigSharedPlusShiftConfiguration { source } => write!(
                formatter,
                "model_parameters.nig_shared_plus_shift_configuration is invalid: {source}"
            ),
        }
    }
}

impl Error for VanillaOptionPricingModelParametersError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidHawkesKernelParameters { source } => Some(source),
            Self::InvalidNewsProcessConfiguration { source } => Some(source),
            Self::InvalidNigSharedPlusShiftConfiguration { source } => Some(source),
        }
    }
}

/// Canonical augmented boundary input for one anchored pricing interval.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AnchoredVanillaOptionPricingBoundaryInput {
    pub interval_index: usize,
    pub hawkes_boundary_state: HawkesExcitationState,
    pub mean_reversion_fitted_parameters: MeanReversionFittedParameters,
    pub interval_delta: f64,
    pub observed_boundary_state: MeanReversionBoundaryState,
}

impl AnchoredVanillaOptionPricingBoundaryInput {
    pub const fn new(
        interval_index: usize,
        hawkes_boundary_state: HawkesExcitationState,
        mean_reversion_fitted_parameters: MeanReversionFittedParameters,
        interval_delta: f64,
        observed_boundary_state: MeanReversionBoundaryState,
    ) -> Self {
        Self {
            interval_index,
            hawkes_boundary_state,
            mean_reversion_fitted_parameters,
            interval_delta,
            observed_boundary_state,
        }
    }

    pub fn try_new(
        interval_index: usize,
        hawkes_boundary_state: HawkesExcitationState,
        mean_reversion_fitted_parameters: MeanReversionFittedParameters,
        interval_delta: f64,
        observed_boundary_state: MeanReversionBoundaryState,
    ) -> Result<Self, AnchoredVanillaOptionPricingBoundaryInputError> {
        let boundary_input = Self::new(
            interval_index,
            hawkes_boundary_state,
            mean_reversion_fitted_parameters,
            interval_delta,
            observed_boundary_state,
        );

        boundary_input.validate()?;

        Ok(boundary_input)
    }

    pub fn validate(&self) -> Result<(), AnchoredVanillaOptionPricingBoundaryInputError> {
        validate_hawkes_boundary_state(self.hawkes_boundary_state).map_err(|(field, value)| {
            AnchoredVanillaOptionPricingBoundaryInputError::InvalidHawkesBoundaryStateComponent {
                field,
                value,
            }
        })?;

        self.mean_reversion_fitted_parameters
            .validate()
            .map_err(|source| {
                AnchoredVanillaOptionPricingBoundaryInputError::InvalidMeanReversionFittedParameters {
                    source,
                }
            })?;

        self.observed_boundary_state.validate().map_err(|source| {
            AnchoredVanillaOptionPricingBoundaryInputError::InvalidObservedBoundaryState { source }
        })?;

        if !self.interval_delta.is_finite() || self.interval_delta <= 0.0 {
            return Err(
                AnchoredVanillaOptionPricingBoundaryInputError::InvalidIntervalDelta {
                    value: self.interval_delta,
                },
            );
        }

        validate_observed_anchor_alignment(
            &self.mean_reversion_fitted_parameters,
            self.observed_boundary_state.clone(),
        )?;

        Ok(())
    }

    pub fn current_anchor_value(
        &self,
    ) -> Result<f64, AnchoredVanillaOptionPricingBoundaryInputError> {
        Ok(self.observed_boundary_state.anchor_value)
    }

    pub fn interval_delta(&self) -> Result<f64, AnchoredVanillaOptionPricingBoundaryInputError> {
        Ok(self.interval_delta)
    }

    pub fn deviation_decay_factor(
        &self,
    ) -> Result<f64, AnchoredVanillaOptionPricingBoundaryInputError> {
        let interval_delta = self.interval_delta()?;
        Ok((-self.mean_reversion_fitted_parameters.kappa * interval_delta).exp())
    }

    pub fn local_martingale_deviation_adjustment(
        &self,
    ) -> Result<f64, AnchoredVanillaOptionPricingBoundaryInputError> {
        let deviation_decay_factor = self.deviation_decay_factor()?;

        Ok((deviation_decay_factor - 1.0) * self.observed_boundary_state.deviation)
    }

    pub fn hawkes_boundary_state_norm(&self) -> f64 {
        self.hawkes_boundary_state.y_plus_plus
            + self.hawkes_boundary_state.y_plus_minus
            + self.hawkes_boundary_state.y_minus_plus
            + self.hawkes_boundary_state.y_minus_minus
    }
}

/// Structured validation failures for anchored pricing boundary input.
#[derive(Debug, Clone, PartialEq)]
pub enum AnchoredVanillaOptionPricingBoundaryInputError {
    InvalidHawkesBoundaryStateComponent {
        field: &'static str,
        value: f64,
    },
    InvalidMeanReversionFittedParameters {
        source: MeanReversionFittedParametersValidationError,
    },
    InvalidObservedBoundaryState {
        source: MeanReversionBoundaryStateValidationError,
    },
    InvalidIntervalDelta {
        value: f64,
    },
}

impl Display for AnchoredVanillaOptionPricingBoundaryInputError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidHawkesBoundaryStateComponent { field, value } => write!(
                formatter,
                "boundary_input.hawkes_boundary_state.{field} must be finite and non-negative, found {value}"
            ),
            Self::InvalidMeanReversionFittedParameters { source } => write!(
                formatter,
                "boundary_input.mean_reversion_fitted_parameters is invalid: {source}"
            ),
            Self::InvalidObservedBoundaryState { source } => write!(
                formatter,
                "boundary_input.observed_boundary_state is invalid: {source}"
            ),
            Self::InvalidIntervalDelta { value } => write!(
                formatter,
                "boundary_input.interval_delta must be finite and strictly positive, found {value}"
            ),
        }
    }
}

impl Error for AnchoredVanillaOptionPricingBoundaryInputError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidMeanReversionFittedParameters { source } => Some(source),
            Self::InvalidObservedBoundaryState { source } => Some(source),
            _ => None,
        }
    }
}

/// Domain-status classification for one anchored local tilt solve.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnchoredLocalTiltDomainStatus {
    InDomain,
    NonFiniteTransformInput,
    OutOfDomainStateRequest,
}

/// Admissibility classification for one anchored local tilt solve.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnchoredLocalTiltAdmissibilityStatus {
    Admissible,
    NoAdmissibleLocalTilt,
}

/// Deterministic boundary-state summary attached to anchored local tilt diagnostics.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnchoredLocalTiltBoundaryStateSummary {
    pub interval_index: usize,
    pub hawkes_boundary_state: HawkesExcitationState,
    pub observed_boundary_state: MeanReversionBoundaryState,
    pub current_anchor_value: f64,
    pub interval_delta: f64,
    pub kappa: f64,
    pub deviation_decay_factor: f64,
    pub local_martingale_deviation_adjustment: f64,
}

/// Deterministic diagnostics for one local anchored pricing tilt solve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnchoredLocalTiltSolveDiagnostics {
    pub boundary_state_summary: AnchoredLocalTiltBoundaryStateSummary,
    pub target_log_moment_exponent: f64,
    pub observed_log_moment_exponent: f64,
    pub theta: f64,
    pub residual: f64,
    pub absolute_residual: f64,
    pub tolerance: f64,
    pub domain_status: AnchoredLocalTiltDomainStatus,
    pub admissibility_status: AnchoredLocalTiltAdmissibilityStatus,
    pub policy: String,
}

/// Deterministic diagnostics for one interval in the anchored recursion.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnchoredPricingRecursionIntervalDiagnostics {
    pub interval_index: usize,
    pub current_anchor_value: f64,
    pub next_anchor_value: f64,
    pub interval_delta: f64,
    pub expected_hawkes_boundary_state_before: HawkesExcitationState,
    pub expected_hawkes_boundary_state_after: HawkesExcitationState,
    pub maximum_tail_probability_mass: f64,
    pub maximum_row_probability_mass_error: f64,
    pub minimum_solved_theta: f64,
    pub maximum_solved_theta: f64,
    pub local_tilt_solve_diagnostics: Vec<AnchoredLocalTiltSolveDiagnostics>,
}

impl AnchoredPricingRecursionIntervalDiagnostics {
    #[allow(clippy::too_many_arguments)]
    pub fn try_new(
        interval_index: usize,
        current_anchor_value: f64,
        next_anchor_value: f64,
        interval_delta: f64,
        expected_hawkes_boundary_state_before: HawkesExcitationState,
        expected_hawkes_boundary_state_after: HawkesExcitationState,
        maximum_tail_probability_mass: f64,
        maximum_row_probability_mass_error: f64,
        minimum_solved_theta: f64,
        maximum_solved_theta: f64,
        local_tilt_solve_diagnostics: Vec<AnchoredLocalTiltSolveDiagnostics>,
    ) -> Result<Self, AnchoredPricingRecursionDiagnosticsError> {
        let diagnostics = Self {
            interval_index,
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
        };

        diagnostics.validate()?;

        Ok(diagnostics)
    }

    pub fn validate(&self) -> Result<(), AnchoredPricingRecursionDiagnosticsError> {
        validate_hawkes_boundary_state(self.expected_hawkes_boundary_state_before).map_err(
            |(field, value)| {
                AnchoredPricingRecursionDiagnosticsError::InvalidExpectedHawkesBoundaryState {
                    field,
                    value,
                }
            },
        )?;
        validate_hawkes_boundary_state(self.expected_hawkes_boundary_state_after).map_err(
            |(field, value)| {
                AnchoredPricingRecursionDiagnosticsError::InvalidExpectedHawkesBoundaryState {
                    field,
                    value,
                }
            },
        )?;
        validate_finite_field(self.current_anchor_value, "current_anchor_value")?;
        validate_finite_field(self.next_anchor_value, "next_anchor_value")?;
        validate_positive_field(self.interval_delta, "interval_delta")?;
        validate_non_negative_field(
            self.maximum_tail_probability_mass,
            "maximum_tail_probability_mass",
        )?;
        validate_non_negative_field(
            self.maximum_row_probability_mass_error,
            "maximum_row_probability_mass_error",
        )?;
        validate_finite_field(self.minimum_solved_theta, "minimum_solved_theta")?;
        validate_finite_field(self.maximum_solved_theta, "maximum_solved_theta")?;

        if self.minimum_solved_theta > self.maximum_solved_theta {
            return Err(
                AnchoredPricingRecursionDiagnosticsError::InvalidThetaOrdering {
                    minimum_solved_theta: self.minimum_solved_theta,
                    maximum_solved_theta: self.maximum_solved_theta,
                },
            );
        }

        Ok(())
    }
}

/// Deterministic diagnostics for the anchored deviation-grid recursion.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnchoredPricingRecursionDiagnostics {
    pub deviation_grid_policy: String,
    pub continuation_recursion_policy: String,
    pub expected_hawkes_state_update_policy: String,
    pub deviation_grid_lower_bound: f64,
    pub deviation_grid_upper_bound: f64,
    pub deviation_grid_node_count: usize,
    pub refined_deviation_grid_node_count: usize,
    pub refinement_absolute_difference: f64,
    pub refinement_relative_difference: f64,
    pub interval_diagnostics: Vec<AnchoredPricingRecursionIntervalDiagnostics>,
}

impl AnchoredPricingRecursionDiagnostics {
    pub fn try_new(
        deviation_grid_lower_bound: f64,
        deviation_grid_upper_bound: f64,
        deviation_grid_node_count: usize,
        refined_deviation_grid_node_count: usize,
        refinement_absolute_difference: f64,
        refinement_relative_difference: f64,
        interval_diagnostics: Vec<AnchoredPricingRecursionIntervalDiagnostics>,
    ) -> Result<Self, AnchoredPricingRecursionDiagnosticsError> {
        let diagnostics = Self {
            deviation_grid_policy: ANCHORED_PRICING_DEVIATION_GRID_POLICY.to_string(),
            continuation_recursion_policy: ANCHORED_PRICING_CONTINUATION_RECURSION_POLICY
                .to_string(),
            expected_hawkes_state_update_policy:
                ANCHORED_PRICING_EXPECTED_HAWKES_STATE_UPDATE_POLICY.to_string(),
            deviation_grid_lower_bound,
            deviation_grid_upper_bound,
            deviation_grid_node_count,
            refined_deviation_grid_node_count,
            refinement_absolute_difference,
            refinement_relative_difference,
            interval_diagnostics,
        };

        diagnostics.validate()?;

        Ok(diagnostics)
    }

    pub fn validate(&self) -> Result<(), AnchoredPricingRecursionDiagnosticsError> {
        if self.deviation_grid_policy.trim().is_empty() {
            return Err(AnchoredPricingRecursionDiagnosticsError::EmptyPolicy {
                field: "deviation_grid_policy",
            });
        }
        if self.continuation_recursion_policy.trim().is_empty() {
            return Err(AnchoredPricingRecursionDiagnosticsError::EmptyPolicy {
                field: "continuation_recursion_policy",
            });
        }
        if self.expected_hawkes_state_update_policy.trim().is_empty() {
            return Err(AnchoredPricingRecursionDiagnosticsError::EmptyPolicy {
                field: "expected_hawkes_state_update_policy",
            });
        }

        validate_finite_field(
            self.deviation_grid_lower_bound,
            "deviation_grid_lower_bound",
        )?;
        validate_finite_field(
            self.deviation_grid_upper_bound,
            "deviation_grid_upper_bound",
        )?;

        if self.deviation_grid_lower_bound >= self.deviation_grid_upper_bound {
            return Err(
                AnchoredPricingRecursionDiagnosticsError::InvalidGridBounds {
                    lower_bound: self.deviation_grid_lower_bound,
                    upper_bound: self.deviation_grid_upper_bound,
                },
            );
        }
        if self.deviation_grid_node_count == 0 {
            return Err(
                AnchoredPricingRecursionDiagnosticsError::InvalidGridNodeCount {
                    field: "deviation_grid_node_count",
                    value: self.deviation_grid_node_count,
                },
            );
        }
        if self.refined_deviation_grid_node_count < self.deviation_grid_node_count {
            return Err(
                AnchoredPricingRecursionDiagnosticsError::InvalidGridNodeCount {
                    field: "refined_deviation_grid_node_count",
                    value: self.refined_deviation_grid_node_count,
                },
            );
        }

        validate_non_negative_field(
            self.refinement_absolute_difference,
            "refinement_absolute_difference",
        )?;
        validate_non_negative_field(
            self.refinement_relative_difference,
            "refinement_relative_difference",
        )?;

        if self.interval_diagnostics.is_empty() {
            return Err(AnchoredPricingRecursionDiagnosticsError::EmptyIntervalDiagnostics);
        }

        for interval_diagnostics in &self.interval_diagnostics {
            interval_diagnostics.validate()?;
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum AnchoredPricingRecursionDiagnosticsError {
    InvalidExpectedHawkesBoundaryState {
        field: &'static str,
        value: f64,
    },
    InvalidFiniteField {
        field: &'static str,
        value: f64,
    },
    InvalidPositiveField {
        field: &'static str,
        value: f64,
    },
    InvalidNonNegativeField {
        field: &'static str,
        value: f64,
    },
    InvalidThetaOrdering {
        minimum_solved_theta: f64,
        maximum_solved_theta: f64,
    },
    EmptyPolicy {
        field: &'static str,
    },
    InvalidGridBounds {
        lower_bound: f64,
        upper_bound: f64,
    },
    InvalidGridNodeCount {
        field: &'static str,
        value: usize,
    },
    EmptyIntervalDiagnostics,
}

impl Display for AnchoredPricingRecursionDiagnosticsError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidExpectedHawkesBoundaryState { field, value } => write!(
                formatter,
                "anchored recursion expected Hawkes boundary state component {field} must be finite and non-negative, found {value}"
            ),
            Self::InvalidFiniteField { field, value } => {
                write!(formatter, "{field} must be finite, found {value}")
            }
            Self::InvalidPositiveField { field, value } => {
                write!(
                    formatter,
                    "{field} must be finite and strictly positive, found {value}"
                )
            }
            Self::InvalidNonNegativeField { field, value } => {
                write!(
                    formatter,
                    "{field} must be finite and non-negative, found {value}"
                )
            }
            Self::InvalidThetaOrdering {
                minimum_solved_theta,
                maximum_solved_theta,
            } => write!(
                formatter,
                "minimum_solved_theta must be less than or equal to maximum_solved_theta, found [{minimum_solved_theta}, {maximum_solved_theta}]"
            ),
            Self::EmptyPolicy { field } => write!(formatter, "{field} must be a non-empty string"),
            Self::InvalidGridBounds {
                lower_bound,
                upper_bound,
            } => write!(
                formatter,
                "deviation-grid lower bound must be strictly less than upper bound, found [{lower_bound}, {upper_bound}]"
            ),
            Self::InvalidGridNodeCount { field, value } => write!(
                formatter,
                "{field} must be at least 1 and finite by construction, found {value}"
            ),
            Self::EmptyIntervalDiagnostics => write!(
                formatter,
                "anchored recursion diagnostics must include at least one interval diagnostics record"
            ),
        }
    }
}

impl Error for AnchoredPricingRecursionDiagnosticsError {}

/// Public request contract for baseline vanilla option pricing.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VanillaOptionPricingRequest {
    pub market_inputs: VanillaOptionPricingMarketInputs,
    pub payoff: VanillaOptionPayoffConfiguration,
    pub boundary_input: AnchoredVanillaOptionPricingBoundaryInput,
    pub model_parameters: VanillaOptionPricingModelParameters,
    pub interval_delta: f64,
    pub fourier_inversion_settings: Option<FourierInversionEngineSettings>,
}

impl VanillaOptionPricingRequest {
    pub const fn new(
        market_inputs: VanillaOptionPricingMarketInputs,
        payoff: VanillaOptionPayoffConfiguration,
        boundary_input: AnchoredVanillaOptionPricingBoundaryInput,
        model_parameters: VanillaOptionPricingModelParameters,
    ) -> Self {
        Self::new_with_interval_delta(
            market_inputs,
            payoff,
            boundary_input,
            model_parameters,
            DEFAULT_INTRADAY_PRICING_INTERVAL_DELTA,
        )
    }

    pub const fn new_with_interval_delta(
        market_inputs: VanillaOptionPricingMarketInputs,
        payoff: VanillaOptionPayoffConfiguration,
        boundary_input: AnchoredVanillaOptionPricingBoundaryInput,
        model_parameters: VanillaOptionPricingModelParameters,
        interval_delta: f64,
    ) -> Self {
        Self {
            market_inputs,
            payoff,
            boundary_input,
            model_parameters,
            interval_delta,
            fourier_inversion_settings: None,
        }
    }

    pub const fn with_fourier_inversion_settings(
        mut self,
        fourier_inversion_settings: FourierInversionEngineSettings,
    ) -> Self {
        self.fourier_inversion_settings = Some(fourier_inversion_settings);
        self
    }

    const fn with_fourier_inversion_settings_option(
        mut self,
        fourier_inversion_settings: Option<FourierInversionEngineSettings>,
    ) -> Self {
        self.fourier_inversion_settings = fourier_inversion_settings;
        self
    }

    pub fn try_new(
        market_inputs: VanillaOptionPricingMarketInputs,
        payoff: VanillaOptionPayoffConfiguration,
        boundary_input: AnchoredVanillaOptionPricingBoundaryInput,
        model_parameters: VanillaOptionPricingModelParameters,
    ) -> Result<Self, VanillaOptionPricingRequestError> {
        let request = Self::new(market_inputs, payoff, boundary_input, model_parameters);
        request.validate()?;

        Ok(request)
    }

    pub fn try_new_with_interval_delta(
        market_inputs: VanillaOptionPricingMarketInputs,
        payoff: VanillaOptionPayoffConfiguration,
        boundary_input: AnchoredVanillaOptionPricingBoundaryInput,
        model_parameters: VanillaOptionPricingModelParameters,
        interval_delta: f64,
    ) -> Result<Self, VanillaOptionPricingRequestError> {
        let request = Self::new_with_interval_delta(
            market_inputs,
            payoff,
            boundary_input,
            model_parameters,
            interval_delta,
        );

        request.validate()?;

        Ok(request)
    }

    pub fn validate(&self) -> Result<(), VanillaOptionPricingRequestError> {
        self.market_inputs
            .validate()
            .map_err(|source| VanillaOptionPricingRequestError::InvalidMarketInputs { source })?;

        validate_interval_delta(self.interval_delta)?;

        self.boundary_input
            .validate()
            .map_err(|source| VanillaOptionPricingRequestError::InvalidBoundaryInput { source })?;

        let anchored_interval_delta = self
            .boundary_input
            .interval_delta()
            .map_err(|source| VanillaOptionPricingRequestError::InvalidBoundaryInput { source })?;
        if (anchored_interval_delta - self.interval_delta).abs()
            > PRICING_MEAN_REVERSION_ALIGNMENT_TOLERANCE
        {
            return Err(
                VanillaOptionPricingRequestError::BoundaryIntervalDeltaMismatch {
                    requested_interval_delta: self.interval_delta,
                    observed_interval_delta: anchored_interval_delta,
                    interval_index: self.boundary_input.interval_index,
                },
            );
        }

        self.model_parameters.validate().map_err(|source| {
            VanillaOptionPricingRequestError::InvalidModelParameters { source }
        })?;

        if let Some(fourier_inversion_settings) = self.fourier_inversion_settings {
            fourier_inversion_settings.validate().map_err(|source| {
                VanillaOptionPricingRequestError::InvalidFourierInversionSettings { source }
            })?;
        }

        Ok(())
    }

    pub const fn validation_policy() -> &'static str {
        VANILLA_OPTION_REQUEST_VALIDATION_POLICY
    }

    pub fn resolved_fourier_inversion_settings(&self) -> FourierInversionEngineSettings {
        self.fourier_inversion_settings.unwrap_or_default()
    }
}

/// Validation errors for vanilla option pricing requests.
#[derive(Debug, Clone, PartialEq)]
pub enum VanillaOptionPricingRequestError {
    InvalidMarketInputs {
        source: VanillaOptionPricingMarketInputsError,
    },
    InvalidIntervalDelta {
        value: f64,
    },
    InvalidBoundaryInput {
        source: AnchoredVanillaOptionPricingBoundaryInputError,
    },
    BoundaryIntervalDeltaMismatch {
        requested_interval_delta: f64,
        observed_interval_delta: f64,
        interval_index: usize,
    },
    InvalidModelParameters {
        source: VanillaOptionPricingModelParametersError,
    },
    InvalidFourierInversionSettings {
        source: FourierInversionEngineSettingsError,
    },
}

impl Display for VanillaOptionPricingRequestError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidMarketInputs { source } => {
                write!(formatter, "market_inputs payload is invalid: {source}")
            }
            Self::InvalidIntervalDelta { value } => {
                write!(
                    formatter,
                    "interval_delta must be finite and strictly positive, found {value}"
                )
            }
            Self::InvalidBoundaryInput { source } => {
                write!(formatter, "boundary_input payload is invalid: {source}")
            }
            Self::BoundaryIntervalDeltaMismatch {
                requested_interval_delta,
                observed_interval_delta,
                interval_index,
            } => write!(
                formatter,
                "interval_delta must match boundary_input.interval_delta for interval_index={interval_index}, found request={requested_interval_delta} boundary_input={observed_interval_delta}"
            ),
            Self::InvalidModelParameters { source } => {
                write!(formatter, "model_parameters payload is invalid: {source}")
            }
            Self::InvalidFourierInversionSettings { source } => {
                write!(
                    formatter,
                    "fourier_inversion_settings payload is invalid: {source}"
                )
            }
        }
    }
}

impl Error for VanillaOptionPricingRequestError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidMarketInputs { source } => Some(source),
            Self::InvalidBoundaryInput { source } => Some(source),
            Self::InvalidModelParameters { source } => Some(source),
            Self::InvalidFourierInversionSettings { source } => Some(source),
            _ => None,
        }
    }
}

/// Public diagnostics payload returned with vanilla option prices.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct VanillaOptionPricingDiagnostics {
    pub forward_price: f64,
    pub discount_factor: f64,
    pub maturity_interval: f64,
    pub boundary_state_norm: f64,
}

impl VanillaOptionPricingDiagnostics {
    pub const fn new(
        forward_price: f64,
        discount_factor: f64,
        maturity_interval: f64,
        boundary_state_norm: f64,
    ) -> Self {
        Self {
            forward_price,
            discount_factor,
            maturity_interval,
            boundary_state_norm,
        }
    }

    pub fn try_new(
        forward_price: f64,
        discount_factor: f64,
        maturity_interval: f64,
        boundary_state_norm: f64,
    ) -> Result<Self, VanillaOptionPricingDiagnosticsError> {
        let diagnostics = Self::new(
            forward_price,
            discount_factor,
            maturity_interval,
            boundary_state_norm,
        );

        diagnostics.validate()?;

        Ok(diagnostics)
    }

    pub fn validate(self) -> Result<(), VanillaOptionPricingDiagnosticsError> {
        if !self.forward_price.is_finite() || self.forward_price <= 0.0 {
            return Err(VanillaOptionPricingDiagnosticsError::InvalidForwardPrice {
                value: self.forward_price,
            });
        }
        if !self.discount_factor.is_finite() || self.discount_factor <= 0.0 {
            return Err(
                VanillaOptionPricingDiagnosticsError::InvalidDiscountFactor {
                    value: self.discount_factor,
                },
            );
        }
        if !self.maturity_interval.is_finite() || self.maturity_interval <= 0.0 {
            return Err(
                VanillaOptionPricingDiagnosticsError::InvalidMaturityInterval {
                    value: self.maturity_interval,
                },
            );
        }
        if !self.boundary_state_norm.is_finite() || self.boundary_state_norm < 0.0 {
            return Err(
                VanillaOptionPricingDiagnosticsError::InvalidBoundaryStateNorm {
                    value: self.boundary_state_norm,
                },
            );
        }

        Ok(())
    }
}

/// Validation errors for vanilla option diagnostics payloads.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum VanillaOptionPricingDiagnosticsError {
    InvalidForwardPrice { value: f64 },
    InvalidDiscountFactor { value: f64 },
    InvalidMaturityInterval { value: f64 },
    InvalidBoundaryStateNorm { value: f64 },
}

impl Display for VanillaOptionPricingDiagnosticsError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidForwardPrice { value } => write!(
                formatter,
                "diagnostics.forward_price must be finite and strictly positive, found {value}"
            ),
            Self::InvalidDiscountFactor { value } => write!(
                formatter,
                "diagnostics.discount_factor must be finite and strictly positive, found {value}"
            ),
            Self::InvalidMaturityInterval { value } => write!(
                formatter,
                "diagnostics.maturity_interval must be finite and strictly positive, found {value}"
            ),
            Self::InvalidBoundaryStateNorm { value } => write!(
                formatter,
                "diagnostics.boundary_state_norm must be finite and non-negative, found {value}"
            ),
        }
    }
}

impl Error for VanillaOptionPricingDiagnosticsError {}

/// Public response contract for baseline vanilla option pricing.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct VanillaOptionPricingResponse {
    pub price: f64,
    pub diagnostics: VanillaOptionPricingDiagnostics,
    pub implied_volatility: Option<f64>,
    pub implied_volatility_inversion_status: VanillaOptionImpliedVolatilityInversionStatus,
}

impl VanillaOptionPricingResponse {
    pub const fn new(
        price: f64,
        diagnostics: VanillaOptionPricingDiagnostics,
        implied_volatility: Option<f64>,
        implied_volatility_inversion_status: VanillaOptionImpliedVolatilityInversionStatus,
    ) -> Self {
        Self {
            price,
            diagnostics,
            implied_volatility,
            implied_volatility_inversion_status,
        }
    }

    pub fn try_new(
        price: f64,
        diagnostics: VanillaOptionPricingDiagnostics,
        implied_volatility: Option<f64>,
        implied_volatility_inversion_status: VanillaOptionImpliedVolatilityInversionStatus,
    ) -> Result<Self, VanillaOptionPricingResponseError> {
        let response = Self::new(
            price,
            diagnostics,
            implied_volatility,
            implied_volatility_inversion_status,
        );

        response.validate()?;

        Ok(response)
    }

    pub fn validate(self) -> Result<(), VanillaOptionPricingResponseError> {
        if !self.price.is_finite() || self.price < 0.0 {
            return Err(VanillaOptionPricingResponseError::InvalidPrice { value: self.price });
        }

        self.diagnostics
            .validate()
            .map_err(|source| VanillaOptionPricingResponseError::InvalidDiagnostics { source })?;

        if let Some(implied_volatility) = self.implied_volatility {
            if !implied_volatility.is_finite() || implied_volatility < 0.0 {
                return Err(
                    VanillaOptionPricingResponseError::InvalidImpliedVolatility {
                        value: implied_volatility,
                    },
                );
            }
        }

        match (
            self.implied_volatility,
            self.implied_volatility_inversion_status,
        ) {
            (Some(_), VanillaOptionImpliedVolatilityInversionStatus::Succeeded) => {}
            (None, VanillaOptionImpliedVolatilityInversionStatus::Succeeded) => {
                return Err(
                    VanillaOptionPricingResponseError::MissingImpliedVolatilityForSuccessfulInversion,
                );
            }
            (Some(_), status) => {
                return Err(
                    VanillaOptionPricingResponseError::UnexpectedImpliedVolatilityForFailedInversion {
                        status,
                    },
                );
            }
            (None, _) => {}
        }

        Ok(())
    }

    pub const fn validation_policy() -> &'static str {
        VANILLA_OPTION_RESPONSE_VALIDATION_POLICY
    }
}

/// Validation errors for vanilla option pricing responses.
#[derive(Debug, Clone, PartialEq)]
pub enum VanillaOptionPricingResponseError {
    InvalidPrice {
        value: f64,
    },
    InvalidDiagnostics {
        source: VanillaOptionPricingDiagnosticsError,
    },
    InvalidImpliedVolatility {
        value: f64,
    },
    MissingImpliedVolatilityForSuccessfulInversion,
    UnexpectedImpliedVolatilityForFailedInversion {
        status: VanillaOptionImpliedVolatilityInversionStatus,
    },
}

impl Display for VanillaOptionPricingResponseError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidPrice { value } => write!(
                formatter,
                "price must be finite and non-negative, found {value}"
            ),
            Self::InvalidDiagnostics { source } => {
                write!(formatter, "diagnostics payload is invalid: {source}")
            }
            Self::InvalidImpliedVolatility { value } => write!(
                formatter,
                "implied_volatility must be finite and non-negative when present, found {value}"
            ),
            Self::MissingImpliedVolatilityForSuccessfulInversion => write!(
                formatter,
                "implied_volatility must be present when implied_volatility_inversion_status=succeeded"
            ),
            Self::UnexpectedImpliedVolatilityForFailedInversion { status } => write!(
                formatter,
                "implied_volatility must be omitted when implied_volatility_inversion_status={status:?}"
            ),
        }
    }
}

impl Error for VanillaOptionPricingResponseError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidDiagnostics { source } => Some(source),
            _ => None,
        }
    }
}

/// Selection controls for optional pricing run outputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VanillaOptionPricingOptionalOutputSelection {
    #[serde(default)]
    pub include_transform_diagnostics: bool,
    #[serde(default)]
    pub include_integration_quality_diagnostics: bool,
    #[serde(default)]
    pub include_calibration_hooks: bool,
}

impl VanillaOptionPricingOptionalOutputSelection {
    pub const fn new(
        include_transform_diagnostics: bool,
        include_integration_quality_diagnostics: bool,
        include_calibration_hooks: bool,
    ) -> Self {
        Self {
            include_transform_diagnostics,
            include_integration_quality_diagnostics,
            include_calibration_hooks,
        }
    }

    pub const fn diagnostics_selected(self) -> bool {
        self.include_transform_diagnostics || self.include_integration_quality_diagnostics
    }
}

impl Default for VanillaOptionPricingOptionalOutputSelection {
    fn default() -> Self {
        Self::new(false, false, false)
    }
}

/// Machine-readable schema metadata for pricing run optional outputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct VanillaOptionPricingRunOutputSchemaMetadata {
    pub schema_version: &'static str,
    pub run_result_schema_version: &'static str,
    pub run_diagnostics_schema_version: &'static str,
    pub calibration_hooks_schema_version: &'static str,
    pub required_run_result_fields: &'static [&'static str],
    pub required_run_diagnostics_fields: &'static [&'static str],
    pub required_calibration_hooks_fields: &'static [&'static str],
}

pub const VANILLA_OPTION_PRICING_RUN_OUTPUT_SCHEMA_METADATA:
    VanillaOptionPricingRunOutputSchemaMetadata = VanillaOptionPricingRunOutputSchemaMetadata {
    schema_version: VANILLA_OPTION_PRICING_RUN_OUTPUT_SCHEMA_METADATA_VERSION,
    run_result_schema_version: VANILLA_OPTION_PRICING_RUN_RESULT_SCHEMA_VERSION,
    run_diagnostics_schema_version: VANILLA_OPTION_PRICING_RUN_DIAGNOSTICS_SCHEMA_VERSION,
    calibration_hooks_schema_version: VANILLA_OPTION_PRICING_CALIBRATION_HOOKS_SCHEMA_VERSION,
    required_run_result_fields: &VANILLA_OPTION_PRICING_RUN_RESULT_REQUIRED_FIELDS,
    required_run_diagnostics_fields: &VANILLA_OPTION_PRICING_RUN_DIAGNOSTICS_REQUIRED_FIELDS,
    required_calibration_hooks_fields: &VANILLA_OPTION_PRICING_CALIBRATION_HOOKS_REQUIRED_FIELDS,
};

/// Optional run diagnostics for one pricing execution.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VanillaOptionPricingRunDiagnostics {
    pub schema_version: &'static str,
    pub transform: Option<ExponentialAffineTransformIntegrationResult>,
    pub integration_quality: Option<FourierInversionDiagnostics>,
    pub anchored_recursion: Option<AnchoredPricingRecursionDiagnostics>,
}

impl VanillaOptionPricingRunDiagnostics {
    pub fn new(
        transform: Option<ExponentialAffineTransformIntegrationResult>,
        integration_quality: Option<FourierInversionDiagnostics>,
        anchored_recursion: Option<AnchoredPricingRecursionDiagnostics>,
    ) -> Self {
        Self {
            schema_version: VANILLA_OPTION_PRICING_RUN_DIAGNOSTICS_SCHEMA_VERSION,
            transform,
            integration_quality,
            anchored_recursion,
        }
    }

    pub fn try_new(
        transform: Option<ExponentialAffineTransformIntegrationResult>,
        integration_quality: Option<FourierInversionDiagnostics>,
        anchored_recursion: Option<AnchoredPricingRecursionDiagnostics>,
    ) -> Result<Self, VanillaOptionPricingRunDiagnosticsError> {
        let diagnostics = Self::new(transform, integration_quality, anchored_recursion);

        diagnostics.validate()?;

        Ok(diagnostics)
    }

    pub fn validate(&self) -> Result<(), VanillaOptionPricingRunDiagnosticsError> {
        if self.transform.is_none()
            && self.integration_quality.is_none()
            && self.anchored_recursion.is_none()
        {
            return Err(VanillaOptionPricingRunDiagnosticsError::EmptyDiagnosticsPayload);
        }

        if let Some(anchored_recursion) = &self.anchored_recursion {
            anchored_recursion.validate().map_err(|source| {
                VanillaOptionPricingRunDiagnosticsError::InvalidAnchoredRecursion { source }
            })?;
        }

        Ok(())
    }
}

/// Validation errors for optional pricing run diagnostics.
#[derive(Debug, Clone, PartialEq)]
pub enum VanillaOptionPricingRunDiagnosticsError {
    EmptyDiagnosticsPayload,
    InvalidAnchoredRecursion {
        source: AnchoredPricingRecursionDiagnosticsError,
    },
}

impl Display for VanillaOptionPricingRunDiagnosticsError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyDiagnosticsPayload => write!(
                formatter,
                "run diagnostics payload must include transform diagnostics, integration diagnostics, anchored recursion diagnostics, or any non-empty combination of them"
            ),
            Self::InvalidAnchoredRecursion { source } => {
                write!(
                    formatter,
                    "anchored recursion diagnostics payload is invalid: {source}"
                )
            }
        }
    }
}

impl Error for VanillaOptionPricingRunDiagnosticsError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidAnchoredRecursion { source } => Some(source),
            Self::EmptyDiagnosticsPayload => None,
        }
    }
}

/// Optional calibration-facing hook payload for one pricing execution.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct VanillaOptionPricingCalibrationHooks {
    pub schema_version: &'static str,
    pub hook_policy: &'static str,
    pub option_price: f64,
    pub log_moneyness: f64,
    pub forward_moneyness: f64,
    pub maturity_interval: f64,
    pub transform_status: Option<ExponentialAffineTransformIntegrationStatus>,
    pub integration_numeric_quality_is_poor: Option<bool>,
    pub integration_convergence_check_passed: Option<bool>,
    pub integration_truncation_check_passed: Option<bool>,
}

impl VanillaOptionPricingCalibrationHooks {
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        option_price: f64,
        log_moneyness: f64,
        forward_moneyness: f64,
        maturity_interval: f64,
        transform_status: Option<ExponentialAffineTransformIntegrationStatus>,
        integration_numeric_quality_is_poor: Option<bool>,
        integration_convergence_check_passed: Option<bool>,
        integration_truncation_check_passed: Option<bool>,
    ) -> Self {
        Self {
            schema_version: VANILLA_OPTION_PRICING_CALIBRATION_HOOKS_SCHEMA_VERSION,
            hook_policy: VANILLA_OPTION_CALIBRATION_HOOK_POLICY,
            option_price,
            log_moneyness,
            forward_moneyness,
            maturity_interval,
            transform_status,
            integration_numeric_quality_is_poor,
            integration_convergence_check_passed,
            integration_truncation_check_passed,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn try_new(
        option_price: f64,
        log_moneyness: f64,
        forward_moneyness: f64,
        maturity_interval: f64,
        transform_status: Option<ExponentialAffineTransformIntegrationStatus>,
        integration_numeric_quality_is_poor: Option<bool>,
        integration_convergence_check_passed: Option<bool>,
        integration_truncation_check_passed: Option<bool>,
    ) -> Result<Self, VanillaOptionPricingCalibrationHooksError> {
        let hooks = Self::new(
            option_price,
            log_moneyness,
            forward_moneyness,
            maturity_interval,
            transform_status,
            integration_numeric_quality_is_poor,
            integration_convergence_check_passed,
            integration_truncation_check_passed,
        );

        hooks.validate()?;

        Ok(hooks)
    }

    pub fn from_pricing_response_with_optional_diagnostics(
        response: VanillaOptionPricingResponse,
        market_inputs: VanillaOptionPricingMarketInputs,
        transform: Option<ExponentialAffineTransformIntegrationResult>,
        integration_quality: Option<FourierInversionDiagnostics>,
    ) -> Result<Self, VanillaOptionPricingCalibrationHooksError> {
        response.validate().map_err(|source| {
            VanillaOptionPricingCalibrationHooksError::InvalidResponse { source }
        })?;
        market_inputs.validate().map_err(|source| {
            VanillaOptionPricingCalibrationHooksError::InvalidMarketInputs { source }
        })?;

        let log_moneyness = (market_inputs.strike / market_inputs.spot).ln();
        let forward_moneyness = response.diagnostics.forward_price / market_inputs.strike;

        let integration_quality_diagnostics =
            integration_quality.map(|diagnostics| diagnostics.numeric_quality_diagnostics);

        Self::try_new(
            response.price,
            log_moneyness,
            forward_moneyness,
            response.diagnostics.maturity_interval,
            transform.map(|diagnostics| diagnostics.status),
            integration_quality_diagnostics.map(|diagnostics| diagnostics.numeric_quality_is_poor),
            integration_quality_diagnostics.map(|diagnostics| diagnostics.convergence_check_passed),
            integration_quality_diagnostics.map(|diagnostics| diagnostics.truncation_check_passed),
        )
    }

    pub fn validate(self) -> Result<(), VanillaOptionPricingCalibrationHooksError> {
        if !self.option_price.is_finite() || self.option_price < 0.0 {
            return Err(
                VanillaOptionPricingCalibrationHooksError::InvalidOptionPrice {
                    value: self.option_price,
                },
            );
        }
        if !self.log_moneyness.is_finite() {
            return Err(
                VanillaOptionPricingCalibrationHooksError::InvalidLogMoneyness {
                    value: self.log_moneyness,
                },
            );
        }
        if !self.forward_moneyness.is_finite() || self.forward_moneyness <= 0.0 {
            return Err(
                VanillaOptionPricingCalibrationHooksError::InvalidForwardMoneyness {
                    value: self.forward_moneyness,
                },
            );
        }
        if !self.maturity_interval.is_finite() || self.maturity_interval <= 0.0 {
            return Err(
                VanillaOptionPricingCalibrationHooksError::InvalidMaturityInterval {
                    value: self.maturity_interval,
                },
            );
        }

        Ok(())
    }
}

/// Validation errors for calibration-facing pricing hook payloads.
#[derive(Debug, Clone, PartialEq)]
pub enum VanillaOptionPricingCalibrationHooksError {
    InvalidResponse {
        source: VanillaOptionPricingResponseError,
    },
    InvalidMarketInputs {
        source: VanillaOptionPricingMarketInputsError,
    },
    InvalidOptionPrice {
        value: f64,
    },
    InvalidLogMoneyness {
        value: f64,
    },
    InvalidForwardMoneyness {
        value: f64,
    },
    InvalidMaturityInterval {
        value: f64,
    },
}

impl Display for VanillaOptionPricingCalibrationHooksError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidResponse { source } => {
                write!(
                    formatter,
                    "response payload is invalid for calibration hooks: {source}"
                )
            }
            Self::InvalidMarketInputs { source } => write!(
                formatter,
                "market inputs are invalid for calibration hooks: {source}"
            ),
            Self::InvalidOptionPrice { value } => write!(
                formatter,
                "calibration hook option_price must be finite and non-negative, found {value}"
            ),
            Self::InvalidLogMoneyness { value } => write!(
                formatter,
                "calibration hook log_moneyness must be finite, found {value}"
            ),
            Self::InvalidForwardMoneyness { value } => write!(
                formatter,
                "calibration hook forward_moneyness must be finite and strictly positive, found {value}"
            ),
            Self::InvalidMaturityInterval { value } => write!(
                formatter,
                "calibration hook maturity_interval must be finite and strictly positive, found {value}"
            ),
        }
    }
}

impl Error for VanillaOptionPricingCalibrationHooksError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidResponse { source } => Some(source),
            Self::InvalidMarketInputs { source } => Some(source),
            _ => None,
        }
    }
}

/// Optional run output payload for baseline vanilla option pricing.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VanillaOptionPricingRunResult {
    pub schema_version: &'static str,
    pub output_schema_metadata: VanillaOptionPricingRunOutputSchemaMetadata,
    pub response: VanillaOptionPricingResponse,
    pub run_diagnostics: Option<VanillaOptionPricingRunDiagnostics>,
    pub calibration_hooks: Option<VanillaOptionPricingCalibrationHooks>,
}

impl VanillaOptionPricingRunResult {
    pub const fn new(
        response: VanillaOptionPricingResponse,
        run_diagnostics: Option<VanillaOptionPricingRunDiagnostics>,
        calibration_hooks: Option<VanillaOptionPricingCalibrationHooks>,
    ) -> Self {
        Self {
            schema_version: VANILLA_OPTION_PRICING_RUN_RESULT_SCHEMA_VERSION,
            output_schema_metadata: VANILLA_OPTION_PRICING_RUN_OUTPUT_SCHEMA_METADATA,
            response,
            run_diagnostics,
            calibration_hooks,
        }
    }

    pub fn try_new(
        response: VanillaOptionPricingResponse,
        run_diagnostics: Option<VanillaOptionPricingRunDiagnostics>,
        calibration_hooks: Option<VanillaOptionPricingCalibrationHooks>,
    ) -> Result<Self, VanillaOptionPricingRunResultError> {
        let run_result = Self::new(response, run_diagnostics, calibration_hooks);

        run_result.validate()?;

        Ok(run_result)
    }

    pub fn validate(&self) -> Result<(), VanillaOptionPricingRunResultError> {
        self.response
            .validate()
            .map_err(|source| VanillaOptionPricingRunResultError::InvalidResponse { source })?;

        if let Some(run_diagnostics) = &self.run_diagnostics {
            run_diagnostics.validate().map_err(|source| {
                VanillaOptionPricingRunResultError::InvalidRunDiagnostics { source }
            })?;
        }
        if let Some(calibration_hooks) = self.calibration_hooks {
            calibration_hooks.validate().map_err(|source| {
                VanillaOptionPricingRunResultError::InvalidCalibrationHooks { source }
            })?;
        }

        Ok(())
    }
}

/// Validation errors for pricing run outputs.
#[derive(Debug, Clone, PartialEq)]
pub enum VanillaOptionPricingRunResultError {
    InvalidResponse {
        source: VanillaOptionPricingResponseError,
    },
    InvalidRunDiagnostics {
        source: VanillaOptionPricingRunDiagnosticsError,
    },
    InvalidCalibrationHooks {
        source: VanillaOptionPricingCalibrationHooksError,
    },
}

impl Display for VanillaOptionPricingRunResultError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidResponse { source } => {
                write!(
                    formatter,
                    "response payload is invalid for run output: {source}"
                )
            }
            Self::InvalidRunDiagnostics { source } => {
                write!(formatter, "run diagnostics payload is invalid: {source}")
            }
            Self::InvalidCalibrationHooks { source } => {
                write!(formatter, "calibration hooks payload is invalid: {source}")
            }
        }
    }
}

impl Error for VanillaOptionPricingRunResultError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidResponse { source } => Some(source),
            Self::InvalidRunDiagnostics { source } => Some(source),
            Self::InvalidCalibrationHooks { source } => Some(source),
        }
    }
}

/// Build one pricing run output with optional diagnostics and calibration hooks.
pub fn build_vanilla_option_pricing_run_result(
    response: VanillaOptionPricingResponse,
    output_selection: VanillaOptionPricingOptionalOutputSelection,
    market_inputs_for_calibration_hooks: Option<VanillaOptionPricingMarketInputs>,
    transform_diagnostics_source: Option<ExponentialAffineTransformIntegrationResult>,
    integration_quality_source: Option<FourierInversionDiagnostics>,
    anchored_recursion_source: Option<AnchoredPricingRecursionDiagnostics>,
) -> Result<VanillaOptionPricingRunResult, VanillaOptionPricingRunResultBuildError> {
    let run_diagnostics = if output_selection.diagnostics_selected() {
        if output_selection.include_transform_diagnostics && transform_diagnostics_source.is_none()
        {
            return Err(VanillaOptionPricingRunResultBuildError::MissingTransformDiagnosticsSource);
        }
        if output_selection.include_integration_quality_diagnostics
            && integration_quality_source.is_none()
            && anchored_recursion_source.is_none()
        {
            return Err(
                VanillaOptionPricingRunResultBuildError::MissingIntegrationQualityDiagnosticsSource,
            );
        }

        let selected_transform_diagnostics = if output_selection.include_transform_diagnostics {
            transform_diagnostics_source
        } else {
            None
        };
        let selected_integration_quality =
            if output_selection.include_integration_quality_diagnostics {
                integration_quality_source
            } else {
                None
            };

        Some(
            VanillaOptionPricingRunDiagnostics::try_new(
                selected_transform_diagnostics,
                selected_integration_quality,
                anchored_recursion_source,
            )
            .map_err(|source| {
                VanillaOptionPricingRunResultBuildError::InvalidRunDiagnostics { source }
            })?,
        )
    } else {
        None
    };

    let calibration_hooks = if output_selection.include_calibration_hooks {
        let market_inputs = market_inputs_for_calibration_hooks.ok_or(
            VanillaOptionPricingRunResultBuildError::MissingMarketInputsForCalibrationHooks,
        )?;

        Some(
            VanillaOptionPricingCalibrationHooks::from_pricing_response_with_optional_diagnostics(
                response,
                market_inputs,
                transform_diagnostics_source,
                integration_quality_source,
            )
            .map_err(|source| {
                VanillaOptionPricingRunResultBuildError::InvalidCalibrationHooks { source }
            })?,
        )
    } else {
        None
    };

    VanillaOptionPricingRunResult::try_new(response, run_diagnostics, calibration_hooks)
        .map_err(|source| VanillaOptionPricingRunResultBuildError::InvalidRunResult { source })
}

/// Structured failures for pricing run output construction.
#[derive(Debug, Clone, PartialEq)]
pub enum VanillaOptionPricingRunResultBuildError {
    MissingTransformDiagnosticsSource,
    MissingIntegrationQualityDiagnosticsSource,
    MissingMarketInputsForCalibrationHooks,
    InvalidRunDiagnostics {
        source: VanillaOptionPricingRunDiagnosticsError,
    },
    InvalidCalibrationHooks {
        source: VanillaOptionPricingCalibrationHooksError,
    },
    InvalidRunResult {
        source: VanillaOptionPricingRunResultError,
    },
}

impl Display for VanillaOptionPricingRunResultBuildError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingTransformDiagnosticsSource => write!(
                formatter,
                "transform diagnostics were requested but no transform diagnostics source was provided"
            ),
            Self::MissingIntegrationQualityDiagnosticsSource => write!(
                formatter,
                "integration diagnostics were requested but no integration diagnostics source was provided"
            ),
            Self::MissingMarketInputsForCalibrationHooks => write!(
                formatter,
                "calibration hooks were requested but no market inputs were provided"
            ),
            Self::InvalidRunDiagnostics { source } => {
                write!(formatter, "run diagnostics payload is invalid: {source}")
            }
            Self::InvalidCalibrationHooks { source } => {
                write!(formatter, "calibration hooks payload is invalid: {source}")
            }
            Self::InvalidRunResult { source } => {
                write!(formatter, "run output payload is invalid: {source}")
            }
        }
    }
}

impl Error for VanillaOptionPricingRunResultBuildError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidRunDiagnostics { source } => Some(source),
            Self::InvalidCalibrationHooks { source } => Some(source),
            Self::InvalidRunResult { source } => Some(source),
            _ => None,
        }
    }
}

fn validate_finite_field(
    value: f64,
    field: &'static str,
) -> Result<(), AnchoredPricingRecursionDiagnosticsError> {
    validate_finite_number(value, field).map_err(|source| {
        AnchoredPricingRecursionDiagnosticsError::InvalidFiniteField {
            field: source.field,
            value: source.value,
        }
    })
}

fn validate_positive_field(
    value: f64,
    field: &'static str,
) -> Result<(), AnchoredPricingRecursionDiagnosticsError> {
    validate_positive_number(value, field).map_err(|source| {
        AnchoredPricingRecursionDiagnosticsError::InvalidPositiveField {
            field: source.field,
            value: source.value,
        }
    })
}

fn validate_non_negative_field(
    value: f64,
    field: &'static str,
) -> Result<(), AnchoredPricingRecursionDiagnosticsError> {
    validate_non_negative_number(value, field).map_err(|source| {
        AnchoredPricingRecursionDiagnosticsError::InvalidNonNegativeField {
            field: source.field,
            value: source.value,
        }
    })
}

fn validate_interval_delta(interval_delta: f64) -> Result<(), VanillaOptionPricingRequestError> {
    if !interval_delta.is_finite() || interval_delta <= 0.0 {
        return Err(VanillaOptionPricingRequestError::InvalidIntervalDelta {
            value: interval_delta,
        });
    }

    Ok(())
}

fn validate_hawkes_boundary_state(
    boundary_state: HawkesExcitationState,
) -> Result<(), (&'static str, f64)> {
    validate_hawkes_boundary_state_component("y_plus_plus", boundary_state.y_plus_plus)?;
    validate_hawkes_boundary_state_component("y_plus_minus", boundary_state.y_plus_minus)?;
    validate_hawkes_boundary_state_component("y_minus_plus", boundary_state.y_minus_plus)?;
    validate_hawkes_boundary_state_component("y_minus_minus", boundary_state.y_minus_minus)?;

    Ok(())
}

fn validate_hawkes_boundary_state_component(
    field: &'static str,
    value: f64,
) -> Result<(), (&'static str, f64)> {
    if !value.is_finite() || value < 0.0 {
        return Err((field, value));
    }

    Ok(())
}

fn validate_observed_anchor_alignment(
    mean_reversion_fitted_parameters: &MeanReversionFittedParameters,
    _observed_boundary_state: MeanReversionBoundaryState,
) -> Result<(), AnchoredVanillaOptionPricingBoundaryInputError> {
    let _ = mean_reversion_fitted_parameters;

    Ok(())
}
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct VanillaOptionPricingMarketInputsPayload {
    spot: f64,
    strike: f64,
    maturity_interval: f64,
    risk_free_rate: f64,
    dividend_yield: f64,
}

impl<'de> Deserialize<'de> for VanillaOptionPricingMarketInputs {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let payload = VanillaOptionPricingMarketInputsPayload::deserialize(deserializer)?;

        Self::try_new(
            payload.spot,
            payload.strike,
            payload.maturity_interval,
            payload.risk_free_rate,
            payload.dividend_yield,
        )
        .map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct VanillaOptionPricingModelParametersPayload {
    hawkes_kernel_parameters: HawkesKernelParameters,
    news_process_configuration: NewsProcessConfiguration,
    nig_shared_plus_shift_configuration: NigSharedPlusShiftConfiguration,
}

impl<'de> Deserialize<'de> for VanillaOptionPricingModelParameters {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let payload = VanillaOptionPricingModelParametersPayload::deserialize(deserializer)?;

        Self::try_new(
            payload.hawkes_kernel_parameters,
            payload.news_process_configuration,
            payload.nig_shared_plus_shift_configuration,
        )
        .map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct AnchoredVanillaOptionPricingBoundaryInputPayload {
    interval_index: usize,
    hawkes_boundary_state: HawkesExcitationState,
    mean_reversion_fitted_parameters: MeanReversionFittedParameters,
    interval_delta: f64,
    observed_boundary_state: MeanReversionBoundaryState,
}

impl<'de> Deserialize<'de> for AnchoredVanillaOptionPricingBoundaryInput {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let payload = AnchoredVanillaOptionPricingBoundaryInputPayload::deserialize(deserializer)?;

        Self::try_new(
            payload.interval_index,
            payload.hawkes_boundary_state,
            payload.mean_reversion_fitted_parameters,
            payload.interval_delta,
            payload.observed_boundary_state,
        )
        .map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct VanillaOptionPricingRequestPayload {
    market_inputs: VanillaOptionPricingMarketInputs,
    payoff: VanillaOptionPayoffConfiguration,
    boundary_input: AnchoredVanillaOptionPricingBoundaryInput,
    model_parameters: VanillaOptionPricingModelParameters,
    #[serde(default = "default_intraday_pricing_interval_delta")]
    interval_delta: f64,
    #[serde(default)]
    fourier_inversion_settings: Option<FourierInversionEngineSettings>,
}

impl<'de> Deserialize<'de> for VanillaOptionPricingRequest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let payload = VanillaOptionPricingRequestPayload::deserialize(deserializer)?;

        let request = Self::new_with_interval_delta(
            payload.market_inputs,
            payload.payoff,
            payload.boundary_input,
            payload.model_parameters,
            payload.interval_delta,
        )
        .with_fourier_inversion_settings_option(payload.fourier_inversion_settings);

        request
            .validate()
            .map(|()| request)
            .map_err(serde::de::Error::custom)
    }
}

const fn default_intraday_pricing_interval_delta() -> f64 {
    DEFAULT_INTRADAY_PRICING_INTERVAL_DELTA
}

impl VanillaOptionPricingRequest {
    pub const fn default_interval_delta() -> f64 {
        DEFAULT_INTRADAY_PRICING_INTERVAL_DELTA
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct VanillaOptionPricingDiagnosticsPayload {
    forward_price: f64,
    discount_factor: f64,
    maturity_interval: f64,
    boundary_state_norm: f64,
}

impl<'de> Deserialize<'de> for VanillaOptionPricingDiagnostics {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let payload = VanillaOptionPricingDiagnosticsPayload::deserialize(deserializer)?;

        Self::try_new(
            payload.forward_price,
            payload.discount_factor,
            payload.maturity_interval,
            payload.boundary_state_norm,
        )
        .map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct VanillaOptionPricingResponsePayload {
    price: f64,
    diagnostics: VanillaOptionPricingDiagnostics,
    implied_volatility: Option<f64>,
    implied_volatility_inversion_status: VanillaOptionImpliedVolatilityInversionStatus,
}

impl<'de> Deserialize<'de> for VanillaOptionPricingResponse {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let payload = VanillaOptionPricingResponsePayload::deserialize(deserializer)?;
        Self::try_new(
            payload.price,
            payload.diagnostics,
            payload.implied_volatility,
            payload.implied_volatility_inversion_status,
        )
        .map_err(serde::de::Error::custom)
    }
}
