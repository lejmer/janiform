//! Tail-heaviness and skew diagnostics for stylized-fact validation.

use std::collections::BTreeSet;
use std::error::Error;
use std::fmt::{Display, Formatter};

use janiform_foundations::numeric::compute_probability_weighted_moment;
use serde::Serialize;

use super::diagnostics_shared::{
    BlockBootstrapConfiguration, compute_block_bootstrap_confidence_interval,
    compute_sorted_quantile, coverage_ratio, derive_metric_bootstrap_seed,
    validate_block_bootstrap_configuration,
};
use super::metric_catalog::STYLIZED_FACT_METRIC_CATALOG_INSUFFICIENT_DATA_STATUS_CODE;
use super::time_buckets::{
    TimestampAnalysisWindow, interval_count_for_window, interval_position_for_timestamp,
};

const RETURNS_ABS_QUANTILE_SHAPE_RATIO_Q99_Q95_METRIC_ID: &str =
    "returns_abs_quantile_shape_ratio_q99_q95";
const RETURNS_ABS_QUANTILE_SHAPE_RATIO_Q99_Q95_METRIC_NAME: &str =
    "Returns Abs Quantile Shape Ratio Q99 Q95";
const RETURNS_LEFT_TAIL_EXPECTED_SHORTFALL_TO_VAR_RATIO_METRIC_ID: &str =
    "returns_left_tail_expected_shortfall_to_var_ratio";
const RETURNS_LEFT_TAIL_EXPECTED_SHORTFALL_TO_VAR_RATIO_METRIC_NAME: &str =
    "Returns Left Tail Expected Shortfall To VaR Ratio";
const RETURNS_BOWLEY_SKEWNESS_QUANTILE_METRIC_ID: &str = "returns_bowley_skewness_quantile";
const RETURNS_BOWLEY_SKEWNESS_QUANTILE_METRIC_NAME: &str = "Returns Bowley Skewness Quantile";
const RETURNS_SIGNED_TAIL_EXPECTED_SHORTFALL_IMBALANCE_METRIC_ID: &str =
    "returns_signed_tail_expected_shortfall_imbalance";
const RETURNS_SIGNED_TAIL_EXPECTED_SHORTFALL_IMBALANCE_METRIC_NAME: &str =
    "Returns Signed Tail Expected Shortfall Imbalance";
const RETURNS_L_MOMENT_LAMBDA2_METRIC_ID: &str = "returns_l_moment_lambda2";
const RETURNS_L_MOMENT_LAMBDA2_METRIC_NAME: &str = "Returns L Moment Lambda2";
const RETURNS_L_MOMENT_TAU3_METRIC_ID: &str = "returns_l_moment_tau3";
const RETURNS_L_MOMENT_TAU3_METRIC_NAME: &str = "Returns L Moment Tau3";
const RETURNS_CENTRAL_VARIANCE_METRIC_ID: &str = "returns_central_variance";
const RETURNS_CENTRAL_VARIANCE_METRIC_NAME: &str = "Returns Central Variance";
const RETURNS_CENTRAL_SKEWNESS_METRIC_ID: &str = "returns_central_skewness";
const RETURNS_CENTRAL_SKEWNESS_METRIC_NAME: &str = "Returns Central Skewness";
const RETURNS_CENTRAL_EXCESS_KURTOSIS_METRIC_ID: &str = "returns_central_excess_kurtosis";
const RETURNS_CENTRAL_EXCESS_KURTOSIS_METRIC_NAME: &str = "Returns Central Excess Kurtosis";

const LEFT_TAIL_QUANTILE_LEVEL: f64 = 0.05;
const RIGHT_TAIL_QUANTILE_LEVEL: f64 = 0.95;
const EXTREME_RIGHT_TAIL_QUANTILE_LEVEL: f64 = 0.99;
const LOWER_QUARTILE_LEVEL: f64 = 0.25;
const MEDIAN_QUANTILE_LEVEL: f64 = 0.50;
const UPPER_QUARTILE_LEVEL: f64 = 0.75;
const MINIMUM_DENOMINATOR_MAGNITUDE: f64 = 1e-12;

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
    fn validate(self, field_prefix: &'static str) -> Result<(), TailSkewDiagnosticsError> {
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
        }
    }
}

impl Error for TailSkewDiagnosticsError {}

pub fn run_tail_skew_diagnostics(
    request: TailSkewDiagnosticsRequest,
) -> Result<TailSkewDiagnosticsReport, TailSkewDiagnosticsError> {
    validate_request(&request)?;

    let analysis_window = TimestampAnalysisWindow::new(
        request.analysis_start_utc_nanos,
        request.analysis_end_utc_nanos,
    );
    let empirical_dataset =
        prepare_dataset("empirical", &request.empirical_dataset, analysis_window)?;
    let simulated_dataset =
        prepare_dataset("simulated", &request.simulated_dataset, analysis_window)?;

    let horizon_reports: Vec<TailSkewHorizonDiagnostics> = request
        .horizon_buckets
        .iter()
        .enumerate()
        .map(|(horizon_position, horizon_bucket)| {
            build_horizon_diagnostics(
                horizon_position,
                horizon_bucket,
                &empirical_dataset,
                &simulated_dataset,
                analysis_window,
                request.sample_sufficiency_policy,
                request.bootstrap_configuration,
            )
        })
        .collect();

    Ok(TailSkewDiagnosticsReport {
        schema_version: TAIL_SKEW_DIAGNOSTICS_REPORT_SCHEMA_VERSION,
        determinism_policy: TAIL_SKEW_DIAGNOSTICS_DETERMINISM_POLICY,
        bootstrap_policy: TAIL_SKEW_DIAGNOSTICS_BOOTSTRAP_POLICY,
        insufficient_data_status_code: STYLIZED_FACT_METRIC_CATALOG_INSUFFICIENT_DATA_STATUS_CODE,
        empirical_dataset_id: empirical_dataset.dataset_id,
        simulated_dataset_id: simulated_dataset.dataset_id,
        analysis_start_utc_nanos: request.analysis_start_utc_nanos,
        analysis_end_utc_nanos: request.analysis_end_utc_nanos,
        horizons: horizon_reports,
    })
}

#[derive(Debug, Clone, PartialEq)]
struct PreparedTailSkewDataset {
    dataset_id: String,
    return_observations_in_window: Vec<TailSkewReturnObservation>,
}

#[derive(Debug, Clone, PartialEq)]
struct HorizonReturnSeries {
    interval_count: usize,
    covered_interval_count: usize,
    interval_returns: Vec<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct TailSkewMetricComputation {
    value: Option<f64>,
    effective_sample_count: usize,
    tail_observation_count: usize,
}

type TailSkewMetricEvaluator = fn(&[f64]) -> TailSkewMetricComputation;

#[derive(Debug, Clone, Copy)]
struct TailSkewMetricSpecification {
    metric_id: &'static str,
    metric_name: &'static str,
    metric_class: TailSkewMetricClass,
    metric_role: u64,
    sample_sufficiency_selector:
        fn(TailSkewSampleSufficiencyPolicy) -> TailSkewMetricSampleSufficiencyPolicy,
    metric_evaluator: TailSkewMetricEvaluator,
}

const TAIL_SKEW_METRIC_SPECIFICATIONS: [TailSkewMetricSpecification; 9] = [
    TailSkewMetricSpecification {
        metric_id: RETURNS_ABS_QUANTILE_SHAPE_RATIO_Q99_Q95_METRIC_ID,
        metric_name: RETURNS_ABS_QUANTILE_SHAPE_RATIO_Q99_Q95_METRIC_NAME,
        metric_class: TailSkewMetricClass::PrimaryRobust,
        metric_role: 101,
        sample_sufficiency_selector: select_abs_quantile_shape_ratio_q99_q95,
        metric_evaluator: compute_abs_quantile_shape_ratio_q99_q95,
    },
    TailSkewMetricSpecification {
        metric_id: RETURNS_LEFT_TAIL_EXPECTED_SHORTFALL_TO_VAR_RATIO_METRIC_ID,
        metric_name: RETURNS_LEFT_TAIL_EXPECTED_SHORTFALL_TO_VAR_RATIO_METRIC_NAME,
        metric_class: TailSkewMetricClass::PrimaryRobust,
        metric_role: 102,
        sample_sufficiency_selector: select_left_tail_expected_shortfall_to_var_ratio,
        metric_evaluator: compute_left_tail_expected_shortfall_to_var_ratio,
    },
    TailSkewMetricSpecification {
        metric_id: RETURNS_BOWLEY_SKEWNESS_QUANTILE_METRIC_ID,
        metric_name: RETURNS_BOWLEY_SKEWNESS_QUANTILE_METRIC_NAME,
        metric_class: TailSkewMetricClass::PrimaryRobust,
        metric_role: 103,
        sample_sufficiency_selector: select_bowley_skewness_quantile,
        metric_evaluator: compute_bowley_skewness_quantile,
    },
    TailSkewMetricSpecification {
        metric_id: RETURNS_SIGNED_TAIL_EXPECTED_SHORTFALL_IMBALANCE_METRIC_ID,
        metric_name: RETURNS_SIGNED_TAIL_EXPECTED_SHORTFALL_IMBALANCE_METRIC_NAME,
        metric_class: TailSkewMetricClass::PrimaryRobust,
        metric_role: 104,
        sample_sufficiency_selector: select_signed_tail_expected_shortfall_imbalance,
        metric_evaluator: compute_signed_tail_expected_shortfall_imbalance,
    },
    TailSkewMetricSpecification {
        metric_id: RETURNS_L_MOMENT_LAMBDA2_METRIC_ID,
        metric_name: RETURNS_L_MOMENT_LAMBDA2_METRIC_NAME,
        metric_class: TailSkewMetricClass::SecondaryMoment,
        metric_role: 105,
        sample_sufficiency_selector: select_l_moment_lambda2,
        metric_evaluator: compute_l_moment_lambda2,
    },
    TailSkewMetricSpecification {
        metric_id: RETURNS_L_MOMENT_TAU3_METRIC_ID,
        metric_name: RETURNS_L_MOMENT_TAU3_METRIC_NAME,
        metric_class: TailSkewMetricClass::SecondaryMoment,
        metric_role: 106,
        sample_sufficiency_selector: select_l_moment_tau3,
        metric_evaluator: compute_l_moment_tau3,
    },
    TailSkewMetricSpecification {
        metric_id: RETURNS_CENTRAL_VARIANCE_METRIC_ID,
        metric_name: RETURNS_CENTRAL_VARIANCE_METRIC_NAME,
        metric_class: TailSkewMetricClass::SecondaryMoment,
        metric_role: 107,
        sample_sufficiency_selector: select_central_variance,
        metric_evaluator: compute_central_variance,
    },
    TailSkewMetricSpecification {
        metric_id: RETURNS_CENTRAL_SKEWNESS_METRIC_ID,
        metric_name: RETURNS_CENTRAL_SKEWNESS_METRIC_NAME,
        metric_class: TailSkewMetricClass::SecondaryMoment,
        metric_role: 108,
        sample_sufficiency_selector: select_central_skewness,
        metric_evaluator: compute_central_skewness,
    },
    TailSkewMetricSpecification {
        metric_id: RETURNS_CENTRAL_EXCESS_KURTOSIS_METRIC_ID,
        metric_name: RETURNS_CENTRAL_EXCESS_KURTOSIS_METRIC_NAME,
        metric_class: TailSkewMetricClass::SecondaryMoment,
        metric_role: 109,
        sample_sufficiency_selector: select_central_excess_kurtosis,
        metric_evaluator: compute_central_excess_kurtosis,
    },
];

fn build_horizon_diagnostics(
    horizon_position: usize,
    horizon_bucket: &TailSkewHorizonBucket,
    empirical_dataset: &PreparedTailSkewDataset,
    simulated_dataset: &PreparedTailSkewDataset,
    analysis_window: TimestampAnalysisWindow,
    sample_sufficiency_policy: TailSkewSampleSufficiencyPolicy,
    bootstrap_configuration: TailSkewBlockBootstrapConfiguration,
) -> TailSkewHorizonDiagnostics {
    let empirical_horizon_series =
        build_horizon_return_series(empirical_dataset, analysis_window, horizon_bucket);
    let simulated_horizon_series =
        build_horizon_return_series(simulated_dataset, analysis_window, horizon_bucket);

    let sample_coverage_empirical = TailSkewSampleCoverage {
        horizon_interval_count: empirical_horizon_series.interval_count,
        covered_interval_count: empirical_horizon_series.covered_interval_count,
    };
    let sample_coverage_simulated = TailSkewSampleCoverage {
        horizon_interval_count: simulated_horizon_series.interval_count,
        covered_interval_count: simulated_horizon_series.covered_interval_count,
    };

    let metrics = TAIL_SKEW_METRIC_SPECIFICATIONS
        .iter()
        .map(|metric_specification| {
            build_metric_comparison_for_specification(
                *metric_specification,
                &empirical_horizon_series.interval_returns,
                &simulated_horizon_series.interval_returns,
                sample_coverage_empirical,
                sample_coverage_simulated,
                sample_sufficiency_policy,
                bootstrap_configuration,
                horizon_position,
            )
        })
        .collect();

    TailSkewHorizonDiagnostics {
        horizon_label: horizon_bucket.horizon_label.clone(),
        horizon_duration_nanos: horizon_bucket.horizon_duration_nanos,
        metrics,
    }
}

#[allow(clippy::too_many_arguments)]
fn build_metric_comparison_for_specification(
    metric_specification: TailSkewMetricSpecification,
    empirical_interval_returns: &[f64],
    simulated_interval_returns: &[f64],
    empirical_sample_coverage: TailSkewSampleCoverage,
    simulated_sample_coverage: TailSkewSampleCoverage,
    sample_sufficiency_policy: TailSkewSampleSufficiencyPolicy,
    bootstrap_configuration: TailSkewBlockBootstrapConfiguration,
    horizon_position: usize,
) -> TailSkewMetricComparison {
    let required_minimums =
        (metric_specification.sample_sufficiency_selector)(sample_sufficiency_policy);

    let empirical = evaluate_metric(
        empirical_interval_returns,
        empirical_sample_coverage,
        required_minimums,
        bootstrap_configuration,
        derive_metric_bootstrap_seed(
            bootstrap_configuration.random_seed,
            horizon_position,
            1,
            metric_specification.metric_role,
        ),
        metric_specification.metric_evaluator,
    );

    let simulated = evaluate_metric(
        simulated_interval_returns,
        simulated_sample_coverage,
        required_minimums,
        bootstrap_configuration,
        derive_metric_bootstrap_seed(
            bootstrap_configuration.random_seed,
            horizon_position,
            2,
            metric_specification.metric_role,
        ),
        metric_specification.metric_evaluator,
    );

    build_metric_comparison(
        metric_specification.metric_id,
        metric_specification.metric_name,
        metric_specification.metric_class,
        empirical,
        simulated,
    )
}

fn select_abs_quantile_shape_ratio_q99_q95(
    sample_sufficiency_policy: TailSkewSampleSufficiencyPolicy,
) -> TailSkewMetricSampleSufficiencyPolicy {
    sample_sufficiency_policy.returns_abs_quantile_shape_ratio_q99_q95
}

fn select_left_tail_expected_shortfall_to_var_ratio(
    sample_sufficiency_policy: TailSkewSampleSufficiencyPolicy,
) -> TailSkewMetricSampleSufficiencyPolicy {
    sample_sufficiency_policy.returns_left_tail_expected_shortfall_to_var_ratio
}

fn select_bowley_skewness_quantile(
    sample_sufficiency_policy: TailSkewSampleSufficiencyPolicy,
) -> TailSkewMetricSampleSufficiencyPolicy {
    sample_sufficiency_policy.returns_bowley_skewness_quantile
}

fn select_signed_tail_expected_shortfall_imbalance(
    sample_sufficiency_policy: TailSkewSampleSufficiencyPolicy,
) -> TailSkewMetricSampleSufficiencyPolicy {
    sample_sufficiency_policy.returns_signed_tail_expected_shortfall_imbalance
}

fn select_l_moment_lambda2(
    sample_sufficiency_policy: TailSkewSampleSufficiencyPolicy,
) -> TailSkewMetricSampleSufficiencyPolicy {
    sample_sufficiency_policy.returns_l_moment_lambda2
}

fn select_l_moment_tau3(
    sample_sufficiency_policy: TailSkewSampleSufficiencyPolicy,
) -> TailSkewMetricSampleSufficiencyPolicy {
    sample_sufficiency_policy.returns_l_moment_tau3
}

fn select_central_variance(
    sample_sufficiency_policy: TailSkewSampleSufficiencyPolicy,
) -> TailSkewMetricSampleSufficiencyPolicy {
    sample_sufficiency_policy.returns_central_variance
}

fn select_central_skewness(
    sample_sufficiency_policy: TailSkewSampleSufficiencyPolicy,
) -> TailSkewMetricSampleSufficiencyPolicy {
    sample_sufficiency_policy.returns_central_skewness
}

fn select_central_excess_kurtosis(
    sample_sufficiency_policy: TailSkewSampleSufficiencyPolicy,
) -> TailSkewMetricSampleSufficiencyPolicy {
    sample_sufficiency_policy.returns_central_excess_kurtosis
}

fn validate_request(request: &TailSkewDiagnosticsRequest) -> Result<(), TailSkewDiagnosticsError> {
    if request.analysis_start_utc_nanos >= request.analysis_end_utc_nanos {
        return Err(TailSkewDiagnosticsError::InvalidAnalysisWindow {
            analysis_start_utc_nanos: request.analysis_start_utc_nanos,
            analysis_end_utc_nanos: request.analysis_end_utc_nanos,
        });
    }

    validate_dataset_id("empirical", &request.empirical_dataset.dataset_id)?;
    validate_dataset_id("simulated", &request.simulated_dataset.dataset_id)?;

    if request.horizon_buckets.is_empty() {
        return Err(TailSkewDiagnosticsError::EmptyHorizonBucketSet);
    }

    let mut observed_horizon_labels: BTreeSet<&str> = BTreeSet::new();
    for (horizon_position, horizon_bucket) in request.horizon_buckets.iter().enumerate() {
        if horizon_bucket.horizon_label.trim().is_empty() {
            return Err(TailSkewDiagnosticsError::EmptyHorizonLabel { horizon_position });
        }
        if horizon_bucket.horizon_duration_nanos <= 0 {
            return Err(TailSkewDiagnosticsError::InvalidHorizonDuration {
                horizon_label: horizon_bucket.horizon_label.clone(),
                horizon_duration_nanos: horizon_bucket.horizon_duration_nanos,
            });
        }
        if !observed_horizon_labels.insert(horizon_bucket.horizon_label.as_str()) {
            return Err(TailSkewDiagnosticsError::DuplicateHorizonLabel {
                horizon_label: horizon_bucket.horizon_label.clone(),
            });
        }
    }

    validate_bootstrap_configuration(request.bootstrap_configuration)?;
    validate_sample_sufficiency_policy(request.sample_sufficiency_policy)?;

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
) -> Result<(), TailSkewDiagnosticsError> {
    if dataset_id.trim().is_empty() {
        return Err(TailSkewDiagnosticsError::EmptyDatasetId { dataset_role });
    }

    Ok(())
}

fn validate_bootstrap_configuration(
    bootstrap_configuration: TailSkewBlockBootstrapConfiguration,
) -> Result<(), TailSkewDiagnosticsError> {
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
        return Err(TailSkewDiagnosticsError::InvalidBootstrapConfiguration {
            field_name,
            details,
        });
    }

    Ok(())
}

fn block_bootstrap_configuration(
    bootstrap_configuration: TailSkewBlockBootstrapConfiguration,
) -> BlockBootstrapConfiguration {
    BlockBootstrapConfiguration {
        confidence_level: bootstrap_configuration.confidence_level,
        replication_count: bootstrap_configuration.replication_count,
        block_length_interval_count: bootstrap_configuration.block_length_interval_count,
    }
}

fn validate_sample_sufficiency_policy(
    sample_sufficiency_policy: TailSkewSampleSufficiencyPolicy,
) -> Result<(), TailSkewDiagnosticsError> {
    sample_sufficiency_policy
        .returns_abs_quantile_shape_ratio_q99_q95
        .validate("returns_abs_quantile_shape_ratio_q99_q95")?;
    sample_sufficiency_policy
        .returns_left_tail_expected_shortfall_to_var_ratio
        .validate("returns_left_tail_expected_shortfall_to_var_ratio")?;
    sample_sufficiency_policy
        .returns_bowley_skewness_quantile
        .validate("returns_bowley_skewness_quantile")?;
    sample_sufficiency_policy
        .returns_signed_tail_expected_shortfall_imbalance
        .validate("returns_signed_tail_expected_shortfall_imbalance")?;
    sample_sufficiency_policy
        .returns_l_moment_lambda2
        .validate("returns_l_moment_lambda2")?;
    sample_sufficiency_policy
        .returns_l_moment_tau3
        .validate("returns_l_moment_tau3")?;
    sample_sufficiency_policy
        .returns_central_variance
        .validate("returns_central_variance")?;
    sample_sufficiency_policy
        .returns_central_skewness
        .validate("returns_central_skewness")?;
    sample_sufficiency_policy
        .returns_central_excess_kurtosis
        .validate("returns_central_excess_kurtosis")?;

    Ok(())
}

fn validate_return_observations(
    dataset_role: &'static str,
    dataset_id: &str,
    return_observations: &[TailSkewReturnObservation],
) -> Result<(), TailSkewDiagnosticsError> {
    for (observation_position, return_observation) in return_observations.iter().enumerate() {
        if !return_observation.return_increment.is_finite() {
            return Err(TailSkewDiagnosticsError::InvalidReturnObservation {
                dataset_role,
                dataset_id: dataset_id.to_string(),
                observation_position,
                return_increment: return_observation.return_increment,
            });
        }
    }

    Ok(())
}

fn prepare_dataset(
    dataset_role: &'static str,
    dataset: &TailSkewDatasetInput,
    analysis_window: TimestampAnalysisWindow,
) -> Result<PreparedTailSkewDataset, TailSkewDiagnosticsError> {
    validate_dataset_id(dataset_role, &dataset.dataset_id)?;
    validate_return_observations(
        dataset_role,
        &dataset.dataset_id,
        &dataset.return_observations,
    )?;

    let mut return_observations_in_window: Vec<TailSkewReturnObservation> = dataset
        .return_observations
        .iter()
        .copied()
        .filter(|return_observation| {
            analysis_window.contains_timestamp(return_observation.timestamp_utc_nanos)
        })
        .collect();
    return_observations_in_window
        .sort_by_key(|return_observation| return_observation.timestamp_utc_nanos);

    Ok(PreparedTailSkewDataset {
        dataset_id: dataset.dataset_id.clone(),
        return_observations_in_window,
    })
}

fn build_horizon_return_series(
    prepared_dataset: &PreparedTailSkewDataset,
    analysis_window: TimestampAnalysisWindow,
    horizon_bucket: &TailSkewHorizonBucket,
) -> HorizonReturnSeries {
    let interval_count =
        interval_count_for_window(analysis_window, horizon_bucket.horizon_duration_nanos)
            .unwrap_or(0);

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

    let covered_interval_count = return_interval_has_observation
        .iter()
        .filter(|covered| **covered)
        .count();

    let interval_returns: Vec<f64> = interval_return_sum
        .iter()
        .zip(return_interval_has_observation.iter())
        .filter_map(|(interval_return, has_observation)| {
            (*has_observation).then_some(*interval_return)
        })
        .collect();

    HorizonReturnSeries {
        interval_count,
        covered_interval_count,
        interval_returns,
    }
}

fn evaluate_metric<F>(
    interval_returns: &[f64],
    sample_coverage: TailSkewSampleCoverage,
    required_minimums: TailSkewMetricSampleSufficiencyPolicy,
    bootstrap_configuration: TailSkewBlockBootstrapConfiguration,
    bootstrap_seed: u64,
    metric_evaluator: F,
) -> TailSkewDatasetMetricResult
where
    F: Fn(&[f64]) -> TailSkewMetricComputation,
{
    let computation = metric_evaluator(interval_returns);
    let observed_counts = TailSkewObservedCounts {
        sample_count: interval_returns.len(),
        effective_sample_count: computation.effective_sample_count,
        tail_observation_count: computation.tail_observation_count,
    };
    let horizon_coverage_ratio = coverage_ratio(
        sample_coverage.covered_interval_count,
        sample_coverage.horizon_interval_count,
    );
    let status = resolve_metric_status(
        computation.value,
        observed_counts,
        horizon_coverage_ratio,
        required_minimums,
    );

    let confidence_interval = if status == TailSkewMetricStatus::Available {
        computation.value.map(|point_estimate| {
            let (lower_bound, upper_bound) = compute_block_bootstrap_confidence_interval(
                interval_returns,
                point_estimate,
                block_bootstrap_configuration(bootstrap_configuration),
                bootstrap_seed,
                |resampled_returns| metric_evaluator(resampled_returns).value,
            );
            TailSkewConfidenceInterval {
                lower_bound,
                upper_bound,
                confidence_level: bootstrap_configuration.confidence_level,
            }
        })
    } else {
        None
    };

    TailSkewDatasetMetricResult {
        status,
        value: computation.value,
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
    metric_class: TailSkewMetricClass,
    empirical: TailSkewDatasetMetricResult,
    simulated: TailSkewDatasetMetricResult,
) -> TailSkewMetricComparison {
    let absolute_delta = match (
        empirical.status,
        empirical.value,
        simulated.status,
        simulated.value,
    ) {
        (
            TailSkewMetricStatus::Available,
            Some(empirical_value),
            TailSkewMetricStatus::Available,
            Some(simulated_value),
        ) => Some((empirical_value - simulated_value).abs()),
        _ => None,
    };

    let status = if absolute_delta.is_some() {
        TailSkewMetricStatus::Available
    } else {
        TailSkewMetricStatus::InsufficientData
    };

    TailSkewMetricComparison {
        metric_id,
        metric_name,
        metric_class,
        status,
        empirical,
        simulated,
        absolute_delta,
    }
}

fn resolve_metric_status(
    value: Option<f64>,
    observed_counts: TailSkewObservedCounts,
    horizon_coverage_ratio: f64,
    required_minimums: TailSkewMetricSampleSufficiencyPolicy,
) -> TailSkewMetricStatus {
    let has_sufficient_counts = observed_counts.sample_count
        >= required_minimums.minimum_sample_count
        && observed_counts.effective_sample_count
            >= required_minimums.minimum_effective_sample_count
        && observed_counts.tail_observation_count
            >= required_minimums.minimum_tail_observation_count;

    let has_sufficient_coverage =
        horizon_coverage_ratio >= required_minimums.minimum_horizon_coverage_ratio;

    if value.is_some() && has_sufficient_counts && has_sufficient_coverage {
        TailSkewMetricStatus::Available
    } else {
        TailSkewMetricStatus::InsufficientData
    }
}

fn compute_abs_quantile_shape_ratio_q99_q95(interval_returns: &[f64]) -> TailSkewMetricComputation {
    let mut absolute_returns: Vec<f64> = interval_returns
        .iter()
        .copied()
        .filter(|value| value.is_finite())
        .map(f64::abs)
        .collect();

    if absolute_returns.is_empty() {
        return TailSkewMetricComputation {
            value: None,
            effective_sample_count: 0,
            tail_observation_count: 0,
        };
    }

    absolute_returns.sort_by(f64::total_cmp);

    let quantile_95 = compute_sorted_quantile(&absolute_returns, RIGHT_TAIL_QUANTILE_LEVEL);
    let quantile_99 = compute_sorted_quantile(&absolute_returns, EXTREME_RIGHT_TAIL_QUANTILE_LEVEL);

    let value = if !quantile_95.is_finite()
        || quantile_95.abs() <= MINIMUM_DENOMINATOR_MAGNITUDE
        || !quantile_99.is_finite()
    {
        None
    } else {
        let ratio = quantile_99 / quantile_95;
        ratio.is_finite().then_some(ratio)
    };

    TailSkewMetricComputation {
        value,
        effective_sample_count: absolute_returns.len(),
        tail_observation_count: 0,
    }
}

fn compute_left_tail_expected_shortfall_to_var_ratio(
    interval_returns: &[f64],
) -> TailSkewMetricComputation {
    let mut sorted_returns = sorted_finite_returns(interval_returns);

    if sorted_returns.is_empty() {
        return TailSkewMetricComputation {
            value: None,
            effective_sample_count: 0,
            tail_observation_count: 0,
        };
    }

    sorted_returns.sort_by(f64::total_cmp);
    let left_tail_var = compute_sorted_quantile(&sorted_returns, LEFT_TAIL_QUANTILE_LEVEL);

    let left_tail_values: Vec<f64> = sorted_returns
        .iter()
        .copied()
        .filter(|value| *value <= left_tail_var)
        .collect();

    let tail_observation_count = left_tail_values.len();

    let value = if left_tail_var >= 0.0
        || left_tail_var.abs() <= MINIMUM_DENOMINATOR_MAGNITUDE
        || left_tail_values.is_empty()
    {
        None
    } else {
        let expected_shortfall =
            left_tail_values.iter().sum::<f64>() / left_tail_values.len() as f64;
        let ratio = expected_shortfall.abs() / left_tail_var.abs();
        ratio.is_finite().then_some(ratio)
    };

    TailSkewMetricComputation {
        value,
        effective_sample_count: sorted_returns.len(),
        tail_observation_count,
    }
}

fn compute_bowley_skewness_quantile(interval_returns: &[f64]) -> TailSkewMetricComputation {
    let mut sorted_returns = sorted_finite_returns(interval_returns);

    if sorted_returns.is_empty() {
        return TailSkewMetricComputation {
            value: None,
            effective_sample_count: 0,
            tail_observation_count: 0,
        };
    }

    sorted_returns.sort_by(f64::total_cmp);

    let lower_quartile = compute_sorted_quantile(&sorted_returns, LOWER_QUARTILE_LEVEL);
    let median = compute_sorted_quantile(&sorted_returns, MEDIAN_QUANTILE_LEVEL);
    let upper_quartile = compute_sorted_quantile(&sorted_returns, UPPER_QUARTILE_LEVEL);
    let denominator = upper_quartile - lower_quartile;

    let value = if !denominator.is_finite() || denominator.abs() <= MINIMUM_DENOMINATOR_MAGNITUDE {
        None
    } else {
        let skewness = (upper_quartile + lower_quartile - 2.0 * median) / denominator;
        skewness.is_finite().then_some(skewness)
    };

    TailSkewMetricComputation {
        value,
        effective_sample_count: sorted_returns.len(),
        tail_observation_count: 0,
    }
}

fn compute_signed_tail_expected_shortfall_imbalance(
    interval_returns: &[f64],
) -> TailSkewMetricComputation {
    let mut sorted_returns = sorted_finite_returns(interval_returns);

    if sorted_returns.is_empty() {
        return TailSkewMetricComputation {
            value: None,
            effective_sample_count: 0,
            tail_observation_count: 0,
        };
    }

    sorted_returns.sort_by(f64::total_cmp);

    let left_tail_var = compute_sorted_quantile(&sorted_returns, LEFT_TAIL_QUANTILE_LEVEL);
    let right_tail_var = compute_sorted_quantile(&sorted_returns, RIGHT_TAIL_QUANTILE_LEVEL);

    let left_tail_values: Vec<f64> = sorted_returns
        .iter()
        .copied()
        .filter(|value| *value <= left_tail_var)
        .collect();
    let right_tail_values: Vec<f64> = sorted_returns
        .iter()
        .copied()
        .filter(|value| *value >= right_tail_var)
        .collect();

    let tail_observation_count = left_tail_values
        .len()
        .saturating_add(right_tail_values.len());

    let value = if left_tail_values.is_empty() || right_tail_values.is_empty() {
        None
    } else {
        let left_expected_shortfall_magnitude =
            left_tail_values.iter().sum::<f64>().abs() / left_tail_values.len() as f64;
        let right_expected_shortfall_magnitude =
            right_tail_values.iter().sum::<f64>().abs() / right_tail_values.len() as f64;

        let normalizer = 0.5 * (left_tail_var.abs() + right_tail_var.abs());
        if !normalizer.is_finite() || normalizer <= MINIMUM_DENOMINATOR_MAGNITUDE {
            None
        } else {
            let imbalance = (right_expected_shortfall_magnitude
                - left_expected_shortfall_magnitude)
                / normalizer;
            imbalance.is_finite().then_some(imbalance)
        }
    };

    TailSkewMetricComputation {
        value,
        effective_sample_count: sorted_returns.len(),
        tail_observation_count,
    }
}

fn compute_l_moment_lambda2(interval_returns: &[f64]) -> TailSkewMetricComputation {
    let mut sorted_returns = sorted_finite_returns(interval_returns);

    if sorted_returns.len() < 2 {
        return TailSkewMetricComputation {
            value: None,
            effective_sample_count: sorted_returns.len(),
            tail_observation_count: 0,
        };
    }

    sorted_returns.sort_by(f64::total_cmp);

    let value = compute_l_moment_lambda2_from_sorted(&sorted_returns);

    TailSkewMetricComputation {
        value,
        effective_sample_count: sorted_returns.len(),
        tail_observation_count: 0,
    }
}

fn compute_l_moment_tau3(interval_returns: &[f64]) -> TailSkewMetricComputation {
    let mut sorted_returns = sorted_finite_returns(interval_returns);

    if sorted_returns.len() < 3 {
        return TailSkewMetricComputation {
            value: None,
            effective_sample_count: sorted_returns.len(),
            tail_observation_count: 0,
        };
    }

    sorted_returns.sort_by(f64::total_cmp);

    let value = compute_l_moment_tau3_from_sorted(&sorted_returns);

    TailSkewMetricComputation {
        value,
        effective_sample_count: sorted_returns.len(),
        tail_observation_count: 0,
    }
}

fn compute_central_variance(interval_returns: &[f64]) -> TailSkewMetricComputation {
    let value = compute_sample_variance(interval_returns);

    TailSkewMetricComputation {
        value,
        effective_sample_count: interval_returns.len().saturating_sub(1),
        tail_observation_count: 0,
    }
}

fn compute_central_skewness(interval_returns: &[f64]) -> TailSkewMetricComputation {
    let value = compute_adjusted_central_skewness(interval_returns);

    TailSkewMetricComputation {
        value,
        effective_sample_count: interval_returns.len().saturating_sub(2),
        tail_observation_count: 0,
    }
}

fn compute_central_excess_kurtosis(interval_returns: &[f64]) -> TailSkewMetricComputation {
    let value = compute_adjusted_central_excess_kurtosis(interval_returns);

    TailSkewMetricComputation {
        value,
        effective_sample_count: interval_returns.len().saturating_sub(3),
        tail_observation_count: 0,
    }
}

fn compute_l_moment_lambda2_from_sorted(sorted_returns: &[f64]) -> Option<f64> {
    let probability_weighted_moment_0 = compute_probability_weighted_moment(sorted_returns, 0)?;
    let probability_weighted_moment_1 = compute_probability_weighted_moment(sorted_returns, 1)?;

    let lambda2 = 2.0 * probability_weighted_moment_1 - probability_weighted_moment_0;

    lambda2.is_finite().then_some(lambda2)
}

fn compute_l_moment_tau3_from_sorted(sorted_returns: &[f64]) -> Option<f64> {
    let probability_weighted_moment_0 = compute_probability_weighted_moment(sorted_returns, 0)?;
    let probability_weighted_moment_1 = compute_probability_weighted_moment(sorted_returns, 1)?;
    let probability_weighted_moment_2 = compute_probability_weighted_moment(sorted_returns, 2)?;

    let lambda2 = 2.0 * probability_weighted_moment_1 - probability_weighted_moment_0;
    if !lambda2.is_finite() || lambda2.abs() <= MINIMUM_DENOMINATOR_MAGNITUDE {
        return None;
    }

    let lambda3 = 6.0 * probability_weighted_moment_2 - 6.0 * probability_weighted_moment_1
        + probability_weighted_moment_0;
    if !lambda3.is_finite() {
        return None;
    }

    let tau3 = lambda3 / lambda2;
    tau3.is_finite().then_some(tau3)
}

fn compute_sample_variance(interval_returns: &[f64]) -> Option<f64> {
    let finite_returns = sorted_finite_returns(interval_returns);
    if finite_returns.len() < 2 {
        return None;
    }

    let sample_mean = finite_returns.iter().sum::<f64>() / finite_returns.len() as f64;

    let sum_squared_deviation = finite_returns
        .iter()
        .map(|return_value| {
            let centered_value = *return_value - sample_mean;
            centered_value * centered_value
        })
        .sum::<f64>();

    let variance = sum_squared_deviation / (finite_returns.len() - 1) as f64;
    variance.is_finite().then_some(variance)
}

fn compute_adjusted_central_skewness(interval_returns: &[f64]) -> Option<f64> {
    let finite_returns = sorted_finite_returns(interval_returns);
    let sample_count = finite_returns.len();
    if sample_count < 3 {
        return None;
    }

    let sample_mean = finite_returns.iter().sum::<f64>() / sample_count as f64;

    let (sum_squared_deviation, sum_cubed_deviation) =
        finite_returns
            .iter()
            .fold((0.0, 0.0), |(running_sum2, running_sum3), return_value| {
                let centered_value = *return_value - sample_mean;
                (
                    running_sum2 + centered_value * centered_value,
                    running_sum3 + centered_value * centered_value * centered_value,
                )
            });

    let variance_population = sum_squared_deviation / sample_count as f64;
    if !variance_population.is_finite() || variance_population <= MINIMUM_DENOMINATOR_MAGNITUDE {
        return Some(0.0);
    }

    let third_central_moment = sum_cubed_deviation / sample_count as f64;
    if !third_central_moment.is_finite() {
        return None;
    }

    let unadjusted_skewness = third_central_moment / variance_population.powf(1.5);
    if !unadjusted_skewness.is_finite() {
        return None;
    }

    let adjustment =
        ((sample_count * (sample_count - 1)) as f64).sqrt() / (sample_count - 2) as f64;
    let adjusted_skewness = adjustment * unadjusted_skewness;

    adjusted_skewness.is_finite().then_some(adjusted_skewness)
}

fn compute_adjusted_central_excess_kurtosis(interval_returns: &[f64]) -> Option<f64> {
    let finite_returns = sorted_finite_returns(interval_returns);
    let sample_count = finite_returns.len();
    if sample_count < 4 {
        return None;
    }

    let sample_mean = finite_returns.iter().sum::<f64>() / sample_count as f64;

    let (sum_squared_deviation, sum_fourth_deviation) =
        finite_returns
            .iter()
            .fold((0.0, 0.0), |(running_sum2, running_sum4), return_value| {
                let centered_value = *return_value - sample_mean;
                let centered_square = centered_value * centered_value;
                (
                    running_sum2 + centered_square,
                    running_sum4 + centered_square * centered_square,
                )
            });

    let variance_population = sum_squared_deviation / sample_count as f64;
    if !variance_population.is_finite() || variance_population <= MINIMUM_DENOMINATOR_MAGNITUDE {
        return Some(0.0);
    }

    let fourth_central_moment = sum_fourth_deviation / sample_count as f64;
    if !fourth_central_moment.is_finite() {
        return None;
    }

    let unadjusted_excess_kurtosis =
        fourth_central_moment / (variance_population * variance_population) - 3.0;
    if !unadjusted_excess_kurtosis.is_finite() {
        return None;
    }

    let n = sample_count as f64;
    let adjusted_excess_kurtosis =
        ((n - 1.0) / ((n - 2.0) * (n - 3.0))) * ((n + 1.0) * unadjusted_excess_kurtosis + 6.0);

    adjusted_excess_kurtosis
        .is_finite()
        .then_some(adjusted_excess_kurtosis)
}

fn sorted_finite_returns(interval_returns: &[f64]) -> Vec<f64> {
    interval_returns
        .iter()
        .copied()
        .filter(|value| value.is_finite())
        .collect()
}
