import json
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


class RegressionHarnessToleranceConfig(TypedDict):
    seasonality_absolute_tolerance: float
    news_intensity_absolute_tolerance: float


class RegressionHarnessSummary(TypedDict):
    total_case_count: int
    passed_case_count: int
    failed_case_count: int


class RegressionCaseReport(TypedDict):
    fixture_id: str
    case_kind: str
    status: str
    summary: str
    failure: dict[str, object] | None


class DeterministicRegressionHarnessReport(TypedDict):
    schema_version: str
    determinism_policy: str
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
    native = native_module()
    report_json: str = native.run_deterministic_regression_harness(
        calendar_registry._native_registry,
        list(calendar_cases),
        list(seasonality_cases),
        seasonality_absolute_tolerance=seasonality_absolute_tolerance,
        news_intensity_absolute_tolerance=news_intensity_absolute_tolerance,
    )

    parsed_report: object = json.loads(report_json)
    if not isinstance(parsed_report, dict):
        raise SeasonalityInputValidationError(
            "deterministic regression harness report must decode to a mapping",
        )

    return cast(DeterministicRegressionHarnessReport, parsed_report)


__all__ = [
    "DETERMINISTIC_REGRESSION_HARNESS_DETERMINISM_POLICY",
    "DETERMINISTIC_REGRESSION_HARNESS_REPORT_SCHEMA_VERSION",
    "DeterministicRegressionHarnessReport",
    "RegressionCaseReport",
    "RegressionHarnessSummary",
    "RegressionHarnessToleranceConfig",
    "run_deterministic_regression_harness",
]
