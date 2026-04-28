use std::io::Write;
use std::process::Command;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use janiform_cli::CliExitCode;

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
        "stylized-facts",
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
    for arguments in [
        &[
            "price",
            "observed-options",
            "--model-path",
            "model.jf",
            "--option-surface-path",
            "options.parquet",
        ][..],
        &["presets", "list", "--kind", "session"][..],
        &["presets", "show", "market-profile", "es_futures_globex"][..],
        &[
            "presets",
            "export",
            "known-news",
            "us_macro_core",
            "--scope",
            "dependencies",
        ][..],
        &[
            "seasonality",
            "preprocess-timestamps",
            "--input-path",
            "ticks.parquet",
        ][..],
        &[
            "seasonality",
            "diagnostics",
            "--input-path",
            "ticks.parquet",
        ][..],
        &[
            "hourly-factors",
            "materialize-seasonality",
            "--input-path",
            "ticks.parquet",
        ][..],
        &[
            "hourly-factors",
            "materialize-news-intensity",
            "--input-path",
            "news.json",
        ][..],
        &[
            "hourly-factors",
            "build-contract",
            "--seasonality-factor-path",
            "seasonality.json",
        ][..],
        &[
            "stylized-facts",
            "horizon-comparison",
            "--empirical-path",
            "empirical.parquet",
        ][..],
        &[
            "dev",
            "regression",
            "deterministic",
            "--baseline-path",
            "baseline.json",
        ][..],
        &[
            "dev",
            "regression",
            "open-closed-gap-leakage",
            "--fixture-path",
            "fixture.json",
        ][..],
    ] {
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

fn write_tick_csv(path: &std::path::Path) {
    std::fs::write(
        path,
        "timestamp,price,side\n2026-04-20T14:30:00Z,10.0,buy\n2026-04-20T14:31:00Z,10.1,sell\n2026-04-20T14:32:00Z,10.2,buy\n",
    )
    .expect("tick CSV should be writable");
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
