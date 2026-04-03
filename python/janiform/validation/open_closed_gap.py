from collections.abc import Mapping, Sequence
from typing import Any, cast

from ._contracts import (
    require_boolean,
    require_field,
    require_mapping,
    require_non_empty_string,
    require_non_empty_string_list,
    require_non_negative_integer,
    validate_finite_float,
    validate_finite_non_negative_float,
)
from .enum import (
    NewsArrivalComponent,
    OpenClosedGapFailureKind,
    OpenClosedGapScenarioClass,
    RegressionStatus,
    SemanticBucket,
    enum_values,
    parse_enum_member,
    require_enum_member,
    require_enum_members_list,
)
from .types import (
    OpenClosedGapBehaviorContributionSummary,
    OpenClosedGapBehaviorDiffMetric,
    OpenClosedGapBehaviorDiffReport,
    OpenClosedGapBehaviorDiffReportSummary,
    OpenClosedGapBehaviorScenarioDiff,
    OpenClosedGapBehaviorScenarioSummaryArtifact,
    OpenClosedGapBehaviorSummaryArtifact,
    OpenClosedGapDeterministicReplayMetadata,
    OpenClosedGapIssueTraceabilityMetadata,
    OpenClosedGapLeakageRegressionCaseReport,
    OpenClosedGapLeakageRegressionFailure,
    OpenClosedGapLeakageRegressionHarnessReport,
    OpenClosedGapLeakageRegressionHarnessSummary,
    OpenClosedGapLeakageRegressionHarnessToleranceConfig,
    OpenClosedGapLeakageRegressionOutputSchemaMetadata,
    OpenClosedGapScenarioClassCoverage,
)
from ..seasonality import SeasonalityInputValidationError


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


def normalize_gap_behavior_cases(
    gap_behavior_cases: Sequence[Mapping[str, Any]],
) -> list[dict[str, Any]]:
    normalized_cases: list[dict[str, Any]] = []

    for case_position, gap_behavior_case in enumerate(gap_behavior_cases):
        context: str = f"gap_behavior_cases[{case_position}]"
        fixture_id: str = require_non_empty_string(gap_behavior_case, "fixture_id", context)
        exchange_code: str = require_non_empty_string(gap_behavior_case, "exchange_code", context)
        scenario_class = require_enum_member(
            gap_behavior_case,
            "scenario_class",
            context,
            OpenClosedGapScenarioClass,
            error_message=(
                f"{context}.scenario_class must be one of 'known_only', 'surprise_only', or 'mixed_news'"
            ),
        )
        deterministic_replay_metadata = validate_open_closed_gap_deterministic_replay_metadata(
            require_mapping(gap_behavior_case, "deterministic_replay_metadata", context),
            f"{context}.deterministic_replay_metadata",
        )
        issue_traceability = validate_open_closed_gap_issue_traceability_metadata(
            require_mapping(gap_behavior_case, "issue_traceability", context),
            f"{context}.issue_traceability",
        )

        initial_j_t: float = validate_finite_float(
            require_field(gap_behavior_case, "initial_j_t", context),
            f"{context}.initial_j_t",
        )
        initial_m_t: float = validate_finite_float(
            require_field(gap_behavior_case, "initial_m_t", context),
            f"{context}.initial_m_t",
        )
        initial_g_t: float = validate_finite_float(
            require_field(gap_behavior_case, "initial_g_t", context),
            f"{context}.initial_g_t",
        )
        injected_events: Sequence[object] = cast(
            Sequence[object],
            require_field(gap_behavior_case, "injected_events", context),
        )
        if not isinstance(injected_events, Sequence) or isinstance(injected_events, str | bytes):
            raise SeasonalityInputValidationError(f"{context}.injected_events must be a sequence")

        normalized_injected_events: list[dict[str, Any]] = []
        for event_position, raw_event in enumerate(injected_events):
            event_context: str = f"{context}.injected_events[{event_position}]"
            if not isinstance(raw_event, Mapping):
                raise SeasonalityInputValidationError(f"{event_context} must be a mapping")

            normalized_event: dict[str, Any] = {
                "event_id": require_non_empty_string(raw_event, "event_id", event_context),
                "timestamp_utc": require_non_empty_string(raw_event, "timestamp_utc", event_context),
                "news_arrival_component": validate_open_closed_gap_news_arrival_component(
                    require_non_empty_string(raw_event, "news_arrival_component", event_context),
                    f"{event_context}.news_arrival_component",
                ).value,
                "shared_severity_mark": validate_finite_non_negative_float(
                    require_field(raw_event, "shared_severity_mark", event_context),
                    f"{event_context}.shared_severity_mark",
                ),
                "coupled_return_jump_mark": validate_finite_float(
                    require_field(raw_event, "coupled_return_jump_mark", event_context),
                    f"{event_context}.coupled_return_jump_mark",
                ),
            }

            for override_field_name in [
                "observed_activity_coupling_j_t_increment_override",
                "observed_open_jump_m_t_increment_override",
                "observed_closed_gap_g_t_increment_override",
                "observed_trading_return_increment_override",
            ]:
                if override_field_name not in raw_event:
                    continue

                raw_override: object = require_field(raw_event, override_field_name, event_context)
                if raw_override is None:
                    continue

                normalized_event[override_field_name] = validate_finite_float(
                    raw_override,
                    f"{event_context}.{override_field_name}",
                )

            normalized_injected_events.append(normalized_event)

        normalized_cases.append(
            {
                "fixture_id": fixture_id,
                "exchange_code": exchange_code,
                "scenario_class": scenario_class.value,
                "deterministic_replay_metadata": deterministic_replay_metadata,
                "issue_traceability": issue_traceability,
                "initial_j_t": initial_j_t,
                "initial_m_t": initial_m_t,
                "initial_g_t": initial_g_t,
                "injected_events": normalized_injected_events,
            },
        )

    return normalized_cases


def validate_open_closed_gap_news_arrival_component(
    value: str,
    context: str,
) -> NewsArrivalComponent:
    return parse_enum_member(
        value,
        NewsArrivalComponent,
        context,
        error_message=f"{context} must be either 'known' or 'surprise'",
    )


def validate_open_closed_gap_deterministic_replay_metadata(
    raw_metadata: Mapping[str, Any],
    context: str,
) -> OpenClosedGapDeterministicReplayMetadata:
    return {
        "replay_baseline_id": require_non_empty_string(raw_metadata, "replay_baseline_id", context),
        "event_calendar_binding_id": require_non_empty_string(
            raw_metadata,
            "event_calendar_binding_id",
            context,
        ),
        "jump_observation_binding_id": require_non_empty_string(
            raw_metadata,
            "jump_observation_binding_id",
            context,
        ),
        "gap_observation_binding_id": require_non_empty_string(
            raw_metadata,
            "gap_observation_binding_id",
            context,
        ),
        "candidate_selection_policy_id": require_non_empty_string(
            raw_metadata,
            "candidate_selection_policy_id",
            context,
        ),
        "candidate_selection_rank": require_non_negative_integer(
            raw_metadata,
            "candidate_selection_rank",
            context,
        ),
    }


def validate_open_closed_gap_issue_traceability_metadata(
    raw_metadata: Mapping[str, Any],
    context: str,
) -> OpenClosedGapIssueTraceabilityMetadata:
    return {
        "issue_id": require_non_empty_string(raw_metadata, "issue_id", context),
        "semantic_focus_tags": require_non_empty_string_list(raw_metadata, "semantic_focus_tags", context),
    }


def validate_open_closed_gap_leakage_report_payload(
    raw_payload: object,
) -> OpenClosedGapLeakageRegressionHarnessReport:
    report_context = "open closed gap leakage regression harness report"
    if not isinstance(raw_payload, Mapping):
        raise SeasonalityInputValidationError(
            f"{report_context} must decode to a mapping",
        )

    schema_version: str = require_non_empty_string(
        raw_payload,
        "schema_version",
        report_context,
    )
    if schema_version != OPEN_CLOSED_GAP_LEAKAGE_REGRESSION_HARNESS_REPORT_SCHEMA_VERSION:
        raise SeasonalityInputValidationError(
            "unsupported open closed gap leakage regression harness report schema_version: "
            f"{schema_version}",
        )

    determinism_policy: str = require_non_empty_string(
        raw_payload,
        "determinism_policy",
        report_context,
    )
    if determinism_policy != OPEN_CLOSED_GAP_LEAKAGE_REGRESSION_HARNESS_DETERMINISM_POLICY:
        raise SeasonalityInputValidationError(
            "unexpected open closed gap leakage regression harness determinism_policy: "
            f"{determinism_policy}",
        )

    output_schema_metadata = validate_open_closed_gap_output_schema_metadata(raw_payload, report_context)
    tolerance_config = validate_open_closed_gap_tolerance_config(raw_payload, report_context)
    summary = validate_open_closed_gap_harness_summary(raw_payload, report_context)
    summary_artifact = validate_open_closed_gap_summary_artifact(raw_payload, report_context)
    behavior_diff_report = validate_open_closed_gap_behavior_diff_report(raw_payload, report_context)
    case_reports = validate_open_closed_gap_case_reports(raw_payload, report_context)

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


def validate_open_closed_gap_output_schema_metadata(
    raw_payload: Mapping[str, Any],
    report_context: str,
) -> OpenClosedGapLeakageRegressionOutputSchemaMetadata:
    output_schema_metadata_context = f"{report_context}.output_schema_metadata"
    output_schema_metadata_mapping: Mapping[str, Any] = require_mapping(
        raw_payload,
        "output_schema_metadata",
        report_context,
    )
    output_schema_metadata: OpenClosedGapLeakageRegressionOutputSchemaMetadata = {
        "schema_version": require_non_empty_string(
            output_schema_metadata_mapping,
            "schema_version",
            output_schema_metadata_context,
        ),
        "summary_artifact_schema_version": require_non_empty_string(
            output_schema_metadata_mapping,
            "summary_artifact_schema_version",
            output_schema_metadata_context,
        ),
        "behavior_diff_report_schema_version": require_non_empty_string(
            output_schema_metadata_mapping,
            "behavior_diff_report_schema_version",
            output_schema_metadata_context,
        ),
        "baseline_expected_output_policy": require_non_empty_string(
            output_schema_metadata_mapping,
            "baseline_expected_output_policy",
            output_schema_metadata_context,
        ),
        "environment_metadata_policy": require_non_empty_string(
            output_schema_metadata_mapping,
            "environment_metadata_policy",
            output_schema_metadata_context,
        ),
        "canonical_component_vocabulary": require_non_empty_string_list(
            output_schema_metadata_mapping,
            "canonical_component_vocabulary",
            output_schema_metadata_context,
        ),
        "canonical_semantic_bucket_vocabulary": require_non_empty_string_list(
            output_schema_metadata_mapping,
            "canonical_semantic_bucket_vocabulary",
            output_schema_metadata_context,
        ),
        "required_case_report_fields": require_non_empty_string_list(
            output_schema_metadata_mapping,
            "required_case_report_fields",
            output_schema_metadata_context,
        ),
        "required_summary_artifact_fields": require_non_empty_string_list(
            output_schema_metadata_mapping,
            "required_summary_artifact_fields",
            output_schema_metadata_context,
        ),
        "required_behavior_diff_report_fields": require_non_empty_string_list(
            output_schema_metadata_mapping,
            "required_behavior_diff_report_fields",
            output_schema_metadata_context,
        ),
        "required_behavior_diff_metric_fields": require_non_empty_string_list(
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

    expected_vocabulary: list[str] = list(enum_values(SemanticBucket))
    canonical_component_vocabulary = require_enum_members_list(
        output_schema_metadata_mapping,
        "canonical_component_vocabulary",
        output_schema_metadata_context,
        SemanticBucket,
    )
    if [component.value for component in canonical_component_vocabulary] != expected_vocabulary:
        raise SeasonalityInputValidationError(
            "unexpected open closed gap leakage regression harness output_schema_metadata.canonical_component_vocabulary: "
            f"{output_schema_metadata['canonical_component_vocabulary']}",
        )

    canonical_semantic_bucket_vocabulary = require_enum_members_list(
        output_schema_metadata_mapping,
        "canonical_semantic_bucket_vocabulary",
        output_schema_metadata_context,
        SemanticBucket,
    )
    if [bucket.value for bucket in canonical_semantic_bucket_vocabulary] != expected_vocabulary:
        raise SeasonalityInputValidationError(
            "unexpected open closed gap leakage regression harness output_schema_metadata.canonical_semantic_bucket_vocabulary: "
            f"{output_schema_metadata['canonical_semantic_bucket_vocabulary']}",
        )

    return output_schema_metadata


def validate_open_closed_gap_tolerance_config(
    raw_payload: Mapping[str, Any],
    report_context: str,
) -> OpenClosedGapLeakageRegressionHarnessToleranceConfig:
    tolerance_config_mapping: Mapping[str, Any] = require_mapping(
        raw_payload,
        "tolerance_config",
        report_context,
    )

    return {
        "component_absolute_tolerance": validate_finite_non_negative_float(
            require_field(
                tolerance_config_mapping,
                "component_absolute_tolerance",
                f"{report_context}.tolerance_config",
            ),
            "tolerance_config.component_absolute_tolerance",
        ),
    }


def validate_open_closed_gap_harness_summary(
    raw_payload: Mapping[str, Any],
    report_context: str,
) -> OpenClosedGapLeakageRegressionHarnessSummary:
    summary_mapping: Mapping[str, Any] = require_mapping(
        raw_payload,
        "summary",
        report_context,
    )
    summary_context = f"{report_context}.summary"

    return {
        "total_case_count": require_non_negative_integer(
            summary_mapping,
            "total_case_count",
            summary_context,
        ),
        "passed_case_count": require_non_negative_integer(
            summary_mapping,
            "passed_case_count",
            summary_context,
        ),
        "failed_case_count": require_non_negative_integer(
            summary_mapping,
            "failed_case_count",
            summary_context,
        ),
        "total_semantic_mismatch_failure_count": require_non_negative_integer(
            summary_mapping,
            "total_semantic_mismatch_failure_count",
            summary_context,
        ),
        "scenario_class_coverage": validate_open_closed_gap_scenario_class_coverage(
            require_mapping(summary_mapping, "scenario_class_coverage", summary_context),
            f"{summary_context}.scenario_class_coverage",
        ),
    }


def validate_open_closed_gap_summary_artifact(
    raw_payload: Mapping[str, Any],
    report_context: str,
) -> OpenClosedGapBehaviorSummaryArtifact:
    summary_artifact_context = f"{report_context}.summary_artifact"
    summary_artifact_mapping: Mapping[str, Any] = require_mapping(
        raw_payload,
        "summary_artifact",
        report_context,
    )

    summary_artifact_schema_version = require_non_empty_string(
        summary_artifact_mapping,
        "schema_version",
        summary_artifact_context,
    )
    if summary_artifact_schema_version != OPEN_CLOSED_GAP_BEHAVIOR_SUMMARY_ARTIFACT_SCHEMA_VERSION:
        raise SeasonalityInputValidationError(
            "unexpected open closed gap leakage regression harness summary_artifact.schema_version: "
            f"{summary_artifact_schema_version}",
        )

    summary_artifact_determinism_policy = require_non_empty_string(
        summary_artifact_mapping,
        "determinism_policy",
        summary_artifact_context,
    )
    if summary_artifact_determinism_policy != OPEN_CLOSED_GAP_BEHAVIOR_SUMMARY_DETERMINISM_POLICY:
        raise SeasonalityInputValidationError(
            "unexpected open closed gap leakage regression harness summary_artifact.determinism_policy: "
            f"{summary_artifact_determinism_policy}",
        )

    summary_artifact_scenarios_raw: object = require_field(
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
                "fixture_id": require_non_empty_string(raw_summary_scenario, "fixture_id", scenario_context),
                "exchange_code": require_non_empty_string(raw_summary_scenario, "exchange_code", scenario_context),
                "scenario_class": validate_open_closed_gap_scenario_class(
                    require_non_empty_string(raw_summary_scenario, "scenario_class", scenario_context),
                    f"{scenario_context}.scenario_class",
                ),
                "deterministic_replay_metadata": validate_open_closed_gap_deterministic_replay_metadata(
                    require_mapping(raw_summary_scenario, "deterministic_replay_metadata", scenario_context),
                    f"{scenario_context}.deterministic_replay_metadata",
                ),
                "issue_traceability": validate_open_closed_gap_issue_traceability_metadata(
                    require_mapping(raw_summary_scenario, "issue_traceability", scenario_context),
                    f"{scenario_context}.issue_traceability",
                ),
                "contributions": validate_open_closed_gap_behavior_contribution_summary(
                    require_mapping(raw_summary_scenario, "contributions", scenario_context),
                    f"{scenario_context}.contributions",
                ),
            },
        )

    return {
        "schema_version": summary_artifact_schema_version,
        "determinism_policy": summary_artifact_determinism_policy,
        "total_case_count": require_non_negative_integer(
            summary_artifact_mapping,
            "total_case_count",
            summary_artifact_context,
        ),
        "total_open_window_event_count": require_non_negative_integer(
            summary_artifact_mapping,
            "total_open_window_event_count",
            summary_artifact_context,
        ),
        "total_closed_window_event_count": require_non_negative_integer(
            summary_artifact_mapping,
            "total_closed_window_event_count",
            summary_artifact_context,
        ),
        "scenario_class_coverage": validate_open_closed_gap_scenario_class_coverage(
            require_mapping(summary_artifact_mapping, "scenario_class_coverage", summary_artifact_context),
            f"{summary_artifact_context}.scenario_class_coverage",
        ),
        "scenarios": summary_artifact_scenarios,
    }


def validate_open_closed_gap_behavior_diff_report(
    raw_payload: Mapping[str, Any],
    report_context: str,
) -> OpenClosedGapBehaviorDiffReport:
    behavior_diff_report_context = f"{report_context}.behavior_diff_report"
    behavior_diff_report_mapping: Mapping[str, Any] = require_mapping(
        raw_payload,
        "behavior_diff_report",
        report_context,
    )

    behavior_diff_report_schema_version = require_non_empty_string(
        behavior_diff_report_mapping,
        "schema_version",
        behavior_diff_report_context,
    )
    if behavior_diff_report_schema_version != OPEN_CLOSED_GAP_BEHAVIOR_DIFF_REPORT_SCHEMA_VERSION:
        raise SeasonalityInputValidationError(
            "unexpected open closed gap leakage regression harness behavior_diff_report.schema_version: "
            f"{behavior_diff_report_schema_version}",
        )

    behavior_diff_determinism_policy = require_non_empty_string(
        behavior_diff_report_mapping,
        "determinism_policy",
        behavior_diff_report_context,
    )
    if behavior_diff_determinism_policy != OPEN_CLOSED_GAP_BEHAVIOR_DIFF_DETERMINISM_POLICY:
        raise SeasonalityInputValidationError(
            "unexpected open closed gap leakage regression harness behavior_diff_report.determinism_policy: "
            f"{behavior_diff_determinism_policy}",
        )

    behavior_diff_baseline_policy = require_non_empty_string(
        behavior_diff_report_mapping,
        "baseline_expected_output_policy",
        behavior_diff_report_context,
    )
    if behavior_diff_baseline_policy != OPEN_CLOSED_GAP_BEHAVIOR_BASELINE_EXPECTED_OUTPUT_POLICY:
        raise SeasonalityInputValidationError(
            "unexpected open closed gap leakage regression harness behavior_diff_report.baseline_expected_output_policy: "
            f"{behavior_diff_baseline_policy}",
        )

    behavior_diff_report_summary_mapping = require_mapping(
        behavior_diff_report_mapping,
        "summary",
        behavior_diff_report_context,
    )
    summary_context = f"{behavior_diff_report_context}.summary"
    behavior_diff_report_summary: OpenClosedGapBehaviorDiffReportSummary = {
        "total_case_count": require_non_negative_integer(
            behavior_diff_report_summary_mapping,
            "total_case_count",
            summary_context,
        ),
        "regression_case_count": require_non_negative_integer(
            behavior_diff_report_summary_mapping,
            "regression_case_count",
            summary_context,
        ),
        "total_regression_metric_count": require_non_negative_integer(
            behavior_diff_report_summary_mapping,
            "total_regression_metric_count",
            summary_context,
        ),
        "trading_return_regression_metric_count": require_non_negative_integer(
            behavior_diff_report_summary_mapping,
            "trading_return_regression_metric_count",
            summary_context,
        ),
        "open_jump_regression_metric_count": require_non_negative_integer(
            behavior_diff_report_summary_mapping,
            "open_jump_regression_metric_count",
            summary_context,
        ),
        "closed_gap_regression_metric_count": require_non_negative_integer(
            behavior_diff_report_summary_mapping,
            "closed_gap_regression_metric_count",
            summary_context,
        ),
        "activity_coupling_regression_metric_count": require_non_negative_integer(
            behavior_diff_report_summary_mapping,
            "activity_coupling_regression_metric_count",
            summary_context,
        ),
    }

    behavior_diff_scenarios_raw: object = require_field(
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
            validate_open_closed_gap_behavior_scenario_diff(raw_scenario_diff, scenario_context),
        )

    return {
        "schema_version": behavior_diff_report_schema_version,
        "determinism_policy": behavior_diff_determinism_policy,
        "baseline_expected_output_policy": behavior_diff_baseline_policy,
        "summary": behavior_diff_report_summary,
        "scenario_diffs": behavior_diff_scenarios,
    }


def validate_open_closed_gap_case_reports(
    raw_payload: Mapping[str, Any],
    report_context: str,
) -> list[OpenClosedGapLeakageRegressionCaseReport]:
    case_reports_raw: object = require_field(
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
        case_reports.append(validate_open_closed_gap_case_report(raw_case_report, case_context))

    return case_reports


def validate_open_closed_gap_case_report(
    raw_case_report: object,
    case_context: str,
) -> OpenClosedGapLeakageRegressionCaseReport:
    if not isinstance(raw_case_report, Mapping):
        raise SeasonalityInputValidationError(f"{case_context} must be a mapping")

    status = require_enum_member(
        raw_case_report,
        "status",
        case_context,
        RegressionStatus,
        error_message=f"{case_context}.status has unsupported value: {{value}}",
    )
    failures = validate_open_closed_gap_case_failures(raw_case_report, case_context)

    return {
        "fixture_id": require_non_empty_string(raw_case_report, "fixture_id", case_context),
        "exchange_code": require_non_empty_string(raw_case_report, "exchange_code", case_context),
        "scenario_class": validate_open_closed_gap_scenario_class(
            require_non_empty_string(raw_case_report, "scenario_class", case_context),
            f"{case_context}.scenario_class",
        ),
        "deterministic_replay_metadata": validate_open_closed_gap_deterministic_replay_metadata(
            require_mapping(raw_case_report, "deterministic_replay_metadata", case_context),
            f"{case_context}.deterministic_replay_metadata",
        ),
        "issue_traceability": validate_open_closed_gap_issue_traceability_metadata(
            require_mapping(raw_case_report, "issue_traceability", case_context),
            f"{case_context}.issue_traceability",
        ),
        "status": status.value,
        "summary": require_non_empty_string(raw_case_report, "summary", case_context),
        "semantic_mismatch_failure_count": require_non_negative_integer(
            raw_case_report,
            "semantic_mismatch_failure_count",
            case_context,
        ),
        "max_absolute_mismatch": validate_finite_non_negative_float(
            require_field(raw_case_report, "max_absolute_mismatch", case_context),
            f"{case_context}.max_absolute_mismatch",
        ),
        "contribution_summary": validate_open_closed_gap_behavior_contribution_summary(
            require_mapping(raw_case_report, "contribution_summary", case_context),
            f"{case_context}.contribution_summary",
        ),
        "behavior_diff": validate_open_closed_gap_behavior_scenario_diff(
            require_mapping(raw_case_report, "behavior_diff", case_context),
            f"{case_context}.behavior_diff",
        ),
        "failures": failures,
    }


def validate_open_closed_gap_case_failures(
    raw_case_report: Mapping[str, Any],
    case_context: str,
) -> list[OpenClosedGapLeakageRegressionFailure]:
    failures_raw: object = require_field(raw_case_report, "failures", case_context)
    if not isinstance(failures_raw, list):
        raise SeasonalityInputValidationError(f"{case_context}.failures must be a list")

    failures: list[OpenClosedGapLeakageRegressionFailure] = []
    for failure_position, raw_failure in enumerate(failures_raw):
        failure_context = f"{case_context}.failures[{failure_position}]"
        failures.append(validate_open_closed_gap_case_failure(raw_failure, failure_context))

    return failures


def validate_open_closed_gap_case_failure(
    raw_failure: object,
    failure_context: str,
) -> OpenClosedGapLeakageRegressionFailure:
    if not isinstance(raw_failure, Mapping):
        raise SeasonalityInputValidationError(f"{failure_context} must be a mapping")

    failure_kind = require_enum_member(
        raw_failure,
        "failure_kind",
        failure_context,
        OpenClosedGapFailureKind,
        error_message=f"{failure_context}.failure_kind has unsupported value: {{value}}",
    )
    match failure_kind:
        case OpenClosedGapFailureKind.DISPATCH_ERROR:
            return {
                "failure_kind": failure_kind.value,
                "semantic_bucket": None,
                "mismatch_kind": None,
                "event_id": None,
                "event_timestamp_utc": None,
                "expected_value": None,
                "observed_value": None,
                "absolute_difference": None,
                "error_message": require_non_empty_string(
                    raw_failure,
                    "error_message",
                    failure_context,
                ),
            }
        case OpenClosedGapFailureKind.SEMANTIC_MISMATCH:
            return {
                "failure_kind": failure_kind.value,
                "semantic_bucket": validate_open_closed_gap_semantic_bucket(
                    require_non_empty_string(raw_failure, "semantic_bucket", failure_context),
                    f"{failure_context}.semantic_bucket",
                ),
                "mismatch_kind": require_non_empty_string(raw_failure, "mismatch_kind", failure_context),
                "event_id": require_non_empty_string(raw_failure, "event_id", failure_context),
                "event_timestamp_utc": require_non_empty_string(
                    raw_failure,
                    "event_timestamp_utc",
                    failure_context,
                ),
                "expected_value": validate_finite_float(
                    require_field(raw_failure, "expected_value", failure_context),
                    f"{failure_context}.expected_value",
                ),
                "observed_value": validate_finite_float(
                    require_field(raw_failure, "observed_value", failure_context),
                    f"{failure_context}.observed_value",
                ),
                "absolute_difference": validate_finite_non_negative_float(
                    require_field(raw_failure, "absolute_difference", failure_context),
                    f"{failure_context}.absolute_difference",
                ),
                "error_message": None,
            }


def validate_open_closed_gap_scenario_class_coverage(
    raw_coverage: Mapping[str, Any],
    context: str,
) -> OpenClosedGapScenarioClassCoverage:
    return {
        "known_only_case_count": require_non_negative_integer(
            raw_coverage,
            "known_only_case_count",
            context,
        ),
        "surprise_only_case_count": require_non_negative_integer(
            raw_coverage,
            "surprise_only_case_count",
            context,
        ),
        "mixed_news_case_count": require_non_negative_integer(
            raw_coverage,
            "mixed_news_case_count",
            context,
        ),
    }


def validate_open_closed_gap_scenario_class(value: str, context: str) -> str:
    return parse_enum_member(
        value,
        OpenClosedGapScenarioClass,
        context,
        error_message=f"{context} must be one of 'known_only', 'surprise_only', or 'mixed_news'",
    ).value


def validate_open_closed_gap_semantic_bucket(value: str, context: str) -> str:
    return parse_enum_member(
        value,
        SemanticBucket,
        context,
        error_message=f"{context} must be one of 'trading_return', 'open_jump', 'closed_gap', or 'activity_coupling'",
    ).value


def validate_open_closed_gap_behavior_contribution_summary(
    raw_contribution_summary: Mapping[str, Any],
    context: str,
) -> OpenClosedGapBehaviorContributionSummary:
    return {
        "open_window_event_count": require_non_negative_integer(
            raw_contribution_summary,
            "open_window_event_count",
            context,
        ),
        "closed_window_event_count": require_non_negative_integer(
            raw_contribution_summary,
            "closed_window_event_count",
            context,
        ),
        "expected_activity_coupling_total": validate_finite_float(
            require_field(raw_contribution_summary, "expected_activity_coupling_total", context),
            f"{context}.expected_activity_coupling_total",
        ),
        "observed_activity_coupling_total": validate_finite_float(
            require_field(raw_contribution_summary, "observed_activity_coupling_total", context),
            f"{context}.observed_activity_coupling_total",
        ),
        "expected_trading_return_total": validate_finite_float(
            require_field(raw_contribution_summary, "expected_trading_return_total", context),
            f"{context}.expected_trading_return_total",
        ),
        "observed_trading_return_total": validate_finite_float(
            require_field(raw_contribution_summary, "observed_trading_return_total", context),
            f"{context}.observed_trading_return_total",
        ),
        "expected_open_jump_total": validate_finite_float(
            require_field(raw_contribution_summary, "expected_open_jump_total", context),
            f"{context}.expected_open_jump_total",
        ),
        "observed_open_jump_total": validate_finite_float(
            require_field(raw_contribution_summary, "observed_open_jump_total", context),
            f"{context}.observed_open_jump_total",
        ),
        "expected_closed_gap_total": validate_finite_float(
            require_field(raw_contribution_summary, "expected_closed_gap_total", context),
            f"{context}.expected_closed_gap_total",
        ),
        "observed_closed_gap_total": validate_finite_float(
            require_field(raw_contribution_summary, "observed_closed_gap_total", context),
            f"{context}.observed_closed_gap_total",
        ),
    }


def validate_open_closed_gap_behavior_scenario_diff(
    raw_scenario_diff: object,
    context: str,
) -> OpenClosedGapBehaviorScenarioDiff:
    if not isinstance(raw_scenario_diff, Mapping):
        raise SeasonalityInputValidationError(f"{context} must be a mapping")

    metrics_raw: object = require_field(raw_scenario_diff, "metrics", context)
    if not isinstance(metrics_raw, list):
        raise SeasonalityInputValidationError(f"{context}.metrics must be a list")

    metrics: list[OpenClosedGapBehaviorDiffMetric] = []
    for metric_position, raw_metric in enumerate(metrics_raw):
        metric_context = f"{context}.metrics[{metric_position}]"
        if not isinstance(raw_metric, Mapping):
            raise SeasonalityInputValidationError(f"{metric_context} must be a mapping")

        metrics.append(
            {
                "metric_id": require_non_empty_string(raw_metric, "metric_id", metric_context),
                "semantic_bucket": validate_open_closed_gap_semantic_bucket(
                    require_non_empty_string(raw_metric, "semantic_bucket", metric_context),
                    f"{metric_context}.semantic_bucket",
                ),
                "expected_value": validate_finite_float(
                    require_field(raw_metric, "expected_value", metric_context),
                    f"{metric_context}.expected_value",
                ),
                "observed_value": validate_finite_float(
                    require_field(raw_metric, "observed_value", metric_context),
                    f"{metric_context}.observed_value",
                ),
                "absolute_difference": validate_finite_non_negative_float(
                    require_field(raw_metric, "absolute_difference", metric_context),
                    f"{metric_context}.absolute_difference",
                ),
                "absolute_tolerance": validate_finite_non_negative_float(
                    require_field(raw_metric, "absolute_tolerance", metric_context),
                    f"{metric_context}.absolute_tolerance",
                ),
                "regression_detected": require_boolean(raw_metric, "regression_detected", metric_context),
            },
        )

    return {
        "fixture_id": require_non_empty_string(raw_scenario_diff, "fixture_id", context),
        "exchange_code": require_non_empty_string(raw_scenario_diff, "exchange_code", context),
        "regression_metric_count": require_non_negative_integer(
            raw_scenario_diff,
            "regression_metric_count",
            context,
        ),
        "max_absolute_difference": validate_finite_non_negative_float(
            require_field(raw_scenario_diff, "max_absolute_difference", context),
            f"{context}.max_absolute_difference",
        ),
        "metrics": metrics,
    }
