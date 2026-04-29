//! Tail/skew diagnostic contracts.

use std::error::Error;
use std::fmt::{Display, Formatter};

use serde::Serialize;

pub const TAIL_SKEW_DIAGNOSTICS_REPORT_SCHEMA_VERSION: &str =
    "validation.stylized_fact.tail_skew.diagnostics.v1";
pub const TAIL_SKEW_DIAGNOSTICS_DETERMINISM_POLICY: &str =
    "stable_horizon_input_order_and_seeded_block_bootstrap.v1";
pub const TAIL_SKEW_DIAGNOSTICS_BOOTSTRAP_POLICY: &str =
    "moving_block_bootstrap_with_wraparound_and_percentile_intervals.v1";

#[derive(Debug, Clone, PartialEq)]
pub struct TailSkewDiagnosticsRequest {
    pub analysis_start_utc_nanos: i64,
    pub analysis_end_utc_nanos: i64,
    pub empirical_dataset: TailSkewDatasetInput,
    pub simulated_dataset: TailSkewDatasetInput,
    pub horizon_buckets: Vec<TailSkewHorizonBucket>,
    pub bootstrap_configuration: TailSkewBlockBootstrapConfiguration,
    pub sample_sufficiency_policy: TailSkewSampleSufficiencyPolicy,
    pub cpu_count: Option<usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TailSkewDatasetInput {
    pub dataset_id: String,
    pub return_observations: Vec<TailSkewReturnObservation>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TailSkewReturnObservation {
    pub timestamp_utc_nanos: i64,
    pub return_increment: f64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TailSkewHorizonBucket {
    pub horizon_label: String,
    pub horizon_duration_nanos: i64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TailSkewBlockBootstrapConfiguration {
    pub confidence_level: f64,
    pub replication_count: usize,
    pub block_length_interval_count: usize,
    pub random_seed: u64,
}

impl Default for TailSkewBlockBootstrapConfiguration {
    fn default() -> Self {
        Self {
            confidence_level: 0.95,
            replication_count: 2_000,
            block_length_interval_count: 5,
            random_seed: 7_099_721,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct TailSkewMetricSampleSufficiencyPolicy {
    pub minimum_sample_count: usize,
    pub minimum_effective_sample_count: usize,
    pub minimum_tail_observation_count: usize,
    pub minimum_horizon_coverage_ratio: f64,
}

impl TailSkewMetricSampleSufficiencyPolicy {
    pub(super) fn validate(
        self,
        field_prefix: &'static str,
    ) -> Result<(), TailSkewDiagnosticsError> {
        if self.minimum_sample_count == 0 {
            return Err(
                TailSkewDiagnosticsError::InvalidSampleSufficiencyConfiguration {
                    field_name: format!("{field_prefix}.minimum_sample_count"),
                    details: "minimum_sample_count must be greater than 0".to_string(),
                },
            );
        }

        if self.minimum_effective_sample_count == 0
            || self.minimum_effective_sample_count > self.minimum_sample_count
        {
            return Err(
                TailSkewDiagnosticsError::InvalidSampleSufficiencyConfiguration {
                    field_name: format!("{field_prefix}.minimum_effective_sample_count"),
                    details: format!(
                        "minimum_effective_sample_count must be in [1, minimum_sample_count], found {}",
                        self.minimum_effective_sample_count
                    ),
                },
            );
        }

        if self.minimum_tail_observation_count > self.minimum_effective_sample_count {
            return Err(
                TailSkewDiagnosticsError::InvalidSampleSufficiencyConfiguration {
                    field_name: format!("{field_prefix}.minimum_tail_observation_count"),
                    details: format!(
                        "minimum_tail_observation_count must be less than or equal to minimum_effective_sample_count, found {}",
                        self.minimum_tail_observation_count
                    ),
                },
            );
        }

        if !self.minimum_horizon_coverage_ratio.is_finite()
            || self.minimum_horizon_coverage_ratio <= 0.0
            || self.minimum_horizon_coverage_ratio > 1.0
        {
            return Err(
                TailSkewDiagnosticsError::InvalidSampleSufficiencyConfiguration {
                    field_name: format!("{field_prefix}.minimum_horizon_coverage_ratio"),
                    details: format!(
                        "minimum_horizon_coverage_ratio must be finite and in (0, 1], found {}",
                        self.minimum_horizon_coverage_ratio
                    ),
                },
            );
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct TailSkewSampleSufficiencyPolicy {
    pub returns_abs_quantile_shape_ratio_q99_q95: TailSkewMetricSampleSufficiencyPolicy,
    pub returns_left_tail_expected_shortfall_to_var_ratio: TailSkewMetricSampleSufficiencyPolicy,
    pub returns_bowley_skewness_quantile: TailSkewMetricSampleSufficiencyPolicy,
    pub returns_signed_tail_expected_shortfall_imbalance: TailSkewMetricSampleSufficiencyPolicy,
    pub returns_l_moment_lambda2: TailSkewMetricSampleSufficiencyPolicy,
    pub returns_l_moment_tau3: TailSkewMetricSampleSufficiencyPolicy,
    pub returns_central_variance: TailSkewMetricSampleSufficiencyPolicy,
    pub returns_central_skewness: TailSkewMetricSampleSufficiencyPolicy,
    pub returns_central_excess_kurtosis: TailSkewMetricSampleSufficiencyPolicy,
}

impl Default for TailSkewSampleSufficiencyPolicy {
    fn default() -> Self {
        Self {
            returns_abs_quantile_shape_ratio_q99_q95: TailSkewMetricSampleSufficiencyPolicy {
                minimum_sample_count: 500,
                minimum_effective_sample_count: 350,
                minimum_tail_observation_count: 0,
                minimum_horizon_coverage_ratio: 0.85,
            },
            returns_left_tail_expected_shortfall_to_var_ratio:
                TailSkewMetricSampleSufficiencyPolicy {
                    minimum_sample_count: 500,
                    minimum_effective_sample_count: 350,
                    minimum_tail_observation_count: 40,
                    minimum_horizon_coverage_ratio: 0.85,
                },
            returns_bowley_skewness_quantile: TailSkewMetricSampleSufficiencyPolicy {
                minimum_sample_count: 450,
                minimum_effective_sample_count: 320,
                minimum_tail_observation_count: 0,
                minimum_horizon_coverage_ratio: 0.8,
            },
            returns_signed_tail_expected_shortfall_imbalance:
                TailSkewMetricSampleSufficiencyPolicy {
                    minimum_sample_count: 500,
                    minimum_effective_sample_count: 360,
                    minimum_tail_observation_count: 45,
                    minimum_horizon_coverage_ratio: 0.85,
                },
            returns_l_moment_lambda2: TailSkewMetricSampleSufficiencyPolicy {
                minimum_sample_count: 300,
                minimum_effective_sample_count: 240,
                minimum_tail_observation_count: 0,
                minimum_horizon_coverage_ratio: 0.8,
            },
            returns_l_moment_tau3: TailSkewMetricSampleSufficiencyPolicy {
                minimum_sample_count: 300,
                minimum_effective_sample_count: 240,
                minimum_tail_observation_count: 0,
                minimum_horizon_coverage_ratio: 0.8,
            },
            returns_central_variance: TailSkewMetricSampleSufficiencyPolicy {
                minimum_sample_count: 300,
                minimum_effective_sample_count: 240,
                minimum_tail_observation_count: 0,
                minimum_horizon_coverage_ratio: 0.8,
            },
            returns_central_skewness: TailSkewMetricSampleSufficiencyPolicy {
                minimum_sample_count: 350,
                minimum_effective_sample_count: 260,
                minimum_tail_observation_count: 0,
                minimum_horizon_coverage_ratio: 0.82,
            },
            returns_central_excess_kurtosis: TailSkewMetricSampleSufficiencyPolicy {
                minimum_sample_count: 350,
                minimum_effective_sample_count: 260,
                minimum_tail_observation_count: 0,
                minimum_horizon_coverage_ratio: 0.82,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TailSkewDiagnosticsReport {
    pub schema_version: &'static str,
    pub determinism_policy: &'static str,
    pub bootstrap_policy: &'static str,
    pub insufficient_data_status_code: &'static str,
    pub empirical_dataset_id: String,
    pub simulated_dataset_id: String,
    pub analysis_start_utc_nanos: i64,
    pub analysis_end_utc_nanos: i64,
    pub horizons: Vec<TailSkewHorizonDiagnostics>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TailSkewHorizonDiagnostics {
    pub horizon_label: String,
    pub horizon_duration_nanos: i64,
    pub metrics: Vec<TailSkewMetricComparison>,
}

/// Dataset side for a tail/skew dataset-metric progress event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TailSkewDatasetRole {
    /// Empirical dataset task.
    Empirical,
    /// Simulated dataset task.
    Simulated,
}

/// Progress event emitted after one tail/skew dataset metric task completes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TailSkewDiagnosticsProgressEvent {
    /// Bucket label supplied in the diagnostics request.
    pub bucket_label: String,
    /// Dataset side whose metric task completed.
    pub dataset_role: TailSkewDatasetRole,
    /// Stable metric identifier.
    pub metric_id: &'static str,
    /// Human-readable metric name.
    pub metric_name: &'static str,
    /// Completed dataset metric task count.
    pub completed_units: usize,
    /// Total dataset metric task count.
    pub total_units: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TailSkewMetricStatus {
    Available,
    InsufficientData,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TailSkewMetricClass {
    PrimaryRobust,
    SecondaryMoment,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct TailSkewConfidenceInterval {
    pub lower_bound: f64,
    pub upper_bound: f64,
    pub confidence_level: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct TailSkewSampleCoverage {
    pub horizon_interval_count: usize,
    pub covered_interval_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct TailSkewObservedCounts {
    pub sample_count: usize,
    pub effective_sample_count: usize,
    pub tail_observation_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TailSkewDatasetMetricResult {
    pub status: TailSkewMetricStatus,
    pub value: Option<f64>,
    pub confidence_interval: Option<TailSkewConfidenceInterval>,
    pub sample_coverage: TailSkewSampleCoverage,
    pub observed_counts: TailSkewObservedCounts,
    pub required_minimums: TailSkewMetricSampleSufficiencyPolicy,
    pub horizon_coverage_ratio: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TailSkewMetricComparison {
    pub metric_id: &'static str,
    pub metric_name: &'static str,
    pub metric_class: TailSkewMetricClass,
    pub status: TailSkewMetricStatus,
    pub empirical: TailSkewDatasetMetricResult,
    pub simulated: TailSkewDatasetMetricResult,
    pub absolute_delta: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TailSkewDiagnosticsError {
    InvalidAnalysisWindow {
        analysis_start_utc_nanos: i64,
        analysis_end_utc_nanos: i64,
    },
    EmptyDatasetId {
        dataset_role: &'static str,
    },
    EmptyHorizonBucketSet,
    EmptyHorizonLabel {
        horizon_position: usize,
    },
    DuplicateHorizonLabel {
        horizon_label: String,
    },
    InvalidHorizonDuration {
        horizon_label: String,
        horizon_duration_nanos: i64,
    },
    InvalidReturnObservation {
        dataset_role: &'static str,
        dataset_id: String,
        observation_position: usize,
        return_increment: f64,
    },
    InvalidBootstrapConfiguration {
        field_name: &'static str,
        details: String,
    },
    InvalidSampleSufficiencyConfiguration {
        field_name: String,
        details: String,
    },
    InvalidParallelismConfiguration {
        details: String,
    },
}

impl Display for TailSkewDiagnosticsError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidAnalysisWindow {
                analysis_start_utc_nanos,
                analysis_end_utc_nanos,
            } => write!(
                formatter,
                "analysis window is invalid, start {} must be less than end {}",
                analysis_start_utc_nanos, analysis_end_utc_nanos
            ),
            Self::EmptyDatasetId { dataset_role } => {
                write!(formatter, "{dataset_role} dataset_id must be non-empty")
            }
            Self::EmptyHorizonBucketSet => {
                write!(formatter, "at least one horizon bucket is required")
            }
            Self::EmptyHorizonLabel { horizon_position } => write!(
                formatter,
                "horizon bucket at position {} must have a non-empty label",
                horizon_position
            ),
            Self::DuplicateHorizonLabel { horizon_label } => {
                write!(
                    formatter,
                    "duplicate horizon label `{horizon_label}` is not allowed"
                )
            }
            Self::InvalidHorizonDuration {
                horizon_label,
                horizon_duration_nanos,
            } => write!(
                formatter,
                "horizon `{horizon_label}` duration must be positive, found {horizon_duration_nanos}"
            ),
            Self::InvalidReturnObservation {
                dataset_role,
                dataset_id,
                observation_position,
                return_increment,
            } => write!(
                formatter,
                "{dataset_role} dataset `{dataset_id}` return observation at position {} must be finite, found {}",
                observation_position, return_increment
            ),
            Self::InvalidBootstrapConfiguration {
                field_name,
                details,
            } => {
                write!(
                    formatter,
                    "bootstrap configuration field `{field_name}` is invalid: {details}"
                )
            }
            Self::InvalidSampleSufficiencyConfiguration {
                field_name,
                details,
            } => write!(
                formatter,
                "sample sufficiency policy field `{field_name}` is invalid: {details}"
            ),
            Self::InvalidParallelismConfiguration { details } => {
                write!(formatter, "parallelism configuration is invalid: {details}")
            }
        }
    }
}

impl Error for TailSkewDiagnosticsError {}
