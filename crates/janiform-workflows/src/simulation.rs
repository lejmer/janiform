//! Shared simulation workflow orchestration.

pub mod artifacts;
pub mod direct;
pub mod model;
pub mod output_preferences;

pub use artifacts::{
    DEFAULT_SIMULATION_EXPORT_BATCH_SIZE, SimulationExportSummary, SimulationOutputColumns,
    export_coarse_simulation, export_seeded_tick_simulation,
};
pub use direct::{
    CoarseIntervalExportSettings, DirectSimulationCompleted, DirectSimulationCompletion,
    DirectSimulationJob, SeededTickExportSettings,
};
pub use model::{
    ModelSimulationContinuationData, ModelSimulationKind, ModelSimulationWorkflowOutput,
    ModelSimulationWorkflowRequest, run_model_simulation_workflow,
};
pub use output_preferences::{
    SimulationOutputPreferences, simulation_output_preferences_from_model_payload,
};
