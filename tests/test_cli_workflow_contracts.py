import json
import sys
from contextlib import redirect_stderr, redirect_stdout
from datetime import UTC, datetime, timedelta
from io import StringIO
from pathlib import Path
from typing import cast
from zoneinfo import ZoneInfo, ZoneInfoNotFoundError

import orjson
import pyarrow as pa
import pytest
from pyarrow import parquet as pyarrow_parquet

import janiform.calibration as calibration_module
import janiform.cli as cli_module
import janiform.cli._commands as cli_commands_module
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


def test_seasonality_cli_help_documents_preprocessing_contract() -> None:
    help_output = _normalize_help_output(_render_help(["seasonality", "preprocess-timestamps"]))

    assert "--config-json PATH" in help_output
    assert "--output {human,json}" in help_output
    assert "--event-records-json PATH" in help_output
    assert "--calendar-registry-json PATH" in help_output
    assert "--exchange-code EXCHANGE_CODE" in help_output
    assert "--timestamp-field-name TIMESTAMP_FIELD_NAME" in help_output
    assert "--session-filter {all,open_only,closed_only}" in help_output
    assert "--sparse-hour-event-threshold SPARSE_HOUR_EVENT_THRESHOLD" in help_output
    assert "downstream seasonality or calibration workflows" in help_output


def test_pricing_cli_help_documents_execution_input_contract() -> None:
    help_output = _normalize_help_output(_render_help(["pricing", "vanilla-option"]))

    assert "--output {human,json}" in help_output
    assert "--execution-input-json PATH" in help_output
    assert cli_module.CLI_REPOSITORY_RELATIVE_PATH_POLICY in help_output


def test_stylized_facts_cli_help_documents_report_generation_contract() -> None:
    help_output = _normalize_help_output(_render_help(["stylized-facts", "horizon-comparison"]))

    assert "--output {human,json}" in help_output
    assert "--metric-catalog-json PATH" in help_output
    assert "--comparison-request-json PATH" in help_output
    assert "--artifact-export-json PATH" in help_output
    assert "--execution-mode {disk,memory}" in help_output
    assert cli_module.CLI_REPOSITORY_RELATIVE_PATH_POLICY in help_output


def test_calibration_cli_help_documents_mode_and_runtime_arguments() -> None:
    help_output = _normalize_help_output(_render_help(["calibration", "staged"]))

    assert "--config-json PATH" in help_output
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

    assert "--config-json PATH" in help_output
    assert "--output {human,json}" in help_output
    assert "--tick-data-path PATH" in help_output
    assert "--session-definition-path PATH" in help_output
    assert "--workflow-scope {seasonality_only,full_without_option_surface,full_with_option_surface}" in help_output
    assert "--tick-price-input-policy" in help_output
    assert "--execution-mode {disk,memory}" in help_output
    assert "market-data calibration summary" in help_output
    assert "preprocessing diagnostics and provenance" in help_output
    assert "explicit flags override environment variables" in help_output


def test_cli_shared_path_flags_use_standardized_repository_relative_help_text() -> None:
    help_outputs = (
        _normalize_help_output(_render_help(["simulation", "seeded-ticks"])),
        _normalize_help_output(_render_help(["seasonality", "preprocess-timestamps"])),
        _normalize_help_output(_render_help(["pricing", "vanilla-option"])),
        _normalize_help_output(_render_help(["stylized-facts", "horizon-comparison"])),
        _normalize_help_output(_render_help(["calibration", "market-data"])),
        _normalize_help_output(_render_help(["calibration", "staged"])),
        _normalize_help_output(_render_help(["dev", "regression", "deterministic"])),
        _normalize_help_output(_render_help(["dev", "regression", "open-closed-gap-leakage"])),
    )

    for help_output in help_outputs:
        assert cli_module.CLI_REPOSITORY_RELATIVE_PATH_POLICY in help_output


def test_deterministic_regression_cli_help_documents_required_inputs() -> None:
    help_output = _normalize_help_output(_render_help(["dev", "regression", "deterministic"]))

    assert "--output {human,json}" in help_output
    assert "--calendar-registry-json PATH" in help_output
    assert "--calendar-cases-json PATH" in help_output
    assert "--seasonality-cases-json PATH" in help_output
    assert "--seasonality-absolute-tolerance" in help_output
    assert "--news-intensity-absolute-tolerance" in help_output


def test_deterministic_regression_cli_parser_requires_case_inputs() -> None:
    parser = cli_module._build_parser()

    with pytest.raises(SystemExit) as captured_exit:
        parser.parse_args(
            [
                "dev",
                "regression",
                "deterministic",
                "--calendar-registry-json",
                "calendar.json",
            ]
        )

    assert captured_exit.value.code == 2


def test_deterministic_regression_cli_parser_defaults_output_to_human() -> None:
    parser = cli_module._build_parser()
    parsed_arguments = parser.parse_args(
        [
            "dev",
            "regression",
            "deterministic",
            "--calendar-registry-json",
            "calendar.json",
            "--calendar-cases-json",
            "calendar-cases.json",
            "--seasonality-cases-json",
            "seasonality-cases.json",
        ]
    )

    assert parsed_arguments.output == "human"


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


def test_seasonality_preprocess_cli_parser_uses_expected_defaults() -> None:
    parser = cli_module._build_parser()
    parsed_arguments = parser.parse_args(
        [
            "seasonality",
            "preprocess-timestamps",
            "--event-records-json",
            "events.json",
            "--calendar-registry-json",
            "calendar.json",
            "--exchange-code",
            "XNYS",
        ]
    )

    assert parsed_arguments.output == "json"
    assert parsed_arguments.timestamp_field_name == "timestamp"
    assert parsed_arguments.session_filter == "open_only"
    assert parsed_arguments.sparse_hour_event_threshold == 1


def test_seasonality_preprocess_cli_parser_rejects_invalid_session_filter() -> None:
    parser = cli_module._build_parser()

    with pytest.raises(SystemExit) as captured_exit:
        parser.parse_args(
            [
                "seasonality",
                "preprocess-timestamps",
                "--event-records-json",
                "events.json",
                "--calendar-registry-json",
                "calendar.json",
                "--exchange-code",
                "XNYS",
                "--session-filter",
                "intraday_only",
            ]
        )

    assert captured_exit.value.code == int(cli_module.CliExitCode.VALIDATION_ERROR)


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
    seasonality_arguments = parser.parse_args(
        [
            "seasonality",
            "preprocess-timestamps",
            "--event-records-json",
            "events.json",
            "--calendar-registry-json",
            "calendar.json",
            "--exchange-code",
            "XNYS",
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
    stylized_facts_arguments = parser.parse_args(
        [
            "stylized-facts",
            "horizon-comparison",
            "--metric-catalog-json",
            "metric-catalog.json",
            "--comparison-request-json",
            "comparison-request.json",
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
    assert seasonality_arguments.output == "json"
    assert pricing_arguments.output == "json"
    assert stylized_facts_arguments.output == "json"
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
    seasonality_arguments = parser.parse_args(
        [
            "seasonality",
            "preprocess-timestamps",
            "--event-records-json",
            "events.json",
            "--calendar-registry-json",
            "calendar.json",
            "--exchange-code",
            "XNYS",
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
    stylized_facts_arguments = parser.parse_args(
        [
            "stylized-facts",
            "horizon-comparison",
            "--metric-catalog-json",
            "metric-catalog.json",
            "--comparison-request-json",
            "comparison-request.json",
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
    assert seasonality_arguments.output == "human"
    assert pricing_arguments.output == "human"
    assert stylized_facts_arguments.output == "human"
    assert staged_arguments.output == "human"


def test_seasonality_preprocess_cli_maps_flags_to_canonical_payload(
    calendar_registry_json: str,
    tmp_path: Path,
) -> None:
    event_records_path = _write_event_records_document(
        tmp_path / "event-records.json",
        [
            {"timestamp": "2026-02-02T14:30:00Z"},
            {"timestamp": "2026-02-02T16:30:00Z"},
            {"timestamp": "2026-02-02T16:31:00Z"},
        ],
    )
    calendar_registry_path = _write_json_path(
        tmp_path / "calendar-registry.json",
        cast(dict[str, object], orjson.loads(calendar_registry_json)),
    )

    exit_code, stdout_text, stderr_text = _invoke_cli(
        [
            "seasonality",
            "preprocess-timestamps",
            "--event-records-json",
            str(event_records_path),
            "--calendar-registry-json",
            str(calendar_registry_path),
            "--exchange-code",
            "XNYS",
            "--session-filter",
            "all",
            "--sparse-hour-event-threshold",
            "2",
        ]
    )

    assert exit_code == int(cli_module.CliExitCode.SUCCESS), stderr_text

    payload = orjson.loads(stdout_text)
    assert payload["schema_version"] == "seasonality.timestamp_event.preprocessed.v1"
    assert payload["input_schema_version"] == "seasonality.timestamp_event.v1"
    assert payload["exchange_code"] == "XNYS"
    assert payload["exchange_timezone_name"] == "America/New_York"
    assert payload["source_record_positions"] == [0, 1, 2]
    assert payload["timestamp_utc_nanos"] == [
        _utc_nanos(2026, 2, 2, 14, 30, 0),
        _utc_nanos(2026, 2, 2, 16, 30, 0),
        _utc_nanos(2026, 2, 2, 16, 31, 0),
    ]
    assert payload["exchange_local_day_of_week"] == [1, 1, 1]
    assert payload["market_state_codes"] == [1, 1, 1]
    assert payload["accepted_record_count"] == 3
    assert payload["filtered_record_count"] == 0
    assert payload["missing_hour_bucket_utc_nanos"] == [_utc_nanos(2026, 2, 2, 15, 0, 0)]
    assert payload["sparse_hour_bucket_utc_nanos"] == [_utc_nanos(2026, 2, 2, 14, 0, 0)]
    assert payload["missing_hour_bucket_count"] == 1
    assert payload["sparse_hour_bucket_count"] == 1

    assert stdout_text == f"{json.dumps(payload, sort_keys=True)}\n"
    assert stderr_text == ""


def test_seasonality_preprocess_cli_supports_human_output_mode(
    calendar_registry_json: str,
    tmp_path: Path,
) -> None:
    event_records_path = _write_event_records_document(
        tmp_path / "event-records-human.json",
        [{"timestamp": "2026-02-02T14:30:00Z"}],
    )
    calendar_registry_path = _write_json_path(
        tmp_path / "calendar-registry-human.json",
        cast(dict[str, object], orjson.loads(calendar_registry_json)),
    )

    exit_code, stdout_text, stderr_text = _invoke_cli(
        [
            "seasonality",
            "preprocess-timestamps",
            "--event-records-json",
            str(event_records_path),
            "--calendar-registry-json",
            str(calendar_registry_path),
            "--exchange-code",
            "XNYS",
            "--output",
            "human",
        ]
    )

    assert exit_code == int(cli_module.CliExitCode.SUCCESS), stderr_text
    assert stdout_text.startswith("seasonality preprocess-timestamps completed:")
    assert "accepted_record_count=1" in stdout_text
    assert "coverage_diagnostics:" in stdout_text
    assert stderr_text == ""


def test_seasonality_preprocess_cli_unknown_exchange_returns_validation_error(
    calendar_registry_json: str,
    tmp_path: Path,
) -> None:
    event_records_path = _write_event_records_document(
        tmp_path / "event-records-unknown-exchange.json",
        [{"timestamp": "2026-02-02T14:30:00Z"}],
    )
    calendar_registry_path = _write_json_path(
        tmp_path / "calendar-registry-unknown-exchange.json",
        cast(dict[str, object], orjson.loads(calendar_registry_json)),
    )

    exit_code, stdout_text, stderr_text = _invoke_cli(
        [
            "seasonality",
            "preprocess-timestamps",
            "--event-records-json",
            str(event_records_path),
            "--calendar-registry-json",
            str(calendar_registry_path),
            "--exchange-code",
            "XBAD",
        ]
    )

    assert exit_code == int(cli_module.CliExitCode.VALIDATION_ERROR)
    assert stdout_text == ""
    assert '[cli.validation.seasonality.preprocess_timestamps.invalid_input]' in stderr_text
    assert 'field_path="exchange_code"' in stderr_text


def test_seasonality_preprocess_cli_missing_timestamp_field_returns_validation_error(
    calendar_registry_json: str,
    tmp_path: Path,
) -> None:
    event_records_path = _write_event_records_document(
        tmp_path / "event-records-missing-timestamp.json",
        [{"event_id": "a"}],
    )
    calendar_registry_path = _write_json_path(
        tmp_path / "calendar-registry-missing-timestamp.json",
        cast(dict[str, object], orjson.loads(calendar_registry_json)),
    )

    exit_code, stdout_text, stderr_text = _invoke_cli(
        [
            "seasonality",
            "preprocess-timestamps",
            "--event-records-json",
            str(event_records_path),
            "--calendar-registry-json",
            str(calendar_registry_path),
            "--exchange-code",
            "XNYS",
        ]
    )

    assert exit_code == int(cli_module.CliExitCode.VALIDATION_ERROR)
    assert stdout_text == ""
    assert '[cli.validation.seasonality.preprocess_timestamps.invalid_input]' in stderr_text
    assert 'field_path="event_records_document"' in stderr_text
    assert "missing required field 'timestamp'" in stderr_text


def test_seasonality_preprocess_cli_invalid_timestamp_returns_validation_error(
    calendar_registry_json: str,
    tmp_path: Path,
) -> None:
    event_records_path = _write_event_records_document(
        tmp_path / "event-records-invalid-timestamp.json",
        [{"timestamp": "not-a-timestamp"}],
    )
    calendar_registry_path = _write_json_path(
        tmp_path / "calendar-registry-invalid-timestamp.json",
        cast(dict[str, object], orjson.loads(calendar_registry_json)),
    )

    exit_code, stdout_text, stderr_text = _invoke_cli(
        [
            "seasonality",
            "preprocess-timestamps",
            "--event-records-json",
            str(event_records_path),
            "--calendar-registry-json",
            str(calendar_registry_path),
            "--exchange-code",
            "XNYS",
        ]
    )

    assert exit_code == int(cli_module.CliExitCode.VALIDATION_ERROR)
    assert stdout_text == ""
    assert '[cli.validation.seasonality.preprocess_timestamps.invalid_input]' in stderr_text
    assert 'field_path="event_records_document"' in stderr_text
    assert "invalid timestamp" in stderr_text


def test_seasonality_preprocess_cli_rejects_malformed_event_records_document(
    calendar_registry_json: str,
    tmp_path: Path,
) -> None:
    event_records_path = _write_json_path(
        tmp_path / "event-records-malformed.json",
        {"event_records": ["not-an-object"]},
    )
    calendar_registry_path = _write_json_path(
        tmp_path / "calendar-registry-malformed.json",
        cast(dict[str, object], orjson.loads(calendar_registry_json)),
    )

    exit_code, stdout_text, stderr_text = _invoke_cli(
        [
            "seasonality",
            "preprocess-timestamps",
            "--event-records-json",
            str(event_records_path),
            "--calendar-registry-json",
            str(calendar_registry_path),
            "--exchange-code",
            "XNYS",
        ]
    )

    assert exit_code == int(cli_module.CliExitCode.VALIDATION_ERROR)
    assert stdout_text == ""
    assert '[cli.validation.seasonality.preprocess_timestamps.invalid_input]' in stderr_text
    assert 'field_path="event_records_document"' in stderr_text
    assert "event records document record must be an object" in stderr_text


def test_seasonality_preprocess_cli_invalid_calendar_timezone_returns_validation_error(
    calendar_registry_json: str,
    tmp_path: Path,
) -> None:
    calendar_registry_payload = cast(dict[str, object], orjson.loads(calendar_registry_json))
    exchanges = cast(list[dict[str, object]], calendar_registry_payload["exchanges"])
    exchanges[0]["timezone_name"] = "Mars/Phobos"
    event_records_path = _write_event_records_document(
        tmp_path / "event-records-invalid-timezone.json",
        [{"timestamp": "2026-02-02T14:30:00Z"}],
    )
    calendar_registry_path = _write_json_path(
        tmp_path / "calendar-registry-invalid-timezone.json",
        calendar_registry_payload,
    )

    exit_code, stdout_text, stderr_text = _invoke_cli(
        [
            "seasonality",
            "preprocess-timestamps",
            "--event-records-json",
            str(event_records_path),
            "--calendar-registry-json",
            str(calendar_registry_path),
            "--exchange-code",
            "XNYS",
        ]
    )

    assert exit_code == int(cli_module.CliExitCode.VALIDATION_ERROR)
    assert stdout_text == ""
    assert '[cli.validation.seasonality.preprocess_timestamps.invalid_input]' in stderr_text
    assert 'field_path="calendar_registry_document"' in stderr_text
    assert "timezone" in stderr_text.lower()


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


def test_deterministic_regression_cli_runs_fixture_backed_harness(
    calendar_regression_fixture_path: Path,
    hourly_factor_regression_fixture_path: Path,
) -> None:
    exit_code, stdout_text, stderr_text = _invoke_cli(
        [
            "dev",
            "regression",
            "deterministic",
            "--calendar-registry-json",
            str(calendar_regression_fixture_path),
            "--calendar-cases-json",
            str(calendar_regression_fixture_path),
            "--seasonality-cases-json",
            str(hourly_factor_regression_fixture_path),
            "--output",
            "json",
        ]
    )

    assert exit_code == int(cli_module.CliExitCode.SUCCESS), stderr_text

    payload = orjson.loads(stdout_text)
    assert payload["schema_version"] == "validation.deterministic_regression_harness.report.v1"
    assert payload["summary"]["failed_case_count"] == 0
    assert stderr_text == ""


def test_deterministic_regression_cli_rejects_malformed_calendar_cases_document(
    calendar_regression_fixture_path: Path,
    hourly_factor_regression_fixture_path: Path,
    tmp_path: Path,
) -> None:
    malformed_calendar_cases_path = _write_json_path(
        tmp_path / "malformed-calendar-cases.json",
        {
            "schema_version": "calendar.service.regression.v1",
            "calendar_registry": {"schema_version": "calendar.registry.v1", "exchanges": []},
            "cases": ["not-a-case-object"],
        },
    )

    exit_code, stdout_text, stderr_text = _invoke_cli(
        [
            "dev",
            "regression",
            "deterministic",
            "--calendar-registry-json",
            str(calendar_regression_fixture_path),
            "--calendar-cases-json",
            str(malformed_calendar_cases_path),
            "--seasonality-cases-json",
            str(hourly_factor_regression_fixture_path),
        ]
    )

    assert exit_code == int(cli_module.CliExitCode.VALIDATION_ERROR)
    assert stdout_text == ""
    assert '[cli.validation.deterministic_regression.invalid_input]' in stderr_text
    assert 'field_path="calendar_cases_document"' in stderr_text
    assert 'field_path="seasonality_cases_document"' not in stderr_text
    assert "calendar cases document case must be an object" in stderr_text


def test_deterministic_regression_cli_rejects_malformed_seasonality_cases_document(
    calendar_regression_fixture_path: Path,
    hourly_factor_regression_fixture_path: Path,
    tmp_path: Path,
) -> None:
    malformed_seasonality_cases_document = orjson.loads(
        hourly_factor_regression_fixture_path.read_text(encoding="utf-8")
    )
    malformed_seasonality_cases_document["canonical_output_cases"][0]["seasonality_source"][
        "source_timestamp_utc"
    ] = malformed_seasonality_cases_document["canonical_output_cases"][0]["seasonality_source"].pop(
        "source_timestamps_utc"
    )
    malformed_seasonality_cases_path = _write_json_path(
        tmp_path / "malformed-seasonality-cases.json",
        malformed_seasonality_cases_document,
    )

    exit_code, stdout_text, stderr_text = _invoke_cli(
        [
            "dev",
            "regression",
            "deterministic",
            "--calendar-registry-json",
            str(calendar_regression_fixture_path),
            "--calendar-cases-json",
            str(calendar_regression_fixture_path),
            "--seasonality-cases-json",
            str(malformed_seasonality_cases_path),
        ]
    )

    assert exit_code == int(cli_module.CliExitCode.VALIDATION_ERROR)
    assert stdout_text == ""
    assert '[cli.validation.deterministic_regression.invalid_input]' in stderr_text
    assert 'field_path="seasonality_cases_document"' in stderr_text
    assert "source_timestamps_utc" in stderr_text


def test_deterministic_regression_cli_missing_input_file_returns_validation_error(
    calendar_regression_fixture_path: Path,
    tmp_path: Path,
) -> None:
    missing_seasonality_path = tmp_path / "missing-seasonality-cases.json"

    exit_code, stdout_text, stderr_text = _invoke_cli(
        [
            "dev",
            "regression",
            "deterministic",
            "--calendar-registry-json",
            str(calendar_regression_fixture_path),
            "--calendar-cases-json",
            str(calendar_regression_fixture_path),
            "--seasonality-cases-json",
            str(missing_seasonality_path),
        ]
    )

    assert exit_code == int(cli_module.CliExitCode.VALIDATION_ERROR)
    assert stdout_text == ""
    assert '[cli.validation.deterministic_regression.invalid_input]' in stderr_text
    assert 'field_path="seasonality_cases_json"' in stderr_text
    assert '"path": ' in stderr_text


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
    assert summary_payload["preparation_schema_version"] == calibration_module.CALIBRATION_MARKET_DATA_PREPARATION_SCHEMA_VERSION
    assert summary_payload["workflow_scope"] == CalibrationMarketDataWorkflowScope.FULL_WITH_OPTION_SURFACE.value
    assert summary_payload["session_context"]["exchange_code"] == "XNYS"
    assert Path(summary_payload["input_files"]["tick_data"]["path"]).parts[-2:] == ("data", "ticks.csv")
    assert summary_payload["input_files"]["option_surface"]["file_format"] == "parquet"
    assert summary_payload["column_mapping"]["known_news"]["event_timestamp_column"] == "event_timestamp_utc"
    assert summary_payload["output_artifact_locations"]["disk_checkpoint_directory"] == str(checkpoint_directory)
    assert summary_payload["output_emission_policy"]["execution_mode"] == "disk"
    assert summary_payload["output_emission_policy"]["emit_derived_staged_execution_input"] is True
    assert summary_payload["diagnostics_summary"]["selected_stages"] == [
        "seasonality",
        "hawkes",
        "return_components",
    ]
    assert "known_news_overlay_derivation" in summary_payload["diagnostics_summary"]["executed_preprocessing_steps"]
    assert summary_payload["provenance_summary"]["tick_data_summary"]["file_format"] == "csv"
    assert summary_payload["provenance_summary"]["option_surface_summary"]["file_format"] == "parquet"
    assert summary_payload["provenance_summary"]["known_news_summary"]["file_format"] == "csv"
    assert summary_payload["output_schema_metadata"]["field_selection_policy"] == (
        cli_module.CLI_MARKET_DATA_SUMMARY_FIELD_SELECTION_POLICY
    )

    assert "request" not in summary_payload
    assert "runtime_resources" not in summary_payload
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
    assert "workflow_diagnostics:" in stdout_text
    assert "loaded_data_summary:" in stdout_text
    assert "derived_references:" in stdout_text
    assert stderr_text == ""


def test_market_data_calibration_cli_supports_config_driven_non_interactive_execution(
    calendar_registry_json: str,
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> None:
    market_data_paths = _write_market_data_input_files(
        tmp_path,
        calendar_registry_json=calendar_registry_json,
        include_known_news=True,
    )

    checkpoint_directory = tmp_path / "config-checkpoints"

    config_path = _write_cli_command_config(
        tmp_path / "market_data_config.json",
        command="calibration.market-data",
        arguments={
            "run-id": "market-data-config-run",
            "workflow-scope": "seasonality_only",
            "tick-data-path": str(market_data_paths["tick_data"]),
            "tick-data-format": "csv",
            "tick-data-provenance-label": "ticks-primary",
            "tick-timestamp-column": "timestamp_utc",
            "exchange-code": "XNYS",
            "session-definition-path": str(market_data_paths["session_definition"]),
            "session-definition-format": "json",
            "session-definition-provenance-label": "session-calendar",
            "session-timezone": "America/New_York",
            "known-news-path": str(market_data_paths["known_news"]),
            "known-news-format": "csv",
            "known-news-provenance-label": "known-news",
            "known-news-timezone": "UTC",
            "known-news-timestamp-column": "event_timestamp_utc",
            "known-news-event-id-column": "event_code",
            "timestamp-timezone": "UTC",
            "execution-mode": "disk",
            "disk-checkpoint-directory": str(checkpoint_directory),
            "output": "json",
        },
    )

    monkeypatch.setattr(sys, "stdin", _ExplodingStdin())
    exit_code, stdout_text, stderr_text = _invoke_cli(
        [
            "calibration",
            "market-data",
            "--config-json",
            str(config_path),
        ]
    )

    assert exit_code == int(cli_module.CliExitCode.SUCCESS), stderr_text
    assert stderr_text == ""

    summary_payload = orjson.loads(stdout_text)
    assert summary_payload["run_id"] == "market-data-config-run"
    assert summary_payload["input_files"]["known_news"]["file_format"] == "csv"
    assert summary_payload["output_emission_policy"]["execution_mode"] == "disk"
    assert summary_payload["output_artifact_locations"]["disk_checkpoint_directory"] == str(checkpoint_directory)


def test_market_data_calibration_cli_environment_overrides_command_config_defaults(
    calendar_registry_json: str,
    tmp_path: Path,
) -> None:
    market_data_paths = _write_market_data_input_files(
        tmp_path,
        calendar_registry_json=calendar_registry_json,
    )

    environment_checkpoint_directory = tmp_path / "environment-checkpoints"

    config_path = _write_cli_command_config(
        tmp_path / "market_data_precedence_config.json",
        command="calibration.market-data",
        arguments={
            "run-id": "market-data-env-override-run",
            "workflow-scope": "seasonality_only",
            "tick-data-path": str(market_data_paths["tick_data"]),
            "tick-data-format": "csv",
            "tick-data-provenance-label": "ticks-primary",
            "tick-timestamp-column": "timestamp_utc",
            "exchange-code": "XNYS",
            "session-definition-path": str(market_data_paths["session_definition"]),
            "session-definition-format": "json",
            "session-definition-provenance-label": "session-calendar",
            "session-timezone": "America/New_York",
            "timestamp-timezone": "UTC",
            "execution-mode": "memory",
            "output": "human",
        },
    )

    exit_code, stdout_text, stderr_text = _invoke_cli(
        [
            "calibration",
            "market-data",
            "--config-json",
            str(config_path),
        ],
        environment={
            cli_module.CLI_OUTPUT_ENV_VAR: "json",
            cli_module.CLI_CALIBRATION_EXECUTION_MODE_ENV_VAR: "disk",
            cli_module.CLI_CALIBRATION_DISK_CHECKPOINT_DIRECTORY_ENV_VAR: str(environment_checkpoint_directory),
        },
    )

    assert exit_code == int(cli_module.CliExitCode.SUCCESS), stderr_text
    assert stderr_text == ""

    summary_payload = orjson.loads(stdout_text)
    assert summary_payload["output_emission_policy"]["execution_mode"] == "disk"
    assert summary_payload["output_artifact_locations"]["disk_checkpoint_directory"] == str(
        environment_checkpoint_directory
    )


def test_cli_explicit_flags_override_environment_defaults(
    calendar_registry_json: str,
    tmp_path: Path,
) -> None:
    market_data_paths = _write_market_data_input_files(
        tmp_path,
        calendar_registry_json=calendar_registry_json,
    )

    config_path = _write_cli_command_config(
        tmp_path / "market_data_output_config.json",
        command="calibration.market-data",
        arguments={
            "run-id": "market-data-cli-override-run",
            "workflow-scope": "seasonality_only",
            "tick-data-path": str(market_data_paths["tick_data"]),
            "tick-data-format": "csv",
            "tick-data-provenance-label": "ticks-primary",
            "tick-timestamp-column": "timestamp_utc",
            "exchange-code": "XNYS",
            "session-definition-path": str(market_data_paths["session_definition"]),
            "session-definition-format": "json",
            "session-definition-provenance-label": "session-calendar",
            "session-timezone": "America/New_York",
            "timestamp-timezone": "UTC",
            "execution-mode": "disk",
            "disk-checkpoint-directory": str(tmp_path / "cli-override-checkpoints"),
        },
    )

    exit_code, stdout_text, stderr_text = _invoke_cli(
        [
            "calibration",
            "market-data",
            "--config-json",
            str(config_path),
            "--output",
            "human",
        ],
        environment={cli_module.CLI_OUTPUT_ENV_VAR: "json"},
    )

    assert exit_code == int(cli_module.CliExitCode.SUCCESS), stderr_text
    assert stdout_text.startswith("calibration market-data request prepared:")
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
    assert 'field_path="session_configuration.session_definition_source.path"' in stderr_text
    assert "valid calendar registry" in stderr_text


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

    monkeypatch.setattr(cli_commands_module, "run_seeded_tick_event_simulation", capturing_runner)

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

    monkeypatch.setattr(cli_commands_module, "run_vanilla_option_pricing", capturing_runner)

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

    monkeypatch.setattr(cli_commands_module, "run_staged_calibration", capturing_runner)

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

    monkeypatch.setattr(cli_commands_module, "run_staged_calibration", capturing_runner)

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
    repository_root = Path(cli_module.__file__).resolve().parents[3]
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

    monkeypatch.setattr(cli_commands_module, "run_staged_calibration", failing_runner)

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

    monkeypatch.setattr(cli_commands_module, "run_vanilla_option_pricing", broken_runner)

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


def _invoke_cli(
    arguments: list[str],
    *,
    environment: dict[str, str] | None = None,
) -> tuple[int, str, str]:
    parsed_arguments = cli_module._parse_cli_arguments(
        arguments,
        environment={} if environment is None else environment,
    )

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


def _write_event_records_document(path: Path, event_records: list[dict[str, object]]) -> Path:
    return _write_json_path(
        path,
        {"event_records": event_records},
    )


def _write_cli_command_config(
    path: Path,
    *,
    command: str,
    arguments: dict[str, object],
) -> Path:
    return _write_json_path(
        path,
        {
            "schema_version": cli_module.CLI_COMMAND_CONFIG_SCHEMA_VERSION,
            "command": command,
            "arguments": arguments,
        },
    )


def _write_market_data_input_files(
    tmp_path: Path,
    *,
    calendar_registry_json: str,
    include_known_news: bool = False,
    include_option_surface: bool = False,
    ambiguous_tick_timestamp: bool = False,
    tick_trade_price_column_name: str = "trade_price",
    tick_data_format: str = "csv",
) -> dict[str, Path]:
    data_directory = tmp_path / "data"
    config_directory = tmp_path / "config"

    data_directory.mkdir(parents=True, exist_ok=True)
    config_directory.mkdir(parents=True, exist_ok=True)

    tick_data_path = data_directory / f"ticks.{tick_data_format}"
    if tick_data_format == "csv":
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
                        "2026-02-02T20:59:00Z,102.0",
                        "2026-02-04T14:30:00Z,95.0",
                    ]
                ),
                encoding="utf-8",
            )
    else:
        pyarrow_parquet.write_table(
            pa.table(
                {
                    "timestamp_utc": [
                        datetime(2026, 2, 2, 14, 30, tzinfo=UTC),
                        datetime(2026, 2, 2, 15, 0, tzinfo=UTC),
                        datetime(2026, 2, 2, 20, 59, tzinfo=UTC),
                        datetime(2026, 2, 4, 14, 30, tzinfo=UTC),
                    ],
                    tick_trade_price_column_name: [100.0, 101.0, 102.0, 95.0],
                }
            ),
            tick_data_path,
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
                    "2026-02-02T22:00:00Z,closed_macro",
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


def _utc_nanos(
    year: int,
    month: int,
    day: int,
    hour: int,
    minute: int,
    second: int,
) -> int:
    return int(datetime(year, month, day, hour, minute, second, tzinfo=UTC).timestamp() * 1_000_000_000)


def _timezone_key_is_available(timezone_name: str) -> bool:
    try:
        ZoneInfo(timezone_name)
    except ZoneInfoNotFoundError:
        return False

    return True


class _ExplodingStdin:
    encoding = "utf-8"

    def isatty(self) -> bool:
        raise AssertionError("CLI should not inspect stdin for automation-friendly commands")

    def read(self, *_args: object, **_kwargs: object) -> str:
        raise AssertionError("CLI should not read stdin for automation-friendly commands")

    def readline(self, *_args: object, **_kwargs: object) -> str:
        raise AssertionError("CLI should not read stdin for automation-friendly commands")
