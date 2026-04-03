"""Public validation harness contracts."""

from __future__ import annotations

from collections.abc import Mapping
from dataclasses import dataclass
from typing import Any, Optional

from ..contracts import ContractModel
from ..contracts.base import (
    require_boolean as _require_boolean,
    require_field as _require_field,
    require_finite_number_sequence,
    require_finite_scalar,
    require_mapping as _require_mapping,
    require_non_empty_string as _require_non_empty_string,
    require_non_empty_string_sequence,
    require_non_negative_finite_scalar,
    require_non_negative_integer as _require_non_negative_integer,
    require_string_sequence,
)
from ._errors import VALIDATION_ERROR_FACTORY


@dataclass(frozen=True, slots=True)
class RegressionHarnessToleranceConfig(ContractModel):
    seasonality_absolute_tolerance: float
    news_intensity_absolute_tolerance: float


@dataclass(frozen=True, slots=True)
class RegressionHarnessSummary(ContractModel):
    total_case_count: int
    passed_case_count: int
    failed_case_count: int


@dataclass(frozen=True, slots=True)
class RegressionHarnessFallbackPolicy(ContractModel):
    schema_version: str
    unknown_date_policy: str
    unsupported_exchange_calendar_segment_policy: str
    incomplete_fixture_coverage_policy: str


@dataclass(frozen=True, slots=True)
class RegressionCaseReport(ContractModel):
    fixture_id: str
    case_kind: str
    status: str
    summary: str
    failure: Optional[dict[str, Any]]


@dataclass(frozen=True, slots=True)
class DeterministicRegressionHarnessReport(ContractModel):
    schema_version: str
    determinism_policy: str
    fallback_policy: RegressionHarnessFallbackPolicy
    tolerance_config: RegressionHarnessToleranceConfig
    summary: RegressionHarnessSummary
    case_reports: list[RegressionCaseReport]


@dataclass(frozen=True, slots=True)
class OpenClosedGapScenarioClassCoverage(ContractModel):
    known_only_case_count: int
    surprise_only_case_count: int
    mixed_news_case_count: int


@dataclass(frozen=True, slots=True)
class OpenClosedGapDeterministicReplayMetadata(ContractModel):
    replay_baseline_id: str
    event_calendar_binding_id: str
    jump_observation_binding_id: str
    gap_observation_binding_id: str
    candidate_selection_policy_id: str
    candidate_selection_rank: int


@dataclass(frozen=True, slots=True)
class OpenClosedGapIssueTraceabilityMetadata(ContractModel):
    issue_id: str
    semantic_focus_tags: list[str]


@dataclass(frozen=True, slots=True)
class OpenClosedGapLeakageRegressionHarnessToleranceConfig(ContractModel):
    component_absolute_tolerance: float


@dataclass(frozen=True, slots=True)
class OpenClosedGapLeakageRegressionFailure(ContractModel):
    failure_kind: str
    semantic_bucket: Optional[str]
    mismatch_kind: Optional[str]
    event_id: Optional[str]
    event_timestamp_utc: Optional[str]
    expected_value: Optional[float]
    observed_value: Optional[float]
    absolute_difference: Optional[float]
    error_message: Optional[str]


@dataclass(frozen=True, slots=True)
class OpenClosedGapLeakageRegressionOutputSchemaMetadata(ContractModel):
    schema_version: str
    summary_artifact_schema_version: str
    behavior_diff_report_schema_version: str
    baseline_expected_output_policy: str
    environment_metadata_policy: str
    canonical_component_vocabulary: list[str]
    canonical_semantic_bucket_vocabulary: list[str]
    required_case_report_fields: list[str]
    required_summary_artifact_fields: list[str]
    required_behavior_diff_report_fields: list[str]
    required_behavior_diff_metric_fields: list[str]


@dataclass(frozen=True, slots=True)
class OpenClosedGapBehaviorContributionSummary(ContractModel):
    open_window_event_count: int
    closed_window_event_count: int
    expected_activity_coupling_total: float
    observed_activity_coupling_total: float
    expected_trading_return_total: float
    observed_trading_return_total: float
    expected_open_jump_total: float
    observed_open_jump_total: float
    expected_closed_gap_total: float
    observed_closed_gap_total: float


@dataclass(frozen=True, slots=True)
class OpenClosedGapBehaviorDiffMetric(ContractModel):
    metric_id: str
    semantic_bucket: str
    expected_value: float
    observed_value: float
    absolute_difference: float
    absolute_tolerance: float
    regression_detected: bool


@dataclass(frozen=True, slots=True)
class OpenClosedGapBehaviorScenarioDiff(ContractModel):
    fixture_id: str
    exchange_code: str
    regression_metric_count: int
    max_absolute_difference: float
    metrics: list[OpenClosedGapBehaviorDiffMetric]


@dataclass(frozen=True, slots=True)
class OpenClosedGapBehaviorScenarioSummaryArtifact(ContractModel):
    fixture_id: str
    exchange_code: str
    scenario_class: str
    deterministic_replay_metadata: OpenClosedGapDeterministicReplayMetadata
    issue_traceability: OpenClosedGapIssueTraceabilityMetadata
    contributions: OpenClosedGapBehaviorContributionSummary


@dataclass(frozen=True, slots=True)
class OpenClosedGapBehaviorSummaryArtifact(ContractModel):
    schema_version: str
    determinism_policy: str
    total_case_count: int
    total_open_window_event_count: int
    total_closed_window_event_count: int
    scenario_class_coverage: OpenClosedGapScenarioClassCoverage
    scenarios: list[OpenClosedGapBehaviorScenarioSummaryArtifact]


@dataclass(frozen=True, slots=True)
class OpenClosedGapBehaviorDiffReportSummary(ContractModel):
    total_case_count: int
    regression_case_count: int
    total_regression_metric_count: int
    trading_return_regression_metric_count: int
    open_jump_regression_metric_count: int
    closed_gap_regression_metric_count: int
    activity_coupling_regression_metric_count: int


@dataclass(frozen=True, slots=True)
class OpenClosedGapBehaviorDiffReport(ContractModel):
    schema_version: str
    determinism_policy: str
    baseline_expected_output_policy: str
    summary: OpenClosedGapBehaviorDiffReportSummary
    scenario_diffs: list[OpenClosedGapBehaviorScenarioDiff]


@dataclass(frozen=True, slots=True)
class OpenClosedGapLeakageRegressionCaseReport(ContractModel):
    fixture_id: str
    exchange_code: str
    scenario_class: str
    deterministic_replay_metadata: OpenClosedGapDeterministicReplayMetadata
    issue_traceability: OpenClosedGapIssueTraceabilityMetadata
    status: str
    summary: str
    semantic_mismatch_failure_count: int
    max_absolute_mismatch: float
    contribution_summary: OpenClosedGapBehaviorContributionSummary
    behavior_diff: OpenClosedGapBehaviorScenarioDiff
    failures: list[OpenClosedGapLeakageRegressionFailure]


@dataclass(frozen=True, slots=True)
class OpenClosedGapLeakageRegressionHarnessSummary(ContractModel):
    total_case_count: int
    passed_case_count: int
    failed_case_count: int
    total_semantic_mismatch_failure_count: int
    scenario_class_coverage: OpenClosedGapScenarioClassCoverage


@dataclass(frozen=True, slots=True)
class OpenClosedGapLeakageRegressionHarnessReport(ContractModel):
    schema_version: str
    determinism_policy: str
    output_schema_metadata: OpenClosedGapLeakageRegressionOutputSchemaMetadata
    tolerance_config: OpenClosedGapLeakageRegressionHarnessToleranceConfig
    summary: OpenClosedGapLeakageRegressionHarnessSummary
    summary_artifact: OpenClosedGapBehaviorSummaryArtifact
    behavior_diff_report: OpenClosedGapBehaviorDiffReport
    case_reports: list[OpenClosedGapLeakageRegressionCaseReport]


def require_field(parent: Mapping[str, Any], field_name: str, context: str) -> Any:
    return _require_field(
        parent,
        field_name,
        f"{context}.{field_name}",
        VALIDATION_ERROR_FACTORY,
    )


def require_mapping(parent: Mapping[str, Any], field_name: str, context: str) -> Mapping[str, Any]:
    return _require_mapping(
        parent,
        field_name,
        f"{context}.{field_name}",
        VALIDATION_ERROR_FACTORY,
    )


def require_non_empty_string(parent: Mapping[str, Any], field_name: str, context: str) -> str:
    return _require_non_empty_string(
        parent,
        field_name,
        f"{context}.{field_name}",
        VALIDATION_ERROR_FACTORY,
    )


def require_non_empty_string_list(parent: Mapping[str, Any], field_name: str, context: str) -> list[str]:
    return require_non_empty_string_sequence(
        parent,
        field_name,
        f"{context}.{field_name}",
        VALIDATION_ERROR_FACTORY,
        require_non_empty_items=True,
        strip_items=True,
    )


def require_string_list(
    parent: Mapping[str, Any],
    field_name: str,
    context: str,
    *,
    require_non_empty: bool,
) -> list[str]:
    return require_string_sequence(
        parent,
        field_name,
        f"{context}.{field_name}",
        VALIDATION_ERROR_FACTORY,
        require_non_empty=require_non_empty,
        require_non_empty_items=True,
        strip_items=True,
    )


def require_float_list(parent: Mapping[str, Any], field_name: str, context: str) -> list[float]:
    return require_finite_number_sequence(
        parent,
        field_name,
        f"{context}.{field_name}",
        VALIDATION_ERROR_FACTORY,
        require_non_empty=True,
    )


def require_non_negative_integer(parent: Mapping[str, Any], field_name: str, context: str) -> int:
    return _require_non_negative_integer(
        parent,
        field_name,
        f"{context}.{field_name}",
        VALIDATION_ERROR_FACTORY,
    )


def require_boolean(parent: Mapping[str, Any], field_name: str, context: str) -> bool:
    return _require_boolean(
        parent,
        field_name,
        f"{context}.{field_name}",
        VALIDATION_ERROR_FACTORY,
    )


def validate_finite_float(raw_value: Any, context: str) -> float:
    return require_finite_scalar(
        raw_value,
        context,
        VALIDATION_ERROR_FACTORY,
    )


def validate_finite_non_negative_float(raw_value: Any, context: str) -> float:
    return require_non_negative_finite_scalar(
        raw_value,
        context,
        VALIDATION_ERROR_FACTORY,
    )


__all__ = [
    "DeterministicRegressionHarnessReport",
    "OpenClosedGapBehaviorContributionSummary",
    "OpenClosedGapBehaviorDiffMetric",
    "OpenClosedGapBehaviorDiffReport",
    "OpenClosedGapBehaviorDiffReportSummary",
    "OpenClosedGapBehaviorScenarioDiff",
    "OpenClosedGapBehaviorScenarioSummaryArtifact",
    "OpenClosedGapBehaviorSummaryArtifact",
    "OpenClosedGapDeterministicReplayMetadata",
    "OpenClosedGapIssueTraceabilityMetadata",
    "OpenClosedGapLeakageRegressionCaseReport",
    "OpenClosedGapLeakageRegressionFailure",
    "OpenClosedGapLeakageRegressionHarnessReport",
    "OpenClosedGapLeakageRegressionHarnessSummary",
    "OpenClosedGapLeakageRegressionHarnessToleranceConfig",
    "OpenClosedGapLeakageRegressionOutputSchemaMetadata",
    "OpenClosedGapScenarioClassCoverage",
    "RegressionCaseReport",
    "RegressionHarnessFallbackPolicy",
    "RegressionHarnessSummary",
    "RegressionHarnessToleranceConfig",
]
