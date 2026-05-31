//! Shared simulation workflow orchestration.

pub mod artifacts;
pub mod direct;
mod execution;
mod intervals;
mod labels;
pub mod model_artifact;
mod output;
pub mod output_preferences;
mod progress;
mod request;
mod run;
mod workflow;

pub use artifacts::{
    DEFAULT_SIMULATION_EXPORT_BATCH_SIZE, SimulationExportSummary, SimulationOutputColumns,
    export_coarse_simulation, export_seeded_tick_simulation,
};
pub use direct::{
    CoarseIntervalExportSettings, DirectSimulationCompleted, DirectSimulationCompletion,
    DirectSimulationJob, SeededTickExportSettings,
};
pub use execution::{
    run_coarse_simulation_workflow, run_direct_simulation_workflow,
    run_seeded_tick_event_simulation_workflow, run_simulation_from_model_artifact_workflow,
};
pub use labels::{
    direct_simulation_scope_label, model_artifact_simulation_scope_label, simulation_workflow_label,
};
pub use model_artifact::run_simulation_from_model_artifact;
pub use output::{DirectSimulationWorkflowOutput, ModelArtifactSimulationWorkflowOutput};
pub use output_preferences::{
    SimulationOutputPreferences, simulation_output_preferences_from_model_payload,
};
pub use request::{
    CoarseSimulationWorkflowRequest, DirectSimulationWorkflowRequest,
    ModelArtifactSimulationContinuationData, ModelArtifactSimulationKind,
    ModelArtifactSimulationWorkflowRequest, SeededTickEventSimulationWorkflowRequest,
};
pub use run::{
    run_coarse_simulation_with_terminal_progress,
    run_seeded_tick_event_simulation_with_terminal_progress,
    run_simulation_from_model_artifact_with_terminal_progress,
};
pub use workflow::SimulationWorkflowObserver;
