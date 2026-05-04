"""Stylized-fact reporting workflows and internal input helpers."""

from __future__ import annotations

import json
from collections.abc import Mapping
from typing import Any, Optional, cast

import orjson

from .._native_api import native_module
from ..contracts.base import (
    _hydrate_contract_model,
    require_non_empty_string as _require_non_empty_string_base,
)
from ..seasonality import SeasonalityInputValidationError
from .contracts import StylizedFactHorizonComparisonReportBundle

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
