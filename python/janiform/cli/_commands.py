import json
import sys
from argparse import Namespace
from collections.abc import Callable
from datetime import timedelta
from enum import StrEnum
from pathlib import Path
from typing import Any, Optional, cast

from ._constants import (
    CLI_CALIBRATION_MARKET_DATA_ENTRYPOINT,
    CLI_CALIBRATION_MARKET_DATA_INTERNAL_ERROR_ID,
    CLI_CALIBRATION_MARKET_DATA_VALIDATION_ERROR_ID,
    CLI_CALIBRATION_STAGED_ENTRYPOINT,
    CLI_CALIBRATION_STAGED_INTERNAL_ERROR_ID,
    CLI_CALIBRATION_STAGED_VALIDATION_ERROR_ID,
    CLI_MARKET_DATA_SUMMARY_FIELD_SELECTION_POLICY,
    CLI_MARKET_DATA_SUMMARY_SCHEMA_VERSION,
    CLI_OPEN_CLOSED_GAP_ENTRYPOINT,
    CLI_OPEN_CLOSED_GAP_INTERNAL_ERROR_ID,
    CLI_OPEN_CLOSED_GAP_VALIDATION_ERROR_ID,
    CLI_PRICING_VANILLA_OPTION_ENTRYPOINT,
    CLI_PRICING_VANILLA_OPTION_INTERNAL_ERROR_ID,
    CLI_PRICING_VANILLA_OPTION_VALIDATION_ERROR_ID,
    CLI_SIMULATION_SEEDED_TICKS_ENTRYPOINT,
    CLI_SIMULATION_SEEDED_TICKS_INTERNAL_ERROR_ID,
    CLI_SIMULATION_SEEDED_TICKS_VALIDATION_ERROR_ID,
    CliExitCode,
    CliOutputMode,
)
from ..calibration import (
    CALIBRATION_MARKET_DATA_PREPARATION_SCHEMA_VERSION,
    CALIBRATION_MARKET_DATA_REQUEST_SCHEMA_VERSION,
    CalibrationExecutionMode,
    CalibrationStagedExecutionRuntimeResources,
    MarketDataCalibrationRequest,
    PreparedMarketDataCalibration,
    StagedCalibrationExecutionInput,
    prepare_market_data_calibration,
    run_staged_calibration,
    validate_market_data_calibration_request,
    validate_staged_calibration_execution_input,
)
from ..calendar import (
    CalendarInvalidTimestampError,
    CalendarInvalidTimezoneError,
    CalendarMissingExchangeError,
    CalendarRegistry,
    CalendarRegistryLoadError,
)
from ..errors import (
    ContractValidationFailure,
    JaniformError,
    JaniformErrorCategory,
    JaniformErrorDetails,
    JaniformIntegrationError,
    JaniformValidationError,
)
from ..hourly_factors import (
    HourlyNewsIntensityProviderExplicitSource,
    HourlySeasonalityProviderExplicitSource,
)
from ..pricing import run_vanilla_option_pricing, validate_vanilla_option_pricing_execution_input
from ..seasonality import SeasonalityInputValidationError
from ..simulation import (
    SeededTickSimulationHourlyFactorSummary,
    SeededTickSimulationRequest,
    load_hourly_factor_contract_for_simulation,
    run_seeded_tick_event_simulation,
)
from ..validation import run_open_closed_gap_leakage_regression_harness


def _run_simulation_seeded_ticks_command(parsed_arguments: Namespace) -> int:
    repository_root = Path(__file__).resolve().parents[3]
    request_path = _resolve_path_from_repository_root(repository_root, cast(Path, parsed_arguments.request_json))
    calendar_registry_path = _resolve_path_from_repository_root(
        repository_root,
        cast(Path, parsed_arguments.calendar_registry_json),
    )

    try:
        request_document = _load_json_document(request_path, "simulation request")
        calendar_registry_document = _load_json_document(calendar_registry_path, "calendar registry")
        request = SeededTickSimulationRequest.from_mapping(request_document)
        calendar_registry = _build_calendar_registry_from_document(calendar_registry_document)
        request_with_runtime_contract = _attach_runtime_hourly_factor_contract(
            request=request,
            calendar_registry=calendar_registry,
        )
        response = run_seeded_tick_event_simulation(request_with_runtime_contract)
    except JaniformError as e:
        return _write_cli_error(e)
    except Exception as e:
        return _write_cli_error(
            _classify_workflow_command_error(
                e,
                entrypoint=CLI_SIMULATION_SEEDED_TICKS_ENTRYPOINT,
                validation_error_id=CLI_SIMULATION_SEEDED_TICKS_VALIDATION_ERROR_ID,
                internal_error_id=CLI_SIMULATION_SEEDED_TICKS_INTERNAL_ERROR_ID,
                file_field_path="request_json",
                document_field_path="request_document",
            )
        )

    _write_command_output(
        parsed_arguments,
        json_payload=response.to_payload(),
        human_output_factory=lambda: _render_simulation_seeded_ticks_human_output(response.to_payload()),
    )

    return int(CliExitCode.SUCCESS)


def _run_pricing_vanilla_option_command(parsed_arguments: Namespace) -> int:
    repository_root = Path(__file__).resolve().parents[3]
    execution_input_path = _resolve_path_from_repository_root(
        repository_root,
        cast(Path, parsed_arguments.execution_input_json),
    )

    try:
        execution_input_document = _load_json_document(execution_input_path, "pricing execution input")
        execution_input = validate_vanilla_option_pricing_execution_input(execution_input_document)
        run_result = run_vanilla_option_pricing(execution_input)
    except JaniformError as e:
        return _write_cli_error(e)
    except Exception as e:
        return _write_cli_error(
            _classify_workflow_command_error(
                e,
                entrypoint=CLI_PRICING_VANILLA_OPTION_ENTRYPOINT,
                validation_error_id=CLI_PRICING_VANILLA_OPTION_VALIDATION_ERROR_ID,
                internal_error_id=CLI_PRICING_VANILLA_OPTION_INTERNAL_ERROR_ID,
                file_field_path="execution_input_json",
                document_field_path="execution_input_document",
            )
        )

    _write_command_output(
        parsed_arguments,
        json_payload=run_result.to_payload(),
        human_output_factory=lambda: _render_pricing_vanilla_option_human_output(run_result.to_payload()),
    )

    return int(CliExitCode.SUCCESS)


def _run_calibration_market_data_command(parsed_arguments: Namespace) -> int:
    repository_root = Path(__file__).resolve().parents[3]

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
    repository_root = Path(__file__).resolve().parents[3]
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
        response = run_staged_calibration(
            normalized_execution_input,
            calendar_registry=calendar_registry,
        )
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

    _write_command_output(
        parsed_arguments,
        json_payload=response.to_payload(),
        human_output_factory=lambda: _render_calibration_staged_human_output(response.to_payload()),
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
    session_definition_path = _require_path_argument(
        repository_root,
        cast(Path | None, parsed_arguments.session_definition_path),
        argument_name="session_definition_path",
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

    _reject_orphan_market_data_arguments(
        option_surface_path is not None,
        (
            ("option_surface_format", parsed_arguments.option_surface_format),
            ("option_surface_provenance_label", parsed_arguments.option_surface_provenance_label),
            ("option_quote_input_policy", parsed_arguments.option_quote_input_policy),
            ("option_quote_timestamp_column", parsed_arguments.option_quote_timestamp_column),
            ("option_expiry_column", parsed_arguments.option_expiry_column),
            ("option_strike_column", parsed_arguments.option_strike_column),
            ("option_type_column", parsed_arguments.option_type_column),
            ("option_mid_price_column", parsed_arguments.option_mid_price_column),
            ("option_bid_column", parsed_arguments.option_bid_column),
            ("option_ask_column", parsed_arguments.option_ask_column),
            ("option_implied_volatility_column", parsed_arguments.option_implied_volatility_column),
        ),
        message="option-surface flags require --option-surface-path",
    )
    _reject_orphan_market_data_arguments(
        known_news_path is not None,
        (
            ("known_news_format", parsed_arguments.known_news_format),
            ("known_news_provenance_label", parsed_arguments.known_news_provenance_label),
            ("known_news_timezone", parsed_arguments.known_news_timezone),
            ("known_news_timestamp_column", parsed_arguments.known_news_timestamp_column),
            ("known_news_event_id_column", parsed_arguments.known_news_event_id_column),
        ),
        message="known-news flags require --known-news-path",
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
            "session_definition_source": {
                "path": str(session_definition_path),
                "file_format": cast(str, parsed_arguments.session_definition_format),
                "provenance_label": cast(str, parsed_arguments.session_definition_provenance_label),
            },
            "session_timezone": cast(str, parsed_arguments.session_timezone),
        },
        "column_mapping": {
            "tick_data": {
                "timestamp_column": cast(str, parsed_arguments.tick_timestamp_column),
                "price_column": cast(Optional[str], parsed_arguments.tick_price_column),
                "trade_price_column": cast(Optional[str], parsed_arguments.tick_trade_price_column),
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
            "disk_checkpoint_directory": (
                str(disk_checkpoint_directory)
                if disk_checkpoint_directory is not None
                else None
            ),
            "emit_derived_staged_execution_input": bool(parsed_arguments.emit_derived_staged_execution_input),
            "emit_input_provenance_report": not bool(parsed_arguments.omit_input_provenance_report),
        },
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
            "session_timezone": request.session_configuration.session_timezone,
        },
        "input_files": {
            "tick_data": request.tick_data_source.to_payload(),
            "session_definition": request.session_configuration.session_definition_source.to_payload(),
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


def _render_market_data_calibration_human_output(
    prepared_calibration: PreparedMarketDataCalibration,
) -> str:
    request = prepared_calibration.request

    selected_stages = ",".join(stage.value for stage in prepared_calibration.diagnostics.selected_stages)
    preprocessing_steps = ",".join(prepared_calibration.diagnostics.executed_preprocessing_steps)

    lines: list[str] = [
        (
            "calibration market-data request prepared: "
            f"run_id={request.run_id} "
            f"workflow_scope={request.workflow_scope.value} "
            f"execution_mode={request.output_preferences.execution_mode.value}"
        ),
        (
            "tick_data: "
            f"path={request.tick_data_source.path} "
            f"format={request.tick_data_source.file_format.value} "
            f"provenance_label={request.tick_data_source.provenance_label}"
        ),
        (
            "session_definition: "
            f"path={request.session_configuration.session_definition_source.path} "
            f"format={request.session_configuration.session_definition_source.file_format.value} "
            f"exchange_code={request.session_configuration.exchange_code} "
            f"session_timezone={request.session_configuration.session_timezone}"
        ),
        (
            "workflow_diagnostics: "
            f"preparation_schema_version={CALIBRATION_MARKET_DATA_PREPARATION_SCHEMA_VERSION} "
            f"selected_stages={selected_stages} "
            f"preprocessing_steps={preprocessing_steps}"
        ),
        (
            "loaded_data_summary: "
            f"tick_records={prepared_calibration.diagnostics.tick_record_count} "
            f"known_news_events={prepared_calibration.diagnostics.known_news_event_count} "
            f"option_surface_records={prepared_calibration.diagnostics.option_surface_record_count}"
        ),
    ]

    if request.known_news_inputs is not None:
        lines.append(
            (
                "known_news: "
                f"path={request.known_news_inputs.calendar_source.path} "
                f"format={request.known_news_inputs.calendar_source.file_format.value} "
                f"provenance_label={request.known_news_inputs.calendar_source.provenance_label}"
            )
        )

    if request.option_surface_source is not None:
        lines.append(
            (
                "option_surface: "
                f"path={request.option_surface_source.path} "
                f"format={request.option_surface_source.file_format.value} "
                f"provenance_label={request.option_surface_source.provenance_label}"
            )
        )

    lines.append(
        (
            "derived_references: "
            f"trade_events={prepared_calibration.provenance.derived_trade_events_data_reference} "
            f"option_surface={prepared_calibration.provenance.derived_option_surface_data_reference or 'none'}"
        )
    )
    lines.append(
        (
            "normalization_policy: "
            f"timestamp_timezone={request.normalization_policy.timestamp_timezone} "
            f"sort_records_by_timestamp={str(request.normalization_policy.sort_records_by_timestamp).lower()} "
            f"duplicate_record_policy={request.normalization_policy.duplicate_record_policy.value} "
            f"tick_price_input_policy={_optional_enum_value(request.normalization_policy.tick_price_input_policy)} "
            f"option_quote_input_policy={_optional_enum_value(request.normalization_policy.option_quote_input_policy)}"
        )
    )
    lines.append(
        (
            "artifact_targets: "
            f"disk_checkpoint_directory={request.output_preferences.disk_checkpoint_directory or 'none'} "
            f"emit_input_provenance_report={str(request.output_preferences.emit_input_provenance_report).lower()} "
            f"emit_derived_staged_execution_input={str(request.output_preferences.emit_derived_staged_execution_input).lower()}"
        )
    )

    return "\n".join(lines) + "\n"


def _render_simulation_seeded_ticks_human_output(response_payload: dict[str, Any]) -> str:
    return (
        "simulation seeded-ticks completed: "
        f"schema_version={response_payload['schema_version']} "
        f"event_count={len(cast(list[dict[str, Any]], response_payload['event_tape']))} "
        f"event_record_schema_version={cast(dict[str, Any], response_payload['metadata'])['event_record_schema_version']}\n"
    )


def _render_pricing_vanilla_option_human_output(run_result_payload: dict[str, Any]) -> str:
    response_payload = cast(dict[str, Any], run_result_payload["response"])
    return (
        "pricing vanilla-option completed: "
        f"schema_version={run_result_payload['schema_version']} "
        f"price={response_payload['price']} "
        f"delta={response_payload['delta']}\n"
    )


def _render_calibration_staged_human_output(response_payload: dict[str, Any]) -> str:
    return (
        "calibration staged completed: "
        f"run_id={response_payload['run_id']} "
        f"execution_mode={response_payload['execution_mode']} "
        f"status={response_payload['status']} "
        f"stage_report_count={len(cast(list[dict[str, Any]], response_payload['stage_reports']))} "
        f"checkpoint_count={len(cast(list[dict[str, Any]], response_payload['emitted_checkpoints']))}\n"
    )


def _run_open_closed_gap_leakage_regression_command(parsed_arguments: Namespace) -> int:
    repository_root = Path(__file__).resolve().parents[3]
    calendar_fixture_path = _resolve_path_from_repository_root(
        repository_root,
        cast(Path, parsed_arguments.calendar_fixture_path),
    )
    gap_behavior_fixture_path = _resolve_path_from_repository_root(
        repository_root,
        cast(Path, parsed_arguments.gap_behavior_fixture_path),
    )

    try:
        calendar_fixture_document = _load_json_document(calendar_fixture_path, "calendar fixture")
        gap_behavior_fixture_document = _load_json_document(
            gap_behavior_fixture_path,
            "open closed gap behavior fixture",
        )

        calendar_registry = _build_calendar_registry(calendar_fixture_document)
        gap_behavior_cases = _extract_gap_behavior_cases(gap_behavior_fixture_document)

        report = run_open_closed_gap_leakage_regression_harness(
            calendar_registry=calendar_registry,
            gap_behavior_cases=gap_behavior_cases,
            component_absolute_tolerance=float(parsed_arguments.component_absolute_tolerance),
        )
    except JaniformError as e:
        return _write_cli_error(e)
    except Exception as e:
        return _write_cli_error(_classify_open_closed_gap_command_error(e))

    output_mode = CliOutputMode(cast(str, parsed_arguments.output))
    failed_case_count = cast(dict[str, int], report["summary"])["failed_case_count"]
    if output_mode == CliOutputMode.JSON:
        _write_contract_json(report)
        return int(CliExitCode.SUCCESS if failed_case_count == 0 else CliExitCode.RUNTIME_ERROR)

    _write_open_closed_gap_human_output(report)
    return int(CliExitCode.SUCCESS if failed_case_count == 0 else CliExitCode.RUNTIME_ERROR)


def _classify_open_closed_gap_command_error(error: Exception) -> JaniformError:
    if isinstance(error, JaniformError):
        return error

    if isinstance(error, FileNotFoundError):
        return JaniformValidationError(
            ContractValidationFailure.from_context(
                error_id=CLI_OPEN_CLOSED_GAP_VALIDATION_ERROR_ID,
                workflow="cli",
                entrypoint=CLI_OPEN_CLOSED_GAP_ENTRYPOINT,
                field_path="fixture_path",
                message="fixture file could not be found",
                context={"path": error.filename},
            )
        )

    if isinstance(error, json.JSONDecodeError):
        return JaniformValidationError(
            ContractValidationFailure.from_context(
                error_id=CLI_OPEN_CLOSED_GAP_VALIDATION_ERROR_ID,
                workflow="cli",
                entrypoint=CLI_OPEN_CLOSED_GAP_ENTRYPOINT,
                field_path="fixture_document",
                message="fixture file must contain valid JSON",
                context={
                    "column_number": error.colno,
                    "line_number": error.lineno,
                },
            )
        )

    if isinstance(
        error,
        (
            CalendarInvalidTimestampError,
            CalendarInvalidTimezoneError,
            CalendarMissingExchangeError,
            CalendarRegistryLoadError,
            SeasonalityInputValidationError,
            ValueError,
        ),
    ):
        return JaniformValidationError(
            ContractValidationFailure.from_context(
                error_id=CLI_OPEN_CLOSED_GAP_VALIDATION_ERROR_ID,
                workflow="cli",
                entrypoint=CLI_OPEN_CLOSED_GAP_ENTRYPOINT,
                field_path="command_input",
                message=str(error),
            )
        )

    return JaniformIntegrationError(
        JaniformErrorDetails.from_context(
            error_id=CLI_OPEN_CLOSED_GAP_INTERNAL_ERROR_ID,
            category=JaniformErrorCategory.INTEGRATION,
            workflow="cli",
            entrypoint=CLI_OPEN_CLOSED_GAP_ENTRYPOINT,
            message="open closed gap leakage regression command failed unexpectedly",
            context={"source_exception_type": type(error).__name__},
        )
    )


def _classify_market_data_command_error(error: Exception) -> JaniformError:
    if isinstance(error, JaniformError):
        return error

    if isinstance(error, ValueError):
        return JaniformValidationError(
            ContractValidationFailure.from_context(
                error_id=CLI_CALIBRATION_MARKET_DATA_VALIDATION_ERROR_ID,
                workflow="cli",
                entrypoint=CLI_CALIBRATION_MARKET_DATA_ENTRYPOINT,
                field_path="command_input",
                message=str(error),
            )
        )

    return JaniformIntegrationError(
        JaniformErrorDetails.from_context(
            error_id=CLI_CALIBRATION_MARKET_DATA_INTERNAL_ERROR_ID,
            category=JaniformErrorCategory.INTEGRATION,
            workflow="cli",
            entrypoint=CLI_CALIBRATION_MARKET_DATA_ENTRYPOINT,
            message="market-data calibration command failed unexpectedly",
            context={"source_exception_type": type(error).__name__},
        )
    )


def _attach_runtime_hourly_factor_contract(
    *,
    request: SeededTickSimulationRequest,
    calendar_registry: CalendarRegistry,
) -> SeededTickSimulationRequest:
    range_end_utc = request.start_time_utc + timedelta(hours=request.horizon_interval_count)

    hourly_factor_contract = load_hourly_factor_contract_for_simulation(
        calendar_registry=calendar_registry,
        exchange_code=request.hourly_factor_summary.exchange_code,
        range_start_utc=request.start_time_utc,
        range_end_utc=range_end_utc,
        seasonality_source=HourlySeasonalityProviderExplicitSource(
            hourly_factors=request.hourly_factor_summary.s_h,
            materialization_method="cli.request_hourly_factor_summary.v1",
        ),
        news_intensity_source=HourlyNewsIntensityProviderExplicitSource(
            hourly_factors=request.hourly_factor_summary.nu_h,
            materialization_method="cli.request_hourly_factor_summary.v1",
        ),
    )

    derived_summary = SeededTickSimulationHourlyFactorSummary.from_contract(hourly_factor_contract)
    if derived_summary != request.hourly_factor_summary:
        raise JaniformValidationError(
            ContractValidationFailure.from_context(
                error_id=CLI_SIMULATION_SEEDED_TICKS_VALIDATION_ERROR_ID,
                workflow="cli",
                entrypoint=CLI_SIMULATION_SEEDED_TICKS_ENTRYPOINT,
                field_path="request_json.hourly_factor_summary",
                message="request hourly_factor_summary must match the rehydrated hourly factor contract",
                context={
                    "derived_interval_count": derived_summary.interval_count,
                    "request_interval_count": request.hourly_factor_summary.interval_count,
                },
            )
        )

    return SeededTickSimulationRequest.from_runtime_inputs(
        seed=request.seed,
        start_time_utc=request.start_time_utc,
        horizon_interval_count=request.horizon_interval_count,
        kernel_parameters=request.kernel_parameters,
        initial_excitation_state=request.initial_excitation_state,
        hourly_factor_contract=hourly_factor_contract,
        debug_trace_enabled=request.debug_trace_enabled,
        sample_every_nth_transition=request.sample_every_nth_transition,
        max_records_per_chunk=request.max_records_per_chunk,
        state_snapshot_capture=request.state_snapshot_capture,
    )


def _override_calibration_execution_mode(
    *,
    execution_input: StagedCalibrationExecutionInput,
    execution_mode: CalibrationExecutionMode,
    disk_checkpoint_directory: Path | None,
) -> StagedCalibrationExecutionInput:
    if execution_mode == CalibrationExecutionMode.MEMORY and disk_checkpoint_directory is not None:
        raise JaniformValidationError(
            ContractValidationFailure.from_context(
                error_id=CLI_CALIBRATION_STAGED_VALIDATION_ERROR_ID,
                workflow="cli",
                entrypoint=CLI_CALIBRATION_STAGED_ENTRYPOINT,
                field_path="disk_checkpoint_directory",
                message="--disk-checkpoint-directory requires --execution-mode disk",
            )
        )

    runtime_resources: Optional[CalibrationStagedExecutionRuntimeResources] = execution_input.runtime_resources
    if execution_mode == CalibrationExecutionMode.DISK and disk_checkpoint_directory is not None:
        runtime_resources = CalibrationStagedExecutionRuntimeResources(
            disk_checkpoint_directory=str(disk_checkpoint_directory),
        )
    if execution_mode == CalibrationExecutionMode.MEMORY:
        runtime_resources = None

    return validate_staged_calibration_execution_input(
        {
            **execution_input.to_payload(),
            "request": {
                **execution_input.request.to_payload(),
                "execution_mode": execution_mode.value,
            },
            "runtime_resources": None if runtime_resources is None else runtime_resources.to_payload(),
        }
    )


def _classify_workflow_command_error(
    error: Exception,
    *,
    entrypoint: str,
    validation_error_id: str,
    internal_error_id: str,
    file_field_path: str,
    document_field_path: str,
) -> JaniformError:
    if isinstance(error, JaniformError):
        return error

    if isinstance(error, FileNotFoundError):
        return JaniformValidationError(
            ContractValidationFailure.from_context(
                error_id=validation_error_id,
                workflow="cli",
                entrypoint=entrypoint,
                field_path=file_field_path,
                message="input file could not be found",
                context={"path": error.filename},
            )
        )

    if isinstance(error, OSError):
        return JaniformValidationError(
            ContractValidationFailure.from_context(
                error_id=validation_error_id,
                workflow="cli",
                entrypoint=entrypoint,
                field_path=file_field_path,
                message=f"input file could not be read: {error}",
                context={"path": error.filename},
            )
        )

    if isinstance(error, json.JSONDecodeError):
        return JaniformValidationError(
            ContractValidationFailure.from_context(
                error_id=validation_error_id,
                workflow="cli",
                entrypoint=entrypoint,
                field_path=document_field_path,
                message="input file must contain valid JSON",
                context={
                    "column_number": error.colno,
                    "line_number": error.lineno,
                },
            )
        )

    if isinstance(
        error,
        (
            CalendarInvalidTimestampError,
            CalendarInvalidTimezoneError,
            CalendarMissingExchangeError,
            CalendarRegistryLoadError,
            SeasonalityInputValidationError,
            ValueError,
        ),
    ):
        return JaniformValidationError(
            ContractValidationFailure.from_context(
                error_id=validation_error_id,
                workflow="cli",
                entrypoint=entrypoint,
                field_path=document_field_path,
                message=str(error),
            )
        )

    return JaniformIntegrationError(
        JaniformErrorDetails.from_context(
            error_id=internal_error_id,
            category=JaniformErrorCategory.INTEGRATION,
            workflow="cli",
            entrypoint=entrypoint,
            message="workflow command failed unexpectedly",
            context={"source_exception_type": type(error).__name__},
        )
    )
def _resolve_path_from_repository_root(repository_root: Path, path: Path) -> Path:
    if path.is_absolute():
        return path

    return repository_root / path


def _require_path_argument(
    repository_root: Path,
    path: Path | None,
    *,
    argument_name: str,
) -> Path:
    if path is None:
        raise ValueError(f"--{argument_name.replace('_', '-')} is required")

    return _resolve_path_from_repository_root(repository_root, path)


def _reject_orphan_market_data_arguments(
    has_primary_path: bool,
    optional_arguments: tuple[tuple[str, object], ...],
    *,
    message: str,
) -> None:
    if has_primary_path:
        return

    for _, argument_value in optional_arguments:
        if argument_value is not None:
            raise ValueError(message)


def _load_json_document(path: Path, context: str) -> dict[str, Any]:
    payload = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(payload, dict):
        raise ValueError(f"{context} at {path} must decode to a JSON object")

    return cast(dict[str, Any], payload)


def _write_contract_json(payload: dict[str, Any]) -> None:
    sys.stdout.write(f"{json.dumps(payload, sort_keys=True)}\n")


def _write_cli_error(error: JaniformError) -> int:
    sys.stderr.write(f"{error}\n")
    return int(_exit_code_for_error(error))


def _exit_code_for_error(error: JaniformError) -> CliExitCode:
    match error.category:
        case JaniformErrorCategory.VALIDATION:
            return CliExitCode.VALIDATION_ERROR
        case JaniformErrorCategory.EXECUTION:
            return CliExitCode.RUNTIME_ERROR
        case JaniformErrorCategory.INTEGRATION:
            return CliExitCode.INTERNAL_ERROR


def _write_command_output(
    parsed_arguments: Namespace,
    *,
    json_payload: dict[str, Any],
    human_output_factory: Callable[[], str],
) -> None:
    output_mode = CliOutputMode(cast(str, parsed_arguments.output))
    match output_mode:
        case CliOutputMode.JSON:
            _write_contract_json(json_payload)
        case CliOutputMode.HUMAN:
            sys.stdout.write(human_output_factory())


def _write_open_closed_gap_human_output(report: dict[str, Any]) -> None:
    summary = cast(dict[str, int], report["summary"])
    behavior_diff_report_summary = cast(
        dict[str, int],
        cast(dict[str, Any], report["behavior_diff_report"])["summary"],
    )
    if summary["failed_case_count"] == 0:
        sys.stdout.write(
            "open closed gap leakage regression passed: "
            f"{summary['passed_case_count']}/{summary['total_case_count']} scenarios, "
            f"semantic_mismatches={summary['total_semantic_mismatch_failure_count']}, "
            f"diff_regression_cases={behavior_diff_report_summary['regression_case_count']}\n",
        )
        return

    sys.stderr.write(
        "open closed gap leakage regression failed: "
        f"{summary['failed_case_count']}/{summary['total_case_count']} scenarios, "
        f"semantic_mismatches={summary['total_semantic_mismatch_failure_count']}\n",
    )

    for case_report in cast(list[dict[str, Any]], report["case_reports"]):
        if case_report["status"] != "fail":
            continue

        sys.stderr.write(
            f"scenario_id={case_report['fixture_id']} exchange_code={case_report['exchange_code']} "
            f"semantic_mismatch_failure_count={case_report['semantic_mismatch_failure_count']} "
            f"max_absolute_mismatch={case_report['max_absolute_mismatch']}\n",
        )

        contribution_summary = cast(dict[str, Any], case_report["contribution_summary"])

        sys.stderr.write(
            "  contributions "
            f"activity_expected={contribution_summary['expected_activity_coupling_total']} "
            f"activity_observed={contribution_summary['observed_activity_coupling_total']} "
            f"trading_expected={contribution_summary['expected_trading_return_total']} "
            f"trading_observed={contribution_summary['observed_trading_return_total']} "
            f"open_jump_expected={contribution_summary['expected_open_jump_total']} "
            f"open_jump_observed={contribution_summary['observed_open_jump_total']} "
            f"closed_gap_expected={contribution_summary['expected_closed_gap_total']} "
            f"closed_gap_observed={contribution_summary['observed_closed_gap_total']}\n",
        )

        behavior_diff = cast(dict[str, Any], case_report["behavior_diff"])
        for diff_metric in cast(list[dict[str, Any]], behavior_diff["metrics"]):
            if not cast(bool, diff_metric["regression_detected"]):
                continue

            sys.stderr.write(
                "  diff_regression "
                f"metric_id={diff_metric['metric_id']} "
                f"semantic_bucket={diff_metric['semantic_bucket']} "
                f"expected={diff_metric['expected_value']} "
                f"observed={diff_metric['observed_value']} "
                f"absolute_difference={diff_metric['absolute_difference']} "
                f"absolute_tolerance={diff_metric['absolute_tolerance']}\n",
            )

        for failure in cast(list[dict[str, Any]], case_report["failures"]):
            failure_kind = cast(str, failure["failure_kind"])
            match failure_kind:
                case "semantic_mismatch":
                    sys.stderr.write(
                        f"  failure_kind={failure_kind} event_id={failure['event_id']} "
                        f"event_timestamp_utc={failure['event_timestamp_utc']} "
                        f"semantic_bucket={failure['semantic_bucket']} "
                        f"mismatch_kind={failure['mismatch_kind']} "
                        f"absolute_difference={failure['absolute_difference']}\n",
                    )
                case "dispatch_error":
                    sys.stderr.write(
                        f"  failure_kind={failure_kind} error_message={failure['error_message']}\n",
                    )
                case _:
                    sys.stderr.write(f"  failure_kind={failure_kind}\n")


def _optional_enum_value(value: Optional[StrEnum]) -> str:
    return value.value if value is not None else "none"


def _build_calendar_registry(calendar_fixture_document: dict[str, Any]) -> CalendarRegistry:
    return _build_calendar_registry_from_document(calendar_fixture_document)


def _build_calendar_registry_from_document(calendar_registry_document: dict[str, Any]) -> CalendarRegistry:
    calendar_registry_payload: dict[str, Any]
    if "calendar_registry" in calendar_registry_document:
        calendar_registry_payload = _extract_calendar_registry_payload(
            calendar_registry_document,
            field_path="calendar_registry",
        )
    else:
        calendar_registry_payload = _extract_calendar_registry_payload(
            calendar_registry_document,
            field_path="calendar_registry_document",
        )

    return CalendarRegistry(json.dumps(calendar_registry_payload))


def _extract_calendar_registry_payload(
    calendar_registry_document: dict[str, Any],
    *,
    field_path: str,
) -> dict[str, Any]:
    schema_version = calendar_registry_document.get("schema_version")
    if isinstance(schema_version, str) and schema_version == "calendar.registry.v1":
        return calendar_registry_document

    calendar_registry_payload: object = calendar_registry_document.get("calendar_registry")
    if not isinstance(calendar_registry_payload, dict):
        raise ValueError(f"{field_path} must be a calendar.registry.v1 JSON object")

    return cast(dict[str, Any], calendar_registry_payload)


def _extract_gap_behavior_cases(
    gap_behavior_fixture_document: dict[str, Any],
) -> list[dict[str, Any]]:
    raw_cases: object = gap_behavior_fixture_document.get("cases")
    if not isinstance(raw_cases, list):
        raise ValueError("gap behavior fixture must include list field `cases`")

    parsed_cases: list[dict[str, Any]] = []
    for raw_case in raw_cases:
        if not isinstance(raw_case, dict):
            raise ValueError("gap behavior fixture case must be an object")

        initial_accumulator_state: object = raw_case.get("initial_accumulator_state")
        if not isinstance(initial_accumulator_state, dict):
            raise ValueError("gap behavior case must include object field `initial_accumulator_state`")

        injected_events: object = raw_case.get("injected_events")
        if not isinstance(injected_events, list):
            raise ValueError("gap behavior case must include list field `injected_events`")

        parsed_cases.append(
            {
                "fixture_id": raw_case.get("fixture_id"),
                "exchange_code": raw_case.get("exchange_code"),
                "scenario_class": raw_case.get("scenario_class"),
                "deterministic_replay_metadata": raw_case.get("deterministic_replay_metadata"),
                "issue_traceability": raw_case.get("issue_traceability"),
                "initial_j_t": initial_accumulator_state.get("j_t"),
                "initial_m_t": initial_accumulator_state.get("m_t"),
                "initial_g_t": initial_accumulator_state.get("g_t"),
                "injected_events": [
                    {
                        "event_id": (
                            raw_event.get("event_id")
                            if isinstance(raw_event, dict)
                            else None
                        ),
                        "timestamp_utc": (
                            raw_event.get("timestamp_utc")
                            if isinstance(raw_event, dict)
                            else None
                        ),
                        "shared_severity_mark": (
                            raw_event.get("shared_severity_mark")
                            if isinstance(raw_event, dict)
                            else None
                        ),
                        "news_arrival_component": (
                            raw_event.get("news_arrival_component")
                            if isinstance(raw_event, dict)
                            else None
                        ),
                        "coupled_return_jump_mark": (
                            raw_event.get("coupled_return_jump_mark")
                            if isinstance(raw_event, dict)
                            else None
                        ),
                    }
                    for raw_event in injected_events
                ],
            },
        )

    return parsed_cases


def main() -> None:
    from ._parser import _parse_cli_arguments

    try:
        parsed_arguments = _parse_cli_arguments()
    except JaniformError as e:
        raise SystemExit(_write_cli_error(e)) from e

    handler = parsed_arguments.handler
    if handler is not None:
        raise SystemExit(handler(parsed_arguments))

    parsed_arguments.help_target.print_help()

__all__ = [
    '_attach_runtime_hourly_factor_contract',
    '_build_calendar_registry',
    '_build_calendar_registry_from_document',
    '_build_market_data_calibration_request_from_arguments',
    '_build_market_data_calibration_summary_payload',
    '_classify_market_data_command_error',
    '_classify_open_closed_gap_command_error',
    '_classify_workflow_command_error',
    '_exit_code_for_error',
    '_extract_calendar_registry_payload',
    '_extract_gap_behavior_cases',
    '_load_json_document',
    '_optional_enum_value',
    '_override_calibration_execution_mode',
    '_reject_orphan_market_data_arguments',
    '_render_calibration_staged_human_output',
    '_render_market_data_calibration_human_output',
    '_render_pricing_vanilla_option_human_output',
    '_render_simulation_seeded_ticks_human_output',
    '_require_path_argument',
    '_resolve_path_from_repository_root',
    '_run_calibration_market_data_command',
    '_run_calibration_staged_command',
    '_run_open_closed_gap_leakage_regression_command',
    '_run_pricing_vanilla_option_command',
    '_run_simulation_seeded_ticks_command',
    '_write_cli_error',
    '_write_command_output',
    '_write_contract_json',
    '_write_open_closed_gap_human_output',
    'main',
]
