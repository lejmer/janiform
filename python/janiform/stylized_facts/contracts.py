"""Public stylized-fact reporting contracts."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Optional

from ..contracts import ContractModel


@dataclass(frozen=True, slots=True)
class StylizedFactMetricIntervalBounds(ContractModel):
    lower_bound: float
    upper_bound: float
    confidence_level: float


@dataclass(frozen=True, slots=True)
class StylizedFactMetricCoverageSummary(ContractModel):
    horizon_interval_count: int
    covered_interval_count: int
    sample_count: int
    effective_sample_count: int
    tail_observation_count: int
    minimum_sample_count: int
    minimum_effective_sample_count: int
    minimum_tail_observation_count: int
    minimum_horizon_coverage_ratio_scaled_bps: int


@dataclass(frozen=True, slots=True)
class StylizedFactMetricInsufficientDataReasons(ContractModel):
    empirical: list[str]
    simulated: list[str]


@dataclass(frozen=True, slots=True)
class StylizedFactDecisionCounts(ContractModel):
    pass_count: int
    fail_count: int
    insufficient_data_count: int


@dataclass(frozen=True, slots=True)
class StylizedFactDatasetMetricSampleCoverage(ContractModel):
    horizon_interval_count: int
    covered_interval_count: int


@dataclass(frozen=True, slots=True)
class StylizedFactDatasetMetricObservedCounts(ContractModel):
    sample_count: int
    effective_sample_count: int
    tail_observation_count: int


@dataclass(frozen=True, slots=True)
class StylizedFactDatasetMetricRequiredMinimums(ContractModel):
    minimum_sample_count: int
    minimum_effective_sample_count: int
    minimum_tail_observation_count: int
    minimum_horizon_coverage_ratio: float


@dataclass(frozen=True, slots=True)
class StylizedFactDatasetMetricReport(ContractModel):
    status: str
    value: Optional[float]
    confidence_interval: Optional[StylizedFactMetricIntervalBounds]
    sample_coverage: StylizedFactDatasetMetricSampleCoverage
    observed_counts: StylizedFactDatasetMetricObservedCounts
    required_minimums: StylizedFactDatasetMetricRequiredMinimums
    horizon_coverage_ratio: float
    insufficiency_reason_codes: list[str]
    input_present: bool


@dataclass(frozen=True, slots=True)
class StylizedFactThresholdContext(ContractModel):
    policy_id: str
    max_absolute_delta: float
    max_standardized_delta: float
    absolute_delta_exceeds_threshold: Optional[bool]
    standardized_delta_exceeds_threshold: Optional[bool]


@dataclass(frozen=True, slots=True)
class StylizedFactUncertaintyContext(ContractModel):
    empirical_interval_half_width: Optional[float]
    simulated_interval_half_width: Optional[float]
    pooled_interval_half_width: Optional[float]
    standardized_delta: Optional[float]


@dataclass(frozen=True, slots=True)
class StylizedFactMetricComparisonReport(ContractModel):
    metric_id: str
    metric_name: str
    metric_variant: Optional[str]
    alignment_status: str
    status: str
    empirical: StylizedFactDatasetMetricReport
    simulated: StylizedFactDatasetMetricReport
    absolute_delta: Optional[float]
    uncertainty_context: StylizedFactUncertaintyContext
    threshold_context: StylizedFactThresholdContext
    horizon_label: str
    decision_reason_codes: list[str]


@dataclass(frozen=True, slots=True)
class StylizedFactHorizonComparisonHorizonReport(ContractModel):
    horizon_label: str
    empirical_horizon_duration_nanos: int
    simulated_horizon_duration_nanos: int
    alignment_status: str
    status: str
    decision_counts: StylizedFactDecisionCounts
    metrics: list[StylizedFactMetricComparisonReport]


@dataclass(frozen=True, slots=True)
class StylizedFactHorizonComparisonRunReport(ContractModel):
    calibration_run_id: str
    empirical_dataset_id: str
    simulated_dataset_id: str
    status: str
    decision_counts: StylizedFactDecisionCounts
    total_horizon_count: int
    total_metric_count: int
    horizons: list[StylizedFactHorizonComparisonHorizonReport]


@dataclass(frozen=True, slots=True)
class StylizedFactHorizonComparisonReportSummary(ContractModel):
    total_run_count: int
    total_metric_count: int
    run_decision_counts: StylizedFactDecisionCounts
    metric_decision_counts: StylizedFactDecisionCounts


@dataclass(frozen=True, slots=True)
class StylizedFactHorizonComparisonOutputSchemaMetadata(ContractModel):
    schema_version: str
    required_batch_report_fields: list[str]
    required_run_report_fields: list[str]
    required_horizon_report_fields: list[str]
    required_metric_report_fields: list[str]
    required_dataset_metric_fields: list[str]
    required_decision_count_fields: list[str]
    decision_state_vocabulary: list[str]
    horizon_alignment_status_vocabulary: list[str]
    metric_alignment_status_vocabulary: list[str]
    dataset_metric_status_vocabulary: list[str]
    decision_reason_vocabulary: list[str]
    sample_sufficiency_reason_vocabulary: list[str]


@dataclass(frozen=True, slots=True)
class StylizedFactHorizonComparisonReport(ContractModel):
    schema_version: str
    decision_policy: str
    uncertainty_policy: str
    insufficient_data_status_code: str
    output_schema_metadata: StylizedFactHorizonComparisonOutputSchemaMetadata
    metric_catalog_id: str
    metric_catalog_version: str
    threshold_policy_id: str
    status: str
    summary: StylizedFactHorizonComparisonReportSummary
    run_reports: list[StylizedFactHorizonComparisonRunReport]


@dataclass(frozen=True, slots=True)
class StylizedFactHorizonComparisonTableSummaryRow(ContractModel):
    calibration_run_id: str
    empirical_dataset_id: str
    simulated_dataset_id: str
    horizon_label: str
    metric_id: str
    metric_name: str
    metric_variant: Optional[str]
    metric_class: str
    alignment_status: str
    status: str
    absolute_delta: Optional[float]
    standardized_delta: Optional[float]
    empirical_status: str
    simulated_status: str
    empirical_value: Optional[float]
    simulated_value: Optional[float]
    empirical_confidence_interval: Optional[StylizedFactMetricIntervalBounds]
    simulated_confidence_interval: Optional[StylizedFactMetricIntervalBounds]
    empirical_sample_coverage: StylizedFactMetricCoverageSummary
    simulated_sample_coverage: StylizedFactMetricCoverageSummary
    empirical_horizon_coverage_ratio: float
    simulated_horizon_coverage_ratio: float
    insufficient_data_reasons: StylizedFactMetricInsufficientDataReasons
    decision_reason_codes: list[str]


@dataclass(frozen=True, slots=True)
class StylizedFactHorizonComparisonTableSummary(ContractModel):
    schema_version: str
    determinism_policy: str
    rows: list[StylizedFactHorizonComparisonTableSummaryRow]


@dataclass(frozen=True, slots=True)
class StylizedFactHorizonComparisonReportBundleOutputSchemaMetadata(ContractModel):
    schema_version: str
    required_bundle_fields: list[str]
    required_table_summary_fields: list[str]
    required_table_summary_row_fields: list[str]
    decision_state_vocabulary: list[str]
    metric_class_vocabulary: list[str]
    insufficient_data_status_code: str


@dataclass(frozen=True, slots=True)
class StylizedFactHorizonComparisonExportedArtifactFile(ContractModel):
    artifact_kind: str
    file_name: str
    path: str


@dataclass(frozen=True, slots=True)
class StylizedFactHorizonComparisonArtifactExportReceipt(ContractModel):
    export_directory: str
    file_stem: str
    exported_files: list[StylizedFactHorizonComparisonExportedArtifactFile]


@dataclass(frozen=True, slots=True)
class StylizedFactHorizonComparisonReportBundle(ContractModel):
    schema_version: str
    determinism_policy: str
    output_schema_metadata: StylizedFactHorizonComparisonReportBundleOutputSchemaMetadata
    comparison_report: StylizedFactHorizonComparisonReport
    table_summary: StylizedFactHorizonComparisonTableSummary
    artifact_export: Optional[StylizedFactHorizonComparisonArtifactExportReceipt]


__all__ = [
    "StylizedFactDatasetMetricObservedCounts",
    "StylizedFactDatasetMetricReport",
    "StylizedFactDatasetMetricRequiredMinimums",
    "StylizedFactDatasetMetricSampleCoverage",
    "StylizedFactDecisionCounts",
    "StylizedFactHorizonComparisonArtifactExportReceipt",
    "StylizedFactHorizonComparisonExportedArtifactFile",
    "StylizedFactHorizonComparisonHorizonReport",
    "StylizedFactHorizonComparisonOutputSchemaMetadata",
    "StylizedFactHorizonComparisonReport",
    "StylizedFactHorizonComparisonReportBundle",
    "StylizedFactHorizonComparisonReportBundleOutputSchemaMetadata",
    "StylizedFactHorizonComparisonReportSummary",
    "StylizedFactHorizonComparisonRunReport",
    "StylizedFactHorizonComparisonTableSummary",
    "StylizedFactHorizonComparisonTableSummaryRow",
    "StylizedFactMetricCoverageSummary",
    "StylizedFactMetricComparisonReport",
    "StylizedFactMetricInsufficientDataReasons",
    "StylizedFactMetricIntervalBounds",
    "StylizedFactThresholdContext",
    "StylizedFactUncertaintyContext",
]
