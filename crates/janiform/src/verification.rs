//! Top-level re-exports for analytics, validation, and governance.

pub use janiform_analytics_stylized_facts::{
    NewsSemanticsScenarioClass, OpenClosedGapDeterministicReplayMetadata,
    OpenClosedGapIssueTraceabilityMetadata, StylizedFactHorizonComparisonReportGenerationError,
    StylizedFactHorizonComparisonReportGeneratorRequest,
    run_stylized_fact_horizon_comparison_report_generator,
};
pub use janiform_governance::{
    CalendarStateRegressionCase, HourlyFactorRegressionCase, HourlyNewsIntensityRegressionSource,
    HourlySeasonalityRegressionSource, RegressionHarnessError, RegressionHarnessToleranceConfig,
    run_deterministic_regression_harness,
};
pub use janiform_validation::{
    OPEN_CLOSED_GAP_LEAKAGE_REGRESSION_HARNESS_REPORT_SCHEMA_VERSION,
    OpenClosedGapLeakageRegressionCase, OpenClosedGapLeakageRegressionEventInput,
    OpenClosedGapLeakageRegressionHarnessError,
    OpenClosedGapLeakageRegressionHarnessToleranceConfig, RegressionCaseStatus,
    SCALE_CONSISTENCY_METRIC_DELTA_BATCH_REPORT_SCHEMA_VERSION,
    SCALE_CONSISTENCY_METRIC_DELTA_ORDERING_POLICY, ScaleConsistencyDecisionReasonCode,
    ScaleConsistencyDriftCategory, ScaleConsistencyEngineBenchmarkInput,
    ScaleConsistencyMetricCategory, ScaleConsistencyMetricComputationFailure,
    ScaleConsistencyMetricComputationStatus, ScaleConsistencyMetricDatasetRole,
    ScaleConsistencyMetricDelta, ScaleConsistencyMetricDeltaBatchReport,
    ScaleConsistencyMetricDeltaKind, ScaleConsistencyMetricDeltaScenarioReport,
    ScaleConsistencyMetricFailureReasonCode, run_open_closed_gap_leakage_regression_harness,
};
