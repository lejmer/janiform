mod contracts;
mod execution;

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
