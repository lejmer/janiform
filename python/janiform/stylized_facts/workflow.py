"""Stylized-fact reporting workflows and internal input helpers."""

from __future__ import annotations

import json
from collections.abc import Mapping
from typing import Any, Optional, cast

import orjson
from plotly import graph_objects as go

from .._native_api import native_module
from ..contracts.base import (
    _hydrate_contract_model,
    require_non_empty_string as _require_non_empty_string_base,
)
from ..seasonality import SeasonalityInputValidationError
from .contracts import (
    StylizedFactHorizonComparisonPlotPayload,
    StylizedFactHorizonComparisonPlotPoint,
    StylizedFactHorizonComparisonReportBundle,
)

STYLIZED_FACT_HORIZON_COMPARISON_REPORT_SCHEMA_VERSION: str = (
    "validation.stylized_fact.horizon_comparison.report.v1"
)
STYLIZED_FACT_HORIZON_COMPARISON_REPORT_BUNDLE_SCHEMA_VERSION: str = (
    "validation.stylized_fact.horizon_comparison.report_bundle.v1"
)
STYLIZED_FACT_HORIZON_COMPARISON_REPORT_BUNDLE_DETERMINISM_POLICY: str = (
    "stable_run_horizon_metric_order_with_metric_catalog_classification.v1"
)
STYLIZED_FACT_HORIZON_COMPARISON_REPORT_BUNDLE_OUTPUT_SCHEMA_METADATA_VERSION: str = (
    "validation.stylized_fact.horizon_comparison.report_bundle.output_schema_metadata.v1"
)
STYLIZED_FACT_HORIZON_COMPARISON_TABLE_SUMMARY_SCHEMA_VERSION: str = (
    "validation.stylized_fact.horizon_comparison.table_summary.v1"
)
STYLIZED_FACT_HORIZON_COMPARISON_PLOT_PAYLOAD_SCHEMA_VERSION: str = (
    "validation.stylized_fact.horizon_comparison.plot_payload.v1"
)


def run_stylized_fact_horizon_comparison_report_generator(
    *,
    metric_catalog: Mapping[str, Any],
    comparison_request: Mapping[str, Any],
    execution_mode: str = "memory",
    artifact_export: Optional[Mapping[str, Any]] = None,
) -> StylizedFactHorizonComparisonReportBundle:
    if execution_mode not in {"memory", "disk"}:
        raise SeasonalityInputValidationError("execution_mode must be either 'memory' or 'disk'")

    if artifact_export is not None and execution_mode != "disk":
        raise SeasonalityInputValidationError("artifact_export is only supported when execution_mode='disk'")

    normalized_metric_catalog: dict[str, Any] = dict(metric_catalog)
    normalized_comparison_request: dict[str, Any] = dict(comparison_request)

    generation_options: dict[str, Any] = {"execution_mode": execution_mode}
    if artifact_export is not None:
        generation_options["artifact_export"] = _validate_artifact_export_config(artifact_export)

    generator_request: dict[str, Any] = {
        "metric_catalog": normalized_metric_catalog,
        "comparison_request": normalized_comparison_request,
        "generation_options": generation_options,
    }

    native = native_module()
    generator_request_json: str = json.dumps(generator_request)
    report_bundle_json: str = native.run_stylized_fact_horizon_comparison_report_generator(
        generator_request_json,
    )

    try:
        parsed_report_bundle: object = orjson.loads(report_bundle_json)
    except orjson.JSONDecodeError as e:
        raise SeasonalityInputValidationError(
            f"stylized fact report bundle must be valid JSON: {e.msg}",
        ) from e

    parsed_report_bundle_mapping = cast(Mapping[str, Any], parsed_report_bundle)
    return _hydrate_contract_model(
        StylizedFactHorizonComparisonReportBundle,
        parsed_report_bundle_mapping,
    )


def build_stylized_fact_plotly_figure(
    plot_payload: StylizedFactHorizonComparisonPlotPayload,
    *,
    title: str = "Stylized Fact Absolute Delta By Horizon",
) -> Any:
    figure = go.Figure()
    status_colors: dict[str, str] = {
        "pass": "#2A9D8F",
        "fail": "#E76F51",
        "insufficient_data": "#6C757D",
    }
    ordered_statuses: tuple[str, ...] = ("pass", "fail", "insufficient_data")

    for status in ordered_statuses:
        points_for_status: list[StylizedFactHorizonComparisonPlotPoint] = [
            point for point in plot_payload.points if point.status == status
        ]
        if not points_for_status:
            continue

        figure.add_trace(
            go.Scatter(
                x=[point.horizon_label for point in points_for_status],
                y=[point.absolute_delta for point in points_for_status],
                mode="markers",
                name=status,
                marker={"color": status_colors[status], "size": 10},
                customdata=[
                    [
                        point.metric_id,
                        point.metric_class,
                        point.status,
                        json.dumps(
                            None
                            if point.empirical_confidence_interval is None
                            else point.empirical_confidence_interval.to_payload()
                        ),
                        json.dumps(
                            None
                            if point.simulated_confidence_interval is None
                            else point.simulated_confidence_interval.to_payload()
                        ),
                        json.dumps(point.insufficient_data_reasons.to_payload()),
                    ]
                    for point in points_for_status
                ],
                hovertemplate=(
                    "horizon=%{x}<br>"
                    "abs_delta=%{y}<br>"
                    "metric_id=%{customdata[0]}<br>"
                    "metric_class=%{customdata[1]}<br>"
                    "status=%{customdata[2]}<br>"
                    "empirical_ci=%{customdata[3]}<br>"
                    "simulated_ci=%{customdata[4]}<br>"
                    "insufficient_data_reasons=%{customdata[5]}"
                    "<extra></extra>"
                ),
            ),
        )

    figure.update_layout(
        title=title,
        xaxis_title="Horizon Label",
        yaxis_title="Absolute Delta",
        legend_title="Decision Status",
    )
    return figure


def build_stylized_fact_plotly_figure_from_report_bundle(
    report_bundle: StylizedFactHorizonComparisonReportBundle,
    *,
    title: str = "Stylized Fact Absolute Delta By Horizon",
) -> Any:
    return build_stylized_fact_plotly_figure(report_bundle.plot_payload, title=title)


def _validate_artifact_export_config(artifact_export: Mapping[str, Any]) -> dict[str, str]:
    export_directory = _require_non_empty_string(
        artifact_export,
        "export_directory",
        "artifact_export",
    )
    file_stem = _require_non_empty_string(artifact_export, "file_stem", "artifact_export")

    return {"export_directory": export_directory, "file_stem": file_stem}


def _require_non_empty_string(payload: Mapping[str, Any], field_name: str, context: str) -> str:
    return _require_non_empty_string_base(
        payload,
        field_name,
        f"{context}.{field_name}",
        _stylized_fact_error,
    ).strip()


def _stylized_fact_error(field_path: str, message: str) -> SeasonalityInputValidationError:
    return SeasonalityInputValidationError(f"{field_path} {message}")
