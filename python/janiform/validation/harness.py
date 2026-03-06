import json
from collections.abc import Mapping, Sequence
from typing import Any

from .deterministic import (
    normalize_calendar_cases,
    normalize_seasonality_cases,
    validate_deterministic_harness_report_payload,
)
from .helpers import validate_finite_non_negative_float
from .open_closed_gap import (
    normalize_gap_behavior_cases,
    validate_open_closed_gap_leakage_report_payload,
)
from .types import (
    DeterministicRegressionHarnessReport,
    OpenClosedGapLeakageRegressionHarnessReport,
)
from ..api import native_module
from ..calendar import CalendarRegistry
from ..seasonality import SeasonalityInputValidationError


def run_deterministic_regression_harness(
    *,
    calendar_registry: CalendarRegistry,
    calendar_cases: Sequence[Mapping[str, Any]],
    seasonality_cases: Sequence[Mapping[str, Any]],
    seasonality_absolute_tolerance: float = 1e-12,
    news_intensity_absolute_tolerance: float = 1e-12,
) -> DeterministicRegressionHarnessReport:
    validate_finite_non_negative_float(seasonality_absolute_tolerance, "seasonality_absolute_tolerance")
    validate_finite_non_negative_float(news_intensity_absolute_tolerance, "news_intensity_absolute_tolerance")

    normalized_calendar_cases: list[dict[str, Any]] = normalize_calendar_cases(calendar_cases)
    normalized_seasonality_cases: list[dict[str, Any]] = normalize_seasonality_cases(seasonality_cases)

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

    return validate_deterministic_harness_report_payload(parsed_report)


def run_open_closed_gap_leakage_regression_harness(
    *,
    calendar_registry: CalendarRegistry,
    gap_behavior_cases: Sequence[Mapping[str, Any]],
    component_absolute_tolerance: float = 1e-12,
) -> OpenClosedGapLeakageRegressionHarnessReport:
    validate_finite_non_negative_float(component_absolute_tolerance, "component_absolute_tolerance")

    normalized_gap_behavior_cases: list[dict[str, Any]] = normalize_gap_behavior_cases(gap_behavior_cases)
    native = native_module()
    report_json: str = native.run_open_closed_gap_leakage_regression_harness(
        calendar_registry._native_registry,
        normalized_gap_behavior_cases,
        component_absolute_tolerance=component_absolute_tolerance,
    )

    try:
        parsed_report: object = json.loads(report_json)
    except json.JSONDecodeError as e:
        raise SeasonalityInputValidationError(
            f"open closed gap leakage regression harness report must be valid JSON: {e.msg}",
        ) from e

    return validate_open_closed_gap_leakage_report_payload(parsed_report)
