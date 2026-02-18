import json
from typing import cast

import pytest

import janiform.validation as validation_module
from janiform.seasonality import SeasonalityInputValidationError
from janiform.validation import (
    DETERMINISTIC_REGRESSION_HARNESS_REPORT_SCHEMA_VERSION,
    run_deterministic_regression_harness,
)
from tests.fixture_validation import (
    CalendarRegressionCase,
    HourlyFactorRegressionCanonicalOutputCase,
    HourlyFactorRegressionFixtureDocument,
    STATE_CLASSIFICATION_STRICTNESS_POLICY,
)

type HourlyCanonicalOutputCase = HourlyFactorRegressionCanonicalOutputCase
type RegressionHarnessReport = dict[str, object]


def test_deterministic_regression_harness_matches_shared_fixtures(
    calendar_registry,
    calendar_regression_cases: list[CalendarRegressionCase],
    hourly_factor_regression_fixture_document: HourlyFactorRegressionFixtureDocument,
    hourly_factor_regression_canonical_cases: list[HourlyCanonicalOutputCase],
) -> None:
    tolerance_config = _harness_tolerance_config(hourly_factor_regression_fixture_document)
    report = run_deterministic_regression_harness(
        calendar_registry=calendar_registry,
        calendar_cases=calendar_regression_cases,
        seasonality_cases=hourly_factor_regression_canonical_cases,
        seasonality_absolute_tolerance=tolerance_config["s_h_absolute_tolerance"],
        news_intensity_absolute_tolerance=tolerance_config["nu_h_absolute_tolerance"],
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
    hourly_factor_regression_fixture_document: HourlyFactorRegressionFixtureDocument,
    hourly_factor_regression_canonical_cases: list[HourlyCanonicalOutputCase],
) -> None:
    threshold_configuration = _threshold_configuration(hourly_factor_regression_fixture_document)
    assert (
        cast(str, threshold_configuration["state_classification_strictness"])
        == STATE_CLASSIFICATION_STRICTNESS_POLICY
    )
    tolerance_config = _harness_tolerance_config(hourly_factor_regression_fixture_document)
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
        seasonality_absolute_tolerance=tolerance_config["s_h_absolute_tolerance"],
        news_intensity_absolute_tolerance=tolerance_config["nu_h_absolute_tolerance"],
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
    hourly_factor_regression_fixture_document: HourlyFactorRegressionFixtureDocument,
    hourly_factor_regression_canonical_cases: list[HourlyCanonicalOutputCase],
) -> None:
    tolerance_config = _harness_tolerance_config(hourly_factor_regression_fixture_document)
    first_report = run_deterministic_regression_harness(
        calendar_registry=calendar_registry,
        calendar_cases=calendar_regression_cases,
        seasonality_cases=hourly_factor_regression_canonical_cases,
        seasonality_absolute_tolerance=tolerance_config["s_h_absolute_tolerance"],
        news_intensity_absolute_tolerance=tolerance_config["nu_h_absolute_tolerance"],
    )
    second_report = run_deterministic_regression_harness(
        calendar_registry=calendar_registry,
        calendar_cases=calendar_regression_cases,
        seasonality_cases=hourly_factor_regression_canonical_cases,
        seasonality_absolute_tolerance=tolerance_config["s_h_absolute_tolerance"],
        news_intensity_absolute_tolerance=tolerance_config["nu_h_absolute_tolerance"],
    )

    assert first_report == second_report
    assert json.dumps(first_report, sort_keys=True) == json.dumps(second_report, sort_keys=True)


def test_deterministic_regression_harness_rejects_invalid_seasonality_timestamp_key(
    calendar_registry,
    calendar_regression_cases: list[CalendarRegressionCase],
    hourly_factor_regression_canonical_cases: list[HourlyCanonicalOutputCase],
) -> None:
    malformed_seasonality_cases = _copy_case_list(hourly_factor_regression_canonical_cases)
    malformed_source: dict[str, object] = cast(
        dict[str, object],
        malformed_seasonality_cases[0]["seasonality_source"],
    )
    malformed_source["source_timestamp_utc"] = malformed_source.pop("source_timestamps_utc")

    with pytest.raises(
        SeasonalityInputValidationError,
        match="source_timestamps_utc",
    ):
        run_deterministic_regression_harness(
            calendar_registry=calendar_registry,
            calendar_cases=calendar_regression_cases,
            seasonality_cases=malformed_seasonality_cases,
        )


def test_deterministic_regression_harness_threshold_boundary_is_inclusive(
    calendar_registry,
    calendar_regression_cases: list[CalendarRegressionCase],
    hourly_factor_regression_fixture_document: HourlyFactorRegressionFixtureDocument,
    hourly_factor_regression_canonical_cases: list[HourlyCanonicalOutputCase],
) -> None:
    tolerance_config = _harness_tolerance_config(hourly_factor_regression_fixture_document)
    seasonality_tolerance = 0.25
    boundary_mutated_cases = _copy_case_list(hourly_factor_regression_canonical_cases)
    boundary_mutated_expected_s_h = cast(list[float], boundary_mutated_cases[0]["expected_s_h"])
    boundary_mutated_expected_s_h[0] = float(boundary_mutated_expected_s_h[0]) + seasonality_tolerance

    report = run_deterministic_regression_harness(
        calendar_registry=calendar_registry,
        calendar_cases=calendar_regression_cases,
        seasonality_cases=boundary_mutated_cases,
        seasonality_absolute_tolerance=seasonality_tolerance,
        news_intensity_absolute_tolerance=tolerance_config["nu_h_absolute_tolerance"],
    )
    summary: dict[str, int] = cast(dict[str, int], report["summary"])

    assert summary["failed_case_count"] == 0


def test_deterministic_regression_harness_threshold_boundary_fails_when_exceeded(
    calendar_registry,
    calendar_regression_cases: list[CalendarRegressionCase],
    hourly_factor_regression_fixture_document: HourlyFactorRegressionFixtureDocument,
    hourly_factor_regression_canonical_cases: list[HourlyCanonicalOutputCase],
) -> None:
    tolerance_config = _harness_tolerance_config(hourly_factor_regression_fixture_document)
    seasonality_tolerance = 0.25
    boundary_mutated_cases = _copy_case_list(hourly_factor_regression_canonical_cases)
    boundary_mutated_expected_s_h = cast(list[float], boundary_mutated_cases[0]["expected_s_h"])
    boundary_mutated_expected_s_h[0] = float(boundary_mutated_expected_s_h[0]) + seasonality_tolerance + 1e-4

    report = run_deterministic_regression_harness(
        calendar_registry=calendar_registry,
        calendar_cases=calendar_regression_cases,
        seasonality_cases=boundary_mutated_cases,
        seasonality_absolute_tolerance=seasonality_tolerance,
        news_intensity_absolute_tolerance=tolerance_config["nu_h_absolute_tolerance"],
    )
    summary: dict[str, int] = cast(dict[str, int], report["summary"])

    assert summary["failed_case_count"] == 1


def test_deterministic_regression_harness_rejects_malformed_report_payload(
    monkeypatch: pytest.MonkeyPatch,
    calendar_registry,
    calendar_regression_cases: list[CalendarRegressionCase],
    hourly_factor_regression_canonical_cases: list[HourlyCanonicalOutputCase],
) -> None:
    class FakeNativeModule:
        def run_deterministic_regression_harness(
            self,
            _calendar_registry: object,
            _calendar_cases: list[dict[str, object]],
            _seasonality_cases: list[dict[str, object]],
            *,
            seasonality_absolute_tolerance: float,
            news_intensity_absolute_tolerance: float,
        ) -> str:
            return json.dumps(
                {
                    "schema_version": DETERMINISTIC_REGRESSION_HARNESS_REPORT_SCHEMA_VERSION,
                    "determinism_policy": "stable_input_order_and_first_mismatch_reporting.v1",
                    "tolerance_config": {
                        "seasonality_absolute_tolerance": seasonality_absolute_tolerance,
                        "news_intensity_absolute_tolerance": news_intensity_absolute_tolerance,
                    },
                    "summary": {
                        "total_case_count": 1,
                        "passed_case_count": 1,
                    },
                    "case_reports": [],
                },
            )

    monkeypatch.setattr(validation_module, "native_module", lambda: FakeNativeModule())

    with pytest.raises(SeasonalityInputValidationError, match="summary.failed_case_count"):
        run_deterministic_regression_harness(
            calendar_registry=calendar_registry,
            calendar_cases=calendar_regression_cases,
            seasonality_cases=hourly_factor_regression_canonical_cases,
        )


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


def _threshold_configuration(
    hourly_factor_regression_fixture_document: HourlyFactorRegressionFixtureDocument,
) -> dict[str, object]:
    return cast(
        dict[str, object],
        hourly_factor_regression_fixture_document["threshold_configuration"],
    )


def _harness_tolerance_config(
    hourly_factor_regression_fixture_document: HourlyFactorRegressionFixtureDocument,
) -> dict[str, float]:
    threshold_configuration = _threshold_configuration(hourly_factor_regression_fixture_document)
    return cast(
        dict[str, float],
        threshold_configuration["tolerance_config"],
    )
