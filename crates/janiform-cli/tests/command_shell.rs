use std::io::Write;
use std::process::Command;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use janiform::verification::{
    StylizedFactCalibrationRunInput, StylizedFactHorizonComparisonRequest,
    StylizedFactHorizonComparisonThresholdPolicy, StylizedFactMetricConfidenceInterval,
    StylizedFactMetricSampleCoverage, StylizedFactMetricThresholdOverride,
    StylizedFactObservedCounts, StylizedFactObservedMetricInput,
    StylizedFactSampleSufficiencyRequirements, build_stylized_fact_metric_catalog,
};
use janiform_cli::CliExitCode;

const HOUR_DURATION_NANOS: i64 = 3_600_000_000_000;

fn janiform_command() -> Command {
    Command::new(env!("CARGO_BIN_EXE_janiform"))
}

#[test]
fn root_help_shows_primary_command_tree() {
    let output = janiform_command()
        .arg("--help")
        .output()
        .expect("janiform --help should execute");

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("help output should be UTF-8");
    for expected_command in [
        "fit",
        "inspect",
        "report",
        "price",
        "simulate",
        "presets",
        "seasonality",
        "hourly-factors",
        "validate",
        "dev",
    ] {
        assert!(
            stdout.contains(expected_command),
            "missing command `{expected_command}` in help:\n{stdout}"
        );
    }

    for expected_global_option in [
        "--color",
        "--silent",
        "--quiet",
        "--verbose",
        "--json",
        "--events",
        "--trace-events",
        "--config",
    ] {
        assert!(
            stdout.contains(expected_global_option),
            "missing global option `{expected_global_option}` in help:\n{stdout}"
        );
    }
}

#[test]
fn recognized_commands_parse_and_stop_at_unimplemented_boundary() {
    for arguments in [&[
        "price",
        "observed-options",
        "--model-path",
        "model.jf",
        "--option-surface-path",
        "options.parquet",
    ][..]]
    {
        let output = janiform_command()
            .args(arguments)
            .output()
            .expect("janiform command should execute");

        assert_eq!(
            output.status.code(),
            Some(CliExitCode::UnimplementedCommand as i32),
            "unexpected exit for arguments {arguments:?}: stdout=`{}` stderr=`{}`",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn dev_deterministic_regression_json_runs_native_harness() {
    let calendar_fixture =
        source_fixture_path("tests/fixtures/calendar/calendar_service_regression_matrix.json");
    let hourly_fixture =
        source_fixture_path("tests/fixtures/hourly_factors/hourly_factor_regression_matrix.json");

    let output = janiform_command()
        .args([
            "--json",
            "dev",
            "regression",
            "deterministic",
            "--calendar-registry-json",
            calendar_fixture
                .to_str()
                .expect("calendar fixture path should be UTF-8"),
            "--calendar-cases-json",
            calendar_fixture
                .to_str()
                .expect("calendar fixture path should be UTF-8"),
            "--seasonality-cases-json",
            hourly_fixture
                .to_str()
                .expect("hourly fixture path should be UTF-8"),
        ])
        .output()
        .expect("deterministic regression command should execute");

    assert_eq!(output.status.code(), Some(CliExitCode::Success as i32));
    assert!(
        output.stderr.is_empty(),
        "json mode should not write terminal diagnostics to stderr: `{}`",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("json output should be UTF-8");
    let payload: serde_json::Value =
        serde_json::from_str(&stdout).expect("stdout should contain one JSON object");

    assert_eq!(
        payload["schema_version"],
        "validation.deterministic_regression_harness.report.v1"
    );
    assert_eq!(payload["summary"]["failed_case_count"], 0);
}

#[test]
fn dev_open_closed_gap_leakage_regression_uses_default_fixtures() {
    let output = janiform_command()
        .args(["--json", "dev", "regression", "open-closed-gap-leakage"])
        .output()
        .expect("open/closed gap regression command should execute");

    assert_eq!(output.status.code(), Some(CliExitCode::Success as i32));
    assert!(
        output.stderr.is_empty(),
        "json mode should not write terminal diagnostics to stderr: `{}`",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("json output should be UTF-8");
    let payload: serde_json::Value =
        serde_json::from_str(&stdout).expect("stdout should contain one JSON object");

    assert_eq!(
        payload["schema_version"],
        "validation.open_closed_gap_leakage_regression_harness.report.v1"
    );
    assert_eq!(payload["summary"]["failed_case_count"], 0);
}

#[test]
fn dev_open_closed_gap_leakage_regression_requires_explicit_fixtures_outside_source_tree() {
    let workspace = temporary_run_directory("dev-regression-outside-source");
    std::fs::create_dir_all(&workspace).expect("temporary workspace should be created");

    let output = janiform_command()
        .current_dir(&workspace)
        .args(["--json", "dev", "regression", "open-closed-gap-leakage"])
        .output()
        .expect("open/closed gap regression command should execute");

    assert_eq!(
        output.status.code(),
        Some(CliExitCode::CommandLineUsage as i32)
    );
    assert!(
        output.stderr.is_empty(),
        "json mode should not write terminal diagnostics to stderr: `{}`",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("json output should be UTF-8");
    let payload: serde_json::Value =
        serde_json::from_str(&stdout).expect("stdout should contain one JSON object");

    assert_eq!(payload["code"], "invalid_command_argument");
    assert!(
        payload["message"]
            .as_str()
            .unwrap_or_default()
            .contains("pass `--calendar-fixture-path` explicitly")
    );

    std::fs::remove_dir_all(workspace).expect("temporary workspace should be removable");
}

#[test]
fn dev_open_closed_gap_leakage_regression_accepts_explicit_fixtures_outside_source_tree() {
    let workspace = temporary_run_directory("dev-regression-explicit-fixtures");
    std::fs::create_dir_all(&workspace).expect("temporary workspace should be created");
    let calendar_fixture =
        source_fixture_path("tests/fixtures/calendar/calendar_service_regression_matrix.json");
    let gap_behavior_fixture =
        source_fixture_path("tests/fixtures/gap_behavior/open_closed_gap_behavior_scenarios.json");

    let output = janiform_command()
        .current_dir(&workspace)
        .args([
            "--json",
            "dev",
            "regression",
            "open-closed-gap-leakage",
            "--calendar-fixture-path",
            calendar_fixture
                .to_str()
                .expect("calendar fixture path should be UTF-8"),
            "--gap-behavior-fixture-path",
            gap_behavior_fixture
                .to_str()
                .expect("gap behavior fixture path should be UTF-8"),
        ])
        .output()
        .expect("open/closed gap regression command should execute");

    assert_eq!(output.status.code(), Some(CliExitCode::Success as i32));

    let stdout = String::from_utf8(output.stdout).expect("json output should be UTF-8");
    let payload: serde_json::Value =
        serde_json::from_str(&stdout).expect("stdout should contain one JSON object");

    assert_eq!(
        payload["schema_version"],
        "validation.open_closed_gap_leakage_regression_harness.report.v1"
    );
    assert_eq!(payload["summary"]["failed_case_count"], 0);

    std::fs::remove_dir_all(workspace).expect("temporary workspace should be removable");
}

#[test]
fn seasonality_preprocess_timestamps_csv_runs_native_workflow() {
    let workspace = temporary_run_directory("seasonality-preprocess");
    std::fs::create_dir_all(&workspace).expect("temporary workspace should be created");
    let calendar_path = workspace.join("calendar.json");
    let events_path = workspace.join("events.csv");
    write_calendar_fixture(&calendar_path);
    std::fs::write(
        &events_path,
        concat!(
            "timestamp,price\n",
            "2026-02-02T14:35:00Z,1.0\n",
            "2026-02-02T15:10:00Z,1.1\n",
            "2026-02-02T16:15:00Z,1.2\n",
            "2026-02-02T22:00:00Z,1.3\n"
        ),
    )
    .expect("event CSV fixture should be written");

    let output = janiform_command()
        .args([
            "--json",
            "seasonality",
            "preprocess-timestamps",
            "--event-records",
            events_path
                .to_str()
                .expect("temporary event path should be valid UTF-8"),
            "--calendar-registry-json",
            calendar_path
                .to_str()
                .expect("temporary calendar path should be valid UTF-8"),
            "--exchange-code",
            "XNYS",
        ])
        .output()
        .expect("seasonality preprocess command should execute");

    assert!(output.status.success());
    assert!(
        output.stderr.is_empty(),
        "json mode should not write terminal diagnostics to stderr: `{}`",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("json output should be UTF-8");
    let payload: serde_json::Value =
        serde_json::from_str(&stdout).expect("stdout should contain one JSON object");

    assert_eq!(
        payload["schema_version"],
        "seasonality.timestamp_event.preprocessed.v1"
    );
    assert_eq!(payload["exchange_code"], "XNYS");
    assert_eq!(payload["accepted_record_count"], 3);
    assert_eq!(payload["filtered_record_count"], 1);
    assert_eq!(
        payload["source_record_positions"],
        serde_json::json!([0, 1, 2])
    );

    std::fs::remove_dir_all(workspace).expect("temporary workspace should be removable");
}

#[test]
fn seasonality_diagnostics_jsonl_runs_full_native_pipeline() {
    let workspace = temporary_run_directory("seasonality-diagnostics");
    std::fs::create_dir_all(&workspace).expect("temporary workspace should be created");
    let calendar_path = workspace.join("calendar.json");
    let events_path = workspace.join("events.jsonl");
    write_calendar_fixture(&calendar_path);
    std::fs::write(
        &events_path,
        concat!(
            "{\"timestamp\":\"2026-02-02T14:35:00Z\"}\n",
            "{\"timestamp\":\"2026-02-02T15:10:00Z\"}\n",
            "{\"timestamp\":\"2026-02-02T16:15:00Z\"}\n"
        ),
    )
    .expect("event JSONL fixture should be written");

    let output = janiform_command()
        .args([
            "--json",
            "seasonality",
            "diagnostics",
            "--event-records",
            events_path
                .to_str()
                .expect("temporary event path should be valid UTF-8"),
            "--calendar-registry-json",
            calendar_path
                .to_str()
                .expect("temporary calendar path should be valid UTF-8"),
            "--dataset-id",
            "xnys_smoke",
            "--exchange-code",
            "XNYS",
        ])
        .output()
        .expect("seasonality diagnostics command should execute");

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("json output should be UTF-8");
    let payload: serde_json::Value =
        serde_json::from_str(&stdout).expect("stdout should contain one JSON object");

    assert_eq!(
        payload["schema_version"],
        "seasonality.diagnostics.report.v1"
    );
    assert_eq!(payload["dataset_id"], "xnys_smoke");
    assert_eq!(
        payload["reproducibility_metadata"]["input_snapshot"]["input_event_count"],
        3
    );

    std::fs::remove_dir_all(workspace).expect("temporary workspace should be removable");
}

#[test]
fn hourly_factor_materialize_seasonality_json_runs_native_workflow() {
    let workspace = temporary_run_directory("hourly-seasonality");
    std::fs::create_dir_all(&workspace).expect("temporary workspace should be created");
    let calendar_path = workspace.join("calendar.json");
    let input_path = workspace.join("seasonality_input.json");
    write_calendar_fixture(&calendar_path);
    write_json_file(
        &input_path,
        &serde_json::json!({
            "source_timestamps_utc": [
                "2026-02-02T14:05:00Z",
                "2026-02-02T14:20:00Z",
                "2026-02-02T15:10:00Z",
                "2026-02-02T17:15:00Z"
            ],
            "source_seasonality_factors": [1.2, 1.0, 0.8, 1.4],
            "interpolation_method": "previous_observation_carry_forward.v1",
            "aggregation_method": "mean_of_observations.v1",
            "sparse_fallback_policy": "previous_materialized_then_global_mean.v1",
            "sparse_hour_minimum_observation_count": 2
        }),
    );

    let output = janiform_command()
        .args([
            "--json",
            "hourly-factors",
            "materialize-seasonality",
            "--calendar-registry-json",
            calendar_path
                .to_str()
                .expect("temporary calendar path should be valid UTF-8"),
            "--exchange-code",
            "XNYS",
            "--range-start-utc",
            "2026-02-02T13:00:00Z",
            "--range-end-utc",
            "2026-02-02T18:00:00Z",
            "--materialization-input-json",
            input_path
                .to_str()
                .expect("temporary input path should be valid UTF-8"),
        ])
        .output()
        .expect("hourly seasonality command should execute");

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("json output should be UTF-8");
    let payload: serde_json::Value =
        serde_json::from_str(&stdout).expect("stdout should contain one JSON object");

    assert_eq!(
        payload["schema_version"],
        "seasonality.hourly_s_h.materialization.v1"
    );
    assert_eq!(payload["s_h"], serde_json::json!([1.1, 1.1, 0.8, 0.8, 1.4]));
    assert_eq!(payload["interval_count"], 5);
    assert_eq!(payload["fallback_interval_count"], 1);

    std::fs::remove_dir_all(workspace).expect("temporary workspace should be removable");
}

#[test]
fn hourly_factor_materialize_news_intensity_json_runs_native_workflow() {
    let workspace = temporary_run_directory("hourly-news");
    std::fs::create_dir_all(&workspace).expect("temporary workspace should be created");
    let calendar_path = workspace.join("calendar.json");
    let input_path = workspace.join("news_input.json");
    write_calendar_fixture(&calendar_path);
    write_json_file(
        &input_path,
        &serde_json::json!({
            "known_scheduled_event_timestamps_utc": [
                "2026-02-02T14:05:00Z",
                "2026-02-02T15:10:00Z"
            ],
            "surprise_baseline_intensity": 0.1,
            "surprise_deterministic_time_multiplier_by_interval_index": {
                "0": 1.0,
                "1": 2.0,
                "2": 1.0
            }
        }),
    );

    let output = janiform_command()
        .args([
            "--json",
            "hourly-factors",
            "materialize-news-intensity",
            "--calendar-registry-json",
            calendar_path
                .to_str()
                .expect("temporary calendar path should be valid UTF-8"),
            "--exchange-code",
            "XNYS",
            "--range-start-utc",
            "2026-02-02T14:00:00Z",
            "--range-end-utc",
            "2026-02-02T17:00:00Z",
            "--materialization-input-json",
            input_path
                .to_str()
                .expect("temporary input path should be valid UTF-8"),
        ])
        .output()
        .expect("hourly news command should execute");

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("json output should be UTF-8");
    let payload: serde_json::Value =
        serde_json::from_str(&stdout).expect("stdout should contain one JSON object");

    assert_eq!(
        payload["schema_version"],
        "seasonality.hourly_nu_h.materialization.v1"
    );
    assert_eq!(
        payload["known_component_nu_h"],
        serde_json::json!([1.0, 1.0, 0.0])
    );
    assert_eq!(payload["nu_h"], serde_json::json!([1.1, 1.2, 0.1]));
    assert_eq!(payload["known_scheduled_arrival_count"], 2);

    std::fs::remove_dir_all(workspace).expect("temporary workspace should be removable");
}

#[test]
fn hourly_factor_build_contract_json_supports_explicit_sources() {
    let workspace = temporary_run_directory("hourly-contract");
    std::fs::create_dir_all(&workspace).expect("temporary workspace should be created");
    let calendar_path = workspace.join("calendar.json");
    let seasonality_path = workspace.join("seasonality.json");
    let news_path = workspace.join("news.json");
    write_calendar_fixture(&calendar_path);
    write_json_file(
        &seasonality_path,
        &serde_json::json!({
            "hourly_factors": [1.0, 0.9, 1.1],
            "materialization_method": "explicit.seasonality.test.v1"
        }),
    );
    write_json_file(
        &news_path,
        &serde_json::json!({
            "hourly_factors": [1.1, 1.2, 0.1],
            "materialization_method": "explicit.news.test.v1"
        }),
    );

    let output = janiform_command()
        .args([
            "--json",
            "hourly-factors",
            "build-contract",
            "--calendar-registry-json",
            calendar_path
                .to_str()
                .expect("temporary calendar path should be valid UTF-8"),
            "--exchange-code",
            "XNYS",
            "--range-start-utc",
            "2026-02-02T14:00:00Z",
            "--range-end-utc",
            "2026-02-02T17:00:00Z",
            "--seasonality-explicit-source-json",
            seasonality_path
                .to_str()
                .expect("temporary seasonality path should be valid UTF-8"),
            "--news-intensity-explicit-source-json",
            news_path
                .to_str()
                .expect("temporary news path should be valid UTF-8"),
        ])
        .output()
        .expect("hourly contract command should execute");

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("json output should be UTF-8");
    let payload: serde_json::Value =
        serde_json::from_str(&stdout).expect("stdout should contain one JSON object");

    assert_eq!(
        payload["schema_version"],
        "seasonality.hourly_factor.contract.v1"
    );
    assert_eq!(payload["s_h"], serde_json::json!([1.0, 0.9, 1.1]));
    assert_eq!(payload["nu_h"], serde_json::json!([1.1, 1.2, 0.1]));
    assert_eq!(
        payload["seasonality_materialization_method"],
        "explicit.seasonality.test.v1"
    );
    assert_eq!(
        payload["news_intensity_materialization_method"],
        "explicit.news.test.v1"
    );

    std::fs::remove_dir_all(workspace).expect("temporary workspace should be removable");
}

#[test]
fn validate_stylized_facts_json_runs_native_report() {
    let workspace = temporary_run_directory("stylized-facts-json");
    std::fs::create_dir_all(&workspace).expect("temporary workspace should be created");
    let metric_catalog_path = workspace.join("metric_catalog.json");
    let comparison_request_path = workspace.join("comparison_request.json");
    write_stylized_fact_inputs(&metric_catalog_path, &comparison_request_path, 0.20);

    let output = janiform_command()
        .args([
            "--json",
            "validate",
            "stylized-facts",
            "--metric-catalog",
            metric_catalog_path
                .to_str()
                .expect("metric catalog path should be valid UTF-8"),
            "--comparison-request",
            comparison_request_path
                .to_str()
                .expect("comparison request path should be valid UTF-8"),
        ])
        .output()
        .expect("stylized facts command should execute");

    assert_eq!(output.status.code(), Some(CliExitCode::Success as i32));
    assert!(
        output.stderr.is_empty(),
        "json mode should not write progress to stderr: `{}`",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("json output should be UTF-8");
    let payload: serde_json::Value =
        serde_json::from_str(&stdout).expect("stdout should contain one JSON object");
    assert_eq!(
        payload["schema_version"],
        "validation.stylized_fact.interval_comparison.report_bundle.v1"
    );
    assert_eq!(
        payload["artifact_export"]["file_stem"],
        serde_json::json!("stylized_facts")
    );
    assert_eq!(
        payload["comparison_report"]["summary"]["total_run_count"],
        1
    );
    assert_eq!(
        payload["comparison_report"]["summary"]["metric_decision_counts"]["pass_count"],
        2
    );
    assert_eq!(
        payload["comparison_report"]["summary"]["metric_decision_counts"]["insufficient_data_count"],
        1
    );

    std::fs::remove_dir_all(workspace).expect("temporary workspace should be removable");
}

#[test]
fn validate_stylized_facts_human_reports_incomplete_coverage() {
    let workspace = temporary_run_directory("stylized-facts-coverage");
    std::fs::create_dir_all(&workspace).expect("temporary workspace should be created");
    let metric_catalog_path = workspace.join("metric_catalog.json");
    let comparison_request_path = workspace.join("comparison_request.json");
    write_stylized_fact_inputs(&metric_catalog_path, &comparison_request_path, 0.20);

    let output = janiform_command()
        .args([
            "--color",
            "never",
            "validate",
            "stylized-facts",
            "--metric-catalog",
            metric_catalog_path
                .to_str()
                .expect("metric catalog path should be valid UTF-8"),
            "--comparison-request",
            comparison_request_path
                .to_str()
                .expect("comparison request path should be valid UTF-8"),
        ])
        .output()
        .expect("stylized facts command should execute");

    assert_eq!(output.status.code(), Some(CliExitCode::Success as i32));

    let stdout = String::from_utf8(output.stdout).expect("human output should be UTF-8");
    for expected in ["Coverage notes", "Not enough data", "Passed checks"] {
        assert!(
            stdout.contains(expected),
            "missing `{expected}` in stylized facts output:\n{stdout}"
        );
    }

    std::fs::remove_dir_all(workspace).expect("temporary workspace should be removable");
}

#[test]
fn validate_stylized_facts_human_reports_failures_and_artifacts() {
    let workspace = temporary_run_directory("stylized-facts-human");
    std::fs::create_dir_all(&workspace).expect("temporary workspace should be created");
    let metric_catalog_path = workspace.join("metric_catalog.json");
    let comparison_request_path = workspace.join("comparison_request.json");
    write_stylized_fact_inputs(&metric_catalog_path, &comparison_request_path, 0.05);

    let output = janiform_command()
        .current_dir(&workspace)
        .args([
            "--color",
            "never",
            "validate",
            "stylized-facts",
            "--metric-catalog",
            metric_catalog_path
                .to_str()
                .expect("metric catalog path should be valid UTF-8"),
            "--comparison-request",
            comparison_request_path
                .to_str()
                .expect("comparison request path should be valid UTF-8"),
            "--output-dir",
            workspace
                .join("exported")
                .to_str()
                .expect("output directory path should be valid UTF-8"),
            "--fail-on-failed-checks",
        ])
        .output()
        .expect("stylized facts command should execute");

    assert_eq!(
        output.status.code(),
        Some(CliExitCode::ReportFailedChecks as i32)
    );

    let stdout = String::from_utf8(output.stdout).expect("human output should be UTF-8");
    for expected in [
        "Stylized-fact interval comparison",
        "Metric checks",
        "Not enough data",
        "Failures by interval",
        "Failures by metric class",
        "Failed checks",
        "Passed checks",
        "Absolute delta",
        "Standardized delta",
        "returns_central_variance",
        "Artifact output",
        "stylized_facts.report_bundle.json",
        "stylized_facts.table_summary.json",
        "stylized_facts.plot_payload.json",
    ] {
        assert!(
            stdout.contains(expected),
            "missing `{expected}` in stylized facts output:\n{stdout}"
        );
    }

    for file_name in [
        "stylized_facts.report_bundle.json",
        "stylized_facts.table_summary.json",
        "stylized_facts.plot_payload.json",
    ] {
        assert!(
            workspace.join("exported").join(file_name).is_file(),
            "expected exported artifact `{file_name}`"
        );
    }

    std::fs::remove_dir_all(workspace).expect("temporary workspace should be removable");
}

#[test]
fn validate_stylized_facts_plan_json_resolves_computation_contract() {
    let workspace = temporary_run_directory("stylized-facts-plan");
    std::fs::create_dir_all(&workspace).expect("temporary workspace should be created");
    let model_path = workspace.join("model.jf");
    let empirical_path = workspace.join("ticks.csv");
    std::fs::write(&model_path, "{}").expect("model fixture should be written");
    write_tick_csv(&empirical_path);

    let output = janiform_command()
        .args([
            "--json",
            "validate",
            "stylized-facts",
            "plan",
            "--model-path",
            model_path
                .to_str()
                .expect("model path should be valid UTF-8"),
            "--empirical-data-path",
            empirical_path
                .to_str()
                .expect("empirical path should be valid UTF-8"),
            "--intervals",
            "5m,1h",
            "--simulation-path-count",
            "2",
            "--seed",
            "123",
            "--cpu-count",
            "2",
            "--cache",
        ])
        .output()
        .expect("stylized facts plan command should execute");

    assert_eq!(output.status.code(), Some(CliExitCode::Success as i32));

    let stdout = String::from_utf8(output.stdout).expect("json output should be UTF-8");
    let payload: serde_json::Value =
        serde_json::from_str(&stdout).expect("stdout should contain one JSON object");

    assert_eq!(
        payload["schema_version"],
        "validation.stylized_facts.plan.v1"
    );
    assert_eq!(payload["execution_mode"], "raw_data");
    assert_eq!(payload["empirical_data_format"], "csv");
    assert_eq!(payload["empirical_timestamp_column"], "timestamp");
    assert_eq!(payload["empirical_price_column"], "price");
    assert_eq!(payload["cpu_count"], 2);
    assert_eq!(
        payload["generated_seeds"]
            .as_array()
            .expect("generated seeds should be an array")
            .len(),
        2
    );
    assert_eq!(payload["cache_level"], "derived");
    assert_eq!(payload["intervals"][0]["label"], "5m");

    std::fs::remove_dir_all(workspace).expect("temporary workspace should be removable");
}

#[test]
fn validate_stylized_facts_plan_uses_model_tick_source_when_empirical_path_is_omitted() {
    let workspace = temporary_run_directory("stylized-facts-plan-model-source");
    std::fs::create_dir_all(&workspace).expect("temporary workspace should be created");
    let model_path = workspace.join("model.jf");
    let empirical_path = workspace.join("ticks.csv");
    write_tick_csv(&empirical_path);
    write_model_with_tick_source(&model_path, &empirical_path);

    let output = janiform_command()
        .args([
            "--json",
            "validate",
            "stylized-facts",
            "plan",
            "--model-path",
            model_path
                .to_str()
                .expect("model path should be valid UTF-8"),
            "--intervals",
            "5m",
        ])
        .output()
        .expect("stylized facts plan command should execute");

    assert_eq!(output.status.code(), Some(CliExitCode::Success as i32));

    let stdout = String::from_utf8(output.stdout).expect("json output should be UTF-8");
    let payload: serde_json::Value =
        serde_json::from_str(&stdout).expect("stdout should contain one JSON object");

    assert_eq!(
        payload["empirical_data_path"],
        empirical_path
            .to_str()
            .expect("empirical path should be valid UTF-8")
    );
    assert_eq!(payload["empirical_data_format"], "csv");
    assert_eq!(payload["empirical_timestamp_column"], "timestamp");
    assert_eq!(payload["empirical_price_column"], "price");

    std::fs::remove_dir_all(workspace).expect("temporary workspace should be removable");
}

#[test]
fn presets_list_json_filters_by_category_and_scope() {
    let output = janiform_command()
        .args([
            "--json",
            "presets",
            "list",
            "--category",
            "session",
            "--scope",
            "built_in",
        ])
        .output()
        .expect("janiform presets list should execute");

    assert!(output.status.success());
    assert!(
        output.stderr.is_empty(),
        "json mode should not write terminal diagnostics to stderr: `{}`",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("json output should be UTF-8");
    let payload: serde_json::Value =
        serde_json::from_str(&stdout).expect("stdout should contain one JSON object");

    assert_eq!(payload["schema_version"], "cli.presets.list.result.v1");
    assert_eq!(payload["command_id"], "presets.list");
    assert_eq!(payload["category_filter"], "session");
    assert_eq!(payload["scope_filter"], "built_in");
    assert_eq!(payload["preset_count"], 5);

    for entry in payload["presets"]
        .as_array()
        .expect("presets should be an array")
    {
        assert_eq!(entry["category"], "session");
        assert_eq!(entry["registry_scope"], "built_in");
    }
}

#[test]
fn presets_list_human_output_aligns_table_columns() {
    let output = janiform_command()
        .args(["presets", "list"])
        .output()
        .expect("janiform presets list should execute");

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("human output should be UTF-8");
    assert!(stdout.contains(
        "Category        Scope     Name                 Version\nmarket-profile  built-in  btcusd_spot          2026.03"
    ));
}

#[test]
fn presets_show_json_returns_artifact_payload() {
    let output = janiform_command()
        .args(["--json", "presets", "show", "market-profile", "eurusd_spot"])
        .output()
        .expect("janiform presets show should execute");

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("json output should be UTF-8");
    let payload: serde_json::Value =
        serde_json::from_str(&stdout).expect("stdout should contain one JSON object");

    assert_eq!(
        payload["schema_version"],
        "preset.artifact.market_profile.v1"
    );
    assert_eq!(payload["category"], "market_profile");
    assert_eq!(payload["preset_name"], "eurusd_spot");
    assert_eq!(
        payload["configuration"]["session_reference"]["preset_name"],
        "fx_spot_24_5"
    );
}

#[test]
fn presets_show_market_profile_human_renders_named_references() {
    let output = janiform_command()
        .args(["presets", "show", "market-profile", "btcusd_spot"])
        .output()
        .expect("janiform presets show should execute");

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("human output should be UTF-8");
    assert!(stdout.contains("Session       : crypto_24_7 (built-in)"));
    assert!(stdout.contains("Known news    : none (built-in)"));
}

#[test]
fn presets_show_known_news_expanded_renders_event_table() {
    let output = janiform_command()
        .args([
            "presets",
            "show",
            "known-news",
            "us_macro_core",
            "--expanded",
        ])
        .output()
        .expect("janiform presets show should execute");

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("human output should be UTF-8");
    assert!(stdout.contains(
        "Events\n\nScheduled At               Event ID            Name                             Tags"
    ));
    assert!(stdout.contains(
        "2026-01-09T13:30:00+00:00  employment_2026_01  US Employment Situation Release  employment, macro, nfp, us"
    ));
}

#[test]
fn presets_show_session_expanded_renders_window_tables() {
    let output = janiform_command()
        .args(["presets", "show", "session", "fx_spot_24_5", "--expanded"])
        .output()
        .expect("janiform presets show should execute");

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("human output should be UTF-8");
    assert!(stdout.contains(
        "Regular Session Windows\n\nWeekday    Open   Close  Label\nMonday     00:00  24:00  spot"
    ));
    assert!(stdout.contains(
        "Recurring Closed Windows\n\nStart Weekday  Start  End Weekday  End    Reason\nFriday         19:00  Sunday       21:00  weekend_close"
    ));
}

#[test]
fn presets_show_market_profile_rejects_expanded_flag() {
    let output = janiform_command()
        .args([
            "presets",
            "show",
            "market-profile",
            "eurusd_spot",
            "--expanded",
        ])
        .output()
        .expect("janiform presets show should execute");

    assert_eq!(
        output.status.code(),
        Some(CliExitCode::CommandLineUsage as i32)
    );
}

#[test]
fn presets_show_auto_scope_prefers_user_registered_presets() {
    let user_preset_path = temporary_run_directory("user-session-preset").with_extension("json");
    let bundled_session_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../python/janiform/calibration/_bundled_presets/generated/sessions/XNYS.json");
    let mut user_session: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(bundled_session_path).expect("bundled session should be readable"),
    )
    .expect("bundled session should be JSON");
    user_session["metadata"]["display_name"] = serde_json::Value::String("Desk XNYS".to_string());
    user_session["metadata"]["provenance"]["content_hash"] = serde_json::Value::Null;
    std::fs::write(
        &user_preset_path,
        serde_json::to_string_pretty(&user_session).expect("user session should serialize"),
    )
    .expect("user preset should be writable");

    let output = janiform_command()
        .args([
            "--json",
            "presets",
            "show",
            "session",
            "XNYS",
            "--user-preset",
            user_preset_path
                .to_str()
                .expect("temporary user preset path should be valid UTF-8"),
        ])
        .output()
        .expect("janiform presets show should execute");

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("json output should be UTF-8");
    let payload: serde_json::Value =
        serde_json::from_str(&stdout).expect("stdout should contain one JSON object");
    assert_eq!(payload["metadata"]["display_name"], "Desk XNYS");

    std::fs::remove_file(user_preset_path).expect("temporary user preset should be removable");
}

#[test]
fn presets_show_loads_user_presets_from_registry_manifest() {
    let registry_directory = temporary_run_directory("preset-registry");
    let preset_directory = registry_directory.join("presets");
    std::fs::create_dir_all(&preset_directory).expect("temporary preset directory should exist");

    let user_preset_path = preset_directory.join("XNYS.json");
    let bundled_session_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../python/janiform/calibration/_bundled_presets/generated/sessions/XNYS.json");
    let mut user_session: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(bundled_session_path).expect("bundled session should be readable"),
    )
    .expect("bundled session should be JSON");
    user_session["metadata"]["display_name"] =
        serde_json::Value::String("Manifest XNYS".to_string());
    user_session["metadata"]["provenance"]["content_hash"] = serde_json::Value::Null;
    std::fs::write(
        &user_preset_path,
        serde_json::to_string_pretty(&user_session).expect("user session should serialize"),
    )
    .expect("user preset should be writable");

    let registry_path = registry_directory.join("registry.json");
    std::fs::write(
        &registry_path,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": "preset.user_registry.v1",
            "preset_artifact_paths": ["presets/XNYS.json"],
        }))
        .expect("registry manifest should serialize"),
    )
    .expect("registry manifest should be writable");

    let output = janiform_command()
        .args([
            "--json",
            "presets",
            "show",
            "session",
            "XNYS",
            "--preset-registry",
            registry_path
                .to_str()
                .expect("temporary registry path should be valid UTF-8"),
        ])
        .output()
        .expect("janiform presets show should execute");

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("json output should be UTF-8");
    let payload: serde_json::Value =
        serde_json::from_str(&stdout).expect("stdout should contain one JSON object");
    assert_eq!(payload["metadata"]["display_name"], "Manifest XNYS");

    std::fs::remove_dir_all(registry_directory)
        .expect("temporary registry directory should be removable");
}

#[test]
fn presets_export_json_includes_market_profile_dependencies() {
    let output = janiform_command()
        .args([
            "--json",
            "presets",
            "export",
            "market-profile",
            "eurusd_spot",
        ])
        .output()
        .expect("janiform presets export should execute");

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("json output should be UTF-8");
    let payload: serde_json::Value =
        serde_json::from_str(&stdout).expect("stdout should contain one JSON object");

    assert_eq!(payload["schema_version"], "preset.runtime.export.v1");
    assert_eq!(payload["command_id"], "presets.export");
    assert_eq!(payload["requested_category"], "market_profile");
    assert_eq!(payload["requested_scope"], "built_in");
    assert_eq!(payload["preset_name"], "eurusd_spot");
    assert_eq!(
        payload["market_profile_preset"]["preset_name"],
        "eurusd_spot"
    );
    assert_eq!(payload["session_preset"]["preset_name"], "fx_spot_24_5");
    assert_eq!(
        payload["known_news_preset"]["preset_name"],
        "eu_us_macro_fx_core"
    );
}

#[test]
fn price_fitted_model_validates_model_path_at_rust_boundary() {
    let output = janiform_command()
        .args([
            "--json",
            "price",
            "fitted-model",
            "--model-path",
            "missing-model.json",
            "--valuation-time-utc",
            "2026-01-01T00:00:00Z",
            "--spot",
            "101",
            "--strike",
            "100",
            "--tenor",
            "1d",
        ])
        .output()
        .expect("janiform price fitted-model should execute");

    assert_eq!(
        output.status.code(),
        Some(CliExitCode::CommandLineUsage as i32)
    );
    assert!(
        output.stderr.is_empty(),
        "json mode should not write terminal diagnostics to stderr: `{}`",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("json output should be UTF-8");
    let payload: serde_json::Value =
        serde_json::from_str(&stdout).expect("stdout should contain one JSON object");
    assert_eq!(payload["code"], "missing_input_file");
}

#[test]
fn simulate_help_exposes_high_level_interval_option() {
    let output = janiform_command()
        .args(["simulate", "--help"])
        .output()
        .expect("janiform simulate --help should execute");

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("help output should be UTF-8");
    assert!(
        stdout.contains("--interval"),
        "simulate help should include high-level interval control:\n{stdout}"
    );
    assert!(
        stdout.contains("--coarse"),
        "simulate help should include the explicit coarse mode switch:\n{stdout}"
    );
    assert!(
        stdout.contains("seeded tick"),
        "simulate help should explain the default seeded tick path:\n{stdout}"
    );
}

#[test]
fn simulate_validates_interval_before_loading_model() {
    let output = janiform_command()
        .args([
            "--json",
            "simulate",
            "--model-path",
            "missing-model.jf",
            "--seed",
            "1",
            "--start-time-utc",
            "2026-01-01T00:00:00Z",
            "--horizon-interval-count",
            "1",
            "--interval",
            "fortnight",
        ])
        .output()
        .expect("janiform simulate should execute");

    assert_eq!(
        output.status.code(),
        Some(CliExitCode::CommandLineUsage as i32)
    );
    assert!(
        output.stderr.is_empty(),
        "json mode should not write terminal diagnostics to stderr: `{}`",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("json output should be UTF-8");
    let payload: serde_json::Value =
        serde_json::from_str(&stdout).expect("stdout should contain one JSON object");
    assert_eq!(payload["code"], "invalid_command_argument");
    assert!(
        payload["message"]
            .as_str()
            .unwrap_or_default()
            .contains("--interval")
    );
}

#[test]
fn simulate_defaults_seed_start_time_and_output_path() {
    let output = janiform_command()
        .args([
            "--json",
            "simulate",
            "--model-path",
            "missing-model.jf",
            "--horizon-interval-count",
            "1",
        ])
        .output()
        .expect("janiform simulate should execute");

    assert_eq!(
        output.status.code(),
        Some(CliExitCode::CommandLineUsage as i32)
    );
    assert!(
        output.stderr.is_empty(),
        "json mode should not write terminal diagnostics to stderr: `{}`",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("json output should be UTF-8");
    let payload: serde_json::Value =
        serde_json::from_str(&stdout).expect("stdout should contain one JSON object");
    assert_eq!(payload["code"], "missing_input_file");
}

#[test]
fn fit_json_runs_seasonality_and_writes_artifacts() {
    let workspace = temporary_run_directory("fit-run");
    std::fs::create_dir_all(&workspace).expect("temporary workspace should be created");
    let tick_data_path = workspace.join("EURUSD_ticks.csv");
    write_fx_tick_csv(&tick_data_path);

    let output = janiform_command()
        .current_dir(&workspace)
        .args([
            "--json",
            "fit",
            "EURUSD_ticks.csv",
            "--workflow-scope",
            "seasonality-only",
        ])
        .output()
        .expect("janiform command should execute");

    assert_eq!(output.status.code(), Some(CliExitCode::Success as i32));

    let stdout = String::from_utf8(output.stdout).expect("json output should be UTF-8");
    let payload: serde_json::Value =
        serde_json::from_str(&stdout).expect("stdout should contain one JSON object");
    assert_eq!(payload["command_id"], "fit");
    assert_eq!(payload["status"], "success");
    assert!(
        payload["run_id"]
            .as_str()
            .expect("run id should be a JSON string")
            .starts_with("eurusd_ticks_")
    );
    assert_eq!(payload["checkpoint_count"], 1);

    let model_path = std::path::PathBuf::from(
        payload["model_path"]
            .as_str()
            .expect("model path should be a JSON string"),
    );
    let report_path = std::path::PathBuf::from(
        payload["report_path"]
            .as_str()
            .expect("report path should be a JSON string"),
    );
    let event_log_path = std::path::PathBuf::from(
        payload["event_log_path"]
            .as_str()
            .expect("event log path should be a JSON string"),
    );

    assert!(model_path.is_file());
    assert!(report_path.is_file());
    assert!(event_log_path.is_file());
    assert!(
        event_log_path
            .parent()
            .expect("event log should have a parent directory")
            .join("latest-state.json")
            .is_file()
    );
    let persisted_report_before_export =
        std::fs::read_to_string(&report_path).expect("persisted report should be readable");

    let model_inspect_output = janiform_command()
        .args([
            "--json",
            "inspect",
            "model",
            model_path
                .to_str()
                .expect("model path should be valid UTF-8"),
        ])
        .output()
        .expect("janiform inspect model should execute");

    assert_eq!(
        model_inspect_output.status.code(),
        Some(CliExitCode::Success as i32)
    );

    let inspected_model: serde_json::Value = serde_json::from_slice(&model_inspect_output.stdout)
        .expect("inspect model should return JSON");
    assert_eq!(inspected_model["run_id"], payload["run_id"]);
    assert!(inspected_model["parameters"]["seasonality"].is_object());

    let report_output = janiform_command()
        .args([
            "--json",
            "report",
            "fit",
            report_path
                .to_str()
                .expect("report path should be valid UTF-8"),
        ])
        .output()
        .expect("janiform report fit should execute");

    assert_eq!(
        report_output.status.code(),
        Some(CliExitCode::Success as i32)
    );

    let reported_fit: serde_json::Value =
        serde_json::from_slice(&report_output.stdout).expect("report fit should return JSON");
    assert_eq!(reported_fit["schema_version"], "janiform.fit_report.v1");
    assert_eq!(reported_fit["run_id"], payload["run_id"]);
    assert!(reported_fit["parameter_fits"]["seasonality"]["parameters"].is_object());

    let human_report_output = janiform_command()
        .args([
            "--color",
            "never",
            "report",
            "fit",
            report_path
                .to_str()
                .expect("report path should be valid UTF-8"),
        ])
        .output()
        .expect("janiform report fit should execute");

    assert_eq!(
        human_report_output.status.code(),
        Some(CliExitCode::Success as i32)
    );

    let human_report =
        String::from_utf8(human_report_output.stdout).expect("human output should be UTF-8");
    for expected in [
        "Fit report",
        "Fit summary",
        "Workflow scope",
        "Selected stages",
        "Calibration outcomes",
        "Artifacts",
        "Paths:",
        "  report:",
    ] {
        assert!(
            human_report.contains(expected),
            "missing `{expected}` in report output:\n{human_report}"
        );
    }
    assert_eq!(
        human_report.matches("  model:").count(),
        1,
        "report fit should list the model artifact once:\n{human_report}"
    );

    let html_report_path = report_path.with_extension("html");
    let export_output = janiform_command()
        .args([
            "--json",
            "report",
            "fit",
            "--html",
            report_path
                .to_str()
                .expect("report path should be valid UTF-8"),
        ])
        .output()
        .expect("janiform report fit export should execute");

    assert_eq!(
        export_output.status.code(),
        Some(CliExitCode::Success as i32)
    );
    assert!(html_report_path.is_file());
    let html_report =
        std::fs::read_to_string(&html_report_path).expect("HTML report should be readable");
    for expected in [
        "Fit report",
        "Overview",
        "Schema version",
        "Staged response",
        "Summary",
        "Parameter fits",
        "Fit seasonality",
        "field-table",
        "data-table",
    ] {
        assert!(
            html_report.contains(expected),
            "missing `{expected}` in HTML report:\n{html_report}"
        );
    }
    assert_eq!(
        std::fs::read_to_string(&report_path).expect("persisted report should still be readable"),
        persisted_report_before_export,
        "report fit presentation exports should not rewrite report.json"
    );

    std::fs::remove_dir_all(workspace).expect("temporary workspace should be removable");
}

#[test]
fn fit_plan_json_prepares_seasonality_without_running_calibration() {
    let workspace = temporary_run_directory("fit-plan");
    std::fs::create_dir_all(&workspace).expect("temporary workspace should be created");
    let tick_data_path = workspace.join("EURUSD_ticks.csv");
    write_fx_tick_csv(&tick_data_path);

    let output = janiform_command()
        .args([
            "--json",
            "fit",
            "plan",
            "--tick-data-path",
            tick_data_path
                .to_str()
                .expect("tick path should be valid UTF-8"),
            "--workflow-scope",
            "seasonality-only",
            "--run-id",
            "plan-test",
        ])
        .output()
        .expect("janiform command should execute");

    assert_eq!(output.status.code(), Some(CliExitCode::Success as i32));
    assert!(
        output.stderr.is_empty(),
        "json fit plan should not write progress to stderr: `{}`",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("json output should be UTF-8");
    let payload: serde_json::Value =
        serde_json::from_str(&stdout).expect("stdout should contain one JSON object");
    assert_eq!(payload["command_id"], "fit.plan");
    assert_eq!(payload["run_id"], "plan-test");
    assert_eq!(payload["workflow_scope"], "seasonality_only");
    assert_eq!(payload["market_profile"]["profile_name"], "eurusd_spot");
    assert_eq!(payload["session_resolution"]["exchange_code"], "FXSPOT");
    assert_eq!(
        payload["diagnostics"]["selected_stages"],
        serde_json::json!(["seasonality"])
    );
    assert_eq!(payload["diagnostics"]["tick_record_count"], 5);
    assert_eq!(payload["staged_artifacts_written"], false);

    std::fs::remove_dir_all(workspace).expect("temporary workspace should be removable");
}

#[test]
fn fit_inspect_human_separates_user_inputs_and_inference() {
    let workspace = temporary_run_directory("fit-inspect");
    std::fs::create_dir_all(&workspace).expect("temporary workspace should be created");
    let tick_data_path = workspace.join("ticks.csv");
    write_tick_csv(&tick_data_path);

    let output = janiform_command()
        .args([
            "--color",
            "never",
            "fit",
            "inspect",
            "--tick-data-path",
            tick_data_path
                .to_str()
                .expect("tick path should be valid UTF-8"),
            "--exchange-code",
            "XNYS",
            "--workflow-scope",
            "seasonality-only",
            "--run-id",
            "inspect-test",
        ])
        .output()
        .expect("janiform command should execute");

    assert_eq!(output.status.code(), Some(CliExitCode::Success as i32));

    let stdout = String::from_utf8(output.stdout).expect("human output should be UTF-8");
    for expected in [
        "Janiform fit inspect",
        "Explicit user inputs",
        "Inferred values",
        "Preset defaults",
        "Recommended defaults",
        "Derived values",
        "Session",
    ] {
        assert!(
            stdout.contains(expected),
            "missing `{expected}` in inspect output:\n{stdout}"
        );
    }

    std::fs::remove_dir_all(workspace).expect("temporary workspace should be removable");
}

#[test]
fn fit_staged_plan_writes_prepared_execution_input() {
    let workspace = temporary_run_directory("fit-staged-plan");
    std::fs::create_dir_all(&workspace).expect("temporary workspace should be created");
    let tick_data_path = workspace.join("ticks.csv");
    let staged_output_path = workspace.join("staged-input.json");
    write_tick_csv(&tick_data_path);

    let output = janiform_command()
        .args([
            "--json",
            "fit",
            "staged",
            "plan",
            "--tick-data-path",
            tick_data_path
                .to_str()
                .expect("tick path should be valid UTF-8"),
            "--exchange-code",
            "XNYS",
            "--workflow-scope",
            "seasonality-only",
            "--run-id",
            "staged-plan-test",
            "--execution-input-output-path",
            staged_output_path
                .to_str()
                .expect("staged output path should be valid UTF-8"),
        ])
        .output()
        .expect("janiform command should execute");

    assert_eq!(output.status.code(), Some(CliExitCode::Success as i32));
    assert!(staged_output_path.is_file());

    let stdout = String::from_utf8(output.stdout).expect("json output should be UTF-8");
    let payload: serde_json::Value =
        serde_json::from_str(&stdout).expect("stdout should contain one JSON object");
    assert_eq!(payload["command_id"], "fit.staged.plan");
    assert_eq!(payload["staged_artifacts_written"], true);
    assert_eq!(
        payload["staged_execution_input"]["request"]["selected_stages"],
        serde_json::json!(["seasonality"])
    );

    let staged_payload: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(&staged_output_path)
            .expect("staged execution input should be readable"),
    )
    .expect("staged execution input should be JSON");
    assert_eq!(staged_payload["request"]["run_id"], "staged-plan-test");

    std::fs::remove_dir_all(workspace).expect("temporary workspace should be removable");
}

#[test]
fn fit_staged_run_executes_prepared_execution_input() {
    let workspace = temporary_run_directory("fit-staged-run");
    let output_directory = workspace.join("out");
    std::fs::create_dir_all(&workspace).expect("temporary workspace should be created");
    let tick_data_path = workspace.join("ticks.csv");
    let staged_output_path = workspace.join("staged-input.json");
    write_tick_csv(&tick_data_path);

    let plan_output = janiform_command()
        .args([
            "--json",
            "fit",
            "staged",
            "plan",
            "--tick-data-path",
            tick_data_path
                .to_str()
                .expect("tick path should be valid UTF-8"),
            "--exchange-code",
            "XNYS",
            "--workflow-scope",
            "seasonality-only",
            "--run-id",
            "staged-run-test",
            "--output-dir",
            output_directory
                .to_str()
                .expect("output directory should be valid UTF-8"),
            "--execution-input-output-path",
            staged_output_path
                .to_str()
                .expect("staged output path should be valid UTF-8"),
        ])
        .output()
        .expect("janiform command should execute");

    assert_eq!(plan_output.status.code(), Some(CliExitCode::Success as i32));

    let run_output = janiform_command()
        .args([
            "--json",
            "fit",
            "staged",
            "run",
            "--execution-input-path",
            staged_output_path
                .to_str()
                .expect("staged output path should be valid UTF-8"),
        ])
        .output()
        .expect("janiform command should execute");

    assert_eq!(run_output.status.code(), Some(CliExitCode::Success as i32));

    let stdout = String::from_utf8(run_output.stdout).expect("json output should be UTF-8");
    let payload: serde_json::Value =
        serde_json::from_str(&stdout).expect("stdout should contain one JSON object");
    assert_eq!(payload["run_id"], "staged-run-test");
    assert_eq!(payload["status"], "success");
    assert!(
        output_directory
            .join("artifacts")
            .join("model.jf")
            .is_file()
    );

    std::fs::remove_dir_all(workspace).expect("temporary workspace should be removable");
}

#[test]
fn inspect_run_reads_latest_state_snapshot_as_json() {
    let run_directory = temporary_run_directory("inspect-run");
    let event_paths = janiform_event_log::default_run_event_paths(&run_directory);
    let workflow = janiform_event_log::WorkflowRef::labeled("fit", "Fit");
    let mut writer = janiform_event_log::RunEventLogWriter::create(
        &event_paths.event_log_path,
        Some(&event_paths.latest_state_snapshot_path),
        "run-001",
        workflow.clone(),
    )
    .expect("event writer should be created");
    writer
        .append_event(&janiform_event_log::RunEvent::at(
            "2026-04-26T10:00:01Z",
            "run-001",
            workflow.clone(),
            janiform_event_log::EventDeliveryPriority::Required,
            janiform_event_log::RunEventPayload::Artifact(janiform_event_log::RunArtifactEvent {
                artifact_kind: "model".to_string(),
                path: run_directory.join("model.json").display().to_string(),
                stage: None,
                phase: None,
                label: None,
            }),
        ))
        .expect("artifact event should write");
    writer
        .append_event(&janiform_event_log::RunEvent::at(
            "2026-04-26T10:00:02Z",
            "run-001",
            workflow,
            janiform_event_log::EventDeliveryPriority::Required,
            janiform_event_log::RunEventPayload::Lifecycle(
                janiform_event_log::RunLifecycleEvent::RunCompleted {
                    total_runtime_seconds: 1.0,
                },
            ),
        ))
        .expect("event should write");
    writer.flush().expect("event log should flush");

    let output = janiform_command()
        .args([
            "--json",
            "inspect",
            "run",
            run_directory
                .to_str()
                .expect("temporary run path should be valid UTF-8"),
        ])
        .output()
        .expect("janiform command should execute");

    assert_eq!(output.status.code(), Some(CliExitCode::Success as i32));

    let stdout = String::from_utf8(output.stdout).expect("json output should be UTF-8");
    let payload: serde_json::Value =
        serde_json::from_str(&stdout).expect("stdout should contain one JSON object");
    assert_eq!(payload["run_id"], "run-001");
    assert_eq!(payload["status"], "completed");
    assert_eq!(payload["event_count"], 2);

    let human_output = janiform_command()
        .args([
            "--color",
            "never",
            "inspect",
            "run",
            run_directory
                .to_str()
                .expect("temporary run path should be valid UTF-8"),
        ])
        .output()
        .expect("janiform command should execute");

    assert_eq!(
        human_output.status.code(),
        Some(CliExitCode::Success as i32)
    );

    let human_stdout =
        String::from_utf8(human_output.stdout).expect("human output should be UTF-8");
    assert!(human_stdout.contains("Run"));
    assert!(human_stdout.contains("Artifacts"));
    assert!(human_stdout.contains("Paths:"));
    assert!(human_stdout.contains("  model: model.json"));
    assert!(human_stdout.contains("  events: events.jsonl"));
    assert!(!human_stdout.contains("Event log"));
    assert!(!human_stdout.contains("State source"));

    std::fs::remove_dir_all(run_directory).expect("temporary run directory should be removable");
}

#[test]
fn inspect_run_replay_ignores_trailing_partial_event_row() {
    let run_directory = temporary_run_directory("inspect-run-partial-event");
    std::fs::create_dir_all(&run_directory).expect("temporary run directory should be created");
    let event_paths = janiform_event_log::default_run_event_paths(&run_directory);
    let workflow = janiform_event_log::WorkflowRef::labeled("fit", "Fit");
    let event = janiform_event_log::RunEvent::at(
        "2026-04-26T10:00:00Z",
        "run-001",
        workflow,
        janiform_event_log::EventDeliveryPriority::Required,
        janiform_event_log::RunEventPayload::Lifecycle(
            janiform_event_log::RunLifecycleEvent::RunStarted {
                scope_id: "seasonality_only".to_string(),
                scope_label: Some("seasonality only".to_string()),
                inputs: vec!["ticks.csv".to_string()],
                output_directory: Some(run_directory.display().to_string()),
                stage_sequence: Vec::new(),
            },
        ),
    );

    let mut event_log =
        std::fs::File::create(&event_paths.event_log_path).expect("event log should be created");
    janiform_event_log::append_event_to_log(&mut event_log, &event_paths.event_log_path, &event)
        .expect("complete event should write");
    write!(event_log, "{{\"schema_version\":\"janiform.run_event.v1\"")
        .expect("partial event should write");
    event_log.flush().expect("event log should flush");

    let output = janiform_command()
        .args([
            "--json",
            "inspect",
            "run",
            run_directory
                .to_str()
                .expect("temporary run path should be valid UTF-8"),
        ])
        .output()
        .expect("janiform command should execute");

    assert_eq!(output.status.code(), Some(CliExitCode::Success as i32));

    let stdout = String::from_utf8(output.stdout).expect("json output should be UTF-8");
    let payload: serde_json::Value =
        serde_json::from_str(&stdout).expect("stdout should contain one JSON object");
    assert_eq!(payload["run_id"], "run-001");
    assert_eq!(payload["event_count"], 1);
    assert_eq!(payload["inspection"]["state_source"], "event_log_replay");
    assert_eq!(payload["inspection"]["replay_fallback_attempted"], true);

    std::fs::remove_dir_all(run_directory).expect("temporary run directory should be removable");
}

#[test]
fn json_mode_writes_one_final_json_error_to_stdout() {
    let output = janiform_command()
        .args(["--json", "report", "fit", "missing-report.json"])
        .output()
        .expect("janiform command should execute");

    assert_eq!(
        output.status.code(),
        Some(CliExitCode::CommandLineUsage as i32)
    );
    assert!(
        output.stderr.is_empty(),
        "json mode should not write terminal diagnostics to stderr: `{}`",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("json output should be UTF-8");
    let payload: serde_json::Value =
        serde_json::from_str(&stdout).expect("stdout should contain one JSON object");
    assert_eq!(payload["status"], "error");
    assert_eq!(payload["code"], "artifact_read");
    assert_eq!(payload["exit_code"], CliExitCode::CommandLineUsage as u8);
}

#[test]
fn terminal_diagnostics_follow_color_policy() {
    let forced_color = janiform_command()
        .args([
            "--color",
            "always",
            "price",
            "observed-options",
            "--model-path",
            "model.jf",
            "--option-surface-path",
            "options.parquet",
        ])
        .output()
        .expect("janiform command should execute");

    assert_eq!(
        forced_color.status.code(),
        Some(CliExitCode::UnimplementedCommand as i32)
    );
    assert!(
        String::from_utf8_lossy(&forced_color.stderr).contains("\u{1b}["),
        "forced color should emit ANSI styling in terminal diagnostics"
    );

    let disabled_color = janiform_command()
        .args([
            "--color",
            "never",
            "price",
            "observed-options",
            "--model-path",
            "model.jf",
            "--option-surface-path",
            "options.parquet",
        ])
        .output()
        .expect("janiform command should execute");

    assert_eq!(
        disabled_color.status.code(),
        Some(CliExitCode::UnimplementedCommand as i32)
    );
    assert!(
        !String::from_utf8_lossy(&disabled_color.stderr).contains("\u{1b}["),
        "disabled color should not emit ANSI styling in terminal diagnostics"
    );
}

#[test]
fn global_output_modes_have_explicit_conflicts() {
    let output = janiform_command()
        .args(["--json", "--quiet", "fit", "ticks.parquet"])
        .output()
        .expect("janiform command should execute");

    assert_eq!(
        output.status.code(),
        Some(CliExitCode::CommandLineUsage as i32)
    );
}

#[test]
fn toml_configuration_supplies_cli_boundary_defaults() {
    let config_path = temporary_config_path();

    std::fs::write(&config_path, "[output]\njson = true\n")
        .expect("test should be able to write CLI config");

    let output = janiform_command()
        .args([
            "--config",
            config_path
                .to_str()
                .expect("temporary config path should be valid UTF-8"),
            "price",
            "observed-options",
            "--model-path",
            "model.jf",
            "--option-surface-path",
            "options.parquet",
        ])
        .output()
        .expect("janiform command should execute");

    assert_eq!(
        output.status.code(),
        Some(CliExitCode::UnimplementedCommand as i32)
    );
    assert!(
        output.stderr.is_empty(),
        "configured json mode should not write terminal diagnostics to stderr: `{}`",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("json output should be UTF-8");
    let payload: serde_json::Value =
        serde_json::from_str(&stdout).expect("stdout should contain one JSON object");
    assert_eq!(payload["code"], "unimplemented_command");

    std::fs::remove_file(config_path).expect("temporary CLI config should be removable");
}

fn temporary_config_path() -> std::path::PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after UNIX epoch")
        .as_nanos();

    std::env::temp_dir().join(format!(
        "janiform-cli-config-{}-{timestamp}.toml",
        std::process::id()
    ))
}

fn temporary_run_directory(label: &str) -> std::path::PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after UNIX epoch")
        .as_nanos();

    std::env::temp_dir().join(format!(
        "janiform-cli-{label}-{}-{timestamp}",
        std::process::id()
    ))
}

fn source_fixture_path(path: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(path)
}

fn write_tick_csv(path: &std::path::Path) {
    std::fs::write(
        path,
        "timestamp,price,side\n2026-04-20T14:30:00Z,10.0,buy\n2026-04-20T14:31:00Z,10.1,sell\n2026-04-20T14:32:00Z,10.2,buy\n",
    )
    .expect("tick CSV should be writable");
}

fn write_model_with_tick_source(model_path: &std::path::Path, tick_path: &std::path::Path) {
    write_json_file(
        model_path,
        &serde_json::json!({
            "metadata": {
                "input_inference": {
                    "tick_data_source": {
                        "path": tick_path.display().to_string(),
                        "file_format": "csv",
                        "provenance_label": "ticks"
                    },
                    "tick_data_column_mapping": {
                        "timestamp_column": "timestamp",
                        "price_column": "price",
                        "trade_sign_column": "side"
                    },
                    "normalization_policy": {
                        "timestamp_timezone": "UTC",
                        "tick_price_input_policy": "explicit_price_column"
                    }
                }
            }
        }),
    );
}

fn write_calendar_fixture(path: &std::path::Path) {
    std::fs::write(
        path,
        include_str!("../../../tests/fixtures/calendar/calendar_service_regression_matrix.json"),
    )
    .expect("calendar fixture should be writable");
}

fn write_json_file(path: &std::path::Path, payload: &serde_json::Value) {
    let contents =
        serde_json::to_string_pretty(payload).expect("test JSON payload should serialize");

    std::fs::write(path, contents).expect("test JSON payload should be writable");
}

fn write_stylized_fact_inputs(
    metric_catalog_path: &std::path::Path,
    comparison_request_path: &std::path::Path,
    default_max_absolute_delta: f64,
) {
    write_json_file(
        metric_catalog_path,
        &serde_json::to_value(build_stylized_fact_metric_catalog())
            .expect("metric catalog should serialize"),
    );
    write_json_file(
        comparison_request_path,
        &serde_json::to_value(build_stylized_fact_comparison_request(
            default_max_absolute_delta,
        ))
        .expect("comparison request should serialize"),
    );
}

fn build_stylized_fact_comparison_request(
    default_max_absolute_delta: f64,
) -> StylizedFactHorizonComparisonRequest {
    StylizedFactHorizonComparisonRequest {
        metric_catalog_id: "stylized_fact_metric_catalog".to_string(),
        metric_catalog_version: "1.0.0".to_string(),
        threshold_policy: StylizedFactHorizonComparisonThresholdPolicy {
            policy_id: "stylized_fact_delta_policy.v1".to_string(),
            default_max_absolute_delta: Some(default_max_absolute_delta),
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
                stylized_fact_observed_metric(
                    "1h",
                    "returns_abs_quantile_shape_ratio_q99_q95",
                    "Returns Abs Quantile Shape Ratio Q99 Q95",
                    Some(1.00),
                ),
                stylized_fact_observed_metric(
                    "1h",
                    "returns_l_moment_lambda2",
                    "Returns L Moment Lambda2",
                    None,
                ),
                stylized_fact_observed_metric(
                    "1h",
                    "returns_central_variance",
                    "Returns Central Variance",
                    Some(2.00),
                ),
            ],
            simulated_metrics: vec![
                stylized_fact_observed_metric(
                    "1h",
                    "returns_abs_quantile_shape_ratio_q99_q95",
                    "Returns Abs Quantile Shape Ratio Q99 Q95",
                    Some(1.10),
                ),
                stylized_fact_observed_metric(
                    "1h",
                    "returns_l_moment_lambda2",
                    "Returns L Moment Lambda2",
                    Some(0.40),
                ),
                stylized_fact_observed_metric(
                    "1h",
                    "returns_central_variance",
                    "Returns Central Variance",
                    Some(2.01),
                ),
            ],
        }],
    }
}

fn stylized_fact_observed_metric(
    horizon_label: &str,
    metric_id: &str,
    metric_name: &str,
    value: Option<f64>,
) -> StylizedFactObservedMetricInput {
    StylizedFactObservedMetricInput {
        horizon_label: horizon_label.to_string(),
        horizon_duration_nanos: HOUR_DURATION_NANOS,
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

fn write_fx_tick_csv(path: &std::path::Path) {
    std::fs::write(
        path,
        concat!(
            "timestamp,price,side\n",
            "2026-04-20T14:30:00Z,1.1000,buy\n",
            "2026-04-21T14:30:00Z,1.1005,sell\n",
            "2026-04-22T14:30:00Z,1.1010,buy\n",
            "2026-04-23T14:30:00Z,1.1015,sell\n",
            "2026-04-24T14:30:00Z,1.1020,buy\n",
        ),
    )
    .expect("FX tick CSV should be writable");
}
