import json
from typing import cast

from janiform.validation import (
    DETERMINISTIC_REGRESSION_HARNESS_REPORT_SCHEMA_VERSION,
    run_deterministic_regression_harness,
)

type CalendarRegressionCase = dict[str, object]
type HourlyCanonicalOutputCase = dict[str, object]
type RegressionHarnessReport = dict[str, object]


def test_deterministic_regression_harness_matches_shared_fixtures(
    calendar_registry,
    calendar_regression_cases: list[CalendarRegressionCase],
    hourly_factor_regression_fixture_document: dict[str, object],
    hourly_factor_regression_canonical_cases: list[HourlyCanonicalOutputCase],
) -> None:
    acceptance_thresholds: dict[str, float] = cast(
        dict[str, float],
        hourly_factor_regression_fixture_document["acceptance_thresholds"],
    )
    report = run_deterministic_regression_harness(
        calendar_registry=calendar_registry,
        calendar_cases=calendar_regression_cases,
        seasonality_cases=hourly_factor_regression_canonical_cases,
        seasonality_absolute_tolerance=acceptance_thresholds["s_h_absolute_tolerance"],
        news_intensity_absolute_tolerance=acceptance_thresholds["nu_h_absolute_tolerance"],
    )

    summary: dict[str, int] = cast(dict[str, int], report["summary"])
    case_reports: list[dict[str, object]] = cast(list[dict[str, object]], report["case_reports"])

    assert report["schema_version"] == DETERMINISTIC_REGRESSION_HARNESS_REPORT_SCHEMA_VERSION
    assert summary["total_case_count"] == len(calendar_regression_cases) + len(
        hourly_factor_regression_canonical_cases,
    )
    assert summary["failed_case_count"] == 0
    assert summary["passed_case_count"] == summary["total_case_count"]
    assert all(cast(str, case_report["status"]) == "pass" for case_report in case_reports)


def test_deterministic_regression_harness_reports_fixture_mismatches(
    calendar_registry,
    calendar_regression_cases: list[CalendarRegressionCase],
    hourly_factor_regression_fixture_document: dict[str, object],
    hourly_factor_regression_canonical_cases: list[HourlyCanonicalOutputCase],
) -> None:
    acceptance_thresholds: dict[str, float] = cast(
        dict[str, float],
        hourly_factor_regression_fixture_document["acceptance_thresholds"],
    )
    mutated_calendar_cases = _copy_case_list(calendar_regression_cases)
    mutated_seasonality_cases = _copy_case_list(hourly_factor_regression_canonical_cases)

    calendar_failure_fixture_id: str = cast(str, mutated_calendar_cases[0]["fixture_id"])
    seasonality_failure_fixture_id: str = cast(str, mutated_seasonality_cases[0]["fixture_id"])
    current_expected_state: str = cast(str, mutated_calendar_cases[0]["expected_state"])
    mutated_calendar_cases[0]["expected_state"] = "closed" if current_expected_state == "open" else "open"
    expected_s_h = cast(list[float], mutated_seasonality_cases[0]["expected_s_h"])
    expected_s_h[0] = float(expected_s_h[0]) + 0.25

    report = run_deterministic_regression_harness(
        calendar_registry=calendar_registry,
        calendar_cases=mutated_calendar_cases,
        seasonality_cases=mutated_seasonality_cases,
        seasonality_absolute_tolerance=acceptance_thresholds["s_h_absolute_tolerance"],
        news_intensity_absolute_tolerance=acceptance_thresholds["nu_h_absolute_tolerance"],
    )

    summary: dict[str, int] = cast(dict[str, int], report["summary"])
    calendar_failure_report = _case_report_by_fixture_id(report, calendar_failure_fixture_id)
    seasonality_failure_report = _case_report_by_fixture_id(report, seasonality_failure_fixture_id)

    assert summary["failed_case_count"] == 2
    assert cast(str, calendar_failure_report["status"]) == "fail"
    assert cast(dict[str, object], calendar_failure_report["failure"])["failure_kind"] == "market_state_mismatch"
    assert cast(str, seasonality_failure_report["status"]) == "fail"
    assert cast(dict[str, object], seasonality_failure_report["failure"])["failure_kind"] == (
        "factor_tolerance_exceeded"
    )


def test_deterministic_regression_harness_report_is_repeatable(
    calendar_registry,
    calendar_regression_cases: list[CalendarRegressionCase],
    hourly_factor_regression_fixture_document: dict[str, object],
    hourly_factor_regression_canonical_cases: list[HourlyCanonicalOutputCase],
) -> None:
    acceptance_thresholds: dict[str, float] = cast(
        dict[str, float],
        hourly_factor_regression_fixture_document["acceptance_thresholds"],
    )
    first_report = run_deterministic_regression_harness(
        calendar_registry=calendar_registry,
        calendar_cases=calendar_regression_cases,
        seasonality_cases=hourly_factor_regression_canonical_cases,
        seasonality_absolute_tolerance=acceptance_thresholds["s_h_absolute_tolerance"],
        news_intensity_absolute_tolerance=acceptance_thresholds["nu_h_absolute_tolerance"],
    )
    second_report = run_deterministic_regression_harness(
        calendar_registry=calendar_registry,
        calendar_cases=calendar_regression_cases,
        seasonality_cases=hourly_factor_regression_canonical_cases,
        seasonality_absolute_tolerance=acceptance_thresholds["s_h_absolute_tolerance"],
        news_intensity_absolute_tolerance=acceptance_thresholds["nu_h_absolute_tolerance"],
    )

    assert first_report == second_report
    assert json.dumps(first_report, sort_keys=True) == json.dumps(second_report, sort_keys=True)


def _case_report_by_fixture_id(
    report: RegressionHarnessReport,
    fixture_id: str,
) -> dict[str, object]:
    case_reports: list[dict[str, object]] = cast(list[dict[str, object]], report["case_reports"])
    for case_report in case_reports:
        if cast(str, case_report["fixture_id"]) == fixture_id:
            return case_report

    raise ValueError(f"missing case report for fixture_id: {fixture_id}")


def _copy_case_list(cases: list[dict[str, object]]) -> list[dict[str, object]]:
    return cast(list[dict[str, object]], json.loads(json.dumps(cases)))
