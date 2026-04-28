use crate::error::WorkflowError;
use crate::model::{FittedModelContext, fitted_model_context};

use super::ModelSimulationWorkflowRequest;

pub(super) use crate::model::FittedModelContext as ModelRuntimeContext;

pub(super) fn runtime_context(
    request: &ModelSimulationWorkflowRequest,
) -> Result<FittedModelContext, WorkflowError> {
    fitted_model_context(
        &request.model_payload,
        request.start_time_utc,
        request.interval_duration_nanos,
        request.horizon_interval_count,
    )
}
