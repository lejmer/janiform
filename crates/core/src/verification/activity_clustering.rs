//! Activity and volatility clustering diagnostics for stylized-fact validation.

use std::collections::BTreeSet;
use std::error::Error;
use std::fmt::{Display, Formatter};

use serde::Serialize;

use crate::randomness::{
    SimulationRandomnessStream, SplitMix64SimulationRandomnessStream, splitmix64_output_mixer,
};

use super::STYLIZED_FACT_METRIC_CATALOG_INSUFFICIENT_DATA_STATUS_CODE;
use super::time_buckets::{
    TimestampAnalysisWindow, interval_count_for_window, interval_position_for_timestamp,
};

const EVENT_BURST_DURATION_METRIC_ID: &str = "activity_burst_duration_quantile_ratio_q90_q50";
const EVENT_BURST_DURATION_METRIC_NAME: &str = "Activity Burst Duration Quantile Ratio Q90 Q50";
const EVENT_INTERARRIVAL_TAIL_RATIO_METRIC_ID: &str =
    "activity_interarrival_expected_shortfall_ratio";
const EVENT_INTERARRIVAL_TAIL_RATIO_METRIC_NAME: &str =
    "Activity Interarrival Expected Shortfall Ratio";
const VOLATILITY_CLUSTERING_METRIC_ID: &str = "volatility_abs_return_lag1_autocorrelation";
const VOLATILITY_CLUSTERING_METRIC_NAME: &str = "Volatility Abs Return Lag1 Autocorrelation";
const INTERARRIVAL_TAIL_QUANTILE_LEVEL: f64 = 0.95;

pub const ACTIVITY_CLUSTERING_DIAGNOSTICS_REPORT_SCHEMA_VERSION: &str =
    "validation.stylized_fact.activity_clustering.diagnostics.v1";
pub const ACTIVITY_CLUSTERING_DIAGNOSTICS_DETERMINISM_POLICY: &str =
    "stable_horizon_input_order_and_seeded_block_bootstrap.v1";
pub const ACTIVITY_CLUSTERING_DIAGNOSTICS_BOOTSTRAP_POLICY: &str =
    "moving_block_bootstrap_with_wraparound_and_percentile_intervals.v1";

#[derive(Debug, Clone, PartialEq)]
pub struct ActivityClusteringDiagnosticsRequest {
    pub analysis_start_utc_nanos: i64,
    pub analysis_end_utc_nanos: i64,
    pub empirical_dataset: ActivityClusteringDatasetInput,
    pub simulated_dataset: ActivityClusteringDatasetInput,
    pub horizon_buckets: Vec<ActivityClusteringHorizonBucket>,
    pub bootstrap_configuration: ActivityClusteringBlockBootstrapConfiguration,
    pub sample_sufficiency_policy: ActivityClusteringSampleSufficiencyPolicy,
    pub threshold_policy: ActivityClusteringThresholdPolicy,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ActivityClusteringDatasetInput {
    pub dataset_id: String,
    pub event_timestamps_utc_nanos: Vec<i64>,
    pub return_observations: Vec<ActivityClusteringReturnObservation>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ActivityClusteringReturnObservation {
    pub timestamp_utc_nanos: i64,
    pub return_increment: f64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivityClusteringHorizonBucket {
    pub horizon_label: String,
    pub horizon_duration_nanos: i64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ActivityClusteringBlockBootstrapConfiguration {
    pub confidence_level: f64,
    pub replication_count: usize,
    pub block_length_interval_count: usize,
    pub random_seed: u64,
}

impl Default for ActivityClusteringBlockBootstrapConfiguration {
    fn default() -> Self {
        Self {
            confidence_level: 0.95,
            replication_count: 2_000,
            block_length_interval_count: 5,
            random_seed: 1_064_627,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct ActivityClusteringMetricSampleSufficiencyPolicy {
    pub minimum_sample_count: usize,
    pub minimum_effective_sample_count: usize,
    pub minimum_tail_observation_count: usize,
    pub minimum_horizon_coverage_ratio: f64,
}

impl ActivityClusteringMetricSampleSufficiencyPolicy {
    fn validate(
        self,
        field_prefix: &'static str,
    ) -> Result<(), ActivityClusteringDiagnosticsError> {
        if !self.minimum_horizon_coverage_ratio.is_finite()
            || self.minimum_horizon_coverage_ratio <= 0.0
            || self.minimum_horizon_coverage_ratio > 1.0
        {
            return Err(
                ActivityClusteringDiagnosticsError::InvalidSampleSufficiencyConfiguration {
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
pub struct ActivityClusteringSampleSufficiencyPolicy {
    pub event_burst_duration_quantile_ratio: ActivityClusteringMetricSampleSufficiencyPolicy,
    pub event_interarrival_expected_shortfall_ratio:
        ActivityClusteringMetricSampleSufficiencyPolicy,
    pub volatility_abs_return_lag1_autocorrelation: ActivityClusteringMetricSampleSufficiencyPolicy,
}

impl Default for ActivityClusteringSampleSufficiencyPolicy {
    fn default() -> Self {
        Self {
            event_burst_duration_quantile_ratio: ActivityClusteringMetricSampleSufficiencyPolicy {
                minimum_sample_count: 250,
                minimum_effective_sample_count: 200,
                minimum_tail_observation_count: 0,
                minimum_horizon_coverage_ratio: 0.8,
            },
            event_interarrival_expected_shortfall_ratio:
                ActivityClusteringMetricSampleSufficiencyPolicy {
                    minimum_sample_count: 300,
                    minimum_effective_sample_count: 220,
                    minimum_tail_observation_count: 35,
                    minimum_horizon_coverage_ratio: 0.8,
                },
            volatility_abs_return_lag1_autocorrelation:
                ActivityClusteringMetricSampleSufficiencyPolicy {
                    minimum_sample_count: 450,
                    minimum_effective_sample_count: 320,
                    minimum_tail_observation_count: 0,
                    minimum_horizon_coverage_ratio: 0.75,
                },
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ActivityClusteringThresholdPolicy {
    pub comparison_policy_id: String,
    pub event_burst_duration_quantile_ratio_max_absolute_delta: f64,
    pub event_interarrival_expected_shortfall_ratio_max_absolute_delta: f64,
    pub volatility_abs_return_lag1_autocorrelation_max_absolute_delta: f64,
}

impl Default for ActivityClusteringThresholdPolicy {
    fn default() -> Self {
        Self {
            comparison_policy_id: "absolute_delta_threshold_policy.v1".to_string(),
            event_burst_duration_quantile_ratio_max_absolute_delta: 0.35,
            event_interarrival_expected_shortfall_ratio_max_absolute_delta: 0.35,
            volatility_abs_return_lag1_autocorrelation_max_absolute_delta: 0.2,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ActivityClusteringDiagnosticsReport {
    pub schema_version: &'static str,
    pub determinism_policy: &'static str,
    pub bootstrap_policy: &'static str,
    pub insufficient_data_status_code: &'static str,
    pub empirical_dataset_id: String,
    pub simulated_dataset_id: String,
    pub analysis_start_utc_nanos: i64,
    pub analysis_end_utc_nanos: i64,
    pub horizons: Vec<ActivityClusteringHorizonDiagnostics>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ActivityClusteringHorizonDiagnostics {
    pub horizon_label: String,
    pub horizon_duration_nanos: i64,
    pub event_burst_duration_quantile_ratio: ActivityClusteringMetricComparison,
    pub event_interarrival_expected_shortfall_ratio: ActivityClusteringMetricComparison,
    pub volatility_abs_return_lag1_autocorrelation: ActivityClusteringMetricComparison,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivityClusteringMetricStatus {
    Available,
    InsufficientData,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct ActivityClusteringConfidenceInterval {
    pub lower_bound: f64,
    pub upper_bound: f64,
    pub confidence_level: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ActivityClusteringSampleCoverage {
    pub horizon_interval_count: usize,
    pub covered_interval_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ActivityClusteringObservedCounts {
    pub sample_count: usize,
    pub effective_sample_count: usize,
    pub tail_observation_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ActivityClusteringDatasetMetricResult {
    pub status: ActivityClusteringMetricStatus,
    pub value: Option<f64>,
    pub confidence_interval: Option<ActivityClusteringConfidenceInterval>,
    pub sample_coverage: ActivityClusteringSampleCoverage,
    pub observed_counts: ActivityClusteringObservedCounts,
    pub required_minimums: ActivityClusteringMetricSampleSufficiencyPolicy,
    pub horizon_coverage_ratio: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ActivityClusteringThresholdContext {
    pub comparison_policy_id: String,
    pub max_absolute_delta: f64,
    pub absolute_delta_exceeds_threshold: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ActivityClusteringMetricComparison {
    pub metric_id: &'static str,
    pub metric_name: &'static str,
    pub status: ActivityClusteringMetricStatus,
    pub empirical: ActivityClusteringDatasetMetricResult,
    pub simulated: ActivityClusteringDatasetMetricResult,
    pub absolute_delta: Option<f64>,
    pub threshold_context: ActivityClusteringThresholdContext,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ActivityClusteringDiagnosticsError {
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
    InvalidThresholdPolicy {
        field_name: &'static str,
        details: String,
    },
}

impl Display for ActivityClusteringDiagnosticsError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidAnalysisWindow {
                analysis_start_utc_nanos,
                analysis_end_utc_nanos,
            } => write!(
                formatter,
                "analysis window must be strictly increasing, found start={} end={}",
                analysis_start_utc_nanos, analysis_end_utc_nanos
            ),
            Self::EmptyDatasetId { dataset_role } => {
                write!(formatter, "{dataset_role} dataset_id must be non-empty")
            }
            Self::EmptyHorizonBucketSet => write!(
                formatter,
                "activity clustering diagnostics requires at least one horizon bucket"
            ),
            Self::EmptyHorizonLabel { horizon_position } => write!(
                formatter,
                "horizon bucket at position {horizon_position} must define a non-empty horizon_label"
            ),
            Self::DuplicateHorizonLabel { horizon_label } => write!(
                formatter,
                "horizon_label `{horizon_label}` is duplicated in request"
            ),
            Self::InvalidHorizonDuration {
                horizon_label,
                horizon_duration_nanos,
            } => write!(
                formatter,
                "horizon `{horizon_label}` must have a strictly positive horizon_duration_nanos, found {horizon_duration_nanos}"
            ),
            Self::InvalidReturnObservation {
                dataset_role,
                dataset_id,
                observation_position,
                return_increment,
            } => write!(
                formatter,
                "{dataset_role} dataset `{dataset_id}` has non-finite return_increment at position {observation_position}: {return_increment}"
            ),
            Self::InvalidBootstrapConfiguration {
                field_name,
                details,
            } => write!(
                formatter,
                "bootstrap configuration field `{field_name}` is invalid: {details}"
            ),
            Self::InvalidSampleSufficiencyConfiguration {
                field_name,
                details,
            } => write!(
                formatter,
                "sample sufficiency policy field `{field_name}` is invalid: {details}"
            ),
            Self::InvalidThresholdPolicy {
                field_name,
                details,
            } => write!(
                formatter,
                "threshold policy field `{field_name}` is invalid: {details}"
            ),
        }
    }
}

impl Error for ActivityClusteringDiagnosticsError {}

pub fn run_activity_clustering_diagnostics(
    request: ActivityClusteringDiagnosticsRequest,
) -> Result<ActivityClusteringDiagnosticsReport, ActivityClusteringDiagnosticsError> {
    validate_request(&request)?;

    let analysis_window = TimestampAnalysisWindow::new(
        request.analysis_start_utc_nanos,
        request.analysis_end_utc_nanos,
    );
    let empirical_dataset =
        prepare_dataset("empirical", &request.empirical_dataset, analysis_window)?;
    let simulated_dataset =
        prepare_dataset("simulated", &request.simulated_dataset, analysis_window)?;

    let mut horizon_reports: Vec<ActivityClusteringHorizonDiagnostics> =
        Vec::with_capacity(request.horizon_buckets.len());
    for (horizon_position, horizon_bucket) in request.horizon_buckets.iter().enumerate() {
        let empirical_horizon_series =
            build_horizon_dataset_series(&empirical_dataset, analysis_window, horizon_bucket);
        let simulated_horizon_series =
            build_horizon_dataset_series(&simulated_dataset, analysis_window, horizon_bucket);

        let event_burst_duration_quantile_ratio = build_metric_comparison(
            EVENT_BURST_DURATION_METRIC_ID,
            EVENT_BURST_DURATION_METRIC_NAME,
            evaluate_event_burst_duration_metric(
                &empirical_horizon_series,
                request
                    .sample_sufficiency_policy
                    .event_burst_duration_quantile_ratio,
                request.bootstrap_configuration,
                derive_metric_bootstrap_seed(
                    request.bootstrap_configuration.random_seed,
                    horizon_position,
                    1,
                    11,
                ),
            ),
            evaluate_event_burst_duration_metric(
                &simulated_horizon_series,
                request
                    .sample_sufficiency_policy
                    .event_burst_duration_quantile_ratio,
                request.bootstrap_configuration,
                derive_metric_bootstrap_seed(
                    request.bootstrap_configuration.random_seed,
                    horizon_position,
                    2,
                    11,
                ),
            ),
            request
                .threshold_policy
                .event_burst_duration_quantile_ratio_max_absolute_delta,
            &request.threshold_policy.comparison_policy_id,
        );

        let event_interarrival_expected_shortfall_ratio = build_metric_comparison(
            EVENT_INTERARRIVAL_TAIL_RATIO_METRIC_ID,
            EVENT_INTERARRIVAL_TAIL_RATIO_METRIC_NAME,
            evaluate_event_interarrival_tail_metric(
                &empirical_horizon_series,
                request
                    .sample_sufficiency_policy
                    .event_interarrival_expected_shortfall_ratio,
                request.bootstrap_configuration,
                derive_metric_bootstrap_seed(
                    request.bootstrap_configuration.random_seed,
                    horizon_position,
                    1,
                    12,
                ),
            ),
            evaluate_event_interarrival_tail_metric(
                &simulated_horizon_series,
                request
                    .sample_sufficiency_policy
                    .event_interarrival_expected_shortfall_ratio,
                request.bootstrap_configuration,
                derive_metric_bootstrap_seed(
                    request.bootstrap_configuration.random_seed,
                    horizon_position,
                    2,
                    12,
                ),
            ),
            request
                .threshold_policy
                .event_interarrival_expected_shortfall_ratio_max_absolute_delta,
            &request.threshold_policy.comparison_policy_id,
        );

        let volatility_abs_return_lag1_autocorrelation = build_metric_comparison(
            VOLATILITY_CLUSTERING_METRIC_ID,
            VOLATILITY_CLUSTERING_METRIC_NAME,
            evaluate_volatility_clustering_metric(
                &empirical_horizon_series,
                request
                    .sample_sufficiency_policy
                    .volatility_abs_return_lag1_autocorrelation,
                request.bootstrap_configuration,
                derive_metric_bootstrap_seed(
                    request.bootstrap_configuration.random_seed,
                    horizon_position,
                    1,
                    13,
                ),
            ),
            evaluate_volatility_clustering_metric(
                &simulated_horizon_series,
                request
                    .sample_sufficiency_policy
                    .volatility_abs_return_lag1_autocorrelation,
                request.bootstrap_configuration,
                derive_metric_bootstrap_seed(
                    request.bootstrap_configuration.random_seed,
                    horizon_position,
                    2,
                    13,
                ),
            ),
            request
                .threshold_policy
                .volatility_abs_return_lag1_autocorrelation_max_absolute_delta,
            &request.threshold_policy.comparison_policy_id,
        );

        horizon_reports.push(ActivityClusteringHorizonDiagnostics {
            horizon_label: horizon_bucket.horizon_label.clone(),
            horizon_duration_nanos: horizon_bucket.horizon_duration_nanos,
            event_burst_duration_quantile_ratio,
            event_interarrival_expected_shortfall_ratio,
            volatility_abs_return_lag1_autocorrelation,
        });
    }

    Ok(ActivityClusteringDiagnosticsReport {
        schema_version: ACTIVITY_CLUSTERING_DIAGNOSTICS_REPORT_SCHEMA_VERSION,
        determinism_policy: ACTIVITY_CLUSTERING_DIAGNOSTICS_DETERMINISM_POLICY,
        bootstrap_policy: ACTIVITY_CLUSTERING_DIAGNOSTICS_BOOTSTRAP_POLICY,
        insufficient_data_status_code: STYLIZED_FACT_METRIC_CATALOG_INSUFFICIENT_DATA_STATUS_CODE,
        empirical_dataset_id: empirical_dataset.dataset_id,
        simulated_dataset_id: simulated_dataset.dataset_id,
        analysis_start_utc_nanos: request.analysis_start_utc_nanos,
        analysis_end_utc_nanos: request.analysis_end_utc_nanos,
        horizons: horizon_reports,
    })
}

#[derive(Debug, Clone, PartialEq)]
struct PreparedActivityClusteringDataset {
    dataset_id: String,
    event_timestamps_in_window: Vec<i64>,
    return_observations_in_window: Vec<ActivityClusteringReturnObservation>,
}

#[derive(Debug, Clone, PartialEq)]
struct HorizonDatasetSeries {
    interval_count: usize,
    event_count_by_interval: Vec<u64>,
    event_covered_interval_count: usize,
    interarrival_interval_units: Vec<f64>,
    abs_return_by_interval: Vec<f64>,
    return_covered_interval_count: usize,
}

fn validate_request(
    request: &ActivityClusteringDiagnosticsRequest,
) -> Result<(), ActivityClusteringDiagnosticsError> {
    if request.analysis_start_utc_nanos >= request.analysis_end_utc_nanos {
        return Err(ActivityClusteringDiagnosticsError::InvalidAnalysisWindow {
            analysis_start_utc_nanos: request.analysis_start_utc_nanos,
            analysis_end_utc_nanos: request.analysis_end_utc_nanos,
        });
    }

    validate_dataset_id("empirical", &request.empirical_dataset.dataset_id)?;
    validate_dataset_id("simulated", &request.simulated_dataset.dataset_id)?;

    if request.horizon_buckets.is_empty() {
        return Err(ActivityClusteringDiagnosticsError::EmptyHorizonBucketSet);
    }
    let mut observed_horizon_labels: BTreeSet<&str> = BTreeSet::new();
    for (horizon_position, horizon_bucket) in request.horizon_buckets.iter().enumerate() {
        if horizon_bucket.horizon_label.trim().is_empty() {
            return Err(ActivityClusteringDiagnosticsError::EmptyHorizonLabel { horizon_position });
        }
        if horizon_bucket.horizon_duration_nanos <= 0 {
            return Err(ActivityClusteringDiagnosticsError::InvalidHorizonDuration {
                horizon_label: horizon_bucket.horizon_label.clone(),
                horizon_duration_nanos: horizon_bucket.horizon_duration_nanos,
            });
        }
        if !observed_horizon_labels.insert(horizon_bucket.horizon_label.as_str()) {
            return Err(ActivityClusteringDiagnosticsError::DuplicateHorizonLabel {
                horizon_label: horizon_bucket.horizon_label.clone(),
            });
        }
    }

    validate_bootstrap_configuration(request.bootstrap_configuration)?;
    request
        .sample_sufficiency_policy
        .event_burst_duration_quantile_ratio
        .validate("event_burst_duration_quantile_ratio")?;
    request
        .sample_sufficiency_policy
        .event_interarrival_expected_shortfall_ratio
        .validate("event_interarrival_expected_shortfall_ratio")?;
    request
        .sample_sufficiency_policy
        .volatility_abs_return_lag1_autocorrelation
        .validate("volatility_abs_return_lag1_autocorrelation")?;
    validate_threshold_policy(&request.threshold_policy)?;

    validate_return_observations(
        "empirical",
        &request.empirical_dataset.dataset_id,
        &request.empirical_dataset.return_observations,
    )?;
    validate_return_observations(
        "simulated",
        &request.simulated_dataset.dataset_id,
        &request.simulated_dataset.return_observations,
    )?;

    Ok(())
}

fn validate_dataset_id(
    dataset_role: &'static str,
    dataset_id: &str,
) -> Result<(), ActivityClusteringDiagnosticsError> {
    if dataset_id.trim().is_empty() {
        return Err(ActivityClusteringDiagnosticsError::EmptyDatasetId { dataset_role });
    }

    Ok(())
}

fn validate_bootstrap_configuration(
    bootstrap_configuration: ActivityClusteringBlockBootstrapConfiguration,
) -> Result<(), ActivityClusteringDiagnosticsError> {
    if !bootstrap_configuration.confidence_level.is_finite()
        || bootstrap_configuration.confidence_level <= 0.0
        || bootstrap_configuration.confidence_level >= 1.0
    {
        return Err(
            ActivityClusteringDiagnosticsError::InvalidBootstrapConfiguration {
                field_name: "confidence_level",
                details: format!(
                    "confidence_level must be finite and in (0, 1), found {}",
                    bootstrap_configuration.confidence_level
                ),
            },
        );
    }
    if bootstrap_configuration.replication_count == 0 {
        return Err(
            ActivityClusteringDiagnosticsError::InvalidBootstrapConfiguration {
                field_name: "replication_count",
                details: "replication_count must be greater than 0".to_string(),
            },
        );
    }
    if bootstrap_configuration.block_length_interval_count == 0 {
        return Err(
            ActivityClusteringDiagnosticsError::InvalidBootstrapConfiguration {
                field_name: "block_length_interval_count",
                details: "block_length_interval_count must be greater than 0".to_string(),
            },
        );
    }

    Ok(())
}

fn validate_threshold_policy(
    threshold_policy: &ActivityClusteringThresholdPolicy,
) -> Result<(), ActivityClusteringDiagnosticsError> {
    if threshold_policy.comparison_policy_id.trim().is_empty() {
        return Err(ActivityClusteringDiagnosticsError::InvalidThresholdPolicy {
            field_name: "comparison_policy_id",
            details: "comparison_policy_id must be non-empty".to_string(),
        });
    }

    validate_non_negative_finite_threshold(
        "event_burst_duration_quantile_ratio_max_absolute_delta",
        threshold_policy.event_burst_duration_quantile_ratio_max_absolute_delta,
    )?;
    validate_non_negative_finite_threshold(
        "event_interarrival_expected_shortfall_ratio_max_absolute_delta",
        threshold_policy.event_interarrival_expected_shortfall_ratio_max_absolute_delta,
    )?;
    validate_non_negative_finite_threshold(
        "volatility_abs_return_lag1_autocorrelation_max_absolute_delta",
        threshold_policy.volatility_abs_return_lag1_autocorrelation_max_absolute_delta,
    )?;

    Ok(())
}

fn validate_non_negative_finite_threshold(
    field_name: &'static str,
    threshold_value: f64,
) -> Result<(), ActivityClusteringDiagnosticsError> {
    if !threshold_value.is_finite() || threshold_value < 0.0 {
        return Err(ActivityClusteringDiagnosticsError::InvalidThresholdPolicy {
            field_name,
            details: format!(
                "threshold must be finite and non-negative, found {}",
                threshold_value
            ),
        });
    }

    Ok(())
}

fn validate_return_observations(
    dataset_role: &'static str,
    dataset_id: &str,
    return_observations: &[ActivityClusteringReturnObservation],
) -> Result<(), ActivityClusteringDiagnosticsError> {
    for (observation_position, return_observation) in return_observations.iter().enumerate() {
        if !return_observation.return_increment.is_finite() {
            return Err(
                ActivityClusteringDiagnosticsError::InvalidReturnObservation {
                    dataset_role,
                    dataset_id: dataset_id.to_string(),
                    observation_position,
                    return_increment: return_observation.return_increment,
                },
            );
        }
    }

    Ok(())
}

fn prepare_dataset(
    dataset_role: &'static str,
    dataset: &ActivityClusteringDatasetInput,
    analysis_window: TimestampAnalysisWindow,
) -> Result<PreparedActivityClusteringDataset, ActivityClusteringDiagnosticsError> {
    validate_dataset_id(dataset_role, &dataset.dataset_id)?;
    validate_return_observations(
        dataset_role,
        &dataset.dataset_id,
        &dataset.return_observations,
    )?;

    let mut event_timestamps_in_window: Vec<i64> = dataset
        .event_timestamps_utc_nanos
        .iter()
        .copied()
        .filter(|timestamp_utc_nanos| analysis_window.contains_timestamp(*timestamp_utc_nanos))
        .collect();
    event_timestamps_in_window.sort_unstable();

    let mut return_observations_in_window: Vec<ActivityClusteringReturnObservation> = dataset
        .return_observations
        .iter()
        .copied()
        .filter(|return_observation| {
            analysis_window.contains_timestamp(return_observation.timestamp_utc_nanos)
        })
        .collect();
    return_observations_in_window
        .sort_by_key(|return_observation| return_observation.timestamp_utc_nanos);

    Ok(PreparedActivityClusteringDataset {
        dataset_id: dataset.dataset_id.clone(),
        event_timestamps_in_window,
        return_observations_in_window,
    })
}

fn build_horizon_dataset_series(
    prepared_dataset: &PreparedActivityClusteringDataset,
    analysis_window: TimestampAnalysisWindow,
    horizon_bucket: &ActivityClusteringHorizonBucket,
) -> HorizonDatasetSeries {
    let interval_count =
        interval_count_for_window(analysis_window, horizon_bucket.horizon_duration_nanos)
            .unwrap_or(0);
    let mut event_count_by_interval: Vec<u64> = vec![0; interval_count];

    for timestamp_utc_nanos in &prepared_dataset.event_timestamps_in_window {
        let Some(interval_position) = interval_position_for_timestamp(
            *timestamp_utc_nanos,
            analysis_window,
            horizon_bucket.horizon_duration_nanos,
            interval_count,
        ) else {
            continue;
        };
        event_count_by_interval[interval_position] =
            event_count_by_interval[interval_position].saturating_add(1);
    }
    let event_covered_interval_count = event_count_by_interval
        .iter()
        .filter(|event_count| **event_count > 0)
        .count();

    let mut interarrival_interval_units: Vec<f64> = Vec::new();
    if prepared_dataset.event_timestamps_in_window.len() >= 2 {
        interarrival_interval_units.reserve(prepared_dataset.event_timestamps_in_window.len() - 1);
        for event_window in prepared_dataset.event_timestamps_in_window.windows(2) {
            let previous_timestamp = event_window[0];
            let current_timestamp = event_window[1];
            let interarrival_nanos = current_timestamp.saturating_sub(previous_timestamp);
            if interarrival_nanos <= 0 {
                continue;
            }
            interarrival_interval_units
                .push(interarrival_nanos as f64 / horizon_bucket.horizon_duration_nanos as f64);
        }
    }

    let mut interval_return_sum: Vec<f64> = vec![0.0; interval_count];
    let mut return_interval_has_observation: Vec<bool> = vec![false; interval_count];
    for return_observation in &prepared_dataset.return_observations_in_window {
        let Some(interval_position) = interval_position_for_timestamp(
            return_observation.timestamp_utc_nanos,
            analysis_window,
            horizon_bucket.horizon_duration_nanos,
            interval_count,
        ) else {
            continue;
        };
        interval_return_sum[interval_position] += return_observation.return_increment;
        return_interval_has_observation[interval_position] = true;
    }
    let return_covered_interval_count = return_interval_has_observation
        .iter()
        .filter(|covered| **covered)
        .count();
    let abs_return_by_interval: Vec<f64> = interval_return_sum
        .iter()
        .map(|return_sum| return_sum.abs())
        .collect();

    HorizonDatasetSeries {
        interval_count,
        event_count_by_interval,
        event_covered_interval_count,
        interarrival_interval_units,
        abs_return_by_interval,
        return_covered_interval_count,
    }
}

fn evaluate_event_burst_duration_metric(
    dataset_series: &HorizonDatasetSeries,
    required_minimums: ActivityClusteringMetricSampleSufficiencyPolicy,
    bootstrap_configuration: ActivityClusteringBlockBootstrapConfiguration,
    bootstrap_seed: u64,
) -> ActivityClusteringDatasetMetricResult {
    let run_lengths = compute_active_run_lengths(&dataset_series.event_count_by_interval);
    let value = compute_burst_duration_quantile_ratio_from_run_lengths(&run_lengths);
    let observed_counts = ActivityClusteringObservedCounts {
        sample_count: run_lengths.len(),
        effective_sample_count: run_lengths.len(),
        tail_observation_count: 0,
    };
    let sample_coverage = ActivityClusteringSampleCoverage {
        horizon_interval_count: dataset_series.interval_count,
        covered_interval_count: dataset_series.event_covered_interval_count,
    };
    let horizon_coverage_ratio = coverage_ratio(
        sample_coverage.covered_interval_count,
        sample_coverage.horizon_interval_count,
    );
    let status = resolve_metric_status(
        value,
        observed_counts,
        horizon_coverage_ratio,
        required_minimums,
    );

    let confidence_interval = if status == ActivityClusteringMetricStatus::Available {
        value.map(|point_estimate| {
            compute_block_bootstrap_confidence_interval(
                &dataset_series.event_count_by_interval,
                point_estimate,
                bootstrap_configuration,
                bootstrap_seed,
                |resampled_event_counts| {
                    let resampled_run_lengths = compute_active_run_lengths(resampled_event_counts);
                    compute_burst_duration_quantile_ratio_from_run_lengths(&resampled_run_lengths)
                },
            )
        })
    } else {
        None
    };

    ActivityClusteringDatasetMetricResult {
        status,
        value,
        confidence_interval,
        sample_coverage,
        observed_counts,
        required_minimums,
        horizon_coverage_ratio,
    }
}

fn evaluate_event_interarrival_tail_metric(
    dataset_series: &HorizonDatasetSeries,
    required_minimums: ActivityClusteringMetricSampleSufficiencyPolicy,
    bootstrap_configuration: ActivityClusteringBlockBootstrapConfiguration,
    bootstrap_seed: u64,
) -> ActivityClusteringDatasetMetricResult {
    let (value, tail_observation_count) = compute_interarrival_expected_shortfall_ratio(
        &dataset_series.interarrival_interval_units,
        INTERARRIVAL_TAIL_QUANTILE_LEVEL,
    );
    let observed_counts = ActivityClusteringObservedCounts {
        sample_count: dataset_series.interarrival_interval_units.len(),
        effective_sample_count: dataset_series.interarrival_interval_units.len(),
        tail_observation_count,
    };
    let sample_coverage = ActivityClusteringSampleCoverage {
        horizon_interval_count: dataset_series.interval_count,
        covered_interval_count: dataset_series.event_covered_interval_count,
    };
    let horizon_coverage_ratio = coverage_ratio(
        sample_coverage.covered_interval_count,
        sample_coverage.horizon_interval_count,
    );
    let status = resolve_metric_status(
        value,
        observed_counts,
        horizon_coverage_ratio,
        required_minimums,
    );

    let confidence_interval = if status == ActivityClusteringMetricStatus::Available {
        value.map(|point_estimate| {
            compute_block_bootstrap_confidence_interval(
                &dataset_series.interarrival_interval_units,
                point_estimate,
                bootstrap_configuration,
                bootstrap_seed,
                |resampled_interarrivals| {
                    compute_interarrival_expected_shortfall_ratio(
                        resampled_interarrivals,
                        INTERARRIVAL_TAIL_QUANTILE_LEVEL,
                    )
                    .0
                },
            )
        })
    } else {
        None
    };

    ActivityClusteringDatasetMetricResult {
        status,
        value,
        confidence_interval,
        sample_coverage,
        observed_counts,
        required_minimums,
        horizon_coverage_ratio,
    }
}

fn evaluate_volatility_clustering_metric(
    dataset_series: &HorizonDatasetSeries,
    required_minimums: ActivityClusteringMetricSampleSufficiencyPolicy,
    bootstrap_configuration: ActivityClusteringBlockBootstrapConfiguration,
    bootstrap_seed: u64,
) -> ActivityClusteringDatasetMetricResult {
    let value = compute_lag_one_autocorrelation(&dataset_series.abs_return_by_interval);
    let observed_counts = ActivityClusteringObservedCounts {
        sample_count: dataset_series.abs_return_by_interval.len(),
        effective_sample_count: dataset_series
            .abs_return_by_interval
            .len()
            .saturating_sub(1),
        tail_observation_count: 0,
    };
    let sample_coverage = ActivityClusteringSampleCoverage {
        horizon_interval_count: dataset_series.interval_count,
        covered_interval_count: dataset_series.return_covered_interval_count,
    };
    let horizon_coverage_ratio = coverage_ratio(
        sample_coverage.covered_interval_count,
        sample_coverage.horizon_interval_count,
    );
    let status = resolve_metric_status(
        value,
        observed_counts,
        horizon_coverage_ratio,
        required_minimums,
    );

    let confidence_interval = if status == ActivityClusteringMetricStatus::Available {
        value.map(|point_estimate| {
            compute_block_bootstrap_confidence_interval(
                &dataset_series.abs_return_by_interval,
                point_estimate,
                bootstrap_configuration,
                bootstrap_seed,
                compute_lag_one_autocorrelation,
            )
        })
    } else {
        None
    };

    ActivityClusteringDatasetMetricResult {
        status,
        value,
        confidence_interval,
        sample_coverage,
        observed_counts,
        required_minimums,
        horizon_coverage_ratio,
    }
}

fn build_metric_comparison(
    metric_id: &'static str,
    metric_name: &'static str,
    empirical: ActivityClusteringDatasetMetricResult,
    simulated: ActivityClusteringDatasetMetricResult,
    max_absolute_delta: f64,
    comparison_policy_id: &str,
) -> ActivityClusteringMetricComparison {
    let absolute_delta = match (
        empirical.status,
        empirical.value,
        simulated.status,
        simulated.value,
    ) {
        (
            ActivityClusteringMetricStatus::Available,
            Some(empirical_value),
            ActivityClusteringMetricStatus::Available,
            Some(simulated_value),
        ) => Some((empirical_value - simulated_value).abs()),
        _ => None,
    };
    let status = if absolute_delta.is_some() {
        ActivityClusteringMetricStatus::Available
    } else {
        ActivityClusteringMetricStatus::InsufficientData
    };
    let absolute_delta_exceeds_threshold = absolute_delta.map(|delta| delta > max_absolute_delta);

    ActivityClusteringMetricComparison {
        metric_id,
        metric_name,
        status,
        empirical,
        simulated,
        absolute_delta,
        threshold_context: ActivityClusteringThresholdContext {
            comparison_policy_id: comparison_policy_id.to_string(),
            max_absolute_delta,
            absolute_delta_exceeds_threshold,
        },
    }
}

fn resolve_metric_status(
    value: Option<f64>,
    observed_counts: ActivityClusteringObservedCounts,
    horizon_coverage_ratio: f64,
    required_minimums: ActivityClusteringMetricSampleSufficiencyPolicy,
) -> ActivityClusteringMetricStatus {
    let has_sufficient_counts = observed_counts.sample_count
        >= required_minimums.minimum_sample_count
        && observed_counts.effective_sample_count
            >= required_minimums.minimum_effective_sample_count
        && observed_counts.tail_observation_count
            >= required_minimums.minimum_tail_observation_count;
    let has_sufficient_coverage =
        horizon_coverage_ratio >= required_minimums.minimum_horizon_coverage_ratio;

    if value.is_some() && has_sufficient_counts && has_sufficient_coverage {
        ActivityClusteringMetricStatus::Available
    } else {
        ActivityClusteringMetricStatus::InsufficientData
    }
}

fn compute_active_run_lengths(event_count_by_interval: &[u64]) -> Vec<f64> {
    let mut run_lengths: Vec<f64> = Vec::new();
    let mut active_run_length: usize = 0;

    for event_count in event_count_by_interval {
        if *event_count > 0 {
            active_run_length = active_run_length.saturating_add(1);
        } else if active_run_length > 0 {
            run_lengths.push(active_run_length as f64);
            active_run_length = 0;
        }
    }
    if active_run_length > 0 {
        run_lengths.push(active_run_length as f64);
    }

    run_lengths
}

fn compute_burst_duration_quantile_ratio_from_run_lengths(run_lengths: &[f64]) -> Option<f64> {
    if run_lengths.is_empty() {
        return None;
    }

    let mut sorted_run_lengths = run_lengths.to_vec();
    sorted_run_lengths.sort_by(f64::total_cmp);
    let quantile_50 = compute_sorted_quantile(&sorted_run_lengths, 0.50);
    if quantile_50 <= 0.0 || !quantile_50.is_finite() {
        return None;
    }
    let quantile_90 = compute_sorted_quantile(&sorted_run_lengths, 0.90);
    let ratio = quantile_90 / quantile_50;

    ratio.is_finite().then_some(ratio)
}

fn compute_interarrival_expected_shortfall_ratio(
    interarrival_interval_units: &[f64],
    quantile_level: f64,
) -> (Option<f64>, usize) {
    if interarrival_interval_units.is_empty() {
        return (None, 0);
    }

    let mut sorted_interarrival: Vec<f64> = interarrival_interval_units
        .iter()
        .copied()
        .filter(|value| value.is_finite() && *value > 0.0)
        .collect();
    if sorted_interarrival.is_empty() {
        return (None, 0);
    }
    sorted_interarrival.sort_by(f64::total_cmp);

    let tail_var = compute_sorted_quantile(&sorted_interarrival, quantile_level);
    if !tail_var.is_finite() || tail_var <= 0.0 {
        return (None, 0);
    }

    let tail_values: Vec<f64> = sorted_interarrival
        .iter()
        .copied()
        .filter(|value| *value >= tail_var)
        .collect();
    if tail_values.is_empty() {
        return (None, 0);
    }

    let expected_shortfall = tail_values.iter().sum::<f64>() / tail_values.len() as f64;
    let ratio = expected_shortfall / tail_var;
    if !ratio.is_finite() {
        return (None, 0);
    }

    (Some(ratio), tail_values.len())
}

fn compute_lag_one_autocorrelation(series: &[f64]) -> Option<f64> {
    if series.len() < 2 {
        return None;
    }
    if series.iter().any(|value| !value.is_finite()) {
        return None;
    }

    let sample_mean = series.iter().sum::<f64>() / series.len() as f64;
    let variance_numerator = series
        .iter()
        .map(|value| {
            let centered = *value - sample_mean;
            centered * centered
        })
        .sum::<f64>();
    if !variance_numerator.is_finite() || variance_numerator <= 0.0 {
        return Some(0.0);
    }

    let covariance_numerator = series
        .windows(2)
        .map(|lag_pair| (lag_pair[0] - sample_mean) * (lag_pair[1] - sample_mean))
        .sum::<f64>();
    let autocorrelation = covariance_numerator / variance_numerator;

    autocorrelation.is_finite().then_some(autocorrelation)
}

fn compute_block_bootstrap_confidence_interval<T, F>(
    source_series: &[T],
    point_estimate: f64,
    bootstrap_configuration: ActivityClusteringBlockBootstrapConfiguration,
    bootstrap_seed: u64,
    metric_evaluator: F,
) -> ActivityClusteringConfidenceInterval
where
    T: Copy,
    F: Fn(&[T]) -> Option<f64>,
{
    if source_series.is_empty() {
        return ActivityClusteringConfidenceInterval {
            lower_bound: point_estimate,
            upper_bound: point_estimate,
            confidence_level: bootstrap_configuration.confidence_level,
        };
    }

    let mut bootstrap_stream = SplitMix64SimulationRandomnessStream::from_seed(bootstrap_seed);
    let mut bootstrap_replication_values: Vec<f64> =
        Vec::with_capacity(bootstrap_configuration.replication_count);
    for _ in 0..bootstrap_configuration.replication_count {
        let resampled_series = generate_block_bootstrap_resample(
            source_series,
            bootstrap_configuration.block_length_interval_count,
            &mut bootstrap_stream,
        );
        let Some(bootstrap_estimate) = metric_evaluator(&resampled_series) else {
            continue;
        };
        if bootstrap_estimate.is_finite() {
            bootstrap_replication_values.push(bootstrap_estimate);
        }
    }

    if bootstrap_replication_values.is_empty() {
        return ActivityClusteringConfidenceInterval {
            lower_bound: point_estimate,
            upper_bound: point_estimate,
            confidence_level: bootstrap_configuration.confidence_level,
        };
    }

    bootstrap_replication_values.sort_by(f64::total_cmp);

    let lower_tail_probability = (1.0 - bootstrap_configuration.confidence_level) * 0.5;
    let upper_tail_probability = 1.0 - lower_tail_probability;
    let lower_bound =
        compute_sorted_quantile(&bootstrap_replication_values, lower_tail_probability);
    let upper_bound =
        compute_sorted_quantile(&bootstrap_replication_values, upper_tail_probability);

    ActivityClusteringConfidenceInterval {
        lower_bound,
        upper_bound,
        confidence_level: bootstrap_configuration.confidence_level,
    }
}

fn generate_block_bootstrap_resample<T>(
    source_series: &[T],
    requested_block_length: usize,
    bootstrap_stream: &mut SplitMix64SimulationRandomnessStream,
) -> Vec<T>
where
    T: Copy,
{
    let sample_count = source_series.len();
    if sample_count == 0 {
        return Vec::new();
    }

    let block_length = requested_block_length.max(1).min(sample_count);

    let mut resampled_series: Vec<T> = Vec::with_capacity(sample_count);
    while resampled_series.len() < sample_count {
        let block_start_position = sample_uniform_position(sample_count, bootstrap_stream);
        for block_offset in 0..block_length {
            if resampled_series.len() >= sample_count {
                break;
            }
            let source_position = (block_start_position + block_offset) % sample_count;
            resampled_series.push(source_series[source_position]);
        }
    }

    resampled_series
}

fn sample_uniform_position(
    upper_bound: usize,
    bootstrap_stream: &mut SplitMix64SimulationRandomnessStream,
) -> usize {
    if upper_bound <= 1 {
        return 0;
    }

    let sampled_position =
        (bootstrap_stream.next_unit_interval_f64() * upper_bound as f64).floor() as usize;
    sampled_position.min(upper_bound - 1)
}

fn compute_sorted_quantile(sorted_values: &[f64], quantile_level: f64) -> f64 {
    if sorted_values.is_empty() {
        return f64::NAN;
    }
    if sorted_values.len() == 1 {
        return sorted_values[0];
    }

    let clamped_quantile = quantile_level.clamp(0.0, 1.0);
    let quantile_position = clamped_quantile * (sorted_values.len() - 1) as f64;
    let lower_position = quantile_position.floor() as usize;
    let upper_position = quantile_position.ceil() as usize;
    let interpolation_weight = quantile_position - lower_position as f64;
    let lower_value = sorted_values[lower_position];
    let upper_value = sorted_values[upper_position];

    lower_value + interpolation_weight * (upper_value - lower_value)
}

fn coverage_ratio(covered_interval_count: usize, horizon_interval_count: usize) -> f64 {
    if horizon_interval_count == 0 {
        return 0.0;
    }

    covered_interval_count as f64 / horizon_interval_count as f64
}

fn derive_metric_bootstrap_seed(
    root_seed: u64,
    horizon_position: usize,
    dataset_role: u64,
    metric_role: u64,
) -> u64 {
    let seed_mixed_by_horizon =
        root_seed ^ (horizon_position as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    let seed_mixed_by_dataset =
        seed_mixed_by_horizon ^ dataset_role.wrapping_mul(0xBF58_476D_1CE4_E5B9);

    splitmix64_output_mixer(seed_mixed_by_dataset ^ metric_role.wrapping_mul(0x94D0_49BB_1331_11EB))
}
