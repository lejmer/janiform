use crate::error::WorkflowError;

pub(super) fn validation_error(field: &'static str, message: impl Into<String>) -> WorkflowError {
    WorkflowError::SimulationValidationFailed {
        field,
        message: message.into(),
    }
}
