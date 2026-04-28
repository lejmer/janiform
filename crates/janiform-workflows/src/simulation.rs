//! Shared simulation workflow orchestration.

pub mod artifacts;
pub mod direct;
pub mod model_artifact;
pub mod output_preferences;

pub use artifacts::{
    DEFAULT_SIMULATION_EXPORT_BATCH_SIZE, SimulationExportSummary, SimulationOutputColumns,
    export_coarse_simulation, export_seeded_tick_simulation,
};
pub use direct::{
    CoarseIntervalExportSettings, DirectSimulationCompleted, DirectSimulationCompletion,
    DirectSimulationJob, SeededTickExportSettings,
};
pub use model_artifact::{
    ModelArtifactSimulationContinuationData, ModelArtifactSimulationKind,
    ModelArtifactSimulationWorkflowOutput, ModelArtifactSimulationWorkflowRequest,
    run_simulation_from_model_artifact,
};
pub use output_preferences::{
    SimulationOutputPreferences, simulation_output_preferences_from_model_payload,
};
