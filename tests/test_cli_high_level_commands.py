from contextlib import redirect_stdout
from io import StringIO
from pathlib import Path

import orjson
import pytest

import janiform.calibration._fit_api as high_level_module
import janiform.cli as cli_module
from janiform.calibration import (
    CalibrationDuplicateRecordPolicy,
    CalibrationInputFileFormat,
    CalibrationMarketDataNormalizationPolicy,
    CalibrationMarketDataWorkflowScope,
    CalibrationTickDataColumnMapping,
    CalibrationTickPriceInputPolicy,
    JaniformEstimator,
    JaniformModel,
    validate_staged_calibration_response,
)
from tests.cli_test_support import invoke_cli, normalize_help_output


def test_cli_top_level_help_lists_high_level_workflow_commands() -> None:
    help_output = _render_help([])

    assert "presets" in help_output
    assert "fit" in help_output
    assert "simulate" in help_output
    assert "fit-and-simulate" in help_output
    assert "calibration" in help_output


def test_fit_help_documents_preset_and_override_inputs() -> None:
    help_output = normalize_help_output(_render_help(["fit"]))

    assert "--market MARKET" in help_output
    assert "--session-preset SESSION_PRESET" in help_output
    assert "--session-json PATH" in help_output
    assert "--known-news KNOWN_NEWS" in help_output
    assert "--known-news-json PATH" in help_output
    assert "--model-output-path PATH" in help_output


def test_presets_show_and_fit_parser_map_high_level_arguments() -> None:
    parser = cli_module._build_parser()

    presets_arguments = parser.parse_args(
        [
            "presets",
            "show",
            "market-profile",
            "us_equity_cash",
            "--scope",
            "built_in",
        ]
    )
    fit_arguments = parser.parse_args(
        [
            "fit",
            "--run-id",
            "fit-parser",
            "--workflow-scope",
            "full_without_option_surface",
            "--tick-data-path",
            "ticks.csv",
            "--tick-data-format",
            "csv",
            "--tick-timestamp-column",
            "timestamp_utc",
            "--tick-trade-price-column",
            "trade_price",
            "--tick-trade-sign-column",
            "trade_sign",
            "--tick-price-input-policy",
            "trade_price_column",
            "--timestamp-timezone",
            "UTC",
            "--market",
            "us_equity_cash",
            "--known-news",
            "none",
            "--model-output-path",
            "model.json",
        ]
    )

    assert presets_arguments.command == "presets"
    assert presets_arguments.presets_command == "show"
    assert presets_arguments.category == "market-profile"
    assert presets_arguments.scope == "built_in"

    assert fit_arguments.command == "fit"
    assert fit_arguments.market == "us_equity_cash"
    assert fit_arguments.known_news == "none"
    assert fit_arguments.model_output_path == Path("model.json")
    assert fit_arguments.execution_mode == "memory"


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
    model_output_path = tmp_path / "model.json"

    monkeypatch.setattr(
        high_level_module,
        "_run_staged_calibration_with_raw_payload",
        lambda *args, **kwargs: _successful_full_model_staged_runner_payload("cli-fit-success"),
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
            "--tick-trade-price-column",
            "trade_price",
            "--tick-trade-sign-column",
            "trade_sign",
            "--tick-price-input-policy",
            "trade_price_column",
            "--timestamp-timezone",
            "UTC",
            "--market",
            "us_equity_cash",
            "--known-news",
            "none",
            "--model-output-path",
            str(model_output_path),
            "--output",
            "json",
        ]
    )

    assert exit_code == int(cli_module.CliExitCode.SUCCESS), stderr
    payload = orjson.loads(stdout)

    assert payload["schema_version"] == cli_module.CLI_FIT_RESULT_SCHEMA_VERSION
    assert payload["command"] == cli_module.CLI_FIT_ENTRYPOINT
    assert payload["model_output_path"] == str(model_output_path)
    assert payload["resolved_configuration"]["market_profile"]["selected_preset"]["preset_name"] == "us_equity_cash"
    assert payload["resolved_configuration"]["session"]["selected_preset"]["preset_name"] == "XNYS"
    assert payload["resolved_configuration"]["known_news"]["selected_preset"]["preset_name"] == "none"
    assert payload["model_summary"]["preset_provenance"]["session"]["inferred_from_data"] is False
    assert payload["session_resolution"]["resolution_kind"] == "explicit_override"
    assert model_output_path.exists()


def test_simulate_outputs_model_provenance_and_simulation_payload(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    model = _build_full_workflow_model(
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
            "--simulation-output",
            "coarse",
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
    assert payload["coarse_simulation"]["schema_version"] == "simulation.coarse_interval.path.v1"
    assert payload["seeded_tick_simulation"] is None


def test_fit_and_simulate_outputs_fit_and_simulation_sections(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    tick_data_path = _write_tick_data(
        tmp_path / "fit-and-simulate.csv",
        rows=(
            "2026-02-02T14:30:00Z,100.0,buy",
            "2026-02-02T15:00:00Z,101.0,sell",
            "2026-02-02T20:59:00Z,102.0,buy",
            "2026-02-03T14:30:00Z,99.0,sell",
            "2026-02-03T20:59:00Z,98.0,buy",
        ),
    )

    monkeypatch.setattr(
        high_level_module,
        "_run_staged_calibration_with_raw_payload",
        lambda *args, **kwargs: _successful_full_model_staged_runner_payload("cli-fit-and-simulate"),
    )

    exit_code, stdout, stderr = invoke_cli(
        [
            "fit-and-simulate",
            "--run-id",
            "cli-fit-and-simulate",
            "--workflow-scope",
            "full_without_option_surface",
            "--tick-data-path",
            str(tick_data_path),
            "--tick-data-format",
            "csv",
            "--tick-timestamp-column",
            "timestamp_utc",
            "--tick-trade-price-column",
            "trade_price",
            "--tick-trade-sign-column",
            "trade_sign",
            "--tick-price-input-policy",
            "trade_price_column",
            "--timestamp-timezone",
            "UTC",
            "--market",
            "us_equity_cash",
            "--known-news",
            "none",
            "--seed",
            "17",
            "--start-time-utc",
            "2026-02-02T15:00:00Z",
            "--horizon-interval-count",
            "2",
            "--simulation-output",
            "coarse",
            "--output",
            "json",
        ]
    )

    assert exit_code == int(cli_module.CliExitCode.SUCCESS), stderr
    payload = orjson.loads(stdout)

    assert payload["schema_version"] == cli_module.CLI_FIT_AND_SIMULATE_RESULT_SCHEMA_VERSION
    assert payload["fit_result"]["resolved_configuration"]["known_news"]["selected_preset"]["preset_name"] == "none"
    assert payload["simulation_result"]["coarse_simulation"]["schema_version"] == "simulation.coarse_interval.path.v1"


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
            "--tick-trade-price-column",
            "trade_price",
            "--tick-trade-sign-column",
            "trade_sign",
            "--tick-price-input-policy",
            "trade_price_column",
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
            "--tick-trade-price-column",
            "trade_price",
            "--tick-trade-sign-column",
            "trade_sign",
            "--tick-price-input-policy",
            "trade_price_column",
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


def test_fit_parser_rejects_contradictory_known_news_flags() -> None:
    parser = cli_module._build_parser()

    with pytest.raises(SystemExit) as captured_exit:
        parser.parse_args(
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

    assert captured_exit.value.code == 2


def test_advanced_calibration_help_remains_available_but_secondary() -> None:
    help_output = normalize_help_output(_render_help(["calibration"]))

    assert "Advanced calibration request and staged execution workflows." in help_output
    assert "market-data" in help_output
    assert "staged" in help_output


def _render_help(arguments: list[str]) -> str:
    parser = cli_module._build_parser()
    stdout_buffer = StringIO()

    with pytest.raises(SystemExit) as captured_exit:
        with redirect_stdout(stdout_buffer):
            parser.parse_args([*arguments, "--help"])

    assert captured_exit.value.code == 0
    return stdout_buffer.getvalue()


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


def _build_full_workflow_model(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    *,
    run_id: str,
) -> JaniformModel:
    tick_data_path = _write_tick_data(
        tmp_path / f"{run_id}.csv",
        rows=(
            "2026-02-02T14:30:00Z,100.0,buy",
            "2026-02-02T15:00:00Z,101.0,sell",
            "2026-02-02T20:59:00Z,102.0,buy",
            "2026-02-03T14:30:00Z,99.0,sell",
            "2026-02-03T20:59:00Z,98.0,buy",
            "2026-02-04T14:30:00Z,97.0,sell",
        ),
    )

    monkeypatch.setattr(
        high_level_module,
        "_run_staged_calibration_with_raw_payload",
        lambda *args, **kwargs: _successful_full_model_staged_runner_payload(run_id),
    )

    estimator = JaniformEstimator(
        workflow_scope=CalibrationMarketDataWorkflowScope.FULL_WITHOUT_OPTION_SURFACE,
        tick_data_column_mapping=CalibrationTickDataColumnMapping(
            timestamp_column="timestamp_utc",
            trade_price_column="trade_price",
            trade_sign_column="trade_sign",
        ),
        normalization_policy=CalibrationMarketDataNormalizationPolicy(
            timestamp_timezone="UTC",
            sort_records_by_timestamp=True,
            duplicate_record_policy=CalibrationDuplicateRecordPolicy.ERROR,
            tick_price_input_policy=CalibrationTickPriceInputPolicy.TRADE_PRICE_COLUMN,
        ),
        market_profile="us_equity_cash",
    )

    return estimator.fit(
        run_id=run_id,
        tick_data_path=tick_data_path,
        tick_data_format=CalibrationInputFileFormat.CSV,
    )


def _successful_full_model_staged_runner_payload(run_id: str) -> tuple[object, dict[str, object]]:
    raw_response = {
        "schema_version": "calibration.orchestration.staged_run.report.v1",
        "failure_triage_policy": "mode_aware_failure_kind_and_checkpoint_context.v1",
        "run_id": run_id,
        "execution_mode": "memory",
        "status": "success",
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
                    "exchange_code": "XNYS",
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
                        }
                    },
                },
            },
            {
                "stage_report_kind": "return_components",
                "stage": "return_components",
                "status": "success",
                "run_id": run_id,
                "execution_mode": "memory",
                "handoff_policy": "explicit_run_spec_handoffs_mode_aware_transport.v1",
                "fitted_stage_payload": {
                    "schema_version": "calibration.stage_output.return_components.v1",
                    "selected_candidate_id": "return_candidate_balanced",
                    "fitted_parameters": {
                        "nig_shared_plus_shift_configuration": {
                            "alpha": 2.3,
                            "beta_0": -0.8,
                            "delta": 0.7,
                            "mu": 0.1,
                            "delta_beta": -0.2,
                        },
                        "news_process_configuration": {
                            "schema_version": "news.process.configuration.v1",
                            "arrivals": {
                                "known": {
                                    "scheduled_event_timestamps_utc_nanos": [],
                                },
                                "surprise": {
                                    "baseline_intensity": 0.0,
                                },
                            },
                            "shared_mark_law": {
                                "gamma_severity_marks": {
                                    "shape_k": 1.5,
                                    "scale_theta": 0.9,
                                },
                                "double_exponential_base_shock": {
                                    "probability_positive": 0.4,
                                    "eta_plus": 1.3,
                                    "eta_minus": 1.1,
                                },
                                "coupled_gap_marks": {
                                    "sigma_g": 0.75,
                                },
                            },
                            "known_component_mark_scales": {
                                "activity_coupling_scale": 0.6,
                                "return_jump_scale": 1.2,
                            },
                            "surprise_component_mark_scales": {
                                "activity_coupling_scale": 0.6,
                                "return_jump_scale": 1.2,
                            },
                        },
                    },
                },
            },
        ],
        "emitted_checkpoints": [],
    }

    return validate_staged_calibration_response(raw_response), raw_response
