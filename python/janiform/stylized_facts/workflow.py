"""Stylized-fact reporting workflows and internal validation helpers."""

import json
from collections.abc import Mapping
from typing import Any, Optional

import orjson

from .._native_api import native_module
from ..contracts.base import (
    require_field as _require_field_base,
    require_finite_number,
    require_mapping as _require_mapping_base,
    require_mapping_value,
    require_non_empty_string as _require_non_empty_string_base,
    require_non_empty_string_sequence,
    require_non_negative_integer,
    require_optional_boolean_value,
    require_optional_finite_scalar,
    require_optional_string_value,
    require_string_sequence,
)
from ..seasonality import SeasonalityInputValidationError
from .contracts import (
    StylizedFactDatasetMetricObservedCounts,
    StylizedFactDatasetMetricReport,
    StylizedFactDatasetMetricRequiredMinimums,
    StylizedFactDatasetMetricSampleCoverage,
    StylizedFactDecisionCounts,
    StylizedFactHorizonComparisonArtifactExportReceipt,
    StylizedFactHorizonComparisonExportedArtifactFile,
    StylizedFactHorizonComparisonHorizonReport,
    StylizedFactHorizonComparisonOutputSchemaMetadata,
    StylizedFactHorizonComparisonPlotPayload,
    StylizedFactHorizonComparisonPlotPoint,
    StylizedFactHorizonComparisonReport,
    StylizedFactHorizonComparisonReportBundle,
    StylizedFactHorizonComparisonReportBundleOutputSchemaMetadata,
    StylizedFactHorizonComparisonReportSummary,
    StylizedFactHorizonComparisonRunReport,
    StylizedFactHorizonComparisonTableSummary,
    StylizedFactHorizonComparisonTableSummaryRow,
    StylizedFactMetricCoverageSummary,
    StylizedFactMetricComparisonReport,
    StylizedFactMetricInsufficientDataReasons,
    StylizedFactMetricIntervalBounds,
    StylizedFactThresholdContext,
    StylizedFactUncertaintyContext,
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
    except json.JSONDecodeError as e:
        raise SeasonalityInputValidationError(
            f"stylized fact report bundle must be valid JSON: {e.msg}",
        ) from e

    return _validate_report_bundle_payload(parsed_report_bundle, "report_bundle")


def render_stylized_fact_table_markdown(
    table_summary: StylizedFactHorizonComparisonTableSummary,
    *,
    max_rows: Optional[int] = None,
) -> str:
    rows: list[StylizedFactHorizonComparisonTableSummaryRow] = table_summary.rows
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
                    row.calibration_run_id,
                    row.horizon_label,
                    row.metric_id,
                    row.metric_class,
                    row.status,
                    _format_optional_float(row.absolute_delta),
                    _format_optional_float(row.standardized_delta),
                ],
            )
            + " |",
        )

    return "\n".join(lines)


def build_stylized_fact_plotly_figure(
    plot_payload: StylizedFactHorizonComparisonPlotPayload,
    *,
    title: str = "Stylized Fact Absolute Delta By Horizon",
) -> Any:
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
    comparison_report = _validate_comparison_report(
        _require_mapping(report_bundle, "comparison_report", context),
        f"{context}.comparison_report",
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

    return StylizedFactHorizonComparisonReportBundle(
        schema_version=schema_version,
        determinism_policy=determinism_policy,
        output_schema_metadata=output_schema_metadata,
        comparison_report=comparison_report,
        table_summary=table_summary,
        plot_payload=plot_payload,
        artifact_export=artifact_export,
    )


def _validate_comparison_report(
    comparison_report: Mapping[str, Any],
    context: str,
) -> StylizedFactHorizonComparisonReport:
    schema_version = _require_non_empty_string(comparison_report, "schema_version", context)
    if schema_version != STYLIZED_FACT_HORIZON_COMPARISON_REPORT_SCHEMA_VERSION:
        raise SeasonalityInputValidationError(
            f"{context}.schema_version must be {STYLIZED_FACT_HORIZON_COMPARISON_REPORT_SCHEMA_VERSION}",
        )

    raw_run_reports = _require_field(comparison_report, "run_reports", context)
    if not isinstance(raw_run_reports, list):
        raise SeasonalityInputValidationError(f"{context}.run_reports must be a list")

    run_reports: list[StylizedFactHorizonComparisonRunReport] = []
    for run_position, raw_run_report in enumerate(raw_run_reports):
        run_context = f"{context}.run_reports[{run_position}]"
        run_reports.append(
            _validate_comparison_run_report(
                _require_mapping(raw_run_report, "run_report", run_context),
                run_context,
            ),
        )

    return StylizedFactHorizonComparisonReport(
        schema_version=schema_version,
        decision_policy=_require_non_empty_string(comparison_report, "decision_policy", context),
        uncertainty_policy=_require_non_empty_string(comparison_report, "uncertainty_policy", context),
        insufficient_data_status_code=_require_non_empty_string(
            comparison_report,
            "insufficient_data_status_code",
            context,
        ),
        output_schema_metadata=_validate_comparison_output_schema_metadata(
            _require_mapping(comparison_report, "output_schema_metadata", context),
            f"{context}.output_schema_metadata",
        ),
        metric_catalog_id=_require_non_empty_string(comparison_report, "metric_catalog_id", context),
        metric_catalog_version=_require_non_empty_string(
            comparison_report,
            "metric_catalog_version",
            context,
        ),
        threshold_policy_id=_require_non_empty_string(comparison_report, "threshold_policy_id", context),
        status=_require_non_empty_string(comparison_report, "status", context),
        summary=_validate_comparison_report_summary(
            _require_mapping(comparison_report, "summary", context),
            f"{context}.summary",
        ),
        run_reports=run_reports,
    )


def _validate_comparison_output_schema_metadata(
    metadata: Mapping[str, Any],
    context: str,
) -> StylizedFactHorizonComparisonOutputSchemaMetadata:
    return StylizedFactHorizonComparisonOutputSchemaMetadata(
        schema_version=_require_non_empty_string(metadata, "schema_version", context),
        required_batch_report_fields=_require_non_empty_string_list(
            metadata,
            "required_batch_report_fields",
            context,
        ),
        required_run_report_fields=_require_non_empty_string_list(
            metadata,
            "required_run_report_fields",
            context,
        ),
        required_horizon_report_fields=_require_non_empty_string_list(
            metadata,
            "required_horizon_report_fields",
            context,
        ),
        required_metric_report_fields=_require_non_empty_string_list(
            metadata,
            "required_metric_report_fields",
            context,
        ),
        required_dataset_metric_fields=_require_non_empty_string_list(
            metadata,
            "required_dataset_metric_fields",
            context,
        ),
        required_decision_count_fields=_require_non_empty_string_list(
            metadata,
            "required_decision_count_fields",
            context,
        ),
        decision_state_vocabulary=_require_non_empty_string_list(
            metadata,
            "decision_state_vocabulary",
            context,
        ),
        horizon_alignment_status_vocabulary=_require_non_empty_string_list(
            metadata,
            "horizon_alignment_status_vocabulary",
            context,
        ),
        metric_alignment_status_vocabulary=_require_non_empty_string_list(
            metadata,
            "metric_alignment_status_vocabulary",
            context,
        ),
        dataset_metric_status_vocabulary=_require_non_empty_string_list(
            metadata,
            "dataset_metric_status_vocabulary",
            context,
        ),
        decision_reason_vocabulary=_require_non_empty_string_list(
            metadata,
            "decision_reason_vocabulary",
            context,
        ),
        sample_sufficiency_reason_vocabulary=_require_non_empty_string_list(
            metadata,
            "sample_sufficiency_reason_vocabulary",
            context,
        ),
    )


def _validate_comparison_report_summary(
    summary: Mapping[str, Any],
    context: str,
) -> StylizedFactHorizonComparisonReportSummary:
    return StylizedFactHorizonComparisonReportSummary(
        total_run_count=_require_non_negative_int(summary, "total_run_count", context),
        total_metric_count=_require_non_negative_int(summary, "total_metric_count", context),
        run_decision_counts=_validate_decision_counts(
            _require_mapping(summary, "run_decision_counts", context),
            f"{context}.run_decision_counts",
        ),
        metric_decision_counts=_validate_decision_counts(
            _require_mapping(summary, "metric_decision_counts", context),
            f"{context}.metric_decision_counts",
        ),
    )


def _validate_decision_counts(
    decision_counts: Mapping[str, Any],
    context: str,
) -> StylizedFactDecisionCounts:
    return StylizedFactDecisionCounts(
        pass_count=_require_non_negative_int(decision_counts, "pass_count", context),
        fail_count=_require_non_negative_int(decision_counts, "fail_count", context),
        insufficient_data_count=_require_non_negative_int(
            decision_counts,
            "insufficient_data_count",
            context,
        ),
    )


def _validate_comparison_run_report(
    run_report: Mapping[str, Any],
    context: str,
) -> StylizedFactHorizonComparisonRunReport:
    raw_horizons = _require_field(run_report, "horizons", context)
    if not isinstance(raw_horizons, list):
        raise SeasonalityInputValidationError(f"{context}.horizons must be a list")

    horizons: list[StylizedFactHorizonComparisonHorizonReport] = []
    for horizon_position, raw_horizon in enumerate(raw_horizons):
        horizon_context = f"{context}.horizons[{horizon_position}]"
        horizons.append(
            _validate_comparison_horizon_report(
                _require_mapping(raw_horizon, "horizon_report", horizon_context),
                horizon_context,
            ),
        )

    return StylizedFactHorizonComparisonRunReport(
        calibration_run_id=_require_non_empty_string(run_report, "calibration_run_id", context),
        empirical_dataset_id=_require_non_empty_string(run_report, "empirical_dataset_id", context),
        simulated_dataset_id=_require_non_empty_string(run_report, "simulated_dataset_id", context),
        status=_require_non_empty_string(run_report, "status", context),
        decision_counts=_validate_decision_counts(
            _require_mapping(run_report, "decision_counts", context),
            f"{context}.decision_counts",
        ),
        total_horizon_count=_require_non_negative_int(run_report, "total_horizon_count", context),
        total_metric_count=_require_non_negative_int(run_report, "total_metric_count", context),
        horizons=horizons,
    )


def _validate_comparison_horizon_report(
    horizon_report: Mapping[str, Any],
    context: str,
) -> StylizedFactHorizonComparisonHorizonReport:
    raw_metrics = _require_field(horizon_report, "metrics", context)
    if not isinstance(raw_metrics, list):
        raise SeasonalityInputValidationError(f"{context}.metrics must be a list")

    metrics: list[StylizedFactMetricComparisonReport] = []
    for metric_position, raw_metric in enumerate(raw_metrics):
        metric_context = f"{context}.metrics[{metric_position}]"
        metrics.append(
            _validate_metric_comparison_report(
                _require_mapping(raw_metric, "metric_report", metric_context),
                metric_context,
            ),
        )

    return StylizedFactHorizonComparisonHorizonReport(
        horizon_label=_require_non_empty_string(horizon_report, "horizon_label", context),
        empirical_horizon_duration_nanos=_require_non_negative_int(
            horizon_report,
            "empirical_horizon_duration_nanos",
            context,
        ),
        simulated_horizon_duration_nanos=_require_non_negative_int(
            horizon_report,
            "simulated_horizon_duration_nanos",
            context,
        ),
        alignment_status=_require_non_empty_string(horizon_report, "alignment_status", context),
        status=_require_non_empty_string(horizon_report, "status", context),
        decision_counts=_validate_decision_counts(
            _require_mapping(horizon_report, "decision_counts", context),
            f"{context}.decision_counts",
        ),
        metrics=metrics,
    )


def _validate_metric_comparison_report(
    metric_report: Mapping[str, Any],
    context: str,
) -> StylizedFactMetricComparisonReport:
    return StylizedFactMetricComparisonReport(
        metric_id=_require_non_empty_string(metric_report, "metric_id", context),
        metric_name=_require_non_empty_string(metric_report, "metric_name", context),
        metric_variant=_require_optional_string(metric_report, "metric_variant", context),
        alignment_status=_require_non_empty_string(metric_report, "alignment_status", context),
        status=_require_non_empty_string(metric_report, "status", context),
        empirical=_validate_dataset_metric_report(
            _require_mapping(metric_report, "empirical", context),
            f"{context}.empirical",
        ),
        simulated=_validate_dataset_metric_report(
            _require_mapping(metric_report, "simulated", context),
            f"{context}.simulated",
        ),
        absolute_delta=_require_optional_float(metric_report, "absolute_delta", context),
        uncertainty_context=_validate_uncertainty_context(
            _require_mapping(metric_report, "uncertainty_context", context),
            f"{context}.uncertainty_context",
        ),
        threshold_context=_validate_threshold_context(
            _require_mapping(metric_report, "threshold_context", context),
            f"{context}.threshold_context",
        ),
        horizon_label=_require_non_empty_string(metric_report, "horizon_label", context),
        decision_reason_codes=_require_string_list(metric_report, "decision_reason_codes", context),
    )


def _validate_dataset_metric_report(
    dataset_metric_report: Mapping[str, Any],
    context: str,
) -> StylizedFactDatasetMetricReport:
    return StylizedFactDatasetMetricReport(
        status=_require_non_empty_string(dataset_metric_report, "status", context),
        value=_require_optional_float(dataset_metric_report, "value", context),
        confidence_interval=_require_optional_interval_bounds(
            dataset_metric_report,
            "confidence_interval",
            context,
        ),
        sample_coverage=_validate_dataset_metric_sample_coverage(
            _require_mapping(dataset_metric_report, "sample_coverage", context),
            f"{context}.sample_coverage",
        ),
        observed_counts=_validate_dataset_metric_observed_counts(
            _require_mapping(dataset_metric_report, "observed_counts", context),
            f"{context}.observed_counts",
        ),
        required_minimums=_validate_dataset_metric_required_minimums(
            _require_mapping(dataset_metric_report, "required_minimums", context),
            f"{context}.required_minimums",
        ),
        horizon_coverage_ratio=_require_float(dataset_metric_report, "horizon_coverage_ratio", context),
        insufficiency_reason_codes=_require_string_list(
            dataset_metric_report,
            "insufficiency_reason_codes",
            context,
        ),
        input_present=_require_boolean(dataset_metric_report, "input_present", context),
    )


def _validate_dataset_metric_sample_coverage(
    sample_coverage: Mapping[str, Any],
    context: str,
) -> StylizedFactDatasetMetricSampleCoverage:
    return StylizedFactDatasetMetricSampleCoverage(
        horizon_interval_count=_require_non_negative_int(
            sample_coverage,
            "horizon_interval_count",
            context,
        ),
        covered_interval_count=_require_non_negative_int(
            sample_coverage,
            "covered_interval_count",
            context,
        ),
    )


def _validate_dataset_metric_observed_counts(
    observed_counts: Mapping[str, Any],
    context: str,
) -> StylizedFactDatasetMetricObservedCounts:
    return StylizedFactDatasetMetricObservedCounts(
        sample_count=_require_non_negative_int(observed_counts, "sample_count", context),
        effective_sample_count=_require_non_negative_int(
            observed_counts,
            "effective_sample_count",
            context,
        ),
        tail_observation_count=_require_non_negative_int(
            observed_counts,
            "tail_observation_count",
            context,
        ),
    )


def _validate_dataset_metric_required_minimums(
    required_minimums: Mapping[str, Any],
    context: str,
) -> StylizedFactDatasetMetricRequiredMinimums:
    return StylizedFactDatasetMetricRequiredMinimums(
        minimum_sample_count=_require_non_negative_int(required_minimums, "minimum_sample_count", context),
        minimum_effective_sample_count=_require_non_negative_int(
            required_minimums,
            "minimum_effective_sample_count",
            context,
        ),
        minimum_tail_observation_count=_require_non_negative_int(
            required_minimums,
            "minimum_tail_observation_count",
            context,
        ),
        minimum_horizon_coverage_ratio=_require_float(
            required_minimums,
            "minimum_horizon_coverage_ratio",
            context,
        ),
    )


def _validate_threshold_context(
    threshold_context: Mapping[str, Any],
    context: str,
) -> StylizedFactThresholdContext:
    return StylizedFactThresholdContext(
        policy_id=_require_non_empty_string(threshold_context, "policy_id", context),
        max_absolute_delta=_require_float(threshold_context, "max_absolute_delta", context),
        max_standardized_delta=_require_float(threshold_context, "max_standardized_delta", context),
        absolute_delta_exceeds_threshold=_require_optional_boolean(
            threshold_context,
            "absolute_delta_exceeds_threshold",
            context,
        ),
        standardized_delta_exceeds_threshold=_require_optional_boolean(
            threshold_context,
            "standardized_delta_exceeds_threshold",
            context,
        ),
    )


def _validate_uncertainty_context(
    uncertainty_context: Mapping[str, Any],
    context: str,
) -> StylizedFactUncertaintyContext:
    return StylizedFactUncertaintyContext(
        empirical_interval_half_width=_require_optional_float(
            uncertainty_context,
            "empirical_interval_half_width",
            context,
        ),
        simulated_interval_half_width=_require_optional_float(
            uncertainty_context,
            "simulated_interval_half_width",
            context,
        ),
        pooled_interval_half_width=_require_optional_float(
            uncertainty_context,
            "pooled_interval_half_width",
            context,
        ),
        standardized_delta=_require_optional_float(uncertainty_context, "standardized_delta", context),
    )


def _validate_output_schema_metadata(
    metadata: Mapping[str, Any],
    context: str,
) -> StylizedFactHorizonComparisonReportBundleOutputSchemaMetadata:
    schema_version = _require_non_empty_string(metadata, "schema_version", context)
    if schema_version != STYLIZED_FACT_HORIZON_COMPARISON_REPORT_BUNDLE_OUTPUT_SCHEMA_METADATA_VERSION:
        raise SeasonalityInputValidationError(
            f"{context}.schema_version must be {STYLIZED_FACT_HORIZON_COMPARISON_REPORT_BUNDLE_OUTPUT_SCHEMA_METADATA_VERSION}",
        )

    return StylizedFactHorizonComparisonReportBundleOutputSchemaMetadata(
        schema_version=schema_version,
        required_bundle_fields=_require_non_empty_string_list(
            metadata,
            "required_bundle_fields",
            context,
        ),
        required_table_summary_fields=_require_non_empty_string_list(
            metadata,
            "required_table_summary_fields",
            context,
        ),
        required_table_summary_row_fields=_require_non_empty_string_list(
            metadata,
            "required_table_summary_row_fields",
            context,
        ),
        required_plot_payload_fields=_require_non_empty_string_list(
            metadata,
            "required_plot_payload_fields",
            context,
        ),
        required_plot_point_fields=_require_non_empty_string_list(
            metadata,
            "required_plot_point_fields",
            context,
        ),
        decision_state_vocabulary=_require_non_empty_string_list(
            metadata,
            "decision_state_vocabulary",
            context,
        ),
        metric_class_vocabulary=_require_non_empty_string_list(
            metadata,
            "metric_class_vocabulary",
            context,
        ),
        insufficient_data_status_code=_require_non_empty_string(
            metadata,
            "insufficient_data_status_code",
            context,
        ),
    )


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

    return StylizedFactHorizonComparisonTableSummary(
        schema_version=schema_version,
        determinism_policy=determinism_policy,
        rows=validated_rows,
    )


def _validate_table_summary_row(
    row: Mapping[str, Any],
    context: str,
) -> StylizedFactHorizonComparisonTableSummaryRow:
    return StylizedFactHorizonComparisonTableSummaryRow(
        calibration_run_id=_require_non_empty_string(row, "calibration_run_id", context),
        empirical_dataset_id=_require_non_empty_string(row, "empirical_dataset_id", context),
        simulated_dataset_id=_require_non_empty_string(row, "simulated_dataset_id", context),
        horizon_label=_require_non_empty_string(row, "horizon_label", context),
        metric_id=_require_non_empty_string(row, "metric_id", context),
        metric_name=_require_non_empty_string(row, "metric_name", context),
        metric_variant=_require_optional_string(row, "metric_variant", context),
        metric_class=_require_non_empty_string(row, "metric_class", context),
        alignment_status=_require_non_empty_string(row, "alignment_status", context),
        status=_require_non_empty_string(row, "status", context),
        absolute_delta=_require_optional_float(row, "absolute_delta", context),
        standardized_delta=_require_optional_float(row, "standardized_delta", context),
        empirical_status=_require_non_empty_string(row, "empirical_status", context),
        simulated_status=_require_non_empty_string(row, "simulated_status", context),
        empirical_value=_require_optional_float(row, "empirical_value", context),
        simulated_value=_require_optional_float(row, "simulated_value", context),
        empirical_confidence_interval=_require_optional_interval_bounds(
            row,
            "empirical_confidence_interval",
            context,
        ),
        simulated_confidence_interval=_require_optional_interval_bounds(
            row,
            "simulated_confidence_interval",
            context,
        ),
        empirical_sample_coverage=_require_coverage_summary(
            row,
            "empirical_sample_coverage",
            context,
        ),
        simulated_sample_coverage=_require_coverage_summary(
            row,
            "simulated_sample_coverage",
            context,
        ),
        empirical_horizon_coverage_ratio=_require_float(row, "empirical_horizon_coverage_ratio", context),
        simulated_horizon_coverage_ratio=_require_float(row, "simulated_horizon_coverage_ratio", context),
        insufficient_data_reasons=_require_insufficient_data_reasons(
            row,
            "insufficient_data_reasons",
            context,
        ),
        decision_reason_codes=_require_string_list(row, "decision_reason_codes", context),
    )


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

    return StylizedFactHorizonComparisonPlotPayload(
        schema_version=schema_version,
        determinism_policy=determinism_policy,
        points=points,
    )


def _validate_plot_point(
    point: Mapping[str, Any],
    context: str,
) -> StylizedFactHorizonComparisonPlotPoint:
    return StylizedFactHorizonComparisonPlotPoint(
        calibration_run_id=_require_non_empty_string(point, "calibration_run_id", context),
        horizon_label=_require_non_empty_string(point, "horizon_label", context),
        metric_id=_require_non_empty_string(point, "metric_id", context),
        metric_name=_require_non_empty_string(point, "metric_name", context),
        metric_variant=_require_optional_string(point, "metric_variant", context),
        metric_class=_require_non_empty_string(point, "metric_class", context),
        status=_require_non_empty_string(point, "status", context),
        absolute_delta=_require_optional_float(point, "absolute_delta", context),
        standardized_delta=_require_optional_float(point, "standardized_delta", context),
        empirical_value=_require_optional_float(point, "empirical_value", context),
        simulated_value=_require_optional_float(point, "simulated_value", context),
        empirical_confidence_interval=_require_optional_interval_bounds(
            point,
            "empirical_confidence_interval",
            context,
        ),
        simulated_confidence_interval=_require_optional_interval_bounds(
            point,
            "simulated_confidence_interval",
            context,
        ),
        empirical_sample_coverage=_require_coverage_summary(
            point,
            "empirical_sample_coverage",
            context,
        ),
        simulated_sample_coverage=_require_coverage_summary(
            point,
            "simulated_sample_coverage",
            context,
        ),
        empirical_horizon_coverage_ratio=_require_float(point, "empirical_horizon_coverage_ratio", context),
        simulated_horizon_coverage_ratio=_require_float(point, "simulated_horizon_coverage_ratio", context),
        insufficient_data_reasons=_require_insufficient_data_reasons(
            point,
            "insufficient_data_reasons",
            context,
        ),
    )


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
            StylizedFactHorizonComparisonExportedArtifactFile(
                artifact_kind=_require_non_empty_string(file_mapping, "artifact_kind", file_context),
                file_name=_require_non_empty_string(file_mapping, "file_name", file_context),
                absolute_path=_require_non_empty_string(file_mapping, "absolute_path", file_context),
            ),
        )

    return StylizedFactHorizonComparisonArtifactExportReceipt(
        export_directory=_require_non_empty_string(receipt, "export_directory", context),
        file_stem=_require_non_empty_string(receipt, "file_stem", context),
        exported_files=exported_files,
    )


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
    return StylizedFactMetricIntervalBounds(
        lower_bound=_require_float(interval, "lower_bound", context),
        upper_bound=_require_float(interval, "upper_bound", context),
        confidence_level=_require_float(interval, "confidence_level", context),
    )


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
    return StylizedFactMetricCoverageSummary(
        horizon_interval_count=_require_non_negative_int(
            coverage,
            "horizon_interval_count",
            f"{context}.{field_name}",
        ),
        covered_interval_count=_require_non_negative_int(
            coverage,
            "covered_interval_count",
            f"{context}.{field_name}",
        ),
        sample_count=_require_non_negative_int(coverage, "sample_count", f"{context}.{field_name}"),
        effective_sample_count=_require_non_negative_int(
            coverage,
            "effective_sample_count",
            f"{context}.{field_name}",
        ),
        tail_observation_count=_require_non_negative_int(
            coverage,
            "tail_observation_count",
            f"{context}.{field_name}",
        ),
        minimum_sample_count=_require_non_negative_int(
            coverage,
            "minimum_sample_count",
            f"{context}.{field_name}",
        ),
        minimum_effective_sample_count=_require_non_negative_int(
            coverage,
            "minimum_effective_sample_count",
            f"{context}.{field_name}",
        ),
        minimum_tail_observation_count=_require_non_negative_int(
            coverage,
            "minimum_tail_observation_count",
            f"{context}.{field_name}",
        ),
        minimum_horizon_coverage_ratio_scaled_bps=_require_non_negative_int(
            coverage,
            "minimum_horizon_coverage_ratio_scaled_bps",
            f"{context}.{field_name}",
        ),
    )


def _require_insufficient_data_reasons(
    payload: Mapping[str, Any],
    field_name: str,
    context: str,
) -> StylizedFactMetricInsufficientDataReasons:
    reasons = _require_mapping(payload, field_name, context)
    return StylizedFactMetricInsufficientDataReasons(
        empirical=_require_string_list(reasons, "empirical", f"{context}.{field_name}"),
        simulated=_require_string_list(reasons, "simulated", f"{context}.{field_name}"),
    )


def _require_non_negative_int(payload: Mapping[str, Any], field_name: str, context: str) -> int:
    return require_non_negative_integer(
        payload,
        field_name,
        f"{context}.{field_name}",
        _stylized_fact_error,
    )


def _require_float(payload: Mapping[str, Any], field_name: str, context: str) -> float:
    return require_finite_number(
        payload,
        field_name,
        f"{context}.{field_name}",
        _stylized_fact_error,
    )


def _require_optional_float(
    payload: Mapping[str, Any],
    field_name: str,
    context: str,
) -> Optional[float]:
    return require_optional_finite_scalar(
        _require_field(payload, field_name, context),
        f"{context}.{field_name}",
        _stylized_fact_error,
    )


def _require_optional_string(
    payload: Mapping[str, Any],
    field_name: str,
    context: str,
) -> Optional[str]:
    value = require_optional_string_value(
        _require_field(payload, field_name, context),
        f"{context}.{field_name}",
        _stylized_fact_error,
    )
    if value is None:
        return None

    normalized_value = value.strip()
    if not normalized_value:
        raise _stylized_fact_error(
            f"{context}.{field_name}",
            "must be a non-empty string or null",
        )

    return normalized_value


def _require_boolean(payload: Mapping[str, Any], field_name: str, context: str) -> bool:
    raw_value = _require_field(payload, field_name, context)
    if not isinstance(raw_value, bool):
        raise _stylized_fact_error(f"{context}.{field_name}", "must be a boolean")

    return raw_value


def _require_optional_boolean(
    payload: Mapping[str, Any],
    field_name: str,
    context: str,
) -> Optional[bool]:
    return require_optional_boolean_value(
        _require_field(payload, field_name, context),
        f"{context}.{field_name}",
        _stylized_fact_error,
    )


def _require_non_empty_string(payload: Mapping[str, Any], field_name: str, context: str) -> str:
    return _require_non_empty_string_base(
        payload,
        field_name,
        f"{context}.{field_name}",
        _stylized_fact_error,
    ).strip()


def _require_non_empty_string_list(
    payload: Mapping[str, Any],
    field_name: str,
    context: str,
) -> list[str]:
    return require_non_empty_string_sequence(
        payload,
        field_name,
        f"{context}.{field_name}",
        _stylized_fact_error,
        require_non_empty_items=True,
        strip_items=True,
    )


def _require_string_list(payload: Mapping[str, Any], field_name: str, context: str) -> list[str]:
    return require_string_sequence(
        payload,
        field_name,
        f"{context}.{field_name}",
        _stylized_fact_error,
        require_non_empty_items=True,
        strip_items=True,
    )


def _require_mapping(payload: object, field_name: str, context: str) -> Mapping[str, Any]:
    if isinstance(payload, Mapping) and field_name not in payload:
        return payload

    if isinstance(payload, Mapping):
        return _require_mapping_base(
            payload,
            field_name,
            f"{context}.{field_name}",
            _stylized_fact_error,
        )

    return require_mapping_value(
        payload,
        context,
        _stylized_fact_error,
    )


def _require_field(payload: Mapping[str, Any], field_name: str, context: str) -> Any:
    return _require_field_base(
        payload,
        field_name,
        f"{context}.{field_name}",
        _stylized_fact_error,
    )


def _stylized_fact_error(field_path: str, message: str) -> SeasonalityInputValidationError:
    return SeasonalityInputValidationError(f"{field_path} {message}")


def _format_optional_float(value: Optional[float]) -> str:
    if value is None:
        return "n/a"

    return f"{value:.6f}"
