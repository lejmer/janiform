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
OPEN_CLOSED_GAP_LEAKAGE_REGRESSION_HARNESS_REPORT_SCHEMA_VERSION: str = (
    "validation.open_closed_gap_leakage_regression_harness.report.v1"
)
OPEN_CLOSED_GAP_LEAKAGE_REGRESSION_HARNESS_DETERMINISM_POLICY: str = (
    "stable_input_order_and_event_timestamp_then_generation_order.v1"
)
OPEN_CLOSED_GAP_LEAKAGE_REGRESSION_OUTPUT_SCHEMA_METADATA_VERSION: str = (
    "validation.open_closed_gap_leakage_regression_harness.output_schema_metadata.v1"
)
OPEN_CLOSED_GAP_BEHAVIOR_SUMMARY_ARTIFACT_SCHEMA_VERSION: str = (
    "validation.open_closed_gap_behavior.summary_artifact.v1"
)
OPEN_CLOSED_GAP_BEHAVIOR_DIFF_REPORT_SCHEMA_VERSION: str = (
    "validation.open_closed_gap_behavior.diff_report.v1"
)
OPEN_CLOSED_GAP_BEHAVIOR_BASELINE_EXPECTED_OUTPUT_POLICY: str = (
    "market_state_gated_expected_contribution_baseline.v1"
)
OPEN_CLOSED_GAP_BEHAVIOR_SUMMARY_DETERMINISM_POLICY: str = (
    "preserve_input_case_order.v1"
)
OPEN_CLOSED_GAP_BEHAVIOR_DIFF_DETERMINISM_POLICY: str = (
    "preserve_input_case_order_and_metric_order.v1"
)
OPEN_CLOSED_GAP_BEHAVIOR_ENVIRONMENT_METADATA_POLICY: str = (
    "exclude_environment_specific_metadata.v1"
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


class OpenClosedGapLeakageRegressionHarnessToleranceConfig(TypedDict):
    direct_gap_absolute_tolerance: float


class OpenClosedGapLeakageRegressionFailure(TypedDict):
    failure_kind: str
    event_id: str | None
    event_timestamp_utc: str | None
    observed_direct_gap_increment: float | None
    absolute_leakage_magnitude: float | None
    error_message: str | None


class OpenClosedGapLeakageRegressionOutputSchemaMetadata(TypedDict):
    schema_version: str
    summary_artifact_schema_version: str
    behavior_diff_report_schema_version: str
    baseline_expected_output_policy: str
    environment_metadata_policy: str
    required_case_report_fields: list[str]
    required_summary_artifact_fields: list[str]
    required_behavior_diff_report_fields: list[str]
    required_behavior_diff_metric_fields: list[str]


class OpenClosedGapBehaviorContributionSummary(TypedDict):
    open_window_event_count: int
    closed_window_event_count: int
    expected_activity_coupling_j_t_increment_total: float
    observed_activity_coupling_j_t_increment_total: float
    expected_open_window_direct_gap_g_t_increment_total: float
    observed_open_window_direct_gap_g_t_increment_total: float
    expected_closed_window_direct_gap_g_t_increment_total: float
    observed_closed_window_direct_gap_g_t_increment_total: float
    expected_total_direct_gap_g_t_increment: float
    observed_total_direct_gap_g_t_increment: float


class OpenClosedGapBehaviorDiffMetric(TypedDict):
    metric_id: str
    expected_value: float
    observed_value: float
    absolute_difference: float
    absolute_tolerance: float
    regression_detected: bool


class OpenClosedGapBehaviorScenarioDiff(TypedDict):
    fixture_id: str
    exchange_code: str
    regression_metric_count: int
    max_absolute_difference: float
    metrics: list[OpenClosedGapBehaviorDiffMetric]


class OpenClosedGapBehaviorScenarioSummaryArtifact(TypedDict):
    fixture_id: str
    exchange_code: str
    contributions: OpenClosedGapBehaviorContributionSummary


class OpenClosedGapBehaviorSummaryArtifact(TypedDict):
    schema_version: str
    determinism_policy: str
    total_case_count: int
    total_open_window_event_count: int
    total_closed_window_event_count: int
    scenarios: list[OpenClosedGapBehaviorScenarioSummaryArtifact]


class OpenClosedGapBehaviorDiffReportSummary(TypedDict):
    total_case_count: int
    regression_case_count: int
    total_regression_metric_count: int


class OpenClosedGapBehaviorDiffReport(TypedDict):
    schema_version: str
    determinism_policy: str
    baseline_expected_output_policy: str
    summary: OpenClosedGapBehaviorDiffReportSummary
    scenario_diffs: list[OpenClosedGapBehaviorScenarioDiff]


class OpenClosedGapLeakageRegressionCaseReport(TypedDict):
    fixture_id: str
    exchange_code: str
    status: str
    summary: str
    open_window_event_count: int
    leakage_failure_count: int
    max_absolute_leakage_magnitude: float
    contribution_summary: OpenClosedGapBehaviorContributionSummary
    behavior_diff: OpenClosedGapBehaviorScenarioDiff
    failures: list[OpenClosedGapLeakageRegressionFailure]


class OpenClosedGapLeakageRegressionHarnessSummary(TypedDict):
    total_case_count: int
    passed_case_count: int
    failed_case_count: int
    total_open_window_event_count: int
    total_leakage_failure_count: int


class OpenClosedGapLeakageRegressionHarnessReport(TypedDict):
    schema_version: str
    determinism_policy: str
    output_schema_metadata: OpenClosedGapLeakageRegressionOutputSchemaMetadata
    tolerance_config: OpenClosedGapLeakageRegressionHarnessToleranceConfig
    summary: OpenClosedGapLeakageRegressionHarnessSummary
    summary_artifact: OpenClosedGapBehaviorSummaryArtifact
    behavior_diff_report: OpenClosedGapBehaviorDiffReport
    case_reports: list[OpenClosedGapLeakageRegressionCaseReport]


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


def run_open_closed_gap_leakage_regression_harness(
    *,
    calendar_registry: CalendarRegistry,
    gap_behavior_cases: Sequence[Mapping[str, object]],
    direct_gap_absolute_tolerance: float = 1e-12,
) -> OpenClosedGapLeakageRegressionHarnessReport:
    _validate_finite_non_negative_float(direct_gap_absolute_tolerance, "direct_gap_absolute_tolerance")

    normalized_gap_behavior_cases: list[dict[str, object]] = _validate_gap_behavior_cases(gap_behavior_cases)
    native = native_module()
    report_json: str = native.run_open_closed_gap_leakage_regression_harness(
        calendar_registry._native_registry,
        normalized_gap_behavior_cases,
        direct_gap_absolute_tolerance=direct_gap_absolute_tolerance,
    )

    try:
        parsed_report: object = json.loads(report_json)
    except json.JSONDecodeError as e:
        raise SeasonalityInputValidationError(
            f"open closed gap leakage regression harness report must be valid JSON: {e.msg}",
        ) from e

    return _validate_open_closed_gap_leakage_report_payload(parsed_report)


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


def _validate_gap_behavior_cases(
    gap_behavior_cases: Sequence[Mapping[str, object]],
) -> list[dict[str, object]]:
    normalized_cases: list[dict[str, object]] = []

    for case_position, gap_behavior_case in enumerate(gap_behavior_cases):
        context: str = f"gap_behavior_cases[{case_position}]"
        fixture_id: str = _require_non_empty_string(gap_behavior_case, "fixture_id", context)
        exchange_code: str = _require_non_empty_string(gap_behavior_case, "exchange_code", context)

        initial_j_t: float = _validate_finite_float(
            _require_field(gap_behavior_case, "initial_j_t", context),
            f"{context}.initial_j_t",
        )
        initial_g_t: float = _validate_finite_float(
            _require_field(gap_behavior_case, "initial_g_t", context),
            f"{context}.initial_g_t",
        )
        injected_events: Sequence[object] = cast(
            Sequence[object],
            _require_field(gap_behavior_case, "injected_events", context),
        )
        if not isinstance(injected_events, Sequence) or isinstance(injected_events, str | bytes):
            raise SeasonalityInputValidationError(f"{context}.injected_events must be a sequence")

        normalized_injected_events: list[dict[str, object]] = []
        for event_position, raw_event in enumerate(injected_events):
            event_context: str = f"{context}.injected_events[{event_position}]"
            if not isinstance(raw_event, Mapping):
                raise SeasonalityInputValidationError(f"{event_context} must be a mapping")

            normalized_event: dict[str, object] = {
                "event_id": _require_non_empty_string(raw_event, "event_id", event_context),
                "timestamp_utc": _require_non_empty_string(raw_event, "timestamp_utc", event_context),
                "shared_severity_mark": _validate_finite_non_negative_float(
                    _require_field(raw_event, "shared_severity_mark", event_context),
                    f"{event_context}.shared_severity_mark",
                ),
                "coupled_gap_jump_mark": _validate_finite_float(
                    _require_field(raw_event, "coupled_gap_jump_mark", event_context),
                    f"{event_context}.coupled_gap_jump_mark",
                ),
            }

            if "observed_direct_gap_increment_override" in raw_event:
                raw_override: object = _require_field(
                    raw_event,
                    "observed_direct_gap_increment_override",
                    event_context,
                )
                if raw_override is not None:
                    normalized_event["observed_direct_gap_increment_override"] = _validate_finite_float(
                        raw_override,
                        f"{event_context}.observed_direct_gap_increment_override",
                    )

            normalized_injected_events.append(normalized_event)

        normalized_cases.append(
            {
                "fixture_id": fixture_id,
                "exchange_code": exchange_code,
                "initial_j_t": initial_j_t,
                "initial_g_t": initial_g_t,
                "injected_events": normalized_injected_events,
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


def _validate_open_closed_gap_leakage_report_payload(
    raw_payload: object,
) -> OpenClosedGapLeakageRegressionHarnessReport:
    report_context = "open closed gap leakage regression harness report"
    if not isinstance(raw_payload, Mapping):
        raise SeasonalityInputValidationError(
            f"{report_context} must decode to a mapping",
        )

    schema_version: str = _require_non_empty_string(
        raw_payload,
        "schema_version",
        report_context,
    )
    if schema_version != OPEN_CLOSED_GAP_LEAKAGE_REGRESSION_HARNESS_REPORT_SCHEMA_VERSION:
        raise SeasonalityInputValidationError(
            "unsupported open closed gap leakage regression harness report schema_version: "
            f"{schema_version}",
        )

    determinism_policy: str = _require_non_empty_string(
        raw_payload,
        "determinism_policy",
        report_context,
    )
    if determinism_policy != OPEN_CLOSED_GAP_LEAKAGE_REGRESSION_HARNESS_DETERMINISM_POLICY:
        raise SeasonalityInputValidationError(
            "unexpected open closed gap leakage regression harness determinism_policy: "
            f"{determinism_policy}",
        )

    output_schema_metadata = _validate_open_closed_gap_output_schema_metadata(raw_payload, report_context)
    tolerance_config = _validate_open_closed_gap_tolerance_config(raw_payload, report_context)
    summary = _validate_open_closed_gap_harness_summary(raw_payload, report_context)
    summary_artifact = _validate_open_closed_gap_summary_artifact(raw_payload, report_context)
    behavior_diff_report = _validate_open_closed_gap_behavior_diff_report(raw_payload, report_context)
    case_reports = _validate_open_closed_gap_case_reports(raw_payload, report_context)

    return {
        "schema_version": schema_version,
        "determinism_policy": determinism_policy,
        "output_schema_metadata": output_schema_metadata,
        "tolerance_config": tolerance_config,
        "summary": summary,
        "summary_artifact": summary_artifact,
        "behavior_diff_report": behavior_diff_report,
        "case_reports": case_reports,
    }


def _extract_open_closed_gap_direct_tolerance(raw_payload: Mapping[str, object]) -> float:
    tolerance_config = raw_payload.get("tolerance_config")
    if not isinstance(tolerance_config, Mapping):
        return 1e-12

    raw_tolerance = tolerance_config.get("direct_gap_absolute_tolerance")
    if not isinstance(raw_tolerance, int | float) or isinstance(raw_tolerance, bool):
        return 1e-12

    tolerance = float(raw_tolerance)
    if not math.isfinite(tolerance) or tolerance < 0.0:
        return 1e-12

    return tolerance


def _synthesize_legacy_contribution_summary(
    raw_case_report: Mapping[str, object],
) -> dict[str, object]:
    fixture_id = str(raw_case_report.get("fixture_id", "")).strip()
    exchange_code = str(raw_case_report.get("exchange_code", "")).strip()
    open_window_event_count = _optional_non_negative_int(raw_case_report.get("open_window_event_count"))
    observed_open_window_direct_gap_total = _legacy_observed_open_window_direct_gap_total(
        raw_case_report.get("failures"),
    )

    contributions: dict[str, object] = {
        "open_window_event_count": open_window_event_count,
        "closed_window_event_count": 0,
        "expected_activity_coupling_j_t_increment_total": 0.0,
        "observed_activity_coupling_j_t_increment_total": 0.0,
        "expected_open_window_direct_gap_g_t_increment_total": 0.0,
        "observed_open_window_direct_gap_g_t_increment_total": observed_open_window_direct_gap_total,
        "expected_closed_window_direct_gap_g_t_increment_total": 0.0,
        "observed_closed_window_direct_gap_g_t_increment_total": 0.0,
        "expected_total_direct_gap_g_t_increment": 0.0,
        "observed_total_direct_gap_g_t_increment": observed_open_window_direct_gap_total,
    }
    return {
        "fixture_id": fixture_id,
        "exchange_code": exchange_code,
        "contributions": contributions,
    }


def _synthesize_legacy_behavior_diff(
    raw_case_report: Mapping[str, object],
    contribution_summary: Mapping[str, object],
    direct_gap_absolute_tolerance: float,
) -> dict[str, object]:
    fixture_id = cast(str, contribution_summary["fixture_id"])
    exchange_code = cast(str, contribution_summary["exchange_code"])
    contributions = cast(Mapping[str, object], contribution_summary["contributions"])

    metrics: list[dict[str, object]] = []
    for metric_id, expected_field, observed_field in [
        (
            "activity_coupling_j_t_increment_total",
            "expected_activity_coupling_j_t_increment_total",
            "observed_activity_coupling_j_t_increment_total",
        ),
        (
            "open_window_direct_gap_g_t_increment_total",
            "expected_open_window_direct_gap_g_t_increment_total",
            "observed_open_window_direct_gap_g_t_increment_total",
        ),
        (
            "closed_window_direct_gap_g_t_increment_total",
            "expected_closed_window_direct_gap_g_t_increment_total",
            "observed_closed_window_direct_gap_g_t_increment_total",
        ),
        (
            "total_direct_gap_g_t_increment",
            "expected_total_direct_gap_g_t_increment",
            "observed_total_direct_gap_g_t_increment",
        ),
    ]:
        expected_value = float(contributions[expected_field])
        observed_value = float(contributions[observed_field])
        absolute_difference = abs(expected_value - observed_value)
        regression_detected = absolute_difference > direct_gap_absolute_tolerance
        metrics.append(
            {
                "metric_id": metric_id,
                "expected_value": expected_value,
                "observed_value": observed_value,
                "absolute_difference": absolute_difference,
                "absolute_tolerance": direct_gap_absolute_tolerance,
                "regression_detected": regression_detected,
            },
        )

    regression_metric_count = sum(1 for metric in metrics if cast(bool, metric["regression_detected"]))
    max_absolute_difference = max(cast(float, metric["absolute_difference"]) for metric in metrics)
    return {
        "fixture_id": fixture_id,
        "exchange_code": exchange_code,
        "regression_metric_count": regression_metric_count,
        "max_absolute_difference": max_absolute_difference,
        "metrics": metrics,
    }


def _legacy_observed_open_window_direct_gap_total(raw_failures: object) -> float:
    if not isinstance(raw_failures, list):
        return 0.0

    observed_direct_gap_total = 0.0
    for raw_failure in raw_failures:
        if not isinstance(raw_failure, Mapping):
            continue
        if raw_failure.get("failure_kind") != "open_window_direct_gap_leakage":
            continue
        observed_direct_gap_increment = raw_failure.get("observed_direct_gap_increment")
        if not isinstance(observed_direct_gap_increment, int | float) or isinstance(
            observed_direct_gap_increment,
            bool,
        ):
            continue
        observed_direct_gap_total += float(observed_direct_gap_increment)

    return observed_direct_gap_total


def _optional_non_negative_int(raw_value: object) -> int:
    if not isinstance(raw_value, int) or isinstance(raw_value, bool):
        return 0
    if raw_value < 0:
        return 0
    return raw_value


def _default_open_closed_gap_output_schema_metadata() -> dict[str, object]:
    return {
        "schema_version": OPEN_CLOSED_GAP_LEAKAGE_REGRESSION_OUTPUT_SCHEMA_METADATA_VERSION,
        "summary_artifact_schema_version": OPEN_CLOSED_GAP_BEHAVIOR_SUMMARY_ARTIFACT_SCHEMA_VERSION,
        "behavior_diff_report_schema_version": OPEN_CLOSED_GAP_BEHAVIOR_DIFF_REPORT_SCHEMA_VERSION,
        "baseline_expected_output_policy": OPEN_CLOSED_GAP_BEHAVIOR_BASELINE_EXPECTED_OUTPUT_POLICY,
        "environment_metadata_policy": OPEN_CLOSED_GAP_BEHAVIOR_ENVIRONMENT_METADATA_POLICY,
        "required_case_report_fields": [
            "fixture_id",
            "exchange_code",
            "status",
            "summary",
            "open_window_event_count",
            "leakage_failure_count",
            "max_absolute_leakage_magnitude",
            "contribution_summary",
            "behavior_diff",
            "failures",
        ],
        "required_summary_artifact_fields": [
            "schema_version",
            "determinism_policy",
            "total_case_count",
            "total_open_window_event_count",
            "total_closed_window_event_count",
            "scenarios",
        ],
        "required_behavior_diff_report_fields": [
            "schema_version",
            "determinism_policy",
            "baseline_expected_output_policy",
            "summary",
            "scenario_diffs",
        ],
        "required_behavior_diff_metric_fields": [
            "metric_id",
            "expected_value",
            "observed_value",
            "absolute_difference",
            "absolute_tolerance",
            "regression_detected",
        ],
    }


def _validate_open_closed_gap_output_schema_metadata(
    raw_payload: Mapping[str, object],
    report_context: str,
) -> OpenClosedGapLeakageRegressionOutputSchemaMetadata:
    output_schema_metadata_context = f"{report_context}.output_schema_metadata"
    output_schema_metadata_mapping: Mapping[str, object] = _require_mapping(
        raw_payload,
        "output_schema_metadata",
        report_context,
    )
    output_schema_metadata: OpenClosedGapLeakageRegressionOutputSchemaMetadata = {
        "schema_version": _require_non_empty_string(
            output_schema_metadata_mapping,
            "schema_version",
            output_schema_metadata_context,
        ),
        "summary_artifact_schema_version": _require_non_empty_string(
            output_schema_metadata_mapping,
            "summary_artifact_schema_version",
            output_schema_metadata_context,
        ),
        "behavior_diff_report_schema_version": _require_non_empty_string(
            output_schema_metadata_mapping,
            "behavior_diff_report_schema_version",
            output_schema_metadata_context,
        ),
        "baseline_expected_output_policy": _require_non_empty_string(
            output_schema_metadata_mapping,
            "baseline_expected_output_policy",
            output_schema_metadata_context,
        ),
        "environment_metadata_policy": _require_non_empty_string(
            output_schema_metadata_mapping,
            "environment_metadata_policy",
            output_schema_metadata_context,
        ),
        "required_case_report_fields": _require_non_empty_string_list(
            output_schema_metadata_mapping,
            "required_case_report_fields",
            output_schema_metadata_context,
        ),
        "required_summary_artifact_fields": _require_non_empty_string_list(
            output_schema_metadata_mapping,
            "required_summary_artifact_fields",
            output_schema_metadata_context,
        ),
        "required_behavior_diff_report_fields": _require_non_empty_string_list(
            output_schema_metadata_mapping,
            "required_behavior_diff_report_fields",
            output_schema_metadata_context,
        ),
        "required_behavior_diff_metric_fields": _require_non_empty_string_list(
            output_schema_metadata_mapping,
            "required_behavior_diff_metric_fields",
            output_schema_metadata_context,
        ),
    }

    if (
        output_schema_metadata["schema_version"]
        != OPEN_CLOSED_GAP_LEAKAGE_REGRESSION_OUTPUT_SCHEMA_METADATA_VERSION
    ):
        raise SeasonalityInputValidationError(
            "unexpected open closed gap leakage regression harness output_schema_metadata.schema_version: "
            f"{output_schema_metadata['schema_version']}",
        )

    if (
        output_schema_metadata["summary_artifact_schema_version"]
        != OPEN_CLOSED_GAP_BEHAVIOR_SUMMARY_ARTIFACT_SCHEMA_VERSION
    ):
        raise SeasonalityInputValidationError(
            "unexpected open closed gap leakage regression harness output_schema_metadata.summary_artifact_schema_version: "
            f"{output_schema_metadata['summary_artifact_schema_version']}",
        )

    if (
        output_schema_metadata["behavior_diff_report_schema_version"]
        != OPEN_CLOSED_GAP_BEHAVIOR_DIFF_REPORT_SCHEMA_VERSION
    ):
        raise SeasonalityInputValidationError(
            "unexpected open closed gap leakage regression harness output_schema_metadata.behavior_diff_report_schema_version: "
            f"{output_schema_metadata['behavior_diff_report_schema_version']}",
        )

    if (
        output_schema_metadata["baseline_expected_output_policy"]
        != OPEN_CLOSED_GAP_BEHAVIOR_BASELINE_EXPECTED_OUTPUT_POLICY
    ):
        raise SeasonalityInputValidationError(
            "unexpected open closed gap leakage regression harness output_schema_metadata.baseline_expected_output_policy: "
            f"{output_schema_metadata['baseline_expected_output_policy']}",
        )

    if (
        output_schema_metadata["environment_metadata_policy"]
        != OPEN_CLOSED_GAP_BEHAVIOR_ENVIRONMENT_METADATA_POLICY
    ):
        raise SeasonalityInputValidationError(
            "unexpected open closed gap leakage regression harness output_schema_metadata.environment_metadata_policy: "
            f"{output_schema_metadata['environment_metadata_policy']}",
        )

    return output_schema_metadata


def _validate_open_closed_gap_tolerance_config(
    raw_payload: Mapping[str, object],
    report_context: str,
) -> OpenClosedGapLeakageRegressionHarnessToleranceConfig:
    tolerance_config_mapping: Mapping[str, object] = _require_mapping(
        raw_payload,
        "tolerance_config",
        report_context,
    )
    return {
        "direct_gap_absolute_tolerance": _validate_finite_non_negative_float(
            _require_field(
                tolerance_config_mapping,
                "direct_gap_absolute_tolerance",
                f"{report_context}.tolerance_config",
            ),
            "tolerance_config.direct_gap_absolute_tolerance",
        ),
    }


def _validate_open_closed_gap_harness_summary(
    raw_payload: Mapping[str, object],
    report_context: str,
) -> OpenClosedGapLeakageRegressionHarnessSummary:
    summary_mapping: Mapping[str, object] = _require_mapping(
        raw_payload,
        "summary",
        report_context,
    )
    summary_context = f"{report_context}.summary"
    return {
        "total_case_count": _require_non_negative_integer(
            summary_mapping,
            "total_case_count",
            summary_context,
        ),
        "passed_case_count": _require_non_negative_integer(
            summary_mapping,
            "passed_case_count",
            summary_context,
        ),
        "failed_case_count": _require_non_negative_integer(
            summary_mapping,
            "failed_case_count",
            summary_context,
        ),
        "total_open_window_event_count": _require_non_negative_integer(
            summary_mapping,
            "total_open_window_event_count",
            summary_context,
        ),
        "total_leakage_failure_count": _require_non_negative_integer(
            summary_mapping,
            "total_leakage_failure_count",
            summary_context,
        ),
    }


def _validate_open_closed_gap_summary_artifact(
    raw_payload: Mapping[str, object],
    report_context: str,
) -> OpenClosedGapBehaviorSummaryArtifact:
    summary_artifact_context = f"{report_context}.summary_artifact"
    summary_artifact_mapping: Mapping[str, object] = _require_mapping(
        raw_payload,
        "summary_artifact",
        report_context,
    )

    summary_artifact_schema_version = _require_non_empty_string(
        summary_artifact_mapping,
        "schema_version",
        summary_artifact_context,
    )
    if summary_artifact_schema_version != OPEN_CLOSED_GAP_BEHAVIOR_SUMMARY_ARTIFACT_SCHEMA_VERSION:
        raise SeasonalityInputValidationError(
            "unexpected open closed gap leakage regression harness summary_artifact.schema_version: "
            f"{summary_artifact_schema_version}",
        )

    summary_artifact_determinism_policy = _require_non_empty_string(
        summary_artifact_mapping,
        "determinism_policy",
        summary_artifact_context,
    )
    if summary_artifact_determinism_policy != OPEN_CLOSED_GAP_BEHAVIOR_SUMMARY_DETERMINISM_POLICY:
        raise SeasonalityInputValidationError(
            "unexpected open closed gap leakage regression harness summary_artifact.determinism_policy: "
            f"{summary_artifact_determinism_policy}",
        )

    summary_artifact_scenarios_raw: object = _require_field(
        summary_artifact_mapping,
        "scenarios",
        summary_artifact_context,
    )
    if not isinstance(summary_artifact_scenarios_raw, list):
        raise SeasonalityInputValidationError(
            "open closed gap leakage regression harness report.summary_artifact.scenarios must be a list",
        )

    summary_artifact_scenarios: list[OpenClosedGapBehaviorScenarioSummaryArtifact] = []
    for scenario_position, raw_summary_scenario in enumerate(summary_artifact_scenarios_raw):
        scenario_context = f"{summary_artifact_context}.scenarios[{scenario_position}]"
        if not isinstance(raw_summary_scenario, Mapping):
            raise SeasonalityInputValidationError(f"{scenario_context} must be a mapping")

        summary_artifact_scenarios.append(
            {
                "fixture_id": _require_non_empty_string(raw_summary_scenario, "fixture_id", scenario_context),
                "exchange_code": _require_non_empty_string(raw_summary_scenario, "exchange_code", scenario_context),
                "contributions": _validate_open_closed_gap_behavior_contribution_summary(
                    _require_mapping(raw_summary_scenario, "contributions", scenario_context),
                    f"{scenario_context}.contributions",
                ),
            },
        )

    return {
        "schema_version": summary_artifact_schema_version,
        "determinism_policy": summary_artifact_determinism_policy,
        "total_case_count": _require_non_negative_integer(
            summary_artifact_mapping,
            "total_case_count",
            summary_artifact_context,
        ),
        "total_open_window_event_count": _require_non_negative_integer(
            summary_artifact_mapping,
            "total_open_window_event_count",
            summary_artifact_context,
        ),
        "total_closed_window_event_count": _require_non_negative_integer(
            summary_artifact_mapping,
            "total_closed_window_event_count",
            summary_artifact_context,
        ),
        "scenarios": summary_artifact_scenarios,
    }


def _validate_open_closed_gap_behavior_diff_report(
    raw_payload: Mapping[str, object],
    report_context: str,
) -> OpenClosedGapBehaviorDiffReport:
    behavior_diff_report_context = f"{report_context}.behavior_diff_report"
    behavior_diff_report_mapping: Mapping[str, object] = _require_mapping(
        raw_payload,
        "behavior_diff_report",
        report_context,
    )

    behavior_diff_report_schema_version = _require_non_empty_string(
        behavior_diff_report_mapping,
        "schema_version",
        behavior_diff_report_context,
    )
    if behavior_diff_report_schema_version != OPEN_CLOSED_GAP_BEHAVIOR_DIFF_REPORT_SCHEMA_VERSION:
        raise SeasonalityInputValidationError(
            "unexpected open closed gap leakage regression harness behavior_diff_report.schema_version: "
            f"{behavior_diff_report_schema_version}",
        )

    behavior_diff_determinism_policy = _require_non_empty_string(
        behavior_diff_report_mapping,
        "determinism_policy",
        behavior_diff_report_context,
    )
    if behavior_diff_determinism_policy != OPEN_CLOSED_GAP_BEHAVIOR_DIFF_DETERMINISM_POLICY:
        raise SeasonalityInputValidationError(
            "unexpected open closed gap leakage regression harness behavior_diff_report.determinism_policy: "
            f"{behavior_diff_determinism_policy}",
        )

    behavior_diff_baseline_policy = _require_non_empty_string(
        behavior_diff_report_mapping,
        "baseline_expected_output_policy",
        behavior_diff_report_context,
    )
    if behavior_diff_baseline_policy != OPEN_CLOSED_GAP_BEHAVIOR_BASELINE_EXPECTED_OUTPUT_POLICY:
        raise SeasonalityInputValidationError(
            "unexpected open closed gap leakage regression harness behavior_diff_report.baseline_expected_output_policy: "
            f"{behavior_diff_baseline_policy}",
        )

    behavior_diff_report_summary_mapping = _require_mapping(
        behavior_diff_report_mapping,
        "summary",
        behavior_diff_report_context,
    )
    summary_context = f"{behavior_diff_report_context}.summary"
    behavior_diff_report_summary: OpenClosedGapBehaviorDiffReportSummary = {
        "total_case_count": _require_non_negative_integer(
            behavior_diff_report_summary_mapping,
            "total_case_count",
            summary_context,
        ),
        "regression_case_count": _require_non_negative_integer(
            behavior_diff_report_summary_mapping,
            "regression_case_count",
            summary_context,
        ),
        "total_regression_metric_count": _require_non_negative_integer(
            behavior_diff_report_summary_mapping,
            "total_regression_metric_count",
            summary_context,
        ),
    }

    behavior_diff_scenarios_raw: object = _require_field(
        behavior_diff_report_mapping,
        "scenario_diffs",
        behavior_diff_report_context,
    )
    if not isinstance(behavior_diff_scenarios_raw, list):
        raise SeasonalityInputValidationError(
            "open closed gap leakage regression harness report.behavior_diff_report.scenario_diffs must be a list",
        )

    behavior_diff_scenarios: list[OpenClosedGapBehaviorScenarioDiff] = []
    for scenario_position, raw_scenario_diff in enumerate(behavior_diff_scenarios_raw):
        scenario_context = f"{behavior_diff_report_context}.scenario_diffs[{scenario_position}]"
        behavior_diff_scenarios.append(
            _validate_open_closed_gap_behavior_scenario_diff(raw_scenario_diff, scenario_context),
        )

    return {
        "schema_version": behavior_diff_report_schema_version,
        "determinism_policy": behavior_diff_determinism_policy,
        "baseline_expected_output_policy": behavior_diff_baseline_policy,
        "summary": behavior_diff_report_summary,
        "scenario_diffs": behavior_diff_scenarios,
    }


def _validate_open_closed_gap_case_reports(
    raw_payload: Mapping[str, object],
    report_context: str,
) -> list[OpenClosedGapLeakageRegressionCaseReport]:
    case_reports_raw: object = _require_field(
        raw_payload,
        "case_reports",
        report_context,
    )
    if not isinstance(case_reports_raw, list):
        raise SeasonalityInputValidationError(
            "open closed gap leakage regression harness report.case_reports must be a list",
        )

    case_reports: list[OpenClosedGapLeakageRegressionCaseReport] = []
    for case_position, raw_case_report in enumerate(case_reports_raw):
        case_context = f"{report_context}.case_reports[{case_position}]"
        case_reports.append(_validate_open_closed_gap_case_report(raw_case_report, case_context))

    return case_reports


def _validate_open_closed_gap_case_report(
    raw_case_report: object,
    case_context: str,
) -> OpenClosedGapLeakageRegressionCaseReport:
    if not isinstance(raw_case_report, Mapping):
        raise SeasonalityInputValidationError(f"{case_context} must be a mapping")

    status: str = _require_non_empty_string(raw_case_report, "status", case_context)
    if status not in {"pass", "fail"}:
        raise SeasonalityInputValidationError(
            f"{case_context}.status has unsupported value: {status}",
        )

    failures = _validate_open_closed_gap_case_failures(raw_case_report, case_context)

    return {
        "fixture_id": _require_non_empty_string(raw_case_report, "fixture_id", case_context),
        "exchange_code": _require_non_empty_string(raw_case_report, "exchange_code", case_context),
        "status": status,
        "summary": _require_non_empty_string(raw_case_report, "summary", case_context),
        "open_window_event_count": _require_non_negative_integer(
            raw_case_report,
            "open_window_event_count",
            case_context,
        ),
        "leakage_failure_count": _require_non_negative_integer(
            raw_case_report,
            "leakage_failure_count",
            case_context,
        ),
        "max_absolute_leakage_magnitude": _validate_finite_non_negative_float(
            _require_field(raw_case_report, "max_absolute_leakage_magnitude", case_context),
            f"{case_context}.max_absolute_leakage_magnitude",
        ),
        "contribution_summary": _validate_open_closed_gap_behavior_contribution_summary(
            _require_mapping(raw_case_report, "contribution_summary", case_context),
            f"{case_context}.contribution_summary",
        ),
        "behavior_diff": _validate_open_closed_gap_behavior_scenario_diff(
            _require_mapping(raw_case_report, "behavior_diff", case_context),
            f"{case_context}.behavior_diff",
        ),
        "failures": failures,
    }


def _validate_open_closed_gap_case_failures(
    raw_case_report: Mapping[str, object],
    case_context: str,
) -> list[OpenClosedGapLeakageRegressionFailure]:
    failures_raw: object = _require_field(raw_case_report, "failures", case_context)
    if not isinstance(failures_raw, list):
        raise SeasonalityInputValidationError(f"{case_context}.failures must be a list")

    failures: list[OpenClosedGapLeakageRegressionFailure] = []
    for failure_position, raw_failure in enumerate(failures_raw):
        failure_context = f"{case_context}.failures[{failure_position}]"
        failures.append(_validate_open_closed_gap_case_failure(raw_failure, failure_context))

    return failures


def _validate_open_closed_gap_case_failure(
    raw_failure: object,
    failure_context: str,
) -> OpenClosedGapLeakageRegressionFailure:
    if not isinstance(raw_failure, Mapping):
        raise SeasonalityInputValidationError(f"{failure_context} must be a mapping")

    failure_kind: str = _require_non_empty_string(raw_failure, "failure_kind", failure_context)
    match failure_kind:
        case "dispatch_error":
            return {
                "failure_kind": failure_kind,
                "event_id": None,
                "event_timestamp_utc": None,
                "observed_direct_gap_increment": None,
                "absolute_leakage_magnitude": None,
                "error_message": _require_non_empty_string(
                    raw_failure,
                    "error_message",
                    failure_context,
                ),
            }
        case "open_window_direct_gap_leakage":
            return {
                "failure_kind": failure_kind,
                "event_id": _require_non_empty_string(raw_failure, "event_id", failure_context),
                "event_timestamp_utc": _require_non_empty_string(
                    raw_failure,
                    "event_timestamp_utc",
                    failure_context,
                ),
                "observed_direct_gap_increment": _validate_finite_float(
                    _require_field(
                        raw_failure,
                        "observed_direct_gap_increment",
                        failure_context,
                    ),
                    f"{failure_context}.observed_direct_gap_increment",
                ),
                "absolute_leakage_magnitude": _validate_finite_non_negative_float(
                    _require_field(
                        raw_failure,
                        "absolute_leakage_magnitude",
                        failure_context,
                    ),
                    f"{failure_context}.absolute_leakage_magnitude",
                ),
                "error_message": None,
            }
        case _:
            raise SeasonalityInputValidationError(
                f"{failure_context}.failure_kind has unsupported value: {failure_kind}",
            )


def _validate_open_closed_gap_behavior_contribution_summary(
    raw_contribution_summary: Mapping[str, object],
    context: str,
) -> OpenClosedGapBehaviorContributionSummary:
    return {
        "open_window_event_count": _require_non_negative_integer(
            raw_contribution_summary,
            "open_window_event_count",
            context,
        ),
        "closed_window_event_count": _require_non_negative_integer(
            raw_contribution_summary,
            "closed_window_event_count",
            context,
        ),
        "expected_activity_coupling_j_t_increment_total": _validate_finite_float(
            _require_field(raw_contribution_summary, "expected_activity_coupling_j_t_increment_total", context),
            f"{context}.expected_activity_coupling_j_t_increment_total",
        ),
        "observed_activity_coupling_j_t_increment_total": _validate_finite_float(
            _require_field(raw_contribution_summary, "observed_activity_coupling_j_t_increment_total", context),
            f"{context}.observed_activity_coupling_j_t_increment_total",
        ),
        "expected_open_window_direct_gap_g_t_increment_total": _validate_finite_float(
            _require_field(raw_contribution_summary, "expected_open_window_direct_gap_g_t_increment_total", context),
            f"{context}.expected_open_window_direct_gap_g_t_increment_total",
        ),
        "observed_open_window_direct_gap_g_t_increment_total": _validate_finite_float(
            _require_field(raw_contribution_summary, "observed_open_window_direct_gap_g_t_increment_total", context),
            f"{context}.observed_open_window_direct_gap_g_t_increment_total",
        ),
        "expected_closed_window_direct_gap_g_t_increment_total": _validate_finite_float(
            _require_field(raw_contribution_summary, "expected_closed_window_direct_gap_g_t_increment_total", context),
            f"{context}.expected_closed_window_direct_gap_g_t_increment_total",
        ),
        "observed_closed_window_direct_gap_g_t_increment_total": _validate_finite_float(
            _require_field(raw_contribution_summary, "observed_closed_window_direct_gap_g_t_increment_total", context),
            f"{context}.observed_closed_window_direct_gap_g_t_increment_total",
        ),
        "expected_total_direct_gap_g_t_increment": _validate_finite_float(
            _require_field(raw_contribution_summary, "expected_total_direct_gap_g_t_increment", context),
            f"{context}.expected_total_direct_gap_g_t_increment",
        ),
        "observed_total_direct_gap_g_t_increment": _validate_finite_float(
            _require_field(raw_contribution_summary, "observed_total_direct_gap_g_t_increment", context),
            f"{context}.observed_total_direct_gap_g_t_increment",
        ),
    }


def _validate_open_closed_gap_behavior_scenario_diff(
    raw_scenario_diff: object,
    context: str,
) -> OpenClosedGapBehaviorScenarioDiff:
    if not isinstance(raw_scenario_diff, Mapping):
        raise SeasonalityInputValidationError(f"{context} must be a mapping")

    metrics_raw: object = _require_field(raw_scenario_diff, "metrics", context)
    if not isinstance(metrics_raw, list):
        raise SeasonalityInputValidationError(f"{context}.metrics must be a list")

    metrics: list[OpenClosedGapBehaviorDiffMetric] = []
    for metric_position, raw_metric in enumerate(metrics_raw):
        metric_context = f"{context}.metrics[{metric_position}]"
        if not isinstance(raw_metric, Mapping):
            raise SeasonalityInputValidationError(f"{metric_context} must be a mapping")

        metrics.append(
            {
                "metric_id": _require_non_empty_string(raw_metric, "metric_id", metric_context),
                "expected_value": _validate_finite_float(
                    _require_field(raw_metric, "expected_value", metric_context),
                    f"{metric_context}.expected_value",
                ),
                "observed_value": _validate_finite_float(
                    _require_field(raw_metric, "observed_value", metric_context),
                    f"{metric_context}.observed_value",
                ),
                "absolute_difference": _validate_finite_non_negative_float(
                    _require_field(raw_metric, "absolute_difference", metric_context),
                    f"{metric_context}.absolute_difference",
                ),
                "absolute_tolerance": _validate_finite_non_negative_float(
                    _require_field(raw_metric, "absolute_tolerance", metric_context),
                    f"{metric_context}.absolute_tolerance",
                ),
                "regression_detected": _require_boolean(raw_metric, "regression_detected", metric_context),
            },
        )

    return {
        "fixture_id": _require_non_empty_string(raw_scenario_diff, "fixture_id", context),
        "exchange_code": _require_non_empty_string(raw_scenario_diff, "exchange_code", context),
        "regression_metric_count": _require_non_negative_integer(
            raw_scenario_diff,
            "regression_metric_count",
            context,
        ),
        "max_absolute_difference": _validate_finite_non_negative_float(
            _require_field(raw_scenario_diff, "max_absolute_difference", context),
            f"{context}.max_absolute_difference",
        ),
        "metrics": metrics,
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


def _require_boolean(parent: Mapping[str, object], field_name: str, context: str) -> bool:
    field_value: object = _require_field(parent, field_name, context)
    if not isinstance(field_value, bool):
        raise SeasonalityInputValidationError(
            f"{context}.{field_name} must be a boolean",
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
    "OPEN_CLOSED_GAP_BEHAVIOR_BASELINE_EXPECTED_OUTPUT_POLICY",
    "OPEN_CLOSED_GAP_BEHAVIOR_DIFF_DETERMINISM_POLICY",
    "OPEN_CLOSED_GAP_BEHAVIOR_DIFF_REPORT_SCHEMA_VERSION",
    "OPEN_CLOSED_GAP_BEHAVIOR_ENVIRONMENT_METADATA_POLICY",
    "OPEN_CLOSED_GAP_BEHAVIOR_SUMMARY_ARTIFACT_SCHEMA_VERSION",
    "OPEN_CLOSED_GAP_BEHAVIOR_SUMMARY_DETERMINISM_POLICY",
    "OPEN_CLOSED_GAP_LEAKAGE_REGRESSION_HARNESS_DETERMINISM_POLICY",
    "OPEN_CLOSED_GAP_LEAKAGE_REGRESSION_HARNESS_REPORT_SCHEMA_VERSION",
    "OPEN_CLOSED_GAP_LEAKAGE_REGRESSION_OUTPUT_SCHEMA_METADATA_VERSION",
    "DeterministicRegressionHarnessReport",
    "OpenClosedGapBehaviorContributionSummary",
    "OpenClosedGapBehaviorDiffMetric",
    "OpenClosedGapBehaviorDiffReport",
    "OpenClosedGapBehaviorDiffReportSummary",
    "OpenClosedGapBehaviorScenarioDiff",
    "OpenClosedGapBehaviorScenarioSummaryArtifact",
    "OpenClosedGapBehaviorSummaryArtifact",
    "OpenClosedGapLeakageRegressionCaseReport",
    "OpenClosedGapLeakageRegressionFailure",
    "OpenClosedGapLeakageRegressionHarnessReport",
    "OpenClosedGapLeakageRegressionOutputSchemaMetadata",
    "OpenClosedGapLeakageRegressionHarnessSummary",
    "OpenClosedGapLeakageRegressionHarnessToleranceConfig",
    "RegressionCaseReport",
    "RegressionHarnessFallbackPolicy",
    "RegressionHarnessSummary",
    "RegressionHarnessToleranceConfig",
    "VALIDATION_FALLBACK_POLICY_SCHEMA_VERSION",
    "VALIDATION_INCOMPLETE_FIXTURE_COVERAGE_POLICY",
    "VALIDATION_UNKNOWN_DATE_FALLBACK_POLICY",
    "VALIDATION_UNSUPPORTED_CALENDAR_SEGMENT_POLICY",
    "run_open_closed_gap_leakage_regression_harness",
    "run_deterministic_regression_harness",
]
