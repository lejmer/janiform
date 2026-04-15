from pathlib import Path
from typing import Any, NoReturn

import orjson
import pytest

import janiform.calibration.fit.estimator as fit_estimator_module
import janiform.cli as cli_module
from janiform.calibration.market_data import CalibrationDuplicateRecordPolicy
from janiform.calibration.staged import StagedCalibrationResponse, validate_staged_calibration_response
from tests.support.cli import invoke_cli, normalize_help_output, render_help
from tests.support.high_level_workflows import (
    build_full_workflow_model,
    successful_full_model_staged_runner_payload,
)


def test_cli_top_level_help_lists_high_level_workflow_commands() -> None:
    help_output = _render_help([])

    assert "fit" in help_output
    assert "simulate" in help_output
    assert "presets" in help_output


def test_fit_help_documents_default_workflow_and_expert_subcommands() -> None:
    help_output = normalize_help_output(_render_help(["fit"]))

    assert "inspect Inspect the inferred fit configuration" in help_output
    assert "fit plan" in help_output
    assert "fit staged" in help_output
    assert "--market MARKET" in help_output
    assert "--explain" in help_output
    assert "--silent" in help_output
    assert "--session-preset SESSION_PRESET" in help_output
    assert "--session-json PATH" in help_output
    assert "--known-news KNOWN_NEWS" in help_output
    assert "--known-news-json PATH" in help_output
    assert "--mean-reversion-grid-interval-hours" in help_output
    assert "--dynamics-search-restart-count" in help_output
    assert "--output-dir DIRECTORY" in help_output


def test_presets_list_outputs_bundled_presets() -> None:
    exit_code, stdout, stderr = invoke_cli(
        [
            "presets",
            "list",
            "--output",
            "json",
        ]
    )

    assert exit_code == int(cli_module.CliExitCode.SUCCESS), stderr
    payload = orjson.loads(stdout)

    assert payload["schema_version"] == cli_module.CLI_PRESET_LIST_SCHEMA_VERSION
    assert payload["command"] == cli_module.CLI_PRESETS_LIST_ENTRYPOINT
    assert any(
        preset["category"] == "market_profile" and preset["preset_name"] == "us_equity_cash"
        for preset in payload["presets"]
    )


def test_presets_export_outputs_nested_market_profile_dependencies() -> None:
    exit_code, stdout, stderr = invoke_cli(
        [
            "presets",
            "export",
            "market-profile",
            "eurusd_spot",
            "--output",
            "json",
        ]
    )

    assert exit_code == int(cli_module.CliExitCode.SUCCESS), stderr
    payload = orjson.loads(stdout)

    assert payload["command"] == cli_module.CLI_PRESETS_EXPORT_ENTRYPOINT
    assert payload["requested_category"] == "market_profile"
    assert payload["market_profile_preset"]["preset_name"] == "eurusd_spot"
    assert payload["session_preset"]["preset_name"] == "fx_spot_24_5"
    assert payload["known_news_preset"]["preset_name"] == "eu_us_macro_fx_core"


def test_fit_outputs_resolved_configuration_and_provenance(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    tick_data_path = _write_tick_data(
        tmp_path / "ticks.csv",
        rows=(
            "2026-02-02T14:30:00Z,100.0,buy",
            "2026-02-02T15:00:00Z,101.0,sell",
            "2026-02-02T20:59:00Z,102.0,buy",
            "2026-02-03T14:30:00Z,99.0,sell",
            "2026-02-03T20:59:00Z,98.0,buy",
        ),
    )
    output_dir = tmp_path / "fit-artifacts"

    monkeypatch.setattr(
        fit_estimator_module,
        "run_prepared_staged_calibration_with_raw_payload",
        lambda *args, **kwargs: successful_full_model_staged_runner_payload("cli-fit-success"),
    )

    exit_code, stdout, stderr = invoke_cli(
        [
            "fit",
            "--run-id",
            "cli-fit-success",
            "--workflow-scope",
            "full_without_option_surface",
            "--tick-data-path",
            str(tick_data_path),
            "--tick-data-format",
            "csv",
            "--tick-timestamp-column",
            "timestamp_utc",
            "--tick-price-column",
            "trade_price",
            "--tick-trade-sign-column",
            "trade_sign",
            "--tick-price-input-policy",
            "explicit_price_column",
            "--timestamp-timezone",
            "UTC",
            "--market",
            "us_equity_cash",
            "--known-news",
            "none",
            "--output-dir",
            str(output_dir),
            "--output",
            "json",
        ]
    )

    assert exit_code == int(cli_module.CliExitCode.SUCCESS), stderr
    payload = orjson.loads(stdout)

    assert payload["schema_version"] == cli_module.CLI_FIT_RESULT_SCHEMA_VERSION
    assert payload["command"] == cli_module.CLI_FIT_ENTRYPOINT
    run_directory = output_dir / "cli-fit-success"

    assert payload["model_output_path"] == str(run_directory / "model.json")
    assert payload["report_path"] == str(run_directory / "fit_report.json")
    assert payload["resolved_configuration"]["market_profile"]["selected_preset"]["preset_name"] == "us_equity_cash"
    assert payload["resolved_configuration"]["session"]["selected_preset"]["preset_name"] == "XNYS"
    assert payload["resolved_configuration"]["known_news"]["selected_preset"]["preset_name"] == "none"
    assert payload["model_summary"]["preset_provenance"]["session"]["inferred_from_data"] is False
    assert payload["session_resolution"]["resolution_kind"] == "automatic_inference"
    assert payload["session_resolution"]["provenance_kind"] == "bundled_preset"
    assert (run_directory / "model.json").exists()
    assert (run_directory / "fit_report.json").exists()
    assert (run_directory / "run.json").exists()
    assert (run_directory / "model.json").read_text(encoding="utf-8").startswith("{\n  ")

    persisted_report = orjson.loads((run_directory / "fit_report.json").read_text(encoding="utf-8"))
    assert "execution_input" not in persisted_report
    assert "stage_report_payloads" not in persisted_report


def test_fit_cli_uses_endogenous_mean_reversion_calibration(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.chdir(tmp_path)

    tick_data_path = _write_tick_data(
        tmp_path / "ticks.csv",
        rows=(
            "2026-02-02T14:30:00Z,100.0,buy",
            "2026-02-02T20:59:00Z,101.0,sell",
            "2026-02-03T14:30:00Z,102.0,buy",
            "2026-02-03T20:59:00Z,103.0,sell",
            "2026-02-04T14:30:00Z,104.0,buy",
            "2026-02-04T20:59:00Z,105.0,sell",
            "2026-02-05T14:30:00Z,106.0,buy",
            "2026-02-05T20:59:00Z,107.0,sell",
        ),
    )

    captured_stage_input: dict[str, Any] = {}

    def _capture_execution_input(prepared_handle, **kwargs):
        captured_stage_input["mean_reversion_stage_input"] = _prepared_payload(prepared_handle)[
            "mean_reversion_stage_input"
        ]
        del kwargs
        return successful_full_model_staged_runner_payload(
            _prepared_runner_run_id(prepared_handle)
        )

    monkeypatch.setattr(
        fit_estimator_module,
        "run_prepared_staged_calibration_with_raw_payload",
        _capture_execution_input,
    )

    exit_code, stdout, stderr = invoke_cli(
        [
            "fit",
            "--run-id",
            "cli-fit-mean-reversion",
            "--workflow-scope",
            "full_without_option_surface",
            "--tick-data-path",
            str(tick_data_path),
            "--tick-data-format",
            "csv",
            "--tick-timestamp-column",
            "timestamp_utc",
            "--tick-price-column",
            "trade_price",
            "--tick-trade-sign-column",
            "trade_sign",
            "--tick-price-input-policy",
            "explicit_price_column",
            "--timestamp-timezone",
            "UTC",
            "--market",
            "us_equity_cash",
            "--known-news",
            "none",
            "--mean-reversion-grid-interval-hours",
            "36",
            "--output",
            "json",
        ]
    )

    assert exit_code == int(cli_module.CliExitCode.SUCCESS), stderr
    payload = orjson.loads(stdout)
    assert payload["run_id"] == "cli-fit-mean-reversion"

    mean_reversion_stage_input = captured_stage_input["mean_reversion_stage_input"]
    assert mean_reversion_stage_input is not None
    assert set(mean_reversion_stage_input) == {
        "calibration_grid_kind",
        "calibration_grid_interval_hours",
        "source_observation_count",
        "coverage_ratio",
        "observations",
    }
    assert mean_reversion_stage_input["calibration_grid_interval_hours"] == pytest.approx(36.0)


def test_fit_supports_beginner_positional_tick_path_and_inferred_defaults(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.chdir(tmp_path)

    tick_data_path = _write_tick_data(
        tmp_path / "eurusd_ticks.csv",
        rows=(
            "2026-02-02T14:30:00Z,1.0850,buy",
            "2026-02-02T15:00:00Z,1.0852,sell",
            "2026-02-02T20:59:00Z,1.0854,buy",
            "2026-02-03T14:30:00Z,1.0851,sell",
        ),
    )

    monkeypatch.setattr(
        fit_estimator_module,
        "run_prepared_staged_calibration_with_raw_payload",
        lambda prepared_handle, **kwargs: successful_full_model_staged_runner_payload(
            _prepared_runner_run_id(prepared_handle)
        ),
    )

    exit_code, stdout, stderr = invoke_cli(
        [
            "fit",
            str(tick_data_path),
            "--market",
            "us_equity_cash",
            "--output",
            "json",
        ]
    )

    assert exit_code == int(cli_module.CliExitCode.SUCCESS), stderr
    payload = orjson.loads(stdout)

    assert payload["run_id"].startswith("eurusd_ticks_")
    assert payload["model_summary"]["workflow_scope"] == "full_without_option_surface"
    assert payload["input_inference"]["tick_data_format"] == "csv"
    assert payload["input_inference"]["timestamp_column"] == "timestamp_utc"
    assert payload["input_inference"]["tick_price_input_policy"] == "explicit_price_column"
    assert payload["input_inference"]["tick_price_columns"]["price_column"] == "trade_price"
    assert payload["input_inference"]["tick_price_columns"]["trade_sign_column"] == "trade_sign"
    assert payload["resolved_configuration"]["session"]["selected_preset"]["preset_name"] == "XNYS"
    assert payload["resolved_configuration"]["known_news"]["selected_preset"]["preset_name"] == "none"


def test_fit_supports_eurusd_market_preset_with_full_day_fx_session_windows(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.chdir(tmp_path)

    tick_data_path = _write_tick_data(
        tmp_path / "eurusd_ticks.csv",
        rows=(
            "2023-03-21T19:00:00Z,1.0830,buy",
            "2023-03-21T19:05:00Z,1.0831,sell",
            "2023-03-22T00:30:00Z,1.0832,buy",
            "2023-03-22T00:35:00Z,1.0833,sell",
            "2023-03-24T18:30:00Z,1.0840,buy",
            "2023-03-24T18:35:00Z,1.0841,sell",
            "2023-03-26T21:30:00Z,1.0842,buy",
            "2023-03-26T21:35:00Z,1.0844,sell",
        ),
    )

    monkeypatch.setattr(
        fit_estimator_module,
        "run_prepared_staged_calibration_with_raw_payload",
        lambda prepared_handle, **kwargs: successful_full_model_staged_runner_payload(
            _prepared_runner_run_id(prepared_handle)
        ),
    )

    exit_code, stdout, stderr = invoke_cli(
        [
            "fit",
            str(tick_data_path),
            "--market",
            "eurusd_spot",
            "--output",
            "json",
        ]
    )

    assert exit_code == int(cli_module.CliExitCode.SUCCESS), stderr
    payload = orjson.loads(stdout)

    assert payload["resolved_configuration"]["market_profile"]["selected_preset"]["preset_name"] == "eurusd_spot"
    assert payload["resolved_configuration"]["session"]["selected_preset"]["preset_name"] == "fx_spot_24_5"
    assert payload["session_resolution"]["exchange_code"] == "FXSPOT"


def test_fit_explain_and_silent_flags_control_success_output(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.chdir(tmp_path)

    tick_data_path = _write_tick_data(
        tmp_path / "explained_ticks.csv",
        rows=(
            "2026-02-02T14:30:00Z,100.0,buy",
            "2026-02-02T15:00:00Z,101.0,sell",
        ),
    )

    monkeypatch.setattr(
        fit_estimator_module,
        "run_prepared_staged_calibration_with_raw_payload",
        lambda prepared_handle, **kwargs: successful_full_model_staged_runner_payload(
            _prepared_runner_run_id(prepared_handle)
        ),
    )

    exit_code, stdout, stderr = invoke_cli(
        [
            "fit",
            str(tick_data_path),
            "--market",
            "us_equity_cash",
            "--explain",
            "--output",
            "human",
        ]
    )

    assert exit_code == int(cli_module.CliExitCode.SUCCESS), stderr
    assert "inferred:\n" in stdout
    assert "  tick data format: csv" in stdout
    assert "  timestamp column: timestamp_utc" in stdout
    assert "  tick price input policy: explicit_price_column" in stdout
    assert "price columns:\n" in stdout
    assert "preset defaults:\n" in stdout

    silent_exit_code, silent_stdout, silent_stderr = invoke_cli(
        [
            "fit",
            str(tick_data_path),
            "--market",
            "us_equity_cash",
            "--silent",
            "--output",
            "json",
        ]
    )

    assert silent_exit_code == int(cli_module.CliExitCode.SUCCESS), silent_stderr
    assert silent_stdout == ""


def test_fit_inspect_previews_inference(
    tmp_path: Path,
) -> None:
    tick_data_path = _write_tick_data(
        tmp_path / "inspect_ticks.csv",
        rows=(
            "2026-02-02T14:30:00Z,100.0,buy",
            "2026-02-02T15:00:00Z,101.0,sell",
        ),
    )

    exit_code, stdout, stderr = invoke_cli(
        [
            "fit",
            "inspect",
            str(tick_data_path),
            "--market",
            "us_equity_cash",
            "--output",
            "json",
        ]
    )

    assert exit_code == int(cli_module.CliExitCode.SUCCESS), stderr
    payload = orjson.loads(stdout)

    assert payload["schema_version"] == cli_module.CLI_FIT_INSPECT_RESULT_SCHEMA_VERSION
    assert payload["command"] == cli_module.CLI_FIT_INSPECT_ENTRYPOINT
    assert payload["input_inference"]["tick_data_format"] == "csv"
    assert payload["input_inference"]["timestamp_column"] == "timestamp_utc"
    assert payload["input_inference"]["tick_price_input_policy"] == "explicit_price_column"
    assert payload["temporary_workspace_materialized"] is False


def test_simulate_outputs_model_provenance_and_simulation_payload(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.chdir(tmp_path)

    model = build_full_workflow_model(
        tmp_path,
        monkeypatch,
        run_id="cli-simulate-model",
    )
    model_path = tmp_path / "saved-model.json"
    model.save(model_path)

    exit_code, stdout, stderr = invoke_cli(
        [
            "simulate",
            "--model-path",
            str(model_path),
            "--seed",
            "17",
            "--start-time-utc",
            "2026-02-02T15:00:00Z",
            "--horizon-interval-count",
            "2",
            "--interval",
            "1h",
            "--output",
            "json",
        ]
    )

    assert exit_code == int(cli_module.CliExitCode.SUCCESS), stderr
    payload = orjson.loads(stdout)

    assert payload["schema_version"] == cli_module.CLI_SIMULATE_RESULT_SCHEMA_VERSION
    assert payload["command"] == cli_module.CLI_HIGH_LEVEL_SIMULATE_ENTRYPOINT
    assert payload["model_path"] == str(model_path)
    assert payload["resolved_configuration"]["market_profile"]["selected_preset"]["preset_name"] == "us_equity_cash"
    assert payload["simulation_kind"] == "coarse_interval"
    assert payload["primary_output_path"].endswith(".csv")
    assert payload["report_path"].endswith("simulation_report.json")


def test_removed_duplicate_roots_are_rejected() -> None:
    for arguments in (
        ["fit-and-simulate"],
        ["calibration", "market-data"],
        ["calibration", "staged"],
        ["simulation", "coarse-interval"],
        ["simulation", "seeded-ticks"],
    ):
        exit_code, _, stderr = invoke_cli(arguments)

        assert exit_code == int(cli_module.CliExitCode.VALIDATION_ERROR)
        assert "No such command" in stderr


def test_fit_fails_for_ambiguous_session_inference(tmp_path: Path) -> None:
    tick_data_path = _write_tick_data(
        tmp_path / "ambiguous.csv",
        rows=(
            "2026-02-02T08:00:00Z,1.0850,buy",
            "2026-02-02T12:00:00Z,1.0852,sell",
            "2026-02-03T08:00:00Z,1.0854,buy",
        ),
    )

    exit_code, _, stderr = invoke_cli(
        [
            "fit",
            "--run-id",
            "cli-fit-ambiguous",
            "--workflow-scope",
            "full_without_option_surface",
            "--tick-data-path",
            str(tick_data_path),
            "--tick-data-format",
            "csv",
            "--tick-timestamp-column",
            "timestamp_utc",
            "--tick-price-column",
            "trade_price",
            "--tick-trade-sign-column",
            "trade_sign",
            "--tick-price-input-policy",
            "explicit_price_column",
            "--timestamp-timezone",
            "UTC",
            "--exchange-code",
            "BROKER_SPOT",
            "--session-timezone",
            "UTC",
            "--output",
            "json",
        ]
    )

    assert exit_code == int(cli_module.CliExitCode.VALIDATION_ERROR)
    assert "automatic 24/5 session inference is ambiguous" in stderr


def test_fit_fails_for_invalid_market_preset(tmp_path: Path) -> None:
    tick_data_path = _write_tick_data(
        tmp_path / "invalid-preset.csv",
        rows=(
            "2026-02-02T14:30:00Z,100.0,buy",
            "2026-02-02T15:00:00Z,101.0,sell",
        ),
    )

    exit_code, _, stderr = invoke_cli(
        [
            "fit",
            "--run-id",
            "cli-fit-invalid-preset",
            "--workflow-scope",
            "full_without_option_surface",
            "--tick-data-path",
            str(tick_data_path),
            "--tick-data-format",
            "csv",
            "--tick-timestamp-column",
            "timestamp_utc",
            "--tick-price-column",
            "trade_price",
            "--tick-trade-sign-column",
            "trade_sign",
            "--tick-price-input-policy",
            "explicit_price_column",
            "--timestamp-timezone",
            "UTC",
            "--market",
            "missing_profile",
            "--output",
            "json",
        ]
    )

    assert exit_code == int(cli_module.CliExitCode.VALIDATION_ERROR)
    assert "missing_profile" in stderr


def test_fit_surfaces_staged_failure_details_in_cli_error(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    tick_data_path = _write_tick_data(
        tmp_path / "failure-debug.csv",
        rows=(
            "2026-02-02T14:30:00Z,100.0,buy",
            "2026-02-02T15:00:00Z,101.0,sell",
            "2026-02-02T20:59:00Z,102.0,buy",
        ),
    )

    monkeypatch.setattr(
        fit_estimator_module,
        "run_prepared_staged_calibration_with_raw_payload",
        lambda *args, **kwargs: _failed_return_components_staged_runner_payload("cli-fit-stage-failure"),
    )

    exit_code, _, stderr = invoke_cli(
        [
            "fit",
            "--run-id",
            "cli-fit-stage-failure",
            "--workflow-scope",
            "full_without_option_surface",
            "--tick-data-path",
            str(tick_data_path),
            "--tick-data-format",
            "csv",
            "--tick-timestamp-column",
            "timestamp_utc",
            "--tick-price-column",
            "trade_price",
            "--tick-trade-sign-column",
            "trade_sign",
            "--tick-price-input-policy",
            "explicit_price_column",
            "--timestamp-timezone",
            "UTC",
            "--market",
            "eurusd_spot",
            "--output",
            "json",
        ]
    )

    assert exit_code == int(cli_module.CliExitCode.RUNTIME_ERROR)
    assert '"failed_stage": "return_components"' in stderr
    assert '"failure_kind": "invalid_candidate_generation_input"' in stderr
    assert (
        '"failure_error_message": "open boundary for exchange `FXSPOT` at '
        '1774224000000000000 must transition from closed to open, found previous=Open '
        'and boundary=Open"'
    ) in stderr


def test_fit_prefers_failed_stage_message_over_generic_top_level_failure(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    tick_data_path = _write_tick_data(
        tmp_path / "failure-generic-top-level.csv",
        rows=(
            "2026-02-02T14:30:00Z,100.0,buy",
            "2026-02-02T15:00:00Z,101.0,sell",
            "2026-02-02T20:59:00Z,102.0,buy",
        ),
    )

    monkeypatch.setattr(
        fit_estimator_module,
        "run_prepared_staged_calibration_with_raw_payload",
        lambda *args, **kwargs: _failed_return_components_with_generic_top_level_staged_runner_payload(
            "cli-fit-generic-top-level"
        ),
    )

    exit_code, _, stderr = invoke_cli(
        [
            "fit",
            "--run-id",
            "cli-fit-generic-top-level",
            "--workflow-scope",
            "full_without_option_surface",
            "--tick-data-path",
            str(tick_data_path),
            "--tick-data-format",
            "csv",
            "--tick-timestamp-column",
            "timestamp_utc",
            "--tick-price-column",
            "trade_price",
            "--tick-trade-sign-column",
            "trade_sign",
            "--tick-price-input-policy",
            "explicit_price_column",
            "--timestamp-timezone",
            "UTC",
            "--market",
            "eurusd_spot",
            "--output",
            "json",
        ]
    )

    assert exit_code == int(cli_module.CliExitCode.RUNTIME_ERROR)
    assert '"failed_stage": "return_components"' in stderr
    assert '"failure_kind": "invalid_candidate_generation_input"' in stderr
    assert '"failure_kind": "stage_execution_failed"' not in stderr
    assert (
        '"failure_error_message": "open boundary for exchange `FXSPOT` at '
        '1774224000000000000 must transition from closed to open, found previous=Open '
        'and boundary=Open"'
    ) in stderr


def test_fit_renders_human_error_output_with_labeled_lines(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    tick_data_path = _write_tick_data(
        tmp_path / "failure-human-output.csv",
        rows=(
            "2026-02-02T14:30:00Z,100.0,buy",
            "2026-02-02T15:00:00Z,101.0,sell",
            "2026-02-02T20:59:00Z,102.0,buy",
        ),
    )

    monkeypatch.setattr(
        fit_estimator_module,
        "run_prepared_staged_calibration_with_raw_payload",
        lambda *args, **kwargs: _failed_return_components_with_generic_top_level_staged_runner_payload(
            "cli-fit-human-error"
        ),
    )

    exit_code, _, stderr = invoke_cli(
        [
            "fit",
            "--run-id",
            "cli-fit-human-error",
            "--workflow-scope",
            "full_without_option_surface",
            "--tick-data-path",
            str(tick_data_path),
            "--tick-data-format",
            "csv",
            "--tick-timestamp-column",
            "timestamp_utc",
            "--tick-price-column",
            "trade_price",
            "--tick-trade-sign-column",
            "trade_sign",
            "--tick-price-input-policy",
            "explicit_price_column",
            "--timestamp-timezone",
            "UTC",
            "--market",
            "eurusd_spot",
            "--output",
            "human",
        ]
    )

    assert exit_code == int(cli_module.CliExitCode.RUNTIME_ERROR)
    assert "fit failed:\n" in stderr
    assert "  reason: high-level fit failed before a JaniformModel could be materialized\n" in stderr
    assert "  failed stage: return_components\n" in stderr
    assert "  failure kind: invalid_candidate_generation_input\n" in stderr
    assert (
        "  stage error: open boundary for exchange `FXSPOT` at 1774224000000000000 "
        "must transition from closed to open, found previous=Open and boundary=Open\n"
    ) in stderr
    assert "context={" not in stderr


def test_fit_rejects_contradictory_known_news_flags() -> None:
    exit_code, _, _ = invoke_cli(
        [
            "fit",
            "--run-id",
            "contradiction",
            "--workflow-scope",
            "seasonality_only",
            "--tick-data-path",
            "ticks.csv",
            "--tick-data-format",
            "csv",
            "--tick-timestamp-column",
            "timestamp_utc",
            "--timestamp-timezone",
            "UTC",
            "--known-news",
            "none",
            "--known-news-json",
            "known-news.json",
        ]
    )

    assert exit_code == 2


def test_explicit_fit_workflows_remain_available_but_secondary() -> None:
    help_output = normalize_help_output(_render_help(["fit"]))

    assert "fit plan" in help_output
    assert "fit staged" in help_output


def _render_help(arguments: list[str]) -> str:
    return render_help(arguments)


def _write_tick_data(path: Path, *, rows: tuple[str, ...]) -> Path:
    path.write_text(
        "\n".join(
            [
                "timestamp_utc,trade_price,trade_sign",
                *rows,
            ]
        ),
        encoding="utf-8",
    )
    return path


def _prepared_runner_run_id(prepared_handle: Any) -> str:
    prepared_payload = _prepared_payload(prepared_handle)
    return str(prepared_payload["request"]["run_id"])


def _prepared_payload(prepared_handle: Any) -> dict[str, Any]:
    return dict(orjson.loads(prepared_handle.serialize_execution_input_json()))




def _failed_return_components_staged_runner_payload(run_id: str) -> tuple[StagedCalibrationResponse, dict[str, Any]]:
    failure = {
        "failure_kind": "invalid_candidate_generation_input",
        "stage": "return_components",
        "error_message": (
            "open boundary for exchange `FXSPOT` at 1774224000000000000 must transition from "
            "closed to open, found previous=Open and boundary=Open"
        ),
        "run_id": run_id,
    }
    raw_response = {
        "schema_version": "calibration.orchestration.staged_run.report.v1",
        "failure_triage_policy": "mode_aware_failure_kind_and_checkpoint_context.v1",
        "run_id": run_id,
        "execution_mode": "memory",
        "status": "failure",
        "failure": failure,
        "stage_reports": [
            {
                "stage_report_kind": "seasonality",
                "stage": "seasonality",
                "status": "success",
                "run_id": run_id,
                "execution_mode": "memory",
                "handoff_policy": "explicit_run_spec_handoffs_mode_aware_transport.v1",
                "normalized_stage_payload": {
                    "composition_schema_version": "seasonality.deterministic.composition.v1",
                    "exchange_code": "FXSPOT",
                    "timestamp_utc_nanos": [
                        1770042600000000000,
                        1770046200000000000,
                        1770129000000000000,
                    ],
                    "normalized_combined_factor_by_event": [0.95, 1.05, 1.0],
                    "intraday_factor_by_bin": [1.0] * 3,
                    "day_of_week_factor": [1.0] * 7,
                    "day_of_year_factor": [1.0] * 366,
                },
            },
            {
                "stage_report_kind": "hawkes",
                "stage": "hawkes",
                "status": "success",
                "run_id": run_id,
                "execution_mode": "memory",
                "handoff_policy": "explicit_run_spec_handoffs_mode_aware_transport.v1",
                "fitted_stage_payload": {
                    "schema_version": "calibration.stage_output.hawkes.v1",
                    "selected_candidate_id": "candidate_stable_low_loss",
                    "fitted_parameters": {
                        "kernel_parameters": {
                            "lambda_bar": 0.8,
                            "beta": 1.6,
                            "alpha": {
                                "plus_plus": 0.2,
                                "plus_minus": 0.1,
                                "minus_plus": 0.1,
                                "minus_minus": 0.25,
                            },
                            "rho": {
                                "plus_plus": 0.08,
                                "plus_minus": 0.04,
                                "minus_plus": 0.05,
                                "minus_minus": 0.1,
                            },
                        },
                    },
                },
            },
            {
                "stage_report_kind": "return_components",
                "stage": "return_components",
                "status": "failure",
                "run_id": run_id,
                "execution_mode": "memory",
                "handoff_policy": "explicit_run_spec_handoffs_mode_aware_transport.v1",
                "failure": failure,
            },
        ],
        "emitted_checkpoints": [],
    }

    return validate_staged_calibration_response(raw_response), raw_response


def _failed_return_components_with_generic_top_level_staged_runner_payload(
    run_id: str,
) -> tuple[StagedCalibrationResponse, dict[str, Any]]:
    raw_response = {
        "schema_version": "calibration.orchestration.staged_run.report.v1",
        "failure_triage_policy": "mode_aware_failure_kind_and_checkpoint_context.v1",
        "run_id": run_id,
        "execution_mode": "memory",
        "status": "failure",
        "failure": {
            "failure_kind": "stage_execution_failed",
            "stage": "return_components",
        },
        "stage_reports": [
            {
                "stage_report_kind": "seasonality",
                "stage": "seasonality",
                "status": "success",
                "run_id": run_id,
                "execution_mode": "memory",
                "handoff_policy": "explicit_run_spec_handoffs_mode_aware_transport.v1",
                "normalized_stage_payload": {
                    "composition_schema_version": "seasonality.deterministic.composition.v1",
                    "exchange_code": "FXSPOT",
                    "timestamp_utc_nanos": [
                        1770042600000000000,
                        1770046200000000000,
                        1770129000000000000,
                    ],
                    "normalized_combined_factor_by_event": [0.95, 1.05, 1.0],
                    "intraday_factor_by_bin": [1.0] * 3,
                    "day_of_week_factor": [1.0] * 7,
                    "day_of_year_factor": [1.0] * 366,
                },
            },
            {
                "stage_report_kind": "hawkes",
                "stage": "hawkes",
                "status": "success",
                "run_id": run_id,
                "execution_mode": "memory",
                "handoff_policy": "explicit_run_spec_handoffs_mode_aware_transport.v1",
                "fitted_stage_payload": {
                    "schema_version": "calibration.stage_output.hawkes.v1",
                    "selected_candidate_id": "candidate_stable_low_loss",
                    "fitted_parameters": {
                        "kernel_parameters": {
                            "lambda_bar": 0.8,
                            "beta": 1.6,
                            "alpha": {
                                "plus_plus": 0.2,
                                "plus_minus": 0.1,
                                "minus_plus": 0.1,
                                "minus_minus": 0.25,
                            },
                            "rho": {
                                "plus_plus": 0.08,
                                "plus_minus": 0.04,
                                "minus_plus": 0.05,
                                "minus_minus": 0.1,
                            },
                        },
                    },
                },
            },
            {
                "stage_report_kind": "return_components",
                "stage": "return_components",
                "status": "failure",
                "run_id": run_id,
                "execution_mode": "memory",
                "handoff_policy": "explicit_run_spec_handoffs_mode_aware_transport.v1",
                "failure": {
                    "failure_kind": "invalid_candidate_generation_input",
                    "error_message": (
                        "open boundary for exchange `FXSPOT` at 1774224000000000000 must "
                        "transition from closed to open, found previous=Open and boundary=Open"
                    ),
                },
            },
        ],
        "emitted_checkpoints": [],
    }

    return validate_staged_calibration_response(raw_response), raw_response
