//! Horizon-aware stylized-fact comparison engine and delta reporting.

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt::{Display, Formatter};

use janiform_foundations::strings::is_non_empty_string;
use serde::{Deserialize, Serialize};

use super::diagnostics_shared::build_delta_uncertainty_context;
use super::metric_catalog::STYLIZED_FACT_METRIC_CATALOG_INSUFFICIENT_DATA_STATUS_CODE;

pub const STYLIZED_FACT_HORIZON_COMPARISON_REPORT_SCHEMA_VERSION: &str =
    "validation.stylized_fact.interval_comparison.report.v1";
pub const STYLIZED_FACT_HORIZON_COMPARISON_DECISION_POLICY: &str =
    "explicit_interval_and_metric_key_alignment_without_implicit_interpolation.v1";
pub const STYLIZED_FACT_HORIZON_COMPARISON_UNCERTAINTY_POLICY: &str =
    "pooled_interval_half_width_standardized_delta_from_input_confidence_intervals.v1";
pub const STYLIZED_FACT_HORIZON_COMPARISON_OUTPUT_SCHEMA_METADATA_VERSION: &str =
    "validation.stylized_fact.interval_comparison.output_schema_metadata.v1";

const REQUIRED_BATCH_REPORT_FIELDS: [&str; 11] = [
    "schema_version",
    "decision_policy",
    "uncertainty_policy",
    "insufficient_data_status_code",
    "output_schema_metadata",
    "metric_catalog_id",
    "metric_catalog_version",
    "threshold_policy_id",
    "status",
    "summary",
    "run_reports",
];
const REQUIRED_RUN_REPORT_FIELDS: [&str; 8] = [
    "calibration_run_id",
    "empirical_dataset_id",
    "simulated_dataset_id",
    "status",
    "decision_counts",
    "total_interval_count",
    "total_metric_count",
    "intervals",
];
const REQUIRED_HORIZON_REPORT_FIELDS: [&str; 7] = [
    "interval_label",
    "empirical_interval_duration_nanos",
    "simulated_interval_duration_nanos",
    "alignment_status",
    "status",
    "decision_counts",
    "metrics",
];
const REQUIRED_METRIC_REPORT_FIELDS: [&str; 12] = [
    "metric_id",
    "metric_name",
    "metric_variant",
    "alignment_status",
    "status",
    "empirical",
    "simulated",
    "absolute_delta",
    "uncertainty_context",
    "threshold_context",
    "interval_label",
    "decision_reason_codes",
];
const REQUIRED_DATASET_METRIC_FIELDS: [&str; 9] = [
    "status",
    "value",
    "confidence_interval",
    "sample_coverage",
    "observed_counts",
    "required_minimums",
    "horizon_coverage_ratio",
    "insufficiency_reason_codes",
    "input_present",
];
const REQUIRED_DECISION_COUNT_FIELDS: [&str; 3] =
    ["pass_count", "fail_count", "insufficient_data_count"];
const DECISION_STATE_VOCABULARY: [&str; 3] = ["pass", "fail", "insufficient_data"];
const HORIZON_ALIGNMENT_STATUS_VOCABULARY: [&str; 4] = [
    "matched",
    "missing_empirical_horizon",
    "missing_simulated_horizon",
    "horizon_duration_mismatch",
];
const METRIC_ALIGNMENT_STATUS_VOCABULARY: [&str; 4] = [
    "matched",
    "missing_empirical_metric",
    "missing_simulated_metric",
    "horizon_duration_mismatch",
];
const DATASET_STATUS_VOCABULARY: [&str; 2] = ["available", "insufficient_data"];
const SUFFICIENCY_REASON_VOCABULARY: [&str; 7] = [
    "missing_metric",
    "missing_point_estimate",
    "invalid_point_estimate",
    "insufficient_sample_count",
    "insufficient_effective_sample_count",
    "insufficient_tail_observation_count",
    "insufficient_horizon_coverage_ratio",
];
const DECISION_REASON_VOCABULARY: [&str; 7] = [
    "missing_empirical_metric",
    "missing_simulated_metric",
    "horizon_duration_mismatch",
    "insufficient_empirical_data",
    "insufficient_simulated_data",
    "absolute_delta_threshold_exceeded",
    "standardized_delta_threshold_exceeded",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StylizedFactComparisonDecisionState {
    Pass,
    Fail,
    InsufficientData,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StylizedFactDatasetMetricStatus {
    Available,
    InsufficientData,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StylizedFactHorizonAlignmentStatus {
    Matched,
    MissingEmpiricalHorizon,
    MissingSimulatedHorizon,
    HorizonDurationMismatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StylizedFactMetricAlignmentStatus {
    Matched,
    MissingEmpiricalMetric,
    MissingSimulatedMetric,
    HorizonDurationMismatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StylizedFactSampleSufficiencyReasonCode {
    MissingMetric,
    MissingPointEstimate,
    InvalidPointEstimate,
    InsufficientSampleCount,
    InsufficientEffectiveSampleCount,
    InsufficientTailObservationCount,
    InsufficientHorizonCoverageRatio,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StylizedFactDecisionReasonCode {
    MissingEmpiricalMetric,
    MissingSimulatedMetric,
    HorizonDurationMismatch,
    InsufficientEmpiricalData,
    InsufficientSimulatedData,
    AbsoluteDeltaThresholdExceeded,
    StandardizedDeltaThresholdExceeded,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StylizedFactHorizonComparisonRequest {
    pub metric_catalog_id: String,
    pub metric_catalog_version: String,
    pub threshold_policy: StylizedFactHorizonComparisonThresholdPolicy,
    pub calibration_runs: Vec<StylizedFactCalibrationRunInput>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StylizedFactHorizonComparisonThresholdPolicy {
    pub policy_id: String,
    pub default_max_absolute_delta: Option<f64>,
    pub default_max_standardized_delta: Option<f64>,
    pub metric_threshold_overrides: Vec<StylizedFactMetricThresholdOverride>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StylizedFactMetricThresholdOverride {
    pub metric_id: String,
    pub metric_variant: Option<String>,
    pub max_absolute_delta: Option<f64>,
    pub max_standardized_delta: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StylizedFactCalibrationRunInput {
    pub calibration_run_id: String,
    pub empirical_dataset_id: String,
    pub simulated_dataset_id: String,
    pub empirical_metrics: Vec<StylizedFactObservedMetricInput>,
    pub simulated_metrics: Vec<StylizedFactObservedMetricInput>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StylizedFactObservedMetricInput {
    pub horizon_label: String,
    pub horizon_duration_nanos: i64,
    pub metric_id: String,
    pub metric_name: String,
    pub metric_variant: Option<String>,
    pub value: Option<f64>,
    pub confidence_interval: Option<StylizedFactMetricConfidenceInterval>,
    pub sample_coverage: StylizedFactMetricSampleCoverage,
    pub observed_counts: StylizedFactObservedCounts,
    pub required_minimums: StylizedFactSampleSufficiencyRequirements,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StylizedFactMetricConfidenceInterval {
    pub lower_bound: f64,
    pub upper_bound: f64,
    pub confidence_level: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StylizedFactMetricSampleCoverage {
    pub horizon_interval_count: usize,
    pub covered_interval_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StylizedFactObservedCounts {
    pub sample_count: usize,
    pub effective_sample_count: usize,
    pub tail_observation_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StylizedFactSampleSufficiencyRequirements {
    pub minimum_sample_count: usize,
    pub minimum_effective_sample_count: usize,
    pub minimum_tail_observation_count: usize,
    pub minimum_horizon_coverage_ratio: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StylizedFactHorizonComparisonBatchReport {
    pub schema_version: &'static str,
    pub decision_policy: &'static str,
    pub uncertainty_policy: &'static str,
    pub insufficient_data_status_code: &'static str,
    pub output_schema_metadata: StylizedFactHorizonComparisonOutputSchemaMetadata,
    pub metric_catalog_id: String,
    pub metric_catalog_version: String,
    pub threshold_policy_id: String,
    pub status: StylizedFactComparisonDecisionState,
    pub summary: StylizedFactHorizonComparisonBatchSummary,
    pub run_reports: Vec<StylizedFactCalibrationRunComparisonReport>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StylizedFactHorizonComparisonOutputSchemaMetadata {
    pub schema_version: &'static str,
    pub required_batch_report_fields: Vec<&'static str>,
    pub required_run_report_fields: Vec<&'static str>,
    pub required_horizon_report_fields: Vec<&'static str>,
    pub required_metric_report_fields: Vec<&'static str>,
    pub required_dataset_metric_fields: Vec<&'static str>,
    pub required_decision_count_fields: Vec<&'static str>,
    pub decision_state_vocabulary: Vec<&'static str>,
    pub horizon_alignment_status_vocabulary: Vec<&'static str>,
    pub metric_alignment_status_vocabulary: Vec<&'static str>,
    pub dataset_metric_status_vocabulary: Vec<&'static str>,
    pub sample_sufficiency_reason_vocabulary: Vec<&'static str>,
    pub decision_reason_vocabulary: Vec<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct StylizedFactDecisionCounts {
    pub pass_count: usize,
    pub fail_count: usize,
    pub insufficient_data_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct StylizedFactHorizonComparisonBatchSummary {
    pub total_run_count: usize,
    pub total_metric_count: usize,
    pub run_decision_counts: StylizedFactDecisionCounts,
    pub metric_decision_counts: StylizedFactDecisionCounts,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StylizedFactCalibrationRunComparisonReport {
    pub calibration_run_id: String,
    pub empirical_dataset_id: String,
    pub simulated_dataset_id: String,
    pub status: StylizedFactComparisonDecisionState,
    pub decision_counts: StylizedFactDecisionCounts,
    #[serde(rename = "total_interval_count")]
    pub total_horizon_count: usize,
    pub total_metric_count: usize,
    #[serde(rename = "intervals")]
    pub horizons: Vec<StylizedFactHorizonComparison>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StylizedFactHorizonComparison {
    #[serde(rename = "interval_label")]
    pub horizon_label: String,
    #[serde(rename = "empirical_interval_duration_nanos")]
    pub empirical_horizon_duration_nanos: Option<i64>,
    #[serde(rename = "simulated_interval_duration_nanos")]
    pub simulated_horizon_duration_nanos: Option<i64>,
    pub alignment_status: StylizedFactHorizonAlignmentStatus,
    pub status: StylizedFactComparisonDecisionState,
    pub decision_counts: StylizedFactDecisionCounts,
    pub metrics: Vec<StylizedFactMetricComparison>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StylizedFactMetricComparison {
    #[serde(rename = "interval_label")]
    pub horizon_label: String,
    pub metric_id: String,
    pub metric_name: String,
    pub metric_variant: Option<String>,
    pub alignment_status: StylizedFactMetricAlignmentStatus,
    pub status: StylizedFactComparisonDecisionState,
    pub empirical: StylizedFactDatasetMetricEvaluation,
    pub simulated: StylizedFactDatasetMetricEvaluation,
    pub absolute_delta: Option<f64>,
    pub uncertainty_context: StylizedFactDeltaUncertaintyContext,
    pub threshold_context: StylizedFactThresholdContext,
    pub decision_reason_codes: Vec<StylizedFactDecisionReasonCode>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StylizedFactDatasetMetricEvaluation {
    pub status: StylizedFactDatasetMetricStatus,
    pub value: Option<f64>,
    pub confidence_interval: Option<StylizedFactMetricConfidenceInterval>,
    pub sample_coverage: StylizedFactMetricSampleCoverage,
    pub observed_counts: StylizedFactObservedCounts,
    pub required_minimums: StylizedFactSampleSufficiencyRequirements,
    #[serde(rename = "interval_coverage_ratio")]
    pub horizon_coverage_ratio: f64,
    pub insufficiency_reason_codes: Vec<StylizedFactSampleSufficiencyReasonCode>,
    pub input_present: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct StylizedFactDeltaUncertaintyContext {
    pub empirical_interval_half_width: Option<f64>,
    pub simulated_interval_half_width: Option<f64>,
    pub pooled_interval_half_width: Option<f64>,
    pub standardized_delta: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StylizedFactThresholdContext {
    pub policy_id: String,
    pub max_absolute_delta: Option<f64>,
    pub max_standardized_delta: Option<f64>,
    pub absolute_delta_exceeds_threshold: Option<bool>,
    pub standardized_delta_exceeds_threshold: Option<bool>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StylizedFactHorizonComparisonError {
    InvalidRequest { field_path: String, details: String },
}

impl Display for StylizedFactHorizonComparisonError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidRequest {
                field_path,
                details,
            } => write!(formatter, "invalid request field `{field_path}`: {details}"),
        }
    }
}

impl Error for StylizedFactHorizonComparisonError {}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct MetricKey {
    horizon_label: String,
    metric_id: String,
    metric_variant: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct MetricDimensionKey {
    metric_id: String,
    metric_variant: Option<String>,
}

#[derive(Debug, Clone)]
struct IndexedDatasetMetrics {
    metrics_by_key: BTreeMap<MetricKey, StylizedFactObservedMetricInput>,
    horizon_duration_by_label: BTreeMap<String, i64>,
}

#[derive(Debug, Clone, Copy)]
struct ResolvedThreshold {
    max_absolute_delta: Option<f64>,
    max_standardized_delta: Option<f64>,
}

pub fn run_stylized_fact_horizon_comparison_engine(
    request: StylizedFactHorizonComparisonRequest,
) -> Result<StylizedFactHorizonComparisonBatchReport, StylizedFactHorizonComparisonError> {
    validate_request(&request)?;

    let mut run_reports: Vec<StylizedFactCalibrationRunComparisonReport> =
        Vec::with_capacity(request.calibration_runs.len());
    for (run_position, run_input) in request.calibration_runs.iter().enumerate() {
        run_reports.push(build_run_report(
            run_input,
            run_position,
            &request.threshold_policy,
        )?);
    }

    run_reports.sort_by(|left, right| left.calibration_run_id.cmp(&right.calibration_run_id));

    let run_decision_counts = aggregate_run_decision_counts(run_reports.as_slice());
    let metric_decision_counts = aggregate_metric_decision_counts(run_reports.as_slice());
    let total_metric_count = metric_decision_counts.pass_count
        + metric_decision_counts.fail_count
        + metric_decision_counts.insufficient_data_count;

    let summary = StylizedFactHorizonComparisonBatchSummary {
        total_run_count: run_reports.len(),
        total_metric_count,
        run_decision_counts,
        metric_decision_counts,
    };

    let status = aggregate_decision_status(run_decision_counts);

    Ok(StylizedFactHorizonComparisonBatchReport {
        schema_version: STYLIZED_FACT_HORIZON_COMPARISON_REPORT_SCHEMA_VERSION,
        decision_policy: STYLIZED_FACT_HORIZON_COMPARISON_DECISION_POLICY,
        uncertainty_policy: STYLIZED_FACT_HORIZON_COMPARISON_UNCERTAINTY_POLICY,
        insufficient_data_status_code: STYLIZED_FACT_METRIC_CATALOG_INSUFFICIENT_DATA_STATUS_CODE,
        output_schema_metadata: build_output_schema_metadata(),
        metric_catalog_id: request.metric_catalog_id,
        metric_catalog_version: request.metric_catalog_version,
        threshold_policy_id: request.threshold_policy.policy_id,
        status,
        summary,
        run_reports,
    })
}

fn build_output_schema_metadata() -> StylizedFactHorizonComparisonOutputSchemaMetadata {
    StylizedFactHorizonComparisonOutputSchemaMetadata {
        schema_version: STYLIZED_FACT_HORIZON_COMPARISON_OUTPUT_SCHEMA_METADATA_VERSION,
        required_batch_report_fields: REQUIRED_BATCH_REPORT_FIELDS.to_vec(),
        required_run_report_fields: REQUIRED_RUN_REPORT_FIELDS.to_vec(),
        required_horizon_report_fields: REQUIRED_HORIZON_REPORT_FIELDS.to_vec(),
        required_metric_report_fields: REQUIRED_METRIC_REPORT_FIELDS.to_vec(),
        required_dataset_metric_fields: REQUIRED_DATASET_METRIC_FIELDS.to_vec(),
        required_decision_count_fields: REQUIRED_DECISION_COUNT_FIELDS.to_vec(),
        decision_state_vocabulary: DECISION_STATE_VOCABULARY.to_vec(),
        horizon_alignment_status_vocabulary: HORIZON_ALIGNMENT_STATUS_VOCABULARY.to_vec(),
        metric_alignment_status_vocabulary: METRIC_ALIGNMENT_STATUS_VOCABULARY.to_vec(),
        dataset_metric_status_vocabulary: DATASET_STATUS_VOCABULARY.to_vec(),
        sample_sufficiency_reason_vocabulary: SUFFICIENCY_REASON_VOCABULARY.to_vec(),
        decision_reason_vocabulary: DECISION_REASON_VOCABULARY.to_vec(),
    }
}

fn build_run_report(
    run_input: &StylizedFactCalibrationRunInput,
    run_position: usize,
    threshold_policy: &StylizedFactHorizonComparisonThresholdPolicy,
) -> Result<StylizedFactCalibrationRunComparisonReport, StylizedFactHorizonComparisonError> {
    let empirical_index = index_dataset_metrics(
        run_input.empirical_metrics.as_slice(),
        &format!("calibration_runs[{run_position}].empirical_metrics"),
    )?;

    let simulated_index = index_dataset_metrics(
        run_input.simulated_metrics.as_slice(),
        &format!("calibration_runs[{run_position}].simulated_metrics"),
    )?;

    let horizon_labels: BTreeSet<String> = empirical_index
        .horizon_duration_by_label
        .keys()
        .chain(simulated_index.horizon_duration_by_label.keys())
        .cloned()
        .collect();

    let mut horizon_reports: Vec<StylizedFactHorizonComparison> =
        Vec::with_capacity(horizon_labels.len());
    let mut total_metric_count = 0usize;

    for horizon_label in horizon_labels {
        let empirical_horizon_duration_nanos = empirical_index
            .horizon_duration_by_label
            .get(&horizon_label)
            .copied();

        let simulated_horizon_duration_nanos = simulated_index
            .horizon_duration_by_label
            .get(&horizon_label)
            .copied();

        let alignment_status = match (
            empirical_horizon_duration_nanos,
            simulated_horizon_duration_nanos,
        ) {
            (Some(empirical_duration), Some(simulated_duration))
                if empirical_duration == simulated_duration =>
            {
                StylizedFactHorizonAlignmentStatus::Matched
            }
            (Some(_), Some(_)) => StylizedFactHorizonAlignmentStatus::HorizonDurationMismatch,
            (None, Some(_)) => StylizedFactHorizonAlignmentStatus::MissingEmpiricalHorizon,
            (Some(_), None) => StylizedFactHorizonAlignmentStatus::MissingSimulatedHorizon,
            (None, None) => StylizedFactHorizonAlignmentStatus::Matched,
        };

        let metric_dimensions = collect_metric_dimensions_for_horizon(
            &horizon_label,
            &empirical_index,
            &simulated_index,
        );

        let mut metric_reports: Vec<StylizedFactMetricComparison> =
            Vec::with_capacity(metric_dimensions.len());

        for metric_dimension in metric_dimensions {
            let metric_key = MetricKey {
                horizon_label: horizon_label.clone(),
                metric_id: metric_dimension.metric_id.clone(),
                metric_variant: metric_dimension.metric_variant.clone(),
            };

            let empirical_metric = empirical_index.metrics_by_key.get(&metric_key);
            let simulated_metric = simulated_index.metrics_by_key.get(&metric_key);

            let metric_name = resolve_metric_name(
                empirical_metric,
                simulated_metric,
                run_position,
                &horizon_label,
                &metric_dimension.metric_id,
                metric_dimension.metric_variant.as_deref(),
            )?;

            metric_reports.push(build_metric_report(
                &horizon_label,
                &metric_dimension,
                metric_name,
                empirical_metric,
                simulated_metric,
                threshold_policy,
            ));
        }

        let decision_counts = count_metric_decisions(metric_reports.as_slice());
        let status = aggregate_decision_status(decision_counts);

        total_metric_count += metric_reports.len();

        horizon_reports.push(StylizedFactHorizonComparison {
            horizon_label,
            empirical_horizon_duration_nanos,
            simulated_horizon_duration_nanos,
            alignment_status,
            status,
            decision_counts,
            metrics: metric_reports,
        });
    }

    let decision_counts = aggregate_horizon_metric_decision_counts(horizon_reports.as_slice());
    let status = aggregate_decision_status(decision_counts);

    Ok(StylizedFactCalibrationRunComparisonReport {
        calibration_run_id: run_input.calibration_run_id.clone(),
        empirical_dataset_id: run_input.empirical_dataset_id.clone(),
        simulated_dataset_id: run_input.simulated_dataset_id.clone(),
        status,
        decision_counts,
        total_horizon_count: horizon_reports.len(),
        total_metric_count,
        horizons: horizon_reports,
    })
}

fn build_metric_report(
    horizon_label: &str,
    metric_dimension: &MetricDimensionKey,
    metric_name: String,
    empirical_metric: Option<&StylizedFactObservedMetricInput>,
    simulated_metric: Option<&StylizedFactObservedMetricInput>,
    threshold_policy: &StylizedFactHorizonComparisonThresholdPolicy,
) -> StylizedFactMetricComparison {
    let alignment_status = match (empirical_metric, simulated_metric) {
        (Some(empirical), Some(simulated))
            if empirical.horizon_duration_nanos == simulated.horizon_duration_nanos =>
        {
            StylizedFactMetricAlignmentStatus::Matched
        }
        (Some(_), Some(_)) => StylizedFactMetricAlignmentStatus::HorizonDurationMismatch,
        (None, Some(_)) => StylizedFactMetricAlignmentStatus::MissingEmpiricalMetric,
        (Some(_), None) => StylizedFactMetricAlignmentStatus::MissingSimulatedMetric,
        (None, None) => StylizedFactMetricAlignmentStatus::Matched,
    };

    let empirical_evaluation = match empirical_metric {
        Some(metric) => evaluate_present_metric(metric),
        None => evaluate_missing_metric(simulated_metric),
    };
    let simulated_evaluation = match simulated_metric {
        Some(metric) => evaluate_present_metric(metric),
        None => evaluate_missing_metric(empirical_metric),
    };

    let threshold = resolve_threshold_for_metric(
        threshold_policy,
        metric_dimension.metric_id.as_str(),
        metric_dimension.metric_variant.as_deref(),
    );

    let mut decision_reason_codes: Vec<StylizedFactDecisionReasonCode> = Vec::new();

    let status = if alignment_status != StylizedFactMetricAlignmentStatus::Matched {
        decision_reason_codes.push(match alignment_status {
            StylizedFactMetricAlignmentStatus::MissingEmpiricalMetric => {
                StylizedFactDecisionReasonCode::MissingEmpiricalMetric
            }
            StylizedFactMetricAlignmentStatus::MissingSimulatedMetric => {
                StylizedFactDecisionReasonCode::MissingSimulatedMetric
            }
            StylizedFactMetricAlignmentStatus::HorizonDurationMismatch => {
                StylizedFactDecisionReasonCode::HorizonDurationMismatch
            }
            StylizedFactMetricAlignmentStatus::Matched => {
                StylizedFactDecisionReasonCode::InsufficientEmpiricalData
            }
        });
        StylizedFactComparisonDecisionState::InsufficientData
    } else if empirical_evaluation.status == StylizedFactDatasetMetricStatus::InsufficientData
        || simulated_evaluation.status == StylizedFactDatasetMetricStatus::InsufficientData
    {
        if empirical_evaluation.status == StylizedFactDatasetMetricStatus::InsufficientData {
            decision_reason_codes.push(StylizedFactDecisionReasonCode::InsufficientEmpiricalData);
        }
        if simulated_evaluation.status == StylizedFactDatasetMetricStatus::InsufficientData {
            decision_reason_codes.push(StylizedFactDecisionReasonCode::InsufficientSimulatedData);
        }
        StylizedFactComparisonDecisionState::InsufficientData
    } else {
        StylizedFactComparisonDecisionState::Pass
    };

    let absolute_delta = if status == StylizedFactComparisonDecisionState::Pass {
        empirical_evaluation
            .value
            .zip(simulated_evaluation.value)
            .map(|(empirical_value, simulated_value)| (empirical_value - simulated_value).abs())
    } else {
        None
    };

    let uncertainty_context = build_delta_uncertainty_context(
        empirical_evaluation
            .confidence_interval
            .and_then(confidence_interval_to_bounds),
        simulated_evaluation
            .confidence_interval
            .and_then(confidence_interval_to_bounds),
        absolute_delta,
        |components| StylizedFactDeltaUncertaintyContext {
            empirical_interval_half_width: components.empirical_interval_half_width,
            simulated_interval_half_width: components.simulated_interval_half_width,
            pooled_interval_half_width: components.pooled_interval_half_width,
            standardized_delta: components.standardized_delta,
        },
    );

    let mut absolute_delta_exceeds_threshold = None;
    let mut standardized_delta_exceeds_threshold = None;

    let status = if status == StylizedFactComparisonDecisionState::Pass {
        absolute_delta_exceeds_threshold = match (threshold.max_absolute_delta, absolute_delta) {
            (Some(max_absolute_delta), Some(delta)) => Some(delta > max_absolute_delta),
            (Some(_), None) | (None, _) => None,
        };

        standardized_delta_exceeds_threshold = match (
            threshold.max_standardized_delta,
            uncertainty_context.standardized_delta,
        ) {
            (Some(max_standardized_delta), Some(standardized_delta)) => {
                Some(standardized_delta > max_standardized_delta)
            }
            (Some(_), None) => None,
            (None, _) => None,
        };

        let absolute_failed = absolute_delta_exceeds_threshold == Some(true);
        let standardized_failed = standardized_delta_exceeds_threshold == Some(true);

        if absolute_failed {
            decision_reason_codes
                .push(StylizedFactDecisionReasonCode::AbsoluteDeltaThresholdExceeded);
        }

        if standardized_failed {
            decision_reason_codes
                .push(StylizedFactDecisionReasonCode::StandardizedDeltaThresholdExceeded);
        }

        if absolute_failed || standardized_failed {
            StylizedFactComparisonDecisionState::Fail
        } else {
            StylizedFactComparisonDecisionState::Pass
        }
    } else {
        status
    };

    StylizedFactMetricComparison {
        horizon_label: horizon_label.to_string(),
        metric_id: metric_dimension.metric_id.clone(),
        metric_name,
        metric_variant: metric_dimension.metric_variant.clone(),
        alignment_status,
        status,
        empirical: empirical_evaluation,
        simulated: simulated_evaluation,
        absolute_delta,
        uncertainty_context,
        threshold_context: StylizedFactThresholdContext {
            policy_id: threshold_policy.policy_id.clone(),
            max_absolute_delta: threshold.max_absolute_delta,
            max_standardized_delta: threshold.max_standardized_delta,
            absolute_delta_exceeds_threshold,
            standardized_delta_exceeds_threshold,
        },
        decision_reason_codes,
    }
}

fn evaluate_present_metric(
    metric: &StylizedFactObservedMetricInput,
) -> StylizedFactDatasetMetricEvaluation {
    let horizon_coverage_ratio = if metric.sample_coverage.horizon_interval_count == 0 {
        0.0
    } else {
        metric.sample_coverage.covered_interval_count as f64
            / metric.sample_coverage.horizon_interval_count as f64
    };

    let mut insufficiency_reason_codes: Vec<StylizedFactSampleSufficiencyReasonCode> = Vec::new();

    match metric.value {
        Some(value) if !value.is_finite() => {
            insufficiency_reason_codes
                .push(StylizedFactSampleSufficiencyReasonCode::InvalidPointEstimate);
        }
        Some(_) => {}
        None => insufficiency_reason_codes
            .push(StylizedFactSampleSufficiencyReasonCode::MissingPointEstimate),
    }

    if metric.observed_counts.sample_count < metric.required_minimums.minimum_sample_count {
        insufficiency_reason_codes
            .push(StylizedFactSampleSufficiencyReasonCode::InsufficientSampleCount);
    }

    if metric.observed_counts.effective_sample_count
        < metric.required_minimums.minimum_effective_sample_count
    {
        insufficiency_reason_codes
            .push(StylizedFactSampleSufficiencyReasonCode::InsufficientEffectiveSampleCount);
    }

    if metric.observed_counts.tail_observation_count
        < metric.required_minimums.minimum_tail_observation_count
    {
        insufficiency_reason_codes
            .push(StylizedFactSampleSufficiencyReasonCode::InsufficientTailObservationCount);
    }

    if horizon_coverage_ratio < metric.required_minimums.minimum_horizon_coverage_ratio {
        insufficiency_reason_codes
            .push(StylizedFactSampleSufficiencyReasonCode::InsufficientHorizonCoverageRatio);
    }

    let status = if insufficiency_reason_codes.is_empty() {
        StylizedFactDatasetMetricStatus::Available
    } else {
        StylizedFactDatasetMetricStatus::InsufficientData
    };

    StylizedFactDatasetMetricEvaluation {
        status,
        value: metric.value,
        confidence_interval: metric.confidence_interval,
        sample_coverage: metric.sample_coverage,
        observed_counts: metric.observed_counts,
        required_minimums: metric.required_minimums,
        horizon_coverage_ratio,
        insufficiency_reason_codes,
        input_present: true,
    }
}

fn evaluate_missing_metric(
    template_metric: Option<&StylizedFactObservedMetricInput>,
) -> StylizedFactDatasetMetricEvaluation {
    let (sample_coverage, required_minimums) = match template_metric {
        Some(metric) => (
            StylizedFactMetricSampleCoverage {
                horizon_interval_count: metric.sample_coverage.horizon_interval_count,
                covered_interval_count: 0,
            },
            metric.required_minimums,
        ),
        None => (
            StylizedFactMetricSampleCoverage {
                horizon_interval_count: 0,
                covered_interval_count: 0,
            },
            StylizedFactSampleSufficiencyRequirements {
                minimum_sample_count: 0,
                minimum_effective_sample_count: 0,
                minimum_tail_observation_count: 0,
                minimum_horizon_coverage_ratio: 0.0,
            },
        ),
    };

    StylizedFactDatasetMetricEvaluation {
        status: StylizedFactDatasetMetricStatus::InsufficientData,
        value: None,
        confidence_interval: None,
        sample_coverage,
        observed_counts: StylizedFactObservedCounts {
            sample_count: 0,
            effective_sample_count: 0,
            tail_observation_count: 0,
        },
        required_minimums,
        horizon_coverage_ratio: 0.0,
        insufficiency_reason_codes: vec![StylizedFactSampleSufficiencyReasonCode::MissingMetric],
        input_present: false,
    }
}

fn resolve_threshold_for_metric(
    threshold_policy: &StylizedFactHorizonComparisonThresholdPolicy,
    metric_id: &str,
    metric_variant: Option<&str>,
) -> ResolvedThreshold {
    let mut metric_level_default: Option<&StylizedFactMetricThresholdOverride> = None;

    for override_entry in &threshold_policy.metric_threshold_overrides {
        if override_entry.metric_id != metric_id {
            continue;
        }

        if override_entry.metric_variant.as_deref() == metric_variant {
            return ResolvedThreshold {
                max_absolute_delta: override_entry.max_absolute_delta,
                max_standardized_delta: override_entry.max_standardized_delta,
            };
        }

        if override_entry.metric_variant.is_none() {
            metric_level_default = Some(override_entry);
        }
    }

    if let Some(metric_level_default) = metric_level_default {
        ResolvedThreshold {
            max_absolute_delta: metric_level_default.max_absolute_delta,
            max_standardized_delta: metric_level_default.max_standardized_delta,
        }
    } else {
        ResolvedThreshold {
            max_absolute_delta: threshold_policy.default_max_absolute_delta,
            max_standardized_delta: threshold_policy.default_max_standardized_delta,
        }
    }
}

fn resolve_metric_name(
    empirical_metric: Option<&StylizedFactObservedMetricInput>,
    simulated_metric: Option<&StylizedFactObservedMetricInput>,
    run_position: usize,
    horizon_label: &str,
    metric_id: &str,
    metric_variant: Option<&str>,
) -> Result<String, StylizedFactHorizonComparisonError> {
    match (empirical_metric, simulated_metric) {
        (Some(empirical), Some(simulated)) => {
            if empirical.metric_name != simulated.metric_name {
                let variant_label = metric_variant.unwrap_or("<none>");
                return Err(invalid_request(
                    &format!("calibration_runs[{run_position}]"),
                    &format!(
                        "metric_name mismatch for horizon `{horizon_label}`, metric_id `{metric_id}`, metric_variant `{variant_label}`"
                    ),
                ));
            }
            Ok(empirical.metric_name.clone())
        }
        (Some(empirical), None) => Ok(empirical.metric_name.clone()),
        (None, Some(simulated)) => Ok(simulated.metric_name.clone()),
        (None, None) => Ok(metric_id.to_string()),
    }
}

fn collect_metric_dimensions_for_horizon(
    horizon_label: &str,
    empirical_index: &IndexedDatasetMetrics,
    simulated_index: &IndexedDatasetMetrics,
) -> BTreeSet<MetricDimensionKey> {
    let mut metric_dimensions: BTreeSet<MetricDimensionKey> = BTreeSet::new();

    for metric_key in empirical_index
        .metrics_by_key
        .keys()
        .chain(simulated_index.metrics_by_key.keys())
    {
        if metric_key.horizon_label == horizon_label {
            metric_dimensions.insert(MetricDimensionKey {
                metric_id: metric_key.metric_id.clone(),
                metric_variant: metric_key.metric_variant.clone(),
            });
        }
    }

    metric_dimensions
}

fn index_dataset_metrics(
    dataset_metrics: &[StylizedFactObservedMetricInput],
    field_prefix: &str,
) -> Result<IndexedDatasetMetrics, StylizedFactHorizonComparisonError> {
    let mut metrics_by_key: BTreeMap<MetricKey, StylizedFactObservedMetricInput> = BTreeMap::new();
    let mut horizon_duration_by_label: BTreeMap<String, i64> = BTreeMap::new();

    for (metric_position, metric) in dataset_metrics.iter().enumerate() {
        let metric_field_prefix = format!("{field_prefix}[{metric_position}]");

        validate_metric_input(metric, metric_field_prefix.as_str())?;

        let known_horizon_duration = horizon_duration_by_label
            .insert(metric.horizon_label.clone(), metric.horizon_duration_nanos);
        if let Some(known_horizon_duration) = known_horizon_duration
            && known_horizon_duration != metric.horizon_duration_nanos
        {
            return Err(invalid_request(
                &format!("{metric_field_prefix}.horizon_duration_nanos"),
                &format!(
                    "horizon label `{}` uses mixed durations {} and {} in one dataset",
                    metric.horizon_label, known_horizon_duration, metric.horizon_duration_nanos
                ),
            ));
        }

        let metric_key = MetricKey {
            horizon_label: metric.horizon_label.clone(),
            metric_id: metric.metric_id.clone(),
            metric_variant: metric.metric_variant.clone(),
        };
        if metrics_by_key
            .insert(metric_key.clone(), metric.clone())
            .is_some()
        {
            let metric_variant_label = metric
                .metric_variant
                .as_deref()
                .unwrap_or("<none>")
                .to_string();
            return Err(invalid_request(
                &metric_field_prefix,
                &format!(
                    "duplicate metric key for horizon `{}`, metric_id `{}`, metric_variant `{metric_variant_label}`",
                    metric.horizon_label, metric.metric_id
                ),
            ));
        }
    }

    Ok(IndexedDatasetMetrics {
        metrics_by_key,
        horizon_duration_by_label,
    })
}

fn validate_request(
    request: &StylizedFactHorizonComparisonRequest,
) -> Result<(), StylizedFactHorizonComparisonError> {
    validate_non_empty_string(
        "metric_catalog_id",
        request.metric_catalog_id.as_str(),
        "must be non-empty",
    )?;
    validate_non_empty_string(
        "metric_catalog_version",
        request.metric_catalog_version.as_str(),
        "must be non-empty",
    )?;
    validate_threshold_policy(&request.threshold_policy, "threshold_policy")?;

    if request.calibration_runs.is_empty() {
        return Err(invalid_request(
            "calibration_runs",
            "must define at least one calibration run",
        ));
    }

    let mut calibration_run_ids: BTreeSet<String> = BTreeSet::new();

    for (run_position, run) in request.calibration_runs.iter().enumerate() {
        let run_prefix = format!("calibration_runs[{run_position}]");

        validate_non_empty_string(
            &format!("{run_prefix}.calibration_run_id"),
            run.calibration_run_id.as_str(),
            "must be non-empty",
        )?;

        if !calibration_run_ids.insert(run.calibration_run_id.clone()) {
            return Err(invalid_request(
                &format!("{run_prefix}.calibration_run_id"),
                &format!(
                    "duplicate calibration_run_id `{}` is not allowed",
                    run.calibration_run_id
                ),
            ));
        }

        validate_non_empty_string(
            &format!("{run_prefix}.empirical_dataset_id"),
            run.empirical_dataset_id.as_str(),
            "must be non-empty",
        )?;
        validate_non_empty_string(
            &format!("{run_prefix}.simulated_dataset_id"),
            run.simulated_dataset_id.as_str(),
            "must be non-empty",
        )?;

        let empirical_prefix = format!("{run_prefix}.empirical_metrics");
        let simulated_prefix = format!("{run_prefix}.simulated_metrics");

        index_dataset_metrics(run.empirical_metrics.as_slice(), empirical_prefix.as_str())?;
        index_dataset_metrics(run.simulated_metrics.as_slice(), simulated_prefix.as_str())?;
    }

    Ok(())
}

fn validate_threshold_policy(
    threshold_policy: &StylizedFactHorizonComparisonThresholdPolicy,
    field_prefix: &str,
) -> Result<(), StylizedFactHorizonComparisonError> {
    validate_non_empty_string(
        &format!("{field_prefix}.policy_id"),
        threshold_policy.policy_id.as_str(),
        "must be non-empty",
    )?;
    if let Some(default_max_absolute_delta) = threshold_policy.default_max_absolute_delta {
        validate_non_negative_finite(
            &format!("{field_prefix}.default_max_absolute_delta"),
            default_max_absolute_delta,
        )?;
    }

    if let Some(default_max_standardized_delta) = threshold_policy.default_max_standardized_delta {
        validate_non_negative_finite(
            &format!("{field_prefix}.default_max_standardized_delta"),
            default_max_standardized_delta,
        )?;
    }

    let mut override_keys: BTreeSet<(String, Option<String>)> = BTreeSet::new();

    for (override_position, metric_override) in threshold_policy
        .metric_threshold_overrides
        .iter()
        .enumerate()
    {
        let override_prefix =
            format!("{field_prefix}.metric_threshold_overrides[{override_position}]");

        validate_non_empty_string(
            &format!("{override_prefix}.metric_id"),
            metric_override.metric_id.as_str(),
            "must be non-empty",
        )?;

        if let Some(metric_variant) = metric_override.metric_variant.as_deref() {
            validate_non_empty_string(
                &format!("{override_prefix}.metric_variant"),
                metric_variant,
                "must be non-empty when provided",
            )?;
        }

        if let Some(max_absolute_delta) = metric_override.max_absolute_delta {
            validate_non_negative_finite(
                &format!("{override_prefix}.max_absolute_delta"),
                max_absolute_delta,
            )?;
        }

        if let Some(max_standardized_delta) = metric_override.max_standardized_delta {
            validate_non_negative_finite(
                &format!("{override_prefix}.max_standardized_delta"),
                max_standardized_delta,
            )?;
        }

        let override_key = (
            metric_override.metric_id.clone(),
            metric_override.metric_variant.clone(),
        );

        if !override_keys.insert(override_key) {
            let metric_variant = metric_override
                .metric_variant
                .as_deref()
                .unwrap_or("<none>");

            return Err(invalid_request(
                &override_prefix,
                &format!(
                    "duplicate threshold override for metric_id `{}` and metric_variant `{metric_variant}`",
                    metric_override.metric_id
                ),
            ));
        }
    }

    Ok(())
}

fn validate_metric_input(
    metric: &StylizedFactObservedMetricInput,
    field_prefix: &str,
) -> Result<(), StylizedFactHorizonComparisonError> {
    validate_non_empty_string(
        &format!("{field_prefix}.horizon_label"),
        metric.horizon_label.as_str(),
        "must be non-empty",
    )?;

    if metric.horizon_duration_nanos <= 0 {
        return Err(invalid_request(
            &format!("{field_prefix}.horizon_duration_nanos"),
            &format!(
                "must be greater than 0, found {}",
                metric.horizon_duration_nanos
            ),
        ));
    }

    validate_non_empty_string(
        &format!("{field_prefix}.metric_id"),
        metric.metric_id.as_str(),
        "must be non-empty",
    )?;
    validate_non_empty_string(
        &format!("{field_prefix}.metric_name"),
        metric.metric_name.as_str(),
        "must be non-empty",
    )?;

    if let Some(metric_variant) = metric.metric_variant.as_deref() {
        validate_non_empty_string(
            &format!("{field_prefix}.metric_variant"),
            metric_variant,
            "must be non-empty when provided",
        )?;
    }

    if let Some(value) = metric.value
        && !value.is_finite()
    {
        return Err(invalid_request(
            &format!("{field_prefix}.value"),
            &format!("must be finite when provided, found {value}"),
        ));
    }

    if let Some(confidence_interval) = metric.confidence_interval {
        validate_confidence_interval(
            confidence_interval,
            &format!("{field_prefix}.confidence_interval"),
        )?;
    }

    if metric.sample_coverage.horizon_interval_count == 0 {
        return Err(invalid_request(
            &format!("{field_prefix}.sample_coverage.horizon_interval_count"),
            "must be greater than 0",
        ));
    }

    if metric.sample_coverage.covered_interval_count > metric.sample_coverage.horizon_interval_count
    {
        return Err(invalid_request(
            &format!("{field_prefix}.sample_coverage.covered_interval_count"),
            "must be less than or equal to horizon_interval_count",
        ));
    }

    if metric.observed_counts.effective_sample_count > metric.observed_counts.sample_count {
        return Err(invalid_request(
            &format!("{field_prefix}.observed_counts.effective_sample_count"),
            "must be less than or equal to sample_count",
        ));
    }

    if metric.observed_counts.tail_observation_count > metric.observed_counts.effective_sample_count
    {
        return Err(invalid_request(
            &format!("{field_prefix}.observed_counts.tail_observation_count"),
            "must be less than or equal to effective_sample_count",
        ));
    }

    validate_sample_sufficiency_requirements(
        metric.required_minimums,
        &format!("{field_prefix}.required_minimums"),
    )?;

    Ok(())
}

fn validate_confidence_interval(
    confidence_interval: StylizedFactMetricConfidenceInterval,
    field_prefix: &str,
) -> Result<(), StylizedFactHorizonComparisonError> {
    validate_finite(
        &format!("{field_prefix}.lower_bound"),
        confidence_interval.lower_bound,
    )?;
    validate_finite(
        &format!("{field_prefix}.upper_bound"),
        confidence_interval.upper_bound,
    )?;

    if confidence_interval.upper_bound < confidence_interval.lower_bound {
        return Err(invalid_request(
            &format!("{field_prefix}.upper_bound"),
            &format!(
                "must be greater than or equal to lower_bound {}, found {}",
                confidence_interval.lower_bound, confidence_interval.upper_bound
            ),
        ));
    }

    if !confidence_interval.confidence_level.is_finite()
        || confidence_interval.confidence_level <= 0.0
        || confidence_interval.confidence_level >= 1.0
    {
        return Err(invalid_request(
            &format!("{field_prefix}.confidence_level"),
            &format!(
                "must be finite and in (0, 1), found {}",
                confidence_interval.confidence_level
            ),
        ));
    }

    Ok(())
}

fn validate_sample_sufficiency_requirements(
    requirements: StylizedFactSampleSufficiencyRequirements,
    field_prefix: &str,
) -> Result<(), StylizedFactHorizonComparisonError> {
    if requirements.minimum_sample_count == 0 {
        return Err(invalid_request(
            &format!("{field_prefix}.minimum_sample_count"),
            "must be greater than 0",
        ));
    }

    if requirements.minimum_effective_sample_count == 0
        || requirements.minimum_effective_sample_count > requirements.minimum_sample_count
    {
        return Err(invalid_request(
            &format!("{field_prefix}.minimum_effective_sample_count"),
            &format!(
                "must be in [1, minimum_sample_count], found {}",
                requirements.minimum_effective_sample_count
            ),
        ));
    }

    if requirements.minimum_tail_observation_count > requirements.minimum_effective_sample_count {
        return Err(invalid_request(
            &format!("{field_prefix}.minimum_tail_observation_count"),
            &format!(
                "must be less than or equal to minimum_effective_sample_count, found {}",
                requirements.minimum_tail_observation_count
            ),
        ));
    }

    if !requirements.minimum_horizon_coverage_ratio.is_finite()
        || requirements.minimum_horizon_coverage_ratio <= 0.0
        || requirements.minimum_horizon_coverage_ratio > 1.0
    {
        return Err(invalid_request(
            &format!("{field_prefix}.minimum_horizon_coverage_ratio"),
            &format!(
                "must be finite and in (0, 1], found {}",
                requirements.minimum_horizon_coverage_ratio
            ),
        ));
    }

    Ok(())
}

fn validate_non_empty_string(
    field_path: &str,
    value: &str,
    details: &str,
) -> Result<(), StylizedFactHorizonComparisonError> {
    if !is_non_empty_string(value) {
        return Err(invalid_request(field_path, details));
    }

    Ok(())
}

fn validate_non_negative_finite(
    field_path: &str,
    value: f64,
) -> Result<(), StylizedFactHorizonComparisonError> {
    if !value.is_finite() || value < 0.0 {
        return Err(invalid_request(
            field_path,
            &format!("must be finite and non-negative, found {value}"),
        ));
    }

    Ok(())
}

fn validate_finite(field_path: &str, value: f64) -> Result<(), StylizedFactHorizonComparisonError> {
    if !value.is_finite() {
        return Err(invalid_request(
            field_path,
            &format!("must be finite, found {value}"),
        ));
    }

    Ok(())
}

fn invalid_request(field_path: &str, details: &str) -> StylizedFactHorizonComparisonError {
    StylizedFactHorizonComparisonError::InvalidRequest {
        field_path: field_path.to_string(),
        details: details.to_string(),
    }
}

fn confidence_interval_to_bounds(
    confidence_interval: StylizedFactMetricConfidenceInterval,
) -> Option<(f64, f64)> {
    let lower_bound = confidence_interval.lower_bound;
    let upper_bound = confidence_interval.upper_bound;

    if lower_bound.is_finite() && upper_bound.is_finite() && upper_bound >= lower_bound {
        Some((lower_bound, upper_bound))
    } else {
        None
    }
}

fn aggregate_horizon_metric_decision_counts(
    horizon_reports: &[StylizedFactHorizonComparison],
) -> StylizedFactDecisionCounts {
    let mut decision_counts = StylizedFactDecisionCounts {
        pass_count: 0,
        fail_count: 0,
        insufficient_data_count: 0,
    };

    for horizon_report in horizon_reports {
        merge_decision_counts(&mut decision_counts, horizon_report.decision_counts);
    }

    decision_counts
}

fn count_metric_decisions(
    metric_reports: &[StylizedFactMetricComparison],
) -> StylizedFactDecisionCounts {
    let mut decision_counts = StylizedFactDecisionCounts {
        pass_count: 0,
        fail_count: 0,
        insufficient_data_count: 0,
    };

    for metric_report in metric_reports {
        increment_decision_count(&mut decision_counts, metric_report.status);
    }

    decision_counts
}

fn aggregate_run_decision_counts(
    run_reports: &[StylizedFactCalibrationRunComparisonReport],
) -> StylizedFactDecisionCounts {
    let mut decision_counts = StylizedFactDecisionCounts {
        pass_count: 0,
        fail_count: 0,
        insufficient_data_count: 0,
    };

    for run_report in run_reports {
        increment_decision_count(&mut decision_counts, run_report.status);
    }

    decision_counts
}

fn aggregate_metric_decision_counts(
    run_reports: &[StylizedFactCalibrationRunComparisonReport],
) -> StylizedFactDecisionCounts {
    let mut decision_counts = StylizedFactDecisionCounts {
        pass_count: 0,
        fail_count: 0,
        insufficient_data_count: 0,
    };

    for run_report in run_reports {
        merge_decision_counts(&mut decision_counts, run_report.decision_counts);
    }

    decision_counts
}

fn increment_decision_count(
    decision_counts: &mut StylizedFactDecisionCounts,
    decision_state: StylizedFactComparisonDecisionState,
) {
    match decision_state {
        StylizedFactComparisonDecisionState::Pass => decision_counts.pass_count += 1,
        StylizedFactComparisonDecisionState::Fail => decision_counts.fail_count += 1,
        StylizedFactComparisonDecisionState::InsufficientData => {
            decision_counts.insufficient_data_count += 1;
        }
    }
}

fn merge_decision_counts(
    target: &mut StylizedFactDecisionCounts,
    source: StylizedFactDecisionCounts,
) {
    target.pass_count += source.pass_count;
    target.fail_count += source.fail_count;
    target.insufficient_data_count += source.insufficient_data_count;
}

fn aggregate_decision_status(
    decision_counts: StylizedFactDecisionCounts,
) -> StylizedFactComparisonDecisionState {
    if decision_counts.fail_count > 0 {
        StylizedFactComparisonDecisionState::Fail
    } else if decision_counts.insufficient_data_count > 0 {
        StylizedFactComparisonDecisionState::InsufficientData
    } else {
        StylizedFactComparisonDecisionState::Pass
    }
}
