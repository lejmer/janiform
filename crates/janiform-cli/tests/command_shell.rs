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
        &["fit", "ticks.parquet"][..],
        &["fit", "inspect", "ticks.parquet"][..],
        &["fit", "plan", "--tick-data-path", "ticks.parquet"][..],
        &["fit", "staged", "--execution-input-path", "stage.json"][..],
        &["fit", "staged", "plan", "--tick-data-path", "ticks.parquet"][..],
        &[
            "fit",
            "staged",
            "run",
            "--execution-input-path",
            "stage.json",
        ][..],
        &["inspect", "run", ".janiform/runs/example"][..],
        &["inspect", "model", "model.jf", "--parameters"][..],
        &["report", "fit", ".janiform/runs/example"][..],
        &[
            "price",
            "fitted-model",
            "--model-path",
            "model.jf",
            "--strike",
            "5000",
        ][..],
        &[
            "price",
            "observed-options",
            "--model-path",
            "model.jf",
            "--option-surface-path",
            "options.parquet",
        ][..],
        &[
            "simulate",
            "seeded-ticks",
            "--model-path",
            "model.jf",
            "--seed",
            "123",
        ][..],
        &[
            "simulate",
            "coarse-interval",
            "--model-path",
            "model.jf",
            "--interval",
            "1h",
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
fn json_mode_writes_one_final_json_error_to_stdout() {
    let output = janiform_command()
        .args(["--json", "fit", "ticks.parquet"])
        .output()
        .expect("janiform command should execute");

    assert_eq!(
        output.status.code(),
        Some(CliExitCode::UnimplementedCommand as i32)
    );
    assert!(
        output.stderr.is_empty(),
        "json mode should not write human diagnostics to stderr: `{}`",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("json output should be UTF-8");
    let payload: serde_json::Value =
        serde_json::from_str(&stdout).expect("stdout should contain one JSON object");
    assert_eq!(payload["status"], "error");
    assert_eq!(payload["code"], "unimplemented_command");
    assert_eq!(
        payload["exit_code"],
        CliExitCode::UnimplementedCommand as u8
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
            "fit",
            "ticks.parquet",
        ])
        .output()
        .expect("janiform command should execute");

    assert_eq!(
        output.status.code(),
        Some(CliExitCode::UnimplementedCommand as i32)
    );
    assert!(
        output.stderr.is_empty(),
        "configured json mode should not write human diagnostics to stderr: `{}`",
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
