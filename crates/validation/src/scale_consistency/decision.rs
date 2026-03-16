//! Threshold policy parsing and pass-fail decisions for scale-consistency validation.

use std::collections::BTreeSet;
use std::error::Error;
use std::fmt::{Display, Formatter};

use janiform_analytics_stylized_facts::{
    StylizedFactMetricCatalog, build_stylized_fact_metric_catalog,
};
use serde::{Deserialize, Serialize};

use crate::{
    COARSE_FINE_CONSISTENCY_REGRESSION_GOVERNANCE_SCOPE_POLICY,
    COARSE_FINE_CONSISTENCY_REGRESSION_REPORT_SCHEMA_VERSION,
    CoarseFineConsistencyRegressionReport, RegressionCaseStatus,
};

use super::deltas::is_supported_scale_consistency_metric;
use super::{
    SCALE_CONSISTENCY_METRIC_DELTA_BATCH_REPORT_SCHEMA_VERSION,
    SCALE_CONSISTENCY_METRIC_DELTA_ORDERING_POLICY, ScaleConsistencyMetricCategory,
    ScaleConsistencyMetricComputationFailure, ScaleConsistencyMetricComputationStatus,
    ScaleConsistencyMetricDelta, ScaleConsistencyMetricDeltaBatchReport,
    ScaleConsistencyMetricDeltaKind, ScaleConsistencyMetricDeltaScenarioReport,
};

pub const SCALE_CONSISTENCY_THRESHOLD_POLICY_SCHEMA_VERSION: &str =
    "validation.scale_consistency.threshold_policy.v1";
pub const SCALE_CONSISTENCY_THRESHOLD_POLICY_GOVERNANCE_SCOPE_POLICY: &str = "model_validation_release_and_calibration_governance_thresholds_with_engine_benchmark_prerequisite_inputs.v1";
pub const SCALE_CONSISTENCY_DECISION_REPORT_SCHEMA_VERSION: &str =
    "validation.scale_consistency.decision_report.v1";
pub const SCALE_CONSISTENCY_DECISION_ORDERING_POLICY: &str =
    "preserve_metric_delta_report_scenario_order_and_metric_order.v1";
pub const SCALE_CONSISTENCY_DECISION_POLICY: &str = "interval_band_absolute_delta_thresholds_with_inclusive_boundaries_missing_metric_failures_and_engine_benchmark_batch_prerequisite.v1";
pub const SCALE_CONSISTENCY_ENGINE_BENCHMARK_INPUT_POLICY: &str = "initiative_004_engine_benchmark_report_status_is_consumed_as_a_governance_prerequisite_without_redefining_engine_level_thresholds.v1";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScaleConsistencyThresholdPolicyChangeLogEntry {
    pub policy_version: String,
    pub recorded_at_utc: String,
    pub rationale: String,
}

impl ScaleConsistencyThresholdPolicyChangeLogEntry {
    pub fn validate(&self) -> Result<(), ScaleConsistencyThresholdPolicyChangeLogEntryError> {
        if self.policy_version.trim().is_empty() {
            return Err(ScaleConsistencyThresholdPolicyChangeLogEntryError::EmptyPolicyVersion);
        }
        if self.recorded_at_utc.trim().is_empty() {
            return Err(ScaleConsistencyThresholdPolicyChangeLogEntryError::EmptyRecordedAtUtc);
        }
        if self.rationale.trim().is_empty() {
            return Err(ScaleConsistencyThresholdPolicyChangeLogEntryError::EmptyRationale);
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ScaleConsistencyThresholdPolicyChangeLogEntryError {
    EmptyPolicyVersion,
    EmptyRecordedAtUtc,
    EmptyRationale,
}

impl Display for ScaleConsistencyThresholdPolicyChangeLogEntryError {
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

impl Error for ScaleConsistencyThresholdPolicyChangeLogEntryError {}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScaleConsistencyMetricThreshold {
    pub threshold_id: String,
    pub metric_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metric_variant: Option<String>,
    pub max_absolute_delta: f64,
    pub rationale: String,
}

impl ScaleConsistencyMetricThreshold {
    fn validate(
        &self,
        metric_catalog: &StylizedFactMetricCatalog,
    ) -> Result<(), ScaleConsistencyMetricThresholdError> {
        if self.threshold_id.trim().is_empty() {
            return Err(ScaleConsistencyMetricThresholdError::EmptyThresholdId);
        }
        if self.metric_id.trim().is_empty() {
            return Err(ScaleConsistencyMetricThresholdError::EmptyMetricId);
        }

        if let Some(metric_variant) = self.metric_variant.as_deref()
            && metric_variant.trim().is_empty()
        {
            return Err(ScaleConsistencyMetricThresholdError::EmptyMetricVariant);
        }

        if !metric_catalog
            .metrics
            .iter()
            .any(|metric| metric.metric_id == self.metric_id)
        {
            return Err(
                ScaleConsistencyMetricThresholdError::MetricNotPresentInStylizedFactCatalog {
                    metric_id: self.metric_id.clone(),
                },
            );
        }
        if !is_supported_scale_consistency_metric(self.metric_id.as_str()) {
            return Err(
                ScaleConsistencyMetricThresholdError::MetricUnsupportedByScaleConsistency {
                    metric_id: self.metric_id.clone(),
                },
            );
        }

        if !self.max_absolute_delta.is_finite() || self.max_absolute_delta < 0.0 {
            return Err(
                ScaleConsistencyMetricThresholdError::InvalidMaxAbsoluteDelta {
                    value: self.max_absolute_delta,
                },
            );
        }

        if self.rationale.trim().is_empty() {
            return Err(ScaleConsistencyMetricThresholdError::EmptyRationale);
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ScaleConsistencyMetricThresholdError {
    EmptyThresholdId,
    EmptyMetricId,
    EmptyMetricVariant,
    MetricNotPresentInStylizedFactCatalog { metric_id: String },
    MetricUnsupportedByScaleConsistency { metric_id: String },
    InvalidMaxAbsoluteDelta { value: f64 },
    EmptyRationale,
}

impl Display for ScaleConsistencyMetricThresholdError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyThresholdId => write!(formatter, "threshold_id must be a non-empty string"),
            Self::EmptyMetricId => write!(formatter, "metric_id must be a non-empty string"),
            Self::EmptyMetricVariant => {
                write!(
                    formatter,
                    "metric_variant must be a non-empty string when provided"
                )
            }
            Self::MetricNotPresentInStylizedFactCatalog { metric_id } => write!(
                formatter,
                "metric_id `{metric_id}` is not present in the stylized-fact metric catalog"
            ),
            Self::MetricUnsupportedByScaleConsistency { metric_id } => write!(
                formatter,
                "metric_id `{metric_id}` is not supported by the scale-consistency decision engine"
            ),
            Self::InvalidMaxAbsoluteDelta { value } => write!(
                formatter,
                "max_absolute_delta must be finite and non-negative, found {value}"
            ),
            Self::EmptyRationale => write!(formatter, "rationale must be a non-empty string"),
        }
    }
}

impl Error for ScaleConsistencyMetricThresholdError {}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScaleConsistencyIntervalThresholdBand {
    pub interval_grid_label: String,
    pub delta_upper_bound: f64,
    pub metric_thresholds: Vec<ScaleConsistencyMetricThreshold>,
}

impl ScaleConsistencyIntervalThresholdBand {
    fn validate(
        &self,
        metric_catalog: &StylizedFactMetricCatalog,
    ) -> Result<(), ScaleConsistencyIntervalThresholdBandError> {
        if self.interval_grid_label.trim().is_empty() {
            return Err(ScaleConsistencyIntervalThresholdBandError::EmptyIntervalGridLabel);
        }
        if !self.delta_upper_bound.is_finite() || self.delta_upper_bound <= 0.0 {
            return Err(
                ScaleConsistencyIntervalThresholdBandError::InvalidDeltaUpperBound {
                    value: self.delta_upper_bound,
                },
            );
        }
        if self.metric_thresholds.is_empty() {
            return Err(ScaleConsistencyIntervalThresholdBandError::EmptyMetricThresholds);
        }

        let mut observed_metric_keys: BTreeSet<(String, Option<String>)> = BTreeSet::new();
        for (position, threshold) in self.metric_thresholds.iter().enumerate() {
            threshold.validate(metric_catalog).map_err(|source| {
                ScaleConsistencyIntervalThresholdBandError::InvalidMetricThreshold {
                    position,
                    source,
                }
            })?;

            let metric_key = (
                threshold.metric_id.clone(),
                threshold.metric_variant.clone(),
            );
            if !observed_metric_keys.insert(metric_key.clone()) {
                return Err(
                    ScaleConsistencyIntervalThresholdBandError::DuplicateMetricThreshold {
                        metric_id: metric_key.0,
                        metric_variant: metric_key.1,
                    },
                );
            }
        }

        Ok(())
    }

    fn resolve_threshold(
        &self,
        metric_id: &str,
        metric_variant: Option<&str>,
    ) -> Option<&ScaleConsistencyMetricThreshold> {
        if let Some(metric_variant) = metric_variant
            && let Some(exact_match) = self.metric_thresholds.iter().find(|threshold| {
                threshold.metric_id == metric_id
                    && threshold.metric_variant.as_deref() == Some(metric_variant)
            })
        {
            return Some(exact_match);
        }

        self.metric_thresholds.iter().find(|threshold| {
            threshold.metric_id == metric_id && threshold.metric_variant.is_none()
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ScaleConsistencyIntervalThresholdBandError {
    EmptyIntervalGridLabel,
    InvalidDeltaUpperBound {
        value: f64,
    },
    EmptyMetricThresholds,
    InvalidMetricThreshold {
        position: usize,
        source: ScaleConsistencyMetricThresholdError,
    },
    DuplicateMetricThreshold {
        metric_id: String,
        metric_variant: Option<String>,
    },
}

impl Display for ScaleConsistencyIntervalThresholdBandError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyIntervalGridLabel => {
                write!(formatter, "interval_grid_label must be a non-empty string")
            }
            Self::InvalidDeltaUpperBound { value } => write!(
                formatter,
                "delta_upper_bound must be finite and strictly positive, found {value}"
            ),
            Self::EmptyMetricThresholds => write!(
                formatter,
                "metric_thresholds must contain at least one configured metric threshold"
            ),
            Self::InvalidMetricThreshold { position, source } => write!(
                formatter,
                "metric_thresholds[{position}] is invalid: {source}"
            ),
            Self::DuplicateMetricThreshold {
                metric_id,
                metric_variant,
            } => write!(
                formatter,
                "metric_thresholds must be unique by metric_id and metric_variant, found duplicate metric_id=`{metric_id}` and metric_variant={metric_variant:?}"
            ),
        }
    }
}

impl Error for ScaleConsistencyIntervalThresholdBandError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidMetricThreshold { source, .. } => Some(source),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScaleConsistencyThresholdPolicy {
    pub schema_version: String,
    pub policy_id: String,
    pub policy_version: String,
    pub governance_scope_policy: String,
    pub interval_threshold_bands: Vec<ScaleConsistencyIntervalThresholdBand>,
    pub change_log: Vec<ScaleConsistencyThresholdPolicyChangeLogEntry>,
}

impl ScaleConsistencyThresholdPolicy {
    pub fn validate(&self) -> Result<(), ScaleConsistencyThresholdPolicyError> {
        if self.schema_version != SCALE_CONSISTENCY_THRESHOLD_POLICY_SCHEMA_VERSION {
            return Err(ScaleConsistencyThresholdPolicyError::InvalidSchemaVersion {
                expected: SCALE_CONSISTENCY_THRESHOLD_POLICY_SCHEMA_VERSION,
                actual: self.schema_version.clone(),
            });
        }

        if self.policy_id.trim().is_empty() {
            return Err(ScaleConsistencyThresholdPolicyError::EmptyPolicyId);
        }
        if self.policy_version.trim().is_empty() {
            return Err(ScaleConsistencyThresholdPolicyError::EmptyPolicyVersion);
        }

        if self.governance_scope_policy
            != SCALE_CONSISTENCY_THRESHOLD_POLICY_GOVERNANCE_SCOPE_POLICY
        {
            return Err(
                ScaleConsistencyThresholdPolicyError::InvalidGovernanceScopePolicy {
                    expected: SCALE_CONSISTENCY_THRESHOLD_POLICY_GOVERNANCE_SCOPE_POLICY,
                    actual: self.governance_scope_policy.clone(),
                },
            );
        }

        if self.interval_threshold_bands.is_empty() {
            return Err(ScaleConsistencyThresholdPolicyError::EmptyIntervalThresholdBands);
        }
        if self.change_log.is_empty() {
            return Err(ScaleConsistencyThresholdPolicyError::EmptyChangeLog);
        }

        let metric_catalog = build_stylized_fact_metric_catalog();
        let mut previous_delta_upper_bound: Option<f64> = None;
        let mut observed_threshold_ids: BTreeSet<String> = BTreeSet::new();
        for (position, interval_band) in self.interval_threshold_bands.iter().enumerate() {
            interval_band.validate(&metric_catalog).map_err(|source| {
                ScaleConsistencyThresholdPolicyError::InvalidIntervalThresholdBand {
                    position,
                    source,
                }
            })?;

            if let Some(previous_delta_upper_bound) = previous_delta_upper_bound
                && interval_band.delta_upper_bound <= previous_delta_upper_bound
            {
                return Err(
                    ScaleConsistencyThresholdPolicyError::NonIncreasingDeltaUpperBound {
                        previous: previous_delta_upper_bound,
                        current: interval_band.delta_upper_bound,
                    },
                );
            }
            previous_delta_upper_bound = Some(interval_band.delta_upper_bound);

            for threshold in &interval_band.metric_thresholds {
                if !observed_threshold_ids.insert(threshold.threshold_id.clone()) {
                    return Err(ScaleConsistencyThresholdPolicyError::DuplicateThresholdId {
                        threshold_id: threshold.threshold_id.clone(),
                    });
                }
            }
        }

        let mut observed_policy_versions: BTreeSet<String> = BTreeSet::new();
        let mut has_current_policy_version = false;
        for (position, change_log_entry) in self.change_log.iter().enumerate() {
            change_log_entry.validate().map_err(|source| {
                ScaleConsistencyThresholdPolicyError::InvalidChangeLogEntry { position, source }
            })?;

            if !observed_policy_versions.insert(change_log_entry.policy_version.clone()) {
                return Err(
                    ScaleConsistencyThresholdPolicyError::DuplicateChangeLogPolicyVersion {
                        policy_version: change_log_entry.policy_version.clone(),
                    },
                );
            }
            has_current_policy_version = has_current_policy_version
                || change_log_entry.policy_version == self.policy_version;
        }

        if !has_current_policy_version {
            return Err(
                ScaleConsistencyThresholdPolicyError::MissingCurrentPolicyVersionInChangeLog {
                    policy_version: self.policy_version.clone(),
                },
            );
        }

        Ok(())
    }

    pub fn select_interval_threshold_band(
        &self,
        interval_delta: f64,
    ) -> Result<&ScaleConsistencyIntervalThresholdBand, ScaleConsistencyThresholdPolicyError> {
        if !interval_delta.is_finite() || interval_delta <= 0.0 {
            return Err(ScaleConsistencyThresholdPolicyError::InvalidIntervalDelta {
                value: interval_delta,
            });
        }

        for interval_band in &self.interval_threshold_bands {
            if interval_delta <= interval_band.delta_upper_bound {
                return Ok(interval_band);
            }
        }

        let maximum_supported_delta = self
            .interval_threshold_bands
            .last()
            .map_or(0.0, |interval_band| interval_band.delta_upper_bound);
        Err(
            ScaleConsistencyThresholdPolicyError::DeltaOutsideThresholdBands {
                interval_delta,
                maximum_supported_delta,
            },
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ScaleConsistencyThresholdPolicyError {
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
    EmptyIntervalThresholdBands,
    InvalidIntervalThresholdBand {
        position: usize,
        source: ScaleConsistencyIntervalThresholdBandError,
    },
    NonIncreasingDeltaUpperBound {
        previous: f64,
        current: f64,
    },
    DuplicateThresholdId {
        threshold_id: String,
    },
    EmptyChangeLog,
    InvalidChangeLogEntry {
        position: usize,
        source: ScaleConsistencyThresholdPolicyChangeLogEntryError,
    },
    DuplicateChangeLogPolicyVersion {
        policy_version: String,
    },
    MissingCurrentPolicyVersionInChangeLog {
        policy_version: String,
    },
    InvalidIntervalDelta {
        value: f64,
    },
    DeltaOutsideThresholdBands {
        interval_delta: f64,
        maximum_supported_delta: f64,
    },
}

impl Display for ScaleConsistencyThresholdPolicyError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidSchemaVersion { expected, actual } => write!(
                formatter,
                "threshold_policy.schema_version must be `{expected}`, found `{actual}`"
            ),
            Self::EmptyPolicyId => {
                write!(formatter, "threshold_policy.policy_id must be non-empty")
            }
            Self::EmptyPolicyVersion => {
                write!(
                    formatter,
                    "threshold_policy.policy_version must be non-empty"
                )
            }
            Self::InvalidGovernanceScopePolicy { expected, actual } => write!(
                formatter,
                "threshold_policy.governance_scope_policy must be `{expected}`, found `{actual}`"
            ),
            Self::EmptyIntervalThresholdBands => write!(
                formatter,
                "threshold_policy.interval_threshold_bands must contain at least one interval band"
            ),
            Self::InvalidIntervalThresholdBand { position, source } => write!(
                formatter,
                "threshold_policy.interval_threshold_bands[{position}] is invalid: {source}"
            ),
            Self::NonIncreasingDeltaUpperBound { previous, current } => write!(
                formatter,
                "threshold_policy interval_threshold_bands delta_upper_bound values must be strictly increasing, found previous={previous}, current={current}"
            ),
            Self::DuplicateThresholdId { threshold_id } => write!(
                formatter,
                "threshold_policy interval_threshold_bands contain duplicate threshold_id `{threshold_id}`"
            ),
            Self::EmptyChangeLog => write!(
                formatter,
                "threshold_policy.change_log must include at least one policy history entry"
            ),
            Self::InvalidChangeLogEntry { position, source } => write!(
                formatter,
                "threshold_policy.change_log[{position}] is invalid: {source}"
            ),
            Self::DuplicateChangeLogPolicyVersion { policy_version } => write!(
                formatter,
                "threshold_policy.change_log contains duplicate policy_version: {policy_version}"
            ),
            Self::MissingCurrentPolicyVersionInChangeLog { policy_version } => write!(
                formatter,
                "threshold_policy.change_log must include an entry for policy_version '{policy_version}'"
            ),
            Self::InvalidIntervalDelta { value } => write!(
                formatter,
                "interval_delta must be finite and strictly positive, found {value}"
            ),
            Self::DeltaOutsideThresholdBands {
                interval_delta,
                maximum_supported_delta,
            } => write!(
                formatter,
                "interval_delta {interval_delta} is outside configured threshold bands, maximum supported interval_delta is {maximum_supported_delta}"
            ),
        }
    }
}

impl Error for ScaleConsistencyThresholdPolicyError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidIntervalThresholdBand { source, .. } => Some(source),
            Self::InvalidChangeLogEntry { source, .. } => Some(source),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ScaleConsistencyThresholdPolicyLoadError {
    InvalidJson {
        source: String,
    },
    InvalidThresholdPolicy {
        source: ScaleConsistencyThresholdPolicyError,
    },
}

impl Display for ScaleConsistencyThresholdPolicyLoadError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidJson { source } => write!(
                formatter,
                "scale consistency threshold policy payload must be valid JSON: {source}"
            ),
            Self::InvalidThresholdPolicy { source } => write!(
                formatter,
                "scale consistency threshold policy payload is invalid: {source}"
            ),
        }
    }
}

impl Error for ScaleConsistencyThresholdPolicyLoadError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidThresholdPolicy { source } => Some(source),
            _ => None,
        }
    }
}

pub fn load_scale_consistency_threshold_policy_from_json(
    threshold_policy_payload: &str,
) -> Result<ScaleConsistencyThresholdPolicy, ScaleConsistencyThresholdPolicyLoadError> {
    let threshold_policy: ScaleConsistencyThresholdPolicy =
        serde_json::from_str(threshold_policy_payload).map_err(|source| {
            ScaleConsistencyThresholdPolicyLoadError::InvalidJson {
                source: source.to_string(),
            }
        })?;
    threshold_policy.validate().map_err(|source| {
        ScaleConsistencyThresholdPolicyLoadError::InvalidThresholdPolicy { source }
    })?;

    Ok(threshold_policy)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScaleConsistencyEngineBenchmarkInput {
    pub source_report_schema_version: String,
    pub source_governance_scope_policy: String,
    pub benchmark_policy_id: String,
    pub benchmark_policy_version: String,
    pub tolerance_policy_id: String,
    pub tolerance_policy_version: String,
    pub status: RegressionCaseStatus,
}

impl ScaleConsistencyEngineBenchmarkInput {
    pub fn from_coarse_fine_consistency_report(
        report: &CoarseFineConsistencyRegressionReport,
    ) -> Self {
        let status = if report.summary.failed_scenario_count == 0 {
            RegressionCaseStatus::Pass
        } else {
            RegressionCaseStatus::Fail
        };

        Self {
            source_report_schema_version: report.schema_version.to_string(),
            source_governance_scope_policy: report.governance_scope_policy.to_string(),
            benchmark_policy_id: report.benchmark_policy_id.clone(),
            benchmark_policy_version: report.benchmark_policy_version.clone(),
            tolerance_policy_id: report.tolerance_config.policy_id.clone(),
            tolerance_policy_version: report.tolerance_config.policy_version.clone(),
            status,
        }
    }

    pub fn validate(&self) -> Result<(), ScaleConsistencyEngineBenchmarkInputError> {
        if self.source_report_schema_version
            != COARSE_FINE_CONSISTENCY_REGRESSION_REPORT_SCHEMA_VERSION
        {
            return Err(
                ScaleConsistencyEngineBenchmarkInputError::InvalidSourceReportSchemaVersion {
                    expected: COARSE_FINE_CONSISTENCY_REGRESSION_REPORT_SCHEMA_VERSION,
                    actual: self.source_report_schema_version.clone(),
                },
            );
        }
        if self.source_governance_scope_policy
            != COARSE_FINE_CONSISTENCY_REGRESSION_GOVERNANCE_SCOPE_POLICY
        {
            return Err(
                ScaleConsistencyEngineBenchmarkInputError::InvalidSourceGovernanceScopePolicy {
                    expected: COARSE_FINE_CONSISTENCY_REGRESSION_GOVERNANCE_SCOPE_POLICY,
                    actual: self.source_governance_scope_policy.clone(),
                },
            );
        }

        if self.benchmark_policy_id.trim().is_empty() {
            return Err(ScaleConsistencyEngineBenchmarkInputError::EmptyBenchmarkPolicyId);
        }
        if self.benchmark_policy_version.trim().is_empty() {
            return Err(ScaleConsistencyEngineBenchmarkInputError::EmptyBenchmarkPolicyVersion);
        }
        if self.tolerance_policy_id.trim().is_empty() {
            return Err(ScaleConsistencyEngineBenchmarkInputError::EmptyTolerancePolicyId);
        }
        if self.tolerance_policy_version.trim().is_empty() {
            return Err(ScaleConsistencyEngineBenchmarkInputError::EmptyTolerancePolicyVersion);
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ScaleConsistencyEngineBenchmarkInputError {
    InvalidSourceReportSchemaVersion {
        expected: &'static str,
        actual: String,
    },
    InvalidSourceGovernanceScopePolicy {
        expected: &'static str,
        actual: String,
    },
    EmptyBenchmarkPolicyId,
    EmptyBenchmarkPolicyVersion,
    EmptyTolerancePolicyId,
    EmptyTolerancePolicyVersion,
}

impl Display for ScaleConsistencyEngineBenchmarkInputError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidSourceReportSchemaVersion { expected, actual } => write!(
                formatter,
                "engine_benchmark_input.source_report_schema_version must be `{expected}`, found `{actual}`"
            ),
            Self::InvalidSourceGovernanceScopePolicy { expected, actual } => write!(
                formatter,
                "engine_benchmark_input.source_governance_scope_policy must be `{expected}`, found `{actual}`"
            ),
            Self::EmptyBenchmarkPolicyId => write!(
                formatter,
                "engine_benchmark_input.benchmark_policy_id must be non-empty"
            ),
            Self::EmptyBenchmarkPolicyVersion => write!(
                formatter,
                "engine_benchmark_input.benchmark_policy_version must be non-empty"
            ),
            Self::EmptyTolerancePolicyId => write!(
                formatter,
                "engine_benchmark_input.tolerance_policy_id must be non-empty"
            ),
            Self::EmptyTolerancePolicyVersion => write!(
                formatter,
                "engine_benchmark_input.tolerance_policy_version must be non-empty"
            ),
        }
    }
}

impl Error for ScaleConsistencyEngineBenchmarkInputError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScaleConsistencyDecisionReasonCode {
    AbsoluteDeltaThresholdExceeded,
    MetricComputationMissing,
    EngineBenchmarkPrerequisiteFailed,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ScaleConsistencyDecisionThresholdContext {
    pub policy_id: String,
    pub policy_version: String,
    pub interval_threshold_band_label: String,
    pub threshold_id: String,
    pub threshold_rationale: String,
    pub max_absolute_delta: f64,
    pub absolute_delta_exceeds_threshold: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ScaleConsistencyMetricDecision {
    pub metric_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metric_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metric_variant: Option<String>,
    pub metric_category: ScaleConsistencyMetricCategory,
    pub delta_kind: ScaleConsistencyMetricDeltaKind,
    pub metric_computation_status: ScaleConsistencyMetricComputationStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coarse_value: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fine_value: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delta_value: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub absolute_delta: Option<f64>,
    pub computation_failures: Vec<ScaleConsistencyMetricComputationFailure>,
    pub status: RegressionCaseStatus,
    pub threshold_context: ScaleConsistencyDecisionThresholdContext,
    pub decision_reason_codes: Vec<ScaleConsistencyDecisionReasonCode>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ScaleConsistencyDecisionScenarioReport {
    pub scenario_id: String,
    pub parameter_set_id: String,
    pub horizon_label: String,
    pub coarse_interval_delta: f64,
    pub fine_interval_delta: f64,
    pub applied_interval_threshold_band_label: String,
    pub status: RegressionCaseStatus,
    pub failed_metric_count: usize,
    pub metric_decisions: Vec<ScaleConsistencyMetricDecision>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ScaleConsistencyDecisionSummary {
    pub total_scenario_count: usize,
    pub passed_scenario_count: usize,
    pub failed_scenario_count: usize,
    pub total_metric_count: usize,
    pub failed_metric_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ScaleConsistencyDecisionBatchReport {
    pub schema_version: &'static str,
    pub ordering_policy: &'static str,
    pub decision_policy: &'static str,
    pub governance_scope_policy: &'static str,
    pub benchmark_input_policy: &'static str,
    pub threshold_policy_id: String,
    pub threshold_policy_version: String,
    pub engine_benchmark_input: ScaleConsistencyEngineBenchmarkInput,
    pub status: RegressionCaseStatus,
    pub summary: ScaleConsistencyDecisionSummary,
    pub scenario_reports: Vec<ScaleConsistencyDecisionScenarioReport>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ScaleConsistencyDecisionEngineError {
    InvalidThresholdPolicy {
        source: ScaleConsistencyThresholdPolicyError,
    },
    InvalidEngineBenchmarkInput {
        source: ScaleConsistencyEngineBenchmarkInputError,
    },
    InvalidMetricDeltaReportSchemaVersion {
        expected: &'static str,
        actual: &'static str,
    },
    InvalidMetricDeltaReportOrderingPolicy {
        expected: &'static str,
        actual: &'static str,
    },
    MissingMetricThreshold {
        interval_threshold_band_label: String,
        metric_id: String,
        metric_variant: Option<String>,
    },
}

impl Display for ScaleConsistencyDecisionEngineError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidThresholdPolicy { source } => write!(
                formatter,
                "scale consistency decision engine requires a valid threshold policy: {source}"
            ),
            Self::InvalidEngineBenchmarkInput { source } => write!(
                formatter,
                "scale consistency decision engine requires a valid engine benchmark input: {source}"
            ),
            Self::InvalidMetricDeltaReportSchemaVersion { expected, actual } => write!(
                formatter,
                "metric_delta_report.schema_version must be `{expected}`, found `{actual}`"
            ),
            Self::InvalidMetricDeltaReportOrderingPolicy { expected, actual } => write!(
                formatter,
                "metric_delta_report.ordering_policy must be `{expected}`, found `{actual}`"
            ),
            Self::MissingMetricThreshold {
                interval_threshold_band_label,
                metric_id,
                metric_variant,
            } => write!(
                formatter,
                "interval threshold band `{interval_threshold_band_label}` does not define a threshold for metric_id `{metric_id}` and metric_variant {metric_variant:?}"
            ),
        }
    }
}

impl Error for ScaleConsistencyDecisionEngineError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidThresholdPolicy { source } => Some(source),
            Self::InvalidEngineBenchmarkInput { source } => Some(source),
            _ => None,
        }
    }
}

pub fn run_scale_consistency_threshold_decision_engine(
    metric_delta_report: &ScaleConsistencyMetricDeltaBatchReport,
    threshold_policy: &ScaleConsistencyThresholdPolicy,
    engine_benchmark_input: &ScaleConsistencyEngineBenchmarkInput,
) -> Result<ScaleConsistencyDecisionBatchReport, ScaleConsistencyDecisionEngineError> {
    threshold_policy
        .validate()
        .map_err(|source| ScaleConsistencyDecisionEngineError::InvalidThresholdPolicy { source })?;

    engine_benchmark_input.validate().map_err(|source| {
        ScaleConsistencyDecisionEngineError::InvalidEngineBenchmarkInput { source }
    })?;

    if metric_delta_report.schema_version
        != SCALE_CONSISTENCY_METRIC_DELTA_BATCH_REPORT_SCHEMA_VERSION
    {
        return Err(
            ScaleConsistencyDecisionEngineError::InvalidMetricDeltaReportSchemaVersion {
                expected: SCALE_CONSISTENCY_METRIC_DELTA_BATCH_REPORT_SCHEMA_VERSION,
                actual: metric_delta_report.schema_version,
            },
        );
    }

    if metric_delta_report.ordering_policy != SCALE_CONSISTENCY_METRIC_DELTA_ORDERING_POLICY {
        return Err(
            ScaleConsistencyDecisionEngineError::InvalidMetricDeltaReportOrderingPolicy {
                expected: SCALE_CONSISTENCY_METRIC_DELTA_ORDERING_POLICY,
                actual: metric_delta_report.ordering_policy,
            },
        );
    }

    let engine_benchmark_failed = engine_benchmark_input.status == RegressionCaseStatus::Fail;
    let scenario_reports = metric_delta_report
        .scenario_reports
        .iter()
        .map(|scenario_report| {
            build_scenario_decision_report(
                scenario_report,
                threshold_policy,
                engine_benchmark_failed,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;

    let total_scenario_count = scenario_reports.len();
    let passed_scenario_count = scenario_reports
        .iter()
        .filter(|scenario_report| scenario_report.status == RegressionCaseStatus::Pass)
        .count();
    let failed_scenario_count = total_scenario_count - passed_scenario_count;
    let total_metric_count = scenario_reports
        .iter()
        .map(|scenario_report| scenario_report.metric_decisions.len())
        .sum();
    let failed_metric_count = scenario_reports
        .iter()
        .map(|scenario_report| scenario_report.failed_metric_count)
        .sum();

    let status = if failed_scenario_count == 0 {
        RegressionCaseStatus::Pass
    } else {
        RegressionCaseStatus::Fail
    };

    Ok(ScaleConsistencyDecisionBatchReport {
        schema_version: SCALE_CONSISTENCY_DECISION_REPORT_SCHEMA_VERSION,
        ordering_policy: SCALE_CONSISTENCY_DECISION_ORDERING_POLICY,
        decision_policy: SCALE_CONSISTENCY_DECISION_POLICY,
        governance_scope_policy: SCALE_CONSISTENCY_THRESHOLD_POLICY_GOVERNANCE_SCOPE_POLICY,
        benchmark_input_policy: SCALE_CONSISTENCY_ENGINE_BENCHMARK_INPUT_POLICY,
        threshold_policy_id: threshold_policy.policy_id.clone(),
        threshold_policy_version: threshold_policy.policy_version.clone(),
        engine_benchmark_input: engine_benchmark_input.clone(),
        status,
        summary: ScaleConsistencyDecisionSummary {
            total_scenario_count,
            passed_scenario_count,
            failed_scenario_count,
            total_metric_count,
            failed_metric_count,
        },
        scenario_reports,
    })
}

fn build_scenario_decision_report(
    scenario_report: &ScaleConsistencyMetricDeltaScenarioReport,
    threshold_policy: &ScaleConsistencyThresholdPolicy,
    engine_benchmark_failed: bool,
) -> Result<ScaleConsistencyDecisionScenarioReport, ScaleConsistencyDecisionEngineError> {
    let interval_threshold_band = threshold_policy
        .select_interval_threshold_band(scenario_report.coarse_interval_delta)
        .map_err(|source| ScaleConsistencyDecisionEngineError::InvalidThresholdPolicy { source })?;

    let metric_decisions = scenario_report
        .metric_deltas
        .iter()
        .map(|metric_delta| {
            build_metric_decision(
                metric_delta,
                threshold_policy,
                interval_threshold_band,
                engine_benchmark_failed,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;

    let failed_metric_count = metric_decisions
        .iter()
        .filter(|metric_decision| metric_decision.status == RegressionCaseStatus::Fail)
        .count();
    let status = if failed_metric_count == 0 {
        RegressionCaseStatus::Pass
    } else {
        RegressionCaseStatus::Fail
    };

    Ok(ScaleConsistencyDecisionScenarioReport {
        scenario_id: scenario_report.scenario_id.clone(),
        parameter_set_id: scenario_report.parameter_set_id.clone(),
        horizon_label: scenario_report.horizon_label.clone(),
        coarse_interval_delta: scenario_report.coarse_interval_delta,
        fine_interval_delta: scenario_report.fine_interval_delta,
        applied_interval_threshold_band_label: interval_threshold_band.interval_grid_label.clone(),
        status,
        failed_metric_count,
        metric_decisions,
    })
}

fn build_metric_decision(
    metric_delta: &ScaleConsistencyMetricDelta,
    threshold_policy: &ScaleConsistencyThresholdPolicy,
    interval_threshold_band: &ScaleConsistencyIntervalThresholdBand,
    engine_benchmark_failed: bool,
) -> Result<ScaleConsistencyMetricDecision, ScaleConsistencyDecisionEngineError> {
    let threshold = interval_threshold_band
        .resolve_threshold(
            metric_delta.metric_id.as_str(),
            metric_delta.metric_variant.as_deref(),
        )
        .ok_or_else(
            || ScaleConsistencyDecisionEngineError::MissingMetricThreshold {
                interval_threshold_band_label: interval_threshold_band.interval_grid_label.clone(),
                metric_id: metric_delta.metric_id.clone(),
                metric_variant: metric_delta.metric_variant.clone(),
            },
        )?;

    let absolute_delta_exceeds_threshold = metric_delta
        .absolute_delta
        .map(|absolute_delta| absolute_delta > threshold.max_absolute_delta);
    let mut decision_reason_codes: Vec<ScaleConsistencyDecisionReasonCode> = Vec::new();

    if engine_benchmark_failed {
        decision_reason_codes
            .push(ScaleConsistencyDecisionReasonCode::EngineBenchmarkPrerequisiteFailed);
    }

    if metric_delta.status == ScaleConsistencyMetricComputationStatus::Missing
        || metric_delta.absolute_delta.is_none()
    {
        decision_reason_codes.push(ScaleConsistencyDecisionReasonCode::MetricComputationMissing);
    }

    if absolute_delta_exceeds_threshold == Some(true) {
        decision_reason_codes
            .push(ScaleConsistencyDecisionReasonCode::AbsoluteDeltaThresholdExceeded);
    }

    let status = if decision_reason_codes.is_empty() {
        RegressionCaseStatus::Pass
    } else {
        RegressionCaseStatus::Fail
    };

    Ok(ScaleConsistencyMetricDecision {
        metric_id: metric_delta.metric_id.clone(),
        metric_name: metric_delta.metric_name.clone(),
        metric_variant: metric_delta.metric_variant.clone(),
        metric_category: metric_delta.metric_category,
        delta_kind: metric_delta.delta_kind,
        metric_computation_status: metric_delta.status,
        coarse_value: metric_delta.coarse_value,
        fine_value: metric_delta.fine_value,
        delta_value: metric_delta.delta_value,
        absolute_delta: metric_delta.absolute_delta,
        computation_failures: metric_delta.computation_failures.clone(),
        status,
        threshold_context: ScaleConsistencyDecisionThresholdContext {
            policy_id: threshold_policy.policy_id.clone(),
            policy_version: threshold_policy.policy_version.clone(),
            interval_threshold_band_label: interval_threshold_band.interval_grid_label.clone(),
            threshold_id: threshold.threshold_id.clone(),
            threshold_rationale: threshold.rationale.clone(),
            max_absolute_delta: threshold.max_absolute_delta,
            absolute_delta_exceeds_threshold,
        },
        decision_reason_codes,
    })
}
