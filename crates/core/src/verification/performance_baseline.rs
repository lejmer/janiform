//! Oracle-pricer-risk performance baseline capture, serialization, and comparisons.

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};

use crate::benchmarking::{
    BenchmarkWorkloadScale, ORACLE_PRICER_RISK_BENCHMARK_DETERMINISM_POLICY,
    ORACLE_PRICER_RISK_BENCHMARK_EXECUTION_POLICY, ORACLE_PRICER_RISK_BENCHMARK_METRIC_POLICY,
    ORACLE_PRICER_RISK_BENCHMARK_REPORT_SCHEMA_VERSION, ORACLE_PRICER_RISK_COMPONENT_ORACLE,
    ORACLE_PRICER_RISK_COMPONENT_PRICING, ORACLE_PRICER_RISK_COMPONENT_RISK,
    OraclePricerRiskBenchmarkComponentMetrics, OraclePricerRiskBenchmarkReport,
};

pub const ORACLE_PRICER_RISK_PERFORMANCE_BASELINE_SCHEMA_VERSION: &str =
    "benchmark.oracle_pricer_risk.performance_baseline.v1";
pub const ORACLE_PRICER_RISK_PERFORMANCE_BASELINE_SCHEMA_FAMILY: &str =
    "benchmark.oracle_pricer_risk.performance_baseline";
pub const ORACLE_PRICER_RISK_PERFORMANCE_BASELINE_SCHEMA_GENERATION: &str = "v0.x";
pub const ORACLE_PRICER_RISK_PERFORMANCE_BASELINE_FORWARD_EXTENSION_POLICY: &str =
    "additive_fields_only.v1";
pub const ORACLE_PRICER_RISK_PERFORMANCE_BASELINE_CAPTURE_POLICY: &str =
    "extract_component_latency_throughput_and_operation_counts_from_benchmark_report.v1";
pub const ORACLE_PRICER_RISK_PERFORMANCE_BASELINE_OWNERSHIP_POLICY: &str =
    "initiative_004_project_004_maintainers.v1";
pub const ORACLE_PRICER_RISK_PERFORMANCE_BASELINE_OUTPUT_SCHEMA_METADATA_VERSION: &str =
    "benchmark.oracle_pricer_risk.performance_baseline.output_schema_metadata.v1";
pub const ORACLE_PRICER_RISK_PERFORMANCE_BASELINE_COMPARISON_REPORT_SCHEMA_VERSION: &str =
    "benchmark.oracle_pricer_risk.performance_baseline_comparison.v1";
pub const ORACLE_PRICER_RISK_PERFORMANCE_BASELINE_COMPARISON_POLICY: &str =
    "candidate_minus_reference_component_deltas_with_epsilon_guarded_relative_changes.v1";

const ORACLE_PRICER_RISK_PERFORMANCE_BASELINE_REQUIRED_FIELDS: [&str; 10] = [
    "schema_version",
    "baseline_id",
    "baseline_version",
    "schema_metadata",
    "ownership_metadata",
    "capture_policy",
    "output_schema_metadata",
    "source_report",
    "summary",
    "profile_records",
];
const ORACLE_PRICER_RISK_PERFORMANCE_BASELINE_REQUIRED_PROFILE_RECORD_FIELDS: [&str; 6] = [
    "profile_id",
    "workload_scale",
    "interval_delta",
    "oracle_solver_metrics",
    "pricing_metrics",
    "risk_sampling_metrics",
];
const ORACLE_PRICER_RISK_PERFORMANCE_BASELINE_REQUIRED_COMPONENT_METRIC_FIELDS: [&str; 3] = [
    "mean_latency_seconds",
    "throughput_operations_per_second",
    "measured_operation_count",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OraclePricerRiskPerformanceBaselineSchemaMetadata {
    pub schema_family: String,
    pub schema_generation: String,
    pub forward_extension_policy: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OraclePricerRiskPerformanceBaselineOwnershipMetadata {
    pub ownership_policy: String,
    pub owner_team: String,
    pub owner_oncall: String,
    pub refresh_process: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OraclePricerRiskPerformanceBaselineOutputSchemaMetadata {
    pub schema_version: String,
    pub required_baseline_fields: Vec<String>,
    pub required_profile_record_fields: Vec<String>,
    pub required_component_metric_fields: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OraclePricerRiskPerformanceBaselineSourceReportMetadata {
    pub benchmark_report_schema_version: String,
    pub determinism_policy: String,
    pub execution_policy: String,
    pub metric_policy: String,
    pub catalog_id: String,
    pub catalog_version: String,
    pub policy_id: String,
    pub policy_version: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OraclePricerRiskPerformanceBaselineComponentMetrics {
    pub mean_latency_seconds: f64,
    pub throughput_operations_per_second: f64,
    pub measured_operation_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OraclePricerRiskPerformanceBaselineProfileRecord {
    pub profile_id: String,
    pub workload_scale: BenchmarkWorkloadScale,
    pub interval_delta: f64,
    pub oracle_solver_metrics: OraclePricerRiskPerformanceBaselineComponentMetrics,
    pub pricing_metrics: OraclePricerRiskPerformanceBaselineComponentMetrics,
    pub risk_sampling_metrics: OraclePricerRiskPerformanceBaselineComponentMetrics,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OraclePricerRiskPerformanceBaselineSummary {
    pub profile_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OraclePricerRiskPerformanceBaselineArtifact {
    pub schema_version: String,
    pub baseline_id: String,
    pub baseline_version: String,
    pub schema_metadata: OraclePricerRiskPerformanceBaselineSchemaMetadata,
    pub ownership_metadata: OraclePricerRiskPerformanceBaselineOwnershipMetadata,
    pub capture_policy: String,
    pub output_schema_metadata: OraclePricerRiskPerformanceBaselineOutputSchemaMetadata,
    pub source_report: OraclePricerRiskPerformanceBaselineSourceReportMetadata,
    pub summary: OraclePricerRiskPerformanceBaselineSummary,
    pub profile_records: Vec<OraclePricerRiskPerformanceBaselineProfileRecord>,
}

impl OraclePricerRiskPerformanceBaselineArtifact {
    pub fn validate(&self) -> Result<(), OraclePricerRiskPerformanceBaselineValidationError> {
        if self.schema_version != ORACLE_PRICER_RISK_PERFORMANCE_BASELINE_SCHEMA_VERSION {
            return Err(
                OraclePricerRiskPerformanceBaselineValidationError::UnsupportedSchemaVersion {
                    expected: ORACLE_PRICER_RISK_PERFORMANCE_BASELINE_SCHEMA_VERSION,
                    actual: self.schema_version.clone(),
                },
            );
        }
        if self.baseline_id.trim().is_empty() {
            return Err(OraclePricerRiskPerformanceBaselineValidationError::EmptyBaselineId);
        }
        if self.baseline_version.trim().is_empty() {
            return Err(OraclePricerRiskPerformanceBaselineValidationError::EmptyBaselineVersion);
        }

        validate_baseline_metadata_field(
            "schema_metadata.schema_family",
            ORACLE_PRICER_RISK_PERFORMANCE_BASELINE_SCHEMA_FAMILY,
            &self.schema_metadata.schema_family,
        )?;
        validate_baseline_metadata_field(
            "schema_metadata.schema_generation",
            ORACLE_PRICER_RISK_PERFORMANCE_BASELINE_SCHEMA_GENERATION,
            &self.schema_metadata.schema_generation,
        )?;
        validate_baseline_metadata_field(
            "schema_metadata.forward_extension_policy",
            ORACLE_PRICER_RISK_PERFORMANCE_BASELINE_FORWARD_EXTENSION_POLICY,
            &self.schema_metadata.forward_extension_policy,
        )?;
        validate_baseline_metadata_field(
            "ownership_metadata.ownership_policy",
            ORACLE_PRICER_RISK_PERFORMANCE_BASELINE_OWNERSHIP_POLICY,
            &self.ownership_metadata.ownership_policy,
        )?;
        validate_baseline_metadata_field(
            "capture_policy",
            ORACLE_PRICER_RISK_PERFORMANCE_BASELINE_CAPTURE_POLICY,
            &self.capture_policy,
        )?;
        validate_baseline_metadata_field(
            "source_report.benchmark_report_schema_version",
            ORACLE_PRICER_RISK_BENCHMARK_REPORT_SCHEMA_VERSION,
            &self.source_report.benchmark_report_schema_version,
        )?;
        validate_baseline_metadata_field(
            "source_report.determinism_policy",
            ORACLE_PRICER_RISK_BENCHMARK_DETERMINISM_POLICY,
            &self.source_report.determinism_policy,
        )?;
        validate_baseline_metadata_field(
            "source_report.execution_policy",
            ORACLE_PRICER_RISK_BENCHMARK_EXECUTION_POLICY,
            &self.source_report.execution_policy,
        )?;
        validate_baseline_metadata_field(
            "source_report.metric_policy",
            ORACLE_PRICER_RISK_BENCHMARK_METRIC_POLICY,
            &self.source_report.metric_policy,
        )?;
        validate_baseline_metadata_field(
            "output_schema_metadata.schema_version",
            ORACLE_PRICER_RISK_PERFORMANCE_BASELINE_OUTPUT_SCHEMA_METADATA_VERSION,
            &self.output_schema_metadata.schema_version,
        )?;

        validate_non_empty_baseline_field(
            "ownership_metadata.owner_team",
            &self.ownership_metadata.owner_team,
        )?;
        validate_non_empty_baseline_field(
            "ownership_metadata.owner_oncall",
            &self.ownership_metadata.owner_oncall,
        )?;
        validate_non_empty_baseline_field(
            "ownership_metadata.refresh_process",
            &self.ownership_metadata.refresh_process,
        )?;
        validate_non_empty_baseline_field(
            "source_report.catalog_id",
            &self.source_report.catalog_id,
        )?;
        validate_non_empty_baseline_field(
            "source_report.catalog_version",
            &self.source_report.catalog_version,
        )?;
        validate_non_empty_baseline_field(
            "source_report.policy_id",
            &self.source_report.policy_id,
        )?;
        validate_non_empty_baseline_field(
            "source_report.policy_version",
            &self.source_report.policy_version,
        )?;

        validate_required_field_list(
            "output_schema_metadata.required_baseline_fields",
            &self.output_schema_metadata.required_baseline_fields,
            &ORACLE_PRICER_RISK_PERFORMANCE_BASELINE_REQUIRED_FIELDS,
        )?;
        validate_required_field_list(
            "output_schema_metadata.required_profile_record_fields",
            &self.output_schema_metadata.required_profile_record_fields,
            &ORACLE_PRICER_RISK_PERFORMANCE_BASELINE_REQUIRED_PROFILE_RECORD_FIELDS,
        )?;
        validate_required_field_list(
            "output_schema_metadata.required_component_metric_fields",
            &self.output_schema_metadata.required_component_metric_fields,
            &ORACLE_PRICER_RISK_PERFORMANCE_BASELINE_REQUIRED_COMPONENT_METRIC_FIELDS,
        )?;

        if self.profile_records.is_empty() {
            return Err(OraclePricerRiskPerformanceBaselineValidationError::EmptyProfileRecords);
        }
        if self.summary.profile_count != self.profile_records.len() {
            return Err(
                OraclePricerRiskPerformanceBaselineValidationError::InvalidSummaryProfileCount {
                    summary_profile_count: self.summary.profile_count,
                    profile_record_count: self.profile_records.len(),
                },
            );
        }

        let mut observed_profile_ids: BTreeSet<String> = BTreeSet::new();
        let mut observed_scales: BTreeSet<BenchmarkWorkloadScale> = BTreeSet::new();
        for profile_record in &self.profile_records {
            if profile_record.profile_id.trim().is_empty() {
                return Err(
                    OraclePricerRiskPerformanceBaselineValidationError::EmptyProfileRecordId,
                );
            }
            if !observed_profile_ids.insert(profile_record.profile_id.clone()) {
                return Err(
                    OraclePricerRiskPerformanceBaselineValidationError::DuplicateProfileRecordId {
                        profile_id: profile_record.profile_id.clone(),
                    },
                );
            }
            if !profile_record.interval_delta.is_finite() || profile_record.interval_delta <= 0.0 {
                return Err(
                    OraclePricerRiskPerformanceBaselineValidationError::InvalidIntervalDelta {
                        profile_id: profile_record.profile_id.clone(),
                        value: profile_record.interval_delta,
                    },
                );
            }
            observed_scales.insert(profile_record.workload_scale);
            validate_component_metrics_for_baseline(
                &profile_record.profile_id,
                ORACLE_PRICER_RISK_COMPONENT_ORACLE,
                profile_record.oracle_solver_metrics,
            )?;
            validate_component_metrics_for_baseline(
                &profile_record.profile_id,
                ORACLE_PRICER_RISK_COMPONENT_PRICING,
                profile_record.pricing_metrics,
            )?;
            validate_component_metrics_for_baseline(
                &profile_record.profile_id,
                ORACLE_PRICER_RISK_COMPONENT_RISK,
                profile_record.risk_sampling_metrics,
            )?;
        }

        let missing_scales: Vec<BenchmarkWorkloadScale> = BenchmarkWorkloadScale::required_order()
            .into_iter()
            .filter(|scale| !observed_scales.contains(scale))
            .collect();
        if !missing_scales.is_empty() {
            return Err(
                OraclePricerRiskPerformanceBaselineValidationError::MissingWorkloadScaleCoverage {
                    missing_scales,
                },
            );
        }

        Ok(())
    }

    fn sort_profile_records_canonically(&mut self) {
        self.profile_records.sort_by(|left, right| {
            left.workload_scale
                .ordering_rank()
                .cmp(&right.workload_scale.ordering_rank())
                .then_with(|| left.profile_id.cmp(&right.profile_id))
        });
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum OraclePricerRiskPerformanceBaselineValidationError {
    UnsupportedSchemaVersion {
        expected: &'static str,
        actual: String,
    },
    InvalidMetadataField {
        field_name: &'static str,
        expected: &'static str,
        actual: String,
    },
    EmptyBaselineId,
    EmptyBaselineVersion,
    EmptyField {
        field_name: &'static str,
    },
    InvalidRequiredFieldList {
        field_name: &'static str,
        expected: Vec<String>,
        actual: Vec<String>,
    },
    EmptyProfileRecords,
    InvalidSummaryProfileCount {
        summary_profile_count: usize,
        profile_record_count: usize,
    },
    EmptyProfileRecordId,
    DuplicateProfileRecordId {
        profile_id: String,
    },
    InvalidComponentMetricValue {
        profile_id: String,
        component_id: &'static str,
        field_name: &'static str,
        value: f64,
    },
    InvalidComponentOperationCount {
        profile_id: String,
        component_id: &'static str,
        value: usize,
    },
    InvalidIntervalDelta {
        profile_id: String,
        value: f64,
    },
    MissingWorkloadScaleCoverage {
        missing_scales: Vec<BenchmarkWorkloadScale>,
    },
}

impl Display for OraclePricerRiskPerformanceBaselineValidationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedSchemaVersion { expected, actual } => write!(
                formatter,
                "schema_version must be `{expected}`, found `{actual}`"
            ),
            Self::InvalidMetadataField {
                field_name,
                expected,
                actual,
            } => write!(
                formatter,
                "{field_name} must be `{expected}`, found `{actual}`"
            ),
            Self::EmptyBaselineId => write!(formatter, "baseline_id must be non-empty"),
            Self::EmptyBaselineVersion => write!(formatter, "baseline_version must be non-empty"),
            Self::EmptyField { field_name } => {
                write!(formatter, "{field_name} must be non-empty")
            }
            Self::InvalidRequiredFieldList {
                field_name,
                expected,
                actual,
            } => write!(
                formatter,
                "{field_name} must exactly match the required field list {:?}, found {:?}",
                expected, actual
            ),
            Self::EmptyProfileRecords => {
                write!(
                    formatter,
                    "profile_records must contain at least one profile record"
                )
            }
            Self::InvalidSummaryProfileCount {
                summary_profile_count,
                profile_record_count,
            } => write!(
                formatter,
                "summary.profile_count must equal profile_records length, found summary.profile_count={summary_profile_count}, profile_records.len()={profile_record_count}"
            ),
            Self::EmptyProfileRecordId => {
                write!(formatter, "profile_records[].profile_id must be non-empty")
            }
            Self::DuplicateProfileRecordId { profile_id } => write!(
                formatter,
                "profile_records contains duplicate profile_id `{profile_id}`"
            ),
            Self::InvalidComponentMetricValue {
                profile_id,
                component_id,
                field_name,
                value,
            } => write!(
                formatter,
                "profile `{profile_id}` component `{component_id}` field `{field_name}` must be finite and non-negative, found {value}"
            ),
            Self::InvalidComponentOperationCount {
                profile_id,
                component_id,
                value,
            } => write!(
                formatter,
                "profile `{profile_id}` component `{component_id}` measured_operation_count must be greater than 0, found {value}"
            ),
            Self::InvalidIntervalDelta { profile_id, value } => write!(
                formatter,
                "profile `{profile_id}` interval_delta must be finite and strictly positive, found {value}"
            ),
            Self::MissingWorkloadScaleCoverage { missing_scales } => {
                let joined_scales = missing_scales
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<String>>()
                    .join(", ");
                write!(
                    formatter,
                    "profile_records must include all workload scales (missing: {joined_scales})"
                )
            }
        }
    }
}

impl Error for OraclePricerRiskPerformanceBaselineValidationError {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OraclePricerRiskPerformanceBaselineCaptureConfig {
    pub baseline_id: String,
    pub baseline_version: String,
    pub owner_team: String,
    pub owner_oncall: String,
    pub refresh_process: String,
}

impl OraclePricerRiskPerformanceBaselineCaptureConfig {
    pub fn validate(
        &self,
    ) -> Result<(), OraclePricerRiskPerformanceBaselineCaptureConfigValidationError> {
        if self.baseline_id.trim().is_empty() {
            return Err(
                OraclePricerRiskPerformanceBaselineCaptureConfigValidationError::EmptyField {
                    field_name: "baseline_id",
                },
            );
        }
        if self.baseline_version.trim().is_empty() {
            return Err(
                OraclePricerRiskPerformanceBaselineCaptureConfigValidationError::EmptyField {
                    field_name: "baseline_version",
                },
            );
        }
        if self.owner_team.trim().is_empty() {
            return Err(
                OraclePricerRiskPerformanceBaselineCaptureConfigValidationError::EmptyField {
                    field_name: "owner_team",
                },
            );
        }
        if self.owner_oncall.trim().is_empty() {
            return Err(
                OraclePricerRiskPerformanceBaselineCaptureConfigValidationError::EmptyField {
                    field_name: "owner_oncall",
                },
            );
        }
        if self.refresh_process.trim().is_empty() {
            return Err(
                OraclePricerRiskPerformanceBaselineCaptureConfigValidationError::EmptyField {
                    field_name: "refresh_process",
                },
            );
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum OraclePricerRiskPerformanceBaselineCaptureConfigValidationError {
    EmptyField { field_name: &'static str },
}

impl Display for OraclePricerRiskPerformanceBaselineCaptureConfigValidationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyField { field_name } => write!(formatter, "{field_name} must be non-empty"),
        }
    }
}

impl Error for OraclePricerRiskPerformanceBaselineCaptureConfigValidationError {}

#[derive(Debug, Clone, PartialEq)]
pub enum OraclePricerRiskPerformanceBaselineCaptureError {
    InvalidCaptureConfig {
        source: OraclePricerRiskPerformanceBaselineCaptureConfigValidationError,
    },
    InvalidReportField {
        field_name: &'static str,
        expected: &'static str,
        actual: String,
    },
    InvalidCapturedBaseline {
        source: OraclePricerRiskPerformanceBaselineValidationError,
    },
}

impl Display for OraclePricerRiskPerformanceBaselineCaptureError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidCaptureConfig { source } => {
                write!(formatter, "baseline capture config is invalid: {source}")
            }
            Self::InvalidReportField {
                field_name,
                expected,
                actual,
            } => write!(
                formatter,
                "benchmark report field `{field_name}` must be `{expected}`, found `{actual}`"
            ),
            Self::InvalidCapturedBaseline { source } => write!(
                formatter,
                "captured performance baseline artifact is invalid: {source}"
            ),
        }
    }
}

impl Error for OraclePricerRiskPerformanceBaselineCaptureError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidCaptureConfig { source } => Some(source),
            Self::InvalidCapturedBaseline { source } => Some(source),
            _ => None,
        }
    }
}

pub fn capture_oracle_pricer_risk_performance_baseline_artifact(
    benchmark_report: &OraclePricerRiskBenchmarkReport,
    capture_config: OraclePricerRiskPerformanceBaselineCaptureConfig,
) -> Result<
    OraclePricerRiskPerformanceBaselineArtifact,
    OraclePricerRiskPerformanceBaselineCaptureError,
> {
    capture_config.validate().map_err(|source| {
        OraclePricerRiskPerformanceBaselineCaptureError::InvalidCaptureConfig { source }
    })?;
    validate_report_field_for_baseline_capture(
        "schema_version",
        ORACLE_PRICER_RISK_BENCHMARK_REPORT_SCHEMA_VERSION,
        benchmark_report.schema_version,
    )?;
    validate_report_field_for_baseline_capture(
        "determinism_policy",
        ORACLE_PRICER_RISK_BENCHMARK_DETERMINISM_POLICY,
        benchmark_report.determinism_policy,
    )?;
    validate_report_field_for_baseline_capture(
        "execution_policy",
        ORACLE_PRICER_RISK_BENCHMARK_EXECUTION_POLICY,
        benchmark_report.execution_policy,
    )?;
    validate_report_field_for_baseline_capture(
        "metric_policy",
        ORACLE_PRICER_RISK_BENCHMARK_METRIC_POLICY,
        benchmark_report.metric_policy,
    )?;

    let mut profile_records: Vec<OraclePricerRiskPerformanceBaselineProfileRecord> =
        benchmark_report
            .profile_reports
            .iter()
            .map(
                |profile_report| OraclePricerRiskPerformanceBaselineProfileRecord {
                    profile_id: profile_report.profile_id.clone(),
                    workload_scale: profile_report.workload_scale,
                    interval_delta: profile_report.interval_delta,
                    oracle_solver_metrics: copy_component_metrics_for_baseline(
                        profile_report.oracle_solver_metrics,
                    ),
                    pricing_metrics: copy_component_metrics_for_baseline(
                        profile_report.pricing_metrics,
                    ),
                    risk_sampling_metrics: copy_component_metrics_for_baseline(
                        profile_report.risk_sampling_metrics,
                    ),
                },
            )
            .collect();

    profile_records.sort_by(|left, right| {
        left.workload_scale
            .ordering_rank()
            .cmp(&right.workload_scale.ordering_rank())
            .then_with(|| left.profile_id.cmp(&right.profile_id))
    });

    let mut baseline_artifact = OraclePricerRiskPerformanceBaselineArtifact {
        schema_version: ORACLE_PRICER_RISK_PERFORMANCE_BASELINE_SCHEMA_VERSION.to_string(),
        baseline_id: capture_config.baseline_id,
        baseline_version: capture_config.baseline_version,
        schema_metadata: OraclePricerRiskPerformanceBaselineSchemaMetadata {
            schema_family: ORACLE_PRICER_RISK_PERFORMANCE_BASELINE_SCHEMA_FAMILY.to_string(),
            schema_generation: ORACLE_PRICER_RISK_PERFORMANCE_BASELINE_SCHEMA_GENERATION
                .to_string(),
            forward_extension_policy:
                ORACLE_PRICER_RISK_PERFORMANCE_BASELINE_FORWARD_EXTENSION_POLICY.to_string(),
        },
        ownership_metadata: OraclePricerRiskPerformanceBaselineOwnershipMetadata {
            ownership_policy: ORACLE_PRICER_RISK_PERFORMANCE_BASELINE_OWNERSHIP_POLICY.to_string(),
            owner_team: capture_config.owner_team,
            owner_oncall: capture_config.owner_oncall,
            refresh_process: capture_config.refresh_process,
        },
        capture_policy: ORACLE_PRICER_RISK_PERFORMANCE_BASELINE_CAPTURE_POLICY.to_string(),
        output_schema_metadata: OraclePricerRiskPerformanceBaselineOutputSchemaMetadata {
            schema_version: ORACLE_PRICER_RISK_PERFORMANCE_BASELINE_OUTPUT_SCHEMA_METADATA_VERSION
                .to_string(),
            required_baseline_fields: required_field_list_to_owned(
                &ORACLE_PRICER_RISK_PERFORMANCE_BASELINE_REQUIRED_FIELDS,
            ),
            required_profile_record_fields: required_field_list_to_owned(
                &ORACLE_PRICER_RISK_PERFORMANCE_BASELINE_REQUIRED_PROFILE_RECORD_FIELDS,
            ),
            required_component_metric_fields: required_field_list_to_owned(
                &ORACLE_PRICER_RISK_PERFORMANCE_BASELINE_REQUIRED_COMPONENT_METRIC_FIELDS,
            ),
        },
        source_report: OraclePricerRiskPerformanceBaselineSourceReportMetadata {
            benchmark_report_schema_version: benchmark_report.schema_version.to_string(),
            determinism_policy: benchmark_report.determinism_policy.to_string(),
            execution_policy: benchmark_report.execution_policy.to_string(),
            metric_policy: benchmark_report.metric_policy.to_string(),
            catalog_id: benchmark_report.catalog_id.clone(),
            catalog_version: benchmark_report.catalog_version.clone(),
            policy_id: benchmark_report.policy_id.clone(),
            policy_version: benchmark_report.policy_version.clone(),
        },
        summary: OraclePricerRiskPerformanceBaselineSummary {
            profile_count: profile_records.len(),
        },
        profile_records,
    };
    baseline_artifact.sort_profile_records_canonically();
    baseline_artifact.validate().map_err(|source| {
        OraclePricerRiskPerformanceBaselineCaptureError::InvalidCapturedBaseline { source }
    })?;

    Ok(baseline_artifact)
}

fn copy_component_metrics_for_baseline(
    source_metrics: OraclePricerRiskBenchmarkComponentMetrics,
) -> OraclePricerRiskPerformanceBaselineComponentMetrics {
    OraclePricerRiskPerformanceBaselineComponentMetrics {
        mean_latency_seconds: source_metrics.mean_latency_seconds,
        throughput_operations_per_second: source_metrics.throughput_operations_per_second,
        measured_operation_count: source_metrics.measured_operation_count,
    }
}

fn validate_report_field_for_baseline_capture(
    field_name: &'static str,
    expected: &'static str,
    actual: &str,
) -> Result<(), OraclePricerRiskPerformanceBaselineCaptureError> {
    if actual == expected {
        return Ok(());
    }

    Err(
        OraclePricerRiskPerformanceBaselineCaptureError::InvalidReportField {
            field_name,
            expected,
            actual: actual.to_string(),
        },
    )
}

#[derive(Debug, Clone, PartialEq)]
pub enum OraclePricerRiskPerformanceBaselineLoadError {
    InvalidJson {
        source: String,
    },
    InvalidBaselineArtifact {
        source: OraclePricerRiskPerformanceBaselineValidationError,
    },
}

impl Display for OraclePricerRiskPerformanceBaselineLoadError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidJson { source } => write!(
                formatter,
                "oracle pricer risk performance baseline payload must be valid JSON: {source}"
            ),
            Self::InvalidBaselineArtifact { source } => write!(
                formatter,
                "oracle pricer risk performance baseline payload is invalid: {source}"
            ),
        }
    }
}

impl Error for OraclePricerRiskPerformanceBaselineLoadError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidBaselineArtifact { source } => Some(source),
            _ => None,
        }
    }
}

pub fn load_oracle_pricer_risk_performance_baseline_from_json(
    baseline_payload: &str,
) -> Result<OraclePricerRiskPerformanceBaselineArtifact, OraclePricerRiskPerformanceBaselineLoadError>
{
    let mut baseline: OraclePricerRiskPerformanceBaselineArtifact =
        serde_json::from_str(baseline_payload).map_err(|source| {
            OraclePricerRiskPerformanceBaselineLoadError::InvalidJson {
                source: source.to_string(),
            }
        })?;
    baseline.validate().map_err(|source| {
        OraclePricerRiskPerformanceBaselineLoadError::InvalidBaselineArtifact { source }
    })?;
    baseline.sort_profile_records_canonically();

    Ok(baseline)
}

#[derive(Debug, Clone, PartialEq)]
pub enum OraclePricerRiskPerformanceBaselineSerializeError {
    InvalidBaselineArtifact {
        source: OraclePricerRiskPerformanceBaselineValidationError,
    },
    SerializationFailure {
        source: String,
    },
}

impl Display for OraclePricerRiskPerformanceBaselineSerializeError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidBaselineArtifact { source } => write!(
                formatter,
                "oracle pricer risk performance baseline artifact is invalid: {source}"
            ),
            Self::SerializationFailure { source } => write!(
                formatter,
                "failed to serialize oracle pricer risk performance baseline artifact: {source}"
            ),
        }
    }
}

impl Error for OraclePricerRiskPerformanceBaselineSerializeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidBaselineArtifact { source } => Some(source),
            _ => None,
        }
    }
}

pub fn serialize_oracle_pricer_risk_performance_baseline_to_json(
    baseline: &OraclePricerRiskPerformanceBaselineArtifact,
) -> Result<String, OraclePricerRiskPerformanceBaselineSerializeError> {
    baseline.validate().map_err(|source| {
        OraclePricerRiskPerformanceBaselineSerializeError::InvalidBaselineArtifact { source }
    })?;
    serde_json::to_string_pretty(baseline).map_err(|source| {
        OraclePricerRiskPerformanceBaselineSerializeError::SerializationFailure {
            source: source.to_string(),
        }
    })
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OraclePricerRiskPerformanceBaselineComparisonComponentEntry {
    pub profile_id: String,
    pub workload_scale: BenchmarkWorkloadScale,
    pub component_id: String,
    pub reference_mean_latency_seconds: f64,
    pub candidate_mean_latency_seconds: f64,
    pub mean_latency_seconds_delta: f64,
    pub mean_latency_relative_change: f64,
    pub reference_throughput_operations_per_second: f64,
    pub candidate_throughput_operations_per_second: f64,
    pub throughput_operations_per_second_delta: f64,
    pub throughput_relative_change: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct OraclePricerRiskPerformanceBaselineComparisonSummary {
    pub profile_count: usize,
    pub component_entry_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OraclePricerRiskPerformanceBaselineComparisonReport {
    pub schema_version: &'static str,
    pub comparison_policy: &'static str,
    pub baseline_id: String,
    pub reference_baseline_version: String,
    pub candidate_baseline_version: String,
    pub catalog_id: String,
    pub catalog_version: String,
    pub policy_id: String,
    pub policy_version: String,
    pub summary: OraclePricerRiskPerformanceBaselineComparisonSummary,
    pub component_entries: Vec<OraclePricerRiskPerformanceBaselineComparisonComponentEntry>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum OraclePricerRiskPerformanceBaselineComparisonError {
    InvalidReferenceBaseline {
        source: OraclePricerRiskPerformanceBaselineValidationError,
    },
    InvalidCandidateBaseline {
        source: OraclePricerRiskPerformanceBaselineValidationError,
    },
    BaselineIdMismatch {
        reference: String,
        candidate: String,
    },
    SourceReportMismatch {
        field_name: &'static str,
        reference: String,
        candidate: String,
    },
    ProfileSetMismatch {
        missing_in_reference: Vec<String>,
        missing_in_candidate: Vec<String>,
    },
    WorkloadScaleMismatch {
        profile_id: String,
        reference: BenchmarkWorkloadScale,
        candidate: BenchmarkWorkloadScale,
    },
    IntervalDeltaMismatch {
        profile_id: String,
        reference: f64,
        candidate: f64,
    },
}

impl Display for OraclePricerRiskPerformanceBaselineComparisonError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidReferenceBaseline { source } => write!(
                formatter,
                "reference performance baseline is invalid: {source}"
            ),
            Self::InvalidCandidateBaseline { source } => write!(
                formatter,
                "candidate performance baseline is invalid: {source}"
            ),
            Self::BaselineIdMismatch {
                reference,
                candidate,
            } => write!(
                formatter,
                "baseline_id mismatch between reference `{reference}` and candidate `{candidate}`"
            ),
            Self::SourceReportMismatch {
                field_name,
                reference,
                candidate,
            } => write!(
                formatter,
                "source report metadata field `{field_name}` mismatch between reference `{reference}` and candidate `{candidate}`"
            ),
            Self::ProfileSetMismatch {
                missing_in_reference,
                missing_in_candidate,
            } => write!(
                formatter,
                "profile set mismatch between baselines (missing_in_reference={:?}, missing_in_candidate={:?})",
                missing_in_reference, missing_in_candidate
            ),
            Self::WorkloadScaleMismatch {
                profile_id,
                reference,
                candidate,
            } => write!(
                formatter,
                "profile `{profile_id}` has different workload scales between reference (`{reference}`) and candidate (`{candidate}`)"
            ),
            Self::IntervalDeltaMismatch {
                profile_id,
                reference,
                candidate,
            } => write!(
                formatter,
                "profile `{profile_id}` has different interval_delta values between reference ({reference}) and candidate ({candidate})"
            ),
        }
    }
}

impl Error for OraclePricerRiskPerformanceBaselineComparisonError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidReferenceBaseline { source } => Some(source),
            Self::InvalidCandidateBaseline { source } => Some(source),
            _ => None,
        }
    }
}

pub fn compare_oracle_pricer_risk_performance_baseline_versions(
    reference_baseline: &OraclePricerRiskPerformanceBaselineArtifact,
    candidate_baseline: &OraclePricerRiskPerformanceBaselineArtifact,
) -> Result<
    OraclePricerRiskPerformanceBaselineComparisonReport,
    OraclePricerRiskPerformanceBaselineComparisonError,
> {
    let mut reference_baseline = reference_baseline.clone();
    let mut candidate_baseline = candidate_baseline.clone();

    reference_baseline.validate().map_err(|source| {
        OraclePricerRiskPerformanceBaselineComparisonError::InvalidReferenceBaseline { source }
    })?;
    candidate_baseline.validate().map_err(|source| {
        OraclePricerRiskPerformanceBaselineComparisonError::InvalidCandidateBaseline { source }
    })?;
    reference_baseline.sort_profile_records_canonically();
    candidate_baseline.sort_profile_records_canonically();

    if reference_baseline.baseline_id != candidate_baseline.baseline_id {
        return Err(
            OraclePricerRiskPerformanceBaselineComparisonError::BaselineIdMismatch {
                reference: reference_baseline.baseline_id,
                candidate: candidate_baseline.baseline_id,
            },
        );
    }

    validate_source_report_match_for_comparison(
        "catalog_id",
        &reference_baseline.source_report.catalog_id,
        &candidate_baseline.source_report.catalog_id,
    )?;
    validate_source_report_match_for_comparison(
        "catalog_version",
        &reference_baseline.source_report.catalog_version,
        &candidate_baseline.source_report.catalog_version,
    )?;
    validate_source_report_match_for_comparison(
        "policy_id",
        &reference_baseline.source_report.policy_id,
        &candidate_baseline.source_report.policy_id,
    )?;
    validate_source_report_match_for_comparison(
        "policy_version",
        &reference_baseline.source_report.policy_version,
        &candidate_baseline.source_report.policy_version,
    )?;
    validate_source_report_match_for_comparison(
        "benchmark_report_schema_version",
        &reference_baseline
            .source_report
            .benchmark_report_schema_version,
        &candidate_baseline
            .source_report
            .benchmark_report_schema_version,
    )?;
    validate_source_report_match_for_comparison(
        "determinism_policy",
        &reference_baseline.source_report.determinism_policy,
        &candidate_baseline.source_report.determinism_policy,
    )?;
    validate_source_report_match_for_comparison(
        "execution_policy",
        &reference_baseline.source_report.execution_policy,
        &candidate_baseline.source_report.execution_policy,
    )?;
    validate_source_report_match_for_comparison(
        "metric_policy",
        &reference_baseline.source_report.metric_policy,
        &candidate_baseline.source_report.metric_policy,
    )?;

    let reference_by_profile: BTreeMap<&str, &OraclePricerRiskPerformanceBaselineProfileRecord> =
        reference_baseline
            .profile_records
            .iter()
            .map(|profile_record| (profile_record.profile_id.as_str(), profile_record))
            .collect();
    let candidate_by_profile: BTreeMap<&str, &OraclePricerRiskPerformanceBaselineProfileRecord> =
        candidate_baseline
            .profile_records
            .iter()
            .map(|profile_record| (profile_record.profile_id.as_str(), profile_record))
            .collect();
    let reference_profile_ids: BTreeSet<&str> = reference_by_profile.keys().copied().collect();
    let candidate_profile_ids: BTreeSet<&str> = candidate_by_profile.keys().copied().collect();
    let missing_in_reference: Vec<String> = candidate_profile_ids
        .difference(&reference_profile_ids)
        .map(|profile_id| (*profile_id).to_string())
        .collect();
    let missing_in_candidate: Vec<String> = reference_profile_ids
        .difference(&candidate_profile_ids)
        .map(|profile_id| (*profile_id).to_string())
        .collect();
    if !missing_in_reference.is_empty() || !missing_in_candidate.is_empty() {
        return Err(
            OraclePricerRiskPerformanceBaselineComparisonError::ProfileSetMismatch {
                missing_in_reference,
                missing_in_candidate,
            },
        );
    }

    let mut component_entries: Vec<OraclePricerRiskPerformanceBaselineComparisonComponentEntry> =
        Vec::with_capacity(reference_by_profile.len() * 3);
    for profile_id in reference_profile_ids {
        let reference_profile = reference_by_profile
            .get(profile_id)
            .copied()
            .expect("profile_id from reference set must exist in reference map");
        let candidate_profile = candidate_by_profile
            .get(profile_id)
            .copied()
            .expect("profile_id from reference set must exist in candidate map");

        if reference_profile.workload_scale != candidate_profile.workload_scale {
            return Err(
                OraclePricerRiskPerformanceBaselineComparisonError::WorkloadScaleMismatch {
                    profile_id: profile_id.to_string(),
                    reference: reference_profile.workload_scale,
                    candidate: candidate_profile.workload_scale,
                },
            );
        }
        if reference_profile.interval_delta != candidate_profile.interval_delta {
            return Err(
                OraclePricerRiskPerformanceBaselineComparisonError::IntervalDeltaMismatch {
                    profile_id: profile_id.to_string(),
                    reference: reference_profile.interval_delta,
                    candidate: candidate_profile.interval_delta,
                },
            );
        }

        component_entries.push(build_performance_baseline_comparison_component_entry(
            profile_id,
            reference_profile.workload_scale,
            ORACLE_PRICER_RISK_COMPONENT_ORACLE,
            reference_profile.oracle_solver_metrics,
            candidate_profile.oracle_solver_metrics,
        ));
        component_entries.push(build_performance_baseline_comparison_component_entry(
            profile_id,
            reference_profile.workload_scale,
            ORACLE_PRICER_RISK_COMPONENT_PRICING,
            reference_profile.pricing_metrics,
            candidate_profile.pricing_metrics,
        ));
        component_entries.push(build_performance_baseline_comparison_component_entry(
            profile_id,
            reference_profile.workload_scale,
            ORACLE_PRICER_RISK_COMPONENT_RISK,
            reference_profile.risk_sampling_metrics,
            candidate_profile.risk_sampling_metrics,
        ));
    }

    let profile_count = component_entries
        .iter()
        .map(|component_entry| component_entry.profile_id.as_str())
        .collect::<BTreeSet<&str>>()
        .len();
    let component_entry_count = component_entries.len();

    Ok(OraclePricerRiskPerformanceBaselineComparisonReport {
        schema_version: ORACLE_PRICER_RISK_PERFORMANCE_BASELINE_COMPARISON_REPORT_SCHEMA_VERSION,
        comparison_policy: ORACLE_PRICER_RISK_PERFORMANCE_BASELINE_COMPARISON_POLICY,
        baseline_id: reference_baseline.baseline_id,
        reference_baseline_version: reference_baseline.baseline_version,
        candidate_baseline_version: candidate_baseline.baseline_version,
        catalog_id: reference_baseline.source_report.catalog_id,
        catalog_version: reference_baseline.source_report.catalog_version,
        policy_id: reference_baseline.source_report.policy_id,
        policy_version: reference_baseline.source_report.policy_version,
        summary: OraclePricerRiskPerformanceBaselineComparisonSummary {
            profile_count,
            component_entry_count,
        },
        component_entries,
    })
}

fn build_performance_baseline_comparison_component_entry(
    profile_id: &str,
    workload_scale: BenchmarkWorkloadScale,
    component_id: &'static str,
    reference_metrics: OraclePricerRiskPerformanceBaselineComponentMetrics,
    candidate_metrics: OraclePricerRiskPerformanceBaselineComponentMetrics,
) -> OraclePricerRiskPerformanceBaselineComparisonComponentEntry {
    let mean_latency_seconds_delta =
        candidate_metrics.mean_latency_seconds - reference_metrics.mean_latency_seconds;
    let throughput_operations_per_second_delta = candidate_metrics.throughput_operations_per_second
        - reference_metrics.throughput_operations_per_second;

    OraclePricerRiskPerformanceBaselineComparisonComponentEntry {
        profile_id: profile_id.to_string(),
        workload_scale,
        component_id: component_id.to_string(),
        reference_mean_latency_seconds: reference_metrics.mean_latency_seconds,
        candidate_mean_latency_seconds: candidate_metrics.mean_latency_seconds,
        mean_latency_seconds_delta,
        mean_latency_relative_change: epsilon_guarded_relative_change(
            mean_latency_seconds_delta,
            reference_metrics.mean_latency_seconds,
        ),
        reference_throughput_operations_per_second: reference_metrics
            .throughput_operations_per_second,
        candidate_throughput_operations_per_second: candidate_metrics
            .throughput_operations_per_second,
        throughput_operations_per_second_delta,
        throughput_relative_change: epsilon_guarded_relative_change(
            throughput_operations_per_second_delta,
            reference_metrics.throughput_operations_per_second,
        ),
    }
}

fn epsilon_guarded_relative_change(delta: f64, baseline_value: f64) -> f64 {
    delta / baseline_value.abs().max(f64::EPSILON)
}

fn validate_source_report_match_for_comparison(
    field_name: &'static str,
    reference: &str,
    candidate: &str,
) -> Result<(), OraclePricerRiskPerformanceBaselineComparisonError> {
    if reference == candidate {
        return Ok(());
    }

    Err(
        OraclePricerRiskPerformanceBaselineComparisonError::SourceReportMismatch {
            field_name,
            reference: reference.to_string(),
            candidate: candidate.to_string(),
        },
    )
}

fn validate_baseline_metadata_field(
    field_name: &'static str,
    expected: &'static str,
    actual: &str,
) -> Result<(), OraclePricerRiskPerformanceBaselineValidationError> {
    if actual == expected {
        return Ok(());
    }

    Err(
        OraclePricerRiskPerformanceBaselineValidationError::InvalidMetadataField {
            field_name,
            expected,
            actual: actual.to_string(),
        },
    )
}

fn validate_non_empty_baseline_field(
    field_name: &'static str,
    value: &str,
) -> Result<(), OraclePricerRiskPerformanceBaselineValidationError> {
    if !value.trim().is_empty() {
        return Ok(());
    }

    Err(OraclePricerRiskPerformanceBaselineValidationError::EmptyField { field_name })
}

fn validate_required_field_list(
    field_name: &'static str,
    actual: &[String],
    expected: &[&str],
) -> Result<(), OraclePricerRiskPerformanceBaselineValidationError> {
    let expected_as_strings = required_field_list_to_owned(expected);
    if actual == expected_as_strings.as_slice() {
        return Ok(());
    }

    Err(
        OraclePricerRiskPerformanceBaselineValidationError::InvalidRequiredFieldList {
            field_name,
            expected: expected_as_strings,
            actual: actual.to_vec(),
        },
    )
}

fn required_field_list_to_owned(field_names: &[&str]) -> Vec<String> {
    field_names
        .iter()
        .map(|field_name| (*field_name).to_string())
        .collect()
}

fn validate_component_metrics_for_baseline(
    profile_id: &str,
    component_id: &'static str,
    component_metrics: OraclePricerRiskPerformanceBaselineComponentMetrics,
) -> Result<(), OraclePricerRiskPerformanceBaselineValidationError> {
    let numeric_fields = [
        (
            "mean_latency_seconds",
            component_metrics.mean_latency_seconds,
        ),
        (
            "throughput_operations_per_second",
            component_metrics.throughput_operations_per_second,
        ),
    ];

    for (field_name, value) in numeric_fields {
        if !value.is_finite() || value < 0.0 {
            return Err(
                OraclePricerRiskPerformanceBaselineValidationError::InvalidComponentMetricValue {
                    profile_id: profile_id.to_string(),
                    component_id,
                    field_name,
                    value,
                },
            );
        }
    }

    if component_metrics.measured_operation_count == 0 {
        return Err(
            OraclePricerRiskPerformanceBaselineValidationError::InvalidComponentOperationCount {
                profile_id: profile_id.to_string(),
                component_id,
                value: component_metrics.measured_operation_count,
            },
        );
    }

    Ok(())
}
