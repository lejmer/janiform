use std::fs;
use std::path::PathBuf;

use janiform_calibration_artifacts::CalibrationExecutionMode;
use janiform_foundations::paths::format_path;
use janiform_validation::{
    StylizedFactCalibrationRunInput, StylizedFactComparisonDecisionState,
    StylizedFactHorizonComparisonArtifactExportConfig,
    StylizedFactHorizonComparisonReportGeneratorOptions,
    StylizedFactHorizonComparisonReportGeneratorRequest,
    StylizedFactHorizonComparisonThresholdPolicy, StylizedFactMetricClass,
    StylizedFactMetricConfidenceInterval, StylizedFactMetricSampleCoverage,
    StylizedFactMetricThresholdOverride, StylizedFactObservedCounts,
    StylizedFactObservedMetricInput, StylizedFactSampleSufficiencyReasonCode,
    StylizedFactSampleSufficiencyRequirements, build_stylized_fact_metric_catalog,
    run_stylized_fact_horizon_comparison_report_generator,
};
use serde_json::Value;

#[test]
fn table_summary_rows_include_metric_classes_and_insufficient_data_reasons() {
    let report_bundle = run_stylized_fact_horizon_comparison_report_generator(
        build_generator_request(CalibrationExecutionMode::Memory, None),
    )
    .expect("report generator should succeed in memory mode");

    assert_eq!(report_bundle.table_summary.rows.len(), 2);

    let primary_row = report_bundle
        .table_summary
        .rows
        .iter()
        .find(|row| row.metric_id == "returns_abs_quantile_shape_ratio_q99_q95")
        .expect("primary metric row should be present");

    assert_eq!(
        primary_row.metric_class,
        StylizedFactMetricClass::PrimaryRobust
    );
    assert_eq!(
        primary_row.status,
        StylizedFactComparisonDecisionState::Pass
    );
    assert!(
        primary_row.standardized_delta.is_some(),
        "primary row should preserve standardized delta when both confidence intervals exist",
    );

    let secondary_row = report_bundle
        .table_summary
        .rows
        .iter()
        .find(|row| row.metric_id == "returns_l_moment_lambda2")
        .expect("secondary metric row should be present");

    assert_eq!(
        secondary_row.metric_class,
        StylizedFactMetricClass::SecondaryMoment
    );
    assert_eq!(
        secondary_row.status,
        StylizedFactComparisonDecisionState::InsufficientData
    );
    assert!(
        secondary_row
            .insufficient_data_reasons
            .empirical
            .contains(&StylizedFactSampleSufficiencyReasonCode::MissingPointEstimate),
        "secondary row should preserve missing point estimate insufficiency reason",
    );
}

#[test]
fn in_memory_bundle_schema_metadata_lists_required_fields() {
    let report_bundle = run_stylized_fact_horizon_comparison_report_generator(
        build_generator_request(CalibrationExecutionMode::Memory, None),
    )
    .expect("report generator should succeed in memory mode");

    let serialized_bundle: Value =
        serde_json::to_value(&report_bundle).expect("bundle should serialize");

    let bundle_object = serialized_bundle
        .as_object()
        .expect("bundle should serialize as an object");

    for required_field in &report_bundle.output_schema_metadata.required_bundle_fields {
        assert!(
            bundle_object.contains_key(*required_field),
            "bundle is missing required field `{required_field}`",
        );
    }

    let table_summary_object = serialized_bundle
        .get("table_summary")
        .and_then(Value::as_object)
        .expect("table_summary should be present as an object");

    for required_field in &report_bundle
        .output_schema_metadata
        .required_table_summary_fields
    {
        assert!(
            table_summary_object.contains_key(*required_field),
            "table_summary is missing required field `{required_field}`",
        );
    }

    let first_table_row = table_summary_object
        .get("rows")
        .and_then(Value::as_array)
        .and_then(|rows| rows.first())
        .and_then(Value::as_object)
        .expect("table_summary.rows should contain at least one object row");

    for required_field in &report_bundle
        .output_schema_metadata
        .required_table_summary_row_fields
    {
        assert!(
            first_table_row.contains_key(*required_field),
            "table_summary row is missing required field `{required_field}`",
        );
    }
}

#[test]
fn disk_mode_export_writes_deterministic_file_names_and_payloads() {
    let export_directory = temporary_export_directory("stylized-fact-report-export");
    if export_directory.exists() {
        fs::remove_dir_all(export_directory.as_path())
            .expect("pre-existing export directory should be removable");
    }

    fs::create_dir_all(export_directory.as_path()).expect("export directory should be creatable");

    let export_config = StylizedFactHorizonComparisonArtifactExportConfig {
        export_directory: format_path(&export_directory),
        file_stem: "batch_run_fixture".to_string(),
    };

    let request = build_generator_request(CalibrationExecutionMode::Disk, Some(export_config));

    let first_bundle = run_stylized_fact_horizon_comparison_report_generator(request.clone())
        .expect("first disk-mode report generation should succeed");

    let second_bundle = run_stylized_fact_horizon_comparison_report_generator(request)
        .expect("second disk-mode report generation should succeed");

    let first_receipt = first_bundle
        .artifact_export
        .as_ref()
        .expect("disk mode should include artifact export receipt");
    let second_receipt = second_bundle
        .artifact_export
        .as_ref()
        .expect("disk mode should include artifact export receipt");

    assert_eq!(first_receipt, second_receipt);

    let exported_file_names: Vec<&str> = first_receipt
        .exported_files
        .iter()
        .map(|file| file.file_name.as_str())
        .collect();

    assert_eq!(
        exported_file_names,
        vec![
            "batch_run_fixture.report_bundle.json",
            "batch_run_fixture.table_summary.json",
        ]
    );

    let mut first_pass_payloads: Vec<String> = Vec::new();

    for exported_file in &first_receipt.exported_files {
        let payload = fs::read_to_string(exported_file.path.as_str())
            .expect("exported artifact file should be readable");

        first_pass_payloads.push(payload);
    }

    let mut second_pass_payloads: Vec<String> = Vec::new();

    for exported_file in &second_receipt.exported_files {
        let payload = fs::read_to_string(exported_file.path.as_str())
            .expect("exported artifact file should be readable on second pass");

        second_pass_payloads.push(payload);
    }

    assert_eq!(first_pass_payloads, second_pass_payloads);
}

fn build_generator_request(
    execution_mode: CalibrationExecutionMode,
    artifact_export: Option<StylizedFactHorizonComparisonArtifactExportConfig>,
) -> StylizedFactHorizonComparisonReportGeneratorRequest {
    StylizedFactHorizonComparisonReportGeneratorRequest {
        metric_catalog: build_stylized_fact_metric_catalog(),
        comparison_request: janiform_validation::StylizedFactHorizonComparisonRequest {
            metric_catalog_id: "stylized_fact_metric_catalog".to_string(),
            metric_catalog_version: "1.0.0".to_string(),
            threshold_policy: StylizedFactHorizonComparisonThresholdPolicy {
                policy_id: "stylized_fact_delta_policy.v1".to_string(),
                default_max_absolute_delta: Some(0.20),
                default_max_standardized_delta: Some(5.0),
                metric_threshold_overrides: vec![StylizedFactMetricThresholdOverride {
                    metric_id: "returns_l_moment_lambda2".to_string(),
                    metric_variant: None,
                    max_absolute_delta: Some(0.25),
                    max_standardized_delta: Some(4.0),
                }],
            },
            calibration_runs: vec![StylizedFactCalibrationRunInput {
                calibration_run_id: "run_001".to_string(),
                empirical_dataset_id: "empirical_set".to_string(),
                simulated_dataset_id: "simulated_set".to_string(),
                empirical_metrics: vec![
                    metric(
                        "1h",
                        "returns_abs_quantile_shape_ratio_q99_q95",
                        "Returns Abs Quantile Shape Ratio Q99 Q95",
                        Some(1.00),
                    ),
                    metric(
                        "1h",
                        "returns_l_moment_lambda2",
                        "Returns L Moment Lambda2",
                        None,
                    ),
                ],
                simulated_metrics: vec![
                    metric(
                        "1h",
                        "returns_abs_quantile_shape_ratio_q99_q95",
                        "Returns Abs Quantile Shape Ratio Q99 Q95",
                        Some(1.10),
                    ),
                    metric(
                        "1h",
                        "returns_l_moment_lambda2",
                        "Returns L Moment Lambda2",
                        Some(0.40),
                    ),
                ],
            }],
        },
        generation_options: StylizedFactHorizonComparisonReportGeneratorOptions {
            execution_mode,
            artifact_export,
        },
    }
}

fn metric(
    horizon_label: &str,
    metric_id: &str,
    metric_name: &str,
    value: Option<f64>,
) -> StylizedFactObservedMetricInput {
    StylizedFactObservedMetricInput {
        horizon_label: horizon_label.to_string(),
        horizon_duration_nanos: 3_600,
        metric_id: metric_id.to_string(),
        metric_name: metric_name.to_string(),
        metric_variant: None,
        value,
        confidence_interval: value.map(|point_estimate| StylizedFactMetricConfidenceInterval {
            lower_bound: point_estimate - 0.05,
            upper_bound: point_estimate + 0.05,
            confidence_level: 0.95,
        }),
        sample_coverage: StylizedFactMetricSampleCoverage {
            horizon_interval_count: 12,
            covered_interval_count: 10,
        },
        observed_counts: StylizedFactObservedCounts {
            sample_count: 100,
            effective_sample_count: 80,
            tail_observation_count: 20,
        },
        required_minimums: StylizedFactSampleSufficiencyRequirements {
            minimum_sample_count: 10,
            minimum_effective_sample_count: 8,
            minimum_tail_observation_count: 1,
            minimum_horizon_coverage_ratio: 0.75,
        },
    }
}

fn temporary_export_directory(test_name: &str) -> PathBuf {
    let directory_name = format!(
        "{}-{}-{}",
        test_name,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0),
    );

    std::env::temp_dir().join(directory_name)
}
