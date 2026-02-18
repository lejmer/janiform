import json
import math
from collections.abc import Mapping, Sequence
from typing import TypedDict, cast

from .api import native_module
from .calendar import CalendarRegistry
from .seasonality import SeasonalityInputValidationError

DETERMINISTIC_REGRESSION_HARNESS_REPORT_SCHEMA_VERSION: str = (
    "validation.deterministic_regression_harness.report.v1"
)
DETERMINISTIC_REGRESSION_HARNESS_DETERMINISM_POLICY: str = (
    "stable_input_order_and_first_mismatch_reporting.v1"
)
VALIDATION_FALLBACK_POLICY_SCHEMA_VERSION: str = "validation.fallback_policy.v1"
VALIDATION_UNKNOWN_DATE_FALLBACK_POLICY: str = (
    "resolve_with_regular_sessions_when_no_date_override.v1"
)
VALIDATION_UNSUPPORTED_CALENDAR_SEGMENT_POLICY: str = (
    "fail_case_with_explicit_calendar_resolution_error.v1"
)
VALIDATION_INCOMPLETE_FIXTURE_COVERAGE_POLICY: str = (
    "fail_fast_on_fixture_contract_validation.v1"
)


class RegressionHarnessToleranceConfig(TypedDict):
    seasonality_absolute_tolerance: float
    news_intensity_absolute_tolerance: float


class RegressionHarnessSummary(TypedDict):
    total_case_count: int
    passed_case_count: int
    failed_case_count: int


class RegressionHarnessFallbackPolicy(TypedDict):
    schema_version: str
    unknown_date_policy: str
    unsupported_exchange_calendar_segment_policy: str
    incomplete_fixture_coverage_policy: str


class RegressionCaseReport(TypedDict):
    fixture_id: str
    case_kind: str
    status: str
    summary: str
    failure: dict[str, object] | None


class DeterministicRegressionHarnessReport(TypedDict):
    schema_version: str
    determinism_policy: str
    fallback_policy: RegressionHarnessFallbackPolicy
    tolerance_config: RegressionHarnessToleranceConfig
    summary: RegressionHarnessSummary
    case_reports: list[RegressionCaseReport]


def run_deterministic_regression_harness(
    *,
    calendar_registry: CalendarRegistry,
    calendar_cases: Sequence[Mapping[str, object]],
    seasonality_cases: Sequence[Mapping[str, object]],
    seasonality_absolute_tolerance: float = 1e-12,
    news_intensity_absolute_tolerance: float = 1e-12,
) -> DeterministicRegressionHarnessReport:
    _validate_finite_non_negative_float(seasonality_absolute_tolerance, "seasonality_absolute_tolerance")
    _validate_finite_non_negative_float(news_intensity_absolute_tolerance, "news_intensity_absolute_tolerance")

    normalized_calendar_cases: list[dict[str, object]] = _validate_calendar_cases(calendar_cases)
    normalized_seasonality_cases: list[dict[str, object]] = _validate_seasonality_cases(seasonality_cases)

    native = native_module()
    report_json: str = native.run_deterministic_regression_harness(
        calendar_registry._native_registry,
        normalized_calendar_cases,
        normalized_seasonality_cases,
        seasonality_absolute_tolerance=seasonality_absolute_tolerance,
        news_intensity_absolute_tolerance=news_intensity_absolute_tolerance,
    )

    try:
        parsed_report: object = json.loads(report_json)
    except json.JSONDecodeError as e:
        raise SeasonalityInputValidationError(
            f"deterministic regression harness report must be valid JSON: {e.msg}",
        ) from e

    return _validate_harness_report_payload(parsed_report)


def _validate_calendar_cases(calendar_cases: Sequence[Mapping[str, object]]) -> list[dict[str, object]]:
    normalized_cases: list[dict[str, object]] = []

    for case_position, calendar_case in enumerate(calendar_cases):
        context: str = f"calendar_cases[{case_position}]"
        fixture_id: str = _require_non_empty_string(calendar_case, "fixture_id", context)
        exchange_code: str = _require_non_empty_string(calendar_case, "exchange_code", context)
        timestamp_utc: str = _require_non_empty_string(calendar_case, "timestamp_utc", context)
        expected_state: str = _require_non_empty_string(calendar_case, "expected_state", context)
        if expected_state not in {"open", "closed"}:
            raise SeasonalityInputValidationError(
                f"{context}.expected_state must be either 'open' or 'closed'",
            )

        normalized_cases.append(
            {
                "fixture_id": fixture_id,
                "exchange_code": exchange_code,
                "timestamp_utc": timestamp_utc,
                "expected_state": expected_state,
            },
        )

    return normalized_cases


def _validate_seasonality_cases(
    seasonality_cases: Sequence[Mapping[str, object]],
) -> list[dict[str, object]]:
    normalized_cases: list[dict[str, object]] = []

    for case_position, seasonality_case in enumerate(seasonality_cases):
        context: str = f"seasonality_cases[{case_position}]"
        fixture_id: str = _require_non_empty_string(seasonality_case, "fixture_id", context)
        exchange_code: str = _require_non_empty_string(seasonality_case, "exchange_code", context)
        range_start_utc: str = _require_non_empty_string(seasonality_case, "range_start_utc", context)
        range_end_utc: str = _require_non_empty_string(seasonality_case, "range_end_utc", context)

        seasonality_source: Mapping[str, object] = _require_mapping(
            seasonality_case,
            "seasonality_source",
            context,
        )
        news_intensity_source: Mapping[str, object] = _require_mapping(
            seasonality_case,
            "news_intensity_source",
            context,
        )
        expected_s_h: list[float] = _require_float_list(seasonality_case, "expected_s_h", context)
        expected_nu_h: list[float] = _require_float_list(seasonality_case, "expected_nu_h", context)

        normalized_cases.append(
            {
                "fixture_id": fixture_id,
                "exchange_code": exchange_code,
                "range_start_utc": range_start_utc,
                "range_end_utc": range_end_utc,
                "seasonality_source": _validate_seasonality_source(seasonality_source, context),
                "news_intensity_source": _validate_news_intensity_source(news_intensity_source, context),
                "expected_s_h": expected_s_h,
                "expected_nu_h": expected_nu_h,
            },
        )

    return normalized_cases


def _validate_seasonality_source(
    seasonality_source: Mapping[str, object],
    case_context: str,
) -> dict[str, object]:
    context: str = f"{case_context}.seasonality_source"
    source_kind: str = _require_non_empty_string(seasonality_source, "source_kind", context)

    match source_kind:
        case "explicit":
            return {
                "source_kind": source_kind,
                "hourly_factors": _require_float_list(seasonality_source, "hourly_factors", context),
                "materialization_method": _require_non_empty_string(
                    seasonality_source,
                    "materialization_method",
                    context,
                ),
            }
        case "materialized":
            if "source_timestamp_utc" in seasonality_source:
                raise SeasonalityInputValidationError(
                    f"{context} uses unsupported field 'source_timestamp_utc'; use 'source_timestamps_utc'",
                )

            return {
                "source_kind": source_kind,
                "source_timestamps_utc": _require_non_empty_string_list(
                    seasonality_source,
                    "source_timestamps_utc",
                    context,
                ),
                "source_seasonality_factors": _require_float_list(
                    seasonality_source,
                    "source_seasonality_factors",
                    context,
                ),
                "interpolation_method": _require_non_empty_string(
                    seasonality_source,
                    "interpolation_method",
                    context,
                ),
                "aggregation_method": _require_non_empty_string(
                    seasonality_source,
                    "aggregation_method",
                    context,
                ),
                "sparse_fallback_policy": _require_non_empty_string(
                    seasonality_source,
                    "sparse_fallback_policy",
                    context,
                ),
                "sparse_hour_minimum_observation_count": _require_non_negative_integer(
                    seasonality_source,
                    "sparse_hour_minimum_observation_count",
                    context,
                ),
            }
        case _:
            raise SeasonalityInputValidationError(
                f"{context}.source_kind must be either 'explicit' or 'materialized'",
            )


def _validate_news_intensity_source(
    news_intensity_source: Mapping[str, object],
    case_context: str,
) -> dict[str, object]:
    context: str = f"{case_context}.news_intensity_source"
    source_kind: str = _require_non_empty_string(news_intensity_source, "source_kind", context)

    match source_kind:
        case "explicit":
            return {
                "source_kind": source_kind,
                "hourly_factors": _require_float_list(news_intensity_source, "hourly_factors", context),
                "materialization_method": _require_non_empty_string(
                    news_intensity_source,
                    "materialization_method",
                    context,
                ),
            }
        case "materialized":
            raw_override_mapping: Mapping[str, object] = _require_mapping(
                news_intensity_source,
                "open_window_override_by_interval_index",
                context,
            )

            normalized_overrides: dict[str, float] = {}
            for raw_interval_index, raw_override_value in raw_override_mapping.items():
                if not isinstance(raw_interval_index, str) or not raw_interval_index.isdigit():
                    raise SeasonalityInputValidationError(
                        f"{context}.open_window_override_by_interval_index keys must be numeric strings",
                    )
                normalized_overrides[raw_interval_index] = _validate_finite_float(
                    raw_override_value,
                    f"{context}.open_window_override_by_interval_index['{raw_interval_index}']",
                )

            return {
                "source_kind": source_kind,
                "closed_window_baseline_intensity": _validate_finite_float(
                    _require_field(news_intensity_source, "closed_window_baseline_intensity", context),
                    f"{context}.closed_window_baseline_intensity",
                ),
                "open_window_override_by_interval_index": normalized_overrides,
            }
        case _:
            raise SeasonalityInputValidationError(
                f"{context}.source_kind must be either 'explicit' or 'materialized'",
            )


def _validate_harness_report_payload(raw_payload: object) -> DeterministicRegressionHarnessReport:
    if not isinstance(raw_payload, Mapping):
        raise SeasonalityInputValidationError(
            "deterministic regression harness report must decode to a mapping",
        )

    schema_version: str = _require_non_empty_string(
        raw_payload,
        "schema_version",
        "deterministic regression harness report",
    )
    if schema_version != DETERMINISTIC_REGRESSION_HARNESS_REPORT_SCHEMA_VERSION:
        raise SeasonalityInputValidationError(
            f"unsupported deterministic regression harness report schema_version: {schema_version}",
        )

    determinism_policy: str = _require_non_empty_string(
        raw_payload,
        "determinism_policy",
        "deterministic regression harness report",
    )
    if determinism_policy != DETERMINISTIC_REGRESSION_HARNESS_DETERMINISM_POLICY:
        raise SeasonalityInputValidationError(
            f"unexpected deterministic regression harness determinism_policy: {determinism_policy}",
        )

    fallback_policy_mapping: Mapping[str, object] = _require_mapping(
        raw_payload,
        "fallback_policy",
        "deterministic regression harness report",
    )
    fallback_policy: RegressionHarnessFallbackPolicy = {
        "schema_version": _require_non_empty_string(
            fallback_policy_mapping,
            "schema_version",
            "deterministic regression harness report.fallback_policy",
        ),
        "unknown_date_policy": _require_non_empty_string(
            fallback_policy_mapping,
            "unknown_date_policy",
            "deterministic regression harness report.fallback_policy",
        ),
        "unsupported_exchange_calendar_segment_policy": _require_non_empty_string(
            fallback_policy_mapping,
            "unsupported_exchange_calendar_segment_policy",
            "deterministic regression harness report.fallback_policy",
        ),
        "incomplete_fixture_coverage_policy": _require_non_empty_string(
            fallback_policy_mapping,
            "incomplete_fixture_coverage_policy",
            "deterministic regression harness report.fallback_policy",
        ),
    }

    if fallback_policy["schema_version"] != VALIDATION_FALLBACK_POLICY_SCHEMA_VERSION:
        raise SeasonalityInputValidationError(
            "unexpected deterministic regression harness fallback_policy.schema_version: "
            f"{fallback_policy['schema_version']}",
        )

    if fallback_policy["unknown_date_policy"] != VALIDATION_UNKNOWN_DATE_FALLBACK_POLICY:
        raise SeasonalityInputValidationError(
            "unexpected deterministic regression harness fallback_policy.unknown_date_policy: "
            f"{fallback_policy['unknown_date_policy']}",
        )

    if (
        fallback_policy["unsupported_exchange_calendar_segment_policy"]
        != VALIDATION_UNSUPPORTED_CALENDAR_SEGMENT_POLICY
    ):
        raise SeasonalityInputValidationError(
            "unexpected deterministic regression harness "
            "fallback_policy.unsupported_exchange_calendar_segment_policy: "
            f"{fallback_policy['unsupported_exchange_calendar_segment_policy']}",
        )

    if (
        fallback_policy["incomplete_fixture_coverage_policy"]
        != VALIDATION_INCOMPLETE_FIXTURE_COVERAGE_POLICY
    ):
        raise SeasonalityInputValidationError(
            "unexpected deterministic regression harness "
            "fallback_policy.incomplete_fixture_coverage_policy: "
            f"{fallback_policy['incomplete_fixture_coverage_policy']}",
        )

    tolerance_config_mapping: Mapping[str, object] = _require_mapping(
        raw_payload,
        "tolerance_config",
        "deterministic regression harness report",
    )
    tolerance_config: RegressionHarnessToleranceConfig = {
        "seasonality_absolute_tolerance": _validate_finite_non_negative_float(
            _require_field(
                tolerance_config_mapping,
                "seasonality_absolute_tolerance",
                "deterministic regression harness report.tolerance_config",
            ),
            "tolerance_config.seasonality_absolute_tolerance",
        ),
        "news_intensity_absolute_tolerance": _validate_finite_non_negative_float(
            _require_field(
                tolerance_config_mapping,
                "news_intensity_absolute_tolerance",
                "deterministic regression harness report.tolerance_config",
            ),
            "tolerance_config.news_intensity_absolute_tolerance",
        ),
    }

    summary_mapping: Mapping[str, object] = _require_mapping(
        raw_payload,
        "summary",
        "deterministic regression harness report",
    )
    summary: RegressionHarnessSummary = {
        "total_case_count": _require_non_negative_integer(
            summary_mapping,
            "total_case_count",
            "deterministic regression harness report.summary",
        ),
        "passed_case_count": _require_non_negative_integer(
            summary_mapping,
            "passed_case_count",
            "deterministic regression harness report.summary",
        ),
        "failed_case_count": _require_non_negative_integer(
            summary_mapping,
            "failed_case_count",
            "deterministic regression harness report.summary",
        ),
    }

    case_reports_raw: object = _require_field(
        raw_payload,
        "case_reports",
        "deterministic regression harness report",
    )
    if not isinstance(case_reports_raw, list):
        raise SeasonalityInputValidationError(
            "deterministic regression harness report.case_reports must be a list",
        )

    case_reports: list[RegressionCaseReport] = []
    for case_position, raw_case_report in enumerate(case_reports_raw):
        if not isinstance(raw_case_report, Mapping):
            raise SeasonalityInputValidationError(
                f"deterministic regression harness report.case_reports[{case_position}] must be a mapping",
            )

        case_report_context: str = (
            f"deterministic regression harness report.case_reports[{case_position}]"
        )
        case_kind: str = _require_non_empty_string(raw_case_report, "case_kind", case_report_context)
        if case_kind not in {"calendar_market_state", "hourly_factor_output"}:
            raise SeasonalityInputValidationError(
                f"{case_report_context}.case_kind has unsupported value: {case_kind}",
            )

        status: str = _require_non_empty_string(raw_case_report, "status", case_report_context)
        if status not in {"pass", "fail"}:
            raise SeasonalityInputValidationError(
                f"{case_report_context}.status has unsupported value: {status}",
            )

        raw_failure: object = _require_field(raw_case_report, "failure", case_report_context)
        if raw_failure is not None and not isinstance(raw_failure, Mapping):
            raise SeasonalityInputValidationError(
                f"{case_report_context}.failure must be null or a mapping",
            )

        case_reports.append(
            {
                "fixture_id": _require_non_empty_string(raw_case_report, "fixture_id", case_report_context),
                "case_kind": case_kind,
                "status": status,
                "summary": _require_non_empty_string(raw_case_report, "summary", case_report_context),
                "failure": cast(dict[str, object] | None, raw_failure),
            },
        )

    return {
        "schema_version": schema_version,
        "determinism_policy": determinism_policy,
        "fallback_policy": fallback_policy,
        "tolerance_config": tolerance_config,
        "summary": summary,
        "case_reports": case_reports,
    }


def _require_mapping(parent: Mapping[str, object], field_name: str, context: str) -> Mapping[str, object]:
    field_value: object = _require_field(parent, field_name, context)
    if not isinstance(field_value, Mapping):
        raise SeasonalityInputValidationError(
            f"{context}.{field_name} must be a mapping",
        )

    return cast(Mapping[str, object], field_value)


def _require_non_empty_string(parent: Mapping[str, object], field_name: str, context: str) -> str:
    field_value: object = _require_field(parent, field_name, context)
    if not isinstance(field_value, str):
        raise SeasonalityInputValidationError(
            f"{context}.{field_name} must be a string",
        )

    normalized_value: str = field_value.strip()
    if not normalized_value:
        raise SeasonalityInputValidationError(
            f"{context}.{field_name} must be non-empty",
        )

    return normalized_value


def _require_non_empty_string_list(
    parent: Mapping[str, object],
    field_name: str,
    context: str,
) -> list[str]:
    field_value: object = _require_field(parent, field_name, context)
    if not isinstance(field_value, Sequence) or isinstance(field_value, str | bytes):
        raise SeasonalityInputValidationError(
            f"{context}.{field_name} must be a sequence of strings",
        )

    normalized_values: list[str] = []
    for value_index, raw_value in enumerate(field_value):
        if not isinstance(raw_value, str):
            raise SeasonalityInputValidationError(
                f"{context}.{field_name}[{value_index}] must be a string",
            )

        normalized_value: str = raw_value.strip()
        if not normalized_value:
            raise SeasonalityInputValidationError(
                f"{context}.{field_name}[{value_index}] must be non-empty",
            )

        normalized_values.append(normalized_value)

    if not normalized_values:
        raise SeasonalityInputValidationError(
            f"{context}.{field_name} must include at least one value",
        )

    return normalized_values


def _require_float_list(parent: Mapping[str, object], field_name: str, context: str) -> list[float]:
    field_value: object = _require_field(parent, field_name, context)
    if not isinstance(field_value, Sequence) or isinstance(field_value, str | bytes):
        raise SeasonalityInputValidationError(
            f"{context}.{field_name} must be a sequence of finite numbers",
        )

    normalized_values: list[float] = []
    for value_index, raw_value in enumerate(field_value):
        normalized_values.append(
            _validate_finite_float(raw_value, f"{context}.{field_name}[{value_index}]"),
        )

    if not normalized_values:
        raise SeasonalityInputValidationError(
            f"{context}.{field_name} must include at least one value",
        )

    return normalized_values


def _require_non_negative_integer(parent: Mapping[str, object], field_name: str, context: str) -> int:
    field_value: object = _require_field(parent, field_name, context)
    if not isinstance(field_value, int) or isinstance(field_value, bool):
        raise SeasonalityInputValidationError(
            f"{context}.{field_name} must be an integer",
        )

    if field_value < 0:
        raise SeasonalityInputValidationError(
            f"{context}.{field_name} must be non-negative",
        )

    return field_value


def _require_field(parent: Mapping[str, object], field_name: str, context: str) -> object:
    if field_name not in parent:
        raise SeasonalityInputValidationError(
            f"{context}.{field_name} is required",
        )

    return parent[field_name]


def _validate_finite_non_negative_float(raw_value: object, context: str) -> float:
    validated_value: float = _validate_finite_float(raw_value, context)
    if validated_value < 0.0:
        raise SeasonalityInputValidationError(f"{context} must be non-negative")

    return validated_value


def _validate_finite_float(raw_value: object, context: str) -> float:
    if not isinstance(raw_value, int | float) or isinstance(raw_value, bool):
        raise SeasonalityInputValidationError(f"{context} must be a finite number")

    validated_value: float = float(raw_value)
    if not math.isfinite(validated_value):
        raise SeasonalityInputValidationError(f"{context} must be a finite number")

    return validated_value


__all__ = [
    "DETERMINISTIC_REGRESSION_HARNESS_DETERMINISM_POLICY",
    "DETERMINISTIC_REGRESSION_HARNESS_REPORT_SCHEMA_VERSION",
    "DeterministicRegressionHarnessReport",
    "RegressionCaseReport",
    "RegressionHarnessFallbackPolicy",
    "RegressionHarnessSummary",
    "RegressionHarnessToleranceConfig",
    "VALIDATION_FALLBACK_POLICY_SCHEMA_VERSION",
    "VALIDATION_INCOMPLETE_FIXTURE_COVERAGE_POLICY",
    "VALIDATION_UNKNOWN_DATE_FALLBACK_POLICY",
    "VALIDATION_UNSUPPORTED_CALENDAR_SEGMENT_POLICY",
    "run_deterministic_regression_harness",
]
