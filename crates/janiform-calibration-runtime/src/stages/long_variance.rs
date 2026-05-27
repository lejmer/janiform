pub(crate) mod estimation;
pub(crate) mod metrics;
pub(crate) mod policy;
mod stage;

pub(crate) use stage::{
    LONG_VARIANCE_ACTIVE_PARAMETER_COUNT, LONG_VARIANCE_CANDIDATE_GEOMETRY_COUNT,
    LONG_VARIANCE_OBJECTIVE_TERM_COUNT,
};
pub use stage::{
    LongVarianceStageExecutionFailure, LongVarianceStageExecutionInput,
    LongVarianceStageExecutionMetadata, LongVarianceStageExecutionReport,
    LongVarianceStageFittedPayload, LongVarianceStageReturnComponentsInput,
    run_long_variance_calibration_stage,
};
