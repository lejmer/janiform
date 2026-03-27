from collections.abc import Mapping
from pathlib import Path
from typing import Any, Optional

import click

from .shared import _build_calendar_registry_from_document, _override_calibration_execution_mode
from ..command_runtime import (
    JaniformCommand,
    command_config_option,
    command_option,
    invoke_command_handler,
    optional_path_option,
    output_option,
    required_json_path_option,
)
from ..context import CliCommandContext
from .._constants import (
    CLI_CALIBRATION_DUPLICATE_RECORD_POLICY_CHOICES,
    CLI_CALIBRATION_EXECUTION_MODE_CHOICES,
    CLI_CALIBRATION_EXECUTION_MODE_HELP,
    CLI_CALIBRATION_MARKET_DATA_ENTRYPOINT,
    CLI_CALIBRATION_MARKET_DATA_EXECUTION_MODE_HELP,
    CLI_CALIBRATION_MARKET_DATA_SCOPE_CHOICES,
    CLI_CALIBRATION_OPTION_QUOTE_POLICY_CHOICES,
    CLI_CALIBRATION_STAGED_ENTRYPOINT,
    CLI_CALIBRATION_STAGED_INTERNAL_ERROR_ID,
    CLI_CALIBRATION_STAGED_VALIDATION_ERROR_ID,
    CLI_CALIBRATION_STRUCTURED_FILE_FORMAT_CHOICES,
    CLI_CALIBRATION_TICK_FILE_FORMAT_CHOICES,
    CLI_CALIBRATION_TICK_PRICE_POLICY_CHOICES,
    CLI_MARKET_DATA_SUMMARY_FIELD_SELECTION_POLICY,
    CLI_MARKET_DATA_SUMMARY_SCHEMA_VERSION,
    CliExitCode,
    CliOutputMode,
)
from ..documents import _load_json_document_for_cli_field
from ..errors import _classify_market_data_command_error, _classify_workflow_command_error
from ..paths import _require_path_argument, _resolve_path_from_repository_root
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


def _run_calibration_market_data_command(
    *,
    cli_context: CliCommandContext,
    output: str,
    **command_arguments,
) -> int:
    try:
        request = _build_market_data_calibration_request_from_arguments(
            cli_context=cli_context,
            **command_arguments,
        )
        prepared_calibration = prepare_market_data_calibration(request)
    except JaniformError as e:
        return _write_cli_error(e)
    except Exception as e:
        return _write_cli_error(_classify_market_data_command_error(e))

    _write_command_output(
        output,
        json_payload=_build_market_data_calibration_summary_payload(prepared_calibration),
        human_output_factory=lambda: _render_market_data_calibration_human_output(prepared_calibration),
    )

    return int(CliExitCode.SUCCESS)


def _run_calibration_staged_command(
    *,
    cli_context: CliCommandContext,
    output: str,
    execution_input_json: Path,
    calendar_registry_json: Path,
    execution_mode: str,
    disk_checkpoint_directory: Optional[Path],
    **_,
) -> int:
    resolved_disk_checkpoint_directory = (
        _resolve_path_from_repository_root(cli_context.repository_root, disk_checkpoint_directory)
        if disk_checkpoint_directory is not None
        else None
    )

    try:
        execution_input_document = _load_json_document_for_cli_field(
            repository_root=cli_context.repository_root,
            path=execution_input_json,
            context="calibration execution input",
            field_path="execution_input_json",
            error_id=CLI_CALIBRATION_STAGED_VALIDATION_ERROR_ID,
            entrypoint=CLI_CALIBRATION_STAGED_ENTRYPOINT,
        )
        calendar_registry_document = _load_json_document_for_cli_field(
            repository_root=cli_context.repository_root,
            path=calendar_registry_json,
            context="calendar registry",
            field_path="calendar_registry_json",
            error_id=CLI_CALIBRATION_STAGED_VALIDATION_ERROR_ID,
            entrypoint=CLI_CALIBRATION_STAGED_ENTRYPOINT,
        )

        execution_input = validate_staged_calibration_execution_input(execution_input_document)

        normalized_execution_input = _override_calibration_execution_mode(
            execution_input=execution_input,
            execution_mode=CalibrationExecutionMode(execution_mode),
            disk_checkpoint_directory=resolved_disk_checkpoint_directory,
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
        output,
        json_payload=payload,
        human_output_factory=lambda: _render_calibration_staged_human_output(payload),
    )

    return int(CliExitCode.SUCCESS)


def _build_market_data_calibration_request_from_arguments(
    *,
    cli_context: CliCommandContext,
    run_id: str,
    tick_data_path: Optional[Path],
    tick_data_format: str,
    tick_data_provenance_label: str,
    exchange_code: str,
    tick_timestamp_column: str,
    tick_price_column: Optional[str],
    tick_trade_sign_column: Optional[str],
    tick_bid_column: Optional[str],
    tick_ask_column: Optional[str],
    workflow_scope: str,
    timestamp_timezone: str,
    disable_timestamp_sort: bool,
    duplicate_record_policy: Optional[str],
    tick_price_input_policy: Optional[str],
    option_quote_input_policy: Optional[str],
    execution_mode: str,
    disk_checkpoint_directory: Optional[Path],
    emit_derived_staged_execution_input: bool,
    omit_input_provenance_report: bool,
    session_timezone: Optional[str],
    session_definition_path: Optional[Path],
    session_definition_format: Optional[str],
    session_definition_provenance_label: Optional[str],
    option_surface_path: Optional[Path],
    option_surface_format: Optional[str],
    option_surface_provenance_label: Optional[str],
    option_quote_timestamp_column: Optional[str],
    option_expiry_column: Optional[str],
    option_strike_column: Optional[str],
    option_type_column: Optional[str],
    option_mid_price_column: Optional[str],
    option_bid_column: Optional[str],
    option_ask_column: Optional[str],
    option_implied_volatility_column: Optional[str],
    known_news_path: Optional[Path],
    known_news_format: Optional[str],
    known_news_provenance_label: Optional[str],
    known_news_timezone: Optional[str],
    known_news_timestamp_column: Optional[str],
    known_news_event_id_column: Optional[str],
    **_,
) -> MarketDataCalibrationRequest:
    repository_root = cli_context.repository_root
    tick_data_path = _require_path_argument(
        repository_root,
        tick_data_path,
        argument_name="tick_data_path",
    )
    session_definition_path = (
        _require_path_argument(
            repository_root,
            session_definition_path,
            argument_name="session_definition_path",
        )
        if session_definition_path is not None
        else None
    )

    disk_checkpoint_directory = (
        _resolve_path_from_repository_root(repository_root, disk_checkpoint_directory)
        if disk_checkpoint_directory is not None
        else None
    )

    option_surface_path = (
        _resolve_path_from_repository_root(repository_root, option_surface_path)
        if option_surface_path is not None
        else None
    )

    known_news_path = (
        _resolve_path_from_repository_root(repository_root, known_news_path)
        if known_news_path is not None
        else None
    )

    payload: dict[str, Any] = {
        "schema_version": CALIBRATION_MARKET_DATA_REQUEST_SCHEMA_VERSION,
        "run_id": run_id,
        "tick_data_source": {
            "path": str(tick_data_path),
            "file_format": tick_data_format,
            "provenance_label": tick_data_provenance_label,
        },
        "session_configuration": {
            "exchange_code": exchange_code,
        },
        "column_mapping": {
            "tick_data": {
                "timestamp_column": tick_timestamp_column,
                "price_column": tick_price_column,
                "trade_sign_column": tick_trade_sign_column,
                "bid_column": tick_bid_column,
                "ask_column": tick_ask_column,
            },
        },
        "workflow_scope": workflow_scope,
        "normalization_policy": {
            "timestamp_timezone": timestamp_timezone,
            "sort_records_by_timestamp": not disable_timestamp_sort,
            "tick_price_input_policy": tick_price_input_policy,
            "option_quote_input_policy": option_quote_input_policy,
        },
        "output_preferences": {
            "execution_mode": execution_mode,
            "disk_checkpoint_directory": None if disk_checkpoint_directory is None else str(disk_checkpoint_directory),
            "emit_derived_staged_execution_input": emit_derived_staged_execution_input,
            "emit_input_provenance_report": not omit_input_provenance_report,
        },
    }

    if duplicate_record_policy is not None:
        payload["normalization_policy"]["duplicate_record_policy"] = duplicate_record_policy

    if session_timezone is not None:
        payload["session_configuration"]["session_timezone"] = session_timezone

    if session_definition_path is not None:
        payload["session_configuration"]["session_definition_source"] = {
            "path": str(session_definition_path),
            "file_format": session_definition_format,
            "provenance_label": session_definition_provenance_label,
        }

    if option_surface_path is not None:
        payload["option_surface_source"] = {
            "path": str(option_surface_path),
            "file_format": option_surface_format,
            "provenance_label": option_surface_provenance_label,
        }
        payload["column_mapping"]["option_surface"] = {
            "quote_timestamp_column": option_quote_timestamp_column,
            "expiry_timestamp_column": option_expiry_column,
            "strike_column": option_strike_column,
            "option_type_column": option_type_column,
            "mid_price_column": option_mid_price_column,
            "bid_column": option_bid_column,
            "ask_column": option_ask_column,
            "implied_volatility_column": option_implied_volatility_column,
        }

    if known_news_path is not None:
        payload["known_news_inputs"] = {
            "calendar_source": {
                "path": str(known_news_path),
                "file_format": known_news_format,
                "provenance_label": known_news_provenance_label,
            },
            "calendar_timezone": known_news_timezone,
        }

        if known_news_timestamp_column is not None:
            payload["column_mapping"]["known_news"] = {
                "event_timestamp_column": known_news_timestamp_column,
                "event_identifier_column": known_news_event_id_column,
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
            "prepared_payload_path": prepared_calibration.prepared_payload_path,
            "staged_execution_input_path": prepared_calibration.staged_execution_input_path,
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


def _build_fit_plan_command(*, environment: Optional[Mapping[str, str]] = None) -> click.Command:
    @click.command(
        "plan",
        cls=JaniformCommand,
        command_path=("fit", "plan"),
        environment=environment,
        help=(
            "Validate market-data inputs, resolve presets and provenance, and build the "
            "derived fit plan without running the staged fit."
        ),
        params=_market_data_calibration_options(),
    )
    @click.pass_context
    def plan(ctx: click.Context, **command_arguments) -> int:
        return invoke_command_handler(
            ctx,
            handler=_run_calibration_market_data_command,
            command_path=("fit", "plan"),
            command_arguments=dict(command_arguments),
        )

    return plan


def _build_fit_staged_command(*, environment: Optional[Mapping[str, str]] = None) -> click.Command:
    @click.command(
        "staged",
        cls=JaniformCommand,
        command_path=("fit", "staged"),
        environment=environment,
        help="Run the explicit staged fit workflow from a prepared execution-input contract.",
        params=_staged_calibration_options(),
    )
    @click.pass_context
    def staged(ctx: click.Context, **command_arguments) -> int:
        return invoke_command_handler(
            ctx,
            handler=_run_calibration_staged_command,
            command_path=("fit", "staged"),
            command_arguments=dict(command_arguments),
        )

    return staged


def _market_data_calibration_options() -> list[click.Parameter]:
    return [
        command_config_option(),
        output_option(default_output_mode=CliOutputMode.JSON),
        command_option(
            "--run-id",
            help_text="Stable run identifier recorded in the market-data request payload.",
            required=True,
        ),
        command_option(
            "--workflow-scope",
            choice_options=CLI_CALIBRATION_MARKET_DATA_SCOPE_CHOICES,
            help_text=(
                "High-level calibration scope. Use seasonality_only for activity-only estimation, "
                "full_without_option_surface for full calibration from tick data only, and "
                "full_with_option_surface when option-surface data is supplied."
            ),
            required=True,
        ),
        command_option(
            "--tick-data-path",
            help_text="Path to the input tick-data file.",
            path_type=True,
            required=True,
        ),
        command_option(
            "--tick-data-format",
            choice_options=CLI_CALIBRATION_TICK_FILE_FORMAT_CHOICES,
            help_text="Tick-data file format.",
            required=True,
        ),
        command_option(
            "--tick-data-provenance-label",
            help_text="Stable provenance label recorded for the tick-data source.",
            required=True,
        ),
        command_option(
            "--tick-timestamp-column",
            help_text="Tick-data timestamp column name.",
            required=True,
        ),
        command_option(
            "--tick-price-input-policy",
            choice_options=CLI_CALIBRATION_TICK_PRICE_POLICY_CHOICES,
            help_text="Explicit tick-price normalization policy for non-seasonality workflows.",
        ),
        command_option("--tick-price-column", help_text="Tick-data explicit price column."),
        command_option(
            "--tick-trade-sign-column",
            help_text="Tick-data trade sign column required for canonical full-model Hawkes calibration.",
        ),
        command_option("--tick-bid-column", help_text="Tick-data bid column."),
        command_option("--tick-ask-column", help_text="Tick-data ask column."),
        command_option(
            "--exchange-code",
            help_text="Exchange code used to resolve session state and hourly factors.",
            required=True,
        ),
        optional_path_option(
            "--session-definition-path",
            help_text="Optional path to an explicit session-definition file override.",
        ),
        command_option(
            "--session-definition-format",
            choice_options=CLI_CALIBRATION_STRUCTURED_FILE_FORMAT_CHOICES,
            help_text="Explicit session-definition file format when --session-definition-path is provided.",
        ),
        command_option(
            "--session-definition-provenance-label",
            help_text="Stable provenance label recorded for the explicit session-definition source.",
        ),
        command_option(
            "--session-timezone",
            help_text="Optional timezone hint for automatic session inference or explicit session overrides.",
        ),
        optional_path_option("--known-news-path", help_text="Optional path to the known-news calendar file."),
        command_option(
            "--known-news-format",
            choice_options=CLI_CALIBRATION_STRUCTURED_FILE_FORMAT_CHOICES,
            help_text="Known-news calendar file format.",
        ),
        command_option(
            "--known-news-provenance-label",
            help_text="Stable provenance label recorded for the known-news source.",
        ),
        command_option("--known-news-timezone", help_text="Optional timezone for known-news timestamps."),
        command_option(
            "--known-news-timestamp-column",
            help_text="Known-news timestamp column name for csv or parquet inputs.",
        ),
        command_option(
            "--known-news-event-id-column",
            help_text="Optional known-news event identifier column name.",
        ),
        optional_path_option("--option-surface-path", help_text="Optional path to the option-surface file."),
        command_option(
            "--option-surface-format",
            choice_options=CLI_CALIBRATION_TICK_FILE_FORMAT_CHOICES,
            help_text="Option-surface file format.",
        ),
        command_option(
            "--option-surface-provenance-label",
            help_text="Stable provenance label recorded for the option-surface source.",
        ),
        command_option(
            "--option-quote-input-policy",
            choice_options=CLI_CALIBRATION_OPTION_QUOTE_POLICY_CHOICES,
            help_text="Explicit option-quote normalization policy.",
        ),
        command_option("--option-quote-timestamp-column", help_text="Option-surface quote timestamp column name."),
        command_option("--option-expiry-column", help_text="Option-surface expiry column name."),
        command_option("--option-strike-column", help_text="Option-surface strike column name."),
        command_option("--option-type-column", help_text="Option-surface option type column name."),
        command_option("--option-mid-price-column", help_text="Option-surface mid-price column name."),
        command_option("--option-bid-column", help_text="Option-surface bid column name."),
        command_option("--option-ask-column", help_text="Option-surface ask column name."),
        command_option(
            "--option-implied-volatility-column",
            help_text="Option-surface implied volatility column name.",
        ),
        command_option(
            "--timestamp-timezone",
            help_text="Timezone used to normalize tick-data timestamps before staged translation.",
            required=True,
        ),
        command_option(
            "--duplicate-record-policy",
            choice_options=CLI_CALIBRATION_DUPLICATE_RECORD_POLICY_CHOICES,
            help_text="Optional tick-data deduplication policy. Omit to preserve duplicate tick rows.",
        ),
        command_option(
            "--disable-timestamp-sort",
            help_text="Record that source data should not be re-sorted by timestamp before translation.",
            is_flag=True,
        ),
        command_option(
            "--execution-mode",
            choice_options=CLI_CALIBRATION_EXECUTION_MODE_CHOICES,
            default="disk",
            help_text=CLI_CALIBRATION_MARKET_DATA_EXECUTION_MODE_HELP,
        ),
        optional_path_option(
            "--disk-checkpoint-directory",
            directory=True,
            help_text=(
                "Disk checkpoint directory recorded in output_preferences. Required when "
                "--execution-mode disk is selected."
            ),
        ),
        command_option(
            "--emit-derived-staged-execution-input",
            help_text="Request emission of the derived staged execution input for replay or debugging.",
            is_flag=True,
        ),
        command_option(
            "--omit-input-provenance-report",
            help_text="Disable provenance-report emission in output_preferences.",
            is_flag=True,
        ),
    ]


def _staged_calibration_options() -> list[click.Parameter]:
    return [
        command_config_option(),
        output_option(default_output_mode=CliOutputMode.JSON),
        required_json_path_option(
            "--execution-input-json",
            contract_label="a staged calibration execution-input JSON object",
            semantic_note=(
                "The execution input must provide explicit known-news return samples and raw "
                "close-open boundary observations for internal gap detection."
            ),
        ),
        required_json_path_option(
            "--calendar-registry-json",
            contract_label="a calendar registry JSON object consumed by the staged calibration runner",
        ),
        command_option(
            "--execution-mode",
            choice_options=CLI_CALIBRATION_EXECUTION_MODE_CHOICES,
            default="disk",
            help_text=CLI_CALIBRATION_EXECUTION_MODE_HELP,
        ),
        optional_path_option(
            "--disk-checkpoint-directory",
            directory=True,
            help_text=(
                "Checkpoint directory override. Use only with --execution-mode disk. When "
                "provided, this overrides runtime_resources.disk_checkpoint_directory from "
                "--execution-input-json."
            ),
        ),
    ]


__all__ = []
