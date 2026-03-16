//! Volatility-persistence diagnostics for stylized-fact validation.

use std::collections::BTreeSet;
use std::error::Error;
use std::fmt::{Display, Formatter};

use serde::Serialize;

use super::diagnostics_shared::{
    BlockBootstrapConfiguration, build_delta_uncertainty_context,
    compute_block_bootstrap_confidence_interval, coverage_ratio, derive_metric_bootstrap_seed,
    validate_block_bootstrap_configuration,
};
use super::metric_catalog::{
    STYLIZED_FACT_METRIC_CATALOG_INSUFFICIENT_DATA_STATUS_CODE, StylizedFactMetricDomain,
    StylizedFactMetricHorizonApplicability, build_stylized_fact_metric_catalog,
};
use super::return_series_shared::{
    ReturnObservationLike, TimestampedReturnObservation, bucket_abs_returns_by_interval,
    compute_lag_autocorrelation, filter_sort_returns_in_window, find_non_finite_return_observation,
};
use super::time_buckets::{TimestampAnalysisWindow, interval_count_for_window};

const VOLATILITY_PERSISTENCE_METRIC_ID: &str = "volatility_abs_return_acf_distribution_distance";

pub const VOLATILITY_PERSISTENCE_DIAGNOSTICS_REPORT_SCHEMA_VERSION: &str =
    "validation.stylized_fact.volatility_persistence.diagnostics.v1";
pub const VOLATILITY_PERSISTENCE_DIAGNOSTICS_DETERMINISM_POLICY: &str =
    "stable_horizon_and_lag_input_order_with_seeded_block_bootstrap.v1";
pub const VOLATILITY_PERSISTENCE_DIAGNOSTICS_BOOTSTRAP_POLICY: &str =
    "moving_block_bootstrap_with_wraparound_and_percentile_intervals.v1";

#[derive(Debug, Clone, PartialEq)]
pub struct VolatilityPersistenceDiagnosticsRequest {
    pub analysis_start_utc_nanos: i64,
    pub analysis_end_utc_nanos: i64,
    pub empirical_dataset: VolatilityPersistenceDatasetInput,
    pub simulated_dataset: VolatilityPersistenceDatasetInput,
    pub horizon_buckets: Vec<VolatilityPersistenceHorizonBucket>,
    pub lag_settings: Vec<VolatilityPersistenceLagSetting>,
    pub bootstrap_configuration: VolatilityPersistenceBlockBootstrapConfiguration,
    pub sample_sufficiency_policy: VolatilityPersistenceSampleSufficiencyPolicy,
    pub threshold_policy: VolatilityPersistenceThresholdPolicy,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VolatilityPersistenceDatasetInput {
    pub dataset_id: String,
    pub return_observations: Vec<VolatilityPersistenceReturnObservation>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VolatilityPersistenceReturnObservation {
    pub timestamp_utc_nanos: i64,
    pub return_increment: f64,
}

impl ReturnObservationLike for VolatilityPersistenceReturnObservation {
    fn timestamp_utc_nanos(self) -> i64 {
        self.timestamp_utc_nanos
    }

    fn return_increment(self) -> f64 {
        self.return_increment
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VolatilityPersistenceHorizonBucket {
    pub horizon_label: String,
    pub horizon_duration_nanos: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VolatilityPersistenceLagSetting {
    pub lag_label: String,
    pub lag_interval_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VolatilityPersistenceBlockBootstrapConfiguration {
    pub confidence_level: f64,
    pub replication_count: usize,
    pub block_length_interval_count: usize,
    pub random_seed: u64,
}

impl Default for VolatilityPersistenceBlockBootstrapConfiguration {
    fn default() -> Self {
        Self {
            confidence_level: 0.95,
            replication_count: 2_000,
            block_length_interval_count: 5,
            random_seed: 8_424_201,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct VolatilityPersistenceMetricSampleSufficiencyPolicy {
    pub minimum_sample_count: usize,
    pub minimum_effective_sample_count: usize,
    pub minimum_tail_observation_count: usize,
    pub minimum_horizon_coverage_ratio: f64,
}

impl VolatilityPersistenceMetricSampleSufficiencyPolicy {
    fn validate(
        self,
        field_prefix: &'static str,
    ) -> Result<(), VolatilityPersistenceDiagnosticsError> {
        if self.minimum_sample_count == 0 {
            return Err(
                VolatilityPersistenceDiagnosticsError::InvalidSampleSufficiencyConfiguration {
                    field_name: format!("{field_prefix}.minimum_sample_count"),
                    details: "minimum_sample_count must be greater than 0".to_string(),
                },
            );
        }

        if self.minimum_effective_sample_count == 0
            || self.minimum_effective_sample_count > self.minimum_sample_count
        {
            return Err(
                VolatilityPersistenceDiagnosticsError::InvalidSampleSufficiencyConfiguration {
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
                VolatilityPersistenceDiagnosticsError::InvalidSampleSufficiencyConfiguration {
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
                VolatilityPersistenceDiagnosticsError::InvalidSampleSufficiencyConfiguration {
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
pub struct VolatilityPersistenceSampleSufficiencyPolicy {
    pub lagged_abs_return_autocorrelation: VolatilityPersistenceMetricSampleSufficiencyPolicy,
}

impl Default for VolatilityPersistenceSampleSufficiencyPolicy {
    fn default() -> Self {
        Self {
            lagged_abs_return_autocorrelation: VolatilityPersistenceMetricSampleSufficiencyPolicy {
                minimum_sample_count: 450,
                minimum_effective_sample_count: 320,
                minimum_tail_observation_count: 0,
                minimum_horizon_coverage_ratio: 0.75,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct VolatilityPersistenceThresholdPolicy {
    pub comparison_policy_id: String,
    pub lagged_abs_return_autocorrelation_max_absolute_delta: f64,
}

impl Default for VolatilityPersistenceThresholdPolicy {
    fn default() -> Self {
        Self {
            comparison_policy_id: "absolute_delta_threshold_policy.v1".to_string(),
            lagged_abs_return_autocorrelation_max_absolute_delta: 0.2,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VolatilityPersistenceDiagnosticsReport {
    pub schema_version: &'static str,
    pub determinism_policy: &'static str,
    pub bootstrap_policy: &'static str,
    pub insufficient_data_status_code: &'static str,
    pub metric_catalog_id: String,
    pub metric_catalog_version: String,
    pub metric_catalog_metric_id: String,
    pub metric_catalog_metric_name: String,
    pub empirical_dataset_id: String,
    pub simulated_dataset_id: String,
    pub analysis_start_utc_nanos: i64,
    pub analysis_end_utc_nanos: i64,
    pub horizons: Vec<VolatilityPersistenceHorizonDiagnostics>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VolatilityPersistenceHorizonDiagnostics {
    pub horizon_label: String,
    pub horizon_duration_nanos: i64,
    pub lag_comparisons: Vec<VolatilityPersistenceLagComparison>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VolatilityPersistenceMetricStatus {
    Available,
    InsufficientData,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct VolatilityPersistenceConfidenceInterval {
    pub lower_bound: f64,
    pub upper_bound: f64,
    pub confidence_level: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct VolatilityPersistenceSampleCoverage {
    pub horizon_interval_count: usize,
    pub covered_interval_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct VolatilityPersistenceObservedCounts {
    pub sample_count: usize,
    pub effective_sample_count: usize,
    pub tail_observation_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VolatilityPersistenceDatasetMetricResult {
    pub status: VolatilityPersistenceMetricStatus,
    pub value: Option<f64>,
    pub confidence_interval: Option<VolatilityPersistenceConfidenceInterval>,
    pub sample_coverage: VolatilityPersistenceSampleCoverage,
    pub observed_counts: VolatilityPersistenceObservedCounts,
    pub required_minimums: VolatilityPersistenceMetricSampleSufficiencyPolicy,
    pub horizon_coverage_ratio: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct VolatilityPersistenceDeltaUncertaintyContext {
    pub empirical_interval_half_width: Option<f64>,
    pub simulated_interval_half_width: Option<f64>,
    pub pooled_interval_half_width: Option<f64>,
    pub standardized_delta: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VolatilityPersistenceThresholdContext {
    pub comparison_policy_id: String,
    pub max_absolute_delta: f64,
    pub absolute_delta_exceeds_threshold: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VolatilityPersistenceLagComparison {
    pub metric_id: String,
    pub metric_name: String,
    pub lag_label: String,
    pub lag_interval_count: usize,
    pub status: VolatilityPersistenceMetricStatus,
    pub empirical: VolatilityPersistenceDatasetMetricResult,
    pub simulated: VolatilityPersistenceDatasetMetricResult,
    pub absolute_delta: Option<f64>,
    pub uncertainty_context: VolatilityPersistenceDeltaUncertaintyContext,
    pub threshold_context: VolatilityPersistenceThresholdContext,
}

#[derive(Debug, Clone, PartialEq)]
pub enum VolatilityPersistenceDiagnosticsError {
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
    EmptyLagSettingSet,
    EmptyLagLabel {
        lag_position: usize,
    },
    DuplicateLagLabel {
        lag_label: String,
    },
    DuplicateLagIntervalCount {
        lag_interval_count: usize,
    },
    InvalidLagIntervalCount {
        lag_label: String,
        lag_interval_count: usize,
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
    MetricCatalogMetricMissing {
        metric_id: &'static str,
    },
    MetricCatalogMetricInvalid {
        metric_id: &'static str,
        details: String,
    },
}

impl Display for VolatilityPersistenceDiagnosticsError {
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
                "volatility persistence diagnostics requires at least one horizon bucket"
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
            Self::EmptyLagSettingSet => write!(
                formatter,
                "volatility persistence diagnostics requires at least one lag setting"
            ),
            Self::EmptyLagLabel { lag_position } => write!(
                formatter,
                "lag setting at position {lag_position} must define a non-empty lag_label"
            ),
            Self::DuplicateLagLabel { lag_label } => {
                write!(
                    formatter,
                    "lag_label `{lag_label}` is duplicated in request"
                )
            }
            Self::DuplicateLagIntervalCount { lag_interval_count } => write!(
                formatter,
                "lag_interval_count `{lag_interval_count}` is duplicated in request"
            ),
            Self::InvalidLagIntervalCount {
                lag_label,
                lag_interval_count,
            } => write!(
                formatter,
                "lag `{lag_label}` must have lag_interval_count greater than 0, found {lag_interval_count}"
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
            Self::MetricCatalogMetricMissing { metric_id } => write!(
                formatter,
                "metric catalog does not define required volatility-persistence metric `{metric_id}`"
            ),
            Self::MetricCatalogMetricInvalid { metric_id, details } => write!(
                formatter,
                "metric catalog metric `{metric_id}` is invalid for volatility persistence diagnostics: {details}"
            ),
        }
    }
}

impl Error for VolatilityPersistenceDiagnosticsError {}

#[derive(Debug, Clone, PartialEq, Eq)]
struct VolatilityPersistenceMetricCatalogContext {
    catalog_id: String,
    catalog_version: String,
    metric_id: String,
    metric_name: String,
}

pub fn run_volatility_persistence_diagnostics(
    request: VolatilityPersistenceDiagnosticsRequest,
) -> Result<VolatilityPersistenceDiagnosticsReport, VolatilityPersistenceDiagnosticsError> {
    validate_request(&request)?;

    let metric_catalog_context = resolve_metric_catalog_context()?;

    let analysis_window = TimestampAnalysisWindow::new(
        request.analysis_start_utc_nanos,
        request.analysis_end_utc_nanos,
    );

    let empirical_dataset =
        prepare_dataset("empirical", &request.empirical_dataset, analysis_window)?;
    let simulated_dataset =
        prepare_dataset("simulated", &request.simulated_dataset, analysis_window)?;

    let mut horizon_reports: Vec<VolatilityPersistenceHorizonDiagnostics> =
        Vec::with_capacity(request.horizon_buckets.len());
    for (horizon_position, horizon_bucket) in request.horizon_buckets.iter().enumerate() {
        let empirical_horizon_series =
            build_horizon_dataset_series(&empirical_dataset, analysis_window, horizon_bucket);
        let simulated_horizon_series =
            build_horizon_dataset_series(&simulated_dataset, analysis_window, horizon_bucket);

        let mut lag_comparisons: Vec<VolatilityPersistenceLagComparison> =
            Vec::with_capacity(request.lag_settings.len());

        for (lag_position, lag_setting) in request.lag_settings.iter().enumerate() {
            let empirical_metric = evaluate_lagged_autocorrelation_metric(
                &empirical_horizon_series,
                lag_setting.lag_interval_count,
                request
                    .sample_sufficiency_policy
                    .lagged_abs_return_autocorrelation,
                request.bootstrap_configuration,
                derive_metric_bootstrap_seed(
                    request.bootstrap_configuration.random_seed,
                    horizon_position,
                    1,
                    1_000 + lag_position as u64,
                ),
            );
            let simulated_metric = evaluate_lagged_autocorrelation_metric(
                &simulated_horizon_series,
                lag_setting.lag_interval_count,
                request
                    .sample_sufficiency_policy
                    .lagged_abs_return_autocorrelation,
                request.bootstrap_configuration,
                derive_metric_bootstrap_seed(
                    request.bootstrap_configuration.random_seed,
                    horizon_position,
                    2,
                    1_000 + lag_position as u64,
                ),
            );

            lag_comparisons.push(build_lag_comparison(
                &metric_catalog_context.metric_id,
                &metric_catalog_context.metric_name,
                lag_setting,
                empirical_metric,
                simulated_metric,
                request
                    .threshold_policy
                    .lagged_abs_return_autocorrelation_max_absolute_delta,
                &request.threshold_policy.comparison_policy_id,
            ));
        }

        horizon_reports.push(VolatilityPersistenceHorizonDiagnostics {
            horizon_label: horizon_bucket.horizon_label.clone(),
            horizon_duration_nanos: horizon_bucket.horizon_duration_nanos,
            lag_comparisons,
        });
    }

    Ok(VolatilityPersistenceDiagnosticsReport {
        schema_version: VOLATILITY_PERSISTENCE_DIAGNOSTICS_REPORT_SCHEMA_VERSION,
        determinism_policy: VOLATILITY_PERSISTENCE_DIAGNOSTICS_DETERMINISM_POLICY,
        bootstrap_policy: VOLATILITY_PERSISTENCE_DIAGNOSTICS_BOOTSTRAP_POLICY,
        insufficient_data_status_code: STYLIZED_FACT_METRIC_CATALOG_INSUFFICIENT_DATA_STATUS_CODE,
        metric_catalog_id: metric_catalog_context.catalog_id,
        metric_catalog_version: metric_catalog_context.catalog_version,
        metric_catalog_metric_id: metric_catalog_context.metric_id,
        metric_catalog_metric_name: metric_catalog_context.metric_name,
        empirical_dataset_id: empirical_dataset.dataset_id,
        simulated_dataset_id: simulated_dataset.dataset_id,
        analysis_start_utc_nanos: request.analysis_start_utc_nanos,
        analysis_end_utc_nanos: request.analysis_end_utc_nanos,
        horizons: horizon_reports,
    })
}

#[derive(Debug, Clone, PartialEq)]
struct PreparedVolatilityPersistenceDataset {
    dataset_id: String,
    return_observations_in_window: Vec<TimestampedReturnObservation>,
}

#[derive(Debug, Clone, PartialEq)]
struct VolatilityPersistenceHorizonDatasetSeries {
    interval_count: usize,
    abs_return_by_interval: Vec<f64>,
    covered_interval_count: usize,
}

fn validate_request(
    request: &VolatilityPersistenceDiagnosticsRequest,
) -> Result<(), VolatilityPersistenceDiagnosticsError> {
    if request.analysis_start_utc_nanos >= request.analysis_end_utc_nanos {
        return Err(
            VolatilityPersistenceDiagnosticsError::InvalidAnalysisWindow {
                analysis_start_utc_nanos: request.analysis_start_utc_nanos,
                analysis_end_utc_nanos: request.analysis_end_utc_nanos,
            },
        );
    }

    validate_dataset_id("empirical", &request.empirical_dataset.dataset_id)?;
    validate_dataset_id("simulated", &request.simulated_dataset.dataset_id)?;

    if request.horizon_buckets.is_empty() {
        return Err(VolatilityPersistenceDiagnosticsError::EmptyHorizonBucketSet);
    }

    let mut observed_horizon_labels: BTreeSet<&str> = BTreeSet::new();
    for (horizon_position, horizon_bucket) in request.horizon_buckets.iter().enumerate() {
        if horizon_bucket.horizon_label.trim().is_empty() {
            return Err(VolatilityPersistenceDiagnosticsError::EmptyHorizonLabel {
                horizon_position,
            });
        }
        if horizon_bucket.horizon_duration_nanos <= 0 {
            return Err(
                VolatilityPersistenceDiagnosticsError::InvalidHorizonDuration {
                    horizon_label: horizon_bucket.horizon_label.clone(),
                    horizon_duration_nanos: horizon_bucket.horizon_duration_nanos,
                },
            );
        }
        if !observed_horizon_labels.insert(horizon_bucket.horizon_label.as_str()) {
            return Err(
                VolatilityPersistenceDiagnosticsError::DuplicateHorizonLabel {
                    horizon_label: horizon_bucket.horizon_label.clone(),
                },
            );
        }
    }

    if request.lag_settings.is_empty() {
        return Err(VolatilityPersistenceDiagnosticsError::EmptyLagSettingSet);
    }

    let mut observed_lag_labels: BTreeSet<&str> = BTreeSet::new();
    let mut observed_lag_interval_counts: BTreeSet<usize> = BTreeSet::new();
    for (lag_position, lag_setting) in request.lag_settings.iter().enumerate() {
        if lag_setting.lag_label.trim().is_empty() {
            return Err(VolatilityPersistenceDiagnosticsError::EmptyLagLabel { lag_position });
        }
        if lag_setting.lag_interval_count == 0 {
            return Err(
                VolatilityPersistenceDiagnosticsError::InvalidLagIntervalCount {
                    lag_label: lag_setting.lag_label.clone(),
                    lag_interval_count: lag_setting.lag_interval_count,
                },
            );
        }
        if !observed_lag_labels.insert(lag_setting.lag_label.as_str()) {
            return Err(VolatilityPersistenceDiagnosticsError::DuplicateLagLabel {
                lag_label: lag_setting.lag_label.clone(),
            });
        }
        if !observed_lag_interval_counts.insert(lag_setting.lag_interval_count) {
            return Err(
                VolatilityPersistenceDiagnosticsError::DuplicateLagIntervalCount {
                    lag_interval_count: lag_setting.lag_interval_count,
                },
            );
        }
    }

    validate_bootstrap_configuration(request.bootstrap_configuration)?;
    request
        .sample_sufficiency_policy
        .lagged_abs_return_autocorrelation
        .validate("lagged_abs_return_autocorrelation")?;
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

fn resolve_metric_catalog_context()
-> Result<VolatilityPersistenceMetricCatalogContext, VolatilityPersistenceDiagnosticsError> {
    let catalog = build_stylized_fact_metric_catalog();
    let metric = catalog
        .metrics
        .iter()
        .find(|metric_definition| metric_definition.metric_id == VOLATILITY_PERSISTENCE_METRIC_ID)
        .ok_or(
            VolatilityPersistenceDiagnosticsError::MetricCatalogMetricMissing {
                metric_id: VOLATILITY_PERSISTENCE_METRIC_ID,
            },
        )?;

    if metric.domain != StylizedFactMetricDomain::VolatilityPersistence {
        return Err(
            VolatilityPersistenceDiagnosticsError::MetricCatalogMetricInvalid {
                metric_id: VOLATILITY_PERSISTENCE_METRIC_ID,
                details: "metric domain must be `volatility_persistence`".to_string(),
            },
        );
    }
    if metric.horizon_applicability != StylizedFactMetricHorizonApplicability::HorizonAndLagGrid {
        return Err(
            VolatilityPersistenceDiagnosticsError::MetricCatalogMetricInvalid {
                metric_id: VOLATILITY_PERSISTENCE_METRIC_ID,
                details: "horizon_applicability must be `horizon_and_lag_grid`".to_string(),
            },
        );
    }

    Ok(VolatilityPersistenceMetricCatalogContext {
        catalog_id: catalog.catalog_id,
        catalog_version: catalog.catalog_version,
        metric_id: metric.metric_id.clone(),
        metric_name: metric.metric_name.clone(),
    })
}

fn validate_dataset_id(
    dataset_role: &'static str,
    dataset_id: &str,
) -> Result<(), VolatilityPersistenceDiagnosticsError> {
    if dataset_id.trim().is_empty() {
        return Err(VolatilityPersistenceDiagnosticsError::EmptyDatasetId { dataset_role });
    }

    Ok(())
}

fn validate_bootstrap_configuration(
    bootstrap_configuration: VolatilityPersistenceBlockBootstrapConfiguration,
) -> Result<(), VolatilityPersistenceDiagnosticsError> {
    if let Err(field_name) = validate_block_bootstrap_configuration(block_bootstrap_configuration(
        bootstrap_configuration,
    )) {
        let details = match field_name {
            "confidence_level" => format!(
                "confidence_level must be finite and in (0, 1), found {}",
                bootstrap_configuration.confidence_level
            ),
            "replication_count" => "replication_count must be greater than 0".to_string(),
            "block_length_interval_count" => {
                "block_length_interval_count must be greater than 0".to_string()
            }
            _ => "invalid bootstrap configuration".to_string(),
        };

        return Err(
            VolatilityPersistenceDiagnosticsError::InvalidBootstrapConfiguration {
                field_name,
                details,
            },
        );
    }

    Ok(())
}

fn block_bootstrap_configuration(
    bootstrap_configuration: VolatilityPersistenceBlockBootstrapConfiguration,
) -> BlockBootstrapConfiguration {
    BlockBootstrapConfiguration {
        confidence_level: bootstrap_configuration.confidence_level,
        replication_count: bootstrap_configuration.replication_count,
        block_length_interval_count: bootstrap_configuration.block_length_interval_count,
    }
}

fn validate_threshold_policy(
    threshold_policy: &VolatilityPersistenceThresholdPolicy,
) -> Result<(), VolatilityPersistenceDiagnosticsError> {
    if threshold_policy.comparison_policy_id.trim().is_empty() {
        return Err(
            VolatilityPersistenceDiagnosticsError::InvalidThresholdPolicy {
                field_name: "comparison_policy_id",
                details: "comparison_policy_id must be non-empty".to_string(),
            },
        );
    }

    validate_non_negative_finite_threshold(
        "lagged_abs_return_autocorrelation_max_absolute_delta",
        threshold_policy.lagged_abs_return_autocorrelation_max_absolute_delta,
    )?;

    Ok(())
}

fn validate_non_negative_finite_threshold(
    field_name: &'static str,
    threshold_value: f64,
) -> Result<(), VolatilityPersistenceDiagnosticsError> {
    if !threshold_value.is_finite() || threshold_value < 0.0 {
        return Err(
            VolatilityPersistenceDiagnosticsError::InvalidThresholdPolicy {
                field_name,
                details: format!(
                    "threshold must be finite and non-negative, found {}",
                    threshold_value
                ),
            },
        );
    }

    Ok(())
}

fn validate_return_observations(
    dataset_role: &'static str,
    dataset_id: &str,
    return_observations: &[VolatilityPersistenceReturnObservation],
) -> Result<(), VolatilityPersistenceDiagnosticsError> {
    if let Some((observation_position, return_increment)) =
        find_non_finite_return_observation(return_observations)
    {
        return Err(
            VolatilityPersistenceDiagnosticsError::InvalidReturnObservation {
                dataset_role,
                dataset_id: dataset_id.to_string(),
                observation_position,
                return_increment,
            },
        );
    }

    Ok(())
}

fn prepare_dataset(
    dataset_role: &'static str,
    dataset: &VolatilityPersistenceDatasetInput,
    analysis_window: TimestampAnalysisWindow,
) -> Result<PreparedVolatilityPersistenceDataset, VolatilityPersistenceDiagnosticsError> {
    validate_dataset_id(dataset_role, &dataset.dataset_id)?;
    validate_return_observations(
        dataset_role,
        &dataset.dataset_id,
        &dataset.return_observations,
    )?;

    let return_observations_in_window =
        filter_sort_returns_in_window(&dataset.return_observations, analysis_window);

    Ok(PreparedVolatilityPersistenceDataset {
        dataset_id: dataset.dataset_id.clone(),
        return_observations_in_window,
    })
}

fn build_horizon_dataset_series(
    prepared_dataset: &PreparedVolatilityPersistenceDataset,
    analysis_window: TimestampAnalysisWindow,
    horizon_bucket: &VolatilityPersistenceHorizonBucket,
) -> VolatilityPersistenceHorizonDatasetSeries {
    let interval_count =
        interval_count_for_window(analysis_window, horizon_bucket.horizon_duration_nanos)
            .unwrap_or(0);
    let bucketed_abs_return_series = bucket_abs_returns_by_interval(
        &prepared_dataset.return_observations_in_window,
        analysis_window,
        horizon_bucket.horizon_duration_nanos,
        interval_count,
    );

    VolatilityPersistenceHorizonDatasetSeries {
        interval_count,
        abs_return_by_interval: bucketed_abs_return_series.abs_return_by_interval,
        covered_interval_count: bucketed_abs_return_series.covered_interval_count,
    }
}

fn evaluate_lagged_autocorrelation_metric(
    dataset_series: &VolatilityPersistenceHorizonDatasetSeries,
    lag_interval_count: usize,
    required_minimums: VolatilityPersistenceMetricSampleSufficiencyPolicy,
    bootstrap_configuration: VolatilityPersistenceBlockBootstrapConfiguration,
    bootstrap_seed: u64,
) -> VolatilityPersistenceDatasetMetricResult {
    let value =
        compute_lag_autocorrelation(&dataset_series.abs_return_by_interval, lag_interval_count);

    let observed_counts = VolatilityPersistenceObservedCounts {
        sample_count: dataset_series.abs_return_by_interval.len(),
        effective_sample_count: dataset_series
            .abs_return_by_interval
            .len()
            .saturating_sub(lag_interval_count),
        tail_observation_count: 0,
    };

    let sample_coverage = VolatilityPersistenceSampleCoverage {
        horizon_interval_count: dataset_series.interval_count,
        covered_interval_count: dataset_series.covered_interval_count,
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

    let confidence_interval = if status == VolatilityPersistenceMetricStatus::Available {
        value.map(|point_estimate| {
            let (lower_bound, upper_bound) = compute_block_bootstrap_confidence_interval(
                &dataset_series.abs_return_by_interval,
                point_estimate,
                block_bootstrap_configuration(bootstrap_configuration),
                bootstrap_seed,
                |resampled_abs_returns| {
                    compute_lag_autocorrelation(resampled_abs_returns, lag_interval_count)
                },
            );
            VolatilityPersistenceConfidenceInterval {
                lower_bound,
                upper_bound,
                confidence_level: bootstrap_configuration.confidence_level,
            }
        })
    } else {
        None
    };

    VolatilityPersistenceDatasetMetricResult {
        status,
        value,
        confidence_interval,
        sample_coverage,
        observed_counts,
        required_minimums,
        horizon_coverage_ratio,
    }
}

fn build_lag_comparison(
    metric_id: &str,
    metric_name: &str,
    lag_setting: &VolatilityPersistenceLagSetting,
    empirical: VolatilityPersistenceDatasetMetricResult,
    simulated: VolatilityPersistenceDatasetMetricResult,
    max_absolute_delta: f64,
    comparison_policy_id: &str,
) -> VolatilityPersistenceLagComparison {
    let absolute_delta = match (
        empirical.status,
        empirical.value,
        simulated.status,
        simulated.value,
    ) {
        (
            VolatilityPersistenceMetricStatus::Available,
            Some(empirical_value),
            VolatilityPersistenceMetricStatus::Available,
            Some(simulated_value),
        ) => Some((empirical_value - simulated_value).abs()),
        _ => None,
    };

    let status = if absolute_delta.is_some() {
        VolatilityPersistenceMetricStatus::Available
    } else {
        VolatilityPersistenceMetricStatus::InsufficientData
    };

    let uncertainty_context = build_delta_uncertainty_context(
        empirical.confidence_interval.map(|confidence_interval| {
            (
                confidence_interval.lower_bound,
                confidence_interval.upper_bound,
            )
        }),
        simulated.confidence_interval.map(|confidence_interval| {
            (
                confidence_interval.lower_bound,
                confidence_interval.upper_bound,
            )
        }),
        absolute_delta,
        |uncertainty_components| VolatilityPersistenceDeltaUncertaintyContext {
            empirical_interval_half_width: uncertainty_components.empirical_interval_half_width,
            simulated_interval_half_width: uncertainty_components.simulated_interval_half_width,
            pooled_interval_half_width: uncertainty_components.pooled_interval_half_width,
            standardized_delta: uncertainty_components.standardized_delta,
        },
    );

    let absolute_delta_exceeds_threshold = absolute_delta.map(|delta| delta > max_absolute_delta);

    VolatilityPersistenceLagComparison {
        metric_id: metric_id.to_string(),
        metric_name: metric_name.to_string(),
        lag_label: lag_setting.lag_label.clone(),
        lag_interval_count: lag_setting.lag_interval_count,
        status,
        empirical,
        simulated,
        absolute_delta,
        uncertainty_context,
        threshold_context: VolatilityPersistenceThresholdContext {
            comparison_policy_id: comparison_policy_id.to_string(),
            max_absolute_delta,
            absolute_delta_exceeds_threshold,
        },
    }
}

fn resolve_metric_status(
    value: Option<f64>,
    observed_counts: VolatilityPersistenceObservedCounts,
    horizon_coverage_ratio: f64,
    required_minimums: VolatilityPersistenceMetricSampleSufficiencyPolicy,
) -> VolatilityPersistenceMetricStatus {
    let has_sufficient_counts = observed_counts.sample_count
        >= required_minimums.minimum_sample_count
        && observed_counts.effective_sample_count
            >= required_minimums.minimum_effective_sample_count
        && observed_counts.tail_observation_count
            >= required_minimums.minimum_tail_observation_count;

    let has_sufficient_coverage =
        horizon_coverage_ratio >= required_minimums.minimum_horizon_coverage_ratio;

    if value.is_some() && has_sufficient_counts && has_sufficient_coverage {
        VolatilityPersistenceMetricStatus::Available
    } else {
        VolatilityPersistenceMetricStatus::InsufficientData
    }
}
