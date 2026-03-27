import json
import sys
from datetime import UTC, datetime
from pathlib import Path
from typing import Any, cast
from zoneinfo import ZoneInfo, ZoneInfoNotFoundError

import orjson
import pytest

import janiform.calibration as calibration_module
import janiform.cli as cli_module
import janiform.cli.commands.calibration as cli_calibration_commands_module
import janiform.cli.paths as cli_paths_module
import janiform.cli.commands._pricing as cli_pricing_commands_module
import janiform.cli.commands.simulation as cli_simulation_commands_module
import janiform.pricing as pricing_module
import janiform.simulation as simulation_module
from janiform.calendar import CalendarRegistry
from janiform.calibration.market_data import (
    CALIBRATION_MARKET_DATA_PREPARATION_SCHEMA_VERSION,
    CALIBRATION_MARKET_DATA_REQUEST_SCHEMA_VERSION,
    CalibrationMarketDataWorkflowScope,
)
from janiform.calibration.staged import (
    CalibrationExecutionMode,
    StagedCalibrationExecutionInput,
    StagedCalibrationResponse,
)
from janiform.errors import JaniformErrorCategory, JaniformErrorDetails, JaniformExecutionError
from janiform.simulation import (
    CoarseSimulationRunRequest,
    SeededTickSimulationRequest,
)
from tests.calibration_test_support import (
    build_constant_mean_reversion_fitted_parameters_payload,
    build_valid_execution_input_model,
    update_model_mean_reversion_payload,
)
from tests.cli_test_support import (
    build_coarse_simulation_request as _build_coarse_simulation_request,
    build_simulation_request as _build_simulation_request,
    invoke_cli as _invoke_cli,
    normalize_help_output as _normalize_help_output,
    render_help as _render_help,
    write_json_path as _write_json_path,
    write_market_data_input_files as _write_market_data_input_files,
)
from tests.test_cli_high_level_commands import _build_full_workflow_model


def test_simulation_cli_help_documents_required_contract_inputs() -> None:
    help_output = _normalize_help_output(_render_help(["simulate", "seeded-ticks"]))

    assert "--output {human,json}" in help_output
    assert "--request-json PATH" in help_output
    assert "--calendar-registry-json PATH" in help_output
    assert cli_module.CLI_REPOSITORY_RELATIVE_PATH_POLICY in help_output


def test_coarse_simulation_cli_help_documents_required_contract_inputs() -> None:
    help_output = _normalize_help_output(_render_help(["simulate", "coarse-interval"]))

    assert "--config-json PATH" in help_output
    assert "--output {human,json}" in help_output
    assert "--request-json PATH" in help_output
    assert "without replaying fine-scale seeded ticks" in help_output
    assert "Use seeded-ticks when you need a priced fine path or tick replay." in help_output
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


def test_seasonality_cli_help_documents_diagnostics_contract() -> None:
    help_output = _normalize_help_output(_render_help(["seasonality", "diagnostics"]))

    assert "--config-json PATH" in help_output
    assert "--output {human,json}" in help_output
    assert "--event-records-json PATH" in help_output
    assert "--calendar-registry-json PATH" in help_output
    assert "--dataset-id DATASET_ID" in help_output
    assert "--exchange-code EXCHANGE_CODE" in help_output
    assert "--intraday-minutes-per-bin INTRADAY_MINUTES_PER_BIN" in help_output
    assert "--intraday-smoothing-kernel {gaussian,uniform}" in help_output
    assert "--day-of-week-minimum-bucket-events DAY_OF_WEEK_MINIMUM_BUCKET_EVENTS" in help_output
    assert "--day-of-year-minimum-bucket-events DAY_OF_YEAR_MINIMUM_BUCKET_EVENTS" in help_output
    assert "canonical diagnostics report" in help_output


def test_price_cli_help_documents_model_driven_workflow_contract() -> None:
    help_output = _normalize_help_output(_render_help(["price"]))

    assert "--output {human,json}" in help_output
    assert "--model-path PATH" in help_output
    assert "--observed-boundary-state-json PATH" in help_output
    assert "--valuation-time-utc VALUATION_TIME_UTC" in help_output
    assert "--spot SPOT" in help_output
    assert "--strike STRIKE" in help_output
    assert "--maturity-interval MATURITY_INTERVAL" in help_output
    assert "--output-path PATH" in help_output
    assert cli_module.CLI_REPOSITORY_RELATIVE_PATH_POLICY in help_output


def test_stylized_facts_cli_help_documents_report_generation_contract() -> None:
    help_output = _normalize_help_output(_render_help(["stylized-facts", "horizon-comparison"]))

    assert "--output {human,json}" in help_output
    assert "--metric-catalog-json PATH" in help_output
    assert "--comparison-request-json PATH" in help_output
    assert "--artifact-export-json PATH" in help_output
    assert "--execution-mode {disk,memory}" in help_output
    assert cli_module.CLI_REPOSITORY_RELATIVE_PATH_POLICY in help_output


def test_fit_staged_cli_help_documents_mode_and_runtime_arguments() -> None:
    help_output = _normalize_help_output(_render_help(["fit", "staged"]))

    assert "--config-json PATH" in help_output
    assert "--output {human,json}" in help_output
    assert "--execution-input-json PATH" in help_output
    assert "--calendar-registry-json PATH" in help_output
    assert "--execution-mode {disk,memory}" in help_output
    assert "Precedence: --execution-mode, then the CLI default disk." in help_output
    assert "overrides request.execution_mode" in help_output
    assert "--disk-checkpoint-directory DIRECTORY" in help_output
    assert "overrides runtime_resources.disk_checkpoint_directory from --execution-input-json" in help_output


def test_fit_plan_cli_help_documents_user_facing_file_inputs() -> None:
    help_output = _normalize_help_output(_render_help(["fit", "plan"]))

    assert "--config-json PATH" in help_output
    assert "--output {human,json}" in help_output
    assert "--tick-data-path PATH" in help_output
    assert "--session-definition-path PATH" in help_output
    assert "--workflow-scope {seasonality_only,full_without_option_surface,full_with_option_surface}" in help_output
    assert "--tick-price-input-policy" in help_output
    assert "--execution-mode {disk,memory}" in help_output
    assert "derived fit plan" in help_output
    assert "without running the staged fit" in help_output
    assert "explicit flags override environment variables" in help_output


def test_cli_shared_path_flags_use_standardized_repository_relative_help_text() -> None:
    help_outputs = (
        _normalize_help_output(_render_help(["simulate", "coarse-interval"])),
        _normalize_help_output(_render_help(["simulate", "seeded-ticks"])),
        _normalize_help_output(_render_help(["seasonality", "diagnostics"])),
        _normalize_help_output(_render_help(["seasonality", "preprocess-timestamps"])),
        _normalize_help_output(_render_help(["hourly-factors", "materialize-seasonality"])),
        _normalize_help_output(_render_help(["hourly-factors", "materialize-news-intensity"])),
        _normalize_help_output(_render_help(["hourly-factors", "build-contract"])),
        _normalize_help_output(_render_help(["price"])),
        _normalize_help_output(_render_help(["stylized-facts", "horizon-comparison"])),
        _normalize_help_output(_render_help(["fit", "plan"])),
        _normalize_help_output(_render_help(["fit", "staged"])),
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


def test_deterministic_regression_cli_requires_case_inputs() -> None:
    exit_code, _, _ = _invoke_cli(
        [
            "dev",
            "regression",
            "deterministic",
            "--calendar-registry-json",
            "calendar.json",
        ]
    )

    assert exit_code == 2


def test_coarse_simulation_cli_requires_request_json() -> None:
    exit_code, _, _ = _invoke_cli(["simulate", "coarse-interval"])

    assert exit_code == 2


def test_market_data_calibration_cli_accepts_automatic_session_inference_without_override(
    tmp_path: Path,
) -> None:
    tick_data_path = tmp_path / "ticks.csv"
    tick_data_path.write_text(
        "\n".join(
            [
                "timestamp_utc",
                "2026-02-02T14:30:00Z",
                "2026-02-02T15:00:00Z",
                "2026-02-03T14:30:00Z",
            ]
        ),
        encoding="utf-8",
    )

    exit_code, stdout_text, stderr_text = _invoke_cli(
        [
            "fit",
            "plan",
            "--run-id",
            "market-data-auto-session-run",
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
            "--timestamp-timezone",
            "UTC",
            "--execution-mode",
            "memory",
        ]
    )

    assert exit_code == int(cli_module.CliExitCode.SUCCESS), stderr_text

    summary_payload = orjson.loads(stdout_text)
    assert summary_payload["input_files"]["session_definition"] is None
    assert summary_payload["session_resolution"]["resolution_kind"] == "automatic_inference"
    assert summary_payload["session_context"]["session_timezone"] == "America/New_York"


def test_seasonality_preprocess_cli_rejects_invalid_session_filter() -> None:
    exit_code, _, _ = _invoke_cli(
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

    assert exit_code == int(cli_module.CliExitCode.VALIDATION_ERROR)


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
        cast(dict[str, Any], orjson.loads(calendar_registry_json)),
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
        cast(dict[str, Any], orjson.loads(calendar_registry_json)),
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


def test_seasonality_diagnostics_cli_generates_canonical_report_from_event_records(
    calendar_registry_json: str,
    tmp_path: Path,
) -> None:
    event_records_path = _write_event_records_document(
        tmp_path / "seasonality-diagnostics-events.json",
        _seasonality_diagnostics_event_records(),
    )
    calendar_registry_path = _write_json_path(
        tmp_path / "seasonality-diagnostics-calendar.json",
        cast(dict[str, Any], orjson.loads(calendar_registry_json)),
    )

    exit_code, stdout_text, stderr_text = _invoke_cli(
        [
            "seasonality",
            "diagnostics",
            "--dataset-id",
            "dataset-alpha",
            "--event-records-json",
            str(event_records_path),
            "--calendar-registry-json",
            str(calendar_registry_path),
            "--exchange-code",
            "XNYS",
            "--session-filter",
            "all",
            "--intraday-minutes-per-bin",
            "30",
            "--intraday-smoothing-kernel",
            "uniform",
            "--intraday-smoothing-bandwidth-bins",
            "1.5",
            "--day-of-week-minimum-bucket-events",
            "2",
            "--day-of-year-minimum-bucket-events",
            "2",
        ]
    )

    assert exit_code == int(cli_module.CliExitCode.SUCCESS), stderr_text

    payload = orjson.loads(stdout_text)
    assert payload["schema_version"] == "seasonality.diagnostics.report.v1"
    assert payload["dataset_id"] == "dataset-alpha"
    assert payload["intraday_component_summary"]["component_name"] == "intraday"
    assert payload["day_of_week_component_summary"]["component_name"] == "day_of_week"
    assert payload["day_of_year_component_summary"]["component_name"] == "day_of_year"
    assert payload["reproducibility_metadata"]["estimator_version"] == "seasonality_estimator.v1"
    assert payload["fit_quality_metrics"]["combined_factor_mean"] > 0.0

    assert stdout_text == f"{json.dumps(payload, sort_keys=True)}\n"
    assert stderr_text == ""


def test_seasonality_diagnostics_cli_supports_human_output_mode(
    calendar_registry_json: str,
    tmp_path: Path,
) -> None:
    event_records_path = _write_event_records_document(
        tmp_path / "seasonality-diagnostics-human-events.json",
        _seasonality_diagnostics_event_records(),
    )
    calendar_registry_path = _write_json_path(
        tmp_path / "seasonality-diagnostics-human-calendar.json",
        cast(dict[str, Any], orjson.loads(calendar_registry_json)),
    )

    exit_code, stdout_text, stderr_text = _invoke_cli(
        [
            "seasonality",
            "diagnostics",
            "--dataset-id",
            "dataset-alpha",
            "--event-records-json",
            str(event_records_path),
            "--calendar-registry-json",
            str(calendar_registry_path),
            "--exchange-code",
            "XNYS",
            "--session-filter",
            "all",
            "--output",
            "human",
        ]
    )

    assert exit_code == int(cli_module.CliExitCode.SUCCESS), stderr_text
    assert stdout_text.startswith("seasonality diagnostics completed:")
    assert "fit_quality:" in stdout_text
    assert "component_sparsity:" in stdout_text
    assert "reproducibility:" in stdout_text
    assert stderr_text == ""


def test_seasonality_diagnostics_cli_invalid_estimator_setting_returns_validation_error(
    calendar_registry_json: str,
    tmp_path: Path,
) -> None:
    event_records_path = _write_event_records_document(
        tmp_path / "seasonality-diagnostics-invalid-estimator-events.json",
        _seasonality_diagnostics_event_records(),
    )
    calendar_registry_path = _write_json_path(
        tmp_path / "seasonality-diagnostics-invalid-estimator-calendar.json",
        cast(dict[str, Any], orjson.loads(calendar_registry_json)),
    )

    exit_code, stdout_text, stderr_text = _invoke_cli(
        [
            "seasonality",
            "diagnostics",
            "--dataset-id",
            "dataset-alpha",
            "--event-records-json",
            str(event_records_path),
            "--calendar-registry-json",
            str(calendar_registry_path),
            "--exchange-code",
            "XNYS",
            "--intraday-minutes-per-bin",
            "0",
        ]
    )

    assert exit_code == int(cli_module.CliExitCode.VALIDATION_ERROR)
    assert stdout_text == ""
    assert '[cli.validation.seasonality.diagnostics.invalid_input]' in stderr_text
    assert 'field_path="intraday_minutes_per_bin"' in stderr_text
    assert 'message="must be strictly positive"' in stderr_text


def test_seasonality_diagnostics_cli_rejects_malformed_event_records_document(
    calendar_registry_json: str,
    tmp_path: Path,
) -> None:
    event_records_path = _write_json_path(
        tmp_path / "seasonality-diagnostics-malformed-events.json",
        {"event_records": ["not-an-object"]},
    )
    calendar_registry_path = _write_json_path(
        tmp_path / "seasonality-diagnostics-malformed-calendar.json",
        cast(dict[str, Any], orjson.loads(calendar_registry_json)),
    )

    exit_code, stdout_text, stderr_text = _invoke_cli(
        [
            "seasonality",
            "diagnostics",
            "--dataset-id",
            "dataset-alpha",
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
    assert '[cli.validation.seasonality.diagnostics.invalid_input]' in stderr_text
    assert 'field_path="event_records_document"' in stderr_text
    assert "event records document record must be an object" in stderr_text


def test_seasonality_diagnostics_cli_unknown_exchange_returns_validation_error(
    calendar_registry_json: str,
    tmp_path: Path,
) -> None:
    event_records_path = _write_event_records_document(
        tmp_path / "seasonality-diagnostics-unknown-exchange-events.json",
        _seasonality_diagnostics_event_records(),
    )
    calendar_registry_path = _write_json_path(
        tmp_path / "seasonality-diagnostics-unknown-exchange-calendar.json",
        cast(dict[str, Any], orjson.loads(calendar_registry_json)),
    )

    exit_code, stdout_text, stderr_text = _invoke_cli(
        [
            "seasonality",
            "diagnostics",
            "--dataset-id",
            "dataset-alpha",
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
    assert '[cli.validation.seasonality.diagnostics.invalid_input]' in stderr_text
    assert 'field_path="exchange_code"' in stderr_text


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
        cast(dict[str, Any], orjson.loads(calendar_registry_json)),
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
        cast(dict[str, Any], orjson.loads(calendar_registry_json)),
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
        cast(dict[str, Any], orjson.loads(calendar_registry_json)),
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
        cast(dict[str, Any], orjson.loads(calendar_registry_json)),
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
    calendar_registry_payload = cast(dict[str, Any], orjson.loads(calendar_registry_json))
    exchanges = cast(list[dict[str, Any]], calendar_registry_payload["exchanges"])
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
            "fit",
            "plan",
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
            "explicit_price_column",
            "--tick-price-column",
            "trade_price",
            "--tick-trade-sign-column",
            "trade_sign",
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
    assert summary_payload["preparation_schema_version"] == CALIBRATION_MARKET_DATA_PREPARATION_SCHEMA_VERSION
    assert summary_payload["workflow_scope"] == CalibrationMarketDataWorkflowScope.FULL_WITH_OPTION_SURFACE.value
    assert summary_payload["session_context"]["exchange_code"] == "XNYS"
    assert summary_payload["session_resolution"]["resolution_kind"] == "explicit_override"
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
        "mean_reversion",
        "joint_refinement",
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
            "fit",
            "plan",
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
    assert stdout_text.startswith("fit plan prepared:")
    assert "workflow_scope=seasonality_only" in stdout_text
    assert "execution_mode=memory" in stdout_text
    assert "session_resolution:" in stdout_text
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
        command="fit.plan",
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
            "fit",
            "plan",
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
        command="fit.plan",
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
            "fit",
            "plan",
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
        command="fit.plan",
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
            "fit",
            "plan",
            "--config-json",
            str(config_path),
            "--output",
            "human",
        ],
        environment={cli_module.CLI_OUTPUT_ENV_VAR: "json"},
    )

    assert exit_code == int(cli_module.CliExitCode.SUCCESS), stderr_text
    assert stdout_text.startswith("fit plan prepared:")
    assert stderr_text == ""


def test_market_data_cli_rejects_unsupported_tick_data_format() -> None:
    exit_code, _, _ = _invoke_cli(
        [
            "fit",
            "plan",
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

    assert exit_code == int(cli_module.CliExitCode.VALIDATION_ERROR)


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
            "fit",
            "plan",
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
    assert 'field_path="request"' in stderr_text
    assert "tick_data_source.path" in stderr_text
    assert "(os error 2)" in stderr_text
    assert (
        "No such file or directory" in stderr_text
        or "The system cannot find the file specified." in stderr_text
    )


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
            "fit",
            "plan",
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
    assert 'field_path="request"' in stderr_text
    assert str(market_data_paths["tick_data"]) in stderr_text

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
        tick_price_column_name="last_trade",
    )

    exit_code, stdout_text, stderr_text = _invoke_cli(
        [
            "fit",
            "plan",
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
            "explicit_price_column",
            "--tick-price-column",
            "trade_price",
            "--tick-trade-sign-column",
            "trade_sign",
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
    assert 'field_path="request"' in stderr_text
    assert "Missing columns: trade_price" in stderr_text
    assert str(market_data_paths["tick_data"]) in stderr_text


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
            "fit",
            "plan",
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
    assert 'field_path="request"' in stderr_text
    assert "session_configuration.session_definition_source.path" in stderr_text
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
            "fit",
            "plan",
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

    monkeypatch.setattr(cli_simulation_commands_module, "run_seeded_tick_event_simulation", capturing_runner)

    request_path = _write_json_path(tmp_path / "simulation_request.json", request.to_payload())
    calendar_registry_path = _write_json_path(
        tmp_path / "calendar_registry.json",
        orjson.loads(calendar_registry_json),
    )

    exit_code, stdout_text, stderr_text = _invoke_cli(
        [
            "simulate",
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


def test_simulation_cli_missing_calendar_registry_file_reports_calendar_registry_argument(
    calendar_registry_json: str,
    tmp_path: Path,
) -> None:
    request_path = _write_json_path(
        tmp_path / "simulation_request.json",
        _build_simulation_request(calendar_registry_json).to_payload(),
    )
    missing_calendar_registry_path = tmp_path / "missing-calendar-registry.json"

    exit_code, stdout_text, stderr_text = _invoke_cli(
        [
            "simulate",
            "seeded-ticks",
            "--request-json",
            str(request_path),
            "--calendar-registry-json",
            str(missing_calendar_registry_path),
        ]
    )

    assert exit_code == int(cli_module.CliExitCode.VALIDATION_ERROR)
    assert stdout_text == ""
    assert 'field_path="calendar_registry_json"' in stderr_text
    assert '[cli.validation.simulate.seeded_ticks.invalid_input]' in stderr_text


def test_coarse_simulation_cli_maps_request_json_to_runtime_contract(
    calendar_registry_json: str,
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> None:
    request = _build_coarse_simulation_request(calendar_registry_json)
    captured_request: dict[str, CoarseSimulationRunRequest] = {}

    def capturing_runner(request_model: CoarseSimulationRunRequest) -> simulation_module.CoarseSimulationPath:
        captured_request["request"] = request_model
        return simulation_module.run_coarse_simulation(request_model)

    monkeypatch.setattr(cli_simulation_commands_module, "run_coarse_simulation", capturing_runner)

    request_path = _write_json_path(tmp_path / "coarse_simulation_request.json", request.to_payload())

    exit_code, stdout_text, stderr_text = _invoke_cli(
        [
            "simulate",
            "coarse-interval",
            "--request-json",
            str(request_path),
        ]
    )

    assert exit_code == int(cli_module.CliExitCode.SUCCESS), stderr_text
    assert captured_request["request"] == request

    command_output = orjson.loads(stdout_text)
    assert command_output["schema_version"] == "simulation.coarse_interval.path.v1"
    assert command_output["simulated_interval_count"] == 3
    assert len(command_output["records"]) == 3


def test_price_cli_maps_model_and_flags_to_runtime_contract(
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> None:
    model_path = _write_saved_price_model(tmp_path)
    captured_arguments: dict[str, Any] = {}
    original_price_option = cli_pricing_commands_module.JaniformModel.price_option

    def capturing_price_option(
        self: calibration_module.JaniformModel,
        **pricing_arguments: Any,
    ) -> pricing_module.VanillaOptionPricingRunResult:
        captured_arguments.update(pricing_arguments)
        return original_price_option(self, **pricing_arguments)

    monkeypatch.setattr(cli_pricing_commands_module.JaniformModel, "price_option", capturing_price_option)

    exit_code, stdout_text, stderr_text = _invoke_cli(
        [
            "price",
            "--model-path",
            str(model_path),
            "--valuation-time-utc",
            "2026-02-02T15:00:00Z",
            "--spot",
            "101.0",
            "--strike",
            "100.0",
            "--maturity-interval",
            "1.0",
            "--risk-free-rate",
            "0.02",
            "--dividend-yield",
            "0.01",
            "--payoff-type",
            "call",
            "--include-transform-diagnostics",
            "--include-integration-quality-diagnostics",
            "--include-calibration-hooks",
            "--output-path",
            str(tmp_path / "pricing.json"),
            "--output",
            "json",
        ]
    )

    assert exit_code == int(cli_module.CliExitCode.SUCCESS), stderr_text
    assert captured_arguments["spot"] == pytest.approx(101.0)
    assert captured_arguments["strike"] == pytest.approx(100.0)
    assert captured_arguments["maturity_interval"] == pytest.approx(1.0)
    assert captured_arguments["risk_free_rate"] == pytest.approx(0.02)
    assert captured_arguments["dividend_yield"] == pytest.approx(0.01)
    assert captured_arguments["payoff_type"] == "call"
    assert captured_arguments["include_transform_diagnostics"] is True
    assert captured_arguments["include_integration_quality_diagnostics"] is True
    assert captured_arguments["include_calibration_hooks"] is True

    command_output = orjson.loads(stdout_text)
    assert command_output["schema_version"] == cli_module.CLI_PRICE_RESULT_SCHEMA_VERSION
    assert command_output["response"]["price"] >= 0.0


def test_price_cli_passes_observed_boundary_state_json_to_high_level_model(
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> None:
    model_path = _write_saved_anchored_price_model(tmp_path)
    observed_boundary_state_path = _write_json_path(
        tmp_path / "observed_boundary_state.json",
        pricing_module.MeanReversionBoundaryState(
            observed_log_price=0.25,
            anchor_value=0.25,
            deviation=0.0,
        ).to_payload(),
    )
    captured_arguments: dict[str, Any] = {}
    original_price_option = cli_pricing_commands_module.JaniformModel.price_option

    def capturing_price_option(
        self: calibration_module.JaniformModel,
        **pricing_arguments: Any,
    ) -> pricing_module.VanillaOptionPricingRunResult:
        captured_arguments.update(pricing_arguments)
        return original_price_option(self, **pricing_arguments)

    monkeypatch.setattr(cli_pricing_commands_module.JaniformModel, "price_option", capturing_price_option)

    exit_code, stdout_text, stderr_text = _invoke_cli(
        [
            "price",
            "--model-path",
            str(model_path),
            "--observed-boundary-state-json",
            str(observed_boundary_state_path),
            "--valuation-time-utc",
            "2026-02-02T15:00:00Z",
            "--spot",
            "101.0",
            "--strike",
            "100.0",
            "--maturity-interval",
            "1.0",
            "--output",
            "json",
        ]
    )

    assert exit_code == int(cli_module.CliExitCode.SUCCESS), stderr_text
    assert captured_arguments["observed_boundary_state"] == pricing_module.MeanReversionBoundaryState(
        observed_log_price=0.25,
        anchor_value=0.25,
        deviation=0.0,
    )
    command_output = orjson.loads(stdout_text)
    assert command_output["response"]["price"] >= 0.0


def test_price_cli_requires_observed_boundary_state_for_anchored_models(tmp_path: Path) -> None:
    model_path = _write_saved_anchored_price_model(tmp_path)

    exit_code, stdout_text, stderr_text = _invoke_cli(
        [
            "price",
            "--model-path",
            str(model_path),
            "--valuation-time-utc",
            "2026-02-02T15:00:00Z",
            "--spot",
            "101.0",
            "--strike",
            "100.0",
            "--maturity-interval",
            "1.0",
        ]
    )

    assert exit_code == int(cli_module.CliExitCode.VALIDATION_ERROR)
    assert "price run_id:" in stdout_text
    assert "observed_boundary_state" in stderr_text


def _write_saved_price_model(tmp_path: Path) -> Path:
    helper_monkeypatch = pytest.MonkeyPatch()
    try:
        helper_monkeypatch.chdir(tmp_path)
        model = _build_full_workflow_model(
            tmp_path,
            helper_monkeypatch,
            run_id="workflow-contract-price-model",
        )
    finally:
        helper_monkeypatch.undo()

    model_path = tmp_path / "workflow-contract-price-model.json"
    model.save(model_path)
    return model_path


def _write_saved_anchored_price_model(tmp_path: Path) -> Path:
    helper_monkeypatch = pytest.MonkeyPatch()
    try:
        helper_monkeypatch.chdir(tmp_path)
        model = _build_full_workflow_model(
            tmp_path,
            helper_monkeypatch,
            run_id="workflow-contract-anchored-price-model",
        )
    finally:
        helper_monkeypatch.undo()

    update_model_mean_reversion_payload(
        model,
        build_constant_mean_reversion_fitted_parameters_payload(kappa=0.35, anchor_value=0.25),
    )
    model_path = tmp_path / "workflow-contract-anchored-price-model.json"
    model.save(model_path)
    return model_path


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

    monkeypatch.setattr(cli_calibration_commands_module, "run_staged_calibration", capturing_runner)

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
            "fit",
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

    monkeypatch.setattr(cli_calibration_commands_module, "run_staged_calibration", capturing_runner)

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
            "fit",
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
    resolved_checkpoint_directory = cli_paths_module._resolve_path_from_repository_root(
        repository_root,
        relative_checkpoint_directory,
    )
    normalized_execution_input = cli_calibration_commands_module.override_calibration_execution_mode(
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

    monkeypatch.setattr(cli_calibration_commands_module, "run_staged_calibration", failing_runner)

    exit_code, stdout_text, stderr_text = _invoke_cli(
        [
            "fit",
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


def test_coarse_simulation_cli_runtime_failures_return_runtime_exit_code(
    calendar_registry_json: str,
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> None:
    request_path = _write_json_path(
        tmp_path / "coarse_simulation_request.json",
        _build_coarse_simulation_request(calendar_registry_json).to_payload(),
    )

    def failing_runner(_: CoarseSimulationRunRequest) -> simulation_module.CoarseSimulationPath:
        raise JaniformExecutionError(
            JaniformErrorDetails.from_context(
                error_id="simulation.execution.coarse_interval.run_failed",
                category=JaniformErrorCategory.EXECUTION,
                workflow="simulation",
                entrypoint="run_coarse_simulation",
                message="native coarse simulation failed",
            )
        )

    monkeypatch.setattr(cli_simulation_commands_module, "run_coarse_simulation", failing_runner)

    exit_code, stdout_text, stderr_text = _invoke_cli(
        [
            "simulate",
            "coarse-interval",
            "--request-json",
            str(request_path),
        ]
    )

    assert exit_code == int(cli_module.CliExitCode.RUNTIME_ERROR)
    assert stdout_text == ""
    assert '[simulation.execution.coarse_interval.run_failed]' in stderr_text
    assert 'category="execution"' in stderr_text


def test_pricing_cli_unexpected_failures_return_internal_exit_code(
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> None:
    monkeypatch.chdir(tmp_path)
    model_path = _write_saved_price_model(tmp_path)

    def broken_price_option(
        self: calibration_module.JaniformModel,
        **_: Any,
    ) -> pricing_module.VanillaOptionPricingRunResult:
        del self
        raise RuntimeError("unexpected pricing bug")

    monkeypatch.setattr(cli_pricing_commands_module.JaniformModel, "price_option", broken_price_option)

    exit_code, stdout_text, stderr_text = _invoke_cli(
        [
            "price",
            "--model-path",
            str(model_path),
            "--valuation-time-utc",
            "2026-02-02T15:00:00Z",
            "--spot",
            "101.0",
            "--strike",
            "100.0",
            "--maturity-interval",
            "1.0",
        ]
    )

    assert exit_code == int(cli_module.CliExitCode.INTERNAL_ERROR)
    assert "price run_id:" in stdout_text
    assert "primary output:" in stdout_text
    assert '[cli.internal.price.failed]' in stderr_text
    assert 'category="integration"' in stderr_text


def test_coarse_simulation_cli_rejects_malformed_request_document(tmp_path: Path) -> None:
    request_path = _write_json_path(
        tmp_path / "malformed_coarse_simulation_request.json",
        {
            "schema_version": "simulation.coarse_interval.run_request.v1",
            "seed": 17,
            "start_time_utc_nanos": _utc_nanos(2026, 2, 2, 14, 0, 0),
            "horizon_interval_count": 1,
            "interval_delta": 1.0,
            "model_inputs": "not-an-object",
            "hourly_factor_contract": {
                "schema_version": "seasonality.hourly_factor.contract.v1",
                "exchange_code": "XNYS",
                "exchange_timezone_name": "America/New_York",
                "request_start_utc_nanos": _utc_nanos(2026, 2, 2, 14, 0, 0),
                "request_end_utc_nanos": _utc_nanos(2026, 2, 2, 15, 0, 0),
                "interval_duration_nanos": 3_600_000_000_000,
                "interval_indices": [0],
                "interval_start_utc_nanos": [_utc_nanos(2026, 2, 2, 14, 0, 0)],
                "interval_end_utc_nanos": [_utc_nanos(2026, 2, 2, 15, 0, 0)],
                "interval_start_exchange_local": ["2026-02-02T09:00:00-05:00"],
                "interval_end_exchange_local": ["2026-02-02T10:00:00-05:00"],
                "s_h": [1.0],
                "known_component_nu_h": [0.0],
                "surprise_component_nu_h": [0.0],
                "nu_h": [0.0],
                "interval_count": 1,
                "first_interval_start_utc_nanos": _utc_nanos(2026, 2, 2, 14, 0, 0),
                "last_interval_end_utc_nanos": _utc_nanos(2026, 2, 2, 15, 0, 0),
                "skipped_local_hour_interval_count": 0,
                "repeated_local_hour_interval_count": 0,
                "interval_boundary_policy": "start_inclusive_end_exclusive.v1",
                "interval_alignment_policy": "request_start_anchored_fixed_width_utc_interval_grid.v1",
                "timezone_transition_policy": "fixed_width_utc_interval_projected_to_exchange_local.v1",
                "spring_forward_policy": "skip_nonexistent_local_hour_labels.v1",
                "fall_back_policy": "preserve_repeated_local_hour_labels_with_numeric_utc_offset.v1",
                "seasonality_materialization_method": "test.explicit_payload.v1",
                "news_intensity_materialization_method": "test.explicit_payload.v1",
            },
        },
    )

    exit_code, stdout_text, stderr_text = _invoke_cli(
        [
            "simulate",
            "coarse-interval",
            "--request-json",
            str(request_path),
        ]
    )

    assert exit_code == int(cli_module.CliExitCode.VALIDATION_ERROR)
    assert stdout_text == ""
    assert '[simulation.validation.contract]' in stderr_text
    assert 'field_path="model_inputs"' in stderr_text
    assert 'message="must be an object"' in stderr_text


def test_coarse_simulation_cli_rejects_interval_factor_alignment_mismatch(
    calendar_registry_json: str,
    tmp_path: Path,
) -> None:
    request_payload = _build_coarse_simulation_request(calendar_registry_json).to_payload()
    request_payload["interval_delta"] = 0.5
    request_path = _write_json_path(
        tmp_path / "misaligned_coarse_simulation_request.json",
        request_payload,
    )

    exit_code, stdout_text, stderr_text = _invoke_cli(
        [
            "simulate",
            "coarse-interval",
            "--request-json",
            str(request_path),
        ]
    )

    assert exit_code == int(cli_module.CliExitCode.VALIDATION_ERROR)
    assert stdout_text == ""
    assert '[simulation.validation.contract]' in stderr_text
    assert 'field_path="hourly_factor_contract.interval_duration_nanos"' in stderr_text
    assert "must match interval_delta exactly" in stderr_text


def test_coarse_simulation_cli_supports_human_output_mode(
    calendar_registry_json: str,
    tmp_path: Path,
) -> None:
    request_path = _write_json_path(
        tmp_path / "coarse_simulation_request_human.json",
        _build_coarse_simulation_request(calendar_registry_json).to_payload(),
    )

    exit_code, stdout_text, stderr_text = _invoke_cli(
        [
            "simulate",
            "coarse-interval",
            "--request-json",
            str(request_path),
            "--output",
            "human",
        ]
    )

    assert exit_code == int(cli_module.CliExitCode.SUCCESS), stderr_text
    assert stdout_text.startswith("simulate coarse-interval completed:")
    assert "simulated_interval_count=3" in stdout_text
    assert "interval_delta=1.0" in stdout_text
    assert stderr_text == ""


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
            "fit",
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


def test_calibration_cli_missing_calendar_registry_file_reports_calendar_registry_argument(
    tmp_path: Path,
) -> None:
    execution_input_path = _write_json_path(
        tmp_path / "calibration_execution_input.json",
        build_valid_execution_input_model().to_payload(),
    )
    missing_calendar_registry_path = tmp_path / "missing-calendar-registry.json"

    exit_code, stdout_text, stderr_text = _invoke_cli(
        [
            "fit",
            "staged",
            "--execution-input-json",
            str(execution_input_path),
            "--calendar-registry-json",
            str(missing_calendar_registry_path),
            "--execution-mode",
            "memory",
        ]
    )

    assert exit_code == int(cli_module.CliExitCode.VALIDATION_ERROR)
    assert stdout_text == ""
    assert 'field_path="calendar_registry_json"' in stderr_text
    assert '[cli.validation.fit.staged.invalid_input]' in stderr_text


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
            "fit",
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


def test_calibration_cli_rejects_missing_prepared_payload_artifact(
    calendar_registry_json: str,
    tmp_path: Path,
) -> None:
    execution_input_payload = build_valid_execution_input_model().to_payload()
    missing_prepared_payload_path = tmp_path / "missing_prepared_payload.json"
    execution_input_payload["prepared_payload_file"]["path"] = str(missing_prepared_payload_path)

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
            "fit",
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
    assert 'field_path="prepared_payload_file.path"' in stderr_text
    assert "could not be read" in stderr_text


def _write_event_records_document(path: Path, event_records: list[dict[str, Any]]) -> Path:
    return _write_json_path(
        path,
        {"event_records": event_records},
    )


def _seasonality_diagnostics_event_records() -> list[dict[str, Any]]:
    return [
        {"timestamp": "2026-02-02T14:30:00Z"},
        {"timestamp": "2026-02-02T15:30:00Z"},
        {"timestamp": "2026-02-03T14:45:00Z"},
        {"timestamp": "2026-02-04T14:30:00Z"},
        {"timestamp": "2026-02-05T15:15:00Z"},
        {"timestamp": "2026-02-06T14:30:00Z"},
        {"timestamp": "2026-02-09T14:30:00Z"},
        {"timestamp": "2026-02-10T15:00:00Z"},
    ]


def _write_cli_command_config(
    path: Path,
    *,
    command: str,
    arguments: dict[str, Any],
) -> Path:
    return _write_json_path(
        path,
        {
            "schema_version": cli_module.CLI_COMMAND_CONFIG_SCHEMA_VERSION,
            "command": command,
            "arguments": arguments,
        },
    )


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

    def read(self, *_args, **_kwargs) -> str:
        raise AssertionError("CLI should not read stdin for automation-friendly commands")

    def readline(self, *_args, **_kwargs) -> str:
        raise AssertionError("CLI should not read stdin for automation-friendly commands")
