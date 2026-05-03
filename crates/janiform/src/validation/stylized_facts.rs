//! Re-exports for stylized-fact validation reports.

pub use janiform_validation::{
    StylizedFactCalibrationRunInput, StylizedFactComparisonDecisionState,
    StylizedFactDecisionReasonCode, StylizedFactHorizonComparisonArtifactExportConfig,
    StylizedFactHorizonComparisonReportBundle, StylizedFactHorizonComparisonReportGenerationError,
    StylizedFactHorizonComparisonReportGeneratorOptions,
    StylizedFactHorizonComparisonReportGeneratorRequest, StylizedFactHorizonComparisonRequest,
    StylizedFactHorizonComparisonTableSummaryRow, StylizedFactHorizonComparisonThresholdPolicy,
    StylizedFactMetricCatalog, StylizedFactMetricClass, StylizedFactMetricConfidenceInterval,
    StylizedFactMetricSampleCoverage, StylizedFactMetricThresholdOverride,
    StylizedFactObservedCounts, StylizedFactObservedMetricInput,
    StylizedFactSampleSufficiencyRequirements, build_stylized_fact_metric_catalog,
    run_stylized_fact_horizon_comparison_report_generator,
};
