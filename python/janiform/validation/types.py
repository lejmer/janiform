from typing import Any, Optional, TypedDict


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
    failure: Optional[dict[str, Any]]


class DeterministicRegressionHarnessReport(TypedDict):
    schema_version: str
    determinism_policy: str
    fallback_policy: RegressionHarnessFallbackPolicy
    tolerance_config: RegressionHarnessToleranceConfig
    summary: RegressionHarnessSummary
    case_reports: list[RegressionCaseReport]


class OpenClosedGapScenarioClassCoverage(TypedDict):
    known_only_case_count: int
    surprise_only_case_count: int
    mixed_news_case_count: int


class OpenClosedGapDeterministicReplayMetadata(TypedDict):
    replay_baseline_id: str
    event_calendar_binding_id: str
    jump_observation_binding_id: str
    gap_observation_binding_id: str
    candidate_selection_policy_id: str
    candidate_selection_rank: int


class OpenClosedGapIssueTraceabilityMetadata(TypedDict):
    issue_id: str
    semantic_focus_tags: list[str]


class OpenClosedGapLeakageRegressionHarnessToleranceConfig(TypedDict):
    component_absolute_tolerance: float


class OpenClosedGapLeakageRegressionFailure(TypedDict):
    failure_kind: str
    semantic_bucket: Optional[str]
    mismatch_kind: Optional[str]
    event_id: Optional[str]
    event_timestamp_utc: Optional[str]
    expected_value: Optional[float]
    observed_value: Optional[float]
    absolute_difference: Optional[float]
    error_message: Optional[str]


class OpenClosedGapLeakageRegressionOutputSchemaMetadata(TypedDict):
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


class OpenClosedGapBehaviorContributionSummary(TypedDict):
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


class OpenClosedGapBehaviorDiffMetric(TypedDict):
    metric_id: str
    semantic_bucket: str
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
    scenario_class: str
    deterministic_replay_metadata: OpenClosedGapDeterministicReplayMetadata
    issue_traceability: OpenClosedGapIssueTraceabilityMetadata
    contributions: OpenClosedGapBehaviorContributionSummary


class OpenClosedGapBehaviorSummaryArtifact(TypedDict):
    schema_version: str
    determinism_policy: str
    total_case_count: int
    total_open_window_event_count: int
    total_closed_window_event_count: int
    scenario_class_coverage: OpenClosedGapScenarioClassCoverage
    scenarios: list[OpenClosedGapBehaviorScenarioSummaryArtifact]


class OpenClosedGapBehaviorDiffReportSummary(TypedDict):
    total_case_count: int
    regression_case_count: int
    total_regression_metric_count: int
    trading_return_regression_metric_count: int
    open_jump_regression_metric_count: int
    closed_gap_regression_metric_count: int
    activity_coupling_regression_metric_count: int


class OpenClosedGapBehaviorDiffReport(TypedDict):
    schema_version: str
    determinism_policy: str
    baseline_expected_output_policy: str
    summary: OpenClosedGapBehaviorDiffReportSummary
    scenario_diffs: list[OpenClosedGapBehaviorScenarioDiff]


class OpenClosedGapLeakageRegressionCaseReport(TypedDict):
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


class OpenClosedGapLeakageRegressionHarnessSummary(TypedDict):
    total_case_count: int
    passed_case_count: int
    failed_case_count: int
    total_semantic_mismatch_failure_count: int
    scenario_class_coverage: OpenClosedGapScenarioClassCoverage


class OpenClosedGapLeakageRegressionHarnessReport(TypedDict):
    schema_version: str
    determinism_policy: str
    output_schema_metadata: OpenClosedGapLeakageRegressionOutputSchemaMetadata
    tolerance_config: OpenClosedGapLeakageRegressionHarnessToleranceConfig
    summary: OpenClosedGapLeakageRegressionHarnessSummary
    summary_artifact: OpenClosedGapBehaviorSummaryArtifact
    behavior_diff_report: OpenClosedGapBehaviorDiffReport
    case_reports: list[OpenClosedGapLeakageRegressionCaseReport]
