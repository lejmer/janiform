//! Risk-sampling adapters and public contracts for sufficient-statistics paths.

use std::error::Error;
use std::fmt::{Display, Formatter};

use serde::Deserialize;
use serde::Serialize;

use crate::affine_oracle::{
    ExponentialAffineTransformEquationSpecification,
    ExponentialAffineTransformEquationSpecificationError,
    ExponentialAffineTransformGuardFailureCode, ExponentialAffineTransformInput,
    ExponentialAffineTransformInputError, ExponentialAffineTransformIntegrationResult,
    ExponentialAffineTransformIntegrationStatus, ExponentialAffineTransformSolverError,
    ExponentialAffineTransformNewsIntensityInput,
    ExponentialAffineTransformNewsIntensityInputError,
    ExponentialAffineTransformSolverSettings,
    solve_exponential_affine_transform_with_optional_settings,
};
use crate::calendar::MarketState;
use crate::hawkes::{
    HawkesEventSide, HawkesExcitationState, HawkesKernelLoadings, HawkesKernelParameters,
    HawkesTransitionError, HawkesTransitionStep, evolve_hawkes_state,
};
use crate::news::SHARED_NEWS_EVENT_DISPATCH_RETURN_ROUTING_POLICY;
use crate::nig::{
    ComplexScalar, NigBuySellDriverParameters, NigSharedPlusShiftConfiguration,
    NigSharedPlusShiftConfigurationError, NigTradingTimeIncrementSamplingError,
};
use crate::randomness::{
    SimulationRandomnessFactory, SimulationRandomnessStream, SimulationRandomnessSubstreamDomain,
    SplitMix64SimulationRandomnessFactory, derive_simulation_substream_seed,
};
use crate::return_decomposition::{
    ReturnDecomposition, ReturnDecompositionInput, ReturnDecompositionValidationError,
};

pub const SUFFICIENT_STATISTICS_RISK_RUN_REQUEST_SCHEMA_VERSION: &str =
    "risk.sufficient_statistics.run_request.v1";
pub const SUFFICIENT_STATISTICS_RISK_PATH_SCHEMA_VERSION: &str =
    "risk.sufficient_statistics.path.v1";
pub const SUFFICIENT_STATISTICS_RISK_PATH_RECORD_SCHEMA_VERSION: &str =
    "risk.sufficient_statistics.path_record.v1";
pub const SUFFICIENT_STATISTICS_RISK_PATH_OUTPUT_SCHEMA_METADATA_VERSION: &str =
    "risk.sufficient_statistics.path_output_schema_metadata.v1";
pub const SUFFICIENT_STATISTICS_RISK_PATH_PROVENANCE_SCHEMA_VERSION: &str =
    "risk.sufficient_statistics.path_provenance.v1";
pub const SUFFICIENT_STATISTICS_RISK_PATH_DIAGNOSTICS_SCHEMA_VERSION: &str =
    "risk.sufficient_statistics.path_diagnostics.v1";
pub const SUFFICIENT_STATISTICS_RISK_PATH_ARTIFACT_WORKFLOW_POLICY: &str =
    "compatible_with_calibration_and_validation_artifact_workflows.v1";
pub const SUFFICIENT_STATISTICS_RISK_PATH_ENVIRONMENT_METADATA_POLICY: &str =
    "exclude_environment_specific_metadata.v1";
pub const SUFFICIENT_STATISTICS_RISK_INTERVAL_FACTOR_POLICY: &str =
    "explicit_interval_aligned_s_h_and_news_intensity_inputs_without_implicit_resampling.v1";
pub const DEFAULT_INTRADAY_RISK_INTERVAL_DELTA: f64 = 1.0;

const SUFFICIENT_STATISTICS_RISK_PATH_REQUIRED_FIELDS: [&str; 11] = [
    "schema_version",
    "run_request_schema_version",
    "output_schema_metadata",
    "provenance",
    "diagnostics",
    "seed",
    "interval_delta",
    "simulated_interval_count",
    "initial_excitation_state",
    "terminal_excitation_state",
    "records",
];
const SUFFICIENT_STATISTICS_RISK_PATH_REQUIRED_PROVENANCE_FIELDS: [&str; 10] = [
    "schema_version",
    "artifact_workflow_policy",
    "environment_metadata_policy",
    "interval_factor_policy",
    "transform_slice_sampling_policy",
    "boundary_state_update_policy",
    "interval_return_generation_policy",
    "interval_return_component_routing_policy",
    "batch_seed_partition_policy",
    "artifact_workflow_tags",
];
const SUFFICIENT_STATISTICS_RISK_PATH_REQUIRED_DIAGNOSTICS_FIELDS: [&str; 13] = [
    "schema_version",
    "simulated_interval_count",
    "first_interval_index",
    "last_interval_index",
    "cumulative_a_plus",
    "cumulative_a_minus",
    "cumulative_n_plus",
    "cumulative_n_minus",
    "cumulative_trading_time_return",
    "cumulative_open_jump_return",
    "cumulative_closed_gap_return",
    "cumulative_total_return",
    "max_abs_total_return",
];
const SUFFICIENT_STATISTICS_RISK_PATH_ARTIFACT_WORKFLOW_TAGS: [&str; 2] =
    ["calibration", "validation"];
const NEWS_INTENSITY_COMPONENT_SUM_TOLERANCE: f64 = 1.0e-12;

/// Per-interval deterministic factors used by the sufficient-statistics risk sampler.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SufficientStatisticsRiskIntervalFactors {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interval_delta: Option<f64>,
    pub s_h: Box<[f64]>,
    #[serde(skip_serializing)]
    pub nu_h: Box<[f64]>,
    pub nu_h_known: Box<[f64]>,
    pub nu_h_surprise: Box<[f64]>,
}

impl SufficientStatisticsRiskIntervalFactors {
    pub fn try_new(
        s_h: Vec<f64>,
        nu_h_known: Vec<f64>,
        nu_h_surprise: Vec<f64>,
        interval_delta: Option<f64>,
    ) -> Result<Self, SufficientStatisticsRiskIntervalFactorsError> {
        if s_h.is_empty() {
            return Err(SufficientStatisticsRiskIntervalFactorsError::MissingIntervals);
        }
        if nu_h_known.len() != s_h.len() {
            return Err(
                SufficientStatisticsRiskIntervalFactorsError::FieldLengthMismatch {
                    field: "nu_h_known",
                    expected_length: s_h.len(),
                    actual_length: nu_h_known.len(),
                },
            );
        }
        if nu_h_surprise.len() != s_h.len() {
            return Err(
                SufficientStatisticsRiskIntervalFactorsError::FieldLengthMismatch {
                    field: "nu_h_surprise",
                    expected_length: s_h.len(),
                    actual_length: nu_h_surprise.len(),
                },
            );
        }

        let nu_h: Vec<f64> = nu_h_known
            .iter()
            .zip(nu_h_surprise.iter())
            .map(|(known_nu_h, surprise_nu_h)| known_nu_h + surprise_nu_h)
            .collect();
        let interval_factors = Self {
            interval_delta,
            s_h: s_h.into_boxed_slice(),
            nu_h: nu_h.into_boxed_slice(),
            nu_h_known: nu_h_known.into_boxed_slice(),
            nu_h_surprise: nu_h_surprise.into_boxed_slice(),
        };
        interval_factors.validate()?;
        Ok(interval_factors)
    }

    pub fn validate(&self) -> Result<(), SufficientStatisticsRiskIntervalFactorsError> {
        let interval_count = self.s_h.len();
        if interval_count == 0 {
            return Err(SufficientStatisticsRiskIntervalFactorsError::MissingIntervals);
        }

        for (interval_index, value) in self.s_h.iter().copied().enumerate() {
            if !value.is_finite() || value < 0.0 {
                return Err(
                    SufficientStatisticsRiskIntervalFactorsError::InvalidSeasonalityFactor {
                        interval_index,
                        value,
                    },
                );
            }
        }

        for (interval_index, value) in self.nu_h.iter().copied().enumerate() {
            if !value.is_finite() || value < 0.0 {
                return Err(
                    SufficientStatisticsRiskIntervalFactorsError::InvalidNewsIntensityFactor {
                        interval_index,
                        value,
                    },
                );
            }
        }

        if self.nu_h_known.len() != interval_count {
            return Err(
                SufficientStatisticsRiskIntervalFactorsError::FieldLengthMismatch {
                    field: "nu_h_known",
                    expected_length: interval_count,
                    actual_length: self.nu_h_known.len(),
                },
            );
        }
        if self.nu_h_surprise.len() != interval_count {
            return Err(
                SufficientStatisticsRiskIntervalFactorsError::FieldLengthMismatch {
                    field: "nu_h_surprise",
                    expected_length: interval_count,
                    actual_length: self.nu_h_surprise.len(),
                },
            );
        }
        if self.nu_h.len() != interval_count {
            return Err(
                SufficientStatisticsRiskIntervalFactorsError::FieldLengthMismatch {
                    field: "nu_h",
                    expected_length: interval_count,
                    actual_length: self.nu_h.len(),
                },
            );
        }

        for interval_index in 0..interval_count {
            let known_nu_h = self.nu_h_known[interval_index];
            if !known_nu_h.is_finite() || known_nu_h < 0.0 {
                return Err(
                    SufficientStatisticsRiskIntervalFactorsError::InvalidKnownNewsIntensityFactor {
                        interval_index,
                        value: known_nu_h,
                    },
                );
            }

            let surprise_nu_h = self.nu_h_surprise[interval_index];
            if !surprise_nu_h.is_finite() || surprise_nu_h < 0.0 {
                return Err(
                    SufficientStatisticsRiskIntervalFactorsError::InvalidSurpriseNewsIntensityFactor {
                        interval_index,
                        value: surprise_nu_h,
                    },
                );
            }

            let total_nu_h = self.nu_h[interval_index];
            let component_sum = known_nu_h + surprise_nu_h;
            if !component_sum.is_finite()
                || (component_sum - total_nu_h).abs() > NEWS_INTENSITY_COMPONENT_SUM_TOLERANCE
            {
                return Err(
                    SufficientStatisticsRiskIntervalFactorsError::InconsistentNewsIntensityAggregation {
                        interval_index,
                        nu_h_total: total_nu_h,
                        nu_h_known: known_nu_h,
                        nu_h_surprise: surprise_nu_h,
                    },
                );
            }
        }

        if let Some(interval_delta) = self.interval_delta
            && (!interval_delta.is_finite() || interval_delta <= 0.0)
        {
            return Err(
                SufficientStatisticsRiskIntervalFactorsError::InvalidIntervalDelta {
                    value: interval_delta,
                },
            );
        }

        Ok(())
    }

    pub const fn policy() -> &'static str {
        SUFFICIENT_STATISTICS_RISK_INTERVAL_FACTOR_POLICY
    }

    pub fn resolved_interval_delta(&self) -> f64 {
        self.interval_delta
            .unwrap_or(DEFAULT_INTRADAY_RISK_INTERVAL_DELTA)
    }

    pub fn news_intensity_input_at(
        &self,
        interval_index: usize,
    ) -> ExponentialAffineTransformNewsIntensityInput {
        ExponentialAffineTransformNewsIntensityInput::try_new(
            self.nu_h_known[interval_index],
            self.nu_h_surprise[interval_index],
        )
        .expect("validated interval factors must produce valid news intensity inputs")
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SufficientStatisticsRiskIntervalFactorsError {
    MissingIntervals,
    FieldLengthMismatch {
        field: &'static str,
        expected_length: usize,
        actual_length: usize,
    },
    InvalidSeasonalityFactor {
        interval_index: usize,
        value: f64,
    },
    InvalidNewsIntensityFactor {
        interval_index: usize,
        value: f64,
    },
    InvalidKnownNewsIntensityFactor {
        interval_index: usize,
        value: f64,
    },
    InvalidSurpriseNewsIntensityFactor {
        interval_index: usize,
        value: f64,
    },
    InconsistentNewsIntensityAggregation {
        interval_index: usize,
        nu_h_total: f64,
        nu_h_known: f64,
        nu_h_surprise: f64,
    },
    InvalidIntervalDelta {
        value: f64,
    },
}

impl Display for SufficientStatisticsRiskIntervalFactorsError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingIntervals => write!(
                formatter,
                "interval_factors must provide at least one interval"
            ),
            Self::FieldLengthMismatch {
                field,
                expected_length,
                actual_length,
            } => write!(
                formatter,
                "interval_factors.{field} length must match s_h length ({expected_length}), found {actual_length}"
            ),
            Self::InvalidSeasonalityFactor {
                interval_index,
                value,
            } => write!(
                formatter,
                "interval_factors.s_h[{interval_index}] must be finite and non-negative, found {value}"
            ),
            Self::InvalidNewsIntensityFactor {
                interval_index,
                value,
            } => write!(
                formatter,
                "interval_factors.nu_h[{interval_index}] must be finite and non-negative, found {value}"
            ),
            Self::InvalidKnownNewsIntensityFactor {
                interval_index,
                value,
            } => write!(
                formatter,
                "interval_factors.nu_h_known[{interval_index}] must be finite and non-negative, found {value}"
            ),
            Self::InvalidSurpriseNewsIntensityFactor {
                interval_index,
                value,
            } => write!(
                formatter,
                "interval_factors.nu_h_surprise[{interval_index}] must be finite and non-negative, found {value}"
            ),
            Self::InconsistentNewsIntensityAggregation {
                interval_index,
                nu_h_total,
                nu_h_known,
                nu_h_surprise,
            } => write!(
                formatter,
                "interval_factors.nu_h[{interval_index}] must match nu_h_known + nu_h_surprise within tolerance {NEWS_INTENSITY_COMPONENT_SUM_TOLERANCE}, found total={nu_h_total}, known={nu_h_known}, surprise={nu_h_surprise}"
            ),
            Self::InvalidIntervalDelta { value } => write!(
                formatter,
                "interval_factors.interval_delta must be finite and strictly positive when provided, found {value}"
            ),
        }
    }
}

impl Error for SufficientStatisticsRiskIntervalFactorsError {}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct SufficientStatisticsRiskModelInputs {
    pub equation_specification: ExponentialAffineTransformEquationSpecification,
    pub nig_shared_plus_shift_configuration: NigSharedPlusShiftConfiguration,
    pub initial_excitation_state: HawkesExcitationState,
}

impl SufficientStatisticsRiskModelInputs {
    pub fn try_new(
        equation_specification: ExponentialAffineTransformEquationSpecification,
        nig_shared_plus_shift_configuration: NigSharedPlusShiftConfiguration,
        initial_excitation_state: HawkesExcitationState,
    ) -> Result<Self, SufficientStatisticsRiskModelInputsError> {
        let model_inputs = Self {
            equation_specification,
            nig_shared_plus_shift_configuration,
            initial_excitation_state,
        };
        model_inputs.validate()?;
        Ok(model_inputs)
    }

    pub fn validate(self) -> Result<(), SufficientStatisticsRiskModelInputsError> {
        self.equation_specification.validate().map_err(|source| {
            SufficientStatisticsRiskModelInputsError::InvalidEquationSpecification { source }
        })?;

        self.nig_shared_plus_shift_configuration
            .derive_driver_parameters()
            .map_err(|source| {
                SufficientStatisticsRiskModelInputsError::InvalidNigSharedPlusShiftConfiguration {
                    source,
                }
            })?;

        validate_excitation_state(self.initial_excitation_state).map_err(|(field, value)| {
            SufficientStatisticsRiskModelInputsError::InvalidInitialState { field, value }
        })?;

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SufficientStatisticsRiskModelInputsError {
    InvalidEquationSpecification {
        source: ExponentialAffineTransformEquationSpecificationError,
    },
    InvalidNigSharedPlusShiftConfiguration {
        source: NigSharedPlusShiftConfigurationError,
    },
    InvalidInitialState {
        field: &'static str,
        value: f64,
    },
}

impl Display for SufficientStatisticsRiskModelInputsError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidEquationSpecification { source } => {
                write!(
                    formatter,
                    "model_inputs.equation_specification is invalid: {source}"
                )
            }
            Self::InvalidNigSharedPlusShiftConfiguration { source } => write!(
                formatter,
                "model_inputs.nig_shared_plus_shift_configuration is invalid: {source}"
            ),
            Self::InvalidInitialState { field, value } => write!(
                formatter,
                "model_inputs.initial_excitation_state.{field} must be finite and non-negative, found {value}"
            ),
        }
    }
}

impl Error for SufficientStatisticsRiskModelInputsError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidEquationSpecification { source } => Some(source),
            Self::InvalidNigSharedPlusShiftConfiguration { source } => Some(source),
            _ => None,
        }
    }
}

/// Public run request contract for sufficient-statistics risk simulation.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SufficientStatisticsRiskRunRequest {
    pub seed: u64,
    pub horizon_interval_count: usize,
    pub interval_delta: f64,
    pub model_inputs: SufficientStatisticsRiskModelInputs,
    pub interval_factors: SufficientStatisticsRiskIntervalFactors,
}

impl SufficientStatisticsRiskRunRequest {
    pub fn try_new(
        seed: u64,
        horizon_interval_count: usize,
        interval_delta: f64,
        model_inputs: SufficientStatisticsRiskModelInputs,
        interval_factors: SufficientStatisticsRiskIntervalFactors,
    ) -> Result<Self, SufficientStatisticsRiskRunRequestError> {
        let run_request = Self {
            seed,
            horizon_interval_count,
            interval_delta,
            model_inputs,
            interval_factors,
        };
        run_request.validate()?;
        Ok(run_request)
    }

    pub fn validate(&self) -> Result<(), SufficientStatisticsRiskRunRequestError> {
        if self.horizon_interval_count == 0 {
            return Err(
                SufficientStatisticsRiskRunRequestError::InvalidHorizonIntervalCount { value: 0 },
            );
        }

        if !self.interval_delta.is_finite() || self.interval_delta <= 0.0 {
            return Err(
                SufficientStatisticsRiskRunRequestError::InvalidIntervalDelta {
                    value: self.interval_delta,
                },
            );
        }

        self.model_inputs.validate().map_err(|source| {
            SufficientStatisticsRiskRunRequestError::InvalidModelInputs { source }
        })?;

        self.interval_factors.validate().map_err(|source| {
            SufficientStatisticsRiskRunRequestError::InvalidIntervalFactors { source }
        })?;

        let resolved_factor_interval_delta = self.interval_factors.resolved_interval_delta();
        if self.interval_delta != resolved_factor_interval_delta {
            return Err(
                SufficientStatisticsRiskRunRequestError::IntervalDeltaFactorMismatch {
                    requested_interval_delta: self.interval_delta,
                    available_factor_interval_delta: resolved_factor_interval_delta,
                },
            );
        }

        let available_interval_count = self.interval_factors.s_h.len();
        if self.horizon_interval_count > available_interval_count {
            return Err(
                SufficientStatisticsRiskRunRequestError::HorizonExceedsAvailableIntervals {
                    requested_interval_count: self.horizon_interval_count,
                    available_interval_count,
                },
            );
        }

        Ok(())
    }

    pub const fn schema_version() -> &'static str {
        SUFFICIENT_STATISTICS_RISK_RUN_REQUEST_SCHEMA_VERSION
    }

    pub const fn default_interval_delta() -> f64 {
        DEFAULT_INTRADAY_RISK_INTERVAL_DELTA
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SufficientStatisticsRiskRunRequestError {
    InvalidHorizonIntervalCount {
        value: usize,
    },
    InvalidIntervalDelta {
        value: f64,
    },
    InvalidModelInputs {
        source: SufficientStatisticsRiskModelInputsError,
    },
    InvalidIntervalFactors {
        source: SufficientStatisticsRiskIntervalFactorsError,
    },
    IntervalDeltaFactorMismatch {
        requested_interval_delta: f64,
        available_factor_interval_delta: f64,
    },
    HorizonExceedsAvailableIntervals {
        requested_interval_count: usize,
        available_interval_count: usize,
    },
}

impl Display for SufficientStatisticsRiskRunRequestError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidHorizonIntervalCount { value } => write!(
                formatter,
                "horizon_interval_count must be strictly positive, found {value}"
            ),
            Self::InvalidIntervalDelta { value } => write!(
                formatter,
                "interval_delta must be finite and strictly positive, found {value}"
            ),
            Self::InvalidModelInputs { source } => {
                write!(formatter, "model_inputs payload is invalid: {source}")
            }
            Self::InvalidIntervalFactors { source } => {
                write!(formatter, "interval_factors payload is invalid: {source}")
            }
            Self::IntervalDeltaFactorMismatch {
                requested_interval_delta,
                available_factor_interval_delta,
            } => write!(
                formatter,
                "interval_delta ({requested_interval_delta}) must match interval_factors.interval_delta ({available_factor_interval_delta}) under the explicit-factor policy; implicit factor resampling is unsupported"
            ),
            Self::HorizonExceedsAvailableIntervals {
                requested_interval_count,
                available_interval_count,
            } => write!(
                formatter,
                "horizon_interval_count ({requested_interval_count}) exceeds available interval_factors entries ({available_interval_count})"
            ),
        }
    }
}

impl Error for SufficientStatisticsRiskRunRequestError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidModelInputs { source } => Some(source),
            Self::InvalidIntervalFactors { source } => Some(source),
            _ => None,
        }
    }
}

pub const SUFFICIENT_STATISTICS_TRANSFORM_SLICE_SETTINGS_POLICY: &str =
    "positive_u_probes_and_positive_non_unit_z_probes_for_oracle_slice_sampling.v1";
pub const SUFFICIENT_STATISTICS_TRANSFORM_SLICE_EXPECTATION_ESTIMATION_POLICY: &str =
    "first_order_finite_difference_of_laplace_and_pgf_slices.v1";
pub const SUFFICIENT_STATISTICS_TRANSFORM_SLICE_SAMPLING_POLICY: &str = "counts_via_fractional_rounding_and_integrated_intensity_via_exponential_with_slice_implied_mean.v1";
pub const SUFFICIENT_STATISTICS_TRANSFORM_SLICE_RANDOMNESS_SUBSTREAM_POLICY: &str =
    "reuse_seeded_tick_substream_domains_with_interval_index_for_slice_sampler.v1";
pub const SUFFICIENT_STATISTICS_STREAMING_CHUNKING_POLICY: &str =
    "fixed_max_intervals_per_chunk_with_ordered_interval_emission.v1";
pub const SUFFICIENT_STATISTICS_BATCH_SEED_PARTITION_POLICY: &str =
    "path_seed_partitioned_from_root_seed_and_path_index_with_shared_substream_mixer.v1";
pub const SUFFICIENT_STATISTICS_BOUNDARY_STATE_UPDATE_POLICY: &str =
    "interval_boundary_drift_then_count_mapped_endogenous_jump_composition.v1";
pub const SUFFICIENT_STATISTICS_INTERVAL_RETURN_GENERATION_POLICY: &str =
    "sample_trading_time_increment_from_sampled_statistics_via_shared_return_decomposition.v1";
pub const SUFFICIENT_STATISTICS_INTERVAL_RETURN_COMPONENT_BREAKDOWN_POLICY: &str =
    "optional_component_breakdown_toggle_for_risk_interval_return_generation.v1";
pub const SUFFICIENT_STATISTICS_INTERVAL_RETURN_COMPONENT_ROUTING_POLICY: &str =
    SHARED_NEWS_EVENT_DISPATCH_RETURN_ROUTING_POLICY;

const DEFAULT_TRANSFORM_SLICE_A_PLUS_U: f64 = 1.0e-3;
const DEFAULT_TRANSFORM_SLICE_A_MINUS_U: f64 = 1.0e-3;
const DEFAULT_TRANSFORM_SLICE_N_PLUS_Z: f64 = 0.99;
const DEFAULT_TRANSFORM_SLICE_N_MINUS_Z: f64 = 0.99;
const DEFAULT_STREAMING_MAX_INTERVALS_PER_CHUNK: usize = 256;
const TRANSFORM_SLICE_REAL_PART_TOLERANCE: f64 = 1.0e-8;
const TRANSFORM_SLICE_NEGATIVE_EXPECTATION_TOLERANCE: f64 = 1.0e-10;
const MAX_INTERVAL_STATISTICS_COUNT: u64 = 1_000_000;
const BATCH_SEED_PARTITION_SUBSTREAM_DOMAIN: SimulationRandomnessSubstreamDomain =
    SimulationRandomnessSubstreamDomain::ExogenousNewsCount;

/// One-interval context used to solve and sample sufficient-statistics transform slices.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct SufficientStatisticsTransformSliceContext {
    pub interval_index: usize,
    pub s_h: f64,
    pub news_intensity: ExponentialAffineTransformNewsIntensityInput,
    pub delta: f64,
    pub initial_excitation_state: HawkesExcitationState,
}

impl SufficientStatisticsTransformSliceContext {
    pub fn try_new(
        interval_index: usize,
        s_h: f64,
        news_intensity: ExponentialAffineTransformNewsIntensityInput,
        delta: f64,
        initial_excitation_state: HawkesExcitationState,
    ) -> Result<Self, SufficientStatisticsTransformSliceContextError> {
        let context = Self {
            interval_index,
            s_h,
            news_intensity,
            delta,
            initial_excitation_state,
        };
        context.validate()?;
        Ok(context)
    }

    pub fn validate(self) -> Result<(), SufficientStatisticsTransformSliceContextError> {
        if !self.s_h.is_finite() || self.s_h < 0.0 {
            return Err(
                SufficientStatisticsTransformSliceContextError::InvalidSeasonalityFactor {
                    interval_index: self.interval_index,
                    value: self.s_h,
                },
            );
        }
        self.news_intensity.validate().map_err(|source| {
            SufficientStatisticsTransformSliceContextError::InvalidNewsIntensity {
                interval_index: self.interval_index,
                source,
            }
        })?;
        if !self.delta.is_finite() || self.delta <= 0.0 {
            return Err(
                SufficientStatisticsTransformSliceContextError::InvalidIntervalDelta {
                    interval_index: self.interval_index,
                    value: self.delta,
                },
            );
        }

        validate_excitation_state(self.initial_excitation_state).map_err(|(field, value)| {
            SufficientStatisticsTransformSliceContextError::InvalidInitialState {
                interval_index: self.interval_index,
                field,
                value,
            }
        })?;

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SufficientStatisticsTransformSliceContextError {
    InvalidSeasonalityFactor {
        interval_index: usize,
        value: f64,
    },
    InvalidNewsIntensity {
        interval_index: usize,
        source: ExponentialAffineTransformNewsIntensityInputError,
    },
    InvalidIntervalDelta {
        interval_index: usize,
        value: f64,
    },
    InvalidInitialState {
        interval_index: usize,
        field: &'static str,
        value: f64,
    },
}

impl Display for SufficientStatisticsTransformSliceContextError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidSeasonalityFactor {
                interval_index,
                value,
            } => write!(
                formatter,
                "interval {interval_index} has invalid s_h, expected finite non-negative value, found {value}"
            ),
            Self::InvalidNewsIntensity {
                interval_index,
                source,
            } => write!(
                formatter,
                "interval {interval_index} has invalid news_intensity: {source}"
            ),
            Self::InvalidIntervalDelta {
                interval_index,
                value,
            } => write!(
                formatter,
                "interval {interval_index} has invalid delta, expected finite strictly positive value, found {value}"
            ),
            Self::InvalidInitialState {
                interval_index,
                field,
                value,
            } => write!(
                formatter,
                "interval {interval_index} has invalid initial_excitation_state.{field}, expected finite non-negative value, found {value}"
            ),
        }
    }
}

impl Error for SufficientStatisticsTransformSliceContextError {}

/// Probe settings for transform-slice extraction used by the risk sampler.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct SufficientStatisticsTransformSliceSettings {
    pub a_plus_u: f64,
    pub a_minus_u: f64,
    pub n_plus_z: f64,
    pub n_minus_z: f64,
}

impl SufficientStatisticsTransformSliceSettings {
    pub fn try_new(
        a_plus_u: f64,
        a_minus_u: f64,
        n_plus_z: f64,
        n_minus_z: f64,
    ) -> Result<Self, SufficientStatisticsTransformSliceSettingsError> {
        let settings = Self {
            a_plus_u,
            a_minus_u,
            n_plus_z,
            n_minus_z,
        };
        settings.validate()?;
        Ok(settings)
    }

    pub fn validate(self) -> Result<(), SufficientStatisticsTransformSliceSettingsError> {
        if !self.a_plus_u.is_finite() || self.a_plus_u <= 0.0 {
            return Err(
                SufficientStatisticsTransformSliceSettingsError::InvalidAPlusU {
                    value: self.a_plus_u,
                },
            );
        }
        if !self.a_minus_u.is_finite() || self.a_minus_u <= 0.0 {
            return Err(
                SufficientStatisticsTransformSliceSettingsError::InvalidAMinusU {
                    value: self.a_minus_u,
                },
            );
        }
        if !self.n_plus_z.is_finite() || self.n_plus_z <= 0.0 || self.n_plus_z == 1.0 {
            return Err(
                SufficientStatisticsTransformSliceSettingsError::InvalidNPlusZ {
                    value: self.n_plus_z,
                },
            );
        }
        if !self.n_minus_z.is_finite() || self.n_minus_z <= 0.0 || self.n_minus_z == 1.0 {
            return Err(
                SufficientStatisticsTransformSliceSettingsError::InvalidNMinusZ {
                    value: self.n_minus_z,
                },
            );
        }

        Ok(())
    }

    pub const fn policy() -> &'static str {
        SUFFICIENT_STATISTICS_TRANSFORM_SLICE_SETTINGS_POLICY
    }
}

impl Default for SufficientStatisticsTransformSliceSettings {
    fn default() -> Self {
        Self {
            a_plus_u: DEFAULT_TRANSFORM_SLICE_A_PLUS_U,
            a_minus_u: DEFAULT_TRANSFORM_SLICE_A_MINUS_U,
            n_plus_z: DEFAULT_TRANSFORM_SLICE_N_PLUS_Z,
            n_minus_z: DEFAULT_TRANSFORM_SLICE_N_MINUS_Z,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SufficientStatisticsTransformSliceSettingsError {
    InvalidAPlusU { value: f64 },
    InvalidAMinusU { value: f64 },
    InvalidNPlusZ { value: f64 },
    InvalidNMinusZ { value: f64 },
}

impl Display for SufficientStatisticsTransformSliceSettingsError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidAPlusU { value } => write!(
                formatter,
                "a_plus_u must be finite and strictly positive, found {value}"
            ),
            Self::InvalidAMinusU { value } => write!(
                formatter,
                "a_minus_u must be finite and strictly positive, found {value}"
            ),
            Self::InvalidNPlusZ { value } => write!(
                formatter,
                "n_plus_z must be finite, strictly positive, and different from 1.0, found {value}"
            ),
            Self::InvalidNMinusZ { value } => write!(
                formatter,
                "n_minus_z must be finite, strictly positive, and different from 1.0, found {value}"
            ),
        }
    }
}

impl Error for SufficientStatisticsTransformSliceSettingsError {}

/// One interval's transform-slice payload used to sample `A` and `N` sufficient statistics.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct SufficientStatisticsTransformSlices {
    pub context: SufficientStatisticsTransformSliceContext,
    pub settings: SufficientStatisticsTransformSliceSettings,
    pub baseline_transform: ComplexScalar,
    pub a_plus_transform: ComplexScalar,
    pub a_minus_transform: ComplexScalar,
    pub n_plus_transform: ComplexScalar,
    pub n_minus_transform: ComplexScalar,
}

impl SufficientStatisticsTransformSlices {
    #[allow(clippy::too_many_arguments)]
    pub fn try_new(
        context: SufficientStatisticsTransformSliceContext,
        settings: SufficientStatisticsTransformSliceSettings,
        baseline_transform: ComplexScalar,
        a_plus_transform: ComplexScalar,
        a_minus_transform: ComplexScalar,
        n_plus_transform: ComplexScalar,
        n_minus_transform: ComplexScalar,
    ) -> Result<Self, SufficientStatisticsTransformSlicesError> {
        let slices = Self {
            context,
            settings,
            baseline_transform,
            a_plus_transform,
            a_minus_transform,
            n_plus_transform,
            n_minus_transform,
        };
        slices.validate()?;
        Ok(slices)
    }

    pub fn validate(self) -> Result<(), SufficientStatisticsTransformSlicesError> {
        self.context.validate().map_err(|source| {
            SufficientStatisticsTransformSlicesError::InvalidContext { source }
        })?;
        self.settings.validate().map_err(|source| {
            SufficientStatisticsTransformSlicesError::InvalidSettings { source }
        })?;

        ensure_finite_transform_slice("baseline_transform", self.baseline_transform)?;
        ensure_finite_transform_slice("a_plus_transform", self.a_plus_transform)?;
        ensure_finite_transform_slice("a_minus_transform", self.a_minus_transform)?;
        ensure_finite_transform_slice("n_plus_transform", self.n_plus_transform)?;
        ensure_finite_transform_slice("n_minus_transform", self.n_minus_transform)?;

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SufficientStatisticsTransformSlicesError {
    InvalidContext {
        source: SufficientStatisticsTransformSliceContextError,
    },
    InvalidSettings {
        source: SufficientStatisticsTransformSliceSettingsError,
    },
    InvalidTransformValue {
        field: &'static str,
        value: ComplexScalar,
    },
}

impl Display for SufficientStatisticsTransformSlicesError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidContext { source } => {
                write!(formatter, "transform-slice context is invalid: {source}")
            }
            Self::InvalidSettings { source } => {
                write!(formatter, "transform-slice settings are invalid: {source}")
            }
            Self::InvalidTransformValue { field, value } => write!(
                formatter,
                "{field} must be finite as a complex scalar, found {value:?}"
            ),
        }
    }
}

impl Error for SufficientStatisticsTransformSlicesError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidContext { source } => Some(source),
            Self::InvalidSettings { source } => Some(source),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SufficientStatisticsTransformSliceSolveError {
    InvalidContext {
        source: SufficientStatisticsTransformSliceContextError,
    },
    InvalidSettings {
        source: SufficientStatisticsTransformSliceSettingsError,
    },
    InvalidTransformInput {
        interval_index: usize,
        slice_name: &'static str,
        source: ExponentialAffineTransformInputError,
    },
    SolverFailure {
        interval_index: usize,
        slice_name: &'static str,
        source: ExponentialAffineTransformSolverError,
    },
    IncompleteSolverOutput {
        interval_index: usize,
        slice_name: &'static str,
        status: ExponentialAffineTransformIntegrationStatus,
        guard_failure_code: Option<ExponentialAffineTransformGuardFailureCode>,
    },
    InvalidSolvedSlices {
        interval_index: usize,
        source: SufficientStatisticsTransformSlicesError,
    },
}

impl Display for SufficientStatisticsTransformSliceSolveError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidContext { source } => {
                write!(
                    formatter,
                    "transform-slice solve context is invalid: {source}"
                )
            }
            Self::InvalidSettings { source } => {
                write!(
                    formatter,
                    "transform-slice solve settings are invalid: {source}"
                )
            }
            Self::InvalidTransformInput {
                interval_index,
                slice_name,
                source,
            } => write!(
                formatter,
                "interval {interval_index} transform input for slice {slice_name} is invalid: {source}"
            ),
            Self::SolverFailure {
                interval_index,
                slice_name,
                source,
            } => write!(
                formatter,
                "interval {interval_index} solver failed for slice {slice_name}: {source}"
            ),
            Self::IncompleteSolverOutput {
                interval_index,
                slice_name,
                status,
                guard_failure_code,
            } => write!(
                formatter,
                "interval {interval_index} solver did not produce completed output for slice {slice_name}, status={status:?}, guard_failure_code={guard_failure_code:?}"
            ),
            Self::InvalidSolvedSlices {
                interval_index,
                source,
            } => write!(
                formatter,
                "interval {interval_index} solved transform slices are invalid: {source}"
            ),
        }
    }
}

impl Error for SufficientStatisticsTransformSliceSolveError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidContext { source } => Some(source),
            Self::InvalidSettings { source } => Some(source),
            Self::InvalidTransformInput { source, .. } => Some(source),
            Self::SolverFailure { source, .. } => Some(source),
            Self::InvalidSolvedSlices { source, .. } => Some(source),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SufficientStatisticsTransformSliceSamplingError {
    InvalidSlices {
        source: SufficientStatisticsTransformSlicesError,
    },
    NonRealTransformSlice {
        interval_index: usize,
        field: &'static str,
        value: ComplexScalar,
        tolerance: f64,
    },
    InvalidExpectedStatistic {
        interval_index: usize,
        statistic: &'static str,
        value: f64,
        numerator: f64,
        denominator: f64,
        context: SufficientStatisticsTransformSliceContext,
    },
    InvalidSampledStatistics {
        interval_index: usize,
        source: SufficientStatisticsIntervalStatisticsError,
    },
}

impl Display for SufficientStatisticsTransformSliceSamplingError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidSlices { source } => {
                write!(formatter, "invalid transform slices for sampling: {source}")
            }
            Self::NonRealTransformSlice {
                interval_index,
                field,
                value,
                tolerance,
            } => write!(
                formatter,
                "interval {interval_index} {field} must be real-valued within imaginary tolerance {tolerance}, found {value:?}"
            ),
            Self::InvalidExpectedStatistic {
                interval_index,
                statistic,
                value,
                numerator,
                denominator,
                context,
            } => write!(
                formatter,
                "interval {interval_index} derived expected {statistic} is invalid (value={value}, numerator={numerator}, denominator={denominator}, s_h={}, total_nu_h={}, known_nu_h={}, surprise_nu_h={}, delta={}, initial_state={:?})",
                context.s_h,
                context.news_intensity.total_nu_h(),
                context.news_intensity.known_nu_h,
                context.news_intensity.surprise_nu_h,
                context.delta,
                context.initial_excitation_state
            ),
            Self::InvalidSampledStatistics {
                interval_index,
                source,
            } => write!(
                formatter,
                "interval {interval_index} sampled sufficient statistics are invalid: {source}"
            ),
        }
    }
}

impl Error for SufficientStatisticsTransformSliceSamplingError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidSlices { source } => Some(source),
            Self::InvalidSampledStatistics { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Chunking settings for streaming sufficient-statistics interval emission.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct SufficientStatisticsStreamingChunkingSettings {
    pub max_intervals_per_chunk: usize,
}

impl SufficientStatisticsStreamingChunkingSettings {
    pub fn try_new(
        max_intervals_per_chunk: usize,
    ) -> Result<Self, SufficientStatisticsStreamingChunkingSettingsError> {
        let settings = Self {
            max_intervals_per_chunk,
        };
        settings.validate()?;
        Ok(settings)
    }

    pub fn validate(self) -> Result<(), SufficientStatisticsStreamingChunkingSettingsError> {
        if self.max_intervals_per_chunk == 0 {
            return Err(
                SufficientStatisticsStreamingChunkingSettingsError::InvalidMaxIntervalsPerChunk {
                    value: self.max_intervals_per_chunk,
                },
            );
        }

        Ok(())
    }

    pub const fn policy() -> &'static str {
        SUFFICIENT_STATISTICS_STREAMING_CHUNKING_POLICY
    }
}

impl Default for SufficientStatisticsStreamingChunkingSettings {
    fn default() -> Self {
        Self {
            max_intervals_per_chunk: DEFAULT_STREAMING_MAX_INTERVALS_PER_CHUNK,
        }
    }
}

/// Validation errors for streaming chunking settings.
#[derive(Debug, Clone, PartialEq)]
pub enum SufficientStatisticsStreamingChunkingSettingsError {
    InvalidMaxIntervalsPerChunk { value: usize },
}

impl Display for SufficientStatisticsStreamingChunkingSettingsError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidMaxIntervalsPerChunk { value } => write!(
                formatter,
                "max_intervals_per_chunk must be greater than 0, found {value}"
            ),
        }
    }
}

impl Error for SufficientStatisticsStreamingChunkingSettingsError {}

/// One emitted chunk of sampled interval statistics for a single path.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SufficientStatisticsIntervalStatisticsChunk {
    pub chunk_index: usize,
    pub start_interval_index: usize,
    pub records: Vec<SufficientStatisticsIntervalStatistics>,
}

/// One emitted chunk of sampled interval statistics for one path in a multi-path batch.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SufficientStatisticsIntervalStatisticsPathChunk {
    pub path_index: usize,
    pub path_seed: u64,
    pub interval_chunk: SufficientStatisticsIntervalStatisticsChunk,
}

/// Errors emitted by streaming and batch transform-slice execution adapters.
#[derive(Debug, Clone, PartialEq)]
pub enum SufficientStatisticsTransformSliceExecutionError {
    InvalidChunkingSettings {
        source: SufficientStatisticsStreamingChunkingSettingsError,
    },
    SamplingFailure {
        source: SufficientStatisticsTransformSliceSamplingError,
    },
    PathSamplingFailure {
        path_index: usize,
        source: SufficientStatisticsTransformSliceSamplingError,
    },
}

impl Display for SufficientStatisticsTransformSliceExecutionError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidChunkingSettings { source } => {
                write!(
                    formatter,
                    "streaming chunking settings are invalid: {source}"
                )
            }
            Self::SamplingFailure { source } => write!(
                formatter,
                "transform-slice sampling failed during streaming execution: {source}"
            ),
            Self::PathSamplingFailure { path_index, source } => write!(
                formatter,
                "path {path_index} sampling failed during batch execution: {source}"
            ),
        }
    }
}

impl Error for SufficientStatisticsTransformSliceExecutionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidChunkingSettings { source } => Some(source),
            Self::SamplingFailure { source } => Some(source),
            Self::PathSamplingFailure { source, .. } => Some(source),
        }
    }
}

pub const fn sufficient_statistics_batch_seed_partition_policy() -> &'static str {
    SUFFICIENT_STATISTICS_BATCH_SEED_PARTITION_POLICY
}

/// Deterministically derive a per-path seed from one root batch seed.
pub fn derive_sufficient_statistics_batch_path_seed(root_seed: u64, path_index: u64) -> u64 {
    derive_simulation_substream_seed(root_seed, BATCH_SEED_PARTITION_SUBSTREAM_DOMAIN, path_index)
}

/// Solve the baseline and `A/N` probe slices used by interval sufficient-statistics sampling.
pub fn solve_sufficient_statistics_transform_slices(
    equation_specification: ExponentialAffineTransformEquationSpecification,
    context: SufficientStatisticsTransformSliceContext,
    settings: SufficientStatisticsTransformSliceSettings,
) -> Result<SufficientStatisticsTransformSlices, SufficientStatisticsTransformSliceSolveError> {
    solve_sufficient_statistics_transform_slices_with_solver_settings(
        equation_specification,
        context,
        settings,
        None,
    )
}

/// Solve transform slices with optional explicit ODE solver settings.
pub fn solve_sufficient_statistics_transform_slices_with_solver_settings(
    equation_specification: ExponentialAffineTransformEquationSpecification,
    context: SufficientStatisticsTransformSliceContext,
    settings: SufficientStatisticsTransformSliceSettings,
    solver_settings: Option<ExponentialAffineTransformSolverSettings>,
) -> Result<SufficientStatisticsTransformSlices, SufficientStatisticsTransformSliceSolveError> {
    context.validate().map_err(|source| {
        SufficientStatisticsTransformSliceSolveError::InvalidContext { source }
    })?;
    settings.validate().map_err(|source| {
        SufficientStatisticsTransformSliceSolveError::InvalidSettings { source }
    })?;

    let baseline_transform = solve_transform_slice_value(
        equation_specification,
        context,
        "baseline",
        ComplexScalar::new(0.0, 0.0),
        ComplexScalar::new(0.0, 0.0),
        ComplexScalar::new(1.0, 0.0),
        ComplexScalar::new(1.0, 0.0),
        solver_settings,
    )?;
    let a_plus_transform = solve_transform_slice_value(
        equation_specification,
        context,
        "a_plus",
        ComplexScalar::new(settings.a_plus_u, 0.0),
        ComplexScalar::new(0.0, 0.0),
        ComplexScalar::new(1.0, 0.0),
        ComplexScalar::new(1.0, 0.0),
        solver_settings,
    )?;
    let a_minus_transform = solve_transform_slice_value(
        equation_specification,
        context,
        "a_minus",
        ComplexScalar::new(0.0, 0.0),
        ComplexScalar::new(settings.a_minus_u, 0.0),
        ComplexScalar::new(1.0, 0.0),
        ComplexScalar::new(1.0, 0.0),
        solver_settings,
    )?;
    let n_plus_transform = solve_transform_slice_value(
        equation_specification,
        context,
        "n_plus",
        ComplexScalar::new(0.0, 0.0),
        ComplexScalar::new(0.0, 0.0),
        ComplexScalar::new(settings.n_plus_z, 0.0),
        ComplexScalar::new(1.0, 0.0),
        solver_settings,
    )?;
    let n_minus_transform = solve_transform_slice_value(
        equation_specification,
        context,
        "n_minus",
        ComplexScalar::new(0.0, 0.0),
        ComplexScalar::new(0.0, 0.0),
        ComplexScalar::new(1.0, 0.0),
        ComplexScalar::new(settings.n_minus_z, 0.0),
        solver_settings,
    )?;

    SufficientStatisticsTransformSlices::try_new(
        context,
        settings,
        baseline_transform,
        a_plus_transform,
        a_minus_transform,
        n_plus_transform,
        n_minus_transform,
    )
    .map_err(
        |source| SufficientStatisticsTransformSliceSolveError::InvalidSolvedSlices {
            interval_index: context.interval_index,
            source,
        },
    )
}

/// Sample one interval's sufficient statistics from solved transform slices.
pub fn sample_interval_statistics_from_transform_slices(
    seed: u64,
    slices: SufficientStatisticsTransformSlices,
) -> Result<SufficientStatisticsIntervalStatistics, SufficientStatisticsTransformSliceSamplingError>
{
    let randomness_factory = SplitMix64SimulationRandomnessFactory;
    sample_interval_statistics_from_transform_slices_with_randomness_factory(
        seed,
        slices,
        &randomness_factory,
    )
}

/// Sample one interval's sufficient statistics with an injected randomness factory.
pub fn sample_interval_statistics_from_transform_slices_with_randomness_factory<F>(
    seed: u64,
    slices: SufficientStatisticsTransformSlices,
    randomness_factory: &F,
) -> Result<SufficientStatisticsIntervalStatistics, SufficientStatisticsTransformSliceSamplingError>
where
    F: SimulationRandomnessFactory,
{
    sample_interval_statistics_from_transform_slices_with_sampling_index(
        seed,
        0,
        slices,
        randomness_factory,
    )
}

/// Sample a batch of interval statistics from solved transform slices.
pub fn sample_interval_statistics_batch_from_transform_slices(
    seed: u64,
    slices_batch: &[SufficientStatisticsTransformSlices],
) -> Result<
    Vec<SufficientStatisticsIntervalStatistics>,
    SufficientStatisticsTransformSliceSamplingError,
> {
    let randomness_factory = SplitMix64SimulationRandomnessFactory;
    sample_interval_statistics_batch_from_transform_slices_with_randomness_factory(
        seed,
        slices_batch,
        &randomness_factory,
    )
}

/// Sample a batch of interval statistics with an injected randomness factory.
pub fn sample_interval_statistics_batch_from_transform_slices_with_randomness_factory<F>(
    seed: u64,
    slices_batch: &[SufficientStatisticsTransformSlices],
    randomness_factory: &F,
) -> Result<
    Vec<SufficientStatisticsIntervalStatistics>,
    SufficientStatisticsTransformSliceSamplingError,
>
where
    F: SimulationRandomnessFactory,
{
    if slices_batch.is_empty() {
        return Ok(Vec::new());
    }

    let chunking_settings =
        SufficientStatisticsStreamingChunkingSettings::try_new(slices_batch.len())
            .expect("non-empty slices batch must produce a valid chunking setting");
    let mut sampled_statistics = Vec::with_capacity(slices_batch.len());
    stream_interval_statistics_from_transform_slices_with_randomness_factory_internal(
        seed,
        slices_batch,
        chunking_settings,
        randomness_factory,
        |interval_chunk| sampled_statistics.extend(interval_chunk.records),
    )?;

    Ok(sampled_statistics)
}

/// Stream sampled interval statistics in deterministic chunks.
pub fn stream_interval_statistics_from_transform_slices<C>(
    seed: u64,
    slices_batch: &[SufficientStatisticsTransformSlices],
    chunking_settings: SufficientStatisticsStreamingChunkingSettings,
    emit_chunk: C,
) -> Result<(), SufficientStatisticsTransformSliceExecutionError>
where
    C: FnMut(SufficientStatisticsIntervalStatisticsChunk),
{
    let randomness_factory = SplitMix64SimulationRandomnessFactory;
    stream_interval_statistics_from_transform_slices_with_randomness_factory(
        seed,
        slices_batch,
        chunking_settings,
        &randomness_factory,
        emit_chunk,
    )
}

/// Stream sampled interval statistics in deterministic chunks with an injected randomness factory.
pub fn stream_interval_statistics_from_transform_slices_with_randomness_factory<F, C>(
    seed: u64,
    slices_batch: &[SufficientStatisticsTransformSlices],
    chunking_settings: SufficientStatisticsStreamingChunkingSettings,
    randomness_factory: &F,
    emit_chunk: C,
) -> Result<(), SufficientStatisticsTransformSliceExecutionError>
where
    F: SimulationRandomnessFactory,
    C: FnMut(SufficientStatisticsIntervalStatisticsChunk),
{
    chunking_settings.validate().map_err(|source| {
        SufficientStatisticsTransformSliceExecutionError::InvalidChunkingSettings { source }
    })?;

    stream_interval_statistics_from_transform_slices_with_randomness_factory_internal(
        seed,
        slices_batch,
        chunking_settings,
        randomness_factory,
        emit_chunk,
    )
    .map_err(|source| SufficientStatisticsTransformSliceExecutionError::SamplingFailure { source })
}

/// Stream multi-path sampled interval statistics with deterministic path-seed partitioning.
pub fn stream_interval_statistics_multi_path_batch_from_transform_slices<C>(
    root_seed: u64,
    slices_path_batch: &[Vec<SufficientStatisticsTransformSlices>],
    chunking_settings: SufficientStatisticsStreamingChunkingSettings,
    emit_chunk: C,
) -> Result<(), SufficientStatisticsTransformSliceExecutionError>
where
    C: FnMut(SufficientStatisticsIntervalStatisticsPathChunk),
{
    let randomness_factory = SplitMix64SimulationRandomnessFactory;
    stream_interval_statistics_multi_path_batch_from_transform_slices_with_randomness_factory(
        root_seed,
        slices_path_batch,
        chunking_settings,
        &randomness_factory,
        emit_chunk,
    )
}

/// Stream multi-path sampled interval statistics with deterministic path-seed partitioning.
pub fn stream_interval_statistics_multi_path_batch_from_transform_slices_with_randomness_factory<
    F,
    C,
>(
    root_seed: u64,
    slices_path_batch: &[Vec<SufficientStatisticsTransformSlices>],
    chunking_settings: SufficientStatisticsStreamingChunkingSettings,
    randomness_factory: &F,
    mut emit_chunk: C,
) -> Result<(), SufficientStatisticsTransformSliceExecutionError>
where
    F: SimulationRandomnessFactory,
    C: FnMut(SufficientStatisticsIntervalStatisticsPathChunk),
{
    chunking_settings.validate().map_err(|source| {
        SufficientStatisticsTransformSliceExecutionError::InvalidChunkingSettings { source }
    })?;

    for (path_index, path_slices) in slices_path_batch.iter().enumerate() {
        let path_seed = derive_sufficient_statistics_batch_path_seed(root_seed, path_index as u64);
        stream_interval_statistics_from_transform_slices_with_randomness_factory_internal(
            path_seed,
            path_slices,
            chunking_settings,
            randomness_factory,
            |interval_chunk| {
                emit_chunk(SufficientStatisticsIntervalStatisticsPathChunk {
                    path_index,
                    path_seed,
                    interval_chunk,
                });
            },
        )
        .map_err(|source| {
            SufficientStatisticsTransformSliceExecutionError::PathSamplingFailure {
                path_index,
                source,
            }
        })?;
    }

    Ok(())
}

/// Batch sample multi-path interval statistics with deterministic path-seed partitioning.
pub fn sample_interval_statistics_multi_path_batch_from_transform_slices(
    root_seed: u64,
    slices_path_batch: &[Vec<SufficientStatisticsTransformSlices>],
    chunking_settings: SufficientStatisticsStreamingChunkingSettings,
) -> Result<
    Vec<Vec<SufficientStatisticsIntervalStatistics>>,
    SufficientStatisticsTransformSliceExecutionError,
> {
    let randomness_factory = SplitMix64SimulationRandomnessFactory;
    sample_interval_statistics_multi_path_batch_from_transform_slices_with_randomness_factory(
        root_seed,
        slices_path_batch,
        chunking_settings,
        &randomness_factory,
    )
}

/// Batch sample multi-path interval statistics with deterministic path-seed partitioning.
pub fn sample_interval_statistics_multi_path_batch_from_transform_slices_with_randomness_factory<
    F,
>(
    root_seed: u64,
    slices_path_batch: &[Vec<SufficientStatisticsTransformSlices>],
    chunking_settings: SufficientStatisticsStreamingChunkingSettings,
    randomness_factory: &F,
) -> Result<
    Vec<Vec<SufficientStatisticsIntervalStatistics>>,
    SufficientStatisticsTransformSliceExecutionError,
>
where
    F: SimulationRandomnessFactory,
{
    let mut sampled_paths = vec![Vec::new(); slices_path_batch.len()];
    stream_interval_statistics_multi_path_batch_from_transform_slices_with_randomness_factory(
        root_seed,
        slices_path_batch,
        chunking_settings,
        randomness_factory,
        |path_chunk| sampled_paths[path_chunk.path_index].extend(path_chunk.interval_chunk.records),
    )?;
    Ok(sampled_paths)
}

fn stream_interval_statistics_from_transform_slices_with_randomness_factory_internal<F, C>(
    seed: u64,
    slices_batch: &[SufficientStatisticsTransformSlices],
    chunking_settings: SufficientStatisticsStreamingChunkingSettings,
    randomness_factory: &F,
    mut emit_chunk: C,
) -> Result<(), SufficientStatisticsTransformSliceSamplingError>
where
    F: SimulationRandomnessFactory,
    C: FnMut(SufficientStatisticsIntervalStatisticsChunk),
{
    let mut chunk_index = 0_usize;
    let mut chunk_start_interval_index = 0_usize;
    let mut pending_chunk_records: Vec<SufficientStatisticsIntervalStatistics> =
        Vec::with_capacity(chunking_settings.max_intervals_per_chunk);

    for (batch_position, slices) in slices_batch.iter().copied().enumerate() {
        if pending_chunk_records.is_empty() {
            chunk_start_interval_index = batch_position;
        }

        pending_chunk_records.push(
            sample_interval_statistics_from_transform_slices_with_sampling_index(
                seed,
                batch_position as u64,
                slices,
                randomness_factory,
            )?,
        );

        if pending_chunk_records.len() >= chunking_settings.max_intervals_per_chunk {
            flush_interval_statistics_chunk(
                &mut pending_chunk_records,
                &mut chunk_index,
                chunk_start_interval_index,
                &mut emit_chunk,
            );
        }
    }

    if !pending_chunk_records.is_empty() {
        flush_interval_statistics_chunk(
            &mut pending_chunk_records,
            &mut chunk_index,
            chunk_start_interval_index,
            &mut emit_chunk,
        );
    }

    Ok(())
}

fn flush_interval_statistics_chunk<C>(
    pending_chunk_records: &mut Vec<SufficientStatisticsIntervalStatistics>,
    chunk_index: &mut usize,
    chunk_start_interval_index: usize,
    emit_chunk: &mut C,
) where
    C: FnMut(SufficientStatisticsIntervalStatisticsChunk),
{
    if pending_chunk_records.is_empty() {
        return;
    }

    let records = std::mem::replace(
        pending_chunk_records,
        Vec::with_capacity(pending_chunk_records.capacity()),
    );
    emit_chunk(SufficientStatisticsIntervalStatisticsChunk {
        chunk_index: *chunk_index,
        start_interval_index: chunk_start_interval_index,
        records,
    });
    *chunk_index = chunk_index.saturating_add(1);
}

fn solve_transform_slice_value(
    equation_specification: ExponentialAffineTransformEquationSpecification,
    context: SufficientStatisticsTransformSliceContext,
    slice_name: &'static str,
    u_plus: ComplexScalar,
    u_minus: ComplexScalar,
    z_plus: ComplexScalar,
    z_minus: ComplexScalar,
    solver_settings: Option<ExponentialAffineTransformSolverSettings>,
) -> Result<ComplexScalar, SufficientStatisticsTransformSliceSolveError> {
    let transform_input = ExponentialAffineTransformInput::try_new(
        equation_specification,
        u_plus,
        u_minus,
        z_plus,
        z_minus,
        context.s_h,
        context.news_intensity,
        context.delta,
        context.initial_excitation_state,
    )
    .map_err(
        |source| SufficientStatisticsTransformSliceSolveError::InvalidTransformInput {
            interval_index: context.interval_index,
            slice_name,
            source,
        },
    )?;

    let solve_result =
        solve_exponential_affine_transform_with_optional_settings(transform_input, solver_settings)
            .map_err(
                |source| SufficientStatisticsTransformSliceSolveError::SolverFailure {
                    interval_index: context.interval_index,
                    slice_name,
                    source,
                },
            )?;

    if solve_result.status != ExponentialAffineTransformIntegrationStatus::Completed {
        return Err(
            SufficientStatisticsTransformSliceSolveError::IncompleteSolverOutput {
                interval_index: context.interval_index,
                slice_name,
                status: solve_result.status,
                guard_failure_code: solve_result
                    .metadata
                    .guard_failure
                    .map(|guard_failure| guard_failure.code),
            },
        );
    }

    let output = solve_result.output.ok_or(
        SufficientStatisticsTransformSliceSolveError::IncompleteSolverOutput {
            interval_index: context.interval_index,
            slice_name,
            status: solve_result.status,
            guard_failure_code: solve_result
                .metadata
                .guard_failure
                .map(|guard_failure| guard_failure.code),
        },
    )?;
    Ok(output.transform_value)
}

fn sample_interval_statistics_from_transform_slices_with_sampling_index<F>(
    seed: u64,
    batch_position: u64,
    slices: SufficientStatisticsTransformSlices,
    randomness_factory: &F,
) -> Result<SufficientStatisticsIntervalStatistics, SufficientStatisticsTransformSliceSamplingError>
where
    F: SimulationRandomnessFactory,
{
    slices.validate().map_err(|source| {
        SufficientStatisticsTransformSliceSamplingError::InvalidSlices { source }
    })?;

    let context = slices.context;
    let settings = slices.settings;
    let baseline_transform =
        extract_real_transform_value(context, "baseline_transform", slices.baseline_transform)?;
    let a_plus_transform =
        extract_real_transform_value(context, "a_plus_transform", slices.a_plus_transform)?;
    let a_minus_transform =
        extract_real_transform_value(context, "a_minus_transform", slices.a_minus_transform)?;
    let n_plus_transform =
        extract_real_transform_value(context, "n_plus_transform", slices.n_plus_transform)?;
    let n_minus_transform =
        extract_real_transform_value(context, "n_minus_transform", slices.n_minus_transform)?;

    let expected_a_plus = derive_non_negative_expectation(
        context,
        "a_plus",
        baseline_transform - a_plus_transform,
        settings.a_plus_u,
    )?;
    let expected_a_minus = derive_non_negative_expectation(
        context,
        "a_minus",
        baseline_transform - a_minus_transform,
        settings.a_minus_u,
    )?;
    let expected_n_plus = derive_non_negative_expectation(
        context,
        "n_plus",
        n_plus_transform - baseline_transform,
        settings.n_plus_z - 1.0,
    )?;
    let expected_n_minus = derive_non_negative_expectation(
        context,
        "n_minus",
        n_minus_transform - baseline_transform,
        settings.n_minus_z - 1.0,
    )?;

    let substream_index = derive_transform_slice_sampling_substream_index(
        context.interval_index as u64,
        batch_position,
    );
    let mut n_plus_stream = randomness_factory.create_substream(
        seed,
        SimulationRandomnessSubstreamDomain::EndogenousBuyEventCount,
        substream_index,
    );
    let mut n_minus_stream = randomness_factory.create_substream(
        seed,
        SimulationRandomnessSubstreamDomain::EndogenousSellEventCount,
        substream_index,
    );
    let mut a_plus_stream = randomness_factory.create_substream(
        seed,
        SimulationRandomnessSubstreamDomain::EndogenousBuyEventTimestamp,
        substream_index,
    );
    let mut a_minus_stream = randomness_factory.create_substream(
        seed,
        SimulationRandomnessSubstreamDomain::EndogenousSellEventTimestamp,
        substream_index,
    );

    let sampled_n_plus = sample_count_from_expectation(expected_n_plus, &mut n_plus_stream);
    let sampled_n_minus = sample_count_from_expectation(expected_n_minus, &mut n_minus_stream);
    let sampled_a_plus = sample_exponential_with_mean(expected_a_plus, &mut a_plus_stream);
    let sampled_a_minus = sample_exponential_with_mean(expected_a_minus, &mut a_minus_stream);

    SufficientStatisticsIntervalStatistics::try_new(
        sampled_a_plus,
        sampled_a_minus,
        sampled_n_plus,
        sampled_n_minus,
    )
    .map_err(|source| {
        SufficientStatisticsTransformSliceSamplingError::InvalidSampledStatistics {
            interval_index: context.interval_index,
            source,
        }
    })
}

fn ensure_finite_transform_slice(
    field: &'static str,
    value: ComplexScalar,
) -> Result<(), SufficientStatisticsTransformSlicesError> {
    if !value.is_finite() {
        return Err(
            SufficientStatisticsTransformSlicesError::InvalidTransformValue { field, value },
        );
    }
    Ok(())
}

fn extract_real_transform_value(
    context: SufficientStatisticsTransformSliceContext,
    field: &'static str,
    value: ComplexScalar,
) -> Result<f64, SufficientStatisticsTransformSliceSamplingError> {
    if value.imaginary.abs() > TRANSFORM_SLICE_REAL_PART_TOLERANCE {
        return Err(
            SufficientStatisticsTransformSliceSamplingError::NonRealTransformSlice {
                interval_index: context.interval_index,
                field,
                value,
                tolerance: TRANSFORM_SLICE_REAL_PART_TOLERANCE,
            },
        );
    }

    Ok(value.real)
}

fn derive_non_negative_expectation(
    context: SufficientStatisticsTransformSliceContext,
    statistic: &'static str,
    numerator: f64,
    denominator: f64,
) -> Result<f64, SufficientStatisticsTransformSliceSamplingError> {
    let expected_value = numerator / denominator;
    if !expected_value.is_finite() {
        return Err(
            SufficientStatisticsTransformSliceSamplingError::InvalidExpectedStatistic {
                interval_index: context.interval_index,
                statistic,
                value: expected_value,
                numerator,
                denominator,
                context,
            },
        );
    }

    if expected_value < -TRANSFORM_SLICE_NEGATIVE_EXPECTATION_TOLERANCE {
        return Err(
            SufficientStatisticsTransformSliceSamplingError::InvalidExpectedStatistic {
                interval_index: context.interval_index,
                statistic,
                value: expected_value,
                numerator,
                denominator,
                context,
            },
        );
    }

    Ok(expected_value.max(0.0))
}

fn derive_transform_slice_sampling_substream_index(
    interval_index: u64,
    batch_position: u64,
) -> u64 {
    let interval_component = interval_index.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    interval_component ^ batch_position
}

fn sample_count_from_expectation<R>(expected_count: f64, randomness_stream: &mut R) -> u64
where
    R: SimulationRandomnessStream,
{
    if !expected_count.is_finite() || expected_count <= 0.0 {
        return 0;
    }

    if expected_count >= u64::MAX as f64 {
        return MAX_INTERVAL_STATISTICS_COUNT;
    }

    let floored_count = expected_count.floor() as u64;
    let fractional_count = expected_count - floored_count as f64;
    let sampled_count = if randomness_stream.next_unit_interval_f64() < fractional_count {
        floored_count.saturating_add(1)
    } else {
        floored_count
    };

    sampled_count.min(MAX_INTERVAL_STATISTICS_COUNT)
}

fn sample_exponential_with_mean<R>(mean: f64, randomness_stream: &mut R) -> f64
where
    R: SimulationRandomnessStream,
{
    if !mean.is_finite() || mean <= 0.0 {
        return 0.0;
    }

    let bounded_draw = randomness_stream
        .next_unit_interval_f64()
        .clamp(0.0, 1.0 - f64::EPSILON);
    -mean * (1.0 - bounded_draw).ln()
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct SufficientStatisticsIntervalStatistics {
    pub a_plus: f64,
    pub a_minus: f64,
    pub n_plus: u64,
    pub n_minus: u64,
}

impl SufficientStatisticsIntervalStatistics {
    pub fn try_new(
        a_plus: f64,
        a_minus: f64,
        n_plus: u64,
        n_minus: u64,
    ) -> Result<Self, SufficientStatisticsIntervalStatisticsError> {
        let interval_statistics = Self {
            a_plus,
            a_minus,
            n_plus,
            n_minus,
        };
        interval_statistics.validate()?;
        Ok(interval_statistics)
    }

    pub fn validate(self) -> Result<(), SufficientStatisticsIntervalStatisticsError> {
        if !self.a_plus.is_finite() || self.a_plus < 0.0 {
            return Err(SufficientStatisticsIntervalStatisticsError::InvalidAPlus {
                value: self.a_plus,
            });
        }
        if !self.a_minus.is_finite() || self.a_minus < 0.0 {
            return Err(SufficientStatisticsIntervalStatisticsError::InvalidAMinus {
                value: self.a_minus,
            });
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SufficientStatisticsIntervalStatisticsError {
    InvalidAPlus { value: f64 },
    InvalidAMinus { value: f64 },
}

impl Display for SufficientStatisticsIntervalStatisticsError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidAPlus { value } => {
                write!(
                    formatter,
                    "a_plus must be finite and non-negative, found {value}"
                )
            }
            Self::InvalidAMinus { value } => {
                write!(
                    formatter,
                    "a_minus must be finite and non-negative, found {value}"
                )
            }
        }
    }
}

impl Error for SufficientStatisticsIntervalStatisticsError {}

/// Snapshot metadata for one deterministic boundary-state update step.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct SufficientStatisticsBoundaryStateUpdateSnapshot {
    pub interval_index: usize,
    pub interval_delta: f64,
    pub state_before_update: HawkesExcitationState,
    pub sampled_statistics: SufficientStatisticsIntervalStatistics,
}

impl SufficientStatisticsBoundaryStateUpdateSnapshot {
    pub const fn new(
        interval_index: usize,
        interval_delta: f64,
        state_before_update: HawkesExcitationState,
        sampled_statistics: SufficientStatisticsIntervalStatistics,
    ) -> Self {
        Self {
            interval_index,
            interval_delta,
            state_before_update,
            sampled_statistics,
        }
    }

    pub const fn policy() -> &'static str {
        SUFFICIENT_STATISTICS_BOUNDARY_STATE_UPDATE_POLICY
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SufficientStatisticsBoundaryStateUpdateError {
    InvalidIntervalDelta {
        snapshot: SufficientStatisticsBoundaryStateUpdateSnapshot,
        value: f64,
    },
    InvalidStateBeforeUpdate {
        snapshot: SufficientStatisticsBoundaryStateUpdateSnapshot,
        field: &'static str,
        value: f64,
    },
    InvalidSampledStatistics {
        snapshot: SufficientStatisticsBoundaryStateUpdateSnapshot,
        source: SufficientStatisticsIntervalStatisticsError,
    },
    InvalidScaledKernelLoading {
        snapshot: SufficientStatisticsBoundaryStateUpdateSnapshot,
        side: HawkesEventSide,
        event_count: u64,
        field: &'static str,
        value: f64,
    },
    TransitionFailure {
        snapshot: SufficientStatisticsBoundaryStateUpdateSnapshot,
        source: HawkesTransitionError,
    },
    InvalidStateAfterUpdate {
        snapshot: SufficientStatisticsBoundaryStateUpdateSnapshot,
        field: &'static str,
        value: f64,
    },
}

impl Display for SufficientStatisticsBoundaryStateUpdateError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidIntervalDelta { snapshot, value } => write!(
                formatter,
                "boundary-state update failed for interval {}: interval_delta must be finite and strictly positive, found {value}; state_before_update={:?}, sampled_statistics={:?}",
                snapshot.interval_index, snapshot.state_before_update, snapshot.sampled_statistics
            ),
            Self::InvalidStateBeforeUpdate {
                snapshot,
                field,
                value,
            } => write!(
                formatter,
                "boundary-state update failed for interval {}: state_before_update.{field} must be finite and non-negative, found {value}; state_before_update={:?}, sampled_statistics={:?}",
                snapshot.interval_index, snapshot.state_before_update, snapshot.sampled_statistics
            ),
            Self::InvalidSampledStatistics { snapshot, source } => write!(
                formatter,
                "boundary-state update failed for interval {}: sampled_statistics are invalid: {source}; state_before_update={:?}, sampled_statistics={:?}",
                snapshot.interval_index, snapshot.state_before_update, snapshot.sampled_statistics
            ),
            Self::InvalidScaledKernelLoading {
                snapshot,
                side,
                event_count,
                field,
                value,
            } => write!(
                formatter,
                "boundary-state update failed for interval {}: scaled loading {field} for side {side:?} with event_count {} must be finite and non-negative, found {value}; state_before_update={:?}, sampled_statistics={:?}",
                snapshot.interval_index,
                event_count,
                snapshot.state_before_update,
                snapshot.sampled_statistics
            ),
            Self::TransitionFailure { snapshot, source } => write!(
                formatter,
                "boundary-state update failed for interval {} during canonical transition composition: {source}; state_before_update={:?}, sampled_statistics={:?}",
                snapshot.interval_index, snapshot.state_before_update, snapshot.sampled_statistics
            ),
            Self::InvalidStateAfterUpdate {
                snapshot,
                field,
                value,
            } => write!(
                formatter,
                "boundary-state update failed for interval {}: state_after_update.{field} must be finite and non-negative, found {value}; state_before_update={:?}, sampled_statistics={:?}",
                snapshot.interval_index, snapshot.state_before_update, snapshot.sampled_statistics
            ),
        }
    }
}

impl Error for SufficientStatisticsBoundaryStateUpdateError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidSampledStatistics { source, .. } => Some(source),
            Self::TransitionFailure { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Update one interval boundary state using sampled sufficient statistics.
///
/// Composition order:
/// 1. shared-decay drift over `interval_delta`
/// 2. buy-count mapped endogenous jump
/// 3. sell-count mapped endogenous jump
pub fn update_boundary_state_from_interval_statistics(
    kernel_parameters: HawkesKernelParameters,
    snapshot: SufficientStatisticsBoundaryStateUpdateSnapshot,
) -> Result<HawkesExcitationState, SufficientStatisticsBoundaryStateUpdateError> {
    if !snapshot.interval_delta.is_finite() || snapshot.interval_delta <= 0.0 {
        return Err(
            SufficientStatisticsBoundaryStateUpdateError::InvalidIntervalDelta {
                snapshot,
                value: snapshot.interval_delta,
            },
        );
    }

    validate_excitation_state(snapshot.state_before_update).map_err(|(field, value)| {
        SufficientStatisticsBoundaryStateUpdateError::InvalidStateBeforeUpdate {
            snapshot,
            field,
            value,
        }
    })?;

    snapshot.sampled_statistics.validate().map_err(|source| {
        SufficientStatisticsBoundaryStateUpdateError::InvalidSampledStatistics { snapshot, source }
    })?;

    let drifted_state =
        evolve_hawkes_state(
            snapshot.state_before_update,
            kernel_parameters,
            HawkesTransitionStep::new(snapshot.interval_delta, None, None),
        )
        .map_err(|source| {
            SufficientStatisticsBoundaryStateUpdateError::TransitionFailure { snapshot, source }
        })?;

    let post_buy_state = apply_endogenous_count_mapped_jump(
        drifted_state,
        kernel_parameters,
        snapshot,
        HawkesEventSide::Buy,
        snapshot.sampled_statistics.n_plus,
    )?;

    let post_sell_state = apply_endogenous_count_mapped_jump(
        post_buy_state,
        kernel_parameters,
        snapshot,
        HawkesEventSide::Sell,
        snapshot.sampled_statistics.n_minus,
    )?;

    validate_excitation_state(post_sell_state).map_err(|(field, value)| {
        SufficientStatisticsBoundaryStateUpdateError::InvalidStateAfterUpdate {
            snapshot,
            field,
            value,
        }
    })?;

    Ok(post_sell_state)
}

fn apply_endogenous_count_mapped_jump(
    state_before_jump: HawkesExcitationState,
    kernel_parameters: HawkesKernelParameters,
    snapshot: SufficientStatisticsBoundaryStateUpdateSnapshot,
    side: HawkesEventSide,
    event_count: u64,
) -> Result<HawkesExcitationState, SufficientStatisticsBoundaryStateUpdateError> {
    if event_count == 0 {
        return Ok(state_before_jump);
    }

    let scaled_alpha = scale_kernel_loadings_for_event_count(
        kernel_parameters.alpha,
        snapshot,
        side,
        event_count,
    )?;
    let scaled_kernel_parameters = HawkesKernelParameters {
        alpha: scaled_alpha,
        ..kernel_parameters
    };

    evolve_hawkes_state(
        state_before_jump,
        scaled_kernel_parameters,
        HawkesTransitionStep::new(0.0, Some(side), None),
    )
    .map_err(
        |source| SufficientStatisticsBoundaryStateUpdateError::TransitionFailure {
            snapshot,
            source,
        },
    )
}

fn scale_kernel_loadings_for_event_count(
    loadings: HawkesKernelLoadings,
    snapshot: SufficientStatisticsBoundaryStateUpdateSnapshot,
    side: HawkesEventSide,
    event_count: u64,
) -> Result<HawkesKernelLoadings, SufficientStatisticsBoundaryStateUpdateError> {
    let event_count_scale = event_count as f64;

    match side {
        HawkesEventSide::Buy => {
            let scaled_plus_plus = loadings.plus_plus * event_count_scale;
            let scaled_minus_plus = loadings.minus_plus * event_count_scale;

            validate_scaled_loading(
                snapshot,
                side,
                event_count,
                "alpha.plus_plus",
                scaled_plus_plus,
            )?;
            validate_scaled_loading(
                snapshot,
                side,
                event_count,
                "alpha.minus_plus",
                scaled_minus_plus,
            )?;

            Ok(HawkesKernelLoadings {
                plus_plus: scaled_plus_plus,
                plus_minus: loadings.plus_minus,
                minus_plus: scaled_minus_plus,
                minus_minus: loadings.minus_minus,
            })
        }
        HawkesEventSide::Sell => {
            let scaled_plus_minus = loadings.plus_minus * event_count_scale;
            let scaled_minus_minus = loadings.minus_minus * event_count_scale;

            validate_scaled_loading(
                snapshot,
                side,
                event_count,
                "alpha.plus_minus",
                scaled_plus_minus,
            )?;
            validate_scaled_loading(
                snapshot,
                side,
                event_count,
                "alpha.minus_minus",
                scaled_minus_minus,
            )?;

            Ok(HawkesKernelLoadings {
                plus_plus: loadings.plus_plus,
                plus_minus: scaled_plus_minus,
                minus_plus: loadings.minus_plus,
                minus_minus: scaled_minus_minus,
            })
        }
    }
}

fn validate_scaled_loading(
    snapshot: SufficientStatisticsBoundaryStateUpdateSnapshot,
    side: HawkesEventSide,
    event_count: u64,
    field: &'static str,
    value: f64,
) -> Result<(), SufficientStatisticsBoundaryStateUpdateError> {
    if !value.is_finite() || value < 0.0 {
        return Err(
            SufficientStatisticsBoundaryStateUpdateError::InvalidScaledKernelLoading {
                snapshot,
                side,
                event_count,
                field,
                value,
            },
        );
    }

    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct SufficientStatisticsRiskReturnComponents {
    pub buy_side_return: f64,
    pub sell_side_return: f64,
    pub trading_time_return: f64,
    pub open_jump_return: f64,
    pub closed_gap_return: f64,
    pub total_return: f64,
}

impl SufficientStatisticsRiskReturnComponents {
    pub fn try_new(
        buy_side_return: f64,
        sell_side_return: f64,
        trading_time_return: f64,
        open_jump_return: f64,
        closed_gap_return: f64,
        total_return: f64,
    ) -> Result<Self, SufficientStatisticsRiskReturnComponentsError> {
        let return_components = Self {
            buy_side_return,
            sell_side_return,
            trading_time_return,
            open_jump_return,
            closed_gap_return,
            total_return,
        };
        return_components.validate()?;
        Ok(return_components)
    }

    pub fn validate(self) -> Result<(), SufficientStatisticsRiskReturnComponentsError> {
        validate_finite_component("buy_side_return", self.buy_side_return)?;
        validate_finite_component("sell_side_return", self.sell_side_return)?;
        validate_finite_component("trading_time_return", self.trading_time_return)?;
        validate_finite_component("open_jump_return", self.open_jump_return)?;
        validate_finite_component("closed_gap_return", self.closed_gap_return)?;
        validate_finite_component("total_return", self.total_return)?;

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SufficientStatisticsRiskReturnComponentsError {
    InvalidFiniteComponent { field: &'static str, value: f64 },
}

impl Display for SufficientStatisticsRiskReturnComponentsError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidFiniteComponent { field, value } => {
                write!(formatter, "{field} must be finite, found {value}")
            }
        }
    }
}

impl Error for SufficientStatisticsRiskReturnComponentsError {}

/// Optional controls for interval return generation from sampled sufficient statistics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SufficientStatisticsRiskReturnGenerationOptions {
    pub include_component_breakdown: bool,
}

impl SufficientStatisticsRiskReturnGenerationOptions {
    pub const fn component_breakdown_policy() -> &'static str {
        SUFFICIENT_STATISTICS_INTERVAL_RETURN_COMPONENT_BREAKDOWN_POLICY
    }
}

impl Default for SufficientStatisticsRiskReturnGenerationOptions {
    fn default() -> Self {
        Self {
            include_component_breakdown: false,
        }
    }
}

/// One generated interval return with optional component breakdown.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SufficientStatisticsGeneratedIntervalReturn {
    pub total_return: f64,
    pub component_breakdown: Option<SufficientStatisticsRiskReturnComponents>,
}

impl SufficientStatisticsGeneratedIntervalReturn {
    pub const fn generation_policy() -> &'static str {
        SUFFICIENT_STATISTICS_INTERVAL_RETURN_GENERATION_POLICY
    }

    pub const fn component_routing_policy() -> &'static str {
        SUFFICIENT_STATISTICS_INTERVAL_RETURN_COMPONENT_ROUTING_POLICY
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SufficientStatisticsRiskReturnGenerationError {
    InvalidSampledStatistics {
        source: SufficientStatisticsIntervalStatisticsError,
    },
    InvalidOpenJumpReturn {
        value: f64,
    },
    InvalidClosedGapReturn {
        value: f64,
    },
    InvalidReturnDecomposition {
        source: ReturnDecompositionValidationError,
    },
    TradingTimeSamplingFailure {
        source: NigTradingTimeIncrementSamplingError,
    },
    InvalidReturnComponents {
        source: SufficientStatisticsRiskReturnComponentsError,
    },
    InvalidTotalReturn {
        value: f64,
    },
}

impl Display for SufficientStatisticsRiskReturnGenerationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidSampledStatistics { source } => {
                write!(formatter, "sampled_statistics is invalid: {source}")
            }
            Self::InvalidOpenJumpReturn { value } => {
                write!(
                    formatter,
                    "open_jump_return input must be finite, found {value}"
                )
            }
            Self::InvalidClosedGapReturn { value } => {
                write!(
                    formatter,
                    "closed_gap_return input must be finite, found {value}"
                )
            }
            Self::InvalidReturnDecomposition { source } => {
                write!(
                    formatter,
                    "return decomposition construction failed: {source}"
                )
            }
            Self::TradingTimeSamplingFailure { source } => {
                write!(
                    formatter,
                    "trading-time increment sampling failed: {source}"
                )
            }
            Self::InvalidReturnComponents { source } => {
                write!(
                    formatter,
                    "generated return components are invalid: {source}"
                )
            }
            Self::InvalidTotalReturn { value } => {
                write!(
                    formatter,
                    "generated total_return must be finite, found {value}"
                )
            }
        }
    }
}

impl Error for SufficientStatisticsRiskReturnGenerationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidSampledStatistics { source } => Some(source),
            Self::InvalidReturnDecomposition { source } => Some(source),
            Self::TradingTimeSamplingFailure { source } => Some(source),
            Self::InvalidReturnComponents { source } => Some(source),
            _ => None,
        }
    }
}

/// Generate one interval return increment from sampled sufficient statistics.
///
/// Composition:
/// - samples trading-time increment via the shared return-decomposition API
/// - routes open-jump and closed-gap inputs by market state
/// - emits optional component breakdown for diagnostics
pub fn generate_interval_return_from_sampled_statistics<R>(
    driver_parameters: NigBuySellDriverParameters,
    sampled_statistics: SufficientStatisticsIntervalStatistics,
    market_state: MarketState,
    open_jump_return: f64,
    closed_gap_return: f64,
    randomness_stream: &mut R,
    options: Option<SufficientStatisticsRiskReturnGenerationOptions>,
) -> Result<
    SufficientStatisticsGeneratedIntervalReturn,
    SufficientStatisticsRiskReturnGenerationError,
>
where
    R: SimulationRandomnessStream,
{
    sampled_statistics.validate().map_err(|source| {
        SufficientStatisticsRiskReturnGenerationError::InvalidSampledStatistics { source }
    })?;

    if !open_jump_return.is_finite() {
        return Err(
            SufficientStatisticsRiskReturnGenerationError::InvalidOpenJumpReturn {
                value: open_jump_return,
            },
        );
    }
    if !closed_gap_return.is_finite() {
        return Err(
            SufficientStatisticsRiskReturnGenerationError::InvalidClosedGapReturn {
                value: closed_gap_return,
            },
        );
    }

    let (effective_open_jump_return, effective_closed_gap_return) =
        market_state_routed_return_jumps(market_state, open_jump_return, closed_gap_return);
    let return_decomposition = ReturnDecomposition::try_new(ReturnDecompositionInput {
        a_plus: sampled_statistics.a_plus,
        a_minus: sampled_statistics.a_minus,
        open_jump: effective_open_jump_return,
        closed_gap: effective_closed_gap_return,
    })
    .map_err(|source| {
        SufficientStatisticsRiskReturnGenerationError::InvalidReturnDecomposition { source }
    })?;

    let trading_time_contributions = return_decomposition
        .sample_nig_trading_time_increment(driver_parameters, randomness_stream)
        .map_err(|source| {
            SufficientStatisticsRiskReturnGenerationError::TradingTimeSamplingFailure { source }
        })?;

    let buy_side_return = trading_time_contributions.buy_side_increment;
    let sell_side_return = -trading_time_contributions.sell_side_increment;
    let trading_time_return = trading_time_contributions.combined_increment;
    let total_return =
        trading_time_return + effective_open_jump_return + effective_closed_gap_return;
    if !total_return.is_finite() {
        return Err(
            SufficientStatisticsRiskReturnGenerationError::InvalidTotalReturn {
                value: total_return,
            },
        );
    }

    let include_component_breakdown = options.unwrap_or_default().include_component_breakdown;
    let component_breakdown = if include_component_breakdown {
        Some(
            SufficientStatisticsRiskReturnComponents::try_new(
                buy_side_return,
                sell_side_return,
                trading_time_return,
                effective_open_jump_return,
                effective_closed_gap_return,
                total_return,
            )
            .map_err(|source| {
                SufficientStatisticsRiskReturnGenerationError::InvalidReturnComponents { source }
            })?,
        )
    } else {
        None
    };

    Ok(SufficientStatisticsGeneratedIntervalReturn {
        total_return,
        component_breakdown,
    })
}

fn market_state_routed_return_jumps(
    market_state: MarketState,
    open_jump_return: f64,
    closed_gap_return: f64,
) -> (f64, f64) {
    match market_state {
        MarketState::Open => (open_jump_return, 0.0),
        MarketState::Closed => (0.0, closed_gap_return),
    }
}

/// Machine-readable schema metadata for risk-path outputs and downstream artifact validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct SufficientStatisticsRiskPathOutputSchemaMetadata {
    pub schema_version: &'static str,
    pub provenance_schema_version: &'static str,
    pub diagnostics_schema_version: &'static str,
    pub artifact_workflow_policy: &'static str,
    pub environment_metadata_policy: &'static str,
    pub required_path_fields: &'static [&'static str],
    pub required_provenance_fields: &'static [&'static str],
    pub required_diagnostics_fields: &'static [&'static str],
}

pub const SUFFICIENT_STATISTICS_RISK_PATH_OUTPUT_SCHEMA_METADATA:
    SufficientStatisticsRiskPathOutputSchemaMetadata =
    SufficientStatisticsRiskPathOutputSchemaMetadata {
        schema_version: SUFFICIENT_STATISTICS_RISK_PATH_OUTPUT_SCHEMA_METADATA_VERSION,
        provenance_schema_version: SUFFICIENT_STATISTICS_RISK_PATH_PROVENANCE_SCHEMA_VERSION,
        diagnostics_schema_version: SUFFICIENT_STATISTICS_RISK_PATH_DIAGNOSTICS_SCHEMA_VERSION,
        artifact_workflow_policy: SUFFICIENT_STATISTICS_RISK_PATH_ARTIFACT_WORKFLOW_POLICY,
        environment_metadata_policy: SUFFICIENT_STATISTICS_RISK_PATH_ENVIRONMENT_METADATA_POLICY,
        required_path_fields: &SUFFICIENT_STATISTICS_RISK_PATH_REQUIRED_FIELDS,
        required_provenance_fields: &SUFFICIENT_STATISTICS_RISK_PATH_REQUIRED_PROVENANCE_FIELDS,
        required_diagnostics_fields: &SUFFICIENT_STATISTICS_RISK_PATH_REQUIRED_DIAGNOSTICS_FIELDS,
    };

/// Deterministic provenance payload for reproducible risk-path artifacts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct SufficientStatisticsRiskPathProvenance {
    pub schema_version: &'static str,
    pub artifact_workflow_policy: &'static str,
    pub environment_metadata_policy: &'static str,
    pub interval_factor_policy: &'static str,
    pub transform_slice_sampling_policy: &'static str,
    pub boundary_state_update_policy: &'static str,
    pub interval_return_generation_policy: &'static str,
    pub interval_return_component_routing_policy: &'static str,
    pub batch_seed_partition_policy: &'static str,
    pub artifact_workflow_tags: &'static [&'static str],
}

pub const SUFFICIENT_STATISTICS_RISK_PATH_PROVENANCE: SufficientStatisticsRiskPathProvenance =
    SufficientStatisticsRiskPathProvenance {
        schema_version: SUFFICIENT_STATISTICS_RISK_PATH_PROVENANCE_SCHEMA_VERSION,
        artifact_workflow_policy: SUFFICIENT_STATISTICS_RISK_PATH_ARTIFACT_WORKFLOW_POLICY,
        environment_metadata_policy: SUFFICIENT_STATISTICS_RISK_PATH_ENVIRONMENT_METADATA_POLICY,
        interval_factor_policy: SUFFICIENT_STATISTICS_RISK_INTERVAL_FACTOR_POLICY,
        transform_slice_sampling_policy: SUFFICIENT_STATISTICS_TRANSFORM_SLICE_SAMPLING_POLICY,
        boundary_state_update_policy: SUFFICIENT_STATISTICS_BOUNDARY_STATE_UPDATE_POLICY,
        interval_return_generation_policy: SUFFICIENT_STATISTICS_INTERVAL_RETURN_GENERATION_POLICY,
        interval_return_component_routing_policy:
            SUFFICIENT_STATISTICS_INTERVAL_RETURN_COMPONENT_ROUTING_POLICY,
        batch_seed_partition_policy: SUFFICIENT_STATISTICS_BATCH_SEED_PARTITION_POLICY,
        artifact_workflow_tags: &SUFFICIENT_STATISTICS_RISK_PATH_ARTIFACT_WORKFLOW_TAGS,
    };

/// Concise deterministic diagnostics for triage and downstream validation reports.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct SufficientStatisticsRiskPathDiagnostics {
    pub schema_version: &'static str,
    pub simulated_interval_count: usize,
    pub first_interval_index: Option<usize>,
    pub last_interval_index: Option<usize>,
    pub cumulative_a_plus: f64,
    pub cumulative_a_minus: f64,
    pub cumulative_n_plus: u64,
    pub cumulative_n_minus: u64,
    pub cumulative_trading_time_return: f64,
    pub cumulative_open_jump_return: f64,
    pub cumulative_closed_gap_return: f64,
    pub cumulative_total_return: f64,
    pub max_abs_total_return: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SufficientStatisticsRiskPathRecord {
    pub schema_version: &'static str,
    pub interval_index: usize,
    pub sufficient_statistics: SufficientStatisticsIntervalStatistics,
    pub return_components: SufficientStatisticsRiskReturnComponents,
}

impl SufficientStatisticsRiskPathRecord {
    pub fn try_new(
        interval_index: usize,
        sufficient_statistics: SufficientStatisticsIntervalStatistics,
        return_components: SufficientStatisticsRiskReturnComponents,
    ) -> Result<Self, SufficientStatisticsRiskPathRecordError> {
        let path_record = Self {
            schema_version: SUFFICIENT_STATISTICS_RISK_PATH_RECORD_SCHEMA_VERSION,
            interval_index,
            sufficient_statistics,
            return_components,
        };
        path_record.validate()?;
        Ok(path_record)
    }

    pub fn validate(&self) -> Result<(), SufficientStatisticsRiskPathRecordError> {
        self.sufficient_statistics.validate().map_err(|source| {
            SufficientStatisticsRiskPathRecordError::InvalidSufficientStatistics { source }
        })?;
        self.return_components.validate().map_err(|source| {
            SufficientStatisticsRiskPathRecordError::InvalidReturnComponents { source }
        })?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SufficientStatisticsRiskPathRecordError {
    InvalidSufficientStatistics {
        source: SufficientStatisticsIntervalStatisticsError,
    },
    InvalidReturnComponents {
        source: SufficientStatisticsRiskReturnComponentsError,
    },
}

impl Display for SufficientStatisticsRiskPathRecordError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidSufficientStatistics { source } => {
                write!(formatter, "sufficient_statistics is invalid: {source}")
            }
            Self::InvalidReturnComponents { source } => {
                write!(formatter, "return_components is invalid: {source}")
            }
        }
    }
}

impl Error for SufficientStatisticsRiskPathRecordError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidSufficientStatistics { source } => Some(source),
            Self::InvalidReturnComponents { source } => Some(source),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SufficientStatisticsRiskPath {
    pub schema_version: &'static str,
    pub run_request_schema_version: &'static str,
    pub output_schema_metadata: SufficientStatisticsRiskPathOutputSchemaMetadata,
    pub provenance: SufficientStatisticsRiskPathProvenance,
    pub diagnostics: SufficientStatisticsRiskPathDiagnostics,
    pub seed: u64,
    pub interval_delta: f64,
    pub simulated_interval_count: usize,
    pub initial_excitation_state: HawkesExcitationState,
    pub terminal_excitation_state: HawkesExcitationState,
    pub records: Vec<SufficientStatisticsRiskPathRecord>,
}

impl SufficientStatisticsRiskPath {
    #[allow(clippy::too_many_arguments)]
    pub fn try_new(
        seed: u64,
        interval_delta: f64,
        simulated_interval_count: usize,
        initial_excitation_state: HawkesExcitationState,
        terminal_excitation_state: HawkesExcitationState,
        records: Vec<SufficientStatisticsRiskPathRecord>,
    ) -> Result<Self, SufficientStatisticsRiskPathError> {
        let diagnostics =
            derive_sufficient_statistics_risk_path_diagnostics(simulated_interval_count, &records)?;
        let path = Self {
            schema_version: SUFFICIENT_STATISTICS_RISK_PATH_SCHEMA_VERSION,
            run_request_schema_version: SUFFICIENT_STATISTICS_RISK_RUN_REQUEST_SCHEMA_VERSION,
            output_schema_metadata: SUFFICIENT_STATISTICS_RISK_PATH_OUTPUT_SCHEMA_METADATA,
            provenance: SUFFICIENT_STATISTICS_RISK_PATH_PROVENANCE,
            diagnostics,
            seed,
            interval_delta,
            simulated_interval_count,
            initial_excitation_state,
            terminal_excitation_state,
            records,
        };

        path.validate()?;
        Ok(path)
    }

    pub fn validate(&self) -> Result<(), SufficientStatisticsRiskPathError> {
        if !self.interval_delta.is_finite() || self.interval_delta <= 0.0 {
            return Err(SufficientStatisticsRiskPathError::InvalidIntervalDelta {
                value: self.interval_delta,
            });
        }

        if self.simulated_interval_count == 0 {
            return Err(
                SufficientStatisticsRiskPathError::InvalidSimulatedIntervalCount { value: 0 },
            );
        }

        validate_risk_path_output_schema_metadata(self.output_schema_metadata)?;
        validate_risk_path_provenance(self.provenance)?;

        if self.records.len() != self.simulated_interval_count {
            return Err(SufficientStatisticsRiskPathError::RecordCountMismatch {
                simulated_interval_count: self.simulated_interval_count,
                record_count: self.records.len(),
            });
        }

        validate_excitation_state(self.initial_excitation_state).map_err(|(field, value)| {
            SufficientStatisticsRiskPathError::InvalidInitialState { field, value }
        })?;
        validate_excitation_state(self.terminal_excitation_state).map_err(|(field, value)| {
            SufficientStatisticsRiskPathError::InvalidTerminalState { field, value }
        })?;

        for (record_position, record) in self.records.iter().enumerate() {
            if record.interval_index != record_position {
                return Err(
                    SufficientStatisticsRiskPathError::NonSequentialIntervalIndex {
                        record_position,
                        record_interval_index: record.interval_index,
                    },
                );
            }

            record.validate().map_err(|source| {
                SufficientStatisticsRiskPathError::InvalidRecord {
                    record_position,
                    source,
                }
            })?;
        }

        validate_risk_path_diagnostics(
            self.simulated_interval_count,
            &self.records,
            self.diagnostics,
        )?;

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SufficientStatisticsRiskPathError {
    InvalidIntervalDelta {
        value: f64,
    },
    InvalidSimulatedIntervalCount {
        value: usize,
    },
    RecordCountMismatch {
        simulated_interval_count: usize,
        record_count: usize,
    },
    InvalidInitialState {
        field: &'static str,
        value: f64,
    },
    InvalidTerminalState {
        field: &'static str,
        value: f64,
    },
    NonSequentialIntervalIndex {
        record_position: usize,
        record_interval_index: usize,
    },
    InvalidRecord {
        record_position: usize,
        source: SufficientStatisticsRiskPathRecordError,
    },
    InvalidOutputSchemaMetadataField {
        field_path: &'static str,
        expected: String,
        actual: String,
    },
    InvalidProvenanceField {
        field_path: &'static str,
        expected: String,
        actual: String,
    },
    InvalidDiagnosticsField {
        field_path: String,
        expected: String,
        actual: String,
    },
}

impl Display for SufficientStatisticsRiskPathError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidIntervalDelta { value } => write!(
                formatter,
                "interval_delta must be finite and strictly positive, found {value}"
            ),
            Self::InvalidSimulatedIntervalCount { value } => write!(
                formatter,
                "simulated_interval_count must be strictly positive, found {value}"
            ),
            Self::RecordCountMismatch {
                simulated_interval_count,
                record_count,
            } => write!(
                formatter,
                "simulated_interval_count ({simulated_interval_count}) must match records length ({record_count})"
            ),
            Self::InvalidInitialState { field, value } => write!(
                formatter,
                "initial_excitation_state.{field} must be finite and non-negative, found {value}"
            ),
            Self::InvalidTerminalState { field, value } => write!(
                formatter,
                "terminal_excitation_state.{field} must be finite and non-negative, found {value}"
            ),
            Self::NonSequentialIntervalIndex {
                record_position,
                record_interval_index,
            } => write!(
                formatter,
                "records[{record_position}].interval_index must equal {record_position}, found {record_interval_index}"
            ),
            Self::InvalidRecord {
                record_position,
                source,
            } => write!(formatter, "records[{record_position}] is invalid: {source}"),
            Self::InvalidOutputSchemaMetadataField {
                field_path,
                expected,
                actual,
            } => write!(
                formatter,
                "{field_path} must be {expected} for deterministic risk-path schema metadata, found {actual}"
            ),
            Self::InvalidProvenanceField {
                field_path,
                expected,
                actual,
            } => write!(
                formatter,
                "{field_path} must be {expected} for deterministic risk-path provenance, found {actual}"
            ),
            Self::InvalidDiagnosticsField {
                field_path,
                expected,
                actual,
            } => write!(
                formatter,
                "{field_path} must be {expected} for deterministic risk-path diagnostics, found {actual}"
            ),
        }
    }
}

impl Error for SufficientStatisticsRiskPathError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidRecord { source, .. } => Some(source),
            _ => None,
        }
    }
}

fn validate_risk_path_output_schema_metadata(
    metadata: SufficientStatisticsRiskPathOutputSchemaMetadata,
) -> Result<(), SufficientStatisticsRiskPathError> {
    let expected = SUFFICIENT_STATISTICS_RISK_PATH_OUTPUT_SCHEMA_METADATA;

    if metadata.schema_version != expected.schema_version {
        return Err(
            SufficientStatisticsRiskPathError::InvalidOutputSchemaMetadataField {
                field_path: "output_schema_metadata.schema_version",
                expected: expected.schema_version.to_string(),
                actual: metadata.schema_version.to_string(),
            },
        );
    }
    if metadata.provenance_schema_version != expected.provenance_schema_version {
        return Err(
            SufficientStatisticsRiskPathError::InvalidOutputSchemaMetadataField {
                field_path: "output_schema_metadata.provenance_schema_version",
                expected: expected.provenance_schema_version.to_string(),
                actual: metadata.provenance_schema_version.to_string(),
            },
        );
    }
    if metadata.diagnostics_schema_version != expected.diagnostics_schema_version {
        return Err(
            SufficientStatisticsRiskPathError::InvalidOutputSchemaMetadataField {
                field_path: "output_schema_metadata.diagnostics_schema_version",
                expected: expected.diagnostics_schema_version.to_string(),
                actual: metadata.diagnostics_schema_version.to_string(),
            },
        );
    }
    if metadata.artifact_workflow_policy != expected.artifact_workflow_policy {
        return Err(
            SufficientStatisticsRiskPathError::InvalidOutputSchemaMetadataField {
                field_path: "output_schema_metadata.artifact_workflow_policy",
                expected: expected.artifact_workflow_policy.to_string(),
                actual: metadata.artifact_workflow_policy.to_string(),
            },
        );
    }
    if metadata.environment_metadata_policy != expected.environment_metadata_policy {
        return Err(
            SufficientStatisticsRiskPathError::InvalidOutputSchemaMetadataField {
                field_path: "output_schema_metadata.environment_metadata_policy",
                expected: expected.environment_metadata_policy.to_string(),
                actual: metadata.environment_metadata_policy.to_string(),
            },
        );
    }
    if metadata.required_path_fields != expected.required_path_fields {
        return Err(
            SufficientStatisticsRiskPathError::InvalidOutputSchemaMetadataField {
                field_path: "output_schema_metadata.required_path_fields",
                expected: format!("{:?}", expected.required_path_fields),
                actual: format!("{:?}", metadata.required_path_fields),
            },
        );
    }
    if metadata.required_provenance_fields != expected.required_provenance_fields {
        return Err(
            SufficientStatisticsRiskPathError::InvalidOutputSchemaMetadataField {
                field_path: "output_schema_metadata.required_provenance_fields",
                expected: format!("{:?}", expected.required_provenance_fields),
                actual: format!("{:?}", metadata.required_provenance_fields),
            },
        );
    }
    if metadata.required_diagnostics_fields != expected.required_diagnostics_fields {
        return Err(
            SufficientStatisticsRiskPathError::InvalidOutputSchemaMetadataField {
                field_path: "output_schema_metadata.required_diagnostics_fields",
                expected: format!("{:?}", expected.required_diagnostics_fields),
                actual: format!("{:?}", metadata.required_diagnostics_fields),
            },
        );
    }

    Ok(())
}

fn validate_risk_path_provenance(
    provenance: SufficientStatisticsRiskPathProvenance,
) -> Result<(), SufficientStatisticsRiskPathError> {
    let expected = SUFFICIENT_STATISTICS_RISK_PATH_PROVENANCE;

    if provenance.schema_version != expected.schema_version {
        return Err(SufficientStatisticsRiskPathError::InvalidProvenanceField {
            field_path: "provenance.schema_version",
            expected: expected.schema_version.to_string(),
            actual: provenance.schema_version.to_string(),
        });
    }
    if provenance.artifact_workflow_policy != expected.artifact_workflow_policy {
        return Err(SufficientStatisticsRiskPathError::InvalidProvenanceField {
            field_path: "provenance.artifact_workflow_policy",
            expected: expected.artifact_workflow_policy.to_string(),
            actual: provenance.artifact_workflow_policy.to_string(),
        });
    }
    if provenance.environment_metadata_policy != expected.environment_metadata_policy {
        return Err(SufficientStatisticsRiskPathError::InvalidProvenanceField {
            field_path: "provenance.environment_metadata_policy",
            expected: expected.environment_metadata_policy.to_string(),
            actual: provenance.environment_metadata_policy.to_string(),
        });
    }
    if provenance.interval_factor_policy != expected.interval_factor_policy {
        return Err(SufficientStatisticsRiskPathError::InvalidProvenanceField {
            field_path: "provenance.interval_factor_policy",
            expected: expected.interval_factor_policy.to_string(),
            actual: provenance.interval_factor_policy.to_string(),
        });
    }
    if provenance.transform_slice_sampling_policy != expected.transform_slice_sampling_policy {
        return Err(SufficientStatisticsRiskPathError::InvalidProvenanceField {
            field_path: "provenance.transform_slice_sampling_policy",
            expected: expected.transform_slice_sampling_policy.to_string(),
            actual: provenance.transform_slice_sampling_policy.to_string(),
        });
    }
    if provenance.boundary_state_update_policy != expected.boundary_state_update_policy {
        return Err(SufficientStatisticsRiskPathError::InvalidProvenanceField {
            field_path: "provenance.boundary_state_update_policy",
            expected: expected.boundary_state_update_policy.to_string(),
            actual: provenance.boundary_state_update_policy.to_string(),
        });
    }
    if provenance.interval_return_generation_policy != expected.interval_return_generation_policy {
        return Err(SufficientStatisticsRiskPathError::InvalidProvenanceField {
            field_path: "provenance.interval_return_generation_policy",
            expected: expected.interval_return_generation_policy.to_string(),
            actual: provenance.interval_return_generation_policy.to_string(),
        });
    }
    if provenance.interval_return_component_routing_policy
        != expected.interval_return_component_routing_policy
    {
        return Err(SufficientStatisticsRiskPathError::InvalidProvenanceField {
            field_path: "provenance.interval_return_component_routing_policy",
            expected: expected
                .interval_return_component_routing_policy
                .to_string(),
            actual: provenance
                .interval_return_component_routing_policy
                .to_string(),
        });
    }
    if provenance.batch_seed_partition_policy != expected.batch_seed_partition_policy {
        return Err(SufficientStatisticsRiskPathError::InvalidProvenanceField {
            field_path: "provenance.batch_seed_partition_policy",
            expected: expected.batch_seed_partition_policy.to_string(),
            actual: provenance.batch_seed_partition_policy.to_string(),
        });
    }
    if provenance.artifact_workflow_tags != expected.artifact_workflow_tags {
        return Err(SufficientStatisticsRiskPathError::InvalidProvenanceField {
            field_path: "provenance.artifact_workflow_tags",
            expected: format!("{:?}", expected.artifact_workflow_tags),
            actual: format!("{:?}", provenance.artifact_workflow_tags),
        });
    }

    Ok(())
}

fn derive_sufficient_statistics_risk_path_diagnostics(
    simulated_interval_count: usize,
    records: &[SufficientStatisticsRiskPathRecord],
) -> Result<SufficientStatisticsRiskPathDiagnostics, SufficientStatisticsRiskPathError> {
    let mut cumulative_a_plus = 0.0;
    let mut cumulative_a_minus = 0.0;
    let mut cumulative_n_plus: u64 = 0;
    let mut cumulative_n_minus: u64 = 0;
    let mut cumulative_trading_time_return = 0.0;
    let mut cumulative_open_jump_return = 0.0;
    let mut cumulative_closed_gap_return = 0.0;
    let mut cumulative_total_return = 0.0;
    let mut max_abs_total_return: f64 = 0.0;

    for (record_position, record) in records.iter().enumerate() {
        cumulative_a_plus += record.sufficient_statistics.a_plus;
        if !cumulative_a_plus.is_finite() {
            return Err(SufficientStatisticsRiskPathError::InvalidDiagnosticsField {
                field_path: "diagnostics.cumulative_a_plus".to_string(),
                expected: "finite cumulative sum".to_string(),
                actual: format!(
                    "non-finite value after records[{record_position}].sufficient_statistics.a_plus aggregation"
                ),
            });
        }

        cumulative_a_minus += record.sufficient_statistics.a_minus;
        if !cumulative_a_minus.is_finite() {
            return Err(SufficientStatisticsRiskPathError::InvalidDiagnosticsField {
                field_path: "diagnostics.cumulative_a_minus".to_string(),
                expected: "finite cumulative sum".to_string(),
                actual: format!(
                    "non-finite value after records[{record_position}].sufficient_statistics.a_minus aggregation"
                ),
            });
        }

        cumulative_n_plus = cumulative_n_plus
            .checked_add(record.sufficient_statistics.n_plus)
            .ok_or(SufficientStatisticsRiskPathError::InvalidDiagnosticsField {
                field_path: "diagnostics.cumulative_n_plus".to_string(),
                expected: "non-overflowing u64 cumulative count".to_string(),
                actual: "overflow during cumulative_n_plus aggregation".to_string(),
            })?;

        cumulative_n_minus = cumulative_n_minus
            .checked_add(record.sufficient_statistics.n_minus)
            .ok_or(SufficientStatisticsRiskPathError::InvalidDiagnosticsField {
                field_path: "diagnostics.cumulative_n_minus".to_string(),
                expected: "non-overflowing u64 cumulative count".to_string(),
                actual: "overflow during cumulative_n_minus aggregation".to_string(),
            })?;

        cumulative_trading_time_return += record.return_components.trading_time_return;
        if !cumulative_trading_time_return.is_finite() {
            return Err(SufficientStatisticsRiskPathError::InvalidDiagnosticsField {
                field_path: "diagnostics.cumulative_trading_time_return".to_string(),
                expected: "finite cumulative sum".to_string(),
                actual: format!(
                    "non-finite value after records[{record_position}].return_components.trading_time_return aggregation"
                ),
            });
        }

        cumulative_open_jump_return += record.return_components.open_jump_return;
        if !cumulative_open_jump_return.is_finite() {
            return Err(SufficientStatisticsRiskPathError::InvalidDiagnosticsField {
                field_path: "diagnostics.cumulative_open_jump_return".to_string(),
                expected: "finite cumulative sum".to_string(),
                actual: format!(
                    "non-finite value after records[{record_position}].return_components.open_jump_return aggregation"
                ),
            });
        }

        cumulative_closed_gap_return += record.return_components.closed_gap_return;
        if !cumulative_closed_gap_return.is_finite() {
            return Err(SufficientStatisticsRiskPathError::InvalidDiagnosticsField {
                field_path: "diagnostics.cumulative_closed_gap_return".to_string(),
                expected: "finite cumulative sum".to_string(),
                actual: format!(
                    "non-finite value after records[{record_position}].return_components.closed_gap_return aggregation"
                ),
            });
        }

        cumulative_total_return += record.return_components.total_return;
        if !cumulative_total_return.is_finite() {
            return Err(SufficientStatisticsRiskPathError::InvalidDiagnosticsField {
                field_path: "diagnostics.cumulative_total_return".to_string(),
                expected: "finite cumulative sum".to_string(),
                actual: format!(
                    "non-finite value after records[{record_position}].return_components.total_return aggregation"
                ),
            });
        }

        let absolute_total_return = record.return_components.total_return.abs();
        if !absolute_total_return.is_finite() {
            return Err(SufficientStatisticsRiskPathError::InvalidDiagnosticsField {
                field_path: "diagnostics.max_abs_total_return".to_string(),
                expected: "finite absolute total return".to_string(),
                actual: format!(
                    "non-finite value at records[{record_position}].return_components.total_return"
                ),
            });
        }

        max_abs_total_return = max_abs_total_return.max(absolute_total_return);
    }

    Ok(SufficientStatisticsRiskPathDiagnostics {
        schema_version: SUFFICIENT_STATISTICS_RISK_PATH_DIAGNOSTICS_SCHEMA_VERSION,
        simulated_interval_count,
        first_interval_index: records.first().map(|record| record.interval_index),
        last_interval_index: records.last().map(|record| record.interval_index),
        cumulative_a_plus,
        cumulative_a_minus,
        cumulative_n_plus,
        cumulative_n_minus,
        cumulative_trading_time_return,
        cumulative_open_jump_return,
        cumulative_closed_gap_return,
        cumulative_total_return,
        max_abs_total_return,
    })
}

fn validate_risk_path_diagnostics(
    simulated_interval_count: usize,
    records: &[SufficientStatisticsRiskPathRecord],
    diagnostics: SufficientStatisticsRiskPathDiagnostics,
) -> Result<(), SufficientStatisticsRiskPathError> {
    let expected_diagnostics =
        derive_sufficient_statistics_risk_path_diagnostics(simulated_interval_count, records)?;

    if diagnostics.schema_version != expected_diagnostics.schema_version {
        return Err(SufficientStatisticsRiskPathError::InvalidDiagnosticsField {
            field_path: "diagnostics.schema_version".to_string(),
            expected: expected_diagnostics.schema_version.to_string(),
            actual: diagnostics.schema_version.to_string(),
        });
    }
    if diagnostics.simulated_interval_count != expected_diagnostics.simulated_interval_count {
        return Err(SufficientStatisticsRiskPathError::InvalidDiagnosticsField {
            field_path: "diagnostics.simulated_interval_count".to_string(),
            expected: expected_diagnostics.simulated_interval_count.to_string(),
            actual: diagnostics.simulated_interval_count.to_string(),
        });
    }
    if diagnostics.first_interval_index != expected_diagnostics.first_interval_index {
        return Err(SufficientStatisticsRiskPathError::InvalidDiagnosticsField {
            field_path: "diagnostics.first_interval_index".to_string(),
            expected: format!("{:?}", expected_diagnostics.first_interval_index),
            actual: format!("{:?}", diagnostics.first_interval_index),
        });
    }
    if diagnostics.last_interval_index != expected_diagnostics.last_interval_index {
        return Err(SufficientStatisticsRiskPathError::InvalidDiagnosticsField {
            field_path: "diagnostics.last_interval_index".to_string(),
            expected: format!("{:?}", expected_diagnostics.last_interval_index),
            actual: format!("{:?}", diagnostics.last_interval_index),
        });
    }
    if diagnostics.cumulative_a_plus != expected_diagnostics.cumulative_a_plus {
        return Err(SufficientStatisticsRiskPathError::InvalidDiagnosticsField {
            field_path: "diagnostics.cumulative_a_plus".to_string(),
            expected: expected_diagnostics.cumulative_a_plus.to_string(),
            actual: diagnostics.cumulative_a_plus.to_string(),
        });
    }
    if diagnostics.cumulative_a_minus != expected_diagnostics.cumulative_a_minus {
        return Err(SufficientStatisticsRiskPathError::InvalidDiagnosticsField {
            field_path: "diagnostics.cumulative_a_minus".to_string(),
            expected: expected_diagnostics.cumulative_a_minus.to_string(),
            actual: diagnostics.cumulative_a_minus.to_string(),
        });
    }
    if diagnostics.cumulative_n_plus != expected_diagnostics.cumulative_n_plus {
        return Err(SufficientStatisticsRiskPathError::InvalidDiagnosticsField {
            field_path: "diagnostics.cumulative_n_plus".to_string(),
            expected: expected_diagnostics.cumulative_n_plus.to_string(),
            actual: diagnostics.cumulative_n_plus.to_string(),
        });
    }
    if diagnostics.cumulative_n_minus != expected_diagnostics.cumulative_n_minus {
        return Err(SufficientStatisticsRiskPathError::InvalidDiagnosticsField {
            field_path: "diagnostics.cumulative_n_minus".to_string(),
            expected: expected_diagnostics.cumulative_n_minus.to_string(),
            actual: diagnostics.cumulative_n_minus.to_string(),
        });
    }
    if diagnostics.cumulative_trading_time_return
        != expected_diagnostics.cumulative_trading_time_return
    {
        return Err(SufficientStatisticsRiskPathError::InvalidDiagnosticsField {
            field_path: "diagnostics.cumulative_trading_time_return".to_string(),
            expected: expected_diagnostics
                .cumulative_trading_time_return
                .to_string(),
            actual: diagnostics.cumulative_trading_time_return.to_string(),
        });
    }
    if diagnostics.cumulative_open_jump_return != expected_diagnostics.cumulative_open_jump_return {
        return Err(SufficientStatisticsRiskPathError::InvalidDiagnosticsField {
            field_path: "diagnostics.cumulative_open_jump_return".to_string(),
            expected: expected_diagnostics.cumulative_open_jump_return.to_string(),
            actual: diagnostics.cumulative_open_jump_return.to_string(),
        });
    }
    if diagnostics.cumulative_closed_gap_return != expected_diagnostics.cumulative_closed_gap_return
    {
        return Err(SufficientStatisticsRiskPathError::InvalidDiagnosticsField {
            field_path: "diagnostics.cumulative_closed_gap_return".to_string(),
            expected: expected_diagnostics
                .cumulative_closed_gap_return
                .to_string(),
            actual: diagnostics.cumulative_closed_gap_return.to_string(),
        });
    }
    if diagnostics.cumulative_total_return != expected_diagnostics.cumulative_total_return {
        return Err(SufficientStatisticsRiskPathError::InvalidDiagnosticsField {
            field_path: "diagnostics.cumulative_total_return".to_string(),
            expected: expected_diagnostics.cumulative_total_return.to_string(),
            actual: diagnostics.cumulative_total_return.to_string(),
        });
    }
    if diagnostics.max_abs_total_return != expected_diagnostics.max_abs_total_return {
        return Err(SufficientStatisticsRiskPathError::InvalidDiagnosticsField {
            field_path: "diagnostics.max_abs_total_return".to_string(),
            expected: expected_diagnostics.max_abs_total_return.to_string(),
            actual: diagnostics.max_abs_total_return.to_string(),
        });
    }

    Ok(())
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct SufficientStatisticsRiskIntervalFactorsPayload {
    #[serde(default)]
    interval_delta: Option<f64>,
    s_h: Vec<f64>,
    nu_h_known: Vec<f64>,
    nu_h_surprise: Vec<f64>,
}

impl<'de> Deserialize<'de> for SufficientStatisticsRiskIntervalFactors {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let payload = SufficientStatisticsRiskIntervalFactorsPayload::deserialize(deserializer)?;
        Self::try_new(
            payload.s_h,
            payload.nu_h_known,
            payload.nu_h_surprise,
            payload.interval_delta,
        )
        .map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct SufficientStatisticsRiskModelInputsPayload {
    equation_specification: ExponentialAffineTransformEquationSpecification,
    nig_shared_plus_shift_configuration: NigSharedPlusShiftConfiguration,
    initial_excitation_state: HawkesExcitationState,
}

impl<'de> Deserialize<'de> for SufficientStatisticsRiskModelInputs {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let payload = SufficientStatisticsRiskModelInputsPayload::deserialize(deserializer)?;
        Self::try_new(
            payload.equation_specification,
            payload.nig_shared_plus_shift_configuration,
            payload.initial_excitation_state,
        )
        .map_err(serde::de::Error::custom)
    }
}

const fn default_intraday_risk_interval_delta() -> f64 {
    DEFAULT_INTRADAY_RISK_INTERVAL_DELTA
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct SufficientStatisticsRiskRunRequestPayload {
    seed: u64,
    horizon_interval_count: usize,
    #[serde(default = "default_intraday_risk_interval_delta")]
    interval_delta: f64,
    model_inputs: SufficientStatisticsRiskModelInputs,
    interval_factors: SufficientStatisticsRiskIntervalFactors,
}

impl<'de> Deserialize<'de> for SufficientStatisticsRiskRunRequest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let payload = SufficientStatisticsRiskRunRequestPayload::deserialize(deserializer)?;
        Self::try_new(
            payload.seed,
            payload.horizon_interval_count,
            payload.interval_delta,
            payload.model_inputs,
            payload.interval_factors,
        )
        .map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct SufficientStatisticsIntervalStatisticsPayload {
    a_plus: f64,
    a_minus: f64,
    n_plus: u64,
    n_minus: u64,
}

impl<'de> Deserialize<'de> for SufficientStatisticsIntervalStatistics {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let payload = SufficientStatisticsIntervalStatisticsPayload::deserialize(deserializer)?;
        Self::try_new(
            payload.a_plus,
            payload.a_minus,
            payload.n_plus,
            payload.n_minus,
        )
        .map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct SufficientStatisticsRiskReturnComponentsPayload {
    buy_side_return: f64,
    sell_side_return: f64,
    trading_time_return: f64,
    open_jump_return: f64,
    closed_gap_return: f64,
    total_return: f64,
}

impl<'de> Deserialize<'de> for SufficientStatisticsRiskReturnComponents {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let payload = SufficientStatisticsRiskReturnComponentsPayload::deserialize(deserializer)?;
        Self::try_new(
            payload.buy_side_return,
            payload.sell_side_return,
            payload.trading_time_return,
            payload.open_jump_return,
            payload.closed_gap_return,
            payload.total_return,
        )
        .map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct SufficientStatisticsRiskPathRecordPayload {
    #[serde(default)]
    schema_version: Option<String>,
    interval_index: usize,
    sufficient_statistics: SufficientStatisticsIntervalStatistics,
    return_components: SufficientStatisticsRiskReturnComponents,
}

impl<'de> Deserialize<'de> for SufficientStatisticsRiskPathRecord {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let payload = SufficientStatisticsRiskPathRecordPayload::deserialize(deserializer)?;

        if let Some(schema_version) = payload.schema_version.as_deref()
            && schema_version != SUFFICIENT_STATISTICS_RISK_PATH_RECORD_SCHEMA_VERSION
        {
            return Err(serde::de::Error::custom(format!(
                "schema_version must be {}, found {schema_version}",
                SUFFICIENT_STATISTICS_RISK_PATH_RECORD_SCHEMA_VERSION
            )));
        }

        Self::try_new(
            payload.interval_index,
            payload.sufficient_statistics,
            payload.return_components,
        )
        .map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct SufficientStatisticsRiskPathPayload {
    #[serde(default)]
    schema_version: Option<String>,
    #[serde(default)]
    run_request_schema_version: Option<String>,
    #[serde(default)]
    output_schema_metadata: Option<SufficientStatisticsRiskPathOutputSchemaMetadataPayload>,
    #[serde(default)]
    provenance: Option<SufficientStatisticsRiskPathProvenancePayload>,
    #[serde(default)]
    diagnostics: Option<SufficientStatisticsRiskPathDiagnosticsPayload>,
    seed: u64,
    interval_delta: f64,
    simulated_interval_count: usize,
    initial_excitation_state: HawkesExcitationState,
    terminal_excitation_state: HawkesExcitationState,
    records: Vec<SufficientStatisticsRiskPathRecord>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct SufficientStatisticsRiskPathOutputSchemaMetadataPayload {
    schema_version: String,
    provenance_schema_version: String,
    diagnostics_schema_version: String,
    artifact_workflow_policy: String,
    environment_metadata_policy: String,
    required_path_fields: Vec<String>,
    required_provenance_fields: Vec<String>,
    required_diagnostics_fields: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct SufficientStatisticsRiskPathProvenancePayload {
    schema_version: String,
    artifact_workflow_policy: String,
    environment_metadata_policy: String,
    interval_factor_policy: String,
    transform_slice_sampling_policy: String,
    boundary_state_update_policy: String,
    interval_return_generation_policy: String,
    interval_return_component_routing_policy: String,
    batch_seed_partition_policy: String,
    artifact_workflow_tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct SufficientStatisticsRiskPathDiagnosticsPayload {
    schema_version: String,
    simulated_interval_count: usize,
    first_interval_index: Option<usize>,
    last_interval_index: Option<usize>,
    cumulative_a_plus: f64,
    cumulative_a_minus: f64,
    cumulative_n_plus: u64,
    cumulative_n_minus: u64,
    cumulative_trading_time_return: f64,
    cumulative_open_jump_return: f64,
    cumulative_closed_gap_return: f64,
    cumulative_total_return: f64,
    max_abs_total_return: f64,
}

impl<'de> Deserialize<'de> for SufficientStatisticsRiskPath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let payload = SufficientStatisticsRiskPathPayload::deserialize(deserializer)?;

        if let Some(schema_version) = payload.schema_version.as_deref()
            && schema_version != SUFFICIENT_STATISTICS_RISK_PATH_SCHEMA_VERSION
        {
            return Err(serde::de::Error::custom(format!(
                "schema_version must be {}, found {schema_version}",
                SUFFICIENT_STATISTICS_RISK_PATH_SCHEMA_VERSION
            )));
        }

        if let Some(run_request_schema_version) = payload.run_request_schema_version.as_deref()
            && run_request_schema_version != SUFFICIENT_STATISTICS_RISK_RUN_REQUEST_SCHEMA_VERSION
        {
            return Err(serde::de::Error::custom(format!(
                "run_request_schema_version must be {}, found {run_request_schema_version}",
                SUFFICIENT_STATISTICS_RISK_RUN_REQUEST_SCHEMA_VERSION
            )));
        }

        let path = Self::try_new(
            payload.seed,
            payload.interval_delta,
            payload.simulated_interval_count,
            payload.initial_excitation_state,
            payload.terminal_excitation_state,
            payload.records,
        )
        .map_err(serde::de::Error::custom)?;

        if let Some(output_schema_metadata) = payload.output_schema_metadata.as_ref() {
            validate_path_output_schema_metadata_payload(output_schema_metadata)
                .map_err(serde::de::Error::custom)?;
        }

        if let Some(provenance) = payload.provenance.as_ref() {
            validate_path_provenance_payload(provenance).map_err(serde::de::Error::custom)?;
        }

        if let Some(diagnostics) = payload.diagnostics.as_ref() {
            validate_path_diagnostics_payload(diagnostics, path.diagnostics)
                .map_err(serde::de::Error::custom)?;
        }

        Ok(path)
    }
}

fn validate_path_output_schema_metadata_payload(
    payload: &SufficientStatisticsRiskPathOutputSchemaMetadataPayload,
) -> Result<(), String> {
    let expected = SUFFICIENT_STATISTICS_RISK_PATH_OUTPUT_SCHEMA_METADATA;

    if payload.schema_version != expected.schema_version {
        return Err(format!(
            "output_schema_metadata.schema_version must be {}, found {}",
            expected.schema_version, payload.schema_version
        ));
    }
    if payload.provenance_schema_version != expected.provenance_schema_version {
        return Err(format!(
            "output_schema_metadata.provenance_schema_version must be {}, found {}",
            expected.provenance_schema_version, payload.provenance_schema_version
        ));
    }
    if payload.diagnostics_schema_version != expected.diagnostics_schema_version {
        return Err(format!(
            "output_schema_metadata.diagnostics_schema_version must be {}, found {}",
            expected.diagnostics_schema_version, payload.diagnostics_schema_version
        ));
    }
    if payload.artifact_workflow_policy != expected.artifact_workflow_policy {
        return Err(format!(
            "output_schema_metadata.artifact_workflow_policy must be {}, found {}",
            expected.artifact_workflow_policy, payload.artifact_workflow_policy
        ));
    }
    if payload.environment_metadata_policy != expected.environment_metadata_policy {
        return Err(format!(
            "output_schema_metadata.environment_metadata_policy must be {}, found {}",
            expected.environment_metadata_policy, payload.environment_metadata_policy
        ));
    }

    validate_required_field_list(
        "output_schema_metadata.required_path_fields",
        payload.required_path_fields.as_slice(),
        expected.required_path_fields,
    )?;
    validate_required_field_list(
        "output_schema_metadata.required_provenance_fields",
        payload.required_provenance_fields.as_slice(),
        expected.required_provenance_fields,
    )?;
    validate_required_field_list(
        "output_schema_metadata.required_diagnostics_fields",
        payload.required_diagnostics_fields.as_slice(),
        expected.required_diagnostics_fields,
    )?;

    Ok(())
}

fn validate_path_provenance_payload(
    payload: &SufficientStatisticsRiskPathProvenancePayload,
) -> Result<(), String> {
    let expected = SUFFICIENT_STATISTICS_RISK_PATH_PROVENANCE;

    if payload.schema_version != expected.schema_version {
        return Err(format!(
            "provenance.schema_version must be {}, found {}",
            expected.schema_version, payload.schema_version
        ));
    }
    if payload.artifact_workflow_policy != expected.artifact_workflow_policy {
        return Err(format!(
            "provenance.artifact_workflow_policy must be {}, found {}",
            expected.artifact_workflow_policy, payload.artifact_workflow_policy
        ));
    }
    if payload.environment_metadata_policy != expected.environment_metadata_policy {
        return Err(format!(
            "provenance.environment_metadata_policy must be {}, found {}",
            expected.environment_metadata_policy, payload.environment_metadata_policy
        ));
    }
    if payload.interval_factor_policy != expected.interval_factor_policy {
        return Err(format!(
            "provenance.interval_factor_policy must be {}, found {}",
            expected.interval_factor_policy, payload.interval_factor_policy
        ));
    }
    if payload.transform_slice_sampling_policy != expected.transform_slice_sampling_policy {
        return Err(format!(
            "provenance.transform_slice_sampling_policy must be {}, found {}",
            expected.transform_slice_sampling_policy, payload.transform_slice_sampling_policy
        ));
    }
    if payload.boundary_state_update_policy != expected.boundary_state_update_policy {
        return Err(format!(
            "provenance.boundary_state_update_policy must be {}, found {}",
            expected.boundary_state_update_policy, payload.boundary_state_update_policy
        ));
    }
    if payload.interval_return_generation_policy != expected.interval_return_generation_policy {
        return Err(format!(
            "provenance.interval_return_generation_policy must be {}, found {}",
            expected.interval_return_generation_policy, payload.interval_return_generation_policy
        ));
    }
    if payload.interval_return_component_routing_policy
        != expected.interval_return_component_routing_policy
    {
        return Err(format!(
            "provenance.interval_return_component_routing_policy must be {}, found {}",
            expected.interval_return_component_routing_policy,
            payload.interval_return_component_routing_policy
        ));
    }
    if payload.batch_seed_partition_policy != expected.batch_seed_partition_policy {
        return Err(format!(
            "provenance.batch_seed_partition_policy must be {}, found {}",
            expected.batch_seed_partition_policy, payload.batch_seed_partition_policy
        ));
    }

    validate_required_field_list(
        "provenance.artifact_workflow_tags",
        payload.artifact_workflow_tags.as_slice(),
        expected.artifact_workflow_tags,
    )?;

    Ok(())
}

fn validate_path_diagnostics_payload(
    payload: &SufficientStatisticsRiskPathDiagnosticsPayload,
    expected: SufficientStatisticsRiskPathDiagnostics,
) -> Result<(), String> {
    if payload.schema_version != expected.schema_version {
        return Err(format!(
            "diagnostics.schema_version must be {}, found {}",
            expected.schema_version, payload.schema_version
        ));
    }
    if payload.simulated_interval_count != expected.simulated_interval_count {
        return Err(format!(
            "diagnostics.simulated_interval_count must be {}, found {}",
            expected.simulated_interval_count, payload.simulated_interval_count
        ));
    }
    if payload.first_interval_index != expected.first_interval_index {
        return Err(format!(
            "diagnostics.first_interval_index must be {:?}, found {:?}",
            expected.first_interval_index, payload.first_interval_index
        ));
    }
    if payload.last_interval_index != expected.last_interval_index {
        return Err(format!(
            "diagnostics.last_interval_index must be {:?}, found {:?}",
            expected.last_interval_index, payload.last_interval_index
        ));
    }
    if payload.cumulative_a_plus != expected.cumulative_a_plus {
        return Err(format!(
            "diagnostics.cumulative_a_plus must be {}, found {}",
            expected.cumulative_a_plus, payload.cumulative_a_plus
        ));
    }
    if payload.cumulative_a_minus != expected.cumulative_a_minus {
        return Err(format!(
            "diagnostics.cumulative_a_minus must be {}, found {}",
            expected.cumulative_a_minus, payload.cumulative_a_minus
        ));
    }
    if payload.cumulative_n_plus != expected.cumulative_n_plus {
        return Err(format!(
            "diagnostics.cumulative_n_plus must be {}, found {}",
            expected.cumulative_n_plus, payload.cumulative_n_plus
        ));
    }
    if payload.cumulative_n_minus != expected.cumulative_n_minus {
        return Err(format!(
            "diagnostics.cumulative_n_minus must be {}, found {}",
            expected.cumulative_n_minus, payload.cumulative_n_minus
        ));
    }
    if payload.cumulative_trading_time_return != expected.cumulative_trading_time_return {
        return Err(format!(
            "diagnostics.cumulative_trading_time_return must be {}, found {}",
            expected.cumulative_trading_time_return, payload.cumulative_trading_time_return
        ));
    }
    if payload.cumulative_open_jump_return != expected.cumulative_open_jump_return {
        return Err(format!(
            "diagnostics.cumulative_open_jump_return must be {}, found {}",
            expected.cumulative_open_jump_return, payload.cumulative_open_jump_return
        ));
    }
    if payload.cumulative_closed_gap_return != expected.cumulative_closed_gap_return {
        return Err(format!(
            "diagnostics.cumulative_closed_gap_return must be {}, found {}",
            expected.cumulative_closed_gap_return, payload.cumulative_closed_gap_return
        ));
    }
    if payload.cumulative_total_return != expected.cumulative_total_return {
        return Err(format!(
            "diagnostics.cumulative_total_return must be {}, found {}",
            expected.cumulative_total_return, payload.cumulative_total_return
        ));
    }
    if payload.max_abs_total_return != expected.max_abs_total_return {
        return Err(format!(
            "diagnostics.max_abs_total_return must be {}, found {}",
            expected.max_abs_total_return, payload.max_abs_total_return
        ));
    }

    Ok(())
}

fn validate_required_field_list(
    field_path: &str,
    provided: &[String],
    expected: &[&str],
) -> Result<(), String> {
    if provided.len() != expected.len() {
        return Err(format!(
            "{field_path} must contain {} entries, found {}",
            expected.len(),
            provided.len()
        ));
    }

    for (field_index, expected_field_name) in expected.iter().enumerate() {
        if provided[field_index] != *expected_field_name {
            return Err(format!(
                "{field_path}[{field_index}] must be {}, found {}",
                expected_field_name, provided[field_index]
            ));
        }
    }

    Ok(())
}

fn validate_finite_component(
    field: &'static str,
    value: f64,
) -> Result<(), SufficientStatisticsRiskReturnComponentsError> {
    if !value.is_finite() {
        return Err(
            SufficientStatisticsRiskReturnComponentsError::InvalidFiniteComponent { field, value },
        );
    }
    Ok(())
}

fn validate_excitation_state(state: HawkesExcitationState) -> Result<(), (&'static str, f64)> {
    validate_non_negative_finite_component("y_plus_plus", state.y_plus_plus)?;
    validate_non_negative_finite_component("y_plus_minus", state.y_plus_minus)?;
    validate_non_negative_finite_component("y_minus_plus", state.y_minus_plus)?;
    validate_non_negative_finite_component("y_minus_minus", state.y_minus_minus)?;

    Ok(())
}

fn validate_non_negative_finite_component(
    field: &'static str,
    value: f64,
) -> Result<(), (&'static str, f64)> {
    if !value.is_finite() || value < 0.0 {
        return Err((field, value));
    }
    Ok(())
}

/// Solve one-interval exponential-affine transform for sufficient-statistics risk sampling.
///
/// This adapter stays conversion-focused and reuses the same shared solver entrypoint as pricing.
pub fn solve_exponential_affine_transform_for_risk_sampling(
    transform_input: ExponentialAffineTransformInput,
) -> Result<ExponentialAffineTransformIntegrationResult, ExponentialAffineTransformSolverError> {
    solve_exponential_affine_transform_with_optional_settings(transform_input, None)
}

/// Solve one-interval exponential-affine transform for risk sampling with explicit settings.
pub fn solve_exponential_affine_transform_for_risk_sampling_with_settings(
    transform_input: ExponentialAffineTransformInput,
    solver_settings: ExponentialAffineTransformSolverSettings,
) -> Result<ExponentialAffineTransformIntegrationResult, ExponentialAffineTransformSolverError> {
    solve_exponential_affine_transform_with_optional_settings(
        transform_input,
        Some(solver_settings),
    )
}

#[cfg(test)]
mod tests {
    use crate::affine_oracle::{
        ExponentialAffineTransformEquationSpecification, ExponentialAffineTransformNewsIntensityInput,
    };
    use crate::calendar::MarketState;
    use crate::hawkes::{
        HawkesEventSide, HawkesExcitationState, HawkesKernelLoadings, HawkesKernelParameters,
        HawkesTransitionError, HawkesTransitionStep, evolve_hawkes_state,
    };
    use crate::news::GammaSeverityMarkConfiguration;
    use crate::nig::{ComplexScalar, NigSharedPlusShiftConfiguration};
    use crate::randomness::SplitMix64SimulationRandomnessStream;
    use crate::return_decomposition::{ReturnDecomposition, ReturnDecompositionInput};

    use super::{
        SUFFICIENT_STATISTICS_BATCH_SEED_PARTITION_POLICY,
        SUFFICIENT_STATISTICS_BOUNDARY_STATE_UPDATE_POLICY,
        SUFFICIENT_STATISTICS_INTERVAL_RETURN_COMPONENT_ROUTING_POLICY,
        SUFFICIENT_STATISTICS_RISK_INTERVAL_FACTOR_POLICY,
        SUFFICIENT_STATISTICS_RISK_PATH_DIAGNOSTICS_SCHEMA_VERSION,
        SUFFICIENT_STATISTICS_RISK_PATH_OUTPUT_SCHEMA_METADATA_VERSION,
        SUFFICIENT_STATISTICS_RISK_PATH_PROVENANCE_SCHEMA_VERSION,
        SUFFICIENT_STATISTICS_RISK_PATH_RECORD_SCHEMA_VERSION,
        SUFFICIENT_STATISTICS_RISK_PATH_SCHEMA_VERSION,
        SUFFICIENT_STATISTICS_RISK_RUN_REQUEST_SCHEMA_VERSION,
        SUFFICIENT_STATISTICS_STREAMING_CHUNKING_POLICY,
        SufficientStatisticsBoundaryStateUpdateError,
        SufficientStatisticsBoundaryStateUpdateSnapshot,
        SufficientStatisticsGeneratedIntervalReturn, SufficientStatisticsIntervalStatistics,
        SufficientStatisticsRiskIntervalFactors, SufficientStatisticsRiskIntervalFactorsError,
        SufficientStatisticsRiskModelInputs,
        SufficientStatisticsRiskPath, SufficientStatisticsRiskPathError,
        SufficientStatisticsRiskPathRecord, SufficientStatisticsRiskReturnComponents,
        SufficientStatisticsRiskReturnGenerationOptions, SufficientStatisticsRiskRunRequest,
        SufficientStatisticsRiskRunRequestError, SufficientStatisticsStreamingChunkingSettings,
        SufficientStatisticsTransformSliceContext,
        SufficientStatisticsTransformSliceExecutionError,
        SufficientStatisticsTransformSliceSamplingError,
        SufficientStatisticsTransformSliceSettings,
        SufficientStatisticsTransformSliceSettingsError, SufficientStatisticsTransformSlices,
        derive_sufficient_statistics_batch_path_seed,
        generate_interval_return_from_sampled_statistics,
        sample_interval_statistics_batch_from_transform_slices,
        sample_interval_statistics_from_transform_slices,
        sample_interval_statistics_multi_path_batch_from_transform_slices,
        solve_sufficient_statistics_transform_slices,
        stream_interval_statistics_from_transform_slices,
        stream_interval_statistics_multi_path_batch_from_transform_slices,
        sufficient_statistics_batch_seed_partition_policy,
        update_boundary_state_from_interval_statistics,
    };

    #[test]
    fn run_request_round_trip_serialization_preserves_contract_shape() {
        let run_request = build_valid_run_request();

        let serialized =
            serde_json::to_value(&run_request).expect("run request payload should serialize");
        assert_eq!(serialized["seed"], serde_json::json!(17));
        assert_eq!(serialized["horizon_interval_count"], serde_json::json!(2));
        assert_eq!(serialized["interval_delta"], serde_json::json!(1.0));
        assert_eq!(
            SufficientStatisticsRiskRunRequest::schema_version(),
            SUFFICIENT_STATISTICS_RISK_RUN_REQUEST_SCHEMA_VERSION
        );
        assert_eq!(
            SufficientStatisticsRiskIntervalFactors::policy(),
            SUFFICIENT_STATISTICS_RISK_INTERVAL_FACTOR_POLICY
        );

        let parsed: SufficientStatisticsRiskRunRequest =
            serde_json::from_value(serialized).expect("serialized payload should deserialize");
        assert_eq!(parsed, run_request);
    }

    #[test]
    fn interval_factors_component_news_intensity_round_trip_and_context_consistency() {
        let factors = SufficientStatisticsRiskIntervalFactors::try_new(
            vec![1.1, 0.9],
            vec![0.12, 0.03],
            vec![0.08, 0.07],
            Some(1.0),
        )
        .expect("component-aware interval factors should construct");
        assert_eq!(factors.nu_h.as_ref(), &[0.2, 0.1]);

        let first_interval_news_intensity = factors.news_intensity_input_at(0);
        assert_eq!(
            first_interval_news_intensity,
            ExponentialAffineTransformNewsIntensityInput::try_new(0.12, 0.08)
                .expect("expected news-intensity input should construct")
        );

        let serialized = serde_json::to_value(&factors).expect("interval factors should serialize");
        assert_eq!(serialized["nu_h_known"], serde_json::json!([0.12, 0.03]));
        assert_eq!(serialized["nu_h_surprise"], serde_json::json!([0.08, 0.07]));

        let parsed: SufficientStatisticsRiskIntervalFactors =
            serde_json::from_value(serialized).expect("serialized interval factors should parse");
        assert_eq!(parsed, factors);
    }

    #[test]
    fn interval_factors_reject_inconsistent_total_and_component_news_intensity() {
        let factors = SufficientStatisticsRiskIntervalFactors {
            interval_delta: Some(1.0),
            s_h: vec![1.1].into_boxed_slice(),
            nu_h: vec![0.3].into_boxed_slice(),
            nu_h_known: vec![0.1].into_boxed_slice(),
            nu_h_surprise: vec![0.1].into_boxed_slice(),
        };
        let error = factors
            .validate()
            .expect_err("inconsistent total and component intensities must fail validation");
        assert!(matches!(
            error,
            SufficientStatisticsRiskIntervalFactorsError::InconsistentNewsIntensityAggregation {
                interval_index: 0,
                ..
            }
        ));
    }

    #[test]
    fn path_round_trip_serialization_preserves_interval_statistics_and_return_components() {
        let record = SufficientStatisticsRiskPathRecord::try_new(
            0,
            SufficientStatisticsIntervalStatistics::try_new(0.7, 0.5, 3, 2)
                .expect("statistics should be valid"),
            SufficientStatisticsRiskReturnComponents::try_new(0.04, -0.03, 0.01, 0.0, -0.02, -0.01)
                .expect("return components should be valid"),
        )
        .expect("record should be valid");
        let path = SufficientStatisticsRiskPath::try_new(
            17,
            1.0,
            1,
            build_initial_state(),
            build_initial_state(),
            vec![record],
        )
        .expect("path should be valid");

        let serialized = serde_json::to_value(&path).expect("path payload should serialize");
        assert_eq!(
            serialized["schema_version"],
            serde_json::json!(SUFFICIENT_STATISTICS_RISK_PATH_SCHEMA_VERSION)
        );
        assert_eq!(
            serialized["records"][0]["schema_version"],
            serde_json::json!(SUFFICIENT_STATISTICS_RISK_PATH_RECORD_SCHEMA_VERSION)
        );
        assert_eq!(
            serialized["records"][0]["sufficient_statistics"]["n_plus"],
            serde_json::json!(3)
        );
        assert_eq!(
            serialized["records"][0]["return_components"]["total_return"],
            serde_json::json!(-0.01)
        );
        assert_eq!(
            serialized["output_schema_metadata"]["schema_version"],
            serde_json::json!(SUFFICIENT_STATISTICS_RISK_PATH_OUTPUT_SCHEMA_METADATA_VERSION)
        );
        assert_eq!(
            serialized["provenance"]["schema_version"],
            serde_json::json!(SUFFICIENT_STATISTICS_RISK_PATH_PROVENANCE_SCHEMA_VERSION)
        );
        assert_eq!(
            serialized["diagnostics"]["schema_version"],
            serde_json::json!(SUFFICIENT_STATISTICS_RISK_PATH_DIAGNOSTICS_SCHEMA_VERSION)
        );
        assert_eq!(
            serialized["diagnostics"]["cumulative_n_plus"],
            serde_json::json!(3)
        );

        let parsed: SufficientStatisticsRiskPath =
            serde_json::from_value(serialized).expect("serialized path should deserialize");
        assert_eq!(parsed, path);
    }

    #[test]
    fn risk_path_diagnostics_schema_metadata_exposes_required_fields() {
        let record = SufficientStatisticsRiskPathRecord::try_new(
            0,
            SufficientStatisticsIntervalStatistics::try_new(0.9, 0.4, 2, 1)
                .expect("statistics should be valid"),
            SufficientStatisticsRiskReturnComponents::try_new(0.03, -0.02, 0.01, 0.0, -0.01, 0.0)
                .expect("return components should be valid"),
        )
        .expect("record should be valid");
        let path = SufficientStatisticsRiskPath::try_new(
            19,
            1.0,
            1,
            build_initial_state(),
            build_initial_state(),
            vec![record],
        )
        .expect("path should be valid");

        let serialized = serde_json::to_value(path).expect("path payload should serialize");
        assert_eq!(
            serialized["output_schema_metadata"]["required_path_fields"],
            serde_json::json!([
                "schema_version",
                "run_request_schema_version",
                "output_schema_metadata",
                "provenance",
                "diagnostics",
                "seed",
                "interval_delta",
                "simulated_interval_count",
                "initial_excitation_state",
                "terminal_excitation_state",
                "records"
            ])
        );
        assert_eq!(
            serialized["output_schema_metadata"]["required_diagnostics_fields"],
            serde_json::json!([
                "schema_version",
                "simulated_interval_count",
                "first_interval_index",
                "last_interval_index",
                "cumulative_a_plus",
                "cumulative_a_minus",
                "cumulative_n_plus",
                "cumulative_n_minus",
                "cumulative_trading_time_return",
                "cumulative_open_jump_return",
                "cumulative_closed_gap_return",
                "cumulative_total_return",
                "max_abs_total_return"
            ])
        );
    }

    #[test]
    fn risk_path_deserialization_rejects_incorrect_diagnostics_with_field_context() {
        let record = SufficientStatisticsRiskPathRecord::try_new(
            0,
            SufficientStatisticsIntervalStatistics::try_new(0.7, 0.5, 3, 2)
                .expect("statistics should be valid"),
            SufficientStatisticsRiskReturnComponents::try_new(0.04, -0.03, 0.01, 0.0, -0.02, -0.01)
                .expect("return components should be valid"),
        )
        .expect("record should be valid");
        let path = SufficientStatisticsRiskPath::try_new(
            17,
            1.0,
            1,
            build_initial_state(),
            build_initial_state(),
            vec![record],
        )
        .expect("path should be valid");

        let mut serialized = serde_json::to_value(path).expect("path payload should serialize");
        serialized["diagnostics"]["cumulative_n_plus"] = serde_json::json!(5);

        let error = serde_json::from_value::<SufficientStatisticsRiskPath>(serialized)
            .expect_err("mismatched diagnostics should fail deserialization");
        assert!(
            error
                .to_string()
                .contains("diagnostics.cumulative_n_plus must be 3, found 5")
        );
    }

    #[test]
    fn risk_path_metadata_values_are_deterministic_across_repeated_runs() {
        let record = SufficientStatisticsRiskPathRecord::try_new(
            0,
            SufficientStatisticsIntervalStatistics::try_new(0.7, 0.5, 3, 2)
                .expect("statistics should be valid"),
            SufficientStatisticsRiskReturnComponents::try_new(0.04, -0.03, 0.01, 0.0, -0.02, -0.01)
                .expect("return components should be valid"),
        )
        .expect("record should be valid");
        let first_path = SufficientStatisticsRiskPath::try_new(
            17,
            1.0,
            1,
            build_initial_state(),
            build_initial_state(),
            vec![record.clone()],
        )
        .expect("first path should be valid");
        let second_path = SufficientStatisticsRiskPath::try_new(
            17,
            1.0,
            1,
            build_initial_state(),
            build_initial_state(),
            vec![record],
        )
        .expect("second path should be valid");

        assert_eq!(
            first_path.output_schema_metadata,
            second_path.output_schema_metadata
        );
        assert_eq!(first_path.provenance, second_path.provenance);
        assert_eq!(first_path.diagnostics, second_path.diagnostics);
    }

    #[test]
    fn run_request_validation_rejects_invalid_values_with_structured_errors() {
        let invalid_horizon = SufficientStatisticsRiskRunRequest::try_new(
            7,
            0,
            1.0,
            build_model_inputs(),
            build_interval_factors(),
        )
        .expect_err("zero horizon must fail validation");
        assert_eq!(
            invalid_horizon,
            SufficientStatisticsRiskRunRequestError::InvalidHorizonIntervalCount { value: 0 }
        );

        let invalid_interval_delta = SufficientStatisticsRiskRunRequest::try_new(
            7,
            1,
            0.0,
            build_model_inputs(),
            build_interval_factors(),
        )
        .expect_err("non-positive interval delta must fail validation");
        assert_eq!(
            invalid_interval_delta,
            SufficientStatisticsRiskRunRequestError::InvalidIntervalDelta { value: 0.0 }
        );

        let unsupported_non_hourly = SufficientStatisticsRiskRunRequest::try_new(
            7,
            1,
            0.5,
            build_model_inputs(),
            build_interval_factors(),
        )
        .expect_err("non-hourly interval delta without compatible factor metadata must fail");
        assert_eq!(
            unsupported_non_hourly,
            SufficientStatisticsRiskRunRequestError::IntervalDeltaFactorMismatch {
                requested_interval_delta: 0.5,
                available_factor_interval_delta: 1.0,
            }
        );

        let supported_sub_hour = SufficientStatisticsRiskRunRequest::try_new(
            7,
            1,
            0.5,
            build_model_inputs(),
            build_interval_factors_with_interval_delta(0.5),
        )
        .expect("non-hourly interval delta with compatible factor metadata should pass");
        assert_eq!(supported_sub_hour.interval_delta, 0.5);
    }

    #[test]
    fn deserialization_rejects_incomplete_or_malformed_request_payloads() {
        let missing_model_inputs_payload = serde_json::json!({
            "seed": 17,
            "horizon_interval_count": 2,
            "interval_delta": 1.0,
            "interval_factors": {
                "s_h": [1.1, 0.9],
                "nu_h_known": [0.12, 0.03],
                "nu_h_surprise": [0.08, 0.07]
            }
        });
        let missing_model_inputs_error =
            serde_json::from_value::<SufficientStatisticsRiskRunRequest>(
                missing_model_inputs_payload,
            )
            .expect_err("missing model_inputs must fail deserialization");

        assert!(
            missing_model_inputs_error
                .to_string()
                .contains("missing field `model_inputs`")
        );

        let malformed_factors_payload = serde_json::json!({
            "seed": 17,
            "horizon_interval_count": 2,
            "interval_delta": 1.0,
            "model_inputs": {
                "equation_specification": {
                    "hawkes_kernel_parameters": {
                        "lambda_bar": 0.8,
                        "beta": 1.6,
                        "alpha": {
                            "plus_plus": 0.2,
                            "plus_minus": 0.1,
                            "minus_plus": 0.1,
                            "minus_minus": 0.25
                        },
                        "rho": {
                            "plus_plus": 0.08,
                            "plus_minus": 0.04,
                            "minus_plus": 0.05,
                            "minus_minus": 0.1
                        }
                    },
                    "gamma_severity_mark_configuration": {
                        "shape_k": 1.8,
                        "scale_theta": 0.7
                    }
                },
                "nig_shared_plus_shift_configuration": {
                    "alpha": 2.3,
                    "beta_0": -0.8,
                    "delta": 0.7,
                    "mu": 0.1,
                    "delta_beta": -0.2
                },
                "initial_excitation_state": {
                    "y_plus_plus": 0.2,
                    "y_plus_minus": 0.1,
                    "y_minus_plus": 0.3,
                    "y_minus_minus": 0.4
                }
            },
            "interval_factors": {
                "s_h": [1.1, 0.9],
                "nu_h_known": [0.2],
                "nu_h_surprise": [0.0, 0.1]
            }
        });
        let malformed_factors_error =
            serde_json::from_value::<SufficientStatisticsRiskRunRequest>(malformed_factors_payload)
                .expect_err("mismatched interval_factors lengths must fail deserialization");

        assert!(
            malformed_factors_error
                .to_string()
                .contains("interval_factors.nu_h_known length must match s_h length")
        );
    }

    #[test]
    fn run_request_deserialization_defaults_interval_delta_to_hourly() {
        let payload_without_interval_delta = serde_json::json!({
            "seed": 17,
            "horizon_interval_count": 2,
            "model_inputs": {
                "equation_specification": {
                    "hawkes_kernel_parameters": {
                        "lambda_bar": 0.8,
                        "beta": 1.6,
                        "alpha": {
                            "plus_plus": 0.2,
                            "plus_minus": 0.1,
                            "minus_plus": 0.1,
                            "minus_minus": 0.25
                        },
                        "rho": {
                            "plus_plus": 0.08,
                            "plus_minus": 0.04,
                            "minus_plus": 0.05,
                            "minus_minus": 0.1
                        }
                    },
                    "gamma_severity_mark_configuration": {
                        "shape_k": 1.8,
                        "scale_theta": 0.7
                    }
                },
                "nig_shared_plus_shift_configuration": {
                    "alpha": 2.3,
                    "beta_0": -0.8,
                    "delta": 0.7,
                    "mu": 0.1,
                    "delta_beta": -0.2
                },
                "initial_excitation_state": {
                    "y_plus_plus": 0.2,
                    "y_plus_minus": 0.1,
                    "y_minus_plus": 0.3,
                    "y_minus_minus": 0.4
                }
            },
            "interval_factors": {
                "s_h": [1.1, 0.9],
                "nu_h_known": [0.12, 0.03],
                "nu_h_surprise": [0.08, 0.07]
            }
        });

        let parsed_request: SufficientStatisticsRiskRunRequest =
            serde_json::from_value(payload_without_interval_delta)
                .expect("payload without interval_delta should deserialize");
        assert_eq!(
            parsed_request.interval_delta,
            SufficientStatisticsRiskRunRequest::default_interval_delta()
        );
    }

    #[test]
    fn run_request_deserialization_accepts_component_only_news_intensity_factors() {
        let payload = serde_json::json!({
            "seed": 17,
            "horizon_interval_count": 2,
            "model_inputs": {
                "equation_specification": {
                    "hawkes_kernel_parameters": {
                        "lambda_bar": 0.8,
                        "beta": 1.6,
                        "alpha": {
                            "plus_plus": 0.2,
                            "plus_minus": 0.1,
                            "minus_plus": 0.1,
                            "minus_minus": 0.25
                        },
                        "rho": {
                            "plus_plus": 0.08,
                            "plus_minus": 0.04,
                            "minus_plus": 0.05,
                            "minus_minus": 0.1
                        }
                    },
                    "gamma_severity_mark_configuration": {
                        "shape_k": 1.8,
                        "scale_theta": 0.7
                    }
                },
                "nig_shared_plus_shift_configuration": {
                    "alpha": 2.3,
                    "beta_0": -0.8,
                    "delta": 0.7,
                    "mu": 0.1,
                    "delta_beta": -0.2
                },
                "initial_excitation_state": {
                    "y_plus_plus": 0.2,
                    "y_plus_minus": 0.1,
                    "y_minus_plus": 0.3,
                    "y_minus_minus": 0.4
                }
            },
            "interval_factors": {
                "s_h": [1.1, 0.9],
                "nu_h_known": [0.12, 0.03],
                "nu_h_surprise": [0.08, 0.07]
            }
        });

        let parsed_request: SufficientStatisticsRiskRunRequest =
            serde_json::from_value(payload).expect("component-only news-intensity payload should parse");
        assert_eq!(parsed_request.interval_factors.nu_h.as_ref(), &[0.2, 0.1]);
        assert_eq!(
            parsed_request.interval_factors.news_intensity_input_at(1),
            ExponentialAffineTransformNewsIntensityInput::try_new(0.03, 0.07)
            .expect("expected news-intensity input should construct")
        );
    }

    #[test]
    fn path_validation_rejects_non_sequential_interval_indices() {
        let first_record = SufficientStatisticsRiskPathRecord::try_new(
            1,
            SufficientStatisticsIntervalStatistics::try_new(0.7, 0.5, 3, 2)
                .expect("statistics should be valid"),
            SufficientStatisticsRiskReturnComponents::try_new(0.04, -0.03, 0.01, 0.0, -0.02, -0.01)
                .expect("return components should be valid"),
        )
        .expect("record should be valid");

        let error = SufficientStatisticsRiskPath::try_new(
            17,
            1.0,
            1,
            build_initial_state(),
            build_initial_state(),
            vec![first_record],
        )
        .expect_err("non-sequential interval index must fail validation");
        assert_eq!(
            error,
            SufficientStatisticsRiskPathError::NonSequentialIntervalIndex {
                record_position: 0,
                record_interval_index: 1,
            }
        );
    }

    #[test]
    fn interval_return_generator_open_window_routes_open_jump_and_suppresses_closed_gap() {
        let sampled_statistics = SufficientStatisticsIntervalStatistics::try_new(0.8, 0.6, 2, 1)
            .expect("sampled statistics should be valid");
        let driver_parameters = valid_nig_driver_parameters();
        let mut observed_stream = SplitMix64SimulationRandomnessStream::from_seed(44_001);
        let mut expected_stream = SplitMix64SimulationRandomnessStream::from_seed(44_001);
        let open_jump_return = -0.27;
        let closed_gap_return = 0.19;

        let generated_return = generate_interval_return_from_sampled_statistics(
            driver_parameters,
            sampled_statistics,
            MarketState::Open,
            open_jump_return,
            closed_gap_return,
            &mut observed_stream,
            Some(SufficientStatisticsRiskReturnGenerationOptions {
                include_component_breakdown: true,
            }),
        )
        .expect("interval return generation should succeed for open-window path");
        let expected_decomposition = ReturnDecomposition::try_new(ReturnDecompositionInput {
            a_plus: sampled_statistics.a_plus,
            a_minus: sampled_statistics.a_minus,
            open_jump: open_jump_return,
            closed_gap: 0.0,
        })
        .expect("expected decomposition should construct");
        let expected_trading_contributions = expected_decomposition
            .sample_nig_trading_time_increment(driver_parameters, &mut expected_stream)
            .expect("reference trading-time sampling should succeed");

        let component_breakdown = generated_return
            .component_breakdown
            .expect("component breakdown should be present");
        assert_eq!(
            SufficientStatisticsGeneratedIntervalReturn::component_routing_policy(),
            SUFFICIENT_STATISTICS_INTERVAL_RETURN_COMPONENT_ROUTING_POLICY
        );
        assert_float_close(
            component_breakdown.buy_side_return,
            expected_trading_contributions.buy_side_increment,
        );
        assert_float_close(
            component_breakdown.sell_side_return,
            -expected_trading_contributions.sell_side_increment,
        );
        assert_float_close(
            component_breakdown.trading_time_return,
            expected_trading_contributions.combined_increment,
        );
        assert_float_close(component_breakdown.open_jump_return, open_jump_return);
        assert_float_close(component_breakdown.closed_gap_return, 0.0);
        assert_float_close(
            component_breakdown.total_return,
            expected_trading_contributions.combined_increment + open_jump_return,
        );
        assert_float_close(
            generated_return.total_return,
            expected_trading_contributions.combined_increment + open_jump_return,
        );
    }

    #[test]
    fn interval_return_generator_closed_window_routes_closed_gap_and_supports_optional_breakdown() {
        let sampled_statistics = SufficientStatisticsIntervalStatistics::try_new(0.9, 0.7, 3, 2)
            .expect("sampled statistics should be valid");
        let driver_parameters = valid_nig_driver_parameters();
        let mut stream_with_breakdown = SplitMix64SimulationRandomnessStream::from_seed(44_002);
        let mut stream_without_breakdown = SplitMix64SimulationRandomnessStream::from_seed(44_002);
        let open_jump_return = -0.11;
        let closed_gap_return = 0.19;

        let with_breakdown = generate_interval_return_from_sampled_statistics(
            driver_parameters,
            sampled_statistics,
            MarketState::Closed,
            open_jump_return,
            closed_gap_return,
            &mut stream_with_breakdown,
            Some(SufficientStatisticsRiskReturnGenerationOptions {
                include_component_breakdown: true,
            }),
        )
        .expect("closed-window return generation should succeed with breakdown");
        let without_breakdown = generate_interval_return_from_sampled_statistics(
            driver_parameters,
            sampled_statistics,
            MarketState::Closed,
            open_jump_return,
            closed_gap_return,
            &mut stream_without_breakdown,
            Some(SufficientStatisticsRiskReturnGenerationOptions {
                include_component_breakdown: false,
            }),
        )
        .expect("closed-window return generation should succeed without breakdown");

        let component_breakdown = with_breakdown
            .component_breakdown
            .expect("component breakdown should be present");
        assert_float_close(
            component_breakdown.total_return,
            component_breakdown.trading_time_return + closed_gap_return,
        );
        assert_float_close(component_breakdown.open_jump_return, 0.0);
        assert_float_close(component_breakdown.closed_gap_return, closed_gap_return);
        assert!(without_breakdown.component_breakdown.is_none());
        assert_float_close(without_breakdown.total_return, with_breakdown.total_return);
    }

    #[test]
    fn interval_return_generator_is_seed_deterministic_across_repeated_runs() {
        let sampled_statistics = SufficientStatisticsIntervalStatistics::try_new(1.0, 0.4, 1, 2)
            .expect("sampled statistics should be valid");
        let driver_parameters = valid_nig_driver_parameters();
        let options = Some(SufficientStatisticsRiskReturnGenerationOptions {
            include_component_breakdown: true,
        });
        let mut first_stream = SplitMix64SimulationRandomnessStream::from_seed(44_003);
        let mut second_stream = SplitMix64SimulationRandomnessStream::from_seed(44_003);

        let first_generated_return = generate_interval_return_from_sampled_statistics(
            driver_parameters,
            sampled_statistics,
            MarketState::Closed,
            0.0,
            -0.05,
            &mut first_stream,
            options,
        )
        .expect("first seeded interval return generation should succeed");
        let second_generated_return = generate_interval_return_from_sampled_statistics(
            driver_parameters,
            sampled_statistics,
            MarketState::Closed,
            0.0,
            -0.05,
            &mut second_stream,
            options,
        )
        .expect("second seeded interval return generation should succeed");

        assert_eq!(first_generated_return, second_generated_return);
    }

    #[test]
    fn boundary_state_update_is_consistent_with_shared_transition_api_reference_sequence() {
        let initial_state = build_initial_state();
        let kernel_parameters = build_equation_specification().hawkes_kernel_parameters;
        let sampled_statistics = SufficientStatisticsIntervalStatistics::try_new(1.4, 1.1, 3, 2)
            .expect("statistics should be valid");
        let update_snapshot = SufficientStatisticsBoundaryStateUpdateSnapshot::new(
            3,
            0.75,
            initial_state,
            sampled_statistics,
        );

        assert_eq!(
            SufficientStatisticsBoundaryStateUpdateSnapshot::policy(),
            SUFFICIENT_STATISTICS_BOUNDARY_STATE_UPDATE_POLICY
        );

        let updated_state =
            update_boundary_state_from_interval_statistics(kernel_parameters, update_snapshot)
                .expect("boundary-state update should succeed");

        let mut reference_state = evolve_hawkes_state(
            initial_state,
            kernel_parameters,
            HawkesTransitionStep::new(0.75, None, None),
        )
        .expect("reference drift step should succeed");

        for _buy_event_position in 0..sampled_statistics.n_plus {
            reference_state = evolve_hawkes_state(
                reference_state,
                kernel_parameters,
                HawkesTransitionStep::new(0.0, Some(HawkesEventSide::Buy), None),
            )
            .expect("reference buy jump step should succeed");
        }

        for _sell_event_position in 0..sampled_statistics.n_minus {
            reference_state = evolve_hawkes_state(
                reference_state,
                kernel_parameters,
                HawkesTransitionStep::new(0.0, Some(HawkesEventSide::Sell), None),
            )
            .expect("reference sell jump step should succeed");
        }

        assert_float_close(updated_state.y_plus_plus, reference_state.y_plus_plus);
        assert_float_close(updated_state.y_plus_minus, reference_state.y_plus_minus);
        assert_float_close(updated_state.y_minus_plus, reference_state.y_minus_plus);
        assert_float_close(updated_state.y_minus_minus, reference_state.y_minus_minus);
    }

    #[test]
    fn boundary_state_update_maps_sampled_counts_to_canonical_components() {
        let initial_state = build_initial_state();
        let kernel_parameters = build_equation_specification().hawkes_kernel_parameters;
        let sampled_statistics = SufficientStatisticsIntervalStatistics::try_new(0.8, 0.6, 4, 5)
            .expect("statistics should be valid");
        let interval_delta = 0.5;
        let update_snapshot = SufficientStatisticsBoundaryStateUpdateSnapshot::new(
            1,
            interval_delta,
            initial_state,
            sampled_statistics,
        );

        let updated_state =
            update_boundary_state_from_interval_statistics(kernel_parameters, update_snapshot)
                .expect("boundary-state mapping update should succeed");

        let shared_decay_factor = (-kernel_parameters.beta * interval_delta).exp();
        let expected_y_plus_plus = initial_state.y_plus_plus * shared_decay_factor
            + kernel_parameters.alpha.plus_plus * sampled_statistics.n_plus as f64;
        let expected_y_plus_minus = initial_state.y_plus_minus * shared_decay_factor
            + kernel_parameters.alpha.plus_minus * sampled_statistics.n_minus as f64;
        let expected_y_minus_plus = initial_state.y_minus_plus * shared_decay_factor
            + kernel_parameters.alpha.minus_plus * sampled_statistics.n_plus as f64;
        let expected_y_minus_minus = initial_state.y_minus_minus * shared_decay_factor
            + kernel_parameters.alpha.minus_minus * sampled_statistics.n_minus as f64;

        assert_float_close(updated_state.y_plus_plus, expected_y_plus_plus);
        assert_float_close(updated_state.y_plus_minus, expected_y_plus_minus);
        assert_float_close(updated_state.y_minus_plus, expected_y_minus_plus);
        assert_float_close(updated_state.y_minus_minus, expected_y_minus_minus);
    }

    #[test]
    fn boundary_state_update_preserves_deterministic_multi_interval_carryover() {
        let kernel_parameters = build_equation_specification().hawkes_kernel_parameters;
        let interval_delta = 1.0;
        let sampled_statistics_path = [
            SufficientStatisticsIntervalStatistics::try_new(0.7, 0.5, 2, 1)
                .expect("first interval statistics should be valid"),
            SufficientStatisticsIntervalStatistics::try_new(0.4, 0.9, 1, 3)
                .expect("second interval statistics should be valid"),
            SufficientStatisticsIntervalStatistics::try_new(1.1, 0.2, 4, 0)
                .expect("third interval statistics should be valid"),
        ];

        let mut first_run_state = build_initial_state();
        let mut second_run_state = build_initial_state();
        let mut first_run_boundary_states: Vec<HawkesExcitationState> = Vec::new();
        let mut second_run_boundary_states: Vec<HawkesExcitationState> = Vec::new();

        for (interval_index, sampled_statistics) in
            sampled_statistics_path.iter().copied().enumerate()
        {
            let update_snapshot = SufficientStatisticsBoundaryStateUpdateSnapshot::new(
                interval_index,
                interval_delta,
                first_run_state,
                sampled_statistics,
            );
            first_run_state =
                update_boundary_state_from_interval_statistics(kernel_parameters, update_snapshot)
                    .expect("first run carryover update should succeed");
            first_run_boundary_states.push(first_run_state);
        }

        for (interval_index, sampled_statistics) in
            sampled_statistics_path.iter().copied().enumerate()
        {
            let update_snapshot = SufficientStatisticsBoundaryStateUpdateSnapshot::new(
                interval_index,
                interval_delta,
                second_run_state,
                sampled_statistics,
            );
            second_run_state =
                update_boundary_state_from_interval_statistics(kernel_parameters, update_snapshot)
                    .expect("second run carryover update should succeed");
            second_run_boundary_states.push(second_run_state);
        }

        assert_eq!(first_run_boundary_states, second_run_boundary_states);
        assert_eq!(first_run_state, second_run_state);
    }

    #[test]
    fn boundary_state_update_failures_include_interval_and_state_snapshot_metadata() {
        let kernel_parameters = build_equation_specification().hawkes_kernel_parameters;
        let sampled_statistics = SufficientStatisticsIntervalStatistics::try_new(0.7, 0.5, 2, 1)
            .expect("statistics should be valid");
        let update_snapshot = SufficientStatisticsBoundaryStateUpdateSnapshot::new(
            9,
            0.0,
            build_initial_state(),
            sampled_statistics,
        );

        let error =
            update_boundary_state_from_interval_statistics(kernel_parameters, update_snapshot)
                .expect_err("zero interval delta should fail boundary-state update");

        assert!(matches!(
            error,
            SufficientStatisticsBoundaryStateUpdateError::InvalidIntervalDelta {
                snapshot,
                value: 0.0
            }
            if snapshot.interval_index == 9
                && snapshot.state_before_update == update_snapshot.state_before_update
                && snapshot.sampled_statistics == update_snapshot.sampled_statistics
        ));
    }

    #[test]
    fn boundary_state_update_wraps_transition_failures_with_snapshot_metadata() {
        let invalid_kernel_parameters = HawkesKernelParameters {
            lambda_bar: 0.8,
            beta: 0.0,
            alpha: HawkesKernelLoadings {
                plus_plus: 0.2,
                plus_minus: 0.1,
                minus_plus: 0.1,
                minus_minus: 0.25,
            },
            rho: HawkesKernelLoadings {
                plus_plus: 0.08,
                plus_minus: 0.04,
                minus_plus: 0.05,
                minus_minus: 0.1,
            },
        };
        let update_snapshot = SufficientStatisticsBoundaryStateUpdateSnapshot::new(
            4,
            0.5,
            build_initial_state(),
            SufficientStatisticsIntervalStatistics::try_new(0.7, 0.5, 2, 1)
                .expect("statistics should be valid"),
        );

        let error = update_boundary_state_from_interval_statistics(
            invalid_kernel_parameters,
            update_snapshot,
        )
        .expect_err("invalid beta should fail transition composition");

        assert!(matches!(
            error,
            SufficientStatisticsBoundaryStateUpdateError::TransitionFailure {
                snapshot,
                source: HawkesTransitionError::InvalidBeta { value: 0.0 }
            }
            if snapshot.interval_index == 4
                && snapshot.state_before_update == update_snapshot.state_before_update
        ));
    }

    #[test]
    fn transform_slice_sampler_outputs_domain_sane_statistics() {
        let equation_specification = build_equation_specification();
        let context = build_transform_slice_context(0, 1.1, 0.2, 1.0);
        let settings = SufficientStatisticsTransformSliceSettings::default();

        let slices =
            solve_sufficient_statistics_transform_slices(equation_specification, context, settings)
                .expect("transform slices should solve");
        let sampled_statistics = sample_interval_statistics_from_transform_slices(17, slices)
            .expect("slice sampling should succeed");

        sampled_statistics
            .validate()
            .expect("sampled statistics should satisfy domain checks");

        let serialized =
            serde_json::to_value(sampled_statistics).expect("sampled statistics should serialize");

        assert!(serialized.get("a_plus").is_some());
        assert!(serialized.get("a_minus").is_some());
        assert!(serialized.get("n_plus").is_some());
        assert!(serialized.get("n_minus").is_some());
    }

    #[test]
    fn transform_slice_sampler_is_seed_deterministic_for_repeated_sampling() {
        let equation_specification = build_equation_specification();
        let context = build_transform_slice_context(1, 0.9, 0.1, 1.0);
        let settings = SufficientStatisticsTransformSliceSettings::default();
        let slices =
            solve_sufficient_statistics_transform_slices(equation_specification, context, settings)
                .expect("transform slices should solve");

        let first_sample = sample_interval_statistics_from_transform_slices(123, slices)
            .expect("first sampling run should succeed");
        let second_sample = sample_interval_statistics_from_transform_slices(123, slices)
            .expect("second sampling run should succeed");

        assert_eq!(first_sample, second_sample);
    }

    #[test]
    fn transform_slice_batch_sampler_is_seed_deterministic_for_repeated_sampling() {
        let equation_specification = build_equation_specification();
        let settings = SufficientStatisticsTransformSliceSettings::default();
        let first_interval_slices = solve_sufficient_statistics_transform_slices(
            equation_specification,
            build_transform_slice_context(0, 1.1, 0.2, 1.0),
            settings,
        )
        .expect("first interval slices should solve");
        let second_interval_slices = solve_sufficient_statistics_transform_slices(
            equation_specification,
            build_transform_slice_context(1, 0.8, 0.15, 1.0),
            settings,
        )
        .expect("second interval slices should solve");
        let batch = vec![first_interval_slices, second_interval_slices];

        let first_batch_sample = sample_interval_statistics_batch_from_transform_slices(77, &batch)
            .expect("first batch sampling run should succeed");
        let second_batch_sample =
            sample_interval_statistics_batch_from_transform_slices(77, &batch)
                .expect("second batch sampling run should succeed");

        assert_eq!(first_batch_sample, second_batch_sample);
    }

    #[test]
    fn transform_slice_streaming_sampler_matches_batch_sampler_with_ordered_complete_output() {
        let equation_specification = build_equation_specification();
        let settings = SufficientStatisticsTransformSliceSettings::default();

        let slices_batch = vec![
            solve_sufficient_statistics_transform_slices(
                equation_specification,
                build_transform_slice_context(0, 1.1, 0.2, 1.0),
                settings,
            )
            .expect("first interval slices should solve"),
            solve_sufficient_statistics_transform_slices(
                equation_specification,
                build_transform_slice_context(1, 0.8, 0.15, 1.0),
                settings,
            )
            .expect("second interval slices should solve"),
            solve_sufficient_statistics_transform_slices(
                equation_specification,
                build_transform_slice_context(2, 0.7, 0.1, 1.0),
                settings,
            )
            .expect("third interval slices should solve"),
        ];

        let chunking_settings = SufficientStatisticsStreamingChunkingSettings::try_new(2)
            .expect("chunking settings should be valid");

        assert_eq!(
            SufficientStatisticsStreamingChunkingSettings::policy(),
            SUFFICIENT_STATISTICS_STREAMING_CHUNKING_POLICY
        );

        let mut streamed_chunks = Vec::new();
        stream_interval_statistics_from_transform_slices(
            77,
            &slices_batch,
            chunking_settings,
            |chunk| streamed_chunks.push(chunk),
        )
        .expect("streaming sampling should succeed");

        assert_eq!(streamed_chunks.len(), 2);
        assert_eq!(streamed_chunks[0].chunk_index, 0);
        assert_eq!(streamed_chunks[0].start_interval_index, 0);
        assert_eq!(streamed_chunks[0].records.len(), 2);
        assert_eq!(streamed_chunks[1].chunk_index, 1);
        assert_eq!(streamed_chunks[1].start_interval_index, 2);
        assert_eq!(streamed_chunks[1].records.len(), 1);

        let streamed_statistics: Vec<SufficientStatisticsIntervalStatistics> = streamed_chunks
            .iter()
            .flat_map(|chunk| chunk.records.iter().copied())
            .collect();
        let batched_statistics =
            sample_interval_statistics_batch_from_transform_slices(77, &slices_batch)
                .expect("batch sampling should succeed");

        assert_eq!(streamed_statistics, batched_statistics);
    }

    #[test]
    fn transform_slice_multi_path_batch_sampler_uses_deterministic_seed_partitioning() {
        let equation_specification = build_equation_specification();
        let settings = SufficientStatisticsTransformSliceSettings::default();

        let first_path = vec![
            solve_sufficient_statistics_transform_slices(
                equation_specification,
                build_transform_slice_context(0, 1.1, 0.2, 1.0),
                settings,
            )
            .expect("first path first interval should solve"),
            solve_sufficient_statistics_transform_slices(
                equation_specification,
                build_transform_slice_context(1, 0.85, 0.13, 1.0),
                settings,
            )
            .expect("first path second interval should solve"),
        ];
        let second_path = vec![
            solve_sufficient_statistics_transform_slices(
                equation_specification,
                build_transform_slice_context(0, 0.9, 0.11, 1.0),
                settings,
            )
            .expect("second path first interval should solve"),
            solve_sufficient_statistics_transform_slices(
                equation_specification,
                build_transform_slice_context(1, 0.75, 0.09, 1.0),
                settings,
            )
            .expect("second path second interval should solve"),
        ];

        let path_batch = vec![first_path, second_path];
        let chunking_settings = SufficientStatisticsStreamingChunkingSettings::try_new(1)
            .expect("chunking settings should be valid");

        assert_eq!(
            sufficient_statistics_batch_seed_partition_policy(),
            SUFFICIENT_STATISTICS_BATCH_SEED_PARTITION_POLICY
        );

        let first_batch = sample_interval_statistics_multi_path_batch_from_transform_slices(
            1_901,
            &path_batch,
            chunking_settings,
        )
        .expect("first multi-path batch sampling run should succeed");
        let second_batch = sample_interval_statistics_multi_path_batch_from_transform_slices(
            1_901,
            &path_batch,
            chunking_settings,
        )
        .expect("second multi-path batch sampling run should succeed");

        assert_eq!(first_batch, second_batch);

        for (path_index, path_slices) in path_batch.iter().enumerate() {
            let path_seed = derive_sufficient_statistics_batch_path_seed(1_901, path_index as u64);
            let expected_path_statistics =
                sample_interval_statistics_batch_from_transform_slices(path_seed, path_slices)
                    .expect("single-path sampling with partitioned seed should succeed");
            assert_eq!(first_batch[path_index], expected_path_statistics);
        }

        let mut streamed_path_chunks = Vec::new();
        stream_interval_statistics_multi_path_batch_from_transform_slices(
            1_901,
            &path_batch,
            chunking_settings,
            |path_chunk| streamed_path_chunks.push(path_chunk),
        )
        .expect("multi-path streaming sampling should succeed");

        assert_eq!(streamed_path_chunks.len(), 4);
        assert_eq!(streamed_path_chunks[0].path_index, 0);
        assert_eq!(streamed_path_chunks[1].path_index, 0);
        assert_eq!(streamed_path_chunks[2].path_index, 1);
        assert_eq!(streamed_path_chunks[3].path_index, 1);
        assert_eq!(
            streamed_path_chunks[0].interval_chunk.start_interval_index,
            0
        );
        assert_eq!(
            streamed_path_chunks[1].interval_chunk.start_interval_index,
            1
        );
        assert_eq!(
            streamed_path_chunks[2].interval_chunk.start_interval_index,
            0
        );
        assert_eq!(
            streamed_path_chunks[3].interval_chunk.start_interval_index,
            1
        );
    }

    #[test]
    fn transform_slice_streaming_sampler_rejects_zero_chunk_size() {
        let equation_specification = build_equation_specification();
        let settings = SufficientStatisticsTransformSliceSettings::default();

        let slices_batch = vec![
            solve_sufficient_statistics_transform_slices(
                equation_specification,
                build_transform_slice_context(0, 1.1, 0.2, 1.0),
                settings,
            )
            .expect("interval slices should solve"),
        ];

        let invalid_chunking_settings = SufficientStatisticsStreamingChunkingSettings {
            max_intervals_per_chunk: 0,
        };

        let error = stream_interval_statistics_from_transform_slices(
            77,
            &slices_batch,
            invalid_chunking_settings,
            |_chunk| {},
        )
        .expect_err("zero chunk size must fail streaming execution");

        assert!(matches!(
            error,
            SufficientStatisticsTransformSliceExecutionError::InvalidChunkingSettings { .. }
        ));
    }

    #[test]
    fn transform_slice_sampler_rejects_non_real_transform_inputs_with_context() {
        let context = build_transform_slice_context(4, 1.2, 0.3, 1.0);
        let slices = SufficientStatisticsTransformSlices::try_new(
            context,
            SufficientStatisticsTransformSliceSettings::default(),
            ComplexScalar::new(1.0, 1.0e-3),
            ComplexScalar::new(0.99, 0.0),
            ComplexScalar::new(0.98, 0.0),
            ComplexScalar::new(0.97, 0.0),
            ComplexScalar::new(0.96, 0.0),
        )
        .expect("finite transform payload should construct");

        let error = sample_interval_statistics_from_transform_slices(8, slices)
            .expect_err("non-real transform slice should fail sampling");
        assert!(matches!(
            error,
            SufficientStatisticsTransformSliceSamplingError::NonRealTransformSlice {
                interval_index: 4,
                field: "baseline_transform",
                ..
            }
        ));
    }

    #[test]
    fn transform_slice_sampler_rejects_invalid_expected_statistics_from_invalid_slices() {
        let context = build_transform_slice_context(6, 1.2, 0.3, 1.0);
        let settings = SufficientStatisticsTransformSliceSettings::default();
        let invalid_slices = SufficientStatisticsTransformSlices::try_new(
            context,
            settings,
            ComplexScalar::new(1.0, 0.0),
            ComplexScalar::new(1.1, 0.0),
            ComplexScalar::new(0.95, 0.0),
            ComplexScalar::new(0.98, 0.0),
            ComplexScalar::new(0.97, 0.0),
        )
        .expect("finite transform payload should construct");

        let error = sample_interval_statistics_from_transform_slices(8, invalid_slices)
            .expect_err("invalid expected A-plus value should fail sampling");
        assert!(matches!(
            error,
            SufficientStatisticsTransformSliceSamplingError::InvalidExpectedStatistic {
                interval_index: 6,
                statistic: "a_plus",
                ..
            }
        ));
    }

    #[test]
    fn transform_slice_settings_rejects_invalid_probe_values() {
        let invalid_settings =
            SufficientStatisticsTransformSliceSettings::try_new(1.0e-3, 1.0e-3, 1.0, 0.99)
                .expect_err("n_plus_z equal to one must fail validation");

        assert_eq!(
            invalid_settings,
            SufficientStatisticsTransformSliceSettingsError::InvalidNPlusZ { value: 1.0 }
        );
    }

    fn build_valid_run_request() -> SufficientStatisticsRiskRunRequest {
        SufficientStatisticsRiskRunRequest::try_new(
            17,
            2,
            1.0,
            build_model_inputs(),
            build_interval_factors(),
        )
        .expect("run request should be valid")
    }

    fn build_transform_slice_context(
        interval_index: usize,
        s_h: f64,
        total_nu_h: f64,
        delta: f64,
    ) -> SufficientStatisticsTransformSliceContext {
        let known_nu_h = total_nu_h * 0.5;
        let surprise_nu_h = total_nu_h - known_nu_h;
        SufficientStatisticsTransformSliceContext::try_new(
            interval_index,
            s_h,
            ExponentialAffineTransformNewsIntensityInput::try_new(known_nu_h, surprise_nu_h)
                .expect("slice context news_intensity should be valid"),
            delta,
            build_initial_state(),
        )
        .expect("slice context should be valid")
    }

    fn build_equation_specification() -> ExponentialAffineTransformEquationSpecification {
        let kernel_parameters = HawkesKernelParameters::try_new(
            0.8,
            1.6,
            HawkesKernelLoadings {
                plus_plus: 0.2,
                plus_minus: 0.1,
                minus_plus: 0.1,
                minus_minus: 0.25,
            },
            HawkesKernelLoadings {
                plus_plus: 0.08,
                plus_minus: 0.04,
                minus_plus: 0.05,
                minus_minus: 0.1,
            },
        )
        .expect("kernel parameters should be valid");

        ExponentialAffineTransformEquationSpecification::try_new(
            kernel_parameters,
            GammaSeverityMarkConfiguration::try_new(1.8, 0.7).expect("gamma marks should be valid"),
        )
        .expect("equation specification should be valid")
    }

    fn build_model_inputs() -> SufficientStatisticsRiskModelInputs {
        SufficientStatisticsRiskModelInputs::try_new(
            build_equation_specification(),
            NigSharedPlusShiftConfiguration::try_new(2.3, -0.8, 0.7, 0.1, -0.2)
                .expect("nig configuration should be valid"),
            build_initial_state(),
        )
        .expect("model inputs should be valid")
    }

    fn valid_nig_driver_parameters() -> crate::nig::NigBuySellDriverParameters {
        build_model_inputs()
            .nig_shared_plus_shift_configuration
            .derive_driver_parameters()
            .expect("nig driver parameters should derive from shared-plus-shift configuration")
    }

    fn build_interval_factors() -> SufficientStatisticsRiskIntervalFactors {
        SufficientStatisticsRiskIntervalFactors::try_new(
            vec![1.1, 0.9],
            vec![0.12, 0.03],
            vec![0.08, 0.07],
            None,
        )
        .expect("interval factors should be valid")
    }

    fn build_interval_factors_with_interval_delta(
        interval_delta: f64,
    ) -> SufficientStatisticsRiskIntervalFactors {
        SufficientStatisticsRiskIntervalFactors::try_new(
            vec![1.1, 0.9],
            vec![0.12, 0.03],
            vec![0.08, 0.07],
            Some(interval_delta),
        )
        .expect("interval factors with explicit interval_delta should be valid")
    }

    fn build_initial_state() -> HawkesExcitationState {
        HawkesExcitationState {
            y_plus_plus: 0.2,
            y_plus_minus: 0.1,
            y_minus_plus: 0.3,
            y_minus_minus: 0.4,
        }
    }

    fn assert_float_close(left: f64, right: f64) {
        assert!(
            (left - right).abs() <= 1.0e-12,
            "float mismatch: left={left}, right={right}, abs_diff={}",
            (left - right).abs()
        );
    }
}
