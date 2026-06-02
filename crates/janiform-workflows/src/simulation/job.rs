use std::time::Instant;

use janiform::simulation::SimulationProgressEvent;
use janiform_foundations::cancellation::CancellationToken;

use crate::error::WorkflowError;

use super::output::SimulationWorkflowOutput;
use super::request::SimulationWorkflowRequest;

pub(super) fn run_simulation_job(
    request: SimulationWorkflowRequest,
    cancellation_token: CancellationToken,
    observer: &mut dyn FnMut(SimulationProgressEvent),
    started_at: Instant,
) -> Result<SimulationWorkflowOutput, WorkflowError> {
    cancellation_token.check_cancelled()?;

    let SimulationWorkflowRequest {
        metadata,
        job,
        export_plan,
    } = request;

    let completed = job.run(cancellation_token.shared_handle(), observer)?;

    cancellation_token.check_cancelled()?;

    let export_summary = completed.export(metadata.output_path.as_deref(), export_plan.as_ref())?;

    cancellation_token.check_cancelled()?;

    Ok(SimulationWorkflowOutput {
        completed,
        export_summary,
        execution_duration: started_at.elapsed(),
    })
}
