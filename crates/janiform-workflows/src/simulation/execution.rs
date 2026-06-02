use std::time::Instant;

use janiform::simulation::{
    CoarseSimulationPath, CoarseSimulationRunError, SeededTickSimulationRunError,
    SeededTickSimulationRunResult, SimulationProgressEvent, SimulationProgressPhase,
    run_coarse_simulation, run_coarse_simulation_with_observer, run_seeded_tick_event_simulation,
    run_seeded_tick_event_simulation_with_observer,
};
use janiform_foundations::cancellation::CancellationToken;

use crate::error::WorkflowError;
use crate::progress::VolatileProgressGate;

use super::intervals::interval_duration_nanos_from_bounds;
use super::labels::{
    coarse_runtime_simulation_scope_label, direct_simulation_scope_label,
    model_artifact_simulation_scope_label, seeded_tick_runtime_simulation_scope_label,
};
use super::model_artifact::run_simulation_from_model_artifact;
use super::output::{DirectSimulationWorkflowOutput, ModelArtifactSimulationWorkflowOutput};
use super::progress::{
    cancel_simulation_progress, complete_simulation_progress, emit_completion_summary,
    emit_start_summary, observe_gated_simulation_terminal_event, start_simulation_progress,
};
use super::request::{
    CoarseSimulationWorkflowRequest, DirectSimulationWorkflowRequest,
    ModelArtifactSimulationWorkflowRequest, SeededTickEventSimulationWorkflowRequest,
};
use super::workflow::SimulationWorkflowObserver;

/// Run a simulation-from-model-artifact workflow.
pub fn run_simulation_from_model_artifact_workflow(
    request: ModelArtifactSimulationWorkflowRequest,
    mut observer: SimulationWorkflowObserver<'_>,
) -> Result<ModelArtifactSimulationWorkflowOutput, WorkflowError> {
    let cancellation_token = observer.cancellation_token.shared_handle();
    cancellation_token.check_cancelled()?;

    let scope_label = model_artifact_simulation_scope_label(request.simulation_kind);

    // Intentional copy because the workflow request is consumed by execution,
    // while completion rendering still needs the display output path.
    let output_path = request.output_path.clone();

    emit_start_summary(
        &observer.terminal_output,
        model_artifact_run_id(&request),
        scope_label,
        request.request_path.as_deref(),
        output_path.as_deref(),
    )?;

    start_simulation_progress(
        observer.progress_adapter.as_deref_mut(),
        model_artifact_run_id(&request),
        scope_label,
        1,
        output_path.as_deref(),
    )?;

    let mut progress_error = None::<WorkflowError>;
    let mut progress_gate = VolatileProgressGate::default();

    let output_result = {
        let mut progress_observer = |event: SimulationProgressEvent| {
            observe_gated_simulation_terminal_event(
                &mut observer.progress_adapter,
                &mut progress_gate,
                &mut progress_error,
                event,
            );
        };

        run_simulation_from_model_artifact(
            request,
            cancellation_token.shared_handle(),
            &mut progress_observer,
        )
    };

    let output = match output_result {
        Ok(output) => output,
        Err(WorkflowError::Cancelled { source }) => {
            cancel_simulation_progress(observer.progress_adapter, progress_error)?;
            return Err(WorkflowError::Cancelled { source });
        }
        Err(error) => return Err(error),
    };

    if let Err(source) = cancellation_token.check_cancelled() {
        cancel_simulation_progress(observer.progress_adapter, progress_error)?;
        return Err(WorkflowError::Cancelled { source });
    }

    complete_simulation_progress(observer.progress_adapter, progress_error)?;

    emit_completion_summary(
        &observer.terminal_output,
        scope_label,
        output_path.as_deref(),
        output.export_summary.as_ref(),
        output.execution_duration,
        output.completed.completion(),
    )?;

    Ok(output)
}

/// Run a direct simulation workflow.
pub fn run_direct_simulation_workflow(
    request: DirectSimulationWorkflowRequest,
    mut observer: SimulationWorkflowObserver<'_>,
) -> Result<DirectSimulationWorkflowOutput, WorkflowError> {
    let cancellation_token = observer.cancellation_token.shared_handle();

    cancellation_token.check_cancelled()?;

    let DirectSimulationWorkflowRequest {
        job,
        run_id,
        request_path,
        output_path,
    } = request;

    let scope_label = direct_simulation_scope_label(&job);

    emit_start_summary(
        &observer.terminal_output,
        run_id.as_str(),
        scope_label,
        request_path.as_deref(),
        output_path.as_deref(),
    )?;

    start_simulation_progress(
        observer.progress_adapter.as_deref_mut(),
        run_id.as_str(),
        scope_label,
        1,
        output_path.as_deref(),
    )?;

    let mut progress_error = None::<WorkflowError>;
    let mut progress_gate = VolatileProgressGate::default();

    let started_at = Instant::now();

    let completed_result = {
        let mut progress_observer = |event: SimulationProgressEvent| {
            observe_gated_simulation_terminal_event(
                &mut observer.progress_adapter,
                &mut progress_gate,
                &mut progress_error,
                event,
            );
        };

        job.run(cancellation_token.shared_handle(), &mut progress_observer)
    };

    let completed = match completed_result {
        Ok(completed) => completed,
        Err(WorkflowError::Cancelled { source }) => {
            cancel_simulation_progress(observer.progress_adapter, progress_error)?;
            return Err(WorkflowError::Cancelled { source });
        }
        Err(error) => return Err(error),
    };

    let execution_duration = started_at.elapsed();

    if let Err(source) = cancellation_token.check_cancelled() {
        cancel_simulation_progress(observer.progress_adapter, progress_error)?;
        return Err(WorkflowError::Cancelled { source });
    }

    let export_summary = completed.export(output_path.as_deref())?;
    if let Err(source) = cancellation_token.check_cancelled() {
        cancel_simulation_progress(observer.progress_adapter, progress_error)?;
        return Err(WorkflowError::Cancelled { source });
    }

    complete_simulation_progress(observer.progress_adapter, progress_error)?;

    emit_completion_summary(
        &observer.terminal_output,
        scope_label,
        output_path.as_deref(),
        export_summary.as_ref(),
        execution_duration,
        completed.completion(),
    )?;

    Ok(DirectSimulationWorkflowOutput {
        completed,
        export_summary,
        execution_duration,
    })
}

/// Run a coarse simulation workflow.
pub fn run_coarse_simulation_workflow(
    request: CoarseSimulationWorkflowRequest,
    observer: SimulationWorkflowObserver<'_>,
) -> Result<CoarseSimulationPath, WorkflowError> {
    run_direct_runtime_simulation_workflow(
        request.run_request,
        observer,
        coarse_runtime_simulation_scope_label(),
        run_coarse_simulation,
        run_coarse_simulation_with_observer,
    )
}

/// Run a seeded tick-event simulation workflow.
pub fn run_seeded_tick_event_simulation_workflow(
    request: SeededTickEventSimulationWorkflowRequest,
    observer: SimulationWorkflowObserver<'_>,
) -> Result<SeededTickSimulationRunResult, WorkflowError> {
    run_direct_runtime_simulation_workflow(
        request.run_config,
        observer,
        seeded_tick_runtime_simulation_scope_label(),
        run_seeded_tick_event_simulation,
        run_seeded_tick_event_simulation_with_observer,
    )
}

fn run_direct_runtime_simulation_workflow<TInput, TRunResult, TRunError, TRun, TRunObserved>(
    input: TInput,
    mut observer: SimulationWorkflowObserver<'_>,
    scope: &str,
    run: TRun,
    run_observed: TRunObserved,
) -> Result<TRunResult, WorkflowError>
where
    TRun: FnOnce(TInput, CancellationToken) -> Result<TRunResult, TRunError>,
    TRunObserved: FnOnce(
        TInput,
        CancellationToken,
        &mut dyn FnMut(SimulationProgressEvent),
    ) -> Result<TRunResult, TRunError>,
    TRunError: RuntimeSimulationRunError,
    TRunResult: DirectSimulationCompletionFields,
{
    let cancellation_token = observer.cancellation_token.shared_handle();

    cancellation_token.check_cancelled()?;

    emit_start_summary(&observer.terminal_output, "simulation", scope, None, None)?;

    start_simulation_progress(
        observer.progress_adapter.as_deref_mut(),
        "simulation",
        scope,
        1,
        None,
    )?;

    let mut progress_error = None::<WorkflowError>;
    let mut progress_gate = VolatileProgressGate::<SimulationProgressPhase>::default();

    let started_at = Instant::now();

    let run_result = if observer.progress_adapter.is_some() {
        let mut progress_observer = |event: SimulationProgressEvent| {
            observe_gated_simulation_terminal_event(
                &mut observer.progress_adapter,
                &mut progress_gate,
                &mut progress_error,
                event,
            );
        };

        run_observed(
            input,
            cancellation_token.shared_handle(),
            &mut progress_observer,
        )
    } else {
        run(input, cancellation_token.shared_handle())
    }
    .map_err(RuntimeSimulationRunError::into_workflow_error);

    let run_result = match run_result {
        Ok(run_result) => run_result,
        Err(WorkflowError::Cancelled { source }) => {
            cancel_simulation_progress(observer.progress_adapter, progress_error)?;
            return Err(WorkflowError::Cancelled { source });
        }
        Err(error) => return Err(error),
    };

    let execution_duration = started_at.elapsed();

    if let Err(source) = cancellation_token.check_cancelled() {
        cancel_simulation_progress(observer.progress_adapter, progress_error)?;
        return Err(WorkflowError::Cancelled { source });
    }

    complete_simulation_progress(observer.progress_adapter, progress_error)?;

    emit_completion_summary(
        &observer.terminal_output,
        scope,
        None,
        None,
        execution_duration,
        direct_completion_for_result(&run_result),
    )?;

    Ok(run_result)
}

trait RuntimeSimulationRunError: ToString {
    fn into_workflow_error(self) -> WorkflowError;
}

impl RuntimeSimulationRunError for SeededTickSimulationRunError {
    fn into_workflow_error(self) -> WorkflowError {
        match self {
            Self::Cancelled { source } => WorkflowError::Cancelled { source },
            source => WorkflowError::SimulationValidationFailed {
                field: "request",
                message: source.to_string(),
            },
        }
    }
}

impl RuntimeSimulationRunError for CoarseSimulationRunError {
    fn into_workflow_error(self) -> WorkflowError {
        match self {
            Self::Cancelled { source } => WorkflowError::Cancelled { source },
            source => WorkflowError::SimulationValidationFailed {
                field: "request",
                message: source.to_string(),
            },
        }
    }
}

fn model_artifact_run_id(request: &ModelArtifactSimulationWorkflowRequest) -> &str {
    request.run_id.as_deref().unwrap_or("simulation")
}

trait DirectSimulationCompletionFields {
    fn seed(&self) -> u64;

    fn simulated_interval_count(&self) -> usize;

    fn simulated_interval_duration_nanos(&self) -> Option<i64>;

    fn generated_tick_count(&self) -> Option<usize>;
}

impl DirectSimulationCompletionFields for CoarseSimulationPath {
    fn seed(&self) -> u64 {
        self.seed
    }

    fn simulated_interval_count(&self) -> usize {
        self.simulated_interval_count
    }

    fn simulated_interval_duration_nanos(&self) -> Option<i64> {
        interval_duration_nanos_from_bounds(
            self.start_time_utc_nanos,
            self.end_time_utc_nanos,
            self.simulated_interval_count,
        )
    }

    fn generated_tick_count(&self) -> Option<usize> {
        None
    }
}

impl DirectSimulationCompletionFields for SeededTickSimulationRunResult {
    fn seed(&self) -> u64 {
        self.seed
    }

    fn simulated_interval_count(&self) -> usize {
        self.simulated_interval_count
    }

    fn simulated_interval_duration_nanos(&self) -> Option<i64> {
        interval_duration_nanos_from_bounds(
            self.start_time_utc.timestamp_nanos_opt()?,
            self.end_time_utc.timestamp_nanos_opt()?,
            self.simulated_interval_count,
        )
    }

    fn generated_tick_count(&self) -> Option<usize> {
        Some(self.event_count())
    }
}

fn direct_completion_for_result(
    result: &impl DirectSimulationCompletionFields,
) -> super::DirectSimulationCompletion {
    super::DirectSimulationCompletion {
        seed: result.seed(),
        simulated_interval_count: result.simulated_interval_count(),
        simulated_interval_duration_nanos: result.simulated_interval_duration_nanos(),
        generated_tick_count: result.generated_tick_count(),
    }
}
