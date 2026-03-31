"""Internal CLI rendering helpers for JSON and human output modes."""

import json
import sys
from collections.abc import Callable
from enum import StrEnum
from typing import Any, Optional, cast

from ._constants import CliExitCode, CliOutputMode
from ..calibration.presets import DEFAULT_HIGH_LEVEL_KNOWN_NEWS_PRESET_NAME
from ..errors import JaniformError, JaniformErrorCategory


def write_contract_json(payload: dict[str, Any]) -> None:
    sys.stdout.write(f"{json.dumps(payload, sort_keys=True)}\n")


def exit_code_for_error(error: JaniformError) -> CliExitCode:
    match error.category:
        case JaniformErrorCategory.VALIDATION:
            return CliExitCode.VALIDATION_ERROR
        case JaniformErrorCategory.EXECUTION:
            return CliExitCode.RUNTIME_ERROR
        case JaniformErrorCategory.INTEGRATION:
            return CliExitCode.INTERNAL_ERROR


def write_cli_error(error: JaniformError, *, output: Optional[str] = None) -> int:
    if output == CliOutputMode.HUMAN.value:
        sys.stderr.write(render_cli_error_human_output(error))
    else:
        sys.stderr.write(f"{error}\n")

    return int(exit_code_for_error(error))


def render_cli_error_human_output(error: JaniformError) -> str:
    details = error.details

    lines = [
        f"{details.entrypoint} failed:",
        f"  reason: {details.message}",
        f"  category: {details.category.value}",
        f"  error id: {details.error_id}",
    ]

    if details.field_path is not None:
        lines.append(f"  field: {details.field_path}")

    for field_name, field_value in ordered_cli_error_context_entries(details.context):
        if field_value is None:
            continue

        lines.append(f"  {field_name}: {field_value}")

    return "\n".join(lines) + "\n"


def ordered_cli_error_context_entries(context: dict[str, Any]) -> list[tuple[str, Any]]:
    label_by_field_name = {
        "failed_stage": "failed stage",
        "failure_kind": "failure kind",
        "failure_error_message": "stage error",
        "run_id": "run id",
        "staged_status": "staged status",
        "checkpoint_reference": "checkpoint reference",
        "artifact_reference": "artifact reference",
    }
    preferred_field_order = (
        "failed_stage",
        "failure_kind",
        "failure_error_message",
        "field_path",
        "run_id",
        "staged_status",
        "checkpoint_reference",
        "artifact_reference",
    )

    ordered_entries: list[tuple[str, Any]] = []
    emitted_field_names: set[str] = set()

    for field_name in preferred_field_order:
        if field_name not in context:
            continue

        ordered_entries.append((label_by_field_name.get(field_name, field_name.replace("_", " ")), context[field_name]))
        emitted_field_names.add(field_name)

    for field_name in sorted(context):
        if field_name in emitted_field_names:
            continue

        ordered_entries.append((label_by_field_name.get(field_name, field_name.replace("_", " ")), context[field_name]))

    return ordered_entries


def write_command_output(
    output: str,
    *,
    json_payload: dict[str, Any],
    human_output_factory: Callable[[], str],
) -> None:
    output_mode = CliOutputMode(output)
    match output_mode:
        case CliOutputMode.JSON:
            write_contract_json(json_payload)
        case CliOutputMode.HUMAN:
            sys.stdout.write(human_output_factory())


def render_presets_list_human_output(payload: dict[str, Any]) -> str:
    return (
        "preset list completed: "
        f"{payload['preset_count']} presets"
        + (
            f" category={payload['category_filter']}"
            if payload["category_filter"] is not None
            else ""
        )
        + (
            f" scope={payload['scope_filter']}"
            if payload["scope_filter"] is not None
            else ""
        )
        + "\n"
    )


def render_presets_show_human_output(payload: dict[str, Any]) -> str:
    artifact = cast(dict[str, Any], payload["artifact"])
    metadata = cast(dict[str, Any], artifact["metadata"])

    return (
        "preset show completed: "
        f"{artifact['category']}:{artifact['preset_name']} "
        f"scope={payload['requested_scope']} "
        f"version={metadata['version_label']}\n"
    )


def render_presets_export_human_output(payload: dict[str, Any]) -> str:
    return (
        "preset export completed: "
        f"{payload['requested_category']}:{payload['preset_name']} "
        f"scope={payload['requested_scope']}\n"
    )


def render_fit_human_output(payload: dict[str, Any]) -> str:
    model_summary = cast(dict[str, Any], payload["model_summary"])

    lines = [
        (
            "fit completed:"
        ),
        (
            f"run_id={payload['run_id']} "
            f"workflow_scope={model_summary['workflow_scope']} "
            f"market_profile={model_summary['market_profile'] or 'none'} "
            "known_news="
            f"{model_summary['known_news_preset'] or DEFAULT_HIGH_LEVEL_KNOWN_NEWS_PRESET_NAME}"
        ),
        f"model path: {payload['model_output_path']}",
        f"report path: {payload['report_path']}",
        f"run directory: {payload['run_directory']}",
    ]

    if bool(payload.get("explain_requested")):
        lines.extend(render_input_inference_lines(cast(dict[str, Any], payload["input_inference"])))

    return "\n".join(lines) + "\n"


def render_fit_inspect_human_output(payload: dict[str, Any]) -> str:
    lines = [
        (
            "fit inspect completed: "
            f"run_id={payload['run_id']} "
            f"temporary_workspace_materialized={str(payload['temporary_workspace_materialized']).lower()} "
            f"execution_mode={payload['execution_mode']}"
        ),
        f"tick_data: path={payload['tick_data_path']}",
        f"resolved output directory: {payload['output_directory']}",
        *render_input_inference_lines(cast(dict[str, Any], payload["input_inference"])),
    ]

    return "\n".join(lines) + "\n"


def render_input_inference_lines(input_inference: dict[str, Any]) -> list[str]:
    tick_price_columns = cast(dict[str, Any], input_inference["tick_price_columns"])
    known_news = cast(dict[str, Any], input_inference["known_news"])

    return [
        "inferred:",
        f"  tick data format: {input_inference['tick_data_format']}",
        f"  timestamp column: {input_inference['timestamp_column']}",
        (
            "  tick price input policy: "
            f"{input_inference['tick_price_input_policy'] or 'none'}"
        ),
        f"  timestamp timezone: {input_inference['timestamp_timezone']}",
        (
            "  timestamp timezone source: "
            f"{input_inference['timestamp_timezone_source']}"
        ),
        "price columns:",
        f"  price: {tick_price_columns['price_column'] or 'none'}",
        f"  trade sign: {tick_price_columns['trade_sign_column'] or 'none'}",
        f"  bid: {tick_price_columns['bid_column'] or 'none'}",
        f"  ask: {tick_price_columns['ask_column'] or 'none'}",
        "preset defaults:",
        f"  market profile: {input_inference['market_profile'] or 'none'}",
        f"  session source: {input_inference['session_source_kind']}",
        (
            "  known news selected: "
            f"{known_news['selected_preset'] or DEFAULT_HIGH_LEVEL_KNOWN_NEWS_PRESET_NAME}"
        ),
        (
            "  known news recommended: "
            f"{known_news['recommended_preset'] or 'none'}"
        ),
        (
            "  known news recommendation applied: "
            f"{str(known_news['recommendation_applied']).lower()}"
        ),
    ]


def render_simulate_human_output(payload: dict[str, Any]) -> str:
    return "\n".join(
        [
            "simulate completed:",
            f"run_id={payload['run_id']} kind={payload['simulation_kind']}",
            f"primary output: {payload['primary_output_path']}",
            f"report path: {payload['report_path']}",
            f"run directory: {payload['run_directory']}",
        ]
    ) + "\n"


def render_market_data_calibration_human_output(prepared_calibration: object) -> str:
    request = cast(Any, prepared_calibration).request

    selected_stages = ",".join(stage.value for stage in prepared_calibration.diagnostics.selected_stages)
    preprocessing_steps = ",".join(prepared_calibration.diagnostics.executed_preprocessing_steps)

    lines: list[str] = [
        (
            "fit plan prepared: "
            f"run_id={request.run_id} "
            f"workflow_scope={request.workflow_scope.value} "
            f"execution_mode={request.output_preferences.execution_mode.value}"
        ),
        (
            "tick_data: "
            f"path={request.tick_data_source.path} "
            f"format={request.tick_data_source.file_format.value} "
            f"provenance_label={request.tick_data_source.provenance_label}"
        ),
        (
            "session_resolution: "
            f"exchange_code={request.session_configuration.exchange_code} "
            f"mode={prepared_calibration.session_resolution.resolution_kind} "
            f"timezone={prepared_calibration.session_resolution.resolved_timezone_name} "
            f"confidence={prepared_calibration.session_resolution.confidence_label}"
        ),
        (
            "workflow_diagnostics: "
            f"preparation_schema_version={prepared_calibration.schema_version} "
            f"selected_stages={selected_stages} "
            f"preprocessing_steps={preprocessing_steps}"
        ),
        (
            "loaded_data_summary: "
            f"tick_records={prepared_calibration.diagnostics.tick_record_count} "
            f"known_news_events={prepared_calibration.diagnostics.known_news_event_count} "
            f"option_surface_records={prepared_calibration.diagnostics.option_surface_record_count}"
        ),
    ]

    if request.session_configuration.session_definition_source is not None:
        lines.append(
            (
                "session_definition_override: "
                f"path={request.session_configuration.session_definition_source.path} "
                f"format={request.session_configuration.session_definition_source.file_format.value} "
                f"provenance_label={request.session_configuration.session_definition_source.provenance_label}"
            )
        )

    if request.known_news_inputs is not None:
        lines.append(
            (
                "known_news: "
                f"path={request.known_news_inputs.calendar_source.path} "
                f"format={request.known_news_inputs.calendar_source.file_format.value} "
                f"provenance_label={request.known_news_inputs.calendar_source.provenance_label}"
            )
        )

    if request.option_surface_source is not None:
        lines.append(
            (
                "option_surface: "
                f"path={request.option_surface_source.path} "
                f"format={request.option_surface_source.file_format.value} "
                f"provenance_label={request.option_surface_source.provenance_label}"
            )
        )

    lines.append(
        (
            "derived_references: "
            f"trade_events={prepared_calibration.provenance.derived_trade_events_data_reference} "
            f"option_surface={prepared_calibration.provenance.derived_option_surface_data_reference or 'none'}"
        )
    )
    lines.append(
        (
            "normalization_policy: "
            f"timestamp_timezone={request.normalization_policy.timestamp_timezone} "
            f"sort_records_by_timestamp={str(request.normalization_policy.sort_records_by_timestamp).lower()} "
            "duplicate_record_policy="
            f"{optional_enum_value(request.normalization_policy.duplicate_record_policy) or 'preserve'} "
            f"tick_price_input_policy={optional_enum_value(request.normalization_policy.tick_price_input_policy)} "
            f"option_quote_input_policy={optional_enum_value(request.normalization_policy.option_quote_input_policy)}"
        )
    )
    lines.append(
        (
            "artifact_targets: "
            f"disk_checkpoint_directory={request.output_preferences.disk_checkpoint_directory or 'none'} "
            f"emit_input_provenance_report={str(request.output_preferences.emit_input_provenance_report).lower()} "
            "emit_derived_staged_execution_input="
            f"{str(request.output_preferences.emit_derived_staged_execution_input).lower()}"
        )
    )

    return "\n".join(lines) + "\n"


def render_simulation_seeded_ticks_human_output(response_payload: dict[str, Any]) -> str:
    diagnostics = cast(dict[str, Any], response_payload["diagnostics"])
    metadata = cast(dict[str, Any], response_payload["metadata"])

    return (
        "simulate seeded-ticks completed: "
        f"schema_version={response_payload['schema_version']} "
        f"transition_count={diagnostics['transition_count']} "
        f"path_record_schema_version={metadata['path_record_schema_version']}\n"
    )


def render_simulation_coarse_interval_human_output(response_payload: dict[str, Any]) -> str:
    diagnostics = cast(dict[str, Any], response_payload["diagnostics"])
    return (
        "simulate coarse-interval completed: "
        f"schema_version={response_payload['schema_version']} "
        f"simulated_interval_count={response_payload['simulated_interval_count']} "
        f"interval_delta={response_payload['interval_delta']} "
        f"cumulative_total_return={diagnostics['cumulative_total_return']}\n"
    )


def render_price_human_output(run_result_payload: dict[str, Any]) -> str:
    response_payload = cast(dict[str, Any], run_result_payload["response"])
    lines = [
        "price completed:",
        (
            f"run_id={run_result_payload['run_id']} price={response_payload['price']} "
            f"primary_output={run_result_payload['primary_output_path']}"
        ),
    ]

    run_diagnostics = cast(Optional[dict[str, Any]], run_result_payload.get("run_diagnostics"))
    anchored_recursion = (
        cast(Optional[dict[str, Any]], run_diagnostics.get("anchored_recursion"))
        if isinstance(run_diagnostics, dict)
        else None
    )
    if isinstance(anchored_recursion, dict):
        interval_diagnostics = anchored_recursion.get("interval_diagnostics")
        interval_count = len(interval_diagnostics) if isinstance(interval_diagnostics, list) else 0
        lines.append(
            (
                "anchored_diagnostics: "
                f"deviation_grid_node_count={anchored_recursion.get('deviation_grid_node_count')} "
                "refined_deviation_grid_node_count="
                f"{anchored_recursion.get('refined_deviation_grid_node_count')} "
                f"interval_count={interval_count}"
            )
        )

    return "\n".join(lines) + "\n"


def render_seasonality_preprocess_timestamps_human_output(payload: dict[str, Any]) -> str:
    lines: list[str] = [
        (
            "seasonality preprocess-timestamps completed: "
            f"schema_version={payload['schema_version']} "
            f"exchange_code={payload['exchange_code']} "
            f"exchange_timezone_name={payload['exchange_timezone_name']} "
            f"accepted_record_count={payload['accepted_record_count']} "
            f"filtered_record_count={payload['filtered_record_count']}"
        ),
        (
            "coverage_diagnostics: "
            f"hour_bucket_count={payload['hour_bucket_count']} "
            f"missing_hour_bucket_count={payload['missing_hour_bucket_count']} "
            f"sparse_hour_bucket_count={payload['sparse_hour_bucket_count']} "
            f"first_event_timestamp_utc_nanos={payload['first_event_timestamp_utc_nanos']} "
            f"last_event_timestamp_utc_nanos={payload['last_event_timestamp_utc_nanos']}"
        ),
    ]

    return "\n".join(lines) + "\n"


def render_seasonality_diagnostics_human_output(
    report: dict[str, Any],
    *,
    preprocessed_events: object,
    intraday_estimate: object,
    day_of_week_estimate: object,
    day_of_year_estimate: object,
) -> str:
    fit_quality_metrics = cast(dict[str, Any], report["fit_quality_metrics"])
    reproducibility_metadata = cast(dict[str, Any], report["reproducibility_metadata"])
    input_snapshot = cast(dict[str, Any], reproducibility_metadata["input_snapshot"])
    overlay_component_summary = cast(Optional[dict[str, Any]], report["overlay_component_summary"])
    active_overlay_event_count = (
        overlay_component_summary["active_overlay_event_count"] if overlay_component_summary is not None else 0
    )

    lines: list[str] = [
        (
            "seasonality diagnostics completed: "
            f"schema_version={report['schema_version']} "
            f"dataset_id={report['dataset_id']} "
            f"exchange_code={cast(Any, preprocessed_events).exchange_code} "
            f"accepted_record_count={cast(Any, preprocessed_events).accepted_record_count} "
            f"filtered_record_count={cast(Any, preprocessed_events).filtered_record_count}"
        ),
        (
            "fit_quality: "
            f"combined_factor_mean={fit_quality_metrics['combined_factor_mean']} "
            f"normalization_anchor_error={fit_quality_metrics['normalization_anchor_error']} "
            f"missing_hour_bucket_count={cast(Any, preprocessed_events).missing_hour_bucket_count} "
            f"sparse_hour_bucket_count={cast(Any, preprocessed_events).sparse_hour_bucket_count}"
        ),
        (
            "component_sparsity: "
            f"intraday_sparse_bin_count={cast(Any, intraday_estimate).sparse_bin_count} "
            f"day_of_week_sparse_bucket_count={cast(Any, day_of_week_estimate).sparse_bucket_count} "
            f"day_of_year_sparse_bucket_count={cast(Any, day_of_year_estimate).sparse_bucket_count} "
            f"active_overlay_event_count={active_overlay_event_count}"
        ),
        (
            "reproducibility: "
            f"estimator_version={reproducibility_metadata['estimator_version']} "
            f"snapshot_fingerprint_hex={input_snapshot['snapshot_fingerprint_hex']}"
        ),
    ]

    return "\n".join(lines) + "\n"


def render_hourly_factor_materialize_seasonality_human_output(payload: dict[str, Any]) -> str:
    lines: list[str] = [
        (
            "hourly-factors materialize-seasonality completed: "
            f"schema_version={payload['schema_version']} "
            f"exchange_code={payload['exchange_code']} "
            f"interval_count={payload['interval_count']} "
            f"source_point_count={payload['source_point_count']}"
        ),
        (
            "materialization_diagnostics: "
            f"sparse_interval_count={payload['sparse_interval_count']} "
            f"interpolated_interval_count={payload['interpolated_interval_count']} "
            f"fallback_interval_count={payload['fallback_interval_count']} "
            f"interpolation_method={payload['interpolation_method']}"
        ),
    ]

    return "\n".join(lines) + "\n"


def render_hourly_factor_materialize_news_intensity_human_output(payload: dict[str, Any]) -> str:
    lines: list[str] = [
        (
            "hourly-factors materialize-news-intensity completed: "
            f"schema_version={payload['schema_version']} "
            f"exchange_code={payload['exchange_code']} "
            f"interval_count={payload['interval_count']} "
            f"known_scheduled_arrival_count={payload['known_scheduled_arrival_count']}"
        ),
        (
            "materialization_diagnostics: "
            f"combined_component_non_zero_interval_count={payload['combined_component_non_zero_interval_count']} "
            f"surprise_multiplier_override_interval_count={payload['surprise_multiplier_override_interval_count']} "
            f"surprise_baseline_intensity={payload['surprise_baseline_intensity']}"
        ),
    ]

    return "\n".join(lines) + "\n"


def render_hourly_factor_build_contract_human_output(payload: dict[str, Any]) -> str:
    lines: list[str] = [
        (
            "hourly-factors build-contract completed: "
            f"schema_version={payload['schema_version']} "
            f"exchange_code={payload['exchange_code']} "
            f"interval_count={payload['interval_count']}"
        ),
        (
            "contract_metadata: "
            f"seasonality_materialization_method={payload['seasonality_materialization_method']} "
            f"news_intensity_materialization_method={payload['news_intensity_materialization_method']} "
            f"skipped_local_hour_interval_count={payload['skipped_local_hour_interval_count']} "
            f"repeated_local_hour_interval_count={payload['repeated_local_hour_interval_count']}"
        ),
    ]

    return "\n".join(lines) + "\n"


def render_stylized_facts_horizon_comparison_human_output(
    report_bundle: dict[str, Any],
    *,
    execution_mode: str,
) -> str:
    comparison_report = cast(dict[str, Any], report_bundle["comparison_report"])
    summary = cast(dict[str, Any], comparison_report["summary"])
    lines: list[str] = [
        (
            "stylized-facts horizon-comparison completed: "
            f"schema_version={report_bundle['schema_version']} "
            f"execution_mode={execution_mode} "
            f"status={comparison_report['status']} "
            f"metric_catalog_id={comparison_report['metric_catalog_id']} "
            f"run_count={summary['total_run_count']} "
            f"table_row_count={len(cast(list[dict[str, Any]], report_bundle['table_summary']['rows']))}"
        ),
        (
            "decision_counts: "
            f"run_pass={summary['run_decision_counts']['pass_count']} "
            f"run_fail={summary['run_decision_counts']['fail_count']} "
            f"run_insufficient_data={summary['run_decision_counts']['insufficient_data_count']} "
            f"metric_pass={summary['metric_decision_counts']['pass_count']} "
            f"metric_fail={summary['metric_decision_counts']['fail_count']} "
            f"metric_insufficient_data={summary['metric_decision_counts']['insufficient_data_count']}"
        ),
    ]

    artifact_export = cast(Optional[dict[str, Any]], report_bundle["artifact_export"])
    if artifact_export is not None:
        lines.append(
            (
                "artifact_export: "
                f"export_directory={artifact_export['export_directory']} "
                f"file_stem={artifact_export['file_stem']} "
                f"exported_file_count={len(cast(list[dict[str, Any]], artifact_export['exported_files']))}"
            )
        )

    return "\n".join(lines) + "\n"


def render_calibration_staged_human_output(response_payload: dict[str, Any]) -> str:
    status = cast(str, response_payload["status"])
    failure = cast(Optional[dict[str, Any]], response_payload.get("failure"))
    prefix = "fit staged completed" if status == "success" else "fit staged failed"
    line = (
        f"{prefix}: "
        f"run_id={response_payload['run_id']} "
        f"execution_mode={response_payload['execution_mode']} "
        f"status={status} "
        f"stage_report_count={len(cast(list[dict[str, Any]], response_payload['stage_reports']))} "
        f"checkpoint_count={len(cast(list[dict[str, Any]], response_payload['emitted_checkpoints']))}"
    )

    if failure is None:
        return line + "\n"

    failure_kind = cast(Optional[str], failure.get("failure_kind"))
    failed_stage = cast(Optional[str], failure.get("stage"))
    error_message = cast(Optional[str], failure.get("error_message"))

    if failed_stage is not None:
        line += f" failed_stage={failed_stage}"
    if failure_kind is not None:
        line += f" failure_kind={failure_kind}"
    if error_message is not None:
        line += f" error_message={error_message}"

    return line + "\n"


def write_deterministic_regression_human_output(report: dict[str, Any]) -> None:
    summary = cast(dict[str, int], report["summary"])
    if summary["failed_case_count"] == 0:
        sys.stdout.write(
            "deterministic regression passed: "
            f"{summary['passed_case_count']}/{summary['total_case_count']} cases\n",
        )
        return

    sys.stderr.write(
        "deterministic regression failed: "
        f"{summary['failed_case_count']}/{summary['total_case_count']} cases\n",
    )

    for case_report in cast(list[dict[str, Any]], report["case_reports"]):
        if case_report["status"] != "fail":
            continue

        failure = cast(Optional[dict[str, Any]], case_report["failure"])
        failure_kind = failure["failure_kind"] if failure is not None else "unknown"
        sys.stderr.write(
            f"fixture_id={case_report['fixture_id']} "
            f"case_kind={case_report['case_kind']} "
            f"failure_kind={failure_kind} "
            f"summary={case_report['summary']}\n",
        )


def write_open_closed_gap_human_output(report: dict[str, Any]) -> None:
    summary = cast(dict[str, int], report["summary"])
    behavior_diff_report_summary = cast(
        dict[str, int],
        cast(dict[str, Any], report["behavior_diff_report"])["summary"],
    )
    if summary["failed_case_count"] == 0:
        sys.stdout.write(
            "open closed gap leakage regression passed: "
            f"{summary['passed_case_count']}/{summary['total_case_count']} scenarios, "
            f"semantic_mismatches={summary['total_semantic_mismatch_failure_count']}, "
            f"diff_regression_cases={behavior_diff_report_summary['regression_case_count']}\n",
        )
        return

    sys.stderr.write(
        "open closed gap leakage regression failed: "
        f"{summary['failed_case_count']}/{summary['total_case_count']} scenarios, "
        f"semantic_mismatches={summary['total_semantic_mismatch_failure_count']}\n",
    )

    for case_report in cast(list[dict[str, Any]], report["case_reports"]):
        if case_report["status"] != "fail":
            continue

        sys.stderr.write(
            f"scenario_id={case_report['fixture_id']} exchange_code={case_report['exchange_code']} "
            f"semantic_mismatch_failure_count={case_report['semantic_mismatch_failure_count']} "
            f"max_absolute_mismatch={case_report['max_absolute_mismatch']}\n",
        )

        contribution_summary = cast(dict[str, Any], case_report["contribution_summary"])

        sys.stderr.write(
            "  contributions "
            f"activity_expected={contribution_summary['expected_activity_coupling_total']} "
            f"activity_observed={contribution_summary['observed_activity_coupling_total']} "
            f"trading_expected={contribution_summary['expected_trading_return_total']} "
            f"trading_observed={contribution_summary['observed_trading_return_total']} "
            f"open_jump_expected={contribution_summary['expected_open_jump_total']} "
            f"open_jump_observed={contribution_summary['observed_open_jump_total']} "
            f"closed_gap_expected={contribution_summary['expected_closed_gap_total']} "
            f"closed_gap_observed={contribution_summary['observed_closed_gap_total']}\n",
        )

        behavior_diff = cast(dict[str, Any], case_report["behavior_diff"])
        for diff_metric in cast(list[dict[str, Any]], behavior_diff["metrics"]):
            if not cast(bool, diff_metric["regression_detected"]):
                continue

            sys.stderr.write(
                "  diff_regression "
                f"metric_id={diff_metric['metric_id']} "
                f"semantic_bucket={diff_metric['semantic_bucket']} "
                f"expected={diff_metric['expected_value']} "
                f"observed={diff_metric['observed_value']} "
                f"absolute_difference={diff_metric['absolute_difference']} "
                f"absolute_tolerance={diff_metric['absolute_tolerance']}\n",
            )

        for failure in cast(list[dict[str, Any]], case_report["failures"]):
            failure_kind = cast(str, failure["failure_kind"])
            match failure_kind:
                case "semantic_mismatch":
                    sys.stderr.write(
                        f"  failure_kind={failure_kind} event_id={failure['event_id']} "
                        f"event_timestamp_utc={failure['event_timestamp_utc']} "
                        f"semantic_bucket={failure['semantic_bucket']} "
                        f"mismatch_kind={failure['mismatch_kind']} "
                        f"absolute_difference={failure['absolute_difference']}\n",
                    )
                case "dispatch_error":
                    sys.stderr.write(
                        f"  failure_kind={failure_kind} error_message={failure['error_message']}\n",
                    )
                case _:
                    sys.stderr.write(f"  failure_kind={failure_kind}\n")


def optional_enum_value(value: Optional[StrEnum]) -> str:
    return value.value if value is not None else "none"
