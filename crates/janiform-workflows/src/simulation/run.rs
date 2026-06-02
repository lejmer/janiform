use janiform::simulation::{
    CoarseSimulationPath, CoarseSimulationRunRequest, SeededTickSimulationRunConfig,
    SeededTickSimulationRunResult,
};
use janiform_foundations::cancellation::CancellationToken;

use crate::error::WorkflowError;
use crate::terminal::{TerminalProgressMode, stderr_progress_adapter, terminal_summary_policy};

use super::execution::{
    run_coarse_simulation_workflow, run_seeded_tick_event_simulation_workflow,
    run_simulation_from_model_artifact_workflow,
};
use super::output::SimulationWorkflowOutput;
use super::request::ModelArtifactSimulationWorkflowRequest;
use super::workflow::SimulationWorkflowObserver;

/// Run simulation from a model artifact with shared terminal progress.
pub fn run_simulation_from_model_artifact_with_terminal_progress(
    request: ModelArtifactSimulationWorkflowRequest,
    cancellation_token: CancellationToken,
    progress_mode: TerminalProgressMode,
) -> Result<SimulationWorkflowOutput, WorkflowError> {
    let mut progress_adapter = stderr_progress_adapter(progress_mode);

    run_simulation_from_model_artifact_workflow(
        request,
        SimulationWorkflowObserver {
            cancellation_token,
            progress_adapter: progress_adapter.as_mut(),
            terminal_output: terminal_summary_policy(progress_mode),
        },
    )
}

/// Run coarse simulation with shared terminal progress.
pub fn run_coarse_simulation_with_terminal_progress(
    request: CoarseSimulationRunRequest,
    cancellation_token: CancellationToken,
    progress_mode: TerminalProgressMode,
) -> Result<CoarseSimulationPath, WorkflowError> {
    let mut progress_adapter = stderr_progress_adapter(progress_mode);

    run_coarse_simulation_workflow(
        request,
        SimulationWorkflowObserver {
            cancellation_token,
            progress_adapter: progress_adapter.as_mut(),
            terminal_output: terminal_summary_policy(progress_mode),
        },
    )
}

/// Run seeded tick-event simulation with shared terminal progress.
pub fn run_seeded_tick_event_simulation_with_terminal_progress(
    run_config: SeededTickSimulationRunConfig,
    cancellation_token: CancellationToken,
    progress_mode: TerminalProgressMode,
) -> Result<SeededTickSimulationRunResult, WorkflowError> {
    let mut progress_adapter = stderr_progress_adapter(progress_mode);

    run_seeded_tick_event_simulation_workflow(
        run_config,
        SimulationWorkflowObserver {
            cancellation_token,
            progress_adapter: progress_adapter.as_mut(),
            terminal_output: terminal_summary_policy(progress_mode),
        },
    )
}
