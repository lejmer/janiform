mod contracts;
mod execution;
mod mean_reversion_runtime;
mod objective;
mod option_surface_tiering;
mod restart_sensitivity;
mod setup;
mod summary_simulation;
mod tick_shape_proxy;
mod worker_runtime;

pub use contracts::{
    JointRefinementProgressCandidateAssessment, JointRefinementStageCandidateDiagnostic,
    JointRefinementStageCoarseTickConsistencyMetric,
    JointRefinementStageCurrentStateEstimationInput, JointRefinementStageExecutionFailure,
    JointRefinementStageExecutionInput, JointRefinementStageExecutionMetadata,
    JointRefinementStageExecutionReport, JointRefinementStageFittedPayload,
    JointRefinementStageHawkesInput, JointRefinementStageMeanReversionInput,
    JointRefinementStageObjectiveTermDiagnostic, JointRefinementStageOptionPricingInput,
    JointRefinementStageOptionSnapshotNode, JointRefinementStageProgressPhase,
    JointRefinementStageProgressUpdate, JointRefinementStageReturnComponentsInput,
    JointRefinementStageSeasonalityFactorsInput, JointRefinementStageSimulationSummary,
};
pub use execution::JointRefinementSeedSummary;
pub use execution::run_joint_refinement_calibration_stage;
pub(crate) use execution::run_joint_refinement_calibration_stage_with_progress_observer_and_persistence;
pub use mean_reversion_runtime::endogenous_interval_factors_from_hourly_contract;
pub(crate) use setup::build_surprise_multipliers_for_interval_grid;
pub use summary_simulation::{
    CloseSessionRequest, CloseSessionResponse, InitializeSessionRequest, InitializeSessionResponse,
    JOINT_REFINEMENT_SUMMARY_SIMULATION_WORKER_PROTOCOL_SCHEMA_VERSION,
    JointRefinementSummarySimulationDiagnostics, JointRefinementSummarySimulationMemoryEstimate,
    JointRefinementSummarySimulationSeedSummary, JointRefinementSummarySimulationSessionContext,
    JointRefinementSummarySimulationSessionState, JointRefinementSummarySimulationWorkerRequest,
    JointRefinementSummarySimulationWorkerResponse, JointRefinementWorkerRequestedOutput,
    RunCandidateRequest, RunCandidateResponse,
    initialize_joint_refinement_summary_simulation_session, read_joint_refinement_worker_frame,
    run_joint_refinement_candidate_in_session, simulate_joint_refinement_seed_summary_in_process,
    write_joint_refinement_worker_frame,
};
pub use worker_runtime::current_process_resident_bytes;
