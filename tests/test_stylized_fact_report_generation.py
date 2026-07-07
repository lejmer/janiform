import json
from collections.abc import Mapping
from typing import Any, Optional, cast

import pytest

import janiform.stylized_facts as stylized_facts_module
import janiform.stylized_facts.workflow as stylized_facts_workflow_module
from janiform.stylized_facts import (
    STYLIZED_FACT_HORIZON_COMPARISON_REPORT_BUNDLE_SCHEMA_VERSION,
    run_stylized_fact_horizon_comparison_report_generator,
)


def test_report_generator_wrapper_parses_bundle_and_preserves_metric_class(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    sample_bundle = _sample_report_bundle_payload()

    class FakeNativeModule:
        def __init__(self) -> None:
            self.last_request: Optional[dict[str, Any]] = None

        def run_stylized_fact_horizon_comparison_report_generator(
            self,
            generator_request_json: str,
        ) -> str:
            self.last_request = cast(dict[str, Any], json.loads(generator_request_json))
            return json.dumps(sample_bundle)

    fake_native = FakeNativeModule()
    monkeypatch.setattr(stylized_facts_workflow_module, "native_module", lambda: fake_native)

    report_bundle = run_stylized_fact_horizon_comparison_report_generator(
        metric_catalog={"catalog_id": "stylized_fact_metric_catalog"},
        comparison_request={"metric_catalog_id": "stylized_fact_metric_catalog"},
        execution_mode="memory",
    )

    assert report_bundle.schema_version == STYLIZED_FACT_HORIZON_COMPARISON_REPORT_BUNDLE_SCHEMA_VERSION
    assert report_bundle.table_summary.rows[0].metric_class == "primary_robust"
    assert fake_native.last_request is not None
    assert cast(str, fake_native.last_request["generation_options"]["execution_mode"]) == "memory"


def _sample_report_bundle_payload() -> dict[str, Any]:
    output_schema_metadata: dict[str, Any] = {
        "schema_version": "validation.stylized_fact.horizon_comparison.report_bundle.output_schema_metadata.v1",
        "required_bundle_fields": [
            "schema_version",
            "determinism_policy",
            "output_schema_metadata",
            "comparison_report",
            "table_summary",
            "artifact_export",
        ],
        "required_table_summary_fields": ["schema_version", "determinism_policy", "rows"],
        "required_table_summary_row_fields": [
            "calibration_run_id",
            "empirical_dataset_id",
            "simulated_dataset_id",
            "horizon_label",
            "metric_id",
            "metric_name",
            "metric_variant",
            "metric_class",
            "alignment_status",
            "status",
            "absolute_delta",
            "standardized_delta",
            "empirical_status",
            "simulated_status",
            "empirical_value",
            "simulated_value",
            "empirical_confidence_interval",
            "simulated_confidence_interval",
            "empirical_sample_coverage",
            "simulated_sample_coverage",
            "empirical_horizon_coverage_ratio",
            "simulated_horizon_coverage_ratio",
            "insufficient_data_reasons",
            "decision_reason_codes",
        ],
        "decision_state_vocabulary": ["pass", "fail", "insufficient_data"],
        "metric_class_vocabulary": ["primary_robust", "secondary_moment"],
        "insufficient_data_status_code": "insufficient_data",
    }
    table_rows: list[dict[str, Any]] = [
        {
            "calibration_run_id": "run_001",
            "empirical_dataset_id": "empirical_set",
            "simulated_dataset_id": "simulated_set",
            "horizon_label": "1h",
            "metric_id": "returns_abs_quantile_shape_ratio_q99_q95",
            "metric_name": "Returns Abs Quantile Shape Ratio Q99 Q95",
            "metric_variant": None,
            "metric_class": "primary_robust",
            "alignment_status": "matched",
            "status": "pass",
            "absolute_delta": 0.1,
            "standardized_delta": 1.0,
            "empirical_status": "available",
            "simulated_status": "available",
            "empirical_value": 1.0,
            "simulated_value": 1.1,
            "empirical_confidence_interval": {
                "lower_bound": 0.95,
                "upper_bound": 1.05,
                "confidence_level": 0.95,
            },
            "simulated_confidence_interval": {
                "lower_bound": 1.05,
                "upper_bound": 1.15,
                "confidence_level": 0.95,
            },
            "empirical_sample_coverage": _sample_coverage(),
            "simulated_sample_coverage": _sample_coverage(),
            "empirical_horizon_coverage_ratio": 0.83,
            "simulated_horizon_coverage_ratio": 0.83,
            "insufficient_data_reasons": {"empirical": [], "simulated": []},
            "decision_reason_codes": [],
        },
        {
            "calibration_run_id": "run_001",
            "empirical_dataset_id": "empirical_set",
            "simulated_dataset_id": "simulated_set",
            "horizon_label": "1h",
            "metric_id": "returns_l_moment_lambda2",
            "metric_name": "Returns L Moment Lambda2",
            "metric_variant": None,
            "metric_class": "secondary_moment",
            "alignment_status": "matched",
            "status": "insufficient_data",
            "absolute_delta": None,
            "standardized_delta": None,
            "empirical_status": "insufficient_data",
            "simulated_status": "available",
            "empirical_value": None,
            "simulated_value": 0.4,
            "empirical_confidence_interval": None,
            "simulated_confidence_interval": {
                "lower_bound": 0.35,
                "upper_bound": 0.45,
                "confidence_level": 0.95,
            },
            "empirical_sample_coverage": _sample_coverage(),
            "simulated_sample_coverage": _sample_coverage(),
            "empirical_horizon_coverage_ratio": 0.0,
            "simulated_horizon_coverage_ratio": 0.83,
            "insufficient_data_reasons": {
                "empirical": ["missing_point_estimate"],
                "simulated": [],
            },
            "decision_reason_codes": ["insufficient_empirical_data"],
        },
    ]

    return {
        "schema_version": "validation.stylized_fact.horizon_comparison.report_bundle.v1",
        "determinism_policy": "stable_run_horizon_metric_order_with_metric_catalog_classification.v1",
        "output_schema_metadata": output_schema_metadata,
        "comparison_report": {
            "schema_version": "validation.stylized_fact.horizon_comparison.report.v1",
            "decision_policy": "explicit_horizon_and_metric_key_alignment_without_implicit_interpolation.v1",
            "uncertainty_policy": (
                "pooled_interval_half_width_standardized_delta_from_input_confidence_intervals.v1"
            ),
            "insufficient_data_status_code": "insufficient_data",
            "output_schema_metadata": {
                "schema_version": "validation.stylized_fact.horizon_comparison.output_schema_metadata.v1",
                "required_batch_report_fields": [
                    "schema_version",
                    "decision_policy",
                    "uncertainty_policy",
                    "insufficient_data_status_code",
                    "output_schema_metadata",
                    "metric_catalog_id",
                    "metric_catalog_version",
                    "threshold_policy_id",
                    "status",
                    "summary",
                    "run_reports",
                ],
                "required_run_report_fields": [
                    "calibration_run_id",
                    "empirical_dataset_id",
                    "simulated_dataset_id",
                    "status",
                    "decision_counts",
                    "total_horizon_count",
                    "total_metric_count",
                    "horizons",
                ],
                "required_horizon_report_fields": [
                    "horizon_label",
                    "empirical_horizon_duration_nanos",
                    "simulated_horizon_duration_nanos",
                    "alignment_status",
                    "status",
                    "decision_counts",
                    "metrics",
                ],
                "required_metric_report_fields": [
                    "metric_id",
                    "metric_name",
                    "metric_variant",
                    "alignment_status",
                    "status",
                    "empirical",
                    "simulated",
                    "absolute_delta",
                    "uncertainty_context",
                    "threshold_context",
                    "horizon_label",
                    "decision_reason_codes",
                ],
                "required_dataset_metric_fields": [
                    "status",
                    "value",
                    "confidence_interval",
                    "sample_coverage",
                    "observed_counts",
                    "required_minimums",
                    "horizon_coverage_ratio",
                    "insufficiency_reason_codes",
                    "input_present",
                ],
                "required_decision_count_fields": [
                    "pass_count",
                    "fail_count",
                    "insufficient_data_count",
                ],
                "decision_state_vocabulary": ["pass", "fail", "insufficient_data"],
                "horizon_alignment_status_vocabulary": [
                    "matched",
                    "missing_empirical_horizon",
                    "missing_simulated_horizon",
                    "horizon_duration_mismatch",
                ],
                "metric_alignment_status_vocabulary": [
                    "matched",
                    "missing_empirical_metric",
                    "missing_simulated_metric",
                    "horizon_duration_mismatch",
                ],
                "dataset_metric_status_vocabulary": ["available", "insufficient_data"],
                "decision_reason_vocabulary": [
                    "missing_empirical_metric",
                    "missing_simulated_metric",
                    "horizon_duration_mismatch",
                    "insufficient_empirical_data",
                    "insufficient_simulated_data",
                    "absolute_delta_threshold_exceeded",
                    "standardized_delta_threshold_exceeded",
                ],
                "sample_sufficiency_reason_vocabulary": [
                    "missing_metric",
                    "missing_point_estimate",
                    "invalid_point_estimate",
                    "insufficient_sample_count",
                    "insufficient_effective_sample_count",
                    "insufficient_tail_observation_count",
                    "insufficient_horizon_coverage_ratio",
                ],
            },
            "metric_catalog_id": "stylized_fact_metric_catalog",
            "metric_catalog_version": "1.0.0",
            "threshold_policy_id": "stylized_fact_delta_policy.v1",
            "status": "insufficient_data",
            "summary": {
                "total_run_count": 1,
                "total_metric_count": 2,
                "run_decision_counts": {
                    "pass_count": 0,
                    "fail_count": 0,
                    "insufficient_data_count": 1,
                },
                "metric_decision_counts": {
                    "pass_count": 1,
                    "fail_count": 0,
                    "insufficient_data_count": 1,
                },
            },
            "run_reports": [
                {
                    "calibration_run_id": "run_001",
                    "empirical_dataset_id": "empirical_set",
                    "simulated_dataset_id": "simulated_set",
                    "status": "insufficient_data",
                    "decision_counts": {
                        "pass_count": 1,
                        "fail_count": 0,
                        "insufficient_data_count": 1,
                    },
                    "total_horizon_count": 1,
                    "total_metric_count": 2,
                    "horizons": [
                        {
                            "horizon_label": "1h",
                            "empirical_horizon_duration_nanos": 3_600_000_000_000,
                            "simulated_horizon_duration_nanos": 3_600_000_000_000,
                            "alignment_status": "matched",
                            "status": "insufficient_data",
                            "decision_counts": {
                                "pass_count": 1,
                                "fail_count": 0,
                                "insufficient_data_count": 1,
                            },
                            "metrics": [
                                _comparison_metric_from_table_row(
                                    table_rows[0],
                                    threshold_context={
                                        "policy_id": "stylized_fact_delta_policy.v1",
                                        "max_absolute_delta": 0.2,
                                        "max_standardized_delta": 5.0,
                                        "absolute_delta_exceeds_threshold": False,
                                        "standardized_delta_exceeds_threshold": False,
                                    },
                                    uncertainty_context={
                                        "empirical_interval_half_width": 0.05,
                                        "simulated_interval_half_width": 0.05,
                                        "pooled_interval_half_width": 0.07071067811865482,
                                        "standardized_delta": 1.414213562373095,
                                    },
                                ),
                                _comparison_metric_from_table_row(
                                    table_rows[1],
                                    threshold_context={
                                        "policy_id": "stylized_fact_delta_policy.v1",
                                        "max_absolute_delta": 0.25,
                                        "max_standardized_delta": 4.0,
                                        "absolute_delta_exceeds_threshold": None,
                                        "standardized_delta_exceeds_threshold": None,
                                    },
                                    uncertainty_context={
                                        "empirical_interval_half_width": None,
                                        "simulated_interval_half_width": 0.05,
                                        "pooled_interval_half_width": None,
                                        "standardized_delta": None,
                                    },
                                ),
                            ],
                        }
                    ],
                }
            ],
        },
        "table_summary": {
            "schema_version": "validation.stylized_fact.horizon_comparison.table_summary.v1",
            "determinism_policy": "stable_run_horizon_metric_order_with_metric_catalog_classification.v1",
            "rows": table_rows,
        },
        "artifact_export": None,
    }


def _sample_report_bundle() -> stylized_facts_module.StylizedFactHorizonComparisonReportBundle:
    sample_bundle_payload = _sample_report_bundle_payload()

    class FakeNativeModule:
        def run_stylized_fact_horizon_comparison_report_generator(
            self,
            generator_request_json: str,
        ) -> str:
            del generator_request_json
            return json.dumps(sample_bundle_payload)

    original_native_module = stylized_facts_workflow_module.native_module
    stylized_facts_workflow_module.native_module = lambda: FakeNativeModule()
    try:
        return run_stylized_fact_horizon_comparison_report_generator(
            metric_catalog={"catalog_id": "stylized_fact_metric_catalog"},
            comparison_request={"metric_catalog_id": "stylized_fact_metric_catalog"},
            execution_mode="memory",
        )
    finally:
        stylized_facts_workflow_module.native_module = original_native_module


def _plot_point_from_table_row(table_row: Mapping[str, Any]) -> dict[str, Any]:
    return {
        "calibration_run_id": table_row["calibration_run_id"],
        "horizon_label": table_row["horizon_label"],
        "metric_id": table_row["metric_id"],
        "metric_name": table_row["metric_name"],
        "metric_variant": table_row["metric_variant"],
        "metric_class": table_row["metric_class"],
        "status": table_row["status"],
        "absolute_delta": table_row["absolute_delta"],
        "standardized_delta": table_row["standardized_delta"],
        "empirical_value": table_row["empirical_value"],
        "simulated_value": table_row["simulated_value"],
        "empirical_confidence_interval": table_row["empirical_confidence_interval"],
        "simulated_confidence_interval": table_row["simulated_confidence_interval"],
        "empirical_sample_coverage": table_row["empirical_sample_coverage"],
        "simulated_sample_coverage": table_row["simulated_sample_coverage"],
        "empirical_horizon_coverage_ratio": table_row["empirical_horizon_coverage_ratio"],
        "simulated_horizon_coverage_ratio": table_row["simulated_horizon_coverage_ratio"],
        "insufficient_data_reasons": table_row["insufficient_data_reasons"],
    }


def _comparison_metric_from_table_row(
    table_row: Mapping[str, Any],
    *,
    threshold_context: dict[str, Any],
    uncertainty_context: dict[str, Any],
) -> dict[str, Any]:
    return {
        "metric_id": table_row["metric_id"],
        "metric_name": table_row["metric_name"],
        "metric_variant": table_row["metric_variant"],
        "alignment_status": table_row["alignment_status"],
        "status": table_row["status"],
        "empirical": {
            "status": table_row["empirical_status"],
            "value": table_row["empirical_value"],
            "confidence_interval": table_row["empirical_confidence_interval"],
            "sample_coverage": {
                "horizon_interval_count": table_row["empirical_sample_coverage"]["horizon_interval_count"],
                "covered_interval_count": table_row["empirical_sample_coverage"]["covered_interval_count"],
            },
            "observed_counts": {
                "sample_count": table_row["empirical_sample_coverage"]["sample_count"],
                "effective_sample_count": table_row["empirical_sample_coverage"]["effective_sample_count"],
                "tail_observation_count": table_row["empirical_sample_coverage"]["tail_observation_count"],
            },
            "required_minimums": {
                "minimum_sample_count": table_row["empirical_sample_coverage"]["minimum_sample_count"],
                "minimum_effective_sample_count": table_row["empirical_sample_coverage"][
                    "minimum_effective_sample_count"
                ],
                "minimum_tail_observation_count": table_row["empirical_sample_coverage"][
                    "minimum_tail_observation_count"
                ],
                "minimum_horizon_coverage_ratio": (
                    table_row["empirical_sample_coverage"]["minimum_horizon_coverage_ratio_scaled_bps"]
                    / 10_000.0
                ),
            },
            "horizon_coverage_ratio": table_row["empirical_horizon_coverage_ratio"],
            "insufficiency_reason_codes": table_row["insufficient_data_reasons"]["empirical"],
            "input_present": True,
        },
        "simulated": {
            "status": table_row["simulated_status"],
            "value": table_row["simulated_value"],
            "confidence_interval": table_row["simulated_confidence_interval"],
            "sample_coverage": {
                "horizon_interval_count": table_row["simulated_sample_coverage"]["horizon_interval_count"],
                "covered_interval_count": table_row["simulated_sample_coverage"]["covered_interval_count"],
            },
            "observed_counts": {
                "sample_count": table_row["simulated_sample_coverage"]["sample_count"],
                "effective_sample_count": table_row["simulated_sample_coverage"]["effective_sample_count"],
                "tail_observation_count": table_row["simulated_sample_coverage"]["tail_observation_count"],
            },
            "required_minimums": {
                "minimum_sample_count": table_row["simulated_sample_coverage"]["minimum_sample_count"],
                "minimum_effective_sample_count": table_row["simulated_sample_coverage"][
                    "minimum_effective_sample_count"
                ],
                "minimum_tail_observation_count": table_row["simulated_sample_coverage"][
                    "minimum_tail_observation_count"
                ],
                "minimum_horizon_coverage_ratio": (
                    table_row["simulated_sample_coverage"]["minimum_horizon_coverage_ratio_scaled_bps"]
                    / 10_000.0
                ),
            },
            "horizon_coverage_ratio": table_row["simulated_horizon_coverage_ratio"],
            "insufficiency_reason_codes": table_row["insufficient_data_reasons"]["simulated"],
            "input_present": True,
        },
        "absolute_delta": table_row["absolute_delta"],
        "uncertainty_context": uncertainty_context,
        "threshold_context": threshold_context,
        "horizon_label": table_row["horizon_label"],
        "decision_reason_codes": table_row["decision_reason_codes"],
    }


def _sample_coverage() -> dict[str, int]:
    return {
        "horizon_interval_count": 12,
        "covered_interval_count": 10,
        "sample_count": 100,
        "effective_sample_count": 80,
        "tail_observation_count": 20,
        "minimum_sample_count": 10,
        "minimum_effective_sample_count": 8,
        "minimum_tail_observation_count": 1,
        "minimum_horizon_coverage_ratio_scaled_bps": 7500,
    }
