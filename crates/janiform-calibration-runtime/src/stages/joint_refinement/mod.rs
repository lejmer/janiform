mod contracts;
mod execution;
mod objective;
mod setup;

pub use contracts::{
    JointRefinementStageCandidateDiagnostic, JointRefinementStageCoarseTickConsistencyMetric,
    JointRefinementStageExecutionFailure, JointRefinementStageExecutionInput,
    JointRefinementStageExecutionMetadata, JointRefinementStageExecutionReport,
    JointRefinementStageFittedPayload, JointRefinementStageHawkesInput,
    JointRefinementStageObjectiveTermDiagnostic, JointRefinementStageReturnComponentsInput,
    JointRefinementStageSeasonalityFactorsInput, JointRefinementStageSimulationSummary,
};
pub use execution::run_joint_refinement_calibration_stage;
pub(crate) use setup::build_surprise_multipliers_for_interval_grid;
