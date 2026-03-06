from collections.abc import Mapping, Sequence
from typing import Any, Optional, cast

from .enum import DeterministicRegressionCaseKind, HourlySourceKind, MarketState, RegressionStatus
from .helpers import (
    require_enum_member,
    require_field,
    require_float_list,
    require_mapping,
    require_non_empty_string,
    require_non_empty_string_list,
    require_non_negative_integer,
    require_string_list,
    validate_finite_float,
    validate_finite_non_negative_float,
)
from .types import (
    DeterministicRegressionHarnessReport,
    RegressionCaseReport,
    RegressionHarnessFallbackPolicy,
    RegressionHarnessSummary,
    RegressionHarnessToleranceConfig,
)
from ..seasonality import SeasonalityInputValidationError


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


def normalize_calendar_cases(calendar_cases: Sequence[Mapping[str, Any]]) -> list[dict[str, Any]]:
    normalized_cases: list[dict[str, Any]] = []

    for case_position, calendar_case in enumerate(calendar_cases):
        context: str = f"calendar_cases[{case_position}]"
        fixture_id: str = require_non_empty_string(calendar_case, "fixture_id", context)
        exchange_code: str = require_non_empty_string(calendar_case, "exchange_code", context)
        timestamp_utc: str = require_non_empty_string(calendar_case, "timestamp_utc", context)
        expected_state = require_enum_member(
            calendar_case,
            "expected_state",
            context,
            MarketState,
            error_message=f"{context}.expected_state must be either 'open' or 'closed'",
        )

        normalized_cases.append(
            {
                "fixture_id": fixture_id,
                "exchange_code": exchange_code,
                "timestamp_utc": timestamp_utc,
                "expected_state": expected_state.value,
            },
        )

    return normalized_cases


def normalize_seasonality_cases(
    seasonality_cases: Sequence[Mapping[str, Any]],
) -> list[dict[str, Any]]:
    normalized_cases: list[dict[str, Any]] = []

    for case_position, seasonality_case in enumerate(seasonality_cases):
        context: str = f"seasonality_cases[{case_position}]"
        fixture_id: str = require_non_empty_string(seasonality_case, "fixture_id", context)
        exchange_code: str = require_non_empty_string(seasonality_case, "exchange_code", context)
        range_start_utc: str = require_non_empty_string(seasonality_case, "range_start_utc", context)
        range_end_utc: str = require_non_empty_string(seasonality_case, "range_end_utc", context)

        seasonality_source: Mapping[str, Any] = require_mapping(
            seasonality_case,
            "seasonality_source",
            context,
        )
        news_intensity_source: Mapping[str, Any] = require_mapping(
            seasonality_case,
            "news_intensity_source",
            context,
        )
        expected_s_h: list[float] = require_float_list(seasonality_case, "expected_s_h", context)
        expected_nu_h: list[float] = require_float_list(seasonality_case, "expected_nu_h", context)

        normalized_cases.append(
            {
                "fixture_id": fixture_id,
                "exchange_code": exchange_code,
                "range_start_utc": range_start_utc,
                "range_end_utc": range_end_utc,
                "seasonality_source": validate_seasonality_source(seasonality_source, context),
                "news_intensity_source": validate_news_intensity_source(news_intensity_source, context),
                "expected_s_h": expected_s_h,
                "expected_nu_h": expected_nu_h,
            },
        )

    return normalized_cases


def validate_seasonality_source(
    seasonality_source: Mapping[str, Any],
    case_context: str,
) -> dict[str, Any]:
    context: str = f"{case_context}.seasonality_source"
    source_kind = require_enum_member(
        seasonality_source,
        "source_kind",
        context,
        HourlySourceKind,
        error_message=f"{context}.source_kind must be either 'explicit' or 'materialized'",
    )

    match source_kind:
        case HourlySourceKind.EXPLICIT:
            return {
                "source_kind": source_kind.value,
                "hourly_factors": require_float_list(seasonality_source, "hourly_factors", context),
                "materialization_method": require_non_empty_string(
                    seasonality_source,
                    "materialization_method",
                    context,
                ),
            }
        case HourlySourceKind.MATERIALIZED:
            if "source_timestamp_utc" in seasonality_source:
                raise SeasonalityInputValidationError(
                    f"{context} uses unsupported field 'source_timestamp_utc'; use 'source_timestamps_utc'",
                )

            return {
                "source_kind": source_kind.value,
                "source_timestamps_utc": require_non_empty_string_list(
                    seasonality_source,
                    "source_timestamps_utc",
                    context,
                ),
                "source_seasonality_factors": require_float_list(
                    seasonality_source,
                    "source_seasonality_factors",
                    context,
                ),
                "interpolation_method": require_non_empty_string(
                    seasonality_source,
                    "interpolation_method",
                    context,
                ),
                "aggregation_method": require_non_empty_string(
                    seasonality_source,
                    "aggregation_method",
                    context,
                ),
                "sparse_fallback_policy": require_non_empty_string(
                    seasonality_source,
                    "sparse_fallback_policy",
                    context,
                ),
                "sparse_hour_minimum_observation_count": require_non_negative_integer(
                    seasonality_source,
                    "sparse_hour_minimum_observation_count",
                    context,
                ),
            }


def validate_news_intensity_source(
    news_intensity_source: Mapping[str, Any],
    case_context: str,
) -> dict[str, Any]:
    context: str = f"{case_context}.news_intensity_source"
    source_kind = require_enum_member(
        news_intensity_source,
        "source_kind",
        context,
        HourlySourceKind,
        error_message=f"{context}.source_kind must be either 'explicit' or 'materialized'",
    )

    match source_kind:
        case HourlySourceKind.EXPLICIT:
            return {
                "source_kind": source_kind.value,
                "hourly_factors": require_float_list(news_intensity_source, "hourly_factors", context),
                "materialization_method": require_non_empty_string(
                    news_intensity_source,
                    "materialization_method",
                    context,
                ),
            }
        case HourlySourceKind.MATERIALIZED:
            raw_multiplier_mapping: Mapping[str, Any] = require_mapping(
                news_intensity_source,
                "surprise_deterministic_time_multiplier_by_interval_index",
                context,
            )

            normalized_multipliers: dict[str, float] = {}
            for raw_interval_index, raw_multiplier_value in raw_multiplier_mapping.items():
                if not isinstance(raw_interval_index, str) or not raw_interval_index.isdigit():
                    raise SeasonalityInputValidationError(
                        f"{context}.surprise_deterministic_time_multiplier_by_interval_index keys must be numeric strings",
                    )
                normalized_multipliers[raw_interval_index] = validate_finite_float(
                    raw_multiplier_value,
                    f"{context}.surprise_deterministic_time_multiplier_by_interval_index['{raw_interval_index}']",
                )

            known_scheduled_event_timestamps_utc: list[str] = require_string_list(
                news_intensity_source,
                "known_scheduled_event_timestamps_utc",
                context,
                require_non_empty=False,
            )

            return {
                "source_kind": source_kind.value,
                "known_scheduled_event_timestamps_utc": known_scheduled_event_timestamps_utc,
                "surprise_baseline_intensity": validate_finite_float(
                    require_field(news_intensity_source, "surprise_baseline_intensity", context),
                    f"{context}.surprise_baseline_intensity",
                ),
                "surprise_deterministic_time_multiplier_by_interval_index": normalized_multipliers,
            }


def validate_deterministic_harness_report_payload(raw_payload: object) -> DeterministicRegressionHarnessReport:
    if not isinstance(raw_payload, Mapping):
        raise SeasonalityInputValidationError(
            "deterministic regression harness report must decode to a mapping",
        )

    schema_version: str = require_non_empty_string(
        raw_payload,
        "schema_version",
        "deterministic regression harness report",
    )
    if schema_version != DETERMINISTIC_REGRESSION_HARNESS_REPORT_SCHEMA_VERSION:
        raise SeasonalityInputValidationError(
            f"unsupported deterministic regression harness report schema_version: {schema_version}",
        )

    determinism_policy: str = require_non_empty_string(
        raw_payload,
        "determinism_policy",
        "deterministic regression harness report",
    )
    if determinism_policy != DETERMINISTIC_REGRESSION_HARNESS_DETERMINISM_POLICY:
        raise SeasonalityInputValidationError(
            f"unexpected deterministic regression harness determinism_policy: {determinism_policy}",
        )

    fallback_policy_mapping: Mapping[str, Any] = require_mapping(
        raw_payload,
        "fallback_policy",
        "deterministic regression harness report",
    )
    fallback_policy: RegressionHarnessFallbackPolicy = {
        "schema_version": require_non_empty_string(
            fallback_policy_mapping,
            "schema_version",
            "deterministic regression harness report.fallback_policy",
        ),
        "unknown_date_policy": require_non_empty_string(
            fallback_policy_mapping,
            "unknown_date_policy",
            "deterministic regression harness report.fallback_policy",
        ),
        "unsupported_exchange_calendar_segment_policy": require_non_empty_string(
            fallback_policy_mapping,
            "unsupported_exchange_calendar_segment_policy",
            "deterministic regression harness report.fallback_policy",
        ),
        "incomplete_fixture_coverage_policy": require_non_empty_string(
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

    tolerance_config_mapping: Mapping[str, Any] = require_mapping(
        raw_payload,
        "tolerance_config",
        "deterministic regression harness report",
    )
    tolerance_config: RegressionHarnessToleranceConfig = {
        "seasonality_absolute_tolerance": validate_finite_non_negative_float(
            require_field(
                tolerance_config_mapping,
                "seasonality_absolute_tolerance",
                "deterministic regression harness report.tolerance_config",
            ),
            "tolerance_config.seasonality_absolute_tolerance",
        ),
        "news_intensity_absolute_tolerance": validate_finite_non_negative_float(
            require_field(
                tolerance_config_mapping,
                "news_intensity_absolute_tolerance",
                "deterministic regression harness report.tolerance_config",
            ),
            "tolerance_config.news_intensity_absolute_tolerance",
        ),
    }

    summary_mapping: Mapping[str, Any] = require_mapping(
        raw_payload,
        "summary",
        "deterministic regression harness report",
    )
    summary: RegressionHarnessSummary = {
        "total_case_count": require_non_negative_integer(
            summary_mapping,
            "total_case_count",
            "deterministic regression harness report.summary",
        ),
        "passed_case_count": require_non_negative_integer(
            summary_mapping,
            "passed_case_count",
            "deterministic regression harness report.summary",
        ),
        "failed_case_count": require_non_negative_integer(
            summary_mapping,
            "failed_case_count",
            "deterministic regression harness report.summary",
        ),
    }

    case_reports_raw: object = require_field(
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
        case_kind = require_enum_member(
            raw_case_report,
            "case_kind",
            case_report_context,
            DeterministicRegressionCaseKind,
            error_message=f"{case_report_context}.case_kind has unsupported value: {{value}}",
        )
        status = require_enum_member(
            raw_case_report,
            "status",
            case_report_context,
            RegressionStatus,
            error_message=f"{case_report_context}.status has unsupported value: {{value}}",
        )

        raw_failure: object = require_field(raw_case_report, "failure", case_report_context)
        if raw_failure is not None and not isinstance(raw_failure, Mapping):
            raise SeasonalityInputValidationError(
                f"{case_report_context}.failure must be null or a mapping",
            )

        case_reports.append(
            {
                "fixture_id": require_non_empty_string(raw_case_report, "fixture_id", case_report_context),
                "case_kind": case_kind.value,
                "status": status.value,
                "summary": require_non_empty_string(raw_case_report, "summary", case_report_context),
                "failure": cast(Optional[dict[str, Any]], raw_failure),
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
