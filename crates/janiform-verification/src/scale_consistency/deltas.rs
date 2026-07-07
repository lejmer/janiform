mod calculator;
mod contracts;
mod resolution;
mod statistics;

pub use calculator::calculate_scale_consistency_metric_deltas;
pub use contracts::{
    SCALE_CONSISTENCY_METRIC_DELTA_BATCH_REPORT_SCHEMA_VERSION,
    SCALE_CONSISTENCY_METRIC_DELTA_ORDERING_POLICY, ScaleConsistencyMetricCategory,
    ScaleConsistencyMetricComputationFailure, ScaleConsistencyMetricComputationStatus,
    ScaleConsistencyMetricDatasetRole, ScaleConsistencyMetricDelta,
    ScaleConsistencyMetricDeltaBatchError, ScaleConsistencyMetricDeltaBatchReport,
    ScaleConsistencyMetricDeltaBatchRequest, ScaleConsistencyMetricDeltaKind,
    ScaleConsistencyMetricDeltaScenarioReport, ScaleConsistencyMetricFailureReasonCode,
    ScaleConsistencyMetricScenarioInput, ScaleConsistencyMetricScenarioInputError,
    ScaleConsistencyMetricSpecification, ScaleConsistencyMetricSpecificationError,
};
pub(crate) use resolution::is_supported_scale_consistency_metric;
