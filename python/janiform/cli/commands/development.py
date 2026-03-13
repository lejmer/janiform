from argparse import Namespace
from pathlib import Path
from typing import Any, cast

from .shared import _build_calendar_registry, _build_calendar_registry_from_document
from .._constants import (
    CLI_DETERMINISTIC_REGRESSION_ENTRYPOINT,
    CLI_DETERMINISTIC_REGRESSION_INTERNAL_ERROR_ID,
    CLI_DETERMINISTIC_REGRESSION_VALIDATION_ERROR_ID,
    CliExitCode,
)
from ..documents import _load_json_document
from ..errors import _classify_open_closed_gap_command_error, _classify_workflow_command_error
from ..paths import _resolve_path_from_repository_root, resolve_repository_root
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


def _run_deterministic_regression_command(parsed_arguments: Namespace) -> int:
    repository_root = resolve_repository_root()
    calendar_registry_path = _resolve_path_from_repository_root(
        repository_root,
        cast(Path, parsed_arguments.calendar_registry_json),
    )
    calendar_cases_path = _resolve_path_from_repository_root(
        repository_root,
        cast(Path, parsed_arguments.calendar_cases_json),
    )
    seasonality_cases_path = _resolve_path_from_repository_root(
        repository_root,
        cast(Path, parsed_arguments.seasonality_cases_json),
    )
    file_field_path = "calendar_registry_json"
    document_field_path = "calendar_registry_document"

    try:
        calendar_registry_document = _load_json_document(calendar_registry_path, "calendar registry")
        file_field_path = "calendar_cases_json"
        document_field_path = "calendar_cases_document"
        calendar_cases_document = _load_json_document(calendar_cases_path, "calendar cases")
        file_field_path = "seasonality_cases_json"
        document_field_path = "seasonality_cases_document"
        seasonality_cases_document = _load_json_document(seasonality_cases_path, "seasonality cases")
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
            seasonality_absolute_tolerance=float(parsed_arguments.seasonality_absolute_tolerance),
            news_intensity_absolute_tolerance=float(parsed_arguments.news_intensity_absolute_tolerance),
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
                file_field_path=file_field_path,
                document_field_path=document_field_path,
            )
        )

    failed_case_count = cast(dict[str, int], report["summary"])["failed_case_count"]

    if parsed_arguments.output == "json":
        _write_contract_json(report)
        return int(CliExitCode.SUCCESS if failed_case_count == 0 else CliExitCode.RUNTIME_ERROR)

    _write_deterministic_regression_human_output(report)
    return int(CliExitCode.SUCCESS if failed_case_count == 0 else CliExitCode.RUNTIME_ERROR)


def _run_open_closed_gap_leakage_regression_command(parsed_arguments: Namespace) -> int:
    repository_root = resolve_repository_root()
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

    failed_case_count = cast(dict[str, int], report["summary"])["failed_case_count"]

    if parsed_arguments.output == "json":
        _write_contract_json(report)
        return int(CliExitCode.SUCCESS if failed_case_count == 0 else CliExitCode.RUNTIME_ERROR)

    _write_open_closed_gap_human_output(report)
    return int(CliExitCode.SUCCESS if failed_case_count == 0 else CliExitCode.RUNTIME_ERROR)


__all__ = [
    "_build_calendar_registry_from_document",
    "_extract_calendar_cases",
    "_extract_gap_behavior_cases",
    "_load_json_document",
    "_run_deterministic_regression_command",
    "_run_open_closed_gap_leakage_regression_command",
]
