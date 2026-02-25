import json
from typing import cast

import pytest

import janiform.validation as validation_module
from janiform.seasonality import SeasonalityInputValidationError
from janiform.validation import (
    OPEN_CLOSED_GAP_BEHAVIOR_BASELINE_EXPECTED_OUTPUT_POLICY,
    OPEN_CLOSED_GAP_BEHAVIOR_DIFF_REPORT_SCHEMA_VERSION,
    OPEN_CLOSED_GAP_BEHAVIOR_SUMMARY_ARTIFACT_SCHEMA_VERSION,
    OPEN_CLOSED_GAP_LEAKAGE_REGRESSION_OUTPUT_SCHEMA_METADATA_VERSION,
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
    assert cast(dict[str, object], report["output_schema_metadata"])["schema_version"] == (
        OPEN_CLOSED_GAP_LEAKAGE_REGRESSION_OUTPUT_SCHEMA_METADATA_VERSION
    )
    assert cast(dict[str, object], report["summary_artifact"])["schema_version"] == (
        OPEN_CLOSED_GAP_BEHAVIOR_SUMMARY_ARTIFACT_SCHEMA_VERSION
    )

    behavior_diff_report = cast(dict[str, object], report["behavior_diff_report"])
    assert behavior_diff_report["schema_version"] == OPEN_CLOSED_GAP_BEHAVIOR_DIFF_REPORT_SCHEMA_VERSION
    assert behavior_diff_report["baseline_expected_output_policy"] == (
        OPEN_CLOSED_GAP_BEHAVIOR_BASELINE_EXPECTED_OUTPUT_POLICY
    )

    assert all(cast(str, case_report["status"]) == "pass" for case_report in case_reports)
    assert all(
        abs(
            cast(float, cast(dict[str, object], case_report["contribution_summary"])[
                "expected_activity_coupling_j_t_increment_total"
            ])
            - cast(float, cast(dict[str, object], case_report["contribution_summary"])[
                "observed_activity_coupling_j_t_increment_total"
            ]),
        )
        <= 1e-12
        for case_report in case_reports
    )


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

    behavior_diff = cast(dict[str, object], case_report["behavior_diff"])
    diff_metrics = cast(list[dict[str, object]], behavior_diff["metrics"])
    open_window_diff_metric = next(
        diff_metric
        for diff_metric in diff_metrics
        if cast(str, diff_metric["metric_id"]) == "open_window_direct_gap_g_t_increment_total"
    )

    assert cast(bool, open_window_diff_metric["regression_detected"])
    assert abs(cast(float, open_window_diff_metric["observed_value"]) - 0.19) <= 1e-12


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


def test_open_closed_gap_leakage_regression_harness_rejects_malformed_artifact_payload(
    monkeypatch: pytest.MonkeyPatch,
    calendar_registry,
    open_closed_gap_behavior_cases: list[OpenClosedGapBehaviorCase],
) -> None:
    gap_behavior_cases = _to_gap_behavior_case_inputs(open_closed_gap_behavior_cases)

    class FakeNativeModule:
        def run_open_closed_gap_leakage_regression_harness(
            self,
            _calendar_registry: object,
            _gap_behavior_cases: list[dict[str, object]],
            *,
            direct_gap_absolute_tolerance: float,
        ) -> str:
            return json.dumps(
                {
                    "schema_version": OPEN_CLOSED_GAP_LEAKAGE_REGRESSION_HARNESS_REPORT_SCHEMA_VERSION,
                    "determinism_policy": OPEN_CLOSED_GAP_LEAKAGE_REGRESSION_HARNESS_DETERMINISM_POLICY,
                    "output_schema_metadata": {
                        "schema_version": OPEN_CLOSED_GAP_LEAKAGE_REGRESSION_OUTPUT_SCHEMA_METADATA_VERSION,
                        "summary_artifact_schema_version": OPEN_CLOSED_GAP_BEHAVIOR_SUMMARY_ARTIFACT_SCHEMA_VERSION,
                        "behavior_diff_report_schema_version": OPEN_CLOSED_GAP_BEHAVIOR_DIFF_REPORT_SCHEMA_VERSION,
                        "baseline_expected_output_policy": OPEN_CLOSED_GAP_BEHAVIOR_BASELINE_EXPECTED_OUTPUT_POLICY,
                        "environment_metadata_policy": "exclude_environment_specific_metadata.v1",
                        "required_case_report_fields": ["fixture_id"],
                        "required_summary_artifact_fields": ["schema_version"],
                        "required_behavior_diff_report_fields": ["schema_version"],
                        "required_behavior_diff_metric_fields": ["metric_id"],
                    },
                    "tolerance_config": {
                        "direct_gap_absolute_tolerance": direct_gap_absolute_tolerance,
                    },
                    "summary": {
                        "total_case_count": 1,
                        "passed_case_count": 1,
                        "failed_case_count": 0,
                        "total_open_window_event_count": 0,
                        "total_leakage_failure_count": 0,
                    },
                    "summary_artifact": {
                        "schema_version": OPEN_CLOSED_GAP_BEHAVIOR_SUMMARY_ARTIFACT_SCHEMA_VERSION,
                        "determinism_policy": "preserve_input_case_order.v1",
                        "total_case_count": 1,
                        "total_open_window_event_count": 0,
                        "total_closed_window_event_count": 0,
                        "scenarios": [
                            {
                                "fixture_id": "fixture_a",
                                "exchange_code": "XNYS",
                            },
                        ],
                    },
                    "behavior_diff_report": {
                        "schema_version": OPEN_CLOSED_GAP_BEHAVIOR_DIFF_REPORT_SCHEMA_VERSION,
                        "determinism_policy": "preserve_input_case_order_and_metric_order.v1",
                        "baseline_expected_output_policy": OPEN_CLOSED_GAP_BEHAVIOR_BASELINE_EXPECTED_OUTPUT_POLICY,
                        "summary": {
                            "total_case_count": 1,
                            "regression_case_count": 0,
                            "total_regression_metric_count": 0,
                        },
                        "scenario_diffs": [],
                    },
                    "case_reports": [],
                },
            )

    monkeypatch.setattr(validation_module, "native_module", lambda: FakeNativeModule())

    with pytest.raises(
        SeasonalityInputValidationError,
        match="summary_artifact.scenarios\\[0\\]\\.contributions is required",
    ):
        run_open_closed_gap_leakage_regression_harness(
            calendar_registry=calendar_registry,
            gap_behavior_cases=gap_behavior_cases,
        )


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
