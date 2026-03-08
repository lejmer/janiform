import json
from contextlib import redirect_stderr, redirect_stdout
from datetime import UTC, datetime, timedelta
from io import StringIO
from pathlib import Path
from zoneinfo import ZoneInfo, ZoneInfoNotFoundError

import orjson
import pyarrow as pa
import pytest
from pyarrow import parquet as pyarrow_parquet

import janiform.calibration as calibration_module
import janiform.cli as cli_module
import janiform.pricing as pricing_module
import janiform.simulation as simulation_module
from janiform.calendar import CalendarRegistry
from janiform.calibration import (
    CALIBRATION_MARKET_DATA_REQUEST_SCHEMA_VERSION,
    CalibrationExecutionMode,
    CalibrationMarketDataWorkflowScope,
    StagedCalibrationExecutionInput,
    StagedCalibrationResponse,
)
from janiform.errors import JaniformErrorCategory, JaniformErrorDetails, JaniformExecutionError
from janiform.hourly_factors import (
    HourlyNewsIntensityProviderExplicitSource,
    HourlySeasonalityProviderExplicitSource,
)
from janiform.pricing import VanillaOptionPricingExecutionInput
from janiform.simulation import (
    HawkesExcitationState,
    HawkesKernelLoadings,
    HawkesKernelParameters,
    SeededTickSimulationRequest,
    load_hourly_factor_contract_for_simulation,
)
from tests.calibration_test_support import build_valid_execution_input_model


def test_simulation_cli_help_documents_required_contract_inputs() -> None:
    help_output = _normalize_help_output(_render_help(["simulation", "seeded-ticks"]))

    assert "--output {human,json}" in help_output
    assert "--request-json PATH" in help_output
    assert "--calendar-registry-json PATH" in help_output
    assert cli_module.CLI_REPOSITORY_RELATIVE_PATH_POLICY in help_output


def test_pricing_cli_help_documents_execution_input_contract() -> None:
    help_output = _normalize_help_output(_render_help(["pricing", "vanilla-option"]))

    assert "--output {human,json}" in help_output
    assert "--execution-input-json PATH" in help_output
    assert cli_module.CLI_REPOSITORY_RELATIVE_PATH_POLICY in help_output


def test_calibration_cli_help_documents_mode_and_runtime_arguments() -> None:
    help_output = _normalize_help_output(_render_help(["calibration", "staged"]))

    assert "--output {human,json}" in help_output
    assert "--execution-input-json PATH" in help_output
    assert "--calendar-registry-json PATH" in help_output
    assert "--execution-mode {disk,memory}" in help_output
    assert "Precedence: --execution-mode, then the CLI default disk." in help_output
    assert "overrides request.execution_mode" in help_output
    assert "--disk-checkpoint-directory DIRECTORY" in help_output
    assert "overrides runtime_resources.disk_checkpoint_directory from --execution-input-json" in help_output


def test_market_data_calibration_cli_help_documents_user_facing_file_inputs() -> None:
    help_output = _normalize_help_output(_render_help(["calibration", "market-data"]))

    assert "--output {human,json}" in help_output
    assert "--tick-data-path PATH" in help_output
    assert "--session-definition-path PATH" in help_output
    assert "--workflow-scope {seasonality_only,full_without_option_surface,full_with_option_surface}" in help_output
    assert "--tick-price-input-policy" in help_output
    assert "--execution-mode {disk,memory}" in help_output
    assert "market-data calibration summary" in help_output


def test_cli_shared_path_flags_use_standardized_repository_relative_help_text() -> None:
    help_outputs = (
        _normalize_help_output(_render_help(["simulation", "seeded-ticks"])),
        _normalize_help_output(_render_help(["pricing", "vanilla-option"])),
        _normalize_help_output(_render_help(["calibration", "market-data"])),
        _normalize_help_output(_render_help(["calibration", "staged"])),
        _normalize_help_output(_render_help(["dev", "regression", "open-closed-gap-leakage"])),
    )

    for help_output in help_outputs:
        assert cli_module.CLI_REPOSITORY_RELATIVE_PATH_POLICY in help_output


def test_calibration_cli_parser_defaults_execution_mode_to_disk() -> None:
    parser = cli_module._build_parser()
    parsed_arguments = parser.parse_args(
        [
            "calibration",
            "staged",
            "--execution-input-json",
            "calibration.json",
            "--calendar-registry-json",
            "calendar.json",
        ]
    )

    assert parsed_arguments.execution_mode == CalibrationExecutionMode.DISK.value


def test_market_data_calibration_cli_parser_defaults_execution_mode_to_disk() -> None:
    parser = cli_module._build_parser()
    parsed_arguments = parser.parse_args(
        [
            "calibration",
            "market-data",
            "--run-id",
            "run-001",
            "--workflow-scope",
            "seasonality_only",
            "--tick-data-path",
            "ticks.csv",
            "--tick-data-format",
            "csv",
            "--tick-data-provenance-label",
            "tick-feed",
            "--tick-timestamp-column",
            "timestamp_utc",
            "--exchange-code",
            "XNYS",
            "--session-definition-path",
            "sessions.json",
            "--session-definition-format",
            "json",
            "--session-definition-provenance-label",
            "session-registry",
            "--session-timezone",
            "America/New_York",
            "--timestamp-timezone",
            "UTC",
        ]
    )

    assert parsed_arguments.execution_mode == CalibrationExecutionMode.DISK.value


def test_primary_workflow_cli_parsers_default_output_to_json() -> None:
    parser = cli_module._build_parser()
    simulation_arguments = parser.parse_args(
        [
            "simulation",
            "seeded-ticks",
            "--request-json",
            "simulation.json",
            "--calendar-registry-json",
            "calendar.json",
        ]
    )
    pricing_arguments = parser.parse_args(
        [
            "pricing",
            "vanilla-option",
            "--execution-input-json",
            "pricing.json",
        ]
    )
    staged_arguments = parser.parse_args(
        [
            "calibration",
            "staged",
            "--execution-input-json",
            "calibration.json",
            "--calendar-registry-json",
            "calendar.json",
        ]
    )

    assert simulation_arguments.output == "json"
    assert pricing_arguments.output == "json"
    assert staged_arguments.output == "json"


def test_primary_workflow_cli_parsers_accept_human_output_mode() -> None:
    parser = cli_module._build_parser()
    simulation_arguments = parser.parse_args(
        [
            "simulation",
            "seeded-ticks",
            "--request-json",
            "simulation.json",
            "--calendar-registry-json",
            "calendar.json",
            "--output",
            "human",
        ]
    )
    pricing_arguments = parser.parse_args(
        [
            "pricing",
            "vanilla-option",
            "--execution-input-json",
            "pricing.json",
            "--output",
            "human",
        ]
    )
    staged_arguments = parser.parse_args(
        [
            "calibration",
            "staged",
            "--execution-input-json",
            "calibration.json",
            "--calendar-registry-json",
            "calendar.json",
            "--output",
            "human",
        ]
    )

    assert simulation_arguments.output == "human"
    assert pricing_arguments.output == "human"
    assert staged_arguments.output == "human"


def test_calibration_cli_parser_accepts_explicit_memory_execution_mode() -> None:
    parser = cli_module._build_parser()
    parsed_arguments = parser.parse_args(
        [
            "calibration",
            "staged",
            "--execution-input-json",
            "calibration.json",
            "--calendar-registry-json",
            "calendar.json",
            "--execution-mode",
            "memory",
        ]
    )

    assert parsed_arguments.execution_mode == CalibrationExecutionMode.MEMORY.value


def test_market_data_calibration_cli_maps_flags_to_summary_contract(
    calendar_registry_json: str,
    tmp_path: Path,
) -> None:
    market_data_paths = _write_market_data_input_files(
        tmp_path,
        calendar_registry_json=calendar_registry_json,
        include_known_news=True,
        include_option_surface=True,
    )
    checkpoint_directory = tmp_path / "checkpoints"

    exit_code, stdout_text, stderr_text = _invoke_cli(
        [
            "calibration",
            "market-data",
            "--run-id",
            "market-data-run-001",
            "--workflow-scope",
            "full_with_option_surface",
            "--tick-data-path",
            str(market_data_paths["tick_data"]),
            "--tick-data-format",
            "csv",
            "--tick-data-provenance-label",
            "ticks-primary",
            "--tick-timestamp-column",
            "timestamp_utc",
            "--tick-price-input-policy",
            "trade_price_column",
            "--tick-trade-price-column",
            "trade_price",
            "--exchange-code",
            "XNYS",
            "--session-definition-path",
            str(market_data_paths["session_definition"]),
            "--session-definition-format",
            "json",
            "--session-definition-provenance-label",
            "session-calendar",
            "--session-timezone",
            "America/New_York",
            "--known-news-path",
            str(market_data_paths["known_news"]),
            "--known-news-format",
            "csv",
            "--known-news-provenance-label",
            "known-news",
            "--known-news-timezone",
            "UTC",
            "--known-news-timestamp-column",
            "event_timestamp_utc",
            "--known-news-event-id-column",
            "event_code",
            "--option-surface-path",
            str(market_data_paths["option_surface"]),
            "--option-surface-format",
            "parquet",
            "--option-surface-provenance-label",
            "options-close",
            "--option-quote-input-policy",
            "bid_ask_midpoint",
            "--option-quote-timestamp-column",
            "quote_timestamp_utc",
            "--option-expiry-column",
            "expiry_utc",
            "--option-strike-column",
            "strike",
            "--option-type-column",
            "option_type",
            "--option-bid-column",
            "bid",
            "--option-ask-column",
            "ask",
            "--timestamp-timezone",
            "UTC",
            "--execution-mode",
            "disk",
            "--disk-checkpoint-directory",
            str(checkpoint_directory),
            "--emit-derived-staged-execution-input",
        ]
    )

    assert exit_code == int(cli_module.CliExitCode.SUCCESS), stderr_text

    summary_payload = orjson.loads(stdout_text)
    assert summary_payload["schema_version"] == cli_module.CLI_MARKET_DATA_SUMMARY_SCHEMA_VERSION
    assert summary_payload["request_schema_version"] == CALIBRATION_MARKET_DATA_REQUEST_SCHEMA_VERSION
    assert summary_payload["workflow_scope"] == CalibrationMarketDataWorkflowScope.FULL_WITH_OPTION_SURFACE.value
    assert summary_payload["session_context"]["exchange_code"] == "XNYS"
    assert Path(summary_payload["input_files"]["tick_data"]["path"]).parts[-2:] == ("data", "ticks.csv")
    assert summary_payload["input_files"]["option_surface"]["file_format"] == "parquet"
    assert summary_payload["column_mapping"]["known_news"]["event_timestamp_column"] == "event_timestamp_utc"
    assert summary_payload["output_artifact_locations"]["disk_checkpoint_directory"] == str(checkpoint_directory)
    assert summary_payload["output_emission_policy"]["execution_mode"] == "disk"
    assert summary_payload["output_emission_policy"]["emit_derived_staged_execution_input"] is True
    assert summary_payload["output_schema_metadata"]["field_selection_policy"] == (
        cli_module.CLI_MARKET_DATA_SUMMARY_FIELD_SELECTION_POLICY
    )

    assert "request" not in summary_payload
    assert "runtime_resources" not in summary_payload
    assert "selected_stages" not in stdout_text
    assert "seasonality_stage_input" not in stdout_text

    assert stdout_text == f"{json.dumps(summary_payload, sort_keys=True)}\n"


def test_market_data_calibration_cli_supports_human_output_mode(
    calendar_registry_json: str,
    tmp_path: Path,
) -> None:
    market_data_paths = _write_market_data_input_files(
        tmp_path,
        calendar_registry_json=calendar_registry_json,
    )

    exit_code, stdout_text, stderr_text = _invoke_cli(
        [
            "calibration",
            "market-data",
            "--run-id",
            "market-data-run-002",
            "--workflow-scope",
            "seasonality_only",
            "--tick-data-path",
            str(market_data_paths["tick_data"]),
            "--tick-data-format",
            "csv",
            "--tick-data-provenance-label",
            "ticks-primary",
            "--tick-timestamp-column",
            "timestamp_utc",
            "--exchange-code",
            "XNYS",
            "--session-definition-path",
            str(market_data_paths["session_definition"]),
            "--session-definition-format",
            "json",
            "--session-definition-provenance-label",
            "session-calendar",
            "--session-timezone",
            "America/New_York",
            "--timestamp-timezone",
            "UTC",
            "--execution-mode",
            "memory",
            "--output",
            "human",
        ]
    )

    assert exit_code == int(cli_module.CliExitCode.SUCCESS), stderr_text
    assert stdout_text.startswith("calibration market-data request prepared:")
    assert "workflow_scope=seasonality_only" in stdout_text
    assert "execution_mode=memory" in stdout_text
    assert stderr_text == ""


def test_market_data_cli_parser_rejects_unsupported_tick_data_format() -> None:
    parser = cli_module._build_parser()

    with pytest.raises(SystemExit) as captured_exit:
        parser.parse_args(
            [
                "calibration",
                "market-data",
                "--run-id",
                "market-data-run-unsupported-format",
                "--workflow-scope",
                "seasonality_only",
                "--tick-data-path",
                "ticks.json",
                "--tick-data-format",
                "json",
                "--tick-data-provenance-label",
                "ticks-primary",
                "--tick-timestamp-column",
                "timestamp_utc",
                "--exchange-code",
                "XNYS",
                "--session-definition-path",
                "sessions.json",
                "--session-definition-format",
                "json",
                "--session-definition-provenance-label",
                "session-calendar",
                "--session-timezone",
                "America/New_York",
                "--timestamp-timezone",
                "UTC",
            ]
        )

    assert captured_exit.value.code == int(cli_module.CliExitCode.VALIDATION_ERROR)


def test_market_data_cli_missing_tick_file_returns_validation_exit_code(
    calendar_registry_json: str,
    tmp_path: Path,
) -> None:
    market_data_paths = _write_market_data_input_files(
        tmp_path,
        calendar_registry_json=calendar_registry_json,
    )
    missing_tick_path = tmp_path / "data" / "missing-ticks.csv"

    exit_code, stdout_text, stderr_text = _invoke_cli(
        [
            "calibration",
            "market-data",
            "--run-id",
            "market-data-run-missing-tick-file",
            "--workflow-scope",
            "seasonality_only",
            "--tick-data-path",
            str(missing_tick_path),
            "--tick-data-format",
            "csv",
            "--tick-data-provenance-label",
            "ticks-primary",
            "--tick-timestamp-column",
            "timestamp_utc",
            "--exchange-code",
            "XNYS",
            "--session-definition-path",
            str(market_data_paths["session_definition"]),
            "--session-definition-format",
            "json",
            "--session-definition-provenance-label",
            "session-calendar",
            "--session-timezone",
            "America/New_York",
            "--timestamp-timezone",
            "UTC",
            "--execution-mode",
            "memory",
        ]
    )

    assert exit_code == int(cli_module.CliExitCode.VALIDATION_ERROR)
    assert stdout_text == ""
    assert 'field_path="tick_data_source.path"' in stderr_text
    assert f'"path": {json.dumps(str(missing_tick_path))}' in stderr_text
    assert '"normalization_step": "source_loading"' in stderr_text


def test_market_data_cli_ambiguous_timestamp_reports_file_field_and_normalization_step(
    calendar_registry_json: str,
    tmp_path: Path,
) -> None:
    market_data_paths = _write_market_data_input_files(
        tmp_path,
        calendar_registry_json=calendar_registry_json,
        ambiguous_tick_timestamp=True,
    )

    exit_code, stdout_text, stderr_text = _invoke_cli(
        [
            "calibration",
            "market-data",
            "--run-id",
            "market-data-run-ambiguous-timestamp",
            "--workflow-scope",
            "seasonality_only",
            "--tick-data-path",
            str(market_data_paths["tick_data"]),
            "--tick-data-format",
            "csv",
            "--tick-data-provenance-label",
            "ticks-primary",
            "--tick-timestamp-column",
            "timestamp_utc",
            "--exchange-code",
            "XNYS",
            "--session-definition-path",
            str(market_data_paths["session_definition"]),
            "--session-definition-format",
            "json",
            "--session-definition-provenance-label",
            "session-calendar",
            "--session-timezone",
            "America/New_York",
            "--timestamp-timezone",
            "America/New_York",
            "--execution-mode",
            "memory",
        ]
    )

    assert exit_code == int(cli_module.CliExitCode.VALIDATION_ERROR)
    assert stdout_text == ""
    assert 'field_path="tick_data_source.rows[1].timestamp_utc"' in stderr_text
    assert f'"source_path": {json.dumps(str(market_data_paths["tick_data"]))}' in stderr_text
    assert '"normalization_step": "timestamp_timezone_normalization"' in stderr_text

    if _timezone_key_is_available("America/New_York"):
        assert "ambiguous in the configured source timezone" in stderr_text
    else:
        assert "references unknown timezone 'America/New_York'" in stderr_text


def test_market_data_cli_missing_required_column_reports_source_file_and_normalization_step(
    calendar_registry_json: str,
    tmp_path: Path,
) -> None:
    market_data_paths = _write_market_data_input_files(
        tmp_path,
        calendar_registry_json=calendar_registry_json,
        tick_trade_price_column_name="last_trade",
    )

    exit_code, stdout_text, stderr_text = _invoke_cli(
        [
            "calibration",
            "market-data",
            "--run-id",
            "market-data-run-missing-column",
            "--workflow-scope",
            "full_without_option_surface",
            "--tick-data-path",
            str(market_data_paths["tick_data"]),
            "--tick-data-format",
            "csv",
            "--tick-data-provenance-label",
            "ticks-primary",
            "--tick-timestamp-column",
            "timestamp_utc",
            "--tick-price-input-policy",
            "trade_price_column",
            "--tick-trade-price-column",
            "trade_price",
            "--exchange-code",
            "XNYS",
            "--session-definition-path",
            str(market_data_paths["session_definition"]),
            "--session-definition-format",
            "json",
            "--session-definition-provenance-label",
            "session-calendar",
            "--session-timezone",
            "America/New_York",
            "--timestamp-timezone",
            "UTC",
            "--execution-mode",
            "memory",
        ]
    )

    assert exit_code == int(cli_module.CliExitCode.VALIDATION_ERROR)
    assert stdout_text == ""
    assert 'field_path="column_mapping.tick_data"' in stderr_text
    assert "Missing columns: trade_price" in stderr_text
    assert f'"source_path": {json.dumps(str(market_data_paths["tick_data"]))}' in stderr_text
    assert '"normalization_step": "column_mapping_validation"' in stderr_text


def test_market_data_cli_invalid_session_definition_returns_validation_exit_code(
    tmp_path: Path,
) -> None:
    invalid_session_definition = tmp_path / "config" / "invalid-sessions.json"
    invalid_session_definition.parent.mkdir(parents=True, exist_ok=True)
    invalid_session_definition.write_text("{}", encoding="utf-8")

    tick_data_path = tmp_path / "data" / "ticks.csv"
    tick_data_path.parent.mkdir(parents=True, exist_ok=True)
    tick_data_path.write_text(
        "\n".join(
            [
                "timestamp_utc",
                "2026-02-02T14:30:00Z",
            ]
        ),
        encoding="utf-8",
    )

    exit_code, stdout_text, stderr_text = _invoke_cli(
        [
            "calibration",
            "market-data",
            "--run-id",
            "market-data-run-invalid-session-definition",
            "--workflow-scope",
            "seasonality_only",
            "--tick-data-path",
            str(tick_data_path),
            "--tick-data-format",
            "csv",
            "--tick-data-provenance-label",
            "ticks-primary",
            "--tick-timestamp-column",
            "timestamp_utc",
            "--exchange-code",
            "XNYS",
            "--session-definition-path",
            str(invalid_session_definition),
            "--session-definition-format",
            "json",
            "--session-definition-provenance-label",
            "session-calendar",
            "--session-timezone",
            "America/New_York",
            "--timestamp-timezone",
            "UTC",
            "--execution-mode",
            "memory",
        ]
    )

    assert exit_code == int(cli_module.CliExitCode.VALIDATION_ERROR)
    assert stdout_text == ""
    assert 'field_path="session_configuration.session_definition_source"' in stderr_text
    assert "calendar.registry.v1" in stderr_text


def test_market_data_cli_inconsistent_known_news_inputs_return_validation_exit_code(
    calendar_registry_json: str,
    tmp_path: Path,
) -> None:
    market_data_paths = _write_market_data_input_files(
        tmp_path,
        calendar_registry_json=calendar_registry_json,
        include_known_news=True,
    )

    exit_code, stdout_text, stderr_text = _invoke_cli(
        [
            "calibration",
            "market-data",
            "--run-id",
            "market-data-run-inconsistent-known-news",
            "--workflow-scope",
            "seasonality_only",
            "--tick-data-path",
            str(market_data_paths["tick_data"]),
            "--tick-data-format",
            "csv",
            "--tick-data-provenance-label",
            "ticks-primary",
            "--tick-timestamp-column",
            "timestamp_utc",
            "--exchange-code",
            "XNYS",
            "--session-definition-path",
            str(market_data_paths["session_definition"]),
            "--session-definition-format",
            "json",
            "--session-definition-provenance-label",
            "session-calendar",
            "--session-timezone",
            "America/New_York",
            "--known-news-path",
            str(market_data_paths["known_news"]),
            "--known-news-format",
            "csv",
            "--known-news-provenance-label",
            "known-news",
            "--known-news-timezone",
            "UTC",
            "--timestamp-timezone",
            "UTC",
            "--execution-mode",
            "memory",
        ]
    )

    assert exit_code == int(cli_module.CliExitCode.VALIDATION_ERROR)
    assert stdout_text == ""
    assert 'field_path="column_mapping.known_news"' in stderr_text
    assert "csv or parquet" in stderr_text


def test_simulation_cli_maps_request_json_to_runtime_request(
    calendar_registry_json: str,
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> None:
    request = _build_simulation_request(calendar_registry_json)
    captured_request: dict[str, SeededTickSimulationRequest] = {}

    def capturing_runner(request_model: SeededTickSimulationRequest) -> simulation_module.SeededTickSimulationResponse:
        captured_request["request"] = request_model
        return simulation_module.run_seeded_tick_event_simulation(request_model)

    monkeypatch.setattr(cli_module, "run_seeded_tick_event_simulation", capturing_runner)

    request_path = _write_json_path(tmp_path / "simulation_request.json", request.to_payload())
    calendar_registry_path = _write_json_path(
        tmp_path / "calendar_registry.json",
        orjson.loads(calendar_registry_json),
    )

    exit_code, stdout_text, stderr_text = _invoke_cli(
        [
            "simulation",
            "seeded-ticks",
            "--request-json",
            str(request_path),
            "--calendar-registry-json",
            str(calendar_registry_path),
        ]
    )

    assert exit_code == int(cli_module.CliExitCode.SUCCESS), stderr_text
    assert captured_request["request"].hourly_factor_summary == request.hourly_factor_summary
    assert captured_request["request"].hourly_factor_contract.interval_count == request.horizon_interval_count

    command_output = orjson.loads(stdout_text)
    assert command_output["schema_version"] == "simulation.seeded_tick_event.path.v1"
    assert "event_tape" in command_output


def test_pricing_cli_maps_execution_input_json_to_runtime_contract(
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> None:
    execution_input = _build_pricing_execution_input()
    captured_execution_input: dict[str, VanillaOptionPricingExecutionInput] = {}

    def capturing_runner(
        execution_input_model: VanillaOptionPricingExecutionInput,
    ) -> pricing_module.VanillaOptionPricingRunResult:
        captured_execution_input["execution_input"] = execution_input_model
        return pricing_module.run_vanilla_option_pricing(execution_input_model)

    monkeypatch.setattr(cli_module, "run_vanilla_option_pricing", capturing_runner)

    execution_input_path = _write_json_path(
        tmp_path / "pricing_execution_input.json",
        execution_input.to_payload(),
    )

    exit_code, stdout_text, stderr_text = _invoke_cli(
        [
            "pricing",
            "vanilla-option",
            "--execution-input-json",
            str(execution_input_path),
        ]
    )

    assert exit_code == int(cli_module.CliExitCode.SUCCESS), stderr_text
    assert captured_execution_input["execution_input"].request.payoff.payoff_type.value == "call"
    assert captured_execution_input["execution_input"].interval_factors.nu_h_known == pytest.approx(0.01)
    assert captured_execution_input["execution_input"].interval_factors.nu_h_surprise == pytest.approx(0.02)

    command_output = orjson.loads(stdout_text)
    assert command_output["schema_version"] == "pricing.vanilla_option.run_result.v1"
    assert command_output["response"]["price"] >= 0.0


def test_calibration_cli_defaults_execution_mode_to_disk(
    calendar_registry_json: str,
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> None:
    execution_input = build_valid_execution_input_model()
    captured_execution_input: dict[str, StagedCalibrationExecutionInput] = {}

    def capturing_runner(
        execution_input_model: StagedCalibrationExecutionInput,
        *,
        calendar_registry: CalendarRegistry,
    ) -> StagedCalibrationResponse:
        del calendar_registry
        captured_execution_input["execution_input"] = execution_input_model
        return calibration_module.run_staged_calibration(
            execution_input_model,
            calendar_registry=CalendarRegistry(calendar_registry_json),
        )

    monkeypatch.setattr(cli_module, "run_staged_calibration", capturing_runner)

    execution_input_path = _write_json_path(
        tmp_path / "calibration_execution_input.json",
        execution_input.to_payload(),
    )
    calendar_registry_path = _write_json_path(
        tmp_path / "calendar_registry.json",
        orjson.loads(calendar_registry_json),
    )
    checkpoint_directory = tmp_path / "disk-checkpoints"

    exit_code, stdout_text, stderr_text = _invoke_cli(
        [
            "calibration",
            "staged",
            "--execution-input-json",
            str(execution_input_path),
            "--calendar-registry-json",
            str(calendar_registry_path),
            "--disk-checkpoint-directory",
            str(checkpoint_directory),
        ]
    )

    assert exit_code == int(cli_module.CliExitCode.SUCCESS), stderr_text

    normalized_execution_input = captured_execution_input["execution_input"]
    assert normalized_execution_input.request.execution_mode == CalibrationExecutionMode.DISK
    assert normalized_execution_input.runtime_resources is not None
    assert normalized_execution_input.runtime_resources.disk_checkpoint_directory == str(checkpoint_directory)

    command_output = orjson.loads(stdout_text)
    assert command_output["execution_mode"] == "disk"


def test_calibration_cli_honors_explicit_memory_override(
    calendar_registry_json: str,
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> None:
    execution_input = build_valid_execution_input_model()
    captured_execution_input: dict[str, StagedCalibrationExecutionInput] = {}

    def capturing_runner(
        execution_input_model: StagedCalibrationExecutionInput,
        *,
        calendar_registry: CalendarRegistry,
    ) -> StagedCalibrationResponse:
        del calendar_registry
        captured_execution_input["execution_input"] = execution_input_model
        return calibration_module.run_staged_calibration(
            execution_input_model,
            calendar_registry=CalendarRegistry(calendar_registry_json),
        )

    monkeypatch.setattr(cli_module, "run_staged_calibration", capturing_runner)

    execution_input_path = _write_json_path(
        tmp_path / "calibration_execution_input.json",
        execution_input.to_payload(),
    )
    calendar_registry_path = _write_json_path(
        tmp_path / "calendar_registry.json",
        orjson.loads(calendar_registry_json),
    )

    exit_code, stdout_text, stderr_text = _invoke_cli(
        [
            "calibration",
            "staged",
            "--execution-input-json",
            str(execution_input_path),
            "--calendar-registry-json",
            str(calendar_registry_path),
            "--execution-mode",
            "memory",
        ]
    )

    assert exit_code == int(cli_module.CliExitCode.SUCCESS), stderr_text

    normalized_execution_input = captured_execution_input["execution_input"]
    assert normalized_execution_input.request.execution_mode == CalibrationExecutionMode.MEMORY
    assert normalized_execution_input.runtime_resources is None

    command_output = orjson.loads(stdout_text)
    assert command_output["execution_mode"] == "memory"


def test_calibration_cli_resolves_relative_disk_checkpoint_directory_from_repository_root() -> None:
    execution_input = build_valid_execution_input_model()
    relative_checkpoint_directory = Path("tmp") / "cli-checkpoints"
    repository_root = Path(cli_module.__file__).resolve().parents[2]
    resolved_checkpoint_directory = cli_module._resolve_path_from_repository_root(
        repository_root,
        relative_checkpoint_directory,
    )
    normalized_execution_input = cli_module._override_calibration_execution_mode(
        execution_input=execution_input,
        execution_mode=CalibrationExecutionMode.DISK,
        disk_checkpoint_directory=resolved_checkpoint_directory,
    )

    assert normalized_execution_input.runtime_resources is not None
    assert normalized_execution_input.runtime_resources.disk_checkpoint_directory == str(
        repository_root / relative_checkpoint_directory
    )


def test_calibration_cli_runtime_failures_return_runtime_exit_code(
    calendar_registry_json: str,
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> None:
    execution_input_path = _write_json_path(
        tmp_path / "calibration_execution_input.json",
        build_valid_execution_input_model().to_payload(),
    )
    calendar_registry_path = _write_json_path(
        tmp_path / "calendar_registry.json",
        orjson.loads(calendar_registry_json),
    )

    def failing_runner(
        execution_input_model: StagedCalibrationExecutionInput,
        *,
        calendar_registry: CalendarRegistry,
    ) -> StagedCalibrationResponse:
        del execution_input_model
        del calendar_registry

        raise JaniformExecutionError(
            JaniformErrorDetails.from_context(
                error_id="calibration.execution.run_failed",
                category=JaniformErrorCategory.EXECUTION,
                workflow="calibration",
                entrypoint="run_staged_calibration",
                message="native calibration failed",
            )
        )

    monkeypatch.setattr(cli_module, "run_staged_calibration", failing_runner)

    exit_code, stdout_text, stderr_text = _invoke_cli(
        [
            "calibration",
            "staged",
            "--execution-input-json",
            str(execution_input_path),
            "--calendar-registry-json",
            str(calendar_registry_path),
            "--execution-mode",
            "memory",
        ]
    )

    assert exit_code == int(cli_module.CliExitCode.RUNTIME_ERROR)
    assert stdout_text == ""
    assert '[calibration.execution.run_failed]' in stderr_text
    assert 'category="execution"' in stderr_text


def test_pricing_cli_unexpected_failures_return_internal_exit_code(
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> None:
    execution_input_path = _write_json_path(
        tmp_path / "pricing_execution_input.json",
        _build_pricing_execution_input().to_payload(),
    )

    def broken_runner(_: VanillaOptionPricingExecutionInput) -> pricing_module.VanillaOptionPricingRunResult:
        raise RuntimeError("unexpected pricing bug")

    monkeypatch.setattr(cli_module, "run_vanilla_option_pricing", broken_runner)

    exit_code, stdout_text, stderr_text = _invoke_cli(
        [
            "pricing",
            "vanilla-option",
            "--execution-input-json",
            str(execution_input_path),
        ]
    )

    assert exit_code == int(cli_module.CliExitCode.INTERNAL_ERROR)
    assert stdout_text == ""
    assert '[cli.internal.pricing.vanilla_option.failed]' in stderr_text
    assert 'category="integration"' in stderr_text


def test_calibration_cli_requires_disk_checkpoint_directory_when_defaulting_to_disk(
    calendar_registry_json: str,
    tmp_path: Path,
) -> None:
    execution_input_path = _write_json_path(
        tmp_path / "calibration_execution_input.json",
        build_valid_execution_input_model().to_payload(),
    )
    calendar_registry_path = _write_json_path(
        tmp_path / "calendar_registry.json",
        orjson.loads(calendar_registry_json),
    )

    exit_code, stdout_text, stderr_text = _invoke_cli(
        [
            "calibration",
            "staged",
            "--execution-input-json",
            str(execution_input_path),
            "--calendar-registry-json",
            str(calendar_registry_path),
        ]
    )

    assert exit_code == int(cli_module.CliExitCode.VALIDATION_ERROR)
    assert stdout_text == ""

    assert 'field_path="runtime_resources.disk_checkpoint_directory"' in stderr_text
    assert 'message="is required when execution_mode=disk"' in stderr_text


def test_calibration_cli_rejects_disk_checkpoint_directory_in_memory_mode(
    calendar_registry_json: str,
    tmp_path: Path,
) -> None:
    execution_input_path = _write_json_path(
        tmp_path / "calibration_execution_input.json",
        build_valid_execution_input_model().to_payload(),
    )
    calendar_registry_path = _write_json_path(
        tmp_path / "calendar_registry.json",
        orjson.loads(calendar_registry_json),
    )
    checkpoint_directory = tmp_path / "unexpected-disk-checkpoints"

    exit_code, stdout_text, stderr_text = _invoke_cli(
        [
            "calibration",
            "staged",
            "--execution-input-json",
            str(execution_input_path),
            "--calendar-registry-json",
            str(calendar_registry_path),
            "--execution-mode",
            "memory",
            "--disk-checkpoint-directory",
            str(checkpoint_directory),
        ]
    )

    assert exit_code == int(cli_module.CliExitCode.VALIDATION_ERROR)
    assert stdout_text == ""

    assert 'field_path="disk_checkpoint_directory"' in stderr_text
    assert 'message="--disk-checkpoint-directory requires --execution-mode disk"' in stderr_text


def test_calibration_cli_rejects_malformed_close_open_boundary_observation(
    calendar_registry_json: str,
    tmp_path: Path,
) -> None:
    execution_input_payload = build_valid_execution_input_model().to_payload()
    del execution_input_payload["return_components_candidate_generation_input"][
        "observed_close_open_boundary_observations"
    ][0]["open_price"]

    execution_input_path = _write_json_path(
        tmp_path / "calibration_execution_input.json",
        execution_input_payload,
    )
    calendar_registry_path = _write_json_path(
        tmp_path / "calendar_registry.json",
        orjson.loads(calendar_registry_json),
    )

    exit_code, stdout_text, stderr_text = _invoke_cli(
        [
            "calibration",
            "staged",
            "--execution-input-json",
            str(execution_input_path),
            "--calendar-registry-json",
            str(calendar_registry_path),
            "--execution-mode",
            "memory",
        ]
    )

    assert exit_code == int(cli_module.CliExitCode.VALIDATION_ERROR)
    assert stdout_text == ""
    assert (
        'field_path="return_components_candidate_generation_input.observed_close_open_boundary_observations[0].open_price"'
        in stderr_text
    )
    assert 'message="is required"' in stderr_text


def _build_pricing_execution_input() -> VanillaOptionPricingExecutionInput:
    from examples._shared import (
        build_pricing_interval_factors,
        build_pricing_optional_output_selection,
        build_pricing_request,
    )

    return VanillaOptionPricingExecutionInput(
        request=build_pricing_request(),
        interval_factors=build_pricing_interval_factors(),
        optional_output_selection=build_pricing_optional_output_selection(),
    )


def _build_simulation_request(calendar_registry_json: str) -> SeededTickSimulationRequest:
    calendar_registry = CalendarRegistry(calendar_registry_json)
    start_time_utc = datetime(2026, 2, 2, 14, tzinfo=UTC)

    hourly_factor_contract = load_hourly_factor_contract_for_simulation(
        calendar_registry=calendar_registry,
        exchange_code="XNYS",
        range_start_utc=start_time_utc,
        range_end_utc=start_time_utc + timedelta(hours=2),
        seasonality_source=HourlySeasonalityProviderExplicitSource(hourly_factors=(1.0, 0.9)),
        news_intensity_source=HourlyNewsIntensityProviderExplicitSource(hourly_factors=(0.2, 0.1)),
    )

    return SeededTickSimulationRequest.from_runtime_inputs(
        seed=17,
        start_time_utc=start_time_utc,
        horizon_interval_count=2,
        kernel_parameters=HawkesKernelParameters(
            lambda_bar=0.8,
            beta=1.6,
            alpha=HawkesKernelLoadings(
                plus_plus=0.2,
                plus_minus=0.1,
                minus_plus=0.1,
                minus_minus=0.25,
            ),
            rho=HawkesKernelLoadings(
                plus_plus=0.08,
                plus_minus=0.04,
                minus_plus=0.05,
                minus_minus=0.1,
            ),
        ),
        initial_excitation_state=HawkesExcitationState(
            y_plus_plus=0.0,
            y_plus_minus=0.0,
            y_minus_plus=0.0,
            y_minus_minus=0.0,
        ),
        hourly_factor_contract=hourly_factor_contract,
    )


def _invoke_cli(arguments: list[str]) -> tuple[int, str, str]:
    parser = cli_module._build_parser()
    parsed_arguments = parser.parse_args(arguments)
    stdout_buffer = StringIO()
    stderr_buffer = StringIO()

    with redirect_stdout(stdout_buffer), redirect_stderr(stderr_buffer):
        exit_code = parsed_arguments.handler(parsed_arguments)

    return exit_code, stdout_buffer.getvalue(), stderr_buffer.getvalue()


def _render_help(arguments: list[str]) -> str:
    parser = cli_module._build_parser()
    stdout_buffer = StringIO()

    with pytest.raises(SystemExit) as captured_exit:
        with redirect_stdout(stdout_buffer):
            parser.parse_args([*arguments, "--help"])

    assert captured_exit.value.code == 0
    return stdout_buffer.getvalue()


def _normalize_help_output(help_output: str) -> str:
    return " ".join(help_output.split())


def _write_json_path(path: Path, payload: dict[str, object]) -> Path:
    path.write_text(json.dumps(payload, sort_keys=True), encoding="utf-8")
    return path


def _write_market_data_input_files(
    tmp_path: Path,
    *,
    calendar_registry_json: str,
    include_known_news: bool = False,
    include_option_surface: bool = False,
    ambiguous_tick_timestamp: bool = False,
    tick_trade_price_column_name: str = "trade_price",
) -> dict[str, Path]:
    data_directory = tmp_path / "data"
    config_directory = tmp_path / "config"

    data_directory.mkdir(parents=True, exist_ok=True)
    config_directory.mkdir(parents=True, exist_ok=True)

    tick_data_path = data_directory / "ticks.csv"
    if ambiguous_tick_timestamp:
        tick_data_path.write_text(
            "\n".join(
                [
                    "timestamp_utc",
                    "2026-11-01T01:30:00",
                ]
            ),
            encoding="utf-8",
        )
    else:
        tick_data_path.write_text(
            "\n".join(
                [
                    f"timestamp_utc,{tick_trade_price_column_name}",
                    "2026-02-02T14:30:00Z,100.0",
                    "2026-02-02T15:00:00Z,101.0",
                ]
            ),
            encoding="utf-8",
        )

    session_definition_path = config_directory / "sessions.json"
    session_definition_path.write_text(calendar_registry_json, encoding="utf-8")

    file_paths: dict[str, Path] = {
        "tick_data": tick_data_path,
        "session_definition": session_definition_path,
    }

    if include_known_news:
        known_news_path = config_directory / "known-news.csv"
        known_news_path.write_text(
            "\n".join(
                [
                    "event_timestamp_utc,event_code",
                    "2026-02-02T15:05:00Z,open_macro",
                ]
            ),
            encoding="utf-8",
        )
        file_paths["known_news"] = known_news_path

    if include_option_surface:
        option_surface_path = data_directory / "options.parquet"
        pyarrow_parquet.write_table(
            pa.table(
                {
                    "quote_timestamp_utc": [datetime(2026, 2, 2, 20, 0, tzinfo=UTC)],
                    "expiry_utc": [datetime(2026, 3, 20, 20, 0, tzinfo=UTC)],
                    "strike": [100.0],
                    "option_type": ["C"],
                    "bid": [4.0],
                    "ask": [5.0],
                }
            ),
            option_surface_path,
        )
        file_paths["option_surface"] = option_surface_path

    return file_paths


def _timezone_key_is_available(timezone_name: str) -> bool:
    try:
        ZoneInfo(timezone_name)
    except ZoneInfoNotFoundError:
        return False

    return True
