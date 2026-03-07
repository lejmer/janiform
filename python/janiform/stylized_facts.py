import json
import math
from collections.abc import Mapping, Sequence
from typing import Any, Optional, TypedDict, cast

from ._native_api import native_module
from .seasonality import SeasonalityInputValidationError

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


class StylizedFactMetricIntervalBounds(TypedDict):
    lower_bound: float
    upper_bound: float
    confidence_level: float


class StylizedFactMetricCoverageSummary(TypedDict):
    horizon_interval_count: int
    covered_interval_count: int
    sample_count: int
    effective_sample_count: int
    tail_observation_count: int
    minimum_sample_count: int
    minimum_effective_sample_count: int
    minimum_tail_observation_count: int
    minimum_horizon_coverage_ratio_scaled_bps: int


class StylizedFactMetricInsufficientDataReasons(TypedDict):
    empirical: list[str]
    simulated: list[str]


class StylizedFactHorizonComparisonTableSummaryRow(TypedDict):
    calibration_run_id: str
    empirical_dataset_id: str
    simulated_dataset_id: str
    horizon_label: str
    metric_id: str
    metric_name: str
    metric_variant: Optional[str]
    metric_class: str
    alignment_status: str
    status: str
    absolute_delta: Optional[float]
    standardized_delta: Optional[float]
    empirical_status: str
    simulated_status: str
    empirical_value: Optional[float]
    simulated_value: Optional[float]
    empirical_confidence_interval: Optional[StylizedFactMetricIntervalBounds]
    simulated_confidence_interval: Optional[StylizedFactMetricIntervalBounds]
    empirical_sample_coverage: StylizedFactMetricCoverageSummary
    simulated_sample_coverage: StylizedFactMetricCoverageSummary
    empirical_horizon_coverage_ratio: float
    simulated_horizon_coverage_ratio: float
    insufficient_data_reasons: StylizedFactMetricInsufficientDataReasons
    decision_reason_codes: list[str]


class StylizedFactHorizonComparisonTableSummary(TypedDict):
    schema_version: str
    determinism_policy: str
    rows: list[StylizedFactHorizonComparisonTableSummaryRow]


class StylizedFactHorizonComparisonPlotPoint(TypedDict):
    calibration_run_id: str
    horizon_label: str
    metric_id: str
    metric_name: str
    metric_variant: Optional[str]
    metric_class: str
    status: str
    absolute_delta: Optional[float]
    standardized_delta: Optional[float]
    empirical_value: Optional[float]
    simulated_value: Optional[float]
    empirical_confidence_interval: Optional[StylizedFactMetricIntervalBounds]
    simulated_confidence_interval: Optional[StylizedFactMetricIntervalBounds]
    empirical_sample_coverage: StylizedFactMetricCoverageSummary
    simulated_sample_coverage: StylizedFactMetricCoverageSummary
    empirical_horizon_coverage_ratio: float
    simulated_horizon_coverage_ratio: float
    insufficient_data_reasons: StylizedFactMetricInsufficientDataReasons


class StylizedFactHorizonComparisonPlotPayload(TypedDict):
    schema_version: str
    determinism_policy: str
    points: list[StylizedFactHorizonComparisonPlotPoint]


class StylizedFactHorizonComparisonReportBundleOutputSchemaMetadata(TypedDict):
    schema_version: str
    required_bundle_fields: list[str]
    required_table_summary_fields: list[str]
    required_table_summary_row_fields: list[str]
    required_plot_payload_fields: list[str]
    required_plot_point_fields: list[str]
    decision_state_vocabulary: list[str]
    metric_class_vocabulary: list[str]
    insufficient_data_status_code: str


class StylizedFactHorizonComparisonExportedArtifactFile(TypedDict):
    artifact_kind: str
    file_name: str
    absolute_path: str


class StylizedFactHorizonComparisonArtifactExportReceipt(TypedDict):
    export_directory: str
    file_stem: str
    exported_files: list[StylizedFactHorizonComparisonExportedArtifactFile]


class StylizedFactHorizonComparisonReportBundle(TypedDict):
    schema_version: str
    determinism_policy: str
    output_schema_metadata: StylizedFactHorizonComparisonReportBundleOutputSchemaMetadata
    comparison_report: dict[str, Any]
    table_summary: StylizedFactHorizonComparisonTableSummary
    plot_payload: StylizedFactHorizonComparisonPlotPayload
    artifact_export: Optional[StylizedFactHorizonComparisonArtifactExportReceipt]


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
        parsed_report_bundle: object = json.loads(report_bundle_json)
    except json.JSONDecodeError as e:
        raise SeasonalityInputValidationError(
            f"stylized fact report bundle must be valid JSON: {e.msg}",
        ) from e

    return _validate_report_bundle_payload(parsed_report_bundle, "report_bundle")


def render_stylized_fact_table_markdown(
    table_summary: Mapping[str, Any],
    *,
    max_rows: Optional[int] = None,
) -> str:
    validated_summary = _validate_table_summary_payload(table_summary, "table_summary")
    rows: list[StylizedFactHorizonComparisonTableSummaryRow] = validated_summary["rows"]
    if max_rows is not None:
        if max_rows < 1:
            raise SeasonalityInputValidationError("max_rows must be at least 1 when provided")
        rows = rows[:max_rows]

    header = "| run_id | horizon | metric_id | class | status | abs_delta | standardized_delta |"
    separator = "| --- | --- | --- | --- | --- | --- | --- |"
    lines: list[str] = [header, separator]
    for row in rows:
        lines.append(
            "| "
            + " | ".join(
                [
                    row["calibration_run_id"],
                    row["horizon_label"],
                    row["metric_id"],
                    row["metric_class"],
                    row["status"],
                    _format_optional_float(row["absolute_delta"]),
                    _format_optional_float(row["standardized_delta"]),
                ],
            )
            + " |",
        )

    return "\n".join(lines)


def build_stylized_fact_plotly_figure(
    plot_payload: Mapping[str, Any],
    *,
    title: str = "Stylized Fact Absolute Delta By Horizon",
) -> Any:
    validated_plot_payload = _validate_plot_payload(plot_payload, "plot_payload")

    try:
        from plotly import graph_objects as go
    except ImportError as e:
        raise SeasonalityInputValidationError(
            "plotly is required to build stylized fact figures",
        ) from e

    figure = go.Figure()
    status_colors: dict[str, str] = {
        "pass": "#2A9D8F",
        "fail": "#E76F51",
        "insufficient_data": "#6C757D",
    }
    ordered_statuses: tuple[str, ...] = ("pass", "fail", "insufficient_data")

    for status in ordered_statuses:
        points_for_status: list[StylizedFactHorizonComparisonPlotPoint] = [
            point for point in validated_plot_payload["points"] if point["status"] == status
        ]
        if not points_for_status:
            continue

        figure.add_trace(
            go.Scatter(
                x=[point["horizon_label"] for point in points_for_status],
                y=[point["absolute_delta"] for point in points_for_status],
                mode="markers",
                name=status,
                marker={"color": status_colors[status], "size": 10},
                customdata=[
                    [
                        point["metric_id"],
                        point["metric_class"],
                        point["status"],
                        json.dumps(point["empirical_confidence_interval"]),
                        json.dumps(point["simulated_confidence_interval"]),
                        json.dumps(point["insufficient_data_reasons"]),
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
    report_bundle: Mapping[str, Any],
    *,
    title: str = "Stylized Fact Absolute Delta By Horizon",
) -> Any:
    validated_report_bundle = _validate_report_bundle_payload(report_bundle, "report_bundle")
    return build_stylized_fact_plotly_figure(validated_report_bundle["plot_payload"], title=title)


def _validate_report_bundle_payload(
    raw_report_bundle: object,
    context: str,
) -> StylizedFactHorizonComparisonReportBundle:
    report_bundle = _require_mapping(raw_report_bundle, context, context)
    schema_version = _require_non_empty_string(report_bundle, "schema_version", context)
    if schema_version != STYLIZED_FACT_HORIZON_COMPARISON_REPORT_BUNDLE_SCHEMA_VERSION:
        raise SeasonalityInputValidationError(
            f"{context}.schema_version must be {STYLIZED_FACT_HORIZON_COMPARISON_REPORT_BUNDLE_SCHEMA_VERSION}",
        )

    determinism_policy = _require_non_empty_string(report_bundle, "determinism_policy", context)
    if determinism_policy != STYLIZED_FACT_HORIZON_COMPARISON_REPORT_BUNDLE_DETERMINISM_POLICY:
        raise SeasonalityInputValidationError(
            f"{context}.determinism_policy must be {STYLIZED_FACT_HORIZON_COMPARISON_REPORT_BUNDLE_DETERMINISM_POLICY}",
        )

    output_schema_metadata = _validate_output_schema_metadata(
        _require_mapping(report_bundle, "output_schema_metadata", context),
        f"{context}.output_schema_metadata",
    )
    comparison_report = _require_mapping(report_bundle, "comparison_report", context)
    if (
        _require_non_empty_string(comparison_report, "schema_version", f"{context}.comparison_report")
        != STYLIZED_FACT_HORIZON_COMPARISON_REPORT_SCHEMA_VERSION
    ):
        raise SeasonalityInputValidationError(
            f"{context}.comparison_report.schema_version must be {STYLIZED_FACT_HORIZON_COMPARISON_REPORT_SCHEMA_VERSION}",
        )

    table_summary = _validate_table_summary_payload(
        _require_mapping(report_bundle, "table_summary", context),
        f"{context}.table_summary",
    )
    plot_payload = _validate_plot_payload(
        _require_mapping(report_bundle, "plot_payload", context),
        f"{context}.plot_payload",
    )

    artifact_export: Optional[StylizedFactHorizonComparisonArtifactExportReceipt]
    raw_artifact_export = _require_field(report_bundle, "artifact_export", context)
    if raw_artifact_export is None:
        artifact_export = None
    else:
        artifact_export = _validate_artifact_export_receipt(
            _require_mapping(report_bundle, "artifact_export", context),
            f"{context}.artifact_export",
        )

    return {
        "schema_version": schema_version,
        "determinism_policy": determinism_policy,
        "output_schema_metadata": output_schema_metadata,
        "comparison_report": dict(comparison_report),
        "table_summary": table_summary,
        "plot_payload": plot_payload,
        "artifact_export": artifact_export,
    }


def _validate_output_schema_metadata(
    metadata: Mapping[str, Any],
    context: str,
) -> StylizedFactHorizonComparisonReportBundleOutputSchemaMetadata:
    schema_version = _require_non_empty_string(metadata, "schema_version", context)
    if schema_version != STYLIZED_FACT_HORIZON_COMPARISON_REPORT_BUNDLE_OUTPUT_SCHEMA_METADATA_VERSION:
        raise SeasonalityInputValidationError(
            f"{context}.schema_version must be {STYLIZED_FACT_HORIZON_COMPARISON_REPORT_BUNDLE_OUTPUT_SCHEMA_METADATA_VERSION}",
        )

    return {
        "schema_version": schema_version,
        "required_bundle_fields": _require_non_empty_string_list(
            metadata,
            "required_bundle_fields",
            context,
        ),
        "required_table_summary_fields": _require_non_empty_string_list(
            metadata,
            "required_table_summary_fields",
            context,
        ),
        "required_table_summary_row_fields": _require_non_empty_string_list(
            metadata,
            "required_table_summary_row_fields",
            context,
        ),
        "required_plot_payload_fields": _require_non_empty_string_list(
            metadata,
            "required_plot_payload_fields",
            context,
        ),
        "required_plot_point_fields": _require_non_empty_string_list(
            metadata,
            "required_plot_point_fields",
            context,
        ),
        "decision_state_vocabulary": _require_non_empty_string_list(
            metadata,
            "decision_state_vocabulary",
            context,
        ),
        "metric_class_vocabulary": _require_non_empty_string_list(
            metadata,
            "metric_class_vocabulary",
            context,
        ),
        "insufficient_data_status_code": _require_non_empty_string(
            metadata,
            "insufficient_data_status_code",
            context,
        ),
    }


def _validate_table_summary_payload(
    table_summary: Mapping[str, Any],
    context: str,
) -> StylizedFactHorizonComparisonTableSummary:
    schema_version = _require_non_empty_string(table_summary, "schema_version", context)
    if schema_version != STYLIZED_FACT_HORIZON_COMPARISON_TABLE_SUMMARY_SCHEMA_VERSION:
        raise SeasonalityInputValidationError(
            f"{context}.schema_version must be {STYLIZED_FACT_HORIZON_COMPARISON_TABLE_SUMMARY_SCHEMA_VERSION}",
        )

    determinism_policy = _require_non_empty_string(table_summary, "determinism_policy", context)

    raw_rows = _require_field(table_summary, "rows", context)
    if not isinstance(raw_rows, list):
        raise SeasonalityInputValidationError(f"{context}.rows must be a list")

    validated_rows: list[StylizedFactHorizonComparisonTableSummaryRow] = []
    for row_position, raw_row in enumerate(raw_rows):
        row_context = f"{context}.rows[{row_position}]"
        validated_rows.append(
            _validate_table_summary_row(_require_mapping(raw_row, "row", row_context), row_context),
        )

    return {
        "schema_version": schema_version,
        "determinism_policy": determinism_policy,
        "rows": validated_rows,
    }


def _validate_table_summary_row(
    row: Mapping[str, Any],
    context: str,
) -> StylizedFactHorizonComparisonTableSummaryRow:
    return {
        "calibration_run_id": _require_non_empty_string(row, "calibration_run_id", context),
        "empirical_dataset_id": _require_non_empty_string(row, "empirical_dataset_id", context),
        "simulated_dataset_id": _require_non_empty_string(row, "simulated_dataset_id", context),
        "horizon_label": _require_non_empty_string(row, "horizon_label", context),
        "metric_id": _require_non_empty_string(row, "metric_id", context),
        "metric_name": _require_non_empty_string(row, "metric_name", context),
        "metric_variant": _require_optional_string(row, "metric_variant", context),
        "metric_class": _require_non_empty_string(row, "metric_class", context),
        "alignment_status": _require_non_empty_string(row, "alignment_status", context),
        "status": _require_non_empty_string(row, "status", context),
        "absolute_delta": _require_optional_float(row, "absolute_delta", context),
        "standardized_delta": _require_optional_float(row, "standardized_delta", context),
        "empirical_status": _require_non_empty_string(row, "empirical_status", context),
        "simulated_status": _require_non_empty_string(row, "simulated_status", context),
        "empirical_value": _require_optional_float(row, "empirical_value", context),
        "simulated_value": _require_optional_float(row, "simulated_value", context),
        "empirical_confidence_interval": _require_optional_interval_bounds(
            row,
            "empirical_confidence_interval",
            context,
        ),
        "simulated_confidence_interval": _require_optional_interval_bounds(
            row,
            "simulated_confidence_interval",
            context,
        ),
        "empirical_sample_coverage": _require_coverage_summary(
            row,
            "empirical_sample_coverage",
            context,
        ),
        "simulated_sample_coverage": _require_coverage_summary(
            row,
            "simulated_sample_coverage",
            context,
        ),
        "empirical_horizon_coverage_ratio": _require_float(row, "empirical_horizon_coverage_ratio", context),
        "simulated_horizon_coverage_ratio": _require_float(row, "simulated_horizon_coverage_ratio", context),
        "insufficient_data_reasons": _require_insufficient_data_reasons(
            row,
            "insufficient_data_reasons",
            context,
        ),
        "decision_reason_codes": _require_string_list(row, "decision_reason_codes", context),
    }


def _validate_plot_payload(
    plot_payload: Mapping[str, Any],
    context: str,
) -> StylizedFactHorizonComparisonPlotPayload:
    schema_version = _require_non_empty_string(plot_payload, "schema_version", context)
    if schema_version != STYLIZED_FACT_HORIZON_COMPARISON_PLOT_PAYLOAD_SCHEMA_VERSION:
        raise SeasonalityInputValidationError(
            f"{context}.schema_version must be {STYLIZED_FACT_HORIZON_COMPARISON_PLOT_PAYLOAD_SCHEMA_VERSION}",
        )

    determinism_policy = _require_non_empty_string(plot_payload, "determinism_policy", context)
    raw_points = _require_field(plot_payload, "points", context)
    if not isinstance(raw_points, list):
        raise SeasonalityInputValidationError(f"{context}.points must be a list")

    points: list[StylizedFactHorizonComparisonPlotPoint] = []
    for point_position, raw_point in enumerate(raw_points):
        point_context = f"{context}.points[{point_position}]"
        points.append(
            _validate_plot_point(_require_mapping(raw_point, "point", point_context), point_context),
        )

    return {
        "schema_version": schema_version,
        "determinism_policy": determinism_policy,
        "points": points,
    }


def _validate_plot_point(
    point: Mapping[str, Any],
    context: str,
) -> StylizedFactHorizonComparisonPlotPoint:
    return {
        "calibration_run_id": _require_non_empty_string(point, "calibration_run_id", context),
        "horizon_label": _require_non_empty_string(point, "horizon_label", context),
        "metric_id": _require_non_empty_string(point, "metric_id", context),
        "metric_name": _require_non_empty_string(point, "metric_name", context),
        "metric_variant": _require_optional_string(point, "metric_variant", context),
        "metric_class": _require_non_empty_string(point, "metric_class", context),
        "status": _require_non_empty_string(point, "status", context),
        "absolute_delta": _require_optional_float(point, "absolute_delta", context),
        "standardized_delta": _require_optional_float(point, "standardized_delta", context),
        "empirical_value": _require_optional_float(point, "empirical_value", context),
        "simulated_value": _require_optional_float(point, "simulated_value", context),
        "empirical_confidence_interval": _require_optional_interval_bounds(
            point,
            "empirical_confidence_interval",
            context,
        ),
        "simulated_confidence_interval": _require_optional_interval_bounds(
            point,
            "simulated_confidence_interval",
            context,
        ),
        "empirical_sample_coverage": _require_coverage_summary(
            point,
            "empirical_sample_coverage",
            context,
        ),
        "simulated_sample_coverage": _require_coverage_summary(
            point,
            "simulated_sample_coverage",
            context,
        ),
        "empirical_horizon_coverage_ratio": _require_float(point, "empirical_horizon_coverage_ratio", context),
        "simulated_horizon_coverage_ratio": _require_float(point, "simulated_horizon_coverage_ratio", context),
        "insufficient_data_reasons": _require_insufficient_data_reasons(
            point,
            "insufficient_data_reasons",
            context,
        ),
    }


def _validate_artifact_export_receipt(
    receipt: Mapping[str, Any],
    context: str,
) -> StylizedFactHorizonComparisonArtifactExportReceipt:
    raw_exported_files = _require_field(receipt, "exported_files", context)
    if not isinstance(raw_exported_files, list):
        raise SeasonalityInputValidationError(f"{context}.exported_files must be a list")

    exported_files: list[StylizedFactHorizonComparisonExportedArtifactFile] = []
    for file_position, raw_file in enumerate(raw_exported_files):
        file_context = f"{context}.exported_files[{file_position}]"
        file_mapping = _require_mapping(raw_file, "exported_file", file_context)
        exported_files.append(
            {
                "artifact_kind": _require_non_empty_string(file_mapping, "artifact_kind", file_context),
                "file_name": _require_non_empty_string(file_mapping, "file_name", file_context),
                "absolute_path": _require_non_empty_string(file_mapping, "absolute_path", file_context),
            },
        )

    return {
        "export_directory": _require_non_empty_string(receipt, "export_directory", context),
        "file_stem": _require_non_empty_string(receipt, "file_stem", context),
        "exported_files": exported_files,
    }


def _validate_artifact_export_config(artifact_export: Mapping[str, Any]) -> dict[str, str]:
    export_directory = _require_non_empty_string(
        artifact_export,
        "export_directory",
        "artifact_export",
    )
    file_stem = _require_non_empty_string(artifact_export, "file_stem", "artifact_export")

    return {"export_directory": export_directory, "file_stem": file_stem}


def _require_interval_bounds(
    interval: Mapping[str, Any],
    context: str,
) -> StylizedFactMetricIntervalBounds:
    return {
        "lower_bound": _require_float(interval, "lower_bound", context),
        "upper_bound": _require_float(interval, "upper_bound", context),
        "confidence_level": _require_float(interval, "confidence_level", context),
    }


def _require_optional_interval_bounds(
    payload: Mapping[str, Any],
    field_name: str,
    context: str,
) -> Optional[StylizedFactMetricIntervalBounds]:
    raw_interval = _require_field(payload, field_name, context)
    if raw_interval is None:
        return None

    return _require_interval_bounds(
        _require_mapping(raw_interval, field_name, context),
        f"{context}.{field_name}",
    )


def _require_coverage_summary(
    payload: Mapping[str, Any],
    field_name: str,
    context: str,
) -> StylizedFactMetricCoverageSummary:
    coverage = _require_mapping(payload, field_name, context)
    return {
        "horizon_interval_count": _require_non_negative_int(
            coverage,
            "horizon_interval_count",
            f"{context}.{field_name}",
        ),
        "covered_interval_count": _require_non_negative_int(
            coverage,
            "covered_interval_count",
            f"{context}.{field_name}",
        ),
        "sample_count": _require_non_negative_int(coverage, "sample_count", f"{context}.{field_name}"),
        "effective_sample_count": _require_non_negative_int(
            coverage,
            "effective_sample_count",
            f"{context}.{field_name}",
        ),
        "tail_observation_count": _require_non_negative_int(
            coverage,
            "tail_observation_count",
            f"{context}.{field_name}",
        ),
        "minimum_sample_count": _require_non_negative_int(
            coverage,
            "minimum_sample_count",
            f"{context}.{field_name}",
        ),
        "minimum_effective_sample_count": _require_non_negative_int(
            coverage,
            "minimum_effective_sample_count",
            f"{context}.{field_name}",
        ),
        "minimum_tail_observation_count": _require_non_negative_int(
            coverage,
            "minimum_tail_observation_count",
            f"{context}.{field_name}",
        ),
        "minimum_horizon_coverage_ratio_scaled_bps": _require_non_negative_int(
            coverage,
            "minimum_horizon_coverage_ratio_scaled_bps",
            f"{context}.{field_name}",
        ),
    }


def _require_insufficient_data_reasons(
    payload: Mapping[str, Any],
    field_name: str,
    context: str,
) -> StylizedFactMetricInsufficientDataReasons:
    reasons = _require_mapping(payload, field_name, context)
    return {
        "empirical": _require_string_list(reasons, "empirical", f"{context}.{field_name}"),
        "simulated": _require_string_list(reasons, "simulated", f"{context}.{field_name}"),
    }


def _require_non_negative_int(payload: Mapping[str, Any], field_name: str, context: str) -> int:
    value = _require_field(payload, field_name, context)
    if not isinstance(value, int) or isinstance(value, bool) or value < 0:
        raise SeasonalityInputValidationError(f"{context}.{field_name} must be a non-negative integer")

    return value


def _require_float(payload: Mapping[str, Any], field_name: str, context: str) -> float:
    value = _require_field(payload, field_name, context)
    if not isinstance(value, int | float) or isinstance(value, bool):
        raise SeasonalityInputValidationError(f"{context}.{field_name} must be numeric")

    parsed_value = float(value)
    if not math.isfinite(parsed_value):
        raise SeasonalityInputValidationError(f"{context}.{field_name} must be finite")

    return parsed_value


def _require_optional_float(
    payload: Mapping[str, Any],
    field_name: str,
    context: str,
) -> Optional[float]:
    value = _require_field(payload, field_name, context)
    if value is None:
        return None
    if not isinstance(value, int | float) or isinstance(value, bool):
        raise SeasonalityInputValidationError(f"{context}.{field_name} must be numeric or null")

    parsed_value = float(value)
    if not math.isfinite(parsed_value):
        raise SeasonalityInputValidationError(f"{context}.{field_name} must be finite when provided")

    return parsed_value


def _require_optional_string(
    payload: Mapping[str, Any],
    field_name: str,
    context: str,
) -> Optional[str]:
    value = _require_field(payload, field_name, context)
    if value is None:
        return None
    if not isinstance(value, str) or not value.strip():
        raise SeasonalityInputValidationError(f"{context}.{field_name} must be a non-empty string or null")

    return value.strip()


def _require_non_empty_string(payload: Mapping[str, Any], field_name: str, context: str) -> str:
    value = _require_field(payload, field_name, context)
    if not isinstance(value, str) or not value.strip():
        raise SeasonalityInputValidationError(f"{context}.{field_name} must be a non-empty string")

    return value.strip()


def _require_non_empty_string_list(
    payload: Mapping[str, Any],
    field_name: str,
    context: str,
) -> list[str]:
    values = _require_string_list(payload, field_name, context)
    if not values:
        raise SeasonalityInputValidationError(f"{context}.{field_name} must not be empty")

    return values


def _require_string_list(payload: Mapping[str, Any], field_name: str, context: str) -> list[str]:
    raw_values = _require_field(payload, field_name, context)
    if not isinstance(raw_values, Sequence) or isinstance(raw_values, str | bytes):
        raise SeasonalityInputValidationError(f"{context}.{field_name} must be a string list")

    values: list[str] = []
    for value_position, raw_value in enumerate(raw_values):
        if not isinstance(raw_value, str) or not raw_value.strip():
            raise SeasonalityInputValidationError(
                f"{context}.{field_name}[{value_position}] must be a non-empty string",
            )
        values.append(raw_value.strip())

    return values


def _require_mapping(payload: object, field_name: str, context: str) -> Mapping[str, Any]:
    if not isinstance(payload, Mapping):
        raise SeasonalityInputValidationError(f"{context} must be a mapping")
    payload_mapping = cast(Mapping[str, Any], payload)

    if field_name in payload_mapping:
        field_value = payload_mapping[field_name]
        if not isinstance(field_value, Mapping):
            raise SeasonalityInputValidationError(f"{context}.{field_name} must be a mapping")
        return cast(Mapping[str, Any], field_value)

    return payload_mapping


def _require_field(payload: Mapping[str, Any], field_name: str, context: str) -> object:
    if field_name not in payload:
        raise SeasonalityInputValidationError(f"{context}.{field_name} is required")

    return payload[field_name]


def _format_optional_float(value: Optional[float]) -> str:
    if value is None:
        return "n/a"

    return f"{value:.6f}"


__all__ = [
    "STYLIZED_FACT_HORIZON_COMPARISON_REPORT_SCHEMA_VERSION",
    "STYLIZED_FACT_HORIZON_COMPARISON_REPORT_BUNDLE_SCHEMA_VERSION",
    "STYLIZED_FACT_HORIZON_COMPARISON_REPORT_BUNDLE_DETERMINISM_POLICY",
    "STYLIZED_FACT_HORIZON_COMPARISON_REPORT_BUNDLE_OUTPUT_SCHEMA_METADATA_VERSION",
    "STYLIZED_FACT_HORIZON_COMPARISON_TABLE_SUMMARY_SCHEMA_VERSION",
    "STYLIZED_FACT_HORIZON_COMPARISON_PLOT_PAYLOAD_SCHEMA_VERSION",
    "StylizedFactHorizonComparisonReportBundle",
    "StylizedFactHorizonComparisonReportBundleOutputSchemaMetadata",
    "StylizedFactHorizonComparisonTableSummary",
    "StylizedFactHorizonComparisonTableSummaryRow",
    "StylizedFactHorizonComparisonPlotPayload",
    "StylizedFactHorizonComparisonPlotPoint",
    "StylizedFactHorizonComparisonArtifactExportReceipt",
    "run_stylized_fact_horizon_comparison_report_generator",
    "render_stylized_fact_table_markdown",
    "build_stylized_fact_plotly_figure",
    "build_stylized_fact_plotly_figure_from_report_bundle",
]
