from argparse import Namespace
from pathlib import Path
from typing import Any, Optional, cast

from .shared import _build_calendar_registry_from_document, _override_calibration_execution_mode
from .._constants import (
    CLI_CALIBRATION_MARKET_DATA_ENTRYPOINT,
    CLI_CALIBRATION_STAGED_ENTRYPOINT,
    CLI_CALIBRATION_STAGED_INTERNAL_ERROR_ID,
    CLI_CALIBRATION_STAGED_VALIDATION_ERROR_ID,
    CLI_MARKET_DATA_SUMMARY_FIELD_SELECTION_POLICY,
    CLI_MARKET_DATA_SUMMARY_SCHEMA_VERSION,
    CliExitCode,
)
from ..documents import _load_json_document
from ..errors import _classify_market_data_command_error, _classify_workflow_command_error
from ..paths import _require_path_argument, _resolve_path_from_repository_root, resolve_repository_root
from ..rendering import (
    _render_calibration_staged_human_output,
    _render_market_data_calibration_human_output,
    _write_cli_error,
    _write_command_output,
)
from ...calibration import (
    CALIBRATION_MARKET_DATA_REQUEST_SCHEMA_VERSION,
    CalibrationExecutionMode,
    MarketDataCalibrationRequest,
    PreparedMarketDataCalibration,
    prepare_market_data_calibration,
    run_staged_calibration,
    validate_market_data_calibration_request,
    validate_staged_calibration_execution_input,
)
from ...errors import JaniformError


def _run_calibration_market_data_command(parsed_arguments: Namespace) -> int:
    repository_root = resolve_repository_root()

    try:
        request = _build_market_data_calibration_request_from_arguments(
            parsed_arguments,
            repository_root=repository_root,
        )
        prepared_calibration = prepare_market_data_calibration(request)
    except JaniformError as e:
        return _write_cli_error(e)
    except Exception as e:
        return _write_cli_error(_classify_market_data_command_error(e))

    _write_command_output(
        parsed_arguments,
        json_payload=_build_market_data_calibration_summary_payload(prepared_calibration),
        human_output_factory=lambda: _render_market_data_calibration_human_output(prepared_calibration),
    )

    return int(CliExitCode.SUCCESS)


def _run_calibration_staged_command(parsed_arguments: Namespace) -> int:
    repository_root = resolve_repository_root()
    execution_input_path = _resolve_path_from_repository_root(
        repository_root,
        cast(Path, parsed_arguments.execution_input_json),
    )
    calendar_registry_path = _resolve_path_from_repository_root(
        repository_root,
        cast(Path, parsed_arguments.calendar_registry_json),
    )

    disk_checkpoint_directory_argument = cast(Path | None, parsed_arguments.disk_checkpoint_directory)
    disk_checkpoint_directory = (
        _resolve_path_from_repository_root(repository_root, disk_checkpoint_directory_argument)
        if disk_checkpoint_directory_argument is not None
        else None
    )

    try:
        execution_input_document = _load_json_document(execution_input_path, "calibration execution input")
        calendar_registry_document = _load_json_document(calendar_registry_path, "calendar registry")

        execution_input = validate_staged_calibration_execution_input(execution_input_document)

        normalized_execution_input = _override_calibration_execution_mode(
            execution_input=execution_input,
            execution_mode=CalibrationExecutionMode(cast(str, parsed_arguments.execution_mode)),
            disk_checkpoint_directory=disk_checkpoint_directory,
        )

        calendar_registry = _build_calendar_registry_from_document(calendar_registry_document)

        response = run_staged_calibration(normalized_execution_input, calendar_registry=calendar_registry)
    except JaniformError as e:
        return _write_cli_error(e)
    except Exception as e:
        return _write_cli_error(
            _classify_workflow_command_error(
                e,
                entrypoint=CLI_CALIBRATION_STAGED_ENTRYPOINT,
                validation_error_id=CLI_CALIBRATION_STAGED_VALIDATION_ERROR_ID,
                internal_error_id=CLI_CALIBRATION_STAGED_INTERNAL_ERROR_ID,
                file_field_path="execution_input_json",
                document_field_path="execution_input_document",
            )
        )

    payload = response.to_payload()

    _write_command_output(
        parsed_arguments,
        json_payload=payload,
        human_output_factory=lambda: _render_calibration_staged_human_output(payload),
    )

    return int(CliExitCode.SUCCESS)


def _build_market_data_calibration_request_from_arguments(
    parsed_arguments: Namespace,
    *,
    repository_root: Path,
) -> MarketDataCalibrationRequest:
    tick_data_path = _require_path_argument(
        repository_root,
        cast(Path | None, parsed_arguments.tick_data_path),
        argument_name="tick_data_path",
    )
    session_definition_path_argument = cast(Path | None, parsed_arguments.session_definition_path)
    session_definition_path = (
        _require_path_argument(
            repository_root,
            session_definition_path_argument,
            argument_name="session_definition_path",
        )
        if session_definition_path_argument is not None
        else None
    )

    disk_checkpoint_directory_argument = cast(Path | None, parsed_arguments.disk_checkpoint_directory)
    disk_checkpoint_directory = (
        _resolve_path_from_repository_root(repository_root, disk_checkpoint_directory_argument)
        if disk_checkpoint_directory_argument is not None
        else None
    )

    option_surface_path_argument = cast(Path | None, parsed_arguments.option_surface_path)
    option_surface_path = (
        _resolve_path_from_repository_root(repository_root, option_surface_path_argument)
        if option_surface_path_argument is not None
        else None
    )

    known_news_path_argument = cast(Path | None, parsed_arguments.known_news_path)
    known_news_path = (
        _resolve_path_from_repository_root(repository_root, known_news_path_argument)
        if known_news_path_argument is not None
        else None
    )

    payload: dict[str, Any] = {
        "schema_version": CALIBRATION_MARKET_DATA_REQUEST_SCHEMA_VERSION,
        "run_id": cast(str, parsed_arguments.run_id),
        "tick_data_source": {
            "path": str(tick_data_path),
            "file_format": cast(str, parsed_arguments.tick_data_format),
            "provenance_label": cast(str, parsed_arguments.tick_data_provenance_label),
        },
        "session_configuration": {
            "exchange_code": cast(str, parsed_arguments.exchange_code),
        },
        "column_mapping": {
            "tick_data": {
                "timestamp_column": cast(str, parsed_arguments.tick_timestamp_column),
                "price_column": cast(Optional[str], parsed_arguments.tick_price_column),
                "trade_price_column": cast(Optional[str], parsed_arguments.tick_trade_price_column),
                "trade_sign_column": cast(Optional[str], parsed_arguments.tick_trade_sign_column),
                "bid_column": cast(Optional[str], parsed_arguments.tick_bid_column),
                "ask_column": cast(Optional[str], parsed_arguments.tick_ask_column),
                "close_price_column": cast(Optional[str], parsed_arguments.tick_close_price_column),
            },
        },
        "workflow_scope": cast(str, parsed_arguments.workflow_scope),
        "normalization_policy": {
            "timestamp_timezone": cast(str, parsed_arguments.timestamp_timezone),
            "sort_records_by_timestamp": not bool(parsed_arguments.disable_timestamp_sort),
            "duplicate_record_policy": cast(str, parsed_arguments.duplicate_record_policy),
            "tick_price_input_policy": cast(Optional[str], parsed_arguments.tick_price_input_policy),
            "option_quote_input_policy": cast(Optional[str], parsed_arguments.option_quote_input_policy),
        },
        "output_preferences": {
            "execution_mode": cast(str, parsed_arguments.execution_mode),
            "disk_checkpoint_directory": None if disk_checkpoint_directory is None else str(disk_checkpoint_directory),
            "emit_derived_staged_execution_input": bool(parsed_arguments.emit_derived_staged_execution_input),
            "emit_input_provenance_report": not bool(parsed_arguments.omit_input_provenance_report),
        },
    }

    if parsed_arguments.session_timezone is not None:
        payload["session_configuration"]["session_timezone"] = cast(str, parsed_arguments.session_timezone)

    if session_definition_path is not None:
        payload["session_configuration"]["session_definition_source"] = {
            "path": str(session_definition_path),
            "file_format": cast(str, parsed_arguments.session_definition_format),
            "provenance_label": cast(str, parsed_arguments.session_definition_provenance_label),
        }

    if option_surface_path is not None:
        payload["option_surface_source"] = {
            "path": str(option_surface_path),
            "file_format": cast(Optional[str], parsed_arguments.option_surface_format),
            "provenance_label": cast(Optional[str], parsed_arguments.option_surface_provenance_label),
        }
        payload["column_mapping"]["option_surface"] = {
            "quote_timestamp_column": cast(Optional[str], parsed_arguments.option_quote_timestamp_column),
            "expiry_timestamp_column": cast(Optional[str], parsed_arguments.option_expiry_column),
            "strike_column": cast(Optional[str], parsed_arguments.option_strike_column),
            "option_type_column": cast(Optional[str], parsed_arguments.option_type_column),
            "mid_price_column": cast(Optional[str], parsed_arguments.option_mid_price_column),
            "bid_column": cast(Optional[str], parsed_arguments.option_bid_column),
            "ask_column": cast(Optional[str], parsed_arguments.option_ask_column),
            "implied_volatility_column": cast(Optional[str], parsed_arguments.option_implied_volatility_column),
        }

    if known_news_path is not None:
        payload["known_news_inputs"] = {
            "calendar_source": {
                "path": str(known_news_path),
                "file_format": cast(Optional[str], parsed_arguments.known_news_format),
                "provenance_label": cast(Optional[str], parsed_arguments.known_news_provenance_label),
            },
            "calendar_timezone": cast(Optional[str], parsed_arguments.known_news_timezone),
        }

        if parsed_arguments.known_news_timestamp_column is not None:
            payload["column_mapping"]["known_news"] = {
                "event_timestamp_column": cast(str, parsed_arguments.known_news_timestamp_column),
                "event_identifier_column": cast(Optional[str], parsed_arguments.known_news_event_id_column),
            }

    return validate_market_data_calibration_request(payload)


def _build_market_data_calibration_summary_payload(
    prepared_calibration: PreparedMarketDataCalibration,
) -> dict[str, Any]:
    request = prepared_calibration.request

    return {
        "schema_version": CLI_MARKET_DATA_SUMMARY_SCHEMA_VERSION,
        "command": CLI_CALIBRATION_MARKET_DATA_ENTRYPOINT,
        "request_schema_version": request.schema_version,
        "preparation_schema_version": prepared_calibration.schema_version,
        "run_id": request.run_id,
        "workflow_scope": request.workflow_scope.value,
        "session_context": {
            "exchange_code": request.session_configuration.exchange_code,
            "session_timezone": prepared_calibration.session_resolution.resolved_timezone_name,
            "resolution_kind": prepared_calibration.session_resolution.resolution_kind,
        },
        "input_files": {
            "tick_data": request.tick_data_source.to_payload(),
            "session_definition": (
                request.session_configuration.session_definition_source.to_payload()
                if request.session_configuration.session_definition_source is not None
                else None
            ),
            "known_news": (
                request.known_news_inputs.calendar_source.to_payload()
                if request.known_news_inputs is not None
                else None
            ),
            "option_surface": (
                request.option_surface_source.to_payload()
                if request.option_surface_source is not None
                else None
            ),
        },
        "column_mapping": request.column_mapping.to_payload(),
        "normalization_policy": request.normalization_policy.to_payload(),
        "output_artifact_locations": {
            "disk_checkpoint_directory": request.output_preferences.disk_checkpoint_directory,
        },
        "output_emission_policy": {
            "execution_mode": request.output_preferences.execution_mode.value,
            "emit_input_provenance_report": request.output_preferences.emit_input_provenance_report,
            "emit_derived_staged_execution_input": request.output_preferences.emit_derived_staged_execution_input,
        },
        "diagnostics_summary": prepared_calibration.diagnostics.to_payload(),
        "session_resolution": prepared_calibration.session_resolution.to_payload(),
        "provenance_summary": prepared_calibration.provenance.to_payload(),
        "output_schema_metadata": {
            "schema_family": "cli.workflow.command_summary",
            "field_selection_policy": CLI_MARKET_DATA_SUMMARY_FIELD_SELECTION_POLICY,
            "required_sections": [
                "run_id",
                "workflow_scope",
                "session_context",
                "input_files",
                "column_mapping",
                "normalization_policy",
                "output_artifact_locations",
                "output_emission_policy",
                "diagnostics_summary",
                "session_resolution",
                "provenance_summary",
            ],
            "optional_input_sections": [
                "input_files.known_news",
                "input_files.option_surface",
                "column_mapping.known_news",
                "column_mapping.option_surface",
                "provenance_summary.option_surface_summary",
                "provenance_summary.known_news_summary",
            ],
        },
    }


__all__ = [
    "_build_market_data_calibration_request_from_arguments",
    "_build_market_data_calibration_summary_payload",
    "_override_calibration_execution_mode",
    "_run_calibration_market_data_command",
    "_run_calibration_staged_command",
]
