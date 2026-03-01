import json
from typing import Any, cast

import pytest

import janiform.validation as validation_module
from janiform.seasonality import SeasonalityInputValidationError
from janiform.validation import (
    OPEN_CLOSED_GAP_BEHAVIOR_BASELINE_EXPECTED_OUTPUT_POLICY,
    OPEN_CLOSED_GAP_BEHAVIOR_DIFF_REPORT_SCHEMA_VERSION,
    OPEN_CLOSED_GAP_BEHAVIOR_SUMMARY_ARTIFACT_SCHEMA_VERSION,
    OPEN_CLOSED_GAP_LEAKAGE_REGRESSION_HARNESS_DETERMINISM_POLICY,
    OPEN_CLOSED_GAP_LEAKAGE_REGRESSION_HARNESS_REPORT_SCHEMA_VERSION,
    OPEN_CLOSED_GAP_LEAKAGE_REGRESSION_OUTPUT_SCHEMA_METADATA_VERSION,
    run_open_closed_gap_leakage_regression_harness,
)
from tests.fixture_validation import OpenClosedGapBehaviorCase

type GapLeakageCaseInput = dict[str, Any]

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

    summary = cast(dict[str, Any], report["summary"])
    case_reports: list[dict[str, Any]] = cast(list[dict[str, Any]], report["case_reports"])

    assert report["schema_version"] == OPEN_CLOSED_GAP_LEAKAGE_REGRESSION_HARNESS_REPORT_SCHEMA_VERSION
    assert report["determinism_policy"] == OPEN_CLOSED_GAP_LEAKAGE_REGRESSION_HARNESS_DETERMINISM_POLICY
    assert cast(int, summary["total_case_count"]) == len(gap_behavior_cases)
    assert cast(int, summary["failed_case_count"]) == 0
    assert cast(int, summary["passed_case_count"]) == cast(int, summary["total_case_count"])
    assert cast(int, summary["total_semantic_mismatch_failure_count"]) == 0

    scenario_class_coverage = cast(dict[str, int], summary["scenario_class_coverage"])
    assert scenario_class_coverage["known_only_case_count"] >= 1
    assert scenario_class_coverage["surprise_only_case_count"] >= 1
    assert scenario_class_coverage["mixed_news_case_count"] >= 1

    assert cast(dict[str, Any], report["output_schema_metadata"])["schema_version"] == (
        OPEN_CLOSED_GAP_LEAKAGE_REGRESSION_OUTPUT_SCHEMA_METADATA_VERSION
    )
    assert cast(dict[str, Any], report["summary_artifact"])["schema_version"] == (
        OPEN_CLOSED_GAP_BEHAVIOR_SUMMARY_ARTIFACT_SCHEMA_VERSION
    )

    behavior_diff_report = cast(dict[str, Any], report["behavior_diff_report"])
    assert behavior_diff_report["schema_version"] == OPEN_CLOSED_GAP_BEHAVIOR_DIFF_REPORT_SCHEMA_VERSION
    assert behavior_diff_report["baseline_expected_output_policy"] == (
        OPEN_CLOSED_GAP_BEHAVIOR_BASELINE_EXPECTED_OUTPUT_POLICY
    )

    assert all(cast(str, case_report["status"]) == "pass" for case_report in case_reports)
    assert all(
        abs(
            cast(float, cast(dict[str, Any], case_report["contribution_summary"])["expected_activity_coupling_total"])
            - cast(float, cast(dict[str, Any], case_report["contribution_summary"])["observed_activity_coupling_total"]),
        ) <= 1e-12
        and abs(
            cast(float, cast(dict[str, Any], case_report["contribution_summary"])["expected_open_jump_total"])
            - cast(float, cast(dict[str, Any], case_report["contribution_summary"])["observed_open_jump_total"]),
        ) <= 1e-12
        and abs(
            cast(float, cast(dict[str, Any], case_report["contribution_summary"])["expected_closed_gap_total"])
            - cast(float, cast(dict[str, Any], case_report["contribution_summary"])["observed_closed_gap_total"]),
        )
        <= 1e-12
        for case_report in case_reports
    )


def test_open_closed_gap_leakage_regression_harness_reports_semantic_bucket_for_failures(
    calendar_registry,
    open_closed_gap_behavior_cases: list[OpenClosedGapBehaviorCase],
) -> None:
    gap_behavior_cases = _to_gap_behavior_case_inputs(open_closed_gap_behavior_cases)
    open_window_case = _case_input_by_fixture_id(gap_behavior_cases, OPEN_WINDOW_SCENARIO_ID)
    expected_event_id = cast(str, cast(list[dict[str, Any]], open_window_case["injected_events"])[0]["event_id"])

    cast(list[dict[str, Any]], open_window_case["injected_events"])[0][
        "observed_closed_gap_g_t_increment_override"
    ] = 0.19

    report = run_open_closed_gap_leakage_regression_harness(
        calendar_registry=calendar_registry,
        gap_behavior_cases=gap_behavior_cases,
    )

    summary = cast(dict[str, int], report["summary"])
    assert summary["total_case_count"] == len(gap_behavior_cases)
    assert summary["failed_case_count"] == 1
    assert summary["total_semantic_mismatch_failure_count"] == 1

    case_report = cast(
        dict[str, Any],
        next(
            case_report
            for case_report in cast(list[dict[str, Any]], report["case_reports"])
            if cast(str, case_report["fixture_id"]) == OPEN_WINDOW_SCENARIO_ID
        ),
    )
    assert cast(str, case_report["status"]) == "fail"
    assert cast(int, case_report["semantic_mismatch_failure_count"]) == 1
    assert abs(cast(float, case_report["max_absolute_mismatch"]) - 0.19) <= 1e-12

    failures: list[dict[str, Any]] = cast(list[dict[str, Any]], case_report["failures"])
    semantic_failure = cast(
        dict[str, Any],
        next(
            failure
            for failure in failures
            if cast(str, failure["failure_kind"]) == "semantic_mismatch"
        ),
    )
    assert semantic_failure["event_id"] == expected_event_id
    assert semantic_failure["semantic_bucket"] == "closed_gap"
    assert semantic_failure["mismatch_kind"] == "open_window_closed_gap_leakage"
    assert abs(cast(float, semantic_failure["absolute_difference"]) - 0.19) <= 1e-12

    behavior_diff = cast(dict[str, Any], case_report["behavior_diff"])
    diff_metrics = cast(list[dict[str, Any]], behavior_diff["metrics"])
    closed_gap_diff_metric = next(
        diff_metric
        for diff_metric in diff_metrics
        if cast(str, diff_metric["metric_id"]) == "closed_gap_g_t_increment_total"
    )

    assert cast(str, closed_gap_diff_metric["semantic_bucket"]) == "closed_gap"
    assert cast(bool, closed_gap_diff_metric["regression_detected"])
    assert abs(cast(float, closed_gap_diff_metric["observed_value"]) - 0.19) <= 1e-12


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
            _gap_behavior_cases: list[dict[str, Any]],
            component_absolute_tolerance: float,
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
                        "canonical_component_vocabulary": [
                            "trading_return",
                            "open_jump",
                            "closed_gap",
                            "activity_coupling",
                        ],
                        "canonical_semantic_bucket_vocabulary": [
                            "trading_return",
                            "open_jump",
                            "closed_gap",
                            "activity_coupling",
                        ],
                        "required_case_report_fields": ["fixture_id"],
                        "required_summary_artifact_fields": ["schema_version"],
                        "required_behavior_diff_report_fields": ["schema_version"],
                        "required_behavior_diff_metric_fields": ["metric_id"],
                    },
                    "tolerance_config": {
                        "component_absolute_tolerance": component_absolute_tolerance,
                    },
                    "summary": {
                        "total_case_count": 1,
                        "passed_case_count": 1,
                        "failed_case_count": 0,
                        "total_semantic_mismatch_failure_count": 0,
                        "scenario_class_coverage": {
                            "known_only_case_count": 1,
                            "surprise_only_case_count": 1,
                            "mixed_news_case_count": 1,
                        },
                    },
                    "summary_artifact": {
                        "schema_version": OPEN_CLOSED_GAP_BEHAVIOR_SUMMARY_ARTIFACT_SCHEMA_VERSION,
                        "determinism_policy": "preserve_input_case_order.v1",
                        "total_case_count": 1,
                        "total_open_window_event_count": 0,
                        "total_closed_window_event_count": 0,
                        "scenario_class_coverage": {
                            "known_only_case_count": 1,
                            "surprise_only_case_count": 1,
                            "mixed_news_case_count": 1,
                        },
                        "scenarios": [
                            {
                                "fixture_id": "fixture_a",
                                "exchange_code": "XNYS",
                                "scenario_class": "known_only",
                                "deterministic_replay_metadata": {
                                    "replay_baseline_id": "baseline",
                                    "event_calendar_binding_id": "calendar",
                                    "jump_observation_binding_id": "jump",
                                    "gap_observation_binding_id": "gap",
                                    "candidate_selection_policy_id": "policy",
                                    "candidate_selection_rank": 0,
                                },
                                "issue_traceability": {
                                    "issue_id": "JAN-140",
                                    "semantic_focus_tags": ["open_jump"],
                                },
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
                            "trading_return_regression_metric_count": 0,
                            "open_jump_regression_metric_count": 0,
                            "closed_gap_regression_metric_count": 0,
                            "activity_coupling_regression_metric_count": 0,
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
            "scenario_class": fixture_case["scenario_class"],
            "deterministic_replay_metadata": fixture_case["deterministic_replay_metadata"],
            "issue_traceability": fixture_case["issue_traceability"],
            "initial_j_t": fixture_case["initial_accumulator_state"]["j_t"],
            "initial_m_t": fixture_case["initial_accumulator_state"]["m_t"],
            "initial_g_t": fixture_case["initial_accumulator_state"]["g_t"],
            "injected_events": [
                {
                    "event_id": injected_event["event_id"],
                    "timestamp_utc": injected_event["timestamp_utc"],
                    "news_arrival_component": injected_event["news_arrival_component"],
                    "shared_severity_mark": injected_event["shared_severity_mark"],
                    "coupled_return_jump_mark": injected_event["coupled_return_jump_mark"],
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
