use janiform_validation::{
    STYLIZED_FACT_HORIZON_COMPARISON_REPORT_SCHEMA_VERSION, StylizedFactCalibrationRunInput,
    StylizedFactComparisonDecisionState, StylizedFactDecisionReasonCode,
    StylizedFactHorizonComparisonRequest, StylizedFactHorizonComparisonThresholdPolicy,
    StylizedFactMetricAlignmentStatus, StylizedFactMetricConfidenceInterval,
    StylizedFactMetricSampleCoverage, StylizedFactMetricThresholdOverride,
    StylizedFactObservedCounts, StylizedFactObservedMetricInput,
    StylizedFactSampleSufficiencyReasonCode, StylizedFactSampleSufficiencyRequirements,
    run_stylized_fact_horizon_comparison_engine,
};
use serde_json::Value;

#[test]
fn horizon_alignment_and_delta_threshold_classification_follow_policy() {
    let request = StylizedFactHorizonComparisonRequest {
        metric_catalog_id: "catalog".to_string(),
        metric_catalog_version: "1.0.0".to_string(),
        threshold_policy: threshold_policy(),
        calibration_runs: vec![StylizedFactCalibrationRunInput {
            calibration_run_id: "run_alignment".to_string(),
            empirical_dataset_id: "empirical".to_string(),
            simulated_dataset_id: "simulated".to_string(),
            empirical_metrics: vec![
                metric("1h", 3_600, "metric_pass", "Metric Pass", None, 1.00),
                metric("1h", 3_600, "metric_fail", "Metric Fail", None, 0.10),
                metric(
                    "2h",
                    7_200,
                    "metric_mismatch",
                    "Metric Mismatch",
                    None,
                    0.20,
                ),
                metric(
                    "4h",
                    14_400,
                    "metric_missing_sim",
                    "Metric Missing Sim",
                    None,
                    0.30,
                ),
            ],
            simulated_metrics: vec![
                metric("1h", 3_600, "metric_pass", "Metric Pass", None, 1.05),
                metric("1h", 3_600, "metric_fail", "Metric Fail", None, 0.50),
                metric(
                    "2h",
                    7_500,
                    "metric_mismatch",
                    "Metric Mismatch",
                    None,
                    0.22,
                ),
            ],
        }],
    };

    let report = run_stylized_fact_horizon_comparison_engine(request)
        .expect("horizon comparison engine should run on fixture");

    assert_eq!(
        report.schema_version,
        STYLIZED_FACT_HORIZON_COMPARISON_REPORT_SCHEMA_VERSION
    );
    assert_eq!(report.status, StylizedFactComparisonDecisionState::Fail);
    assert_eq!(report.run_reports.len(), 1);

    let run_report = &report.run_reports[0];
    assert_eq!(run_report.total_horizon_count, 3);
    assert_eq!(run_report.total_metric_count, 4);
    assert_eq!(run_report.status, StylizedFactComparisonDecisionState::Fail);
    assert_eq!(run_report.decision_counts.pass_count, 1);
    assert_eq!(run_report.decision_counts.fail_count, 1);
    assert_eq!(run_report.decision_counts.insufficient_data_count, 2);

    let one_hour_horizon = run_report
        .horizons
        .iter()
        .find(|horizon| horizon.horizon_label == "1h")
        .expect("1h horizon should be present");
    assert_eq!(one_hour_horizon.metrics.len(), 2);

    let pass_metric = one_hour_horizon
        .metrics
        .iter()
        .find(|metric| metric.metric_id == "metric_pass")
        .expect("metric_pass should be present");
    assert_eq!(
        pass_metric.status,
        StylizedFactComparisonDecisionState::Pass
    );
    assert_eq!(
        pass_metric.alignment_status,
        StylizedFactMetricAlignmentStatus::Matched
    );
    assert_eq!(
        pass_metric
            .threshold_context
            .absolute_delta_exceeds_threshold,
        Some(false)
    );
    assert!(
        pass_metric.uncertainty_context.standardized_delta.is_some(),
        "standardized delta should be available when both confidence intervals exist",
    );

    let fail_metric = one_hour_horizon
        .metrics
        .iter()
        .find(|metric| metric.metric_id == "metric_fail")
        .expect("metric_fail should be present");
    assert_eq!(
        fail_metric.status,
        StylizedFactComparisonDecisionState::Fail
    );
    assert_eq!(
        fail_metric
            .threshold_context
            .absolute_delta_exceeds_threshold,
        Some(true)
    );
    assert!(
        fail_metric
            .decision_reason_codes
            .contains(&StylizedFactDecisionReasonCode::AbsoluteDeltaThresholdExceeded)
    );

    let mismatch_metric = run_report
        .horizons
        .iter()
        .find(|horizon| horizon.horizon_label == "2h")
        .and_then(|horizon| horizon.metrics.first())
        .expect("mismatch metric should be present");
    assert_eq!(
        mismatch_metric.alignment_status,
        StylizedFactMetricAlignmentStatus::HorizonDurationMismatch
    );
    assert_eq!(
        mismatch_metric.status,
        StylizedFactComparisonDecisionState::InsufficientData
    );

    let missing_metric = run_report
        .horizons
        .iter()
        .find(|horizon| horizon.horizon_label == "4h")
        .and_then(|horizon| horizon.metrics.first())
        .expect("missing metric should be present");
    assert_eq!(
        missing_metric.alignment_status,
        StylizedFactMetricAlignmentStatus::MissingSimulatedMetric
    );
    assert_eq!(
        missing_metric.status,
        StylizedFactComparisonDecisionState::InsufficientData
    );
    assert!(
        missing_metric
            .simulated
            .insufficiency_reason_codes
            .contains(&StylizedFactSampleSufficiencyReasonCode::MissingMetric)
    );
}

#[test]
fn comparison_report_schema_and_ordering_are_deterministic() {
    let request = StylizedFactHorizonComparisonRequest {
        metric_catalog_id: "catalog".to_string(),
        metric_catalog_version: "1.0.0".to_string(),
        threshold_policy: threshold_policy(),
        calibration_runs: vec![
            StylizedFactCalibrationRunInput {
                calibration_run_id: "run_b".to_string(),
                empirical_dataset_id: "empirical".to_string(),
                simulated_dataset_id: "simulated".to_string(),
                empirical_metrics: vec![
                    metric("2h", 7_200, "metric_b", "Metric B", Some("lag_2"), 1.0),
                    metric("1h", 3_600, "metric_a", "Metric A", Some("lag_1"), 2.0),
                ],
                simulated_metrics: vec![
                    metric("2h", 7_200, "metric_b", "Metric B", Some("lag_2"), 1.1),
                    metric("1h", 3_600, "metric_a", "Metric A", Some("lag_1"), 2.1),
                ],
            },
            StylizedFactCalibrationRunInput {
                calibration_run_id: "run_a".to_string(),
                empirical_dataset_id: "empirical".to_string(),
                simulated_dataset_id: "simulated".to_string(),
                empirical_metrics: vec![metric("1h", 3_600, "metric_c", "Metric C", None, 1.0)],
                simulated_metrics: vec![metric("1h", 3_600, "metric_c", "Metric C", None, 1.0)],
            },
        ],
    };

    let first_report = run_stylized_fact_horizon_comparison_engine(request.clone())
        .expect("first deterministic report should succeed");
    let second_report = run_stylized_fact_horizon_comparison_engine(request)
        .expect("second deterministic report should succeed");
    assert_eq!(first_report, second_report);

    let first_payload =
        serde_json::to_string(&first_report).expect("first payload should serialize");
    let second_payload =
        serde_json::to_string(&second_report).expect("second payload should serialize");
    assert_eq!(first_payload, second_payload);

    let run_ids: Vec<&str> = first_report
        .run_reports
        .iter()
        .map(|run_report| run_report.calibration_run_id.as_str())
        .collect();
    assert_eq!(run_ids, vec!["run_a", "run_b"]);

    let run_b_report = first_report
        .run_reports
        .iter()
        .find(|run_report| run_report.calibration_run_id == "run_b")
        .expect("run_b report should exist");
    let horizon_labels: Vec<&str> = run_b_report
        .horizons
        .iter()
        .map(|horizon| horizon.horizon_label.as_str())
        .collect();
    assert_eq!(horizon_labels, vec!["1h", "2h"]);

    let metric_ids_1h: Vec<&str> = run_b_report.horizons[0]
        .metrics
        .iter()
        .map(|metric| metric.metric_id.as_str())
        .collect();
    assert_eq!(metric_ids_1h, vec!["metric_a"]);

    let serialized_report: Value =
        serde_json::to_value(&first_report).expect("report should serialize to JSON value");
    let report_object = serialized_report
        .as_object()
        .expect("batch report should serialize as object");
    for required_field in &first_report
        .output_schema_metadata
        .required_batch_report_fields
    {
        assert!(
            report_object.contains_key(*required_field),
            "batch report is missing required field `{required_field}`",
        );
    }

    let serialized_run_reports = serialized_report
        .get("run_reports")
        .and_then(Value::as_array)
        .expect("run_reports should be present as an array");
    for run_report in serialized_run_reports {
        let run_report_object = run_report
            .as_object()
            .expect("run report should serialize as object");
        for required_field in &first_report
            .output_schema_metadata
            .required_run_report_fields
        {
            assert!(
                run_report_object.contains_key(*required_field),
                "run report is missing required field `{required_field}`",
            );
        }
    }
}

#[test]
fn run_decision_states_cover_pass_fail_and_insufficient_data() {
    let request = StylizedFactHorizonComparisonRequest {
        metric_catalog_id: "catalog".to_string(),
        metric_catalog_version: "1.0.0".to_string(),
        threshold_policy: threshold_policy(),
        calibration_runs: vec![
            StylizedFactCalibrationRunInput {
                calibration_run_id: "run_pass".to_string(),
                empirical_dataset_id: "empirical".to_string(),
                simulated_dataset_id: "simulated".to_string(),
                empirical_metrics: vec![metric("1h", 3_600, "metric_x", "Metric X", None, 1.0)],
                simulated_metrics: vec![metric("1h", 3_600, "metric_x", "Metric X", None, 1.05)],
            },
            StylizedFactCalibrationRunInput {
                calibration_run_id: "run_fail".to_string(),
                empirical_dataset_id: "empirical".to_string(),
                simulated_dataset_id: "simulated".to_string(),
                empirical_metrics: vec![metric("1h", 3_600, "metric_x", "Metric X", None, 0.0)],
                simulated_metrics: vec![metric("1h", 3_600, "metric_x", "Metric X", None, 0.5)],
            },
            StylizedFactCalibrationRunInput {
                calibration_run_id: "run_insufficient".to_string(),
                empirical_dataset_id: "empirical".to_string(),
                simulated_dataset_id: "simulated".to_string(),
                empirical_metrics: vec![metric_with_observed_counts(
                    "1h",
                    3_600,
                    "metric_x",
                    "Metric X",
                    None,
                    1.0,
                    StylizedFactObservedCounts {
                        sample_count: 2,
                        effective_sample_count: 1,
                        tail_observation_count: 0,
                    },
                )],
                simulated_metrics: vec![metric("1h", 3_600, "metric_x", "Metric X", None, 1.0)],
            },
        ],
    };

    let report = run_stylized_fact_horizon_comparison_engine(request)
        .expect("state coverage fixture should run");

    let run_status_by_id: std::collections::BTreeMap<&str, StylizedFactComparisonDecisionState> =
        report
            .run_reports
            .iter()
            .map(|run_report| (run_report.calibration_run_id.as_str(), run_report.status))
            .collect();

    assert_eq!(
        run_status_by_id.get("run_pass"),
        Some(&StylizedFactComparisonDecisionState::Pass)
    );
    assert_eq!(
        run_status_by_id.get("run_fail"),
        Some(&StylizedFactComparisonDecisionState::Fail)
    );
    assert_eq!(
        run_status_by_id.get("run_insufficient"),
        Some(&StylizedFactComparisonDecisionState::InsufficientData)
    );
}

fn threshold_policy() -> StylizedFactHorizonComparisonThresholdPolicy {
    StylizedFactHorizonComparisonThresholdPolicy {
        policy_id: "stylized_fact_delta_policy.v1".to_string(),
        default_max_absolute_delta: Some(0.10),
        default_max_standardized_delta: Some(3.0),
        metric_threshold_overrides: vec![StylizedFactMetricThresholdOverride {
            metric_id: "metric_fail".to_string(),
            metric_variant: None,
            max_absolute_delta: Some(0.20),
            max_standardized_delta: Some(2.0),
        }],
    }
}

fn metric(
    horizon_label: &str,
    horizon_duration_nanos: i64,
    metric_id: &str,
    metric_name: &str,
    metric_variant: Option<&str>,
    value: f64,
) -> StylizedFactObservedMetricInput {
    metric_with_observed_counts(
        horizon_label,
        horizon_duration_nanos,
        metric_id,
        metric_name,
        metric_variant,
        value,
        StylizedFactObservedCounts {
            sample_count: 100,
            effective_sample_count: 80,
            tail_observation_count: 20,
        },
    )
}

fn metric_with_observed_counts(
    horizon_label: &str,
    horizon_duration_nanos: i64,
    metric_id: &str,
    metric_name: &str,
    metric_variant: Option<&str>,
    value: f64,
    observed_counts: StylizedFactObservedCounts,
) -> StylizedFactObservedMetricInput {
    StylizedFactObservedMetricInput {
        horizon_label: horizon_label.to_string(),
        horizon_duration_nanos,
        metric_id: metric_id.to_string(),
        metric_name: metric_name.to_string(),
        metric_variant: metric_variant.map(str::to_string),
        value: Some(value),
        confidence_interval: Some(StylizedFactMetricConfidenceInterval {
            lower_bound: value - 0.05,
            upper_bound: value + 0.05,
            confidence_level: 0.95,
        }),
        sample_coverage: StylizedFactMetricSampleCoverage {
            horizon_interval_count: 10,
            covered_interval_count: 9,
        },
        observed_counts,
        required_minimums: StylizedFactSampleSufficiencyRequirements {
            minimum_sample_count: 10,
            minimum_effective_sample_count: 8,
            minimum_tail_observation_count: 1,
            minimum_horizon_coverage_ratio: 0.8,
        },
    }
}
