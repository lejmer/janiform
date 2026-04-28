use std::time::Instant;

use janiform::simulation::SimulationProgressEvent;
use serde_json::{Value, json};

use crate::error::WorkflowError;
use crate::simulation::direct::DirectSimulationCompleted;

use super::context;
use super::factors;
use super::job;
use super::state;
use super::support::validation_error;
use super::{ModelArtifactSimulationWorkflowOutput, ModelArtifactSimulationWorkflowRequest};

/// Run a simulation workflow from a persisted model artifact payload.
pub fn run_simulation_from_model_artifact(
    request: ModelArtifactSimulationWorkflowRequest,
    observer: &mut dyn FnMut(SimulationProgressEvent),
) -> Result<ModelArtifactSimulationWorkflowOutput, WorkflowError> {
    validate_request(&request)?;

    let started_at = Instant::now();

    let context = context::runtime_context(&request)?;
    let hourly_factor_contract = factors::build_hourly_factor_contract(&request, &context)?;
    let initial_state = state::resolve_initial_state(&request, &context)?;

    let job = job::build_direct_job(&request, &context, hourly_factor_contract, initial_state)?;
    let completed = job.run(observer)?;

    let export_summary = completed.export(request.output_path.as_deref())?;

    Ok(ModelArtifactSimulationWorkflowOutput {
        completed,
        export_summary,
        execution_duration: started_at.elapsed(),
    })
}

fn validate_request(request: &ModelArtifactSimulationWorkflowRequest) -> Result<(), WorkflowError> {
    if request.horizon_interval_count == 0 {
        return Err(validation_error(
            "horizon_interval_count",
            "must be a positive integer",
        ));
    }

    if request.interval_duration_nanos <= 0 {
        return Err(validation_error(
            "interval_duration_nanos",
            "must be a positive integer number of nanoseconds",
        ));
    }

    if !request.reference_price.is_finite() || request.reference_price <= 0.0 {
        return Err(validation_error(
            "reference_price",
            "must be finite and strictly positive",
        ));
    }

    if request.export_batch_size == 0 {
        return Err(validation_error("export_batch_size", "must be positive"));
    }

    Ok(())
}

impl ModelArtifactSimulationWorkflowOutput {
    /// Serialize the public workflow metadata fields.
    pub fn response_payload(&self) -> Result<Value, WorkflowError> {
        let completion = self.completed.completion();

        let simulation_kind = match &self.completed {
            DirectSimulationCompleted::SeededTick { .. } => "seeded_tick",
            DirectSimulationCompleted::CoarseInterval { .. } => "coarse_interval",
        };

        let export_summary = match &self.export_summary {
            Some(summary) => Some(json!({
                "record_count": summary.record_count,
                "chunk_count": summary.chunk_count,
            })),
            None => None,
        };

        Ok(json!({
            "simulation_kind": simulation_kind,
            "completion": {
                "seed": completion.seed,
                "simulated_interval_count": completion.simulated_interval_count,
            },
            "export_summary": export_summary,
            "execution_duration_seconds": self.execution_duration.as_secs_f64(),
        }))
    }
}
