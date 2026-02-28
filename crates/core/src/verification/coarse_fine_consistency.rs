//! Coarse/fine consistency regression harness and tolerance-policy contracts.

use std::collections::BTreeSet;
use std::error::Error;
use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};

use crate::affine_oracle::{
    ExponentialAffineTransformEquationSpecification, ExponentialAffineTransformIntegrationResult,
    ExponentialAffineTransformIntegrationStatus, ExponentialAffineTransformIntegratorMethod,
    ExponentialAffineTransformNewsIntensityInput, ExponentialAffineTransformOutput,
    ExponentialAffineTransformSolverSettings,
    solve_exponential_affine_transform_with_settings,
};
use crate::benchmarking::{
    BenchmarkScenarioCatalog, BenchmarkScenarioCatalogLoadError, BenchmarkScenarioWorkloadProfile,
    OraclePricerRiskBenchmarkPolicy, OraclePricerRiskBenchmarkPolicyValidationError,
    build_equation_specification, build_oracle_transform_input, build_pricing_engine_settings,
    build_pricing_market_inputs, derive_invocation_seed, fnv1a_64, interpolate_range, splitmix64,
    unit_interval_from_seed,
};
use crate::complex_math::complex_exponential;
use crate::hawkes::HawkesExcitationState;
use crate::nig::ComplexScalar;
use crate::pricing::{
    VanillaOptionPayoffConfiguration, VanillaOptionPayoffType,
    price_vanilla_option_via_fourier_inversion,
};
use crate::risk::{
    SufficientStatisticsTransformSliceContext, SufficientStatisticsTransformSliceSettings,
    SufficientStatisticsTransformSlices, sample_interval_statistics_batch_from_transform_slices,
    solve_sufficient_statistics_transform_slices_with_solver_settings,
};

pub const COARSE_FINE_CONSISTENCY_REGRESSION_REPORT_SCHEMA_VERSION: &str =
    "benchmark.coarse_fine_consistency.report.v1";
pub const COARSE_FINE_CONSISTENCY_REGRESSION_DETERMINISM_POLICY: &str =
    "canonical_profile_order_and_reproducibility_seed_partitioning.v1";
pub const COARSE_FINE_CONSISTENCY_REGRESSION_VARIANCE_CONTROL_POLICY: &str =
    "paired_path_seed_reuse_for_risk_statistics_and_fixed_seeded_fine_pricing_reference.v1";
pub const COARSE_FINE_CONSISTENCY_REGRESSION_METRIC_POLICY: &str =
    "absolute_and_relative_metric_delta_thresholds_with_inclusive_pass_boundaries.v1";

/// Note:
/// This harness covers engine-level coarse versus fine sanity checks for benchmark workflows.
/// Model-level multi-regime scale-consistency governance is owned by separate validation workflows.
pub const COARSE_FINE_CONSISTENCY_REGRESSION_GOVERNANCE_SCOPE_POLICY: &str = "engine_level_consistency_only_model_level_multi_regime_governance_delegated_to_scale_consistency_validation_workflows.v1";
pub const COARSE_FINE_CONSISTENCY_REGRESSION_OUTPUT_SCHEMA_METADATA_VERSION: &str =
    "benchmark.coarse_fine_consistency.output_schema_metadata.v1";
pub const COARSE_FINE_CONSISTENCY_REGRESSION_TOLERANCE_SCHEMA_VERSION: &str =
    "benchmark.coarse_fine_consistency.tolerance.v1";

const COARSE_FINE_CONSISTENCY_REQUIRED_REPORT_FIELDS: [&str; 14] = [
    "schema_version",
    "determinism_policy",
    "variance_control_policy",
    "metric_policy",
    "governance_scope_policy",
    "output_schema_metadata",
    "catalog_id",
    "catalog_version",
    "benchmark_policy_id",
    "benchmark_policy_version",
    "tolerance_config",
    "execution_config",
    "summary",
    "scenario_reports",
];
const COARSE_FINE_CONSISTENCY_REQUIRED_SCENARIO_REPORT_FIELDS: [&str; 8] = [
    "scenario_id",
    "applied_tolerance_band_label",
    "status",
    "failed_metric_count",
    "max_absolute_delta",
    "run_metadata",
    "summary",
    "metric_deltas",
];
const COARSE_FINE_CONSISTENCY_REQUIRED_METRIC_DELTA_FIELDS: [&str; 8] = [
    "metric_id",
    "coarse_value",
    "fine_reference_value",
    "absolute_delta",
    "relative_delta",
    "absolute_tolerance",
    "relative_tolerance",
    "passed",
];
const COARSE_FINE_CONSISTENCY_REQUIRED_RUN_METADATA_FIELDS: [&str; 12] = [
    "scenario_id",
    "profile_seed",
    "oracle_component_seed",
    "pricing_component_seed",
    "risk_component_seed",
    "coarse_interval_delta",
    "fine_interval_delta",
    "horizon_interval_count",
    "fine_subinterval_count_per_coarse_interval",
    "paired_risk_path_count",
    "fine_pricing_reference_path_count",
    "fine_pricing_reference_step_count",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoarseFineConsistencyRegressionExecutionConfig {
    pub reproducibility_seed: u64,
    pub fine_subinterval_count_per_coarse_interval: usize,
    pub paired_risk_path_count: usize,
    pub fine_pricing_reference_path_count: usize,
    pub fine_pricing_reference_step_count: usize,
}

impl Default for CoarseFineConsistencyRegressionExecutionConfig {
    fn default() -> Self {
        Self {
            reproducibility_seed: 0,
            fine_subinterval_count_per_coarse_interval: 4,
            paired_risk_path_count: 128,
            fine_pricing_reference_path_count: 2_048,
            fine_pricing_reference_step_count: 16,
        }
    }
}

impl CoarseFineConsistencyRegressionExecutionConfig {
    pub fn validate(self) -> Result<(), CoarseFineConsistencyRegressionExecutionConfigError> {
        if self.fine_subinterval_count_per_coarse_interval == 0 {
            return Err(
                CoarseFineConsistencyRegressionExecutionConfigError::InvalidFineSubintervalCount {
                    value: self.fine_subinterval_count_per_coarse_interval,
                },
            );
        }
        if self.paired_risk_path_count == 0 {
            return Err(
                CoarseFineConsistencyRegressionExecutionConfigError::InvalidRiskPathCount {
                    value: self.paired_risk_path_count,
                },
            );
        }
        if self.fine_pricing_reference_path_count == 0 {
            return Err(
                CoarseFineConsistencyRegressionExecutionConfigError::InvalidFinePricingPathCount {
                    value: self.fine_pricing_reference_path_count,
                },
            );
        }
        if self.fine_pricing_reference_step_count == 0 {
            return Err(
                CoarseFineConsistencyRegressionExecutionConfigError::InvalidFinePricingStepCount {
                    value: self.fine_pricing_reference_step_count,
                },
            );
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CoarseFineConsistencyRegressionExecutionConfigError {
    InvalidFineSubintervalCount { value: usize },
    InvalidRiskPathCount { value: usize },
    InvalidFinePricingPathCount { value: usize },
    InvalidFinePricingStepCount { value: usize },
}

impl Display for CoarseFineConsistencyRegressionExecutionConfigError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidFineSubintervalCount { value } => write!(
                formatter,
                "execution_config.fine_subinterval_count_per_coarse_interval must be greater than 0, found {value}"
            ),
            Self::InvalidRiskPathCount { value } => write!(
                formatter,
                "execution_config.paired_risk_path_count must be greater than 0, found {value}"
            ),
            Self::InvalidFinePricingPathCount { value } => write!(
                formatter,
                "execution_config.fine_pricing_reference_path_count must be greater than 0, found {value}"
            ),
            Self::InvalidFinePricingStepCount { value } => write!(
                formatter,
                "execution_config.fine_pricing_reference_step_count must be greater than 0, found {value}"
            ),
        }
    }
}

impl Error for CoarseFineConsistencyRegressionExecutionConfigError {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoarseFineConsistencyRegressionTolerancePolicyChangeLogEntry {
    pub policy_version: String,
    pub recorded_at_utc: String,
    pub rationale: String,
}

impl CoarseFineConsistencyRegressionTolerancePolicyChangeLogEntry {
    pub fn validate(
        &self,
    ) -> Result<(), CoarseFineConsistencyRegressionTolerancePolicyChangeLogEntryError> {
        if self.policy_version.trim().is_empty() {
            return Err(
                CoarseFineConsistencyRegressionTolerancePolicyChangeLogEntryError::EmptyPolicyVersion,
            );
        }
        if self.recorded_at_utc.trim().is_empty() {
            return Err(
                CoarseFineConsistencyRegressionTolerancePolicyChangeLogEntryError::EmptyRecordedAtUtc,
            );
        }
        if self.rationale.trim().is_empty() {
            return Err(
                CoarseFineConsistencyRegressionTolerancePolicyChangeLogEntryError::EmptyRationale,
            );
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CoarseFineConsistencyRegressionTolerancePolicyChangeLogEntryError {
    EmptyPolicyVersion,
    EmptyRecordedAtUtc,
    EmptyRationale,
}

impl Display for CoarseFineConsistencyRegressionTolerancePolicyChangeLogEntryError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyPolicyVersion => {
                write!(formatter, "policy_version must be a non-empty string")
            }
            Self::EmptyRecordedAtUtc => {
                write!(formatter, "recorded_at_utc must be a non-empty string")
            }
            Self::EmptyRationale => write!(formatter, "rationale must be a non-empty string"),
        }
    }
}

impl Error for CoarseFineConsistencyRegressionTolerancePolicyChangeLogEntryError {}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoarseFineConsistencyRegressionIntervalToleranceBand {
    pub interval_grid_label: String,
    pub delta_upper_bound: f64,
    pub oracle_c_at_delta_absolute_tolerance: f64,
    pub oracle_d_component_absolute_tolerance: f64,
    pub oracle_transform_value_absolute_tolerance: f64,
    pub pricing_price_absolute_delta_tolerance: f64,
    pub pricing_price_relative_delta_tolerance: f64,
    pub risk_integrated_intensity_mean_absolute_delta_tolerance: f64,
    pub risk_integrated_intensity_mean_relative_delta_tolerance: f64,
    pub risk_event_count_mean_absolute_delta_tolerance: f64,
    pub risk_event_count_mean_relative_delta_tolerance: f64,
}

impl CoarseFineConsistencyRegressionIntervalToleranceBand {
    pub fn validate(
        &self,
    ) -> Result<(), CoarseFineConsistencyRegressionIntervalToleranceBandError> {
        if self.interval_grid_label.trim().is_empty() {
            return Err(
                CoarseFineConsistencyRegressionIntervalToleranceBandError::EmptyIntervalGridLabel,
            );
        }
        if !self.delta_upper_bound.is_finite() || self.delta_upper_bound <= 0.0 {
            return Err(
                CoarseFineConsistencyRegressionIntervalToleranceBandError::InvalidDeltaUpperBound {
                    value: self.delta_upper_bound,
                },
            );
        }

        let tolerance_fields = [
            (
                "oracle_c_at_delta_absolute_tolerance",
                self.oracle_c_at_delta_absolute_tolerance,
            ),
            (
                "oracle_d_component_absolute_tolerance",
                self.oracle_d_component_absolute_tolerance,
            ),
            (
                "oracle_transform_value_absolute_tolerance",
                self.oracle_transform_value_absolute_tolerance,
            ),
            (
                "pricing_price_absolute_delta_tolerance",
                self.pricing_price_absolute_delta_tolerance,
            ),
            (
                "pricing_price_relative_delta_tolerance",
                self.pricing_price_relative_delta_tolerance,
            ),
            (
                "risk_integrated_intensity_mean_absolute_delta_tolerance",
                self.risk_integrated_intensity_mean_absolute_delta_tolerance,
            ),
            (
                "risk_integrated_intensity_mean_relative_delta_tolerance",
                self.risk_integrated_intensity_mean_relative_delta_tolerance,
            ),
            (
                "risk_event_count_mean_absolute_delta_tolerance",
                self.risk_event_count_mean_absolute_delta_tolerance,
            ),
            (
                "risk_event_count_mean_relative_delta_tolerance",
                self.risk_event_count_mean_relative_delta_tolerance,
            ),
        ];

        for (field_name, value) in tolerance_fields {
            if !value.is_finite() || value < 0.0 {
                return Err(
                    CoarseFineConsistencyRegressionIntervalToleranceBandError::InvalidToleranceValue {
                        field_name,
                        value,
                    },
                );
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CoarseFineConsistencyRegressionIntervalToleranceBandError {
    EmptyIntervalGridLabel,
    InvalidDeltaUpperBound {
        value: f64,
    },
    InvalidToleranceValue {
        field_name: &'static str,
        value: f64,
    },
}

impl Display for CoarseFineConsistencyRegressionIntervalToleranceBandError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyIntervalGridLabel => {
                write!(formatter, "interval_grid_label must be a non-empty string")
            }
            Self::InvalidDeltaUpperBound { value } => write!(
                formatter,
                "delta_upper_bound must be finite and strictly positive, found {value}"
            ),
            Self::InvalidToleranceValue { field_name, value } => write!(
                formatter,
                "{field_name} must be finite and non-negative, found {value}"
            ),
        }
    }
}

impl Error for CoarseFineConsistencyRegressionIntervalToleranceBandError {}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoarseFineConsistencyRegressionToleranceConfig {
    pub schema_version: String,
    pub policy_id: String,
    pub policy_version: String,
    pub governance_scope_policy: String,
    pub interval_tolerance_bands: Vec<CoarseFineConsistencyRegressionIntervalToleranceBand>,
    pub change_log: Vec<CoarseFineConsistencyRegressionTolerancePolicyChangeLogEntry>,
}

impl Default for CoarseFineConsistencyRegressionToleranceConfig {
    fn default() -> Self {
        Self {
            schema_version: COARSE_FINE_CONSISTENCY_REGRESSION_TOLERANCE_SCHEMA_VERSION.to_string(),
            policy_id: "coarse_fine_consistency_default_tolerance".to_string(),
            policy_version: "1.0.0".to_string(),
            governance_scope_policy:
                COARSE_FINE_CONSISTENCY_REGRESSION_GOVERNANCE_SCOPE_POLICY.to_string(),
            interval_tolerance_bands: vec![CoarseFineConsistencyRegressionIntervalToleranceBand {
                interval_grid_label: "hourly_grid".to_string(),
                delta_upper_bound: 1.0,
                oracle_c_at_delta_absolute_tolerance: 0.005,
                oracle_d_component_absolute_tolerance: 0.005,
                oracle_transform_value_absolute_tolerance: 0.005,
                pricing_price_absolute_delta_tolerance: 0.20,
                pricing_price_relative_delta_tolerance: 0.15,
                risk_integrated_intensity_mean_absolute_delta_tolerance: 0.65,
                risk_integrated_intensity_mean_relative_delta_tolerance: 0.65,
                risk_event_count_mean_absolute_delta_tolerance: 0.75,
                risk_event_count_mean_relative_delta_tolerance: 0.75,
            }],
            change_log: vec![CoarseFineConsistencyRegressionTolerancePolicyChangeLogEntry {
                policy_version: "1.0.0".to_string(),
                recorded_at_utc: "2026-02-27T00:00:00Z".to_string(),
                rationale: "Default engine-level benchmark and regression thresholds covering oracle outputs, pricing outputs, and risk summary metrics.".to_string(),
            }],
        }
    }
}

impl CoarseFineConsistencyRegressionToleranceConfig {
    pub fn validate(&self) -> Result<(), CoarseFineConsistencyRegressionToleranceConfigError> {
        if self.schema_version != COARSE_FINE_CONSISTENCY_REGRESSION_TOLERANCE_SCHEMA_VERSION {
            return Err(
                CoarseFineConsistencyRegressionToleranceConfigError::InvalidSchemaVersion {
                    expected: COARSE_FINE_CONSISTENCY_REGRESSION_TOLERANCE_SCHEMA_VERSION,
                    actual: self.schema_version.clone(),
                },
            );
        }
        if self.policy_id.trim().is_empty() {
            return Err(CoarseFineConsistencyRegressionToleranceConfigError::EmptyPolicyId);
        }
        if self.policy_version.trim().is_empty() {
            return Err(CoarseFineConsistencyRegressionToleranceConfigError::EmptyPolicyVersion);
        }
        if self.governance_scope_policy
            != COARSE_FINE_CONSISTENCY_REGRESSION_GOVERNANCE_SCOPE_POLICY
        {
            return Err(
                CoarseFineConsistencyRegressionToleranceConfigError::InvalidGovernanceScopePolicy {
                    expected: COARSE_FINE_CONSISTENCY_REGRESSION_GOVERNANCE_SCOPE_POLICY,
                    actual: self.governance_scope_policy.clone(),
                },
            );
        }
        if self.interval_tolerance_bands.is_empty() {
            return Err(
                CoarseFineConsistencyRegressionToleranceConfigError::EmptyIntervalToleranceBands,
            );
        }

        let mut previous_delta_upper_bound: Option<f64> = None;
        for (position, tolerance_band) in self.interval_tolerance_bands.iter().enumerate() {
            tolerance_band.validate().map_err(|source| {
                CoarseFineConsistencyRegressionToleranceConfigError::InvalidIntervalToleranceBand {
                    position,
                    source,
                }
            })?;

            if let Some(previous_delta_upper_bound) = previous_delta_upper_bound {
                if tolerance_band.delta_upper_bound <= previous_delta_upper_bound {
                    return Err(
                        CoarseFineConsistencyRegressionToleranceConfigError::NonIncreasingDeltaUpperBound {
                            previous: previous_delta_upper_bound,
                            current: tolerance_band.delta_upper_bound,
                        },
                    );
                }
            }
            previous_delta_upper_bound = Some(tolerance_band.delta_upper_bound);
        }

        if self.change_log.is_empty() {
            return Err(CoarseFineConsistencyRegressionToleranceConfigError::EmptyChangeLog);
        }

        let mut observed_policy_versions: BTreeSet<String> = BTreeSet::new();
        let mut has_current_policy_version = false;
        for (position, change_log_entry) in self.change_log.iter().enumerate() {
            change_log_entry.validate().map_err(|source| {
                CoarseFineConsistencyRegressionToleranceConfigError::InvalidChangeLogEntry {
                    position,
                    source,
                }
            })?;

            if !observed_policy_versions.insert(change_log_entry.policy_version.clone()) {
                return Err(
                    CoarseFineConsistencyRegressionToleranceConfigError::DuplicateChangeLogPolicyVersion {
                        policy_version: change_log_entry.policy_version.clone(),
                    },
                );
            }
            has_current_policy_version = has_current_policy_version
                || change_log_entry.policy_version == self.policy_version;
        }

        if !has_current_policy_version {
            return Err(
                CoarseFineConsistencyRegressionToleranceConfigError::MissingCurrentPolicyVersionInChangeLog {
                    policy_version: self.policy_version.clone(),
                },
            );
        }

        Ok(())
    }

    pub fn select_tolerance_band_for_interval_delta(
        &self,
        interval_delta: f64,
    ) -> Result<
        CoarseFineConsistencyRegressionIntervalToleranceBand,
        CoarseFineConsistencyRegressionToleranceConfigError,
    > {
        if !interval_delta.is_finite() || interval_delta <= 0.0 {
            return Err(
                CoarseFineConsistencyRegressionToleranceConfigError::InvalidIntervalDelta {
                    value: interval_delta,
                },
            );
        }

        for tolerance_band in &self.interval_tolerance_bands {
            if interval_delta <= tolerance_band.delta_upper_bound {
                return Ok(tolerance_band.clone());
            }
        }

        let maximum_supported_delta = self
            .interval_tolerance_bands
            .last()
            .map_or(0.0, |tolerance_band| tolerance_band.delta_upper_bound);

        Err(
            CoarseFineConsistencyRegressionToleranceConfigError::DeltaOutsideToleranceBands {
                interval_delta,
                maximum_supported_delta,
            },
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CoarseFineConsistencyRegressionToleranceConfigError {
    InvalidSchemaVersion {
        expected: &'static str,
        actual: String,
    },
    EmptyPolicyId,
    EmptyPolicyVersion,
    InvalidGovernanceScopePolicy {
        expected: &'static str,
        actual: String,
    },
    EmptyIntervalToleranceBands,
    InvalidIntervalToleranceBand {
        position: usize,
        source: CoarseFineConsistencyRegressionIntervalToleranceBandError,
    },
    NonIncreasingDeltaUpperBound {
        previous: f64,
        current: f64,
    },
    InvalidIntervalDelta {
        value: f64,
    },
    DeltaOutsideToleranceBands {
        interval_delta: f64,
        maximum_supported_delta: f64,
    },
    EmptyChangeLog,
    InvalidChangeLogEntry {
        position: usize,
        source: CoarseFineConsistencyRegressionTolerancePolicyChangeLogEntryError,
    },
    DuplicateChangeLogPolicyVersion {
        policy_version: String,
    },
    MissingCurrentPolicyVersionInChangeLog {
        policy_version: String,
    },
}

impl Display for CoarseFineConsistencyRegressionToleranceConfigError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidSchemaVersion { expected, actual } => write!(
                formatter,
                "tolerance_config.schema_version must be `{expected}`, found `{actual}`"
            ),
            Self::EmptyPolicyId => {
                write!(formatter, "tolerance_config.policy_id must be non-empty")
            }
            Self::EmptyPolicyVersion => {
                write!(
                    formatter,
                    "tolerance_config.policy_version must be non-empty"
                )
            }
            Self::InvalidGovernanceScopePolicy { expected, actual } => write!(
                formatter,
                "tolerance_config.governance_scope_policy must be `{expected}`, found `{actual}`"
            ),
            Self::EmptyIntervalToleranceBands => write!(
                formatter,
                "tolerance_config.interval_tolerance_bands must contain at least one tolerance band"
            ),
            Self::InvalidIntervalToleranceBand { position, source } => write!(
                formatter,
                "tolerance_config.interval_tolerance_bands[{position}] is invalid: {source}"
            ),
            Self::NonIncreasingDeltaUpperBound { previous, current } => write!(
                formatter,
                "tolerance_config interval_tolerance_bands delta_upper_bound values must be strictly increasing, found previous={previous}, current={current}"
            ),
            Self::InvalidIntervalDelta { value } => write!(
                formatter,
                "interval_delta must be finite and strictly positive, found {value}"
            ),
            Self::DeltaOutsideToleranceBands {
                interval_delta,
                maximum_supported_delta,
            } => write!(
                formatter,
                "interval_delta {interval_delta} is outside configured tolerance bands, maximum supported interval_delta is {maximum_supported_delta}"
            ),
            Self::EmptyChangeLog => {
                write!(
                    formatter,
                    "tolerance_config.change_log must include at least one policy history entry"
                )
            }
            Self::InvalidChangeLogEntry { position, source } => {
                write!(
                    formatter,
                    "tolerance_config.change_log[{position}] is invalid: {source}"
                )
            }
            Self::DuplicateChangeLogPolicyVersion { policy_version } => write!(
                formatter,
                "tolerance_config.change_log contains duplicate policy_version: {policy_version}"
            ),
            Self::MissingCurrentPolicyVersionInChangeLog { policy_version } => write!(
                formatter,
                "tolerance_config.change_log must include an entry for policy_version '{policy_version}'"
            ),
        }
    }
}

impl Error for CoarseFineConsistencyRegressionToleranceConfigError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidIntervalToleranceBand { source, .. } => Some(source),
            Self::InvalidChangeLogEntry { source, .. } => Some(source),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CoarseFineConsistencyRegressionToleranceConfigLoadError {
    InvalidJson {
        source: String,
    },
    InvalidToleranceConfig {
        source: CoarseFineConsistencyRegressionToleranceConfigError,
    },
}

impl Display for CoarseFineConsistencyRegressionToleranceConfigLoadError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidJson { source } => write!(
                formatter,
                "coarse fine consistency tolerance policy payload must be valid JSON: {source}"
            ),
            Self::InvalidToleranceConfig { source } => write!(
                formatter,
                "coarse fine consistency tolerance policy payload is invalid: {source}"
            ),
        }
    }
}

impl Error for CoarseFineConsistencyRegressionToleranceConfigLoadError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidToleranceConfig { source } => Some(source),
            _ => None,
        }
    }
}

pub fn load_coarse_fine_consistency_regression_tolerance_config_from_json(
    tolerance_policy_payload: &str,
) -> Result<
    CoarseFineConsistencyRegressionToleranceConfig,
    CoarseFineConsistencyRegressionToleranceConfigLoadError,
> {
    let tolerance_config: CoarseFineConsistencyRegressionToleranceConfig =
        serde_json::from_str(tolerance_policy_payload).map_err(|source| {
            CoarseFineConsistencyRegressionToleranceConfigLoadError::InvalidJson {
                source: source.to_string(),
            }
        })?;
    tolerance_config.validate().map_err(|source| {
        CoarseFineConsistencyRegressionToleranceConfigLoadError::InvalidToleranceConfig { source }
    })?;

    Ok(tolerance_config)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CoarseFineConsistencyRegressionOutputSchemaMetadata {
    pub schema_version: &'static str,
    pub tolerance_schema_version: &'static str,
    pub required_report_fields: Vec<&'static str>,
    pub required_scenario_report_fields: Vec<&'static str>,
    pub required_metric_delta_fields: Vec<&'static str>,
    pub required_run_metadata_fields: Vec<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CoarseFineConsistencyScenarioStatus {
    Pass,
    Fail,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CoarseFineConsistencyScenarioRunMetadata {
    pub scenario_id: String,
    pub profile_seed: u64,
    pub oracle_component_seed: u64,
    pub pricing_component_seed: u64,
    pub risk_component_seed: u64,
    pub coarse_interval_delta: f64,
    pub fine_interval_delta: f64,
    pub horizon_interval_count: usize,
    pub fine_subinterval_count_per_coarse_interval: usize,
    pub paired_risk_path_count: usize,
    pub fine_pricing_reference_path_count: usize,
    pub fine_pricing_reference_step_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CoarseFineConsistencyMetricDelta {
    pub metric_id: &'static str,
    pub coarse_value: f64,
    pub fine_reference_value: f64,
    pub absolute_delta: f64,
    pub relative_delta: f64,
    pub absolute_tolerance: f64,
    pub relative_tolerance: f64,
    pub passed: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CoarseFineConsistencyScenarioSummary {
    pub coarse_pricing_value: f64,
    pub fine_pricing_reference_value: f64,
    pub coarse_risk_mean_a_plus: f64,
    pub fine_risk_mean_a_plus: f64,
    pub coarse_risk_mean_a_minus: f64,
    pub fine_risk_mean_a_minus: f64,
    pub coarse_risk_mean_n_plus: f64,
    pub fine_risk_mean_n_plus: f64,
    pub coarse_risk_mean_n_minus: f64,
    pub fine_risk_mean_n_minus: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CoarseFineConsistencyScenarioReport {
    pub scenario_id: String,
    pub applied_tolerance_band_label: String,
    pub status: CoarseFineConsistencyScenarioStatus,
    pub failed_metric_count: usize,
    pub max_absolute_delta: f64,
    pub run_metadata: CoarseFineConsistencyScenarioRunMetadata,
    pub summary: CoarseFineConsistencyScenarioSummary,
    pub metric_deltas: Vec<CoarseFineConsistencyMetricDelta>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct CoarseFineConsistencyRegressionSummary {
    pub total_scenario_count: usize,
    pub passed_scenario_count: usize,
    pub failed_scenario_count: usize,
    pub total_metric_count: usize,
    pub failed_metric_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CoarseFineConsistencyRegressionReport {
    pub schema_version: &'static str,
    pub determinism_policy: &'static str,
    pub variance_control_policy: &'static str,
    pub metric_policy: &'static str,
    pub governance_scope_policy: &'static str,
    pub output_schema_metadata: CoarseFineConsistencyRegressionOutputSchemaMetadata,
    pub catalog_id: String,
    pub catalog_version: String,
    pub benchmark_policy_id: String,
    pub benchmark_policy_version: String,
    pub tolerance_config: CoarseFineConsistencyRegressionToleranceConfig,
    pub execution_config: CoarseFineConsistencyRegressionExecutionConfig,
    pub summary: CoarseFineConsistencyRegressionSummary,
    pub scenario_reports: Vec<CoarseFineConsistencyScenarioReport>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CoarseFineConsistencyRegressionHarnessError {
    InvalidScenarioCatalog {
        source: BenchmarkScenarioCatalogLoadError,
    },
    InvalidBenchmarkPolicy {
        source: OraclePricerRiskBenchmarkPolicyValidationError,
    },
    InvalidExecutionConfig {
        source: CoarseFineConsistencyRegressionExecutionConfigError,
    },
    InvalidToleranceConfig {
        source: CoarseFineConsistencyRegressionToleranceConfigError,
    },
    ScenarioExperimentFailure {
        scenario_id: String,
        detail: String,
    },
}

impl Display for CoarseFineConsistencyRegressionHarnessError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidScenarioCatalog { source } => write!(
                formatter,
                "coarse fine consistency harness requires a valid scenario catalog: {source}"
            ),
            Self::InvalidBenchmarkPolicy { source } => write!(
                formatter,
                "coarse fine consistency harness requires a valid benchmark policy: {source}"
            ),
            Self::InvalidExecutionConfig { source } => write!(
                formatter,
                "coarse fine consistency execution configuration is invalid: {source}"
            ),
            Self::InvalidToleranceConfig { source } => write!(
                formatter,
                "coarse fine consistency tolerance configuration is invalid: {source}"
            ),
            Self::ScenarioExperimentFailure {
                scenario_id,
                detail,
            } => write!(
                formatter,
                "scenario `{scenario_id}` coarse fine experiment failed: {detail}"
            ),
        }
    }
}

impl Error for CoarseFineConsistencyRegressionHarnessError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidScenarioCatalog { source } => Some(source),
            Self::InvalidBenchmarkPolicy { source } => Some(source),
            Self::InvalidExecutionConfig { source } => Some(source),
            Self::InvalidToleranceConfig { source } => Some(source),
            Self::ScenarioExperimentFailure { .. } => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct CoarseFineRiskMetricMeans {
    mean_a_plus: f64,
    mean_a_minus: f64,
    mean_n_plus: f64,
    mean_n_minus: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
struct CoarseFineRiskMetricAccumulator {
    sum_a_plus: f64,
    sum_a_minus: f64,
    sum_n_plus: f64,
    sum_n_minus: f64,
    observation_count: usize,
}

impl CoarseFineRiskMetricAccumulator {
    fn observe(
        &mut self,
        a_plus: f64,
        a_minus: f64,
        n_plus: f64,
        n_minus: f64,
    ) -> Result<(), &'static str> {
        if !a_plus.is_finite()
            || !a_minus.is_finite()
            || !n_plus.is_finite()
            || !n_minus.is_finite()
        {
            return Err("non-finite sufficient-statistics sample");
        }

        self.sum_a_plus += a_plus;
        self.sum_a_minus += a_minus;
        self.sum_n_plus += n_plus;
        self.sum_n_minus += n_minus;
        self.observation_count = self.observation_count.saturating_add(1);

        Ok(())
    }

    fn finalize(self) -> Option<CoarseFineRiskMetricMeans> {
        if self.observation_count == 0 {
            return None;
        }

        let denominator = self.observation_count as f64;

        Some(CoarseFineRiskMetricMeans {
            mean_a_plus: self.sum_a_plus / denominator,
            mean_a_minus: self.sum_a_minus / denominator,
            mean_n_plus: self.sum_n_plus / denominator,
            mean_n_minus: self.sum_n_minus / denominator,
        })
    }
}

pub fn run_coarse_fine_consistency_regression_harness(
    catalog: &BenchmarkScenarioCatalog,
    benchmark_policy: &OraclePricerRiskBenchmarkPolicy,
    tolerance_config: CoarseFineConsistencyRegressionToleranceConfig,
    execution_config: CoarseFineConsistencyRegressionExecutionConfig,
) -> Result<CoarseFineConsistencyRegressionReport, CoarseFineConsistencyRegressionHarnessError> {
    benchmark_policy.validate().map_err(|source| {
        CoarseFineConsistencyRegressionHarnessError::InvalidBenchmarkPolicy { source }
    })?;
    tolerance_config.validate().map_err(|source| {
        CoarseFineConsistencyRegressionHarnessError::InvalidToleranceConfig { source }
    })?;
    execution_config.validate().map_err(|source| {
        CoarseFineConsistencyRegressionHarnessError::InvalidExecutionConfig { source }
    })?;

    let mut normalized_catalog = catalog.clone();
    normalized_catalog.validate().map_err(|source| {
        CoarseFineConsistencyRegressionHarnessError::InvalidScenarioCatalog { source }
    })?;
    normalized_catalog.sort_profiles_canonically();

    let mut scenario_reports: Vec<CoarseFineConsistencyScenarioReport> =
        Vec::with_capacity(normalized_catalog.profiles.len());
    for profile in &normalized_catalog.profiles {
        let scenario_report = run_coarse_fine_consistency_scenario(
            profile,
            benchmark_policy,
            &tolerance_config,
            execution_config,
        )?;
        scenario_reports.push(scenario_report);
    }

    let passed_scenario_count = scenario_reports
        .iter()
        .filter(|scenario_report| {
            scenario_report.status == CoarseFineConsistencyScenarioStatus::Pass
        })
        .count();
    let total_scenario_count = scenario_reports.len();
    let failed_scenario_count = total_scenario_count - passed_scenario_count;
    let total_metric_count = scenario_reports
        .iter()
        .map(|scenario_report| scenario_report.metric_deltas.len())
        .sum();
    let failed_metric_count = scenario_reports
        .iter()
        .map(|scenario_report| scenario_report.failed_metric_count)
        .sum();

    Ok(CoarseFineConsistencyRegressionReport {
        schema_version: COARSE_FINE_CONSISTENCY_REGRESSION_REPORT_SCHEMA_VERSION,
        determinism_policy: COARSE_FINE_CONSISTENCY_REGRESSION_DETERMINISM_POLICY,
        variance_control_policy: COARSE_FINE_CONSISTENCY_REGRESSION_VARIANCE_CONTROL_POLICY,
        metric_policy: COARSE_FINE_CONSISTENCY_REGRESSION_METRIC_POLICY,
        governance_scope_policy: COARSE_FINE_CONSISTENCY_REGRESSION_GOVERNANCE_SCOPE_POLICY,
        output_schema_metadata: CoarseFineConsistencyRegressionOutputSchemaMetadata {
            schema_version: COARSE_FINE_CONSISTENCY_REGRESSION_OUTPUT_SCHEMA_METADATA_VERSION,
            tolerance_schema_version: COARSE_FINE_CONSISTENCY_REGRESSION_TOLERANCE_SCHEMA_VERSION,
            required_report_fields: COARSE_FINE_CONSISTENCY_REQUIRED_REPORT_FIELDS.to_vec(),
            required_scenario_report_fields:
                COARSE_FINE_CONSISTENCY_REQUIRED_SCENARIO_REPORT_FIELDS.to_vec(),
            required_metric_delta_fields: COARSE_FINE_CONSISTENCY_REQUIRED_METRIC_DELTA_FIELDS
                .to_vec(),
            required_run_metadata_fields: COARSE_FINE_CONSISTENCY_REQUIRED_RUN_METADATA_FIELDS
                .to_vec(),
        },
        catalog_id: normalized_catalog.catalog_id,
        catalog_version: normalized_catalog.catalog_version,
        benchmark_policy_id: benchmark_policy.policy_id.clone(),
        benchmark_policy_version: benchmark_policy.policy_version.clone(),
        tolerance_config,
        execution_config,
        summary: CoarseFineConsistencyRegressionSummary {
            total_scenario_count,
            passed_scenario_count,
            failed_scenario_count,
            total_metric_count,
            failed_metric_count,
        },
        scenario_reports,
    })
}

fn run_coarse_fine_consistency_scenario(
    profile: &BenchmarkScenarioWorkloadProfile,
    benchmark_policy: &OraclePricerRiskBenchmarkPolicy,
    tolerance_config: &CoarseFineConsistencyRegressionToleranceConfig,
    execution_config: CoarseFineConsistencyRegressionExecutionConfig,
) -> Result<CoarseFineConsistencyScenarioReport, CoarseFineConsistencyRegressionHarnessError> {
    let scenario_id = profile.profile_id.clone();
    let profile_seed = splitmix64(
        execution_config
            .reproducibility_seed
            .wrapping_add(fnv1a_64(profile.profile_id.as_bytes())),
    );
    let oracle_component_seed =
        splitmix64(profile_seed ^ benchmark_policy.seed_partition.oracle_component_seed_salt);
    let pricing_component_seed =
        splitmix64(profile_seed ^ benchmark_policy.seed_partition.pricing_component_seed_salt);
    let risk_component_seed =
        splitmix64(profile_seed ^ benchmark_policy.seed_partition.risk_component_seed_salt);
    let fine_interval_delta = profile.interval_settings.interval_delta
        / execution_config.fine_subinterval_count_per_coarse_interval as f64;
    let tolerance_band = tolerance_config
        .select_tolerance_band_for_interval_delta(profile.interval_settings.interval_delta)
        .map_err(|source| {
            coarse_fine_scenario_error(
                &scenario_id,
                format!("failed to resolve interval tolerance band: {source}"),
            )
        })?;

    let equation_specification =
        build_equation_specification(profile, benchmark_policy).map_err(|source| {
            coarse_fine_scenario_error(
                &scenario_id,
                format!("failed to build equation specification: {source}"),
            )
        })?;

    let oracle_solver_settings = ExponentialAffineTransformSolverSettings::try_new(
        ExponentialAffineTransformIntegratorMethod::FourthOrderRungeKutta,
        profile.interval_settings.oracle_solver_step_count,
    )
    .map_err(|source| {
        coarse_fine_scenario_error(
            &scenario_id,
            format!("failed to construct oracle solver settings: {source}"),
        )
    })?;

    let oracle_reference_step_count = profile
        .interval_settings
        .oracle_solver_step_count
        .checked_mul(execution_config.fine_subinterval_count_per_coarse_interval)
        .ok_or_else(|| {
            coarse_fine_scenario_error(
                &scenario_id,
                "oracle reference solver step count overflowed usize".to_string(),
            )
        })?;

    let oracle_reference_solver_settings = ExponentialAffineTransformSolverSettings::try_new(
        ExponentialAffineTransformIntegratorMethod::FourthOrderRungeKutta,
        oracle_reference_step_count,
    )
    .map_err(|source| {
        coarse_fine_scenario_error(
            &scenario_id,
            format!("failed to construct oracle reference solver settings: {source}"),
        )
    })?;

    let oracle_invocation_seed = derive_invocation_seed(oracle_component_seed, 1);
    let oracle_transform_input = build_oracle_transform_input(
        profile,
        benchmark_policy,
        equation_specification,
        oracle_invocation_seed,
    )
    .map_err(|source| {
        coarse_fine_scenario_error(
            &scenario_id,
            format!("failed to build oracle transform input: {source}"),
        )
    })?;

    let coarse_oracle_result = solve_exponential_affine_transform_with_settings(
        oracle_transform_input,
        oracle_solver_settings,
    )
    .map_err(|source| {
        coarse_fine_scenario_error(
            &scenario_id,
            format!("coarse oracle solve failed: {source}"),
        )
    })?;

    let oracle_reference_result = solve_exponential_affine_transform_with_settings(
        oracle_transform_input,
        oracle_reference_solver_settings,
    )
    .map_err(|source| {
        coarse_fine_scenario_error(
            &scenario_id,
            format!("oracle reference solve failed: {source}"),
        )
    })?;

    let coarse_oracle_output =
        extract_completed_oracle_output(&scenario_id, "coarse", coarse_oracle_result)?;
    let oracle_reference_output =
        extract_completed_oracle_output(&scenario_id, "reference", oracle_reference_result)?;

    let pricing_engine_settings = build_pricing_engine_settings(profile, benchmark_policy)
        .map_err(|source| {
            coarse_fine_scenario_error(
                &scenario_id,
                format!("failed to construct pricing engine settings: {source}"),
            )
        })?;

    let pricing_invocation_seed = derive_invocation_seed(pricing_component_seed, 1);
    let pricing_market_inputs =
        build_pricing_market_inputs(profile, benchmark_policy, pricing_invocation_seed).map_err(
            |source| {
                coarse_fine_scenario_error(
                    &scenario_id,
                    format!("failed to build pricing market inputs: {source}"),
                )
            },
        )?;
    let pricing_payoff_type = VanillaOptionPayoffType::Call;
    let pricing_payoff = VanillaOptionPayoffConfiguration {
        payoff_type: pricing_payoff_type,
    };
    let pricing_volatility = interpolate_range(
        benchmark_policy.pricing_input_ranges.volatility_min,
        benchmark_policy.pricing_input_ranges.volatility_max,
        unit_interval_from_seed(pricing_invocation_seed ^ 0x51),
    );

    let pricing_spot = pricing_market_inputs.spot;
    let pricing_strike = pricing_market_inputs.strike;
    let pricing_maturity_interval = pricing_market_inputs.maturity_interval;
    let pricing_risk_free_rate = pricing_market_inputs.risk_free_rate;
    let pricing_dividend_yield = pricing_market_inputs.dividend_yield;

    let pricing_drift = (pricing_risk_free_rate
        - pricing_dividend_yield
        - 0.5 * pricing_volatility * pricing_volatility)
        * pricing_maturity_interval;
    let pricing_variance = pricing_volatility * pricing_volatility * pricing_maturity_interval;

    let coarse_pricing_result = price_vanilla_option_via_fourier_inversion(
        pricing_market_inputs,
        pricing_payoff,
        pricing_engine_settings,
        |argument| {
            let i_times_argument = ComplexScalar::new(-argument.imaginary, argument.real);
            let drift_term = i_times_argument.scale(pricing_drift);
            let diffusion_term = argument.square().scale(-0.5 * pricing_variance);
            complex_exponential(drift_term.add(diffusion_term))
        },
    )
    .map_err(|source| {
        coarse_fine_scenario_error(
            &scenario_id,
            format!("coarse pricing call failed: {source}"),
        )
    })?;
    let coarse_pricing_value = coarse_pricing_result.price;

    let fine_pricing_reference_value = estimate_fine_pricing_reference_price(
        &scenario_id,
        pricing_component_seed,
        execution_config.fine_pricing_reference_path_count,
        execution_config.fine_pricing_reference_step_count,
        pricing_spot,
        pricing_strike,
        pricing_maturity_interval,
        pricing_risk_free_rate,
        pricing_dividend_yield,
        pricing_volatility,
        pricing_payoff_type,
    )?;

    let coarse_risk_factor_series =
        build_coarse_risk_factor_series(profile, benchmark_policy, risk_component_seed);
    let fine_risk_factor_series = expand_risk_factor_series_for_fine_reference(
        &coarse_risk_factor_series,
        execution_config.fine_subinterval_count_per_coarse_interval,
    );

    let coarse_risk_slices = solve_coarse_fine_risk_slices(
        &scenario_id,
        equation_specification,
        oracle_solver_settings,
        profile.model_state.initial_excitation_state,
        profile.interval_settings.interval_delta,
        &coarse_risk_factor_series,
    )?;
    let fine_risk_slices = solve_coarse_fine_risk_slices(
        &scenario_id,
        equation_specification,
        oracle_solver_settings,
        profile.model_state.initial_excitation_state,
        fine_interval_delta,
        &fine_risk_factor_series,
    )?;

    let coarse_risk_metric_means = sample_coarse_risk_metric_means(
        &scenario_id,
        risk_component_seed,
        execution_config.paired_risk_path_count,
        &coarse_risk_slices,
    )?;
    let fine_risk_metric_means = sample_fine_risk_reference_metric_means(
        &scenario_id,
        risk_component_seed,
        execution_config.paired_risk_path_count,
        profile.interval_settings.horizon_interval_count,
        execution_config.fine_subinterval_count_per_coarse_interval,
        &fine_risk_slices,
    )?;

    let metric_deltas = vec![
        build_coarse_fine_metric_delta(
            "oracle_c_at_delta_absolute_difference",
            complex_difference_norm(
                coarse_oracle_output.c_at_delta,
                oracle_reference_output.c_at_delta,
            ),
            0.0,
            tolerance_band.oracle_c_at_delta_absolute_tolerance,
            0.0,
        ),
        build_coarse_fine_metric_delta(
            "oracle_d_component_max_absolute_difference",
            max_abs_oracle_d_component_difference(coarse_oracle_output, oracle_reference_output),
            0.0,
            tolerance_band.oracle_d_component_absolute_tolerance,
            0.0,
        ),
        build_coarse_fine_metric_delta(
            "oracle_transform_value_absolute_difference",
            complex_difference_norm(
                coarse_oracle_output.transform_value,
                oracle_reference_output.transform_value,
            ),
            0.0,
            tolerance_band.oracle_transform_value_absolute_tolerance,
            0.0,
        ),
        build_coarse_fine_metric_delta(
            "pricing_vanilla_option_price",
            coarse_pricing_value,
            fine_pricing_reference_value,
            tolerance_band.pricing_price_absolute_delta_tolerance,
            tolerance_band.pricing_price_relative_delta_tolerance,
        ),
        build_coarse_fine_metric_delta(
            "risk_mean_a_plus",
            coarse_risk_metric_means.mean_a_plus,
            fine_risk_metric_means.mean_a_plus,
            tolerance_band.risk_integrated_intensity_mean_absolute_delta_tolerance,
            tolerance_band.risk_integrated_intensity_mean_relative_delta_tolerance,
        ),
        build_coarse_fine_metric_delta(
            "risk_mean_a_minus",
            coarse_risk_metric_means.mean_a_minus,
            fine_risk_metric_means.mean_a_minus,
            tolerance_band.risk_integrated_intensity_mean_absolute_delta_tolerance,
            tolerance_band.risk_integrated_intensity_mean_relative_delta_tolerance,
        ),
        build_coarse_fine_metric_delta(
            "risk_mean_n_plus",
            coarse_risk_metric_means.mean_n_plus,
            fine_risk_metric_means.mean_n_plus,
            tolerance_band.risk_event_count_mean_absolute_delta_tolerance,
            tolerance_band.risk_event_count_mean_relative_delta_tolerance,
        ),
        build_coarse_fine_metric_delta(
            "risk_mean_n_minus",
            coarse_risk_metric_means.mean_n_minus,
            fine_risk_metric_means.mean_n_minus,
            tolerance_band.risk_event_count_mean_absolute_delta_tolerance,
            tolerance_band.risk_event_count_mean_relative_delta_tolerance,
        ),
    ];

    let failed_metric_count = metric_deltas
        .iter()
        .filter(|metric_delta| !metric_delta.passed)
        .count();
    let max_absolute_delta = metric_deltas
        .iter()
        .map(|metric_delta| metric_delta.absolute_delta)
        .fold(0.0_f64, f64::max);
    let status = if failed_metric_count == 0 {
        CoarseFineConsistencyScenarioStatus::Pass
    } else {
        CoarseFineConsistencyScenarioStatus::Fail
    };

    Ok(CoarseFineConsistencyScenarioReport {
        scenario_id: scenario_id.clone(),
        applied_tolerance_band_label: tolerance_band.interval_grid_label.clone(),
        status,
        failed_metric_count,
        max_absolute_delta,
        run_metadata: CoarseFineConsistencyScenarioRunMetadata {
            scenario_id,
            profile_seed,
            oracle_component_seed,
            pricing_component_seed,
            risk_component_seed,
            coarse_interval_delta: profile.interval_settings.interval_delta,
            fine_interval_delta,
            horizon_interval_count: profile.interval_settings.horizon_interval_count,
            fine_subinterval_count_per_coarse_interval: execution_config
                .fine_subinterval_count_per_coarse_interval,
            paired_risk_path_count: execution_config.paired_risk_path_count,
            fine_pricing_reference_path_count: execution_config.fine_pricing_reference_path_count,
            fine_pricing_reference_step_count: execution_config.fine_pricing_reference_step_count,
        },
        summary: CoarseFineConsistencyScenarioSummary {
            coarse_pricing_value,
            fine_pricing_reference_value,
            coarse_risk_mean_a_plus: coarse_risk_metric_means.mean_a_plus,
            fine_risk_mean_a_plus: fine_risk_metric_means.mean_a_plus,
            coarse_risk_mean_a_minus: coarse_risk_metric_means.mean_a_minus,
            fine_risk_mean_a_minus: fine_risk_metric_means.mean_a_minus,
            coarse_risk_mean_n_plus: coarse_risk_metric_means.mean_n_plus,
            fine_risk_mean_n_plus: fine_risk_metric_means.mean_n_plus,
            coarse_risk_mean_n_minus: coarse_risk_metric_means.mean_n_minus,
            fine_risk_mean_n_minus: fine_risk_metric_means.mean_n_minus,
        },
        metric_deltas,
    })
}

fn extract_completed_oracle_output(
    scenario_id: &str,
    label: &str,
    result: ExponentialAffineTransformIntegrationResult,
) -> Result<ExponentialAffineTransformOutput, CoarseFineConsistencyRegressionHarnessError> {
    if result.status != ExponentialAffineTransformIntegrationStatus::Completed {
        return Err(coarse_fine_scenario_error(
            scenario_id,
            format!(
                "{label} oracle solve did not complete successfully, status={:?}",
                result.status
            ),
        ));
    }

    result.output.ok_or_else(|| {
        coarse_fine_scenario_error(
            scenario_id,
            format!("{label} oracle solve returned no output payload"),
        )
    })
}

fn complex_difference_norm(left: ComplexScalar, right: ComplexScalar) -> f64 {
    left.subtract(right).norm()
}

fn max_abs_oracle_d_component_difference(
    coarse_output: ExponentialAffineTransformOutput,
    fine_reference_output: ExponentialAffineTransformOutput,
) -> f64 {
    let d_at_delta_coarse = coarse_output.d_at_delta;
    let d_at_delta_fine_reference = fine_reference_output.d_at_delta;
    [
        complex_difference_norm(
            d_at_delta_coarse.plus_plus,
            d_at_delta_fine_reference.plus_plus,
        ),
        complex_difference_norm(
            d_at_delta_coarse.plus_minus,
            d_at_delta_fine_reference.plus_minus,
        ),
        complex_difference_norm(
            d_at_delta_coarse.minus_plus,
            d_at_delta_fine_reference.minus_plus,
        ),
        complex_difference_norm(
            d_at_delta_coarse.minus_minus,
            d_at_delta_fine_reference.minus_minus,
        ),
    ]
    .into_iter()
    .fold(0.0_f64, f64::max)
}

fn coarse_fine_scenario_error(
    scenario_id: &str,
    detail: String,
) -> CoarseFineConsistencyRegressionHarnessError {
    CoarseFineConsistencyRegressionHarnessError::ScenarioExperimentFailure {
        scenario_id: scenario_id.to_string(),
        detail,
    }
}

fn build_coarse_fine_metric_delta(
    metric_id: &'static str,
    coarse_value: f64,
    fine_reference_value: f64,
    absolute_tolerance: f64,
    relative_tolerance: f64,
) -> CoarseFineConsistencyMetricDelta {
    let absolute_delta = if coarse_value.is_finite() && fine_reference_value.is_finite() {
        (coarse_value - fine_reference_value).abs()
    } else {
        f64::INFINITY
    };
    let denominator = fine_reference_value.abs().max(1.0e-12);
    let relative_delta = if absolute_delta.is_finite() {
        absolute_delta / denominator
    } else {
        f64::INFINITY
    };
    let passed = absolute_delta <= absolute_tolerance || relative_delta <= relative_tolerance;

    CoarseFineConsistencyMetricDelta {
        metric_id,
        coarse_value,
        fine_reference_value,
        absolute_delta,
        relative_delta,
        absolute_tolerance,
        relative_tolerance,
        passed,
    }
}

fn build_coarse_risk_factor_series(
    profile: &BenchmarkScenarioWorkloadProfile,
    benchmark_policy: &OraclePricerRiskBenchmarkPolicy,
    risk_component_seed: u64,
) -> Vec<(f64, f64)> {
    let mut factor_series: Vec<(f64, f64)> =
        Vec::with_capacity(profile.interval_settings.horizon_interval_count);
    for interval_index in 0..profile.interval_settings.horizon_interval_count {
        let interval_seed = derive_invocation_seed(risk_component_seed, interval_index + 1);
        let factor_noise = unit_interval_from_seed(interval_seed);
        let seasonality_factor = interpolate_range(
            benchmark_policy.oracle_solver_probes.seasonality_factor_min,
            benchmark_policy.oracle_solver_probes.seasonality_factor_max,
            factor_noise,
        );
        let news_intensity_factor = interpolate_range(
            benchmark_policy
                .oracle_solver_probes
                .news_intensity_factor_min,
            benchmark_policy
                .oracle_solver_probes
                .news_intensity_factor_max,
            1.0 - factor_noise,
        );

        factor_series.push((seasonality_factor, news_intensity_factor));
    }

    factor_series
}

fn expand_risk_factor_series_for_fine_reference(
    coarse_factor_series: &[(f64, f64)],
    fine_subinterval_count_per_coarse_interval: usize,
) -> Vec<(f64, f64)> {
    let mut fine_factor_series: Vec<(f64, f64)> =
        Vec::with_capacity(coarse_factor_series.len() * fine_subinterval_count_per_coarse_interval);

    for &(seasonality_factor, news_intensity_factor) in coarse_factor_series {
        for _ in 0..fine_subinterval_count_per_coarse_interval {
            fine_factor_series.push((seasonality_factor, news_intensity_factor));
        }
    }

    fine_factor_series
}

fn solve_coarse_fine_risk_slices(
    scenario_id: &str,
    equation_specification: ExponentialAffineTransformEquationSpecification,
    solver_settings: ExponentialAffineTransformSolverSettings,
    initial_excitation_state: HawkesExcitationState,
    interval_delta: f64,
    factor_series: &[(f64, f64)],
) -> Result<Vec<SufficientStatisticsTransformSlices>, CoarseFineConsistencyRegressionHarnessError> {
    let sampling_settings = SufficientStatisticsTransformSliceSettings::default();
    let mut slices_by_interval: Vec<SufficientStatisticsTransformSlices> =
        Vec::with_capacity(factor_series.len());

    for (interval_index, (seasonality_factor, news_intensity_factor)) in
        factor_series.iter().copied().enumerate()
    {
        let known_news_intensity_factor = news_intensity_factor * 0.5;
        let surprise_news_intensity_factor =
            news_intensity_factor - known_news_intensity_factor;

        let context = SufficientStatisticsTransformSliceContext::try_new(
            interval_index,
            seasonality_factor,
            ExponentialAffineTransformNewsIntensityInput::new(
                known_news_intensity_factor,
                surprise_news_intensity_factor,
            ),
            interval_delta,
            initial_excitation_state,
        )
        .map_err(|source| {
            coarse_fine_scenario_error(
                scenario_id,
                format!("interval {interval_index} risk slice context validation failed: {source}"),
            )
        })?;

        let solved_slices = solve_sufficient_statistics_transform_slices_with_solver_settings(
            equation_specification,
            context,
            sampling_settings,
            Some(solver_settings),
        )
        .map_err(|source| {
            coarse_fine_scenario_error(
                scenario_id,
                format!("interval {interval_index} risk slice solve failed: {source}"),
            )
        })?;

        slices_by_interval.push(solved_slices);
    }

    Ok(slices_by_interval)
}

fn sample_coarse_risk_metric_means(
    scenario_id: &str,
    risk_component_seed: u64,
    paired_risk_path_count: usize,
    slices_by_interval: &[SufficientStatisticsTransformSlices],
) -> Result<CoarseFineRiskMetricMeans, CoarseFineConsistencyRegressionHarnessError> {
    let mut accumulator = CoarseFineRiskMetricAccumulator::default();

    for path_index in 0..paired_risk_path_count {
        let path_seed = derive_invocation_seed(risk_component_seed, path_index + 1);
        let sampled_statistics =
            sample_interval_statistics_batch_from_transform_slices(path_seed, slices_by_interval)
                .map_err(|source| {
                coarse_fine_scenario_error(
                    scenario_id,
                    format!("coarse risk slice sampling failed: {source}"),
                )
            })?;

        for interval_statistics in sampled_statistics {
            accumulator
                .observe(
                    interval_statistics.a_plus,
                    interval_statistics.a_minus,
                    interval_statistics.n_plus as f64,
                    interval_statistics.n_minus as f64,
                )
                .map_err(|detail| coarse_fine_scenario_error(scenario_id, detail.to_string()))?;
        }
    }

    let metric_means = accumulator.finalize().ok_or_else(|| {
        coarse_fine_scenario_error(
            scenario_id,
            "coarse risk metric aggregation produced no observations".to_string(),
        )
    })?;

    validate_risk_metric_means(scenario_id, "coarse", metric_means)
}

fn sample_fine_risk_reference_metric_means(
    scenario_id: &str,
    risk_component_seed: u64,
    paired_risk_path_count: usize,
    coarse_interval_count: usize,
    fine_subinterval_count_per_coarse_interval: usize,
    slices_by_interval: &[SufficientStatisticsTransformSlices],
) -> Result<CoarseFineRiskMetricMeans, CoarseFineConsistencyRegressionHarnessError> {
    let expected_fine_interval_count =
        coarse_interval_count.saturating_mul(fine_subinterval_count_per_coarse_interval);
    if slices_by_interval.len() != expected_fine_interval_count {
        return Err(coarse_fine_scenario_error(
            scenario_id,
            format!(
                "fine slice count mismatch, expected {expected_fine_interval_count} intervals but found {}",
                slices_by_interval.len()
            ),
        ));
    }

    let mut accumulator = CoarseFineRiskMetricAccumulator::default();
    for path_index in 0..paired_risk_path_count {
        let path_seed = derive_invocation_seed(risk_component_seed, path_index + 1);
        let sampled_statistics =
            sample_interval_statistics_batch_from_transform_slices(path_seed, slices_by_interval)
                .map_err(|source| {
                coarse_fine_scenario_error(
                    scenario_id,
                    format!("fine risk slice sampling failed: {source}"),
                )
            })?;
        if sampled_statistics.len() != expected_fine_interval_count {
            return Err(coarse_fine_scenario_error(
                scenario_id,
                format!(
                    "fine sampled-statistics count mismatch, expected {expected_fine_interval_count} entries but found {}",
                    sampled_statistics.len()
                ),
            ));
        }

        for coarse_interval_index in 0..coarse_interval_count {
            let fine_start =
                coarse_interval_index.saturating_mul(fine_subinterval_count_per_coarse_interval);
            let fine_end = fine_start + fine_subinterval_count_per_coarse_interval;
            let mut aggregated_a_plus = 0.0_f64;
            let mut aggregated_a_minus = 0.0_f64;
            let mut aggregated_n_plus = 0.0_f64;
            let mut aggregated_n_minus = 0.0_f64;

            for interval_statistics in &sampled_statistics[fine_start..fine_end] {
                aggregated_a_plus += interval_statistics.a_plus;
                aggregated_a_minus += interval_statistics.a_minus;
                aggregated_n_plus += interval_statistics.n_plus as f64;
                aggregated_n_minus += interval_statistics.n_minus as f64;
            }

            accumulator
                .observe(
                    aggregated_a_plus,
                    aggregated_a_minus,
                    aggregated_n_plus,
                    aggregated_n_minus,
                )
                .map_err(|detail| coarse_fine_scenario_error(scenario_id, detail.to_string()))?;
        }
    }

    let metric_means = accumulator.finalize().ok_or_else(|| {
        coarse_fine_scenario_error(
            scenario_id,
            "fine risk metric aggregation produced no observations".to_string(),
        )
    })?;
    validate_risk_metric_means(scenario_id, "fine", metric_means)
}

fn validate_risk_metric_means(
    scenario_id: &str,
    label: &str,
    metric_means: CoarseFineRiskMetricMeans,
) -> Result<CoarseFineRiskMetricMeans, CoarseFineConsistencyRegressionHarnessError> {
    let fields = [
        ("mean_a_plus", metric_means.mean_a_plus),
        ("mean_a_minus", metric_means.mean_a_minus),
        ("mean_n_plus", metric_means.mean_n_plus),
        ("mean_n_minus", metric_means.mean_n_minus),
    ];
    for (field_name, value) in fields {
        if !value.is_finite() {
            return Err(coarse_fine_scenario_error(
                scenario_id,
                format!("{label} risk metric `{field_name}` is non-finite: {value}"),
            ));
        }
    }

    Ok(metric_means)
}

#[allow(clippy::too_many_arguments)]
fn estimate_fine_pricing_reference_price(
    scenario_id: &str,
    pricing_component_seed: u64,
    path_count: usize,
    step_count: usize,
    spot: f64,
    strike: f64,
    maturity_interval: f64,
    risk_free_rate: f64,
    dividend_yield: f64,
    volatility: f64,
    payoff_type: VanillaOptionPayoffType,
) -> Result<f64, CoarseFineConsistencyRegressionHarnessError> {
    let fields = [
        ("spot", spot),
        ("strike", strike),
        ("maturity_interval", maturity_interval),
        ("risk_free_rate", risk_free_rate),
        ("dividend_yield", dividend_yield),
        ("volatility", volatility),
    ];
    for (field_name, value) in fields {
        if !value.is_finite() {
            return Err(coarse_fine_scenario_error(
                scenario_id,
                format!(
                    "fine pricing reference input `{field_name}` must be finite, found {value}"
                ),
            ));
        }
    }

    if spot <= 0.0 {
        return Err(coarse_fine_scenario_error(
            scenario_id,
            format!("fine pricing reference input `spot` must be positive, found {spot}"),
        ));
    }
    if strike <= 0.0 {
        return Err(coarse_fine_scenario_error(
            scenario_id,
            format!("fine pricing reference input `strike` must be positive, found {strike}"),
        ));
    }
    if maturity_interval <= 0.0 {
        return Err(coarse_fine_scenario_error(
            scenario_id,
            format!(
                "fine pricing reference input `maturity_interval` must be positive, found {maturity_interval}"
            ),
        ));
    }
    if volatility <= 0.0 {
        return Err(coarse_fine_scenario_error(
            scenario_id,
            format!(
                "fine pricing reference input `volatility` must be positive, found {volatility}"
            ),
        ));
    }

    let time_step = maturity_interval / step_count as f64;
    let drift_per_step =
        (risk_free_rate - dividend_yield - 0.5 * volatility * volatility) * time_step;
    let volatility_per_step = volatility * time_step.sqrt();
    let mut payoff_sum = 0.0_f64;

    for path_index in 0..path_count {
        let path_seed = derive_invocation_seed(pricing_component_seed, path_index + 1);
        let mut log_terminal_spot = spot.ln();

        for step_index in 0..step_count {
            let step_seed = derive_invocation_seed(path_seed, step_index + 1);
            let standard_normal = sample_standard_normal_from_seed(step_seed);

            log_terminal_spot += drift_per_step + volatility_per_step * standard_normal;
        }

        let terminal_spot = log_terminal_spot.exp();
        let path_payoff = match payoff_type {
            VanillaOptionPayoffType::Call => (terminal_spot - strike).max(0.0),
            VanillaOptionPayoffType::Put => (strike - terminal_spot).max(0.0),
        };
        payoff_sum += path_payoff;
    }

    let discounted_price =
        (-risk_free_rate * maturity_interval).exp() * (payoff_sum / path_count as f64);
    if !discounted_price.is_finite() {
        return Err(coarse_fine_scenario_error(
            scenario_id,
            format!("fine pricing reference value is non-finite: {discounted_price}"),
        ));
    }

    Ok(discounted_price)
}
fn sample_standard_normal_from_seed(seed: u64) -> f64 {
    let first_uniform_draw = unit_interval_from_seed(seed ^ 0x8B77_D4C9_14F2_5A11)
        .clamp(f64::MIN_POSITIVE, 1.0 - f64::EPSILON);
    let second_uniform_draw = unit_interval_from_seed(seed ^ 0xC4D5_1E6A_227A_F093);
    let radius = (-2.0 * first_uniform_draw.ln()).sqrt();
    let angle = 2.0 * std::f64::consts::PI * second_uniform_draw;

    radius * angle.cos()
}
