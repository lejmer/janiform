from collections.abc import Mapping, Sequence
from typing import Any

from . import harness
from .deterministic import (
    DETERMINISTIC_REGRESSION_HARNESS_DETERMINISM_POLICY,
    DETERMINISTIC_REGRESSION_HARNESS_REPORT_SCHEMA_VERSION,
    VALIDATION_FALLBACK_POLICY_SCHEMA_VERSION,
    VALIDATION_INCOMPLETE_FIXTURE_COVERAGE_POLICY,
    VALIDATION_UNKNOWN_DATE_FALLBACK_POLICY,
    VALIDATION_UNSUPPORTED_CALENDAR_SEGMENT_POLICY,
)
from .open_closed_gap import (
    OPEN_CLOSED_GAP_BEHAVIOR_BASELINE_EXPECTED_OUTPUT_POLICY,
    OPEN_CLOSED_GAP_BEHAVIOR_DIFF_DETERMINISM_POLICY,
    OPEN_CLOSED_GAP_BEHAVIOR_DIFF_REPORT_SCHEMA_VERSION,
    OPEN_CLOSED_GAP_BEHAVIOR_ENVIRONMENT_METADATA_POLICY,
    OPEN_CLOSED_GAP_BEHAVIOR_SUMMARY_ARTIFACT_SCHEMA_VERSION,
    OPEN_CLOSED_GAP_BEHAVIOR_SUMMARY_DETERMINISM_POLICY,
    OPEN_CLOSED_GAP_LEAKAGE_REGRESSION_HARNESS_DETERMINISM_POLICY,
    OPEN_CLOSED_GAP_LEAKAGE_REGRESSION_HARNESS_REPORT_SCHEMA_VERSION,
    OPEN_CLOSED_GAP_LEAKAGE_REGRESSION_OUTPUT_SCHEMA_METADATA_VERSION,
)
from .types import (
    DeterministicRegressionHarnessReport,
    OpenClosedGapBehaviorContributionSummary,
    OpenClosedGapBehaviorDiffMetric,
    OpenClosedGapBehaviorDiffReport,
    OpenClosedGapBehaviorDiffReportSummary,
    OpenClosedGapBehaviorScenarioDiff,
    OpenClosedGapBehaviorScenarioSummaryArtifact,
    OpenClosedGapBehaviorSummaryArtifact,
    OpenClosedGapLeakageRegressionCaseReport,
    OpenClosedGapLeakageRegressionFailure,
    OpenClosedGapLeakageRegressionHarnessReport,
    OpenClosedGapLeakageRegressionHarnessSummary,
    OpenClosedGapLeakageRegressionHarnessToleranceConfig,
    OpenClosedGapLeakageRegressionOutputSchemaMetadata,
    RegressionCaseReport,
    RegressionHarnessFallbackPolicy,
    RegressionHarnessSummary,
    RegressionHarnessToleranceConfig,
)
from ..calendar import CalendarRegistry

native_module = harness.native_module


def run_deterministic_regression_harness(
    *,
    calendar_registry: CalendarRegistry,
    calendar_cases: Sequence[Mapping[str, Any]],
    seasonality_cases: Sequence[Mapping[str, Any]],
    seasonality_absolute_tolerance: float = 1e-12,
    news_intensity_absolute_tolerance: float = 1e-12,
) -> DeterministicRegressionHarnessReport:
    harness.native_module = native_module
    return harness.run_deterministic_regression_harness(
        calendar_registry=calendar_registry,
        calendar_cases=calendar_cases,
        seasonality_cases=seasonality_cases,
        seasonality_absolute_tolerance=seasonality_absolute_tolerance,
        news_intensity_absolute_tolerance=news_intensity_absolute_tolerance,
    )


def run_open_closed_gap_leakage_regression_harness(
    *,
    calendar_registry: CalendarRegistry,
    gap_behavior_cases: Sequence[Mapping[str, Any]],
    component_absolute_tolerance: float = 1e-12,
) -> OpenClosedGapLeakageRegressionHarnessReport:
    harness.native_module = native_module
    return harness.run_open_closed_gap_leakage_regression_harness(
        calendar_registry=calendar_registry,
        gap_behavior_cases=gap_behavior_cases,
        component_absolute_tolerance=component_absolute_tolerance,
    )


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
    "OpenClosedGapLeakageRegressionHarnessSummary",
    "OpenClosedGapLeakageRegressionHarnessToleranceConfig",
    "OpenClosedGapLeakageRegressionOutputSchemaMetadata",
    "RegressionCaseReport",
    "RegressionHarnessFallbackPolicy",
    "RegressionHarnessSummary",
    "RegressionHarnessToleranceConfig",
    "VALIDATION_FALLBACK_POLICY_SCHEMA_VERSION",
    "VALIDATION_INCOMPLETE_FIXTURE_COVERAGE_POLICY",
    "VALIDATION_UNKNOWN_DATE_FALLBACK_POLICY",
    "VALIDATION_UNSUPPORTED_CALENDAR_SEGMENT_POLICY",
    "native_module",
    "run_deterministic_regression_harness",
    "run_open_closed_gap_leakage_regression_harness",
]
