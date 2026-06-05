mod activity;
mod contracts;
mod execution;

pub(crate) use activity::short_gap_ratio_from_threshold;
pub use contracts::{
    HawkesStageCandidateDiagnostic, HawkesStageCandidateInput, HawkesStageCandidateInputError,
    HawkesStageDirectionalLabelDiagnostics, HawkesStageDirectionalLabelDiagnosticsError,
    HawkesStageExecutionFailure, HawkesStageExecutionInput, HawkesStageExecutionMetadata,
    HawkesStageExecutionReport, HawkesStageFittedPayload, HawkesStageHandoffOutput,
    HawkesStageImpliedMetricsDiagnostic, HawkesStageObjectiveTermDiagnostic,
    HawkesStageObservedSourceRegime, HawkesStageObservedSourceRegimeError,
    HawkesStageSeasonalityInput,
};
pub use execution::run_hawkes_calibration_stage;
pub(crate) use execution::stationary_open_event_rate_per_hour;
