from collections.abc import Mapping
from pathlib import Path
from typing import Any, Optional, cast

import click

from .shared import _build_calendar_registry, _build_calendar_registry_from_document
from ..command_runtime import (
    JaniformCommand,
    JaniformGroup,
    command_option,
    group_help_callback,
    invoke_command_handler,
    optional_path_option,
    output_option,
    required_json_path_option,
)
from ..context import CliCommandContext
from .._constants import (
    CLI_DETERMINISTIC_REGRESSION_ENTRYPOINT,
    CLI_DETERMINISTIC_REGRESSION_INTERNAL_ERROR_ID,
    CLI_DETERMINISTIC_REGRESSION_VALIDATION_ERROR_ID,
    CLI_OPEN_CLOSED_GAP_ENTRYPOINT,
    CLI_OPEN_CLOSED_GAP_VALIDATION_ERROR_ID,
    CliExitCode,
    DEFAULT_CALENDAR_FIXTURE_PATH,
    DEFAULT_GAP_BEHAVIOR_FIXTURE_PATH,
    CliOutputMode,
)
from ..documents import _load_json_document_for_cli_field
from ..errors import _classify_open_closed_gap_command_error, _classify_workflow_command_error
from ..rendering import (
    _write_cli_error,
    _write_contract_json,
    _write_deterministic_regression_human_output,
    _write_open_closed_gap_human_output,
)
from ...errors import JaniformError
from ...validation import run_deterministic_regression_harness, run_open_closed_gap_leakage_regression_harness


def _extract_gap_behavior_cases(gap_behavior_fixture_document: dict[str, Any]) -> list[dict[str, Any]]:
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
                        "event_id": raw_event.get("event_id") if isinstance(raw_event, dict) else None,
                        "timestamp_utc": raw_event.get("timestamp_utc") if isinstance(raw_event, dict) else None,
                        "shared_severity_mark": (
                            raw_event.get("shared_severity_mark") if isinstance(raw_event, dict) else None
                        ),
                        "news_arrival_component": (
                            raw_event.get("news_arrival_component") if isinstance(raw_event, dict) else None
                        ),
                        "coupled_return_jump_mark": (
                            raw_event.get("coupled_return_jump_mark") if isinstance(raw_event, dict) else None
                        ),
                    }
                    for raw_event in injected_events
                ],
            }
        )
    return parsed_cases


def _extract_calendar_cases(calendar_cases_document: dict[str, Any]) -> list[dict[str, Any]]:
    raw_cases: object = calendar_cases_document.get("cases")
    if not isinstance(raw_cases, list):
        raise ValueError("calendar cases document must include list field `cases`")

    parsed_cases: list[dict[str, Any]] = []
    for raw_case in raw_cases:
        if not isinstance(raw_case, dict):
            raise ValueError("calendar cases document case must be an object")

        parsed_cases.append(cast(dict[str, Any], raw_case))

    return parsed_cases


def _extract_seasonality_cases(seasonality_cases_document: dict[str, Any]) -> list[dict[str, Any]]:
    raw_cases: object = seasonality_cases_document.get("canonical_output_cases")
    if not isinstance(raw_cases, list):
        raise ValueError("seasonality cases document must include list field `canonical_output_cases`")

    parsed_cases: list[dict[str, Any]] = []
    for raw_case in raw_cases:
        if not isinstance(raw_case, dict):
            raise ValueError("seasonality cases document case must be an object")

        parsed_cases.append(cast(dict[str, Any], raw_case))

    return parsed_cases


def _run_deterministic_regression_command(
    *,
    cli_context: CliCommandContext,
    output: str,
    calendar_registry_json: Path,
    calendar_cases_json: Path,
    seasonality_cases_json: Path,
    seasonality_absolute_tolerance: float,
    news_intensity_absolute_tolerance: float,
    **_,
) -> int:
    document_field_path = "calendar_registry_document"

    try:
        calendar_registry_document = _load_json_document_for_cli_field(
            repository_root=cli_context.repository_root,
            path=calendar_registry_json,
            context="calendar registry",
            field_path="calendar_registry_json",
            error_id=CLI_DETERMINISTIC_REGRESSION_VALIDATION_ERROR_ID,
            entrypoint=CLI_DETERMINISTIC_REGRESSION_ENTRYPOINT,
        )
        document_field_path = "calendar_cases_document"
        calendar_cases_document = _load_json_document_for_cli_field(
            repository_root=cli_context.repository_root,
            path=calendar_cases_json,
            context="calendar cases",
            field_path="calendar_cases_json",
            error_id=CLI_DETERMINISTIC_REGRESSION_VALIDATION_ERROR_ID,
            entrypoint=CLI_DETERMINISTIC_REGRESSION_ENTRYPOINT,
        )
        document_field_path = "seasonality_cases_document"
        seasonality_cases_document = _load_json_document_for_cli_field(
            repository_root=cli_context.repository_root,
            path=seasonality_cases_json,
            context="seasonality cases",
            field_path="seasonality_cases_json",
            error_id=CLI_DETERMINISTIC_REGRESSION_VALIDATION_ERROR_ID,
            entrypoint=CLI_DETERMINISTIC_REGRESSION_ENTRYPOINT,
        )
        document_field_path = "calendar_registry_document"
        calendar_registry = _build_calendar_registry_from_document(calendar_registry_document)
        document_field_path = "calendar_cases_document"
        calendar_cases = _extract_calendar_cases(calendar_cases_document)
        document_field_path = "seasonality_cases_document"
        seasonality_cases = _extract_seasonality_cases(seasonality_cases_document)
        report = run_deterministic_regression_harness(
            calendar_registry=calendar_registry,
            calendar_cases=calendar_cases,
            seasonality_cases=seasonality_cases,
            seasonality_absolute_tolerance=seasonality_absolute_tolerance,
            news_intensity_absolute_tolerance=news_intensity_absolute_tolerance,
        )
    except JaniformError as e:
        return _write_cli_error(e)
    except Exception as e:
        return _write_cli_error(
            _classify_workflow_command_error(
                e,
                entrypoint=CLI_DETERMINISTIC_REGRESSION_ENTRYPOINT,
                validation_error_id=CLI_DETERMINISTIC_REGRESSION_VALIDATION_ERROR_ID,
                internal_error_id=CLI_DETERMINISTIC_REGRESSION_INTERNAL_ERROR_ID,
                file_field_path="calendar_registry_json",
                document_field_path=document_field_path,
            )
        )

    failed_case_count = cast(dict[str, int], report["summary"])["failed_case_count"]

    if output == "json":
        _write_contract_json(report)
        return int(CliExitCode.SUCCESS if failed_case_count == 0 else CliExitCode.RUNTIME_ERROR)

    _write_deterministic_regression_human_output(report)
    return int(CliExitCode.SUCCESS if failed_case_count == 0 else CliExitCode.RUNTIME_ERROR)


def _run_open_closed_gap_leakage_regression_command(
    *,
    cli_context: CliCommandContext,
    output: str,
    calendar_fixture_path: Path,
    gap_behavior_fixture_path: Path,
    component_absolute_tolerance: float,
    **_,
) -> int:
    try:
        calendar_fixture_document = _load_json_document_for_cli_field(
            repository_root=cli_context.repository_root,
            path=calendar_fixture_path,
            context="calendar fixture",
            field_path="calendar_fixture_path",
            error_id=CLI_OPEN_CLOSED_GAP_VALIDATION_ERROR_ID,
            entrypoint=CLI_OPEN_CLOSED_GAP_ENTRYPOINT,
        )
        gap_behavior_fixture_document = _load_json_document_for_cli_field(
            repository_root=cli_context.repository_root,
            path=gap_behavior_fixture_path,
            context="open closed gap behavior fixture",
            field_path="gap_behavior_fixture_path",
            error_id=CLI_OPEN_CLOSED_GAP_VALIDATION_ERROR_ID,
            entrypoint=CLI_OPEN_CLOSED_GAP_ENTRYPOINT,
        )
        calendar_registry = _build_calendar_registry(calendar_fixture_document)
        gap_behavior_cases = _extract_gap_behavior_cases(gap_behavior_fixture_document)
        report = run_open_closed_gap_leakage_regression_harness(
            calendar_registry=calendar_registry,
            gap_behavior_cases=gap_behavior_cases,
            component_absolute_tolerance=component_absolute_tolerance,
        )
    except JaniformError as e:
        return _write_cli_error(e)
    except Exception as e:
        return _write_cli_error(_classify_open_closed_gap_command_error(e))

    failed_case_count = cast(dict[str, int], report["summary"])["failed_case_count"]

    if output == "json":
        _write_contract_json(report)
        return int(CliExitCode.SUCCESS if failed_case_count == 0 else CliExitCode.RUNTIME_ERROR)

    _write_open_closed_gap_human_output(report)
    return int(CliExitCode.SUCCESS if failed_case_count == 0 else CliExitCode.RUNTIME_ERROR)


def _build_development_group(*, environment: Optional[Mapping[str, str]] = None) -> click.Group:
    @click.group(
        cls=JaniformGroup,
        command_path=("dev",),
        environment=environment,
        help="Developer-only utilities.",
        invoke_without_command=True,
    )
    @click.pass_context
    def dev(ctx: click.Context) -> None:
        group_help_callback(ctx)

    @dev.group(
        "regression",
        cls=JaniformGroup,
        command_path=("dev", "regression"),
        environment=environment,
        help="Regression validation suites.",
        invoke_without_command=True,
    )
    @click.pass_context
    def regression(ctx: click.Context) -> None:
        group_help_callback(ctx)

    @regression.command(
        "deterministic",
        cls=JaniformCommand,
        command_path=("dev", "regression", "deterministic"),
        environment=environment,
        help="Run deterministic calendar and hourly-factor regression harness.",
        params=[
            output_option(default_output_mode=CliOutputMode.HUMAN),
            required_json_path_option(
                "--calendar-registry-json",
                contract_label="a calendar registry JSON object consumed by the deterministic regression harness",
            ),
            required_json_path_option(
                "--calendar-cases-json",
                contract_label="a calendar regression case document with a top-level `cases` list",
            ),
            required_json_path_option(
                "--seasonality-cases-json",
                contract_label="an hourly-factor regression case document with a top-level `canonical_output_cases` list",
            ),
            command_option(
                "--seasonality-absolute-tolerance",
                default=1e-12,
                value_type=float,
                help_text="Absolute tolerance for deterministic seasonality factor mismatch detection.",
            ),
            command_option(
                "--news-intensity-absolute-tolerance",
                default=1e-12,
                value_type=float,
                help_text="Absolute tolerance for deterministic news-intensity mismatch detection.",
            ),
        ],
    )
    @click.pass_context
    def deterministic(ctx: click.Context, **command_arguments) -> int:
        return invoke_command_handler(
            ctx,
            handler=_run_deterministic_regression_command,
            command_path=("dev", "regression", "deterministic"),
            command_arguments=dict(command_arguments),
        )

    @regression.command(
        "open-closed-gap-leakage",
        cls=JaniformCommand,
        command_path=("dev", "regression", "open-closed-gap-leakage"),
        environment=environment,
        help="Run open and closed gap leakage regression harness.",
        params=[
            output_option(default_output_mode=CliOutputMode.HUMAN),
            optional_path_option(
                "--calendar-fixture-path",
                default=Path(DEFAULT_CALENDAR_FIXTURE_PATH),
                help_text="Path to the calendar regression fixture JSON.",
            ),
            optional_path_option(
                "--gap-behavior-fixture-path",
                default=Path(DEFAULT_GAP_BEHAVIOR_FIXTURE_PATH),
                help_text="Path to the open and closed gap behavior fixture JSON.",
            ),
            command_option(
                "--component-absolute-tolerance",
                default=1e-12,
                value_type=float,
                help_text="Absolute tolerance for component-level semantic mismatch detection.",
            ),
        ],
    )
    @click.pass_context
    def open_closed_gap_leakage(ctx: click.Context, **command_arguments) -> int:
        return invoke_command_handler(
            ctx,
            handler=_run_open_closed_gap_leakage_regression_command,
            command_path=("dev", "regression", "open-closed-gap-leakage"),
            command_arguments=dict(command_arguments),
        )

    return dev


__all__ = []
