import json
import sys
from argparse import ArgumentParser, Namespace
from pathlib import Path
from typing import Any, cast

from . import __version__
from .calendar import CalendarRegistry
from .validation import run_open_closed_gap_leakage_regression_harness

DEFAULT_CALENDAR_FIXTURE_PATH: str = "tests/fixtures/calendar/calendar_service_regression_matrix.json"
DEFAULT_GAP_BEHAVIOR_FIXTURE_PATH: str = (
    "tests/fixtures/gap_behavior/open_closed_gap_behavior_scenarios.json"
)


def _build_parser() -> ArgumentParser:
    parser: ArgumentParser = ArgumentParser(
        prog="janiform",
        description="Command line interface for Janiform.",
    )
    parser.add_argument("--version", action="version", version=f"%(prog)s {__version__}")
    parser.set_defaults(command=None, regression_command=None)

    subparsers = parser.add_subparsers(dest="command")
    regression_parser = subparsers.add_parser(
        "regression",
        help="Run regression validation suites.",
    )
    regression_subparsers = regression_parser.add_subparsers(dest="regression_command")

    gap_leakage_parser = regression_subparsers.add_parser(
        "open-closed-gap-leakage",
        help="Run open and closed gap leakage regression harness.",
    )
    gap_leakage_parser.add_argument(
        "--calendar-fixture-path",
        type=Path,
        default=Path(DEFAULT_CALENDAR_FIXTURE_PATH),
        help="Path to calendar regression fixture JSON.",
    )
    gap_leakage_parser.add_argument(
        "--gap-behavior-fixture-path",
        type=Path,
        default=Path(DEFAULT_GAP_BEHAVIOR_FIXTURE_PATH),
        help="Path to open and closed gap behavior fixture JSON.",
    )
    gap_leakage_parser.add_argument(
        "--direct-gap-absolute-tolerance",
        type=float,
        default=1e-12,
        help="Absolute tolerance for open-window direct gap leakage detection.",
    )

    return parser


def main() -> None:
    parser: ArgumentParser = _build_parser()
    parsed_arguments: Namespace = parser.parse_args()

    match parsed_arguments.command, parsed_arguments.regression_command:
        case "regression", "open-closed-gap-leakage":
            raise SystemExit(_run_open_closed_gap_leakage_regression_command(parsed_arguments))
        case _:
            return


def _run_open_closed_gap_leakage_regression_command(parsed_arguments: Namespace) -> int:
    repository_root = Path(__file__).resolve().parents[2]
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
            direct_gap_absolute_tolerance=float(parsed_arguments.direct_gap_absolute_tolerance),
        )
    except Exception as error:
        sys.stderr.write(f"open closed gap leakage regression command failed: {error}\n")
        return 1

    summary = report["summary"]
    behavior_diff_report_summary = cast(
        dict[str, int],
        cast(dict[str, object], report["behavior_diff_report"])["summary"],
    )
    if summary["failed_case_count"] == 0:
        sys.stdout.write(
            "open closed gap leakage regression passed: "
            f"{summary['passed_case_count']}/{summary['total_case_count']} scenarios, "
            f"open_window_events={summary['total_open_window_event_count']}, "
            f"diff_regression_cases={behavior_diff_report_summary['regression_case_count']}\n",
        )
        return 0

    sys.stderr.write(
        "open closed gap leakage regression failed: "
        f"{summary['failed_case_count']}/{summary['total_case_count']} scenarios, "
        f"leakage_failures={summary['total_leakage_failure_count']}\n",
    )

    for case_report in report["case_reports"]:
        if case_report["status"] != "fail":
            continue

        sys.stderr.write(
            f"scenario_id={case_report['fixture_id']} exchange_code={case_report['exchange_code']} "
            f"leakage_failure_count={case_report['leakage_failure_count']} "
            f"max_absolute_leakage_magnitude={case_report['max_absolute_leakage_magnitude']}\n",
        )

        contribution_summary = cast(dict[str, object], case_report["contribution_summary"])

        sys.stderr.write(
            "  contributions "
            f"activity_expected={contribution_summary['expected_activity_coupling_j_t_increment_total']} "
            f"activity_observed={contribution_summary['observed_activity_coupling_j_t_increment_total']} "
            f"open_gap_expected={contribution_summary['expected_open_window_direct_gap_g_t_increment_total']} "
            f"open_gap_observed={contribution_summary['observed_open_window_direct_gap_g_t_increment_total']} "
            f"closed_gap_expected={contribution_summary['expected_closed_window_direct_gap_g_t_increment_total']} "
            f"closed_gap_observed={contribution_summary['observed_closed_window_direct_gap_g_t_increment_total']}\n",
        )

        behavior_diff = cast(dict[str, object], case_report["behavior_diff"])
        for diff_metric in cast(list[dict[str, object]], behavior_diff["metrics"]):
            if not cast(bool, diff_metric["regression_detected"]):
                continue
            sys.stderr.write(
                "  diff_regression "
                f"metric_id={diff_metric['metric_id']} "
                f"expected={diff_metric['expected_value']} "
                f"observed={diff_metric['observed_value']} "
                f"absolute_difference={diff_metric['absolute_difference']} "
                f"absolute_tolerance={diff_metric['absolute_tolerance']}\n",
            )

        for failure in case_report["failures"]:
            failure_kind = cast(str, failure["failure_kind"])
            match failure_kind:
                case "open_window_direct_gap_leakage":
                    sys.stderr.write(
                        f"  failure_kind={failure_kind} event_id={failure['event_id']} "
                        f"event_timestamp_utc={failure['event_timestamp_utc']} "
                        f"absolute_leakage_magnitude={failure['absolute_leakage_magnitude']} "
                        f"observed_direct_gap_increment={failure['observed_direct_gap_increment']}\n",
                    )
                case "dispatch_error":
                    sys.stderr.write(
                        f"  failure_kind={failure_kind} error_message={failure['error_message']}\n",
                    )
                case _:
                    sys.stderr.write(f"  failure_kind={failure_kind}\n")

    return 1


def _resolve_path_from_repository_root(repository_root: Path, path: Path) -> Path:
    if path.is_absolute():
        return path

    return repository_root / path


def _load_json_document(path: Path, context: str) -> dict[str, object]:
    payload = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(payload, dict):
        raise ValueError(f"{context} at {path} must decode to a JSON object")

    return cast(dict[str, object], payload)


def _build_calendar_registry(calendar_fixture_document: dict[str, object]) -> CalendarRegistry:
    calendar_registry_payload: object = calendar_fixture_document.get("calendar_registry")
    if not isinstance(calendar_registry_payload, dict):
        raise ValueError("calendar fixture must include object field `calendar_registry`")

    return CalendarRegistry(json.dumps(calendar_registry_payload))


def _extract_gap_behavior_cases(
    gap_behavior_fixture_document: dict[str, object],
) -> list[dict[str, object]]:
    raw_cases: object = gap_behavior_fixture_document.get("cases")
    if not isinstance(raw_cases, list):
        raise ValueError("gap behavior fixture must include list field `cases`")

    parsed_cases: list[dict[str, object]] = []
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
                "initial_j_t": initial_accumulator_state.get("j_t"),
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
                        "coupled_gap_jump_mark": (
                            raw_event.get("coupled_gap_jump_mark")
                            if isinstance(raw_event, dict)
                            else None
                        ),
                    }
                    for raw_event in injected_events
                ],
            },
        )

    return parsed_cases
