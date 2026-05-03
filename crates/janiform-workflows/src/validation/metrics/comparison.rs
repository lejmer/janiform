use std::collections::BTreeMap;

use janiform::validation::stylized_facts::{
    StylizedFactCalibrationRunInput, StylizedFactHorizonComparisonRequest,
    StylizedFactHorizonComparisonThresholdPolicy, StylizedFactMetricCatalog,
    StylizedFactMetricConfidenceInterval, StylizedFactMetricSampleCoverage,
    StylizedFactMetricThresholdOverride, StylizedFactObservedCounts,
    StylizedFactObservedMetricInput, StylizedFactSampleSufficiencyRequirements,
};
use janiform::validation::tail_skew::{
    TAIL_SKEW_DATASET_METRIC_TASK_COUNT, TailSkewBlockBootstrapConfiguration,
    TailSkewConfidenceInterval, TailSkewDatasetInput, TailSkewDatasetMetricResult,
    TailSkewDatasetRole, TailSkewDiagnosticsProgressEvent, TailSkewDiagnosticsRequest,
    TailSkewHorizonBucket, TailSkewMetricComparison, TailSkewMetricSampleSufficiencyPolicy,
    TailSkewObservedCounts, TailSkewReturnObservation, TailSkewSampleCoverage,
    TailSkewSampleSufficiencyPolicy, run_tail_skew_diagnostics_with_observer,
};

use super::super::empirical::{ParsedInterval, ReturnObservation, analysis_window};
use super::super::request::StylizedFactsValidationWorkflowRequest;
use super::super::workflow::StylizedFactsValidationWorkflowObserver;
use super::progress::{
    emit_metric_completed, emit_metric_plan, emit_metric_started, emit_metric_task_completed,
};
use crate::error::WorkflowError;

const GENERATED_THRESHOLD_POLICY_ID: &str =
    "bootstrap_delta_confidence_interval_non_overlap_policy.v1";
// A standardized delta of one is exactly the pooled confidence-interval half-width boundary.
const STANDARDIZED_DELTA_INTERVAL_NON_OVERLAP_THRESHOLD: f64 = 1.0;

pub(in crate::validation) fn build_comparison_request(
    metric_catalog: &StylizedFactMetricCatalog,
    threshold_policy: StylizedFactHorizonComparisonThresholdPolicy,
    intervals: &[ParsedInterval],
    empirical_returns: &[ReturnObservation],
    simulated_returns: &[ReturnObservation],
    bootstrap_configuration: TailSkewBlockBootstrapConfiguration,
    cpu_count: usize,
    observer: &mut StylizedFactsValidationWorkflowObserver<'_>,
) -> Result<StylizedFactHorizonComparisonRequest, WorkflowError> {
    let base_interval_duration_nanos = base_interval_duration_nanos(intervals).ok_or(
        WorkflowError::ValidationMissingRequired {
            field: "--intervals",
        },
    )?;

    let empirical_series =
        compact_observed_interval_timeline(empirical_returns, base_interval_duration_nanos)?;
    let simulated_series =
        compact_observed_interval_timeline(simulated_returns, base_interval_duration_nanos)?;

    let interval_metrics = compute_interval_metrics(
        empirical_series,
        simulated_series,
        intervals,
        bootstrap_configuration,
        cpu_count,
        observer,
    )?;

    let mut empirical_metrics = Vec::new();
    let mut simulated_metrics = Vec::new();

    for interval_output in interval_metrics {
        empirical_metrics.extend(interval_output.empirical_metrics);
        simulated_metrics.extend(interval_output.simulated_metrics);
    }

    Ok(StylizedFactHorizonComparisonRequest {
        metric_catalog_id: metric_catalog.catalog_id.clone(),
        metric_catalog_version: metric_catalog.catalog_version.clone(),
        threshold_policy,
        calibration_runs: vec![StylizedFactCalibrationRunInput {
            calibration_run_id: "validation".to_string(),
            empirical_dataset_id: "empirical".to_string(),
            simulated_dataset_id: "simulated".to_string(),
            empirical_metrics,
            simulated_metrics,
        }],
    })
}

fn compute_interval_metrics(
    empirical_series: CompactedReturnSeries,
    simulated_series: CompactedReturnSeries,
    intervals: &[ParsedInterval],
    bootstrap_configuration: TailSkewBlockBootstrapConfiguration,
    cpu_count: usize,
    observer: &mut StylizedFactsValidationWorkflowObserver<'_>,
) -> Result<Vec<IntervalMetricOutput>, WorkflowError> {
    let planned_work_item_labels = intervals
        .iter()
        .map(|interval| interval.label.clone())
        .collect::<Vec<_>>();
    let total_metric_units =
        metric_progress_total_units(intervals.len(), TAIL_SKEW_DATASET_METRIC_TASK_COUNT)?;

    emit_metric_plan(
        observer,
        planned_work_item_labels,
        intervals.len(),
        total_metric_units,
        cpu_count,
    );

    let mut outputs = Vec::with_capacity(intervals.len());
    let mut completed_units = 0usize;

    for interval in intervals.iter().cloned() {
        emit_metric_started(
            observer,
            interval.label.as_str(),
            completed_units,
            total_metric_units,
            cpu_count,
            TAIL_SKEW_DATASET_METRIC_TASK_COUNT,
        );

        // Intentional copy because each interval owns the diagnostics request payload.
        let empirical_series = empirical_series.clone();

        // Intentional copy because each interval owns the diagnostics request payload.
        let simulated_series = simulated_series.clone();

        let output = compute_interval_metric_output(
            interval,
            empirical_series,
            simulated_series,
            bootstrap_configuration,
            cpu_count,
            completed_units,
            total_metric_units,
            observer,
        )?;

        completed_units += TAIL_SKEW_DATASET_METRIC_TASK_COUNT;

        emit_metric_completed(
            observer,
            output.interval.label.as_str(),
            output.metric_count(),
            completed_units,
            total_metric_units,
            cpu_count,
            TAIL_SKEW_DATASET_METRIC_TASK_COUNT,
        );

        outputs.push(output);
    }

    Ok(outputs)
}

fn compute_interval_metric_output(
    interval: ParsedInterval,
    empirical_series: CompactedReturnSeries,
    simulated_series: CompactedReturnSeries,
    bootstrap_configuration: TailSkewBlockBootstrapConfiguration,
    cpu_count: usize,
    completed_units_before_interval: usize,
    total_metric_units: usize,
    observer: &mut StylizedFactsValidationWorkflowObserver<'_>,
) -> Result<IntervalMetricOutput, WorkflowError> {
    let horizon_bucket = TailSkewHorizonBucket {
        horizon_label: interval.label.clone(),
        horizon_duration_nanos: interval.duration_nanos,
    };

    let empirical_metrics = observed_metrics_for_single_dataset(
        "empirical",
        empirical_series,
        &horizon_bucket,
        bootstrap_configuration,
        TailSkewDatasetSide::Empirical,
        cpu_count,
        completed_units_before_interval,
        0,
        total_metric_units,
        observer,
    )?;

    let side_task_count = TAIL_SKEW_DATASET_METRIC_TASK_COUNT / 2;

    let simulated_metrics = observed_metrics_for_single_dataset(
        "simulated",
        simulated_series,
        &horizon_bucket,
        bootstrap_configuration,
        TailSkewDatasetSide::Simulated,
        cpu_count,
        completed_units_before_interval + side_task_count,
        side_task_count,
        total_metric_units,
        observer,
    )?;

    Ok(IntervalMetricOutput {
        interval,
        empirical_metrics,
        simulated_metrics,
    })
}

fn metric_progress_total_units(
    interval_count: usize,
    metric_task_count: usize,
) -> Result<usize, WorkflowError> {
    interval_count
        .checked_mul(metric_task_count)
        .ok_or_else(|| WorkflowError::ValidationInputFailed {
            field: "validate stylized-facts",
            message: "metric progress unit count does not fit in usize".to_string(),
        })
}

fn observed_metrics_for_single_dataset(
    dataset_id: &str,
    series: CompactedReturnSeries,
    horizon_bucket: &TailSkewHorizonBucket,
    bootstrap_configuration: TailSkewBlockBootstrapConfiguration,
    dataset_side: TailSkewDatasetSide,
    cpu_count: usize,
    completed_units_before_dataset: usize,
    interval_completed_units_before_dataset: usize,
    total_metric_units: usize,
    observer: &mut StylizedFactsValidationWorkflowObserver<'_>,
) -> Result<Vec<StylizedFactObservedMetricInput>, WorkflowError> {
    let return_observations = series.return_observations;
    let dataset_role = dataset_side.dataset_role();

    let mut dataset_completed_units = 0usize;

    let mut progress_observer = |event: TailSkewDiagnosticsProgressEvent| {
        if event.dataset_role != dataset_role {
            return;
        }

        dataset_completed_units += 1;

        emit_metric_task_completed(
            observer,
            event.bucket_label.as_str(),
            event.dataset_role,
            event.metric_id,
            completed_units_before_dataset + dataset_completed_units,
            total_metric_units,
            interval_completed_units_before_dataset + dataset_completed_units,
            TAIL_SKEW_DATASET_METRIC_TASK_COUNT,
            cpu_count,
        );
    };

    let diagnostics = run_tail_skew_diagnostics_with_observer(
        TailSkewDiagnosticsRequest {
            analysis_start_utc_nanos: series.analysis_start_utc_nanos,
            analysis_end_utc_nanos: series.analysis_end_utc_nanos,
            empirical_dataset: TailSkewDatasetInput {
                dataset_id: dataset_id.to_string(),
                // Intentional copy because the single-dataset diagnostic adapter
                // must populate both sides of the tail/skew diagnostics contract.
                return_observations: return_observations.clone(),
            },
            simulated_dataset: TailSkewDatasetInput {
                dataset_id: dataset_id.to_string(),
                return_observations,
            },
            horizon_buckets: vec![TailSkewHorizonBucket {
                horizon_label: horizon_bucket.horizon_label.clone(),
                horizon_duration_nanos: horizon_bucket.horizon_duration_nanos,
            }],
            bootstrap_configuration,
            sample_sufficiency_policy: TailSkewSampleSufficiencyPolicy::default(),
            cpu_count: Some(cpu_count),
        },
        &mut progress_observer,
    )
    .map_err(|source| WorkflowError::ValidationInputFailed {
        field: "validate stylized-facts",
        message: source.to_string(),
    })?;

    let mut metrics = Vec::new();

    for horizon in &diagnostics.horizons {
        for metric in &horizon.metrics {
            let dataset_metric = match dataset_side {
                TailSkewDatasetSide::Empirical => &metric.empirical,
                TailSkewDatasetSide::Simulated => &metric.simulated,
            };

            metrics.push(observed_metric_from_tail_skew(
                horizon.horizon_label.as_str(),
                horizon.horizon_duration_nanos,
                metric,
                dataset_metric,
            ));
        }
    }

    Ok(metrics)
}

#[derive(Clone, Copy)]
enum TailSkewDatasetSide {
    Empirical,
    Simulated,
}

impl TailSkewDatasetSide {
    fn dataset_role(self) -> TailSkewDatasetRole {
        match self {
            Self::Empirical => TailSkewDatasetRole::Empirical,
            Self::Simulated => TailSkewDatasetRole::Simulated,
        }
    }
}

struct IntervalMetricOutput {
    interval: ParsedInterval,
    empirical_metrics: Vec<StylizedFactObservedMetricInput>,
    simulated_metrics: Vec<StylizedFactObservedMetricInput>,
}

impl IntervalMetricOutput {
    fn metric_count(&self) -> usize {
        self.empirical_metrics.len()
    }
}

pub(in crate::validation) fn threshold_policy_for_options(
    request: &StylizedFactsValidationWorkflowRequest,
    metric_catalog: &StylizedFactMetricCatalog,
) -> Result<StylizedFactHorizonComparisonThresholdPolicy, WorkflowError> {
    if let Some(path) = &request.threshold_policy {
        return super::super::artifacts::load_typed_json_artifact(path);
    }

    let alpha = request
        .significance_level
        .unwrap_or(1.0 - metric_catalog.uncertainty_policy.default_confidence_level);

    Ok(StylizedFactHorizonComparisonThresholdPolicy {
        policy_id: format!("{GENERATED_THRESHOLD_POLICY_ID}:alpha={alpha}"),
        default_max_absolute_delta: None,
        default_max_standardized_delta: Some(STANDARDIZED_DELTA_INTERVAL_NON_OVERLAP_THRESHOLD),
        metric_threshold_overrides: Vec::<StylizedFactMetricThresholdOverride>::new(),
    })
}

pub(in crate::validation) fn bootstrap_configuration_for_options(
    request: &StylizedFactsValidationWorkflowRequest,
    metric_catalog: &StylizedFactMetricCatalog,
) -> TailSkewBlockBootstrapConfiguration {
    let confidence_level = request
        .significance_level
        .map(|significance_level| 1.0 - significance_level)
        .unwrap_or(metric_catalog.uncertainty_policy.default_confidence_level);

    TailSkewBlockBootstrapConfiguration {
        confidence_level,
        replication_count: metric_catalog
            .uncertainty_policy
            .default_bootstrap_replication_count,
        block_length_interval_count: metric_catalog
            .uncertainty_policy
            .default_block_length_horizon_count,
        random_seed: request.seed,
    }
}

fn base_interval_duration_nanos(intervals: &[ParsedInterval]) -> Option<i64> {
    intervals
        .iter()
        .map(|interval| interval.duration_nanos)
        .min()
}

fn compact_observed_interval_timeline(
    returns: &[ReturnObservation],
    base_interval_duration_nanos: i64,
) -> Result<CompactedReturnSeries, WorkflowError> {
    if base_interval_duration_nanos <= 0 {
        return Err(WorkflowError::ValidationInputFailed {
            field: "--intervals",
            message: "base interval duration must be positive".to_string(),
        });
    }

    let analysis_window = analysis_window(returns)?;

    let mut interval_positions = BTreeMap::<i128, usize>::new();
    let mut compacted_returns = Vec::with_capacity(returns.len());

    for observation in returns {
        let offset_nanos = i128::from(observation.timestamp_utc_nanos)
            - i128::from(analysis_window.start_utc_nanos);
        if offset_nanos < 0 {
            continue;
        }

        let base_duration = i128::from(base_interval_duration_nanos);

        let original_interval_position = offset_nanos / base_duration;
        let next_compact_position = interval_positions.len();
        let compact_interval_position = *interval_positions
            .entry(original_interval_position)
            .or_insert(next_compact_position);

        let offset_within_interval = offset_nanos % base_duration;

        let compact_timestamp = i128::from(analysis_window.start_utc_nanos)
            + i128::try_from(compact_interval_position).map_err(|_| {
                WorkflowError::ValidationInputFailed {
                    field: "validate stylized-facts",
                    message: "compacted interval position does not fit in i128".to_string(),
                }
            })? * base_duration
            + offset_within_interval;

        let timestamp_utc_nanos =
            i64::try_from(compact_timestamp).map_err(|_| WorkflowError::ValidationInputFailed {
                field: "validate stylized-facts",
                message: "compacted timestamp does not fit in i64".to_string(),
            })?;

        compacted_returns.push(TailSkewReturnObservation {
            timestamp_utc_nanos,
            return_increment: observation.return_increment,
        });
    }

    let compact_interval_count = interval_positions.len();
    if compact_interval_count == 0 {
        return Err(WorkflowError::ValidationInputFailed {
            field: "--empirical-data-path",
            message: "must contain observed return intervals".to_string(),
        });
    }

    let analysis_span_nanos = i128::try_from(compact_interval_count).map_err(|_| {
        WorkflowError::ValidationInputFailed {
            field: "validate stylized-facts",
            message: "compacted interval count does not fit in i128".to_string(),
        }
    })? * i128::from(base_interval_duration_nanos);

    let analysis_end_utc_nanos = i64::try_from(
        i128::from(analysis_window.start_utc_nanos) + analysis_span_nanos,
    )
    .map_err(|_| WorkflowError::ValidationInputFailed {
        field: "validate stylized-facts",
        message: "compacted analysis window end does not fit in i64".to_string(),
    })?;

    Ok(CompactedReturnSeries {
        analysis_start_utc_nanos: analysis_window.start_utc_nanos,
        analysis_end_utc_nanos,
        return_observations: compacted_returns,
    })
}

#[derive(Clone)]
struct CompactedReturnSeries {
    analysis_start_utc_nanos: i64,
    analysis_end_utc_nanos: i64,
    return_observations: Vec<TailSkewReturnObservation>,
}

fn observed_metric_from_tail_skew(
    interval_label: &str,
    interval_duration_nanos: i64,
    metric: &TailSkewMetricComparison,
    dataset_metric: &TailSkewDatasetMetricResult,
) -> StylizedFactObservedMetricInput {
    StylizedFactObservedMetricInput {
        horizon_label: interval_label.to_string(),
        horizon_duration_nanos: interval_duration_nanos,
        metric_id: metric.metric_id.to_string(),
        metric_name: metric.metric_name.to_string(),
        metric_variant: None,
        value: dataset_metric.value,
        confidence_interval: dataset_metric
            .confidence_interval
            .map(stylized_fact_confidence_interval),
        sample_coverage: stylized_fact_sample_coverage(dataset_metric.sample_coverage),
        observed_counts: stylized_fact_observed_counts(dataset_metric.observed_counts),
        required_minimums: stylized_fact_required_minimums(dataset_metric.required_minimums),
    }
}

fn stylized_fact_confidence_interval(
    confidence_interval: TailSkewConfidenceInterval,
) -> StylizedFactMetricConfidenceInterval {
    StylizedFactMetricConfidenceInterval {
        lower_bound: confidence_interval.lower_bound,
        upper_bound: confidence_interval.upper_bound,
        confidence_level: confidence_interval.confidence_level,
    }
}

fn stylized_fact_sample_coverage(
    sample_coverage: TailSkewSampleCoverage,
) -> StylizedFactMetricSampleCoverage {
    StylizedFactMetricSampleCoverage {
        horizon_interval_count: sample_coverage.horizon_interval_count,
        covered_interval_count: sample_coverage.covered_interval_count,
    }
}

fn stylized_fact_observed_counts(
    observed_counts: TailSkewObservedCounts,
) -> StylizedFactObservedCounts {
    StylizedFactObservedCounts {
        sample_count: observed_counts.sample_count,
        effective_sample_count: observed_counts.effective_sample_count,
        tail_observation_count: observed_counts.tail_observation_count,
    }
}

fn stylized_fact_required_minimums(
    required_minimums: TailSkewMetricSampleSufficiencyPolicy,
) -> StylizedFactSampleSufficiencyRequirements {
    StylizedFactSampleSufficiencyRequirements {
        minimum_sample_count: required_minimums.minimum_sample_count,
        minimum_effective_sample_count: required_minimums.minimum_effective_sample_count,
        minimum_tail_observation_count: required_minimums.minimum_tail_observation_count,
        minimum_horizon_coverage_ratio: required_minimums.minimum_horizon_coverage_ratio,
    }
}

#[cfg(test)]
mod tests {
    use janiform::validation::stylized_facts::{
        StylizedFactHorizonComparisonThresholdPolicy, build_stylized_fact_metric_catalog,
    };
    use janiform::validation::tail_skew::TailSkewBlockBootstrapConfiguration;

    use super::{build_comparison_request, compact_observed_interval_timeline};
    use crate::validation::empirical::{ParsedInterval, ReturnObservation};

    const FIVE_MINUTES_NANOS: i64 = 300_000_000_000;

    #[test]
    fn compact_observed_interval_timeline_removes_calendar_gaps() {
        let two_days_nanos = 2 * 24 * 60 * FIVE_MINUTES_NANOS;

        let returns = vec![
            ReturnObservation {
                timestamp_utc_nanos: 1_000_000_000,
                return_increment: 0.01,
            },
            ReturnObservation {
                timestamp_utc_nanos: 1_000_000_000 + FIVE_MINUTES_NANOS,
                return_increment: 0.02,
            },
            ReturnObservation {
                timestamp_utc_nanos: 1_000_000_000 + two_days_nanos,
                return_increment: -0.01,
            },
            ReturnObservation {
                timestamp_utc_nanos: 1_000_000_000 + two_days_nanos + FIVE_MINUTES_NANOS,
                return_increment: 0.03,
            },
        ];

        let compacted = compact_observed_interval_timeline(&returns, FIVE_MINUTES_NANOS)
            .expect("calendar gaps should compact");

        assert_eq!(compacted.analysis_start_utc_nanos, 1_000_000_000);
        assert_eq!(
            compacted.analysis_end_utc_nanos,
            1_000_000_000 + 4 * FIVE_MINUTES_NANOS
        );
        assert_eq!(
            compacted
                .return_observations
                .iter()
                .map(|observation| observation.timestamp_utc_nanos)
                .collect::<Vec<_>>(),
            vec![
                1_000_000_000,
                1_000_000_000 + FIVE_MINUTES_NANOS,
                1_000_000_000 + 2 * FIVE_MINUTES_NANOS,
                1_000_000_000 + 3 * FIVE_MINUTES_NANOS,
            ]
        );
    }

    #[test]
    fn build_comparison_request_counts_concatenated_simulated_paths_as_samples() {
        let interval = ParsedInterval {
            label: "5m".to_string(),
            duration_nanos: FIVE_MINUTES_NANOS,
        };

        let empirical_returns = synthetic_returns(500, 0, 0.001);
        let simulated_returns = synthetic_returns(1_000, 0, 0.002);

        let metric_catalog = build_stylized_fact_metric_catalog();

        let mut progress_events = Vec::new();

        let comparison_request = build_comparison_request(
            &metric_catalog,
            StylizedFactHorizonComparisonThresholdPolicy {
                policy_id: "test_policy".to_string(),
                default_max_absolute_delta: None,
                default_max_standardized_delta: Some(1.0),
                metric_threshold_overrides: Vec::new(),
            },
            &[interval],
            &empirical_returns,
            &simulated_returns,
            TailSkewBlockBootstrapConfiguration {
                confidence_level: 0.95,
                replication_count: 1,
                block_length_interval_count: 1,
                random_seed: 7,
            },
            2,
            &mut |event| progress_events.push(event),
        )
        .expect("comparison request should be built");

        let run = comparison_request
            .calibration_runs
            .first()
            .expect("validation run should be present");

        assert_eq!(run.empirical_metrics[0].observed_counts.sample_count, 500);
        assert_eq!(run.simulated_metrics[0].observed_counts.sample_count, 1_000);
        assert!(
            progress_events.iter().any(|event| {
                event.planned_work_item_labels == vec!["5m".to_string()]
                    && event
                        .work_item_columns
                        .iter()
                        .map(|column| column.label.as_str())
                        .collect::<Vec<_>>()
                        == vec!["Family", "Dataset", "Metric", "Checks", "Tasks", "CPU"]
            }),
            "metric progress should seed the interval work table"
        );
        assert!(
            progress_events.iter().any(|event| {
                event.work_item_label.as_deref() == Some("5m")
                    && event.completed_units == Some(9)
                    && event.total_units == Some(18)
                    && event.work_item_progress_percent == Some(50)
                    && event
                        .work_item_metrics
                        .iter()
                        .any(|metric| metric.key == "dataset" && metric.value == "empirical")
                    && event
                        .work_item_metrics
                        .iter()
                        .any(|metric| metric.key == "checks" && metric.value == "9/18")
            }),
            "metric progress should emit one update per empirical dataset metric task"
        );
        assert!(
            progress_events.iter().any(|event| {
                event.work_item_label.as_deref() == Some("5m")
                    && event.completed_units == Some(10)
                    && event.total_units == Some(18)
                    && event
                        .work_item_metrics
                        .iter()
                        .any(|metric| metric.key == "dataset" && metric.value == "simulated")
                    && event
                        .work_item_metrics
                        .iter()
                        .any(|metric| metric.key == "checks" && metric.value == "10/18")
            }),
            "metric progress should continue with simulated dataset metric tasks"
        );
        assert!(
            progress_events.iter().any(|event| {
                event.work_item_label.as_deref() == Some("5m")
                    && event.work_item_progress_percent == Some(100)
                    && event
                        .work_item_metrics
                        .iter()
                        .any(|metric| metric.key == "cpu" && metric.value == "2")
                    && event
                        .work_item_metrics
                        .iter()
                        .any(|metric| metric.key == "tasks" && metric.value == "18")
            }),
            "metric progress should mark completed interval work items"
        );
    }

    fn synthetic_returns(
        observation_count: usize,
        start_utc_nanos: i64,
        return_scale: f64,
    ) -> Vec<ReturnObservation> {
        (0..observation_count)
            .map(|observation_position| ReturnObservation {
                timestamp_utc_nanos: start_utc_nanos
                    + i64::try_from(observation_position)
                        .expect("test observation position should fit in i64")
                        * FIVE_MINUTES_NANOS,
                return_increment: return_scale * (observation_position as f64 + 1.0),
            })
            .collect()
    }
}
