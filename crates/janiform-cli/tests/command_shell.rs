use std::process::Command;

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
