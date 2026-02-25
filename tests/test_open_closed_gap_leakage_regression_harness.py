import json
from typing import cast

from janiform.validation import (
    OPEN_CLOSED_GAP_LEAKAGE_REGRESSION_HARNESS_DETERMINISM_POLICY,
    OPEN_CLOSED_GAP_LEAKAGE_REGRESSION_HARNESS_REPORT_SCHEMA_VERSION,
    run_open_closed_gap_leakage_regression_harness,
)
from tests.fixture_validation import OpenClosedGapBehaviorCase

type GapLeakageCaseInput = dict[str, object]
type GapLeakageHarnessReport = dict[str, object]

OPEN_WINDOW_SCENARIO_ID: str = "xnys_intraday_open_window_gap_suppression"


def test_open_closed_gap_leakage_regression_harness_matches_shared_fixture_suite(
    calendar_registry,
    open_closed_gap_behavior_cases: list[OpenClosedGapBehaviorCase],
) -> None:
    gap_behavior_cases = _to_gap_behavior_case_inputs(open_closed_gap_behavior_cases)
    report = run_open_closed_gap_leakage_regression_harness(
        calendar_registry=calendar_registry,
        gap_behavior_cases=gap_behavior_cases,
    )

    summary: dict[str, int] = cast(dict[str, int], report["summary"])
    case_reports: list[dict[str, object]] = cast(list[dict[str, object]], report["case_reports"])

    assert report["schema_version"] == OPEN_CLOSED_GAP_LEAKAGE_REGRESSION_HARNESS_REPORT_SCHEMA_VERSION
    assert report["determinism_policy"] == OPEN_CLOSED_GAP_LEAKAGE_REGRESSION_HARNESS_DETERMINISM_POLICY
    assert summary["total_case_count"] == len(gap_behavior_cases)
    assert summary["failed_case_count"] == 0
    assert summary["passed_case_count"] == summary["total_case_count"]
    assert summary["total_leakage_failure_count"] == 0
    assert all(cast(str, case_report["status"]) == "pass" for case_report in case_reports)


def test_open_closed_gap_leakage_regression_harness_reports_scenario_and_magnitude_for_failures(
    calendar_registry,
    open_closed_gap_behavior_cases: list[OpenClosedGapBehaviorCase],
) -> None:
    gap_behavior_cases = _to_gap_behavior_case_inputs(open_closed_gap_behavior_cases)
    open_window_case = _case_input_by_fixture_id(gap_behavior_cases, OPEN_WINDOW_SCENARIO_ID)
    expected_event_id = cast(str, cast(list[dict[str, object]], open_window_case["injected_events"])[0]["event_id"])

    cast(list[dict[str, object]], open_window_case["injected_events"])[0][
        "observed_direct_gap_increment_override"
    ] = 0.19

    report = run_open_closed_gap_leakage_regression_harness(
        calendar_registry=calendar_registry,
        gap_behavior_cases=[open_window_case],
    )

    summary: dict[str, int] = cast(dict[str, int], report["summary"])
    assert summary["total_case_count"] == 1
    assert summary["failed_case_count"] == 1
    assert summary["total_leakage_failure_count"] == 1

    case_report = cast(dict[str, object], cast(list[dict[str, object]], report["case_reports"])[0])
    failures: list[dict[str, object]] = cast(list[dict[str, object]], case_report["failures"])
    leakage_failure = cast(
        dict[str, object],
        next(
            failure
            for failure in failures
            if cast(str, failure["failure_kind"]) == "open_window_direct_gap_leakage"
        ),
    )

    assert case_report["fixture_id"] == OPEN_WINDOW_SCENARIO_ID
    assert leakage_failure["event_id"] == expected_event_id
    assert abs(cast(float, leakage_failure["absolute_leakage_magnitude"]) - 0.19) <= 1e-12
    assert abs(cast(float, leakage_failure["observed_direct_gap_increment"]) - 0.19) <= 1e-12


def test_open_closed_gap_leakage_regression_harness_report_is_repeatable(
    calendar_registry,
    open_closed_gap_behavior_cases: list[OpenClosedGapBehaviorCase],
) -> None:
    gap_behavior_cases = _to_gap_behavior_case_inputs(open_closed_gap_behavior_cases)

    first_report = run_open_closed_gap_leakage_regression_harness(
        calendar_registry=calendar_registry,
        gap_behavior_cases=gap_behavior_cases,
    )
    second_report = run_open_closed_gap_leakage_regression_harness(
        calendar_registry=calendar_registry,
        gap_behavior_cases=gap_behavior_cases,
    )

    assert first_report == second_report
    assert json.dumps(first_report, sort_keys=True) == json.dumps(second_report, sort_keys=True)


def _to_gap_behavior_case_inputs(
    fixture_cases: list[OpenClosedGapBehaviorCase],
) -> list[GapLeakageCaseInput]:
    return [
        {
            "fixture_id": fixture_case["fixture_id"],
            "exchange_code": fixture_case["exchange_code"],
            "initial_j_t": fixture_case["initial_accumulator_state"]["j_t"],
            "initial_g_t": fixture_case["initial_accumulator_state"]["g_t"],
            "injected_events": [
                {
                    "event_id": injected_event["event_id"],
                    "timestamp_utc": injected_event["timestamp_utc"],
                    "shared_severity_mark": injected_event["shared_severity_mark"],
                    "coupled_gap_jump_mark": injected_event["coupled_gap_jump_mark"],
                }
                for injected_event in fixture_case["injected_events"]
            ],
        }
        for fixture_case in fixture_cases
    ]


def _case_input_by_fixture_id(
    case_inputs: list[GapLeakageCaseInput],
    fixture_id: str,
) -> GapLeakageCaseInput:
    return cast(
        GapLeakageCaseInput,
        next(case_input for case_input in case_inputs if cast(str, case_input["fixture_id"]) == fixture_id),
    )
