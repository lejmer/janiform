mod activity;
mod bit_flags;
mod clock;
mod contracts;
mod execution;
mod likelihood;
mod likelihood_objective;
mod progress;
mod seasonality_input;
mod selection;

pub(crate) use activity::short_gap_ratio_from_threshold;
pub(crate) use clock::hawkes_open_events_are_adjacent;
pub use contracts::{
    HAWKES_EVENT_SIDE_CODE_BUY, HAWKES_EVENT_SIDE_CODE_SELL, HawkesStageCandidateDiagnostic,
    HawkesStageCandidateInput, HawkesStageCandidateInputError,
    HawkesStageDirectionalLabelDiagnostics, HawkesStageDirectionalLabelDiagnosticsError,
    HawkesStageExecutionFailure, HawkesStageExecutionInput, HawkesStageExecutionMetadata,
    HawkesStageExecutionReport, HawkesStageFittedPayload, HawkesStageHandoffOutput,
    HawkesStageImpliedMetricsDiagnostic, HawkesStageObjectiveTermDiagnostic,
    HawkesStageObservedSourceRegime, HawkesStageObservedSourceRegimeError,
    HawkesStageProgressPhase, HawkesStageProgressUpdate, HawkesStageSeasonalityInput,
};
pub use execution::run_hawkes_calibration_stage;
pub(crate) use execution::run_hawkes_calibration_stage_with_progress_observer;
pub(crate) use execution::stationary_open_event_rate_per_hour;
