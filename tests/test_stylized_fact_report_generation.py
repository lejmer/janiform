import json
import sys
from collections.abc import Mapping
from types import SimpleNamespace
from typing import Any, cast

import pytest

import janiform.stylized_facts as stylized_facts_module
from janiform.stylized_facts import (
    STYLIZED_FACT_HORIZON_COMPARISON_REPORT_BUNDLE_SCHEMA_VERSION,
    build_stylized_fact_plotly_figure_from_report_bundle,
    render_stylized_fact_table_markdown,
    run_stylized_fact_horizon_comparison_report_generator,
)


def test_report_generator_wrapper_parses_bundle_and_preserves_metric_class(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    sample_bundle = _sample_report_bundle_payload()

    class FakeNativeModule:
        def __init__(self) -> None:
            self.last_request: dict[str, Any] | None = None

        def run_stylized_fact_horizon_comparison_report_generator(
            self,
            generator_request_json: str,
        ) -> str:
            self.last_request = cast(dict[str, Any], json.loads(generator_request_json))
            return json.dumps(sample_bundle)

    fake_native = FakeNativeModule()
    monkeypatch.setattr(stylized_facts_module, "native_module", lambda: fake_native)

    report_bundle = run_stylized_fact_horizon_comparison_report_generator(
        metric_catalog={"catalog_id": "stylized_fact_metric_catalog"},
        comparison_request={"metric_catalog_id": "stylized_fact_metric_catalog"},
        execution_mode="memory",
    )

    assert report_bundle["schema_version"] == STYLIZED_FACT_HORIZON_COMPARISON_REPORT_BUNDLE_SCHEMA_VERSION
    assert report_bundle["table_summary"]["rows"][0]["metric_class"] == "primary_robust"
    assert fake_native.last_request is not None
    assert cast(str, fake_native.last_request["generation_options"]["execution_mode"]) == "memory"


def test_table_markdown_renderer_outputs_deterministic_columns() -> None:
    report_bundle = _sample_report_bundle_payload()
    markdown_table = render_stylized_fact_table_markdown(report_bundle["table_summary"], max_rows=1)

    assert "| run_id | horizon | metric_id | class | status | abs_delta | standardized_delta |" in markdown_table
    assert "returns_abs_quantile_shape_ratio_q99_q95" in markdown_table


def test_plotly_adapter_preserves_metric_and_insufficient_metadata(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    report_bundle = _sample_report_bundle_payload()

    class FakeScatter:
        def __init__(self, **kwargs: Any) -> None:
            self.kwargs: dict[str, Any] = kwargs

    class FakeFigure:
        def __init__(self) -> None:
            self.traces: list[FakeScatter] = []
            self.layout_updates: list[dict[str, Any]] = []

        def add_trace(self, trace: FakeScatter) -> None:
            self.traces.append(trace)

        def update_layout(self, **kwargs: Any) -> None:
            self.layout_updates.append(kwargs)

    fake_graph_objects = SimpleNamespace(Figure=FakeFigure, Scatter=FakeScatter)
    monkeypatch.setitem(sys.modules, "plotly", SimpleNamespace(graph_objects=fake_graph_objects))

    figure = build_stylized_fact_plotly_figure_from_report_bundle(report_bundle)
    assert isinstance(figure, FakeFigure)
    assert figure.traces, "plot adapter should produce one or more traces"

    first_trace = figure.traces[0]
    first_custom_data = cast(list[list[str]], first_trace.kwargs["customdata"])[0]
    assert first_custom_data[0] == "returns_abs_quantile_shape_ratio_q99_q95"
    assert first_custom_data[1] == "primary_robust"

    parsed_insufficient_data_reasons = cast(dict[str, list[str]], json.loads(first_custom_data[5]))
    assert parsed_insufficient_data_reasons["empirical"] == []
    assert parsed_insufficient_data_reasons["simulated"] == []


def _sample_report_bundle_payload() -> dict[str, Any]:
    output_schema_metadata: dict[str, Any] = {
        "schema_version": "validation.stylized_fact.horizon_comparison.report_bundle.output_schema_metadata.v1",
        "required_bundle_fields": [
            "schema_version",
            "determinism_policy",
            "output_schema_metadata",
            "comparison_report",
            "table_summary",
            "plot_payload",
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
        "required_plot_payload_fields": ["schema_version", "determinism_policy", "points"],
        "required_plot_point_fields": [
            "calibration_run_id",
            "horizon_label",
            "metric_id",
            "metric_name",
            "metric_variant",
            "metric_class",
            "status",
            "absolute_delta",
            "standardized_delta",
            "empirical_value",
            "simulated_value",
            "empirical_confidence_interval",
            "simulated_confidence_interval",
            "empirical_sample_coverage",
            "simulated_sample_coverage",
            "empirical_horizon_coverage_ratio",
            "simulated_horizon_coverage_ratio",
            "insufficient_data_reasons",
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
        },
        "table_summary": {
            "schema_version": "validation.stylized_fact.horizon_comparison.table_summary.v1",
            "determinism_policy": "stable_run_horizon_metric_order_with_metric_catalog_classification.v1",
            "rows": table_rows,
        },
        "plot_payload": {
            "schema_version": "validation.stylized_fact.horizon_comparison.plot_payload.v1",
            "determinism_policy": "stable_run_horizon_metric_order_with_metric_catalog_classification.v1",
            "points": [
                _plot_point_from_table_row(table_rows[0]),
                _plot_point_from_table_row(table_rows[1]),
            ],
        },
        "artifact_export": None,
    }


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
